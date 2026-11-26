use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::DispatchError;
use zen_lsp::fake::Reply;

use super::*;
use crate::language::testing::{Fixture, fixture};

const MAIN: &str = "package main\n\nfunc main() {\n\tfmt.Println(total(a) + 1)\n}\n";

fn range(line: u64, start: u64, end: u64) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
}

fn uri(fixture: &Fixture) -> String {
    zen_lsp::position::file_uri(&fixture.path("main.go"))
}

fn project() -> Fixture {
    let mut fixture = fixture(&[("main.go", MAIN)]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "codeActionProvider": { "resolveProvider": false },
        "executeCommandProvider": { "commands": ["gopls.apply_fix"] }
    }));
    fixture.open("main.go");
    fixture
}

fn offset_of(needle: &str) -> usize {
    MAIN[..MAIN.find(needle).unwrap()].chars().count()
}

fn caret(fixture: &mut Fixture, offset: usize) {
    fixture.with(|_, workbench| {
        let selection = zen_editor::Selections::single(zen_editor::Selection::caret(offset));
        workbench.active_document_mut().unwrap().set_selections(selection);
    });
}

fn text(fixture: &Fixture) -> String {
    fixture.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
}

fn extract(fixture: &mut Fixture, command: &str, params: Value) -> Result<Extraction, DispatchError> {
    fixture.dispatch(command, params).map(|value| serde_json::from_value(value).unwrap())
}

fn failure(result: Result<Extraction, DispatchError>) -> String {
    match result {
        Err(DispatchError::Failed { error, .. }) => error.message,
        other => panic!("expected a failure, got {other:?}"),
    }
}

fn extract_variable(fixture: &Fixture) -> Value {
    json!({
        "title": "Extract variable",
        "kind": "refactor.extract.variable",
        "edit": { "changes": { uri(fixture): [
            { "range": range(3, 0, 0), "newText": "\tx := total(a) + 1\n" },
            { "range": range(3, 13, 25), "newText": "x" }
        ] } }
    })
}

#[test]
fn every_server_names_its_actions_its_own_way() {
    let cases = [
        ("refactor.extract.variable", "Extract to local variable", Some(Extract::Variable)),
        ("refactor.extract.variable", "Extract to local variable (replace all occurrences)", Some(Extract::Variable)),
        ("refactor.extract.constant", "Extract to constant", Some(Extract::Constant)),
        ("refactor.extract.field", "Extract to field", Some(Extract::Field)),
        ("refactor.introduce.parameter", "Introduce Parameter", Some(Extract::Parameter)),
        ("refactor.extract.function", "Extract to method", Some(Extract::Method)),
        ("refactor.inline", "Inline", Some(Extract::Inline)),
        ("refactor.extract", "Extract into variable", Some(Extract::Variable)),
        ("refactor.extract", "Extract into constant", Some(Extract::Constant)),
        ("refactor.extract", "Extract into function", Some(Extract::Method)),
        ("refactor.extract", "Extract into module", None),
        ("refactor.extract", "Extract type as type alias", None),
        ("refactor.inline", "Inline into all callers", Some(Extract::Inline)),
        ("refactor.extract.variable-all", "Extract 2 occurrences of total(a)", Some(Extract::Variable)),
        ("refactor.extract.method", "Extract method", Some(Extract::Method)),
        ("refactor.extract.toNewFile", "Extract declarations to new file", None),
        ("refactor.inline.call", "Inline call to total", Some(Extract::Inline)),
        ("refactor.extract.constant", "Extract to constant in enclosing scope", Some(Extract::Variable)),
        ("refactor.extract.constant", "Extract to constant in module scope", Some(Extract::Constant)),
        ("refactor.extract.constant", "Extract to readonly field in class 'Cart'", Some(Extract::Field)),
        ("refactor.extract.function", "Extract to function in module scope", Some(Extract::Method)),
        ("refactor.extract.type", "Extract to type alias", None),
        ("refactor.inline.variable", "Inline variable", Some(Extract::Inline)),
        ("quickfix", "Create variable", None),
    ];
    for (kind, title, wanted) in cases {
        assert_eq!(refactoring_of(kind, title), wanted, "{kind} {title}");
    }
}

