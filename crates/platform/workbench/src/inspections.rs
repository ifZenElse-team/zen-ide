use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::inspect::{
    Finding, FindingSeverity, FixJob, FixKind, FixNotice, FixState, InspectContext, InspectionFix, Inspector, Wake,
};

use crate::Workbench;
use crate::language::{Languages, Problem, Severity, char_offset};
use crate::notifications::{Group, Kind, NewNotification};

#[derive(Default)]
pub struct Inspections {
    inspectors: Vec<Arc<dyn Inspector>>,
    found: HashMap<PathBuf, Found>,
    jobs: Vec<Job>,
    next_job: u64,
    wake: Option<Wake>,
}

struct Found {
    key: Key,
    problems: Vec<Problem>,
}

#[derive(Clone, PartialEq)]
struct Key {
    version: u64,
    generations: Vec<u64>,
    settings: Value,
    tool: PathBuf,
    inputs: Vec<Option<(u64, std::time::SystemTime)>>,
}

fn stamps(paths: &[PathBuf]) -> Vec<Option<(u64, std::time::SystemTime)>> {
    paths
        .iter()
        .map(|path| std::fs::metadata(path).ok().and_then(|meta| Some((meta.len(), meta.modified().ok()?))))
        .collect()
}

struct Job {
    id: u64,
    path: PathBuf,
    titles: Vec<String>,
    job: Box<dyn FixJob>,
    state: FixState,
    task: crate::tasks::TaskHandle,
    notice: Option<FixNotice>,
}

impl Inspections {
    pub fn set_inspectors(&mut self, inspectors: Vec<Arc<dyn Inspector>>, caches: Option<&Path>) {
        for inspector in &inspectors {
            if let Some(wake) = &self.wake {
                inspector.set_wake(Arc::clone(wake));
            }
            if let Some(caches) = caches {
                inspector.set_cache_dir(caches.join(inspector.id()));
            }
        }
        self.inspectors = inspectors;
        self.found.clear();
    }

    pub fn inspectors(&self) -> &[Arc<dyn Inspector>] {
        &self.inspectors
    }

    pub fn set_wake(&mut self, wake: Wake) {
        for inspector in &self.inspectors {
            inspector.set_wake(Arc::clone(&wake));
        }
        self.wake = Some(wake);
    }

    pub fn wake(&self) -> Option<Wake> {
        self.wake.clone()
    }

    pub fn cached(&self, path: &Path) -> &[Problem] {
        self.found.get(path).map_or(&[], |found| found.problems.as_slice())
    }

    pub(crate) fn inspector_for(&self, path: &Path) -> Option<Arc<dyn Inspector>> {
        let name = zen_vfs::file_types::known_name_at(path, None)?;
        self.inspectors.iter().find(|inspector| inspector.handles(&name)).cloned()
    }

    pub fn declared(&self) -> Vec<zen_project::inspect::DeclaredSetting> {
        self.inspectors.iter().flat_map(|inspector| inspector.settings()).collect()
    }

    pub fn running(&self) -> bool {
        self.jobs.iter().any(|job| matches!(job.state, FixState::Running { .. }))
    }

    pub(crate) fn job_state(&self, id: u64) -> Option<&FixState> {
        self.jobs.iter().find(|job| job.id == id).map(|job| &job.state)
    }
}

pub(crate) fn settings_of(session: &Session) -> Value {
    Value::Object(
        session
            .settings()
            .iter()
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .collect::<serde_json::Map<_, _>>(),
    )
}

