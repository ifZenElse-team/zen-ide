use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogStatus {
    NotFetched,
    Loading,
    Ready,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub update: Option<String>,
    pub bundled: bool,
    pub enabled: bool,
    pub installing: bool,
    pub extensions: Vec<String>,
    pub names: Vec<String>,
    pub language_servers: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub person_only: Vec<String>,
    pub tool_windows: Vec<String>,
}

impl PluginSummary {
    pub fn matches(&self, query: &str) -> bool {
        let haystack = [self.name.as_str(), self.id.as_str(), self.description.as_str()]
            .into_iter()
            .chain(self.extensions.iter().map(String::as_str))
            .chain(self.names.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
    }
}

pub fn sort(plugins: &mut [PluginSummary]) {
    plugins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginList {
    pub catalog: CatalogStatus,
    pub plugins: Vec<PluginSummary>,
    pub installing: Vec<String>,
    pub restart_required: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_failed: Option<String>,
}

pub const NOTES_LIMIT: usize = 64 * 1024;

pub fn clip_notes(notes: &str) -> String {
    if notes.len() <= NOTES_LIMIT {
        return notes.to_owned();
    }
    let end = (0..=NOTES_LIMIT).rev().find(|&index| notes.is_char_boundary(index)).unwrap_or(0);
    format!("{}…", &notes[..end])
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionNotes {
    pub version: String,
    pub date: String,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetails {
    #[serde(flatten)]
    pub plugin: PluginSummary,
    pub notes: Vec<VersionNotes>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Queued {
    pub queued: Vec<String>,
    pub installing: Vec<String>,
}

pub trait PluginManager: Send + Sync {
    fn catalog(&self, query: &str) -> PluginList;
    fn installed(&self, query: &str) -> PluginList;
    fn details(&self, id: &str) -> Result<PluginDetails, String>;
    fn install(&self, id: &str) -> Result<Queued, String>;
    fn update(&self, id: &str) -> Result<Queued, String>;
    fn update_all(&self) -> Result<Queued, String>;
    fn refresh(&self) -> PluginList;
    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String>;
    fn uninstall(&self, id: &str) -> Result<(), String>;
    fn agent_may_install(&self, id: Option<&str>) -> Result<(), String> {
        let _ = id;
        Ok(())
    }
    fn agent_may_enable(&self, id: &str) -> Result<(), String> {
        let _ = id;
        Ok(())
    }
}

pub const USER_ONLY: &str = "userOnly";

fn agent_may_install(session: &Session, plugins: &SharedPlugins, id: Option<&str>) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    plugins.agent_may_install(id).map_err(|message| CommandError::with_code(USER_ONLY, message))
}

pub type SharedPlugins = Arc<dyn PluginManager>;

/// The parameters of `plugins.catalog.list` and `plugins.installed.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    /// Words to look for in the name, id, description and file types; all of them must match.
    #[serde(default)]
    pub query: Option<String>,
}

/// The parameters of the commands on one plug-in.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginIdParams {
    /// The plug-in id, from `plugins.catalog.list` or `plugins.installed.list`.
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    pub id: String,
    pub restart_required: Vec<String>,
}

fn failed(error: String) -> CommandError {
    CommandError::new(error)
}

pub struct ListCatalog(SharedPlugins);

impl Command for ListCatalog {
    type Params = ListParams;
    type Output = PluginList;
    const ID: &'static str = "plugins.catalog.list";
    const TITLE: &'static str = "Marketplace Plugins";
    const DESCRIPTION: &'static str = "Lists the plug-ins of the signed catalog (Settings → Plugins → Marketplace) matching query, sorted by name: \
         id, name, description, the installed and newest versions, an update, bundled, enabled, file types. Fetches the catalog the first time; \
         while catalog is loading, call again.";

    fn run(&self, _session: &mut Session, params: ListParams) -> Result<PluginList, CommandError> {
        Ok(self.0.catalog(params.query.as_deref().unwrap_or_default()))
    }
}

pub struct ListInstalled(SharedPlugins);

impl Command for ListInstalled {
    type Params = ListParams;
    type Output = PluginList;
    const ID: &'static str = "plugins.installed.list";
    const TITLE: &'static str = "Installed Plugins";
    const DESCRIPTION: &'static str = "Lists the installed plug-ins, bundled ones included (Settings → Plugins → Installed), matching query, sorted by name; \
         update is the newer version of a catalog already fetched, and installing what is being installed now.";

    fn run(&self, _session: &mut Session, params: ListParams) -> Result<PluginList, CommandError> {
        Ok(self.0.installed(params.query.as_deref().unwrap_or_default()))
    }
}

pub struct PluginDetailsCommand(SharedPlugins);

impl Command for PluginDetailsCommand {
    type Params = PluginIdParams;
    type Output = PluginDetails;
    const ID: &'static str = "plugins.plugin.details";
    const TITLE: &'static str = "Plugin Details";
    const DESCRIPTION: &'static str = "Describes a plug-in as its details pane does, with the release notes (Markdown) of the newest version and, \
         when older, of the installed one.";

    fn run(&self, _session: &mut Session, params: PluginIdParams) -> Result<PluginDetails, CommandError> {
        self.0.details(&params.id).map_err(failed)
    }
}

pub struct InstallPlugin(SharedPlugins);

impl Command for InstallPlugin {
    type Params = PluginIdParams;
    type Output = Queued;
    const ID: &'static str = "plugins.plugin.install";
    const TITLE: &'static str = "Install Plugin";
    const DESCRIPTION: &'static str = "Downloads and installs the newest version of a catalog plug-in in the background, after the \
         plug-ins it requires that are not installed yet (Nuxt's Vue); it works at once, and the language servers it \
         declares that are not found yet are installed after it (lsp.servers.list shows them, with source local once \
         installed). The catalog must be fetched first (plugins.catalog.list).";

    fn run(&self, session: &mut Session, params: PluginIdParams) -> Result<Queued, CommandError> {
        agent_may_install(session, &self.0, Some(&params.id))?;
        self.0.install(&params.id).map_err(failed)
    }
}

pub struct UpdatePlugin(SharedPlugins);

impl Command for UpdatePlugin {
    type Params = PluginIdParams;
    type Output = Queued;
    const ID: &'static str = "plugins.plugin.update";
    const TITLE: &'static str = "Update Plugin";
    const DESCRIPTION: &'static str = "Updates an installed plug-in to the newest version of the catalog in the background; \
         an update of a bundled plug-in takes its place until it is uninstalled.";