#[test]
fn several_expressions_at_the_caret_are_listed_before_anything_changes() {
    let mut fixture = project();
    fixture.server.on("textDocument/codeAction", json!([]));
    caret(&mut fixture, offset_of("a)"));
    let listed = extract(&mut fixture, "refactor.extract.variable", json!({})).unwrap();
    let texts: Vec<&str> = listed.expressions.iter().map(|expression| expression.text.as_str()).collect();
    assert_eq!(texts, ["a", "total(a)", "total(a) + 1"]);
    let last = &listed.expressions[2];
    assert_eq!((last.start, last.end), (offset_of("total"), offset_of(")\n}")));
    assert!(fixture.server.requests("textDocument/codeAction").is_empty(), "the server is asked once one is chosen");
    assert_eq!(text(&fixture), MAIN);
    let inline = failure(extract(&mut fixture, "refactor.inline.apply", json!({})));
    assert_eq!(inline, zen_i18n::t!("workbench-extract-none", refactoring = zen_i18n::t!("workbench-extract-inline")));
    assert_eq!(
        fixture.server.requests("textDocument/codeAction")[0]["range"],
        range(3, 19, 19),
        "inline asks at the caret"
    );
}

#[test]
fn the_chosen_expression_is_extracted_and_its_new_name_found_everywhere() {
    let mut fixture = project();
    let action = extract_variable(&fixture);
    fixture.server.on(
        "textDocument/codeAction",
        json!([{ "title": "Extract function", "kind": "refactor.extract.function", "edit": {} }, action]),
    );
    let (start, end) = (offset_of("total"), offset_of(")\n}"));
    let done = extract(&mut fixture, "refactor.extract.variable", json!({ "start": start, "end": end })).unwrap();
    let sent = &fixture.server.requests("textDocument/codeAction")[0];
    assert_eq!(sent["range"], range(3, 13, 25));
    assert_eq!(sent["context"]["only"], json!(["refactor.extract", "refactor.introduce"]));
    let after = "package main\n\nfunc main() {\n\tx := total(a) + 1\n\tfmt.Println(x)\n}\n";
    assert_eq!(text(&fixture), after);
    assert_eq!(done.title.as_deref(), Some("Extract variable"));
    assert_eq!(done.name.as_deref(), Some("x"));
    let at: Vec<usize> = done.occurrences.iter().map(|occurrence| occurrence.start).collect();
    let find = |needle: &str| after[..after.find(needle).unwrap()].chars().count();
    assert_eq!(at, [find("x :="), find("x)")]);
    assert_eq!(done.suggestions, ["x"], "a sum gives no name of its own");
    assert_eq!(done.files, [fixture.path("main.go")]);

    fixture.dispatch("editor.edit.undo", json!({})).unwrap();
    assert_eq!(text(&fixture), MAIN, "one undo step");
}

#[test]
fn a_given_name_replaces_the_servers_in_the_same_undo_step() {
    let mut fixture = project();
    let action = extract_variable(&fixture);
    fixture.server.on("textDocument/codeAction", json!([action]));
    let (start, end) = (offset_of("total"), offset_of(")\n}"));
    let params = json!({ "start": start, "end": end, "name": "sum" });
    let done = extract(&mut fixture, "refactor.extract.variable", params).unwrap();
    assert_eq!(text(&fixture), "package main\n\nfunc main() {\n\tsum := total(a) + 1\n\tfmt.Println(sum)\n}\n");
    assert_eq!(done.name.as_deref(), Some("sum"));
    assert_eq!(done.occurrences.len(), 2);
    fixture.dispatch("editor.edit.undo", json!({})).unwrap();
    assert_eq!(text(&fixture), MAIN);

    let refused = failure(extract(&mut fixture, "refactor.extract.variable", json!({ "start": start, "name": "1x" })));
    assert_eq!(refused, zen_i18n::t!("workbench-rename-invalid-name", name = "1x"));
    assert_eq!(text(&fixture), MAIN);
}

#[test]
fn several_ways_are_offered_and_a_variant_picks_one() {
    let mut fixture = project();
    let mut all = extract_variable(&fixture);
    all["title"] = json!("Extract 1 occurrence of total(a) + 1");
    all["kind"] = json!("refactor.extract.variable-all");
    fixture.server.on("textDocument/codeAction", json!([extract_variable(&fixture), all]));
    let (start, end) = (offset_of("total"), offset_of(")\n}"));
    let offered = extract(&mut fixture, "refactor.extract.variable", json!({ "start": start, "end": end })).unwrap();
    assert_eq!(offered.variants, ["Extract variable", "Extract 1 occurrence of total(a) + 1"]);
    assert_eq!(text(&fixture), MAIN);
    let params = json!({ "start": start, "end": end, "variant": 1 });
    let done = extract(&mut fixture, "refactor.extract.variable", params).unwrap();
    assert_eq!(done.title.as_deref(), Some("Extract 1 occurrence of total(a) + 1"));
    assert_eq!(fixture.server.requests("textDocument/codeAction").len(), 1, "the answer is kept for the choice");
}

