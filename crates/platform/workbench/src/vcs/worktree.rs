use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::Repository;

use super::history::{self, Done, Operation};
use super::workspace::{GitWorkspace, RefTarget, WorkspaceState};
use super::{Shared, changed, command_error, message, repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListWorktrees)?;
    registry.register(CreateWorktree)?;
    registry.register(RemoveWorktree)?;
    registry.register(LockWorktree)?;
    registry.register(OpenWorktree)?;
    registry.register(ListSubmodules)?;
    registry.register(UpdateSubmodules)?;
    registry.register(OpenSubmodule)?;
    registry.register(WorktreeMenu)?;
    registry.register(WorktreeActionCommand)?;
    registry.register(SubmoduleMenu)?;
    registry.register(SubmoduleActionCommand)?;
    Ok(())
}

pub const CREATE_DIALOG: &str = "createWorktree";
pub const REMOVE_DIALOG: &str = "removeWorktree";

/// A worktree, as commands report it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WorktreeInfo {
    /// Its folder.
    pub path: PathBuf,
    /// The folder's name, as the Git workspace shows it.
    pub name: String,
    /// The branch it is on; `None` when its HEAD is detached.
    pub branch: Option<String>,
    /// The commit its HEAD is at.
    pub head: Option<String>,
    /// Whether it is the main worktree (where the repository's `.git` folder is).
    pub main: bool,
    /// Whether it is this project's own.
    pub current: bool,
    /// Whether it is locked: git neither prunes nor removes it.
    pub locked: bool,
    /// Why it is locked, when a reason was given.
    pub lock_reason: Option<String>,
    /// Whether its folder is gone (git would prune it).
    pub prunable: bool,
    /// How many of its files differ from its HEAD: its work in progress. `None` until counted.
    pub changes: Option<usize>,
}

impl WorktreeInfo {
    fn new(worktree: zen_vcs::Worktree, root: &Path) -> Self {
        let name = worktree.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        WorktreeInfo {
            current: worktree.path == root,
            name,
            path: worktree.path,
            branch: worktree.branch,
            head: worktree.head,
            main: worktree.main,
            locked: worktree.locked.is_some(),
            lock_reason: worktree.locked.filter(|reason| !reason.is_empty()),
            prunable: worktree.prunable.is_some(),
            changes: None,
        }
    }

    pub fn label(&self) -> &str {
        self.branch.as_deref().unwrap_or(&self.name)
    }
}

/// The worktrees.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WorktreeList {
    /// The main worktree first, then the linked ones by folder.
    pub worktrees: Vec<WorktreeInfo>,
    /// Whether they are being listed and counted again (the list is the last one meanwhile).
    pub computing: bool,
    /// Raised whenever they were listed again.
    pub revision: u64,
    /// Why listing them failed, if it did.
    pub error: Option<String>,
}

