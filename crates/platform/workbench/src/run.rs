use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{
    CommandLine, Configuration, Console, ConsoleLine, KindSpec, LinkSpan, Process, ProcessEvent, ProcessSink,
    ProfileKind, RunConfigurations, RunContext, RunEnv, RunOutput, RunProvider, RunState, RunTarget, ScratchStarter,
    SharedOutput, TestCounts, TestId, TestNode, TestNodeKind, TestSource, TestStatus, Waker, provider_for,
};

use crate::guard::{refusal, refuses_foreign_settings};
use crate::project_model::ProjectModels;
use crate::project_settings::ProjectSettingsState;
use crate::{SaveAll, Workbench, display};

pub const ADOPTED_PREFIX: &str = "jb:";

pub const RUN_LIMIT: usize = 20;

pub const TEMPORARY_LIMIT: &str = "run.temporaryConfigurationsLimit";

pub const MAX_TEMPORARY_LIMIT: usize = 100;

pub const RERUN_CONFIRMATION: &str = "rerunConfirmation";

pub const PROMPT_NEEDED: &str = "promptNeeded";

const ALLOW_PARALLEL: &str = "allowRunningInParallel";

pub const CONSOLE_BUFFER_OVERRIDE: &str = "console.cycleBuffer.override";

pub const CONSOLE_BUFFER_KB: &str = "console.cycleBuffer.sizeKb";

pub fn console_limit(settings: &zen_core::Settings) -> usize {
    let kb = if settings.get_or(CONSOLE_BUFFER_OVERRIDE, false) {
        settings.get_or(CONSOLE_BUFFER_KB, zen_run::DEFAULT_BUFFER_KB).clamp(1, zen_run::MAX_BUFFER_KB)
    } else {
        zen_run::DEFAULT_BUFFER_KB
    };
    kb * 1024
}

#[derive(Clone)]
pub struct Run {
    pub id: u64,
    pub configuration: Configuration,
    pub provider: String,
    pub command_lines: Vec<CommandLine>,
    pub output: SharedOutput,
    pub debug: bool,
    pub instance: u32,
    pub replaces: Option<u64>,
    pub compound: Option<String>,
    pub coverage: bool,
    pub profile: Option<ProfileKind>,
    control: Option<Arc<dyn RunControl>>,
}

pub trait RunControl: Send + Sync {
    fn stop(&self) -> bool;

    fn kill(&self);

    fn stopping(&self) -> bool {
        false
    }
}

impl Run {
    pub fn stop(&self) -> bool {
        match &self.control {
            Some(control) => control.stop(),
            None => self.output.update(|output| {
                let running = output.is_running();
                output.end(RunState::Stopped, None);
                running
            }),
        }
    }

    pub fn kill(&self) {
        match &self.control {
            Some(control) => control.kill(),
            None => self.output.update(|output| output.end(RunState::Stopped, None)),
        }
    }

    pub fn is_running(&self) -> bool {
        self.output.read(RunOutput::is_running) && !self.control.as_ref().is_some_and(|control| control.stopping())
    }

    pub fn title(&self) -> String {
        if self.instance > 1 {
            format!("{} ({})", self.configuration.name, self.instance)
        } else {
            self.configuration.name.clone()
        }
    }
}

#[derive(Default)]
struct Control {
    process: Option<Process>,
    stopped: bool,
    debuggee: Option<zen_run::Debuggee>,
    attached: bool,
    debuggee_exit: Option<Option<i32>>,
}

type After = Box<dyn FnOnce() + Send>;

pub(crate) struct Driver {
    command_lines: Vec<CommandLine>,
    env: Vec<(String, String)>,
    output: SharedOutput,
    control: Mutex<Control>,
    scratch: PathBuf,
    after: Mutex<Option<After>>,
    task: Option<String>,
}

impl Driver {
    pub(crate) fn new(
        command_lines: Vec<CommandLine>,
        env: Vec<(String, String)>,
        output: SharedOutput,
        scratch: PathBuf,
    ) -> Arc<Self> {
        create_scratch(&scratch);
        Arc::new(Self {
            command_lines,
            env,
            output,
            control: Mutex::new(Control::default()),
            scratch,
            after: Mutex::new(None),
            task: None,
        })
    }

    pub(crate) fn task(
        command_lines: Vec<CommandLine>,
        env: Vec<(String, String)>,
        output: SharedOutput,
        label: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            command_lines,
            env,
            output,
            control: Mutex::new(Control::default()),
            scratch: PathBuf::new(),
            after: Mutex::new(None),
            task: Some(label),
        })
    }

    pub(crate) fn set_debuggee(&self, debuggee: zen_run::Debuggee) {
        self.control().debuggee = Some(debuggee);
    }

    pub(crate) fn wait_debuggee(&self, timeout: Duration) -> Option<Option<i32>> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let control = self.control();
                if !control.attached {
                    return None;
                }
                if let Some(code) = control.debuggee_exit {
                    return Some(code);
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn start_debuggee(self: &Arc<Self>) -> bool {
        let mut control = self.control();
        let Some(debuggee) = control.debuggee.clone() else { return false };
        if control.stopped {
            drop(control);
            self.finish(true, None);
            return true;
        }
        let line = match &debuggee.agent {
            Some(agent) => match agent.find() {
                Some(jar) => agent.applied(&debuggee.line, &jar),
                None => {
                    let missing = t!("workbench-run-agent-missing", prefix = agent.prefix.as_str());
                    self.output.update(|output| output.system(&missing));
                    debuggee.line.clone()
                }
            },
            None => debuggee.line.clone(),
        };
        self.output.update(|output| output.system(&line.display));
        let mut spawned = line;
        spawned.env = self.env.iter().chain(&debuggee.line.env).cloned().collect();
        let driver = self.clone();
        let sink: ProcessSink = Arc::new(move |event| driver.debuggee_event(event));
        match Process::spawn(&spawned, sink) {
            Ok(process) => control.process = Some(process),
            Err(error) => {
                drop(control);
                self.clean();
                self.output.update(|output| {
                    output.system(&error);
                    output.fail(error);
                });
            }
        }
        true
    }

    fn debuggee_event(self: &Arc<Self>, event: ProcessEvent) {
        match event {
            ProcessEvent::Output { stream, text } => {
                self.output.update(|output| output.feed(stream, &text));
                let ready = {
                    let mut control = self.control();
                    let waits = control.debuggee.as_ref().is_some_and(|debuggee| text.contains(&debuggee.ready));
                    let first = waits && !control.attached;
                    control.attached |= waits;
                    first
                };
                if ready && let Some(after) = self.take_after() {
                    after();
                }
            }
            ProcessEvent::Exited { code } => {
                let (attached, stopped) = {
                    let mut control = self.control();
                    control.process = None;
                    if control.attached {
                        control.debuggee_exit = Some(code);
                    }
                    (control.attached, control.stopped)
                };
                if !attached {
                    self.finish(stopped, code);
                }
            }
        }
    }

    pub(crate) fn then(&self, after: After) {
        *self.after.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(after);
    }

    pub(crate) fn start(self: &Arc<Self>) {
        if self.command_lines.is_empty() {
            if self.start_debuggee() {
                return;
            }
            if let Some(after) = self.take_after() {
                after();
            }
            return;
        }
        self.step(0);
    }

    fn take_after(&self) -> Option<After> {
        self.after.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take()
    }

    fn control(&self) -> MutexGuard<'_, Control> {
        self.control.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn step(self: &Arc<Self>, index: usize) {
        let Some(line) = self.command_lines.get(index) else {
            return;
        };
        let mut control = self.control();
        if control.stopped {
            self.finish(true, None);
            return;
        }
        self.output.update(|output| {
            for part in line.display.lines() {
                output.system(part);
            }
        });
        let mut spawned = line.clone();
        spawned.env = self.env.iter().chain(&line.env).cloned().collect();
        let driver = self.clone();
        let sink: ProcessSink = Arc::new(move |event| driver.event(index, event));
        match Process::spawn(&spawned, sink) {
            Ok(process) => control.process = Some(process),
            Err(error) => {
                self.clean();
                let task = self.task.clone();
                self.output.update(|output| {
                    output.system(&error);
                    match task {
                        Some(label) => {
                            let message = zen_i18n::Message::new("workbench-before-launch-failed")
                                .with("task", label)
                                .with("error", error.as_str());
                            output.system(&message.text());
                            output.fail_before_launch_worded(message, None);
                        }
                        None => output.fail(error),
                    }
                });
            }
        }
    }

    fn event(self: &Arc<Self>, index: usize, event: ProcessEvent) {
        match event {
            ProcessEvent::Output { stream, text } => self.output.update(|output| output.feed(stream, &text)),
            ProcessEvent::Exited { code } => {
                let stopped = {
                    let mut control = self.control();
                    control.process = None;
                    control.stopped
                };
                if code == Some(0) && !stopped && index + 1 < self.command_lines.len() {
                    self.step(index + 1);
                } else if code == Some(0) && !stopped && self.start_debuggee() {
                } else if let Some(after) = (code == Some(0) && !stopped).then(|| self.take_after()).flatten() {
                    after();
                } else {
                    self.finish(stopped, code);
                }
            }
        }
    }

    fn finish(&self, stopped: bool, code: Option<i32>) {
        self.clean();
        self.output.update(|output| {
            if stopped {
                output.system(&t!("workbench-run-process-stopped"));
                output.end(RunState::Stopped, None);
                return;
            }
            let ended = match code {
                Some(code) => {
                    zen_i18n::Message::new("workbench-run-process-finished").with_number("code", f64::from(code))
                }
                None => zen_i18n::Message::new("workbench-run-process-interrupted"),
            };
            output.system(&ended.text());
            match &self.task {
                Some(label) => {
                    let message = zen_i18n::Message::new("workbench-before-launch-failed")
                        .with("task", label.as_str())
                        .with_message("error", ended);
                    output.system(&message.text());
                    output.fail_before_launch_worded(message, code);
                }
                None => output.end(RunState::Finished, code),
            }
        });
    }

    pub(crate) fn clean(&self) {
        if !self.scratch.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.scratch);
        }
    }

    pub(crate) fn kill(&self) {
        let mut control = self.control();
        control.stopped = true;
        if let Some(process) = &control.process {
            process.kill();
        }
    }

    pub(crate) fn stop(&self) -> bool {
        let mut control = self.control();
        if !self.output.read(RunOutput::is_running) {
            return false;
        }
        control.stopped = true;
        if let Some(process) = &control.process {
            process.stop();
        }
        true
    }
}

impl RunControl for Driver {
    fn stop(&self) -> bool {
        Driver::stop(self)
    }

    fn stopping(&self) -> bool {
        self.control().stopped
    }

    fn kill(&self) {
        Driver::kill(self);
        self.clean();
    }
}

#[derive(Default)]
pub struct Runs {
    providers: Vec<Arc<dyn RunProvider>>,
    env: RunEnv,
    debugger: PathBuf,
    waker: Option<Waker>,
    runs: Vec<Run>,
    next_id: u64,
    selected: Option<String>,
    temporary: Vec<Configuration>,
    console_limit: Option<usize>,
    caller: crate::plugin_runs::Caller,
    person_question: Option<crate::plugin_runs::PersonQuestion>,
    person_only: std::collections::BTreeSet<u64>,
    asked_for: Option<Configuration>,
}

impl Runs {
    pub fn person_question(&self) -> Option<&crate::plugin_runs::PersonQuestion> {
        self.person_question.as_ref()
    }

    pub fn dismiss_person_question(&mut self) {
        self.person_question = None;
    }

    pub fn person_only_running(&self) -> bool {
        self.runs.iter().any(|run| self.person_only.contains(&run.id) && run.is_running())
    }

    pub fn is_person_only(&self, run_id: u64) -> bool {
        self.person_only.contains(&run_id)
    }

    pub(crate) fn set_caller(&mut self, caller: crate::plugin_runs::Caller, asked_for: Option<Configuration>) {
        self.caller = caller;
        self.asked_for = asked_for;
    }

    fn check_person(&mut self, launched: &Configuration, lines: &[CommandLine]) -> Result<(), CommandError> {
        use crate::plugin_runs::{Caller, PERSON_CONFIRMATION, PersonQuestion};
        let command_line = lines.iter().map(|line| line.display.clone()).collect::<Vec<_>>().join("\n");
        let folder = lines.first().map(|line| line.cwd.clone()).unwrap_or_default();
        let name = launched.name.as_str();
        match std::mem::take(&mut self.caller) {
            Caller::Person(Some(token))
                if self.person_question.as_ref().is_some_and(|asked| {
                    asked.token == token && asked.command_line == command_line && asked.folder == folder
                }) =>
            {
                self.person_question = None;
                Ok(())
            }
            Caller::Person(_) => {
                let configuration = self.asked_for.take().unwrap_or_else(|| launched.clone());
                let plugin = launched.options["plugin"].as_str().unwrap_or_default().to_owned();
                self.person_question = Some(PersonQuestion {
                    token: format!("person-{}-{}", self.next_id + 1, crate::notifications::now_ms()),
                    configuration,
                    name: name.to_owned(),
                    plugin,
                    command_line: command_line.clone(),
                    folder,
                });
                Err(CommandError::with_code(
                    PERSON_CONFIRMATION,
                    t!("workbench-plugin-run-person-asked", name = name, line = command_line.as_str()),
                ))
            }
            Caller::Agent | Caller::Unknown => Err(CommandError::with_code(
                crate::plugin_manager::USER_ONLY,
                t!("workbench-plugin-run-person-only", name = name, line = command_line.as_str()),
            )),
        }
    }

    pub fn set_console_limit(&mut self, bytes: usize) {
        self.console_limit = Some(bytes);
    }

    pub fn console_limit(&self) -> usize {
        self.console_limit.unwrap_or(zen_run::DEFAULT_BUFFER_KB * 1024)
    }

    pub fn set_providers(&mut self, providers: Vec<Arc<dyn RunProvider>>, env: RunEnv) {
        self.providers = providers;
        self.env = env;
    }

    pub fn set_env(&mut self, env: RunEnv) {
        self.env = env;
    }

    pub fn set_debugger(&mut self, debugger: PathBuf) {
        self.debugger = debugger;
    }

    pub fn debugger(&self) -> &Path {
        &self.debugger
    }

    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    pub fn waker(&self) -> Option<&Waker> {
        self.waker.as_ref()
    }

    pub fn providers(&self) -> &[Arc<dyn RunProvider>] {
        &self.providers
    }

    pub fn env(&self) -> &RunEnv {
        &self.env
    }

    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    pub fn run(&self, id: u64) -> Option<&Run> {
        self.runs.iter().find(|run| run.id == id)
    }