#[test]
fn a_snippet_placeholder_names_what_was_extracted() {
    let mut fixture = project();
    fixture.server.on(
        "textDocument/codeAction",
        json!([{
            "title": "Extract into function",
            "kind": "refactor.extract",
            "edit": { "documentChanges": [{ "textDocument": { "uri": uri(&fixture), "version": null }, "edits": [
                { "range": range(3, 13, 25), "newText": "fun_name(a)", "insertTextFormat": 1 },
                { "range": range(4, 1, 1), "newText": "\n\nfunc ${0:fun_name}(a int) int { return total(a) + 1 }", "insertTextFormat": 2 }
            ] }] }
        }]),
    );
    let (start, end) = (offset_of("total"), offset_of(")\n}"));
    let done = extract(&mut fixture, "refactor.extract.method", json!({ "start": start, "end": end })).unwrap();
    let after = "package main\n\nfunc main() {\n\tfmt.Println(fun_name(a))\n}\n\nfunc fun_name(a int) int { return total(a) + 1 }\n";
    assert_eq!(text(&fixture), after, "the snippet goes in as plain text");
    assert_eq!(done.name.as_deref(), Some("fun_name"));
    assert_eq!(done.occurrences.len(), 2);
    assert_eq!(placeholder_name("let $0var_name = 1;").as_deref(), Some("var_name"));
    assert_eq!(placeholder_name(r"cost \$5 ${1:total}").as_deref(), Some("total"));
    assert_eq!(placeholder_name("plain"), None);
}

#[test]
fn an_inline_through_a_server_command_applies_the_edit_it_sends() {
    let mut fixture = project();
    fixture.server.on(
        "textDocument/codeAction",
        json!([{
            "title": "Inline call to total",
            "kind": "refactor.inline.call",
            "command": { "title": "Inline call to total", "command": "gopls.apply_fix", "arguments": [{ "fix": "inline_call" }] }
        }]),
    );
    let edit = json!({ "changes": { uri(&fixture): [{ "range": range(3, 13, 21), "newText": "a * 2" }] } });
    fixture.server.reply(
        "workspace/executeCommand",
        Reply::Ask {
            method: "workspace/applyEdit".to_owned(),
            params: std::sync::Arc::new(move |_| json!({ "edit": edit.clone() })),
            then: Value::Null,
        },
    );
    caret(&mut fixture, offset_of("total"));
    let done = extract(&mut fixture, "refactor.inline.apply", json!({})).unwrap();
    assert_eq!(text(&fixture), "package main\n\nfunc main() {\n\tfmt.Println(a * 2 + 1)\n}\n");
    assert_eq!(done.name, None, "nothing to name after inlining");
    assert_eq!(fixture.server.requests("textDocument/codeAction")[0]["context"]["only"], json!(["refactor.inline"]));
}

#[test]
fn a_disabled_action_tells_why_and_nothing_offered_says_so() {
    let mut fixture = project();
    fixture.server.on(
        "textDocument/codeAction",
        json!([{ "title": "Extract constant", "kind": "refactor.extract.constant", "disabled": { "reason": "not constant" } }]),
    );
    let (start, end) = (offset_of("total"), offset_of(")\n}"));
    let refused = failure(extract(&mut fixture, "refactor.extract.constant", json!({ "start": start, "end": end })));
    assert_eq!(refused, "not constant");
    let refused = failure(extract(&mut fixture, "refactor.extract.field", json!({ "start": start, "end": end })));
    assert_eq!(refused, zen_i18n::t!("workbench-extract-none", refactoring = zen_i18n::t!("workbench-extract-field")));
}

