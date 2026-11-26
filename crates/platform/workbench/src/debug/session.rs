use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::CommandError;
use zen_dap::protocol::{OutputEvent, StoppedEvent};
use zen_dap::{
    Client, Connection, DapError, DataBreakpoint, DebugEvent, FunctionBreakpoint, Incoming, ReverseListener,
    SetExceptionBreakpointsArguments, SourceBreakpoint, StackFrame, Thread,
};
use zen_i18n::t;
use zen_run::{AdapterLine, Process, ProcessEvent, ProcessSink, RunProvider, RunState, SharedOutput, Stream, Waker};

use super::breakpoints::{Breakpoints, Plan, Suspend, hit_message};
use crate::run::{Driver, RunControl};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub const FRAME_PAGE: usize = 20;

const ASKED_LIMIT: usize = 64;

const ACCEPT_TIMEOUT: Duration = Duration::from_secs(30);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_GRACE: Duration = Duration::from_secs(2);
const DEBUGGEE_GRACE: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DebugState {
    #[default]
    Starting,
    Running,
    Stopped {
        reason: String,
        thread_id: Option<i64>,
        description: Option<String>,
    },
    Ended {
        reason: String,
    },
}

impl DebugState {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopped { .. } => "stopped",
            Self::Ended { .. } => "ended",
        }
    }

    pub fn is_live(&self) -> bool {
        !matches!(self, Self::Ended { .. })
    }

    pub fn is_stopped(&self) -> bool {
        matches!(self, Self::Stopped { .. })
    }
}

pub trait AdapterProcess: Send + Sync {
    fn has_exited(&self) -> bool;

    fn kill(&self);
}

impl AdapterProcess for Process {
    fn has_exited(&self) -> bool {
        Process::has_exited(self)
    }

    fn kill(&self) {
        Process::kill(self);
    }
}

pub trait AdapterLauncher: Send + Sync {
    fn launch(
        &self,
        adapter: &AdapterLine,
        env: &[(String, String)],
        address: SocketAddr,
        sink: ProcessSink,
    ) -> Result<Arc<dyn AdapterProcess>, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessLauncher;

impl AdapterLauncher for ProcessLauncher {
    fn launch(
        &self,
        adapter: &AdapterLine,
        env: &[(String, String)],
        address: SocketAddr,
        sink: ProcessSink,
    ) -> Result<Arc<dyn AdapterProcess>, String> {
        if adapter.builtin.is_some() {
            return crate::nodejs::adapter::launch(env, address);
        }
        let mut line = adapter.line.clone();
        line.args.push(format!("{}={address}", adapter.address_flag));
        line.env = env.iter().chain(&adapter.line.env).cloned().collect();
        Ok(Arc::new(Process::spawn(&line, sink)?))
    }
}

#[cfg(any(test, feature = "fake"))]
#[derive(Clone, Default)]
pub struct FakeLauncher {
    adapter: zen_dap::fake::FakeAdapter,
}

#[cfg(any(test, feature = "fake"))]
impl FakeLauncher {
    pub fn new(adapter: zen_dap::fake::FakeAdapter) -> Self {
        Self { adapter }
    }
}

#[cfg(any(test, feature = "fake"))]
struct FakeProcess(std::thread::JoinHandle<()>);

#[cfg(any(test, feature = "fake"))]
impl AdapterProcess for FakeProcess {
    fn has_exited(&self) -> bool {
        self.0.is_finished()
    }

    fn kill(&self) {}
}

#[cfg(any(test, feature = "fake"))]
impl AdapterLauncher for FakeLauncher {
    fn launch(
        &self,
        adapter: &AdapterLine,
        env: &[(String, String)],
        address: SocketAddr,
        sink: ProcessSink,
    ) -> Result<Arc<dyn AdapterProcess>, String> {
        if adapter.line.program != Path::new(zen_run::fake::FAKE_ADAPTER) {
            return ProcessLauncher.launch(adapter, env, address, sink);
        }
        let thread = self.adapter.connect(address).map_err(|error| error.to_string())?;
        Ok(Arc::new(FakeProcess(thread)))
    }
}

/// A frame of a stack, with lines and columns from 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FrameInfo {
    /// The frame's id, for `debug.frame.select`, `debug.variables.list` and `debug.expression.evaluate`.
    pub id: i64,
    /// The function's name.
    pub name: String,
    /// The file; absent when the frame has no source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The line, from 0.
    pub line: u32,
    /// The column, from 0.
    pub column: u32,
    /// `label` for a separator the debugger puts above the frames of an awaited or scheduled call, `subtle` for a frame it plays down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Whether the frame is below such a separator: part of the async stack trace.
    #[serde(rename = "async", skip_serializing_if = "std::ops::Not::not")]
    pub is_async: bool,
    /// Whether `debug.frame.reset` can run the frame again; absent when it cannot.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub can_restart: bool,
}

impl FrameInfo {
    fn new(frame: &StackFrame, restart: bool, is_async: bool) -> Self {
        let from_zero = |value: i64| u32::try_from(value.saturating_sub(1).max(0)).unwrap_or(u32::MAX);
        let hint = frame.presentation_hint.clone().filter(|hint| hint == "label" || hint == "subtle");
        let label = hint.as_deref() == Some("label");
        Self {
            id: frame.id,
            name: frame.name.clone(),
            path: frame
                .source
                .as_ref()
                .and_then(|source| source.path.as_ref())
                .map(PathBuf::from)
                .map(|path| std::fs::read_link(&path).unwrap_or(path)),
            line: from_zero(frame.line),
            column: from_zero(frame.column),
            can_restart: restart && !label && !is_async && frame.can_restart != Some(false),
            hint,
            is_async,
        }
    }
}

