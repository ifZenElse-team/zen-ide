use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::external_tool::{KIND, OutputFilter, ToolLaunch};
use zen_run::macros::{self, Macro};
use zen_run::{BeforeLaunchTask, Common, Configuration, FieldKind, FieldSpec, KindSpec, RunOutput, RunState, Stream};

use super::{
    DEBOUNCE, EnableWatcher, FileWatcher, Level, QUIET, SOURCE, ShowConsole, WatchScope, listed, project_root,
};
use crate::before_launch::{Started, TaskStart, TaskType};
use crate::disk::{SyncParams, SyncWithDisk};
use crate::language::{Problem, Severity};
use crate::notifications::{Action, Group, Kind, NewNotification};
use crate::run::{MAX_RUN_WAIT, Runs};
use crate::tasks::{TaskHandle, Tasks};
use crate::{Workbench, external_tools};

pub const AUTO_SAVE_DELAY: Duration = Duration::from_secs(1);

const FINISHED_KEPT: usize = 50;

pub const TASK: &str = "fileWatchers";

pub const TASK_FILES: &[&str] = &["changed", "open", "all"];

const TASK_FILE_LIMIT: usize = 20_000;

type Key = (Level, String);

#[derive(Clone, Copy, Debug)]
struct Pending {
    due: Instant,
    saved: bool,
    edited: bool,
}

struct Active {
    run_id: u64,
    file: PathBuf,
    cwd: PathBuf,
    outputs: Vec<PathBuf>,
    watcher: FileWatcher,
    task: TaskHandle,
}

#[derive(Default)]
pub(crate) struct Engine {
    seen: BTreeMap<PathBuf, (u64, u64)>,
    pending: BTreeMap<PathBuf, Pending>,
    queues: BTreeMap<Key, VecDeque<PathBuf>>,
    active: BTreeMap<Key, Active>,
    quiet: BTreeMap<PathBuf, Instant>,
    problems: BTreeMap<(Key, PathBuf), Vec<(PathBuf, Problem)>>,
    finished: VecDeque<FinishedRun>,
}

impl Engine {
    fn note(&mut self, path: &Path, due: Instant, saved: bool) {
        let entry = self.pending.entry(path.to_owned()).or_insert(Pending { due, saved, edited: !saved });
        entry.due = entry.due.max(due);
        entry.saved |= saved;
        entry.edited |= !saved;
    }

    fn enqueue(&mut self, key: Key, path: PathBuf) {
        let queue = self.queues.entry(key).or_default();
        if !queue.contains(&path) {
            queue.push_back(path);
        }
    }

    fn is_quiet(&self, path: &Path, now: Instant) -> bool {
        self.quiet.get(path).is_some_and(|until| *until > now)
    }

    #[cfg(test)]
    pub(crate) fn active_runs(&self) -> Vec<u64> {
        self.active.values().map(|active| active.run_id).collect()
    }

    #[cfg(test)]
    pub(crate) fn active_names(&self) -> Vec<String> {
        self.active.values().map(|active| active.watcher.name.clone()).collect()
    }

    #[cfg(test)]
    pub(crate) fn pending_files(&self) -> Vec<PathBuf> {
        self.pending.keys().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) fn queued_files(&self) -> Vec<PathBuf> {
        self.queues.values().flatten().cloned().collect()
    }
}

/// A watcher's run that has ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FinishedRun {
    pub id: String,
    pub level: Level,
    pub name: String,
    /// The file it ran for.
    pub file: PathBuf,
    /// The run's id, for `run.output.get`.
    pub run_id: u64,
    pub state: RunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// How many problems its output filters found.
    pub problems: usize,
    /// Why it failed and was switched off, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<String>,
}

fn key_of(level: Level, watcher: &FileWatcher) -> Key {
    (level, watcher.id.clone())
}

fn inside_project(root: Option<&Path>, path: &Path) -> bool {
    let Some(root) = root else { return false };
    let Ok(relative) = path.strip_prefix(root) else { return false };
    let mut folder = root.to_owned();
    for part in relative.components() {
        let name = part.as_os_str().to_string_lossy();
        if matches!(name.as_ref(), ".zen" | ".idea" | ".git") || zen_vfs::file_types::is_ignored(&name) {
            return false;
        }
        folder.push(part);
        if zen_vfs::excluded_folders::is_excluded(&folder) {
            return false;
        }
    }
    true
}

