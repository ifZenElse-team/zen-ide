use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::{Workbench, groups, history, language, local_history};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CloseOthers)?;
    registry.register(CloseAll)?;
    registry.register(CloseUnmodified)?;
    registry.register(CloseAllButPinned)?;
    registry.register(CloseLeft)?;
    registry.register(CloseRight)?;
    registry.register(CloseReadOnly)?;
    Ok(())
}

/// What closing editor tabs did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ClosedTabs {
    /// The files whose tabs closed, in tab order.
    pub closed: Vec<PathBuf>,
    /// The files whose unsaved changes were saved before their tabs closed.
    pub saved: Vec<PathBuf>,
    /// The id of the workspace's inner tab that closed, when the tab in front was one the workspace's feature draws.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
}

/// The editor group whose tabs to close.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct GroupParams {
    /// The editor group (`editor.split.list`); the focused one when absent.
    pub group: Option<groups::GroupId>,
}

pub struct CloseOthers;

impl Command for CloseOthers {
    type Params = crate::CloseParams;
    type Output = ClosedTabs;
    const ID: &'static str = "file.document.closeOthers";
    const TITLE: &'static str = "Close Other Tabs";
    const DESCRIPTION: &'static str = "Closes every tab of an editor group but one (the group's active tab when no index is \
         given) and the pinned ones. Unsaved changes are saved first, as Save All saves them; a file that cannot be saved keeps its tab and \
         its changes, and the error says why. Returns the files closed and the files saved.";

    fn run(&self, session: &mut Session, params: crate::CloseParams) -> Result<ClosedTabs, CommandError> {
        let (group, picked) = picked(session, &params, |at, kept| at != kept)?;
        close_tabs(session, group, |path, _| picked.iter().any(|tab| tab == path))
    }
}

fn picked(
    session: &Session,
    params: &crate::CloseParams,
    pick: impl Fn(usize, usize) -> bool,
) -> Result<(groups::GroupId, Vec<PathBuf>), CommandError> {
    let workbench = workbench(session)?;
    let group = params.group.unwrap_or(workbench.groups.focused());
    let tabs = workbench.groups.group(group).ok_or_else(no_document)?;
    let kept = params.index.or(tabs.active).filter(|index| *index < tabs.tabs.len()).ok_or_else(no_document)?;
    let picked = tabs
        .tabs
        .iter()
        .enumerate()
        .filter(|(at, path)| pick(*at, kept) && !tabs.is_pinned(path))
        .map(|(_, path)| path.clone())
        .collect();
    Ok((group, picked))
}

fn has_tabs(session: &Session) -> bool {
    session.state::<Workbench>().is_some_and(|workbench| !workbench.groups.focused_group().tabs.is_empty())
}

pub struct CloseAllButPinned;

impl Command for CloseAllButPinned {
    type Params = GroupParams;
    type Output = ClosedTabs;
    const ID: &'static str = "file.document.closeAllButPinned";
    const TITLE: &'static str = "Close All but Pinned";
    const DESCRIPTION: &'static str = "Closes every tab of an editor group that is not pinned (`editor.tab.pin`). Unsaved \
         changes are saved first, as Save All saves them; a file that cannot be saved keeps its tab and its changes, \
         and the error says why. Returns the files closed and the files saved.";

    fn is_available(&self, session: &Session) -> bool {
        has_tabs(session)
    }

    fn run(&self, session: &mut Session, params: GroupParams) -> Result<ClosedTabs, CommandError> {
        let workbench = workbench(session)?;
        let group = params.group.unwrap_or(workbench.groups.focused());
        let pinned = workbench.groups.group(group).map(|tabs| tabs.pinned.clone()).unwrap_or_default();
        close_tabs(session, group, |path, _| !pinned.iter().any(|tab| tab == path))
    }
}

pub struct CloseLeft;

impl Command for CloseLeft {
    type Params = crate::CloseParams;
    type Output = ClosedTabs;
    const ID: &'static str = "file.document.closeLeft";
    const TITLE: &'static str = "Close Tabs to the Left";
    const DESCRIPTION: &'static str = "Closes the tabs left of a tab (the group's active one when no index is given), \
         except the pinned ones. Unsaved changes are saved first, as Save All saves them; a file that cannot be saved \
         keeps its tab and its changes, and the error says why. Returns the files closed and the files saved.";

    fn is_available(&self, session: &Session) -> bool {
        has_tabs(session)
    }

    fn run(&self, session: &mut Session, params: crate::CloseParams) -> Result<ClosedTabs, CommandError> {
        let (group, picked) = picked(session, &params, |at, kept| at < kept)?;
        close_tabs(session, group, |path, _| picked.iter().any(|tab| tab == path))
    }
}

pub struct CloseRight;

