use std::time::Duration;

use serde_json::{Value, json};
use zen_core::DispatchError;
use zen_lsp::fake::Reply;

use super::*;
use crate::language::testing::{Fixture, fixture};

const WAIT: Duration = Duration::from_secs(5);
const MAIN: &str = "package main\n\n// привет 😀\nfunc main() { s := \"ё😀\"; greet(s) }\n";
const LIB: &str = "package main\n\n/* ё😀 */ func greet(a string) {}\n";

fn utf16(text: &str) -> u64 {
    text.encode_utf16().count() as u64
}

fn chars(text: &str, needle: &str) -> usize {
    text[..text.find(needle).unwrap()].chars().count()
}

fn range(line: u64, start: u64, length: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": start + length } })
}

fn main_greet() -> Value {
    range(3, utf16("func main() { s := \"ё😀\"; "), 5)
}

fn lib_greet() -> Value {
    range(2, utf16("/* ё😀 */ func "), 5)
}

fn project(files: &[(&str, &str)], rename_provider: Value) -> Fixture {
    let mut fixture = fixture(files);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "renameProvider": rename_provider
    }));
    fixture.open("main.go");
    fixture
}

fn go_project() -> Fixture {
    project(&[("main.go", MAIN), ("lib.go", LIB)], json!({ "prepareProvider": true }))
}

fn uri(fixture: &Fixture, name: &str) -> String {
    zen_lsp::position::file_uri(&fixture.path(name))
}

fn rename_answer(fixture: &Fixture) -> Value {
    json!({
        "documentChanges": [
            { "textDocument": { "uri": uri(fixture, "main.go"), "version": 1 }, "edits": [{ "range": main_greet(), "newText": "hello" }] },
            { "textDocument": { "uri": uri(fixture, "lib.go"), "version": null }, "edits": [{ "range": lib_greet(), "newText": "hello" }] }
        ]
    })
}

fn prepare(fixture: &mut Fixture, offset: usize) -> RenameTarget {
    let target = fixture.dispatch("refactor.rename.prepare", json!({ "offset": offset, "waitMs": 5000 })).unwrap();
    serde_json::from_value(target).unwrap()
}

fn preview(fixture: &mut Fixture, new_name: &str) -> Value {
    let offset = chars(MAIN, "greet") + 2;
    fixture
        .dispatch("refactor.rename.preview", json!({ "offset": offset, "newName": new_name, "waitMs": 5000 }))
        .unwrap()
}

fn failure(result: Result<Value, DispatchError>) -> String {
    match result {
        Err(DispatchError::Failed { error, .. }) => error.message,
        other => panic!("expected a failure, got {other:?}"),
    }
}

fn main_text(fixture: &mut Fixture) -> (String, bool) {
    let path = fixture.path("main.go");
    fixture.with(|_, workbench| {
        let document = workbench.documents().iter().find(|document| document.path() == path).unwrap();
        (document.text().to_owned(), document.is_dirty())
    })
}

fn read(fixture: &Fixture, name: &str) -> String {
    std::fs::read_to_string(fixture.path(name)).unwrap()
}

#[test]
fn prepare_gives_the_range_and_the_placeholder_or_why_not() {
    let mut fixture = go_project();
    let at = chars(MAIN, "greet") + 2;
    let start = chars(MAIN, "greet");
    fixture.server.on("textDocument/prepareRename", json!({ "range": main_greet(), "placeholder": "greet" }));
    assert_eq!(
        prepare(&mut fixture, at),
        RenameTarget { pending: false, start, end: start + 5, placeholder: "greet".to_owned(), error: None }
    );
    let sent = &fixture.server.wait_for("textDocument/prepareRename", 1, WAIT)[0];
    assert_eq!(sent["position"]["character"], utf16("func main() { s := \"ё😀\"; gr"));

    fixture.dispatch("refactor.rename.cancel", json!({})).unwrap();
    fixture.server.on("textDocument/prepareRename", main_greet());
    assert_eq!(prepare(&mut fixture, at).placeholder, "greet");

    fixture.dispatch("refactor.rename.cancel", json!({})).unwrap();
    fixture.server.on("textDocument/prepareRename", json!({ "defaultBehavior": true }));
    let target = prepare(&mut fixture, at);
    assert_eq!((target.start, target.end, target.placeholder.as_str()), (start, start + 5, "greet"));

    fixture.dispatch("refactor.rename.cancel", json!({})).unwrap();
    fixture.server.on("textDocument/prepareRename", Value::Null);
    assert_eq!(prepare(&mut fixture, at).error, Some(t!("workbench-rename-not-here")));

    fixture.dispatch("refactor.rename.cancel", json!({})).unwrap();
    let message = "cannot rename a built-in".to_owned();
    fixture.server.reply("textDocument/prepareRename", Reply::Error { code: -32603, message: message.clone() });
    let error = prepare(&mut fixture, at).error.unwrap();
    assert!(error.contains(&message), "{error}");
}

