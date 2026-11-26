use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use zen_dap::connection::{read_message, write_message};
use zen_i18n::t;

use super::websocket::{self, Writer};
use crate::debug::AdapterProcess;

pub const ADAPTER: &str = "node";

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const START_TIMEOUT: Duration = Duration::from_secs(30);
const THREAD: i64 = 1;
const BREAK_ON_START: &str = "Break on start";
const LISTENING: &str = "Debugger listening on ";
const WAITING: &str = "Waiting for the debugger to disconnect";
const QUIET: [&str; 3] = ["For help, see:", "Debugger attached.", "Debugger ending on "];

enum Event {
    Request(Value),
    RequestsClosed,
    Protocol(Value),
    ProtocolClosed,
    Listening(String),
    Finishing,
    Output { category: &'static str, text: String },
    Exited(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Reference {
    Object(String),
    Scope { object: String, number: i64, frame: String, this: Option<Value> },
}

#[derive(Clone, Default)]
pub struct NodeProcess {
    child: Arc<Mutex<Option<Child>>>,
    done: Arc<AtomicBool>,
}

impl NodeProcess {
    fn kill(&self) {
        if let Some(child) = self.child.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            zen_run::kill_group(child.id());
            let _ = child.kill();
        }
    }
}

impl AdapterProcess for NodeProcess {
    fn has_exited(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    fn kill(&self) {
        NodeProcess::kill(self);
        self.done.store(true, Ordering::SeqCst);
    }
}

pub fn launch(env: &[(String, String)], address: SocketAddr) -> Result<Arc<dyn AdapterProcess>, String> {
    let stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    stream.set_nodelay(true).map_err(|error| error.to_string())?;
    let process = NodeProcess::default();
    let handle = process.clone();
    let env = env.to_vec();
    std::thread::spawn(move || {
        let mut adapter = Adapter::new(stream, env, handle.clone());
        adapter.run();
        handle.kill();
        handle.done.store(true, Ordering::SeqCst);
    });
    Ok(Arc::new(process))
}

fn read_requests(mut reader: impl BufRead, events: Sender<Event>) {
    while let Ok(Some(message)) = read_message(&mut reader) {
        if events.send(Event::Request(message)).is_err() {
            return;
        }
    }
    let _ = events.send(Event::RequestsClosed);
}

fn read_output(stream: impl Read, category: &'static str, events: Sender<Event>) {
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let event = if let Some(address) = line.strip_prefix(LISTENING) {
            Event::Listening(address.trim().to_owned())
        } else if line.starts_with(WAITING) {
            Event::Finishing
        } else if QUIET.iter().any(|quiet| line.starts_with(quiet)) {
            continue;
        } else {
            Event::Output { category, text: format!("{line}\n") }
        };
        if events.send(event).is_err() {
            return;
        }
    }
}

fn watch_exit(process: NodeProcess, events: Sender<Event>) {
    loop {
        let status = match process.child.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            Some(child) => child.try_wait(),
            None => return,
        };
        match status {
            Ok(Some(status)) => {
                let _ = events.send(Event::Exited(status.code().unwrap_or(-1)));
                return;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(_) => return,
        }
    }
}

fn percent_encoded(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

pub fn file_url(path: &Path) -> String {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_owned());
    format!("file://{}", percent_encoded(&real.to_string_lossy()))
}

pub fn path_of_url(url: &str) -> Option<PathBuf> {
    let encoded = url.strip_prefix("file://")?.as_bytes();
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut at = 0;
    while at < encoded.len() {
        let hex = (encoded[at] == b'%').then(|| encoded.get(at + 1..at + 3)).flatten();
        match hex.and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()) {
            Some(byte) => {
                bytes.push(byte);
                at += 3;
            }
            None => {
                bytes.push(encoded[at]);
                at += 1;
            }
        }
    }
    Some(PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()))
}

fn preview_text(preview: &Value) -> Option<String> {
    let properties = preview["properties"].as_array()?;
    let items: Vec<String> = properties
        .iter()
        .map(|property| {
            let text = property["value"].as_str().unwrap_or("…");
            let value = match property["type"].as_str() {
                Some("string") => serde_json::to_string(text).unwrap_or_default(),
                _ => text.to_owned(),
            };
            if preview["subtype"] == "array" {
                value
            } else {
                format!("{}: {value}", property["name"].as_str().unwrap_or_default())
            }
        })
        .collect();
    let more = if preview["overflow"] == true { ", …" } else { "" };
    let (open, close) = if preview["subtype"] == "array" { ("[", "]") } else { ("{", "}") };
    Some(format!("{open}{}{more}{close}", items.join(", ")))
}

