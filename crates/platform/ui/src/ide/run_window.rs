use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{
    Align2, Color32, CursorIcon, Event, FontId, Galley, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, Ui,
    UiBuilder, Vec2, pos2, vec2,
};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_run::{ConsoleLine, LinkSpan, RunState, Stream, TestCounts, TestNode, TestNodeKind, TestSource, TestStatus};
use zen_workbench::Workbench;
use zen_workbench::run::Runs;

use super::glyphs::{
    Glyph, glyph_minus, glyph_rerun, glyph_rerun_failed, glyph_run, glyph_show_ignored, glyph_show_passed, glyph_stop,
    glyph_test_failed, glyph_test_history, glyph_test_passed, glyph_test_running, glyph_test_skipped,
    glyph_test_terminated,
};
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const CONSOLE_ROW: f32 = 20.0;
const CONSOLE_INSET: f32 = 8.0;
const CONSOLE_FONT: f32 = 13.0;
const TOOLBAR: f32 = 40.0;
const BUTTON: f32 = 26.0;
const COUNTS: f32 = 30.0;
const BENCHMARK_ROW: f32 = 22.0;
const BENCHMARK_CELL: f32 = 100.0;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunInfo {
    run_id: u64,
    name: String,
    #[serde(default)]
    configuration_id: String,
    #[serde(default)]
    debug: bool,
    #[serde(default)]
    replaces: Option<u64>,
    #[serde(default = "shown_by_default")]
    activate_tool_window: bool,
    #[serde(default)]
    focus_tool_window: bool,
    #[serde(default)]
    external_tool: Option<ToolConsole>,
}

