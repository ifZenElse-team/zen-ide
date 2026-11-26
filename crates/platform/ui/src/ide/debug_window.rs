use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Key, Modifiers, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_run::{Configuration, ConsoleLine};
use zen_workbench::Workbench;
use zen_workbench::run::Runs;

use super::debug_coroutines::CoroutinesView;
use super::evaluate::Evaluation;
use super::glyphs::{
    Glyph, glyph_bug, glyph_minus, glyph_mute_breakpoints, glyph_pause, glyph_plus, glyph_rerun, glyph_reset_frame,
    glyph_resume, glyph_run_to_cursor, glyph_step_into, glyph_step_out, glyph_step_over, glyph_stop,
};
use super::run_window::{Console, Shown, Tool, call, char_column, error_color, message, resolve_link};
use super::{IdePalette, IdeView, Side, customization, hint, hover, metrics};
use crate::{accessibility, theme};

const BAR: f32 = 34.0;
const BUTTON: f32 = 26.0;
const FIELD_ROW: f32 = 32.0;
pub(super) const PAGE: usize = 100;
const FRAMES: usize = 200;
const THREADS_SHOWN: usize = 100;
const DEPTH: usize = 32;
const MORE: &str = "\u{0}more";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum SessionState {
    #[default]
    Starting,
    Running,
    Stopped,
    #[serde(other)]
    Ended,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SessionInfo {
    #[serde(alias = "id")]
    session_id: Value,
    run_id: Option<u64>,
    name: String,
    state: SessionState,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct SessionList {
    sessions: Vec<SessionInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct FrameRef {
    path: PathBuf,
    line: u32,
    name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(super) struct ThreadInfo {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SessionAnswer {
    state: SessionState,
    reason: Option<String>,
    thread_id: Option<u64>,
    frame: Option<FrameRef>,
    threads: Vec<ThreadInfo>,
    stops: u64,
    support: Support,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Support {
    set_value: bool,
    reset_frame: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(super) struct FrameInfo {
    pub id: u64,
    pub name: String,
    pub path: Option<PathBuf>,
    pub line: u32,
    pub column: u32,
    pub hint: Option<String>,
    #[serde(rename = "async")]
    pub is_async: bool,
    #[serde(rename = "canRestart")]
    pub can_restart: bool,
}

impl FrameInfo {
    fn is_separator(&self) -> bool {
        self.hint.as_deref() == Some("label")
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct FramesAnswer {
    frames: Vec<FrameInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(super) struct Variable {
    pub name: String,
    pub value: String,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    pub reference: u64,
    pub indexed: Option<usize>,
    pub named: Option<usize>,
    #[serde(rename = "evaluateName")]
    pub evaluate_name: Option<String>,
    pub entry: Option<MapEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(super) struct MapEntry {
    pub key: String,
    pub value: String,
}

impl Variable {
    pub(super) fn total(&self) -> Option<usize> {
        (self.indexed.is_some() || self.named.is_some()).then(|| self.indexed.unwrap_or(0) + self.named.unwrap_or(0))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct VariablesAnswer {
    variables: Vec<Variable>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(super) struct Watch {
    pub expression: String,
    pub value: Option<String>,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    pub reference: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct WatchesAnswer {
    watches: Vec<Watch>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Started {
    session_id: Value,
    run_id: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct OutputAnswer {
    lines: Vec<ConsoleLine>,
    next: usize,
}

pub(super) type RowKey = Vec<String>;

pub(super) type ChildKey = (u64, bool);

pub(super) type Missing = Vec<(u64, bool, Option<usize>)>;

pub(super) struct TreeState<'a> {
    pub expanded: &'a BTreeSet<RowKey>,
    pub children: &'a HashMap<ChildKey, Vec<Variable>>,
    pub raw: &'a BTreeSet<RowKey>,
    pub all_raw: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum DebugAction {
    Hide,
    Rerun { session: Value, live: bool, configuration: Box<Configuration> },
    Stop { session: Value },
    Close { session: Value, live: bool },
    Execute { command: &'static str, session: Value, thread: Option<u64> },
    RunToCursor,
    Mute { muted: bool },
    SelectThread { thread: u64 },
    SelectFrame { frame: u64 },
    ResetFrame { session: Value, frame: u64 },
    Expand { reference: u64, raw: bool, total: Option<usize> },
    More { reference: u64, raw: bool, start: usize, total: Option<usize> },
    RawRow { key: RowKey },
    RawAll { raw: bool },
    Evaluate { expression: String },
    AddWatch { expression: String },
    RemoveWatch { expression: String },
    EditWatch { old: String, new: String },
    SetValue { key: RowKey, reference: u64, name: String, evaluate_name: Option<String>, value: String },
    Open { path: PathBuf, line: u32, column: Option<u32> },
}

impl DebugAction {
    pub(super) fn commands(&self) -> Vec<(&'static str, Value)> {
        let stop = |session: &Value| ("debug.session.stop", json!({ "sessionId": session }));
        match self {
            Self::Rerun { session, live, configuration } => {
                let mut commands = if *live { vec![stop(session)] } else { Vec::new() };
                commands.push(("debug.session.start", json!({ "configuration": configuration })));
                commands
            }
            Self::Stop { session } => vec![stop(session)],
            Self::Close { session, live: true } => vec![stop(session)],
            Self::Execute { command, session, thread } => {
                let mut params = json!({ "sessionId": session });
                if let Some(thread) = thread {
                    params["threadId"] = json!(thread);
                }
                vec![(command, params)]
            }
            Self::RunToCursor => vec![("debug.execution.runToCursor", json!({}))],
            Self::Mute { muted } => vec![("debug.breakpoints.mute", json!({ "muted": muted }))],
            Self::SelectFrame { frame } => vec![("debug.frame.select", json!({ "frameId": frame }))],
            Self::ResetFrame { session, frame } => {
                vec![("debug.frame.reset", json!({ "sessionId": session, "frameId": frame }))]
            }
            Self::AddWatch { expression } => vec![("debug.watch.add", json!({ "expression": expression }))],
            Self::RemoveWatch { expression } => vec![("debug.watch.remove", json!({ "expression": expression }))],
            Self::EditWatch { old, new } => vec![
                ("debug.watch.remove", json!({ "expression": old })),
                ("debug.watch.add", json!({ "expression": new })),
            ],
            Self::Open { path, .. } => vec![("file.document.open", json!({ "path": path }))],
            Self::RawAll { raw } => vec![("debug.variables.raw", json!({ "raw": raw }))],
            Self::Hide
            | Self::Close { .. }
            | Self::SelectThread { .. }
            | Self::Expand { .. }
            | Self::More { .. }
            | Self::RawRow { .. }
            | Self::Evaluate { .. }
            | Self::SetValue { .. } => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Content {
    #[default]
    Variables,
    Console,
    Coroutines,
}

pub(super) struct VarRow {
    pub key: RowKey,
    pub depth: usize,
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
    pub error: bool,
    pub reference: u64,
    pub total: Option<usize>,
    pub more: Option<(u64, usize, Option<usize>)>,
    pub watch: bool,
    pub parent: u64,
    pub evaluate_name: Option<String>,
    pub raw: bool,
    pub entry: Option<MapEntry>,
}

impl VarRow {
    pub(super) fn of(key: RowKey, depth: usize, variable: &Variable) -> Self {
        Self {
            key,
            depth,
            name: variable.name.clone(),
            value: variable.value.clone(),
            type_name: variable.type_name.clone(),
            error: false,
            reference: variable.reference,
            total: variable.total(),
            more: None,
            watch: false,
            parent: 0,
            evaluate_name: variable.evaluate_name.clone(),
            raw: false,
            entry: variable.entry.clone(),
        }
    }
}

pub(super) fn push_rows(row: VarRow, state: &TreeState<'_>, rows: &mut Vec<VarRow>, missing: &mut Missing) {
    let open = row.reference > 0 && row.depth < DEPTH && state.expanded.contains(&row.key);
    let raw = state.all_raw != state.raw.contains(&row.key);
    let (key, depth, reference, total) = (row.key.clone(), row.depth, row.reference, row.total);
    rows.push(VarRow { raw, ..row });
    if !open {
        return;
    }
    let Some(known) = state.children.get(&(reference, raw)) else {
        missing.push((reference, raw, total));
        return;
    };
    for child in known {
        let mut child_key = key.clone();
        child_key.push(child.name.clone());
        let child_row = VarRow { parent: reference, ..VarRow::of(child_key, depth + 1, child) };
        push_rows(child_row, state, rows, missing);
    }
    if let Some(total) = total.filter(|total| known.len() < *total) {
        let mut more_key = key;
        more_key.push(MORE.to_owned());
        rows.push(VarRow {
            key: more_key,
            depth: depth + 1,
            name: t!("ide-debug-more"),
            value: String::new(),
            type_name: None,
            error: false,
            reference: 0,
            total: None,
            more: Some((reference, known.len(), Some(total))),
            watch: false,
            parent: 0,
            evaluate_name: None,
            raw,
            entry: None,
        });
    }
}

pub(super) fn plain_value(value: &str) -> (Option<&str>, &str) {
    let Some((object, rest)) = value.split_once(" \"") else { return (None, value) };
    let looks_like_object = object.split_once('@').is_some_and(|(type_name, id)| {
        !type_name.is_empty()
            && !type_name.contains(char::is_whitespace)
            && !id.is_empty()
            && id.chars().all(|c| c.is_ascii_hexdigit())
    });
    match rest.strip_suffix('"') {
        Some(text) if looks_like_object && !text.contains("\":\"") => (Some(object), text),
        _ => (None, value),
    }
}

pub(super) fn paint_row(ui: &Ui, p: &IdePalette, row: &VarRow, rect: Rect, open: bool) -> Rect {
    let left = rect.left() + 8.0 + row.depth as f32 * metrics::TREE_INDENT;
    let chevron = Rect::from_center_size(pos2(left + 6.0, rect.center().y), Vec2::splat(12.0));
    if row.reference > 0 {
        if open {
            crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
        } else {
            crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
        }
    }
    let galley = ui.painter().layout_job(row_job(row, p));
    ui.painter().with_clip_rect(rect).galley(
        pos2(left + 18.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    chevron
}

#[derive(Debug)]
pub(super) struct DebugTab {
    session: Value,
    run_id: Option<u64>,
    name: String,
    configuration: Option<Configuration>,
    cwd: PathBuf,
    root: PathBuf,
    state: SessionState,
    last: Option<SessionAnswer>,
    threads: Vec<ThreadInfo>,
    thread: Option<u64>,
    frames: Vec<FrameInfo>,
    frame: Option<u64>,
    scopes: Vec<Variable>,
    scopes_seen: bool,
    watches: Vec<Watch>,
    result: Option<Evaluation>,
    children: HashMap<ChildKey, Vec<Variable>>,
    expanded: BTreeSet<RowKey>,
    raw_rows: BTreeSet<RowKey>,
    all_raw: bool,
    asking: Vec<(ChildKey, usize, Option<usize>)>,
    selected: Option<RowKey>,
    editing: Option<(String, String, bool)>,
    set_value: bool,
    reset_frame: bool,
    value_edit: Option<(RowKey, String, bool)>,
    value_error: Option<(RowKey, String)>,
    setting: Option<(Value, RowKey)>,
    console: Console,
    seen: Option<u64>,
    frame_rows: Vec<(u64, Rect)>,
    frame_menu: Option<Rect>,
    var_rows: Vec<(RowKey, Rect)>,
    jvm: bool,
    coroutines: CoroutinesView,
}

impl DebugTab {
    fn new(info: &SessionInfo, session: &Session) -> Self {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        let run = info.run_id.and_then(|id| session.state::<Runs>().and_then(|runs| runs.run(id)));
        let cwd = run.and_then(|run| run.command_lines.first()).map_or_else(|| root.clone(), |line| line.cwd.clone());
        let name = if info.name.is_empty() {
            run.map(|run| run.configuration.name.clone()).unwrap_or_default()
        } else {
            info.name.clone()
        };
        Self {
            session: info.session_id.clone(),
            run_id: info.run_id,
            name,
            configuration: run.map(|run| run.configuration.clone()),
            cwd,
            root,
            state: info.state,
            last: None,
            threads: Vec::new(),
            thread: None,
            frames: Vec::new(),
            frame: None,
            scopes: Vec::new(),
            scopes_seen: false,
            watches: Vec::new(),
            result: None,
            children: HashMap::new(),
            expanded: BTreeSet::new(),
            raw_rows: BTreeSet::new(),
            all_raw: zen_workbench::debug::views::shows_raw(session),
            asking: Vec::new(),
            selected: None,
            editing: None,
            set_value: false,
            reset_frame: false,
            value_edit: None,
            value_error: None,
            setting: None,
            console: Console::default(),
            seen: None,
            frame_rows: Vec::new(),
            frame_menu: None,
            var_rows: Vec::new(),
            jvm: run.is_some_and(|run| zen_workbench::java::coroutines::jvm_kind(&run.configuration.kind)),
            coroutines: CoroutinesView::default(),
        }
    }

    fn coroutines_stop(&self) -> Option<u64> {
        (self.state == SessionState::Stopped).then(|| self.last.as_ref().map_or(0, |last| last.stops))
    }

    fn restart(&mut self, started: &Started, session: &Session) {
        let info = SessionInfo {
            session_id: started.session_id.clone(),
            run_id: started.run_id,
            name: self.name.clone(),
            state: SessionState::Starting,
        };
        let expanded = std::mem::take(&mut self.expanded);
        *self = Self { expanded, scopes_seen: true, ..Self::new(&info, session) };
    }

    fn is_live(&self) -> bool {
        self.state != SessionState::Ended
    }

    fn look(&self, p: &IdePalette) -> (Glyph, egui::Color32) {
        match self.state {
            SessionState::Starting | SessionState::Running => (glyph_bug, p.run),
            SessionState::Stopped => (glyph_pause, p.accent),
            SessionState::Ended => (glyph_bug, p.text_secondary),
        }
    }

    fn read_state(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let params = json!({ "sessionId": self.session, "waitMs": 0 });
        let answer = match call(registry, session, "debug.session.get", params) {
            Ok(value) => serde_json::from_value::<SessionAnswer>(value).unwrap_or_default(),
            Err(_) => SessionAnswer { state: SessionState::Ended, ..SessionAnswer::default() },
        };
        if self.last.as_ref() == Some(&answer) {
            return false;
        }
        let stopped = answer.state == SessionState::Stopped;
        self.state = answer.state;
        self.threads = answer.threads.clone();
        self.set_value = answer.support.set_value;
        self.reset_frame = answer.support.reset_frame;
        let mut new_stop = false;
        if stopped {
            let last = self.last.as_ref();
            new_stop = last.is_none_or(|last| {
                last.state != SessionState::Stopped
                    || last.stops != answer.stops
                    || last.thread_id != answer.thread_id
                    || last.frame != answer.frame
                    || last.reason != answer.reason
            });
        }
        if new_stop || !stopped {
            self.value_edit = None;
            self.value_error = None;
            self.setting = None;
        }
        if new_stop {
            self.thread = answer.thread_id.or(self.thread).or_else(|| self.threads.first().map(|thread| thread.id));
            self.read_frames(registry, session);
            let at = answer.frame.as_ref().and_then(|frame| {
                self.frames.iter().find(|candidate| {
                    candidate.path.as_deref() == Some(frame.path.as_path()) && candidate.line == frame.line
                })
            });
            self.frame = at.or(self.frames.iter().find(|frame| !frame.is_separator())).map(|frame| frame.id);
            self.read_variables(registry, session);
        } else if !stopped {
            self.result = None;
            self.frames.clear();
            self.frame = None;
            self.scopes.clear();
            self.children.clear();
            self.asking.clear();
        }
        self.last = Some(answer);
        self.read_watches(registry, session);
        new_stop
    }

    fn read_frames(&mut self, registry: &Registry, session: &mut Session) {
        let mut params = json!({ "sessionId": self.session, "start": 0, "count": FRAMES });
        if let Some(thread) = self.thread {
            params["threadId"] = json!(thread);
        }
        self.frames = call(registry, session, "debug.frames.list", params)
            .ok()
            .and_then(|value| serde_json::from_value::<FramesAnswer>(value).ok())
            .map(|answer| answer.frames)
            .unwrap_or_default();
    }

    fn read_variables(&mut self, registry: &Registry, session: &mut Session) {
        self.children.clear();
        self.asking.clear();
        self.scopes.clear();
        let Some(frame) = self.frame else { return };
        let params = json!({ "sessionId": self.session, "frameId": frame });
        self.scopes = call(registry, session, "debug.variables.list", params)
            .ok()
            .and_then(|value| serde_json::from_value::<VariablesAnswer>(value).ok())
            .map(|answer| answer.variables)
            .unwrap_or_default();
        if !self.scopes_seen
            && let Some(first) = self.scopes.first()
        {
            self.scopes_seen = true;
            self.expanded.insert(vec!["scope".to_owned(), first.name.clone()]);
        }
        self.load_missing(registry, session);
    }

    fn read_watches(&mut self, registry: &Registry, session: &mut Session) {
        let Ok(value) = call(registry, session, "debug.watch.list", json!({})) else { return };
        if let Ok(answer) = serde_json::from_value::<WatchesAnswer>(value) {
            for watch in &self.watches {
                if let Some(reference) = watch.reference.filter(|reference| *reference > 0) {
                    self.children.remove(&(reference, false));
                    self.children.remove(&(reference, true));
                }
            }
            self.watches = answer.watches;
            self.load_missing(registry, session);
        }
    }

    fn ask_page(&mut self, key: ChildKey, start: usize, total: Option<usize>) {
        if !self.asking.iter().any(|(asked, from, _)| *asked == key && *from == start) {
            self.asking.push((key, start, total));
        }
    }

    fn load_missing(&mut self, registry: &Registry, session: &mut Session) {
        for _ in 0..DEPTH {
            if !self.read_pages(registry, session) {
                return;
            }
        }
    }

    fn read_pages(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let (_, missing) = self.rows();
        for (reference, raw, total) in missing {
            self.ask_page((reference, raw), 0, total);
        }
        let mut asking = std::mem::take(&mut self.asking);
        let asked = asking.len();
        asking.retain(|&((reference, raw), start, total)| {
            let mut params = json!({ "sessionId": self.session, "reference": reference, "raw": raw, "waitMs": 0 });
            if let Some(total) = total.filter(|total| *total > 0) {
                params["start"] = json!(start);
                params["count"] = json!(PAGE.min(total.saturating_sub(start)));
            }
            match call(registry, session, "debug.variables.list", params) {
                Ok(answer) if answer["pending"] == true => true,
                answer => {
                    let page = answer
                        .ok()
                        .and_then(|value| serde_json::from_value::<VariablesAnswer>(value).ok())
                        .map(|answer| answer.variables)
                        .unwrap_or_default();
                    let known = self.children.entry((reference, raw)).or_default();
                    if start == 0 {
                        known.clear();
                    }
                    known.extend(page);
                    false
                }
            }
        });
        let answered = asking.len() < asked;
        self.asking = asking;
        answered
    }

    fn refresh_console(&mut self, registry: &Registry, session: &mut Session) {
        let Some(run_id) = self.run_id else { return };
        let revision = session
            .state::<Runs>()
            .and_then(|runs| runs.run(run_id))
            .map(|run| run.output.read(|output| output.revision()));
        if revision.is_none() || self.seen == revision {
            return;
        }
        self.seen = revision;
        let params = json!({ "runId": run_id, "from": self.console.next(), "waitMs": 0 });
        if let Ok(value) = registry.dispatch(session, "run.output.get", params)
            && let Ok(answer) = serde_json::from_value::<OutputAnswer>(value)
        {
            self.console.extend(answer.lines, answer.next);
        }
    }

    fn rows(&self) -> (Vec<VarRow>, Missing) {
        let mut rows = Vec::new();
        let mut missing = Vec::new();
        for watch in &self.watches {
            let row = VarRow {
                key: vec!["watch".to_owned(), watch.expression.clone()],
                depth: 0,
                name: watch.expression.clone(),
                value: watch.error.clone().or_else(|| watch.value.clone()).unwrap_or_default(),
                type_name: watch.type_name.clone(),
                error: watch.error.is_some(),
                reference: watch.reference.unwrap_or(0),
                total: None,
                more: None,
                watch: true,
                parent: 0,
                evaluate_name: None,
                raw: false,
                entry: None,
            };
            self.push(row, &mut rows, &mut missing);
        }
        for scope in &self.scopes {
            let row = VarRow {
                key: vec!["scope".to_owned(), scope.name.clone()],
                depth: 0,
                name: scope.name.clone(),
                value: String::new(),
                type_name: None,
                error: false,
                reference: scope.reference,
                total: scope.total(),
                more: None,
                watch: false,
                parent: 0,
                evaluate_name: None,
                raw: false,
                entry: None,
            };
            self.push(row, &mut rows, &mut missing);
        }
        (rows, missing)
    }

    fn push(&self, row: VarRow, rows: &mut Vec<VarRow>, missing: &mut Missing) {
        let state = TreeState {
            expanded: &self.expanded,
            children: &self.children,
            raw: &self.raw_rows,
            all_raw: self.all_raw,
        };
        push_rows(row, &state, rows, missing);
    }

    fn settable(&self, row: &VarRow) -> bool {
        self.set_value && self.state == SessionState::Stopped && row.parent > 0 && row.more.is_none()
    }

    fn start_value_edit(&mut self, row: &VarRow) {
        self.value_error = None;
        self.value_edit = Some((row.key.clone(), row.value.clone(), true));
    }

    fn poll_setting(&mut self, registry: &Registry, session: &mut Session) {
        let Some((params, key)) = self.setting.take() else { return };
        match call(registry, session, "debug.variable.set", params.clone()) {
            Ok(answer) if answer["pending"] == true => self.setting = Some((params, key)),
            Ok(_) => {
                self.read_variables(registry, session);
                self.read_watches(registry, session);
            }
            Err(message) => self.value_error = Some((key, message)),
        }
    }

    fn toolbar(
        &self,
        ui: &Ui,
        p: &IdePalette,
        (left, muted): (f32, bool),
        bar: Rect,
        actions: &mut Vec<DebugAction>,
    ) -> Vec<(&'static str, Rect)> {
        let stopped = self.state == SessionState::Stopped;
        let running = self.state == SessionState::Running;
        let live = self.is_live();
        let session = self.session.clone();
        let thread = self.thread;
        let execute = |command: &'static str| DebugAction::Execute { command, session: session.clone(), thread };
        let shortcut = crate::keymap::shortcut;
        let rerun = self.configuration.clone().map(|configuration| DebugAction::Rerun {
            session: session.clone(),
            live,
            configuration: Box::new(configuration),
        });
        let stop_shortcut = shortcut("debug.session.stop").or_else(|| shortcut("run.process.stop"));
        let tool = |salt: &'static str, glyph: Glyph, name: &str, command: Option<&str>, color| Tool {
            shortcut: command.and_then(shortcut),
            ..Tool::new(salt, glyph, t!(name), color)
        };
        let stop = Tool {
            shortcut: stop_shortcut,
            ..Tool::new("debug-stop", glyph_stop, t!("ide-debug-stop"), error_color(p))
        };
        let buttons: Vec<(Tool, bool, Option<DebugAction>)> = vec![
            (tool("debug-rerun", glyph_rerun, "ide-debug-rerun", None, p.run), rerun.is_some(), rerun),
            (stop, live, Some(DebugAction::Stop { session: session.clone() })),
            (
                tool("debug-resume", glyph_resume, "ide-debug-resume", Some("debug.execution.resume"), p.run),
                stopped,
                Some(execute("debug.execution.resume")),
            ),
            (
                tool("debug-pause", glyph_pause, "ide-debug-pause", Some("debug.execution.pause"), p.text),
                running,
                Some(execute("debug.execution.pause")),
            ),
            (
                tool(
                    "debug-step-over",
                    glyph_step_over,
                    "ide-debug-step-over",
                    Some("debug.execution.stepOver"),
                    p.accent,
                ),
                stopped,
                Some(execute("debug.execution.stepOver")),
            ),
            (
                tool(
                    "debug-step-into",
                    glyph_step_into,
                    "ide-debug-step-into",
                    Some("debug.execution.stepInto"),
                    p.accent,
                ),
                stopped,
                Some(execute("debug.execution.stepInto")),
            ),
            (
                tool("debug-step-out", glyph_step_out, "ide-debug-step-out", Some("debug.execution.stepOut"), p.accent),
                stopped,
                Some(execute("debug.execution.stepOut")),
            ),
            (
                tool(
                    "debug-run-to-cursor",
                    glyph_run_to_cursor,
                    "ide-debug-run-to-cursor",
                    Some("debug.execution.runToCursor"),
                    p.accent,
                ),
                stopped,
                Some(DebugAction::RunToCursor),
            ),
        ];
        let mut buttons = buttons;
        if self.reset_frame {
            let selected = self.frame.and_then(|id| self.frames.iter().find(|frame| frame.id == id));
            let frame = selected.filter(|frame| frame.can_restart).map(|frame| frame.id);
            buttons.push((
                tool(
                    "debug-reset-frame",
                    glyph_reset_frame,
                    "ide-debug-reset-frame",
                    Some("debug.frame.reset"),
                    p.accent,
                ),
                stopped && frame.is_some(),
                frame.map(|frame| DebugAction::ResetFrame { session: session.clone(), frame }),
            ));
        }
        buttons.push((
            Tool {
                on: Some(muted),
                shortcut: shortcut("debug.breakpoints.mute"),
                ..Tool::new("debug-mute", glyph_mute_breakpoints, t!("ide-debug-mute"), p.text)
            },
            true,
            Some(DebugAction::Mute { muted: !muted }),
        ));
        let mut x = left;
        let mut drawn = Vec::new();
        for (index, (tool, enabled, action)) in buttons.into_iter().enumerate() {
            if index == 2 || index == 4 || tool.salt == "debug-mute" {
                ui.painter().vline(x + 2.0, bar.shrink2(vec2(0.0, 9.0)).y_range(), Stroke::new(1.0, p.tab_border));
                x += 8.0;
            }
            let rect = Rect::from_center_size(pos2(x + BUTTON / 2.0, bar.center().y), Vec2::splat(BUTTON));
            let salt = tool.salt;
            if tool.show_hinted(ui, p, rect, enabled, Side::Below)
                && let Some(action) = action
            {
                actions.push(action);
            }
            drawn.push((salt, rect));
            x = rect.right() + 4.0;
        }
        drawn
    }

    fn frames_pane(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<DebugAction>) {
        let rect = ui.max_rect();
        let combo =
            Rect::from_min_max(rect.min + vec2(8.0, 4.0), pos2(rect.right() - 8.0, rect.top() + FIELD_ROW - 4.0));
        let current = self.thread.and_then(|id| self.threads.iter().find(|thread| thread.id == id));
        let label = current.map_or_else(|| t!("ide-debug-no-threads"), |thread| thread.name.clone());
        let threads = &self.threads;
        let selected = self.thread;
        ui.scope_builder(UiBuilder::new().max_rect(combo), |ui| {
            let threads_combo = egui::ComboBox::from_id_salt(("debug-threads", self.session.to_string()))
                .width(combo.width())
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for thread in threads.iter().take(THREADS_SHOWN) {
                        if ui.selectable_label(selected == Some(thread.id), &thread.name).clicked() {
                            actions.push(DebugAction::SelectThread { thread: thread.id });
                        }
                    }
                    if threads.len() > THREADS_SHOWN {
                        ui.label(
                            egui::RichText::new(t!("ide-debug-threads-more", count = threads.len() - THREADS_SHOWN))
                                .color(p.text_secondary),
                        );
                    }
                });
            accessibility::edit(&threads_combo.response, |node| node.set_label(t!("a11y-threads")));
        });
        let list = Rect::from_min_max(pos2(rect.left(), rect.top() + FIELD_ROW), rect.max);
        if self.frames.is_empty() {
            ui.painter().text(
                pos2(list.left() + 14.0, list.top() + 14.0),
                Align2::LEFT_TOP,
                t!("ide-debug-frames-empty"),
                theme::regular(13.0),
                p.text_secondary,
            );
            self.frame_rows.clear();
            return;
        }
        let mut drawn = Vec::new();
        let mut clicked = None;
        let mut reset = None;
        let mut menu = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("debug-frames", self.session.to_string()))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for frame in &self.frames {
                        if frame.is_separator() {
                            let (row, response) =
                                ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::hover());
                            let label = if frame.name.trim().is_empty() {
                                t!("ide-debug-async-separator")
                            } else {
                                frame.name.clone()
                            };
                            accessibility::name(&response, egui::Role::Label, &label);
                            let painter = ui.painter().with_clip_rect(row);
                            let text = painter.text(
                                pos2(row.left() + 14.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &label,
                                theme::regular(12.0),
                                p.text_secondary,
                            );
                            let line = Stroke::new(1.0, p.tab_border);
                            painter.hline((text.right() + 6.0)..=(row.right() - 8.0), row.center().y, line);
                            continue;
                        }
                        let (row, response) =
                            ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                        drawn.push((frame.id, row));
                        let highlight = row.shrink2(vec2(4.0, 0.0));
                        if self.frame == Some(frame.id) {
                            ui.painter().rect_filled(highlight, 4, p.selection);
                        } else if response.hovered() {
                            ui.painter().rect_filled(highlight, 4, p.hover);
                        }
                        let file = frame
                            .path
                            .as_ref()
                            .and_then(|path| path.file_name())
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let text = t!("ide-debug-frame", name = frame.name.clone(), line = frame.line + 1, file = file);
                        accessibility::item(&response, egui::Role::ListBoxOption, self.frame == Some(frame.id), &text);
                        let plain = frame.path.is_some() && frame.hint.as_deref() != Some("subtle");
                        let color = if plain { p.text } else { p.text_secondary };
                        ui.painter().with_clip_rect(row).text(
                            pos2(row.left() + 14.0, row.center().y),
                            Align2::LEFT_CENTER,
                            text,
                            theme::regular(13.0),
                            color,
                        );
                        if response.clicked() || response.secondary_clicked() {
                            clicked = Some(frame.id);
                        }
                        let group = customization::DEBUG_FRAME_POPUP;
                        let available = self.reset_frame;
                        if customization::shows_anything(group, |_| available) {
                            response.context_menu(|ui| {
                                customization::popup(
                                    ui,
                                    group,
                                    |_| available,
                                    |ui, _| {
                                        let mut item = egui::Button::new(t!("ide-debug-reset-frame"));
                                        if let Some(shortcut) = crate::keymap::shortcut("debug.frame.reset") {
                                            item = item.shortcut_text(shortcut);
                                        }
                                        let item = ui.add_enabled(frame.can_restart, item);
                                        menu = Some(item.rect);
                                        if item.clicked() {
                                            reset = Some(frame.id);
                                            ui.close();
                                        }
                                    },
                                );
                            });
                        }
                    }
                });
        });
        self.frame_rows = drawn;
        self.frame_menu = menu;
        if let Some(frame) = clicked {
            actions.push(DebugAction::SelectFrame { frame });
        }
        if let Some(frame) = reset {
            actions.push(DebugAction::ResetFrame { session: self.session.clone(), frame });
        }
    }

    fn variables_pane(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        new_watch: &mut String,
        actions: &mut Vec<DebugAction>,
    ) -> (Rect, Rect) {
        let rect = ui.max_rect();
        let row = Rect::from_min_size(rect.min, vec2(rect.width(), FIELD_ROW));
        let remove = Rect::from_center_size(pos2(row.right() - 20.0, row.center().y), Vec2::splat(BUTTON));
        let add = Rect::from_center_size(pos2(remove.left() - BUTTON / 2.0 - 4.0, row.center().y), Vec2::splat(BUTTON));
        let field_rect =
            Rect::from_min_max(pos2(row.left() + 12.0, row.top() + 6.0), pos2(add.left() - 8.0, row.bottom() - 6.0));
        let field = ui.put(
            field_rect,
            egui::TextEdit::singleline(new_watch)
                .id(ui.scope_id().with("debug-new-watch"))
                .frame(egui::Frame::NONE)
                .font(theme::regular(13.0))
                .hint_text(field_hint())
                .desired_width(field_rect.width()),
        );
        let hint = field_hint();
        accessibility::edit(&field, |node| node.set_label(hint.as_str()));
        let watch_keys = Modifiers::COMMAND | Modifiers::SHIFT;
        let watched = field.has_focus() && ui.input_mut(|input| input.consume_key(watch_keys, Key::Enter));
        let entered = field.lost_focus() && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
        let plus = Tool::new("debug-watch-add", glyph_plus, t!("ide-debug-watch-add"), p.text);
        let added = plus.show_hinted(ui, p, add, true, Side::Below);
        if (watched || added) && !new_watch.trim().is_empty() {
            actions.push(DebugAction::AddWatch { expression: new_watch.trim().to_owned() });
            new_watch.clear();
        } else if entered && !new_watch.trim().is_empty() {
            actions.push(DebugAction::Evaluate { expression: new_watch.trim().to_owned() });
            field.request_focus();
        }
        let selected_watch =
            self.selected.as_ref().filter(|key| key.len() == 2 && key[0] == "watch").map(|key| key[1].clone());
        let minus = Tool::new("debug-watch-remove", glyph_minus, t!("ide-debug-watch-remove"), p.text);
        if minus.show_hinted(ui, p, remove, selected_watch.is_some(), Side::Below)
            && let Some(expression) = selected_watch
        {
            actions.push(DebugAction::RemoveWatch { expression });
        }
        ui.painter().hline(rect.x_range(), row.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
        let mut top = row.bottom();
        if let Some(result) = self.result.as_mut() {
            let height = super::evaluate::result_height(result);
            let area =
                Rect::from_min_max(pos2(rect.left() + 4.0, top + 2.0), pos2(rect.right() - 4.0, top + 2.0 + height));
            ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("debug-evaluated", self.session.to_string()))
                    .max_height(height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| result.show(ui, p));
            });
            top = area.bottom() + 2.0;
            ui.painter().hline(rect.x_range(), top - 0.5, Stroke::new(1.0, p.tab_border));
        }
        let tree = Rect::from_min_max(pos2(rect.left(), top), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(tree), |ui| self.tree(ui, p, actions));
        (add, remove)
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<DebugAction>) {
        let id = ui.make_persistent_id(("debug-variables", self.session.to_string()));
        let area = ui.interact(ui.max_rect(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("ide-debug-threads-variables"));
        if area.clicked() {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        let focused = ui.memory(|memory| memory.has_focus(id));
        if focused && self.editing.is_none() && self.value_edit.is_none() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: false },
                );
            });
            self.keys(ui, actions);
        }
        let (rows, _) = self.rows();
        if rows.is_empty() {
            let text = match self.state {
                SessionState::Starting => t!("ide-debug-starting"),
                SessionState::Running => t!("ide-debug-running"),
                SessionState::Stopped => t!("ide-debug-variables-empty"),
                SessionState::Ended => t!("ide-debug-ended"),
            };
            let rect = ui.max_rect();
            ui.painter().text(
                rect.min + vec2(14.0, 14.0),
                Align2::LEFT_TOP,
                text,
                theme::regular(13.0),
                p.text_secondary,
            );
            self.var_rows.clear();
            return;
        }
        let mut drawn = Vec::with_capacity(rows.len());
        let mut clicked: Option<(usize, bool, bool)> = None;
        let mut edited: Option<DebugAction> = None;
        let mut cancel = false;
        let mut cancel_value = false;
        let mut menu_edit: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt(("debug-variables-scroll", self.session.to_string()))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(2.0);
                for (index, row) in rows.iter().enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                    drawn.push((row.key.clone(), rect));
                    let expanded = (row.reference > 0).then(|| self.expanded.contains(&row.key));
                    accessibility::name_with(&response, egui::Role::TreeItem, || {
                        format!("{} = {}", row.name, row.value)
                    });
                    accessibility::edit(&response, |node| {
                        node.set_level(row.depth + 1);
                        node.set_selected(self.selected.as_ref() == Some(&row.key));
                        if let Some(expanded) = expanded {
                            node.set_expanded(expanded);
                        }
                    });
                    let highlight = rect.shrink2(vec2(4.0, 0.0));
                    if self.selected.as_ref() == Some(&row.key) {
                        ui.painter().rect_filled(highlight, 4, if focused { p.selection } else { p.hover });
                    } else if response.hovered() {
                        ui.painter().rect_filled(highlight, 4, p.hover);
                    }
                    let text_left = rect.left() + 26.0 + row.depth as f32 * metrics::TREE_INDENT;
                    if row.watch
                        && let Some((old, text, focus)) = self.editing.as_mut()
                        && *old == row.name
                    {
                        let field_rect = Rect::from_min_max(
                            pos2(text_left, rect.top() + 3.0),
                            pos2(rect.right() - 8.0, rect.bottom() - 3.0),
                        );
                        let field = ui.put(
                            field_rect,
                            egui::TextEdit::singleline(text)
                                .id(ui.scope_id().with(("debug-edit-watch", index)))
                                .font(theme::regular(13.0))
                                .desired_width(field_rect.width()),
                        );
                        accessibility::labelled(&field, &row.name);
                        if std::mem::take(focus) {
                            field.request_focus();
                        }
                        if field.lost_focus() {
                            if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter))
                                && !text.trim().is_empty()
                            {
                                edited = Some(DebugAction::EditWatch { old: old.clone(), new: text.trim().to_owned() });
                            }
                            cancel = true;
                        }
                        continue;
                    }
                    if let Some((key, text, focus)) = self.value_edit.as_mut()
                        && *key == row.key
                    {
                        let left = rect.left() + 8.0 + row.depth as f32 * metrics::TREE_INDENT + 18.0;
                        let label =
                            ui.painter().layout_no_wrap(format!("{} = ", row.name), theme::regular(13.0), p.text);
                        let label_width = label.size().x;
                        ui.painter().galley(pos2(left, rect.center().y - label.size().y / 2.0), label, p.text);
                        let field_rect = Rect::from_min_max(
                            pos2(left + label_width, rect.top() + 3.0),
                            pos2(rect.right() - 8.0, rect.bottom() - 3.0),
                        );
                        let field = ui.put(
                            field_rect,
                            egui::TextEdit::singleline(text)
                                .id(ui.scope_id().with(("debug-set-value", index)))
                                .font(theme::regular(13.0))
                                .desired_width(field_rect.width()),
                        );
                        accessibility::labelled(&field, &t!("ide-debug-set-value-of", name = row.name.as_str()));
                        if std::mem::take(focus) {
                            field.request_focus();
                        }
                        if field.lost_focus() {
                            if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
                                edited = Some(DebugAction::SetValue {
                                    key: row.key.clone(),
                                    reference: row.parent,
                                    name: row.name.clone(),
                                    evaluate_name: row.evaluate_name.clone(),
                                    value: text.clone(),
                                });
                            }
                            cancel_value = true;
                        }
                        continue;
                    }
                    let chevron = paint_row(ui, p, row, rect, self.expanded.contains(&row.key));
                    let group = customization::DEBUG_VARIABLE_POPUP;
                    let settable = self.settable(row);
                    let container = row.reference > 0 && row.more.is_none() && (row.depth > 0 || row.watch);
                    let customized = customization::shows_anything(group, |_| settable);
                    if customized || container {
                        response.context_menu(|ui| {
                            customization::popup(
                                ui,
                                group,
                                |_| settable,
                                |ui, _| {
                                    let item = egui::Button::new(t!("ide-debug-set-value")).shortcut_text("F2");
                                    if ui.add(item).clicked() {
                                        menu_edit = Some(index);
                                        ui.close();
                                    }
                                },
                            );
                            if container {
                                if customized {
                                    ui.separator();
                                }
                                if ui.add(egui::Button::selectable(row.raw, t!("ide-debug-raw-fields"))).clicked() {
                                    actions.push(DebugAction::RawRow { key: row.key.clone() });
                                    ui.close();
                                }
                                let all = egui::Button::selectable(self.all_raw, t!("ide-debug-raw-fields-all"));
                                if ui.add(all).clicked() {
                                    actions.push(DebugAction::RawAll { raw: !self.all_raw });
                                    ui.close();
                                }
                            }
                        });
                    }
                    if let Some((_, message)) = self.value_error.as_ref().filter(|(key, _)| *key == row.key) {
                        let (below, _) =
                            ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::hover());
                        let left = rect.left() + 26.0 + row.depth as f32 * metrics::TREE_INDENT;
                        ui.painter().with_clip_rect(below).text(
                            pos2(left, below.center().y),
                            Align2::LEFT_CENTER,
                            message,
                            theme::regular(12.0),
                            error_color(p),
                        );
                    }
                    if response.clicked() || response.double_clicked() {
                        let on_chevron = row.reference > 0
                            && response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                        clicked = Some((index, response.double_clicked(), on_chevron));
                    }
                }
            });
        self.var_rows = drawn;
        if cancel {
            self.editing = None;
        }
        if cancel_value {
            self.value_edit = None;
        }
        if let Some(row) = menu_edit.and_then(|index| rows.get(index)) {
            self.selected = Some(row.key.clone());
            self.start_value_edit(row);
        }
        actions.extend(edited);
        if let Some((index, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            let row = &rows[index];
            self.selected = Some(row.key.clone());
            if let Some((reference, start, total)) = row.more {
                actions.push(DebugAction::More { reference, raw: row.raw, start, total });
            } else if row.reference > 0 && (on_chevron || double) {
                self.toggle(row, actions);
            } else if double && row.watch {
                self.editing = Some((row.name.clone(), row.name.clone(), true));
            }
        }
    }

    fn toggle(&mut self, row: &VarRow, actions: &mut Vec<DebugAction>) {
        if self.expanded.remove(&row.key) {
            return;
        }
        self.expanded.insert(row.key.clone());
        if !self.children.contains_key(&(row.reference, row.raw)) {
            actions.push(DebugAction::Expand { reference: row.reference, raw: row.raw, total: row.total });
        }
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<DebugAction>) {
        let none = Modifiers::NONE;
        let (up, down, left, right, delete, edit) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Delete) || input.consume_key(none, Key::Backspace),
                input.consume_key(none, Key::F2),
            )
        });
        let (rows, _) = self.rows();
        let at = self.selected.as_ref().and_then(|selected| rows.iter().position(|row| row.key == *selected));
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(rows.len().saturating_sub(1)),
            };
            if let Some(row) = rows.get(next) {
                self.selected = Some(row.key.clone());
            }
            return;
        }
        let Some(row) = at.and_then(|at| rows.get(at)) else { return };
        let open = self.expanded.contains(&row.key);
        if (right && row.reference > 0 && !open) || (left && row.reference > 0 && open) {
            self.toggle(row, actions);
        } else if left && row.depth > 0 {
            let parent = rows[..at.unwrap_or(0)].iter().rev().find(|candidate| candidate.depth < row.depth);
            if let Some(parent) = parent {
                self.selected = Some(parent.key.clone());
            }
        } else if delete && row.watch {
            actions.push(DebugAction::RemoveWatch { expression: row.name.clone() });
        } else if edit && row.watch {
            self.editing = Some((row.name.clone(), row.name.clone(), true));
        } else if edit && self.settable(row) {
            self.start_value_edit(row);
        }
    }

    fn console_pane(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<DebugAction>) {
        let shown = Shown::All(self.console.all());
        let id = ui.make_persistent_id(("debug-console", self.session.to_string()));
        if let Some(link) = self.console.show(ui, p, id, id, &shown)
            && let Some(path) = resolve_link(&link, &self.cwd, &self.root)
        {
            actions.push(DebugAction::Open { path, line: link.line, column: link.column });
        }
    }
}