pub fn shown(remote: &Value) -> String {
    match remote["type"].as_str().unwrap_or_default() {
        "undefined" => "undefined".to_owned(),
        "string" => serde_json::to_string(remote["value"].as_str().unwrap_or_default()).unwrap_or_default(),
        "function" => {
            remote["description"].as_str().unwrap_or("function").lines().next().unwrap_or_default().to_owned()
        }
        "object" if remote["subtype"] == "null" => "null".to_owned(),
        "object" => {
            let name = remote["description"].as_str().unwrap_or("Object");
            match remote.get("preview").and_then(preview_text) {
                Some(preview) if name == "Object" => preview,
                Some(preview) => format!("{name} {preview}"),
                None => name.to_owned(),
            }
        }
        _ => remote
            .get("unserializableValue")
            .and_then(Value::as_str)
            .or_else(|| remote["description"].as_str())
            .map_or_else(|| remote["value"].to_string(), str::to_owned),
    }
}

fn type_of(remote: &Value) -> String {
    remote["className"]
        .as_str()
        .or_else(|| remote["subtype"].as_str())
        .or_else(|| remote["type"].as_str())
        .unwrap_or_default()
        .to_owned()
}

fn scope_name(kind: &str) -> String {
    let mut chars = kind.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

pub fn log_condition(message: &str) -> String {
    let mut template = String::with_capacity(message.len());
    let mut within = 0;
    for c in message.chars() {
        match c {
            '{' => {
                template.push_str(if within == 0 { "${" } else { "{" });
                within += 1;
            }
            '}' if within > 0 => {
                template.push('}');
                within -= 1;
            }
            '`' | '\\' if within == 0 => {
                template.push('\\');
                template.push(c);
            }
            '$' if within == 0 => template.push_str("\\$"),
            other => template.push(other),
        }
    }
    format!("console.log(`{template}`), false")
}

fn condition(breakpoint: &Value) -> Option<String> {
    match (breakpoint["logMessage"].as_str(), breakpoint["condition"].as_str()) {
        (Some(message), Some(condition)) => Some(format!("({condition}) && ({})", log_condition(message))),
        (Some(message), None) => Some(log_condition(message)),
        (None, Some(condition)) => Some(condition.to_owned()),
        (None, None) => None,
    }
}

fn capabilities() -> Value {
    json!({
        "supportsConfigurationDoneRequest": true,
        "supportsConditionalBreakpoints": true,
        "supportsLogPoints": true,
        "supportsEvaluateForHovers": true,
        "supportsSetVariable": true,
        "supportsTerminateRequest": true,
        "exceptionBreakpointFilters": [
            { "filter": "all", "label": t!("workbench-node-debug-filter-all"), "default": false },
            { "filter": "uncaught", "label": t!("workbench-node-debug-filter-uncaught"), "default": false }
        ],
    })
}

struct Set {
    protocol_id: String,
    id: i64,
    line: i64,
}

struct Adapter {
    output: TcpStream,
    events: Receiver<Event>,
    sender: Sender<Event>,
    queue: VecDeque<Event>,
    socket: Option<Writer>,
    env: Vec<(String, String)>,
    process: NodeProcess,
    seq: i64,
    calls: i64,
    next_breakpoint: i64,
    next_reference: i64,
    set: HashMap<PathBuf, Vec<Set>>,
    references: HashMap<i64, Reference>,
    frames: Vec<Value>,
    scripts: HashMap<String, String>,
    generation: i64,
    configured: bool,
    at_entry: bool,
    stepping: bool,
    pausing: bool,
    inspect_child: bool,
    ended: bool,
}

impl Adapter {
    fn new(stream: TcpStream, env: Vec<(String, String)>, process: NodeProcess) -> Self {
        let (sender, events) = channel();
        if let Ok(reader) = stream.try_clone() {
            let requests = sender.clone();
            std::thread::spawn(move || read_requests(BufReader::new(reader), requests));
        }
        Self {
            output: stream,
            events,
            sender,
            queue: VecDeque::new(),
            socket: None,
            env,
            process,
            seq: 0,
            calls: 0,
            next_breakpoint: 0,
            next_reference: 0,
            set: HashMap::new(),
            references: HashMap::new(),
            frames: Vec::new(),
            scripts: HashMap::new(),
            generation: 0,
            configured: false,
            at_entry: false,
            stepping: false,
            pausing: false,
            inspect_child: false,
            ended: false,
        }
    }

    fn write(&mut self, mut message: Value) {
        self.seq += 1;
        message["seq"] = json!(self.seq);
        let _ = write_message(&mut self.output, &message);
    }

    fn event(&mut self, event: &str, body: Value) {
        self.write(json!({ "type": "event", "event": event, "body": body }));
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.calls += 1;
        let id = self.calls;
        let text = json!({ "id": id, "method": method, "params": params }).to_string();
        let socket = self.socket.as_mut().ok_or_else(|| t!("workbench-node-debug-not-running"))?;
        socket.send(&text).map_err(|error| error.to_string())?;
        loop {
            let event = self.events.recv_timeout(CALL_TIMEOUT).map_err(|error| match error {
                RecvTimeoutError::Timeout => t!("workbench-node-debug-no-answer", method = method),
                RecvTimeoutError::Disconnected => t!("workbench-node-debug-not-running"),
            })?;
            match event {
                Event::Protocol(answer) if answer["id"] == id => {
                    return match answer.get("error") {
                        Some(error) => Err(error["message"].as_str().unwrap_or("failed").to_owned()),
                        None => Ok(answer["result"].clone()),
                    };
                }
                Event::ProtocolClosed => {
                    self.queue.push_back(Event::ProtocolClosed);
                    return Err(t!("workbench-node-debug-not-running"));
                }
                other => self.queue.push_back(other),
            }
        }
    }

    fn run(&mut self) {
        loop {
            let next = self.queue.pop_front().map_or_else(|| self.events.recv().ok(), Some);
            match next {
                None | Some(Event::RequestsClosed) => return,
                Some(Event::Request(message)) => {
                    if self.request(&message) {
                        return;
                    }
                }
                Some(Event::Protocol(message)) => self.protocol(&message),
                Some(Event::ProtocolClosed) => self.socket = None,
                Some(Event::Listening(_)) => {}
                Some(Event::Finishing) => {
                    if let Some(mut socket) = self.socket.take() {
                        socket.close();
                    }
                }
                Some(Event::Output { category, text }) => {
                    self.event("output", json!({ "category": category, "output": text }));
                }
                Some(Event::Exited(code)) => self.ended(code),
            }
        }
    }

    fn ended(&mut self, code: i32) {
        if self.ended {
            return;
        }
        self.ended = true;
        self.event("exited", json!({ "exitCode": code }));
        self.event("terminated", json!({}));
    }

    fn request(&mut self, message: &Value) -> bool {
        let command = message["command"].as_str().unwrap_or_default().to_owned();
        let arguments = &message["arguments"];
        let answer = match command.as_str() {
            "initialize" => Ok(capabilities()),
            "launch" => self.launch_program(arguments),
            "configurationDone" => self.configuration_done(),
            "setBreakpoints" => self.set_breakpoints(arguments),
            "setExceptionBreakpoints" => self.set_exceptions(arguments),
            "threads" => Ok(json!({ "threads": [{ "id": THREAD, "name": "main" }] })),
            "stackTrace" => Ok(self.stack_trace(arguments)),
            "scopes" => Ok(self.scopes(arguments)),
            "variables" => self.variables(arguments),
            "evaluate" => self.evaluate(arguments),
            "setVariable" => self.set_variable(arguments),
            "continue" => self.resume("Debugger.resume").map(|()| json!({ "allThreadsContinued": true })),
            "next" => self.resume("Debugger.stepOver").map(|()| json!({})),
            "stepIn" => self.resume("Debugger.stepInto").map(|()| json!({})),
            "stepOut" => self.resume("Debugger.stepOut").map(|()| json!({})),
            "pause" => {
                self.pausing = true;
                self.call("Debugger.pause", json!({})).map(|_| json!({}))
            }
            "terminate" | "disconnect" => {
                self.process.kill();
                Ok(json!({}))
            }
            other => Err(t!("workbench-node-debug-unsupported", request = other)),
        };
        let mut response = json!({
            "type": "response",
            "request_seq": message["seq"],
            "command": command,
            "success": answer.is_ok(),
        });
        match answer {
            Ok(body) => response["body"] = body,
            Err(error) => response["message"] = json!(error),
        }
        let launched = command == "launch" && response["success"] == true;
        self.write(response);
        if launched {
            self.event("initialized", json!({}));
        }
        command == "disconnect"
    }

    fn launch_program(&mut self, arguments: &Value) -> Result<Value, String> {
        let program = arguments["program"].as_str().ok_or_else(|| t!("workbench-node-debug-no-program"))?;
        let mut command = Command::new(program);
        self.inspect_child = arguments["inspectChild"] == true;
        if !self.inspect_child {
            command.arg("--inspect-brk=127.0.0.1:0");
        }
        command.args(arguments["args"].as_array().into_iter().flatten().filter_map(Value::as_str));
        if let Some(cwd) = arguments["cwd"].as_str() {
            command.current_dir(cwd);
        }
        command.envs(self.env.iter().map(|(name, value)| (name.as_str(), value.as_str())));
        for pair in arguments["env"].as_array().into_iter().flatten() {
            if let (Some(name), Some(value)) = (pair[0].as_str(), pair[1].as_str()) {
                command.env(name, value);
            }
        }
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        zen_run::own_group(&mut command);
        let mut child = command.spawn().map_err(|error| format!("{program}: {error}"))?;
        if let Some(stdout) = child.stdout.take() {
            let events = self.sender.clone();
            std::thread::spawn(move || read_output(stdout, "stdout", events));
        }
        if let Some(stderr) = child.stderr.take() {
            let events = self.sender.clone();
            std::thread::spawn(move || read_output(stderr, "stderr", events));
        }
        *self.process.child.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
        let watcher = self.process.clone();
        let events = self.sender.clone();
        std::thread::spawn(move || watch_exit(watcher, events));
        let url = self.wait_for_address()?;
        let (mut reader, writer) = websocket::connect(&url).map_err(|error| error.to_string())?;
        self.socket = Some(writer);
        let events = self.sender.clone();
        std::thread::spawn(move || {
            while let Ok(Some(text)) = reader.next_text() {
                let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
                if events.send(Event::Protocol(message)).is_err() {
                    return;
                }
            }
            let _ = events.send(Event::ProtocolClosed);
        });
        self.call("Runtime.enable", json!({}))?;
        self.call("Debugger.enable", json!({}))?;
        let _ = self.call("Debugger.setBlackboxPatterns", json!({ "patterns": ["^node:"] }));
        self.call("Runtime.runIfWaitingForDebugger", json!({}))?;
        Ok(json!({}))
    }

    fn wait_for_address(&mut self) -> Result<String, String> {
        loop {
            let event = self.events.recv_timeout(START_TIMEOUT).map_err(|_| t!("workbench-node-debug-not-started"))?;
            match event {
                Event::Listening(url) => return Ok(url),
                Event::Exited(code) => {
                    self.queue.push_back(Event::Exited(code));
                    return Err(t!("workbench-node-debug-ended", code = code.to_string().as_str()));
                }
                other => self.queue.push_back(other),
            }
        }
    }

    fn configuration_done(&mut self) -> Result<Value, String> {
        self.configured = true;
        if std::mem::take(&mut self.at_entry) {
            self.leave_entry()?;
        }
        Ok(json!({}))
    }

    fn at_worker_entry(&self) -> bool {
        self.frames.first().is_some_and(|frame| frame["location"]["lineNumber"] == 0 && frame["functionName"] == "")
    }

    fn url_of<'a>(&'a self, frame: &'a Value) -> &'a str {
        let script = frame["location"]["scriptId"].as_str().unwrap_or_default();
        frame["url"]
            .as_str()
            .filter(|url| !url.is_empty())
            .or_else(|| self.scripts.get(script).map(String::as_str))
            .unwrap_or_default()
    }

    fn at_breakpoint(&self) -> Option<i64> {
        let frame = self.frames.first()?;
        let path = path_of_url(self.url_of(frame))?;
        let line = frame["location"]["lineNumber"].as_i64()? + 1;
        self.set.get(&path)?.iter().find(|set| set.line == line).map(|set| set.id)
    }

    fn leave_entry(&mut self) -> Result<(), String> {
        match self.at_breakpoint() {
            Some(id) => {
                self.stopped("breakpoint", None, vec![id]);
                Ok(())
            }
            None => self.resume("Debugger.resume"),
        }
    }

    fn resume(&mut self, method: &str) -> Result<(), String> {
        self.references.clear();
        self.frames.clear();
        self.stepping = method != "Debugger.resume";
        self.call(method, json!({})).map(|_| ())
    }

    fn stopped(&mut self, reason: &str, text: Option<String>, hits: Vec<i64>) {
        let mut body = json!({ "reason": reason, "threadId": THREAD, "allThreadsStopped": true });
        if let Some(text) = text {
            body["text"] = json!(text);
        }
        if !hits.is_empty() {
            body["hitBreakpointIds"] = json!(hits);
        }
        self.event("stopped", body);
    }

    fn protocol(&mut self, message: &Value) {
        let params = &message["params"];
        match message["method"].as_str().unwrap_or_default() {
            "Debugger.paused" => self.pause(params),
            "Debugger.scriptParsed" => {
                if let (Some(id), Some(url)) = (params["scriptId"].as_str(), params["url"].as_str()) {
                    self.scripts.insert(id.to_owned(), url.to_owned());
                }
            }
            "Debugger.breakpointResolved" => self.resolved(params),
            "Runtime.executionContextDestroyed" => {
                if let Some(mut socket) = self.socket.take() {
                    socket.close();
                }
            }
            _ => {}
        }
    }

    fn pause(&mut self, params: &Value) {
        self.generation += 1;
        self.references.clear();
        self.frames = params["callFrames"].as_array().cloned().unwrap_or_default();
        if params["reason"] == BREAK_ON_START {
            if self.configured {
                let _ = self.leave_entry();
            } else {
                self.at_entry = true;
            }
            return;
        }
        let hits: Vec<i64> = params["hitBreakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|hit| self.set.values().flatten().find(|set| set.protocol_id == hit).map(|set| set.id))
            .collect();
        let exception = matches!(params["reason"].as_str(), Some("exception" | "promiseRejection"));
        let text = exception.then(|| params["data"]["description"].as_str().unwrap_or_default().to_owned());
        let stepping = std::mem::take(&mut self.stepping);
        let pausing = std::mem::take(&mut self.pausing);
        if stepping && self.frames.first().is_some_and(|frame| self.url_of(frame).starts_with("node:")) {
            let _ = self.resume("Debugger.resume");
            return;
        }
        if self.inspect_child && !stepping && !pausing && !exception && hits.is_empty() && self.at_worker_entry() {
            let _ = self.leave_entry();
            return;
        }
        let reason = match (exception, hits.is_empty(), stepping) {
            (true, _, _) => "exception",
            (_, false, _) => "breakpoint",
            (_, true, true) => "step",
            _ => "pause",
        };
        self.stopped(reason, text, hits);
    }

    fn resolved(&mut self, params: &Value) {
        let id = params["breakpointId"].as_str().unwrap_or_default();
        let line = params["location"]["lineNumber"].as_i64().unwrap_or(0) + 1;
        let found = self.set.values_mut().flatten().find(|set| set.protocol_id == id).map(|set| {
            set.line = line;
            set.id
        });
        if let Some(id) = found {
            self.event(
                "breakpoint",
                json!({ "reason": "changed", "breakpoint": { "id": id, "verified": true, "line": line } }),
            );
        }
    }

    fn set_breakpoints(&mut self, arguments: &Value) -> Result<Value, String> {
        let path =
            PathBuf::from(arguments["source"]["path"].as_str().ok_or_else(|| t!("workbench-node-debug-no-source"))?);
        let path = path.canonicalize().unwrap_or(path);
        for old in self.set.remove(&path).unwrap_or_default() {
            let _ = self.call("Debugger.removeBreakpoint", json!({ "breakpointId": old.protocol_id }));
        }
        let url = file_url(&path);
        let mut answered = Vec::new();
        let mut set = Vec::new();
        for breakpoint in arguments["breakpoints"].as_array().into_iter().flatten() {
            let line = breakpoint["line"].as_i64().unwrap_or(1);
            let mut params = json!({ "url": url, "lineNumber": line - 1 });
            if let Some(column) = breakpoint["column"].as_i64() {
                params["columnNumber"] = json!(column - 1);
            }
            if let Some(condition) = condition(breakpoint) {
                params["condition"] = json!(condition);
            }
            match self.call("Debugger.setBreakpointByUrl", params) {
                Ok(result) => {
                    self.next_breakpoint += 1;
                    let id = self.next_breakpoint;
                    let location = result["locations"].get(0);
                    let at =
                        location.and_then(|location| location["lineNumber"].as_i64()).map_or(line, |line| line + 1);
                    let protocol_id = result["breakpointId"].as_str().unwrap_or_default().to_owned();
                    set.push(Set { protocol_id, id, line: at });
                    answered.push(json!({ "id": id, "verified": location.is_some(), "line": at }));
                }
                Err(error) => answered.push(json!({ "verified": false, "line": line, "message": error })),
            }
        }
        self.set.insert(path, set);
        Ok(json!({ "breakpoints": answered }))
    }

    fn set_exceptions(&mut self, arguments: &Value) -> Result<Value, String> {
        let filters: Vec<&str> =
            arguments["filters"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let state = if filters.contains(&"all") {
            "all"
        } else if filters.contains(&"uncaught") {
            "uncaught"
        } else {
            "none"
        };
        self.call("Debugger.setPauseOnExceptions", json!({ "state": state })).map(|_| json!({}))
    }

    fn frame_id(&self, index: usize) -> i64 {
        self.generation * 100_000 + index as i64 + 1
    }

    fn frame_at(&self, id: i64) -> Option<&Value> {
        let index = usize::try_from(id - self.generation * 100_000 - 1).ok()?;
        self.frames.get(index)
    }

    fn stack_trace(&self, arguments: &Value) -> Value {
        let start = arguments["startFrame"].as_u64().unwrap_or(0) as usize;
        let levels = arguments["levels"].as_u64().filter(|levels| *levels > 0).unwrap_or(1000) as usize;
        let frames: Vec<Value> = self
            .frames
            .iter()
            .enumerate()
            .skip(start)
            .take(levels)
            .map(|(index, frame)| {
                let url = self.url_of(frame);
                let name = frame["functionName"].as_str().filter(|name| !name.is_empty()).unwrap_or("<anonymous>");
                let source = match path_of_url(url) {
                    Some(path) => json!({ "name": path.file_name().map(|name| name.to_string_lossy().into_owned()), "path": path }),
                    None => json!({ "name": url, "presentationHint": "deemphasize" }),
                };
                json!({
                    "id": self.frame_id(index),
                    "name": name,
                    "source": source,
                    "line": frame["location"]["lineNumber"].as_i64().unwrap_or(0) + 1,
                    "column": frame["location"]["columnNumber"].as_i64().unwrap_or(0) + 1,
                })
            })
            .collect();
        json!({ "stackFrames": frames, "totalFrames": self.frames.len() })
    }

    fn reference(&mut self, reference: Reference) -> i64 {
        self.next_reference += 1;
        self.references.insert(self.next_reference, reference);
        self.next_reference
    }

    fn scopes(&mut self, arguments: &Value) -> Value {
        let Some(frame) = arguments["frameId"].as_i64().and_then(|id| self.frame_at(id)).cloned() else {
            return json!({ "scopes": [] });
        };
        let call_frame = frame["callFrameId"].as_str().unwrap_or_default().to_owned();
        let mut scopes = Vec::new();
        for (number, scope) in frame["scopeChain"].as_array().into_iter().flatten().enumerate() {
            let kind = scope["type"].as_str().unwrap_or_default();
            let Some(object) = scope["object"]["objectId"].as_str() else { continue };
            let this = (number == 0).then(|| frame["this"].clone()).filter(|this| this["type"] != "undefined");
            let reference = self.reference(Reference::Scope {
                object: object.to_owned(),
                number: number as i64,
                frame: call_frame.clone(),
                this,
            });
            scopes.push(
                json!({ "name": scope_name(kind), "variablesReference": reference, "expensive": kind == "global" }),
            );
        }
        json!({ "scopes": scopes })
    }

    fn variable(&mut self, name: &str, remote: &Value) -> Value {
        let reference =
            remote["objectId"].as_str().map_or(0, |object| self.reference(Reference::Object(object.to_owned())));
        json!({ "name": name, "value": shown(remote), "type": type_of(remote), "variablesReference": reference })
    }

    fn variables(&mut self, arguments: &Value) -> Result<Value, String> {
        let wanted = arguments["variablesReference"].as_i64().unwrap_or(0);
        let reference = self.references.get(&wanted).cloned().ok_or_else(|| t!("workbench-node-debug-moved-on"))?;
        let (object, this) = match reference {
            Reference::Object(object) => (object, None),
            Reference::Scope { object, this, .. } => (object, this),
        };
        let params = json!({ "objectId": object, "ownProperties": true, "generatePreview": true });
        let answer = self.call("Runtime.getProperties", params)?;
        let mut variables = Vec::new();
        if let Some(this) = this {
            variables.push(self.variable("this", &this));
        }
        for property in answer["result"].as_array().into_iter().flatten() {
            let name = property["name"].as_str().unwrap_or_default();
            if name == "__proto__" {
                continue;
            }
            let variable = match property.get("value") {
                Some(value) => self.variable(name, value),
                None => json!({ "name": name, "value": "(accessor)", "variablesReference": 0 }),
            };
            variables.push(variable);
        }
        for property in answer["internalProperties"].as_array().into_iter().flatten() {
            let name = property["name"].as_str().unwrap_or_default();
            if name != "[[Prototype]]"
                && let Some(value) = property.get("value")
            {
                variables.push(self.variable(name, value));
            }
        }
        Ok(json!({ "variables": variables }))
    }

    fn evaluated(&mut self, expression: &str, frame: Option<&str>, hover: bool) -> Result<Value, String> {
        let answer = match frame {
            Some(frame) => {
                let params = json!({
                    "callFrameId": frame, "expression": expression, "generatePreview": true, "throwOnSideEffect": hover,
                });
                self.call("Debugger.evaluateOnCallFrame", params)?
            }
            None => {
                let params = json!({ "expression": expression, "generatePreview": true, "throwOnSideEffect": hover });
                self.call("Runtime.evaluate", params)?
            }
        };
        if let Some(details) = answer.get("exceptionDetails") {
            let text = details["exception"]["description"].as_str().or_else(|| details["text"].as_str());
            let failed = t!("workbench-node-debug-expression-failed");
            return Err(text.unwrap_or(&failed).lines().next().unwrap_or_default().to_owned());
        }
        Ok(answer["result"].clone())
    }

    fn evaluate(&mut self, arguments: &Value) -> Result<Value, String> {
        let expression = arguments["expression"].as_str().unwrap_or_default();
        let frame = arguments["frameId"]
            .as_i64()
            .and_then(|id| self.frame_at(id))
            .and_then(|frame| frame["callFrameId"].as_str())
            .map(str::to_owned);
        let result = self.evaluated(expression, frame.as_deref(), arguments["context"] == "hover")?;
        let shown_value = self.variable("", &result);
        Ok(json!({
            "result": shown_value["value"],
            "type": shown_value["type"],
            "variablesReference": shown_value["variablesReference"],
        }))
    }

    fn set_variable(&mut self, arguments: &Value) -> Result<Value, String> {
        let wanted = arguments["variablesReference"].as_i64().unwrap_or(0);
        let reference = self.references.get(&wanted).cloned().ok_or_else(|| t!("workbench-node-debug-moved-on"))?;
        let name = arguments["name"].as_str().unwrap_or_default();
        let expression = arguments["value"].as_str().unwrap_or_default();
        let top = self.frames.first().and_then(|frame| frame["callFrameId"].as_str()).map(str::to_owned);
        let frame = match &reference {
            Reference::Scope { frame, .. } => Some(frame.clone()),
            Reference::Object(_) => top,
        };
        let value = self.evaluated(expression, frame.as_deref(), false)?;
        let argument = match (value.get("objectId"), value.get("unserializableValue")) {
            (Some(object), _) => json!({ "objectId": object }),
            (None, Some(number)) => json!({ "unserializableValue": number }),
            _ => json!({ "value": value.get("value") }),
        };
        match reference {
            Reference::Scope { number, frame, .. } => {
                let params =
                    json!({ "scopeNumber": number, "variableName": name, "newValue": argument, "callFrameId": frame });
                self.call("Debugger.setVariableValue", params)?;
            }
            Reference::Object(object) => {
                let params = json!({
                    "objectId": object,
                    "functionDeclaration": "function (name, value) { this[name] = value; }",
                    "arguments": [{ "value": name }, argument],
                });
                self.call("Runtime.callFunctionOn", params)?;
            }
        }
        let variable = self.variable(name, &value);
        Ok(
            json!({ "value": variable["value"], "type": variable["type"], "variablesReference": variable["variablesReference"] }),
        )
    }
}

#[cfg(test)]
mod tests;
