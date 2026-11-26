use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_syntax::SymbolKind;

use super::structure::{Resolved, StructureSource, StructureStatus, Symbol, current, resolve, target};

pub const SHOW_BREADCRUMBS: &str = "editor.breadcrumbs.show";

/// One declaration on the path to the caret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    /// The declaration's name.
    pub name: String,
    /// What the declaration is.
    pub kind: SymbolKind,
    /// The first character of the whole declaration (a character offset of the file's text).
    pub start: usize,
    /// The character after the declaration's last.
    pub end: usize,
    /// The first character of the declaration's name, where a click puts the caret.
    pub selection_start: usize,
    /// The line of the name, one-based.
    pub line: u32,
    /// The column of the name, one-based, in characters.
    pub column: u32,
}

/// The parameters of `editor.breadcrumbs.get`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreadcrumbsParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset to find the declarations around; the caret of the open file when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// How long to wait for the language server, in milliseconds; not at all when absent.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `editor.breadcrumbs.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreadcrumbsState {
    /// The file.
    pub path: PathBuf,
    /// Whether the breadcrumbs are shown under the editor (Settings → Editor → General → Breadcrumbs).
    pub shown: bool,
    /// Whether the symbols behind the crumbs are ready.
    pub status: StructureStatus,
    /// Where the symbols come from.
    pub source: StructureSource,
    /// The offset the crumbs are about; absent for a file that is not open when no offset is given.
    pub offset: Option<usize>,
    /// The declarations that hold the offset, outermost first.
    pub crumbs: Vec<Crumb>,
}

pub fn crumbs(symbols: &[Symbol], offset: usize) -> Vec<Crumb> {
    let mut level = symbols;
    let mut path = Vec::new();
    for index in current(symbols, offset) {
        let Some(symbol) = level.get(index) else { break };
        path.push(Crumb {
            name: symbol.name.clone(),
            kind: symbol.kind,
            start: symbol.start,
            end: symbol.end,
            selection_start: symbol.selection_start,
            line: symbol.line,
            column: symbol.column,
        });
        level = &symbol.children;
    }
    path
}

pub struct GetBreadcrumbs;

impl Command for GetBreadcrumbs {
    type Params = BreadcrumbsParams;
    type Output = BreadcrumbsState;
    const ID: &'static str = "editor.breadcrumbs.get";
    const TITLE: &'static str = "Breadcrumbs";
    const DESCRIPTION: &'static str = "The declarations around the caret, outermost first, as the breadcrumbs under the \
         editor show them, from the language server or the language's outliner; the active file by default.";

    fn run(&self, session: &mut Session, params: BreadcrumbsParams) -> Result<BreadcrumbsState, CommandError> {
        let path = target(session, params.path)?;
        let Resolved { status, source, symbols, caret, .. } = resolve(session, &path, super::wait(params.wait_ms));
        let offset = params.offset.or(caret);
        let crumbs = offset.map(|offset| crumbs(&symbols, offset)).unwrap_or_default();
        let shown = session.settings().get::<bool>(SHOW_BREADCRUMBS).unwrap_or(true);
        Ok(BreadcrumbsState { path, shown, status, source, offset, crumbs })
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetBreadcrumbs)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::{Value, json};
    use zen_syntax::{OutlineItem, Outliner};

    use super::*;
    use crate::language::structure::{Outliners, from_outline};

    const SOURCE: &str = "package shop\n\ntype Корзина struct {\n\tтовары []string\n}\n\nfunc (к *Корзина) Добавить(т string) {\n\tк.товары = append(к.товары, т)\n}\n\nfunc main() {\n}\n";

    fn at(needle: &str) -> usize {
        SOURCE[..SOURCE.find(needle).unwrap()].chars().count()
    }

