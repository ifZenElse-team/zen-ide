use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_lsp::servers::{SuppressionDeclaration, SuppressionForm};
use zen_syntax::{HighlightKind, OutlineItem, Outliner, Span, SuppressedLines, SymbolKind};

use super::*;
use crate::language::testing::{self, Fixture};

const MAIN: &str = "// Package main.\npackage main\n\nfunc first() {\n\tfor i, _ := range s {\n\t}\n}\n\nvar x = 1\n";

struct Funcs;

impl Outliner for Funcs {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn outline(&self, text: &str) -> Vec<OutlineItem> {
        let chars = |at: usize| text[..at].chars().count();
        text.match_indices("func first")
            .map(|(at, _)| {
                let end = at + text[at..].find("\n}\n").unwrap_or(0) + 2;
                OutlineItem {
                    name: "first".to_owned(),
                    detail: None,
                    kind: SymbolKind::Function,
                    start: chars(at),
                    end: chars(end),
                    selection_start: chars(at) + 5,
                    children: Vec::new(),
                }
            })
            .collect()
    }

    fn reads_suppressions(&self) -> bool {
        true
    }

    fn suppressions(&self, text: &str) -> Vec<SuppressedLines> {
        text.lines()
            .enumerate()
            .filter_map(|(at, line)| {
                let list = line.split_once("//nolint:")?.1.split_whitespace().next()?;
                let line = at as u32 + 1;
                Some(SuppressedLines { lines: line..=line + 1, ids: list.split(',').map(str::to_owned).collect() })
            })
            .collect()
    }
}

fn go_forms() -> SuppressionDeclaration {
    let form = |line_end, before: Option<&str>| {
        Some(SuppressionForm { text: "//nolint:{id}".to_owned(), line_end, before: before.map(str::to_owned) })
    };
    SuppressionDeclaration {
        sources: Vec::new(),
        separator: ",".to_owned(),
        statement: form(true, None),
        function: form(false, None),
        file: form(false, Some("package ")),
    }
}

fn diagnostic(line: u32, severity: u32, source: &str, code: &str) -> Value {
    json!({
        "range": { "start": { "line": line, "character": 1 }, "end": { "line": line, "character": 4 } },
        "severity": severity, "code": code, "source": source, "message": format!("{source} here")
    })
}

fn fixture(text: &str, diagnostics: Vec<Value>) -> Fixture {
    let mut fixture = testing::fixture(&[("main.go", text)]);
    fixture.session.state_mut::<Outliners>().set(vec![Arc::new(Funcs)]);
    fixture.suppresses(go_forms());
    fixture.server.publish_on(
        "textDocument/didOpen",
        "textDocument/publishDiagnostics",
        move |params| json!({ "uri": params["textDocument"]["uri"], "diagnostics": diagnostics }),
    );
    fixture.open("main.go");
    settle(&mut fixture, |problems| !problems.is_empty());
    fixture
}

fn text_of_main(fixture: &Fixture) -> String {
    text_of(&fixture.session, &fixture.path("main.go")).unwrap()
}

fn problems(fixture: &mut Fixture) -> Vec<Value> {
    let path = fixture.path("main.go");
    let answer = fixture.dispatch("problems.file.list", json!({ "path": path })).unwrap();
    crate::language::listed_problems(&answer)
}

fn settle(fixture: &mut Fixture, done: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        fixture.dispatch("language.servers.sync", json!({})).unwrap();
        let found = problems(fixture);
        if done(&found) || Instant::now() > deadline {
            return found;
        }
        std::thread::yield_now();
    }
}

fn analyzer() -> Value {
    diagnostic(4, 2, "simplifyrange", "default")
}

fn offset_of(fixture: &mut Fixture, line: usize) -> usize {
    let text = text_of_main(fixture);
    text.split_inclusive('\n').take(line).map(|line| line.chars().count()).sum::<usize>() + 1
}

fn options(fixture: &mut Fixture, offset: usize) -> Vec<Value> {
    let path = fixture.path("main.go");
    let list = fixture.dispatch("inspect.suppress.list", json!({ "path": path, "offset": offset })).unwrap();
    list.as_array().unwrap().clone()
}

fn apply(fixture: &mut Fixture, offset: usize, inspection: &str, scope: &str) -> Value {
    let path = fixture.path("main.go");
    let params = json!({ "path": path, "offset": offset, "inspection": inspection, "scope": scope });
    fixture.dispatch("inspect.suppress.apply", params).unwrap()
}

