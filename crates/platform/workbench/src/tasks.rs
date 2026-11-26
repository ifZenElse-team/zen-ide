use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::inspect::Programs;

pub const LOG_LINES: usize = 5000;
const KEPT: usize = 20;
const CANCEL_GRACE: Duration = Duration::from_secs(2);

pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// How a task is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    /// Still running.
    Running,
    /// It ended well.
    Succeeded,
    /// It failed.
    Failed,
    /// It was stopped before it ended.
    Cancelled,
}

/// A task as the window and agents see it, without its log.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    /// Its id, unique in the window.
    pub id: u64,
    /// What it does, e.g. "Updating 3 dependencies".
    pub title: String,
    /// Who started it, e.g. `inspections` or `vcs`.
    pub source: String,
    /// How it is doing.
    pub state: TaskState,
    /// How far it is, from 0 to 1, when it knows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f32>,
    /// Its current step or the last line it printed.
    pub detail: String,
    /// When it is over: what to tell (the error when it failed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The lines of its log (`tasks.log.read` reads them).
    pub log_lines: usize,
    /// When it started, in seconds since 1970.
    pub started: u64,
    /// When it ended, in seconds since 1970.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<u64>,
    /// Whether it can be stopped while it runs (`tasks.background.cancel`, the × in the status bar).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cancellable: bool,
    /// Whether it was asked to stop and is still stopping.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cancelling: bool,
    /// Whether it can be paused and resumed (`tasks.background.pause`, `tasks.background.resume`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pausable: bool,
    /// Whether it is paused now.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
}

type CancelHook = Arc<dyn Fn() + Send + Sync>;
type PauseHook = Arc<dyn Fn(bool) + Send + Sync>;

struct TaskData {
    info: TaskInfo,
    log: VecDeque<String>,
    programs: Vec<Arc<dyn Programs>>,
    cancel: Vec<CancelHook>,
    pause: Option<PauseHook>,
}

#[derive(Default)]
struct Shared {
    tasks: Mutex<Vec<TaskData>>,
    waker: Mutex<Option<Waker>>,
}

impl Shared {
    fn tasks(&self) -> MutexGuard<'_, Vec<TaskData>> {
        self.tasks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wake(&self) {
        let waker = self.waker.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(waker) = waker {
            waker();
        }
    }

    fn with(&self, id: u64, f: impl FnOnce(&mut TaskData)) {
        if let Some(task) = self.tasks().iter_mut().find(|task| task.info.id == id) {
            f(task);
        }
        self.wake();
    }
}

#[derive(Default)]
pub struct Tasks {
    shared: Arc<Shared>,
    next: u64,
    panel: bool,
    selected: Option<u64>,
}

