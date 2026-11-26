use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;
use zen_dap::connection::Incoming;
use zen_dap::fake::{FakeAdapter, FakeVariable, InitializedOrder, program_path};
use zen_dap::protocol::{DebugEvent, OutputEvent, Thread};
use zen_dap::{
    Client, Configuration, Connection, DapError, DataBreakpoint, FunctionBreakpoint, ReverseListener,
    SetExceptionBreakpointsArguments, SourceBreakpoint, StackFrame, StepInTarget,
};

const WAIT: Duration = Duration::from_secs(5);

fn breakpoint(line: i64) -> SourceBreakpoint {
    SourceBreakpoint { line, ..SourceBreakpoint::default() }
}

fn scripted() -> (FakeAdapter, PathBuf) {
    let adapter = FakeAdapter::new();
    let main = program_path("main.go");
    adapter.set_program(&[(&main, 3), (&main, 4), (&main, 5), (&main, 7)]);
    (adapter, main)
}

fn events_until_stop(client: &Client) -> Vec<DebugEvent> {
    let mut events = Vec::new();
    while let Some(Incoming::Event(event)) = client.next_incoming(WAIT) {
        let done = matches!(event, DebugEvent::Stopped(_) | DebugEvent::Terminated);
        events.push(event);
        if done {
            break;
        }
    }
    events
}

#[test]
fn requests_get_their_own_answers() {
    let adapter = FakeAdapter::new();
    adapter.set_threads(vec![Thread { id: 7, name: "g7".to_owned() }]);
    let client = Client::new(adapter.connect_pipes().unwrap(), "fake");
    let threads = client.threads().unwrap();
    let unknown = client.request("goto", json!({ "threadId": 7 })).unwrap();
    assert!(unknown.request_seq() > threads.request_seq());
    assert!(matches!(unknown.wait(WAIT), Err(DapError::Adapter { message }) if message == "unknown command goto"));
    assert_eq!(threads.wait(WAIT).unwrap().threads, vec![Thread { id: 7, name: "g7".to_owned() }]);
}

#[test]
fn the_handshake_runs_in_order_when_initialized_comes_first() {
    let (adapter, main) = scripted();
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "go");
    let launch = json!({ "request": "launch", "mode": "exec", "program": "/tmp/bin" });
    let handshake = client.handshake(&launch, &[(main.clone(), vec![breakpoint(4), breakpoint(6)])], WAIT).unwrap();
    assert!(handshake.capabilities.supports_configuration_done_request);
    assert!(client.capabilities().supports_conditional_breakpoints);
    let set = &handshake.breakpoints[0];
    assert_eq!(set.0, main);
    assert_eq!(set.1.iter().map(|breakpoint| breakpoint.verified).collect::<Vec<_>>(), vec![true, false]);
    assert_eq!(
        adapter.commands(),
        ["initialize", "launch", "setBreakpoints", "setExceptionBreakpoints", "configurationDone"]
    );
    let initialize = &adapter.requests("initialize")[0];
    assert_eq!(initialize["clientID"], "zen");
    assert_eq!(initialize["adapterID"], "go");
    assert_eq!(adapter.requests("launch")[0], launch);
    assert_eq!(adapter.requests("setExceptionBreakpoints")[0], json!({ "filters": [] }));
    let events = events_until_stop(&client);
    let Some(DebugEvent::Stopped(stopped)) = events.last() else { panic!("no stop: {events:?}") };
    assert_eq!(stopped.reason, "breakpoint");
    assert_eq!(stopped.hit_breakpoint_ids, set.1[0].id.map(|id| vec![id]));
}

#[test]
fn the_handshake_waits_for_initialized_after_the_launch_response() {
    let (adapter, main) = scripted();
    adapter.set_initialized_order(InitializedOrder::AfterLaunchResponse);
    adapter.set_output(&[("stdout", "hello\n")]);
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "go");
    let handshake = client.handshake(&json!({}), &[(main, vec![breakpoint(5)])], WAIT).unwrap();
    assert!(handshake.breakpoints[0].1[0].verified);
    let events = events_until_stop(&client);
    assert_eq!(
        events.first(),
        Some(&DebugEvent::Output(OutputEvent { category: Some("stdout".to_owned()), output: "hello\n".to_owned() }))
    );
    assert!(matches!(events.last(), Some(DebugEvent::Stopped(_))));
}