fn field_hint() -> String {
    let shown =
        |text: &str| zen_core::keymap::Keystroke::parse(text).map(|stroke| stroke.to_string()).unwrap_or_default();
    t!("ide-debug-evaluate-hint", enter = shown("⏎"), watch = shown("⇧⌘⏎"))
}

const SHOWN_CHARS: usize = 1000;

pub(super) fn shown_value(value: &str) -> std::borrow::Cow<'_, str> {
    match value.char_indices().nth(SHOWN_CHARS) {
        Some((end, _)) => std::borrow::Cow::Owned(format!("{}…", &value[..end])),
        None => std::borrow::Cow::Borrowed(value),
    }
}

pub(super) fn row_job(row: &VarRow, p: &IdePalette) -> LayoutJob {
    let format = |color| TextFormat { font_id: theme::regular(13.0), color, ..TextFormat::default() };
    let mut job = LayoutJob::default();
    if row.more.is_some() {
        job.append(&row.name, 0.0, format(p.link));
        return job;
    }
    if let Some(entry) = &row.entry {
        job.append(&shown_value(plain_value(&entry.key).1), 0.0, format(p.text));
        job.append(" → ", 0.0, format(p.text_secondary));
        job.append(&shown_value(plain_value(&entry.value).1), 0.0, format(p.text));
        return job;
    }
    job.append(&row.name, 0.0, format(p.text));
    if row.value.is_empty() && row.type_name.is_none() {
        return job;
    }
    job.append(" = ", 0.0, format(p.text_secondary));
    let (object, value) = if row.error { (None, row.value.as_str()) } else { plain_value(&row.value) };
    let type_name = row.type_name.as_ref().filter(|type_name| !type_name.is_empty() && !row.error);
    if let Some(prefix) = object.or(type_name.map(String::as_str)) {
        job.append(&format!("{{{prefix}}} "), 0.0, format(p.text_secondary));
    }
    job.append(&shown_value(value), 0.0, format(if row.error { error_color(p) } else { p.text }));
    job
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Polled {
    pub opened: bool,
    pub stopped: bool,
    pub live: bool,
    pub busy: bool,
}

#[derive(Debug, Default)]
pub(super) struct DebugWindow {
    tabs: Vec<DebugTab>,
    active: usize,
    content: Content,
    known: Vec<Value>,
    known_run: u64,
    relist_until: f64,
    new_watch: String,
    buttons: Vec<(&'static str, Rect)>,
    content_tabs: Vec<(Content, Rect)>,
    session_tabs: Vec<(Rect, Rect)>,
    watch_buttons: Option<(Rect, Rect)>,
    muted: bool,
}

impl DebugWindow {
    pub(super) fn poll(&mut self, registry: &Registry, session: &mut Session, now: f64) -> Polled {
        let mut polled = Polled::default();
        self.muted =
            session.state::<zen_workbench::debug::Breakpoints>().is_some_and(|breakpoints| breakpoints.is_muted());
        let last_run = session.state::<Runs>().and_then(|runs| runs.runs().last()).map_or(0, |run| run.id);
        if last_run != self.known_run {
            self.known_run = last_run;
            self.relist_until = now + 2.0;
        }
        if now <= self.relist_until {
            polled.live = true;
            polled.opened = self.adopt_new_sessions(registry, session);
        }
        let mut stopped = None;
        let all_raw = zen_workbench::debug::views::shows_raw(session);
        let (active, content) = (self.active, self.content);
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if tab.all_raw != all_raw {
                tab.all_raw = all_raw;
                tab.raw_rows.clear();
                if tab.state == SessionState::Stopped {
                    tab.load_missing(registry, session);
                }
            }
            if tab.is_live() && tab.read_state(registry, session) {
                stopped = Some(index);
                let target = tab.frame.and_then(|id| tab.frames.iter().find(|frame| frame.id == id)).cloned();
                if let Some(frame) = target {
                    go_to_frame(registry, session, &frame);
                }
            }
            tab.refresh_console(registry, session);
            tab.poll_setting(registry, session);
            polled.busy |= tab.setting.is_some();
            if tab.state == SessionState::Stopped {
                tab.load_missing(registry, session);
            }
            polled.busy |= !tab.asking.is_empty();
            if let Some(result) = tab.result.as_mut() {
                result.poll(registry, session);
                polled.busy |= result.is_pending();
            }
            if index == active && content == Content::Coroutines && tab.jvm {
                let stop = tab.coroutines_stop();
                polled.busy |= tab.coroutines.poll(registry, session, &tab.session, stop);
            }
            polled.live |= tab.is_live();
            polled.busy |= matches!(tab.state, SessionState::Starting | SessionState::Running);
        }
        if let Some(index) = stopped {
            polled.stopped = true;
            self.active = index;
        }
        polled
    }

    fn adopt_new_sessions(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let Ok(value) = call(registry, session, "debug.session.list", json!({})) else { return false };
        let Ok(list) = serde_json::from_value::<SessionList>(value) else { return false };
        let mut adopted = false;
        for info in list.sessions {
            if info.session_id.is_null() || self.known.contains(&info.session_id) {
                continue;
            }
            self.known.push(info.session_id.clone());
            if info.state == SessionState::Ended {
                continue;
            }
            self.tabs.push(DebugTab::new(&info, session));
            self.active = self.tabs.len() - 1;
            adopted = true;
        }
        adopted
    }

    pub(super) fn any_live(&self) -> bool {
        self.tabs.iter().any(DebugTab::is_live)
    }

    pub(super) fn perform(
        &mut self,
        action: &DebugAction,
        registry: &Registry,
        session: &mut Session,
    ) -> Result<(), String> {
        let mut last = Value::Null;
        for (command, params) in action.commands() {
            last = call(registry, session, command, params)?;
        }
        let active = self.active.min(self.tabs.len().saturating_sub(1));
        match action {
            DebugAction::Rerun { session: id, .. } => {
                let started = serde_json::from_value::<Started>(last).unwrap_or_default();
                if !started.session_id.is_null() {
                    self.known.push(started.session_id.clone());
                }
                if let Some(index) = self.index_of(id) {
                    self.tabs[index].restart(&started, session);
                    self.active = index;
                }
            }
            DebugAction::Close { session: id, .. } => {
                if let Some(index) = self.index_of(id) {
                    self.tabs.remove(index);
                    if index < self.active || self.active >= self.tabs.len() {
                        self.active = self.active.saturating_sub(1);
                    }
                }
            }
            DebugAction::SelectThread { thread } => {
                let Some(tab) = self.tabs.get_mut(active) else { return Ok(()) };
                tab.thread = Some(*thread);
                tab.read_frames(registry, session);
                tab.frame = None;
                if let Some(frame) = tab.frames.iter().find(|frame| !frame.is_separator()).cloned() {
                    call(registry, session, "debug.frame.select", json!({ "frameId": frame.id }))?;
                    tab.frame = Some(frame.id);
                    go_to_frame(registry, session, &frame);
                }
                tab.read_variables(registry, session);
                tab.read_watches(registry, session);
            }
            DebugAction::SelectFrame { frame } => {
                let Some(tab) = self.tabs.get_mut(active) else { return Ok(()) };
                tab.frame = Some(*frame);
                if let Some(target) = tab.frames.iter().find(|candidate| candidate.id == *frame).cloned() {
                    go_to_frame(registry, session, &target);
                }
                tab.read_variables(registry, session);
                tab.read_watches(registry, session);
            }
            DebugAction::Expand { reference, raw, total } => {
                if let Some(tab) = self.tabs.get_mut(active) {
                    tab.ask_page((*reference, *raw), 0, *total);
                    tab.load_missing(registry, session);
                }
            }
            DebugAction::More { reference, raw, start, total } => {
                if let Some(tab) = self.tabs.get_mut(active) {
                    tab.ask_page((*reference, *raw), *start, *total);
                    tab.load_missing(registry, session);
                }
            }
            DebugAction::RawRow { key } => {
                if let Some(tab) = self.tabs.get_mut(active) {
                    if !tab.raw_rows.remove(key) {
                        tab.raw_rows.insert(key.clone());
                    }
                    tab.load_missing(registry, session);
                }
            }
            DebugAction::RawAll { raw } => {
                for tab in &mut self.tabs {
                    tab.all_raw = *raw;
                    tab.raw_rows.clear();
                    if tab.state == SessionState::Stopped {
                        tab.load_missing(registry, session);
                    }
                }
            }
            DebugAction::AddWatch { .. } | DebugAction::RemoveWatch { .. } | DebugAction::EditWatch { .. } => {
                if let DebugAction::EditWatch { old, new } = action
                    && let Some(tab) = self.tabs.get_mut(active)
                    && tab.selected.as_ref().is_some_and(|key| key.len() == 2 && key[0] == "watch" && key[1] == *old)
                {
                    tab.selected = Some(vec!["watch".to_owned(), new.clone()]);
                }
                if let Some(tab) = self.tabs.get_mut(active) {
                    tab.read_watches(registry, session);
                }
            }
            DebugAction::SetValue { key, reference, name, evaluate_name, value } => {
                if let Some(tab) = self.tabs.get_mut(active) {
                    let mut params = json!({
                        "sessionId": tab.session,
                        "reference": reference,
                        "name": name,
                        "value": value,
                        "waitMs": 0,
                    });
                    if let Some(evaluate_name) = evaluate_name {
                        params["evaluateName"] = json!(evaluate_name);
                    }
                    tab.value_error = None;
                    tab.setting = Some((params, key.clone()));
                    tab.poll_setting(registry, session);
                }
            }
            DebugAction::Evaluate { expression } => {
                if let Some(tab) = self.tabs.get_mut(active) {
                    let mut result = Evaluation::new(expression, "repl");
                    result.poll(registry, session);
                    tab.result = Some(result);
                }
            }
            DebugAction::Open { line, column, .. } => {
                let column = column.map(|byte| char_column(session, *line, byte));
                call(registry, session, "editor.caret.goTo", json!({ "line": line, "column": column }))?;
            }
            DebugAction::Hide
            | DebugAction::Stop { .. }
            | DebugAction::ResetFrame { .. }
            | DebugAction::Execute { .. }
            | DebugAction::RunToCursor
            | DebugAction::Mute { .. } => {}
        }
        Ok(())
    }

    fn index_of(&self, session: &Value) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.session == *session)
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Vec<DebugAction> {
        let mut actions = Vec::new();
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.header(ui, p, &mut actions);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            self.buttons.clear();
            self.content_tabs.clear();
            self.watch_buttons = None;
            message(ui, p, t!("ide-debug-empty"));
            return actions;
        };
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), BAR), Sense::hover());
        let mut x = bar.left() + 8.0;
        self.content_tabs.clear();
        let mut contents =
            vec![(Content::Variables, "ide-debug-threads-variables"), (Content::Console, "ide-debug-console")];
        if tab.jvm {
            contents.push((Content::Coroutines, "ide-debug-coroutines"));
        } else if self.content == Content::Coroutines {
            self.content = Content::Variables;
        }
        for (content, key) in contents {
            let galley = ui.painter().layout_no_wrap(t!(key), theme::regular(13.0), p.text);
            let rect = Rect::from_min_size(pos2(x, bar.center().y - 12.0), vec2(galley.size().x + 20.0, 24.0));
            let response = ui.interact(rect, ui.scope_id().with(("debug-content", key)), Sense::click());
            accessibility::item(&response, egui::Role::Tab, self.content == content, galley.text());
            if self.content == content {
                ui.painter().rect_filled(rect, 6, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 6, p.hover);
            }
            ui.painter().galley(rect.center() - galley.size() / 2.0, galley, p.text);
            if response.clicked() {
                self.content = content;
            }
            self.content_tabs.push((content, rect));
            x = rect.right() + 4.0;
        }
        ui.painter().vline(x + 4.0, bar.shrink2(vec2(0.0, 9.0)).y_range(), Stroke::new(1.0, p.tab_border));
        self.buttons = tab.toolbar(ui, p, (x + 12.0, self.muted), bar, &mut actions);
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
        let body = ui.available_rect_before_wrap();
        match self.content {
            Content::Variables => {
                let width = (body.width() * 0.35).clamp(200.0, 420.0);
                let frames = Rect::from_min_max(body.min, pos2(body.left() + width, body.bottom()));
                ui.painter().vline(frames.right() + 0.5, body.y_range(), Stroke::new(1.0, p.tab_border));
                ui.scope_builder(UiBuilder::new().max_rect(frames), |ui| tab.frames_pane(ui, p, &mut actions));
                let variables = Rect::from_min_max(pos2(frames.right() + 1.0, body.top()), body.max);
                let new_watch = &mut self.new_watch;
                let buttons = ui.scope_builder(UiBuilder::new().max_rect(variables), |ui| {
                    tab.variables_pane(ui, p, new_watch, &mut actions)
                });
                self.watch_buttons = Some(buttons.inner);
            }
            Content::Console => {
                self.watch_buttons = None;
                ui.scope_builder(UiBuilder::new().max_rect(body), |ui| tab.console_pane(ui, p, &mut actions));
            }
            Content::Coroutines => {
                self.watch_buttons = None;
                let stopped = tab.state == SessionState::Stopped;
                let salt = tab.session.to_string();
                ui.scope_builder(UiBuilder::new().max_rect(body), |ui| tab.coroutines.show(ui, p, stopped, &salt));
            }
        }
        ui.allocate_rect(body, Sense::hover());
        actions
    }

    fn header(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<DebugAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-tool-debug"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("debug-title"), title, || t!("ide-tool-debug"));
        let mut x = title.right() + 14.0;
        self.session_tabs.clear();
        for (index, tab) in self.tabs.iter().enumerate() {
            let galley = ui.painter().layout_no_wrap(tab.name.clone(), theme::regular(13.0), p.text);
            let rect = Rect::from_min_size(pos2(x, header.center().y - 13.0), vec2(galley.size().x + 58.0, 26.0));
            let response = ui.interact(rect, ui.scope_id().with(("debug-tab", index)), Sense::click());
            let close = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(16.0));
            let close_response = ui.interact(close, ui.scope_id().with(("debug-tab-close", index)), Sense::click());
            let active = self.active == index;
            accessibility::item(&response, egui::Role::Tab, active, &tab.name);
            accessibility::button_with(&close_response, || t!("a11y-close-tab", name = tab.name.as_str()));
            if active {
                ui.painter().rect_filled(rect, 6, p.selection);
            } else if response.hovered() || close_response.hovered() {
                ui.painter().rect_filled(rect, 6, p.hover);
            }
            let (glyph, color) = tab.look(p);
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
                actions.push(DebugAction::Close { session: tab.session.clone(), live: tab.is_live() });
            } else if response.clicked() {
                self.active = index;
            }
            self.session_tabs.push((rect, close));
            x = rect.right() + 6.0;
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("debug-hide"), Sense::click());
        hover(ui, &response, p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-debug-hide"), None, Side::Below);
        if response.clicked() {
            actions.push(DebugAction::Hide);
        }
    }
}

