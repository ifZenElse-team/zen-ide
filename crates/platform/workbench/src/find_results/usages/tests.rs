use std::time::Duration;

use serde_json::{Value, json};

use crate::language::testing::{Fixture, fixture};

const MAIN: &str =
    "package main\n\nvar Version = \"1\"\n\nfunc main() {\n\tVersion = \"2\"\n\tprintln(Version, Version)\n}\n";
const UTIL: &str = "package util\n\nvar _ = app.Version\n";
const TEST: &str = "package main\n\nfunc TestV() { _ = Version }\n";

fn range(text: &str, nth: usize) -> Value {
    let at = text.match_indices("Version").nth(nth).unwrap().0;
    json!({
        "start": zen_lsp::position::position(text, at),
        "end": zen_lsp::position::position(text, at + "Version".len()),
    })
}

fn location(fixture: &Fixture, name: &str, text: &str, nth: usize) -> Value {
    json!({ "uri": zen_lsp::position::file_uri(&fixture.path(name)), "range": range(text, nth) })
}

fn chars(text: &str, nth: usize) -> usize {
    text[..text.match_indices("Version").nth(nth).unwrap().0].chars().count()
}

fn served() -> Fixture {
    served_with(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "referencesProvider": true,
        "documentSymbolProvider": true,
        "documentHighlightProvider": true,
    }))
}

fn served_with(capabilities: Value) -> Fixture {
    zen_i18n::set_language(Some("en"));
    let mut fixture =
        fixture(&[("go.mod", "module app\n"), ("main.go", MAIN), ("util/util.go", UTIL), ("main_test.go", TEST)]);
    fixture.server.set_capabilities(capabilities);
    fixture.server.on(
        "textDocument/documentSymbol",
        json!([{ "name": "Version", "kind": 13, "range": range(MAIN, 0), "selectionRange": range(MAIN, 0) }]),
    );
    let answer = json!([
        location(&fixture, "util/util.go", UTIL, 0),
        location(&fixture, "main.go", MAIN, 3),
        location(&fixture, "main.go", MAIN, 1),
        location(&fixture, "main.go", MAIN, 2),
        location(&fixture, "main_test.go", TEST, 0),
    ]);
    fixture.server.on("textDocument/references", answer);
    let highlights: Vec<Value> = [(0, 3), (1, 3), (2, 2), (3, 2)]
        .into_iter()
        .map(|(nth, kind)| json!({ "range": range(MAIN, nth), "kind": kind }))
        .collect();
    fixture.server.on("textDocument/documentHighlight", json!(highlights));
    fixture.open("main.go");
    fixture.dispatch("search.results.setOption", json!({ "option": "module", "on": false })).unwrap();
    fixture
}

fn labels(node: &Value, depth: usize, out: &mut Vec<String>) {
    let mut text = format!("{}{}", "  ".repeat(depth), node["label"].as_str().unwrap());
    if node["kind"] == "usage" {
        text = format!("{}{} {}", "  ".repeat(depth), node["line"], node["label"].as_str().unwrap());
    } else if node["kind"] != "target" && node["kind"] != "targets" {
        text.push_str(&format!(" ({})", node["count"]));
    }
    out.push(text);
    for child in node["children"].as_array().into_iter().flatten() {
        labels(child, depth + 1, out);
    }
}

fn shape(fixture: &mut Fixture, tab: &Value) -> Vec<String> {
    let tree = fixture.dispatch("search.results.tree", json!({ "tab": tab, "waitMs": 5000 })).unwrap();
    let mut out = Vec::new();
    for node in tree["nodes"].as_array().unwrap() {
        labels(node, 0, &mut out);
    }
    out
}

fn find(fixture: &mut Fixture, nth: usize, extra: Value) -> Value {
    let mut params = json!({ "offset": chars(MAIN, nth), "waitMs": 5000 });
    for (key, value) in extra.as_object().unwrap() {
        params[key] = value.clone();
    }
    fixture.dispatch("navigate.usages.find", params).unwrap()
}

