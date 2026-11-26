use std::time::Instant;

use serde_json::{Value, json};
use zen_core::DispatchError;
use zen_dap::fake::{FakeAdapter, FakeVariable};
use zen_run::fake::{FAKE_ADAPTER, FAKE_ADDRESS_FLAG, FakeProvider};
use zen_run::{
    AdapterLine, CommandLine, Configuration, DebugPlan, KindSpec, OutputParser, PlainParser, RunContext, RunEnv,
    RunProvider, RunTarget,
};

use super::*;

trait Mentions {
    fn mentions(&self, id: &str) -> bool;
}

impl<T: AsRef<str> + ?Sized> Mentions for T {
    fn mentions(&self, id: &str) -> bool {
        let text = self.as_ref();
        let translated = t!(id);
        let head = translated.split(['{', '\u{2068}']).next().unwrap_or_default();
        text.contains(id) || (!head.is_empty() && head != id && text.contains(head))
    }
}

const WAIT: u64 = 5000;

const IDS: [&str; 51] = [
    "debug.session.start",
    "debug.session.stop",
    "debug.session.list",
    "debug.session.get",
    "debug.execution.resume",
    "debug.execution.pause",
    "debug.execution.stepOver",
    "debug.execution.stepInto",
    "debug.execution.stepOut",
    "debug.execution.runToCursor",
    "debug.frames.list",
    "debug.frame.select",
    "debug.variables.list",
    "debug.expression.evaluate",
    "debug.expression.history",
    "debug.executionPoint.show",
    "debug.breakpoint.toggle",
    "debug.breakpoint.set",
    "debug.breakpoint.remove",
    "debug.breakpoint.list",
    "debug.watch.add",
    "debug.watch.remove",
    "debug.watch.list",
    "debug.exceptionBreakpoint.set",
    "debug.exceptionBreakpoint.list",
    "debug.classes.reload",
    "debug.breakpoint.toggleEnabled",
    "debug.breakpoint.toggleTemporary",
    "debug.breakpoint.move",
    "debug.breakpoint.copy",
    "debug.breakpoint.removeOthers",
    "debug.breakpoint.disableOthers",
    "debug.breakpoints.mute",
    "debug.functionBreakpoint.add",
    "debug.functionBreakpoint.remove",
    "debug.exceptionBreakpoint.add",
    "debug.exceptionBreakpoint.remove",
    "debug.dataBreakpoint.add",
    "debug.dataBreakpoint.set",
    "debug.dataBreakpoint.remove",
    "debug.execution.forceRunToCursor",
    "debug.execution.forceStepOver",
    "debug.execution.forceStepInto",
    "debug.variable.set",
    "debug.inlineValues.get",
    "debug.execution.stepInTargets",
    "debug.execution.smartStepInto",
    "debug.frame.reset",
    "debug.variables.raw",
    "debug.stream.trace",
    "debug.coroutines.list",
];

struct Setup {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    adapter: FakeAdapter,
    main: PathBuf,
}

impl Setup {
    fn new(providers: Vec<Arc<dyn RunProvider>>) -> Self {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
        let adapter = FakeAdapter::new();
        session.state_mut::<Debugger>().set_launcher(Arc::new(FakeLauncher::new(adapter.clone())));
        let main = dir.path().join("main.go");
        let text: String = (1..=12).map(|line| format!("line {line}\n")).collect();
        std::fs::write(&main, text).unwrap();
        Self { registry, session, _dir: dir, adapter, main }
    }

    fn fake() -> Self {
        Self::new(vec![Arc::new(FakeProvider::new())])
    }

    fn dispatch(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> CommandError {
        match self.registry.dispatch(&mut self.session, id, params) {
            Err(DispatchError::Failed { error, .. }) => error,
            other => panic!("{id} did not fail: {other:?}"),
        }
    }

    fn program(&self, lines: &[i64]) {
        let lines: Vec<(&Path, i64)> = lines.iter().map(|line| (self.main.as_path(), *line)).collect();
        self.adapter.set_program(&lines);
    }

    fn start(&mut self) -> (u64, u64) {
        let configuration = serde_json::to_value(FakeProvider::configuration("app")).unwrap();
        let started = self.dispatch("debug.session.start", json!({ "configuration": configuration }));
        (started["sessionId"].as_u64().unwrap(), started["runId"].as_u64().unwrap())
    }

    fn get(&mut self) -> Value {
        self.dispatch("debug.session.get", json!({ "waitMs": WAIT }))
    }

    fn breakpoint(&mut self, line: u32) {
        let main = self.main.clone();
        self.dispatch("debug.breakpoint.set", json!({ "path": main, "line": line }));
    }

    fn output(&mut self, run_id: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let output = self.dispatch("run.output.get", json!({ "runId": run_id, "waitMs": 200 }));
            if output["state"] != "running" {
                return output;
            }
            assert!(Instant::now() < deadline, "the run does not end: {output}");
        }
    }

    fn set(&mut self, line: u32, change: Value) {
        let mut params = change;
        params["path"] = json!(self.main);
        params["line"] = json!(line);
        self.dispatch("debug.breakpoint.set", params);
    }

    fn sent(&self) -> Vec<Value> {
        let sent = self.adapter.requests("setBreakpoints");
        sent.last().map(|last| last["breakpoints"].as_array().cloned().unwrap_or_default()).unwrap_or_default()
    }