#[test]
fn a_problem_is_offered_for_its_statement_function_and_file() {
    let mut fixture = fixture(MAIN, vec![analyzer()]);
    let at = offset_of(&mut fixture, 4);
    let listed = options(&mut fixture, at);
    let scopes: Vec<&str> = listed.iter().map(|option| option["scope"].as_str().unwrap()).collect();
    assert_eq!(scopes, ["statement", "function", "file"]);
    assert_eq!(listed[0]["title"], "Suppress simplifyrange for statement");
    assert_eq!(listed[0]["inspection"], "simplifyrange/default");
}

#[test]
fn outside_a_function_there_is_no_function_scope() {
    let mut fixture = fixture(MAIN, vec![diagnostic(8, 2, "unused", "")]);
    let at = offset_of(&mut fixture, 8);
    let listed = options(&mut fixture, at);
    let scopes: Vec<&str> = listed.iter().map(|option| option["scope"].as_str().unwrap()).collect();
    assert_eq!(scopes, ["statement", "file"]);
}

#[test]
fn a_compiler_error_is_not_offered_and_a_directive_does_not_hide_it() {
    let text = "package main\n\nfunc first() {\n\tx := 1 //nolint:compiler\n}\n";
    let mut fixture = fixture(text, vec![diagnostic(3, 1, "compiler", "UnusedVar")]);
    let at = offset_of(&mut fixture, 3);
    assert_eq!(options(&mut fixture, at), Vec::<Value>::new());
    assert_eq!(problems(&mut fixture).len(), 1);
}

#[test]
fn a_statement_suppression_ends_the_line_hides_the_problem_and_one_undo_takes_it_back() {
    let mut fixture = fixture(MAIN, vec![analyzer()]);
    let at = offset_of(&mut fixture, 4);
    assert_eq!(apply(&mut fixture, at, "simplifyrange/default", "statement")["changed"], true);
    assert!(text_of_main(&fixture).contains("\tfor i, _ := range s { //nolint:simplifyrange\n"));
    assert_eq!(settle(&mut fixture, <[Value]>::is_empty), Vec::<Value>::new());
    fixture.dispatch("editor.edit.undo", json!({})).unwrap();
    assert_eq!(text_of_main(&fixture), MAIN);
}

#[test]
fn a_function_suppression_goes_above_the_function() {
    let mut fixture = fixture(MAIN, vec![analyzer()]);
    let at = offset_of(&mut fixture, 4);
    apply(&mut fixture, at, "simplifyrange/default", "function");
    assert!(text_of_main(&fixture).contains("\n\n//nolint:simplifyrange\nfunc first() {\n"));
}

#[test]
fn a_file_suppression_goes_above_the_package_clause() {
    let mut fixture = fixture(MAIN, vec![analyzer()]);
    let at = offset_of(&mut fixture, 4);
    apply(&mut fixture, at, "simplifyrange/default", "file");
    assert!(text_of_main(&fixture).starts_with("// Package main.\n//nolint:simplifyrange\npackage main\n"));
}

#[test]
fn a_second_inspection_joins_the_directive_that_is_there() {
    let text = "package main\n\nvar x = 1 //nolint:unused // kept\n";
    let mut fixture = fixture(text, vec![diagnostic(2, 2, "shadow", "")]);
    let at = offset_of(&mut fixture, 2);
    fixture.session.state_mut::<Suppressions>().files.clear();
    let path = fixture.path("main.go");
    let problem = Problem {
        start: at,
        end: at + 3,
        line: 3,
        column: 2,
        severity: Severity::Warning,
        message: "shadow here".to_owned(),
        source: Some("shadow".to_owned()),
        code: None,
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        deprecated: false,
        unused: false,
    };
    assert!(write(&mut fixture.session, &path, &problem, SuppressScope::Statement).unwrap().is_some());
    assert!(text_of_main(&fixture).contains("var x = 1 //nolint:unused,shadow // kept\n"));
    assert!(
        write(&mut fixture.session, &path, &problem, SuppressScope::Statement).unwrap().is_none(),
        "what the directive names is not written again"
    );
}