fn changed_files(session: &mut Session) -> BTreeSet<PathBuf> {
    crate::vcs::StatusList
        .run(session, NoParams {})
        .map(|report| report.changes.into_iter().map(|change| change.path).collect())
        .unwrap_or_default()
}

fn is_open(session: &Session, path: &Path) -> bool {
    session
        .state::<Workbench>()
        .is_some_and(|workbench| workbench.documents().iter().any(|document| document.path() == path))
}

pub(crate) fn in_scope(session: &mut Session, watcher: &FileWatcher, path: &Path) -> bool {
    let root = project_root(session);
    match watcher.scope {
        WatchScope::ProjectFiles => inside_project(root.as_deref(), path),
        WatchScope::OpenFiles => is_open(session, path),
        WatchScope::ChangedFiles => inside_project(root.as_deref(), path) && changed_files(session).contains(path),
    }
}

fn type_matches(watcher: &FileWatcher, path: &Path) -> bool {
    let wanted = watcher.file_type.trim();
    wanted.is_empty() || zen_vfs::file_types::type_at(path, None).is_some_and(|found| found.id == wanted)
}

fn has_syntax_errors(session: &Session, path: &Path) -> bool {
    crate::inspections::known_problems(session, path).iter().any(|problem| {
        let syntax = |text: &Option<String>| text.as_deref().is_some_and(|text| text.to_lowercase().contains("syntax"));
        problem.severity == Severity::Error && (syntax(&problem.source) || syntax(&problem.code))
    })
}

fn may_run(session: &Session, level: Level) -> bool {
    level == Level::Global || project_root(session).is_some_and(|root| crate::trust::trusted(session, &root))
}

fn matching(session: &mut Session, path: &Path, external: bool) -> Vec<(Level, FileWatcher)> {
    let mut found = Vec::new();
    for entry in listed(session) {
        if !may_run(session, entry.level) {
            continue;
        }
        let watcher = entry.watcher;
        if !watcher.enabled || (external && !watcher.trigger_on_external) || !type_matches(&watcher, path) {
            continue;
        }
        if !in_scope(session, &watcher, path) {
            continue;
        }
        if !watcher.trigger_on_syntax_errors && has_syntax_errors(session, path) {
            continue;
        }
        found.push((entry.level, watcher));
    }
    found
}

pub fn files_changed(session: &mut Session, paths: &[PathBuf], now: Instant) {
    if let Some(root) = project_root(session)
        && paths.iter().any(|path| *path == root.join(super::PROJECT_FILE))
    {
        super::reload_project(session);
    }
    let engine = session.state_mut::<Engine>();
    for path in paths {
        if engine.is_quiet(path, now) || !path.is_file() {
            continue;
        }
        let entry = engine.pending.entry(path.clone()).or_insert(Pending { due: now, saved: false, edited: false });
        entry.due = entry.due.max(now + DEBOUNCE);
    }
}

fn observe_documents(session: &mut Session, now: Instant) {
    let documents: Vec<(PathBuf, u64, u64, bool)> = session
        .state::<Workbench>()
        .map(|workbench| {
            workbench
                .documents()
                .iter()
                .filter(|document| document.image().is_none())
                .map(|document| {
                    (document.path().to_owned(), document.save_count(), document.version(), document.is_dirty())
                })
                .collect()
        })
        .unwrap_or_default();
    let engine = session.state_mut::<Engine>();
    engine.seen.retain(|path, _| documents.iter().any(|(open, ..)| open == path));
    for (path, saves, version, dirty) in documents {
        match engine.seen.insert(path.clone(), (saves, version)) {
            Some((before, _)) if before != saves => {
                engine.note(&path, now + DEBOUNCE, true);
                engine.quiet.insert(path, now + QUIET);
            }
            Some((_, before)) if before != version && dirty => engine.note(&path, now + AUTO_SAVE_DELAY, false),
            _ => {}
        }
    }
}

fn auto_save(session: &mut Session, path: &Path, now: Instant) -> bool {
    let wanted = matching(session, path, false).iter().any(|(_, watcher)| watcher.auto_save);
    if !wanted {
        return false;
    }
    let (saved, _) = crate::closing::save_documents(session, &[path.to_owned()]);
    if saved.is_empty() {
        return false;
    }
    let state = session.state::<Workbench>().and_then(|workbench| {
        workbench
            .documents()
            .iter()
            .find(|document| document.path() == path)
            .map(|document| (document.save_count(), document.version()))
    });
    let engine = session.state_mut::<Engine>();
    if let Some(state) = state {
        engine.seen.insert(path.to_owned(), state);
    }
    engine.quiet.insert(path.to_owned(), now + QUIET);
    true
}

