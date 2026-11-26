use std::time::Duration;

use serde_json::{Value, json};
use zen_lsp::fake::Reply;

use super::*;
use crate::language::testing::{Fixture, fixture};

const WAIT: Duration = Duration::from_secs(5);
const MAIN: &str = "package main\n\nimport \"shop/lib\"\n\nfunc main() { lib.Greet() }\n";
const LIB: &str = "package lib\n\nfunc Greet() {}\n";

fn filters(glob: &str) -> Value {
    json!({ "filters": [{ "scheme": "file", "pattern": { "glob": glob } }] })
}

fn project(file_operations: Value) -> Fixture {
    let mut fixture = fixture(&[("main.go", MAIN), ("lib/lib.go", LIB), ("README.md", "# shop\n")]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "workspace": { "fileOperations": file_operations }
    }));
    fixture.open("main.go");
    fixture
}

fn uri(fixture: &Fixture, name: &str) -> String {
    zen_lsp::position::file_uri(&fixture.path(name))
}

fn import_edit(fixture: &Fixture, to: &str) -> Value {
    let range = json!({ "start": { "line": 2, "character": 8 }, "end": { "line": 2, "character": 16 } });
    json!({ "changes": { uri(fixture, "main.go"): [{ "range": range, "newText": to }] } })
}

fn main_text(fixture: &mut Fixture) -> String {
    let path = fixture.path("main.go");
    fixture.with(|_, workbench| {
        workbench.documents().iter().find(|document| document.path() == path).unwrap().text().to_owned()
    })
}

#[test]
fn a_rename_in_the_project_asks_the_server_first_applies_its_edits_and_tells_it_after() {
    let mut fixture = project(json!({ "willRename": filters("**/*.go"), "didRename": filters("**/*.go") }));
    let moved = import_edit(&fixture, "shop/pkg");
    fixture.server.on("workspace/willRenameFiles", moved);
    let renamed = fixture.dispatch("file.fs.rename", json!({ "path": "lib/lib.go", "name": "shop.go" })).unwrap();
    assert_eq!(renamed, json!(fixture.path("lib/shop.go")));
    let asked = fixture.server.requests("workspace/willRenameFiles");
    let files = json!({ "files": [{ "oldUri": uri(&fixture, "lib/lib.go"), "newUri": uri(&fixture, "lib/shop.go") }] });
    assert_eq!(asked, vec![files.clone()]);
    assert_eq!(fixture.server.wait_for("workspace/didRenameFiles", 1, WAIT), vec![files]);
    assert_eq!(main_text(&mut fixture), MAIN.replace("shop/lib\"", "shop/pkg\""));

    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(main_text(&mut fixture), MAIN);
    assert!(fixture.path("lib/lib.go").exists() && !fixture.path("lib/shop.go").exists(), "one undo moves it back");
}

#[test]
fn only_files_the_server_registered_for_are_sent() {
    let mut fixture = project(json!({
        "willRename": { "filters": [{ "pattern": { "glob": "**/*.go", "matches": "file" } }] },
        "didRename": { "filters": [{ "pattern": { "glob": "**", "matches": "folder" } }] }
    }));
    fixture.dispatch("file.fs.rename", json!({ "path": "README.md", "name": "NOTES.md" })).unwrap();
    fixture.dispatch("file.fs.rename", json!({ "path": "lib/lib.go", "name": "shop.go" })).unwrap();
    fixture.dispatch("file.fs.rename", json!({ "path": "lib", "name": "pkg" })).unwrap();
    let asked = fixture.server.requests("workspace/willRenameFiles");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["files"][0]["oldUri"], uri(&fixture, "lib/lib.go"));
    let told = fixture.server.wait_for("workspace/didRenameFiles", 1, WAIT);
    assert_eq!(told, vec![json!({ "files": [{ "oldUri": uri(&fixture, "lib"), "newUri": uri(&fixture, "pkg") }] })]);
    fixture
        .dispatch("file.fs.rename", json!({ "path": "pkg/shop.go", "name": "x.go", "updateReferences": false }))
        .unwrap();
    assert_eq!(fixture.server.requests("workspace/willRenameFiles").len(), 1, "updateReferences: false asks nobody");
}

