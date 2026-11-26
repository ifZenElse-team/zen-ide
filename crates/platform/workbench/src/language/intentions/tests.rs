use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::DispatchError;
use zen_lsp::fake::Reply;

use super::*;
use crate::language::testing::{Fixture, fixture};

const WAIT: Duration = Duration::from_secs(5);
const MAIN: &str = "package main\n\n// привет\nfunc main() {\n\tfmt.Println(x)\n}\n";
const LIB: &str = "package main\n\nvar y = 1\n";

fn range(line: u64, start: u64, end: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
}

fn uri(fixture: &Fixture, name: &str) -> String {
    zen_lsp::position::file_uri(&fixture.path(name))
}

fn project(resolve: bool) -> Fixture {
    let mut fixture = fixture(&[("main.go", MAIN), ("lib.go", LIB)]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "codeActionProvider": { "resolveProvider": resolve },
        "executeCommandProvider": { "commands": ["fix.all"] }
    }));
    fixture.open("main.go");
    fixture
}

fn undefined_x() -> Value {
    json!({ "range": range(4, 13, 14), "severity": 1, "message": "undefined: x", "code": "UndeclaredName" })
}

fn caret(fixture: &mut Fixture, offset: usize) {
    fixture.with(|_, workbench| {
        let selection = zen_editor::Selections::single(zen_editor::Selection::caret(offset));
        workbench.active_document_mut().unwrap().set_selections(selection);
    });
}

fn offset_of(needle: &str) -> usize {
    MAIN[..MAIN.find(needle).unwrap()].chars().count()
}

fn list(fixture: &mut Fixture, params: Value) -> IntentionList {
    let mut params = params;
    params["waitMs"] = json!(5000);
    serde_json::from_value(fixture.dispatch("editor.intentions.list", params).unwrap()).unwrap()
}

fn text(fixture: &Fixture) -> String {
    fixture.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
}

fn failure(result: Result<Value, DispatchError>) -> String {
    match result {
        Err(DispatchError::Failed { error, .. }) => error.message,
        other => panic!("expected a failure, got {other:?}"),
    }
}

fn import_fmt(fixture: &Fixture) -> Value {
    json!({ "changes": { uri(fixture, "main.go"): [{ "range": range(1, 0, 0), "newText": "import \"fmt\"\n" }] } })
}

#[test]
fn the_list_sends_the_problems_at_the_caret_and_puts_quick_fixes_first() {
    let mut fixture = project(false);
    fixture.publish("main.go", json!([undefined_x()]));
    fixture.server.on(
        "textDocument/codeAction",
        json!([
            { "title": "Extract function", "kind": "refactor.extract" },
            { "title": "Run go mod tidy", "command": "fix.all", "arguments": [] },
            { "title": "Create variable x", "kind": "quickfix", "diagnostics": [undefined_x()] },
            { "title": "Import fmt", "kind": "quickfix", "isPreferred": true, "edit": {} },
            { "title": "Inline", "kind": "refactor.inline", "disabled": { "reason": "nothing to inline" } }
        ]),
    );
    caret(&mut fixture, offset_of("x)"));
    let listed = list(&mut fixture, json!({}));
    let titles: Vec<&str> = listed.actions.iter().map(|action| action.title.as_str()).collect();
    assert_eq!(titles, ["Import fmt", "Create variable x", "Extract function", "Run go mod tidy", "Inline"]);
    assert!(listed.actions[0].quick_fix && listed.actions[0].preferred);
    assert!(listed.actions[1].quick_fix && !listed.actions[2].quick_fix);
    assert_eq!(listed.actions[3].kind, None);
    assert_eq!(listed.actions[4].disabled.as_deref(), Some("nothing to inline"));
    let sent = &fixture.server.requests("textDocument/codeAction")[0];
    assert_eq!(sent["range"], range(4, 13, 13));
    assert_eq!(sent["context"]["diagnostics"], json!([undefined_x()]));
    assert_eq!(sent["context"]["triggerKind"], 1);

    let again = list(&mut fixture, json!({}));
    assert_eq!(again, listed, "the same place is not asked again");
    assert_eq!(fixture.server.requests("textDocument/codeAction").len(), 1);
}

