use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use super::{Languages, LookupKind, LookupParams, WaitParams, char_offset, file_text, with_languages};
use crate::Workbench;

pub const AT_CARET: &str = "editor.highlightUsages.atCaret";

pub fn at_caret(settings: &zen_core::Settings) -> bool {
    settings.get_or(AT_CARET, true)
}

/// How a highlighted range uses the symbol, as the protocol's `DocumentHighlightKind` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HighlightKind {
    /// A textual occurrence: the text matched, or the server did not say how it is used.
    Text,
    /// The symbol is read there.
    Read,
    /// The symbol is written there: declared, assigned or changed.
    Write,
}

impl HighlightKind {
    fn from_protocol(kind: &Value) -> Self {
        match kind.as_u64() {
            Some(2) => Self::Read,
            Some(3) => Self::Write,
            _ => Self::Text,
        }
    }
}

/// One highlighted range of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct HighlightRange {
    /// The first character.
    pub start: usize,
    /// The character after the last.
    pub end: usize,
    /// How it uses the symbol.
    pub kind: HighlightKind,
}

/// Where the highlighted ranges come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HighlightSource {
    /// Nothing is highlighted.
    #[default]
    None,
    /// The language server's `textDocument/documentHighlight`.
    Server,
    /// The name's whole-word occurrences in the text, for a file no server highlights.
    Text,
    /// The selected text's occurrences anywhere in the file.
    Selection,
}

/// A set of highlighted usages in a file.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HighlightsState {
    /// The file; `None` when nothing is highlighted.
    pub path: Option<PathBuf>,
    /// Where the ranges come from.
    pub source: HighlightSource,
    /// Whether the server has not answered yet.
    pub pending: bool,
    /// The ranges, in text order.
    pub ranges: Vec<HighlightRange>,
}

impl HighlightsState {
    fn ready(path: &Path, source: HighlightSource, ranges: Vec<HighlightRange>) -> Self {
        Self { path: Some(path.to_owned()), source, pending: false, ranges }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    path: PathBuf,
    version: u64,
    offset: usize,
    selection: Option<Range<usize>>,
}

#[derive(Debug)]
struct Slot {
    key: Key,
    state: HighlightsState,
}

#[derive(Debug, Default)]
pub struct Highlights {
    caret: Option<Slot>,
    pinned: Option<Slot>,
}

/// The parameters of `editor.highlights.get` and of the commands that pin highlights.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HighlightParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset of the symbol; the primary caret when absent, and then a non-empty
    /// selection of the file highlights its text's occurrences instead.
    #[serde(default)]
    pub offset: Option<usize>,
    /// The line, one-based as answers count them: with `column`, the place instead of `offset`.
    #[serde(default)]
    pub line: Option<u32>,
    /// The column on `line`, one-based, in characters; 1 when absent.
    #[serde(default)]
    pub column: Option<u32>,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

impl HighlightParams {
    fn lookup(&self) -> LookupParams {
        LookupParams {
            path: self.path.clone(),
            offset: self.offset,
            line: self.line,
            column: self.column,
            wait_ms: self.wait_ms,
        }
    }
}

fn version_and_selection(workbench: &Workbench, path: &Path, explicit: bool) -> (u64, Option<Range<usize>>) {
    let Some(document) = workbench.documents().iter().find(|document| document.path() == path) else {
        return (0, None);
    };
    let primary = document.selections().primary();
    let selection = (!explicit && !primary.is_empty()).then(|| primary.range());
    (document.version(), selection)
}

fn key_of(session: &Session, params: &HighlightParams) -> Result<Key, CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let (path, offset) = params.lookup().place(workbench)?;
    let explicit = params.offset.is_some() || params.line.is_some();
    let (version, selection) = version_and_selection(workbench, &path, explicit);
    Ok(Key { path, version, offset, selection })
}

fn server_came(languages: &mut Languages, slot: &Slot) -> bool {
    slot.state.source == HighlightSource::Text
        && languages.client_for(&slot.key.path).is_some_and(|client| client.document_highlights())
}

