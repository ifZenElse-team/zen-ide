use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_editor::{Selection, Selections, Structure, StructureOptions, Token, TokenKind, double_click_word, hump_at};
use zen_i18n::t;
use zen_lsp::lsp_types::{Position, SelectionRangeProviderCapability};
use zen_lsp::position::LineStarts;
use zen_text::Text;

use crate::brackets::Highlighters;
use crate::editing::{SelectionInfo, camel_humps, describe_selections};
use crate::language::{MAX_WAIT, byte_offset, char_offset, request_in, with_languages};
use crate::{Document, Workbench};

pub const SELECT_WHITESPACE: &str = "editor.extendSelection.whitespace";

pub const DEFAULT_WAIT: Duration = Duration::from_millis(300);

const SELECTION_RANGE: &str = "textDocument/selectionRange";

const PROSE: [&str; 6] = ["txt", "text", "md", "markdown", "rst", "adoc"];

/// The parameters of `editor.selection.extend` and `editor.selection.shrink`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepParams {
    /// How long to wait for the language server's selection ranges, in milliseconds (at most 10 000; 300 by default; 0 uses the text alone).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

impl StepParams {
    fn wait(&self) -> Duration {
        self.wait_ms.map_or(DEFAULT_WAIT, Duration::from_millis).min(MAX_WAIT)
    }
}

#[derive(Default)]
struct Series {
    path: PathBuf,
    version: u64,
    after: Vec<(usize, usize)>,
    stack: Vec<Selections>,
}

fn shape(selections: &Selections) -> Vec<(usize, usize)> {
    selections.iter().map(|selection| (selection.anchor, selection.head)).collect()
}

fn text_document(session: &Session) -> Option<&Document> {
    session.state::<Workbench>().and_then(Workbench::active_document).filter(|document| document.image().is_none())
}

fn no_document() -> CommandError {
    CommandError::new(t!("workbench-error-no-document"))
}

fn char_ranges(text: &str, spans: Vec<zen_syntax::Span>) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = 0;
    let mut byte = 0;
    let mut indices = text.char_indices().peekable();
    let mut to_char = |target: usize| {
        while byte < target {
            match indices.next() {
                Some((at, c)) => {
                    byte = at + c.len_utf8();
                    chars += 1;
                }
                None => break,
            }
        }
        chars
    };
    for span in spans {
        let kind = if span.kind.is_comment() {
            TokenKind::Comment
        } else if span.kind.is_string() {
            TokenKind::String
        } else {
            continue;
        };
        let start = to_char(span.range.start);
        let end = to_char(span.range.end);
        tokens.push(Token { range: start..end, kind });
    }
    let mut merged: Vec<Token> = Vec::with_capacity(tokens.len());
    for token in tokens {
        match merged.last_mut() {
            Some(last) if last.kind == token.kind && last.range.end == token.range.start => {
                last.range.end = token.range.end
            }
            _ => merged.push(token),
        }
    }
    merged
}

pub(crate) fn structure(session: &Session, document: &Document) -> Structure {
    let text = document.text();
    let highlighting = session.state::<Highlighters>().and_then(|highlighters| highlighters.highlighting_of(document));
    let spans = highlighting.as_ref().map_or_else(Vec::new, |highlighting| {
        highlighting.lock().unwrap_or_else(PoisonError::into_inner).spans(text, 0..text.len())
    });
    let extension = document.path().extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase);
    let plain = match extension {
        Some(extension) => PROSE.contains(&extension.as_str()),
        None => highlighting.is_none(),
    };
    let options = StructureOptions { whitespace: session.settings().get_or(SELECT_WHITESPACE, false), plain };
    Structure::new(text, char_ranges(text, spans), options)
}

fn chain(text: &str, lines: &LineStarts, mut node: &Value) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    loop {
        let point = |key: &str| {
            let position: Position = serde_json::from_value(node["range"][key].clone()).ok()?;
            lines.offset(text, position).map(|byte| char_offset(text, byte))
        };
        if let (Some(start), Some(end)) = (point("start"), point("end"))
            && start <= end
        {
            ranges.push(start..end);
        }
        match node.get("parent") {
            Some(parent) if parent.is_object() => node = parent,
            _ => return ranges,
        }
    }
}

