use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::settings::{MavenSettings, Purpose};
use super::{KIND, MavenRun, env, pom, pom_files, root_pom, selected_profiles, wait_load};
use crate::build_tools::{
    BuildEnv, LoadState, Suggestion, SuggestionKind, Suggestions, complete_line, relative_to, remember_line,
};
use crate::virtual_documents::{self, VirtualDocument, VirtualKind};

pub const TOOL: &str = "mavenWorkspace";

pub const EFFECTIVE_FOLDER: &str = "effective-pom";

/// How the Maven tool window shows the projects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewOptions {
    /// Group Modules: modules under the project that aggregates them; off lists every project at the top.
    pub group_modules: bool,
    /// Show Ignored Projects, greyed; off hides them.
    pub show_ignored: bool,
    /// Show Basic Phases Only: the nine phases people run; off lists every phase of the three lifecycles.
    pub basic_phases: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self { group_modules: true, show_ignored: true, basic_phases: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MavenWorkspace {
    pub profiles: Option<Vec<String>>,
    pub linked: Vec<String>,
    pub root_unlinked: bool,
    pub ignored: Vec<String>,
    pub history: Vec<String>,
    pub view: ViewOptions,
}

impl MavenWorkspace {
    pub fn read(env: &BuildEnv, root: &Path) -> Self {
        env.project_settings(TOOL, root)
    }

    pub fn save(&self, env: &BuildEnv, root: &Path) -> Result<(), CommandError> {
        env.set_project_settings(TOOL, root, self).map_err(CommandError::new)
    }

    pub fn linked_poms(&self, root: &Path) -> Vec<PathBuf> {
        self.linked.iter().map(|pom| root.join(pom)).collect()
    }

    pub fn ignored_poms(&self, root: &Path) -> Vec<PathBuf> {
        self.ignored.iter().map(|pom| root.join(pom)).collect()
    }

    pub fn remember(&mut self, goals: &str) {
        remember_line(&mut self.history, goals);
    }
}

pub fn remember(session: &Session, root: &Path, goals: &str) {
    let env = env(session);
    let mut workspace = MavenWorkspace::read(&env, root);
    workspace.remember(goals);
    let _ = workspace.save(&env, root);
}

fn pom_param(root: &Path, pom: &Path) -> PathBuf {
    let pom = if pom.is_absolute() { pom.to_owned() } else { root.join(pom) };
    if pom.is_dir() { pom.join("pom.xml") } else { pom }
}

fn known_poms(root: &Path, workspace: &MavenWorkspace) -> Vec<PathBuf> {
    let mut poms = pom_files(&pom::read(&root.join("pom.xml")));
    for linked in workspace.linked_poms(root) {
        poms.extend(pom_files(&pom::read(&linked)));
    }
    poms
}

/// The Maven projects linked to the tool window, as the commands that change them answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkedProjects {
    /// Whether the project's own `pom.xml` is linked.
    pub root_linked: bool,
    /// The `pom.xml` files linked beside it.
    pub linked: Vec<PathBuf>,
    /// The `pom.xml` files ignored.
    pub ignored: Vec<PathBuf>,
}

fn linked_projects(root: &Path, workspace: &MavenWorkspace) -> LinkedProjects {
    LinkedProjects {
        root_linked: !workspace.root_unlinked,
        linked: workspace.linked_poms(root),
        ignored: workspace.ignored_poms(root),
    }
}

/// A `pom.xml` of the build.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PomParams {
    /// The `pom.xml`, or its folder: absolute or relative to the project.
    pub pom: PathBuf,
}

pub struct AddProject;

impl Command for AddProject {
    type Params = PomParams;
    type Output = LinkedProjects;
    const ID: &'static str = "maven.project.add";
    const TITLE: &'static str = "Add Maven Project";
    const DESCRIPTION: &'static str = "Links a pom.xml to the Maven tool window beside the project's own, as its + \
         button does: one outside the project's folder or not a module of it; the project's own pom.xml links it \
         again after Unlink. Kept with the project on this computer.";