#[test]
fn a_failed_launch_ends_the_handshake_with_the_adapters_message() {
    let adapter = FakeAdapter::new();
    adapter.fail_launch(Some("cannot find the program"));
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "go");
    let error = client.handshake(&json!({}), &[], WAIT).unwrap_err();
    assert_eq!(error, DapError::Adapter { message: "cannot find the program".to_owned() });
}

#[test]
fn requests_time_out_and_the_handshake_gives_up() {
    let adapter = FakeAdapter::new();
    adapter.set_silent("threads");
    adapter.set_silent("launch");
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "go");
    let threads = client.threads().unwrap();
    assert_eq!(threads.wait(Duration::from_millis(50)), Err(DapError::Timeout));
    assert!(threads.try_answer().is_none());
    assert_eq!(client.handshake(&json!({}), &[], Duration::from_millis(200)), Err(DapError::Timeout));
}

#[test]
fn reverse_requests_are_refused() {
    let adapter = FakeAdapter::new();
    adapter.set_reverse_request(Some("runInTerminal"));
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "go");
    client.handshake(&json!({}), &[], WAIT).unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    while adapter.responses().is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let response = &adapter.responses()[0];
    assert_eq!(response["command"], "runInTerminal");
    assert_eq!(response["success"], false);
    assert_eq!(response["message"], "not supported");
}

#[test]
fn closing_answers_every_pending_request() {
    let adapter = FakeAdapter::new();
    adapter.set_silent("threads");
    let client = Client::new(adapter.connect_pipes().unwrap(), "go");
    let threads = client.threads().unwrap();
    client.disconnect(true).unwrap().wait(WAIT).unwrap();
    assert_eq!(threads.wait(WAIT), Err(DapError::Closed));
    let mut closed = false;
    while let Some(incoming) = client.next_incoming(WAIT) {
        if incoming == Incoming::Closed {
            closed = true;
            break;
        }
    }
    assert!(closed);
    assert!(!client.is_alive());
    assert!(matches!(client.threads(), Err(DapError::Closed)));
}

