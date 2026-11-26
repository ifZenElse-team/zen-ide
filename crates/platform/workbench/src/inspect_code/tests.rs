use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_project::inspect::{
    DeclaredInspection, Finding, FindingSeverity, FixJob, FixKind, FixState, InspectContext, InspectionFix, Inspector,
};

use super::*;
use crate::language::testing;

struct Marks;

struct Finished(Option<FixState>);

impl FixJob for Finished {
    fn poll(&mut self) -> FixState {
        self.0.take().unwrap_or(FixState::Running { line: None })
    }
}

fn finding(at: usize, word: &str, code: &str, severity: FindingSeverity) -> Finding {
    Finding {
        start: at,
        end: at + word.len(),
        severity,
        message: format!("{word} here"),
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: vec![InspectionFix {
            id: format!("clear@{at}"),
            title: format!("Clear {word}"),
            kind: FixKind::Apply,
            batch: None,
            label: None,
            inline: false,
            args: json!({ "word": word }),
        }],
        source: "marks".to_owned(),
        code: code.to_owned(),
        deprecated: false,
        unused: false,
    }
}

impl Inspector for Marks {
    fn id(&self) -> &str {
        "marks"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".txt")
    }

    fn inspections(&self) -> Vec<DeclaredInspection> {
        vec![DeclaredInspection {
            id: "todo".to_owned(),
            title: "To do".to_owned(),
            group: "Marks".to_owned(),
            description: "Work left".to_owned(),
            cleanup: false,
            off: false,
        }]
    }

    fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
        let mut found: Vec<Finding> = context
            .text
            .match_indices("TODO")
            .map(|(at, word)| finding(at, word, "todo", FindingSeverity::Warning))
            .collect();
        found.extend(
            context.text.match_indices("BAD").map(|(at, word)| finding(at, word, "bad", FindingSeverity::Error)),
        );
        found
    }

    fn apply(&self, context: &InspectContext<'_>, fixes: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        let text = std::fs::read_to_string(context.path).map_err(|error| error.to_string())?;
        let word = fixes[0].args["word"].as_str().unwrap_or("TODO");
        std::fs::write(context.path, text.replace(word, "DONE")).map_err(|error| error.to_string())?;
        Ok(Box::new(Finished(Some(FixState::Done { changed: vec![context.path.to_owned()] }))))
    }
}

struct Tidy;

impl Inspector for Tidy {
    fn id(&self) -> &str {
        "tidy"
    }

    fn handles(&self, _: &str) -> bool {
        false
    }

    fn inspections(&self) -> Vec<DeclaredInspection> {
        let declared = |id: &str| DeclaredInspection {
            id: id.to_owned(),
            title: id.to_owned(),
            group: "Tidy".to_owned(),
            description: String::new(),
            cleanup: true,
            off: false,
        };
        vec![declared("bad"), declared("unused")]
    }

    fn inspect(&self, _: &InspectContext<'_>) -> Vec<Finding> {
        Vec::new()
    }

    fn apply(&self, _: &InspectContext<'_>, _: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        Err("no fixes".to_owned())
    }
}

fn tidy(session: &mut Session) {
    session.state_mut::<Inspections>().set_inspectors(vec![Arc::new(Marks), Arc::new(Tidy)], None);
}

struct Project {
    dir: tempfile::TempDir,
    registry: Registry,
    session: Session,
}

fn project(files: &[(&str, &str)]) -> Project {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.state_mut::<Inspections>().set_inspectors(vec![Arc::new(Marks)], None);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    let mut project = Project { dir, registry, session };
    project.wait_for_index(files.len());
    project
}

