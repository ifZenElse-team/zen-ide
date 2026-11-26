use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::generators::{
    Field, FieldKind, FieldOption, GeneratedFile, GeneratorEnvironment, GeneratorForm, GeneratorInfo, GeneratorRequest,
    GeneratorSection, ModuleBuild, ProjectGenerator, StepAnswer, Values, jdk_release_key,
};
use zen_project::inspect::Programs as _;

use crate::jdk::{Jdks, default_jdk};

mod from_sources;
mod from_vcs;
mod module;
mod tools;

pub use from_sources::{DetectedSources, FromSources, SourcesParams, detect_sources};
pub use from_vcs::{CloneCheck, CloneDefaults, CloneParams, FromVcs, FromVcsCheck, FromVcsDefaults};
pub use module::{
    BUILD_FILE_UNSAVED, ModuleFromSources, ModuleOptions, ModuleOptionsAnswer, NO_BUILD, NewModule, NewModuleParams,
    ProjectBuild, Registered,
};
pub use tools::BuildToolRunner;

pub const DEFAULT_DIRECTORY: &str = "project.defaultDirectory";

pub const GENERATORS_NETWORK: &str = "project.generators.network";

pub const GENERATOR_SETTINGS: &str = "project.generators.settings";

pub const EMPTY: &str = "empty";

pub const FOLDER_NOT_EMPTY: &str = "folderNotEmpty";

pub const INVALID_LOCATION: &str = "invalidLocation";

pub const UNKNOWN_GENERATOR: &str = "unknownGenerator";

const MAX_WAIT_MS: u64 = 60_000;

const MAX_PREVIEW_TEXT: usize = 512 << 10;

const FIRST_NAME: &str = "untitled";

const KEPT_FILE: &str = "new-project.json";

const MAX_OUTPUT: usize = 400;

const LISTS_KEY: &str = "lists";

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListGenerators)?;
    registry.register(LoadForm)?;
    registry.register(CheckLocation)?;
    registry.register(Preview)?;
    registry.register(Create)?;
    registry.register(CreateStatus)?;
    registry.register(CancelCreate)?;
    registry.register(GetList)?;
    registry.register(SetList)?;
    registry.register(FromVcs)?;
    registry.register(FromVcsDefaults)?;
    registry.register(FromVcsCheck)?;
    registry.register(FromSources)?;
    registry.register(ModuleOptions)?;
    registry.register(NewModule)?;
    registry.register(ModuleFromSources)?;
    Ok(())
}

struct EmptyProject;

impl ProjectGenerator for EmptyProject {
    fn info(&self) -> GeneratorInfo {
        GeneratorInfo {
            id: EMPTY.to_owned(),
            name: t!("workbench-new-project-empty"),
            description: Some(t!("workbench-new-project-empty-description")),
            ..GeneratorInfo::default()
        }
    }

    fn form(&self) -> Result<GeneratorForm, String> {
        Ok(GeneratorForm::default())
    }

    fn files(&self, _request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
        Ok(Vec::new())
    }
}

#[derive(Clone)]
enum Stage<T> {
    Running,
    Ready(T),
    Failed(String),
}

struct Slot<T>(Arc<(Mutex<Stage<T>>, Condvar)>);

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: Clone + Send + 'static> Slot<T> {
    fn start(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Self {
        let slot = Self(Arc::new((Mutex::new(Stage::Running), Condvar::new())));
        let filled = slot.clone();
        let spawned = std::thread::Builder::new().name("new-project".to_owned()).spawn(move || {
            let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
            filled.fill(match answer {
                Ok(Ok(value)) => Stage::Ready(value),
                Ok(Err(error)) => Stage::Failed(error),
                Err(_) => Stage::Failed(t!("workbench-new-project-crashed")),
            });
        });
        if let Err(error) = spawned {
            slot.fill(Stage::Failed(error.to_string()));
        }
        slot
    }

    fn fill(&self, stage: Stage<T>) {
        let (state, changed) = &*self.0;
        *state.lock().unwrap_or_else(PoisonError::into_inner) = stage;
        changed.notify_all();
    }

    fn wait(&self, wait_ms: Option<u64>) -> Stage<T> {
        let (state, changed) = &*self.0;
        let wait = Duration::from_millis(wait_ms.unwrap_or(0).min(MAX_WAIT_MS));
        let deadline = Instant::now() + wait;
        let mut stage = state.lock().unwrap_or_else(PoisonError::into_inner);
        while matches!(&*stage, Stage::Running) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            stage = changed.wait_timeout(stage, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        stage.clone()
    }
}

struct CreateJob {
    slot: Slot<Created>,
    stop: Arc<AtomicBool>,
    programs: zen_run::Programs,
    output: JobOutput,
    trust: bool,
}

#[derive(Clone, Debug, Default)]
struct Created {
    path: PathBuf,
    kept: Vec<String>,
    registered: Option<String>,
    reload: Option<String>,
}

/// How far a running job is.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    /// What it is doing.
    pub phase: String,
    /// How much of it is done, from 0 to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f32>,
}