fn shown_by_default() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolConsole {
    open_console: bool,
    activate_on_stdout: bool,
    activate_on_stderr: bool,
    #[serde(default)]
    console_on_error: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct RunList {
    runs: Vec<RunInfo>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutputAnswer {
    lines: Vec<ConsoleLine>,
    next: usize,
    state: RunState,
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct TestsAnswer {
    pub tree: TestNode,
    pub counts: TestCounts,
    pub done: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunStarted {
    run_id: u64,
}

type NodeKey = (String, Option<String>);

fn key_of(node: &TestNode) -> NodeKey {
    (node.package.clone(), node.test.clone())
}

fn find<'a>(node: &'a TestNode, key: &NodeKey) -> Option<&'a TestNode> {
    if node.package == key.0 && node.test == key.1 {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, key))
}

fn first_failed(node: &TestNode) -> Option<&TestNode> {
    node.children
        .iter()
        .find(|child| child.status == TestStatus::Failed)
        .map(|child| first_failed(child).unwrap_or(child))
}

fn failed_first(node: &TestNode, done: bool) -> Vec<&TestNode> {
    let mut children: Vec<&TestNode> = node.children.iter().collect();
    if done {
        children.sort_by_key(|child| child.status != TestStatus::Failed);
    }
    children
}

fn ancestors(key: &NodeKey) -> Vec<NodeKey> {
    let mut keys = vec![(String::new(), None)];
    if key.0.is_empty() {
        return keys;
    }
    keys.push((key.0.clone(), None));
    if let Some(test) = &key.1 {
        let mut full = String::new();
        for part in test.split('/') {
            full = if full.is_empty() { part.to_owned() } else { format!("{full}/{part}") };
            if full != *test {
                keys.push((key.0.clone(), Some(full.clone())));
            }
        }
    }
    keys
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Spot {
    pub row: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Shown {
    All(Range<usize>),
    Ranges(Vec<Range<usize>>),
}

impl Shown {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::All(range) => range.len(),
            Self::Ranges(ranges) => ranges.iter().map(|range| range.len()).sum(),
        }
    }

    pub(super) fn line(&self, row: usize) -> Option<usize> {
        match self {
            Self::All(range) => (row < range.len()).then(|| range.start + row),
            Self::Ranges(ranges) => {
                let mut row = row;
                for range in ranges {
                    if row < range.len() {
                        return Some(range.start + row);
                    }
                    row -= range.len();
                }
                None
            }
        }
    }
}

pub(super) fn line_job(line: &ConsoleLine, p: &IdePalette) -> LayoutJob {
    let color = match line.stream {
        Stream::Stdout => p.text,
        Stream::Stderr => error_color(p),
        Stream::System => p.text_secondary,
    };
    let font = FontId::monospace(CONSOLE_FONT);
    let plain = TextFormat { font_id: font.clone(), color, ..TextFormat::default() };
    let link = TextFormat { font_id: font, color: p.link, ..TextFormat::default() };
    let byte = |index: usize| line.text.char_indices().nth(index).map_or(line.text.len(), |(byte, _)| byte);
    let mut job = LayoutJob::default();
    let mut at = 0;
    for span in &line.links {
        let (start, end) = (byte(span.range.start).max(at), byte(span.range.end));
        if start >= end {
            continue;
        }
        job.append(&line.text[at..start], 0.0, plain.clone());
        job.append(&line.text[start..end], 0.0, link.clone());
        at = end;
    }
    job.append(&line.text[at..], 0.0, plain);
    job
}

pub(super) fn error_color(p: &IdePalette) -> Color32 {
    p.problem("error")
}

fn warning_color(p: &IdePalette) -> Color32 {
    p.problem("warning")
}

pub(super) fn status_look(status: TestStatus, p: &IdePalette) -> (Glyph, Color32) {
    match status {
        TestStatus::Running => (glyph_test_running, p.accent),
        TestStatus::Passed => (glyph_test_passed, p.run),
        TestStatus::Failed => (glyph_test_failed, error_color(p)),
        TestStatus::Skipped => (glyph_test_skipped, p.text_secondary),
        TestStatus::Terminated => (glyph_test_terminated, warning_color(p)),
    }
}

pub(super) fn duration_text(ms: u64) -> String {
    if ms < 1000 {
        t!("ide-run-duration-ms", ms = ms)
    } else {
        t!("ide-run-duration-seconds", seconds = ms / 1000, ms = ms % 1000)
    }
}

pub(super) fn counts_text(tests: &TestsAnswer) -> String {
    let counts = &tests.counts;
    let text = if counts.failed > 0 {
        t!("ide-run-tests-failed", failed = counts.failed, passed = counts.passed, total = counts.total)
    } else {
        t!("ide-run-tests-passed", passed = counts.passed, total = counts.total)
    };
    match tests.tree.duration_ms {
        Some(ms) if tests.done => t!("ide-run-tests-summary", counts = text, duration = duration_text(ms)),
        _ => text,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct BenchmarkTable {
    pub(super) units: Vec<String>,
    pub(super) rows: Vec<BenchmarkRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BenchmarkRow {
    pub(super) test: String,
    pub(super) iterations: String,
    pub(super) cells: Vec<String>,
}

fn collect_benchmarks<'a>(node: &'a TestNode, out: &mut Vec<&'a TestNode>) {
    if node.iterations.is_some() {
        out.push(node);
    }
    node.children.iter().for_each(|child| collect_benchmarks(child, out));
}

pub(super) fn benchmark_table(tree: &TestNode) -> BenchmarkTable {
    let mut nodes = Vec::new();
    collect_benchmarks(tree, &mut nodes);
    let mut units: Vec<String> = Vec::new();
    for measure in nodes.iter().flat_map(|node| &node.measures) {
        if !units.contains(&measure.unit) {
            units.push(measure.unit.clone());
        }
    }
    let rows = nodes
        .iter()
        .map(|node| BenchmarkRow {
            test: node.test.clone().unwrap_or_default(),
            iterations: node.iterations.unwrap_or_default().to_string(),
            cells: units
                .iter()
                .map(|unit| {
                    let found = node.measures.iter().find(|measure| measure.unit == *unit);
                    found.map(|measure| measure.value.clone()).unwrap_or_default()
                })
                .collect(),
        })
        .collect();
    BenchmarkTable { units, rows }
}

pub(super) fn resolve_link(link: &LinkSpan, cwd: &Path, root: &Path) -> Option<PathBuf> {
    let candidates =
        if link.file.is_absolute() { vec![link.file.clone()] } else { vec![link.resolve(cwd), root.join(&link.file)] };
    candidates.into_iter().find(|path| path.is_file())
}

pub(super) fn byte_to_char_column(line: &str, byte_column: u32) -> usize {
    let byte = (byte_column.max(1) as usize - 1).min(line.len());
    line.char_indices().take_while(|(at, _)| *at < byte).count() + 1
}

pub(super) fn follows_end(offset: f32, max_offset: f32) -> bool {
    offset >= max_offset - 1.0
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum RunAction {
    Hide,
    Rerun { run_id: u64, running: bool },
    Stop { run_id: u64 },
    RerunFailed { run_id: u64 },
    RerunTest { run_id: u64, package: String, test: Option<String> },
    OpenTest { run_id: u64, package: String, test: String },
    Close { run_id: u64, running: bool },
    Open { path: PathBuf, line: u32, column: Option<u32> },
    History { at: Pos2 },
}

impl RunAction {
    pub(super) fn commands(&self) -> Vec<(&'static str, Value)> {
        let stop = |run_id: u64| ("run.process.stop", json!({ "runId": run_id }));
        match self {
            Self::Hide | Self::History { .. } => Vec::new(),
            Self::Rerun { run_id, running } => {
                let mut commands = if *running { vec![stop(*run_id)] } else { Vec::new() };
                commands.push(("run.process.rerun", json!({ "runId": run_id })));
                commands
            }
            Self::Stop { run_id } => vec![stop(*run_id)],
            Self::RerunFailed { run_id } => vec![("run.tests.rerunFailed", json!({ "runId": run_id }))],
            Self::RerunTest { run_id, package, test } => {
                vec![("run.tests.rerun", json!({ "runId": run_id, "package": package, "test": test }))]
            }
            Self::OpenTest { run_id, package, test } => {
                vec![("run.tests.source", json!({ "runId": run_id, "package": package, "test": test }))]
            }
            Self::Close { run_id, running } => {
                if *running {
                    vec![stop(*run_id)]
                } else {
                    Vec::new()
                }
            }
            Self::Open { path, .. } => vec![("file.document.open", json!({ "path": path }))],
        }
    }
}

struct Laid {
    row: usize,
    index: usize,
    rect: Rect,
    galley: Arc<Galley>,
    at: Pos2,
}

struct NodeRow {
    key: NodeKey,
    depth: usize,
    name: String,
    status: TestStatus,
    duration: Option<u64>,
    parent: bool,
}

fn test_menu(ui: &mut Ui, run_id: u64, row: &NodeRow, running: bool, actions: &mut Vec<RunAction>) {
    let label = t!("ide-run-test-rerun", name = row.name.as_str());
    if ui.add_enabled(!running, egui::Button::new(label)).clicked() {
        actions.push(RunAction::RerunTest { run_id, package: row.key.0.clone(), test: row.key.1.clone() });
        ui.close();
    }
}

#[derive(Debug)]
pub(super) struct RunTab {
    run_id: u64,
    configuration_id: String,
    name: String,
    cwd: PathBuf,
    root: PathBuf,
    state: RunState,
    error: Option<String>,
    console: Console,
    tests: Option<TestsAnswer>,
    seen: Option<u64>,
    selected: Option<NodeKey>,
    collapsed: BTreeSet<NodeKey>,
    picked_failed: bool,
    show_passed: bool,
    show_ignored: bool,
    reveal: bool,
    tree_rows: Vec<(NodeKey, Rect)>,
    benchmark_rows: Vec<String>,
    activate: (bool, bool),
    wants_front: bool,
}

impl RunTab {
    fn new(info: &RunInfo, cwd: PathBuf, root: PathBuf) -> Self {
        Self {
            run_id: info.run_id,
            configuration_id: info.configuration_id.clone(),
            name: info.name.clone(),
            cwd,
            root,
            state: RunState::Running,
            error: None,
            console: Console::default(),
            tests: None,
            seen: None,
            selected: None,
            collapsed: BTreeSet::new(),
            picked_failed: false,
            show_passed: true,
            show_ignored: true,
            reveal: false,
            tree_rows: Vec::new(),
            benchmark_rows: Vec::new(),
            activate: info
                .external_tool
                .map_or((false, false), |tool| (tool.activate_on_stdout, tool.activate_on_stderr)),
            wants_front: false,
        }
    }

    fn restart(&mut self, run_id: u64) {
        let info = RunInfo {
            run_id,
            name: self.name.clone(),
            configuration_id: self.configuration_id.clone(),
            debug: false,
            replaces: None,
            activate_tool_window: true,
            focus_tool_window: false,
            external_tool: None,
        };
        let (show_passed, show_ignored, activate) = (self.show_passed, self.show_ignored, self.activate);
        *self = Self { show_passed, show_ignored, activate, ..Self::new(&info, self.cwd.clone(), self.root.clone()) };
    }

    fn is_running(&self) -> bool {
        self.state == RunState::Running
    }

    fn has_tests(&self) -> bool {
        self.tests.as_ref().is_some_and(|tests| !tests.tree.children.is_empty())
    }

    fn apply_output(&mut self, answer: OutputAnswer) {
        let (stdout, stderr) = self.activate;
        self.wants_front |= answer
            .lines
            .iter()
            .any(|line| (stdout && line.stream == Stream::Stdout) || (stderr && line.stream == Stream::Stderr));
        self.console.extend(answer.lines, answer.next);
        self.state = answer.state;
        self.error = answer.error;
    }

    fn apply_tests(&mut self, answer: TestsAnswer) {
        let done = answer.done;
        self.tests = Some(answer);
        if !done || self.picked_failed {
            return;
        }
        self.picked_failed = true;
        let unselected = self.selected.as_ref().is_none_or(|key| key.0.is_empty());
        let failed = self.tests.as_ref().and_then(|tests| first_failed(&tests.tree)).map(key_of);
        if let (true, Some(key)) = (unselected, failed) {
            for ancestor in ancestors(&key) {
                self.collapsed.remove(&ancestor);
            }
            self.selected = Some(key);
            self.console.clear_selection();
            self.reveal = true;
        }
    }

    pub(super) fn shown(&self) -> Shown {
        let all = self.console.all();
        let node = self
            .selected
            .as_ref()
            .filter(|key| !key.0.is_empty())
            .and_then(|key| self.tests.as_ref().and_then(|tests| find(&tests.tree, key)));
        match node {
            Some(node) => {
                let clamp = |index: usize| index.clamp(all.start, all.end);
                Shown::Ranges(node.lines.iter().map(|range| clamp(range.start)..clamp(range.end)).collect())
            }
            None => Shown::All(all),
        }
    }

    fn hidden(&self, node: &TestNode) -> bool {
        let filtered = match node.status {
            TestStatus::Passed => !self.show_passed,
            TestStatus::Skipped => !self.show_ignored,
            _ => false,
        };
        node.kind != TestNodeKind::Root && filtered && node.children.iter().all(|child| self.hidden(child))
    }

    fn collect(&self, node: &TestNode, depth: usize, rows: &mut Vec<NodeRow>) {
        if self.hidden(node) {
            return;
        }
        let key = key_of(node);
        let name = if node.kind == TestNodeKind::Root { t!("ide-run-test-results") } else { node.name.clone() };
        let expanded = !self.collapsed.contains(&key);
        rows.push(NodeRow {
            key,
            depth,
            name,
            status: node.status,
            duration: node.duration_ms,
            parent: !node.children.is_empty(),
        });
        if expanded {
            let done = self.tests.as_ref().is_some_and(|tests| tests.done);
            failed_first(node, done).into_iter().for_each(|child| self.collect(child, depth + 1, rows));
        }
    }

    fn node_rows(&self) -> Vec<NodeRow> {
        let mut rows = Vec::new();
        if let Some(tests) = &self.tests {
            self.collect(&tests.tree, 0, &mut rows);
        }
        rows
    }

    fn source_of(&self, key: &NodeKey) -> Option<RunAction> {
        let node = find(&self.tests.as_ref()?.tree, key)?;
        let lines = node.lines.iter().flat_map(|range| range.clone());
        let failure = lines.filter_map(|index| self.console.lines.get(index)).find_map(|line| {
            line.links.iter().find_map(|link| {
                resolve_link(link, &self.cwd, &self.root).map(|path| RunAction::Open {
                    path,
                    line: link.line,
                    column: link.column,
                })
            })
        });
        failure.or_else(|| {
            let (package, test) = (key.0.clone(), key.1.clone()?);
            Some(RunAction::OpenTest { run_id: self.run_id, package, test })
        })
    }

    fn toggle(&mut self, key: &NodeKey) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.clone());
        }
    }

    fn select(&mut self, key: NodeKey) {
        if self.selected.as_ref() != Some(&key) {
            self.selected = Some(key);
            self.console.clear_selection();
        }
    }

    fn status_look(&self, p: &IdePalette) -> (Glyph, Color32) {
        match &self.tests {
            Some(tests) if !tests.tree.children.is_empty() => status_look(tests.tree.status, p),
            _ => match self.state {
                RunState::Running => (glyph_run, p.run),
                RunState::Failed | RunState::BeforeLaunchFailed => (glyph_test_failed, error_color(p)),
                RunState::Finished | RunState::Stopped => (glyph_run, p.text_secondary),
            },
        }
    }

    #[cfg(test)]
    fn selected_text(&self) -> Option<String> {
        self.console.selected_text(&self.shown())
    }

    fn show(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<RunAction>) {
        let body = ui.available_rect_before_wrap();
        let toolbar = Rect::from_min_max(body.min, pos2(body.left() + TOOLBAR, body.bottom()));
        self.toolbar(ui, p, toolbar, actions);
        let rest = Rect::from_min_max(pos2(toolbar.right(), body.top()), body.max);
        if self.has_tests() {
            let width = (rest.width() * 0.35).clamp(180.0, 420.0);
            let tree = Rect::from_min_max(rest.min, pos2(rest.left() + width, rest.bottom()));
            ui.painter().vline(tree.right() + 0.5, rest.y_range(), Stroke::new(1.0, p.frame));
            ui.scope_builder(UiBuilder::new().max_rect(tree), |ui| self.tree(ui, p, actions));
            let counts =
                Rect::from_min_max(pos2(tree.right() + 1.0, rest.top()), pos2(rest.right(), rest.top() + COUNTS));
            self.counts(ui, p, counts);
            let console = Rect::from_min_max(pos2(counts.left(), counts.bottom()), rest.max);
            let console = self.benchmarks(ui, p, console);
            self.error_and_console(ui, p, console, actions);
        } else {
            self.error_and_console(ui, p, rest, actions);
        }
        ui.allocate_rect(body, Sense::hover());
    }

    fn benchmarks(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect) -> Rect {
        let table = self.tests.as_ref().map(|tests| benchmark_table(&tests.tree)).unwrap_or_default();
        self.benchmark_rows = table.rows.iter().map(|row| row.test.clone()).collect();
        if table.rows.is_empty() {
            return rect;
        }
        let wanted = (table.rows.len() + 1) as f32 * BENCHMARK_ROW;
        let height = wanted.min(rect.height() * 0.5);
        let area = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + height));
        ui.painter().hline(area.x_range(), area.bottom() + 0.5, Stroke::new(1.0, p.frame));
        let columns = table.units.len() + 1;
        let right = |rect: Rect, column: usize| rect.right() - 12.0 - (columns - 1 - column) as f32 * BENCHMARK_CELL;
        let row_painter = |ui: &Ui, row: Rect, name: &str, cells: &[&str], color| {
            let painter = ui.painter().with_clip_rect(row);
            let at = pos2(row.left() + 12.0, row.center().y);
            painter.text(at, Align2::LEFT_CENTER, name, theme::regular(12.5), color);
            for (column, cell) in cells.iter().enumerate() {
                let at = pos2(right(row, column), row.center().y);
                painter.text(at, Align2::RIGHT_CENTER, cell, theme::regular(12.5), color);
            }
        };
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            let (header, _) = ui.allocate_exact_size(vec2(area.width(), BENCHMARK_ROW), Sense::hover());
            let iterations = t!("ide-run-benchmark-iterations");
            let titles: Vec<&str> =
                std::iter::once(iterations.as_str()).chain(table.units.iter().map(String::as_str)).collect();
            row_painter(ui, header, &t!("ide-run-benchmark-name"), &titles, p.text_secondary);
            egui::ScrollArea::vertical().id_salt("run-benchmarks").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in &table.rows {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), BENCHMARK_ROW), Sense::hover());
                    let cells: Vec<&str> =
                        std::iter::once(row.iterations.as_str()).chain(row.cells.iter().map(String::as_str)).collect();
                    row_painter(ui, rect, &row.test, &cells, p.text);
                    accessibility::item(
                        &response,
                        egui::Role::Row,
                        false,
                        &format!("{} {}", row.test, cells.join(" ")),
                    );
                }
            });
        });
        Rect::from_min_max(pos2(rect.left(), area.bottom() + 1.0), rect.max)
    }

    fn error_and_console(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect, actions: &mut Vec<RunAction>) {
        let mut console = rect;
        if let Some(error) = &self.error {
            let strip = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + COUNTS));
            ui.painter().with_clip_rect(strip).text(
                pos2(strip.left() + CONSOLE_INSET, strip.center().y),
                Align2::LEFT_CENTER,
                error,
                theme::regular(13.0),
                error_color(p),
            );
            console.min.y = strip.bottom();
        }
        ui.scope_builder(UiBuilder::new().max_rect(console), |ui| self.console(ui, p, actions));
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect, actions: &mut Vec<RunAction>) {
        let running = self.is_running();
        let run_id = self.run_id;
        let slot = |index: usize| {
            let y = rect.top() + 6.0 + BUTTON / 2.0 + index as f32 * (BUTTON + 4.0);
            Rect::from_center_size(pos2(rect.center().x, y), Vec2::splat(BUTTON))
        };
        if Tool::new("run-rerun", glyph_rerun, t!("ide-run-rerun"), p.run).show(ui, p, slot(0), true) {
            actions.push(RunAction::Rerun { run_id, running });
        }
        let stop = Tool {
            shortcut: crate::keymap::shortcut("run.process.stop"),
            ..Tool::new("run-stop", glyph_stop, t!("ide-run-stop"), error_color(p))
        };
        if stop.show(ui, p, slot(1), running) {
            actions.push(RunAction::Stop { run_id });
        }
        if !self.has_tests() {
            return;
        }
        let failed = self.tests.as_ref().is_some_and(|tests| tests.counts.failed > 0);
        let rerun_failed =
            Tool::new("run-rerun-failed", glyph_rerun_failed, t!("ide-run-rerun-failed"), error_color(p));
        if rerun_failed.show(ui, p, slot(2), !running && failed) {
            actions.push(RunAction::RerunFailed { run_id });
        }
        let line = slot(3);
        ui.painter().hline(line.shrink2(vec2(4.0, 0.0)).x_range(), line.top() + 2.0, Stroke::new(1.0, p.tab_border));
        let passed = line.translate(vec2(0.0, 6.0));
        let show_passed = Tool {
            on: Some(self.show_passed),
            ..Tool::new("run-show-passed", glyph_show_passed, t!("ide-run-show-passed"), p.text)
        };
        if show_passed.show(ui, p, passed, true) {
            self.show_passed = !self.show_passed;
        }
        let show_ignored = Tool {
            on: Some(self.show_ignored),
            ..Tool::new("run-show-ignored", glyph_show_ignored, t!("ide-run-show-ignored"), p.text)
        };
        if show_ignored.show(ui, p, passed.translate(vec2(0.0, BUTTON + 4.0)), true) {
            self.show_ignored = !self.show_ignored;
        }
        let history = passed.translate(vec2(0.0, 2.0 * (BUTTON + 4.0) + 6.0));
        if Tool::new("run-test-history", glyph_test_history, t!("ide-test-history"), p.text).show(ui, p, history, true)
        {
            actions.push(RunAction::History { at: history.right_top() + vec2(4.0, 0.0) });
        }
    }

    fn counts(&self, ui: &Ui, p: &IdePalette, rect: Rect) {
        let Some(tests) = &self.tests else { return };
        let (glyph, color) = status_look(tests.tree.status, p);
        glyph(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(14.0)),
            color,
        );
        ui.painter().with_clip_rect(rect).text(
            pos2(rect.left() + 34.0, rect.center().y),
            Align2::LEFT_CENTER,
            counts_text(tests),
            theme::regular(13.0),
            p.text,
        );
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<RunAction>) {
        let id = ui.make_persistent_id("run-test-tree");
        let area = ui.interact(ui.max_rect(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("a11y-tests"));
        if area.clicked() {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        let focused = ui.memory(|memory| memory.has_focus(id));
        if focused {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: false },
                );
            });
            self.tree_keys(ui, actions);
        }
        let rows = self.node_rows();
        let mut clicked: Option<(NodeKey, bool, bool, bool)> = None;
        let mut drawn = Vec::with_capacity(rows.len());
        egui::ScrollArea::vertical().id_salt(("run-tree", self.run_id)).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(4.0);
            for row in &rows {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                drawn.push((row.key.clone(), rect));
                let expanded = row.parent.then(|| !self.collapsed.contains(&row.key));
                let selected = self.selected.as_ref() == Some(&row.key);
                accessibility::tree_item(&response, row.depth + 1, expanded, selected, &row.name);
                let highlight = rect.shrink2(vec2(4.0, 0.0));
                if self.selected.as_ref() == Some(&row.key) {
                    ui.painter().rect_filled(highlight, 4, if focused { p.selection } else { p.hover });
                    if std::mem::take(&mut self.reveal) {
                        response.scroll_to_me(None);
                    }
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 4, p.hover);
                }
                let left = rect.left() + 8.0 + row.depth as f32 * metrics::TREE_INDENT;
                let chevron = Rect::from_center_size(pos2(left + 6.0, rect.center().y), Vec2::splat(12.0));
                if row.parent {
                    if self.collapsed.contains(&row.key) {
                        crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
                    } else {
                        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
                    }
                }
                let (glyph, color) = status_look(row.status, p);
                glyph(
                    ui.painter(),
                    Rect::from_center_size(pos2(left + 22.0, rect.center().y), Vec2::splat(14.0)),
                    color,
                );
                let painter = ui.painter().with_clip_rect(rect);
                let duration = row.duration.map(duration_text).map(|text| {
                    painter.text(
                        pos2(rect.right() - 10.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        text,
                        theme::regular(12.5),
                        p.text_secondary,
                    )
                });
                let name_clip = Rect::from_min_max(
                    rect.min,
                    pos2(duration.map_or(rect.right(), |shown| shown.left() - 8.0), rect.bottom()),
                );
                painter.with_clip_rect(name_clip).text(
                    pos2(left + 34.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &row.name,
                    theme::regular(13.0),
                    p.text,
                );
                if !row.key.0.is_empty() {
                    let running = self.is_running();
                    response.context_menu(|ui| test_menu(ui, self.run_id, row, running, actions));
                }
                if response.clicked() || response.double_clicked() {
                    let on_chevron =
                        row.parent && response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    clicked = Some((row.key.clone(), response.double_clicked(), on_chevron, row.parent));
                }
            }
        });
        self.tree_rows = drawn;
        if let Some((key, double, on_chevron, parent)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            self.select(key.clone());
            if on_chevron || (double && parent && key.1.is_none()) {
                self.toggle(&key);
            } else if double {
                actions.extend(self.source_of(&key));
            }
        }
    }

    fn tree_keys(&mut self, ui: &mut Ui, actions: &mut Vec<RunAction>) {
        let none = Modifiers::NONE;
        let (up, down, left, right, enter) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Enter),
            )
        });
        let rows = self.node_rows();
        let at = self.selected.as_ref().and_then(|selected| rows.iter().position(|row| row.key == *selected));
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(rows.len().saturating_sub(1)),
            };
            if let Some(row) = rows.get(next) {
                self.select(row.key.clone());
                self.reveal = true;
            }
        }
        let Some(row) = at.and_then(|at| rows.get(at)) else { return };
        let collapsed = self.collapsed.contains(&row.key);
        if left && row.parent && !collapsed || right && row.parent && collapsed {
            self.toggle(&row.key.clone());
        } else if left && row.depth > 0 {
            let parent = rows[..at.unwrap_or(0)].iter().rev().find(|candidate| candidate.depth < row.depth);
            if let Some(parent) = parent {
                self.select(parent.key.clone());
                self.reveal = true;
            }
        } else if enter {
            actions.extend(self.source_of(&row.key.clone()));
        }
    }

    fn console(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<RunAction>) {
        let shown = self.shown();
        let id = ui.make_persistent_id(("run-console", self.run_id));
        let scroll = id.with(self.selected.clone());
        if let Some(link) = self.console.show(ui, p, id, scroll, &shown)
            && let Some(path) = resolve_link(&link, &self.cwd, &self.root)
        {
            actions.push(RunAction::Open { path, line: link.line, column: link.column });
        }
    }
}