impl Project {
    fn wait_for_index(&mut self, files: usize) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let found = self.dispatch("search.scope.files", json!({}))["total"].as_u64().unwrap_or_default();
            if found as usize >= files || Instant::now() > deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn dispatch(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn inspect(&mut self, request: Value) -> Value {
        self.dispatch("inspect.code.run", request);
        self.settle();
        self.dispatch("inspect.results.list", json!({}))
    }

    fn settle(&mut self) {
        let status = self.dispatch("inspect.code.status", json!({ "waitMs": 20000 }));
        assert_eq!(status["running"], false, "{status}");
    }
}

fn relatives(results: &Value) -> Vec<String> {
    let items = results["items"].as_array().unwrap();
    let mut files: Vec<String> = items.iter().map(|item| item["relative"].as_str().unwrap().to_owned()).collect();
    files.dedup();
    files
}

#[test]
fn a_whole_project_run_inspects_files_that_are_not_open() {
    let mut project = project(&[("a.txt", "TODO\n"), ("sub/b.txt", "x BAD\n"), ("c.md", "TODO\n")]);
    let results = project.inspect(json!({}));
    assert_eq!(results["files"], 3);
    assert_eq!(relatives(&results), ["a.txt", "sub/b.txt"], "{results}");
    assert_eq!(results["items"][1]["inspection"], "marks/bad");
    assert_eq!(results["items"][1]["problem"]["line"], 1);
    assert_eq!(results["items"][1]["problem"]["column"], 3);
}

#[test]
fn the_scope_narrows_what_is_inspected() {
    let mut project = project(&[("a.txt", "TODO\n"), ("sub/b.txt", "TODO\n"), ("sub/b.test.txt", "TODO\n")]);
    let directory = project.inspect(json!({ "scope": "directory", "directory": "sub" }));
    assert_eq!(relatives(&directory), ["sub/b.test.txt", "sub/b.txt"]);
    assert_eq!(directory["title"], "Directory sub");
    let file = project.dir.path().join("a.txt");
    let files = project.inspect(json!({ "scope": "files", "files": [file] }));
    assert_eq!((relatives(&files), files["title"].as_str()), (vec!["a.txt".to_owned()], Some("a.txt")));
    let masked = project.inspect(json!({ "fileMask": "*.txt,!*.test.*" }));
    assert_eq!(relatives(&masked), ["a.txt", "sub/b.txt"]);
    let custom = project.inspect(json!({ "scope": "custom", "custom": "tests" }));
    assert_eq!(relatives(&custom), ["sub/b.test.txt"]);
}

#[test]
fn leaving_test_sources_out_drops_the_files_scopes_call_tests() {
    let mut project = project(&[("a_test.go", "TODO\n"), ("a.txt", "TODO\n")]);
    let results = project.inspect(json!({ "includeTests": false }));
    assert_eq!(results["files"], 1);
}

#[test]
fn one_inspection_by_name_keeps_only_its_problems() {
    let mut project = project(&[("a.txt", "TODO BAD\n")]);
    let results = project.inspect(json!({ "inspection": "bad" }));
    assert_eq!(results["items"].as_array().unwrap().len(), 1, "{results}");
    assert_eq!(results["items"][0]["problem"]["code"], "bad");
    let listed = project.dispatch("inspect.inspections.list", json!({}));
    let ids: Vec<&str> = listed.as_array().unwrap().iter().map(|info| info["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["marks/bad", "todo"], "the seen one and the declared one");
}

#[test]
fn the_tree_goes_severity_inspection_file_and_groups_by_directory() {
    let mut project = project(&[("a.txt", "TODO BAD\n"), ("sub/b.txt", "TODO\n")]);
    project.inspect(json!({}));
    let tree = project.dispatch("inspect.results.tree", json!({}));
    assert_eq!(tree["count"], 3);
    assert_eq!(tree["severities"][0]["severity"], "error");
    assert_eq!(tree["severities"][1]["severity"], "warning");
    let todo = &tree["severities"][1]["inspections"][0];
    assert_eq!(
        (&todo["name"], &todo["description"], &todo["count"]),
        (&json!("To do"), &json!("Work left"), &json!(2))
    );
    assert_eq!(todo["folders"].as_array().unwrap().len(), 1);
    assert_eq!(todo["folders"][0]["files"].as_array().unwrap().len(), 2);
    assert!(todo["folders"][0].get("directory").is_none());
    let grouped = project.dispatch("inspect.results.tree", json!({ "groupByDirectory": true }));
    let folders = grouped["severities"][1]["inspections"][0]["folders"].as_array().unwrap().clone();
    assert_eq!((&folders[0]["directory"], &folders[1]["directory"]), (&json!(""), &json!("sub")));
}

#[test]
fn excluded_items_leave_the_tree_and_exports_until_included_again() {
    let mut project = project(&[("a.txt", "TODO BAD\n")]);
    let results = project.inspect(json!({}));
    let run = results["run"].clone();
    let first = results["items"][0]["id"].clone();
    project.dispatch("inspect.results.exclude", json!({ "run": run, "items": [first] }));
    assert_eq!(project.dispatch("inspect.results.tree", json!({}))["count"], 1);
    assert_eq!(project.dispatch("inspect.results.tree", json!({ "showExcluded": true }))["count"], 2);
    let file = project.dir.path().join("out.xml");
    assert_eq!(project.dispatch("inspect.results.export", json!({ "format": "xml", "path": file })), 1);
    project.dispatch("inspect.results.exclude", json!({ "run": run, "items": [first], "excluded": false }));
    assert_eq!(project.dispatch("inspect.results.tree", json!({}))["count"], 2);
}

#[test]
fn exports_escape_what_the_problems_say() {
    let mut project = project(&[("a.txt", "BAD\n")]);
    let results = project.inspect(json!({}));
    let run = results["run"].as_u64().unwrap();
    project.session.state_mut::<InspectCode>().run_mut(run).unwrap().items[0].problem.message =
        "a<b & \"c\"".to_owned();
    let xml_file = project.dir.path().join("out.xml");
    let html_file = project.dir.path().join("out.html");
    project.dispatch("inspect.results.export", json!({ "format": "xml", "path": xml_file }));
    project.dispatch("inspect.results.export", json!({ "format": "html", "path": html_file }));
    let xml = std::fs::read_to_string(xml_file).unwrap();
    assert!(xml.contains("<description>a&lt;b &amp; &quot;c&quot;</description>"), "{xml}");
    assert!(xml.contains("<file>a.txt</file>") && xml.contains("<line>1</line>"), "{xml}");
    let html = std::fs::read_to_string(html_file).unwrap();
    assert!(html.contains("<h1>Inspection Results: Whole project</h1>"), "{html}");
    assert!(html.contains("a.txt:1:1 a&lt;b &amp; &quot;c&quot;"), "{html}");
}

#[test]
fn a_cancelled_run_keeps_no_results() {
    let mut project = project(&[("a.txt", "TODO\n")]);
    let started = project.dispatch("inspect.code.run", json!({}));
    project.dispatch("tasks.background.cancel", json!({ "id": started["task"] }));
    project.settle();
    assert_eq!(project.dispatch("inspect.code.status", json!({}))["finished"], json!([]));
    let tasks = project.dispatch("tasks.background.list", json!({}));
    assert!(tasks.to_string().contains("Cancelled"), "{tasks}");
}

#[test]
fn a_rerun_replaces_its_results_and_sees_the_files_as_they_are_now() {
    let mut project = project(&[("a.txt", "TODO\n")]);
    let run = project.inspect(json!({}))["run"].clone();
    std::fs::write(project.dir.path().join("a.txt"), "fine\n").unwrap();
    project.dispatch("inspect.results.rerun", json!({ "run": run }));
    project.settle();
    let status = project.dispatch("inspect.code.status", json!({}));
    assert_eq!(status["finished"].as_array().unwrap().len(), 1);
    assert_eq!(project.dispatch("inspect.results.list", json!({}))["items"], json!([]));
}

#[test]
fn fix_all_in_file_applies_inspector_fixes_and_clears_the_items() {
    let mut project = project(&[("a.txt", "TODO TODO\n")]);
    let results = project.inspect(json!({}));
    let ids: Vec<Value> = results["items"].as_array().unwrap().iter().map(|item| item["id"].clone()).collect();
    assert_eq!(ids.len(), 2);
    let fixes = project.dispatch("inspect.results.fixes", json!({ "run": results["run"], "item": ids[0] }));
    assert_eq!(fixes["fixes"][0]["title"], "Clear TODO");
    let started = project.dispatch("inspect.results.fix", json!({ "run": results["run"], "items": ids }));
    assert_eq!(started["jobs"].as_array().unwrap().len(), 1, "one job per file: {started}");
    project.settle();
    assert_eq!(std::fs::read_to_string(project.dir.path().join("a.txt")).unwrap(), "DONE DONE\n");
    assert_eq!(project.dispatch("inspect.results.list", json!({}))["items"], json!([]));
}

fn diagnostic(line: u32) -> Value {
    json!({
        "range": { "start": { "line": line, "character": 1 }, "end": { "line": line, "character": 2 } },
        "severity": 2, "code": "unused", "source": "lint", "message": "x is not used"
    })
}

#[test]
fn language_servers_check_files_that_are_not_open() {
    let mut fixture = testing::fixture(&[("main.go", "package main\n\tx := 1\n"), ("two.go", "package main\n")]);
    fixture.server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", |params| {
        let uri = params["textDocument"]["uri"].clone();
        let diagnostics = if uri.as_str().unwrap().ends_with("main.go") { vec![diagnostic(1)] } else { Vec::new() };
        json!({ "uri": uri, "diagnostics": diagnostics })
    });
    let file = fixture.path("main.go");
    fixture.dispatch("inspect.code.run", json!({ "scope": "files", "files": [file, fixture.path("two.go")] })).unwrap();
    let status = fixture.dispatch("inspect.code.status", json!({ "waitMs": 20000 })).unwrap();
    assert_eq!(status["running"], false);
    let results = fixture.dispatch("inspect.results.list", json!({})).unwrap();
    assert_eq!(results["items"].as_array().unwrap().len(), 1, "{results}");
    assert_eq!(results["items"][0]["inspection"], "lint/unused");
    assert_eq!(results["items"][0]["problem"]["line"], 2);
    assert_eq!(results["unchecked"], json!([]));
    fixture.dispatch("language.servers.sync", json!({})).unwrap();
    let open = fixture.with(|languages, _| languages.synced_text(&file).is_some());
    assert!(!open, "the file is closed in the server again");
}

#[test]
fn a_language_servers_fix_is_made_from_the_results_in_a_file_that_is_not_open() {
    let mut fixture = testing::fixture(&[("main.go", "package main\n\tx := 1\n")]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "codeActionProvider": { "codeActionKinds": ["quickfix"] }
    }));
    fixture.server.publish_on(
        "textDocument/didOpen",
        "textDocument/publishDiagnostics",
        |params| json!({ "uri": params["textDocument"]["uri"], "diagnostics": [diagnostic(1)] }),
    );
    fixture.server.on_with("textDocument/codeAction", |params| {
        let uri = params["textDocument"]["uri"].as_str().unwrap().to_owned();
        json!([{
            "title": "Remove x", "kind": "quickfix", "isPreferred": true,
            "edit": { "changes": { uri: [{
                "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 2, "character": 0 } },
                "newText": ""
            }] } }
        }])
    });
    let file = fixture.path("main.go");
    fixture.dispatch("inspect.code.run", json!({ "scope": "files", "files": [file] })).unwrap();
    fixture.dispatch("inspect.code.status", json!({ "waitMs": 20000 })).unwrap();
    let results = fixture.dispatch("inspect.results.list", json!({})).unwrap();
    let (run, item) = (results["run"].clone(), results["items"][0]["id"].clone());
    let mut offered = json!({});
    for _ in 0..100 {
        offered =
            fixture.dispatch("inspect.results.fixes", json!({ "run": run, "item": item, "waitMs": 100 })).unwrap();
        if offered["pending"] == false {
            break;
        }
    }
    assert_eq!(offered["fixes"][0]["title"], "Remove x", "{offered}");
    let started = fixture.dispatch("inspect.results.fix", json!({ "run": run, "items": [item] })).unwrap();
    assert_eq!(started["serverFixes"], 1);
    let status = fixture.dispatch("inspect.code.status", json!({ "waitMs": 20000 })).unwrap();
    assert_eq!(status["running"], false, "{status}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "package main\n");
    assert_eq!(fixture.dispatch("inspect.results.list", json!({})).unwrap()["items"], json!([]));
}