    fn sent_lines(&self) -> Vec<i64> {
        let sent = self.adapter.requests("setBreakpoints");
        let last = sent.last().cloned().unwrap_or(Value::Null);
        last["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|breakpoint| breakpoint["line"].as_i64())
            .collect()
    }
}

fn texts(output: &Value) -> Vec<(String, String)> {
    output["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| (line["stream"].as_str().unwrap().to_owned(), line["text"].as_str().unwrap().to_owned()))
        .collect()
}

fn line(stream: &str, text: &str) -> (String, String) {
    (stream.to_owned(), text.to_owned())
}

#[test]
fn every_debug_command_is_registered_with_three_segments() {
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    for id in IDS {
        assert_eq!(id.split('.').count(), 3, "{id}");
        assert!(registry.commands().any(|info| info.id.as_str() == id), "{id} is not registered");
    }
}

fn answered(setup: &mut Setup, id: &str, mut params: Value) -> Value {
    let answer = setup.dispatch(id, params.clone());
    if answer["pending"] != true {
        return answer;
    }
    params["waitMs"] = json!(WAIT);
    let answer = setup.dispatch(id, params);
    assert_ne!(answer["pending"], true, "{id} stays pending");
    answer
}

#[test]
fn evaluation_without_waiting_answers_pending_until_the_debugger_answers_and_keeps_recent_expressions() {
    let mut setup = Setup::fake();
    setup.program(&[3, 4, 5]);
    setup.adapter.set_variables(vec![
        FakeVariable::new("x", "1", "int"),
        FakeVariable::new("s", "{a: 2}", "main.S").with_children(vec![FakeVariable::new("a", "2", "int")]),
    ]);
    setup.breakpoint(2);
    setup.start();
    assert_eq!(setup.get()["state"], "stopped");
    let evaluated = answered(&mut setup, "debug.expression.evaluate", json!({ "expression": "s", "waitMs": 0 }));
    assert_eq!(evaluated["result"], "{a: 2}");
    let reference = evaluated["reference"].as_i64().unwrap();
    let children = answered(&mut setup, "debug.variables.list", json!({ "reference": reference, "waitMs": 0 }));
    assert_eq!(
        children,
        json!({ "variables": [{ "name": "a", "value": "2", "type": "int", "reference": 0, "evaluateName": "s.a" }] })
    );
    let scopes = answered(&mut setup, "debug.variables.list", json!({ "waitMs": 0 }));
    assert_eq!(scopes["variables"][0]["name"], "Locals");
    assert_eq!(setup.adapter.requests("evaluate").len(), 1, "asking again reads the same request");

    setup.dispatch("debug.expression.evaluate", json!({ "expression": "x", "context": "watch" }));
    let history = setup.dispatch("debug.expression.history", json!({}));
    assert_eq!(history, json!({ "expressions": ["s"] }), "watch evaluations are not recent expressions");
    setup.session.settings_mut().set(RECENT_EXPRESSIONS, 2).unwrap();
    for expression in ["x", "nope", "s"] {
        let _ = setup.registry.dispatch(
            &mut setup.session,
            "debug.expression.evaluate",
            json!({ "expression": expression }),
        );
    }
    let history = setup.dispatch("debug.expression.history", json!({}));
    assert_eq!(
        history,
        json!({ "expressions": ["s", "nope"] }),
        "the newest first, failed ones too, at most the limit"
    );

    let before = setup.adapter.requests("evaluate").len();
    setup.adapter.set_silent("evaluate");
    let params = json!({ "expression": "x", "waitMs": 0 });
    assert_eq!(
        setup.dispatch("debug.expression.evaluate", params.clone()),
        json!({ "result": "", "reference": 0, "pending": true })
    );
    assert_eq!(setup.dispatch("debug.expression.evaluate", params)["pending"], true);
    let asked = setup.adapter.wait_for("evaluate", before + 1, Duration::from_millis(WAIT));
    assert_eq!(asked.len(), before + 1, "one request for the pending expression");
    assert_eq!(setup.dispatch("debug.expression.history", json!({}))["expressions"], json!(["s", "nope"]));
}

#[test]
fn hover_evaluation_needs_a_debugger_that_evaluates_for_hovers() {
    let mut setup = Setup::fake();
    setup.program(&[3, 4]);
    setup.adapter.set_variables(vec![FakeVariable::new("x", "1", "int")]);
    setup.adapter.set_capabilities(json!({ "supportsEvaluateForHovers": false }));
    setup.breakpoint(2);
    setup.start();
    assert_eq!(setup.get()["state"], "stopped");
    let error = setup.fail("debug.expression.evaluate", json!({ "expression": "x", "context": "hover" }));
    assert!(error.message.mentions("debug-no-hover"), "{}", error.message);
    assert!(setup.adapter.requests("evaluate").is_empty());
    setup.dispatch("debug.session.stop", json!({}));

    let mut setup = Setup::fake();
    setup.program(&[3, 4]);
    setup.adapter.set_variables(vec![FakeVariable::new("x", "1", "int")]);
    setup.breakpoint(2);
    setup.start();
    assert_eq!(setup.get()["state"], "stopped");
    assert_eq!(
        setup.dispatch("debug.expression.evaluate", json!({ "expression": "x", "context": "hover" }))["result"],
        "1"
    );
    assert_eq!(setup.adapter.requests("evaluate")[0]["context"], "hover");
    assert_eq!(setup.dispatch("debug.expression.history", json!({}))["expressions"], json!([]));
}

#[test]
fn show_execution_point_selects_the_top_frame_and_opens_its_line() {
    let mut setup = Setup::fake();
    setup.program(&[3, 4]);
    setup.breakpoint(2);
    assert!(setup.fail("debug.executionPoint.show", json!({})).message.mentions("debug-no-session"));
    setup.start();
    assert_eq!(setup.get()["state"], "stopped");
    let shown = setup.dispatch("debug.executionPoint.show", json!({}));
    assert_eq!(shown, json!({ "path": setup.main, "line": 2, "frameId": 1000 }));
    let workbench = setup.session.state::<Workbench>().unwrap();
    let document = workbench.active_document().unwrap();
    assert_eq!(document.path(), setup.main);
    let head = document.selections().primary().head;
    assert_eq!(document.snapshot().offset_to_point(head).unwrap().line, 2);
}

#[test]
fn a_session_stops_at_a_breakpoint_shows_its_state_steps_and_ends_with_the_exit_code() {
    let mut setup = Setup::fake();
    setup.program(&[3, 4, 5, 8, 9]);
    setup.adapter.set_variables(vec![
        FakeVariable::new("x", "1", "int"),
        FakeVariable::new("s", "{a: 2, b: 3}", "main.S")
            .with_children(vec![FakeVariable::new("a", "2", "int"), FakeVariable::new("b", "3", "int")]),
    ]);
    setup.adapter.set_output(&[("stdout", "hel"), ("stdout", "lo\nwor"), ("console", "note\n"), ("stderr", "warn\n")]);
    setup.adapter.set_exit_code(3);
    setup.breakpoint(2);
    let (session_id, run_id) = setup.start();
    let state = setup.get();
    assert_eq!(state["state"], "stopped", "{state}");
    assert_eq!(state["reason"], "breakpoint");
    assert_eq!(state["threadId"], 1);
    assert_eq!(state["frame"], json!({ "path": setup.main, "line": 2, "name": "main.main" }));
    assert_eq!(state["threads"], json!([{ "id": 1, "name": "main" }]));
    let listed = setup.dispatch("debug.session.list", json!({}));
    assert_eq!(
        listed["sessions"],
        json!([{ "sessionId": session_id, "runId": run_id, "name": "app", "state": "stopped" }])
    );

    let frames = setup.dispatch("debug.frames.list", json!({}));
    assert_eq!(
        frames["frames"],
        json!([{ "id": 1000, "name": "main.main", "path": setup.main, "line": 2, "column": 0 }])
    );
    setup.dispatch("debug.frame.select", json!({ "frameId": 1000 }));
    assert!(setup.fail("debug.frame.select", json!({ "frameId": 7 })).message.mentions("debug-unknown-frame"));
    let scopes = setup.dispatch("debug.variables.list", json!({}));
    assert_eq!(scopes["variables"], json!([{ "name": "Locals", "value": "", "reference": 1 }]));
    let locals = setup.dispatch("debug.variables.list", json!({ "reference": 1 }));
    assert_eq!(
        locals["variables"][0],
        json!({ "name": "x", "value": "1", "type": "int", "reference": 0, "evaluateName": "x" })
    );
    let s = &locals["variables"][1];
    assert_eq!(s["named"], 2);
    let reference = s["reference"].as_i64().unwrap();
    assert!(reference > 0);
    let children = setup.dispatch("debug.variables.list", json!({ "reference": reference, "start": 1, "count": 1 }));
    assert_eq!(
        children["variables"],
        json!([{ "name": "b", "value": "3", "type": "int", "reference": 0, "evaluateName": "s.b" }])
    );
    assert_eq!(setup.adapter.requests("variables").len(), 2);
    setup.dispatch("debug.variables.list", json!({ "reference": 1 }));
    assert_eq!(setup.adapter.requests("variables").len(), 2, "children are cached while the program is stopped");

    let evaluated = setup.dispatch("debug.expression.evaluate", json!({ "expression": "s.a" }));
    assert_eq!(evaluated, json!({ "result": "2", "type": "int", "reference": 0 }));
    assert_eq!(setup.adapter.requests("evaluate")[0]["context"], "repl");
    assert!(
        setup
            .fail("debug.expression.evaluate", json!({ "expression": "nope" }))
            .message
            .mentions("debug-adapter-error")
    );
    setup.dispatch("debug.watch.add", json!({ "expression": "x" }));
    setup.dispatch("debug.watch.add", json!({ "expression": "nope" }));
    setup.dispatch("debug.watch.add", json!({ "expression": "x" }));
    let watches = setup.dispatch("debug.watch.list", json!({}));
    assert_eq!(watches["watches"][0], json!({ "expression": "x", "value": "1", "type": "int", "reference": 0 }));
    assert!(watches["watches"][1]["error"].as_str().unwrap().mentions("debug-adapter-error"));
    assert_eq!(setup.adapter.requests("evaluate").last().unwrap()["context"], "watch");
    setup.dispatch("debug.watch.remove", json!({ "expression": "nope" }));
    assert!(setup.fail("debug.watch.remove", json!({ "expression": "nope" })).message.mentions("debug-unknown-watch"));

    setup.dispatch("debug.execution.stepOver", json!({}));
    let state = setup.get();
    assert_eq!((state["reason"].clone(), state["frame"]["line"].clone()), (json!("step"), json!(3)));
    setup.dispatch("debug.variables.list", json!({ "reference": 1 }));
    assert_eq!(setup.adapter.requests("variables").len(), 3, "a step clears the cache");
    setup.dispatch("debug.execution.stepInto", json!({}));
    assert_eq!(setup.get()["frame"]["line"], 4);
    setup.dispatch("debug.execution.stepOut", json!({ "sessionId": session_id, "threadId": 1 }));
    assert_eq!(setup.get()["frame"]["line"], 7);
    setup.dispatch("debug.execution.pause", json!({}));
    let deadline = Instant::now() + Duration::from_secs(60);
    while setup.dispatch("debug.session.get", json!({}))["reason"] != "pause" {
        assert!(Instant::now() < deadline, "the pause did not arrive");
        std::thread::sleep(Duration::from_millis(10));
    }
    setup.dispatch("debug.execution.resume", json!({}));
    let state = setup.get();
    assert_eq!((state["state"].clone(), state["reason"].clone()), (json!("ended"), json!("exited")));
    assert_eq!(setup.fail("debug.execution.resume", json!({})).message, t!("debug-session-ended", id = session_id));
    let watches = setup.dispatch("debug.watch.list", json!({}));
    assert_eq!(watches["watches"], json!([{ "expression": "x" }]));

    let output = setup.output(run_id);
    assert_eq!(output["state"], "finished");
    assert_eq!(output["exitCode"], 3);
    let lines = texts(&output);
    assert_eq!(lines[0].0, "system");
    assert!(lines[0].1.starts_with(&format!("{FAKE_ADAPTER} app {FAKE_ADDRESS_FLAG}=127.0.0.1:")), "{lines:?}");
    assert!(lines.contains(&line("stdout", "hello")), "{lines:?}");
    assert!(lines.contains(&line("system", "note")), "{lines:?}");
    assert!(lines.contains(&line("stderr", "warn")), "{lines:?}");
    assert!(lines.contains(&line("stdout", "wor")), "the last part is kept: {lines:?}");
    assert_eq!(lines.last().unwrap(), &line("system", &t!("workbench-run-process-finished", code = 3)));
    let runs = setup.dispatch("run.process.list", json!({}));
    assert_eq!(runs["runs"][0]["debug"], true);
    assert_eq!(runs["runs"][0]["exitCode"], 3);
    let commands = setup.adapter.commands();
    for command in ["next", "stepIn", "stepOut", "pause", "continue", "disconnect"] {
        assert!(commands.iter().any(|sent| sent == command), "{command} was not sent: {commands:?}");
    }
    assert_eq!(setup.adapter.requests("launch")[0]["program"], "app");
}

#[test]
fn breakpoints_move_with_the_edits_of_an_open_document() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    setup.dispatch("file.document.open", json!({ "path": main }));
    let toggled = setup.dispatch("debug.breakpoint.toggle", json!({ "line": 2 }));
    assert_eq!(toggled["breakpoint"], json!({ "path": main, "line": 2, "enabled": true }));
    setup.dispatch("debug.breakpoint.set", json!({ "path": main, "line": 5, "condition": "x > 1" }));
    let edit = |setup: &mut Setup, range: std::ops::Range<usize>, text: &str| {
        let document = setup.session.state_mut::<Workbench>().active_document_mut().unwrap();
        assert!(document.replace_ranges(vec![(range, text.to_owned())]));
    };
    edit(&mut setup, 0..0, "new\n");
    let lines = |setup: &mut Setup| -> Vec<u64> {
        let listed = setup.dispatch("debug.breakpoint.list", json!({ "path": setup.main }));
        listed["breakpoints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|breakpoint| breakpoint["line"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(lines(&mut setup), [3, 6]);
    let start = setup.session.state::<Workbench>().unwrap().active_document().unwrap().text().find("line 3").unwrap();
    edit(&mut setup, start..start + "line 3\n".len(), "");
    assert_eq!(lines(&mut setup), [5], "a deleted line takes its breakpoint along");
    let text = setup.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    let joined = text.find("line 6").unwrap() - 1;
    edit(&mut setup, joined..joined + 1, "");
    assert_eq!(lines(&mut setup), [4], "joining lines keeps the breakpoint on the joined line");
    sync(&mut setup.session);
    let breakpoints = setup.session.state::<Breakpoints>().unwrap();
    assert_eq!(breakpoints.for_path(&main)[0].condition.as_deref(), Some("x > 1"));
    let document = setup.session.state_mut::<Workbench>().active_document_mut().unwrap();
    let all = document.text().chars().count();
    let replaced: String = (1..=12).map(|line| format!("other {line}\n")).collect();
    assert!(document.replace_ranges(vec![(0..all, replaced)]));
    assert_eq!(lines(&mut setup), [4], "replacing the whole text keeps breakpoints on their lines");
    setup.dispatch("debug.breakpoint.toggle", json!({ "path": main, "line": 4 }));
    assert_eq!(setup.dispatch("debug.breakpoint.list", json!({}))["breakpoints"], json!([]));
    assert!(
        setup
            .fail("debug.breakpoint.remove", json!({ "path": main, "line": 4 }))
            .message
            .mentions("debug-no-breakpoint")
    );
}

#[test]
fn breakpoints_round_trip_through_the_workspace() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    setup.dispatch("debug.breakpoint.set", json!({ "path": main, "line": 1, "enabled": false }));
    setup.dispatch("debug.breakpoint.set", json!({ "path": main, "line": 7, "condition": "n == 2" }));
    let saved = serde_json::to_value(setup.session.state::<Breakpoints>().unwrap().all()).unwrap();
    assert_eq!(
        saved,
        json!([
            { "path": main, "line": 1, "enabled": false },
            { "path": main, "line": 7, "enabled": true, "condition": "n == 2" },
        ])
    );
    let mut other = Setup::fake();
    other.session.state_mut::<Breakpoints>().restore(serde_json::from_value(saved.clone()).unwrap());
    assert_eq!(other.dispatch("debug.breakpoint.list", json!({}))["breakpoints"], saved);
}

#[test]
fn breakpoints_changed_while_a_session_runs_are_sent_at_once() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    setup.dispatch("file.document.open", json!({ "path": main }));
    setup.program(&[2, 6, 7, 9]);
    setup.breakpoint(5);
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 5);
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert_eq!(listed["breakpoints"][0]["verified"], true);
    setup.dispatch("debug.breakpoint.toggle", json!({ "path": main, "line": 6 }));
    assert_eq!(setup.sent_lines(), [6, 7]);
    let toggled = setup.dispatch("debug.breakpoint.toggle", json!({ "path": main, "line": 9 }));
    assert_eq!(toggled["breakpoint"]["verified"], false);
    assert_eq!(toggled["breakpoint"]["message"], "no code here");
    setup.dispatch("debug.breakpoint.set", json!({ "path": main, "line": 9, "enabled": false }));
    assert_eq!(setup.sent_lines(), [6, 7]);
    let document = setup.session.state_mut::<Workbench>().active_document_mut().unwrap();
    document.replace_ranges(vec![(0..0, "\n".to_owned())]);
    sync(&mut setup.session);
    assert_eq!(setup.sent_lines(), [7, 8], "an edit moves breakpoints and resends them");
    let document = setup.session.state_mut::<Workbench>().active_document_mut().unwrap();
    document.replace_ranges(vec![(0..1, String::new())]);
    sync(&mut setup.session);
    assert_eq!(setup.sent_lines(), [6, 7]);
    setup.dispatch("debug.execution.resume", json!({}));
    let state = setup.get();
    assert_eq!((state["reason"].clone(), state["frame"]["line"].clone()), (json!("breakpoint"), json!(6)));
    setup.dispatch("debug.execution.resume", json!({}));
    assert_eq!(setup.get()["state"], "ended");
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert!(
        listed["breakpoints"].as_array().unwrap().iter().all(|breakpoint| breakpoint.get("verified").is_none()),
        "{listed}"
    );
}

