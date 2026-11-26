use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;
use zen_dap::{
    Client, Connection, DebugEvent, Handshake, Incoming, ReverseListener, SourceBreakpoint, StackFrame, StoppedEvent,
};
use zen_lang_go::GoRun;
use zen_run::{
    CommandLine, Configuration, DebugPlan, Module, RunContext, RunEnv, RunOutput, RunProvider, RunState, Stream,
};

const MAIN: &str = "package main\n\nimport \"fmt\"\n\nfunc main() {\n\ttotal := 0\n\tfor i := 1; i <= 3; i++ {\n\t\ttotal += i\n\t}\n\tfmt.Println(\"total\", total)\n}\n";

const CALC_TEST: &str = "package calc\n\nimport \"testing\"\n\nfunc TestOk(t *testing.T) {\n\tt.Run(\"sub\", func(t *testing.T) {})\n}\n\nfunc TestBad(t *testing.T) {\n\tt.Errorf(\"got %d\", 2)\n}\n\nfunc TestSkip(t *testing.T) {\n\tt.Skip(\"later\")\n}\n";

const TIMEOUT: Duration = Duration::from_secs(60);

fn on_path(name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(extra.iter().cloned())
        .map(|folder| folder.join(&file))
        .find(|candidate| candidate.is_file())
}

fn find_go() -> Option<PathBuf> {
    on_path("go", &["/opt/homebrew/bin", "/usr/local/go/bin", "/usr/local/bin"].map(PathBuf::from))
}

fn find_dlv() -> Option<PathBuf> {
    let mut extra: Vec<PathBuf> = std::env::var_os("GOBIN").map(PathBuf::from).into_iter().collect();
    if let Some(gopath) = std::env::var_os("GOPATH") {
        extra.extend(std::env::split_paths(&gopath).map(|folder| folder.join("bin")));
    }
    if let Some(home) = std::env::var_os("HOME") {
        extra.push(Path::new(&home).join("go/bin"));
    }
    on_path("dlv", &extra)
}

struct Project {
    temp: tempfile::TempDir,
    root: PathBuf,
}

fn project(go: PathBuf, debugger: PathBuf) -> (Project, RunContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("calc")).unwrap();
    std::fs::write(root.join("go.mod"), "module example.com/dbg\n\ngo 1.21\n").unwrap();
    std::fs::write(root.join("main.go"), MAIN).unwrap();
    std::fs::write(root.join("calc/calc_test.go"), CALC_TEST).unwrap();
    let cache = |name: &str| temp.path().join(name).to_string_lossy().into_owned();
    let env: Vec<(String, String)> = [
        ("GOPATH", cache("gopath")),
        ("GOCACHE", cache("gocache")),
        ("GOMODCACHE", cache("modcache")),
        ("GOFLAGS", "-mod=mod".to_owned()),
        ("GOPROXY", "off".to_owned()),
        ("GOTOOLCHAIN", "local".to_owned()),
        ("GOTELEMETRY", "off".to_owned()),
        ("GOENV", "off".to_owned()),
        ("GOWORK", "off".to_owned()),
    ]
    .map(|(name, value)| (name.to_owned(), value))
    .into();
    let scratch = temp.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let context = RunContext {
        root: root.clone(),
        env: RunEnv { tool: go, env },
        modules: vec![Module { name: "example.com/dbg".to_owned(), root: root.clone(), ..Module::default() }],
        scratch,
        debugger,
    };
    (Project { temp, root }, context)
}

fn configuration(kind: &str, options: serde_json::Value) -> Configuration {
    Configuration { name: "c".to_owned(), kind: kind.to_owned(), options, ..Configuration::default() }
}

fn command(line: &CommandLine) -> Command {
    let mut command = Command::new(&line.program);
    command.args(&line.args).current_dir(&line.cwd).envs(line.env.iter().map(|(name, value)| (name, value)));
    command
}