#[derive(Debug)]
pub(super) struct Console {
    lines: zen_run::Console,
    next: usize,
    widest: usize,
    follow: bool,
    selection: Option<(Spot, Spot)>,
    rows: Vec<(usize, Rect)>,
    announcer: accessibility::Announcer,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            lines: zen_run::Console::default(),
            next: 0,
            widest: 0,
            follow: true,
            selection: None,
            rows: Vec::new(),
            announcer: accessibility::Announcer::default(),
        }
    }
}

impl Console {
    #[cfg(test)]
    pub(super) fn lines(&self) -> &[ConsoleLine] {
        self.lines.lines()
    }

    pub(super) fn all(&self) -> Range<usize> {
        self.lines.first()..self.lines.len()
    }

    pub(super) fn set_limit(&mut self, bytes: usize) {
        self.lines.set_limit(bytes);
    }

    pub(super) fn next(&self) -> usize {
        self.next
    }

    #[cfg(test)]
    pub(super) fn rows(&self) -> &[(usize, Rect)] {
        &self.rows
    }

    pub(super) fn extend(&mut self, lines: Vec<ConsoleLine>, next: usize) {
        self.widest = lines.iter().map(|line| line.text.chars().count()).fold(self.widest, usize::max);
        let start = next.saturating_sub(lines.len());
        for (offset, line) in lines.into_iter().enumerate() {
            self.lines.push_at(start + offset, line);
        }
        self.next = next;
    }

