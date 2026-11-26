use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{
    BeforeLaunchTask, CommandLine, Configuration, FieldKind, FieldSpec, ForeignTask, KindSpec, RunOutput, RunProvider,
    RunState, SharedOutput,
};

use crate::SaveAll;
use crate::run::{Driver, RunControl, Runs, project_root, run_context};

pub const RUN_CONFIGURATION: &str = "runConfiguration";

pub const EXTERNAL_TOOL: &str = "externalTool";

pub const TOOL_WINDOW: &str = "toolWindow";

pub const SAVE_FILES: &str = "saveFiles";

pub const SKIPPED: &str = "skipped";

pub const TOOL_WINDOWS: &[&str] = &[
    "project",
    "structure",
    "bookmarks",
    "commit",
    "git",
    "terminal",
    "problems",
    "find",
    "hierarchy",
    "todo",
    "run",
    "debug",
    "services",
    "notifications",
];

pub(crate) const STEP: Duration = Duration::from_millis(25);

pub(crate) type Begin = Box<dyn FnOnce() + Send>;

pub type Poll = Box<dyn FnMut(&mut Session) -> Option<Result<(), String>> + Send>;

pub enum Started {
    Done,
    Lines(Vec<CommandLine>),
    Run(u64),
    Wait(Poll),
    Skipped(String),
}

pub struct TaskStart<'a> {
    pub root: &'a Path,
    pub configuration: &'a Configuration,
    pub chain: &'a [String],
}

pub trait TaskType: Send + Sync {
    fn spec(&self) -> KindSpec;

