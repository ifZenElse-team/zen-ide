use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::commit::{DiffView, current_text, diff_view, relative};
use super::log::CommitFile;
use super::workspace::{GitWorkspace, WorkspaceState, find, state};
use super::{absolute, message, repository};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(DiffCommits)?;
    registry.register(FileHistory)?;
    registry.register(FileMenu)?;
    registry.register(FileActionCommand)?;
    Ok(())
}

/// What the Git workspace's right panel shows instead of the selected commit or the work in
/// progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RightPanel {
    /// The files that differ between two commits (`from` the older), or between a commit and the
    /// work in progress (`to` absent).
    Compare {
        /// The commit compared from (its full id).
        from: String,
        /// The commit compared to (its full id); the working tree when absent.
        #[serde(default)]
        to: Option<String>,
    },
    /// The commits that changed a file.
    FileHistory { path: PathBuf },
}

/// A file of the right panel, whose menu is open.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileTarget {
    /// The file.
    pub path: PathBuf,
    /// The commit whose files list it; none for the work in progress or a comparison.
    pub id: Option<String>,
}

fn failed(error: zen_vcs::VcsError) -> CommandError {
    CommandError::new(message(&error))
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

pub(super) fn follow_picks(session: &mut Session) {
    let Some(workspace) = session.state::<GitWorkspace>() else { return };
    let (picked, wip) = (workspace.picked().to_vec(), workspace.picked_wip());
    let compare = match (picked.as_slice(), wip) {
        ([one], true) => Some((one.clone(), None)),
        ([first, second], false) => Some(older_first(session, first, second)),
        _ => None,
    };
    let workspace = session.state_mut::<GitWorkspace>();
    match compare {
        Some((from, to)) => workspace.panel = Some(RightPanel::Compare { from, to }),
        None if matches!(workspace.panel, Some(RightPanel::Compare { .. })) => workspace.panel = None,
        None => {}
    }
}

fn older_first(session: &mut Session, first: &str, second: &str) -> (String, Option<String>) {
    let row = |session: &mut Session, id: &str| find(session, id).ok().and_then(|found| found.index);
    let (one, two) = (row(session, first), row(session, second));
    let first_older = match (one, two) {
        (Some(one), Some(two)) => one > two,
        _ => repository(session).is_ok_and(|repository| repository.is_ancestor(first, second).unwrap_or(false)),
    };
    if first_older { (first.to_owned(), Some(second.to_owned())) } else { (second.to_owned(), Some(first.to_owned())) }
}

pub(super) fn compare_picked(session: &mut Session, id: &str) -> Result<(), CommandError> {
    let workspace = session.state::<GitWorkspace>();
    let picked = workspace.map(|w| w.picked().to_vec()).unwrap_or_default();
    let wip = workspace.is_some_and(GitWorkspace::picked_wip);
    if picked.len() + usize::from(wip) != 2 || !picked.iter().any(|picked| picked == id) {
        return Err(CommandError::new(t!("vcs-error-compare-two")));
    }
    follow_picks(session);
    Ok(())
}

/// What to compare.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffCommitsParams {
    /// The commit to compare from: an id (or its start), a branch, a tag, `HEAD~2`.
    pub from: String,
    /// The commit to compare to; the working tree as it is now (the work in progress) when
    /// absent.
    pub to: Option<String>,
    /// A file (absolute or relative to the repository) whose diff to answer with, in `diff`.
    pub path: Option<PathBuf>,
    /// Show the comparison in the Git workspace's right panel (opening the workspace), as two
    /// commits picked with ⌘-click do.
    pub show: bool,
}

/// Two points compared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitsDiff {
    /// The commit compared from, its full id.
    pub from: String,
    /// The commit compared to, its full id; `None` for the working tree.
    pub to: Option<String>,
    /// The files that differ, as `to` changed them from `from`: added, modified, deleted or
    /// renamed (with where they were).
    pub files: Vec<CommitFile>,
    /// The diff of the file asked for with `path`.
    pub diff: Option<DiffView>,
}

pub(super) fn compare(session: &mut Session, from: &str, to: Option<&str>) -> Result<CommitsDiff, CommandError> {
    let repository = repository(session)?;
    let from = repository.resolve(from).map_err(failed)?;
    let to = to.filter(|to| !to.trim().is_empty()).map(|to| repository.resolve(to)).transpose().map_err(failed)?;
    let files = repository.diff_revisions(&from, to.as_deref()).map_err(failed)?;
    let root = repository.root();
    let files = files
        .into_iter()
        .map(|file| CommitFile {
            path: absolute(root, &file.path),
            status: file.status.as_str().to_owned(),
            relative: file.path,
            old_path: file.old_path,
        })
        .collect();
    Ok(CommitsDiff { from, to, files, diff: None })
}