    pub fn latest_running(&self) -> Option<&Run> {
        self.runs.iter().rev().find(|run| run.output.read(RunOutput::is_running))
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn set_selected(&mut self, id: Option<String>) {
        self.selected = id;
    }

    pub fn temporary(&self) -> &[Configuration] {
        &self.temporary
    }

    pub fn set_temporary(&mut self, configurations: Vec<Configuration>) {
        self.temporary = configurations
            .into_iter()
            .map(|configuration| Configuration { temporary: true, ..configuration })
            .collect();
    }

    pub fn kinds(&self) -> Vec<KindInfo> {
        let mut kinds: Vec<KindInfo> = self
            .providers
            .iter()
            .flat_map(|provider| {
                provider.kinds().into_iter().map(|spec| KindInfo { provider: provider.id().to_owned(), spec })
            })
            .collect();
        kinds.push(crate::compound::kind());
        kinds
    }

    pub fn set_compound(&mut self, run_id: u64, compound: &str) {
        if let Some(run) = self.runs.iter_mut().find(|run| run.id == run_id) {
            run.compound = Some(compound.to_owned());
        }
    }

    pub fn of_configuration(&self, configuration_id: &str) -> Vec<&Run> {
        self.runs
            .iter()
            .filter(|run| {
                !configuration_id.is_empty()
                    && (run.configuration.id == configuration_id || run.compound.as_deref() == Some(configuration_id))
            })
            .collect()
    }

    pub fn start(&mut self, configuration: Configuration, context: &RunContext) -> Result<u64, CommandError> {
        let launched = configuration.clone();
        let (id, begin) = self.prepare(configuration, &launched, context, Instance::default(), None, None)?;
        begin();
        Ok(id)
    }

    pub fn instances(&self, configuration_id: &str) -> Vec<&Run> {
        self.runs
            .iter()
            .filter(|run| !configuration_id.is_empty() && !run.debug && run.configuration.id == configuration_id)
            .filter(|run| run.is_running())
            .collect()
    }

    pub fn launch(
        &mut self,
        configuration: Configuration,
        launched: &Configuration,
        context: &RunContext,
        instance: Instance,
    ) -> Result<u64, CommandError> {
        let (id, begin) = self.prepare(configuration, launched, context, instance, None, None)?;
        begin();
        Ok(id)
    }

    pub(crate) fn prepare(
        &mut self,
        configuration: Configuration,
        launched: &Configuration,
        context: &RunContext,
        instance: Instance,
        gate: Option<Arc<crate::before_launch::Gate>>,
        recording: Option<&Recording>,
    ) -> Result<(u64, crate::before_launch::Begin), CommandError> {
        let id = self.next_id + 1;
        let context = &RunContext { scratch: scratch_folder(), ..context.clone() };
        let provider = if configuration.kind == zen_run::external_tool::KIND {
            Arc::new(zen_run::external_tool::ExternalToolProvider) as Arc<dyn RunProvider>
        } else {
            provider_for(&self.providers, &configuration.kind).cloned().ok_or_else(|| {
                CommandError::new(t!("workbench-run-unknown-kind", kind = configuration.kind.as_str()))
            })?
        };
        let cannot_start = |error: String| {
            CommandError::new(t!("workbench-run-cannot-start", name = configuration.name.as_str(), error = error))
        };
        let command_lines = match recording {
            Some(Recording::Coverage(report)) => {
                let coverage = provider.coverage(launched).ok_or_else(|| {
                    CommandError::new(t!("workbench-coverage-unsupported", kind = configuration.kind.as_str()))
                })?;
                coverage.command_lines(launched, context, report)
            }
            Some(Recording::Profile(kind, report)) => {
                let profiler = provider.profiler(launched).ok_or_else(|| {
                    CommandError::new(t!("workbench-profile-unsupported", kind = configuration.kind.as_str()))
                })?;
                profiler.command_lines(launched, context, *kind, report)
            }
            None => provider.command_lines(launched, context),
        }
        .map_err(cannot_start)?;
        if command_lines.is_empty() {
            return Err(cannot_start(t!("workbench-run-nothing-to-start")));
        }
        let person_only = crate::plugin_runs::person_only(launched, &command_lines);
        if person_only {
            self.check_person(launched, &command_lines)?;
        }
        let output = SharedOutput::new(
            RunOutput::new(provider.parser(launched, context)).with_console_limit(self.console_limit()),
            self.waker.clone(),
        );
        let (begin, control): (crate::before_launch::Begin, Option<Arc<dyn RunControl>>) =
            match provider.replay(launched) {
                Some(replay) => {
                    let output = output.clone();
                    let lines = command_lines.clone();
                    let begin = Box::new(move || {
                        output.update(|output| {
                            for part in lines.iter().flat_map(|line| line.display.lines()) {
                                output.system(part);
                            }
                            for (stream, text) in &replay.lines {
                                output.feed(*stream, text);
                            }
                            output.end(RunState::Finished, Some(replay.exit_code));
                            output.system(&t!("workbench-run-process-finished", code = replay.exit_code));
                        });
                    });
                    (begin, None)
                }
                None => {
                    let driver = Driver::new(
                        command_lines.clone(),
                        context.env.env.clone(),
                        output.clone(),
                        context.scratch.clone(),
                    );
                    let started = driver.clone();
                    (Box::new(move || started.step(0)), Some(driver as Arc<dyn RunControl>))
                }
            };
        let control = match gate {
            Some(gate) => {
                gate.bind(output.clone(), control);
                Some(gate as Arc<dyn RunControl>)
            }
            None => control,
        };
        self.next_id = id;
        if person_only {
            self.person_only.insert(id);
        }
        let provider = provider.id().to_owned();
        self.runs.push(Run {
            id,
            configuration,
            provider,
            command_lines,
            output,
            debug: false,
            instance: instance.number.max(1),
            replaces: instance.replaces,
            compound: None,
            coverage: matches!(recording, Some(Recording::Coverage(_))),
            profile: match recording {
                Some(Recording::Profile(kind, _)) => Some(*kind),
                _ => None,
            },
            control,
        });
        self.drop_old_runs();
        Ok((id, begin))
    }

    pub fn insert_external(
        &mut self,
        configuration: Configuration,
        provider: String,
        command_lines: Vec<CommandLine>,
        output: SharedOutput,
        control: Arc<dyn RunControl>,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.runs.push(Run {
            id,
            configuration,
            provider,
            command_lines,
            output,
            debug: true,
            instance: 1,
            replaces: None,
            compound: None,
            coverage: false,
            profile: None,
            control: Some(control),
        });
        self.drop_old_runs();
        id
    }

    pub fn insert_task(
        &mut self,
        configuration: Configuration,
        provider: String,
        command_lines: Vec<CommandLine>,
        output: SharedOutput,
        control: Arc<dyn RunControl>,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.runs.push(Run {
            id,
            configuration,
            provider,
            command_lines,
            output,
            debug: false,
            instance: 1,
            replaces: None,
            compound: None,
            coverage: false,
            profile: None,
            control: Some(control),
        });
        self.drop_old_runs();
        id
    }

    fn drop_old_runs(&mut self) {
        while self.runs.len() > RUN_LIMIT {
            let older = &self.runs[..self.runs.len() - 1];
            match older.iter().position(|run| !run.output.read(RunOutput::is_running)) {
                Some(index) => {
                    self.runs.remove(index);
                }
                None => break,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Instance {
    pub number: u32,
    pub replaces: Option<u64>,
}

impl Drop for Runs {
    fn drop(&mut self) {
        for control in self.runs.iter().filter_map(|run| run.control.as_ref()) {
            control.kill();
        }
    }
}

pub(crate) fn scratch_folder() -> PathBuf {
    use std::hash::{BuildHasher, Hasher};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let run = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(run);
    hasher.write_u32(std::process::id());
    std::env::temp_dir().join(format!("zen-run-{:016x}", hasher.finish()))
}

#[cfg(unix)]
fn private(builder: &mut std::fs::DirBuilder) {
    std::os::unix::fs::DirBuilderExt::mode(builder, 0o700);
}

#[cfg(not(unix))]
fn private(_builder: &mut std::fs::DirBuilder) {}

pub(crate) fn create_scratch(folder: &Path) {
    if folder.as_os_str().is_empty() {
        return;
    }
    let mut builder = std::fs::DirBuilder::new();
    private(&mut builder);
    let _ = builder.create(folder);
}

/// A kind of configuration and the provider that runs it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KindInfo {
    /// The provider's id.
    pub provider: String,
    /// The kind.
    #[serde(flatten)]
    pub spec: KindSpec,
}

pub(crate) fn project_root(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

fn save_permanent(root: &Path, file: &RunConfigurations) -> Result<(), CommandError> {
    let target = zen_project::run::file_in(root);
    if refuses_foreign_settings(target.strip_prefix(root).unwrap_or(&target)) {
        return Err(CommandError::new(refusal(&target)));
    }
    file.write(root).map_err(|error| {
        CommandError::new(t!("workbench-run-file-save", path = display(&error.path), error = error.message))
    })
}

pub(crate) fn read_file(session: &mut Session, root: &Path) -> Result<RunConfigurations, CommandError> {
    let mut file = RunConfigurations::read(root).map_err(|error| {
        CommandError::new(t!("workbench-run-file-broken", path = display(&error.path), error = error.message))
    })?;
    let runs = session.state_mut::<Runs>();
    if file.selected.is_some() || file.configurations.iter().any(|kept| kept.temporary) {
        for kept in file.configurations.iter().filter(|kept| kept.temporary) {
            if !runs.temporary.iter().any(|temporary| temporary.id == kept.id) {
                runs.temporary.push(kept.clone());
            }
        }
        if runs.selected.is_none() {
            runs.selected = file.selected.take();
        }
        file = file.permanent();
        save_permanent(root, &file)?;
    }
    let adopted = adopted_configurations(session, root, &file);
    file.configurations.extend(adopted);
    let runs = session.state_mut::<Runs>();
    for temporary in &mut runs.temporary {
        if temporary.id.is_empty() || file.get(&temporary.id).is_some() {
            temporary.id = file.new_id(&temporary.kind);
        }
        file.configurations.push(temporary.clone());
    }
    file.selected = runs.selected.clone();
    Ok(file)
}

fn adopted_configurations(session: &mut Session, root: &Path, file: &RunConfigurations) -> Vec<Configuration> {
    if session.state_mut::<ProjectSettingsState>().ensure(Some(root)) {
        crate::index_project(session);
    }
    let Some(settings) = session.state::<ProjectSettingsState>().map(ProjectSettingsState::settings) else {
        return Vec::new();
    };
    let providers = session.state::<Runs>().map(|runs| runs.providers.clone()).unwrap_or_default();
    let mut adopted: Vec<Configuration> = Vec::new();
    let mut compounds: Vec<(usize, Vec<String>)> = Vec::new();
    for foreign in &settings.run_configurations {
        let compound = foreign.type_id == crate::compound::FOREIGN_TYPE;
        let configuration = if compound {
            Some(Configuration {
                name: foreign.name.clone(),
                kind: crate::compound::KIND.to_owned(),
                options: crate::compound::kind().spec.defaults,
                ..Configuration::default()
            })
        } else {
            providers.iter().find_map(|provider| provider.adopt(foreign, root))
        };
        let Some(configuration) = configuration else {
            continue;
        };
        let shadowed = |kept: &Configuration| kept.kind == configuration.kind && kept.name == configuration.name;
        if file.configurations.iter().any(shadowed) || adopted.iter().any(shadowed) {
            continue;
        }
        let relative = foreign.source_file.strip_prefix(root).unwrap_or(&foreign.source_file);
        let id = format!("{ADOPTED_PREFIX}{}#{}", relative.to_string_lossy().replace('\\', "/"), foreign.name);
        let mut common = configuration.common.clone();
        common.before_launch = crate::before_launch::adopt_tasks(session, root, &foreign.before_launch, &providers);
        common.allow_multiple_instances |=
            foreign.options.get(ALLOW_PARALLEL).is_some_and(|value| value.eq_ignore_ascii_case("true"));
        if compound {
            compounds.push((adopted.len(), foreign_members(foreign)));
        }
        adopted.push(Configuration {
            id,
            temporary: false,
            common,
            adopted_from: Some(foreign.source_file.clone()),
            folder: foreign.options.get(FOLDER_NAME).cloned().unwrap_or_default(),
            ..configuration
        });
    }
    for (index, names) in compounds {
        let ids: Vec<String> = names
            .iter()
            .filter_map(|name| {
                file.configurations.iter().chain(&adopted).find(|kept| kept.name == *name).map(|kept| kept.id.clone())
            })
            .collect();
        crate::compound::set_members(&mut adopted[index], &ids);
    }
    adopted
}

const FOLDER_NAME: &str = "folderName";

fn foreign_members(foreign: &zen_run::ForeignRunConfiguration) -> Vec<String> {
    let single = foreign.options.get("toRun.name").cloned();
    let numbered = (0..).map_while(|index| foreign.options.get(&format!("toRun.{index}.name")).cloned());
    single.into_iter().chain(numbered).collect()
}

pub(crate) fn refuse_adopted(file: &RunConfigurations, id: &str) -> Result<(), CommandError> {
    match file.get(id) {
        Some(kept) if kept.is_adopted() => Err(CommandError::new(t!(
            "workbench-run-adopted-read-only",
            name = kept.name.as_str(),
            file = kept.adopted_from.as_deref().map(display).unwrap_or_default()
        ))),
        _ => Ok(()),
    }
}

pub(crate) fn write_file(session: &mut Session, root: &Path, file: &RunConfigurations) -> Result<(), CommandError> {
    let permanent = file.permanent();
    if RunConfigurations::read(root).ok().as_ref() != Some(&permanent) {
        save_permanent(root, &permanent)?;
    }
    let runs = session.state_mut::<Runs>();
    runs.temporary = file.configurations.iter().filter(|kept| kept.temporary).cloned().collect();
    runs.selected = file.selected.clone();
    Ok(())
}

pub(crate) fn unknown_configuration(id: &str) -> CommandError {
    CommandError::new(t!("workbench-run-unknown-configuration", id = id))
}

fn unknown_run(id: u64) -> CommandError {
    CommandError::new(t!("workbench-run-unknown-run", id = id))
}

pub fn project_environment(session: &Session, root: &Path) -> Vec<(String, String)> {
    session
        .state::<crate::language::servers::LanguageServers>()
        .map(|servers| servers.project_environment(root))
        .unwrap_or_default()
}

pub(crate) fn run_context(session: &mut Session, root: &Path) -> RunContext {
    let (mut env, debugger) =
        session.state::<Runs>().map(|runs| (runs.env.clone(), runs.debugger.clone())).unwrap_or_default();
    if crate::jdk::is_jvm_project(root) {
        let jdk = session.state_mut::<crate::jdk::Jdks>().clone().store().project(root);
        crate::jdk::with_project_jdk(&mut env.env, jdk.as_ref());
    }
    env.env.extend(project_environment(session, root));
    let models = session.state_mut::<ProjectModels>();
    models.ensure(Some(root));
    RunContext {
        root: root.to_owned(),
        env,
        modules: models.model().modules.clone(),
        debugger,
        ..RunContext::default()
    }
}

pub(crate) fn file_text(session: &Session, path: &Path) -> Result<(String, Option<usize>), CommandError> {
    if let Some(patterns) = crate::hidden_files::for_caller(session) {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        if crate::hidden_files::is_hidden(&patterns, root.as_deref(), path) {
            return Err(crate::hidden_files::refused(&display(path)));
        }
    }
    if path.is_dir() {
        return Ok((String::new(), None));
    }
    let open = session.state::<Workbench>().and_then(|workbench| {
        workbench.documents().iter().find(|document| document.path() == path && document.image().is_none())
    });
    match open {
        Some(document) => Ok((document.text().to_owned(), Some(document.selections().primary().head))),
        None => zen_vfs::read_text(path).map(|text| (text, None)).map_err(|error| {
            CommandError::new(t!("workbench-error-open", path = display(path), error = error.to_string()))
        }),
    }
}

fn same_settings(a: &Configuration, b: &Configuration) -> bool {
    a.kind == b.kind && a.options == b.options && a.common == b.common
}

/// Where to look for something to run: a file and a character offset in it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextParams {
    /// The file, absolute or relative to the project; the active editor's when absent.
    pub path: Option<PathBuf>,
    /// The character offset; the caret's when absent and the file is open, otherwise 0.
    pub offset: Option<usize>,
    /// The line, one-based as `run.targets.list` and failed tests give it, instead of `offset`:
    /// its first character that is not a space.
    pub line: Option<usize>,
}

/// What runs at a place.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextAnswer {
    /// The configuration that runs it, without an id; absent when nothing runs there.
    pub configuration: Option<Configuration>,
    /// The id of a saved configuration with the same settings, which running the context reuses.
    pub existing: Option<String>,
}

fn offered_providers(session: &Session) -> Vec<Arc<dyn RunProvider>> {
    let providers = session.state::<Runs>().map(|runs| runs.providers.clone()).unwrap_or_default();
    let projects = session.state::<crate::plugin_projects::PluginProjects>();
    providers
        .into_iter()
        .filter(|provider| provider.plugin().is_none_or(|id| projects.is_some_and(|projects| projects.installed(id))))
        .collect()
}

fn context_of(session: &mut Session, params: &ContextParams) -> Result<ContextAnswer, CommandError> {
    let root = project_root(session)?;
    let path = match &params.path {
        Some(path) => resolve_path(session, path),
        None => session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
    };
    let (text, caret) = file_text(session, &path)?;
    let offset = params
        .offset
        .or_else(|| params.line.map(|line| line_start(&text, line.saturating_sub(1))))
        .or(caret)
        .unwrap_or(0);
    let providers = offered_providers(session);
    let configuration = if is_scratch(session, &path) {
        single_file_targets(&providers, &path, &text).into_iter().next().map(|(_, target)| target.configuration)
    } else {
        providers.iter().find_map(|provider| provider.context(&root, &path, &text, offset))
    };
    let file = read_file(session, &root)?;
    let trusted = crate::trust::trusted(session, &root);
    let configuration =
        configuration.map(|configuration| if trusted { file.from_template(configuration) } else { configuration });
    let existing = configuration.as_ref().and_then(|wanted| {
        file.configurations.iter().find(|kept| same_settings(kept, wanted)).map(|kept| kept.id.clone())
    });
    Ok(ContextAnswer { configuration, existing })
}

/// The project's run configurations and the kinds that can be made.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationList {
    /// The configurations, in order.
    pub configurations: Vec<Configuration>,
    /// The id of the selected configuration.
    pub selected: Option<String>,
    /// The kinds the providers run.
    pub kinds: Vec<KindInfo>,
}

pub struct ListConfigurations;

impl Command for ListConfigurations {
    type Params = NoParams;
    type Output = ConfigurationList;
    const ID: &'static str = "run.configuration.list";
    const TITLE: &'static str = "Run Configurations";
    const DESCRIPTION: &'static str = "The project's run configurations (the permanent ones from \
         .zen/runConfigurations.json, then the read-only ones adopted from a JetBrains IDE's .idea and .run files, with \
         adoptedFrom, then the window's temporary ones), the selected one, and the kinds of \
         configuration the IDE can run with their option fields.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ConfigurationList, CommandError> {
        let root = project_root(session)?;
        let file = read_file(session, &root)?;
        let kinds = session.state::<Runs>().map(Runs::kinds).unwrap_or_default();
        Ok(ConfigurationList { configurations: file.configurations, selected: file.selected, kinds })
    }
}

/// The parameters of `run.configuration.create`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateParams {
    /// The new configuration; an id is made when it has none or a taken one.
    pub configuration: Configuration,
    /// Whether to select it.
    #[serde(default)]
    pub select: bool,
    /// Whether settings the configuration leaves out are taken from its kind's template
    /// (`run.template.list`); on by default.
    #[serde(default)]
    pub template: Option<bool>,
}

pub struct CreateConfiguration;

impl Command for CreateConfiguration {
    type Params = CreateParams;
    type Output = Configuration;
    const ID: &'static str = "run.configuration.create";
    const TITLE: &'static str = "Add Run Configuration";
    const DESCRIPTION: &'static str = "Adds a run configuration to the project and saves the file; returns it with \
         its id. What it leaves out comes from its kind's template unless `template` is false. A compound \
         configuration (kind `compound`, options.configurations: the ids it starts) that contains itself, directly \
         or not, is refused.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<Configuration, CommandError> {
        let root = project_root(session)?;
        let mut file = read_file(session, &root)?;
        let limit = temporary_limit(session);
        let mut configuration = Configuration { adopted_from: None, ..params.configuration };
        if params.template != Some(false) {
            configuration = file.from_template(configuration);
        }
        crate::compound::check(&file, &configuration)?;
        let created = file.add_keeping(configuration, limit);
        if params.select {
            file.selected = Some(created.id.clone());
        }
        write_file(session, &root, &file)?;
        Ok(created)
    }
}

/// The parameters of `run.configuration.update`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateParams {
    /// The configuration, replacing the one with its id.
    pub configuration: Configuration,
}

pub struct UpdateConfiguration;