pub(crate) fn server_ranges(
    session: &mut Session,
    path: &Path,
    offsets: &[usize],
    wait: Duration,
) -> Vec<Vec<Range<usize>>> {
    let none = vec![Vec::new(); offsets.len()];
    if wait.is_zero() {
        return none;
    }
    with_languages(session, |languages, workbench| {
        let offers = languages
            .client_for(path)
            .and_then(|client| client.capabilities())
            .and_then(|capabilities| capabilities.selection_range_provider.as_ref())
            .is_some_and(|provider| !matches!(provider, SelectionRangeProviderCapability::Simple(false)));
        if !offers {
            return none.clone();
        }
        let request = request_in(languages, workbench, path, SELECTION_RANGE, |uri, text| {
            let positions: Vec<Position> =
                offsets.iter().map(|&offset| zen_lsp::position::position(text, byte_offset(text, offset))).collect();
            json!({ "textDocument": { "uri": uri }, "positions": positions })
        });
        let Ok(request) = request else { return none.clone() };
        let answer = request.pending.wait(wait);
        if answer.is_err()
            && let Some(client) = languages.client_for(path)
        {
            client.cancel(&request.pending);
        }
        let (Ok(Value::Array(items)), Some(text)) = (answer, languages.synced_text(path)) else { return none.clone() };
        let lines = LineStarts::new(text);
        let mut ranges: Vec<Vec<Range<usize>>> = items.iter().map(|item| chain(text, &lines, item)).collect();
        ranges.resize(offsets.len(), Vec::new());
        ranges
    })
}

fn apply(session: &mut Session, path: &Path, selections: Selections, stack: Vec<Selections>) -> Vec<SelectionInfo> {
    let workbench = session.state_mut::<Workbench>();
    let Some(document) = workbench.active_document_mut().filter(|document| document.path() == path) else {
        return Vec::new();
    };
    document.set_selections(selections);
    let after = shape(document.selections());
    let described = describe_selections(document);
    let version = document.version();
    session.set_state(Series { path: path.to_owned(), version, after, stack });
    described
}

fn current_series(session: &mut Session, document_path: &Path, version: u64, now: &Selections) -> Vec<Selections> {
    let series = std::mem::take(session.state_mut::<Series>());
    if series.path == document_path && series.version == version && series.after == shape(now) {
        series.stack
    } else {
        Vec::new()
    }
}

fn hump_inside_word(text: &Text, selection: &Selection) -> Option<Range<usize>> {
    if !selection.is_empty() {
        return None;
    }
    let hump = hump_at(text, selection.head);
    let word = double_click_word(text, selection.head, false);
    (!hump.is_empty() && hump != word).then_some(hump)
}

pub struct ExtendSelection;

impl Command for ExtendSelection {
    type Params = StepParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.selection.extend";
    const TITLE: &'static str = "Extend Selection";
    const DESCRIPTION: &'static str = "Grows every selection of the active file to the next enclosing syntax unit that is \
         larger: the camel hump at the caret when the camelHumps setting is on, the word at the caret, a string's content, the string, an argument, the argument list without and with \
         its brackets, the expression, the statement, a block's content, the block, the declaration with its doc comment, \
         the file; in comments and text by word, sentence, paragraph and the whole comment. The language server's \
         selection ranges join in when it answers within waitMs. Each caret grows on its own; \
         editor.selection.shrink walks back. Returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        text_document(session).is_some()
    }

    fn run(&self, session: &mut Session, params: StepParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = text_document(session).ok_or_else(no_document)?;
        let path = document.path().to_owned();
        let version = document.version();
        let now = document.selections().clone();
        let starts: Vec<usize> = now.iter().map(|selection| selection.range().start).collect();
        let mut stack = current_series(session, &path, version, &now);
        let extra = server_ranges(session, &path, &starts, params.wait());
        let humps = camel_humps(session.settings());
        let document = text_document(session).ok_or_else(no_document)?;
        let units = structure(session, document);
        let grown: Vec<Selection> = now
            .iter()
            .zip(&extra)
            .map(|(selection, extra)| {
                if humps && let Some(hump) = hump_inside_word(document.snapshot(), selection) {
                    return Selection::new(hump.start, hump.end);
                }
                match units.extend(selection.range(), extra) {
                    Some(range) => Selection::new(range.start, range.end),
                    None => *selection,
                }
            })
            .collect();
        let grown = Selections::from_vec(grown, now.primary_index());
        if shape(&grown) != shape(&now) {
            stack.push(now);
        }
        Ok(apply(session, &path, grown, stack))
    }
}

pub struct ShrinkSelection;

