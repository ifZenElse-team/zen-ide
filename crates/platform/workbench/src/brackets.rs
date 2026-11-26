use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::inject::Injector;
use zen_syntax::{Highlighter, Highlighting};

use crate::{Document, Workbench};

pub type HighlighterSource = Arc<dyn Fn(&str) -> Option<Arc<dyn Highlighter>> + Send + Sync>;

pub type InjectorSource = Arc<dyn Fn(&str) -> Vec<Arc<dyn Injector>> + Send + Sync>;

const KEPT: usize = 12;

struct Kept {
    path: PathBuf,
    version: u64,
    highlighter: usize,
    highlighting: Arc<Mutex<Highlighting>>,
}

#[derive(Default)]
pub struct Highlighters {
    source: Option<HighlighterSource>,
    injectors: Option<InjectorSource>,
    kept: Mutex<Vec<Kept>>,
}

fn key(highlighter: &Arc<dyn Highlighter>) -> usize {
    Arc::as_ptr(highlighter).cast::<()>() as usize
}

impl Highlighters {
    pub fn set_source(&mut self, source: HighlighterSource) {
        self.source = Some(source);
        self.kept.get_mut().unwrap_or_else(PoisonError::into_inner).clear();
    }

    pub fn set_injectors(&mut self, injectors: InjectorSource) {
        self.injectors = Some(injectors);
        self.kept.get_mut().unwrap_or_else(PoisonError::into_inner).clear();
    }

    pub fn injectors_for(&self, name: &str) -> Vec<Arc<dyn Injector>> {
        self.injectors.as_ref().map(|source| source(name)).unwrap_or_default()
    }

    pub fn highlighting(&self, document: &Document, highlighter: &Arc<dyn Highlighter>) -> Arc<Mutex<Highlighting>> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let wanted = key(highlighter);
        let found = kept.iter().position(|kept| kept.path == document.path() && kept.highlighter == wanted);
        let entry = match found {
            Some(at) => {
                let mut entry = kept.remove(at);
                if entry.version != document.version() {
                    let changes = document.changes_since(entry.version);
                    entry
                        .highlighting
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .update(document.text(), changes.as_deref());
                    entry.version = document.version();
                }
                entry
            }
            None => {
                let mut highlighting = Highlighting::new(Arc::clone(highlighter), document.text());
                let name = document.path().file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
                highlighting.inject(document.text(), self.injectors_for(&name));
                Kept {
                    path: document.path().to_owned(),
                    version: document.version(),
                    highlighter: wanted,
                    highlighting: Arc::new(Mutex::new(highlighting)),
                }
            }
        };
        let highlighting = Arc::clone(&entry.highlighting);
        kept.push(entry);
        if kept.len() > KEPT {
            kept.remove(0);
        }
        highlighting
    }

    pub fn highlighting_of(&self, document: &Document) -> Option<Arc<Mutex<Highlighting>>> {
        let first = zen_vfs::file_types::head(document.text());
        let name = zen_vfs::file_types::known_name_at(document.path(), Some(first))?;
        let highlighter = self.for_name(&name)?;
        Some(self.highlighting(document, &highlighter))
    }

    pub fn source(&self) -> Option<HighlighterSource> {
        self.source.clone()
    }

    pub fn for_name(&self, name: &str) -> Option<Arc<dyn Highlighter>> {
        self.source.as_ref().and_then(|source| source(name))
    }
}

/// The parameters of `editor.brackets.matching`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct MatchingParams {
    /// The character offset of the caret; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

/// What `editor.brackets.matching` and `editor.brackets.goToMatching` answer.
#[derive(Serialize, JsonSchema)]
pub struct MatchingBracket {
    /// The character offset of the bracket next to the caret; `None` when there is none.
    pub bracket: Option<usize>,
    /// The character offset of the bracket that matches it; `None` when it is unmatched.
    pub matching: Option<usize>,
}

fn text_document(session: &Session) -> Option<&Document> {
    session.state::<Workbench>().and_then(Workbench::active_document).filter(|document| document.image().is_none())
}