fn severities(results: &Value) -> Vec<String> {
    let items = results["items"].as_array().unwrap();
    items.iter().map(|item| item["problem"]["severity"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn a_profile_decides_what_inspect_code_reports_and_how_severe_it_is() {
    let mut project = project(&[("a.txt", "TODO BAD\n")]);
    let set = json!({ "inspections": { "todo": { "enabled": false }, "bad": { "severity": "weakWarning" } } });
    project.dispatch("inspect.profile.set", set);
    let results = project.inspect(json!({}));
    assert_eq!(severities(&results), ["weakWarning"], "{results}");
    assert_eq!(results["items"][0]["inspection"], "marks/bad");
}

#[test]
fn run_inspection_by_name_runs_what_the_profile_turned_off() {
    let mut project = project(&[("a.txt", "TODO\n")]);
    project.dispatch("inspect.profile.set", json!({ "inspections": { "todo": { "enabled": false } } }));
    assert_eq!(project.inspect(json!({ "inspection": "todo" }))["items"].as_array().unwrap().len(), 1);
}

#[test]
fn a_run_can_ask_for_a_profile_other_than_the_one_that_applies() {
    let mut project = project(&[("a.txt", "TODO\n")]);
    let strict = json!({ "name": "Strict", "level": "project" });
    project.dispatch("inspect.profile.copy", json!({ "to": strict }));
    let set = json!({ "profile": strict, "inspections": { "todo": { "severity": "error" } } });
    project.dispatch("inspect.profile.set", set);
    assert_eq!(severities(&project.inspect(json!({}))), ["warning"]);
    assert_eq!(severities(&project.inspect(json!({ "profile": strict }))), ["error"]);
    let results =
        project.registry.dispatch(&mut project.session, "inspect.code.run", json!({ "profile": { "name": "No" } }));
    assert!(results.is_err());
}

#[test]
fn code_cleanup_fixes_only_what_is_flagged_and_opens_no_results() {
    let mut project = project(&[("a.txt", "TODO BAD\n")]);
    tidy(&mut project.session);
    project.dispatch("inspect.cleanup.run", json!({}));
    project.settle();
    assert_eq!(std::fs::read_to_string(project.dir.path().join("a.txt")).unwrap(), "TODO DONE\n");
    let status = project.dispatch("inspect.code.status", json!({}));
    assert_eq!(status["finished"], json!([]));
    assert_eq!(status["cleanup"], json!({ "fixed": 1, "skipped": 0, "files": 1 }), "{status}");
}

#[test]
fn code_cleanup_leaves_what_the_profile_turned_off() {
    let mut project = project(&[("a.txt", "BAD\n")]);
    tidy(&mut project.session);
    project.dispatch("inspect.profile.set", json!({ "inspections": { "bad": { "enabled": false } } }));
    project.dispatch("inspect.cleanup.run", json!({}));
    project.settle();
    assert_eq!(std::fs::read_to_string(project.dir.path().join("a.txt")).unwrap(), "BAD\n");
    assert_eq!(project.dispatch("inspect.code.status", json!({}))["cleanup"]["fixed"], 0);
}

fn cleanup_fixture() -> (testing::Fixture, PathBuf) {
    let mut fixture = testing::fixture(&[("main.go", "package main\n\tx := 1\n\ty := 2\n")]);
    tidy(&mut fixture.session);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "codeActionProvider": { "codeActionKinds": ["quickfix"] }
    }));
    let unused = |text: &str| {
        let lines: Vec<Value> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.starts_with('\t'))
            .map(|(line, _)| diagnostic(line as u32))
            .collect();
        lines
    };
    fixture.server.publish_on("textDocument/didOpen", "textDocument/publishDiagnostics", move |params| {
        json!({ "uri": params["textDocument"]["uri"], "diagnostics": unused(params["textDocument"]["text"].as_str().unwrap()) })
    });
    fixture.server.publish_on("textDocument/didChange", "textDocument/publishDiagnostics", |params| {
        let left = if params["textDocument"]["version"].as_i64().unwrap_or_default() >= 3 { 0 } else { 1 };
        json!({ "uri": params["textDocument"]["uri"], "diagnostics": (0..left).map(|_| diagnostic(1)).collect::<Vec<_>>() })
    });
    fixture.server.on_with("textDocument/codeAction", |params| {
        let uri = params["textDocument"]["uri"].as_str().unwrap().to_owned();
        let line = params["range"]["start"]["line"].clone();
        json!([{
            "title": "Remove", "kind": "quickfix", "isPreferred": true,
            "edit": { "changes": { uri: [{
                "range": { "start": { "line": line, "character": 0 }, "end": { "line": line.as_u64().unwrap() + 1, "character": 0 } },
                "newText": ""
            }] } }
        }])
    });
    let file = fixture.path("main.go");
    (fixture, file)
}