    fn run(&self, session: &mut Session, params: PomParams) -> Result<LinkedProjects, CommandError> {
        let (root, root_file) = root_pom(session)?;
        let pom = pom_param(&root, &params.pom);
        if !pom.is_file() {
            return Err(CommandError::new(t!("workbench-maven-no-pom-file", pom = pom.display().to_string())));
        }
        if let Some(folder) = pom.parent() {
            crate::trust::guard_build_load(session, folder)?;
        }
        let env = env(session);
        let mut workspace = MavenWorkspace::read(&env, &root);
        if pom == root_file {
            workspace.root_unlinked = false;
        } else {
            let relative = relative_to(&root, &pom.to_string_lossy());
            if !workspace.linked.contains(&relative) {
                workspace.linked.push(relative);
            }
        }
        workspace.save(&env, &root)?;
        Ok(linked_projects(&root, &workspace))
    }
}

pub struct UnlinkProject;

impl Command for UnlinkProject {
    type Params = PomParams;
    type Output = LinkedProjects;
    const ID: &'static str = "maven.project.unlink";
    const TITLE: &'static str = "Unlink Maven Projects";
    const DESCRIPTION: &'static str = "Removes a linked Maven project and its modules from the Maven tool window: the \
         project's own pom.xml or one maven.project.add linked. A module goes with its project; maven.project.add \
         links a project again.";

    fn run(&self, session: &mut Session, params: PomParams) -> Result<LinkedProjects, CommandError> {
        let (root, root_file) = root_pom(session)?;
        let pom = pom_param(&root, &params.pom);
        let env = env(session);
        let mut workspace = MavenWorkspace::read(&env, &root);
        let relative = relative_to(&root, &pom.to_string_lossy());
        if pom == root_file {
            workspace.root_unlinked = true;
        } else if workspace.linked.contains(&relative) {
            workspace.linked.retain(|known| *known != relative);
        } else {
            return Err(CommandError::new(t!("workbench-maven-not-linked", pom = pom.display().to_string())));
        }
        workspace.save(&env, &root)?;
        Ok(linked_projects(&root, &workspace))
    }
}

/// A `pom.xml` to ignore or take back.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreParams {
    /// The `pom.xml`, or its folder: absolute or relative to the project.
    pub pom: PathBuf,
    /// Whether to ignore it; absent turns it the other way.
    #[serde(default)]
    pub ignored: Option<bool>,
}

pub struct IgnoreProject;

impl Command for IgnoreProject {
    type Params = IgnoreParams;
    type Output = LinkedProjects;
    const ID: &'static str = "maven.project.ignore";
    const TITLE: &'static str = "Ignore Projects";
    const DESCRIPTION: &'static str = "Ignores a project of the build, or takes it back (Unignore Projects): an \
         ignored project is greyed in the Maven tool window, or hidden while Show Ignored Projects is off. It \
         changes only the tool window: Maven and the language server still see the project. Kept with the \
         project on this computer.";

    fn run(&self, session: &mut Session, params: IgnoreParams) -> Result<LinkedProjects, CommandError> {
        let (root, _) = root_pom(session)?;
        let pom = pom_param(&root, &params.pom);
        let env = env(session);
        let mut workspace = MavenWorkspace::read(&env, &root);
        if !known_poms(&root, &workspace).contains(&pom) {
            return Err(CommandError::new(t!("workbench-maven-no-module", pom = pom.display().to_string())));
        }
        let relative = relative_to(&root, &pom.to_string_lossy());
        let ignored = workspace.ignored.contains(&relative);
        if params.ignored.unwrap_or(!ignored) {
            if !ignored {
                workspace.ignored.push(relative);
            }
        } else {
            workspace.ignored.retain(|known| *known != relative);
        }
        workspace.save(&env, &root)?;
        Ok(linked_projects(&root, &workspace))
    }
}

/// The view options to change; what is absent stays.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewParams {
    /// Group Modules.
    pub group_modules: Option<bool>,
    /// Show Ignored Projects.
    pub show_ignored: Option<bool>,
    /// Show Basic Phases Only.
    pub basic_phases: Option<bool>,
}

pub struct SetView;

impl Command for SetView {
    type Params = ViewParams;
    type Output = ViewOptions;
    const ID: &'static str = "maven.view.set";
    const TITLE: &'static str = "Maven View Options";
    const DESCRIPTION: &'static str = "Sets the Maven tool window's view options (the eye button): Group Modules, Show \
         Ignored Projects and Show Basic Phases Only; what is not given stays. Kept with the project on this \
         computer; answers the options.";

    fn run(&self, session: &mut Session, params: ViewParams) -> Result<ViewOptions, CommandError> {
        let (root, _) = root_pom(session)?;
        let env = env(session);
        let mut workspace = MavenWorkspace::read(&env, &root);
        let view = &mut workspace.view;
        view.group_modules = params.group_modules.unwrap_or(view.group_modules);
        view.show_ignored = params.show_ignored.unwrap_or(view.show_ignored);
        view.basic_phases = params.basic_phases.unwrap_or(view.basic_phases);
        workspace.save(&env, &root)?;
        Ok(workspace.view)
    }
}

