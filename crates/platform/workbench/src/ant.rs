pub mod model;

use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{
    CommandLine, Configuration, FieldKind, FieldSpec, ForeignRunConfiguration, KindSpec, OutputParser, PlainParser,
    RunContext, RunProvider, RunTarget,
};

use crate::build_tools::{BuildEnv, relative_to};
use crate::run::{RunStarted, project_root, wait_for_run};

pub const KIND: &str = "ant";

pub const SETTINGS_FILE: &str = ".zen/ant.json";

pub const DOWNLOAD_VERSION: &str = "1.10.18";
#[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "no download in the browser"))]
const DOWNLOAD_SHA512: &str = "c510d744876d8da48dabc9495b023b6ec5284a0f18b40ee50d98ce099e791ca5c73e3cd4c80d3c90d3efc03f9620be43aad0a373ed47198284f7e3373e0db2c3";

/// The options of an `ant` configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct AntOptions {
    /// The build file, relative to the project; Ant runs in its folder.
    pub build_file: String,
    /// The targets, in order: `clean jar`; the build file's default target when empty.
    pub targets: String,
}

impl Default for AntOptions {
    fn default() -> Self {
        Self { build_file: "build.xml".to_owned(), targets: String::new() }
    }
}

/// A property a build file's runs pass to Ant (`-Dname=value`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Property {
    pub name: String,
    pub value: String,
}

/// What the runs of one build file use, as its Properties set them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildFileSettings {
    /// The build file, relative to the project.
    pub path: String,
    /// The JDK's home folder to run Ant with; the project's JDK when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub jdk: String,
    /// Extra classpath entries for Ant's own class loader (`-lib`): jars or folders, relative to
    /// the project or absolute.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub classpath: Vec<String>,
    /// The most memory Ant's JVM may take, in megabytes (`-Xmx` in `ANT_OPTS`); Java's own
    /// default when 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub max_memory: u32,
    /// Properties passed to every run (`-Dname=value`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<Property>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Settings {
    build_files: Vec<BuildFileSettings>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    filter_targets: bool,
}

fn settings_path(root: &Path) -> PathBuf {
    root.join(SETTINGS_FILE)
}

fn load(root: &Path) -> Settings {
    match std::fs::read(settings_path(root)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) if root.join("build.xml").is_file() => Settings {
            build_files: vec![BuildFileSettings { path: "build.xml".to_owned(), ..Default::default() }],
            ..Default::default()
        },
        Err(_) => Settings::default(),
    }
}