fn with_context<T>(session: &Session, path: &Path, text: &str, f: impl FnOnce(&InspectContext<'_>) -> T) -> T {
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
    let mut env = session.state::<crate::run::Runs>().map(|runs| runs.env().clone()).unwrap_or_default();
    env.env.extend(crate::run::project_environment(session, &root));
    let settings = session.settings();
    let setting = |key: &str| settings.get::<Value>(key);
    let context = InspectContext { root: &root, path, text, tool: &env.tool, env: &env.env, setting: &setting };
    f(&context)
}

pub(crate) fn inspect(session: &mut Session, path: &Path) -> Vec<Problem> {
    if crate::power_save::is_on(session) {
        return Vec::new();
    }
    let Some(inspector) = session.state::<Inspections>().and_then(|inspections| inspections.inspector_for(path)) else {
        return Vec::new();
    };
    if let Some(plugin) = inspector.plugin()
        && !session.state::<crate::plugin_projects::PluginProjects>().is_some_and(|projects| projects.installed(plugin))
    {
        return Vec::new();
    }
    if inspector.runs_programs()
        && session
            .state::<Workbench>()
            .and_then(Workbench::project)
            .map(Path::to_owned)
            .is_some_and(|root| !crate::trust::trusted(session, &root))
    {
        return Vec::new();
    }
    let Some((version, text)) = session.state::<Workbench>().and_then(|workbench| {
        let document = workbench.documents().iter().find(|document| document.path() == path)?;
        document.image().is_none().then(|| (document.version(), document.text().to_owned()))
    }) else {
        return Vec::new();
    };
    let tool = session.state::<crate::run::Runs>().map(|runs| runs.env().tool.clone()).unwrap_or_default();
    let inputs = stamps(&inspector.inputs(path));
    let key = Key { version, generations: vec![inspector.generation()], settings: settings_of(session), tool, inputs };
    if let Some(found) = session.state::<Inspections>().and_then(|inspections| inspections.found.get(path))
        && found.key == key
    {
        return found.problems.clone();
    }
    let findings = with_context(session, path, &text, |context| inspector.inspect(context));
    let mut problems: Vec<Problem> = findings.iter().map(|finding| problem(&text, finding)).collect();
    problems.sort_by_key(|problem| (problem.start, problem.severity));
    let inspections = session.state_mut::<Inspections>();
    inspections.found.retain(|kept, _| kept.exists());
    inspections.found.insert(path.to_owned(), Found { key, problems: problems.clone() });
    problems
}

pub(crate) fn all_problems(session: &mut Session, path: &Path) -> Vec<Problem> {
    crate::inspection_profiles::refresh(session);
    crate::suppression::refresh(session, path);
    #[cfg(not(target_arch = "wasm32"))]
    if crate::database::injected::has_sql(session, path) {
        crate::database::assist::poll(session, path);
    }
    let mut found = inspect(session, path);
    found.extend(crate::plugins::problems(session, path));
    found.extend(crate::project_index::problems(session, path));
    found.extend(crate::spring_boot::problems(session, path));
    found.extend(crate::spelling::problems(session, path));
    found.extend(crate::file_watchers::problems(session, path));
    joined(session, path, found)
}

pub(crate) fn known_problems(session: &Session, path: &Path) -> Vec<Problem> {
    let mut found =
        session.state::<Inspections>().map(|inspections| inspections.cached(path).to_vec()).unwrap_or_default();
    found.extend(crate::plugins::known_problems(session, path));
    found.extend(crate::project_index::problems(session, path));
    found.extend(crate::spring_boot::known(session, path));
    found.extend(crate::spelling::known(session, path));
    found.extend(crate::file_watchers::problems(session, path));
    joined(session, path, found)
}

fn joined(session: &Session, path: &Path, found: Vec<Problem>) -> Vec<Problem> {
    if crate::power_save::is_on(session) {
        return Vec::new();
    }
    let mut problems =
        session.state::<Languages>().map(|languages| languages.problems(path).to_vec()).unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    problems.extend(crate::database::consoles::problems(session, path));
    #[cfg(not(target_arch = "wasm32"))]
    problems.extend(crate::database::assist::problems(session, path));
    if let Some(inspector) = session.state::<Inspections>().and_then(|inspections| inspections.inspector_for(path)) {
        for problem in &mut problems {
            if inspector.unused_diagnostic(problem.source.as_deref(), &problem.message) {
                problem.unused = true;
            }
        }
    }
    problems.retain(|problem| {
        !(problem.unused
            && problem.severity != Severity::Error
            && found.iter().any(|finding| {
                finding.unused
                    && finding.start < problem.end.max(problem.start + 1)
                    && problem.start < finding.end.max(finding.start + 1)
            }))
    });
    let repeated = |finding: &Problem| {
        problems.iter().any(|problem| problem.line == finding.line && problem.message == finding.message)
    };
    let found: Vec<Problem> = found.into_iter().filter(|finding| !repeated(finding)).collect();
    problems.extend(found);
    let model: Vec<Problem> = crate::project_model::problems_of(session, path)
        .into_iter()
        .filter(|from_model| {
            !problems.iter().any(|problem| problem.line == from_model.line && problem.severity == Severity::Error)
        })
        .collect();
    problems.extend(model);
    crate::suppression::apply(session, path, &mut problems);
    crate::inspection_profiles::apply(session, &mut problems);
    problems.sort_by_key(|problem| (problem.start, problem.severity));
    problems
}

pub(crate) fn inspects(session: &Session, path: &Path) -> bool {
    session.state::<Inspections>().is_some_and(|inspections| inspections.inspector_for(path).is_some())
}

pub(crate) fn inspected_files(session: &Session) -> Vec<PathBuf> {
    let Some(inspections) = session.state::<Inspections>() else { return Vec::new() };
    let Some(workbench) = session.state::<Workbench>() else { return Vec::new() };
    workbench
        .documents()
        .iter()
        .map(|document| document.path().to_owned())
        .filter(|path| inspections.inspector_for(path).is_some())
        .collect()
}

pub(crate) fn problem(text: &str, finding: &Finding) -> Problem {
    let start = floor_boundary(text, finding.start.min(text.len()));
    let end = floor_boundary(text, finding.end.clamp(start, text.len()));
    let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
    Problem {
        start: char_offset(text, start),
        end: char_offset(text, end),
        line: text[..start].matches('\n').count() as u32 + 1,
        column: text[line_start..start].chars().count() as u32 + 1,
        severity: match finding.severity {
            FindingSeverity::Error => Severity::Error,
            FindingSeverity::Warning => Severity::Warning,
            FindingSeverity::WeakWarning => Severity::WeakWarning,
            FindingSeverity::Information => Severity::Information,
        },
        message: finding.message.clone(),
        source: Some(finding.source.clone()),
        code: Some(finding.code.clone()),
        note: finding.note.clone(),
        rows: finding.rows.clone(),
        details: finding.details.clone(),
        fixes: finding.fixes.clone(),
        deprecated: finding.deprecated,
        unused: finding.unused,
    }
}

fn floor_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The parameters of `problems.fixes.list`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FixesParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

fn place(session: &Session, path: Option<PathBuf>, offset: Option<usize>) -> Result<(PathBuf, usize), CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let path = path
        .or_else(|| workbench.active_document().map(|document| document.path().to_owned()))
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let offset = match offset {
        Some(offset) => offset,
        None => workbench
            .documents()
            .iter()
            .find(|document| document.path() == path)
            .map(|document| document.selections().primary().head)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
    };
    Ok((path, offset))
}

