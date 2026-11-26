pub mod analysis;
pub mod dependencies;
pub mod pom;
pub mod pom_edit;
pub mod problems;
pub mod settings;
pub mod tree;
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
use crate::build_sync::{Severity, SyncMessage, SyncRun};
use crate::build_tools::{BuildEnv, Executable, LoadState, foreign_list, foreign_option, relative_to};
use crate::run::{RunStarted, project_root, wait_for_run};
use settings::{MavenSettings, Purpose};
use workspace::MavenWorkspace;

pub const KIND: &str = "maven";

pub const GOAL_TASK: &str = "maven.goal";

const FOREIGN_GOAL_TASK: &str = "Maven.BeforeRunTask";

pub const DOWNLOAD_VERSION: &str = "3.9.16";
#[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "no download in the browser"))]
const DOWNLOAD_SHA512: &str = "831a8591fe20c8243b1dbe7d71e3244f31d1665b0804b2e825e38cbbe5ce0cafb8338851f90780735568773e0a6cd07bbec107cda0b896b008b861075358b6f6";

pub(crate) fn tree_key(root: &Path) -> String {
    format!("maven.dependencyTree\n{}", root.display())
}

/// The options of a `maven` configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MavenOptions {
    /// The `pom.xml`, relative to the project; Maven runs in its folder.
    pub pom: String,
    /// The goals and phases, as typed: `clean install`, `compiler:compile`.
    pub goals: String,
    /// The profiles, as `-P` takes them: `ci,!fast`.
    pub profiles: String,
    /// Work offline (`-o`).
    pub offline: bool,
    /// Skip the tests (`-DskipTests`).
    pub skip_tests: bool,
}

impl Default for MavenOptions {
    fn default() -> Self {
        Self {
            pom: "pom.xml".to_owned(),
            goals: String::new(),
            profiles: String::new(),
            offline: false,
            skip_tests: false,
        }
    }
}

