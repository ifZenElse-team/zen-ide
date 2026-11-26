use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::DispatchError;

use super::*;
use crate::language::testing::{Fixture, fixture};

const WAIT: Duration = Duration::from_secs(5);
const MAIN: &str = "package main\n\nfunc main() { greet() }\n";
pub(crate) const LIB: &str = "package main\n\n// greet says hello.\nfunc greet() { greet() }\n\nfunc other() {}\n";
const LIB_WITHOUT_GREET: &str = "package main\n\nfunc other() {}\n";

fn range(line: u64, start: u64, end_line: u64, end: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": end_line, "character": end } })
}

fn uri(fixture: &Fixture, name: &str) -> String {
    zen_lsp::position::file_uri(&fixture.path(name))
}

pub(crate) fn project() -> Fixture {
    let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
    let filters = json!({ "filters": [{ "pattern": { "glob": "**/*.go" } }] });
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "documentSymbolProvider": true,
        "referencesProvider": true,
        "workspace": { "fileOperations": { "willDelete": filters, "didDelete": filters } }
    }));
    fixture.server.on(
        "textDocument/documentSymbol",
        json!([
            { "name": "greet", "kind": 12, "range": range(3, 0, 3, 24), "selectionRange": range(3, 5, 3, 10) },
            { "name": "other", "kind": 12, "range": range(5, 0, 5, 15), "selectionRange": range(5, 5, 5, 10) }
        ]),
    );
    fixture.open("lib.go");
    fixture
}

pub(crate) fn answer_usages(fixture: &Fixture, outside: bool) {
    let (main, lib) = (uri(fixture, "main.go"), uri(fixture, "lib.go"));
    fixture.server.on_with("textDocument/references", move |params| {
        assert_eq!(params["context"]["includeDeclaration"], false);
        let mut found = vec![json!({ "uri": lib, "range": range(3, 15, 3, 20) })];
        if outside {
            found.push(json!({ "uri": main, "range": range(2, 14, 2, 19) }));
        }
        json!(found)
    });
}

pub(crate) fn greet_offset() -> usize {
    LIB.find("greet()").unwrap() + 2
}

fn find(fixture: &mut Fixture, params: Value) -> Value {
    let mut params = params;
    params["waitMs"] = json!(5000);
    fixture.dispatch_once_running("refactor.safeDelete.find", params).unwrap()
}

pub(crate) fn lib_text(fixture: &mut Fixture) -> String {
    let path = fixture.path("lib.go");
    fixture.with(|_, workbench| {
        workbench.documents().iter().find(|document| document.path() == path).unwrap().text().to_owned()
    })
}