#[test]
fn suggestions_follow_the_names_style() {
    assert_eq!(suggestions("x", "user.getFullName()", Extract::Variable, Some("go")), ["x", "fullName", "name"]);
    assert_eq!(
        suggestions("var_name", "self.read_config(path)", Extract::Variable, Some("rust")),
        ["var_name", "config"]
    );
    assert_eq!(
        suggestions("EXTRACTED", "order.totalPrice", Extract::Constant, Some("java")),
        ["EXTRACTED", "TOTAL_PRICE", "PRICE"]
    );
    assert_eq!(suggestions("x", "1 + 2", Extract::Variable, Some("go")), ["x"]);
}

fn real_server(program: PathBuf, language: &str, extension: &str, files: &[(&str, &str)], open: &str) -> Fixture {
    let mut fixture = fixture(files);
    fixture.session.state_mut::<Languages>().set_specs(vec![crate::language::ServerSpec {
        language_id: language.to_owned(),
        extensions: vec![extension.to_owned()],
        launch: std::sync::Arc::new(move |root: &Path| {
            Ok(zen_lsp::process_launcher(program.clone(), Vec::new(), root.to_owned(), Vec::new()))
        }),
        initialization_options: json!({}),
    }]);
    fixture.open(open);
    fixture
}

fn program(name: &str, candidates: &[&str]) -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut found: Vec<PathBuf> = candidates.iter().map(|candidate| home.join(candidate)).collect();
    if let Some(path) = std::env::var_os("PATH") {
        found.extend(std::env::split_paths(&path).map(|folder| folder.join(name)));
    }
    found.into_iter().find(|path| path.is_file()).unwrap_or_else(|| panic!("{name} is not installed"))
}

