use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::language::with_languages;
use crate::notifications::{Action, Group, Kind, NewNotification, Notifications, now_ms};
use crate::{Workbench, disk, exit, run, tasks};

const SCHEDULE: &str = "invalidate-caches.json";
const LOCAL_HISTORY: &str = "local-history";
const LANGUAGE_SERVER_CACHES: [&str; 2] = ["caches/language-servers", "caches/libraries"];
const VCS_LOG_CACHES: [&str; 1] = ["caches/avatars"];

pub const INVALIDATE_DIALOG: &str = "view.invalidateCaches.open";

#[derive(Debug, Default)]
pub struct Maintenance {
    data: Option<PathBuf>,
    repair_notification: Option<u64>,
}

impl Maintenance {
    pub fn set_data_dir(&mut self, data: PathBuf) {
        self.data = Some(data);
    }
}

fn clears_vcs_log_by_default() -> bool {
    true
}

/// What to clear besides the index and the language servers' caches, which always go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InvalidateParams {
    /// Clear file system cache and Local History: the saved versions of every file are lost.
    #[serde(default)]
    pub clear_local_history: bool,
    /// Clear VCS Log caches and indexes, on when absent.
    #[serde(default = "clears_vcs_log_by_default")]
    pub clear_vcs_log: bool,
}

impl Default for InvalidateParams {
    fn default() -> Self {
        Self { clear_local_history: false, clear_vcs_log: clears_vcs_log_by_default() }
    }
}

impl InvalidateParams {
    fn folders(self, data: &Path) -> Vec<PathBuf> {
        let history = self.clear_local_history.then_some(LOCAL_HISTORY).into_iter();
        let vcs = VCS_LOG_CACHES.into_iter().filter(|_| self.clear_vcs_log);
        LANGUAGE_SERVER_CACHES.into_iter().chain(vcs).chain(history).map(|folder| data.join(folder)).collect()
    }
}

/// What the next start of the IDE removes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Invalidation {
    /// The folders it removes.
    pub folders: Vec<PathBuf>,
    /// Always true: nothing is removed until the IDE restarts (`view.ide.restart`).
    pub restart_required: bool,
}

pub struct InvalidateCaches;

impl Command for InvalidateCaches {
    type Params = InvalidateParams;
    type Output = Invalidation;
    const ID: &'static str = "ide.caches.invalidate";
    const TITLE: &'static str = "Invalidate Caches";
    const DESCRIPTION: &'static str = "Marks the IDE's derived data of all projects for removal at the next start: the \
         language servers' caches and libraries, with `clearVcsLog` (on by default) the Git log's caches, with \
         `clearLocalHistory` (off by default; only a person's choice, refused for an agent with code agentNotAllowed) \
         Local History. The indexes live in memory and are rebuilt by any start. \
         Nothing is removed until the IDE restarts (view.ide.restart; a headless IDE never restarts by itself). \
         Answers the folders it will remove.";

    fn run(&self, session: &mut Session, params: InvalidateParams) -> Result<Invalidation, CommandError> {
        if params.clear_local_history && zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(
                crate::trust::AGENT_NOT_ALLOWED,
                t!("maintenance-agent-local-history"),
            ));
        }
        let data = session.state::<Maintenance>().and_then(|state| state.data.clone());
        let data = data.ok_or_else(|| CommandError::new(t!("maintenance-no-data-folder")))?;
        let text = serde_json::to_string(&params).map_err(|error| CommandError::new(error.to_string()))?;
        std::fs::write(data.join(SCHEDULE), text).map_err(|error| CommandError::new(error.to_string()))?;
        Ok(Invalidation { folders: params.folders(&data), restart_required: true })
    }
}

