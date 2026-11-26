use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::inspect::{DeclaredInspection, InspectContext, Inspector};
use zen_syntax::{Feature, TextFeatures};

use crate::Workbench;
use crate::inspection_profiles::{InspectionProfile, ProfileId};
use crate::inspections::Inspections;
use crate::language::servers::LanguageServers;
use crate::language::sweep::Sweep;
use crate::language::{Problem, with_languages};
use crate::search_scopes::{ALL_CHANGED, is_test, resolve_in};
use crate::tasks::{TaskHandle, Tasks};

mod export;
mod fix;
mod tree;

pub use export::{ExportFormat, ExportParams, ExportResults};
pub use fix::{
    CleanupOutcome, FixItems, FixParams, FixStarted, ItemFix, ItemFixList, ItemFixes, ItemFixesParams, SuppressItem,
    SuppressItemParams, UndoCleanup, UndoCleanupParams,
};
pub use tree::{InspectionNode, ResultsTree, ResultsTreeCommand, SeverityNode, TreeFile, TreeFolder, TreeParams};

const MAX_RUNS: usize = 8;
const MAX_WAIT: Duration = Duration::from_secs(30);
const SOURCE: &str = "inspections";

/// What Inspect Code covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ScopeKind {
    /// Every file of the project.
    #[default]
    Project,
    /// The files of `directory`, with its subfolders.
    Directory,
    /// The files in `files`.
    Files,
    /// The files with uncommitted changes.
    Uncommitted,
    /// The scope `custom` from search.scope.list.
    Custom,
}

/// What to inspect, and with what.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InspectRequest {
    /// Which kind of scope.
    pub scope: ScopeKind,
    /// The folder of the `directory` scope, relative to the project.
    pub directory: String,
    /// The files of the `files` scope.
    pub files: Vec<PathBuf>,
    /// The scope id of the `custom` scope.
    pub custom: String,
    /// Whether test sources count; absent means they do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_tests: Option<bool>,
    /// Only this inspection (an id from inspect.inspections.list); all when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection: Option<String>,
    /// File masks separated by commas, `!` excludes, e.g. `*.go,!*_test.go`; all files when empty.
    pub file_mask: String,
    /// The inspection profile; the one that applies to the project when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileId>,
}

/// A problem found, in the results of a run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultItem {
    /// Its id in the run.
    pub id: u64,
    /// Its file.
    pub path: PathBuf,
    /// Its file, relative to the project.
    pub relative: String,
    /// The id of its inspection.
    pub inspection: String,
    /// Whether it was excluded from the results.
    pub excluded: bool,
    /// The problem.
    pub problem: Problem,
}

/// A file a language server could not check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UncheckedFile {
    /// The file, relative to the project.
    pub relative: String,
    /// Why.
    pub reason: String,
}

/// What a run of Inspect Code found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectionResults {
    /// The run's id.
    pub run: u64,
    /// The name of its scope, as its tab shows it.
    pub title: String,
    /// What was asked.
    pub request: InspectRequest,
    /// How many files were inspected.
    pub files: usize,
    /// The problems found.
    pub items: Vec<ResultItem>,
    /// The files no language server checked, with why.
    pub unchecked: Vec<UncheckedFile>,
    /// Why language servers were not asked at all, if they were not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

type FoundProblems = Arc<Mutex<Vec<(PathBuf, Vec<Problem>)>>>;

struct Active {
    run: u64,
    title: String,
    request: InspectRequest,
    total: usize,
    task: TaskHandle,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicUsize>,
    finished: Arc<AtomicBool>,
    found: FoundProblems,
    sweep: Sweep,
    files: Vec<PathBuf>,
    note: Option<String>,
    profile: InspectionProfile,
    cleanup: bool,
}

#[derive(Default)]
pub struct InspectCode {
    active: Option<Active>,
    runs: Vec<InspectionResults>,
    next: u64,
    revision: u64,
    fixing: fix::Fixing,
}

