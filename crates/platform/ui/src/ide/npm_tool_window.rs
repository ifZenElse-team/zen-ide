use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::hidden_files::{HiddenFromAgents, is_hidden};

use super::build_tool_window::{Row, RowIcon, paint_row, row};
use super::glyphs::*;
use super::{IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const ID: &str = "npm";
const READ_EVERY: f64 = 2.0;
const CHECK_EVERY: f64 = 1.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NpmAction {
    Run { package_json: String, script: String },
    Open(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct NpmRow {
    pub(super) row: Row,
    pub(super) action: Option<NpmAction>,
    pub(super) package_json: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

pub(super) fn npm_rows(project: &Value, collapsed: &BTreeSet<String>) -> Vec<NpmRow> {
    let mut rows = Vec::new();
    for package in project["packages"].as_array().into_iter().flatten() {
        let package_json = text(&package["relative"]);
        let open_file = Some(NpmAction::Open(PathBuf::from(text(&package["path"]))));
        let mut head = row(package_json.clone(), 0, text(&package["name"]), RowIcon::Project);
        if let Some(error) = package["error"].as_str() {
            head.detail = error.lines().next().unwrap_or_default().to_owned();
            rows.push(NpmRow { row: head, action: open_file, package_json });
            continue;
        }
        head.detail = format!("{package_json} · {}", text(&package["manager"]));
        let scripts: Vec<&Value> = package["scripts"].as_array().into_iter().flatten().collect();
        head.parent = !scripts.is_empty();
        rows.push(NpmRow { row: head, action: open_file, package_json: package_json.clone() });
        if collapsed.contains(&package_json) {
            continue;
        }
        for script in scripts {
            let name = text(&script["name"]);
            let mut leaf = row(format!("{package_json}/{name}"), 1, name.clone(), RowIcon::Run);
            leaf.detail = text(&script["command"]);
            rows.push(NpmRow {
                row: leaf,
                action: Some(NpmAction::Run { package_json: package_json.clone(), script: name }),
                package_json: package_json.clone(),
            });
        }
    }
    rows
}

struct Scan {
    root: PathBuf,
    has_packages: bool,
    project: Option<Value>,
}

#[derive(Default)]
pub(super) struct NpmToolWindow {
    checked: Option<(PathBuf, u64, f64)>,
    project: Option<Value>,
    error: Option<String>,
    read: f64,
    scanning: Arc<AtomicBool>,
    scanned: Arc<Mutex<Option<Scan>>>,
    collapsed: BTreeSet<String>,
    selected: Option<String>,
}

impl NpmToolWindow {
    fn stale(&mut self) {
        self.read = f64::NEG_INFINITY;
    }

    fn read_due(&self, now: f64) -> bool {
        now - self.read >= READ_EVERY
    }

    fn start_scan(&mut self, ctx: &egui::Context, root: PathBuf, with_project: bool, now: f64) {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        if with_project {
            self.read = now;
        }
        let (scanning, scanned, ctx) = (self.scanning.clone(), self.scanned.clone(), ctx.clone());
        let reader = std::thread::Builder::new().name("zen-npm-scan".to_owned());
        let flag = self.scanning.clone();
        let started = reader.spawn(move || {
            let has_packages = zen_workbench::npm::model::has_packages(&root);
            let project = (with_project && has_packages)
                .then(|| serde_json::to_value(zen_workbench::npm::read_project(&root)).ok())
                .flatten();
            *scanned.lock().unwrap_or_else(PoisonError::into_inner) = Some(Scan { root, has_packages, project });
            scanning.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
        if started.is_err() {
            flag.store(false, Ordering::SeqCst);
        }
    }

    fn take_scan(&self, root: &Path) -> Option<Scan> {
        let scan = self.scanned.lock().unwrap_or_else(PoisonError::into_inner).take()?;
        (scan.root == root).then_some(scan)
    }

    fn read(&mut self, registry: &Registry, session: &mut Session) {
        match registry.dispatch(session, "npm.project.get", json!({})) {
            Ok(project) => {
                self.project = Some(project);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn read_now(&mut self, registry: &Registry, session: &mut Session) {
        self.read(registry, session);
        self.stale();
    }

    fn rows(&self) -> Vec<NpmRow> {
        self.project.as_ref().map(|project| npm_rows(project, &self.collapsed)).unwrap_or_default()
    }

    fn toggle(&mut self, id: &str) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
    }

    pub(super) fn state(&self, available: bool, shown: bool, hidden: &dyn Fn(&str) -> bool) -> Value {
        let mut state = json!({ "available": available, "selected": self.selected });
        if available && shown {
            let rows: Vec<Value> = self
                .rows()
                .iter()
                .filter(|entry| !hidden(&entry.package_json))
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
                            NpmAction::Run { .. } => "script",
                            NpmAction::Open(_) => "open",
                        });
                    }
                    row
                })
                .collect();
            state["rows"] = Value::Array(rows);
            if let Some(project) = &self.project {
                state["offline"] = project["offline"].clone();
            }
        }
        state
    }
}

fn run_action(registry: &Registry, session: &mut Session, action: &NpmAction) -> Result<(), String> {
    let (command, params) = match action {
        NpmAction::Run { package_json, script } => {
            ("npm.script.run", json!({ "packageJson": package_json, "script": script }))
        }
        NpmAction::Open(path) => ("file.document.open", json!({ "path": path })),
    };
    registry.dispatch(session, command, params).map(|_| ()).map_err(|error| error.to_string())
}

impl IdeView {
    pub(super) fn follow_npm(&mut self, ctx: &egui::Context, session: &Session, now: f64) {
        let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
            self.build_tools.npm = false;
            return;
        };
        let installed = session
            .state::<zen_workbench::plugin_projects::PluginProjects>()
            .is_some_and(|projects| projects.installed(ID));
        if let Some(scan) = self.npm.take_scan(&root) {
            if let Some(project) = scan.project {
                self.npm.project = Some(project);
                self.npm.error = None;
            }
            self.build_tools.npm = installed && scan.has_packages;
            if !self.build_tools.npm && self.tool_visible(ID) {
                self.hide_tool(ID);
            }
        }
        if !installed {
            self.build_tools.npm = false;
            return;
        }
        let generation = self.plugins.generation();
        let fresh = self.npm.checked.as_ref().is_some_and(|(seen, at_generation, at)| {
            *seen == root && *at_generation == generation && now - at < CHECK_EVERY
        });
        if fresh {
            return;
        }
        self.npm.checked = Some((root.clone(), generation, now));
        let with_project = self.build_tools.npm && self.tool_visible(ID) && self.npm.read_due(now);
        self.npm.start_scan(ctx, root, with_project, now);
    }

    pub(super) fn toggle_npm(&mut self, registry: &Registry, session: &mut Session) -> Result<(), String> {
        if !self.build_tools.npm {
            return Err(t!("ide-npm-not-available"));
        }
        self.toggle_tool(ID);
        if self.tool_visible(ID) {
            self.npm.read_now(registry, session);
        }
        Ok(())
    }

    pub(super) fn npm_state(&self, session: &Session) -> Value {
        let patterns = session.state::<HiddenFromAgents>().map(HiddenFromAgents::patterns).unwrap_or_default();
        let root = session.state::<Workbench>().and_then(Workbench::project);
        let hidden = |relative: &str| root.is_some_and(|root| is_hidden(patterns, Some(root), &root.join(relative)));
        self.npm.state(self.build_tools.npm, self.tool_visible(ID), &hidden)
    }

    pub(super) fn npm_tool_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, String> {
        if !self.build_tools.npm {
            return Err(t!("ide-npm-not-available"));
        }
        if !self.tool_visible(ID) {
            self.show_tool(ID);
        }
        self.npm.read_now(registry, session);
        let rows = self.npm.rows();
        let named = params.get("row").and_then(Value::as_str).map(str::to_owned);
        let id = match (action, named) {
            (_, Some(id)) => id,
            ("view.buildTool.run", None) => self.npm.selected.clone().ok_or("no row is selected; give `row`")?,
            _ => return Err("`row` is required: an id of get_window_state's buildTools.npm rows".to_owned()),
        };
        let entry = rows
            .iter()
            .find(|entry| entry.row.id == id)
            .cloned()
            .ok_or_else(|| format!("no row `{id}` in the npm tool window (open its package first)"))?;
        self.npm.selected = Some(entry.row.id.clone());
        match action {
            "view.buildTool.select" => {}
            "view.buildTool.toggle" if entry.row.parent => self.npm.toggle(&entry.row.id),
            "view.buildTool.toggle" => return Err(format!("`{id}` has no rows under it")),
            _ => {
                let run = entry.action.as_ref().ok_or_else(|| format!("`{id}` runs nothing"))?;
                run_action(registry, session, run)?;
                self.npm.stale();
            }
        }
        Ok(json!({ "selected": entry.row.id }))
    }

    pub(super) fn npm_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let rows = self.npm.rows();
        let selected = self.npm.selected.clone();
        let selected_entry = selected.as_ref().and_then(|id| rows.iter().find(|entry| &entry.row.id == id)).cloned();
        let offline = self.npm.project.as_ref().is_some_and(|project| project["offline"] == true);

        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-npm-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("npm-tool-title"), title, || t!("ide-npm-title"));
        let mut x = title.right() + 14.0;
        let mut button = |ui: &mut Ui, key: &str, glyph: Glyph, label: String, on: Option<bool>, enabled: bool| {
            let rect = Rect::from_center_size(pos2(x + 12.0, header.center().y), Vec2::splat(24.0));
            x += 26.0;
            let response = ui.interact(
                rect,
                ui.scope_id().with(("npm-tool", key)),
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
        let run = selected_entry
            .as_ref()
            .and_then(|entry| entry.action.clone())
            .filter(|action| matches!(action, NpmAction::Run { .. }));
        let run_selected = button(ui, "run", glyph_run, t!("ide-build-run-selected"), None, run.is_some());
        let target = selected_entry.as_ref().map(|entry| entry.package_json.clone());
        if button(ui, "install", glyph_arrow_down, t!("ide-npm-install"), None, !rows.is_empty()) {
            let params = target.as_ref().map_or_else(|| json!({}), |path| json!({ "packageJson": path }));
            actions.push(("npm.dependencies.install", params));
        }
        if button(ui, "offline", glyph_filter, t!("ide-npm-offline"), Some(offline), true) {
            actions.push(("npm.options.set", json!({ "offline": !offline })));
        }
        if button(ui, "collapse", glyph_collapse_all, t!("ide-build-collapse-all"), None, true) {
            self.npm.collapsed =
                rows.iter().filter(|entry| entry.row.parent).map(|entry| entry.row.id.clone()).collect();
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("npm-tool-hide"), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        accessibility::button(&response, &t!("ide-run-hide"));
        if response.clicked() {
            self.hide_tool(ID);
        }

        if let Some(error) = self.npm.error.clone() {
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            ui.painter().with_clip_rect(line).text(
                pos2(line.left() + 12.0, line.center().y),
                Align2::LEFT_CENTER,
                error,
                theme::regular(12.0),
                p.error_text,
            );
        }

        let mut clicked: Option<(String, bool, bool)> = None;
        egui::ScrollArea::vertical().id_salt("npm-tool-tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for entry in &rows {
                let row = &entry.row;
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let is_selected = selected.as_deref() == Some(row.id.as_str());
                let expanded = row.parent.then(|| !self.npm.collapsed.contains(&row.id));
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
                        egui::RichText::new(t!("ide-npm-empty")).color(p.text_secondary).font(theme::regular(12.5)),
                    );
                });
            }
        });
        if let Some((id, activate, on_chevron)) = clicked {
            self.npm.selected = Some(id.clone());
            if let Some(entry) = rows.iter().find(|entry| entry.row.id == id) {
                if on_chevron {
                    self.npm.toggle(&id);
                } else if activate && let Some(action) = &entry.action {
                    if let Err(error) = run_action(registry, session, action) {
                        self.post(ui, error);
                    }
                    self.npm.stale();
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
                self.post(ui, error.to_string());
            }
        }
        if acted {
            self.npm.stale();
            self.npm.checked = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Value {
        json!({
            "offline": false,
            "packages": [
                {
                    "name": "shop", "path": "/p/package.json", "relative": "package.json", "manager": "pnpm",
                    "scripts": [
                        { "name": "dev", "command": "vite", "line": 3 },
                        { "name": "build", "command": "vite build", "line": 4 }
                    ]
                },
                { "name": "web", "path": "/p/web/package.json", "relative": "web/package.json", "manager": "npm", "scripts": [] },
                { "name": "bad", "path": "/p/bad/package.json", "relative": "bad/package.json", "error": "expected value\nat 1", "scripts": [] }
            ]
        })
    }

    fn labels(rows: &[NpmRow]) -> Vec<String> {
        rows.iter()
            .map(|entry| format!("{}{} {}", "  ".repeat(entry.row.depth), entry.row.label, entry.row.detail))
            .collect()
    }

    #[test]
    fn packages_list_their_scripts_and_a_file_that_does_not_read_says_why() {
        let rows = npm_rows(&project(), &BTreeSet::new());
        assert_eq!(
            labels(&rows),
            [
                "shop package.json · pnpm",
                "  dev vite",
                "  build vite build",
                "web web/package.json · npm",
                "bad expected value"
            ]
        );
        assert!(rows[0].row.parent && !rows[3].row.parent && !rows[4].row.parent);
        assert_eq!(rows[0].action, Some(NpmAction::Open("/p/package.json".into())));
        assert_eq!(rows[1].action, Some(NpmAction::Run { package_json: "package.json".into(), script: "dev".into() }));
    }

    #[test]
    fn the_window_state_keeps_a_hidden_package_from_agents() {
        let window = NpmToolWindow { project: Some(project()), ..NpmToolWindow::default() };
        let all = window.state(true, true, &|_| false)["rows"].as_array().unwrap().len();
        let first = npm_rows(&project(), &BTreeSet::new())[0].package_json.clone();
        let hidden = window.state(true, true, &|relative| relative == first);
        let rows = hidden["rows"].as_array().unwrap();
        assert!(rows.len() < all, "{hidden}");
        assert!(rows.iter().all(|row| !row["id"].as_str().unwrap().starts_with(&first)), "{hidden}");
    }

    #[test]
    fn the_disk_is_read_on_a_thread_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        std::fs::write(root.join("package.json"), r#"{ "name": "shop" }"#).unwrap();
        let mut window = NpmToolWindow::default();
        window.start_scan(&egui::Context::default(), root.clone(), true, 1.0);
        assert!(!window.read_due(1.0), "the read is on its way");
        let started = std::time::Instant::now();
        let scan = loop {
            if let Some(scan) = window.take_scan(&root) {
                break scan;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(30));
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(scan.has_packages);
        assert_eq!(scan.project.unwrap()["packages"][0]["name"], "shop");
        assert!(window.take_scan(&root).is_none(), "one read, one answer");
    }

    #[test]
    fn a_collapsed_package_hides_its_scripts() {
        let collapsed: BTreeSet<String> = ["package.json".to_owned()].into_iter().collect();
        assert_eq!(npm_rows(&project(), &collapsed).len(), 3);
    }
}
