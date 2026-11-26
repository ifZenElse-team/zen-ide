use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_run::CoverageCounts;
use zen_workbench::Workbench;

use super::glyphs::{glyph_close, glyph_export, glyph_minus};
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const INDENT: f32 = 16.0;
const PERCENT_COLUMN: f32 = 150.0;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Summary {
    run_id: u64,
    name: String,
    error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct FileRow {
    path: PathBuf,
    counts: CoverageCounts,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Report {
    shown: bool,
    loading: Option<u64>,
    revision: u64,
    summary: Option<Summary>,
    files: Vec<FileRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Node {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) file: bool,
    pub(super) counts: CoverageCounts,
    pub(super) children: Vec<Node>,
}

impl Node {
    fn folder(path: PathBuf, name: String) -> Self {
        Self { path, name, file: false, counts: CoverageCounts::default(), children: Vec::new() }
    }

    fn insert(&mut self, parts: &[String], path: &Path, counts: &CoverageCounts) {
        self.counts.add(counts);
        match parts {
            [] => {}
            [name] => self.children.push(Node {
                path: path.to_owned(),
                name: name.clone(),
                file: true,
                counts: *counts,
                children: Vec::new(),
            }),
            [folder, rest @ ..] => {
                let index = match self.children.iter().position(|child| !child.file && child.name == *folder) {
                    Some(index) => index,
                    None => {
                        self.children.push(Node::folder(self.path.join(folder), folder.clone()));
                        self.children.len() - 1
                    }
                };
                self.children[index].insert(rest, path, counts);
            }
        }
    }

    fn compact(mut self) -> Self {
        self.children = self.children.into_iter().map(Node::compact).collect();
        while !self.file && self.children.len() == 1 && !self.children[0].file && !self.name.is_empty() {
            let child = self.children.remove(0);
            self.name = format!("{}/{}", self.name, child.name);
            self.path = child.path;
            self.children = child.children;
        }
        self.children.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.name.cmp(&b.name)));
        self
    }
}

