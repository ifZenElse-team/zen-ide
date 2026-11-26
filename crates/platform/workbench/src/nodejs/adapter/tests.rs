use std::io::BufReader;
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_dap::ReverseListener;
use zen_dap::connection::{read_message, write_message};

use super::*;

const WAIT: Duration = Duration::from_secs(60);

fn node() -> PathBuf {
    zen_lsp::servers::find_executable("node", &zen_lsp::servers::search_folders())
        .expect("Node.js is needed for the debugger tests")
}

struct Ide {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    seq: i64,
    events: Vec<Value>,
    process: Arc<dyn AdapterProcess>,
}

impl Ide {
    fn start() -> Self {
        let listener = ReverseListener::bind().unwrap();
        let process = launch(&[], listener.addr()).unwrap();
        let stream = listener.accept(WAIT).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        Self {
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
            seq: 0,
            events: Vec::new(),
            process,
        }
    }

    fn send(&mut self, command: &str, arguments: Value) -> i64 {
        self.seq += 1;
        let message = json!({ "seq": self.seq, "type": "request", "command": command, "arguments": arguments });
        write_message(&mut self.writer, &message).unwrap();
        self.seq
    }

    fn request(&mut self, command: &str, arguments: Value) -> Value {
        let seq = self.send(command, arguments);
        loop {
            let message = read_message(&mut self.reader).unwrap().expect("the adapter answers");
            if message["type"] == "event" {
                self.events.push(message);
            } else if message["request_seq"] == seq {
                assert_eq!(message["success"], true, "{command}: {message}");
                return message["body"].clone();
            }
        }
    }

    fn event(&mut self, name: &str) -> Value {
        if let Some(at) = self.events.iter().position(|event| event["event"] == name) {
            return self.events.remove(at)["body"].clone();
        }
        let deadline = Instant::now() + WAIT;
        loop {
            assert!(Instant::now() < deadline, "no {name} event: {:?}", self.events);
            let message = read_message(&mut self.reader).unwrap().expect("the adapter sends events");
            if message["event"] == name {
                return message["body"].clone();
            }
            self.events.push(message);
        }
    }

    fn started(&mut self, file: &Path, lines: &[Value]) -> Value {
        self.request("initialize", json!({ "adapterID": "node" }));
        let cwd = file.parent().unwrap();
        self.request("launch", json!({ "program": node(), "args": [file], "cwd": cwd, "env": [["ZEN_TEST", "1"]] }));
        self.event("initialized");
        let answer = self.request("setBreakpoints", json!({ "source": { "path": file }, "breakpoints": lines }));
        self.request("setExceptionBreakpoints", json!({ "filters": [] }));
        self.request("configurationDone", json!({}));
        answer
    }

    fn top_frame(&mut self) -> Value {
        self.request("stackTrace", json!({ "threadId": 1 }))["stackFrames"][0].clone()
    }

    fn locals(&mut self, frame: &Value) -> Vec<Value> {
        let scopes = self.request("scopes", json!({ "frameId": frame["id"] }));
        let reference = scopes["scopes"][0]["variablesReference"].clone();
        self.request("variables", json!({ "variablesReference": reference }))["variables"].as_array().unwrap().clone()
    }
}

fn script(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.canonicalize().unwrap().join(name);
    std::fs::write(&path, text).unwrap();
    path
}

const PROGRAM: &str = "function add(a, b) {
  let sum = a + b;
  return sum;
}
const shop = { name: 'zen', items: [1, 2] };
console.log('before');
const total = add(2, 3);
console.log('total', total, shop.name, process.env.ZEN_TEST);
";

fn value_of<'a>(variables: &'a [Value], name: &str) -> &'a Value {
    variables.iter().find(|variable| variable["name"] == name).unwrap_or_else(|| panic!("{name}: {variables:?}"))
}