fn dispatch_due(session: &mut Session, now: Instant) {
    let due: Vec<(PathBuf, Pending)> = {
        let engine = session.state_mut::<Engine>();
        let due: Vec<(PathBuf, Pending)> = engine
            .pending
            .iter()
            .filter(|(_, pending)| pending.due <= now)
            .map(|(path, pending)| (path.clone(), *pending))
            .collect();
        for (path, _) in &due {
            engine.pending.remove(path);
        }
        due
    };
    for (path, pending) in due {
        let saved = pending.saved || (pending.edited && auto_save(session, &path, now));
        if pending.edited && !saved {
            continue;
        }
        for (level, watcher) in matching(session, &path, !saved) {
            session.state_mut::<Engine>().enqueue(key_of(level, &watcher), path.clone());
        }
    }
}

fn expand_all(watcher: &FileWatcher, context: &macros::MacroContext) -> Result<Expanded, Macro> {
    let program = macros::expand(watcher.program.trim(), context)?;
    let args = macros::expand_arguments(&watcher.arguments, context)?;
    let folder = macros::expand(watcher.working_directory.trim(), context)?;
    let root = context.project.clone().unwrap_or_default();
    let cwd = if folder.is_empty() { root } else { root.join(folder) };
    let outputs_text = macros::expand(watcher.output_paths.trim(), context)?;
    let outputs = std::env::split_paths(&outputs_text)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| if path.is_absolute() { path } else { cwd.join(path) })
        .collect();
    let mut env = Vec::new();
    for variable in &watcher.environment {
        env.push((variable.name.trim().to_owned(), macros::expand(&variable.value, context)?));
    }
    Ok(Expanded { program, args, cwd, outputs, env })
}

struct Expanded {
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    outputs: Vec<PathBuf>,
    env: Vec<(String, String)>,
}

fn disable(session: &mut Session, level: Level, watcher: &FileWatcher, reason: &str) {
    let _ = super::set_enabled(session, level, &watcher.id, false);
    let body = t!("workbench-file-watchers-failed", name = watcher.name.as_str(), reason = reason);
    let notification = NewNotification::message(Group::General, Kind::Error, body)
        .titled_by(zen_i18n::Message::new("workbench-file-watchers-failed-title"))
        .action(Action::new(
            "enable",
            t!("workbench-file-watchers-enable-again"),
            EnableWatcher::ID,
            json!({ "id": watcher.id, "level": level }),
        ));
    crate::notifications::post(session, notification);
}

fn start_run(session: &mut Session, level: Level, watcher: FileWatcher, file: PathBuf) -> Option<u64> {
    let root = project_root(session)?;
    let context = external_tools::macro_context(session, Some(&file));
    let expanded = match expand_all(&watcher, &context) {
        Ok(expanded) => expanded,
        Err(item) => {
            let reason = t!(
                "workbench-file-watchers-no-value",
                token = item.token(),
                reason = external_tools::missing_reason(item)
            );
            disable(session, level, &watcher, &reason);
            return None;
        }
    };
    let launch = ToolLaunch {
        tool_id: format!("fileWatcher:{}:{}", level.id(), watcher.id),
        program: PathBuf::from(&expanded.program),
        args: expanded.args,
        cwd: expanded.cwd.clone(),
        filters: watcher.output_filters.iter().filter(|filter| !filter.trim().is_empty()).cloned().collect(),
        synchronize: false,
        open_console: watcher.show_console == ShowConsole::Always,
        activate_on_stdout: false,
        activate_on_stderr: false,
        env: expanded.env,
        console_on_error: watcher.show_console == ShowConsole::OnError,
    };
    let configuration = Configuration {
        id: launch.tool_id.clone(),
        name: watcher.name.clone(),
        kind: KIND.to_owned(),
        temporary: false,
        common: Common::default(),
        options: serde_json::to_value(&launch).unwrap_or_default(),
        ..Configuration::default()
    };
    let run_context = crate::run::run_context(session, &root);
    match session.state_mut::<Runs>().start(configuration, &run_context) {
        Ok(run_id) => {
            let title = t!("workbench-file-watchers-task", name = watcher.name.as_str(), file = crate::display(&file));
            let task = session.state_mut::<Tasks>().start(&title, SOURCE, false);
            task.log(&launch.command_line().display);
            if let Some(run) = session.state::<Runs>().and_then(|runs| runs.run(run_id)).cloned() {
                task.on_cancel(move || {
                    run.stop();
                });
            }
            let active = Active { run_id, file, cwd: expanded.cwd, outputs: expanded.outputs, watcher, task };
            session.state_mut::<Engine>().active.insert((level, active.watcher.id.clone()), active);
            Some(run_id)
        }
        Err(error) => {
            disable(session, level, &watcher, &error.message);
            None
        }
    }
}