    fn run(&self, session: &mut Session, params: PluginIdParams) -> Result<Queued, CommandError> {
        agent_may_install(session, &self.0, Some(&params.id))?;
        self.0.update(&params.id).map_err(failed)
    }
}

pub struct UpdateAll(SharedPlugins);

impl Command for UpdateAll {
    type Params = NoParams;
    type Output = Queued;
    const ID: &'static str = "plugins.updates.all";
    const TITLE: &'static str = "Update All Plugins";
    const DESCRIPTION: &'static str = "Updates every installed plug-in the catalog has a newer version of (Update All), in the background; \
         fetches the catalog the first time, so while it loads, call again.";

    fn run(&self, session: &mut Session, _params: NoParams) -> Result<Queued, CommandError> {
        agent_may_install(session, &self.0, None)?;
        self.0.update_all().map_err(failed)
    }
}

pub struct CheckUpdates(SharedPlugins);

impl Command for CheckUpdates {
    type Params = NoParams;
    type Output = PluginList;
    const ID: &'static str = "plugins.updates.check";
    const TITLE: &'static str = "Check for Plugin Updates";
    const DESCRIPTION: &'static str = "Fetches the catalog again (also after it could not be loaded) and returns the installed plug-ins; \
         if it cannot be fetched, the one fetched before is kept and checkFailed says why. While catalog is loading, plugins.installed.list shows the updates once it is in.";

    fn run(&self, _session: &mut Session, _params: NoParams) -> Result<PluginList, CommandError> {
        Ok(self.0.refresh())
    }
}

pub struct EnablePlugin(SharedPlugins);

impl Command for EnablePlugin {
    type Params = PluginIdParams;
    type Output = Changed;
    const ID: &'static str = "plugins.plugin.enable";
    const TITLE: &'static str = "Enable Plugin";
    const DESCRIPTION: &'static str = "Switches an installed or bundled plug-in on; it is remembered and works at once. \
         An agent gets `userOnly` for a switched-off plug-in that brings database driver programs or terminal agent \
         adapters (`terminalAgents`): only the person switches those on.";