fn start(session: &mut Session, kind: LookupKind, key: &Key) -> Result<HighlightsState, CommandError> {
    let text = {
        let workbench = session.state_mut::<Workbench>();
        file_text(workbench, &key.path).map_err(CommandError::new)?
    };
    if let Some(selection) = &key.selection {
        session.state_mut::<Languages>().end_lookup(kind);
        let ranges = zen_editor::occurrences::selection_occurrences(&text, selection.clone())
            .into_iter()
            .map(|range| HighlightRange { start: range.start, end: range.end, kind: HighlightKind::Text })
            .collect();
        return Ok(HighlightsState::ready(&key.path, HighlightSource::Selection, ranges));
    }
    let asked = with_languages(session, |languages, workbench| {
        languages.end_lookup(kind);
        let highlights = languages.client_for(&key.path).is_some_and(|client| client.document_highlights());
        highlights
            && languages
                .start_lookup(workbench, kind, &key.path, key.offset, "textDocument/documentHighlight", json!({}))
                .is_ok()
    });
    if asked {
        return Ok(HighlightsState { path: Some(key.path.clone()), pending: true, ..HighlightsState::default() });
    }
    Ok(textual(&key.path, &text, key.offset))
}

fn textual(path: &Path, text: &str, offset: usize) -> HighlightsState {
    let ranges: Vec<HighlightRange> = zen_editor::occurrences(text, offset)
        .into_iter()
        .map(|range| HighlightRange { start: range.start, end: range.end, kind: HighlightKind::Text })
        .collect();
    let source = if ranges.is_empty() { HighlightSource::None } else { HighlightSource::Text };
    HighlightsState::ready(path, source, ranges)
}

fn poll(session: &mut Session, kind: LookupKind, slot: &mut Slot, wait: Duration) {
    if !slot.state.pending {
        return;
    }
    let languages = session.state_mut::<Languages>();
    let answer = if languages.lookup(kind).is_none() || languages.is_outdated(kind) {
        languages.end_lookup(kind);
        Some(Err(String::new()))
    } else if languages.lookup_mut(kind).is_some_and(|lookup| lookup.poll(wait)) {
        languages.end_lookup(kind).and_then(|lookup| lookup.answer)
    } else {
        None
    };
    let Some(answer) = answer else { return };
    slot.state = match answer {
        Ok(answer) => {
            let synced = languages.synced_text(&slot.key.path).unwrap_or_default();
            HighlightsState::ready(&slot.key.path, HighlightSource::Server, server_ranges(synced, &answer))
        }
        Err(_) => {
            let text = file_text(session.state_mut::<Workbench>(), &slot.key.path).unwrap_or_default();
            textual(&slot.key.path, &text, slot.key.offset)
        }
    };
}

pub fn server_ranges(text: &str, answer: &Value) -> Vec<HighlightRange> {
    let position = |at: &Value| -> Option<usize> {
        let position =
            lsp_types::Position { line: at["line"].as_u64()? as u32, character: at["character"].as_u64()? as u32 };
        Some(char_offset(text, zen_lsp::position::offset(text, position).unwrap_or(text.len())))
    };
    let mut ranges: Vec<HighlightRange> = answer
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|highlight| {
            let start = position(&highlight["range"]["start"])?;
            let end = position(&highlight["range"]["end"])?.max(start);
            Some(HighlightRange { start, end, kind: HighlightKind::from_protocol(&highlight["kind"]) })
        })
        .filter(|range| range.end > range.start)
        .collect();
    ranges.sort_by_key(|range| (range.start, range.end));
    ranges.dedup_by_key(|range| (range.start, range.end));
    ranges
}

pub(crate) fn server_highlights(session: &Session, path: &Path) -> Vec<HighlightRange> {
    let Some(highlights) = session.state::<Highlights>() else { return Vec::new() };
    [&highlights.caret, &highlights.pinned]
        .into_iter()
        .flatten()
        .filter(|slot| slot.key.path == path && slot.state.source == HighlightSource::Server && !slot.state.pending)
        .flat_map(|slot| slot.state.ranges.iter().copied())
        .collect()
}

fn with_highlights<T>(session: &mut Session, f: impl FnOnce(&mut Highlights, &mut Session) -> T) -> T {
    let mut highlights = std::mem::take(session.state_mut::<Highlights>());
    let result = f(&mut highlights, session);
    session.set_state(highlights);
    result
}