fn failure(result: Result<Value, DispatchError>) -> String {
    match result {
        Err(DispatchError::Failed { error, .. }) => error.message,
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_declaration_nothing_uses_is_deleted_with_its_doc_comment_and_one_undo_brings_it_back() {
    let mut fixture = project();
    answer_usages(&fixture, false);
    let at = json!({ "path": "lib.go", "offset": greet_offset() });
    let found = find(&mut fixture, at.clone());
    assert_eq!(
        (found["pending"].clone(), found["name"].clone(), found["total"].clone()),
        (json!(false), json!("greet"), json!(0))
    );
    assert_eq!(found["deletes"][0]["changes"][0]["line"], 3, "from the doc comment");
    assert_eq!(found["deletes"][0]["changes"][0]["newText"], "");
    let sent = &fixture.server.wait_for("textDocument/references", 1, WAIT)[0];
    assert_eq!(sent["position"], json!({ "line": 3, "character": 5 }), "asked at the name");

    let deleted = fixture.dispatch("refactor.safeDelete.apply", at).unwrap();
    assert_eq!(deleted["name"], "greet");
    assert_eq!(lib_text(&mut fixture), LIB_WITHOUT_GREET, "the comment, the function and one blank line");
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(lib_text(&mut fixture), LIB);
}

#[test]
fn usages_outside_the_declaration_stop_the_deletion_unless_it_is_forced() {
    let mut fixture = project();
    answer_usages(&fixture, true);
    let at = json!({ "path": "lib.go", "offset": greet_offset() });
    let found = find(&mut fixture, at.clone());
    assert_eq!(found["total"], 1, "the call inside greet itself does not count");
    assert_eq!(found["usages"][0]["path"], "main.go");
    assert_eq!(found["usages"][0]["usages"][0]["line"], 3);

    let refused = failure(fixture.dispatch("refactor.safeDelete.apply", at.clone()));
    assert_eq!(refused, t!("workbench-safe-delete-has-usages", name = "greet", count = 1));
    assert_eq!(lib_text(&mut fixture), LIB);
    let mut forced = at;
    forced["force"] = json!(true);
    fixture.dispatch("refactor.safeDelete.apply", forced).unwrap();
    assert_eq!(lib_text(&mut fixture), LIB_WITHOUT_GREET);
}

#[test]
fn a_caret_off_every_declaration_name_has_nothing_to_delete() {
    let mut fixture = project();
    answer_usages(&fixture, false);
    let offset = LIB.find("{ greet").unwrap();
    let found = find(&mut fixture, json!({ "path": "lib.go", "offset": offset }));
    assert_eq!(found["error"], t!("workbench-safe-delete-not-here"));
    assert!(fixture.server.requests("textDocument/references").is_empty());
    fixture.dispatch("refactor.safeDelete.cancel", json!({})).unwrap();
}

#[test]
fn a_file_is_checked_for_usages_from_other_files_and_deleted_through_the_project_delete() {
    let mut fixture = project();
    answer_usages(&fixture, true);
    let files = json!({ "paths": ["lib.go"] });
    let found = find(&mut fixture, files.clone());
    assert_eq!(found["name"], "lib.go");
    assert_eq!(found["total"], 1, "usages inside the deleted file do not count");
    assert_eq!(found["operations"][0]["kind"], "delete");
    assert_eq!(fixture.server.requests("textDocument/references").len(), 2, "every top-level declaration");
    assert!(fixture.dispatch("refactor.safeDelete.apply", files.clone()).is_err());

    let mut forced = files;
    forced["force"] = json!(true);
    let deleted = fixture.dispatch("refactor.safeDelete.apply", forced).unwrap();
    assert_eq!(deleted["files"], json!([fixture.path("lib.go")]));
    assert!(!fixture.path("lib.go").exists());
    let asked = fixture.server.requests("workspace/willDeleteFiles");
    assert_eq!(asked, vec![json!({ "files": [{ "uri": uri(&fixture, "lib.go") }] })]);
}

#[test]
fn a_file_without_a_language_server_has_no_usages() {
    let mut fixture = project();
    std::fs::write(fixture.path("notes.txt"), "greet\n").unwrap();
    let found = find(&mut fixture, json!({ "paths": ["notes.txt"] }));
    assert_eq!((found["pending"].clone(), found["total"].clone()), (json!(false), json!(0)));
    fixture.dispatch("refactor.safeDelete.apply", json!({ "paths": ["notes.txt"] })).unwrap();
    assert!(!fixture.path("notes.txt").exists());
}

#[test]
fn the_deleted_text_takes_attributes_annotations_and_doc_comments_above_it() {
    let rust = "use x;\n\n/// Says hello.\n#[inline]\npub fn greet() {}\n\nfn other() {}\n";
    let start = rust.find("pub fn").unwrap();
    let end = rust.find("{}\n").unwrap() + 2;
    let (from, to) = deletion(rust, start, end);
    assert_eq!(format!("{}{}", &rust[..from], &rust[to..]), "use x;\n\nfn other() {}\n");

    let java = "class A {\n    /**\n     * Old.\n     */\n    @Deprecated\n    void old() {}\n    void kept() {}\n}\n";
    let start = java.find("void old").unwrap();
    let end = java.find("{}\n").unwrap() + 2;
    let (from, to) = deletion(java, start, end);
    assert_eq!(format!("{}{}", &java[..from], &java[to..]), "class A {\n    void kept() {}\n}\n");

    let last = "package main\n\nfunc kept() {}\n\nfunc last() {}\n";
    let start = last.find("func last").unwrap();
    let (from, to) = deletion(last, start, last.len() - 1);
    assert_eq!(
        format!("{}{}", &last[..from], &last[to..]),
        "package main\n\nfunc kept() {}\n",
        "no blank line is left at the end"
    );

    let inline = "let a = 1, b = 2;\n";
    let start = inline.find("b = 2").unwrap();
    assert_eq!(deletion(inline, start, start + 5), (start, start + 5), "part of a line stays exact");
}

#[test]
#[ignore = "needs gopls on PATH"]
fn gopls_finds_the_usages_that_keep_a_function_and_a_file_from_being_deleted() {
    use crate::language::servers::{LanguageServers, ServerDeclaration, SystemTools};
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let files = [
        ("go.mod", "module shop\n\ngo 1.22\n"),
        ("main.go", "package main\n\nfunc main() { greet() }\n"),
        (
            "lib.go",
            "package main\n\n// greet says hello.\nfunc greet() {}\n\n// unused is never called.\nfunc unused() {}\n",
        ),
    ];
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    let data = tempfile::tempdir().unwrap();
    let servers = LanguageServers::load(Some(data.path().join("servers.json")), std::sync::Arc::new(SystemTools));
    let declared = include_str!("../../../../../lang/go/language-servers.json");
    servers.declare(ServerDeclaration::parse_list(declared).unwrap());
    session.set_state(servers);
    let lib = dir.path().join("lib.go");
    registry.dispatch(&mut session, "file.document.open", json!({ "path": lib })).unwrap();
    let mut until = |params: Value| {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let _ = registry.dispatch(&mut session, "language.servers.sync", json!({}));
            let answer = registry.dispatch(&mut session, "refactor.safeDelete.find", params.clone());
            if let Ok(answer) = &answer
                && answer["pending"] == false
                && answer.get("error").is_none()
            {
                return answer.clone();
            }
            assert!(Instant::now() < deadline, "{answer:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let text = std::fs::read_to_string(&lib).unwrap();
    let greet = until(json!({ "path": lib, "offset": text.find("greet()").unwrap() + 1, "waitMs": 2000 }));
    assert_eq!((greet["name"].clone(), greet["total"].clone()), (json!("greet"), json!(1)), "{greet}");
    assert_eq!(greet["usages"][0]["path"], "main.go");
    let unused = until(json!({ "path": lib, "offset": text.find("unused()").unwrap() + 1, "waitMs": 2000 }));
    assert_eq!(unused["total"], 0, "{unused}");
    let file = until(json!({ "paths": [dir.path().join("main.go")], "waitMs": 2000 }));
    assert_eq!(file["total"], 0, "main is used by nobody: {file}");
    let file = until(json!({ "paths": [lib.clone()], "waitMs": 2000 }));
    assert_eq!(file["total"], 1, "{file}");
    let deleted = registry.dispatch(
        &mut session,
        "refactor.safeDelete.apply",
        json!({ "path": lib, "offset": text.find("unused()").unwrap() + 1 }),
    );
    assert!(deleted.is_ok(), "{deleted:?}");
    let workbench = session.state::<Workbench>().unwrap();
    let now = workbench.documents().iter().find(|document| document.path() == lib).unwrap().text().to_owned();
    assert_eq!(now, "package main\n\n// greet says hello.\nfunc greet() {}\n");
}

#[test]
fn an_agent_is_told_nothing_about_a_file_hidden_from_it() {
    let mut fixture = project();
    let path = fixture.path("lib.go");
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["lib.go".to_owned()]);
    let offset = greet_offset();
    let code = |fixture: &mut Fixture, command: &str, params: Value| {
        let (registry, session) = (&fixture.registry, &mut fixture.session);
        match zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params)) {
            Err(DispatchError::Failed { error, .. }) => error.code.unwrap_or_default(),
            other => panic!("{command}: not refused: {other:?}"),
        }
    };
    let at = json!({ "path": path, "offset": offset });
    assert_eq!(code(&mut fixture, "refactor.safeDelete.find", at.clone()), "redacted");
    assert_eq!(code(&mut fixture, "refactor.safeDelete.apply", at.clone()), "redacted");
    let paths = json!({ "paths": [path] });
    assert_eq!(code(&mut fixture, "refactor.safeDelete.find", paths), "redacted");
    let context = json!({ "provider": "safeDelete", "context": at, "values": {} });
    assert_eq!(code(&mut fixture, "refactor.dialog.preview", context.clone()), "redacted");
    assert_eq!(code(&mut fixture, "refactor.dialog.form", context.clone()), "redacted");
    assert_eq!(code(&mut fixture, "refactor.dialog.apply", context), "redacted");
    let extract = json!({ "path": path, "start": offset, "end": offset + 1 });
    assert_eq!(code(&mut fixture, "refactor.extract.variable", extract), "redacted");
}

#[test]
fn a_usage_added_after_the_search_stops_the_deletion() {
    let mut fixture = project();
    let path = fixture.path("lib.go");
    answer_usages(&fixture, false);
    let at = json!({ "paths": [path] });
    let found = find(&mut fixture, at.clone());
    assert_eq!(found["total"], 0, "{found}");
    answer_usages(&fixture, true);
    let refused = failure(fixture.dispatch("refactor.safeDelete.apply", json!({ "paths": [path], "waitMs": 5000 })));
    assert!(refused.contains("lib.go"), "{refused}");
    assert!(path.is_file(), "the search was run again before anything was deleted");
}

#[test]
fn a_file_with_unsaved_changes_is_not_deleted() {
    let mut fixture = project();
    answer_usages(&fixture, false);
    fixture.dispatch("editor.edit.insert", json!({ "text": "// typed\n" })).unwrap();
    let refused =
        failure(fixture.dispatch("refactor.safeDelete.apply", json!({ "paths": ["lib.go"], "waitMs": 5000 })));
    assert!(refused.contains("lib.go"), "{refused}");
    assert!(fixture.path("lib.go").is_file());
    let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert!(document.is_dirty() && document.text().contains("// typed"), "the edit is still there");
}
