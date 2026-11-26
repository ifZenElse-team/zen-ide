use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::build_tool_window::{Row, RowIcon, paint_row, row};
use super::glyphs::*;
use super::{IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const ID: &str = "ant";
const READ_EVERY: f64 = 2.0;
const CHECK_EVERY: f64 = 1.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AntAction {
    Run { build_file: String, target: String },
    Open(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AntRow {
    pub(super) row: Row,
    pub(super) action: Option<AntAction>,
    pub(super) build_file: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

pub(super) fn ant_rows(project: &Value, collapsed: &BTreeSet<String>) -> Vec<AntRow> {
    let filter = project["filterTargets"] == true;
    let mut rows = Vec::new();
    for file in project["buildFiles"].as_array().into_iter().flatten() {
        let build_file = text(&file["settings"]["path"]);
        let mut head = row(build_file.clone(), 0, text(&file["name"]), RowIcon::Project);
        head.detail = build_file.clone();
        if let Some(error) = file["error"].as_str() {
            head.detail = error.lines().next().unwrap_or_default().to_owned();
            rows.push(AntRow {
                row: head,
                action: Some(AntAction::Open(file["path"].as_str().unwrap_or_default().into())),
                build_file,
            });
            continue;
        }
        head.parent = true;
        let open = !collapsed.contains(&build_file);
        rows.push(AntRow {
            row: head,
            action: Some(AntAction::Open(PathBuf::from(text(&file["path"])))),
            build_file: build_file.clone(),
        });
        if !open {
            continue;
        }
        let default = text(&file["default"]);
        for target in file["targets"].as_array().into_iter().flatten() {
            let name = text(&target["name"]);
            let description = text(&target["description"]);
            let is_default = name == default;
            if filter && description.is_empty() && !is_default {
                continue;
            }
            let mut leaf = row(format!("{build_file}/{name}"), 1, name.clone(), RowIcon::Run);
            leaf.detail = match (is_default, description.is_empty()) {
                (true, true) => t!("ide-ant-default"),
                (true, false) => format!("{} {description}", t!("ide-ant-default")),
                (false, _) => description,
            };
            rows.push(AntRow {
                row: leaf,
                action: Some(AntAction::Run { build_file: build_file.clone(), target: name }),
                build_file: build_file.clone(),
            });
        }
    }
    rows
}

#[derive(Default)]
pub(super) struct AntToolWindow {
    checked: Option<(PathBuf, u64, f64)>,
    project: Option<Value>,
    error: Option<String>,
    read: f64,
    collapsed: BTreeSet<String>,
    selected: Option<String>,
}

impl AntToolWindow {
    fn stale(&mut self) {
        self.read = f64::NEG_INFINITY;
    }

    fn read(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        if now - self.read < READ_EVERY {
            return;
        }
        self.read = now;
        match registry.dispatch(session, "ant.project.get", json!({})) {
            Ok(project) => {
                self.project = Some(project);
                self.error = None;
            }
            Err(error) => self.error = Some(error.text()),
        }
    }

    fn read_now(&mut self, registry: &Registry, session: &mut Session) {
        self.stale();
        self.read(registry, session, f64::MAX);
        self.stale();
    }

    fn rows(&self) -> Vec<AntRow> {
        self.project.as_ref().map(|project| ant_rows(project, &self.collapsed)).unwrap_or_default()
    }

    fn toggle(&mut self, id: &str) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
    }

    pub(super) fn state(&self, available: bool, shown: bool) -> Value {
        let mut state = json!({ "available": available, "selected": self.selected });
        if available && shown {
            let rows: Vec<Value> = self
                .rows()
                .iter()
                .map(|entry| {
                    let mut row = json!({ "id": entry.row.id, "depth": entry.row.depth, "label": entry.row.label });
                    if !entry.row.detail.is_empty() {
                        row["detail"] = json!(entry.row.detail);
                    }
                    if entry.row.parent {
                        row["open"] = json!(!self.collapsed.contains(&entry.row.id));
                    }
                    if let Some(action) = &entry.action {
                        row["runs"] = json!(match action {
                            AntAction::Run { .. } => "target",
                            AntAction::Open(_) => "open",
                        });
                    }
                    row
                })
                .collect();
            state["rows"] = Value::Array(rows);
            if let Some(project) = &self.project {
                state["filterTargets"] = project["filterTargets"].clone();
            }
        }
        state
    }
}

fn run_action(registry: &Registry, session: &mut Session, action: &AntAction) -> Result<(), String> {
    let (command, params) = match action {
        AntAction::Run { build_file, target } => {
            ("ant.target.run", json!({ "buildFile": build_file, "targets": target }))
        }
        AntAction::Open(path) => ("file.document.open", json!({ "path": path })),
    };
    registry.dispatch(session, command, params).map(|_| ()).map_err(|error| error.text())
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_build_file(project: &Path) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(t!("ide-ant-add")).set_directory(project).add_filter("XML", &["xml"]).pick_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_build_file(_project: &Path) -> Option<PathBuf> {
    None
}

impl IdeView {
    pub(super) fn follow_ant(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        if self.build_tools.ant && self.tool_visible(ID) {
            self.ant.read(registry, session, now);
        }
        let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
            self.build_tools.ant = false;
            return;
        };
        let generation = self.plugins.generation();
        let fresh = self.ant.checked.as_ref().is_some_and(|(seen, at_generation, at)| {
            *seen == root && *at_generation == generation && now - at < CHECK_EVERY
        });
        if fresh {
            return;
        }
        self.ant.checked = Some((root.clone(), generation, now));
        let installed = session
            .state::<zen_workbench::plugin_projects::PluginProjects>()
            .is_some_and(|projects| projects.installed(ID));
        self.build_tools.ant = installed && zen_workbench::ant::has_build_files(&root);
        if !self.build_tools.ant && self.tool_visible(ID) {
            self.hide_tool(ID);
        }
    }

    pub(super) fn toggle_ant(&mut self, registry: &Registry, session: &mut Session) -> Result<(), String> {
        if !self.build_tools.ant {
            return Err(t!("ide-ant-not-available"));
        }
        self.toggle_tool(ID);
        if self.tool_visible(ID) {
            self.ant.read_now(registry, session);
        }
        Ok(())
    }

    pub(super) fn ant_state(&self) -> Value {
        self.ant.state(self.build_tools.ant, self.tool_visible(ID))
    }

    pub(super) fn ant_tool_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, String> {
        if !self.build_tools.ant {
            return Err(t!("ide-ant-not-available"));
        }
        if !self.tool_visible(ID) {
            self.show_tool(ID);
        }
        self.ant.read_now(registry, session);
        let rows = self.ant.rows();
        let named = params.get("row").and_then(Value::as_str).map(str::to_owned);
        let id = match (action, named) {
            (_, Some(id)) => id,
            ("view.buildTool.run", None) => self.ant.selected.clone().ok_or("no row is selected; give `row`")?,
            _ => return Err("`row` is required: an id of get_window_state's buildTools.ant rows".to_owned()),
        };
        let entry = rows
            .iter()
            .find(|entry| entry.row.id == id)
            .cloned()
            .ok_or_else(|| format!("no row `{id}` in the ant tool window (open its build file first)"))?;
        self.ant.selected = Some(entry.row.id.clone());
        match action {
            "view.buildTool.select" => {}
            "view.buildTool.toggle" if entry.row.parent => self.ant.toggle(&entry.row.id),
            "view.buildTool.toggle" => return Err(format!("`{id}` has no rows under it")),
            _ => {
                let run = entry.action.as_ref().ok_or_else(|| format!("`{id}` runs nothing"))?;
                run_action(registry, session, run)?;
                self.ant.stale();
            }
        }
        Ok(json!({ "selected": entry.row.id }))
    }

    pub(super) fn ant_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let now = ui.input(|input| input.time);
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        self.ant.read(registry, session, now);
        let rows = self.ant.rows();
        let selected = self.ant.selected.clone();
        let selected_entry = selected.as_ref().and_then(|id| rows.iter().find(|entry| &entry.row.id == id)).cloned();
        let filter = self.ant.project.as_ref().is_some_and(|project| project["filterTargets"] == true);

        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-ant-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("ant-tool-title"), title, || t!("ide-ant-title"));
        let mut x = title.right() + 14.0;
        let mut button = |ui: &mut Ui, key: &str, glyph: Glyph, label: String, on: Option<bool>, enabled: bool| {
            let rect = Rect::from_center_size(pos2(x + 12.0, header.center().y), Vec2::splat(24.0));
            x += 26.0;
            let response = ui.interact(
                rect,
                ui.scope_id().with(("ant-tool", key)),
                if enabled { Sense::click() } else { Sense::hover() },
            );
            if on == Some(true) {
                ui.painter().rect_filled(rect, 5, p.hover);
            }
            if enabled {
                hover(ui, &response, &p);
            }
            let color = if enabled { p.text_secondary } else { p.text_secondary.gamma_multiply(0.4) };
            glyph(ui.painter(), rect.shrink(6.0), color);
            hint(ui, &response, &label, None, Side::Below);
            accessibility::button(&response, &label);
            if let Some(on) = on {
                accessibility::pressed(&response, on);
            }
            if !enabled {
                accessibility::disabled(&response);
            }
            enabled && response.clicked()
        };
        let mut actions: Vec<(&'static str, Value)> = Vec::new();
        if button(ui, "add", glyph_plus, t!("ide-ant-add"), None, true)
            && let Some(path) = pick_build_file(&root)
        {
            actions.push(("ant.buildFile.add", json!({ "path": path })));
        }
        let remove = selected_entry.as_ref().map(|entry| entry.build_file.clone());
        if button(ui, "remove", glyph_minus, t!("ide-ant-remove"), None, remove.is_some())
            && let Some(build_file) = &remove
        {
            actions.push(("ant.buildFile.remove", json!({ "path": build_file })));
        }
        let run = selected_entry
            .as_ref()
            .and_then(|entry| entry.action.clone())
            .filter(|action| matches!(action, AntAction::Run { .. }));
        let run_selected = button(ui, "run", glyph_run, t!("ide-build-run-selected"), None, run.is_some());
        if button(ui, "filter", glyph_filter, t!("ide-ant-filter"), Some(filter), true) {
            actions.push(("ant.options.set", json!({ "filterTargets": !filter })));
        }
        if button(ui, "collapse", glyph_collapse_all, t!("ide-build-collapse-all"), None, true) {
            self.ant.collapsed =
                rows.iter().filter(|entry| entry.row.parent).map(|entry| entry.row.id.clone()).collect();
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("ant-tool-hide"), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        accessibility::button(&response, &t!("ide-run-hide"));
        if response.clicked() {
            self.hide_tool(ID);
        }

        if let Some(error) = self.ant.error.clone() {
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            ui.painter().with_clip_rect(line).text(
                pos2(line.left() + 12.0, line.center().y),
                Align2::LEFT_CENTER,
                error,
                theme::regular(12.0),
                p.error_text,
            );
        }
        if self.ant.project.as_ref().is_some_and(|project| project.get("executable").is_none()) {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(t!("ide-ant-missing")).color(p.error_text).font(theme::regular(12.0)));
                if ui.button(t!("ide-ant-download")).clicked() {
                    actions.push(("ant.distribution.download", json!({})));
                }
            });
        }

        let mut clicked: Option<(String, bool, bool)> = None;
        egui::ScrollArea::vertical().id_salt("ant-tool-tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for entry in &rows {
                let row = &entry.row;
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let is_selected = selected.as_deref() == Some(row.id.as_str());
                let expanded = row.parent.then(|| !self.ant.collapsed.contains(&row.id));
                accessibility::tree_item(&response, row.depth + 1, expanded, is_selected, &row.label);
                let highlight =
                    Rect::from_min_max(pos2(rect.left() + 12.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                if is_selected {
                    ui.painter().rect_filled(highlight, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 5, p.hover);
                }
                let x = highlight.left() + row.depth as f32 * metrics::TREE_INDENT;
                let chevron = Rect::from_center_size(pos2(x + metrics::TREE_ARROW, rect.center().y), Vec2::splat(11.0));
                match expanded {
                    Some(true) => crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary),
                    Some(false) => crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary),
                    None => {}
                }
                paint_row(ui, &p, rect, x, row);
                if response.clicked() || response.double_clicked() {
                    let on_chevron =
                        row.parent && response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    clicked = Some((row.id.clone(), response.double_clicked(), on_chevron));
                }
            }
            if rows.is_empty() {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new(t!("ide-build-empty")).color(p.text_secondary).font(theme::regular(12.5)),
                    );
                });
            }
        });
        if let Some((id, activate, on_chevron)) = clicked {
            self.ant.selected = Some(id.clone());
            if let Some(entry) = rows.iter().find(|entry| entry.row.id == id) {
                let runs = matches!(entry.action, Some(AntAction::Run { .. }));
                if on_chevron || (activate && entry.row.parent && !runs) {
                    self.ant.toggle(&id);
                } else if activate && let Some(action) = &entry.action {
                    if let Err(error) = run_action(registry, session, action) {
                        self.post(ui, error);
                    }
                    self.ant.stale();
                }
            }
        }
        if run_selected
            && let Some(action) = run
            && let Err(error) = run_action(registry, session, &action)
        {
            self.post(ui, error);
        }
        let acted = !actions.is_empty();
        for (command, params) in actions {
            if let Err(error) = registry.dispatch(session, command, params) {
                self.post(ui, error.text());
            }
        }
        if acted {
            self.ant.stale();
            self.ant.checked = None;
        }
    }
}

#[cfg(test)]
mod tests;
