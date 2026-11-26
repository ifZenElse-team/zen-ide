use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::{Move, RefState, Repository};

use super::operations::HeadMoved;
use super::{changed, message, repository};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListHistory)?;
    registry.register(UndoOperation)?;
    registry.register(RedoOperation)?;
    Ok(())
}

pub const LIMIT: usize = 100;

pub const MOVED: &str = "moved";

/// What an entry did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    /// A branch, a remote branch, a tag or a revision checked out.
    Checkout,
    /// A new commit.
    Commit,
    /// The last commit amended.
    Amend,
    /// The current branch (or a detached `HEAD`) reset to a commit.
    Reset,
    /// A branch created (and maybe checked out).
    BranchCreate,
    /// A local branch deleted.
    BranchDelete,
    /// A branch merged (fast-forward or a merge commit), without conflicts.
    Merge,
    /// A branch rebased, without conflicts.
    Rebase,
    /// A commit cherry-picked.
    CherryPick,
    /// A commit reverted.
    Revert,
    /// A branch fast-forwarded.
    FastForward,
    /// Unstaged changes discarded (whole files, or lines of one); unversioned files deleted.
    Discard,
    /// Changes rolled back to the last commit, staged ones too.
    Rollback,
    /// Changes shelved: kept as a patch in the shelf, then rolled back.
    Shelve,
    /// Shelved changes brought back to the working tree.
    Unshelve,
    /// A branch's commits rewritten by an interactive rebase (picked, reworded, squashed, dropped,
    /// reordered).
    InteractiveRebase,
    /// Several commits cherry-picked at once (the interactive rebase editor's cherry-pick).
    CherryPickMany,
    /// A patch applied to the working tree.
    ApplyPatch,
}

impl Operation {
    fn message_id(self) -> &'static str {
        match self {
            Operation::Checkout => "vcs-history-op-checkout",
            Operation::Commit => "vcs-history-op-commit",
            Operation::Amend => "vcs-history-op-amend",
            Operation::Reset => "vcs-history-op-reset",
            Operation::BranchCreate => "vcs-history-op-branch-create",
            Operation::BranchDelete => "vcs-history-op-branch-delete",
            Operation::Merge => "vcs-history-op-merge",
            Operation::Rebase => "vcs-history-op-rebase",
            Operation::CherryPick => "vcs-history-op-cherry-pick",
            Operation::Revert => "vcs-history-op-revert",
            Operation::FastForward => "vcs-history-op-fast-forward",
            Operation::Discard => "vcs-history-op-discard",
            Operation::Rollback => "vcs-history-op-rollback",
            Operation::Shelve => "vcs-history-op-shelve",
            Operation::Unshelve => "vcs-history-op-unshelve",
            Operation::InteractiveRebase => "vcs-history-op-interactive-rebase",
            Operation::CherryPickMany => "vcs-history-op-cherry-pick-many",
            Operation::ApplyPatch => "vcs-history-op-apply-patch",
        }
    }

    fn reflog_name(self) -> &'static str {
        match self {
            Operation::Checkout => "checkout",
            Operation::Commit => "commit",
            Operation::Amend => "amend",
            Operation::Reset => "reset",
            Operation::BranchCreate => "branch create",
            Operation::BranchDelete => "branch delete",
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
            Operation::FastForward => "fast-forward",
            Operation::Discard => "discard",
            Operation::Rollback => "rollback",
            Operation::Shelve => "shelve",
            Operation::Unshelve => "unshelve",
            Operation::InteractiveRebase => "interactive rebase",
            Operation::CherryPickMany => "cherry-pick",
            Operation::ApplyPatch => "apply patch",
        }
    }
}

/// How the branch `HEAD` stays on moves when an entry is undone or redone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MoveMode {
    /// The files and the index stay (a commit undone leaves its changes staged).
    Soft,
    /// The files stay, the index follows (a mixed reset).
    Mixed,
    /// The files follow, and nothing uncommitted is lost (git refuses otherwise).
    #[default]
    Keep,
}

impl From<MoveMode> for Move {
    fn from(mode: MoveMode) -> Self {
        match mode {
            MoveMode::Soft => Move::Soft,
            MoveMode::Mixed => Move::Mixed,
            MoveMode::Keep => Move::Keep,
        }
    }
}

