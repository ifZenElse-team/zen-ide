use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use crate::notifications::Notice;
use crate::settings::{UPDATES_AUTOMATIC, UPDATES_HELD, UPDATES_TOOLS, UpdateMode};
use crate::{Tool, ToolParams, ToolState, Tools, launchers, newer, not_in_demo, start_install};

pub fn updated(id: &str, name: &str, version: &str) -> Notice {
    Notice {
        id: id.to_owned(),
        version: version.to_owned(),
        title: t!("toolbox-updated-title", tool = name, version = version),
        body: t!("toolbox-updated-body"),
    }
}

pub fn failed(id: &str, name: &str, version: &str, error: &str) -> Notice {
    Notice {
        id: id.to_owned(),
        version: version.to_owned(),
        title: t!("toolbox-update-failed-title", tool = name, version = version),
        body: t!("toolbox-update-failed-body", error = error),
    }
}

fn listed(session: &Session) -> Vec<String> {
    session.settings().get(UPDATES_AUTOMATIC).unwrap_or_default()
}

fn held(session: &Session) -> BTreeMap<String, String> {
    session.settings().get(UPDATES_HELD).unwrap_or_default()
}

pub fn is_on(session: &Session, id: &str) -> bool {
    session.settings().get_or(UPDATES_TOOLS, UpdateMode::default()) == UpdateMode::Automatic
        || listed(session).iter().any(|listed| listed == id)
}

pub fn hold_after_going_back(session: &mut Session, tool: &Tool, version: &str) {
    let newest = match &tool.state {
        ToolState::Installed { update: Some(update), .. } => update.clone(),
        ToolState::Installed { version, .. } => version.clone(),
        _ => return,
    };
    if !newer(&newest, version) {
        return;
    }
    let mut kept = held(session);
    kept.insert(tool.id.clone(), newest);
    let _ = session.settings_mut().set(UPDATES_HELD, &kept);
}

fn wanted(session: &Session, tool: &Tool) -> bool {
    let ToolState::Installed { update: Some(update), .. } = &tool.state else { return false };
    is_on(session, &tool.id) && held(session).get(&tool.id).is_none_or(|left| newer(update, left))
}

pub fn follow(session: &mut Session) {
    follow_then(session, || {});
}

pub(crate) fn follow_then(session: &mut Session, done: impl FnOnce() + Send + 'static) {
    let Some(tools) = session.state::<Tools>().cloned() else { return done() };
    let turn = tools.take_automatic_turn();
    let snapshot = tools.snapshot();
    let waiting: Vec<Tool> = snapshot
        .tools
        .into_iter()
        .filter(|tool| wanted(session, tool) && !snapshot.jobs.contains_key(&tool.id))
        .collect();
    if !turn || waiting.is_empty() {
        return done();
    }
    crate::background(move || {
        if let Some(running) = tools.running_programs() {
            for tool in waiting.iter().filter(|tool| !tools.is_running(&tool.id, &running)) {
                tools.lock().automatic_updates.insert(tool.id.clone());
                if start_install(&tools, tool, None).is_err() {
                    tools.lock().automatic_updates.remove(&tool.id);
                }
            }
        }
        done();
    });
}

/// Parameters of `toolbox.tool.setAutoUpdate`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutoUpdateParams {
    /// The tool's id, e.g. `rustronin`.
    pub id: String,
    /// Whether the tool updates by itself.
    pub enabled: bool,
}

pub struct SetAutoUpdate;

impl Command for SetAutoUpdate {
    type Params = AutoUpdateParams;
    type Output = ();
    const ID: &'static str = "toolbox.tool.setAutoUpdate";
    const TITLE: &'static str = "Update Automatically";
    const DESCRIPTION: &'static str = "Turns automatic updates of one tool on or off. An automatic update runs in the \
        background after a check finds a newer version, never while the tool runs, and shows a system notification \
        afterwards. Off by default.";

    fn run(&self, session: &mut Session, params: AutoUpdateParams) -> Result<(), CommandError> {
        if params.enabled && zen_core::macros::by_agent(session) {
            return Err(CommandError::new(t!("toolbox-error-person-chooses")));
        }
        let tools = crate::tools(session)?;
        not_in_demo(&tools)?;
        let tool = crate::tool(&tools, &params.id)?;
        let mut ids = listed(session);
        ids.retain(|id| *id != tool.id);
        if params.enabled {
            ids.push(tool.id);
        }
        session.settings_mut().set(UPDATES_AUTOMATIC, &ids).map_err(|error| CommandError::new(error.to_string()))
    }
}

/// Where automatic updates of a tool stand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutoUpdateState {
    /// Whether the tool updates by itself.
    pub enabled: bool,
    /// The newest version the person left by going back: only a newer one is installed by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<String>,
    /// Whether the tool runs now, which makes an update wait; absent when the process list is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
}

pub struct AutoUpdateStatus;

impl Command for AutoUpdateStatus {
    type Params = ToolParams;
    type Output = AutoUpdateState;
    const ID: &'static str = "toolbox.tool.autoUpdateStatus";
    const TITLE: &'static str = "Automatic Update Status";
    const DESCRIPTION: &'static str = "Says whether a tool updates by itself, which version it is held at after \
        going back, and whether it runs now.";

    fn run(&self, session: &mut Session, params: ToolParams) -> Result<AutoUpdateState, CommandError> {
        let tools = crate::tools(session)?;
        let tool = crate::tool(&tools, &params.id)?;
        let running = tools.running_programs().map(|programs: Vec<PathBuf>| tools.is_running(&tool.id, &programs));
        Ok(AutoUpdateState { enabled: is_on(session, &tool.id), held: held(session).remove(&tool.id), running })
    }
}

pub(crate) fn runs(executable: Option<PathBuf>, running: &[PathBuf]) -> bool {
    let Some(executable) = executable else { return false };
    running.iter().any(|program| {
        *program == executable
            || (program.parent().is_none_or(|folder| folder.as_os_str().is_empty())
                && Some(program.as_os_str()) == executable.file_name())
    })
}

pub(crate) fn tool_executable(installations: &[zen_update::install::Installation], id: &str) -> Option<PathBuf> {
    installations.iter().find_map(|installation| launchers::executable(installation, id))
}