/// What `maven.goal.complete` completes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CompleteParams {
    /// The command line typed so far (`clean ins`); its last word is completed.
    pub text: String,
}

pub fn suggestions(text: &str, history: &[String], goals: &[String]) -> Vec<Suggestion> {
    let phases = pom::FULL_LIFECYCLE.iter().map(|phase| (*phase, SuggestionKind::Phase));
    complete_line(text, history, phases.chain(goals.iter().map(|goal| (goal.as_str(), SuggestionKind::Goal))))
}

fn plugin_goals(project: &pom::PomProject, into: &mut Vec<String>) {
    for plugin in &project.plugins {
        for goal in &plugin.goals {
            let full = format!("{}:{goal}", plugin.prefix);
            if !into.contains(&full) {
                into.push(full);
            }
        }
    }
    for module in &project.modules {
        plugin_goals(module, into);
    }
}

pub struct CompleteGoal;

impl Command for CompleteGoal {
    type Params = CompleteParams;
    type Output = Suggestions;
    const ID: &'static str = "maven.goal.complete";
    const TITLE: &'static str = "Execute Maven Goal: Completion";
    const DESCRIPTION: &'static str = "What Execute Maven Goal suggests for the command line typed: the lines run \
         before that start with it (latest first), then the lifecycle phases and the plug-in goals \
         (`compiler:compile`) of the build's plug-ins that complete its last word.";

    fn run(&self, session: &mut Session, params: CompleteParams) -> Result<Suggestions, CommandError> {
        let (root, root_file) = root_pom(session)?;
        let workspace = MavenWorkspace::read(&env(session), &root);
        let mut goals = Vec::new();
        plugin_goals(&pom::read(&root_file), &mut goals);
        for linked in workspace.linked_poms(&root) {
            plugin_goals(&pom::read(&linked), &mut goals);
        }
        Ok(Suggestions { suggestions: suggestions(&params.text, &workspace.history, &goals) })
    }
}

const SETTINGS_SKELETON: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<settings xmlns="http://maven.apache.org/SETTINGS/1.2.0"
          xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
          xsi:schemaLocation="http://maven.apache.org/SETTINGS/1.2.0 https://maven.apache.org/xsd/settings-1.2.0.xsd">
</settings>
"#;

/// What `maven.settingsFile.open` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsFile {
    /// The `settings.xml` opened.
    pub path: PathBuf,
    /// Whether it was created now.
    pub created: bool,
}

pub struct OpenSettingsFile;

impl Command for OpenSettingsFile {
    type Params = zen_core::NoParams;
    type Output = SettingsFile;
    const ID: &'static str = "maven.settingsFile.open";
    const TITLE: &'static str = "Open settings.xml";
    const DESCRIPTION: &'static str = "Opens the user settings.xml Maven reads (the Maven settings' User settings \
         file, else ~/.m2/settings.xml) in the editor; Create settings.xml when it does not exist writes an empty \
         <settings> there first.";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<SettingsFile, CommandError> {
        let (root, _) = root_pom(session)?;
        let env = env(session);
        let path = MavenSettings::read(&env, &root).user_settings_file(&env.home());
        let created = !path.is_file();
        if created {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| CommandError::new(error.to_string()))?;
            }
            zen_vfs::write_atomic(&path, SETTINGS_SKELETON.as_bytes())
                .map_err(|error| CommandError::new(error.to_string()))?;
        }
        crate::OpenDocument
            .run(session, crate::OpenParams { path: path.clone(), editor_only: true, group: None, preview: false })?;
        Ok(SettingsFile { path, created })
    }
}

/// Which attachments to download.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadParams {
    /// The sources; on unless `false`.
    pub sources: Option<bool>,
    /// The documentation (javadoc); on unless `false`.
    pub documentation: Option<bool>,
}

/// What `maven.sources.download` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Downloads {
    /// The background tasks downloading them (`tasks.log.read`).
    pub tasks: Vec<u64>,
}

pub struct DownloadSources;

