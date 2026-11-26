use std::ops::Range;
use std::sync::PoisonError;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_editor::{Selection, Selections, UnwrapKind, UnwrapOption, unwrap_options};
use zen_i18n::t;
use zen_text::Edit;

use crate::brackets::Highlighters;
use crate::editing::{SelectionInfo, describe_selections};
use crate::{Document, Workbench};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListUnwrap)?;
    registry.register(ApplyUnwrap)?;
    Ok(())
}

/// The parameters of `editor.unwrap.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct ListParams {
    /// The character offset to look around; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

/// The parameters of `editor.unwrap.apply`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct ApplyParams {
    /// The entry to apply: its index in the `editor.unwrap.list` answer for the same place.
    pub index: usize,
    /// The character offset to look around; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

/// One Unwrap/Remove entry as `editor.unwrap.list` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnwrapEntry {
    /// The entry's place in the list, for `editor.unwrap.apply`.
    pub index: usize,
    /// `unwrap` keeps the body, `remove` drops it.
    pub kind: String,
    /// The entry as the popup shows it, for example `Unwrap 'if…'` or `Remove 'else…'`.
    pub title: String,
    /// The construct the entry names (`if…`, `else…`, `for…`, `case…`, `func…`, `f(…)`); none for braces.
    pub construct: Option<String>,
    /// The character range the edit replaces.
    pub start: usize,
    /// The end of that range.
    pub end: usize,
    /// The text that replaces it, re-indented.
    pub replacement: String,
    /// The character ranges that stay, as `[start, end]` pairs: what the popup highlights.
    pub kept: Vec<[usize; 2]>,
}

fn text_document(session: &Session) -> Option<&Document> {
    session.state::<Workbench>().and_then(Workbench::active_document).filter(|document| document.image().is_none())
}

fn no_document() -> CommandError {
    CommandError::new(t!("workbench-error-no-document"))
}

fn skipped(session: &Session, document: &Document) -> Vec<Range<usize>> {
    let text = document.text();
    let Some(highlighting) =
        session.state::<Highlighters>().and_then(|highlighters| highlighters.highlighting_of(document))
    else {
        return Vec::new();
    };
    let spans = highlighting.lock().unwrap_or_else(PoisonError::into_inner).spans(text, 0..text.len());
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut chars = 0;
    let mut byte = 0;
    let mut indices = text.char_indices();
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
    for span in spans.into_iter().filter(|span| span.kind.is_comment() || span.kind.is_string()) {
        let start = to_char(span.range.start);
        let end = to_char(span.range.end);
        match ranges.last_mut() {
            Some(last) if last.end >= start => last.end = last.end.max(end),
            _ => ranges.push(start..end),
        }
    }
    ranges
}

pub fn title(option: &UnwrapOption) -> String {
    match (&option.construct, option.kind) {
        (None, _) => t!("workbench-unwrap-braces"),
        (Some(construct), UnwrapKind::Unwrap) => t!("workbench-unwrap-unwrap", construct = construct.clone()),
        (Some(construct), UnwrapKind::Remove) => t!("workbench-unwrap-remove", construct = construct.clone()),
    }
}

pub fn options_at(session: &Session, offset: Option<usize>) -> Result<Vec<UnwrapOption>, CommandError> {
    let document = text_document(session).ok_or_else(no_document)?;
    let offset = offset.unwrap_or_else(|| document.selections().primary().head);
    Ok(unwrap_options(document.text(), &skipped(session, document), offset))
}

pub struct ListUnwrap;

impl Command for ListUnwrap {
    type Params = ListParams;
    type Output = Vec<UnwrapEntry>;
    const ID: &'static str = "editor.unwrap.list";
    const TITLE: &'static str = "List Unwrap/Remove";
    const DESCRIPTION: &'static str = "The entries of Code → Unwrap/Remove… (⇧⌘⌦) at the primary caret (or an offset) \
         of the active file, innermost first: the if/else chains, loops, switch and select cases, function literals, \
         bare braces and calls with one argument around it. Each is `unwrap` (the construct goes, its body stays, \
         re-indented) or `remove` (an else branch or a case goes with its body), with the range it replaces, the \
         replacement and the ranges that stay. Strings and comments do not count. editor.unwrap.apply applies one.";

    fn is_available(&self, session: &Session) -> bool {
        text_document(session).is_some()
    }

    fn run(&self, session: &mut Session, params: ListParams) -> Result<Vec<UnwrapEntry>, CommandError> {
        let options = options_at(session, params.offset)?;
        Ok(options
            .iter()
            .enumerate()
            .map(|(index, option)| UnwrapEntry {
                index,
                kind: if option.kind == UnwrapKind::Unwrap { "unwrap" } else { "remove" }.to_owned(),
                title: title(option),
                construct: option.construct.clone(),
                start: option.range.start,
                end: option.range.end,
                replacement: option.replacement.clone(),
                kept: option.kept.iter().map(|range| [range.start, range.end]).collect(),
            })
            .collect())
    }
}

pub struct ApplyUnwrap;