#[test]
fn the_context_actions_of_a_problem_offer_and_apply_a_suppression() {
    let mut fixture = fixture(MAIN, vec![analyzer()]);
    let path = fixture.path("main.go");
    let at = offset_of(&mut fixture, 4);
    let list =
        fixture.dispatch("editor.intentions.list", json!({ "path": path, "offset": at, "waitMs": 5000 })).unwrap();
    let titles: Vec<&str> = list["actions"].as_array().unwrap().iter().map(|a| a["title"].as_str().unwrap()).collect();
    assert_eq!(
        titles,
        [
            "Suppress simplifyrange for statement",
            "Suppress simplifyrange for function",
            "Suppress simplifyrange for file"
        ]
    );
    let id = list["actions"][2]["id"].clone();
    fixture.dispatch("editor.intentions.apply", json!({ "id": id })).unwrap();
    assert!(text_of_main(&fixture).contains("//nolint:simplifyrange\npackage main\n"));
}

#[test]
fn a_problem_with_quick_fixes_lists_suppressions_after_them() {
    let mut fixture = fixture(MAIN, vec![analyzer()]);
    let path = fixture.path("main.go");
    let at = offset_of(&mut fixture, 4);
    let mut problems = crate::inspections::all_problems(&mut fixture.session, &path);
    assert_eq!(problems.len(), 1);
    let main = InspectionFix {
        id: "main".to_owned(),
        title: "Main fix".to_owned(),
        kind: FixKind::Apply,
        batch: None,
        label: None,
        inline: false,
        args: Value::Null,
    };
    problems[0].fixes.push(main);
    with_fixes(&mut fixture.session, &path, &mut problems);
    assert_eq!(problems[0].fixes[0].id, "main");
    let fix = &problems[0].fixes[1];
    assert_eq!((fix.kind, fix.title.as_str()), (FixKind::Action, "Suppress simplifyrange for statement"));
    assert_eq!(fix.args["action"], ApplySuppression::ID);
    let answer = fixture.dispatch(ApplySuppression::ID, fix.args["params"].clone()).unwrap();
    assert_eq!(answer["changed"], true, "at {at}");
}

#[test]
fn a_suppressed_file_on_disk_is_read_again_when_it_changes() {
    let mut fixture = fixture("package main\n\nvar x = 1\n", vec![diagnostic(2, 2, "unused", "")]);
    let path = fixture.path("main.go");
    fixture.dispatch("file.document.close", json!({ "path": path })).unwrap();
    std::fs::write(&path, "package main\n\nvar x = 1 //nolint:unused\n").unwrap();
    refresh(&mut fixture.session, &path);
    let read = &fixture.session.state::<Suppressions>().unwrap().files[&path];
    assert_eq!((read.version, read.found.len()), (None, 1));
}

#[test]
fn inspect_code_leaves_out_what_a_file_suppresses_and_suppressing_an_item_takes_the_covered_ones() {
    let text = "package main\n\nfunc first() {\n\ta()\n\tb() //nolint:other\n\tc()\n}\n\nvar y = 1\n";
    let found = vec![
        diagnostic(3, 2, "unused", ""),
        diagnostic(4, 2, "other", ""),
        diagnostic(5, 2, "unused", ""),
        diagnostic(8, 2, "unused", ""),
    ];
    let mut fixture = fixture(text, found);
    let path = fixture.path("main.go");
    settle(&mut fixture, |problems| problems.len() == 3);
    fixture.dispatch("inspect.code.run", json!({ "scope": "files", "files": [path] })).unwrap();
    fixture.dispatch("inspect.code.status", json!({ "waitMs": 20000 })).unwrap();
    let results = fixture.dispatch("inspect.results.list", json!({})).unwrap();
    let lines: Vec<u64> =
        results["items"].as_array().unwrap().iter().map(|i| i["problem"]["line"].as_u64().unwrap()).collect();
    assert_eq!(lines, [4, 6, 9], "line 5 says //nolint:other");
    let (run, item) = (results["run"].clone(), results["items"][0]["id"].clone());
    let fixes = fixture.dispatch("inspect.results.fixes", json!({ "run": run, "item": item })).unwrap();
    let titles: Vec<&str> = fixes["fixes"].as_array().unwrap().iter().map(|f| f["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Suppress unused for statement", "Suppress unused for function", "Suppress unused for file"]);
    let params = fixes["fixes"][1]["action"]["params"].clone();
    assert_eq!(fixture.dispatch("inspect.results.suppress", params).unwrap(), 2, "both items of the function");
    let results = fixture.dispatch("inspect.results.list", json!({})).unwrap();
    assert_eq!(results["items"].as_array().unwrap().len(), 1);
    assert_eq!(results["items"][0]["problem"]["line"], 10, "the rest moved down with the new line");
}

