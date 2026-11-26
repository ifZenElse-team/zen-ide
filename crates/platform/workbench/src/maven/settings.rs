use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_modules::form::{Action, Browse, Choice, Form, Item, Target, Tone};

use crate::build_tools::found::{Distribution, MAVEN};
use crate::build_tools::{BuildEnv, Executable};

pub const TOOL: &str = "maven";

pub const BUILD_TOOLS_PAGE: &str = "buildTools";
pub const PAGE: &str = "maven";
pub const IMPORTING_PAGE: &str = "mavenImporting";
pub const RUNNER_PAGE: &str = "mavenRunner";
pub const TESTS_PAGE: &str = "mavenRunningTests";

pub const JAVA_HOME: &str = "JAVA_HOME";

pub const USER_HOME: &str = "MAVEN_USER_HOME";

/// The Maven settings of a project: Settings → Build, Execution, Deployment → Build Tools → Maven.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MavenSettings {
    /// Which Maven runs.
    pub home: MavenHome,
    /// The Maven home folder (with `bin/mvn`) when `home` is `custom`.
    pub home_path: String,
    /// A `settings.xml` used instead of `~/.m2/settings.xml` (`-s`); empty for the default.
    pub user_settings: String,
    /// A local repository used instead of the one `settings.xml` names (`-Dmaven.repo.local`);
    /// empty for the default.
    pub local_repository: String,
    /// Work offline (`-o`), as Toggle Offline Mode of the Maven tool window.
    pub offline: bool,
    /// Thread count (`-T`): `4`, or `1C` for one per CPU core; empty for one thread.
    pub threads: String,
    /// Always update snapshots (`-U`).
    pub update_snapshots: bool,
    pub output_level: OutputLevel,
    pub checksum_policy: ChecksumPolicy,
    /// Multiproject build fail policy.
    pub fail_policy: FailPolicy,
    pub importing: Importing,
    pub runner: Runner,
    /// Running Tests.
    pub tests: RunningTests,
}

/// Which Maven runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MavenHome {
    /// The project's Maven Wrapper (`mvnw`), else Maven found on this computer.
    #[default]
    Wrapper,
    /// Maven found on this computer (PATH, SDKMAN!, downloaded by the IDE, `~/.m2/wrapper/dists`), never
    /// the project's wrapper.
    Installed,
    /// The Maven in `homePath`.
    Custom,
}

/// How much Maven prints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OutputLevel {
    /// Errors only (`-q`).
    Quiet,
    #[default]
    Info,
    /// Debug output (`-X`).
    Debug,
}

/// What Maven does when a checksum of a download does not match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChecksumPolicy {
    /// What the repositories say.
    #[default]
    Default,
    /// Warn (`-c`).
    Warn,
    /// Fail (`-C`).
    Fail,
}

/// When a build of several projects stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FailPolicy {
    #[default]
    Default,
    /// At the first failure (`-ff`).
    FailFast,
    /// Only at the end, after building what does not depend on a failed project (`-fae`).
    FailAtEnd,
    /// Never (`-fn`).
    Never,
}

/// Where generated sources are found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GeneratedSources {
    #[default]
    Detect,
    /// `target/generated-sources` itself.
    GeneratedSources,
    /// Each folder in `target/generated-sources`.
    Subdirectories,
    /// None.
    Ignore,
}

/// Importing: how Reload All Maven Projects runs Maven.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Importing {
    /// The JDK the importer runs with: empty for the project's JDK, `JAVA_HOME`, or a JDK's home.
    pub jdk: String,
    /// VM options of the importer's Maven (`MAVEN_OPTS`), e.g. `-Xmx2g`.
    pub vm_options: String,
    /// Generated sources folders.
    pub generated_sources: GeneratedSources,
    /// Download the dependencies' sources when the project reloads (`dependency:sources`).
    pub download_sources: bool,
    /// Download the dependencies' documentation when the project reloads (`javadoc` jars).
    pub download_documentation: bool,
}