/// Where `HEAD` and the local branches were.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Refs {
    /// The branch `HEAD` was on; `None` when detached.
    pub branch: Option<String>,
    /// The commit `HEAD` was at.
    pub head: Option<String>,
    /// Every local branch and its commit.
    pub branches: BTreeMap<String, String>,
    /// The upstreams of the local branches that had one.
    pub upstreams: BTreeMap<String, String>,
}

impl From<RefState> for Refs {
    fn from(state: RefState) -> Self {
        Refs { branch: state.branch, head: state.head, branches: state.branches, upstreams: state.upstreams }
    }
}

impl From<&Refs> for RefState {
    fn from(refs: &Refs) -> Self {
        RefState {
            branch: refs.branch.clone(),
            head: refs.head.clone(),
            branches: refs.branches.clone(),
            upstreams: refs.upstreams.clone(),
        }
    }
}

impl Refs {
    fn same(&self, other: &Refs) -> bool {
        self.branch == other.branch && self.head == other.head && self.branches == other.branches
    }
}

/// One side of a file: its git mode and its content's blob id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    /// The git mode (`33188` is `100644`).
    pub mode: u32,
    /// The content's blob id.
    pub id: String,
}

/// A file as the index and the working tree had it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileSides {
    /// What the index had; `None`: nothing.
    pub index: Option<Side>,
    /// What the working tree had; `None`: no file.
    pub worktree: Option<Side>,
}

/// A file an operation changed: before it, in the middle (after the files changed, before the
/// branches moved: a hard reset first throws the changes away, then moves), and after it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileStep {
    /// The file, relative to the repository's root, with `/`.
    pub path: String,
    /// Before the operation.
    pub before: FileSides,
    /// After its files changed, before `HEAD` moved.
    pub middle: FileSides,
    /// After the operation.
    pub after: FileSides,
}

fn sides(state: &zen_vcs::FileState) -> FileSides {
    let side = |blob: &Option<zen_vcs::Blob>| blob.as_ref().map(|blob| Side { mode: blob.mode, id: blob.id.clone() });
    FileSides { index: side(&state.index), worktree: side(&state.worktree) }
}