fn inserted_into(text: &str, spans: &[Span], form: &SuppressionForm, separator: &str, line: usize, id: &str) -> String {
    let mut after = text.to_owned();
    if let Some(Some(insertion)) = inserted(text, spans, form, separator, line, id) {
        after.insert_str(insertion.at, &insertion.text);
    }
    after
}

fn nolint(line_end: bool) -> SuppressionForm {
    SuppressionForm { text: "//nolint:{id}".to_owned(), line_end, before: None }
}

#[test]
fn a_line_with_a_comment_gets_the_directive_above_it_with_its_indent() {
    let text = "a\n\tb // why\n";
    let span = Span { range: 5..11, kind: HighlightKind::Comment };
    assert_eq!(inserted_into(text, &[span], &nolint(true), ",", 1, "x"), "a\n\t//nolint:x\n\tb // why\n");
}

#[test]
fn a_line_that_ends_inside_a_raw_string_gets_the_directive_above_it() {
    let text = "f(`one\ntwo`)\n";
    let span = Span { range: 2..11, kind: HighlightKind::String };
    assert_eq!(inserted_into(text, &[span], &nolint(true), ",", 0, "x"), "//nolint:x\nf(`one\ntwo`)\n");
}

#[test]
fn a_directive_on_the_line_above_is_added_to() {
    assert_eq!(inserted_into("//nolint:a\nb\n", &[], &nolint(false), ",", 1, "x"), "//nolint:a,x\nb\n");
}

#[test]
fn windows_line_ends_stay_as_they_are() {
    assert_eq!(inserted_into("a\r\nb\r\n", &[], &nolint(true), ",", 1, "x"), "a\r\nb //nolint:x\r\n");
    assert_eq!(inserted_into("a\r\nb\r\n", &[], &nolint(false), ",", 1, "x"), "a\r\n//nolint:x\r\nb\r\n");
}

#[test]
fn a_rust_attribute_goes_above_and_takes_a_second_lint_in_its_parentheses() {
    let form = SuppressionForm { text: "#[allow({id})]".to_owned(), line_end: false, before: None };
    assert_eq!(inserted_into("fn a() {}\n", &[], &form, ", ", 0, "dead_code"), "#[allow(dead_code)]\nfn a() {}\n");
    assert_eq!(
        inserted_into("    #[allow(dead_code)]\n    fn a() {}\n", &[], &form, ", ", 1, "clippy::x"),
        "    #[allow(dead_code, clippy::x)]\n    fn a() {}\n"
    );
}

#[test]
fn a_rust_declaration_suppresses_only_what_the_compiler_and_clippy_report() {
    let declared: Vec<zen_lsp::servers::ServerDeclaration> =
        serde_json::from_str(include_str!("../../../../lang/rust/language-servers.json")).unwrap();
    let suppression = declared[0].suppression.as_ref().unwrap();
    assert_eq!(suppression.sources, ["rustc", "clippy"]);
    assert!(suppression.statement.is_none(), "an attribute cannot go in front of any line");
    let declared: Vec<zen_lsp::servers::ServerDeclaration> =
        serde_json::from_str(include_str!("../../../../lang/go/language-servers.json")).unwrap();
    assert_eq!(declared[0].suppression.as_ref().unwrap(), &go_forms());
}

#[test]
fn an_agent_reads_and_writes_no_suppression_of_a_hidden_file() {
    let mut fixture = fixture(MAIN, vec![analyzer()]);
    let at = offset_of(&mut fixture, 4);
    let path = fixture.path("main.go");
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["main.go".to_owned()]);
    let list = json!({ "path": path, "offset": at });
    let apply = json!({ "path": path, "offset": at, "inspection": "simplifyrange/default", "scope": "statement" });
    for (command, params) in [("inspect.suppress.list", list), ("inspect.suppress.apply", apply)] {
        let Fixture { registry, session, .. } = &mut fixture;
        let answer = zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params));
        assert!(format!("{answer:?}").contains(crate::hidden_files::REDACTED_CODE), "{command}: {answer:?}");
    }
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[]);
    assert_eq!(text_of_main(&fixture), MAIN);
}

#[test]
fn a_code_that_is_not_one_word_is_never_written_into_the_source() {
    assert!(plain_id("unused") && plain_id("S1000") && plain_id("clippy::needless_range_loop"));
    for code in ["a\nrm -rf /", "x */ evil(); /*", "a b", ""] {
        assert!(!plain_id(code), "{code:?}");
    }
}
