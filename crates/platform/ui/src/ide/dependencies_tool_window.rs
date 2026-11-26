use std::path::{Path, PathBuf};

use egui::{Align2, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::connection_dialog::input;
use super::form::combo;
use super::glyphs::*;
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const ID: &str = "dependencies";
const READ_EVERY: f64 = 2.0;
const POLL_EVERY: f64 = 0.3;
const CHECK_EVERY: f64 = 1.0;
const TYPED_AFTER: f64 = 0.3;
const MODULES_WIDTH: f32 = 180.0;
const DETAIL_HEIGHT: f32 = 92.0;
const SHOWN_VERSIONS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    Plain,
    Outdated,
    Vulnerable,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DependencyRow {
    pub(super) id: String,
    pub(super) group: String,
    pub(super) artifact: String,
    pub(super) version: String,
    pub(super) scope: String,
    pub(super) note: String,
    pub(super) mark: Mark,
    pub(super) value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Adding {
    group: String,
    artifact: String,
    version: String,
    scope: String,
    versions: Vec<String>,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().map(text).collect()
}

pub(super) fn dependency_rows(module: &Value) -> Vec<DependencyRow> {
    let mut rows = Vec::new();
    for dependency in module["dependencies"].as_array().into_iter().flatten() {
        let status = &dependency["status"];
        let count = status["advisories"].as_array().map_or(0, Vec::len);
        let newest = status["newer"].get(0).and_then(Value::as_str).map(str::to_owned);
        let (mark, note) = match (count, newest) {
            (0, None) => (Mark::Plain, String::new()),
            (0, Some(newest)) => (Mark::Outdated, t!("ide-deps-newer", version = newest.as_str())),
            (count, _) => (Mark::Vulnerable, t!("ide-deps-vulnerable", count = count)),
        };
        let (group, artifact, scope) =
            (text(&dependency["group"]), text(&dependency["artifact"]), text(&dependency["scope"]));
        rows.push(DependencyRow {
            id: format!("{group}:{artifact}:{scope}"),
            version: text(&dependency["version"]),
            group,
            artifact,
            scope,
            note,
            mark,
            value: dependency.clone(),
        });
    }
    rows
}

fn place_note(place: &Value) -> Option<String> {
    let by = place["by"].as_str().unwrap_or_default();
    match place["kind"].as_str()? {
        "managed" => Some(t!("ide-deps-managed", by = by)),
        "managedHere" => Some(t!("ide-deps-managed-here", by = by)),
        _ => None,
    }
}

fn changes(place: &Value) -> bool {
    !matches!(place["kind"].as_str(), Some("managed" | "unknown"))
}

#[derive(Default)]
pub(super) struct DependenciesToolWindow {
    checked: Option<(PathBuf, u64, f64)>,
    list: Option<Value>,
    error: Option<String>,
    read: f64,
    module: Option<String>,
    selected: Option<String>,
    target: Option<String>,
    query: String,
    typed_at: f64,
    search: Option<Value>,
    searched_at: f64,
    adding: Option<Adding>,
}

impl DependenciesToolWindow {
    fn stale(&mut self) {
        self.read = f64::NEG_INFINITY;
    }

    #[cfg(test)]
    pub(super) fn force_read(&mut self) {
        self.stale();
        self.searched_at = f64::NEG_INFINITY;
    }

    #[cfg(test)]
    pub(super) fn selected_row(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    fn pending(&self) -> bool {
        self.list.as_ref().is_some_and(|list| list["pending"] == true)
            || self.search.as_ref().is_some_and(|search| search["pending"] == true)
    }

    fn read(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        let every = if self.pending() { POLL_EVERY } else { READ_EVERY };
        if now - self.read < every {
            return;
        }
        self.read = now;
        match registry.dispatch(session, "dependencies.project.list", json!({})) {
            Ok(list) => {
                self.list = Some(list);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        let modules = self.modules();
        let known = self.module.as_ref().is_some_and(|id| modules.iter().any(|module| module["id"] == id.as_str()));
        if !known {
            self.module = modules.first().map(|module| text(&module["id"]));
        }
    }

    fn poll_search(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        let query = self.query.trim().to_owned();
        if query.is_empty() {
            self.search = None;
            return;
        }
        let same = self.search.as_ref().is_some_and(|search| search["query"] == query.as_str());
        let waiting = self.search.as_ref().is_some_and(|search| search["pending"] == true);
        let ask =
            (!same && now - self.typed_at >= TYPED_AFTER) || (same && waiting && now - self.searched_at >= POLL_EVERY);
        if !ask {
            return;
        }
        self.searched_at = now;
        let params = json!({ "query": query, "module": self.module });
        match registry.dispatch(session, "dependencies.repository.search", params) {
            Ok(answer) => {
                self.search = Some(answer);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn modules(&self) -> Vec<Value> {
        self.list.as_ref().and_then(|list| list["modules"].as_array().cloned()).unwrap_or_default()
    }

    fn current(&self) -> Option<Value> {
        let id = self.module.as_ref()?;
        self.modules().into_iter().find(|module| module["id"] == id.as_str())
    }

    fn rows(&self) -> Vec<DependencyRow> {
        self.current().map(|module| dependency_rows(&module)).unwrap_or_default()
    }

    fn choose_result(&mut self, result: &Value, scope: String) {
        let versions = strings(&result["versions"]);
        let version = result["latest"].as_str().map(str::to_owned).or_else(|| versions.first().cloned());
        self.adding = Some(Adding {
            group: text(&result["group"]),
            artifact: text(&result["artifact"]),
            version: version.unwrap_or_default(),
            scope,
            versions,
        });
    }

    pub(super) fn state(&self, available: bool, shown: bool) -> Value {
        let mut state = json!({ "available": available, "module": self.module, "selected": self.selected });
        if available && shown {
            state["modules"] = json!(
                self.modules()
                    .iter()
                    .map(|module| json!({ "id": module["id"], "name": module["name"] }))
                    .collect::<Vec<_>>()
            );
            state["rows"] = json!(
                self.rows()
                    .iter()
                    .map(|row| json!({ "id": row.id, "version": row.version, "scope": row.scope, "note": row.note }))
                    .collect::<Vec<_>>()
            );
            state["query"] = json!(self.query);
            if let Some(search) = &self.search {
                state["results"] = search["results"].clone();
            }
            if let Some(adding) = &self.adding {
                state["adding"] = json!(format!("{}:{}", adding.group, adding.artifact));
            }
        }
        state
    }
}

fn column_x(rect: Rect, share: f32) -> f32 {
    rect.left() + 12.0 + (rect.width() - 24.0) * share
}

const COLUMNS: [f32; 5] = [0.0, 0.3, 0.55, 0.68, 0.8];

impl IdeView {
    pub(super) fn follow_dependencies(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        if self.build_tools.dependencies && self.tool_visible(ID) {
            self.dependencies.read(registry, session, now);
            self.dependencies.poll_search(registry, session, now);
        }
        let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
            self.build_tools.dependencies = false;
            return;
        };
        let generation = self.plugins.generation();
        let fresh = self.dependencies.checked.as_ref().is_some_and(|(seen, at_generation, at)| {
            *seen == root && *at_generation == generation && now - at < CHECK_EVERY
        });
        if fresh {
            return;
        }
        self.dependencies.checked = Some((root.clone(), generation, now));
        self.build_tools.dependencies = zen_workbench::dependencies::offered(session, &root);
        if !self.build_tools.dependencies && self.tool_visible(ID) {
            self.hide_tool(ID);
        }
    }

    pub(super) fn toggle_dependencies(&mut self, registry: &Registry, session: &mut Session) -> Result<(), String> {
        if !self.build_tools.dependencies {
            return Err(t!("ide-deps-not-available"));
        }
        self.toggle_tool(ID);
        if self.tool_visible(ID) {
            self.dependencies.stale();
            self.dependencies.read(registry, session, f64::MAX);
            self.dependencies.stale();
        }
        Ok(())
    }

    pub(super) fn dependencies_state(&self) -> Value {
        self.dependencies.state(self.build_tools.dependencies, self.tool_visible(ID))
    }

    pub(super) fn dependencies_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, String> {
        if !self.build_tools.dependencies {
            return Err(t!("ide-deps-not-available"));
        }
        if !self.tool_visible(ID) {
            self.show_tool(ID);
        }
        self.dependencies.stale();
        self.dependencies.read(registry, session, f64::MAX);
        self.dependencies.stale();
        if action == "view.dependencies.search" {
            self.dependencies.query = params["query"].as_str().unwrap_or_default().to_owned();
            self.dependencies.typed_at = f64::NEG_INFINITY;
            self.dependencies.poll_search(registry, session, f64::MAX);
            return Ok(json!({ "query": self.dependencies.query }));
        }
        if let Some(module) = params["module"].as_str() {
            if !self.dependencies.modules().iter().any(|known| known["id"] == module) {
                return Err(format!("no module `{module}` in the Dependencies tool window"));
            }
            self.dependencies.module = Some(module.to_owned());
        }
        if let Some(row) = params["row"].as_str() {
            if !self.dependencies.rows().iter().any(|known| known.id == row) {
                return Err(format!("no row `{row}` in the Dependencies tool window"));
            }
            self.dependencies.selected = Some(row.to_owned());
            self.dependencies.target = None;
        }
        Ok(json!({ "module": self.dependencies.module, "selected": self.dependencies.selected }))
    }

    pub(super) fn dependencies_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        if self.dependencies.pending() {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(POLL_EVERY));
        }
        let mut actions: Vec<(&'static str, Value)> = Vec::new();
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-deps-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("deps-tool-title"), title, || t!("ide-deps-title"));
        let refresh = Rect::from_center_size(pos2(title.right() + 26.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(refresh, ui.scope_id().with("deps-refresh"), Sense::click());
        hover(ui, &response, &p);
        glyph_refresh(ui.painter(), refresh.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-deps-refresh"), None, Side::Below);
        accessibility::button(&response, &t!("ide-deps-refresh"));
        if response.clicked() {
            actions.push(("dependencies.project.refresh", json!({})));
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("deps-tool-hide"), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        accessibility::button(&response, &t!("ide-run-hide"));
        if response.clicked() {
            self.hide_tool(ID);
        }
        let list = self.dependencies.list.clone().unwrap_or_default();
        let message = if let Some(error) = &self.dependencies.error {
            Some((error.clone(), p.error_text))
        } else if list["network"] == false && !list.is_null() {
            Some((t!("ide-deps-network-off"), p.text_secondary))
        } else if list["failed"] == true {
            Some((t!("ide-deps-offline"), p.warning_text))
        } else {
            None
        };
        if let Some((message, color)) = message {
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            ui.painter().with_clip_rect(line).text(
                pos2(line.left() + 12.0, line.center().y),
                Align2::LEFT_CENTER,
                message,
                theme::regular(12.0),
                color,
            );
        }
        let (body, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let width = MODULES_WIDTH.min(body.width() * 0.4);
        let left = Rect::from_min_max(body.min, pos2(body.left() + width, body.bottom()));
        let right = Rect::from_min_max(pos2(left.right() + 1.0, body.top()), body.max);
        ui.painter().vline(left.right(), body.y_range(), egui::Stroke::new(1.0, p.gutter_separator));
        ui.scope_builder(UiBuilder::new().max_rect(left), |ui| self.dependency_modules(ui, &p));
        ui.scope_builder(UiBuilder::new().max_rect(right), |ui| self.dependency_pane(ui, &p, &mut actions));
        let acted = !actions.is_empty();
        let added = actions.iter().any(|(command, _)| *command == "dependencies.module.add");
        for (command, params) in actions {
            match registry.dispatch(session, command, params) {
                Ok(answer) => {
                    if let Some(reload) = answer["reloadError"].as_str() {
                        self.post(ui, reload.to_owned());
                    }
                }
                Err(error) => self.post(ui, error.to_string()),
            }
        }
        if added {
            self.dependencies.query.clear();
            self.dependencies.search = None;
        }
        if acted {
            self.dependencies.stale();
            self.dependencies.adding = None;
            self.dependencies.target = None;
        }
    }

    fn dependency_modules(&mut self, ui: &mut Ui, p: &IdePalette) {
        let modules = self.dependencies.modules();
        egui::ScrollArea::vertical().id_salt("deps-modules").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for module in &modules {
                let id = text(&module["id"]);
                let name = text(&module["name"]);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let selected = self.dependencies.module.as_deref() == Some(id.as_str());
                accessibility::tree_item(&response, 1, None, selected, &name);
                let shape = rect.shrink2(vec2(6.0, 0.0));
                if selected {
                    ui.painter().rect_filled(shape, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(shape, 5, p.hover);
                }
                ui.painter().with_clip_rect(shape).text(
                    pos2(shape.left() + 8.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &name,
                    theme::regular(13.0),
                    p.text,
                );
                if response.clicked() {
                    self.dependencies.module = Some(id);
                    self.dependencies.selected = None;
                    self.dependencies.target = None;
                }
            }
            if modules.is_empty() {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(t!("ide-deps-no-modules")).color(p.text_secondary).font(theme::regular(12.5)),
                    );
                });
            }
        });
    }

    fn dependency_pane(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<(&'static str, Value)>) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let edit = egui::TextEdit::singleline(&mut self.dependencies.query)
                .hint_text(t!("ide-deps-search"))
                .font(theme::regular(13.0))
                .desired_width((ui.available_width() - 24.0).max(40.0));
            let response = input(ui, p, edit);
            accessibility::name(&response, egui::Role::TextInput, &t!("ide-deps-search"));
            if response.changed() {
                self.dependencies.typed_at = ui.input(|input| input.time);
                self.dependencies.adding = None;
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(TYPED_AFTER));
            }
        });
        ui.add_space(4.0);
        let area = ui.available_rect_before_wrap();
        let detail_shown = if self.dependencies.query.trim().is_empty() {
            self.dependencies.selected.is_some()
        } else {
            self.dependencies.adding.is_some()
        };
        let detail = detail_shown
            .then(|| Rect::from_min_max(pos2(area.left(), (area.bottom() - DETAIL_HEIGHT).max(area.top())), area.max));
        let list = Rect::from_min_max(area.min, pos2(area.right(), detail.map_or(area.bottom(), |rect| rect.top())));
        if self.dependencies.query.trim().is_empty() {
            ui.scope_builder(UiBuilder::new().max_rect(list), |ui| self.dependency_table(ui, p));
            if let Some(rect) = detail {
                ui.painter().hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0, p.gutter_separator));
                ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(12.0, 6.0))), |ui| {
                    self.dependency_detail(ui, p, actions);
                });
            }
        } else {
            ui.scope_builder(UiBuilder::new().max_rect(list), |ui| self.dependency_results(ui, p));
            if let Some(rect) = detail {
                ui.painter().hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0, p.gutter_separator));
                ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(12.0, 6.0))), |ui| {
                    self.dependency_add_form(ui, p, actions);
                });
            }
        }
    }

    fn dependency_table(&mut self, ui: &mut Ui, p: &IdePalette) {
        let rows = self.dependencies.rows();
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        for (share, title) in COLUMNS.iter().zip([
            t!("ide-deps-group"),
            t!("ide-deps-artifact"),
            t!("ide-deps-version"),
            t!("ide-deps-scope"),
        ]) {
            ui.painter().text(
                pos2(column_x(head, *share), head.center().y),
                Align2::LEFT_CENTER,
                title,
                theme::regular(11.5),
                p.text_secondary,
            );
        }
        egui::ScrollArea::vertical().id_salt("deps-table").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let selected = self.dependencies.selected.as_deref() == Some(row.id.as_str());
                let name = format!("{}:{} {}", row.group, row.artifact, row.version);
                accessibility::tree_item(&response, 1, None, selected, &name);
                let shape = rect.shrink2(vec2(6.0, 0.0));
                if selected {
                    ui.painter().rect_filled(shape, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(shape, 5, p.hover);
                }
                let version_color = match row.mark {
                    Mark::Vulnerable => p.error_text,
                    Mark::Outdated => p.warning_text,
                    Mark::Plain => p.text,
                };
                let cells: [(&str, egui::Color32); 5] = [
                    (&row.group, p.text_secondary),
                    (&row.artifact, p.text),
                    (&row.version, version_color),
                    (&row.scope, p.text_secondary),
                    (&row.note, version_color),
                ];
                for (index, (share, (cell, color))) in COLUMNS.iter().zip(cells).enumerate() {
                    let x = column_x(rect, *share);
                    let end = COLUMNS.get(index + 1).map_or(rect.right(), |next| column_x(rect, *next) - 6.0);
                    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(end, rect.bottom()));
                    ui.painter().with_clip_rect(clip).text(
                        pos2(x, rect.center().y),
                        Align2::LEFT_CENTER,
                        cell,
                        theme::regular(12.5),
                        color,
                    );
                }
                if response.clicked() {
                    self.dependencies.selected = Some(row.id.clone());
                    self.dependencies.target = None;
                }
            }
            if rows.is_empty() {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(t!("ide-deps-empty")).color(p.text_secondary).font(theme::regular(12.5)));
                });
            }
        });
    }

    fn dependency_detail(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<(&'static str, Value)>) {
        let rows = self.dependencies.rows();
        let Some(module) = self.dependencies.module.clone() else { return };
        let Some(row) = rows.iter().find(|row| Some(&row.id) == self.dependencies.selected.as_ref()).cloned() else {
            return;
        };
        let status = &row.value["status"];
        let place = &row.value["versionPlace"];
        let newer = strings(&status["newer"]);
        let safe = status["safe"].as_str().map(str::to_owned);
        let advisories = status["advisories"].as_array().cloned().unwrap_or_default();
        let target = self.dependencies.target.clone().or_else(|| newer.first().cloned());
        let coordinates = json!({ "module": module, "group": row.group, "artifact": row.artifact });
        let upgrade = |version: &str| {
            let mut params = coordinates.clone();
            params["version"] = json!(version);
            ("dependencies.version.set", params)
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{}:{}", row.group, row.artifact)).font(theme::semibold(12.5)).color(p.text),
            );
            if let (true, Some(target)) = (changes(place), &target) {
                combo(ui, p, "deps-upgrade-version", &t!("ide-deps-version"), target, 130.0, |ui| {
                    for version in newer.iter().take(SHOWN_VERSIONS) {
                        let label = RichText::new(version).font(theme::regular(13.0));
                        if ui.selectable_label(version == target, label).clicked() {
                            self.dependencies.target = Some(version.clone());
                        }
                    }
                });
                if ui.button(t!("ide-deps-upgrade")).clicked() {
                    actions.push(upgrade(target));
                }
            }
            if ui.button(t!("ide-deps-remove")).clicked() {
                let mut params = coordinates.clone();
                params["scope"] = json!(row.scope);
                actions.push(("dependencies.module.remove", params));
                self.dependencies.selected = None;
            }
        });
        if let Some(note) = place_note(place) {
            ui.label(RichText::new(note).font(theme::regular(12.0)).color(p.text_secondary));
        } else if newer.is_empty() && advisories.is_empty() && status["latest"].is_string() {
            ui.label(RichText::new(t!("ide-deps-up-to-date")).font(theme::regular(12.0)).color(p.text_secondary));
        }
        for advisory in advisories.iter().take(2) {
            let line = format!("{}  {}", text(&advisory["id"]), text(&advisory["summary"]));
            ui.add(egui::Label::new(RichText::new(line).font(theme::regular(12.0)).color(p.error_text)).truncate());
        }
        if !advisories.is_empty() {
            match safe.filter(|_| changes(place)) {
                Some(safe) => {
                    if ui.button(t!("ide-deps-upgrade-to", version = safe.as_str())).clicked() {
                        actions.push(upgrade(&safe));
                    }
                }
                None => {
                    ui.label(RichText::new(t!("ide-deps-no-safe")).font(theme::regular(12.0)).color(p.text_secondary));
                }
            }
        }
    }

    fn dependency_results(&mut self, ui: &mut Ui, p: &IdePalette) {
        let search = self.dependencies.search.clone().unwrap_or_default();
        let results = search["results"].as_array().cloned().unwrap_or_default();
        let scope = self
            .dependencies
            .current()
            .and_then(|module| module["scopes"].get(0).and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default();
        egui::ScrollArea::vertical().id_salt("deps-results").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for result in &results {
                let name = format!("{}:{}", text(&result["group"]), text(&result["artifact"]));
                let latest = text(&result["latest"]);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let selected = self
                    .dependencies
                    .adding
                    .as_ref()
                    .is_some_and(|adding| format!("{}:{}", adding.group, adding.artifact) == name);
                accessibility::tree_item(&response, 1, None, selected, &format!("{name} {latest}"));
                let shape = rect.shrink2(vec2(6.0, 0.0));
                if selected {
                    ui.painter().rect_filled(shape, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(shape, 5, p.hover);
                }
                let split = column_x(rect, 0.75);
                ui.painter().with_clip_rect(Rect::from_min_max(rect.min, pos2(split - 6.0, rect.bottom()))).text(
                    pos2(shape.left() + 8.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &name,
                    theme::regular(12.5),
                    p.text,
                );
                ui.painter().with_clip_rect(rect).text(
                    pos2(split, rect.center().y),
                    Align2::LEFT_CENTER,
                    &latest,
                    theme::regular(12.5),
                    p.text_secondary,
                );
                if response.clicked() {
                    self.dependencies.choose_result(result, scope.clone());
                }
            }
            let state = if search["disabled"] == true {
                Some(t!("ide-deps-network-off"))
            } else if search["failed"] == true && results.is_empty() {
                Some(t!("ide-deps-offline"))
            } else if search.is_null() || search["pending"] == true {
                Some(t!("ide-deps-searching"))
            } else if results.is_empty() {
                Some(t!("ide-deps-nothing-found"))
            } else {
                None
            };
            if let Some(state) = state {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(state).color(p.text_secondary).font(theme::regular(12.5)));
                });
            }
        });
    }

    fn dependency_add_form(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<(&'static str, Value)>) {
        let Some(mut adding) = self.dependencies.adding.clone() else { return };
        let module = self.dependencies.current();
        let scopes = module.as_ref().map(|module| strings(&module["scopes"])).unwrap_or_default();
        ui.label(
            RichText::new(format!("{}:{}", adding.group, adding.artifact)).font(theme::semibold(12.5)).color(p.text),
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("ide-deps-version")).font(theme::regular(12.5)).color(p.text_secondary));
            let shown = adding.version.clone();
            combo(ui, p, "deps-add-version", &t!("ide-deps-version"), &shown, 130.0, |ui| {
                for version in adding.versions.clone().iter().take(SHOWN_VERSIONS) {
                    let label = RichText::new(version).font(theme::regular(13.0));
                    if ui.selectable_label(*version == shown, label).clicked() {
                        adding.version = version.clone();
                    }
                }
            });
            ui.label(RichText::new(t!("ide-deps-scope")).font(theme::regular(12.5)).color(p.text_secondary));
            let current = adding.scope.clone();
            combo(ui, p, "deps-add-scope", &t!("ide-deps-scope"), &current, 160.0, |ui| {
                for scope in &scopes {
                    let label = RichText::new(scope).font(theme::regular(13.0));
                    if ui.selectable_label(*scope == current, label).clicked() {
                        adding.scope = scope.clone();
                    }
                }
            });
            if ui.button(t!("ide-deps-add")).clicked()
                && let Some(module) = &self.dependencies.module
            {
                actions.push((
                    "dependencies.module.add",
                    json!({
                        "module": module, "group": adding.group, "artifact": adding.artifact,
                        "version": adding.version, "scope": adding.scope,
                    }),
                ));
            }
        });
        self.dependencies.adding = Some(adding);
    }
}

#[cfg(test)]
mod tests;