impl Command for UpdateConfiguration {
    type Params = UpdateParams;
    type Output = Configuration;
    const ID: &'static str = "run.configuration.update";
    const TITLE: &'static str = "Change Run Configuration";
    const DESCRIPTION: &'static str = "Replaces the run configuration with the same id and saves the file; saving a \
         temporary configuration is an update with temporary: false. An adopted one is refused: save it with \
         run.configuration.saveAdopted first.";

    fn run(&self, session: &mut Session, params: UpdateParams) -> Result<Configuration, CommandError> {
        let root = project_root(session)?;
        let mut file = read_file(session, &root)?;
        let configuration = Configuration { adopted_from: None, ..params.configuration };
        refuse_adopted(&file, &configuration.id)?;
        crate::compound::check(&file, &configuration)?;
        if !file.update(configuration.clone()) {
            return Err(unknown_configuration(&configuration.id));
        }
        write_file(session, &root, &file)?;
        Ok(configuration)
    }
}

/// A configuration by id.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationIdParams {
    /// The configuration's id.
    pub id: String,
}

pub struct DeleteConfiguration;

impl Command for DeleteConfiguration {
    type Params = ConfigurationIdParams;
    type Output = ();
    const ID: &'static str = "run.configuration.delete";
    const TITLE: &'static str = "Remove Run Configuration";
    const DESCRIPTION: &'static str =
        "Removes a run configuration from the project and saves the file; an adopted one is refused.";

    fn run(&self, session: &mut Session, params: ConfigurationIdParams) -> Result<(), CommandError> {
        let root = project_root(session)?;
        let mut file = read_file(session, &root)?;
        refuse_adopted(&file, &params.id)?;
        if !file.remove(&params.id) {
            return Err(unknown_configuration(&params.id));
        }
        write_file(session, &root, &file)
    }
}

pub struct SaveAdoptedConfiguration;

impl Command for SaveAdoptedConfiguration {
    type Params = ConfigurationIdParams;
    type Output = Configuration;
    const ID: &'static str = "run.configuration.saveAdopted";
    const TITLE: &'static str = "Save as Zen Configuration";
    const DESCRIPTION: &'static str = "Copies a configuration adopted from a JetBrains IDE's .idea or .run files into \
         .zen/runConfigurations.json, where it can be changed; the copy keeps the name and takes the adopted one's \
         place and selection, and the JetBrains file is not changed. Returns the copy with its new id.";

    fn run(&self, session: &mut Session, params: ConfigurationIdParams) -> Result<Configuration, CommandError> {
        let root = project_root(session)?;
        let mut file = read_file(session, &root)?;
        let adopted = file
            .get(&params.id)
            .filter(|kept| kept.is_adopted())
            .cloned()
            .ok_or_else(|| CommandError::new(t!("workbench-run-not-adopted", id = params.id.as_str())))?;
        let was_selected = file.selected.as_deref() == Some(params.id.as_str());
        file.configurations.retain(|kept| kept.id != params.id);
        let saved = file.add(Configuration { id: String::new(), adopted_from: None, ..adopted });
        if was_selected {
            file.selected = Some(saved.id.clone());
        }
        write_file(session, &root, &file)?;
        Ok(saved)
    }
}

/// The parameters of `run.configuration.select`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectParams {
    /// The configuration to select; none when absent.
    pub id: Option<String>,
}

pub struct SelectConfiguration;

impl Command for SelectConfiguration {
    type Params = SelectParams;
    type Output = ();
    const ID: &'static str = "run.configuration.select";
    const TITLE: &'static str = "Select Run Configuration";
    const DESCRIPTION: &'static str =
        "Selects the configuration the run widget shows and Run (⌃R) starts; the window keeps the selection.";

    fn run(&self, session: &mut Session, params: SelectParams) -> Result<(), CommandError> {
        let root = project_root(session)?;
        let mut file = read_file(session, &root)?;
        if let Some(id) = &params.id
            && file.get(id).is_none()
        {
            return Err(unknown_configuration(id));
        }
        file.selected = params.id;
        write_file(session, &root, &file)
    }
}

pub struct GetContext;

impl Command for GetContext {
    type Params = ContextParams;
    type Output = ContextAnswer;
    const ID: &'static str = "run.context.get";
    const TITLE: &'static str = "Context Configuration";
    const DESCRIPTION: &'static str = "The configuration that runs what is at a place of a file (an offset, or a \
         one-based line as run.targets.list and failed tests give it; the active editor's caret by default), such as \
         the test or main function around it, and the saved configuration it matches.";

    fn run(&self, session: &mut Session, params: ContextParams) -> Result<ContextAnswer, CommandError> {
        context_of(session, &params)
    }
}

fn resolve_path(session: &Session, path: &Path) -> PathBuf {
    session.state::<Workbench>().map_or_else(|| path.to_owned(), |workbench| workbench.resolve(path))
}

fn line_start(text: &str, line: usize) -> usize {
    let mut offset = 0;
    for (index, row) in text.split('\n').enumerate() {
        if index == line {
            return offset + row.chars().take_while(|c| c.is_whitespace()).count();
        }
        offset += row.chars().count() + 1;
    }
    text.chars().count()
}

/// The parameters of `run.targets.list`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetsParams {
    /// The file, absolute or relative to the project.
    pub path: PathBuf,
    /// Each target with the whole configuration that runs it, as the gutter needs it; without
    /// it a target is its line, kind, name and package, and run.process.start with `context`
    /// `{path, line}` runs it.
    #[serde(default)]
    pub full: Option<bool>,
}

/// Something a file can run, with its provider: compact for agents (card ZneOu428), its
/// configuration only with `full`.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    /// The provider's id.
    pub provider: String,
    /// The line the icon is on, one-based.
    pub line: u32,
    /// What it runs.
    pub kind: zen_run::TargetKind,
    /// The function's or test's name.
    pub name: String,
    /// The package its test results name; left out when not known.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub package: String,
    /// The configuration that runs it, without an id, with `full`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Configuration>,
}

impl TargetInfo {
    fn new(provider: &str, target: RunTarget, full: bool) -> Self {
        Self {
            provider: provider.to_owned(),
            line: target.line + 1,
            kind: target.kind,
            name: target.name,
            package: target.package,
            configuration: full.then_some(target.configuration),
        }
    }
}

/// What `run.targets.list` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetsAnswer {
    /// The targets, by line.
    pub targets: Vec<TargetInfo>,
}

pub struct ListTargets;

impl Command for ListTargets {
    type Params = TargetsParams;
    type Output = TargetsAnswer;
    const ID: &'static str = "run.targets.list";
    const TITLE: &'static str = "Run Targets";
    const DESCRIPTION: &'static str = "What a file can run, by line (one-based): main functions, tests, benchmarks, \
         examples and fuzz tests, each as {provider, line, kind, name, package?}; run.process.start with `context` \
         {path, line} runs one, and `full: true` adds the configuration that runs each, as the gutter's run icons \
         use it.";

    fn run(&self, session: &mut Session, params: TargetsParams) -> Result<TargetsAnswer, CommandError> {
        let full = params.full == Some(true);
        let params = TargetsParams { path: resolve_path(session, &params.path), full: params.full };
        let (text, _) = file_text(session, &params.path)?;
        let context = match project_root(session) {
            Ok(root) => run_context(session, &root),
            Err(_) => RunContext {
                root: params.path.parent().map(Path::to_owned).unwrap_or_default(),
                env: session.state::<Runs>().map(|runs| runs.env.clone()).unwrap_or_default(),
                debugger: session.state::<Runs>().map(|runs| runs.debugger.clone()).unwrap_or_default(),
                ..RunContext::default()
            },
        };
        let providers = offered_providers(session);
        let mut targets: Vec<TargetInfo> = if is_scratch(session, &params.path) {
            single_file_targets(&providers, &params.path, &text)
                .into_iter()
                .map(|(provider, target)| TargetInfo::new(&provider, target, full))
                .collect()
        } else {
            providers
                .iter()
                .flat_map(|provider| {
                    provider
                        .targets(&context, &params.path, &text)
                        .into_iter()
                        .map(|target| TargetInfo::new(provider.id(), target, full))
                })
                .collect()
        };
        let replaced: Vec<(String, u32)> = providers
            .iter()
            .filter_map(|provider| Some((provider.id(), provider.replaces()?)))
            .flat_map(|(by, replaced)| {
                targets
                    .iter()
                    .filter(move |target| target.provider == by)
                    .map(move |target| (replaced.to_owned(), target.line))
            })
            .collect();
        targets.retain(|target| !replaced.contains(&(target.provider.clone(), target.line)));
        targets.sort_by_key(|info| info.line);
        Ok(TargetsAnswer { targets })
    }
}

pub(crate) fn scratch_starter(session: &Session, language: &str) -> Option<ScratchStarter> {
    let runs = session.state::<Runs>()?;
    runs.providers.iter().find_map(|provider| provider.scratch_starter(language))
}

pub(crate) fn scratch_home(session: &Session, file: &Path, text: &str, context: &RunContext) -> Option<PathBuf> {
    let runs = session.state::<Runs>()?;
    runs.providers.iter().find_map(|provider| provider.scratch_home(file, text, context))
}

fn is_scratch(session: &Session, path: &Path) -> bool {
    session.state::<crate::scratch::Scratches>().is_some_and(|scratches| scratches.contains(path))
}

fn single_file_targets(providers: &[Arc<dyn RunProvider>], path: &Path, text: &str) -> Vec<(String, RunTarget)> {
    providers
        .iter()
        .find_map(|provider| {
            let targets = provider.single_file(path, text);
            (!targets.is_empty())
                .then(|| targets.into_iter().map(|target| (provider.id().to_owned(), target)).collect())
        })
        .unwrap_or_default()
}