#[derive(Clone)]
pub struct TaskHandle {
    shared: Arc<Shared>,
    id: u64,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl TaskHandle {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn log(&self, text: &str) {
        self.shared.with(self.id, |task| {
            for line in text.lines() {
                let line = line.trim_end();
                task.log.push_back(line.to_owned());
                if !line.trim().is_empty() {
                    task.info.detail = line.trim().to_owned();
                }
            }
            while task.log.len() > LOG_LINES {
                task.log.pop_front();
            }
            task.info.log_lines = task.log.len();
        });
    }

    pub fn progress(&self, fraction: Option<f32>, detail: &str) {
        self.shared.with(self.id, |task| {
            task.info.fraction = fraction.map(|fraction| fraction.clamp(0.0, 1.0));
            if !detail.is_empty() {
                task.info.detail = detail.to_owned();
            }
        });
    }

    pub fn finish(&self, result: Result<String, String>) {
        self.shared.with(self.id, |task| {
            let (state, message) = match result {
                Ok(message) if task.info.cancelling && !message.is_empty() => (TaskState::Cancelled, message),
                _ if task.info.cancelling => (TaskState::Cancelled, t!("workbench-task-cancelled")),
                Ok(message) => (TaskState::Succeeded, message),
                Err(error) => {
                    for line in error.lines() {
                        if !task.log.iter().any(|logged| logged == line) {
                            task.log.push_back(line.to_owned());
                        }
                    }
                    task.info.log_lines = task.log.len();
                    (TaskState::Failed, error)
                }
            };
            if state == TaskState::Cancelled {
                task.info.detail.clear();
            }
            task.info.state = state;
            task.info.fraction = None;
            task.info.message = Some(message);
            task.info.ended = Some(now());
            task.info.cancellable = false;
            task.info.cancelling = false;
            task.info.pausable = false;
            task.info.paused = false;
        });
    }

    pub fn dismiss(&self) {
        if self.cancel_requested() {
            self.finish(Ok(String::new()));
            return;
        }
        self.shared.tasks().retain(|task| task.info.id != self.id);
        self.shared.wake();
    }

    pub fn running(&self) -> bool {
        self.shared.tasks().iter().any(|task| task.info.id == self.id && task.info.state == TaskState::Running)
    }

    pub fn cancel_requested(&self) -> bool {
        self.shared.tasks().iter().any(|task| task.info.id == self.id && task.info.cancelling)
    }

    pub fn on_cancel(&self, hook: impl Fn() + Send + Sync + 'static) {
        self.shared.with(self.id, |task| {
            task.info.cancellable = true;
            task.cancel.push(Arc::new(hook));
        });
    }

    pub fn cancel_with(&self, flag: Arc<AtomicBool>) {
        self.on_cancel(move || flag.store(true, Ordering::Relaxed));
    }

    pub fn cancel_kills(&self, programs: Arc<dyn Programs>) {
        self.attach(Arc::clone(&programs));
        self.on_cancel(move || {
            let programs = [Arc::clone(&programs)];
            std::thread::spawn(move || crate::exit::stop_work(&[], &programs, CANCEL_GRACE));
        });
    }

    pub fn set_cancellable(&self, cancellable: bool) {
        self.shared.with(self.id, |task| {
            task.info.cancellable = cancellable && !task.cancel.is_empty() && !task.info.cancelling;
        });
    }

    pub fn on_pause(&self, hook: impl Fn(bool) + Send + Sync + 'static) {
        self.shared.with(self.id, |task| {
            if task.info.state == TaskState::Running {
                task.info.pausable = true;
                task.pause = Some(Arc::new(hook));
            }
        });
    }

    pub fn attach(&self, programs: Arc<dyn Programs>) {
        if let Some(task) = self.shared.tasks().iter_mut().find(|task| task.info.id == self.id) {
            task.programs.push(programs);
        }
    }
}

impl Tasks {
    pub fn start(&mut self, title: &str, source: &str, show: bool) -> TaskHandle {
        self.next += 1;
        let id = self.next;
        {
            let mut tasks = self.shared.tasks();
            tasks.push(TaskData {
                info: TaskInfo {
                    id,
                    title: title.to_owned(),
                    source: source.to_owned(),
                    state: TaskState::Running,
                    fraction: None,
                    detail: String::new(),
                    message: None,
                    log_lines: 0,
                    started: now(),
                    ended: None,
                    cancellable: false,
                    cancelling: false,
                    pausable: false,
                    paused: false,
                },
                log: VecDeque::new(),
                programs: Vec::new(),
                cancel: Vec::new(),
                pause: None,
            });
            while tasks.len() > KEPT {
                match tasks.iter().position(|task| task.info.state != TaskState::Running) {
                    Some(index) => {
                        tasks.remove(index);
                    }
                    None => break,
                }
            }
        }
        if show {
            self.panel = true;
            self.selected = Some(id);
        }
        self.shared.wake();
        TaskHandle { shared: Arc::clone(&self.shared), id }
    }

    pub fn set_waker(&mut self, waker: Waker) {
        *self.shared.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
    }

    pub fn list(&self) -> Vec<TaskInfo> {
        self.shared.tasks().iter().map(|task| task.info.clone()).collect()
    }

    pub fn running_programs(&self) -> Vec<Arc<dyn Programs>> {
        self.shared
            .tasks()
            .iter()
            .filter(|task| task.info.state == TaskState::Running)
            .flat_map(|task| task.programs.iter().cloned())
            .collect()
    }

    pub fn running(&self) -> bool {
        self.shared.tasks().iter().any(|task| task.info.state == TaskState::Running)
    }

    pub fn log(&self, id: Option<u64>, tail: Option<usize>) -> Option<(TaskInfo, Vec<String>)> {
        let tasks = self.shared.tasks();
        let task = match id {
            Some(id) => tasks.iter().find(|task| task.info.id == id)?,
            None => tasks.last()?,
        };
        let skip = tail.map_or(0, |tail| task.log.len().saturating_sub(tail));
        Some((task.info.clone(), task.log.iter().skip(skip).cloned().collect()))
    }