#[test]
fn a_session_runs_end_to_end_over_the_reverse_connection() {
    let (adapter, main) = scripted();
    adapter.set_exit_code(3);
    adapter.set_variables(vec![
        FakeVariable::new("n", "42", "int"),
        FakeVariable::new("p", "main.Point {X: 1, Y: 2}", "main.Point")
            .with_children(vec![FakeVariable::new("X", "1", "int"), FakeVariable::new("Y", "2", "int")]),
    ]);
    adapter.set_frames(vec![vec![
        StackFrame { id: 1, name: "main.add".to_owned(), source: None, line: 4, column: 2, ..StackFrame::default() },
        StackFrame { id: 2, name: "main.main".to_owned(), source: None, line: 9, column: 1, ..StackFrame::default() },
    ]]);
    let listener = ReverseListener::bind().unwrap();
    let adapter_thread = adapter.connect(listener.addr()).unwrap();
    let stream = listener.accept(WAIT).unwrap();
    let mut client = Client::new(Connection::tcp(stream).unwrap(), "go");
    client.handshake(&json!({ "mode": "exec" }), &[(main.clone(), vec![breakpoint(4), breakpoint(7)])], WAIT).unwrap();
    assert!(matches!(events_until_stop(&client).last(), Some(DebugEvent::Stopped(_))));

    let trace = client.stack_trace(1, Some(0), Some(1)).unwrap().wait(WAIT).unwrap();
    assert_eq!(trace.total_frames, Some(2));
    assert_eq!(trace.stack_frames.iter().map(|frame| frame.name.as_str()).collect::<Vec<_>>(), ["main.add"]);
    let scopes = client.scopes(1).unwrap().wait(WAIT).unwrap().scopes;
    assert_eq!(scopes[0].name, "Locals");
    let locals = client.variables(scopes[0].variables_reference, None, None).unwrap().wait(WAIT).unwrap().variables;
    assert_eq!(locals.iter().map(|variable| variable.name.as_str()).collect::<Vec<_>>(), ["n", "p"]);
    assert_eq!(locals[1].named_variables, Some(2));
    let fields = client.variables(locals[1].variables_reference, Some(1), Some(1)).unwrap().wait(WAIT).unwrap();
    assert_eq!(fields.variables[0].evaluate_name.as_deref(), Some("p.Y"));
    let evaluated = client.evaluate("p.X", Some(1), Some("watch")).unwrap().wait(WAIT).unwrap();
    assert_eq!((evaluated.result.as_str(), evaluated.type_.as_deref()), ("1", Some("int")));
    assert!(matches!(client.evaluate("q", Some(1), None).unwrap().wait(WAIT), Err(DapError::Adapter { .. })));

    client.next(1).unwrap().wait(WAIT).unwrap();
    let Some(DebugEvent::Stopped(step)) = events_until_stop(&client).pop() else { panic!("no step") };
    assert_eq!(step.reason, "step");
    let frames = client.stack_trace(1, None, None).unwrap().wait(WAIT).unwrap().stack_frames;
    assert_eq!(
        (frames[0].line, frames[0].source.as_ref().and_then(|source| source.path.clone())),
        (5, Some(main.to_string_lossy().into_owned()))
    );

    client.continue_(1).unwrap().wait(WAIT).unwrap();
    let Some(DebugEvent::Stopped(second)) = events_until_stop(&client).pop() else { panic!("no second stop") };
    assert_eq!(second.reason, "breakpoint");
    client.continue_(1).unwrap().wait(WAIT).unwrap();
    let events = events_until_stop(&client);
    assert_eq!(
        &events[events.len() - 2..],
        [DebugEvent::Exited(zen_dap::protocol::ExitedEvent { exit_code: 3 }), DebugEvent::Terminated]
    );

    client.disconnect(true).unwrap().wait(WAIT).unwrap();
    adapter_thread.join().unwrap();
    assert_eq!(adapter.requests("disconnect")[0], json!({ "terminateDebuggee": true }));
}

struct ByParity;

impl zen_dap::SourceMap for ByParity {
    fn split(&self, path: &std::path::Path, breakpoints: &[SourceBreakpoint]) -> Option<Vec<(PathBuf, Vec<usize>)>> {
        if path.extension().is_none_or(|extension| extension != "kt") {
            return None;
        }
        let (even, odd): (Vec<usize>, Vec<usize>) =
            (0..breakpoints.len()).partition(|index| breakpoints[*index].line % 2 == 0);
        let parts = [(program_path("Even.java"), even), (program_path("Odd.java"), odd)];
        Some(parts.into_iter().filter(|(_, indices)| !indices.is_empty()).collect())
    }
}

