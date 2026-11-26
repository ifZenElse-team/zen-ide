use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_modules::form::{Action, Browse, Choice, Form, Item, Tone};

use crate::build_tools::found::GRADLE;
use crate::build_tools::{BuildEnv, Executable, LoadState};
use crate::maven::settings::{JAVA_HOME, jdk_home};

pub const TOOL: &str = "gradle";

pub const PAGE: &str = "gradle";

pub const USER_HOME: &str = "GRADLE_USER_HOME";

/// The Gradle settings of a project: Settings → Build, Execution, Deployment → Build Tools → Gradle.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GradleSettings {
    /// Gradle user home (`GRADLE_USER_HOME`), where Gradle keeps its caches and the distributions the
    /// wrapper downloads; empty for the default (`~/.gradle`).
    pub user_home: String,
    /// Work offline (`--offline`): Gradle uses its caches only.
    pub offline: bool,
    /// Build and run using: who builds Java applications of the build before they run.
    pub build_and_run: Runner,
    /// Run tests using: who runs the tests of the build.
    pub run_tests: Runner,
    /// Which Gradle runs.
    pub distribution: Distribution,
    /// The Gradle home folder (with `bin/gradle`) when `distribution` is `custom`.
    pub home_path: String,
    /// The JDK Gradle runs with (`JAVA_HOME` of Gradle): empty for the project's JDK, `JAVA_HOME`, or
    /// a JDK's home.
    pub jvm: String,
    /// VM options of the Gradle daemon (`-Dorg.gradle.jvmargs`, which wins over `gradle.properties`),
    /// e.g. `-Xmx2g`; empty for the build's own.
    pub vm_options: String,
    /// Download the dependencies' sources when the build reloads.
    pub download_sources: bool,
}

/// Who builds or runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Runner {
    /// Gradle: `classes` before an application runs, the `test` task for tests.
    #[default]
    Gradle,
    /// The IDE: Gradle only resolves the classpath; the IDE compiles with `javac`, and runs tests on
    /// the JUnit Platform itself.
    Ide,
}

/// Which Gradle runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Distribution {
    /// The build's Gradle Wrapper (`gradlew`), else Gradle found on this computer.
    #[default]
    Wrapper,
    /// Gradle found on this computer (PATH, SDKMAN!, downloaded by the IDE, `~/.gradle/wrapper/dists`),
    /// never the build's wrapper.
    Installed,
    /// The Gradle in `homePath`.
    Custom,
}

impl GradleSettings {
    pub fn read(env: &BuildEnv, root: &Path) -> Self {
        env.project_settings(TOOL, root)
    }

    pub fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.offline {
            args.push("--offline".to_owned());
        }
        if !self.vm_options.trim().is_empty() {
            args.push(format!("-Dorg.gradle.jvmargs={}", self.vm_options.trim()));
        }
        args
    }

    pub fn java_home(&self, env: &BuildEnv, root: &Path) -> Option<PathBuf> {
        jdk_home(&self.jvm).or_else(|| env.java_home(root))
    }

    pub fn environment(&self, env: &BuildEnv, root: &Path, folders: &[PathBuf]) -> Vec<(String, String)> {
        let mut vars = env.environment_with(self.java_home(env, root), folders);
        if !self.user_home.trim().is_empty() {
            vars.push((USER_HOME.to_owned(), self.user_home.trim().to_owned()));
        }
        vars
    }

    pub fn user_home_folder(&self, home: &Path) -> PathBuf {
        match self.user_home.trim() {
            "" => std::env::var_os(USER_HOME)
                .filter(|folder| !folder.is_empty())
                .map_or_else(|| home.join(".gradle"), PathBuf::from),
            folder => PathBuf::from(folder),
        }
    }
}

fn distributions(settings: &GradleSettings, env: &BuildEnv) -> Vec<crate::build_tools::found::Distribution> {
    let Some(home) = env.user_home() else { return Vec::new() };
    GRADLE.distributions(&settings.user_home_folder(&home).join("wrapper/dists"))
}