#[test]
fn a_pending_prepare_is_polled_and_cancelled() {
    let mut fixture = go_project();
    fixture.server.reply("textDocument/prepareRename", Reply::Hold);
    let at = chars(MAIN, "greet");
    let target = fixture.dispatch("refactor.rename.prepare", json!({ "offset": at })).unwrap();
    assert_eq!(target["pending"], true);
    let target = fixture.dispatch("refactor.rename.prepare", json!({ "offset": at, "waitMs": 20 })).unwrap();
    assert_eq!(target["pending"], true);
    assert_eq!(fixture.server.requests("textDocument/prepareRename").len(), 1, "the same request is polled");
    fixture.dispatch("refactor.rename.cancel", json!({})).unwrap();
    assert_eq!(fixture.server.wait_for("$/cancelRequest", 1, WAIT).len(), 1);
}

#[test]
fn prepare_in_a_closed_file_keeps_it_in_the_server_while_it_waits() {
    let mut fixture = go_project();
    let lib = fixture.path("lib.go");
    let answer = json!({ "range": lib_greet(), "placeholder": "greet" });
    fixture.server.reply("textDocument/prepareRename", Reply::Delayed(answer, Duration::from_millis(200)));
    let at = chars(LIB, "greet") + 1;
    let target = fixture.dispatch("refactor.rename.prepare", json!({ "path": lib, "offset": at })).unwrap();
    assert_eq!(target["pending"], true);
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let params = json!({ "path": lib, "offset": at, "waitMs": 5000 });
    let target: RenameTarget =
        serde_json::from_value(fixture.dispatch("refactor.rename.prepare", params).unwrap()).unwrap();
    let start = chars(LIB, "greet");
    assert_eq!((target.start, target.end, target.placeholder.as_str()), (start, start + 5, "greet"));
    assert_eq!(fixture.server.requests("textDocument/prepareRename").len(), 1, "lib.go stayed in the server");
    assert!(fixture.server.requests("textDocument/didClose").is_empty());
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let closed = fixture.server.wait_for("textDocument/didClose", 1, WAIT);
    assert_eq!(closed[0]["textDocument"]["uri"], uri(&fixture, "lib.go"));
    assert_eq!(fixture.open_files(), [fixture.path("main.go")]);
}

#[test]
fn without_prepare_support_the_identifier_at_the_caret_is_renamed() {
    let mut fixture = project(&[("main.go", MAIN)], json!(true));
    let start = chars(MAIN, "greet");
    let target = prepare(&mut fixture, start + 5);
    assert_eq!((target.start, target.end, target.placeholder.as_str()), (start, start + 5, "greet"));
    assert_eq!(prepare(&mut fixture, chars(MAIN, "{ s") + 1).error, Some(t!("workbench-rename-not-here")));
    assert!(fixture.server.requests("textDocument/prepareRename").is_empty());

    let mut fixture = project(&[("main.go", MAIN)], Value::Null);
    assert_eq!(prepare(&mut fixture, start).error, Some(t!("workbench-rename-not-here")));
}