#[test]
fn moving_creating_and_deleting_files_tell_the_server_too() {
    let mut fixture = project(json!({
        "willRename": filters("**"),
        "didRename": filters("**"),
        "willCreate": filters("**/*.go"),
        "didCreate": filters("**/*.go"),
        "willDelete": filters("**/*.go"),
        "didDelete": filters("**/*.go")
    }));
    fixture.dispatch("file.fs.newDirectory", json!({ "directory": ".", "name": "pkg" })).unwrap();
    fixture.dispatch("project.file.cut", json!({ "paths": [fixture.path("lib/lib.go")], "clipboard": false })).unwrap();
    fixture.dispatch("project.file.paste", json!({ "directory": "pkg", "clipboard": false })).unwrap();
    let moved = json!({ "files": [{ "oldUri": uri(&fixture, "lib/lib.go"), "newUri": uri(&fixture, "pkg/lib.go") }] });
    assert_eq!(fixture.server.requests("workspace/willRenameFiles"), vec![moved.clone()]);
    assert_eq!(fixture.server.wait_for("workspace/didRenameFiles", 1, WAIT), vec![moved]);

    fixture.dispatch("file.fs.newFile", json!({ "directory": "pkg", "name": "util.go" })).unwrap();
    let created = json!({ "files": [{ "uri": uri(&fixture, "pkg/util.go") }] });
    assert_eq!(fixture.server.requests("workspace/willCreateFiles"), vec![created.clone()]);
    assert_eq!(fixture.server.wait_for("workspace/didCreateFiles", 1, WAIT), vec![created]);

    let edit = import_edit(&fixture, "shop/gone");
    fixture.server.on("workspace/willDeleteFiles", edit);
    let (registry, session) = (&fixture.registry, &mut fixture.session);
    let gone = zen_core::macros::as_agent(session, |session| {
        registry.dispatch(session, "file.fs.delete", json!({ "paths": ["pkg/util.go"] })).unwrap()
    });
    assert_eq!(gone, json!(["pkg/util.go"]), "relative for an agent");
    let deleted = json!({ "files": [{ "uri": uri(&fixture, "pkg/util.go") }] });
    assert_eq!(fixture.server.requests("workspace/willDeleteFiles"), vec![deleted.clone()]);
    assert_eq!(fixture.server.wait_for("workspace/didDeleteFiles", 1, WAIT), vec![deleted]);
    assert_eq!(main_text(&mut fixture), MAIN.replace("shop/lib\"", "shop/gone\""));
}

#[test]
fn a_server_that_does_not_answer_in_time_is_skipped_and_the_rename_goes_on() {
    let mut fixture = project(json!({ "willRename": filters("**/*.go") }));
    fixture.server.reply("workspace/willRenameFiles", Reply::Hold);
    fixture.dispatch("file.fs.rename", json!({ "path": "lib/lib.go", "name": "shop.go" })).unwrap();
    assert!(fixture.path("lib/shop.go").exists());
    assert_eq!(fixture.server.wait_for("$/cancelRequest", 1, WAIT).len(), 1);
    let messages = fixture.with(|_, workbench| workbench.take_messages());
    assert!(messages.contains(&t!("workbench-file-operation-timeout", server = "go")), "{messages:?}");
    assert_eq!(main_text(&mut fixture), MAIN);
}

#[test]
fn edits_a_server_sends_for_a_rename_that_fails_are_reverted() {
    let mut fixture = project(json!({ "willRename": filters("**/*.go") }));
    let edit = import_edit(&fixture, "shop/pkg");
    fixture.server.on("workspace/willRenameFiles", edit);
    std::fs::write(fixture.path("lib/taken.go"), "package lib\n").unwrap();
    let error = fixture.dispatch("file.fs.rename", json!({ "path": "lib/lib.go", "name": "taken.go" }));
    assert!(error.is_err());
    assert_eq!(main_text(&mut fixture), MAIN);
    assert!(fixture.session.state::<Refactoring>().is_none_or(|refactoring| refactoring.journal().is_empty()));
}

