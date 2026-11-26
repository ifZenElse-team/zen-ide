use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_syntax::SymbolKind;

use super::structure::{Resolved, StructureSource, StructureStatus, Symbol, resolve, target};
use crate::line_numbers::method_separators_shown;
use crate::{CharEdit, Workbench};

const COMMENT_STARTS: [&str; 5] = ["//", "/*", "*", "#", "@"];

/// One method separator: the line it is drawn above and the declaration it belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Separator {
    /// The line the separator is drawn above, one-based: the declaration's first line, or the first
    /// line of the comments, attributes or decorators right above it.
    pub line: u32,
    /// The declaration's name.
    pub name: String,
    /// What the declaration is.
    pub kind: SymbolKind,
}

/// The parameters of `editor.methodSeparators.list`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MethodSeparatorsParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// How long to wait for the language server, in milliseconds; not at all when absent.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `editor.methodSeparators.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MethodSeparatorsState {
    /// The file.
    pub path: PathBuf,
    /// Whether the editor draws the separators (Settings → Editor → General → Appearance → Show method separators).
    pub shown: bool,
    /// Whether the declarations behind the separators are ready.
    pub status: StructureStatus,
    /// Where the declarations come from.
    pub source: StructureSource,
    /// The separators, in the order of the text, one per line.
    pub separators: Vec<Separator>,
}

fn separates(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Struct
            | SymbolKind::Interface
            | SymbolKind::Type
            | SymbolKind::Class
    )
}

fn top_level(symbols: &[Symbol]) -> Vec<&Symbol> {
    let mut found = Vec::new();
    for symbol in symbols {
        found.push(symbol);
        found.extend(symbol.children.iter().filter(|child| child.start >= symbol.end || child.end <= symbol.start));
    }
    found.retain(|symbol| separates(symbol.kind));
    found
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut count = 0;
    for c in text.chars() {
        count += 1;
        if c == '\n' {
            starts.push(count);
        }
    }
    starts
}

fn leading_line(lines: &[&str], mut line: usize) -> usize {
    while line > 0 {
        let above = lines[line - 1].trim_start();
        if above.is_empty() || !COMMENT_STARTS.iter().any(|start| above.starts_with(start)) {
            break;
        }
        line -= 1;
    }
    line
}

pub fn separators(text: &str, symbols: &[Symbol]) -> Vec<Separator> {
    let starts = line_starts(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut found: Vec<Separator> = top_level(symbols)
        .into_iter()
        .map(|symbol| {
            let line = starts.partition_point(|&start| start <= symbol.start).saturating_sub(1);
            (leading_line(&lines, line), symbol)
        })
        .filter(|&(line, _)| line > 0)
        .map(|(line, symbol)| Separator { line: line as u32 + 1, name: symbol.name.clone(), kind: symbol.kind })
        .collect();
    found.sort_by_key(|separator| separator.line);
    found.dedup_by_key(|separator| separator.line);
    found
}

pub fn moved_offset(offset: usize, edits: &[CharEdit]) -> Option<usize> {
    edits.iter().try_fold(offset, |offset, edit| (!edit.range.contains(&offset)).then(|| edit.map(offset)))
}

fn moved(symbols: &[Symbol], edits: &[CharEdit]) -> Vec<Symbol> {
    symbols
        .iter()
        .filter_map(|symbol| {
            let start = moved_offset(symbol.start, edits)?;
            let end = edits.iter().fold(symbol.end, |offset, edit| edit.map(offset)).max(start);
            Some(Symbol { start, end, children: moved(&symbol.children, edits), ..symbol.clone() })
        })
        .collect()
}

fn text_of(
    session: &Session,
    path: &Path,
    read_for: Option<u64>,
    symbols: Vec<Symbol>,
) -> Option<(String, Vec<Symbol>)> {
    let open = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path));
    let Some(document) = open else { return std::fs::read_to_string(path).ok().map(|text| (text, symbols)) };
    let symbols = match read_for {
        Some(version) => moved(&symbols, &document.char_edits_since(version)?),
        None => symbols,
    };
    Some((document.text().to_owned(), symbols))
}

pub struct ListMethodSeparators;

