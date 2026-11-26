mod engine;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::external_tool::OutputFilter;

use crate::Workbench;

pub use engine::{
    AUTO_SAVE_DELAY, ActiveRun, FinishedRun, QueuedFiles, RunWatchers, StatusAnswer, TASK, TASK_FILES, TriggerAnswer,
    TriggerParams, WatcherStatus, files_changed, problem_files, problems, step, task_type,
};

pub const FILE_NAME: &str = "fileWatchers.json";

pub const PROJECT_FILE: &str = ".zen/fileWatchers.json";

pub const FORMAT_VERSION: u32 = 1;

pub const DEBOUNCE: Duration = Duration::from_millis(300);

pub const QUIET: Duration = Duration::from_secs(2);

pub const SOURCE: &str = "fileWatchers";

/// Where a watcher is kept: the project's `.zen/fileWatchers.json` or the IDE's data folder, for every project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    #[default]
    Project,
    Global,
}

impl Level {
    pub fn id(self) -> &'static str {
        match self {
            Level::Project => "project",
            Level::Global => "global",
        }
    }
}

/// Which files a watcher runs on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WatchScope {
    /// The project's files, without excluded folders and ignored files.
    #[default]
    ProjectFiles,
    /// The files open in editor tabs, in the project or not.
    OpenFiles,
    /// The project's files that Git sees as changed, added or not yet added.
    ChangedFiles,
}

/// When the watcher's output gets a tab in the Run tool window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ShowConsole {
    /// For every run.
    Always,
    /// When a run fails.
    #[default]
    OnError,
    Never,
}

/// An environment variable a watcher's program gets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EnvVar {
    pub name: String,
    /// The value, with macros.
    pub value: String,
}

/// A program run automatically when matching files are saved or change on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileWatcher {
    /// The stable id within its level, kept when the watcher is renamed.
    pub id: String,
    pub name: String,
    /// Whether it runs; a watcher that fails is switched off.
    pub enabled: bool,
    /// The file type it watches, as `file.types.list` names it (`go`, `typescript`); empty watches every type.
    pub file_type: String,
    pub scope: WatchScope,
    /// The program: a path or a name found on `PATH`, with macros.
    pub program: String,
    /// The arguments as typed, split as a shell splits them, with macros.
    pub arguments: String,
    /// The files it writes, with macros, separated by the system's path separator (`:` on macOS and Linux), relative to the working directory; they are read again after a run.
    pub output_paths: String,
    /// The folder it runs in, with macros; the project's folder when empty, relative to it otherwise.
    pub working_directory: String,
    /// The environment variables added to the IDE's.
    pub environment: Vec<EnvVar>,
    /// Save edited files after a pause so that the watcher runs; when off, it runs only on an explicit save.
    pub auto_save: bool,
    /// Run when a matching file is changed by another program.
    pub trigger_on_external: bool,
    /// Run even when the file has syntax errors.
    pub trigger_on_syntax_errors: bool,
    /// Write the program's standard output into the first output path.
    pub create_output_from_stdout: bool,
    pub show_console: ShowConsole,
    /// Regular expressions with `$FILE_PATH$`, `$LINE$`, `$COLUMN$` and `$MESSAGE$`; their matches in the output become problems and file links.
    pub output_filters: Vec<String>,
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            file_type: String::new(),
            scope: WatchScope::ProjectFiles,
            program: String::new(),
            arguments: String::new(),
            output_paths: String::new(),
            working_directory: String::new(),
            environment: Vec::new(),
            auto_save: true,
            trigger_on_external: true,
            trigger_on_syntax_errors: false,
            create_output_from_stdout: false,
            show_console: ShowConsole::OnError,
            output_filters: Vec::new(),
        }
    }
}

impl FileWatcher {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err(t!("workbench-file-watchers-no-name"));
        }
        if self.program.trim().is_empty() {
            return Err(t!("workbench-file-watchers-no-program", name = self.name.as_str()));
        }
        for filter in self.output_filters.iter().filter(|filter| !filter.trim().is_empty()) {
            OutputFilter::compile_relative(filter).map_err(|error| {
                t!("workbench-file-watchers-bad-filter", filter = filter.as_str(), error = error.as_str())
            })?;
        }
        if let Some(variable) = self.environment.iter().find(|variable| variable.name.trim().is_empty()) {
            return Err(t!("workbench-file-watchers-no-variable-name", value = variable.value.as_str()));
        }
        Ok(())
    }
}