impl InspectCode {
    pub fn running(&self) -> bool {
        self.active.is_some() || self.fixing.running()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn runs(&self) -> &[InspectionResults] {
        &self.runs
    }

    fn run_mut(&mut self, run: u64) -> Result<&mut InspectionResults, CommandError> {
        self.runs
            .iter_mut()
            .find(|results| results.run == run)
            .ok_or_else(|| CommandError::new(t!("workbench-inspect-unknown-run", run = run.to_string())))
    }
}

fn run_of(session: &Session, run: u64) -> Result<&InspectionResults, CommandError> {
    session
        .state::<InspectCode>()
        .and_then(|state| state.runs.iter().find(|results| results.run == run))
        .ok_or_else(|| CommandError::new(t!("workbench-inspect-unknown-run", run = run.to_string())))
}

fn shown_to_caller(session: &Session, mut results: InspectionResults) -> InspectionResults {
    if let Some(patterns) = crate::hidden_files::for_caller(session) {
        let root = root_of(session);
        results.items.retain(|item| !crate::hidden_files::is_hidden(&patterns, root.as_deref(), &item.path));
        results
            .unchecked
            .retain(|file| !crate::hidden_files::is_hidden(&patterns, root.as_deref(), Path::new(&file.relative)));
    }
    results
}

pub fn inspection_id(problem: &Problem) -> String {
    let source = problem.source.as_deref().unwrap_or_default();
    let code = problem.code.as_deref().unwrap_or_default();
    match (source, code) {
        ("", "") => "other".to_owned(),
        ("", only) | (only, "") => only.to_owned(),
        (source, code) if source == code => source.to_owned(),
        (source, code) => format!("{source}/{code}"),
    }
}

pub fn is_inspection(problem: &Problem, wanted: &str) -> bool {
    inspection_id(problem) == wanted
        || problem.source.as_deref() == Some(wanted)
        || problem.code.as_deref() == Some(wanted)
}

pub fn known_inspections(session: &Session) -> Vec<DeclaredInspection> {
    let mut known: Vec<DeclaredInspection> = session
        .state::<Inspections>()
        .map(|inspections| inspections.inspectors().iter().flat_map(|inspector| inspector.inspections()).collect())
        .unwrap_or_default();
    let language = zen_i18n::language();
    let declarations = session.state::<LanguageServers>().map(LanguageServers::declarations).unwrap_or_default();
    for declaration in declarations {
        for inspection in &declaration.inspections {
            known.push(DeclaredInspection {
                id: inspection.id.clone(),
                title: inspection.id.clone(),
                group: declaration.name.clone(),
                description: zen_lsp::servers::text_in(&inspection.description, &language),
                cleanup: inspection.cleanup,
                off: inspection.off,
            });
        }
    }
    known
}

fn mask_accepts(masks: &str, name: &str) -> bool {
    let parts: Vec<&str> = masks.split(',').map(str::trim).filter(|part| !part.is_empty()).collect();
    if parts.iter().any(|part| part.strip_prefix('!').is_some_and(|mask| zen_index::scope::glob(mask, name))) {
        return false;
    }
    let mut wanted = parts.iter().filter(|part| !part.starts_with('!')).peekable();
    wanted.peek().is_none() || wanted.any(|mask| zen_index::scope::glob(mask, name))
}

fn root_of(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

fn scope_title(session: &mut Session, request: &InspectRequest) -> String {
    match request.scope {
        ScopeKind::Project => t!("workbench-inspect-scope-project"),
        ScopeKind::Directory => t!("workbench-inspect-scope-directory", directory = request.directory.as_str()),
        ScopeKind::Uncommitted => t!("workbench-inspect-scope-uncommitted"),
        ScopeKind::Custom => crate::search_scopes::scopes(session, true)
            .into_iter()
            .find(|scope| scope.id == request.custom)
            .map_or_else(|| request.custom.clone(), |scope| scope.name),
        ScopeKind::Files => match request.files.as_slice() {
            [one] => one.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            many => t!("workbench-inspect-scope-files", count = many.len()),
        },
    }
}

fn files_of(session: &mut Session, request: &InspectRequest) -> Result<Vec<PathBuf>, CommandError> {
    let root = root_of(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let scoped = |session: &mut Session, id: Option<&str>, directory: &str| {
        let scope = resolve_in(session, id, directory, true)?;
        let index = session.state::<Workbench>().and_then(|workbench| workbench.index.clone());
        let mut files: Vec<PathBuf> = index
            .map(|index| index.file_list())
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, relative)| scope.filter.accepts(relative))
            .map(|(path, _)| path)
            .collect();
        files.extend(scope.extra_files().into_iter().map(|(path, _)| path));
        Ok::<_, CommandError>(files)
    };
    let mut files = match request.scope {
        ScopeKind::Project => scoped(session, None, "")?,
        ScopeKind::Directory => scoped(session, None, &request.directory)?,
        ScopeKind::Uncommitted => scoped(session, Some(ALL_CHANGED), "")?,
        ScopeKind::Custom => scoped(session, Some(&request.custom), "")?,
        ScopeKind::Files => {
            let hidden = crate::hidden_files::for_caller(session).unwrap_or_default();
            let chosen = request.files.iter().filter(|path| path.is_file());
            chosen
                .filter(|path| !crate::hidden_files::is_hidden(&hidden, Some(&root), path))
                .cloned()
                .collect::<Vec<_>>()
        }
    };
    let tests = request.include_tests.unwrap_or(true);
    files.retain(|path| {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let relative = crate::language::project_relative(path, Some(&root));
        (tests || !is_test(&relative)) && mask_accepts(&request.file_mask, &name)
    });
    files.sort();
    files.dedup();
    Ok(files)
}

struct FileJob {
    path: PathBuf,
    text: Option<String>,
    inspector: Option<Arc<dyn Inspector>>,
    plugin: Option<Arc<dyn TextFeatures>>,
}

fn inspector_of(session: &Session, path: &Path) -> Option<Arc<dyn Inspector>> {
    let inspector = session.state::<Inspections>()?.inspector_for(path)?;
    let installed = inspector.plugin().is_none_or(|plugin| {
        session.state::<crate::plugin_projects::PluginProjects>().is_some_and(|projects| projects.installed(plugin))
    });
    let allowed =
        !inspector.runs_programs() || root_of(session).is_none_or(|root| crate::trust::trusted(session, &root));
    (installed && allowed).then_some(inspector)
}

struct Context {
    root: PathBuf,
    tool: PathBuf,
    env: Vec<(String, String)>,
    settings: Value,
}

fn inspect_file(job: &FileJob, context: &Context) -> Vec<Problem> {
    let Some(text) = job.text.clone().or_else(|| zen_vfs::read_text(&job.path).ok()) else { return Vec::new() };
    let mut found = Vec::new();
    if let Some(inspector) = &job.inspector {
        let setting = |key: &str| context.settings.get(key).cloned();
        let inspect = InspectContext {
            root: &context.root,
            path: &job.path,
            text: &text,
            tool: &context.tool,
            env: &context.env,
            setting: &setting,
        };
        found.extend(inspector.inspect(&inspect).iter().map(|finding| crate::inspections::problem(&text, finding)));
    }
    if let Some(plugin) = &job.plugin {
        found.extend(crate::plugins::text_problems(plugin.as_ref(), &job.path, &text));
    }
    found
}

fn start(session: &mut Session, request: InspectRequest, cleanup: bool) -> Result<RunStarted, CommandError> {
    stop_active(session);
    let profile = crate::inspection_profiles::resolve(session, request.profile.as_ref())?;
    let files = files_of(session, &request)?;
    let title = scope_title(session, &request);
    let task_title = if cleanup {
        t!("workbench-cleanup-task", scope = title.as_str())
    } else {
        t!("workbench-inspect-task", scope = title.as_str())
    };
    let task = session.state_mut::<Tasks>().start(&task_title, SOURCE, false);
    let cancel = Arc::new(AtomicBool::new(false));
    task.cancel_with(Arc::clone(&cancel));
    let env = session.state::<crate::run::Runs>().map(|runs| runs.env().clone()).unwrap_or_default();
    let root = root_of(session).unwrap_or_default();
    let mut project_env = env.env.clone();
    project_env.extend(crate::run::project_environment(session, &root));
    let context =
        Context { root, tool: env.tool.clone(), env: project_env, settings: crate::inspections::settings_of(session) };
    let open: HashMap<PathBuf, String> = session
        .state::<Workbench>()
        .map(|workbench| {
            workbench
                .documents()
                .iter()
                .filter(|document| document.image().is_none())
                .map(|document| (document.path().to_owned(), document.text().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    let jobs: Vec<FileJob> = files
        .iter()
        .map(|path| FileJob {
            path: path.clone(),
            text: open.get(path).cloned(),
            inspector: inspector_of(session, path),
            plugin: crate::plugins::plugin_for(session, path, Feature::Problems),
        })
        .collect();
    let done = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicBool::new(false));
    let found = Arc::new(Mutex::new(Vec::new()));
    let (worker_cancel, worker_done, worker_finished, worker_found) =
        (Arc::clone(&cancel), Arc::clone(&done), Arc::clone(&finished), Arc::clone(&found));
    let wake = session.state::<Inspections>().and_then(Inspections::wake);
    let work = move || {
        for job in &jobs {
            if worker_cancel.load(Ordering::Relaxed) {
                break;
            }
            let problems = inspect_file(job, &context);
            if !problems.is_empty()
                && let Ok(mut found) = worker_found.lock()
            {
                found.push((job.path.clone(), problems));
            }
            worker_done.fetch_add(1, Ordering::Relaxed);
        }
        worker_finished.store(true, Ordering::Relaxed);
        if let Some(wake) = wake {
            wake();
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(work);
    #[cfg(target_arch = "wasm32")]
    work();
    let power_save = crate::power_save::is_on(session);
    let sweep = if power_save {
        Sweep::default()
    } else {
        with_languages(session, |languages, workbench| Sweep::new(files.clone(), languages, workbench))
    };
    let state = session.state_mut::<InspectCode>();
    state.next += 1;
    let run = state.next;
    let task_id = task.id();
    let total = files.len();
    state.active = Some(Active {
        run,
        title,
        request,
        total,
        task,
        cancel,
        done,
        finished,
        found,
        sweep,
        files,
        note: power_save.then(|| t!("workbench-inspect-power-save")),
        profile,
        cleanup,
    });
    Ok(RunStarted { run, task: task_id, files: total })
}

fn stop_active(session: &mut Session) {
    let Some(mut active) = session.state_mut::<InspectCode>().active.take() else { return };
    active.cancel.store(true, Ordering::Relaxed);
    active.task.finish(Ok(t!("workbench-inspect-cancelled")));
    with_languages(session, |languages, workbench| active.sweep.stop(languages, workbench));
}

fn merged(session: &mut Session, active: &mut Active) -> BTreeMap<PathBuf, Vec<Problem>> {
    let mut by_file: BTreeMap<PathBuf, Vec<Problem>> = BTreeMap::new();
    let found = active.found.lock().map(|mut found| std::mem::take(&mut *found)).unwrap_or_default();
    for (path, problems) in found.into_iter().chain(active.sweep.take_found()) {
        by_file.entry(path).or_default().extend(problems);
    }
    let open: BTreeSet<PathBuf> = session
        .state::<Workbench>()
        .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
        .unwrap_or_default();
    for path in &active.files {
        let mut more = crate::project_model::problems_of(session, path);
        more.extend(crate::file_watchers::problems(session, path));
        if open.contains(path) {
            more.extend(crate::inspections::all_problems(session, path));
        }
        if !more.is_empty() {
            by_file.entry(path.clone()).or_default().extend(more);
        }
    }
    by_file
}

fn finish(session: &mut Session, mut active: Active) {
    let root = root_of(session);
    let wanted = active.request.inspection.clone();
    let mut id = 0;
    let mut items: Vec<ResultItem> = Vec::new();
    for (path, mut problems) in merged(session, &mut active) {
        crate::suppression::refresh(session, &path);
        crate::suppression::apply(session, &path, &mut problems);
        problems.sort_by_key(|problem| (problem.start, problem.end, problem.severity));
        problems.dedup_by(|a, b| (a.start, a.end, &a.message) == (b.start, b.end, &b.message));
        let relative = crate::language::project_relative(&path, root.as_deref());
        for mut problem in problems {
            if wanted.as_deref().is_some_and(|wanted| !is_inspection(&problem, wanted)) {
                continue;
            }
            if !active.profile.adjust(&mut problem) && wanted.is_none() {
                continue;
            }
            id += 1;
            let inspection = inspection_id(&problem);
            items.push(ResultItem {
                id,
                path: path.clone(),
                relative: relative.clone(),
                inspection,
                excluded: false,
                problem,
            });
        }
    }
    let unchecked = active
        .sweep
        .take_unchecked()
        .into_iter()
        .map(|(path, reason)| UncheckedFile {
            relative: crate::language::project_relative(&path, root.as_deref()),
            reason,
        })
        .collect();
    if active.cleanup {
        let declared = known_inspections(session);
        items.retain(|item| {
            declared.iter().any(|inspection| inspection.cleanup && is_inspection(&item.problem, &inspection.id))
        });
        active.task.finish(Ok(t!("workbench-cleanup-found", count = items.len())));
        return fix::clean(session, items);
    }
    active.task.finish(Ok(t!("workbench-inspect-done", count = items.len())));
    let results = InspectionResults {
        run: active.run,
        title: active.title,
        request: active.request,
        files: active.total,
        items,
        unchecked,
        note: active.note,
    };
    let state = session.state_mut::<InspectCode>();
    state.runs.retain(|kept| kept.request != results.request);
    state.runs.push(results);
    let excess = state.runs.len().saturating_sub(MAX_RUNS);
    state.runs.drain(..excess);
    state.revision += 1;
}

fn step(session: &mut Session) {
    fix::step(session);
    let Some(mut active) = session.state_mut::<InspectCode>().active.take() else { return };
    if active.cancel.load(Ordering::Relaxed) {
        active.task.finish(Ok(t!("workbench-inspect-cancelled")));
        with_languages(session, |languages, workbench| active.sweep.stop(languages, workbench));
        return;
    }
    let swept = with_languages(session, |languages, workbench| active.sweep.step(languages, workbench));
    let finished = active.finished.load(Ordering::Relaxed);
    let inspected = active.done.load(Ordering::Relaxed);
    let total = active.total + active.sweep.total();
    let done = inspected + active.sweep.total() - active.sweep.left();
    let fraction = if total == 0 { 1.0 } else { done as f32 / total as f32 };
    active.task.progress(Some(fraction), &t!("workbench-inspect-progress", done = done, total = total));
    if swept && finished {
        finish(session, active);
    } else {
        session.state_mut::<InspectCode>().active = Some(active);
    }
}

/// What `inspect.code.run` started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RunStarted {
    /// The run's id.
    pub run: u64,
    /// Its background task, which tasks.background.cancel stops.
    pub task: u64,
    /// How many files are in scope.
    pub files: usize,
}

pub struct RunInspections;

impl Command for RunInspections {
    type Params = InspectRequest;
    type Output = RunStarted;
    const ID: &'static str = "inspect.code.run";
    const TITLE: &'static str = "Inspect Code";
    const DESCRIPTION: &'static str = "Runs the inspections over a scope as a background task that can be cancelled \
         (tasks.background.cancel): the IDE's inspectors and the language servers, which get every file in scope \
         opened for them for a moment. A run already going is cancelled. With `inspection`, only that inspection; with \
         `fileMask`, only the files it matches. Follow it with inspect.code.status, then read inspect.results.list.";

    fn run(&self, session: &mut Session, params: InspectRequest) -> Result<RunStarted, CommandError> {
        start(session, params, false)
    }
}

pub struct RunCleanup;

impl Command for RunCleanup {
    type Params = InspectRequest;
    type Output = RunStarted;
    const ID: &'static str = "inspect.cleanup.run";
    const TITLE: &'static str = "Code Cleanup";
    const DESCRIPTION: &'static str = "Applies the fixes of the code cleanup inspections (cleanup in inspect.profile.get) that \
         the profile leaves enabled over a scope, as a background task that can be cancelled: it inspects like \
         inspect.code.run, opens no results, then fixes one problem at a time. Each file's fixes are one undoable change. \
         An analyzer the profile that applies to the project turned off finds nothing, whichever profile is asked for. \
         Follow it with inspect.code.status, whose cleanup says what was done.";

    fn run(&self, session: &mut Session, params: InspectRequest) -> Result<RunStarted, CommandError> {
        start(session, InspectRequest { inspection: None, ..params }, true)
    }
}

/// The parameters of `inspect.code.status`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusParams {
    /// How long to wait for the run to end in milliseconds, at most 30 000; none by default.
    pub wait_ms: Option<u64>,
}

/// How inspecting is going.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    /// Whether a run or a batch of fixes is going.
    pub running: bool,
    /// The run going, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<u64>,
    /// The runs with results, the latest last.
    pub finished: Vec<u64>,
    /// What the last Code Cleanup did, once it ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<CleanupOutcome>,
}

pub struct InspectStatus;

impl Command for InspectStatus {
    type Params = StatusParams;
    type Output = RunStatus;
    const ID: &'static str = "inspect.code.status";
    const TITLE: &'static str = "Inspect Code Status";
    const DESCRIPTION: &'static str = "Moves a run of Inspect Code and the fixes of its results along and says how they are \
         doing; with waitMs it waits for them to end. The window runs it every frame while one runs.";

    fn run(&self, session: &mut Session, params: StatusParams) -> Result<RunStatus, CommandError> {
        let deadline = Instant::now() + params.wait_ms.map_or(Duration::ZERO, Duration::from_millis).min(MAX_WAIT);
        loop {
            step(session);
            let running = session.state::<InspectCode>().is_some_and(InspectCode::running);
            if !running || Instant::now() >= deadline {
                let state = session.state::<InspectCode>();
                return Ok(RunStatus {
                    running,
                    run: state.and_then(|state| state.active.as_ref()).map(|active| active.run),
                    finished: state.map(|state| state.runs.iter().map(|run| run.run).collect()).unwrap_or_default(),
                    cleanup: state.and_then(|state| state.fixing.outcome()),
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// An inspection offered by Run Inspection by Name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectionInfo {
    /// What it is.
    #[serde(flatten)]
    pub inspection: DeclaredInspection,
    /// Whether it was only seen in the results of a run, not declared.
    pub seen: bool,
}

pub struct ListInspections;

impl Command for ListInspections {
    type Params = NoParams;
    type Output = Vec<InspectionInfo>;
    const ID: &'static str = "inspect.inspections.list";
    const TITLE: &'static str = "List Inspections";
    const DESCRIPTION: &'static str = "The inspections inspect.code.run can run alone: those the IDE's inspectors and language \
         servers declare, and those only seen in results so far.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<InspectionInfo>, CommandError> {
        let known = known_inspections(session);
        let mut list: Vec<InspectionInfo> =
            known.into_iter().map(|inspection| InspectionInfo { inspection, seen: false }).collect();
        let results = session.state::<InspectCode>().map(InspectCode::runs).unwrap_or_default();
        for item in results.iter().flat_map(|run| &run.items) {
            let declared = list.iter().any(|info| is_inspection(&item.problem, &info.inspection.id));
            if !declared {
                let (title, group) = tree::fallback_name(&item.problem);
                list.push(InspectionInfo {
                    inspection: DeclaredInspection {
                        id: item.inspection.clone(),
                        title,
                        group,
                        description: String::new(),
                        cleanup: false,
                        off: false,
                    },
                    seen: true,
                });
            }
        }
        list.sort_by_key(|info| (info.inspection.group.to_lowercase(), info.inspection.title.to_lowercase()));
        Ok(list)
    }
}

/// The parameters of `inspect.results.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResultsParams {
    /// The run; the latest when absent.
    pub run: Option<u64>,
}

fn run_or_latest(session: &Session, run: Option<u64>) -> Result<u64, CommandError> {
    run.or_else(|| session.state::<InspectCode>().and_then(|state| state.runs.last()).map(|run| run.run))
        .ok_or_else(|| CommandError::new(t!("workbench-inspect-no-results")))
}

pub struct ListResults;

impl Command for ListResults {
    type Params = ResultsParams;
    type Output = InspectionResults;
    const ID: &'static str = "inspect.results.list";
    const TITLE: &'static str = "Inspection Results";
    const DESCRIPTION: &'static str = "The results of a run of Inspect Code: its items with their files and problems, and the \
         files no language server checked, with why.";

    fn run(&self, session: &mut Session, params: ResultsParams) -> Result<InspectionResults, CommandError> {
        let run = run_or_latest(session, params.run)?;
        let results = run_of(session, run)?.clone();
        Ok(shown_to_caller(session, results))
    }
}

/// The parameters of `inspect.results.exclude`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExcludeParams {
    /// The run.
    pub run: u64,
    /// The items.
    pub items: Vec<u64>,
    /// Whether they are excluded; false brings them back.
    #[serde(default = "yes")]
    pub excluded: bool,
}

fn yes() -> bool {
    true
}

pub struct ExcludeResults;

impl Command for ExcludeResults {
    type Params = ExcludeParams;
    type Output = usize;
    const ID: &'static str = "inspect.results.exclude";
    const TITLE: &'static str = "Exclude from Inspection Results";
    const DESCRIPTION: &'static str = "Excludes items from the results of a run, or brings them back with excluded false; \
         excluded items are left out of the tree and of exports. Returns how many changed.";

    fn run(&self, session: &mut Session, params: ExcludeParams) -> Result<usize, CommandError> {
        let state = session.state_mut::<InspectCode>();
        let results = state.run_mut(params.run)?;
        let mut changed = 0;
        for item in results.items.iter_mut().filter(|item| params.items.contains(&item.id)) {
            changed += usize::from(item.excluded != params.excluded);
            item.excluded = params.excluded;
        }
        state.revision += 1;
        Ok(changed)
    }
}

/// The parameters of a command on one run.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunParams {
    /// The run.
    pub run: u64,
}

pub struct RerunResults;

impl Command for RerunResults {
    type Params = RunParams;
    type Output = RunStarted;
    const ID: &'static str = "inspect.results.rerun";
    const TITLE: &'static str = "Rerun Inspection";
    const DESCRIPTION: &'static str = "Runs the inspections of a run again with what it asked for; its results are replaced \
         when the new run ends.";

    fn run(&self, session: &mut Session, params: RunParams) -> Result<RunStarted, CommandError> {
        let request = run_of(session, params.run)?.request.clone();
        start(session, request, false)
    }
}

pub struct CloseResults;

impl Command for CloseResults {
    type Params = RunParams;
    type Output = ();
    const ID: &'static str = "inspect.results.close";
    const TITLE: &'static str = "Close Inspection Results";
    const DESCRIPTION: &'static str = "Forgets the results of a run and closes its tab.";

    fn run(&self, session: &mut Session, params: RunParams) -> Result<(), CommandError> {
        run_of(session, params.run)?;
        let state = session.state_mut::<InspectCode>();
        state.runs.retain(|results| results.run != params.run);
        state.revision += 1;
        Ok(())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(RunInspections)?;
    registry.register(RunCleanup)?;
    registry.register(UndoCleanup)?;
    registry.register(InspectStatus)?;
    registry.register(ListInspections)?;
    registry.register(ListResults)?;
    registry.register(ResultsTreeCommand)?;
    registry.register(ExcludeResults)?;
    registry.register(RerunResults)?;
    registry.register(CloseResults)?;
    registry.register(ExportResults)?;
    registry.register(FixItems)?;
    registry.register(SuppressItem)?;
    registry.register(ItemFixes)?;
    Ok(())
}

#[cfg(test)]
mod tests;