fn options(configuration: &Configuration) -> MavenOptions {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

pub fn arguments(options: &MavenOptions, extra: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    if options.offline {
        args.push("-o".to_owned());
    }
    if !options.profiles.trim().is_empty() {
        args.push("-P".to_owned());
        args.push(options.profiles.trim().to_owned());
    }
    if options.skip_tests {
        args.push("-DskipTests".to_owned());
    }
    args.extend(zen_run::args::split(&options.goals));
    args.extend(extra.iter().cloned());
    args
}

pub fn settings_line(
    env: &BuildEnv,
    settings: &MavenSettings,
    purpose: Purpose,
    root: &Path,
    folders: &[PathBuf],
) -> (Vec<String>, Vec<(String, String)>) {
    let invocation = settings.invocation(purpose);
    let mut vars = env.environment(root, folders);
    if let Some(home) = &invocation.java_home {
        settings::with_java(&mut vars, home);
    }
    vars.extend(invocation.env);
    (invocation.args, vars)
}

#[derive(Clone, Debug, Default)]
pub struct MavenRun {
    env: BuildEnv,
}

impl MavenRun {
    pub fn new(env: BuildEnv) -> Self {
        Self { env }
    }

    pub fn goals(name: &str, pom: &str, goals: &str, profiles: &str) -> Configuration {
        let options = MavenOptions {
            pom: pom.to_owned(),
            goals: goals.to_owned(),
            profiles: profiles.to_owned(),
            ..MavenOptions::default()
        };
        Configuration {
            name: format!("{name} [{goals}]"),
            kind: KIND.to_owned(),
            options: serde_json::to_value(options).unwrap_or_default(),
            ..Configuration::default()
        }
    }

    pub fn command_line(&self, configuration: &Configuration, root: &Path) -> Result<CommandLine, String> {
        self.line_for(configuration, root, Purpose::Goal)
    }

    pub fn line_for(
        &self,
        configuration: &Configuration,
        root: &Path,
        purpose: Purpose,
    ) -> Result<CommandLine, String> {
        let options = options(configuration);
        if options.goals.trim().is_empty() {
            return Err(t!("workbench-maven-no-goals"));
        }
        let pom = if options.pom.trim().is_empty() { "pom.xml" } else { options.pom.trim() };
        let pom = root.join(pom);
        let folder = pom.parent().map_or_else(|| root.to_owned(), Path::to_owned);
        let folders = self.env.folders("maven");
        let settings = MavenSettings::read(&self.env, root);
        let found = settings::executable(&settings, &folder, root, &self.env, &folders)
            .ok_or_else(|| t!("workbench-maven-not-found"))?;
        let (mut args, mut env) = settings_line(&self.env, &settings, purpose, root, &folders);
        args.extend(arguments(&options, &zen_run::args::split(&configuration.common.program_arguments)));
        if pom.file_name().is_some_and(|name| name != "pom.xml") {
            args.splice(0..0, ["-f".to_owned(), pom.to_string_lossy().into_owned()]);
        }
        let cwd = match &configuration.common.working_directory {
            Some(dir) if !dir.as_os_str().is_empty() => root.join(dir),
            _ => folder,
        };
        env.extend(configuration.common.env.iter().cloned());
        let shown = if found.wrapper { "./mvnw".to_owned() } else { "mvn".to_owned() };
        let display = std::iter::once(shown).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        Ok(CommandLine { program: found.path, args, cwd, env, display })
    }
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

const FOREIGN_TYPE: &str = "MavenRunConfiguration";

impl RunProvider for MavenRun {
    fn id(&self) -> &str {
        KIND
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: KIND.to_owned(),
            name_message: "workbench-maven-kind".to_owned(),
            fields: vec![
                field("pom", "workbench-maven-field-pom", FieldKind::Path),
                field("goals", "workbench-maven-field-goals", FieldKind::Text),
                field("profiles", "workbench-maven-field-profiles", FieldKind::Text),
                field("offline", "workbench-maven-field-offline", FieldKind::Bool),
                field("skipTests", "workbench-maven-field-skip-tests", FieldKind::Bool),
            ],
            defaults: serde_json::to_value(MavenOptions::default()).unwrap_or_default(),
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
        Some(
            self.command_line(configuration, &context.root)
                .and_then(|line| crate::java::debug::options_plan(line, "MAVEN_OPTS", &context.root)),
        )
    }

    fn task_kinds(&self) -> Vec<KindSpec> {
        self.kinds()
            .into_iter()
            .map(|kind| KindSpec { id: GOAL_TASK.to_owned(), name_message: "workbench-maven-task".to_owned(), ..kind })
            .collect()
    }

    fn task_lines(&self, task: &BeforeLaunchTask, context: &RunContext) -> Option<Result<Vec<CommandLine>, String>> {
        (task.kind == GOAL_TASK).then(|| {
            let configuration = Configuration {
                kind: KIND.to_owned(),
                options: serde_json::Value::Object(task.options.clone()),
                ..Configuration::default()
            };
            self.command_line(&configuration, &context.root).map(|line| vec![line])
        })
    }

    fn adopt_task(&self, foreign: &ForeignTask, root: &Path) -> Option<BeforeLaunchTask> {
        if foreign.id != FOREIGN_GOAL_TASK {
            return None;
        }
        let option = |key: &str| foreign.options.get(key).cloned().unwrap_or_default();
        let pom = relative_to(root, &option("file"));
        let options = MavenOptions {
            pom: if pom.is_empty() { "pom.xml".to_owned() } else { pom },
            goals: option("goal"),
            ..MavenOptions::default()
        };
        Some(BeforeLaunchTask::new(GOAL_TASK, serde_json::to_value(options).ok()?))
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }

    fn adopt(&self, foreign: &ForeignRunConfiguration, root: &Path) -> Option<Configuration> {
        if foreign.type_id != FOREIGN_TYPE {
            return None;
        }
        let goals = foreign_list(&foreign.options, "goals");
        let folder = relative_to(root, &foreign_option(&foreign.options, "workingDirPath"));
        let pom_name = foreign_option(&foreign.options, "pomFileName");
        let pom_name = if pom_name.is_empty() { "pom.xml".to_owned() } else { pom_name };
        let pom = if folder.is_empty() || folder == "." { pom_name } else { format!("{folder}/{pom_name}") };
        let mut profiles: Vec<String> = foreign
            .options
            .iter()
            .filter_map(|(key, value)| {
                let (_, name) = key.split_once("profilesMap.map.")?;
                Some(if value == "false" { format!("!{name}") } else { name.to_owned() })
            })
            .collect();
        profiles.extend(foreign_list(&foreign.options, "profiles"));
        let options = MavenOptions {
            pom,
            goals: goals.join(" "),
            profiles: profiles.join(","),
            offline: foreign_option(&foreign.options, "offlineMode") == "true",
            skip_tests: foreign_option(&foreign.options, "skipTests") == "true",
        };
        Some(Configuration {
            name: foreign.name.clone(),
            kind: KIND.to_owned(),
            options: serde_json::to_value(options).ok()?,
            common: zen_run::Common {
                env: foreign.env.clone(),
                program_arguments: foreign_option(&foreign.options, "cmdOptions"),
                ..zen_run::Common::default()
            },
            ..Configuration::default()
        })
    }

    fn anything_prefixes(&self) -> Vec<AnythingPrefix> {
        let commands = ["mvn clean", "mvn compile", "mvn test", "mvn package", "mvn install", "mvn verify"];
        vec![AnythingPrefix {
            prefix: "mvn".to_owned(),
            help_message: "workbench-maven-anything-help".to_owned(),
            commands: commands.iter().map(|command| (*command).to_owned()).collect(),
        }]
    }

    fn anything(&self, words: &[String], cwd: &Path, context: &RunContext) -> Option<Result<Configuration, String>> {
        let (first, goals) = words.split_first()?;
        if first != "mvn" {
            return None;
        }
        if goals.is_empty() {
            return Some(Err(t!("workbench-maven-no-goals")));
        }
        let pom = cwd.ancestors().take_while(|folder| folder.starts_with(&context.root)).find_map(|folder| {
            let pom = folder.join("pom.xml");
            pom.is_file().then(|| relative_to(&context.root, &pom.to_string_lossy()))
        });
        let goals = zen_run::external_tool::joined(goals);
        let mut configuration = MavenRun::goals("maven", &pom.unwrap_or_else(|| "pom.xml".to_owned()), &goals, "");
        configuration.name = zen_run::external_tool::joined(words);
        configuration.temporary = true;
        Some(Ok(configuration))
    }
}

fn env(session: &Session) -> BuildEnv {
    session.state::<BuildEnv>().cloned().unwrap_or_default()
}

fn root_pom(session: &Session) -> Result<(PathBuf, PathBuf), CommandError> {
    let root = project_root(session)?;
    let pom = root.join("pom.xml");
    if !pom.is_file() {
        return Err(CommandError::new(t!("workbench-maven-no-pom")));
    }
    Ok((root, pom))
}

/// The dependency tree, as far as it is loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencyTree {
    /// Where its loading is; absent before it was ever asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LoadState>,
    /// One root per project of the build, once loaded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<tree::TreeNode>,
    /// Why it did not load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The background task that loads it (`tasks.log.read`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

fn dependency_tree(env: &BuildEnv, root: &Path, with_roots: bool) -> DependencyTree {
    match env.load(&tree_key(root)) {
        None => DependencyTree { state: None, roots: Vec::new(), error: None, task: None },
        Some(load) => DependencyTree {
            roots: if with_roots && load.state == LoadState::Loaded { tree::parse(&load.output) } else { Vec::new() },
            state: Some(load.state),
            error: load.error,
            task: load.task,
        },
    }
}

/// What `maven.project.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MavenProject {
    /// The project's `pom.xml`, read with its modules.
    pub project: pom::PomProject,
    /// The lifecycle phases the tool window lists.
    pub lifecycle: Vec<String>,
    /// The profiles chosen for runs from the tool window.
    pub selected_profiles: Vec<String>,
    /// Whether runs work offline (`-o`): Work offline of the Maven settings.
    pub offline: bool,
    /// Whether runs skip the tests: Skip tests of the Maven settings' Runner.
    pub skip_tests: bool,
    /// The program Maven runs with; absent when none is found (`maven.distribution.download`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<Executable>,
    /// The JDK Maven runs with; absent when none is known (Maven then finds Java itself).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_home: Option<PathBuf>,
    /// The JDK it runs with against the one the build asks for.
    pub jdk: BuildJdk,
    /// Why the project does not load as it should, as the tool window's banner says; absent when it
    /// does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<LoadBlock>,
    /// Where the dependency tree is (`maven.dependencies.tree` gives it).
    pub dependency_tree: DependencyTree,
    /// Whether the project's own `pom.xml` is linked; `false` after Unlink (`maven.project.add` links it again).
    pub root_linked: bool,
    /// The projects linked beside it (`maven.project.add`), read with their modules.
    pub linked: Vec<pom::PomProject>,
    /// The `pom.xml` files ignored (`maven.project.ignore`).
    pub ignored: Vec<PathBuf>,
    /// The tool window's view options (`maven.view.set`).
    pub view: workspace::ViewOptions,
    /// The goals run from the tool window and Execute Maven Goal, latest first.
    pub history: Vec<String>,
    /// The user `settings.xml` Maven reads (`maven.settingsFile.open`).
    pub user_settings: PathBuf,
    /// Whether it exists.
    pub user_settings_exists: bool,
}

fn check(session: &mut Session, env: &BuildEnv, root: &Path, settings: &MavenSettings) -> Checked {
    let folders = env.folders("maven");
    let chosen = settings.invocation(Purpose::Goal).java_home;
    let tool = ToolCheck {
        found: settings::executable(settings, root, root, env, &folders).is_some(),
        not_found: t!("workbench-maven-not-found"),
        download: problems::download_fix(),
        jdk_set_for_tool: chosen.is_some(),
        java_home: chosen.or_else(|| env.java_home(root)),
    };
    crate::build_check::check(session, root, tool)
}

fn selected_profiles(session: &Session, project: &pom::PomProject) -> Vec<String> {
    let chosen = project_root(session).ok().and_then(|root| MavenWorkspace::read(&env(session), &root).profiles);
    match chosen {
        Some(chosen) => chosen,
        None => project.profiles.iter().filter(|profile| profile.active_by_default).map(|p| p.id.clone()).collect(),
    }
}

pub struct GetMavenProject;

impl Command for GetMavenProject {
    type Params = NoParams;
    type Output = MavenProject;
    const ID: &'static str = "maven.project.get";
    const TITLE: &'static str = "Maven Project";
    const DESCRIPTION: &'static str = "The open project's Maven build as the Maven tool window shows it, read from its \
         pom.xml files without running Maven: coordinates, modules, profiles (and those chosen for runs), declared \
         dependencies, plug-ins with their goals, the lifecycle phases; the Maven program with its version and where it \
         was found (as the Maven settings choose it: by default the project's mvnw, the version its wrapper \
         properties name, Maven on PATH, SDKMAN! or downloaded by the IDE, then the newest working distribution in \
         ~/.m2/wrapper/dists), the JDK it runs with against the one the build asks for; blocked, the reason the \
         tool window's banner gives why the project does not load, first of toolNotFound, notTrusted, jdkMismatch, \
         with the commands that fix it (project.trust.grant is the person's); whether the dependency tree is \
         loaded (maven.dependencies.tree); the projects linked beside it and those ignored, the view options and the \
         goals run before. The profiles chosen and the rest of the tool window's state are kept with the project on \
         this computer. Refused without a pom.xml at the project's root.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<MavenProject, CommandError> {
        let (root, pom_file) = root_pom(session)?;
        let project = pom::read(&pom_file);
        let env = env(session);
        let folders = env.folders("maven");
        let settings = MavenSettings::read(&env, &root);
        let executable = settings::executable(&settings, &root, &root, &env, &folders);
        let java_home = settings.invocation(Purpose::Goal).java_home.or_else(|| env.java_home(&root));
        let kept = MavenWorkspace::read(&env, &root);
        let phases: &[&str] = if kept.view.basic_phases { &pom::LIFECYCLE } else { &pom::FULL_LIFECYCLE };
        let Checked { jdk, block: blocked } = check(session, &env, &root, &settings);
        Ok(MavenProject {
            selected_profiles: selected_profiles(session, &project),
            project,
            lifecycle: phases.iter().map(|phase| (*phase).to_owned()).collect(),
            root_linked: !kept.root_unlinked,
            linked: kept.linked_poms(&root).iter().map(|pom| pom::read(pom)).collect(),
            ignored: kept.ignored_poms(&root),
            view: kept.view,
            history: kept.history,
            user_settings_exists: settings.user_settings_file(&env.home()).is_file(),
            user_settings: settings.user_settings_file(&env.home()),
            offline: settings.offline,
            skip_tests: settings.runner.skip_tests,
            executable,
            java_home,
            jdk,
            blocked,
            dependency_tree: dependency_tree(&env, &root, false),
        })
    }
}

/// The parameters of `maven.goal.run`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunGoalParams {
    /// The goals and phases, as typed: `clean install`, `compiler:compile`, `exec:java`.
    pub goals: String,
    /// The `pom.xml` of the module to run them in, absolute or relative to the project; the
    /// project's own when absent.
    #[serde(default)]
    pub pom: Option<PathBuf>,
    /// The profiles (`ci,!fast`); the tool window's chosen ones when absent.
    #[serde(default)]
    pub profiles: Option<String>,
    /// Work offline; the Maven settings' Work offline when absent.
    #[serde(default)]
    pub offline: Option<bool>,
    /// Skip the tests; the Maven settings' Skip tests when absent.
    #[serde(default)]
    pub skip_tests: Option<bool>,
    /// Waits up to this long for the run to end (at most 10 minutes) and answers its summary.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct RunGoal;

impl Command for RunGoal {
    type Params = RunGoalParams;
    type Output = RunStarted;
    const ID: &'static str = "maven.goal.run";
    const TITLE: &'static str = "Run Maven Goal";
    const DESCRIPTION: &'static str = "Runs Maven goals or phases (`clean install`, `compiler:compile`) in the project \
         or one of its modules (pom), as a double-click in the Maven tool window does: a temporary run configuration \
         of kind maven, its output in the Run tool window; returns the run's id, and with waitMs its summary once it \
         ended. Profiles default to the tool window's choice, offline and skipTests to the Maven settings; the other \
         Maven settings (maven.settings.get) apply too.";

    fn run(&self, session: &mut Session, params: RunGoalParams) -> Result<RunStarted, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let configuration = goal_configuration(session, &params)?;
        crate::trust::guard_run(session, &project_root(session)?, &configuration)?;
        let started = crate::run::start(session, configuration)?;
        match params.wait_ms.map(Duration::from_millis) {
            Some(wait) => Ok(RunStarted { summary: Some(wait_for_run(session, started.run_id, wait)?), ..started }),
            None => Ok(started),
        }
    }
}

fn goal_configuration(session: &mut Session, params: &RunGoalParams) -> Result<Configuration, CommandError> {
    let (root, root_file) = root_pom(session)?;
    if params.goals.trim().is_empty() {
        return Err(CommandError::new(t!("workbench-maven-no-goals")));
    }
    let pom_file = match &params.pom {
        Some(pom) if pom.is_absolute() => pom.clone(),
        Some(pom) => root.join(pom),
        None => root_file.clone(),
    };
    let pom_file = if pom_file.is_dir() { pom_file.join("pom.xml") } else { pom_file };
    if !pom_file.is_file() {
        return Err(CommandError::new(t!("workbench-maven-no-module", pom = pom_file.display().to_string())));
    }
    let project = pom::read(&pom_file);
    let settings = MavenSettings::read(&env(session), &root);
    let profiles =
        params.profiles.clone().unwrap_or_else(|| selected_profiles(session, &pom::read(&root_file)).join(","));
    let relative = relative_to(&root, &pom_file.to_string_lossy());
    let name = if project.artifact_id.is_empty() { "maven".to_owned() } else { project.artifact_id.clone() };
    let mut configuration = MavenRun::goals(&name, &relative, params.goals.trim(), &profiles);
    configuration.temporary = true;
    configuration.options["offline"] = serde_json::json!(params.offline.unwrap_or(settings.offline));
    configuration.options["skipTests"] = serde_json::json!(params.skip_tests.unwrap_or(settings.runner.skip_tests));
    workspace::remember(session, &root, &params.goals);
    Ok(configuration)
}

pub struct DebugGoal;

impl Command for DebugGoal {
    type Params = RunGoalParams;
    type Output = crate::debug::SessionStarted;
    const ID: &'static str = "maven.goal.debug";
    const TITLE: &'static str = "Debug Maven Goal";
    const DESCRIPTION: &'static str = "Debugs Maven goals or phases as Debug in the Maven tool window's menu does: \
         Maven's own JVM starts suspended with a debug agent (MAVEN_OPTS) and the debugger attaches, so breakpoints \
         in code that runs inside Maven (exec:java, plug-ins, tests run without a fork) stop. The parameters of \
         maven.goal.run except waitMs; answers the debug session's and the run's ids.";

    fn run(&self, session: &mut Session, params: RunGoalParams) -> Result<crate::debug::SessionStarted, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let configuration = goal_configuration(session, &params)?;
        crate::debug::StartSession
            .run(session, crate::run::StartParams { configuration: Some(configuration), ..Default::default() })
    }
}

