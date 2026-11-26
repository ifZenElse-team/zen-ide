pub mod breakpoints;
pub mod coroutines;
pub mod inline;
pub mod session;
pub mod stream;
pub mod views;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_dap::{SourceBreakpoint, Thread};
use zen_i18n::t;
use zen_run::{RunOutput, SharedOutput, provider_for};

pub use zen_dap::ExceptionBreakpointsFilter;

pub use breakpoints::{
    AfterHit, Breakpoint, BreakpointChange, BreakpointInfo, Breakpoints, DataBreakpointInfo, ExceptionBreakpointInfo,
    FunctionBreakpointInfo, LinePlace, Suspend, hit_message,
};
#[cfg(any(test, feature = "fake"))]
pub use session::FakeLauncher;
pub use session::{
    AdapterLauncher, AdapterProcess, DebugSession, DebugState, FRAME_PAGE, FrameInfo, ProcessLauncher, Reload,
    SessionView, reload_message,
};

use crate::run::{
    Driver, Prompts, Runs, StartParams, configuration_to_start, launched, project_root, run_context, scratch_folder,
};
use crate::{SaveAll, Workbench};
use session::{Connect, debugger_error, wait};

pub const SESSION_LIMIT: usize = 20;

pub const DEBUGGER_MISSING: &str = "debuggerMissing";

pub const REMOVE_BY_DRAG: &str = "debugger.removeBreakpointByDrag";

pub const RECENT_EXPRESSIONS: &str = "debugger.recentExpressions";

pub const RECENT_EXPRESSIONS_DEFAULT: usize = 50;

pub const VALUE_TOOLTIP: &str = "debugger.valueTooltip";

pub const VALUE_TOOLTIP_DELAY: &str = "debugger.valueTooltipDelay";

pub const VALUE_TOOLTIP_DELAY_DEFAULT: u64 = 700;

pub const INLINE_VALUES: &str = "debugger.inlineValues";

pub const RUN_TO_CURSOR_INLAY: &str = "debugger.runToCursorInlay";

const WAIT_LIMIT: u64 = 10_000;

#[derive(Default)]
pub struct Debugger {
    launcher: Option<Arc<dyn AdapterLauncher>>,
    sessions: Vec<Arc<DebugSession>>,
    active: Option<u64>,
    watches: Vec<String>,
    history: Vec<String>,
    next_id: u64,
}

impl Debugger {
    pub fn set_launcher(&mut self, launcher: Arc<dyn AdapterLauncher>) {
        self.launcher = Some(launcher);
    }

    pub fn sessions(&self) -> &[Arc<DebugSession>] {
        &self.sessions
    }

    pub fn session(&self, id: u64) -> Option<&Arc<DebugSession>> {
        self.sessions.iter().find(|session| session.id() == id)
    }

    pub fn active_session(&self) -> Option<&Arc<DebugSession>> {
        self.active.and_then(|id| self.session(id)).or_else(|| self.sessions.last())
    }

    pub fn watches(&self) -> &[String] {
        &self.watches
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn set_history(&mut self, history: Vec<String>, limit: usize) {
        self.history = Vec::new();
        for expression in history.into_iter().rev() {
            self.remember(expression, limit);
        }
    }

    fn remember(&mut self, expression: String, limit: usize) {
        let expression = expression.trim().to_owned();
        if expression.is_empty() {
            return;
        }
        self.history.retain(|known| *known != expression);
        self.history.insert(0, expression);
        self.history.truncate(limit);
    }

    fn live(&self) -> Vec<Arc<DebugSession>> {
        self.sessions.iter().filter(|session| session.state().is_live()).cloned().collect()
    }

    fn drop_old_sessions(&mut self) {
        while self.sessions.len() > SESSION_LIMIT {
            match self.sessions.iter().position(|session| !session.state().is_live()) {
                Some(index) => {
                    self.sessions.remove(index);
                }
                None => break,
            }
        }
    }
}

pub fn sync(session: &mut Session) {
    sync_within(session, session::REQUEST_TIMEOUT);
}

pub fn sync_soon(session: &mut Session) {
    sync_within(session, Duration::ZERO);
}

fn sync_within(session: &mut Session, wait: Duration) {
    let mut breakpoints = std::mem::take(session.state_mut::<Breakpoints>());
    breakpoints.pull(session.state_mut::<Workbench>());
    let live = session.state::<Debugger>().map(Debugger::live).unwrap_or_default();
    let all = session.state::<Debugger>().map(|debugger| debugger.sessions.clone()).unwrap_or_default();
    for debug in &all {
        for (path, line) in debug.take_hits() {
            breakpoints.hit(&path, line);
        }
    }
    let mut statuses = Vec::new();
    for debug in &live {
        let synced = debug.sync_breakpoints(&mut breakpoints, wait);
        statuses.extend(synced.lines);
        breakpoints.set_function_status(&synced.functions);
        breakpoints.set_data_status(&synced.data);
    }
    let lines: Vec<(PathBuf, u32)> = breakpoints
        .files()
        .flat_map(|(path, list)| list.iter().map(move |breakpoint| (path.to_owned(), breakpoint.line)))
        .collect();
    for (path, line) in lines {
        let status = statuses.iter().rev().find(|(file, at, _, _)| *file == path && *at == line);
        match status {
            Some((_, _, verified, message)) => breakpoints.set_status(&path, line, Some(*verified), message.clone()),
            None => breakpoints.set_status(&path, line, None, None),
        }
    }
    if live.is_empty() {
        breakpoints.clear_status();
        breakpoints.end_sessions();
    }
    breakpoints.push(session.state_mut::<Workbench>());
    session.set_state(breakpoints);
}

fn no_session() -> CommandError {
    CommandError::new(t!("debug-no-session"))
}

fn find_session(session: &Session, id: Option<u64>) -> Result<Arc<DebugSession>, CommandError> {
    let debugger = session.state::<Debugger>().ok_or_else(no_session)?;
    match id {
        Some(id) => {
            debugger.session(id).cloned().ok_or_else(|| CommandError::new(t!("debug-unknown-session", id = id)))
        }
        None => debugger.active_session().cloned().ok_or_else(no_session),
    }
}

fn caret_place(session: &Session, path: Option<PathBuf>, line: Option<u32>) -> Result<(PathBuf, u32), CommandError> {
    let active =
        session.state::<Workbench>().and_then(Workbench::active_document).filter(|document| document.image().is_none());
    let path = match path.or_else(|| active.map(|document| document.path().to_owned())) {
        Some(path) => path,
        None => return Err(CommandError::new(t!("workbench-error-no-document"))),
    };
    let line = match line {
        Some(line) => line,
        None => active
            .filter(|document| document.path() == path)
            .and_then(|document| document.snapshot().offset_to_point(document.selections().primary().head).ok())
            .and_then(|point| u32::try_from(point.line).ok())
            .ok_or_else(|| CommandError::new(t!("debug-no-line")))?,
    };
    Ok((path, line))
}

/// A debug session that was started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionStarted {
    /// The session's id.
    pub session_id: u64,
    /// The id of its run, for `run.output.get`.
    pub run_id: u64,
    /// For a compound configuration: the run of each configuration it started, debugged or run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<u64>,
    /// For a compound configuration: why the configurations that did not start could not.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

pub(crate) fn start(
    session: &mut Session,
    configuration: zen_run::Configuration,
    prompts: &Prompts,
) -> Result<SessionStarted, CommandError> {
    let root = project_root(session)?;
    SaveAll.run(session, NoParams {})?;
    let launched = launched(session, &root, &configuration, prompts)?;
    sync(session);
    let mut context = run_context(session, &root);
    context.scratch = scratch_folder();
    let runs = session.state_mut::<Runs>();
    let provider = provider_for(runs.providers(), &configuration.kind)
        .cloned()
        .ok_or_else(|| CommandError::new(t!("workbench-run-unknown-kind", kind = configuration.kind.as_str())))?;
    let name = configuration.name.clone();
    let plan = match provider.debug(&launched, &context) {
        None => return Err(CommandError::new(t!("debug-cannot-debug", name = name.as_str()))),
        Some(Err(error)) => {
            return Err(CommandError::new(t!("debug-cannot-start", name = name.as_str(), error = error)));
        }
        Some(Ok(plan)) => {
            let trusted = crate::trust::trusted(session, &root);
            crate::java::coroutines::prepare(session.settings(), &launched, plan, trusted)
        }
    };
    if plan.adapter.server.is_none() && plan.adapter.line.program.as_os_str().is_empty() {
        return Err(CommandError::with_code(DEBUGGER_MISSING, t!("debug-debugger-missing", name = name.as_str())));
    }
    let mut plan = plan;
    let connect = match plan.adapter.server.clone() {
        Some(adapter) => Some(
            server_adapter(session, &adapter, &mut plan.launch)
                .map_err(|error| CommandError::new(t!("debug-cannot-start", name = name.as_str(), error = error)))?,
        ),
        None => None,
    };
    let limit = crate::run::console_limit(session.settings());
    let runs = session.state_mut::<Runs>();
    crate::run::create_scratch(&context.scratch);
    let waker = runs.waker().cloned();
    let env = runs.env().env.clone();
    let output = SharedOutput::new(
        RunOutput::new(provider.parser(&configuration, &context)).with_console_limit(limit),
        waker.clone(),
    );
    let driver = Driver::new(plan.build.clone(), env.clone(), output.clone(), context.scratch.clone());
    if let Some(debuggee) = plan.debuggee.clone() {
        driver.set_debuggee(debuggee);
    }
    let model = session.state::<Breakpoints>().cloned().unwrap_or_default();
    let debugger = session.state_mut::<Debugger>();
    debugger.next_id += 1;
    let session_id = debugger.next_id;
    let launcher = debugger.launcher.clone().unwrap_or_else(|| Arc::new(ProcessLauncher));
    let debug = DebugSession::new(session_id, name, output.clone(), driver.clone(), waker);
    debug.set_views(plan.adapter.views.clone(), plan.adapter.server.as_ref().map(|server| server.server.clone()));
    let mut lines = plan.build.clone();
    lines.extend(plan.debuggee.as_ref().map(|debuggee| debuggee.line.clone()));
    if plan.adapter.server.is_none() {
        lines.push(plan.adapter.line.clone());
    }
    let launched = configuration.clone();
    let gate = (!launched.common.before_launch.is_empty()).then(crate::before_launch::Gate::new);
    let mut control: Arc<dyn crate::run::RunControl> = debug.clone();
    if let Some(gate) = &gate {
        gate.bind(output.clone(), Some(control));
        control = gate.clone();
    }
    let run_id =
        session.state_mut::<Runs>().insert_external(configuration, provider.id().to_owned(), lines, output, control);
    debug.set_run_id(run_id);
    let debugger = session.state_mut::<Debugger>();
    debugger.sessions.push(debug.clone());
    debugger.active = Some(session_id);
    debugger.drop_old_sessions();
    let build_env =
        session.state::<crate::build_tools::BuildEnv>().cloned().unwrap_or_else(crate::build_tools::BuildEnv::global);
    let source_map = crate::java::debug::source_map(&plan.adapter, build_env, &context.scratch);
    let job = Connect {
        adapter: plan.adapter,
        launch: plan.launch,
        env,
        launcher,
        model,
        provider: provider.clone(),
        connect,
        source_map,
    };
    let weak = Arc::downgrade(&debug);
    driver.then(Box::new(move || {
        if let Some(debug) = weak.upgrade() {
            debug.connect(job);
        }
    }));
    match gate {
        Some(gate) => {
            let begin = Box::new(move || driver.start());
            crate::before_launch::launch(session, &root, run_id, &launched, gate, begin, &[]);
        }
        None => driver.start(),
    }
    Ok(SessionStarted { session_id, run_id, runs: Vec::new(), errors: Vec::new() })
}

const SERVER_ADAPTER_TIMEOUT: Duration = Duration::from_secs(120);

fn server_adapter(
    session: &mut Session,
    adapter: &zen_run::ServerAdapter,
    launch: &mut serde_json::Value,
) -> Result<std::net::SocketAddr, String> {
    let deadline = std::time::Instant::now() + SERVER_ADAPTER_TIMEOUT;
    crate::language::with_languages(session, |languages, workbench| {
        for resolve in &adapter.resolve {
            let answer = crate::language::execute_server_command(
                languages,
                workbench,
                &adapter.server,
                &resolve.command,
                serde_json::Value::Array(resolve.arguments.clone()),
                deadline,
            )?;
            for (pointer, field) in &resolve.fields {
                let value = answer.pointer(pointer).cloned().unwrap_or(serde_json::Value::Null);
                if let Some(object) = launch.as_object_mut() {
                    object.insert(field.clone(), value);
                }
            }
        }
        let answer = crate::language::execute_server_command(
            languages,
            workbench,
            &adapter.server,
            &adapter.command,
            serde_json::json!([]),
            deadline,
        )?;
        let port = answer
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .ok_or_else(|| t!("debug-server-adapter-no-port", answer = answer.to_string()))?;
        Ok(std::net::SocketAddr::from(([127, 0, 0, 1], port)))
    })
}

pub struct StartSession;

impl Command for StartSession {
    type Params = StartParams;
    type Output = SessionStarted;
    const ID: &'static str = "debug.session.start";
    const TITLE: &'static str = "Debug";
    const DESCRIPTION: &'static str = "Saves modified files and debugs a configuration (⌃D the selected one, ⌃⇧D the \
         context at the caret; the parameters of run.process.start): builds it in a run whose console is the \
         session's, starts the debugger with the project's breakpoints and returns the session's and the run's ids. \
         Fails with the code debuggerMissing when the debugger is not installed.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<SessionStarted, CommandError> {
        let prompts = Prompts::of(&params);
        let configuration = configuration_to_start(session, params)?;
        let root = project_root(session)?;
        crate::trust::guard_debug(session, &root, &configuration)?;
        if crate::compound::is_compound(&configuration) {
            return crate::compound::debug(session, configuration, &prompts);
        }
        start(session, configuration, &prompts)
    }
}