pub(super) fn tree(root: &Path, files: &[(PathBuf, CoverageCounts)]) -> Vec<Node> {
    let mut top = Node::folder(root.to_owned(), String::new());
    for (path, counts) in files {
        let relative = path.strip_prefix(root).unwrap_or(path);
        let parts: Vec<String> = relative
            .components()
            .filter_map(|part| match part {
                std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        top.insert(&parts, path, counts);
    }
    top.compact().children
}

pub(super) fn percent_label(counts: &CoverageCounts) -> String {
    t!(
        "ide-coverage-percent",
        percent = format!("{:.0}", counts.percent().floor()),
        hit = counts.hit(),
        total = counts.total()
    )
}

fn project_counts(root: &Path, files: &[(PathBuf, CoverageCounts)]) -> HashMap<PathBuf, CoverageCounts> {
    let mut totals: HashMap<PathBuf, CoverageCounts> = HashMap::new();
    for (path, counts) in files {
        totals.entry(path.clone()).or_default().add(counts);
        for folder in path.ancestors().skip(1).take_while(|folder| folder.starts_with(root) && *folder != root) {
            totals.entry(folder.to_owned()).or_default().add(counts);
        }
    }
    totals
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CoverageAction {
    HideWindow,
    HideCoverage,
    Export,
    Open(PathBuf),
}

#[derive(Debug, Default)]
pub(super) struct CoverageWindow {
    revision: Option<u64>,
    report: Report,
    nodes: Vec<Node>,
    project: HashMap<PathBuf, CoverageCounts>,
    expanded: BTreeSet<PathBuf>,
    selected: Option<PathBuf>,
    pub(super) rows: Vec<(PathBuf, Rect)>,
    pub(super) buttons: Vec<(&'static str, Rect)>,
}

impl CoverageWindow {
    pub(super) fn revision(&self) -> Option<u64> {
        self.revision
    }

    pub(super) fn project_percent(&self, path: &Path) -> Option<String> {
        let counts = self.project.get(path)?;
        Some(t!("ide-coverage-project-percent", percent = format!("{:.0}", counts.percent().floor())))
    }

    pub(super) fn update(&mut self, answer: Value, root: &Path) -> bool {
        let report: Report = serde_json::from_value(answer).unwrap_or_default();
        let before = self.report.summary.as_ref().map(|summary| summary.run_id);
        let now = report.summary.as_ref().map(|summary| summary.run_id);
        self.revision = Some(report.revision);
        let files: Vec<(PathBuf, CoverageCounts)> =
            report.files.iter().map(|file| (file.path.clone(), file.counts)).collect();
        self.nodes = tree(root, &files);
        self.project = project_counts(root, &files);
        let fresh = now.is_some() && now != before;
        if fresh {
            self.expanded.clear();
            let mut folders = Vec::new();
            collect_folders(&self.nodes, &mut folders);
            self.expanded.extend(folders);
        }
        self.report = report;
        fresh
    }

    fn header(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<CoverageAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-coverage-title"),
            theme::semibold(13.0),
            p.text,
        );
        if let Some(summary) = &self.report.summary {
            ui.painter().text(
                pos2(title.right() + 10.0, header.center().y),
                Align2::LEFT_CENTER,
                &summary.name,
                theme::regular(13.0),
                p.text_secondary,
            );
        }
        let shown = self.report.shown;
        let buttons: [(&'static str, String, super::glyphs::Glyph, bool, CoverageAction); 3] = [
            ("hide", t!("ide-run-hide"), glyph_minus, true, CoverageAction::HideWindow),
            ("hideCoverage", t!("ide-coverage-hide"), glyph_close, shown, CoverageAction::HideCoverage),
            ("export", t!("ide-coverage-export"), glyph_export, shown, CoverageAction::Export),
        ];
        self.buttons.clear();
        let mut x = header.right() - 18.0;
        for (name, label, glyph, enabled, action) in buttons {
            let rect = Rect::from_center_size(pos2(x, header.center().y), Vec2::splat(24.0));
            let response = ui.interact(rect, ui.scope_id().with(("coverage-button", name)), Sense::click());
            if enabled {
                hover(ui, &response, p);
            }
            glyph(ui.painter(), rect.shrink(6.0), if enabled { p.text_secondary } else { p.tab_border });
            hint(ui, &response, &label, None, Side::Below);
            if enabled && response.clicked() {
                actions.push(action);
            }
            self.buttons.push((name, rect));
            x -= 28.0;
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Vec<CoverageAction> {
        let mut actions = Vec::new();
        self.header(ui, p, &mut actions);
        let body = ui.available_rect_before_wrap();
        let message = match (&self.report.summary, self.report.loading) {
            (_, Some(_)) if self.nodes.is_empty() => Some(t!("ide-coverage-loading")),
            (Some(Summary { error: Some(error), .. }), _) => Some(error.clone()),
            (None, _) => Some(t!("ide-coverage-empty")),
            (Some(_), _) if self.nodes.is_empty() => Some(t!("ide-coverage-no-files")),
            _ => None,
        };
        self.rows.clear();
        if let Some(message) = message {
            let galley = ui.painter().layout(message, theme::regular(13.0), p.text_secondary, body.width() - 32.0);
            let at = Align2::CENTER_CENTER.anchor_size(body.center(), galley.size());
            ui.painter().galley(at.min, galley, p.text_secondary);
            ui.allocate_rect(body, Sense::hover());
            return actions;
        }
        let (columns, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        ui.painter().text(
            pos2(columns.left() + 12.0, columns.center().y),
            Align2::LEFT_CENTER,
            t!("ide-coverage-element"),
            theme::regular(12.5),
            p.text_secondary,
        );
        ui.painter().text(
            pos2(columns.right() - PERCENT_COLUMN, columns.center().y),
            Align2::LEFT_CENTER,
            t!("ide-coverage-lines"),
            theme::regular(12.5),
            p.text_secondary,
        );
        let mut shown = Vec::new();
        visible(&self.nodes, &self.expanded, 0, &mut shown);
        let mut toggled = None;
        egui::ScrollArea::vertical().id_salt("coverage-tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (node, depth) in &shown {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                let selected = self.selected.as_ref() == Some(&node.path);
                accessibility::item(&response, egui::Role::TreeItem, selected, &node.name);
                if selected {
                    ui.painter().rect_filled(rect, 4, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 4, p.hover);
                }
                let left = rect.left() + 12.0 + *depth as f32 * INDENT;
                if !node.file {
                    let open = self.expanded.contains(&node.path);
                    let arrow = Rect::from_center_size(pos2(left + 5.0, rect.center().y), Vec2::splat(9.0));
                    if open {
                        crate::icons::chevron_down(ui.painter(), arrow, p.text_secondary);
                    } else {
                        crate::icons::chevron_right(ui.painter(), arrow, p.text_secondary);
                    }
                }
                ui.painter().text(
                    pos2(left + 16.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &node.name,
                    theme::regular(13.0),
                    p.text,
                );
                ui.painter().text(
                    pos2(rect.right() - PERCENT_COLUMN, rect.center().y),
                    Align2::LEFT_CENTER,
                    percent_label(&node.counts),
                    theme::regular(13.0),
                    p.text_secondary,
                );
                if response.clicked() {
                    self.selected = Some(node.path.clone());
                    if node.file {
                        actions.push(CoverageAction::Open(node.path.clone()));
                    } else {
                        toggled = Some(node.path.clone());
                    }
                }
                self.rows.push((node.path.clone(), rect));
            }
        });
        if let Some(folder) = toggled
            && !self.expanded.remove(&folder)
        {
            self.expanded.insert(folder);
        }
        actions
    }
}

fn collect_folders(nodes: &[Node], out: &mut Vec<PathBuf>) {
    for node in nodes.iter().filter(|node| !node.file) {
        out.push(node.path.clone());
        collect_folders(&node.children, out);
    }
}

fn visible<'a>(nodes: &'a [Node], expanded: &BTreeSet<PathBuf>, depth: usize, out: &mut Vec<(&'a Node, usize)>) {
    for node in nodes {
        out.push((node, depth));
        if !node.file && expanded.contains(&node.path) {
            visible(&node.children, expanded, depth + 1, out);
        }
    }
}

impl IdeView {
    pub(super) fn follow_coverage(&mut self, registry: &Registry, session: &mut Session) {
        let revision =
            session.state::<zen_workbench::coverage::Coverage>().map(zen_workbench::coverage::Coverage::revision);
        if revision.is_none() || revision == self.coverage_window.revision() {
            return;
        }
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        let answer = registry.dispatch(session, "coverage.report.get", json!({})).unwrap_or_default();
        if self.coverage_window.update(answer, &root) {
            self.show_tool("coverage");
        }
    }

    pub(super) fn coverage_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        for action in self.coverage_window.show(ui, &p) {
            match action {
                CoverageAction::HideWindow => self.hide_tool("coverage"),
                CoverageAction::HideCoverage => self.dispatch(ui, registry, session, "coverage.report.hide", json!({})),
                CoverageAction::Open(path) => {
                    self.dispatch(ui, registry, session, "file.document.open", json!({ "path": path }));
                }
                CoverageAction::Export => self.export_coverage(ui, registry, session),
            }
        }
    }

    fn export_coverage(&mut self, ui: &Ui, registry: &Registry, session: &mut Session) {
        let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let mut dialog = rfd::FileDialog::new().set_title(t!("ide-coverage-export-folder"));
        if let Some(project) = &project {
            dialog = dialog.set_directory(project);
        }
        let Some(folder) = dialog.pick_folder() else { return };
        match registry.dispatch(session, "coverage.report.export", json!({ "folder": folder })) {
            Ok(answer) => {
                if let Some(index) = answer["index"].as_str() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file://{index}")));
                }
            }
            Err(error) => self.report(ui, error),
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, PointerButton, Pos2};

    use super::*;

    fn counts(covered: u32, partly: u32, not_covered: u32) -> CoverageCounts {
        CoverageCounts { covered, partly, not_covered, branches: None }
    }

    fn report() -> Value {
        json!({
            "shown": true,
            "revision": 3,
            "summary": { "runId": 7, "name": "go test calc", "percent": 50.0 },
            "files": [
                { "path": "/p/calc/add.go", "counts": { "covered": 3, "partly": 0, "notCovered": 1 } },
                { "path": "/p/calc/deep/x/sub.go", "counts": { "covered": 0, "partly": 1, "notCovered": 1 } },
                { "path": "/p/main.go", "counts": { "covered": 0, "partly": 0, "notCovered": 2 } },
            ],
        })
    }

    #[test]
    fn files_make_a_tree_of_folders_with_their_totals_and_single_folders_folded_together() {
        let files = [
            (PathBuf::from("/p/calc/add.go"), counts(3, 0, 1)),
            (PathBuf::from("/p/calc/deep/x/sub.go"), counts(0, 1, 1)),
            (PathBuf::from("/p/main.go"), counts(0, 0, 2)),
        ];
        let nodes = tree(Path::new("/p"), &files);
        let names: Vec<(&str, bool)> = nodes.iter().map(|node| (node.name.as_str(), node.file)).collect();
        assert_eq!(names, [("calc", false), ("main.go", true)], "folders first");
        let calc = &nodes[0];
        assert_eq!(calc.counts, counts(3, 1, 2));
        let inside: Vec<&str> = calc.children.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(inside, ["deep/x", "add.go"]);
        assert_eq!(calc.children[0].path, PathBuf::from("/p/calc/deep/x"));
        zen_i18n::register(crate::toolbox::LOCALES);
        assert_eq!(percent_label(&calc.counts), t!("ide-coverage-percent", percent = "66", hit = 4, total = 6));
    }

    #[test]
    fn the_project_tree_shows_percentages_of_files_and_their_folders_until_coverage_is_hidden() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let root = Path::new("/p");
        let mut window = CoverageWindow::default();
        window.update(report(), root);
        let percent = |percent: &str| Some(t!("ide-coverage-project-percent", percent = percent));
        assert_eq!(window.project_percent(Path::new("/p/calc/add.go")), percent("75"));
        assert_eq!(window.project_percent(Path::new("/p/calc")), percent("66"), "a folder sums what is inside");
        assert_eq!(window.project_percent(Path::new("/p/calc/deep/x")), percent("50"));
        assert_eq!(window.project_percent(Path::new("/p/README.md")), None, "a file not measured shows nothing");
        assert_eq!(window.project_percent(root), None, "the project's row keeps its path");
        window.update(json!({ "shown": false, "revision": 4, "files": [] }), root);
        assert_eq!(window.project_percent(Path::new("/p/calc/add.go")), None);
    }

    struct Harness {
        ctx: egui::Context,
        window: CoverageWindow,
        actions: Vec<CoverageAction>,
        time: f64,
    }

    impl Harness {
        fn new(answer: Value) -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut window = CoverageWindow::default();
            assert!(window.update(answer, Path::new("/p")), "a new suite shows the window");
            let mut harness = Self { ctx, window, actions: Vec::new(), time: 0.0 };
            harness.frame(Vec::new());
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.01;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(500.0, 400.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                self.actions.extend(self.window.show(ui, &IdePalette::zen_dark()));
            });
            output.textures_delta.clear();
        }

        fn click(&mut self, at: Pos2) {
            self.time += 1.0;
            self.frame(vec![Event::PointerMoved(at)]);
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }

        fn rows(&self) -> Vec<PathBuf> {
            self.window.rows.iter().map(|(path, _)| path.clone()).collect()
        }

        fn row(&self, path: &str) -> Rect {
            self.window.rows.iter().find(|(shown, _)| shown == Path::new(path)).map(|(_, rect)| *rect).unwrap()
        }

        fn button(&self, name: &str) -> Rect {
            self.window.buttons.iter().find(|(shown, _)| *shown == name).map(|(_, rect)| *rect).unwrap()
        }
    }

    #[test]
    fn the_tree_opens_files_folds_folders_and_its_buttons_hide_and_export() {
        let mut harness = Harness::new(report());
        assert_eq!(
            harness.rows(),
            ["/p/calc", "/p/calc/deep/x", "/p/calc/deep/x/sub.go", "/p/calc/add.go", "/p/main.go"].map(PathBuf::from),
            "a new suite opens every folder"
        );
        harness.click(harness.row("/p/calc/add.go").center());
        assert_eq!(harness.actions, [CoverageAction::Open(PathBuf::from("/p/calc/add.go"))]);
        harness.click(harness.row("/p/calc").center());
        harness.frame(Vec::new());
        assert_eq!(harness.rows(), ["/p/calc", "/p/main.go"].map(PathBuf::from));
        harness.actions.clear();
        for button in ["export", "hideCoverage", "hide"] {
            harness.click(harness.button(button).center());
        }
        assert_eq!(harness.actions, [CoverageAction::Export, CoverageAction::HideCoverage, CoverageAction::HideWindow]);
        assert!(!harness.window.update(report(), Path::new("/p")), "the same suite does not show the window again");
        assert_eq!(harness.window.expanded.len(), 1, "and keeps what was folded");
    }

    #[test]
    fn without_coverage_the_window_says_so_and_cannot_export() {
        let mut window = CoverageWindow::default();
        assert!(!window.update(json!({ "shown": false, "revision": 1, "files": [] }), Path::new("/p")));
        let mut harness = Harness::new(report());
        harness.window = window;
        harness.frame(Vec::new());
        assert!(harness.window.rows.is_empty());
        harness.click(harness.button("export").center());
        assert!(harness.actions.is_empty(), "nothing to export");
    }
}