    fn start(&self, session: &mut Session, task: &BeforeLaunchTask, launch: &TaskStart<'_>) -> Result<Started, String>;

    fn summary(&self, session: &mut Session, task: &BeforeLaunchTask) -> String {
        let _ = session;
        first_value(&self.spec(), task)
    }
}

fn first_value(spec: &KindSpec, task: &BeforeLaunchTask) -> String {
    spec.fields
        .iter()
        .filter(|field| !matches!(field.kind, FieldKind::Path | FieldKind::Bool | FieldKind::Env))
        .map(|field| task.text(&field.key).trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[derive(Default)]
pub struct TaskTypes {
    types: Vec<Arc<dyn TaskType>>,
}

impl TaskTypes {
    pub fn register(&mut self, task_type: Arc<dyn TaskType>) {
        let id = task_type.spec().id;
        self.types.retain(|kept| kept.spec().id != id);
        self.types.push(task_type);
    }
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

fn spec(id: &str, name: &str, fields: Vec<FieldSpec>, defaults: Value) -> KindSpec {
    KindSpec { id: id.to_owned(), name_message: name.to_owned(), fields, defaults }
}

struct RunAnotherConfiguration;

impl TaskType for RunAnotherConfiguration {
    fn spec(&self) -> KindSpec {
        spec(
            RUN_CONFIGURATION,
            "workbench-before-launch-run-configuration",
            vec![field("name", "workbench-before-launch-field-configuration", FieldKind::Configuration)],
            serde_json::json!({ "name": "" }),
        )
    }

    fn start(&self, session: &mut Session, task: &BeforeLaunchTask, launch: &TaskStart<'_>) -> Result<Started, String> {
        let name = task.text("name").trim();
        let file = crate::run::read_file(session, launch.root).map_err(|error| error.message)?;
        let target = file
            .configurations
            .iter()
            .find(|kept| kept.name == name)
            .cloned()
            .ok_or_else(|| t!("workbench-before-launch-no-configuration", name = name))?;
        if target.id == launch.configuration.id || launch.chain.contains(&target.id) {
            return Err(t!("workbench-before-launch-cycle", name = name));
        }
        let mut chain = launch.chain.to_vec();
        chain.push(launch.configuration.id.clone());
        let started = crate::run::start_chained(session, target, &chain).map_err(|error| error.message)?;
        Ok(Started::Run(started.run_id))
    }
}

struct RunExternalTool;

impl RunExternalTool {
    fn tool(session: &mut Session, task: &BeforeLaunchTask) -> Option<crate::external_tools::ExternalTool> {
        let wanted = task.text("tool").trim();
        let tools = session.state_mut::<crate::external_tools::ExternalTools>().tools();
        tools.iter().find(|tool| tool.id == wanted).or_else(|| tools.iter().find(|tool| tool.name == wanted)).cloned()
    }
}

impl TaskType for RunExternalTool {
    fn spec(&self) -> KindSpec {
        spec(
            EXTERNAL_TOOL,
            "workbench-before-launch-external-tool",
            vec![field("tool", "workbench-before-launch-field-tool", FieldKind::ExternalTool)],
            serde_json::json!({ "tool": "" }),
        )
    }

    fn start(&self, session: &mut Session, task: &BeforeLaunchTask, _: &TaskStart<'_>) -> Result<Started, String> {
        let tool = Self::tool(session, task)
            .ok_or_else(|| t!("workbench-external-tools-unknown", id = task.text("tool").trim()))?;
        if !tool.enabled {
            return Err(t!("workbench-external-tools-disabled", name = tool.name.as_str()));
        }
        let context = crate::external_tools::macro_context(session, None);
        let launch = crate::external_tools::launch(&tool, &context).map_err(|error| error.message)?;
        let configuration = crate::external_tools::configuration(&tool, &launch);
        let started = crate::run::start(session, configuration).map_err(|error| error.message)?;
        Ok(Started::Run(started.run_id))
    }

    fn summary(&self, session: &mut Session, task: &BeforeLaunchTask) -> String {
        Self::tool(session, task).map_or_else(|| task.text("tool").trim().to_owned(), |tool| tool.name)
    }
}

#[derive(Default)]
pub struct ToolWindowRequests(Vec<String>);

pub fn take_tool_windows(session: &mut Session) -> Vec<String> {
    std::mem::take(&mut session.state_mut::<ToolWindowRequests>().0)
}

struct ActivateToolWindow;

impl TaskType for ActivateToolWindow {
    fn spec(&self) -> KindSpec {
        spec(
            TOOL_WINDOW,
            "workbench-before-launch-tool-window",
            vec![field(
                "toolWindow",
                "workbench-before-launch-field-tool-window",
                FieldKind::Choice(TOOL_WINDOWS.iter().map(|id| (*id).to_owned()).collect()),
            )],
            serde_json::json!({ "toolWindow": "run" }),
        )
    }

    fn start(&self, session: &mut Session, task: &BeforeLaunchTask, _: &TaskStart<'_>) -> Result<Started, String> {
        let id = task.text("toolWindow").trim();
        if !TOOL_WINDOWS.contains(&id) {
            return Err(t!("workbench-before-launch-unknown-tool-window", id = id, known = TOOL_WINDOWS.join(", ")));
        }
        session.state_mut::<ToolWindowRequests>().0.push(id.to_owned());
        Ok(Started::Done)
    }

    fn summary(&self, _: &mut Session, task: &BeforeLaunchTask) -> String {
        let id = task.text("toolWindow").trim();
        if TOOL_WINDOWS.contains(&id) {
            t!(&format!("workbench-before-launch-field-tool-window-{id}"))
        } else {
            id.to_owned()
        }
    }
}

struct SaveFiles;

impl TaskType for SaveFiles {
    fn spec(&self) -> KindSpec {
        spec(SAVE_FILES, "workbench-before-launch-save-files", Vec::new(), serde_json::json!({}))
    }

    fn start(&self, session: &mut Session, _: &BeforeLaunchTask, _: &TaskStart<'_>) -> Result<Started, String> {
        SaveAll.run(session, NoParams {}).map_err(|error| error.message)?;
        Ok(Started::Done)
    }
}

struct ProviderTask {
    provider: Arc<dyn RunProvider>,
    spec: KindSpec,
}

impl TaskType for ProviderTask {
    fn spec(&self) -> KindSpec {
        self.spec.clone()
    }

    fn start(&self, session: &mut Session, task: &BeforeLaunchTask, launch: &TaskStart<'_>) -> Result<Started, String> {
        let context = run_context(session, launch.root);
        match self.provider.task_lines(task, &context) {
            Some(lines) => lines.map(Started::Lines),
            None => Ok(Started::Skipped(t!("workbench-before-launch-unknown-type", kind = task.kind.as_str()))),
        }
    }
}

struct Typed {
    provider: Option<String>,
    task_type: Arc<dyn TaskType>,
}

fn types(session: &Session) -> Vec<Typed> {
    let built_in: [Arc<dyn TaskType>; 4] = [
        Arc::new(RunAnotherConfiguration),
        Arc::new(RunExternalTool),
        Arc::new(ActivateToolWindow),
        Arc::new(SaveFiles),
    ];
    let mut found: Vec<Typed> = built_in.into_iter().map(|task_type| Typed { provider: None, task_type }).collect();
    if let Some(registered) = session.state::<TaskTypes>() {
        found.extend(registered.types.iter().map(|task_type| Typed { provider: None, task_type: task_type.clone() }));
    }
    let providers = session.state::<Runs>().map(|runs| runs.providers().to_vec()).unwrap_or_default();
    for provider in providers {
        for spec in provider.task_kinds() {
            let task_type: Arc<dyn TaskType> = Arc::new(ProviderTask { provider: provider.clone(), spec });
            found.push(Typed { provider: Some(provider.id().to_owned()), task_type });
        }
    }
    found
}

pub(crate) fn find_type(session: &Session, kind: &str) -> Option<Arc<dyn TaskType>> {
    types(session).into_iter().map(|typed| typed.task_type).find(|task_type| task_type.spec().id == kind)
}

pub fn label(session: &mut Session, task: &BeforeLaunchTask) -> String {
    if task.kind == SKIPPED {
        return t!("workbench-before-launch-skipped-label", id = task.text("foreignId"));
    }
    let Some(task_type) = find_type(session, &task.kind) else {
        return t!("workbench-before-launch-unknown-label", kind = task.kind.as_str());
    };
    let name = t!(&task_type.spec().name_message);
    let summary = task_type.summary(session, task);
    if summary.is_empty() { name } else { t!("workbench-before-launch-label", name = name, summary = summary) }
}

enum GateState {
    Tasks(Option<Arc<Driver>>),
    Open(Option<Arc<dyn RunControl>>),
}

pub(crate) struct Gate {
    output: Mutex<Option<SharedOutput>>,
    state: Mutex<GateState>,
    inner: Mutex<Option<Arc<dyn RunControl>>>,
}

impl Gate {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self { output: Mutex::new(None), state: Mutex::new(GateState::Tasks(None)), inner: Mutex::new(None) })
    }

    pub(crate) fn bind(&self, output: SharedOutput, inner: Option<Arc<dyn RunControl>>) {
        *lock(&self.output) = Some(output);
        *lock(&self.inner) = inner;
    }

    fn set_task(&self, driver: Option<Arc<Driver>>) {
        let mut state = lock(&self.state);
        if let GateState::Tasks(task) = &mut *state {
            *task = driver;
        }
    }

    fn open(&self) {
        *lock(&self.state) = GateState::Open(lock(&self.inner).clone());
    }

    fn end_stopped(&self) -> bool {
        let Some(output) = lock(&self.output).clone() else { return false };
        output.update(|output| {
            let running = output.is_running();
            if running {
                output.system(&t!("workbench-run-process-stopped"));
                output.end(RunState::Stopped, None);
            }
            running
        })
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl RunControl for Gate {
    fn stop(&self) -> bool {
        let state = lock(&self.state);
        match &*state {
            GateState::Tasks(Some(task)) => task.stop(),
            GateState::Tasks(None) | GateState::Open(None) => {
                drop(state);
                self.end_stopped()
            }
            GateState::Open(Some(inner)) => inner.stop(),
        }
    }

    fn kill(&self) {
        let state = lock(&self.state);
        match &*state {
            GateState::Tasks(Some(task)) => task.kill(),
            GateState::Tasks(None) | GateState::Open(None) => {
                drop(state);
                self.end_stopped();
            }
            GateState::Open(Some(inner)) => inner.kill(),
        }
    }
}

enum Waiting {
    Nothing,
    Lines(Arc<AtomicBool>),
    Run { label: String, output: SharedOutput },
    Poll { label: String, poll: Poll },
}

struct Launch {
    root: PathBuf,
    configuration: Configuration,
    output: SharedOutput,
    tasks: VecDeque<BeforeLaunchTask>,
    waiting: Waiting,
    begin: Option<Begin>,
    gate: Arc<Gate>,
    chain: Vec<String>,
}

#[derive(Default)]
pub struct Launches(Vec<Launch>);

impl Launches {
    pub fn pending(&self) -> usize {
        self.0.len()
    }
}

pub(crate) fn launch(
    session: &mut Session,
    root: &Path,
    run_id: u64,
    configuration: &Configuration,
    gate: Arc<Gate>,
    begin: Begin,
    chain: &[String],
) {
    let Some(output) = session.state::<Runs>().and_then(|runs| runs.run(run_id)).map(|run| run.output.clone()) else {
        return;
    };
    let mut launch = Launch {
        root: root.to_owned(),
        configuration: configuration.clone(),
        output,
        tasks: configuration.common.before_launch.iter().cloned().collect(),
        waiting: Waiting::Nothing,
        begin: Some(begin),
        gate,
        chain: chain.to_vec(),
    };
    if step(session, &mut launch) {
        session.state_mut::<Launches>().0.push(launch);
    }
}

pub fn advance(session: &mut Session) -> bool {
    let launches = std::mem::take(&mut session.state_mut::<Launches>().0);
    let mut kept = Vec::new();
    for mut launch in launches {
        if step(session, &mut launch) {
            kept.push(launch);
        }
    }
    let state = session.state_mut::<Launches>();
    let added = std::mem::take(&mut state.0);
    state.0 = kept;
    state.0.extend(added);
    !state.0.is_empty()
}

fn fail(launch: &Launch, message: String) {
    launch.output.update(|output| {
        output.system(&message);
        output.fail_before_launch(message.clone(), None);
    });
}

fn system(launch: &Launch, text: &str) {
    launch.output.update(|output| {
        output.system(text);
    });
}

fn step(session: &mut Session, launch: &mut Launch) -> bool {
    loop {
        if !launch.output.read(RunOutput::is_running) {
            launch.gate.set_task(None);
            return false;
        }
        if let Waiting::Poll { label, poll } = &mut launch.waiting {
            let Some(result) = poll(session) else { return true };
            let label = std::mem::take(label);
            launch.waiting = Waiting::Nothing;
            if let Err(error) = result {
                fail(launch, t!("workbench-before-launch-failed", task = label.as_str(), error = error));
                return false;
            }
            system(launch, &t!("workbench-before-launch-done", task = label.as_str()));
        }
        match &launch.waiting {
            Waiting::Nothing | Waiting::Poll { .. } => {}
            Waiting::Lines(done) if done.load(Ordering::SeqCst) => {
                launch.gate.set_task(None);
                launch.waiting = Waiting::Nothing;
            }
            Waiting::Lines(_) => return true,
            Waiting::Run { output, .. } if output.read(RunOutput::is_running) => return true,
            Waiting::Run { label, output } => {
                let (state, code) = output.read(|ended| (ended.state(), ended.exit_code()));
                if state == RunState::Finished && code == Some(0) {
                    let text = t!("workbench-before-launch-done", task = label.as_str());
                    system(launch, &text);
                    launch.waiting = Waiting::Nothing;
                } else {
                    let error = match code {
                        Some(code) => t!("workbench-run-process-finished", code = code),
                        None => t!(
                            "workbench-before-launch-ended",
                            state = serde_json::to_value(state)
                                .ok()
                                .and_then(|value| value.as_str().map(str::to_owned))
                                .unwrap_or_default()
                        ),
                    };
                    fail(launch, t!("workbench-before-launch-failed", task = label.as_str(), error = error));
                    return false;
                }
            }
        }
        let Some(task) = launch.tasks.pop_front() else {
            launch.gate.open();
            if let Some(begin) = launch.begin.take() {
                begin();
            }
            return false;
        };
        let label = label(session, &task);
        system(launch, &t!("workbench-before-launch-task", task = label.as_str()));
        let Some(task_type) = (task.kind != SKIPPED).then(|| find_type(session, &task.kind)).flatten() else {
            let reason = if task.kind == SKIPPED {
                t!("workbench-before-launch-not-supported")
            } else {
                t!("workbench-before-launch-unknown-type", kind = task.kind.as_str())
            };
            system(launch, &t!("workbench-before-launch-skipped", task = label.as_str(), reason = reason));
            continue;
        };
        let start = TaskStart { root: &launch.root, configuration: &launch.configuration, chain: &launch.chain };
        match task_type.start(session, &task, &start) {
            Err(error) => {
                fail(launch, t!("workbench-before-launch-failed", task = label.as_str(), error = error));
                return false;
            }
            Ok(Started::Done) => {}
            Ok(Started::Skipped(reason)) => {
                system(launch, &t!("workbench-before-launch-skipped", task = label.as_str(), reason = reason));
            }
            Ok(Started::Lines(lines)) if lines.is_empty() => {}
            Ok(Started::Lines(lines)) => {
                let env = run_context(session, &launch.root).env.env;
                let driver = Driver::task(lines, env, launch.output.clone(), label);
                let done = Arc::new(AtomicBool::new(false));
                let finished = done.clone();
                let output = launch.output.clone();
                driver.then(Box::new(move || {
                    finished.store(true, Ordering::SeqCst);
                    output.update(|_| ());
                }));
                launch.gate.set_task(Some(driver.clone()));
                launch.waiting = Waiting::Lines(done);
                driver.start();
            }
            Ok(Started::Run(run_id)) => {
                let Some(output) =
                    session.state::<Runs>().and_then(|runs| runs.run(run_id)).map(|run| run.output.clone())
                else {
                    continue;
                };
                launch.waiting = Waiting::Run { label, output };
            }
            Ok(Started::Wait(poll)) => launch.waiting = Waiting::Poll { label, poll },
        }
    }
}

pub(crate) fn adopt_tasks(
    session: &Session,
    root: &Path,
    foreign: &[ForeignTask],
    providers: &[Arc<dyn RunProvider>],
) -> Vec<BeforeLaunchTask> {
    let tools = session.state::<crate::external_tools::ExternalTools>().map(|store| store.tools()).unwrap_or_default();
    foreign
        .iter()
        .filter(|task| task.enabled)
        .map(|task| {
            let option = |key: &str| task.options.get(key).cloned().unwrap_or_default();
            match task.id.as_str() {
                "RunConfigurationTask" => BeforeLaunchTask::new(
                    RUN_CONFIGURATION,
                    serde_json::json!({ "name": option("run_configuration_name") }),
                ),
                "ToolBeforeRunTask" => {
                    let action = option("actionId");
                    let name = action
                        .strip_prefix("Tool_")
                        .and_then(|rest| rest.split_once('_').map(|(_, name)| name.to_owned()))
                        .unwrap_or(action);
                    let tool = tools.iter().find(|tool| tool.name == name).map_or(name, |tool| tool.id.clone());
                    BeforeLaunchTask::new(EXTERNAL_TOOL, serde_json::json!({ "tool": tool }))
                }
                _ => providers
                    .iter()
                    .find_map(|provider| provider.adopt_task(task, root))
                    .unwrap_or_else(|| BeforeLaunchTask::new(SKIPPED, serde_json::json!({ "foreignId": task.id }))),
            }
        })
        .collect()
}

/// A type of before-launch task, as the dialog's Add menu lists it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskTypeInfo {
    /// The run provider (language module or plug-in) that adds it; absent for the IDE's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// The type's name in the interface's language.
    pub name: String,
    #[serde(flatten)]
    pub spec: KindSpec,
}

/// What `run.beforeLaunch.types` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskTypeList {
    pub types: Vec<TaskTypeInfo>,
}

pub struct ListTypes;

impl Command for ListTypes {
    type Params = NoParams;
    type Output = TaskTypeList;
    const ID: &'static str = "run.beforeLaunch.types";
    const TITLE: &'static str = "Before Launch Task Types";
    const DESCRIPTION: &'static str = "The types of before-launch task a configuration can have, as the Add menu lists \
         them: each with its id (the task's `type`), name, option fields and defaults, and the run provider that adds \
         it (a language module or plug-in) when it is not the IDE's own.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<TaskTypeList, CommandError> {
        let types = types(session)
            .into_iter()
            .map(|typed| {
                let spec = typed.task_type.spec();
                TaskTypeInfo { provider: typed.provider, name: t!(&spec.name_message), spec }
            })
            .collect();
        Ok(TaskTypeList { types })
    }
}