fn save(root: &Path, settings: &Settings) -> Result<(), CommandError> {
    let path = settings_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| CommandError::new(error.to_string()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(settings).map_err(|error| CommandError::new(error.to_string()))?;
    bytes.push(b'\n');
    zen_vfs::write_atomic(&path, &bytes).map_err(|error| CommandError::new(error.to_string()))
}

pub fn has_build_files(root: &Path) -> bool {
    !load(root).build_files.is_empty()
}

fn file_settings(root: &Path, build_file: &str) -> BuildFileSettings {
    load(root).build_files.into_iter().find(|file| file.path == build_file).unwrap_or_default()
}

fn options(configuration: &Configuration) -> AntOptions {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

pub fn executable(env: &BuildEnv, folders: &[PathBuf]) -> Option<PathBuf> {
    let mut all = folders.to_vec();
    if let Some(home) = std::env::var_os("ANT_HOME").filter(|home| !home.is_empty()) {
        all.push(PathBuf::from(home).join("bin"));
    }
    if let Some(data) = env.data_dir() {
        all.push(data.join("tools/ant").join(DOWNLOAD_VERSION).join("bin"));
    }
    zen_lsp::servers::find_executable("ant", &all)
}

pub fn arguments(build_file: &Path, settings: &BuildFileSettings, root: &Path, targets: &str) -> Vec<String> {
    let mut args = vec!["-buildfile".to_owned(), build_file.to_string_lossy().into_owned()];
    for entry in settings.classpath.iter().map(|entry| entry.trim()).filter(|entry| !entry.is_empty()) {
        args.push("-lib".to_owned());
        args.push(root.join(entry).to_string_lossy().into_owned());
    }
    for property in settings.properties.iter().filter(|property| !property.name.trim().is_empty()) {
        args.push(format!("-D{}={}", property.name.trim(), property.value));
    }
    args.extend(zen_run::args::split(targets));
    args
}

#[derive(Clone, Debug, Default)]
pub struct AntRun {
    env: BuildEnv,
}

impl AntRun {
    pub fn new(env: BuildEnv) -> Self {
        Self { env }
    }

    pub fn targets(name: &str, build_file: &str, targets: &str) -> Configuration {
        let options = AntOptions { build_file: build_file.to_owned(), targets: targets.to_owned() };
        let name = if targets.trim().is_empty() { name.to_owned() } else { format!("{name} [{}]", targets.trim()) };
        Configuration {
            name,
            kind: KIND.to_owned(),
            options: serde_json::to_value(options).unwrap_or_default(),
            ..Configuration::default()
        }
    }

    pub fn command_line(&self, configuration: &Configuration, root: &Path) -> Result<CommandLine, String> {
        let options = options(configuration);
        let relative = if options.build_file.trim().is_empty() { "build.xml" } else { options.build_file.trim() };
        let build_file = root.join(relative);
        if !build_file.is_file() {
            return Err(t!("workbench-ant-no-build-file", path = build_file.display().to_string()));
        }
        let folder = build_file.parent().map_or_else(|| root.to_owned(), Path::to_owned);
        let folders = self.env.folders("ant");
        let program = executable(&self.env, &folders).ok_or_else(|| t!("workbench-ant-not-found"))?;
        let settings = file_settings(root, &relative_to(root, &build_file.to_string_lossy()));
        let mut args = arguments(&build_file, &settings, root, &options.targets);
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        let cwd = match &configuration.common.working_directory {
            Some(dir) if !dir.as_os_str().is_empty() => root.join(dir),
            _ => folder,
        };
        let mut env = self.env.environment(root, &folders);
        let jdk = Path::new(settings.jdk.trim());
        if !settings.jdk.trim().is_empty() {
            if !jdk.join("bin").is_dir() {
                return Err(t!("workbench-ant-no-jdk", path = settings.jdk.trim()));
            }
            env.retain(|(name, _)| name != "JAVA_HOME");
            env.push(("JAVA_HOME".to_owned(), jdk.to_string_lossy().into_owned()));
            if let Some((_, path)) = env.iter_mut().find(|(name, _)| name == "PATH") {
                let mut joined = std::env::split_paths(path.as_str()).collect::<Vec<_>>();
                joined.insert(0, jdk.join("bin"));
                if let Ok(value) = std::env::join_paths(joined) {
                    *path = value.to_string_lossy().into_owned();
                }
            }
        }
        if settings.max_memory > 0 {
            let inherited = std::env::var("ANT_OPTS").unwrap_or_default();
            let options = format!("{inherited} -Xmx{}m", settings.max_memory);
            env.push(("ANT_OPTS".to_owned(), options.trim().to_owned()));
        }
        env.extend(configuration.common.env.iter().cloned());
        let shown = std::iter::once("ant".to_owned())
            .chain(["-buildfile".to_owned(), relative.to_owned()])
            .chain(args.iter().skip(2).cloned())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(CommandLine { program, args, cwd, env, display: shown })
    }
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

impl RunProvider for AntRun {
    fn id(&self) -> &str {
        KIND
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: KIND.to_owned(),
            name_message: "workbench-ant-kind".to_owned(),
            fields: vec![
                field("buildFile", "workbench-ant-field-build-file", FieldKind::Path),
                field("targets", "workbench-ant-field-targets", FieldKind::Text),
            ],
            defaults: serde_json::to_value(AntOptions::default()).unwrap_or_default(),
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

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }

    fn adopt(&self, _foreign: &ForeignRunConfiguration, _root: &Path) -> Option<Configuration> {
        None
    }
}

fn env(session: &Session) -> BuildEnv {
    session.state::<BuildEnv>().cloned().unwrap_or_default()
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    let path = if path.is_absolute() { path.to_owned() } else { root.join(path) };
    if path.is_dir() { path.join("build.xml") } else { path }
}

fn listed(root: &Path, settings: &Settings, path: &Path) -> Result<String, CommandError> {
    let relative = relative_to(root, &resolve(root, path).to_string_lossy());
    if settings.build_files.iter().any(|file| file.path == relative) {
        Ok(relative)
    } else {
        Err(CommandError::new(t!("workbench-ant-not-listed", path = relative.as_str())))
    }
}

/// A build file as `ant.project.get` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AntBuildFile {
    /// What the file declares.
    #[serde(flatten)]
    pub file: model::BuildFile,
    /// What its runs use (`ant.buildFile.configure`).
    pub settings: BuildFileSettings,
}

/// What `ant.project.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AntProject {
    /// The build files the Ant tool window lists, in its order.
    pub build_files: Vec<AntBuildFile>,
    /// Whether the tool window shows only the targets with a description (and the default one).
    pub filter_targets: bool,
    /// The `ant` program; absent when none is found (`ant.distribution.download`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    /// The project's JDK Ant runs with unless a build file names its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_home: Option<PathBuf>,
}

fn project(session: &Session) -> Result<AntProject, CommandError> {
    let root = project_root(session)?;
    let settings = load(&root);
    let env = env(session);
    let folders = env.folders("ant");
    Ok(AntProject {
        build_files: settings
            .build_files
            .iter()
            .map(|file| AntBuildFile { file: model::read(&root.join(&file.path)), settings: file.clone() })
            .collect(),
        filter_targets: settings.filter_targets,
        executable: executable(&env, &folders),
        java_home: env.java_home(&root),
    })
}

pub struct GetAntProject;

impl Command for GetAntProject {
    type Params = NoParams;
    type Output = AntProject;
    const ID: &'static str = "ant.project.get";
    const TITLE: &'static str = "Ant Build Files";
    const DESCRIPTION: &'static str = "The open project's Ant build files as the Ant tool window lists them, read without \
         running Ant: each file's project name, default target, description and targets (name, description, \
         depends, the offset and line of its name), and the settings its runs use (JDK, extra classpath, maximum \
         memory, properties); whether Filter Targets is on; the ant program and the project's JDK. The project's \
         build.xml is listed until build files are added or removed (.zen/ant.json).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<AntProject, CommandError> {
        project(session)
    }
}

