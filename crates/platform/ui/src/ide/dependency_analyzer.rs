use std::collections::BTreeSet;

use egui::{Align2, Rect, RichText, Role, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::{IdePalette, IdeView, metrics};
use crate::{accessibility, theme};

pub(super) const OPEN: &str = "view.dependencyAnalyzer.open";
pub(super) const SET: &str = "view.dependencyAnalyzer.set";
pub(super) const SELECT: &str = "view.dependencyAnalyzer.select";
pub(super) const TOGGLE: &str = "view.dependencyAnalyzer.toggle";
pub(super) const REFRESH: &str = "view.dependencyAnalyzer.refresh";

const ANALYZE: &str = "build.dependencies.analyze";
const DECLARATION: &str = "build.dependencies.declaration";
const EXCLUDE: &str = "build.dependencies.exclude";
const CHECK_EVERY: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    Plain,
    Conflict,
    Omitted,
    Failed,
}

impl Mark {
    fn name(self) -> &'static str {
        match self {
            Mark::Plain => "plain",
            Mark::Conflict => "conflict",
            Mark::Omitted => "omitted",
            Mark::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) id: String,
    pub(super) depth: usize,
    pub(super) label: String,
    pub(super) version: String,
    pub(super) scope: String,
    pub(super) mark: Mark,
    pub(super) parent: bool,
    pub(super) open: bool,
    pub(super) group: String,
    pub(super) artifact: String,
    pub(super) via: Option<String>,
    pub(super) excludable: bool,
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn version_shown(requested: &str, selected: &str) -> String {
    if requested.is_empty() || requested == selected {
        selected.to_owned()
    } else {
        t!("ide-dependency-version-changed", requested = requested, selected = selected)
    }
}

fn mark_of(value: &Value) -> Mark {
    if value["reason"] == "failed" {
        Mark::Failed
    } else if value["conflict"] == true {
        Mark::Conflict
    } else if value["omitted"] == true {
        Mark::Omitted
    } else {
        Mark::Plain
    }
}

pub(super) fn list_rows(analysis: &Value) -> Vec<Row> {
    let entries = analysis["dependencies"].as_array().map_or(&[][..], Vec::as_slice);
    entries
        .iter()
        .map(|entry| {
            let name = format!("{}:{}", text(&entry["groupId"]), text(&entry["artifactId"]));
            let usages = entry["usages"].as_array().map_or(&[][..], Vec::as_slice);
            let scopes = entry["scopes"].as_array().map_or(&[][..], Vec::as_slice);
            let scopes: Vec<&str> = scopes.iter().map(text).collect();
            Row {
                id: match text(&entry["classifier"]) {
                    "" => name.clone(),
                    classifier => format!("{name}:{classifier}"),
                },
                depth: 0,
                label: name,
                version: text(&entry["version"]).to_owned(),
                scope: scopes.join(", "),
                mark: mark_of(entry),
                parent: false,
                open: false,
                group: text(&entry["groupId"]).to_owned(),
                artifact: text(&entry["artifactId"]).to_owned(),
                via: None,
                excludable: usages.iter().any(|usage| usage["path"].as_array().is_some_and(|path| path.len() > 1)),
            }
        })
        .collect()
}

struct TreeRows<'a> {
    rows: Vec<Row>,
    flipped: &'a BTreeSet<String>,
    opened: bool,
}

impl TreeRows<'_> {
    fn nodes(&mut self, nodes: &[Value], depth: usize, parent: &str, via: Option<&str>) {
        for node in nodes {
            let name = format!("{}:{}", text(&node["groupId"]), text(&node["artifactId"]));
            let id = format!("{parent}/{name}");
            let children = node["children"].as_array().map_or(&[][..], Vec::as_slice);
            let open = !children.is_empty() && self.opened != self.flipped.contains(&id);
            self.rows.push(Row {
                id: id.clone(),
                depth,
                label: name.clone(),
                version: version_shown(text(&node["requestedVersion"]), text(&node["selectedVersion"])),
                scope: text(&node["scope"]).to_owned(),
                mark: mark_of(node),
                parent: !children.is_empty(),
                open,
                group: text(&node["groupId"]).to_owned(),
                artifact: text(&node["artifactId"]).to_owned(),
                via: via.map(str::to_owned),
                excludable: via.is_some(),
            });
            if open {
                self.nodes(children, depth + 1, &id, Some(via.unwrap_or(&name)));
            }
        }
    }
}