/// A configuration's before-launch task as `run.beforeLaunch.list` gives it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    /// Its place in the list, from 0.
    pub index: usize,
    /// How the list and the console name it.
    pub label: String,
    #[serde(flatten)]
    pub task: BeforeLaunchTask,
}

/// A configuration's before-launch tasks.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskList {
    pub configuration_id: String,
    pub tasks: Vec<TaskInfo>,
    /// Whether the Run/Debug Configurations dialog opens before each launch from the window.
    pub show_page: bool,
}

/// A configuration whose before-launch tasks are read or changed.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    pub configuration_id: String,
}

fn task_list(session: &mut Session, configuration: &Configuration) -> TaskList {
    let tasks = configuration
        .common
        .before_launch
        .iter()
        .enumerate()
        .map(|(index, task)| TaskInfo { index, label: label(session, task), task: task.clone() })
        .collect();
    TaskList { configuration_id: configuration.id.clone(), tasks, show_page: configuration.common.show_page }
}

fn change(
    session: &mut Session,
    id: &str,
    edit: impl FnOnce(&mut Configuration) -> Result<(), CommandError>,
) -> Result<TaskList, CommandError> {
    let root = project_root(session)?;
    let mut file = crate::run::read_file(session, &root)?;
    crate::run::refuse_adopted(&file, id)?;
    let mut configuration = file.get(id).cloned().ok_or_else(|| crate::run::unknown_configuration(id))?;
    edit(&mut configuration)?;
    file.update(configuration.clone());
    crate::run::write_file(session, &root, &file)?;
    Ok(task_list(session, &configuration))
}