/// The parameters of `ant.buildFile.add` and `ant.buildFile.remove`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildFileParams {
    /// The build file, absolute or relative to the project; a folder means its `build.xml`.
    pub path: PathBuf,
}

pub struct AddBuildFile;

impl Command for AddBuildFile {
    type Params = BuildFileParams;
    type Output = AntProject;
    const ID: &'static str = "ant.buildFile.add";
    const TITLE: &'static str = "Add Ant Build File";
    const DESCRIPTION: &'static str = "Adds a build file to the Ant tool window, as its + button does: an XML file of the \
         project whose root element is <project>. Kept in the project's .zen/ant.json; answers the build files.";

    fn run(&self, session: &mut Session, params: BuildFileParams) -> Result<AntProject, CommandError> {
        let root = project_root(session)?;
        let path = resolve(&root, &params.path);
        if !path.starts_with(&root) {
            return Err(CommandError::new(t!("workbench-ant-outside", path = path.display().to_string())));
        }
        let text = zen_vfs::read_text(&path).map_err(|error| {
            CommandError::new(t!("workbench-error-open", path = path.display().to_string(), error = error.to_string()))
        })?;
        if !model::is_build_file(&text) {
            return Err(CommandError::new(t!("workbench-ant-not-build-file", path = path.display().to_string())));
        }
        let mut settings = load(&root);
        let relative = relative_to(&root, &path.to_string_lossy());
        if !settings.build_files.iter().any(|file| file.path == relative) {
            settings.build_files.push(BuildFileSettings { path: relative, ..BuildFileSettings::default() });
        }
        save(&root, &settings)?;
        project(session)
    }
}