#[test]
fn a_caret_on_the_line_of_a_problem_asks_about_the_problem_and_a_selection_about_itself() {
    let mut fixture = project(false);
    fixture.publish("main.go", json!([undefined_x()]));
    fixture.server.on("textDocument/codeAction", json!([]));
    caret(&mut fixture, offset_of("\tfmt"));
    assert!(list(&mut fixture, json!({})).actions.is_empty());
    let sent = &fixture.server.requests("textDocument/codeAction")[0];
    assert_eq!(sent["range"], range(4, 13, 14));
    assert_eq!(sent["context"]["diagnostics"], json!([undefined_x()]));

    fixture.with(|_, workbench| {
        let selection = zen_editor::Selection::new(offset_of("func") + 4, offset_of("func"));
        workbench.active_document_mut().unwrap().set_selections(zen_editor::Selections::single(selection));
    });
    list(&mut fixture, json!({}));
    let sent = &fixture.server.requests("textDocument/codeAction")[1];
    assert_eq!(sent["range"], range(3, 0, 4));
    assert_eq!(sent["context"]["diagnostics"], json!([]));
}

#[test]
fn applying_an_edit_changes_every_file_as_one_undo_step() {
    let mut fixture = project(false);
    let edit = json!({ "documentChanges": [
        { "textDocument": { "uri": uri(&fixture, "main.go"), "version": 1 }, "edits": [{ "range": range(1, 0, 0), "newText": "import \"fmt\"\n" }] },
        { "textDocument": { "uri": uri(&fixture, "lib.go"), "version": null }, "edits": [{ "range": range(2, 8, 9), "newText": "2" }] }
    ] });
    fixture.server.on("textDocument/codeAction", json!([{ "title": "Fix", "kind": "quickfix", "edit": edit }]));
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let applied = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id })).unwrap();
    assert_eq!(applied["title"], "Fix");
    assert_eq!(applied["files"], json!([fixture.path("lib.go"), fixture.path("main.go")]));
    assert_eq!(applied["command"], Value::Null);
    assert_eq!(text(&fixture), MAIN.replacen('\n', "\nimport \"fmt\"\n", 1));
    assert_eq!(std::fs::read_to_string(fixture.path("lib.go")).unwrap(), "package main\n\nvar y = 2\n");
    assert!(fixture.server.requests("workspace/executeCommand").is_empty());

    let stale = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id }));
    assert_eq!(failure(stale), t!("workbench-intentions-unknown", id = listed.actions[0].id.as_str()));

    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), MAIN);
    assert_eq!(std::fs::read_to_string(fixture.path("lib.go")).unwrap(), LIB);
}

#[test]
fn a_command_runs_on_the_server_and_the_edit_it_sends_back_is_applied() {
    let mut fixture = project(false);
    let edit = import_fmt(&fixture);
    fixture.server.on(
        "textDocument/codeAction",
        json!([{ "title": "Add import", "kind": "quickfix", "command": { "title": "Add import", "command": "fix.all", "arguments": [{ "pkg": "fmt" }] } }]),
    );
    fixture.server.reply(
        "workspace/executeCommand",
        Reply::Ask {
            method: "workspace/applyEdit".to_owned(),
            params: std::sync::Arc::new(move |_| json!({ "label": "Add import", "edit": edit.clone() })),
            then: Value::Null,
        },
    );
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let applied = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id })).unwrap();
    assert_eq!(applied["command"], "fix.all");
    assert_eq!(applied["files"], json!([fixture.path("main.go")]));
    assert_eq!(text(&fixture), MAIN.replacen('\n', "\nimport \"fmt\"\n", 1));
    let sent = &fixture.server.requests("workspace/executeCommand")[0];
    assert_eq!(sent, &json!({ "command": "fix.all", "arguments": [{ "pkg": "fmt" }] }));
    assert_eq!(fixture.server.wait_for_responses(1, WAIT)[0]["result"], json!({ "applied": true }));
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), MAIN);
}

#[test]
fn an_action_without_its_edit_is_resolved_first() {
    let mut fixture = project(true);
    fixture
        .server
        .on("textDocument/codeAction", json!([{ "title": "Fill struct", "kind": "refactor.rewrite", "data": 7 }]));
    let edit = import_fmt(&fixture);
    fixture.server.on_with("codeAction/resolve", move |action| {
        let mut action = action.clone();
        action["edit"] = edit.clone();
        action
    });
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id })).unwrap();
    assert_eq!(fixture.server.requests("codeAction/resolve")[0]["data"], 7);
    assert_eq!(text(&fixture), MAIN.replacen('\n', "\nimport \"fmt\"\n", 1));
}