fn at_place(session: &mut Session, params: &HighlightParams) -> Result<HighlightsState, CommandError> {
    let key = key_of(session, params)?;
    let wait = params.lookup().wait();
    with_highlights(session, |highlights, session| {
        let reuse = highlights
            .caret
            .as_ref()
            .is_some_and(|slot| slot.key == key && !server_came(session.state_mut::<Languages>(), slot));
        if !reuse {
            let state = start(session, LookupKind::Highlights, &key)?;
            highlights.caret = Some(Slot { key, state });
        }
        let slot = highlights.caret.as_mut().expect("just set");
        poll(session, LookupKind::Highlights, slot, wait);
        Ok(slot.state.clone())
    })
}

fn pinned(session: &mut Session, wait: Duration) -> HighlightsState {
    with_highlights(session, |highlights, session| {
        let changed = highlights.pinned.as_ref().is_some_and(|slot| {
            let workbench = session.state::<Workbench>();
            let version = workbench.map_or(0, |workbench| version_and_selection(workbench, &slot.key.path, true).0);
            version != slot.key.version
        });
        if changed {
            highlights.pinned = None;
            session.state_mut::<Languages>().end_lookup(LookupKind::PinnedHighlights);
        }
        let Some(slot) = highlights.pinned.as_mut() else { return HighlightsState::default() };
        poll(session, LookupKind::PinnedHighlights, slot, wait);
        slot.state.clone()
    })
}

fn pin(session: &mut Session, params: &HighlightParams, toggle: bool) -> Result<HighlightsState, CommandError> {
    let key = key_of(session, params)?;
    let current = pinned(session, Duration::ZERO);
    let inside = current.path.as_ref() == Some(&key.path)
        && current.ranges.iter().any(|range| (range.start..=range.end).contains(&key.offset));
    if toggle && inside {
        clear(session);
        return Ok(HighlightsState::default());
    }
    let wait = params.lookup().wait();
    with_highlights(session, |highlights, session| {
        let state = start(session, LookupKind::PinnedHighlights, &key)?;
        let slot = highlights.pinned.insert(Slot { key, state });
        poll(session, LookupKind::PinnedHighlights, slot, wait);
        Ok(slot.state.clone())
    })
}

pub fn clear(session: &mut Session) -> bool {
    let had = session.state_mut::<Highlights>().pinned.take().is_some();
    session.state_mut::<Languages>().end_lookup(LookupKind::PinnedHighlights);
    had
}

fn has_text_document(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

pub struct GetHighlights;

impl Command for GetHighlights {
    type Params = HighlightParams;
    type Output = HighlightsState;
    const ID: &'static str = "editor.highlights.get";
    const TITLE: &'static str = "Highlighted Usages at the Caret";
    const DESCRIPTION: &'static str = "The usages of the symbol at the caret (or at an offset) in a file, as the editor \
         highlights them: from the language server's textDocument/documentHighlight, each range read, write or text, \
         else the name's whole-word occurrences for a file no server highlights; a selection gives its text's \
         occurrences. Asks once per place and text; call again (or pass waitMs) while pending.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: HighlightParams) -> Result<HighlightsState, CommandError> {
        at_place(session, &params)
    }
}

pub struct PinUsages<const TOGGLE: bool>;

impl<const TOGGLE: bool> Command for PinUsages<TOGGLE> {
    type Params = HighlightParams;
    type Output = HighlightsState;
    const ID: &'static str = if TOGGLE { "editor.highlightUsages.toggle" } else { "navigate.usages.findInFile" };
    const TITLE: &'static str = if TOGGLE { "Highlight Usages in File" } else { "Find Usages in File" };
    const DESCRIPTION: &'static str = if TOGGLE {
        "Pins the highlight of the usages of the symbol at the caret (or of the selected text) in the file until \
         Escape or an edit; on one of the pinned usages it unpins them. editor.highlightUsages.list reads them."
    } else {
        "Highlights the usages of the symbol at the caret (or of the selected text) in the file and keeps them until \
         Escape or an edit; ⌃⌥↓ and ⌃⌥↑ step through them. editor.highlightUsages.list reads them."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: HighlightParams) -> Result<HighlightsState, CommandError> {
        pin(session, &params, TOGGLE)
    }
}

pub struct ListPinned;

impl Command for ListPinned {
    type Params = WaitParams;
    type Output = HighlightsState;
    const ID: &'static str = "editor.highlightUsages.list";
    const TITLE: &'static str = "Pinned Usages";
    const DESCRIPTION: &'static str = "The usages pinned by Highlight Usages in File or Find Usages in File, waiting up \
         to waitMs for the server's answer; nothing once they are cleared or their file changed.";