pub struct RemoveBuildFile;

impl Command for RemoveBuildFile {
    type Params = BuildFileParams;
    type Output = AntProject;
    const ID: &'static str = "ant.buildFile.remove";
    const TITLE: &'static str = "Remove Ant Build File";
    const DESCRIPTION: &'static str = "Removes a build file from the Ant tool window, as its - button does; the file \
         itself stays. Answers the build files left.";

    fn run(&self, session: &mut Session, params: BuildFileParams) -> Result<AntProject, CommandError> {
        let root = project_root(session)?;
        let mut settings = load(&root);
        let relative = listed(&root, &settings, &params.path)?;
        settings.build_files.retain(|file| file.path != relative);
        save(&root, &settings)?;
        project(session)
    }
}

/// The parameters of `ant.buildFile.configure`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureParams {
    /// The build file, absolute or relative to the project; one the tool window lists.
    pub path: PathBuf,
    /// The JDK's home folder its runs use; `""` for the project's JDK.
    #[serde(default)]
    pub jdk: Option<String>,
    /// Extra classpath entries (`-lib`), jars or folders; `[]` for none.
    #[serde(default)]
    pub classpath: Option<Vec<String>>,
    /// The most memory Ant may take, in megabytes; 0 for Java's default.
    #[serde(default)]
    pub max_memory: Option<u32>,
    /// The properties passed to every run (`-Dname=value`); `[]` for none.
    #[serde(default)]
    pub properties: Option<Vec<Property>>,
}

pub struct ConfigureBuildFile;

impl Command for ConfigureBuildFile {
    type Params = ConfigureParams;
    type Output = BuildFileSettings;
    const ID: &'static str = "ant.buildFile.configure";
    const TITLE: &'static str = "Ant Build File Properties";
    const DESCRIPTION: &'static str = "Sets what a build file's runs use, as its Properties do: the JDK (its home folder; \
         empty for the project's), extra classpath entries for Ant (-lib), the maximum memory in megabytes \
         (-Xmx through ANT_OPTS; 0 for Java's default) and properties (-Dname=value). What is not given stays; \
         answers the build file's settings.";

    fn run(&self, session: &mut Session, params: ConfigureParams) -> Result<BuildFileSettings, CommandError> {
        let given = [
            params.jdk.is_some().then_some("jdk"),
            params.classpath.is_some().then_some("classpath"),
            params.properties.is_some().then_some("properties"),
        ];
        crate::trust::guard_agent_build_settings(
            session,
            given.into_iter().flatten(),
            &["jdk", "classpath", "properties"],
        )?;
        let root = project_root(session)?;
        let mut settings = load(&root);
        let relative = listed(&root, &settings, &params.path)?;
        if let Some(jdk) = params.jdk.as_deref().map(str::trim).filter(|jdk| !jdk.is_empty())
            && !Path::new(jdk).join("bin").is_dir()
        {
            return Err(CommandError::new(t!("workbench-ant-no-jdk", path = jdk)));
        }
        let Some(file) = settings.build_files.iter_mut().find(|file| file.path == relative) else {
            return Err(CommandError::new(t!("workbench-ant-not-listed", path = relative.as_str())));
        };
        if let Some(jdk) = params.jdk {
            file.jdk = jdk.trim().to_owned();
        }
        if let Some(classpath) = params.classpath {
            file.classpath =
                classpath.into_iter().map(|entry| entry.trim().to_owned()).filter(|e| !e.is_empty()).collect();
        }
        if let Some(memory) = params.max_memory {
            file.max_memory = memory;
        }
        if let Some(properties) = params.properties {
            file.properties = properties.into_iter().filter(|property| !property.name.trim().is_empty()).collect();
        }
        let answer = file.clone();
        save(&root, &settings)?;
        Ok(answer)
    }
}