pub(super) fn file_diff(
    session: &mut Session,
    from: &str,
    to: Option<&str>,
    path: &Path,
) -> Result<DiffView, CommandError> {
    let compared = compare(session, from, to)?;
    let repository = repository(session)?;
    let relative = relative(&repository, path)?;
    let old = compared
        .files
        .iter()
        .find(|file| file.relative == relative)
        .and_then(|file| file.old_path.clone())
        .unwrap_or_else(|| relative.clone());
    let before = repository.file_at(&compared.from, &old).map_err(failed)?.unwrap_or_default();
    let path = absolute(repository.root(), &relative);
    let (after, after_title) = match &compared.to {
        Some(to) => (repository.file_at(to, &relative).map_err(failed)?.unwrap_or_default(), short(to)),
        None => (current_text(session, &path).unwrap_or_default(), t!("vcs-diff-yours")),
    };
    Ok(diff_view(path, relative, (short(&compared.from), after_title), &before, &after))
}

fn open_workspace(session: &mut Session) -> Result<(), CommandError> {
    if !session.state::<GitWorkspace>().is_some_and(GitWorkspace::is_open) {
        super::workspace::OpenWorkspace.run(session, zen_core::NoParams::default())?;
    }
    Ok(())
}

pub struct DiffCommits;

impl Command for DiffCommits {
    type Params = DiffCommitsParams;
    type Output = CommitsDiff;
    const ID: &'static str = "vcs.diff.commits";
    const TITLE: &'static str = "Compare Commits";
    const DESCRIPTION: &'static str = "Compares two commits (`from` and `to`: ids, branches, tags), or a commit and the working \
         tree as it is now (without `to`, staged or not, new files too): the files that differ, as \
         `to` changed them from `from` (added, modified, deleted, renamed). With `path`, also that \
         file's diff (both texts and the changes). With `show`, the Git workspace shows the \
         comparison in its right panel, as two commits picked with ⌘-click do; \
         vcs.workspace.openDiff with `base` opens a file's diff in the diff tab.";

    fn run(&self, session: &mut Session, params: DiffCommitsParams) -> Result<CommitsDiff, CommandError> {
        if params.from.trim().is_empty() {
            return Err(CommandError::new(t!("vcs-error-revision")));
        }
        let mut compared = compare(session, &params.from, params.to.as_deref())?;
        if let Some(path) = &params.path {
            compared.diff = Some(file_diff(session, &compared.from, compared.to.as_deref(), path)?);
        }
        if params.show {
            open_workspace(session)?;
            let workspace = session.state_mut::<GitWorkspace>();
            workspace.close_menu();
            workspace.panel = Some(RightPanel::Compare { from: compared.from.clone(), to: compared.to.clone() });
            workspace.details = true;
        }
        Ok(compared)
    }
}

/// Whose history to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileHistoryParams {
    /// The file, absolute or relative to the repository.
    pub path: PathBuf,
    /// The history up to this commit (a branch, a tag, an id); `HEAD` when absent.
    pub revision: Option<String>,
    /// Commits to leave out first, for paging.
    pub skip: usize,
    /// The most to list; 300 when absent.
    pub limit: Option<usize>,
    /// Show the history in the Git workspace's right panel (opening the workspace), as History in
    /// a file's menu there does.
    pub show: bool,
}

/// A commit that changed the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileHistoryEntry {
    /// The commit's full id.
    pub id: String,
    /// Its first eight characters.
    pub short_id: String,
    /// The parents' ids.
    pub parents: Vec<String>,
    /// The author's name.
    pub author: String,
    /// The author's e-mail.
    pub email: String,
    /// When it was authored, in seconds since 1970.
    pub time: i64,
    /// The author's time zone, in seconds east of UTC: dates show in it.
    pub offset: i32,
    /// The message's first line.
    pub subject: String,
    /// The file in that commit (it may have been renamed since).
    pub path: PathBuf,
    /// The same, relative to the repository.
    pub relative: String,
    /// How the commit changed the file: `added`, `modified`, `deleted` or `renamed`.
    pub status: String,
    /// Where the file was before, when the commit renamed it (relative).
    pub old_path: Option<String>,
}

/// A file's history.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileHistoryList {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the repository.
    pub relative: String,
    /// The commits that changed it, newest first.
    pub commits: Vec<FileHistoryEntry>,
    /// Whether there are more after them (ask with `skip`).
    pub more: bool,
}

pub struct FileHistory;

impl Command for FileHistory {
    type Params = FileHistoryParams;
    type Output = FileHistoryList;
    const ID: &'static str = "vcs.file.history";
    const TITLE: &'static str = "File History";
    const DESCRIPTION: &'static str = "The commits that changed a file, newest first, from HEAD (or `revision`) back, following \
         the file through renames: each with the file's path in it and how it changed it (added, \
         modified, deleted, renamed); a merge that took the file from one side is left out. \
         vcs.workspace.openDiff with the commit's `id` and that `path` opens its diff at the commit. \
         With `show`, the Git workspace shows the history in its right panel.";

