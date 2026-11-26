use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::{GradleRun, build_model, build_root, env, model, relative_to_root};
use crate::build_tools::{BuildEnv, Suggestion, SuggestionKind, Suggestions, complete_line, remember_line};
use crate::maven::workspace::CreatedConfiguration;

pub const TOOL: &str = "gradleWorkspace";

const OPTIONS: [&str; 10] = [
    "--build-cache",
    "--continue",
    "--debug",
    "--dry-run",
    "--info",
    "--offline",
    "--parallel",
    "--refresh-dependencies",
    "--rerun-tasks",
    "--stacktrace",
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GradleWorkspace {
    pub linked: Vec<String>,
    pub history: Vec<String>,
}

impl GradleWorkspace {
    pub fn read(env: &BuildEnv, root: &Path) -> Self {
        env.project_settings(TOOL, root)
    }

    fn save(&self, env: &BuildEnv, root: &Path) -> Result<(), CommandError> {
        env.set_project_settings(TOOL, root, self).map_err(CommandError::new)
    }

    pub fn linked_folders(&self, root: &Path) -> Vec<PathBuf> {
        self.linked.iter().map(|folder| root.join(folder)).collect()
    }
}

pub fn build_folders(env: &BuildEnv, root: &Path) -> Vec<PathBuf> {
    std::iter::once(root.to_owned()).chain(GradleWorkspace::read(env, root).linked_folders(root)).collect()
}

pub fn project_path(root: &Path, path: Option<&Path>) -> String {
    match path {
        Some(path) if path.is_absolute() => relative_to_root(root, path),
        Some(path) if !path.as_os_str().is_empty() => relative_to_root(root, &root.join(path)),
        _ => ".".to_owned(),
    }
}

pub fn remember(session: &Session, root: &Path, tasks: &str) {
    let env = env(session);
    let mut workspace = GradleWorkspace::read(&env, root);
    remember_line(&mut workspace.history, tasks);
    let _ = workspace.save(&env, root);
}

fn build_folder(root: &Path, path: &Path) -> PathBuf {
    let path = if path.is_absolute() { path.to_owned() } else { root.join(path) };
    if path.is_file() { path.parent().map_or_else(|| root.to_owned(), Path::to_owned) } else { path }
}

/// A Gradle build to link or unlink.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildParams {
    /// The build's folder or one of its build scripts: absolute or relative to the project.
    pub path: PathBuf,
}

/// The Gradle builds linked to the tool window beside the project's own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkedBuilds {
    /// Their folders.
    pub linked: Vec<PathBuf>,
}

pub struct AddBuild;

impl Command for AddBuild {
    type Params = BuildParams;
    type Output = LinkedBuilds;
    const ID: &'static str = "gradle.project.add";
    const TITLE: &'static str = "Add Gradle Project";
    const DESCRIPTION: &'static str = "Links a Gradle build to the Gradle tool window beside the project's own, as its + \
         button does, and has Gradle load its model: a folder with a build or settings script, or one of those \
         scripts. Its code runs, so the folder must be trusted. Kept with the project on this computer.";

    fn run(&self, session: &mut Session, params: BuildParams) -> Result<LinkedBuilds, CommandError> {
        let root = build_root(session)?;
        let folder = build_folder(&root, &params.path);
        if !model::is_build(&folder) {
            return Err(CommandError::new(t!("workbench-gradle-no-build-at", folder = folder.display().to_string())));
        }
        crate::trust::guard_project_build(session, &folder)?;
        let env = env(session);
        let mut workspace = GradleWorkspace::read(&env, &root);
        let linked = relative_to_root(&root, &folder);
        if folder != root && !workspace.linked.contains(&linked) {
            workspace.linked.push(linked);
            workspace.save(&env, &root)?;
            build_model::sync(session, &env, &root, std::slice::from_ref(&folder));
        }
        Ok(LinkedBuilds { linked: workspace.linked_folders(&root) })
    }
}

pub struct UnlinkBuild;

impl Command for UnlinkBuild {
    type Params = BuildParams;
    type Output = LinkedBuilds;
    const ID: &'static str = "gradle.project.unlink";
    const TITLE: &'static str = "Unlink Gradle Project";
    const DESCRIPTION: &'static str =
        "Removes a build gradle.project.add linked from the Gradle tool window; its files stay.";