pub fn apply_scheduled_invalidation(data: &Path) -> Vec<PathBuf> {
    let schedule = data.join(SCHEDULE);
    let Ok(text) = std::fs::read_to_string(&schedule) else { return Vec::new() };
    let _ = std::fs::remove_file(&schedule);
    let Ok(params) = serde_json::from_str::<InvalidateParams>(&text) else { return Vec::new() };
    let folders = params.folders(data);
    folders.into_iter().filter(|folder| std::fs::remove_dir_all(folder).is_ok()).collect()
}

/// A step of Repair IDE, in the order the steps are offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RepairStep {
    /// Refresh file system: compares the open files and the project tree with the disk.
    #[default]
    RefreshFileSystem,
    /// Rescan Project Indexes: reads the project's files into the indexes again.
    RescanIndexes,
    /// Reopen Project and Re-sync: restarts the language servers and rereads the run configurations and Git state.
    Resync,
    /// Drop Indexes and Reindex Current Project: builds the indexes again from nothing.
    DropIndexes,
}

impl RepairStep {
    fn next(self) -> Option<Self> {
        match self {
            Self::RefreshFileSystem => Some(Self::RescanIndexes),
            Self::RescanIndexes => Some(Self::Resync),
            Self::Resync => Some(Self::DropIndexes),
            Self::DropIndexes => None,
        }
    }

    fn name(self) -> String {
        match self {
            Self::RefreshFileSystem => t!("maintenance-step-refresh"),
            Self::RescanIndexes => t!("maintenance-step-rescan"),
            Self::Resync => t!("maintenance-step-resync"),
            Self::DropIndexes => t!("maintenance-step-drop"),
        }
    }
}

/// Which step of Repair IDE to run.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RepairParams {
    /// The step; the first when absent.
    pub step: RepairStep,
}

/// What a step of Repair IDE did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RepairReport {
    /// The step that ran.
    pub step: RepairStep,
    /// What it did, in words.
    pub did: String,
    /// The step offered next; after the last one, only `view.invalidateCaches.open` is left.
    pub next: Option<RepairStep>,
}