#[test]
fn find_usages_lists_a_tab_grouped_by_usage_type_with_read_and_write_from_the_server() {
    let mut fixture = served();
    let state = find(&mut fixture, 2, json!({}));
    assert_eq!((state["pending"].clone(), state["total"].clone()), (json!(false), json!(5)), "{state}");
    let tab = state["tab"].clone();
    assert!(tab.is_u64(), "the tab is told: {state}");
    let tree = fixture.dispatch("search.results.tree", json!({ "tab": tab, "depth": 0 })).unwrap();
    assert_eq!(
        (&tree["title"], &tree["kind"], &tree["accessKnown"], &tree["usages"]["kind"], &tree["usages"]["scope"]),
        (&json!("Usages of Version"), &json!("usages"), &json!(true), &json!("variable"), &json!("projectFiles")),
        "{tree}"
    );
    assert_eq!(
        shape(&mut fixture, &tab),
        [
            "Targets",
            "  Variable Version",
            "Usages in Project Files (5)",
            "  Unclassified (2)",
            "    util (1)",
            "      util.go (1)",
            "        3 var _ = app.Version",
            "    main_test.go (1)",
            "      3 func TestV() { _ = Version }",
            "  Value read (2)",
            "    main.go (2)",
            "      7 println(Version, Version)",
            "  Value write (1)",
            "    main.go (1)",
            "      6 Version = \"2\"",
        ],
        "util.go and main_test.go have no access: the server answered for main.go only"
    );
    let highlights = fixture.server.requests("textDocument/documentHighlight");
    assert_eq!(highlights.len(), 3, "one request per file: {highlights:?}");

    fixture.dispatch("search.results.setOption", json!({ "option": "showWrite", "on": false })).unwrap();
    let shown = shape(&mut fixture, &tab);
    assert_eq!(shown[2], "Usages in Project Files (4)");
    assert!(!shown.iter().any(|line| line.contains("Value write")), "{shown:?}");
    fixture.dispatch("search.results.setOption", json!({ "option": "showRead", "on": false })).unwrap();
    let shown = shape(&mut fixture, &tab);
    assert_eq!(shown[2], "Usages in Project Files (2)", "the usages of unknown access stay: {shown:?}");
    fixture.dispatch("search.results.setOption", json!({ "option": "showRead", "on": true })).unwrap();
    fixture.dispatch("search.results.setOption", json!({ "option": "showWrite", "on": true })).unwrap();

    fixture.dispatch("search.results.setOption", json!({ "option": "mergeSameLine", "on": false })).unwrap();
    fixture.dispatch("search.results.setOption", json!({ "option": "usageType", "on": false })).unwrap();
    fixture.dispatch("search.results.setOption", json!({ "option": "productionTest", "on": true })).unwrap();
    assert_eq!(
        shape(&mut fixture, &tab)[2..],
        [
            "Usages in Project Files (5)",
            "  Production (4)",
            "    util (1)",
            "      util.go (1)",
            "        3 var _ = app.Version",
            "    main.go (3)",
            "      6 Version = \"2\"",
            "      7 println(Version, Version)",
            "      7 println(Version, Version)",
            "  Test (1)",
            "    main_test.go (1)",
            "      3 func TestV() { _ = Version }",
        ]
    );
    let listed = fixture.dispatch("navigate.usages.list", json!({})).unwrap();
    assert_eq!(
        (listed["total"].clone(), listed["tab"].clone()),
        (json!(5), tab),
        "navigate.usages.list still reads it"
    );
}

#[test]
fn tabs_are_replaced_unless_pinned_or_new_and_rerun_asks_the_server_again() {
    let mut fixture = served();
    let first = find(&mut fixture, 1, json!({}))["tab"].clone();
    let second = find(&mut fixture, 2, json!({}))["tab"].clone();
    let list = fixture.dispatch("search.results.list", json!({})).unwrap();
    assert_eq!(list["tabs"].as_array().unwrap().len(), 1, "the second took the first's place: {list}");
    assert_ne!(first, second);
    fixture.dispatch("search.results.pin", json!({ "tab": second, "on": true })).unwrap();
    find(&mut fixture, 3, json!({}));
    find(&mut fixture, 3, json!({ "newTab": true }));
    let list = fixture.dispatch("search.results.list", json!({})).unwrap();
    let kinds: Vec<(&Value, &Value)> =
        list["tabs"].as_array().unwrap().iter().map(|tab| (&tab["kind"], &tab["pinned"])).collect();
    assert_eq!(
        kinds,
        [(&json!("usages"), &json!(true)), (&json!("usages"), &json!(false)), (&json!("usages"), &json!(false))],
        "pinned, then Find Usages, then Open in new tab: {list}"
    );

    let asked = fixture.server.requests("textDocument/references").len();
    fixture.server.on("textDocument/references", json!([location(&fixture, "main.go", MAIN, 1)]));
    let rerun = fixture.dispatch("search.results.rerun", json!({ "tab": second, "waitMs": 5000 })).unwrap();
    assert_eq!((rerun["total"].clone(), rerun["done"].clone(), rerun["tab"].clone()), (json!(1), json!(true), second));
    assert_eq!(fixture.server.requests("textDocument/references").len(), asked + 1);
    let pinned = fixture.dispatch("search.results.list", json!({})).unwrap()["tabs"][0]["pinned"].clone();
    assert_eq!(pinned, true, "Rerun keeps the pin");
}