/// The parameters of `maven.options.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionsParams {
    /// The profiles chosen for runs from the tool window, by id; `[]` for none.
    #[serde(default)]
    pub profiles: Option<Vec<String>>,
    /// Toggle Offline Mode.
    #[serde(default)]
    pub offline: Option<bool>,
    /// Skip Tests.
    #[serde(default)]
    pub skip_tests: Option<bool>,
}

/// What `maven.options.set` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MavenOptionsState {
    /// The profiles chosen.
    pub profiles: Vec<String>,
    /// Whether runs work offline.
    pub offline: bool,
    /// Whether runs skip the tests.
    pub skip_tests: bool,
}

pub struct SetOptions;

impl Command for SetOptions {
    type Params = OptionsParams;
    type Output = MavenOptionsState;
    const ID: &'static str = "maven.options.set";
    const TITLE: &'static str = "Maven Options";
    const DESCRIPTION: &'static str = "Sets what the Maven tool window's runs use: the profiles chosen (the ticks of its \
         Profiles node; those active by default until any is chosen), Toggle Offline Mode (-o) and Skip Tests \
         (-DskipTests); what is not given stays. Offline and skip tests are the project's Maven settings `offline` \
         and `runner.skipTests` (maven.settings.set), kept with the project.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<MavenOptionsState, CommandError> {
        let (root, pom_file) = root_pom(session)?;
        if let Some(profiles) = params.profiles {
            let mut kept = MavenWorkspace::read(&env(session), &root);
            kept.profiles = Some(profiles);
            kept.save(&env(session), &root)?;
        }
        let mut settings = MavenSettings::read(&env(session), &root);
        if params.offline.is_some() || params.skip_tests.is_some() {
            settings.offline = params.offline.unwrap_or(settings.offline);
            settings.runner.skip_tests = params.skip_tests.unwrap_or(settings.runner.skip_tests);
            settings::save(session, &root, &settings)?;
        }
        let project = pom::read(&pom_file);
        Ok(MavenOptionsState {
            profiles: selected_profiles(session, &project),
            offline: settings.offline,
            skip_tests: settings.runner.skip_tests,
        })
    }
}