/// The parameters of `ant.options.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionsParams {
    /// Filter Targets: list only the targets with a description (and the default one).
    pub filter_targets: bool,
}

pub struct SetOptions;

impl Command for SetOptions {
    type Params = OptionsParams;
    type Output = AntProject;
    const ID: &'static str = "ant.options.set";
    const TITLE: &'static str = "Filter Ant Targets";
    const DESCRIPTION: &'static str = "Turns the Ant tool window's Filter Targets on or off: on, it lists only the \
         targets with a description, and the default one. Answers the build files.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<AntProject, CommandError> {
        let root = project_root(session)?;
        let mut settings = load(&root);
        settings.filter_targets = params.filter_targets;
        save(&root, &settings)?;
        project(session)
    }
}

/// The parameters of `ant.target.run`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunTargetParams {
    /// The targets, in order (`clean jar`); the build file's default target when empty.
    #[serde(default)]
    pub targets: String,
    /// The build file, absolute or relative to the project; the first one listed when absent.
    #[serde(default)]
    pub build_file: Option<PathBuf>,
    /// Waits up to this long for the run to end (at most 10 minutes) and answers its summary.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct RunAntTarget;

impl Command for RunAntTarget {
    type Params = RunTargetParams;
    type Output = RunStarted;
    const ID: &'static str = "ant.target.run";
    const TITLE: &'static str = "Run Ant Target";
    const DESCRIPTION: &'static str = "Runs Ant targets of a build file, as a double-click in the Ant tool window does: a \
         temporary run configuration of kind ant with the build file's JDK, classpath, memory and properties, its \
         output in the Run tool window with links to the files it names; returns the run's id, and with waitMs its \
         summary once it ended.";

    fn run(&self, session: &mut Session, params: RunTargetParams) -> Result<RunStarted, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let root = project_root(session)?;
        let settings = load(&root);
        let relative = match &params.build_file {
            Some(path) => listed(&root, &settings, path)?,
            None => settings
                .build_files
                .first()
                .map(|file| file.path.clone())
                .ok_or_else(|| CommandError::new(t!("workbench-ant-no-build-files")))?,
        };
        let file = model::read(&root.join(&relative));
        if let Some(error) = &file.error {
            return Err(CommandError::new(t!(
                "workbench-ant-unreadable",
                path = relative.as_str(),
                error = error.as_str()
            )));
        }
        let targets: Vec<String> = zen_run::args::split(&params.targets);
        if let Some(unknown) = targets.iter().find(|name| !file.targets.iter().any(|target| target.name == **name)) {
            return Err(CommandError::new(t!(
                "workbench-ant-no-target",
                target = unknown.as_str(),
                path = relative.as_str()
            )));
        }
        let mut configuration = AntRun::targets(&file.name, &relative, params.targets.trim());
        configuration.temporary = true;
        crate::trust::guard_run(session, &project_root(session)?, &configuration)?;
        let started = crate::run::start(session, configuration)?;
        match params.wait_ms.map(Duration::from_millis) {
            Some(wait) => Ok(RunStarted { summary: Some(wait_for_run(session, started.run_id, wait)?), ..started }),
            None => Ok(started),
        }
    }
}

pub(crate) fn go_to(session: &mut Session, path: &Path, offset: usize) -> Option<Result<(), String>> {
    if path.extension().is_none_or(|extension| extension != "xml") {
        return None;
    }
    let (text, _) = crate::run::file_text(session, path).ok()?;
    if !model::is_build_file(&text) {
        return None;
    }
    let byte = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
    let name = model::target_at(&text, byte)?;
    let file = model::parse(path, &text);
    let Some(target) = file.targets.iter().find(|target| target.name == name) else {
        return Some(Err(t!("workbench-ant-no-target", target = name.as_str(), path = file.name.as_str())));
    };
    let params = crate::history::NavigateToParams {
        path: path.to_owned(),
        offset: target.offset,
        end: Some(target.offset + target.name.chars().count()),
    };
    Some(crate::history::NavigateTo.run(session, params).map(|_| ()).map_err(|error| error.to_string()))
}