#[test]
fn skip_results_tab_with_one_usage_opens_it_in_the_editor() {
    let mut fixture = served();
    fixture.server.on("textDocument/references", json!([location(&fixture, "util/util.go", UTIL, 0)]));
    let options = fixture.dispatch("navigate.usagesSettings.set", json!({ "skipSingle": true })).unwrap();
    assert_eq!(options["options"]["skipSingle"], true);
    let state = find(&mut fixture, 1, json!({}));
    assert_eq!((state["total"].clone(), state.get("tab").cloned()), (json!(1), None), "{state}");
    assert_eq!(fixture.dispatch("search.results.list", json!({})).unwrap()["tabs"], json!([]));
    let workbench = fixture.session.state::<crate::Workbench>().unwrap();
    let document = workbench.active_document().unwrap();
    assert_eq!(document.path(), fixture.path("util/util.go"));
    assert_eq!(document.selections().primary().head, chars(UTIL, 0));

    fixture.server.on(
        "textDocument/references",
        json!([location(&fixture, "util/util.go", UTIL, 0), location(&fixture, "main.go", MAIN, 1)]),
    );
    fixture.dispatch("file.document.open", json!({ "path": fixture.path("main.go") })).unwrap();
    let state = find(&mut fixture, 1, json!({}));
    assert!(state["tab"].is_u64(), "two usages make a tab: {state}");
}

#[test]
fn recent_find_usages_lists_the_searches_and_runs_one_again_where_the_symbol_moved() {
    let mut fixture = served();
    find(&mut fixture, 1, json!({}));
    find(&mut fixture, 2, json!({ "scope": "currentFile" }));
    let recent = fixture.dispatch("navigate.usages.recent", json!({})).unwrap();
    let searches = recent["searches"].as_array().unwrap();
    assert_eq!(searches.len(), 2);
    assert_eq!(
        (&searches[0]["title"], &searches[0]["scope"], &searches[0]["scopeName"], &searches[0]["relative"]),
        (&json!("Variable Version"), &json!("currentFile"), &json!("Current File"), &json!("main.go")),
        "{recent}"
    );
    assert_eq!(searches[1]["offset"], chars(MAIN, 1));

    fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
    fixture.dispatch("editor.edit.insert", json!({ "text": "// moved\n" })).unwrap();
    let asked = fixture.server.requests("textDocument/references").len();
    let state = fixture.dispatch("navigate.usages.openRecent", json!({ "index": 1, "waitMs": 5000 })).unwrap();
    assert!(state["tab"].is_u64(), "{state}");
    let sent = fixture.server.wait_for("textDocument/references", asked + 1, Duration::from_secs(5));
    assert_eq!(sent.last().unwrap()["position"]["line"], 6, "the symbol is found a line lower");
    let error = fixture.dispatch("navigate.usages.openRecent", json!({ "index": 9 })).unwrap_err().to_string();
    assert!(error.contains(&zen_i18n::t!("workbench-usages-no-recent")), "{error}");
}