    pub fn panel_shown(&self) -> bool {
        self.panel
    }

    pub fn selected(&self) -> Option<u64> {
        let tasks = self.shared.tasks();
        self.selected
            .filter(|id| tasks.iter().any(|task| task.info.id == *id))
            .or(tasks.last().map(|task| task.info.id))
    }

    pub fn set_panel(&mut self, shown: bool, id: Option<u64>) {
        self.panel = shown;
        if id.is_some() {
            self.selected = id;
        }
    }

    pub fn toggle_panel(&mut self) {
        self.panel = !self.panel;
    }

    pub fn cancel(&mut self, id: Option<u64>) -> Option<u64> {
        let (id, hooks) = {
            let mut tasks = self.shared.tasks();
            let task = tasks.iter_mut().rev().find(|task| {
                task.info.state == TaskState::Running && task.info.cancellable && id.is_none_or(|id| task.info.id == id)
            })?;
            task.info.cancelling = true;
            task.info.cancellable = false;
            task.info.detail = t!("workbench-task-cancelling");
            (task.info.id, task.cancel.clone())
        };
        for hook in hooks {
            hook();
        }
        self.shared.wake();
        Some(id)
    }

    pub fn pause(&mut self, id: Option<u64>, paused: bool) -> Option<u64> {
        let (id, hook) = {
            let mut tasks = self.shared.tasks();
            let task = tasks.iter_mut().rev().find(|task| {
                task.info.state == TaskState::Running
                    && task.info.pausable
                    && task.info.paused != paused
                    && id.is_none_or(|id| task.info.id == id)
            })?;
            task.info.paused = paused;
            (task.info.id, task.pause.clone()?)
        };
        hook(paused);
        self.shared.wake();
        Some(id)
    }

    pub fn clear_finished(&mut self) {
        self.shared.tasks().retain(|task| task.info.state == TaskState::Running);
    }
}

/// What `tasks.background.list` answers.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TasksReport {
    /// Whether the panel with the log is shown.
    pub panel_shown: bool,
    /// The task the panel shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<u64>,
    /// Every task, the oldest first.
    pub tasks: Vec<TaskInfo>,
}

pub struct ListTasks;

impl Command for ListTasks {
    type Params = NoParams;
    type Output = TasksReport;
    const ID: &'static str = "tasks.background.list";
    const TITLE: &'static str = "Background Tasks";
    const DESCRIPTION: &'static str = "The background tasks of the window (e.g. a quick-fix running go get, a Git fetch, \
         indexing, a language server's work), the oldest first, each with its state (running, succeeded, failed, \
         cancelled), progress, last line, whether it can be cancelled or paused and, when over, its message or error; \
         and whether the panel with their log is shown. tasks.log.read reads a task's log.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<TasksReport, CommandError> {
        let tasks = session.state_mut::<Tasks>();
        Ok(TasksReport { panel_shown: tasks.panel_shown(), selected: tasks.selected(), tasks: tasks.list() })
    }
}

/// The parameters of `tasks.log.read`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogParams {
    /// The task; the latest when absent.
    #[serde(default)]
    pub id: Option<u64>,
    /// Only the last lines, this many.
    #[serde(default)]
    pub tail: Option<usize>,
}

/// What `tasks.log.read` answers.
#[derive(Serialize, JsonSchema)]
pub struct TaskLog {
    /// The task.
    pub task: TaskInfo,
    /// The lines of its log, the oldest first.
    pub lines: Vec<String>,
}

pub struct ReadLog;

impl Command for ReadLog {
    type Params = LogParams;
    type Output = TaskLog;
    const ID: &'static str = "tasks.log.read";
    const TITLE: &'static str = "Background Task Log";
    const DESCRIPTION: &'static str = "The log of a background task (the latest by default): what its programs printed, \
         as the panel at the bottom right shows it, with the task's state and message.";

    fn run(&self, session: &mut Session, params: LogParams) -> Result<TaskLog, CommandError> {
        let tasks = session.state_mut::<Tasks>();
        let (task, lines) =
            tasks.log(params.id, params.tail).ok_or_else(|| CommandError::new(t!("workbench-task-none")))?;
        Ok(TaskLog { task, lines })
    }
}

/// The parameters of `tasks.panel.show`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShowParams {
    /// The task to show; the latest when absent.
    #[serde(default)]
    pub id: Option<u64>,
}

pub struct ShowPanel;