/// Runner: how goals, tests and the build of Java applications run Maven.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Runner {
    /// Delegate IDE build/run actions to Maven: Java applications of a Maven module are built by
    /// `mvn compile`; off, the IDE compiles them with `javac` on the classpath Maven resolves.
    pub delegate: bool,
    /// VM options of the Maven that runs (`MAVEN_OPTS`).
    pub vm_options: String,
    /// The JDK Maven runs with: empty for the project's JDK, `JAVA_HOME`, or a JDK's home.
    pub jre: String,
    /// Environment variables Maven gets.
    pub environment: BTreeMap<String, String>,
    /// Properties passed as `-Dname=value`.
    pub properties: BTreeMap<String, String>,
    /// Skip tests (`-DskipTests`), as Toggle 'Skip Tests' Mode of the Maven tool window.
    pub skip_tests: bool,
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            delegate: true,
            vm_options: String::new(),
            jre: String::new(),
            environment: BTreeMap::new(),
            properties: BTreeMap::new(),
            skip_tests: false,
        }
    }
}

/// Running Tests: which arguments of Surefire's and Failsafe's configuration in `pom.xml` are passed
/// to tests that run inside Maven (a debugged test).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunningTests {
    /// `argLine`, as VM options.
    pub arg_line: bool,
    /// `systemPropertyVariables`, as `-Dname=value`.
    pub system_properties: bool,
    /// `environmentVariables`.
    pub environment: bool,
}