pub(crate) fn scratch_file_of(session: &Session, configuration: &Configuration) -> Option<PathBuf> {
    fn words(value: &serde_json::Value, found: &mut Vec<String>) {
        match value {
            serde_json::Value::String(text) => found.extend(zen_run::args::split(text)),
            serde_json::Value::Array(items) => items.iter().for_each(|item| words(item, found)),
            serde_json::Value::Object(fields) => fields.values().for_each(|field| words(field, found)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    words(&configuration.options, &mut found);
    let providers = session.state::<Runs>().map(|runs| runs.providers.clone()).unwrap_or_default();
    found.into_iter().map(PathBuf::from).filter(|path| is_scratch(session, path)).find(|path| {
        file_text(session, path).is_ok_and(|(text, _)| {
            single_file_targets(&providers, path, &text)
                .iter()
                .any(|(_, target)| same_settings(&target.configuration, configuration))
        })
    })
}

pub const MENU_TARGET_FILE_LIMIT: u64 = 1 << 20;

pub fn plugin_targets_at(session: &Session, path: &Path) -> Vec<(String, u32)> {
    let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
        return Vec::new();
    };
    if !path.starts_with(&root) {
        return Vec::new();
    }
    let text = if path.is_dir() {
        String::new()
    } else if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() <= MENU_TARGET_FILE_LIMIT) {
        match file_text(session, path) {
            Ok((text, _)) => text,
            Err(_) => return Vec::new(),
        }
    } else {
        return Vec::new();
    };
    let context = RunContext { root, ..RunContext::default() };
    crate::plugin_runs::PluginRun::default()
        .targets(&context, path, &text)
        .into_iter()
        .map(|target| (target.name, target.line))
        .collect()
}

pub fn scratch_run_name(session: &Session, path: &Path) -> Option<String> {
    if !is_scratch(session, path) {
        return None;
    }
    let (text, _) = file_text(session, path).ok()?;
    let providers = session.state::<Runs>().map(|runs| runs.providers.clone()).unwrap_or_default();
    single_file_targets(&providers, path, &text).into_iter().next().map(|(_, target)| target.configuration.name)
}

/// The parameters of `file.scratch.run`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunScratchParams {
    /// The scratch file.
    pub path: PathBuf,
    /// Waits up to this long for the run to end (at most 10 minutes) and answers its summary, as `run.process.start` does.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct RunScratch;

impl Command for RunScratch {
    type Params = RunScratchParams;
    type Output = RunStarted;
    const ID: &'static str = "file.scratch.run";
    const TITLE: &'static str = "Run Scratch File";
    const DESCRIPTION: &'static str = "Runs a scratch file on its own, as the ▶ in its gutter and Run on its \
         context menu do: the language's provider builds and starts that one file with the project's toolchain (Go: \
         go build, Rust: rustc, Java: the single-file source launcher of the project JDK), saved as a temporary \
         configuration, its output in the Run tool window. Refused for a file outside the scratches or a language \
         that cannot run one file.";

    fn run(&self, session: &mut Session, params: RunScratchParams) -> Result<RunStarted, CommandError> {
        let path = resolve_path(session, &params.path);
        if !is_scratch(session, &path) {
            return Err(CommandError::new(t!("workbench-run-not-scratch", path = display(&path))));
        }
        file_text(session, &path)?;
        if scratch_run_name(session, &path).is_none() {
            return Err(CommandError::new(t!("workbench-run-scratch-cannot-run", path = display(&path))));
        }
        let context = ContextParams { path: Some(path), offset: Some(0), line: None };
        StartRun.run(session, StartParams { context: Some(context), wait_ms: params.wait_ms, ..StartParams::default() })
    }
}

/// The parameters of `run.process.start`: `configuration`, else `configurationId`, else `context`, else the selected one.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StartParams {
    /// A saved configuration to run.
    pub configuration_id: Option<String>,
    /// Runs what is at a place, saving it as a temporary configuration unless a saved one matches.
    pub context: Option<ContextParams>,
    /// A configuration to run as given, without saving it.
    pub configuration: Option<Configuration>,
    /// Waits up to this long (at most 10 minutes) for the run to end and answers its summary too,
    /// as `run.tests.get` with `summary` does, so that nothing needs to be polled.
    pub wait_ms: Option<u64>,
    /// The text `$Prompt$` stands for in this run's arguments and working directory.
    pub prompt: Option<String>,
    /// The file `$FilePrompt$` stands for in this run.
    pub file_prompt: Option<PathBuf>,
    /// Refuses with `rerunConfirmation` when it runs already and allows one instance, so the caller
    /// can ask first; without it the running instance is restarted.
    pub confirm_rerun: bool,
    /// After `confirmationNeeded`: its confirmation, once the person allowed it.
    pub confirmation: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Prompts {
    pub prompt: Option<String>,
    pub file_prompt: Option<PathBuf>,
}

impl Prompts {
    pub(crate) fn of(params: &StartParams) -> Self {
        Self { prompt: params.prompt.clone(), file_prompt: params.file_prompt.clone() }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Recording {
    Coverage(PathBuf),
    Profile(ProfileKind, PathBuf),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Launch {
    pub prompts: Prompts,
    pub confirm_rerun: bool,
    pub rerun_of: Option<u64>,
    pub recording: Option<Recording>,
    pub confirmation: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
fn is_backup_run(configuration: &Configuration) -> bool {
    configuration.kind == crate::database::backups::RUN_KIND
}

#[cfg(target_arch = "wasm32")]
fn is_backup_run(_: &Configuration) -> bool {
    false
}

pub const MAX_RUN_WAIT: Duration = Duration::from_secs(600);

/// A run that was started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunStarted {
    /// The run's id, for `run.output.get` and `run.tests.get`.
    pub run_id: u64,
    /// With `waitMs`: where the run is and its test counts, once it ended or the time was up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<RunSummary>,
    /// A compound's runs; `runId` is the first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<RunStarted>,
    /// A compound's start errors.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl RunStarted {
    pub fn of(run_id: u64) -> Self {
        Self { run_id, summary: None, children: Vec::new(), errors: Vec::new() }
    }
}

/// A run in a few bytes: where it is and how many tests ended each way (`run.tests.get` with
/// `summary`, `run.process.start` with `waitMs`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    /// Where the run is.
    pub state: RunState,
    /// Whether the run has ended.
    pub done: bool,
    /// The exit code, once the run has ended by itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Why the run failed, if it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How many tests ended each way.
    pub counts: TestCounts,
    /// How long the tests took, in milliseconds, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl RunSummary {
    pub fn of(output: &RunOutput) -> Self {
        Self {
            state: output.state(),
            done: !output.is_running(),
            exit_code: output.exit_code(),
            error: output.error_worded(),
            counts: output.tree().counts(),
            duration_ms: output.tree().root().duration_ms,
        }
    }
}

pub(crate) fn configuration_to_start(
    session: &mut Session,
    params: StartParams,
) -> Result<Configuration, CommandError> {
    let root = project_root(session)?;
    if let Some(configuration) = params.configuration {
        check_options(session, &configuration)?;
        crate::trust::guard_run(session, &root, &configuration)?;
        return Ok(configuration);
    }
    let mut file = read_file(session, &root)?;
    let chosen = if let Some(id) = &params.configuration_id {
        file.get(id).cloned().ok_or_else(|| unknown_configuration(id))?
    } else if let Some(context) = &params.context {
        let answer = context_of(session, context)?;
        match (answer.existing.and_then(|id| file.get(&id).cloned()), answer.configuration) {
            (Some(existing), _) => existing,
            (None, Some(configuration)) => {
                let limit = temporary_limit(session);
                file.add_keeping(Configuration { id: String::new(), temporary: true, ..configuration }, limit)
            }
            (None, None) => return Err(CommandError::new(t!("workbench-run-no-context"))),
        }
    } else {
        file.selected_configuration().cloned().ok_or_else(|| CommandError::new(t!("workbench-run-no-configuration")))?
    };
    crate::trust::guard_run(session, &root, &chosen)?;
    if file.selected.as_deref() != Some(chosen.id.as_str()) {
        file.selected = Some(chosen.id.clone());
        write_file(session, &root, &file)?;
    }
    Ok(chosen)
}

pub(crate) fn check_options(session: &Session, configuration: &Configuration) -> Result<(), CommandError> {
    let kinds = session.state::<Runs>().map(Runs::kinds).unwrap_or_default();
    let Some(kind) = kinds.iter().find(|info| info.spec.id == configuration.kind) else {
        let known: Vec<&str> = kinds.iter().map(|info| info.spec.id.as_str()).collect();
        return Err(CommandError::new(t!(
            "workbench-run-unknown-kind-of",
            kind = configuration.kind.as_str(),
            kinds = known.join(", ")
        )));
    };
    let Some(options) = configuration.options.as_object() else { return Ok(()) };
    let mut keys: Vec<&str> = kind.spec.fields.iter().map(|field| field.key.as_str()).collect();
    let defaults = kind.spec.defaults.as_object().into_iter().flat_map(|defaults| defaults.keys());
    for key in defaults.map(String::as_str) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let unknown: Vec<&str> = options.keys().map(String::as_str).filter(|key| !keys.contains(key)).collect();
    if unknown.is_empty() || keys.is_empty() {
        return Ok(());
    }
    Err(CommandError::new(t!(
        "workbench-run-unknown-options",
        kind = configuration.kind.as_str(),
        unknown = unknown.join(", "),
        keys = keys.join(", ")
    )))
}

pub(crate) fn add_missing(
    session: &mut Session,
    root: &Path,
    configurations: Vec<Configuration>,
) -> Result<Vec<Configuration>, CommandError> {
    let mut file = read_file(session, root)?;
    let mut added = Vec::new();
    for configuration in configurations {
        if file.configurations.iter().any(|kept| kept.kind == configuration.kind && kept.name == configuration.name) {
            continue;
        }
        let fresh = Configuration { id: String::new(), temporary: false, adopted_from: None, ..configuration };
        added.push(file.add(fresh));
    }
    if !added.is_empty() {
        write_file(session, root, &file)?;
    }
    Ok(added)
}

pub(crate) fn temporary_limit(session: &Session) -> usize {
    session.settings().get_or(TEMPORARY_LIMIT, zen_project::run::TEMPORARY_LIMIT).clamp(1, MAX_TEMPORARY_LIMIT)
}

pub(crate) fn start(session: &mut Session, configuration: Configuration) -> Result<RunStarted, CommandError> {
    start_with(session, configuration, Launch::default(), &[])
}

pub(crate) fn start_chained(
    session: &mut Session,
    configuration: Configuration,
    chain: &[String],
) -> Result<RunStarted, CommandError> {
    start_with(session, configuration, Launch::default(), chain)
}

pub(crate) fn start_with(
    session: &mut Session,
    configuration: Configuration,
    launch: Launch,
    chain: &[String],
) -> Result<RunStarted, CommandError> {
    crate::login_env::sync(session);
    if crate::compound::is_compound(&configuration) {
        return crate::compound::start(session, configuration, launch, chain);
    }
    let root = project_root(session)?;
    crate::trust::guard_configuration(session, &root, &configuration)?;
    crate::plugin_runs::guard_agent(session, &configuration, launch.confirmation.as_deref())?;
    let caller = if zen_core::macros::by_agent(session) {
        crate::plugin_runs::Caller::Agent
    } else {
        crate::plugin_runs::Caller::Person(launch.confirmation.clone())
    };
    let (running, rerun) = {
        let runs = session.state_mut::<Runs>();
        let running: Vec<(u64, u32)> =
            runs.instances(&configuration.id).iter().map(|run| (run.id, run.instance)).collect();
        let rerun = launch.rerun_of.and_then(|id| runs.run(id)).map(|run| (run.id, run.instance));
        (running, rerun)
    };
    let multiple = configuration.common.allow_multiple_instances;
    if launch.confirm_rerun && !multiple && rerun.is_none() && !running.is_empty() {
        return Err(CommandError::with_code(
            RERUN_CONFIRMATION,
            t!("workbench-run-already-running", name = configuration.name.as_str()),
        ));
    }
    SaveAll.run(session, NoParams {})?;
    let launched = launched(session, &root, &configuration, &launch.prompts)?;
    let (launched, place) = crate::plugin_runs::with_window_values(session, launched)?;
    let mut context = run_context(session, &root);
    if !crate::trust::trusted(session, &root)
        && let Some(folder) =
            scratch_file_of(session, &configuration).and_then(|file| file.parent().map(Path::to_owned))
    {
        context.root = folder;
        context.modules.clear();
    }
    let limit = console_limit(session.settings());
    let runs = session.state_mut::<Runs>();
    runs.set_console_limit(limit);
    let stop = |runs: &Runs, id: u64| {
        if let Some(run) = runs.run(id).filter(|run| run.is_running()) {
            run.stop();
        }
    };
    let instance = match rerun {
        Some((id, number)) => {
            stop(runs, id);
            Instance { number, replaces: Some(id) }
        }
        None if multiple => {
            let number = (1..).find(|number| running.iter().all(|(_, taken)| taken != number)).unwrap_or(1);
            Instance { number, replaces: None }
        }
        None => {
            for (id, _) in &running {
                stop(runs, *id);
            }
            Instance { number: 1, replaces: running.last().map(|(id, _)| *id) }
        }
    };
    let tasks = configuration.common.before_launch.clone();
    let gate = (!tasks.is_empty()).then(crate::before_launch::Gate::new);
    runs.set_caller(caller, Some(configuration.clone()));
    let prepared = runs.prepare(configuration, &launched, &context, instance, gate.clone(), launch.recording.as_ref());
    runs.set_caller(crate::plugin_runs::Caller::Unknown, None);
    let (run_id, begin) = prepared?;
    if crate::plugin_runs::is_diff(&launched)
        && let Some(output) = session.state::<Runs>().and_then(|runs| runs.run(run_id)).map(|run| run.output.clone())
    {
        let place = place.iter().map(|(title, value)| format!("{title} {value}")).collect::<Vec<_>>().join(", ");
        crate::plugin_diff::watch(session, run_id, output, crate::plugin_runs::diff_file(&launched), place);
    } else if let Some(document_type) = crate::plugin_runs::document_type(&launched)
        && let Some(output) = session.state::<Runs>().and_then(|runs| runs.run(run_id)).map(|run| run.output.clone())
    {
        let command = crate::plugin_runs::shown_command(&launched);
        let file = crate::plugin_runs::diff_file(&launched);
        crate::plugin_document::watch(
            session,
            run_id,
            output,
            crate::plugin_document::Asked { file, command, document_type },
        );
    }
    match gate {
        Some(gate) => crate::before_launch::launch(session, &root, run_id, &launched, gate, begin, chain),
        None => begin(),
    }
    Ok(RunStarted::of(run_id))
}

fn parent_variables(session: &Session) -> impl Fn(&str) -> Option<String> + use<> {
    let env = session.state::<Runs>().map(|runs| runs.env.env.clone()).unwrap_or_default();
    move |name: &str| {
        env.iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .or_else(|| std::env::var(name).ok())
    }
}

fn set_variable(env: &mut Vec<(String, String)>, name: String, value: String) {
    match env.iter_mut().find(|(key, _)| *key == name) {
        Some(kept) => kept.1 = value,
        None => env.push((name, value)),
    }
}

pub(crate) fn env_file_path(root: &Path, file: &Path) -> PathBuf {
    if file.is_absolute() { file.to_owned() } else { root.join(file) }
}

pub(crate) fn launched(
    session: &mut Session,
    root: &Path,
    configuration: &Configuration,
    prompts: &Prompts,
) -> Result<Configuration, CommandError> {
    let name = configuration.name.as_str();
    let common = &configuration.common;
    let folder = common.working_directory.as_deref().map(|folder| folder.to_string_lossy().into_owned());
    let texts = [common.program_arguments.as_str(), common.tool_arguments.as_str(), folder.as_deref().unwrap_or("")];
    let mut asked = Vec::new();
    for item in texts.iter().flat_map(|text| zen_run::macros::used(text)) {
        let given = match item {
            zen_run::macros::Macro::Prompt => prompts.prompt.is_some(),
            zen_run::macros::Macro::FilePrompt => prompts.file_prompt.is_some(),
            _ => true,
        };
        if !given && !asked.contains(&item.token()) {
            asked.push(item.token());
        }
    }
    if !asked.is_empty() {
        return Err(CommandError::with_code(
            PROMPT_NEEDED,
            t!("workbench-run-prompt-needed", name = name, tokens = asked.join(", ")),
        ));
    }
    let mut context = crate::external_tools::macro_context(session, None);
    context.project = Some(root.to_owned());
    context.prompt = prompts.prompt.clone();
    context.file_prompt = prompts.file_prompt.clone();
    let fail = |item: zen_run::macros::Macro| {
        CommandError::new(t!(
            "workbench-run-no-value",
            name = name,
            token = item.token(),
            reason = crate::external_tools::missing_reason(item)
        ))
    };
    let parent = parent_variables(session);
    let mut env = Vec::new();
    for file in &common.env_files {
        let path = env_file_path(root, file);
        crate::hidden_files::guard_change(session, root, &[&path])?;
        let read = zen_run::env_file::read(&path).map_err(|error| {
            CommandError::new(t!(
                "workbench-run-env-file",
                name = name,
                path = display(&path),
                error = error.to_string()
            ))
        })?;
        for (key, value) in read {
            set_variable(&mut env, key, zen_run::env_file::expand_references(&value, &parent));
        }
    }
    for (key, value) in &common.env {
        set_variable(&mut env, key.clone(), zen_run::env_file::expand_references(value, &parent));
    }
    let working_directory = match &folder {
        Some(folder) => Some(PathBuf::from(zen_run::macros::expand(folder, &context).map_err(fail)?)),
        None => None,
    };
    let common = zen_run::Common {
        working_directory,
        env,
        program_arguments: zen_run::macros::expand_line(&common.program_arguments, &context).map_err(fail)?,
        tool_arguments: zen_run::macros::expand_line(&common.tool_arguments, &context).map_err(fail)?,
        env_files: Vec::new(),
        ..common.clone()
    };
    Ok(Configuration { common, ..configuration.clone() })
}

pub fn wait_for_run(session: &mut Session, run_id: u64, wait: Duration) -> Result<RunSummary, CommandError> {
    let output = session
        .state::<Runs>()
        .and_then(|runs| runs.run(run_id))
        .map(|run| run.output.clone())
        .ok_or_else(|| unknown_run(run_id))?;
    let deadline = Instant::now() + wait.min(MAX_RUN_WAIT);
    loop {
        let pending = crate::before_launch::advance(session);
        let left = deadline.saturating_duration_since(Instant::now());
        if !pending {
            output.wait_end(left);
            break;
        }
        if output.wait_end(left.min(crate::before_launch::STEP)) || left.is_zero() {
            break;
        }
    }
    Ok(output.read(RunSummary::of))
}

pub struct StartRun;

impl Command for StartRun {
    type Params = StartParams;
    type Output = RunStarted;
    const ID: &'static str = "run.process.start";
    const TITLE: &'static str = "Run";
    const DESCRIPTION: &'static str = "Saves modified files, runs a configuration (⌃R runs the selected one, ⌃⇧R the \
         context at the caret) and returns the run's id; read its console with run.output.get and its tests with \
         run.tests.get. A configuration that is running already is stopped and started again, unless its \
         common.allowMultipleInstances is on: then another instance starts, named `name (2)`. The .env files of \
         common.envFiles are read first, the macros of the arguments and working directory expand (prompt and \
         filePrompt answer $Prompt$ and $FilePrompt$; without them the code is promptNeeded). With waitMs it \
         answers once the run has ended (or the time is up) with its summary: state, exit code and test counts.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<RunStarted, CommandError> {
        let wait = params.wait_ms.map(Duration::from_millis);
        let launch = Launch {
            prompts: Prompts::of(&params),
            confirm_rerun: params.confirm_rerun,
            confirmation: params.confirmation.clone(),
            ..Launch::default()
        };
        let configuration = configuration_to_start(session, params)?;
        let started = start_with(session, configuration, launch, &[])?;
        match wait {
            Some(wait) if !started.children.is_empty() => {
                let mut children = Vec::new();
                for child in started.children.clone() {
                    let summary = wait_for_run(session, child.run_id, wait)?;
                    children.push(RunStarted { summary: Some(summary), ..child });
                }
                Ok(RunStarted {
                    summary: children.first().and_then(|first| first.summary.clone()),
                    children,
                    ..started
                })
            }
            Some(wait) => {
                let summary = wait_for_run(session, started.run_id, wait)?;
                Ok(RunStarted { summary: Some(summary), ..started })
            }
            None => Ok(started),
        }
    }
}

/// The parameters of `run.process.stop`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StopParams {
    /// The run to stop; the one started last that still runs when absent.
    pub run_id: Option<u64>,
    /// Stops every running run of this configuration instead, and for a compound configuration
    /// every run it started.
    pub configuration_id: Option<String>,
}

/// What `run.process.stop` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopAnswer {
    /// The run that was stopped; absent when nothing was running.
    pub stopped: Option<u64>,
    /// With `configurationId`: every run that was stopped.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<u64>,
}

pub struct StopRun;

impl Command for StopRun {
    type Params = StopParams;
    type Output = StopAnswer;
    const ID: &'static str = "run.process.stop";
    const TITLE: &'static str = "Stop";
    const DESCRIPTION: &'static str = "Stops a run's programs (⌘F2): the given run, or the one started last that is still running; they are \
         interrupted, then killed after 2 s or on a second stop. With configurationId it stops every running run of \
         that configuration, and for a compound configuration every run the compound started.";

    fn run(&self, session: &mut Session, params: StopParams) -> Result<StopAnswer, CommandError> {
        let runs = session.state_mut::<Runs>();
        if let Some(id) = params.configuration_id.filter(|_| params.run_id.is_none()) {
            let stopped: Vec<u64> = runs
                .of_configuration(&id)
                .into_iter()
                .filter(|run| run.is_running())
                .filter(|run| run.stop())
                .map(|run| run.id)
                .collect();
            return Ok(StopAnswer { stopped: stopped.first().copied(), runs: stopped });
        }
        let run = match params.run_id {
            Some(id) => runs.run(id).ok_or_else(|| unknown_run(id))?,
            None => match runs.latest_running() {
                Some(run) => run,
                None => return Ok(StopAnswer { stopped: None, runs: Vec::new() }),
            },
        };
        Ok(StopAnswer { stopped: run.stop().then_some(run.id), runs: Vec::new() })
    }
}

/// A run as `run.process.list` shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    /// The run's id.
    pub run_id: u64,
    /// The configuration's name.
    pub name: String,
    /// The configuration's id; empty for one run without saving.
    pub configuration_id: String,
    /// The configuration's kind.
    pub kind: String,
    /// Where the run is.
    pub state: RunState,
    /// The exit code, once the run has ended by itself.
    pub exit_code: Option<i32>,
    /// Whether it is a debug session's run.
    pub debug: bool,
    /// Which running instance of its configuration it is, from 1; the name ends in ` (2)` from the second.
    pub instance: u32,
    /// The run it stopped and took the place of, whose tab it reuses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<u64>,
    /// The id of the compound configuration that started it, if one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compound: Option<String>,
    /// Whether the Run tool window opens for it (common.activateToolWindow).
    pub activate_tool_window: bool,
    /// Whether the Run tool window takes the keyboard for it (common.focusToolWindow).
    pub focus_tool_window: bool,
    /// For an external tool's run: how its console shows (`run.externalTools.run`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_tool: Option<ToolConsole>,
}

/// How an external tool's run shows in the Run tool window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolConsole {
    /// The tool's id.
    pub tool_id: String,
    /// Whether the run gets a tab in the Run tool window.
    pub open_console: bool,
    /// Whether the Run tool window comes forward on standard output.
    pub activate_on_stdout: bool,
    /// Whether the Run tool window comes forward on standard error.
    pub activate_on_stderr: bool,
    /// Whether the run gets a tab once it has failed, when it gets none from the start (a file watcher's On error).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub console_on_error: bool,
}

/// What `run.process.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunList {
    /// The runs of the session, oldest first.
    pub runs: Vec<RunInfo>,
}

pub struct ListRuns;

impl Command for ListRuns {
    type Params = NoParams;
    type Output = RunList;
    const ID: &'static str = "run.process.list";
    const TITLE: &'static str = "Runs";
    const DESCRIPTION: &'static str =
        "The runs of this window, running and ended, as the Run tool window's tabs show them.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RunList, CommandError> {
        let runs = session.state::<Runs>().map(Runs::runs).unwrap_or_default();
        let runs = runs
            .iter()
            .map(|run| {
                let (state, exit_code) = run.output.read(|output| (output.state(), output.exit_code()));
                RunInfo {
                    run_id: run.id,
                    name: run.title(),
                    configuration_id: run.configuration.id.clone(),
                    kind: run.configuration.kind.clone(),
                    state,
                    exit_code,
                    debug: run.debug,
                    instance: run.instance,
                    replaces: run.replaces,
                    compound: run.compound.clone(),
                    activate_tool_window: run.configuration.common.activate_tool_window,
                    focus_tool_window: run.configuration.common.focus_tool_window,
                    external_tool: zen_run::external_tool::ToolLaunch::of(&run.configuration).map(|launch| {
                        ToolConsole {
                            tool_id: launch.tool_id,
                            open_console: launch.open_console,
                            activate_on_stdout: launch.activate_on_stdout,
                            activate_on_stderr: launch.activate_on_stderr,
                            console_on_error: launch.console_on_error,
                        }
                    }),
                }
            })
            .collect();
        Ok(RunList { runs })
    }
}

/// A run by id.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunIdParams {
    /// The run's id.
    pub run_id: u64,
}

/// A run to start again.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RerunParams {
    /// The run's id.
    pub run_id: u64,
    /// Waits up to this long (at most 10 minutes) for the new run to end and answers its summary too, as
    /// `run.process.start` does.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

fn summarised(session: &mut Session, started: RunStarted, wait_ms: Option<u64>) -> Result<RunStarted, CommandError> {
    match wait_ms.map(Duration::from_millis) {
        Some(wait) => {
            let summary = wait_for_run(session, started.run_id, wait)?;
            Ok(RunStarted { summary: Some(summary), ..started })
        }
        None => Ok(started),
    }
}

pub struct RerunRun;