/// The parameters of `maven.project.reload`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReloadParams {
    /// Load the dependency tree too (`mvn dependency:tree`, which may download); on unless `false`.
    #[serde(default)]
    pub dependencies: Option<bool>,
    /// Waits up to this long for the dependency tree (at most 10 minutes).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `maven.project.reload` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reloaded {
    /// Whether the Java language server was started or asked to import the project; `false` when none
    /// is installed and enabled. The step of `build.sync.get` gives the result.
    pub reimported: bool,
    /// The modules read.
    pub modules: usize,
    /// The dependency tree, as far as it is loaded.
    pub dependency_tree: DependencyTree,
    /// The background tasks downloading the dependencies' sources and documentation, as the Maven
    /// settings' Importing asks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub downloads: Vec<u64>,
}

pub struct ReloadProject;

impl Command for ReloadProject {
    type Params = ReloadParams;
    type Output = Reloaded;
    const ID: &'static str = "maven.project.reload";
    const TITLE: &'static str = "Reload All Maven Projects";
    const DESCRIPTION: &'static str = "Reload All Maven Projects, as the Maven tool window's first button: reads the \
         pom.xml files again, starts the Java language server when it is installed and enabled, without a file open, \
         and asks it to import the build (reimported says whether it did; the Sync step build.sync.get reports waits \
         for the real result, and says so with Install when the server is not installed), and loads the dependency tree with mvn dependency:tree in a background task (unless \
         dependencies is false; it may download), downloading the sources and documentation too when the Maven \
         settings' Importing asks. Maven runs with the importer's JDK and VM options. With waitMs it waits for the \
         tree and the import.";