#[test]
fn an_action_with_nothing_to_do_or_disabled_is_refused() {
    let mut fixture = project(false);
    fixture.server.on(
        "textDocument/codeAction",
        json!([
            { "title": "Empty", "kind": "refactor" },
            { "title": "Inline", "kind": "refactor.inline", "disabled": { "reason": "nothing to inline" } }
        ]),
    );
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let ids: Vec<String> = listed.actions.iter().map(|action| action.id.clone()).collect();
    assert_eq!(failure(fixture.dispatch("editor.intentions.apply", json!({ "id": ids[1] }))), "nothing to inline");
    assert_eq!(
        failure(fixture.dispatch("editor.intentions.apply", json!({ "id": ids[0] }))),
        t!("workbench-intentions-nothing")
    );
    assert_eq!(text(&fixture), MAIN);
}

#[test]
fn actions_listed_before_the_text_changed_are_not_applied() {
    let mut fixture = project(false);
    let edit = import_fmt(&fixture);
    fixture.server.on("textDocument/codeAction", json!([{ "title": "Fix", "kind": "quickfix", "edit": edit }]));
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    fixture.with(|_, workbench| {
        workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "// x\n".to_owned())]);
    });
    let refused = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id }));
    assert_eq!(failure(refused), t!("workbench-intentions-stale"));
    assert_eq!(text(&fixture), format!("// x\n{MAIN}"));
}

#[test]
fn a_read_only_file_is_not_changed() {
    let mut fixture = project(false);
    let path = fixture.path("latin.go");
    std::fs::write(&path, b"package main\n\n// caf\xe9\n").unwrap();
    fixture.open("latin.go");
    let reason =
        fixture.session.state::<Workbench>().unwrap().active_document().unwrap().read_only().unwrap().to_owned();
    let edit = json!({ "changes": { uri(&fixture, "latin.go"): [{ "range": range(0, 0, 0), "newText": "// x\n" }] } });
    fixture.server.on("textDocument/codeAction", json!([{ "title": "Fix", "kind": "quickfix", "edit": edit }]));
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let refused = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id }));
    assert_eq!(failure(refused), reason);
    assert_eq!(std::fs::read(&path).unwrap(), b"package main\n\n// caf\xe9\n");
}

#[test]
fn an_edit_of_another_ides_project_settings_is_refused() {
    let mut fixture = project(false);
    std::fs::create_dir_all(fixture.path(".idea")).unwrap();
    std::fs::write(fixture.path(".idea/workspace.xml"), "<project/>\n").unwrap();
    let edit =
        json!({ "changes": { uri(&fixture, ".idea/workspace.xml"): [{ "range": range(0, 0, 0), "newText": "x" }] } });
    fixture.server.on("textDocument/codeAction", json!([{ "title": "Fix", "kind": "quickfix", "edit": edit }]));
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let refused = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id }));
    assert_eq!(failure(refused), crate::guard::refusal(&fixture.path(".idea/workspace.xml")));
    assert_eq!(std::fs::read_to_string(fixture.path(".idea/workspace.xml")).unwrap(), "<project/>\n");
}