pub(crate) fn fixable_at(session: &mut Session, path: &Path, offset: usize) -> Vec<Problem> {
    let problems: Vec<Problem> =
        all_problems(session, path).into_iter().filter(|problem| !problem.fixes.is_empty()).collect();
    let mut at: Vec<Problem> = problems
        .iter()
        .filter(|problem| problem.start <= offset && offset <= problem.end.max(problem.start + 1))
        .cloned()
        .collect();
    if at.is_empty() {
        let line = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .and_then(|document| document.snapshot().offset_to_point(offset).ok())
            .map(|point| point.line as u32 + 1);
        at = problems
            .into_iter()
            .filter(|problem| Some(problem.line) == line && !crate::spelling::is_typo(problem))
            .collect();
    }
    at.sort_by_key(|problem| problem.severity);
    for problem in &mut at {
        crate::spelling::with_suggestions(session, path, problem);
    }
    crate::suppression::with_fixes(session, path, &mut at);
    at
}

pub struct ListFixes;

impl Command for ListFixes {
    type Params = FixesParams;
    type Output = Vec<Problem>;
    const ID: &'static str = "problems.fixes.list";
    const TITLE: &'static str = "Show Context Actions";
    const DESCRIPTION: &'static str = "The problems with quick-fixes at a place (the caret of the active file by default, \
         else anywhere on its line), the worst first, each with its fixes (the first that is not a heading is the main \
         one, ⌥⇧↵). Apply one with problems.fix.apply and its id; a fix of kind batch lists the fixes with its batch \
         key that can be applied together, and one of kind heading is only a caption (Fix typo, over a typo's \
         suggestions).";

    fn run(&self, session: &mut Session, params: FixesParams) -> Result<Vec<Problem>, CommandError> {
        let (path, offset) = place(session, params.path, params.offset)?;
        Ok(fixable_at(session, &path, offset))
    }
}

/// The parameters of `problems.fix.apply`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The ids of the fixes to apply together (`problems.fixes.list`, `problems.file.list`); fixes
    /// of kind `apply` only.
    pub ids: Vec<String>,
}

/// What `problems.fix.apply` started.
#[derive(Serialize, JsonSchema)]
pub struct Started {
    /// The job's id, for `problems.fix.status`.
    pub job: u64,
    /// Its background task, for `tasks.log.read`.
    pub task: u64,
}

pub struct ApplyFixes;

impl Command for ApplyFixes {
    type Params = ApplyParams;
    type Output = Started;
    const ID: &'static str = "problems.fix.apply";
    const TITLE: &'static str = "Apply Quick-Fix";
    const DESCRIPTION: &'static str = "Applies quick-fixes of a file's problems by id, in the background (e.g. go get for \
         a go.mod dependency), after saving the file. Returns the job and its background task: problems.fix.status \
         says how it is doing and tasks.log.read reads what it prints (the window shows it in the background tasks \
         panel). A fix of kind details is read from the problem's details instead, and one of kind action is a \
         window action to run with run_action (args.action, args.params).";

    fn run(&self, session: &mut Session, params: ApplyParams) -> Result<Started, CommandError> {
        let path = match params.path {
            Some(path) => path,
            None => place(session, None, Some(0))?.0,
        };
        if params.ids.is_empty() {
            return Err(CommandError::new(t!("workbench-fix-none")));
        }
        let problems = all_problems(session, &path);
        let mut fixes: Vec<InspectionFix> = Vec::new();
        for id in &params.ids {
            let fix = problems
                .iter()
                .flat_map(|problem| &problem.fixes)
                .find(|fix| &fix.id == id && !matches!(fix.kind, FixKind::Batch | FixKind::Heading))
                .ok_or_else(|| CommandError::new(t!("workbench-fix-unknown", id = id.as_str())))?;
            match fix.kind {
                FixKind::Details => return Err(CommandError::new(t!("workbench-fix-details", id = id.as_str()))),
                FixKind::Action => {
                    let action = fix.args["action"].as_str().unwrap_or_default();
                    return Err(CommandError::new(t!("workbench-fix-action", id = id.as_str(), action = action)));
                }
                FixKind::Apply | FixKind::Batch | FixKind::Heading => {}
            }
            if !fixes.iter().any(|kept| kept.id == fix.id) {
                fixes.push(fix.clone());
            }
        }
        start_fixes(session, path, fixes)
    }
}