#[test]
fn the_settings_dialog_changes_the_options_only_when_find_keeps_them() {
    let mut fixture = served();
    let dialog = fixture.dispatch("navigate.usages.settings", json!({ "offset": chars(MAIN, 2) })).unwrap();
    assert_eq!(
        (&dialog["open"], &dialog["showUsages"], &dialog["targetName"], &dialog["options"]),
        (
            &json!(true),
            &json!(false),
            &json!("Variable Version"),
            &json!({ "scope": "projectFiles", "newTab": false, "skipSingle": false })
        ),
        "{dialog}"
    );
    let scopes: Vec<&str> =
        dialog["scopes"].as_array().unwrap().iter().map(|scope| scope["name"].as_str().unwrap()).collect();
    assert_eq!(scopes, ["Project Files", "All Places", "Open Files", "Current File"]);
    fixture.dispatch("navigate.usagesSettings.set", json!({ "scope": "openFiles", "newTab": true })).unwrap();
    fixture.dispatch("navigate.usagesSettings.cancel", json!({})).unwrap();
    let closed = fixture.dispatch("navigate.usagesSettings.get", json!({})).unwrap();
    assert_eq!((closed["open"].clone(), closed["options"]["scope"].clone()), (json!(false), json!("projectFiles")));
    assert!(fixture.dispatch("navigate.usagesSettings.find", json!({})).is_err(), "nothing to find when closed");

    fixture.dispatch("navigate.usages.settings", json!({ "offset": chars(MAIN, 2) })).unwrap();
    fixture.dispatch("navigate.usagesSettings.set", json!({ "scope": "currentFile", "newTab": true })).unwrap();
    fixture.dispatch("navigate.usagesSettings.find", json!({ "waitMs": 5000 })).unwrap();
    let kept = fixture.dispatch("navigate.usagesSettings.get", json!({})).unwrap();
    assert_eq!(kept["options"], json!({ "scope": "currentFile", "newTab": true, "skipSingle": false }));
    let tree = fixture.dispatch("search.results.tree", json!({ "depth": 0, "waitMs": 5000 })).unwrap();
    assert_eq!((tree["nodes"][1]["label"].clone(), tree["total"].clone()), (json!("Usages in Current File"), json!(3)));
    find(&mut fixture, 1, json!({}));
    let list = fixture.dispatch("search.results.list", json!({})).unwrap();
    assert_eq!(list["tabs"].as_array().unwrap().len(), 2, "Open in new tab is kept: {list}");
}

fn many(count: usize) -> Fixture {
    zen_i18n::set_language(Some("en"));
    let names: Vec<String> = (0..count).map(|index| format!("p{index}/use.go")).collect();
    let mut files = vec![("go.mod", "module app\n"), ("main.go", MAIN)];
    files.extend(names.iter().map(|name| (name.as_str(), UTIL)));
    let mut fixture = fixture(&files);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "referencesProvider": true,
        "documentHighlightProvider": true,
    }));
    let answer: Vec<Value> = names.iter().map(|name| location(&fixture, name, UTIL, 0)).collect();
    fixture.server.on("textDocument/references", json!(answer));
    fixture.server.reply("textDocument/documentHighlight", zen_lsp::fake::Reply::Hold);
    fixture.open("main.go");
    fixture
}

#[test]
fn the_access_is_asked_in_the_background_a_few_files_at_a_time_and_closing_the_tab_cancels_it() {
    let mut fixture = many(12);
    let started = std::time::Instant::now();
    let state = find(&mut fixture, 1, json!({ "waitMs": 300 }));
    assert!(started.elapsed() < Duration::from_secs(60), "the server's silence does not hold the window");
    let tab = state["tab"].clone();
    assert!(tab.is_u64(), "the tab is made before the access is told: {state}");
    let tree = fixture.dispatch("search.results.tree", json!({ "tab": tab, "depth": 1 })).unwrap();
    assert_eq!((&tree["total"], &tree["accessPending"]), (&json!(12), &json!(true)), "{tree}");
    let asked = fixture.server.wait_for("textDocument/documentHighlight", 8, Duration::from_secs(5)).len();
    std::thread::sleep(Duration::from_millis(100));
    fixture.dispatch("search.results.list", json!({})).unwrap();
    assert_eq!(fixture.server.requests("textDocument/documentHighlight").len(), asked);
    assert_eq!(asked, 8, "at most 8 requests wait at a time");

    fixture.dispatch("search.results.close", json!({ "tab": tab })).unwrap();
    let cancelled = fixture.server.wait_for("$/cancelRequest", 8, Duration::from_secs(5));
    assert_eq!(cancelled.len(), 8, "closing the tab cancels what waits: {cancelled:?}");
    fixture.dispatch("search.results.list", json!({})).unwrap();
    assert_eq!(fixture.server.requests("textDocument/documentHighlight").len(), 8, "and asks no more");
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let closed = fixture.server.wait_for("textDocument/didClose", 8, Duration::from_secs(5));
    assert_eq!(closed.len(), 8, "the files lent for it are given back: {closed:?}");
}