#[test]
#[ignore = "needs Node.js on PATH (docs/development.md)"]
fn a_breakpoint_stops_the_program_and_its_frames_variables_and_steps_work() {
    let dir = tempfile::tempdir().unwrap();
    let file = script(dir.path(), "main.js", PROGRAM);
    let mut ide = Ide::start();
    let answer = ide.started(&file, &[json!({ "line": 2 })]);
    assert_eq!(answer["breakpoints"][0]["line"], 2);

    let stopped = ide.event("stopped");
    assert_eq!(stopped["reason"], "breakpoint", "{stopped}");
    ide.request("next", json!({ "threadId": 1 }));
    assert_eq!(ide.event("stopped")["reason"], "step");
    let frame = ide.top_frame();
    assert_eq!((frame["name"].as_str(), frame["line"].as_i64()), (Some("add"), Some(3)));
    assert_eq!(frame["source"]["path"], json!(file));
    let locals = ide.locals(&frame);
    assert_eq!(value_of(&locals, "sum")["value"], "5");
    assert_eq!(value_of(&locals, "a")["value"], "2");

    let doubled =
        ide.request("evaluate", json!({ "expression": "sum * 2", "frameId": frame["id"], "context": "watch" }));
    assert_eq!(doubled["result"], "10");
    let scopes = ide.request("scopes", json!({ "frameId": frame["id"] }));
    let local = scopes["scopes"][0]["variablesReference"].clone();
    let changed = ide.request("setVariable", json!({ "variablesReference": local, "name": "sum", "value": "40 + 2" }));
    assert_eq!(changed["value"], "42");
    let read = ide.request("evaluate", json!({ "expression": "sum", "frameId": frame["id"], "context": "watch" }));
    assert_eq!(read["result"], "42", "the program sees the new value");

    ide.request("stepOut", json!({ "threadId": 1 }));
    assert_eq!(ide.event("stopped")["reason"], "step");
    let frame = ide.top_frame();
    assert_eq!(frame["line"], 8);
    let total = ide.request("evaluate", json!({ "expression": "total", "frameId": frame["id"], "context": "watch" }));
    assert_eq!(total["result"], "5", "a stop at return has already read what it returns");
    let shop = ide.request("evaluate", json!({ "expression": "shop", "frameId": frame["id"], "context": "watch" }));
    assert_eq!(shop["result"], "{name: \"zen\", items: Array(2)}");
    let reference = shop["variablesReference"].clone();
    let fields =
        ide.request("variables", json!({ "variablesReference": reference }))["variables"].as_array().unwrap().clone();
    assert_eq!(value_of(&fields, "name")["value"], "\"zen\"");
    assert_ne!(value_of(&fields, "items")["variablesReference"], 0);

    ide.request("stepOut", json!({ "threadId": 1 }));
    assert_eq!(ide.event("exited")["exitCode"], 0, "stepping out of the file never shows Node.js's own code");
    ide.event("terminated");
    let printed: Vec<String> = ide
        .events
        .iter()
        .filter(|event| event["event"] == "output")
        .map(|event| event["body"]["output"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        printed.join(""),
        "before\ntotal 5 zen 1\n",
        "the program's output and environment, without the debugger's lines"
    );
    ide.request("disconnect", json!({}));
}

#[test]
#[ignore = "needs Node.js on PATH (docs/development.md)"]
fn a_condition_a_log_point_and_a_breakpoint_on_the_first_line_work() {
    let dir = tempfile::tempdir().unwrap();
    let file = script(
        dir.path(),
        "with space.js",
        "let n = 0;\nfor (let i = 0; i < 3; i++) {\n  n += i;\n}\nconsole.log(n);\n",
    );
    let mut ide = Ide::start();
    ide.started(
        &file,
        &[
            json!({ "line": 1 }),
            json!({ "line": 3, "condition": "i === 2" }),
            json!({ "line": 5, "logMessage": "n is {n}" }),
        ],
    );
    assert_eq!(ide.event("stopped")["reason"], "breakpoint", "the entry stop leaves a breakpoint where it is");
    assert_eq!(ide.top_frame()["line"], 1);
    ide.request("continue", json!({ "threadId": 1 }));
    assert_eq!(ide.event("stopped")["reason"], "breakpoint");
    let frame = ide.top_frame();
    assert_eq!(value_of(&ide.locals(&frame), "i")["value"], "2", "the condition held only at the last round");
    ide.request("continue", json!({ "threadId": 1 }));
    ide.event("terminated");
    let printed: String = ide
        .events
        .iter()
        .filter(|event| event["event"] == "output")
        .map(|event| event["body"]["output"].as_str().unwrap().to_owned())
        .collect();
    assert!(printed.contains("n is 3"), "{printed}");
    ide.request("disconnect", json!({}));
}

#[test]
#[ignore = "needs Node.js on PATH (docs/development.md)"]
fn an_uncaught_exception_stops_when_asked_and_stopping_ends_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let file = script(dir.path(), "fail.js", "setInterval(() => {}, 1000);\nthrow new Error('boom');\n");
    let mut ide = Ide::start();
    ide.request("initialize", json!({}));
    ide.request("launch", json!({ "program": node(), "args": [file], "cwd": file.parent().unwrap() }));
    ide.event("initialized");
    ide.request("setExceptionBreakpoints", json!({ "filters": ["uncaught"] }));
    ide.request("configurationDone", json!({}));
    let stopped = ide.event("stopped");
    assert_eq!(stopped["reason"], "exception");
    assert!(stopped["text"].as_str().unwrap().contains("boom"), "{stopped}");
    ide.request("terminate", json!({}));
    ide.event("terminated");
    ide.request("disconnect", json!({}));
    let deadline = Instant::now() + WAIT;
    while !ide.process.has_exited() {
        assert!(Instant::now() < deadline, "the adapter ends with the session");
        std::thread::yield_now();
    }
}

