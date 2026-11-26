use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::merge::{ChunkKind, Merge, Pick, Taken};
use zen_vcs::{Applying, Repository};

use super::history;
use super::workspace::GitWorkspace;
use super::{CONFLICTS, changed, command_error, message, repository};
use crate::Workbench;

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListConflicts)?;
    registry.register(OpenConflict)?;
    registry.register(GetConflict)?;
    registry.register(ResolveConflict)?;
    registry.register(CloseConflict)?;
    registry.register(ContinueOperation)?;
    registry.register(AbortOperation)?;
    Ok(())
}

pub const UNRESOLVED: &str = "unresolved";

/// The operation a conflict comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    /// A branch merged into the current one.
    Merge,
    /// A branch rebased (also an interactive rebase, or several commits cherry-picked at once).
    Rebase,
    /// A commit cherry-picked.
    CherryPick,
    /// A commit reverted.
    Revert,
    /// A stash applied; it stays.
    StashApply,
    /// A stash popped; it is dropped once the conflicts are resolved and Continue is pressed.
    StashPop,
    /// Shelved changes brought back; git keeps nothing in progress for them, so only their
    /// conflicted files are listed.
    Unshelve,
    /// A patch applied with hunks left to merge; only its conflicted files are listed.
    Patch,
}

impl From<Applying> for OperationKind {
    fn from(operation: Applying) -> Self {
        match operation {
            Applying::Merge => OperationKind::Merge,
            Applying::Rebase => OperationKind::Rebase,
            Applying::CherryPick => OperationKind::CherryPick,
            Applying::Revert => OperationKind::Revert,
            Applying::StashApply => OperationKind::StashApply,
            Applying::StashPop => OperationKind::StashPop,
            Applying::Unshelve => OperationKind::Unshelve,
            Applying::Patch => OperationKind::Patch,
        }
    }
}

/// The operation in progress, as the Git workspace's banner shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InProgressInfo {
    /// What it is.
    pub kind: Option<OperationKind>,
    /// What the banner says: `Merging 'feature' into 'main'`.
    pub title: String,
    /// The branch the changes go onto (the one being rebased, for a rebase); `None` when detached.
    pub branch: Option<String>,
    /// Ours: what the left pane shows, named (`main`, `onto 1a2b3c4`).
    pub ours: String,
    /// Theirs: what the right pane shows, named (`feature`, `1a2b3c4 Fix login`, `stash@{0}`).
    pub theirs: String,
    /// The commit applied, as hex (merged in, picked, reverted, the one a rebase stopped at, the
    /// stash's).
    pub theirs_commit: Option<String>,
    /// For a rebase: the step it stopped at, from 1.
    pub step: usize,
    /// For a rebase: how many steps there are.
    pub total: usize,
    /// The files with unresolved conflicts, relative, with `/`.
    pub conflicts: Vec<String>,
    /// Whether Continue can go on now: nothing is conflicted.
    pub can_continue: bool,
}

/// How the two sides left a conflicted file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    /// Both changed it.
    #[default]
    BothModified,
    /// Both added it, differently.
    BothAdded,
    /// Ours deleted it; theirs changed it.
    DeletedByUs,
    /// Theirs deleted it; ours changed it.
    DeletedByThem,
    /// Only ours has it (theirs renamed or deleted it).
    AddedByUs,
    /// Only theirs has it.
    AddedByThem,
    /// Both deleted it.
    BothDeleted,
}

impl ConflictKind {
    fn of(conflict: &zen_vcs::Conflicted) -> Self {
        match (conflict.base.is_some(), conflict.ours.is_some(), conflict.theirs.is_some()) {
            (_, true, true) if conflict.base.is_none() => ConflictKind::BothAdded,
            (_, true, true) => ConflictKind::BothModified,
            (true, false, true) => ConflictKind::DeletedByUs,
            (true, true, false) => ConflictKind::DeletedByThem,
            (false, true, false) => ConflictKind::AddedByUs,
            (false, false, true) => ConflictKind::AddedByThem,
            _ => ConflictKind::BothDeleted,
        }
    }
}