#[derive(Default)]
pub(super) struct WorktreeCache {
    list: Vec<WorktreeInfo>,
    stamp: Option<u64>,
    computing: bool,
    again: bool,
    revision: u64,
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct WipWorktree {
    pub(super) path: PathBuf,
    pub(super) head: String,
    pub(super) label: String,
}

fn cache(shared: &Shared) -> std::sync::MutexGuard<'_, WorktreeCache> {
    shared.worktrees.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn wip_worktrees(shared: &Shared) -> Vec<WipWorktree> {
    cache(shared)
        .list
        .iter()
        .filter(|worktree| !worktree.current && worktree.changes.unwrap_or(0) > 0)
        .filter_map(|worktree| {
            Some(WipWorktree {
                path: worktree.path.clone(),
                head: worktree.head.clone()?,
                label: worktree.label().to_owned(),
            })
        })
        .collect()
}

pub(super) fn wip_stamp(worktrees: &[WipWorktree]) -> u64 {
    use std::hash::{Hash, Hasher};
    if worktrees.is_empty() {
        return 0;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    worktrees.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn count_if_stale(session: &mut Session, force: bool) {
    let vcs = vcs(session);
    let Some(repository) = vcs.repository.clone() else { return };
    let (revision, current_changes) = {
        let snapshot = vcs.shared.snapshot();
        (snapshot.revision, snapshot.status.changes.len())
    };
    let stamp = repository.worktrees_stamp() ^ revision.rotate_left(17);
    let shared = Arc::clone(&vcs.shared);
    {
        let mut cache = cache(&shared);
        if !force && cache.stamp == Some(stamp) {
            return;
        }
        cache.stamp = Some(stamp);
        if cache.computing {
            cache.again = true;
            return;
        }
        cache.computing = true;
    }
    let compute = move || {
        loop {
            let root = repository.root().to_owned();
            let listed = repository.worktrees();
            let result = listed.map(|worktrees| {
                worktrees
                    .into_iter()
                    .map(|worktree| {
                        let mut info = WorktreeInfo::new(worktree, &root);
                        info.changes = if info.current {
                            Some(current_changes)
                        } else if info.prunable {
                            None
                        } else {
                            Repository::worktree_changes(&info.path).ok()
                        };
                        info
                    })
                    .collect::<Vec<_>>()
            });
            let again = {
                let mut cache = cache(&shared);
                match result {
                    Ok(list) => {
                        cache.list = list;
                        cache.error = None;
                    }
                    Err(error) => cache.error = Some(message(&error)),
                }
                cache.revision += 1;
                let again = std::mem::take(&mut cache.again);
                cache.computing = again;
                again
            };
            shared.wake();
            if !again {
                break;
            }
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(compute);
    #[cfg(target_arch = "wasm32")]
    compute();
}

fn listing(session: &mut Session, refresh: bool, wait: bool) -> Result<WorktreeList, CommandError> {
    repository(session)?;
    count_if_stale(session, refresh);
    let shared = Arc::clone(&vcs(session).shared);
    if wait {
        for _ in 0..4000 {
            if !cache(&shared).computing {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    let cache = cache(&shared);
    Ok(WorktreeList {
        worktrees: cache.list.clone(),
        computing: cache.computing,
        revision: cache.revision,
        error: cache.error.clone(),
    })
}

fn relist(session: &mut Session) -> Result<WorktreeList, CommandError> {
    changed(session);
    vcs(session).settle();
    listing(session, true, true)
}

/// Whether to list the worktrees again and wait for it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// List and count them again now, even if nothing seems to have changed (files edited in
    /// another worktree are not noticed otherwise).
    pub refresh: bool,
    /// Wait until they are listed and counted.
    pub wait: bool,
}

pub struct ListWorktrees;

impl Command for ListWorktrees {
    type Params = ListParams;
    type Output = WorktreeList;
    const ID: &'static str = "vcs.worktree.list";
    const TITLE: &'static str = "Worktrees";
    const DESCRIPTION: &'static str = "Lists the repository's worktrees (git worktree): the main one first, then the linked \
         ones, each with its folder, branch (or detached HEAD), whether it is this project's own \
         (`current`), locked (with the reason) or gone (`prunable`), and how many of its files changed \
         (`changes`). They are counted in the background when the worktrees or this repository change; \
         `refresh` counts them again now, `wait` waits for the counts.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<WorktreeList, CommandError> {
        listing(session, params.refresh, params.wait)
    }
}

pub fn default_worktree_folder(root: &Path, branch: &str) -> PathBuf {
    let parent = root.parent().unwrap_or(root);
    let repository = root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let short: String =
        branch.chars().map(|c| if c == '/' || c == '\\' || c.is_whitespace() { '-' } else { c }).collect();
    let base = format!("{repository}-{}", short.trim_matches('-'));
    let mut folder = parent.join(&base);
    let mut number = 2;
    while folder.exists() {
        folder = parent.join(format!("{base}-{number}"));
        number += 1;
    }
    folder
}

/// What worktree to make.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateParams {
    /// The branch to check out there: a local branch (`feature`), or a remote one (`origin/fix`),
    /// from which a local branch of the same name (`fix`) is made unless `newBranch` names
    /// another; any revision (`v1.0`, an id) with `newBranch`.
    pub branch: String,
    /// The folder, which must not exist or be empty; a relative one is taken next to the
    /// repository's folder. Next to it, `<repository>-<branch>`, when absent.
    pub path: Option<PathBuf>,
    /// A new branch to make at `branch` and check out there instead (a local branch can be
    /// checked out in one worktree only).
    pub new_branch: Option<String>,
    /// Open the new worktree in a project window of its own as well.
    pub open: bool,
}

pub struct CreateWorktree;

impl Command for CreateWorktree {
    type Params = CreateParams;
    type Output = WorktreeInfo;
    const ID: &'static str = "vcs.worktree.create";
    const TITLE: &'static str = "Create Worktree";
    const DESCRIPTION: &'static str = "Makes a new worktree (git worktree add) checked out at `branch`: a local branch as it \
         is (git refuses one checked out in another worktree), a remote branch through a new local \
         branch of the same name that tracks it, or a new branch `newBranch` made at `branch`. The \
         folder is `path` (next to the repository's folder when relative), `<repository>-<branch>` \
         next to it when absent. With `open`, the worktree opens in a project window of its own. \
         Answers with the worktree.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<WorktreeInfo, CommandError> {
        let repository = repository(session)?;
        let branch = params.branch.trim();
        if branch.is_empty() {
            return Err(CommandError::new(t!("vcs-error-branch-name")));
        }
        let mut new_branch =
            params.new_branch.as_deref().map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned);
        if new_branch.is_none() {
            let branches = repository.branches().map_err(|error| command_error(&error))?;
            let remote = branches.iter().any(|known| known.remote && known.name == branch);
            if remote {
                let short = branch.split_once('/').map_or(branch, |(_, name)| name).to_owned();
                if branches.iter().any(|known| !known.remote && known.name == short) {
                    return Err(CommandError::new(t!("vcs-error-worktree-branch-exists", branch = short.as_str())));
                }
                new_branch = Some(short);
            }
        }
        let named = new_branch.as_deref().unwrap_or(branch);
        let base = repository.root().parent().unwrap_or(repository.root()).to_owned();
        let folder = match params.path {
            Some(path) if !path.as_os_str().is_empty() => {
                if path.is_absolute() {
                    path
                } else {
                    base.join(path)
                }
            }
            _ => default_worktree_folder(repository.root(), named),
        };
        if folder.exists() && std::fs::read_dir(&folder).is_ok_and(|mut entries| entries.next().is_some()) {
            return Err(CommandError::new(t!("vcs-error-worktree-folder", folder = folder.display().to_string())));
        }
        let made = repository.add_worktree(&folder, branch, new_branch.as_deref());
        let list = relist(session)?;
        let made = made.map_err(|error| command_error(&error))?;
        let info = list
            .worktrees
            .into_iter()
            .find(|worktree| worktree.path == made.path)
            .unwrap_or_else(|| WorktreeInfo::new(made, repository.root()));
        if params.open {
            session.state_mut::<ProjectWindowRequest>().path = Some(info.path.clone());
        }
        Ok(info)
    }
}

fn find_worktree(session: &mut Session, path: &Path) -> Result<WorktreeInfo, CommandError> {
    let same =
        |info: &WorktreeInfo| info.path == path || path.canonicalize().is_ok_and(|canonical| canonical == info.path);
    let listed = listing(session, false, true)?;
    if let Some(found) = listed.worktrees.into_iter().find(same) {
        return Ok(found);
    }
    listing(session, true, true)?
        .worktrees
        .into_iter()
        .find(same)
        .ok_or_else(|| CommandError::new(t!("vcs-error-no-worktree", folder = path.display().to_string())))
}

/// A worktree to remove.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoveParams {
    /// Its folder.
    pub path: PathBuf,
    /// Delete its branch too, once it is removed (Undo brings the branch back).
    pub delete_branch: bool,
    /// Remove it even with changes or untracked files in it: they are lost.
    pub force: bool,
}

pub struct RemoveWorktree;

impl Command for RemoveWorktree {
    type Params = RemoveParams;
    type Output = WorktreeList;
    const ID: &'static str = "vcs.worktree.remove";
    const TITLE: &'static str = "Remove Worktree";
    const DESCRIPTION: &'static str = "Removes a linked worktree (git worktree remove): its folder and git's record of it. One \
         with changes or untracked files is refused unless `force` (they are lost), a locked one \
         always (unlock it first), and neither the main worktree nor this project's own can be \
         removed. With `deleteBranch`, its branch is deleted afterwards (recorded for Undo, which \
         brings the branch back). The Git workspace asks first; this command does not. Answers with \
         the worktrees left.";

    fn run(&self, session: &mut Session, params: RemoveParams) -> Result<WorktreeList, CommandError> {
        let repository = repository(session)?;
        let worktree = find_worktree(session, &params.path)?;
        if worktree.main || worktree.current {
            return Err(CommandError::new(t!("vcs-error-worktree-remove-own")));
        }
        if worktree.locked {
            return Err(CommandError::new(t!("vcs-error-worktree-locked", folder = worktree.name.as_str())));
        }
        crate::hidden_files::guard_change(session, &worktree.path, std::slice::from_ref(&worktree.path))?;
        let removed = repository.remove_worktree(&worktree.path, params.force);
        if let Err(error) = removed {
            relist(session)?;
            return Err(command_error(&error));
        }
        if params.delete_branch
            && let Some(branch) = &worktree.branch
        {
            let recording = history::begin(session, &repository);
            let deleted = repository.delete_branch(branch, true);
            if deleted.is_ok() {
                history::finish(session, &repository, recording, Done::new(Operation::BranchDelete, branch.as_str()));
            }
            let list = relist(session)?;
            deleted.map_err(|error| command_error(&error))?;
            return Ok(list);
        }
        relist(session)
    }
}

/// A worktree to lock or unlock.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LockParams {
    /// Its folder.
    pub path: PathBuf,
    /// Lock it (the default) or unlock it.
    #[serde(default = "locked_by_default")]
    pub locked: bool,
    /// Why it is locked (on a removable disk, in use...), kept with the lock.
    #[serde(default)]
    pub reason: Option<String>,
}

fn locked_by_default() -> bool {
    true
}

pub struct LockWorktree;

impl Command for LockWorktree {
    type Params = LockParams;
    type Output = WorktreeInfo;
    const ID: &'static str = "vcs.worktree.lock";
    const TITLE: &'static str = "Lock Worktree";
    const DESCRIPTION: &'static str = "Locks a linked worktree (git worktree lock, with `reason` if given), so that git neither \
         prunes nor removes it, as for one on a removable disk; with `locked: false`, unlocks it. The \
         main worktree has no lock. Answers with the worktree.";