fn known(session: &Session, task: &BeforeLaunchTask) -> Result<(), CommandError> {
    if find_type(session, &task.kind).is_some() {
        return Ok(());
    }
    let ids: Vec<String> = types(session).iter().map(|typed| typed.task_type.spec().id).collect();
    Err(CommandError::new(t!(
        "workbench-before-launch-unknown-type-of",
        kind = task.kind.as_str(),
        kinds = ids.join(", ")
    )))
}

fn out_of_range(index: usize, len: usize) -> CommandError {
    CommandError::new(t!("workbench-before-launch-no-task", index = index, count = len))
}

pub struct ListTasks;

impl Command for ListTasks {
    type Params = ListParams;
    type Output = TaskList;
    const ID: &'static str = "run.beforeLaunch.list";
    const TITLE: &'static str = "Before Launch Tasks";
    const DESCRIPTION: &'static str = "A run configuration's before-launch tasks in the order they run, each with its \
         index, type, options and the label the list and the console give it, and whether Show this page is on.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<TaskList, CommandError> {
        let root = project_root(session)?;
        let file = crate::run::read_file(session, &root)?;
        let configuration = file
            .get(&params.configuration_id)
            .cloned()
            .ok_or_else(|| crate::run::unknown_configuration(&params.configuration_id))?;
        Ok(task_list(session, &configuration))
    }
}