#[test]
fn a_servers_edit_for_an_agents_rename_never_changes_a_file_hidden_from_agents() {
    let mut fixture = project(json!({ "willRename": filters("**/*.go") }));
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["main.go".to_owned()]);
    let moved = import_edit(&fixture, "shop/pkg");
    fixture.server.on("workspace/willRenameFiles", moved);
    let Fixture { registry, session, .. } = &mut fixture;
    let params = json!({ "path": "lib/lib.go", "name": "shop.go" });
    zen_core::macros::as_agent(session, |session| registry.dispatch(session, "file.fs.rename", params)).unwrap();
    assert!(fixture.path("lib/shop.go").exists(), "the rename goes on without the server's edit");
    assert_eq!(main_text(&mut fixture), MAIN);
}

const SHAPE: &str = "package shop\n\ntype Shape struct{}\n";
const USE: &str = "package shop\n\nvar shape Shape\n";

fn shapes() -> Fixture {
    let mut fixture = fixture(&[("Shape.go", SHAPE), ("use.go", USE), ("other.go", "package shop\n\n// Shapes\n")]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "workspace": { "fileOperations": { "willRename": filters("**/*.go") } }
    }));
    fixture.open("use.go");
    fixture
}

fn renamed_type(fixture: &Fixture, with_use: bool) -> Value {
    let at = |line: u32, from: u32| json!([{ "range": { "start": { "line": line, "character": from }, "end": { "line": line, "character": from + 5 } }, "newText": "Figure" }]);
    let mut changes = serde_json::Map::new();
    changes.insert(uri(fixture, "Shape.go"), at(2, 5));
    if with_use {
        changes.insert(uri(fixture, "use.go"), at(2, 10));
    }
    json!({ "changes": changes })
}

fn use_text(fixture: &mut Fixture) -> String {
    let path = fixture.path("use.go");
    fixture.with(|_, workbench| {
        workbench.documents().iter().find(|document| document.path() == path).unwrap().text().to_owned()
    })
}

#[test]
fn an_edit_that_leaves_a_file_naming_the_old_name_is_asked_for_again() {
    let mut fixture = shapes();
    let partial = renamed_type(&fixture, false);
    let whole = renamed_type(&fixture, true);
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = std::sync::Arc::clone(&asked);
    fixture.server.on_with("workspace/willRenameFiles", move |_| {
        if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 { partial.clone() } else { whole.clone() }
    });
    fixture.dispatch("file.fs.rename", json!({ "path": "Shape.go", "name": "Figure.go" })).unwrap();
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 2, "the first answer missed use.go");
    assert_eq!(use_text(&mut fixture), USE.replace("Shape", "Figure"));
    let messages = fixture.with(|_, workbench| workbench.take_messages());
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn a_file_the_server_never_updates_is_named_to_the_person() {
    let mut fixture = shapes();
    let partial = renamed_type(&fixture, false);
    fixture.server.on("workspace/willRenameFiles", partial);
    fixture.dispatch("file.fs.rename", json!({ "path": "Shape.go", "name": "Figure.go" })).unwrap();
    assert!(fixture.path("Figure.go").exists());
    assert_eq!(std::fs::read_to_string(fixture.path("Figure.go")).unwrap(), SHAPE.replace("Shape", "Figure"));
    assert_eq!(use_text(&mut fixture), USE, "what the server gave is applied, nothing more");
    let messages = fixture.with(|_, workbench| workbench.take_messages());
    let told = t!("workbench-file-operation-missed", server = "go", name = "Shape", count = 1, files = "use.go");
    assert!(messages.contains(&told), "{messages:?}");
    assert!(fixture.server.requests("workspace/willRenameFiles").len() > 1, "asked again before telling");
    let told = fixture.server.wait_for("workspace/didChangeWatchedFiles", 1, WAIT);
    assert_eq!(told[0]["changes"][0], json!({ "uri": uri(&fixture, "use.go"), "type": 2 }), "{told:?}");
}