#[test]
fn an_edit_the_server_sends_on_its_own_is_refused() {
    let mut fixture = project(false);
    let edit = import_fmt(&fixture);
    fixture.server.reply(
        "textDocument/hover",
        Reply::Ask {
            method: "workspace/applyEdit".to_owned(),
            params: std::sync::Arc::new(move |_| json!({ "edit": edit.clone() })),
            then: Value::Null,
        },
    );
    let path = fixture.path("main.go");
    let pending = fixture
        .with(|languages, _| languages.client_for(&path).unwrap().request("textDocument/hover", json!({})).unwrap());
    let deadline = Instant::now() + WAIT;
    while pending.try_answer().is_none() && Instant::now() < deadline {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let answer = &fixture.server.responses()[0]["result"];
    assert_eq!(answer["applied"], false);
    assert_eq!(answer["failureReason"], t!("workbench-intentions-unasked-edit"));
    assert_eq!(text(&fixture), MAIN);
}

#[test]
fn a_server_without_code_actions_lists_none() {
    let mut fixture = fixture(&[("main.go", MAIN)]);
    fixture.open("main.go");
    assert_eq!(list(&mut fixture, json!({})), IntentionList::default());
    assert!(fixture.server.requests("textDocument/codeAction").is_empty());
}

fn asks(edits: Vec<Value>) -> Reply {
    let asks = edits
        .into_iter()
        .map(|edit| {
            let make: zen_lsp::fake::Handler = std::sync::Arc::new(move |_| json!({ "edit": edit.clone() }));
            ("workspace/applyEdit".to_owned(), make)
        })
        .collect();
    Reply::AskAll { asks, then: Value::Null }
}

fn change(fixture: &Fixture, name: &str, range: Value, new_text: &str) -> Value {
    json!({ "changes": { uri(fixture, name): [{ "range": range, "newText": new_text }] } })
}

fn command_action(edit: Option<Value>) -> Value {
    let mut action =
        json!({ "title": "Fix all", "kind": "quickfix", "command": { "title": "Fix all", "command": "fix.all" } });
    if let Some(edit) = edit {
        action["edit"] = edit;
    }
    json!([action])
}

fn journal_len(fixture: &Fixture) -> usize {
    fixture.session.state::<Refactoring>().map_or(0, |refactoring| refactoring.journal().len())
}

#[test]
fn an_action_whose_server_sends_several_edits_is_one_undo_step_in_every_file() {
    let mut fixture = project(false);
    let edits = vec![
        change(&fixture, "main.go", range(5, 13, 14), "y"),
        change(&fixture, "main.go", range(0, 0, 0), "// a\n"),
        change(&fixture, "lib.go", range(2, 8, 9), "2"),
        change(&fixture, "lib.go", range(2, 8, 9), "3"),
    ];
    fixture.server.on("textDocument/codeAction", command_action(Some(import_fmt(&fixture))));
    fixture.server.reply("workspace/executeCommand", asks(edits));
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let applied = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id })).unwrap();
    assert_eq!(applied["files"], json!([fixture.path("lib.go"), fixture.path("main.go")]));
    assert_eq!(text(&fixture), "// a\npackage main\nimport \"fmt\"\n\n// привет\nfunc main() {\n\tfmt.Println(y)\n}\n");
    assert_eq!(std::fs::read_to_string(fixture.path("lib.go")).unwrap(), "package main\n\nvar y = 3\n");
    assert_eq!(journal_len(&fixture), 1);
    assert_eq!(fixture.session.state::<Refactoring>().unwrap().journal()[0].files.len(), 2);

    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), MAIN);
    assert_eq!(std::fs::read_to_string(fixture.path("lib.go")).unwrap(), LIB);
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(false));
    assert_eq!(fixture.dispatch("editor.edit.redo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), "// a\npackage main\nimport \"fmt\"\n\n// привет\nfunc main() {\n\tfmt.Println(y)\n}\n");
}

#[test]
fn a_refused_edit_of_the_server_undoes_the_whole_action() {
    let mut fixture = project(false);
    std::fs::create_dir_all(fixture.path(".idea")).unwrap();
    std::fs::write(fixture.path(".idea/workspace.xml"), "<project/>\n").unwrap();
    let edits = vec![
        change(&fixture, "lib.go", range(2, 8, 9), "2"),
        change(&fixture, ".idea/workspace.xml", range(0, 0, 0), "x"),
        change(&fixture, "main.go", range(0, 0, 0), "// late\n"),
    ];
    fixture.server.on("textDocument/codeAction", command_action(Some(import_fmt(&fixture))));
    fixture.server.reply("workspace/executeCommand", asks(edits));
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let refused = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id }));
    assert_eq!(failure(refused), crate::guard::refusal(&fixture.path(".idea/workspace.xml")));
    assert_eq!(text(&fixture), MAIN);
    assert_eq!(std::fs::read_to_string(fixture.path("lib.go")).unwrap(), LIB);
    assert_eq!(std::fs::read_to_string(fixture.path(".idea/workspace.xml")).unwrap(), "<project/>\n");
    let answers: Vec<Value> =
        fixture.server.wait_for_responses(3, WAIT).iter().map(|answer| answer["result"]["applied"].clone()).collect();
    assert_eq!(answers, [json!(true), json!(false), json!(false)]);
    assert_eq!(journal_len(&fixture), 0);
}

