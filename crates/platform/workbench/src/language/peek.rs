use std::path::PathBuf;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

use super::docs::hover;
use super::{Languages, LookupKind, LookupParams, WaitParams, with_languages};
use crate::Workbench;

/// The declaration peeked at.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeekState {
    /// Whether a peek is open.
    pub open: bool,
    /// Whether the server has not answered yet.
    pub pending: bool,
    /// The declaration's first line, Go code; `None` when the server knows none.
    pub declaration: Option<String>,
    /// The fence's language of the declaration (`go`), for highlighting.
    pub language: Option<String>,
    /// The characters of the name in the file, when the server says.
    pub range: Option<[usize; 2]>,
    /// Why there is nothing, in words for the user.
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Peek {
    shown: Option<Shown>,
}

struct Shown {
    path: PathBuf,
    offset: usize,
    state: PeekState,
}

impl Peek {
    fn state(&mut self, languages: &mut Languages, wait: Duration) -> PeekState {
        let Some(shown) = &mut self.shown else { return PeekState::default() };
        if shown.state.pending {
            let answer = match languages.lookup_mut(LookupKind::Peek) {
                Some(lookup) => lookup.poll(wait).then(|| lookup.answer.clone()),
                None => Some(None),
            };
            if let Some(answer) = answer {
                let text = languages.synced_text(&shown.path).unwrap_or_default().to_owned();
                match answer {
                    Some(Ok(value)) => {
                        let (markdown, range) = hover(&text, &value);
                        if let Some((language, line)) = markdown.as_deref().and_then(declaration) {
                            shown.state.declaration = Some(line);
                            shown.state.language = Some(language);
                        }
                        shown.state.range = range;
                    }
                    Some(Err(error)) => shown.state.error = Some(error),
                    None => {}
                }
                shown.state.pending = false;
            }
        }
        PeekState { open: true, ..shown.state.clone() }
    }
}

pub fn declaration(markdown: &str) -> Option<(String, String)> {
    let mut lines = markdown.lines();
    let fence = lines.by_ref().find(|line| line.trim_start().starts_with("```"))?;
    let language = fence.trim().trim_start_matches('`').trim().to_owned();
    let first = lines.find(|line| !line.trim().is_empty())?;
    if first.trim_start().starts_with("```") {
        return None;
    }
    let mut line = first.trim();
    if let Some(at) = line.find(" //") {
        line = line[..at].trim_end();
    }
    let line = line.strip_suffix('{').unwrap_or(line).trim_end();
    Some((language, line.to_owned())).filter(|(_, line)| !line.is_empty())
}

fn with_peek<T>(session: &mut Session, f: impl FnOnce(&mut Peek, &mut Languages, &mut Workbench) -> T) -> T {
    let mut peek = std::mem::take(session.state_mut::<Peek>());
    let result = with_languages(session, |languages, workbench| f(&mut peek, languages, workbench));
    session.set_state(peek);
    result
}

pub struct ShowPeek;

impl Command for ShowPeek {
    type Params = LookupParams;
    type Output = PeekState;
    const ID: &'static str = "editor.declarationPeek.show";
    const TITLE: &'static str = "Peek Declaration";
    const DESCRIPTION: &'static str = "Asks the language server for the declaration of the name at a place (the caret by \
         default), as the editor shows it while ⌘ is held over a name; editor.declarationPeek.state reads it.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<PeekState, CommandError> {
        let wait = params.wait();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(workbench) = session.state::<Workbench>()
            && let Ok((path, offset)) = params.place(workbench)
            && let Some(documented) = crate::database::assist::documentation(session, &path, offset)
        {
            let state = PeekState {
                open: true,
                pending: false,
                declaration: documented.as_ref().map(|documented| documented.declaration.clone()),
                language: documented.as_ref().map(|_| "sql".to_owned()),
                range: documented.as_ref().map(|documented| documented.range),
                error: None,
            };
            session.state_mut::<Peek>().shown = Some(Shown { path, offset, state: state.clone() });
            return Ok(state);
        }
        with_peek(session, |peek, languages, workbench| {
            let (path, offset) = params.place(workbench)?;
            let same = peek.shown.as_ref().is_some_and(|shown| shown.path == path && shown.offset == offset);
            if !same {
                languages
                    .start_lookup(workbench, LookupKind::Peek, &path, offset, "textDocument/hover", json!({}))
                    .map_err(CommandError::new)?;
                peek.shown = Some(Shown { path, offset, state: PeekState { pending: true, ..PeekState::default() } });
            }
            Ok(peek.state(languages, wait))
        })
    }
}

pub struct PeekStateCommand;

impl Command for PeekStateCommand {
    type Params = WaitParams;
    type Output = PeekState;
    const ID: &'static str = "editor.declarationPeek.state";
    const TITLE: &'static str = "Peek Declaration State";
    const DESCRIPTION: &'static str = "The declaration peeked at, waiting up to waitMs for the server's answer.";

    fn run(&self, session: &mut Session, params: WaitParams) -> Result<PeekState, CommandError> {
        let wait = params.wait();
        Ok(with_peek(session, |peek, languages, _| peek.state(languages, wait)))
    }
}

pub struct HidePeek;

impl Command for HidePeek {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "editor.declarationPeek.hide";
    const TITLE: &'static str = "Hide Peek Declaration";
    const DESCRIPTION: &'static str = "Closes the declaration peek, cancelling its request if it still waits.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        with_peek(session, |peek, languages, _| {
            languages.end_lookup(LookupKind::Peek);
            peek.shown = None;
        });
        Ok(())
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ShowPeek)?;
    registry.register(PeekStateCommand)?;
    registry.register(HidePeek)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::testing::fixture;

    const MAIN: &str = "package main\n\n// привет\nfunc main() { TestLegacyKey(nil) }\n";

    #[test]
    fn the_declaration_is_the_first_line_of_the_first_code_block() {
        let function = "```go\nfunc TestLegacyKey(t *testing.T)\n```\n\nTestLegacyKey checks.";
        assert_eq!(declaration(function), Some(("go".into(), "func TestLegacyKey(t *testing.T)".into())));
        let structure = "```go\ntype App struct { // size=48 (0x30)\n\tConfig *Config\n}\n```";
        assert_eq!(declaration(structure), Some(("go".into(), "type App struct".into())));
        assert_eq!(declaration("just text"), None);
        assert_eq!(declaration("```go\n```"), None);
    }

    #[test]
    fn a_peek_comes_from_hover_and_closes() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.server.on(
            "textDocument/hover",
            json!({ "contents": { "kind": "markdown", "value": "```go\nfunc TestLegacyKey(t *testing.T)\n```\n\nDocs." } }),
        );
        fixture.open("main.go");
        let offset = MAIN[..MAIN.find("TestLegacyKey").unwrap()].chars().count() + 3;
        let shown =
            fixture.dispatch("editor.declarationPeek.show", json!({ "offset": offset, "waitMs": 5000 })).unwrap();
        assert_eq!((shown["open"].clone(), shown["pending"].clone()), (json!(true), json!(false)));
        assert_eq!(shown["declaration"], "func TestLegacyKey(t *testing.T)");
        assert_eq!(shown["language"], "go");
        fixture.dispatch("editor.declarationPeek.show", json!({ "offset": offset })).unwrap();
        assert_eq!(fixture.server.requests("textDocument/hover").len(), 1);
        fixture.dispatch("editor.declarationPeek.hide", json!({})).unwrap();
        let state = fixture.dispatch("editor.declarationPeek.state", json!({})).unwrap();
        assert_eq!(state["open"], false);
    }
}