    fn run(&self, session: &mut Session, params: ReloadParams) -> Result<Reloaded, CommandError> {
        let (root, pom_file) = root_pom(session)?;
        let mut sync = SyncRun::start(KIND, &t!("workbench-maven-kind"), &root);
        let started = Instant::now();
        let project = pom::read(&pom_file);
        sync.step(t!("workbench-sync-maven-read"), started, problems::pom_errors(&project));
        let env = env(session);
        let settings = MavenSettings::read(&env, &root);
        let block = check(session, &env, &root, &settings).block;
        crate::build_check::note_in_sync(session, &root, block.as_ref(), &mut sync);
        if let Err(refused) = crate::trust::guard_build_load(session, &root) {
            sync.finish(session);
            return Err(refused);
        }
        let files = pom_files(&project);
        let reimported = crate::build_sync::java_import(session, &mut sync, &env, &root, &files);
        let mut downloads = Vec::new();
        if params.dependencies.unwrap_or(true) {
            let started = Instant::now();
            let title = t!("workbench-sync-maven-dependencies");
            if let Err(error) = load_tree(session, &env, &root, &project, true) {
                let tool = block.filter(|block| block.kind == BlockKind::ToolNotFound);
                let message =
                    tool.map_or_else(|| SyncMessage::new(Severity::Error, error.to_string()), |tool| tool.message());
                sync.step(title, started, vec![message]);
                sync.finish(session);
                return Err(error);
            }
            sync.background(title, tree_key(&root), problems::problems);
            let settings = MavenSettings::read(&env, &root);
            downloads = download_attachments(
                session,
                &env,
                &root,
                settings.importing.download_sources,
                settings.importing.download_documentation,
            )?;
        }
        sync.finish(session);
        if let Some(wait) = params.wait_ms {
            wait_load(&env, &tree_key(&root), Duration::from_millis(wait));
            crate::build_sync::wait_imports(session, Duration::from_millis(wait));
        }
        Ok(Reloaded {
            reimported,
            modules: count_modules(&project),
            dependency_tree: dependency_tree(&env, &root, false),
            downloads,
        })
    }
}