#[test]
fn a_frame_never_waits_for_set_breakpoints_and_a_refused_set_is_not_resent() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    setup.program(&[2, 6, 7, 9]);
    setup.breakpoint(5);
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 5);
    let before = setup.adapter.requests("setBreakpoints").len();
    setup.adapter.set_silent("setBreakpoints");
    setup.session.state_mut::<Breakpoints>().toggle(&main, 6);
    let started = Instant::now();
    sync_soon(&mut setup.session);
    sync_soon(&mut setup.session);
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    assert_eq!(setup.adapter.wait_for("setBreakpoints", before + 1, Duration::from_secs(30)).len(), before + 1);
    sync_soon(&mut setup.session);
    assert_eq!(setup.adapter.requests("setBreakpoints").len(), before + 1, "one request while it is unanswered");
    sync_within(&mut setup.session, Duration::from_millis(50));
    for _ in 0..3 {
        sync_soon(&mut setup.session);
    }
    sync_within(&mut setup.session, Duration::from_millis(50));
    assert_eq!(setup.adapter.requests("setBreakpoints").len(), before + 1, "a failed set is not sent again");
    setup.session.state_mut::<Breakpoints>().toggle(&main, 7);
    sync_soon(&mut setup.session);
    assert_eq!(setup.adapter.wait_for("setBreakpoints", before + 2, Duration::from_secs(30)).len(), before + 2);
}

#[test]
fn run_to_cursor_stops_at_the_line_and_removes_its_breakpoint() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    setup.program(&[2, 4, 6, 8]);
    setup.breakpoint(1);
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 1);
    setup.dispatch("debug.execution.runToCursor", json!({ "path": main, "line": 5 }));
    let state = setup.get();
    assert_eq!(state["frame"]["line"], 5, "{state}");
    let deadline = Instant::now() + Duration::from_secs(60);
    while setup.sent_lines() != [2] {
        assert!(Instant::now() < deadline, "the temporary breakpoint stays: {:?}", setup.sent_lines());
        std::thread::sleep(Duration::from_millis(10));
    }
    let all = setup.adapter.requests("setBreakpoints");
    assert_eq!(all[all.len() - 2]["breakpoints"], json!([{ "line": 2 }, { "line": 6 }]));
    setup.dispatch("debug.execution.resume", json!({}));
    assert_eq!(setup.get()["state"], "ended");
}

#[test]
fn stopping_a_session_from_the_run_disconnects_and_ends_it() {
    let mut setup = Setup::fake();
    setup.program(&[2, 3]);
    setup.breakpoint(1);
    let (session_id, run_id) = setup.start();
    assert_eq!(setup.get()["state"], "stopped");
    let stopped = setup.dispatch("run.process.stop", json!({}));
    assert_eq!(stopped["stopped"], run_id);
    let state = setup.get();
    assert_eq!((state["state"].clone(), state["reason"].clone()), (json!("ended"), json!("stopped")));
    let output = setup.output(run_id);
    assert_eq!(output["state"], "stopped");
    assert_eq!(texts(&output).last().unwrap(), &line("system", &t!("workbench-run-process-stopped")));
    assert_eq!(setup.adapter.requests("disconnect"), [json!({ "terminateDebuggee": true })]);

    let (second, second_run) = setup.start();
    assert_ne!(second, session_id);
    assert_eq!(setup.get()["state"], "stopped");
    setup.dispatch("debug.session.stop", json!({ "sessionId": second }));
    assert_eq!(setup.get()["reason"], "stopped");
    assert_eq!(setup.output(second_run)["state"], "stopped");
    let first = setup.dispatch("debug.session.get", json!({ "sessionId": session_id }));
    assert_eq!(first["state"], "ended");
    assert!(setup.fail("debug.session.get", json!({ "sessionId": 99 })).message.mentions("debug-unknown-session"));
}

#[test]
fn a_debugger_that_cannot_start_the_program_fails_the_run() {
    let mut setup = Setup::fake();
    setup.adapter.fail_launch(Some("cannot exec"));
    let (_, run_id) = setup.start();
    let state = setup.get();
    assert_eq!((state["state"].clone(), state["reason"].clone()), (json!("ended"), json!("failed")));
    let output = setup.output(run_id);
    assert_eq!(output["state"], "failed");
    assert!(output["error"].as_str().unwrap().mentions("debug-handshake-failed"), "{output}");
}

#[test]
fn nothing_starts_without_a_session_or_a_debugger() {
    let mut setup = Setup::fake();
    assert!(setup.fail("debug.session.get", json!({})).message.mentions("debug-no-session"));
    assert!(setup.fail("debug.execution.resume", json!({})).message.mentions("debug-no-session"));
    let mut configuration = serde_json::to_value(FakeProvider::configuration("x")).unwrap();
    configuration["options"] = json!({ "noDebug": true });
    let error = setup.fail("debug.session.start", json!({ "configuration": configuration }));
    assert!(error.message.mentions("debug-cannot-debug"), "{error}");
    assert_eq!(error.code, None);
    configuration["options"] = json!({ "fail": "no module" });
    let error = setup.fail("debug.session.start", json!({ "configuration": configuration }));
    assert!(error.message.mentions("debug-cannot-start"), "{error}");

    let planned = Planned::new(Vec::new(), "");
    let mut setup = Setup::new(vec![Arc::new(planned)]);
    let error = setup.fail("debug.session.start", json!({ "configuration": Planned::configuration() }));
    assert_eq!(error.code.as_deref(), Some(DEBUGGER_MISSING));
    let serialised = serde_json::to_value(&error).unwrap();
    assert_eq!(serialised["code"], "debuggerMissing");
    assert!(setup.session.state::<Runs>().unwrap().runs().is_empty());
}

struct Planned {
    build: Vec<CommandLine>,
    program: String,
    debugger: Arc<std::sync::Mutex<PathBuf>>,
    scratch: Arc<std::sync::Mutex<PathBuf>>,
}

impl Planned {
    fn new(build: Vec<CommandLine>, program: &str) -> Self {
        Self { build, program: program.to_owned(), debugger: Arc::default(), scratch: Arc::default() }
    }

    fn configuration() -> Value {
        json!({ "kind": "planned.run", "name": "planned" })
    }
}