impl Command for ShrinkSelection {
    type Params = StepParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.selection.shrink";
    const TITLE: &'static str = "Shrink Selection";
    const DESCRIPTION: &'static str = "Takes back the last editor.selection.extend of the active file, step by step, as \
         long as the carets and the text have not changed since; otherwise shrinks every selection to the largest syntax \
         unit inside it. Returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        text_document(session).is_some()
    }

    fn run(&self, session: &mut Session, params: StepParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = text_document(session).ok_or_else(no_document)?;
        let path = document.path().to_owned();
        let version = document.version();
        let now = document.selections().clone();
        let mut stack = current_series(session, &path, version, &now);
        if let Some(previous) = stack.pop() {
            return Ok(apply(session, &path, previous, stack));
        }
        let starts: Vec<usize> = now.iter().map(|selection| selection.range().start).collect();
        let extra = server_ranges(session, &path, &starts, params.wait());
        let document = text_document(session).ok_or_else(no_document)?;
        let units = structure(session, document);
        let shrunk: Vec<Selection> = now
            .iter()
            .zip(&extra)
            .map(|(selection, extra)| match units.shrink(selection.range(), extra) {
                Some(range) => Selection::new(range.start, range.end),
                None => *selection,
            })
            .collect();
        Ok(apply(session, &path, Selections::from_vec(shrunk, now.primary_index()), Vec::new()))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ExtendSelection)?;
    registry.register(ShrinkSelection)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use zen_syntax::{HighlightKind, Highlighter, Span};

    use super::*;
    use crate::language::testing::fixture;

    struct Lexer;

    impl Highlighter for Lexer {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<Span> {
            let mut spans = Vec::new();
            let bytes = text.as_bytes();
            let mut at = 0;
            while at < bytes.len() {
                let start = at;
                if text[at..].starts_with("//") {
                    at = text[at..].find('\n').map_or(text.len(), |end| at + end);
                    spans.push(Span { range: start..at, kind: HighlightKind::Comment });
                } else if bytes[at] == b'"' {
                    at = text[at + 1..].find('"').map_or(text.len(), |end| at + end + 2);
                    spans.push(Span { range: start..at, kind: HighlightKind::String });
                } else {
                    at += 1;
                }
            }
            spans
        }
    }

    const GO: &str = "package main\n\n// Run prints «два» words.\nfunc Run() {\n\tfmt.Println(\"привет, мир\", name)\n\tother(x)\n}\n";

    fn session() -> (tempfile::TempDir, Registry, Session) {
        zen_i18n::register(crate::LOCALES);
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("main.go"), GO).unwrap();
        std::fs::write(folder.path().join("notes.txt"), "One two. Three four.\n\nFive.").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let lexer: Arc<dyn Highlighter> = Arc::new(Lexer);
        session
            .state_mut::<Highlighters>()
            .set_source(Arc::new(move |name| Lexer.handles(name).then(|| Arc::clone(&lexer))));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        let path = folder.path().join("main.go");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        (folder, registry, session)
    }

    fn at(text: &str, mark: &str) -> usize {
        text[..text.find(mark).unwrap()].chars().count()
    }

    fn selected(value: &Value) -> Vec<String> {
        value.as_array().unwrap().iter().map(|selection| selection["text"].as_str().unwrap().to_owned()).collect()
    }

    fn run(registry: &Registry, session: &mut Session, command: &str) -> Vec<String> {
        selected(&registry.dispatch(session, command, json!({ "waitMs": 0 })).unwrap())
    }

    #[test]
    fn extend_grows_by_syntax_units_and_shrink_walks_the_series_back() {
        let (_folder, registry, mut session) = session();
        let caret = at(GO, "мир");
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] })).unwrap();
        let mut steps = Vec::new();
        for _ in 0..7 {
            steps.push(run(&registry, &mut session, "editor.selection.extend")[0].clone());
        }
        assert_eq!(
            steps,
            [
                "мир",
                "привет, мир",
                "\"привет, мир\"",
                "\"привет, мир\", name",
                "(\"привет, мир\", name)",
                "fmt.Println(\"привет, мир\", name)",
                "fmt.Println(\"привет, мир\", name)\n\tother(x)",
            ]
        );
        assert_eq!(run(&registry, &mut session, "editor.selection.shrink"), ["fmt.Println(\"привет, мир\", name)"]);
        assert_eq!(run(&registry, &mut session, "editor.selection.shrink"), ["(\"привет, мир\", name)"]);
        for _ in 0..5 {
            run(&registry, &mut session, "editor.selection.shrink");
        }
        let back = registry.dispatch(&mut session, "editor.selection.get", json!({})).unwrap();
        assert_eq!((back[0]["anchor"].as_u64(), back[0]["head"].as_u64()), (Some(caret as u64), Some(caret as u64)));
        assert_eq!(run(&registry, &mut session, "editor.selection.extend"), ["мир"], "a new series");
    }

    #[test]
    fn with_camel_humps_on_the_first_step_is_the_hump_and_shrink_walks_it_back() {
        let (folder, registry, mut session) = session();
        let text = "package main\n\nfunc f() {\n\tpay(totalAmount, max_retry_count)\n}\n";
        let path = folder.path().join("camel.go");
        std::fs::write(&path, text).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let caret = at(text, "Amount");
        let place = |session: &mut Session, head: usize| {
            registry.dispatch(session, "editor.selection.set", json!({ "selections": [{ "head": head }] })).unwrap();
        };
        place(&mut session, caret);
        assert_eq!(run(&registry, &mut session, "editor.selection.extend"), ["totalAmount"], "off: the whole word");
        session.settings_mut().set(crate::editing::CAMEL_HUMPS, true).unwrap();
        place(&mut session, caret);
        let steps: Vec<String> =
            (0..3).map(|_| run(&registry, &mut session, "editor.selection.extend")[0].clone()).collect();
        assert_eq!(steps, ["Amount", "totalAmount", "totalAmount, max_retry_count"]);
        let back: Vec<String> =
            (0..2).map(|_| run(&registry, &mut session, "editor.selection.shrink")[0].clone()).collect();
        assert_eq!(back, ["totalAmount", "Amount"]);
        place(&mut session, at(text, "retry") + 2);
        let steps: Vec<String> =
            (0..2).map(|_| run(&registry, &mut session, "editor.selection.extend")[0].clone()).collect();
        assert_eq!(steps, ["retry", "max_retry_count"], "a snake_case segment");
        place(&mut session, at(text, "pay") + 1);
        assert_eq!(run(&registry, &mut session, "editor.selection.extend"), ["pay"], "a one-hump word is one step");
    }

    #[test]
    fn comments_go_by_sentence_and_plain_text_by_paragraph() {
        let (folder, registry, mut session) = session();
        let caret = at(GO, "words");
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] })).unwrap();
        let steps: Vec<String> =
            (0..3).map(|_| run(&registry, &mut session, "editor.selection.extend")[0].clone()).collect();
        assert_eq!(steps, ["words", "Run prints «два» words.", "// Run prints «два» words."]);
        let notes = folder.path().join("notes.txt");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": notes })).unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 10 }] })).unwrap();
        let steps: Vec<String> =
            (0..4).map(|_| run(&registry, &mut session, "editor.selection.extend")[0].clone()).collect();
        assert_eq!(steps, ["Three", "Three four.", "One two. Three four.", "One two. Three four.\n\nFive."]);
    }

    #[test]
    fn every_caret_extends_on_its_own_and_a_move_or_an_edit_ends_the_series() {
        let (_folder, registry, mut session) = session();
        let carets = json!({ "selections": [{ "head": at(GO, "name)") }, { "head": at(GO, "x)") }] });
        registry.dispatch(&mut session, "editor.selection.set", carets.clone()).unwrap();
        assert_eq!(run(&registry, &mut session, "editor.selection.extend"), ["name", "x"]);
        assert_eq!(run(&registry, &mut session, "editor.selection.extend"), ["\"привет, мир\", name", "(x)"]);
        assert_eq!(run(&registry, &mut session, "editor.selection.extend"), ["(\"привет, мир\", name)", "other(x)"]);
        registry
            .dispatch(
                &mut session,
                "editor.selection.set",
                json!({ "selections": [{ "anchor": at(GO, "Run()"), "head": at(GO, "() {") }] }),
            )
            .unwrap();
        assert_eq!(
            run(&registry, &mut session, "editor.selection.shrink"),
            ["Run"],
            "no series: nothing smaller than the word"
        );
        registry.dispatch(&mut session, "editor.selection.set", carets).unwrap();
        run(&registry, &mut session, "editor.selection.extend");
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "z" })).unwrap();
        let edited = registry.dispatch(&mut session, "editor.selection.get", json!({})).unwrap();
        let carets: Vec<(u64, u64)> = edited
            .as_array()
            .unwrap()
            .iter()
            .map(|selection| (selection["anchor"].as_u64().unwrap(), selection["head"].as_u64().unwrap()))
            .collect();
        assert!(carets.iter().all(|(anchor, head)| anchor == head));
        let shrunk = registry.dispatch(&mut session, "editor.selection.shrink", json!({ "waitMs": 0 })).unwrap();
        let after: Vec<(u64, u64)> = shrunk
            .as_array()
            .unwrap()
            .iter()
            .map(|selection| (selection["anchor"].as_u64().unwrap(), selection["head"].as_u64().unwrap()))
            .collect();
        assert_eq!(after, carets, "the edit ended the series: nothing to walk back");
    }

    #[test]
    fn a_manual_selection_shrinks_to_the_largest_unit_inside_it() {
        let (_folder, registry, mut session) = session();
        let start = at(GO, "{\n\tfmt");
        let end = at(GO, "}\n") + 1;
        registry
            .dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "anchor": start, "head": end }] }))
            .unwrap();
        assert_eq!(
            run(&registry, &mut session, "editor.selection.shrink"),
            ["fmt.Println(\"привет, мир\", name)\n\tother(x)"]
        );
    }

    #[test]
    fn whitespace_is_selected_when_the_advanced_setting_is_on() {
        let (_folder, registry, mut session) = session();
        let caret = at(GO, " name") + 1;
        registry
            .dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": caret - 1 }] }))
            .unwrap();
        assert_eq!(
            run(&registry, &mut session, "editor.selection.extend"),
            ["\"привет, мир\", name"],
            "off: the arguments"
        );
        session.settings_mut().set(SELECT_WHITESPACE, true).unwrap();
        registry
            .dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": caret - 1 }] }))
            .unwrap();
        assert_eq!(run(&registry, &mut session, "editor.selection.extend"), [" "]);
    }

    #[test]
    fn the_language_servers_selection_ranges_join_in() {
        let text = "package main\n\nfunc f() {\n\tx := a + b*c\n}\n";
        let mut fixture = fixture(&[("main.go", text)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "selectionRangeProvider": true
        }));
        let range = |line: u32, start: u32, end: u32| json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } });
        fixture.server.on(
            SELECTION_RANGE,
            json!([{
                "range": range(3, 10, 11),
                "parent": { "range": range(3, 10, 13), "parent": { "range": range(3, 6, 13) } }
            }]),
        );
        fixture.open("main.go");
        let caret = at(text, "b*c");
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": caret }] })).unwrap();
        let mut steps = Vec::new();
        for _ in 0..3 {
            let answer = fixture.dispatch_once_running("editor.selection.extend", json!({ "waitMs": 5000 })).unwrap();
            steps.push(selected(&answer)[0].clone());
        }
        assert_eq!(steps, ["b", "b*c", "a + b*c"]);
        let sent = &fixture.server.requests(SELECTION_RANGE)[0];
        assert_eq!(sent["positions"], json!([{ "line": 3, "character": 10 }]));
    }

    #[test]
    fn a_late_answer_is_dropped_and_never_moves_the_selection() {
        let text = "package main\n\nfunc f() {\n\tx := a + b*c\n}\n";
        let mut fixture = fixture(&[("main.go", text)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "selectionRangeProvider": true
        }));
        let range = |line: u32, start: u32, end: u32| json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } });
        let late = json!([{ "range": range(3, 10, 11), "parent": { "range": range(3, 10, 13) } }]);
        fixture.server.reply(SELECTION_RANGE, zen_lsp::fake::Reply::Delayed(late, Duration::from_millis(300)));
        fixture.open("main.go");
        let caret = at(text, "b*c");
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": caret }] })).unwrap();
        let first = fixture.dispatch_once_running("editor.selection.extend", json!({ "waitMs": 20 })).unwrap();
        assert_eq!(selected(&first), ["b"], "the text's unit when the server is late");
        std::thread::sleep(Duration::from_millis(400));
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let now = fixture.dispatch("editor.selection.get", json!({})).unwrap();
        assert_eq!(selected(&now), ["b"], "the late answer does not move the selection");
        fixture
            .server
            .on(SELECTION_RANGE, json!([{ "range": range(3, 10, 13), "parent": { "range": range(3, 6, 13) } }]));
        let second = fixture.dispatch("editor.selection.extend", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(selected(&second), ["b*c"], "the next step reads the answer to its own request");
    }
}