#[test]
fn rerun_and_stop_cancel_the_access_still_asked() {
    let mut fixture = many(3);
    let tab = find(&mut fixture, 1, json!({ "waitMs": 300 }))["tab"].clone();
    fixture.server.wait_for("textDocument/documentHighlight", 3, Duration::from_secs(5));
    fixture.dispatch("search.results.rerun", json!({ "tab": tab, "waitMs": 300 })).unwrap();
    let cancelled = fixture.server.wait_for("$/cancelRequest", 3, Duration::from_secs(5));
    assert_eq!(cancelled.len(), 3, "the rerun forgets the old requests: {cancelled:?}");
    fixture.server.wait_for("textDocument/documentHighlight", 6, Duration::from_secs(5));
    let stopped = fixture.dispatch("search.results.stop", json!({ "tab": tab })).unwrap();
    assert_eq!(stopped.get("accessPending"), None, "{stopped}");
    let cancelled = fixture.server.wait_for("$/cancelRequest", 6, Duration::from_secs(5));
    assert_eq!(cancelled.len(), 6, "{cancelled:?}");
    let tree = fixture.dispatch("search.results.tree", json!({ "tab": tab, "depth": 1 })).unwrap();
    assert_eq!(tree["nodes"][1]["children"][0]["label"], "Unclassified", "{tree}");
}

#[test]
fn a_server_without_document_highlights_is_not_asked_and_its_usages_stay_unclassified() {
    let mut fixture = served_with(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "referencesProvider": true,
        "documentSymbolProvider": true,
    }));
    let tab = find(&mut fixture, 1, json!({}))["tab"].clone();
    let tree = fixture.dispatch("search.results.tree", json!({ "tab": tab, "waitMs": 5000 })).unwrap();
    assert_eq!(tree["accessKnown"], Value::Null, "{tree}");
    assert!(fixture.server.requests("textDocument/documentHighlight").is_empty());
    fixture.dispatch("search.results.setOption", json!({ "option": "showWrite", "on": false })).unwrap();
    fixture.dispatch("search.results.setOption", json!({ "option": "showRead", "on": false })).unwrap();
    let tree = fixture.dispatch("search.results.tree", json!({ "tab": tab })).unwrap();
    assert_eq!(tree["nodes"][1]["count"], 5, "nothing is filtered: {tree}");
}

#[test]
fn a_moved_symbol_is_found_on_its_line_not_as_a_namesake_nearer_to_where_it_was() {
    let mut fixture = served();
    let tab = find(&mut fixture, 0, json!({}))["tab"].clone();
    fixture.dispatch("search.results.pin", json!({ "tab": tab, "on": true })).unwrap();
    fixture.dispatch("editor.selection.set", json!({ "selections": [{ "head": chars(MAIN, 0) - 4 }] })).unwrap();
    fixture.dispatch("editor.edit.insert", json!({ "text": "func f() { Version := 3; _ = Version }\n\n" })).unwrap();
    let asked = fixture.server.requests("textDocument/references").len();
    fixture.dispatch("navigate.usages.openRecent", json!({ "index": 0, "waitMs": 5000 })).unwrap();
    let sent = fixture.server.wait_for("textDocument/references", asked + 1, Duration::from_secs(5));
    let at = sent.last().unwrap()["position"].clone();
    assert_eq!((&at["line"], &at["character"]), (&json!(4), &json!(4)), "var Version, not the local: {at}");

    fixture.dispatch("search.results.rerun", json!({ "tab": tab, "waitMs": 5000 })).unwrap();
    let sent = fixture.server.wait_for("textDocument/references", asked + 2, Duration::from_secs(5));
    assert_eq!(sent.last().unwrap()["position"], at, "Rerun asks where the symbol is now");
    let list = fixture.dispatch("search.results.list", json!({})).unwrap();
    let rerun = list["tabs"].as_array().unwrap().iter().find(|one| one["tab"] == tab).unwrap().clone();
    assert_eq!(rerun["usages"]["offset"], json!(chars(MAIN, 0) + "func f() { Version := 3; _ = Version }\n\n".len()));
}

#[test]
fn nearest_prefers_the_line_and_column_of_the_symbol() {
    let text = "a := Version\nx := Version + Version\n";
    let second = text.rfind("Version").unwrap();
    let target = crate::find_results::UsagesTarget {
        path: std::path::PathBuf::new(),
        offset: second,
        symbol: "Version".to_owned(),
        kind: crate::language::show_usages::DeclarationKind::Variable,
        scope: crate::language::show_usages::UsagesScope::ProjectFiles,
        line: "x := Version + Version".to_owned(),
        column: "x := Version + ".len(),
    };
    assert_eq!(super::nearest(text, &target), Some(second), "unchanged");
    let moved = format!("// one\n// two\n{text}");
    assert_eq!(
        super::nearest(&moved, &target),
        Some("// one\n// two\n".len() + second),
        "the same occurrence of its line"
    );
    assert_eq!(super::line_context("\t\tx := Version\n", 7), ("x := Version".to_owned(), 5));
}