#[test]
fn code_cleanup_makes_one_undoable_change_of_a_files_fixes() {
    let (mut fixture, file) = cleanup_fixture();
    fixture.open("main.go");
    fixture.dispatch("inspect.cleanup.run", json!({ "scope": "files", "files": [file] })).unwrap();
    let status = fixture.dispatch("inspect.code.status", json!({ "waitMs": 30000 })).unwrap();
    assert_eq!(status["cleanup"], json!({ "fixed": 2, "skipped": 0, "files": 1 }), "{status}");
    let text = |fixture: &mut testing::Fixture| {
        fixture.with(|_, workbench| workbench.documents().iter().find(|d| d.path() == file).unwrap().text().to_owned())
    };
    assert_eq!(text(&mut fixture), "package main\n");
    fixture.dispatch("editor.edit.undo", json!({})).unwrap();
    assert_eq!(text(&mut fixture), "package main\n\tx := 1\n\ty := 2\n");
}

#[test]
fn code_cleanup_fixes_files_that_are_not_open() {
    let (mut fixture, file) = cleanup_fixture();
    fixture.dispatch("inspect.cleanup.run", json!({ "scope": "files", "files": [file] })).unwrap();
    let status = fixture.dispatch("inspect.code.status", json!({ "waitMs": 30000 })).unwrap();
    assert_eq!(status["cleanup"]["fixed"], 2, "{status}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "package main\n");
    assert_eq!(fixture.dispatch("inspect.cleanup.undo", json!({ "path": file })).unwrap(), 1);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "package main\n\tx := 1\n\ty := 2\n");
    assert!(fixture.dispatch("inspect.cleanup.undo", json!({})).is_err(), "it is gone");
}