pub(super) fn offset_of(session: &Session, path: &Path, line: u32, column: u32) -> usize {
    let open = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| document.text().to_owned());
    let text = open.or_else(|| std::fs::read_to_string(path).ok()).unwrap_or_default();
    let mut offset = 0;
    for (index, text) in text.split('\n').enumerate() {
        let length = text.chars().count();
        if index == line as usize {
            return offset + (column as usize).min(length);
        }
        offset += length + 1;
    }
    offset.saturating_sub(1)
}

fn go_to_frame(registry: &Registry, session: &mut Session, frame: &FrameInfo) {
    let Some(path) = frame.path.as_ref().filter(|path| path.is_file()) else { return };
    let offset = offset_of(session, path, frame.line, frame.column);
    let _ = registry.dispatch(session, "navigate.place.goTo", json!({ "path": path, "offset": offset }));
}

impl IdeView {
    pub(super) fn follow_debug(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session, now: f64) {
        zen_workbench::debug::sync_soon(session);
        let polled = self.debug_window.poll(registry, session, now);
        if polled.opened || polled.stopped {
            self.show_debug_window();
        }
        if polled.busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if polled.live {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    pub(super) fn show_debug_window(&mut self) {
        self.show_tool("debug");
    }

    pub(super) fn debug_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        for action in self.debug_window.show(ui, &palette) {
            if action == DebugAction::Hide {
                self.hide_tool("debug");
                continue;
            }
            if matches!(action, DebugAction::Mute { .. }) {
                self.editor_view.forget_breakpoints();
            }
            match self.debug_window.perform(&action, registry, session) {
                Ok(()) if matches!(action, DebugAction::Open { .. }) => self.editor_view.focus(),
                Ok(()) => {}
                Err(message) => self.post(ui, message),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use egui::{Event, PointerButton, Pos2};
    use zen_dap::fake::{FakeAdapter, FakeVariable};
    use zen_dap::{Source, StackFrame, Thread};
    use zen_run::RunEnv;
    use zen_run::fake::FakeProvider;
    use zen_workbench::debug::{Debugger, FakeLauncher};

    use super::super::evaluate::Evaluations;
    use super::super::smart_step::SmartStep;
    use super::super::stream_trace::StreamTraces;
    use super::*;

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        adapter: FakeAdapter,
        _dir: tempfile::TempDir,
        main: PathBuf,
        window: DebugWindow,
        actions: Vec<DebugAction>,
        polled: Polled,
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
            session.set_state(crate::ide::trusting_everything());
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            session.state_mut::<Runs>().set_providers(vec![Arc::new(FakeProvider::new())], RunEnv::default());
            let adapter = FakeAdapter::new();
            session.state_mut::<Debugger>().set_launcher(Arc::new(FakeLauncher::new(adapter.clone())));
            let main = dir.path().join("main.go");
            let text: String = (1..=12).map(|line| format!("line {line}\n")).collect();
            std::fs::write(&main, text).unwrap();
            Self {
                ctx,
                registry,
                session,
                adapter,
                _dir: dir,
                main,
                window: DebugWindow::default(),
                actions: Vec::new(),
                polled: Polled::default(),
                time: 0.0,
            }
        }

        fn dispatch(&mut self, id: &str, params: Value) -> Value {
            self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
        }

        fn program(&self, lines: &[i64]) {
            let lines: Vec<(&Path, i64)> = lines.iter().map(|line| (self.main.as_path(), *line)).collect();
            self.adapter.set_program(&lines);
        }

        fn breakpoint(&mut self, line: u32) {
            let main = self.main.clone();
            self.dispatch("debug.breakpoint.set", json!({ "path": main, "line": line }));
        }

        fn start(&mut self, name: &str) -> u64 {
            let params = json!({ "configuration": FakeProvider::configuration(name) });
            let started = self.dispatch("debug.session.start", params);
            started["sessionId"].as_u64().unwrap()
        }

        fn wait(&mut self, id: u64, state: &str) -> Value {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                let answer = self.dispatch("debug.session.get", json!({ "sessionId": id }));
                if answer["state"] == state {
                    return answer;
                }
                assert!(Instant::now() < deadline, "session {id} is not {state}: {answer}");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn requests(&self, command: &str) -> Vec<Value> {
            self.adapter.requests(command)
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.1;
            self.polled = self.window.poll(&self.registry, &mut self.session, self.time);
            self.settle_children();
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 500.0))),
                ..Default::default()
            };
            let mut actions = Vec::new();
            let mut output = self.ctx.run_ui(input, |ui| actions = self.window.show(ui, &IdePalette::zen_dark()));
            output.textures_delta.clear();
            for action in &actions {
                if *action != DebugAction::Hide {
                    let _ = self.window.perform(action, &self.registry, &mut self.session);
                }
            }
            self.settle_children();
            self.actions.extend(actions);
        }