/// A debug session by id.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
}

pub struct StopSession;

impl Command for StopSession {
    type Params = SessionParams;
    type Output = ();
    const ID: &'static str = "debug.session.stop";
    const TITLE: &'static str = "Stop";
    const DESCRIPTION: &'static str = "Ends a debug session: disconnects with the program ended and kills the debugger \
         if it does not exit in 2 s; during the build it stops the build.";

    fn run(&self, session: &mut Session, params: SessionParams) -> Result<(), CommandError> {
        find_session(session, params.session_id)?.stop();
        Ok(())
    }
}

/// A debug session as `debug.session.list` shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    /// The session's id.
    pub session_id: u64,
    /// The id of its run.
    pub run_id: u64,
    /// The configuration's name.
    pub name: String,
    /// `starting`, `running`, `stopped` or `ended`.
    pub state: String,
}

/// What `debug.session.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionList {
    /// The sessions of the window, oldest first.
    pub sessions: Vec<SessionSummary>,
}

pub struct ListSessions;

impl Command for ListSessions {
    type Params = NoParams;
    type Output = SessionList;
    const ID: &'static str = "debug.session.list";
    const TITLE: &'static str = "Debug Sessions";
    const DESCRIPTION: &'static str = "The debug sessions of this window, running and ended, with their run ids.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<SessionList, CommandError> {
        let sessions = session.state::<Debugger>().map(Debugger::sessions).unwrap_or_default();
        let sessions = sessions
            .iter()
            .map(|debug| SessionSummary {
                session_id: debug.id(),
                run_id: debug.run_id(),
                name: debug.name().to_owned(),
                state: debug.state().name().to_owned(),
            })
            .collect();
        Ok(SessionList { sessions })
    }
}

/// The parameters of `debug.session.get`.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GetSessionParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// How long to wait, in milliseconds (at most 10 000), while the session starts, runs or is being stopped.
    pub wait_ms: Option<u64>,
}

/// The selected frame's place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FramePlace {
    /// The file; absent when the frame has no source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The line, from 0.
    pub line: u32,
    /// The function's name.
    pub name: String,
}

/// What `debug.session.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionAnswer {
    /// `starting`, `running`, `stopped` or `ended`.
    pub state: String,
    /// Why it stopped (`breakpoint`, `step`, `pause`, `exception`, `entry`, `restart` after `debug.frame.reset`) or ended (`exited`, `stopped`, `failed`, `buildFailed`, `beforeLaunchFailed`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The stopped thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<i64>,
    /// The selected frame of a stopped session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<FramePlace>,
    /// The program's threads, as of its last stop.
    pub threads: Vec<Thread>,
    /// How many times the program has stopped, so that a stop at the same place again is told apart.
    pub stops: u64,
    /// The last reload of changed classes (Java's hot code replace, ADR-0154).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reloaded: Option<Reload>,
    /// Raised whenever the values may have changed: at each stop and after `debug.variable.set`.
    pub values: u64,
    /// What the session's debugger can do; absent before it is connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<SessionSupport>,
}

/// What a session's debugger can do beyond stepping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionSupport {
    /// Set Value: `debug.variable.set` can change a variable.
    pub set_value: bool,
    /// Smart Step Into: `debug.execution.stepInTargets` lists the calls on the line and `debug.execution.smartStepInto` steps into one.
    pub smart_step_into: bool,
    /// Reset Frame: `debug.frame.reset` runs a frame again from its start.
    pub reset_frame: bool,
}

fn session_support(debug: &DebugSession) -> Option<SessionSupport> {
    let client = debug.client().ok()?;
    let capabilities = client.capabilities();
    Some(SessionSupport {
        set_value: capabilities.supports_set_variable || capabilities.supports_set_expression,
        smart_step_into: capabilities.supports_step_in_targets_request,
        reset_frame: capabilities.supports_restart_frame,
    })
}

fn stopped_with(session: &Session, wanted: impl Fn(&SessionSupport) -> bool) -> bool {
    find_session(session, None)
        .ok()
        .filter(|debug| debug.state().is_stopped())
        .and_then(|debug| session_support(&debug))
        .is_some_and(|support| wanted(&support))
}

pub struct GetSession;

impl Command for GetSession {
    type Params = GetSessionParams;
    type Output = SessionAnswer;
    const ID: &'static str = "debug.session.get";
    const TITLE: &'static str = "Debug Session";
    const DESCRIPTION: &'static str = "A debug session's state (starting, running, stopped, ended), why it stopped or \
         ended, the stopped thread, the selected frame, the threads and how many times it stopped; with waitMs it first waits while the session \
         starts, runs or is being stopped, e.g. after debug.execution.resume for the next stop.";

    fn run(&self, session: &mut Session, params: GetSessionParams) -> Result<SessionAnswer, CommandError> {
        let debug = find_session(session, params.session_id)?;
        if let Some(wait) = params.wait_ms {
            debug.wait_while_moving(Duration::from_millis(wait).min(crate::language::MAX_WAIT));
        }
        for reload in debug.take_unreported() {
            notify_reload(session, &reload);
        }
        let view = debug.view();
        let (reason, thread_id) = match &view.state {
            DebugState::Stopped { reason, thread_id, .. } => (Some(reason.clone()), *thread_id),
            DebugState::Ended { reason } => (Some(reason.clone()), None),
            DebugState::Starting | DebugState::Running => (None, None),
        };
        let frame = view.state.is_stopped().then(|| debug.selected_frame()).flatten().map(|frame| FramePlace {
            path: frame.path,
            line: frame.line,
            name: frame.name,
        });
        Ok(SessionAnswer {
            state: view.state.name().to_owned(),
            reason,
            thread_id,
            frame,
            threads: view.threads,
            stops: view.stops,
            reloaded: debug.reloaded(),
            values: view.values,
            support: session_support(&debug),
        })
    }
}

/// A thread of a debug session.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ThreadParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// The thread; the stopped one when absent.
    pub thread_id: Option<i64>,
}

#[derive(Clone, Copy)]
enum Motion {
    Resume,
    StepOver,
    StepInto,
    StepOut,
    ForceStepOver,
}

