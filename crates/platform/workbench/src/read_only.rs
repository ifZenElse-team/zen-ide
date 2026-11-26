use std::collections::HashMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;
use crate::notifications::{Action, Group, Kind, NewNotification, Notifications};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ToggleReadOnly)?;
    registry.register(ReadOnlyState)?;
    Ok(())
}

pub const MAKE_WRITABLE: &str = "makeWritable";

pub(crate) fn lock_reason(path: &Path) -> Option<String> {
    if crate::virtual_documents::get(path).is_some() {
        return None;
    }
    (path.is_file() && !zen_vfs::is_writable(path)).then(|| t!("workbench-read-only-locked", file = file_name(path)))
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

/// Which file, and what to make it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ToggleParams {
    /// The file; the active document's when absent. A relative path is the project's.
    pub path: Option<PathBuf>,
    /// `true` makes the file read-only, `false` writable; absent turns it the other way.
    pub read_only: Option<bool>,
}

/// Which file.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FileParams {
    /// The file; the active document's when absent. A relative path is the project's.
    pub path: Option<PathBuf>,
}

/// Whether a file can be written and edited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadOnlyInfo {
    /// The file.
    pub path: PathBuf,
    /// Whether the file's write permission is off on disk (the status bar's lock is closed).
    pub read_only: bool,
    /// Why its open document cannot be edited, when it cannot (the file is read-only, its encoding
    /// is not UTF-8...); absent when it can or the file is not open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn target(session: &Session, path: Option<&Path>) -> Result<PathBuf, CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    match path {
        Some(path) => Ok(workbench.resolve(path)),
        None => workbench
            .active_document()
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document"))),
    }
}

fn info(session: &Session, path: &Path) -> Result<ReadOnlyInfo, CommandError> {
    if !path.is_file() {
        return Err(CommandError::new(t!("workbench-read-only-no-file", path = path.display().to_string())));
    }
    let reason = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents.iter().find(|document| document.path() == path))
        .and_then(|document| document.read_only().map(str::to_owned));
    Ok(ReadOnlyInfo { path: path.to_owned(), read_only: !zen_vfs::is_writable(path), reason })
}

fn has_active_document(session: &Session) -> bool {
    session.state::<Workbench>().is_some_and(|workbench| workbench.active_document().is_some())
}

pub struct ToggleReadOnly;

impl Command for ToggleReadOnly {
    type Params = ToggleParams;
    type Output = ReadOnlyInfo;
    const ID: &'static str = "file.document.toggleReadOnly";
    const TITLE: &'static str = "Toggle Read-Only Attribute";
    const DESCRIPTION: &'static str = "Makes a file read-only on disk (its write permission off) or writable again, as \
         the status bar's lock and File → File Properties → Make file read-only / Make file writable do; `readOnly` \
         says which, absent turns it the other way. Unsaved changes are saved before the file is made read-only. \
         An open document of a read-only file cannot be edited. Returns the file's state.";

    fn is_available(&self, session: &Session) -> bool {
        has_active_document(session)
    }

    fn run(&self, session: &mut Session, params: ToggleParams) -> Result<ReadOnlyInfo, CommandError> {
        let path = target(session, params.path.as_deref())?;
        let current = info(session, &path)?;
        let read_only = params.read_only.unwrap_or(!current.read_only);
        let failed =
            |error: String| CommandError::new(t!("workbench-read-only-failed", file = file_name(&path), error = error));
        if read_only && !current.read_only {
            crate::text_replace::save(session, &path, &file_name(&path)).map_err(|error| failed(error.to_string()))?;
            let access = zen_vfs::make_read_only(&path).map_err(|error| failed(error.to_string()))?;
            session.state_mut::<Remembered>().0.insert(path.clone(), access);
        } else if !read_only && current.read_only {
            let access = session.state_mut::<Remembered>().0.remove(&path);
            zen_vfs::make_writable(&path, access).map_err(|error| failed(error.to_string()))?;
        }
        if let Some(document) =
            session.state_mut::<Workbench>().documents.iter_mut().find(|document| document.path() == path)
        {
            document.locked = lock_reason(&path);
            document.refused = false;
        }
        if !read_only {
            settle_offers(session, &path);
        }
        info(session, &path)
    }
}

#[derive(Default)]
struct Remembered(HashMap<PathBuf, zen_vfs::WriteAccess>);

pub struct ReadOnlyState;

impl Command for ReadOnlyState {
    type Params = FileParams;
    type Output = ReadOnlyInfo;
    const ID: &'static str = "file.document.readOnlyState";
    const TITLE: &'static str = "Read-Only State";
    const DESCRIPTION: &'static str = "Whether a file (the active document's when `path` is absent) is read-only on \
         disk, as the status bar's lock shows, and why its open document cannot be edited, when it cannot.";

    fn run(&self, session: &mut Session, params: FileParams) -> Result<ReadOnlyInfo, CommandError> {
        let path = target(session, params.path.as_deref())?;
        info(session, &path)
    }
}

fn offers_of<'a>(store: &'a Notifications, path: &'a Path) -> impl Iterator<Item = u64> + 'a {
    store
        .newest_first()
        .filter(move |item| {
            !item.read
                && item.actions.iter().any(|action| {
                    action.id == MAKE_WRITABLE
                        && action.command == ToggleReadOnly::ID
                        && action.params["path"].as_str() == path.to_str()
                })
        })
        .map(|item| item.id)
}

pub(crate) fn settle_offers(session: &mut Session, path: &Path) {
    if session.state::<Notifications>().is_none() {
        return;
    }
    let store = session.state_mut::<Notifications>();
    let ids: Vec<u64> = offers_of(store, path).collect();
    if !ids.is_empty() {
        store.mark_read(Some(&ids));
    }
}

pub fn offer_writable(session: &mut Session) -> Vec<PathBuf> {
    let Some(workbench) = session.state::<Workbench>() else { return Vec::new() };
    if !workbench.documents.iter().any(|document| document.refused) {
        return Vec::new();
    }
    let workbench = session.state_mut::<Workbench>();
    let refused: Vec<PathBuf> = workbench
        .documents
        .iter_mut()
        .filter_map(|document| {
            (std::mem::take(&mut document.refused) && document.locked.is_some()).then(|| document.path().to_owned())
        })
        .collect();
    let mut offered = Vec::new();
    for path in refused {
        if session.state::<Notifications>().is_some_and(|store| offers_of(store, &path).next().is_some()) {
            continue;
        }
        let action = Action::new(
            MAKE_WRITABLE,
            t!("workbench-read-only-make-writable"),
            ToggleReadOnly::ID,
            json!({ "path": path, "readOnly": false }),
        );
        let message = t!("workbench-read-only-offer", file = file_name(&path));
        crate::notifications::post(
            session,
            NewNotification::message(Group::General, Kind::Warning, message).action(action),
        );
        offered.push(path);
    }
    offered
}

#[cfg(test)]
mod tests;