struct Writes(PathBuf);

impl FixJob for Writes {
    fn poll(&mut self) -> FixState {
        FixState::Done { changed: vec![self.0.clone()] }
    }

    fn writes(&self) -> Vec<zen_project::inspect::FixWrite> {
        vec![zen_project::inspect::FixWrite { path: self.0.clone(), text: "written\n".to_owned(), read: None }]
    }
}

struct WritesThrough;

impl Inspector for WritesThrough {
    fn id(&self) -> &str {
        "writes"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".txt")
    }

    fn inspect(&self, _: &InspectContext<'_>) -> Vec<Finding> {
        Vec::new()
    }

    fn apply(&self, context: &InspectContext<'_>, _: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        Ok(Box::new(Writes(context.path.to_owned())))
    }
}

fn a_fix() -> Vec<InspectionFix> {
    vec![InspectionFix {
        id: "write".to_owned(),
        title: "Write".to_owned(),
        kind: FixKind::Apply,
        batch: None,
        label: None,
        inline: false,
        args: json!({}),
    }]
}

#[test]
fn what_a_fix_writes_goes_through_the_guarded_write() {
    let mut project = project(&[("a.txt", "before\n"), ("secret.txt", "before\n")]);
    project.session.state_mut::<Inspections>().set_inspectors(vec![Arc::new(WritesThrough)], None);
    let (open, secret) = (project.dir.path().join("a.txt"), project.dir.path().join("secret.txt"));
    project.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret.txt".to_owned()]);
    let refused = zen_core::macros::as_agent(&mut project.session, |session| {
        crate::inspections::start_fixes(session, secret.clone(), a_fix())
    });
    assert_eq!(refused.err().and_then(|error| error.code).as_deref(), Some(crate::hidden_files::REDACTED_CODE));
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "before\n", "an agent's fix leaves a hidden file alone");
    crate::inspections::start_fixes(&mut project.session, open.clone(), a_fix()).unwrap();
    assert_eq!(std::fs::read_to_string(&open).unwrap(), "written\n", "the planned write reaches the file");
}

