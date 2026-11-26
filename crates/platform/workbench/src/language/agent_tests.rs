use std::time::Duration;

use serde_json::{Value, json};

use super::testing::{self, Fixture};

const MODEL: &str = "package model\n\n// Store keeps items; see Store.Get.\ntype Store struct {\n\tcount int\n}\n\nfunc (s *Store) Get() int { return s.count }\n\nvar label = \"Store\"\n";
const MAIN: &str =
    "package main\n\nimport \"example.com/shop/model\"\n\nfunc main() {\n\tvar s model.Store\n\t_ = s.Get()\n}\n";

fn location(fixture: &Fixture, file: &str, text: &str, word: &str, nth: usize) -> Value {
    let (line, character) = place(text, word, nth);
    json!({ "uri": zen_lsp::position::file_uri(&fixture.path(file)), "range": {
        "start": { "line": line, "character": character },
        "end": { "line": line, "character": character + word.len() as u32 } } })
}

fn place(text: &str, word: &str, nth: usize) -> (u32, u32) {
    let at = text.match_indices(word).nth(nth).unwrap().0;
    let line = text[..at].matches('\n').count() as u32;
    let character = (at - text[..at].rfind('\n').map_or(0, |newline| newline + 1)) as u32;
    (line, character)
}

fn project() -> Fixture {
    let fixture =
        testing::fixture(&[("main.go", MAIN), ("model/model.go", MODEL), ("go.mod", "module example.com/shop\n")]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "workspaceSymbolProvider": true,
        "referencesProvider": true,
        "documentSymbolProvider": true
    }));
    let model = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
    let symbol = |name: &str, kind: u64, nth: usize| {
        let (line, character) = place(MODEL, name.rsplit('.').next().unwrap(), nth);
        json!({ "name": name, "kind": kind, "containerName": "example.com/shop/model", "location": { "uri": model,
            "range": { "start": { "line": line, "character": character }, "end": { "line": line, "character": character + 3 } } } })
    };
    let symbols = json!([symbol("Store", 23, 2), symbol("Store.Get", 6, 1), symbol("Store.count", 8, 0)]);
    fixture.server.on_with("workspace/symbol", move |_| symbols.clone());
    fixture
}

#[test]
fn symbols_and_types_come_as_rows_when_compact() {
    let mut fixture = project();
    let full = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "Store", "waitMs": 5000 })).unwrap();
    let rows =
        fixture.dispatch("navigate.symbol.goTo", json!({ "query": "Store", "compact": true, "waitMs": 5000 })).unwrap();
    assert_eq!(rows["symbols"], json!([]), "{rows}");
    assert_eq!(rows["rows"][0], "model/model.go:4:6 struct Store", "{rows}");
    let get =
        fixture.dispatch("navigate.symbol.goTo", json!({ "query": "Get", "compact": true, "waitMs": 5000 })).unwrap();
    assert_eq!(get["rows"], json!(["model/model.go:8:17 method Store.Get"]), "{get}");
    assert!(rows.to_string().len() * 2 < full.to_string().len(), "{} vs {}", rows, full);
    let types =
        fixture.dispatch("navigate.class.goTo", json!({ "query": "Store", "compact": true, "waitMs": 5000 })).unwrap();
    assert_eq!(types["rows"], json!(["model/model.go:4:6 struct Store"]), "{types}");
}

#[test]
fn a_file_structure_comes_as_indented_rows_when_compact() {
    let mut fixture = project();
    let range = |line: u32, start: u32, end: u32| json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } });
    fixture.server.on(
        "textDocument/documentSymbol",
        json!([{ "name": "Store", "detail": "struct{...}", "kind": 23, "range": range(3, 0, 20), "selectionRange": range(3, 5, 10),
            "children": [{ "name": "count", "detail": "int", "kind": 8, "range": range(4, 1, 10), "selectionRange": range(4, 1, 6) }] }]),
    );
    fixture.open("model/model.go");
    let path = fixture.path("model/model.go");
    let structure = loop {
        let answer =
            fixture.dispatch("editor.structure.get", json!({ "path": path, "compact": true, "waitMs": 2000 })).unwrap();
        if answer["rows"].is_array() && answer["source"] != "outline" {
            break answer;
        }
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
    };
    assert_eq!(structure["rows"], json!(["4:6 struct Store struct{...}", "  5:2 field count int"]), "{structure}");
    assert_eq!(structure["symbols"], json!([]));
}

fn diagnostic(line: u32, severity: u32, message: &str, source: &str) -> Value {
    json!({ "range": { "start": { "line": line, "character": 1 }, "end": { "line": line, "character": 4 } },
        "severity": severity, "message": message, "source": source, "code": "X1" })
}