pub fn executable(
    settings: &GradleSettings,
    folder: &Path,
    root: &Path,
    env: &BuildEnv,
    folders: &[PathBuf],
) -> Option<Executable> {
    match settings.distribution {
        Distribution::Wrapper => GRADLE.found(env, folder, root, folders, distributions(settings, env)),
        Distribution::Installed => GRADLE.installed(env, folders, distributions(settings, env)),
        Distribution::Custom => GRADLE.custom(&settings.home_path),
    }
}

const CHOOSE_PROGRAM: &[&str] = &["distribution", "homePath", "jvm", "userHome", "vmOptions"];

fn change(settings: &GradleSettings, key: &str, value: serde_json::Value) -> Result<GradleSettings, CommandError> {
    let unknown = || CommandError::new(t!("gradle-settings-unknown", key = key));
    let mut whole = serde_json::to_value(settings).map_err(|error| CommandError::new(error.to_string()))?;
    let defaults = serde_json::to_value(GradleSettings::default()).unwrap_or_default();
    let field = whole.as_object_mut().and_then(|fields| fields.get_mut(key)).ok_or_else(unknown)?;
    let value = if value.is_null() { defaults.get(key).cloned().ok_or_else(unknown)? } else { value };
    let shown = value.to_string();
    *field = value;
    serde_json::from_value(whole)
        .map_err(|_| CommandError::new(t!("gradle-settings-bad-value", key = key, value = shown.as_str())))
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

pub struct GetSettings;

impl Command for GetSettings {
    type Params = ProjectParams;
    type Output = GradleSettings;
    const ID: &'static str = "gradle.settings.get";
    const TITLE: &'static str = "Gradle Settings";
    const DESCRIPTION: &'static str = "The project's Gradle settings (Settings → Build, Execution, Deployment → Build \
         Tools → Gradle), kept per project: Gradle user home, work offline, build and run using and run tests using \
         (gradle or ide), the distribution (wrapper, installed or custom with homePath), the Gradle JVM, VM options \
         and downloading sources. Every Gradle run uses them: tasks, Reload All Gradle Projects and the task list, \
         the classpath of Java applications and tests. Change them with gradle.settings.set.";

    fn run(&self, session: &mut Session, params: ProjectParams) -> Result<GradleSettings, CommandError> {
        let root = root(session, params.project)?;
        Ok(GradleSettings::read(&env(session), &root))
    }
}

/// The parameters of `gradle.settings.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetParams {
    /// The setting, as gradle.settings.get names it: `offline`, `vmOptions`, `distribution`.
    #[serde(default)]
    pub key: Option<String>,
    /// Its new value; `null` puts the default back.
    #[serde(default)]
    pub value: serde_json::Value,
    /// Or several settings at once, by key: `{ "jvm": "JAVA_HOME", "runTests": "ide" }`.
    #[serde(default)]
    pub values: std::collections::BTreeMap<String, serde_json::Value>,
    /// The project's root folder; the window's project when absent.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

pub struct SetSettings;

impl Command for SetSettings {
    type Params = SetParams;
    type Output = GradleSettings;
    const ID: &'static str = "gradle.settings.set";
    const TITLE: &'static str = "Change Gradle Settings";
    const DESCRIPTION: &'static str = "Changes the project's Gradle settings, one by key and value or several with \
         values, and answers all of them (gradle.settings.get).";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<GradleSettings, CommandError> {
        let root = root(session, params.project)?;
        let env = env(session);
        let mut settings = GradleSettings::read(&env, &root);
        let mut changes: Vec<(String, serde_json::Value)> = params.values.into_iter().collect();
        if let Some(key) = params.key {
            changes.push((key, params.value));
        }
        crate::trust::guard_agent_build_settings(session, changes.iter().map(|(key, _)| key.as_str()), CHOOSE_PROGRAM)?;
        for (key, value) in changes {
            settings = change(&settings, &key, value)?;
        }
        env.set_project_settings(TOOL, &root, &settings).map_err(CommandError::new)?;
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

fn jvm_choice(session: &mut Session, selected: &str) -> Item {
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
    Item::Choice { label: t!("gradle-settings-jvm"), options, selected: Some(selected.to_owned()), action: set("jvm") }
}

pub struct GradleForm;

impl Command for GradleForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "gradle.settings.form";
    const TITLE: &'static str = "Gradle Settings Page";
    const DESCRIPTION: &'static str = "Settings → Build, Execution, Deployment → Build Tools → Gradle as a form: the \
         Gradle user home, work offline, and for the project's build: build and run using, run tests using, the \
         distribution with Download Gradle when none is found, the Gradle JVM, VM options and downloading sources. \
         Its changes run gradle.settings.set.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let Ok(root) = crate::run::project_root(session) else {
            return Ok(Form { items: vec![note(t!("gradle-settings-no-project"))], busy: false });
        };
        let env = env(session);
        let settings = GradleSettings::read(&env, &root);
        let using = |key: &str, selected: Runner, label: String| {
            choice(
                label,
                key,
                selected,
                &[(Runner::Gradle, t!("gradle-settings-using-gradle")), (Runner::Ide, t!("gradle-settings-using-ide"))],
            )
        };
        let mut items = vec![
            field(t!("gradle-settings-user-home"), &settings.user_home, "userHome", Browse::Folder),
            note(t!(
                "gradle-settings-default-path",
                path = GradleSettings::default().user_home_folder(&env.home()).display().to_string()
            )),
            check(t!("gradle-settings-offline"), settings.offline, "offline"),
            Item::Group {
                title: t!(
                    "gradle-settings-linked",
                    project = root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
                ),
            },
            using("buildAndRun", settings.build_and_run, t!("gradle-settings-build-and-run")),
            using("runTests", settings.run_tests, t!("gradle-settings-run-tests")),
            note(t!("gradle-settings-using-note")),
            choice(
                t!("gradle-settings-distribution"),
                "distribution",
                settings.distribution,
                &[
                    (Distribution::Wrapper, t!("gradle-settings-distribution-wrapper")),
                    (Distribution::Installed, t!("gradle-settings-distribution-installed")),
                    (Distribution::Custom, t!("gradle-settings-distribution-custom")),
                ],
            ),
        ];
        if settings.distribution == Distribution::Custom {
            items.push(field(t!("gradle-settings-home-path"), &settings.home_path, "homePath", Browse::Folder));
        }
        let downloading = env.load(super::download::KEY).is_some_and(|load| load.state == LoadState::Loading);
        match executable(&settings, &root, &root, &env, &env.folders("gradle")) {
            Some(found) => {
                items.push(note(t!("gradle-settings-found", program = found.path.display().to_string())));
            }
            None if downloading => {
                items.push(Item::Text { text: t!("gradle-settings-downloading"), tone: Tone::Normal, spinner: true });
            }
            None => {
                let failed = env.load(super::download::KEY).and_then(|load| load.error);
                items.push(Item::Text { text: t!("gradle-settings-none"), tone: Tone::Error, spinner: false });
                items.push(Item::Button {
                    label: t!("gradle-settings-download"),
                    enabled: true,
                    action: Action::command(super::download::DownloadGradle::ID),
                });
                if let Some(error) = failed {
                    items.push(Item::Text { text: error, tone: Tone::Error, spinner: false });
                }
            }
        }
        items.extend([
            jvm_choice(session, &settings.jvm),
            field(t!("gradle-settings-vm-options"), &settings.vm_options, "vmOptions", Browse::None),
            note(t!("gradle-settings-vm-note")),
            check(t!("gradle-settings-download-sources"), settings.download_sources, "downloadSources"),
        ]);
        Ok(Form { items, busy: downloading })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetSettings)?;
    registry.register(SetSettings)?;
    registry.register(GradleForm)?;
    Ok(())
}