struct Program(Arc<AtomicBool>);

impl Inspector for Program {
    fn id(&self) -> &str {
        "program"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".txt")
    }

    fn runs_programs(&self) -> bool {
        true
    }

    fn inspect(&self, _: &InspectContext<'_>) -> Vec<Finding> {
        self.0.store(true, Ordering::Relaxed);
        Vec::new()
    }

    fn apply(&self, _: &InspectContext<'_>, _: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        self.0.store(true, Ordering::Relaxed);
        Ok(Box::new(Finished(Some(FixState::Done { changed: Vec::new() }))))
    }
}

#[test]
fn inspect_code_and_its_fixes_run_no_program_in_safe_mode() {
    let mut project = project(&[("a.txt", "TODO\n")]);
    project.session.set_state(crate::build_tools::BuildEnv::default());
    let ran = Arc::new(AtomicBool::new(false));
    project.session.state_mut::<Inspections>().set_inspectors(vec![Arc::new(Program(Arc::clone(&ran)))], None);
    for command in ["inspect.code.run", "inspect.cleanup.run"] {
        project.dispatch(command, json!({}));
        project.settle();
        assert!(!ran.load(Ordering::Relaxed), "{command} ran the project's program");
    }
    let path = project.dir.path().join("a.txt");
    let refused = crate::inspections::start_fixes(&mut project.session, path.clone(), a_fix());
    assert_eq!(refused.err().and_then(|error| error.code).as_deref(), Some(crate::trust::NOT_TRUSTED));
    assert!(!ran.load(Ordering::Relaxed), "a fix ran it");
    let root = project.dir.path().to_owned();
    crate::trust::trust_session_in_tests(&mut project.session, &root);
    project.dispatch("inspect.code.run", json!({}));
    project.settle();
    assert!(ran.load(Ordering::Relaxed), "trusted: it runs");
}