/// A conflicted file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ConflictFileInfo {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the repository's root, with `/`.
    pub relative: String,
    /// How the sides left it.
    pub kind: ConflictKind,
    /// Whether it is the one the conflict editor has open.
    pub open: bool,
}

/// The conflicted files and the operation they stopped.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ConflictList {
    /// The operation in progress, if any (a stash that stopped on conflicts is one too).
    pub operation: Option<InProgressInfo>,
    /// The files with unresolved conflicts, by path.
    pub files: Vec<ConflictFileInfo>,
}

/// One version of a conflicted file, as a pane shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ConflictSide {
    /// What it is named: the branch, the commit, the stash.
    pub label: String,
    /// The commit it comes from, as hex.
    pub commit: Option<String>,
    /// Its text; `None` when this side has no such file (it deleted it) or it is not text.
    pub text: Option<String>,
    /// Whether this side has the file.
    pub exists: bool,
}

/// A change of a conflicted file: lines one side (or both) changed, and what the result has.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ConflictChange {
    /// Its number, from 0, as `change` of vcs.conflict.resolve names it.
    pub index: usize,
    /// How the sides changed it: `ours`, `theirs`, `both` (the same way) or `conflict`; `same` for
    /// unchanged lines typed over in the result.
    pub kind: String,
    /// What the result has: `auto` (the change of the side that made it), `unresolved` (a
    /// conflict: the base's lines), `ours`, `theirs`, `both` (ours, then theirs) or `edited`.
    pub state: String,
    /// Its lines in the base, from 0, the end excluded.
    pub base: [usize; 2],
    /// Its lines in ours.
    pub ours: [usize; 2],
    /// Its lines in theirs.
    pub theirs: [usize; 2],
    /// Its lines in the result.
    pub result: [usize; 2],
}

/// A conflicted file in the conflict editor: the three versions, the changes and the result.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ConflictView {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the repository's root, with `/`.
    pub relative: String,
    /// How the sides left it.
    pub kind: ConflictKind,
    /// The operation it comes from.
    pub operation: Option<OperationKind>,
    /// Whether its lines can be merged here: both sides have it and it is text. Otherwise only a
    /// whole side is taken (vcs.conflict.resolve with `take` alone).
    pub mergeable: bool,
    /// The common ancestor's text (empty when the sides added the file).
    pub base: Option<String>,
    /// Ours: the left pane.
    pub ours: ConflictSide,
    /// Theirs: the right pane.
    pub theirs: ConflictSide,
    /// The result (the middle pane), as it is now.
    pub result: Option<String>,
    /// The changes, in order; the lines between two changes are the same in all four texts.
    pub changes: Vec<ConflictChange>,
    /// How many of the changes are conflicts.
    pub conflicts: usize,
    /// How many conflicts are not resolved yet.
    pub unresolved: usize,
    /// Whether the result differs from what merging gave at first.
    pub edited: bool,
    /// Whether the conflict editor has this file open.
    pub open: bool,
    /// Whether the file was marked resolved by this command (it is staged and no longer
    /// conflicted).
    pub resolved: bool,
    /// How many conflicted files are left in the repository.
    pub remaining: usize,
}

#[derive(Clone, Debug)]
struct Loaded {
    relative: String,
    path: PathBuf,
    kind: ConflictKind,
    operation: Option<OperationKind>,
    ours: ConflictSide,
    theirs: ConflictSide,
    base: Option<String>,
    merge: Option<Merge>,
    first: String,
}