impl RunProvider for Planned {
    fn id(&self) -> &str {
        "planned"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: "planned.run".to_owned(),
            name_message: "planned-kind".to_owned(),
            fields: Vec::new(),
            defaults: json!({}),
        }]
    }

    fn targets(&self, _context: &RunContext, _path: &Path, _text: &str) -> Vec<RunTarget> {
        Vec::new()
    }

    fn context(&self, _root: &Path, _path: &Path, _text: &str, _offset: usize) -> Option<Configuration> {
        None
    }

    fn command_lines(&self, _configuration: &Configuration, _context: &RunContext) -> Result<Vec<CommandLine>, String> {
        Ok(self.build.clone())
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }

    fn debug(&self, _configuration: &Configuration, context: &RunContext) -> Option<Result<DebugPlan, String>> {
        if let Ok(mut debugger) = self.debugger.lock() {
            *debugger = context.debugger.clone();
        }
        if let Ok(mut scratch) = self.scratch.lock() {
            *scratch = context.scratch.clone();
        }
        let scratch = context.scratch.to_string_lossy().into_owned();
        let build = self
            .build
            .iter()
            .cloned()
            .map(|mut line| {
                line.args = line.args.iter().map(|arg| arg.replace("$SCRATCH", &scratch)).collect();
                line
            })
            .collect();
        let line = CommandLine {
            program: PathBuf::from(&self.program),
            display: self.program.clone(),
            ..CommandLine::default()
        };
        Some(Ok(DebugPlan {
            build,
            adapter: AdapterLine {
                line,
                address_flag: FAKE_ADDRESS_FLAG.to_owned(),
                server: None,
                views: None,
                builtin: None,
            },
            launch: json!({ "request": "launch", "mode": "exec" }),
            debuggee: None,
        }))
    }

    fn explain_debug_error(&self, message: &str) -> Option<String> {
        message.contains("debugserver").then(|| "Install the command line tools.".to_owned())
    }
}

#[cfg(unix)]
fn shell(script: &str) -> CommandLine {
    CommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), script.to_owned()],
        display: format!("sh {script}"),
        ..CommandLine::default()
    }
}

#[cfg(unix)]
#[test]
fn the_build_runs_first_and_a_failed_build_ends_the_session_with_links() {
    let planned = Planned::new(vec![shell("echo built")], FAKE_ADAPTER);
    let debugger = planned.debugger.clone();
    let mut setup = Setup::new(vec![Arc::new(planned)]);
    setup.session.state_mut::<Runs>().set_debugger(PathBuf::from("/opt/dlv"));
    setup.program(&[2, 3]);
    setup.breakpoint(1);
    let started = setup.dispatch("debug.session.start", json!({ "configuration": Planned::configuration() }));
    assert_eq!(setup.get()["state"], "stopped");
    assert_eq!(*debugger.lock().unwrap(), PathBuf::from("/opt/dlv"));
    setup.dispatch("debug.execution.resume", json!({}));
    let output = setup.output(started["runId"].as_u64().unwrap());
    let lines = texts(&output);
    assert_eq!(lines[..2], [line("system", "sh echo built"), line("stdout", "built")]);
    assert!(lines[2].1.starts_with(&format!("{FAKE_ADAPTER} {FAKE_ADDRESS_FLAG}=127.0.0.1:")), "{lines:?}");

    let failing = Planned::new(vec![shell("echo 'main.go:3:1: undefined: x' >&2; exit 2")], FAKE_ADAPTER);
    let mut setup = Setup::new(vec![Arc::new(failing)]);
    let started = setup.dispatch("debug.session.start", json!({ "configuration": Planned::configuration() }));
    let state = setup.get();
    assert_eq!((state["state"].clone(), state["reason"].clone()), (json!("ended"), json!("buildFailed")));
    let output = setup.output(started["runId"].as_u64().unwrap());
    assert_eq!(output["exitCode"], 2);
    assert_eq!(output["lines"][1]["links"][0]["file"], "main.go");
    assert!(setup.adapter.received().is_empty(), "no debugger starts after a failed build");
}

#[test]
fn killing_a_session_asks_the_debugger_to_end_the_debuggee() {
    let mut setup = Setup::fake();
    setup.program(&[2, 6, 7, 9]);
    setup.breakpoint(5);
    let (_, run_id) = setup.start();
    assert_eq!(setup.get()["frame"]["line"], 5);
    setup.session.state::<Runs>().unwrap().run(run_id).unwrap().kill();
    let disconnect = setup.adapter.wait_for("disconnect", 1, Duration::from_secs(30));
    assert_eq!(disconnect[0]["terminateDebuggee"], true);
}

#[cfg(unix)]
#[test]
fn stopping_during_the_build_stops_the_build() {
    let planned = Planned::new(vec![shell("echo building; sleep 30")], FAKE_ADAPTER);
    let mut setup = Setup::new(vec![Arc::new(planned)]);
    let started = setup.dispatch("debug.session.start", json!({ "configuration": Planned::configuration() }));
    let run_id = started["runId"].as_u64().unwrap();
    setup.dispatch("run.output.get", json!({ "runId": run_id, "from": 1, "waitMs": WAIT }));
    assert_eq!(setup.dispatch("debug.session.get", json!({}))["state"], "starting");
    setup.dispatch("debug.session.stop", json!({}));
    assert_eq!(setup.output(run_id)["state"], "stopped");
    assert_eq!(setup.get()["reason"], "stopped");
    assert!(setup.adapter.received().is_empty());
}

#[test]
fn a_known_adapter_error_is_explained_by_the_provider() {
    let mut setup = Setup::new(vec![Arc::new(Planned::new(Vec::new(), FAKE_ADAPTER))]);
    setup.adapter.fail_launch(Some("could not launch process: debugserver not found"));
    let started = setup.dispatch("debug.session.start", json!({ "configuration": Planned::configuration() }));
    assert_eq!(setup.get()["reason"], "failed");
    let output = setup.output(started["runId"].as_u64().unwrap());
    let error = output["error"].as_str().unwrap();
    assert!(error.mentions("debug-handshake-failed") && error.ends_with(" Install the command line tools."), "{error}");
}

#[cfg(unix)]
#[test]
fn the_built_program_stays_until_the_session_ends() {
    let planned = Planned::new(vec![shell("echo built > $SCRATCH/app")], FAKE_ADAPTER);
    let scratch = planned.scratch.clone();
    let mut setup = Setup::new(vec![Arc::new(planned)]);
    setup.program(&[2, 3]);
    setup.breakpoint(1);
    let started = setup.dispatch("debug.session.start", json!({ "configuration": Planned::configuration() }));
    assert_eq!(setup.get()["state"], "stopped");
    let program = scratch.lock().unwrap().join("app");
    assert!(program.is_file(), "the program is there while the session is stopped");
    setup.dispatch("debug.execution.resume", json!({}));
    assert_eq!(setup.get()["state"], "ended");
    setup.output(started["runId"].as_u64().unwrap());
    assert!(!program.exists(), "the run's folder goes with the session");
}

#[test]
fn a_debugger_without_hot_code_replace_says_it_cannot_reload_classes() {
    let mut setup = Setup::fake();
    setup.program(&[3, 4]);
    setup.breakpoint(2);
    setup.start();
    assert_eq!(setup.get()["state"], "stopped");
    let error = setup.fail("debug.classes.reload", json!({}));
    assert!(error.message.mentions("debug-reload-unsupported"), "{error:?}");
    assert!(setup.get().get("reloaded").is_none());
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn a_reload_reads_as_a_line_for_people() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let done = Reload { changed_classes: vec!["com.acme.App".into()], error: None, automatic: false };
    assert_eq!(reload_message(&done), "Reloaded 1 class: com.acme.App");
    let failed =
        Reload { changed_classes: Vec::new(), error: Some("add method not implemented".into()), automatic: true };
    assert!(reload_message(&failed).contains("add method not implemented"));
    let none = Reload { changed_classes: Vec::new(), error: None, automatic: false };
    assert!(reload_message(&none).contains("No classes changed"));
}

fn console(output: &Value) -> Vec<String> {
    texts(output).into_iter().map(|(_, text)| text).collect()
}

#[test]
fn a_logging_breakpoint_is_a_log_point_when_the_debugger_has_them() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({ "supportsLogPoints": true }));
    setup.program(&[2, 4, 6]);
    setup.set(3, json!({ "suspend": "none", "logHit": true, "logExpression": "x" }));
    let (_, run) = setup.start();
    let output = setup.output(run);
    assert_eq!(setup.sent()[0]["logMessage"], "Breakpoint reached at main.go:4\n{x}");
    assert!(console(&output).contains(&"Breakpoint reached at main.go:4".to_owned()), "{output}");
    assert_eq!(setup.get()["stops"], 0, "a log point never stops the program");
}

#[test]
fn without_log_points_the_ide_logs_the_hit_and_lets_the_program_run_on() {
    let mut setup = Setup::fake();
    setup.program(&[2, 4, 6]);
    setup.adapter.set_variables(vec![FakeVariable::new("x", "41", "int")]);
    setup.set(3, json!({ "suspend": "none", "logHit": true, "logStack": true, "logExpression": "x" }));
    let (_, run) = setup.start();
    let output = setup.output(run);
    assert_eq!(setup.sent()[0].get("logMessage"), None);
    let lines = console(&output);
    let at = lines.iter().position(|line| line == "Breakpoint reached at main.go:4").expect("the hit is logged");
    assert_eq!(lines[at + 1], "\tmain.main (main.go:4)", "the stack follows");
    assert_eq!(lines[at + 2], "41", "then the expression's value");
    assert_eq!(setup.get()["stops"], 0, "the stop is not shown");
}

#[test]
fn a_temporary_breakpoint_is_removed_once_it_stops_the_program() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    setup.program(&[2, 4]);
    let set = setup.dispatch("debug.breakpoint.toggleTemporary", json!({ "path": main, "line": 1 }));
    assert_eq!(set["breakpoint"]["removeOnceHit"], true);
    setup.breakpoint(3);
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 1);
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    let lines: Vec<&Value> =
        listed["breakpoints"].as_array().unwrap().iter().map(|breakpoint| &breakpoint["line"]).collect();
    assert_eq!(lines, [&json!(3)], "the program stays stopped, the breakpoint is gone");
    assert_eq!(setup.sent_lines(), [4]);
}

