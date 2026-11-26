use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_lsp::fake::FakeServer;

use super::testing::{Fixture, fixture};
use super::{Entry, ServerSpec};

const MAIN: &str = "package main\n\nfunc main() {\n\tx := 1\n}\n";

fn diagnostic(line: u32, code: &str, source: &str) -> Value {
    json!({
        "range": { "start": { "line": line, "character": 1 }, "end": { "line": line, "character": 2 } },
        "severity": 1, "code": code, "source": source, "message": format!("{source}: {code}")
    })
}

fn edit_at_start(uri: &Value, text: &str) -> Value {
    json!({ "changes": { uri.as_str().unwrap(): [
        { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }, "newText": text }
    ] } })
}

fn with_linter(formats: bool) -> (Fixture, FakeServer) {
    let mut fixture = fixture(&[("main.go", MAIN)]);
    let lint = FakeServer::new();
    lint.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "codeActionProvider": { "codeActionKinds": ["quickfix", "source.fixAll.lint"] },
        "documentFormattingProvider": formats
    }));
    let launched = lint.clone();
    let spec = ServerSpec {
        language_id: "lint".to_owned(),
        extensions: vec!["go".to_owned()],
        launch: Arc::new(move |_| Ok(launched.launcher())),
        initialization_options: json!({}),
    };
    fixture.with(|languages, _| {
        let mut entry = Entry::from_spec(&spec);
        entry.declaration.alongside = true;
        entry.declaration.formats = formats;
        entry.declaration.fix_on_save = Some(zen_lsp::servers::FixOnSave {
            kind: "source.fixAll.lint".to_owned(),
            setting: "codeQuality.lint.fixOnSave".to_owned(),
        });
        languages.entries.push(entry);
    });
    (fixture, lint)
}

fn wait_for(fixture: &mut Fixture, done: impl Fn(&mut Fixture) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done(fixture) {
        assert!(Instant::now() < deadline, "timed out");
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn text(fixture: &Fixture) -> String {
    let workbench = fixture.session.state::<crate::Workbench>().unwrap();
    workbench.active_document().unwrap().text().to_owned()
}

#[test]
fn both_servers_get_the_file_and_their_problems_are_merged() {
    let (mut fixture, lint) = with_linter(false);
    fixture.server.publish_on(
        "textDocument/didOpen",
        "textDocument/publishDiagnostics",
        |params| json!({ "uri": params["textDocument"]["uri"], "diagnostics": [diagnostic(3, "unused", "compiler")] }),
    );
    lint.publish_on(
        "textDocument/didOpen",
        "textDocument/publishDiagnostics",
        |params| json!({ "uri": params["textDocument"]["uri"], "diagnostics": [diagnostic(2, "style", "lint")] }),
    );
    fixture.open("main.go");
    let path = fixture.path("main.go");
    wait_for(&mut fixture, |fixture| {
        let answer = fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap();
        super::listed_problems(&answer).len() == 2
    });
    let answer = fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap();
    let sources: Vec<String> =
        super::listed_problems(&answer).iter().map(|problem| problem["source"].as_str().unwrap().to_owned()).collect();
    assert_eq!(sources, ["lint", "compiler"], "in the file's order: {answer}");
    assert_eq!(lint.requests("textDocument/didOpen").len(), 1, "the server alongside has the file open");
    assert_eq!(
        fixture.with(|languages, _| languages.server_id(&path)),
        Some("go".to_owned()),
        "lookups still go to the file's own server"
    );
}

#[test]
fn context_actions_of_both_servers_are_listed_and_applied_through_their_server() {
    let (mut fixture, lint) = with_linter(false);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "codeActionProvider": { "codeActionKinds": ["quickfix"] }
    }));
    fixture.server.on_with("textDocument/codeAction", |params| {
        let uri = params["textDocument"]["uri"].clone();
        json!([{ "title": "Language fix", "kind": "quickfix", "edit": edit_at_start(&uri, "// language\n") }])
    });
    lint.on_with("textDocument/codeAction", |params| {
        let uri = params["textDocument"]["uri"].clone();
        json!([{ "title": "Lint fix", "kind": "quickfix", "isPreferred": true, "edit": edit_at_start(&uri, "// lint\n") }])
    });
    fixture.open("main.go");
    let path = fixture.path("main.go");
    wait_for(&mut fixture, |fixture| fixture.with(|languages, _| !languages.alongside_running(&path).is_empty()));
    let list = loop {
        let list =
            fixture.dispatch("editor.intentions.list", json!({ "path": path, "offset": 0, "waitMs": 2000 })).unwrap();
        if list["pending"] == false {
            break list;
        }
    };
    let titles: Vec<&str> =
        list["actions"].as_array().unwrap().iter().filter_map(|action| action["title"].as_str()).collect();
    assert_eq!(titles, ["Lint fix", "Language fix"], "quick fixes, the preferred first: {list}");
    let id = list["actions"][0]["id"].clone();
    fixture.dispatch("editor.intentions.apply", json!({ "id": id, "waitMs": 2000 })).unwrap();
    assert!(text(&fixture).starts_with("// lint\npackage main"), "{}", text(&fixture));
    assert_eq!(lint.requests("textDocument/codeAction").len(), 1);
}