pub(crate) fn start_fixes(
    session: &mut Session,
    path: PathBuf,
    fixes: Vec<InspectionFix>,
) -> Result<Started, CommandError> {
    let unknown = || CommandError::new(t!("workbench-fix-unknown", id = fixes[0].id.as_str()));
    let inspector =
        session.state::<Inspections>().and_then(|inspections| inspections.inspector_for(&path)).ok_or_else(unknown)?;
    if inspector.runs_programs()
        && let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
        && !crate::trust::trusted(session, &root)
    {
        return Err(crate::trust::refused(session, &root, t!("workbench-fix-safe-mode")));
    }
    crate::on_save::before_save(session, &path);
    if let Some(document) =
        session.state_mut::<Workbench>().documents.iter_mut().find(|document| document.path() == path && document.dirty)
    {
        let name = document.name();
        document.save().map_err(|error| CommandError::new(crate::closing::save_failed(&name, &error)))?;
    }
    let text = crate::language::file_text(
        session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        &path,
    )
    .map_err(CommandError::new)?;
    let job =
        with_context(session, &path, &text, |context| inspector.apply(context, &fixes)).map_err(CommandError::new)?;
    crate::dependencies::write_changes(session, &job.writes())?;
    let titles: Vec<String> = fixes.iter().map(|fix| fix.title.clone()).collect();
    let title = job.title().unwrap_or_else(|| match titles.as_slice() {
        [one] => t!("workbench-fix-task-one", fix = one.as_str()),
        many => t!("workbench-fix-task-many", count = many.len()),
    });
    let task = session.state_mut::<crate::tasks::Tasks>().start(&title, "inspections", true);
    if let Some(programs) = job.programs() {
        task.cancel_kills(programs);
    }
    let inspections = session.state_mut::<Inspections>();
    inspections.next_job += 1;
    let id = inspections.next_job;
    let task_id = task.id();
    inspections.jobs.push(Job { id, path, titles, job, state: FixState::Running { line: None }, task, notice: None });
    Ok(Started { job: id, task: task_id })
}

/// A fix being applied, or applied, as `problems.fix.status` reports it.
#[derive(Serialize, JsonSchema)]
pub struct JobStatus {
    /// The job's id.
    pub job: u64,
    /// The file of the problems.
    pub path: PathBuf,
    /// The titles of the fixes.
    pub fixes: Vec<String>,
    /// How it is doing.
    pub state: FixState,
    /// What it told the user when it was done, if anything, e.g. why the go directive was raised.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<FixNotice>,
}

pub struct FixStatus;

impl Command for FixStatus {
    type Params = NoParams;
    type Output = Vec<JobStatus>;
    const ID: &'static str = "problems.fix.status";
    const TITLE: &'static str = "Quick-Fix Status";
    const DESCRIPTION: &'static str = "How the quick-fixes applied in the background are doing, the latest last. When one \
         has finished, the open files it changed are read again and what it told the user (notice, e.g. why go get \
         raised the go directive) is posted as a notification; when one failed, the window tells what its program \
         printed. The window runs it every frame while one runs.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<JobStatus>, CommandError> {
        let mut finished = Vec::new();
        if let Some(inspections) = session.state::<Inspections>()
            && inspections.jobs.is_empty()
        {
            return Ok(Vec::new());
        }
        let inspections = session.state_mut::<Inspections>();
        for job in &mut inspections.jobs {
            if !matches!(job.state, FixState::Running { .. }) {
                continue;
            }
            job.state = job.job.poll();
            let output = job.job.drain_output();
            if !output.is_empty() {
                job.task.log(&output.join("\n"));
            }
            if !matches!(job.state, FixState::Running { .. }) && job.task.cancel_requested() {
                job.state = FixState::Cancelled;
            }
            match &job.state {
                FixState::Running { .. } => continue,
                FixState::Cancelled => job.task.finish(Ok(String::new())),
                FixState::Done { .. } => {
                    job.notice = job.job.notice();
                    job.task.finish(Ok(t!("workbench-fix-done")));
                }
                FixState::Failed { output } => job.task.finish(Err(output.clone())),
            }
            finished.push((job.path.clone(), job.state.clone(), job.titles.clone(), job.notice.clone()));
        }
        for (path, _, _, _) in &finished {
            inspections.found.remove(path);
        }
        let excess = inspections.jobs.len().saturating_sub(20);
        inspections.jobs.drain(..excess);
        let report = inspections
            .jobs
            .iter()
            .map(|job| JobStatus {
                job: job.id,
                path: job.path.clone(),
                fixes: job.titles.clone(),
                state: job.state.clone(),
                notice: job.notice.clone(),
            })
            .collect();
        for (_, state, titles, notice) in finished {
            match state {
                FixState::Done { changed } => {
                    let workbench = session.state_mut::<Workbench>();
                    for document in workbench.documents.iter_mut().filter(|document| changed.contains(&document.path)) {
                        document.sync_with_disk();
                    }
                    workbench.revision += 1;
                    if let Some(FixNotice { title, text }) = notice {
                        crate::notifications::post(
                            session,
                            NewNotification::message(Group::General, Kind::Info, text).titled(title),
                        );
                    }
                }
                FixState::Failed { output } => {
                    let fix = titles.first().cloned().unwrap_or_default();
                    crate::notifications::post(
                        session,
                        NewNotification::message(Group::Errors, Kind::Error, output)
                            .titled_by(zen_i18n::Message::new("workbench-fix-failed").with("fix", fix)),
                    );
                }
                FixState::Running { .. } | FixState::Cancelled => {}
            }
        }
        Ok(report)
    }
}