impl Command for CloseRight {
    type Params = crate::CloseParams;
    type Output = ClosedTabs;
    const ID: &'static str = "file.document.closeRight";
    const TITLE: &'static str = "Close Tabs to the Right";
    const DESCRIPTION: &'static str = "Closes the tabs right of a tab (the group's active one when no index is given), \
         except the pinned ones. Unsaved changes are saved first, as Save All saves them; a file that cannot be saved \
         keeps its tab and its changes, and the error says why. Returns the files closed and the files saved.";

    fn is_available(&self, session: &Session) -> bool {
        has_tabs(session)
    }

    fn run(&self, session: &mut Session, params: crate::CloseParams) -> Result<ClosedTabs, CommandError> {
        let (group, picked) = picked(session, &params, |at, kept| at > kept)?;
        close_tabs(session, group, |path, _| picked.iter().any(|tab| tab == path))
    }
}

pub struct CloseReadOnly;

impl Command for CloseReadOnly {
    type Params = GroupParams;
    type Output = ClosedTabs;
    const ID: &'static str = "file.document.closeReadOnly";
    const TITLE: &'static str = "Close All Read-Only Tabs";
    const DESCRIPTION: &'static str = "Closes the tabs of an editor group whose files are read-only: locked in the IDE, \
         not writable on disk, or files that cannot be edited (library sources). Returns the files closed.";

    fn is_available(&self, session: &Session) -> bool {
        has_tabs(session)
    }

    fn run(&self, session: &mut Session, params: GroupParams) -> Result<ClosedTabs, CommandError> {
        let group = params.group.unwrap_or(workbench(session)?.groups.focused());
        close_tabs(session, group, |path, workbench| is_read_only(workbench, path))
    }
}

pub fn is_read_only(workbench: &Workbench, path: &Path) -> bool {
    workbench.documents.iter().any(|document| document.path == path && document.read_only().is_some())
        || (path.is_file() && !zen_vfs::is_writable(path))
}

pub struct CloseAll;

impl Command for CloseAll {
    type Params = GroupParams;
    type Output = ClosedTabs;
    const ID: &'static str = "file.document.closeAll";
    const TITLE: &'static str = "Close All Tabs";
    const DESCRIPTION: &'static str = "Closes every tab of an editor group. Unsaved changes are saved first, as Save All \
         saves them; a file that cannot be saved keeps its tab and its changes, and the error says why. Returns the \
         files closed and the files saved.";

    fn run(&self, session: &mut Session, params: GroupParams) -> Result<ClosedTabs, CommandError> {
        let group = params.group.unwrap_or(workbench(session)?.groups.focused());
        close_tabs(session, group, |_, _| true)
    }
}

pub struct CloseUnmodified;

impl Command for CloseUnmodified {
    type Params = GroupParams;
    type Output = ClosedTabs;
    const ID: &'static str = "file.document.closeUnmodified";
    const TITLE: &'static str = "Close Unmodified Tabs";
    const DESCRIPTION: &'static str = "Closes the tabs of an editor group whose files have no unsaved changes; the tabs \
         with unsaved changes stay open. Returns the files closed.";

    fn run(&self, session: &mut Session, params: GroupParams) -> Result<ClosedTabs, CommandError> {
        let group = params.group.unwrap_or(workbench(session)?.groups.focused());
        close_tabs(session, group, |path, workbench| !is_dirty(workbench, path))
    }
}

pub(crate) fn close_tab(
    session: &mut Session,
    group: Option<groups::GroupId>,
    index: Option<usize>,
) -> Result<ClosedTabs, CommandError> {
    crate::tabs::follow_policy(session);
    let (group, path) = {
        let workbench = workbench(session)?;
        let group = group.unwrap_or(workbench.groups.focused());
        let tabs = workbench.groups.group(group).ok_or_else(no_document)?;
        let path = index.or(tabs.active).and_then(|index| tabs.tabs.get(index)).cloned().ok_or_else(no_document)?;
        (group, path)
    };
    let (saved, failed) = save_documents(session, std::slice::from_ref(&path));
    if let Some(error) = failure(failed) {
        return Err(error);
    }
    let origin = history::caret(session);
    history::closing(session);
    let workbench = session.state_mut::<Workbench>();
    let index = index.or_else(|| workbench.groups.group(group).and_then(|group| group.active));
    if let Some(index) = index {
        workbench.note_closing(group, index);
    }
    let closed = index.and_then(|index| workbench.groups.close(group, index)).ok_or_else(no_document)?;
    let mut gone = None;
    if !workbench.groups.contains(&closed) {
        gone = workbench
            .documents
            .iter()
            .find(|document| document.path == closed)
            .map(|document| document.text().to_owned());
        workbench.forget_document(&closed);
    }
    workbench.sync_groups();
    if let Some(text) = gone {
        crate::scratch::closed(session, &closed, &text);
        crate::workspaces::closed(session, &closed, &text);
    }
    history::left_file(session, origin.filter(|origin| origin.path == closed));
    Ok(ClosedTabs { closed: vec![closed], saved, tab: None })
}