#[test]
fn an_edit_outside_the_project_or_through_a_link_out_of_it_is_refused() {
    let mut fixture = project(false);
    let elsewhere = tempfile::tempdir().unwrap();
    let outside = elsewhere.path().join("secret.go");
    std::fs::write(&outside, "package secret\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, fixture.path("link.go")).unwrap();
    let outside_uri = zen_lsp::position::file_uri(&outside);
    let mut targets = vec![(outside.clone(), outside_uri)];
    if fixture.path("link.go").exists() {
        targets.push((fixture.path("link.go"), uri(&fixture, "link.go")));
    }
    for (offset, (path, target)) in targets.into_iter().enumerate() {
        let edit = json!({ "changes": { target: [{ "range": range(0, 0, 0), "newText": "// x\n" }] } });
        fixture.server.on("textDocument/codeAction", json!([{ "title": "Fix", "kind": "quickfix", "edit": edit }]));
        let listed = list(&mut fixture, json!({ "offset": offset }));
        let refused = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id }));
        assert_eq!(failure(refused), t!("workbench-edit-outside-project", path = path.display().to_string()));
    }
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "package secret\n");
}

#[test]
fn edits_the_server_sends_before_or_after_its_command_are_refused() {
    let mut fixture = project(false);
    fixture.server.on("textDocument/codeAction", command_action(None));
    fixture.server.on("workspace/executeCommand", Value::Null);
    let unasked = |id: &str, fixture: &Fixture| {
        let request = zen_lsp::ServerRequest {
            id: json!(id),
            method: "workspace/applyEdit".to_owned(),
            params: json!({ "edit": import_fmt(fixture) }),
        };
        ("go".to_owned(), request)
    };
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let early = unasked("early", &fixture);
    fixture.with(|languages, _| languages.server_requests.push(early));
    let applied = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id })).unwrap();
    assert_eq!(applied["files"], json!([]));
    let late = unasked("late", &fixture);
    fixture.with(|languages, _| languages.server_requests.push(late));
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let answers = fixture.server.wait_for_responses(2, WAIT);
    let ids: Vec<&Value> = answers.iter().map(|answer| &answer["id"]).collect();
    assert_eq!(ids, [&json!("early"), &json!("late")]);
    assert!(answers.iter().all(|answer| answer["result"]["applied"] == false));
    assert_eq!(text(&fixture), MAIN);
}

#[test]
fn while_a_command_runs_only_its_servers_edits_wait_for_the_ide() {
    let mut fixture = project(false);
    let request = |id: &str| zen_lsp::ServerRequest {
        id: json!(id),
        method: "workspace/applyEdit".to_owned(),
        params: json!({ "edit": {} }),
    };
    fixture.with(|languages, workbench| {
        languages.set_executing(Some("go"));
        languages.server_requests.push(("rust".to_owned(), request("other")));
        languages.server_requests.push(("go".to_owned(), request("own")));
        languages.sync(workbench);
        let kept: Vec<&str> = languages.server_requests.iter().map(|(server, _)| server.as_str()).collect();
        assert_eq!(kept, ["go"]);
        languages.set_executing(None);
        languages.sync(workbench);
        assert!(languages.server_requests.is_empty());
    });
}

#[test]
fn an_action_that_creates_a_file_writes_it_and_one_undo_removes_it() {
    let mut fixture = project(false);
    let edit = json!({ "documentChanges": [
        { "kind": "create", "uri": uri(&fixture, "x/x.go") },
        { "textDocument": { "uri": uri(&fixture, "x/x.go"), "version": null }, "edits": [{ "range": range(0, 0, 0), "newText": "package main\n\nvar x = 1\n" }] },
        { "textDocument": { "uri": uri(&fixture, "main.go"), "version": null }, "edits": [{ "range": range(2, 0, 0), "newText": "// x\n" }] }
    ] });
    fixture
        .server
        .on("textDocument/codeAction", json!([{ "title": "Create variable x", "kind": "quickfix", "edit": edit }]));
    let listed = list(&mut fixture, json!({ "offset": 0 }));
    let applied = fixture.dispatch("editor.intentions.apply", json!({ "id": listed.actions[0].id })).unwrap();
    assert_eq!(applied["files"], json!([fixture.path("main.go"), fixture.path("x/x.go")]));
    assert_eq!(std::fs::read_to_string(fixture.path("x/x.go")).unwrap(), "package main\n\nvar x = 1\n");
    assert_eq!(fixture.dispatch("editor.edit.undo", json!(null)).unwrap(), json!(true));
    assert_eq!(text(&fixture), MAIN);
    assert!(!fixture.path("x").exists());
}