/// The parameters of `run.beforeLaunch.add`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddParams {
    pub configuration_id: String,
    /// The task: `{ type, ...options }`, a type `run.beforeLaunch.types` lists.
    pub task: BeforeLaunchTask,
    /// Where it goes, from 0; the end when absent.
    #[serde(default)]
    pub index: Option<usize>,
}

pub struct AddTask;

impl Command for AddTask {
    type Params = AddParams;
    type Output = TaskList;
    const ID: &'static str = "run.beforeLaunch.add";
    const TITLE: &'static str = "Add Before Launch Task";
    const DESCRIPTION: &'static str = "Adds a before-launch task to a saved run configuration (Add, ⌘N, in the \
         dialog's Before launch list), at the end or at `index`, and saves the file; returns the list.";

    fn run(&self, session: &mut Session, params: AddParams) -> Result<TaskList, CommandError> {
        known(session, &params.task)?;
        change(session, &params.configuration_id, |configuration| {
            let tasks = &mut configuration.common.before_launch;
            let index = params.index.unwrap_or(tasks.len());
            if index > tasks.len() {
                return Err(out_of_range(index, tasks.len()));
            }
            tasks.insert(index, params.task);
            Ok(())
        })
    }
}

/// A task of a configuration, by its place in the list.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskParams {
    pub configuration_id: String,
    /// The task's place in the list, from 0.
    pub index: usize,
}