/// A watcher with the level it is kept at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListedWatcher {
    pub level: Level,
    #[serde(flatten)]
    pub watcher: FileWatcher,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    format_version: u32,
    watchers: Vec<FileWatcher>,
}

#[derive(Debug, Default)]
pub(crate) struct Store {
    file: Option<PathBuf>,
    watchers: Vec<FileWatcher>,
    notice: Option<String>,
}

fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() { "watcher".to_owned() } else { slug }
}

fn parse_list(bytes: &[u8]) -> Result<Vec<FileWatcher>, String> {
    let saved = serde_json::from_slice::<Saved>(bytes).map_err(|error| error.to_string())?;
    let mut watchers: Vec<FileWatcher> = Vec::new();
    for watcher in saved.watchers {
        if watcher.id.is_empty() || watchers.iter().any(|known| known.id == watcher.id) {
            continue;
        }
        watchers.push(watcher);
    }
    Ok(watchers)
}

impl Store {
    pub(crate) fn load(file: Option<PathBuf>) -> Self {
        let Some(path) = file.clone() else { return Self::default() };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Self { file, ..Self::default() },
            Err(error) => {
                let notice = t!("workbench-file-watchers-unreadable", detail = error.to_string());
                return Self { file, notice: Some(notice), ..Self::default() };
            }
        };
        match parse_list(&bytes) {
            Ok(watchers) => Self { file, watchers, notice: None },
            Err(error) => {
                let kept = path.with_extension("json.corrupt");
                let kept = if std::fs::rename(&path, &kept).is_ok() { kept } else { path };
                let notice = t!("workbench-file-watchers-corrupt", detail = error, file = kept.display().to_string());
                Self { file, watchers: Vec::new(), notice: Some(notice) }
            }
        }
    }

    fn free_id(&self, name: &str) -> String {
        let base = slug(name);
        let mut id = base.clone();
        let mut suffix = 1;
        while self.watchers.iter().any(|watcher| watcher.id == id) {
            suffix += 1;
            id = format!("{base}-{suffix}");
        }
        id
    }

    fn index(&self, id: &str) -> Result<usize, CommandError> {
        self.watchers
            .iter()
            .position(|watcher| watcher.id == id)
            .ok_or_else(|| CommandError::new(t!("workbench-file-watchers-unknown", id = id)))
    }

    fn save(&self) -> Result<(), CommandError> {
        let Some(file) = &self.file else { return Ok(()) };
        if self.watchers.is_empty() && !file.exists() {
            return Ok(());
        }
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let saved = Saved { format_version: FORMAT_VERSION, watchers: self.watchers.clone() };
        let json = serde_json::to_vec_pretty(&saved).unwrap_or_default();
        zen_vfs::write_atomic(file, &json)
            .map_err(|error| CommandError::new(t!("workbench-file-watchers-save-failed", detail = error.to_string())))
    }

    fn put(&mut self, mut watcher: FileWatcher) -> String {
        match self.watchers.iter().position(|known| !watcher.id.is_empty() && known.id == watcher.id) {
            Some(index) => self.watchers[index] = watcher.clone(),
            None => {
                if watcher.id.is_empty() {
                    watcher.id = self.free_id(&watcher.name);
                }
                self.watchers.push(watcher.clone());
            }
        }
        watcher.id
    }

    fn replace(&mut self, watchers: Vec<FileWatcher>) {
        let mut seen = BTreeSet::new();
        self.watchers = watchers
            .into_iter()
            .map(|mut watcher| {
                if !seen.insert(watcher.id.clone()) {
                    watcher.id.clear();
                }
                watcher
            })
            .collect();
        for index in 0..self.watchers.len() {
            if self.watchers[index].id.is_empty() {
                let name = self.watchers[index].name.clone();
                self.watchers[index].id = self.free_id(&name);
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct GlobalWatchers {
    inner: Arc<Mutex<Store>>,
}

impl GlobalWatchers {
    pub fn load(file: Option<PathBuf>) -> Self {
        Self { inner: Arc::new(Mutex::new(Store::load(file))) }
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn take_notice(&self) -> Option<String> {
        self.lock().notice.take()
    }

    pub fn watchers(&self) -> Vec<FileWatcher> {
        self.lock().watchers.clone()
    }
}

#[derive(Default)]
pub(crate) struct ProjectWatchers {
    root: Option<Option<PathBuf>>,
    store: Store,
}

/// A template offered by + in Settings → Tools → File Watchers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    /// Who offers it: `platform` or a module's id, such as `go`.
    pub origin: String,
    /// The watcher it creates, without an id.
    pub watcher: FileWatcher,
}

#[derive(Clone, Debug, Default)]
pub struct Templates {
    declared: Vec<Template>,
}

impl Templates {
    pub fn declare(&mut self, origin: &str, json: &str) {
        let Ok(watchers) = serde_json::from_str::<Vec<FileWatcher>>(json) else { return };
        for watcher in watchers {
            if self.declared.iter().any(|known| known.watcher.name == watcher.name) {
                continue;
            }
            self.declared
                .push(Template { origin: origin.to_owned(), watcher: FileWatcher { id: String::new(), ..watcher } });
        }
    }
}

pub const WEB_TEMPLATES: &str = include_str!("file_watchers/web-templates.json");

pub fn templates(session: &Session) -> Vec<Template> {
    let mut all = session.state::<Templates>().cloned().unwrap_or_default();
    all.declare("platform", WEB_TEMPLATES);
    all.declared.sort_by_key(|template| template.watcher.name.to_lowercase());
    all.declared
}

fn project_root(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

pub(crate) fn project_store(session: &mut Session) -> &mut Store {
    let root = project_root(session);
    let state = session.state_mut::<ProjectWatchers>();
    if state.root.as_ref() != Some(&root) {
        state.store = Store::load(root.as_ref().map(|root| root.join(PROJECT_FILE)));
        state.root = Some(root);
    }
    &mut state.store
}

pub(crate) fn reload_project(session: &mut Session) {
    session.state_mut::<ProjectWatchers>().root = None;
}

fn global(session: &mut Session) -> GlobalWatchers {
    session.state_mut::<GlobalWatchers>().clone()
}

pub(crate) fn with_store<R>(session: &mut Session, level: Level, change: impl FnOnce(&mut Store) -> R) -> R {
    match level {
        Level::Project => change(project_store(session)),
        Level::Global => {
            let global = global(session);
            let mut store = global.lock();
            change(&mut store)
        }
    }
}

pub fn listed(session: &mut Session) -> Vec<ListedWatcher> {
    let project = project_store(session).watchers.clone();
    let global = global(session).watchers();
    project
        .into_iter()
        .map(|watcher| ListedWatcher { level: Level::Project, watcher })
        .chain(global.into_iter().map(|watcher| ListedWatcher { level: Level::Global, watcher }))
        .collect()
}

pub(crate) fn find(session: &mut Session, level: Level, id: &str) -> Option<FileWatcher> {
    with_store(session, level, |store| store.watchers.iter().find(|watcher| watcher.id == id).cloned())
}

pub(crate) fn set_enabled(session: &mut Session, level: Level, id: &str, enabled: bool) -> Result<(), CommandError> {
    with_store(session, level, |store| {
        let index = store.index(id)?;
        store.watchers[index].enabled = enabled;
        store.save()
    })
}

fn person_only(session: &Session) -> Result<(), CommandError> {
    if zen_core::macros::by_agent(session) {
        return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("workbench-file-watchers-agent")));
    }
    Ok(())
}

fn no_project_level(session: &Session, level: Level) -> Result<(), CommandError> {
    if level == Level::Project && project_root(session).is_none() {
        return Err(CommandError::new(t!("workbench-file-watchers-no-project")));
    }
    Ok(())
}

/// What `run.fileWatchers.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WatcherList {
    /// The project's watchers first, then the global ones, each in the user's order.
    pub watchers: Vec<ListedWatcher>,
    /// The project's file, when a project is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_file: Option<PathBuf>,
    /// The file of the global ones, when the IDE has a data folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_file: Option<PathBuf>,
}

fn list(session: &mut Session) -> WatcherList {
    let watchers = listed(session);
    let project_file = project_store(session).file.clone();
    let global_file = global(session).lock().file.clone();
    WatcherList { watchers, project_file, global_file }
}

pub struct ListWatchers;

impl Command for ListWatchers {
    type Params = NoParams;
    type Output = WatcherList;
    const ID: &'static str = "run.fileWatchers.list";
    const TITLE: &'static str = "File Watchers";
    const DESCRIPTION: &'static str = "Lists the file watchers of Settings → Tools → File Watchers: the project's (.zen/fileWatchers.json) first, then the global ones (the IDE's data folder), each with its level and every option, and the two files.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<WatcherList, CommandError> {
        Ok(list(session))
    }
}

pub struct ListTemplates;

impl Command for ListTemplates {
    type Params = NoParams;
    type Output = Vec<Template>;
    const ID: &'static str = "run.fileWatchers.templates";
    const TITLE: &'static str = "File Watcher Templates";
    const DESCRIPTION: &'static str = "Lists the templates + offers in Settings → Tools → File Watchers besides <custom>: the web ones every IDE has and those of its language modules (go fmt, goimports, golangci-lint in an IDE with Go), by name. run.fileWatchers.save with a template's watcher adds it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<Template>, CommandError> {
        Ok(templates(session))
    }
}

/// A watcher by its level and id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WatcherRef {
    pub id: String,
    #[serde(default)]
    pub level: Level,
}