        fn settle_children(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.window.tabs.iter().any(|tab| !tab.asking.is_empty()) {
                assert!(Instant::now() < deadline, "the variables stay pending");
                for tab in &mut self.window.tabs {
                    tab.load_missing(&self.registry, &mut self.session);
                }
                std::thread::yield_now();
            }
        }

        fn click(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.time += 1.0;
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }

        fn tab(&self) -> &DebugTab {
            &self.window.tabs[self.window.active]
        }

        fn button(&self, salt: &str) -> Pos2 {
            self.window.buttons.iter().find(|(drawn, _)| *drawn == salt).map(|(_, rect)| rect.center()).unwrap()
        }

        fn row(&self, key: &[&str]) -> Rect {
            let key: RowKey = key.iter().map(|part| (*part).to_owned()).collect();
            self.tab().var_rows.iter().find(|(drawn, _)| *drawn == key).map(|(_, rect)| *rect).unwrap()
        }

        fn keys(&self) -> Vec<RowKey> {
            self.tab().var_rows.iter().map(|(key, _)| key.clone()).collect()
        }

        fn stopped(&mut self, id: u64) {
            self.wait(id, "stopped");
            self.frame(Vec::new());
            assert!(self.polled.stopped, "a new stop is noticed");
            self.frame(Vec::new());
            assert!(!self.polled.stopped, "and only once");
        }