fn wait_for_problems(fixture: &mut Fixture, path: &std::path::Path) -> Value {
    for _ in 0..500 {
        let answer = fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap();
        if answer.is_array() {
            return answer;
        }
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("no problems came");
}

#[test]
fn problems_are_kept_by_severity_source_and_place_and_come_as_rows() {
    let mut fixture = project();
    fixture.server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", |params| {
        json!({ "uri": params["textDocument"]["uri"], "diagnostics": [
            diagnostic(5, 1, "undefined: x", "compiler"),
            diagnostic(6, 2, "unused result\nmore lines", "vet"),
            diagnostic(6, 4, "could be simpler", "simplifier"),
        ] })
    });
    fixture.open("main.go");
    fixture.open("model/model.go");
    let main = fixture.path("main.go");
    wait_for_problems(&mut fixture, &main);
    let model = fixture.path("model/model.go");
    wait_for_problems(&mut fixture, &model);

    let errors = fixture.dispatch("problems.file.list", json!({ "path": main, "severity": "error" })).unwrap();
    assert_eq!(errors["checked"], true, "{errors}");
    assert_eq!(errors["problems"].as_array().unwrap().len(), 1);
    assert_eq!(errors["problems"][0]["message"], "undefined: x");
    let rows = fixture
        .dispatch("problems.file.list", json!({ "path": main, "severity": "warning", "compact": true }))
        .unwrap();
    assert_eq!(rows["problems"], json!(["6:2 error undefined: x [compiler X1]", "7:2 warning unused result [vet X1]"]));
    let limited = fixture.dispatch("problems.file.list", json!({ "path": main, "limit": 1, "compact": true })).unwrap();
    assert_eq!((limited["problems"].as_array().unwrap().len(), limited["truncated"].as_u64()), (1, Some(2)));
    let vet = fixture.dispatch("problems.file.list", json!({ "path": main, "sources": ["VET"] })).unwrap();
    assert_eq!(vet["problems"][0]["source"], "vet");
    assert!(
        fixture.dispatch("problems.file.list", json!({ "path": main })).unwrap().is_array(),
        "unfiltered as before"
    );

    let project = fixture.dispatch("problems.project.list", json!({ "severity": "error", "compact": true })).unwrap();
    assert_eq!(
        project["problems"],
        json!(["main.go:6:2 error undefined: x [compiler X1]", "model/model.go:6:2 error undefined: x [compiler X1]"])
    );
    assert_eq!(project["total"], 2);
    let only_model =
        fixture.dispatch("problems.project.list", json!({ "paths": ["model"], "severity": "error" })).unwrap();
    assert_eq!(only_model["problems"][0]["path"], "model/model.go", "{only_model}");
    let without = fixture.dispatch("problems.project.list", json!({ "exclude": ["*.go"] })).unwrap();
    assert_eq!(without["total"], 0);
    let capped = fixture.dispatch("problems.project.list", json!({ "limit": 4, "compact": true })).unwrap();
    assert_eq!((capped["problems"].as_array().unwrap().len(), capped["truncated"].as_u64()), (4, Some(2)));
}

#[test]
fn usages_are_found_by_name_and_by_symbol_with_the_text_matches() {
    let mut fixture = project();
    let usages = json!([location(&fixture, "main.go", MAIN, "Store", 0)]);
    fixture.server.on("textDocument/references", usages);
    fixture.open("main.go");
    let found = fixture
        .dispatch(
            "navigate.usages.find",
            json!({ "path": "model/model.go", "line": 4, "name": "Store", "waitMs": 5000, "compact": true }),
        )
        .unwrap();
    assert_eq!(found["rows"], json!(["main.go:6:14 var s model.Store"]), "{found}");
    let sent = fixture.server.requests("textDocument/references");
    assert_eq!(sent[0]["position"], json!({ "line": 3, "character": 5 }));
    let found = fixture
        .dispatch(
            "navigate.usages.find",
            json!({ "symbol": "Store", "waitMs": 5000, "includeTextMatches": true, "compact": true }),
        )
        .unwrap();
    assert_eq!(found["total"], 1, "{found}");
    assert_eq!(
        found["textMatches"],
        json!([
            "model/model.go:3:4 // Store keeps items; see Store.Get.",
            "model/model.go:3:27 // Store keeps items; see Store.Get.",
            "model/model.go:8:10 func (s *Store) Get() int { return s.count }",
            "model/model.go:10:14 var label = \"Store\""
        ]),
        "what the server did not count, not its usage or the declaration: {found}"
    );
    let missing = fixture.dispatch("navigate.usages.find", json!({ "path": "main.go", "line": 2, "name": "Store" }));
    assert!(missing.unwrap_err().to_string().contains("Store"));
}

