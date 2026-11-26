use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::support::ProjectSupport;

use crate::run::{RunStarted, project_root, wait_for_run};

pub const FILE: &str = "plugin-projects.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginProject {
    pub id: String,
    pub name: String,
    pub support: ProjectSupport,
}

pub type ProjectSource = Arc<dyn Fn() -> Vec<PluginProject> + Send + Sync>;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Done {
    configurations: BTreeSet<String>,
    setups: BTreeSet<String>,
}

#[derive(Default)]
pub struct PluginProjects {
    source: Option<ProjectSource>,
    data: Option<PathBuf>,
    done: Done,
}

impl PluginProjects {
    pub fn set_source(&mut self, source: ProjectSource) {
        self.source = Some(source);
    }

    pub fn source(&self) -> Option<ProjectSource> {
        self.source.clone()
    }

    pub fn set(&mut self, plugins: Vec<PluginProject>) {
        self.set_source(Arc::new(move || plugins.clone()));
    }

    pub fn set_data_dir(&mut self, data: PathBuf) {
        self.data = Some(data);
    }

    pub fn installed(&self, id: &str) -> bool {
        self.source.as_ref().is_some_and(|source| source().iter().any(|plugin| plugin.id == id))
    }

    pub fn supporting(&self, root: &Path) -> Vec<PluginProject> {
        let plugins = self.source.as_ref().map(|source| source()).unwrap_or_default();
        plugins.into_iter().filter(|plugin| plugin.support.supports(root)).collect()
    }

    fn read(&self) -> Done {
        match &self.data {
            Some(data) => std::fs::read(data.join(FILE))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default(),
            None => self.done.clone(),
        }
    }

    fn write(&mut self, done: Done) {
        match &self.data {
            Some(data) => {
                let _ = std::fs::create_dir_all(data);
                let _ = zen_vfs::write_atomic(&data.join(FILE), &serde_json::to_vec_pretty(&done).unwrap_or_default());
            }
            None => self.done = done,
        }
    }
}

fn key(plugin: &str, root: &Path) -> String {
    format!("{plugin}\n{}", root.display())
}

fn supporting(session: &Session, root: &Path) -> Vec<PluginProject> {
    session.state::<PluginProjects>().map(|projects| projects.supporting(root)).unwrap_or_default()
}

/// A plug-in's setup step, as the commands show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupInfo {
    /// The plug-in's id, for `plugins.project.setup`.
    pub plugin: String,
    /// The plug-in's name.
    pub plugin_name: String,
    /// The name of the configuration that runs the step, e.g. `nuxi prepare`.
    pub name: String,
    /// What the step creates, relative to the project, e.g. `.nuxt`.
    pub creates: String,
    /// Whether it is missing, so that the step is needed.
    pub needed: bool,
}

/// A plug-in that supports the open project, as `plugins.project.list` shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPlugin {
    /// The plug-in's id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The names of the run configurations it adds.
    pub run_configurations: Vec<String>,
    /// Its setup step, if it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup: Option<SetupInfo>,
}

fn setup_info(plugin: &PluginProject, root: &Path) -> Option<SetupInfo> {
    let setup = plugin.support.setup.as_ref()?;
    Some(SetupInfo {
        plugin: plugin.id.clone(),
        plugin_name: plugin.name.clone(),
        name: setup.configuration.name.clone(),
        creates: setup.creates.clone(),
        needed: !setup.done(root),
    })
}

/// What `plugins.project.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPlugins {
    /// The installed plug-ins that are on and support the open project.
    pub plugins: Vec<ProjectPlugin>,
}

pub struct ListProjectPlugins;

impl Command for ListProjectPlugins {
    type Params = NoParams;
    type Output = ProjectPlugins;
    const ID: &'static str = "plugins.project.list";
    const TITLE: &'static str = "Plugins of the Project";
    const DESCRIPTION: &'static str = "The installed plug-ins that support the open project by their project markers \
         (Nuxt: nuxt.config.* at the root, or nuxt in package.json), with the run configurations each adds and its \
         setup step (Nuxt: nuxi prepare) and whether that is needed now.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ProjectPlugins, CommandError> {
        let root = project_root(session)?;
        let plugins = supporting(session, &root)
            .iter()
            .map(|plugin| ProjectPlugin {
                id: plugin.id.clone(),
                name: plugin.name.clone(),
                run_configurations: plugin.support.configurations_for(&root).iter().map(|c| c.name.clone()).collect(),
                setup: setup_info(plugin, &root),
            })
            .collect();
        Ok(ProjectPlugins { plugins })
    }
}

/// What `plugins.project.apply` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// The run configurations added to the project, with their ids.
    pub added: Vec<AddedConfiguration>,
    /// The setup steps to offer: needed, and not offered in this project before.
    pub offers: Vec<SetupInfo>,
}