fn state(path: &str, sides: &FileSides) -> zen_vcs::FileState {
    let blob = |side: &Option<Side>| side.as_ref().map(|side| zen_vcs::Blob { mode: side.mode, id: side.id.clone() });
    zen_vcs::FileState { path: path.to_owned(), index: blob(&sides.index), worktree: blob(&sides.worktree) }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Entry {
    id: u64,
    operation: Option<Operation>,
    subject: String,
    target: Option<String>,
    count: usize,
    time: u64,
    before: Refs,
    after: Refs,
    mode: MoveMode,
    files: Vec<FileStep>,
    reflog: Vec<String>,
}

impl Entry {
    fn title(&self) -> String {
        let Some(operation) = self.operation else { return String::new() };
        let target = self.target.clone().unwrap_or_default();
        t!(operation.message_id(), subject = self.subject.as_str(), target = target.as_str(), count = self.count)
    }

    fn info(&self) -> HistoryEntry {
        HistoryEntry {
            id: self.id,
            operation: self.operation,
            title: self.title(),
            time: self.time,
            before: HeadAt { branch: self.before.branch.clone(), head: self.before.head.clone() },
            after: HeadAt { branch: self.after.branch.clone(), head: self.after.head.clone() },
            branches: changed_branches(&self.before, &self.after),
            files: self.files.iter().map(|file| file.path.clone()).collect(),
            reflog: self.reflog.clone(),
        }
    }

    fn contents(&self) -> impl Iterator<Item = &str> {
        self.files.iter().flat_map(|file| {
            [&file.before, &file.middle, &file.after]
                .into_iter()
                .flat_map(|sides| [&sides.index, &sides.worktree])
                .filter_map(|side| side.as_ref().map(|side| side.id.as_str()))
        })
    }
}

fn changed_branches(before: &Refs, after: &Refs) -> Vec<String> {
    let names: BTreeSet<&String> = before.branches.keys().chain(after.branches.keys()).collect();
    names.into_iter().filter(|name| before.branches.get(*name) != after.branches.get(*name)).cloned().collect()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Journal {
    root: PathBuf,
    next: u64,
    done: Vec<Entry>,
    undone: Vec<Entry>,
    pending: Option<Pending>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Pending {
    before: Refs,
    log_len: u64,
    operation: Option<Operation>,
    subject: String,
    target: Option<String>,
    count: usize,
    mode: MoveMode,
    stash: Option<String>,
    pop: bool,
    files: Vec<FileStep>,
}

impl Pending {
    fn contents(&self) -> impl Iterator<Item = &str> {
        self.files.iter().flat_map(|file| {
            [&file.before.index, &file.before.worktree].into_iter().flatten().map(|side| side.id.as_str())
        })
    }
}

#[derive(Default)]
pub struct GitHistory {
    folder: Option<PathBuf>,
    memory: HashMap<PathBuf, Journal>,
}

impl GitHistory {
    pub fn set_folder(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
    }

    fn folder_of(&self, root: &Path) -> PathBuf {
        let base = self.folder.clone().unwrap_or_else(|| std::env::temp_dir().join("zen-vcs-history"));
        base.join(key(root))
    }

    fn store(&self, root: &Path) -> PathBuf {
        self.folder_of(root).join("files")
    }

    fn journal(&self, root: &Path) -> Journal {
        let mut journal = match &self.folder {
            Some(_) => std::fs::read(self.folder_of(root).join("journal.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Journal>(&bytes).ok())
                .filter(|journal| journal.root == root)
                .unwrap_or_default(),
            None => self.memory.get(root).cloned().unwrap_or_default(),
        };
        journal.root = root.to_owned();
        journal
    }

    fn save(&mut self, journal: Journal) {
        let root = journal.root.clone();
        let folder = self.folder_of(&root);
        let mut kept: BTreeSet<&str> = journal.done.iter().chain(&journal.undone).flat_map(Entry::contents).collect();
        kept.extend(journal.pending.iter().flat_map(Pending::contents));
        if let Ok(files) = std::fs::read_dir(folder.join("files")) {
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                if !kept.contains(name.as_str()) {
                    let _ = std::fs::remove_file(file.path());
                }
            }
        }
        if self.folder.is_some() {
            let _ = std::fs::create_dir_all(&folder);
            if let Ok(bytes) = serde_json::to_vec_pretty(&journal) {
                let partial = folder.join("journal.json.partial");
                if std::fs::write(&partial, bytes).is_ok() {
                    let _ = std::fs::rename(&partial, folder.join("journal.json"));
                }
            }
        } else {
            self.memory.insert(root, journal);
        }
    }
}

pub(crate) fn key(root: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let name: String = root
        .file_name()
        .map(|name| {
            name.to_string_lossy().chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect()
        })
        .unwrap_or_default();
    format!("{name}-{hash:016x}")
}

pub(crate) struct Recording {
    before: RefState,
    log_len: u64,
    paths: Vec<String>,
    files: Vec<zen_vcs::FileState>,
    clean_middle: bool,
    store: PathBuf,
}

pub(crate) fn begin(session: &mut Session, repository: &Repository) -> Option<Recording> {
    begin_with_files(session, repository, Vec::new())
}

pub(crate) fn begin_with_files(
    session: &mut Session,
    repository: &Repository,
    paths: Vec<String>,
) -> Option<Recording> {
    if repository.in_progress().is_some() {
        return None;
    }
    let history = session.state_mut::<GitHistory>();
    let mut journal = history.journal(repository.root());
    if journal.pending.take().is_some() {
        history.save(journal);
    }
    let store = session.state_mut::<GitHistory>().store(repository.root());
    let before = repository.ref_state().ok()?;
    let files = repository.file_states(&paths, Some(&store)).ok()?;
    Some(Recording { before, log_len: repository.head_log_len(), paths, files, clean_middle: false, store })
}

impl Recording {
    pub(crate) fn cleaning(mut self) -> Self {
        self.clean_middle = true;
        self
    }

    pub(crate) fn nothing_changed(&self, repository: &Repository) -> bool {
        repository.file_states(&self.paths, Some(&self.store)).is_ok_and(|now| {
            now.len() == self.files.len() && now.iter().zip(&self.files).all(|(now, then)| now.same(then))
        })
    }
}

pub(crate) struct Done {
    pub operation: Operation,
    pub subject: String,
    pub target: Option<String>,
    pub count: usize,
    pub mode: MoveMode,
}

impl Done {
    pub(crate) fn new(operation: Operation, subject: impl Into<String>) -> Self {
        Done { operation, subject: subject.into(), target: None, count: 0, mode: MoveMode::Keep }
    }

    pub(crate) fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub(crate) fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    pub(crate) fn mode(mut self, mode: MoveMode) -> Self {
        self.mode = mode;
        self
    }
}

pub(crate) fn finish(session: &mut Session, repository: &Repository, recording: Option<Recording>, done: Done) {
    let Some(recording) = recording else { return };
    if repository.in_progress().is_some() {
        return;
    }
    let Ok(after) = repository.ref_state() else { return };
    let store = &recording.store;
    let Ok(files_after) = repository.file_states(&recording.paths, Some(store)) else { return };
    let middle = if recording.clean_middle {
        match repository.clean_file_states(recording.before.head.as_deref(), &recording.paths, Some(store)) {
            Ok(middle) => middle,
            Err(_) => return,
        }
    } else {
        files_after.clone()
    };
    let (before, after) = (Refs::from(recording.before), Refs::from(after));
    let files: Vec<FileStep> = recording
        .files
        .iter()
        .zip(&middle)
        .zip(&files_after)
        .filter(|((before, _), after)| !before.same(after))
        .map(|((before, middle), after)| FileStep {
            path: before.path.clone(),
            before: sides(before),
            middle: sides(middle),
            after: sides(after),
        })
        .collect();
    if before.same(&after) && files.is_empty() {
        return;
    }
    let reflog = repository.head_log_since(recording.log_len);
    let history = session.state_mut::<GitHistory>();
    let mut journal = history.journal(repository.root());
    journal.next += 1;
    let entry = Entry {
        id: journal.next,
        operation: Some(done.operation),
        subject: done.subject,
        target: done.target,
        count: done.count,
        time: crate::recent::now(),
        before,
        after,
        mode: done.mode,
        files,
        reflog,
    };
    journal.done.push(entry);
    journal.undone.clear();
    if journal.done.len() > LIMIT {
        let extra = journal.done.len() - LIMIT;
        journal.done.drain(..extra);
    }
    history.save(journal);
}

pub(crate) fn keep_pending(session: &mut Session, repository: &Repository, recording: Option<Recording>, done: Done) {
    let Some(recording) = recording.filter(|recording| recording.paths.is_empty()) else { return };
    if repository.in_progress().is_none() {
        return;
    }
    let history = session.state_mut::<GitHistory>();
    let mut journal = history.journal(repository.root());
    journal.pending = Some(Pending {
        before: Refs::from(recording.before),
        log_len: recording.log_len,
        operation: Some(done.operation),
        subject: done.subject,
        target: done.target,
        count: done.count,
        mode: done.mode,
        ..Pending::default()
    });
    history.save(journal);
}

pub(crate) fn finish_pending(session: &mut Session, repository: &Repository, aborted: bool) {
    if repository.in_progress().is_some() {
        return;
    }
    let history = session.state_mut::<GitHistory>();
    let mut journal = history.journal(repository.root());
    if journal.pending.as_ref().is_some_and(|pending| pending.stash.is_some()) {
        return;
    }
    let Some(pending) = journal.pending.take() else { return };
    let store = history.store(repository.root());
    history.save(journal);
    let Some(operation) = pending.operation.filter(|_| !aborted) else { return };
    let recording = Recording {
        before: RefState::from(&pending.before),
        log_len: pending.log_len,
        paths: Vec::new(),
        files: Vec::new(),
        clean_middle: false,
        store,
    };
    let done =
        Done { operation, subject: pending.subject, target: pending.target, count: pending.count, mode: pending.mode };
    finish(session, repository, Some(recording), done);
}

pub(crate) struct StashRecording {
    commit: String,
    files: Vec<zen_vcs::FileState>,
}

pub(crate) fn begin_stash(session: &mut Session, repository: &Repository, commit: &str) -> Option<StashRecording> {
    let paths = repository.stash_paths(commit).ok()?;
    let store = session.state_mut::<GitHistory>().store(repository.root());
    let files = repository.file_states(&paths, Some(&store)).ok()?;
    Some(StashRecording { commit: commit.to_owned(), files })
}

pub(crate) fn keep_stash(session: &mut Session, repository: &Repository, recording: StashRecording, pop: bool) {
    let history = session.state_mut::<GitHistory>();
    let mut journal = history.journal(repository.root());
    let files = recording
        .files
        .iter()
        .map(|state| FileStep { path: state.path.clone(), before: sides(state), ..FileStep::default() })
        .collect();
    journal.pending = Some(Pending { stash: Some(recording.commit), pop, files, ..Pending::default() });
    history.save(journal);
}

pub(crate) fn stash_pending(session: &mut Session, repository: &Repository) -> Option<(String, bool)> {
    if repository.in_progress().is_some() {
        return None;
    }
    let journal = session.state_mut::<GitHistory>().journal(repository.root());
    journal.pending.and_then(|pending| pending.stash.map(|stash| (stash, pending.pop)))
}

pub(crate) fn continue_stash(session: &mut Session, repository: &Repository) -> Result<(), zen_vcs::VcsError> {
    let Some((commit, pop)) = stash_pending(session, repository) else {
        return Err(zen_vcs::VcsError::Git(t!("vcs-operation-none")));
    };
    let files = repository.conflicted_files();
    if !files.is_empty() {
        let operation = if pop { zen_vcs::Applying::StashPop } else { zen_vcs::Applying::StashApply };
        return Err(zen_vcs::VcsError::Conflicts { operation, files });
    }
    if pop {
        repository.drop_stash_commit(&commit)?;
    }
    forget_pending(session, repository);
    Ok(())
}

pub(crate) fn abort_stash(session: &mut Session, repository: &Repository) -> Result<(), zen_vcs::VcsError> {
    if stash_pending(session, repository).is_none() {
        return Err(zen_vcs::VcsError::Git(t!("vcs-operation-none")));
    }
    let history = session.state_mut::<GitHistory>();
    let journal = history.journal(repository.root());
    let store = history.store(repository.root());
    let files = journal.pending.map(|pending| pending.files).unwrap_or_default();
    let states: Vec<zen_vcs::FileState> = files.iter().map(|file| state(&file.path, &file.before)).collect();
    let paths: Vec<String> = states.iter().map(|state| state.path.clone()).collect();
    let conflicted: Vec<String> =
        repository.conflicted_files().into_iter().filter(|path| paths.contains(path)).collect();
    repository.forget_in_index(&conflicted)?;
    repository.restore_files(&states, &store)?;
    forget_pending(session, repository);
    Ok(())
}

fn forget_pending(session: &mut Session, repository: &Repository) {
    let history = session.state_mut::<GitHistory>();
    let mut journal = history.journal(repository.root());
    if journal.pending.take().is_some() {
        history.save(journal);
    }
}

/// Where `HEAD` was.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HeadAt {
    /// The branch; `None` when detached.
    pub branch: Option<String>,
    /// The commit.
    pub head: Option<String>,
}

/// An operation of the journal, as commands report it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryEntry {
    /// Its number in the repository's journal.
    pub id: u64,
    /// What it did.
    pub operation: Option<Operation>,
    /// Its name, as the toolbar shows it: `Commit 'Fix login'`.
    pub title: String,
    /// When, in seconds since the Unix epoch.
    pub time: u64,
    /// Where `HEAD` was before it.
    pub before: HeadAt,
    /// Where `HEAD` was after it.
    pub after: HeadAt,
    /// The local branches it moved, created or deleted.
    pub branches: Vec<String>,
    /// The files it changed whose contents are kept (Discard, Rollback, a hard reset).
    pub files: Vec<String>,
    /// What git wrote to `HEAD`'s reflog for it.
    pub reflog: Vec<String>,
}

/// The journal of the project's repository.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryList {
    /// What Undo would undo: the newest operation done.
    pub undo: Option<HistoryEntry>,
    /// What Redo would redo: the newest operation undone.
    pub redo: Option<HistoryEntry>,
    /// The operations done, newest first.
    pub done: Vec<HistoryEntry>,
    /// The operations undone, the next to redo first.
    pub undone: Vec<HistoryEntry>,
}

pub struct ListHistory;

impl Command for ListHistory {
    type Params = NoParams;
    type Output = HistoryList;
    const ID: &'static str = "vcs.history.list";
    const TITLE: &'static str = "Git Operations History";
    const DESCRIPTION: &'static str = "Lists the Git operations done in the IDE on the project's repository that Undo and Redo \
         take back and do again (checkout, commit, amend, reset, branch create and delete, merge, \
         rebase, cherry-pick, revert, fast-forward, discard and rollback of changes): `undo` is the \
         newest done, `redo` the newest undone; each with its title, where HEAD was before and after, \
         the branches it moved, the files whose contents are kept and git's reflog lines for it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<HistoryList, CommandError> {
        let repository = repository(session)?;
        let journal = session.state_mut::<GitHistory>().journal(repository.root());
        let done: Vec<HistoryEntry> = journal.done.iter().rev().map(Entry::info).collect();
        let undone: Vec<HistoryEntry> = journal.undone.iter().rev().map(Entry::info).collect();
        Ok(HistoryList { undo: done.first().cloned(), redo: undone.first().cloned(), done, undone })
    }
}

