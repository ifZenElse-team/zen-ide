use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use zen_core::{CommandError, Session};
use zen_i18n::t;

use super::{GradleRun, KIND, init_script, model, problems, relative_to_root};
use crate::build_sync::{Severity, SyncMessage, SyncRun};
use crate::build_tools::{BuildEnv, Load, LoadState};
use crate::build_triggers::{self, When};

pub const MODEL_SCRIPT: &str = include_str!("zen-model.gradle");
pub const MODEL_TASK: &str = ":zenModel";

const OUT_PROPERTY: &str = "zen.model.out";
const OTHER_INPUTS: [&str; 3] =
    ["gradle.properties", "gradle/libs.versions.toml", "gradle/wrapper/gradle-wrapper.properties"];

/// A task of a project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelTask {
    /// Its name, e.g. `build`.
    pub name: String,
    /// Its group as the build gives it; `other` for a task without one.
    pub group: String,
    /// Its description, when it has one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// A source set of a project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelSourceSet {
    /// Its name, e.g. `main`.
    pub name: String,
    /// Its source folders.
    pub sources: Vec<PathBuf>,
    /// Its resource folders.
    pub resources: Vec<PathBuf>,
    /// The folders it compiles to.
    pub output: Vec<PathBuf>,
}

/// A dependency as Gradle resolved it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResolvedDependency {
    /// The group of a library.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group_id: String,
    /// The artifact of a library.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub artifact_id: String,
    /// The version selected.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// The project it is, for a dependency on a project of the build: `:lib`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub project: String,
    /// What the build asked for, when that is not what was selected.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub requested: String,
    /// Why it was not resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved: Option<String>,
    /// It is listed in full earlier in the same configuration.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub repeated: bool,
    /// What it depends on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ResolvedDependency>,
}

/// A resolved configuration of a project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResolvedConfiguration {
    /// Its name, e.g. `compileClasspath`.
    pub name: String,
    /// Its direct dependencies.
    pub dependencies: Vec<ResolvedDependency>,
    /// Why it could not be resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A project of the build, as Gradle answers it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelProject {
    /// The build it belongs to: `:` for the project's own, `:tools` for an included build, `:buildSrc`.
    pub build: String,
    /// Its path from the root build: `:`, `:app`, `:tools:core`.
    pub path: String,
    /// Its name.
    pub name: String,
    /// Its folder.
    pub folder: PathBuf,
    /// Its build script, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_file: Option<PathBuf>,
    /// Its tasks.
    pub tasks: Vec<ModelTask>,
    /// Its source sets.
    pub source_sets: Vec<ModelSourceSet>,
    /// Its resolved class paths and annotation processor paths.
    pub configurations: Vec<ResolvedConfiguration>,
}

