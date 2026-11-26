use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::connection::{Connection, read_message, write_message};
use crate::protocol::{Source, StackFrame, StepInTarget, Thread, Variable, VariablePresentationHint};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FakeVariable {
    pub name: String,
    pub value: String,
    pub type_: Option<String>,
    pub children: Vec<FakeVariable>,
    pub raw: Option<Vec<FakeVariable>>,
    pub lazy: Option<String>,
    pub indexed: bool,
}

impl FakeVariable {
    pub fn new(name: &str, value: &str, type_: &str) -> Self {
        Self { name: name.to_owned(), value: value.to_owned(), type_: Some(type_.to_owned()), ..Self::default() }
    }

    pub fn with_children(self, children: Vec<FakeVariable>) -> Self {
        Self { children, ..self }
    }

    pub fn with_raw(self, raw: Vec<FakeVariable>) -> Self {
        Self { raw: Some(raw), ..self }
    }

    pub fn lazy(self, shown: &str) -> Self {
        Self { lazy: Some(shown.to_owned()), ..self }
    }

    pub fn indexed(self) -> Self {
        Self { indexed: true, ..self }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InitializedOrder {
    #[default]
    BeforeLaunchResponse,
    AfterLaunchResponse,
}

#[derive(Clone)]
struct Script {
    threads: Vec<Thread>,
    program: Vec<(String, i64)>,
    verified: Option<Vec<i64>>,
    frames: Vec<Vec<StackFrame>>,
    variables: Vec<FakeVariable>,
    output: Vec<(String, String)>,
    exit_code: i64,
    order: InitializedOrder,
    launch_failure: Option<String>,
    silent: Vec<String>,
    reverse_request: Option<String>,
    hold: bool,
    capabilities: Value,
    functions: Vec<(String, usize)>,
    step_targets: Vec<StepInTarget>,
    evaluations: Vec<(String, FakeVariable)>,
    evaluation_frames: Option<Vec<i64>>,
    views: Option<String>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            threads: vec![Thread { id: 1, name: "main".to_owned() }],
            program: Vec::new(),
            verified: None,
            frames: Vec::new(),
            variables: Vec::new(),
            output: Vec::new(),
            exit_code: 0,
            order: InitializedOrder::default(),
            launch_failure: None,
            silent: Vec::new(),
            reverse_request: None,
            hold: false,
            capabilities: json!({}),
            functions: Vec::new(),
            step_targets: Vec::new(),
            evaluations: Vec::new(),
            evaluation_frames: None,
            views: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct FakeAdapter {
    script: Arc<Mutex<Script>>,
    log: Arc<Mutex<Vec<Value>>>,
}

impl FakeAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn edit(&self, change: impl FnOnce(&mut Script)) {
        if let Ok(mut script) = self.script.lock() {
            change(&mut script);
        }
    }

    pub fn set_threads(&self, threads: Vec<Thread>) {
        self.edit(|script| script.threads = threads);
    }

    pub fn set_program(&self, lines: &[(&Path, i64)]) {
        let lines = lines.iter().map(|(path, line)| (path.to_string_lossy().into_owned(), *line)).collect();
        self.edit(|script| script.program = lines);
    }

    pub fn set_verified(&self, lines: Option<Vec<i64>>) {
        self.edit(|script| script.verified = lines);
    }

    pub fn set_frames(&self, frames: Vec<Vec<StackFrame>>) {
        self.edit(|script| script.frames = frames);
    }

    pub fn set_variables(&self, variables: Vec<FakeVariable>) {
        self.edit(|script| script.variables = variables);
    }

    pub fn set_output(&self, output: &[(&str, &str)]) {
        let output = output.iter().map(|(category, text)| ((*category).to_owned(), (*text).to_owned())).collect();
        self.edit(|script| script.output = output);
    }

    pub fn set_exit_code(&self, code: i64) {
        self.edit(|script| script.exit_code = code);
    }

    pub fn set_initialized_order(&self, order: InitializedOrder) {
        self.edit(|script| script.order = order);
    }

    pub fn fail_launch(&self, message: Option<&str>) {
        self.edit(|script| script.launch_failure = message.map(str::to_owned));
    }

    pub fn set_silent(&self, command: &str) {
        self.edit(|script| script.silent.push(command.to_owned()));
    }

    pub fn set_reverse_request(&self, command: Option<&str>) {
        self.edit(|script| script.reverse_request = command.map(str::to_owned));
    }

    pub fn set_hold(&self, hold: bool) {
        self.edit(|script| script.hold = hold);
    }

    pub fn set_capabilities(&self, capabilities: Value) {
        self.edit(|script| script.capabilities = capabilities);
    }

    pub fn set_functions(&self, functions: &[(&str, usize)]) {
        let functions = functions.iter().map(|(name, at)| ((*name).to_owned(), *at)).collect();
        self.edit(|script| script.functions = functions);
    }

    pub fn set_step_in_targets(&self, targets: Vec<StepInTarget>) {
        self.edit(|script| script.step_targets = targets);
    }

    pub fn set_evaluations(&self, evaluations: Vec<(&str, FakeVariable)>) {
        let evaluations = evaluations.into_iter().map(|(expression, value)| (expression.to_owned(), value)).collect();
        self.edit(|script| script.evaluations = evaluations);
    }

    pub fn set_evaluation_frames(&self, frames: Option<Vec<i64>>) {
        self.edit(|script| script.evaluation_frames = frames);
    }

    pub fn set_views_request(&self, command: Option<&str>) {
        self.edit(|script| script.views = command.map(str::to_owned));
    }

    pub fn connect_pipes(&self) -> std::io::Result<Connection> {
        let (from_client, to_adapter) = std::io::pipe()?;
        let (from_adapter, to_client) = std::io::pipe()?;
        let adapter = self.clone();
        std::thread::spawn(move || adapter.serve(from_client, to_client, None));
        Ok(Connection::new(from_adapter, to_adapter))
    }

    pub fn connect(&self, addr: SocketAddr) -> std::io::Result<JoinHandle<()>> {
        let stream = TcpStream::connect(addr)?;
        let reader = stream.try_clone()?;
        let adapter = self.clone();
        Ok(std::thread::spawn(move || {
            let closing = stream.try_clone().ok();
            adapter.serve(reader, stream, closing);
        }))
    }

    pub fn received(&self) -> Vec<Value> {
        self.log.lock().map(|log| log.clone()).unwrap_or_default()
    }

    pub fn requests(&self, command: &str) -> Vec<Value> {
        self.received()
            .into_iter()
            .filter(|message| message["type"] == "request" && message["command"] == command)
            .map(|message| message.get("arguments").cloned().unwrap_or(Value::Null))
            .collect()
    }

    pub fn commands(&self) -> Vec<String> {
        self.received()
            .into_iter()
            .filter(|message| message["type"] == "request")
            .filter_map(|message| message["command"].as_str().map(str::to_owned))
            .collect()
    }

    pub fn responses(&self) -> Vec<Value> {
        self.received().into_iter().filter(|message| message["type"] == "response").collect()
    }

    pub fn wait_for(&self, command: &str, count: usize, timeout: Duration) -> Vec<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let found = self.requests(command);
            if found.len() >= count || Instant::now() >= deadline {
                return found;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn serve(&self, reader: impl Read, writer: impl Write, closing: Option<TcpStream>) {
        let mut session = Session {
            writer,
            seq: 1,
            breakpoints: HashMap::new(),
            functions: Vec::new(),
            hits: HashMap::new(),
            at: None,
            stops: 0,
            next_id: 1,
            assigned: HashMap::new(),
            raw: false,
        };
        let mut reader = BufReader::new(reader);
        while let Ok(Some(message)) = read_message(&mut reader) {
            if let Ok(mut log) = self.log.lock() {
                log.push(message.clone());
            }
            if message["type"] != "request" {
                continue;
            }
            let script = self.script.lock().map(|script| script.clone()).unwrap_or_default();
            if !session.handle(&script, &message) {
                break;
            }
        }
        if let Some(stream) = closing {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

#[derive(Clone)]
struct Set {
    id: i64,
    line: i64,
    log: Option<String>,
    hit: Option<u64>,
}

struct Session<W: Write> {
    writer: W,
    seq: i64,
    breakpoints: HashMap<String, Vec<Set>>,
    functions: Vec<(i64, usize)>,
    hits: HashMap<i64, u64>,
    at: Option<usize>,
    stops: usize,
    next_id: i64,
    assigned: HashMap<String, String>,
    raw: bool,
}

impl<W: Write> Session<W> {
    fn send(&mut self, mut message: Value) {
        message["seq"] = json!(self.seq);
        self.seq += 1;
        let _ = write_message(&mut self.writer, &message);
    }

    fn respond(&mut self, request: &Value, body: Value) {
        let mut message = json!({
            "type": "response",
            "request_seq": request["seq"],
            "success": true,
            "command": request["command"],
        });
        if !body.is_null() {
            message["body"] = body;
        }
        self.send(message);
    }

    fn fail(&mut self, request: &Value, text: &str) {
        self.send(json!({
            "type": "response",
            "request_seq": request["seq"],
            "success": false,
            "command": request["command"],
            "message": text,
            "body": { "error": { "id": 1, "format": text } },
        }));
    }

    fn event(&mut self, event: &str, body: Value) {
        let mut message = json!({ "type": "event", "event": event });
        if !body.is_null() {
            message["body"] = body;
        }
        self.send(message);
    }

    fn handle(&mut self, script: &Script, request: &Value) -> bool {
        let command = request["command"].as_str().unwrap_or_default();
        if script.silent.iter().any(|silent| silent == command) {
            return true;
        }
        let arguments = &request["arguments"];
        match command {
            "initialize" => {
                let mut capabilities = json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsConditionalBreakpoints": true,
                    "supportsEvaluateForHovers": true,
                    "supportsTerminateRequest": true,
                    "supportTerminateDebuggee": true,
                });
                if let (Some(all), Some(extra)) = (capabilities.as_object_mut(), script.capabilities.as_object()) {
                    all.extend(extra.clone());
                }
                self.respond(request, capabilities);
            }
            "launch" | "attach" => {
                if let Some(reverse) = &script.reverse_request {
                    self.send(json!({ "type": "request", "command": reverse, "arguments": {} }));
                }
                if let Some(failure) = &script.launch_failure {
                    self.fail(request, failure);
                } else if script.order == InitializedOrder::BeforeLaunchResponse {
                    self.event("initialized", Value::Null);
                    self.respond(request, Value::Null);
                } else {
                    self.respond(request, Value::Null);
                    self.event("initialized", Value::Null);
                }
            }
            "setBreakpoints" => {
                let path = arguments["source"]["path"].as_str().unwrap_or_default().to_owned();
                let mut set = Vec::new();
                let mut answer = Vec::new();
                for breakpoint in arguments["breakpoints"].as_array().into_iter().flatten() {
                    let line = breakpoint["line"].as_i64().unwrap_or_default();
                    let verified = match &script.verified {
                        Some(lines) => lines.contains(&line),
                        None => script.program.iter().any(|(file, at)| *file == path && *at == line),
                    };
                    let id = self.next_id;
                    self.next_id += 1;
                    if verified {
                        let log = breakpoint["logMessage"]
                            .as_str()
                            .filter(|_| script.capabilities["supportsLogPoints"] == true)
                            .map(str::to_owned);
                        let hit = breakpoint["hitCondition"]
                            .as_str()
                            .filter(|_| script.capabilities["supportsHitConditionalBreakpoints"] == true)
                            .and_then(|hit| hit.trim().parse().ok());
                        set.push(Set { id, line, log, hit });
                        answer.push(json!({ "id": id, "verified": true, "line": line, "source": { "path": path } }));
                    } else {
                        answer.push(json!({ "id": id, "verified": false, "line": line, "message": "no code here" }));
                    }
                }
                self.breakpoints.insert(path, set);
                self.respond(request, json!({ "breakpoints": answer }));
            }
            "setExceptionBreakpoints" => {
                let count = arguments["filters"].as_array().map_or(0, Vec::len)
                    + arguments["filterOptions"].as_array().map_or(0, Vec::len);
                let answer: Vec<Value> = (0..count).map(|_| json!({ "verified": true })).collect();
                self.respond(request, json!({ "breakpoints": answer }));
            }
            "setFunctionBreakpoints" => {
                let mut answer = Vec::new();
                self.functions.clear();
                for breakpoint in arguments["breakpoints"].as_array().into_iter().flatten() {
                    let name = breakpoint["name"].as_str().unwrap_or_default();
                    let id = self.next_id;
                    self.next_id += 1;
                    match script.functions.iter().find(|(known, _)| known == name) {
                        Some((_, at)) => {
                            self.functions.push((id, *at));
                            answer.push(json!({ "id": id, "verified": true }));
                        }
                        None => answer.push(json!({ "id": id, "verified": false, "message": "no such function" })),
                    }
                }
                self.respond(request, json!({ "breakpoints": answer }));
            }
            "dataBreakpointInfo" => {
                let name = arguments["name"].as_str().unwrap_or_default();
                let reference = arguments["variablesReference"].as_i64().unwrap_or_default();
                self.respond(
                    request,
                    json!({
                        "dataId": format!("{reference}/{name}"),
                        "description": name,
                        "accessTypes": ["write", "read", "readWrite"],
                        "canPersist": false,
                    }),
                );
            }
            "setDataBreakpoints" => {
                let answer: Vec<Value> = arguments["breakpoints"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|_| {
                        let id = self.next_id;
                        self.next_id += 1;
                        json!({ "id": id, "verified": true })
                    })
                    .collect();
                self.respond(request, json!({ "breakpoints": answer }));
            }
            "configurationDone" => {
                self.respond(request, Value::Null);
                for (category, output) in &script.output {
                    self.event("output", json!({ "category": category, "output": output }));
                }
                if !script.hold {
                    self.run(script, 0);
                }
            }
            "threads" => self.respond(request, json!({ "threads": script.threads })),
            "stackTrace" => {
                let frames = self.frames(script);
                let total = frames.len();
                let start = usize::try_from(arguments["startFrame"].as_i64().unwrap_or(0)).unwrap_or(0).min(total);
                let levels = usize::try_from(arguments["levels"].as_i64().unwrap_or(0)).unwrap_or(0);
                let end = if levels == 0 { total } else { (start + levels).min(total) };
                self.respond(request, json!({ "stackFrames": frames[start..end], "totalFrames": total }));
            }
            "scopes" => self.respond(
                request,
                json!({ "scopes": [{ "name": "Locals", "variablesReference": 1, "expensive": false }] }),
            ),
            "variables" => {
                let table = self.table(script);
                let reference = usize::try_from(arguments["variablesReference"].as_i64().unwrap_or(0)).unwrap_or(0);
                let children = reference.checked_sub(1).and_then(|index| table.get(index)).cloned().unwrap_or_default();
                let start = usize::try_from(arguments["start"].as_i64().unwrap_or(0)).unwrap_or(0).min(children.len());
                let count = usize::try_from(arguments["count"].as_i64().unwrap_or(0)).unwrap_or(0);
                let end = if count == 0 { children.len() } else { (start + count).min(children.len()) };
                self.respond(request, json!({ "variables": children[start..end] }));
            }
            "evaluate"
                if script.evaluation_frames.as_ref().is_some_and(|frames| {
                    !frames.iter().any(|frame| Some(*frame) == arguments["frameId"].as_i64())
                }) =>
            {
                self.fail(request, "Cannot evaluate because of compilation error(s): not in this frame");
            }
            "evaluate" => {
                let expression = arguments["expression"].as_str().unwrap_or_default();
                let found = self
                    .table(script)
                    .into_iter()
                    .flatten()
                    .find(|variable| variable.evaluate_name.as_deref() == Some(expression));
                match found {
                    Some(variable) => self.respond(
                        request,
                        json!({
                            "result": variable.value,
                            "type": variable.type_,
                            "variablesReference": variable.variables_reference,
                            "indexedVariables": variable.indexed_variables,
                            "presentationHint": variable.presentation_hint,
                        }),
                    ),
                    None => self.fail(request, &format!("could not find symbol value for {expression}")),
                }
            }
            "setVariable" | "setExpression" => {
                let capability = if command == "setVariable" { "supportsSetVariable" } else { "supportsSetExpression" };
                let value = arguments["value"].as_str().unwrap_or_default().to_owned();
                let table = self.table(script);
                let found = if command == "setVariable" {
                    let reference = usize::try_from(arguments["variablesReference"].as_i64().unwrap_or(0)).unwrap_or(0);
                    let name = arguments["name"].as_str().unwrap_or_default();
                    reference
                        .checked_sub(1)
                        .and_then(|index| table.get(index))
                        .and_then(|level| level.iter().find(|variable| variable.name == name))
                        .cloned()
                } else {
                    let expression = arguments["expression"].as_str().unwrap_or_default();
                    table.into_iter().flatten().find(|variable| variable.evaluate_name.as_deref() == Some(expression))
                };
                match found {
                    _ if script.capabilities[capability] != true => {
                        self.fail(request, &format!("unknown command {command}"))
                    }
                    _ if value.trim().is_empty() || value.contains('?') => {
                        self.fail(request, &format!("cannot assign {value}"));
                    }
                    Some(variable) => {
                        let key = variable.evaluate_name.clone().unwrap_or_default();
                        self.assigned.insert(key, value.clone());
                        self.respond(
                            request,
                            json!({ "value": value, "type": variable.type_, "variablesReference": variable.variables_reference }),
                        );
                    }
                    None => self.fail(request, "no such variable"),
                }
            }
            "continue" => {
                let alone = arguments["singleThread"] == true;
                self.respond(request, json!({ "allThreadsContinued": !alone }));
                if !script.hold && !alone {
                    let from = self.at.map_or(0, |at| at + 1);
                    self.run(script, from);
                }
            }
            "stepInTargets" | "restartFrame" if script.capabilities[capability_of(command)] != true => {
                self.fail(request, &format!("unknown command {command}"));
            }
            "stepInTargets" => self.respond(request, json!({ "targets": script.step_targets })),
            "restartFrame" => {
                let known = self.frames(script).iter().any(|frame| Some(frame.id) == arguments["frameId"].as_i64());
                if known {
                    self.respond(request, Value::Null);
                    self.stop(script, self.at.unwrap_or(0), "restart", None);
                } else {
                    self.fail(request, "this frame cannot be restarted");
                }
            }
            "next" | "stepIn" | "stepOut" => {
                self.respond(request, Value::Null);
                let next = self.at.map_or(0, |at| at + 1);
                if next < script.program.len() {
                    self.stop(script, next, "step", None);
                } else {
                    self.end(script);
                }
            }
            "pause" => {
                self.respond(request, Value::Null);
                self.stop(script, self.at.unwrap_or(0), "pause", None);
            }
            "terminate" => {
                self.respond(request, Value::Null);
                self.end(script);
            }
            "disconnect" => {
                self.respond(request, Value::Null);
                return false;
            }
            other if script.views.as_deref() == Some(other) => {
                self.raw = arguments["raw"] == true;
                self.respond(request, Value::Null);
            }
            other => self.fail(request, &format!("unknown command {other}")),
        }
        true
    }

    fn run(&mut self, script: &Script, from: usize) {
        for index in from..script.program.len() {
            let (path, line) = &script.program[index];
            let found = self.breakpoints.get(path).and_then(|set| set.iter().find(|set| set.line == *line)).cloned();
            if let Some(set) = found {
                let count = self.hits.entry(set.id).or_default();
                *count += 1;
                if set.hit.is_some_and(|wanted| wanted != *count) {
                    continue;
                }
                if let Some(log) = &set.log {
                    self.event("output", json!({ "category": "console", "output": format!("{log}\n") }));
                    continue;
                }
                self.stop(script, index, "breakpoint", Some(set.id));
                return;
            }
            if let Some((id, _)) = self.functions.iter().find(|(_, at)| *at == index).copied() {
                self.stop(script, index, "function breakpoint", Some(id));
                return;
            }
        }
        self.end(script);
    }

    fn stop(&mut self, script: &Script, at: usize, reason: &str, breakpoint: Option<i64>) {
        self.at = Some(at);
        self.stops += 1;
        let thread = script.threads.first().map_or(1, |thread| thread.id);
        let mut body = json!({ "reason": reason, "threadId": thread, "allThreadsStopped": true });
        if let Some(id) = breakpoint {
            body["hitBreakpointIds"] = json!([id]);
        }
        self.event("stopped", body);
    }

    fn end(&mut self, script: &Script) {
        self.at = None;
        self.event("exited", json!({ "exitCode": script.exit_code }));
        self.event("terminated", Value::Null);
    }

    fn table(&self, script: &Script) -> Vec<Vec<Variable>> {
        let mut table = table(&script.variables, &script.evaluations, self.raw);
        for variable in table.iter_mut().flatten() {
            if let Some(value) = variable.evaluate_name.as_ref().and_then(|name| self.assigned.get(name)) {
                variable.value = value.clone();
            }
        }
        table
    }

    fn frames(&self, script: &Script) -> Vec<StackFrame> {
        if let Some(frames) = self.stops.checked_sub(1).and_then(|stop| script.frames.get(stop)) {
            return frames.clone();
        }
        let Some((path, line)) = self.at.and_then(|at| script.program.get(at)) else { return Vec::new() };
        vec![StackFrame {
            id: 1000,
            name: "main.main".to_owned(),
            source: Some(Source::path(path)),
            line: *line,
            column: 1,
            ..StackFrame::default()
        }]
    }
}

fn capability_of(command: &str) -> &'static str {
    if command == "stepInTargets" { "supportsStepInTargetsRequest" } else { "supportsRestartFrame" }
}

fn table(variables: &[FakeVariable], evaluations: &[(String, FakeVariable)], raw: bool) -> Vec<Vec<Variable>> {
    let mut table = Vec::new();
    flatten(variables, "", raw, &mut table);
    let named: Vec<FakeVariable> = evaluations
        .iter()
        .map(|(expression, value)| FakeVariable { name: expression.clone(), ..value.clone() })
        .collect();
    let slot = flatten(&named, "", raw, &mut table);
    for variable in &mut table[slot] {
        variable.evaluate_name = Some(variable.name.clone());
    }
    table
}

fn flatten(variables: &[FakeVariable], prefix: &str, raw: bool, table: &mut Vec<Vec<Variable>>) -> usize {
    let slot = table.len();
    table.push(Vec::new());
    let mut level = Vec::with_capacity(variables.len());
    for variable in variables {
        let name = if prefix.is_empty() { variable.name.clone() } else { format!("{prefix}.{}", variable.name) };
        let children = match (&variable.raw, raw) {
            (Some(fields), true) => fields,
            _ => &variable.children,
        };
        let reference = if children.is_empty() { 0 } else { flatten(children, &name, raw, table) + 1 };
        let count = (!children.is_empty()).then(|| i64::try_from(children.len()).unwrap_or_default());
        let indexed = variable.indexed && !(raw && variable.raw.is_some());
        let shown = Variable {
            name: variable.name.clone(),
            value: variable.value.clone(),
            type_: variable.type_.clone(),
            variables_reference: i64::try_from(reference).unwrap_or_default(),
            indexed_variables: if indexed { count } else { None },
            named_variables: if indexed { None } else { count },
            evaluate_name: Some(name.clone()),
            presentation_hint: None,
        };
        match &variable.lazy {
            Some(placeholder) => {
                let slot = table.len();
                table.push(vec![Variable { name: String::new(), ..shown.clone() }]);
                level.push(Variable {
                    value: placeholder.clone(),
                    variables_reference: i64::try_from(slot + 1).unwrap_or_default(),
                    indexed_variables: None,
                    named_variables: None,
                    presentation_hint: Some(VariablePresentationHint {
                        lazy: true,
                        ..VariablePresentationHint::default()
                    }),
                    ..shown
                });
            }
            None => level.push(shown),
        }
    }
    table[slot] = level;
    slot
}

pub fn program_path(name: &str) -> PathBuf {
    PathBuf::from("/fake/src").join(name)
}