pub(super) fn tree_rows(analysis: &Value, flipped: &BTreeSet<String>, filtered: bool) -> Vec<Row> {
    let mut rows = TreeRows { rows: Vec::new(), flipped, opened: filtered };
    let nodes = analysis["tree"].as_array().map_or(&[][..], Vec::as_slice);
    if analysis["byConfiguration"] != true {
        rows.nodes(nodes, 0, "", None);
        return rows.rows;
    }
    let mut scopes: Vec<&str> = Vec::new();
    for node in nodes {
        if !scopes.contains(&text(&node["scope"])) {
            scopes.push(text(&node["scope"]));
        }
    }
    for scope in scopes {
        let id = format!("scope/{scope}");
        let open = !flipped.contains(&id);
        rows.rows.push(Row {
            id: id.clone(),
            depth: 0,
            label: scope.to_owned(),
            version: String::new(),
            scope: String::new(),
            mark: Mark::Plain,
            parent: true,
            open,
            group: String::new(),
            artifact: String::new(),
            via: None,
            excludable: false,
        });
        if open {
            let own: Vec<Value> = nodes.iter().filter(|node| text(&node["scope"]) == scope).cloned().collect();
            rows.nodes(&own, 1, &id, None);
        }
    }
    rows.rows
}

fn reason_text(system: &str, reason: &str, version: &str) -> Option<String> {
    Some(match (reason, system) {
        ("conflict", "gradle") => t!("ide-dependency-reason-conflict-gradle", version = version),
        ("conflict", _) => t!("ide-dependency-reason-conflict-maven", version = version),
        ("managed", _) => t!("ide-dependency-reason-managed", version = version),
        ("forced", _) => t!("ide-dependency-reason-forced", version = version),
        ("constraint", _) => t!("ide-dependency-reason-constraint", version = version),
        ("rule", _) => t!("ide-dependency-reason-rule", version = version),
        ("duplicate", _) => t!("ide-dependency-reason-duplicate"),
        ("cycle", _) => t!("ide-dependency-reason-cycle"),
        ("failed", _) => t!("ide-dependency-reason-failed"),
        _ => return None,
    })
}

pub(super) fn details(analysis: &Value, row: &Row) -> Vec<String> {
    let entries = analysis["dependencies"].as_array().map_or(&[][..], Vec::as_slice);
    let Some(entry) = entries.iter().find(|entry| entry["groupId"] == row.group && entry["artifactId"] == row.artifact)
    else {
        return Vec::new();
    };
    let version = text(&entry["version"]);
    let mut lines = vec![format!("{}:{version}", row.label)];
    let system = text(&analysis["system"]);
    lines.extend(reason_text(system, text(&entry["reason"]), version));
    if let Some(requested) = entry["requested"].as_array().filter(|requested| requested.len() > 1) {
        let requested: Vec<&str> = requested.iter().map(text).collect();
        lines.push(t!("ide-dependency-requested", versions = requested.join(", ")));
    }
    for usage in entry["usages"].as_array().into_iter().flatten() {
        let path: Vec<&str> = usage["path"].as_array().into_iter().flatten().map(text).collect();
        let shown = t!("ide-dependency-usage", path = path.join(" › "), scope = text(&usage["scope"]));
        lines.push(match text(&usage["note"]) {
            "" => shown,
            note => format!("{shown} ({note})"),
        });
    }
    lines
}

#[derive(Default)]
pub(super) struct DependencyAnalyzerUi {
    pub(super) tab: bool,
    system: Option<String>,
    module: Option<String>,
    scope: Option<String>,
    conflicts_only: bool,
    tree: bool,
    search: String,
    analysis: Option<Value>,
    failure: Option<String>,
    seen: Value,
    checked: f64,
    flipped: BTreeSet<String>,
    selected: Option<String>,
}

enum Act {
    Set(Value),
    Select(String),
    Toggle(String),
    Declaration(Row),
    Exclude(Row),
    Refresh,
}

impl DependencyAnalyzerUi {
    fn filtered(&self) -> bool {
        self.conflicts_only || !self.search.trim().is_empty()
    }

    fn rows(&self) -> Vec<Row> {
        match &self.analysis {
            Some(analysis) if self.tree => tree_rows(analysis, &self.flipped, self.filtered()),
            Some(analysis) => list_rows(analysis),
            None => Vec::new(),
        }
    }

