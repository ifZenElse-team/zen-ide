use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{BeforeLaunchTask, CommandLine, Configuration};

use crate::before_launch::{Started, TaskStart};

pub const FILE: &str = ".zen/build-triggers.json";

/// When the IDE runs a task around its own builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum When {
    /// Before Build: before Build Artifacts compiles anything.
    BeforeBuild,
    /// After Build: once Build Artifacts compiled, before the jars are written.
    AfterBuild,
    /// Before Rebuild: before Rebuild Artifacts cleans.
    BeforeRebuild,
    /// After Rebuild: once Rebuild Artifacts compiled, before the jars are written.
    AfterRebuild,
    /// Before Sync: in the Gradle run that reloads the build, before the model; Gradle tasks only.
    BeforeSync,
    /// After Sync: in the Gradle run that reloads the build, after the model; Gradle tasks only.
    AfterSync,
}

/// A task the IDE runs around its builds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Trigger {
    /// When it runs.
    pub when: When,
    /// The task: `{ type, ...options }` of a type that runs programs, as `run.beforeLaunch.types` lists them
    /// (`maven.goal`, `gradle.task`, `ant.target`).
    pub task: BeforeLaunchTask,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Stored {
    triggers: Vec<Trigger>,
}

pub fn saved(root: &Path) -> Vec<Trigger> {
    std::fs::read(root.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .map(|stored| stored.triggers)
        .unwrap_or_default()
}

fn write_saved(root: &Path, triggers: Vec<Trigger>) -> Result<(), CommandError> {
    let failed = |error: &dyn std::fmt::Display| {
        CommandError::new(t!("workbench-build-triggers-write", error = error.to_string()))
    };
    let file = root.join(FILE);
    if triggers.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(failed(&error)),
            _ => Ok(()),
        };
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| failed(&error))?;
    }
    let mut text = serde_json::to_vec_pretty(&Stored { triggers }).unwrap_or_default();
    text.push(b'\n');
    zen_vfs::write_atomic(&file, &text).map_err(|error| failed(&error))
}

pub fn lines(session: &mut Session, root: &Path, when: When) -> Result<Vec<CommandLine>, String> {
    let configuration = Configuration::default();
    let launch = TaskStart { root, configuration: &configuration, chain: &[] };
    let mut found = Vec::new();
    for trigger in saved(root).into_iter().filter(|trigger| trigger.when == when) {
        let Some(task_type) = crate::before_launch::find_type(session, &trigger.task.kind) else {
            continue;
        };
        match task_type.start(session, &trigger.task, &launch)? {
            Started::Lines(lines) => found.extend(lines),
            Started::Done | Started::Skipped(_) => {}
            Started::Run(_) | Started::Wait(_) => {
                return Err(t!("workbench-build-triggers-not-lines", kind = trigger.task.kind.as_str()));
            }
        }
    }
    Ok(found)
}

fn project(session: &Session) -> Result<PathBuf, CommandError> {
    crate::run::project_root(session)
}

/// The tasks the IDE runs around its builds.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerList {
    /// In the order they run.
    pub triggers: Vec<Trigger>,
}

pub struct ListTriggers;

impl Command for ListTriggers {
    type Params = NoParams;
    type Output = TriggerList;
    const ID: &'static str = "build.triggers.list";
    const TITLE: &'static str = "Build Triggers";
    const DESCRIPTION: &'static str = "The tasks run before or after the IDE's builds (Execute Before Build, After \
         Build, Before Rebuild, After Rebuild of a build tool window's goal or task), kept in the project's \
         .zen/build-triggers.json: Build and Rebuild Artifacts run them in their run, each program in turn, and stop \
         at the first that fails. Before Sync and After Sync hold Gradle tasks, which run in the Gradle run that \
         reloads the build.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<TriggerList, CommandError> {
        Ok(TriggerList { triggers: saved(&project(session)?) })
    }
}

/// A trigger to add or remove.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerParams {
    /// When it runs.
    pub when: When,
    /// The task, `{ type, ...options }`.
    pub task: BeforeLaunchTask,
    /// Whether it should be there; absent turns it the other way, as the tool windows' ticked menu items do.
    #[serde(default)]
    pub on: Option<bool>,
}

pub struct SetTrigger;

impl Command for SetTrigger {
    type Params = TriggerParams;
    type Output = TriggerList;
    const ID: &'static str = "build.triggers.set";
    const TITLE: &'static str = "Execute Before or After Build";
    const DESCRIPTION: &'static str = "Adds or removes a task run before or after the IDE's builds (see \
         build.triggers.list): `on` true adds it at the end when it is not there, false removes it, absent turns it \
         the other way. The task's type must run programs (run.beforeLaunch.types lists the types).";

    fn run(&self, session: &mut Session, params: TriggerParams) -> Result<TriggerList, CommandError> {
        let root = project(session)?;
        if crate::before_launch::find_type(session, &params.task.kind).is_none() {
            return Err(CommandError::new(t!(
                "workbench-before-launch-unknown-type",
                kind = params.task.kind.as_str()
            )));
        }
        let around_sync = matches!(params.when, When::BeforeSync | When::AfterSync);
        if around_sync && params.task.kind != crate::gradle::TASK_TASK {
            return Err(CommandError::new(t!("workbench-build-triggers-sync-gradle")));
        }
        let mut triggers = saved(&root);
        let trigger = Trigger { when: params.when, task: params.task };
        let present = triggers.contains(&trigger);
        if params.on.unwrap_or(!present) {
            if !present {
                triggers.push(trigger);
            }
        } else {
            triggers.retain(|known| *known != trigger);
        }
        write_saved(&root, triggers.clone())?;
        Ok(TriggerList { triggers })
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTriggers)?;
    registry.register(SetTrigger)?;
    Ok(())
}