#[test]
fn a_program_that_cannot_start_fails_the_launch() {
    let mut ide = Ide::start();
    ide.request("initialize", json!({}));
    let seq = ide.send("launch", json!({ "program": "/nowhere/node", "args": [] }));
    let answer = loop {
        let message = read_message(&mut ide.reader).unwrap().unwrap();
        if message["request_seq"] == seq {
            break message;
        }
    };
    assert_eq!(answer["success"], false);
    assert!(answer["message"].as_str().unwrap().contains("/nowhere/node"));
}

#[test]
fn file_urls_go_both_ways_as_node_writes_them() {
    let dir = tempfile::tempdir().unwrap();
    let file = script(dir.path(), "a b+ü.js", "");
    let url = file_url(&file);
    assert!(url.starts_with("file:///") && url.ends_with("a%20b%2B%C3%BC.js"), "{url}");
    assert_eq!(path_of_url(&url).unwrap(), file);
    assert_eq!(path_of_url("node:internal/main"), None);
}

#[test]
fn a_log_message_becomes_a_condition_that_never_stops() {
    assert_eq!(log_condition("n is {n}"), "console.log(`n is ${n}`), false");
    assert_eq!(log_condition("`$x` {a.b({c: 1})}"), "console.log(`\\`\\$x\\` ${a.b({c: 1})}`), false");
}

#[test]
fn values_show_as_their_kind_does() {
    assert_eq!(shown(&json!({ "type": "undefined" })), "undefined");
    assert_eq!(shown(&json!({ "type": "string", "value": "a\"b" })), "\"a\\\"b\"");
    assert_eq!(shown(&json!({ "type": "number", "value": 3, "description": "3" })), "3");
    assert_eq!(shown(&json!({ "type": "number", "unserializableValue": "NaN" })), "NaN");
    assert_eq!(shown(&json!({ "type": "object", "subtype": "null", "value": null })), "null");
    assert_eq!(shown(&json!({ "type": "function", "description": "function a() {\n}" })), "function a() {");
    let array = json!({
        "type": "object", "subtype": "array", "description": "Array(3)",
        "preview": { "subtype": "array", "overflow": true, "properties": [{ "name": "0", "value": "1" }] },
    });
    assert_eq!(shown(&array), "Array(3) [1, …]");
}

#[test]
#[ignore = "needs Node.js on PATH (docs/development.md)"]
fn stopping_the_session_ends_the_programs_children_too() {
    let dir = tempfile::tempdir().unwrap();
    let pid = dir.path().canonicalize().unwrap().join("child.pid");
    let child = script(
        dir.path(),
        "child.js",
        "require('fs').writeFileSync(process.argv[2], `${process.pid}`);\nsetInterval(() => {}, 1000);\n",
    );
    let parent = script(
        dir.path(),
        "parent.js",
        &format!(
            "require('child_process').spawn(process.execPath, [{child:?}, {pid:?}], {{ stdio: 'ignore' }});\nsetInterval(() => {{}}, 1000);\n"
        ),
    );
    let mut ide = Ide::start();
    ide.started(&parent, &[]);
    let deadline = Instant::now() + WAIT;
    while !pid.is_file() || std::fs::read_to_string(&pid).unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the child did not start");
        std::thread::yield_now();
    }
    let child_pid = std::fs::read_to_string(&pid).unwrap();
    ide.request("terminate", json!({}));
    ide.event("terminated");
    let alive = || Command::new("kill").args(["-0", &child_pid]).stderr(Stdio::null()).status().unwrap().success();
    while alive() {
        assert!(Instant::now() < deadline, "the child outlives the session");
        std::thread::yield_now();
    }
    ide.request("disconnect", json!({}));
}