#[derive(Default)]
struct JobLines {
    lines: std::collections::VecDeque<String>,
    progress: Option<JobProgress>,
}

#[derive(Clone, Default)]
struct JobOutput(Arc<Mutex<JobLines>>);

impl JobOutput {
    fn lock(&self) -> MutexGuard<'_, JobLines> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn line(&self, line: &str) {
        let mut lines = self.lock();
        lines.lines.push_back(line.to_owned());
        if lines.lines.len() > MAX_OUTPUT {
            lines.lines.pop_front();
        }
    }

    fn progress(&self, phase: &str, fraction: Option<f32>) {
        self.lock().progress = Some(JobProgress { phase: phase.to_owned(), fraction });
    }
}

pub(crate) type Row = std::collections::BTreeMap<String, String>;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Kept {
    clone_urls: Vec<String>,
    lists: std::collections::BTreeMap<String, Vec<Row>>,
}

pub type GeneratorSource = Arc<dyn Fn(&GeneratorEnvironment) -> Vec<Arc<dyn ProjectGenerator>> + Send + Sync>;

pub type BuildSource = Arc<dyn Fn() -> Vec<Arc<dyn ModuleBuild>> + Send + Sync>;

type FormKey = (String, String, String);

#[derive(Default)]
struct Inner {
    modules: Vec<Arc<dyn ProjectGenerator>>,
    plugins: Option<GeneratorSource>,
    environment: GeneratorEnvironment,
    builds: Option<BuildSource>,
    data: Option<PathBuf>,
    kept: Kept,
    forms: HashMap<FormKey, Slot<GeneratorForm>>,
    previews: HashMap<String, (String, Slot<Vec<PreviewFile>>)>,
    jobs: HashMap<u64, CreateJob>,
    jdks: Jdks,
}

#[derive(Clone, Default)]
pub struct NewProjects {
    inner: Arc<Mutex<Inner>>,
    next_job: Arc<AtomicU64>,
}

impl std::fmt::Debug for NewProjects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewProjects").finish_non_exhaustive()
    }
}

impl NewProjects {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn declare(&self, generator: Arc<dyn ProjectGenerator>) {
        self.lock().modules.push(generator);
    }

    pub fn set_plugins(&self, source: GeneratorSource) {
        self.lock().plugins = Some(source);
    }

    fn set_environment(&self, environment: GeneratorEnvironment) {
        let mut inner = self.lock();
        if inner.environment != environment {
            inner.environment = environment;
            inner.forms.clear();
            inner.previews.clear();
        }
    }

    pub fn set_builds(&self, source: BuildSource) {
        self.lock().builds = Some(source);
    }