fn until_extracted(fixture: &mut Fixture, command: &str, params: Value) -> Extraction {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        match extract(fixture, command, params.clone()) {
            Ok(done) if done.title.is_some() || !done.variants.is_empty() => return done,
            other => assert!(Instant::now() < deadline, "{command}: {other:?}"),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn char_range(text: &str, needle: &str) -> Value {
    let start = text[..text.find(needle).unwrap()].chars().count();
    json!({ "start": start, "end": start + needle.chars().count(), "waitMs": 10000 })
}

#[test]
#[ignore = "needs gopls on PATH or in ~/go/bin"]
fn gopls_extracts_a_variable_and_a_function_and_inlines_a_call() {
    let gopls = program("gopls", &["go/bin/gopls"]);
    let main = "package main\n\nimport \"fmt\"\n\nfunc total(a int) int { return a * 2 }\n\nfunc main() {\n\ta := 1\n\tfmt.Println(total(a) + 1)\n}\n";
    let mut fixture = real_server(
        gopls,
        "go",
        "go",
        &[("go.mod", "module example.com/x\n\ngo 1.22\n"), ("main.go", main)],
        "main.go",
    );
    let done = until_extracted(&mut fixture, "refactor.extract.variable", char_range(main, "total(a) + 1"));
    let done = if done.variants.is_empty() {
        done
    } else {
        checked(&format!("gopls variants: {:?}", done.variants));
        let mut params = char_range(main, "total(a) + 1");
        params["variant"] = json!(0);
        until_extracted(&mut fixture, "refactor.extract.variable", params)
    };
    checked(&format!("gopls extract variable: {:?} {:?} {:?}", done.title, done.name, done.occurrences.len()));
    let name = done.name.clone().unwrap();
    assert!(text(&fixture).contains(&format!("{name} := total(a) + 1")), "{}", text(&fixture));
    assert_eq!(done.occurrences.len(), 2);
    fixture.dispatch("editor.edit.undo", json!({})).unwrap();
    assert_eq!(text(&fixture), main);

    let call = "a := 1\n\tfmt.Println(total(a) + 1)";
    let done = until_extracted(&mut fixture, "refactor.extract.method", char_range(main, call));
    let done = if done.variants.is_empty() {
        done
    } else {
        checked(&format!("gopls method variants: {:?}", done.variants));
        let mut params = char_range(main, call);
        params["variant"] = json!(0);
        until_extracted(&mut fixture, "refactor.extract.method", params)
    };
    checked(&format!("gopls extract function: {:?} {:?} {:?}", done.title, done.name, done.occurrences.len()));
    assert!(done.occurrences.len() >= 2, "{done:?}\n{}", text(&fixture));
    fixture.dispatch("editor.edit.undo", json!({})).unwrap();
    assert_eq!(text(&fixture), main);

    let mut params = char_range(main, "total(a)");
    params["end"] = params["start"].clone();
    let done = until_extracted(&mut fixture, "refactor.inline.apply", params);
    checked(&format!("gopls inline: {:?}", done.title));
    assert!(!text(&fixture).contains("Println(total(a)"), "{}", text(&fixture));
}

#[test]
#[ignore = "needs rust-analyzer on PATH or in ~/.cargo/bin"]
fn rust_analyzer_extracts_a_variable_with_a_snippet_and_a_function() {
    let analyzer = program("rust-analyzer", &[".cargo/bin/rust-analyzer"]);
    let main = "fn total(a: i32) -> i32 {\n    a * 2\n}\n\nfn main() {\n    let a = 1;\n    let b = (total(a) + 1) * 3;\n    println!(\"{b}\");\n}\n";
    let manifest = "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
    let mut fixture =
        real_server(analyzer, "rust", "rs", &[("Cargo.toml", manifest), ("src/main.rs", main)], "src/main.rs");
    let done = until_extracted(&mut fixture, "refactor.extract.variable", char_range(main, "total(a) + 1"));
    checked(&format!(
        "rust-analyzer extract variable: {:?} {:?} {:?} {:?}",
        done.title,
        done.name,
        done.occurrences.len(),
        done.suggestions
    ));
    let name = done.name.clone().unwrap();
    assert!(text(&fixture).contains(&format!("let {name} = total(a) + 1;")), "{}", text(&fixture));
    assert!(!text(&fixture).contains('$'), "the snippet goes in as plain text");
    assert_eq!(done.occurrences.len(), 2);
    fixture.dispatch("editor.edit.undo", json!({})).unwrap();
    assert_eq!(text(&fixture), main);

    let done =
        until_extracted(&mut fixture, "refactor.extract.method", char_range(main, "let b = (total(a) + 1) * 3;"));
    checked(&format!("rust-analyzer extract function: {:?} {:?} {:?}", done.title, done.name, done.occurrences.len()));
    assert!(done.occurrences.len() >= 2, "{done:?}\n{}", text(&fixture));
}

#[test]
fn a_name_that_was_there_before_is_found_by_its_new_uses_and_a_rename_place_names_it() {
    let before = "double total = shape.area();\n";
    let after = "double area = shape.area();\ndouble total = area;\n";
    assert_eq!(new_name(before, after, 15).as_deref(), Some("area"), "a name used more than before");
    assert_eq!(occurrences(before, after, "area").len(), 2, "the method call is not one of them");
    let field = "class A {\n    private double area;\n    void f() {\n        area = shape.area();\n        total = area;\n    }\n}\n";
    let had = "class A {\n    void f() {\n        total = shape.area();\n    }\n}\n";
    assert_eq!(new_name(had, field, 30).as_deref(), Some("area"), "a modifier the change brought is not the name");
    let arguments = json!([{ "textDocument": { "uri": "file:///a.ts" }, "position": { "line": 1, "character": 17 } }]);
    assert_eq!(renamed_at(after, &arguments).as_deref(), Some("area"));
}

#[allow(clippy::print_stderr, reason = "what the real servers answered is what the test reports")]
fn checked(what: &str) {
    eprintln!("{what}");
}

#[test]
fn a_file_the_server_changes_on_disk_keeps_its_version_in_local_history() {
    let mut fixture = fixture(&[("main.go", MAIN), ("other.go", "package main\n")]);
    let data = tempfile::tempdir().unwrap();
    fixture
        .session
        .set_state(crate::local_history::LocalHistory::open(data.path().join("history"), Default::default()));
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "codeActionProvider": { "resolveProvider": false }
    }));
    fixture.open("main.go");
    let other = zen_lsp::position::file_uri(&fixture.path("other.go"));
    let edit = json!({ "changes": { other: [{ "range": range(0, 0, 0), "newText": "// made\n" }] } });
    fixture.server.on(
        "textDocument/codeAction",
        json!([{ "title": "Extract to variable", "kind": "refactor.extract.variable", "edit": edit }]),
    );
    let at = offset_of("total");
    extract(&mut fixture, "refactor.extract.variable", json!({ "start": at, "end": at + 5 })).unwrap();
    let path = fixture.path("other.go");
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("// made"));
    let versions = fixture.dispatch("localHistory.versions.list", json!({ "path": path })).unwrap();
    assert!(versions.to_string().contains("Extract or Inline"), "{versions}");
}