    fn loading(&self) -> bool {
        self.seen["state"] == "loading"
    }

    fn params(&self) -> Value {
        json!({
            "system": self.system,
            "module": self.module,
            "scope": self.scope,
            "conflictsOnly": self.conflicts_only,
            "search": self.search,
        })
    }

    fn read(&mut self, registry: &Registry, session: &mut Session) {
        match registry.dispatch(session, ANALYZE, self.params()) {
            Ok(analysis) => {
                self.seen = json!({ "state": analysis["state"], "task": analysis["task"] });
                self.failure = analysis["error"].as_str().map(str::to_owned);
                let keeps_last_good = analysis["state"] != "loaded"
                    && self.analysis.as_ref().is_some_and(|last| last["state"] == "loaded");
                if !keeps_last_good {
                    self.analysis = Some(analysis);
                }
            }
            Err(_) if self.module.is_some() => {
                self.module = None;
                self.scope = None;
                self.read(registry, session);
            }
            Err(error) => self.failure = Some(error.to_string()),
        }
    }

    fn follow(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        if now - self.checked < CHECK_EVERY {
            return;
        }
        self.checked = now;
        let brief = json!({ "system": self.system, "brief": true });
        let Ok(state) = registry.dispatch(session, ANALYZE, brief) else { return };
        let seen = json!({ "state": state["state"], "task": state["task"] });
        if seen != self.seen || self.analysis.is_none() {
            self.read(registry, session);
        }
    }

    fn load(&mut self, registry: &Registry, session: &mut Session) -> Result<(), String> {
        let mut params = self.params();
        params["load"] = json!(true);
        registry.dispatch(session, ANALYZE, params).map_err(|error| error.to_string())?;
        self.read(registry, session);
        Ok(())
    }

    fn apply(&mut self, params: &Value) {
        if let Some(module) = params.get("module") {
            self.module = module.as_str().map(str::to_owned);
            self.scope = None;
            self.flipped.clear();
        }
        if let Some(scope) = params.get("scope") {
            self.scope = scope.as_str().map(str::to_owned).filter(|scope| !scope.is_empty());
        }
        if let Some(on) = params.get("conflictsOnly").and_then(Value::as_bool) {
            self.conflicts_only = on;
        }
        if let Some(on) = params.get("tree").and_then(Value::as_bool) {
            self.tree = on;
        }
        if let Some(search) = params.get("search").and_then(Value::as_str) {
            search.clone_into(&mut self.search);
        }
        if let Some(shown) = params.get("shown").and_then(Value::as_bool) {
            self.tab = shown;
        }
    }

    pub(super) fn state(&self, shown: bool) -> Value {
        let mut state = json!({
            "tab": self.tab,
            "system": self.system,
            "module": self.module,
            "scope": self.scope,
            "conflictsOnly": self.conflicts_only,
            "tree": self.tree,
            "search": self.search,
            "selected": self.selected,
        });
        if !shown || !self.tab {
            return state;
        }
        state["loading"] = json!(self.loading());
        let rows = self.rows();
        state["rows"] = rows
            .iter()
            .map(|row| {
                let mut value = json!({
                    "id": row.id, "depth": row.depth, "label": row.label, "version": row.version,
                    "scope": row.scope, "mark": row.mark.name(), "excludable": row.excludable,
                });
                if row.parent {
                    value["open"] = json!(row.open);
                }
                if let Some(via) = &row.via {
                    value["via"] = json!(via);
                }
                value
            })
            .collect();
        if let Some(analysis) = &self.analysis {
            state["system"] = analysis["system"].clone();
            state["module"] = analysis["module"].clone();
            state["modules"] = analysis["modules"].clone();
            state["scopes"] = analysis["scopes"].clone();
            state["error"] = json!(self.failure);
            if let Some(row) = self.selected_row(&rows) {
                state["details"] = json!(details(analysis, row));
            }
        }
        state
    }

    fn selected_row<'a>(&self, rows: &'a [Row]) -> Option<&'a Row> {
        self.selected.as_ref().and_then(|id| rows.iter().find(|row| row.id == *id))
    }
}