fn proceed(session: &Session, params: ThreadParams, motion: Motion) -> Result<(), CommandError> {
    let debug = find_session(session, params.session_id)?;
    let client = debug.stopped_client()?;
    let thread = debug.thread(params.thread_id)?;
    if matches!(motion, Motion::ForceStepOver) {
        debug.ignore_breakpoints(&client, None)?;
    }
    let before = debug.mark_running();
    let answer = match motion {
        Motion::Resume => wait(client.continue_(thread)).map(|_| ()),
        Motion::StepOver | Motion::ForceStepOver => wait(client.next(thread)).map(|_| ()),
        Motion::StepInto => wait(client.step_in(thread)).map(|_| ()),
        Motion::StepOut => wait(client.step_out(thread)).map(|_| ()),
    };
    if answer.is_err() {
        debug.restore_state(before);
    }
    answer
}

pub struct Resume;

impl Command for Resume {
    type Params = ThreadParams;
    type Output = ();
    const ID: &'static str = "debug.execution.resume";
    const TITLE: &'static str = "Resume Program";
    const DESCRIPTION: &'static str = "Lets a stopped program run on (⌥⌘R) until the next breakpoint or its end.";

    fn run(&self, session: &mut Session, params: ThreadParams) -> Result<(), CommandError> {
        proceed(session, params, Motion::Resume)
    }
}

pub struct Pause;

impl Command for Pause {
    type Params = ThreadParams;
    type Output = ();
    const ID: &'static str = "debug.execution.pause";
    const TITLE: &'static str = "Pause Program";
    const DESCRIPTION: &'static str = "Pauses a running program; the session stops with the reason pause.";

    fn run(&self, session: &mut Session, params: ThreadParams) -> Result<(), CommandError> {
        let debug = find_session(session, params.session_id)?;
        let client = debug.client()?;
        let thread = debug.thread(params.thread_id)?;
        wait(client.pause(thread)).map(|_| ())
    }
}

pub struct StepOver;

impl Command for StepOver {
    type Params = ThreadParams;
    type Output = ();
    const ID: &'static str = "debug.execution.stepOver";
    const TITLE: &'static str = "Step Over";
    const DESCRIPTION: &'static str = "Runs a stopped thread to the next line of the same function (F8).";

    fn run(&self, session: &mut Session, params: ThreadParams) -> Result<(), CommandError> {
        proceed(session, params, Motion::StepOver)
    }
}

pub struct StepInto;

impl Command for StepInto {
    type Params = ThreadParams;
    type Output = ();
    const ID: &'static str = "debug.execution.stepInto";
    const TITLE: &'static str = "Step Into";
    const DESCRIPTION: &'static str = "Runs a stopped thread into the function called on its line (F7).";

    fn run(&self, session: &mut Session, params: ThreadParams) -> Result<(), CommandError> {
        proceed(session, params, Motion::StepInto)
    }
}

pub struct StepOut;

impl Command for StepOut {
    type Params = ThreadParams;
    type Output = ();
    const ID: &'static str = "debug.execution.stepOut";
    const TITLE: &'static str = "Step Out";
    const DESCRIPTION: &'static str = "Runs a stopped thread until its function returns (⇧F8).";

    fn run(&self, session: &mut Session, params: ThreadParams) -> Result<(), CommandError> {
        proceed(session, params, Motion::StepOut)
    }
}

pub struct ForceStepOver;

impl Command for ForceStepOver {
    type Params = ThreadParams;
    type Output = ();
    const ID: &'static str = "debug.execution.forceStepOver";
    const TITLE: &'static str = "Force Step Over";
    const DESCRIPTION: &'static str = "Runs a stopped thread to the next line of the same function with every \
         breakpoint ignored on the way, so that the functions called on the line run through (⌥⇧F8); the \
         breakpoints are back once it stops.";

    fn run(&self, session: &mut Session, params: ThreadParams) -> Result<(), CommandError> {
        proceed(session, params, Motion::ForceStepOver)
    }
}

pub struct ForceStepInto;

impl Command for ForceStepInto {
    type Params = ThreadParams;
    type Output = ();
    const ID: &'static str = "debug.execution.forceStepInto";
    const TITLE: &'static str = "Force Step Into";
    const DESCRIPTION: &'static str = "Runs a stopped thread into the function called on its line, even one the \
         IDE would step over (⌥⇧F7); a debugger that steps into every function does what Step Into does.";

    fn run(&self, session: &mut Session, params: ThreadParams) -> Result<(), CommandError> {
        proceed(session, params, Motion::StepInto)
    }
}

/// The parameters of `debug.execution.stepInTargets`.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StepTargetsParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// The frame whose line to look at; the selected one when absent.
    pub frame_id: Option<i64>,
    /// How long to wait for the debugger, at most 10 000; 0 answers `pending` at once, then call again with the same parameters.
    pub wait_ms: Option<u64>,
}

/// A call on the stopped line that Smart Step Into can go into, with lines and columns from 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepTarget {
    /// The target, for `debug.execution.smartStepInto`.
    pub id: i64,
    /// The call, as the debugger names it.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column the call starts at; absent when the debugger does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// The column the call ends before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
}

impl StepTarget {
    fn new(target: zen_dap::StepInTarget) -> Self {
        let from_zero = |value: Option<i64>| value.and_then(|value| u32::try_from(value.saturating_sub(1)).ok());
        Self {
            id: target.id,
            label: target.label,
            line: from_zero(target.line),
            column: from_zero(target.column),
            end_line: from_zero(target.end_line),
            end_column: from_zero(target.end_column),
        }
    }
}

/// What `debug.execution.stepInTargets` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepTargetsAnswer {
    /// The calls, in the order the debugger gives them.
    pub targets: Vec<StepTarget>,
    /// Whether the debugger has not answered within `waitMs`; ask again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

pub struct StepInTargets;

impl Command for StepInTargets {
    type Params = StepTargetsParams;
    type Output = StepTargetsAnswer;
    const ID: &'static str = "debug.execution.stepInTargets";
    const TITLE: &'static str = "Smart Step Into Targets";
    const DESCRIPTION: &'static str = "The calls on the stopped line that Smart Step Into (⇧F7) can go into, for \
         debug.execution.smartStepInto; only with a debugger that lists them (debug.session.get's \
         support.smartStepInto). With `waitMs` 0 it answers `pending` at once while the debugger works: call it \
         again with the same parameters.";

    fn is_available(&self, session: &Session) -> bool {
        stopped_with(session, |support| support.smart_step_into)
    }

    fn run(&self, session: &mut Session, params: StepTargetsParams) -> Result<StepTargetsAnswer, CommandError> {
        let debug = find_session(session, params.session_id)?;
        let client = debug.stopped_client()?;
        if !client.capabilities().supports_step_in_targets_request {
            return Err(CommandError::new(t!("debug-no-smart-step")));
        }
        let frame = debug.frame_id(params.frame_id)?;
        let wait = params.wait_ms.map_or(session::REQUEST_TIMEOUT, |wait| Duration::from_millis(wait.min(WAIT_LIMIT)));
        let Some(body) = debug.ask("stepInTargets", serde_json::json!({ "frameId": frame }), wait)? else {
            return Ok(StepTargetsAnswer { pending: true, ..StepTargetsAnswer::default() });
        };
        let answer = serde_json::from_value::<zen_dap::protocol::StepInTargetsResponse>(body)
            .map_err(|error| CommandError::new(error.to_string()))?;
        Ok(StepTargetsAnswer { targets: answer.targets.into_iter().map(StepTarget::new).collect(), pending: false })
    }
}

/// The parameters of `debug.execution.smartStepInto`.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmartStepParams {
    /// The session; the current one when absent.
    #[serde(default)]
    pub session_id: Option<u64>,
    /// The thread; the stopped one when absent.
    #[serde(default)]
    pub thread_id: Option<i64>,
    /// The call to step into, from `debug.execution.stepInTargets`.
    pub target_id: i64,
}

pub struct SmartStepInto;

impl Command for SmartStepInto {
    type Params = SmartStepParams;
    type Output = ();
    const ID: &'static str = "debug.execution.smartStepInto";
    const TITLE: &'static str = "Smart Step Into";
    const DESCRIPTION: &'static str = "Runs a stopped thread into one of the calls on its line (⇧F7 picks it in a \
         popup), by a target from debug.execution.stepInTargets; only with a debugger that lists them.";

    fn is_available(&self, session: &Session) -> bool {
        stopped_with(session, |support| support.smart_step_into)
    }

    fn run(&self, session: &mut Session, params: SmartStepParams) -> Result<(), CommandError> {
        let debug = find_session(session, params.session_id)?;
        let client = debug.stopped_client()?;
        if !client.capabilities().supports_step_in_targets_request {
            return Err(CommandError::new(t!("debug-no-smart-step")));
        }
        let thread = debug.thread(params.thread_id)?;
        let before = debug.mark_running();
        let answer = wait(client.step_into_target(thread, params.target_id)).map(|_| ());
        if answer.is_err() {
            debug.restore_state(before);
        }
        answer
    }
}

/// A line of a file.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaceParams {
    /// The file; the active editor's when absent.
    pub path: Option<PathBuf>,
    /// The line, from 0; the caret's when absent.
    pub line: Option<u32>,
}

pub struct RunToCursor;

impl Command for RunToCursor {
    type Params = PlaceParams;
    type Output = ();
    const ID: &'static str = "debug.execution.runToCursor";
    const TITLE: &'static str = "Run to Cursor";
    const DESCRIPTION: &'static str = "Lets the stopped program of the current session run until it reaches a line \
         (the caret's by default, ⌥F9) or a breakpoint.";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<(), CommandError> {
        sync(session);
        let (path, line) = caret_place(session, params.path, params.line)?;
        let debug = find_session(session, None)?;
        let client = debug.stopped_client()?;
        let thread = debug.thread(None)?;
        let asked = debug.sent(&path).unwrap_or_default();
        let target = i64::from(line) + 1;
        if !asked.iter().any(|breakpoint| breakpoint.line == target) {
            let mut temporary = asked;
            temporary.push(SourceBreakpoint { line: target, ..SourceBreakpoint::default() });
            client.put_breakpoints(&path, &temporary, session::REQUEST_TIMEOUT).map_err(debugger_error)?;
            debug.set_run_to(path);
        }
        let before = debug.mark_running();
        let answer = wait(client.continue_(thread)).map(|_| ());
        if answer.is_err() {
            debug.restore_state(before);
        }
        answer
    }
}