    fn run(&self, session: &mut Session, params: LockParams) -> Result<WorktreeInfo, CommandError> {
        let repository = repository(session)?;
        let worktree = find_worktree(session, &params.path)?;
        if worktree.main {
            return Err(CommandError::new(t!("vcs-error-worktree-lock-main")));
        }
        if worktree.locked != params.locked {
            let result = repository.lock_worktree(&worktree.path, params.locked, params.reason.as_deref());
            relist(session)?;
            result.map_err(|error| command_error(&error))?;
        }
        find_worktree(session, &worktree.path)
    }
}

#[derive(Default)]
pub struct ProjectWindowRequest {
    path: Option<PathBuf>,
}

impl ProjectWindowRequest {
    pub fn take(&mut self) -> Option<PathBuf> {
        self.path.take()
    }
}

/// A worktree or a submodule to open, by its folder.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PathParams {
    /// The worktree's folder; for a submodule, its path in the repository (`libs/lib`) or its
    /// folder.
    pub path: PathBuf,
}

/// Where a project opens.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Opened {
    /// The folder, opened in a project window of its own (or brought to the front when it is open).
    pub path: PathBuf,
}

pub struct OpenWorktree;

impl Command for OpenWorktree {
    type Params = PathParams;
    type Output = Opened;
    const ID: &'static str = "vcs.worktree.open";
    const TITLE: &'static str = "Open Worktree in New Window";
    const DESCRIPTION: &'static str = "Opens a worktree's folder as a project in a window of its own (or brings its window to \
         the front when it is open), as the worktree menu's Open in New Window does. Agents then \
         address that window by its folder (`project`).";

    fn run(&self, session: &mut Session, params: PathParams) -> Result<Opened, CommandError> {
        let worktree = find_worktree(session, &params.path)?;
        if worktree.prunable || !worktree.path.is_dir() {
            return Err(CommandError::new(t!("vcs-error-worktree-gone", folder = worktree.name.as_str())));
        }
        session.state_mut::<ProjectWindowRequest>().path = Some(worktree.path.clone());
        Ok(Opened { path: worktree.path })
    }
}

/// Where a submodule is against the commit the repository records for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SubmoduleStatus {
    /// Not initialised: its folder is empty; Initialise clones it.
    #[default]
    Uninitialized,
    /// At the recorded commit.
    Current,
    /// At another commit than the recorded one; Update checks the recorded one out.
    Moved,
    /// The recorded commit is in conflict (a merge left both sides).
    Conflicted,
}

impl From<zen_vcs::SubmoduleState> for SubmoduleStatus {
    fn from(state: zen_vcs::SubmoduleState) -> Self {
        match state {
            zen_vcs::SubmoduleState::Uninitialized => SubmoduleStatus::Uninitialized,
            zen_vcs::SubmoduleState::Current => SubmoduleStatus::Current,
            zen_vcs::SubmoduleState::Moved => SubmoduleStatus::Moved,
            zen_vcs::SubmoduleState::Conflicted => SubmoduleStatus::Conflicted,
        }
    }
}

/// A submodule, as commands report it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SubmoduleInfo {
    /// Its name in `.gitmodules`.
    pub name: String,
    /// Its path in the repository, with `/`: `libs/lib`.
    pub path: String,
    /// Its folder.
    pub folder: PathBuf,
    /// Where it is cloned from.
    pub url: Option<String>,
    /// The commit the repository records for it.
    pub recorded: Option<String>,
    /// The commit it is checked out at; `None` when it is not initialised.
    pub checked_out: Option<String>,
    /// Where it is against the recorded commit.
    pub status: SubmoduleStatus,
    /// Whether it is checked out at the recorded commit.
    pub at_recorded: bool,
}