impl Loaded {
    fn view(&self, open: bool, remaining: usize) -> ConflictView {
        let mut view = ConflictView {
            path: self.path.clone(),
            relative: self.relative.clone(),
            kind: self.kind,
            operation: self.operation,
            mergeable: self.merge.is_some(),
            base: self.base.clone(),
            ours: self.ours.clone(),
            theirs: self.theirs.clone(),
            open,
            remaining,
            ..ConflictView::default()
        };
        if let Some(merge) = &self.merge {
            let result = merge.result();
            view.edited = result != self.first;
            view.result = Some(result);
            let ranges = merge.result_ranges();
            for (index, piece) in changes(merge) {
                let chunk = &piece.chunk;
                let range = &ranges[index];
                view.changes.push(ConflictChange {
                    index: view.changes.len(),
                    kind: chunk.kind.as_str().to_owned(),
                    state: piece.taken.as_str().to_owned(),
                    base: [chunk.base.start, chunk.base.end],
                    ours: [chunk.ours.start, chunk.ours.end],
                    theirs: [chunk.theirs.start, chunk.theirs.end],
                    result: [range.start, range.end],
                });
            }
            view.conflicts = merge.pieces().iter().filter(|piece| piece.chunk.kind == ChunkKind::Conflict).count();
            view.unresolved = merge.unresolved();
        }
        view
    }

    fn piece_of(&self, change: usize) -> Result<usize, CommandError> {
        let merge = self.merge.as_ref().ok_or_else(|| CommandError::new(t!("vcs-conflict-error-not-mergeable")))?;
        changes(merge)
            .nth(change)
            .map(|(index, _)| index)
            .ok_or_else(|| CommandError::new(t!("vcs-conflict-error-no-change", change = change)))
    }
}

fn changes(merge: &Merge) -> impl Iterator<Item = (usize, &zen_vcs::merge::Piece)> {
    merge
        .pieces()
        .iter()
        .enumerate()
        .filter(|(_, piece)| piece.chunk.kind != ChunkKind::Same || piece.taken != Taken::Auto)
}

#[derive(Default)]
pub struct ConflictEditor {
    file: Option<Loaded>,
    raised: u64,
    revision: u64,
}

impl ConflictEditor {
    pub fn is_open(&self) -> bool {
        self.file.is_some()
    }

    pub fn relative(&self) -> Option<&str> {
        self.file.as_ref().map(|file| file.relative.as_str())
    }