#[test]
fn a_preview_lists_the_changes_in_an_open_and_an_unopened_file_in_characters() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    let preview: RenamePreviewOutput = serde_json::from_value(preview(&mut fixture, "hello")).unwrap();
    let sent = &fixture.server.wait_for("textDocument/rename", 1, WAIT)[0];
    assert_eq!(sent["newName"], "hello");
    assert_eq!(sent["position"]["line"], 3);
    assert!(!preview.pending);
    assert_eq!(preview.error, None);
    assert_eq!(preview.usage_count, 2);
    let relative: Vec<&str> = preview.files.iter().map(|file| file.relative.as_str()).collect();
    assert_eq!(relative, ["lib.go", "main.go"]);
    let lib = &preview.files[0].changes[0];
    assert_eq!((lib.start, lib.end, lib.line, lib.column), (chars(LIB, "greet"), chars(LIB, "greet") + 5, 3, 15));
    assert_eq!(lib.preview, "/* ё😀 */ func greet(a string) {}");
    assert_eq!(lib.highlight, [14, 19]);
    assert_eq!(lib.new_text, "hello");
    let main = &preview.files[1].changes[0];
    assert_eq!((main.start, main.line, main.column), (chars(MAIN, "greet"), 4, 26));
    assert_eq!(main_text(&mut fixture), (MAIN.to_owned(), false), "a preview changes nothing");
    assert_eq!(read(&fixture, "lib.go"), LIB);
}

#[test]
fn the_legacy_changes_form_is_read_too() {
    let mut fixture = go_project();
    let changes = json!({ "changes": {
        uri(&fixture, "main.go"): [{ "range": main_greet(), "newText": "hello" }],
        uri(&fixture, "lib.go"): [{ "range": lib_greet(), "newText": "hello" }]
    }});
    fixture.server.on("textDocument/rename", changes);
    let preview: RenamePreviewOutput = serde_json::from_value(preview(&mut fixture, "hello")).unwrap();
    assert_eq!(preview.usage_count, 2);
    assert_eq!(preview.files[0].changes[0].start, chars(LIB, "greet"));
    assert_eq!(preview.files[1].changes[0].start, chars(MAIN, "greet"));
}

#[test]
fn apply_edits_the_open_file_writes_the_other_and_undo_reverts_both() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    let applied = fixture.dispatch("refactor.rename.apply", json!({})).unwrap();
    assert_eq!(applied["edits"], 2);
    assert_eq!(applied["files"], json!([fixture.path("lib.go"), fixture.path("main.go")]));
    assert_eq!(main_text(&mut fixture), (MAIN.replace("greet", "hello"), true), "unsaved");
    assert_eq!(read(&fixture, "lib.go"), LIB.replace("greet", "hello"));
    assert_eq!(read(&fixture, "main.go"), MAIN, "the open file is not saved");
    let changed = fixture.server.wait_for("workspace/didChangeWatchedFiles", 1, WAIT);
    assert_eq!(changed, vec![json!({ "changes": [{ "uri": uri(&fixture, "lib.go"), "type": 2 }] })]);
    assert_eq!(fixture.session.state::<Refactoring>().unwrap().journal().len(), 1);

    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(main_text(&mut fixture), (MAIN.to_owned(), false), "one undo step");
    assert_eq!(read(&fixture, "lib.go"), LIB);
    assert_eq!(fixture.server.wait_for("workspace/didChangeWatchedFiles", 2, WAIT).len(), 2);
    assert!(fixture.session.state::<Refactoring>().unwrap().journal().is_empty());
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(false));
}

#[test]
fn apply_without_a_preview_renames_in_one_go_and_undo_skips_files_changed_since() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    let at = chars(MAIN, "greet");
    let applied = fixture.dispatch("refactor.rename.apply", json!({ "newName": "hello", "offset": at })).unwrap();
    assert_eq!(applied["edits"], 2);
    std::fs::write(fixture.path("lib.go"), "package main\n").unwrap();
    fixture.with(|_, workbench| workbench.take_messages());
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(main_text(&mut fixture).0, MAIN);
    assert_eq!(read(&fixture, "lib.go"), "package main\n");
    let messages = fixture.with(|_, workbench| workbench.take_messages());
    assert_eq!(messages, vec![t!("workbench-undo-skipped", count = 1, files = "lib.go")]);
}