    fn item(name: &str, kind: SymbolKind, from: &str, to: &str, children: Vec<OutlineItem>) -> OutlineItem {
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind,
            start: at(from),
            end: at(to) + to.chars().count(),
            selection_start: at(name),
            children,
        }
    }

    fn outline() -> Vec<OutlineItem> {
        let field = item("товары", SymbolKind::Field, "товары []", "товары []string", Vec::new());
        let method = item("Добавить", SymbolKind::Method, "func (к", "т)\n}", Vec::new());
        vec![
            item("main", SymbolKind::Function, "func main", "func main() {\n}", Vec::new()),
            item("Корзина", SymbolKind::Struct, "type Корзина", "string\n}", vec![method, field]),
        ]
    }

    struct Counting(Arc<AtomicUsize>);

    impl Outliner for Counting {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn outline(&self, _: &str) -> Vec<OutlineItem> {
            self.0.fetch_add(1, Ordering::SeqCst);
            outline()
        }
    }

    fn names(state: &Value) -> Vec<String> {
        state["crumbs"].as_array().unwrap().iter().map(|crumb| crumb["name"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn the_crumbs_are_the_nested_declarations_at_an_offset() {
        let symbols = from_outline(SOURCE, &outline());
        let names = |offset| crumbs(&symbols, offset).into_iter().map(|crumb| crumb.name).collect::<Vec<_>>();
        assert_eq!(names(at("append")), ["Корзина", "Добавить"]);
        assert_eq!(names(at("[]string")), ["Корзина", "товары"]);
        assert_eq!(names(at("struct")), ["Корзина"]);
        assert_eq!(names(at("main() {") + 2), ["main"]);
        assert!(names(at("shop")).is_empty(), "outside any declaration");
        let method = &crumbs(&symbols, at("append"))[1];
        assert_eq!(method.kind, SymbolKind::Method);
        assert_eq!(method.selection_start, at("Добавить"));
        assert_eq!((method.line, method.column), (7, 19));
    }

    #[test]
    fn the_command_follows_the_caret_and_reuses_the_outline_of_a_version() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut fixture = crate::language::testing::fixture(&[("cart.go", SOURCE)]);
        fixture.session.state_mut::<Outliners>().set(vec![Arc::new(Counting(Arc::clone(&calls)))]);
        fixture.open("cart.go");
        fixture.with(|_, workbench| {
            let document = workbench.active_document_mut().unwrap();
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(at("append"))));
        });
        let state = fixture.dispatch("editor.breadcrumbs.get", json!({})).unwrap();
        assert_eq!(names(&state), ["Корзина", "Добавить"]);
        assert_eq!((state["status"].as_str(), state["source"].as_str()), (Some("ready"), Some("syntax")));
        assert_eq!(state["shown"], true);
        assert_eq!(state["offset"], at("append"));
        assert_eq!(state["crumbs"][1]["kind"], "method");
        assert_eq!(state["crumbs"][1]["selectionStart"], at("Добавить"));

        let state = fixture.dispatch("editor.breadcrumbs.get", json!({ "offset": at("[]string") })).unwrap();
        assert_eq!(names(&state), ["Корзина", "товары"]);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the outline of one version is made once");

        fixture.with(|_, workbench| {
            workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "\n".to_owned())]);
        });
        fixture.dispatch("editor.breadcrumbs.get", json!({})).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2, "an edit makes a new outline");

        fixture.session.settings_mut().set(SHOW_BREADCRUMBS, false).unwrap();
        assert_eq!(fixture.dispatch("editor.breadcrumbs.get", json!({})).unwrap()["shown"], false);
    }

    #[test]
    fn the_crumbs_come_from_the_servers_document_symbols() {
        let mut fixture = crate::language::testing::fixture(&[("cart.go", SOURCE)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "documentSymbolProvider": true
        }));
        let range = |start: (u32, u32), end: (u32, u32)| json!({ "start": { "line": start.0, "character": start.1 }, "end": { "line": end.0, "character": end.1 } });
        fixture.server.reply(
            "textDocument/documentSymbol",
            zen_lsp::fake::Reply::Result(json!([
                {
                    "name": "Корзина", "kind": 23, "range": range((2, 0), (4, 1)), "selectionRange": range((2, 5), (2, 12)),
                    "children": [{ "name": "товары", "kind": 8, "range": range((3, 1), (3, 16)), "selectionRange": range((3, 1), (3, 7)) }]
                },
                { "name": "(*Корзина).Добавить", "kind": 6, "range": range((6, 0), (8, 1)), "selectionRange": range((6, 18), (6, 26)) }
            ])),
        );
        fixture.open("cart.go");
        let state =
            fixture.dispatch("editor.breadcrumbs.get", json!({ "offset": at("append"), "waitMs": 5000 })).unwrap();
        assert_eq!((state["status"].as_str(), state["source"].as_str()), (Some("ready"), Some("server")));
        assert_eq!(names(&state), ["Корзина", "Добавить"]);
        assert_eq!(state["crumbs"][1]["selectionStart"], at("Добавить"));
    }

    #[test]
    fn without_a_file_or_an_outline_there_are_no_crumbs() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        assert!(registry.dispatch(&mut session, "editor.breadcrumbs.get", json!({})).is_err(), "no file");
        let state = registry
            .dispatch(&mut session, "editor.breadcrumbs.get", json!({ "path": "/p/main.go", "offset": 3 }))
            .unwrap();
        assert_eq!(state["status"], "unavailable");
        assert_eq!(state["crumbs"], json!([]));
        assert_eq!(state["offset"], 3);
    }
}