impl SubmoduleInfo {
    fn new(submodule: zen_vcs::Submodule, root: &Path) -> Self {
        let status = SubmoduleStatus::from(submodule.state);
        SubmoduleInfo {
            folder: super::absolute(root, &submodule.path),
            name: submodule.name,
            path: submodule.path,
            url: submodule.url,
            recorded: submodule.recorded,
            checked_out: submodule.checked_out,
            at_recorded: status == SubmoduleStatus::Current,
            status,
        }
    }
}

/// The submodules.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SubmoduleList {
    /// In the order git lists them.
    pub submodules: Vec<SubmoduleInfo>,
    /// With `cached`, raised each time the list changes.
    pub revision: u64,
}

/// How to list the submodules.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SubmoduleListParams {
    /// Answer at once with the last list read in the background.
    pub cached: bool,
    /// With `cached`, read the list again in the background.
    pub refresh: bool,
}

#[derive(Default)]
pub(super) struct SubmoduleCache {
    list: SubmoduleList,
    computing: bool,
    again: bool,
}

fn submodule_cache(shared: &Shared) -> std::sync::MutexGuard<'_, SubmoduleCache> {
    shared.submodules.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_submodules_behind(shared: Arc<Shared>, repository: Arc<Repository>) {
    {
        let mut cache = submodule_cache(&shared);
        cache.again = cache.computing;
        if cache.computing {
            return;
        }
        cache.computing = true;
    }
    let compute = move || {
        loop {
            let listed = submodules(&repository);
            let mut cache = submodule_cache(&shared);
            if let Ok(list) = listed
                && list.submodules != cache.list.submodules
            {
                cache.list = SubmoduleList { revision: cache.list.revision + 1, ..list };
            }
            let again = std::mem::take(&mut cache.again);
            cache.computing = again;
            drop(cache);
            shared.wake();
            if !again {
                break;
            }
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(compute);
    #[cfg(target_arch = "wasm32")]
    compute();
}

fn submodules(repository: &Repository) -> Result<SubmoduleList, CommandError> {
    let listed = repository.submodules().map_err(|error| command_error(&error))?;
    let root = repository.root();
    let submodules = listed.into_iter().map(|submodule| SubmoduleInfo::new(submodule, root)).collect();
    Ok(SubmoduleList { submodules, revision: 0 })
}

pub struct ListSubmodules;

impl Command for ListSubmodules {
    type Params = SubmoduleListParams;
    type Output = SubmoduleList;
    const ID: &'static str = "vcs.submodule.list";
    const TITLE: &'static str = "Submodules";
    const DESCRIPTION: &'static str = "Lists the repository's submodules: each one's name, path, folder and address, the \
         commit the repository records for it, the one it is checked out at, and whether it is at the \
         recorded commit (`status`: current, moved, uninitialized or conflicted; `atRecorded`). With \
         `cached` it answers at once with the last list read in the background, and `refresh` reads \
         it again.";

    fn run(&self, session: &mut Session, params: SubmoduleListParams) -> Result<SubmoduleList, CommandError> {
        let repository = repository(session)?;
        if !params.cached {
            return submodules(&repository);
        }
        let shared = Arc::clone(&vcs(session).shared);
        if params.refresh {
            read_submodules_behind(Arc::clone(&shared), repository);
        }
        let cache = submodule_cache(&shared);
        Ok(cache.list.clone())
    }
}

fn find_submodule(repository: &Repository, path: &Path) -> Result<SubmoduleInfo, CommandError> {
    let list = submodules(repository)?;
    let relative = path.strip_prefix(repository.root()).unwrap_or(path);
    let relative = relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    list.submodules
        .into_iter()
        .find(|submodule| submodule.path == relative || submodule.folder == path)
        .ok_or_else(|| CommandError::new(t!("vcs-error-no-submodule", path = relative.as_str())))
}

/// Which submodules to update, and how.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateParams {
    /// Their paths in the repository (`libs/lib`) or folders; every submodule when empty.
    pub paths: Vec<PathBuf>,
    /// Initialise (clone) the ones that are not yet: the submodule menu's Initialise.
    pub init: bool,
    /// Their own submodules too (the default).
    pub recursive: bool,
    /// Wait until it is over; otherwise it runs in the background and vcs.operation.status
    /// follows it.
    pub wait: bool,
}

impl Default for UpdateParams {
    fn default() -> Self {
        UpdateParams { paths: Vec::new(), init: false, recursive: true, wait: false }
    }
}

pub struct UpdateSubmodules;

impl Command for UpdateSubmodules {
    type Params = UpdateParams;
    type Output = super::OperationInfo;
    const ID: &'static str = "vcs.submodule.update";
    const TITLE: &'static str = "Update Submodules";
    const DESCRIPTION: &'static str = "Checks submodules out at the commits the repository records (git submodule update), \
         fetching what is missing: those of `paths` (every one when empty), with `init` also \
         initialising and cloning those that are not yet, with `recursive` (the default) their own \
         submodules too. Runs in the background unless `wait` is set; vcs.operation.status reports \
         the progress and the outcome.";

    fn run(&self, session: &mut Session, params: UpdateParams) -> Result<super::OperationInfo, CommandError> {
        let repository = repository(session)?;
        let mut paths = Vec::new();
        for path in &params.paths {
            paths.push(find_submodule(&repository, path)?.path);
        }
        if paths.is_empty() && submodules(&repository)?.submodules.is_empty() {
            return Err(CommandError::new(t!("vcs-error-no-submodules")));
        }
        let (init, recursive) = (params.init, params.recursive);
        let title = if init { t!("vcs-progress-submodule-init") } else { t!("vcs-progress-submodule-update") };
        super::remote::start(
            session,
            "submodule",
            title,
            params.wait,
            Box::new(move |repository, progress| {
                repository.update_submodules(&paths, init, recursive, progress).map_err(|error| message(&error))?;
                let count = if paths.is_empty() {
                    repository.submodules().map(|list| list.len()).unwrap_or(0)
                } else {
                    paths.len()
                };
                Ok(zen_i18n::Message::new("vcs-submodule-updated").with_number("count", count as f64))
            }),
        )
    }
}

pub struct OpenSubmodule;

impl Command for OpenSubmodule {
    type Params = PathParams;
    type Output = Opened;
    const ID: &'static str = "vcs.submodule.open";
    const TITLE: &'static str = "Open Submodule";
    const DESCRIPTION: &'static str = "Opens a submodule's folder as a project in a window of its own (or brings its window to \
         the front), as the submodule menu's Open as Project does; `path` is its path in the \
         repository (`libs/lib`) or its folder. A submodule that is not initialised is refused: \
         vcs.submodule.update with `init` clones it first.";

    fn run(&self, session: &mut Session, params: PathParams) -> Result<Opened, CommandError> {
        let repository = repository(session)?;
        let submodule = find_submodule(&repository, &params.path)?;
        if submodule.status == SubmoduleStatus::Uninitialized {
            return Err(CommandError::new(t!("vcs-error-submodule-uninitialized", path = submodule.path.as_str())));
        }
        session.state_mut::<ProjectWindowRequest>().path = Some(submodule.folder.clone());
        Ok(Opened { path: submodule.folder })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreePrompt {
    Create { branch: RefTarget },
    Remove { worktree: WorktreeInfo },
}

/// The worktree whose menu to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WorktreeMenuParams {
    /// Its folder, as a right click on its row in the WORKTREES section (or on its work in
    /// progress in the graph) opens it. The menu closes when absent.
    pub path: Option<PathBuf>,
}

pub struct WorktreeMenu;

impl Command for WorktreeMenu {
    type Params = WorktreeMenuParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.worktreeMenu";
    const TITLE: &'static str = "Worktree Menu";
    const DESCRIPTION: &'static str = "Opens the context menu of a worktree in the Git workspace, as a right click on its row \
         does: the worktree whose folder is `path`. Its HEAD is selected; vcs.workspace.worktreeAction \
         runs an item; without `path` the menu closes. Answers with the tab's state, `worktreeMenu` \
         the folder.";

    fn run(&self, session: &mut Session, params: WorktreeMenuParams) -> Result<WorkspaceState, CommandError> {
        let Some(path) = params.path.filter(|path| !path.as_os_str().is_empty()) else {
            session.state_mut::<GitWorkspace>().worktree_menu = None;
            return Ok(super::workspace::state(session));
        };
        let worktree = find_worktree(session, &path)?;
        let workspace = session.state_mut::<GitWorkspace>();
        if let Some(head) = &worktree.head {
            workspace.select_commit(head.clone());
        }
        workspace.close_menu();
        workspace.worktree_menu = Some(worktree.path);
        Ok(super::workspace::state(session))
    }
}

/// An item of a worktree's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeAction {
    /// Open in New Window (vcs.worktree.open).
    Open,
    /// Lock Worktree (vcs.worktree.lock).
    Lock,
    /// Unlock Worktree (vcs.worktree.lock with `locked: false`).
    Unlock,
    /// Remove Worktree…: asks first, and whether to delete its branch (vcs.worktree.remove).
    Remove,
}

/// An item of a worktree's menu to run.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeActionParams {
    /// The worktree's folder.
    pub path: PathBuf,
    /// The item.
    pub action: WorktreeAction,
}