fn start_queued(session: &mut Session) -> Vec<u64> {
    let ready: Vec<(Key, PathBuf)> = {
        let engine = session.state_mut::<Engine>();
        let idle: Vec<Key> = engine
            .queues
            .iter()
            .filter(|(key, queue)| !queue.is_empty() && !engine.active.contains_key(*key))
            .map(|(key, _)| key.clone())
            .collect();
        let mut ready = Vec::new();
        for key in idle {
            if let Some(path) = engine.queues.get_mut(&key).and_then(VecDeque::pop_front) {
                ready.push((key, path));
            }
        }
        engine.queues.retain(|_, queue| !queue.is_empty());
        ready
    };
    let mut started = Vec::new();
    for ((level, id), path) in ready {
        if !may_run(session, level) {
            continue;
        }
        let Some(watcher) = super::find(session, level, &id).filter(|watcher| watcher.enabled) else { continue };
        started.extend(start_run(session, level, watcher, path));
    }
    started
}

fn text_of(session: &Session, path: &Path) -> Option<String> {
    let open = session.state::<Workbench>().and_then(|workbench| {
        workbench.documents().iter().find(|document| document.path() == path).map(|document| document.text().to_owned())
    });
    open.or_else(|| zen_vfs::read_text(path).ok())
}

fn problem_at(text: Option<&str>, line: u32, column: Option<u32>, message: String, source: &str) -> Problem {
    let column = column.unwrap_or(1).max(1);
    let (start, end) = text
        .map(|text| {
            let mut offset = 0;
            for (index, row) in text.split('\n').enumerate() {
                let length = row.chars().count();
                if index + 1 == line as usize {
                    let at = offset + (column as usize - 1).min(length);
                    return (at, (offset + length).max(at + 1));
                }
                offset += length + 1;
            }
            (offset, offset)
        })
        .unwrap_or((0, 0));
    Problem {
        start,
        end,
        line,
        column,
        severity: Severity::Warning,
        message,
        source: Some(source.to_owned()),
        code: None,
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        deprecated: false,
        unused: false,
    }
}

fn found_problems(session: &Session, active: &Active, lines: &[(Stream, String)]) -> Vec<(PathBuf, Problem)> {
    let filters: Vec<OutputFilter> =
        active.watcher.output_filters.iter().filter_map(|filter| OutputFilter::compile_relative(filter).ok()).collect();
    let mut found = Vec::new();
    let mut texts: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    for (_, line) in lines.iter().filter(|(stream, _)| *stream != Stream::System) {
        for filter in &filters {
            for hit in filter.matches(line) {
                let path = if hit.file.is_absolute() { hit.file.clone() } else { active.cwd.join(&hit.file) };
                let text = texts.entry(path.clone()).or_insert_with(|| text_of(session, &path));
                let message = hit.message.clone().unwrap_or_else(|| line.trim().to_owned());
                found.push((path, problem_at(text.as_deref(), hit.line, hit.column, message, &active.watcher.name)));
            }
        }
    }
    found
}

fn finish_ended(session: &mut Session, now: Instant) -> bool {
    let ended: Vec<Key> = {
        let runs = session.state::<Runs>();
        let Some(engine) = session.state::<Engine>() else { return false };
        engine
            .active
            .iter()
            .filter(|(_, active)| {
                runs.and_then(|runs| runs.run(active.run_id)).is_none_or(|run| !run.output.read(RunOutput::is_running))
            })
            .map(|(key, _)| key.clone())
            .collect()
    };
    for key in &ended {
        let Some(active) = session.state_mut::<Engine>().active.remove(key) else { continue };
        finish(session, key.0, active, now);
    }
    !ended.is_empty()
}