#[test]
fn an_agent_reads_the_paths_of_a_rename_relative_to_the_project() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    let at = chars(MAIN, "greet");
    let params = json!({ "newName": "hello", "offset": at, "waitMs": 5000 });
    let places = json!([
        { "uri": uri(&fixture, "main.go"), "range": main_greet() },
        { "uri": uri(&fixture, "lib.go"), "range": lib_greet() }
    ]);
    fixture.server.on("textDocument/definition", places);
    let (registry, session) = (&fixture.registry, &mut fixture.session);
    let targets = zen_core::macros::as_agent(session, |session| {
        registry.dispatch(session, "navigate.declaration.goTo", json!({ "offset": at })).unwrap();
        registry.dispatch(session, "navigate.lookup.poll", json!({ "waitMs": 5000 })).unwrap()
    });
    let paths: Vec<&str> =
        targets["targets"].as_array().unwrap().iter().filter_map(|target| target["path"].as_str()).collect();
    assert_eq!(paths, ["main.go", "lib.go"], "{targets}");
    let (preview, applied) = zen_core::macros::as_agent(session, |session| {
        let preview = registry.dispatch(session, "refactor.rename.preview", params.clone()).unwrap();
        (preview, registry.dispatch(session, "refactor.rename.apply", params).unwrap())
    });
    assert_eq!(preview["files"][0]["path"], "lib.go", "{preview}");
    assert_eq!(preview["files"][0]["changes"][0]["path"], "lib.go", "{preview}");
    assert_eq!(applied["files"], json!(["lib.go", "main.go"]), "{applied}");
}

#[test]
fn an_agents_rename_saves_the_open_files_it_changed() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    let at = chars(MAIN, "greet");
    let params = json!({ "newName": "hello", "offset": at });
    let applied = zen_core::macros::as_agent(&mut fixture.session, |session| {
        fixture.registry.dispatch(session, "refactor.rename.apply", params)
    })
    .unwrap();
    assert_eq!(applied["edits"], 2);
    assert!(read(&fixture, "main.go").contains("hello"), "the open main.go is on disk: {}", read(&fixture, "main.go"));
}

#[test]
fn a_place_is_taken_as_a_line_and_column_too() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    let line = MAIN.lines().position(|line| line.contains("greet")).unwrap() + 1;
    let column = chars(MAIN.lines().nth(line - 1).unwrap(), "greet") + 1;
    let at = json!({ "newName": "hello", "path": "main.go", "line": line, "column": column, "waitMs": 5000 });
    let preview = fixture.dispatch("refactor.rename.preview", at.clone()).unwrap();
    assert_eq!(preview["usageCount"], 2, "{preview}");
    let sent = &fixture.server.requests("textDocument/rename")[0];
    assert_eq!(sent["position"], json!({ "line": 3, "character": utf16("func main() { s := \"ё😀\"; ") }));
    let applied = fixture.dispatch("refactor.rename.apply", at).unwrap();
    assert_eq!(applied["edits"], 2);
    assert_eq!(fixture.server.requests("textDocument/rename").len(), 1, "the previewed rename is applied");
}

#[test]
fn an_open_file_edited_after_the_preview_stops_the_whole_rename() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    fixture.with(|_, workbench| {
        workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "// x\n".to_owned())])
    });
    let error = failure(fixture.dispatch("refactor.rename.apply", json!({})));
    assert_eq!(error, t!("workbench-rename-files-changed"));
    assert_eq!(main_text(&mut fixture).0, format!("// x\n{MAIN}"));
    assert_eq!(read(&fixture, "lib.go"), LIB);
    assert!(fixture.server.requests("workspace/didChangeWatchedFiles").is_empty());
}