fn frame_infos(frames: &[StackFrame], restart: bool) -> Vec<FrameInfo> {
    let mut below_label = false;
    frames
        .iter()
        .map(|frame| {
            let info = FrameInfo::new(frame, restart, below_label);
            below_label |= frame.presentation_hint.as_deref() == Some("label");
            info
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionView {
    pub state: DebugState,
    pub threads: Vec<Thread>,
    pub thread_id: Option<i64>,
    pub frames: Vec<FrameInfo>,
    pub frame_id: Option<i64>,
    pub revision: u64,
    pub stops: u64,
    pub values: u64,
}

pub(crate) struct Connect {
    pub(crate) adapter: AdapterLine,
    pub(crate) launch: serde_json::Value,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) launcher: Arc<dyn AdapterLauncher>,
    pub(crate) model: Breakpoints,
    pub(crate) provider: Arc<dyn RunProvider>,
    pub(crate) connect: Option<SocketAddr>,
    pub(crate) source_map: Option<Arc<dyn zen_dap::SourceMap>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Sent {
    pub(crate) asked: Vec<SourceBreakpoint>,
    pub(crate) set: Vec<zen_dap::Breakpoint>,
    pub(crate) plans: Vec<Plan>,
}

type Asking = (Vec<SourceBreakpoint>, Vec<Plan>, zen_dap::Pending<zen_dap::protocol::SetBreakpointsResponse>);

#[derive(Clone, Debug, Default)]
struct SentOthers {
    functions: Option<Vec<FunctionBreakpoint>>,
    function_set: Vec<zen_dap::Breakpoint>,
    exceptions: Option<SetExceptionBreakpointsArguments>,
    data: Option<Vec<DataBreakpoint>>,
    data_set: Vec<zen_dap::Breakpoint>,
}

pub(crate) struct Synced {
    pub(crate) lines: Vec<(PathBuf, u32, bool, Option<String>)>,
    pub(crate) functions: Vec<(String, bool, Option<String>)>,
    pub(crate) data: Vec<(String, bool)>,
}

#[derive(Default)]
struct InlineMemory {
    stop: u64,
    frame: String,
    values: HashMap<String, String>,
    before: HashMap<String, String>,
}

#[derive(Clone, Default)]
struct Frames {
    frames: Vec<StackFrame>,
    total: Option<i64>,
}

#[derive(Default)]
struct Inner {
    state: DebugState,
    client: Option<Arc<Client>>,
    adapter: Option<Arc<dyn AdapterProcess>>,
    threads: Vec<Thread>,
    thread_id: Option<i64>,
    frames: HashMap<i64, Frames>,
    frame_id: Option<i64>,
    variables: HashMap<VariablesKey, Vec<zen_dap::Variable>>,
    scopes: HashMap<i64, Vec<zen_dap::Scope>>,
    asked: HashMap<String, zen_dap::Pending<serde_json::Value>>,
    answered: HashMap<String, Result<serde_json::Value, String>>,
    files: HashMap<PathBuf, Sent>,
    asking: HashMap<PathBuf, Asking>,
    refused: HashMap<PathBuf, Vec<SourceBreakpoint>>,
    others: SentOthers,
    hits: Vec<(PathBuf, u32)>,
    events: Vec<zen_dap::Breakpoint>,
    restore: Vec<PathBuf>,
    restore_others: bool,
    exit_code: Option<i64>,
    partial: HashMap<Stream, String>,
    revision: u64,
    stops: u64,
    values: u64,
    inline: InlineMemory,
    hot_code_replace: bool,
    reloaded: Option<Reload>,
    unreported: Vec<Reload>,
    views: Option<Views>,
}

type VariablesKey = (i64, Option<i64>, Option<i64>, bool);

struct Views {
    setup: zen_run::AdapterViews,
    server: Option<String>,
    shown: Option<bool>,
    switching: Option<(bool, Switch)>,
    inflight: Vec<VariablesKey>,
}

pub(crate) enum Switch {
    Server(zen_lsp::Pending),
    Adapter(zen_dap::Pending<serde_json::Value>),
}

impl Switch {
    fn is_answered(&self) -> bool {
        match self {
            Self::Server(pending) => pending.try_answer().is_some(),
            Self::Adapter(pending) => pending.try_answer().is_some(),
        }
    }
}

pub(crate) enum ViewGate {
    Ready,
    Waiting,
    Switch { command: String, arguments: Vec<serde_json::Value>, server: Option<String> },
}

impl Inner {
    fn can_restart(&self) -> bool {
        self.client.as_ref().is_some_and(|client| client.capabilities().supports_restart_frame)
    }
}

/// What a reload of changed classes did (ADR-0154).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reload {
    /// The classes replaced in the running program, by name.
    pub changed_classes: Vec<String>,
    /// Why it failed, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether it followed a build by itself, rather than Reload Changed Classes.
    pub automatic: bool,
}

const REDEFINE_CLASSES: &str = "redefineClasses";
const RELOAD_TIMEOUT: Duration = Duration::from_secs(60);

enum End {
    Exited,
    Stopped,
    Failed(String),
}

pub struct DebugSession {
    id: u64,
    run_id: AtomicU64,
    name: String,
    output: SharedOutput,
    driver: Arc<Driver>,
    waker: Option<Waker>,
    inner: Mutex<Inner>,
    changed: Condvar,
    stopping: AtomicBool,
    this: Weak<DebugSession>,
}

impl DebugSession {
    pub(crate) fn new(
        id: u64,
        name: String,
        output: SharedOutput,
        driver: Arc<Driver>,
        waker: Option<Waker>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            id,
            run_id: AtomicU64::new(0),
            name,
            output,
            driver,
            waker,
            inner: Mutex::default(),
            changed: Condvar::new(),
            stopping: AtomicBool::new(false),
            this: this.clone(),
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn run_id(&self) -> u64 {
        self.run_id.load(Ordering::SeqCst)
    }

    pub(crate) fn set_run_id(&self, id: u64) {
        self.run_id.store(id, Ordering::SeqCst);
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn output(&self) -> &SharedOutput {
        &self.output
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.state == DebugState::Starting && inner.client.is_none() && inner.adapter.is_none() {
            let reason = match self.output.read(|output| output.state()) {
                RunState::Running => None,
                RunState::Stopped => Some("stopped"),
                RunState::Failed => Some("failed"),
                RunState::BeforeLaunchFailed => Some("beforeLaunchFailed"),
                RunState::Finished => Some("buildFailed"),
            };
            if let Some(reason) = reason {
                inner.state = DebugState::Ended { reason: reason.to_owned() };
                inner.revision += 1;
            }
        }
        inner
    }

    fn notify(&self) {
        self.changed.notify_all();
        if let Some(waker) = &self.waker {
            waker();
        }
    }

    pub fn state(&self) -> DebugState {
        self.lock().state.clone()
    }

    pub fn view(&self) -> SessionView {
        let inner = self.lock();
        let frames = inner
            .thread_id
            .and_then(|thread| inner.frames.get(&thread))
            .map(|frames| frame_infos(&frames.frames, inner.can_restart()))
            .unwrap_or_default();
        SessionView {
            state: inner.state.clone(),
            threads: inner.threads.clone(),
            thread_id: inner.thread_id,
            frames,
            frame_id: inner.frame_id,
            revision: inner.revision,
            stops: inner.stops,
            values: inner.values,
        }
    }

    pub fn selected_frame(&self) -> Option<FrameInfo> {
        let id = self.lock().frame_id?;
        self.frame(id)
    }

    pub(crate) fn frame(&self, id: i64) -> Option<FrameInfo> {
        let inner = self.lock();
        let restart = inner.can_restart();
        inner.frames.values().flat_map(|frames| frame_infos(&frames.frames, restart)).find(|frame| frame.id == id)
    }

    pub(crate) fn wait_while_moving(&self, timeout: Duration) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let deadline = Instant::now() + timeout;
            loop {
                let inner = self.lock();
                let left = deadline.saturating_duration_since(Instant::now());
                let moving = matches!(inner.state, DebugState::Starting | DebugState::Running)
                    || (self.is_stopping() && inner.state.is_live());
                if !moving || left.is_zero() {
                    return;
                }
                let _ = self.changed.wait_timeout(inner, left.min(Duration::from_millis(50)));
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = timeout;
    }

    pub(crate) fn client(&self) -> Result<Arc<Client>, CommandError> {
        let inner = self.lock();
        match (&inner.state, &inner.client) {
            (DebugState::Ended { .. }, _) => Err(CommandError::new(t!("debug-session-ended", id = self.id))),
            (_, Some(client)) => Ok(client.clone()),
            (_, None) => Err(CommandError::new(t!("debug-not-connected"))),
        }
    }

    pub fn reload_classes(&self, automatic: bool) -> Result<Reload, CommandError> {
        let client = self.client()?;
        if !self.lock().hot_code_replace {
            return Err(CommandError::new(t!("debug-reload-unsupported")));
        }
        let answer =
            client.request(REDEFINE_CLASSES, serde_json::json!({})).and_then(|pending| pending.wait(RELOAD_TIMEOUT));
        let reload = match answer {
            Ok(body) => Reload {
                changed_classes: body["changedClasses"]
                    .as_array()
                    .map(|classes| classes.iter().filter_map(|class| class.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default(),
                error: body["errorMessage"].as_str().filter(|message| !message.is_empty()).map(str::to_owned),
                automatic,
            },
            Err(error) => Reload { changed_classes: Vec::new(), error: Some(error_text(&error)), automatic },
        };
        let line = reload_message(&reload);
        self.output.update(|output| output.system(&line));
        let mut inner = self.lock();
        inner.reloaded = Some(reload.clone());
        if automatic && (reload.error.is_some() || !reload.changed_classes.is_empty()) {
            inner.unreported.push(reload.clone());
        }
        inner.revision += 1;
        drop(inner);
        self.notify();
        Ok(reload)
    }

    pub fn reloaded(&self) -> Option<Reload> {
        self.lock().reloaded.clone()
    }

    pub(crate) fn take_unreported(&self) -> Vec<Reload> {
        std::mem::take(&mut self.lock().unreported)
    }

    pub(crate) fn stopped_client(&self) -> Result<Arc<Client>, CommandError> {
        let client = self.client()?;
        if self.lock().state.is_stopped() { Ok(client) } else { Err(CommandError::new(t!("debug-not-stopped"))) }
    }

    pub(crate) fn stopped_thread(&self) -> Option<i64> {
        match &self.lock().state {
            DebugState::Stopped { thread_id, .. } => *thread_id,
            _ => None,
        }
    }

    pub(crate) fn stop_reason(&self) -> Option<String> {
        match &self.lock().state {
            DebugState::Stopped { reason, .. } => Some(reason.clone()),
            _ => None,
        }
    }

    pub(crate) fn thread(&self, wanted: Option<i64>) -> Result<i64, CommandError> {
        if let Some(thread) = wanted {
            return Ok(thread);
        }
        let known = {
            let inner = self.lock();
            let stopped = match &inner.state {
                DebugState::Stopped { thread_id, .. } => *thread_id,
                _ => None,
            };
            inner.thread_id.or(stopped).or_else(|| inner.threads.first().map(|thread| thread.id))
        };
        if let Some(thread) = known {
            return Ok(thread);
        }
        let threads = wait(self.client()?.threads())?.threads;
        let first = threads.first().map(|thread| thread.id);
        self.lock().threads = threads;
        first.ok_or_else(|| CommandError::new(t!("debug-no-thread")))
    }

    pub(crate) fn mark_running(&self) -> DebugState {
        let mut inner = self.lock();
        let before = std::mem::replace(&mut inner.state, DebugState::Running);
        inner.frames.clear();
        inner.frame_id = None;
        inner.variables.clear();
        inner.scopes.clear();
        inner.asked.clear();
        inner.answered.clear();
        inner.revision += 1;
        drop(inner);
        self.notify();
        before
    }

    pub(crate) fn forget_values(&self) {
        let mut inner = self.lock();
        inner.variables.clear();
        inner.asked.clear();
        inner.answered.clear();
        inner.values += 1;
        inner.revision += 1;
        drop(inner);
        self.notify();
    }

    pub(crate) fn scopes_soon(&self, frame: i64, wait: Duration) -> Result<Option<Vec<zen_dap::Scope>>, CommandError> {
        if let Some(cached) = self.lock().scopes.get(&frame) {
            return Ok(Some(cached.clone()));
        }
        let Some(body) = self.ask("scopes", serde_json::json!({ "frameId": frame }), wait)? else { return Ok(None) };
        let scopes = serde_json::from_value::<zen_dap::protocol::ScopesResponse>(body)
            .map_err(|error| debugger_error(DapError::Io(error.to_string())))?
            .scopes;
        self.lock().scopes.insert(frame, scopes.clone());
        Ok(Some(scopes))
    }

    pub(crate) fn values_before(&self, frame: &str, values: &HashMap<String, String>) -> HashMap<String, String> {
        let mut inner = self.lock();
        let stop = inner.stops;
        let memory = &mut inner.inline;
        if memory.stop != stop {
            let same = memory.frame == frame;
            let last = std::mem::take(&mut memory.values);
            memory.before = if same { last } else { HashMap::new() };
            memory.stop = stop;
            memory.frame = frame.to_owned();
        } else if memory.frame != frame {
            memory.before.clear();
            memory.frame = frame.to_owned();
        }
        memory.values.clone_from(values);
        memory.before.clone()
    }

    pub(crate) fn restore_state(&self, before: DebugState) {
        let mut inner = self.lock();
        if inner.state == DebugState::Running {
            inner.state = before;
            inner.revision += 1;
        }
    }

    pub(crate) fn frames(&self, thread: i64, start: usize, count: usize) -> Result<Vec<FrameInfo>, CommandError> {
        let (loaded, total) = {
            let inner = self.lock();
            let frames = inner.frames.get(&thread).cloned().unwrap_or_default();
            (frames.frames.len(), frames.total)
        };
        let wanted = start + count;
        let more = total.is_none_or(|total| (loaded as i64) < total);
        if loaded < wanted && more {
            let levels = (wanted - loaded).div_ceil(FRAME_PAGE) * FRAME_PAGE;
            let client = self.stopped_client()?;
            let answer = wait(client.stack_trace(thread, Some(loaded as i64), Some(levels as i64)))?;
            let mut inner = self.lock();
            let frames = inner.frames.entry(thread).or_default();
            if frames.frames.len() == loaded {
                let complete = answer.stack_frames.len() < levels;
                frames.frames.extend(answer.stack_frames);
                frames.total = answer.total_frames.or(complete.then_some(frames.frames.len() as i64));
            }
        }
        let inner = self.lock();
        let restart = inner.can_restart();
        Ok(inner
            .frames
            .get(&thread)
            .map(|frames| frame_infos(&frames.frames, restart).into_iter().skip(start).take(count).collect())
            .unwrap_or_default())
    }

    pub(crate) fn select_frame(&self, id: i64) -> bool {
        let mut inner = self.lock();
        let thread = inner
            .frames
            .iter()
            .find(|(_, frames)| frames.frames.iter().any(|frame| frame.id == id))
            .map(|(thread, _)| *thread);
        let Some(thread) = thread else { return false };
        inner.thread_id = Some(thread);
        inner.frame_id = Some(id);
        inner.revision += 1;
        drop(inner);
        self.notify();
        true
    }

    pub(crate) fn frame_id(&self, wanted: Option<i64>) -> Result<i64, CommandError> {
        wanted.or(self.lock().frame_id).ok_or_else(|| CommandError::new(t!("debug-no-frame")))
    }

    pub(crate) fn set_views(&self, setup: Option<zen_run::AdapterViews>, server: Option<String>) {
        self.lock().views =
            setup.map(|setup| Views { setup, server, shown: None, switching: None, inflight: Vec::new() });
    }

    pub(crate) fn has_views(&self) -> bool {
        self.lock().views.is_some()
    }

    pub(crate) fn view_gate(&self, raw: bool) -> ViewGate {
        let mut inner = self.lock();
        let Inner { views, asked, variables, .. } = &mut *inner;
        let Some(views) = views.as_mut() else { return ViewGate::Ready };
        if let Some((target, switch)) = &views.switching {
            if !switch.is_answered() {
                return ViewGate::Waiting;
            }
            views.shown = Some(*target);
            views.switching = None;
        }
        if views.shown == Some(raw) {
            return ViewGate::Ready;
        }
        let mut still = Vec::new();
        for key in std::mem::take(&mut views.inflight) {
            let Some(pending) = asked.remove(&variables_ask_key(key)) else { continue };
            match pending.try_answer() {
                None => {
                    asked.insert(variables_ask_key(key), pending);
                    still.push(key);
                }
                Some(Ok(body)) => {
                    if let Ok(answer) = serde_json::from_value::<zen_dap::protocol::VariablesResponse>(body) {
                        variables.insert(key, answer.variables);
                    }
                }
                Some(Err(_)) => {}
            }
        }
        if !still.is_empty() {
            views.inflight = still;
            return ViewGate::Waiting;
        }
        let arguments = if raw { views.setup.raw.clone() } else { views.setup.logical.clone() };
        ViewGate::Switch { command: views.setup.command.clone(), arguments, server: views.server.clone() }
    }

    pub(crate) fn switching(&self, raw: bool, switch: Option<Switch>) {
        if let Some(views) = self.lock().views.as_mut() {
            match switch {
                Some(switch) => views.switching = Some((raw, switch)),
                None => views.shown = Some(raw),
            }
        }
    }

    pub(crate) fn ask(
        &self,
        command: &str,
        arguments: serde_json::Value,
        wait: Duration,
    ) -> Result<Option<serde_json::Value>, CommandError> {
        let key = format!("{command} {arguments}");
        let known = self.lock().asked.remove(&key);
        let pending = match known {
            Some(pending) => pending,
            None => self.stopped_client()?.request(command, arguments).map_err(debugger_error)?,
        };
        let answer = if wait.is_zero() {
            pending.try_answer()
        } else {
            match pending.wait(wait) {
                Err(DapError::Timeout) => None,
                answer => Some(answer),
            }
        };
        match answer {
            Some(answer) => answer.map(Some).map_err(debugger_error),
            None => {
                let mut inner = self.lock();
                if inner.asked.len() >= ASKED_LIMIT {
                    inner.asked.clear();
                    inner.answered.clear();
                }
                inner.asked.insert(key, pending);
                Ok(None)
            }
        }
    }

    pub(crate) fn ask_once(
        &self,
        command: &str,
        arguments: serde_json::Value,
        wait: Duration,
    ) -> Result<Option<serde_json::Value>, String> {
        let key = format!("{command} {arguments}");
        if let Some(known) = self.lock().answered.get(&key) {
            return known.clone().map(Some);
        }
        let answer = match self.ask(command, arguments, wait) {
            Ok(None) => return Ok(None),
            Ok(Some(body)) => Ok(body),
            Err(error) => Err(error.message),
        };
        let mut inner = self.lock();
        if inner.answered.len() >= ASKED_LIMIT * 4 {
            inner.answered.clear();
        }
        inner.answered.insert(key, answer.clone());
        answer.map(Some)
    }

    pub(crate) fn variables_soon(
        &self,
        reference: i64,
        start: Option<i64>,
        count: Option<i64>,
        raw: bool,
        wait: Duration,
    ) -> Result<Option<Vec<zen_dap::Variable>>, CommandError> {
        let key = (reference, start, count, raw);
        if let Some(cached) = self.lock().variables.get(&key) {
            return Ok(Some(cached.clone()));
        }
        let ask_key = variables_ask_key(key);
        let known = self.lock().asked.remove(&ask_key);
        let pending = match known {
            Some(pending) => pending,
            None => {
                let arguments = serde_json::to_value(zen_dap::protocol::VariablesArguments {
                    variables_reference: reference,
                    start,
                    count,
                })
                .map_err(|error| CommandError::new(error.to_string()))?;
                self.stopped_client()?.request("variables", arguments).map_err(debugger_error)?
            }
        };
        let answer = if wait.is_zero() {
            pending.try_answer()
        } else {
            match pending.wait(wait) {
                Err(DapError::Timeout) => None,
                answer => Some(answer),
            }
        };
        let mut inner = self.lock();
        if let Some(views) = inner.views.as_mut() {
            views.inflight.retain(|known| *known != key);
        }
        let Some(answer) = answer else {
            if inner.asked.len() >= ASKED_LIMIT {
                inner.asked.clear();
            }
            inner.asked.insert(ask_key, pending);
            if let Some(views) = inner.views.as_mut() {
                views.inflight.push(key);
            }
            return Ok(None);
        };
        let variables = serde_json::from_value::<zen_dap::protocol::VariablesResponse>(answer.map_err(debugger_error)?)
            .map_err(|error| debugger_error(DapError::Io(error.to_string())))?
            .variables;
        inner.variables.insert(key, variables.clone());
        Ok(Some(variables))
    }

    pub(crate) fn sent(&self, path: &Path) -> Option<Vec<SourceBreakpoint>> {
        self.lock().files.get(path).map(|sent| sent.asked.clone())
    }

    pub(crate) fn set_run_to(&self, path: PathBuf) {
        let mut inner = self.lock();
        if !inner.restore.contains(&path) {
            inner.restore.push(path);
        }
    }

    pub(crate) fn ignore_breakpoints(&self, client: &Client, target: Option<(&Path, i64)>) -> Result<(), CommandError> {
        let (mut paths, functions, data) = {
            let inner = self.lock();
            let paths: Vec<PathBuf> =
                inner.files.iter().filter(|(_, sent)| !sent.asked.is_empty()).map(|(path, _)| path.clone()).collect();
            let functions = inner.others.functions.as_ref().is_some_and(|functions| !functions.is_empty());
            let data = inner.others.data.as_ref().is_some_and(|data| !data.is_empty());
            (paths, functions, data)
        };
        if let Some((path, _)) = target
            && !paths.iter().any(|known| known == path)
        {
            paths.push(path.to_owned());
        }
        for path in &paths {
            let lines: Vec<SourceBreakpoint> = match target {
                Some((at, line)) if at == path => vec![SourceBreakpoint { line, ..SourceBreakpoint::default() }],
                _ => Vec::new(),
            };
            wait(client.set_breakpoints(&path.to_string_lossy(), &lines))?;
        }
        if functions {
            wait(client.set_function_breakpoints(&[]))?;
        }
        if data {
            wait(client.set_data_breakpoints(&[]))?;
        }
        let mut inner = self.lock();
        for path in paths {
            if !inner.restore.contains(&path) {
                inner.restore.push(path);
            }
        }
        inner.restore_others |= functions || data;
        Ok(())
    }

    pub(crate) fn take_hits(&self) -> Vec<(PathBuf, u32)> {
        std::mem::take(&mut self.lock().hits)
    }

    pub(crate) fn sync_breakpoints(&self, breakpoints: &mut Breakpoints, wait: Duration) -> Synced {
        let mut synced = Synced { lines: Vec::new(), functions: Vec::new(), data: Vec::new() };
        let client = {
            let inner = self.lock();
            match (&inner.state, &inner.client) {
                (DebugState::Running | DebugState::Stopped { .. }, Some(client)) => client.clone(),
                _ => return synced,
            }
        };
        let capabilities = client.capabilities().clone();
        breakpoints.offer(&capabilities.exception_breakpoint_filters);
        let mut paths: Vec<PathBuf> = breakpoints.files().map(|(path, _)| path.to_owned()).collect();
        {
            let mut inner = self.lock();
            let extra: Vec<PathBuf> = inner.files.keys().filter(|path| !paths.contains(path)).cloned().collect();
            paths.extend(extra);
            for event in std::mem::take(&mut inner.events) {
                let Some(id) = event.id else { continue };
                for sent in inner.files.values_mut() {
                    if let Some(set) = sent.set.iter_mut().find(|set| set.id == Some(id)) {
                        *set = event.clone();
                    }
                }
            }
        }
        for path in &paths {
            let in_flight = self.lock().asking.remove(path);
            let (asked, plans, pending) = match in_flight {
                Some(in_flight) => in_flight,
                None => {
                    let (asked, plans): (Vec<_>, Vec<_>) = breakpoints.planned(path, &capabilities).into_iter().unzip();
                    let sent = self.lock().files.get(path).map(|sent| (sent.asked.clone(), sent.plans.clone()));
                    let (was_asked, was_planned) = sent.unwrap_or_default();
                    if asked == was_asked {
                        if plans != was_planned
                            && let Some(sent) = self.lock().files.get_mut(path)
                        {
                            sent.plans = plans;
                        }
                        continue;
                    }
                    if self.lock().refused.get(path) == Some(&asked) {
                        continue;
                    }
                    if client.has_source_map() {
                        if let Ok(set) = client.put_breakpoints(path, &asked, REQUEST_TIMEOUT) {
                            self.lock().files.insert(path.clone(), Sent { asked, set, plans });
                        }
                        continue;
                    }
                    match client.set_breakpoints(&path.to_string_lossy(), &asked) {
                        Ok(pending) => (asked, plans, pending),
                        Err(_) => {
                            self.lock().refused.insert(path.clone(), asked);
                            continue;
                        }
                    }
                }
            };
            let answer = if wait.is_zero() { pending.try_answer() } else { Some(pending.wait(wait)) };
            let mut inner = self.lock();
            match answer {
                None => {
                    inner.asking.insert(path.clone(), (asked, plans, pending));
                }
                Some(Ok(answer)) => {
                    inner.refused.remove(path);
                    inner.files.insert(path.clone(), Sent { asked, set: answer.breakpoints, plans });
                }
                Some(Err(_)) => {
                    inner.refused.insert(path.clone(), asked);
                }
            }
        }
        self.sync_others(&client, &capabilities, breakpoints);
        let mut inner = self.lock();
        for (path, sent) in &mut inner.files {
            for ((asked, set), plan) in sent.asked.iter_mut().zip(&sent.set).zip(sent.plans.iter_mut()) {
                let Ok(mut line) = u32::try_from(asked.line - 1) else { continue };
                if let Some(moved) = set.line.filter(|moved| *moved != asked.line && *moved > 0)
                    && let Ok(to) = u32::try_from(moved - 1)
                    && breakpoints.move_to(path, line, to)
                {
                    asked.line = moved;
                    line = to;
                    plan.line = to;
                }
                synced.lines.push((path.clone(), line, set.verified, set.message.clone()));
            }
        }
        if let Some(functions) = &inner.others.functions {
            for (function, set) in functions.iter().zip(&inner.others.function_set) {
                synced.functions.push((function.name.clone(), set.verified, set.message.clone()));
            }
        }
        if let Some(data) = &inner.others.data {
            for (data, set) in data.iter().zip(&inner.others.data_set) {
                synced.data.push((data.data_id.clone(), set.verified));
            }
        }
        synced
    }

    fn sync_others(&self, client: &Client, capabilities: &zen_dap::Capabilities, breakpoints: &Breakpoints) {
        let others = self.lock().others.clone();
        let functions = breakpoints.planned_functions(capabilities);
        if capabilities.supports_function_breakpoints
            && others.functions.as_ref() != Some(&functions)
            && let Ok(answer) =
                client.set_function_breakpoints(&functions).and_then(|pending| pending.wait(REQUEST_TIMEOUT))
        {
            let mut inner = self.lock();
            inner.others.functions = Some(functions);
            inner.others.function_set = answer.breakpoints;
        }
        let exceptions = breakpoints.planned_exceptions(capabilities);
        if others.exceptions.as_ref() != Some(&exceptions)
            && client
                .set_exception_breakpoints_with(&exceptions)
                .and_then(|pending| pending.wait(REQUEST_TIMEOUT))
                .is_ok()
        {
            self.lock().others.exceptions = Some(exceptions);
        }
        let data = breakpoints.planned_data(capabilities);
        if capabilities.supports_data_breakpoints
            && others.data.as_ref() != Some(&data)
            && let Ok(answer) = client.set_data_breakpoints(&data).and_then(|pending| pending.wait(REQUEST_TIMEOUT))
        {
            let mut inner = self.lock();
            inner.others.data = Some(data);
            inner.others.data_set = answer.breakpoints;
        }
    }

    pub(crate) fn connect(&self, job: Connect) {
        let me = self.this.upgrade();
        std::thread::spawn(move || {
            let Some(me) = me else { return };
            match me.try_connect(job) {
                Ok(client) if me.is_stopping() => {
                    let _ = client.disconnect(true).and_then(|pending| pending.wait(STOP_GRACE));
                    client.close();
                    me.finish(End::Stopped);
                }
                Ok(client) => me.pump(&client),
                Err(message) => me.finish(if me.is_stopping() { End::Stopped } else { End::Failed(message) }),
            }
        });
    }

    fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    fn try_connect(&self, job: Connect) -> Result<Arc<Client>, String> {
        if self.is_stopping() {
            return Err(String::new());
        }
        let stream = match job.connect {
            Some(address) => {
                let server = job.adapter.server.as_ref().map_or("", |server| server.server.as_str());
                self.output.update(|output| {
                    output.system(&t!("debug-server-adapter", server = server, address = address.to_string()))
                });
                TcpStream::connect_timeout(&address, ACCEPT_TIMEOUT)
                    .map_err(|error| t!("debug-connect-failed", error = error.to_string()))?
            }
            None => {
                let listener =
                    ReverseListener::bind().map_err(|error| t!("debug-listen-failed", error = error.to_string()))?;
                let address = listener.addr();
                let display = match job.adapter.builtin {
                    Some(_) => job.adapter.line.display.clone(),
                    None => format!("{} {}={address}", job.adapter.line.display, job.adapter.address_flag),
                };
                self.output.update(|output| output.system(display.trim_start()));
                let output = self.output.clone();
                let sink: ProcessSink = Arc::new(move |event| {
                    if let ProcessEvent::Output { stream, text } = event {
                        output.update(|output| output.feed(stream, &text));
                    }
                });
                let adapter = job
                    .launcher
                    .launch(&job.adapter, &job.env, address, sink)
                    .map_err(|error| t!("debug-launch-failed", error = error))?;
                self.lock().adapter = Some(adapter.clone());
                listener
                    .accept_while(ACCEPT_TIMEOUT, || !adapter.has_exited() && !self.is_stopping())
                    .map_err(|error| t!("debug-connect-failed", error = error.to_string()))?
            }
        };
        let connection =
            Connection::tcp(stream).map_err(|error| t!("debug-connect-failed", error = error.to_string()))?;
        connection.set_waker(self.waker.clone());
        let mut client = Client::new(connection, job.provider.id());
        client.set_source_map(job.source_map.clone());
        let mut planned = None;
        let configured = client.configure(
            &job.launch,
            |capabilities| {
                let (configuration, plans) = job.model.configuration(capabilities);
                planned = Some((configuration.clone(), plans));
                configuration
            },
            HANDSHAKE_TIMEOUT,
        );
        let handshake = match configured {
            Ok(handshake) => handshake,
            Err(error) => {
                client.close();
                let failed = t!("debug-handshake-failed", error = error_text(&error));
                let hint = match &error {
                    DapError::Adapter { message } => job.provider.explain_debug_error(message),
                    _ => None,
                };
                return Err(match hint {
                    Some(hint) => format!("{failed} {hint}"),
                    None => failed,
                });
            }
        };
        let client = Arc::new(client);
        let mut inner = self.lock();
        if let Some((configuration, plans)) = planned {
            for (((path, asked), (_, set)), plans) in
                configuration.breakpoints.into_iter().zip(handshake.breakpoints).zip(plans)
            {
                inner.files.insert(path, Sent { asked, set, plans });
            }
            let capabilities = &handshake.capabilities;
            inner.others = SentOthers {
                functions: capabilities.supports_function_breakpoints.then_some(configuration.functions),
                function_set: handshake.functions,
                exceptions: Some(configuration.exceptions),
                data: capabilities.supports_data_breakpoints.then_some(configuration.data),
                data_set: handshake.data,
            };
        }
        inner.client = Some(client.clone());
        inner.hot_code_replace = job.adapter.server.as_ref().is_some_and(|server| server.hot_code_replace);
        if inner.state == DebugState::Starting {
            inner.state = DebugState::Running;
        }
        inner.revision += 1;
        drop(inner);
        self.notify();
        Ok(client)
    }

    fn pump(&self, client: &Arc<Client>) {
        loop {
            match client.next_incoming(POLL) {
                None => {
                    if !self.lock().state.is_live() {
                        return;
                    }
                }
                Some(Incoming::Closed) => {
                    self.finish(if self.is_stopping() { End::Stopped } else { End::Exited });
                    return;
                }
                Some(Incoming::Event(event)) => match event {
                    DebugEvent::Output(output) => self.output_event(&output),
                    DebugEvent::Stopped(stopped) => self.stopped(client, stopped),
                    DebugEvent::Continued(_) => {
                        if self.lock().state.is_stopped() {
                            self.mark_running();
                        }
                    }
                    DebugEvent::Exited(exited) => self.lock().exit_code = Some(exited.exit_code),
                    DebugEvent::Terminated => {
                        let _ = client.disconnect(true).and_then(|pending| pending.wait(STOP_GRACE));
                        client.close();
                        self.finish(if self.is_stopping() { End::Stopped } else { End::Exited });
                        return;
                    }
                    DebugEvent::Breakpoint(changed) => {
                        let mut inner = self.lock();
                        inner.events.push(changed.breakpoint);
                        inner.revision += 1;
                        drop(inner);
                        self.notify();
                    }
                    DebugEvent::Other { event, body }
                        if event == "hotcodereplace" && body["changeType"] == "BUILD_COMPLETE" =>
                    {
                        if self.lock().hot_code_replace {
                            let _ = self.reload_classes(true);
                        }
                    }
                    DebugEvent::Thread(_) | DebugEvent::Initialized | DebugEvent::Other { .. } => {}
                },
            }
        }
    }

    fn hit_plan(&self, event: &StoppedEvent, top: Option<&StackFrame>) -> Option<(PathBuf, Plan)> {
        if !event.reason.contains("breakpoint") {
            return None;
        }
        let inner = self.lock();
        for id in event.hit_breakpoint_ids.iter().flatten() {
            for (path, sent) in &inner.files {
                if let Some(index) = sent.set.iter().position(|set| set.id == Some(*id))
                    && let Some(plan) = sent.plans.get(index)
                {
                    return Some((path.clone(), plan.clone()));
                }
            }
        }
        let top = top?;
        let path = PathBuf::from(top.source.as_ref()?.path.as_ref()?);
        let line = u32::try_from(top.line - 1).ok()?;
        let sent = inner.files.get(&path)?;
        let plan = sent.plans.iter().find(|plan| plan.line == line)?;
        Some((path, plan.clone()))
    }

    fn log_hit(&self, client: &Client, path: &Path, plan: &Plan, frames: &[StackFrame]) {
        let mut lines = Vec::new();
        if plan.log_hit {
            lines.push(hit_message(path, plan.line));
        }
        if plan.log_stack {
            for frame in frames {
                let place = frame
                    .source
                    .as_ref()
                    .and_then(|source| source.path.as_deref())
                    .map(|file| {
                        let name = Path::new(file)
                            .file_name()
                            .map_or_else(|| file.to_owned(), |name| name.to_string_lossy().into_owned());
                        format!("{name}:{}", frame.line)
                    })
                    .unwrap_or_default();
                lines.push(format!("\t{} ({place})", frame.name));
            }
        }
        if let Some(expression) = &plan.evaluate {
            let frame = frames.first().map(|frame| frame.id);
            let answer =
                client.evaluate(expression, frame, Some("watch")).and_then(|pending| pending.wait(REQUEST_TIMEOUT));
            let value = match answer {
                Ok(answer) => answer.result,
                Err(error) => error_text(&error),
            };
            lines.push(value);
        }
        self.write(Stream::System, lines);
    }

    fn stopped(&self, client: &Client, event: StoppedEvent) {
        let threads = wait(client.threads()).map(|answer| answer.threads).unwrap_or_default();
        let thread = event.thread_id.or_else(|| threads.first().map(|thread| thread.id));
        let frames = thread
            .and_then(|thread| wait(client.stack_trace(thread, Some(0), Some(FRAME_PAGE as i64))).ok())
            .unwrap_or_default();
        if let Some((path, plan)) = self.hit_plan(&event, frames.stack_frames.first()) {
            self.log_hit(client, &path, &plan, &frames.stack_frames);
            self.lock().hits.push((path, plan.line));
            match plan.suspend {
                Suspend::None => {
                    if let Some(thread) = thread
                        && wait(client.continue_(thread)).is_ok()
                    {
                        self.lock().revision += 1;
                        self.notify();
                        return;
                    }
                }
                Suspend::Thread if client.capabilities().supports_single_thread_execution_requests => {
                    for other in threads.iter().filter(|other| Some(other.id) != thread) {
                        let _ = client.continue_alone(other.id);
                    }
                }
                Suspend::All | Suspend::Thread => {}
            }
        }
        let (restore, others) = {
            let mut inner = self.lock();
            let paths = std::mem::take(&mut inner.restore);
            let restore: Vec<_> = paths
                .into_iter()
                .map(|path| {
                    let sent = inner.files.get(&path).cloned().unwrap_or_default();
                    (path, sent.asked, sent.plans)
                })
                .collect();
            let others = std::mem::take(&mut inner.restore_others).then(|| inner.others.clone());
            (restore, others)
        };
        for (path, asked, plans) in restore {
            if let Ok(set) = client.put_breakpoints(&path, &asked, REQUEST_TIMEOUT) {
                self.lock().files.insert(path, Sent { asked, set, plans });
            }
        }
        if let Some(others) = others {
            if let Some(functions) = others.functions.filter(|functions| !functions.is_empty())
                && let Ok(answer) = wait(client.set_function_breakpoints(&functions))
            {
                self.lock().others.function_set = answer.breakpoints;
            }
            if let Some(data) = others.data.filter(|data| !data.is_empty())
                && let Ok(answer) = wait(client.set_data_breakpoints(&data))
            {
                self.lock().others.data_set = answer.breakpoints;
            }
        }
        let mut inner = self.lock();
        inner.state = DebugState::Stopped {
            reason: event.reason,
            thread_id: thread,
            description: event.description.or(event.text),
        };
        inner.threads = threads;
        inner.thread_id = thread;
        inner.frames.clear();
        inner.variables.clear();
        inner.scopes.clear();
        inner.asked.clear();
        inner.answered.clear();
        inner.values += 1;
        inner.frame_id = frames.stack_frames.first().map(|frame| frame.id);
        if let Some(thread) = thread {
            let complete = frames.stack_frames.len() < FRAME_PAGE;
            let total = frames.total_frames.or(complete.then_some(frames.stack_frames.len() as i64));
            inner.frames.insert(thread, Frames { frames: frames.stack_frames, total });
        }
        inner.stops += 1;
        inner.revision += 1;
        drop(inner);
        self.notify();
    }

    fn output_event(&self, event: &OutputEvent) {
        let stream = match event.category.as_deref() {
            Some("stdout") => Stream::Stdout,
            Some("stderr") => Stream::Stderr,
            Some("telemetry") => return,
            _ => Stream::System,
        };
        let lines = {
            let mut inner = self.lock();
            let partial = inner.partial.entry(stream).or_default();
            partial.push_str(&event.output);
            let mut lines = Vec::new();
            while let Some(end) = partial.find('\n') {
                lines.push(partial[..end].trim_end_matches('\r').to_owned());
                partial.drain(..=end);
            }
            lines
        };
        self.write(stream, lines);
    }

    fn write(&self, stream: Stream, lines: Vec<String>) {
        if lines.is_empty() {
            return;
        }
        self.output.update(|output| {
            for line in &lines {
                if stream == Stream::System {
                    output.system(line);
                } else {
                    output.feed(stream, line);
                }
            }
        });
    }

    fn finish(&self, end: End) {
        let (client, adapter, exit_code, partial) = {
            let mut inner = self.lock();
            if !inner.state.is_live() {
                return;
            }
            let reason = match &end {
                End::Exited => "exited",
                End::Stopped => "stopped",
                End::Failed(_) => "failed",
            };
            inner.state = DebugState::Ended { reason: reason.to_owned() };
            inner.revision += 1;
            (inner.client.take(), inner.adapter.take(), inner.exit_code, std::mem::take(&mut inner.partial))
        };
        self.notify();
        if let Some(client) = client {
            client.close();
        }
        if let Some(adapter) = adapter {
            let deadline = Instant::now() + STOP_GRACE;
            while !adapter.has_exited() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            adapter.kill();
        }
        for (stream, text) in partial {
            self.write(stream, vec![text].into_iter().filter(|text| !text.is_empty()).collect());
        }
        let debuggee = match end {
            End::Exited => self.driver.wait_debuggee(DEBUGGEE_GRACE),
            End::Stopped | End::Failed(_) => {
                self.driver.kill();
                None
            }
        };
        self.driver.clean();
        self.output.update(|output| match end {
            End::Exited => {
                let code = exit_code.and_then(|code| i32::try_from(code).ok()).or(debuggee.flatten());
                match code {
                    Some(code) => output.system(&t!("workbench-run-process-finished", code = code)),
                    None => output.system(&t!("debug-disconnected")),
                };
                output.end(RunState::Finished, code);
            }
            End::Stopped => {
                output.system(&t!("workbench-run-process-stopped"));
                output.end(RunState::Stopped, None);
            }
            End::Failed(message) => {
                output.system(&message);
                output.fail(message);
            }
        });
        self.notify();
    }

    pub fn stop(&self) -> bool {
        if !self.lock().state.is_live() {
            return false;
        }
        if self.stopping.swap(true, Ordering::SeqCst) {
            self.kill_now();
            return true;
        }
        let (client, adapter) = {
            let inner = self.lock();
            (inner.client.clone(), inner.adapter.clone())
        };
        match (client, adapter) {
            (Some(client), _) => {
                let me = self.this.upgrade();
                std::thread::spawn(move || {
                    let _ = client.disconnect(true).and_then(|pending| pending.wait(STOP_GRACE));
                    client.close();
                    if let Some(me) = me {
                        me.finish(End::Stopped);
                    }
                });
            }
            (None, Some(adapter)) => adapter.kill(),
            (None, None) => {
                self.driver.stop();
            }
        }
        true
    }

    fn kill_now(&self) {
        self.driver.kill();
        let inner = self.lock();
        if let Some(client) = &inner.client {
            let _ = client.disconnect(true);
            client.close();
        }
        if let Some(adapter) = &inner.adapter {
            adapter.kill();
        }
    }
}

impl RunControl for DebugSession {
    fn stop(&self) -> bool {
        DebugSession::stop(self)
    }

    fn kill(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.kill_now();
        self.driver.clean();
    }
}

pub fn reload_message(reload: &Reload) -> String {
    match (&reload.error, reload.changed_classes.len()) {
        (Some(error), _) => t!("debug-reload-failed", error = error.as_str()),
        (None, 0) => t!("debug-reload-none"),
        (None, count) => t!("debug-reload-done", count = count, classes = reload.changed_classes.join(", ")),
    }
}

fn variables_ask_key((reference, start, count, raw): VariablesKey) -> String {
    format!("variables {reference} {start:?} {count:?} {raw}")
}

pub(crate) fn wait<T: serde::de::DeserializeOwned>(
    pending: Result<zen_dap::Pending<T>, DapError>,
) -> Result<T, CommandError> {
    pending.and_then(|pending| pending.wait(REQUEST_TIMEOUT)).map_err(debugger_error)
}

pub(crate) fn debugger_error(error: DapError) -> CommandError {
    CommandError::new(t!("debug-adapter-error", error = error_text(&error)))
}

fn error_text(error: &DapError) -> String {
    match error {
        DapError::Io(error) => t!("debug-error-io", error = error.to_string()),
        DapError::Closed => t!("debug-error-closed"),
        DapError::Timeout => t!("debug-error-timeout"),
        DapError::Adapter { message } => message.clone(),
    }
}