        fn set_value_answered(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                self.frame(Vec::new());
                if self.tab().setting.is_none() {
                    return;
                }
                assert!(Instant::now() < deadline, "Set Value stays pending");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn watches(&self) -> Vec<String> {
            self.session.state::<Debugger>().map(|debugger| debugger.watches().to_vec()).unwrap_or_default()
        }
    }

    fn caret(session: &Session) -> (PathBuf, usize) {
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        (document.path().to_owned(), document.selections().primary().head)
    }

    fn stack_frame(id: i64, name: &str, path: &Path, line: i64, column: i64) -> StackFrame {
        let source = Some(Source::path(&path.to_string_lossy()));
        StackFrame { id, name: name.to_owned(), source, line, column, ..StackFrame::default() }
    }

    #[test]
    fn a_session_gets_a_tab_and_closing_a_live_one_stops_it() {
        let mut debug = Harness::new();
        debug.frame(Vec::new());
        assert!(debug.window.tabs.is_empty(), "nothing is being debugged");
        assert!(!debug.polled.opened && !debug.polled.live);
        debug.program(&[3, 5]);
        debug.breakpoint(2);
        let first = debug.start("one");
        debug.wait(first, "stopped");
        debug.frame(Vec::new());
        assert!(debug.polled.opened, "a new session opens the window");
        assert_eq!(debug.window.tabs.len(), 1);
        assert_eq!(debug.tab().name, "one");
        assert_eq!(debug.tab().state, SessionState::Stopped);
        assert!(debug.window.any_live());
        let second = debug.start("two");
        debug.wait(second, "stopped");
        debug.frame(Vec::new());
        assert_eq!(debug.window.tabs.len(), 2);
        assert_eq!(debug.window.active, 1, "the new session's tab is shown");
        debug.frame(Vec::new());
        assert!(!debug.polled.opened, "a known session opens nothing again");

        let close = debug.window.session_tabs[0].1.center();
        debug.click(close);
        assert!(debug.actions.contains(&DebugAction::Close { session: json!(first), live: true }));
        assert_eq!(debug.wait(first, "ended")["reason"], "stopped");
        assert_eq!(debug.window.tabs.len(), 1);
        assert_eq!(debug.tab().session, json!(second));
        debug.time += 5.0;
        debug.frame(Vec::new());
        assert_eq!(debug.window.tabs.len(), 1, "a closed session does not come back");

        debug.dispatch("debug.execution.resume", json!({ "sessionId": second }));
        debug.wait(second, "ended");
        debug.frame(Vec::new());
        assert_eq!(debug.tab().state, SessionState::Ended);
        assert!(!debug.window.any_live());
        debug.frame(Vec::new());
        assert!(!debug.polled.live);
    }