fn finish(session: &mut Session, level: Level, active: Active, now: Instant) {
    let (state, exit_code, error, lines) = session
        .state::<Runs>()
        .and_then(|runs| runs.run(active.run_id))
        .map(|run| {
            run.output.read(|output| {
                let lines: Vec<(Stream, String)> =
                    output.console().lines().iter().map(|line| (line.stream, line.text.clone())).collect();
                (output.state(), output.exit_code(), output.error().map(str::to_owned), lines)
            })
        })
        .unwrap_or((RunState::Stopped, None, None, Vec::new()));
    let problems = found_problems(session, &active, &lines);
    let ok = error.is_none() && exit_code == Some(0) && state == RunState::Finished;
    if ok
        && active.watcher.create_output_from_stdout
        && let Some(target) = active.outputs.first()
    {
        let text: String =
            lines.iter().filter(|(stream, _)| *stream == Stream::Stdout).map(|(_, line)| format!("{line}\n")).collect();
        if let Some(folder) = target.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = zen_vfs::write_atomic(target, text.as_bytes());
    }
    let mut refreshed = vec![active.file.clone()];
    refreshed.extend(active.outputs.iter().cloned());
    let _ = SyncWithDisk.run(session, SyncParams { paths: refreshed.clone() });
    let key = (level, active.watcher.id.clone());
    let count = problems.len();
    let findings = count > 0 && error.is_none();
    let reason = match (&error, exit_code) {
        (Some(error), _) => error.clone(),
        (None, Some(code)) => t!("workbench-file-watchers-exit-code", code = code),
        (None, None) => t!("workbench-file-watchers-interrupted"),
    };
    let disabled = (!ok && state != RunState::Stopped && !findings).then(|| reason.clone());
    {
        let engine = session.state_mut::<Engine>();
        for path in refreshed {
            engine.quiet.insert(path, now + QUIET);
        }
        if problems.is_empty() {
            engine.problems.remove(&(key.clone(), active.file.clone()));
        } else {
            engine.problems.insert((key.clone(), active.file.clone()), problems);
        }
        engine.finished.push_back(FinishedRun {
            id: active.watcher.id.clone(),
            level,
            name: active.watcher.name.clone(),
            file: active.file.clone(),
            run_id: active.run_id,
            state,
            exit_code,
            problems: count,
            disabled: disabled.clone(),
        });
        while engine.finished.len() > FINISHED_KEPT {
            engine.finished.pop_front();
        }
    }
    if ok || findings {
        active.task.finish(Ok(t!("workbench-file-watchers-task-done", name = active.watcher.name.as_str())));
    } else {
        active.task.finish(Err(reason));
    }
    if let Some(reason) = disabled {
        session.state_mut::<Engine>().queues.remove(&key);
        disable(session, level, &active.watcher, &reason);
    }
}

pub fn step(session: &mut Session, now: Instant) -> Option<Duration> {
    observe_documents(session, now);
    dispatch_due(session, now);
    finish_ended(session, now);
    start_queued(session);
    let engine = session.state_mut::<Engine>();
    engine.quiet.retain(|_, until| *until > now);
    engine.pending.values().map(|pending| pending.due.saturating_duration_since(now)).min()
}

pub fn problems(session: &Session, path: &Path) -> Vec<Problem> {
    let Some(engine) = session.state::<Engine>() else { return Vec::new() };
    engine.problems.values().flatten().filter(|(file, _)| file == path).map(|(_, problem)| problem.clone()).collect()
}

pub fn problem_files(session: &Session) -> Vec<PathBuf> {
    let Some(engine) = session.state::<Engine>() else { return Vec::new() };
    let files: BTreeSet<PathBuf> = engine.problems.values().flatten().map(|(file, _)| file.clone()).collect();
    files.into_iter().collect()
}

/// A watcher's run in progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRun {
    pub id: String,
    pub level: Level,
    pub name: String,
    pub file: PathBuf,
    pub run_id: u64,
}

/// What `run.fileWatchers.status` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusAnswer {
    /// Files changed or saved whose pause has not passed yet.
    pub pending: Vec<PathBuf>,
    /// The runs going on, one per watcher at most.
    pub running: Vec<ActiveRun>,
    /// The files waiting for a watcher's run to end, by watcher.
    pub queued: Vec<QueuedFiles>,
    /// The latest ended runs, oldest first.
    pub finished: Vec<FinishedRun>,
}

/// The files waiting for one watcher.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueuedFiles {
    pub id: String,
    pub level: Level,
    pub files: Vec<PathBuf>,
}