#[test]
fn a_source_map_sends_breakpoints_for_other_files_and_answers_for_the_asked_one() {
    let adapter = FakeAdapter::new();
    adapter.set_verified(Some(vec![3, 4, 5]));
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "java");
    client.set_source_map(Some(std::sync::Arc::new(ByParity)));
    let kotlin = program_path("App.kt");
    let set = client.put_breakpoints(&kotlin, &[breakpoint(3), breakpoint(4), breakpoint(8)], WAIT).unwrap();
    let answered: Vec<(Option<i64>, bool, Option<String>)> = set
        .iter()
        .map(|breakpoint| {
            (breakpoint.line, breakpoint.verified, breakpoint.source.as_ref().and_then(|source| source.path.clone()))
        })
        .collect();
    let path = Some(kotlin.to_string_lossy().into_owned());
    assert_eq!(answered, [(Some(3), true, path.clone()), (Some(4), true, path.clone()), (Some(8), false, path)]);
    let sent: Vec<(String, usize)> = adapter
        .requests("setBreakpoints")
        .iter()
        .map(|request| {
            let file = request["source"]["path"].as_str().unwrap().rsplit('/').next().unwrap().to_owned();
            (file, request["breakpoints"].as_array().unwrap().len())
        })
        .collect();
    assert_eq!(sent, [("Even.java".to_owned(), 2), ("Odd.java".to_owned(), 1)]);

    client.put_breakpoints(&kotlin, &[breakpoint(4)], WAIT).unwrap();
    let sent = adapter.requests("setBreakpoints");
    assert_eq!(sent.len(), 4, "the odd file is cleared");
    assert!(sent[2]["source"]["path"].as_str().unwrap().ends_with("Odd.java"));
    assert_eq!(sent[2]["breakpoints"], json!([]));
    let go = program_path("main.go");
    client.put_breakpoints(&go, &[breakpoint(5)], WAIT).unwrap();
    assert_eq!(adapter.requests("setBreakpoints")[4]["source"]["path"], json!(go.to_string_lossy()));
}
#[test]
fn the_configuration_follows_what_the_adapter_offers() {
    let (adapter, main) = scripted();
    adapter.set_capabilities(json!({
        "supportsFunctionBreakpoints": true,
        "exceptionBreakpointFilters": [{ "filter": "uncaught", "label": "Uncaught", "supportsCondition": true }],
    }));
    adapter.set_functions(&[("main.run", 1)]);
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "go");
    let handshake = client
        .configure(
            &json!({}),
            |capabilities| Configuration {
                breakpoints: vec![(main.clone(), vec![breakpoint(4)])],
                functions: capabilities
                    .supports_function_breakpoints
                    .then(|| FunctionBreakpoint { name: "main.run".to_owned(), ..FunctionBreakpoint::default() })
                    .into_iter()
                    .collect(),
                exceptions: SetExceptionBreakpointsArguments {
                    filters: capabilities
                        .exception_breakpoint_filters
                        .iter()
                        .map(|filter| filter.filter.clone())
                        .collect(),
                    filter_options: None,
                },
                data: vec![DataBreakpoint { data_id: "1/x".to_owned(), ..DataBreakpoint::default() }],
            },
            WAIT,
        )
        .unwrap();
    assert!(handshake.capabilities.exception_breakpoint_filters[0].supports_condition);
    assert_eq!(handshake.functions.len(), 1);
    assert!(handshake.data.is_empty(), "no data breakpoints without supportsDataBreakpoints");
    assert_eq!(adapter.requests("setExceptionBreakpoints")[0]["filters"], json!(["uncaught"]));
    assert!(adapter.requests("setDataBreakpoints").is_empty());
    let commands = adapter.commands();
    let order: Vec<&str> = commands
        .iter()
        .map(String::as_str)
        .filter(|command| command.starts_with("set") || *command == "configurationDone")
        .collect();
    assert_eq!(order, ["setBreakpoints", "setFunctionBreakpoints", "setExceptionBreakpoints", "configurationDone"]);
}

