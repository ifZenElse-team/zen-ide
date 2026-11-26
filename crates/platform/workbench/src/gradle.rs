pub mod analysis;
pub mod build_model;
pub mod catalog;
pub mod dependencies;
pub mod download;
pub mod model;
pub mod problems;
pub mod script;
pub mod settings;
pub mod workspace;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{
    AnythingPrefix, BeforeLaunchTask, CommandLine, Configuration, DebugPlan, FieldKind, FieldSpec,
    ForeignRunConfiguration, ForeignTask, KindSpec, OutputParser, PlainParser, RunContext, RunProvider, RunTarget,
};

use crate::build_check::{BlockKind, BuildJdk, Checked, LoadBlock, ToolCheck};
use crate::build_sync::SyncRun;
use crate::build_tools::{BuildEnv, Executable, LoadState, foreign_list, foreign_option, relative_to};
use crate::run::{RunStarted, project_root, wait_for_run};
use build_model::BuildModel;
use settings::GradleSettings;
use workspace::GradleWorkspace;

pub const KIND: &str = "gradle";

pub const SOURCES_SCRIPT: &str = include_str!("gradle/zen-sources.gradle");

pub const DEBUG_SCRIPT: &str = include_str!("gradle/zen-debug.gradle");

pub fn init_script(stem: &str, text: &str) -> Result<PathBuf, String> {
    use sha2::Digest;
    let hash = hex::encode(sha2::Sha256::digest(text.as_bytes()));
    let folder = std::env::temp_dir().join("zen-ide");
    let path = folder.join(format!("{stem}-{}.gradle", &hash[..12]));
    if !path.is_file() {
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        std::fs::write(&path, text).map_err(|error| error.to_string())?;
    }
    Ok(path)
}

fn sources_key(root: &Path) -> String {
    format!("gradle.sources\n{}", root.display())
}
pub const TASK_TASK: &str = "gradle.task";

const FOREIGN_TASK_TASK: &str = "Gradle.BeforeRunTask";

fn tasks_key(root: &Path) -> String {
    build_model::key(root)
}

pub fn relative_to_root(root: &Path, folder: &Path) -> String {
    match relative_to(root, &folder.to_string_lossy()) {
        path if path.is_empty() => ".".to_owned(),
        path => path,
    }
}

/// The options of a `gradle` configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GradleOptions {
    /// The build's root folder, relative to the project (`.` for the project's own).
    pub project_path: String,
    /// The tasks and their options, as typed: `clean build`, `:app:run --args="x"`.
    pub tasks: String,
}

impl Default for GradleOptions {
    fn default() -> Self {
        Self { project_path: ".".to_owned(), tasks: String::new() }
    }
}