pub struct RemoveTask;

impl Command for RemoveTask {
    type Params = TaskParams;
    type Output = TaskList;
    const ID: &'static str = "run.beforeLaunch.remove";
    const TITLE: &'static str = "Remove Before Launch Task";
    const DESCRIPTION: &'static str =
        "Removes a configuration's before-launch task by its index and saves the file; returns the list.";

    fn run(&self, session: &mut Session, params: TaskParams) -> Result<TaskList, CommandError> {
        change(session, &params.configuration_id, |configuration| {
            let tasks = &mut configuration.common.before_launch;
            if params.index >= tasks.len() {
                return Err(out_of_range(params.index, tasks.len()));
            }
            tasks.remove(params.index);
            Ok(())
        })
    }
}

/// The parameters of `run.beforeLaunch.update`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateParams {
    pub configuration_id: String,
    /// The task's place in the list, from 0.
    pub index: usize,
    /// The task that replaces it.
    pub task: BeforeLaunchTask,
}

pub struct UpdateTask;

impl Command for UpdateTask {
    type Params = UpdateParams;
    type Output = TaskList;
    const ID: &'static str = "run.beforeLaunch.update";
    const TITLE: &'static str = "Edit Before Launch Task";
    const DESCRIPTION: &'static str =
        "Replaces a configuration's before-launch task at an index (Edit in the dialog) and saves the file.";