#[test]
fn a_file_changed_on_disk_after_the_preview_stops_the_whole_rename() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    let changed = format!("{LIB}// more\n");
    std::fs::write(fixture.path("lib.go"), &changed).unwrap();
    let error = failure(fixture.dispatch("refactor.rename.apply", json!({})));
    assert_eq!(error, t!("workbench-rename-files-changed"));
    assert_eq!(main_text(&mut fixture), (MAIN.to_owned(), false));
    assert_eq!(read(&fixture, "lib.go"), changed);
}

#[cfg(unix)]
#[test]
fn a_write_that_fails_restores_the_files_written_before_it() {
    use std::os::unix::fs::PermissionsExt;

    let mut fixture =
        project(&[("main.go", MAIN), ("a/x.go", LIB), ("b/y.go", LIB)], json!({ "prepareProvider": true }));
    let edit = |name: &str| json!({ "textDocument": { "uri": uri(&fixture, name), "version": null }, "edits": [{ "range": lib_greet(), "newText": "hello" }] });
    let answer = json!({ "documentChanges": [
        edit("a/x.go"),
        edit("b/y.go"),
        { "textDocument": { "uri": uri(&fixture, "main.go"), "version": 1 }, "edits": [{ "range": main_greet(), "newText": "hello" }] }
    ]});
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    let locked = fixture.path("b");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = fixture.dispatch("refactor.rename.apply", json!({}));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(failure(result), t!("workbench-rename-files-changed"));
    assert_eq!(read(&fixture, "a/x.go"), LIB, "restored");
    assert_eq!(read(&fixture, "b/y.go"), LIB);
    assert_eq!(main_text(&mut fixture), (MAIN.to_owned(), false));
    assert!(fixture.session.state::<Refactoring>().unwrap().journal().is_empty());
}

#[test]
fn overlapping_edits_are_refused() {
    let mut fixture = go_project();
    let overlapping = json!({ "changes": { uri(&fixture, "lib.go"): [
        { "range": lib_greet(), "newText": "hello" },
        { "range": range(2, utf16("/* ё😀 */ func gr"), 5), "newText": "x" }
    ]}});
    fixture.server.on("textDocument/rename", overlapping);
    assert_eq!(preview(&mut fixture, "other")["error"], t!("workbench-rename-overlapping"));
    assert_eq!(read(&fixture, "lib.go"), LIB);
}

fn text_change(fixture: &Fixture, name: &str, range: Value, new_text: &str) -> Value {
    json!({ "textDocument": { "uri": uri(fixture, name), "version": null }, "edits": [{ "range": range, "newText": new_text }] })
}

fn exists(fixture: &Fixture, name: &str) -> bool {
    fixture.path(name).exists()
}

fn file_project() -> Fixture {
    project(&[("main.go", MAIN), ("lib.go", LIB), ("old.go", "package main\n")], json!({ "prepareProvider": true }))
}

fn operations_answer(fixture: &Fixture) -> Value {
    json!({ "documentChanges": [
        text_change(fixture, "lib.go", lib_greet(), "hello"),
        { "kind": "rename", "oldUri": uri(fixture, "lib.go"), "newUri": uri(fixture, "pkg/hello.go") },
        { "textDocument": { "uri": uri(fixture, "main.go"), "version": 1 }, "edits": [{ "range": main_greet(), "newText": "hello" }] },
        { "kind": "create", "uri": uri(fixture, "new.go") },
        text_change(fixture, "new.go", range(0, 0, 0), "package main\n"),
        text_change(fixture, "pkg/hello.go", range(0, 0, 0), "// moved\n"),
        { "kind": "delete", "uri": uri(fixture, "old.go") }
    ]})
}