#[test]
fn fixes_on_save_fix_problems_and_formatting_go_to_the_server_alongside() {
    let (mut fixture, lint) = with_linter(true);
    lint.on_with("textDocument/codeAction", |params| {
        assert_eq!(params["context"]["only"], json!(["source.fixAll.lint"]));
        let uri = params["textDocument"]["uri"].clone();
        json!([{ "title": "Fix all", "kind": "source.fixAll.lint", "edit": edit_at_start(&uri, "// fixed\n") }])
    });
    lint.on(
        "textDocument/formatting",
        json!([{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }, "newText": "// formatted\n" }]),
    );
    fixture.open("main.go");
    let path = fixture.path("main.go");
    wait_for(&mut fixture, |fixture| fixture.with(|languages, _| !languages.alongside_running(&path).is_empty()));

    let fixed = fixture.dispatch("editor.code.fixAll", json!({ "path": path, "waitMs": 2000 })).unwrap();
    assert_eq!(fixed["changed"], true, "{fixed}");
    assert!(text(&fixture).starts_with("// fixed\n"));

    let formatted = fixture.dispatch("editor.code.reformat", json!({ "path": path, "waitMs": 2000 })).unwrap();
    assert_eq!(formatted["changed"], true, "{formatted}");
    assert!(text(&fixture).starts_with("// formatted\n// fixed\n"), "the server alongside formats");
    assert!(fixture.server.requests("textDocument/formatting").is_empty(), "not the language's server");

    fixture.dispatch("editor.edit.type", json!({ "text": "x" })).unwrap();
    fixture.dispatch("file.document.save", json!({})).unwrap();
    assert_eq!(lint.requests("textDocument/codeAction").len(), 1, "no fixes on save by default");
    fixture.session.settings_mut().set("codeQuality.lint.fixOnSave", true).unwrap();
    fixture.dispatch("editor.edit.type", json!({ "text": "y" })).unwrap();
    fixture.dispatch("file.document.save", json!({})).unwrap();
    assert_eq!(lint.requests("textDocument/codeAction").len(), 2, "the fixes on save");
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.matches("// fixed\n").count() == 2, "{saved}");
}

#[test]
fn settings_name_the_project() {
    let settings = [(
        "".to_owned(),
        json!({ "workspaceFolder": { "name": "${projectName}", "uri": "${projectUri}" }, "nodePath": null }),
    )]
    .into_iter()
    .collect();
    let root = std::path::Path::new("/work/shop");
    let expanded = super::settings_for(&settings, root);
    assert_eq!(expanded[""]["workspaceFolder"], json!({ "name": "shop", "uri": "file:///work/shop" }));
    assert_eq!(expanded[""]["nodePath"], Value::Null);
}