/// The parameters of `run.fileWatchers.save`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveParams {
    /// The watcher; an empty or unknown id adds a new one, a known id replaces that watcher.
    pub watcher: FileWatcher,
    /// Where to keep it; the project by default.
    #[serde(default)]
    pub level: Level,
}

pub struct SaveWatcher;

impl Command for SaveWatcher {
    type Params = SaveParams;
    type Output = WatcherRef;
    const ID: &'static str = "run.fileWatchers.save";
    const TITLE: &'static str = "Save File Watcher";
    const DESCRIPTION: &'static str = "Adds a file watcher or changes one, by level and id (New Watcher and Edit Watcher): it needs a name and a program, every output filter must be a regular expression with $FILE_PATH$, and every environment variable a name. Returns its id and level. Refused for agents with code `agentNotAllowed`: a watcher runs a program on every save, so only the person sets one up.";

    fn run(&self, session: &mut Session, params: SaveParams) -> Result<WatcherRef, CommandError> {
        person_only(session)?;
        params.watcher.validate().map_err(CommandError::new)?;
        no_project_level(session, params.level)?;
        let id = with_store(session, params.level, |store| {
            let id = store.put(params.watcher);
            store.save().map(|()| id)
        })?;
        Ok(WatcherRef { id, level: params.level })
    }
}