    pub fn set_data_dir(&self, data: PathBuf) {
        let kept = std::fs::read(data.join(KEPT_FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let mut inner = self.lock();
        inner.kept = kept.unwrap_or_default();
        inner.data = Some(data);
    }

    fn change_kept(&self, change: impl FnOnce(&mut Kept)) {
        let mut inner = self.lock();
        change(&mut inner.kept);
        let Some(data) = &inner.data else { return };
        if let Ok(text) = serde_json::to_vec_pretty(&inner.kept) {
            let _ = std::fs::create_dir_all(data);
            let _ = std::fs::write(data.join(KEPT_FILE), text);
        }
    }

    fn list_key(generator: &str, list: &str) -> String {
        format!("{generator}/{list}")
    }

    fn list(&self, generator: &str, list: &str) -> Vec<Row> {
        self.lock().kept.lists.get(&Self::list_key(generator, list)).cloned().unwrap_or_default()
    }

    fn lists_of(&self, generator: &str) -> serde_json::Value {
        let prefix = format!("{generator}/");
        let inner = self.lock();
        let lists =
            inner.kept.lists.iter().filter_map(|(key, rows)| {
                key.strip_prefix(&prefix).map(|list| (list.to_owned(), serde_json::json!(rows)))
            });
        serde_json::Value::Object(lists.collect())
    }

    fn form_inputs(&self, generator: &GeneratorInfo, values: &Values) -> Values {
        let mut inputs: Values = generator
            .form_depends_on
            .iter()
            .filter_map(|field| values.get(field).map(|value| (field.clone(), value.clone())))
            .collect();
        let lists = self.lists_of(&generator.id);
        if lists.as_object().is_some_and(|lists| !lists.is_empty()) {
            inputs.insert(LISTS_KEY.to_owned(), lists);
        }
        inputs
    }

    fn generators(&self) -> Vec<Arc<dyn ProjectGenerator>> {
        let (modules, plugins, environment) = {
            let inner = self.lock();
            (inner.modules.clone(), inner.plugins.clone(), inner.environment.clone())
        };
        let (languages, others): (Vec<_>, Vec<_>) = plugins
            .map(|source| source(&environment))
            .unwrap_or_default()
            .into_iter()
            .partition(|generator| generator.info().section == GeneratorSection::Types);
        let mut generators = modules;
        generators.extend(languages);
        generators.push(Arc::new(EmptyProject));
        generators.extend(others);
        generators.sort_by_key(|generator| generator.info().section == GeneratorSection::Generators);
        generators
    }

    fn generator(&self, id: &str) -> Result<Arc<dyn ProjectGenerator>, CommandError> {
        self.generators().into_iter().find(|generator| generator.info().id == id).ok_or_else(|| {
            CommandError::with_code(UNKNOWN_GENERATOR, t!("workbench-new-project-unknown", generator = id))
        })
    }

    fn form_slot(&self, generator: &Arc<dyn ProjectGenerator>, values: &Values, retry: bool) -> Slot<GeneratorForm> {
        let info = generator.info();
        let inputs = self.form_inputs(&info, values);
        let key = (info.id, zen_i18n::language(), serde_json::to_string(&inputs).unwrap_or_default());
        let mut inner = self.lock();
        if !retry && let Some(slot) = inner.forms.get(&key) {
            return slot.clone();
        }
        let generator = Arc::clone(generator);
        let jdks = inner.jdks.clone();
        let slot = Slot::start(move || generator.form_with(&inputs).map(|form| with_jdks(form, &jdks)));
        inner.forms.insert(key, slot.clone());
        slot
    }

    fn resolved(
        &self,
        generator: &Arc<dyn ProjectGenerator>,
        name: &str,
        values: &Values,
    ) -> Result<(GeneratorRequest, Option<PathBuf>), String> {
        let form = match self.form_slot(generator, values, false).wait(Some(MAX_WAIT_MS)) {
            Stage::Ready(form) => form,
            Stage::Failed(error) => return Err(error),
            Stage::Running => return Err(t!("workbench-new-project-form-pending")),
        };
        let mut values = form.resolved(name, values);
        if let Some(field) = form.missing(&values) {
            return Err(missing_text(field));
        }
        if let Some(blocked) = &form.blocked {
            return Err(blocked.clone());
        }
        values.insert(LISTS_KEY.to_owned(), self.lists_of(&generator.info().id));
        let jdk = self.note_jdk(&form, &mut values);
        Ok((GeneratorRequest { name: name.to_owned(), values }, jdk))
    }

    fn note_jdk(&self, form: &GeneratorForm, values: &mut Values) -> Option<PathBuf> {
        let field = form.fields.iter().find(|field| is_jdk(field) && values.contains_key(&field.id))?;
        let home = PathBuf::from(values.get(&field.id)?.as_str()?);
        let jdks = self.lock().jdks.clone();
        let jdk = jdks.store().jdks().into_iter().find(|jdk| jdk.home == home)?;
        values.insert(jdk_release_key(&field.id), jdk.major.into());
        Some(home)
    }

    fn keep_jdk(&self, target: &Path, home: &Path) {
        let jdks = self.lock().jdks.clone();
        let root = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_owned());
        let _ = jdks.store().choose(&root, Some(home));
    }
}

fn is_jdk(field: &Field) -> bool {
    matches!(field.kind, FieldKind::Jdk { .. })
}

pub fn missing_text(field: &Field) -> String {
    if is_jdk(field) {
        t!("workbench-new-project-no-jdk")
    } else {
        t!("workbench-new-project-required", field = field.label.as_str())
    }
}

fn with_jdks(mut form: GeneratorForm, jdks: &Jdks) -> GeneratorForm {
    if !form.fields.iter().any(is_jdk) {
        return form;
    }
    let found: Vec<_> = jdks.store().jdks().into_iter().filter(|jdk| jdk.compiler).collect();
    let home = |jdk: &crate::jdk::Jdk| jdk.home.to_string_lossy().into_owned();
    for field in &mut form.fields {
        if let FieldKind::Jdk { options, default } = &mut field.kind {
            *options = found
                .iter()
                .map(|jdk| FieldOption { value: home(jdk), label: jdk.label(), ..FieldOption::default() })
                .collect();
            *default = default_jdk(&found).map(home);
        }
    }
    form
}

#[derive(Clone)]
pub(crate) struct StepJob(Slot<StepAnswer>);

impl StepJob {
    pub(crate) fn poll(&self, wait_ms: Option<u64>) -> Option<Result<StepAnswer, String>> {
        match self.0.wait(wait_ms) {
            Stage::Running => None,
            Stage::Ready(answer) => Some(Ok(answer)),
            Stage::Failed(error) => Some(Err(error)),
        }
    }
}

pub(crate) fn start_step(
    session: &mut Session,
    generator: &str,
    step: &str,
    values: Values,
) -> Result<StepJob, CommandError> {
    let generator = shared(session).generator(generator)?;
    let step = step.to_owned();
    Ok(StepJob(Slot::start(move || generator.step(&step, values))))
}

fn shared(session: &mut Session) -> NewProjects {
    if session.state::<NewProjects>().is_none() {
        session.set_state(NewProjects::default());
    }
    let projects = session.state::<NewProjects>().cloned().unwrap_or_default();
    projects.lock().jdks = session.state_mut::<Jdks>().clone();
    projects.set_environment(GeneratorEnvironment {
        network: session.settings().get_or(GENERATORS_NETWORK, true),
        settings: session.settings().get(GENERATOR_SETTINGS).unwrap_or_default(),
    });
    projects
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn expanded(text: &str) -> PathBuf {
    let text = text.trim();
    match text.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with('/') => home().join(&rest[1..]),
        _ => PathBuf::from(text),
    }
}

pub fn default_directory(session: &Session) -> PathBuf {
    session
        .settings()
        .get::<String>(DEFAULT_DIRECTORY)
        .filter(|directory| !directory.trim().is_empty())
        .map_or_else(|| home().join("Projects"), |directory| expanded(&directory))
}

fn suggested(directory: &Path) -> PathBuf {
    let candidate = |index: usize| match index {
        0 => directory.join(FIRST_NAME),
        _ => directory.join(format!("{FIRST_NAME}{index}")),
    };
    (0..).map(candidate).find(|path| !path.exists()).unwrap_or_else(|| directory.join(FIRST_NAME))
}

fn is_empty_folder(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

fn invalid_name(name: &str) -> bool {
    name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(|c| c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\\'))
}

fn nearest_existing(path: &Path) -> Option<&Path> {
    path.ancestors().skip(1).find(|folder| folder.exists())
}

fn location_problem(text: &str, path: &Path) -> Option<String> {
    if text.trim().is_empty() {
        return Some(t!("workbench-new-project-no-location"));
    }
    if !path.is_absolute() {
        return Some(t!("workbench-new-project-not-absolute"));
    }
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    if invalid_name(&name) {
        return Some(t!("workbench-new-project-bad-name"));
    }
    if path.is_file() {
        return Some(t!("workbench-new-project-is-file"));
    }
    let Some(parent) = nearest_existing(path) else { return Some(t!("workbench-new-project-no-parent")) };
    if !parent.is_dir() {
        return Some(t!("workbench-new-project-parent-file", parent = parent.display().to_string()));
    }
    if std::fs::metadata(parent).is_ok_and(|metadata| metadata.permissions().readonly()) {
        return Some(t!("workbench-new-project-read-only", parent = parent.display().to_string()));
    }
    None
}

/// The project types of the New Project wizard and where a new project goes by default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorList {
    /// The types in the wizard's order.
    pub generators: Vec<GeneratorInfo>,
    /// Settings → System Settings → Default project directory.
    pub default_directory: PathBuf,
    /// A free location in it for a new project.
    pub suggested: PathBuf,
    /// System Settings → Project Generators: whether generators may use the network.
    pub network_allowed: bool,
}

pub struct ListGenerators;

impl Command for ListGenerators {
    type Params = NoParams;
    type Output = GeneratorList;
    const ID: &'static str = "project.new.generators";
    const TITLE: &'static str = "List New Project Types";
    const DESCRIPTION: &'static str = "The project types of the New Project wizard in its order: the language modules', Empty Project (`empty`), then the plug-ins' under Generators; with the default project directory and a free location in it. Works with no project open.";

    fn run(&self, session: &mut Session, _params: NoParams) -> Result<GeneratorList, CommandError> {
        let generators = shared(session).generators().iter().map(|generator| generator.info()).collect();
        let default_directory = default_directory(session);
        let network_allowed = session.settings().get_or(GENERATORS_NETWORK, true);
        Ok(GeneratorList { generators, suggested: suggested(&default_directory), default_directory, network_allowed })
    }
}

/// How something running in the background is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LoadState {
    /// Still running.
    Running,
    /// Finished.
    Ready,
    /// It failed: `error` says why.
    Failed,
}

/// Which form.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FormParams {
    /// The project type's id.
    pub generator: String,
    /// Load it again, after a failure or for fresh data.
    pub retry: bool,
    /// How long to wait for it, in milliseconds (at most 60 000); answers at once without it.
    pub wait_ms: Option<u64>,
    /// The values the wizard has so far; a form that depends on fields reads theirs.
    pub values: Values,
    /// Load it for New Module: the JDK field offers the project's JDK first.
    pub module: bool,
}

