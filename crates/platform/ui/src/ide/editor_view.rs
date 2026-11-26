use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{Align2, Color32, CornerRadius, Event, FontId, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::{HighlightKind, Highlighter, Highlighting, Span, TextChange};
use zen_theme::SyntaxKey;
use zen_workbench::{Document, Workbench};

use super::IdePalette;
use super::caret_popup::{CaretPlace, PopupAnchor};
use super::code_vision::{Click, RowLayout};
use super::inlay_hints::{self, Placed};
use super::metrics::{self, EditorMetrics, gutter};
use super::soft_wrap::{self, ShownRows};
use super::visual_aids;
use crate::accessibility;
use crate::theme_bridge::CodeStyle;

type FoundMatches = (u64, zen_editor::Query, Option<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>);

const TOP: f32 = 0.0;
const TEXT_LEFT: f32 = gutter::TEXT_GAP;
const BLINK: f64 = 0.5;
const CARET_WIDTH: f32 = 2.0;
const GALLEY_CACHE: usize = 4096;
const SETTLE: f64 = 0.3;
const FOLD_MARGIN: usize = 300;
const MAX_REVEAL_FRAMES: u32 = 120;

const FORMAT_COMMANDS: [&str; 2] = ["editor.code.reformat", "editor.imports.optimize"];

const PREVIEW_MARGIN: usize = 50;

const SECOND_KEYSTROKE_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

type PreviewKey = (u64, zen_editor::Query, String, Vec<std::ops::Range<usize>>);
type MergedExtras =
    (Arc<Vec<inlay_hints::ShownHint>>, super::workspace_host::TextExtras, Arc<Vec<inlay_hints::ShownHint>>);
type Previews = (PreviewKey, (usize, usize), Arc<Vec<inlay_hints::ShownHint>>, Arc<Vec<inlay_hints::ShownHint>>);

#[derive(Default)]
pub(super) struct EditorView {
    extras: Option<(PathBuf, super::workspace_host::TextExtras)>,
    extras_merged: Option<MergedExtras>,
    completion: Option<(CompletionShown, usize)>,
    completion_offset: f32,
    completion_documentation: Option<(u64, usize, Arc<Vec<super::doc_popups::Block>>)>,
    folds_revealed: Option<(PathBuf, u64)>,
    fold_listings: u64,
    caret_rect: Option<Rect>,
    scrolled_x: f32,
    caret_moving: bool,
    caret_moving_frames: u32,
    caret_text: Option<Rect>,
    semantic: Option<(PathBuf, u64, Arc<Vec<SemanticSpan>>)>,
    todo_marks: super::todo_marks::TodoMarks,
    inlay_hints: inlay_hints::InlayHints,
    pub(super) ligatures: bool,
    pub(super) run_icons: bool,
    pub(super) passive: bool,
    pub(super) split: Option<(usize, usize)>,
    pub(super) code_vision: super::code_vision::CodeVisionView,
    pub(super) square_bottom: bool,
    double_option: DoubleOption,
    sequence: Option<(zen_core::keymap::Keystroke, std::time::Instant)>,
    pub(super) font: super::settings::EditorFont,
    pub(super) hide_line_numbers: bool,
    pub(super) line_numbers_on: bool,
    separators: super::line_numbers::MethodSeparators,
    line_height: f32,
    folds: Option<(FoldsKey, Arc<Vec<FoldShown>>)>,
    shown_lines: (usize, usize),
    scroll_line: Option<usize>,
    edited: Option<(PathBuf, u64, f64)>,
    completion_reveal: bool,
    last_press: Option<(f64, egui::Pos2, u8)>,
    highlight: Option<HighlightCache>,
    galleys: HashMap<u64, Arc<egui::Galley>>,
    galley_scale: u32,
    galley_font: Option<(FontId, u32)>,
    galley_styles: Option<[CodeStyle; SyntaxKey::COUNT]>,
    galley_atlas: u64,
    shown: Option<(PathBuf, u64)>,
    last_activity: f64,
    reveal: bool,
    drag: Option<(usize, usize, u8)>,
    column_drag: Option<(usize, usize)>,
    preedit: String,
    page_lines: usize,
    focus_next: bool,
    matches: Option<FoundMatches>,
    previews: Option<Previews>,
    targets: super::targets_popup::TargetsPopup,
    regions: super::regions_popup::RegionsPopup,
    usages: super::usages_popup::UsagesPopup,
    lookup_at: Option<egui::Pos2>,
    focus_id: Option<egui::Id>,
    markdown_toolbar_shown: Option<Rect>,
    format_pending: Option<(String, PathBuf, u64)>,
    notice: Option<String>,
    paste_offer: Option<super::convert_paste::PasteOffer>,
    doc_popups: super::doc_popups::DocPopups,
    problem_popups: super::problem_popup::ProblemPopups,
    peek: Option<PeekHover>,
    occurrences: Option<((ShownAt, bool), bool, Vec<UsageBand>)>,
    brackets: Option<(BracketsAt, Option<zen_editor::BracketMatch>)>,
    run_targets: Option<RunTargetsShown>,
    test_marks: HashMap<(String, String), TestMark>,
    runs_seen: HashSet<u64>,
    run_menu: Option<RunMenu>,
    line_markers: super::line_markers::LineMarkers,
    swatches: super::color_swatches::ColorSwatches,
    window_actions: Vec<&'static str>,
    changes: Vec<zen_workbench::vcs::commit::LineChange>,
    change_popup: Option<(usize, egui::Pos2)>,
    diff_request: Option<PathBuf>,
    blame: Option<(PathBuf, u64, f64, zen_workbench::vcs::log::BlameInfo)>,
    blame_request: Option<String>,
    breakpoints: Option<BreakpointsShown>,
    bookmarks: Option<BookmarksShown>,
    drawn_bookmarks: Vec<(usize, Option<char>)>,
    gutter_menu_line: Option<usize>,
    gutter_actions: Vec<(String, Value)>,
    breakpoint_drag: Option<usize>,
    breakpoint_anchor: Option<egui::Pos2>,
    value_hover: Option<(String, Rect, u64)>,
    value_anchor: Option<Rect>,
    no_gutter_menu: bool,
    debugging: Option<(f64, bool)>,
    execution: Option<(f64, Option<(PathBuf, usize)>)>,
    drawn_marks: Vec<(usize, GutterMark)>,
    coverage: Option<CoverageShown>,
    drawn_coverage: Vec<(usize, zen_run::LineStatus)>,
    drawn_execution: Option<usize>,
    stop_key: Option<Value>,
    inline_values: super::inline_values::InlineValues,
    drawn_run_inlay: Option<(usize, Rect)>,
    step_target: Option<(PathBuf, std::ops::Range<usize>)>,
    drawn_step_target: Option<Rect>,
    debug_request: Option<Value>,
    external_tool_request: Option<String>,
    starters_request: Option<PathBuf>,
    soft_wrap: soft_wrap::SoftWrap,
    sticky: super::sticky_lines::StickyLines,
    access: super::editor_access::EditorAccess,
    intentions: super::intentions_popup::IntentionsPopup,
    generate: super::generate_popup::GeneratePopup,
    caret_block: visual_aids::CaretBlock,
    surround: super::surround_popup::SurroundPopup,
    configure_templates: bool,
    unwrap: super::unwrap_popup::UnwrapPopup,
    sql_connection: super::sql_connection_popup::SqlConnectionPopup,
    extract: super::extract_popup::ExtractFlow,
}

#[derive(Clone, Debug, PartialEq)]
struct GutterTarget {
    line: usize,
    kind: String,
    name: String,
    package: String,
    configuration: Value,
}

impl GutterTarget {
    fn from_json(target: &Value) -> Option<Self> {
        Some(Self {
            line: target["line"].as_u64()?.checked_sub(1)? as usize,
            kind: target["kind"].as_str()?.to_owned(),
            name: target["name"].as_str()?.to_owned(),
            package: target["package"].as_str().unwrap_or_default().to_owned(),
            configuration: target["configuration"].clone(),
        })
    }
}

const HTTP: &str = "http";

const SQL: &str = "sql";

pub(super) fn is_script(session: &Session, path: &std::path::Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
        && !session
            .state::<zen_workbench::workspaces::Workspaces>()
            .is_some_and(|workspaces| workspaces.is_console(path))
}

fn sql_targets(registry: &Registry, session: &mut Session, path: &std::path::Path) -> Vec<GutterTarget> {
    let listed = registry.dispatch(session, "db.script.targets", json!({ "path": path })).ok();
    listed
        .as_ref()
        .and_then(|answer| answer["targets"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|target| {
            Some(GutterTarget {
                line: (target["line"].as_u64()? as usize).checked_sub(1)?,
                kind: SQL.to_owned(),
                name: target["label"].as_str()?.to_owned(),
                package: String::new(),
                configuration: json!({ "offset": target["offset"] }),
            })
        })
        .collect()
}

fn code_sql_targets(registry: &Registry, session: &mut Session, path: &std::path::Path) -> Vec<GutterTarget> {
    if !zen_workbench::database::injected::hosts(path) || registry.info("db.injection.run").is_none() {
        return Vec::new();
    }
    let listed = registry.dispatch(session, "db.injection.list", json!({ "path": path, "limit": 500 })).ok();
    listed
        .as_ref()
        .and_then(|answer| answer["fragments"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|fragment| {
            Some(GutterTarget {
                line: (fragment["line"].as_u64()? as usize).checked_sub(1)?,
                kind: SQL.to_owned(),
                name: fragment["text"].as_str()?.lines().next().unwrap_or_default().to_owned(),
                package: String::new(),
                configuration: json!({ "offset": fragment["start"] }),
            })
        })
        .collect()
}

fn http_targets(registry: &Registry, session: &mut Session, path: &std::path::Path) -> Vec<GutterTarget> {
    let listed = registry.dispatch(session, "http.requests.list", json!({ "path": path })).ok();
    listed
        .as_ref()
        .and_then(|answer| answer["requests"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|request| {
            let name = request["name"].as_str().map_or_else(
                || {
                    format!(
                        "{} {}",
                        request["method"].as_str().unwrap_or_default(),
                        request["url"].as_str().unwrap_or_default()
                    )
                },
                str::to_owned,
            );
            Some(GutterTarget {
                line: (request["line"].as_u64()? as usize).checked_sub(1)?,
                kind: HTTP.to_owned(),
                name,
                package: String::new(),
                configuration: json!({ "index": request["index"] }),
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestMark {
    Passed,
    Failed,
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BreakpointState {
    Verified,
    Invalid,
    Disabled,
    Conditional,
    Logging,
    Muted,
    MutedDisabled,
}

#[derive(Clone, Debug, PartialEq)]
struct BreakpointMark {
    line: usize,
    state: BreakpointState,
    message: Option<String>,
}

impl BreakpointMark {
    fn from_json(breakpoint: &Value, muted: bool) -> Option<Self> {
        let off = breakpoint["enabled"] == false || breakpoint["waiting"] == true;
        let state = if muted && off {
            BreakpointState::MutedDisabled
        } else if muted {
            BreakpointState::Muted
        } else if off {
            BreakpointState::Disabled
        } else if breakpoint["verified"] == false {
            BreakpointState::Invalid
        } else if breakpoint["suspend"] == "none" {
            BreakpointState::Logging
        } else if breakpoint["condition"].is_string() {
            BreakpointState::Conditional
        } else {
            BreakpointState::Verified
        };
        Some(Self {
            line: breakpoint["line"].as_u64()? as usize,
            state,
            message: breakpoint["message"].as_str().filter(|message| !message.is_empty()).map(str::to_owned),
        })
    }
}

struct BreakpointsShown {
    path: PathBuf,
    version: u64,
    read: f64,
    marks: Vec<BreakpointMark>,
}

#[derive(Clone, Debug, PartialEq)]
struct BookmarkMark {
    line: usize,
    mnemonic: Option<char>,
    description: String,
}

impl BookmarkMark {
    fn from_json(bookmark: &Value) -> Option<Self> {
        Some(Self {
            line: bookmark["line"].as_u64()?.checked_sub(1)? as usize,
            mnemonic: bookmark["mnemonic"].as_str().and_then(zen_workbench::bookmarks::mnemonic_of),
            description: bookmark["description"].as_str().unwrap_or_default().to_owned(),
        })
    }

    fn hint(&self) -> String {
        match (self.description.is_empty(), self.mnemonic) {
            (false, _) => self.description.clone(),
            (true, Some(mnemonic)) => zen_i18n::t!("ide-bookmark-hint-mnemonic", mnemonic = mnemonic.to_string()),
            (true, None) => zen_i18n::t!("ide-bookmark-hint"),
        }
    }
}

struct BookmarksShown {
    path: PathBuf,
    key: (u64, u64),
    marks: Vec<BookmarkMark>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GutterMark {
    Breakpoint(BreakpointState),
    Hover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuIcon {
    Run,
    Debug,
    Coverage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuChoice {
    Run,
    Debug,
    Coverage,
    Modify,
}

struct RunMenu {
    target: GutterTarget,
    at: egui::Pos2,
    fresh: bool,
    coverage: bool,
    items: Vec<Rect>,
}

type CoverageShown = (PathBuf, u64, Arc<Vec<zen_run::LineCoverage>>);
const ANNOTATIONS: f32 = 190.0;

struct GutterRows<'a> {
    gutter: Rect,
    rows: &'a ShownRows,
    layout: &'a RowLayout,
    first: usize,
    count: usize,
    scrolled: f32,
}

type ShownAt = (PathBuf, u64, u64);

#[derive(Clone, Copy, Debug, PartialEq)]
struct UsageBand {
    start: usize,
    end: usize,
    write: bool,
}

fn usage_bands(answer: Option<Value>, path: &std::path::Path) -> (bool, Vec<UsageBand>) {
    let Some(answer) = answer.filter(|answer| answer["path"].as_str().map(std::path::Path::new) == Some(path)) else {
        return (false, Vec::new());
    };
    let bands = answer["ranges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|range| {
            Some(UsageBand {
                start: range["start"].as_u64()? as usize,
                end: range["end"].as_u64()? as usize,
                write: range["kind"] == "write",
            })
        })
        .collect();
    (answer["pending"] == true, bands)
}

type RunTargetsShown = ((PathBuf, u64), Arc<Vec<GutterTarget>>);

type FoldsKey = (PathBuf, u64, u64, (usize, usize), bool);

struct PeekHover {
    path: PathBuf,
    start: usize,
    end: usize,
    anchor: Rect,
    since: std::time::Instant,
    asked: bool,
    state: Option<Value>,
}

const PEEK_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

fn types_text(key: egui::Key) -> bool {
    key == egui::Key::Space || (egui::Key::Colon..=egui::Key::Z).contains(&key)
}

fn reports_failure(command: &str) -> bool {
    command.starts_with("navigate.")
        || command.starts_with("editor.documentation.")
        || command.starts_with("editor.parameterInfo.")
}

type BracketsAt = (PathBuf, u64, usize, usize);

struct HighlightCache {
    path: PathBuf,
    version: u64,
    highlighter: usize,
    highlighted_by: Option<Arc<dyn Highlighter>>,
    highlighting: Option<Arc<std::sync::Mutex<Highlighting>>>,
    line_bytes: Vec<usize>,
    widths: Vec<usize>,
    max_columns: usize,
    tab_size: usize,
}

impl HighlightCache {
    fn spans(&self, text: &str, range: std::ops::Range<usize>) -> Vec<Span> {
        self.highlighting.as_ref().map_or_else(Vec::new, |highlighting| {
            highlighting.lock().unwrap_or_else(std::sync::PoisonError::into_inner).spans_with_injections(text, range)
        })
    }

    fn kind_at(&self, text: &str, offset: usize) -> Option<HighlightKind> {
        let highlighting = self.highlighting.as_ref()?;
        highlighting.lock().unwrap_or_else(std::sync::PoisonError::into_inner).kind_at(text, offset)
    }
}

#[derive(Debug)]
struct DoubleOption {
    down: Option<bool>,
    released: f64,
    armed: bool,
}

impl Default for DoubleOption {
    fn default() -> Self {
        Self { down: None, released: f64::NEG_INFINITY, armed: false }
    }
}

impl DoubleOption {
    const INTERVAL: f64 = 0.4;

    fn update(&mut self, now: f64, modifiers: egui::Modifiers, typed: bool) -> bool {
        let alone = modifiers.alt && !modifiers.shift && !modifiers.ctrl && !modifiers.command;
        match self.down {
            None if alone => {
                self.armed = !typed && now - self.released < Self::INTERVAL;
                self.down = Some(!typed);
            }
            Some(clean) if modifiers.alt => self.down = Some(clean && alone && !typed),
            Some(clean) => {
                self.released = if clean && !self.armed { now } else { f64::NEG_INFINITY };
                self.down = None;
                self.armed = false;
            }
            None => {
                if typed {
                    self.released = f64::NEG_INFINITY;
                }
            }
        }
        self.armed
    }
}

struct Intent(&'static str, Value);

impl EditorView {
    #[cfg(test)]
    pub(super) fn access_is_empty(&self) -> bool {
        self.access.is_empty()
    }

    #[cfg(test)]
    pub(super) fn shown_top(&self) -> (usize, f32) {
        (self.shown_lines.0, self.line_height)
    }

    pub(super) fn first_shown_line(&self) -> usize {
        self.shown_lines.0
    }

    #[cfg(test)]
    pub(super) fn completing(&self) -> bool {
        self.completion.is_some()
    }

    pub(super) fn scroll_to_line(&mut self, line: usize) {
        self.scroll_line = Some(line);
    }

    pub(super) fn focus(&mut self) {
        self.focus_next = true;
    }

    pub(super) fn cancel_focus(&mut self) -> bool {
        std::mem::take(&mut self.focus_next)
    }

    pub(super) fn caret_place(&self) -> CaretPlace {
        CaretPlace::new(self.caret_rect, self.caret_moving, self.caret_text)
    }

    pub(super) fn release_focus(&mut self, ctx: &egui::Context) {
        self.focus_next = false;
        if let Some(id) = self.focus_id {
            ctx.memory_mut(|memory| memory.surrender_focus(id));
        }
    }

    pub(super) fn open_intentions(&mut self) {
        self.completion = None;
        self.intentions.open(PopupAnchor::UNDER_CARET);
    }

    pub(super) fn ask_sql_connection(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &std::path::Path,
        run: Option<Value>,
    ) {
        self.completion = None;
        self.sql_connection.open(registry, session, path, run);
        self.focus();
    }

    #[cfg(test)]
    pub(super) fn sql_connection_highlighted(&self) -> Option<&str> {
        self.sql_connection.highlighted()
    }

    #[cfg(test)]
    pub(super) fn scrolled_x(&self) -> f32 {
        self.scrolled_x
    }

    #[cfg(test)]
    pub(super) fn sql_connection_open(&self) -> bool {
        self.sql_connection.is_open()
    }

    fn sql_connection_chosen(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        chosen: super::sql_connection_popup::Chosen,
    ) {
        let bound = if chosen.project {
            registry
                .dispatch(session, "db.injection.setDefault", json!({ "connection": chosen.connection }))
                .and_then(|_| registry.dispatch(session, "db.injection.bind", json!({ "path": chosen.path })))
        } else {
            registry.dispatch(
                session,
                "db.injection.bind",
                json!({ "path": chosen.path, "connection": chosen.connection }),
            )
        };
        match bound {
            Ok(_) => {
                if let Some(run) = chosen.run {
                    self.gutter_actions.push(("db.injection.run".to_owned(), run));
                }
            }
            Err(zen_core::DispatchError::Failed { error, .. }) => self.notice = Some(error.message),
            Err(error) => self.notice = Some(error.to_string()),
        }
        self.focus();
    }

    pub(super) fn intentions_open(&self) -> bool {
        self.intentions.is_open()
    }

    pub(super) fn open_generate(&mut self, action: Option<&str>) {
        self.completion = None;
        self.generate.open(PopupAnchor::UNDER_CARET, action);
    }

    pub(super) fn generate_open(&self) -> Option<&'static str> {
        self.generate.open_name()
    }

    pub(super) fn open_extract(&mut self, command: &'static str) {
        self.completion = None;
        self.extract.open(command);
    }

    pub(super) fn extract_open(&self) -> Option<&'static str> {
        self.extract.open_name()
    }

    pub(super) fn open_custom_folding(&mut self, regions: &Value) {
        self.completion = None;
        if !self.regions.open(regions) {
            self.notice = Some(t!("ide-custom-folding-none"));
        }
        self.focus();
    }

    pub(super) fn custom_folding_open(&self) -> bool {
        self.regions.is_open()
    }

    pub(super) fn open_surround(&mut self, registry: &Registry, session: &mut Session, templates_only: bool) {
        self.completion = None;
        let at = self.caret_place().rect().map_or(egui::Pos2::ZERO, |caret| caret.left_bottom() + vec2(0.0, 2.0));
        if let Some(outcome) = self.surround.open(registry, session, at, templates_only) {
            self.surround_outcome(registry, session, outcome);
        }
    }

    pub(super) fn surround_open(&self) -> Option<&'static str> {
        self.surround.open_name()
    }

    pub(super) fn take_configure_templates(&mut self) -> bool {
        std::mem::take(&mut self.configure_templates)
    }

    pub(super) fn unwrap_open(&self) -> bool {
        self.unwrap.is_open()
    }

    pub(super) fn open_unwrap(&mut self, registry: &Registry, session: &mut Session) -> Result<usize, String> {
        let Some((offset, version)) = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| (document.selections().primary().head, document.version()))
        else {
            return Ok(0);
        };
        let listed = registry
            .dispatch(session, "editor.unwrap.list", json!({ "offset": offset }))
            .map_err(|error| error.to_string())?;
        let count = listed.as_array().map_or(0, Vec::len);
        if count == 1 {
            registry
                .dispatch(session, "editor.unwrap.apply", json!({ "index": 0, "offset": offset }))
                .map_err(|error| error.to_string())?;
            self.reveal = true;
        } else if count > 1 {
            self.completion = None;
            let at = self.caret_place().rect().map_or(egui::Pos2::ZERO, |caret| caret.left_bottom() + vec2(-24.0, 2.0));
            self.unwrap.open(&listed, at, offset, version);
        }
        Ok(count)
    }

    pub(super) fn has_popup(&self) -> bool {
        self.completion.is_some()
            || self.run_menu.is_some()
            || self.line_markers.menu_open()
            || self.change_popup.is_some()
            || self.intentions_open()
            || self.generate_open().is_some()
            || self.surround_open().is_some()
            || self.unwrap_open()
            || self.extract.is_open()
            || self.usages.is_open()
            || self.regions.is_open()
    }

    pub(super) fn takes_key(&self, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        !self.passive && (self.usages.takes_key(key, modifiers) || self.completes_sequence(key, modifiers))
    }

    fn pending_sequence(&self) -> Option<&zen_core::keymap::Keystroke> {
        self.sequence.as_ref().filter(|(_, since)| since.elapsed() < SECOND_KEYSTROKE_WAIT).map(|(first, _)| first)
    }

    fn completes_sequence(&self, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        self.pending_sequence().is_some_and(|first| {
            let editor = zen_core::keymap::Context::Editor;
            let stroke = first.followed_by(crate::keymap::keystroke(key, modifiers));
            crate::keymap::current().lookup(&stroke, editor).is_some_and(|binding| binding.context == editor)
        })
    }

    pub(super) fn set_extras(&mut self, extras: Option<(PathBuf, super::workspace_host::TextExtras)>) {
        self.extras = extras;
    }

    fn with_extras(
        &mut self,
        path: &std::path::Path,
        hints: Arc<Vec<inlay_hints::ShownHint>>,
    ) -> Arc<Vec<inlay_hints::ShownHint>> {
        let Some((_, extras)) =
            self.extras.as_ref().filter(|(shown, extras)| shown == path && !extras.hints.is_empty())
        else {
            return hints;
        };
        if let Some((base, known, merged)) = &self.extras_merged
            && Arc::ptr_eq(base, &hints)
            && known == extras
        {
            return Arc::clone(merged);
        }
        let mut all: Vec<inlay_hints::ShownHint> = hints.as_ref().clone();
        for (index, (offset, label, tooltip, _)) in extras.hints.iter().enumerate() {
            all.push(inlay_hints::ShownHint {
                id: u64::MAX / 2 - index as u64,
                offset: *offset,
                label: label.clone(),
                padding_left: true,
                padding_right: false,
                tooltip: tooltip.clone(),
                resolvable: false,
            });
        }
        all.sort_by_key(|hint| hint.offset);
        let merged = Arc::new(all);
        self.extras_merged = Some((hints, extras.clone(), Arc::clone(&merged)));
        merged
    }

    pub(super) fn usages_open(&self) -> bool {
        !self.passive && self.usages.is_open()
    }

    fn markdown_toolbar(
        &mut self,
        ui: &Ui,
        session: &Session,
        palette: &IdePalette,
        (band, area): (Rect, Rect),
        path: &std::path::Path,
        was_shown: Option<Rect>,
    ) -> Option<(&'static str, Value)> {
        use zen_workbench::preview::PreviewKind;
        let pointer = ui.input(|input| input.pointer.latest_pos());
        let in_use = was_shown.is_some_and(|rect| {
            egui::Popup::is_any_open(ui.ctx()) || pointer.is_some_and(|pointer| rect.contains(pointer))
        });
        let shown = !self.passive
            && PreviewKind::of(path) == Some(PreviewKind::Markdown)
            && !session.settings().get_or(zen_workbench::markdown::format::HIDE_FLOATING_TOOLBAR, false)
            && (self.has_focus_in(ui.ctx()) || in_use)
            && (in_use || !ui.input(|input| input.pointer.primary_down()));
        if !shown {
            return None;
        }
        let level = session.state::<Workbench>().and_then(Workbench::active_document).map_or(0, |document| {
            zen_workbench::markdown::format::heading_level(document.snapshot(), document.selections())
        });
        let (asked, rect) =
            super::markdown_toolbar::show(ui, palette, super::markdown_toolbar::place(band, area), level);
        self.markdown_toolbar_shown = Some(rect);
        if asked.is_some()
            && let Some(id) = self.focus_id
        {
            ui.ctx().memory_mut(|memory| memory.request_focus(id));
        }
        asked
    }

    pub(super) fn has_focus_in(&self, ctx: &egui::Context) -> bool {
        self.focus_id.is_some_and(|id| ctx.memory(|memory| memory.has_focus(id)))
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        highlighter: Option<Arc<dyn Highlighter>>,
        palette: &IdePalette,
    ) -> Option<(usize, usize)> {
        let started = std::time::Instant::now();
        let caret = self.show_inner(ui, registry, session, highlighter, palette);
        profile(started.elapsed());
        caret
    }

    fn show_inner(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        highlighter: Option<Arc<dyn Highlighter>>,
        palette: &IdePalette,
    ) -> Option<(usize, usize)> {
        let (path, selection_version) = {
            let document = session.state::<Workbench>()?.active_document()?;
            (document.path().to_owned(), document.selection_version())
        };
        let id = ui.make_persistent_id(("editor-view", path.clone()));
        self.focus_id = Some(id);
        let edit_starters = zen_workbench::spring_boot::starters::offered(session, &path);
        let tools = session
            .state::<zen_workbench::external_tools::ExternalTools>()
            .map(zen_workbench::external_tools::ExternalTools::tools)
            .unwrap_or_default();
        let now = ui.input(|input| input.time);
        if std::mem::take(&mut self.focus_next) {
            ui.memory_mut(|memory| memory.request_focus(id));
        }

        let take_keyboard = !self.passive;
        match &self.shown {
            Some((shown, _)) if *shown != path => {
                if take_keyboard {
                    ui.memory_mut(|memory| memory.request_focus(id));
                }
                self.reveal = true;
            }
            Some((_, version)) if *version != selection_version => self.reveal = true,
            None => {
                if take_keyboard {
                    ui.memory_mut(|memory| memory.request_focus(id));
                }
                self.reveal = true;
            }
            _ => {}
        }

        self.swatches.keys(ui);
        if !self.passive {
            let actions = self.usages.keys(ui);
            self.usages_actions(registry, session, actions);
        }
        let focused = ui.memory(|memory| memory.has_focus(id));
        if focused {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
            self.completion_keys(ui, registry, session);
            if let Some(outcome) = self.intentions.keys(ui) {
                self.intention_outcome(registry, session, outcome);
            }
            if let Some(outcome) = self.generate.keys(ui) {
                self.generate_outcome(registry, session, outcome);
            }
            if let Some(chosen) = self.sql_connection.keys(ui) {
                self.sql_connection_chosen(registry, session, chosen);
            }
            if let Some(outcome) = self.surround.keys(ui) {
                self.surround_outcome(registry, session, outcome);
            }
            if let Some(target) = self.targets.keys(ui) {
                self.target_chosen(registry, session, target);
            }
            if let Some(start) = self.regions.keys(ui) {
                let _ = registry.dispatch(session, "navigate.place.goTo", json!({ "path": path, "offset": start }));
            }
            if let Some(chosen) = self.unwrap.keys(ui) {
                self.apply_unwrap(registry, session, chosen);
            }
            if let Some(notice) = self.extract.keys(ui, registry, session) {
                self.notice = Some(notice);
            }
            self.doc_popups.keys(ui, registry, session);
            let typed: String = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| if let Event::Text(text) = event { Some(text.as_str()) } else { None })
                    .collect()
            });
            let commands = if self.problem_popups.is_open()
                || self.sql_connection.is_open()
                || self.generate_open() == Some("memberChooser")
            {
                Vec::new()
            } else {
                self.keyboard(ui, session)
            };
            if !commands.is_empty() {
                self.last_activity = now;
                self.reveal = true;
            }
            for (command, mut params) in commands {
                if command.starts_with("debug.") {
                    self.breakpoints = None;
                }
                if command.starts_with("db.script.") {
                    if is_script(session, &path) {
                        params["path"] = json!(path);
                        self.gutter_actions.push((command, params));
                    } else if command == "db.script.run"
                        && zen_workbench::database::injected::sql_at(session, &path, None)
                    {
                        params["path"] = json!(path);
                        self.gutter_actions.push(("db.injection.run".to_owned(), params));
                    }
                    continue;
                }
                let result = registry.dispatch(session, &command, params);
                match result {
                    Ok(value) if FORMAT_COMMANDS.contains(&command.as_str()) => {
                        let version =
                            session.state::<Workbench>().and_then(Workbench::active_document).map(Document::version);
                        self.format_answer(ui, (command, path.clone(), version.unwrap_or_default()), &value);
                    }
                    Err(zen_core::DispatchError::Failed { error, .. }) if reports_failure(&command) => {
                        self.notice = Some(error.message);
                    }
                    _ => {}
                }
            }
            if let Some(offer) = self.paste_offer.clone()
                && let Some(convert) = super::convert_paste::show(ui, palette, &offer)
            {
                self.paste_offer = None;
                if let Some(notice) = super::convert_paste::paste(registry, session, &offer, convert) {
                    self.notice = Some(notice);
                }
            }
            if let Some(last) = typed.chars().last()
                && (last == '.' || (self.completion.is_none() && (last.is_alphanumeric() || last == '_')))
                && !zen_workbench::power_save::is_on(session)
            {
                let _ = registry.dispatch(session, "editor.completion.start", json!({}));
            }
            let since = self.completion.as_ref().map(|(shown, _)| shown.revision);
            let listed = registry.dispatch(session, "editor.completion.list", json!({ "since": since })).ok();
            self.completion = listed.filter(|list| list["open"] == true).map(|list| {
                let previous = self.completion.take();
                let shown = CompletionShown::from_json(&list, previous.as_ref().map(|(shown, _)| shown));
                if previous.as_ref().is_none_or(|(before, _)| before.revision != shown.revision) {
                    self.completion_reveal = true;
                }
                let selected = previous.map_or(0, |(_, selected)| selected).min(shown.items.len().saturating_sub(1));
                (shown, selected)
            });
            if self.completion.as_ref().is_some_and(|(shown, _)| shown.pending) {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
        } else if self.completion.take().is_some() {
            let _ = registry.dispatch(session, "editor.completion.cancel", json!({}));
        }
        if self.passive {
            self.doc_popups = super::doc_popups::DocPopups::default();
        } else {
            self.doc_popups.follow(ui, registry, session);
        }
        self.follow_format(ui, registry, session);

        let problems: Vec<ShownProblem> = registry
            .dispatch(session, "problems.file.list", json!({ "path": path }))
            .map(|answer| zen_workbench::language::listed_problems(&answer))
            .unwrap_or_default()
            .iter()
            .filter_map(ShownProblem::from_json)
            .collect();

        let version = session.state::<Workbench>().and_then(Workbench::active_document).map(Document::version);
        let settled = match (&self.edited, version) {
            (Some((shown, at, since)), Some(version)) if *shown == path && *at == version => {
                let waited = now - since;
                if waited < SETTLE {
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SETTLE - waited));
                }
                waited >= SETTLE
            }
            (Some((shown, _, _)), Some(version)) if *shown == path => {
                self.edited = Some((path.clone(), version, now));
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SETTLE));
                false
            }
            (_, version) => {
                self.edited = version.map(|version| (path.clone(), version, f64::NEG_INFINITY));
                true
            }
        };

        let carets = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| ((path.clone(), document.selection_version()), !document.folds().is_empty()));
        if let Some((carets, folded)) = carets
            && self.folds_revealed.as_ref() != Some(&carets)
        {
            if folded {
                let _ = registry.dispatch(session, "editor.fold.reveal", json!({}));
            }
            self.folds_revealed = Some(carets);
        }
        let key = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| (path.clone(), document.version(), document.folds_version()));
        let listed_for = self.folds.as_ref().filter(|((shown, ..), _)| *shown == path).map(|(at, _)| at.clone());
        let (first, last) = self.shown_lines;
        let window = listed_for
            .as_ref()
            .map(|(_, _, _, window, _)| *window)
            .filter(|&(from, to)| from <= first.saturating_sub(FOLD_MARGIN / 3) && last + FOLD_MARGIN / 3 <= to)
            .unwrap_or((first.saturating_sub(FOLD_MARGIN), last + FOLD_MARGIN));
        let current = listed_for.as_ref().is_some_and(|(_, _, _, _, current)| *current);
        let wanted = key.clone().map(|(shown, version, folds)| (shown, version, folds, window, settled));
        let same_text =
            |at: &FoldsKey| key.as_ref().is_some_and(|(_, version, folds)| at.1 == *version && at.2 == *folds);
        let fresh = listed_for.as_ref().is_some_and(|at| same_text(at) && at.3 == window && (current || !settled));
        let moved = match (&self.folds, &key) {
            (Some((at, listed)), Some((_, version, folds)))
                if !fresh && !settled && at.0 == path && at.2 == *folds && at.3 == window =>
            {
                let document = session.state::<Workbench>().and_then(Workbench::active_document);
                document.and_then(|document| {
                    let edits = document.char_edits_since(at.1)?;
                    Some((
                        (path.clone(), *version, *folds, window, false),
                        moved_folds(listed, &edits, document.snapshot()),
                    ))
                })
            }
            _ => None,
        };
        if let Some((key, list)) = moved {
            self.folds = Some((key, Arc::new(list)));
        } else if let Some(wanted) = wanted.filter(|_| !fresh) {
            self.fold_listings += 1;
            let (from, to) = window;
            let listed: Vec<FoldShown> = registry
                .dispatch(
                    session,
                    "editor.fold.list",
                    json!({ "current": settled, "fromLine": from, "toLine": to, "enclosing": true }),
                )
                .ok()
                .and_then(|list| list.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(FoldShown::from_json)
                .collect();
            let key = session
                .state::<Workbench>()
                .and_then(Workbench::active_document)
                .map(|document| (wanted.0.clone(), document.version(), document.folds_version(), window, settled));
            self.folds = key.map(|key| (key, Arc::new(listed)));
        }
        let at_caret = zen_workbench::language::highlights::at_caret(session.settings());
        let at = session.state::<Workbench>().and_then(Workbench::active_document).map(|document| {
            (
                (path.clone(), document.version(), document.selection_version()),
                settled && (at_caret || !document.selections().primary().is_empty()),
            )
        });
        if let Some(at) = at {
            let pending = self.occurrences.as_ref().is_some_and(|(shown, pending, _)| *shown == at && *pending);
            if pending || self.occurrences.as_ref().map(|(shown, _, _)| shown) != Some(&at) {
                let (pending, bands) = if at.1 {
                    usage_bands(registry.dispatch(session, "editor.highlights.get", json!({})).ok(), &path)
                } else {
                    (false, Vec::new())
                };
                if pending {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
                }
                self.occurrences = Some((at, pending, bands));
            }
        }
        let (pinned_pending, pinned) =
            usage_bands(registry.dispatch(session, "editor.highlightUsages.list", json!({})).ok(), &path);
        if pinned_pending {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }
        let caret_settled = settled && now - self.last_activity >= SETTLE;
        if !caret_settled {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SETTLE));
        }
        let bulb = if self.passive {
            self.intentions = super::intentions_popup::IntentionsPopup::default();
            None
        } else {
            if let Some(outcome) = self.intentions.follow(ui, registry, session, caret_settled) {
                self.intention_outcome(registry, session, outcome);
            }
            if let Some(outcome) = self.generate.follow(ui, registry, session) {
                self.generate_outcome(registry, session, outcome);
            }
            if let Some(notice) = self.extract.follow(ui, registry, session) {
                self.notice = Some(notice);
            }
            self.intentions.bulb(session)
        };
        let mut occurrences: Vec<UsageBand> = self
            .occurrences
            .as_ref()
            .filter(|(((shown, _, _), _), _, _)| *shown == path)
            .map(|(_, _, bands)| bands.clone())
            .unwrap_or_default();
        occurrences.extend(pinned);
        let fold_list: Arc<Vec<FoldShown>> = self.folds.as_ref().map(|(_, list)| Arc::clone(list)).unwrap_or_default();
        if self.run_icons {
            self.follow_run_targets(registry, session, &path, settled);
        } else {
            self.run_targets = None;
            self.run_menu = None;
        }
        match version {
            Some(version) if self.run_icons => {
                let now = ui.input(|input| input.time);
                self.line_markers.follow(registry, session, &path, version, now);
            }
            _ => self.line_markers.clear(),
        }
        let line_markers = self.line_markers.shown(&path);
        if let Some(version) = version {
            self.swatches.follow(registry, session, &path, version, settled);
            self.todo_marks.follow(registry, session, &path, version, settled);
            let enabled = zen_workbench::line_numbers::method_separators_shown(session.settings());
            self.separators.follow(ui.ctx(), registry, session, &path, version, settled, enabled);
        }
        self.follow_debugger(ui, registry, session);
        self.inline_values.follow(ui.ctx(), registry, session, &path, self.stop_key.as_ref());
        let run_inlay = session.settings().get_or(zen_workbench::debug::RUN_TO_CURSOR_INLAY, true);
        self.follow_breakpoints(ui, registry, session, &path);
        self.follow_bookmarks(registry, session, &path);
        let bookmarks: Vec<BookmarkMark> = self
            .bookmarks
            .as_ref()
            .filter(|shown| shown.path == path)
            .map(|shown| shown.marks.clone())
            .unwrap_or_default();
        let execution_line = self
            .execution
            .as_ref()
            .and_then(|(_, stopped)| stopped.as_ref())
            .and_then(|(stopped, line)| (*stopped == path).then_some(*line));
        let band_lines =
            self.extras.as_ref().filter(|(shown, _)| *shown == path).and_then(|(_, extras)| extras.band).and_then(
                |(start, end)| {
                    let text = session.state::<Workbench>()?.active_document()?.snapshot().clone();
                    let first = text.offset_to_point(start.min(text.len_chars())).ok()?.line;
                    let last = text.offset_to_point(end.min(text.len_chars())).ok()?.line;
                    Some(first..=last)
                },
            );
        let flash_lines =
            self.extras.as_ref().filter(|(shown, _)| *shown == path).and_then(|(_, extras)| extras.flash).and_then(
                |(start, end, strength)| {
                    let text = session.state::<Workbench>()?.active_document()?.snapshot().clone();
                    let first = text.offset_to_point(start.min(text.len_chars())).ok()?.line;
                    let last = text.offset_to_point(end.min(text.len_chars())).ok()?.line;
                    Some((first..=last, strength))
                },
            );
        let breakpoints: Vec<BreakpointMark> = self
            .breakpoints
            .as_ref()
            .filter(|shown| shown.path == path)
            .map(|shown| shown.marks.clone())
            .unwrap_or_default();
        let run_targets: Arc<Vec<GutterTarget>> = self
            .run_targets
            .as_ref()
            .filter(|((shown, _), _)| *shown == path)
            .map(|(_, list)| Arc::clone(list))
            .unwrap_or_default();
        if !run_targets.is_empty() {
            self.follow_test_runs(registry, session);
        }
        if self.run_menu.is_some() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.run_menu = None;
        }
        let since = self.semantic.as_ref().filter(|(shown, _, _)| *shown == path).map(|(_, generation, _)| *generation);
        if let Ok(answer) =
            registry.dispatch(session, "editor.semantic.tokens", json!({ "path": path, "since": since }))
            && let Some(tokens) = answer["tokens"].as_array()
        {
            let web = is_web_script(&path);
            let kept: Vec<(usize, usize, SemanticKind)> = tokens
                .iter()
                .filter_map(|token| {
                    let kind = if web { web_semantic_kind(token) } else { semantic_kind(token) };
                    Some((token["start"].as_u64()? as usize, token["end"].as_u64()? as usize, kind?))
                })
                .collect();
            self.semantic = Some((path.clone(), answer["generation"].as_u64().unwrap_or(0), Arc::new(kept)));
        }
        let semantic_tokens: Arc<Vec<SemanticSpan>> = self
            .semantic
            .as_ref()
            .filter(|(shown, _, _)| *shown == path)
            .map(|(_, _, tokens)| Arc::clone(tokens))
            .unwrap_or_default();
        let hints = self.inlay_hints.follow(ui, registry, session, &path, self.shown_lines);
        let hints = self.with_extras(&path, hints);
        let hints = self.replace_previews(registry, session, hints);
        let link = self
            .peek
            .as_ref()
            .filter(|peek| {
                peek.path == path && peek.state.as_ref().is_some_and(|state| state["declaration"].is_string())
            })
            .map(|peek| (peek.start, peek.end));

        let version = session.state::<Workbench>().and_then(Workbench::active_document).map_or(0, Document::version);
        let stale = self.blame.as_ref().is_none_or(|(shown, at, read, info)| {
            *shown != path || *at != version || (info.pending && now - read > 0.2) || now - read > 2.0
        });
        if stale {
            let info = registry
                .dispatch(session, "vcs.blame.get", json!({ "path": path }))
                .ok()
                .and_then(|info| serde_json::from_value::<zen_workbench::vcs::log::BlameInfo>(info).ok())
                .unwrap_or_default();
            if info.pending {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
            self.blame = Some((path.clone(), version, now, info));
        }
        let annotated = self.blame.as_ref().is_some_and(|(_, _, _, info)| info.shown);
        if !self.passive {
            if let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) {
                self.code_vision.follow(document);
            }
            self.code_vision.refresh(ui, registry, session, &path, settled, self.shown_lines);
        }
        self.changes = registry
            .dispatch(session, "vcs.lineStatus.list", json!({ "path": path }))
            .ok()
            .and_then(|info| serde_json::from_value::<zen_workbench::vcs::commit::LineStatusInfo>(info).ok())
            .map(|info| info.changes)
            .unwrap_or_default();
        self.follow_coverage(registry, session, &path);
        let workbench = session.state::<Workbench>()?;
        let find = workbench.find().clone();
        let document = workbench.active_document()?;
        let popup_highlighter = highlighter.clone();
        let tab_size = zen_workbench::project_settings::code_style_for_path(session, &path).tab_size;
        let aids = zen_workbench::visual_aids::shown(session, document);
        let editor = EditorMetrics::new(ui.ctx(), &self.font, tab_size);
        let highlighters = session.state::<zen_workbench::brackets::Highlighters>();
        self.refresh(ui, document, highlighter, highlighters, palette, &editor);
        if !find.open || find.query.text.is_empty() {
            self.matches = None;
        } else if self.matches.as_ref().is_none_or(|(version, query, scope, _)| {
            *version != document.version() || *query != find.query || *scope != document.find_scope()
        }) {
            let found = zen_workbench::find::find_matches(highlighters, document, &find.query).unwrap_or_default();
            self.matches = Some((document.version(), find.query.clone(), document.find_scope(), found));
        }
        let matches: &[std::ops::Range<usize>] = self.matches.as_ref().map_or(&[], |(_, _, _, found)| found.as_slice());
        let matches = matches.to_vec();
        let excluded = document.find_excluded().to_vec();
        self.unwrap.follow(document.version());
        let mut kept = self.unwrap.kept().to_vec();
        kept.extend(self.extract.highlighted());
        let step_target = self.step_target.as_ref().filter(|(shown, _)| *shown == path).map(|(_, range)| range.clone());
        self.drawn_step_target = None;
        let owned = self.highlight.take()?;
        let cache = &owned;
        let text = document.text();
        let snapshot = document.snapshot();
        let bracket_marks = self.brackets_at(document, cache);
        let lines = cache.line_bytes.len() - 1;
        self.line_numbers_on = zen_workbench::line_numbers::is_shown(session.settings(), document);
        let numbers_shown = self.line_numbers_on && !self.hide_line_numbers;
        let numbered = if numbers_shown { lines } else { 0 };
        let separator_lines = self.separators.lines(&path, document.version());
        let point = |offset: usize| snapshot.offset_to_point(offset).unwrap_or_default();
        let mut folded_at: HashMap<usize, (usize, usize, usize, FoldMark, usize)> = HashMap::new();
        let mut hidden_until = 0;
        for fold in fold_list.iter().filter(|fold| fold.folded) {
            if fold.start < hidden_until {
                continue;
            }
            let (start, end) = (point(fold.start), point(fold.end));
            if fold.end > fold.start && !folded_at.contains_key(&start.line) {
                let mark = match fold.kind.as_str() {
                    "imports" => FoldMark::Imports,
                    "region" => FoldMark::Region,
                    "manual" => FoldMark::Manual,
                    "comment"
                        if snapshot
                            .slice(fold.start..fold.start + 2)
                            .is_ok_and(|head| head == "//" || head == "/*") =>
                    {
                        FoldMark::Comment
                    }
                    _ => FoldMark::Block,
                };
                folded_at.insert(start.line, (start.column, end.line, end.column, mark, fold.start));
                hidden_until = fold.end;
            }
        }
        let mut rows: Vec<usize> = Vec::with_capacity(lines);
        let mut line = 0;
        while line < lines {
            rows.push(line);
            line = folded_at.get(&line).map_or(line + 1, |(_, end_line, _, _, _)| end_line + 1);
        }
        let wrapping = zen_workbench::soft_wrap::is_soft_wrapped(session.settings(), document);
        let click_humps = zen_workbench::editing::camel_humps_on_double_click(session.settings());
        let font = editor.font.clone();
        let char_width = editor.char_width;
        let line_height = editor.line_height;
        let tab = editor.tab_size;
        self.line_height = line_height;

        let area = ui.available_rect_before_wrap();
        let gutter_width = gutter_width(numbered, char_width, annotated);
        let gutter = Rect::from_min_max(area.min, pos2(area.left() + gutter_width, area.bottom()));
        let text_area = Rect::from_min_max(pos2(gutter.right(), area.top()), area.max);
        let whole_in_view = ui.clip_rect().expand(1.0).contains_rect(text_area);
        let ctx = ui.ctx().clone();
        let wrap_metrics = wrapping.then(|| soft_wrap::Metrics {
            ctx: &ctx,
            font: &font,
            tab,
            width: soft_wrap::Metrics::wrap_width(text_area.width() - TEXT_LEFT, char_width),
            char_width,
        });
        let mut rows = match wrap_metrics {
            Some(metrics) => {
                self.soft_wrap.follow(&path, document.version(), &cache.line_bytes, metrics);
                self.soft_wrap.rows(rows, |line| folded_at.contains_key(&line))
            }
            None => ShownRows::new(rows),
        };
        let row_of = |line: usize| rows.partition_point(|&shown| shown <= line).saturating_sub(1);
        let layout = RowLayout::new(line_height, &rows, self.code_vision.lines(&path));
        ui.allocate_rect(area, Sense::hover());
        if palette.editor_background != palette.island {
            let radius = if self.square_bottom { 0 } else { metrics::ISLAND_RADIUS };
            let corners = CornerRadius { sw: radius, se: radius, ..CornerRadius::ZERO };
            ui.painter().rect_filled(area, corners, palette.editor_background);
        }

        let primary = document.selections().primary();
        let column_mode = document.column_mode();
        let primary_point = snapshot.offset_to_point(primary.head).unwrap_or_default();
        let carets: Vec<usize> = document.selections().iter().map(|selection| selection.head).collect();
        let mut intents: Vec<Intent> = Vec::new();
        let mut selected_band: Option<Rect> = None;
        let mut first_visible = 0;
        let mut visible_lines = 0;
        let mut scrolled_to = 0.0;
        let mut page_rows = None;
        let mut shown_lines = self.shown_lines;
        let caret_on = focused && (((now - self.last_activity) / BLINK) as u64).is_multiple_of(2);
        if focused {
            let next = BLINK - (now - self.last_activity) % BLINK;
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(next.max(0.01)));
        }

        let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_area));
        let hint_font = inlay_hints::font(&font);
        let hint_widths = self.inlay_hints.widths(ui, &hints, &hint_font);
        let content_width = if wrapping {
            0.0
        } else {
            let hinted_width = self.inlay_hints.widest(
                &hints,
                &hint_widths,
                document.version(),
                char_width,
                |offset| point(offset).line,
                |line| display_line(line_text(text, cache, line), tab).1.last().copied().unwrap_or_default(),
            );
            (cache.max_columns as f32 * char_width).max(hinted_width) + TEXT_LEFT + 80.0
        };
        let caret_row = row_of(primary_point.line);
        let mut fold_marks: Vec<(Rect, usize)> = Vec::new();
        let mut tail_shifts: HashMap<usize, (usize, f32, f32)> = HashMap::new();
        let mark_text = |kind: FoldMark, start: usize| {
            let placeholder =
                fold_list.iter().find(|fold| fold.start == start).and_then(|fold| fold.placeholder.clone());
            match (placeholder, kind) {
                (Some(placeholder), _) => placeholder,
                (None, FoldMark::Comment) => "/.../".to_owned(),
                (None, _) => "...".to_owned(),
            }
        };
        let mut peeked = None;
        let mut fold_preview: Option<(Rect, String)> = None;
        let mut hint_boxes: Vec<(Rect, u64)> = Vec::new();
        let gutter_wheel =
            if ui.rect_contains_pointer(gutter) { ui.input(|input| input.smooth_scroll_delta) } else { Vec2::ZERO };
        let folded = |line| folded_at.contains_key(&line);
        let text_of = |line| line_text(text, cache, line);
        let view_rows = (text_area.height() / line_height).ceil() as usize + 1;
        let todo_marks = self.todo_marks.of(&path);
        let mut scroll = egui::ScrollArea::both().id_salt(("editor", path.clone())).auto_shrink([false, false]);
        if let Some(metrics) = wrap_metrics
            && let Some(offset) = self.soft_wrap.anchored(metrics, &path, &mut rows, view_rows, (folded, text_of))
        {
            scroll = scroll.vertical_scroll_offset(TOP + layout.y_of_visual(&rows, offset));
        }
        let visual_row_of = |line: usize| {
            let row = rows.partition_point(|&shown| shown <= line).saturating_sub(1);
            (layout.top(&rows, row) / line_height).round() as usize
        };
        if let Some(offset) = self.sticky.take_scroll(visual_row_of, line_height) {
            scroll = scroll.vertical_scroll_offset(TOP + offset);
            self.reveal = false;
        }
        if let Some(line) = self.scroll_line.take() {
            let row = rows.partition_point(|&shown| shown <= line).saturating_sub(1);
            scroll = scroll.vertical_scroll_offset(TOP + layout.top(&rows, row));
            self.reveal = false;
        }
        let mut sticky_text_x = 0.0;
        self.caret_moving = false;
        self.caret_rect = None;
        self.caret_text = Some(text_area);
        let scrolled = scroll.show_viewport(
            &mut text_ui,
            |ui, viewport| {
                if gutter_wheel.y != 0.0 {
                    ui.scroll_with_delta_animation(vec2(0.0, gutter_wheel.y), egui::style::ScrollAnimation::none());
                }
                let first_row = layout.visual_at(&rows, viewport.min.y - TOP).floor().max(0.0) as usize;
                let last_row = layout.visual_at(&rows, viewport.max.y - TOP).ceil() as usize;
                if let Some(metrics) = wrap_metrics {
                    self.soft_wrap.fit(metrics, &mut rows, first_row..last_row + 1, folded, text_of);
                    page_rows = Some(last_row.saturating_sub(first_row));
                }
                let content_height = layout.height(&rows, rows.len()) + TOP + line_height;
                let origin = ui.min_rect().min;
                let size = vec2(content_width.max(viewport.width()), content_height);
                let (_, content) = ui.allocate_space(size);
                let response = ui.interact(content, id, Sense::click_and_drag());
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
                let has_tools = !super::external_tools::menu(&tools).is_empty();
                let shown = |slot: &str| slot != super::customization::TOOLS || has_tools;
                let customized = super::customization::shows_anything(super::customization::EDITOR_POPUP, shown);
                if customized || edit_starters {
                    response.context_menu(|ui| {
                        if edit_starters && ui.button(zen_i18n::t!("ide-starters-edit")).clicked() {
                            self.starters_request = Some(path.clone());
                            ui.close();
                        }
                        if edit_starters && customized {
                            ui.separator();
                        }
                        super::customization::popup(ui, super::customization::EDITOR_POPUP, shown, |ui, _| {
                            if let Some(chosen) = super::external_tools::context_submenus(ui, &tools) {
                                self.external_tool_request = Some(chosen);
                            }
                        });
                    });
                }
                first_visible = rows.row_at(first_row).min(rows.len());
                let last_visible = (rows.row_at(last_row) + 1).min(rows.len());
                visible_lines = last_visible.saturating_sub(first_visible);
                scrolled_to = viewport.min.y;
                let first_line = rows.get(first_visible).copied().unwrap_or_default();
                let last_line = rows[..last_visible.max(1).min(rows.len())]
                    .last()
                    .map(|&line| folded_at.get(&line).map_or(line, |(_, end_line, ..)| *end_line))
                    .unwrap_or_default();
                shown_lines = (first_line, last_line);
                let mut semantic = visible_semantic(snapshot, &semantic_tokens, link, first_line..last_line + 1);
                semantic.extend(bracket_marks.iter().map(|&(at, kind)| (at, at + 1, kind)));
                let shown_chars = snapshot.point_to_offset(zen_text::Point::new(first_line, 0)).unwrap_or_default()
                    ..snapshot.point_to_offset(zen_text::Point::new(last_line + 1, 0)).unwrap_or(usize::MAX);
                semantic.extend(
                    todo_marks
                        .iter()
                        .filter(|mark| mark.start < shown_chars.end && mark.end > shown_chars.start)
                        .map(|mark| (mark.start, mark.end, SemanticKind::Todo(mark.color))),
                );
                semantic.sort_by_key(|(token_start, _, _)| *token_start);
                let semantic = with_unused(
                    semantic,
                    problems.iter().filter(|problem| problem.unused).map(|problem| (problem.start, problem.end)),
                );
                let text_x = origin.x + TEXT_LEFT;
                sticky_text_x = text_x;
                let line_top = |row: usize| origin.y + TOP + layout.top(&rows, row);
                let painter = ui.painter();
                let mut wraps: HashMap<usize, Arc<soft_wrap::Measured>> = HashMap::new();
                let drawn_before_wrap =
                    if wrapping { self.soft_wrap.drawn_before_wrap(&carets) } else { HashSet::new() };
                let hinted_line = |line: usize| -> (usize, String, Vec<usize>, Vec<Placed>) {
                    let line_start = snapshot.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or_default();
                    let plain = display_line(line_text(text, cache, line), tab);
                    let on_line = inlay_hints::on_line(&hints, line_start, plain.1.len() - 1);
                    let (display, columns, placed) =
                        inlay_hints::place(plain, &hints[on_line.clone()], &hint_widths[on_line], line_start);
                    (line_start, display, columns, placed)
                };
                let viewport_ys = (origin.y + viewport.min.y)..=(origin.y + viewport.max.y);
                let guide_x = |column: u16| (text_x + f32::from(column) * char_width).round() + 0.5;
                for &column in &aids.guides {
                    painter.vline(guide_x(column), viewport_ys.clone(), Stroke::new(1.0, palette.right_margin));
                }
                let indent_guides = if aids.indent_guides {
                    let drawn = (first_visible..last_visible.max(first_visible)).map(|row| rows[row]);
                    visual_aids::guides_of(drawn, lines, text_of, tab)
                } else {
                    HashMap::new()
                };
                let selected_guide = aids.indent_guides.then(|| {
                    let at = (document.version(), primary_point.line, lines);
                    self.caret_block.find(at, tab, text_of)
                }).flatten();
                let selected_ranges: Vec<std::ops::Range<usize>> = document
                    .selections()
                    .iter()
                    .filter(|selection| !selection.is_empty())
                    .map(|selection| selection.range())
                    .collect();
                let template_boxes: Vec<(usize, usize, bool)> = document
                    .template_session()
                    .map(|template| template.ranges().map(|(range, current)| (range.start, range.end, current)).collect())
                    .unwrap_or_default();

                let inline_lines = self.inline_values.lines();
                let shown_rows = &rows;
                let visual_rows = (first_visible..last_visible.max(first_visible))
                    .flat_map(|row| (0..shown_rows.height(row)).map(move |sub| (row, shown_rows[row], sub)));
                for (row_index, line, sub) in visual_rows {
                    let (line_start, display, columns, placed) = hinted_line(line);
                    let line_len = columns.len() - 1;
                    let line_end = line_start + line_len;
                    let single = zen_editor::LineWrap::single(line_len);
                    if let Some(metrics) = wrap_metrics.filter(|_| sub == 0 && !folded_at.contains_key(&line)) {
                        wraps.insert(line, self.soft_wrap.measure(metrics, line_text(text, cache, line)));
                    }
                    let wrap = wraps.get(&line).map_or(&single, |measured| &measured.wrap);
                    let line_rows = wrap.rows();
                    let has_caret = carets.iter().any(|&caret| (line_start..=line_end).contains(&caret));
                    if sub == 0 && layout.is_marked(row_index) {
                        let top = line_top(row_index);
                        let indent = line_text(text, cache, line).chars().take_while(|c| c.is_whitespace()).count();
                        let plain_columns = display_line(line_text(text, cache, line), tab).1;
                        let lens_left = text_x + plain_columns[indent.min(line_len)] as f32 * char_width;
                        let lens_row = Rect::from_min_max(
                            pos2(lens_left, top - line_height),
                            pos2(content.left() + content.width().max(viewport.width()), top),
                        );
                        match self.code_vision.paint(ui, id, line, lens_row, &font, palette) {
                            Some(Click::Usages(offset)) => {
                                if let Some(at) = ui.input(|input| input.pointer.interact_pos()) {
                                    self.usages.open_at(at + vec2(0.0, line_height));
                                }
                                intents.push(Intent("navigate.usages.show", json!({ "path": path, "offset": offset })));
                            }
                            Some(Click::Authors(commit)) => self.blame_request = Some(commit),
                            None => {}
                        }
                    }
                    if sub == 0 {
                        let row = Rect::from_min_size(
                            pos2(content.left(), line_top(row_index)),
                            vec2(content.width().max(viewport.width()), line_height * line_rows as f32),
                        );
                        let band = if execution_line == Some(line) {
                            Some(palette.execution_line)
                        } else if has_caret && document.selections().iter().all(|selection| selection.is_empty()) {
                            Some(palette.current_line)
                        } else if band_lines.as_ref().is_some_and(|lines| lines.contains(&line)) {
                            Some(palette.current_line.gamma_multiply(0.45))
                        } else {
                            None
                        };
                        if let Some(band) = band {
                            painter.rect_filled(row, 0, band);
                            for &column in &aids.guides {
                                painter.vline(guide_x(column), row.y_range(), Stroke::new(1.0, palette.right_margin));
                            }
                        }
                        if let Some((_, strength)) = flash_lines.as_ref().filter(|(lines, _)| lines.contains(&line)) {
                            painter.rect_filled(row, 0, palette.search_highlight.gamma_multiply(0.6 * strength));
                        }
                    }
                    let segment = wrap.row_range(sub);
                    let last = sub + 1 == line_rows;
                    let galley = if line_rows == 1 {
                        let pieces = (display.as_str(), columns.as_slice(), placed.as_slice());
                        self.galley(ui, text, cache, line, pieces, palette, &font, (line_start, &semantic))
                    } else {
                        let pieces = (display.as_str(), columns.as_slice(), placed.as_slice());
                        let semantic = (line_start, semantic.as_slice());
                        self.galley_range(ui, text, cache, line, pieces, palette, &font, semantic, segment.clone())
                    };
                    let top = line_top(row_index) + sub as f32 * line_height;
                    let left = text_x + wrap.indent_of(sub);
                    let (row_start, row_end) = (line_start + segment.start, line_start + segment.end);
                    let raw_x = |column: usize| {
                        let column = column.clamp(segment.start, segment.end);
                        left + galley.pos_from_cursor(CCursor::new(columns[column] - columns[segment.start])).min.x
                    };
                    let inline_fold = folded_at.get(&line).filter(|(_, end_line, ..)| *end_line == line).map(
                        |&(column, _, end_column, kind, start)| {
                            let width = painter.layout_no_wrap(mark_text(kind, start), font.clone(), palette.fold_text).size().x;
                            let mark_right = raw_x(column) + if kind == FoldMark::Block { 2.0 } else { 0.0 } + width + 8.0;
                            (column, end_column, mark_right + 1.0 - raw_x(end_column), mark_right)
                        },
                    );
                    if let Some((_, end_column, shift, mark_right)) = inline_fold {
                        tail_shifts.insert(line, (end_column, shift, mark_right));
                    }
                    let x_of = |column: usize| match inline_fold {
                        Some((_, to, shift, _)) if column >= to => raw_x(column) + shift,
                        Some((from, ..)) if column > from => raw_x(from),
                        _ => raw_x(column),
                    };
                    let shown_to = if last { display.chars().count() } else { columns[segment.end] };
                    let row_placed = inlay_hints::on_row(&placed, columns[segment.start]..shown_to);
                    let x_from = |offset: usize| {
                        if offset < row_start || placed.is_empty() {
                            return x_of(offset.max(row_start) - line_start);
                        }
                        let column = (offset - line_start).min(segment.end);
                        let display = inlay_hints::after(&columns, &placed, column) - columns[segment.start];
                        left + galley.pos_from_cursor(CCursor::new(display)).min.x
                    };
                    let first = matches.partition_point(|range| range.end <= row_start);
                    for range in
                        matches[first..].iter().take_while(|range| range.start < row_end || (last && range.start <= row_end))
                    {
                        let to = range.end.min(row_end) - line_start;
                        let band = Rect::from_min_max(pos2(x_from(range.start), top + 2.0), pos2(x_of(to), top + line_height - 2.0));
                        if excluded.contains(range) {
                            painter.hline(band.x_range(), band.center().y, Stroke::new(1.0, palette.text_secondary));
                        } else {
                            painter.rect_filled(band, CornerRadius::same(2), palette.search);
                        }
                    }
                    for &UsageBand { start, end, write } in &occurrences {
                        if end <= row_start || start > row_end || (!last && start >= row_end) {
                            continue;
                        }
                        let to = end.min(row_end) - line_start;
                        let band = Rect::from_min_max(pos2(x_from(start), top), pos2(x_of(to), top + line_height));
                        let fill = if write { palette.symbol_highlight_write } else { palette.symbol_highlight };
                        painter.rect_filled(band, CornerRadius::ZERO, fill);
                    }
                    for range in &kept {
                        if range.end <= row_start || range.start > row_end || (!last && range.start >= row_end) {
                            continue;
                        }
                        let to = range.end.min(row_end) - line_start;
                        let band = Rect::from_min_max(pos2(x_from(range.start), top), pos2(x_of(to), top + line_height));
                        painter.rect_filled(band, CornerRadius::ZERO, palette.search);
                    }
                    if let Some(range) = step_target.as_ref().filter(|range| range.start < row_end && range.end > row_start) {
                        let to = range.end.min(row_end) - line_start;
                        let band = Rect::from_min_max(pos2(x_from(range.start), top + 1.0), pos2(x_of(to), top + line_height - 1.0));
                        painter.rect_filled(band, CornerRadius::same(2), palette.accent.gamma_multiply(0.18));
                        painter.rect_stroke(band, CornerRadius::same(2), Stroke::new(1.0, palette.accent), StrokeKind::Inside);
                        self.drawn_step_target = Some(band);
                    }
                    for &(at, kind) in bracket_marks.iter().filter(|(at, _)| (row_start..row_end).contains(at)) {
                        let band = Rect::from_min_max(
                            pos2(x_from(at), top),
                            pos2(x_of(at + 1 - line_start), top + line_height),
                        );
                        let fill = if kind == SemanticKind::MatchedBracket {
                            palette.bracket_matched_background
                        } else {
                            palette.bracket_unmatched_background
                        };
                        painter.rect_filled(band, CornerRadius::ZERO, fill);
                    }
                    for selection in document.selections().iter().filter(|selection| !selection.is_empty()) {
                        let range = selection.range();
                        if range.end < row_start
                            || range.start > row_end
                            || (range.end == row_start && (sub > 0 || line_len > 0))
                            || (!last && range.start == row_end)
                        {
                            continue;
                        }
                        let to = range.end.min(row_end) - line_start;
                        let mut right = x_of(to);
                        if range.end > row_end {
                            right += char_width * 0.6;
                        }
                        let band = Rect::from_min_max(pos2(x_from(range.start), top), pos2(right, top + line_height));
                        painter.rect_filled(band, CornerRadius::same(2), palette.editor_selection);
                        if *selection == primary && selected_band.is_none() {
                            selected_band = Some(band);
                        }
                    }
                    for &(start, end, current) in &template_boxes {
                        if end < row_start || start > row_end || (!last && start >= row_end) {
                            continue;
                        }
                        let left = x_from(start.max(row_start));
                        let right = x_of(end.min(row_end) - line_start).max(left + char_width * 0.5);
                        let frame = Rect::from_min_max(pos2(left - 1.0, top + 1.0), pos2(right + 1.0, top + line_height - 1.0));
                        let color = if current { palette.accent } else { palette.button_border };
                        painter.rect_stroke(frame, CornerRadius::same(2), Stroke::new(1.0, color), StrokeKind::Inside);
                    }
                    if let Some(columns) = indent_guides.get(&line) {
                        let reach = if sub == 0 && layout.is_marked(row_index) { line_height } else { 0.0 };
                        for &column in columns {
                            let selected = selected_guide.is_some_and(|guide| guide.is(line, column));
                            let color = if selected { palette.indent_guide_selected } else { palette.indent_guide };
                            let x = (text_x + column as f32 * char_width).round() + 0.5;
                            let stroke = Stroke::new(visual_aids::GUIDE_WIDTH, color);
                            painter.vline(x, (top - reach)..=(top + line_height), stroke);
                        }
                    }
                    if let Some(kinds) = aids.whitespaces {
                        let line_chars = line_text(text, cache, line);
                        let shape = visual_aids::LineShape::of(line_chars);
                        let shown_to = folded_at.get(&line).map_or(segment.end, |(column, ..)| (*column).min(segment.end));
                        let middle = top + line_height / 2.0;
                        for (index, c) in line_chars.chars().enumerate().take(shown_to).skip(segment.start) {
                            if c != ' ' && c != '\t' {
                                continue;
                            }
                            let offset = line_start + index;
                            let selected = kinds.selection && selected_ranges.iter().any(|range| range.contains(&offset));
                            if !shape.shows(index, kinds, selected) {
                                continue;
                            }
                            let cell = (x_of(index), x_of(index + 1));
                            if c == ' ' {
                                visual_aids::space(painter, cell, middle, char_width, palette.whitespace);
                            } else {
                                visual_aids::tab(painter, cell, middle, char_width, aids.tab_rendering, palette.whitespace);
                            }
                        }
                    }
                    match folded_at.get(&line) {
                        None => {
                            painter.galley(pos2(left, top), Arc::clone(&galley), palette.code);
                            underlines(painter, pos2(left, top), &galley);
                            let boxes = inlay_hints::boxes(&galley, left, top, line_height, &row_placed);
                            inlay_hints::paint(painter, &boxes, &row_placed, &hint_font, palette);
                            hint_boxes.extend(boxes.into_iter().zip(row_placed.iter().map(|place| place.hint.id)));
                            if last && let Some(values) = inline_lines.get(&line) {
                                let at = pos2(left + galley.size().x + char_width * 2.0, top);
                                self.inline_values.paint(painter, at, line, values, &font, palette);
                            }
                        }
                        Some(&(column, end_line, end_column, kind, start)) => {
                            let cut = x_of(column);
                            let clip = Rect::from_min_max(pos2(text_x - 4.0, top), pos2(cut, top + line_height));
                            painter.with_clip_rect(clip).galley(pos2(text_x, top), Arc::clone(&galley), palette.code);
                            underlines(&painter.with_clip_rect(clip), pos2(text_x, top), &galley);
                            let boxes = inlay_hints::boxes(&galley, text_x, top, line_height, &placed);
                            let before = boxes.iter().take_while(|rect| rect.right() <= cut).count();
                            inlay_hints::paint(painter, &boxes[..before], &placed[..before], &hint_font, palette);
                            let mark_galley = painter.layout_no_wrap(mark_text(kind, start), font.clone(), palette.fold_text);
                            let mark = Rect::from_min_size(
                                pos2(cut + if kind == FoldMark::Block { 2.0 } else { 0.0 }, top + 3.0),
                                vec2(mark_galley.size().x + 8.0, line_height - 6.0),
                            );
                            painter.rect_filled(mark, 3, palette.fold_mark);
                            painter.galley(pos2(mark.left() + 4.0, top + (line_height - mark_galley.size().y) / 2.0), mark_galley, palette.fold_text);
                            fold_marks.push((mark, start));
                            let tail: String = line_text(text, cache, end_line).chars().skip(end_column).collect();
                            if !tail.is_empty() {
                                let format = TextFormat {
                                    line_height: Some(line_height),
                                    ..TextFormat::simple(font.clone(), palette.code)
                                };
                                let tail = painter.layout_job(LayoutJob::single_section(display_line(&tail, tab).0, format));
                                painter.galley(pos2(mark.right() + 1.0, top), tail, palette.code);
                            }
                        }
                    }
                    for problem in problems.iter().filter(|problem| {
                        problem.start <= row_end
                            && problem.end >= row_start
                            && (last || problem.start < row_end)
                            && (sub == 0 || problem.end > row_start || problem.start >= row_start)
                    }) {
                        let to = problem.end.min(row_end) - line_start;
                        let (left, mut right) = (x_from(problem.start), x_of(to));
                        if right - left < char_width {
                            right = left + char_width;
                        }
                        match problem.mark() {
                            ProblemMark::StrikeOut => {
                                painter.hline(left..=right, top + line_height / 2.0 + 0.5, Stroke::new(1.0, palette.text));
                            }
                            ProblemMark::Wavy => {
                                wavy_line(painter, left, right, top + line_height - 3.0, palette.problem(&problem.severity));
                            }
                            ProblemMark::None => {}
                        }
                    }
                    for &caret in &carets {
                        let before_wrap = drawn_before_wrap.contains(&caret);
                        if !(row_start..=row_end).contains(&caret)
                            || (!last && caret == row_end && !before_wrap)
                            || (sub > 0 && caret == row_start && before_wrap)
                        {
                            continue;
                        }
                        let x = x_of(caret - line_start);
                        if caret == primary.head && !self.preedit.is_empty() {
                            let preedit = painter.layout_no_wrap(self.preedit.clone(), font.clone(), palette.text);
                            let at = pos2(x, top + (line_height - preedit.size().y) / 2.0);
                            let width = preedit.size().x;
                            painter.rect_filled(Rect::from_min_size(at, preedit.size()), 0, palette.editor_background);
                            painter.galley(at, preedit, palette.text);
                            painter.hline(
                                x..=x + width,
                                at.y + line_height - 6.0,
                                Stroke::new(1.0, palette.text),
                            );
                        } else if caret_on {
                            let bar = Rect::from_min_size(
                                pos2(x - CARET_WIDTH / 2.0, top + 3.0),
                                vec2(CARET_WIDTH, line_height - 6.0),
                            );
                            painter.rect_filled(bar, 0, palette.caret);
                        }
                        if caret == primary.head {
                            let cursor = Rect::from_min_size(pos2(x, top), vec2(1.0, line_height));
                            self.caret_rect = Some(cursor);
                            if self.reveal && whole_in_view {
                                ui.scroll_to_rect(cursor.expand2(vec2(char_width * 4.0, line_height)), None);
                            }
                            if focused {
                                ui.ctx().output_mut(|output| {
                                    output.ime = Some(egui::output::IMEOutput {
                                        purpose: egui::IMEPurpose::Normal,
                                        rect: text_area,
                                        cursor_rect: cursor,
                                        should_interrupt_composition: false,
                                    });
                                });
                            }
                        }
                    }
                }
                if !separator_lines.is_empty() {
                    let stroke = Stroke::new(1.0, palette.method_separator);
                    let across = origin.x + viewport.min.x..=origin.x + viewport.max.x;
                    for row in first_visible..last_visible {
                        if separator_lines.binary_search(&rows[row]).is_ok() {
                            let lens = if layout.is_marked(row) { line_height } else { 0.0 };
                            painter.hline(across.clone(), line_top(row) - lens + 0.5, stroke);
                        }
                    }
                }
                if self.reveal && self.caret_moving_frames >= MAX_REVEAL_FRAMES {
                    self.reveal = false;
                }
                if self.reveal && whole_in_view && !(first_visible..last_visible).contains(&caret_row) {
                    self.caret_moving = true;
                    self.caret_moving_frames += 1;
                    ui.ctx().request_repaint();
                    let top = line_top(caret_row);
                    ui.scroll_to_rect(
                        Rect::from_min_size(pos2(text_x, top), vec2(1.0, line_height))
                            .expand2(vec2(0.0, line_height)),
                        None,
                    );
                } else if whole_in_view {
                    self.reveal = false;
                    self.caret_moving_frames = 0;
                }

                let pointed = |pos: egui::Pos2| -> soft_wrap::PointedRow {
                    let visual = layout.visual_at(&rows, pos.y - origin.y - TOP).floor().max(0.0) as usize;
                    let visual = visual.min(rows.total().saturating_sub(1));
                    let row = rows.row_at(visual).min(rows.len().saturating_sub(1));
                    let line = rows.get(row).copied().unwrap_or_default();
                    let (_, display, columns, placed) = hinted_line(line);
                    let single = zen_editor::LineWrap::single(columns.len() - 1);
                    let wrap = wraps.get(&line).map_or(&single, |measured| &measured.wrap);
                    let sub = visual.saturating_sub(rows.top(row)).min(wrap.rows() - 1);
                    let segment = wrap.row_range(sub);
                    let last = sub + 1 == wrap.rows();
                    let from = columns[segment.start];
                    let to = if last { display.chars().count() } else { columns[segment.end] };
                    let row_placed = inlay_hints::on_row(&placed, from..to);
                    let shown: String =
                        if wrap.rows() == 1 { display } else { display.chars().skip(from).take(to - from).collect() };
                    let galley = inlay_hints::plain_galley(ui, shown, &row_placed, &font);
                    soft_wrap::PointedRow {
                        line,
                        columns,
                        segment,
                        galley,
                        left: text_x + wrap.indent_of(sub),
                        top: origin.y + TOP + layout.top(&rows, row) + sub as f32 * line_height,
                        wrapped: !last,
                        hint_columns: placed.iter().map(|place| place.column).collect(),
                    }
                };
                let locate = |pos: egui::Pos2| -> (usize, usize, bool) {
                    let at = pointed(pos);
                    if let Some(&(from, shift, mark_right)) = tail_shifts.get(&at.line)
                        && pos.x >= mark_right
                    {
                        let (column, before_wrap) = at.column_at(pos.x - shift);
                        return (at.line, column.max(from), before_wrap);
                    }
                    let (column, before_wrap) = at.column_at(pos.x);
                    (at.line, column, before_wrap)
                };
                let point_at = |pos: egui::Pos2| -> (usize, usize) {
                    let (line, column, _) = locate(pos);
                    (line, column)
                };
                let offset_at = |pos: egui::Pos2| -> usize {
                    let (line, column) = point_at(pos);
                    snapshot.point_to_offset(zen_text::Point::new(line, column)).unwrap_or_default()
                };
                if let Some(pos) = response.hover_pos()
                    && let Some(&(_, id)) = hint_boxes.iter().find(|(rect, _)| rect.contains(pos))
                    && let Some(hint) = hints.iter().find(|hint| hint.id == id)
                {
                    self.inlay_hints.hover(hint);
                    if let Some(tooltip) = &hint.tooltip {
                        egui::Tooltip::for_widget(&response).at_pointer().show(|ui| {
                            ui.set_max_width(520.0);
                            ui.label(tooltip);
                        });
                    }
                }
                self.inlay_hints.set_drawn(std::mem::take(&mut hint_boxes));
                let mut hovered_problems = Vec::new();
                let mut hovered_bottom = 0.0;
                if let Some(pos) = response.hover_pos() {
                    let at = pointed(pos);
                    let line_start = snapshot.point_to_offset(zen_text::Point::new(at.line, 0)).unwrap_or_default();
                    let spans: Vec<(usize, usize)> = problems.iter().map(|problem| (problem.start, problem.end)).collect();
                    let over = problems_under(&at, line_start, &spans, (char_width, line_height), pos);
                    hovered_problems = over.into_iter().map(|index| problems[index].raw.clone()).collect();
                    hovered_bottom = at.top + line_height;
                }
                self.problem_popups.hover(ui, &path, hovered_problems, response.hover_pos(), hovered_bottom);
                if let Some(pos) = response.hover_pos().filter(|_| ui.input(|input| input.modifiers.command)) {
                    let (start, end) = unit_at(snapshot, offset_at(pos), 2, false);
                    let at = pointed(pos);
                    let line = at.line;
                    let line_start = snapshot.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or_default();
                    let x_of = |offset: usize, from: bool| {
                        let column = offset.saturating_sub(line_start);
                        if from { at.x_after(column) } else { at.x_of(column) }
                    };
                    let is_name = snapshot.slice(start..end).is_ok_and(|name| name.chars().all(|c| c.is_alphanumeric() || c == '_'));
                    let line_text_now = line_text(text, cache, line);
                    let byte = cache.line_bytes[line]
                        + line_text_now.char_indices().nth(start.saturating_sub(line_start)).map_or(line_text_now.len(), |(at, _)| at);
                    let lexed = cache.kind_at(text, byte);
                    let leads_nowhere = matches!(
                        lexed,
                        Some(
                            HighlightKind::Keyword
                                | HighlightKind::String
                                | HighlightKind::Number
                                | HighlightKind::Comment
                                | HighlightKind::DocComment
                        )
                    );
                    let is_name = is_name && !leads_nowhere;
                    if end > start && is_name && (x_of(start, true)..=x_of(end, false)).contains(&pos.x) {
                        let top = at.top;
                        let anchor = Rect::from_x_y_ranges(x_of(start, true)..=x_of(end, false), top..=top + line_height);
                        peeked = Some((start, end, anchor));
                    }
                }
                self.value_hover = None;
                if self.execution.as_ref().is_some_and(|(_, stopped)| stopped.is_some()) {
                    let under = |pos: egui::Pos2| -> Option<(String, Rect)> {
                        let at = pointed(pos);
                        let line_start = snapshot.point_to_offset(zen_text::Point::new(at.line, 0)).ok()?;
                        let column = offset_at(pos).checked_sub(line_start)?;
                        let line_text_now = line_text(text, cache, at.line);
                        let (start, end) = super::evaluate::expression_at(line_text_now, column)?;
                        let byte = cache.line_bytes[at.line]
                            + line_text_now.char_indices().nth(column).map_or(line_text_now.len(), |(at, _)| at);
                        let lexed = cache.kind_at(text, byte);
                        if matches!(
                            lexed,
                            Some(
                                HighlightKind::Keyword
                                    | HighlightKind::String
                                    | HighlightKind::Number
                                    | HighlightKind::Comment
                                    | HighlightKind::DocComment
                            )
                        ) {
                            return None;
                        }
                        let (left, right) = (at.x_after(start), at.x_of(end));
                        if !(left..=right).contains(&pos.x) {
                            return None;
                        }
                        let expression = line_text_now.chars().skip(start).take(end - start).collect();
                        Some((expression, Rect::from_x_y_ranges(left..=right, at.top..=at.top + line_height)))
                    };
                    let held = ui.input(|input| input.modifiers);
                    let pressed = ui.input(|input| input.pointer.any_down());
                    if let Some(pos) = response.hover_pos().filter(|_| held.is_none() && !pressed) {
                        let frame = ui.ctx().cumulative_frame_nr();
                        self.value_hover = under(pos).map(|(expression, anchor)| (expression, anchor, frame));
                    }
                    let only_alt = held.alt && !held.shift && !held.command && !held.ctrl;
                    if response.clicked()
                        && only_alt
                        && let Some(pos) = response.interact_pointer_pos()
                        && let Some((expression, anchor)) = under(pos)
                    {
                        self.value_anchor = Some(anchor);
                        self.gutter_actions
                            .push((super::evaluate::QUICK_EVALUATE.to_owned(), json!({ "expression": expression })));
                    }
                }
                self.drawn_run_inlay = None;
                if run_inlay
                    && self.execution.as_ref().is_some_and(|(_, stopped)| stopped.is_some())
                    && let Some(pos) = ui.ctx().pointer_hover_pos().filter(|pos| response.rect.contains(*pos))
                {
                    let at = pointed(pos);
                    if execution_line != Some(at.line) {
                        let indent = line_text(text, cache, at.line).chars().take_while(|c| c.is_whitespace()).count();
                        let size = (line_height - 4.0).clamp(10.0, 16.0);
                        let left = (at.x_of(indent) - size - 2.0).max(at.left - TEXT_LEFT + 1.0);
                        let button = Rect::from_min_size(pos2(left, at.top + (line_height - size) / 2.0), Vec2::splat(size));
                        if super::inline_values::run_to_cursor_button(ui, button, palette) {
                            intents.push(Intent("debug.execution.runToCursor", json!({ "path": path, "line": at.line })));
                        }
                        self.drawn_run_inlay = Some((at.line, button));
                    }
                }
                if let Some(pos) = response.hover_pos()
                    && let Some(&(mark, start)) = fold_marks.iter().find(|(mark, _)| mark.contains(pos))
                {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    if let Some(fold) = fold_list.iter().find(|fold| fold.start == start) {
                        let (first, last) = (point(fold.start).line, point(fold.end).line);
                        let shown: Vec<&str> = (first..=last.min(first + 19)).map(|line| line_text(text, cache, line)).collect();
                        fold_preview = Some((mark, shown.join("\n")));
                    }
                }
                if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                    && let Some((_, start)) = fold_marks.iter().find(|(mark, _)| mark.contains(pos))
                {
                    intents.push(Intent("editor.fold.toggle", json!({ "start": start })));
                }
                let modifiers = ui.input(|input| input.modifiers);
                if response.is_pointer_button_down_on()
                    && ui.input(|input| input.pointer.any_pressed())
                    && let Some(pos) = ui.input(|input| input.pointer.interact_pos())
                {
                    response.request_focus();
                    self.last_activity = ui.input(|input| input.time);
                    let offset = offset_at(pos);
                    self.soft_wrap.clicked(offset, locate(pos).2);
                    let time = ui.input(|input| input.time);
                    let clicks = match self.last_press {
                        Some((at, from, count)) if time - at < DOUBLE_CLICK && from.distance(pos) < 6.0 => count % 3 + 1,
                        _ => 1,
                    };
                    self.last_press = Some((time, pos, clicks));
                    let (start, end) = unit_at(snapshot, offset, clicks, click_humps);
                    self.column_drag = None;
                    let (word_start, word_end) = unit_at(snapshot, offset, 2, false);
                    if modifiers.command && !modifiers.alt && clicks == 1 && word_end > word_start {
                        let command =
                            if modifiers.shift { "navigate.typeDeclaration.goTo" } else { "navigate.declaration.goTo" };
                        intents.push(Intent(command, json!({ "path": path, "offset": offset })));
                        self.lookup_at = Some(pos + vec2(0.0, line_height));
                        self.drag = None;
                    } else if modifiers.alt && modifiers.shift && clicks == 1 {
                        intents.push(Intent("editor.caret.toggle", json!({ "head": offset })));
                        self.drag = None;
                    } else if modifiers.shift && clicks == 1 {
                        intents.push(Intent(
                            "editor.selection.set",
                            json!({ "selections": [{ "anchor": primary.anchor, "head": offset }] }),
                        ));
                        self.drag = Some((primary.anchor, primary.anchor, 1));
                    } else if (modifiers.alt || column_mode) && clicks == 1 {
                        self.column_drag = Some(point_at(pos));
                        self.drag = None;
                        intents.push(Intent("editor.selection.set", json!({ "selections": [{ "head": offset }] })));
                    } else {
                        intents.push(Intent(
                            "editor.selection.set",
                            json!({ "selections": [{ "anchor": start, "head": end }] }),
                        ));
                        self.drag = Some((start, end, clicks));
                    }
                } else if response.dragged()
                    && let (Some((line, column)), Some(pos)) =
                        (self.column_drag, ui.input(|input| input.pointer.interact_pos()))
                {
                    let (head_line, head_column) = point_at(pos);
                    intents.push(Intent(
                        "editor.selection.column",
                        json!({ "anchor": { "line": line, "column": column }, "head": { "line": head_line, "column": head_column } }),
                    ));
                } else if response.dragged()
                    && let (Some((start, end, clicks)), Some(pos)) =
                        (self.drag, ui.input(|input| input.pointer.interact_pos()))
                {
                    let offset = offset_at(pos);
                    let (unit_start, unit_end) = unit_at(snapshot, offset, clicks, click_humps);
                    let (anchor, head) = if offset < start { (end, unit_start) } else { (start, unit_end.max(offset)) };
                    intents.push(Intent(
                        "editor.selection.set",
                        json!({ "selections": [{ "anchor": anchor, "head": head }] }),
                    ));
                    let edge = ui.clip_rect();
                    let delta = vec2(
                        if pos.x < edge.left() {
                            edge.left() - pos.x
                        } else if pos.x > edge.right() {
                            edge.right() - pos.x
                        } else {
                            0.0
                        },
                        if pos.y < edge.top() {
                            edge.top() - pos.y
                        } else if pos.y > edge.bottom() {
                            edge.bottom() - pos.y
                        } else {
                            0.0
                        },
                    );
                    if delta != Vec2::ZERO {
                        ui.scroll_with_delta_animation(delta * 0.5, egui::style::ScrollAnimation::none());
                    }
                }
                if response.drag_stopped() {
                    self.drag = None;
                    self.column_drag = None;
                }
                let bounds = |line: usize| {
                    let row = rows.partition_point(|&shown| shown <= line).saturating_sub(1);
                    (rows.get(row) == Some(&line)).then(|| {
                        let width = line_text(text, cache, line).chars().count().max(1) as f32 * char_width;
                        let height = line_height * rows.height(row) as f32;
                        Rect::from_min_size(pos2(text_x, origin.y + TOP + layout.top(&rows, row)), vec2(width, height))
                    })
                };
                let anchor = point(primary.anchor);
                let shown = super::editor_access::Shown {
                    path: &path,
                    version: document.version(),
                    lines,
                    line: &|line| line_text(text, cache, line),
                    view: shown_lines,
                    selection: ((anchor.line, anchor.column), (primary_point.line, primary_point.column)),
                    bounds: &bounds,
                    split: self.split,
                };
                self.access.publish(ui, id, &shown);
            },
        );
        self.scrolled_x = scrolled.state.clamped_offset().x;
        if !todo_marks.is_empty() {
            let lines = todo_marks
                .iter()
                .filter_map(|mark| snapshot.offset_to_point(mark.start).ok().map(|point| (point.line, mark.color)));
            super::todo_marks::paint_on_scroll_bar(ui.painter(), text_area, lines, snapshot.len_lines(), palette);
        }
        if let Some(metrics) = wrap_metrics {
            let rows_at = |y: f32| layout.visual_at(&rows, y - TOP);
            let (from, to) = (rows_at(scrolled_to), rows_at(scrolled.state.clamped_offset().y));
            self.soft_wrap.settle(metrics, &path, &mut rows, (from, to), view_rows, (folded, text_of));
        }

        let sticky_max = if zen_workbench::sticky::enabled(session.settings()) {
            zen_workbench::sticky::max_lines(session.settings()).min(visible_lines / 2)
        } else {
            0
        };
        let mut scopes: Vec<zen_workbench::sticky::Scope> = fold_list
            .iter()
            .filter(|fold| !fold.folded)
            .filter_map(|fold| {
                let start_line = fold.sticky_line?;
                (fold.end_line > start_line)
                    .then_some(zen_workbench::sticky::Scope { start_line, end_line: fold.end_line })
            })
            .collect();
        zen_workbench::sticky::sort_scopes(&mut scopes);
        let stuck = super::sticky_lines::StickyLines::pick(&scopes, &rows, first_visible, sticky_max);
        let stuck: Vec<(usize, Arc<egui::Galley>)> = stuck
            .into_iter()
            .filter(|&line| line < lines)
            .map(|line| {
                let (display, columns) = display_line(line_text(text, cache, line), tab);
                let line_start = snapshot.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or_default();
                let semantic = (line_start, semantic_tokens.as_slice());
                (line, self.galley(ui, text, cache, line, (&display, &columns, &[]), palette, &font, semantic))
            })
            .collect();
        let number_right = numbers_right(gutter, numbered, char_width, annotated);
        let numbering = super::line_numbers::Numbering {
            mode: zen_workbench::line_numbers::mode(session.settings()),
            rows: &rows,
            caret_row: rows.partition_point(|&shown| shown <= primary_point.line).saturating_sub(1),
        };
        let scroll_bar = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_outer_margin;
        let sticky_frame = super::sticky_lines::StickyFrame {
            area: Rect::from_min_max(area.min, pos2((area.right() - scroll_bar).max(gutter.right()), area.bottom())),
            gutter,
            text_x: sticky_text_x,
            number_right,
            numbers: numbers_shown,
            numbering,
            line_height,
            number_font: editor.gutter_font(),
            caret_line: primary_point.line,
            lines: &stuck,
        };
        super::sticky_lines::StickyLines::paint_text(ui, &sticky_frame, palette);

        let count = |severity: &str| problems.iter().filter(|problem| problem.severity == severity).count();
        let mut right = text_area.right() - 18.0;
        if zen_workbench::power_save::is_on(session) {
            let icon = Rect::from_center_size(pos2(right - 8.0, text_area.top() + 12.0), vec2(18.0, 14.0));
            super::status_widgets::battery_icon(ui.painter(), icon, palette.text_secondary);
            let hover = ui.interact(icon, id.with("power-save"), Sense::hover());
            let text = format!("{}\n{}", t!("ide-power-save-notice-title"), t!("ide-power-save-notice-body"));
            accessibility::name(&hover, egui::Role::Image, &text);
            hover.on_hover_text(text);
            right = icon.left() - 8.0;
        }
        for severity in ["typo", "weakWarning", "warning", "error"] {
            let count = count(severity);
            if count == 0 {
                continue;
            }
            let number =
                ui.painter().layout_no_wrap(count.to_string(), FontId::proportional(12.0), palette.text_secondary);
            let number_left = right - number.size().x;
            let y = text_area.top() + 12.0;
            ui.painter().galley(pos2(number_left, y - number.size().y / 2.0), number, palette.text_secondary);
            ui.painter().circle_filled(pos2(number_left - 9.0, y), 4.5, palette.problem(severity));
            right = number_left - 22.0;
        }

        let share_of = |line: usize| {
            if wrapping {
                let row = rows.partition_point(|&shown| shown <= line).saturating_sub(1);
                rows.top(row) as f32 / rows.total().max(1) as f32
            } else {
                line as f32 / lines as f32
            }
        };
        if !problems.is_empty() && lines > 0 {
            let mut marked: Vec<&ShownProblem> = problems.iter().collect();
            marked.sort_by_key(|problem| std::cmp::Reverse(problem.rank()));
            for problem in marked {
                let y = text_area.top() + text_area.height() * share_of(problem.line - 1);
                let mark = Rect::from_min_size(pos2(text_area.right() - 8.0, y), vec2(6.0, 3.0));
                ui.painter().rect_filled(mark, 1, palette.problem(&problem.severity));
                let hit =
                    ui.interact(mark.expand2(vec2(2.0, 2.0)), id.with(("problem-mark", problem.start)), Sense::click());
                accessibility::button_with(&hit, || {
                    t!(
                        "a11y-problem",
                        severity = problem.severity.as_str(),
                        message = problem.message.as_str(),
                        line = problem.line
                    )
                });
                if hit.hovered() {
                    egui::Tooltip::for_widget(&hit).at_pointer().show(|ui| {
                        ui.label(&problem.message);
                    });
                }
                if hit.clicked() {
                    intents.push(Intent("editor.selection.set", json!({ "selections": [{ "head": problem.start }] })));
                    self.reveal = true;
                }
            }
        }

        if !matches.is_empty() && lines > 0 {
            let mut last_y = f32::NEG_INFINITY;
            for range in &matches {
                let line = snapshot.offset_to_point(range.start).unwrap_or_default().line;
                let y = text_area.top() + text_area.height() * share_of(line);
                if y - last_y >= 2.0 {
                    let mark = Rect::from_min_size(pos2(text_area.right() - 6.0, y), vec2(4.0, 2.0));
                    ui.painter().rect_filled(mark, 0, palette.search_mark);
                    last_y = y;
                }
            }
        }

        let painter = ui.painter().with_clip_rect(gutter);
        let gutter_response = ui.interact(gutter, id.with("gutter"), Sense::click_and_drag());
        accessibility::name_with(&gutter_response, egui::Role::Group, || t!("a11y-gutter"));
        let numbers = number_area(gutter, number_right, annotated);
        let dot_x = number_right - numbered.to_string().len().max(2) as f32 * char_width / 2.0;
        let radius = (line_height * gutter::BREAKPOINT).clamp(4.0, 9.0);
        painter.rect_filled(
            Rect::from_min_max(pos2(gutter.right() - gutter::SEPARATOR, gutter.top()), gutter.right_bottom()),
            0,
            palette.gutter_separator,
        );
        let gutter_hovered = ui.rect_contains_pointer(gutter);
        let arrows = FoldArrows::of(session.settings(), gutter_hovered);
        let fold_x = fold_center(gutter);
        let line_at = |y: f32| {
            let row = layout.row_at(&rows, y - gutter.top() - TOP + scrolled_to);
            rows.get(row.min(rows.len().saturating_sub(1))).copied().unwrap_or_default()
        };
        let hovered_line = gutter_response.hover_pos().filter(|pos| numbers.contains(*pos)).map(|pos| line_at(pos.y));
        self.drawn_marks.clear();
        self.drawn_bookmarks.clear();
        self.drawn_execution = None;
        let starts: HashMap<usize, (usize, bool)> = fold_list
            .iter()
            .filter(|fold| fold.end_line > fold.start_line || fold.kind == "manual")
            .map(|fold| (fold.start_line, (fold.start, fold.folded)))
            .rev()
            .collect();
        let ends: HashMap<usize, usize> = fold_list
            .iter()
            .filter(|fold| arrows.bottom && !fold.folded && fold.end_line > fold.start_line)
            .map(|fold| (fold.end_line, fold.start))
            .rev()
            .collect();
        let mut toggle = None;
        let mut run_clicked = None;
        let mut bulb_clicked = None;
        let mut swatch_clicked = false;
        let mut bookmark_clicked = None;
        let mut bookmark_hint = None;
        let mut marker_clicked = None;
        self.swatches.begin();
        for (row_index, &line) in rows.iter().enumerate().take(first_visible + visible_lines).skip(first_visible) {
            let center = gutter.top() + TOP + layout.top(&rows, row_index) - scrolled_to + line_height / 2.0;
            if let Some(target) = run_targets.iter().find(|target| target.line == line) {
                let icon = run_icon_rect(number_right, center);
                let response = ui.interact(icon, id.with(("run-icon", line)), Sense::click());
                accessibility::button_with(&response, || t!("a11y-run-icon", name = target.name.as_str()));
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    painter.rect_filled(icon.expand(1.0), 3, palette.hover);
                }
                let mark = (target.kind != "main" && target.kind != HTTP && target.kind != SQL)
                    .then(|| self.test_mark_of(target))
                    .flatten();
                run_icon(&painter, icon.shrink(1.0), mark, palette);
                if response.clicked() {
                    run_clicked = Some((target.clone(), icon.left_bottom() + vec2(0.0, 2.0)));
                }
            }
            if let Some(quick_fix) = bulb.filter(|_| line == primary_point.line)
                && !run_targets.iter().any(|target| target.line == line)
            {
                let icon = run_icon_rect(number_right, center);
                let response = ui.interact(icon, id.with(("bulb", line)), Sense::click());
                accessibility::button_with(&response, || t!("command-editor-intentions-list"));
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    painter.rect_filled(icon.expand(1.0), 3, palette.hover);
                }
                let color = super::intentions_popup::bulb_color(palette, quick_fix);
                super::glyphs::glyph_bulb(&painter, icon.shrink(1.0), color);
                if response.clicked() {
                    bulb_clicked = Some(icon.left_bottom() + vec2(0.0, 2.0));
                }
            }
            if !run_targets.iter().any(|target| target.line == line)
                && !(bulb.is_some() && line == primary_point.line)
                && let Some(swatch) = self.swatches.on_line(&path, line)
            {
                let icon = run_icon_rect(number_right, center);
                swatch_clicked |= self.swatches.paint(ui, &painter, id, (&path, &swatch), icon, palette);
            }
            if let Some(bookmark) = bookmarks.iter().find(|bookmark| bookmark.line == line) {
                let taken = run_targets.iter().any(|target| target.line == line)
                    || (bulb.is_some() && line == primary_point.line)
                    || self.swatches.on_line(&path, line).is_some();
                let icon = if taken {
                    let left = gutter.left() + if annotated { ANNOTATIONS } else { 0.0 };
                    Rect::from_center_size(pos2(left + gutter::NUMBERS_LEFT / 2.0, center), vec2(10.0, 13.0))
                } else {
                    run_icon_rect(number_right, center)
                };
                let response = ui.interact(icon, id.with(("bookmark", line)), Sense::click());
                accessibility::button_with(&response, || bookmark.hint());
                bookmark_icon(&painter, icon, bookmark.mnemonic, palette);
                self.drawn_bookmarks.push((line, bookmark.mnemonic));
                if response.hovered() {
                    bookmark_hint = Some(bookmark.hint());
                }
                if response.clicked() {
                    bookmark_clicked = Some((line, ui.input(|input| input.modifiers.alt)));
                }
            }
            if let Some(marker) = line_markers.iter().find(|marker| marker.line == line) {
                let busy = run_targets.iter().any(|target| target.line == line)
                    || (bulb.is_some() && line == primary_point.line)
                    || self.swatches.on_line(&path, line).is_some();
                let bookmarked = bookmarks.iter().any(|bookmark| bookmark.line == line);
                if !(busy && bookmarked) {
                    let icon = if busy || bookmarked {
                        let left = gutter.left() + if annotated { ANNOTATIONS } else { 0.0 };
                        Rect::from_center_size(pos2(left + gutter::NUMBERS_LEFT / 2.0, center), Vec2::splat(12.0))
                    } else {
                        run_icon_rect(number_right, center)
                    };
                    if super::line_markers::LineMarkers::paint(ui, &painter, id, icon, marker, palette) {
                        marker_clicked = Some((marker.clone(), icon.left_bottom() + vec2(0.0, 2.0)));
                    }
                }
            }
            let arrow = starts
                .get(&line)
                .filter(|(_, folded)| arrows.shows(*folded))
                .map(|&(start, folded)| (start, folded, false))
                .or_else(|| ends.get(&line).filter(|_| arrows.shows(false)).map(|&start| (start, false, true)));
            if let Some((start, folded, bottom)) = arrow {
                let mark = Rect::from_center_size(pos2(fold_x, center), Vec2::splat(14.0));
                let response = ui.interact(mark, id.with(("fold", line)), Sense::click());
                accessibility::name_with(&response, egui::Role::DisclosureTriangle, || {
                    t!("a11y-fold", line = line + 1)
                });
                accessibility::edit(&response, |node| node.set_expanded(!folded));
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let color = if response.hovered() { palette.text } else { palette.fold_arrow };
                let icon = Rect::from_center_size(mark.center(), Vec2::splat(gutter::ARROW));
                if folded {
                    crate::icons::chevron_right(&painter, icon, color);
                } else if bottom {
                    crate::icons::chevron_up(&painter, icon, color);
                } else {
                    crate::icons::chevron_down(&painter, icon, color);
                }
                if response.clicked() {
                    toggle = Some(start);
                }
            }
            let mark = breakpoints
                .iter()
                .find(|breakpoint| breakpoint.line == line)
                .map(|breakpoint| GutterMark::Breakpoint(breakpoint.state))
                .or((hovered_line == Some(line)).then_some(GutterMark::Hover));
            let dot = pos2(dot_x, center);
            if let Some(mark) = mark {
                breakpoint_dot(&painter, dot, radius, mark, palette);
                self.drawn_marks.push((line, mark));
            }
            if execution_line == Some(line) {
                execution_arrow(&painter, dot, radius, palette);
                self.drawn_execution = Some(line);
            } else if mark.is_none() && numbers_shown {
                let color = if line == primary_point.line { palette.text_secondary } else { palette.gutter_text };
                painter.text(
                    pos2(number_right, center),
                    Align2::RIGHT_CENTER,
                    numbering.label(line),
                    editor.gutter_font(),
                    color,
                );
            }
        }
        if let Some(hint) = bookmark_hint {
            egui::Tooltip::for_widget(&gutter_response).at_pointer().show(|ui| {
                ui.set_max_width(420.0);
                ui.label(hint);
            });
        } else if let Some(message) = hovered_line
            .and_then(|line| breakpoints.iter().find(|breakpoint| breakpoint.line == line))
            .and_then(|breakpoint| breakpoint.message.clone())
        {
            egui::Tooltip::for_widget(&gutter_response).at_pointer().show(|ui| {
                ui.set_max_width(420.0);
                ui.label(message);
            });
        }
        if gutter_response.secondary_clicked()
            && let Some(pos) = gutter_response.interact_pointer_pos()
        {
            let line = line_at(pos.y);
            self.gutter_menu_line = Some(line);
            if numbers.contains(pos) && breakpoints.iter().any(|breakpoint| breakpoint.line == line) {
                self.no_gutter_menu = true;
                self.breakpoint_anchor = Some(pos + vec2(8.0, 8.0));
                self.gutter_actions
                    .push((super::breakpoints_dialog::PROPERTIES.to_owned(), json!({ "path": path, "line": line })));
            }
        }
        let remove_by_drag = session.settings().get_or(zen_workbench::debug::REMOVE_BY_DRAG, false);
        if gutter_response.drag_started()
            && let Some(origin) = ui.input(|input| input.pointer.press_origin())
            && numbers.contains(origin)
        {
            let line = line_at(origin.y);
            self.breakpoint_drag = breakpoints.iter().any(|breakpoint| breakpoint.line == line).then_some(line);
        }
        if let Some(from) = self.breakpoint_drag
            && let Some(pointer) = ui.input(|input| input.pointer.latest_pos())
        {
            if gutter_response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                let state =
                    breakpoints.iter().find(|breakpoint| breakpoint.line == from).map(|breakpoint| breakpoint.state);
                if let Some(state) = state {
                    let ghost = ui.painter().with_clip_rect(ui.clip_rect());
                    breakpoint_dot(&ghost, pointer, radius, GutterMark::Breakpoint(state), palette);
                }
            }
            if gutter_response.drag_stopped() {
                self.breakpoint_drag = None;
                let copy = ui.input(|input| input.modifiers.command);
                let place = json!({ "path": path, "line": from });
                if gutter.contains(pointer) {
                    let to = line_at(pointer.y);
                    if to != from {
                        let command = if copy { "debug.breakpoint.copy" } else { "debug.breakpoint.move" };
                        let mut params = place;
                        params["toLine"] = json!(to);
                        intents.push(Intent(command, params));
                    }
                } else if remove_by_drag {
                    intents.push(Intent("debug.breakpoint.remove", place));
                }
            }
        }
        if gutter_response.middle_clicked()
            && let Some(pos) = gutter_response.interact_pointer_pos()
            && numbers.contains(pos)
        {
            let line = line_at(pos.y);
            if breakpoints.iter().any(|breakpoint| breakpoint.line == line) {
                let command = if remove_by_drag { "debug.breakpoint.remove" } else { "debug.breakpoint.toggleEnabled" };
                intents.push(Intent(command, json!({ "path": path, "line": line })));
            }
        }
        let annotation_clicked = self.annotations(
            ui,
            &mut intents,
            &path,
            &gutter_response,
            (&rows, &layout),
            (first_visible, visible_lines, scrolled_to),
            (palette, aids.indent_guides),
        );
        self.coverage_marks(
            ui,
            &gutter_response,
            &GutterRows {
                gutter,
                rows: &rows,
                layout: &layout,
                first: first_visible,
                count: visible_lines,
                scrolled: scrolled_to,
            },
            palette,
        );
        let marked = annotation_clicked
            || self.change_markers(
                ui,
                &mut intents,
                &path,
                &GutterRows {
                    gutter,
                    rows: &rows,
                    layout: &layout,
                    first: first_visible,
                    count: visible_lines,
                    scrolled: scrolled_to,
                },
                palette,
            );
        if let Some((line, true)) = bookmark_clicked {
            self.gutter_actions.push(("view.bookmark.rename".to_owned(), json!({ "path": path, "line": line + 1 })));
        }
        if gutter_response.clicked()
            && !marked
            && toggle.is_none()
            && run_clicked.is_none()
            && bulb_clicked.is_none()
            && bookmark_clicked.is_none()
            && marker_clicked.is_none()
            && !swatch_clicked
            && let Some(pos) = gutter_response.interact_pointer_pos()
        {
            let line = line_at(pos.y);
            if numbers.contains(pos) {
                let modifiers = ui.input(|input| input.modifiers);
                let place = json!({ "path": path, "line": line });
                let exists = breakpoints.iter().any(|breakpoint| breakpoint.line == line);
                if modifiers.alt {
                    if exists {
                        intents.push(Intent("debug.breakpoint.toggleEnabled", place));
                    }
                } else if exists {
                    if !remove_by_drag {
                        intents.push(Intent("debug.breakpoint.remove", place));
                    }
                } else if modifiers.shift {
                    let mut logging = place;
                    logging["suspend"] = json!("none");
                    logging["logHit"] = json!(true);
                    intents.push(Intent("debug.breakpoint.set", logging));
                } else {
                    intents.push(Intent("debug.breakpoint.toggle", place));
                }
            } else {
                let start = snapshot.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or_default();
                let end = snapshot.point_to_offset(zen_text::Point::new(line + 1, 0)).unwrap_or(snapshot.len_chars());
                intents
                    .push(Intent("editor.selection.set", json!({ "selections": [{ "anchor": start, "head": end }] })));
                ui.memory_mut(|memory| memory.request_focus(id));
            }
        }

        if let Some(start) = toggle {
            intents.push(Intent("editor.fold.toggle", json!({ "start": start })));
        }
        if let Some(line) = self.sticky.paint_gutter(ui, id, &sticky_frame, palette) {
            intents.push(Intent("editor.stickyLines.goTo", json!({ "line": line })));
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        if let Some((target, at)) = run_clicked {
            if target.kind == HTTP {
                self.run_http(registry, session, &path, &target);
            } else if target.kind == SQL {
                let params = json!({ "path": path, "offset": target.configuration["offset"] });
                let command = if is_script(session, &path) { "db.script.run" } else { "db.injection.run" };
                self.gutter_actions.push((command.to_owned(), params));
            } else {
                let coverage = registry
                    .dispatch(session, "coverage.run.supports", json!({ "configuration": target.configuration }))
                    .is_ok_and(|answer| answer["supported"] == true);
                self.run_menu = Some(RunMenu { target, at, fresh: true, coverage, items: Vec::new() });
            }
        }
        if let Some((marker, at)) = marker_clicked {
            self.line_markers.clicked(registry, session, &path, marker, at);
            if let Some(notice) = self.line_markers.notice.take() {
                self.notice = Some(notice);
            }
        }
        if let Some(at) = bulb_clicked {
            self.completion = None;
            self.intentions.open(PopupAnchor::At(at));
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        let caret = Some((primary_point.line + 1, primary_point.column + 1));
        self.page_lines = page_rows.unwrap_or(visible_lines).saturating_sub(2).max(1);
        self.shown_lines = shown_lines;
        self.follow_peek(ui, registry, session, &path, peeked);
        session.state_mut::<zen_workbench::sticky::ShownTop>().set(&path, shown_lines.0);
        let toolbar_was_shown = std::mem::take(&mut self.markdown_toolbar_shown);
        if let Some(band) = selected_band
            && let Some(asked) =
                self.markdown_toolbar(ui, session, palette, (band, text_area), &path, toolbar_was_shown)
        {
            intents.push(Intent(asked.0, asked.1));
        }
        for Intent(command, params) in intents {
            if command.starts_with("debug.") {
                self.breakpoints = None;
            }
            let _ = registry.dispatch_unrecorded(session, command, params);
        }
        self.run_menu(ui, registry, session, palette);
        self.line_markers.menu(ui, registry, session, palette);
        if let Some(notice) = self.line_markers.notice.take() {
            self.notice = Some(notice);
        }
        self.swatches.picker(ui, registry, session, palette);
        self.completion_popup(ui, registry, session, palette, text_area);
        self.navigation(ui, registry, session, palette);
        self.show_usages(ui, registry, session, palette);
        if let Some(outcome) = self.intentions.show(ui, palette, self.caret_place()) {
            self.intention_outcome(registry, session, outcome);
        }
        if let Some(outcome) = self.generate.show(ui, palette, self.caret_place()) {
            self.generate_outcome(registry, session, outcome);
        }
        if let Some(chosen) = self.sql_connection.show(ui, palette, self.caret_place()) {
            self.sql_connection_chosen(registry, session, chosen);
        }
        if let Some(outcome) = self.surround.show(ui, palette, popup_highlighter.as_ref()) {
            self.surround_outcome(registry, session, outcome);
        }
        if let Some(chosen) = self.unwrap.show(ui, palette) {
            self.apply_unwrap(registry, session, chosen);
            self.focus();
        }
        if let Some(index) = self.extract.show(ui, session, palette, self.caret_place()) {
            if let Some(notice) = self.extract.clicked(registry, session, index) {
                self.notice = Some(notice);
            }
            self.focus();
        }
        self.doc_popups.show(ui, palette, self.caret_place().rect(), text_area, popup_highlighter.as_ref());
        if let Some(notice) = self.problem_popups.show(ui, registry, session, palette, self.caret_place()) {
            self.notice = Some(notice);
        }
        self.show_peek(ui, palette, popup_highlighter.as_ref());
        if let Some((mark, preview)) = fold_preview {
            egui::Area::new(ui.scope_id().with("fold-preview"))
                .order(egui::Order::Foreground)
                .pivot(Align2::LEFT_TOP)
                .fixed_pos(pos2(mark.left() - 10.0, mark.bottom() + 4.0))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::symmetric(10, 6)).show(
                        ui,
                        |ui| {
                            let job = super::doc_popups::code_job(
                                &preview,
                                "",
                                palette,
                                popup_highlighter.as_ref(),
                                text_area.width().max(200.0),
                            );
                            ui.add(egui::Label::new(job).extend());
                        },
                    );
                });
        }
        match wrap_metrics {
            Some(metrics) => {
                let folded = folded_at.iter().map(|(&line, &(column, ..))| (line, column)).collect();
                self.soft_wrap.keep(session, &path, rows, folded, metrics);
            }
            None => self.soft_wrap.drop_frame(),
        }
        self.highlight = Some(owned);
        let version =
            session.state::<Workbench>().and_then(Workbench::active_document).map_or(0, Document::selection_version);
        self.shown = Some((path, version));
        caret
    }

    fn intention_outcome(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        outcome: super::intentions_popup::IntentionsOutcome,
    ) {
        match outcome {
            super::intentions_popup::IntentionsOutcome::Notice(message) => self.notice = Some(message),
            super::intentions_popup::IntentionsOutcome::Apply(id)
                if id == zen_workbench::database::injected::RUN_ACTION =>
            {
                let path =
                    session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
                self.gutter_actions.push(("db.injection.run".to_owned(), json!({ "path": path })));
                self.focus();
            }
            super::intentions_popup::IntentionsOutcome::Apply(id)
                if id == zen_workbench::database::injected::CHOOSE_ACTION =>
            {
                let path =
                    session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
                if let Some(path) = path {
                    self.ask_sql_connection(registry, session, &path, None);
                }
            }
            super::intentions_popup::IntentionsOutcome::Apply(id) => {
                match registry.dispatch(session, "editor.intentions.apply", json!({ "id": id })) {
                    Err(zen_core::DispatchError::Failed { error, .. }) => self.notice = Some(error.message),
                    Err(error) => self.notice = Some(error.to_string()),
                    Ok(_) => {}
                }
                self.focus();
            }
        }
    }

    fn generate_outcome(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        outcome: super::generate_popup::GenerateOutcome,
    ) {
        match outcome {
            super::generate_popup::GenerateOutcome::Notice(message) => self.notice = Some(message),
            super::generate_popup::GenerateOutcome::Apply { id, members, options } => {
                let params = json!({ "id": id, "members": members, "options": options });
                match registry.dispatch(session, "editor.generate.apply", params) {
                    Err(zen_core::DispatchError::Failed { error, .. }) => self.notice = Some(error.message),
                    Err(error) => self.notice = Some(error.to_string()),
                    Ok(_) => {}
                }
                self.reveal = true;
                self.focus();
            }
        }
    }

    fn surround_outcome(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        outcome: super::surround_popup::SurroundOutcome,
    ) {
        match outcome {
            super::surround_popup::SurroundOutcome::Notice(message) => self.notice = Some(message),
            super::surround_popup::SurroundOutcome::Configure => self.configure_templates = true,
            super::surround_popup::SurroundOutcome::Apply { name, template } => {
                let params = json!({ "name": name, "template": template });
                match registry.dispatch(session, "editor.surround.apply", params) {
                    Err(zen_core::DispatchError::Failed { error, .. }) => self.notice = Some(error.message),
                    Err(error) => self.notice = Some(error.to_string()),
                    Ok(_) => {}
                }
                self.reveal = true;
                self.focus();
            }
        }
    }

    fn keyboard(&mut self, ui: &Ui, session: &Session) -> Vec<(String, Value)> {
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else {
            return Vec::new();
        };
        let events = ui.input(|input| input.events.clone());
        let (now, modifiers) = ui.input(|input| (input.time, input.modifiers));
        let armed = self.double_option.armed;
        let typed = events.iter().any(|event| {
            matches!(event, Event::Key { key, .. } if !(armed && matches!(key, egui::Key::ArrowUp | egui::Key::ArrowDown)))
                || matches!(event, Event::Text(_) | Event::PointerButton { .. })
        });
        let cloning = self.double_option.update(now, modifiers, typed);
        let keymap = crate::keymap::current();
        let wrapped = || zen_workbench::soft_wrap::is_soft_wrapped(session.settings(), document);
        let mut commands = Vec::new();
        let insert = |text: &str| ("editor.edit.insert".to_owned(), json!({ "text": text }));
        let editor = zen_core::keymap::Context::Editor;
        let editor_binding = |stroke: &zen_core::keymap::Keystroke| {
            keymap.lookup(stroke, editor).filter(|binding| binding.context == editor)
        };
        if ui.input(|input| input.viewport().focused.unwrap_or(true)) {
            for stroke in crate::keypad::take() {
                if keymap.starts_sequence(&stroke, editor) {
                    self.sequence = Some((stroke, std::time::Instant::now()));
                } else if let Some(command) = editor_binding(&stroke).and_then(|binding| self.command_of(&binding)) {
                    commands.push(command);
                }
            }
        }
        let mut swallow_text = false;
        let mut option_bound = false;
        for event in events {
            let after_option_binding = std::mem::take(&mut option_bound);
            if std::mem::take(&mut swallow_text) && matches!(event, Event::Text(_)) {
                continue;
            }
            if let Event::Key { key, pressed: true, modifiers, .. } = &event {
                let stroke = crate::keymap::keystroke(*key, *modifiers);
                let first = self.pending_sequence().cloned();
                self.sequence = None;
                if let Some(command) = first
                    .and_then(|first| editor_binding(&first.followed_by(stroke.clone())))
                    .and_then(|binding| self.command_of(&binding))
                {
                    commands.push(command);
                    swallow_text = true;
                    continue;
                }
                if keymap.starts_sequence(&stroke, editor) {
                    self.sequence = Some((stroke, std::time::Instant::now()));
                    swallow_text = true;
                    continue;
                }
            }
            match event {
                Event::Text(text) if !text.is_empty() => {
                    let modifiers = ui.input(|input| input.modifiers);
                    if !modifiers.command && !modifiers.ctrl && !after_option_binding {
                        commands.push(("editor.edit.type".to_owned(), json!({ "text": text })));
                    }
                }
                Event::Ime(egui::ImeEvent::Preedit { text, .. }) => self.preedit = text,
                Event::Ime(egui::ImeEvent::Commit(text)) => {
                    self.preedit.clear();
                    if !text.is_empty() {
                        commands.push(insert(&text));
                    }
                }
                Event::Paste(text) if !text.is_empty() => {
                    match super::convert_paste::offer(ui.ctx(), session, document.path(), &text) {
                        Some(offer) => self.paste_offer = Some(offer),
                        None => commands.push(("editor.edit.paste".to_owned(), json!({ "text": text }))),
                    }
                }
                Event::Copy | Event::Cut => {
                    let (copied, whole_lines) = copy_text(document);
                    super::convert_paste::remember_copy(ui.ctx(), document.path(), &copied);
                    ui.ctx().copy_text(copied);
                    if matches!(event, Event::Cut) {
                        if let Some(lines) = whole_lines {
                            commands.push(("editor.selection.set".to_owned(), json!({ "selections": lines })));
                        }
                        commands.push(("editor.edit.deleteBackward".to_owned(), json!({})));
                    }
                }
                Event::Key {
                    key: key @ (egui::Key::ArrowUp | egui::Key::ArrowDown), pressed: true, modifiers, ..
                } if cloning && modifiers.alt && !modifiers.shift && !modifiers.command && !modifiers.ctrl => {
                    let id =
                        if key == egui::Key::ArrowUp { "editor.caret.cloneAbove" } else { "editor.caret.cloneBelow" };
                    commands.push((id.to_owned(), json!({})));
                }
                Event::Key { key, physical_key, pressed: true, modifiers, .. } => {
                    let editor = zen_core::keymap::Context::Editor;
                    if let Some(binding) = crate::keymap::presses(key, physical_key, modifiers)
                        .iter()
                        .filter_map(|stroke| keymap.lookup(stroke, editor))
                        .find(|binding| binding.context == editor)
                        && let Some(command) = self.command_of(&binding)
                    {
                        let mut command = command;
                        option_bound = modifiers.alt && !modifiers.command && !modifiers.ctrl && types_text(key);
                        if wrapped() {
                            if soft_wrap::moves_by_rows(&command) {
                                command.1["visual"] = json!(true);
                            } else {
                                self.soft_wrap.forget_carets();
                            }
                        }
                        commands.push(command);
                    }
                }
                _ => {}
            }
        }
        commands
    }

    fn completion_keys(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some((shown, selected)) = &mut self.completion else { return };
        let count = shown.items.len();
        let none = egui::Modifiers::NONE;
        let (up, down, enter, tab, escape) = ui.input_mut(|input| {
            if count == 0 {
                return (false, false, false, false, input.consume_key(none, egui::Key::Escape));
            }
            (
                input.consume_key(none, egui::Key::ArrowUp),
                input.consume_key(none, egui::Key::ArrowDown),
                input.consume_key(none, egui::Key::Enter),
                input.consume_key(none, egui::Key::Tab),
                input.consume_key(none, egui::Key::Escape),
            )
        });
        if up {
            *selected = selected.checked_sub(1).unwrap_or(count - 1);
        }
        if down {
            *selected = (*selected + 1) % count.max(1);
        }
        if up || down {
            self.completion_reveal = true;
        }
        if enter || tab {
            let index = shown.items.get(*selected).map_or(0, |item| item.index);
            let _ = registry.dispatch(session, "editor.completion.accept", json!({ "index": index, "replace": tab }));
            self.completion = None;
            self.reveal = true;
        } else if escape {
            let _ = registry.dispatch(session, "editor.completion.cancel", json!({}));
            self.completion = None;
        }
    }

    fn completion_popup(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        area: Rect,
    ) {
        const ROW: f32 = 22.0;
        const VISIBLE: usize = 10;
        let Some((shown, selected)) = &self.completion else { return };
        let items = Arc::clone(&shown.items);
        let revision = shown.revision;
        if items.is_empty() {
            return;
        }
        let selected = *selected;
        let width = 460.0_f32.min(area.width() - 20.0).max(240.0);
        let popup = egui::Area::new(ui.scope_id().with("completion")).order(egui::Order::Foreground);
        let Some(popup) = PopupAnchor::UNDER_CARET.area(ui.ctx(), popup, self.caret_place()) else { return };
        let mut scroll = egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]);
        if self.completion_reveal {
            scroll = scroll.vertical_scroll_offset(revealed_offset(self.completion_offset, selected, ROW, VISIBLE));
        }
        let mut chosen = None;
        let mut offset = self.completion_offset;
        let shown = popup.show(ui.ctx(), |ui| {
            palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                let output = scroll.show_rows(ui, ROW, items.len(), |ui, rows| {
                    for row in rows {
                        let item = &items[row];
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        if row == selected {
                            ui.painter().rect_filled(rect, 4, palette.selection);
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 4, palette.popup_hover);
                        }
                        let (letter, color) = palette.completion_mark(&item.kind);
                        let badge =
                            Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), Vec2::splat(15.0));
                        ui.painter().rect_filled(badge, 4, color.gamma_multiply(0.25));
                        ui.painter().text(
                            badge.center(),
                            Align2::CENTER_CENTER,
                            letter,
                            FontId::proportional(10.5),
                            color,
                        );
                        accessibility::item(&response, egui::Role::ListBoxOption, row == selected, &item.label);
                        let mut job = LayoutJob::default();
                        for (index, c) in item.label.chars().enumerate() {
                            let strong = item.matches.contains(&index);
                            job.append(
                                c.encode_utf8(&mut [0; 4]),
                                0.0,
                                TextFormat::simple(
                                    if strong { crate::theme::semibold(13.0) } else { crate::theme::regular(13.0) },
                                    if strong { palette.text } else { palette.text.gamma_multiply(0.9) },
                                ),
                            );
                        }
                        let galley = ui.painter().layout_job(job);
                        let name_right = rect.left() + 26.0 + galley.size().x;
                        ui.painter().galley(
                            pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
                            galley,
                            palette.text,
                        );
                        if let Some(detail) = &item.detail {
                            let clip = Rect::from_min_max(
                                pos2(name_right + 16.0, rect.top()),
                                rect.right_bottom() - vec2(6.0, 0.0),
                            );
                            ui.painter().with_clip_rect(clip).text(
                                pos2(rect.right() - 6.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                detail,
                                crate::theme::regular(12.0),
                                palette.text_secondary,
                            );
                        }
                        if response.clicked() {
                            chosen = Some(item.index);
                        }
                    }
                });
                offset = output.state.clamped_offset().y;
            });
        });
        self.completion_offset = offset;
        let documentation = items[selected].documentation.as_deref().filter(|text| !text.trim().is_empty());
        if let Some(documentation) = documentation {
            let blocks = match &self.completion_documentation {
                Some((at, row, blocks)) if *at == revision && *row == selected => Arc::clone(blocks),
                _ => {
                    let blocks = Arc::new(super::doc_popups::parse(documentation));
                    self.completion_documentation = Some((revision, selected, Arc::clone(&blocks)));
                    blocks
                }
            };
            let list = shown.response.rect;
            let at = documentation_panel_at(list, ui.ctx().content_rect());
            egui::Area::new(ui.scope_id().with("completion-documentation"))
                .order(egui::Order::Foreground)
                .fixed_pos(at)
                .show(ui.ctx(), |ui| {
                    palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                        ui.set_width(DOCUMENTATION_WIDTH - 16.0);
                        egui::ScrollArea::vertical()
                            .max_height(list.height().max(ROW * 6.0))
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                super::doc_popups::render(ui, &blocks, palette, DOCUMENTATION_WIDTH - 24.0, None);
                            });
                    });
                });
        }
        self.completion_reveal = false;
        if let Some(index) = chosen {
            let _ = registry.dispatch(session, "editor.completion.accept", json!({ "index": index, "replace": false }));
            self.completion = None;
            self.reveal = true;
            self.focus();
        }
    }

    fn format_answer(&mut self, ui: &Ui, job: (String, PathBuf, u64), value: &Value) {
        if value["pending"] == true {
            self.format_pending = Some(job);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        } else if let Some(error) = value["error"].as_str() {
            self.notice = Some(error.to_owned());
        }
    }

    fn follow_format(&mut self, ui: &Ui, registry: &Registry, session: &mut Session) {
        let Some((command, path, version)) = self.format_pending.take() else { return };
        let unchanged = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .is_some_and(|document| document.version() == version);
        if !unchanged {
            return;
        }
        if let Ok(value) = registry.dispatch_unrecorded(session, &command, json!({ "path": path, "waitMs": 0 })) {
            self.format_answer(ui, (command, path, version), &value);
        }
    }

    pub(super) fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    pub(super) fn context_actions(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        main: bool,
    ) {
        if let Some(notice) = self.problem_popups.context_actions(ctx, registry, session, main) {
            self.notice = Some(notice);
        }
        ctx.request_repaint();
    }

    pub(super) fn select_dictionary(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        params: &Value,
    ) -> Result<(), String> {
        use zen_workbench::spelling::DictionaryLevel;
        let Some(path) = params["path"].as_str().map(PathBuf::from).or_else(|| {
            session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path().to_owned())
        }) else {
            let message = t!("ide-spelling-no-file");
            self.notice = Some(message.clone());
            return Err(message);
        };
        let fixes: Vec<Value> = [
            (DictionaryLevel::Application, "ide-spelling-application-level"),
            (DictionaryLevel::Project, "ide-spelling-project-level"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (level, title))| {
            let params = json!({ "word": params["word"], "path": path, "level": level });
            json!({
                "id": format!("spelling.level:{index}"),
                "title": t!(title),
                "kind": "action",
                "args": { "action": "spelling.word.save", "params": params },
            })
        })
        .collect();
        self.problem_popups.open_menu(ctx, path, Some(t!("ide-spelling-select-dictionary")), fixes);
        ctx.request_repaint();
        Ok(())
    }

    pub(super) fn show_context_actions(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let fixes = !zen_workbench::scratch::has_selection(session)
            && session
                .state::<Workbench>()
                .and_then(Workbench::active_document)
                .map(|document| document.path().to_owned())
                .and_then(|path| registry.dispatch(session, "problems.fixes.list", json!({ "path": path })).ok())
                .is_some_and(|problems| {
                    problems.as_array().is_some_and(|problems| {
                        problems
                            .iter()
                            .any(|problem| problem["fixes"].as_array().is_some_and(|fixes| !fixes.is_empty()))
                    })
                });
        if fixes {
            self.context_actions(ctx, registry, session, false);
        } else {
            self.open_intentions();
        }
    }

    #[cfg(test)]
    pub(super) fn menu_titles(&self) -> Vec<String> {
        self.problem_popups.menu_titles()
    }

    #[cfg(test)]
    pub(super) fn menu_title(&self) -> Option<String> {
        self.problem_popups.menu_title()
    }

    pub(super) fn problem_popups_open(&self) -> Vec<&'static str> {
        self.problem_popups.open_names()
    }

    pub(super) fn take_problem_actions(&mut self) -> Vec<(String, Value)> {
        self.problem_popups.take_actions()
    }

    pub(super) fn take_window_actions(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.window_actions)
    }

    pub(super) fn set_step_target(&mut self, target: Option<(PathBuf, std::ops::Range<usize>)>) {
        self.step_target = target;
    }

    pub(super) fn take_debug_request(&mut self) -> Option<Value> {
        self.debug_request.take()
    }

    pub(super) fn take_starters_request(&mut self) -> Option<PathBuf> {
        self.starters_request.take()
    }

    pub(super) fn take_external_tool_request(&mut self) -> Option<String> {
        self.external_tool_request.take()
    }

    fn follow_debugger(&mut self, ui: &Ui, registry: &Registry, session: &mut Session) {
        let now = ui.input(|input| input.time);
        if self.debugging.is_none_or(|(asked, _)| now - asked > SESSIONS_POLL) {
            let active = registry.dispatch(session, "debug.session.list", json!({})).ok().is_some_and(|answer| {
                answer["sessions"]
                    .as_array()
                    .is_some_and(|sessions| sessions.iter().any(|session| session["state"] != "ended"))
            });
            self.debugging = Some((now, active));
        }
        if !self.debugging.is_some_and(|(_, active)| active) {
            self.execution = None;
            self.stop_key = None;
            return;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(STATE_POLL));
        if self.execution.as_ref().is_none_or(|(asked, _)| now - asked >= STATE_POLL) {
            let state = registry.dispatch(session, "debug.session.get", json!({})).ok();
            let stopped = state.as_ref().and_then(stopped_at);
            self.stop_key = state
                .filter(|_| stopped.is_some())
                .map(|state| json!([state["frame"], state["stops"], state["values"]]));
            self.execution = Some((now, stopped));
        }
    }

    fn follow_bookmarks(&mut self, registry: &Registry, session: &mut Session, path: &std::path::Path) {
        let version = session.state::<Workbench>().and_then(Workbench::active_document).map_or(0, Document::version);
        let revision = session
            .state::<zen_workbench::bookmarks::Bookmarks>()
            .map_or(0, zen_workbench::bookmarks::Bookmarks::revision);
        if self.bookmarks.as_ref().is_some_and(|shown| shown.path == path && shown.key == (version, revision)) {
            return;
        }
        let marks = registry
            .dispatch(session, "navigate.bookmark.list", json!({ "path": path }))
            .ok()
            .and_then(|answer| answer["bookmarks"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(BookmarkMark::from_json)
            .collect();
        let revision = session
            .state::<zen_workbench::bookmarks::Bookmarks>()
            .map_or(0, zen_workbench::bookmarks::Bookmarks::revision);
        self.bookmarks = Some(BookmarksShown { path: path.to_owned(), key: (version, revision), marks });
    }

    pub(super) fn take_gutter_actions(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.gutter_actions)
    }

    pub(super) fn hovered_value(&self, ctx: &egui::Context) -> Option<(String, Rect)> {
        let (expression, anchor, frame) = self.value_hover.clone()?;
        (frame + 1 >= ctx.cumulative_frame_nr()).then_some((expression, anchor))
    }

    pub(super) fn take_value_anchor(&mut self) -> Option<Rect> {
        self.value_anchor.take()
    }

    pub(super) fn take_breakpoint_anchor(&mut self) -> Option<egui::Pos2> {
        self.breakpoint_anchor.take()
    }

    pub(super) fn forget_breakpoints(&mut self) {
        self.breakpoints = None;
    }

    fn follow_breakpoints(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, path: &std::path::Path) {
        let version = session.state::<Workbench>().and_then(Workbench::active_document).map_or(0, Document::version);
        let now = ui.input(|input| input.time);
        let every = if self.debugging.is_some_and(|(_, active)| active) { STATE_POLL * 2.0 } else { 2.0 };
        let fresh = self
            .breakpoints
            .as_ref()
            .is_some_and(|shown| shown.path == path && shown.version == version && now - shown.read <= every);
        if fresh {
            return;
        }
        let answer = registry.dispatch(session, "debug.breakpoint.list", json!({ "path": path })).unwrap_or_default();
        let muted = answer["muted"] == true;
        let marks = answer["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|breakpoint| breakpoint["path"].as_str().is_none_or(|of| std::path::Path::new(of) == path))
            .filter_map(|breakpoint| BreakpointMark::from_json(breakpoint, muted))
            .collect();
        self.breakpoints = Some(BreakpointsShown { path: path.to_owned(), version, read: now, marks });
    }

    fn follow_run_targets(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &std::path::Path,
        settled: bool,
    ) {
        let Some(version) = session.state::<Workbench>().and_then(Workbench::active_document).map(Document::version)
        else {
            return;
        };
        let key = (path.to_owned(), version);
        if self.run_targets.as_ref().is_some_and(|((shown, at), _)| *shown == key.0 && (*at == key.1 || !settled)) {
            return;
        }
        let listed: Vec<GutterTarget> = if zen_http::is_http_file(path) {
            http_targets(registry, session, path)
        } else if is_script(session, path) {
            sql_targets(registry, session, path)
        } else {
            let mut listed: Vec<GutterTarget> = registry
                .dispatch(session, "run.targets.list", json!({ "path": path, "full": true }))
                .ok()
                .and_then(|answer| answer["targets"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(GutterTarget::from_json)
                .collect();
            let sql: Vec<GutterTarget> = code_sql_targets(registry, session, path)
                .into_iter()
                .filter(|target| !listed.iter().any(|other| other.line == target.line))
                .collect();
            listed.extend(sql);
            listed
        };
        if self.run_menu.as_ref().is_some_and(|menu| !listed.contains(&menu.target)) {
            self.run_menu = None;
        }
        self.run_targets = Some((key, Arc::new(listed)));
    }

    fn follow_test_runs(&mut self, registry: &Registry, session: &mut Session) {
        let ended: Vec<u64> = session
            .state::<zen_workbench::run::Runs>()
            .map(|runs| {
                runs.runs()
                    .iter()
                    .filter(|run| !self.runs_seen.contains(&run.id) && !run.output.read(zen_run::RunOutput::is_running))
                    .map(|run| run.id)
                    .collect()
            })
            .unwrap_or_default();
        for run_id in ended {
            self.runs_seen.insert(run_id);
            let Ok(tests) = registry.dispatch(session, "run.tests.get", json!({ "runId": run_id })) else { continue };
            let mut found = Vec::new();
            top_level_tests(&tests["tree"], &mut found);
            for (package, name, mark) in found {
                self.test_marks.insert((package, name), mark);
            }
        }
    }

    fn test_mark_of(&self, target: &GutterTarget) -> Option<TestMark> {
        if target.package.is_empty() {
            return self.test_marks.iter().find(|((_, name), _)| *name == target.name).map(|(_, mark)| *mark);
        }
        self.test_marks.get(&(target.package.clone(), target.name.clone())).copied()
    }

    fn run_menu(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, palette: &IdePalette) {
        const ROW: f32 = 24.0;
        let Some(menu) = &mut self.run_menu else { return };
        let fresh = std::mem::take(&mut menu.fresh);
        let mut rows = vec![
            (
                zen_i18n::t!("ide-gutter-run", name = menu.target.name.as_str()),
                crate::keymap::shortcut("run.process.start/context"),
                Some(MenuIcon::Run),
                MenuChoice::Run,
            ),
            (
                zen_i18n::t!("ide-gutter-debug", name = menu.target.name.as_str()),
                crate::keymap::shortcut("debug.session.start/context"),
                Some(MenuIcon::Debug),
                MenuChoice::Debug,
            ),
        ];
        if menu.coverage {
            rows.push((
                zen_i18n::t!("ide-gutter-coverage", name = menu.target.name.as_str()),
                crate::keymap::shortcut("coverage.run.start"),
                Some(MenuIcon::Coverage),
                MenuChoice::Coverage,
            ));
        }
        rows.push((zen_i18n::t!("ide-gutter-modify-configuration"), None, None, MenuChoice::Modify));
        let mut chosen = None;
        let mut items = Vec::new();
        let area = egui::Area::new(ui.scope_id().with("run-icon-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.at)
            .show(ui.ctx(), |ui| {
                super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let width = rows
                        .iter()
                        .map(|(label, shortcut, _, _)| {
                            let label =
                                ui.painter().layout_no_wrap(label.clone(), crate::theme::regular(13.0), palette.text);
                            let shortcut = shortcut.as_ref().map_or(0.0, |shortcut| {
                                ui.painter()
                                    .layout_no_wrap(shortcut.clone(), crate::theme::regular(13.0), palette.text)
                                    .size()
                                    .x
                                    + 32.0
                            });
                            label.size().x + shortcut
                        })
                        .fold(0.0_f32, f32::max)
                        + 44.0;
                    for (label, shortcut, icon, choice) in &rows {
                        let (rect, response) = ui.allocate_exact_size(vec2(width.max(200.0), ROW), Sense::click());
                        accessibility::name(&response, egui::Role::MenuItem, label);
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 4, palette.selection);
                        }
                        let at = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(10.0));
                        match icon {
                            Some(MenuIcon::Run) => super::glyphs::glyph_run(ui.painter(), at, palette.run),
                            Some(MenuIcon::Debug) => {
                                super::glyphs::glyph_bug(ui.painter(), at.expand(1.0), palette.run);
                            }
                            Some(MenuIcon::Coverage) => {
                                super::glyphs::glyph_coverage(ui.painter(), at.expand(1.0), palette.run);
                            }
                            None => {}
                        }
                        ui.painter().text(
                            pos2(rect.left() + 28.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            crate::theme::regular(13.0),
                            palette.text,
                        );
                        if let Some(shortcut) = shortcut {
                            ui.painter().text(
                                pos2(rect.right() - 10.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                shortcut,
                                crate::theme::regular(13.0),
                                palette.text_secondary,
                            );
                        }
                        if response.clicked() {
                            chosen = Some(*choice);
                        }
                        items.push(rect);
                    }
                });
            });
        menu.items = items;
        let target = menu.target.clone();
        match chosen {
            Some(choice @ (MenuChoice::Run | MenuChoice::Coverage)) => {
                self.run_menu = None;
                let Some(context) = target_context(session, &target) else { return };
                let command = if choice == MenuChoice::Run { "run.process.start" } else { "coverage.run.start" };
                let started = registry.dispatch(session, command, json!({ "context": context }));
                match started {
                    Ok(_) => self.window_actions.push("view.toolWindow.run"),
                    Err(zen_core::DispatchError::Failed { error, .. }) => self.notice = Some(error.message),
                    Err(_) => {}
                }
            }
            Some(MenuChoice::Debug) => {
                self.run_menu = None;
                self.debug_request = Some(json!({ "configuration": target.configuration }));
            }
            Some(MenuChoice::Modify) => {
                self.run_menu = None;
                self.select_configuration_of(registry, session, &target);
                self.window_actions.push("view.runConfigurations.open");
            }
            None if !fresh && area.response.clicked_elsewhere() => self.run_menu = None,
            None => {}
        }
    }

    fn bookmark_menu(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>, path: &std::path::Path) {
        let Some(line) = self.gutter_menu_line else { return };
        let bookmark = self
            .bookmarks
            .as_ref()
            .filter(|shown| shown.path == path)
            .and_then(|shown| shown.marks.iter().find(|mark| mark.line == line))
            .cloned();
        let place = json!({ "path": path, "line": line + 1 });
        ui.menu_button(zen_i18n::t!("ide-gutter-bookmarks"), |ui| {
            let has = bookmark.is_some();
            if ui.add_enabled(has, egui::Button::new(zen_i18n::t!("ide-bookmark-another-list"))).clicked() {
                self.gutter_actions.push(("view.bookmark.moveToList".to_owned(), place.clone()));
                ui.close();
            }
            if ui.add_enabled(has, egui::Button::new(zen_i18n::t!("keymap-view-bookmark-rename"))).clicked() {
                self.gutter_actions.push(("view.bookmark.rename".to_owned(), place.clone()));
                ui.close();
            }
            if ui.button(zen_i18n::t!("keymap-navigate-bookmark-toggle")).clicked() {
                intents.push(Intent("navigate.bookmark.toggle", place.clone()));
                ui.close();
            }
            let mnemonic = bookmark.as_ref().is_some_and(|bookmark| bookmark.mnemonic.is_some());
            if ui
                .add_enabled(mnemonic, egui::Button::new(zen_i18n::t!("keymap-navigate-bookmark-removeMnemonic")))
                .clicked()
            {
                intents.push(Intent("navigate.bookmark.removeMnemonic", place.clone()));
                ui.close();
            }
            if ui.button(zen_i18n::t!("ide-bookmark-toggle-mnemonic")).clicked() {
                self.gutter_actions.push(("view.bookmark.mnemonic".to_owned(), place.clone()));
                ui.close();
            }
        });
    }

    fn run_http(&mut self, registry: &Registry, session: &mut Session, path: &std::path::Path, target: &GutterTarget) {
        let params = json!({ "path": path, "index": target.configuration["index"] });
        match registry.dispatch(session, "http.request.run", params) {
            Ok(_) => self.window_actions.push("view.toolWindow.services"),
            Err(zen_core::DispatchError::Failed { error, .. }) => self.notice = Some(error.message),
            Err(_) => {}
        }
    }

    fn select_configuration_of(&mut self, registry: &Registry, session: &mut Session, target: &GutterTarget) {
        let Some(context) = target_context(session, target) else { return };
        let Ok(answer) = registry.dispatch(session, "run.context.get", context) else { return };
        let result = match (answer["existing"].as_str(), answer.get("configuration")) {
            (Some(id), _) => registry.dispatch(session, "run.configuration.select", json!({ "id": id })),
            (None, Some(configuration)) if configuration.is_object() => {
                let mut configuration = configuration.clone();
                configuration["temporary"] = json!(true);
                registry.dispatch(
                    session,
                    "run.configuration.create",
                    json!({ "configuration": configuration, "select": true }),
                )
            }
            _ => return,
        };
        if let Err(zen_core::DispatchError::Failed { error, .. }) = result {
            self.notice = Some(error.message);
        }
    }

    pub(super) fn take_blame_request(&mut self) -> Option<String> {
        self.blame_request.take()
    }

    #[cfg(test)]
    pub(super) fn click_annotation(&mut self, commit: String) {
        self.blame_request = Some(commit);
    }

    #[allow(clippy::too_many_arguments, reason = "the gutter's geometry and where the rows are")]
    fn annotations(
        &mut self,
        ui: &mut Ui,
        intents: &mut Vec<Intent>,
        path: &std::path::Path,
        gutter_response: &egui::Response,
        (rows, layout): (&ShownRows, &RowLayout),
        (first, count, scrolled): (usize, usize, f32),
        (palette, indent_guides): (&IdePalette, bool),
    ) -> bool {
        let id = ui.make_persistent_id(("annotations", path));
        let line_height = self.line_height;
        let gutter = gutter_response.rect;
        let shown = self.blame.as_ref().is_some_and(|(_, _, _, info)| info.shown);
        let mut numbers = self.line_numbers_on;
        let no_menu = std::mem::take(&mut self.no_gutter_menu);
        let menu_breakpoint = self.gutter_menu_line.filter(|line| {
            self.breakpoints
                .as_ref()
                .filter(|shown| shown.path == path)
                .is_some_and(|shown| shown.marks.iter().any(|mark| mark.line == *line))
        });
        if !no_menu {
            gutter_response.context_menu(|ui| {
                let has_line = self.gutter_menu_line.is_some();
                let visible =
                    |slot: &str| has_line || !slot.starts_with("ide-gutter-") || slot == "ide-gutter-appearance";
                super::customization::popup(ui, super::customization::GUTTER_POPUP, visible, |ui, slot| match slot {
                    "keymap-vcs-blame-annotate" => {
                        let (title, show) = if shown {
                            (zen_i18n::t!("vcs-blame-close"), false)
                        } else {
                            (zen_i18n::t!("keymap-vcs-blame-annotate"), true)
                        };
                        if ui.button(title).clicked() {
                            intents.push(Intent("vcs.blame.annotate", json!({ "path": path, "show": show })));
                            self.blame = None;
                            ui.close();
                        }
                    }
                    "ide-gutter-bookmarks" => self.bookmark_menu(ui, intents, path),
                    "ide-gutter-remove-other-breakpoints" | "ide-gutter-disable-other-breakpoints" => {
                        let Some(line) = self.gutter_menu_line else { return };
                        let place = json!({ "path": path, "line": line });
                        let has = menu_breakpoint.is_some();
                        let command = if slot == "ide-gutter-remove-other-breakpoints" {
                            "debug.breakpoint.removeOthers"
                        } else {
                            "debug.breakpoint.disableOthers"
                        };
                        if ui.add_enabled(has, egui::Button::new(zen_i18n::t!(slot))).clicked() {
                            intents.push(Intent(command, place));
                            ui.close();
                        }
                    }
                    "ide-gutter-appearance" => {
                        ui.menu_button(zen_i18n::t!("ide-gutter-appearance"), |ui| {
                            if ui.checkbox(&mut numbers, zen_i18n::t!("keymap-editor-lineNumbers-toggle")).clicked() {
                                intents.push(Intent("editor.lineNumbers.toggle", json!({ "enabled": numbers })));
                                ui.close();
                            }
                            let mut on = indent_guides;
                            let title = super::keymap_page::action_title("editor.indentGuides.toggle");
                            if ui.checkbox(&mut on, title).changed() {
                                intents.push(Intent("editor.indentGuides.toggle", json!({ "enabled": on })));
                                ui.close();
                            }
                        });
                    }
                    _ => {}
                });
            });
        }
        let Some((_, _, _, info)) = self.blame.as_ref().filter(|(_, _, _, info)| info.shown) else { return false };
        let area = Rect::from_min_max(gutter.min, pos2(gutter.left() + ANNOTATIONS, gutter.bottom()));
        let painter = ui.painter().with_clip_rect(area);
        if info.pending {
            painter.text(
                pos2(area.left() + 8.0, area.top() + TOP + line_height / 2.0),
                Align2::LEFT_CENTER,
                zen_i18n::t!("vcs-blame-loading"),
                FontId::proportional(12.0),
                palette.text_secondary,
            );
            return false;
        }
        if let Some(error) = &info.error {
            painter.text(
                pos2(area.left() + 8.0, area.top() + TOP + line_height / 2.0),
                Align2::LEFT_CENTER,
                error.lines().next().unwrap_or_default(),
                FontId::proportional(12.0),
                palette.text_secondary,
            );
            return false;
        }
        let mut clicked = None;
        let hovered_commit =
            ui.input(|input| input.pointer.hover_pos()).filter(|pointer| area.contains(*pointer)).and_then(|pointer| {
                let row = layout.row_at(rows, pointer.y - gutter.top() - TOP + scrolled);
                rows.get(row).and_then(|line| info.lines.get(*line).cloned().flatten())
            });
        for (offset, &line) in rows.iter().enumerate().skip(first).take(count) {
            let Some(Some(annotation)) = info.lines.get(line) else { continue };
            let top = gutter.top() + TOP + layout.top(rows, offset) - scrolled;
            let row = Rect::from_min_size(pos2(area.left(), top), vec2(ANNOTATIONS - 6.0, line_height));
            let same = hovered_commit.as_ref().is_some_and(|hovered| hovered.commit == annotation.commit);
            let (ink, shade) = annotation_colors(palette, annotation.age);
            painter.rect_filled(row, 0, if same { palette.hover } else { shade });
            let date = super::vcs::format_date(annotation.time + i64::from(annotation.offset));
            let text = format!("{}  {}", &date[..10], annotation.author);
            painter.text(
                pos2(row.left() + 8.0, row.center().y),
                Align2::LEFT_CENTER,
                text,
                FontId::proportional(12.0),
                if same { palette.text } else { ink },
            );
            let response = ui.interact(row, id.with(("annotation", line)), Sense::click());
            accessibility::button_with(&response, || {
                format!("{}  {}  {}", &date[..10], annotation.author, annotation.subject)
            });
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let response = response.on_hover_text(format!(
                "{}  {}  {}\n{}",
                annotation.short_id, annotation.author, date, annotation.subject
            ));
            if response.clicked() {
                clicked = Some(annotation.commit.clone());
            }
        }
        if let Some(commit) = clicked {
            self.blame_request = Some(commit);
            return true;
        }
        false
    }

    pub(super) fn take_diff_request(&mut self) -> Option<PathBuf> {
        self.diff_request.take()
    }

    fn follow_coverage(&mut self, registry: &Registry, session: &mut Session, path: &std::path::Path) {
        let known = self.coverage.as_ref().filter(|(shown, _, _)| shown == path).map(|(_, revision, _)| *revision);
        let Ok(answer) = registry.dispatch(session, "coverage.file.get", json!({ "path": path, "known": known }))
        else {
            self.coverage = None;
            return;
        };
        if answer["unchanged"] == true {
            return;
        }
        let lines: Vec<zen_run::LineCoverage> = serde_json::from_value(answer["lines"].clone()).unwrap_or_default();
        self.coverage = Some((path.to_owned(), answer["revision"].as_u64().unwrap_or_default(), Arc::new(lines)));
    }

    fn coverage_marks(
        &mut self,
        ui: &Ui,
        gutter_response: &egui::Response,
        view: &GutterRows<'_>,
        palette: &IdePalette,
    ) {
        self.drawn_coverage.clear();
        let Some(lines) = self.coverage.as_ref().map(|(_, _, lines)| Arc::clone(lines)) else { return };
        if lines.is_empty() {
            return;
        }
        let x = fold_center(view.gutter) - 10.0;
        let line_height = self.line_height;
        let painter = ui.painter().with_clip_rect(view.gutter);
        let pointer = gutter_response.hover_pos();
        let mut hovered = None;
        let end = (view.first + view.count).min(view.rows.len());
        for row in view.first.min(end)..end {
            let line = view.rows[row];
            let Ok(index) = lines.binary_search_by_key(&(line as u32 + 1), |measured| measured.line) else { continue };
            let measured = lines[index];
            let top = view.gutter.top() + TOP + view.layout.top(view.rows, row) - view.scrolled;
            let bottom = top + view.rows.height(row) as f32 * line_height;
            let mark = Rect::from_min_max(pos2(x, top), pos2(x + 6.0, bottom));
            let color = coverage_color(palette, measured.status);
            painter.rect_filled(mark, 0, color);
            self.drawn_coverage.push((line, measured.status));
            if pointer.is_some_and(|pointer| mark.expand2(vec2(2.0, 0.0)).contains(pointer)) {
                hovered = Some(measured);
            }
        }
        if let Some(measured) = hovered {
            egui::Tooltip::for_widget(gutter_response).at_pointer().show(|ui| {
                ui.label(coverage_hint(&measured));
            });
        }
    }

    fn change_markers(
        &mut self,
        ui: &mut Ui,
        intents: &mut Vec<Intent>,
        path: &std::path::Path,
        view: &GutterRows<'_>,
        palette: &IdePalette,
    ) -> bool {
        if self.changes.is_empty() {
            self.change_popup = None;
            return false;
        }
        let x = fold_center(view.gutter) - 10.0;
        let line_height = self.line_height;
        let row_top = |row: usize| view.gutter.top() + TOP + view.layout.top(view.rows, row) - view.scrolled;
        let painter = ui.painter().with_clip_rect(view.gutter);
        let mut clicked = false;
        let shown = &view.rows[view.first.min(view.rows.len())..(view.first + view.count).min(view.rows.len())];
        for (index, change) in self.changes.iter().enumerate() {
            let rows: Vec<usize> = shown
                .iter()
                .enumerate()
                .filter(|(_, line)| {
                    if change.kind == "deleted" {
                        **line == change.start
                    } else {
                        (change.start..change.end).contains(*line)
                    }
                })
                .map(|(offset, _)| view.first + offset)
                .collect();
            let (Some(&first), Some(&last)) = (rows.first(), rows.last()) else { continue };
            let (rect, color) = if change.kind == "deleted" {
                let y = row_top(first);
                (Rect::from_min_max(pos2(x - 1.0, y - 4.0), pos2(x + 6.0, y + 4.0)), palette.gutter_deleted)
            } else {
                let color = if change.kind == "added" { palette.gutter_added } else { palette.gutter_modified };
                (
                    Rect::from_min_max(
                        pos2(x, row_top(first) + 1.0),
                        pos2(x + 3.0, row_top(last) + view.rows.height(last) as f32 * line_height - 1.0),
                    ),
                    color,
                )
            };
            let response = ui.interact(
                rect.expand2(vec2(3.0, 0.0)),
                ui.make_persistent_id(("change", path, index)),
                Sense::click(),
            );
            accessibility::button_with(&response, || t!("a11y-change", kind = change.kind.as_str()));
            let color = if response.hovered() { palette.text_secondary } else { color };
            if change.kind == "deleted" {
                let y = rect.center().y;
                painter.add(egui::Shape::convex_polygon(
                    vec![pos2(rect.left(), y - 4.0), pos2(rect.right(), y), pos2(rect.left(), y + 4.0)],
                    color,
                    Stroke::NONE,
                ));
            } else {
                painter.rect_filled(rect, 1, color);
            }
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                clicked = true;
                self.change_popup = Some((index, pos2(view.gutter.right(), rect.bottom())));
            }
        }
        self.change_popup_ui(ui, intents, path, palette);
        clicked
    }

    fn change_popup_ui(
        &mut self,
        ui: &mut Ui,
        intents: &mut Vec<Intent>,
        path: &std::path::Path,
        palette: &IdePalette,
    ) {
        let Some((index, at)) = self.change_popup else { return };
        let Some(change) = self.changes.get(index).cloned() else {
            self.change_popup = None;
            return;
        };
        let (mut rollback, mut diff) = (false, false);
        let area = egui::Area::new(ui.make_persistent_id(("change-popup", path)))
            .order(egui::Order::Foreground)
            .fixed_pos(at + vec2(4.0, 2.0))
            .show(ui.ctx(), |ui| {
                super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_max_width(640.0);
                    ui.horizontal(|ui| {
                        let link = |ui: &mut Ui, text: String| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(text).color(palette.link)).sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        };
                        rollback = link(ui, zen_i18n::t!("vcs-change-rollback"));
                        ui.add_space(12.0);
                        diff = link(ui, zen_i18n::t!("vcs-change-diff"));
                    });
                    if !change.before.is_empty() {
                        ui.add_space(6.0);
                        let shown: String = change.before.lines().take(12).collect::<Vec<_>>().join("\n");
                        egui::Frame::new().fill(palette.diff_deleted).corner_radius(4).inner_margin(6).show(ui, |ui| {
                            ui.label(egui::RichText::new(shown).font(FontId::monospace(12.5)).color(palette.code));
                        });
                    }
                });
            });
        let outside = ui.input(|input| input.pointer.any_pressed())
            && !area.response.rect.contains(ui.input(|input| input.pointer.interact_pos()).unwrap_or_default());
        if rollback {
            let line = change.start;
            intents.push(Intent("vcs.lineStatus.rollback", json!({ "path": path, "line": line })));
            self.change_popup = None;
        } else if diff {
            self.diff_request = Some(path.to_owned());
            self.change_popup = None;
        } else if outside {
            self.change_popup = None;
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.change_popup = None;
        }
    }

    fn show_usages(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, palette: &IdePalette) {
        use zen_workbench::language::show_usages::{ShowUsages, view};
        let Some((generation, open, pending)) =
            session.state::<ShowUsages>().map(|state| (state.generation(), state.is_open(), state.is_pending()))
        else {
            return;
        };
        if !open || self.passive {
            self.usages.hide();
            return;
        }
        if self.usages.generation() != Some(generation) || pending {
            if registry.dispatch(session, "navigate.showUsages.get", json!({})).is_ok() {
                self.usages.update(view(session));
            }
            if pending {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        self.usages.follow_source(session);
        let actions = self.usages.show(ui, palette, self.caret_place());
        self.usages_actions(registry, session, actions);
    }

    fn usages_actions(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        actions: Vec<super::usages_popup::PopupAction>,
    ) {
        for action in actions {
            let (command, params) = action.command();
            if let Err(error) = registry.dispatch(session, command, params) {
                self.notice = Some(error.to_string());
            }
            if matches!(action, super::usages_popup::PopupAction::GoTo { .. } | super::usages_popup::PopupAction::Close)
            {
                self.focus();
            }
        }
    }

    fn navigation(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, palette: &IdePalette) {
        if zen_workbench::language::goto::lookup_in_progress(session)
            && let Ok(state) = registry.dispatch(session, "navigate.lookup.poll", json!({}))
        {
            let several = state["targets"].as_array().is_some_and(|targets| targets.len() > 1);
            if state["pending"] == true {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            } else {
                let clicked = self.lookup_at.take();
                if let Some(error) = state["error"].as_str() {
                    self.notice = Some(error.to_owned());
                } else if state["navigated"] == false && (several || state["createTest"] == true) {
                    let at = clicked.map_or(PopupAnchor::UNDER_CARET, PopupAnchor::At);
                    let root = session.state::<Workbench>().and_then(Workbench::project);
                    self.targets.open(&state, at, root);
                }
            }
        }
        if let Some(target) = self.targets.show(ui, palette, self.caret_place()) {
            self.target_chosen(registry, session, target);
            self.focus();
        }
        let caret = self.caret_rect.map_or(egui::Pos2::ZERO, |caret| caret.left_bottom() + vec2(-24.0, 2.0));
        if let Some(start) = self.regions.show(ui, palette, caret) {
            let path = session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
            if let Some(path) = path {
                let _ = registry.dispatch(session, "navigate.place.goTo", json!({ "path": path, "offset": start }));
            }
            self.focus();
        }
    }

    fn target_chosen(&mut self, registry: &Registry, session: &mut Session, target: Value) {
        if target["createTest"] == true {
            self.window_actions.push("view.createTest.open");
        } else {
            let _ = registry.dispatch(session, "navigate.place.goTo", target);
        }
    }

    fn apply_unwrap(&mut self, registry: &Registry, session: &mut Session, params: Value) {
        match registry.dispatch(session, "editor.unwrap.apply", params) {
            Ok(_) => self.reveal = true,
            Err(error) => self.notice = Some(error.to_string()),
        }
    }

    fn command_of(&self, binding: &zen_core::keymap::Binding) -> Option<(String, Value)> {
        let mut params = binding.params.clone();
        if params["movement"]["lines"] == 0 {
            params["movement"]["lines"] = json!(self.page_lines);
        }
        Some((binding.command.clone(), params))
    }

    fn replace_previews(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        hints: Arc<Vec<inlay_hints::ShownHint>>,
    ) -> Arc<Vec<inlay_hints::ShownHint>> {
        let Some(workbench) = session.state::<Workbench>() else { return hints };
        let find = workbench.find();
        let Some(document) = workbench.active_document() else { return hints };
        if !(find.open && find.replace_open && find.query.regex && !find.query.text.is_empty()) {
            self.previews = None;
            return hints;
        }
        let key = (document.version(), find.query.clone(), find.replacement.clone(), document.find_excluded().to_vec());
        let shown = self.shown_lines;
        if let Some((known, lines, base, merged)) = &self.previews
            && *known == key
            && lines.0 <= shown.0
            && shown.1 <= lines.1
            && (Arc::ptr_eq(base, &hints) || base.is_empty() && hints.is_empty())
        {
            return Arc::clone(merged);
        }
        let lines = (shown.0.saturating_sub(PREVIEW_MARGIN), shown.1.saturating_add(PREVIEW_MARGIN));
        let params = json!({ "fromLine": lines.0 + 1, "toLine": lines.1 + 1 });
        let preview = registry.dispatch(session, "editor.find.preview", params).unwrap_or_default();
        let mut all: Vec<inlay_hints::ShownHint> = hints.as_ref().clone();
        for (index, item) in preview["matches"].as_array().into_iter().flatten().enumerate() {
            let (Some(end), Some(label)) = (item["end"].as_u64(), item["replacement"].as_str()) else { continue };
            if item["excluded"] == true || label.is_empty() {
                continue;
            }
            all.push(inlay_hints::ShownHint {
                id: u64::MAX - index as u64,
                offset: end as usize,
                label: label.replace('\n', "⏎"),
                padding_left: true,
                padding_right: false,
                tooltip: None,
                resolvable: false,
            });
        }
        all.sort_by_key(|hint| hint.offset);
        let merged = Arc::new(all);
        self.previews = Some((key, lines, hints, Arc::clone(&merged)));
        merged
    }

    #[allow(clippy::too_many_arguments, reason = "what the highlighting and the line index are made from")]
    fn refresh(
        &mut self,
        ui: &Ui,
        document: &Document,
        highlighter: Option<Arc<dyn Highlighter>>,
        highlighters: Option<&zen_workbench::brackets::Highlighters>,
        palette: &IdePalette,
        editor: &EditorMetrics,
    ) {
        let scale = ui.ctx().pixels_per_point().to_bits();
        let font = Some((editor.font.clone(), editor.line_height.to_bits()));
        let atlas = crate::fonts::atlas_generation(ui.ctx());
        if scale != self.galley_scale
            || atlas != self.galley_atlas
            || self.galleys.len() > GALLEY_CACHE
            || self.galley_styles != Some(palette.styles)
            || self.galley_font != font
        {
            self.galleys.clear();
            self.galley_scale = scale;
            self.galley_styles = Some(palette.styles);
            self.galley_font = font;
            self.galley_atlas = atlas;
        }
        let key = highlighter.as_ref().map_or(0, |h| Arc::as_ptr(h).cast::<()>() as usize);
        let fresh = self.highlight.as_ref().is_some_and(|cache| {
            cache.path == document.path()
                && cache.version == document.version()
                && cache.highlighter == key
                && cache.tab_size == editor.tab_size
        });
        if fresh {
            return;
        }
        let text = document.text();
        let previous = self.highlight.take().filter(|cache| cache.path == document.path());
        let changes = previous.as_ref().and_then(|cache| document.changes_since(cache.version));
        let kept = previous.as_ref().and_then(|cache| cache.highlighting.clone().filter(|_| cache.highlighter == key));
        let highlighting = highlighter.as_ref().map(|highlighter| match (highlighters, kept) {
            (Some(highlighters), _) => highlighters.highlighting(document, highlighter),
            (None, Some(kept)) => {
                kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner).update(text, changes.as_deref());
                kept
            }
            (None, None) => Arc::new(std::sync::Mutex::new(Highlighting::new(Arc::clone(highlighter), text))),
        });
        if let Some(highlighting) = &highlighting {
            crate::capture::note_lexed(highlighting.lock().unwrap_or_else(std::sync::PoisonError::into_inner).lexed());
        }
        let merged = changes.as_deref().and_then(TextChange::merged);
        let (line_bytes, widths, max_columns) = match (previous, merged) {
            (Some(cache), Some(change)) if cache.tab_size == editor.tab_size => {
                let (mut starts, mut widths) = (cache.line_bytes, cache.widths);
                let widest = follow_lines(&mut starts, &mut widths, cache.max_columns, text, change, editor.tab_size);
                (starts, widths, widest)
            }
            (Some(cache), None) if changes.is_some() && cache.tab_size == editor.tab_size => {
                (cache.line_bytes, cache.widths, cache.max_columns)
            }
            _ => line_starts(text, editor.tab_size),
        };
        self.highlight = Some(HighlightCache {
            path: document.path().to_owned(),
            version: document.version(),
            highlighter: key,
            highlighted_by: highlighter,
            highlighting,
            line_bytes,
            widths,
            max_columns,
            tab_size: editor.tab_size,
        });
    }

    fn follow_peek(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        path: &std::path::Path,
        hovered: Option<(usize, usize, Rect)>,
    ) {
        let same = |peek: &PeekHover| {
            hovered.is_some_and(|(start, end, _)| (peek.path.as_path(), peek.start, peek.end) == (path, start, end))
        };
        if self.peek.as_ref().is_some_and(|peek| !same(peek)) {
            if self.peek.take().is_some_and(|peek| peek.asked) {
                let _ = registry.dispatch(session, "editor.declarationPeek.hide", json!({}));
            }
            ui.ctx().request_repaint();
        }
        let Some((start, end, anchor)) = hovered else { return };
        let peek = self.peek.get_or_insert_with(|| {
            ui.ctx().request_repaint();
            PeekHover {
                path: path.to_owned(),
                start,
                end,
                anchor,
                since: std::time::Instant::now(),
                asked: false,
                state: None,
            }
        });
        peek.anchor = anchor;
        let waited = peek.since.elapsed();
        if waited < PEEK_DELAY {
            ui.ctx().request_repaint_after(PEEK_DELAY - waited);
        }
        if !peek.asked {
            peek.asked = true;
            peek.state = registry
                .dispatch(session, "editor.declarationPeek.show", json!({ "path": path, "offset": start }))
                .ok();
        } else if peek.state.as_ref().is_none_or(|state| state["pending"] == true) {
            peek.state = registry.dispatch(session, "editor.declarationPeek.state", json!({})).ok();
        }
        if peek.state.as_ref().is_some_and(|state| state["declaration"].is_string()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if peek.state.as_ref().is_some_and(|state| state["pending"] == true) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn show_peek(&self, ui: &Ui, palette: &IdePalette, highlighter: Option<&Arc<dyn Highlighter>>) {
        let Some(peek) = self.peek.as_ref().filter(|peek| peek.since.elapsed() >= PEEK_DELAY) else { return };
        let Some(state) = &peek.state else { return };
        let Some(declaration) = state["declaration"].as_str() else { return };
        let language = state["language"].as_str().unwrap_or_default();
        egui::Area::new(ui.scope_id().with("declaration-peek"))
            .order(egui::Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(peek.anchor.left_top() + vec2(-8.0, -4.0))
            .interactable(false)
            .show(ui.ctx(), |ui| {
                super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                    let job = super::doc_popups::code_job(declaration, language, palette, highlighter, 640.0);
                    ui.add(egui::Label::new(job).extend());
                });
            });
    }

    fn brackets_at(&mut self, document: &Document, cache: &HighlightCache) -> Vec<(usize, SemanticKind)> {
        let head = document.selections().primary().head;
        let key = (document.path().to_owned(), document.version(), cache.highlighter, head);
        if self.brackets.as_ref().map(|(at, _)| at) != Some(&key) {
            let angle = cache.highlighted_by.as_ref().is_some_and(|highlighter| highlighter.angle_brackets());
            let highlighting = cache
                .highlighting
                .as_ref()
                .map(|highlighting| highlighting.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            let text = document.text();
            let found =
                zen_editor::matching_bracket(document.snapshot(), head, zen_editor::bracket_pairs(angle), &|byte| {
                    highlighting.as_ref().is_some_and(|highlighting| highlighting.in_string_or_comment(text, byte))
                });
            self.brackets = Some((key, found));
        }
        match self.brackets.as_ref().and_then(|(_, found)| *found) {
            Some(zen_editor::BracketMatch { bracket, matching: Some(matching) }) => {
                vec![(bracket, SemanticKind::MatchedBracket), (matching, SemanticKind::MatchedBracket)]
            }
            Some(zen_editor::BracketMatch { bracket, matching: None }) => {
                vec![(bracket, SemanticKind::UnmatchedBracket)]
            }
            None => Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments, reason = "the pieces of one line's layout")]
    fn galley(
        &mut self,
        ui: &Ui,
        text: &str,
        cache: &HighlightCache,
        line: usize,
        (display, columns, placed): (&str, &[usize], &[Placed]),
        palette: &IdePalette,
        font: &FontId,
        semantic: (usize, &[(usize, usize, SemanticKind)]),
    ) -> Arc<egui::Galley> {
        let all = 0..columns.len() - 1;
        self.galley_range(ui, text, cache, line, (display, columns, placed), palette, font, semantic, all)
    }

    #[allow(clippy::too_many_arguments, reason = "the pieces of one line's layout")]
    fn galley_range(
        &mut self,
        ui: &Ui,
        text: &str,
        cache: &HighlightCache,
        line: usize,
        (display, columns, placed): (&str, &[usize], &[Placed]),
        palette: &IdePalette,
        font: &FontId,
        (line_chars, semantic): (usize, &[(usize, usize, SemanticKind)]),
        range: std::ops::Range<usize>,
    ) -> Arc<egui::Galley> {
        let shown_from = columns[range.start];
        let shown_to = if range.end + 1 >= columns.len() { display.chars().count() } else { columns[range.end] };
        let display: std::borrow::Cow<str> = if shown_from == 0 && shown_to == display.chars().count() {
            std::borrow::Cow::Borrowed(display)
        } else {
            std::borrow::Cow::Owned(display.chars().skip(shown_from).take(shown_to - shown_from).collect())
        };
        let placed = inlay_hints::on_row(placed, shown_from..shown_to);
        let start = cache.line_bytes[line];
        let raw = line_text(text, cache, line);
        let end = start + raw.len();
        let spans: Vec<Span> = cache.spans(text, start..end);
        let line_end_chars = line_chars + raw.chars().count();
        let first_token = semantic.partition_point(|(_, token_end, _)| *token_end <= line_chars);
        let tokens: Vec<(usize, usize, SemanticKind)> = semantic[first_token..]
            .iter()
            .take_while(|(token_start, _, _)| *token_start < line_end_chars)
            .map(|&(token_start, token_end, kind)| {
                (token_start.saturating_sub(line_chars), token_end - line_chars, kind)
            })
            .collect();
        let line_height = self.line_height;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        display.hash(&mut hasher);
        for span in &spans {
            (span.range.start.saturating_sub(start), span.range.end.min(end) - start, span.kind as u8)
                .hash(&mut hasher);
        }
        tokens.hash(&mut hasher);
        range.hash(&mut hasher);
        for place in &placed {
            (place.display, place.width.to_bits()).hash(&mut hasher);
        }
        self.ligatures.hash(&mut hasher);
        let key = hasher.finish();
        if let Some(galley) = self.galleys.get(&key) {
            return Arc::clone(galley);
        }
        let format = |style: CodeStyle| {
            let mut format = TextFormat { line_height: Some(line_height), ..style.format(font.clone()) };
            format.underline.width = 0.0;
            format
        };
        let plain = palette.style(SyntaxKey::Default);
        let mut job = LayoutJob::default();
        let display_chars: Vec<char> = display.chars().collect();
        let mut span = 0;
        let mut piece = String::new();
        let mut piece_style = None;
        for (column, (byte, _)) in raw.char_indices().enumerate().take(range.end).skip(range.start) {
            let at = start + byte;
            while span < spans.len() && spans[span].range.end <= at {
                span += 1;
            }
            let lexed = spans.get(span).filter(|span| span.range.start <= at).map(|span| palette.syntax(span.kind));
            let style = match tokens.iter().find(|(from, to, _)| (*from..*to).contains(&column)) {
                Some((_, _, SemanticKind::Package)) => Some(palette.style(SyntaxKey::SemanticPackage)),
                Some((_, _, SemanticKind::Type)) => Some(palette.style(SyntaxKey::SemanticType)),
                Some((_, _, SemanticKind::BuiltinType)) => Some(palette.style(SyntaxKey::SemanticBuiltinType)),
                Some((_, _, SemanticKind::Call)) => Some(palette.style(SyntaxKey::SemanticCall)),
                Some((_, _, SemanticKind::Function)) => Some(palette.style(SyntaxKey::Function)),
                Some((_, _, SemanticKind::Link)) => Some(palette.style(SyntaxKey::NavigationLink)),
                Some((_, _, SemanticKind::DocName)) => Some(palette.style(SyntaxKey::DocName)),
                Some((_, _, SemanticKind::Constant)) => Some(palette.style(SyntaxKey::Constant)),
                Some((_, _, SemanticKind::Property)) => Some(palette.style(SyntaxKey::Property)),
                Some((_, _, SemanticKind::GlobalVariable)) => Some(palette.style(SyntaxKey::GlobalVariable)),
                Some((_, _, SemanticKind::MatchedBracket)) => {
                    Some(CodeStyle { color: palette.bracket_matched, ..lexed.unwrap_or(plain) })
                }
                Some((_, _, SemanticKind::UnmatchedBracket)) => {
                    Some(CodeStyle { color: palette.bracket_unmatched, ..lexed.unwrap_or(plain) })
                }
                Some((_, _, SemanticKind::Todo(own))) => {
                    let todo = palette.style(SyntaxKey::Todo);
                    Some(CodeStyle { color: own.unwrap_or(todo.color), ..todo })
                }
                Some((_, _, SemanticKind::Unused)) => Some(CodeStyle {
                    color: palette.unused,
                    background: None,
                    underline: None,
                    ..lexed.unwrap_or(plain)
                }),
                None => lexed,
            };
            if piece_style != Some(style) && !piece.is_empty() {
                job.append(&piece, 0.0, format(piece_style.flatten().unwrap_or(plain)));
                piece.clear();
            }
            piece_style = Some(style);
            piece.extend(&display_chars[columns[column] - shown_from..columns[column + 1] - shown_from]);
        }
        job.append(&piece, 0.0, format(piece_style.flatten().unwrap_or(plain)));
        if !self.ligatures {
            job.sections = job
                .sections
                .iter()
                .flat_map(|section| {
                    let start = section.byte_range.start.0;
                    let text = &job.text[start..section.byte_range.end.0];
                    text.char_indices().map(move |(at, c)| egui::text::LayoutSection {
                        leading_space: 0.0,
                        byte_range: egui::text::ByteIndex(start + at)..egui::text::ByteIndex(start + at + c.len_utf8()),
                        format: section.format.clone(),
                    })
                })
                .collect();
        }
        inlay_hints::make_room(&mut job, &display, &placed, || ui.fonts_mut(|fonts| fonts.glyph_width(font, ' ')));
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        self.galleys.insert(key, Arc::clone(&galley));
        galley
    }
}

fn profile(spent: std::time::Duration) {
    static STATE: std::sync::Mutex<(Option<std::time::Instant>, u32, f64, f64)> =
        std::sync::Mutex::new((None, 0, 0.0, 0.0));
    if std::env::var_os("ZEN_UI_PROFILE").is_none() {
        return;
    }
    let Ok(mut state) = STATE.lock() else { return };
    let now = std::time::Instant::now();
    let ms = spent.as_secs_f64() * 1000.0;
    state.1 += 1;
    state.2 += ms;
    state.3 = state.3.max(ms);
    match state.0 {
        None => state.0 = Some(now),
        Some(started) if now.duration_since(started).as_secs_f64() >= 2.0 => {
            #[allow(clippy::print_stderr, reason = "a developer tool, run from a terminal")]
            {
                eprintln!(
                    "editor: {:.2} ms mean, {:.2} ms longest, {} frames",
                    state.2 / f64::from(state.1),
                    state.3,
                    state.1
                );
            }
            *state = (Some(now), 0, 0.0, 0.0);
        }
        Some(_) => {}
    }
}

fn annotation_colors(palette: &IdePalette, age: u8) -> (Color32, Color32) {
    let old = f32::from(age.min(100)) / 100.0;
    let ink = crate::icons::mix(palette.annotation_recent, palette.annotation_old, old);
    let shade = palette.annotation_recent.gamma_multiply(0.1 * (1.0 - old));
    (ink, shade)
}

fn gutter_width(lines: usize, char_width: f32, annotated: bool) -> f32 {
    (if annotated { ANNOTATIONS } else { 0.0 })
        + gutter::NUMBERS_LEFT
        + (lines.to_string().len().max(2) as f32) * char_width
        + gutter::NUMBERS_TO_FOLD
        + gutter::FOLD_TO_SEPARATOR
        + gutter::SEPARATOR
}

fn numbers_right(gutter: Rect, lines: usize, char_width: f32, annotated: bool) -> f32 {
    gutter.left()
        + (if annotated { ANNOTATIONS } else { 0.0 })
        + gutter::NUMBERS_LEFT
        + (lines.to_string().len().max(2) as f32) * char_width
}

fn fold_center(gutter: Rect) -> f32 {
    gutter.right() - gutter::SEPARATOR - gutter::FOLD_TO_SEPARATOR
}

fn line_text<'a>(text: &'a str, cache: &HighlightCache, line: usize) -> &'a str {
    let raw = &text[cache.line_bytes[line]..cache.line_bytes[line + 1]];
    raw.strip_suffix("\r\n").or_else(|| raw.strip_suffix('\n')).or_else(|| raw.strip_suffix('\r')).unwrap_or(raw)
}

fn line_starts(text: &str, tab: usize) -> (Vec<usize>, Vec<usize>, usize) {
    let (mut starts, widths) = lines_between(text, 0, text.len(), true, tab);
    starts.push(text.len());
    let widest = widths.iter().copied().max().unwrap_or(0);
    (starts, widths, widest)
}

fn lines_between(text: &str, from: usize, to: usize, tail: bool, tab: usize) -> (Vec<usize>, Vec<usize>) {
    let mut starts = vec![from];
    let mut widths = Vec::new();
    let mut column = 0;
    let bytes = text.as_bytes();
    for (index, c) in text[from..to].char_indices() {
        let index = from + index;
        let breaks = match c {
            '\n' => true,
            '\r' => bytes.get(index + 1) != Some(&b'\n'),
            _ => false,
        };
        if breaks {
            widths.push(column);
            column = 0;
            if index + 1 < to || tail {
                starts.push(index + 1);
            }
            continue;
        }
        match c {
            '\r' => {}
            '\t' => column += tab - column % tab,
            _ => column += 1,
        }
    }
    if starts.len() > widths.len() {
        widths.push(column);
    }
    (starts, widths)
}

fn follow_lines(
    starts: &mut Vec<usize>,
    widths: &mut Vec<usize>,
    widest: usize,
    text: &str,
    change: TextChange,
    tab: usize,
) -> usize {
    let lines = widths.len();
    let old_len = starts[lines];
    let line_of = |offset: usize| starts.partition_point(|start| *start <= offset).saturating_sub(1).min(lines - 1);
    let first = line_of(change.start).saturating_sub(1);
    let last = (line_of(change.old_end) + 1).min(lines - 1);
    let delta = text.len() as isize - old_len as isize;
    let to = (starts[last + 1] as isize + delta) as usize;
    let (fresh_starts, fresh_widths) = lines_between(text, starts[first], to, last + 1 == lines, tab);
    for start in &mut starts[last + 1..] {
        *start = (*start as isize + delta) as usize;
    }
    starts.splice(first..=last, fresh_starts);
    let fresh_widest = fresh_widths.iter().copied().max().unwrap_or(0);
    let lost_widest = widths.splice(first..=last, fresh_widths).any(|width| width == widest);
    if fresh_widest >= widest {
        fresh_widest
    } else if lost_widest {
        widths.iter().copied().max().unwrap_or(0)
    } else {
        widest
    }
}

fn display_line(line: &str, tab: usize) -> (String, Vec<usize>) {
    let mut display = String::with_capacity(line.len());
    let mut columns = Vec::with_capacity(line.len() + 1);
    let mut column = 0;
    for c in line.chars() {
        columns.push(column);
        if c == '\t' {
            let width = tab - column % tab;
            display.extend(std::iter::repeat_n(' ', width));
            column += width;
        } else {
            display.push(c);
            column += 1;
        }
    }
    columns.push(column);
    (display, columns)
}

fn unit_at(text: &zen_text::Text, offset: usize, clicks: u8, humps: bool) -> (usize, usize) {
    let point = text.offset_to_point(offset).unwrap_or_default();
    let line_start = offset - point.column;
    match clicks {
        2 => {
            let word = zen_editor::double_click_word(text, offset, humps);
            (word.start, word.end)
        }
        3 => {
            let end = text.point_to_offset(zen_text::Point::new(point.line + 1, 0)).unwrap_or(text.len_chars());
            (line_start, end)
        }
        _ => (offset, offset),
    }
}

fn copy_text(document: &Document) -> (String, Option<Vec<Value>>) {
    let text = document.snapshot();
    let selections = document.selections();
    if selections.iter().any(|selection| !selection.is_empty()) {
        let pieces: Vec<String> = selections
            .iter()
            .filter(|selection| !selection.is_empty())
            .map(|selection| text.slice(selection.range()).map(|slice| slice.into_owned()).unwrap_or_default())
            .collect();
        return (pieces.join("\n"), None);
    }
    let mut copied = String::new();
    let mut lines = Vec::new();
    for selection in selections.iter() {
        let line = text.offset_to_point(selection.head).unwrap_or_default().line;
        let start = text.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or_default();
        let end = text.point_to_offset(zen_text::Point::new(line + 1, 0)).unwrap_or(text.len_chars());
        let piece = text.slice(start..end).map(|slice| slice.into_owned()).unwrap_or_default();
        copied.push_str(&piece);
        if !piece.ends_with('\n') && !piece.ends_with('\r') {
            copied.push('\n');
        }
        lines.push(json!({ "anchor": start, "head": end }));
    }
    (copied, Some(lines))
}

fn problems_under(
    at: &soft_wrap::PointedRow,
    line_start: usize,
    spans: &[(usize, usize)],
    (char_width, line_height): (f32, f32),
    pos: egui::Pos2,
) -> Vec<usize> {
    if pos.y < at.top || pos.y > at.top + line_height {
        return Vec::new();
    }
    let (row_start, row_end) = (line_start + at.segment.start, line_start + at.segment.end);
    spans
        .iter()
        .enumerate()
        .filter(|&(_, &(start, end))| start <= row_end && end >= row_start && (start < row_end || !at.wrapped))
        .filter(|&(_, &(start, end))| {
            let left = at.x_after(start.max(row_start) - line_start);
            let mut right = at.x_of(end.min(row_end) - line_start);
            if right - left < char_width {
                right = left + char_width;
            }
            (left..=right).contains(&pos.x)
        })
        .map(|(index, _)| index)
        .collect()
}

struct ShownProblem {
    start: usize,
    end: usize,
    line: usize,
    severity: String,
    message: String,
    deprecated: bool,
    unused: bool,
    raw: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProblemMark {
    Wavy,
    StrikeOut,
    None,
}

impl ShownProblem {
    fn from_json(problem: &Value) -> Option<Self> {
        Some(Self {
            start: problem["start"].as_u64()? as usize,
            end: problem["end"].as_u64()? as usize,
            line: problem["line"].as_u64()?.max(1) as usize,
            severity: problem["severity"].as_str()?.to_owned(),
            message: problem["message"].as_str().unwrap_or_default().to_owned(),
            deprecated: problem["deprecated"] == true,
            unused: problem["unused"] == true,
            raw: problem.clone(),
        })
    }

    fn mark(&self) -> ProblemMark {
        if self.deprecated {
            ProblemMark::StrikeOut
        } else if self.unused && self.severity != "error" {
            ProblemMark::None
        } else {
            ProblemMark::Wavy
        }
    }

    fn rank(&self) -> u8 {
        match self.severity.as_str() {
            "error" => 5,
            "warning" => 4,
            "weakWarning" => 3,
            "typo" => 2,
            "information" => 1,
            _ => 0,
        }
    }
}

fn visible_semantic(
    text: &zen_text::Text,
    tokens: &[SemanticSpan],
    link: Option<(usize, usize)>,
    lines: std::ops::Range<usize>,
) -> Vec<SemanticSpan> {
    let start = text.point_to_offset(zen_text::Point::new(lines.start, 0)).unwrap_or_default();
    let end = text.point_to_offset(zen_text::Point::new(lines.end, 0)).unwrap_or(text.len_chars());
    let first = tokens.partition_point(|(_, token_end, _)| *token_end <= start);
    let mut shown: Vec<SemanticSpan> =
        tokens[first..].iter().take_while(|(token_start, _, _)| *token_start < end).copied().collect();
    if let Some((link_start, link_end)) = link {
        shown.retain(|(token_start, token_end, _)| *token_end <= link_start || *token_start >= link_end);
        shown.push((link_start, link_end, SemanticKind::Link));
    }
    shown.extend(
        zen_editor::doc_comment_names_in(text, lines)
            .into_iter()
            .map(|name| (name.start, name.end, SemanticKind::DocName)),
    );
    shown.sort_by_key(|(token_start, _, _)| *token_start);
    shown
}

fn with_unused(semantic: Vec<SemanticSpan>, unused: impl IntoIterator<Item = (usize, usize)>) -> Vec<SemanticSpan> {
    let mut ranges: Vec<(usize, usize)> = unused.into_iter().filter(|(start, end)| start < end).collect();
    if ranges.is_empty() {
        return semantic;
    }
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut spans: Vec<SemanticSpan> = Vec::with_capacity(semantic.len() + merged.len());
    for (start, end, kind) in semantic {
        let mut from = start;
        for &(unused_start, unused_end) in merged.iter().filter(|(s, e)| *s < end && *e > start) {
            if from < unused_start {
                spans.push((from, unused_start, kind));
            }
            from = from.max(unused_end);
        }
        if from < end {
            spans.push((from, end, kind));
        }
    }
    spans.extend(merged.into_iter().map(|(start, end)| (start, end, SemanticKind::Unused)));
    spans.sort_by_key(|(token_start, _, _)| *token_start);
    spans
}

fn semantic_kind(token: &Value) -> Option<SemanticKind> {
    let has = |modifier: &str| token["modifiers"].as_array().is_some_and(|list| list.iter().any(|m| m == modifier));
    let library = has("defaultLibrary");
    Some(match token["kind"].as_str()? {
        "namespace" => SemanticKind::Package,
        "type" | "class" | "interface" | "struct" | "typeParameter" | "enum" if !library => SemanticKind::Type,
        "type" => SemanticKind::BuiltinType,
        "function" | "method" if !library && !has("definition") && !has("declaration") => SemanticKind::Call,
        "variable" if has("readonly") && !library => SemanticKind::Constant,
        "enumMember" if !library => SemanticKind::Constant,
        _ => return None,
    })
}

fn is_web_script(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts"))
}

fn web_semantic_kind(token: &Value) -> Option<SemanticKind> {
    let has = |modifier: &str| token["modifiers"].as_array().is_some_and(|list| list.iter().any(|m| m == modifier));
    let declared = has("declaration") || has("definition");
    Some(match token["kind"].as_str()? {
        "function" | "method" if !declared => SemanticKind::Function,
        "property" if declared => SemanticKind::Property,
        "variable" if has("defaultLibrary") || !has("local") => SemanticKind::GlobalVariable,
        "enumMember" => SemanticKind::Constant,
        "namespace" => SemanticKind::Package,
        _ => return None,
    })
}

fn revealed_offset(offset: f32, selected: usize, row: f32, visible: usize) -> f32 {
    let top = selected as f32 * row;
    let height = row * visible as f32;
    if top < offset {
        top
    } else if top + row > offset + height {
        top + row - height
    } else {
        offset
    }
}

const DOCUMENTATION_WIDTH: f32 = 380.0;

fn documentation_panel_at(list: Rect, window: Rect) -> egui::Pos2 {
    const GAP: f32 = 4.0;
    let right = list.right() + GAP;
    if right + DOCUMENTATION_WIDTH <= window.right() || list.left() - GAP - DOCUMENTATION_WIDTH < window.left() {
        pos2(right.min(window.right() - DOCUMENTATION_WIDTH).max(window.left()), list.top())
    } else {
        pos2(list.left() - GAP - DOCUMENTATION_WIDTH, list.top())
    }
}

type SemanticSpan = (usize, usize, SemanticKind);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SemanticKind {
    Package,
    Type,
    BuiltinType,
    Call,
    Function,
    Link,
    DocName,
    Constant,
    Property,
    GlobalVariable,
    MatchedBracket,
    UnmatchedBracket,
    Unused,
    Todo(Option<Color32>),
}

#[derive(Clone, Copy, Debug)]
struct FoldArrows {
    shown: bool,
    unfolded: bool,
    bottom: bool,
}

impl FoldArrows {
    fn of(settings: &zen_core::Settings, hovered: bool) -> Self {
        let shown = settings.get_or(zen_workbench::folding::SHOW_ARROWS, true);
        let always = settings.get::<String>(zen_workbench::folding::ARROWS).as_deref() == Some("always");
        let bottom = settings.get_or(zen_workbench::folding::BOTTOM_ARROWS, false);
        Self { shown, unfolded: shown && (always || hovered), bottom: shown && bottom }
    }

    fn shows(self, folded: bool) -> bool {
        self.shown && (folded || self.unfolded)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FoldMark {
    Block,
    Imports,
    Comment,
    Region,
    Manual,
}

fn coverage_color(palette: &IdePalette, status: zen_run::LineStatus) -> Color32 {
    match status {
        zen_run::LineStatus::Covered => palette.coverage_covered,
        zen_run::LineStatus::Partly => palette.coverage_partly,
        zen_run::LineStatus::NotCovered => palette.coverage_not_covered,
    }
}

fn coverage_hint(measured: &zen_run::LineCoverage) -> String {
    let hits = t!("ide-coverage-hits", hits = measured.hits);
    match measured.branches {
        Some(branches) => {
            format!("{hits}\n{}", t!("ide-coverage-branches", covered = branches.covered, total = branches.total))
        }
        None => hits,
    }
}

fn target_context(session: &Session, target: &GutterTarget) -> Option<Value> {
    let document = session.state::<Workbench>().and_then(Workbench::active_document)?;
    let text = document.text();
    let mut offset = 0;
    let mut lines = text.split('\n');
    for line in lines.by_ref().take(target.line) {
        offset += line.chars().count() + 1;
    }
    offset += lines.next().map_or(0, |line| line.chars().take_while(|c| c.is_whitespace()).count());
    Some(json!({ "path": document.path(), "offset": offset }))
}

const SESSIONS_POLL: f64 = 1.0;

const STATE_POLL: f64 = 0.25;

fn stopped_at(state: &Value) -> Option<(PathBuf, usize)> {
    if state["state"] != "stopped" {
        return None;
    }
    let frame = &state["frame"];
    Some((PathBuf::from(frame["path"].as_str()?), frame["line"].as_u64()? as usize))
}

fn number_area(gutter: Rect, number_right: f32, annotated: bool) -> Rect {
    let left = gutter.left() + if annotated { ANNOTATIONS } else { 0.0 };
    let right = number_right + 4.0;
    Rect::from_x_y_ranges(left..=right.max(left), gutter.y_range())
}

fn breakpoint_dot(painter: &egui::Painter, center: egui::Pos2, radius: f32, mark: GutterMark, palette: &IdePalette) {
    match mark {
        GutterMark::Breakpoint(BreakpointState::Conditional) => {
            painter.circle_filled(center, radius, palette.breakpoint);
            let font = FontId::proportional((radius * 1.5).max(8.0));
            painter.text(center, Align2::CENTER_CENTER, "?", font, palette.editor_background);
        }
        GutterMark::Breakpoint(BreakpointState::Logging) => {
            painter.circle_filled(center, radius, palette.warning_text);
        }
        GutterMark::Breakpoint(BreakpointState::Muted) => {
            painter.circle_filled(center, radius, palette.text_secondary);
        }
        GutterMark::Breakpoint(BreakpointState::MutedDisabled) => {
            painter.circle_stroke(center, radius - 0.75, Stroke::new(1.5, palette.text_secondary));
        }
        GutterMark::Hover => {
            painter.circle_filled(center, radius, palette.breakpoint.gamma_multiply(0.4));
        }
        GutterMark::Breakpoint(BreakpointState::Verified) => {
            painter.circle_filled(center, radius, palette.breakpoint);
        }
        GutterMark::Breakpoint(BreakpointState::Invalid) => {
            painter.circle_filled(center, radius, palette.breakpoint);
            let arm = radius * 0.45;
            let stroke = Stroke::new((radius / 4.0).max(1.2), palette.editor_background);
            painter.line_segment([center + vec2(-arm, -arm), center + vec2(arm, arm)], stroke);
            painter.line_segment([center + vec2(arm, -arm), center + vec2(-arm, arm)], stroke);
        }
        GutterMark::Breakpoint(BreakpointState::Disabled) => {
            painter.circle_stroke(center, radius - 0.75, Stroke::new(1.5, palette.breakpoint_disabled));
        }
    }
}

pub(super) fn bookmark_icon(painter: &egui::Painter, rect: Rect, mnemonic: Option<char>, palette: &IdePalette) {
    let ink = palette.text_secondary;
    match mnemonic {
        Some(mnemonic) => {
            let square = Rect::from_center_size(rect.center(), Vec2::splat(rect.width().min(rect.height())));
            painter.rect(square, 2, palette.editor_background, Stroke::new(1.0, ink), StrokeKind::Inside);
            painter.text(
                square.center(),
                Align2::CENTER_CENTER,
                mnemonic,
                FontId::proportional(square.height() * 0.72),
                palette.text,
            );
        }
        None => {
            let width = rect.width().min(rect.height() * 0.75);
            let (left, right) = (rect.center().x - width / 2.0, rect.center().x + width / 2.0);
            let (top, bottom) = (rect.top() + 1.0, rect.bottom() - 1.0);
            let notch = pos2(rect.center().x, bottom - width * 0.45);
            let body = vec![pos2(left, top), pos2(right, top), pos2(right, notch.y), pos2(left, notch.y)];
            painter.add(egui::Shape::convex_polygon(body, ink, Stroke::NONE));
            let left_tail = vec![pos2(left, notch.y), notch, pos2(left, bottom)];
            painter.add(egui::Shape::convex_polygon(left_tail, ink, Stroke::NONE));
            let right_tail = vec![notch, pos2(right, notch.y), pos2(right, bottom)];
            painter.add(egui::Shape::convex_polygon(right_tail, ink, Stroke::NONE));
        }
    }
}

fn execution_arrow(painter: &egui::Painter, center: egui::Pos2, radius: f32, palette: &IdePalette) {
    let (w, h) = (radius * 1.1, radius * 0.75);
    let points = vec![
        center + vec2(-w, -h),
        center + vec2(w * 0.25, -h),
        center + vec2(w, 0.0),
        center + vec2(w * 0.25, h),
        center + vec2(-w, h),
    ];
    painter.add(egui::Shape::convex_polygon(points, palette.execution_arrow, Stroke::NONE));
}

fn run_icon_rect(number_right: f32, center_y: f32) -> Rect {
    Rect::from_center_size(pos2(number_right + 12.0, center_y), Vec2::splat(14.0))
}

fn run_icon(painter: &egui::Painter, rect: Rect, mark: Option<TestMark>, palette: &IdePalette) {
    let red = palette.problem("error");
    let amber = palette.problem("warning");
    let color = match mark {
        None | Some(TestMark::Passed) => palette.run,
        Some(TestMark::Failed) => red,
        Some(TestMark::Mixed) => amber,
    };
    super::glyphs::glyph_run_ring(painter, rect, color);
    let Some(mark) = mark else { return };
    let badge = Rect::from_center_size(rect.right_bottom() + vec2(0.5, -0.5), Vec2::splat(6.0));
    painter.circle_filled(badge.center(), 4.0, palette.island);
    let stroke = Stroke::new(1.3, color);
    match mark {
        TestMark::Passed => {
            painter.line_segment([badge.left_center(), pos2(badge.center().x - 0.5, badge.bottom())], stroke);
            painter.line_segment([pos2(badge.center().x - 0.5, badge.bottom()), badge.right_top()], stroke);
        }
        TestMark::Failed => {
            painter.line_segment([badge.left_top(), badge.right_bottom()], stroke);
            painter.line_segment([badge.right_top(), badge.left_bottom()], stroke);
        }
        TestMark::Mixed => {
            painter.line_segment(
                [pos2(badge.center().x, badge.top()), pos2(badge.center().x, badge.bottom() - 2.0)],
                stroke,
            );
            painter.circle_filled(pos2(badge.center().x, badge.bottom()), 0.7, color);
        }
    }
}

fn top_level_tests(node: &Value, found: &mut Vec<(String, String, TestMark)>) {
    if node["kind"] == "test"
        && let Some(name) = node["test"].as_str()
        && !name.contains('/')
    {
        if let Some(mark) = test_mark(node) {
            found.push((node["package"].as_str().unwrap_or_default().to_owned(), name.to_owned(), mark));
        }
        return;
    }
    for child in node["children"].as_array().into_iter().flatten() {
        top_level_tests(child, found);
    }
}

fn test_mark(node: &Value) -> Option<TestMark> {
    let mut statuses = Vec::new();
    collect_statuses(node, &mut statuses);
    if statuses.contains(&"running") {
        return None;
    }
    let passed = statuses.contains(&"passed");
    let failed = statuses.iter().any(|status| *status == "failed" || *status == "terminated");
    Some(match (passed, failed) {
        (true, false) if !statuses.contains(&"skipped") => TestMark::Passed,
        (false, true) => TestMark::Failed,
        _ => TestMark::Mixed,
    })
}

fn collect_statuses<'a>(node: &'a Value, out: &mut Vec<&'a str>) {
    if let Some(status) = node["status"].as_str() {
        out.push(status);
    }
    for child in node["children"].as_array().into_iter().flatten() {
        collect_statuses(child, out);
    }
}

#[derive(Clone, Debug)]
struct FoldShown {
    start: usize,
    end: usize,
    start_line: usize,
    end_line: usize,
    kind: String,
    folded: bool,
    placeholder: Option<String>,
    sticky_line: Option<usize>,
}

impl FoldShown {
    fn from_json(fold: &Value) -> Option<Self> {
        Some(Self {
            start: fold["start"].as_u64()? as usize,
            end: fold["end"].as_u64()? as usize,
            start_line: fold["startLine"].as_u64()? as usize,
            end_line: fold["endLine"].as_u64()? as usize,
            kind: fold["kind"].as_str()?.to_owned(),
            folded: fold["folded"].as_bool()?,
            placeholder: fold["placeholder"].as_str().map(str::to_owned),
            sticky_line: fold["stickyLine"].as_u64().map(|line| line as usize),
        })
    }
}

fn moved_folds(folds: &[FoldShown], edits: &[zen_workbench::CharEdit], text: &zen_text::Text) -> Vec<FoldShown> {
    let line = |offset: usize| text.offset_to_point(offset).map(|point| point.line).unwrap_or_default();
    folds
        .iter()
        .filter_map(|fold| {
            if edits.iter().all(|edit| fold.end < edit.range.start) {
                return Some(fold.clone());
            }
            let (start, end) =
                if fold.kind == "manual" {
                    let moved = edits.iter().try_fold(fold.start..fold.end, |range, edit| edit.map_region(&range))?;
                    (moved.start, moved.end)
                } else {
                    let start = edits.iter().fold(fold.start, |offset, edit| {
                        if edit.range.start == offset { offset } else { edit.map(offset) }
                    });
                    (start, edits.iter().fold(fold.end, |offset, edit| edit.map(offset)))
                };
            if start >= end {
                return None;
            }
            let start_line = line(start);
            let sticky_line = fold.sticky_line.map(|sticky| (sticky + start_line).saturating_sub(fold.start_line));
            Some(FoldShown { start, end, start_line, end_line: line(end), sticky_line, ..fold.clone() })
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
struct CompletionRow {
    index: u64,
    label: String,
    kind: String,
    detail: Option<String>,
    documentation: Option<String>,
    matches: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
struct CompletionShown {
    revision: u64,
    pending: bool,
    items: Arc<Vec<CompletionRow>>,
}

impl CompletionShown {
    fn from_json(list: &Value, previous: Option<&Self>) -> Self {
        let pending = list["pending"] == true;
        let revision = list["revision"].as_u64().unwrap_or_default();
        if list["unchanged"] == true
            && let Some(previous) = previous.filter(|previous| previous.revision == revision)
        {
            return Self { pending, ..previous.clone() };
        }
        let items = list["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| CompletionRow {
                        index: item["index"].as_u64().unwrap_or(0),
                        label: item["label"].as_str().unwrap_or_default().to_owned(),
                        kind: item["kind"].as_str().unwrap_or_default().to_owned(),
                        detail: item["detail"].as_str().map(str::to_owned),
                        documentation: item["documentation"].as_str().map(str::to_owned),
                        matches: item["matches"]
                            .as_array()
                            .map(|list| list.iter().filter_map(|at| at.as_u64().map(|at| at as usize)).collect())
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { revision, pending, items: Arc::new(items) }
    }
}

const DOUBLE_CLICK: f64 = 0.4;

fn underlines(painter: &egui::Painter, origin: egui::Pos2, galley: &egui::Galley) {
    let job = &galley.job;
    let mut offsets = job.text.char_indices().map(|(byte, _)| byte);
    let mut section = 0;
    for row in &galley.rows {
        let mut run: Option<(f32, f32, f32, Color32)> = None;
        let flush = |run: &mut Option<(f32, f32, f32, Color32)>| {
            if let Some((left, right, y, color)) = run.take() {
                painter.hline(origin.x + left..=origin.x + right, origin.y + y + 1.5, Stroke::new(1.0, color));
            }
        };
        for glyph in &row.glyphs {
            let byte = offsets.next().unwrap_or(job.text.len());
            while section + 1 < job.sections.len() && job.sections[section].byte_range.end.0 <= byte {
                section += 1;
            }
            let Some(format) = job.sections.get(section).map(|section| &section.format) else { continue };
            let underline = format.underline;
            let marked = underline.width == 0.0 && underline.color != Color32::TRANSPARENT;
            if !marked {
                flush(&mut run);
                continue;
            }
            let (left, right) = (glyph.pos.x, glyph.pos.x + glyph.advance_width);
            let y = row.pos.y + glyph.pos.y;
            match &mut run {
                Some((_, end, _, color)) if *color == underline.color => *end = right,
                _ => {
                    flush(&mut run);
                    run = Some((left, right, y, underline.color));
                }
            }
        }
        flush(&mut run);
    }
}

fn wavy_line(painter: &egui::Painter, left: f32, right: f32, y: f32, color: Color32) {
    const STEP: f32 = 2.0;
    const HEIGHT: f32 = 1.5;
    let mut points = Vec::new();
    let mut x = left;
    let mut up = true;
    while x <= right {
        points.push(pos2(x, if up { y - HEIGHT } else { y + HEIGHT } * 1.0));
        up = !up;
        x += STEP;
    }
    if points.len() >= 2 {
        painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
    }
}

#[cfg(test)]
mod tests {
    use egui::{Key, Modifiers};

    use super::*;

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        view: EditorView,
        _dir: tempfile::TempDir,
    }

    impl Harness {
        fn new(contents: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("main.go");
            std::fs::write(&file, contents).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.set_state(Workbench::default());
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let mut harness =
                Self { ctx: egui::Context::default(), registry, session, view: EditorView::default(), _dir: dir };
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.frame_with(events, Modifiers::NONE);
        }

        fn painted(&mut self) -> Vec<egui::Shape> {
            self.frame_with(Vec::new(), Modifiers::NONE).into_iter().map(|clipped| clipped.shape).collect()
        }

        fn frame_with(&mut self, mut events: Vec<Event>, modifiers: Modifiers) -> Vec<egui::epaint::ClippedShape> {
            events.insert(0, Event::ModifiersChanged(modifiers));
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            };
            let palette = IdePalette::zen_dark();
            let mut output = self.ctx.run_ui(input, |ui| {
                self.view.show(ui, &self.registry, &mut self.session, None, &palette);
            });
            output.textures_delta.clear();
            output.shapes
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        }

        fn text(&self) -> String {
            self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
        }

        fn char_width(&self) -> f32 {
            EditorMetrics::new(&self.ctx, &self.view.font, 4).char_width
        }

        fn caret(&self) -> usize {
            self.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head
        }

        fn with_runs(contents: &str, targets: &[(u32, &str)]) -> (Self, zen_run::fake::FakeProvider) {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("main_test.go");
            std::fs::write(&file, contents).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.set_state(crate::ide::trusting_everything());
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            let fake = zen_run::fake::FakeProvider::new();
            fake.set_targets(
                targets
                    .iter()
                    .map(|&(line, name)| zen_run::RunTarget {
                        line,
                        kind: if name == "main" { zen_run::TargetKind::Main } else { zen_run::TargetKind::Test },
                        name: name.to_owned(),
                        package: "p".to_owned(),
                        configuration: zen_run::fake::FakeProvider::configuration(name),
                    })
                    .collect(),
            );
            session
                .state_mut::<zen_workbench::run::Runs>()
                .set_providers(vec![Arc::new(fake.clone())], zen_run::RunEnv::default());
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let view = EditorView { run_icons: true, ..EditorView::default() };
            let mut harness = Self { ctx: egui::Context::default(), registry, session, view, _dir: dir };
            harness.frame(Vec::new());
            (harness, fake)
        }

        fn with_http(contents: &str) -> (Self, tempfile::TempDir) {
            let dir = tempfile::tempdir().unwrap();
            let data = tempfile::tempdir().unwrap();
            let file = dir.path().join("api.http");
            std::fs::write(&file, contents).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            session.state_mut::<zen_workbench::http::HttpClient>().set_data_dir(data.path().to_owned());
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let view = EditorView { run_icons: true, ..EditorView::default() };
            let mut harness = Self { ctx: egui::Context::default(), registry, session, view, _dir: dir };
            harness.frame(Vec::new());
            (harness, data)
        }

        fn with_sql(contents: &str) -> (Self, tempfile::TempDir) {
            let dir = tempfile::tempdir().unwrap();
            let data = tempfile::tempdir().unwrap();
            let file = dir.path().join("report.sql");
            std::fs::write(&file, contents).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            zen_workbench::database::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            session.state_mut::<zen_workbench::workspaces::Workspaces>().set_data_dir(data.path().to_owned());
            zen_workbench::database::install(&mut session);
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let view = EditorView { run_icons: true, ..EditorView::default() };
            let mut harness = Self { ctx: egui::Context::default(), registry, session, view, _dir: dir };
            harness.frame(Vec::new());
            (harness, data)
        }

        fn click(&mut self, at: egui::Pos2) {
            let press = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at), press(true)]);
            self.frame(vec![press(false)]);
        }

        fn click_with(&mut self, at: egui::Pos2, modifiers: Modifiers) {
            let press =
                |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
            self.frame_with(vec![Event::PointerMoved(at), press(true)], modifiers);
            self.frame_with(vec![press(false)], modifiers);
            self.frame(Vec::new());
        }

        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2, modifiers: Modifiers) {
            let press =
                |pos, pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers };
            self.frame_with(vec![Event::PointerMoved(from), press(from, true)], modifiers);
            let middle = from + (to - from) / 2.0;
            self.frame_with(vec![Event::PointerMoved(middle)], modifiers);
            self.frame_with(vec![Event::PointerMoved(to)], modifiers);
            self.frame_with(vec![press(to, false)], modifiers);
            self.frame(Vec::new());
        }

        fn run_icon_at(&self, line: usize, lines: usize) -> egui::Pos2 {
            let center = TOP + line as f32 * self.view.line_height + self.view.line_height / 2.0;
            let numbers = gutter::NUMBERS_LEFT + lines.to_string().len().max(2) as f32 * self.char_width();
            run_icon_rect(numbers, center).center()
        }

        fn runs(&mut self) -> Vec<Value> {
            let list = self.registry.dispatch(&mut self.session, "run.process.list", json!({})).unwrap();
            list["runs"].as_array().cloned().unwrap_or_default()
        }
    }

    const MARKUP: &str = "<div>\n<p>\na\n</p>\n</div>\nend\n";

    impl Harness {
        fn with_html_plugin() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("page.html");
            std::fs::write(&file, MARKUP).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.set_state(Workbench::default());
            let html = zen_plugins::native::NativePlugin::new(
                &["html"],
                zen_plugins::native::Exports {
                    fold: Some(|input| {
                        let text = std::str::from_utf8(input).unwrap();
                        zen_plugin_html::encode_folds(&zen_plugin_html::fold(text))
                    }),
                    ..Default::default()
                },
            );
            session.state_mut::<zen_workbench::plugins::PluginFeatures>().set(vec![Arc::new(html)]);
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let mut harness =
                Self { ctx: egui::Context::default(), registry, session, view: EditorView::default(), _dir: dir };
            harness.frame(Vec::new());
            harness
        }

        fn folded(&mut self) -> Vec<(u64, u64, bool)> {
            let list = self.registry.dispatch(&mut self.session, "editor.fold.list", json!({})).unwrap();
            list.as_array()
                .unwrap()
                .iter()
                .map(|fold| {
                    (fold["startLine"].as_u64().unwrap(), fold["endLine"].as_u64().unwrap(), fold["folded"] == true)
                })
                .collect()
        }

        fn set_caret(&mut self, head: usize) {
            self.registry
                .dispatch(&mut self.session, "editor.selection.set", json!({ "selections": [{ "head": head }] }))
                .unwrap();
            self.frame(Vec::new());
        }

        fn row(&self, row: usize) -> f32 {
            TOP + self.view.line_height * (row as f32 + 0.5)
        }
    }

    fn offset_of(line: usize) -> usize {
        MARKUP.split_inclusive('\n').take(line).map(|line| line.chars().count()).sum()
    }

    #[test]
    fn the_fold_arrows_follow_the_code_folding_settings() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let text = "package main\n\nfunc a() {\n\tx()\n}\n\nfunc b() {\n\ty()\n\tz()\n}\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(vec2(420.0, 280.0), 2.0);
        palette.apply(screen.context());
        let mut view = EditorView::default();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let show = |ui: &mut Ui, view: &mut EditorView, session: &mut Session| {
            ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
            view.show(ui, &registry, session, None, &palette);
        };
        screen.step(Vec::new(), |ui| show(ui, &mut view, &mut session));
        let char_width = screen.context().fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let gutter = gutter_width(11, char_width, false);
        let over_gutter = pos2(gutter / 2.0, TOP + view.line_height * 0.5);
        let away = pos2(gutter + 200.0, TOP + view.line_height * 0.5);
        let arrows = |screen: &mut crate::offscreen::Offscreen, view: &mut EditorView, session: &mut Session, at| {
            screen.step(vec![Event::PointerMoved(at)], |ui| show(ui, view, session));
            let tree = screen.accessibility_tree(|ui| show(ui, view, session));
            let mut found: Vec<(String, bool)> = tree
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == egui::accesskit::Role::DisclosureTriangle)
                .map(|(_, node)| (node.label().unwrap_or_default().to_owned(), node.is_expanded() == Some(true)))
                .collect();
            found.sort();
            found
        };
        let set = |session: &mut Session, show: bool, always: bool, bottom: bool| {
            let settings = session.settings_mut();
            settings.set(zen_workbench::folding::SHOW_ARROWS, show).unwrap();
            settings.set(zen_workbench::folding::ARROWS, if always { "always" } else { "onHover" }).unwrap();
            settings.set(zen_workbench::folding::BOTTOM_ARROWS, bottom).unwrap();
        };
        let line = |line: usize| t!("a11y-fold", line = line);
        let top = vec![(line(3), true), (line(7), true)];
        let save =
            |screen: &mut crate::offscreen::Offscreen, view: &mut EditorView, session: &mut Session, at, name: &str| {
                if let Some(dir) = &snapshots {
                    let image = screen.frame(vec![Event::PointerMoved(at)], |ui| show(ui, view, session));
                    crate::offscreen::write_png(&dir.join(format!("gutter-{name}.png")), &image).unwrap();
                }
            };

        assert_eq!(arrows(&mut screen, &mut view, &mut session, away), [], "on mouse hover by default: none away");
        save(&mut screen, &mut view, &mut session, away, "hover-away");
        assert_eq!(arrows(&mut screen, &mut view, &mut session, over_gutter), top, "over the gutter");
        save(&mut screen, &mut view, &mut session, over_gutter, "hover-over");

        set(&mut session, true, true, false);
        assert_eq!(arrows(&mut screen, &mut view, &mut session, away), top, "always");
        save(&mut screen, &mut view, &mut session, away, "always");

        set(&mut session, true, true, true);
        let mut with_bottom = vec![(line(5), true), (line(10), true)];
        with_bottom.extend(top.clone());
        with_bottom.sort();
        assert_eq!(
            arrows(&mut screen, &mut view, &mut session, away),
            with_bottom,
            "bottom arrows on the closing lines"
        );
        save(&mut screen, &mut view, &mut session, away, "always-bottom");
        let tree = screen.accessibility_tree(|ui| show(ui, &mut view, &mut session));
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label().is_some_and(|label| label == line(10)))
            .and_then(|(_, node)| node.bounds())
            .unwrap();
        let bottom = pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        for events in [
            vec![Event::PointerMoved(bottom)],
            vec![Event::PointerButton {
                pos: bottom,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
            vec![Event::PointerButton {
                pos: bottom,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            Vec::new(),
        ] {
            screen.step(events, |ui| show(ui, &mut view, &mut session));
        }
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        let folded: Vec<bool> = folds.as_array().unwrap().iter().map(|fold| fold["folded"] == true).collect();
        assert_eq!(folded, [false, true], "a click on the bottom arrow folds its region");
        assert_eq!(
            arrows(&mut screen, &mut view, &mut session, away),
            [(line(3), true), (line(5), true), (line(7), false)],
            "a folded region shows no bottom arrow"
        );
        save(&mut screen, &mut view, &mut session, away, "always-bottom-folded");

        set(&mut session, false, true, true);
        assert_eq!(
            arrows(&mut screen, &mut view, &mut session, over_gutter),
            [],
            "no arrows at all, folded ones neither"
        );
        save(&mut screen, &mut view, &mut session, over_gutter, "off");
    }

    #[test]
    fn a_plugins_region_folds_from_its_first_lines_end_and_the_rows_skip_its_lines() {
        let mut editor = Harness::with_html_plugin();
        assert_eq!(editor.folded(), [(0, 4, false), (1, 3, false)], "the plug-in's folds in editor.fold.*");
        let gutter = gutter_width(7, editor.char_width(), false);
        let arrow = pos2(fold_center(Rect::from_min_max(pos2(0.0, 0.0), pos2(gutter, 100.0))), editor.row(0));
        editor.frame(vec![Event::PointerMoved(arrow)]);
        editor.click(arrow);
        assert_eq!(editor.folded(), [(0, 4, true), (1, 3, false)]);
        assert_eq!(editor.caret(), 0, "a click on the arrow does not select the line");
        let shown = editor.view.folds.as_ref().map(|(_, list)| list.clone()).unwrap_or_default();
        assert_eq!((shown[0].start, shown[0].end), (5, offset_of(5) - 1), "from the first line's end to the last's");
        editor.click(pos2(gutter + TEXT_LEFT + 1.0, editor.row(1)));
        assert_eq!(editor.caret(), offset_of(5), "the second row shows the line after the region");
        assert_eq!(editor.folded()[0], (0, 4, true));
        editor.frame(vec![Event::PointerMoved(arrow)]);
        editor.click(arrow);
        assert_eq!(editor.folded(), [(0, 4, false), (1, 3, false)]);
    }

    #[test]
    fn the_folding_keys_fold_the_plugins_regions_and_a_caret_moved_in_opens_them() {
        let mut editor = Harness::with_html_plugin();
        editor.set_caret(offset_of(2));
        editor.key(Key::Minus, Modifiers::COMMAND);
        assert_eq!(editor.folded(), [(0, 4, false), (1, 3, true)], "⌘− folds the innermost region");
        assert_eq!(editor.caret(), offset_of(2) - 1, "the caret moved out of it");
        editor.key(Key::Minus, Modifiers::COMMAND);
        assert_eq!(editor.folded(), [(0, 4, true), (1, 3, true)], "then the one around it");
        editor.key(Key::Equals, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(editor.folded(), [(0, 4, false), (1, 3, false)], "⇧⌘= unfolds everything");
        editor.key(Key::Minus, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(editor.folded(), [(0, 4, true), (1, 3, true)], "⇧⌘− folds everything");
        editor.set_caret(offset_of(3));
        assert_eq!(editor.folded(), [(0, 4, false), (1, 3, false)], "a caret on a hidden line opens what hides it");
    }

    impl Harness {
        fn painted_texts(&mut self) -> Vec<(String, f32)> {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            };
            let palette = IdePalette::zen_dark();
            let output = self.ctx.run_ui(input, |ui| {
                self.view.show(ui, &self.registry, &mut self.session, None, &palette);
            });
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.pos.y)),
                    _ => None,
                })
                .collect()
        }
    }

    impl Harness {
        fn galleys_of(&mut self, needle: &str, palette: &IdePalette, apply: bool) -> Vec<Arc<egui::Galley>> {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                if apply {
                    palette.apply(ui.ctx());
                }
                self.view.show(ui, &self.registry, &mut self.session, None, palette);
            });
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().contains(needle) => Some(Arc::clone(&text.galley)),
                    _ => None,
                })
                .collect()
        }
    }

    #[test]
    fn a_theme_switch_lays_the_lines_out_again_in_egui_s_new_font_atlas() {
        let mut editor = Harness::new("package main\n\nfunc main() {}\n");
        let dark = IdePalette::zen_dark();
        let light = IdePalette::from_theme(&zen_theme::zen_light().resolve(|_| None).theme);
        let before = editor.galleys_of("func main", &dark, false);
        let again = editor.galleys_of("func main", &dark, false);
        assert_eq!(before.len(), 1, "the line is painted once");
        assert!(Arc::ptr_eq(&before[0], &again[0]), "an unchanged line keeps its galley between frames");
        let switched = editor.galleys_of("func main", &light, true);
        let after = editor.galleys_of("func main", &light, false);
        assert_eq!(after.len(), 1);
        assert!(
            !Arc::ptr_eq(&switched[0], &after[0]),
            "a galley of the old atlas would draw pieces of the new atlas in place of the letters"
        );
        let settled = editor.galleys_of("func main", &light, false);
        assert!(Arc::ptr_eq(&after[0], &settled[0]), "and is kept again once the new atlas stays");
    }

    #[test]
    fn sticky_lines_are_painted_and_a_click_goes_to_one() {
        let body: String = (0..120).map(|i| format!("\tx{i}()\n")).collect();
        let text = format!("package main\n\nfunc main() {{\n{body}}}\n");
        let mut editor = Harness::new(&text);
        editor.set_caret(text.find("x100()").unwrap());
        for _ in 0..40 {
            editor.frame(Vec::new());
        }
        assert!(editor.view.shown_lines.0 > 3, "scrolled down to {:?}", editor.view.shown_lines);
        assert_eq!(editor.view.sticky.shown(), [2], "main's first line sticks");
        let texts = editor.painted_texts();
        assert!(
            texts.iter().any(|(painted, y)| painted == "func main() {" && *y < editor.view.line_height),
            "drawn over the first row: {texts:?}"
        );
        assert!(texts.iter().any(|(painted, _)| painted == "3"), "with its line number");
        let shown = editor.registry.dispatch(&mut editor.session, "editor.stickyLines.list", json!({})).unwrap();
        assert_eq!(shown["lines"][0]["line"], 2, "agents read what is shown: {shown}");
        let gutter = gutter_width(124, editor.char_width(), false);
        editor.click(pos2(gutter + TEXT_LEFT + 30.0, editor.view.line_height / 2.0));
        assert_eq!(editor.caret(), text.find("func main").unwrap(), "the caret goes to the line");
        editor.frame(Vec::new());
        editor.frame(Vec::new());
        assert_eq!(editor.view.shown_lines.0, 2, "the line is scrolled to the top");
        assert!(editor.view.sticky.shown().is_empty());
    }

    #[test]
    fn a_literal_does_not_stick_and_a_long_header_sticks_with_its_first_line() {
        let body: String = (0..120).map(|i| format!("\t\tT{{{i}}},\n")).collect();
        let text = format!("package main\n\nfunc main(\n\ta int,\n) {{\n\tx := []T{{\n{body}\t}}\n}}\n");
        let mut editor = Harness::new(&text);
        editor.set_caret(text.find("T{100}").unwrap());
        for _ in 0..40 {
            editor.frame(Vec::new());
        }
        assert!(editor.view.shown_lines.0 > 6, "scrolled down to {:?}", editor.view.shown_lines);
        assert_eq!(editor.view.sticky.shown(), [2], "main's header sticks, the literal does not");
        assert!(editor.painted_texts().iter().any(|(painted, _)| painted == "func main("));
    }

    #[test]
    fn the_setting_hides_sticky_lines() {
        let body: String = (0..120).map(|i| format!("\tx{i}()\n")).collect();
        let text = format!("package main\n\nfunc main() {{\n{body}}}\n");
        let mut editor = Harness::new(&text);
        editor.session.settings_mut().set(zen_workbench::sticky::ENABLED, false).unwrap();
        editor.set_caret(text.find("x100()").unwrap());
        for _ in 0..40 {
            editor.frame(Vec::new());
        }
        assert!(editor.view.shown_lines.0 > 3);
        assert!(editor.view.sticky.shown().is_empty());
        assert!(!editor.painted_texts().iter().any(|(painted, _)| painted == "func main() {"));
        let listed = editor.registry.dispatch(&mut editor.session, "editor.stickyLines.list", json!({})).unwrap();
        assert_eq!(listed["enabled"], false);
    }

    const TESTS: &str = "package main\n\nfunc TestA(t *testing.T) {}\n\nfunc TestB(t *testing.T) {}\n";

    #[test]
    fn option_pressed_twice_and_held_clones_carets() {
        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        let none = Modifiers::NONE;
        let mut detector = DoubleOption::default();
        assert!(!detector.update(0.0, alt, false), "one press");
        assert!(!detector.update(0.1, none, false));
        assert!(detector.update(0.2, alt, false), "the second press, held");
        assert!(detector.update(0.5, alt, false), "still held: ↑ and ↓ clone");
        assert!(!detector.update(0.6, none, false), "released");
        let mut slow = DoubleOption::default();
        slow.update(0.0, alt, false);
        slow.update(0.1, none, false);
        assert!(!slow.update(0.9, alt, false));
        let mut typed = DoubleOption::default();
        typed.update(0.0, alt, true);
        typed.update(0.1, none, false);
        assert!(!typed.update(0.2, alt, false), "⌥ with a key is not a press alone");
    }

    #[test]
    fn typescript_tokens_colour_calls_declared_properties_and_globals() {
        let token = |kind: &str, modifiers: &[&str]| json!({ "kind": kind, "modifiers": modifiers });
        assert_eq!(
            web_semantic_kind(&token("variable", &["declaration", "readonly"])),
            Some(SemanticKind::GlobalVariable),
            "d declared at the top"
        );
        assert_eq!(web_semantic_kind(&token("variable", &["readonly"])), Some(SemanticKind::GlobalVariable), "d used");
        assert_eq!(
            web_semantic_kind(&token("variable", &["defaultLibrary"])),
            Some(SemanticKind::GlobalVariable),
            "console"
        );
        assert_eq!(web_semantic_kind(&token("method", &["defaultLibrary"])), Some(SemanticKind::Function), "warn");
        assert_eq!(web_semantic_kind(&token("function", &[])), Some(SemanticKind::Function), "test()");
        assert_eq!(web_semantic_kind(&token("function", &["declaration"])), None, "function test: the lexer's colour");
        assert_eq!(web_semantic_kind(&token("method", &["declaration"])), None);
        assert_eq!(web_semantic_kind(&token("variable", &["local", "readonly"])), None, "a local const");
        assert_eq!(web_semantic_kind(&token("variable", &["declaration", "local"])), None);
        assert_eq!(web_semantic_kind(&token("parameter", &["declaration"])), None);
        assert_eq!(web_semantic_kind(&token("property", &[])), None, "options.statusCode");
        assert_eq!(web_semantic_kind(&token("property", &["defaultLibrary"])), None, "args.length");
        assert_eq!(
            web_semantic_kind(&token("property", &["declaration"])),
            Some(SemanticKind::Property),
            "{{ statusCode: … }}"
        );
        assert_eq!(web_semantic_kind(&token("class", &[])), None, "classes keep the lexer's colour");
        assert_eq!(web_semantic_kind(&token("enumMember", &[])), Some(SemanticKind::Constant));
        assert!(is_web_script(std::path::Path::new("/p/i.ts")) && is_web_script(std::path::Path::new("a.jsx")));
        assert!(!is_web_script(std::path::Path::new("main.go")), "Go keeps gopls' mapping");
    }

    #[test]
    fn constants_are_coloured_from_semantic_tokens() {
        let token = |kind: &str, modifiers: &[&str]| json!({ "kind": kind, "modifiers": modifiers });
        assert_eq!(semantic_kind(&token("variable", &["readonly"])), Some(SemanticKind::Constant), "model.MaxItems");
        assert_eq!(semantic_kind(&token("variable", &["definition", "readonly"])), Some(SemanticKind::Constant));
        assert_eq!(semantic_kind(&token("variable", &["readonly", "defaultLibrary"])), None, "true, nil, iota");
        assert_eq!(semantic_kind(&token("variable", &[])), None, "a plain variable");
        assert_eq!(semantic_kind(&token("namespace", &[])), Some(SemanticKind::Package));
        assert_eq!(semantic_kind(&token("function", &[])), Some(SemanticKind::Call));
        assert_eq!(semantic_kind(&token("function", &["definition"])), None);
    }

    fn regions_text(regions: bool) -> String {
        (0..400)
            .map(|n| {
                let body = format!("func f{n}() {{\n\tx := {n}\n\ty := x + 1\n}}\n\n");
                let (open, close) = if regions { ("region", "endregion") } else { ("note", "end of note") };
                match n % 4 {
                    0 => format!("// {open} Part {n}\n{body}"),
                    3 => format!("{body}// {close}\n"),
                    _ => body,
                }
            })
            .collect()
    }

    fn typing_frames(editor: &mut Harness, keys: usize) -> f64 {
        let started = std::time::Instant::now();
        for _ in 0..keys {
            editor.frame(vec![Event::Text("a".to_owned())]);
            editor.key(Key::Backspace, Modifiers::NONE);
        }
        started.elapsed().as_secs_f64() * 1000.0 / (keys * 2) as f64
    }

    type Listed = Vec<(usize, usize, String, bool, Option<String>)>;

    fn shown_and_read(editor: &mut Harness) -> (Listed, Listed) {
        let shown: Listed = editor
            .view
            .folds
            .as_ref()
            .unwrap()
            .1
            .iter()
            .map(|fold| (fold.start, fold.end, fold.kind.clone(), fold.folded, fold.placeholder.clone()))
            .collect();
        let (from, to) = editor.view.folds.as_ref().unwrap().0.3;
        let params = json!({ "fromLine": from, "toLine": to, "enclosing": true });
        let read = editor.registry.dispatch(&mut editor.session, "editor.fold.list", params).unwrap();
        let read: Listed = read
            .as_array()
            .unwrap()
            .iter()
            .filter_map(FoldShown::from_json)
            .map(|fold| (fold.start, fold.end, fold.kind, fold.folded, fold.placeholder))
            .collect();
        (shown, read)
    }

    #[test]
    fn typing_moves_custom_and_manual_regions_without_reading_the_text_again() {
        let text = regions_text(true);
        let mut editor = Harness::new(&text);
        let inline = text.find("y := x + 1").unwrap();
        editor
            .registry
            .dispatch(
                &mut editor.session,
                "editor.selection.set",
                json!({ "selections": [{ "anchor": inline + 5, "head": inline + 10 }] }),
            )
            .unwrap();
        editor.registry.dispatch(&mut editor.session, "editor.fold.selection", json!({})).unwrap();
        let region = text.find("// region Part 0").unwrap();
        editor.registry.dispatch(&mut editor.session, "editor.fold.toggle", json!({ "start": region })).unwrap();
        for _ in 0..30 {
            editor.frame(Vec::new());
        }
        editor.set_caret(text.find("x := 10\n").unwrap());
        let listed = editor.view.fold_listings;
        let with_regions = typing_frames(&mut editor, 40);
        editor.frame(vec![Event::Text("z".to_owned())]);
        assert_eq!(editor.view.fold_listings, listed, "moved, not read again, while typing goes on");
        let (shown, read) = shown_and_read(&mut editor);
        assert_eq!(shown, read, "where reading the text again puts them");
        assert!(shown.iter().any(|fold| fold.2 == "manual" && fold.3), "{shown:?}");
        assert!(
            shown.iter().any(|fold| fold.2 == "region" && fold.3 && fold.4.as_deref() == Some("Part 0")),
            "{shown:?}"
        );
        let manual = shown.iter().find(|fold| fold.2 == "manual").unwrap().clone();
        editor.set_caret(manual.1);
        editor.frame(vec![Event::Text("q".to_owned())]);
        editor.set_caret(manual.0);
        editor.frame(vec![Event::Text("p".to_owned())]);
        let (shown, read) = shown_and_read(&mut editor);
        assert_eq!(shown, read, "typed right after and before a manual region");
        assert!(shown.iter().any(|fold| (fold.0, fold.1, fold.3) == (manual.0 + 1, manual.1 + 1, true)), "{shown:?}");

        let mut plain = Harness::new(&regions_text(false));
        for _ in 0..30 {
            plain.frame(Vec::new());
        }
        plain.set_caret(regions_text(false).find("x := 10\n").unwrap());
        let without = typing_frames(&mut plain, 40);
        let read = |editor: &mut Harness| {
            editor.frame(vec![Event::Text("a".to_owned())]);
            let started = std::time::Instant::now();
            editor.registry.dispatch(&mut editor.session, "editor.fold.list", json!({})).unwrap();
            started.elapsed().as_secs_f64() * 1000.0
        };
        let (read_with, read_without) = (read(&mut editor), read(&mut plain));
        #[allow(clippy::print_stderr, reason = "the measurement, read with --nocapture")]
        {
            eprintln!(
                "typing frame: {with_regions:.2} ms with 100 custom regions and a manual one, {without:.2} ms with plain \
                 comments in their place; reading the folds once the text settles: {read_with:.2} ms, {read_without:.2} ms"
            );
        }
    }

    #[test]
    fn a_fold_inside_a_line_with_another_fold_loses_no_text_for_carets_selections_and_copy() {
        let text = "package main\n\nfunc main() {\n\tprintln(\"one\", \"two\")\n}\n";
        let mut editor = Harness::new(text);
        let name = text.find("main()").unwrap();
        let select = |editor: &mut Harness, anchor: usize, head: usize| {
            let selections = json!({ "selections": [{ "anchor": anchor, "head": head }] });
            editor.registry.dispatch(&mut editor.session, "editor.selection.set", selections).unwrap();
        };
        select(&mut editor, name, name + "main()".len());
        editor.registry.dispatch(&mut editor.session, "editor.fold.selection", json!({})).unwrap();
        let block = text.find(" {\n").unwrap() + 2;
        editor.registry.dispatch(&mut editor.session, "editor.fold.toggle", json!({ "start": block })).unwrap();
        for _ in 0..3 {
            editor.frame(Vec::new());
        }
        let folded: Vec<(String, bool)> =
            editor.view.folds.as_ref().unwrap().1.iter().map(|fold| (fold.kind.clone(), fold.folded)).collect();
        assert_eq!(folded, [("manual".to_owned(), true), ("block".to_owned(), true)]);
        let painted: Vec<String> = editor.painted_texts().into_iter().map(|(text, _)| text).collect();
        for shown in ["...", " {", "    println(\"one\", \"two\")", "}"] {
            assert!(painted.iter().any(|text| text == shown), "{shown:?} is drawn: {painted:?}");
        }
        let line_start = text.find("func").unwrap();
        let line_end = text.find(" {\n").unwrap() + 2;
        editor.set_caret(line_start);
        editor.key(Key::End, Modifiers::SHIFT);
        let document = editor.session.state::<Workbench>().unwrap().active_document().unwrap().clone();
        let selected = document.selections().primary();
        assert_eq!((selected.anchor, selected.head), (line_start, line_end), "⇧End selects over the fold");
        assert_eq!(copy_text(&document).0, "func main() {", "copy takes the real text, not the mark");
        let after = name + "main()".len();
        editor.set_caret(after);
        let behind = editor.view.caret_rect.unwrap();
        editor.set_caret(name);
        let before = editor.view.caret_rect.unwrap();
        assert!(
            behind.left() > before.left() + 2.0 * editor.char_width(),
            "the caret after the fold is after its mark"
        );
        editor.click(pos2(behind.left() + editor.char_width() * 1.6, behind.center().y));
        assert_eq!(editor.caret(), after + 2, "a click on the text after the mark lands there");
        assert_eq!(editor.folded()[0], (2, 2, true));
        editor.set_caret(name);
        editor.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(editor.caret(), name + 1);
        assert_eq!(editor.folded()[0], (2, 2, false), "a caret moved into it opens it, as any fold");
    }

    #[test]
    fn typing_moves_the_listed_folds_instead_of_listing_them_again() {
        let text: String = (0..30).map(|n| format!("func f{n}() {{\n\tx := {n}\n}}\n\n")).collect();
        let mut editor = Harness::new(&text);
        let listed = editor.view.fold_listings;
        assert!(editor.view.folds.as_ref().is_some_and(|((.., current), _)| *current), "read from the text");
        editor.set_caret(text.find("x := 3").unwrap());
        assert_eq!(editor.view.fold_listings, listed, "moving the caret lists nothing");
        editor.frame(vec![Event::Text("y".to_owned())]);
        editor.key(Key::Enter, Modifiers::NONE);
        editor.frame(vec![Event::Text("z".to_owned())]);
        assert!(editor.text().contains("y\n\tzx := 3"), "{}", editor.text());
        assert_eq!(editor.view.fold_listings, listed, "the folds move with the edits while typing goes on");
        let shown: Vec<(usize, usize, usize, usize, Option<usize>)> = editor
            .view
            .folds
            .as_ref()
            .unwrap()
            .1
            .iter()
            .map(|fold| (fold.start, fold.end, fold.start_line, fold.end_line, fold.sticky_line))
            .collect();
        let read = editor.registry.dispatch(&mut editor.session, "editor.fold.list", json!({})).unwrap();
        let read: Vec<(usize, usize, usize, usize, Option<usize>)> = read
            .as_array()
            .unwrap()
            .iter()
            .filter_map(FoldShown::from_json)
            .map(|fold| (fold.start, fold.end, fold.start_line, fold.end_line, fold.sticky_line))
            .collect();
        assert_eq!(shown, read, "where reading the text again puts them");
        for _ in 0..30 {
            editor.frame(Vec::new());
        }
        assert_eq!(editor.view.fold_listings, listed + 1, "read again once the text settled");
        assert!(editor.view.folds.as_ref().is_some_and(|((.., current), _)| *current));
    }

    #[test]
    fn moved_folds_are_where_reading_again_puts_them_after_carets_line_joins_and_undo() {
        type Shown = Vec<(usize, usize, usize, usize, bool, Option<usize>)>;
        fn shown(editor: &Harness) -> Shown {
            let folds = &editor.view.folds.as_ref().unwrap().1;
            folds.iter().map(|f| (f.start, f.end, f.start_line, f.end_line, f.folded, f.sticky_line)).collect()
        }
        fn read(editor: &mut Harness) -> Shown {
            let (from, to) = editor.view.folds.as_ref().unwrap().0.3;
            let params = json!({ "fromLine": from, "toLine": to, "enclosing": true });
            let read = editor.registry.dispatch(&mut editor.session, "editor.fold.list", params).unwrap();
            read.as_array()
                .unwrap()
                .iter()
                .filter_map(FoldShown::from_json)
                .map(|f| (f.start, f.end, f.start_line, f.end_line, f.folded, f.sticky_line))
                .collect()
        }
        let text: String = (0..30).map(|n| format!("func f{n}() {{\n\tx := {n}\n}}\n\n")).collect();
        let mut editor = Harness::new(&text);
        let folded = read(&mut editor).iter().find(|fold| fold.2 == 80).map(|fold| fold.0).unwrap();
        editor.registry.dispatch(&mut editor.session, "editor.fold.toggle", json!({ "start": folded })).unwrap();
        for _ in 0..30 {
            editor.frame(Vec::new());
        }
        assert!(shown(&editor).iter().any(|fold| fold.4), "f20 is folded");
        let listed = editor.view.fold_listings;
        let carets = [text.find("x := 3").unwrap(), text.find("x := 12").unwrap()];
        let selections = json!({ "selections": [{ "head": carets[0] }, { "head": carets[1] }] });
        editor.registry.dispatch(&mut editor.session, "editor.selection.set", selections).unwrap();
        editor.key(Key::Enter, Modifiers::NONE);
        editor.frame(vec![Event::Text("y".to_owned())]);
        assert_eq!(shown(&editor), read(&mut editor), "two carets");
        let after_brace = editor.text().find("func f6() {").unwrap() + "func f6() {".len();
        editor.set_caret(after_brace);
        editor.key(Key::Delete, Modifiers::NONE);
        assert!(editor.text().contains("func f6() {\tx := 6"), "{}", editor.text());
        let unstuck =
            |folds: Shown| folds.into_iter().map(|fold| (fold.0, fold.1, fold.2, fold.3, fold.4)).collect::<Vec<_>>();
        assert_eq!(unstuck(shown(&editor)), unstuck(read(&mut editor)), "the line break after a fold's start");
        let closing = editor.text().find("\tx := 7\n}").unwrap() + "\tx := 7\n".len();
        editor.set_caret(closing);
        editor.key(Key::Backspace, Modifiers::NONE);
        assert!(editor.text().contains("\tx := 7}"), "{}", editor.text());
        assert_eq!(unstuck(shown(&editor)), unstuck(read(&mut editor)), "the line break before a fold's end");
        for _ in 0..3 {
            editor.registry.dispatch(&mut editor.session, "editor.edit.undo", json!({})).unwrap();
            editor.frame(Vec::new());
            assert_eq!(unstuck(shown(&editor)), unstuck(read(&mut editor)), "undone");
        }
        assert_eq!(shown(&editor), read(&mut editor), "the joined block apart again sticks as before");
        assert_eq!(editor.view.fold_listings, listed, "moved, not read again, while typing goes on");
        assert!(shown(&editor).iter().any(|fold| fold.4), "f20 is still folded");
    }

    #[test]
    fn moved_folds_follow_the_edits_and_drop_the_ones_removed() {
        let text = zen_text::Text::from("a {\n}\n\nb {\n\n}\n");
        let fold = |start, end, start_line, end_line| FoldShown {
            start,
            end,
            start_line,
            end_line,
            kind: "block".to_owned(),
            folded: false,
            placeholder: None,
            sticky_line: Some(start_line),
        };
        let before = [fold(3, 4, 0, 1), fold(9, 11, 2, 4)];
        let edits = [zen_workbench::CharEdit { range: 6..6, inserted: 1 }];
        let moved = moved_folds(&before, &edits, &text);
        assert_eq!((moved[0].start, moved[0].end, moved[0].start_line), (3, 4, 0), "before the edit: as it was");
        assert_eq!((moved[1].start, moved[1].end), (10, 12));
        assert_eq!((moved[1].start_line, moved[1].end_line, moved[1].sticky_line), (3, 5, Some(3)));
        let removed = [zen_workbench::CharEdit { range: 2..6, inserted: 0 }];
        assert!(moved_folds(&before[..1], &removed, &text).is_empty(), "a fold whose text went is dropped");
        let manual = FoldShown { kind: "manual".to_owned(), ..fold(3, 4, 0, 1) };
        let typed = [
            zen_workbench::CharEdit { range: 4..4, inserted: 1 },
            zen_workbench::CharEdit { range: 3..3, inserted: 1 },
        ];
        let moved = moved_folds(&[manual], &typed, &text);
        assert_eq!((moved[0].start, moved[0].end), (4, 5), "typed right after and before a manual region: outside it");
    }

    impl Harness {
        fn with_templates(count: usize) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("notes.txt");
            std::fs::write(&file, "\n").unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.set_state(Workbench::default());
            let templates = zen_workbench::templates::LiveTemplates::default();
            templates.declare((0..count).map(|n| zen_workbench::templates::Template {
                language: "other".to_owned(),
                abbreviation: format!("tpl{n:02}"),
                description: format!("Template {n}"),
                text: format!("template {n}"),
                variables: Vec::new(),
                imports: Vec::new(),
                contexts: Vec::new(),
                enabled: true,
            }));
            session.set_state(templates);
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let mut harness =
                Self { ctx: egui::Context::default(), registry, session, view: EditorView::default(), _dir: dir };
            harness.frame(Vec::new());
            harness
        }

        fn completion_rows_painted(&mut self) -> Vec<String> {
            let mut rows: Vec<String> = self
                .painted()
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Text(text) if text.galley.text().starts_with("tpl") => {
                        Some(text.galley.text().to_owned())
                    }
                    _ => None,
                })
                .collect();
            rows.sort();
            rows
        }
    }

    #[test]
    fn the_completion_popup_keeps_its_items_and_lays_out_only_the_rows_shown() {
        let mut editor = Harness::with_templates(60);
        editor.frame(vec![Event::Text("t".to_owned())]);
        let items = editor.view.completion.as_ref().map(|(shown, _)| Arc::clone(&shown.items)).unwrap();
        assert_eq!(items.len(), 60);
        let rows = editor.completion_rows_painted();
        assert!((10..=11).contains(&rows.len()), "the rows in view, not all 60: {rows:?}");
        assert_eq!(rows[0], "tpl00");
        for _ in 0..3 {
            editor.frame(Vec::new());
        }
        let kept = editor.view.completion.as_ref().map(|(shown, _)| Arc::clone(&shown.items)).unwrap();
        assert!(Arc::ptr_eq(&items, &kept), "an unchanged list is not read again");
        for _ in 0..15 {
            editor.key(Key::ArrowDown, Modifiers::NONE);
        }
        assert_eq!(editor.view.completion.as_ref().map(|(_, selected)| *selected), Some(15));
        let rows = editor.completion_rows_painted();
        assert!(rows.contains(&"tpl15".to_owned()), "the selected row is scrolled into view: {rows:?}");
        assert!(!rows.contains(&"tpl00".to_owned()), "{rows:?}");
        editor.frame(vec![Event::Text("1".to_owned())]);
        let narrowed = editor.view.completion.as_ref().map(|(shown, _)| Arc::clone(&shown.items)).unwrap();
        assert!(!Arc::ptr_eq(&items, &narrowed), "typing lists the items again");
        assert!(narrowed.len() < 60 && narrowed.iter().all(|item| item.label.contains('1')), "{narrowed:?}");
    }

    #[test]
    fn a_completion_opened_again_shows_its_first_rows() {
        let mut editor = Harness::with_templates(60);
        editor.frame(vec![Event::Text("t".to_owned())]);
        for _ in 0..15 {
            editor.key(Key::ArrowDown, Modifiers::NONE);
        }
        assert!(!editor.completion_rows_painted().contains(&"tpl00".to_owned()));
        editor.key(Key::Escape, Modifiers::NONE);
        editor.frame(vec![Event::Text(" ".to_owned())]);
        editor.frame(vec![Event::Text("t".to_owned())]);
        assert_eq!(editor.view.completion.as_ref().map(|(_, selected)| *selected), Some(0));
        let rows = editor.completion_rows_painted();
        assert!(rows.contains(&"tpl00".to_owned()), "the selected first row is in view: {rows:?}");
    }

    fn commented_go(contents: &str) -> Harness {
        let mut editor = Harness::new(contents);
        let colours: Arc<dyn Highlighter> = Arc::new(GoModColours);
        editor
            .session
            .state_mut::<zen_workbench::brackets::Highlighters>()
            .set_source(Arc::new(move |_: &str| Some(Arc::clone(&colours))));
        let end = contents.chars().count();
        editor
            .registry
            .dispatch(&mut editor.session, "editor.selection.set", json!({ "selections": [{ "head": end }] }))
            .unwrap();
        editor.frame(Vec::new());
        editor
    }

    fn type_text(editor: &mut Harness, text: &str) {
        for c in text.chars() {
            editor.frame(vec![Event::Text(c.to_string())]);
        }
    }

    fn painted_todo_words(editor: &mut Harness) -> Vec<String> {
        editor
            .painted()
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) if ["TODO", "FIXME"].contains(&text.galley.text()) => {
                    Some(text.galley.text().to_owned())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn typing_in_a_comment_pops_up_todo_and_enter_puts_it_in() {
        let mut editor = commented_go("package main\n\n");
        type_text(&mut editor, "// to");
        let shown = editor
            .view
            .completion
            .as_ref()
            .map(|(shown, _)| shown.items.iter().map(|item| item.label.clone()).collect::<Vec<_>>());
        assert_eq!(shown, Some(vec!["TODO".to_owned()]));
        assert_eq!(painted_todo_words(&mut editor), ["TODO"], "the popup is drawn");
        editor.key(Key::Enter, Modifiers::NONE);
        assert_eq!(editor.text(), "package main\n\n// TODO");
        editor.key(Key::Enter, Modifiers::NONE);
        type_text(&mut editor, "func f() { to");
        assert!(painted_todo_words(&mut editor).is_empty(), "nothing in code");
    }

    #[test]
    fn enter_after_todo_typed_out_starts_a_new_line() {
        let mut editor = commented_go("package main\n\n");
        type_text(&mut editor, "// TODO");
        assert!(painted_todo_words(&mut editor).is_empty(), "no popup for the whole word");
        editor.key(Key::Enter, Modifiers::NONE);
        assert!(editor.text().starts_with("package main\n\n// TODO\n"), "{:?}", editor.text());
    }

    #[test]
    fn the_selected_completion_row_is_scrolled_into_view() {
        assert_eq!(revealed_offset(0.0, 3, 22.0, 10), 0.0, "in view already");
        assert_eq!(revealed_offset(0.0, 12, 22.0, 10), 22.0 * 3.0, "below: the last row shown");
        assert_eq!(revealed_offset(220.0, 2, 22.0, 10), 44.0, "above: the first row shown");
    }

    #[test]
    fn completion_documentation_goes_beside_the_list() {
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0));
        let list = Rect::from_min_size(pos2(100.0, 200.0), vec2(460.0, 220.0));
        assert_eq!(documentation_panel_at(list, window), pos2(564.0, 200.0), "to the right");
        let near_edge = list.translate(vec2(700.0, 0.0));
        assert_eq!(
            documentation_panel_at(near_edge, window),
            pos2(800.0 - 4.0 - DOCUMENTATION_WIDTH, 200.0),
            "flipped left"
        );
        let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(700.0, 900.0));
        let at = documentation_panel_at(list, narrow);
        assert!(at.x + DOCUMENTATION_WIDTH <= narrow.right() && at.x >= 0.0, "kept in the window: {at:?}");
    }

    #[test]
    fn an_http_request_is_sent_only_from_its_run_icon() {
        use zen_http::test_server::TestServer;
        let server = TestServer::start(vec![TestServer::json(200, "{\"ok\":true}")]);
        let text = format!(
            "### First\nGET {}\n\n###\nPOST {}\nContent-Type: application/json\n\n{{}}\n",
            server.url("/a"),
            server.url("/b")
        );
        let (mut editor, data) = Harness::with_http(&text);
        let listed = editor.view.run_targets.as_ref().map(|(_, targets)| targets.clone()).unwrap_or_default();
        assert_eq!(
            listed.iter().map(|target| (target.line, target.kind.as_str(), target.name.clone())).collect::<Vec<_>>(),
            [(1, HTTP, "First".to_owned()), (4, HTTP, format!("POST {}", server.url("/b")))]
        );
        for _ in 0..5 {
            editor.frame(Vec::new());
        }
        assert_eq!(server.connections(), 0, "opening and drawing the file sends nothing");
        editor.click(editor.run_icon_at(1, 9));
        assert!(editor.view.run_menu.is_none(), "the request runs at once");
        assert_eq!(editor.view.take_window_actions(), ["view.toolWindow.services"]);
        let listed = editor.registry.dispatch(&mut editor.session, "http.responses.list", json!({})).unwrap();
        let id = listed["responses"][0]["id"].clone();
        let got = editor
            .registry
            .dispatch(&mut editor.session, "http.response.get", json!({ "id": id, "waitMs": 10000 }))
            .unwrap();
        assert_eq!(got["state"], "done", "{got}");
        assert_eq!(got["response"]["status"], 200);
        assert_eq!(server.finish().iter().map(|seen| seen.target.as_str()).collect::<Vec<_>>(), ["/a"]);
        assert!(data.path().join("http-client").is_dir());
    }

    #[test]
    fn a_sql_files_statements_run_from_their_run_icons_and_the_keys() {
        let (mut editor, _data) = Harness::with_sql("SELECT 1;\n\nSELECT 2\nFROM t;\n");
        let listed = editor.view.run_targets.as_ref().map(|(_, targets)| targets.clone()).unwrap_or_default();
        assert_eq!(
            listed.iter().map(|target| (target.line, target.kind.as_str(), target.name.as_str())).collect::<Vec<_>>(),
            [(0, SQL, "SELECT 1"), (2, SQL, "SELECT 2")]
        );
        assert!(editor.view.take_gutter_actions().is_empty(), "drawing runs nothing");
        let path = editor._dir.path().join("report.sql");
        editor.key(Key::Enter, Modifiers::COMMAND);
        assert_eq!(editor.view.take_gutter_actions(), [("db.script.run".to_owned(), json!({ "path": path }))]);
        editor.key(Key::Enter, Modifiers::COMMAND | Modifiers::ALT);
        assert_eq!(
            editor.view.take_gutter_actions(),
            [("db.script.run".to_owned(), json!({ "path": path, "scope": "script" }))]
        );
        assert_eq!(editor.text(), "SELECT 1;\n\nSELECT 2\nFROM t;\n", "⌘⏎ types nothing");
        editor.click(editor.run_icon_at(2, 5));
        assert!(editor.view.run_menu.is_none(), "the statement runs at once");
        assert_eq!(
            editor.view.take_gutter_actions(),
            [("db.script.run".to_owned(), json!({ "path": path, "offset": 11 }))]
        );
    }

    #[test]
    fn run_icons_take_no_room_of_their_own_in_the_gutter() {
        let (mut plain, _) = Harness::with_runs(TESTS, &[]);
        assert_eq!(plain.view.run_targets.as_ref().map(|(_, targets)| targets.len()), Some(0));
        let char_width = plain.char_width();
        let line_three = |gutter: f32| pos2(gutter + TEXT_LEFT + char_width * 5.0, TOP + 22.0 * 2.5);
        plain.click(line_three(gutter_width(6, char_width, false)));
        assert_eq!(plain.caret(), 14 + 5);

        let (mut editor, _) = Harness::with_runs(TESTS, &[(2, "TestA"), (4, "TestB")]);
        let listed = editor.view.run_targets.as_ref().map(|(_, targets)| targets.clone()).unwrap_or_default();
        assert_eq!(
            listed.iter().map(|target| (target.line, target.name.as_str())).collect::<Vec<_>>(),
            [(2, "TestA"), (4, "TestB")]
        );
        editor.click(line_three(gutter_width(6, char_width, false)));
        assert_eq!(editor.caret(), 14 + 5, "the text starts where it does without run icons");
        assert!(editor.view.run_menu.is_none());
    }

    #[test]
    fn run_icons_and_the_text_line_up_at_any_font_size_and_zoom() {
        for (size, zoom) in [(13.0, 1.0), (20.0, 1.0), (10.0, 1.5), (18.0, 1.25)] {
            let (mut editor, _) = Harness::with_runs(TESTS, &[(2, "TestA"), (4, "TestB")]);
            editor.view.font.size = size;
            editor.ctx.set_zoom_factor(zoom);
            editor.frame(Vec::new());
            editor.frame(Vec::new());
            let char_width = editor.char_width();
            let line_height = editor.view.line_height;
            assert!((line_height * zoom - (22.0 * size / 13.0).round()).abs() < 1e-3, "{size} at {zoom}");
            let gutter = gutter_width(6, char_width, false);
            editor.click(pos2(gutter + TEXT_LEFT + char_width * 5.0, TOP + line_height * 2.5));
            assert_eq!(editor.caret(), 14 + 5, "{size} at {zoom}: the text starts after the gutter");
            assert!(editor.view.run_menu.is_none());
            editor.click(editor.run_icon_at(4, 6));
            editor.frame(Vec::new());
            assert!(editor.view.run_menu.is_some(), "{size} at {zoom}: the icon is where it is drawn");
        }
    }

    #[test]
    fn the_targets_are_listed_again_when_the_text_changes() {
        let (mut editor, fake) = Harness::with_runs(TESTS, &[(2, "TestA")]);
        fake.set_targets(Vec::new());
        editor.frame(Vec::new());
        assert_eq!(editor.view.run_targets.as_ref().map(|(_, targets)| targets.len()), Some(1), "same version");
        editor.frame(vec![Event::Text("x".to_owned())]);
        assert_eq!(
            editor.view.run_targets.as_ref().map(|(_, targets)| targets.len()),
            Some(1),
            "kept while typing goes on"
        );
        for _ in 0..40 {
            editor.frame(Vec::new());
        }
        assert_eq!(editor.view.run_targets.as_ref().map(|(_, targets)| targets.len()), Some(0));
    }

    #[test]
    fn a_run_icon_opens_its_menu_and_run_starts_its_configuration() {
        let (mut editor, fake) = Harness::with_runs(TESTS, &[(2, "TestA"), (4, "TestB")]);
        fake.set_context(Some(zen_run::fake::FakeProvider::configuration("TestB")));
        let icon = editor.run_icon_at(4, 6);
        editor.click(icon);
        let menu = editor.view.run_menu.as_ref().expect("the menu is open");
        assert_eq!(menu.target.name, "TestB");
        assert_eq!(editor.caret(), 0, "a click on the icon does not select the line");
        editor.frame(Vec::new());
        let items = editor.view.run_menu.as_ref().map(|menu| menu.items.clone()).unwrap_or_default();
        assert_eq!(items.len(), 3);
        editor.click(items[0].center());
        assert!(editor.view.run_menu.is_none());
        assert_eq!(fake.replays(), 1);
        let runs = editor.runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["name"], "TestB");
        let listed = editor.registry.dispatch(&mut editor.session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["configurations"][0]["temporary"], true, "the gutter runs its line's context");
        assert_eq!(runs[0]["configurationId"], listed["selected"]);
        assert_eq!(editor.view.take_window_actions(), ["view.toolWindow.run"]);

        editor.click(icon);
        editor.frame(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        assert!(editor.view.run_menu.is_none(), "Escape closes the menu");
        editor.click(icon);
        editor.click(pos2(600.0, 500.0));
        assert!(editor.view.run_menu.is_none(), "a click elsewhere closes the menu");
    }

    #[test]
    fn run_with_coverage_from_the_gutter_marks_the_lines_that_ran() {
        let (mut editor, fake) = Harness::with_runs(TESTS, &[(2, "TestA"), (4, "TestB")]);
        fake.set_context(Some(zen_run::fake::FakeProvider::configuration("TestA")));
        fake.set_coverage(Some("mode: count\nmain_test.go:3.1,3.20 1 2\nmain_test.go:5.1,5.20 1 0\n"));
        editor.click(editor.run_icon_at(2, 6));
        editor.frame(Vec::new());
        let items = editor.view.run_menu.as_ref().map(|menu| menu.items.clone()).unwrap_or_default();
        assert_eq!(items.len(), 4, "Run, Debug, Run with Coverage and Modify Run Configuration");
        assert!(editor.view.drawn_coverage.is_empty());
        editor.click(items[2].center());
        assert!(editor.view.run_menu.is_none());
        assert_eq!(editor.view.take_window_actions(), ["view.toolWindow.run"]);
        let run_id = editor.runs()[0]["runId"].as_u64().unwrap();
        let coverage = editor.session.state::<zen_workbench::coverage::Coverage>().cloned().unwrap();
        assert!(coverage.wait_loaded(run_id, std::time::Duration::from_secs(10)));
        editor.frame(Vec::new());
        assert_eq!(
            editor.view.drawn_coverage,
            [(2, zen_run::LineStatus::Covered), (4, zen_run::LineStatus::NotCovered)]
        );
        let hint = coverage_hint(&zen_run::LineCoverage {
            line: 3,
            hits: 2,
            status: zen_run::LineStatus::Partly,
            branches: Some(zen_run::BranchCounts { covered: 1, total: 2 }),
        });
        assert_eq!(
            hint,
            format!("{}\n{}", t!("ide-coverage-hits", hits = 2), t!("ide-coverage-branches", covered = 1, total = 2))
        );
        editor.registry.dispatch(&mut editor.session, "coverage.report.hide", json!({})).unwrap();
        editor.frame(Vec::new());
        assert!(editor.view.drawn_coverage.is_empty(), "hidden coverage leaves the gutter");
    }

    #[test]
    fn modify_run_configuration_selects_a_temporary_configuration_and_opens_the_dialog() {
        let (mut editor, fake) = Harness::with_runs(TESTS, &[(2, "TestA")]);
        fake.set_context(Some(zen_run::fake::FakeProvider::configuration("TestA")));
        editor.click(editor.run_icon_at(2, 6));
        editor.frame(Vec::new());
        let items = editor.view.run_menu.as_ref().map(|menu| menu.items.clone()).unwrap_or_default();
        editor.click(items[2].center());
        assert_eq!(editor.view.take_window_actions(), ["view.runConfigurations.open"]);
        let listed = editor.registry.dispatch(&mut editor.session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["configurations"].as_array().map(Vec::len), Some(1));
        assert_eq!(listed["configurations"][0]["temporary"], true);
        assert_eq!(listed["selected"], listed["configurations"][0]["id"]);
        editor.click(editor.run_icon_at(2, 6));
        editor.frame(Vec::new());
        let items = editor.view.run_menu.as_ref().map(|menu| menu.items.clone()).unwrap_or_default();
        editor.click(items[2].center());
        let listed = editor.registry.dispatch(&mut editor.session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["configurations"].as_array().map(Vec::len), Some(1), "the saved one is reused");
    }

    #[test]
    fn a_run_icon_shows_how_the_last_run_of_its_test_went() {
        let (mut editor, fake) = Harness::with_runs(TESTS, &[(0, "main"), (2, "TestA"), (4, "TestB")]);
        let key = |name: &str| ("p".to_owned(), name.to_owned());
        assert!(editor.view.test_marks.is_empty());
        fake.set_script(
            &[
                (zen_run::Stream::Stdout, "@start p TestA"),
                (zen_run::Stream::Stdout, "@fail p TestA 3"),
                (zen_run::Stream::Stdout, "@start p TestB"),
                (zen_run::Stream::Stdout, "@pass p TestB 1"),
            ],
            1,
        );
        let configuration = serde_json::to_value(zen_run::fake::FakeProvider::configuration("all")).unwrap();
        editor
            .registry
            .dispatch(&mut editor.session, "run.process.start", json!({ "configuration": configuration }))
            .unwrap();
        editor.frame(Vec::new());
        assert_eq!(editor.view.test_marks.get(&key("TestA")), Some(&TestMark::Failed));
        assert_eq!(editor.view.test_marks.get(&key("TestB")), Some(&TestMark::Passed));
        fake.set_script(
            &[(zen_run::Stream::Stdout, "@start p TestA"), (zen_run::Stream::Stdout, "@pass p TestA 3")],
            0,
        );
        editor
            .registry
            .dispatch(&mut editor.session, "run.process.start", json!({ "configuration": configuration }))
            .unwrap();
        editor.frame(Vec::new());
        assert_eq!(editor.view.test_marks.get(&key("TestA")), Some(&TestMark::Passed), "the latest run wins");
        assert_eq!(
            editor.view.test_marks.get(&key("TestB")),
            Some(&TestMark::Passed),
            "kept from the run that covered it"
        );
        fake.set_script(
            &[(zen_run::Stream::Stdout, "@start q TestA"), (zen_run::Stream::Stdout, "@fail q TestA 3")],
            1,
        );
        editor
            .registry
            .dispatch(&mut editor.session, "run.process.start", json!({ "configuration": configuration }))
            .unwrap();
        editor.frame(Vec::new());
        let icon = editor.view.run_targets.as_ref().unwrap().1[1].clone();
        assert_eq!(
            editor.view.test_mark_of(&icon),
            Some(TestMark::Passed),
            "a test of another package is another test"
        );
    }

    #[test]
    fn subtests_that_went_different_ways_make_a_mixed_mark() {
        let test = |status: &str, children: Value| json!({ "kind": "test", "package": "p", "test": "TestA", "status": status, "children": children });
        let sub = |status: &str| json!({ "kind": "test", "test": "TestA/x", "status": status, "children": [] });
        assert_eq!(test_mark(&test("passed", json!([sub("passed")]))), Some(TestMark::Passed));
        assert_eq!(test_mark(&test("failed", json!([sub("failed")]))), Some(TestMark::Failed));
        assert_eq!(test_mark(&test("failed", json!([sub("passed"), sub("failed")]))), Some(TestMark::Mixed));
        assert_eq!(test_mark(&test("passed", json!([sub("skipped")]))), Some(TestMark::Mixed));
        assert_eq!(test_mark(&test("running", json!([]))), None);
        let tree = json!({ "kind": "root", "status": "failed", "children": [
            { "kind": "package", "status": "failed", "children": [test("failed", json!([sub("failed")]))] }
        ] });
        let mut found = Vec::new();
        top_level_tests(&tree, &mut found);
        assert_eq!(found, [("p".to_owned(), "TestA".to_owned(), TestMark::Failed)]);
    }

    #[test]
    fn control_shift_r_runs_the_context_at_the_caret_of_the_active_editor() {
        let (mut editor, fake) = Harness::with_runs(TESTS, &[]);
        fake.set_context(Some(zen_run::fake::FakeProvider::configuration("TestA")));
        let stroke = crate::keymap::keystroke(Key::R, Modifiers { ctrl: true, shift: true, ..Modifiers::NONE });
        editor.key(Key::R, Modifiers { ctrl: true, shift: true, ..Modifiers::NONE });
        assert!(editor.runs().is_empty(), "the window's shortcut, not the editor's");
        let binding = zen_core::keymap::Keymap::default_macos()
            .lookup(&stroke, zen_core::keymap::Context::Editor)
            .expect("⌃⇧R is bound in the editor");
        assert_eq!(binding.action, "run.process.start/context");
        assert_eq!(binding.context, zen_core::keymap::Context::Global);
        editor.registry.dispatch(&mut editor.session, &binding.command, binding.params.clone()).unwrap();
        let runs = editor.runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["name"], "TestA");
        let listed = editor.registry.dispatch(&mut editor.session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["configurations"][0]["temporary"], true, "saved as a temporary configuration");
        assert_eq!(listed["selected"], listed["configurations"][0]["id"]);
        assert_eq!(runs[0]["configurationId"], listed["selected"]);
    }

    #[test]
    fn the_keypad_star_and_a_digit_expand_to_that_level() {
        let text = "package main\n\nfunc f() {\n\tif x {\n\t\tfor {\n\t\t\ty()\n\t\t}\n\t}\n}\n";
        let mut editor = Harness::new(text);
        let folded = |editor: &mut Harness| -> Vec<u64> {
            let folds = editor.registry.dispatch(&mut editor.session, "editor.fold.list", json!({})).unwrap();
            folds
                .as_array()
                .unwrap()
                .iter()
                .filter(|fold| fold["folded"] == true)
                .map(|fold| fold["startLine"].as_u64().unwrap())
                .collect()
        };
        let digit = |key: Key, text: &str| {
            vec![
                Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
                Event::Text(text.to_owned()),
            ]
        };
        editor.registry.dispatch(&mut editor.session, "editor.fold.collapseAll", json!({})).unwrap();
        assert_eq!(folded(&mut editor), [2, 3, 4]);
        if cfg!(target_os = "macos") {
            crate::keypad::press(false, true, false, true);
        } else {
            crate::keypad::press(false, false, true, true);
        }
        editor.frame(Vec::new());
        editor.frame(digit(Key::Num2, "2"));
        assert_eq!(folded(&mut editor), [4], "Expand All to Level 2");
        assert_eq!(editor.text(), text, "the digit completes the shortcut and is not typed");
        let line = text.find("\tif").unwrap();
        editor
            .registry
            .dispatch(&mut editor.session, "editor.selection.set", json!({ "selections": [{ "head": line }] }))
            .unwrap();
        editor.registry.dispatch(&mut editor.session, "editor.fold.expandAll", json!({})).unwrap();
        crate::keypad::press(false, false, false, true);
        editor.frame(digit(Key::Num1, "1"));
        assert_eq!(folded(&mut editor), [4], "Expand to Level 1 at the `if`");
        editor.frame(digit(Key::Num3, "3"));
        assert!(editor.text().contains("3\tif"), "a digit alone is typed: {}", editor.text());
        crate::keypad::press(false, false, false, true);
        editor.frame(Vec::new());
        assert!(editor.view.takes_key(Key::Num2, Modifiers::NONE), "the window leaves the second keystroke");
        assert!(!editor.view.takes_key(Key::A, Modifiers::NONE));
        editor.frame(digit(Key::A, "a"));
        assert!(editor.text().contains("3a\tif"), "a key that completes nothing is typed: {}", editor.text());
        assert_eq!(folded(&mut editor), [4]);
        crate::keypad::press(false, false, false, true);
        editor.frame(Vec::new());
        let (_, since) = editor.view.sequence.as_mut().expect("waiting for the second keystroke");
        *since = since.checked_sub(SECOND_KEYSTROKE_WAIT).unwrap_or(*since);
        assert!(!editor.view.takes_key(Key::Num2, Modifiers::NONE), "no longer waiting");
        editor.frame(digit(Key::Num2, "2"));
        assert!(editor.text().contains("3a2\tif"), "too late, the digit is typed: {}", editor.text());
        assert_eq!(folded(&mut editor), [4], "and folds nothing");
    }

    #[test]
    fn reformat_says_why_nothing_changed() {
        let mut editor = Harness::new("func  main() {}\n");
        editor.key(Key::L, Modifiers::ALT | Modifiers::COMMAND);
        assert_eq!(editor.view.take_notice(), Some(zen_i18n::t!("workbench-language-not-running")));
        assert_eq!(editor.text(), "func  main() {}\n");
    }

    #[test]
    fn a_reformat_still_pending_is_polled_until_it_is_applied() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "func  main() {}\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let server = zen_lsp::fake::FakeServer::new();
        let launched = server.clone();
        session.state_mut::<zen_workbench::language::Languages>().set_specs(vec![
            zen_workbench::language::ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch: Arc::new(move |_| Ok(launched.launcher())),
                initialization_options: json!({}),
            },
        ]);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let mut editor =
            Harness { ctx: egui::Context::default(), registry, session, view: EditorView::default(), _dir: dir };
        let path = editor.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while editor.session.state::<zen_workbench::language::Languages>().unwrap().synced_text(&path).is_none() {
            assert!(std::time::Instant::now() < deadline, "the server did not get the file");
            let _ = editor.registry.dispatch(&mut editor.session, "language.servers.sync", json!({}));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let edits = json!([{
            "range": { "start": { "line": 0, "character": 4 }, "end": { "line": 0, "character": 6 } },
            "newText": " "
        }]);
        server.reply(
            "textDocument/formatting",
            zen_lsp::fake::Reply::Delayed(edits, std::time::Duration::from_millis(1300)),
        );
        editor.frame(Vec::new());
        assert!(editor.view.focus_id.is_some_and(|id| editor.ctx.memory(|memory| memory.has_focus(id))));
        editor.registry.dispatch(&mut editor.session, "editor.macro.startRecording", json!({})).unwrap();
        editor.key(Key::L, Modifiers::ALT | Modifiers::COMMAND);
        assert!(editor.view.format_pending.is_some(), "still pending after the first second");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while editor.text() != "func main() {}\n" {
            assert!(std::time::Instant::now() < deadline, "the reformat was not applied");
            editor.frame(Vec::new());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(editor.view.format_pending.is_none());
        assert_eq!(server.requests("textDocument/formatting").len(), 1);
        let recorded = editor.session.state::<zen_core::macros::Recorder>().unwrap().steps().to_vec();
        assert_eq!(
            recorded.iter().map(|step| step.command.as_str()).collect::<Vec<_>>(),
            ["editor.code.reformat"],
            "polling for the answer is not a step of a macro: {recorded:?}"
        );
        assert!(recorded[0].params.get("path").is_none(), "{recorded:?}");
    }

    impl Harness {
        fn with_hints(contents: &str, hints: Value) -> (Self, zen_lsp::fake::FakeServer) {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("main.go");
            std::fs::write(&file, contents).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            let server = zen_lsp::fake::FakeServer::new();
            server.set_capabilities(
                json!({ "textDocumentSync": { "openClose": true, "change": 2 }, "inlayHintProvider": true }),
            );
            server.on("textDocument/inlayHint", hints);
            let launched = server.clone();
            session.state_mut::<zen_workbench::language::Languages>().set_specs(vec![
                zen_workbench::language::ServerSpec {
                    language_id: "go".to_owned(),
                    extensions: vec!["go".to_owned()],
                    launch: Arc::new(move |_| Ok(launched.launcher())),
                    initialization_options: json!({}),
                },
            ]);
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let mut harness =
                Self { ctx: egui::Context::default(), registry, session, view: EditorView::default(), _dir: dir };
            harness.frame(Vec::new());
            (harness, server)
        }

        fn hint_boxes(&mut self, count: usize) -> Vec<Rect> {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let _ = self.registry.dispatch(&mut self.session, "language.servers.sync", json!({}));
                self.frame(Vec::new());
                let drawn: Vec<Rect> = self.view.inlay_hints.drawn().iter().map(|(rect, _)| *rect).collect();
                if drawn.len() == count {
                    return drawn;
                }
                assert!(std::time::Instant::now() < deadline, "{count} hints were not drawn");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn inlay_hints_are_drawn_between_characters_that_the_caret_and_clicks_step_over() {
        let text = "package main\n\nfunc main() {\n\tgreet(\"hi\", 3)\n}\n";
        let hints = json!([
            { "position": { "line": 3, "character": 7 }, "label": "name:", "kind": 2, "paddingRight": true },
            { "position": { "line": 3, "character": 13 }, "label": "times:", "kind": 2, "paddingRight": true }
        ]);
        let (mut editor, _server) = Harness::with_hints(text, hints);
        let boxes = editor.hint_boxes(2);
        let quote = text.find('"').unwrap();
        editor.set_caret(quote - 1);
        let before = editor.view.caret_rect.unwrap().left();
        editor.set_caret(quote);
        let at = editor.view.caret_rect.unwrap().left();
        editor.set_caret(quote + 1);
        let after = editor.view.caret_rect.unwrap().left();
        let char_width = editor.char_width();
        assert!((at - before - char_width).abs() < 0.5, "a caret at the hint is drawn before it");
        assert!(at <= boxes[0].left() && boxes[0].right() <= after - char_width, "the quote is drawn after the hint");
        assert!(boxes[0].width() > 2.0 * char_width && boxes[0].height() > 0.0);
        assert!((boxes[0].top() - boxes[1].top()).abs() < 0.1 && boxes[1].left() > boxes[0].right());
        editor.click(boxes[0].center() + vec2(boxes[0].width() / 4.0, 0.0));
        assert_eq!(editor.caret(), quote, "a click on a hint lands at its place");
        editor.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(editor.caret(), quote + 1);
        editor.set_caret(quote);
        editor.frame(vec![Event::Text("x".to_owned())]);
        let_hint_follow(&mut editor, boxes[0].left() + char_width);
    }

    #[test]
    fn a_selection_from_a_hinted_place_starts_after_the_hint() {
        let text = "package main\n\nfunc main() {\n\tgreet(\"hi\", 3)\n}\n";
        let hints = json!([{ "position": { "line": 3, "character": 7 }, "label": "name:", "kind": 2 }]);
        let (mut editor, _server) = Harness::with_hints(text, hints);
        let hint = editor.hint_boxes(1)[0];
        let quote = text.find('"').unwrap();
        editor
            .registry
            .dispatch(
                &mut editor.session,
                "editor.selection.set",
                json!({ "selections": [{ "anchor": quote, "head": quote + 4 }] }),
            )
            .unwrap();
        let palette = IdePalette::zen_dark();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
            ..Default::default()
        };
        let output = editor.ctx.run_ui(input, |ui| {
            editor.view.show(ui, &editor.registry, &mut editor.session, None, &palette);
        });
        let bands: Vec<Rect> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) if rect.fill == palette.editor_selection => Some(rect.rect),
                _ => None,
            })
            .collect();
        assert_eq!(bands.len(), 1);
        assert!(bands[0].left() >= hint.right(), "the band {:?} leaves the hint {hint:?} out", bands[0]);
    }

    #[test]
    fn a_hint_on_a_wrapped_line_is_drawn_on_the_row_of_its_place() {
        let words: String = (0..40).map(|i| format!(" w{i}")).collect();
        let text = format!("package main\n\nfunc main() {{\n\tgreet(\"hi\"{words})\n}}\n");
        let column = text.lines().nth(3).unwrap().find("w30").unwrap();
        let hints = json!([{ "position": { "line": 3, "character": column }, "label": "name:", "kind": 2 }]);
        let (mut editor, _server) = Harness::with_hints(&text, hints);
        editor.registry.dispatch(&mut editor.session, "editor.softWrap.toggle", json!({})).unwrap();
        let hint = editor.hint_boxes(1)[0];
        let starts = editor.row_starts(3);
        assert!(starts.len() >= 2, "the line wraps: {starts:?}");
        let row = starts.iter().filter(|&&start| start <= column).count() - 1;
        assert!(row > 0, "the hint is past the first row: {starts:?}");
        let line_height = editor.view.line_height;
        let rows_above = editor.view.soft_wrap.kept().unwrap().top(3);
        let top = (rows_above + row) as f32 * line_height;
        assert!(hint.top() >= top && hint.bottom() <= top + line_height, "{hint:?} on row {row} at {top}");
        let place = text.find("w30").unwrap();
        editor.click(hint.center());
        assert_eq!(editor.caret(), place, "a click on the hint lands at its place");
    }

    fn let_hint_follow(editor: &mut Harness, left: f32) {
        let _ = editor.registry.dispatch(&mut editor.session, "language.servers.sync", json!({}));
        editor.frame(Vec::new());
        let moved = editor.view.inlay_hints.drawn()[0].0.left();
        assert!((moved - left).abs() < 0.5, "the hint moves with the typed text at once: {moved} for {left}");
    }

    #[test]
    fn typing_and_keys_edit_through_commands() {
        let mut editor = Harness::new("func main() {}\n");
        editor.key(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(editor.caret(), 4, "⌥→ goes to the end of the word");
        editor.frame(vec![Event::Text("X".to_owned())]);
        assert_eq!(editor.text(), "funcX main() {}\n");
        editor.key(Key::Backspace, Modifiers::NONE);
        editor.key(Key::ArrowLeft, Modifiers::COMMAND | Modifiers::SHIFT);
        editor.frame(vec![Event::Text("fn".to_owned())]);
        assert_eq!(editor.text(), "fn main() {}\n", "⇧⌘← selects to the line start, typing replaces it");
        editor.key(Key::ArrowDown, Modifiers::COMMAND);
        editor.key(Key::Enter, Modifiers::NONE);
        editor.key(Key::Tab, Modifiers::NONE);
        assert_eq!(editor.text(), "fn main() {}\n\n\t");
        editor.key(Key::A, Modifiers::COMMAND);
        editor.key(Key::Delete, Modifiers::NONE);
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn paste_and_input_method_commit_insert_text() {
        let mut editor = Harness::new("ab");
        editor.key(Key::ArrowRight, Modifiers::NONE);
        editor.frame(vec![Event::Paste("123".to_owned())]);
        assert_eq!(editor.text(), "a123b");
        editor.frame(vec![Event::Ime(egui::ImeEvent::Preedit { text: "ni".to_owned(), active_range_chars: None })]);
        assert_eq!(editor.view.preedit, "ni", "the composition is shown, not inserted");
        assert_eq!(editor.text(), "a123b");
        editor.frame(vec![Event::Ime(egui::ImeEvent::Commit("你".to_owned()))]);
        assert_eq!(editor.text(), "a123你b");
        assert!(editor.view.preedit.is_empty());
    }

    #[test]
    fn a_click_places_the_caret_and_shift_click_selects() {
        let mut editor = Harness::new("first line\nsecond line\n");
        let gutter = gutter_width(2, editor.char_width(), false);
        let char_width = editor.ctx.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let line_height = 22.0;
        let at = pos2(gutter + TEXT_LEFT + char_width * 3.0, TOP + line_height * 1.5);
        let press = |pressed: bool, modifiers: Modifiers| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        editor.frame(vec![Event::PointerMoved(at), press(true, Modifiers::NONE)]);
        editor.frame(vec![press(false, Modifiers::NONE)]);
        editor.frame(Vec::new());
        assert_eq!(editor.caret(), 11 + 3, "line 2, column 3");
    }

    #[test]
    fn go_to_test_without_a_test_offers_create_new_test_which_opens_the_create_test_dialog() {
        let mut editor = Harness::new("package main\n\nfunc Total() int {\n\treturn 0\n}\n");
        let declared = json!([{
            "id": "go-like", "extensions": ["go"], "pairs": [{ "subject": "{name}.go", "test": "{name}_test.go" }],
            "members": "^func (?P<name>\\w+)", "tests": ["Test{Name}"],
            "frameworks": [{ "id": "t", "name": "T", "file": "", "method": "" }]
        }]);
        let support = zen_run::test_support::DeclaredTests::parse_list(&declared).pop().unwrap();
        editor.session.state_mut::<zen_run::test_support::TestSupports>().add(Arc::new(support));
        editor.set_caret(20);
        editor.key(Key::T, Modifiers::COMMAND | Modifiers::SHIFT);
        editor.frame(Vec::new());
        assert!(editor.view.take_window_actions().is_empty(), "the chooser asks first");
        editor.key(Key::Enter, Modifiers::NONE);
        assert_eq!(editor.view.take_window_actions(), ["view.createTest.open"]);
    }

    #[test]
    fn a_lookup_shortcut_that_cannot_run_tells_why_once() {
        let mut editor = Harness::new("func main() {}\n");
        editor.key(Key::F7, Modifiers::ALT);
        assert_eq!(editor.view.take_notice(), Some(zen_i18n::t!("workbench-language-not-running")));
        assert_eq!(editor.view.take_notice(), None);
        editor.key(Key::F1, Modifiers::NONE);
        assert_eq!(editor.view.take_notice(), Some(zen_i18n::t!("workbench-language-not-running")));
        editor.key(Key::B, Modifiers::COMMAND);
        assert_eq!(editor.view.take_notice(), Some(zen_i18n::t!("workbench-language-not-running")));
        editor.frame(Vec::new());
        assert_eq!(editor.view.take_notice(), None, "the lookup's outcome is told once");
    }

    #[test]
    fn a_command_click_on_a_word_asks_for_its_declaration_at_the_click() {
        #[derive(Default)]
        struct Asked(Vec<Value>);
        struct Record;
        impl zen_core::Command for Record {
            type Params = Value;
            type Output = ();
            const ID: &'static str = "navigate.declaration.goTo";
            const TITLE: &'static str = "Record";
            const DESCRIPTION: &'static str = "Records its parameters.";
            fn run(&self, session: &mut Session, params: Value) -> Result<(), zen_core::CommandError> {
                session.state_mut::<Asked>().0.push(params);
                Ok(())
            }
        }
        let mut editor = Harness::new("first  line\nsecond line\n");
        let path = editor.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        editor.registry = Registry::new();
        editor.registry.register(Record).unwrap();
        let gutter = gutter_width(2, editor.char_width(), false);
        let char_width = editor.ctx.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let line_height = 22.0;
        let press = |at: egui::Pos2, pressed: bool| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::COMMAND,
        };
        let word = pos2(gutter + TEXT_LEFT + char_width * 3.0, TOP + line_height * 1.5);
        editor.frame_with(vec![Event::PointerMoved(word), press(word, true)], Modifiers::COMMAND);
        editor.frame_with(vec![press(word, false)], Modifiers::COMMAND);
        let asked = editor.session.state::<Asked>().map(|asked| asked.0.clone()).unwrap_or_default();
        assert_eq!(asked, vec![json!({ "path": path, "offset": 12 + 3 })]);
        assert_eq!(editor.caret(), 0, "the caret stays where it was");

        let space = pos2(gutter + TEXT_LEFT + char_width * 6.0, TOP + line_height * 0.5);
        editor.frame_with(vec![Event::PointerMoved(space), press(space, true)], Modifiers::COMMAND);
        editor.frame_with(vec![press(space, false)], Modifiers::COMMAND);
        assert_eq!(editor.session.state::<Asked>().unwrap().0.len(), 1, "not between words");
    }

    #[test]
    fn a_shift_command_click_on_a_word_asks_for_its_type_declaration() {
        #[derive(Default)]
        struct Asked(Vec<Value>);
        struct Record;
        impl zen_core::Command for Record {
            type Params = Value;
            type Output = ();
            const ID: &'static str = "navigate.typeDeclaration.goTo";
            const TITLE: &'static str = "Record";
            const DESCRIPTION: &'static str = "Records its parameters.";
            fn run(&self, session: &mut Session, params: Value) -> Result<(), zen_core::CommandError> {
                session.state_mut::<Asked>().0.push(params);
                Ok(())
            }
        }
        let mut editor = Harness::new("first  line\nsecond line\n");
        let path = editor.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        editor.registry = Registry::new();
        editor.registry.register(Record).unwrap();
        let gutter = gutter_width(2, editor.char_width(), false);
        let char_width = editor.ctx.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let modifiers = Modifiers::COMMAND | Modifiers::SHIFT;
        let word = pos2(gutter + TEXT_LEFT + char_width * 3.0, TOP + 22.0 * 1.5);
        let press = |pressed: bool| Event::PointerButton {
            pos: word,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        editor.frame_with(vec![Event::PointerMoved(word), press(true)], modifiers);
        editor.frame_with(vec![press(false)], modifiers);
        let asked = editor.session.state::<Asked>().map(|asked| asked.0.clone()).unwrap_or_default();
        assert_eq!(asked, vec![json!({ "path": path, "offset": 12 + 3 })]);
        assert_eq!(editor.caret(), 0, "the caret stays where it was");
    }

    #[test]
    fn double_and_triple_clicks_select_a_word_and_a_line_at_the_press() {
        let mut editor = Harness::new("first line\nsecond line\n");
        let gutter = gutter_width(2, editor.char_width(), false);
        let char_width = editor.ctx.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let line_height = 22.0;
        let at = pos2(gutter + TEXT_LEFT + char_width * 2.0, TOP + line_height * 1.5);
        let press = |pressed: bool| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let selection = |editor: &Harness| {
            let document = editor.session.state::<Workbench>().unwrap().active_document().unwrap().clone();
            let range = document.selections().primary().range();
            document.text().chars().skip(range.start).take(range.len()).collect::<String>()
        };
        editor.frame(vec![Event::PointerMoved(at), press(true)]);
        editor.frame(vec![press(false)]);
        editor.frame(vec![press(true)]);
        assert_eq!(selection(&editor), "second");
        editor.frame(vec![press(false)]);
        editor.frame(vec![press(true)]);
        assert_eq!(selection(&editor).trim_end(), "second line", "the third press selects the line");
        editor.frame(vec![press(false)]);
    }

    #[test]
    fn a_double_click_selects_the_camel_hump_when_both_settings_are_on() {
        let mut editor = Harness::new("fooBarBaz x\n");
        let gutter = gutter_width(2, editor.char_width(), false);
        let char_width = editor.ctx.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let at = pos2(gutter + TEXT_LEFT + char_width * 4.2, TOP + 11.0);
        let press = |pressed: bool| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let selection = |editor: &Harness| {
            let document = editor.session.state::<Workbench>().unwrap().active_document().unwrap().clone();
            let range = document.selections().primary().range();
            document.text().chars().skip(range.start).take(range.len()).collect::<String>()
        };
        let double_click = |editor: &mut Harness| {
            editor.frame(vec![Event::PointerMoved(at), press(true)]);
            editor.frame(vec![press(false)]);
            editor.frame(vec![press(true)]);
            editor.frame(vec![press(false)]);
            editor.ctx.input_mut(|input| input.time += 1.0);
            selection(editor)
        };
        assert_eq!(double_click(&mut editor), "fooBarBaz", "off by default");
        editor.session.settings_mut().set(zen_workbench::editing::CAMEL_HUMPS, true).unwrap();
        editor.frame(Vec::new());
        assert_eq!(double_click(&mut editor), "Bar");
        editor.session.settings_mut().set(zen_workbench::editing::CAMEL_HUMPS_ON_DOUBLE_CLICK, false).unwrap();
        editor.frame(Vec::new());
        assert_eq!(double_click(&mut editor), "fooBarBaz", "the second check box off");
    }

    #[test]
    fn option_arrows_extend_and_shrink_and_option_twice_still_clones() {
        let mut editor = Harness::new("package main\n\nfunc f() {\n\tg(a, b)\n}\n");
        editor.set_caret(28);
        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        let arrow = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: alt };
        let selected = |editor: &Harness| {
            let document = editor.session.state::<Workbench>().unwrap().active_document().unwrap();
            let selections: Vec<_> = document.selections().iter().map(|selection| selection.range()).collect();
            selections
        };
        editor.frame_with(Vec::new(), alt);
        editor.frame_with(vec![arrow(Key::ArrowUp)], alt);
        assert_eq!(selected(&editor), vec![28..29], "⌥↑ selects the word");
        editor.frame_with(Vec::new(), Modifiers::NONE);
        editor.frame_with(Vec::new(), alt);
        editor.frame_with(vec![arrow(Key::ArrowUp)], alt);
        assert_eq!(selected(&editor), vec![28..32], "a second ⌥↑ right after the first extends again");
        editor.frame_with(vec![arrow(Key::ArrowDown)], alt);
        assert_eq!(selected(&editor), vec![28..29], "⌥↓ walks back");
        editor.frame_with(Vec::new(), Modifiers::NONE);
        editor.frame_with(Vec::new(), alt);
        editor.frame_with(Vec::new(), Modifiers::NONE);
        editor.frame_with(Vec::new(), alt);
        editor.frame_with(vec![arrow(Key::ArrowUp)], alt);
        assert_eq!(selected(&editor).len(), 2, "⌥⌥↑ clones the caret instead");
    }

    #[test]
    fn occurrence_and_clone_keys() {
        let mut editor = Harness::new("a := a + a\nb\nc\n");
        let ctrl = Modifiers { ctrl: true, ..Modifiers::NONE };
        editor.key(Key::G, ctrl);
        editor.key(Key::G, ctrl);
        let count = |editor: &Harness| {
            editor.session.state::<Workbench>().unwrap().active_document().unwrap().selections().len()
        };
        assert_eq!(count(&editor), 2, "⌃G twice: the word, then its next occurrence");
        editor.key(Key::G, Modifiers { ctrl: true, command: true, mac_cmd: true, ..Modifiers::NONE });
        assert_eq!(count(&editor), 3, "⌃⌘G: all occurrences");
        editor.key(Key::Escape, Modifiers::NONE);
        assert_eq!(count(&editor), 1);
        for _ in 0..2 {
            editor
                .registry
                .dispatch(&mut editor.session, "editor.caret.clone", json!({ "direction": "down" }))
                .unwrap();
        }
        assert_eq!(count(&editor), 3, "two clones: three carets");
        editor.frame(vec![Event::Paste("1\n2\n3".to_owned())]);
        assert_eq!(editor.text(), "a := 1 + a\nb2\nc3\n");
    }

    #[test]
    fn indent_guides_the_right_margin_and_whitespaces_are_painted() {
        let mut editor = Harness::new("func main() {\n\tif x {\n\t\ta()  \n\t}\n}\n");
        let palette = IdePalette::zen_dark();
        let lines = |shapes: &[egui::Shape], color: Color32| {
            shapes
                .iter()
                .filter(|shape| matches!(shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == color))
                .count()
        };
        let dots = |shapes: &[egui::Shape]| {
            shapes
                .iter()
                .filter(|shape| matches!(shape, egui::Shape::Circle(circle) if circle.fill == palette.whitespace))
                .count()
        };
        let shapes = editor.painted();
        assert_eq!(lines(&shapes, palette.indent_guide), 4, "one guide for each level of lines 2 to 4");
        assert_eq!(lines(&shapes, palette.indent_guide_selected), 0, "the caret is on a top-level line");
        assert!(lines(&shapes, palette.right_margin) >= 1, "the margin at column 120");
        assert_eq!(dots(&shapes), 0, "whitespaces are hidden by default");

        editor.set_caret(editor.text().find("a()").unwrap());
        let shapes = editor.painted();
        assert_eq!(lines(&shapes, palette.indent_guide_selected), 1, "the if's guide on the caret's line");
        assert_eq!(lines(&shapes, palette.indent_guide), 3);

        editor.registry.dispatch(&mut editor.session, "editor.whitespaces.toggle", json!({})).unwrap();
        let shapes = editor.painted();
        assert_eq!(dots(&shapes), 6, "the spaces of lines 1 and 2 and the trailing two of line 3");
        assert_eq!(lines(&shapes, palette.whitespace), 4, "a line for each tab");

        editor.registry.dispatch(&mut editor.session, "editor.indentGuides.toggle", json!({})).unwrap();
        let shapes = editor.painted();
        assert_eq!(lines(&shapes, palette.indent_guide) + lines(&shapes, palette.indent_guide_selected), 0);
        let _ = editor.session.settings_mut().set(zen_workbench::visual_aids::SHOW_HARD_WRAP_GUIDES, false);
        assert_eq!(lines(&editor.painted(), palette.right_margin), 0);
    }

    #[test]
    fn visual_aids_render_offscreen() {
        let text = "package main\n\nfunc main() {\n\tfor i := 0; i < 3; i++ {\n\t\tif i > 1 {\n\t\t\tprintln(i)   \n\n\t\t}\n\t}\n}\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let caret = text.find("println").unwrap();
        registry
            .dispatch(
                &mut session,
                "editor.selection.set",
                json!({ "selections": [{ "anchor": caret, "head": caret }] }),
            )
            .unwrap();
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 260.0), 2.0);
        palette.apply(screen.context());
        let mut view = EditorView::default();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let mut draw = |screen: &mut crate::offscreen::Offscreen, session: &mut Session, name: &str| {
            for _ in 0..2 {
                screen.step(Vec::new(), |ui| {
                    ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                    view.show(ui, &registry, session, None, &palette);
                });
            }
            let image = screen.frame(Vec::new(), |ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                view.show(ui, &registry, session, None, &palette);
            });
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(name), &image).unwrap();
            }
            image
        };
        let guides = draw(&mut screen, &mut session, "visual-aids-guides.png");
        let aids = zen_workbench::visual_aids::VisualAidSettings { whitespaces: true, ..Default::default() };
        aids.apply_to(session.settings_mut());
        let whitespaces = draw(&mut screen, &mut session, "visual-aids-whitespaces.png");
        let arrows = zen_workbench::visual_aids::VisualAidSettings {
            whitespaces: true,
            tab_rendering: zen_workbench::visual_aids::TabRendering::Arrow,
            ..Default::default()
        };
        arrows.apply_to(session.settings_mut());
        let arrowed = draw(&mut screen, &mut session, "visual-aids-arrows.png");
        let none = zen_workbench::visual_aids::VisualAidSettings {
            hard_wrap_guides: false,
            indent_guides: false,
            ..Default::default()
        };
        none.apply_to(session.settings_mut());
        let plain = draw(&mut screen, &mut session, "visual-aids-off.png");
        assert_ne!(guides.pixels, plain.pixels, "the guides and the margin show");
        assert_ne!(whitespaces.pixels, guides.pixels, "the whitespaces show");
        assert_ne!(arrowed.pixels, whitespaces.pixels, "tabs as arrows differ from lines");
    }

    #[test]
    #[ignore = "a benchmark, run by hand in release"]
    fn typing_with_the_visual_aids() {
        let mut text = String::from("package main\n\n");
        for index in 0..2_000 {
            text.push_str(&format!(
                "func f{index}(a int) int {{\n\tif a > 0 {{\n\t\treturn  a + 1   \n\n\t}}\n\treturn a // done\n}}\n\n"
            ));
        }
        let mut editor = Harness::new(&text);
        let middle = text.len() / 2;
        let at = middle + text[middle..].find("return  a").unwrap() + 6;
        editor.set_caret(at);
        let measure = |editor: &mut Harness, aids: zen_workbench::visual_aids::VisualAidSettings| {
            aids.apply_to(editor.session.settings_mut());
            for _ in 0..20 {
                editor.frame(Vec::new());
            }
            let mut times = Vec::new();
            for index in 0..6000 {
                let event = if index % 2 == 0 {
                    Event::Text("x".to_owned())
                } else {
                    Event::Key {
                        key: Key::Backspace,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    }
                };
                let started = std::time::Instant::now();
                editor.frame(vec![event]);
                times.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
            (at(0.5), at(0.95), at(1.0))
        };
        let off = zen_workbench::visual_aids::VisualAidSettings {
            hard_wrap_guides: false,
            indent_guides: false,
            whitespaces: false,
            ..Default::default()
        };
        let on = zen_workbench::visual_aids::VisualAidSettings {
            whitespaces: true,
            whitespace_kinds: zen_workbench::visual_aids::WhitespaceKinds { selection: true, ..Default::default() },
            ..Default::default()
        };
        let before = measure(&mut editor, off);
        let after = measure(&mut editor, on);
        let again = measure(&mut editor, off);
        #[allow(clippy::print_stdout, reason = "a benchmark run by hand")]
        {
            println!("visual aids off: p50 {:.2} p95 {:.2} max {:.2} ms", before.0, before.1, before.2);
            println!("visual aids on:  p50 {:.2} p95 {:.2} max {:.2} ms", after.0, after.1, after.2);
            println!("visual aids off: p50 {:.2} p95 {:.2} max {:.2} ms", again.0, again.1, again.2);
        }
        assert_eq!(editor.text(), text, "every x was deleted again");
    }

    #[test]
    fn usages_at_the_caret_a_selection_and_pinned_usages_are_painted() {
        let mut editor = Harness::new("ab := ab + abc\nc := 1\nab++\n");
        let palette = IdePalette::zen_dark();
        let bands = |editor: &mut Harness| {
            editor
                .painted()
                .iter()
                .filter(|shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == palette.symbol_highlight))
                .count()
        };
        editor.set_caret(1);
        assert_eq!(bands(&mut editor), 3, "`ab` three times, not `abc`");
        editor
            .registry
            .dispatch(
                &mut editor.session,
                "editor.selection.set",
                json!({ "selections": [{ "anchor": 0, "head": 2 }] }),
            )
            .unwrap();
        assert_eq!(bands(&mut editor), 4);
        editor.session.settings_mut().set(zen_workbench::language::highlights::AT_CARET, false).unwrap();
        assert_eq!(bands(&mut editor), 4);
        editor.set_caret(1);
        assert_eq!(bands(&mut editor), 0, "nothing at the caret while the setting is off");
        let shift_cmd = Modifiers { shift: true, command: true, mac_cmd: true, ..Modifiers::NONE };
        editor.key(Key::F7, shift_cmd);
        editor.set_caret(17);
        assert_eq!(bands(&mut editor), 3, "pinned");
        editor.key(Key::ArrowDown, Modifiers { ctrl: true, alt: true, ..Modifiers::NONE });
        assert_eq!(editor.caret(), 22, "⌃⌥↓ to the next pinned usage");
        editor.key(Key::ArrowUp, Modifiers { ctrl: true, alt: true, ..Modifiers::NONE });
        assert_eq!(editor.caret(), 6, "⌃⌥↑ back");
        editor.key(Key::Escape, Modifiers::NONE);
        assert_eq!(bands(&mut editor), 0, "Escape unpinned them");
    }

    #[test]
    fn written_usages_have_a_colour_of_their_own() {
        let path = std::path::Path::new("/p/main.go");
        let answer = json!({
            "path": path,
            "pending": false,
            "ranges": [{ "start": 0, "end": 2, "kind": "write" }, { "start": 6, "end": 8, "kind": "read" }],
        });
        let (pending, bands) = usage_bands(Some(answer.clone()), path);
        assert!(!pending);
        assert_eq!(bands, [UsageBand { start: 0, end: 2, write: true }, UsageBand { start: 6, end: 8, write: false }]);
        assert_eq!(usage_bands(Some(answer), std::path::Path::new("/p/other.go")), (false, Vec::new()));
        assert_ne!(IdePalette::zen_dark().symbol_highlight_write, IdePalette::zen_dark().symbol_highlight);
    }

    #[test]
    fn the_bracket_at_the_caret_and_its_match_are_painted() {
        let mut editor = Harness::new("f(a[1])\ng(\n");
        let palette = IdePalette::zen_dark();
        let bands = |editor: &mut Harness, fill: Color32| {
            editor
                .painted()
                .iter()
                .filter(|shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == fill))
                .count()
        };
        editor.set_caret(1);
        assert_eq!(bands(&mut editor, palette.bracket_matched_background), 2, "`(` and `)`");
        assert_eq!(
            editor.view.brackets.as_ref().and_then(|(_, found)| *found).and_then(|found| found.matching),
            Some(6)
        );
        editor.set_caret(10);
        assert_eq!(bands(&mut editor, palette.bracket_matched_background), 0);
        assert_eq!(bands(&mut editor, palette.bracket_unmatched_background), 1, "`(` has no match");
        editor.set_caret(0);
        assert_eq!(bands(&mut editor, palette.bracket_unmatched_background), 0);
        assert_eq!(bands(&mut editor, palette.bracket_matched_background), 0, "no bracket next to the caret");
        editor.key(Key::M, Modifiers { ctrl: true, shift: true, ..Modifiers::NONE });
        assert_eq!(editor.caret(), 0, "⌃⇧M does nothing away from a bracket");
        editor.set_caret(3);
        editor.key(Key::M, Modifiers { ctrl: true, shift: true, ..Modifiers::NONE });
        assert_eq!(editor.caret(), 6, "⌃⇧M goes past the matching bracket");
        editor.key(Key::M, Modifiers { ctrl: true, shift: true, ..Modifiers::NONE });
        assert_eq!(editor.caret(), 3, "and back");
    }

    #[test]
    fn line_actions_from_the_keyboard() {
        let mut editor = Harness::new("package main\n\nfunc main() {}\n");
        editor.key(Key::D, Modifiers::COMMAND);
        assert_eq!(editor.text(), "package main\npackage main\n\nfunc main() {}\n", "⌘D");
        editor.key(Key::Backspace, Modifiers::COMMAND);
        assert_eq!(editor.text(), "package main\n\nfunc main() {}\n", "⌘⌫");
        editor.key(Key::ArrowUp, Modifiers::NONE);
        editor.key(Key::Slash, Modifiers::COMMAND);
        assert_eq!(editor.text(), "// package main\n\nfunc main() {}\n", "⌘/");
        editor.key(Key::Slash, Modifiers::COMMAND);
        editor.key(Key::ArrowDown, Modifiers { alt: true, shift: true, ..Modifiers::NONE });
        assert_eq!(editor.text(), "\npackage main\nfunc main() {}\n", "⌥⇧↓ moves the line");
        editor.key(Key::ArrowDown, Modifiers::NONE);
        editor.key(Key::ArrowRight, Modifiers::COMMAND);
        editor.key(Key::ArrowLeft, Modifiers::NONE);
        editor.key(Key::Enter, Modifiers::NONE);
        editor.frame(vec![Event::Text("f".to_owned()), Event::Text("(".to_owned())]);
        assert_eq!(editor.text(), "\npackage main\nfunc main() {\n\tf()\n}\n");
        editor.key(Key::Backspace, Modifiers::NONE);
        assert_eq!(editor.text(), "\npackage main\nfunc main() {\n\tf\n}\n", "⌫ deletes the empty pair");
    }

    #[test]
    fn option_slash_expands_the_word_without_typing_its_character() {
        let mut editor = Harness::new("count := 1\ncounter := 2\nco");
        editor.set_caret(26);
        let option_slash = |shift: bool, text: &str| {
            let modifiers = Modifiers { alt: true, shift, ..Modifiers::NONE };
            vec![
                Event::Key { key: Key::Slash, physical_key: Some(Key::Slash), pressed: true, repeat: false, modifiers },
                Event::Text(text.to_owned()),
            ]
        };
        editor.frame(option_slash(false, "÷"));
        assert_eq!(editor.text(), "count := 1\ncounter := 2\ncounter");
        editor.frame(option_slash(false, "÷"));
        assert_eq!(editor.text(), "count := 1\ncounter := 2\ncount");
        editor.frame(option_slash(true, "¿"));
        assert_eq!(editor.text(), "count := 1\ncounter := 2\ncounter", "⌥⇧/ steps back");
        editor.frame(vec![Event::Text("÷".to_owned())]);
        assert_eq!(editor.text(), "count := 1\ncounter := 2\ncounter÷", "⌥ still types characters of its own");
    }

    fn option_key(key: Key, physical: Key, shift: bool) -> Event {
        let modifiers = Modifiers { alt: true, shift, ..Modifiers::NONE };
        Event::Key { key, physical_key: Some(physical), pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn german_option_characters_are_typed_beside_the_expansion() {
        let mut editor = Harness::new("count := 1\nco");
        editor.set_caret(13);
        editor.frame(vec![
            option_key(Key::L, Key::L, false),
            Event::Text("@".to_owned()),
            option_key(Key::Pipe, Key::Num7, false),
            Event::Text("|".to_owned()),
        ]);
        assert_eq!(editor.text(), "count := 1\nco@|", "⌥L and ⌥7 have no binding: their characters are typed");
        editor.set_caret(13);
        editor.frame(vec![
            option_key(Key::Slash, Key::Slash, false),
            Event::Text("–".to_owned()),
            option_key(Key::L, Key::L, false),
            Event::Text("@".to_owned()),
        ]);
        assert_eq!(editor.text(), "count := 1\ncount@@|", "only the bound key's own character is dropped");
    }

    #[test]
    fn russian_option_characters_are_typed_and_cyrillic_words_expand() {
        let mut editor = Harness::new("счётчик := 1\nсч");
        editor.set_caret(15);
        editor.frame(vec![option_key(Key::Slash, Key::Slash, false), Event::Text("÷".to_owned())]);
        assert_eq!(editor.text(), "счётчик := 1\nсчётчик");
        editor.frame(vec![option_key(Key::Quote, Key::Quote, false), Event::Text("«".to_owned())]);
        editor.frame(vec![option_key(Key::Backslash, Key::Backslash, true), Event::Text("»".to_owned())]);
        assert_eq!(editor.text(), "счётчик := 1\nсчётчик«»", "unbound ⌥ keys type their characters");
    }

    #[test]
    fn a_bound_option_key_without_a_character_drops_nothing() {
        let mut editor = Harness::new("one two");
        editor.set_caret(7);
        editor.frame(vec![option_key(Key::ArrowLeft, Key::ArrowLeft, false), Event::Text("§".to_owned())]);
        assert_eq!(editor.text(), "one §two", "⌥← moves a word left; the next key's character is typed");
    }

    #[test]
    fn copy_takes_the_selection_or_the_whole_line() {
        let mut editor = Harness::new("one\ntwo\n");
        let document =
            |editor: &Harness| editor.session.state::<Workbench>().unwrap().active_document().unwrap().clone();
        let (copied, lines) = copy_text(&document(&editor));
        assert_eq!(copied, "one\n", "no selection: the caret's line");
        assert!(lines.is_some());
        editor.key(Key::ArrowRight, Modifiers::SHIFT);
        let (copied, lines) = copy_text(&document(&editor));
        assert_eq!(copied, "o");
        assert!(lines.is_none());
    }

    #[test]
    fn tabs_expand_to_the_next_stop() {
        let (display, columns) = display_line("\tif x {\t}", 4);
        assert_eq!(display, "    if x {  }");
        assert_eq!(columns, [0, 4, 5, 6, 7, 8, 9, 10, 12, 13]);
        assert_eq!(display_line("\tx\t", 2).0, "  x ");
        assert_eq!(display_line("\tx\t", 8).0, "        x       ");
    }

    impl Harness {
        fn with_debugger(contents: &str) -> (Self, PathBuf, zen_dap::fake::FakeAdapter) {
            let (mut editor, _) = Harness::with_runs(contents, &[]);
            let path = editor.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
            let adapter = zen_dap::fake::FakeAdapter::new();
            let launcher = zen_workbench::debug::FakeLauncher::new(adapter.clone());
            editor.session.state_mut::<zen_workbench::debug::Debugger>().set_launcher(Arc::new(launcher));
            editor.frame(Vec::new());
            (editor, path, adapter)
        }

        fn number(&self, line: usize) -> egui::Pos2 {
            pos2(12.0, TOP + self.view.line_height * (line as f32 + 0.5))
        }

        fn breakpoint_lines(&mut self) -> Vec<u64> {
            let listed = self.registry.dispatch(&mut self.session, "debug.breakpoint.list", json!({})).unwrap();
            listed["breakpoints"]
                .as_array()
                .unwrap()
                .iter()
                .map(|breakpoint| breakpoint["line"].as_u64().unwrap())
                .collect()
        }

        fn selection(&self) -> (usize, usize) {
            let document = self.session.state::<Workbench>().unwrap().active_document().unwrap();
            let primary = document.selections().primary();
            (primary.anchor, primary.head)
        }

        fn start_debugging(&mut self) -> u64 {
            let configuration = zen_run::fake::FakeProvider::configuration("app");
            let started = self
                .registry
                .dispatch(&mut self.session, "debug.session.start", json!({ "configuration": configuration }))
                .unwrap();
            started["sessionId"].as_u64().unwrap()
        }

        fn wait_for(&mut self, state: &str) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let answer = self.registry.dispatch(&mut self.session, "debug.session.get", json!({})).unwrap();
                if answer["state"] == state {
                    return;
                }
                assert!(std::time::Instant::now() < deadline, "the session is not {state}: {answer}");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    #[test]
    fn without_line_numbers_the_gutter_narrows_and_clicks_still_hit_the_text_and_the_breakpoints() {
        let (mut editor, _, _) = Harness::with_debugger("first\nsecond\nthird\n");
        let char_width = editor.char_width();
        let line_height = editor.view.line_height;
        let text_at =
            |gutter: f32, line: f32| pos2(gutter + TEXT_LEFT + char_width * 3.0, TOP + line_height * (line + 0.5));
        editor.click(text_at(gutter_width(3, char_width, false), 1.0));
        assert_eq!(editor.caret(), 6 + 3);
        editor
            .registry
            .dispatch(&mut editor.session, "editor.lineNumbers.toggle", json!({ "enabled": false }))
            .unwrap();
        editor.frame(Vec::new());
        editor.click(text_at(gutter_width(0, char_width, false), 2.0));
        assert_eq!(editor.caret(), 13 + 3, "the text starts where the narrower gutter ends");
        editor.click(editor.number(1));
        assert_eq!(editor.breakpoint_lines(), [1], "the breakpoints keep their column");
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_marks, [(1, GutterMark::Breakpoint(BreakpointState::Verified))]);
        editor.session.settings_mut().set(zen_workbench::line_numbers::LINE_NUMBERS_MODE, "relative").unwrap();
        editor.registry.dispatch(&mut editor.session, "editor.lineNumbers.toggle", json!({ "enabled": true })).unwrap();
        for line in [0.0, 2.0, 1.0] {
            editor.click(text_at(gutter_width(3, char_width, false), line));
            assert_eq!(editor.caret(), [0, 6, 13][line as usize] + 3, "relative numbers keep the gutter's width");
        }
    }

    #[test]
    fn a_click_on_a_line_number_toggles_its_breakpoint_and_shift_click_sets_a_logging_one() {
        let (mut editor, _, _) = Harness::with_debugger("first\nsecond\nthird\n");
        let before = editor.selection();
        editor.click(editor.number(1));
        assert_eq!(editor.breakpoint_lines(), [1]);
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_marks, [(1, GutterMark::Breakpoint(BreakpointState::Verified))]);
        assert_eq!(editor.selection(), before, "toggling a breakpoint does not select the line");
        editor.click(editor.number(1));
        editor.frame(vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        assert!(editor.view.drawn_marks.is_empty(), "a second click removes it");
        assert!(editor.breakpoint_lines().is_empty());

        editor.click_with(editor.number(2), Modifiers::SHIFT);
        assert_eq!(editor.breakpoint_lines(), [2], "⇧-click sets a breakpoint");
        assert_eq!(editor.selection(), before, "⇧-click no longer selects the line");
        let listed = editor.registry.dispatch(&mut editor.session, "debug.breakpoint.list", json!({})).unwrap();
        let logging = &listed["breakpoints"][0];
        assert_eq!((logging["suspend"].clone(), logging["logHit"].clone()), (json!("none"), json!(true)));
        editor.view.breakpoints = None;
        editor.frame(vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        assert_eq!(editor.view.drawn_marks, [(2, GutterMark::Breakpoint(BreakpointState::Logging))]);
    }

    #[test]
    fn option_click_disables_a_drag_moves_and_command_drag_copies_a_breakpoint() {
        let (mut editor, path, _) = Harness::with_debugger("a\nb\nc\nd\ne\n");
        editor.click(editor.number(1));
        editor.click_with(editor.number(1), Modifiers::ALT);
        let listed = editor.registry.dispatch(&mut editor.session, "debug.breakpoint.list", json!({})).unwrap();
        assert_eq!(listed["breakpoints"][0]["enabled"], false, "⌥-click disables it and keeps it");
        editor.click_with(editor.number(1), Modifiers::ALT);
        editor.drag(editor.number(1), editor.number(3), Modifiers::NONE);
        assert_eq!(editor.breakpoint_lines(), [3], "a drag moves it");
        editor.drag(editor.number(3), editor.number(0), Modifiers::COMMAND);
        assert_eq!(editor.breakpoint_lines(), [0, 3], "a ⌘-drag copies it");
        editor.drag(editor.number(0), pos2(500.0, 300.0), Modifiers::NONE);
        assert_eq!(editor.breakpoint_lines(), [0, 3], "dragging it to the editor keeps it by default");
        editor.session.settings_mut().set(zen_workbench::debug::REMOVE_BY_DRAG, true).unwrap();
        editor.click(editor.number(3));
        assert_eq!(editor.breakpoint_lines(), [0, 3], "a click does not remove it then");
        editor.drag(editor.number(0), pos2(500.0, 300.0), Modifiers::NONE);
        assert_eq!(editor.breakpoint_lines(), [3], "dragging it to the editor removes it");
        let at = editor.number(3);
        let middle = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Middle,
            pressed,
            modifiers: Modifiers::NONE,
        };
        editor.frame(vec![Event::PointerMoved(at), middle(true)]);
        editor.frame(vec![middle(false)]);
        assert!(editor.breakpoint_lines().is_empty(), "so does a middle click");
        let _ = path;
    }

    #[test]
    fn a_right_click_on_a_breakpoint_asks_for_its_properties_and_the_menu_has_the_other_breakpoints() {
        let (mut editor, path, _) = Harness::with_debugger("a\nb\nc\n");
        editor.click(editor.number(1));
        editor.frame(Vec::new());
        let at = editor.number(1);
        let secondary = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        editor.frame(vec![Event::PointerMoved(at), secondary(true)]);
        editor.frame(vec![secondary(false)]);
        assert_eq!(
            editor.view.take_gutter_actions(),
            [(super::super::breakpoints_dialog::PROPERTIES.to_owned(), json!({ "path": path, "line": 1 }))]
        );
        assert!(editor.view.take_breakpoint_anchor().is_some(), "the popup opens at the breakpoint");
    }

    #[test]
    fn conditional_and_muted_breakpoints_have_their_own_marks() {
        let (mut editor, path, _) = Harness::with_debugger("a\nb\nc\n");
        let set = |editor: &mut Harness, params: Value| {
            editor.registry.dispatch(&mut editor.session, "debug.breakpoint.set", params).unwrap();
        };
        set(&mut editor, json!({ "path": path, "line": 0, "condition": "x > 1" }));
        set(&mut editor, json!({ "path": path, "line": 1, "enabled": false }));
        editor.view.breakpoints = None;
        editor.frame(vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        assert_eq!(
            editor.view.drawn_marks,
            [
                (0, GutterMark::Breakpoint(BreakpointState::Conditional)),
                (1, GutterMark::Breakpoint(BreakpointState::Disabled))
            ]
        );
        editor.registry.dispatch(&mut editor.session, "debug.breakpoints.mute", json!({})).unwrap();
        editor.view.breakpoints = None;
        editor.frame(vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        assert_eq!(
            editor.view.drawn_marks,
            [
                (0, GutterMark::Breakpoint(BreakpointState::Muted)),
                (1, GutterMark::Breakpoint(BreakpointState::MutedDisabled))
            ]
        );
    }

    #[test]
    fn bookmarks_show_in_the_gutter_follow_edits_and_alt_click_renames() {
        let mut editor = Harness::new("first\nsecond\nthird\n");
        let path = editor.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        let run = |editor: &mut Harness, command: &str, params: Value| {
            editor.registry.dispatch(&mut editor.session, command, params).unwrap()
        };
        run(&mut editor, "navigate.bookmark.toggle", json!({ "line": 1 }));
        run(&mut editor, "navigate.bookmark.setMnemonic", json!({ "line": 3, "mnemonic": "7", "description": "here" }));
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_bookmarks, [(0, None), (2, Some('7'))]);
        let marks = editor.view.bookmarks.as_ref().map(|shown| shown.marks.clone()).unwrap();
        assert_eq!(marks[1].hint(), "here", "the description shows on hover");

        let at = editor.run_icon_at(2, 3);
        let before = editor.selection();
        let press = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::ALT,
        };
        editor.frame_with(vec![Event::PointerMoved(at), press(true)], Modifiers::ALT);
        editor.frame_with(vec![press(false)], Modifiers::ALT);
        assert_eq!(
            editor.view.take_gutter_actions(),
            [("view.bookmark.rename".to_owned(), json!({ "path": path, "line": 3 }))]
        );
        assert_eq!(editor.selection(), before, "a click on the icon does not select the line");

        let document = editor.session.state_mut::<Workbench>().active_document_mut().unwrap();
        assert!(document.replace_ranges(vec![(0..0, "zero\n".to_owned())]));
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_bookmarks, [(1, None), (3, Some('7'))], "the icons move with their lines");
        run(&mut editor, "navigate.bookmark.toggle", json!({ "line": 2 }));
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_bookmarks, [(3, Some('7'))]);
    }

    #[test]
    fn the_gutter_shows_bookmarks_their_description_and_the_bookmarks_menu() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        let text = "package main\n\nimport \"fmt\"\n\nfunc a() {\n\tfmt.Println(\"a\")\n}\n\nfunc main() {\n\ta()\n}\n";
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        registry.dispatch(&mut session, "navigate.bookmark.toggle", json!({ "line": 3 })).unwrap();
        let mnemonic = json!({ "line": 5, "mnemonic": "1", "description": "the helper" });
        registry.dispatch(&mut session, "navigate.bookmark.setMnemonic", mnemonic).unwrap();
        registry
            .dispatch(&mut session, "navigate.bookmark.setMnemonic", json!({ "line": 9, "mnemonic": "K" }))
            .unwrap();
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(vec2(640.0, 300.0), 2.0);
        palette.apply(screen.context());
        let mut view = EditorView::default();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let save = |name: &str, image: &crate::offscreen::Frame| {
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(name), image).unwrap();
            }
        };
        screen.step(Vec::new(), |ui| {
            view.show(ui, &registry, &mut session, None, &palette);
        });
        let line_height = view.line_height;
        assert_eq!(view.drawn_bookmarks, [(2, None), (4, Some('1')), (8, Some('K'))]);
        let mut draw = |ui: &mut Ui| {
            ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
            view.show(ui, &registry, &mut session, None, &palette);
        };
        save("bookmarks-gutter.png", &screen.render(&mut draw));
        let char_width = screen.context().fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let numbers = gutter::NUMBERS_LEFT + 2.0 * char_width;
        let row = |line: usize| TOP + line as f32 * line_height + line_height / 2.0;
        let icon = run_icon_rect(numbers, row(4)).center();
        screen.step(vec![Event::PointerMoved(icon)], &mut draw);
        let hover = screen.render(&mut draw);
        let tree = screen.accessibility_tree(&mut draw);
        let labels: Vec<String> =
            tree.nodes.iter().filter_map(|(_, node)| node.label().or(node.value()).map(str::to_owned)).collect();
        assert!(labels.iter().any(|label| label == "the helper"), "the description on hover: {labels:?}");
        save("bookmarks-gutter-hover.png", &hover);

        let number = pos2(gutter::NUMBERS_LEFT + char_width, row(4));
        let secondary = |pressed| Event::PointerButton {
            pos: number,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        screen.step(vec![Event::PointerMoved(number), secondary(true)], &mut draw);
        screen.step(vec![secondary(false)], &mut draw);
        let menu = screen.render(&mut draw);
        let tree = screen.accessibility_tree(&mut draw);
        let name = t!("ide-gutter-bookmarks");
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(name.as_str()))
            .and_then(|(_, node)| node.bounds())
            .expect("the gutter menu has Bookmarks");
        save("bookmarks-gutter-menu.png", &menu);
        let at = pos2((bounds.x0 + bounds.x1) as f32 / 2.0, (bounds.y0 + bounds.y1) as f32 / 2.0);
        screen.step(vec![Event::PointerMoved(at)], &mut draw);
        let submenu = screen.render(&mut draw);
        let tree = screen.accessibility_tree(&mut draw);
        let top_of = |item: &str| {
            tree.nodes
                .iter()
                .find(|(_, node)| node.label() == Some(item))
                .and_then(|(_, node)| node.bounds())
                .map(|bounds| bounds.y0)
                .unwrap_or_else(|| panic!("{item} in the Bookmarks submenu"))
        };
        let items = [
            t!("ide-bookmark-another-list"),
            t!("keymap-view-bookmark-rename"),
            t!("keymap-navigate-bookmark-toggle"),
            t!("keymap-navigate-bookmark-removeMnemonic"),
            t!("ide-bookmark-toggle-mnemonic"),
        ];
        let tops: Vec<f64> = items.iter().map(|item| top_of(item)).collect();
        assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "in order: {tops:?}");
        save("bookmarks-gutter-menu-bookmarks.png", &submenu);
    }

    #[test]
    fn breakpoints_are_drawn_by_state_over_the_numbers_with_a_faint_dot_on_hover() {
        let (mut editor, path, adapter) = Harness::with_debugger("a\nb\nc\nd\ne\n");
        adapter.set_program(&[(path.as_path(), 1), (path.as_path(), 2)]);
        adapter.set_verified(Some(vec![1]));
        adapter.set_hold(true);
        for (line, enabled) in [(0, true), (1, true), (2, false)] {
            let params = json!({ "path": path, "line": line, "enabled": enabled });
            editor.registry.dispatch(&mut editor.session, "debug.breakpoint.set", params).unwrap();
        }
        let elsewhere = json!({ "path": "/elsewhere/other.go", "line": 3 });
        editor.registry.dispatch(&mut editor.session, "debug.breakpoint.set", elsewhere).unwrap();
        editor.start_debugging();
        editor.wait_for("running");
        editor.view.breakpoints = None;
        let hover = editor.number(3);
        editor.frame(vec![Event::PointerMoved(hover)]);
        assert_eq!(
            editor.view.drawn_marks,
            [
                (0, GutterMark::Breakpoint(BreakpointState::Verified)),
                (1, GutterMark::Breakpoint(BreakpointState::Invalid)),
                (2, GutterMark::Breakpoint(BreakpointState::Disabled)),
                (3, GutterMark::Hover),
            ]
        );
        let marks = editor.view.breakpoints.as_ref().map(|shown| shown.marks.clone()).unwrap_or_default();
        assert_eq!(marks[1].message.as_deref(), Some("no code here"), "the tooltip's message");
        assert_eq!(marks.len(), 3, "another file's breakpoint is not drawn here");
        let pane = pos2(600.0, 500.0);
        editor.frame(vec![Event::PointerMoved(pane)]);
        assert_eq!(editor.view.drawn_marks.len(), 3, "no faint dot without the pointer");
        let version = editor.view.breakpoints.as_ref().map(|shown| shown.version);
        let breakpoints = editor.session.state_mut::<zen_workbench::debug::Breakpoints>();
        for line in 0..3 {
            breakpoints.remove(&path, line);
        }
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_marks.len(), 3, "the list is kept for the document's version");
        editor.registry.dispatch(&mut editor.session, "editor.edit.insert", json!({ "text": "x" })).unwrap();
        editor.frame(Vec::new());
        assert_ne!(editor.view.breakpoints.as_ref().map(|shown| shown.version), version);
        assert!(editor.view.drawn_marks.is_empty(), "an edit reads the breakpoints again");
    }

    #[test]
    fn inline_values_and_the_run_to_cursor_inlay_follow_the_stopped_program() {
        let (mut editor, path, adapter) = Harness::with_debugger("a := 1\nb := a + 1\nc := b\nd := c\n");
        adapter.set_variables(vec![
            zen_dap::fake::FakeVariable::new("a", "1", "int"),
            zen_dap::fake::FakeVariable::new("b", "2", "int"),
        ]);
        adapter.set_program(&[(path.as_path(), 3), (path.as_path(), 4)]);
        let params = json!({ "path": path, "line": 2 });
        editor.registry.dispatch(&mut editor.session, "debug.breakpoint.set", params).unwrap();
        editor.start_debugging();
        editor.wait_for("stopped");
        let drawn = |editor: &mut Harness| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                editor.view.debugging = None;
                editor.view.execution = None;
                editor.frame(Vec::new());
                let drawn = editor.view.inline_values.drawn().to_vec();
                if !drawn.is_empty() || std::time::Instant::now() >= deadline {
                    return drawn;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        assert_eq!(
            drawn(&mut editor),
            [(0, "a = 1".to_owned(), false), (1, "b = 2, a = 1".to_owned(), false), (2, "b = 2".to_owned(), false)]
        );

        let line_height = editor.view.line_height;
        let over = |line: f32| pos2(400.0, TOP + line_height * (line + 0.5));
        editor.frame(vec![Event::PointerMoved(over(2.0))]);
        assert_eq!(editor.view.drawn_run_inlay, None, "not on the stopped line");
        editor.frame(vec![Event::PointerMoved(over(3.0))]);
        let (line, button) = editor.view.drawn_run_inlay.expect("the inlay on the hovered line");
        assert_eq!(line, 3);
        assert!(button.right() < 400.0 && button.height() <= line_height, "at the line start: {button:?}");
        adapter.set_variables(vec![
            zen_dap::fake::FakeVariable::new("a", "1", "int"),
            zen_dap::fake::FakeVariable::new("b", "5", "int"),
        ]);
        editor.frame(vec![Event::PointerMoved(button.center())]);
        editor.click(button.center());
        assert_eq!(adapter.wait_for("continue", 1, std::time::Duration::from_secs(10)).len(), 1);
        editor.wait_for("stopped");
        let stopped = editor.registry.dispatch(&mut editor.session, "debug.session.get", json!({})).unwrap();
        assert_eq!(stopped["frame"]["line"], 3, "the program ran to the inlay's line");
        editor.view.inline_values = super::super::inline_values::InlineValues::default();
        let shown = drawn(&mut editor);
        assert!(shown.contains(&(1, "b = 5, a = 1".to_owned(), true)), "a changed value is marked: {shown:?}");

        editor.session.settings_mut().set(zen_workbench::debug::INLINE_VALUES, false).unwrap();
        editor.session.settings_mut().set(zen_workbench::debug::RUN_TO_CURSOR_INLAY, false).unwrap();
        editor.view.debugging = None;
        editor.frame(vec![Event::PointerMoved(over(1.0))]);
        assert!(editor.view.inline_values.drawn().is_empty(), "Show values inline is off");
        assert_eq!(editor.view.drawn_run_inlay, None, "the inlay is off");
        editor.registry.dispatch(&mut editor.session, "debug.session.stop", json!({})).unwrap();
    }

    #[test]
    fn a_smart_step_target_is_framed_in_its_own_file_only() {
        let (mut editor, path, _) = Harness::with_debugger("a := twice(b)\nc\n");
        editor.view.set_step_target(Some((path.clone(), 5..10)));
        editor.frame(Vec::new());
        let call = editor.view.drawn_step_target.expect("the call is framed");
        assert!(call.top() >= TOP && call.bottom() <= TOP + editor.view.line_height, "on the first line: {call:?}");
        editor.view.set_step_target(Some((path.clone(), 5..6)));
        editor.frame(Vec::new());
        let one = editor.view.drawn_step_target.unwrap();
        assert!(
            (call.width() - one.width() * 5.0).abs() < 2.0 && (call.left() - one.left()).abs() < 0.5,
            "{call:?} {one:?}"
        );
        editor.view.set_step_target(Some((PathBuf::from("/elsewhere/main.go"), 5..10)));
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_step_target, None);
    }

    #[test]
    fn the_execution_point_is_painted_only_while_a_session_stops_in_this_file() {
        let (mut editor, path, adapter) = Harness::with_debugger("a\nb\nc\n");
        editor.frame(Vec::new());
        assert!(editor.view.execution.is_none(), "no session, no polling");
        assert_eq!(editor.view.drawn_execution, None);

        adapter.set_program(&[(path.as_path(), 2)]);
        let params = json!({ "path": path, "line": 1 });
        editor.registry.dispatch(&mut editor.session, "debug.breakpoint.set", params).unwrap();
        editor.start_debugging();
        editor.wait_for("stopped");
        editor.view.debugging = None;
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_execution, Some(1));

        adapter.set_hold(true);
        editor.registry.dispatch(&mut editor.session, "debug.execution.resume", json!({})).unwrap();
        editor.wait_for("running");
        editor.view.execution = None;
        editor.frame(Vec::new());
        assert_eq!(editor.view.drawn_execution, None, "a running program has no execution point");

        editor.registry.dispatch(&mut editor.session, "debug.session.stop", json!({})).unwrap();
        editor.wait_for("ended");
        editor.view.debugging = None;
        editor.frame(Vec::new());
        assert!(editor.view.execution.is_none(), "ended sessions are not polled");
        assert_eq!(
            stopped_at(&json!({ "state": "stopped", "frame": { "path": "/a.go", "line": 4 } })),
            Some(("/a.go".into(), 4))
        );
        assert_eq!(stopped_at(&json!({ "state": "ended" })), None);
    }

    #[test]
    fn while_stopped_the_expression_under_the_pointer_is_offered_and_option_click_evaluates_it() {
        let (mut editor, path, adapter) = Harness::with_debugger("a\nprint(order.items, 42)\nc\n");
        let char_width = editor.char_width();
        let gutter = gutter_width(4, char_width, false);
        let at = |editor: &Harness, column: f32| {
            pos2(gutter + TEXT_LEFT + char_width * (column + 0.5), TOP + editor.view.line_height * 1.5)
        };
        editor.frame(vec![Event::PointerMoved(at(&editor, 13.0))]);
        assert!(editor.view.hovered_value(&editor.ctx).is_none(), "nothing to evaluate without a stopped session");

        adapter.set_program(&[(path.as_path(), 2)]);
        let params = json!({ "path": path, "line": 1 });
        editor.registry.dispatch(&mut editor.session, "debug.breakpoint.set", params).unwrap();
        editor.start_debugging();
        editor.wait_for("stopped");
        editor.view.debugging = None;
        editor.frame(vec![Event::PointerMoved(at(&editor, 13.0))]);
        let (expression, anchor) = editor.view.hovered_value(&editor.ctx).expect("a name under the pointer");
        assert_eq!(expression, "order.items");
        assert!(anchor.contains(at(&editor, 13.0)));
        editor.frame(vec![Event::PointerMoved(at(&editor, 19.0))]);
        assert!(editor.view.hovered_value(&editor.ctx).is_none(), "a number is not evaluated");

        editor.click_with(at(&editor, 7.0), Modifiers::ALT);
        let actions = editor.view.take_gutter_actions();
        assert_eq!(actions, [(super::super::evaluate::QUICK_EVALUATE.to_owned(), json!({ "expression": "order" }))]);
        assert!(editor.view.take_value_anchor().is_some(), "the popup opens at the expression");

        let document = editor.session.state::<Workbench>().unwrap().active_document().unwrap();
        let line_start = document.snapshot().point_to_offset(zen_text::Point::new(1, 0)).unwrap();
        let select = json!({ "selections": [{ "anchor": line_start, "head": line_start + 5 }] });
        editor.registry.dispatch(&mut editor.session, "editor.selection.set", select).unwrap();
        assert_eq!(super::super::evaluate::at_caret(&editor.session).as_deref(), Some("print"), "the selection");
        let caret = json!({ "selections": [{ "head": line_start + 14 }] });
        editor.registry.dispatch(&mut editor.session, "editor.selection.set", caret).unwrap();
        assert_eq!(
            super::super::evaluate::at_caret(&editor.session).as_deref(),
            Some("order.items"),
            "the caret's name"
        );
    }

    #[test]
    fn debug_in_a_run_icons_menu_asks_the_window_to_debug_its_configuration() {
        let (mut editor, _) = Harness::with_runs(TESTS, &[(2, "TestA"), (4, "TestB")]);
        editor.click(editor.run_icon_at(4, 6));
        editor.frame(Vec::new());
        let items = editor.view.run_menu.as_ref().map(|menu| menu.items.clone()).unwrap_or_default();
        assert_eq!(items.len(), 3, "Run, Debug and Modify Run Configuration");
        editor.click(items[1].center());
        assert!(editor.view.run_menu.is_none());
        let configuration = serde_json::to_value(zen_run::fake::FakeProvider::configuration("TestB")).unwrap();
        assert_eq!(editor.view.take_debug_request(), Some(json!({ "configuration": configuration })));
        assert!(editor.runs().is_empty(), "nothing is run");
        assert!(editor.view.take_window_actions().is_empty(), "the window opens the Debug tool window");
    }

    #[test]
    fn blame_annotations_fade_with_age() {
        let palette = IdePalette::zen_dark();
        let (newest, tint) = annotation_colors(&palette, 0);
        assert_eq!(newest, palette.annotation_recent);
        assert!(tint.a() > 0, "the newest lines are tinted");
        let (oldest, tint) = annotation_colors(&palette, 100);
        assert_eq!((oldest, tint.a()), (palette.annotation_old, 0), "the oldest are plain and faint");
        let distance = |color: Color32| {
            let r = palette.annotation_recent;
            (i32::from(color.r()) - i32::from(r.r())).abs()
                + (i32::from(color.g()) - i32::from(r.g())).abs()
                + (i32::from(color.b()) - i32::from(r.b())).abs()
        };
        let steps: Vec<i32> = [0, 25, 50, 75, 100].map(|age| distance(annotation_colors(&palette, age).0)).to_vec();
        assert!(steps.windows(2).all(|pair| pair[0] < pair[1]), "{steps:?}");
    }

    #[test]
    fn the_gutter_grows_with_the_font() {
        let narrow = gutter_width(99, 7.8, false);
        assert!((narrow - (13.75 + 2.0 * 7.8 + 30.0 + 12.25 + 1.0)).abs() < 1e-4, "{narrow}");
        assert!(gutter_width(99, 12.0, false) > narrow);
        assert_eq!(gutter_width(99, 7.8, true), narrow + ANNOTATIONS);
    }

    #[test]
    fn a_wider_tab_size_widens_the_lines_and_redraws_them() {
        let mut editor = Harness::new("\tx\n");
        assert_eq!(editor.view.highlight.as_ref().unwrap().max_columns, 5);
        let mut styles = std::collections::BTreeMap::new();
        styles.insert("go".to_owned(), zen_workbench::code_style::CodeStyle { use_tabs: true, tab_size: 8, indent: 8 });
        zen_workbench::code_style::apply(editor.session.settings_mut(), &styles);
        editor.frame(Vec::new());
        assert_eq!(editor.view.highlight.as_ref().unwrap().max_columns, 9);
    }

    #[test]
    fn the_editor_font_sets_the_line_height() {
        let mut editor = Harness::new("x\n");
        assert_eq!(editor.view.line_height, 22.0);
        editor.view.font = super::super::settings::EditorFont { size: 16.0, line_spacing: 1.5, ..Default::default() };
        editor.frame(Vec::new());
        assert_eq!(editor.view.line_height, 34.0);
    }

    #[test]
    fn line_starts_follow_every_line_break() {
        let (starts, widths, widest) = line_starts("ab\r\ncd\ref\n\tg", 4);
        assert_eq!(starts, [0, 4, 7, 10, 12]);
        assert_eq!(widths, [2, 2, 2, 5]);
        assert_eq!(widest, 5);
        assert_eq!(line_starts("", 4), (vec![0, 0], vec![0], 0));
        assert_eq!(line_starts("a\n", 4), (vec![0, 2, 2], vec![1, 0], 1));
    }

    #[test]
    fn the_line_index_follows_random_edits() {
        let mut seed = 0x5851_f42d_4c95_7f2du64;
        let mut next = move |below: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % below.max(1) as u64) as usize
        };
        let pieces = ["\n", "\r", "\r\n", "\t", "abc", "", "ё", "\n\n", "wide line of text"];
        let mut text = String::from("one\r\ntwo\rthree\n\tfour");
        let (mut starts, mut widths, mut widest) = line_starts(&text, 4);
        for _ in 0..3_000 {
            let mut start = next(text.len() + 1);
            while !text.is_char_boundary(start) {
                start -= 1;
            }
            let mut end = (start + next(5)).min(text.len());
            while !text.is_char_boundary(end) {
                end += 1;
            }
            let insert = pieces[next(pieces.len())];
            text.replace_range(start..end, insert);
            let change = TextChange { start, old_end: end, new_end: start + insert.len() };
            widest = follow_lines(&mut starts, &mut widths, widest, &text, change, 4);
            assert_eq!((starts.clone(), widths.clone(), widest), line_starts(&text, 4), "{text:?}");
        }
    }

    #[test]
    fn clicks_select_words_and_lines() {
        let text = zen_text::Text::from("let foo_bar = 1\nnext");
        assert_eq!(unit_at(&text, 6, 2, false), (4, 11));
        assert_eq!(unit_at(&text, 12, 2, false), (12, 12), "a blank is not a word");
        assert_eq!(unit_at(&text, 6, 3, false), (0, 16));
        assert_eq!(unit_at(&text, 18, 3, false), (16, 20));
        assert_eq!(unit_at(&text, 9, 2, true), (8, 11), "the camel hump");
        assert_eq!(unit_at(&text, 12, 2, true), (12, 12));
    }

    struct GoModFindings;

    impl zen_project::inspect::Inspector for GoModFindings {
        fn id(&self) -> &str {
            "test"
        }

        fn handles(&self, file_name: &str) -> bool {
            file_name == "go.mod"
        }

        fn inspect(&self, context: &zen_project::inspect::InspectContext<'_>) -> Vec<zen_project::inspect::Finding> {
            use zen_project::inspect::{Finding, FindingRow, FindingSeverity, FixKind, InspectionFix};
            let text = context.text;
            let fix = |id: &str, title: &str, kind: FixKind, label: Option<&str>| InspectionFix {
                id: id.to_owned(),
                title: title.to_owned(),
                kind,
                batch: Some("go.dependencies.update".to_owned()),
                label: label.map(str::to_owned),
                inline: false,
                args: json!({}),
            };
            let brotli = text.find("v1.0.0").unwrap();
            let lzw = text.find("github.com/hhrutter/lzw").unwrap_or_default();
            let crypto = text.find("golang.org/x/crypto").unwrap();
            let crypto_end = crypto + text[crypto..].find('\n').unwrap();
            let row = |label: &str, badge: &str, text: &str| FindingRow {
                label: label.to_owned(),
                url: Some(format!("https://example.com/{label}")),
                badge: Some(badge.to_owned()),
                text: text.to_owned(),
            };
            vec![
                Finding {
                    start: brotli,
                    end: brotli + 6,
                    severity: FindingSeverity::WeakWarning,
                    message: "Newer version is available: v1.2.6".to_owned(),
                    note: None,
                    rows: Vec::new(),
                    details: None,
                    fixes: vec![
                        fix("batch", "Update dependencies…", FixKind::Batch, None),
                        fix(
                            "brotli",
                            "Update to v1.2.6",
                            FixKind::Apply,
                            Some("github.com/andybalholm/brotli: v1.0.0 → v1.2.6"),
                        ),
                    ],
                    source: "go.mod".to_owned(),
                    code: "newerVersion".to_owned(),
                    deprecated: false,
                    unused: false,
                },
                Finding {
                    start: lzw,
                    end: lzw + "github.com/hhrutter/lzw".len(),
                    severity: FindingSeverity::Warning,
                    message: "Deprecated module github.com/hhrutter/lzw.".to_owned(),
                    note: Some("Deprecation notice: no longer maintained as a standalone module.".to_owned()),
                    rows: Vec::new(),
                    details: Some("## Deprecated module github.com/hhrutter/lzw.\n\nno longer maintained as a standalone module.\n".to_owned()),
                    fixes: vec![InspectionFix {
                        id: "go.deprecation:github.com/hhrutter/lzw".to_owned(),
                        title: "Show github.com/hhrutter/lzw deprecation notice".to_owned(),
                        kind: FixKind::Details,
                        batch: None,
                        label: None,
                        inline: false,
                        args: json!({}),
                    }],
                    source: "go.mod".to_owned(),
                    code: "deprecatedModule".to_owned(),
                    deprecated: true,
                    unused: false,
                },
                Finding {
                    start: crypto,
                    end: crypto_end,
                    severity: FindingSeverity::Warning,
                    message: "2 known vulnerabilities in this dependency:".to_owned(),
                    note: None,
                    rows: vec![
                        row(
                            "CVE-2024-45337",
                            "9.1",
                            "Misuse of ServerConfig.PublicKeyCallback may cause authorization bypass",
                        ),
                        row("CVE-2025-22869", "7.5", "Potential denial of service in golang.org/x/crypto"),
                    ],
                    details: Some(DETAILS.to_owned()),
                    fixes: vec![
                        fix("crypto", "Update to unaffected version v0.35.0", FixKind::Apply, None),
                        fix("crypto-tidy", "Update to v0.35.0 and run go mod tidy", FixKind::Apply, None),
                        fix("batch", "Update dependencies…", FixKind::Batch, None),
                    ],
                    source: "go.mod".to_owned(),
                    code: "vulnerability".to_owned(),
                    deprecated: false,
                    unused: false,
                },
            ]
            .into_iter()
            .filter(|finding| finding.code != "deprecatedModule" || text.contains("hhrutter"))
            .collect()
        }

        fn apply(
            &self,
            _context: &zen_project::inspect::InspectContext<'_>,
            _fixes: &[zen_project::inspect::InspectionFix],
        ) -> Result<Box<dyn zen_project::inspect::FixJob>, String> {
            Err("not in tests".to_owned())
        }
    }

    const DETAILS: &str = "## Vulnerabilities of golang.org/x/crypto v0.1.0\n\nThe oldest version without the vulnerabilities that have a fix: v0.35.0.\n\n### GO-2024-3321 Misuse of ServerConfig.PublicKeyCallback\n\n- Also known as CVE-2024-45337, GHSA-v778-237x-gjrc\n- Severity: 9.1 critical\n- Fixed in v0.31.0\n\nApplications and libraries which misuse the callback may be susceptible to an authorization bypass.\n\n[https://pkg.go.dev/vuln/GO-2024-3321](https://pkg.go.dev/vuln/GO-2024-3321)\n";

    struct GoModColours;

    impl Highlighter for GoModColours {
        fn handles(&self, _file_name: &str) -> bool {
            true
        }

        fn highlight(&self, text: &str) -> Vec<Span> {
            let mut spans = Vec::new();
            let mut at = 0;
            for line in text.split_inclusive('\n') {
                let mut offset = 0;
                for word in line.split_inclusive([' ', '\t', '\n']) {
                    let trimmed = word.trim_end();
                    let start = at + offset;
                    let kind = if trimmed.starts_with("//") {
                        Some(HighlightKind::Comment)
                    } else if ["module", "go", "require"].contains(&trimmed) && offset == 0 {
                        Some(HighlightKind::Keyword)
                    } else if trimmed.starts_with('v') || trimmed.starts_with("1.") {
                        Some(HighlightKind::Number)
                    } else {
                        None
                    };
                    if trimmed.starts_with("//") {
                        spans.push(Span { range: start..at + line.trim_end().len(), kind: HighlightKind::Comment });
                        break;
                    }
                    if let Some(kind) = kind.filter(|_| !trimmed.is_empty()) {
                        spans.push(Span { range: start..start + trimmed.len(), kind });
                    }
                    offset += word.len();
                }
                at += line.len();
            }
            spans
        }
    }

    #[test]
    fn problems_show_their_popup_their_fixes_and_their_dialogs() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let text = "module example.com/demo\n\ngo 1.25.0\n\nrequire (\n\tgithub.com/andybalholm/brotli v1.0.0 // indirect\n\tgolang.org/x/crypto v0.1.0 // indirect\n)\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("go.mod");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session
            .state_mut::<zen_workbench::inspections::Inspections>()
            .set_inspectors(vec![Arc::new(GoModFindings)], None);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(vec2(980.0, 380.0), 2.0);
        palette.apply(screen.context());
        let mut view = EditorView::default();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let save = |name: &str, image: &crate::offscreen::Frame| {
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(name), image).unwrap();
            }
        };
        let colours: Arc<dyn Highlighter> = Arc::new(GoModColours);
        let show = |ui: &mut Ui, view: &mut EditorView, session: &mut Session| {
            ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
            view.show(ui, &registry, session, Some(Arc::clone(&colours)), &palette);
        };
        let draw = |screen: &mut crate::offscreen::Offscreen,
                    view: &mut EditorView,
                    session: &mut Session,
                    events: Vec<Event>| { screen.frame(events, |ui| show(ui, view, session)) };
        let step = |screen: &mut crate::offscreen::Offscreen,
                    view: &mut EditorView,
                    session: &mut Session,
                    events: Vec<Event>| {
            screen.step(events, |ui| show(ui, view, session));
        };
        step(&mut screen, &mut view, &mut session, Vec::new());
        let char_width = screen.context().fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let gutter = gutter_width(2, char_width, false);
        let line_height = view.line_height;
        let version = pos2(gutter + TEXT_LEFT + char_width * 36.0, TOP + line_height * 5.5);
        step(&mut screen, &mut view, &mut session, vec![Event::PointerMoved(version)]);
        let mut image;
        for _ in 0..39 {
            step(&mut screen, &mut view, &mut session, Vec::new());
        }
        image = draw(&mut screen, &mut view, &mut session, Vec::new());
        assert_eq!(view.problem_popups_open(), ["problem"], "the popup opens after a moment");
        save("3-hover-newer-version.png", &image);

        let module = pos2(gutter + TEXT_LEFT + char_width * 8.0, TOP + line_height * 6.5);
        step(&mut screen, &mut view, &mut session, vec![Event::PointerMoved(module)]);
        for _ in 0..39 {
            step(&mut screen, &mut view, &mut session, Vec::new());
        }
        image = draw(&mut screen, &mut view, &mut session, Vec::new());
        assert_eq!(view.problem_popups_open(), ["problem"]);
        save("4-hover-vulnerabilities.png", &image);

        step(&mut screen, &mut view, &mut session, vec![Event::PointerMoved(pos2(960.0, 370.0))]);
        for _ in 0..40 {
            step(&mut screen, &mut view, &mut session, Vec::new());
        }
        assert!(view.problem_popups_open().is_empty());
        registry.dispatch(&mut session, "editor.caret.goTo", json!({ "line": 7, "column": 5 })).unwrap();
        let ctx = screen.context().clone();
        view.context_actions(&ctx, &registry, &mut session, false);
        for _ in 0..2 {
            step(&mut screen, &mut view, &mut session, Vec::new());
        }
        image = draw(&mut screen, &mut view, &mut session, Vec::new());
        assert_eq!(view.problem_popups_open(), ["contextActions"]);
        save("5-context-actions.png", &image);

        let key =
            |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        step(&mut screen, &mut view, &mut session, vec![key(Key::ArrowDown)]);
        step(&mut screen, &mut view, &mut session, vec![key(Key::ArrowDown)]);
        step(&mut screen, &mut view, &mut session, vec![key(Key::Enter)]);
        for _ in 0..9 {
            step(&mut screen, &mut view, &mut session, Vec::new());
        }
        image = draw(&mut screen, &mut view, &mut session, Vec::new());
        assert_eq!(view.problem_popups_open(), ["problemBatch"]);
        save("6-update-dependencies.png", &image);
        step(&mut screen, &mut view, &mut session, vec![key(Key::Escape)]);
        for _ in 0..3 {
            step(&mut screen, &mut view, &mut session, Vec::new());
        }
        assert!(view.problem_popups_open().is_empty(), "{:?}", view.problem_popups_open());

        view.context_actions(&ctx, &registry, &mut session, true);
        assert!(view.problem_popups_open().is_empty());
        assert!(view.notice.as_deref().is_some_and(|notice| notice.contains("not in tests")), "{:?}", view.notice);

        view.problem_popups.show_details_of("2 known vulnerabilities in this dependency", DETAILS);
        for _ in 0..9 {
            step(&mut screen, &mut view, &mut session, Vec::new());
        }
        image = draw(&mut screen, &mut view, &mut session, Vec::new());
        assert_eq!(view.problem_popups_open(), ["problemDetails"]);
        save("7-vulnerability-details.png", &image);
    }

    struct UnusedRequirement {
        unused: bool,
    }

    impl zen_project::inspect::Inspector for UnusedRequirement {
        fn id(&self) -> &str {
            "test"
        }

        fn handles(&self, file_name: &str) -> bool {
            file_name == "go.mod"
        }

        fn inspect(&self, context: &zen_project::inspect::InspectContext<'_>) -> Vec<zen_project::inspect::Finding> {
            let text = context.text;
            let start = text.find("go.yaml.in").unwrap();
            vec![zen_project::inspect::Finding {
                start,
                end: start + text[start..].find('\n').unwrap(),
                severity: zen_project::inspect::FindingSeverity::WeakWarning,
                message: "Unused indirect requirement: go mod tidy removes it".to_owned(),
                note: None,
                rows: Vec::new(),
                details: None,
                fixes: Vec::new(),
                source: "go.mod".to_owned(),
                code: "unusedRequirement".to_owned(),
                deprecated: false,
                unused: self.unused,
            }]
        }

        fn apply(
            &self,
            _context: &zen_project::inspect::InspectContext<'_>,
            _fixes: &[zen_project::inspect::InspectionFix],
        ) -> Result<Box<dyn zen_project::inspect::FixJob>, String> {
            Err("not in tests".to_owned())
        }
    }

    fn painted_unused_requirement(unused: bool, palette: &IdePalette) -> Vec<egui::Shape> {
        let text = "module example.com/demo\n\ngo 1.25.0\n\nrequire (\n\tgithub.com/andybalholm/brotli v1.0.0 // indirect\n\tgo.yaml.in/yaml/v2 v2.4.4 // indirect\n)\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("go.mod");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session
            .state_mut::<zen_workbench::inspections::Inspections>()
            .set_inspectors(vec![Arc::new(UnusedRequirement { unused })], None);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let listed = registry.dispatch(&mut session, "problems.file.list", json!({ "path": file })).unwrap();
        assert_eq!(listed[0]["code"], "unusedRequirement", "the Problems list still has it: {listed}");
        assert_eq!(listed[0]["unused"] == true, unused, "{listed}");
        let colours: Arc<dyn Highlighter> = Arc::new(GoModColours);
        let ctx = egui::Context::default();
        let mut view = EditorView::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, 400.0))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                view.show(ui, &registry, &mut session, Some(Arc::clone(&colours)), palette);
            });
            output.textures_delta.clear();
            shapes = output.shapes.into_iter().map(|clipped| clipped.shape).collect();
        }
        shapes
    }

    fn painted_colours(shapes: &[egui::Shape], needle: &str) -> Vec<(char, Color32)> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .filter_map(|text| {
                let job = &text.galley.job;
                let at = job.text.find(needle)?;
                Some(
                    job.sections
                        .iter()
                        .filter(|section| section.byte_range.start.0 >= at)
                        .flat_map(|section| {
                            let piece = &job.text[section.byte_range.start.0..section.byte_range.end.0];
                            piece.chars().map(|c| (c, section.format.color)).collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .next()
            .unwrap_or_else(|| panic!("{needle} is painted"))
    }

    fn wavy_lines(shapes: &[egui::Shape], color: Color32) -> usize {
        shapes
            .iter()
            .filter(|shape| matches!(shape, egui::Shape::Path(path) if path.stroke.color == egui::epaint::ColorMode::Solid(color) && path.points.len() > 2))
            .count()
    }

    #[test]
    fn unused_findings_are_greyed_out_and_not_underlined() {
        for palette in [IdePalette::zen_dark(), IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light())] {
            assert_ne!(palette.unused, palette.code);
            let weak = palette.problem("weakWarning");
            let shapes = painted_unused_requirement(true, &palette);
            let colours = painted_colours(&shapes, "go.yaml.in");
            assert_eq!(
                colours.iter().map(|(c, _)| c).collect::<String>().trim_end(),
                "go.yaml.in/yaml/v2 v2.4.4 // indirect"
            );
            assert!(colours.iter().all(|(_, color)| *color == palette.unused), "{colours:?}");
            assert_eq!(wavy_lines(&shapes, weak), 0, "not underlined");
            let other = painted_colours(&shapes, "v1.0.0");
            assert!(other.iter().all(|(_, color)| *color != palette.unused), "{other:?}");

            let shapes = painted_unused_requirement(false, &palette);
            let colours = painted_colours(&shapes, "go.yaml.in");
            assert!(colours.iter().all(|(_, color)| *color != palette.unused), "{colours:?}");
            assert_eq!(wavy_lines(&shapes, weak), 1, "a weak warning that is not unused is underlined");
        }
    }

    #[test]
    fn unused_ranges_cut_the_semantic_spans() {
        let spans = vec![(0, 4, SemanticKind::Package), (6, 12, SemanticKind::Call), (20, 22, SemanticKind::Type)];
        assert_eq!(with_unused(spans.clone(), []), spans);
        assert_eq!(
            with_unused(spans, [(8, 10), (9, 14), (3, 3)]),
            [
                (0, 4, SemanticKind::Package),
                (6, 8, SemanticKind::Call),
                (8, 14, SemanticKind::Unused),
                (20, 22, SemanticKind::Type),
            ]
        );
        let shown = |severity: &str, deprecated: bool, unused: bool| {
            ShownProblem::from_json(&json!({
                "start": 0, "end": 1, "line": 1, "severity": severity, "message": "",
                "deprecated": deprecated, "unused": unused,
            }))
            .unwrap()
            .mark()
        };
        assert_eq!(shown("weakWarning", false, true), ProblemMark::None);
        assert_eq!(shown("hint", false, true), ProblemMark::None);
        assert_eq!(shown("warning", false, true), ProblemMark::None);
        assert_eq!(shown("error", false, true), ProblemMark::Wavy, "an unused import does not compile");
        assert_eq!(shown("warning", true, true), ProblemMark::StrikeOut);
        assert_eq!(shown("weakWarning", false, false), ProblemMark::Wavy);
    }

    #[test]
    fn problem_popups_wait_for_the_delay_every_time_and_only_over_the_marks() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let text = "module example.com/demo\n\ngo 1.25.0\n\nrequire (\n\tgithub.com/andybalholm/brotli v1.0.0 // indirect\n\tgolang.org/x/crypto v0.1.0 // indirect\n\tgithub.com/hhrutter/lzw v1.0.1 // indirect\n)\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("go.mod");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session
            .state_mut::<zen_workbench::inspections::Inspections>()
            .set_inspectors(vec![Arc::new(GoModFindings)], None);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(vec2(980.0, 380.0), 2.0);
        palette.apply(screen.context());
        let mut view = EditorView::default();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let colours: Arc<dyn Highlighter> = Arc::new(GoModColours);
        let show = |ui: &mut Ui, view: &mut EditorView, session: &mut Session| {
            ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
            view.show(ui, &registry, session, Some(Arc::clone(&colours)), &palette);
        };
        let step = |screen: &mut crate::offscreen::Offscreen,
                    view: &mut EditorView,
                    session: &mut Session,
                    events: Vec<Event>| {
            screen.step(events, |ui| show(ui, view, session));
        };
        let delay = screen.context().global_style().interaction.tooltip_delay;
        let frames = (delay * 60.0).ceil() as usize;
        let rest =
            |screen: &mut crate::offscreen::Offscreen, view: &mut EditorView, session: &mut Session, count: usize| {
                for _ in 0..count {
                    step(screen, view, session, Vec::new());
                }
            };
        step(&mut screen, &mut view, &mut session, Vec::new());
        let char_width = screen.context().fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let gutter = gutter_width(2, char_width, false);
        let line_height = view.line_height;
        let at =
            |column: f32, line: f32| pos2(gutter + TEXT_LEFT + char_width * column, TOP + line_height * (line - 0.5));
        let moved = |pos| vec![Event::PointerMoved(pos)];

        step(&mut screen, &mut view, &mut session, moved(at(60.0, 6.0)));
        rest(&mut screen, &mut view, &mut session, frames + 10);
        assert!(view.problem_popups_open().is_empty(), "empty space at the end of the line");
        step(&mut screen, &mut view, &mut session, moved(at(44.0, 6.0)));
        rest(&mut screen, &mut view, &mut session, frames + 10);
        assert!(view.problem_popups_open().is_empty(), "the comment after the version");
        step(&mut screen, &mut view, &mut session, moved(at(6.0, 10.0)));
        rest(&mut screen, &mut view, &mut session, frames + 10);
        assert!(view.problem_popups_open().is_empty(), "an empty line");

        step(&mut screen, &mut view, &mut session, moved(at(36.0, 6.0)));
        rest(&mut screen, &mut view, &mut session, frames / 2);
        assert!(view.problem_popups_open().is_empty(), "not before the delay");
        rest(&mut screen, &mut view, &mut session, frames);
        assert_eq!(view.problem_popups_open(), ["problem"], "after the delay");

        step(&mut screen, &mut view, &mut session, moved(at(8.0, 8.0)));
        rest(&mut screen, &mut view, &mut session, 4);
        assert!(view.problem_popups_open().is_empty(), "no instant popup after the first one");
        rest(&mut screen, &mut view, &mut session, frames);
        let image = screen.frame(Vec::new(), |ui| show(ui, &mut view, &mut session));
        assert_eq!(view.problem_popups_open(), ["problem"]);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join("8-hover-deprecated.png"), &image).unwrap();
        }

        step(&mut screen, &mut view, &mut session, moved(at(40.0, 3.0)));
        rest(&mut screen, &mut view, &mut session, 14);
        assert!(view.problem_popups_open().is_empty(), "hidden when the pointer leaves");

        registry.dispatch(&mut session, "editor.caret.goTo", json!({ "line": 8, "column": 5 })).unwrap();
        let ctx = screen.context().clone();
        view.context_actions(&ctx, &registry, &mut session, true);
        rest(&mut screen, &mut view, &mut session, 3);
        assert_eq!(view.problem_popups_open(), ["problemDetails"]);
    }

    struct Replayed(Vec<zen_project::inspect::Finding>);

    impl zen_project::inspect::Inspector for Replayed {
        fn id(&self) -> &str {
            "replayed"
        }

        fn handles(&self, file_name: &str) -> bool {
            file_name == "go.mod"
        }

        fn inspect(&self, _context: &zen_project::inspect::InspectContext<'_>) -> Vec<zen_project::inspect::Finding> {
            self.0.clone()
        }

        fn apply(
            &self,
            _context: &zen_project::inspect::InspectContext<'_>,
            _fixes: &[zen_project::inspect::InspectionFix],
        ) -> Result<Box<dyn zen_project::inspect::FixJob>, String> {
            Err("not in tests".to_owned())
        }
    }

    #[test]
    fn problem_popup_gallery() {
        let Some(folder) = std::env::var_os("ZEN_UI_GALLERY").map(PathBuf::from) else { return };
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let text = std::fs::read_to_string(folder.join("go.mod")).unwrap();
        let problems: Vec<Value> =
            serde_json::from_slice(&std::fs::read(folder.join("problems.json")).unwrap()).unwrap();
        let findings: Vec<zen_project::inspect::Finding> =
            problems.iter().map(|problem| serde_json::from_value(problem.clone()).unwrap()).collect();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("go.mod");
        std::fs::write(&file, &text).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session
            .state_mut::<zen_workbench::inspections::Inspections>()
            .set_inspectors(vec![Arc::new(Replayed(findings))], None);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(vec2(1100.0, 600.0), 2.0);
        palette.apply(screen.context());
        let mut view = EditorView::default();
        let colours: Arc<dyn Highlighter> = Arc::new(GoModColours);
        let show = |ui: &mut Ui, view: &mut EditorView, session: &mut Session| {
            ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
            view.show(ui, &registry, session, Some(Arc::clone(&colours)), &palette);
        };
        screen.step(Vec::new(), |ui| show(ui, &mut view, &mut session));
        let char_width = screen.context().fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let gutter = gutter_width(2, char_width, false);
        let line_height = view.line_height;
        for (index, problem) in problems.iter().enumerate() {
            let line = problem["line"].as_u64().unwrap() as usize;
            let column = problem["column"].as_u64().unwrap() as usize;
            let line_text = text.lines().nth(line - 1).unwrap_or_default();
            let shown: usize = line_text.chars().take(column - 1).map(|c| if c == '\t' { 4 } else { 1 }).sum();
            let pos =
                pos2(gutter + TEXT_LEFT + char_width * (shown as f32 + 1.5), TOP + line_height * (line as f32 - 0.5));
            screen.step(vec![Event::PointerMoved(pos2(5.0, 5.0))], |ui| show(ui, &mut view, &mut session));
            for _ in 0..20 {
                screen.step(Vec::new(), |ui| show(ui, &mut view, &mut session));
            }
            screen.step(vec![Event::PointerMoved(pos)], |ui| show(ui, &mut view, &mut session));
            for _ in 0..40 {
                screen.step(Vec::new(), |ui| show(ui, &mut view, &mut session));
            }
            let image = screen.frame(Vec::new(), |ui| show(ui, &mut view, &mut session));
            let code = problem["code"].as_str().unwrap_or_default();
            crate::offscreen::write_png(&folder.join(format!("popup-{index:02}-{code}.png")), &image).unwrap();
        }
    }

    impl Harness {
        fn with_file(name: &str, contents: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join(name);
            std::fs::write(&file, contents).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.set_state(Workbench::default());
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let mut harness =
                Self { ctx: egui::Context::default(), registry, session, view: EditorView::default(), _dir: dir };
            harness.frame(Vec::new());
            harness.frame(Vec::new());
            harness
        }

        fn row_starts(&mut self, line: usize) -> Vec<usize> {
            let text = self.text();
            let content = text.lines().nth(line).unwrap_or_default().to_owned();
            let editor = EditorMetrics::new(&self.ctx, &self.view.font, 4);
            let metrics = soft_wrap::Metrics {
                ctx: &self.ctx,
                font: &editor.font,
                tab: 4,
                width: self.view.soft_wrap.width(),
                char_width: editor.char_width,
            };
            let wrap = self.view.soft_wrap.measure(metrics, &content).wrap.clone();
            (0..wrap.rows()).map(|row| wrap.row_range(row).start).collect()
        }

        fn numbers(&mut self) -> Vec<(String, f32)> {
            fn walk(shape: &egui::Shape, found: &mut Vec<(String, f32)>) {
                match shape {
                    egui::Shape::Text(text) if text.galley.text().chars().all(|c| c.is_ascii_digit()) => {
                        found.push((text.galley.text().to_owned(), text.pos.y + text.galley.size().y / 2.0));
                    }
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
                    _ => {}
                }
            }
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            };
            let palette = IdePalette::zen_dark();
            let output = self.ctx.run_ui(input, |ui| {
                self.view.show(ui, &self.registry, &mut self.session, None, &palette);
            });
            let mut found = Vec::new();
            for clipped in &output.shapes {
                walk(&clipped.shape, &mut found);
            }
            found
        }

        fn text_left(&self, lines: usize) -> f32 {
            gutter_width(lines, self.char_width(), false) + TEXT_LEFT
        }

        fn soft_wrapped(&mut self) -> bool {
            self.registry.dispatch(&mut self.session, "editor.softWrap.get", json!({})).unwrap()["enabled"] == true
        }
    }

    const PROSE: &str = "The quick brown fox jumps over the lazy dog and keeps running across the wide green field \
        until the evening comes, then it rests under an old oak tree and dreams of the next morning's run.";

    fn prose_file() -> String {
        format!("{PROSE} {PROSE}\nsecond\n")
    }

    #[test]
    fn markdown_wraps_at_the_edge_and_numbers_stay_on_a_lines_first_row() {
        let mut editor = Harness::with_file("README.md", &prose_file());
        assert!(editor.soft_wrapped());
        let starts = editor.row_starts(0);
        assert!(starts.len() >= 3, "{starts:?}");
        let rows = editor.view.soft_wrap.kept().unwrap().clone();
        assert_eq!((rows.height(0), rows.top(1), rows.total()), (starts.len(), starts.len(), starts.len() + 2));
        let numbers = editor.numbers();
        let y = |number: &str| numbers.iter().find(|(text, _)| text == number).map(|(_, y)| *y).unwrap();
        let line_height = editor.view.line_height;
        assert!((y("2") - y("1") - line_height * starts.len() as f32).abs() < 0.5, "{numbers:?}");
        assert_eq!(numbers.len(), 3, "one number per line, not per row: {numbers:?}");
    }

    #[test]
    fn a_wrapped_row_continues_under_the_lines_indent() {
        let mut editor = Harness::with_file("notes.txt", &format!("    {PROSE}\n"));
        let starts = editor.row_starts(0);
        let left = editor.text_left(2);
        let char_width = editor.char_width();
        editor.click(pos2(left + char_width * 4.0 + 1.0, editor.view.line_height * 1.5));
        assert_eq!(editor.caret(), starts[1], "the second row starts four columns in");
        editor.click(pos2(left + 1.0, editor.view.line_height * 1.5));
        assert_eq!(editor.caret(), starts[1], "left of the indent is still the row's first character");
    }

    #[test]
    fn clicks_on_a_wrapped_line_find_their_row() {
        let mut editor = Harness::with_file("README.md", &prose_file());
        let starts = editor.row_starts(0);
        let (left, char_width, line_height) = (editor.text_left(3), editor.char_width(), editor.view.line_height);
        editor.click(pos2(left + char_width * 3.0 + 1.0, line_height * 1.5));
        assert_eq!(editor.caret(), starts[1] + 3);
        assert!(!editor.view.soft_wrap.before_wrap(editor.caret()));
        editor.click(pos2(780.0, line_height * 0.5));
        assert_eq!(editor.caret(), starts[1], "past a row's end: the caret stays on that row, before the wrap");
        assert!(editor.view.soft_wrap.before_wrap(starts[1]));
        let second = prose_file().find("second").unwrap();
        editor.click(pos2(left + char_width * 2.0 + 1.0, line_height * (starts.len() as f32 + 0.5)));
        assert_eq!(editor.caret(), second + 2, "the row after the wrapped line is the next line");
    }

    #[test]
    fn up_and_down_go_by_visual_rows_and_home_and_end_stay_in_a_row() {
        let mut editor = Harness::with_file("README.md", &prose_file());
        let starts = editor.row_starts(0);
        editor.set_caret(3);
        editor.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(editor.caret(), starts[1] + 3, "down keeps the column on the next row of the same line");
        editor.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(editor.caret(), starts[2] + 3);
        editor.key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(editor.caret(), starts[1] + 3);
        editor.key(Key::End, Modifiers::NONE);
        assert_eq!(editor.caret(), starts[2], "End goes to the row's end");
        assert!(editor.view.soft_wrap.before_wrap(starts[2]), "drawn before the wrap");
        editor.key(Key::End, Modifiers::NONE);
        let line_end = prose_file().find('\n').unwrap();
        assert_eq!(editor.caret(), line_end, "End again goes to the line's end");
        editor.key(Key::Home, Modifiers::NONE);
        assert_eq!(editor.caret(), *starts.last().unwrap(), "Home goes to the row's start");
        editor.key(Key::Home, Modifiers::NONE);
        assert_eq!(editor.caret(), 0, "Home again goes to the line's start");
        editor.set_caret(starts[1] + 3);
        editor.key(Key::ArrowDown, Modifiers::SHIFT);
        let selection = editor.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary();
        assert_eq!((selection.anchor, selection.head), (starts[1] + 3, starts[2] + 3), "⇧↓ extends by a row");
        editor.set_caret(line_end - 1);
        editor.key(Key::ArrowDown, Modifiers::NONE);
        let second = prose_file().find("second").unwrap();
        assert!(
            (second..second + 7).contains(&editor.caret()),
            "down from the last row goes to the next line: {}",
            editor.caret()
        );
    }

    #[test]
    fn a_folded_region_in_a_wrapped_file_is_one_row() {
        let long = PROSE.replace(',', "");
        let contents = format!("func main() {{\n\t// {long}\n\treturn\n}}\n// {long}\n");
        let mut editor = Harness::new(&contents);
        assert!(!editor.soft_wrapped(), "Go files do not wrap by default");
        editor.registry.dispatch(&mut editor.session, "editor.softWrap.toggle", json!({})).unwrap();
        editor.frame(Vec::new());
        editor.frame(Vec::new());
        let unfolded = editor.view.soft_wrap.kept().unwrap().total();
        editor.registry.dispatch(&mut editor.session, "editor.fold.collapseAll", json!({})).unwrap();
        editor.frame(Vec::new());
        editor.frame(Vec::new());
        let rows = editor.view.soft_wrap.kept().unwrap().clone();
        assert_eq!(rows[..2], [0, 4], "the folded block is one row, then the comment after it");
        assert!(rows.height(1) > 1 && rows.total() < unfolded, "the comment after the fold still wraps");
        let line_height = editor.view.line_height;
        let left = editor.text_left(6);
        editor.click(pos2(left + editor.char_width() * 1.0 + 1.0, line_height * 1.5));
        assert_eq!(editor.caret(), contents.find("\n// ").unwrap() + 2);
        editor.set_caret(0);
        editor.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(editor.caret(), contents.find("\n// ").unwrap() + 1, "down skips the folded lines");
    }

    #[test]
    fn the_toggle_turns_wraps_off_and_on_for_the_file() {
        let mut editor = Harness::with_file("README.md", &prose_file());
        assert!(editor.view.soft_wrap.kept().is_some());
        let state = editor.registry.dispatch(&mut editor.session, "editor.softWrap.toggle", json!({})).unwrap();
        assert_eq!(state["enabled"], false);
        editor.frame(Vec::new());
        assert!(editor.view.soft_wrap.kept().is_none(), "the long line is one row again");
        editor.set_caret(3);
        editor.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(editor.caret(), prose_file().find("second").unwrap() + 3, "down goes to the next line");
        editor.registry.dispatch(&mut editor.session, "editor.softWrap.toggle", json!({})).unwrap();
        editor.frame(Vec::new());
        assert!(editor.view.soft_wrap.kept().unwrap().height(0) > 1);
    }

    #[test]
    fn typing_lays_out_only_the_lines_it_changed_not_the_whole_file() {
        let contents: String = (0..5000).map(|line| format!("{line} {PROSE}\n")).collect();
        let mut editor = Harness::with_file("big.md", &contents);
        let first = editor.view.soft_wrap.laid_out();
        assert!(first < 60, "only the lines near the viewport are laid out: {first}");
        let rows = editor.view.soft_wrap.kept().unwrap();
        assert!(rows.total() > 10_000, "the others are estimated: {}", rows.total());
        editor.set_caret(1);
        for c in ["a", "b", "c"] {
            editor.frame(vec![Event::Text(c.to_owned())]);
        }
        assert!(editor.text().starts_with("0abc "));
        let typed = editor.view.soft_wrap.laid_out() - first;
        assert!(typed <= 3, "one line per keystroke, the edited one: {typed}");
        editor
            .registry
            .dispatch(
                &mut editor.session,
                "editor.selection.set",
                json!({ "selections": [{ "head": contents.len() - 2 }] }),
            )
            .unwrap();
        editor.frame(Vec::new());
        editor.frame(Vec::new());
        let scrolled = editor.view.soft_wrap.laid_out() - first - typed;
        assert!(scrolled < 60, "going to the end lays out the lines shown there: {scrolled}");
    }

    impl Harness {
        fn frame_sized(&mut self, width: f32) {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 600.0))),
                ..Default::default()
            };
            let palette = IdePalette::zen_dark();
            let mut output = self.ctx.run_ui(input, |ui| {
                self.view.show(ui, &self.registry, &mut self.session, None, &palette);
            });
            output.textures_delta.clear();
        }

        fn line_of(&self, offset: usize) -> usize {
            self.text().chars().take(offset).filter(|&c| c == '\n').count()
        }
    }

    fn numbered_prose(lines: usize) -> String {
        (0..lines).map(|line| format!("{line} {PROSE}\n")).collect()
    }

    #[test]
    fn a_file_that_does_not_wrap_lays_out_no_rows() {
        let contents: String = (0..400).map(|line| format!("// {line} {PROSE} {PROSE}\n")).collect();
        let mut editor = Harness::new(&contents);
        assert!(!editor.soft_wrapped());
        editor.set_caret(contents.len() / 2);
        for c in ["a", "b"] {
            editor.frame(vec![Event::Text(c.to_owned())]);
        }
        editor.key(Key::ArrowDown, Modifiers::NONE);
        editor.key(Key::PageDown, Modifiers::NONE);
        editor.frame(vec![Event::PointerMoved(pos2(300.0, 200.0))]);
        assert_eq!(editor.view.soft_wrap.laid_out(), 0, "nothing is measured for wraps");
        assert!(editor.view.soft_wrap.kept().is_none());
        assert!(editor.session.state::<zen_workbench::soft_wrap::ShownVisualRows>().is_none());
    }

    #[test]
    fn keys_pressed_in_one_frame_each_go_a_row() {
        let mut editor = Harness::with_file("README.md", &prose_file());
        let starts = editor.row_starts(0);
        editor.set_caret(3);
        let down = Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        editor.frame(vec![down.clone(), down]);
        assert_eq!(editor.caret(), starts[2] + 3);
    }

    #[test]
    fn page_down_goes_by_the_rows_on_the_screen() {
        let mut editor = Harness::with_file("notes.md", &numbered_prose(300));
        let rows_per_line = editor.row_starts(0).len();
        assert!(rows_per_line >= 2, "{rows_per_line}");
        let page = editor.view.page_lines;
        editor.key(Key::PageDown, Modifiers::NONE);
        let line = editor.line_of(editor.caret());
        assert!(line > 0 && line <= page / rows_per_line + 1, "a page of {page} rows went {line} lines");
        editor.key(Key::PageUp, Modifiers::NONE);
        assert_eq!(editor.caret(), 0);
    }

    #[test]
    fn agents_move_by_rows_with_visual() {
        let mut editor = Harness::with_file("README.md", &prose_file());
        let starts = editor.row_starts(0);
        editor.set_caret(3);
        let moved = editor
            .registry
            .dispatch(
                &mut editor.session,
                "editor.caret.move",
                json!({ "movement": { "kind": "down" }, "visual": true }),
            )
            .unwrap();
        assert_eq!(moved[0]["head"], starts[1] + 3);
        editor
            .registry
            .dispatch(
                &mut editor.session,
                "editor.caret.move",
                json!({ "movement": { "kind": "lineEnd" }, "visual": true }),
            )
            .unwrap();
        assert_eq!(editor.caret(), starts[2]);
        editor.set_caret(3);
        editor
            .registry
            .dispatch(&mut editor.session, "editor.caret.move", json!({ "movement": { "kind": "down" } }))
            .unwrap();
        assert_eq!(editor.caret(), prose_file().find("second").unwrap() + 3, "without visual: the next line");
    }

    #[test]
    fn the_line_at_the_top_stays_there_when_the_width_changes() {
        let mut editor = Harness::with_file("notes.md", &numbered_prose(600));
        let target = numbered_prose(600).find("\n400 ").unwrap() + 1;
        editor.set_caret(target);
        for _ in 0..60 {
            editor.frame(Vec::new());
        }
        let top = editor.view.shown_lines.0;
        assert!(top > 300, "scrolled down to {top}");
        for width in [700.0, 560.0, 450.0, 450.0, 450.0, 900.0, 900.0, 900.0] {
            editor.frame_sized(width);
        }
        assert_eq!(editor.view.shown_lines.0, top, "the same line is at the top");
    }

    #[test]
    fn scrolling_up_into_lines_not_laid_out_moves_the_text_by_the_scroll_only() {
        let word = "w".repeat(50);
        let contents: String = (0..600).map(|line| format!("{line} {word} {word} {word} {word}\n")).collect();
        let mut editor = Harness::with_file("notes.log", &contents);
        assert!(!editor.soft_wrapped());
        editor.set_caret(contents.find("\n400 ").unwrap() + 1);
        for _ in 0..60 {
            editor.frame(Vec::new());
        }
        editor.registry.dispatch(&mut editor.session, "editor.softWrap.toggle", json!({})).unwrap();
        for _ in 0..3 {
            editor.frame(Vec::new());
        }
        let watched = (editor.view.shown_lines.0 + 3).to_string();
        let y_of = |editor: &mut Harness| {
            editor.numbers().iter().find(|(number, _)| *number == watched).map(|(_, y)| *y).unwrap()
        };
        let before = y_of(&mut editor);
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 200.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        };
        editor.frame(vec![Event::PointerMoved(pos2(400.0, 300.0)), wheel]);
        for _ in 0..30 {
            editor.frame(Vec::new());
        }
        let after = y_of(&mut editor);
        assert!((after - before - 200.0).abs() < 1.5, "the line moved {} for a scroll of 200", after - before);
    }

    #[test]
    fn every_caret_goes_a_row_and_shift_extends_each_selection() {
        let mut editor = Harness::with_file("README.md", &prose_file());
        let starts = editor.row_starts(0);
        let second = prose_file().find("second").unwrap();
        editor
            .registry
            .dispatch(
                &mut editor.session,
                "editor.selection.set",
                json!({ "selections": [{ "head": 3 }, { "head": second + 2 }] }),
            )
            .unwrap();
        editor.frame(Vec::new());
        editor.key(Key::ArrowDown, Modifiers::SHIFT);
        let document = editor.session.state::<Workbench>().unwrap().active_document().unwrap();
        let selections: Vec<(usize, usize)> =
            document.selections().iter().map(|selection| (selection.anchor, selection.head)).collect();
        assert_eq!(selections[0], (3, starts[1] + 3), "the first caret extends by a row of its line");
        assert_eq!(selections[1].0, second + 2, "the second keeps its anchor: {selections:?}");
        assert!(selections[1].1 > second + 2, "and goes down past its line: {selections:?}");
    }

    #[test]
    fn colour_literals_show_swatches_and_the_picker_writes_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("style.css");
        std::fs::write(&file, "a { color: #f00; border-color: tan }\nb { }\nc { color: rgb(0 0 255) }\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        let css = zen_plugins::native::NativePlugin::new(
            &["css"],
            zen_plugins::native::Exports { colors: Some(zen_plugin_css::colors_request), ..Default::default() },
        );
        session.state_mut::<zen_workbench::plugins::PluginFeatures>().set(vec![Arc::new(css)]);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let mut editor =
            Harness { ctx: egui::Context::default(), registry, session, view: EditorView::default(), _dir: dir };
        let shapes = editor.painted();
        let lines: Vec<usize> = editor.view.swatches.drawn.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [0, 2], "one swatch per line, on the lines with colours");
        let filled = |shapes: &[egui::Shape], color: Color32| {
            shapes.iter().any(|shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == color))
        };
        assert!(filled(&shapes, Color32::from_rgb(255, 0, 0)), "the first colour of the line");
        assert!(!filled(&shapes, Color32::from_rgb(0xd2, 0xb4, 0x8c)), "not the second one");
        assert!(filled(&shapes, Color32::from_rgb(0, 0, 255)));
        let (_, swatch) = editor.view.swatches.drawn[1];
        assert!(swatch.right() <= editor.char_width() * 20.0, "in the gutter");
        editor.click(swatch.center());
        assert_eq!(editor.caret(), 0, "a click on a swatch does not select the line");
        editor.view.swatches.pick([0, 255, 0, 128]);
        editor.frame(Vec::new());
        editor.click(pos2(600.0, 500.0));
        assert_eq!(editor.text(), "a { color: #f00; border-color: tan }\nb { }\nc { color: rgb(0 255 0 / 0.5) }\n");
        editor.click(editor.view.swatches.drawn[0].1.center());
        editor.view.swatches.pick([0, 0, 0, 255]);
        editor.key(Key::Escape, Modifiers::NONE);
        editor.click(pos2(600.0, 500.0));
        assert!(editor.text().starts_with("a { color: #f00;"), "Escape leaves the colour as it was");
    }

    #[test]
    fn with_regex_on_every_match_shows_its_replacement_as_a_hint() {
        let mut editor = Harness::new("user_name := 1\nuser_id := 2\n");
        let run = |editor: &mut Harness, id: &str, params: Value| {
            editor.registry.dispatch(&mut editor.session, id, params).unwrap();
        };
        run(&mut editor, "editor.find.open", json!({ "replace": true }));
        let query = json!({ "text": r"user_(\w+)", "regex": true });
        run(&mut editor, "editor.find.set", json!({ "query": query, "replacement": r"user\u$1" }));
        editor.frame(Vec::new());
        let labels = |editor: &Harness| -> Vec<String> {
            editor
                .view
                .previews
                .as_ref()
                .map(|(_, _, _, merged)| merged.iter().map(|hint| hint.label.clone()).collect())
                .unwrap_or_default()
        };
        assert_eq!(labels(&editor), ["userName", "userId"]);
        assert_eq!(editor.view.inlay_hints.drawn().len(), 2, "both are drawn");
        assert_eq!(editor.view.previews.as_ref().unwrap().3[0].offset, 9, "after the match");
        let first = Arc::clone(&editor.view.previews.as_ref().unwrap().3);
        editor.frame(Vec::new());
        assert!(Arc::ptr_eq(&first, &editor.view.previews.as_ref().unwrap().3), "kept between frames");
        run(&mut editor, "editor.find.exclude", json!({}));
        editor.frame(Vec::new());
        assert_eq!(labels(&editor), ["userId"], "an excluded match shows none");
        let plain = json!({ "text": "user_", "regex": false });
        run(&mut editor, "editor.find.set", json!({ "query": plain }));
        editor.frame(Vec::new());
        assert!(editor.view.previews.is_none(), "plain text shows no previews");
        assert!(editor.view.inlay_hints.drawn().is_empty());
    }

    struct Declarations;

    impl zen_syntax::Outliner for Declarations {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn outline(&self, text: &str) -> Vec<zen_syntax::OutlineItem> {
            let item = |name: &str, from: &str| {
                let at = text[..text.find(from)?].chars().count();
                Some(zen_syntax::OutlineItem {
                    name: name.to_owned(),
                    detail: None,
                    kind: zen_syntax::SymbolKind::Function,
                    start: at,
                    end: at + from.chars().count(),
                    selection_start: at,
                    children: Vec::new(),
                })
            };
            [item("a", "func a"), item("b", "func b")].into_iter().flatten().collect()
        }
    }

    const NUMBERED: &str = "package main\n\nfunc a() {\n}\n\n// b is the second.\nfunc b() {\n}\n";

    fn number_texts(editor: &mut Harness) -> Vec<String> {
        fn walk(shape: &egui::Shape, found: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) if text.galley.text().chars().all(|c| c.is_ascii_digit()) => {
                    found.push(text.galley.text().to_owned());
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for shape in editor.painted() {
            walk(&shape, &mut found);
        }
        found
    }

    fn separator_ys(editor: &mut Harness) -> Vec<f32> {
        fn walk(shape: &egui::Shape, found: &mut Vec<f32>) {
            match shape {
                egui::Shape::LineSegment { points, stroke }
                    if stroke.color == IdePalette::zen_dark().method_separator && points[0].y == points[1].y =>
                {
                    found.push(points[0].y);
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for shape in editor.painted() {
            walk(&shape, &mut found);
        }
        found
    }

    #[test]
    fn the_line_numbers_follow_the_mode_and_hide_with_the_active_editor() {
        let mut editor = Harness::new(NUMBERED);
        let caret = NUMBERED.find("// b").unwrap();
        editor
            .registry
            .dispatch(&mut editor.session, "editor.selection.set", json!({ "selections": [{ "head": caret }] }))
            .unwrap();
        assert_eq!(number_texts(&mut editor)[..8], ["1", "2", "3", "4", "5", "6", "7", "8"]);
        let settings = editor.session.settings_mut();
        settings.set(zen_workbench::line_numbers::LINE_NUMBERS_MODE, "relative").unwrap();
        assert_eq!(number_texts(&mut editor)[..8], ["5", "4", "3", "2", "1", "0", "1", "2"]);
        let settings = editor.session.settings_mut();
        settings.set(zen_workbench::line_numbers::LINE_NUMBERS_MODE, "hybrid").unwrap();
        assert_eq!(number_texts(&mut editor)[..8], ["5", "4", "3", "2", "1", "6", "1", "2"]);
        assert!(editor.view.line_numbers_on);
        editor.registry.dispatch(&mut editor.session, "editor.lineNumbers.toggle", json!({})).unwrap();
        assert!(number_texts(&mut editor).is_empty(), "hidden for the active editor");
        assert!(!editor.view.line_numbers_on, "the menu item loses its check mark");
        editor.registry.dispatch(&mut editor.session, "editor.lineNumbers.toggle", json!({})).unwrap();
        assert_eq!(number_texts(&mut editor).len(), 9);
        editor.view.hide_line_numbers = true;
        assert!(number_texts(&mut editor).is_empty(), "Distraction Free hides them whatever the setting");
        assert!(editor.view.line_numbers_on, "the editor still has them on");
    }

    #[test]
    fn method_separators_are_drawn_above_the_declarations_and_read_once_per_version() {
        let mut editor = Harness::new(NUMBERED);
        editor.session.state_mut::<zen_workbench::language::structure::Outliners>().set(vec![Arc::new(Declarations)]);
        assert!(separator_ys(&mut editor).is_empty(), "off by default");
        editor.session.settings_mut().set(zen_workbench::line_numbers::SHOW_METHOD_SEPARATORS, true).unwrap();
        let line_height = editor.view.line_height;
        let ys = separator_ys(&mut editor);
        assert_eq!(ys, [TOP + 2.0 * line_height + 0.5, TOP + 5.0 * line_height + 0.5], "above func a and b's comment");
        let reads = editor.view.separators.reads();
        for _ in 0..5 {
            editor.frame(Vec::new());
        }
        assert_eq!(editor.view.separators.reads(), reads, "not read again while the text stays the same");
        editor.session.settings_mut().set(zen_workbench::line_numbers::SHOW_METHOD_SEPARATORS, false).unwrap();
        assert!(separator_ys(&mut editor).is_empty());
    }

    #[test]
    fn method_separators_move_with_the_edits_until_they_are_read_again() {
        let mut editor = Harness::new(NUMBERED);
        editor.session.state_mut::<zen_workbench::language::structure::Outliners>().set(vec![Arc::new(Declarations)]);
        editor.session.settings_mut().set(zen_workbench::line_numbers::SHOW_METHOD_SEPARATORS, true).unwrap();
        let line_height = editor.view.line_height;
        let row = |row: f32| TOP + row * line_height + 0.5;
        assert_eq!(separator_ys(&mut editor), [row(2.0), row(5.0)]);
        let reads = editor.view.separators.reads();
        editor.set_caret(0);
        editor.registry.dispatch(&mut editor.session, "editor.edit.insert", json!({ "text": "\n\n" })).unwrap();
        assert_eq!(separator_ys(&mut editor), [row(4.0), row(7.0)], "two lines typed above move them down at once");
        let start = NUMBERED.find("func a").unwrap() + 2;
        let end = start + "func a() {\n}\n".len();
        editor
            .registry
            .dispatch(
                &mut editor.session,
                "editor.selection.set",
                json!({ "selections": [{ "anchor": start, "head": end }] }),
            )
            .unwrap();
        editor.registry.dispatch(&mut editor.session, "editor.edit.insert", json!({ "text": "" })).unwrap();
        assert_eq!(separator_ys(&mut editor), [row(5.0)], "the removed function's separator goes with it");
        editor.set_caret(NUMBERED.find("// b").unwrap() + 2 - "func a() {\n}\n".len());
        editor.registry.dispatch(&mut editor.session, "editor.edit.insert", json!({ "text": "x" })).unwrap();
        assert!(separator_ys(&mut editor).is_empty(), "a comment that is no longer one is not drawn above");
        assert_eq!(editor.view.separators.reads(), reads, "nothing is read while typing goes on");
        for _ in 0..30 {
            editor.frame(Vec::new());
        }
        assert_eq!(editor.view.separators.reads(), reads + 1, "read again once the text settled");
        assert_eq!(separator_ys(&mut editor), [row(6.0)], "above func b, the line above is no comment now");
    }

    #[test]
    fn the_unwrap_popup_marks_remove_entries_and_highlights_what_stays() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let text = "package main\n\nfunc main() {\n\tfor _, x := range xs {\n\t\tif x > 0 {\n\t\t\tprintln(x)\n\t\t} else {\n\t\t\tskip(x)\n\t\t\tcount++\n\t\t}\n\t}\n}\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, text).unwrap();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for &language in languages {
            zen_i18n::set_language(Some(language));
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            let caret = text[..text.find("skip").unwrap()].chars().count() + 2;
            registry
                .dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] }))
                .unwrap();
            let palette = IdePalette::zen_dark();
            let mut screen = crate::offscreen::Offscreen::new(vec2(640.0, 300.0), 2.0);
            palette.apply(screen.context());
            let mut view = EditorView::default();
            let show = |ui: &mut Ui, view: &mut EditorView, session: &mut Session| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                view.show(ui, &registry, session, None, &palette);
            };
            screen.step(Vec::new(), |ui| show(ui, &mut view, &mut session));
            assert_eq!(view.open_unwrap(&registry, &mut session), Ok(4));
            for _ in 0..30 {
                screen.step(Vec::new(), |ui| show(ui, &mut view, &mut session));
            }
            let image = screen.frame(Vec::new(), |ui| show(ui, &mut view, &mut session));
            let first = view.unwrap.kept().to_vec();
            assert_eq!(first.len(), 1, "the call's argument stays");
            let row = view.caret_rect.unwrap().left_bottom() + vec2(-24.0 + 40.0, 2.0 + 4.0 + 22.0 * 2.5);
            screen.step(vec![Event::PointerMoved(row)], |ui| show(ui, &mut view, &mut session));
            screen.step(Vec::new(), |ui| show(ui, &mut view, &mut session));
            let hovered = screen.frame(Vec::new(), |ui| show(ui, &mut view, &mut session));
            assert_eq!(view.unwrap.kept().len(), 2, "removing the else keeps what is before and after it");
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("unwrap-{language}-1-popup.png")), &image).unwrap();
                crate::offscreen::write_png(&dir.join(format!("unwrap-{language}-2-hover-remove.png")), &hovered)
                    .unwrap();
            }
        }
        zen_i18n::set_language(Some("en"));
    }

    fn extract_harness(contents: &str, edits: impl Fn(&Value) -> Value) -> (Harness, zen_lsp::fake::FakeServer) {
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, contents).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let server = zen_lsp::fake::FakeServer::new();
        server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "codeActionProvider": true
        }));
        let launched = server.clone();
        session.state_mut::<zen_workbench::language::Languages>().set_specs(vec![
            zen_workbench::language::ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch: Arc::new(move |_| Ok(launched.launcher())),
                initialization_options: json!({}),
            },
        ]);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let mut editor =
            Harness { ctx: egui::Context::default(), registry, session, view: EditorView::default(), _dir: dir };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while editor.session.state::<zen_workbench::language::Languages>().unwrap().synced_text(&file).is_none() {
            assert!(std::time::Instant::now() < deadline, "the server did not get the file");
            let _ = editor.registry.dispatch(&mut editor.session, "language.servers.sync", json!({}));
        }
        let uri = zen_lsp::position::file_uri(&file);
        let range = |line: u32, start: u32, end: u32| json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } });
        server.on(
            "textDocument/codeAction",
            json!([{
                "title": "Extract variable",
                "kind": "refactor.extract.variable",
                "edit": { "changes": { uri: edits(&json!([range(3, 0, 0), range(3, 13, 25)])) } }
            }]),
        );
        editor.frame(Vec::new());
        (editor, server)
    }

    fn until(editor: &mut Harness, done: impl Fn(&Harness) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !done(editor) {
            assert!(std::time::Instant::now() < deadline, "timed out: {:?}", editor.view.notice);
            editor.frame(Vec::new());
        }
    }

    #[test]
    fn extract_variable_asks_which_expression_and_names_it_in_place_everywhere() {
        let contents = "package main\n\nfunc main() {\n\tfmt.Println(total(a) + 1)\n}\n";
        let (mut editor, server) = extract_harness(contents, total);
        let caret = contents[..contents.find("a)").unwrap()].chars().count();
        editor
            .registry
            .dispatch(&mut editor.session, "editor.selection.set", json!({ "selections": [{ "head": caret }] }))
            .unwrap();
        editor.view.open_extract("refactor.extract.variable");
        until(&mut editor, |editor| editor.view.extract_open() == Some("extractExpressions"));
        assert_eq!(
            editor.view.extract.highlighted(),
            std::slice::from_ref(&(caret..caret + 1)),
            "the first expression is shown in the text"
        );
        editor.key(Key::ArrowDown, Modifiers::NONE);
        editor.key(Key::ArrowDown, Modifiers::NONE);
        let whole = contents[..contents.find("total").unwrap()].chars().count();
        assert_eq!(editor.view.extract.highlighted(), std::slice::from_ref(&(whole..whole + "total(a) + 1".len())));
        editor.key(Key::Enter, Modifiers::NONE);
        until(&mut editor, |editor| editor.view.extract_open() == Some("inPlaceName"));
        assert_eq!(server.requests("textDocument/codeAction")[0]["range"]["end"]["character"], 25);
        assert_eq!(editor.text(), "package main\n\nfunc main() {\n\tx := total(a) + 1\n\tfmt.Println(x)\n}\n");
        let selections = editor.session.state::<Workbench>().unwrap().active_document().unwrap().selections().len();
        assert_eq!(selections, 2, "every occurrence of the new name is selected");
        editor.frame(vec![Event::Text("sum".to_owned())]);
        assert_eq!(editor.text(), "package main\n\nfunc main() {\n\tsum := total(a) + 1\n\tfmt.Println(sum)\n}\n");
        assert_eq!(editor.view.extract.highlighted().len(), 2, "the occurrences follow the typing");
        editor.key(Key::Enter, Modifiers::NONE);
        assert_eq!(editor.view.extract_open(), None);
        assert_eq!(
            editor.text(),
            "package main\n\nfunc main() {\n\tsum := total(a) + 1\n\tfmt.Println(sum)\n}\n",
            "Enter ends the naming, no new line"
        );
        let document = editor.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.selections().len(), 1);
    }

    #[test]
    fn escape_keeps_the_servers_name_and_a_caret_elsewhere_ends_the_naming() {
        let contents = "package main\n\nfunc main() {\n\tfmt.Println(total(a) + 1)\n}\n";
        let (mut editor, _server) = extract_harness(contents, total);
        let start = contents[..contents.find("total").unwrap()].chars().count();
        let selection = json!({ "selections": [{ "anchor": start, "head": start + "total(a) + 1".len() }] });
        editor.registry.dispatch(&mut editor.session, "editor.selection.set", selection).unwrap();
        editor.view.open_extract("refactor.extract.variable");
        until(&mut editor, |editor| editor.view.extract_open() == Some("inPlaceName"));
        editor.key(Key::Escape, Modifiers::NONE);
        assert_eq!(editor.view.extract_open(), None);
        let after = "package main\n\nfunc main() {\n\tx := total(a) + 1\n\tfmt.Println(x)\n}\n";
        assert_eq!(editor.text(), after);
        let document = editor.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.selections().len(), 1);
        assert!(document.selections().primary().is_empty(), "the name is no longer selected");

        editor.registry.dispatch(&mut editor.session, "editor.edit.undo", json!({})).unwrap();
        assert_eq!(editor.text(), contents);
        editor.registry.dispatch(&mut editor.session, "editor.selection.set", selection_of(start)).unwrap();
        editor.view.open_extract("refactor.extract.variable");
        until(&mut editor, |editor| editor.view.extract_open() == Some("inPlaceName"));
        editor
            .registry
            .dispatch(&mut editor.session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] }))
            .unwrap();
        editor.frame(Vec::new());
        assert_eq!(editor.view.extract_open(), None, "moving away ends it");
    }

    fn total(ranges: &Value) -> Value {
        json!([
            { "range": ranges[0], "newText": "\tx := total(a) + 1\n" },
            { "range": ranges[1], "newText": "x" }
        ])
    }

    #[test]
    fn a_suggested_name_is_picked_with_the_arrows_and_enter() {
        let contents = "package main\n\nfunc main() {\n\tfmt.Println(cart.getTotal())\n}\n";
        let (mut editor, _server) = extract_harness(contents, |ranges| {
            let mut whole = ranges[1].clone();
            whole["end"]["character"] = json!(28);
            json!([
                { "range": ranges[0], "newText": "\tx := cart.getTotal()\n" },
                { "range": whole, "newText": "x" }
            ])
        });
        let start = contents[..contents.find("cart").unwrap()].chars().count();
        let selection = json!({ "selections": [{ "anchor": start, "head": start + "cart.getTotal()".len() }] });
        editor.registry.dispatch(&mut editor.session, "editor.selection.set", selection).unwrap();
        editor.view.open_extract("refactor.extract.variable");
        until(&mut editor, |editor| editor.view.extract_open() == Some("inPlaceName"));
        assert_eq!(
            editor.view.extract.suggested(&editor.session),
            ["x", "total"],
            "the server's name, then one from the call"
        );
        editor.key(Key::ArrowDown, Modifiers::NONE);
        editor.key(Key::Enter, Modifiers::NONE);
        assert_eq!(
            editor.text(),
            "package main\n\nfunc main() {\n\ttotal := cart.getTotal()\n\tfmt.Println(total)\n}\n"
        );
        assert_eq!(editor.view.extract_open(), None);
        let caret = editor.caret();
        let after = editor.text();
        assert_eq!(
            caret,
            after[..after.find("total :=").unwrap()].chars().count() + "total".len(),
            "the caret ends after the declared name"
        );
    }

    fn selection_of(start: usize) -> Value {
        json!({ "selections": [{ "anchor": start, "head": start + "total(a) + 1".len() }] })
    }
}