impl IdeView {
    pub(super) fn dependency_analyzer_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, String> {
        if !self.build_tools.available("build") {
            return Err(t!("ide-sync-not-available"));
        }
        match action {
            OPEN => {
                let system = params.get("system").and_then(Value::as_str).map(str::to_owned);
                if system.is_some() && system != self.dependency_analyzer.system {
                    self.dependency_analyzer = DependencyAnalyzerUi { system, ..DependencyAnalyzerUi::default() };
                }
                if !self.tool_visible("build") {
                    self.show_tool("build");
                }
                self.dependency_analyzer.tab = true;
                self.dependency_analyzer.apply(params);
                self.dependency_analyzer.read(registry, session);
                let state = &self.dependency_analyzer.seen["state"];
                if state.is_null() || *state == "failed" {
                    self.dependency_analyzer.load(registry, session)?;
                }
            }
            SET => {
                self.dependency_analyzer.apply(params);
                self.dependency_analyzer.read(registry, session);
            }
            REFRESH => self.dependency_analyzer.load(registry, session)?,
            SELECT | TOGGLE => {
                let id = params.get("row").and_then(Value::as_str).ok_or("`row` is required")?.to_owned();
                let rows = self.dependency_analyzer.rows();
                let row = rows.iter().find(|row| row.id == id).ok_or_else(|| format!("no row `{id}`"))?;
                self.dependency_analyzer.selected = Some(row.id.clone());
                if action == TOGGLE {
                    if !row.parent {
                        return Err(format!("`{id}` has no rows under it"));
                    }
                    if !self.dependency_analyzer.flipped.remove(&id) {
                        self.dependency_analyzer.flipped.insert(id);
                    }
                }
            }
            _ => return Err(format!("unknown action `{action}`")),
        }
        Ok(self.dependency_analyzer.state(true))
    }

    fn analyzer_target(&self, row: &Row) -> Value {
        let ui = &self.dependency_analyzer;
        let module = ui.analysis.as_ref().map(|analysis| analysis["module"].clone());
        json!({
            "system": ui.analysis.as_ref().map(|analysis| analysis["system"].clone()),
            "module": module,
            "groupId": row.group,
            "artifactId": row.artifact,
            "via": row.via,
        })
    }

    pub(super) fn dependency_analyzer_body(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let now = ui.input(|input| input.time);
        self.dependency_analyzer.follow(registry, session, now);
        if self.dependency_analyzer.loading() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        let mut acts = Vec::new();
        self.dependency_toolbar(ui, &p, &mut acts);
        let analyzer = &self.dependency_analyzer;
        let status = if analyzer.loading() {
            Some((t!("ide-build-loading"), false))
        } else {
            analyzer.failure.clone().map(|error| (error, true))
        };
        if let Some((message, error)) = status {
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            ui.painter().with_clip_rect(line).text(
                pos2(line.left() + 12.0, line.center().y),
                Align2::LEFT_CENTER,
                message.lines().next().unwrap_or_default(),
                theme::regular(12.0),
                if error { p.error_text } else { p.text_secondary },
            );
        }
        let area = ui.available_rect_before_wrap();
        let split = area.left() + area.width() * 0.55;
        let left = Rect::from_min_max(area.min, pos2(split, area.bottom()));
        let right = Rect::from_min_max(pos2(split + 1.0, area.top()), area.max);
        ui.painter().vline(split, area.y_range(), egui::Stroke::new(1.0, p.frame));
        let rows = self.dependency_analyzer.rows();
        let mut list = ui.new_child(egui::UiBuilder::new().id_salt("dependency-rows").max_rect(left));
        self.dependency_rows(&mut list, &p, &rows, &mut acts);
        let mut panel = ui.new_child(egui::UiBuilder::new().id_salt("dependency-details").max_rect(right.shrink(8.0)));
        self.dependency_details(&mut panel, &p, &rows, &mut acts);
        ui.advance_cursor_after_rect(area);
        for act in acts {
            self.dependency_act(ui, registry, session, act);
        }
    }