fn status(session: &mut Session) -> StatusAnswer {
    let engine = session.state_mut::<Engine>();
    StatusAnswer {
        pending: engine.pending.keys().cloned().collect(),
        running: engine
            .active
            .iter()
            .map(|((level, id), active)| ActiveRun {
                id: id.clone(),
                level: *level,
                name: active.watcher.name.clone(),
                file: active.file.clone(),
                run_id: active.run_id,
            })
            .collect(),
        queued: engine
            .queues
            .iter()
            .map(|((level, id), files)| QueuedFiles {
                id: id.clone(),
                level: *level,
                files: files.iter().cloned().collect(),
            })
            .collect(),
        finished: engine.finished.iter().cloned().collect(),
    }
}

pub struct WatcherStatus;

impl Command for WatcherStatus {
    type Params = NoParams;
    type Output = StatusAnswer;
    const ID: &'static str = "run.fileWatchers.status";
    const TITLE: &'static str = "File Watcher Runs";
    const DESCRIPTION: &'static str = "What the file watchers are doing: the files saved or changed whose pause (300 ms after a save or a change on disk, 1 s after an edit with auto-save) has not passed, the run of each watcher going on (runs are one at a time per watcher), the files waiting for it, and the latest runs that ended with their exit code, the problems their output filters found and, for one that failed, why it was switched off.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<StatusAnswer, CommandError> {
        step(session, Instant::now());
        Ok(status(session))
    }
}

/// The parameters of `run.fileWatchers.run`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TriggerParams {
    /// The files, absolute or from the project's folder; the active file when empty.
    pub paths: Vec<PathBuf>,
    /// Only this watcher; every enabled watcher that matches a file when absent.
    pub id: Option<String>,
    pub level: Option<Level>,
    /// Waits up to this long (at most 10 minutes) for the runs and those queued behind them to end.
    pub wait_ms: Option<u64>,
}

/// What `run.fileWatchers.run` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerAnswer {
    /// How many runs it asked for: a file and a watcher each.
    pub queued: usize,
    /// With `waitMs`: the runs that ended, in order.
    pub finished: Vec<FinishedRun>,
    /// With `waitMs`: whether every run asked for has ended.
    pub done: bool,
}

pub struct RunWatchers;

impl Command for RunWatchers {
    type Params = TriggerParams;
    type Output = TriggerAnswer;
    const ID: &'static str = "run.fileWatchers.run";
    const TITLE: &'static str = "Run File Watchers";
    const DESCRIPTION: &'static str = "Runs the file watchers on files now, without the pause, as a save would but whatever their trigger options say: every enabled watcher whose file type and scope match each file (a watcher never runs on a file its scope leaves out, such as one outside the project), or only the one named by id and level. Runs of one watcher go one after another. Returns how many runs it queued; with waitMs it waits for them and answers how each ended. Refused with code `notTrusted` when only a project's own watchers would run and the project is not trusted, and for an agent with code `redacted` on a file hidden from agents.";