/// What an item of a worktree's menu did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeActionResult {
    /// The item.
    pub action: WorktreeAction,
    /// The worktree, as it is afterwards.
    pub worktree: WorktreeInfo,
    /// The dialog the item opened (`removeWorktree`), where the user goes on; `get_window_state`
    /// lists it among the popups.
    pub dialog: Option<String>,
}

pub struct WorktreeActionCommand;

impl Command for WorktreeActionCommand {
    type Params = WorktreeActionParams;
    type Output = WorktreeActionResult;
    const ID: &'static str = "vcs.workspace.worktreeAction";
    const TITLE: &'static str = "Worktree Menu Item";
    const DESCRIPTION: &'static str = "Runs an item of the Git workspace's worktree menu as a click on it does, and closes the \
         menu: open (in a project window of its own), lock and unlock run at once; remove asks first \
         in the `removeWorktree` dialog (which also offers to delete its branch). Removing or locking \
         the main worktree, and removing this project's own, are refused. The operations themselves \
         are vcs.worktree.open, vcs.worktree.lock and vcs.worktree.remove.";

    fn run(&self, session: &mut Session, params: WorktreeActionParams) -> Result<WorktreeActionResult, CommandError> {
        let worktree = find_worktree(session, &params.path)?;
        session.state_mut::<GitWorkspace>().close_menu();
        let path = worktree.path.clone();
        let mut result = WorktreeActionResult { action: params.action, worktree, dialog: None };
        match params.action {
            WorktreeAction::Open => {
                OpenWorktree.run(session, PathParams { path })?;
            }
            WorktreeAction::Lock | WorktreeAction::Unlock => {
                let locked = params.action == WorktreeAction::Lock;
                result.worktree = LockWorktree.run(session, LockParams { path, locked, reason: None })?;
            }
            WorktreeAction::Remove => {
                if result.worktree.main || result.worktree.current {
                    return Err(CommandError::new(t!("vcs-error-worktree-remove-own")));
                }
                session.state_mut::<GitWorkspace>().worktree_prompt =
                    Some(WorktreePrompt::Remove { worktree: result.worktree.clone() });
                result.dialog = Some(REMOVE_DIALOG.to_owned());
            }
        }
        Ok(result)
    }
}

/// The submodule whose menu to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SubmoduleMenuParams {
    /// Its path in the repository (`libs/lib`), as a right click on its row in the SUBMODULES
    /// section opens it. The menu closes when absent.
    pub path: Option<String>,
}