    fn run(&self, session: &mut Session, params: PluginIdParams) -> Result<Changed, CommandError> {
        if zen_core::macros::by_agent(session) {
            self.0.agent_may_enable(&params.id).map_err(|message| CommandError::with_code(USER_ONLY, message))?;
        }
        self.0.set_enabled(&params.id, true).map_err(failed)?;
        Ok(changed(&*self.0, params.id))
    }
}

pub struct DisablePlugin(SharedPlugins);

impl Command for DisablePlugin {
    type Params = PluginIdParams;
    type Output = Changed;
    const ID: &'static str = "plugins.plugin.disable";
    const TITLE: &'static str = "Disable Plugin";
    const DESCRIPTION: &'static str =
        "Switches an installed or bundled plug-in off; it is remembered and its file types lose its support at once.";

    fn run(&self, _session: &mut Session, params: PluginIdParams) -> Result<Changed, CommandError> {
        self.0.set_enabled(&params.id, false).map_err(failed)?;
        Ok(changed(&*self.0, params.id))
    }
}

pub struct UninstallPlugin(SharedPlugins);

impl Command for UninstallPlugin {
    type Params = PluginIdParams;
    type Output = Changed;
    const ID: &'static str = "plugins.plugin.uninstall";
    const TITLE: &'static str = "Uninstall Plugin";
    const DESCRIPTION: &'static str = "Removes a downloaded plug-in; for a bundled one only an update installed over it, which brings the bundled version back. \
         A bundled plug-in itself cannot be uninstalled, only disabled.";

    fn run(&self, _session: &mut Session, params: PluginIdParams) -> Result<Changed, CommandError> {
        self.0.uninstall(&params.id).map_err(failed)?;
        Ok(changed(&*self.0, params.id))
    }
}

fn changed(plugins: &dyn PluginManager, id: String) -> Changed {
    Changed { id, restart_required: plugins.installed("").restart_required }
}

pub fn register(registry: &mut Registry, plugins: &SharedPlugins) -> Result<(), RegistryError> {
    registry.register(ListCatalog(Arc::clone(plugins)))?;
    registry.register(ListInstalled(Arc::clone(plugins)))?;
    registry.register(PluginDetailsCommand(Arc::clone(plugins)))?;
    registry.register(InstallPlugin(Arc::clone(plugins)))?;
    registry.register(UpdatePlugin(Arc::clone(plugins)))?;
    registry.register(CheckUpdates(Arc::clone(plugins)))?;
    registry.register(UpdateAll(Arc::clone(plugins)))?;
    registry.register(EnablePlugin(Arc::clone(plugins)))?;
    registry.register(DisablePlugin(Arc::clone(plugins)))?;
    registry.register(UninstallPlugin(Arc::clone(plugins)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, name: &str, description: &str, extensions: &[&str]) -> PluginSummary {
        PluginSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            description: description.to_owned(),
            installed: None,
            latest: Some("0.1.0".to_owned()),
            update: None,
            bundled: false,
            enabled: false,
            installing: false,
            extensions: extensions.iter().map(|&extension| extension.to_owned()).collect(),
            names: Vec::new(),
            language_servers: false,
            runs: Vec::new(),
            person_only: Vec::new(),
            tool_windows: Vec::new(),
        }
    }

    #[test]
    fn search_matches_every_word_in_name_id_description_or_file_types() {
        let sql = summary("sql", "SQL", "Highlights SQL scripts.", &["sql"]);
        assert!(sql.matches(""));
        assert!(sql.matches("sql"));
        assert!(sql.matches("  Highlights   SCRIPTS "));
        assert!(!sql.matches("sql yaml"));
        let docker = PluginSummary { names: vec!["Dockerfile".to_owned()], ..summary("dockerfile", "Docker", "", &[]) };
        assert!(docker.matches("dockerfile"));
    }

    #[test]
    fn long_notes_are_cut_at_a_character_boundary() {
        assert_eq!(clip_notes("- Short."), "- Short.");
        let long = "й".repeat(NOTES_LIMIT);
        let clipped = clip_notes(&long);
        assert!(clipped.len() <= NOTES_LIMIT + "…".len(), "{}", clipped.len());
        assert!(clipped.ends_with('…') && clipped.starts_with("йй"));
        assert_eq!(clip_notes(&"a".repeat(NOTES_LIMIT)).len(), NOTES_LIMIT, "exactly the limit is kept whole");
    }

    #[test]
    fn plugins_sort_by_name_ignoring_case() {
        let mut plugins =
            vec![summary("yaml", "YAML", "", &[]), summary("css", "css", "", &[]), summary("bash", "Bash", "", &[])];
        sort(&mut plugins);
        let ids: Vec<_> = plugins.iter().map(|plugin| plugin.id.as_str()).collect();
        assert_eq!(ids, ["bash", "css", "yaml"]);
    }
}