fn matching(
    session: &Session,
    offsets: Option<&[usize]>,
) -> Result<Vec<Option<zen_editor::BracketMatch>>, CommandError> {
    let document = text_document(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let primary = [document.selections().primary().head];
    let highlighters = session.state::<Highlighters>();
    let highlighter = highlighters.and_then(|highlighters| highlighters.for_name(&document.name()));
    let pairs = zen_editor::bracket_pairs(highlighter.as_ref().is_some_and(|highlighter| highlighter.angle_brackets()));
    let highlighting = highlighters
        .zip(highlighter)
        .map(|(highlighters, highlighter)| highlighters.highlighting(document, &highlighter));
    let highlighting =
        highlighting.as_ref().map(|highlighting| highlighting.lock().unwrap_or_else(PoisonError::into_inner));
    let text = document.text();
    let skipped =
        |byte| highlighting.as_ref().is_some_and(|highlighting| highlighting.in_string_or_comment(text, byte));
    Ok(offsets
        .unwrap_or(&primary)
        .iter()
        .map(|&offset| zen_editor::matching_bracket(document.snapshot(), offset, pairs, &skipped))
        .collect())
}

impl From<Option<zen_editor::BracketMatch>> for MatchingBracket {
    fn from(found: Option<zen_editor::BracketMatch>) -> Self {
        Self { bracket: found.map(|found| found.bracket), matching: found.and_then(|found| found.matching) }
    }
}

pub struct ListMatching;

impl Command for ListMatching {
    type Params = MatchingParams;
    type Output = MatchingBracket;
    const ID: &'static str = "editor.brackets.matching";
    const TITLE: &'static str = "Matching Bracket";
    const DESCRIPTION: &'static str = "The bracket next to the caret (or to an offset) in the active file and the \
         bracket that matches it, as the editor highlights them; brackets in strings and comments do not count.";

    fn is_available(&self, session: &Session) -> bool {
        text_document(session).is_some()
    }

    fn run(&self, session: &mut Session, params: MatchingParams) -> Result<MatchingBracket, CommandError> {
        let found = matching(session, params.offset.as_ref().map(std::slice::from_ref))?;
        Ok(found.into_iter().next().flatten().into())
    }
}

pub struct GoToMatching;

impl Command for GoToMatching {
    type Params = NoParams;
    type Output = MatchingBracket;
    const ID: &'static str = "editor.brackets.goToMatching";
    const TITLE: &'static str = "Move Caret to Matching Brace";
    const DESCRIPTION: &'static str = "Moves every caret of the active file next to a bracket to its match: past \
         a closing bracket, before an opening one, so that running it again goes back; a caret with no matched \
         bracket next to it stays. Answers for the primary caret.";

    fn is_available(&self, session: &Session) -> bool {
        text_document(session).is_some()
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<MatchingBracket, CommandError> {
        let document = text_document(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let selections = document.selections().clone();
        let heads: Vec<usize> = selections.iter().map(|selection| selection.head).collect();
        let found = matching(session, Some(&heads))?;
        let moved: Vec<zen_editor::Selection> = selections
            .iter()
            .zip(&found)
            .map(|(selection, found)| match *found {
                Some(zen_editor::BracketMatch { bracket, matching: Some(matching) }) => {
                    zen_editor::Selection::caret(if matching > bracket { matching + 1 } else { matching })
                }
                _ => *selection,
            })
            .collect();
        if found.iter().any(|found| found.is_some_and(|found| found.matching.is_some()))
            && let Some(document) = session.state_mut::<Workbench>().active_document_mut()
        {
            document.set_selections(zen_editor::Selections::from_vec(moved, selections.primary_index()));
        }
        Ok(found.get(selections.primary_index()).copied().flatten().into())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListMatching)?;
    registry.register(GoToMatching)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use zen_syntax::{HighlightKind, Highlighter, Span};

    use super::Highlighters;

    struct Quotes;

    impl Highlighter for Quotes {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<Span> {
            let quotes: Vec<usize> = text.match_indices('"').map(|(at, _)| at).collect();
            quotes
                .chunks(2)
                .filter(|pair| pair.len() == 2)
                .map(|pair| Span { range: pair[0]..pair[1] + 1, kind: HighlightKind::String })
                .collect()
        }
    }

    #[test]
    fn the_matching_bracket_skips_strings_and_the_caret_goes_to_it() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.go");
        std::fs::write(&path, "f(\"ё)\", g(x))\n").unwrap();
        let mut registry = zen_core::Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        let quotes: Arc<dyn Highlighter> = Arc::new(Quotes);
        session
            .state_mut::<Highlighters>()
            .set_source(Arc::new(move |name| Quotes.handles(name).then(|| Arc::clone(&quotes))));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let mut ask = |params| registry.dispatch(&mut session, "editor.brackets.matching", params).unwrap();
        assert_eq!(ask(json!({ "offset": 1 })), json!({ "bracket": 1, "matching": 12 }));
        assert_eq!(ask(json!({ "offset": 4 })), json!({ "bracket": null, "matching": null }), "in a string");
        assert_eq!(ask(json!({ "offset": 14 })), json!({ "bracket": null, "matching": null }));
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 13 }] })).unwrap();
        let moved = registry.dispatch(&mut session, "editor.brackets.goToMatching", json!({})).unwrap();
        assert_eq!(moved, json!({ "bracket": 12, "matching": 1 }));
        let caret = |session: &zen_core::Session| {
            session.state::<crate::Workbench>().unwrap().active_document().unwrap().selections().primary().head
        };
        assert_eq!(caret(&session), 1);
        registry.dispatch(&mut session, "editor.brackets.goToMatching", json!({})).unwrap();
        assert_eq!(caret(&session), 13, "and back, past the closing bracket");
        registry
            .dispatch(
                &mut session,
                "editor.selection.set",
                json!({ "selections": [{ "head": 1 }, { "head": 7 }, { "head": 9 }] }),
            )
            .unwrap();
        registry.dispatch(&mut session, "editor.brackets.goToMatching", json!({})).unwrap();
        let carets: Vec<usize> = session
            .state::<crate::Workbench>()
            .unwrap()
            .active_document()
            .unwrap()
            .selections()
            .iter()
            .map(|selection| selection.head)
            .collect();
        assert_eq!(carets, [7, 12, 13], "every caret moves; one away from a bracket stays");
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "\"" })).unwrap();
        let quoted = registry.dispatch(&mut session, "editor.brackets.matching", json!({ "offset": 2 })).unwrap();
        assert_eq!(quoted, json!({ "bracket": null, "matching": null }), "the bracket is in a string now");
        let workbench = session.state::<crate::Workbench>().unwrap();
        let document = workbench.active_document().unwrap();
        let highlighters = session.state::<Highlighters>().unwrap();
        let highlighter: Arc<dyn Highlighter> = highlighters.for_name("main.go").unwrap();
        assert!(Arc::ptr_eq(
            &highlighters.highlighting(document, &highlighter),
            &highlighters.highlighting_of(document).unwrap()
        ));
        std::fs::write(folder.path().join("a.txt"), "(\")\"").unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": folder.path().join("a.txt") })).unwrap();
        let plain = registry.dispatch(&mut session, "editor.brackets.matching", json!({ "offset": 0 })).unwrap();
        assert_eq!(plain, json!({ "bracket": 0, "matching": 2 }), "without a highlighter every bracket counts");
    }
}
