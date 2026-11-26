use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::DapError;
use crate::connection::{Connection, Incoming, Pending};
use crate::protocol::{
    Ack, Breakpoint, BreakpointsResponse, Capabilities, ContinueResponse, DataBreakpoint, DataBreakpointInfoArguments,
    DataBreakpointInfoResponse, DebugEvent, DisconnectArguments, EvaluateArguments, EvaluateResponse, FrameArguments,
    FunctionBreakpoint, InitializeArguments, ScopesArguments, ScopesResponse, SetBreakpointsArguments,
    SetBreakpointsResponse, SetDataBreakpointsArguments, SetExceptionBreakpointsArguments, SetExpressionArguments,
    SetFunctionBreakpointsArguments, SetValueResponse, SetVariableArguments, Source, SourceBreakpoint,
    StackTraceArguments, StackTraceResponse, StepInArguments, StepInTargetsResponse, TerminateArguments,
    ThreadArguments, ThreadsResponse, VariablesArguments, VariablesResponse,
};

const POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Handshake {
    pub capabilities: Capabilities,
    pub breakpoints: Vec<(PathBuf, Vec<Breakpoint>)>,
    pub functions: Vec<Breakpoint>,
    pub data: Vec<Breakpoint>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Configuration {
    pub breakpoints: Vec<(PathBuf, Vec<SourceBreakpoint>)>,
    pub functions: Vec<FunctionBreakpoint>,
    pub exceptions: SetExceptionBreakpointsArguments,
    pub data: Vec<DataBreakpoint>,
}

pub trait SourceMap: Send + Sync {
    fn split(&self, path: &Path, breakpoints: &[SourceBreakpoint]) -> Option<Vec<(PathBuf, Vec<usize>)>>;

    fn prepare(&self, _launch: &mut Value) {}
}

pub struct Client {
    connection: Connection,
    adapter_id: String,
    capabilities: Capabilities,
    backlog: Mutex<VecDeque<Incoming>>,
    source_map: Option<Arc<dyn SourceMap>>,
    mapped: Mutex<HashMap<PathBuf, Vec<PathBuf>>>,
}

impl Client {
    pub fn new(connection: Connection, adapter_id: &str) -> Self {
        Self {
            connection,
            adapter_id: adapter_id.to_owned(),
            capabilities: Capabilities::default(),
            backlog: Mutex::default(),
            source_map: None,
            mapped: Mutex::default(),
        }
    }

    pub fn has_source_map(&self) -> bool {
        self.source_map.is_some()
    }

    pub fn set_source_map(&mut self, map: Option<Arc<dyn SourceMap>>) {
        self.source_map = map;
    }

    pub fn put_breakpoints(
        &self,
        path: &Path,
        breakpoints: &[SourceBreakpoint],
        timeout: Duration,
    ) -> Result<Vec<Breakpoint>, DapError> {
        let deadline = Instant::now() + timeout;
        let Some(parts) = self.source_map.as_ref().and_then(|map| map.split(path, breakpoints)) else {
            return Ok(self.set_breakpoints(&path.to_string_lossy(), breakpoints)?.wait(timeout)?.breakpoints);
        };
        let targets: Vec<PathBuf> = parts.iter().map(|(target, _)| target.clone()).collect();
        let before = self
            .mapped
            .lock()
            .map(|mut mapped| mapped.insert(path.to_owned(), targets.clone()).unwrap_or_default())
            .unwrap_or_default();
        for gone in before.iter().filter(|old| !targets.contains(old)) {
            self.set_breakpoints(&gone.to_string_lossy(), &[])?.wait(remaining(deadline))?;
        }
        let mut set = vec![Breakpoint::default(); breakpoints.len()];
        for (target, indices) in parts {
            let asked: Vec<SourceBreakpoint> =
                indices.iter().filter_map(|index| breakpoints.get(*index).cloned()).collect();
            let answer = self.set_breakpoints(&target.to_string_lossy(), &asked)?.wait(remaining(deadline))?;
            for (index, mut breakpoint) in indices.into_iter().zip(answer.breakpoints) {
                breakpoint.source = Some(Source::path(&path.to_string_lossy()));
                if let Some(slot) = set.get_mut(index) {
                    *slot = breakpoint;
                }
            }
        }
        Ok(set)
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub fn is_alive(&self) -> bool {
        self.connection.is_alive()
    }

    pub fn handshake(
        &mut self,
        launch: &Value,
        breakpoints: &[(PathBuf, Vec<SourceBreakpoint>)],
        timeout: Duration,
    ) -> Result<Handshake, DapError> {
        self.handshake_with(launch, breakpoints, &[], timeout)
    }

    pub fn handshake_with(
        &mut self,
        launch: &Value,
        breakpoints: &[(PathBuf, Vec<SourceBreakpoint>)],
        filters: &[String],
        timeout: Duration,
    ) -> Result<Handshake, DapError> {
        self.configure(
            launch,
            |capabilities| Configuration {
                breakpoints: breakpoints.to_vec(),
                exceptions: SetExceptionBreakpointsArguments {
                    filters: filters
                        .iter()
                        .filter(|filter| {
                            capabilities.exception_breakpoint_filters.iter().any(|known| known.filter == **filter)
                        })
                        .cloned()
                        .collect(),
                    filter_options: None,
                },
                ..Configuration::default()
            },
            timeout,
        )
    }

    pub fn configure(
        &mut self,
        launch: &Value,
        plan: impl FnOnce(&Capabilities) -> Configuration,
        timeout: Duration,
    ) -> Result<Handshake, DapError> {
        let deadline = Instant::now() + timeout;
        let capabilities = self.initialize()?.wait(remaining(deadline))?;
        self.capabilities = capabilities.clone();
        let configuration = plan(&capabilities);
        let mut launch = launch.clone();
        if let Some(map) = &self.source_map {
            map.prepare(&mut launch);
        }
        let launched = if launch.get("request").and_then(Value::as_str) == Some("attach") {
            self.attach(launch)?
        } else {
            self.launch(launch)?
        };
        let mut answered = false;
        let mut initialized = false;
        while !initialized {
            if !answered && let Some(answer) = launched.try_answer() {
                answer?;
                answered = true;
            }
            match self.connection.next_incoming(remaining(deadline).min(POLL)) {
                Some(Incoming::Event(DebugEvent::Initialized)) => initialized = true,
                Some(Incoming::Closed) => {
                    if !answered && let Some(Err(error)) = launched.try_answer() {
                        return Err(error);
                    }
                    self.keep(Incoming::Closed);
                    return Err(DapError::Closed);
                }
                Some(other) => self.keep(other),
                None if Instant::now() >= deadline => return Err(DapError::Timeout),
                None => {}
            }
        }
        let mut set = Vec::with_capacity(configuration.breakpoints.len());
        for (path, lines) in &configuration.breakpoints {
            set.push((path.clone(), self.put_breakpoints(path, lines, remaining(deadline))?));
        }
        let functions = if capabilities.supports_function_breakpoints && !configuration.functions.is_empty() {
            self.set_function_breakpoints(&configuration.functions)?.wait(remaining(deadline))?.breakpoints
        } else {
            Vec::new()
        };
        let data = if capabilities.supports_data_breakpoints && !configuration.data.is_empty() {
            self.set_data_breakpoints(&configuration.data)?.wait(remaining(deadline))?.breakpoints
        } else {
            Vec::new()
        };
        self.set_exception_breakpoints_with(&configuration.exceptions)?.wait(remaining(deadline))?;
        self.configuration_done()?.wait(remaining(deadline))?;
        if !answered {
            launched.wait(remaining(deadline))?;
        }
        Ok(Handshake { capabilities, breakpoints: set, functions, data })
    }

    fn keep(&self, incoming: Incoming) {
        if let Ok(mut backlog) = self.backlog.lock() {
            backlog.push_back(incoming);
        }
    }

    pub fn take_incoming(&self) -> Vec<Incoming> {
        let mut events: Vec<Incoming> =
            self.backlog.lock().map(|mut backlog| backlog.drain(..).collect()).unwrap_or_default();
        events.extend(self.connection.take_incoming());
        events
    }

    pub fn next_incoming(&self, timeout: Duration) -> Option<Incoming> {
        if let Some(kept) = self.backlog.lock().ok().and_then(|mut backlog| backlog.pop_front()) {
            return Some(kept);
        }
        self.connection.next_incoming(timeout)
    }

    pub fn request(&self, command: &str, arguments: Value) -> Result<Pending<Value>, DapError> {
        self.connection.request(command, arguments)
    }

    fn typed<A: Serialize, T: DeserializeOwned>(&self, command: &str, arguments: &A) -> Result<Pending<T>, DapError> {
        let arguments = serde_json::to_value(arguments).map_err(|error| DapError::Io(error.to_string()))?;
        Ok(self.connection.request(command, arguments)?.typed())
    }

    pub fn initialize(&self) -> Result<Pending<Capabilities>, DapError> {
        self.typed("initialize", &InitializeArguments::zen(&self.adapter_id))
    }

    pub fn launch(&self, arguments: Value) -> Result<Pending<Ack>, DapError> {
        Ok(self.connection.request("launch", arguments)?.typed())
    }

    pub fn attach(&self, arguments: Value) -> Result<Pending<Ack>, DapError> {
        Ok(self.connection.request("attach", arguments)?.typed())
    }

    pub fn set_breakpoints(
        &self,
        path: &str,
        breakpoints: &[SourceBreakpoint],
    ) -> Result<Pending<SetBreakpointsResponse>, DapError> {
        let arguments = SetBreakpointsArguments {
            source: Source::path(path),
            breakpoints: breakpoints.to_vec(),
            source_modified: None,
        };
        self.typed("setBreakpoints", &arguments)
    }

    pub fn set_exception_breakpoints(&self, filters: &[String]) -> Result<Pending<Ack>, DapError> {
        self.set_exception_breakpoints_with(&SetExceptionBreakpointsArguments {
            filters: filters.to_vec(),
            filter_options: None,
        })
    }

    pub fn set_exception_breakpoints_with(
        &self,
        arguments: &SetExceptionBreakpointsArguments,
    ) -> Result<Pending<Ack>, DapError> {
        self.typed("setExceptionBreakpoints", arguments)
    }

    pub fn set_function_breakpoints(
        &self,
        breakpoints: &[FunctionBreakpoint],
    ) -> Result<Pending<BreakpointsResponse>, DapError> {
        self.typed("setFunctionBreakpoints", &SetFunctionBreakpointsArguments { breakpoints: breakpoints.to_vec() })
    }

    pub fn data_breakpoint_info(
        &self,
        variables_reference: Option<i64>,
        name: &str,
        frame_id: Option<i64>,
    ) -> Result<Pending<DataBreakpointInfoResponse>, DapError> {
        let arguments = DataBreakpointInfoArguments { variables_reference, name: name.to_owned(), frame_id };
        self.typed("dataBreakpointInfo", &arguments)
    }

    pub fn set_data_breakpoints(
        &self,
        breakpoints: &[DataBreakpoint],
    ) -> Result<Pending<BreakpointsResponse>, DapError> {
        self.typed("setDataBreakpoints", &SetDataBreakpointsArguments { breakpoints: breakpoints.to_vec() })
    }

    pub fn configuration_done(&self) -> Result<Pending<Ack>, DapError> {
        Ok(self.connection.request("configurationDone", Value::Null)?.typed())
    }

    pub fn threads(&self) -> Result<Pending<ThreadsResponse>, DapError> {
        Ok(self.connection.request("threads", Value::Null)?.typed())
    }

    pub fn stack_trace(
        &self,
        thread_id: i64,
        start_frame: Option<i64>,
        levels: Option<i64>,
    ) -> Result<Pending<StackTraceResponse>, DapError> {
        self.typed("stackTrace", &StackTraceArguments { thread_id, start_frame, levels })
    }

    pub fn scopes(&self, frame_id: i64) -> Result<Pending<ScopesResponse>, DapError> {
        self.typed("scopes", &ScopesArguments { frame_id })
    }

    pub fn variables(
        &self,
        variables_reference: i64,
        start: Option<i64>,
        count: Option<i64>,
    ) -> Result<Pending<VariablesResponse>, DapError> {
        self.typed("variables", &VariablesArguments { variables_reference, start, count })
    }

    pub fn evaluate(
        &self,
        expression: &str,
        frame_id: Option<i64>,
        context: Option<&str>,
    ) -> Result<Pending<EvaluateResponse>, DapError> {
        let arguments =
            EvaluateArguments { expression: expression.to_owned(), frame_id, context: context.map(str::to_owned) };
        self.typed("evaluate", &arguments)
    }

    pub fn set_variable(
        &self,
        variables_reference: i64,
        name: &str,
        value: &str,
    ) -> Result<Pending<SetValueResponse>, DapError> {
        let arguments = SetVariableArguments { variables_reference, name: name.to_owned(), value: value.to_owned() };
        self.typed("setVariable", &arguments)
    }

    pub fn set_expression(
        &self,
        expression: &str,
        value: &str,
        frame_id: Option<i64>,
    ) -> Result<Pending<SetValueResponse>, DapError> {
        let arguments = SetExpressionArguments { expression: expression.to_owned(), value: value.to_owned(), frame_id };
        self.typed("setExpression", &arguments)
    }

    pub fn continue_(&self, thread_id: i64) -> Result<Pending<ContinueResponse>, DapError> {
        self.typed("continue", &ThreadArguments { thread_id, single_thread: None })
    }

    pub fn continue_alone(&self, thread_id: i64) -> Result<Pending<ContinueResponse>, DapError> {
        self.typed("continue", &ThreadArguments { thread_id, single_thread: Some(true) })
    }

    pub fn next(&self, thread_id: i64) -> Result<Pending<Ack>, DapError> {
        self.typed("next", &ThreadArguments { thread_id, single_thread: None })
    }

    pub fn step_in(&self, thread_id: i64) -> Result<Pending<Ack>, DapError> {
        self.typed("stepIn", &ThreadArguments { thread_id, single_thread: None })
    }

    pub fn step_in_targets(&self, frame_id: i64) -> Result<Pending<StepInTargetsResponse>, DapError> {
        self.typed("stepInTargets", &FrameArguments { frame_id })
    }

    pub fn step_into_target(&self, thread_id: i64, target_id: i64) -> Result<Pending<Ack>, DapError> {
        self.typed("stepIn", &StepInArguments { thread_id, target_id: Some(target_id) })
    }

    pub fn restart_frame(&self, frame_id: i64) -> Result<Pending<Ack>, DapError> {
        self.typed("restartFrame", &FrameArguments { frame_id })
    }

    pub fn step_out(&self, thread_id: i64) -> Result<Pending<Ack>, DapError> {
        self.typed("stepOut", &ThreadArguments { thread_id, single_thread: None })
    }

    pub fn pause(&self, thread_id: i64) -> Result<Pending<Ack>, DapError> {
        self.typed("pause", &ThreadArguments { thread_id, single_thread: None })
    }

    pub fn terminate(&self) -> Result<Pending<Ack>, DapError> {
        self.typed("terminate", &TerminateArguments::default())
    }

    pub fn disconnect(&self, terminate_debuggee: bool) -> Result<Pending<Ack>, DapError> {
        self.typed("disconnect", &DisconnectArguments { restart: None, terminate_debuggee: Some(terminate_debuggee) })
    }

    pub fn close(&self) {
        self.connection.close();
    }
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}