impl Command for RerunRun {
    type Params = RerunParams;
    type Output = RunStarted;
    const ID: &'static str = "run.process.rerun";
    const TITLE: &'static str = "Rerun";
    const DESCRIPTION: &'static str = "Starts a run's configuration again, as it was when the run started; returns the new run's id. A run with coverage runs with coverage again, a run with the profiler records again. A database backup runs again with the same tool and options and replaces its file; a database restore is refused with `confirmExecute` (its `retry` is db.backup.restore) and the Restore dialog asks. With waitMs it answers once the new run has ended (or the time is up) with its summary.";

    fn run(&self, session: &mut Session, params: RerunParams) -> Result<RunStarted, CommandError> {
        let started = rerun(session, params.run_id)?;
        summarised(session, started, params.wait_ms)
    }
}

fn rerun(session: &mut Session, run_id: u64) -> Result<RunStarted, CommandError> {
    let (configuration, coverage, profile) = session
        .state::<Runs>()
        .and_then(|runs| runs.run(run_id))
        .map(|run| (run.configuration.clone(), run.coverage, run.profile))
        .ok_or_else(|| unknown_run(run_id))?;
    let backup = cfg!(not(target_arch = "wasm32")) && is_backup_run(&configuration);
    if configuration.kind == zen_run::external_tool::KIND {
        crate::trust::guard_agent_project(session)?;
    } else if !backup {
        let root = project_root(session)?;
        crate::trust::guard_run(session, &root, &configuration)?;
    }
    if coverage {
        let launch = Launch { rerun_of: Some(run_id), ..Launch::default() };
        let run_id = crate::coverage::start(session, configuration, launch)?;
        return Ok(RunStarted::of(run_id));
    }
    if let Some(kind) = profile {
        let launch = Launch { rerun_of: Some(run_id), ..Launch::default() };
        let (run_id, _) = crate::profile::start(session, configuration, kind, launch)?;
        return Ok(RunStarted::of(run_id));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if configuration.kind == crate::database::backups::RUN_KIND {
        let run_id = crate::database::backups::rerun(session, run_id, &configuration)?;
        return Ok(RunStarted::of(run_id));
    }
    start_with(session, configuration, Launch { rerun_of: Some(run_id), ..Launch::default() }, &[])
}

/// The parameters of `run.output.get`.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputParams {
    /// The run's id.
    pub run_id: u64,
    /// The first console line to return; 0 when absent.
    #[serde(default)]
    pub from: Option<usize>,
    /// How long to wait for a line after `from` while the run is running, in milliseconds (at most 10 000).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `run.output.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputAnswer {
    /// The console lines from `from` on.
    pub lines: Vec<ConsoleLine>,
    /// The `from` to read the next lines with.
    pub next: usize,
    /// Where the run is.
    pub state: RunState,
    /// The exit code, once the run has ended by itself.
    pub exit_code: Option<i32>,
    /// Why the run failed, if it did.
    pub error: Option<String>,
}

pub struct GetOutput;

impl Command for GetOutput {
    type Params = OutputParams;
    type Output = OutputAnswer;
    const ID: &'static str = "run.output.get";
    const TITLE: &'static str = "Run Console";
    const DESCRIPTION: &'static str = "A run's console lines from `from` on (stream, text without escapes, file links), \
         waiting up to waitMs for new ones while it runs, and its state; read on from `next`.";

    fn run(&self, session: &mut Session, params: OutputParams) -> Result<OutputAnswer, CommandError> {
        let output = session
            .state::<Runs>()
            .and_then(|runs| runs.run(params.run_id))
            .map(|run| run.output.clone())
            .ok_or_else(|| unknown_run(params.run_id))?;
        let from = params.from.unwrap_or(0);
        if let Some(wait) = params.wait_ms {
            output.wait(from, Duration::from_millis(wait).min(crate::language::MAX_WAIT));
        }
        Ok(output.read(|output| {
            let lines = output.console().since(from).to_vec();
            OutputAnswer {
                next: from.max(output.console().len()),
                lines,
                state: output.state(),
                exit_code: output.exit_code(),
                error: output.error_worded(),
            }
        }))
    }
}

/// The parameters of `run.tests.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TestsParams {
    /// The run's id.
    pub run_id: u64,
    /// Only where the run is and the counts, a few hundred bytes: for polling.
    pub summary: bool,
    /// The summary and the tests that failed, each with the place it failed (`file:line`) and its
    /// message, instead of the whole tree.
    pub failed_only: bool,
    /// One test's console output, by its full name as `failedOnly` or the tree gives it.
    pub test: Option<String>,
    /// The package of `test`, when two packages have a test of that name.
    pub package: Option<String>,
}

/// A test that failed, as `run.tests.get` with `failedOnly` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FailedTest {
    /// The package it is in.
    pub package: String,
    /// Its full name, subtests after `/`: what `test` takes to fetch its output.
    pub test: String,
    /// How long it took, in milliseconds, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Where it failed: the first file position of its output (relative to the project when in
    /// it), e.g. a failed assertion's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The line there, one-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column there, one-based, when the output gives one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// What went wrong: its output from that place on, at most a few lines.
    pub message: String,
}

/// One test's console output, as `run.tests.get` with `test` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestOutput {
    /// The package it is in.
    pub package: String,
    /// Its full name.
    pub test: String,
    /// How it went.
    pub status: TestStatus,
    /// How long it took, in milliseconds, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Its console lines, as text.
    pub output: Vec<String>,
}

/// What `run.tests.get` answers: the whole tree by default; with `summary`, `failedOnly` or
/// `test`, only what they ask for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestsAnswer {
    /// The run's tests: the run, its packages, their tests and subtests (only by default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<TestNode>,
    /// How many tests ended each way (by default; in `summary` otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<TestCounts>,
    /// Whether the run has ended (by default; in `summary` otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    /// The run in a few bytes (`summary`, `failedOnly`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<RunSummary>,
    /// The tests that failed themselves, in tree order (`failedOnly`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<Vec<FailedTest>>,
    /// The output of the test asked for (`test`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<TestOutput>,
}

const MESSAGE_LINES: usize = 6;
const MESSAGE_CHARS: usize = 600;

fn test_nodes<'a>(node: &'a TestNode, out: &mut Vec<&'a TestNode>) {
    if node.kind == TestNodeKind::Test {
        out.push(node);
    }
    node.children.iter().for_each(|child| test_nodes(child, out));
}

fn node_lines<'a>(node: &TestNode, console: &'a Console) -> Vec<&'a ConsoleLine> {
    node.lines.iter().flat_map(|range| console.range(range.clone())).collect()
}

fn link_target(link: &LinkSpan, cwd: &Path, root: &Path) -> Option<PathBuf> {
    let candidates =
        if link.file.is_absolute() { vec![link.file.clone()] } else { vec![link.resolve(cwd), root.join(&link.file)] };
    candidates.into_iter().find(|path| path.is_file())
}

fn link_file(link: &LinkSpan, cwd: &Path, root: &Path) -> String {
    match link_target(link, cwd, root) {
        Some(path) => match path.strip_prefix(root) {
            Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
            Err(_) => path.display().to_string(),
        },
        None => link.file.display().to_string(),
    }
}

fn failure_message(lines: &[&ConsoleLine], from: usize) -> String {
    let frame = |index: usize| lines.get(index).is_some_and(|line| line.text.trim_start().starts_with("at "));
    let mut from = from;
    if frame(from) {
        while from > 0 && frame(from - 1) {
            from -= 1;
        }
        from = from.saturating_sub(1);
    }
    let kept: Vec<&str> = lines
        .iter()
        .skip(from)
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .take(MESSAGE_LINES)
        .collect();
    let message = kept.join("\n");
    if message.chars().count() > MESSAGE_CHARS {
        return message.chars().take(MESSAGE_CHARS).collect::<String>() + "…";
    }
    message
}

fn failures(output: &RunOutput, cwd: &Path, root: &Path) -> Vec<FailedTest> {
    let mut nodes = Vec::new();
    test_nodes(output.tree().root(), &mut nodes);
    let console = output.console();
    let failed = |node: &TestNode| node.status == TestStatus::Failed;
    nodes
        .into_iter()
        .filter(|node| failed(node) && !node.children.iter().any(failed))
        .map(|node| {
            let lines = node_lines(node, console);
            let links: Vec<(usize, &LinkSpan)> = lines
                .iter()
                .enumerate()
                .flat_map(|(index, line)| line.links.iter().map(move |link| (index, link)))
                .collect();
            let place = links
                .iter()
                .find(|(_, link)| link_target(link, cwd, root).is_some())
                .or_else(|| links.first())
                .copied();
            FailedTest {
                package: node.package.clone(),
                test: node.test.clone().unwrap_or_else(|| node.name.clone()),
                duration_ms: node.duration_ms,
                file: place.map(|(_, link)| link_file(link, cwd, root)),
                line: place.map(|(_, link)| link.line),
                column: place.and_then(|(_, link)| link.column),
                message: failure_message(&lines, place.map_or(0, |(index, _)| index)),
            }
        })
        .collect()
}

pub struct GetTests;

impl Command for GetTests {
    type Params = TestsParams;
    type Output = TestsAnswer;
    const ID: &'static str = "run.tests.get";
    const TITLE: &'static str = "Test Results";
    const DESCRIPTION: &'static str = "A run's test results: with summary only its state and test counts (a few \
         hundred bytes, for polling); with failedOnly the summary and the tests that failed, each with file, line \
         and message; with test (and package) one test's console output; without them the whole tree with each \
         test's status, duration and console line ranges, and the counts.";

    fn run(&self, session: &mut Session, params: TestsParams) -> Result<TestsAnswer, CommandError> {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        let run = session
            .state::<Runs>()
            .and_then(|runs| runs.run(params.run_id))
            .ok_or_else(|| unknown_run(params.run_id))?;
        let cwd = run.command_lines.first().map_or_else(|| root.clone(), |line| line.cwd.clone());
        run.output.read(|output| {
            if let Some(test) = &params.test {
                let mut nodes = Vec::new();
                test_nodes(output.tree().root(), &mut nodes);
                let node = nodes
                    .into_iter()
                    .find(|node| {
                        node.test.as_deref() == Some(test.as_str())
                            && params.package.as_deref().is_none_or(|package| node.package == package)
                    })
                    .ok_or_else(|| CommandError::new(t!("workbench-run-unknown-test", test = test.as_str())))?;
                let lines = node_lines(node, output.console()).into_iter().map(|line| line.text.clone());
                let output = TestOutput {
                    package: node.package.clone(),
                    test: test.clone(),
                    status: node.status,
                    duration_ms: node.duration_ms,
                    output: lines.collect(),
                };
                return Ok(TestsAnswer { output: Some(output), ..TestsAnswer::default() });
            }
            if params.summary || params.failed_only {
                let failed = params.failed_only.then(|| failures(output, &cwd, &root));
                return Ok(TestsAnswer { summary: Some(RunSummary::of(output)), failed, ..TestsAnswer::default() });
            }
            Ok(TestsAnswer {
                tree: Some(output.tree().root().clone()),
                counts: Some(output.tree().counts()),
                done: Some(output.tree().is_done()),
                ..TestsAnswer::default()
            })
        })
    }
}

fn rerun_tests(session: &mut Session, run_id: u64, tests: &[TestId]) -> Result<RunStarted, CommandError> {
    let runs = session.state::<Runs>().ok_or_else(|| unknown_run(run_id))?;
    let run = runs.run(run_id).ok_or_else(|| unknown_run(run_id))?;
    let configuration = provider_for(&runs.providers, &run.configuration.kind)
        .and_then(|provider| provider.rerun_failed(&run.configuration, tests))
        .ok_or_else(|| {
            CommandError::new(t!("workbench-run-cannot-rerun-failed", kind = run.configuration.kind.as_str()))
        })?;
    let root = project_root(session)?;
    crate::trust::guard_run(session, &root, &configuration)?;
    start(session, configuration)
}

pub struct RerunFailed;

impl Command for RerunFailed {
    type Params = RerunParams;
    type Output = RunStarted;
    const ID: &'static str = "run.tests.rerunFailed";
    const TITLE: &'static str = "Rerun Failed Tests";
    const DESCRIPTION: &'static str = "Runs only the tests that failed in a run, with the same settings; returns the new run's id, and with waitMs \
         its summary once it ended.";

    fn run(&self, session: &mut Session, params: RerunParams) -> Result<RunStarted, CommandError> {
        let runs = session.state::<Runs>().ok_or_else(|| unknown_run(params.run_id))?;
        let run = runs.run(params.run_id).ok_or_else(|| unknown_run(params.run_id))?;
        let failed = run.output.read(|output| output.tree().failed());
        if failed.is_empty() {
            return Err(CommandError::new(t!("workbench-run-no-failed")));
        }
        let started = rerun_tests(session, params.run_id, &failed)?;
        summarised(session, started, params.wait_ms)
    }
}

/// The parameters of `run.tests.rerun` and `run.tests.source`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TestParams {
    /// The run.
    pub run_id: u64,
    /// The test's full name, subtests after `/`; absent to take every test of `package`.
    pub test: Option<String>,
    /// The package, needed when two packages have a test of that name.
    pub package: Option<String>,
}

fn id_of(node: &TestNode) -> Option<TestId> {
    node.test.clone().map(|test| TestId { package: node.package.clone(), test })
}

fn named_test(root: &TestNode, params: &TestParams) -> Option<TestId> {
    let mut nodes = Vec::new();
    test_nodes(root, &mut nodes);
    nodes
        .into_iter()
        .find(|node| {
            node.test == params.test && params.package.as_deref().is_none_or(|package| node.package == package)
        })
        .and_then(id_of)
}

fn chosen_tests(root: &TestNode, params: &TestParams) -> Vec<TestId> {
    if params.test.is_some() {
        return named_test(root, params).into_iter().collect();
    }
    root.children
        .iter()
        .filter(|node| params.package.as_deref() == Some(node.package.as_str()))
        .flat_map(|node| node.children.iter().filter_map(id_of))
        .collect()
}

fn unknown_test(params: &TestParams) -> CommandError {
    let test = params.test.as_deref().or(params.package.as_deref()).unwrap_or_default();
    CommandError::new(t!("workbench-run-unknown-test", test = test))
}

pub struct RerunTest;

impl Command for RerunTest {
    type Params = TestParams;
    type Output = RunStarted;
    const ID: &'static str = "run.tests.rerun";
    const TITLE: &'static str = "Rerun Test";
    const DESCRIPTION: &'static str = "Runs one test of a run (or, with package alone, that package's tests), with \
         the same settings; returns the new run's id.";

    fn run(&self, session: &mut Session, params: TestParams) -> Result<RunStarted, CommandError> {
        let runs = session.state::<Runs>().ok_or_else(|| unknown_run(params.run_id))?;
        let run = runs.run(params.run_id).ok_or_else(|| unknown_run(params.run_id))?;
        let tests = run.output.read(|output| chosen_tests(output.tree().root(), &params));
        if tests.is_empty() {
            return Err(unknown_test(&params));
        }
        rerun_tests(session, params.run_id, &tests)
    }
}

pub struct GetTestSource;

impl Command for GetTestSource {
    type Params = TestParams;
    type Output = TestSource;
    const ID: &'static str = "run.tests.source";
    const TITLE: &'static str = "Test Source";
    const DESCRIPTION: &'static str = "Where a test of a run is written: its file and one-based line; a subtest gives \
         its test function.";

    fn run(&self, session: &mut Session, params: TestParams) -> Result<TestSource, CommandError> {
        let root = project_root(session)?;
        let (provider, configuration, id) = {
            let runs = session.state::<Runs>().ok_or_else(|| unknown_run(params.run_id))?;
            let run = runs.run(params.run_id).ok_or_else(|| unknown_run(params.run_id))?;
            let id = run.output.read(|output| named_test(output.tree().root(), &params));
            let id = id.ok_or_else(|| unknown_test(&params))?;
            (provider_for(&runs.providers, &run.configuration.kind).cloned(), run.configuration.clone(), id)
        };
        let context = run_context(session, &root);
        let hidden = crate::hidden_files::for_caller(session);
        provider
            .and_then(|provider| provider.test_source(&configuration, &id, &context))
            .filter(|source| {
                hidden
                    .as_ref()
                    .is_none_or(|patterns| !crate::hidden_files::is_hidden(patterns, Some(&root), &source.path))
            })
            .ok_or_else(|| CommandError::new(t!("workbench-run-no-test-source", test = id.test.as_str())))
    }
}

/// The parameters of `run.configuration.resolve`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResolveParams {
    /// A saved configuration.
    pub configuration_id: Option<String>,
    /// A configuration as given, e.g. one being edited; used when `configurationId` is absent.
    pub configuration: Option<Configuration>,
    /// The text `$Prompt$` stands for.
    pub prompt: Option<String>,
    /// The file `$FilePrompt$` stands for.
    pub file_prompt: Option<PathBuf>,
}

/// What a configuration launches with now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    /// The program's arguments, one per item, as the program gets them.
    pub program_arguments: Vec<String>,
    /// The tool's arguments, one per item.
    pub tool_arguments: Vec<String>,
    /// The folder it runs in.
    pub working_directory: PathBuf,
    /// The variables added for it: the `envFiles` in order, then its own, with `$NAME$` expanded.
    pub env: Vec<(String, String)>,
}

pub struct ResolveConfiguration;

impl Command for ResolveConfiguration {
    type Params = ResolveParams;
    type Output = Resolved;
    const ID: &'static str = "run.configuration.resolve";
    const TITLE: &'static str = "Resolve Run Configuration";
    const DESCRIPTION: &'static str = "What a configuration would launch with now, without starting it: its .env \
         files read in order with its own variables on top, `$NAME$` references to the IDE's variables expanded, and \
         the macros of its arguments and working directory expanded from the editor, the project, the clipboard and \
         the SDKs. Fails as run.process.start would: a missing .env file, a macro without a value, or promptNeeded.";