pub struct ForceRunToCursor;

impl Command for ForceRunToCursor {
    type Params = PlaceParams;
    type Output = ();
    const ID: &'static str = "debug.execution.forceRunToCursor";
    const TITLE: &'static str = "Force Run to Cursor";
    const DESCRIPTION: &'static str = "Lets the stopped program of the current session run until it reaches a line \
         (the caret's by default, ⌥⌘F9) with every breakpoint on the way ignored; the breakpoints are back once it \
         stops.";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<(), CommandError> {
        sync(session);
        let (path, line) = caret_place(session, params.path, params.line)?;
        let debug = find_session(session, None)?;
        let client = debug.stopped_client()?;
        let thread = debug.thread(None)?;
        debug.ignore_breakpoints(&client, Some((&path, i64::from(line) + 1)))?;
        let before = debug.mark_running();
        let answer = wait(client.continue_(thread)).map(|_| ());
        if answer.is_err() {
            debug.restore_state(before);
        }
        answer
    }
}

/// The parameters of `debug.frames.list`.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FramesParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// The thread; the selected one when absent.
    pub thread_id: Option<i64>,
    /// The first frame, from 0; 0 when absent.
    pub start: Option<usize>,
    /// How many frames; 20 when absent.
    pub count: Option<usize>,
}

/// What `debug.frames.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FramesAnswer {
    /// The frames, the innermost first.
    pub frames: Vec<FrameInfo>,
}

pub struct ListFrames;

impl Command for ListFrames {
    type Params = FramesParams;
    type Output = FramesAnswer;
    const ID: &'static str = "debug.frames.list";
    const TITLE: &'static str = "Frames";
    const DESCRIPTION: &'static str = "The frames of a stopped thread's stack, the innermost first, with lines and \
         columns from 0, in pages (20 by default).";

    fn run(&self, session: &mut Session, params: FramesParams) -> Result<FramesAnswer, CommandError> {
        let debug = find_session(session, params.session_id)?;
        debug.stopped_client()?;
        let thread = debug.thread(params.thread_id)?;
        let frames = debug.frames(thread, params.start.unwrap_or(0), params.count.unwrap_or(FRAME_PAGE))?;
        Ok(FramesAnswer { frames })
    }
}

/// A frame by id.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FrameParams {
    /// The frame, as `debug.frames.list` gives it.
    pub frame_id: i64,
}

pub struct SelectFrame;

impl Command for SelectFrame {
    type Params = FrameParams;
    type Output = ();
    const ID: &'static str = "debug.frame.select";
    const TITLE: &'static str = "Select Frame";
    const DESCRIPTION: &'static str =
        "Makes a frame of the current session the one variables, evaluation and watches use.";

    fn run(&self, session: &mut Session, params: FrameParams) -> Result<(), CommandError> {
        let debug = find_session(session, None)?;
        if debug.select_frame(params.frame_id) {
            Ok(())
        } else {
            Err(CommandError::new(t!("debug-unknown-frame", id = params.frame_id)))
        }
    }
}

/// The parameters of `debug.frame.reset`.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResetFrameParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// The frame, as `debug.frames.list` gives it; the selected one when absent.
    pub frame_id: Option<i64>,
}

pub struct ResetFrame;

impl Command for ResetFrame {
    type Params = ResetFrameParams;
    type Output = ();
    const ID: &'static str = "debug.frame.reset";
    const TITLE: &'static str = "Reset Frame";
    const DESCRIPTION: &'static str = "Drops the frames above a frame of the stopped program and runs it again from \
         its start (Reset Frame, formerly Drop Frame); the session stops with the reason restart. Only with a \
         debugger that restarts frames (debug.session.get's support.resetFrame) and a frame whose canRestart is \
         true; what the dropped frames changed outside their own variables stays changed.";

    fn is_available(&self, session: &Session) -> bool {
        stopped_with(session, |support| support.reset_frame)
    }

    fn run(&self, session: &mut Session, params: ResetFrameParams) -> Result<(), CommandError> {
        let debug = find_session(session, params.session_id)?;
        let client = debug.stopped_client()?;
        if !client.capabilities().supports_restart_frame {
            return Err(CommandError::new(t!("debug-no-reset-frame")));
        }
        let frame = debug.frame_id(params.frame_id)?;
        match debug.frame(frame) {
            Some(info) if info.can_restart => {}
            Some(_) => return Err(CommandError::new(t!("debug-frame-cannot-reset", id = frame))),
            None => return Err(CommandError::new(t!("debug-unknown-frame", id = frame))),
        }
        let before = debug.mark_running();
        let answer = wait(client.restart_frame(frame)).map(|_| ());
        if answer.is_err() {
            debug.restore_state(before);
        }
        answer
    }
}

/// The parameters of `debug.variables.list`: a frame's scopes, or with `reference` the children of a scope or a variable.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct VariablesParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// The frame whose scopes to list; the selected one when absent.
    pub frame_id: Option<i64>,
    /// The scope or variable whose children to list.
    pub reference: Option<i64>,
    /// The first child, from 0.
    pub start: Option<i64>,
    /// How many children; all when absent.
    pub count: Option<i64>,
    /// Whether to list a collection's, map's or array's fields instead of its elements; debug.variables.raw's choice when absent.
    pub raw: Option<bool>,
    /// How long to wait for the debugger, at most 10 000; 0 answers `pending` at once, then call again with the same parameters.
    pub wait_ms: Option<u64>,
}

/// A scope or a variable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VariableInfo {
    /// The name.
    pub name: String,
    /// The value, as text; empty for a scope.
    pub value: String,
    /// The type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The reference to its children, for `debug.variables.list`; 0 when it has none.
    pub reference: i64,
    /// How many children it has by index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed: Option<i64>,
    /// How many children it has by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named: Option<i64>,
    /// The expression that reads it, for `debug.variable.set` and watches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluate_name: Option<String>,
    /// A map entry's key and value, in the elements view of a map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<MapEntry>,
}

/// A map entry shown as key → value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MapEntry {
    pub key: String,
    pub value: String,
}

/// What `debug.variables.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VariablesAnswer {
    /// The scopes or the children.
    pub variables: Vec<VariableInfo>,
    /// Whether the debugger has not answered within `waitMs`; ask again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

pub struct ListVariables;

impl Command for ListVariables {
    type Params = VariablesParams;
    type Output = VariablesAnswer;
    const ID: &'static str = "debug.variables.list";
    const TITLE: &'static str = "Variables";
    const DESCRIPTION: &'static str = "With a frame (the selected one by default) its scopes, with a reference the \
         children of a scope or a variable, `count` from `start`; a reference of 0 has no children.";

    fn run(&self, session: &mut Session, params: VariablesParams) -> Result<VariablesAnswer, CommandError> {
        let debug = find_session(session, params.session_id)?;
        let client = debug.stopped_client()?;
        let soon = params.wait_ms.map(|wait| Duration::from_millis(wait.min(WAIT_LIMIT)));
        let child = |variable: zen_dap::Variable| VariableInfo {
            name: variable.name,
            value: variable.value,
            type_: variable.type_,
            reference: variable.variables_reference,
            indexed: variable.indexed_variables,
            named: variable.named_variables,
            evaluate_name: variable.evaluate_name,
            entry: None,
        };
        let scope = |scope: zen_dap::Scope| VariableInfo {
            name: scope.name,
            value: String::new(),
            type_: None,
            reference: scope.variables_reference,
            indexed: None,
            named: None,
            evaluate_name: None,
            entry: None,
        };
        let pending = || Ok(VariablesAnswer { variables: Vec::new(), pending: true });
        let variables = match (params.reference, soon) {
            (Some(reference), soon) => {
                let raw = params.raw.unwrap_or_else(|| views::shows_raw(session));
                let wait = soon.unwrap_or(session::REQUEST_TIMEOUT);
                let ready = if wait.is_zero() {
                    views::view_ready(session, &debug, raw)
                } else {
                    views::wait_for_view(session, &debug, raw, wait)?
                };
                let listed = match ready {
                    true => debug.variables_soon(reference, params.start, params.count, raw, wait)?,
                    false => None,
                };
                let resolved = match listed {
                    Some(variables) => views::resolve_lazy(&debug, variables, raw, wait)?,
                    None => None,
                };
                let entries = resolved.as_ref().and_then(|variables| views::map_entries(&debug, variables, raw, wait));
                match (resolved.zip(entries), soon) {
                    (Some((variables, entries)), _) => variables
                        .into_iter()
                        .zip(entries)
                        .map(|(variable, entry)| VariableInfo {
                            entry: entry.map(|(key, value)| MapEntry { key, value }),
                            ..child(variable)
                        })
                        .collect(),
                    (None, Some(_)) => return pending(),
                    (None, None) => return Err(debugger_error(zen_dap::DapError::Timeout)),
                }
            }
            (None, None) => {
                let frame = debug.frame_id(params.frame_id)?;
                wait(client.scopes(frame))?.scopes.into_iter().map(scope).collect()
            }
            (None, Some(soon)) => {
                let frame = debug.frame_id(params.frame_id)?;
                let Some(scopes) = debug.scopes_soon(frame, soon)? else { return pending() };
                scopes.into_iter().map(scope).collect()
            }
        };
        Ok(VariablesAnswer { variables, pending: false })
    }
}

/// The parameters of `debug.expression.evaluate`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateParams {
    /// The expression, in the program's language.
    pub expression: String,
    /// The frame; the selected one when absent.
    #[serde(default)]
    pub frame_id: Option<i64>,
    /// Where it comes from: `repl` (the default, kept in the recent expressions), `watch`, `hover` (only with a debugger that evaluates for hovers) or `clipboard`.
    #[serde(default)]
    pub context: Option<String>,
    /// How long to wait for the debugger, at most 10 000; 0 answers `pending` at once, then call again with the same parameters.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `debug.expression.evaluate` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateAnswer {
    /// The value, as text.
    pub result: String,
    /// Its type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The reference to its children, for `debug.variables.list`; 0 when it has none.
    pub reference: i64,
    /// How many children it has by index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed: Option<i64>,
    /// How many children it has by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named: Option<i64>,
    /// Whether the debugger has not answered within `waitMs`; ask again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