#[test]
fn a_breakpoint_waits_for_its_master_and_disables_again_after_its_own_hit() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    setup.program(&[2, 4, 6, 4, 6]);
    setup.breakpoint(1);
    setup.set(5, json!({ "dependsOn": { "path": main, "line": 1 } }));
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert_eq!(listed["breakpoints"][1]["dependsOn"], json!({ "path": main, "line": 1 }));
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 1);
    let first = setup.adapter.requests("setBreakpoints");
    assert_eq!(
        first[0]["breakpoints"],
        json!([{ "line": 2 }]),
        "the dependent one is not sent before its master's hit"
    );
    setup.dispatch("debug.breakpoint.list", json!({}));
    assert_eq!(setup.sent_lines(), [2, 6], "the master's hit turns it on");
    setup.dispatch("debug.execution.resume", json!({}));
    assert_eq!(setup.get()["frame"]["line"], 5);
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert_eq!(listed["breakpoints"][1]["waiting"], true, "{listed}");
    assert_eq!(setup.sent_lines(), [2], "Disable again");
    setup.set(5, json!({ "afterHit": "leaveEnabled" }));
    setup.dispatch("debug.execution.resume", json!({}));
    assert_eq!(setup.get()["state"], "ended");
    let missing = setup
        .fail("debug.breakpoint.set", json!({ "path": main, "line": 5, "dependsOn": { "path": main, "line": 9 } }));
    assert!(missing.message.mentions("debug-no-breakpoint"), "{missing:?}");
}

#[test]
fn muted_breakpoints_send_empty_lists_and_come_back_unmuted() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({
        "supportsFunctionBreakpoints": true,
        "exceptionBreakpointFilters": [{ "filter": "uncaught", "label": "Uncaught Exceptions" }],
    }));
    setup.program(&[2, 4, 6]);
    setup.breakpoint(1);
    setup.breakpoint(3);
    setup.dispatch("debug.functionBreakpoint.add", json!({ "name": "main.run" }));
    setup.dispatch("debug.exceptionBreakpoint.add", json!({ "filter": "uncaught" }));
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 1);
    assert_eq!(setup.adapter.requests("setExceptionBreakpoints").last().unwrap()["filters"], json!(["uncaught"]));
    let muted = setup.dispatch("debug.breakpoints.mute", json!({}));
    assert_eq!(muted["muted"], true);
    assert!(setup.sent_lines().is_empty());
    assert_eq!(setup.adapter.requests("setFunctionBreakpoints").last().unwrap()["breakpoints"], json!([]));
    assert_eq!(setup.adapter.requests("setExceptionBreakpoints").last().unwrap()["filters"], json!([]));
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert_eq!((listed["muted"].clone(), listed["breakpoints"].as_array().unwrap().len()), (json!(true), 2));
    setup.dispatch("debug.breakpoints.mute", json!({ "muted": false }));
    assert_eq!(setup.sent_lines(), [2, 4]);
}

#[test]
fn what_the_debugger_lacks_is_not_sent() {
    let mut setup = Setup::fake();
    setup.program(&[2, 4, 4, 4]);
    setup.set(3, json!({ "hitCondition": "3", "condition": "x > 1" }));
    setup.dispatch("debug.functionBreakpoint.add", json!({ "name": "main.run" }));
    setup.dispatch("debug.exceptionBreakpoint.add", json!({ "filter": "uncaught" }));
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 3);
    assert_eq!(setup.sent()[0], json!({ "line": 4, "condition": "x > 1" }));
    assert!(setup.adapter.requests("setFunctionBreakpoints").is_empty());
    assert_eq!(setup.adapter.requests("setExceptionBreakpoints").last().unwrap()["filters"], json!([]));
    let support = setup.dispatch("debug.breakpoint.list", json!({}))["support"].clone();
    assert_eq!((support["hitConditions"].clone(), support["functions"].clone()), (json!(false), json!(false)));
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn pass_counts_function_and_data_breakpoints_reach_a_debugger_that_has_them() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({
        "supportsHitConditionalBreakpoints": true,
        "supportsFunctionBreakpoints": true,
        "supportsDataBreakpoints": true,
    }));
    setup.adapter.set_functions(&[("main.run", 3)]);
    setup.adapter.set_variables(vec![FakeVariable::new("x", "1", "int")]);
    setup.program(&[4, 4, 4, 9]);
    setup.set(3, json!({ "hitCondition": "2" }));
    setup.dispatch("debug.functionBreakpoint.add", json!({ "name": "main.run" }));
    setup.dispatch("debug.functionBreakpoint.add", json!({ "name": "main.gone" }));
    let (_, run) = setup.start();
    let state = setup.get();
    assert_eq!((state["frame"]["line"].clone(), setup.sent()[0]["hitCondition"].clone()), (json!(3), json!("2")));
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert_eq!(listed["functions"][0]["verified"], true);
    assert_eq!(listed["functions"][1]["message"], "no such function");
    let added = setup.dispatch("debug.dataBreakpoint.add", json!({ "reference": 1, "name": "x" }));
    assert_eq!(added["breakpoint"]["accessType"], "write");
    setup.dispatch("debug.breakpoint.list", json!({}));
    assert_eq!(setup.adapter.requests("setDataBreakpoints").last().unwrap()["breakpoints"][0]["dataId"], "1/x");
    setup.dispatch("debug.execution.resume", json!({}));
    let state = setup.get();
    assert_eq!((state["reason"].clone(), state["frame"]["line"].clone()), (json!("function breakpoint"), json!(8)));
    setup.dispatch("debug.execution.resume", json!({}));
    setup.output(run);
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert!(listed["data"].as_array().unwrap().is_empty(), "data breakpoints last as long as their session");
}

#[test]
fn suspend_thread_lets_the_other_threads_run_on() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({ "supportsSingleThreadExecutionRequests": true }));
    setup
        .adapter
        .set_threads(vec![Thread { id: 1, name: "main".to_owned() }, Thread { id: 2, name: "worker".to_owned() }]);
    setup.program(&[2, 4]);
    setup.set(1, json!({ "suspend": "thread" }));
    setup.start();
    assert_eq!(setup.get()["state"], "stopped");
    let continued = setup.adapter.wait_for("continue", 1, Duration::from_secs(5));
    assert_eq!(continued[0]["threadId"], 2);
    assert_eq!(continued[0]["singleThread"], true);
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn breakpoints_are_moved_copied_disabled_and_removed_as_the_gutter_asks() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    setup.dispatch("file.document.open", json!({ "path": main }));
    setup.set(1, json!({ "condition": "a", "group": "Mine" }));
    setup.breakpoint(4);
    let moved = setup.dispatch("debug.breakpoint.move", json!({ "path": main, "line": 1, "toLine": 2 }));
    assert_eq!((moved["breakpoint"]["line"].clone(), moved["breakpoint"]["condition"].clone()), (json!(2), json!("a")));
    let refused = setup.fail("debug.breakpoint.move", json!({ "path": main, "line": 2, "toLine": 4 }));
    assert!(refused.message.mentions("debug-cannot-move"));
    let copied = setup.dispatch("debug.breakpoint.copy", json!({ "path": main, "line": 2, "toLine": 6 }));
    assert_eq!(copied["breakpoint"]["group"], "Mine");
    let toggled = setup.dispatch("debug.breakpoint.toggleEnabled", json!({ "path": main, "line": 6 }));
    assert_eq!(toggled["breakpoint"]["enabled"], false);
    let disabled = setup.dispatch("debug.breakpoint.disableOthers", json!({ "path": main, "line": 6 }));
    assert_eq!(disabled["count"], 2);
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert!(listed["breakpoints"].as_array().unwrap().iter().all(|breakpoint| breakpoint["enabled"] == false));
    let removed = setup.dispatch("debug.breakpoint.removeOthers", json!({ "path": main, "line": 4 }));
    assert_eq!(removed["count"], 2);
    let listed = setup.dispatch("debug.breakpoint.list", json!({}));
    assert_eq!(listed["breakpoints"].as_array().unwrap().len(), 1);
    let none = setup.fail("debug.breakpoint.toggleEnabled", json!({ "path": main, "line": 9 }));
    assert!(none.message.mentions("debug-no-breakpoint"));
}

#[test]
fn debugging_a_compound_debugs_the_members_that_can_be_debugged_and_runs_the_others() {
    let mut setup = Setup::fake();
    let root = setup._dir.path().to_owned();
    crate::trust::trust_session_in_tests(&mut setup.session, &root);
    let mut app = FakeProvider::configuration("app");
    app.id = "app".to_owned();
    let mut tool = FakeProvider::configuration("tool");
    tool.id = "tool".to_owned();
    tool.options = json!({ "target": "tool", "noDebug": true });
    for configuration in [&app, &tool] {
        setup.dispatch("run.configuration.create", json!({ "configuration": configuration }));
    }
    let both =
        json!({ "id": "both", "name": "both", "kind": "compound", "options": { "configurations": ["app", "tool"] } });
    setup.dispatch("run.configuration.create", json!({ "configuration": both }));
    let started = setup.dispatch("debug.session.start", json!({ "configurationId": "both" }));
    assert_eq!(started["runs"].as_array().unwrap().len(), 2, "{started}");
    let list = setup.dispatch("run.process.list", json!({}));
    let runs = list["runs"].as_array().unwrap();
    let debugged: Vec<(&str, bool)> =
        runs.iter().map(|run| (run["name"].as_str().unwrap(), run["debug"].as_bool().unwrap())).collect();
    assert_eq!(debugged, [("app", true), ("tool", false)]);
    assert!(runs.iter().all(|run| run["compound"] == "both"), "{list}");
    assert_eq!(setup.dispatch("debug.session.list", json!({}))["sessions"].as_array().unwrap().len(), 1);

    let only = json!({ "id": "only", "name": "only", "kind": "compound", "options": { "configurations": ["tool"] } });
    setup.dispatch("run.configuration.create", json!({ "configuration": only }));
    let error = setup.fail("debug.session.start", json!({ "configurationId": "only" }));
    assert!(error.message.mentions("workbench-run-compound-no-debug"), "{}", error.message);
}