fn options(configuration: &Configuration) -> GradleOptions {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub wrapper: bool,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

pub fn invocation(env: &BuildEnv, folder: &Path, root: &Path) -> Option<Invocation> {
    let settings = GradleSettings::read(env, root);
    let folders = env.folders("gradle");
    let found = settings::executable(&settings, folder, root, env, &folders)?;
    Some(Invocation {
        program: found.path,
        wrapper: found.wrapper,
        args: settings.args(),
        env: settings.environment(env, root, &folders),
    })
}

#[derive(Clone, Debug, Default)]
pub struct GradleRun {
    env: BuildEnv,
}

impl GradleRun {
    pub fn new(env: BuildEnv) -> Self {
        Self { env }
    }

    pub fn tasks(name: &str, project_path: &str, tasks: &str) -> Configuration {
        let options = GradleOptions { project_path: project_path.to_owned(), tasks: tasks.to_owned() };
        Configuration {
            name: format!("{name} [{tasks}]"),
            kind: KIND.to_owned(),
            options: serde_json::to_value(options).unwrap_or_default(),
            ..Configuration::default()
        }
    }

    pub fn command_line(&self, configuration: &Configuration, root: &Path) -> Result<CommandLine, String> {
        let options = options(configuration);
        if options.tasks.trim().is_empty() {
            return Err(t!("workbench-gradle-no-tasks"));
        }
        let folder = match options.project_path.trim() {
            "" | "." => root.to_owned(),
            path => root.join(path),
        };
        let Invocation { program, wrapper, mut args, mut env } =
            invocation(&self.env, &folder, root).ok_or_else(|| t!("workbench-gradle-not-found"))?;
        args.extend(zen_run::args::split(&options.tasks));
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        let cwd = match &configuration.common.working_directory {
            Some(dir) if !dir.as_os_str().is_empty() => root.join(dir),
            _ => folder,
        };
        env.extend(configuration.common.env.iter().cloned());
        let shown = if wrapper { "./gradlew".to_owned() } else { "gradle".to_owned() };
        let display = std::iter::once(shown).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        Ok(CommandLine { program, args, cwd, env, display })
    }
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

const FOREIGN_TYPE: &str = "GradleRunConfiguration";

impl RunProvider for GradleRun {
    fn id(&self) -> &str {
        KIND
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: KIND.to_owned(),
            name_message: "workbench-gradle-kind".to_owned(),
            fields: vec![
                field("projectPath", "workbench-gradle-field-project", FieldKind::Path),
                field("tasks", "workbench-gradle-field-tasks", FieldKind::Text),
            ],
            defaults: serde_json::to_value(GradleOptions::default()).unwrap_or_default(),
        }]
    }

    fn targets(&self, _context: &RunContext, _path: &Path, _text: &str) -> Vec<RunTarget> {
        Vec::new()
    }

    fn context(&self, _root: &Path, _path: &Path, _text: &str, _offset: usize) -> Option<Configuration> {
        None
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        Ok(vec![self.command_line(configuration, &context.root)?])
    }

    fn debug(&self, configuration: &Configuration, context: &RunContext) -> Option<Result<DebugPlan, String>> {
        Some(self.command_line(configuration, &context.root).and_then(|line| {
            let script = init_script("zen-debug", DEBUG_SCRIPT)?;
            crate::java::debug::gradle_plan(line, &script, &context.root)
        }))
    }

    fn task_kinds(&self) -> Vec<KindSpec> {
        self.kinds()
            .into_iter()
            .map(|kind| KindSpec { id: TASK_TASK.to_owned(), name_message: "workbench-gradle-task".to_owned(), ..kind })
            .collect()
    }

    fn task_lines(&self, task: &BeforeLaunchTask, context: &RunContext) -> Option<Result<Vec<CommandLine>, String>> {
        (task.kind == TASK_TASK).then(|| {
            let configuration = Configuration {
                kind: KIND.to_owned(),
                options: serde_json::Value::Object(task.options.clone()),
                ..Configuration::default()
            };
            self.command_line(&configuration, &context.root).map(|line| vec![line])
        })
    }

    fn adopt_task(&self, foreign: &ForeignTask, root: &Path) -> Option<BeforeLaunchTask> {
        if foreign.id != FOREIGN_TASK_TASK {
            return None;
        }
        let option = |key: &str| foreign.options.get(key).cloned().unwrap_or_default();
        let path = relative_to(root, &option("externalProjectPath"));
        let options =
            GradleOptions { project_path: if path.is_empty() { ".".to_owned() } else { path }, tasks: option("tasks") };
        Some(BeforeLaunchTask::new(TASK_TASK, serde_json::to_value(options).ok()?))
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }

    fn adopt(&self, foreign: &ForeignRunConfiguration, root: &Path) -> Option<Configuration> {
        if foreign.type_id != FOREIGN_TYPE {
            return None;
        }
        let path = relative_to(root, &foreign_option(&foreign.options, "externalProjectPath"));
        let options = GradleOptions {
            project_path: if path.is_empty() { ".".to_owned() } else { path },
            tasks: foreign_list(&foreign.options, "taskNames").join(" "),
        };
        Some(Configuration {
            name: foreign.name.clone(),
            kind: KIND.to_owned(),
            options: serde_json::to_value(options).ok()?,
            common: zen_run::Common {
                env: foreign.env.clone(),
                program_arguments: foreign_option(&foreign.options, "scriptParameters"),
                ..zen_run::Common::default()
            },
            ..Configuration::default()
        })
    }

    fn anything_prefixes(&self) -> Vec<AnythingPrefix> {
        let commands = ["gradle build", "gradle test", "gradle clean", "gradle run", "gradle tasks"];
        vec![AnythingPrefix {
            prefix: "gradle".to_owned(),
            help_message: "workbench-gradle-anything-help".to_owned(),
            commands: commands.iter().map(|command| (*command).to_owned()).collect(),
        }]
    }

    fn anything(&self, words: &[String], cwd: &Path, context: &RunContext) -> Option<Result<Configuration, String>> {
        let (first, tasks) = words.split_first()?;
        if first != "gradle" {
            return None;
        }
        if tasks.is_empty() {
            return Some(Err(t!("workbench-gradle-no-tasks")));
        }
        let builds = ["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"];
        let folder = cwd
            .ancestors()
            .take_while(|folder| folder.starts_with(&context.root))
            .find(|folder| builds.iter().any(|name| folder.join(name).is_file()))
            .and_then(|folder| folder.strip_prefix(&context.root).ok())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .filter(|relative| !relative.is_empty())
            .unwrap_or_else(|| ".".to_owned());
        let mut configuration = GradleRun::tasks("gradle", &folder, &zen_run::external_tool::joined(tasks));
        configuration.name = zen_run::external_tool::joined(words);
        configuration.temporary = true;
        Some(Ok(configuration))
    }
}