    fn run(&self, session: &mut Session, params: ResolveParams) -> Result<Resolved, CommandError> {
        let root = project_root(session)?;
        let configuration = match (&params.configuration_id, params.configuration) {
            (Some(id), _) => read_file(session, &root)?.get(id).cloned().ok_or_else(|| unknown_configuration(id))?,
            (None, Some(configuration)) => configuration,
            (None, None) => return Err(CommandError::new(t!("workbench-run-no-configuration"))),
        };
        let prompts = Prompts { prompt: params.prompt, file_prompt: params.file_prompt };
        let launched = launched(session, &root, &configuration, &prompts)?;
        let common = launched.common;
        Ok(Resolved {
            program_arguments: zen_run::args::split(&common.program_arguments),
            tool_arguments: zen_run::args::split(&common.tool_arguments),
            working_directory: common.working_directory_in(&root),
            env: common.env,
        })
    }
}

/// The variables every run inherits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemEnvironment {
    /// By name: the IDE's own environment with the SDK variables the IDE sets for runs on top.
    pub variables: Vec<(String, String)>,
}

pub struct ListSystemEnvironment;

impl Command for ListSystemEnvironment {
    type Params = NoParams;
    type Output = SystemEnvironment;
    const ID: &'static str = "run.environment.system";
    const TITLE: &'static str = "System Environment Variables";
    const DESCRIPTION: &'static str = "The environment variables every run inherits, by name, as the Environment \
         Variables table of the Run/Debug Configurations dialog shows them read-only below a configuration's own.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<SystemEnvironment, CommandError> {
        let mut variables: std::collections::BTreeMap<String, String> = std::env::vars().collect();
        if let Some(runs) = session.state::<Runs>() {
            variables.extend(runs.env.env.iter().cloned());
        }
        Ok(SystemEnvironment { variables: variables.into_iter().collect() })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ResolveConfiguration)?;
    registry.register(ListSystemEnvironment)?;
    registry.register(ListConfigurations)?;
    registry.register(CreateConfiguration)?;
    registry.register(UpdateConfiguration)?;
    registry.register(DeleteConfiguration)?;
    registry.register(SaveAdoptedConfiguration)?;
    registry.register(SelectConfiguration)?;
    registry.register(GetContext)?;
    registry.register(RunScratch)?;
    registry.register(ListTargets)?;
    registry.register(StartRun)?;
    registry.register(StopRun)?;
    registry.register(ListRuns)?;
    registry.register(RerunRun)?;
    registry.register(GetOutput)?;
    registry.register(GetTests)?;
    registry.register(RerunFailed)?;
    registry.register(RerunTest)?;
    registry.register(GetTestSource)?;
    crate::before_launch::register(registry)?;
    crate::run_templates::register(registry)?;
    crate::run_services::register(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zen_run::Stream;
    use zen_run::fake::{FakeProvider, KIND};

    use super::*;

    #[cfg(unix)]
    fn debuggee_console(agent: zen_run::ClasspathAgent) -> Vec<String> {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let output = SharedOutput::new(RunOutput::new(Box::new(zen_run::PlainParser)), None);
        let scratch = tempfile::tempdir().unwrap();
        let driver = Driver::new(Vec::new(), Vec::new(), output.clone(), scratch.path().to_owned());
        let line = CommandLine {
            program: PathBuf::from("/bin/echo"),
            args: vec!["-agentlib:jdwp=x".into(), "Main".into()],
            display: "java -agentlib:jdwp=x Main".into(),
            ..CommandLine::default()
        };
        driver.set_debuggee(zen_run::Debuggee { line, ready: "never".into(), agent: Some(agent) });
        assert!(driver.start_debuggee());
        output.wait_end(Duration::from_secs(10));
        output.read(|output| output.console().lines().iter().map(|line| line.text.clone()).collect())
    }

    #[cfg(unix)]
    #[test]
    fn a_debuggee_loads_the_agent_jar_of_its_resolved_classpath_or_says_it_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let classpath = dir.path().join("classpath.args");
        std::fs::write(&classpath, "\"/nowhere/other.jar\"\n").unwrap();
        let agent = zen_run::ClasspathAgent {
            classpath: classpath.clone(),
            prefix: "probe-agent-".into(),
            option: "-javaagent:".into(),
            after: "-agentlib:jdwp".into(),
        };
        let without = debuggee_console(agent.clone());
        assert!(without[0].contains("No probe-agent-… jar"), "{without:?}");
        assert_eq!(without[1], "java -agentlib:jdwp=x Main", "it starts without it");
        let jar = dir.path().join("probe-agent-1.jar");
        std::fs::write(&jar, "jar").unwrap();
        std::fs::write(&classpath, format!("\"{}\"\n", jar.display())).unwrap();
        let with = debuggee_console(agent);
        let shown = format!("java -agentlib:jdwp=x -javaagent:{} Main", jar.display());
        assert_eq!(with[0], shown, "the agent is in the command line the console shows");
        assert!(with.iter().any(|line| line.contains(&format!("-javaagent:{}", jar.display()))), "{with:?}");
    }

    const IDS: [&str; 29] = [
        "file.scratch.run",
        "run.template.list",
        "run.template.update",
        "run.template.reset",
        "run.configuration.new",
        "run.services.list",
        "run.services.add",
        "run.services.remove",
        "run.services.hide",
        "run.services.restore",
        "run.configuration.resolve",
        "run.environment.system",
        "run.configuration.list",
        "run.configuration.create",
        "run.configuration.update",
        "run.configuration.delete",
        "run.configuration.saveAdopted",
        "run.configuration.select",
        "run.context.get",
        "run.targets.list",
        "run.process.start",
        "run.process.stop",
        "run.process.list",
        "run.process.rerun",
        "run.output.get",
        "run.tests.get",
        "run.tests.rerunFailed",
        "run.tests.rerun",
        "run.tests.source",
    ];