    fn run(&self, session: &mut Session, params: UpdateParams) -> Result<TaskList, CommandError> {
        known(session, &params.task)?;
        change(session, &params.configuration_id, |configuration| {
            let tasks = &mut configuration.common.before_launch;
            let len = tasks.len();
            let slot = tasks.get_mut(params.index).ok_or_else(|| out_of_range(params.index, len))?;
            *slot = params.task;
            Ok(())
        })
    }
}

/// The parameters of `run.beforeLaunch.move`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveParams {
    pub configuration_id: String,
    /// The task's place in the list, from 0.
    pub index: usize,
    /// Up (earlier) when true, down otherwise.
    pub up: bool,
}

pub struct MoveTask;

impl Command for MoveTask {
    type Params = MoveParams;
    type Output = TaskList;
    const ID: &'static str = "run.beforeLaunch.move";
    const TITLE: &'static str = "Move Before Launch Task";
    const DESCRIPTION: &'static str = "Moves a configuration's before-launch task one place up or down (Move Up and \
         Move Down in the dialog) and saves the file; at either end it stays.";

    fn run(&self, session: &mut Session, params: MoveParams) -> Result<TaskList, CommandError> {
        change(session, &params.configuration_id, |configuration| {
            let tasks = &mut configuration.common.before_launch;
            if params.index >= tasks.len() {
                return Err(out_of_range(params.index, tasks.len()));
            }
            let other = if params.up { params.index.checked_sub(1) } else { Some(params.index + 1) };
            if let Some(other) = other.filter(|other| *other < tasks.len()) {
                tasks.swap(params.index, other);
            }
            Ok(())
        })
    }
}