impl Command for ListMethodSeparators {
    type Params = MethodSeparatorsParams;
    type Output = MethodSeparatorsState;
    const ID: &'static str = "editor.methodSeparators.list";
    const TITLE: &'static str = "Method Separators";
    const DESCRIPTION: &'static str = "The lines the editor draws a thin separator above while Show method separators \
         is on: one above each top-level function, method and type declaration (and the comments right above it), \
         from the language server's document symbols or the language's outliner; the active file by default.";

    fn run(
        &self,
        session: &mut Session,
        params: MethodSeparatorsParams,
    ) -> Result<MethodSeparatorsState, CommandError> {
        let path = target(session, params.path)?;
        let Resolved { status, source, symbols, read_for, .. } = resolve(session, &path, super::wait(params.wait_ms));
        let separators = text_of(session, &path, read_for, symbols)
            .map(|(text, symbols)| separators(&text, &symbols))
            .unwrap_or_default();
        let shown = method_separators_shown(session.settings());
        Ok(MethodSeparatorsState { path, shown, status, source, separators })
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListMethodSeparators)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use zen_syntax::{OutlineItem, Outliner};

    use super::*;
    use crate::language::structure::Outliners;

    const SOURCE: &str = "package shop\n\nimport \"fmt\"\n\nconst limit = 3\n\n// Корзина holds things.\n// It is a cart.\ntype Корзина struct {\n\tтовары []string\n}\n\n// Добавить adds one.\nfunc (к *Корзина) Добавить(т string) {\n\tк.товары = append(к.товары, т)\n}\nfunc main() {\n\tfmt.Println(limit)\n}\n";

    fn at(needle: &str) -> usize {
        SOURCE[..SOURCE.find(needle).unwrap()].chars().count()
    }