    pub fn raised(&self) -> u64 {
        self.raised
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn close(&mut self) {
        self.file = None;
        self.revision += 1;
    }
}

fn in_progress(session: &mut Session, repository: &Repository) -> Option<InProgressInfo> {
    let conflicts = repository.conflicted_files();
    let short = |id: &str| id.chars().take(7).collect::<String>();
    let subject = |id: &str| repository.commit_details(id).map(|(commit, _)| commit.subject).unwrap_or_default();
    let head = repository.head().ok();
    let current = head.as_ref().map(|head| head.label()).unwrap_or_else(|| "HEAD".to_owned());
    let branch = head.as_ref().and_then(|head| head.branch.clone());
    let mut info = InProgressInfo { branch: branch.clone(), ours: current.clone(), ..InProgressInfo::default() };
    if let Some(found) = repository.in_progress_info() {
        let kind = OperationKind::from(found.operation);
        info.kind = Some(kind);
        info.theirs_commit = found.theirs.clone();
        let named =
            |id: &Option<String>| id.as_deref().map(|id| format!("{} {}", short(id), subject(id))).unwrap_or_default();
        match kind {
            OperationKind::Merge => {
                info.theirs =
                    found.theirs_name.clone().or_else(|| found.theirs.as_deref().map(short)).unwrap_or_default();
                info.title = t!("vcs-operation-merge", theirs = info.theirs.as_str(), ours = current.as_str());
            }
            OperationKind::Rebase => {
                let progress = repository.rebase_progress().unwrap_or_default();
                let onto = progress.onto.as_deref().map(short).unwrap_or_default();
                info.branch = progress.branch.clone();
                info.ours = t!("vcs-conflict-ours-rebase", onto = onto.as_str());
                info.theirs = named(&found.theirs);
                info.step = progress.step;
                info.total = progress.total;
                let rebased = progress.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
                info.title = t!(
                    "vcs-operation-rebase",
                    branch = rebased.as_str(),
                    onto = onto.as_str(),
                    step = progress.step,
                    total = progress.total
                );
            }
            OperationKind::CherryPick => {
                info.theirs = named(&found.theirs);
                info.title = t!("vcs-operation-cherry-pick", commit = info.theirs.as_str(), ours = current.as_str());
            }
            OperationKind::Revert => {
                info.theirs = named(&found.theirs);
                info.title = t!("vcs-operation-revert", commit = info.theirs.as_str(), ours = current.as_str());
            }
            OperationKind::StashApply | OperationKind::StashPop | OperationKind::Unshelve | OperationKind::Patch => {}
        }
    } else if let Some((commit, pop)) = history::stash_pending(session, repository) {
        let kind = if pop { OperationKind::StashPop } else { OperationKind::StashApply };
        info.kind = Some(kind);
        let stashes = repository.stashes().unwrap_or_default();
        info.theirs = stashes
            .iter()
            .find(|stash| stash.commit == commit)
            .map(|stash| format!("stash@{{{}}}", stash.index))
            .unwrap_or_else(|| short(&commit));
        info.theirs_commit = Some(commit);
        let key = if pop { "vcs-operation-stash-pop" } else { "vcs-operation-stash-apply" };
        info.title = t!(key, stash = info.theirs.as_str(), ours = current.as_str());
    } else {
        return None;
    }
    info.can_continue = conflicts.is_empty();
    info.conflicts = conflicts;
    Some(info)
}

fn load(session: &mut Session, repository: &Repository, relative: &str) -> Result<Loaded, CommandError> {
    let conflicts = repository.conflicts().map_err(|error| command_error(&error))?;
    let Some(conflict) = conflicts.into_iter().find(|conflict| conflict.path == relative) else {
        return Err(CommandError::new(t!("vcs-conflict-error-not-conflicted", file = relative)));
    };
    let operation = in_progress(session, repository);
    let read = |blob: &Option<zen_vcs::Blob>| -> Result<Option<Vec<u8>>, CommandError> {
        match blob {
            Some(blob) => repository.blob(&blob.id).map(Some).map_err(|error| command_error(&error)),
            None => Ok(None),
        }
    };
    let text = |blob: &Option<zen_vcs::Blob>, data: &Option<Vec<u8>>| -> Option<String> {
        let regular = blob.as_ref().is_some_and(|blob| blob.mode & 0o170000 == 0o100000);
        let data = data.as_ref()?;
        (regular && !data.contains(&0)).then(|| String::from_utf8(data.clone()).ok()).flatten()
    };
    let (base_data, ours_data, theirs_data) = (read(&conflict.base)?, read(&conflict.ours)?, read(&conflict.theirs)?);
    let (base, ours, theirs) =
        (text(&conflict.base, &base_data), text(&conflict.ours, &ours_data), text(&conflict.theirs, &theirs_data));
    let mergeable = ours.is_some() && theirs.is_some() && (conflict.base.is_none() || base.is_some());
    let merge = mergeable.then(|| {
        Merge::new(
            base.as_deref().unwrap_or_default(),
            ours.as_deref().unwrap_or_default(),
            theirs.as_deref().unwrap_or_default(),
        )
    });
    let first = merge.as_ref().map(Merge::result).unwrap_or_default();
    let info = operation.clone().unwrap_or_default();
    let head = repository.head().ok().and_then(|head| head.commit);
    Ok(Loaded {
        relative: relative.to_owned(),
        path: super::absolute(repository.root(), relative),
        kind: ConflictKind::of(&conflict),
        operation: operation.and_then(|operation| operation.kind),
        ours: ConflictSide { label: info.ours.clone(), commit: head, text: ours, exists: conflict.ours.is_some() },
        theirs: ConflictSide {
            label: info.theirs.clone(),
            commit: info.theirs_commit.clone(),
            text: theirs,
            exists: conflict.theirs.is_some(),
        },
        base: base.or_else(|| conflict.base.is_none().then(String::new)),
        merge,
        first,
    })
}

fn target(session: &mut Session, repository: &Repository, path: Option<&str>) -> Result<String, CommandError> {
    match path.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => super::commit::relative(repository, std::path::Path::new(path)),
        None => session
            .state::<ConflictEditor>()
            .and_then(ConflictEditor::relative)
            .map(str::to_owned)
            .ok_or_else(|| CommandError::new(t!("vcs-conflict-error-closed"))),
    }
}