    fn dependency_toolbar(&self, ui: &mut Ui, p: &IdePalette, acts: &mut Vec<Act>) {
        let analyzer = &self.dependency_analyzer;
        let empty = Value::Null;
        let analysis = analyzer.analysis.as_ref().unwrap_or(&empty);
        ui.horizontal(|ui| {
            ui.set_height(30.0);
            ui.add_space(12.0);
            let current = text(&analysis["module"]).to_owned();
            let modules = analysis["modules"].as_array().map_or(&[][..], Vec::as_slice);
            let shown = modules.iter().find(|module| module["id"] == current.as_str()).map_or("", |m| text(&m["name"]));
            let response = egui::ComboBox::from_id_salt("dependency-module")
                .selected_text(RichText::new(shown).font(theme::regular(12.5)).color(p.text))
                .width(160.0)
                .show_ui(ui, |ui| {
                    for module in modules {
                        if ui.selectable_label(module["id"] == current.as_str(), text(&module["name"])).clicked() {
                            acts.push(Act::Set(json!({ "module": module["id"] })));
                        }
                    }
                })
                .response;
            accessibility::name(&response, Role::ComboBox, &t!("ide-dependency-module"));
            let all = t!("ide-dependency-all-scopes");
            let shown = analyzer.scope.clone().unwrap_or_else(|| all.clone());
            let response = egui::ComboBox::from_id_salt("dependency-scope")
                .selected_text(RichText::new(shown).font(theme::regular(12.5)).color(p.text))
                .width(150.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(analyzer.scope.is_none(), all).clicked() {
                        acts.push(Act::Set(json!({ "scope": "" })));
                    }
                    for scope in analysis["scopes"].as_array().into_iter().flatten() {
                        let on = analyzer.scope.as_deref() == scope.as_str();
                        if ui.selectable_label(on, text(scope)).clicked() {
                            acts.push(Act::Set(json!({ "scope": scope })));
                        }
                    }
                })
                .response;
            accessibility::name(&response, Role::ComboBox, &t!("ide-dependency-scope"));
            let mut conflicts = analyzer.conflicts_only;
            if ui.checkbox(&mut conflicts, t!("ide-dependency-conflicts-only")).changed() {
                acts.push(Act::Set(json!({ "conflictsOnly": conflicts })));
            }
            if ui.selectable_label(!analyzer.tree, t!("ide-dependency-list")).clicked() {
                acts.push(Act::Set(json!({ "tree": false })));
            }
            if ui.selectable_label(analyzer.tree, t!("ide-dependency-tree")).clicked() {
                acts.push(Act::Set(json!({ "tree": true })));
            }
            let mut search = analyzer.search.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut search)
                    .desired_width(160.0)
                    .hint_text(t!("ide-dependency-search"))
                    .id_salt("dependency-search"),
            );
            accessibility::labelled(&response, &t!("ide-dependency-search"));
            if response.changed() {
                acts.push(Act::Set(json!({ "search": search })));
            }
            if ui.add_enabled(!analyzer.loading(), egui::Button::new(t!("ide-dependency-refresh"))).clicked() {
                acts.push(Act::Refresh);
            }
        });
    }

    fn dependency_rows(&self, ui: &mut Ui, p: &IdePalette, rows: &[Row], acts: &mut Vec<Act>) {
        let selected = self.dependency_analyzer.selected.as_deref();
        egui::ScrollArea::vertical().id_salt("dependency-rows-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if rows.is_empty() && !self.dependency_analyzer.loading() {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new(t!("ide-dependency-empty")).color(p.text_secondary).font(theme::regular(12.5)),
                    );
                });
            }
            for row in rows {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let is_selected = selected == Some(row.id.as_str());
                accessibility::tree_item(
                    &response,
                    row.depth + 1,
                    row.parent.then_some(row.open),
                    is_selected,
                    &row.label,
                );
                let highlight =
                    Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                if is_selected {
                    ui.painter().rect_filled(highlight, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 5, p.hover);
                }
                let x = highlight.left() + row.depth as f32 * metrics::TREE_INDENT;
                let chevron = Rect::from_center_size(pos2(x + metrics::TREE_ARROW, rect.center().y), Vec2::splat(11.0));
                if row.parent {
                    if row.open {
                        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
                    } else {
                        crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
                    }
                }
                paint_row(ui, p, rect, x, row);
                if response.clicked() || response.double_clicked() {
                    let on_chevron = response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    acts.push(Act::Select(row.id.clone()));
                    if row.parent && on_chevron {
                        acts.push(Act::Toggle(row.id.clone()));
                    } else if response.double_clicked() && !row.artifact.is_empty() {
                        acts.push(Act::Declaration(row.clone()));
                    }
                }
                if !row.artifact.is_empty() {
                    response.context_menu(|ui| {
                        if ui.button(t!("ide-dependency-go-to")).clicked() {
                            acts.push(Act::Declaration(row.clone()));
                            ui.close();
                        }
                        if ui.add_enabled(row.excludable, egui::Button::new(t!("ide-dependency-exclude"))).clicked() {
                            acts.push(Act::Exclude(row.clone()));
                            ui.close();
                        }
                    });
                }
            }
        });
    }

    fn dependency_details(&self, ui: &mut Ui, p: &IdePalette, rows: &[Row], acts: &mut Vec<Act>) {
        let analyzer = &self.dependency_analyzer;
        let selected = analyzer.selected_row(rows).filter(|row| !row.artifact.is_empty());
        let (Some(row), Some(analysis)) = (selected, &analyzer.analysis) else {
            ui.label(RichText::new(t!("ide-dependency-select")).color(p.text_secondary).font(theme::regular(12.5)));
            return;
        };
        ui.horizontal(|ui| {
            if ui.button(t!("ide-dependency-go-to")).clicked() {
                acts.push(Act::Declaration(row.clone()));
            }
            if ui.add_enabled(row.excludable, egui::Button::new(t!("ide-dependency-exclude"))).clicked() {
                acts.push(Act::Exclude(row.clone()));
            }
        });
        egui::ScrollArea::vertical().id_salt("dependency-details-scroll").auto_shrink([false, false]).show(ui, |ui| {
            for (index, line) in details(analysis, row).iter().enumerate() {
                let color = if index == 0 { p.text } else { p.text_secondary };
                ui.label(RichText::new(line).color(color).font(theme::regular(12.5)));
            }
        });
    }

    fn dependency_act(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, act: Act) {
        match act {
            Act::Set(params) => self.run_action(ui, registry, session, SET, params),
            Act::Select(row) => self.run_action(ui, registry, session, SELECT, json!({ "row": row })),
            Act::Toggle(row) => self.run_action(ui, registry, session, TOGGLE, json!({ "row": row })),
            Act::Refresh => self.run_action(ui, registry, session, REFRESH, json!({})),
            Act::Declaration(row) => {
                let params = self.analyzer_target(&row);
                self.dispatch(ui, registry, session, DECLARATION, params);
            }
            Act::Exclude(row) => {
                let params = self.analyzer_target(&row);
                match registry.dispatch(session, EXCLUDE, params) {
                    Ok(done) => {
                        if let Some(error) = done["reloadError"].as_str() {
                            self.post(ui, error.to_owned());
                        }
                        self.dependency_analyzer.checked = f64::NEG_INFINITY;
                    }
                    Err(error) => self.report(ui, error),
                }
            }
        }
    }
}