pub struct Evaluate;

impl Command for Evaluate {
    type Params = EvaluateParams;
    type Output = EvaluateAnswer;
    const ID: &'static str = "debug.expression.evaluate";
    const TITLE: &'static str = "Evaluate Expression";
    const DESCRIPTION: &'static str = "Evaluates an expression in a frame of the current stopped session (the selected \
         one by default). With `waitMs` 0 it answers `pending` at once while the debugger works: call it again with \
         the same parameters.";

    fn run(&self, session: &mut Session, params: EvaluateParams) -> Result<EvaluateAnswer, CommandError> {
        let debug = find_session(session, None)?;
        let client = debug.stopped_client()?;
        let frame = debug.frame_id(params.frame_id)?;
        let context = params.context.as_deref().unwrap_or("repl");
        if context == "hover" && !client.capabilities().supports_evaluate_for_hovers {
            return Err(CommandError::new(t!("debug-no-hover")));
        }
        let wait = params.wait_ms.map_or(session::REQUEST_TIMEOUT, |wait| Duration::from_millis(wait.min(WAIT_LIMIT)));
        let arguments = serde_json::json!({ "expression": params.expression, "frameId": frame, "context": context });
        let answer = debug.ask("evaluate", arguments, wait);
        if matches!(answer, Ok(None)) {
            return Ok(EvaluateAnswer { pending: true, ..EvaluateAnswer::default() });
        }
        if context == "repl" {
            let limit = session.settings().get_or(RECENT_EXPRESSIONS, RECENT_EXPRESSIONS_DEFAULT);
            session.state_mut::<Debugger>().remember(params.expression.clone(), limit);
        }
        let body = answer?.unwrap_or_default();
        let answer = serde_json::from_value::<zen_dap::protocol::EvaluateResponse>(body)
            .map_err(|error| CommandError::new(error.to_string()))?;
        Ok(EvaluateAnswer {
            result: answer.result,
            type_: answer.type_,
            reference: answer.variables_reference,
            indexed: answer.indexed_variables,
            named: answer.named_variables,
            pending: false,
        })
    }
}

/// The parameters of `debug.variable.set`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetValueParams {
    /// The session; the current one when absent.
    #[serde(default)]
    pub session_id: Option<u64>,
    /// The `reference` of the variable's scope or parent, from `debug.variables.list`; absent for an expression.
    #[serde(default)]
    pub reference: Option<i64>,
    /// The variable's name, or the expression to assign to when `reference` is absent.
    pub name: String,
    /// The new value, in the program's language, such as `42` or `"text"`.
    pub value: String,
    /// The variable's `evaluateName` from `debug.variables.list`, for a debugger that assigns expressions only.
    #[serde(default)]
    pub evaluate_name: Option<String>,
    /// The frame an expression is assigned in; the selected one when absent.
    #[serde(default)]
    pub frame_id: Option<i64>,
    /// How long to wait for the debugger, at most 10 000; 0 answers `pending` at once, then call again with the same parameters.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `debug.variable.set` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetValueAnswer {
    /// The value now, as the debugger shows it.
    pub value: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The reference to its children, for `debug.variables.list`; 0 when it has none.
    pub reference: i64,
    /// Whether the debugger has not answered within `waitMs`; ask again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

fn can_set_value(session: &Session) -> bool {
    stopped_with(session, |support| support.set_value)
}

pub struct SetValue;

impl Command for SetValue {
    type Params = SetValueParams;
    type Output = SetValueAnswer;
    const ID: &'static str = "debug.variable.set";
    const TITLE: &'static str = "Set Value";
    const DESCRIPTION: &'static str = "Changes a variable of the stopped program (Set Value…, F2 in Variables): by \
         its scope's or parent's reference and name, or by an expression; only with a debugger that can change \
         variables (debug.session.get's support.setValue). The variables and watches are read again after it.";

    fn is_available(&self, session: &Session) -> bool {
        can_set_value(session)
    }

    fn run(&self, session: &mut Session, params: SetValueParams) -> Result<SetValueAnswer, CommandError> {
        let debug = find_session(session, params.session_id)?;
        let client = debug.stopped_client()?;
        let capabilities = client.capabilities().clone();
        let wait = params.wait_ms.map_or(session::REQUEST_TIMEOUT, |wait| Duration::from_millis(wait.min(WAIT_LIMIT)));
        let asked = match (params.reference, capabilities.supports_set_variable, capabilities.supports_set_expression) {
            (Some(reference), true, _) => {
                let arguments =
                    serde_json::json!({ "variablesReference": reference, "name": params.name, "value": params.value });
                debug.ask("setVariable", arguments, wait)?
            }
            (reference, _, true) => {
                let expression = match (&params.evaluate_name, reference) {
                    (Some(expression), _) => expression.clone(),
                    (None, None) => params.name.clone(),
                    (None, Some(_)) => return Err(CommandError::new(t!("debug-no-set-value"))),
                };
                let frame = debug.frame_id(params.frame_id)?;
                let arguments =
                    serde_json::json!({ "expression": expression, "value": params.value, "frameId": frame });
                debug.ask("setExpression", arguments, wait)?
            }
            _ => return Err(CommandError::new(t!("debug-no-set-value"))),
        };
        let Some(body) = asked else { return Ok(SetValueAnswer { pending: true, ..SetValueAnswer::default() }) };
        debug.forget_values();
        let answer = serde_json::from_value::<zen_dap::SetValueResponse>(body)
            .map_err(|error| CommandError::new(error.to_string()))?;
        Ok(SetValueAnswer {
            value: answer.value,
            type_: answer.type_,
            reference: answer.variables_reference,
            pending: false,
        })
    }
}

/// What `debug.expression.history` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryAnswer {
    /// The expressions evaluated in the project, the newest first.
    pub expressions: Vec<String>,
}

pub struct ExpressionHistory;

impl Command for ExpressionHistory {
    type Params = NoParams;
    type Output = HistoryAnswer;
    const ID: &'static str = "debug.expression.history";
    const TITLE: &'static str = "Recent Expressions";
    const DESCRIPTION: &'static str = "The expressions evaluated in the project (`debug.expression.evaluate` in the \
         `repl` context), the newest first, as many as Advanced Settings → Debugger → Recent expressions \
         to remember sets.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<HistoryAnswer, CommandError> {
        let expressions = session.state::<Debugger>().map(|debugger| debugger.history.clone()).unwrap_or_default();
        Ok(HistoryAnswer { expressions })
    }
}

/// What `debug.executionPoint.show` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPoint {
    pub path: PathBuf,
    /// The line, from 0.
    pub line: u32,
    /// The frame, now the selected one.
    pub frame_id: i64,
}

pub struct ShowExecutionPoint;

impl Command for ShowExecutionPoint {
    type Params = SessionParams;
    type Output = ExecutionPoint;
    const ID: &'static str = "debug.executionPoint.show";
    const TITLE: &'static str = "Show Execution Point";
    const DESCRIPTION: &'static str = "Selects the top frame of the thread the program stopped in and opens its line \
         in the editor.";

    fn run(&self, session: &mut Session, params: SessionParams) -> Result<ExecutionPoint, CommandError> {
        let debug = find_session(session, params.session_id)?;
        debug.stopped_client()?;
        let thread = debug.thread(debug.stopped_thread())?;
        let frame =
            debug.frames(thread, 0, 1)?.into_iter().next().ok_or_else(|| CommandError::new(t!("debug-no-frame")))?;
        let path = frame.path.clone().ok_or_else(|| CommandError::new(t!("debug-no-source")))?;
        debug.select_frame(frame.id);
        crate::tabs::open_preview(session, &path, None)?;
        let offset = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .and_then(|document| {
                let text = document.snapshot();
                let line = (frame.line as usize).min(text.len_lines().saturating_sub(1));
                text.point_to_offset(zen_text::Point::new(line, 0)).ok()
            })
            .unwrap_or_default();
        let place = crate::history::NavigateToParams { path: path.clone(), offset, end: None };
        Command::run(&crate::history::NavigateTo, session, place)?;
        Ok(ExecutionPoint { path, line: frame.line, frame_id: frame.id })
    }
}

/// What the breakpoint commands answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointAnswer {
    /// The breakpoint; absent when toggling removed it.
    pub breakpoint: Option<BreakpointInfo>,
}

fn with_breakpoints<R>(session: &mut Session, change: impl FnOnce(&mut Breakpoints) -> R) -> R {
    sync(session);
    let result = change(session.state_mut::<Breakpoints>());
    let mut breakpoints = std::mem::take(session.state_mut::<Breakpoints>());
    breakpoints.push(session.state_mut::<Workbench>());
    session.set_state(breakpoints);
    sync(session);
    result
}

fn current(session: &Session, path: &Path, line: u32) -> Option<BreakpointInfo> {
    let breakpoints = session.state::<Breakpoints>()?;
    breakpoints.list(Some(path)).into_iter().find(|info| info.line == line)
}

pub struct ToggleBreakpoint;

impl Command for ToggleBreakpoint {
    type Params = PlaceParams;
    type Output = BreakpointAnswer;
    const ID: &'static str = "debug.breakpoint.toggle";
    const TITLE: &'static str = "Toggle Line Breakpoint";
    const DESCRIPTION: &'static str = "Sets a line breakpoint or removes the one there (⌘F8), at the caret of the \
         active editor by default; a running session gets the change at once.";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<BreakpointAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line)?;
        let set = with_breakpoints(session, |breakpoints| breakpoints.toggle(&path, line));
        Ok(BreakpointAnswer { breakpoint: set.and_then(|_| current(session, &path, line)) })
    }
}

/// The parameters of `debug.breakpoint.set`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointParams {
    /// The file.
    pub path: PathBuf,
    /// The line, from 0.
    pub line: u32,
    #[serde(flatten)]
    pub change: BreakpointChange,
}

fn known_master(session: &Session, change: &BreakpointChange) -> Result<(), CommandError> {
    if let Some(Some(master)) = &change.depends_on
        && session.state::<Breakpoints>().and_then(|breakpoints| breakpoints.get(&master.path, master.line)).is_none()
    {
        return Err(no_breakpoint(&master.path, master.line));
    }
    Ok(())
}