fn pom_files(project: &pom::PomProject) -> Vec<PathBuf> {
    std::iter::once(project.pom.clone()).chain(project.modules.iter().flat_map(pom_files)).collect()
}

fn count_modules(project: &pom::PomProject) -> usize {
    project.modules.iter().map(|module| 1 + count_modules(module)).sum()
}

fn wait_load(env: &BuildEnv, key: &str, wait: Duration) {
    let end = Instant::now() + wait.min(crate::run::MAX_RUN_WAIT);
    while Instant::now() < end && env.load(key).is_some_and(|load| load.state == LoadState::Loading) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn load_tree(
    session: &mut Session,
    env: &BuildEnv,
    root: &Path,
    project: &pom::PomProject,
    again: bool,
) -> Result<(), CommandError> {
    if !again && env.load(&tree_key(root)).is_some_and(|load| load.state != LoadState::Failed) {
        return Ok(());
    }
    let scratch = std::env::temp_dir().join(format!("zen-maven-tree-{}-{}", std::process::id(), {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        root.hash(&mut hasher);
        hasher.finish()
    }));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|error| CommandError::new(error.to_string()))?;
    let settings = MavenSettings::read(env, root);
    let mut configuration = MavenRun::goals(
        &project.artifact_id,
        "pom.xml",
        "dependency:tree",
        &selected_profiles(session, project).join(","),
    );
    configuration.options["offline"] = serde_json::json!(settings.offline);
    let mut line =
        MavenRun::new(env.clone()).line_for(&configuration, root, Purpose::Import).map_err(CommandError::new)?;
    let output = format!("-DoutputFile={}", scratch.join(TREE_FILE).display());
    line.display = format!("{} -B {output}", line.display);
    line.args.extend(["-B".to_owned(), output]);
    let order = tree_order(project);
    let title = t!("workbench-maven-tree-task");
    let task = session.state_mut::<crate::tasks::Tasks>().start(&title, "maven", false);
    env.spawn(&tree_key(root), line, Some(task), move |_| {
        let text = gather_trees(&scratch, &order);
        let _ = std::fs::remove_dir_all(&scratch);
        text
    });
    Ok(())
}

const TREE_FILE: &str = "${project.groupId}.${project.artifactId}.txt";

fn tree_order(project: &pom::PomProject) -> Vec<String> {
    let own = format!("{}.{}.txt", project.group_id, project.artifact_id);
    std::iter::once(own).chain(project.modules.iter().flat_map(tree_order)).collect()
}