    pub(super) fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub(super) fn selected_text(&self, shown: &Shown) -> Option<String> {
        let (from, to) = self.selection.map(|(a, b)| if a <= b { (a, b) } else { (b, a) })?;
        if from == to {
            return None;
        }
        let mut text = Vec::new();
        for row in from.row..=to.row {
            let Some(line) = shown.line(row).and_then(|index| self.lines.get(index)) else { continue };
            let start = if row == from.row { from.column } else { 0 };
            let end = if row == to.row { to.column } else { usize::MAX };
            text.push(line.text.chars().skip(start).take(end.saturating_sub(start)).collect::<String>());
        }
        Some(text.join("\n"))
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        id: egui::Id,
        scroll: egui::Id,
        shown: &Shown,
    ) -> Option<LinkSpan> {
        let total = shown.len();
        let mut clicked = None;
        let char_width = CONSOLE_FONT * 0.6;
        let width = self.widest as f32 * char_width + CONSOLE_INSET * 2.0 + 40.0;
        let lines = &self.lines;
        let output = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                egui::ScrollArea::both()
                    .id_salt(scroll)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .scroll_source(egui::scroll_area::ScrollSource {
                        drag: egui::scroll_area::DragScroll::Never,
                        ..Default::default()
                    })
                    .show_rows(ui, CONSOLE_ROW, total, |ui, range| {
                        ui.set_min_width(width);
                        let painter = ui.painter().clone();
                        let background = painter.add(Shape::Noop);
                        let top = ui.max_rect().top();
                        let mut laid = Vec::new();
                        for row in range.clone() {
                            let Some(index) = shown.line(row) else { continue };
                            let Some(line) = lines.get(index) else { continue };
                            let rect = Rect::from_min_size(
                                pos2(ui.max_rect().left(), top + (row - range.start) as f32 * CONSOLE_ROW),
                                vec2(ui.max_rect().width().max(ui.clip_rect().width()), CONSOLE_ROW),
                            );
                            let galley = painter.layout_job(line_job(line, p));
                            let at = pos2(rect.left() + CONSOLE_INSET, rect.center().y - galley.size().y / 2.0);
                            painter.galley(at, Arc::clone(&galley), p.text);
                            laid.push(Laid { row, index, rect, galley, at });
                        }
                        let response = ui.interact(ui.clip_rect(), id, Sense::click_and_drag());
                        (painter, background, laid, response, range.start, top)
                    })
            })
            .inner;
        let max_offset = output.content_size.y - output.inner_rect.height();
        self.follow = follows_end(output.state.clamped_offset().y, max_offset);
        let (painter, background, laid, response, first, top) = output.inner;
        accessibility::name_with(&response, egui::Role::Log, || t!("a11y-console"));
        accessibility::edit(&response, |node| {
            node.set_value(laid.iter().map(|laid| laid.galley.text()).collect::<Vec<_>>().join("\n"));
        });
        self.announcer.show(ui, id.with("announce"), response.rect, self.lines.len(), |range| {
            self.lines.range(range).iter().map(|line| line.text.as_str()).collect()
        });
        self.rows = laid.iter().map(|laid| (laid.index, laid.rect)).collect();
        let spot_at = |pos: Pos2| -> Spot {
            let row = ((pos.y - top) / CONSOLE_ROW).floor() as isize + first as isize;
            let row = row.clamp(0, total.saturating_sub(1) as isize) as usize;
            let column = match laid.iter().find(|laid| laid.row == row) {
                Some(laid) => laid.galley.cursor_from_pos(pos - laid.at).index.0,
                None if row < first => 0,
                None => shown.line(row).and_then(|index| lines.get(index)).map_or(0, |line| line.text.chars().count()),
            };
            Spot { row, column }
        };
        let hovered = response.hover_pos().and_then(|pos| {
            let laid = laid.iter().find(|laid| laid.rect.y_range().contains(pos.y))?;
            let boundary = laid.galley.cursor_from_pos(pos - laid.at).index.0;
            let left = laid.galley.pos_from_cursor(CCursor::new(boundary)).left() + laid.at.x;
            let column = if left > pos.x { boundary.checked_sub(1)? } else { boundary };
            let link = lines.get(laid.index)?.links.iter().find(|link| link.range.contains(&column))?;
            Some((laid, link))
        });
        if let Some((laid, link)) = hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            let from = laid.galley.pos_from_cursor(CCursor::new(link.range.start)).left();
            let to = laid.galley.pos_from_cursor(CCursor::new(link.range.end)).left();
            let y = laid.at.y + laid.galley.size().y - 1.5;
            painter.hline((laid.at.x + from)..=(laid.at.x + to), y, Stroke::new(1.0, p.link));
        }
        if response.drag_started()
            && let Some(origin) = ui.input(|input| input.pointer.press_origin())
        {
            let anchor = spot_at(origin);
            self.selection = Some((anchor, anchor));
            ui.memory_mut(|memory| memory.request_focus(id));
        } else if response.dragged()
            && let (Some(pos), Some((anchor, _))) = (response.interact_pointer_pos(), self.selection)
        {
            self.selection = Some((anchor, spot_at(pos)));
        }
        if response.clicked() {
            ui.memory_mut(|memory| memory.request_focus(id));
            self.selection = None;
            clicked = hovered.map(|(_, link)| link.clone());
        }
        if ui.memory(|memory| memory.has_focus(id)) {
            let (copy, select_all) = ui.input(|input| {
                (
                    input.events.iter().any(|event| matches!(event, Event::Copy)),
                    input.events.iter().any(|event| {
                        matches!(event, Event::Key { key: Key::A, pressed: true, modifiers, .. } if modifiers.command)
                    }),
                )
            });
            if select_all && total > 0 {
                let last = shown
                    .line(total - 1)
                    .and_then(|index| lines.get(index))
                    .map_or(0, |line| line.text.chars().count());
                self.selection = Some((Spot::default(), Spot { row: total - 1, column: last }));
            }
            if copy && let Some(text) = self.selected_text(shown) {
                ui.ctx().copy_text(text);
            }
        }
        let mut shapes = Vec::new();
        if let Some((a, b)) = self.selection {
            let (from, to) = if a <= b { (a, b) } else { (b, a) };
            for laid in laid.iter().filter(|laid| (from.row..=to.row).contains(&laid.row)) {
                let start = if laid.row == from.row { from.column } else { 0 };
                let end = if laid.row == to.row { to.column } else { usize::MAX };
                let left = laid.galley.pos_from_cursor(CCursor::new(start)).left();
                let right =
                    laid.galley.pos_from_cursor(CCursor::new(end)).left() + if laid.row < to.row { 6.0 } else { 0.0 };
                if right > left {
                    let rect = Rect::from_min_max(
                        pos2(laid.at.x + left, laid.rect.top()),
                        pos2(laid.at.x + right, laid.rect.bottom()),
                    );
                    shapes.push(Shape::rect_filled(rect, 0, p.selection));
                }
            }
        }
        painter.set(background, Shape::Vec(shapes));
        clicked
    }
}

pub(super) struct Tool {
    pub salt: &'static str,
    pub glyph: Glyph,
    pub name: String,
    pub shortcut: Option<String>,
    pub on: Option<bool>,
    pub color: Color32,
}

impl Tool {
    pub(super) fn new(salt: &'static str, glyph: Glyph, name: String, color: Color32) -> Self {
        Self { salt, glyph, name, shortcut: None, on: None, color }
    }

    pub(super) fn show(self, ui: &Ui, p: &IdePalette, rect: Rect, enabled: bool) -> bool {
        self.show_hinted(ui, p, rect, enabled, Side::Right)
    }

    pub(super) fn show_hinted(self, ui: &Ui, p: &IdePalette, rect: Rect, enabled: bool, side: Side) -> bool {
        let response = ui.interact(rect, ui.scope_id().with(self.salt), Sense::click());
        if self.on == Some(true) {
            ui.painter().rect_filled(rect, 6, p.popup_hover);
        } else if enabled {
            hover(ui, &response, p);
        }
        let color = match self.on {
            Some(true) => p.text,
            Some(false) => p.text_secondary,
            None if enabled => self.color,
            None => p.gutter_text,
        };
        (self.glyph)(ui.painter(), rect.shrink(6.0), color);
        hint(ui, &response, &self.name, self.shortcut.as_deref(), side);
        if let Some(on) = self.on {
            accessibility::pressed(&response, on);
        }
        if !enabled {
            accessibility::disabled(&response);
        }
        enabled && response.clicked()
    }
}

pub(super) fn message(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(egui::RichText::new(text).color(p.text_secondary));
    });
}

pub(super) fn call(registry: &Registry, session: &mut Session, command: &str, params: Value) -> Result<Value, String> {
    registry.dispatch(session, command, params).map_err(|error| match error {
        DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    })
}

pub(super) fn char_column(session: &Session, line: u32, byte_column: u32) -> usize {
    session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .and_then(|document| {
            document
                .text()
                .split('\n')
                .nth(line.saturating_sub(1) as usize)
                .map(|text| byte_to_char_column(text, byte_column))
        })
        .unwrap_or(byte_column as usize)
}

#[derive(Debug, Default)]
pub(super) struct RunWindow {
    tabs: Vec<RunTab>,
    active: usize,
    known: u64,
    focus: bool,
    deferred: Vec<RunInfo>,
}

impl RunWindow {
    fn adopt_failed(&mut self, session: &Session) -> bool {
        if self.deferred.is_empty() {
            return false;
        }
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        let mut adopted = false;
        for info in std::mem::take(&mut self.deferred) {
            let run = session.state::<Runs>().and_then(|runs| runs.run(info.run_id));
            let Some(run) = run else { continue };
            let (running, failed) = run.output.read(|output| {
                let failed =
                    output.error().is_some() || (output.state() == RunState::Finished && output.exit_code() != Some(0));
                (output.is_running(), failed)
            });
            if running {
                self.deferred.push(info);
            } else if failed {
                let cwd = run.command_lines.first().map_or_else(|| root.clone(), |line| line.cwd.clone());
                self.adopt(&info, cwd, root.clone());
                adopted = true;
            }
        }
        adopted
    }

    pub(super) fn adopt_new_runs(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let failed = self.adopt_failed(session);
        let last = session.state::<Runs>().and_then(|runs| runs.runs().last()).map_or(0, |run| run.id);
        if last <= self.known {
            return failed;
        }
        let Ok(value) = registry.dispatch(session, "run.process.list", json!({})) else { return false };
        let Ok(list) = serde_json::from_value::<RunList>(value) else { return false };
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        let known = self.known;
        self.known = last;
        let mut adopted = false;
        let shown = |info: &RunInfo| info.external_tool.is_none_or(|tool| tool.open_console);
        let on_error = |info: &RunInfo| info.external_tool.is_some_and(|tool| tool.console_on_error);
        for info in list.runs.into_iter().filter(|info| info.run_id > known && !info.debug) {
            if !shown(&info) {
                if on_error(&info) {
                    self.deferred.push(info);
                }
                continue;
            }
            let cwd = session
                .state::<Runs>()
                .and_then(|runs| runs.run(info.run_id))
                .and_then(|run| run.command_lines.first())
                .map_or_else(|| root.clone(), |line| line.cwd.clone());
            self.adopt(&info, cwd, root.clone());
            adopted |= info.activate_tool_window || info.focus_tool_window;
            self.focus |= info.focus_tool_window;
        }
        adopted || failed
    }