fn close_tabs(
    session: &mut Session,
    group: groups::GroupId,
    close: impl Fn(&Path, &Workbench) -> bool,
) -> Result<ClosedTabs, CommandError> {
    crate::tabs::follow_policy(session);
    let (picked, active) = {
        let workbench = workbench(session)?;
        let tabs = workbench.groups.group(group).ok_or_else(no_document)?;
        let picked: Vec<PathBuf> = tabs.tabs.iter().filter(|path| close(path, workbench)).cloned().collect();
        (picked, tabs.active_path().map(Path::to_owned))
    };
    let (saved, failed) = save_documents(session, &picked);
    let origin = history::caret(session);
    history::closing(session);
    let workbench = session.state_mut::<Workbench>();
    let closing: Vec<PathBuf> =
        picked.into_iter().filter(|path| !failed.iter().any(|(stays, _)| stays == path)).collect();
    let mut closed = Vec::new();
    while let Some(index) =
        workbench.groups.group(group).and_then(|tabs| tabs.tabs.iter().rposition(|path| closing.contains(path)))
    {
        workbench.note_closing(group, index);
        if let Some(path) = workbench.groups.close(group, index) {
            closed.insert(0, path);
        }
    }
    if let Some(index) =
        active.as_ref().and_then(|active| workbench.groups.group(group)?.tabs.iter().position(|path| path == active))
    {
        workbench.groups.select(group, index);
    }
    for path in &closed {
        if !workbench.groups.contains(path) {
            workbench.forget_document(path);
        }
    }
    workbench.sync_groups();
    history::left_file(session, origin.filter(|origin| closed.contains(&origin.path)));
    match failure(failed) {
        Some(error) => Err(error),
        None => Ok(ClosedTabs { closed, saved, tab: None }),
    }
}

pub(crate) fn save_documents(session: &mut Session, paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let (saved, failed, notes) = save_documents_noting(session, paths);
    if !notes.is_empty() {
        session.state_mut::<Workbench>().messages.extend(notes.into_iter().map(|(_, note)| note));
    }
    (saved, failed)
}

pub(crate) fn save_documents_noting(session: &mut Session, paths: &[PathBuf]) -> Saving {
    let dirty: Vec<PathBuf> = session
        .state::<Workbench>()
        .map(|workbench| paths.iter().filter(|path| is_dirty(workbench, path)).cloned().collect())
        .unwrap_or_default();
    if dirty.is_empty() {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let (dirty, mut failed) = refuse_changed_on_disk(session.state_mut::<Workbench>(), dirty);
    let mut notes = Vec::new();
    for path in &dirty {
        let (answered, note) = language::format::before_save_noting(session, path);
        notes.extend(note.map(|note| (path.clone(), note)));
        if !answered {
            break;
        }
    }
    for path in &dirty {
        crate::on_save::before_save(session, path);
        crate::editor_config::before_write(session, path);
    }
    let workbench = session.state_mut::<Workbench>();
    let mut saved = Vec::new();
    for document in workbench.documents.iter_mut().filter(|document| document.dirty && dirty.contains(&document.path)) {
        match document.save() {
            Ok(()) => saved.push(document.path.clone()),
            Err(error) => failed.push((document.path.clone(), save_failed(&document.name(), &error))),
        }
    }
    if !failed.is_empty() {
        workbench.revision += 1;
    }
    local_history::record_saves(session);
    (saved, failed, notes)
}

fn refuse_changed_on_disk(workbench: &mut Workbench, paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let mut failed = Vec::new();
    let unchanged = paths
        .into_iter()
        .filter(|path| {
            let Some(document) = workbench.documents.iter_mut().find(|document| document.path == *path) else {
                return true;
            };
            let Err(error) = document.check_disk() else { return true };
            failed.push((path.clone(), save_failed(&document.name(), &error)));
            false
        })
        .collect();
    (unchanged, failed)
}

pub(crate) fn read_failed(path: &Path, error: &std::io::Error) -> String {
    t!("workbench-error-open", path = path.display().to_string(), error = error.to_string())
}

pub(crate) fn save_failed(name: &str, error: &zen_vfs::VfsError) -> String {
    t!("workbench-error-save", file = name, error = error.source.to_string())
}

pub(crate) type Saving = (Vec<PathBuf>, Vec<(PathBuf, String)>, Vec<(PathBuf, String)>);

pub(crate) fn failure(failed: Vec<(PathBuf, String)>) -> Option<CommandError> {
    (!failed.is_empty())
        .then(|| CommandError::new(failed.into_iter().map(|(_, error)| error).collect::<Vec<_>>().join("\n")))
}

pub(crate) fn is_dirty(workbench: &Workbench, path: &Path) -> bool {
    workbench.documents.iter().any(|document| document.path == path && document.dirty)
}

fn workbench(session: &Session) -> Result<&Workbench, CommandError> {
    session.state::<Workbench>().ok_or_else(no_document)
}

fn no_document() -> CommandError {
    CommandError::new(t!("workbench-error-no-document"))
}

#[cfg(test)]
mod tests;