fn gather_trees(folder: &Path, order: &[String]) -> Result<String, String> {
    let entries = std::fs::read_dir(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let mut files: Vec<(usize, String, PathBuf)> = entries
        .flatten()
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let at = order.iter().position(|known| *known == name).unwrap_or(order.len());
            (at, name, entry.path())
        })
        .collect();
    files.sort();
    let mut text = String::new();
    for (_, _, file) in files {
        text.push_str(&std::fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?);
        text.push('\n');
    }
    Ok(text)
}

fn download_attachments(
    session: &mut Session,
    env: &BuildEnv,
    root: &Path,
    sources: bool,
    documentation: bool,
) -> Result<Vec<u64>, CommandError> {
    let settings = MavenSettings::read(env, root);
    let mut tasks = Vec::new();
    for (wanted, goals, key, title) in [
        (sources, "dependency:sources", "maven.sources", "workbench-maven-sources-task"),
        (
            documentation,
            "dependency:resolve -Dclassifier=javadoc",
            "maven.documentation",
            "workbench-maven-documentation-task",
        ),
    ] {
        if !wanted {
            continue;
        }
        let mut configuration = MavenRun::goals("maven", "pom.xml", goals, "");
        configuration.options["offline"] = serde_json::json!(settings.offline);
        let mut line =
            MavenRun::new(env.clone()).line_for(&configuration, root, Purpose::Import).map_err(CommandError::new)?;
        line.args.push("-B".to_owned());
        line.display.push_str(" -B");
        let task = session.state_mut::<crate::tasks::Tasks>().start(&t!(title), "maven", false);
        tasks.push(task.id());
        env.spawn(&format!("{key}\n{}", root.display()), line, Some(task), |_| Ok(String::new()));
    }
    Ok(tasks)
}

/// The parameters of `maven.dependencies.tree`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TreeParams {
    /// Load it (again) with `mvn dependency:tree`, which may download; without it, only what is loaded.
    #[serde(default)]
    pub load: bool,
    /// Waits up to this long for it to load (at most 10 minutes).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct GetDependencyTree;

impl Command for GetDependencyTree {
    type Params = TreeParams;
    type Output = DependencyTree;
    const ID: &'static str = "maven.dependencies.tree";
    const TITLE: &'static str = "Show Dependencies";
    const DESCRIPTION: &'static str = "The project's dependency tree as mvn dependency:tree resolves it: one root per \
         project of the build, each dependency (group, artifact, type, version, scope, Maven's note) under the one \
         that brings it. With load it is loaded (again) in a background task, which may download; with waitMs the \
         answer waits for it. state is absent before it was ever loaded.";

    fn run(&self, session: &mut Session, params: TreeParams) -> Result<DependencyTree, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let (root, pom_file) = root_pom(session)?;
        let env = env(session);
        if params.load {
            crate::trust::guard_build_load(session, &root)?;
            load_tree(session, &env, &root, &pom::read(&pom_file), true)?;
        }
        if let Some(wait) = params.wait_ms {
            wait_load(&env, &tree_key(&root), Duration::from_millis(wait));
        }
        Ok(dependency_tree(&env, &root, true))
    }
}

/// The parameters of `maven.dependency.open`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenDependencyParams {
    /// A `pom.xml` and a character offset in it: the dependency, plug-in or parent there; the
    /// active editor's file and caret when neither coordinates nor a path are given.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset in `path`.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Or the coordinates themselves: `groupId`.
    #[serde(default)]
    pub group_id: Option<String>,
    /// `artifactId`.
    #[serde(default)]
    pub artifact_id: Option<String>,
    /// `version`.
    #[serde(default)]
    pub version: Option<String>,
}

/// What `maven.dependency.open` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDependency {
    /// `group:artifact:version`.
    pub coordinates: String,
    /// Its POM in the local repository, opened in the editor.
    pub pom: PathBuf,
}

pub struct OpenDependency;

impl Command for OpenDependency {
    type Params = OpenDependencyParams;
    type Output = OpenedDependency;
    const ID: &'static str = "maven.dependency.open";
    const TITLE: &'static str = "Go to Dependency POM";
    const DESCRIPTION: &'static str = "Opens the POM of a dependency, plug-in or parent from the local Maven repository \
         (the Maven settings' local repository, else settings.xml's localRepository, else ~/.m2/repository): the \
         one at offset of a pom.xml (path; the active editor's caret when absent), or the coordinates given. \
         Refused when it is not downloaded yet (Reload downloads it) or the version is not written in the file.";

    fn run(&self, session: &mut Session, params: OpenDependencyParams) -> Result<OpenedDependency, CommandError> {
        let (group, artifact, version) = match (&params.group_id, &params.artifact_id, &params.version) {
            (Some(group), Some(artifact), Some(version)) => (group.clone(), artifact.clone(), version.clone()),
            _ => {
                let root = project_root(session)?;
                let path = match &params.path {
                    Some(path) if path.is_absolute() => path.clone(),
                    Some(path) => root.join(path),
                    None => session
                        .state::<crate::Workbench>()
                        .and_then(crate::Workbench::active_document)
                        .map(|document| document.path().to_owned())
                        .ok_or_else(|| CommandError::new(t!("workbench-maven-no-coordinates")))?,
                };
                let (text, head) = crate::run::file_text(session, &path)?;
                let offset = params.offset.or(head).unwrap_or_default();
                let offset = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
                pom::coordinates_at(&text, offset)
                    .ok_or_else(|| CommandError::new(t!("workbench-maven-no-coordinates")))?
            }
        };
        if group.is_empty() || artifact.is_empty() || version.is_empty() || version.contains("${") {
            return Err(CommandError::new(t!(
                "workbench-maven-no-version",
                coordinates = format!("{group}:{artifact}")
            )));
        }
        let env = env(session);
        let repository = match project_root(session) {
            Ok(root) => MavenSettings::read(&env, &root),
            Err(_) => MavenSettings::default(),
        }
        .local_repository(&env.home());
        let found = pom::local_pom(&repository, &group, &artifact, &version);
        let coordinates = format!("{group}:{artifact}:{version}");
        if !found.is_file() {
            return Err(CommandError::new(t!("workbench-maven-not-downloaded", coordinates = coordinates.as_str())));
        }
        crate::OpenDocument
            .run(session, crate::OpenParams { path: found.clone(), editor_only: true, group: None, preview: false })?;
        Ok(OpenedDependency { coordinates, pom: found })
    }
}