fn no_breakpoint(path: &Path, line: u32) -> CommandError {
    CommandError::new(t!("debug-no-breakpoint", path = crate::display(path), line = line + 1))
}

pub struct SetBreakpoint;

impl Command for SetBreakpoint {
    type Params = SetBreakpointParams;
    type Output = BreakpointAnswer;
    const ID: &'static str = "debug.breakpoint.set";
    const TITLE: &'static str = "Set Breakpoint";
    const DESCRIPTION: &'static str = "Sets a line breakpoint or changes the one there: enabled, condition, \
         suspend policy, logging (\"Breakpoint hit\" message, stack trace, an expression), remove once hit, pass \
         count, the breakpoint it waits for, group and description; what is absent stays.";

    fn run(&self, session: &mut Session, params: SetBreakpointParams) -> Result<BreakpointAnswer, CommandError> {
        let (path, line) = (params.path, params.line);
        known_master(session, &params.change)?;
        with_breakpoints(session, |breakpoints| breakpoints.set(&path, line, params.change));
        Ok(BreakpointAnswer { breakpoint: current(session, &path, line) })
    }
}

pub struct ToggleBreakpointEnabled;

impl Command for ToggleBreakpointEnabled {
    type Params = PlaceParams;
    type Output = BreakpointAnswer;
    const ID: &'static str = "debug.breakpoint.toggleEnabled";
    const TITLE: &'static str = "Toggle Breakpoint Enabled";
    const DESCRIPTION: &'static str = "Enables a disabled line breakpoint or disables an enabled one (⌥-click in the \
         gutter), at the caret of the active editor by default.";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<BreakpointAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line)?;
        let enabled = session
            .state::<Breakpoints>()
            .and_then(|breakpoints| breakpoints.get(&path, line))
            .map(|kept| kept.enabled);
        let Some(enabled) = enabled else { return Err(no_breakpoint(&path, line)) };
        let change = BreakpointChange { enabled: Some(!enabled), ..BreakpointChange::default() };
        with_breakpoints(session, |breakpoints| breakpoints.set(&path, line, change));
        Ok(BreakpointAnswer { breakpoint: current(session, &path, line) })
    }
}

pub struct ToggleTemporaryBreakpoint;

impl Command for ToggleTemporaryBreakpoint {
    type Params = PlaceParams;
    type Output = BreakpointAnswer;
    const ID: &'static str = "debug.breakpoint.toggleTemporary";
    const TITLE: &'static str = "Toggle Temporary Line Breakpoint";
    const DESCRIPTION: &'static str = "Sets a line breakpoint removed once it stops the program (⌥⇧⌘F8), or removes \
         the breakpoint there, at the caret of the active editor by default.";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<BreakpointAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line)?;
        let change = BreakpointChange { remove_once_hit: Some(true), ..BreakpointChange::default() };
        let set = with_breakpoints(session, |breakpoints| {
            if breakpoints.remove(&path, line) { None } else { Some(breakpoints.set(&path, line, change)) }
        });
        Ok(BreakpointAnswer { breakpoint: set.and_then(|_| current(session, &path, line)) })
    }
}

/// A breakpoint and the line it goes to.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveBreakpointParams {
    pub path: PathBuf,
    /// Its line, from 0.
    pub line: u32,
    /// The line it goes to, from 0; one without a breakpoint.
    pub to_line: u32,
}

pub struct MoveBreakpoint;

impl Command for MoveBreakpoint {
    type Params = MoveBreakpointParams;
    type Output = BreakpointAnswer;
    const ID: &'static str = "debug.breakpoint.move";
    const TITLE: &'static str = "Move Breakpoint";
    const DESCRIPTION: &'static str = "Moves a line breakpoint with its properties to another line of its file (a drag \
         in the gutter); a line with a breakpoint already is refused.";

    fn run(&self, session: &mut Session, params: MoveBreakpointParams) -> Result<BreakpointAnswer, CommandError> {
        let (path, from, to) = (params.path, params.line, params.to_line);
        if with_breakpoints(session, |breakpoints| breakpoints.move_to(&path, from, to)) {
            Ok(BreakpointAnswer { breakpoint: current(session, &path, to) })
        } else {
            Err(CommandError::new(t!("debug-cannot-move", line = to + 1)))
        }
    }
}

pub struct CopyBreakpoint;

impl Command for CopyBreakpoint {
    type Params = MoveBreakpointParams;
    type Output = BreakpointAnswer;
    const ID: &'static str = "debug.breakpoint.copy";
    const TITLE: &'static str = "Copy Breakpoint";
    const DESCRIPTION: &'static str = "Copies a line breakpoint with its properties to another line of its file (a \
         ⌘-drag in the gutter), replacing a breakpoint there.";

    fn run(&self, session: &mut Session, params: MoveBreakpointParams) -> Result<BreakpointAnswer, CommandError> {
        let (path, from, to) = (params.path, params.line, params.to_line);
        match with_breakpoints(session, |breakpoints| breakpoints.copy(&path, from, to)) {
            Some(_) => Ok(BreakpointAnswer { breakpoint: current(session, &path, to) }),
            None => Err(no_breakpoint(&path, from)),
        }
    }
}

/// What a command changed in the other breakpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OthersAnswer {
    /// How many breakpoints it changed.
    pub count: usize,
}

pub struct RemoveOtherBreakpoints;

impl Command for RemoveOtherBreakpoints {
    type Params = PlaceParams;
    type Output = OthersAnswer;
    const ID: &'static str = "debug.breakpoint.removeOthers";
    const TITLE: &'static str = "Remove Other Breakpoints";
    const DESCRIPTION: &'static str = "Removes every line breakpoint of the project but the one at a line (the \
         caret's by default).";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<OthersAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line)?;
        let count = with_breakpoints(session, |breakpoints| {
            let others = breakpoints.others(&path, line);
            for place in &others {
                breakpoints.remove(&place.path, place.line);
            }
            others.len()
        });
        Ok(OthersAnswer { count })
    }
}

pub struct DisableOtherBreakpoints;

impl Command for DisableOtherBreakpoints {
    type Params = PlaceParams;
    type Output = OthersAnswer;
    const ID: &'static str = "debug.breakpoint.disableOthers";
    const TITLE: &'static str = "Disable Other Breakpoints";
    const DESCRIPTION: &'static str = "Disables every line breakpoint of the project but the one at a line (the \
         caret's by default).";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<OthersAnswer, CommandError> {
        let (path, line) = caret_place(session, params.path, params.line)?;
        let count = with_breakpoints(session, |breakpoints| {
            let others = breakpoints.others(&path, line);
            for place in &others {
                let change = BreakpointChange { enabled: Some(false), ..BreakpointChange::default() };
                breakpoints.set(&place.path, place.line, change);
            }
            others.len()
        });
        Ok(OthersAnswer { count })
    }
}

/// Whether breakpoints are muted.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MuteParams {
    /// Muted or not; the other way round when absent.
    pub muted: Option<bool>,
}

/// Whether breakpoints are muted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MuteAnswer {
    pub muted: bool,
}

pub struct MuteBreakpoints;

impl Command for MuteBreakpoints {
    type Params = MuteParams;
    type Output = MuteAnswer;
    const ID: &'static str = "debug.breakpoints.mute";
    const TITLE: &'static str = "Mute Breakpoints";
    const DESCRIPTION: &'static str = "Mutes every breakpoint or lets them stop again (the Debug toolbar's Mute \
         Breakpoints): while muted, the debuggers get no line, function, exception or data breakpoints; the \
         breakpoints are kept.";

    fn run(&self, session: &mut Session, params: MuteParams) -> Result<MuteAnswer, CommandError> {
        let muted = with_breakpoints(session, |breakpoints| {
            let muted = params.muted.unwrap_or(!breakpoints.is_muted());
            breakpoints.set_muted(muted);
            muted
        });
        Ok(MuteAnswer { muted })
    }
}

/// A function breakpoint to add or change; what is absent stays.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FunctionBreakpointParams {
    /// The function's name as the debugger reads it, such as `main.run`.
    pub name: String,
    pub enabled: Option<bool>,
    /// An expression that must be true for it to stop; cleared when empty.
    pub condition: Option<String>,
    /// How many hits it takes to stop; cleared when empty.
    pub hit_condition: Option<String>,
}

/// A function breakpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionBreakpointAnswer {
    pub breakpoint: FunctionBreakpointInfo,
}

pub struct AddFunctionBreakpoint;

impl Command for AddFunctionBreakpoint {
    type Params = FunctionBreakpointParams;
    type Output = FunctionBreakpointAnswer;
    const ID: &'static str = "debug.functionBreakpoint.add";
    const TITLE: &'static str = "Add Function Breakpoint";
    const DESCRIPTION: &'static str = "Adds a breakpoint on a function by name, or changes the one there; a \
         debugger without function breakpoints keeps it unused.";

    fn run(
        &self,
        session: &mut Session,
        params: FunctionBreakpointParams,
    ) -> Result<FunctionBreakpointAnswer, CommandError> {
        let name = params.name.trim().to_owned();
        if name.is_empty() {
            return Err(CommandError::new(t!("debug-empty-function")));
        }
        with_breakpoints(session, |breakpoints| {
            breakpoints.set_function(&name, params.enabled, params.condition, params.hit_condition)
        });
        let breakpoint = session
            .state::<Breakpoints>()
            .and_then(|breakpoints| breakpoints.functions().iter().find(|function| function.name == name).cloned())
            .ok_or_else(|| CommandError::new(t!("debug-no-function", name = name.as_str())))?;
        Ok(FunctionBreakpointAnswer { breakpoint })
    }
}

/// A function breakpoint by name.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NameParams {
    pub name: String,
}

pub struct RemoveFunctionBreakpoint;

impl Command for RemoveFunctionBreakpoint {
    type Params = NameParams;
    type Output = ();
    const ID: &'static str = "debug.functionBreakpoint.remove";
    const TITLE: &'static str = "Remove Function Breakpoint";
    const DESCRIPTION: &'static str = "Removes a function breakpoint by name.";