fn sent_before(setup: &Setup, asked: &str, later: &str) -> Value {
    let requests: Vec<Value> =
        setup.adapter.received().into_iter().filter(|message| message["type"] == "request").collect();
    let later = requests.iter().rposition(|request| request["command"] == later).unwrap();
    let found = requests[..later].iter().rfind(|request| request["command"] == asked).unwrap();
    found["arguments"].clone()
}

fn wait_for_lines(setup: &Setup, wanted: &[i64]) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while setup.sent_lines() != wanted {
        assert!(Instant::now() < deadline, "the breakpoints are {:?}, not {wanted:?}", setup.sent_lines());
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn force_run_to_cursor_passes_the_breakpoints_on_the_way_and_brings_them_back() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({ "supportsFunctionBreakpoints": true }));
    setup.adapter.set_functions(&[("main.run", 2)]);
    let main = setup.main.clone();
    setup.program(&[2, 4, 6, 8]);
    setup.breakpoint(1);
    setup.breakpoint(3);
    setup.dispatch("debug.functionBreakpoint.add", json!({ "name": "main.run" }));
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 1);
    setup.dispatch("debug.execution.forceRunToCursor", json!({ "path": main, "line": 7 }));
    let lines = sent_before(&setup, "setBreakpoints", "continue");
    assert_eq!(lines["breakpoints"], json!([{ "line": 8 }]), "only the target line");
    assert_eq!(sent_before(&setup, "setFunctionBreakpoints", "continue")["breakpoints"], json!([]));
    let state = setup.get();
    assert_eq!(state["frame"]["line"], 7, "the breakpoint at line 4 and main.run were passed: {state}");
    wait_for_lines(&setup, &[2, 4]);
    let functions = setup.adapter.requests("setFunctionBreakpoints");
    assert_eq!(functions.last().unwrap()["breakpoints"][0]["name"], "main.run");
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn force_step_over_ignores_the_breakpoints_for_the_step_and_force_step_into_steps_in() {
    let mut setup = Setup::fake();
    setup.program(&[2, 4, 6]);
    setup.breakpoint(1);
    setup.breakpoint(5);
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 1);
    setup.dispatch("debug.execution.forceStepOver", json!({}));
    let commands = setup.adapter.commands();
    let next = commands.iter().rposition(|command| command == "next").unwrap();
    assert_eq!(commands[next - 1], "setBreakpoints");
    assert_eq!(sent_before(&setup, "setBreakpoints", "next")["breakpoints"], json!([]));
    assert_eq!(setup.get()["frame"]["line"], 3);
    wait_for_lines(&setup, &[2, 6]);
    setup.dispatch("debug.execution.forceStepInto", json!({}));
    assert_eq!(setup.adapter.requests("stepIn").len(), 1);
    assert_eq!(setup.get()["frame"]["line"], 5);
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn set_value_changes_a_variable_with_set_variable_and_reports_a_refusal() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({ "supportsSetVariable": true }));
    setup.adapter.set_variables(vec![
        FakeVariable::new("x", "1", "int"),
        FakeVariable::new("s", "{a: 2}", "main.S").with_children(vec![FakeVariable::new("a", "2", "int")]),
    ]);
    setup.program(&[2, 3]);
    setup.breakpoint(1);
    setup.start();
    assert_eq!(setup.get()["support"]["setValue"], true);
    assert!(setup.registry.is_available(&setup.session, "debug.variable.set"));
    let before = setup.get()["values"].as_u64().unwrap();
    assert_eq!(
        answered(&mut setup, "debug.variables.list", json!({ "reference": 1, "waitMs": 0 }))["variables"][0]["value"],
        "1"
    );
    let set =
        answered(&mut setup, "debug.variable.set", json!({ "reference": 1, "name": "x", "value": "42", "waitMs": 0 }));
    assert_eq!((set["value"].clone(), set["type"].clone()), (json!("42"), json!("int")));
    assert_eq!(setup.adapter.requests("setVariable"), [json!({ "variablesReference": 1, "name": "x", "value": "42" })]);
    let listed = setup.dispatch("debug.variables.list", json!({ "reference": 1 }));
    assert_eq!(listed["variables"][0]["value"], "42", "the variables are read again");
    assert!(setup.get()["values"].as_u64().unwrap() > before);
    setup.dispatch("debug.variable.set", json!({ "reference": 2, "name": "a", "value": "7" }));
    assert_eq!(setup.dispatch("debug.variables.list", json!({ "reference": 2 }))["variables"][0]["value"], "7");
    let error = setup.fail("debug.variable.set", json!({ "reference": 1, "name": "x", "value": "x ?" }));
    assert!(error.message.contains("cannot assign"), "{}", error.message);
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn set_value_assigns_an_expression_or_is_refused_by_a_debugger_that_cannot() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({ "supportsSetExpression": true }));
    setup.adapter.set_variables(vec![FakeVariable::new("x", "1", "int")]);
    setup.program(&[2, 3]);
    setup.breakpoint(1);
    setup.start();
    assert_eq!(setup.get()["support"]["setValue"], true);
    let set =
        setup.dispatch("debug.variable.set", json!({ "reference": 1, "name": "x", "evaluateName": "x", "value": "5" }));
    assert_eq!(set["value"], "5");
    assert_eq!(setup.adapter.requests("setExpression")[0], json!({ "expression": "x", "value": "5", "frameId": 1000 }));
    assert!(setup.adapter.requests("setVariable").is_empty());
    setup.dispatch("debug.session.stop", json!({}));

    let mut setup = Setup::fake();
    setup.adapter.set_variables(vec![FakeVariable::new("x", "1", "int")]);
    setup.program(&[2, 3]);
    setup.breakpoint(1);
    setup.start();
    assert_eq!(setup.get()["support"]["setValue"], false);
    assert!(!setup.registry.is_available(&setup.session, "debug.variable.set"));
    let refused = setup.registry.dispatch(
        &mut setup.session,
        "debug.variable.set",
        json!({ "reference": 1, "name": "x", "value": "5" }),
    );
    assert!(matches!(refused, Err(DispatchError::Unavailable { .. })), "{refused:?}");
    assert!(setup.adapter.requests("setVariable").is_empty() && setup.adapter.requests("setExpression").is_empty());
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn inline_values_name_the_frames_variables_on_its_lines_and_mark_the_changed_ones() {
    let mut setup = Setup::fake();
    let main = setup.main.clone();
    std::fs::write(&main, "x := 1\ny := x + 2\nz := \"y\" // y\nfmt.Println(y.x, x)\nlater(x)\n").unwrap();
    setup.adapter.set_variables(vec![
        FakeVariable::new("x", "1", "int"),
        FakeVariable::new("y", "3", "int"),
        FakeVariable::new("fmt", "line one\nline two", "pkg"),
    ]);
    setup.program(&[3, 4, 5]);
    setup.breakpoint(3);
    setup.start();
    assert_eq!(setup.get()["frame"]["line"], 3);
    let inline = answered(&mut setup, "debug.inlineValues.get", json!({ "path": main, "waitMs": 0 }));
    assert_eq!(inline["path"], json!(main));
    let shown = |inline: &Value| -> Vec<(u64, String)> {
        inline["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| {
                let values: Vec<String> = line["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| {
                        let mark = if value["changed"] == true { "*" } else { "" };
                        format!("{}={}{mark}", value["name"].as_str().unwrap(), value["value"].as_str().unwrap())
                    })
                    .collect();
                (line["line"].as_u64().unwrap(), values.join(" "))
            })
            .collect()
    };
    assert_eq!(
        shown(&inline),
        [(0, "x=1".to_owned()), (1, "y=3 x=1".to_owned()), (3, "fmt=line one line two y=3 x=1".to_owned()),],
        "nothing after the stopped line, in strings, comments or after a dot"
    );
    let other = setup.dispatch("debug.inlineValues.get", json!({ "path": setup.main.with_file_name("other.go") }));
    assert_eq!(other["lines"], json!([]));

    setup.adapter.set_variables(vec![FakeVariable::new("x", "2", "int"), FakeVariable::new("y", "3", "int")]);
    setup.dispatch("debug.execution.stepOver", json!({}));
    assert_eq!(setup.get()["frame"]["line"], 4);
    let inline = answered(&mut setup, "debug.inlineValues.get", json!({ "waitMs": 0 }));
    assert_eq!(shown(&inline)[0], (0, "x=2*".to_owned()), "x changed since the previous stop");
    assert_eq!(shown(&inline)[1], (1, "y=3 x=2*".to_owned()));
    let again = setup.dispatch("debug.inlineValues.get", json!({}));
    assert_eq!(again, inline, "asking again in the same stop keeps the marks");
    setup.dispatch("debug.session.stop", json!({}));
}

fn frame(id: i64, name: &str, path: Option<&Path>, line: i64) -> zen_dap::StackFrame {
    let source = path.map(|path| zen_dap::Source::path(&path.to_string_lossy()));
    zen_dap::StackFrame { id, name: name.to_owned(), source, line, column: 1, ..zen_dap::StackFrame::default() }
}