/// A run configuration a plug-in added.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddedConfiguration {
    /// The plug-in's id.
    pub plugin: String,
    /// The configuration's id.
    pub id: String,
    /// Its name.
    pub name: String,
}

pub struct ApplyProjectPlugins;

impl Command for ApplyProjectPlugins {
    type Params = NoParams;
    type Output = Applied;
    const ID: &'static str = "plugins.project.apply";
    const TITLE: &'static str = "Apply Plugins to the Project";
    const DESCRIPTION: &'static str = "For each installed plug-in that supports the open project: adds the run \
         configurations it declares that the project does not have (same kind and name), once per project, so that \
         deleted ones do not come back; and answers its setup step when that is needed and was not offered in this \
         project before, marking it offered. The window calls it when the project opens and when plug-ins change, \
         and offers the setups in a notification.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Applied, CommandError> {
        let root = project_root(session)?;
        let plugins = supporting(session, &root);
        if plugins.is_empty() {
            return Ok(Applied { added: Vec::new(), offers: Vec::new() });
        }
        let mut done = session.state_mut::<PluginProjects>().read();
        let mut applied = Applied { added: Vec::new(), offers: Vec::new() };
        for plugin in &plugins {
            let key = key(&plugin.id, &root);
            let configurations = plugin.support.configurations_for(&root);
            if !configurations.is_empty() && !done.configurations.contains(&key) {
                let added = crate::run::add_missing(session, &root, configurations)?;
                applied.added.extend(added.into_iter().map(|configuration| AddedConfiguration {
                    plugin: plugin.id.clone(),
                    id: configuration.id,
                    name: configuration.name,
                }));
                done.configurations.insert(key.clone());
            }
            if let Some(setup) = setup_info(plugin, &root).filter(|setup| setup.needed)
                && done.setups.insert(key)
            {
                applied.offers.push(setup);
            }
        }
        session.state_mut::<PluginProjects>().write(done);
        Ok(applied)
    }
}

/// The parameters of `plugins.project.setup`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupParams {
    /// The plug-in whose setup step to run, e.g. `nuxt`; the first whose step is needed, else the
    /// first with a step, when absent.
    #[serde(default)]
    pub plugin: Option<String>,
    /// Waits up to this long for the step to end (at most 10 minutes) and answers its summary, as
    /// `run.process.start` does.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct RunProjectSetup;

impl Command for RunProjectSetup {
    type Params = SetupParams;
    type Output = RunStarted;
    const ID: &'static str = "plugins.project.setup";
    const TITLE: &'static str = "Run Project Setup";
    const DESCRIPTION: &'static str = "Runs the setup step a plug-in declares for the open project (Nuxt: nuxi \
         prepare, which writes .nuxt/ so that the Vue language server sees Nuxt's auto-imports) with its output in \
         the Run tool window, and returns the run's id; with waitMs it answers once the step has ended, with its \
         summary. Without plugin, the first plug-in whose step is needed. Refused when the plug-in does not support the \
         project or has no setup step, and with code notTrusted in safe mode.";

    fn run(&self, session: &mut Session, params: SetupParams) -> Result<RunStarted, CommandError> {
        let root = project_root(session)?;
        let plugins = supporting(session, &root);
        let plugin = match &params.plugin {
            Some(id) => plugins
                .into_iter()
                .find(|plugin| plugin.id == *id)
                .ok_or_else(|| CommandError::new(t!("workbench-plugin-project-unsupported", plugin = id.as_str())))?,
            None => {
                let with_setup: Vec<PluginProject> =
                    plugins.into_iter().filter(|plugin| plugin.support.setup.is_some()).collect();
                let needed = with_setup.iter().position(|plugin| plugin.support.needs_setup(&root)).unwrap_or(0);
                with_setup
                    .into_iter()
                    .nth(needed)
                    .ok_or_else(|| CommandError::new(t!("workbench-plugin-project-no-setups")))?
            }
        };
        let setup =
            plugin.support.setup.clone().ok_or_else(|| {
                CommandError::new(t!("workbench-plugin-project-no-setup", plugin = plugin.name.as_str()))
            })?;
        crate::trust::guard_run(session, &root, &setup.configuration)?;
        let started = crate::run::start(session, setup.configuration)?;
        match params.wait_ms.map(Duration::from_millis) {
            Some(wait) => {
                let summary = wait_for_run(session, started.run_id, wait)?;
                Ok(RunStarted { summary: Some(summary), ..started })
            }
            None => Ok(started),
        }
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListProjectPlugins)?;
    registry.register(ApplyProjectPlugins)?;
    registry.register(RunProjectSetup)?;
    Ok(())
}

#[cfg(test)]
mod tests;