/// Which operation to undo or redo.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StepParams {
    /// The operation expected (its `id` in vcs.history.list); refused when another one is next.
    /// The next one when absent.
    pub id: Option<u64>,
}

/// What Undo or Redo did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryStep {
    /// The operation undone or redone.
    pub entry: HistoryEntry,
    /// Where `HEAD` is now.
    pub head: HeadMoved,
}

pub struct UndoOperation;

impl Command for UndoOperation {
    type Params = StepParams;
    type Output = HistoryStep;
    const ID: &'static str = "vcs.history.undo";
    const TITLE: &'static str = "Undo Git Operation";
    const DESCRIPTION: &'static str = "Undoes the newest Git operation done in the IDE (vcs.history.list): HEAD and the branches \
         go back where they were (a commit undone leaves its changes staged), and discarded changes \
         come back from the copies kept. Refused with the code `moved`, changing nothing, when the \
         repository changed since (HEAD or a branch it moved is elsewhere, a file it changed was \
         changed again) or an operation is in progress; git refuses too when uncommitted changes \
         would be overwritten. With `id`, only that operation is undone.";

    fn run(&self, session: &mut Session, params: StepParams) -> Result<HistoryStep, CommandError> {
        step(session, params, true)
    }
}

pub struct RedoOperation;