fn require_project(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

fn rescan_in_background(session: &mut Session) {
    let index = session.state::<Workbench>().and_then(Workbench::file_index).cloned();
    let Some(index) = index else { return };
    let task = session.state_mut::<tasks::Tasks>().start(&t!("maintenance-step-rescan"), exit::INDEX, false);
    let rescan = move || {
        index.rescan();
        task.finish(Ok(t!("workbench-index-done", files = index.len())));
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(rescan);
    #[cfg(target_arch = "wasm32")]
    rescan();
}

fn refresh_file_system(session: &mut Session) -> Result<String, CommandError> {
    let synced = disk::SyncWithDisk.run(session, disk::SyncParams::default())?;
    session.state_mut::<Workbench>().revision += 1;
    Ok(t!("maintenance-did-refresh", count = synced.len()))
}

fn resync(session: &mut Session, root: &Path) -> Result<String, CommandError> {
    let servers = with_languages(session, |languages, _| languages.restart(None)).len();
    let configurations = run::read_file(session, root).map_or(0, |file| file.configurations.len());
    crate::vcs::StatusRefresh.run(session, crate::vcs::RefreshParams::default())?;
    Ok(t!("maintenance-did-resync", servers = servers, configurations = configurations))
}

fn run_step(session: &mut Session, step: RepairStep) -> Result<String, CommandError> {
    let root = require_project(session)?;
    match step {
        RepairStep::RefreshFileSystem => refresh_file_system(session),
        RepairStep::RescanIndexes => {
            rescan_in_background(session);
            Ok(t!("maintenance-did-rescan"))
        }
        RepairStep::Resync => resync(session, &root),
        RepairStep::DropIndexes => {
            session.state_mut::<Workbench>().index = None;
            crate::index_project(session);
            Ok(t!("maintenance-did-drop"))
        }
    }
}

fn close_notification(session: &mut Session) {
    let shown = session.state_mut::<Maintenance>().repair_notification.take();
    if let Some(id) = shown {
        session.state_mut::<Notifications>().remove(Some(&[id]));
    }
}

fn notify(session: &mut Session, report: &RepairReport) {
    close_notification(session);
    let body = match report.next {
        Some(next) => t!("maintenance-next", did = report.did.as_str(), step = next.name()),
        None => t!("maintenance-last", did = report.did.as_str()),
    };
    let mut notification = NewNotification::message(Group::General, Kind::Info, body)
        .titled_by(zen_i18n::Message::new("maintenance-repair-title"))
        .sticky()
        .action(Action::new("works", t!("maintenance-works"), "ide.repair.finish", json!({})));
    if let Some(next) = report.next {
        let label = t!("maintenance-continue");
        notification = notification.action(Action::new("continue", label, "ide.repair.run", json!({ "step": next })));
    }
    notification = notification.action(Action::new("more", t!("maintenance-more"), INVALIDATE_DIALOG, json!({})));
    let id = session.state_mut::<Notifications>().add(notification, now_ms());
    session.state_mut::<Maintenance>().repair_notification = Some(id);
}

pub struct RunRepair;

impl Command for RunRepair {
    type Params = RepairParams;
    type Output = RepairReport;
    const ID: &'static str = "ide.repair.run";
    const TITLE: &'static str = "Repair IDE";
    const DESCRIPTION: &'static str = "Runs one step of Repair IDE, the first when `step` is absent: refreshFileSystem, \
         rescanIndexes, resync (restarts the language servers, rereads run configurations and Git state), dropIndexes. \
         Answers what it did and the next step, and replaces the notification that offers Everything Works Now \
         (ide.repair.finish), Continue and More (view.invalidateCaches.open).";

    fn run(&self, session: &mut Session, params: RepairParams) -> Result<RepairReport, CommandError> {
        let did = run_step(session, params.step)?;
        let report = RepairReport { step: params.step, did, next: params.step.next() };
        notify(session, &report);
        Ok(report)
    }
}

pub struct FinishRepair;

impl Command for FinishRepair {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "ide.repair.finish";
    const TITLE: &'static str = "Everything Works Now";
    const DESCRIPTION: &'static str = "Ends Repair IDE and closes its notification.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        close_notification(session);
        Ok(())
    }
}

/// The file to repair.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RepairFileParams {
    /// The file: absolute, or relative to the project.
    pub path: PathBuf,
}

/// What Repair IDE on File did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RepairFileReport {
    /// The file.
    pub path: PathBuf,
    /// What it did, in words.
    pub did: String,
}

pub struct RepairFile;

impl Command for RepairFile {
    type Params = RepairFileParams;
    type Output = RepairFileReport;
    const ID: &'static str = "ide.repair.file";
    const TITLE: &'static str = "Repair IDE on File";
    const DESCRIPTION: &'static str = "Runs the first two steps of Repair IDE for one file: compares it with the disk \
         when it is open and reads it into the indexes again.";

    fn run(&self, session: &mut Session, params: RepairFileParams) -> Result<RepairFileReport, CommandError> {
        require_project(session)?;
        let path = session.state_mut::<Workbench>().resolve(&params.path);
        let synced = disk::SyncWithDisk.run(session, disk::SyncParams { paths: vec![path.clone()] })?;
        if let Some(index) = session.state::<Workbench>().and_then(Workbench::file_index) {
            index.refresh(std::slice::from_ref(&path));
        }
        let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
        let did = if synced.is_empty() {
            t!("maintenance-did-file", file = name)
        } else {
            t!("maintenance-did-file-reloaded", file = name)
        };
        let notification = NewNotification::message(Group::General, Kind::Info, did.clone());
        session.state_mut::<Notifications>().add(notification, now_ms());
        Ok(RepairFileReport { path, did })
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(InvalidateCaches)?;
    registry.register(RunRepair)?;
    registry.register(FinishRepair)?;
    registry.register(RepairFile)?;
    Ok(())
}

#[cfg(test)]
mod tests;