impl Command for ApplyUnwrap {
    type Params = ApplyParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.unwrap.apply";
    const TITLE: &'static str = "Apply Unwrap/Remove";
    const DESCRIPTION: &'static str = "Applies the entry `index` of editor.unwrap.list at the same place to the active \
         file as one undo step, and puts the caret where its code went. Returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        text_document(session).is_some()
    }

    fn run(&self, session: &mut Session, params: ApplyParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let options = options_at(session, params.offset)?;
        let count = options.len();
        let option = options
            .into_iter()
            .nth(params.index)
            .ok_or_else(|| CommandError::new(t!("workbench-unwrap-no-entry", index = params.index, count = count)))?;
        let document = session.state_mut::<Workbench>().active_document_mut().ok_or_else(no_document)?;
        if let Some(reason) = document.read_only() {
            return Err(CommandError::new(reason.to_owned()));
        }
        let caret = Selections::from_vec(vec![Selection::caret(option.caret)], 0);
        document.apply_action(vec![Edit::replace(option.range, option.replacement)], caret);
        Ok(describe_selections(document))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use zen_syntax::{HighlightKind, Highlighter, Span};

    use super::*;

    struct Lexer;

    impl Highlighter for Lexer {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<Span> {
            let mut spans = Vec::new();
            let mut at = 0;
            while at < text.len() {
                let start = at;
                if text[at..].starts_with("//") {
                    at = text[at..].find('\n').map_or(text.len(), |end| at + end);
                    spans.push(Span { range: start..at, kind: HighlightKind::Comment });
                } else if text[at..].starts_with('"') {
                    at = text[at + 1..].find('"').map_or(text.len(), |end| at + end + 2);
                    spans.push(Span { range: start..at, kind: HighlightKind::String });
                } else {
                    at += text[at..].chars().next().map_or(1, char::len_utf8);
                }
            }
            spans
        }
    }

    const GO: &str = "package main\n\nfunc f(x int) {\n\tif x > 0 {\n\t\tprintln(\"привет }\") // {\n\t} else {\n\t\tprintln(\"no\")\n\t}\n}\n";

    fn session() -> (tempfile::TempDir, Registry, Session) {
        zen_i18n::register(crate::LOCALES);
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("main.go"), GO).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let lexer: Arc<dyn Highlighter> = Arc::new(Lexer);
        session
            .state_mut::<Highlighters>()
            .set_source(Arc::new(move |name| Lexer.handles(name).then(|| Arc::clone(&lexer))));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry
            .dispatch(&mut session, "file.document.open", json!({ "path": folder.path().join("main.go") }))
            .unwrap();
        (folder, registry, session)
    }

    fn at(text: &str, mark: &str) -> usize {
        text[..text.find(mark).unwrap()].chars().count()
    }

    fn text(session: &Session) -> String {
        session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn titles(listed: &Value) -> Vec<String> {
        listed.as_array().unwrap().iter().map(|entry| entry["title"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn the_list_skips_strings_and_comments_and_apply_is_one_undo_step() {
        let (_folder, registry, mut session) = session();
        let caret = at(GO, "привет");
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": caret }] })).unwrap();
        let listed = registry.dispatch(&mut session, "editor.unwrap.list", json!({})).unwrap();
        assert_eq!(titles(&listed), ["Unwrap 'println(…)'", "Unwrap 'if…'"]);
        assert_eq!(listed[1]["kind"], "unwrap");
        let else_branch = registry.dispatch(&mut session, "editor.unwrap.list", json!({ "offset": at(GO, "\"no\"") }));
        assert_eq!(titles(&else_branch.unwrap()), ["Unwrap 'println(…)'", "Unwrap 'else…'", "Remove 'else…'"]);
        let selections = registry.dispatch(&mut session, "editor.unwrap.apply", json!({ "index": 1 })).unwrap();
        assert_eq!(
            text(&session),
            "package main\n\nfunc f(x int) {\n\tprintln(\"привет }\") // {\n}\n",
            "the body is re-indented and the else branch goes"
        );
        let head = selections[0]["head"].as_u64().unwrap() as usize;
        assert_eq!(head, at(&text(&session), "привет"), "the caret stays on its code");
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        assert_eq!(text(&session), GO, "one undo step");
        let missing = registry.dispatch(&mut session, "editor.unwrap.apply", json!({ "index": 9 }));
        assert!(missing.is_err());
    }

    #[test]
    fn titles_are_translated() {
        zen_i18n::register(crate::LOCALES);
        let option = |kind, construct: Option<&str>| UnwrapOption {
            kind,
            construct: construct.map(str::to_owned),
            range: 0..0,
            replacement: String::new(),
            kept: Vec::new(),
            caret: 0,
        };
        assert_eq!(title(&option(UnwrapKind::Unwrap, Some("for…"))), "Unwrap 'for…'");
        assert_eq!(title(&option(UnwrapKind::Remove, Some("else…"))), "Remove 'else…'");
        assert_eq!(title(&option(UnwrapKind::Unwrap, None)), "Unwrap braces");
    }
}