    fn run(&self, session: &mut Session, params: WaitParams) -> Result<HighlightsState, CommandError> {
        Ok(pinned(session, params.wait()))
    }
}

pub struct ClearPinned;

impl Command for ClearPinned {
    type Params = NoParams;
    type Output = bool;
    const ID: &'static str = "editor.highlightUsages.clear";
    const TITLE: &'static str = "Clear Highlighted Usages";
    const DESCRIPTION: &'static str = "Unpins the usages of Highlight Usages in File or Find Usages in File, as Escape does; answers whether there were some.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<bool, CommandError> {
        Ok(clear(session))
    }
}

pub struct StepUsages<const NEXT: bool>;

impl<const NEXT: bool> Command for StepUsages<NEXT> {
    type Params = NoParams;
    type Output = Option<HighlightRange>;
    const ID: &'static str = if NEXT { "editor.highlightUsages.next" } else { "editor.highlightUsages.previous" };
    const TITLE: &'static str = if NEXT { "Next Highlighted Usage" } else { "Previous Highlighted Usage" };
    const DESCRIPTION: &'static str = if NEXT {
        "Moves the caret to the next highlighted usage in the active file (the pinned ones, else those at the caret), \
         going round to the first."
    } else {
        "Moves the caret to the previous highlighted usage in the active file (the pinned ones, else those at the \
         caret), going round to the last."
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Option<HighlightRange>, CommandError> {
        let (path, caret) = {
            let document = session
                .state::<Workbench>()
                .and_then(Workbench::active_document)
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
            (document.path().to_owned(), document.selections().primary().head)
        };
        let pinned = pinned(session, Duration::ZERO);
        let ranges = if pinned.path.as_ref() == Some(&path) && !pinned.ranges.is_empty() {
            pinned.ranges
        } else {
            at_place(session, &HighlightParams::default())?.ranges
        };
        let Some(target) = step(&ranges, caret, NEXT) else { return Ok(None) };
        let origin = crate::history::caret(session);
        if let Some(document) = session.state_mut::<Workbench>().active_document_mut() {
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(target.start)));
        }
        crate::history::jumped(session, origin);
        Ok(Some(target))
    }
}