    fn run(&self, session: &mut Session, params: BuildParams) -> Result<LinkedBuilds, CommandError> {
        let root = build_root(session)?;
        let env = env(session);
        let folder = build_folder(&root, &params.path);
        let mut workspace = GradleWorkspace::read(&env, &root);
        let linked = relative_to_root(&root, &folder);
        if !workspace.linked.contains(&linked) {
            return Err(CommandError::new(t!("workbench-gradle-not-linked", folder = folder.display().to_string())));
        }
        workspace.linked.retain(|known| *known != linked);
        workspace.save(&env, &root)?;
        Ok(LinkedBuilds { linked: workspace.linked_folders(&root) })
    }
}

/// The parameters of `gradle.task.complete`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompleteParams {
    /// The command line typed so far (`clean :app:te`); its last word is completed.
    pub text: String,
    /// The build's root folder, absolute or relative to the project; the project's own when absent.
    #[serde(default)]
    pub project_path: Option<PathBuf>,
}

pub fn suggestions(text: &str, history: &[String], projects: &[build_model::ModelProject]) -> Vec<Suggestion> {
    let mut names: Vec<String> = Vec::new();
    for project in projects {
        for task in &project.tasks {
            if project.path == ":" {
                names.push(task.name.clone());
            }
            let path =
                model::GradleTask { project: project.path.clone(), name: task.name.clone(), ..Default::default() };
            names.push(path.path());
        }
    }
    names.sort_by_key(|name| (name.starts_with(':'), name.clone()));
    names.dedup();
    let tasks = names.iter().map(|name| (name.as_str(), SuggestionKind::Task));
    complete_line(text, history, tasks.chain(OPTIONS.iter().map(|option| (*option, SuggestionKind::Option))))
}

pub struct CompleteTask;

impl Command for CompleteTask {
    type Params = CompleteParams;
    type Output = Suggestions;
    const ID: &'static str = "gradle.task.complete";
    const TITLE: &'static str = "Execute Gradle Task: Completion";
    const DESCRIPTION: &'static str = "What Execute Gradle Task suggests for the command line typed: the lines run \
         before that start with it (latest first), then the build's tasks (`build`, `:app:run`, `:tools:core:test`) \
         and Gradle's common options that complete its last word.";

    fn run(&self, session: &mut Session, params: CompleteParams) -> Result<Suggestions, CommandError> {
        let root = build_root(session)?;
        let env = env(session);
        let path = project_path(&root, params.project_path.as_deref());
        let projects = build_model::shown_projects(&env, &root.join(path));
        let history = GradleWorkspace::read(&env, &root).history;
        Ok(Suggestions { suggestions: suggestions(&params.text, &history, &projects) })
    }
}

/// The tasks of a run configuration to make.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateConfigurationParams {
    /// The tasks and their options (`:app:run --args="x"`).
    pub tasks: String,
    /// The build's root folder, absolute or relative to the project; the project's own when absent.
    #[serde(default)]
    pub project_path: Option<PathBuf>,
}

pub struct CreateRunConfiguration;

impl Command for CreateRunConfiguration {
    type Params = CreateConfigurationParams;
    type Output = CreatedConfiguration;
    const ID: &'static str = "gradle.runConfiguration.create";
    const TITLE: &'static str = "Create Gradle Run Configuration";
    const DESCRIPTION: &'static str = "Saves a run configuration of kind gradle for tasks of a build, named `project \
         [tasks]`, and selects it; it is then listed under the build's Run Configurations node.";

    fn run(
        &self,
        session: &mut Session,
        params: CreateConfigurationParams,
    ) -> Result<CreatedConfiguration, CommandError> {
        let root = build_root(session)?;
        if params.tasks.trim().is_empty() {
            return Err(CommandError::new(t!("workbench-gradle-no-tasks")));
        }
        let path = project_path(&root, params.project_path.as_deref());
        let name = model::projects(&root.join(&path)).first().map(|project| project.name.clone()).unwrap_or_default();
        let configuration = GradleRun::tasks(&name, &path, params.tasks.trim());
        let created = crate::run::CreateConfiguration.run(
            session,
            crate::run::CreateParams { configuration: configuration.clone(), select: true, template: None },
        )?;
        Ok(CreatedConfiguration { id: created.id, name: configuration.name })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(AddBuild)?;
    registry.register(UnlinkBuild)?;
    registry.register(CompleteTask)?;
    registry.register(CreateRunConfiguration)?;
    Ok(())
}