    fn run(&self, session: &mut Session, params: FileHistoryParams) -> Result<FileHistoryList, CommandError> {
        if params.path.as_os_str().is_empty() {
            return Err(CommandError::new(t!("vcs-error-no-file")));
        }
        let repository = repository(session)?;
        let relative = relative(&repository, &params.path)?;
        let revision = params.revision.as_deref().filter(|revision| !revision.trim().is_empty());
        let limit = params.limit.unwrap_or(300).max(1);
        let (revisions, more) = repository.file_history(&relative, revision, params.skip, limit).map_err(failed)?;
        let root = repository.root();
        let commits = revisions
            .into_iter()
            .map(|revision| FileHistoryEntry {
                short_id: short(&revision.commit.id),
                id: revision.commit.id,
                parents: revision.commit.parents,
                author: revision.commit.author,
                email: revision.commit.email,
                time: revision.commit.time,
                offset: revision.commit.offset,
                subject: revision.commit.subject,
                path: absolute(root, &revision.path),
                relative: revision.path,
                status: revision.status.as_str().to_owned(),
                old_path: revision.old_path,
            })
            .collect();
        let path = absolute(root, &relative);
        if params.show {
            open_workspace(session)?;
            let workspace = session.state_mut::<GitWorkspace>();
            workspace.close_menu();
            workspace.panel = Some(RightPanel::FileHistory { path: path.clone() });
            workspace.details = true;
        }
        Ok(FileHistoryList { path, relative, commits, more })
    }
}

/// The file whose menu to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileMenuParams {
    /// The file (absolute or relative to the repository); the menu closes when absent.
    pub path: Option<PathBuf>,
    /// The commit whose files list it (the right panel of a commit); none for the work in progress
    /// or a comparison.
    pub id: Option<String>,
}

fn file_path(session: &mut Session, path: &Path) -> Result<PathBuf, CommandError> {
    if path.as_os_str().is_empty() {
        return Err(CommandError::new(t!("vcs-error-no-file")));
    }
    let repository = repository(session)?;
    Ok(absolute(repository.root(), &relative(&repository, path)?))
}

pub struct FileMenu;

impl Command for FileMenu {
    type Params = FileMenuParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.fileMenu";
    const TITLE: &'static str = "File Menu in Git Workspace";
    const DESCRIPTION: &'static str = "Opens the context menu of a file in the Git workspace's right panel (a commit's files, a \
         comparison's, the work in progress), as a right click on it does: History, Blame and Open \
         File, which vcs.workspace.fileAction runs; without `path` the menu closes. Answers with the \
         tab's state, `fileMenu` the file.";

    fn run(&self, session: &mut Session, params: FileMenuParams) -> Result<WorkspaceState, CommandError> {
        let target = match params.path {
            Some(path) => Some(FileTarget { path: file_path(session, &path)?, id: params.id }),
            None => None,
        };
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        workspace.file_menu = target;
        Ok(state(session))
    }
}

/// An item of a file's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FileAction {
    /// History: the commits that changed the file, in the right panel (vcs.file.history with
    /// `show`).
    History,
    /// Blame: the file in the editor with its blame annotations (vcs.blame.annotate); a click on
    /// an annotation selects its commit in the workspace.
    Blame,
    /// Open File: the file in the editor.
    Open,
}

/// An item of a file's menu to run.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileActionParams {
    /// The file, absolute or relative to the repository.
    pub path: PathBuf,
    /// The item.
    pub action: FileAction,
}

/// What an item of a file's menu did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileActionResult {
    /// The item.
    pub action: FileAction,
    /// The file.
    pub path: PathBuf,
    /// For History: the commits that changed the file.
    pub history: Option<FileHistoryList>,
}

pub struct FileActionCommand;

impl Command for FileActionCommand {
    type Params = FileActionParams;
    type Output = FileActionResult;
    const ID: &'static str = "vcs.workspace.fileAction";
    const TITLE: &'static str = "File Menu Item in Git Workspace";
    const DESCRIPTION: &'static str = "Runs an item of a file's menu in the Git workspace as a click on it does, and closes the \
         menu: history shows the commits that changed the file in the right panel (vcs.file.history, \
         answered in `history`); blame opens the file in the editor with its blame annotations \
         (vcs.blame.annotate; a click on an annotation then selects its commit in the workspace); \
         open opens the file in the editor. Blame and open need the file in the working tree.";

    fn run(&self, session: &mut Session, params: FileActionParams) -> Result<FileActionResult, CommandError> {
        let path = file_path(session, &params.path)?;
        session.state_mut::<GitWorkspace>().file_menu = None;
        let mut result = FileActionResult { action: params.action, path: path.clone(), history: None };
        match params.action {
            FileAction::History => {
                let params = FileHistoryParams { path, show: true, ..FileHistoryParams::default() };
                result.history = Some(FileHistory.run(session, params)?);
            }
            FileAction::Blame | FileAction::Open => {
                if !path.is_file() {
                    let name = path.display().to_string();
                    return Err(CommandError::new(t!("vcs-error-not-in-working-tree", path = name.as_str())));
                }
                crate::open_document(session, &path)?;
                if params.action == FileAction::Blame {
                    let annotate = super::log::AnnotateParams { path: Some(path.clone()), show: Some(true) };
                    super::log::Annotate.run(session, annotate)?;
                    session.state_mut::<GitWorkspace>().blamed.insert(path);
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