fn paint_row(ui: &Ui, p: &IdePalette, rect: Rect, x: f32, row: &Row) {
    let painter = ui.painter().with_clip_rect(rect);
    let mark = Rect::from_center_size(pos2(x + metrics::TREE_ICON, rect.center().y), Vec2::splat(8.0));
    match row.mark {
        Mark::Plain => {}
        Mark::Conflict => {
            painter.circle_filled(mark.center(), 4.0, p.problem("warning"));
        }
        Mark::Omitted => {
            painter.circle_stroke(mark.center(), 3.5, egui::Stroke::new(1.0, p.text_secondary));
        }
        Mark::Failed => {
            painter.circle_filled(mark.center(), 4.0, p.problem("error"));
        }
    }
    let name_color = if row.mark == Mark::Omitted { p.text_secondary } else { p.text };
    let name = painter.text(
        pos2(x + metrics::TREE_TEXT, rect.center().y),
        Align2::LEFT_CENTER,
        &row.label,
        theme::regular(13.0),
        name_color,
    );
    let detail = [row.version.as_str(), row.scope.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  ");
    painter.text(
        pos2(name.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        detail,
        theme::regular(12.5),
        p.text_secondary,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis() -> Value {
        json!({
            "system": "maven",
            "module": "demo:app",
            "dependencies": [
                { "groupId": "c", "artifactId": "codec", "version": "1.10", "requested": ["1.11", "1.10"],
                  "scopes": ["compile"], "conflict": true, "reason": "conflict", "direct": true, "usages": [
                    { "path": ["h:client:4", "c:codec:1.10"], "scope": "compile", "conflict": true, "omitted": true, "note": "omitted for conflict with 1.10" },
                    { "path": ["c:codec:1.10"], "scope": "compile", "conflict": false, "omitted": false },
                ] },
                { "groupId": "h", "artifactId": "client", "version": "4", "requested": ["4"], "scopes": ["compile"],
                  "conflict": false, "direct": true, "usages": [{ "path": ["h:client:4"], "scope": "compile" }] },
            ],
            "tree": [
                { "groupId": "h", "artifactId": "client", "requestedVersion": "4", "selectedVersion": "4", "scope": "compile",
                  "children": [{ "groupId": "c", "artifactId": "codec", "requestedVersion": "1.11", "selectedVersion": "1.10",
                                 "scope": "compile", "conflict": true, "omitted": true, "reason": "conflict" }] },
                { "groupId": "c", "artifactId": "codec", "requestedVersion": "1.10", "selectedVersion": "1.10", "scope": "compile" },
            ],
        })
    }

    #[test]
    fn the_list_marks_a_conflict_and_offers_exclude_only_for_what_comes_with_something() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let rows = list_rows(&analysis());
        assert_eq!((rows[0].label.as_str(), rows[0].version.as_str()), ("c:codec", "1.10"));
        assert_eq!(rows[0].mark, Mark::Conflict);
        assert!(rows[0].excludable);
        assert!(!rows[1].excludable, "only the module declares it");
    }

    #[test]
    fn the_tree_opens_by_a_click_or_a_filter_and_shows_which_version_won() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let closed = tree_rows(&analysis(), &BTreeSet::new(), false);
        assert_eq!(closed.len(), 2);
        assert!(closed[0].parent && !closed[0].open);
        let filtered = tree_rows(&analysis(), &BTreeSet::new(), true);
        assert_eq!(filtered.len(), 3);
        let codec = &filtered[1];
        assert_eq!(codec.id, "/h:client/c:codec");
        assert_eq!(codec.version, "1.11 → 1.10");
        assert_eq!(codec.via.as_deref(), Some("h:client"));
        assert!(codec.excludable && !filtered[2].excludable);
    }

    #[test]
    fn the_last_good_analysis_stays_while_another_loads_or_fails() {
        let mut registry = Registry::new();
        registry.register(Answer).unwrap();
        let mut session = Session::new();
        let mut ui = DependencyAnalyzerUi::default();
        session.set_state(Answers(vec![
            json!({ "state": "failed", "error": "no Maven", "dependencies": [] }),
            json!({ "state": "loading", "dependencies": [] }),
            json!({ "state": "loaded", "dependencies": [{ "groupId": "a", "artifactId": "b", "version": "1" }] }),
        ]));
        ui.read(&registry, &mut session);
        assert_eq!(ui.rows().len(), 1);
        ui.read(&registry, &mut session);
        assert_eq!(ui.rows().len(), 1, "kept while loading");
        assert!(ui.loading());
        ui.read(&registry, &mut session);
        assert_eq!(ui.rows().len(), 1, "kept after a failure");
        assert_eq!(ui.failure.as_deref(), Some("no Maven"));
    }

    #[derive(Default)]
    struct Answers(Vec<Value>);

    struct Answer;

    impl zen_core::Command for Answer {
        type Params = zen_core::NoParams;
        type Output = Value;
        const ID: &'static str = ANALYZE;
        const TITLE: &'static str = "Answer";
        const DESCRIPTION: &'static str = "Answers the next recorded analysis.";

        fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<Value, zen_core::CommandError> {
            Ok(session.state_mut::<Answers>().0.pop().unwrap_or_default())
        }
    }

    #[test]
    fn gradle_trees_have_a_folder_per_configuration() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let analysis = json!({ "byConfiguration": true, "tree": [
            { "groupId": "a", "artifactId": "b", "selectedVersion": "1", "scope": "compileClasspath" },
            { "groupId": "a", "artifactId": "b", "selectedVersion": "1", "scope": "runtimeClasspath" },
        ] });
        let rows = tree_rows(&analysis, &BTreeSet::new(), false);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["compileClasspath", "a:b", "runtimeClasspath", "a:b"]);
        assert_ne!(rows[1].id, rows[3].id);
        assert_eq!(rows[1].depth, 1);
    }

    #[test]
    fn the_details_say_which_version_won_why_and_every_path() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let value = analysis();
        let rows = list_rows(&value);
        let lines = details(&value, &rows[0]);
        assert_eq!(lines[0], "c:codec:1.10");
        assert!(lines[1].contains("1.10") && lines[1].contains("nearer"), "{lines:?}");
        assert!(lines[2].contains("1.11, 1.10"), "{lines:?}");
        assert!(
            lines[3].starts_with("h:client:4 › c:codec:1.10") && lines[3].contains("omitted for conflict"),
            "{lines:?}"
        );
    }
}