/// A project type's form as far as it has loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormAnswer {
    /// How the loading is doing.
    pub state: LoadState,
    /// The form, once it is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<GeneratorForm>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct LoadForm;

impl Command for LoadForm {
    type Params = FormParams;
    type Output = FormAnswer;
    const ID: &'static str = "project.new.form";
    const TITLE: &'static str = "Load New Project Form";
    const DESCRIPTION: &'static str = "The fields of project type `generator`, loaded in the background: a generator may need the network. A field of type `jdk` lists the JDKs of toolchain.jdk.list by their home; none means project.new.create refuses until one is added or downloaded (toolchain.jdk.download), then `retry`. Answers `state` running, ready (with `form`) or failed (with `error`); ask again, or give `waitMs`. `retry` loads it again.";

    fn run(&self, session: &mut Session, params: FormParams) -> Result<FormAnswer, CommandError> {
        let projects = shared(session);
        let generator = projects.generator(&params.generator)?;
        let stage = projects.form_slot(&generator, &params.values, params.retry).wait(params.wait_ms);
        Ok(match stage {
            Stage::Running => FormAnswer { state: LoadState::Running, form: None, error: None },
            Stage::Ready(form) => {
                let form = if params.module { module::with_project_jdk(session, form) } else { form };
                FormAnswer { state: LoadState::Ready, form: Some(form), error: None }
            }
            Stage::Failed(error) => FormAnswer { state: LoadState::Failed, form: None, error: Some(error) },
        })
    }
}