    fn adopt(&mut self, info: &RunInfo, cwd: PathBuf, root: PathBuf) {
        let same = |tab: &RunTab| {
            !info.configuration_id.is_empty() && tab.configuration_id == info.configuration_id && !tab.is_running()
        };
        let replaced = |tab: &RunTab| info.replaces.is_some_and(|run_id| tab.run_id == run_id);
        let index = if let Some(index) = self.tabs.iter().position(|tab| tab.run_id == info.run_id) {
            index
        } else if let Some(index) = self.tabs.iter().position(replaced) {
            self.tabs[index].restart(info.run_id);
            index
        } else if let Some(index) = self.tabs.iter().position(same) {
            self.tabs[index].restart(info.run_id);
            index
        } else {
            self.tabs.push(RunTab::new(info, cwd.clone(), root.clone()));
            self.tabs.len() - 1
        };
        let tab = &mut self.tabs[index];
        if let Some(tool) = info.external_tool {
            tab.activate = (tool.activate_on_stdout, tool.activate_on_stderr);
        }
        tab.name = info.name.clone();
        tab.configuration_id = info.configuration_id.clone();
        tab.cwd = cwd;
        tab.root = root;
        self.active = index;
    }

    pub(super) fn refresh(&mut self, registry: &Registry, session: &mut Session) {
        let limit = zen_workbench::run::console_limit(session.settings());
        for tab in &mut self.tabs {
            tab.console.set_limit(limit);
            let revision = session
                .state::<Runs>()
                .and_then(|runs| runs.run(tab.run_id))
                .map(|run| run.output.read(|output| output.revision()));
            if revision.is_none() || tab.seen == revision {
                continue;
            }
            tab.seen = revision;
            let params = json!({ "runId": tab.run_id, "from": tab.console.next, "waitMs": 0 });
            if let Ok(value) = registry.dispatch(session, "run.output.get", params)
                && let Ok(answer) = serde_json::from_value::<OutputAnswer>(value)
            {
                tab.apply_output(answer);
            }
            if let Ok(value) = registry.dispatch(session, "run.tests.get", json!({ "runId": tab.run_id }))
                && let Ok(answer) = serde_json::from_value::<TestsAnswer>(value)
            {
                tab.apply_tests(answer);
            }
        }
    }

    pub(super) fn perform(
        &mut self,
        action: &RunAction,
        registry: &Registry,
        session: &mut Session,
    ) -> Result<(), String> {
        let mut last = Value::Null;
        for (command, params) in action.commands() {
            last = call(registry, session, command, params)?;
        }
        match action {
            RunAction::Rerun { run_id, .. }
            | RunAction::RerunFailed { run_id }
            | RunAction::RerunTest { run_id, .. } => {
                if let Ok(started) = serde_json::from_value::<RunStarted>(last)
                    && let Some(index) = self.tabs.iter().position(|tab| tab.run_id == *run_id)
                {
                    self.tabs[index].restart(started.run_id);
                    self.active = index;
                }
            }
            RunAction::Close { run_id, .. } => {
                if let Some(index) = self.tabs.iter().position(|tab| tab.run_id == *run_id) {
                    self.tabs.remove(index);
                    if index < self.active || self.active >= self.tabs.len() {
                        self.active = self.active.saturating_sub(1);
                    }
                }
            }
            RunAction::Open { line, column, .. } => {
                let column = column.map(|byte| char_column(session, *line, byte));
                call(registry, session, "editor.caret.goTo", json!({ "line": line, "column": column }))?;
            }
            RunAction::OpenTest { .. } => {
                let source: TestSource = serde_json::from_value(last).map_err(|error| error.to_string())?;
                call(registry, session, "file.document.open", json!({ "path": source.path }))?;
                call(registry, session, "editor.caret.goTo", json!({ "line": source.line }))?;
            }
            RunAction::Hide | RunAction::Stop { .. } | RunAction::History { .. } => {}
        }
        Ok(())
    }

    pub(super) fn take_front(&mut self) -> bool {
        let mut front = false;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if std::mem::take(&mut tab.wants_front) {
                self.active = index;
                front = true;
            }
        }
        front
    }

    pub(super) fn any_running(&self) -> bool {
        self.tabs.iter().any(RunTab::is_running)
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Vec<RunAction> {
        let mut actions = Vec::new();
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.header(ui, p, &mut actions);
        match self.tabs.get_mut(self.active) {
            Some(tab) => tab.show(ui, p, &mut actions),
            None => message(ui, p, t!("ide-run-empty")),
        }
        actions
    }

    fn header(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<RunAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-tool-run"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("run-title"), title, || t!("ide-tool-run"));
        let mut x = title.right() + 14.0;
        for (index, tab) in self.tabs.iter().enumerate() {
            let galley = ui.painter().layout_no_wrap(tab.name.clone(), theme::regular(13.0), p.text);
            let rect = Rect::from_min_size(pos2(x, header.center().y - 13.0), vec2(galley.size().x + 58.0, 26.0));
            let response = ui.interact(rect, ui.scope_id().with(("run-tab", index)), Sense::click());
            let close = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(16.0));
            let close_response = ui.interact(close, ui.scope_id().with(("run-tab-close", index)), Sense::click());
            let active = self.active == index;
            accessibility::item(&response, egui::Role::Tab, active, &tab.name);
            accessibility::button_with(&close_response, || t!("a11y-close-tab", name = tab.name.as_str()));
            if active {
                ui.painter().rect_filled(rect, 6, p.selection);
            } else if response.hovered() || close_response.hovered() {
                ui.painter().rect_filled(rect, 6, p.hover);
            }
            let (glyph, color) = tab.status_look(p);
            glyph(
                ui.painter(),
                Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(14.0)),
                color,
            );
            ui.painter().galley(pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
            if active || response.hovered() || close_response.hovered() {
                let c = close.shrink(4.0);
                let color = if close_response.hovered() { p.text } else { p.text_secondary };
                ui.painter().line_segment([c.left_top(), c.right_bottom()], Stroke::new(1.2, color));
                ui.painter().line_segment([c.right_top(), c.left_bottom()], Stroke::new(1.2, color));
            }
            if close_response.clicked() || response.middle_clicked() {
                actions.push(RunAction::Close { run_id: tab.run_id, running: tab.is_running() });
            } else if response.clicked() {
                self.active = index;
            }
            x = rect.right() + 6.0;
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("run-hide"), Sense::click());
        hover(ui, &response, p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        if response.clicked() {
            actions.push(RunAction::Hide);
        }
    }
}