fn step(ranges: &[HighlightRange], caret: usize, next: bool) -> Option<HighlightRange> {
    let at = ranges.iter().find(|range| (range.start..=range.end).contains(&caret)).map_or(caret, |range| range.start);
    if next {
        ranges.iter().find(|range| range.start > at).or_else(|| ranges.first())
    } else {
        ranges.iter().rev().find(|range| range.start < at).or_else(|| ranges.last())
    }
    .copied()
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetHighlights)?;
    registry.register(PinUsages::<true>)?;
    registry.register(PinUsages::<false>)?;
    registry.register(ListPinned)?;
    registry.register(ClearPinned)?;
    registry.register(StepUsages::<true>)?;
    registry.register(StepUsages::<false>)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use zen_lsp::fake::Reply;

    use super::{HighlightKind, HighlightRange, step};
    use crate::language::testing::fixture;

    const MAIN: &str = "package main\n\n// ёж counts\nfunc main() {\n\tёж := 1\n\tёж = ёж + 1\n\tprintln(ёж)\n}\n";

    fn chars(text: &str, needle: &str, nth: usize) -> usize {
        text[..text.match_indices(needle).nth(nth).unwrap().0].chars().count()
    }

    fn highlight(text: &str, needle: &str, nth: usize, kind: u64) -> Value {
        let at = text.match_indices(needle).nth(nth).unwrap().0;
        let start = zen_lsp::position::position(text, at);
        let end = zen_lsp::position::position(text, at + needle.len());
        json!({ "range": { "start": start, "end": end }, "kind": kind })
    }

    fn ranges(state: &Value) -> Vec<(u64, &str)> {
        state["ranges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|range| (range["start"].as_u64().unwrap(), range["kind"].as_str().unwrap()))
            .collect()
    }

    fn answer() -> Value {
        json!([
            highlight(MAIN, "ёж", 3, 2),
            highlight(MAIN, "ёж", 1, 3),
            highlight(MAIN, "ёж", 2, 3),
            highlight(MAIN, "ёж", 4, 2),
            highlight(MAIN, "ёж", 4, 2),
        ])
    }

    #[test]
    fn the_server_highlights_reads_and_writes() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.server.on("textDocument/documentHighlight", answer());
        fixture.open("main.go");
        let at = chars(MAIN, "ёж", 2) + 1;
        let state = fixture.dispatch("editor.highlights.get", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        let sent = &fixture.server.requests("textDocument/documentHighlight")[0];
        assert_eq!(sent["position"]["line"], 5);
        assert_eq!(state["source"], "server");
        assert_eq!(state["pending"], false);
        let expected: Vec<(u64, &str)> = vec![
            (chars(MAIN, "ёж", 1) as u64, "write"),
            (chars(MAIN, "ёж", 2) as u64, "write"),
            (chars(MAIN, "ёж", 3) as u64, "read"),
            (chars(MAIN, "ёж", 4) as u64, "read"),
        ];
        assert_eq!(ranges(&state), expected, "the comment's ёж is not a usage: {state}");
        assert_eq!(state["ranges"][0]["end"], chars(MAIN, "ёж", 1) + 2);

        let again = fixture.dispatch("editor.highlights.get", json!({ "offset": at })).unwrap();
        assert_eq!(again, state);
        assert_eq!(fixture.server.requests("textDocument/documentHighlight").len(), 1);
    }

    #[test]
    fn without_a_server_or_with_a_failing_one_the_text_is_matched() {
        let mut fixture = fixture(&[("main.go", MAIN), ("notes.txt", "ёж and ёжик, ёж\n")]);
        fixture.server.reply("textDocument/documentHighlight", Reply::Error { code: -32603, message: "no".into() });
        fixture.open("main.go");
        let at = chars(MAIN, "ёж", 2);
        let state = fixture.dispatch("editor.highlights.get", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        assert_eq!(state["source"], "text", "{state}");
        assert_eq!(ranges(&state).len(), 5, "the comment's too");

        fixture.dispatch("file.document.open", json!({ "path": fixture.path("notes.txt") })).unwrap();
        let state = fixture.dispatch("editor.highlights.get", json!({ "offset": 1 })).unwrap();
        assert_eq!(state["source"], "text");
        assert_eq!(ranges(&state), [(0, "text"), (13, "text")], "whole words");
        let none = fixture.dispatch("editor.highlights.get", json!({ "offset": 12 })).unwrap();
        assert_eq!((none["source"].clone(), none["ranges"].clone()), (json!("none"), json!([])));
    }

    #[test]
    fn a_server_that_does_not_highlight_is_not_asked() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.server.set_capabilities(json!({ "textDocumentSync": 2, "documentHighlightProvider": false }));
        fixture.open("main.go");
        let state = fixture.dispatch("editor.highlights.get", json!({ "offset": chars(MAIN, "ёж", 1) })).unwrap();
        assert_eq!(state["source"], "text");
        assert!(fixture.server.requests("textDocument/documentHighlight").is_empty());
    }

    #[test]
    fn a_selection_highlights_its_text_anywhere() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.server.on("textDocument/documentHighlight", answer());
        fixture.open("main.go");
        let from = chars(MAIN, "ain", 0);
        fixture
            .dispatch("editor.selection.set", json!({ "selections": [{ "anchor": from, "head": from + 3 }] }))
            .unwrap();
        let state = fixture.dispatch("editor.highlights.get", json!({})).unwrap();
        assert_eq!(state["source"], "selection");
        assert_eq!(ranges(&state), [(from as u64, "text"), (chars(MAIN, "ain", 1) as u64, "text")]);
        assert!(fixture.server.requests("textDocument/documentHighlight").is_empty());
    }

    #[test]
    fn pinned_usages_toggle_step_and_clear() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.server.on("textDocument/documentHighlight", answer());
        fixture.open("main.go");
        let first = chars(MAIN, "ёж", 1);
        fixture
            .dispatch("editor.selection.set", json!({ "selections": [{ "anchor": first, "head": first }] }))
            .unwrap();
        let pinned = fixture.dispatch("editor.highlightUsages.toggle", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(ranges(&pinned).len(), 4, "{pinned}");
        assert_eq!(fixture.dispatch("editor.highlightUsages.list", json!({})).unwrap(), pinned);

        let caret = |fixture: &mut crate::language::testing::Fixture| {
            fixture.dispatch("editor.selection.get", json!({})).unwrap()[0]["head"].as_u64().unwrap() as usize
        };
        let next = fixture.dispatch("editor.highlightUsages.next", json!({})).unwrap();
        assert_eq!(next["start"], chars(MAIN, "ёж", 2));
        assert_eq!(caret(&mut fixture), chars(MAIN, "ёж", 2));
        fixture.dispatch("editor.highlightUsages.next", json!({})).unwrap();
        fixture.dispatch("editor.highlightUsages.next", json!({})).unwrap();
        let round = fixture.dispatch("editor.highlightUsages.next", json!({})).unwrap();
        assert_eq!(round["start"], first, "round to the first");
        let back = fixture.dispatch("editor.highlightUsages.previous", json!({})).unwrap();
        assert_eq!(back["start"], chars(MAIN, "ёж", 4));

        fixture.dispatch("navigate.usages.findInFile", json!({ "waitMs": 5000 })).unwrap();
        let still = fixture.dispatch("navigate.usages.findInFile", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(ranges(&still).len(), 4);
        let off = fixture.dispatch("editor.highlightUsages.toggle", json!({})).unwrap();
        assert_eq!(off["ranges"], json!([]));
        assert_eq!(fixture.dispatch("editor.highlightUsages.list", json!({})).unwrap()["path"], Value::Null);

        fixture.dispatch("editor.highlightUsages.toggle", json!({ "waitMs": 5000 })).unwrap();
        fixture.dispatch("editor.caret.keepPrimary", json!({})).unwrap();
        assert_eq!(fixture.dispatch("editor.highlightUsages.list", json!({})).unwrap()["ranges"], json!([]));
        fixture.dispatch("editor.highlightUsages.toggle", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(fixture.dispatch("editor.highlightUsages.clear", json!({})).unwrap(), json!(true));
        assert_eq!(fixture.dispatch("editor.highlightUsages.clear", json!({})).unwrap(), json!(false));
        fixture.dispatch("editor.highlightUsages.toggle", json!({ "waitMs": 5000 })).unwrap();
        fixture.dispatch("editor.edit.insert", json!({ "text": "x" })).unwrap();
        assert_eq!(fixture.dispatch("editor.highlightUsages.list", json!({})).unwrap()["ranges"], json!([]));
    }

    #[test]
    fn a_slow_answer_is_pending_and_a_moved_caret_drops_it() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.server.reply("textDocument/documentHighlight", Reply::Delayed(answer(), Duration::from_millis(200)));
        fixture.open("main.go");
        let at = chars(MAIN, "ёж", 1);
        let state = fixture.dispatch("editor.highlights.get", json!({ "offset": at })).unwrap();
        assert_eq!((state["pending"].clone(), state["ranges"].clone()), (json!(true), json!([])));
        let state = fixture.dispatch("editor.highlights.get", json!({ "offset": at, "waitMs": 5000 })).unwrap();
        assert_eq!(state["pending"], false);
        assert_eq!(ranges(&state).len(), 4);

        fixture.server.reply("textDocument/documentHighlight", Reply::Hold);
        let moved = fixture.dispatch("editor.highlights.get", json!({ "offset": at + 1 })).unwrap();
        assert_eq!(moved["pending"], true, "a new place asks again");
        fixture.dispatch("editor.edit.insert", json!({ "text": "x" })).unwrap();
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let outdated = fixture.dispatch("editor.highlights.get", json!({ "offset": at + 1 })).unwrap();
        assert_eq!(outdated["pending"], true, "the edited text asks again: {outdated}");
    }

    #[test]
    fn stepping_goes_round_the_ends() {
        let range = |start: usize| HighlightRange { start, end: start + 2, kind: HighlightKind::Read };
        let ranges = [range(3), range(10), range(20)];
        assert_eq!(step(&ranges, 4, true), Some(range(10)), "from inside the first");
        assert_eq!(step(&ranges, 21, true), Some(range(3)));
        assert_eq!(step(&ranges, 0, false), Some(range(20)));
        assert_eq!(step(&ranges, 11, false), Some(range(3)));
        assert_eq!(step(&ranges, 15, false), Some(range(10)), "from between two");
        assert_eq!(step(&[], 0, true), None);
    }
}