/// Where the project is to go.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CheckParams {
    /// The project's folder, a full path; `~` is the home folder.
    pub location: String,
}

/// Whether a project can go there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocationCheck {
    /// The folder, with `~` expanded.
    pub path: PathBuf,
    /// The project's name: the last part of the path.
    pub name: String,
    /// Why Create is disabled; absent when the location is fine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Whether the folder holds something, so that Create asks before using it.
    pub not_empty: bool,
}

pub fn check_location(text: &str) -> LocationCheck {
    let path = expanded(text);
    let problem = location_problem(text, &path);
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let not_empty = problem.is_none() && path.is_dir() && !is_empty_folder(&path);
    LocationCheck { path, name, problem, not_empty }
}

pub struct CheckLocation;

impl Command for CheckLocation {
    type Params = CheckParams;
    type Output = LocationCheck;
    const ID: &'static str = "project.new.check";
    const TITLE: &'static str = "Check New Project Location";
    const DESCRIPTION: &'static str = "Whether a project can be created at `location`: `problem` is the reason it cannot (Create is disabled with it), `notEmpty` that the folder holds something, so that project.new.create needs `reuse`.";

    fn run(&self, _session: &mut Session, params: CheckParams) -> Result<LocationCheck, CommandError> {
        Ok(check_location(&params.location))
    }
}

/// What to generate.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewParams {
    /// The project type's id.
    pub generator: String,
    /// The project's name.
    pub name: String,
    /// The fields' values by id; a field left out has its default.
    pub values: Values,
    /// How long to wait, in milliseconds (at most 60 000); answers at once without it.
    pub wait_ms: Option<u64>,
}

/// One generated file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFile {
    /// Its path in the project, with `/`.
    pub path: String,
    /// Its text; absent for a binary or a very long file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Its size in bytes.
    pub size: u64,
}

fn previewed(file: GeneratedFile) -> PreviewFile {
    let size = file.bytes.len() as u64;
    let text = (file.bytes.len() <= MAX_PREVIEW_TEXT).then(|| String::from_utf8(file.bytes).ok()).flatten();
    PreviewFile { path: file.path, text, size }
}

/// The files a generator would write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAnswer {
    /// How the generating is doing.
    pub state: LoadState,
    /// The files in path order, once they are ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<PreviewFile>>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct Preview;

impl Command for Preview {
    type Params = PreviewParams;
    type Output = PreviewAnswer;
    const ID: &'static str = "project.new.preview";
    const TITLE: &'static str = "Preview New Project";
    const DESCRIPTION: &'static str = "The files project type `generator` would write for `name` and `values`, generated in the background and written nowhere. Answers `state` running, ready (with `files`) or failed (with `error`); ask again with the same parameters, or give `waitMs`.";