#[test]
fn an_agent_sees_nothing_of_a_hidden_file_in_results_a_person_ran() {
    let mut project = project(&[("a.txt", "TODO\n"), ("secret.txt", "TODO\n")]);
    let results = project.inspect(json!({}));
    let run = results["run"].as_u64().unwrap();
    let secret =
        results["items"].as_array().unwrap().iter().find(|item| item["relative"] == "secret.txt").unwrap()["id"]
            .as_u64()
            .unwrap();
    project.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret.txt".to_owned()]);
    let out = project.dir.path().join("out.xml");
    let Project { registry, session, .. } = &mut project;
    let mut as_agent = |command: &str, params: Value| {
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params))
    };
    let listed = as_agent("inspect.results.list", json!({ "run": run })).unwrap().to_string();
    assert!(!listed.contains("secret.txt"), "{listed}");
    let tree = as_agent("inspect.results.tree", json!({ "run": run })).unwrap().to_string();
    assert!(!tree.contains("secret.txt"), "{tree}");
    for (command, params) in [
        ("inspect.results.fixes", json!({ "run": run, "item": secret })),
        ("inspect.results.fix", json!({ "run": run, "items": [secret] })),
        ("inspect.results.suppress", json!({ "run": run, "item": secret, "scope": "statement" })),
    ] {
        let answer = as_agent(command, params);
        assert!(answer.is_err(), "{command}: {answer:?}");
    }
    as_agent("inspect.results.export", json!({ "run": run, "format": "xml", "path": out })).unwrap();
    let exported = std::fs::read_to_string(&out).unwrap();
    assert!(!exported.contains("secret.txt") && exported.contains("a.txt"), "{exported}");
    assert_eq!(std::fs::read_to_string(project.dir.path().join("secret.txt")).unwrap(), "TODO\n");
}

#[test]
fn an_agent_exports_inside_the_project_and_never_over_a_hidden_file() {
    let mut project = project(&[("a.txt", "TODO\n"), ("secret.txt", "keep\n")]);
    project.inspect(json!({}));
    project.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["secret.txt".to_owned()]);
    let elsewhere = tempfile::tempdir().unwrap();
    let outside = elsewhere.path().join("out.xml");
    let secret = project.dir.path().join("secret.txt");
    let Project { registry, session, .. } = &mut project;
    for path in [&secret, &outside] {
        let answer = zen_core::macros::as_agent(session, |session| {
            registry.dispatch(session, "inspect.results.export", json!({ "format": "xml", "path": path }))
        });
        assert!(answer.is_err(), "{path:?}: {answer:?}");
    }
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "keep\n");
    assert!(!outside.exists());
    project.dispatch("inspect.results.export", json!({ "format": "xml", "path": outside }));
    assert!(outside.exists(), "the person chooses any place");
}