#[test]
fn step_in_targets_a_target_step_and_a_restarted_frame_reach_an_adapter_that_has_them() {
    let (adapter, main) = scripted();
    adapter.set_capabilities(json!({ "supportsStepInTargetsRequest": true, "supportsRestartFrame": true }));
    let target =
        StepInTarget { id: 7, label: "twice".to_owned(), line: Some(3), column: Some(9), ..StepInTarget::default() };
    adapter.set_step_in_targets(vec![target.clone()]);
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "go");
    let handshake = client.handshake(&json!({}), &[(main.clone(), vec![breakpoint(3)])], WAIT).unwrap();
    assert!(handshake.capabilities.supports_step_in_targets_request && handshake.capabilities.supports_restart_frame);
    assert!(matches!(events_until_stop(&client).last(), Some(DebugEvent::Stopped(_))));
    let frame = client.stack_trace(1, None, None).unwrap().wait(WAIT).unwrap().stack_frames[0].id;
    assert_eq!(client.step_in_targets(frame).unwrap().wait(WAIT).unwrap().targets, [target]);
    client.step_into_target(1, 7).unwrap().wait(WAIT).unwrap();
    assert_eq!(adapter.requests("stepIn"), [json!({ "threadId": 1, "targetId": 7 })]);
    assert!(matches!(events_until_stop(&client).last(), Some(DebugEvent::Stopped(_))));
    let frame = client.stack_trace(1, None, None).unwrap().wait(WAIT).unwrap().stack_frames[0].id;
    client.restart_frame(frame).unwrap().wait(WAIT).unwrap();
    let Some(DebugEvent::Stopped(stopped)) = events_until_stop(&client).pop() else {
        panic!("a stop after the restart")
    };
    assert_eq!(stopped.reason, "restart");
    assert!(matches!(client.restart_frame(99).unwrap().wait(WAIT), Err(DapError::Adapter { .. })));

    let (adapter, main) = scripted();
    let mut client = Client::new(adapter.connect_pipes().unwrap(), "go");
    let handshake = client.handshake(&json!({}), &[(main, vec![breakpoint(3)])], WAIT).unwrap();
    assert!(!handshake.capabilities.supports_step_in_targets_request && !handshake.capabilities.supports_restart_frame);
    assert!(matches!(client.step_in_targets(1000).unwrap().wait(WAIT), Err(DapError::Adapter { .. })));
}