    fn run(&self, session: &mut Session, params: PreviewParams) -> Result<PreviewAnswer, CommandError> {
        let projects = shared(session);
        let generator = projects.generator(&params.generator)?;
        let key = format!("{}\n{}", params.name, serde_json::to_string(&params.values).unwrap_or_default());
        let usable = |(kept, slot): &&(String, Slot<Vec<PreviewFile>>)| {
            *kept == key && !matches!(slot.wait(None), Stage::Failed(_))
        };
        let kept = projects.lock().previews.get(&params.generator).filter(usable).cloned();
        let slot = kept.map_or_else(
            || {
                let (working, name, values) = (projects.clone(), params.name.clone(), params.values.clone());
                let slot = Slot::start(move || {
                    let (request, _) = working.resolved(&generator, &name, &values)?;
                    let files = generator.files(&request)?.into_iter().filter(|file| !file.is_folder());
                    let mut files: Vec<PreviewFile> = files.map(previewed).collect();
                    files.sort_by(|a, b| a.path.cmp(&b.path));
                    Ok(files)
                });
                projects.lock().previews.insert(params.generator.clone(), (key, slot.clone()));
                slot
            },
            |(_, slot)| slot,
        );
        Ok(match slot.wait(params.wait_ms) {
            Stage::Running => PreviewAnswer { state: LoadState::Running, files: None, error: None },
            Stage::Ready(files) => PreviewAnswer { state: LoadState::Ready, files: Some(files), error: None },
            Stage::Failed(error) => PreviewAnswer { state: LoadState::Failed, files: None, error: Some(error) },
        })
    }
}

struct Staging {
    folder: PathBuf,
    made_parent: Option<PathBuf>,
    published: bool,
}

