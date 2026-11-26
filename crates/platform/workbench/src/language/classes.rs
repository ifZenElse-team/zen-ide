use std::path::PathBuf;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_syntax::SymbolKind;

use super::structure::symbol_kind;
use super::symbols::{DEFAULT_LIMIT, TYPE_KINDS, server_query};
use super::{AbsentServer, Reason, WorkProgress, with_languages};

/// The parameters of `navigate.class.goTo`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassParams {
    /// The name, its camel humps (`IS` for `ItemStore`), or a dotted name with its package (`model.Item`).
    pub query: String,
    /// How many types to return; 50 by default, at most 500.
    #[serde(default)]
    pub limit: Option<usize>,
    /// How long to wait for the servers, in milliseconds: at most 10 000, or 120 000 while a
    /// server is still loading the project (`scanning`, `starting`, `indexing`); not at all when absent.
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// Whether to answer `rows`, one line of text per type (`path:line:column kind name`),
    /// instead of `{classes}` objects with offsets and matched ranges; false by default.
    #[serde(default)]
    pub compact: Option<bool>,
}

/// A type found by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassItem {
    /// Its name.
    pub name: String,
    /// What it is.
    pub kind: SymbolKind,
    /// Where it is declared, as the server says it: for Go, the package path.
    pub container: Option<String>,
    /// The file it is declared in.
    #[serde(skip)]
    pub path: PathBuf,
    /// The file relative to the project, with `/`, or the whole path outside it (card WXtO7EzR).
    #[serde(rename = "path")]
    pub relative: String,
    /// The character offset of its name in the file, for `navigate.place.goTo`.
    pub offset: usize,
    /// The line of its name, one-based.
    pub line: u32,
    /// The column of its name, one-based, in characters.
    pub column: u32,
    /// Whether the file is in the project (not in the SDK or a library).
    pub in_project: bool,
    /// The matched characters of `name`, as `[start, end)` character indexes.
    pub ranges: Vec<[usize; 2]>,
}

/// What `navigate.class.goTo` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassResult {
    /// The types found, best kind of match first, then the project's, then the closer matches, then the shorter names.
    pub classes: Vec<ClassItem>,
    /// With `compact`: the types as rows, `path:line:column kind name`, and `classes` empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<String>,
    /// Whether a server is still starting or searching: call again for more.
    pub pending: bool,
    /// Why more may come (`scanning`, `starting`, `indexing`, `searching`), or why nothing came
    /// (`noMatch`, `noServer`, `serverMissing`, `serverNotRunning`); absent when types came and the
    /// search is done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// The work a server reports under way while `reason` is `indexing`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<WorkProgress>,
    /// With `reason` `serverMissing`, `serverNotRunning` or `starting`: the server the project
    /// needs, how to install it and what it means.
    #[serde(flatten)]
    pub absent: Option<AbsentServer>,
}

pub struct GoToClass;

impl Command for GoToClass {
    type Params = ClassParams;
    type Output = ClassResult;
    const ID: &'static str = "navigate.class.goTo";
    const TITLE: &'static str = "Go to Class";
    const DESCRIPTION: &'static str = "Finds the project's types (classes, interfaces, structs) by name with the language \
         servers' workspace symbols, matched by the shared name matcher (navigate.names.match): camel humps (`IS`), `*` and dotted names (`model.Item`); best matches first, with their matched ranges (`compact`: one row of text each, `path:line:column kind name`); starts the servers when needed. Answers with what has come and \
         `pending` while a server works; `waitMs` waits for it, up to 120 s. Open one with navigate.place.goTo and its path and offset.";

    fn run(&self, session: &mut Session, params: ClassParams) -> Result<ClassResult, CommandError> {
        let typed = params.query.trim_start().to_owned();
        let wait = Duration::from_millis(params.wait_ms.unwrap_or(0));
        let explicit = params.wait_ms.is_some();
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
        Ok(with_languages(session, |languages, workbench| {
            if server_query(&typed).is_empty() {
                return ClassResult {
                    classes: Vec::new(),
                    rows: Vec::new(),
                    pending: false,
                    reason: Some(Reason::NoMatch),
                    progress: None,
                    absent: None,
                };
            }
            let searched = languages.search_waiting(workbench, &typed, wait, explicit);
            let classes = languages
                .matched_symbols(workbench, &typed, |kind| TYPE_KINDS.contains(&kind), limit)
                .into_iter()
                .map(|found| {
                    let at = found.qualified.chars().count() - found.name.chars().count();
                    let ranges: Vec<std::ops::Range<usize>> = found
                        .ranges
                        .iter()
                        .filter(|range| range.start >= at)
                        .map(|range| range.start - at..range.end - at)
                        .collect();
                    (found, ranges)
                })
                .map(|(found, ranges)| ClassItem {
                    name: found.name,
                    kind: symbol_kind(found.kind),
                    container: found.container,
                    path: found.path,
                    relative: found.relative,
                    offset: found.offset,
                    line: found.line,
                    column: found.column,
                    in_project: found.in_project,
                    ranges: crate::navigate::spans(&ranges),
                })
                .collect::<Vec<_>>();
            let reason = searched.reason_if_empty(classes.is_empty());
            let (pending, reason, absent) = languages.explain_no_server(workbench, searched.pending, reason);
            let (classes, rows) = if params.compact == Some(true) {
                let rows = classes
                    .iter()
                    .map(|class| {
                        let what = format!("{} {}", super::compact::name_of(&class.kind), class.name);
                        super::compact::row(&class.relative, class.line, class.column, &what)
                    })
                    .collect();
                (Vec::new(), rows)
            } else {
                (classes, Vec::new())
            };
            ClassResult { classes, rows, pending, reason, progress: searched.progress, absent }
        }))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GoToClass)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::testing;