pub struct ListConflicts;

impl Command for ListConflicts {
    type Params = NoParams;
    type Output = ConflictList;
    const ID: &'static str = "vcs.conflict.list";
    const TITLE: &'static str = "Merge Conflicts";
    const DESCRIPTION: &'static str = "Lists the files with unresolved conflicts (`kind`: bothModified, bothAdded, deletedByUs, \
         deletedByThem, addedByUs, addedByThem, bothDeleted) and the operation in progress, if any: a \
         merge, a rebase, a cherry-pick, a revert, or a stash applied or popped that stopped on \
         conflicts, with what the Git workspace's banner says, ours and theirs, the rebase's step and \
         whether vcs.operation.continue can go on now. vcs.conflict.open opens a file in the conflict \
         editor.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ConflictList, CommandError> {
        let repository = repository(session)?;
        let open = session.state::<ConflictEditor>().and_then(ConflictEditor::relative).map(str::to_owned);
        let files = repository
            .conflicts()
            .map_err(|error| command_error(&error))?
            .iter()
            .map(|conflict| ConflictFileInfo {
                path: super::absolute(repository.root(), &conflict.path),
                relative: conflict.path.clone(),
                kind: ConflictKind::of(conflict),
                open: open.as_deref() == Some(conflict.path.as_str()),
            })
            .collect();
        Ok(ConflictList { operation: in_progress(session, &repository), files })
    }
}

/// Which conflicted file.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ConflictParams {
    /// The file: relative to the repository's root, or absolute. vcs.conflict.open takes the first
    /// conflicted file without it; vcs.conflict.get the one open in the editor.
    pub path: Option<String>,
}

pub struct OpenConflict;

impl Command for OpenConflict {
    type Params = ConflictParams;
    type Output = ConflictView;
    const ID: &'static str = "vcs.conflict.open";
    const TITLE: &'static str = "Resolve Conflicts";
    const DESCRIPTION: &'static str = "Opens a conflicted file (`path`; the first one when absent) in the conflict editor tab: \
         ours on the left, the result in the middle, theirs on the right, every change of either side \
         lined up. Answers with the file as vcs.conflict.get does. Nothing is written until \
         vcs.conflict.resolve marks it resolved.";

    fn run(&self, session: &mut Session, params: ConflictParams) -> Result<ConflictView, CommandError> {
        let repository = repository(session)?;
        let relative = match params.path.as_deref().map(str::trim).filter(|path| !path.is_empty()) {
            Some(path) => super::commit::relative(&repository, std::path::Path::new(path))?,
            None => repository
                .conflicted_files()
                .into_iter()
                .next()
                .ok_or_else(|| CommandError::new(t!("vcs-conflict-error-none")))?,
        };
        let loaded = load(session, &repository, &relative)?;
        let remaining = repository.conflicted_files().len();
        let view = loaded.view(true, remaining);
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        let editor = session.state_mut::<ConflictEditor>();
        editor.file = Some(loaded);
        editor.raised += 1;
        editor.revision += 1;
        Ok(view)
    }
}

pub struct GetConflict;