#[test]
fn smart_step_into_lists_the_calls_on_the_line_and_steps_into_the_chosen_one() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({ "supportsStepInTargetsRequest": true }));
    setup.adapter.set_step_in_targets(vec![
        zen_dap::StepInTarget {
            id: 4,
            label: "twice".to_owned(),
            line: Some(2),
            column: Some(9),
            end_line: Some(2),
            end_column: Some(17),
        },
        zen_dap::StepInTarget { id: 5, label: "add".to_owned(), ..zen_dap::StepInTarget::default() },
    ]);
    setup.program(&[2, 3]);
    setup.breakpoint(1);
    setup.start();
    assert_eq!(setup.get()["support"]["smartStepInto"], true);
    assert!(setup.registry.is_available(&setup.session, "debug.execution.smartStepInto"));
    let targets = answered(&mut setup, "debug.execution.stepInTargets", json!({ "waitMs": 0 }));
    assert_eq!(
        targets,
        json!({ "targets": [
            { "id": 4, "label": "twice", "line": 1, "column": 8, "endLine": 1, "endColumn": 16 },
            { "id": 5, "label": "add" },
        ] })
    );
    assert_eq!(setup.adapter.requests("stepInTargets"), [json!({ "frameId": 1000 })]);
    setup.dispatch("debug.execution.smartStepInto", json!({ "targetId": 5 }));
    assert_eq!(setup.adapter.requests("stepIn"), [json!({ "threadId": 1, "targetId": 5 })]);
    let stopped = setup.get();
    assert_eq!((stopped["reason"].clone(), stopped["frame"]["line"].clone()), (json!("step"), json!(2)));
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn reset_frame_restarts_a_frame_that_can_restart_and_async_frames_follow_their_separator() {
    let mut setup = Setup::fake();
    setup.adapter.set_capabilities(json!({ "supportsRestartFrame": true }));
    let main = setup.main.clone();
    let mut fixed = frame(2, "main.main", Some(&main), 9);
    fixed.can_restart = Some(false);
    let mut separator = frame(3, "await", None, 0);
    separator.presentation_hint = Some("label".to_owned());
    let mut subtle = frame(5, "runtime.goexit", None, 0);
    subtle.presentation_hint = Some("subtle".to_owned());
    let stack = vec![
        frame(1, "main.helper", Some(&main), 3),
        fixed,
        separator,
        frame(4, "main.caller", Some(&main), 11),
        subtle,
    ];
    setup.adapter.set_frames(vec![stack.clone(), stack[..2].to_vec()]);
    setup.program(&[3, 4]);
    setup.breakpoint(2);
    setup.start();
    assert_eq!(setup.get()["support"]["resetFrame"], true);
    let frames = setup.dispatch("debug.frames.list", json!({}))["frames"].clone();
    let shown: Vec<(Value, Value, Value, Value)> = frames
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| {
            (frame["name"].clone(), frame["hint"].clone(), frame["async"].clone(), frame["canRestart"].clone())
        })
        .collect();
    assert_eq!(
        shown,
        [
            (json!("main.helper"), Value::Null, Value::Null, json!(true)),
            (json!("main.main"), Value::Null, Value::Null, Value::Null),
            (json!("await"), json!("label"), Value::Null, Value::Null),
            (json!("main.caller"), Value::Null, json!(true), Value::Null),
            (json!("runtime.goexit"), json!("subtle"), json!(true), Value::Null),
        ]
    );
    let error = setup.fail("debug.frame.reset", json!({ "frameId": 2 }));
    assert!(error.message.mentions("debug-frame-cannot-reset"), "{}", error.message);
    assert!(setup.adapter.requests("restartFrame").is_empty());
    setup.dispatch("debug.frame.reset", json!({}));
    assert_eq!(setup.adapter.requests("restartFrame"), [json!({ "frameId": 1 })]);
    let stopped = setup.get();
    assert_eq!(stopped["reason"], "restart");
    assert_eq!(setup.dispatch("debug.frames.list", json!({}))["frames"].as_array().unwrap().len(), 2);
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn without_the_capabilities_smart_step_into_and_reset_frame_are_unavailable_and_nothing_is_sent() {
    let mut setup = Setup::fake();
    setup.program(&[2, 3]);
    setup.breakpoint(1);
    setup.start();
    let support = setup.get()["support"].clone();
    assert_eq!((support["smartStepInto"].clone(), support["resetFrame"].clone()), (json!(false), json!(false)));
    assert_eq!(setup.dispatch("debug.frames.list", json!({}))["frames"][0]["canRestart"], Value::Null);
    for (id, params) in [
        ("debug.execution.stepInTargets", json!({})),
        ("debug.execution.smartStepInto", json!({ "targetId": 1 })),
        ("debug.frame.reset", json!({})),
    ] {
        assert!(!setup.registry.is_available(&setup.session, id), "{id}");
        let refused = setup.registry.dispatch(&mut setup.session, id, params);
        assert!(matches!(refused, Err(DispatchError::Unavailable { .. })), "{id}: {refused:?}");
    }
    let commands = setup.adapter.commands();
    assert!(
        !commands.iter().any(|command| command == "stepInTargets" || command == "restartFrame" || command == "stepIn")
    );
    setup.dispatch("debug.session.stop", json!({}));
}

fn with_views(setup: &mut Setup) -> (u64, u64) {
    setup.adapter.set_views_request(Some("zenViews"));
    let mut configuration = serde_json::to_value(FakeProvider::configuration("app")).unwrap();
    configuration["options"]["views"] = json!("zenViews");
    let started = setup.dispatch("debug.session.start", json!({ "configuration": configuration }));
    (started["sessionId"].as_u64().unwrap(), started["runId"].as_u64().unwrap())
}

fn shown(answer: &Value) -> Vec<(String, String)> {
    answer["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variable| (variable["name"].as_str().unwrap().to_owned(), variable["value"].as_str().unwrap().to_owned()))
        .collect()
}

fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

#[test]
fn collections_show_their_elements_with_lazy_values_resolved_map_entries_and_raw_fields_on_request() {
    let mut setup = Setup::fake();
    setup.adapter.set_variables(vec![
        FakeVariable::new("items", "ArrayList size=2", "ArrayList")
            .with_children(vec![
                FakeVariable::new("0", "Integer@7 \"3\"", "Integer").lazy("Integer@7"),
                FakeVariable::new("1", "\"b\"", "String"),
            ])
            .with_raw(vec![
                FakeVariable::new("elementData", "Object[2]", "Object[]"),
                FakeVariable::new("size", "2", "int"),
            ])
            .indexed(),
        FakeVariable::new("ages", "HashMap size=1", "HashMap")
            .with_children(vec![FakeVariable::new("0", "HashMap$Node@9 \"ann\":\"30\"", "HashMap$Node").with_children(
                vec![
                    FakeVariable::new("key", "\"ann\"", "String"),
                    FakeVariable::new("value", "Integer@4 \"30\"", "Integer").lazy("Integer@4"),
                ],
            )])
            .indexed(),
    ]);
    setup.program(&[2]);
    setup.breakpoint(1);
    with_views(&mut setup);
    assert_eq!(setup.get()["state"], "stopped");
    let scopes = setup.dispatch("debug.variables.list", json!({}));
    let locals = setup.dispatch("debug.variables.list", json!({ "reference": scopes["variables"][0]["reference"] }));
    let items = locals["variables"][0]["reference"].clone();
    let ages = locals["variables"][1]["reference"].clone();

    let elements = answered(&mut setup, "debug.variables.list", json!({ "reference": items, "waitMs": 0 }));
    assert_eq!(shown(&elements), [pair("0", "Integer@7 \"3\""), pair("1", "\"b\"")], "a lazy value is fetched");
    assert_eq!(setup.adapter.requests("zenViews"), [json!({ "raw": false })], "the elements view is chosen first");

    let raw = setup.dispatch("debug.variables.list", json!({ "reference": items, "raw": true }));
    assert_eq!(shown(&raw), [pair("elementData", "Object[2]"), pair("size", "2")]);
    assert_eq!(setup.adapter.requests("zenViews").last(), Some(&json!({ "raw": true })));
    let again = setup.dispatch("debug.variables.list", json!({ "reference": items }));
    assert_eq!(shown(&again), shown(&elements), "each view is kept for the stop");

    let entries = setup.dispatch("debug.variables.list", json!({ "reference": ages }));
    assert_eq!(entries["variables"][0]["entry"], json!({ "key": "\"ann\"", "value": "Integer@4 \"30\"" }));

    let toggled = setup.dispatch("debug.variables.raw", json!({ "raw": true }));
    assert_eq!(toggled, json!({ "raw": true, "supported": true }));
    let all_raw = setup.dispatch("debug.variables.list", json!({ "reference": items }));
    assert_eq!(shown(&all_raw), shown(&raw), "every collection shows its fields");
    assert!(all_raw["variables"][0].get("entry").is_none());
    setup.dispatch("debug.variables.raw", json!({ "raw": false }));
    let back = setup.dispatch("debug.variables.list", json!({ "reference": ages }));
    assert!(back["variables"][0]["entry"].is_object());
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn without_a_views_command_a_raw_request_lists_what_the_debugger_gives() {
    let mut setup = Setup::fake();
    setup.adapter.set_variables(vec![
        FakeVariable::new("items", "list", "List").with_children(vec![FakeVariable::new("0", "1", "int")]),
    ]);
    setup.program(&[2]);
    setup.breakpoint(1);
    setup.start();
    assert_eq!(setup.get()["state"], "stopped");
    let scopes = setup.dispatch("debug.variables.list", json!({}));
    let locals = setup.dispatch("debug.variables.list", json!({ "reference": scopes["variables"][0]["reference"] }));
    let raw = setup
        .dispatch("debug.variables.list", json!({ "reference": locals["variables"][0]["reference"], "raw": true }));
    assert_eq!(shown(&raw), [pair("0", "1")]);
    assert_eq!(setup.dispatch("debug.variables.raw", json!({ "raw": true }))["supported"], false);
    setup.dispatch("debug.variables.raw", json!({ "raw": false }));
    setup.dispatch("debug.session.stop", json!({}));
}

const SHOP: &str = "class Shop {
    void run(List<String> names) {
        var lengths = names.stream()
            .filter(n -> n.length() > 1)
            .map(String::length)
            .toList();
        names.stream().peek(System.out::println).count();
        names.stream().forEach(System.out::println);
    }
}
";

fn array(values: &[(&str, &str)]) -> FakeVariable {
    let children = values.iter().enumerate().map(|(index, (value, type_))| {
        let variable = FakeVariable::new(&index.to_string(), value, type_);
        if *type_ == "Integer" { variable.lazy("Integer@1") } else { variable }
    });
    FakeVariable::new("", &format!("Object[{}]@5", values.len()), "Object[]")
        .with_children(children.collect())
        .indexed()
}

#[test]
fn a_stream_chain_is_traced_one_operation_after_another_without_waiting() {
    let mut setup = Setup::fake();
    let shop = setup.main.with_file_name("Shop.java");
    std::fs::write(&shop, SHOP).unwrap();
    setup.adapter.set_program(&[(shop.as_path(), 3)]);
    setup.dispatch("debug.breakpoint.set", json!({ "path": shop, "line": 2 }));
    let source = "names.stream()";
    let filtered = "names.stream()\n            .filter(n -> n.length() > 1)";
    let mapped = format!("{filtered}\n            .map(String::length)");
    let whole = format!("{mapped}\n            .toList()");
    setup.adapter.set_evaluations(vec![
        (&format!("{source}.toArray()"), array(&[("\"a\"", "String"), ("\"bb\"", "String"), ("\"ccc\"", "String")])),
        (&format!("{filtered}.toArray()"), array(&[("\"bb\"", "String"), ("\"ccc\"", "String")])),
        (&format!("{mapped}.toArray()"), array(&[("Integer@2 \"2\"", "Integer"), ("Integer@3 \"3\"", "Integer")])),
        (
            &whole,
            FakeVariable::new("", "ImmutableCollections$ListN@8 size=2", "List")
                .with_children(vec![
                    FakeVariable::new("0", "Integer@2 \"2\"", "Integer"),
                    FakeVariable::new("1", "Integer@3 \"3\"", "Integer"),
                ])
                .indexed(),
        ),
    ]);
    setup.start();
    assert_eq!(setup.get()["state"], "stopped");

    let mut trace = setup.dispatch("debug.stream.trace", json!({ "waitMs": 0 }));
    assert_eq!(trace["pending"], true, "the first answer never waits: {trace}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while trace["pending"] == true {
        assert!(Instant::now() < deadline, "the trace stays pending: {trace}");
        trace = setup.dispatch("debug.stream.trace", json!({ "waitMs": 0 }));
    }
    let columns: Vec<(String, String, Vec<String>)> = trace["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| {
            let values = column["values"].as_array().unwrap().iter().map(|value| value.as_str().unwrap().to_owned());
            (
                column["label"].as_str().unwrap().to_owned(),
                column["kind"].as_str().unwrap().to_owned(),
                values.collect(),
            )
        })
        .collect();
    let strings = |values: &[&str]| values.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        columns,
        [
            ("names.stream()".to_owned(), "elements".to_owned(), strings(&["\"a\"", "\"bb\"", "\"ccc\""])),
            ("filter(n -> n.length() > 1)".to_owned(), "elements".to_owned(), strings(&["\"bb\"", "\"ccc\""])),
            ("map(String::length)".to_owned(), "elements".to_owned(), strings(&["Integer@2 \"2\"", "Integer@3 \"3\""])),
            ("toList()".to_owned(), "result".to_owned(), strings(&["Integer@2 \"2\"", "Integer@3 \"3\""])),
        ]
    );
    assert_eq!(trace["columns"][0]["count"], 3);
    assert_eq!(trace["columns"][3]["result"], "ImmutableCollections$ListN@8 size=2");
    let contexts: Vec<Value> =
        setup.adapter.requests("evaluate").iter().map(|asked| asked["context"].clone()).collect();
    assert!(contexts.iter().all(|context| context == "watch"), "kept out of the recent expressions: {contexts:?}");
    assert_eq!(setup.adapter.requests("evaluate").len(), 4, "each prefix is evaluated once");

    let peek = setup.fail("debug.stream.trace", json!({ "path": shop, "line": 6 }));
    assert!(peek.message.mentions("debug-stream-side-effect"), "{}", peek.message);
    let each = setup.dispatch("debug.stream.trace", json!({ "path": shop, "line": 7, "waitMs": 0 }));
    assert_eq!(each["columns"][1]["kind"], "notRun");
    let main = setup.main.clone();
    let go = setup.fail("debug.stream.trace", json!({ "path": main, "line": 0 }));
    assert!(go.message.mentions("debug-stream-java-only"), "{}", go.message);

    setup.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["**/Shop.java".to_owned()]);
    let params = json!({ "path": shop, "line": 2 });
    let Setup { session, registry, .. } = &mut setup;
    let hidden =
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, "debug.stream.trace", params));
    let hidden = format!("{:?}", hidden.unwrap_err());
    assert!(hidden.contains(crate::hidden_files::REDACTED_CODE), "an agent reads no hidden chain: {hidden}");
    setup.dispatch("debug.session.stop", json!({}));
}