pub struct SubmoduleMenu;

impl Command for SubmoduleMenu {
    type Params = SubmoduleMenuParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.submoduleMenu";
    const TITLE: &'static str = "Submodule Menu";
    const DESCRIPTION: &'static str = "Opens the context menu of a submodule in the Git workspace, as a right click on its row \
         does: the submodule whose path is `path`. vcs.workspace.submoduleAction runs an item; without \
         `path` the menu closes. Answers with the tab's state, `submoduleMenu` the path.";

    fn run(&self, session: &mut Session, params: SubmoduleMenuParams) -> Result<WorkspaceState, CommandError> {
        let Some(path) = params.path.filter(|path| !path.trim().is_empty()) else {
            session.state_mut::<GitWorkspace>().submodule_menu = None;
            return Ok(super::workspace::state(session));
        };
        let submodule = find_submodule(&*repository(session)?, Path::new(path.trim()))?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        workspace.submodule_menu = Some(submodule.path);
        Ok(super::workspace::state(session))
    }
}

/// An item of a submodule's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SubmoduleAction {
    /// Update: check the recorded commit out (vcs.submodule.update).
    Update,
    /// Initialise: clone it and check the recorded commit out (vcs.submodule.update with `init`).
    Initialize,
    /// Open as Project, in a window of its own (vcs.submodule.open).
    Open,
}

/// An item of a submodule's menu to run.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmoduleActionParams {
    /// The submodule's path in the repository (`libs/lib`).
    pub path: String,
    /// The item.
    pub action: SubmoduleAction,
    /// For Update and Initialise: wait until it is over.
    #[serde(default)]
    pub wait: bool,
}

/// What an item of a submodule's menu did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmoduleActionResult {
    /// The item.
    pub action: SubmoduleAction,
    /// The submodule, as it was.
    pub submodule: SubmoduleInfo,
    /// The background operation of Update and Initialise.
    pub operation: Option<super::OperationInfo>,
}

pub struct SubmoduleActionCommand;

impl Command for SubmoduleActionCommand {
    type Params = SubmoduleActionParams;
    type Output = SubmoduleActionResult;
    const ID: &'static str = "vcs.workspace.submoduleAction";
    const TITLE: &'static str = "Submodule Menu Item";
    const DESCRIPTION: &'static str = "Runs an item of the Git workspace's submodule menu as a click on it does, and closes the \
         menu: update (to the recorded commit) and initialize (clone it first) run in the background \
         unless `wait`; open opens it as a project in a window of its own (refused while it is not \
         initialised). The operations themselves are vcs.submodule.update and vcs.submodule.open.";