impl Command for ShowPanel {
    type Params = ShowParams;
    type Output = ();
    const ID: &'static str = "tasks.panel.show";
    const TITLE: &'static str = "Show Background Tasks";
    const DESCRIPTION: &'static str =
        "Shows the panel with the background tasks' log at the bottom right of the window, on a task when given.";

    fn run(&self, session: &mut Session, params: ShowParams) -> Result<(), CommandError> {
        let tasks = session.state_mut::<Tasks>();
        if let Some(id) = params.id
            && tasks.log(Some(id), Some(0)).is_none()
        {
            return Err(CommandError::new(t!("workbench-task-none")));
        }
        tasks.set_panel(true, params.id);
        Ok(())
    }
}

pub struct HidePanel;

impl Command for HidePanel {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "tasks.panel.hide";
    const TITLE: &'static str = "Hide Background Tasks";
    const DESCRIPTION: &'static str = "Hides the panel with the background tasks' log; the tasks go on, and the status \
         bar shows them (clicking it shows the panel again).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        session.state_mut::<Tasks>().set_panel(false, None);
        Ok(())
    }
}

pub struct TogglePanel;

impl Command for TogglePanel {
    type Params = NoParams;
    type Output = bool;
    const ID: &'static str = "tasks.panel.toggle";
    const TITLE: &'static str = "Processes";
    const DESCRIPTION: &'static str = "Shows the panel with the background tasks when it is hidden and hides it when it \
         is shown, as Window → Processes does, also when no task exists. Answers whether it is shown now.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<bool, CommandError> {
        let tasks = session.state_mut::<Tasks>();
        tasks.toggle_panel();
        Ok(tasks.panel_shown())
    }
}

pub struct CancelTask;

impl Command for CancelTask {
    type Params = ShowParams;
    type Output = u64;
    const ID: &'static str = "tasks.background.cancel";
    const TITLE: &'static str = "Cancel Background Task";
    const DESCRIPTION: &'static str = "Stops a running background task that can be stopped (`cancellable`), the latest such \
         one when `id` is absent, as the × next to its progress does: its programs are interrupted, then killed after \
         2 s, a language server is asked to stop, and the task ends `cancelled`. Answers the task's id.";

    fn run(&self, session: &mut Session, params: ShowParams) -> Result<u64, CommandError> {
        session.state_mut::<Tasks>().cancel(params.id).ok_or_else(|| CommandError::new(t!("workbench-task-none")))
    }
}

pub struct PauseTask;

impl Command for PauseTask {
    type Params = ShowParams;
    type Output = u64;
    const ID: &'static str = "tasks.background.pause";
    const TITLE: &'static str = "Pause Background Task";
    const DESCRIPTION: &'static str = "Pauses a running background task that can be paused (`pausable`, e.g. indexing the \
         project), the latest such one when `id` is absent, as the ‖ next to its progress does. What it finished stays \
         usable. Answers the task's id.";

    fn run(&self, session: &mut Session, params: ShowParams) -> Result<u64, CommandError> {
        session.state_mut::<Tasks>().pause(params.id, true).ok_or_else(|| CommandError::new(t!("workbench-task-none")))
    }
}

pub struct ResumeTask;

impl Command for ResumeTask {
    type Params = ShowParams;
    type Output = u64;
    const ID: &'static str = "tasks.background.resume";
    const TITLE: &'static str = "Resume Background Task";
    const DESCRIPTION: &'static str = "Resumes a paused background task, the latest such one when `id` is absent, as the ▷ \
         next to its progress does. Answers the task's id.";

    fn run(&self, session: &mut Session, params: ShowParams) -> Result<u64, CommandError> {
        session.state_mut::<Tasks>().pause(params.id, false).ok_or_else(|| CommandError::new(t!("workbench-task-none")))
    }
}

pub struct ClearTasks;

