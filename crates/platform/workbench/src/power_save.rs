use schemars::JsonSchema;
use serde::Serialize;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

use crate::Workbench;
use crate::language::Languages;

pub const ENABLED: &str = "ide.powerSaveMode";

pub fn is_on(session: &Session) -> bool {
    session.settings().get_or(ENABLED, false)
}

/// What Power Save Mode holds back while it is on, as `powerSave.status.get` reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PowerSaveStatus {
    /// Whether Power Save Mode is on.
    pub on: bool,
    /// What is paused: `diagnostics`, `completionPopup`, `networkChecks` and `reindexing`.
    pub paused: Vec<String>,
    /// The changed files the index reads once Power Save Mode is turned off.
    pub deferred_files: usize,
    /// The language servers told to pause their own background work (gopls checks only on save).
    pub servers_paused: Vec<String>,
}

pub fn status(session: &Session) -> PowerSaveStatus {
    let on = is_on(session);
    let paused = if on {
        ["diagnostics", "completionPopup", "networkChecks", "reindexing"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    } else {
        Vec::new()
    };
    let deferred_files = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.index.as_ref())
        .map_or(0, zen_index::FileIndex::deferred);
    let servers_paused = if on {
        session.state::<Languages>().map(Languages::servers_told_to_pause).unwrap_or_default()
    } else {
        Vec::new()
    };
    PowerSaveStatus { on, paused, deferred_files, servers_paused }
}

pub struct PowerSaveStatusGet;

impl Command for PowerSaveStatusGet {
    type Params = NoParams;
    type Output = PowerSaveStatus;
    const ID: &'static str = "powerSave.status.get";
    const TITLE: &'static str = "Power Save Mode Status";
    const DESCRIPTION: &'static str = "Whether Power Save Mode (File → Power Save Mode) is on and what it holds back: \
         `paused` lists diagnostics (checks as you type), completionPopup, networkChecks (go.mod's dependency checks) \
         and reindexing while it is on; `deferredFiles` is how many changed files the index reads when it is turned \
         off; `serversPaused` the language servers told to pause their own work (gopls checks only on save). \
         Turn it on or off with the window action view.powerSaveMode.toggle or set_settings {powerSaveMode}.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<PowerSaveStatus, CommandError> {
        Ok(status(session))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(PowerSaveStatusGet)?;
    Ok(())
}

#[cfg(test)]
mod tests;