#[test]
fn files_created_renamed_and_deleted_by_a_rename_are_previewed_in_order() {
    let mut fixture = file_project();
    let answer = operations_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    let preview = preview(&mut fixture, "hello");
    assert_eq!(preview["error"], Value::Null, "{preview}");
    assert_eq!(
        preview["operations"],
        json!([
            { "kind": "rename", "path": fixture.path("lib.go"), "relative": "lib.go", "newPath": fixture.path("pkg/hello.go"), "newRelative": "pkg/hello.go" },
            { "kind": "create", "path": fixture.path("new.go"), "relative": "new.go" },
            { "kind": "delete", "path": fixture.path("old.go"), "relative": "old.go" }
        ])
    );
    let files: Vec<&str> =
        preview["files"].as_array().unwrap().iter().map(|file| file["relative"].as_str().unwrap()).collect();
    assert_eq!(files, ["lib.go", "main.go", "new.go", "pkg/hello.go"]);
    assert_eq!(preview["usageCount"], 4);
    assert!(
        exists(&fixture, "lib.go") && !exists(&fixture, "new.go") && exists(&fixture, "old.go"),
        "a preview changes nothing"
    );
}

#[test]
fn text_and_file_changes_are_applied_in_order_and_undone_as_one_step() {
    let mut fixture = file_project();
    let answer = operations_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    let applied = fixture.dispatch("refactor.rename.apply", json!({})).unwrap();
    assert_eq!(
        applied["files"],
        json!([
            fixture.path("lib.go"),
            fixture.path("main.go"),
            fixture.path("new.go"),
            fixture.path("old.go"),
            fixture.path("pkg/hello.go")
        ])
    );
    assert!(!exists(&fixture, "lib.go") && !exists(&fixture, "old.go"));
    assert_eq!(read(&fixture, "pkg/hello.go"), format!("// moved\n{}", LIB.replace("greet", "hello")));
    assert_eq!(read(&fixture, "new.go"), "package main\n");
    assert_eq!(main_text(&mut fixture), (MAIN.replace("greet", "hello"), true));
    let watched: Vec<Value> = fixture
        .server
        .wait_for("workspace/didChangeWatchedFiles", 7, WAIT)
        .iter()
        .map(|sent| sent["changes"][0].clone())
        .collect();
    assert!(watched.contains(&json!({ "uri": uri(&fixture, "lib.go"), "type": 3 })), "{watched:?}");
    assert!(watched.contains(&json!({ "uri": uri(&fixture, "pkg/hello.go"), "type": 1 })), "{watched:?}");
    assert!(watched.contains(&json!({ "uri": uri(&fixture, "old.go"), "type": 3 })), "{watched:?}");
    assert_eq!(fixture.session.state::<Refactoring>().unwrap().journal().len(), 1);

    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(read(&fixture, "lib.go"), LIB);
    assert_eq!(read(&fixture, "old.go"), "package main\n");
    assert!(!exists(&fixture, "new.go") && !exists(&fixture, "pkg"), "created files and folders are removed");
    assert_eq!(main_text(&mut fixture), (MAIN.to_owned(), false));
    assert!(fixture.session.state::<Refactoring>().unwrap().journal().is_empty());
}

#[test]
fn renaming_the_open_file_moves_its_tab_and_undo_there_moves_it_back() {
    let mut fixture = go_project();
    let answer = json!({ "documentChanges": [
        { "textDocument": { "uri": uri(&fixture, "main.go"), "version": 1 }, "edits": [{ "range": main_greet(), "newText": "hello" }] },
        { "kind": "rename", "oldUri": uri(&fixture, "main.go"), "newUri": uri(&fixture, "app.go") },
        text_change(&fixture, "lib.go", lib_greet(), "hello")
    ]});
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    fixture.dispatch("refactor.rename.apply", json!({})).unwrap();
    assert_eq!(fixture.open_files(), [fixture.path("app.go")]);
    assert_eq!(read(&fixture, "app.go"), MAIN, "the moved tab keeps its change unsaved");
    let active = fixture.with(|_, workbench| {
        workbench.active_document().map(|document| (document.path().to_owned(), document.text().to_owned()))
    });
    assert_eq!(active, Some((fixture.path("app.go"), MAIN.replace("greet", "hello"))));
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let opened = fixture.server.wait_for("textDocument/didOpen", 2, WAIT);
    assert_eq!(opened[1]["textDocument"]["uri"], uri(&fixture, "app.go"));

    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(fixture.open_files(), [fixture.path("main.go")]);
    assert_eq!(main_text(&mut fixture), (MAIN.to_owned(), false));
    assert_eq!(read(&fixture, "lib.go"), LIB);
    assert!(!exists(&fixture, "app.go"));
}