fn env(session: &Session) -> BuildEnv {
    session.state::<BuildEnv>().cloned().unwrap_or_default()
}

fn build_root(session: &Session) -> Result<PathBuf, CommandError> {
    let root = project_root(session)?;
    if !model::is_build(&root) {
        return Err(CommandError::new(t!("workbench-gradle-no-build")));
    }
    Ok(root)
}

/// The task list, as far as it is loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskList {
    /// Where its loading is; absent before it was ever asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LoadState>,
    /// The tasks of the last good model, by project; a failed loading keeps them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<model::GradleTask>,
    /// Why it did not load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The background task that loads it (`tasks.log.read`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

fn model_tasks(projects: &[build_model::ModelProject]) -> Vec<model::GradleTask> {
    let mut tasks = Vec::new();
    for project in projects {
        tasks.extend(project.tasks.iter().map(|task| model::GradleTask {
            project: project.path.clone(),
            name: task.name.clone(),
            group: task.group.clone(),
            description: task.description.clone(),
        }));
    }
    tasks
}

fn task_list(env: &BuildEnv, root: &Path, with_tasks: bool) -> TaskList {
    let load = env.load(&tasks_key(root));
    let tasks = match build_model::restore(env, root) {
        Some(kept) if with_tasks => model_tasks(&kept.projects),
        _ => Vec::new(),
    };
    TaskList {
        tasks,
        state: load.as_ref().map(|load| load.state.clone()),
        error: load.as_ref().and_then(|load| load.error.clone()),
        task: load.as_ref().and_then(|load| load.task),
    }
}

/// What `gradle.project.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GradleBuild {
    /// The build's settings script, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_file: Option<PathBuf>,
    /// Its projects, those of its included builds and of `buildSrc`: the root first.
    pub projects: Vec<model::GradleProject>,
    /// The builds linked beside the project's own (`gradle.project.add`).
    pub linked: Vec<PathBuf>,
    /// Whether Gradle works offline.
    pub offline: bool,
    /// Changes when a model of the build or of a linked one changes (`gradle.model.get`).
    pub model_hash: String,
    /// Where the model of each build is: the project's own first, then the linked ones.
    pub model_state: Vec<build_model::ModelState>,
    /// The program Gradle runs with; absent when none is found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<Executable>,
    /// The JDK Gradle runs with; absent when none is known (Gradle then finds Java itself).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_home: Option<PathBuf>,
    /// The JDK it runs with against the one the build asks for.
    pub jdk: BuildJdk,
    /// Why the build does not load as it should, as the tool window's banner says; absent when it
    /// does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<LoadBlock>,
    /// The tasks, as far as they are loaded: their count and where the loading is
    /// (`gradle.tasks.list` gives them).
    pub task_list: TaskList,
    /// How many tasks are loaded.
    pub tasks: usize,
}