impl Command for GetConflict {
    type Params = ConflictParams;
    type Output = ConflictView;
    const ID: &'static str = "vcs.conflict.get";
    const TITLE: &'static str = "Merge Conflict";
    const DESCRIPTION: &'static str = "A conflicted file (`path`; the one open in the conflict editor when absent): the base, ours \
         and theirs as text with their names, the result as it is now (with what was taken and typed \
         in the editor, when it is the open file), and every change with its lines in the four texts \
         (`kind`: ours, theirs, both, conflict; `state`: auto, unresolved, ours, theirs, both, \
         edited). The lines between two changes are the same everywhere. `mergeable` is false for a \
         binary file or one a side deleted: only a whole side can be taken then.";

    fn run(&self, session: &mut Session, params: ConflictParams) -> Result<ConflictView, CommandError> {
        let repository = repository(session)?;
        let relative = target(session, &repository, params.path.as_deref())?;
        let remaining = repository.conflicted_files().len();
        if let Some(file) = session.state::<ConflictEditor>().and_then(|editor| editor.file.as_ref())
            && file.relative == relative
        {
            return Ok(file.view(true, remaining));
        }
        Ok(load(session, &repository, &relative)?.view(false, remaining))
    }
}

/// What to take for a change, or for the whole file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Take {
    /// Ours (the left pane).
    Ours,
    /// Theirs (the right pane).
    Theirs,
    /// Ours, then theirs (for changes only).
    Both,
}

impl From<Take> for Pick {
    fn from(take: Take) -> Self {
        match take {
            Take::Ours => Pick::Ours,
            Take::Theirs => Pick::Theirs,
            Take::Both => Pick::Both,
        }
    }
}

/// What to do with a conflicted file.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResolveParams {
    /// The file (relative or absolute); the one open in the conflict editor when absent.
    pub path: Option<String>,
    /// A side to take: for the change `change`, the changes `changes`, or every unresolved
    /// conflict with `conflicts`; with none of them, for the whole file, which is then resolved at
    /// once (ours or theirs as it is, git checkout --ours/--theirs: a side that deleted the file
    /// deletes it).
    pub take: Option<Take>,
    /// The change (its `index` in vcs.conflict.get).
    pub change: Option<usize>,
    /// Several changes.
    pub changes: Vec<usize>,
    /// Every conflict not resolved yet.
    pub conflicts: bool,
    /// New text for the result: the whole result, or the lines of the change `change` (typed in
    /// the middle pane).
    pub text: Option<String>,
    /// Write the result to the file and stage it, which marks it resolved (after what the other
    /// parameters did). Refused with the code `unresolved` while conflicts are left in it, unless
    /// `force`.
    pub mark_resolved: bool,
    /// Mark it resolved even with conflicts left (they keep the base's lines).
    pub force: bool,
}

pub struct ResolveConflict;

impl Command for ResolveConflict {
    type Params = ResolveParams;
    type Output = ConflictView;
    const ID: &'static str = "vcs.conflict.resolve";
    const TITLE: &'static str = "Resolve Conflict";
    const DESCRIPTION: &'static str = "Resolves a conflicted file (`path`; the one open in the conflict editor when absent). `take` \
         (ours, theirs, both) with `change`, `changes` or `conflicts: true` takes that side for those \
         changes of the result, as the » and « buttons of the editor do (both: ours, then theirs); \
         `take` alone takes ours or theirs for the whole file and resolves it at once. `text` replaces \
         the whole result, or with `change` that change's lines, as typing in the middle pane does. \
         `markResolved` then writes the result and stages the file (refused with the code \
         `unresolved` while conflicts are left, unless `force`); the editor closes. Answers with the \
         file: its changes, the result, and `remaining`, the conflicted files left (none: \
         vcs.operation.continue goes on).";