impl Command for DownloadSources {
    type Params = DownloadParams;
    type Output = Downloads;
    const ID: &'static str = "maven.sources.download";
    const TITLE: &'static str = "Download Sources and Documentation";
    const DESCRIPTION: &'static str = "Downloads the sources and the documentation of the build's dependencies into the \
         local repository now (mvn dependency:sources and dependency:resolve -Dclassifier=javadoc), in background \
         tasks, whatever the Maven settings' Importing says; uses the network unless Maven works offline.";

    fn run(&self, session: &mut Session, params: DownloadParams) -> Result<Downloads, CommandError> {
        let (root, _) = root_pom(session)?;
        crate::trust::guard_build_load(session, &root)?;
        let env = env(session);
        let tasks = super::download_attachments(
            session,
            &env,
            &root,
            params.sources.unwrap_or(true),
            params.documentation.unwrap_or(true),
        )?;
        Ok(Downloads { tasks })
    }
}

/// Which effective POM to show.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EffectivePomParams {
    /// The project's `pom.xml`, or its folder; the project's own when absent.
    pub pom: Option<PathBuf>,
    /// Start Maven when the effective POM is neither cached nor being computed; on unless `false`.
    pub start: Option<bool>,
    /// Waits up to this long for Maven (at most 10 minutes).
    pub wait_ms: Option<u64>,
}

/// What `maven.effectivePom.show` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EffectivePom {
    /// Where it is: absent when nothing was started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LoadState>,
    /// The read-only document opened, once loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Why Maven failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The background task running Maven (`tasks.log.read`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

pub fn effective_hash(pom: &Path, profiles: &str, offline: bool) -> String {
    let mut text = format!("{}\n{profiles}\n{offline}\n", pom.display());
    for file in pom_files(&pom::read(pom)) {
        text.push_str(&std::fs::read_to_string(&file).unwrap_or_default());
    }
    if let Some(parent) = pom.parent().and_then(Path::parent) {
        text.push_str(&std::fs::read_to_string(parent.join("pom.xml")).unwrap_or_default());
    }
    virtual_documents::hashed(&text)
}

pub fn effective_file(hash: &str) -> PathBuf {
    Path::new(EFFECTIVE_FOLDER).join(hash).join("effective-pom.xml")
}

fn open_effective(session: &mut Session, pom: &Path, hash: &str, text: Option<&str>) -> Result<PathBuf, CommandError> {
    let folder = virtual_documents::folder();
    let relative = effective_file(hash);
    let text = match text {
        Some(text) => text.to_owned(),
        None => {
            std::fs::read_to_string(folder.join(&relative)).map_err(|error| CommandError::new(error.to_string()))?
        }
    };
    let document = VirtualDocument {
        kind: VirtualKind::Generated,
        origin: pom.to_string_lossy().into_owned(),
        library: None,
        bytecode: None,
        class_path: None,
    };
    let path = virtual_documents::store(&folder, &relative, &text, document, None)
        .map_err(|error| CommandError::new(error.to_string()))?;
    crate::OpenDocument
        .run(session, crate::OpenParams { path: path.clone(), editor_only: true, group: None, preview: false })?;
    Ok(path)
}

pub struct ShowEffectivePom;

impl Command for ShowEffectivePom {
    type Params = EffectivePomParams;
    type Output = EffectivePom;
    const ID: &'static str = "maven.effectivePom.show";
    const TITLE: &'static str = "Show Effective POM";
    const DESCRIPTION: &'static str = "Opens a project's effective POM read-only: Maven computes it (mvn \
         help:effective-pom, with the profiles chosen) in a background task, which may download the help plug-in; \
         the result is kept per content of the pom.xml files, so it opens at once until they change. Answers the \
         state, and the document once it is open; call again (start false) to open it once loaded, or give waitMs.";