#[test]
fn a_file_is_checked_again_when_its_server_publishes_only_what_changed() {
    let mut fixture = project();
    let last = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let publish = move |params: &Value| {
        let diagnostics = json!([diagnostic(0, 2, "old warning", "vet")]);
        let text = diagnostics.to_string();
        let mut sent = last.lock().unwrap();
        let unchanged = *sent == text;
        *sent = text;
        let uri = if unchanged { json!("file:///elsewhere.go") } else { params["textDocument"]["uri"].clone() };
        json!({ "uri": uri, "version": params["textDocument"]["version"], "diagnostics": diagnostics })
    };
    let again = publish.clone();
    fixture.server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", publish);
    fixture.server.publish_on("textDocument/didChange", "textDocument/publishDiagnostics", again);
    fixture.open("main.go");
    let main = fixture.path("main.go");
    wait_for_problems(&mut fixture, &main);
    let edit = json!({ "path": "main.go", "oldText": "_ = s.Get()", "newText": "_ = s.Get() " });
    fixture.dispatch("editor.text.replace", edit).unwrap();
    let started = std::time::Instant::now();
    let answer = loop {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let answer = fixture.dispatch("problems.file.list", json!({ "path": main })).unwrap();
        if answer.is_array() || started.elapsed() > Duration::from_secs(8) {
            break answer;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(answer.is_array(), "a server that publishes nothing new left the file checking: {answer}");
    assert!(started.elapsed() < Duration::from_secs(5), "within a few seconds: {:?}", started.elapsed());
}

#[test]
fn an_edit_answers_the_problems_it_brought() {
    let mut fixture = project();
    let publish = |params: &Value| {
        let text = params["contentChanges"].to_string() + &params["textDocument"]["text"].to_string();
        let mut diagnostics = vec![diagnostic(0, 2, "old warning", "vet")];
        if text.contains("broken") {
            diagnostics.push(diagnostic(6, 1, "undefined: broken", "compiler"));
        }
        json!({ "uri": params["textDocument"]["uri"], "version": params["textDocument"]["version"], "diagnostics": diagnostics })
    };
    fixture.server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", publish);
    fixture.server.publish_on("textDocument/didChange", "textDocument/publishDiagnostics", publish);
    fixture.open("main.go");
    let main = fixture.path("main.go");
    wait_for_problems(&mut fixture, &main);
    let edit = json!({ "path": "main.go", "oldText": "_ = s.Get()", "newText": "_ = broken", "checkProblems": true });
    let answer = fixture.dispatch("editor.text.replace", edit).unwrap();
    assert_eq!(answer["newProblems"], json!(["7:2 error undefined: broken [compiler X1]"]), "{answer}");
    assert!(answer.get("unchecked").is_none(), "{answer}");
    assert!(answer.to_string().len() < 120, "short: {answer}");
    let fine = json!({ "path": "main.go", "oldText": "_ = broken", "newText": "_ = s.Get()", "checkProblems": true });
    assert_eq!(fixture.dispatch("editor.text.replace", fine).unwrap()["newProblems"], json!([]));
    let plain = fixture
        .dispatch("editor.text.replace", json!({ "path": "main.go", "oldText": "var s", "newText": "var t" }))
        .unwrap();
    assert!(plain.get("newProblems").is_none(), "only when asked: {plain}");
    let batch = json!({ "files": [{ "path": "main.go", "oldText": "_ = s.Get()", "newText": "_ = broken" }], "checkProblems": true });
    let answer = fixture.dispatch("editor.text.batch", batch).unwrap();
    assert_eq!(answer["files"][0]["newProblems"], json!(["7:2 error undefined: broken [compiler X1]"]), "{answer}");
}

#[test]
fn an_edit_saved_in_the_same_call_tells_the_server_of_the_save() {
    let mut fixture = testing::fixture(&[("main.go", MAIN)]);
    fixture.open("main.go");
    let path = fixture.path("main.go");
    fixture
        .dispatch(
            "editor.text.replace",
            json!({ "path": path, "oldText": "_ = s.Get()", "newText": "_ = 1", "save": true }),
        )
        .unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let saves = fixture.server.wait_for("textDocument/didSave", 1, Duration::from_secs(10));
    assert_eq!(saves.len(), 1, "{:?}", fixture.server.received());
    fixture.dispatch("file.document.save", json!({ "path": path })).unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let saves = fixture.server.wait_for("textDocument/didSave", 2, Duration::from_secs(10));
    assert!(saves.len() <= 2, "{saves:?}");
}