    fn run(&self, session: &mut Session, params: ResolveParams) -> Result<ConflictView, CommandError> {
        let repository = repository(session)?;
        let relative = target(session, &repository, params.path.as_deref())?;
        let is_open = session
            .state::<ConflictEditor>()
            .and_then(|editor| editor.file.as_ref())
            .is_some_and(|file| file.relative == relative);
        let mut file = match session.state::<ConflictEditor>().and_then(|editor| editor.file.clone()) {
            Some(file) if is_open => file,
            _ => load(session, &repository, &relative)?,
        };
        let picked: Vec<usize> = params.change.into_iter().chain(params.changes.iter().copied()).collect();
        let whole = params.take.is_some() && picked.is_empty() && !params.conflicts;
        if whole {
            let side = match params.take {
                Some(Take::Ours) => true,
                Some(Take::Theirs) => false,
                _ => return Err(CommandError::new(t!("vcs-conflict-error-both-file"))),
            };
            repository.resolve_with_side(&relative, side).map_err(|error| command_error(&error))?;
            return Ok(resolved(session, &repository, file));
        }
        if let Some(take) = params.take {
            let pieces = if params.conflicts {
                let merge =
                    file.merge.as_ref().ok_or_else(|| CommandError::new(t!("vcs-conflict-error-not-mergeable")))?;
                merge
                    .pieces()
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.taken == Taken::Unresolved)
                    .map(|(i, _)| i)
                    .collect()
            } else {
                picked.iter().map(|change| file.piece_of(*change)).collect::<Result<Vec<_>, _>>()?
            };
            if let Some(merge) = &mut file.merge {
                for piece in pieces {
                    merge.take(piece, take.into());
                }
            }
        }
        if let Some(text) = &params.text {
            let piece = match params.change.filter(|_| params.take.is_none()) {
                Some(change) => Some(file.piece_of(change)?),
                None => None,
            };
            let merge = file.merge.as_mut().ok_or_else(|| CommandError::new(t!("vcs-conflict-error-not-mergeable")))?;
            match piece {
                Some(piece) => {
                    merge.set_piece_text(piece, text);
                }
                None => merge.set_result(text),
            }
        }
        if params.mark_resolved {
            let Some(merge) = &file.merge else {
                return Err(CommandError::new(t!("vcs-conflict-error-not-mergeable")));
            };
            let left = merge.unresolved();
            if left > 0 && !params.force {
                return Err(CommandError::with_code(UNRESOLVED, t!("vcs-conflict-error-unresolved", count = left)));
            }
            repository.resolve_with(&relative, merge.result().as_bytes()).map_err(|error| command_error(&error))?;
            return Ok(resolved(session, &repository, file));
        }
        let remaining = repository.conflicted_files().len();
        let view = file.view(is_open, remaining);
        if is_open {
            let editor = session.state_mut::<ConflictEditor>();
            editor.file = Some(file);
            editor.revision += 1;
        }
        Ok(view)
    }
}

fn resolved(session: &mut Session, repository: &Repository, file: Loaded) -> ConflictView {
    let remaining = repository.conflicted_files().len();
    let mut view = file.view(false, remaining);
    view.resolved = true;
    let editor = session.state_mut::<ConflictEditor>();
    if editor.relative() == Some(file.relative.as_str()) {
        editor.close();
    }
    reload(session, repository, &[file.relative]);
    changed(session);
    session.state_mut::<GitWorkspace>().select_wip();
    view
}

fn reload(session: &mut Session, repository: &Repository, paths: &[String]) {
    let workbench = session.state_mut::<Workbench>();
    for document in &mut workbench.documents {
        let listed = super::commit::relative(repository, document.path())
            .is_ok_and(|path| paths.is_empty() || paths.contains(&path));
        if listed && !document.is_dirty() && document.image().is_none() {
            document.reload_from_disk();
        }
    }
}

pub struct CloseConflict;

impl Command for CloseConflict {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "vcs.conflict.close";
    const TITLE: &'static str = "Close Conflict Editor";
    const DESCRIPTION: &'static str = "Closes the conflict editor tab without writing anything: what was taken and typed in it \
         is let go, and the file stays conflicted.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        session.state_mut::<ConflictEditor>().close();
        Ok(())
    }
}