/// What `ant.distribution.download` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub version: String,
    /// Where it is unpacked; its `bin/ant` is used where no other Ant is found.
    pub folder: PathBuf,
    /// The background task that downloads it (`tasks.log.read`), when it is not there already.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

pub struct DownloadAnt;

impl Command for DownloadAnt {
    type Params = NoParams;
    type Output = Download;
    const ID: &'static str = "ant.distribution.download";
    const TITLE: &'static str = "Download Ant";
    const DESCRIPTION: &'static str = "Downloads Apache Ant 1.10.18 from the Apache archive (checked against its \
         SHA-512) into the IDE's data folder, in a background task, for a computer without Ant: the network is \
         used only when this command runs.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Download, CommandError> {
        let env = env(session);
        let data = env.data_dir().ok_or_else(|| CommandError::new(t!("workbench-ant-download-no-folder")))?;
        let folder = data.join("tools/ant").join(DOWNLOAD_VERSION);
        if folder.join("bin/ant").is_file() {
            return Ok(Download { version: DOWNLOAD_VERSION.to_owned(), folder, task: None });
        }
        let title = t!("workbench-ant-download-task", version = DOWNLOAD_VERSION);
        let task = session.state_mut::<crate::tasks::Tasks>().start(&title, "ant", true);
        let id = task.id();
        let target = folder.clone();
        std::thread::spawn(move || task.finish(download(&target, &task)));
        Ok(Download { version: DOWNLOAD_VERSION.to_owned(), folder, task: Some(id) })
    }
}

#[cfg(target_arch = "wasm32")]
fn download(_folder: &Path, _task: &crate::tasks::TaskHandle) -> Result<String, String> {
    Err(t!("workbench-ant-download-unavailable"))
}

#[cfg(not(target_arch = "wasm32"))]
fn download(folder: &Path, task: &crate::tasks::TaskHandle) -> Result<String, String> {
    let url = format!("https://archive.apache.org/dist/ant/binaries/apache-ant-{v}-bin.tar.gz", v = DOWNLOAD_VERSION);
    task.log(&url);
    let bytes = zen_update::fetch::document(&url, 64 * 1024 * 1024, Duration::from_secs(600))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{url}: not found"))?;
    install(&bytes, folder, &url)
}

#[cfg(not(target_arch = "wasm32"))]
fn install(bytes: &[u8], folder: &Path, url: &str) -> Result<String, String> {
    use sha2::Digest;
    if hex::encode(sha2::Sha512::digest(bytes)) != DOWNLOAD_SHA512 {
        return Err(t!("workbench-ant-download-checksum", url = url));
    }
    let unpacked = folder.with_extension("partial");
    let _ = std::fs::remove_dir_all(&unpacked);
    zen_update::install::unpack_tar_gz(bytes, &unpacked).map_err(|error| error.to_string())?;
    let inner = unpacked.join(format!("apache-ant-{DOWNLOAD_VERSION}"));
    let _ = std::fs::remove_dir_all(folder);
    if let Some(parent) = folder.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&inner, folder).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_dir_all(&unpacked);
    Ok(t!("workbench-ant-download-done", folder = folder.display().to_string()))
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetAntProject)?;
    registry.register(AddBuildFile)?;
    registry.register(RemoveBuildFile)?;
    registry.register(ConfigureBuildFile)?;
    registry.register(SetOptions)?;
    registry.register(RunAntTarget)?;
    registry.register(DownloadAnt)?;
    Ok(())
}

#[cfg(test)]
mod tests;