fn build(line: &CommandLine) {
    let result = command(line).output().unwrap();
    assert!(result.status.success(), "{} failed: {}", line.display, String::from_utf8_lossy(&result.stderr));
}

#[test]
fn every_go_kind_has_a_plan_and_other_kinds_have_none() {
    let context = RunContext { env: RunEnv { tool: PathBuf::from("go"), env: Vec::new() }, ..RunContext::default() };
    assert!(GoRun.debug(&configuration("go.build", json!({ "package": "./a" })), &context).unwrap().is_ok());
    assert!(GoRun.debug(&configuration("go.test", json!({ "package": "./a" })), &context).unwrap().is_ok());
    assert!(GoRun.debug(&configuration("cargo.run", json!({})), &context).is_none());
}

#[test]
fn the_test_parser_reads_a_real_verbose_test_binary() {
    let Some(go) = find_go() else { return };
    let (project, context) = project(go, PathBuf::new());
    let tests = configuration("go.test", json!({ "package": "./calc" }));
    let plan = GoRun.debug(&tests, &context).unwrap().unwrap();
    build(&plan.build[0]);
    let program = PathBuf::from(plan.launch["program"].as_str().unwrap());
    let args: Vec<String> = serde_json::from_value(plan.launch["args"].clone()).unwrap();
    let result = Command::new(&program).args(&args).current_dir(plan.launch["cwd"].as_str().unwrap()).output().unwrap();
    let mut output = RunOutput::new(GoRun.parser(&tests, &context));
    for text in String::from_utf8_lossy(&result.stdout).lines() {
        output.feed(Stream::Stdout, text);
    }
    output.end(RunState::Finished, result.status.code());
    let counts = output.tree().counts();
    assert_eq!((counts.passed, counts.failed, counts.skipped), (2, 1, 1), "{:?}", output.console().lines());
    let error = output.console().lines().iter().find(|line| line.text.contains("got 2")).unwrap();
    assert_eq!(error.links[0].file, project.root.join("calc/calc_test.go"));
    drop(project.temp);
}

fn stopped(client: &Client, deadline: Instant) -> StoppedEvent {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "Delve did not stop in time");
        match client.next_incoming(left) {
            Some(Incoming::Event(DebugEvent::Stopped(event))) => return event,
            Some(Incoming::Closed) => panic!("Delve closed the connection"),
            _ => {}
        }
    }
}

fn local(client: &Client, thread: i64, name: &str) -> String {
    let frames = client.stack_trace(thread, None, Some(1)).unwrap().wait(TIMEOUT).unwrap();
    let frame = &frames.stack_frames[0];
    let scopes = client.scopes(frame.id).unwrap().wait(TIMEOUT).unwrap();
    let variables = client.variables(scopes.scopes[0].variables_reference, None, None).unwrap().wait(TIMEOUT).unwrap();
    variables.variables.into_iter().find(|variable| variable.name == name).map(|variable| variable.value).unwrap()
}

struct Session {
    child: Child,
    client: Client,
    handshake: Handshake,
}

fn start(plan: &DebugPlan, breakpoints: &[(PathBuf, Vec<SourceBreakpoint>)]) -> Option<Session> {
    build(&plan.build[0]);
    let listener = ReverseListener::bind().unwrap();
    let mut adapter = command(&plan.adapter.line);
    adapter.arg(format!("{}={}", plan.adapter.address_flag, listener.addr()));
    let mut child = adapter.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let stream = listener.accept(TIMEOUT).unwrap();
    let mut client = Client::new(Connection::tcp(stream).unwrap(), "go");
    match client.handshake(&plan.launch, breakpoints, TIMEOUT) {
        Ok(handshake) => Some(Session { child, client, handshake }),
        Err(zen_dap::DapError::Adapter { message }) if zen_lang_go::explain_adapter_error(&message).is_some() => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
        Err(error) => panic!("handshake failed: {error}"),
    }
}