#[test]
fn file_operation_options_decide_what_happens_to_a_taken_or_missing_name() {
    let mut fixture = file_project();
    let ignored = json!({ "documentChanges": [
        { "kind": "create", "uri": uri(&fixture, "old.go"), "options": { "ignoreIfExists": true } },
        { "kind": "delete", "uri": uri(&fixture, "gone.go"), "options": { "ignoreIfNotExists": true } },
        { "kind": "rename", "oldUri": uri(&fixture, "lib.go"), "newUri": uri(&fixture, "old.go"), "options": { "ignoreIfExists": true } },
        text_change(&fixture, "lib.go", lib_greet(), "hello")
    ]});
    fixture.server.on("textDocument/rename", ignored);
    let preview_answer = preview(&mut fixture, "hello");
    assert_eq!(preview_answer["operations"], json!([]), "{preview_answer}");
    fixture.dispatch("refactor.rename.apply", json!({})).unwrap();
    assert_eq!(read(&fixture, "old.go"), "package main\n");
    assert_eq!(read(&fixture, "lib.go"), LIB.replace("greet", "hello"));

    let taken = json!({ "documentChanges": [{ "kind": "create", "uri": uri(&fixture, "old.go") }] });
    fixture.server.on("textDocument/rename", taken);
    let error = preview(&mut fixture, "other")["error"].clone();
    assert_eq!(error, t!("workbench-edit-file-exists", path = fixture.path("old.go").display().to_string()));

    let missing = json!({ "documentChanges": [{ "kind": "delete", "uri": uri(&fixture, "gone.go") }] });
    fixture.server.on("textDocument/rename", missing);
    let error = preview(&mut fixture, "third")["error"].clone();
    assert_eq!(error, t!("workbench-edit-file-missing", path = fixture.path("gone.go").display().to_string()));

    let overwrite = json!({ "documentChanges": [
        { "kind": "rename", "oldUri": uri(&fixture, "lib.go"), "newUri": uri(&fixture, "old.go"), "options": { "overwrite": true, "ignoreIfExists": true } }
    ]});
    fixture.server.on("textDocument/rename", overwrite);
    preview(&mut fixture, "fourth");
    fixture.dispatch("refactor.rename.apply", json!({})).unwrap();
    assert_eq!(read(&fixture, "old.go"), LIB.replace("greet", "hello"), "overwrite wins over ignoreIfExists");
    assert!(!exists(&fixture, "lib.go"));
}

#[test]
fn file_operations_outside_the_project_are_refused_and_change_nothing() {
    let mut fixture = file_project();
    let elsewhere = tempfile::tempdir().unwrap();
    let outside = zen_lsp::position::file_uri(&elsewhere.path().join("lib.go"));
    let answer = json!({ "documentChanges": [
        text_change(&fixture, "lib.go", lib_greet(), "hello"),
        { "kind": "rename", "oldUri": uri(&fixture, "lib.go"), "newUri": outside }
    ]});
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    let error = failure(fixture.dispatch("refactor.rename.apply", json!({})));
    assert_eq!(
        error,
        t!("workbench-edit-outside-project", path = elsewhere.path().join("lib.go").display().to_string())
    );
    assert_eq!(read(&fixture, "lib.go"), LIB);
    assert!(!elsewhere.path().join("lib.go").exists());
}