/// The parameters of `run.fileWatchers.replace`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceParams {
    /// Every watcher with its level, in the order wanted.
    pub watchers: Vec<ListedWatcher>,
}

pub struct ReplaceWatchers;

impl Command for ReplaceWatchers {
    type Params = ReplaceParams;
    type Output = WatcherList;
    const ID: &'static str = "run.fileWatchers.replace";
    const TITLE: &'static str = "Apply File Watchers";
    const DESCRIPTION: &'static str = "Replaces both lists of file watchers, as OK in Settings → Tools → File Watchers does: the project's and the global ones, by each watcher's level; a watcher without an id gets one. Returns the lists. Refused for agents with code `agentNotAllowed`: a watcher runs a program on every save, so only the person sets one up.";

    fn run(&self, session: &mut Session, params: ReplaceParams) -> Result<WatcherList, CommandError> {
        person_only(session)?;
        for listed in &params.watchers {
            listed.watcher.validate().map_err(CommandError::new)?;
        }
        let pick = |level: Level| -> Vec<FileWatcher> {
            params.watchers.iter().filter(|listed| listed.level == level).map(|listed| listed.watcher.clone()).collect()
        };
        let (project, global) = (pick(Level::Project), pick(Level::Global));
        if !project.is_empty() {
            no_project_level(session, Level::Project)?;
        }
        if project_root(session).is_some() {
            with_store(session, Level::Project, |store| {
                store.replace(project);
                store.save()
            })?;
        }
        with_store(session, Level::Global, |store| {
            store.replace(global);
            store.save()
        })?;
        Ok(list(session))
    }
}