fn coroutines(setup: &mut Setup) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let answer = setup.dispatch("debug.coroutines.list", json!({ "waitMs": 0 }));
        if answer["pending"] != true {
            return answer;
        }
        assert!(Instant::now() < deadline, "the coroutines stay pending: {answer}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn coroutines_are_one_fixed_dump_read_in_the_first_of_eight_frames_that_runs_it_and_never_after_a_pause() {
    use crate::java::coroutines::DUMP;
    let mut setup = Setup::fake();
    let kotlin = setup.main.with_file_name("App.kt");
    let java = setup.main.with_file_name("Main.java");
    let deep: Vec<zen_dap::StackFrame> =
        (101..=110).map(|id| frame(id, &format!("frame{id}"), (id == 101).then_some(kotlin.as_path()), 3)).collect();
    let near = vec![
        frame(201, "AppKt$main$1.invokeSuspend", Some(&kotlin), 7),
        frame(202, "BaseContinuationImpl.resumeWith", None, 0),
        frame(203, "Main.lambda$0", Some(&java), 5),
    ];
    setup.adapter.set_frames(vec![deep, near]);
    setup.adapter.set_evaluation_frames(Some(vec![110, 203]));
    let dump = "\"RUNNING\u{1e}root\u{1e}BlockingEventLoop@1\u{1e}\"root#1\":BlockingCoroutine{Active}@2\u{1e}\
                AppKt$main$1.invokeSuspend(App.kt:7)\u{1f}\nSUSPENDED\u{1e}worker\u{1e}Dispatchers.Default\u{1e}\
                \"worker#2\":StandaloneCoroutine{Active}@3\u{1e}\n\"";
    setup.adapter.set_evaluations(vec![(DUMP, FakeVariable::new("", dump, "String"))]);
    setup.program(&[3, 4, 5]);
    setup.breakpoint(2);
    setup.breakpoint(3);
    setup.breakpoint(4);
    setup.start();
    assert_eq!(setup.get()["reason"], "breakpoint");

    let first = setup.dispatch("debug.coroutines.list", json!({ "waitMs": 0 }));
    assert_eq!(first["pending"], true, "the first answer never waits: {first}");
    let beyond = coroutines(&mut setup);
    assert_eq!(beyond["unavailable"], "noFrame", "{beyond}");
    let asked = setup.adapter.requests("evaluate");
    let frames: Vec<i64> = asked.iter().filter_map(|asked| asked["frameId"].as_i64()).collect();
    assert_eq!(frames, (101..=108).collect::<Vec<_>>(), "at most the top 8 frames, each once");
    assert!(asked.iter().all(|asked| asked["expression"] == DUMP && asked["context"] == "watch"));

    setup.dispatch("debug.execution.resume", json!({}));
    assert_eq!(setup.get()["frame"]["name"], "AppKt$main$1.invokeSuspend");
    let listed = coroutines(&mut setup);
    assert_eq!(listed["frame"], "Main.lambda$0", "{listed}");
    assert_eq!(
        listed["coroutines"],
        json!([
            {
                "state": "RUNNING",
                "name": "root",
                "dispatcher": "BlockingEventLoop@1",
                "job": "\"root#1\":BlockingCoroutine{Active}@2",
                "frames": ["AppKt$main$1.invokeSuspend(App.kt:7)"],
            },
            {
                "state": "SUSPENDED",
                "name": "worker",
                "dispatcher": "Dispatchers.Default",
                "job": "\"worker#2\":StandaloneCoroutine{Active}@3",
                "frames": [],
            },
        ])
    );
    assert_eq!(setup.adapter.requests("evaluate").len(), 11, "the walk stops at the frame that reads the list");

    setup.dispatch("debug.execution.resume", json!({}));
    assert_eq!(setup.get()["frame"]["name"], "main.main");
    assert_eq!(coroutines(&mut setup)["unavailable"], "notJvm");
    setup.dispatch("debug.execution.pause", json!({}));
    let deadline = Instant::now() + Duration::from_secs(60);
    while setup.dispatch("debug.session.get", json!({}))["reason"] != "pause" {
        assert!(Instant::now() < deadline, "the pause did not arrive");
        std::thread::sleep(Duration::from_millis(10));
    }
    let paused = coroutines(&mut setup);
    assert_eq!(paused["unavailable"], "noEvent", "{paused}");
    assert!(paused["message"].as_str().unwrap().mentions("debug-coroutines-no-event"));
    assert_eq!(setup.adapter.requests("evaluate").len(), 11, "nothing is evaluated after a pause");
    setup.dispatch("debug.session.stop", json!({}));
}

#[test]
fn a_dump_that_throws_says_the_agent_is_missing_instead_of_listing_nothing() {
    let mut setup = Setup::fake();
    let java = setup.main.with_file_name("Main.java");
    setup.adapter.set_frames(vec![vec![frame(1, "Main.lambda$0", Some(&java), 5)]]);
    let thrown = "IllegalStateException@1304 \"java.lang.IllegalStateException: Debug probes are not installed\"";
    let dump = crate::java::coroutines::DUMP;
    setup.adapter.set_evaluations(vec![(dump, FakeVariable::new("", thrown, "IllegalStateException"))]);
    setup.program(&[3]);
    setup.breakpoint(2);
    setup.start();
    assert_eq!(setup.get()["reason"], "breakpoint");
    let answer = coroutines(&mut setup);
    assert_eq!(answer["unavailable"], "notInstalled", "{answer}");
    assert!(answer["message"].as_str().unwrap().mentions("debug-coroutines-not-installed"));
    assert_eq!(answer["coroutines"], json!([]));
    setup.dispatch("debug.session.stop", json!({}));
}