fn check(session: &mut Session, env: &BuildEnv, root: &Path, settings: &GradleSettings) -> Checked {
    let tool = ToolCheck {
        found: settings::executable(settings, root, root, env, &env.folders("gradle")).is_some(),
        not_found: t!("workbench-gradle-not-found"),
        download: problems::download_fix(),
        jdk_set_for_tool: !settings.jvm.trim().is_empty(),
        java_home: settings.java_home(env, root),
    };
    crate::build_check::check(session, root, tool)
}

pub struct GetGradleBuild;

impl Command for GetGradleBuild {
    type Params = NoParams;
    type Output = GradleBuild;
    const ID: &'static str = "gradle.project.get";
    const TITLE: &'static str = "Gradle Project";
    const DESCRIPTION: &'static str = "The open project's Gradle build as the Gradle tool window shows it: its projects \
         (from the model Gradle gave, or the settings script until it did, without running Gradle), the Gradle program \
         with its version and where it was found (by default the build's gradlew, the version its wrapper properties \
         name, Gradle on PATH, SDKMAN! or downloaded by the IDE, then the newest working distribution in \
         ~/.gradle/wrapper/dists), the JDK it runs with against the one the build asks for, the linked builds, offline, \
         blocked, the reason the tool window's banner gives why the build does not load, first of toolNotFound, \
         notTrusted, jdkMismatch, with the commands that fix it (project.trust.grant is the person's), and whether its \
         model is loaded (gradle.model.get, gradle.tasks.list; gradle.project.reload loads them). \
         Refused when the project has no settings.gradle(.kts) or build.gradle(.kts) at its root.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<GradleBuild, CommandError> {
        let root = build_root(session)?;
        let env = env(session);
        let settings = GradleSettings::read(&env, &root);
        let executable = settings::executable(&settings, &root, &root, &env, &env.folders("gradle"));
        let list = task_list(&env, &root, true);
        let workspace = GradleWorkspace::read(&env, &root);
        let linked = workspace.linked_folders(&root);
        let mut hashes = Vec::new();
        let mut model_state = Vec::new();
        for folder in std::iter::once(&root).chain(&linked) {
            hashes.extend(build_model::restore(&env, folder).map(|kept| kept.hash.clone()));
            model_state.push(build_model::model_state(&env, folder));
        }
        let projects = build_model::shown_projects(&env, &root).into_iter().map(model::GradleProject::from).collect();
        let Checked { jdk, block: blocked } = check(session, &env, &root, &settings);
        Ok(GradleBuild {
            settings_file: model::settings_file(&root),
            projects,
            linked,
            offline: settings.offline,
            model_hash: hashes.join(" "),
            model_state,
            executable,
            java_home: settings.java_home(&env, &root),
            jdk,
            blocked,
            tasks: list.tasks.len(),
            task_list: TaskList { tasks: Vec::new(), ..list },
        })
    }
}

/// The parameters of `gradle.tasks.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TasksParams {
    /// Load them (again) with Gradle, which may download; without it, only what is loaded.
    #[serde(default)]
    pub load: bool,
    /// Only the tasks of this project (`:app`); all when absent.
    #[serde(default)]
    pub project: Option<String>,
    /// Waits up to this long for them to load (at most 10 minutes).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

fn wait_load(env: &BuildEnv, key: &str, wait: Duration) {
    let end = Instant::now() + wait.min(crate::run::MAX_RUN_WAIT);
    while Instant::now() < end && env.load(key).is_some_and(|load| load.state == LoadState::Loading) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn download_sources(session: &mut Session, env: &BuildEnv, root: &Path) -> Result<u64, CommandError> {
    let script = init_script("zen-sources", SOURCES_SCRIPT).map_err(CommandError::new)?;
    let configuration = GradleRun::tasks("sources", ".", "zenDownloadSources");
    let mut line = GradleRun::new(env.clone()).command_line(&configuration, root).map_err(CommandError::new)?;
    let extra = ["--console=plain".to_owned(), "--no-configuration-cache".to_owned(), "-I".to_owned()];
    line.args.extend(extra.into_iter().chain([script.display().to_string()]));
    line.display = format!("{} --console=plain --no-configuration-cache -I {}", line.display, script.display());
    let task = session.state_mut::<crate::tasks::Tasks>().start(&t!("gradle-sources-task"), "gradle", false);
    let id = task.id();
    env.spawn(&sources_key(root), line, Some(task), |_| Ok(String::new()));
    Ok(id)
}

pub struct ListTasks;

impl Command for ListTasks {
    type Params = TasksParams;
    type Output = TaskList;
    const ID: &'static str = "gradle.tasks.list";
    const TITLE: &'static str = "Gradle Tasks";
    const DESCRIPTION: &'static str = "The build's tasks from its model: project (`:`, `:app`, `:tools:core`), name, group \
         as the build gives it (`other` without one) and description; project filters them. With load Gradle \
         answers again in a background task, which may download Gradle and the build's plug-ins; with waitMs the \
         answer waits for it. state is absent before Gradle was asked; after a failed load the last good tasks stay.";

    fn run(&self, session: &mut Session, params: TasksParams) -> Result<TaskList, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let root = build_root(session)?;
        let env = env(session);
        if params.load {
            crate::trust::guard_build_load(session, &root)?;
            build_model::sync(session, &env, &root, std::slice::from_ref(&root));
        }
        if let Some(wait) = params.wait_ms {
            wait_load(&env, &tasks_key(&root), Duration::from_millis(wait));
        }
        let mut list = task_list(&env, &root, true);
        if let Some(project) = &params.project {
            list.tasks.retain(|task| task.project == *project);
        }
        Ok(list)
    }
}

/// The parameters of `gradle.task.run`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunTaskParams {
    /// The tasks and their options, as typed: `build`, `:app:run`, `clean test --tests 'a.B'`.
    pub tasks: String,
    /// The build's root folder, absolute or relative to the project; the project's own when absent.
    #[serde(default)]
    pub project_path: Option<PathBuf>,
    /// Waits up to this long for the run to end (at most 10 minutes) and answers its summary.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct RunTask;

impl Command for RunTask {
    type Params = RunTaskParams;
    type Output = RunStarted;
    const ID: &'static str = "gradle.task.run";
    const TITLE: &'static str = "Run Gradle Task";
    const DESCRIPTION: &'static str = "Runs Gradle tasks (`build`, `:app:run`, with options) as a double-click in the \
         Gradle tool window does: a temporary run configuration of kind gradle, its output in the Run tool window; \
         returns the run's id, and with waitMs its summary once it ended.";