pub struct DeclaredSettings;

/// A declared setting with its value.
#[derive(Serialize, JsonSchema)]
pub struct DeclaredValue {
    /// The setting.
    #[serde(flatten)]
    pub setting: zen_project::inspect::DeclaredSetting,
    /// Its value now.
    pub value: bool,
}

impl Command for DeclaredSettings {
    type Params = NoParams;
    type Output = Vec<DeclaredValue>;
    const ID: &'static str = "settings.declared.list";
    const TITLE: &'static str = "Declared Settings";
    const DESCRIPTION: &'static str = "The on/off settings the IDE's inspectors declare (e.g. go.mod's dependency checks), \
         with their page, the servers they contact and their values. Change them with the MCP tool \
         set_settings (declaredSettings).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<DeclaredValue>, CommandError> {
        let settings = session.state::<Inspections>().map(Inspections::declared).unwrap_or_default();
        Ok(settings
            .into_iter()
            .map(|setting| {
                let value = session.settings().get::<bool>(&setting.key).unwrap_or(setting.default);
                DeclaredValue { setting, value }
            })
            .collect())
    }
}

pub struct RefreshInspections;

impl Command for RefreshInspections {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "problems.inspections.refresh";
    const TITLE: &'static str = "Refresh Inspections";
    const DESCRIPTION: &'static str = "Makes the inspectors (e.g. the go.mod dependency checks) forget what they looked up \
         and look again when the files are next inspected.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        let inspections = session.state_mut::<Inspections>();
        for inspector in &inspections.inspectors {
            inspector.refresh();
        }
        inspections.found.clear();
        Ok(())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListFixes)?;
    registry.register(ApplyFixes)?;
    registry.register(FixStatus)?;
    registry.register(RefreshInspections)?;
    registry.register(DeclaredSettings)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use serde_json::json;
    use zen_project::inspect::FindingRow;

    use super::*;

    #[derive(Default)]
    struct Todo {
        generation: AtomicU64,
        applied: Mutex<Vec<String>>,
    }

    struct Instant(Option<FixState>);

    impl FixJob for Instant {
        fn poll(&mut self) -> FixState {
            self.0.take().unwrap_or(FixState::Running { line: None })
        }
    }

    impl Inspector for Todo {
        fn id(&self) -> &str {
            "todo"
        }

        fn handles(&self, file_name: &str) -> bool {
            file_name == "notes.txt"
        }

        fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
            if (context.setting)("todo.off") == Some(json!(true)) {
                return Vec::new();
            }
            context
                .text
                .match_indices("TODO")
                .map(|(at, _)| Finding {
                    start: at,
                    end: at + 4,
                    severity: FindingSeverity::WeakWarning,
                    message: format!("todo {}", self.generation.load(Ordering::SeqCst)),
                    note: None,
                    rows: vec![FindingRow { label: "T-1".to_owned(), url: None, badge: None, text: "row".to_owned() }],
                    details: None,
                    fixes: vec![InspectionFix {
                        id: format!("done@{at}"),
                        title: "Mark done".to_owned(),
                        kind: FixKind::Apply,
                        batch: None,
                        label: None,
                        inline: false,
                        args: json!({ "at": at }),
                    }],
                    source: "todo".to_owned(),
                    code: "todo".to_owned(),
                    deprecated: false,
                    unused: false,
                })
                .collect()
        }

        fn generation(&self) -> u64 {
            self.generation.load(Ordering::SeqCst)
        }

        fn refresh(&self) {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }

        fn apply(&self, context: &InspectContext<'_>, fixes: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
            self.applied.lock().unwrap().extend(fixes.iter().map(|fix| fix.id.clone()));
            let text = std::fs::read_to_string(context.path).map_err(|error| error.to_string())?;
            if text.contains("FAIL") {
                return Ok(Box::new(Instant(Some(FixState::Failed { output: "it broke".to_owned() }))));
            }
            std::fs::write(context.path, text.replace("TODO", "DONE")).map_err(|error| error.to_string())?;
            Ok(Box::new(Instant(Some(FixState::Done { changed: vec![context.path.to_owned()] }))))
        }
    }

    fn fixture(text: &str) -> (tempfile::TempDir, Registry, Session, PathBuf, Arc<Todo>) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let todo = Arc::new(Todo::default());
        session.state_mut::<Inspections>().set_inspectors(vec![todo.clone()], None);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let file = session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        (dir, registry, session, file, todo)
    }

    #[test]
    fn findings_join_the_problems_with_their_fixes() {
        let (_dir, registry, mut session, file, todo) = fixture("a\nb TODO\n");
        let problems = registry.dispatch(&mut session, "problems.file.list", json!({})).unwrap();
        assert_eq!(problems.as_array().unwrap().len(), 1, "{problems}");
        let problem = &problems[0];
        assert_eq!(
            (problem["start"].as_u64(), problem["line"].as_u64(), problem["column"].as_u64()),
            (Some(4), Some(2), Some(3))
        );
        assert_eq!(problem["severity"], "weakWarning");
        assert_eq!(problem["fixes"][0]["id"], "done@4");
        assert_eq!(problem["rows"][0]["label"], "T-1");
        let fixes = registry.dispatch(&mut session, "problems.fixes.list", json!({ "offset": 2 })).unwrap();
        assert_eq!(fixes[0]["fixes"][0]["id"], "done@4", "{fixes}");
        let none = registry.dispatch(&mut session, "problems.fixes.list", json!({ "offset": 0 })).unwrap();
        assert_eq!(none, json!([]));
        todo.refresh();
        let problems = registry.dispatch(&mut session, "problems.file.list", json!({ "path": file })).unwrap();
        assert_eq!(problems[0]["message"], "todo 1");
        let project = registry.dispatch(&mut session, "problems.project.list", json!({})).unwrap();
        assert_eq!(project[0]["problems"][0]["message"], "todo 1", "{project}");
        session.settings_mut().set("todo.off", true).unwrap();
        let problems = registry.dispatch(&mut session, "problems.file.list", json!({})).unwrap();
        assert_eq!(problems, json!({ "problems": [], "checked": true }), "inspected and clean");
    }

    #[test]
    fn a_fix_runs_and_the_changed_file_is_read_again() {
        let (_dir, registry, mut session, file, todo) = fixture("TODO\n");
        let started = registry.dispatch(&mut session, "problems.fix.apply", json!({ "ids": ["done@0"] })).unwrap();
        assert_eq!(started["job"], 1);
        assert_eq!(*todo.applied.lock().unwrap(), ["done@0"]);
        let status = registry.dispatch(&mut session, "problems.fix.status", json!({})).unwrap();
        assert_eq!(status[0]["state"]["state"], "done", "{status}");
        let tasks = registry.dispatch(&mut session, "tasks.background.list", json!({})).unwrap();
        assert_eq!(tasks["panelShown"], true, "{tasks}");
        let task = tasks["tasks"].as_array().unwrap().iter().find(|task| task["source"] == "inspections").unwrap();
        assert_eq!(task["id"], started["task"]);
        assert_eq!(task["title"], "Mark done");
        assert_eq!(task["state"], "succeeded");
        assert_eq!(session.state::<Workbench>().unwrap().active_document().unwrap().text(), "DONE\n");
        let problems = registry.dispatch(&mut session, "problems.file.list", json!({ "path": file })).unwrap();
        assert_eq!(problems, json!({ "problems": [], "checked": true }));
        let unknown = registry.dispatch(&mut session, "problems.fix.apply", json!({ "ids": ["nope"] }));
        assert!(unknown.is_err());
    }

    #[test]
    fn a_dirty_file_is_saved_first_and_a_failure_is_told() {
        let (_dir, registry, mut session, file, _todo) = fixture("TODO\n");
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "FAIL " })).unwrap();
        registry.dispatch(&mut session, "problems.fix.apply", json!({ "ids": ["done@5"] })).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "FAIL TODO\n", "saved before the fix");
        let tasks = registry.dispatch(&mut session, "tasks.background.list", json!({})).unwrap();
        assert_eq!(tasks["panelShown"], true, "{tasks}");
        registry.dispatch(&mut session, "problems.fix.status", json!({})).unwrap();
        let notifications = session.state_mut::<crate::notifications::Notifications>();
        let told = notifications.newest_first().next().expect("a notification tells the failure");
        assert_eq!(told.kind, Kind::Error);
        assert!(!told.title.is_empty() && told.body.contains("it broke"), "{told:?}");
        let log = registry.dispatch(&mut session, "tasks.log.read", json!({})).unwrap();
        assert_eq!(log["task"]["state"], "failed", "{log}");
        assert_eq!(log["lines"], json!(["it broke"]));
    }

    struct Interrupted(Arc<AtomicBool>);

    impl zen_project::inspect::Programs for Interrupted {
        fn interrupt(&self) {
            self.0.store(true, Ordering::SeqCst);
        }

        fn kill(&self) {}

        fn running(&self) -> bool {
            false
        }
    }

    struct Hanging(Arc<AtomicBool>);

    impl FixJob for Hanging {
        fn poll(&mut self) -> FixState {
            if self.0.load(Ordering::SeqCst) {
                FixState::Failed { output: "signal: interrupt".to_owned() }
            } else {
                FixState::Running { line: None }
            }
        }

        fn programs(&self) -> Option<Arc<dyn zen_project::inspect::Programs>> {
            Some(Arc::new(Interrupted(Arc::clone(&self.0))))
        }
    }

    #[test]
    fn a_cancelled_fix_ends_cancelled_and_tells_no_error() {
        let (_dir, registry, mut session, file, _todo) = fixture("TODO\n");
        let task = session.state_mut::<crate::tasks::Tasks>().start("Updating", "inspections", true);
        let stopped = Arc::new(AtomicBool::new(false));
        let job = Hanging(Arc::clone(&stopped));
        task.cancel_kills(job.programs().unwrap());
        let inspections = session.state_mut::<Inspections>();
        inspections.next_job += 1;
        inspections.jobs.push(Job {
            id: inspections.next_job,
            path: file,
            titles: vec!["Update".to_owned()],
            job: Box::new(job),
            state: FixState::Running { line: None },
            task: task.clone(),
            notice: None,
        });
        let id = task.id();
        assert_eq!(registry.dispatch(&mut session, "tasks.background.cancel", json!({})).unwrap(), json!(id));
        let status = loop {
            let status = registry.dispatch(&mut session, "problems.fix.status", json!({})).unwrap();
            if status[0]["state"]["state"] != "running" {
                break status;
            }
            std::thread::yield_now();
        };
        assert_eq!(status[0]["state"]["state"], "cancelled", "{status}");
        let log = registry.dispatch(&mut session, "tasks.log.read", json!({ "id": id })).unwrap();
        assert_eq!(log["task"]["state"], "cancelled", "{log}");
        let notifications = session.state_mut::<crate::notifications::Notifications>();
        assert!(notifications.newest_first().next().is_none());
    }

    struct Sidecar;

    impl Inspector for Sidecar {
        fn id(&self) -> &str {
            "sidecar"
        }

        fn handles(&self, file_name: &str) -> bool {
            file_name == "notes.txt"
        }

        fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
            let done = std::fs::read_to_string(context.path.with_file_name("done.txt")).unwrap_or_default();
            if done.contains("TODO") || !context.text.contains("TODO") {
                return Vec::new();
            }
            vec![Finding {
                start: 0,
                end: 4,
                severity: FindingSeverity::Error,
                message: "not in done.txt".to_owned(),
                note: None,
                rows: Vec::new(),
                details: None,
                fixes: vec![InspectionFix {
                    id: "nothing".to_owned(),
                    title: "Do nothing".to_owned(),
                    kind: FixKind::Apply,
                    batch: None,
                    label: None,
                    inline: false,
                    args: json!({}),
                }],
                source: "sidecar".to_owned(),
                code: "sidecar".to_owned(),
                deprecated: false,
                unused: false,
            }]
        }

        fn inputs(&self, path: &Path) -> Vec<PathBuf> {
            vec![path.with_file_name("done.txt")]
        }

        fn apply(&self, _context: &InspectContext<'_>, _fixes: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
            Ok(Box::new(Instant(Some(FixState::Done { changed: Vec::new() }))))
        }
    }

    #[test]
    fn an_outside_change_of_a_file_the_findings_depend_on_refreshes_them() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, "TODO\n").unwrap();
        let done = dir.path().join("done.txt");
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.state_mut::<Inspections>().set_inspectors(vec![Arc::new(Sidecar)], None);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let count = |session: &mut Session| {
            crate::language::listed_problems(&registry.dispatch(session, "problems.file.list", json!({})).unwrap())
                .len()
        };
        assert_eq!(count(&mut session), 1);
        std::fs::write(&done, "TODO\n").unwrap();
        assert_eq!(count(&mut session), 0, "the change of done.txt is seen");
        std::fs::remove_file(&done).unwrap();
        assert_eq!(count(&mut session), 1);
    }

    #[test]
    fn a_finished_fix_refreshes_the_marks_even_when_it_changed_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, "TODO\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.state_mut::<Inspections>().set_inspectors(vec![Arc::new(Sidecar)], None);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        assert_eq!(registry.dispatch(&mut session, "problems.file.list", json!({})).unwrap()[0]["code"], "sidecar");
        let path = session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        let found = session.state_mut::<Inspections>().found.get_mut(&path).unwrap();
        found.problems[0].message = "stale".to_owned();
        registry.dispatch(&mut session, "problems.fix.apply", json!({ "ids": ["nothing"] })).unwrap();
        registry.dispatch(&mut session, "problems.fix.status", json!({})).unwrap();
        let problems = registry.dispatch(&mut session, "problems.file.list", json!({})).unwrap();
        assert_eq!(problems[0]["message"], "not in done.txt", "inspected again after the fix: {problems}");
    }

    struct Tidy;

    impl Inspector for Tidy {
        fn id(&self) -> &str {
            "tidy"
        }

        fn handles(&self, file_name: &str) -> bool {
            file_name == "go.mod"
        }

        fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
            let start = context.text.find("golang.org/x/sync").unwrap();
            let end = start + context.text[start..].find('\n').unwrap();
            vec![Finding {
                start,
                end,
                severity: FindingSeverity::WeakWarning,
                message: "Unused indirect requirement: go mod tidy removes it".to_owned(),
                note: None,
                rows: Vec::new(),
                details: None,
                fixes: Vec::new(),
                source: "go.mod".to_owned(),
                code: "unusedRequirement".to_owned(),
                deprecated: false,
                unused: true,
            }]
        }

        fn unused_diagnostic(&self, source: Option<&str>, message: &str) -> bool {
            source == Some("go mod tidy") && message.ends_with(" is not used in this module")
        }

        fn apply(&self, _: &InspectContext<'_>, _: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
            Err("no fixes".to_owned())
        }
    }

    struct Checker;

    impl Inspector for Checker {
        fn id(&self) -> &str {
            "checker"
        }

        fn handles(&self, file_name: &str) -> bool {
            file_name == "go.mod"
        }

        fn runs_programs(&self) -> bool {
            true
        }

        fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
            Tidy.inspect(context)
        }

        fn apply(&self, _: &InspectContext<'_>, _: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
            Err("no fixes".to_owned())
        }
    }

    #[test]
    fn a_check_that_runs_programs_waits_for_trust_and_runs_once_it_is_given() {
        let text = "module example.com/demo\n\nrequire golang.org/x/sync v0.24.0 // indirect\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("go.mod");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(crate::build_tools::BuildEnv::default());
        session.state_mut::<Inspections>().set_inspectors(vec![Arc::new(Checker)], None);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let listed = |registry: &Registry, session: &mut Session| {
            registry.dispatch(session, "problems.file.list", json!({})).unwrap().to_string().contains("Unused indirect")
        };
        assert!(!listed(&registry, &mut session), "safe mode");
        crate::trust::trust_session_in_tests(&mut session, dir.path());
        assert!(listed(&registry, &mut session));
    }

    #[test]
    fn a_finding_a_server_already_reports_on_its_line_is_listed_once() {
        let text = "x\nTODO one\n";
        let (_dir, registry, mut session, path, _) = fixture(text);
        let found = registry.dispatch(&mut session, "problems.file.list", json!({})).unwrap();
        assert_eq!(found.as_array().unwrap().len(), 1);
        let line = found[0]["line"].as_u64().unwrap() - 1;
        let range = json!({ "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 1 } });
        let same = json!({ "range": range, "severity": 2, "source": "gopls", "message": found[0]["message"] });
        session.state_mut::<Languages>().publish_for_test(&path, text, &[same]);
        let joined = registry.dispatch(&mut session, "problems.file.list", json!({})).unwrap();
        assert_eq!(joined.as_array().unwrap().len(), 1);
        assert_eq!(joined[0]["source"], "gopls");
    }

    #[test]
    fn a_server_diagnostic_of_unused_code_is_greyed_out_and_not_doubled() {
        let text = "module example.com/demo\n\ngo 1.24\n\nrequire golang.org/x/text v0.43.0\n\nrequire (\n\tgolang.org/x/sync v0.24.0 // indirect\n)\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("go.mod");
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.state_mut::<Inspections>().set_inspectors(vec![Arc::new(Tidy)], None);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let file = session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        let range = |line: u32, start: u32, end: u32| json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } });
        let diagnostics = [
            json!({ "range": range(4, 0, 33), "severity": 2, "source": "go mod tidy",
                    "message": "golang.org/x/text is not used in this module" }),
            json!({ "range": range(7, 1, 26), "severity": 2, "source": "go mod tidy",
                    "message": "golang.org/x/sync is not used in this module" }),
            json!({ "range": range(7, 1, 18), "severity": 1, "source": "compiler", "tags": [1],
                    "message": "an unused error keeps its underline" }),
            json!({ "range": range(2, 3, 7), "severity": 2, "source": "go mod tidy",
                    "message": "go.mod has a newer go directive" }),
        ];
        session.state_mut::<Languages>().publish_for_test(&file, text, &diagnostics);
        for id in ["problems.file.list", "problems.project.list"] {
            let listed = registry.dispatch(&mut session, id, json!({})).unwrap();
            let problems = if id == "problems.project.list" { listed[0]["problems"].clone() } else { listed };
            let summary: Vec<(String, bool, String)> = problems
                .as_array()
                .unwrap()
                .iter()
                .map(|problem| {
                    (
                        problem["message"].as_str().unwrap().to_owned(),
                        problem["unused"].as_bool().unwrap_or(false),
                        problem["severity"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            let row = |message: &str, unused: bool, severity: &str| (message.to_owned(), unused, severity.to_owned());
            assert_eq!(
                summary,
                [
                    row("go.mod has a newer go directive", false, "warning"),
                    row("golang.org/x/text is not used in this module", true, "warning"),
                    row("an unused error keeps its underline", true, "error"),
                    row("Unused indirect requirement: go mod tidy removes it", true, "weakWarning"),
                ],
                "{id}: {problems}"
            );
        }
    }
}