pub struct RemoveWatcher;

impl Command for RemoveWatcher {
    type Params = WatcherRef;
    type Output = NoParams;
    const ID: &'static str = "run.fileWatchers.remove";
    const TITLE: &'static str = "Remove File Watcher";
    const DESCRIPTION: &'static str = "Removes a file watcher (− in Settings → Tools → File Watchers).";

    fn run(&self, session: &mut Session, params: WatcherRef) -> Result<NoParams, CommandError> {
        with_store(session, params.level, |store| {
            let index = store.index(&params.id)?;
            store.watchers.remove(index);
            store.save()
        })?;
        Ok(NoParams {})
    }
}

/// The parameters of `run.fileWatchers.move`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveParams {
    pub id: String,
    #[serde(default)]
    pub level: Level,
    /// Up, else down.
    pub up: bool,
}

/// Whether a watcher moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    /// False when it was first (or last) of its level already.
    pub moved: bool,
}

pub struct MoveWatcher;

impl Command for MoveWatcher {
    type Params = MoveParams;
    type Output = Moved;
    const ID: &'static str = "run.fileWatchers.move";
    const TITLE: &'static str = "Move File Watcher";
    const DESCRIPTION: &'static str = "Moves a file watcher one place up or down among the watchers of its level (Up and Down in Settings → Tools → File Watchers).";

    fn run(&self, session: &mut Session, params: MoveParams) -> Result<Moved, CommandError> {
        let moved = with_store(session, params.level, |store| {
            let index = store.index(&params.id)?;
            let other = if params.up { index.checked_sub(1) } else { Some(index + 1) };
            let Some(other) = other.filter(|other| *other < store.watchers.len()) else { return Ok(false) };
            store.watchers.swap(index, other);
            store.save().map(|()| true)
        })?;
        Ok(Moved { moved })
    }
}

pub struct CopyWatcher;

impl Command for CopyWatcher {
    type Params = WatcherRef;
    type Output = WatcherRef;
    const ID: &'static str = "run.fileWatchers.copy";
    const TITLE: &'static str = "Copy File Watcher";
    const DESCRIPTION: &'static str = "Adds a copy of a file watcher right after it, at the same level, named as a copy; returns the copy's id. Refused for agents with code `agentNotAllowed`.";

    fn run(&self, session: &mut Session, params: WatcherRef) -> Result<WatcherRef, CommandError> {
        person_only(session)?;
        let id = with_store(session, params.level, |store| {
            let index = store.index(&params.id)?;
            let mut copy = store.watchers[index].clone();
            copy.name = t!("workbench-file-watchers-copy-name", name = copy.name.as_str());
            copy.id = store.free_id(&copy.name);
            let id = copy.id.clone();
            store.watchers.insert(index + 1, copy);
            store.save().map(|()| id)
        })?;
        Ok(WatcherRef { id, level: params.level })
    }
}

/// The parameters of `run.fileWatchers.enable`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnableParams {
    pub id: String,
    #[serde(default)]
    pub level: Level,
    /// Off switches it off; on by default.
    #[serde(default = "on")]
    pub enabled: bool,
}

fn on() -> bool {
    true
}

pub struct EnableWatcher;

impl Command for EnableWatcher {
    type Params = EnableParams;
    type Output = NoParams;
    const ID: &'static str = "run.fileWatchers.enable";
    const TITLE: &'static str = "Enable File Watcher";
    const DESCRIPTION: &'static str = "Switches a file watcher on (Enable again in the notification of a watcher that failed and was switched off) or, with enabled false, off. Agents may only switch one off: switching on is refused for them with code `agentNotAllowed`, also through the notification's action.";

    fn run(&self, session: &mut Session, params: EnableParams) -> Result<NoParams, CommandError> {
        if params.enabled {
            person_only(session)?;
        }
        set_enabled(session, params.level, &params.id, params.enabled)?;
        Ok(NoParams {})
    }
}

/// The parameters of `run.fileWatchers.import`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportParams {
    /// A file written by `run.fileWatchers.export` (Export in Settings → Tools → File Watchers).
    pub path: PathBuf,
    /// Where to add its watchers; the project by default.
    #[serde(default)]
    pub level: Level,
}

/// The watchers an import added.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    /// Their ids; a watcher whose id is taken gets a new one.
    pub ids: Vec<String>,
}