    fn run(&self, session: &mut Session, params: RunTaskParams) -> Result<RunStarted, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let configuration = task_configuration(session, &params)?;
        crate::trust::guard_run(session, &project_root(session)?, &configuration)?;
        let started = crate::run::start(session, configuration)?;
        match params.wait_ms.map(Duration::from_millis) {
            Some(wait) => Ok(RunStarted { summary: Some(wait_for_run(session, started.run_id, wait)?), ..started }),
            None => Ok(started),
        }
    }
}

fn task_configuration(session: &Session, params: &RunTaskParams) -> Result<Configuration, CommandError> {
    let root = build_root(session)?;
    if params.tasks.trim().is_empty() {
        return Err(CommandError::new(t!("workbench-gradle-no-tasks")));
    }
    let path = workspace::project_path(&root, params.project_path.as_deref());
    let name = model::projects(&root.join(&path)).first().map(|project| project.name.clone()).unwrap_or_default();
    let mut configuration = GradleRun::tasks(&name, &path, params.tasks.trim());
    configuration.temporary = true;
    workspace::remember(session, &root, &params.tasks);
    Ok(configuration)
}

pub struct DebugTask;

impl Command for DebugTask {
    type Params = RunTaskParams;
    type Output = crate::debug::SessionStarted;
    const ID: &'static str = "gradle.task.debug";
    const TITLE: &'static str = "Debug Gradle Task";
    const DESCRIPTION: &'static str = "Debugs Gradle tasks as Debug in the Gradle tool window's menu does: the JVMs the \
         tasks fork (JavaExec tasks such as run, and Test tasks, one at a time) start suspended with a debug agent and \
         the debugger attaches; a task that forks no JVM has nothing to debug. The parameters of gradle.task.run \
         except waitMs; answers the debug session's and the run's ids.";