    fn setup() -> (Registry, Session, tempfile::TempDir, FakeProvider) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        let fake = FakeProvider::new();
        session.state_mut::<Runs>().set_providers(vec![Arc::new(fake.clone())], RunEnv::default());
        (registry, session, dir, fake)
    }

    fn configuration(name: &str) -> Value {
        serde_json::to_value(FakeProvider::configuration(name)).unwrap()
    }

    #[test]
    fn run_contexts_carry_the_debugger() {
        let (_registry, mut session, dir, _fake) = setup();
        assert_eq!(run_context(&mut session, dir.path()).debugger, PathBuf::new());
        session.state_mut::<Runs>().set_debugger(PathBuf::from("/go/bin/dlv"));
        assert_eq!(session.state::<Runs>().map(|runs| runs.debugger().to_owned()), Some(PathBuf::from("/go/bin/dlv")));
        assert_eq!(run_context(&mut session, dir.path()).debugger, PathBuf::from("/go/bin/dlv"));
    }

    #[test]
    fn a_run_gets_the_project_s_environment_after_the_toolchain_s() {
        struct ProjectTools;
        impl zen_lsp::ServerTools for ProjectTools {
            fn project_environment(&self, root: &Path) -> Vec<(String, String)> {
                vec![("GOFLAGS".to_owned(), format!("-tags={}", root.file_name().unwrap().to_string_lossy()))]
            }
        }
        let (_registry, mut session, dir, _fake) = setup();
        let toolchain = vec![("GOFLAGS".to_owned(), "-mod=mod".to_owned())];
        session.state_mut::<Runs>().set_env(RunEnv { tool: PathBuf::from("/go/bin/go"), env: toolchain });
        session.set_state(crate::language::servers::LanguageServers::load(None, Arc::new(ProjectTools)));
        let env = run_context(&mut session, dir.path()).env.env;
        let tags = format!("-tags={}", dir.path().file_name().unwrap().to_string_lossy());
        assert_eq!(env, [("GOFLAGS".to_owned(), "-mod=mod".to_owned()), ("GOFLAGS".to_owned(), tags)]);
    }

    #[test]
    fn every_run_command_is_registered_with_three_segments() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        for id in IDS {
            assert_eq!(id.split('.').count(), 3, "{id}");
            assert!(registry.commands().any(|info| info.id.as_str() == id), "{id} is not registered");
        }
    }

    #[test]
    fn configurations_are_created_selected_changed_and_removed_in_the_file() {
        let (registry, mut session, dir, _fake) = setup();
        let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["configurations"], json!([]));
        assert_eq!(listed["kinds"][0]["id"], KIND);
        assert_eq!(listed["kinds"][0]["provider"], "fake");
        let params = json!({ "configuration": configuration("app"), "select": true });
        let created = registry.dispatch(&mut session, "run.configuration.create", params).unwrap();
        assert_eq!(created["id"], "fake.run-1");
        let second = registry
            .dispatch(&mut session, "run.configuration.create", json!({ "configuration": configuration("b") }))
            .unwrap();
        assert_eq!(second["id"], "fake.run-2");
        let file = RunConfigurations::read(dir.path()).unwrap();
        assert_eq!(file.selected, None, "the selection is kept with the window");
        assert_eq!(session.state::<Runs>().unwrap().selected(), Some("fake.run-1"));
        assert_eq!(file.configurations.len(), 2);
        let mut changed = created.clone();
        changed["name"] = json!("renamed");
        registry.dispatch(&mut session, "run.configuration.update", json!({ "configuration": changed })).unwrap();
        assert_eq!(RunConfigurations::read(dir.path()).unwrap().configurations[0].name, "renamed");
        let mut missing = created.clone();
        missing["id"] = json!("nope");
        assert!(
            registry.dispatch(&mut session, "run.configuration.update", json!({ "configuration": missing })).is_err()
        );
        registry.dispatch(&mut session, "run.configuration.select", json!({ "id": "fake.run-2" })).unwrap();
        assert!(registry.dispatch(&mut session, "run.configuration.select", json!({ "id": "nope" })).is_err());
        registry.dispatch(&mut session, "run.configuration.delete", json!({ "id": "fake.run-2" })).unwrap();
        let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["selected"], "fake.run-1");
        assert_eq!(listed["configurations"].as_array().unwrap().len(), 1);
        assert!(registry.dispatch(&mut session, "run.configuration.delete", json!({ "id": "fake.run-2" })).is_err());
    }

    struct Foreign;

    impl zen_project::settings::SettingsProvider for Foreign {
        fn id(&self) -> &str {
            "foreign"
        }

        fn watches(&self, _: &Path) -> bool {
            false
        }

        fn load(
            &self,
            root: &Path,
            _: &dyn zen_project::settings::ReadFiles,
        ) -> Option<zen_project::settings::ProjectSettings> {
            let foreign = |name: &str, type_id: &str| zen_run::ForeignRunConfiguration {
                source_file: root.join(".run/Serve.run.xml"),
                name: name.to_owned(),
                type_id: type_id.to_owned(),
                options: [
                    ("target".to_owned(), "serve".to_owned()),
                    ("allowRunningInParallel".to_owned(), "true".to_owned()),
                ]
                .into_iter()
                .collect(),
                shared: true,
                ..Default::default()
            };
            Some(zen_project::settings::ProjectSettings {
                run_configurations: vec![
                    foreign("Serve", zen_run::fake::ADOPTED_TYPE),
                    foreign("Serve", zen_run::fake::ADOPTED_TYPE),
                    foreign("Remote", "SomeOtherType"),
                ],
                ..Default::default()
            })
        }
    }

    #[test]
    fn adopted_configurations_run_but_are_changed_only_as_zen_copies() {
        let (registry, mut session, dir, _fake) = setup();
        std::fs::create_dir_all(dir.path().join(".run")).unwrap();
        std::fs::write(dir.path().join(".run/Serve.run.xml"), "<component/>").unwrap();
        session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(Foreign)]);
        let id = "jb:.run/Serve.run.xml#Serve";
        let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
        let configurations = listed["configurations"].as_array().unwrap();
        assert_eq!(configurations.len(), 1, "a duplicate and a type no provider adopts are left out");
        assert_eq!(configurations[0]["id"], id);
        assert_eq!(configurations[0]["adoptedFrom"], json!(dir.path().join(".run/Serve.run.xml")));
        assert_eq!(configurations[0]["options"]["target"], "serve");
        assert_eq!(configurations[0]["common"]["allowMultipleInstances"], true, "Allow multiple instances is adopted");

        registry.dispatch(&mut session, "run.configuration.select", json!({ "id": id })).unwrap();
        let started = registry.dispatch(&mut session, "run.process.start", json!({ "configurationId": id })).unwrap();
        assert_eq!(started["runId"], 1);
        let changed = json!({ "configuration": { "id": id, "kind": KIND, "name": "Serve", "options": {} } });
        let refused = registry.dispatch(&mut session, "run.configuration.update", changed).unwrap_err().to_string();
        assert!(
            refused.contains(&t!(
                "workbench-run-adopted-read-only",
                name = "Serve",
                file = display(&dir.path().join(".run/Serve.run.xml"))
            )),
            "{refused}"
        );
        assert!(registry.dispatch(&mut session, "run.configuration.delete", json!({ "id": id })).is_err());
        assert!(!dir.path().join(zen_project::run::FILE).exists());

        let saved = registry.dispatch(&mut session, "run.configuration.saveAdopted", json!({ "id": id })).unwrap();
        assert_eq!((saved["id"].as_str(), saved["name"].as_str()), (Some("fake.run-1"), Some("Serve")));
        assert!(saved.get("adoptedFrom").is_none());
        let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["configurations"].as_array().unwrap().len(), 1, "the copy takes the adopted one's place");
        assert_eq!(listed["selected"], "fake.run-1");
        let file = std::fs::read_to_string(dir.path().join(zen_project::run::FILE)).unwrap();
        assert!(file.contains("\"Serve\"") && !file.contains("adoptedFrom") && !file.contains("jb:"), "{file}");
        assert!(registry.dispatch(&mut session, "run.configuration.saveAdopted", json!({ "id": id })).is_err());
        let copy = json!({ "configuration": { "id": "fake.run-1", "kind": KIND, "name": "Serve", "options": { "target": "x" } } });
        registry.dispatch(&mut session, "run.configuration.update", copy).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join(".run/Serve.run.xml")).unwrap(), "<component/>");
    }

    #[test]
    fn a_broken_file_is_reported() {
        let (registry, mut session, dir, _fake) = setup();
        std::fs::create_dir_all(dir.path().join(".zen")).unwrap();
        std::fs::write(dir.path().join(".zen/runConfigurations.json"), "{ broken").unwrap();
        let error = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap_err().to_string();
        let file = display(&dir.path().join(".zen/runConfigurations.json"));
        assert!(error.contains(&t!("workbench-run-file-broken", path = file, error = "")), "{error}");
    }

    #[test]
    fn tests_are_answered_as_a_summary_the_failures_or_one_test_and_a_start_can_wait_for_the_end() {
        let (registry, mut session, dir, fake) = setup();
        std::fs::create_dir_all(dir.path().join("store")).unwrap();
        std::fs::write(dir.path().join("store/cart_test.go"), "package store\n").unwrap();
        let mut script = vec![(Stream::Stdout, "@start app TestA".to_owned())];
        script.push((Stream::Stdout, "=== RUN TestA".to_owned()));
        script.push((Stream::Stdout, "    /usr/lib/go/src/testing/testing.go:1: called from".to_owned()));
        script.push((Stream::Stdout, "    store/cart_test.go:7:3: got 1, want 2".to_owned()));
        script.push((Stream::Stdout, "    more detail".to_owned()));
        script.push((Stream::Stdout, "@fail app TestA 4".to_owned()));
        script.push((Stream::Stdout, "@start app TestB".to_owned()));
        script.push((Stream::Stdout, "@start app TestB/case".to_owned()));
        script.push((Stream::Stdout, "boom".to_owned()));
        script.push((Stream::Stdout, "@fail app TestB/case 1".to_owned()));
        script.push((Stream::Stdout, "@fail app TestB 2".to_owned()));
        for index in 0..600 {
            script.push((Stream::Stdout, format!("@start app TestPass{index}")));
            script.push((Stream::Stdout, format!("output of a passing test {index}")));
            script.push((Stream::Stdout, format!("@pass app TestPass{index} 1")));
        }
        script.push((Stream::Stdout, "@fail app - 9".to_owned()));
        let lines: Vec<(Stream, &str)> = script.iter().map(|(stream, text)| (*stream, text.as_str())).collect();
        fake.set_script(&lines, 1);

        let params = json!({ "configuration": configuration("tests"), "waitMs": 10_000 });
        let started = registry.dispatch(&mut session, "run.process.start", params).unwrap();
        let run_id = started["runId"].as_u64().unwrap();
        let summary = &started["summary"];
        assert_eq!((summary["state"].as_str(), summary["done"].as_bool()), (Some("finished"), Some(true)), "{started}");
        assert_eq!((summary["exitCode"].as_i64(), summary["counts"]["failed"].as_u64()), (Some(1), Some(3)));
        assert_eq!(summary["counts"]["passed"], 600);

        let tree = registry.dispatch(&mut session, "run.tests.get", json!({ "runId": run_id })).unwrap();
        assert!(tree.get("summary").is_none() && tree["tree"]["children"].is_array(), "the tree, as before");
        let summary = registry.dispatch(&mut session, "run.tests.get", json!({ "runId": run_id, "summary": true }));
        let summary = summary.unwrap();
        assert!(summary.get("tree").is_none() && summary.get("failed").is_none(), "{summary}");
        assert!(summary.to_string().len() < 300, "{} bytes: {summary}", summary.to_string().len());
        assert!(
            tree.to_string().len() > 100 * summary.to_string().len(),
            "the tree is {} bytes",
            tree.to_string().len()
        );

        let failed = registry.dispatch(&mut session, "run.tests.get", json!({ "runId": run_id, "failedOnly": true }));
        let failed = failed.unwrap();
        assert_eq!(failed["summary"]["counts"]["failed"], 3);
        assert_eq!(
            failed["failed"],
            json!([
                {
                    "package": "app", "test": "TestA", "durationMs": 4,
                    "file": "store/cart_test.go", "line": 7, "column": 3,
                    "message": "store/cart_test.go:7:3: got 1, want 2\nmore detail"
                },
                { "package": "app", "test": "TestB/case", "durationMs": 1, "message": "boom" }
            ]),
            "the tests that failed themselves, at the first place of the project their output names"
        );
        assert!(failed.to_string().len() < 1000, "{failed}");

        let one =
            registry.dispatch(&mut session, "run.tests.get", json!({ "runId": run_id, "test": "TestA" })).unwrap();
        assert_eq!(one["output"]["status"], "failed");
        assert_eq!(one["output"]["output"][0], "=== RUN TestA");
        assert_eq!(one["output"]["output"].as_array().unwrap().len(), 4);
        let params = json!({ "runId": run_id, "test": "TestA", "package": "other" });
        assert!(registry.dispatch(&mut session, "run.tests.get", params).is_err(), "no such test in that package");
    }

    #[test]
    fn a_fake_run_fills_the_console_and_the_test_tree() {
        let (registry, mut session, _dir, fake) = setup();
        fake.set_script(
            &[
                (Stream::Stdout, "@start app TestA"),
                (Stream::Stdout, "\u{1b}[31mmain_test.go:7: boom\u{1b}[0m"),
                (Stream::Stdout, "@fail app TestA 4"),
                (Stream::Stdout, "@start app TestB"),
                (Stream::Stdout, "@pass app TestB 1"),
                (Stream::Stderr, "@fail app - 9"),
            ],
            1,
        );
        let params = json!({ "configuration": configuration("tests") });
        let started = registry.dispatch(&mut session, "run.process.start", params).unwrap();
        let run_id = started["runId"].as_u64().unwrap();
        let output =
            registry.dispatch(&mut session, "run.output.get", json!({ "runId": run_id, "waitMs": 1000 })).unwrap();
        assert_eq!(output["state"], "finished");
        assert_eq!(output["exitCode"], 1);
        let lines = output["lines"].as_array().unwrap();
        assert_eq!(lines[0], json!({ "stream": "system", "text": "fake tests", "links": [] }));
        assert_eq!(lines[1]["text"], "main_test.go:7: boom");
        assert_eq!(lines[1]["links"][0]["file"], "main_test.go");
        assert_eq!(lines[1]["links"][0]["line"], 7);
        assert_eq!(lines.last().unwrap()["stream"], "system");
        assert_eq!(output["next"], lines.len());
        let rest = registry
            .dispatch(&mut session, "run.output.get", json!({ "runId": run_id, "from": lines.len(), "waitMs": 0 }))
            .unwrap();
        assert_eq!(rest["lines"], json!([]));
        let tests = registry.dispatch(&mut session, "run.tests.get", json!({ "runId": run_id })).unwrap();
        assert_eq!(tests["done"], true);
        assert_eq!(tests["counts"]["failed"], 1);
        assert_eq!(tests["counts"]["passed"], 1);
        assert_eq!(tests["tree"]["status"], "failed");
        assert_eq!(tests["tree"]["children"][0]["children"][0]["lines"], json!([{ "start": 1, "end": 2 }]));
        let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
        assert_eq!(list["runs"][0]["runId"], run_id);
        assert_eq!(list["runs"][0]["state"], "finished");
        let waited = json!({ "runId": run_id, "waitMs": 60000 });
        let failed = registry.dispatch(&mut session, "run.tests.rerunFailed", waited.clone()).unwrap();
        assert_eq!(failed["summary"]["state"], "finished", "it waits and answers the summary: {failed}");
        let rerun = registry.dispatch(&mut session, "run.process.rerun", waited).unwrap();
        assert_eq!(rerun["summary"]["state"], "finished", "{rerun}");
        assert_ne!(failed["runId"], rerun["runId"]);
        assert_eq!(fake.replays(), 3);
        let one = json!({ "runId": run_id, "test": "TestB", "package": "app" });
        let single = registry.dispatch(&mut session, "run.tests.rerun", one.clone()).unwrap();
        assert_ne!(single["runId"], rerun["runId"]);
        assert_eq!(fake.replays(), 4);
        registry.dispatch(&mut session, "run.tests.rerun", json!({ "runId": run_id, "package": "app" })).unwrap();
        assert_eq!(fake.replays(), 5, "a package alone runs its tests");
        let missing = json!({ "runId": run_id, "test": "TestGone" });
        assert!(registry.dispatch(&mut session, "run.tests.rerun", missing.clone()).is_err());
        assert!(registry.dispatch(&mut session, "run.tests.source", missing).is_err());
        let source = registry.dispatch(&mut session, "run.tests.source", one).unwrap();
        assert_eq!(
            source["path"].as_str(),
            session.state::<Workbench>().unwrap().project().unwrap().join("main_test.go").to_str()
        );
        assert_eq!(source["line"], 2);
        let stopped = registry.dispatch(&mut session, "run.process.stop", json!({})).unwrap();
        assert_eq!(stopped, json!({ "stopped": null }));
        assert!(registry.dispatch(&mut session, "run.output.get", json!({ "runId": 99 })).is_err());
    }

    #[test]
    fn running_the_selected_configuration_and_a_context() {
        let (registry, mut session, dir, fake) = setup();
        assert!(registry.dispatch(&mut session, "run.process.start", json!({})).is_err(), "nothing to run");
        let main = dir.path().join("main.fake");
        std::fs::write(&main, "run me").unwrap();
        fake.set_context(Some(FakeProvider::configuration("context")));
        let context = registry.dispatch(&mut session, "run.context.get", json!({ "path": main, "offset": 2 })).unwrap();
        assert_eq!(context["configuration"]["name"], "context");
        assert_eq!(context["existing"], Value::Null);
        let started =
            registry.dispatch(&mut session, "run.process.start", json!({ "context": { "path": main } })).unwrap();
        assert_eq!(started["runId"], 1);
        assert!(!dir.path().join(zen_project::run::FILE).exists(), "temporary configurations stay out of the file");
        let runs = session.state::<Runs>().unwrap();
        let file = RunConfigurations { configurations: runs.temporary().to_vec(), ..RunConfigurations::default() };
        assert_eq!(file.configurations.len(), 1);
        assert!(file.configurations[0].temporary);
        assert_eq!(runs.selected(), Some(file.configurations[0].id.as_str()));
        registry.dispatch(&mut session, "run.process.start", json!({ "context": { "path": main } })).unwrap();
        assert_eq!(session.state::<Runs>().unwrap().temporary().len(), 1, "the kept one is reused");
        let selected = registry.dispatch(&mut session, "run.process.start", json!({})).unwrap();
        assert_eq!(selected["runId"], 3);
        let by_id = json!({ "configurationId": file.configurations[0].id });
        assert_eq!(registry.dispatch(&mut session, "run.process.start", by_id).unwrap()["runId"], 4);
        fake.set_context(None);
        assert!(registry.dispatch(&mut session, "run.process.start", json!({ "context": { "path": main } })).is_err());
        let failing = json!({ "configuration": { "kind": KIND, "name": "x", "options": { "fail": "no tool" } } });
        let error = registry.dispatch(&mut session, "run.process.start", failing).unwrap_err().to_string();
        assert!(error.ends_with(&t!("workbench-run-cannot-start", name = "x", error = "no tool")), "{error}");
        let unknown = json!({ "configuration": { "kind": "other.kind", "name": "x" } });
        let error = registry.dispatch(&mut session, "run.process.start", unknown).unwrap_err().to_string();
        let known = format!("{KIND}, {}", crate::compound::KIND);
        let kinds = t!("workbench-run-unknown-kind-of", kind = "other.kind", kinds = known);
        assert!(error.ends_with(&kinds), "the error names the kinds there are: {error}");
        let misspelt = json!({ "configuration": { "kind": KIND, "name": "x", "options": { "tagret": "one" } } });
        let error = registry.dispatch(&mut session, "run.process.start", misspelt).unwrap_err().to_string();
        let keys = t!(
            "workbench-run-unknown-options",
            kind = KIND,
            unknown = "tagret",
            keys = "target, fail, noDebug, noDebugger, views"
        );
        assert!(error.ends_with(&keys), "{error}");
    }

    #[test]
    fn a_place_is_a_line_too_and_paths_are_the_projects() {
        let text = "fn main() {}\n\n#[test]\n    fn adds() {}\n";
        assert_eq!(line_start(text, 0), 0);
        assert_eq!(line_start(text, 3), 26, "the first character that is not a space");
        assert_eq!(line_start(text, 9), text.chars().count(), "past the end");
        let (registry, mut session, dir, fake) = setup();
        std::fs::write(dir.path().join("main.fake"), "run me").unwrap();
        fake.set_context(Some(FakeProvider::configuration("context")));
        let context =
            registry.dispatch(&mut session, "run.context.get", json!({ "path": "main.fake", "line": 1 })).unwrap();
        assert_eq!(context["configuration"]["name"], "context", "{context}");
        let listed = registry.dispatch(&mut session, "run.targets.list", json!({ "path": "main.fake" })).unwrap();
        assert_eq!(listed["targets"], json!([]), "a relative path is read from the project");
    }

    #[test]
    fn a_scratch_file_runs_on_its_own_through_its_provider() {
        let (registry, mut session, dir, fake) = setup();
        let data = tempfile::tempdir().unwrap();
        session.state_mut::<crate::scratch::Scratches>().set_data_dir(data.path().to_owned());
        let scratches = data.path().join("scratches");
        std::fs::create_dir_all(&scratches).unwrap();
        let scratch = scratches.join("scratch.fake");
        std::fs::write(&scratch, "main\n").unwrap();
        let in_project = dir.path().join("main.fake");
        std::fs::write(&in_project, "main\n").unwrap();
        let target = |line, name: &str| RunTarget {
            line,
            kind: zen_run::TargetKind::Main,
            name: name.to_owned(),
            package: String::new(),
            configuration: FakeProvider::configuration(name),
        };
        fake.set_targets(vec![target(3, "project")]);
        fake.set_context(Some(FakeProvider::configuration("project")));

        let refused = registry.dispatch(&mut session, "file.scratch.run", json!({ "path": scratch }));
        assert!(refused.is_err(), "no provider runs it alone: {refused:?}");
        let listed = registry.dispatch(&mut session, "run.targets.list", json!({ "path": scratch })).unwrap();
        assert_eq!(listed["targets"], json!([]), "a scratch never gets the project's targets: {listed}");

        fake.set_single_file(vec![target(0, "scratch.fake")]);
        let listed =
            registry.dispatch(&mut session, "run.targets.list", json!({ "path": scratch, "full": true })).unwrap();
        assert_eq!(listed["targets"].as_array().map(Vec::len), Some(1), "{listed}");
        assert_eq!(listed["targets"][0]["line"], 1);
        assert_eq!(listed["targets"][0]["configuration"]["name"], "scratch.fake");
        let context = registry.dispatch(&mut session, "run.context.get", json!({ "path": scratch })).unwrap();
        assert_eq!(context["configuration"]["name"], "scratch.fake", "⌃⇧R runs the file: {context}");
        let project = registry.dispatch(&mut session, "run.targets.list", json!({ "path": in_project })).unwrap();
        assert_eq!(project["targets"][0]["name"], "project", "project files keep their targets: {project}");
        assert_eq!(scratch_run_name(&session, &scratch).as_deref(), Some("scratch.fake"));
        assert_eq!(scratch_run_name(&session, &in_project), None);

        let refused = registry.dispatch(&mut session, "file.scratch.run", json!({ "path": in_project }));
        assert!(refused.is_err(), "only scratch files: {refused:?}");
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["**/scratch.fake".to_owned()]);
        let hidden = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "file.scratch.run", json!({ "path": scratch }))
        });
        match hidden {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::hidden_files::REDACTED_CODE), "{error:?}")
            }
            other => panic!("an agent ran a hidden scratch: {other:?}"),
        }
        let through_context = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "run.process.start", json!({ "context": { "path": scratch } }))
        });
        assert!(through_context.is_err(), "nor through its context: {through_context:?}");
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[]);
        fake.set_script(&[(Stream::Stdout, "hello from the scratch")], 0);
        let started =
            registry.dispatch(&mut session, "file.scratch.run", json!({ "path": scratch, "waitMs": 10_000 })).unwrap();
        assert_eq!(started["summary"]["exitCode"], 0, "{started}");
        let output =
            registry.dispatch(&mut session, "run.output.get", json!({ "runId": started["runId"], "from": 0 })).unwrap();
        assert!(output.to_string().contains("hello from the scratch"), "{output}");
        let temporary = session.state::<Runs>().unwrap().temporary().to_vec();
        assert_eq!(temporary.iter().map(|kept| kept.name.as_str()).collect::<Vec<_>>(), ["scratch.fake"]);
    }

    #[test]
    fn targets_are_listed_by_line() {
        let (registry, mut session, dir, fake) = setup();
        let file = dir.path().join("a.fake");
        std::fs::write(&file, "x").unwrap();
        let target = |line, name: &str| RunTarget {
            line,
            kind: zen_run::TargetKind::Test,
            name: name.to_owned(),
            package: String::new(),
            configuration: FakeProvider::configuration(name),
        };
        fake.set_targets(vec![target(9, "TestLate"), target(2, "TestEarly")]);
        let listed = registry.dispatch(&mut session, "run.targets.list", json!({ "path": file })).unwrap();
        assert_eq!(listed["targets"][0]["name"], "TestEarly");
        assert_eq!(listed["targets"][0]["provider"], "fake");
        assert_eq!(listed["targets"][1]["line"], 10, "one-based");
        assert!(listed["targets"][1].get("configuration").is_none(), "compact without `full`: {listed}");
        let full = registry.dispatch(&mut session, "run.targets.list", json!({ "path": file, "full": true })).unwrap();
        assert_eq!(full["targets"][1]["configuration"]["name"], "TestLate", "{full}");
        assert_eq!(listed["targets"][1]["kind"], "test");
    }

    #[cfg(unix)]
    struct Shell;

    #[cfg(unix)]
    impl RunProvider for Shell {
        fn id(&self) -> &str {
            "shell"
        }

        fn kinds(&self) -> Vec<KindSpec> {
            vec![KindSpec {
                id: "shell.run".to_owned(),
                name_message: "shell-run-kind".to_owned(),
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

        fn command_lines(
            &self,
            configuration: &Configuration,
            context: &RunContext,
        ) -> Result<Vec<CommandLine>, String> {
            let program = configuration.options["program"].as_str().unwrap_or("/bin/sh");
            let steps = configuration.options["steps"].as_array().cloned().unwrap_or_default();
            Ok(steps
                .iter()
                .map(|step| CommandLine {
                    program: PathBuf::from(program),
                    args: vec![
                        "-c".to_owned(),
                        step.as_str().unwrap_or_default().replace("{scratch}", &context.scratch.to_string_lossy()),
                    ],
                    cwd: configuration.common.working_directory_in(&context.root),
                    env: configuration.common.env.clone(),
                    display: format!("sh {}", step.as_str().unwrap_or_default()),
                })
                .collect())
        }

        fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn zen_run::OutputParser> {
            Box::new(zen_run::PlainParser)
        }
    }

    #[cfg(unix)]
    fn shell_setup() -> (Registry, Session, tempfile::TempDir) {
        let (registry, mut session, dir, fake) = setup();
        let env =
            RunEnv { tool: PathBuf::from("/bin/sh"), env: vec![("ZEN_RUN_ENV".to_owned(), "from-env".to_owned())] };
        session.state_mut::<Runs>().set_providers(vec![Arc::new(fake), Arc::new(Shell)], env);
        (registry, session, dir)
    }

    #[cfg(unix)]
    fn shell(steps: &[&str]) -> Value {
        json!({ "configuration": { "kind": "shell.run", "name": "sh", "options": { "steps": steps } } })
    }

    #[cfg(unix)]
    fn ended(registry: &Registry, session: &mut Session, run_id: &Value) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let output = registry
                .dispatch(session, "run.output.get", json!({ "runId": run_id, "from": 0, "waitMs": 200 }))
                .unwrap();
            if output["state"] != "running" {
                return output;
            }
            assert!(Instant::now() < deadline, "the run does not end: {output}");
        }
    }

    #[cfg(unix)]
    fn texts(output: &Value) -> Vec<(String, String)> {
        output["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| (line["stream"].as_str().unwrap().to_owned(), line["text"].as_str().unwrap().to_owned()))
            .collect()
    }

    #[cfg(unix)]
    fn line(stream: &str, text: &str) -> (String, String) {
        (stream.to_owned(), text.to_owned())
    }

    #[cfg(unix)]
    #[test]
    fn a_process_runs_with_the_environment_and_its_folder_and_reports_its_exit_code() {
        let (registry, mut session, dir) = shell_setup();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let script = "echo \"$ZEN_RUN_ENV $ZEN_RUN_LINE\"; pwd -P; sleep 0.1; echo oops >&2; exit 3";
        let mut params = shell(&[script]);
        params["configuration"]["common"] = json!({ "workingDirectory": "sub", "env": [["ZEN_RUN_LINE", "line"]] });
        let started = registry.dispatch(&mut session, "run.process.start", params).unwrap();
        let output = ended(&registry, &mut session, &started["runId"]);
        let folder = dir.path().join("sub").canonicalize().unwrap();
        assert_eq!(
            texts(&output),
            vec![
                line("system", &format!("sh {script}")),
                line("stdout", "from-env line"),
                line("stdout", folder.to_str().unwrap()),
                line("stderr", "oops"),
                line("system", &t!("workbench-run-process-finished", code = 3)),
            ]
        );
        assert_eq!(output["state"], "finished");
        assert_eq!(output["exitCode"], 3);
        let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
        assert_eq!(list["runs"][0]["exitCode"], 3);
    }

    #[cfg(unix)]
    #[test]
    fn env_files_are_read_in_order_and_the_own_variables_and_parent_references_go_on_top() {
        let (registry, mut session, dir) = shell_setup();
        std::fs::write(dir.path().join(".env"), "A=from-file\nB=first\n# note\n").unwrap();
        std::fs::create_dir(dir.path().join("conf")).unwrap();
        std::fs::write(dir.path().join("conf/local.env"), "export B='second'\nC=$ZEN_RUN_ENV$-c\n").unwrap();
        let mut params = shell(&["echo \"$A $B $C $D\""]);
        params["configuration"]["common"] = json!({
            "envFiles": [".env", dir.path().join("conf/local.env")],
            "env": [["A", "own"], ["D", "$ZEN_RUN_ENV$:$zen_run_env$"]],
        });
        let started = registry.dispatch(&mut session, "run.process.start", params.clone()).unwrap();
        let output = ended(&registry, &mut session, &started["runId"]);
        assert_eq!(texts(&output)[1], line("stdout", "own second from-env-c from-env:$zen_run_env$"));
        assert!(
            !texts(&output)[0].1.contains("from-file") && !texts(&output)[0].1.contains("second"),
            "no value in the command line shown: {:?}",
            texts(&output)[0]
        );
        let resolved = registry
            .dispatch(&mut session, "run.configuration.resolve", json!({ "configuration": params["configuration"] }))
            .unwrap();
        assert_eq!(
            resolved["env"],
            json!([["A", "own"], ["B", "second"], ["C", "from-env-c"], ["D", "from-env:$zen_run_env$"]])
        );
        params["configuration"]["common"]["envFiles"] = json!([".env", "missing.env"]);
        let error = registry.dispatch(&mut session, "run.process.start", params.clone()).unwrap_err().to_string();
        assert!(error.contains(&display(&dir.path().join("missing.env"))), "the missing file is named: {error}");
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
        let mut hidden = params.clone();
        hidden["configuration"]["common"]["envFiles"] = json!([".env"]);
        let refused = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "run.configuration.resolve", json!({ "configuration": hidden["configuration"] }))
        })
        .unwrap_err()
        .to_string();
        assert!(
            refused.contains("hidden from agents") && !refused.contains("from-file"),
            "an agent reads no hidden .env: {refused}"
        );
        let started = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "run.process.start", hidden.clone())
        });
        assert!(started.is_err(), "nor runs with it");
        registry
            .dispatch(&mut session, "run.configuration.resolve", json!({ "configuration": hidden["configuration"] }))
            .unwrap();
        let system = registry.dispatch(&mut session, "run.environment.system", json!({})).unwrap();
        let names: Vec<&str> =
            system["variables"].as_array().unwrap().iter().filter_map(|pair| pair[0].as_str()).collect();
        assert!(names.contains(&"ZEN_RUN_ENV") && names.is_sorted(), "{names:?}");
    }

    #[test]
    fn arguments_and_the_working_directory_expand_macros_and_a_prompt_must_be_given() {
        let (registry, mut session, dir, _fake) = setup();
        let mut configuration = serde_json::to_value(FakeProvider::configuration("app")).unwrap();
        configuration["common"] = json!({
            "programArguments": "--root $ProjectFileDir$ --name=$ProjectName$ $Prompt$",
            "toolArguments": "-C $ProjectFileDir$",
            "workingDirectory": "$ProjectFileDir$/sub",
        });
        let error = registry
            .dispatch(&mut session, "run.process.start", json!({ "configuration": configuration.clone() }))
            .unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = error else { panic!("{error}") };
        assert_eq!(error.code.as_deref(), Some(PROMPT_NEEDED));
        let resolved = registry
            .dispatch(
                &mut session,
                "run.configuration.resolve",
                json!({ "configuration": configuration.clone(), "prompt": "two words" }),
            )
            .unwrap();
        let root = dir.path().display().to_string();
        let name = dir.path().file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            resolved["programArguments"],
            json!(["--root", root, format!("--name={name}"), "two words"]),
            "every value stays one argument"
        );
        assert_eq!(resolved["toolArguments"], json!(["-C", root]));
        assert_eq!(resolved["workingDirectory"], json!(dir.path().join("sub")));
        configuration["common"] = json!({ "programArguments": "$FilePath$" });
        let error = registry
            .dispatch(&mut session, "run.configuration.resolve", json!({ "configuration": configuration }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("$FilePath$"), "{error}");
        crate::file_clipboard::set_clipboard_text(&mut session, "s3cret-token");
        configuration["common"] = json!({ "programArguments": "$ClipboardContent$" });
        let resolve = json!({ "configuration": configuration });
        let person = registry.dispatch(&mut session, "run.configuration.resolve", resolve.clone()).unwrap();
        assert_eq!(person["programArguments"], json!(["s3cret-token"]));
        let agent = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "run.configuration.resolve", resolve.clone())
        });
        assert!(agent.is_err_and(|error| !error.to_string().contains("s3cret")), "agents get no clipboard");
    }

    #[cfg(unix)]
    #[test]
    fn a_running_configuration_is_stopped_and_rerun_unless_it_allows_multiple_instances() {
        let (registry, mut session, _dir) = shell_setup();
        let mut saved = shell(&["sleep 30"])["configuration"].clone();
        saved["id"] = json!("server");
        registry.dispatch(&mut session, "run.configuration.create", json!({ "configuration": saved.clone() })).unwrap();
        let by_id = json!({ "configurationId": "server" });
        let first = registry.dispatch(&mut session, "run.process.start", by_id.clone()).unwrap()["runId"].clone();
        let asked = json!({ "configurationId": "server", "confirmRerun": true });
        let refused = registry.dispatch(&mut session, "run.process.start", asked).unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = refused else { panic!("{refused}") };
        assert_eq!(error.code.as_deref(), Some(RERUN_CONFIRMATION));
        assert_eq!(session.state::<Runs>().unwrap().instances("server").len(), 1, "asking starts nothing");
        let second = registry.dispatch(&mut session, "run.process.start", by_id.clone()).unwrap()["runId"].clone();
        assert_eq!(ended(&registry, &mut session, &first)["state"], "stopped", "over MCP it stops without asking");
        let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
        assert_eq!(list["runs"][1]["replaces"], first, "the rerun takes the stopped run's tab");
        assert_eq!(list["runs"][1]["name"], "sh");

        saved["common"] = json!({ "allowMultipleInstances": true });
        registry.dispatch(&mut session, "run.configuration.update", json!({ "configuration": saved })).unwrap();
        let third = registry
            .dispatch(&mut session, "run.process.start", json!({ "configurationId": "server", "confirmRerun": true }))
            .unwrap()["runId"]
            .clone();
        let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
        let run = |id: &Value| list["runs"].as_array().unwrap().iter().find(|run| run["runId"] == *id).unwrap().clone();
        assert_eq!(run(&second)["state"], "running", "another instance leaves the first running");
        assert_eq!((run(&third)["name"].clone(), run(&third)["instance"].clone()), (json!("sh (2)"), json!(2)));
        assert_eq!(run(&third).get("replaces"), None);
        let rerun =
            registry.dispatch(&mut session, "run.process.rerun", json!({ "runId": third })).unwrap()["runId"].clone();
        let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
        let last = list["runs"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(
            (last["runId"].clone(), last["replaces"].clone(), last["name"].clone()),
            (rerun, third.clone(), json!("sh (2)"))
        );
        assert_eq!(ended(&registry, &mut session, &third)["state"], "stopped", "a tab's rerun stops its own run");
        registry.dispatch(&mut session, "run.process.stop", json!({ "runId": second })).unwrap();
        registry.dispatch(&mut session, "run.process.stop", json!({ "runId": last["runId"] })).unwrap();
    }

    #[test]
    fn the_temporary_limit_comes_from_the_setting() {
        let (registry, mut session, dir, fake) = setup();
        session.settings_mut().set(TEMPORARY_LIMIT, 2).unwrap();
        for name in ["a", "b", "c"] {
            let file = dir.path().join(format!("{name}.fake"));
            std::fs::write(&file, name).unwrap();
            fake.set_context(Some(FakeProvider::configuration(name)));
            registry.dispatch(&mut session, "run.process.start", json!({ "context": { "path": file } })).unwrap();
        }
        let names: Vec<String> =
            session.state::<Runs>().unwrap().temporary().iter().map(|kept| kept.name.clone()).collect();
        assert_eq!(names, ["b", "c"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_step_runs_only_when_the_one_before_succeeded() {
        let (registry, mut session, _dir) = shell_setup();
        let started = registry.dispatch(&mut session, "run.process.start", shell(&["echo one", "echo two"])).unwrap();
        let output = ended(&registry, &mut session, &started["runId"]);
        assert_eq!(
            texts(&output),
            vec![
                line("system", "sh echo one"),
                line("stdout", "one"),
                line("system", "sh echo two"),
                line("stdout", "two"),
                line("system", &t!("workbench-run-process-finished", code = 0)),
            ]
        );
        let failing = shell(&["echo one; exit 2", "echo two"]);
        let started = registry.dispatch(&mut session, "run.process.start", failing).unwrap();
        let output = ended(&registry, &mut session, &started["runId"]);
        assert_eq!(
            texts(&output),
            vec![
                line("system", "sh echo one; exit 2"),
                line("stdout", "one"),
                line("system", &t!("workbench-run-process-finished", code = 2)),
            ]
        );
        assert_eq!(output["exitCode"], 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_cannot_start_fails_the_run() {
        let (registry, mut session, _dir) = shell_setup();
        let mut params = shell(&["true"]);
        params["configuration"]["options"]["program"] = json!("/no/such/program");
        let started = registry.dispatch(&mut session, "run.process.start", params).unwrap();
        let output = ended(&registry, &mut session, &started["runId"]);
        assert_eq!(output["state"], "failed");
        assert!(output["error"].as_str().unwrap().contains("/no/such/program"), "{output}");
    }

    #[cfg(unix)]
    #[test]
    fn stopping_a_run_interrupts_it_and_kills_it_when_it_does_not_stop() {
        let (registry, mut session, _dir) = shell_setup();
        let started = registry.dispatch(&mut session, "run.process.start", shell(&["sleep 60", "echo next"])).unwrap();
        let run_id = started["runId"].clone();
        registry.dispatch(&mut session, "run.output.get", json!({ "runId": run_id, "waitMs": 1000 })).unwrap();
        let stopped = registry.dispatch(&mut session, "run.process.stop", json!({})).unwrap();
        assert_eq!(stopped["stopped"], run_id);
        let output = ended(&registry, &mut session, &run_id);
        assert_eq!(output["state"], "stopped");
        assert_eq!(texts(&output).last().unwrap(), &line("system", &t!("workbench-run-process-stopped")));
        assert!(!texts(&output).contains(&line("stdout", "next")));
        let again = registry.dispatch(&mut session, "run.process.stop", json!({ "runId": run_id })).unwrap();
        assert_eq!(again, json!({ "stopped": null }));

        let stubborn = shell(&["trap '' INT; echo ready; while :; do sleep 0.05; done"]);
        let started = registry.dispatch(&mut session, "run.process.start", stubborn).unwrap();
        let run_id = started["runId"].clone();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !texts(&registry.dispatch(&mut session, "run.output.get", json!({ "runId": run_id })).unwrap())
            .contains(&line("stdout", "ready"))
        {
            assert!(Instant::now() < deadline, "the program did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        let at = Instant::now();
        registry.dispatch(&mut session, "run.process.stop", json!({ "runId": run_id })).unwrap();
        let output = ended(&registry, &mut session, &run_id);
        assert_eq!(output["state"], "stopped");
        assert!(at.elapsed() >= zen_run::process::STOP_GRACE - Duration::from_millis(100), "{:?}", at.elapsed());
        let rerun = registry.dispatch(&mut session, "run.process.rerun", json!({ "runId": run_id })).unwrap();
        let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
        assert_eq!(list["runs"].as_array().unwrap().last().unwrap()["runId"], rerun["runId"]);
        assert_eq!(list["runs"].as_array().unwrap().last().unwrap()["state"], "running");
        registry.dispatch(&mut session, "run.process.stop", json!({ "runId": rerun["runId"] })).unwrap();
        registry.dispatch(&mut session, "run.process.stop", json!({ "runId": rerun["runId"] })).unwrap();
        assert_eq!(ended(&registry, &mut session, &rerun["runId"])["state"], "stopped");
    }

    #[cfg(unix)]
    #[test]
    fn a_scratch_folder_is_unpredictable_and_private() {
        let (first, second) = (scratch_folder(), scratch_folder());
        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(std::env::temp_dir().as_path()), "no shared parent another user could own");
        create_scratch(&first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&first).unwrap().permissions().mode() & 0o777, 0o700);
        }
        std::fs::remove_dir(&first).unwrap();
    }

    #[test]
    fn each_run_has_its_own_scratch_folder_removed_when_it_ends() {
        let (registry, mut session, dir) = shell_setup();
        let record = dir.path().join("scratch.txt");
        let step = format!(
            "mkdir -p '{{scratch}}' && touch '{{scratch}}/program' && echo '{{scratch}}' >> '{}'",
            record.display()
        );
        let first = registry.dispatch(&mut session, "run.process.start", shell(&[&step])).unwrap();
        let second = registry.dispatch(&mut session, "run.process.start", shell(&[&step])).unwrap();
        assert_eq!(ended(&registry, &mut session, &first["runId"])["exitCode"], 0);
        assert_eq!(ended(&registry, &mut session, &second["runId"])["exitCode"], 0);
        let folders: Vec<String> = std::fs::read_to_string(&record).unwrap().lines().map(str::to_owned).collect();
        assert_eq!(folders.len(), 2);
        assert_ne!(folders[0], folders[1], "two runs never share a built program");
        for folder in &folders {
            assert!(folder.contains("zen-run-"), "{folder}");
            assert!(!Path::new(folder).exists(), "{folder} is removed when its run ends");
        }
    }

    #[cfg(unix)]
    #[test]
    fn closing_the_window_kills_its_running_programs() {
        let (registry, mut session, dir) = shell_setup();
        let file = dir.path().join("pid");
        let step = format!("echo $$ > '{}'; trap '' INT; while :; do sleep 0.05; done", file.display());
        registry.dispatch(&mut session, "run.process.start", shell(&[&step])).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Ok(text) = std::fs::read_to_string(&file)
                && text.ends_with('\n')
            {
                break text.trim().to_owned();
            }
            assert!(Instant::now() < deadline, "the program did not start");
            std::thread::sleep(Duration::from_millis(20));
        };
        let alive = |pid: &str| {
            std::process::Command::new("/bin/sh")
                .args(["-c", &format!("kill -0 {pid} 2>/dev/null")])
                .status()
                .unwrap()
                .success()
        };
        assert!(alive(&pid));
        drop(session);
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(&pid) {
            assert!(Instant::now() < deadline, "the program outlived its window");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn modified_documents_are_saved_before_a_run() {
        let (registry, mut session, dir, _fake) = setup();
        let main = dir.path().join("main.fake");
        std::fs::write(&main, "old").unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();
        session.state_mut::<Workbench>().active_document_mut().unwrap().replace_ranges(vec![(0..3, "new".to_owned())]);
        registry.dispatch(&mut session, "run.process.start", json!({ "configuration": configuration("app") })).unwrap();
        assert_eq!(std::fs::read_to_string(&main).unwrap(), "new");
        assert!(!session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty());
    }

    #[test]
    fn the_session_keeps_the_newest_runs() {
        let (registry, mut session, _dir, _fake) = setup();
        for _ in 0..RUN_LIMIT + 5 {
            registry
                .dispatch(&mut session, "run.process.start", json!({ "configuration": configuration("app") }))
                .unwrap();
        }
        let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
        let runs = list["runs"].as_array().unwrap();
        assert_eq!(runs.len(), RUN_LIMIT);
        assert_eq!(runs[0]["runId"], 6);
        assert_eq!(runs.last().unwrap()["runId"], RUN_LIMIT as u64 + 5);
    }

    #[test]
    fn an_older_file_gives_its_selection_and_temporary_configurations_to_the_session() {
        let (registry, mut session, dir, _fake) = setup();
        let mut kept = FakeProvider::configuration("kept");
        kept.id = "fake.run-1".to_owned();
        let mut temporary = FakeProvider::configuration("temporary");
        temporary.id = "fake.run-2".to_owned();
        temporary.temporary = true;
        let old = RunConfigurations {
            selected: Some("fake.run-2".to_owned()),
            configurations: vec![kept.clone(), temporary.clone()],
            ..RunConfigurations::default()
        };
        std::fs::create_dir_all(dir.path().join(".zen")).unwrap();
        std::fs::write(dir.path().join(zen_project::run::FILE), serde_json::to_vec(&old).unwrap()).unwrap();
        let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["selected"], "fake.run-2");
        assert_eq!(listed["configurations"].as_array().unwrap().len(), 2);
        let text = std::fs::read_to_string(dir.path().join(zen_project::run::FILE)).unwrap();
        assert!(!text.contains("selected") && !text.contains("fake.run-2"), "{text}");
        assert_eq!(RunConfigurations::read(dir.path()).unwrap().configurations, vec![kept]);
        let runs = session.state::<Runs>().unwrap();
        assert_eq!(runs.selected(), Some("fake.run-2"));
        assert_eq!(runs.temporary(), [temporary.clone()]);
        let again = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
        assert_eq!(again, listed);

        let (registry, mut restored, other, _fake) = setup();
        let runs = restored.state_mut::<Runs>();
        runs.set_temporary(vec![Configuration { temporary: false, ..temporary.clone() }]);
        runs.set_selected(Some("fake.run-2".to_owned()));
        let listed = registry.dispatch(&mut restored, "run.configuration.list", json!({})).unwrap();
        assert_eq!(listed["selected"], "fake.run-2");
        assert_eq!(listed["configurations"][0]["temporary"], true);
        let mut saved = listed["configurations"][0].clone();
        saved["temporary"] = json!(false);
        registry.dispatch(&mut restored, "run.configuration.update", json!({ "configuration": saved })).unwrap();
        assert!(restored.state::<Runs>().unwrap().temporary().is_empty());
        assert_eq!(RunConfigurations::read(other.path()).unwrap().configurations.len(), 1, "saving makes it permanent");
    }
}