impl Command for ClearTasks {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "tasks.finished.clear";
    const TITLE: &'static str = "Clear Finished Background Tasks";
    const DESCRIPTION: &'static str = "Forgets the background tasks that are over, with their logs.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        session.state_mut::<Tasks>().clear_finished();
        Ok(())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTasks)?;
    registry.register(ReadLog)?;
    registry.register(ShowPanel)?;
    registry.register(HidePanel)?;
    registry.register(ClearTasks)?;
    registry.register(CancelTask)?;
    registry.register(PauseTask)?;
    registry.register(ResumeTask)?;
    registry.register(TogglePanel)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_task_reports_from_another_thread_and_agents_read_its_log() {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&woken);
        session.state_mut::<Tasks>().set_waker(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }));
        let handle = session.state_mut::<Tasks>().start("Updating 2 dependencies", "inspections", true);
        let worker = handle.clone();
        std::thread::spawn(move || {
            worker.log("$ go get a@v1.1.0 b@v1.2.0\ngo: downloading a v1.1.0");
            worker.progress(Some(0.5), "");
        })
        .join()
        .unwrap();
        assert!(woken.load(Ordering::SeqCst) >= 2);
        let list = registry.dispatch(&mut session, "tasks.background.list", json!({})).unwrap();
        assert_eq!(list["panelShown"], true, "the panel opens at once");
        assert_eq!(list["tasks"][0]["state"], "running");
        assert_eq!(list["tasks"][0]["detail"], "go: downloading a v1.1.0");
        assert_eq!(list["tasks"][0]["fraction"], 0.5);
        assert_eq!(list["tasks"][0]["logLines"], 2);

        registry.dispatch(&mut session, "tasks.panel.hide", json!({})).unwrap();
        assert!(!session.state::<Tasks>().unwrap().panel_shown());
        handle.finish(Err("go: a@v1.1.0: unknown revision".to_owned()));
        let log = registry.dispatch(&mut session, "tasks.log.read", json!({ "tail": 1 })).unwrap();
        assert_eq!(log["task"]["state"], "failed");
        assert_eq!(log["task"]["message"], "go: a@v1.1.0: unknown revision");
        assert_eq!(log["lines"], json!(["go: a@v1.1.0: unknown revision"]), "the error ends the log");
        registry.dispatch(&mut session, "tasks.panel.show", json!({ "id": handle.id() })).unwrap();
        assert!(session.state::<Tasks>().unwrap().panel_shown());
        assert!(registry.dispatch(&mut session, "tasks.panel.show", json!({ "id": 99 })).is_err());

        let quiet = session.state_mut::<Tasks>().start("Fetching", "vcs", false);
        quiet.finish(Ok("Fetched".to_owned()));
        assert_eq!(session.state::<Tasks>().unwrap().selected(), Some(handle.id()), "a quiet task takes no focus");
        registry.dispatch(&mut session, "tasks.finished.clear", json!({})).unwrap();
        assert!(session.state::<Tasks>().unwrap().list().is_empty());
    }

    fn registry_and_session() -> (Registry, Session) {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        (registry, Session::new())
    }

    #[test]
    fn a_cancelled_task_ends_cancelled_whatever_its_work_reports() {
        let (registry, mut session) = registry_and_session();
        let task = session.state_mut::<Tasks>().start("Fetching", "vcs", false);
        let stopped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&stopped);
        task.on_cancel(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let quiet = session.state_mut::<Tasks>().start("Listing", "other", false);
        assert!(registry.dispatch(&mut session, "tasks.background.cancel", json!({ "id": quiet.id() })).is_err());

        let id = registry.dispatch(&mut session, "tasks.background.cancel", json!({})).unwrap();
        assert_eq!(id, json!(task.id()), "the latest cancellable one");
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
        let list = registry.dispatch(&mut session, "tasks.background.list", json!({})).unwrap();
        assert_eq!(list["tasks"][0]["state"], "running", "it runs until its work has stopped");
        assert_eq!(list["tasks"][0]["cancelling"], true);
        assert!(registry.dispatch(&mut session, "tasks.background.cancel", json!({})).is_err(), "only once");

        task.finish(Err("signal: interrupt".to_owned()));
        let log = registry.dispatch(&mut session, "tasks.log.read", json!({ "id": task.id() })).unwrap();
        assert_eq!(log["task"]["state"], "cancelled");
        assert_eq!(log["task"]["message"], "Cancelled");
        assert_eq!(log["lines"], json!([]), "a kill's own error is not the task's output");
        assert!(session.state::<Tasks>().unwrap().list()[0].cancellable.eq(&false));
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_kills_the_whole_process_group_of_the_task() {
        use std::process::{Command as Process, Stdio};

        use zen_project::inspect::Programs as _;
        let (registry, mut session) = registry_and_session();
        let task = session.state_mut::<Tasks>().start("Updating", "inspections", false);
        let programs = zen_run::Programs::default();
        task.cancel_kills(Arc::new(programs.clone()));
        let dir = tempfile::tempdir().unwrap();
        let grandchild = dir.path().join("grandchild");
        let script = format!("sleep 60 & echo $! > '{}'; wait", grandchild.display());
        let mut command = Process::new("sh");
        command.args(["-c", &script]).stdin(Stdio::null());
        let mut child = programs.spawn(&mut command).unwrap();
        while !std::fs::read_to_string(&grandchild).is_ok_and(|pid| pid.ends_with('\n')) {
            std::thread::yield_now();
        }
        registry.dispatch(&mut session, "tasks.background.cancel", json!({})).unwrap();
        child.wait().unwrap();
        while programs.running() {
            std::thread::yield_now();
        }
        task.finish(Err("killed".to_owned()));
        assert_eq!(session.state::<Tasks>().unwrap().list()[0].state, TaskState::Cancelled);
    }

    #[test]
    fn only_a_pausable_task_pauses_and_it_resumes() {
        let (registry, mut session) = registry_and_session();
        let task = session.state_mut::<Tasks>().start("Indexing…", crate::exit::INDEX, false);
        let plain = session.state_mut::<Tasks>().start("Fetching", "vcs", false);
        assert!(registry.dispatch(&mut session, "tasks.background.pause", json!({})).is_err());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let told = Arc::clone(&seen);
        task.on_pause(move |paused| told.lock().unwrap().push(paused));
        assert!(registry.dispatch(&mut session, "tasks.background.resume", json!({})).is_err(), "not paused");
        assert!(registry.dispatch(&mut session, "tasks.background.pause", json!({ "id": plain.id() })).is_err());
        registry.dispatch(&mut session, "tasks.background.pause", json!({})).unwrap();
        let list = registry.dispatch(&mut session, "tasks.background.list", json!({})).unwrap();
        assert_eq!((&list["tasks"][0]["pausable"], &list["tasks"][0]["paused"]), (&json!(true), &json!(true)));
        assert!(list["tasks"][0].get("cancellable").is_none(), "indexing is paused, not cancelled");
        assert!(registry.dispatch(&mut session, "tasks.background.pause", json!({})).is_err(), "paused already");
        registry.dispatch(&mut session, "tasks.background.resume", json!({ "id": task.id() })).unwrap();
        assert_eq!(*seen.lock().unwrap(), [true, false]);
        task.finish(Ok("Indexed".to_owned()));
        assert!(registry.dispatch(&mut session, "tasks.background.pause", json!({})).is_err(), "it is over");
    }

    #[test]
    fn a_dismissed_task_leaves_no_trace_unless_it_was_cancelled() {
        let (registry, mut session) = registry_and_session();
        let quiet = session.state_mut::<Tasks>().start("gopls: Loading", crate::exit::LANGUAGE_SERVER, false);
        quiet.dismiss();
        assert!(session.state::<Tasks>().unwrap().list().is_empty());
        let stopped =
            session.state_mut::<Tasks>().start("rust-analyzer: Indexing", crate::exit::LANGUAGE_SERVER, false);
        stopped.on_cancel(|| {});
        stopped.set_cancellable(false);
        assert!(registry.dispatch(&mut session, "tasks.background.cancel", json!({})).is_err(), "the server said no");
        stopped.set_cancellable(true);
        registry.dispatch(&mut session, "tasks.background.cancel", json!({})).unwrap();
        stopped.dismiss();
        let tasks = session.state::<Tasks>().unwrap().list();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].state, TaskState::Cancelled);
    }

    #[test]
    fn the_panel_toggles_with_no_task_at_all() {
        let (registry, mut session) = registry_and_session();
        assert_eq!(registry.dispatch(&mut session, "tasks.panel.toggle", json!({})).unwrap(), json!(true));
        assert_eq!(registry.dispatch(&mut session, "tasks.panel.toggle", json!({})).unwrap(), json!(false));
    }

    #[test]
    fn the_log_keeps_its_last_lines() {
        let mut tasks = Tasks::default();
        let handle = tasks.start("Long", "test", false);
        for index in 0..LOG_LINES + 10 {
            handle.log(&format!("line {index}"));
        }
        let (info, lines) = tasks.log(None, None).unwrap();
        assert_eq!(info.log_lines, LOG_LINES);
        assert_eq!(lines.first().map(String::as_str), Some("line 10"));
        assert!(handle.running());
        handle.finish(Ok(String::new()));
        assert!(!tasks.running());
    }
}