    fn run(&self, session: &mut Session, params: RunTaskParams) -> Result<crate::debug::SessionStarted, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let configuration = task_configuration(session, &params)?;
        crate::debug::StartSession
            .run(session, crate::run::StartParams { configuration: Some(configuration), ..Default::default() })
    }
}

/// The parameters of `gradle.model.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelParams {
    /// Load what is missing or stale: the cached model while the build files are unchanged, else Gradle in a
    /// background task, which may download. In a project not trusted nothing is loaded.
    #[serde(default)]
    pub load: bool,
    /// Waits up to this long for the project's own build to load (at most 10 minutes).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `gradle.model.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GradleModelAnswer {
    /// The project's own build first, then the linked ones.
    pub builds: Vec<BuildModel>,
    /// Whether the project is trusted to run Gradle.
    pub trusted: bool,
}

pub struct GetModel;

impl Command for GetModel {
    type Params = ModelParams;
    type Output = GradleModelAnswer;
    const ID: &'static str = "gradle.model.get";
    const TITLE: &'static str = "Gradle Build Model";
    const DESCRIPTION: &'static str = "The build as Gradle itself describes it, which the Gradle tool window shows: for \
         the project's build and each linked one, its projects with those of included builds and buildSrc, with \
         their tasks, source sets and resolved configurations (compileClasspath, runtimeClasspath, \
         testCompileClasspath...) as dependency trees. Until Gradle answered, the projects come from the settings \
         script; after a failure the last good model stays and the error is in build.sync.get. With load the model \
         comes from the cache of its build files' hash, else Gradle runs in a background task.";

    fn run(&self, session: &mut Session, params: ModelParams) -> Result<GradleModelAnswer, CommandError> {
        let root = build_root(session)?;
        let env = env(session);
        let folders = workspace::build_folders(&env, &root);
        let trusted = crate::trust::trusted(session, &root);
        if params.load {
            crate::trust::guard_agent_build(session, &root)?;
        }
        if params.load && trusted {
            let (cached, stale): (Vec<PathBuf>, Vec<PathBuf>) =
                folders.iter().cloned().partition(|folder| build_model::fresh(&env, folder));
            cached.iter().for_each(|folder| build_model::mark_cached(&env, folder));
            let idle = |folder: &PathBuf| {
                env.load(&build_model::key(folder)).is_none_or(|load| load.state != LoadState::Loading)
            };
            let stale: Vec<PathBuf> = stale.into_iter().filter(idle).collect();
            if !stale.is_empty() {
                build_model::sync(session, &env, &root, &stale);
            }
        }
        if let Some(wait) = params.wait_ms {
            wait_load(&env, &tasks_key(&root), Duration::from_millis(wait));
        }
        let builds = folders.iter().map(|folder| build_model::build_model(&env, &root, folder)).collect();
        Ok(GradleModelAnswer { builds, trusted })
    }
}

/// The parameters of `gradle.project.reload`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReloadParams {
    /// Load the model too (Gradle runs and may download); on unless `false`.
    #[serde(default)]
    pub tasks: Option<bool>,
    /// Waits up to this long for the task list (at most 10 minutes).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `gradle.project.reload` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reloaded {
    /// Whether the Java language server was started or asked to import the build; `false` when none
    /// is installed and enabled. The step of `build.sync.get` gives the result.
    pub reimported: bool,
    /// The projects read.
    pub projects: usize,
    /// The task list, as far as it is loaded.
    pub task_list: TaskList,
    /// The background task downloading the dependencies' sources, as the Gradle settings ask.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<u64>,
}

pub struct ReloadBuild;

impl Command for ReloadBuild {
    type Params = ReloadParams;
    type Output = Reloaded;
    const ID: &'static str = "gradle.project.reload";
    const TITLE: &'static str = "Reload All Gradle Projects";
    const DESCRIPTION: &'static str = "Reload All Gradle Projects (sync), as the Gradle tool window's first button: reads \
         the settings script again, starts the Java language server when it is installed and enabled, without a file \
         open, and asks it to import the build (reimported says whether it did; the Sync step build.sync.get reports \
         waits for the real result, and says so with Install when the server is not installed), and in background \
         tasks has Gradle write the model of the project's build and of each linked one (gradle.model.get) and resolve \
         the libraries External Libraries lists (unless tasks is false; it may download), and downloads the \
         dependencies' sources in another when the Gradle settings ask. Gradle runs as the Gradle settings say \
         (gradle.settings.get), and tasks set to Execute Before or After Sync run in it. With waitMs it waits for the \
         project's own model.";