    fn item(name: &str, kind: SymbolKind, from: &str, to: &str) -> OutlineItem {
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind,
            start: at(from),
            end: at(to) + to.chars().count(),
            selection_start: at(from),
            children: Vec::new(),
        }
    }

    struct Fixed;

    impl Outliner for Fixed {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn outline(&self, _: &str) -> Vec<OutlineItem> {
            let mut cart = item("Корзина", SymbolKind::Struct, "type Корзина", "string\n}");
            cart.children.push(item("товары", SymbolKind::Field, "товары []", "товары []string"));
            cart.children.push(item("Добавить", SymbolKind::Method, "func (к", "т)\n}"));
            vec![
                item("limit", SymbolKind::Constant, "const limit", "limit = 3"),
                cart,
                item("main", SymbolKind::Function, "func main", "limit)\n}"),
            ]
        }
    }

    fn lines(state: &Value) -> Vec<(u64, String)> {
        state["separators"]
            .as_array()
            .unwrap()
            .iter()
            .map(|separator| (separator["line"].as_u64().unwrap(), separator["name"].as_str().unwrap().to_owned()))
            .collect()
    }

    #[test]
    fn a_separator_goes_above_each_top_level_declaration_and_its_comments() {
        let mut fixture = crate::language::testing::fixture(&[("cart.go", SOURCE)]);
        fixture.session.state_mut::<Outliners>().set(vec![Arc::new(Fixed)]);
        fixture.open("cart.go");
        let state = fixture.dispatch("editor.methodSeparators.list", json!({})).unwrap();
        assert_eq!(
            lines(&state),
            [(7, "Корзина".to_owned()), (13, "Добавить".to_owned()), (17, "main".to_owned())],
            "the constant and the field have none; a method grouped under its type has one"
        );
        assert_eq!((state["status"].as_str(), state["source"].as_str()), (Some("ready"), Some("syntax")));
        assert_eq!(state["shown"], false, "off by default");
        assert_eq!(state["separators"][1]["kind"], "method");
        fixture.session.settings_mut().set(crate::line_numbers::SHOW_METHOD_SEPARATORS, true).unwrap();
        assert_eq!(fixture.dispatch("editor.methodSeparators.list", json!({})).unwrap()["shown"], true);
    }

    #[test]
    fn the_declarations_come_from_the_servers_document_symbols() {
        let mut fixture = crate::language::testing::fixture(&[("cart.go", SOURCE)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "documentSymbolProvider": true
        }));
        let range = |start: (u32, u32), end: (u32, u32)| json!({ "start": { "line": start.0, "character": start.1 }, "end": { "line": end.0, "character": end.1 } });
        fixture.server.reply(
            "textDocument/documentSymbol",
            zen_lsp::fake::Reply::Result(json!([
                { "name": "limit", "kind": 14, "range": range((4, 6), (4, 15)), "selectionRange": range((4, 6), (4, 11)) },
                { "name": "Корзина", "kind": 23, "range": range((8, 5), (10, 1)), "selectionRange": range((8, 5), (8, 12)) },
                { "name": "(*Корзина).Добавить", "kind": 6, "range": range((13, 0), (15, 1)), "selectionRange": range((13, 18), (13, 26)) },
                { "name": "main", "kind": 12, "range": range((16, 0), (18, 1)), "selectionRange": range((16, 5), (16, 9)) }
            ])),
        );
        fixture.open("cart.go");
        let state = fixture.dispatch("editor.methodSeparators.list", json!({ "waitMs": 2000 })).unwrap();
        assert_eq!((state["status"].as_str(), state["source"].as_str()), (Some("ready"), Some("server")));
        assert_eq!(lines(&state), [(7, "Корзина".to_owned()), (13, "Добавить".to_owned()), (17, "main".to_owned())]);
    }

    #[test]
    fn the_servers_last_symbols_follow_the_edits_until_it_answers_again() {
        let mut fixture = crate::language::testing::fixture(&[("cart.go", SOURCE)]);
        fixture.server.set_capabilities(json!({
            "textDocumentSync": { "openClose": true, "change": 2 },
            "documentSymbolProvider": true
        }));
        let range = |start: (u32, u32), end: (u32, u32)| json!({ "start": { "line": start.0, "character": start.1 }, "end": { "line": end.0, "character": end.1 } });
        fixture.server.reply(
            "textDocument/documentSymbol",
            zen_lsp::fake::Reply::Result(json!([
                { "name": "Корзина", "kind": 23, "range": range((8, 0), (10, 1)), "selectionRange": range((8, 5), (8, 12)) },
                { "name": "(*Корзина).Добавить", "kind": 6, "range": range((13, 0), (15, 1)), "selectionRange": range((13, 18), (13, 26)) },
                { "name": "main", "kind": 12, "range": range((16, 0), (18, 1)), "selectionRange": range((16, 5), (16, 9)) }
            ])),
        );
        fixture.open("cart.go");
        let ready = fixture.dispatch("editor.methodSeparators.list", json!({ "waitMs": 2000 })).unwrap();
        assert_eq!(lines(&ready), [(7, "Корзина".to_owned()), (13, "Добавить".to_owned()), (17, "main".to_owned())]);
        fixture.server.reply("textDocument/documentSymbol", zen_lsp::fake::Reply::Hold);
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
        fixture.dispatch("editor.edit.insert", json!({ "text": "\n\n" })).unwrap();
        let shifted = fixture.dispatch("editor.methodSeparators.list", json!({})).unwrap();
        assert_eq!(shifted["status"], "pending");
        assert_eq!(
            lines(&shifted),
            [(9, "Корзина".to_owned()), (15, "Добавить".to_owned()), (19, "main".to_owned())],
            "two lines inserted above move them down"
        );
        let main = at("func main") + 2;
        let end = SOURCE.chars().count() + 2;
        fixture.dispatch("editor.selection.set", json!({ "selections": [{ "anchor": main, "head": end }] })).unwrap();
        fixture.dispatch("editor.edit.insert", json!({ "text": "" })).unwrap();
        let removed = fixture.dispatch("editor.methodSeparators.list", json!({})).unwrap();
        assert_eq!(
            lines(&removed),
            [(9, "Корзина".to_owned()), (15, "Добавить".to_owned())],
            "a removed declaration loses its separator"
        );
    }

    #[test]
    fn nothing_is_drawn_above_the_first_line_and_one_line_takes_one_separator() {
        let text = "func a() {}\nfunc b() {}; func c() {}\n";
        let symbol = |name: &str, start: usize| Symbol {
            name: name.to_owned(),
            detail: None,
            kind: SymbolKind::Function,
            start,
            end: start + 11,
            selection_start: start + 5,
            line: 1,
            column: 1,
            children: Vec::new(),
        };
        let found = separators(text, &[symbol("a", 0), symbol("b", 12), symbol("c", 25)]);
        assert_eq!(found, [Separator { line: 2, name: "b".to_owned(), kind: SymbolKind::Function }]);
    }
}