    fn run(&self, session: &mut Session, params: NameParams) -> Result<(), CommandError> {
        if with_breakpoints(session, |breakpoints| breakpoints.remove_function(&params.name)) {
            Ok(())
        } else {
            Err(CommandError::new(t!("debug-no-function", name = params.name.as_str())))
        }
    }
}

/// An exception breakpoint to add or change; what is absent stays.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExceptionBreakpointParams {
    /// The filter's id, as `debug.breakpoint.list` offers it, such as `uncaught`.
    pub filter: String,
    pub enabled: Option<bool>,
    /// An expression that must be true for it to stop, when the debugger takes one; cleared when empty.
    pub condition: Option<String>,
}

/// An exception breakpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionBreakpointAnswer {
    pub breakpoint: ExceptionBreakpointInfo,
}

pub struct AddExceptionBreakpoint;

impl Command for AddExceptionBreakpoint {
    type Params = ExceptionBreakpointParams;
    type Output = ExceptionBreakpointAnswer;
    const ID: &'static str = "debug.exceptionBreakpoint.add";
    const TITLE: &'static str = "Add Exception Breakpoint";
    const DESCRIPTION: &'static str = "Adds an exception breakpoint for a filter the debugger offers (View \
         Breakpoints' +), or changes it: enabled, condition.";

    fn run(
        &self,
        session: &mut Session,
        params: ExceptionBreakpointParams,
    ) -> Result<ExceptionBreakpointAnswer, CommandError> {
        let filter = params.filter.trim().to_owned();
        if filter.is_empty() {
            return Err(CommandError::new(t!("debug-empty-filter")));
        }
        let breakpoint = with_breakpoints(session, |breakpoints| {
            breakpoints.set_exception(&filter, params.enabled, params.condition)
        });
        Ok(ExceptionBreakpointAnswer { breakpoint })
    }
}

/// An exception breakpoint by filter.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilterParams {
    pub filter: String,
}

pub struct RemoveExceptionBreakpoint;

impl Command for RemoveExceptionBreakpoint {
    type Params = FilterParams;
    type Output = ();
    const ID: &'static str = "debug.exceptionBreakpoint.remove";
    const TITLE: &'static str = "Remove Exception Breakpoint";
    const DESCRIPTION: &'static str = "Removes an exception breakpoint by its filter.";

    fn run(&self, session: &mut Session, params: FilterParams) -> Result<(), CommandError> {
        if with_breakpoints(session, |breakpoints| breakpoints.remove_exception(&params.filter)) {
            Ok(())
        } else {
            Err(CommandError::new(t!("debug-no-filter", filter = params.filter.as_str())))
        }
    }
}

/// What a data breakpoint watches.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DataBreakpointParams {
    /// The `reference` of the variable's scope or parent, from `debug.variables.list`; absent for an expression.
    pub reference: Option<i64>,
    /// The variable's name, or an expression.
    pub name: String,
    /// The frame an expression is read in; the selected one when absent.
    pub frame_id: Option<i64>,
    /// `read`, `write` or `readWrite`; the debugger's first choice when absent.
    pub access_type: Option<String>,
    /// An expression that must be true for it to stop.
    pub condition: Option<String>,
    /// How many hits it takes to stop.
    pub hit_condition: Option<String>,
}

/// A data breakpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataBreakpointAnswer {
    pub breakpoint: DataBreakpointInfo,
}

pub struct AddDataBreakpoint;

impl Command for AddDataBreakpoint {
    type Params = DataBreakpointParams;
    type Output = DataBreakpointAnswer;
    const ID: &'static str = "debug.dataBreakpoint.add";
    const TITLE: &'static str = "Add Data Breakpoint";
    const DESCRIPTION: &'static str = "Watches a variable of the stopped program: the program stops when it is \
         written (or read); only debuggers with data breakpoints, and only while the session lasts.";

    fn run(&self, session: &mut Session, params: DataBreakpointParams) -> Result<DataBreakpointAnswer, CommandError> {
        let debug = find_session(session, None)?;
        let client = debug.stopped_client()?;
        if !client.capabilities().supports_data_breakpoints {
            return Err(CommandError::new(t!("debug-no-data-breakpoints")));
        }
        let frame = if params.reference.is_none() { debug.frame_id(params.frame_id).ok() } else { None };
        let info = wait(client.data_breakpoint_info(params.reference, &params.name, frame))?;
        let Some(data_id) = info.data_id else {
            return Err(CommandError::new(t!("debug-cannot-watch", reason = info.description)));
        };
        let access_type = params.access_type.or_else(|| info.access_types.and_then(|types| types.first().cloned()));
        let breakpoint = DataBreakpointInfo {
            data_id,
            description: if info.description.is_empty() { params.name } else { info.description },
            access_type,
            enabled: true,
            condition: params.condition.filter(|condition| !condition.trim().is_empty()),
            hit_condition: params.hit_condition.filter(|hit| !hit.trim().is_empty()),
            verified: None,
        };
        with_breakpoints(session, |breakpoints| breakpoints.add_data(breakpoint.clone()));
        Ok(DataBreakpointAnswer { breakpoint })
    }
}

/// A data breakpoint by its id.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataIdParams {
    pub data_id: String,
    /// Whether it stops the program; for `debug.dataBreakpoint.set`.
    #[serde(default)]
    pub enabled: Option<bool>,
}

pub struct SetDataBreakpoint;

impl Command for SetDataBreakpoint {
    type Params = DataIdParams;
    type Output = DataBreakpointAnswer;
    const ID: &'static str = "debug.dataBreakpoint.set";
    const TITLE: &'static str = "Set Data Breakpoint";
    const DESCRIPTION: &'static str = "Enables or disables a data breakpoint.";

    fn run(&self, session: &mut Session, params: DataIdParams) -> Result<DataBreakpointAnswer, CommandError> {
        with_breakpoints(session, |breakpoints| breakpoints.set_data(&params.data_id, params.enabled))
            .map(|breakpoint| DataBreakpointAnswer { breakpoint })
            .ok_or_else(|| CommandError::new(t!("debug-no-data", id = params.data_id.as_str())))
    }
}

pub struct RemoveDataBreakpoint;

impl Command for RemoveDataBreakpoint {
    type Params = DataIdParams;
    type Output = ();
    const ID: &'static str = "debug.dataBreakpoint.remove";
    const TITLE: &'static str = "Remove Data Breakpoint";
    const DESCRIPTION: &'static str = "Removes a data breakpoint.";

    fn run(&self, session: &mut Session, params: DataIdParams) -> Result<(), CommandError> {
        if with_breakpoints(session, |breakpoints| breakpoints.remove_data(&params.data_id)) {
            Ok(())
        } else {
            Err(CommandError::new(t!("debug-no-data", id = params.data_id.as_str())))
        }
    }
}

/// A breakpoint by file and line.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveBreakpointParams {
    /// The file.
    pub path: PathBuf,
    /// The line, from 0.
    pub line: u32,
}

pub struct RemoveBreakpoint;

impl Command for RemoveBreakpoint {
    type Params = RemoveBreakpointParams;
    type Output = ();
    const ID: &'static str = "debug.breakpoint.remove";
    const TITLE: &'static str = "Remove Breakpoint";
    const DESCRIPTION: &'static str = "Removes the line breakpoint at a line of a file.";

    fn run(&self, session: &mut Session, params: RemoveBreakpointParams) -> Result<(), CommandError> {
        if with_breakpoints(session, |breakpoints| breakpoints.remove(&params.path, params.line)) {
            Ok(())
        } else {
            Err(CommandError::new(t!(
                "debug-no-breakpoint",
                path = crate::display(&params.path),
                line = params.line + 1
            )))
        }
    }
}

/// The parameters of `debug.breakpoint.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListBreakpointsParams {
    /// Only this file's breakpoints.
    pub path: Option<PathBuf>,
}

/// What the current session's debugger can do with breakpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointSupport {
    /// Conditions.
    pub conditions: bool,
    /// Pass counts (`hitCondition`).
    pub hit_conditions: bool,
    /// Logging without stopping by itself; otherwise the IDE stops, logs and resumes.
    pub log_points: bool,
    /// Suspend Thread: only the thread that hit a breakpoint stops.
    pub thread_suspend: bool,
    /// Function breakpoints.
    pub functions: bool,
    /// Data breakpoints.
    pub data: bool,
    /// Conditions on exception breakpoints.
    pub exception_conditions: bool,
}

/// What `debug.breakpoint.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointList {
    /// The line breakpoints, by file and line.
    pub breakpoints: Vec<BreakpointInfo>,
    /// The function breakpoints.
    pub functions: Vec<FunctionBreakpointInfo>,
    /// The exception breakpoints.
    pub exceptions: Vec<ExceptionBreakpointInfo>,
    /// The exception filters the debuggers offered, for `debug.exceptionBreakpoint.add`.
    pub offered: Vec<ExceptionFilter>,
    /// The data breakpoints of the running sessions.
    pub data: Vec<DataBreakpointInfo>,
    /// Whether every breakpoint is muted.
    pub muted: bool,
    /// What the current session's debugger supports; absent without a connected session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<BreakpointSupport>,
}

fn support(session: &Session) -> Option<BreakpointSupport> {
    let client = find_session(session, None).ok()?.client().ok()?;
    let capabilities = client.capabilities();
    Some(BreakpointSupport {
        conditions: capabilities.supports_conditional_breakpoints,
        hit_conditions: capabilities.supports_hit_conditional_breakpoints,
        log_points: capabilities.supports_log_points,
        thread_suspend: capabilities.supports_single_thread_execution_requests,
        functions: capabilities.supports_function_breakpoints,
        data: capabilities.supports_data_breakpoints,
        exception_conditions: capabilities.supports_exception_filter_options,
    })
}

pub struct ListBreakpoints;

impl Command for ListBreakpoints {
    type Params = ListBreakpointsParams;
    type Output = BreakpointList;
    const ID: &'static str = "debug.breakpoint.list";
    const TITLE: &'static str = "Breakpoints";
    const DESCRIPTION: &'static str = "The line breakpoints, by file and line from 0, with what the debugger of a \
         running session says about them (verified, message); the function, exception and data breakpoints, the \
         exception filters offered, whether breakpoints are muted and what the current debugger supports.";