#[test]
#[ignore = "needs lldb-dap (Xcode or the Command Line Tools) and rustc"]
fn lldb_dap_takes_log_points_function_breakpoints_and_hit_conditions() {
    let dir = std::env::temp_dir().join(format!("zen-lldb-dap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs");
    std::fs::write(&source, "fn step(n: u32) -> u32 {\n    n + 1\n}\n\nfn main() {\n    let mut total = 0;\n    for n in 0..5 {\n        total = step(total + n);\n    }\n    println!(\"{total}\");\n}\n").unwrap();
    let program = dir.join("main");
    let built = std::process::Command::new("rustc").args(["-g", "-o"]).arg(&program).arg(&source).status().unwrap();
    assert!(built.success());
    let found = std::process::Command::new("xcrun").args(["-f", "lldb-dap"]).output().unwrap();
    let lldb = String::from_utf8(found.stdout).unwrap().trim().to_owned();
    let mut child = std::process::Command::new(lldb)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let connection = Connection::new(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let mut client = Client::new(connection, "lldb-dap");
    let launch = json!({ "program": program, "cwd": dir });
    let handshake = client
        .configure(
            &launch,
            |capabilities| Configuration {
                breakpoints: vec![(
                    source.clone(),
                    vec![
                        SourceBreakpoint { line: 2, log_message: Some("n is {n}".to_owned()), ..breakpoint(2) },
                        SourceBreakpoint { hit_condition: Some("3".to_owned()), ..breakpoint(8) },
                    ],
                )],
                functions: capabilities
                    .supports_function_breakpoints
                    .then(|| FunctionBreakpoint { name: "nothing_here".to_owned(), ..FunctionBreakpoint::default() })
                    .into_iter()
                    .collect(),
                ..Configuration::default()
            },
            Duration::from_secs(60),
        )
        .unwrap();
    let capabilities = &handshake.capabilities;
    assert!(capabilities.supports_log_points && capabilities.supports_hit_conditional_breakpoints);
    assert!(handshake.breakpoints[0].1.iter().all(|set| set.verified), "{:?}", handshake.breakpoints);
    let mut logged = Vec::new();
    let mut stopped = None;
    while let Some(Incoming::Event(event)) = client.next_incoming(Duration::from_secs(30)) {
        match event {
            DebugEvent::Output(OutputEvent { output, .. }) => logged.push(output),
            DebugEvent::Stopped(event) => {
                stopped = Some(event);
                break;
            }
            DebugEvent::Terminated => break,
            _ => {}
        }
    }
    let stopped = stopped.expect("the hit condition stops the program");
    let thread = stopped.thread_id.unwrap();
    let frames = client.stack_trace(thread, None, None).unwrap().wait(WAIT).unwrap().stack_frames;
    assert_eq!(frames[0].line, 8);
    let logs: Vec<&String> = logged.iter().filter(|line| line.starts_with("n is")).collect();
    assert_eq!(logs.len(), 2, "the log point ran twice before the third hit: {logged:?}");
    let _ = client.disconnect(true).and_then(|pending| pending.wait(WAIT));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "needs lldb-dap (Xcode or the Command Line Tools) and rustc"]
fn lldb_dap_evaluates_for_hovers_and_reads_a_values_fields() {
    let dir = std::env::temp_dir().join(format!("zen-lldb-hover-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs");
    std::fs::write(&source, "struct Point {\n    x: i32,\n    y: i32,\n}\n\nfn main() {\n    let point = Point { x: 3, y: 4 };\n    let total = point.x + point.y;\n    println!(\"{total}\");\n}\n").unwrap();
    let program = dir.join("main");
    let built = std::process::Command::new("rustc").args(["-g", "-o"]).arg(&program).arg(&source).status().unwrap();
    assert!(built.success());
    let found = std::process::Command::new("xcrun").args(["-f", "lldb-dap"]).output().unwrap();
    let lldb = String::from_utf8(found.stdout).unwrap().trim().to_owned();
    let mut child = std::process::Command::new(lldb)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let connection = Connection::new(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let mut client = Client::new(connection, "lldb-dap");
    let launch = json!({ "program": program, "cwd": dir });
    let handshake = client
        .configure(
            &launch,
            |_| Configuration { breakpoints: vec![(source.clone(), vec![breakpoint(9)])], ..Configuration::default() },
            Duration::from_secs(60),
        )
        .unwrap();
    assert!(handshake.capabilities.supports_evaluate_for_hovers);
    let mut stopped = None;
    while let Some(Incoming::Event(event)) = client.next_incoming(Duration::from_secs(30)) {
        match event {
            DebugEvent::Stopped(event) => {
                stopped = Some(event);
                break;
            }
            DebugEvent::Terminated => break,
            _ => {}
        }
    }
    let thread = stopped.expect("the breakpoint stops the program").thread_id.unwrap();
    let frame = client.stack_trace(thread, None, None).unwrap().wait(WAIT).unwrap().stack_frames[0].id;
    let total = client.evaluate("total", Some(frame), Some("hover")).unwrap().wait(WAIT).unwrap();
    assert_eq!(total.result, "7");
    let point = client.evaluate("point", Some(frame), Some("hover")).unwrap();
    let point = loop {
        if let Some(answer) = point.try_answer() {
            break answer.unwrap();
        }
        assert!(client.is_alive());
        std::thread::yield_now();
    };
    assert!(point.variables_reference > 0, "{point:?}");
    let fields = client.variables(point.variables_reference, None, None).unwrap().wait(WAIT).unwrap().variables;
    let shown: Vec<(&str, &str)> = fields.iter().map(|field| (field.name.as_str(), field.value.as_str())).collect();
    assert_eq!(shown, [("x", "3"), ("y", "4")]);
    let _ = client.disconnect(true).and_then(|pending| pending.wait(WAIT));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "needs lldb-dap (Xcode or the Command Line Tools) and rustc"]
fn lldb_dap_sets_a_local_variable() {
    let dir = std::env::temp_dir().join(format!("zen-lldb-set-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs");
    std::fs::write(&source, "fn main() {\n    let total = std::hint::black_box(7);\n    println!(\"{total}\");\n}\n")
        .unwrap();
    let program = dir.join("main");
    let built = std::process::Command::new("rustc").args(["-g", "-o"]).arg(&program).arg(&source).status().unwrap();
    assert!(built.success());
    let found = std::process::Command::new("xcrun").args(["-f", "lldb-dap"]).output().unwrap();
    let lldb = String::from_utf8(found.stdout).unwrap().trim().to_owned();
    let mut child = std::process::Command::new(lldb)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let connection = Connection::new(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let mut client = Client::new(connection, "lldb-dap");
    let launch = json!({ "program": program, "cwd": dir });
    let handshake = client
        .configure(
            &launch,
            |_| Configuration { breakpoints: vec![(source.clone(), vec![breakpoint(3)])], ..Configuration::default() },
            Duration::from_secs(60),
        )
        .unwrap();
    assert!(handshake.capabilities.supports_set_variable);
    let mut stopped = None;
    while let Some(Incoming::Event(event)) = client.next_incoming(Duration::from_secs(30)) {
        match event {
            DebugEvent::Stopped(event) => {
                stopped = Some(event);
                break;
            }
            DebugEvent::Terminated => break,
            _ => {}
        }
    }
    let thread = stopped.expect("the breakpoint stops the program").thread_id.unwrap();
    let frame = client.stack_trace(thread, None, None).unwrap().wait(WAIT).unwrap().stack_frames[0].id;
    let scopes = client.scopes(frame).unwrap().wait(WAIT).unwrap().scopes;
    let locals = scopes.first().expect("a scope").variables_reference;
    let set = client.set_variable(locals, "total", "41").unwrap().wait(WAIT).unwrap();
    assert_eq!(set.value, "41");
    let total = client.evaluate("total", Some(frame), Some("watch")).unwrap().wait(WAIT).unwrap();
    assert_eq!(total.result, "41");
    let _ = client.disconnect(true).and_then(|pending| pending.wait(WAIT));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "needs lldb-dap (Xcode or the Command Line Tools) and rustc"]
fn lldb_dap_offers_neither_step_in_targets_nor_restart_frame_and_no_async_frames() {
    let dir = std::env::temp_dir().join(format!("zen-lldb-smart-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs");
    std::fs::write(
        &source,
        "fn twice(n: i32) -> i32 {\n    n * 2\n}\n\nfn main() {\n    let total = twice(std::hint::black_box(3)) + 1;\n    println!(\"{total}\");\n}\n",
    )
    .unwrap();
    let program = dir.join("main");
    let built = std::process::Command::new("rustc").args(["-g", "-o"]).arg(&program).arg(&source).status().unwrap();
    assert!(built.success());
    let found = std::process::Command::new("xcrun").args(["-f", "lldb-dap"]).output().unwrap();
    let lldb = String::from_utf8(found.stdout).unwrap().trim().to_owned();
    let mut child = std::process::Command::new(lldb)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let connection = Connection::new(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let mut client = Client::new(connection, "lldb-dap");
    let launch = json!({ "program": program, "cwd": dir });
    let handshake = client
        .configure(
            &launch,
            |_| Configuration { breakpoints: vec![(source.clone(), vec![breakpoint(2)])], ..Configuration::default() },
            Duration::from_secs(60),
        )
        .unwrap();
    assert!(!handshake.capabilities.supports_step_in_targets_request, "the IDE hides Smart Step Into");
    assert!(!handshake.capabilities.supports_restart_frame, "the IDE hides Reset Frame and never sends restartFrame");
    let mut stopped = None;
    while let Some(Incoming::Event(event)) = client.next_incoming(Duration::from_secs(30)) {
        match event {
            DebugEvent::Stopped(event) => {
                stopped = Some(event);
                break;
            }
            DebugEvent::Terminated => break,
            _ => {}
        }
    }
    let thread = stopped.expect("the breakpoint stops the program").thread_id.unwrap();
    let frames = client.stack_trace(thread, None, None).unwrap().wait(WAIT).unwrap().stack_frames;
    assert!(frames.len() >= 2 && frames[0].name.contains("twice"), "{frames:?}");
    assert!(frames.iter().all(|frame| frame.presentation_hint.as_deref() != Some("label")), "no async separators");
    let _ = client.disconnect(true).and_then(|pending| pending.wait(WAIT));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