impl Default for RunningTests {
    fn default() -> Self {
        Self { arg_line: true, system_properties: true, environment: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Goal,
    Import,
    Test,
    Build,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Invocation {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub java_home: Option<PathBuf>,
}

pub fn jdk_home(choice: &str) -> Option<PathBuf> {
    match choice.trim() {
        "" => None,
        JAVA_HOME => std::env::var_os(JAVA_HOME).filter(|home| !home.is_empty()).map(PathBuf::from),
        path => {
            let home = PathBuf::from(path);
            Some(if home.join("Contents/Home/bin").is_dir() { home.join("Contents/Home") } else { home })
        }
    }
}

impl MavenSettings {
    pub fn read(env: &BuildEnv, root: &Path) -> Self {
        env.project_settings(TOOL, root)
    }

    pub fn invocation(&self, purpose: Purpose) -> Invocation {
        let mut args = Vec::new();
        if !self.user_settings.trim().is_empty() {
            args.extend(["-s".to_owned(), self.user_settings.trim().to_owned()]);
        }
        if !self.local_repository.trim().is_empty() {
            args.push(format!("-Dmaven.repo.local={}", self.local_repository.trim()));
        }
        if self.offline && matches!(purpose, Purpose::Test | Purpose::Build) {
            args.push("-o".to_owned());
        }
        if !self.threads.trim().is_empty() {
            args.extend(["-T".to_owned(), self.threads.trim().to_owned()]);
        }
        if self.update_snapshots {
            args.push("-U".to_owned());
        }
        if purpose != Purpose::Build {
            match self.output_level {
                OutputLevel::Quiet => args.push("-q".to_owned()),
                OutputLevel::Info => {}
                OutputLevel::Debug => args.push("-X".to_owned()),
            }
        }
        match self.checksum_policy {
            ChecksumPolicy::Default => {}
            ChecksumPolicy::Warn => args.push("-c".to_owned()),
            ChecksumPolicy::Fail => args.push("-C".to_owned()),
        }
        match self.fail_policy {
            FailPolicy::Default => {}
            FailPolicy::FailFast => args.push("-ff".to_owned()),
            FailPolicy::FailAtEnd => args.push("-fae".to_owned()),
            FailPolicy::Never => args.push("-fn".to_owned()),
        }
        let (vm_options, jdk) = match purpose {
            Purpose::Import => (&self.importing.vm_options, &self.importing.jdk),
            _ => (&self.runner.vm_options, &self.runner.jre),
        };
        let mut env = Vec::new();
        if purpose != Purpose::Import {
            args.extend(self.runner.properties.iter().map(|(name, value)| format!("-D{name}={value}")));
            env.extend(self.runner.environment.iter().map(|(name, value)| (name.clone(), value.clone())));
        }
        if !vm_options.trim().is_empty() {
            env.push(("MAVEN_OPTS".to_owned(), vm_options.trim().to_owned()));
        }
        Invocation { args, env, java_home: jdk_home(jdk) }
    }

    pub fn local_repository(&self, home: &Path) -> PathBuf {
        if !self.local_repository.trim().is_empty() {
            return PathBuf::from(self.local_repository.trim());
        }
        let settings = self.user_settings_file(home);
        std::fs::read_to_string(settings)
            .ok()
            .and_then(|text| {
                let document = roxmltree::Document::parse(&text).ok()?;
                let local = document
                    .root_element()
                    .children()
                    .find(|node| node.has_tag_name("localRepository"))
                    .and_then(|node| node.text())
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_owned();
                (!local.is_empty()).then(|| PathBuf::from(local.replace("${user.home}", &home.to_string_lossy())))
            })
            .unwrap_or_else(|| home.join(".m2/repository"))
    }

    pub fn user_settings_file(&self, home: &Path) -> PathBuf {
        match self.user_settings.trim() {
            "" => home.join(".m2/settings.xml"),
            path => PathBuf::from(path),
        }
    }
}

pub fn with_java(env: &mut Vec<(String, String)>, home: &Path) {
    let bin = home.join("bin");
    for (name, value) in env.iter_mut() {
        if name == "PATH" {
            let rest = std::env::split_paths(value.as_str()).filter(|folder| *folder != bin);
            if let Ok(joined) = std::env::join_paths(std::iter::once(bin.clone()).chain(rest)) {
                *value = joined.to_string_lossy().into_owned();
            }
        }
    }
    env.retain(|(name, _)| name != JAVA_HOME);
    env.push((JAVA_HOME.to_owned(), home.to_string_lossy().into_owned()));
}

fn distributions(env: &BuildEnv) -> Vec<Distribution> {
    let Some(home) = env.user_home() else { return Vec::new() };
    let user = std::env::var_os(USER_HOME).filter(|folder| !folder.is_empty());
    MAVEN.distributions(&user.map_or_else(|| home.join(".m2"), PathBuf::from).join("wrapper/dists"))
}

pub fn executable(
    settings: &MavenSettings,
    folder: &Path,
    root: &Path,
    env: &BuildEnv,
    folders: &[PathBuf],
) -> Option<Executable> {
    match settings.home {
        MavenHome::Wrapper => MAVEN.found(env, folder, root, folders, distributions(env)),
        MavenHome::Installed => MAVEN.installed(env, folders, distributions(env)),
        MavenHome::Custom => MAVEN.custom(&settings.home_path),
    }
}

fn parse_pairs(text: &str) -> BTreeMap<String, String> {
    text.split([';', '\n'])
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return None;
            }
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = name.trim().trim_start_matches("-D");
            (!name.is_empty()).then(|| (name.to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn show_pairs(pairs: &BTreeMap<String, String>) -> String {
    pairs.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; ")
}

const CHOOSE_PROGRAM: &[&str] = &[
    "home",
    "homePath",
    "userSettings",
    "localRepository",
    "importing.jdk",
    "importing.vmOptions",
    "runner.vmOptions",
    "runner.jre",
    "runner.environment",
    "runner.properties",
];

fn change(settings: &MavenSettings, key: &str, value: serde_json::Value) -> Result<MavenSettings, CommandError> {
    let unknown = || CommandError::new(t!("maven-settings-unknown", key = key));
    let mut whole = serde_json::to_value(settings).map_err(|error| CommandError::new(error.to_string()))?;
    let defaults = serde_json::to_value(MavenSettings::default()).unwrap_or_default();
    let pointer = format!("/{}", key.replace('.', "/"));
    let default = defaults.pointer(&pointer).cloned().ok_or_else(unknown)?;
    let value = match (&default, value) {
        (_, serde_json::Value::Null) => default.clone(),
        (serde_json::Value::Object(_), serde_json::Value::String(text)) => {
            serde_json::to_value(parse_pairs(&text)).unwrap_or_default()
        }
        (_, value) => value,
    };
    let shown = value.to_string();
    *whole.pointer_mut(&pointer).ok_or_else(unknown)? = value;
    serde_json::from_value(whole)
        .map_err(|_| CommandError::new(t!("maven-settings-bad-value", key = key, value = shown.as_str())))
}

/// Which project's settings a command is about.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectParams {
    /// The project's root folder; the window's project when absent.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

fn root(session: &Session, given: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    match given {
        Some(root) => Ok(root),
        None => crate::run::project_root(session),
    }
}

fn env(session: &Session) -> BuildEnv {
    session.state::<BuildEnv>().cloned().unwrap_or_default()
}

pub fn of_session(session: &Session) -> Option<(PathBuf, MavenSettings)> {
    let root = crate::run::project_root(session).ok()?;
    let settings = MavenSettings::read(&env(session), &root);
    Some((root, settings))
}

pub fn save(session: &Session, root: &Path, settings: &MavenSettings) -> Result<(), CommandError> {
    env(session).set_project_settings(TOOL, root, settings).map_err(CommandError::new)
}

pub struct GetSettings;

impl Command for GetSettings {
    type Params = ProjectParams;
    type Output = MavenSettings;
    const ID: &'static str = "maven.settings.get";
    const TITLE: &'static str = "Maven Settings";
    const DESCRIPTION: &'static str = "The project's Maven settings (Settings → Build, Execution, Deployment → Build \
         Tools → Maven, with Importing, Runner and Running Tests), kept per project. Every Maven run uses them: goals, \
         Reload All Maven Projects and the dependency tree, Maven runs of tests and the build of Java applications. \
         Change them with maven.settings.set.";

    fn run(&self, session: &mut Session, params: ProjectParams) -> Result<MavenSettings, CommandError> {
        let root = root(session, params.project)?;
        Ok(MavenSettings::read(&env(session), &root))
    }
}

/// The parameters of `maven.settings.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetParams {
    /// The setting, as maven.settings.get names it, nested with dots: `offline`, `runner.vmOptions`,
    /// `importing.downloadSources`, `tests.argLine`.
    #[serde(default)]
    pub key: Option<String>,
    /// Its new value; `null` puts the default back. A map (`runner.environment`,
    /// `runner.properties`) also takes `NAME=value; OTHER=value`.
    #[serde(default)]
    pub value: serde_json::Value,
    /// Or several settings at once, by key: `{ "threads": "4", "runner.skipTests": true }`.
    #[serde(default)]
    pub values: BTreeMap<String, serde_json::Value>,
    /// The project's root folder; the window's project when absent.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

pub struct SetSettings;

impl Command for SetSettings {
    type Params = SetParams;
    type Output = MavenSettings;
    const ID: &'static str = "maven.settings.set";
    const TITLE: &'static str = "Change Maven Settings";
    const DESCRIPTION: &'static str = "Changes the project's Maven settings, one by key and value or several with \
         values, and answers all of them (maven.settings.get). `offline` and `runner.skipTests` are the Maven tool \
         window's Toggle Offline Mode and Toggle 'Skip Tests' Mode.";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<MavenSettings, CommandError> {
        let root = root(session, params.project)?;
        let mut settings = MavenSettings::read(&env(session), &root);
        let mut changes: Vec<(String, serde_json::Value)> = params.values.into_iter().collect();
        if let Some(key) = params.key {
            changes.push((key, params.value));
        }
        crate::trust::guard_agent_build_settings(session, changes.iter().map(|(key, _)| key.as_str()), CHOOSE_PROGRAM)?;
        for (key, value) in changes {
            settings = change(&settings, &key, value)?;
        }
        save(session, &root, &settings)?;
        Ok(settings)
    }
}

fn set(key: &str) -> Action {
    Action::with_value(SetSettings::ID, "value").param("key", key)
}

fn check(label: String, value: bool, key: &str) -> Item {
    Item::Check { label, value, action: set(key) }
}

fn field(label: String, value: &str, key: &str, browse: Browse) -> Item {
    Item::Field { label, value: value.to_owned(), action: set(key), browse }
}

fn note(text: String) -> Item {
    Item::Text { text, tone: Tone::Secondary, spinner: false }
}

fn choice<T: Serialize + PartialEq + Copy>(label: String, key: &str, selected: T, options: &[(T, String)]) -> Item {
    let value = |option: T| serde_json::to_value(option).ok().and_then(|value| value.as_str().map(str::to_owned));
    Item::Choice {
        label,
        options: options
            .iter()
            .filter_map(|(option, label)| Some(Choice { value: value(*option)?, label: label.clone() }))
            .collect(),
        selected: value(selected),
        action: set(key),
    }
}

fn jdk_choice(session: &mut Session, label: String, key: &str, selected: &str) -> Item {
    let mut options = vec![
        Choice { value: String::new(), label: t!("maven-settings-jdk-project") },
        Choice { value: JAVA_HOME.to_owned(), label: t!("maven-settings-jdk-java-home") },
    ];
    let jdks = session.state_mut::<crate::jdk::Jdks>().clone();
    options.extend(jdks.store().jdks().iter().map(|jdk| Choice {
        value: jdk.home.display().to_string(),
        label: format!("{} — {}", jdk.label(), jdk.home.display()),
    }));
    if !options.iter().any(|option| option.value == selected) {
        options.push(Choice { value: selected.to_owned(), label: selected.to_owned() });
    }
    Item::Choice { label, options, selected: Some(selected.to_owned()), action: set(key) }
}

fn no_project() -> Form {
    Form { items: vec![note(t!("maven-settings-no-project"))], busy: false }
}

pub struct BuildToolsForm;

impl Command for BuildToolsForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "buildTools.settings.form";
    const TITLE: &'static str = "Build Tools Settings";
    const DESCRIPTION: &'static str = "Settings → Build, Execution, Deployment → Build Tools as a form: links to the \
         settings of each build tool.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        Ok(Form {
            items: vec![
                note(t!("build-tools-page-note")),
                Item::Link { label: t!("maven-settings-page"), target: Target::Page { page: PAGE.to_owned() } },
                Item::Link {
                    label: t!("gradle-settings-page"),
                    target: Target::Page { page: crate::gradle::settings::PAGE.to_owned() },
                },
            ],
            busy: false,
        })
    }
}