/// What Continue or Abort did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OperationOutcome {
    /// The operation it was.
    pub operation: Option<OperationKind>,
    /// The commit HEAD is at now.
    pub head: String,
    /// The branch HEAD is on; `None` when detached.
    pub branch: Option<String>,
}

pub struct ContinueOperation;

impl Command for ContinueOperation {
    type Params = NoParams;
    type Output = OperationOutcome;
    const ID: &'static str = "vcs.operation.continue";
    const TITLE: &'static str = "Continue Git Operation";
    const DESCRIPTION: &'static str = "Goes on with the operation in progress once no file is conflicted: commits the merge, the \
         cherry-pick or the revert with git's message (one resolved to no change is skipped), \
         continues the rebase (an interactive one keeps its reworded messages), or ends a stash that \
         stopped on conflicts (a popped one is dropped now). Fails with the code `conflicts` while \
         files are conflicted or when the next step of a rebase conflicts. Once it is over, the new \
         HEAD is selected in the Git workspace and Undo takes the whole operation back.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<OperationOutcome, CommandError> {
        let repository = repository(session)?;
        let operation = in_progress(session, &repository).and_then(|info| info.kind);
        let result = match operation {
            Some(OperationKind::StashApply | OperationKind::StashPop) => {
                history::continue_stash(session, &repository).map(|()| String::new())
            }
            Some(_) => repository.continue_operation(),
            None => return Err(CommandError::new(t!("vcs-operation-none"))),
        };
        if result.is_ok() {
            history::finish_pending(session, &repository, false);
        }
        outcome(session, &repository, operation, result)
    }
}

pub struct AbortOperation;

impl Command for AbortOperation {
    type Params = NoParams;
    type Output = OperationOutcome;
    const ID: &'static str = "vcs.operation.abort";
    const TITLE: &'static str = "Abort Git Operation";
    const DESCRIPTION: &'static str = "Aborts the operation in progress: a merge, a cherry-pick, a revert or a rebase puts the \
         branch, the index and the files back as they were before it started (git merge --abort and \
         the like); a stash that stopped on conflicts puts back the files it touched and is kept. \
         Answers with HEAD.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<OperationOutcome, CommandError> {
        let repository = repository(session)?;
        let operation = in_progress(session, &repository).and_then(|info| info.kind);
        let result = match operation {
            Some(OperationKind::StashApply | OperationKind::StashPop) => history::abort_stash(session, &repository),
            Some(_) => repository.abort_operation(),
            None => return Err(CommandError::new(t!("vcs-operation-none"))),
        };
        if result.is_ok() {
            history::finish_pending(session, &repository, true);
        }
        outcome(session, &repository, operation, result.map(|()| String::new()))
    }
}

fn outcome(
    session: &mut Session,
    repository: &Repository,
    operation: Option<OperationKind>,
    result: Result<String, zen_vcs::VcsError>,
) -> Result<OperationOutcome, CommandError> {
    if repository.conflicted_files().is_empty() {
        session.state_mut::<ConflictEditor>().close();
    }
    reload(session, repository, &[]);
    changed(session);
    match result {
        Ok(_) => {
            let head = repository.head().map_err(|error| command_error(&error))?;
            let id = head.commit.clone().unwrap_or_default();
            session.state_mut::<GitWorkspace>().select_commit(id.clone());
            Ok(OperationOutcome { operation, head: id, branch: head.branch })
        }
        Err(error) => {
            let error = match &error {
                zen_vcs::VcsError::Conflicts { .. } => CommandError::with_code(CONFLICTS, message(&error)),
                _ => command_error(&error),
            };
            if error.code.as_deref() == Some(CONFLICTS) {
                session.state_mut::<GitWorkspace>().select_wip();
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