/// The parameters of `run.beforeLaunch.showPage`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShowPageParams {
    pub configuration_id: String,
    pub show: bool,
}

pub struct SetShowPage;

impl Command for SetShowPage {
    type Params = ShowPageParams;
    type Output = TaskList;
    const ID: &'static str = "run.beforeLaunch.showPage";
    const TITLE: &'static str = "Show This Page";
    const DESCRIPTION: &'static str = "Turns Show this page on or off for a configuration: when on, Run and Debug \
         from the window open the Run/Debug Configurations dialog on it first, and its Run or Debug button starts \
         it. Starts by agents (run.process.start) never open the dialog.";

    fn run(&self, session: &mut Session, params: ShowPageParams) -> Result<TaskList, CommandError> {
        change(session, &params.configuration_id, |configuration| {
            configuration.common.show_page = params.show;
            Ok(())
        })
    }
}

/// The parameters of `run.beforeLaunch.page`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PageParams {
    /// The configuration to start; the selected one when absent.
    pub configuration_id: Option<String>,
}

/// What `run.beforeLaunch.page` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageAnswer {
    /// The configuration whose dialog opens before it starts; absent when it starts at once.
    pub show: Option<String>,
}

pub struct PageBeforeStart;

impl Command for PageBeforeStart {
    type Params = PageParams;
    type Output = PageAnswer;
    const ID: &'static str = "run.beforeLaunch.page";
    const TITLE: &'static str = "Show Page Before Launch";
    const DESCRIPTION: &'static str = "Whether a start from the window opens the Run/Debug Configurations dialog \
         first: the id of the configuration (the given one, else the selected one) when its Show this page is on.";

    fn run(&self, session: &mut Session, params: PageParams) -> Result<PageAnswer, CommandError> {
        let root = project_root(session)?;
        let file = crate::run::read_file(session, &root)?;
        let chosen = match &params.configuration_id {
            Some(id) => file.get(id),
            None => file.selected_configuration(),
        };
        Ok(PageAnswer { show: chosen.filter(|kept| kept.common.show_page).map(|kept| kept.id.clone()) })
    }
}

pub fn register_type(session: &mut Session, task_type: Arc<dyn TaskType>) {
    session.state_mut::<TaskTypes>().register(task_type);
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTypes)?;
    registry.register(ListTasks)?;
    registry.register(AddTask)?;
    registry.register(RemoveTask)?;
    registry.register(UpdateTask)?;
    registry.register(MoveTask)?;
    registry.register(SetShowPage)?;
    registry.register(PageBeforeStart)?;
    Ok(())
}

#[cfg(test)]
mod tests;