pub struct MavenForm;

impl Command for MavenForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "maven.settings.form";
    const TITLE: &'static str = "Maven Settings Page";
    const DESCRIPTION: &'static str = "Settings → Build, Execution, Deployment → Build Tools → Maven as a form: work \
         offline, always update snapshots, output level, checksum and fail policies, thread count, the Maven home, \
         the user settings file and the local repository. Its changes run maven.settings.set.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let Some((root, settings)) = of_session(session) else { return Ok(no_project()) };
        let env = env(session);
        let mut items = vec![
            check(t!("maven-settings-offline"), settings.offline, "offline"),
            check(t!("maven-settings-update-snapshots"), settings.update_snapshots, "updateSnapshots"),
            choice(
                t!("maven-settings-output-level"),
                "outputLevel",
                settings.output_level,
                &[
                    (OutputLevel::Quiet, t!("maven-settings-output-quiet")),
                    (OutputLevel::Info, t!("maven-settings-output-info")),
                    (OutputLevel::Debug, t!("maven-settings-output-debug")),
                ],
            ),
            choice(
                t!("maven-settings-checksum"),
                "checksumPolicy",
                settings.checksum_policy,
                &[
                    (ChecksumPolicy::Default, t!("maven-settings-checksum-default")),
                    (ChecksumPolicy::Warn, t!("maven-settings-checksum-warn")),
                    (ChecksumPolicy::Fail, t!("maven-settings-checksum-fail")),
                ],
            ),
            choice(
                t!("maven-settings-fail-policy"),
                "failPolicy",
                settings.fail_policy,
                &[
                    (FailPolicy::Default, t!("maven-settings-fail-default")),
                    (FailPolicy::FailFast, t!("maven-settings-fail-fast")),
                    (FailPolicy::FailAtEnd, t!("maven-settings-fail-at-end")),
                    (FailPolicy::Never, t!("maven-settings-fail-never")),
                ],
            ),
            field(t!("maven-settings-threads"), &settings.threads, "threads", Browse::None),
            note(t!("maven-settings-threads-note")),
            choice(
                t!("maven-settings-home"),
                "home",
                settings.home,
                &[
                    (MavenHome::Wrapper, t!("maven-settings-home-wrapper")),
                    (MavenHome::Installed, t!("maven-settings-home-installed")),
                    (MavenHome::Custom, t!("maven-settings-home-custom")),
                ],
            ),
        ];
        if settings.home == MavenHome::Custom {
            items.push(field(t!("maven-settings-home-path"), &settings.home_path, "homePath", Browse::Folder));
        }
        items.push(match executable(&settings, &root, &root, &env, &env.folders("maven")) {
            Some(found) => note(t!("maven-settings-home-found", program = found.path.display().to_string())),
            None => Item::Text { text: t!("maven-settings-home-none"), tone: Tone::Error, spinner: false },
        });
        let home = env.home();
        let defaults = MavenSettings::default();
        items.extend([
            field(t!("maven-settings-user-settings"), &settings.user_settings, "userSettings", Browse::File),
            note(t!("maven-settings-default-path", path = defaults.user_settings_file(&home).display().to_string())),
            field(t!("maven-settings-local-repository"), &settings.local_repository, "localRepository", Browse::Folder),
            note(t!(
                "maven-settings-default-path",
                path = MavenSettings { local_repository: String::new(), ..settings.clone() }
                    .local_repository(&home)
                    .display()
                    .to_string()
            )),
        ]);
        Ok(Form { items, busy: false })
    }
}