impl Staging {
    fn new(target: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = target.parent().ok_or_else(|| t!("workbench-new-project-no-parent"))?;
        let made_parent = (!parent.exists()).then(|| {
            let mut topmost = parent;
            while let Some(above) = topmost.parent().filter(|above| !above.exists()) {
                topmost = above;
            }
            topmost.to_owned()
        });
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let folder = parent.join(format!(".{name}.new-{}-{number}", std::process::id()));
        let staging = Self { folder, made_parent, published: false };
        std::fs::create_dir(&staging.folder).map_err(|error| error.to_string())?;
        Ok(staging)
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
        if !self.published
            && let Some(parent) = &self.made_parent
        {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

fn merged(from: &Path, into: &Path, below: &Path, kept: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = into.join(entry.file_name());
        let relative = below.join(entry.file_name());
        if !target.exists() {
            std::fs::rename(entry.path(), target)?;
        } else if entry.file_type()?.is_dir() && target.is_dir() && !target.is_symlink() {
            merged(&entry.path(), &target, &relative, kept)?;
        } else {
            kept.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn publish(staging: &mut Staging, target: &Path) -> Result<Vec<String>, String> {
    let mut kept = Vec::new();
    if target.exists() {
        merged(&staging.folder, target, Path::new(""), &mut kept).map_err(|error| error.to_string())?;
    } else {
        std::fs::rename(&staging.folder, target).map_err(|error| error.to_string())?;
    }
    staging.published = true;
    kept.sort();
    Ok(kept)
}

fn create_project(
    generator: &dyn ProjectGenerator,
    request: &GeneratorRequest,
    target: &Path,
    git: bool,
    stop: &AtomicBool,
    output: &JobOutput,
    register: Option<&module::Registration>,
) -> Result<Created, String> {
    let mut staging = Staging::new(target)?;
    generator.generate_showing(request, &staging.folder, stop, &|line| output.line(line))?;
    if git && !target.join(".git").exists() {
        zen_vcs::git::init(&staging.folder).map_err(|error| error.to_string())?;
    }
    if stop.load(Ordering::Relaxed) {
        return Err(t!("workbench-new-project-cancelled"));
    }
    let edits = register.map(|register| register.edits(&staging.folder, target, false)).transpose()?;
    let existed = target.exists();
    let kept = publish(&mut staging, target)?;
    let mut created = Created { path: target.to_owned(), kept, ..Created::default() };
    if let (Some(register), Some(edits)) = (register, edits.filter(|edits| !edits.is_empty())) {
        if let Err(error) = register.write(&edits) {
            if !existed {
                let _ = std::fs::remove_dir_all(target);
            }
            return Err(error);
        }
        created.registered = Some(register.file.clone());
        created.reload = Some(register.reload.clone());
    }
    Ok(created)
}

/// What to create.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateParams {
    /// The project type's id.
    pub generator: String,
    /// The project's folder, a full path; its last part is the project's name.
    pub location: String,
    /// The fields' values by id; a field left out has its default.
    pub values: Values,
    /// Create a Git repository in it.
    pub git: bool,
    /// Use a folder that holds something; existing files are kept.
    pub reuse: bool,
    /// How long to wait for it to end, in milliseconds (at most 60 000); answers at once without it.
    pub wait_ms: Option<u64>,
}

/// How creating a project is doing.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAnswer {
    /// Its job, for project.new.status and project.new.cancel.
    pub job: u64,
    /// How it is doing.
    pub state: LoadState,
    /// The project's folder, once it is created; open it with `open_project`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Files of the generator that were not written, as the folder had them already.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<String>,
    /// Whether the project is trusted: a new folder the person creates is; an agent's, a clone and a folder that held something are not (ADR-0205).
    pub trusted: bool,
    /// Why it failed; nothing is left behind then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What the job is doing while it runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<JobProgress>,
    /// The last lines a program running for it wrote.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub output: Vec<String>,
    /// The build file that now lists a new module.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registered: Option<String>,
    /// The command that loads the project again with the new module.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload: Option<String>,
}

fn answer(
    session: &mut Session,
    projects: &NewProjects,
    job: u64,
    wait_ms: Option<u64>,
) -> Result<CreateAnswer, CommandError> {
    let found = {
        let inner = projects.lock();
        inner.jobs.get(&job).map(|running| (running.slot.clone(), running.trust, running.output.clone()))
    };
    let (slot, trust, output) =
        found.ok_or_else(|| CommandError::new(t!("workbench-new-project-no-job", job = job)))?;
    let stage = slot.wait(wait_ms);
    let (progress, lines) = {
        let output = output.lock();
        (output.progress.clone(), output.lines.iter().cloned().collect())
    };
    let base = CreateAnswer {
        job,
        state: LoadState::Running,
        path: None,
        kept: Vec::new(),
        trusted: false,
        error: None,
        progress,
        output: lines,
        registered: None,
        reload: None,
    };
    Ok(match stage {
        Stage::Running => base,
        Stage::Failed(error) => CreateAnswer { state: LoadState::Failed, error: Some(error), ..base },
        Stage::Ready(created) => {
            if trust {
                crate::trust::trust_new_project(session, &created.path)?;
            }
            let trusted = crate::trust::trusted(session, &created.path);
            CreateAnswer {
                state: LoadState::Ready,
                path: Some(created.path),
                kept: created.kept,
                trusted,
                registered: created.registered,
                reload: created.reload,
                ..base
            }
        }
    })
}

fn refuse_tool(session: &mut Session, generator: &dyn ProjectGenerator) -> Result<(), CommandError> {
    let Some(tool) = generator.info().tool else { return Ok(()) };
    if zen_core::macros::by_agent(session) {
        return Err(CommandError::with_code(
            crate::trust::NOT_TRUSTED,
            t!("workbench-new-project-tool-agent", tool = tool.as_str()),
        ));
    }
    match session.state::<crate::Workbench>().and_then(crate::Workbench::project).map(Path::to_owned) {
        Some(root) if !crate::trust::trusted(session, &root) => {
            Err(crate::trust::refused(session, &root, t!("workbench-new-project-tool-safe-mode", tool = tool.as_str())))
        }
        _ => Ok(()),
    }
}

pub struct Create;

impl Command for Create {
    type Params = CreateParams;
    type Output = CreateAnswer;
    const ID: &'static str = "project.new.create";
    const TITLE: &'static str = "Create Project";
    const DESCRIPTION: &'static str = "Creates a project of type `generator` at `location` in the background, in a hidden folder beside it that replaces `location` only when everything worked, so a failure leaves nothing. `git` creates a Git repository. A location that holds something needs `reuse` (code `folderNotEmpty`), and existing files are kept (`kept`); a bad one is refused (code `invalidLocation`). Answers its `job`: follow it with project.new.status, or give `waitMs`. The person's projects are trusted; an agent's are not (ADR-0205). A `jdk` field's JDK becomes the project's. Does not open the project: call `open_project` with `path`.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<CreateAnswer, CommandError> {
        let projects = shared(session);
        let generator = projects.generator(&params.generator)?;
        refuse_tool(session, generator.as_ref())?;
        let check = check_location(&params.location);
        if let Some(problem) = check.problem {
            return Err(CommandError::with_code(INVALID_LOCATION, problem));
        }
        if check.not_empty && !params.reuse {
            return Err(CommandError::with_code(
                FOLDER_NOT_EMPTY,
                t!("workbench-new-project-not-empty", path = check.path.display().to_string()),
            ));
        }
        let (working, values, git) = (projects.clone(), params.values, params.git);
        let trust = !zen_core::macros::by_agent(session) && !check.not_empty;
        let (target, name) = (check.path, check.name);
        let job = projects.start(trust, move |stop, output, _| {
            let (request, jdk) = working.resolved(&generator, &name, &values)?;
            let created = create_project(generator.as_ref(), &request, &target, git, stop, output, None)?;
            if let Some(home) = jdk {
                working.keep_jdk(&target, &home);
            }
            Ok(created)
        });
        answer(session, &projects, job, params.wait_ms)
    }
}

impl NewProjects {
    fn start(
        &self,
        trust: bool,
        work: impl FnOnce(&AtomicBool, &JobOutput, &zen_run::Programs) -> Result<Created, String> + Send + 'static,
    ) -> u64 {
        let stop = Arc::new(AtomicBool::new(false));
        let programs = zen_run::Programs::default();
        let output = JobOutput::default();
        let (flag, lines, running) = (Arc::clone(&stop), output.clone(), programs.clone());
        let slot = Slot::start(move || work(&flag, &lines, &running));
        let job = self.next_job.fetch_add(1, Ordering::Relaxed) + 1;
        self.lock().jobs.insert(job, CreateJob { slot, stop, programs, output, trust });
        job
    }
}

/// Which creation.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct JobParams {
    /// The job project.new.create answered.
    pub job: u64,
    /// How long to wait for it to end, in milliseconds (at most 60 000); answers at once without it.
    pub wait_ms: Option<u64>,
}

pub struct CreateStatus;

impl Command for CreateStatus {
    type Params = JobParams;
    type Output = CreateAnswer;
    const ID: &'static str = "project.new.status";
    const TITLE: &'static str = "New Project Status";
    const DESCRIPTION: &'static str =
        "How creating a project is doing: `state` running, ready (with `path`) or failed (with `error`).";