    fn symbol(name: &str, kind: u64, uri: &str, line: u32, character: u32) -> Value {
        json!({
            "name": name,
            "kind": kind,
            "containerName": "example.com/shop/model",
            "location": { "uri": uri, "range": {
                "start": { "line": line, "character": character },
                "end": { "line": line, "character": character + name.len() as u32 } } }
        })
    }

    #[test]
    fn types_are_found_by_name_and_open_at_their_name() {
        let model = "package model\n\n// Item is a thing.\ntype Item struct{}\n\ntype ItemStore interface{}\n\nfunc Items() {}\n";
        let mut fixture = testing::fixture(&[("main.go", "package main\n"), ("model/model.go", model)]);
        fixture.server.set_capabilities(json!({ "workspaceSymbolProvider": true }));
        let uri = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
        let outside = "file:///usr/local/go/src/net/http/server.go";
        fixture.server.on(
            "workspace/symbol",
            json!([
                symbol("Items", 12, &uri, 7, 5),
                symbol("ItemHandler", 11, outside, 10, 5),
                symbol("ItemStore", 11, &uri, 5, 5),
                symbol("Item", 23, &uri, 3, 5),
            ]),
        );
        let found = fixture.dispatch("navigate.class.goTo", json!({ "query": "item", "waitMs": 5000 })).unwrap();
        assert_eq!(found["pending"], false, "{found}");
        let names: Vec<&str> =
            found["classes"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Item", "ItemStore", "ItemHandler"], "the project's types first, the function left out");
        let item = &found["classes"][0];
        assert_eq!(item["kind"], "struct");
        assert_eq!((item["line"].as_u64(), item["column"].as_u64()), (Some(4), Some(6)));
        assert_eq!(item["inProject"], true);
        assert_eq!(found["classes"][2]["inProject"], false);
        assert_eq!(item["path"], "model/model.go", "relative to the project (card WXtO7EzR)");
        assert_eq!(found["classes"][2]["path"], "/usr/local/go/src/net/http/server.go");
        let offset = item["offset"].as_u64().unwrap() as usize;
        assert!(model.chars().skip(offset).collect::<String>().starts_with("Item struct"));
        assert_eq!(fixture.server.requests("workspace/symbol").len(), 1);
        let again = fixture.dispatch("navigate.class.goTo", json!({ "query": "item", "limit": 1 })).unwrap();
        assert_eq!(again["classes"].as_array().unwrap().len(), 1);
        assert_eq!(fixture.server.requests("workspace/symbol").len(), 1);
        let empty = fixture.dispatch("navigate.class.goTo", json!({ "query": "  " })).unwrap();
        assert_eq!(empty, json!({ "classes": [], "pending": false, "reason": "noMatch" }));
    }

    #[test]
    fn a_project_type_comes_before_a_shorter_sdk_type_that_matches_as_well() {
        let mut fixture = testing::fixture(&[("main.go", "package main\n\ntype CartStore struct{}\n")]);
        fixture.server.set_capabilities(json!({ "workspaceSymbolProvider": true }));
        let uri = zen_lsp::position::file_uri(&fixture.path("main.go"));
        let sdk = "file:///usr/local/go/src/cart/pool.go";
        fixture.server.on(
            "workspace/symbol",
            json!([
                symbol("CartPool", 23, sdk, 1, 5),
                symbol("Cart", 23, sdk, 3, 5),
                symbol("MyCart", 23, &uri, 4, 5),
                symbol("CartStore", 23, &uri, 2, 5),
            ]),
        );
        let found = fixture.dispatch("navigate.class.goTo", json!({ "query": "Cart", "waitMs": 5000 })).unwrap();
        let names: Vec<&str> =
            found["classes"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Cart", "CartStore", "CartPool", "MyCart"], "{found}");
    }
}