pub struct ImportingForm;

impl Command for ImportingForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "maven.importing.form";
    const TITLE: &'static str = "Maven Importing Page";
    const DESCRIPTION: &'static str = "Settings → … → Maven → Importing as a form: the importer's JDK and VM options, \
         the generated sources folders, and downloading sources and documentation when the project reloads.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let Some((_, settings)) = of_session(session) else { return Ok(no_project()) };
        let importing = &settings.importing;
        let items = vec![
            choice(
                t!("maven-settings-generated"),
                "importing.generatedSources",
                importing.generated_sources,
                &[
                    (GeneratedSources::Detect, t!("maven-settings-generated-detect")),
                    (GeneratedSources::GeneratedSources, t!("maven-settings-generated-root")),
                    (GeneratedSources::Subdirectories, t!("maven-settings-generated-subfolders")),
                    (GeneratedSources::Ignore, t!("maven-settings-generated-ignore")),
                ],
            ),
            Item::Group { title: t!("maven-settings-download") },
            Item::Row {
                items: vec![
                    check(
                        t!("maven-settings-download-sources"),
                        importing.download_sources,
                        "importing.downloadSources",
                    ),
                    check(
                        t!("maven-settings-download-docs"),
                        importing.download_documentation,
                        "importing.downloadDocumentation",
                    ),
                ],
            },
            Item::Group { title: t!("maven-settings-importer") },
            field(t!("maven-settings-importer-vm"), &importing.vm_options, "importing.vmOptions", Browse::None),
            jdk_choice(session, t!("maven-settings-importer-jdk"), "importing.jdk", &importing.jdk),
            note(t!("maven-settings-importing-note")),
        ];
        Ok(Form { items, busy: false })
    }
}