fn finish(session: Session) {
    let Session { mut child, client, .. } = session;
    let _ = client.disconnect(true).map(|pending| pending.wait(Duration::from_secs(5)));
    client.close();
    let quit = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() && Instant::now() < quit {
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn top_frame(client: &Client, thread: i64) -> StackFrame {
    client.stack_trace(thread, None, Some(1)).unwrap().wait(TIMEOUT).unwrap().stack_frames.remove(0)
}

#[test]
fn delve_stops_at_a_breakpoint_steps_and_runs_to_the_end() {
    let (Some(go), Some(dlv)) = (find_go(), find_dlv()) else { return };
    let (project, context) = project(go, dlv);
    let main = configuration("go.build", json!({ "package": "." }));
    let plan = GoRun.debug(&main, &context).unwrap().unwrap();
    let file = project.root.join("main.go");
    let breakpoints = [(file, vec![SourceBreakpoint { line: 8, ..SourceBreakpoint::default() }])];
    let Some(session) = start(&plan, &breakpoints) else { return };
    let client = &session.client;
    assert!(session.handshake.breakpoints[0].1.iter().all(|breakpoint| breakpoint.verified), "{:?}", session.handshake);
    let deadline = Instant::now() + TIMEOUT;
    let stop = stopped(client, deadline);
    assert_eq!(stop.reason, "breakpoint");
    let thread = stop.thread_id.unwrap();
    assert_eq!(local(client, thread, "total"), "0");
    client.next(thread).unwrap().wait(TIMEOUT).unwrap();
    let step = stopped(client, deadline);
    assert_eq!(step.reason, "step");
    assert_eq!(local(client, step.thread_id.unwrap_or(thread), "total"), "1");
    client.set_breakpoints(&project.root.join("main.go").to_string_lossy(), &[]).unwrap().wait(TIMEOUT).unwrap();
    client.continue_(thread).unwrap().wait(TIMEOUT).unwrap();
    let mut ended = false;
    while !ended && Instant::now() < deadline {
        match client.next_incoming(Duration::from_millis(200)) {
            Some(Incoming::Event(DebugEvent::Exited(exited))) => {
                assert_eq!(exited.exit_code, 0);
                ended = true;
            }
            Some(Incoming::Event(DebugEvent::Terminated) | Incoming::Closed) => ended = true,
            _ => {}
        }
    }
    assert!(ended, "the program did not end");
    finish(session);
    drop(project.temp);
}

#[test]
fn delve_stops_only_where_a_condition_holds_and_evaluates_go_expressions() {
    let (Some(go), Some(dlv)) = (find_go(), find_dlv()) else { return };
    let (project, context) = project(go, dlv);
    let main = configuration("go.build", json!({ "package": "." }));
    let plan = GoRun.debug(&main, &context).unwrap().unwrap();
    let conditional = SourceBreakpoint { line: 8, condition: Some("i == 3".to_owned()), ..SourceBreakpoint::default() };
    let breakpoints = [(project.root.join("main.go"), vec![conditional])];
    let Some(session) = start(&plan, &breakpoints) else { return };
    let client = &session.client;
    assert!(session.handshake.capabilities.supports_conditional_breakpoints);
    assert!(session.handshake.breakpoints[0].1[0].verified, "{:?}", session.handshake);
    let stop = stopped(client, Instant::now() + TIMEOUT);
    let thread = stop.thread_id.unwrap();
    assert_eq!(local(client, thread, "i"), "3");
    assert_eq!(local(client, thread, "total"), "3");
    let frame = top_frame(client, thread);
    for evaluation in [None, Some("watch"), Some("hover"), Some("repl")] {
        let sum = client.evaluate("total + i*2", Some(frame.id), evaluation).unwrap().wait(TIMEOUT).unwrap();
        assert_eq!(sum.result, "9", "{evaluation:?}");
    }
    let unknown = client.evaluate("missing", Some(frame.id), None).unwrap().wait(TIMEOUT);
    assert!(unknown.is_err(), "{unknown:?}");
    finish(session);
    drop(project.temp);
}