    fn run(&self, session: &mut Session, params: ReloadParams) -> Result<Reloaded, CommandError> {
        let root = build_root(session)?;
        let mut sync = SyncRun::start(KIND, &t!("workbench-gradle-kind"), &root);
        let started = Instant::now();
        let env = env(session);
        let mut files: Vec<PathBuf> = model::settings_file(&root).into_iter().collect();
        files.extend(model::builds(&root).into_iter().filter_map(|project| project.build_file));
        sync.step(t!("workbench-sync-gradle-read"), started, Vec::new());
        let settings = GradleSettings::read(&env, &root);
        let block = check(session, &env, &root, &settings).block;
        crate::build_check::note_in_sync(session, &root, block.as_ref(), &mut sync);
        if let Err(refused) = crate::trust::guard_build_load(session, &root) {
            sync.finish(session);
            return Err(refused);
        }
        let reimported = crate::build_sync::java_import(session, &mut sync, &env, &root, &files);
        if params.tasks.unwrap_or(true) {
            let mut folders = workspace::build_folders(&env, &root);
            if let Some(tool) = block.filter(|block| block.kind == BlockKind::ToolNotFound) {
                folders.retain(|folder| *folder != root);
                build_model::start_all(session, &env, &root, &folders, &mut sync);
                sync.step(t!("workbench-sync-gradle-tasks"), Instant::now(), vec![tool.message()]);
                sync.finish(session);
                return Err(CommandError::new(tool.text));
            }
            build_model::start_all(session, &env, &root, &folders, &mut sync);
            let _ = crate::jvm_libraries::load_gradle_libraries(session, &env, &root);
        }
        let sources = if GradleSettings::read(&env, &root).download_sources {
            Some(download_sources(session, &env, &root)?)
        } else {
            None
        };
        sync.finish(session);
        if let Some(wait) = params.wait_ms {
            wait_load(&env, &tasks_key(&root), Duration::from_millis(wait));
            crate::build_sync::wait_imports(session, Duration::from_millis(wait));
        }
        let list = task_list(&env, &root, false);
        let projects = build_model::shown_projects(&env, &root).len();
        Ok(Reloaded { reimported, projects, task_list: list, sources })
    }
}

/// The parameters of `gradle.options.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionsParams {
    /// Work offline (`--offline`), from Gradle's caches only; unchanged when absent.
    #[serde(default)]
    pub offline: Option<bool>,
}

/// What `gradle.options.set` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GradleOptionsState {
    pub offline: bool,
}

pub struct SetOptions;

impl Command for SetOptions {
    type Params = OptionsParams;
    type Output = GradleOptionsState;
    const ID: &'static str = "gradle.options.set";
    const TITLE: &'static str = "Gradle Offline Mode";
    const DESCRIPTION: &'static str = "Toggle Offline Mode: sets whether every Gradle run of the project works offline \
         (--offline: from Gradle's caches, without the network), the setting offline of gradle.settings.get; \
         answers the options as they are now.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<GradleOptionsState, CommandError> {
        let root = project_root(session)?;
        let env = env(session);
        let mut settings = GradleSettings::read(&env, &root);
        if let Some(offline) = params.offline {
            settings.offline = offline;
            env.set_project_settings(settings::TOOL, &root, &settings).map_err(CommandError::new)?;
        }
        Ok(GradleOptionsState { offline: settings.offline })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(SetOptions)?;
    registry.register(GetGradleBuild)?;
    registry.register(ListTasks)?;
    registry.register(RunTask)?;
    registry.register(DebugTask)?;
    registry.register(GetModel)?;
    registry.register(ReloadBuild)?;
    registry.register(download::DownloadGradle)?;
    settings::register(registry)?;
    workspace::register(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests;