pub struct RunnerForm;

impl Command for RunnerForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "maven.runner.form";
    const TITLE: &'static str = "Maven Runner Page";
    const DESCRIPTION: &'static str = "Settings → … → Maven → Runner as a form: delegating the build and run of Java \
         applications to Maven, VM options, JRE, environment variables, properties and skip tests.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let Some((_, settings)) = of_session(session) else { return Ok(no_project()) };
        let runner = &settings.runner;
        let items = vec![
            check(t!("maven-settings-delegate"), runner.delegate, "runner.delegate"),
            field(t!("maven-settings-runner-vm"), &runner.vm_options, "runner.vmOptions", Browse::None),
            jdk_choice(session, t!("maven-settings-runner-jre"), "runner.jre", &runner.jre),
            field(
                t!("maven-settings-runner-env"),
                &show_pairs(&runner.environment),
                "runner.environment",
                Browse::None,
            ),
            Item::Group { title: t!("maven-settings-runner-properties") },
            check(t!("maven-settings-skip-tests"), runner.skip_tests, "runner.skipTests"),
            field(
                t!("maven-settings-runner-properties-field"),
                &show_pairs(&runner.properties),
                "runner.properties",
                Browse::None,
            ),
            note(t!("maven-settings-pairs-note")),
            note(t!("maven-settings-runner-note")),
        ];
        Ok(Form { items, busy: false })
    }
}

pub struct TestsForm;

impl Command for TestsForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "maven.runningTests.form";
    const TITLE: &'static str = "Maven Running Tests Page";
    const DESCRIPTION: &'static str = "Settings → … → Maven → Running Tests as a form: which of Surefire's and \
         Failsafe's argLine, systemPropertyVariables and environmentVariables are passed to tests run inside Maven.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let Some((_, settings)) = of_session(session) else { return Ok(no_project()) };
        let tests = &settings.tests;
        let items = vec![
            Item::Group { title: t!("maven-settings-tests-pass") },
            check("argLine".to_owned(), tests.arg_line, "tests.argLine"),
            check("systemPropertyVariables".to_owned(), tests.system_properties, "tests.systemProperties"),
            check("environmentVariables".to_owned(), tests.environment, "tests.environment"),
            note(t!("maven-settings-tests-note")),
        ];
        Ok(Form { items, busy: false })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetSettings)?;
    registry.register(SetSettings)?;
    registry.register(BuildToolsForm)?;
    registry.register(MavenForm)?;
    registry.register(ImportingForm)?;
    registry.register(RunnerForm)?;
    registry.register(TestsForm)?;
    Ok(())
}