    fn run(&self, session: &mut Session, params: ListBreakpointsParams) -> Result<BreakpointList, CommandError> {
        sync(session);
        let support = support(session);
        let offered = exception_breakpoints(session).offered;
        let model = session.state::<Breakpoints>().cloned().unwrap_or_default();
        Ok(BreakpointList {
            breakpoints: model.list(params.path.as_deref()),
            functions: model.functions().to_vec(),
            exceptions: model.exception_rows().to_vec(),
            offered,
            data: model.data().to_vec(),
            muted: model.is_muted(),
            support,
        })
    }
}

/// A watched expression.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WatchParams {
    /// The expression.
    pub expression: String,
}

pub struct AddWatch;

impl Command for AddWatch {
    type Params = WatchParams;
    type Output = ();
    const ID: &'static str = "debug.watch.add";
    const TITLE: &'static str = "New Watch";
    const DESCRIPTION: &'static str = "Adds an expression to the watches, evaluated whenever the program stops.";

    fn run(&self, session: &mut Session, params: WatchParams) -> Result<(), CommandError> {
        let expression = params.expression.trim().to_owned();
        if expression.is_empty() {
            return Err(CommandError::new(t!("debug-empty-watch")));
        }
        let watches = &mut session.state_mut::<Debugger>().watches;
        if !watches.contains(&expression) {
            watches.push(expression);
        }
        Ok(())
    }
}

pub struct RemoveWatch;

impl Command for RemoveWatch {
    type Params = WatchParams;
    type Output = ();
    const ID: &'static str = "debug.watch.remove";
    const TITLE: &'static str = "Remove Watch";
    const DESCRIPTION: &'static str = "Removes an expression from the watches.";

    fn run(&self, session: &mut Session, params: WatchParams) -> Result<(), CommandError> {
        let watches = &mut session.state_mut::<Debugger>().watches;
        let before = watches.len();
        watches.retain(|watch| *watch != params.expression.trim());
        if watches.len() == before {
            return Err(CommandError::new(t!("debug-unknown-watch", expression = params.expression.as_str())));
        }
        Ok(())
    }
}

/// A watch and its value in the selected frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WatchInfo {
    /// The expression.
    pub expression: String,
    /// Its value; absent when no session is stopped or it cannot be evaluated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Its type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The reference to its children, for `debug.variables.list`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<i64>,
    /// Why it cannot be evaluated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `debug.watch.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WatchList {
    /// The watches, in order.
    pub watches: Vec<WatchInfo>,
}

pub struct ListWatches;

impl Command for ListWatches {
    type Params = NoParams;
    type Output = WatchList;
    const ID: &'static str = "debug.watch.list";
    const TITLE: &'static str = "Watches";
    const DESCRIPTION: &'static str = "The watches with their values in the selected frame of the current session \
         while it is stopped, or why a value cannot be had.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<WatchList, CommandError> {
        let expressions = session.state::<Debugger>().map(|debugger| debugger.watches.clone()).unwrap_or_default();
        let target = find_session(session, None)
            .ok()
            .and_then(|debug| Some((debug.stopped_client().ok()?, debug.frame_id(None).ok()?)));
        let watches = expressions
            .into_iter()
            .map(|expression| {
                let mut info = WatchInfo { expression, value: None, type_: None, reference: None, error: None };
                if let Some((client, frame)) = &target {
                    match wait(client.evaluate(&info.expression, Some(*frame), Some("watch"))) {
                        Ok(answer) => {
                            info.value = Some(answer.result);
                            info.type_ = answer.type_;
                            info.reference = Some(answer.variables_reference);
                        }
                        Err(error) => info.error = Some(error.message),
                    }
                }
                info
            })
            .collect();
        Ok(WatchList { watches })
    }
}

fn notify_reload(session: &mut Session, reload: &Reload) {
    use crate::notifications::{Group, Kind, NewNotification};
    let kind = if reload.error.is_some() { Kind::Error } else { Kind::Info };
    let new = NewNotification::message(Group::Debugger, kind, reload_message(reload))
        .titled_by(zen_i18n::Message::new("debug-reload-title"));
    crate::notifications::post(session, new);
}

pub struct ReloadClasses;

impl Command for ReloadClasses {
    type Params = SessionParams;
    type Output = Reload;
    const ID: &'static str = "debug.classes.reload";
    const TITLE: &'static str = "Reload Changed Classes";
    const DESCRIPTION: &'static str = "Reloads the classes changed since the program started into the running \
         program of a debug session (the current one by default), with the debugger's hot code replace: Java's \
         java-debug, after the language server has built them (saving a file builds it; a build also reloads by \
         itself). Answers the classes replaced, or why it failed (a change hot code replace cannot apply, such as \
         a new method or field); tells the user in a notification as well.";

    fn run(&self, session: &mut Session, params: SessionParams) -> Result<Reload, CommandError> {
        let debug = find_session(session, params.session_id)?;
        let reload = debug.reload_classes(false)?;
        notify_reload(session, &reload);
        Ok(reload)
    }
}

/// An exception breakpoint filter of the current session's debugger.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionFilter {
    /// Its id, e.g. `uncaught`.
    pub filter: String,
    /// Its name, e.g. `Uncaught Exceptions`.
    pub label: String,
    /// Whether it is on.
    pub enabled: bool,
    /// Whether it takes a condition.
    pub supports_condition: bool,
}

/// What `debug.exceptionBreakpoint.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionBreakpoints {
    /// The filters turned on, by id.
    pub enabled: Vec<String>,
    /// The filters the current session's debugger offers; empty without a connected session.
    pub offered: Vec<ExceptionFilter>,
}

/// The exception breakpoint filters to turn on.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExceptionParams {
    /// The filters by id, e.g. `["uncaught"]`; every other one is turned off.
    pub filters: Vec<String>,
}

fn exception_breakpoints(session: &Session) -> ExceptionBreakpoints {
    let enabled = session.state::<Breakpoints>().map(Breakpoints::exceptions).unwrap_or_default();
    let offered = find_session(session, None)
        .ok()
        .and_then(|debug| debug.client().ok())
        .map(|client| client.capabilities().exception_breakpoint_filters.clone())
        .filter(|filters| !filters.is_empty())
        .or_else(|| session.state::<Breakpoints>().map(|breakpoints| breakpoints.known_filters().to_vec()))
        .unwrap_or_default()
        .into_iter()
        .map(|filter| ExceptionFilter {
            enabled: enabled.contains(&filter.filter),
            supports_condition: filter.supports_condition,
            filter: filter.filter,
            label: filter.label,
        })
        .collect();
    ExceptionBreakpoints { enabled, offered }
}

pub struct SetExceptionBreakpoints;

impl Command for SetExceptionBreakpoints {
    type Params = ExceptionParams;
    type Output = ExceptionBreakpoints;
    const ID: &'static str = "debug.exceptionBreakpoint.set";
    const TITLE: &'static str = "Exception Breakpoints";
    const DESCRIPTION: &'static str = "Turns on exactly the given exception breakpoint filters (Java: `caught` and `uncaught`, as debug.exceptionBreakpoint.list offers them) for the sessions started from now on and the current one: the program stops where such an exception is thrown.";

    fn run(&self, session: &mut Session, params: ExceptionParams) -> Result<ExceptionBreakpoints, CommandError> {
        with_breakpoints(session, |breakpoints| breakpoints.set_exceptions(params.filters));
        Ok(exception_breakpoints(session))
    }
}

pub struct ListExceptionBreakpoints;

impl Command for ListExceptionBreakpoints {
    type Params = NoParams;
    type Output = ExceptionBreakpoints;
    const ID: &'static str = "debug.exceptionBreakpoint.list";
    const TITLE: &'static str = "Exception Breakpoints";
    const DESCRIPTION: &'static str = "The exception breakpoint filters turned on, and the ones the current session's debugger offers (Java: `caught`, `uncaught`).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ExceptionBreakpoints, CommandError> {
        Ok(exception_breakpoints(session))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(StartSession)?;
    registry.register(StopSession)?;
    registry.register(ListSessions)?;
    registry.register(GetSession)?;
    registry.register(Resume)?;
    registry.register(Pause)?;
    registry.register(StepOver)?;
    registry.register(StepInto)?;
    registry.register(StepOut)?;
    registry.register(RunToCursor)?;
    registry.register(ForceRunToCursor)?;
    registry.register(ForceStepOver)?;
    registry.register(ForceStepInto)?;
    registry.register(StepInTargets)?;
    registry.register(SmartStepInto)?;
    registry.register(ResetFrame)?;
    registry.register(SetValue)?;
    registry.register(inline::InlineValues)?;
    registry.register(views::ShowRawFields)?;
    registry.register(stream::TraceStream)?;
    registry.register(coroutines::ListCoroutines)?;
    registry.register(ListFrames)?;
    registry.register(SelectFrame)?;
    registry.register(ListVariables)?;
    registry.register(Evaluate)?;
    registry.register(ExpressionHistory)?;
    registry.register(ShowExecutionPoint)?;
    registry.register(ToggleBreakpoint)?;
    registry.register(SetBreakpoint)?;
    registry.register(RemoveBreakpoint)?;
    registry.register(ListBreakpoints)?;
    registry.register(AddWatch)?;
    registry.register(RemoveWatch)?;
    registry.register(ListWatches)?;
    registry.register(SetExceptionBreakpoints)?;
    registry.register(ReloadClasses)?;
    registry.register(ListExceptionBreakpoints)?;
    registry.register(ToggleBreakpointEnabled)?;
    registry.register(ToggleTemporaryBreakpoint)?;
    registry.register(MoveBreakpoint)?;
    registry.register(CopyBreakpoint)?;
    registry.register(RemoveOtherBreakpoints)?;
    registry.register(DisableOtherBreakpoints)?;
    registry.register(MuteBreakpoints)?;
    registry.register(AddFunctionBreakpoint)?;
    registry.register(RemoveFunctionBreakpoint)?;
    registry.register(AddExceptionBreakpoint)?;
    registry.register(RemoveExceptionBreakpoint)?;
    registry.register(AddDataBreakpoint)?;
    registry.register(SetDataBreakpoint)?;
    registry.register(RemoveDataBreakpoint)?;
    Ok(())
}

#[cfg(test)]
mod tests;