    fn run(&self, session: &mut Session, params: TriggerParams) -> Result<TriggerAnswer, CommandError> {
        let now = Instant::now();
        let mut paths: Vec<PathBuf> = match session.state::<Workbench>() {
            Some(workbench) => params.paths.iter().map(|path| workbench.resolve(path)).collect(),
            None => params.paths.clone(),
        };
        if paths.is_empty() {
            let active = session
                .state::<Workbench>()
                .and_then(Workbench::active_document)
                .map(|document| document.path().to_owned());
            paths.extend(active);
        }
        if paths.is_empty() {
            return Err(CommandError::new(t!("workbench-error-no-document")));
        }
        if let Some(root) = project_root(session) {
            crate::hidden_files::guard_change(session, &root, &paths)?;
        }
        if let Some(id) = &params.id
            && super::find(session, params.level.unwrap_or_default(), id).is_none()
        {
            return Err(CommandError::new(t!("workbench-file-watchers-unknown", id = id.as_str())));
        }
        let mut asked = Vec::new();
        let mut untrusted = false;
        for path in &paths {
            for entry in listed(session) {
                let watcher = entry.watcher;
                let named = params
                    .id
                    .as_ref()
                    .is_none_or(|id| *id == watcher.id && params.level.unwrap_or_default() == entry.level);
                if !named || !watcher.enabled || !type_matches(&watcher, path) || !in_scope(session, &watcher, path) {
                    continue;
                }
                if !may_run(session, entry.level) {
                    untrusted = true;
                    continue;
                }
                session.state_mut::<Engine>().enqueue(key_of(entry.level, &watcher), path.clone());
                asked.push((entry.level, watcher.id.clone(), path.clone()));
            }
        }
        if asked.is_empty()
            && untrusted
            && let Some(root) = project_root(session)
        {
            crate::trust::guard_file_watchers(session, &root)?;
        }
        let mark =
            session.state::<Engine>().and_then(|engine| engine.finished.back().map(|run| run.run_id)).unwrap_or(0);
        start_queued(session);
        let Some(wait) = params.wait_ms else {
            return Ok(TriggerAnswer { queued: asked.len(), finished: Vec::new(), done: false });
        };
        let deadline = now + Duration::from_millis(wait).min(MAX_RUN_WAIT);
        loop {
            let outputs: Vec<zen_run::SharedOutput> = {
                let runs = session.state::<Runs>();
                session
                    .state::<Engine>()
                    .map(|engine| {
                        engine
                            .active
                            .values()
                            .filter_map(|active| {
                                runs.and_then(|runs| runs.run(active.run_id)).map(|run| run.output.clone())
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            for output in &outputs {
                output.wait_end(deadline.saturating_duration_since(Instant::now()));
            }
            let now = Instant::now();
            finish_ended(session, now);
            start_queued(session);
            let waiting = session.state::<Engine>().is_some_and(|engine| {
                asked.iter().any(|(level, id, path)| {
                    let key = (*level, id.clone());
                    engine.active.get(&key).is_some_and(|active| active.file == *path)
                        || engine.queues.get(&key).is_some_and(|queue| queue.contains(path))
                })
            });
            if !waiting || now >= deadline {
                let finished = session
                    .state::<Engine>()
                    .map(|engine| engine.finished.iter().filter(|run| run.run_id > mark).cloned().collect())
                    .unwrap_or_default();
                return Ok(TriggerAnswer { queued: asked.len(), finished, done: !waiting });
            }
        }
    }
}

fn chosen_watcher(session: &mut Session, value: &str) -> Option<Result<(Level, FileWatcher), String>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (level, id) = match value.split_once(':') {
        Some(("project", id)) => (Some(Level::Project), id),
        Some(("global", id)) => (Some(Level::Global), id),
        _ => (None, value),
    };
    let found = listed(session)
        .into_iter()
        .find(|entry| entry.watcher.id == id && level.is_none_or(|level| level == entry.level))
        .map(|entry| (entry.level, entry.watcher));
    Some(found.ok_or_else(|| t!("workbench-file-watchers-unknown", id = value)))
}

fn walk(folder: &Path, root: &Path, index: Option<&zen_index::FileIndex>, found: &mut Vec<PathBuf>) {
    let mut stack = vec![folder.to_owned()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            if found.len() >= TASK_FILE_LIMIT {
                return;
            }
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let skipped = zen_vfs::HIDDEN.contains(&name.as_ref()) || zen_vfs::TOOL_FOLDERS.contains(&name.as_ref());
            if !inside_project(Some(root), &path) || skipped {
                continue;
            }
            if index.is_some_and(|index| index.exclusions().excludes_entry(&path) || index.git_ignores(&path)) {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(path),
                Ok(kind) if kind.is_file() => found.push(path),
                _ => {}
            }
        }
    }
    found.sort();
}

fn task_files(session: &mut Session, files: &str, root: &Path, folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = match files {
        "open" => session
            .state::<Workbench>()
            .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
            .unwrap_or_default(),
        "all" => {
            let index = session.state::<Workbench>().and_then(|workbench| workbench.index.clone());
            let mut walked = Vec::new();
            walk(folder, root, index.as_ref(), &mut walked);
            walked
        }
        _ => changed_files(session).into_iter().collect(),
    };
    found.retain(|path| path.starts_with(folder) && path.is_file());
    if let Some(patterns) = crate::hidden_files::for_caller(session) {
        found.retain(|path| !crate::hidden_files::is_hidden(&patterns, Some(root), path));
    }
    found.truncate(TASK_FILE_LIMIT);
    found
}

fn run_failed(run: &FinishedRun) -> Option<String> {
    if run.state == RunState::Finished && run.exit_code == Some(0) {
        return None;
    }
    let reason = match (&run.disabled, run.exit_code) {
        (Some(reason), _) => reason.clone(),
        (None, Some(code)) => t!("workbench-file-watchers-exit-code", code = code),
        (None, None) => t!("workbench-file-watchers-interrupted"),
    };
    Some(t!(
        "workbench-file-watchers-before-launch-failed",
        name = run.name.as_str(),
        file = crate::display(&run.file),
        reason = reason
    ))
}

struct RunBeforeLaunch;

impl TaskType for RunBeforeLaunch {
    fn spec(&self) -> KindSpec {
        let field = |key: &str, kind: FieldKind| FieldSpec {
            key: key.to_owned(),
            label_message: format!("workbench-file-watchers-before-launch-{key}"),
            kind,
        };
        KindSpec {
            id: TASK.to_owned(),
            name_message: "workbench-file-watchers-before-launch".to_owned(),
            fields: vec![
                field("watcher", FieldKind::FileWatcher),
                field("files", FieldKind::Choice(TASK_FILES.iter().map(|files| (*files).to_owned()).collect())),
            ],
            defaults: json!({ "watcher": "", "files": "changed" }),
        }
    }

    fn start(&self, session: &mut Session, task: &BeforeLaunchTask, launch: &TaskStart<'_>) -> Result<Started, String> {
        let only = chosen_watcher(session, task.text("watcher")).transpose()?;
        let files = task.text("files").trim();
        if !files.is_empty() && !TASK_FILES.contains(&files) {
            return Err(t!(
                "workbench-file-watchers-before-launch-bad-files",
                files = files,
                known = TASK_FILES.join(", ")
            ));
        }
        let folder = launch.configuration.common.working_directory_in(launch.root);
        let paths = task_files(session, files, launch.root, &folder);
        let mut asked: Vec<(Level, String, PathBuf)> = Vec::new();
        let mut untrusted = false;
        for path in &paths {
            for entry in listed(session) {
                let watcher = entry.watcher;
                let named = only.as_ref().is_none_or(|(level, kept)| *level == entry.level && kept.id == watcher.id);
                if !named || !watcher.enabled || !type_matches(&watcher, path) || !in_scope(session, &watcher, path) {
                    continue;
                }
                if !may_run(session, entry.level) {
                    untrusted = true;
                    continue;
                }
                session.state_mut::<Engine>().enqueue(key_of(entry.level, &watcher), path.clone());
                asked.push((entry.level, watcher.id.clone(), path.clone()));
            }
        }
        if asked.is_empty() {
            if untrusted {
                crate::trust::guard_file_watchers(session, launch.root).map_err(|error| error.message)?;
            }
            return Ok(Started::Skipped(t!("workbench-file-watchers-before-launch-nothing")));
        }
        let mut seen =
            session.state::<Engine>().and_then(|engine| engine.finished.back().map(|run| run.run_id)).unwrap_or(0);
        let mut failure: Option<String> = None;
        start_queued(session);
        Ok(Started::Wait(Box::new(move |session: &mut Session| {
            let now = Instant::now();
            finish_ended(session, now);
            start_queued(session);
            let engine = session.state::<Engine>()?;
            let since = seen;
            for run in engine.finished.iter().filter(|run| run.run_id > since) {
                let ours =
                    asked.iter().any(|(level, id, file)| *level == run.level && *id == run.id && *file == run.file);
                if ours && failure.is_none() {
                    failure = run_failed(run);
                }
                seen = seen.max(run.run_id);
            }
            let waiting = asked.iter().any(|(level, id, path)| {
                let key = (*level, id.clone());
                engine.active.get(&key).is_some_and(|active| active.file == *path)
                    || engine.queues.get(&key).is_some_and(|queue| queue.contains(path))
            });
            if waiting {
                return None;
            }
            if failure.is_none() {
                let mut watchers: Vec<(Level, String)> =
                    asked.iter().map(|(level, id, _)| (*level, id.clone())).collect();
                watchers.dedup();
                for (level, id) in watchers {
                    if let Some(watcher) = super::find(session, level, &id).filter(|watcher| !watcher.enabled) {
                        failure = Some(t!("workbench-file-watchers-before-launch-off", name = watcher.name.as_str()));
                        break;
                    }
                }
            }
            Some(failure.take().map_or(Ok(()), Err))
        })))
    }

    fn summary(&self, session: &mut Session, task: &BeforeLaunchTask) -> String {
        match chosen_watcher(session, task.text("watcher")) {
            Some(Ok((_, watcher))) => watcher.name,
            Some(Err(_)) => task.text("watcher").trim().to_owned(),
            None => {
                let files = task.text("files").trim();
                let files = if files.is_empty() { TASK_FILES[0] } else { files };
                t!(&format!("workbench-file-watchers-before-launch-files-{files}"))
            }
        }
    }
}

pub fn task_type() -> Arc<dyn TaskType> {
    Arc::new(RunBeforeLaunch)
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(WatcherStatus)?;
    registry.register(RunWatchers)?;
    Ok(())
}