    #[test]
    fn the_toolbar_is_enabled_by_state_and_dispatches_the_commands() {
        let mut debug = Harness::new();
        debug.adapter.set_threads(vec![Thread { id: 3, name: "main".to_owned() }]);
        debug.program(&[2, 3, 4, 5, 6, 7]);
        debug.adapter.set_hold(true);
        let main = debug.main.clone();
        debug.dispatch("file.document.open", json!({ "path": main }));
        let id = debug.start("app");
        debug.wait(id, "running");
        debug.frame(Vec::new());
        assert_eq!(debug.tab().state, SessionState::Running);
        debug.click(debug.button("debug-resume"));
        debug.click(debug.button("debug-step-over"));
        assert!(debug.requests("continue").is_empty(), "Resume is off while running");
        assert!(debug.requests("next").is_empty(), "stepping is off while running");
        debug.click(debug.button("debug-pause"));
        assert_eq!(debug.requests("pause"), [json!({ "threadId": 3 })]);
        debug.wait(id, "stopped");
        debug.frame(Vec::new());
        assert_eq!(debug.tab().state, SessionState::Stopped);
        debug.click(debug.button("debug-pause"));
        assert_eq!(debug.requests("pause").len(), 1, "Pause is off while stopped");
        debug.adapter.set_hold(false);
        for (salt, command) in
            [("debug-step-over", "next"), ("debug-step-into", "stepIn"), ("debug-step-out", "stepOut")]
        {
            debug.click(debug.button(salt));
            assert_eq!(debug.requests(command).last().unwrap()["threadId"], 3, "{salt}");
            debug.wait(id, "stopped");
            debug.frame(Vec::new());
        }
        debug.dispatch("editor.caret.goTo", json!({ "line": 6, "column": 1 }));
        debug.click(debug.button("debug-run-to-cursor"));
        assert_eq!(debug.requests("continue").len(), 1);
        assert_eq!(debug.wait(id, "stopped")["frame"]["line"], 5, "Run to Cursor stops at the caret's line");
        debug.frame(Vec::new());
        debug.click(debug.button("debug-resume"));
        assert_eq!(debug.requests("continue").last().unwrap()["threadId"], 3);
        debug.wait(id, "ended");
        debug.frame(Vec::new());
        assert_eq!(debug.tab().state, SessionState::Ended);

        let disconnects = debug.requests("disconnect").len();
        debug.click(debug.button("debug-stop"));
        assert_eq!(debug.requests("disconnect").len(), disconnects, "Stop is off once ended");
        debug.adapter.set_hold(true);
        debug.click(debug.button("debug-rerun"));
        assert_eq!(debug.window.tabs.len(), 1, "Rerun debugs again in the same tab");
        assert_eq!(debug.tab().session, json!(id + 1));
        debug.wait(id + 1, "running");
        debug.frame(Vec::new());
        assert_eq!(debug.tab().state, SessionState::Running);
        assert_eq!(debug.window.tabs.len(), 1);
        debug.click(debug.button("debug-stop"));
        assert_eq!(debug.wait(id + 1, "ended")["reason"], "stopped");
        debug.frame(Vec::new());
        assert_eq!(debug.tab().state, SessionState::Ended);

        let configuration = Box::new(FakeProvider::configuration("app"));
        let rerun = DebugAction::Rerun { session: json!(2), live: true, configuration };
        let ids: Vec<&str> = rerun.commands().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["debug.session.stop", "debug.session.start"], "a live session is stopped first");
        assert!(DebugAction::Close { session: json!(2), live: false }.commands().is_empty());
    }

    #[test]
    fn a_stop_and_a_selected_frame_go_to_the_frame_in_the_editor() {
        let mut debug = Harness::new();
        let file = debug.main.clone();
        std::fs::write(&file, "package main\n\nfunc helper() {\n\tprintln()\n}\n\nfunc main() {\n\thelper()\n}\n")
            .unwrap();
        debug
            .adapter
            .set_threads(vec![Thread { id: 1, name: "main".to_owned() }, Thread { id: 7, name: "worker".to_owned() }]);
        debug.adapter.set_frames(vec![vec![
            stack_frame(11, "main.helper", &file, 4, 2),
            stack_frame(12, "main.main", &file, 8, 2),
        ]]);
        debug.program(&[4]);
        debug.breakpoint(3);
        let id = debug.start("app");
        debug.stopped(id);
        assert_eq!(debug.tab().frame, Some(11), "the top frame is selected");
        assert_eq!(caret(&debug.session), (file.clone(), offset_of(&debug.session, &file, 3, 1)), "the stop is shown");
        assert_eq!(debug.requests("scopes").last().unwrap()["frameId"], 11);

        let row = debug.tab().frame_rows.iter().find(|(id, _)| *id == 12).map(|(_, rect)| *rect).unwrap();
        debug.click(row.center());
        assert!(debug.actions.contains(&DebugAction::SelectFrame { frame: 12 }));
        assert_eq!(debug.dispatch("debug.session.get", json!({}))["frame"]["name"], "main.main");
        let expected = "package main\n\nfunc helper() {\n\tprintln()\n}\n\nfunc main() {\n\t".chars().count();
        assert_eq!(caret(&debug.session), (file, expected));
        assert_eq!(debug.tab().frame, Some(12));
        assert_eq!(debug.requests("scopes").last().unwrap()["frameId"], 12);

        debug.window.perform(&DebugAction::SelectThread { thread: 7 }, &debug.registry, &mut debug.session).unwrap();
        assert_eq!(debug.requests("stackTrace").last().unwrap()["threadId"], 7);
        assert_eq!(debug.tab().thread, Some(7));
    }

    #[test]
    fn variables_expand_lazily_and_load_more_by_pages() {
        let mut debug = Harness::new();
        let items: Vec<FakeVariable> =
            (0..250).map(|index| FakeVariable::new(&format!("[{index}]"), &index.to_string(), "int")).collect();
        debug.adapter.set_variables(vec![
            FakeVariable::new("n", "3", "int"),
            FakeVariable::new("items", "[]int len: 250", "[]int").with_children(items),
        ]);
        debug.program(&[2, 3]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        let locals = || vec!["scope".to_owned(), "Locals".to_owned()];
        assert_eq!(
            debug.keys(),
            [locals(), [locals(), vec!["n".to_owned()]].concat(), [locals(), vec!["items".to_owned()]].concat()],
            "Locals open at once"
        );
        assert_eq!(debug.requests("variables").last().unwrap()["variablesReference"], 1);
        let reads = debug.requests("variables").len();
        debug.frame(Vec::new());
        assert_eq!(debug.requests("variables").len(), reads, "nothing is read again without a change");

        let page = |start: usize, count: usize| json!({ "variablesReference": 2, "start": start, "count": count });
        let row = debug.row(&["scope", "Locals", "items"]);
        debug.click(pos2(row.left() + 8.0 + metrics::TREE_INDENT + 6.0, row.center().y));
        assert_eq!(debug.requests("variables").last(), Some(&page(0, PAGE)));
        debug.frame(Vec::new());
        assert_eq!(debug.keys().len(), 3 + PAGE + 1, "a page of children and …more");
        let more = debug.keys().last().cloned().unwrap();
        assert_eq!(more.last().map(String::as_str), Some(MORE));
        let load_more = |debug: &mut Harness| {
            let (rows, _) = debug.tab().rows();
            let (reference, start, total) = rows.last().and_then(|row| row.more).unwrap();
            let action = DebugAction::More { reference, raw: false, start, total };
            debug.window.perform(&action, &debug.registry, &mut debug.session).unwrap();
            debug.settle_children();
        };
        load_more(&mut debug);
        assert_eq!(debug.requests("variables").last(), Some(&page(100, PAGE)));
        debug.frame(Vec::new());
        load_more(&mut debug);
        assert_eq!(debug.requests("variables").last(), Some(&page(200, 50)));
        debug.frame(Vec::new());
        assert_eq!(debug.keys().len(), 3 + 250, "every child, and no …more");
        let reads = debug.requests("variables").len();
        let row = debug.row(&["scope", "Locals", "items"]);
        let chevron = pos2(row.left() + 8.0 + metrics::TREE_INDENT + 6.0, row.center().y);
        debug.click(chevron);
        debug.frame(Vec::new());
        assert_eq!(debug.keys().len(), 3, "collapsed");
        debug.click(chevron);
        debug.frame(Vec::new());
        assert_eq!(debug.keys().len(), 253, "expanded again from what was read");
        assert_eq!(debug.requests("variables").len(), reads);

        debug.dispatch("debug.execution.stepOver", json!({}));
        debug.wait(id, "stopped");
        debug.frame(Vec::new());
        debug.frame(Vec::new());
        assert_eq!(debug.requests("variables").last(), Some(&page(0, PAGE)), "what was open opens again after a step");
    }

    #[test]
    fn watches_are_added_edited_and_removed() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(vec![FakeVariable::new("n", "3", "int"), FakeVariable::new("m", "4", "int")]);
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        debug.window.new_watch = "n".to_owned();
        let (add, remove) = debug.window.watch_buttons.unwrap();
        debug.click(add.center());
        assert_eq!(debug.watches(), ["n"]);
        assert!(debug.window.new_watch.is_empty());
        debug.window.new_watch = "bad".to_owned();
        debug.click(add.center());
        debug.frame(Vec::new());
        let (rows, _) = debug.tab().rows();
        let shown: Vec<(&str, &str, bool)> =
            rows.iter().map(|row| (row.name.as_str(), row.value.as_str(), row.error)).collect();
        assert_eq!(shown[0], ("n", "3", false), "watches first");
        assert_eq!((shown[1].0, shown[1].2), ("bad", true));
        assert!(!shown[1].1.is_empty(), "the error is shown as the value: {shown:?}");
        assert_eq!(shown[2], ("Locals", "", false));
        let p = IdePalette::zen_dark();
        let job = row_job(&rows[1], &p);
        assert_eq!(job.sections.last().map(|section| section.format.color), Some(error_color(&p)));
        assert_eq!(row_job(&rows[0], &p).text, "n = {int} 3");
        let huge = VarRow {
            key: Vec::new(),
            depth: 0,
            name: "n".to_owned(),
            value: "é".repeat(2_000_000),
            type_name: Some("int".to_owned()),
            error: false,
            reference: 0,
            total: None,
            more: None,
            watch: false,
            parent: 0,
            evaluate_name: None,
            entry: None,
            raw: Default::default(),
        };
        let text = row_job(&huge, &p).text;
        assert_eq!(text.chars().count(), "n = {int} ".chars().count() + SHOWN_CHARS + 1, "a huge value is cut");
        assert!(text.ends_with("é…"));

        let edit = DebugAction::EditWatch { old: "bad".to_owned(), new: "m".to_owned() };
        debug.window.perform(&edit, &debug.registry, &mut debug.session).unwrap();
        assert_eq!(debug.watches(), ["n", "m"]);

        debug.frame(Vec::new());
        debug.click(debug.row(&["watch", "n"]).center());
        debug.click(remove.center());
        assert_eq!(debug.watches(), ["m"]);
        debug.frame(Vec::new());
        debug.click(debug.row(&["watch", "m"]).center());
        debug.key(Key::Delete);
        assert!(debug.watches().is_empty());
        debug.frame(Vec::new());
        assert_eq!(debug.keys()[0], vec!["scope".to_owned(), "Locals".to_owned()]);
    }

    fn press(key: Key, modifiers: Modifiers) -> [Event; 2] {
        let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        [event(true), event(false)]
    }

    fn structs() -> Vec<FakeVariable> {
        vec![
            FakeVariable::new("x", "1", "int"),
            FakeVariable::new("s", "{a: 2}", "main.S").with_children(vec![FakeVariable::new("a", "2", "int")]),
        ]
    }

    impl Harness {
        fn evaluate(&mut self, evaluations: &mut Evaluations, events: Vec<Event>) -> Vec<String> {
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                ..Default::default()
            };
            let (registry, session) = (&self.registry, &mut self.session);
            let mut watches = Vec::new();
            let mut output =
                self.ctx.run_ui(input, |ui| watches = evaluations.show(ui, registry, session, &IdePalette::zen_dark()));
            output.textures_delta.clear();
            watches
        }

        fn settle(&mut self, evaluations: &mut Evaluations) {
            let deadline = Instant::now() + Duration::from_secs(10);
            self.evaluate(evaluations, Vec::new());
            while evaluations.pending() {
                assert!(Instant::now() < deadline, "the evaluation does not end");
                self.evaluate(evaluations, Vec::new());
            }
            self.evaluate(evaluations, Vec::new());
        }

        fn hover(&mut self, evaluations: &mut Evaluations, wanted: Option<(&str, Rect)>, delay: f64) {
            self.time += 0.2;
            let input = egui::RawInput { time: Some(self.time), ..Default::default() };
            let wanted = wanted.map(|(expression, rect)| (expression.to_owned(), rect));
            let mut output = self.ctx.run_ui(input, |ui| evaluations.hover(ui, wanted.clone(), (true, delay)));
            output.textures_delta.clear();
        }
    }

    #[test]
    fn enter_in_the_variables_field_evaluates_and_shift_command_enter_adds_a_watch() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(structs());
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        let (add, _) = debug.window.watch_buttons.unwrap();
        debug.click(pos2(add.left() - 30.0, add.center().y));
        debug.frame(vec![Event::Text("s".to_owned())]);
        debug.frame(press(Key::Enter, Modifiers::NONE).into());
        assert!(debug.watches().is_empty(), "Enter evaluates, it adds no watch");
        assert_eq!(debug.window.new_watch, "s", "the expression stays in the field");
        let deadline = Instant::now() + Duration::from_secs(10);
        while debug.tab().result.as_ref().is_none_or(Evaluation::is_pending) {
            assert!(Instant::now() < deadline, "the evaluation does not end");
            debug.frame(Vec::new());
        }
        let shown = debug.tab().result.as_ref().unwrap().shown();
        assert_eq!(shown, ["s = {main.S} {a: 2}"], "the result under the field, collapsed");
        assert_eq!(debug.requests("evaluate")[0]["context"], "repl");
        assert_eq!(debug.dispatch("debug.expression.history", json!({}))["expressions"], json!(["s"]));

        debug.frame(press(Key::Enter, Modifiers::COMMAND | Modifiers::SHIFT).into());
        assert_eq!(debug.watches(), ["s"]);
        assert!(debug.window.new_watch.is_empty());
        debug.dispatch("debug.execution.resume", json!({}));
        debug.wait(id, "ended");
        debug.frame(Vec::new());
        assert!(debug.tab().result.is_none(), "a value is gone once the program runs on");
    }

    #[test]
    fn the_evaluate_dialog_shows_a_lazy_tree_keeps_the_history_and_adds_a_watch() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(structs());
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        let mut evaluations = Evaluations::default();
        evaluations.open_dialog(&debug.registry, &mut debug.session, " s ", pos2(100.0, 40.0));
        assert_eq!(evaluations.names(), ["evaluateExpression"]);
        debug.settle(&mut evaluations);
        let (text, shown) = evaluations.dialog_shown(0).unwrap();
        assert_eq!(text, "s", "the selection or the name at the caret is the expression");
        assert_eq!(shown, ["s = {main.S} {a: 2}", "a = {int} 2"], "the value is opened and its fields read lazily");
        let watch = evaluations.drawn(0, "watch").center();
        debug.evaluate(&mut evaluations, vec![Event::PointerMoved(watch)]);
        let click = |pressed| Event::PointerButton {
            pos: watch,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        debug.evaluate(&mut evaluations, vec![click(true)]);
        assert_eq!(debug.evaluate(&mut evaluations, vec![click(false)]), ["s"], "Add to Watches");

        let field = evaluations.drawn(0, "field").center();
        let click = |pressed| Event::PointerButton {
            pos: field,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        debug.evaluate(&mut evaluations, vec![Event::PointerMoved(field), click(true)]);
        debug.evaluate(&mut evaluations, vec![click(false)]);
        let select_all = press(Key::A, Modifiers::COMMAND);
        debug.evaluate(&mut evaluations, select_all.into());
        debug.evaluate(&mut evaluations, vec![Event::Text("x".to_owned())]);
        debug.evaluate(&mut evaluations, press(Key::Enter, Modifiers::NONE).into());
        debug.settle(&mut evaluations);
        assert_eq!(evaluations.dialog_shown(0).unwrap().1, ["x = {int} 1"]);
        let history = debug.dispatch("debug.expression.history", json!({}));
        assert_eq!(history["expressions"], json!(["x", "s"]), "the newest first");

        evaluations.open_dialog(&debug.registry, &mut debug.session, "nope", pos2(100.0, 40.0));
        debug.settle(&mut evaluations);
        let (_, shown) = evaluations.dialog_shown(1).unwrap();
        assert_eq!(shown.len(), 1, "the debugger's error is the result: {shown:?}");
        assert_ne!(shown[0], t!("ide-evaluate-pending"));
        assert!(evaluations.dialog_shown(0).is_some(), "dialogs are not modal: several stay open");
        debug.evaluate(&mut evaluations, press(Key::Escape, Modifiers::NONE).into());
        assert!(evaluations.dialog_shown(1).is_none(), "Esc closes the dialog with the keyboard");
        assert!(evaluations.dialog_shown(0).is_some());
    }

    #[test]
    fn evaluation_never_waits_for_the_debugger_in_a_frame() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(structs());
        debug.adapter.set_silent("evaluate");
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        let mut evaluations = Evaluations::default();
        evaluations.open_dialog(&debug.registry, &mut debug.session, "x", pos2(100.0, 40.0));
        evaluations.quick("s", Rect::from_min_size(pos2(300.0, 300.0), vec2(20.0, 16.0)), debug.time);
        debug.evaluate(&mut evaluations, Vec::new());
        debug.evaluate(&mut evaluations, Vec::new());
        assert_eq!(evaluations.dialog_shown(0).unwrap().1, [t!("ide-evaluate-pending")], "no answer yet, no error");
        assert_eq!(evaluations.popup_shown(), Some((false, vec![t!("ide-evaluate-pending")])));
        assert_eq!(debug.adapter.wait_for("evaluate", 2, Duration::from_secs(10)).len(), 2, "asked once each");
    }

    #[test]
    fn opening_a_value_never_waits_for_the_debugger_in_a_frame() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(structs());
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        debug.adapter.set_silent("variables");
        let row = debug.tab().rows().0.into_iter().find(|row| row.name == "s").unwrap();
        let action = DebugAction::Expand { reference: row.reference, raw: false, total: row.total };
        debug.window.expanded_for_test(&row.key);
        debug.window.perform(&action, &debug.registry, &mut debug.session).unwrap();
        let polled = debug.window.poll(&debug.registry, &mut debug.session, 1.0);
        assert!(polled.busy, "the frame goes on and asks again");
        assert_eq!(debug.tab().asking.len(), 1, "asked once, still waiting");
        assert_eq!(debug.tab().rows().0.iter().filter(|row| row.name == "a").count(), 0);
    }

    #[test]
    fn quick_evaluate_shows_a_popup_and_a_resting_pointer_shows_a_value_tooltip() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(structs());
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        let mut evaluations = Evaluations::default();
        let anchor = Rect::from_min_size(pos2(300.0, 300.0), vec2(20.0, 16.0));
        evaluations.quick("s", anchor, debug.time);
        debug.settle(&mut evaluations);
        assert_eq!(evaluations.names(), ["quickEvaluate"]);
        assert_eq!(
            evaluations.popup_shown(),
            Some((false, vec!["s = {main.S} {a: 2}".to_owned(), "a = {int} 2".to_owned()]))
        );
        debug.hover(&mut evaluations, Some(("x", anchor)), 0.5);
        assert_eq!(
            evaluations.popup_shown().map(|(hover, _)| hover),
            Some(false),
            "a quick popup is not replaced by hovering"
        );
        debug.evaluate(&mut evaluations, press(Key::Escape, Modifiers::NONE).into());
        assert!(evaluations.popup_shown().is_none(), "Esc closes it");

        debug.hover(&mut evaluations, Some(("x", anchor)), 0.5);
        assert!(evaluations.popup_shown().is_none(), "not before the delay");
        for _ in 0..3 {
            debug.hover(&mut evaluations, Some(("x", anchor)), 0.5);
        }
        debug.settle(&mut evaluations);
        assert_eq!(evaluations.popup_shown(), Some((true, vec!["x = {int} 1".to_owned()])));
        assert_eq!(evaluations.names(), ["valueTooltip"]);
        assert_eq!(debug.requests("evaluate").last().unwrap()["context"], "hover");
        debug.hover(&mut evaluations, None, 0.5);
        debug.hover(&mut evaluations, None, 0.5);
        assert!(evaluations.popup_shown().is_none(), "it goes once the pointer leaves");
        debug.hover(&mut evaluations, Some(("nope", anchor)), 0.0);
        debug.hover(&mut evaluations, Some(("nope", anchor)), 0.0);
        debug.settle(&mut evaluations);
        assert!(evaluations.popup_shown().is_none(), "what cannot be evaluated shows no tooltip");
        assert_eq!(
            debug.dispatch("debug.expression.history", json!({}))["expressions"],
            json!(["s"]),
            "hovers are not recent expressions"
        );
    }

    #[test]
    fn the_console_tab_shows_the_sessions_run_with_the_shared_console() {
        let mut debug = Harness::new();
        let file = debug.main.clone();
        std::fs::write(&file, "package main\nfunc main() {}\n").unwrap();
        debug.adapter.set_output(&[("stdout", "hello\n"), ("stderr", "main.go:2:1: boom\n")]);
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.wait(id, "stopped");
        debug.frame(Vec::new());
        let tab = debug.window.content_tabs.iter().find(|(content, _)| *content == Content::Console).unwrap().1;
        debug.click(tab.center());
        assert_eq!(debug.window.content, Content::Console);
        let texts: Vec<&str> = debug.tab().console.lines().iter().map(|line| line.text.as_str()).collect();
        assert!(texts.contains(&"hello") && texts.contains(&"main.go:2:1: boom"), "{texts:?}");
        let index = debug.tab().console.lines().iter().position(|line| line.text.starts_with("main.go")).unwrap();
        let rect = debug.tab().console.rows().iter().find(|(drawn, _)| *drawn == index).map(|(_, rect)| *rect).unwrap();
        debug.click(rect.left_center() + vec2(12.0, 0.0));
        assert!(debug.actions.contains(&DebugAction::Open { path: file.clone(), line: 2, column: Some(1) }));
        assert_eq!(caret(&debug.session).0, file);
        assert!(debug.window.watch_buttons.is_none());
    }

    #[test]
    fn a_jvm_session_has_a_coroutines_tab_that_reads_them_once_a_stop() {
        let mut debug = Harness::new();
        let kotlin = debug.main.with_file_name("App.kt");
        debug.adapter.set_frames(vec![vec![stack_frame(1, "AppKt.main", &kotlin, 3, 1)]]);
        let dump = "\"SUSPENDED\u{1e}worker\u{1e}Dispatchers.Default\u{1e}StandaloneCoroutine{Active}@3\u{1e}App.kt:7\u{1f}\n\"";
        debug
            .adapter
            .set_evaluations(vec![(zen_workbench::java::coroutines::DUMP, FakeVariable::new("", dump, "String"))]);
        debug.program(&[3]);
        debug.breakpoint(2);
        let id = debug.start("app");
        debug.stopped(id);
        assert!(
            !debug.window.content_tabs.iter().any(|(content, _)| *content == Content::Coroutines),
            "only a JVM run has coroutines"
        );
        debug.window.tabs[0].jvm = true;
        debug.frame(Vec::new());
        let tab = debug.window.content_tabs.iter().find(|(content, _)| *content == Content::Coroutines).unwrap().1;
        debug.click(tab.center());
        assert_eq!(debug.window.content, Content::Coroutines);
        let deadline = Instant::now() + Duration::from_secs(10);
        while debug.tab().coroutines.answer().is_none() {
            assert!(Instant::now() < deadline, "the coroutines are never read");
            debug.frame(Vec::new());
            std::thread::sleep(Duration::from_millis(5));
        }
        let answer = debug.tab().coroutines.answer().unwrap().clone();
        assert_eq!(answer.coroutines.len(), 1);
        assert_eq!(answer.coroutines[0].name.as_deref(), Some("worker"));
        assert_eq!(answer.coroutines[0].frames, ["App.kt:7"]);
        for _ in 0..5 {
            debug.frame(Vec::new());
        }
        assert_eq!(debug.requests("evaluate").len(), 1, "one dump a stop");
        let content = debug.window.content_tabs.iter().find(|(content, _)| *content == Content::Variables).unwrap().1;
        debug.click(content.center());
        assert_eq!(debug.window.content, Content::Variables);
    }

    #[test]
    fn polling_reads_only_what_changed_and_stops_with_the_sessions() {
        let mut debug = Harness::new();
        for _ in 0..3 {
            debug.frame(Vec::new());
        }
        assert!(!debug.polled.live, "no run, no session to look for");
        debug.adapter.set_variables(vec![FakeVariable::new("n", "3", "int")]);
        debug.program(&[2, 3]);
        debug.breakpoint(1);
        debug.dispatch("debug.watch.add", json!({ "expression": "n" }));
        let id = debug.start("app");
        debug.stopped(id);
        let counts = |debug: &Harness| {
            (debug.requests("stackTrace").len(), debug.requests("scopes").len(), debug.requests("evaluate").len())
        };
        let before = counts(&debug);
        for _ in 0..5 {
            debug.frame(Vec::new());
        }
        assert_eq!(counts(&debug), before, "a session that did not change is not read again");
        assert!(debug.polled.live && !debug.polled.busy);
        debug.dispatch("debug.execution.stepOver", json!({}));
        debug.wait(id, "stopped");
        debug.frame(Vec::new());
        assert_eq!(counts(&debug), (before.0 + 1, before.1 + 1, before.2 + 1), "a step reads it again");
        debug.dispatch("debug.execution.resume", json!({}));
        debug.wait(id, "ended");
        debug.time += 5.0;
        debug.frame(Vec::new());
        debug.frame(Vec::new());
        assert!(!debug.polled.live, "nothing to poll once every session ended");
    }

    #[test]
    fn the_window_is_empty_without_sessions_and_a_plain_run_is_no_session() {
        let mut debug = Harness::new();
        debug.frame(Vec::new());
        assert!(debug.window.buttons.is_empty());
        debug.dispatch("run.process.start", json!({ "configuration": FakeProvider::configuration("plain") }));
        debug.frame(Vec::new());
        assert!(!debug.polled.opened && debug.window.tabs.is_empty(), "a plain run is no debug session");
    }

    #[test]
    fn offsets_count_characters_from_line_and_column() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.go");
        std::fs::write(&file, "ыы\nab\n").unwrap();
        let session = Session::new();
        assert_eq!(offset_of(&session, &file, 1, 1), 4);
        assert_eq!(offset_of(&session, &file, 0, 9), 2, "past the end of the line");
        assert_eq!(offset_of(&session, &file, 9, 0), 6, "past the end of the file");
    }

    #[test]
    fn the_variables_are_a_tree_for_screen_readers() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(vec![FakeVariable::new("n", "3", "int")]);
        debug.program(&[2, 3]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        let tree = accessibility::testing::tree_of(&debug.ctx, vec2(1000.0, 500.0), |ui| {
            let _ = debug.window.show(ui, &IdePalette::zen_dark());
        });
        assert_eq!(accessibility::unnamed(&tree), Vec::<String>::new());
        let variable = accessibility::testing::find(&tree, egui::Role::TreeItem, "n = 3").unwrap();
        assert_eq!(variable.level(), Some(2));
        assert!(accessibility::testing::find(&tree, egui::Role::Tab, &t!("ide-debug-console")).is_some());
    }

    #[test]
    fn f2_sets_a_value_in_place_shows_a_refusal_under_the_row_and_still_edits_a_watch() {
        let mut debug = Harness::new();
        debug.adapter.set_capabilities(json!({ "supportsSetVariable": true }));
        debug.adapter.set_variables(vec![FakeVariable::new("x", "1", "int")]);
        debug.program(&[2, 3]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        assert!(debug.tab().set_value, "the debugger can set values");
        let x = || vec!["scope".to_owned(), "Locals".to_owned(), "x".to_owned()];
        debug.click(debug.row(&["scope", "Locals", "x"]).center());
        debug.key(Key::F2);
        assert_eq!(
            debug.tab().value_edit.as_ref().map(|(key, text, _)| (key.clone(), text.clone())),
            Some((x(), "1".to_owned()))
        );
        debug.frame(Vec::new());
        debug.window.tabs[0].value_edit.as_mut().unwrap().1 = "42".to_owned();
        debug.frame(press(Key::Enter, Modifiers::NONE).to_vec());
        assert!(debug.tab().value_edit.is_none(), "Enter ends the editing");
        let asked = debug.adapter.wait_for("setVariable", 1, Duration::from_secs(10));
        assert_eq!(asked, [json!({ "variablesReference": 1, "name": "x", "value": "42" })]);
        debug.set_value_answered();
        let (rows, _) = debug.tab().rows();
        assert_eq!(rows.iter().find(|row| row.key == x()).map(|row| row.value.clone()), Some("42".to_owned()));

        debug.click(debug.row(&["scope", "Locals", "x"]).center());
        debug.key(Key::F2);
        debug.frame(Vec::new());
        debug.window.tabs[0].value_edit.as_mut().unwrap().1 = "x ?".to_owned();
        debug.frame(press(Key::Enter, Modifiers::NONE).to_vec());
        debug.adapter.wait_for("setVariable", 2, Duration::from_secs(10));
        debug.set_value_answered();
        let error = debug.tab().value_error.clone().expect("the refusal is kept");
        assert_eq!(error.0, x());
        assert!(!error.1.is_empty());
        let (rows, _) = debug.tab().rows();
        let below = debug.row(&["scope", "Locals", "x"]).bottom();
        assert!(
            debug
                .tab()
                .var_rows
                .iter()
                .all(|(_, rect)| rect.top() < below || rect.top() >= below + metrics::TREE_ROW - 0.5)
                && rows.len() == debug.tab().var_rows.len(),
            "the error takes a row of its own"
        );

        debug
            .window
            .perform(&DebugAction::AddWatch { expression: "x".to_owned() }, &debug.registry, &mut debug.session)
            .unwrap();
        debug.frame(Vec::new());
        debug.click(debug.row(&["watch", "x"]).center());
        debug.key(Key::F2);
        assert!(debug.tab().editing.is_some() && debug.tab().value_edit.is_none(), "F2 on a watch edits the watch");
    }

    #[test]
    fn without_set_variable_f2_changes_nothing() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(vec![FakeVariable::new("x", "1", "int")]);
        debug.program(&[2, 3]);
        debug.breakpoint(1);
        let id = debug.start("app");
        debug.stopped(id);
        assert!(!debug.tab().set_value);
        debug.click(debug.row(&["scope", "Locals", "x"]).center());
        debug.key(Key::F2);
        assert!(debug.tab().value_edit.is_none());
    }

    #[test]
    fn reset_frame_is_on_the_toolbar_and_the_frames_menu_and_async_frames_follow_a_separator() {
        let mut debug = Harness::new();
        debug.adapter.set_capabilities(json!({ "supportsRestartFrame": true }));
        let main = debug.main.clone();
        let mut fixed = stack_frame(12, "main.main", &main, 8, 2);
        fixed.can_restart = Some(false);
        let separator = StackFrame {
            id: 13,
            name: "await".to_owned(),
            presentation_hint: Some("label".to_owned()),
            ..StackFrame::default()
        };
        let top = stack_frame(11, "main.helper", &main, 4, 2);
        let stack = vec![top.clone(), fixed, separator, stack_frame(14, "main.caller", &main, 10, 2)];
        debug.adapter.set_frames(vec![stack, vec![top.clone()], vec![top]]);
        debug.program(&[3, 5]);
        debug.breakpoint(2);
        let id = debug.start("app");
        debug.stopped(id);
        let drawn: Vec<u64> = debug.tab().frame_rows.iter().map(|(id, _)| *id).collect();
        assert_eq!(drawn, [11, 12, 14], "the separator is no frame to select");
        assert!(debug.tab().frames[3].is_async && !debug.tab().frames[0].is_async);

        let row = |debug: &Harness, id: u64| debug.tab().frame_rows.iter().find(|(drawn, _)| *drawn == id).unwrap().1;
        debug.click(row(&debug, 12).center());
        debug.frame(Vec::new());
        assert_eq!(debug.tab().frame, Some(12));
        debug.click(debug.button("debug-reset-frame"));
        assert!(debug.requests("restartFrame").is_empty(), "a frame that cannot restart is not reset");
        debug.click(row(&debug, 11).center());
        debug.frame(Vec::new());
        debug.click(debug.button("debug-reset-frame"));
        assert_eq!(debug.requests("restartFrame"), [json!({ "frameId": 11 })]);
        assert_eq!(debug.wait(id, "stopped")["reason"], "restart");
        debug.frame(Vec::new());
        debug.frame(Vec::new());

        let at = row(&debug, 11).center();
        let secondary = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        debug.frame(vec![Event::PointerMoved(at)]);
        debug.frame(vec![secondary(true)]);
        debug.frame(vec![secondary(false)]);
        debug.frame(Vec::new());
        let item = debug.tab().frame_menu.expect("Reset Frame in the frame's menu");
        debug.click(item.center());
        assert_eq!(debug.requests("restartFrame").len(), 2, "the menu resets the frame");
        debug.dispatch("debug.session.stop", json!({ "sessionId": id }));

        let mut plain = Harness::new();
        plain.program(&[3, 5]);
        plain.breakpoint(2);
        let id = plain.start("app");
        plain.stopped(id);
        assert!(
            !plain.window.buttons.iter().any(|(salt, _)| *salt == "debug-reset-frame"),
            "hidden without the capability"
        );
        plain.dispatch("debug.session.stop", json!({ "sessionId": id }));
    }

    impl Harness {
        fn smart(&mut self, smart: &mut SmartStep, events: Vec<Event>) -> Option<(&'static str, Value)> {
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                ..Default::default()
            };
            let mut chosen = None;
            let mut output = self.ctx.run_ui(input, |ui| chosen = smart.show(ui, &IdePalette::zen_dark()));
            output.textures_delta.clear();
            chosen
        }

        fn targets(&mut self, smart: &mut SmartStep) -> Option<(&'static str, Value)> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let stepped = smart.poll(&self.registry, &mut self.session);
                if stepped.is_some() || smart.shown().is_none_or(|(labels, _)| !labels.is_empty()) {
                    return stepped;
                }
                assert!(Instant::now() < deadline, "the targets do not come");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    #[test]
    fn smart_step_into_picks_a_call_in_a_popup_highlights_it_and_steps_into_it() {
        let mut debug = Harness::new();
        debug.adapter.set_capabilities(json!({ "supportsStepInTargetsRequest": true }));
        let target = |id: i64, label: &str, column: i64, end_column: Option<i64>| zen_dap::StepInTarget {
            id,
            label: label.to_owned(),
            line: Some(3),
            column: Some(column),
            end_column,
            ..zen_dap::StepInTarget::default()
        };
        debug.adapter.set_step_in_targets(vec![target(1, "line", 1, Some(5)), target(2, "3", 6, None)]);
        debug.program(&[3, 5, 7]);
        debug.breakpoint(2);
        let id = debug.start("app");
        debug.stopped(id);
        let anchor = Rect::from_min_size(pos2(100.0, 100.0), vec2(1.0, 16.0));
        let mut smart = SmartStep::default();
        smart.open(&debug.registry, &mut debug.session, anchor).unwrap();
        assert_eq!(smart.name(), Some("smartStepInto"));
        assert_eq!(debug.targets(&mut smart), None, "two calls to choose from");
        assert_eq!(smart.shown(), Some((vec!["line".to_owned(), "3".to_owned()], 0)));
        let start = offset_of(&debug.session, &debug.main, 2, 0);
        assert_eq!(smart.highlight(&debug.session), Some((debug.main.clone(), start..start + 4)));
        assert_eq!(debug.smart(&mut smart, press(Key::ArrowDown, Modifiers::NONE).into()), None);
        assert_eq!(
            smart.highlight(&debug.session),
            Some((debug.main.clone(), start + 5..start + 6)),
            "moving highlights"
        );
        assert!(debug.smart(&mut smart, Vec::new()).is_none());
        let first = smart.row(0).unwrap();
        debug.smart(&mut smart, vec![Event::PointerMoved(first.center())]);
        debug.smart(&mut smart, vec![Event::PointerMoved(first.center() + vec2(2.0, 0.0))]);
        assert_eq!(smart.shown().unwrap().1, 0, "the pointer moves the choice too");
        debug.smart(&mut smart, press(Key::ArrowDown, Modifiers::NONE).into());
        let chosen = debug.smart(&mut smart, press(Key::Enter, Modifiers::NONE).into());
        assert_eq!(chosen, Some(("debug.execution.smartStepInto", json!({ "targetId": 2 }))));
        assert!(smart.name().is_none() && smart.highlight(&debug.session).is_none(), "the popup closes");
        debug.dispatch("debug.execution.smartStepInto", json!({ "targetId": 2 }));
        assert_eq!(debug.requests("stepIn"), [json!({ "threadId": 1, "targetId": 2 })]);
        debug.stopped(id);

        debug.adapter.set_step_in_targets(vec![target(9, "line", 1, Some(5))]);
        smart.open(&debug.registry, &mut debug.session, anchor).unwrap();
        assert_eq!(
            debug.targets(&mut smart),
            Some(("debug.execution.smartStepInto", json!({ "targetId": 9 }))),
            "one call is stepped into at once"
        );
        smart.open(&debug.registry, &mut debug.session, anchor).unwrap();
        debug.targets(&mut smart);
        debug.smart(&mut smart, press(Key::Escape, Modifiers::NONE).into());
        assert!(smart.name().is_none(), "Esc closes it");
        debug.dispatch("debug.session.stop", json!({ "sessionId": id }));

        let mut plain = Harness::new();
        plain.program(&[3, 5]);
        plain.breakpoint(2);
        let id = plain.start("app");
        plain.stopped(id);
        let refused = SmartStep::default().open(&plain.registry, &mut plain.session, anchor);
        assert_eq!(refused, Err(t!("ide-smart-step-unsupported")));
        plain.dispatch("debug.session.stop", json!({ "sessionId": id }));
    }

    fn collections() -> Vec<FakeVariable> {
        vec![
            FakeVariable::new("items", "ArrayList@3 size=2", "ArrayList")
                .with_children(vec![
                    FakeVariable::new("0", "Integer@7 \"3\"", "Integer").lazy("Integer@7"),
                    FakeVariable::new("1", "\"b\"", "String"),
                ])
                .with_raw(vec![
                    FakeVariable::new("elementData", "Object[2]@4", "Object[]"),
                    FakeVariable::new("size", "2", "int"),
                ])
                .indexed(),
            FakeVariable::new("ages", "HashMap@5 size=1", "HashMap")
                .with_children(vec![
                    FakeVariable::new("0", "HashMap$Node@9 \"ann\":\"30\"", "HashMap$Node").with_children(vec![
                        FakeVariable::new("key", "\"ann\"", "String"),
                        FakeVariable::new("value", "Integer@4 \"30\"", "Integer").lazy("Integer@4"),
                    ]),
                ])
                .with_raw(vec![FakeVariable::new("table", "HashMap$Node[16]@6", "HashMap$Node[]")])
                .indexed(),
        ]
    }

    impl DebugWindow {
        fn expanded_for_test(&mut self, key: &RowKey) {
            let active = self.active;
            self.tabs[active].expanded.insert(key.clone());
        }
    }

    impl Harness {
        fn start_with_views(&mut self) -> u64 {
            self.adapter.set_views_request(Some("zenViews"));
            let mut configuration = serde_json::to_value(FakeProvider::configuration("app")).unwrap();
            configuration["options"]["views"] = json!("zenViews");
            let started = self.dispatch("debug.session.start", json!({ "configuration": configuration }));
            started["sessionId"].as_u64().unwrap()
        }

        fn texts(&self) -> Vec<String> {
            let p = IdePalette::zen_dark();
            self.tab().rows().0.iter().map(|row| row_job(row, &p).text).collect()
        }

        fn open_row(&mut self, key: &[&str]) {
            let row = self.row(key);
            let depth = key.len() - 2;
            let chevron = pos2(row.left() + 8.0 + depth as f32 * metrics::TREE_INDENT + 6.0, row.center().y);
            self.click(chevron);
            self.frame(Vec::new());
        }
    }

    #[test]
    fn collections_show_their_elements_map_entries_as_key_to_value_and_raw_fields_per_row_or_everywhere() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(collections());
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start_with_views();
        debug.stopped(id);
        debug.open_row(&["scope", "Locals", "items"]);
        debug.open_row(&["scope", "Locals", "ages"]);
        let texts = debug.texts();
        assert!(texts.contains(&"0 = {Integer@7} 3".to_owned()), "a lazy value is shown with its object: {texts:?}");
        assert!(texts.contains(&"1 = {String} \"b\"".to_owned()), "{texts:?}");
        assert!(texts.contains(&"\"ann\" → 30".to_owned()), "a map entry is key → value: {texts:?}");

        debug
            .window
            .perform(
                &DebugAction::RawRow { key: vec!["scope".into(), "Locals".into(), "items".into()] },
                &debug.registry,
                &mut debug.session,
            )
            .unwrap();
        debug.frame(Vec::new());
        let texts = debug.texts();
        assert!(texts.iter().any(|text| text.starts_with("elementData = ")), "one row shows its fields: {texts:?}");
        assert!(texts.contains(&"\"ann\" → 30".to_owned()), "the others keep their elements: {texts:?}");
        assert_eq!(debug.requests("variables").last().unwrap()["variablesReference"], 2);
        let (rows, _) = debug.tab().rows();
        assert!(rows.iter().find(|row| row.name == "items").unwrap().raw);

        let all = DebugAction::RawAll { raw: true };
        debug.window.perform(&all, &debug.registry, &mut debug.session).unwrap();
        debug.frame(Vec::new());
        let texts = debug.texts();
        assert!(texts.iter().any(|text| text.starts_with("table = ")), "every collection shows its fields: {texts:?}");
        assert!(texts.iter().any(|text| text.starts_with("elementData = ")), "{texts:?}");
        assert_eq!(debug.requests("zenViews").last(), Some(&json!({ "raw": true })));
        debug.window.perform(&DebugAction::RawAll { raw: false }, &debug.registry, &mut debug.session).unwrap();
        debug.frame(Vec::new());
        assert!(debug.texts().contains(&"\"ann\" → 30".to_owned()));
        debug.dispatch("debug.session.stop", json!({ "sessionId": id }));
    }

    #[test]
    fn an_evaluated_collection_shows_raw_fields_from_its_rows_menu() {
        let mut debug = Harness::new();
        debug.adapter.set_variables(collections());
        debug.program(&[2]);
        debug.breakpoint(1);
        let id = debug.start_with_views();
        debug.stopped(id);
        let mut evaluations = Evaluations::default();
        let (registry, session) = (&debug.registry, &mut debug.session);
        evaluations.open_dialog(registry, session, "items", pos2(100.0, 100.0));
        debug.settle(&mut evaluations);
        let (_, shown) = evaluations.dialog_shown(0).unwrap();
        assert_eq!(shown, ["items = {ArrayList} ArrayList@3 size=2", "0 = {Integer@7} 3", "1 = {String} \"b\""]);
        evaluations.toggle_raw(0, &["result"]);
        debug.settle(&mut evaluations);
        let (_, shown) = evaluations.dialog_shown(0).unwrap();
        assert!(shown.iter().any(|line| line.starts_with("elementData = ")), "{shown:?}");
        debug.dispatch("debug.session.stop", json!({ "sessionId": id }));
    }

    #[test]
    fn plain_values_move_the_object_aside_and_keep_everything_else() {
        assert_eq!(plain_value("Integer@7 \"3\""), (Some("Integer@7"), "3"));
        assert_eq!(plain_value("\"text\""), (None, "\"text\""));
        assert_eq!(plain_value("ArrayList@11 size=4"), (None, "ArrayList@11 size=4"));
        assert_eq!(plain_value("HashMap$Node@9 \"ann\":\"30\""), (None, "HashMap$Node@9 \"ann\":\"30\""));
        assert_eq!(plain_value("a b@c \"x\""), (None, "a b@c \"x\""));
    }

    const SHOP: &str =
        "class Shop {\n    void run() {\n        long n = names.stream().filter(s -> s.isEmpty()).count();\n    }\n}\n";

    impl Harness {
        fn trace_frame(&mut self, traces: &mut StreamTraces) {
            self.time += 0.1;
            traces.poll(&self.registry, &mut self.session);
            let input = egui::RawInput {
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| traces.show(ui, &IdePalette::zen_dark()));
            output.textures_delta.clear();
        }
    }

    #[test]
    fn trace_current_stream_chain_fills_one_column_per_operation_without_waiting() {
        let mut debug = Harness::new();
        let shop = debug.main.with_file_name("Shop.java");
        std::fs::write(&shop, SHOP).unwrap();
        debug.adapter.set_program(&[(shop.as_path(), 3)]);
        debug.dispatch("debug.breakpoint.set", json!({ "path": shop, "line": 2 }));
        let array = |values: &[&str]| {
            let children =
                values.iter().enumerate().map(|(index, value)| FakeVariable::new(&index.to_string(), value, "String"));
            FakeVariable::new("", &format!("Object[{}]@1", values.len()), "Object[]")
                .with_children(children.collect())
                .indexed()
        };
        debug.adapter.set_evaluations(vec![
            ("names.stream().toArray()", array(&["\"\"", "\"x\""])),
            ("names.stream().filter(s -> s.isEmpty()).toArray()", array(&["\"\""])),
            ("names.stream().filter(s -> s.isEmpty()).count()", FakeVariable::new("", "1", "long")),
        ]);
        let id = debug.start("app");
        debug.stopped(id);
        let mut traces = StreamTraces::default();
        traces.open(&debug.registry, &mut debug.session, &json!({})).unwrap();
        assert_eq!(traces.name(), Some("streamTrace"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while traces.is_pending() {
            assert!(Instant::now() < deadline, "the trace stays pending");
            debug.trace_frame(&mut traces);
        }
        debug.trace_frame(&mut traces);
        let strings = |values: &[&str]| values.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            traces.shown().unwrap(),
            [
                ("names.stream()".to_owned(), strings(&["\"\"", "\"x\""])),
                ("filter(s -> s.isEmpty())".to_owned(), strings(&["\"\""])),
                ("count()".to_owned(), strings(&["Result: 1"])),
            ]
        );
        debug.dispatch("debug.execution.resume", json!({}));
        debug.wait(id, "ended");
        debug.trace_frame(&mut traces);
        assert_eq!(traces.name(), None, "the dialog closes when the program goes on");

        let mut plain = Harness::new();
        let mut traces = StreamTraces::default();
        let refused = traces.open(&plain.registry, &mut plain.session, &json!({})).unwrap_err();
        assert_eq!(refused, t!("ide-stream-not-stopped"));
        plain.frame(Vec::new());
    }
}