impl From<model::GradleProject> for ModelProject {
    fn from(project: model::GradleProject) -> Self {
        Self {
            build: project.build,
            path: project.path,
            name: project.name,
            folder: project.folder,
            build_file: project.build_file,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub hash: String,
    pub projects: Vec<ModelProject>,
}

pub fn key(folder: &Path) -> String {
    format!("gradle.tasks\n{}", folder.display())
}

fn cache_file(env: &BuildEnv, folder: &Path) -> Option<PathBuf> {
    let name = hex::encode(sha2::Sha256::digest(folder.to_string_lossy().as_bytes()));
    Some(env.data_dir()?.join("caches").join("gradle-model").join(format!("{}.json", &name[..16])))
}

fn out_folder(folder: &Path) -> PathBuf {
    let name = hex::encode(sha2::Sha256::digest(folder.to_string_lossy().as_bytes()));
    std::env::temp_dir().join("zen-ide").join(format!("gradle-model-{}", &name[..16]))
}

fn inputs(folder: &Path, projects: &[ModelProject]) -> BTreeSet<PathBuf> {
    let mut files: BTreeSet<PathBuf> = OTHER_INPUTS.iter().map(|name| folder.join(name)).collect();
    let read = model::builds(folder).into_iter().map(|project| (project.folder, project.build_file));
    let known = projects.iter().map(|project| (project.folder.clone(), project.build_file.clone()));
    for (project, build_file) in read.chain(known) {
        files.extend(model::settings_file(&project));
        files.extend(build_file);
    }
    files.retain(|file| file.is_file());
    files
}

pub fn input_hash(folder: &Path, projects: &[ModelProject]) -> String {
    let mut hasher = sha2::Sha256::new();
    for file in inputs(folder, projects) {
        hasher.update(file.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(std::fs::read(&file).unwrap_or_default());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

pub fn read_answers(out: &Path) -> Result<Vec<ModelProject>, String> {
    let mut projects = Vec::new();
    for entry in std::fs::read_dir(out).map_err(|error| error.to_string())?.flatten() {
        let bytes = std::fs::read(entry.path()).map_err(|error| error.to_string())?;
        projects.push(serde_json::from_slice::<ModelProject>(&bytes).map_err(|error| error.to_string())?);
    }
    if projects.is_empty() {
        return Err(t!("workbench-gradle-model-empty"));
    }
    projects.sort_by_key(|project| (project.build != ":", project.build.clone(), project.path.clone()));
    Ok(projects)
}

fn keep(env: &BuildEnv, folder: &Path, projects: Vec<ModelProject>) {
    let kept = Kept { hash: input_hash(folder, &projects), projects };
    if let Some(file) = cache_file(env, folder) {
        let text = serde_json::to_vec(&kept).unwrap_or_default();
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = zen_vfs::write_atomic(&file, &text);
    }
    env.set_gradle_model(folder, Arc::new(kept));
}

pub fn restore(env: &BuildEnv, folder: &Path) -> Option<Arc<Kept>> {
    if let Some(kept) = env.gradle_model(folder) {
        return Some(kept);
    }
    let bytes = std::fs::read(cache_file(env, folder)?).ok()?;
    let kept = Arc::new(serde_json::from_slice::<Kept>(&bytes).ok()?);
    env.set_gradle_model(folder, kept.clone());
    Some(kept)
}

pub fn fresh(env: &BuildEnv, folder: &Path) -> bool {
    restore(env, folder).is_some_and(|kept| kept.hash == input_hash(folder, &kept.projects))
}

fn around_sync(root: &Path, when: When) -> Vec<String> {
    build_triggers::saved(root)
        .into_iter()
        .filter(|trigger| trigger.when == when && trigger.task.kind == super::TASK_TASK)
        .map(|trigger| trigger.task.text("tasks").to_owned())
        .filter(|tasks| !tasks.trim().is_empty())
        .collect()
}

pub fn start(session: &mut Session, env: &BuildEnv, root: &Path, folder: &Path) -> Result<(), CommandError> {
    let script = init_script("zen-model", MODEL_SCRIPT).map_err(CommandError::new)?;
    let out = out_folder(folder);
    let _ = std::fs::remove_dir_all(&out);
    let mut tasks = around_sync(root, When::BeforeSync);
    tasks.push(MODEL_TASK.to_owned());
    tasks.extend(around_sync(root, When::AfterSync));
    let path = relative_to_root(root, folder);
    let configuration = GradleRun::tasks("model", &path, &tasks.join(" "));
    let mut line = GradleRun::new(env.clone()).command_line(&configuration, root).map_err(CommandError::new)?;
    let extra = [
        "-q".to_owned(),
        "--console=plain".to_owned(),
        "--no-configuration-cache".to_owned(),
        "-I".to_owned(),
        script.display().to_string(),
        format!("-D{OUT_PROPERTY}={}", out.display()),
    ];
    line.display = format!("{} {}", line.display, extra.join(" "));
    line.args.extend(extra);
    let task = session.state_mut::<crate::tasks::Tasks>().start(&t!("workbench-gradle-tasks-task"), "gradle", false);
    let owner = env.clone();
    let built = folder.to_owned();
    env.spawn(&key(folder), line, Some(task), move |output| {
        let projects = read_answers(&out)?;
        let _ = std::fs::remove_dir_all(&out);
        keep(&owner, &built, projects);
        Ok(output.to_owned())
    });
    Ok(())
}

pub fn sync(session: &mut Session, env: &BuildEnv, root: &Path, folders: &[PathBuf]) {
    let mut sync = SyncRun::start(KIND, &t!("workbench-gradle-kind"), root);
    start_all(session, env, root, folders, &mut sync);
    sync.finish(session);
}

pub fn start_all(session: &mut Session, env: &BuildEnv, root: &Path, folders: &[PathBuf], sync: &mut SyncRun) {
    for folder in folders {
        let started = Instant::now();
        let title = t!("workbench-sync-gradle-tasks");
        match start(session, env, root, folder) {
            Ok(()) => sync.background(title, key(folder), problems::problems),
            Err(error) => sync.step(title, started, vec![SyncMessage::new(Severity::Error, error.to_string())]),
        }
    }
}

pub fn mark_cached(env: &BuildEnv, folder: &Path) {
    if env.load(&key(folder)).is_none() {
        let load = Load {
            state: LoadState::Loaded,
            output: String::new(),
            error: None,
            task: None,
            started: Instant::now(),
            took: None,
        };
        env.set_load(&key(folder), load);
    }
}

pub fn shown_projects(env: &BuildEnv, folder: &Path) -> Vec<ModelProject> {
    match restore(env, folder) {
        Some(kept) => kept.projects.clone(),
        None => model::builds(folder).into_iter().map(ModelProject::from).collect(),
    }
}

/// Where the loading of a build's model is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelState {
    /// The build's root folder.
    pub folder: PathBuf,
    /// Where it is; absent before Gradle was asked in this session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LoadState>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn model_state(env: &BuildEnv, folder: &Path) -> ModelState {
    let load = env.load(&key(folder));
    ModelState {
        folder: folder.to_owned(),
        state: load.as_ref().map(|load| load.state.clone()),
        error: load.and_then(|load| load.error),
    }
}

/// A build as far as its model is loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildModel {
    /// The build's root folder.
    pub folder: PathBuf,
    /// The same folder as run configurations and `gradle.task.run` take it: `.` for the project's own.
    pub project_path: String,
    /// Where its loading is; absent before Gradle was asked in this session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LoadState>,
    /// Why the last loading failed; `projects` then stays the last good model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The background task that loads it (`tasks.log.read`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
    /// The hash of the build files the model was made from; absent while nothing was loaded and the
    /// projects come from the settings script.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Its projects, those of its included builds and of `buildSrc`, the root first.
    pub projects: Vec<ModelProject>,
}

pub fn build_model(env: &BuildEnv, root: &Path, folder: &Path) -> BuildModel {
    let load = env.load(&key(folder));
    BuildModel {
        folder: folder.to_owned(),
        project_path: relative_to_root(root, folder),
        state: load.as_ref().map(|load| load.state.clone()),
        error: load.as_ref().and_then(|load| load.error.clone()),
        task: load.as_ref().and_then(|load| load.task),
        hash: restore(env, folder).map(|kept| kept.hash.clone()),
        projects: shown_projects(env, folder),
    }
}