    fn run(&self, session: &mut Session, params: EffectivePomParams) -> Result<EffectivePom, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let (root, root_file) = root_pom(session)?;
        let pom = params.pom.as_deref().map_or_else(|| root_file.clone(), |pom| pom_param(&root, pom));
        if !pom.is_file() {
            return Err(CommandError::new(t!("workbench-maven-no-module", pom = pom.display().to_string())));
        }
        let env = env(session);
        let settings = MavenSettings::read(&env, &root);
        let profiles = selected_profiles(session, &pom::read(&root_file)).join(",");
        let hash = effective_hash(&pom, &profiles, settings.offline);
        if virtual_documents::folder().join(effective_file(&hash)).is_file() {
            let path = open_effective(session, &pom, &hash, None)?;
            return Ok(EffectivePom { state: Some(LoadState::Loaded), path: Some(path), error: None, task: None });
        }
        let key = format!("maven.effectivePom\n{hash}");
        let failed = env.load(&key).is_some_and(|load| load.state == LoadState::Failed);
        if params.start.unwrap_or(true) && (env.load(&key).is_none() || failed) {
            crate::trust::guard_build_load(session, &root)?;
            start_effective(session, &env, &root, &pom, &profiles, settings.offline, &key)?;
        }
        if let Some(wait) = params.wait_ms {
            wait_load(&env, &key, Duration::from_millis(wait));
        }
        let Some(load) = env.load(&key) else {
            return Ok(EffectivePom { state: None, path: None, error: None, task: None });
        };
        let path = match load.state {
            LoadState::Loaded => {
                env.forget(&key);
                Some(open_effective(session, &pom, &hash, Some(&load.output))?)
            }
            _ => None,
        };
        Ok(EffectivePom { state: Some(load.state), path, error: load.error, task: load.task })
    }
}

fn start_effective(
    session: &mut Session,
    env: &BuildEnv,
    root: &Path,
    pom: &Path,
    profiles: &str,
    offline: bool,
    key: &str,
) -> Result<(), CommandError> {
    let scratch = crate::run::scratch_folder();
    std::fs::create_dir_all(&scratch).map_err(|error| CommandError::new(error.to_string()))?;
    let output = scratch.join("effective-pom.xml");
    let relative = relative_to(root, &pom.to_string_lossy());
    let mut configuration = MavenRun::goals(KIND, &relative, "help:effective-pom", profiles);
    configuration.options["offline"] = serde_json::json!(offline);
    let mut line =
        MavenRun::new(env.clone()).line_for(&configuration, root, Purpose::Import).map_err(CommandError::new)?;
    let flag = format!("-Doutput={}", output.display());
    line.display = format!("{} -B {flag}", line.display);
    line.args.extend(["-B".to_owned(), flag]);
    let title = t!("workbench-maven-effective-task");
    let task = session.state_mut::<crate::tasks::Tasks>().start(&title, KIND, false);
    env.spawn(key, line, Some(task), move |_| {
        let text = std::fs::read_to_string(&output).map_err(|error| format!("{}: {error}", output.display()));
        let _ = std::fs::remove_dir_all(&scratch);
        text
    });
    Ok(())
}

/// The goals of a run configuration to make.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateConfigurationParams {
    /// The goals and phases (`clean install`).
    pub goals: String,
    /// The `pom.xml` they run in, absolute or relative to the project; the project's own when absent.
    #[serde(default)]
    pub pom: Option<PathBuf>,
}

/// What `maven.runConfiguration.create` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedConfiguration {
    /// The new configuration's id.
    pub id: String,
    /// Its name, `artifact [goals]`.
    pub name: String,
}

pub struct CreateRunConfiguration;

impl Command for CreateRunConfiguration {
    type Params = CreateConfigurationParams;
    type Output = CreatedConfiguration;
    const ID: &'static str = "maven.runConfiguration.create";
    const TITLE: &'static str = "Create Run Configuration";
    const DESCRIPTION: &'static str = "Saves a run configuration of kind maven for goals of a project, named `artifact \
         [goals]`, with the profiles chosen in the Maven tool window, and selects it; it is then listed under the \
         project's Run Configurations node.";

    fn run(
        &self,
        session: &mut Session,
        params: CreateConfigurationParams,
    ) -> Result<CreatedConfiguration, CommandError> {
        let (root, root_file) = root_pom(session)?;
        if params.goals.trim().is_empty() {
            return Err(CommandError::new(t!("workbench-maven-no-goals")));
        }
        let pom = params.pom.as_deref().map_or_else(|| root_file.clone(), |pom| pom_param(&root, pom));
        if !pom.is_file() {
            return Err(CommandError::new(t!("workbench-maven-no-module", pom = pom.display().to_string())));
        }
        let project = pom::read(&pom);
        let name = if project.artifact_id.is_empty() { KIND.to_owned() } else { project.artifact_id.clone() };
        let profiles = selected_profiles(session, &pom::read(&root_file)).join(",");
        let relative = relative_to(&root, &pom.to_string_lossy());
        let configuration = MavenRun::goals(&name, &relative, params.goals.trim(), &profiles);
        let created = crate::run::CreateConfiguration.run(
            session,
            crate::run::CreateParams { configuration: configuration.clone(), select: true, template: None },
        )?;
        Ok(CreatedConfiguration { id: created.id, name: configuration.name })
    }
}