    fn run(&self, session: &mut Session, params: SubmoduleActionParams) -> Result<SubmoduleActionResult, CommandError> {
        let submodule = find_submodule(&*repository(session)?, Path::new(params.path.trim()))?;
        session.state_mut::<GitWorkspace>().close_menu();
        let path = PathBuf::from(&submodule.path);
        let mut result = SubmoduleActionResult { action: params.action, submodule, operation: None };
        match params.action {
            SubmoduleAction::Update | SubmoduleAction::Initialize => {
                let init = params.action == SubmoduleAction::Initialize;
                let update = UpdateParams { paths: vec![path], init, recursive: true, wait: params.wait };
                result.operation = Some(UpdateSubmodules.run(session, update)?);
            }
            SubmoduleAction::Open => {
                OpenSubmodule.run(session, PathParams { path })?;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    fn run(
        registry: &Registry,
        session: &mut Session,
        command: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, zen_core::DispatchError> {
        registry.dispatch(session, command, params)
    }

    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = test_repository();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("app");
        std::fs::create_dir(&root).unwrap();
        for name in [".git", "a.txt"] {
            std::fs::rename(base.join(name), root.join(name)).unwrap();
        }
        (dir, root)
    }

    #[test]
    fn worktrees_are_made_counted_locked_opened_and_removed_with_their_branch() {
        let (dir, root) = project();
        let base = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        let listed = run(&registry, s, "vcs.worktree.list", json!({ "wait": true })).unwrap();
        assert_eq!(listed["worktrees"].as_array().unwrap().len(), 1);
        assert_eq!(
            (listed["worktrees"][0]["main"].as_bool(), listed["worktrees"][0]["current"].as_bool()),
            (Some(true), Some(true))
        );

        assert!(run(&registry, s, "vcs.worktree.create", json!({ "branch": " " })).is_err());
        let made =
            run(&registry, s, "vcs.worktree.create", json!({ "branch": "main", "newBranch": "feature" })).unwrap();
        assert_eq!(made["path"], json!(base.join("app-feature")));
        assert_eq!((made["branch"].as_str(), made["current"].as_bool()), (Some("feature"), Some(false)));
        git(&root, &["branch", "fix/login"]);
        let fix = run(
            &registry,
            s,
            "vcs.worktree.create",
            json!({ "branch": "fix/login", "path": "elsewhere", "open": true }),
        )
        .unwrap();
        assert_eq!(fix["path"], json!(base.join("elsewhere")));
        assert_eq!(session_take(s), Some(base.join("elsewhere")), "asked to open in a window of its own");
        assert!(
            run(&registry, s, "vcs.worktree.create", json!({ "branch": "fix/login", "path": "again" })).is_err(),
            "a branch is checked out in one worktree only"
        );
        assert_eq!(default_worktree_folder(&root, "fix/login"), base.join("app-fix-login"));
        std::fs::create_dir(base.join("app-fix-login")).unwrap();
        assert_eq!(default_worktree_folder(&root, "fix/login"), base.join("app-fix-login-2"), "taken");

        std::fs::write(base.join("app-feature/a.txt"), "changed\n").unwrap();
        let listed = run(&registry, s, "vcs.worktree.list", json!({ "refresh": true, "wait": true })).unwrap();
        let feature =
            listed["worktrees"].as_array().unwrap().iter().find(|w| w["branch"] == "feature").unwrap().clone();
        assert_eq!(feature["changes"], 1);
        let page = run(&registry, s, "vcs.graph.page", json!({ "wait": true })).unwrap();
        let rows = page["commits"].as_array().unwrap();
        assert_eq!(rows[0]["worktree"], json!(base.join("app-feature")), "{rows:?}");
        assert_eq!((rows[0]["subject"].as_str(), rows[0]["id"].as_str()), (Some("feature"), Some("")));
        let head = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
        assert_eq!(rows[0]["parents"], json!([head]));
        assert_eq!(rows[1]["id"], head.as_str());

        let feature_path = base.join("app-feature");
        let locked =
            run(&registry, s, "vcs.worktree.lock", json!({ "path": feature_path, "reason": "on a disk" })).unwrap();
        assert_eq!((locked["locked"].as_bool(), locked["lockReason"].as_str()), (Some(true), Some("on a disk")));
        assert!(run(&registry, s, "vcs.worktree.remove", json!({ "path": feature_path, "force": true })).is_err());
        let unlocked =
            run(&registry, s, "vcs.worktree.lock", json!({ "path": feature_path, "locked": false })).unwrap();
        assert_eq!(unlocked["locked"], false);
        assert!(run(&registry, s, "vcs.worktree.remove", json!({ "path": feature_path })).is_err(), "changes");
        assert!(run(&registry, s, "vcs.worktree.remove", json!({ "path": root })).is_err(), "never its own");
        assert!(run(&registry, s, "vcs.worktree.lock", json!({ "path": root })).is_err(), "the main one");

        let left = run(
            &registry,
            s,
            "vcs.worktree.remove",
            json!({ "path": feature_path, "force": true, "deleteBranch": true }),
        )
        .unwrap();
        assert_eq!(left["worktrees"].as_array().unwrap().len(), 2);
        assert!(!feature_path.exists());
        assert!(git(&root, &["branch", "--list", "feature"]).trim().is_empty());
        let history = run(&registry, s, "vcs.history.list", json!({})).unwrap();
        assert_eq!(history["done"].as_array().unwrap().last().unwrap()["operation"], "branchDelete", "{history}");
        run(&registry, s, "vcs.history.undo", json!({})).unwrap();
        assert!(git(&root, &["branch", "--list", "feature"]).contains("feature"));

        let opened = run(&registry, s, "vcs.worktree.open", json!({ "path": base.join("elsewhere") })).unwrap();
        assert_eq!(opened["path"], json!(base.join("elsewhere")));
        assert_eq!(session_take(s), Some(base.join("elsewhere")));
        assert!(run(&registry, s, "vcs.worktree.open", json!({ "path": base.join("nowhere") })).is_err());
    }

    #[test]
    fn the_sections_of_the_left_panel_start_folded_and_unfold() {
        let (_dir, root) = project();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        assert_eq!(run(&registry, s, "vcs.workspace.state", json!({})).unwrap()["openSections"], json!([]));
        run(&registry, s, "vcs.workspace.section", json!({ "section": "submodules", "open": true })).unwrap();
        let state = run(&registry, s, "vcs.workspace.section", json!({ "section": "worktrees" })).unwrap();
        assert_eq!(state["openSections"], json!(["worktrees", "submodules"]), "in the panel's order");
        let state = run(&registry, s, "vcs.workspace.section", json!({ "section": "worktrees" })).unwrap();
        assert_eq!(state["openSections"], json!(["submodules"]), "a second click folds it");
        assert!(run(&registry, s, "vcs.workspace.section", json!({ "section": "nothing" })).is_err());
    }

    fn session_take(session: &mut Session) -> Option<PathBuf> {
        session.state_mut::<ProjectWindowRequest>().take()
    }

    #[test]
    fn a_remote_branch_makes_a_local_branch_of_its_own() {
        let (dir, root) = project();
        let base = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        git(&root, &["update-ref", "refs/remotes/origin/fix", "HEAD"]);
        let made = run(&registry, &mut session, "vcs.worktree.create", json!({ "branch": "origin/fix" })).unwrap();
        assert_eq!((made["branch"].as_str(), made["path"].clone()), (Some("fix"), json!(base.join("app-fix"))));
        let again = run(&registry, &mut session, "vcs.worktree.create", json!({ "branch": "origin/fix" }));
        assert!(again.is_err());
        let other = json!({ "branch": "origin/fix", "newBranch": "fix-2", "path": base.join("other") });
        assert_eq!(run(&registry, &mut session, "vcs.worktree.create", other).unwrap()["branch"], "fix-2");
    }

    #[test]
    fn the_worktree_menu_opens_its_dialog_and_runs_its_items() {
        let (dir, root) = project();
        let base = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        let asked = run(&registry, s, "vcs.workspace.refAction", json!({ "name": "main", "action": "createWorktree" }))
            .unwrap();
        assert_eq!(asked["dialog"], CREATE_DIALOG);
        let prompt = s.state_mut::<GitWorkspace>().take_worktree_prompt();
        assert_eq!(
            prompt,
            Some(WorktreePrompt::Create { branch: RefTarget { name: "main".into(), kind: Default::default() } })
        );
        git(&root, &["tag", "v1"]);
        assert!(
            run(
                &registry,
                s,
                "vcs.workspace.refAction",
                json!({ "name": "v1", "kind": "tag", "action": "createWorktree" })
            )
            .is_err()
        );

        run(&registry, s, "vcs.worktree.create", json!({ "branch": "main", "newBranch": "side" })).unwrap();
        let side = base.join("app-side");
        let state = run(&registry, s, "vcs.workspace.worktreeMenu", json!({ "path": side })).unwrap();
        assert_eq!(state["worktreeMenu"], json!(side));
        let head = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
        assert_eq!(state["selected"], head.as_str(), "its HEAD is selected");
        let locked =
            run(&registry, s, "vcs.workspace.worktreeAction", json!({ "path": side, "action": "lock" })).unwrap();
        assert_eq!(locked["worktree"]["locked"], true);
        assert_eq!(run(&registry, s, "vcs.workspace.state", json!({})).unwrap()["worktreeMenu"], json!(null), "closed");
        run(&registry, s, "vcs.workspace.worktreeAction", json!({ "path": side, "action": "unlock" })).unwrap();
        let removing =
            run(&registry, s, "vcs.workspace.worktreeAction", json!({ "path": side, "action": "remove" })).unwrap();
        assert_eq!(removing["dialog"], REMOVE_DIALOG);
        assert!(
            matches!(s.state_mut::<GitWorkspace>().take_worktree_prompt(), Some(WorktreePrompt::Remove { worktree }) if worktree.path == side)
        );
        assert!(side.exists(), "only asked");
        assert!(
            run(&registry, s, "vcs.workspace.worktreeAction", json!({ "path": root, "action": "remove" })).is_err()
        );
        run(&registry, s, "vcs.workspace.worktreeAction", json!({ "path": side, "action": "open" })).unwrap();
        assert_eq!(session_take(s), Some(side));
        let closed = run(&registry, s, "vcs.workspace.worktreeMenu", json!({})).unwrap();
        assert_eq!(closed["worktreeMenu"], json!(null));
        assert!(run(&registry, s, "vcs.workspace.worktreeMenu", json!({ "path": base.join("nowhere") })).is_err());
    }

    #[test]
    fn submodules_are_listed_updated_initialised_and_opened() {
        let (dir, root) = project();
        let base = dir.path().canonicalize().unwrap();
        let library = base.join("library");
        std::fs::create_dir(&library).unwrap();
        git(&library, &["init", "-q", "-b", "main"]);
        std::fs::write(library.join("lib.txt"), "one\n").unwrap();
        git(&library, &["add", "lib.txt"]);
        git(&library, &["commit", "-q", "-m", "Library"]);
        std::fs::write(library.join("lib.txt"), "two\n").unwrap();
        git(&library, &["commit", "-q", "-am", "Library 2"]);
        let (registry, mut session) = super::super::tests::session(&root);
        let s = &mut session;
        assert_eq!(run(&registry, s, "vcs.submodule.list", json!({})).unwrap()["submodules"], json!([]));
        assert!(run(&registry, s, "vcs.submodule.update", json!({ "wait": true })).is_err(), "none");
        git(&root, &["submodule", "add", "-q", library.to_str().unwrap(), "libs/lib"]);
        git(&root, &["commit", "-q", "-m", "Library"]);
        let listed = run(&registry, s, "vcs.submodule.list", json!({})).unwrap();
        let lib = &listed["submodules"][0];
        assert_eq!(
            (lib["path"].as_str(), lib["status"].as_str(), lib["atRecorded"].as_bool()),
            (Some("libs/lib"), Some("current"), Some(true))
        );
        assert_eq!(lib["folder"], json!(root.join("libs/lib")));

        git(&root.join("libs/lib"), &["checkout", "-q", "HEAD~1"]);
        let moved = run(&registry, s, "vcs.submodule.list", json!({})).unwrap();
        assert_eq!(
            (moved["submodules"][0]["status"].as_str(), moved["submodules"][0]["atRecorded"].as_bool()),
            (Some("moved"), Some(false))
        );
        let menu = run(&registry, s, "vcs.workspace.submoduleMenu", json!({ "path": "libs/lib" })).unwrap();
        assert_eq!(menu["submoduleMenu"], "libs/lib");
        let updated = run(
            &registry,
            s,
            "vcs.workspace.submoduleAction",
            json!({ "path": "libs/lib", "action": "update", "wait": true }),
        )
        .unwrap();
        assert_eq!(updated["operation"]["succeeded"], true, "{updated}");
        assert_eq!(run(&registry, s, "vcs.submodule.list", json!({})).unwrap()["submodules"][0]["status"], "current");

        git(&root, &["submodule", "deinit", "-q", "libs/lib"]);
        assert_eq!(
            run(&registry, s, "vcs.submodule.list", json!({})).unwrap()["submodules"][0]["status"],
            "uninitialized"
        );
        assert!(run(&registry, s, "vcs.submodule.open", json!({ "path": "libs/lib" })).is_err());
        let init = json!({ "paths": [root.join("libs/lib")], "init": true, "wait": true });
        let done = run(&registry, s, "vcs.submodule.update", init).unwrap();
        assert_eq!(done["succeeded"], true, "{done}");
        assert_eq!(run(&registry, s, "vcs.submodule.list", json!({})).unwrap()["submodules"][0]["status"], "current");
        let opened =
            run(&registry, s, "vcs.workspace.submoduleAction", json!({ "path": "libs/lib", "action": "open" }))
                .unwrap();
        assert_eq!(opened["action"], "open");
        assert_eq!(session_take(s), Some(root.join("libs/lib")));
        assert!(run(&registry, s, "vcs.submodule.update", json!({ "paths": ["nothing"] })).is_err());
    }

    #[test]
    fn the_cached_submodule_list_comes_at_once_and_catches_up_behind() {
        let (dir, root) = project();
        let library = dir.path().canonicalize().unwrap().join("library");
        std::fs::create_dir(&library).unwrap();
        git(&library, &["init", "-q", "-b", "main"]);
        std::fs::write(library.join("lib.txt"), "one\n").unwrap();
        git(&library, &["add", "lib.txt"]);
        git(&library, &["commit", "-q", "-m", "Library"]);
        let (registry, mut session) = super::super::tests::session(&root);
        git(&root, &["submodule", "add", "-q", library.to_str().unwrap(), "lib"]);
        let first = run(&registry, &mut session, "vcs.submodule.list", json!({ "cached": true })).unwrap();
        assert_eq!((first["submodules"].as_array().map(Vec::len), first["revision"].as_u64()), (Some(0), Some(0)));
        run(&registry, &mut session, "vcs.submodule.list", json!({ "cached": true, "refresh": true })).unwrap();
        loop {
            let list = run(&registry, &mut session, "vcs.submodule.list", json!({ "cached": true })).unwrap();
            if list["revision"] == 1 {
                assert_eq!(list["submodules"][0]["path"], "lib");
                break;
            }
            std::thread::yield_now();
        }
    }
}