#[cfg(unix)]
#[test]
fn a_file_operation_that_fails_reverts_the_changes_made_before_it() {
    use std::os::unix::fs::PermissionsExt;

    let mut fixture =
        project(&[("main.go", MAIN), ("lib.go", LIB), ("locked/keep.go", LIB)], json!({ "prepareProvider": true }));
    let answer = json!({ "documentChanges": [
        text_change(&fixture, "lib.go", lib_greet(), "hello"),
        { "kind": "rename", "oldUri": uri(&fixture, "lib.go"), "newUri": uri(&fixture, "hello.go") },
        { "kind": "create", "uri": uri(&fixture, "locked/new.go") }
    ]});
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    let locked = fixture.path("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = fixture.dispatch("refactor.rename.apply", json!({}));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    failure(result);
    assert_eq!(read(&fixture, "lib.go"), LIB, "renamed back and restored");
    assert!(!exists(&fixture, "hello.go") && !exists(&fixture, "locked/new.go"));
    assert!(fixture.session.state::<Refactoring>().unwrap().journal().is_empty());
}

#[test]
fn the_new_name_must_be_an_identifier() {
    for name in ["hello", "_x", "имя", "x1", "Δt"] {
        assert!(is_identifier(Some("go"), name), "{name}");
    }
    for name in ["", "1x", "a-b", "a b", "func", "range"] {
        assert!(!is_identifier(Some("go"), name), "{name}");
    }
    assert!(is_identifier(None, "func"));

    let mut fixture = go_project();
    assert_eq!(preview(&mut fixture, "func")["error"], t!("workbench-rename-invalid-name", name = "func"));
    let error = failure(fixture.dispatch("refactor.rename.apply", json!({ "newName": "a-b" })));
    assert_eq!(error, t!("workbench-rename-invalid-name", name = "a-b"));
    assert!(fixture.server.requests("textDocument/rename").is_empty());
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenamePreviewOutput {
    pending: bool,
    files: Vec<RenameFileOutput>,
    usage_count: usize,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
struct RenameFileOutput {
    relative: String,
    changes: Vec<RenameChangeOutput>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameChangeOutput {
    start: usize,
    end: usize,
    line: u32,
    column: u32,
    preview: String,
    highlight: [usize; 2],
    new_text: String,
}

#[test]
fn undo_reloads_a_file_opened_after_the_rename_wrote_it() {
    let mut fixture = go_project();
    let answer = rename_answer(&fixture);
    fixture.server.on("textDocument/rename", answer);
    preview(&mut fixture, "hello");
    fixture.dispatch("refactor.rename.apply", json!({})).unwrap();
    let (lib, main) = (fixture.path("lib.go"), fixture.path("main.go"));
    fixture.dispatch("file.document.open", json!({ "path": lib })).unwrap();
    fixture.dispatch("file.document.open", json!({ "path": main })).unwrap();
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(read(&fixture, "lib.go"), LIB);
    let opened = fixture.with(|_, workbench| {
        let document = workbench.documents().iter().find(|document| document.path() == lib).unwrap();
        (document.text().to_owned(), document.is_dirty())
    });
    assert_eq!(opened, (LIB.to_owned(), false));
}

#[test]
fn an_agents_edit_never_deletes_or_moves_a_folder_holding_a_hidden_file() {
    let files = [("main.go", MAIN), ("lib.go", LIB), ("config/.env", "TOKEN=1\n")];
    for operation in ["delete", "rename"] {
        let mut fixture = project(&files, json!({ "prepareProvider": true }));
        fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["config/.env".to_owned()]);
        let step = match operation {
            "delete" => json!({ "kind": "delete", "uri": uri(&fixture, "config"), "options": { "recursive": true } }),
            _ => json!({ "kind": "rename", "oldUri": uri(&fixture, "config"), "newUri": uri(&fixture, "settings") }),
        };
        let answer = json!({ "documentChanges": [text_change(&fixture, "lib.go", lib_greet(), "hello"), step] });
        fixture.server.on("textDocument/rename", answer);
        preview(&mut fixture, "hello");
        let Fixture { registry, session, .. } = &mut fixture;
        let refused = zen_core::macros::as_agent(session, |session| {
            registry.dispatch(session, "refactor.rename.apply", json!({}))
        });
        let error = failure(refused);
        assert!(error.contains("config/.env"), "{operation}: {error}");
        assert_eq!(read(&fixture, "config/.env"), "TOKEN=1\n", "{operation}");
        assert_eq!(read(&fixture, "lib.go"), LIB, "{operation}: nothing changed");
    }
}