/// What `maven.distribution.download` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    /// The version downloaded.
    pub version: String,
    /// Where it is unpacked; its `bin/mvn` is used where no wrapper is.
    pub folder: PathBuf,
    /// The background task that downloads it (`tasks.log.read`), when it is not there already.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

pub struct DownloadMaven;

impl Command for DownloadMaven {
    type Params = NoParams;
    type Output = Download;
    const ID: &'static str = "maven.distribution.download";
    const TITLE: &'static str = "Download Maven";
    const DESCRIPTION: &'static str = "Downloads Apache Maven 3.9.16 from Maven Central (checked against its SHA-512) \
         into the IDE's data folder, in a background task, for projects without a Maven Wrapper on a computer \
         without Maven: the network is used only when this command runs.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Download, CommandError> {
        let env = env(session);
        let data = env.data_dir().ok_or_else(|| CommandError::new(t!("workbench-maven-download-no-folder")))?;
        let folder = data.join("tools/maven").join(DOWNLOAD_VERSION);
        if folder.join("bin/mvn").is_file() {
            return Ok(Download { version: DOWNLOAD_VERSION.to_owned(), folder, task: None });
        }
        let title = t!("workbench-maven-download-task", version = DOWNLOAD_VERSION);
        let task = session.state_mut::<crate::tasks::Tasks>().start(&title, "maven", true);
        let id = task.id();
        let target = folder.clone();
        std::thread::spawn(move || task.finish(download(&target, &task)));
        Ok(Download { version: DOWNLOAD_VERSION.to_owned(), folder, task: Some(id) })
    }
}

#[cfg(target_arch = "wasm32")]
fn download(_folder: &Path, _task: &crate::tasks::TaskHandle) -> Result<String, String> {
    Err(t!("workbench-maven-download-unavailable"))
}

#[cfg(not(target_arch = "wasm32"))]
fn download(folder: &Path, task: &crate::tasks::TaskHandle) -> Result<String, String> {
    use sha2::Digest;
    let url = format!(
        "https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/{v}/apache-maven-{v}-bin.tar.gz",
        v = DOWNLOAD_VERSION
    );
    task.log(&url);
    let bytes = zen_update::fetch::document(&url, 64 * 1024 * 1024, Duration::from_secs(600))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{url}: not found"))?;
    let digest = hex::encode(sha2::Sha512::digest(&bytes));
    if digest != DOWNLOAD_SHA512 {
        return Err(t!("workbench-maven-download-checksum", url = url.as_str()));
    }
    let unpacked = folder.with_extension("partial");
    let _ = std::fs::remove_dir_all(&unpacked);
    zen_update::install::unpack_tar_gz(&bytes, &unpacked).map_err(|error| error.to_string())?;
    let inner = unpacked.join(format!("apache-maven-{DOWNLOAD_VERSION}"));
    let _ = std::fs::remove_dir_all(folder);
    if let Some(parent) = folder.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&inner, folder).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_dir_all(&unpacked);
    Ok(t!("workbench-maven-download-done", folder = folder.display().to_string()))
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetMavenProject)?;
    registry.register(RunGoal)?;
    registry.register(SetOptions)?;
    registry.register(ReloadProject)?;
    registry.register(GetDependencyTree)?;
    registry.register(OpenDependency)?;
    registry.register(DownloadMaven)?;
    registry.register(DebugGoal)?;
    registry.register(workspace::AddProject)?;
    registry.register(workspace::UnlinkProject)?;
    registry.register(workspace::IgnoreProject)?;
    registry.register(workspace::SetView)?;
    registry.register(workspace::CompleteGoal)?;
    registry.register(workspace::OpenSettingsFile)?;
    registry.register(workspace::DownloadSources)?;
    registry.register(workspace::ShowEffectivePom)?;
    registry.register(workspace::CreateRunConfiguration)?;
    settings::register(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests;