    fn run(&self, session: &mut Session, params: JobParams) -> Result<CreateAnswer, CommandError> {
        let projects = shared(session);
        answer(session, &projects, params.job, params.wait_ms)
    }
}

/// Whether a cancellation was passed on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Whether the job was still known.
    pub cancelled: bool,
}

pub struct CancelCreate;

impl Command for CancelCreate {
    type Params = JobParams;
    type Output = Cancelled;
    const ID: &'static str = "project.new.cancel";
    const TITLE: &'static str = "Cancel New Project";
    const DESCRIPTION: &'static str =
        "Stops creating a project: whatever the generator wrote is removed, and the job ends failed.";

    fn run(&self, session: &mut Session, params: JobParams) -> Result<Cancelled, CommandError> {
        let projects = shared(session);
        let inner = projects.lock();
        let job = inner.jobs.get(&params.job);
        if let Some(job) = job {
            job.stop.store(true, Ordering::Relaxed);
            job.programs.kill();
        }
        Ok(Cancelled { cancelled: job.is_some() })
    }
}

/// Which list a project type keeps.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ListParams {
    /// The project type's id.
    pub generator: String,
    /// The list's id, from a field's `manage`.
    pub list: String,
}

/// The rows of a kept list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ListRows {
    /// The rows, each values by column id.
    pub rows: Vec<Row>,
}

pub struct GetList;

impl Command for GetList {
    type Params = ListParams;
    type Output = ListRows;
    const ID: &'static str = "project.newLists.get";
    const TITLE: &'static str = "Get New Project List";
    const DESCRIPTION: &'static str = "The rows of a list a project type keeps, as Manage catalogs… and Add… edit them; the field's `manage` names the list and its columns.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ListRows, CommandError> {
        Ok(ListRows { rows: shared(session).list(&params.generator, &params.list) })
    }
}

/// The new rows of a kept list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SetListParams {
    /// The project type's id.
    pub generator: String,
    /// The list's id, from a field's `manage`.
    pub list: String,
    /// All its rows, each values by column id.
    pub rows: Vec<Row>,
}

pub struct SetList;

impl Command for SetList {
    type Params = SetListParams;
    type Output = ListRows;
    const ID: &'static str = "project.newLists.set";
    const TITLE: &'static str = "Set New Project List";
    const DESCRIPTION: &'static str = "Replaces the rows of a list a project type keeps. The type's form then reads them; load it again with project.new.form.";

    fn run(&self, session: &mut Session, params: SetListParams) -> Result<ListRows, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(
                crate::trust::AGENT_NOT_ALLOWED,
                t!("workbench-new-project-lists-person-only"),
            ));
        }
        let projects = shared(session);
        let key = NewProjects::list_key(&params.generator, &params.list);
        let rows = params.rows.clone();
        projects.change_kept(|kept| {
            kept.lists.insert(key, rows);
        });
        Ok(ListRows { rows: params.rows })
    }
}

#[cfg(test)]
mod tests;