impl Command for RedoOperation {
    type Params = StepParams;
    type Output = HistoryStep;
    const ID: &'static str = "vcs.history.redo";
    const TITLE: &'static str = "Redo Git Operation";
    const DESCRIPTION: &'static str = "Does again the newest Git operation undone (vcs.history.list): HEAD, the branches and the \
         files go where the operation had left them. Refused with the code `moved`, changing \
         nothing, when the repository changed since the undo; a new operation forgets what could be \
         redone. With `id`, only that operation is redone.";

    fn run(&self, session: &mut Session, params: StepParams) -> Result<HistoryStep, CommandError> {
        step(session, params, false)
    }
}

fn step(session: &mut Session, params: StepParams, undo: bool) -> Result<HistoryStep, CommandError> {
    let repository = repository(session)?;
    let root = repository.root().to_owned();
    let (mut journal, store) = {
        let history = session.state_mut::<GitHistory>();
        (history.journal(&root), history.store(&root))
    };
    let next = if undo { journal.done.last() } else { journal.undone.last() };
    let Some(entry) = next.cloned() else {
        return Err(CommandError::new(t!(if undo { "vcs-history-nothing-undo" } else { "vcs-history-nothing-redo" })));
    };
    let title = entry.title();
    if let Some(id) = params.id
        && id != entry.id
    {
        let key = if undo { "vcs-history-not-next-undo" } else { "vcs-history-not-next-redo" };
        return Err(CommandError::new(t!(key, operation = title.as_str(), id = id)));
    }
    let refused = |reason: String| {
        let key = if undo { "vcs-history-refused-undo" } else { "vcs-history-refused-redo" };
        CommandError::with_code(MOVED, t!(key, operation = title.as_str(), reason = reason))
    };
    if let Some(operation) = repository.in_progress() {
        return Err(refused(t!("vcs-history-reason-in-progress", operation = operation.as_str())));
    }
    let (from, to) = if undo { (&entry.after, &entry.before) } else { (&entry.before, &entry.after) };
    let now = Refs::from(repository.ref_state().map_err(|error| CommandError::new(message(&error)))?);
    if let Some(reason) = moved(from, to, &now) {
        return Err(refused(reason));
    }
    let paths: Vec<String> = entry.files.iter().map(|file| file.path.clone()).collect();
    let files_now = repository.file_states(&paths, None).map_err(|error| CommandError::new(message(&error)))?;
    for (file, now) in entry.files.iter().zip(&files_now) {
        let expected = state(&file.path, if undo { &file.after } else { &file.before });
        if !expected.same(now) {
            return Err(refused(t!("vcs-history-reason-file", file = file.path.as_str())));
        }
    }
    let operation = entry.operation.map_or("", Operation::reflog_name);
    let action = format!("{} {operation}: {}", if undo { "zen undo" } else { "zen redo" }, entry.subject);
    let (from_state, to_state) = (RefState::from(from), RefState::from(to));
    let result = if undo {
        repository.move_refs(&from_state, &to_state, entry.mode.into(), &action).and_then(|()| {
            let before: Vec<_> = entry.files.iter().map(|file| state(&file.path, &file.before)).collect();
            repository.restore_files(&before, &store)
        })
    } else {
        let middle: Vec<_> = entry.files.iter().map(|file| state(&file.path, &file.middle)).collect();
        repository
            .restore_files(&middle, &store)
            .and_then(|()| repository.move_refs(&from_state, &to_state, entry.mode.into(), &action))
    };
    changed(session);
    super::index::reload(session, &repository, &paths);
    if let Err(error) = result {
        let key = if undo { "vcs-history-failed-undo" } else { "vcs-history-failed-redo" };
        return Err(CommandError::new(t!(key, operation = title.as_str(), message = message(&error))));
    }
    if undo {
        journal.done.pop();
        journal.undone.push(entry.clone());
    } else {
        journal.undone.pop();
        journal.done.push(entry.clone());
    }
    session.state_mut::<GitHistory>().save(journal);
    let head = repository.head().ok();
    Ok(HistoryStep {
        entry: entry.info(),
        head: HeadMoved {
            head: head.as_ref().and_then(|head| head.commit.clone()).unwrap_or_default(),
            branch: head.and_then(|head| head.branch),
        },
    })
}

fn moved(expected: &Refs, target: &Refs, now: &Refs) -> Option<String> {
    let label = |refs: &Refs| refs.branch.clone().unwrap_or_else(|| short(refs.head.as_deref().unwrap_or_default()));
    if now.branch != expected.branch || (expected.branch.is_none() && now.head != expected.head) {
        return Some(t!("vcs-history-reason-head", now = label(now), expected = label(expected)));
    }
    let mut names = changed_branches(expected, target);
    names.extend(expected.branch.clone());
    for name in names {
        match (expected.branches.get(&name), now.branches.get(&name)) {
            (Some(was), Some(is)) if was != is => {
                return Some(t!(
                    "vcs-history-reason-branch",
                    branch = name.as_str(),
                    now = short(is),
                    expected = short(was)
                ));
            }
            (Some(_), None) => return Some(t!("vcs-history-reason-deleted", branch = name.as_str())),
            (None, Some(_)) => return Some(t!("vcs-history-reason-created", branch = name.as_str())),
            _ => {}
        }
    }
    None
}

fn short(id: &str) -> String {
    id.chars().take(7).collect()
}

#[cfg(test)]
mod tests;