pub struct ImportWatchers;

impl Command for ImportWatchers {
    type Params = ImportParams;
    type Output = Imported;
    const ID: &'static str = "run.fileWatchers.import";
    const TITLE: &'static str = "Import File Watchers";
    const DESCRIPTION: &'static str = "Adds the file watchers of a file Export wrote (Import in Settings → Tools → File Watchers) at a level, after the watchers there; every one must be valid, or none is added. Refused for agents with code `agentNotAllowed`: a watcher runs a program on every save, so only the person sets one up.";

    fn run(&self, session: &mut Session, params: ImportParams) -> Result<Imported, CommandError> {
        person_only(session)?;
        let path = session
            .state::<Workbench>()
            .map_or_else(|| params.path.clone(), |workbench| workbench.resolve(&params.path));
        let bytes = std::fs::read(&path).map_err(|error| {
            CommandError::new(t!(
                "workbench-file-watchers-import-failed",
                file = crate::display(&path),
                detail = error.to_string()
            ))
        })?;
        let watchers = serde_json::from_slice::<Saved>(&bytes).map(|saved| saved.watchers).map_err(|error| {
            CommandError::new(t!(
                "workbench-file-watchers-import-failed",
                file = crate::display(&path),
                detail = error.to_string()
            ))
        })?;
        for watcher in &watchers {
            watcher.validate().map_err(CommandError::new)?;
        }
        no_project_level(session, params.level)?;
        let ids = with_store(session, params.level, |store| {
            let ids: Vec<String> = watchers
                .into_iter()
                .map(|mut watcher| {
                    if store.watchers.iter().any(|known| known.id == watcher.id) {
                        watcher.id.clear();
                    }
                    store.put(watcher)
                })
                .collect();
            store.save().map(|()| ids)
        })?;
        Ok(Imported { ids })
    }
}

/// The parameters of `run.fileWatchers.export`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    /// The file to write.
    pub path: PathBuf,
    /// The watchers to write; every one when absent.
    #[serde(default)]
    pub watchers: Option<Vec<WatcherRef>>,
}

/// How many watchers an export wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub count: usize,
}

pub struct ExportWatchers;

impl Command for ExportWatchers {
    type Params = ExportParams;
    type Output = Exported;
    const ID: &'static str = "run.fileWatchers.export";
    const TITLE: &'static str = "Export File Watchers";
    const DESCRIPTION: &'static str = "Writes file watchers into a file in the format of .zen/fileWatchers.json (Export in Settings → Tools → File Watchers), for run.fileWatchers.import in another project or IDE. An agent cannot write into a file hidden from agents (code `redacted`).";

    fn run(&self, session: &mut Session, params: ExportParams) -> Result<Exported, CommandError> {
        let path = session
            .state::<Workbench>()
            .map_or_else(|| params.path.clone(), |workbench| workbench.resolve(&params.path));
        if let Some(root) = project_root(session) {
            crate::hidden_files::guard_change(session, &root, std::slice::from_ref(&path))?;
        }
        let watchers: Vec<FileWatcher> = listed(session)
            .into_iter()
            .filter(|listed| {
                params.watchers.as_ref().is_none_or(|wanted| {
                    wanted.iter().any(|one| one.id == listed.watcher.id && one.level == listed.level)
                })
            })
            .map(|listed| listed.watcher)
            .collect();
        let count = watchers.len();
        let json = serde_json::to_vec_pretty(&Saved { format_version: FORMAT_VERSION, watchers }).unwrap_or_default();
        zen_vfs::write_atomic(&path, &json).map_err(|error| {
            CommandError::new(t!(
                "workbench-file-watchers-export-failed",
                file = crate::display(&path),
                detail = error.to_string()
            ))
        })?;
        Ok(Exported { count })
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListWatchers)?;
    registry.register(ListTemplates)?;
    registry.register(SaveWatcher)?;
    registry.register(ReplaceWatchers)?;
    registry.register(RemoveWatcher)?;
    registry.register(MoveWatcher)?;
    registry.register(CopyWatcher)?;
    registry.register(EnableWatcher)?;
    registry.register(ImportWatchers)?;
    registry.register(ExportWatchers)?;
    engine::register(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests;