impl IdeView {
    pub(super) fn follow_runs(&mut self, registry: &Registry, session: &mut Session) {
        if self.run_window.adopt_new_runs(registry, session) {
            self.show_tool("run");
        }
        if std::mem::take(&mut self.run_window.focus) {
            self.tool_window_took_keyboard();
        }
        self.run_window.refresh(registry, session);
        if self.run_window.take_front() {
            self.show_tool("run");
        }
        zen_workbench::test_history::settle(session);
        if zen_workbench::external_tools::settle(session) {
            self.listings.clear();
        }
        zen_workbench::before_launch::advance(session);
        for tool in zen_workbench::before_launch::take_tool_windows(session) {
            self.show_tool(&tool);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn follow_file_watchers(&mut self, ctx: &egui::Context, session: &mut Session) {
        if let Some(wake) = zen_workbench::file_watchers::step(session, std::time::Instant::now()) {
            ctx.request_repaint_after(wake);
        }
    }

    pub(super) fn run_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        for action in self.run_window.show(ui, &palette) {
            if action == RunAction::Hide {
                self.hide_tool("run");
                continue;
            }
            if let RunAction::History { at } = action {
                if let Err(error) = self.open_test_history(registry, session, at) {
                    self.report(ui, error);
                }
                continue;
            }
            match self.run_window.perform(&action, registry, session) {
                Ok(()) if matches!(action, RunAction::Open { .. } | RunAction::OpenTest { .. }) => {
                    self.editor_view.focus()
                }
                Ok(()) => {}
                Err(_) if zen_workbench::database::backups::has_asked(session) => {}
                Err(message) => self.post(ui, message),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::{MouseWheelUnit, PointerButton, TouchPhase};
    use zen_run::RunEnv;
    use zen_run::fake::FakeProvider;

    use super::*;

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        fake: FakeProvider,
        dir: tempfile::TempDir,
        window: RunWindow,
        actions: Vec<RunAction>,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            session.set_state(zen_workbench::build_tools::BuildEnv::default());
            registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
            let fake = FakeProvider::new();
            session.state_mut::<Runs>().set_providers(vec![Arc::new(fake.clone())], RunEnv::default());
            Self { ctx, registry, session, fake, dir, window: RunWindow::default(), actions: Vec::new(), time: 0.0 }
        }

        fn start(&mut self, params: Value) -> u64 {
            let started = self.registry.dispatch(&mut self.session, "run.process.start", params).unwrap();
            started["runId"].as_u64().unwrap()
        }

        fn start_named(&mut self, name: &str) -> u64 {
            self.start(json!({ "configuration": FakeProvider::configuration(name) }))
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.window.adopt_new_runs(&self.registry, &mut self.session);
            self.window.refresh(&self.registry, &mut self.session);
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 400.0))),
                ..Default::default()
            };
            let mut actions = Vec::new();
            let mut output = self.ctx.run_ui(input, |ui| actions = self.window.show(ui, &IdePalette::zen_dark()));
            output.textures_delta.clear();
            for action in &actions {
                if *action != RunAction::Hide {
                    let _ = self.window.perform(action, &self.registry, &mut self.session);
                }
            }
            self.actions.extend(actions);
        }

        fn click_with(&mut self, at: Pos2, double: bool) {
            self.time += 1.0;
            self.frame(vec![Event::PointerMoved(at)]);
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            if double {
                self.frame(vec![button(true)]);
                self.frame(vec![button(false)]);
            }
        }

        fn click(&mut self, at: Pos2) {
            self.click_with(at, false);
        }

        fn tab(&self) -> &RunTab {
            &self.window.tabs[self.window.active]
        }

        fn shown_texts(&self) -> Vec<String> {
            let tab = self.tab();
            let shown = tab.shown();
            (0..shown.len())
                .filter_map(|row| shown.line(row))
                .map(|index| tab.console.lines.get(index).unwrap().text.clone())
                .collect()
        }

        fn tree_row(&self, test: Option<&str>) -> Rect {
            let tab = self.tab();
            tab.tree_rows.iter().find(|(key, _)| key.1.as_deref() == test).map(|(_, rect)| *rect).unwrap()
        }

        fn toolbar_button(&self, index: usize) -> Pos2 {
            pos2(TOOLBAR / 2.0, metrics::TOOL_HEADER + 6.0 + BUTTON / 2.0 + index as f32 * (BUTTON + 4.0))
        }
    }

    fn line(stream: Stream, text: &str) -> ConsoleLine {
        ConsoleLine::new(stream, text)
    }

    const TESTS: [(Stream, &str); 9] = [
        (Stream::Stdout, "@start app TestOk"),
        (Stream::Stdout, "ok output"),
        (Stream::Stdout, "@pass app TestOk 3"),
        (Stream::Stdout, "@start app TestBad"),
        (Stream::Stdout, "main_test.go:2:5: boom"),
        (Stream::Stderr, "more trouble"),
        (Stream::Stdout, "@fail app TestBad 1500"),
        (Stream::Stdout, "@skip app TestSkip 0"),
        (Stream::Stdout, "@fail app - 1503"),
    ];

    #[test]
    fn a_run_gets_a_tab_and_a_rerun_of_its_configuration_reuses_it() {
        let mut run = Harness::new();
        run.frame(Vec::new());
        assert!(run.window.tabs.is_empty(), "nothing is running");
        run.fake.set_script(&[(Stream::Stdout, "hello")], 0);
        let first = run.start_named("one");
        run.start_named("two");
        run.frame(Vec::new());
        assert_eq!(run.window.tabs.len(), 2, "a configuration run without saving gets a tab each time");
        assert_eq!(run.tab().name, "two");
        let params = json!({ "configuration": FakeProvider::configuration("saved") });
        let saved = run.registry.dispatch(&mut run.session, "run.configuration.create", params).unwrap();
        run.start(json!({ "configurationId": saved["id"] }));
        run.frame(Vec::new());
        assert_eq!(run.window.tabs.len(), 3);
        run.start(json!({ "configurationId": saved["id"] }));
        run.frame(Vec::new());
        assert_eq!(run.window.tabs.len(), 3, "the same configuration reuses its tab");
        assert_eq!(run.tab().name, "saved");
        assert_eq!(
            run.tab().console.lines().iter().filter(|line| line.text == "hello").count(),
            1,
            "the tab starts afresh"
        );

        let tab = run.window.tabs.iter().position(|tab| tab.run_id == first).unwrap();
        run.window.active = tab;
        run.frame(Vec::new());
        let replays = run.fake.replays();
        run.click(run.toolbar_button(0));
        assert_eq!(run.actions.last(), Some(&RunAction::Rerun { run_id: first, running: false }));
        assert_eq!(run.fake.replays(), replays + 1);
        run.frame(Vec::new());
        assert_eq!(run.window.tabs.len(), 3, "Rerun runs again in the same tab");
        assert_eq!(run.window.active, tab);
        assert_ne!(run.tab().run_id, first);
        assert_eq!(run.tab().name, "one");
    }

    #[cfg(unix)]
    #[test]
    fn rerun_and_closing_a_tab_stop_a_real_running_program() {
        let mut run = Harness::new();
        let providers: Vec<Arc<dyn zen_run::RunProvider>> =
            vec![Arc::new(run.fake.clone()), Arc::new(zen_run::fake::ShellProvider)];
        run.session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
        let state = |run: &mut Harness, id: u64, wait: bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                let params = json!({ "runId": id, "from": 0, "waitMs": if wait { 200 } else { 0 } });
                let output = run.registry.dispatch(&mut run.session, "run.output.get", params).unwrap();
                if !wait || output["state"] != "running" {
                    return output["state"].clone();
                }
                assert!(std::time::Instant::now() < deadline, "run {id} does not end");
            }
        };
        let script = zen_run::fake::ShellProvider::configuration("server", "echo ready; sleep 60");
        let first = run.start(json!({ "configuration": script }));
        run.frame(Vec::new());
        assert!(run.tab().is_running());
        run.click(run.toolbar_button(0));
        assert_eq!(run.actions.last(), Some(&RunAction::Rerun { run_id: first, running: true }));
        let second = run.tab().run_id;
        assert_ne!(second, first, "the rerun runs in the same tab");
        assert_eq!(run.window.tabs.len(), 1);
        assert_eq!(state(&mut run, first, true), "stopped", "the running program was stopped first");
        assert_eq!(state(&mut run, second, false), "running");

        run.window
            .perform(&RunAction::Close { run_id: second, running: true }, &run.registry, &mut run.session)
            .unwrap();
        assert!(run.window.tabs.is_empty());
        assert_eq!(state(&mut run, second, true), "stopped", "closing the tab stopped its program");
    }

    #[cfg(unix)]
    #[test]
    fn instances_get_tabs_of_their_own_and_a_rerun_takes_the_stopped_ones_tab() {
        let mut run = Harness::new();
        let providers: Vec<Arc<dyn zen_run::RunProvider>> =
            vec![Arc::new(run.fake.clone()), Arc::new(zen_run::fake::ShellProvider)];
        run.session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
        let mut saved =
            serde_json::to_value(zen_run::fake::ShellProvider::configuration("server", "sleep 30")).unwrap();
        saved["id"] = json!("srv");
        saved["common"] = json!({ "allowMultipleInstances": true, "activateToolWindow": false });
        run.registry
            .dispatch(&mut run.session, "run.configuration.create", json!({ "configuration": saved.clone() }))
            .unwrap();
        let first = run.start(json!({ "configurationId": "srv" }));
        let second = run.start(json!({ "configurationId": "srv" }));
        assert!(!run.window.adopt_new_runs(&run.registry, &mut run.session), "the tool window stays closed");
        let names: Vec<&str> = run.window.tabs.iter().map(|tab| tab.name.as_str()).collect();
        assert_eq!(names, ["server", "server (2)"]);
        saved["common"] = json!({ "focusToolWindow": true });
        run.registry.dispatch(&mut run.session, "run.configuration.update", json!({ "configuration": saved })).unwrap();
        let third = run.start(json!({ "configurationId": "srv" }));
        assert!(run.window.adopt_new_runs(&run.registry, &mut run.session));
        assert!(std::mem::take(&mut run.window.focus), "the tool window takes the keyboard");
        let ids: Vec<u64> = run.window.tabs.iter().map(|tab| tab.run_id).collect();
        assert_eq!(ids, [first, third], "the rerun took the tab of the instance it stopped last");
        assert_ne!(second, third);
        run.registry.dispatch(&mut run.session, "run.process.stop", json!({ "runId": third })).unwrap();
    }

    struct ListMarkingDebugRuns;

    impl zen_core::Command for ListMarkingDebugRuns {
        type Params = Value;
        type Output = Value;
        const ID: &'static str = "run.process.list";
        const TITLE: &'static str = "List Runs";
        const DESCRIPTION: &'static str = "Lists the runs, marking those named `debug…` as a debug session's.";

        fn run(&self, session: &mut Session, _: Value) -> Result<Value, zen_core::CommandError> {
            let runs: Vec<Value> = session
                .state::<Runs>()
                .map(|runs| runs.runs())
                .unwrap_or_default()
                .iter()
                .map(|run| {
                    let name = &run.configuration.name;
                    json!({ "runId": run.id, "name": name, "debug": name.starts_with("debug") })
                })
                .collect();
            Ok(json!({ "runs": runs }))
        }
    }

    #[test]
    fn a_debug_sessions_run_stays_out_of_the_run_window() {
        let info: RunInfo = serde_json::from_value(json!({ "runId": 1, "name": "app" })).unwrap();
        assert!(!info.debug, "a run without the field is a plain run");
        let mut registry = Registry::new();
        registry.register(ListMarkingDebugRuns).unwrap();
        let mut session = Session::new();
        let fake = FakeProvider::new();
        session.state_mut::<Runs>().set_providers(vec![Arc::new(fake.clone())], RunEnv::default());
        let start = |session: &mut Session, name: &str| {
            session
                .state_mut::<Runs>()
                .start(FakeProvider::configuration(name), &zen_run::RunContext::default())
                .unwrap()
        };
        let mut window = RunWindow::default();
        start(&mut session, "debug app");
        assert!(!window.adopt_new_runs(&registry, &mut session), "a debug run opens no Run tool window");
        assert!(window.tabs.is_empty());
        let plain = start(&mut session, "app");
        start(&mut session, "debug again");
        assert!(window.adopt_new_runs(&registry, &mut session));
        assert_eq!(window.tabs.iter().map(|tab| tab.run_id).collect::<Vec<_>>(), [plain], "only the plain run");
        assert!(!window.adopt_new_runs(&registry, &mut session), "nothing new");
    }

    #[test]
    fn an_on_error_run_gets_a_tab_only_once_it_failed() {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let start = |session: &mut Session, name: &str, code: i32| {
            let launch = zen_run::external_tool::ToolLaunch {
                tool_id: name.to_owned(),
                program: PathBuf::from("/bin/sh"),
                args: vec!["-c".to_owned(), format!("exit {code}")],
                console_on_error: true,
                ..zen_run::external_tool::ToolLaunch::default()
            };
            let configuration = zen_run::Configuration {
                id: format!("fileWatcher:{name}"),
                name: name.to_owned(),
                kind: zen_run::external_tool::KIND.to_owned(),
                temporary: false,
                common: Default::default(),
                options: serde_json::to_value(&launch).unwrap(),
                ..Default::default()
            };
            let id = session.state_mut::<Runs>().start(configuration, &zen_run::RunContext::default()).unwrap();
            let output = session.state::<Runs>().and_then(|runs| runs.run(id)).unwrap().output.clone();
            (id, output)
        };
        let mut window = RunWindow::default();
        let (_, fine) = start(&mut session, "fine", 0);
        let (failed, broken) = start(&mut session, "broken", 2);
        assert!(!window.adopt_new_runs(&registry, &mut session), "no tab while they run");
        fine.wait_end(std::time::Duration::from_secs(20));
        broken.wait_end(std::time::Duration::from_secs(20));
        assert!(window.adopt_new_runs(&registry, &mut session));
        assert_eq!(window.tabs.iter().map(|tab| tab.run_id).collect::<Vec<_>>(), [failed], "only the failed run");
        assert!(!window.adopt_new_runs(&registry, &mut session), "nothing new");
    }

    #[test]
    fn toolbar_and_tab_actions_are_commands() {
        let rerun = RunAction::Rerun { run_id: 4, running: true };
        let ids = |action: &RunAction| action.commands().into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        assert_eq!(ids(&rerun), ["run.process.stop", "run.process.rerun"], "a running run is stopped first");
        assert_eq!(rerun.commands()[1].1, json!({ "runId": 4 }));
        assert_eq!(ids(&RunAction::Rerun { run_id: 4, running: false }), ["run.process.rerun"]);
        assert_eq!(ids(&RunAction::Stop { run_id: 4 }), ["run.process.stop"]);
        assert_eq!(ids(&RunAction::RerunFailed { run_id: 4 }), ["run.tests.rerunFailed"]);
        assert_eq!(ids(&RunAction::Close { run_id: 4, running: true }), ["run.process.stop"], "closing stops");
        assert!(ids(&RunAction::Close { run_id: 4, running: false }).is_empty());

        let mut run = Harness::new();
        run.fake.set_script(&TESTS, 1);
        let id = run.start_named("tests");
        run.frame(Vec::new());
        run.frame(Vec::new());
        run.click(run.toolbar_button(1));
        assert!(!run.actions.iter().any(|action| matches!(action, RunAction::Stop { .. })), "Stop is off once ended");
        run.window.tabs[0].state = RunState::Running;
        run.frame(Vec::new());
        run.click(run.toolbar_button(1));
        assert_eq!(run.actions.last(), Some(&RunAction::Stop { run_id: id }));
        run.window.tabs[0].state = RunState::Finished;
        let replays = run.fake.replays();
        run.click(run.toolbar_button(2));
        assert_eq!(run.actions.last(), Some(&RunAction::RerunFailed { run_id: id }));
        assert_eq!(run.fake.replays(), replays + 1);
        run.frame(Vec::new());
        assert_eq!(run.window.tabs.len(), 1, "the failed tests run in the same tab");
        assert_eq!(run.tab().name, "tests (failed)");

        run.frame(Vec::new());
        let passed = run.toolbar_button(3) + vec2(0.0, 6.0);
        run.click(passed);
        assert!(!run.tab().show_passed);
        assert!(run.tab().tree_rows.iter().all(|(key, _)| key.1.as_deref() != Some("TestOk")), "passed tests hidden");
        let ignored = passed + vec2(0.0, BUTTON + 4.0);
        run.click(ignored);
        assert!(!run.tab().show_ignored);
        assert!(run.tab().tree_rows.iter().all(|(key, _)| key.1.as_deref() != Some("TestSkip")), "ignored hidden");
        assert!(run.tab().tree_rows.iter().any(|(key, _)| key.1.as_deref() == Some("TestBad")));

        let close = run.window.tabs[0].run_id;
        run.window.tabs[0].state = RunState::Running;
        run.window
            .perform(&RunAction::Close { run_id: close, running: true }, &run.registry, &mut run.session)
            .unwrap();
        assert!(run.window.tabs.is_empty());
        run.frame(Vec::new());
    }

    #[test]
    fn console_lines_are_styled_by_stream_and_links_by_colour() {
        let p = IdePalette::zen_dark();
        let stdout = line_job(&line(Stream::Stdout, "plain"), &p);
        assert_eq!(stdout.sections[0].format.color, p.text);
        let stderr = line_job(&line(Stream::Stderr, "панic: bad"), &p);
        assert_eq!(stderr.sections[0].format.color, error_color(&p), "stderr is in the error colour");
        let system = line_job(&line(Stream::System, "Process finished with exit code 0"), &p);
        assert_eq!(system.sections[0].format.color, p.text_secondary, "the IDE's own lines are dimmed");
        let linked = line_job(&line(Stream::Stdout, "    файл_test.go:42: expected"), &p);
        let parts: Vec<(&str, Color32)> = linked
            .sections
            .iter()
            .map(|section| {
                (
                    &linked.text[usize::from(section.byte_range.start)..usize::from(section.byte_range.end)],
                    section.format.color,
                )
            })
            .collect();
        assert_eq!(parts, [("    ", p.text), ("файл_test.go:42", p.link), (": expected", p.text)]);
        assert_eq!(linked.sections[1].format.font_id, FontId::monospace(CONSOLE_FONT));
    }

    #[test]
    fn a_click_on_a_link_opens_the_file_at_the_character_column() {
        assert_eq!(byte_to_char_column("ыы x", 5), 3, "two letters of two bytes each");
        assert_eq!(byte_to_char_column("abc", 2), 2);
        assert_eq!(byte_to_char_column("ы", 99), 2, "past the end");
        let mut run = Harness::new();
        let file = run.dir.path().join("main_test.go");
        std::fs::write(&file, "package app\nыы x := 1\n").unwrap();
        run.fake.set_script(&[(Stream::Stdout, "main_test.go:2:5: boom")], 1);
        run.start_named("app");
        run.frame(Vec::new());
        run.frame(Vec::new());
        let (_, rect) = *run
            .tab()
            .console
            .rows
            .iter()
            .find(|(index, _)| run.tab().console.lines.get(*index).unwrap().text.starts_with("main"))
            .unwrap();
        let at = rect.left_center() + vec2(CONSOLE_INSET + 4.0, 0.0);
        run.click(at);
        let open = RunAction::Open { path: file.clone(), line: 2, column: Some(5) };
        assert_eq!(run.actions, [open]);
        let document = run.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.path(), file.as_path());
        assert_eq!(document.selections().primary().head, "package app\n".chars().count() + 2);
        let elsewhere = rect.left_center() + vec2(CONSOLE_INSET + 200.0, 0.0);
        run.click(elsewhere);
        assert_eq!(run.actions.len(), 1, "text after the link is not a link");
    }

    #[test]
    fn links_resolve_in_the_run_folder_then_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let (cwd, root) = (dir.path().join("cmd"), dir.path().to_owned());
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("main.go"), "").unwrap();
        std::fs::write(root.join("lib.go"), "").unwrap();
        let link = |text: &str| zen_run::detect(text).remove(0);
        assert_eq!(resolve_link(&link("main.go:1"), &cwd, &root), Some(cwd.join("main.go")));
        assert_eq!(resolve_link(&link("lib.go:1"), &cwd, &root), Some(root.join("lib.go")));
        assert_eq!(resolve_link(&link("nowhere.go:1"), &cwd, &root), None);
        let absolute = format!("{}:3", root.join("lib.go").display());
        assert_eq!(resolve_link(&link(&absolute), &cwd, &root), Some(root.join("lib.go")));
    }

    #[test]
    fn benchmarks_show_as_a_table_of_their_results_above_the_console() {
        let measure = |unit: &str, value: &str| json!({ "unit": unit, "value": value });
        let node = |name: &str, test: Option<&str>, iterations: Option<u64>, measures: Value, children: Value| {
            json!({
                "name": name, "kind": if test.is_some() { "test" } else { "package" }, "package": "p", "test": test,
                "status": "passed", "durationMs": null, "iterations": iterations, "measures": measures,
                "lines": [], "children": children,
            })
        };
        let small = node(
            "small",
            Some("BenchmarkSum/small"),
            Some(100),
            json!([measure("ns/op", "5.4"), measure("B/op", "0")]),
            json!([]),
        );
        let sum = node("BenchmarkSum", Some("BenchmarkSum"), None, json!([]), json!([small]));
        let add = node(
            "BenchmarkAdd",
            Some("BenchmarkAdd"),
            Some(7),
            json!([measure("ns/op", "4.1"), measure("widgets/op", "3.5")]),
            json!([]),
        );
        let test = node("TestPlain", Some("TestPlain"), None, json!([]), json!([]));
        let tree: TestNode = serde_json::from_value(node("p", None, None, json!([]), json!([sum, add, test]))).unwrap();
        let table = benchmark_table(&tree);
        assert_eq!(table.units, ["ns/op", "B/op", "widgets/op"]);
        assert_eq!(
            table.rows,
            [
                BenchmarkRow {
                    test: "BenchmarkSum/small".to_owned(),
                    iterations: "100".to_owned(),
                    cells: vec!["5.4".to_owned(), "0".to_owned(), String::new()],
                },
                BenchmarkRow {
                    test: "BenchmarkAdd".to_owned(),
                    iterations: "7".to_owned(),
                    cells: vec!["4.1".to_owned(), String::new(), "3.5".to_owned()],
                },
            ]
        );
        let mut run = Harness::new();
        run.fake.set_script(&TESTS, 1);
        run.start_named("tests");
        run.frame(Vec::new());
        run.frame(Vec::new());
        assert!(run.tab().benchmark_rows.is_empty(), "plain tests have no table");
        let active = run.window.active;
        run.window.tabs[active].tests.as_mut().unwrap().tree = tree;
        run.frame(Vec::new());
        assert_eq!(run.tab().benchmark_rows, ["BenchmarkSum/small", "BenchmarkAdd"]);
    }

    #[test]
    fn the_test_tree_selects_the_first_failure_and_filters_the_console() {
        let mut run = Harness::new();
        std::fs::write(run.dir.path().join("main_test.go"), "package app\nfunc TestBad() {}\n").unwrap();
        run.fake.set_script(&TESTS, 1);
        run.start_named("tests");
        run.frame(Vec::new());
        run.frame(Vec::new());
        assert_eq!(run.tab().selected, Some(("app".to_owned(), Some("TestBad".to_owned()))), "the first failure");
        assert_eq!(run.shown_texts(), ["main_test.go:2:5: boom", "more trouble"]);
        let keys: Vec<Option<&str>> = run.tab().tree_rows.iter().map(|(key, _)| key.1.as_deref()).collect();
        assert_eq!(keys, [None, None, Some("TestBad"), Some("TestOk"), Some("TestSkip")], "failed tests first");
        run.click(run.tree_row(Some("TestOk")).center());
        assert_eq!(run.shown_texts(), ["ok output"]);
        let root = run.tab().tree_rows[0].1;
        run.click(root.center());
        assert_eq!(run.shown_texts().len(), run.tab().console.lines().len(), "the run's node shows the whole console");
        assert_eq!(run.shown_texts()[0], "fake tests");
        run.click_with(run.tree_row(Some("TestBad")).center(), true);
        let file = run.dir.path().join("main_test.go");
        assert_eq!(run.actions.last(), Some(&RunAction::Open { path: file, line: 2, column: Some(5) }));
        let package = run.tab().tree_rows[1].1;
        run.click(package.left_center() + vec2(8.0 + metrics::TREE_INDENT + 6.0, 0.0));
        run.frame(Vec::new());
        assert_eq!(run.tab().tree_rows.len(), 2, "the chevron collapses the package");
    }

    #[test]
    fn a_running_tree_keeps_its_order_and_only_a_finished_one_puts_failures_first() {
        let leaf = |name: &str, status| TestNode {
            name: name.to_owned(),
            kind: TestNodeKind::Test,
            package: "app".to_owned(),
            test: Some(name.to_owned()),
            status,
            duration_ms: None,
            iterations: None,
            measures: Vec::new(),
            lines: Vec::new(),
            children: Vec::new(),
        };
        let mut package = leaf("app", TestStatus::Failed);
        package.children = vec![
            leaf("TestA", TestStatus::Passed),
            leaf("TestB", TestStatus::Failed),
            leaf("TestC", TestStatus::Skipped),
            leaf("TestD", TestStatus::Failed),
        ];
        let names = |done| failed_first(&package, done).iter().map(|node| node.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(false), ["TestA", "TestB", "TestC", "TestD"]);
        assert_eq!(names(true), ["TestB", "TestD", "TestA", "TestC"]);
    }

    #[test]
    fn a_test_without_a_failure_place_opens_where_it_is_written() {
        let mut run = Harness::new();
        std::fs::write(run.dir.path().join("main_test.go"), "package app\nfunc TestOk() {}\n").unwrap();
        run.fake.set_script(&TESTS, 0);
        let id = run.start_named("tests");
        run.frame(Vec::new());
        run.frame(Vec::new());
        let ok = run.tree_row(Some("TestOk")).center();
        run.click_with(ok, true);
        let action = RunAction::OpenTest { run_id: id, package: "app".to_owned(), test: "TestOk".to_owned() };
        assert_eq!(run.actions.last(), Some(&action));
        let document = run.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.path(), run.dir.path().join("main_test.go").as_path());
        assert_eq!(document.selections().primary().head, "package app\n".chars().count());
    }

    #[test]
    fn a_tests_menu_reruns_only_that_test_in_its_tab() {
        let mut run = Harness::new();
        run.fake.set_script(&TESTS, 1);
        let id = run.start_named("tests");
        run.frame(Vec::new());
        run.frame(Vec::new());
        let row = run.tree_row(Some("TestOk")).center();
        let button = |pressed| Event::PointerButton {
            pos: row,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        run.frame(vec![Event::PointerMoved(row)]);
        run.frame(vec![button(true)]);
        run.frame(vec![button(false)]);
        run.frame(Vec::new());
        assert!(egui::Popup::is_any_open(&run.ctx), "a right click opens the test's menu");
        let row = NodeRow {
            key: ("app".to_owned(), Some("TestOk".to_owned())),
            depth: 2,
            name: "TestOk".to_owned(),
            status: TestStatus::Passed,
            duration: None,
            parent: false,
        };
        let tree = accessibility::testing::tree_of(&egui::Context::default(), vec2(300.0, 100.0), |ui| {
            test_menu(ui, id, &row, false, &mut Vec::new());
        });
        assert!(accessibility::testing::find(&tree, egui::Role::Button, "Run 'TestOk'").is_some());
        let action = RunAction::RerunTest { run_id: id, package: "app".to_owned(), test: Some("TestOk".to_owned()) };
        assert_eq!(action.commands()[0].0, "run.tests.rerun");
        run.window.perform(&action, &run.registry, &mut run.session).unwrap();
        run.frame(Vec::new());
        assert_eq!(run.window.tabs.len(), 1, "the test runs in the same tab");
        assert_ne!(run.tab().run_id, id);
        assert_eq!(run.tab().name, "tests (failed)");
    }

    #[test]
    fn the_counts_line_and_durations() {
        let mut run = Harness::new();
        run.fake.set_script(&TESTS, 1);
        run.start_named("tests");
        run.frame(Vec::new());
        let tests = run.tab().tests.clone().unwrap();
        assert_eq!((tests.counts.failed, tests.counts.passed, tests.counts.total), (1, 1, 3));
        let counts = t!("ide-run-tests-failed", failed = 1, passed = 1, total = 3);
        let expected = t!("ide-run-tests-summary", counts = counts, duration = duration_text(1503));
        assert_eq!(counts_text(&tests), expected);
        assert_eq!(duration_text(1503), t!("ide-run-duration-seconds", seconds = 1, ms = 503));
        assert_eq!(duration_text(12), t!("ide-run-duration-ms", ms = 12));
        let mut passing = tests.clone();
        passing.counts.failed = 0;
        passing.done = false;
        assert_eq!(counts_text(&passing), t!("ide-run-tests-passed", passed = 1, total = 3));
    }

    #[test]
    fn a_long_run_keeps_only_the_console_cycle_buffer() {
        let mut run = Harness::new();
        run.session.settings_mut().set(zen_workbench::run::CONSOLE_BUFFER_OVERRIDE, true).unwrap();
        run.session.settings_mut().set(zen_workbench::run::CONSOLE_BUFFER_KB, 1).unwrap();
        let lines: Vec<String> = (0..2000).map(|index| format!("line {index:04}")).collect();
        let script: Vec<(Stream, &str)> = lines.iter().map(|text| (Stream::Stdout, text.as_str())).collect();
        run.fake.set_script(&script, 0);
        run.start_named("long");
        run.frame(Vec::new());
        run.frame(Vec::new());
        let console = &run.tab().console;
        assert!(console.lines().len() < 200, "{}", console.lines().len());
        let output = run.session.state::<Runs>().unwrap().run(run.tab().run_id).unwrap().output.clone();
        assert!(output.read(|output| output.console().lines().len()) < 200);
        let texts = run.shown_texts();
        assert_eq!(texts.first().map(String::as_str), console.lines().first().map(|line| line.text.as_str()));
        assert!(texts.iter().any(|text| text == "line 1999"), "{:?}", texts.last());
        assert_eq!(run.tab().console.rows.last().map(|(index, _)| *index), Some(run.tab().console.all().end - 1));
    }

    #[test]
    fn the_console_follows_its_end_unless_scrolled_up() {
        assert!(follows_end(100.0, 100.0));
        assert!(follows_end(0.0, -50.0), "content shorter than the console");
        assert!(!follows_end(40.0, 100.0));
        let mut run = Harness::new();
        let lines: Vec<String> = (0..200).map(|index| format!("line {index}")).collect();
        let script: Vec<(Stream, &str)> = lines.iter().map(|text| (Stream::Stdout, text.as_str())).collect();
        run.fake.set_script(&script, 0);
        run.start_named("long");
        run.frame(Vec::new());
        run.frame(Vec::new());
        assert!(run.tab().console.follow);
        let last = run.tab().console.rows.last().map(|(index, _)| *index).unwrap();
        assert_eq!(last, run.tab().console.lines().len() - 1, "the end is shown");
        let over = pos2(500.0, 200.0);
        let wheel = |delta: f32| Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, delta),
            phase: TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        };
        run.frame(vec![Event::PointerMoved(over), wheel(600.0)]);
        for _ in 0..10 {
            run.frame(Vec::new());
        }
        assert!(!run.tab().console.follow, "scrolled up");
        assert!(run.tab().console.rows.last().map(|(index, _)| *index).unwrap() < last);
        run.frame(vec![Event::PointerMoved(over), wheel(-100_000.0)]);
        for _ in 0..10 {
            run.frame(Vec::new());
        }
        assert!(run.tab().console.follow, "back at the end, it follows again");
    }

    #[test]
    fn console_text_is_selected_by_dragging_and_copied() {
        let mut run = Harness::new();
        run.fake.set_script(&[(Stream::Stdout, "alpha"), (Stream::Stdout, "beta")], 0);
        run.start_named("copy");
        run.frame(Vec::new());
        run.frame(Vec::new());
        let rows = run.tab().console.rows.clone();
        let row_of = |text: &str| {
            rows.iter().find(|(index, _)| run.tab().console.lines.get(*index).unwrap().text == text).unwrap().1
        };
        let (alpha, beta) = (row_of("alpha"), row_of("beta"));
        let start = alpha.left_center() + vec2(CONSOLE_INSET + 0.5, 0.0);
        let end = beta.left_center() + vec2(CONSOLE_INSET + 300.0, 0.0);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        run.frame(vec![Event::PointerMoved(start)]);
        run.frame(vec![button(start, true)]);
        run.frame(vec![Event::PointerMoved(start + vec2(20.0, 0.0))]);
        run.frame(vec![Event::PointerMoved(end)]);
        run.frame(vec![button(end, false)]);
        assert_eq!(run.tab().selected_text().as_deref(), Some("alpha\nbeta"));
    }

    #[test]
    fn the_tests_and_the_console_are_read_by_screen_readers() {
        let mut run = Harness::new();
        run.fake.set_script(&TESTS, 1);
        run.start_named("tests");
        run.frame(Vec::new());
        run.frame(Vec::new());
        let tree = accessibility::testing::tree_of(&run.ctx, vec2(900.0, 400.0), |ui| {
            let _ = run.window.show(ui, &IdePalette::zen_dark());
        });
        assert_eq!(accessibility::unnamed(&tree), Vec::<String>::new());
        let failed = accessibility::testing::find(&tree, egui::Role::TreeItem, "TestBad").unwrap();
        assert_eq!(failed.is_selected(), Some(true), "the first failure is selected");
        let console = accessibility::testing::find(&tree, egui::Role::Log, &t!("a11y-console")).unwrap();
        assert!(console.value().is_some_and(|text| text.contains("boom")), "{:?}", console.value());
        assert_eq!(console.live(), None, "the console itself is not a live region, its announcements are");
        assert!(accessibility::testing::find(&tree, egui::Role::Tab, "tests").is_some());
    }
}
