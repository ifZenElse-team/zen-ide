use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::Repository;
use zen_vcs::diff::{self, Hunk};

use super::background::{self, Step};
use super::history::{self, Done, MoveMode, Operation};
use super::{changed, message, repository};
use crate::compare::Whitespace;
use crate::{Document, Workbench};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CreateCommit)?;
    registry.register(LastMessage)?;
    registry.register(Rollback)?;
    registry.register(FileDiff)?;
    registry.register(LineStatus)?;
    registry.register(RollbackLines)?;
    Ok(())
}

pub(super) fn relative(repository: &Repository, path: &Path) -> Result<String, CommandError> {
    let root = repository.root();
    let path = if path.is_absolute() { path.to_owned() } else { root.join(path) };
    let outside = || CommandError::new(t!("vcs-error-outside", path = path.display().to_string()));
    let real = real_folder_and_name(&path).ok_or_else(outside)?;
    let real_root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
    match real.strip_prefix(&real_root).or_else(|_| real.strip_prefix(root)) {
        Ok(inside) if !inside.as_os_str().is_empty() => Ok(inside
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")),
        _ => Err(outside()),
    }
}

fn real_folder_and_name(path: &Path) -> Option<PathBuf> {
    let mut missing = vec![path.file_name()?];
    let mut folder = path.parent()?;
    let real = loop {
        match folder.canonicalize() {
            Ok(real) => break real,
            Err(_) => {
                missing.push(folder.file_name()?);
                folder = folder.parent()?;
            }
        }
    };
    Some(missing.into_iter().rev().fold(real, |real, part| real.join(part)))
}

/// What to commit.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitParams {
    /// The commit message.
    pub message: String,
    /// The changed files to commit (absolute or relative to the repository); new files are added.
    /// Other changes stay uncommitted.
    pub paths: Vec<PathBuf>,
    /// Amend the last commit instead of making a new one.
    pub amend: bool,
    /// Push after committing (Commit and Push…).
    pub push: bool,
    /// With `push`, wait until the push is over and answer with its outcome (`push` in the answer).
    pub wait: bool,
    /// Commit what is staged, and only that (the Git workspace's Commit); `paths` are not taken
    /// then. Fails when nothing is staged, unless amending.
    pub staged: bool,
    /// Commit the files of this changelist (vcs.changelist.list) when no `paths` are given: one
    /// list committed, the others left as they are.
    pub changelist: Option<String>,
    /// The commit's author, `Name <email>` (a patch's author, vcs.commit.draft); the configured
    /// one when absent.
    pub author: Option<String>,
    /// Sign-off commit: adds `Signed-off-by:` with the committer's name and e-mail (git's
    /// `user.name` and `user.email`) at the end of the message, unless it is there already.
    pub sign_off: bool,
}

fn signed_off(message: &str, user: (Option<String>, Option<String>)) -> Result<String, CommandError> {
    let (Some(name), Some(email)) = user else {
        return Err(CommandError::new(t!("vcs-error-sign-off-identity")));
    };
    let line = format!("Signed-off-by: {name} <{email}>");
    if message.lines().any(|existing| existing.trim() == line) {
        return Ok(message.to_owned());
    }
    let last = message.lines().last().unwrap_or_default();
    let after_trailer = message.lines().count() > 1
        && last.split_once(": ").is_some_and(|(key, _)| !key.is_empty() && !key.contains(' '));
    Ok(if after_trailer { format!("{message}\n{line}") } else { format!("{message}\n\n{line}") })
}

/// What a commit made.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitResult {
    /// The new commit's id.
    pub commit: String,
    /// What to tell the user: `1 file committed: Fix the build`.
    pub message: String,
    /// With `push`, the push: its outcome with `wait`, otherwise how it started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push: Option<super::OperationInfo>,
}

pub struct CreateCommit;

impl Command for CreateCommit {
    type Params = CommitParams;
    type Output = CommitResult;
    const ID: &'static str = "vcs.commit.create";
    const TITLE: &'static str = "Commit…";
    const DESCRIPTION: &'static str = "Commits the changes of the given files with a message (adding new files first), or amends \
         the last commit; with `changelist` and no `paths`, commits the files of that changelist; \
         with `staged`, commits exactly what is staged (vcs.index.stage) and leaves \
         the other changes in the working tree; with `author` (`Name <email>`), as that author; with \
         `push`, pushes afterwards in the background (vcs.operation.status); with `signOff`, adds the \
         committer's Signed-off-by line.";

    fn run(&self, session: &mut Session, mut params: CommitParams) -> Result<CommitResult, CommandError> {
        if params.paths.is_empty()
            && !params.staged
            && let Some(list) = params.changelist.clone().filter(|name| !name.trim().is_empty())
        {
            let files = super::changelists::files_of(session, list.trim())?;
            let root = repository(session)?.root().to_owned();
            let (kept, _) = crate::hidden_files::without_hidden(session, &root, files);
            params.paths = kept.into_iter().map(PathBuf::from).collect();
        }
        let mut summary = params.message.trim().to_owned();
        if summary.is_empty() {
            return Err(CommandError::new(t!("vcs-error-empty-message")));
        }
        if params.sign_off {
            summary = signed_off(&summary, repository(session)?.user())?;
        }
        let summary = summary.as_str();
        if params.staged {
            return commit_staged(session, summary, &params);
        }
        if params.paths.is_empty() && !params.amend {
            return Err(CommandError::new(t!("vcs-error-nothing-to-commit")));
        }
        let repository = repository(session)?;
        let paths = params
            .paths
            .iter()
            .map(|path| relative(&repository, path))
            .collect::<Result<Vec<String>, CommandError>>()?;
        crate::hidden_files::guard_change(session, repository.root(), &paths)?;
        save_documents(session, &repository, &paths)?;
        let recording = history::begin(session, &repository);
        let author = params.author.as_deref().map(str::trim).filter(|author| !author.is_empty()).map(str::to_owned);
        let subject = summary.lines().next().unwrap_or_default().to_owned();
        let told = zen_i18n::Message::new("vcs-commit-done")
            .with_number("count", paths.len() as f64)
            .with("subject", subject.as_str());
        let (summary, amend) = (summary.to_owned(), params.amend);
        background::run(
            session,
            Step { kind: "commit", title: t!("vcs-progress-commit"), done: told.clone() },
            move |repository| repository.commit_as(&summary, &paths, amend, author.as_deref()),
            move |session, result| {
                committed(session, &repository, result, recording, (&subject, amend, params.push, params.wait), told)
            },
        )
    }
}

fn committed(
    session: &mut Session,
    repository: &Repository,
    result: Result<String, zen_vcs::VcsError>,
    recording: Option<history::Recording>,
    (subject, amend, push, wait): (&str, bool, bool, bool),
    message: zen_i18n::Message,
) -> Result<CommitResult, CommandError> {
    if result.is_ok() {
        history::finish(session, repository, recording, commit_done(subject, amend));
        history::finish_pending(session, repository, false);
    }
    changed(session);
    let commit = result.map_err(|error| CommandError::new(self::message(&error)))?;
    let push = if push {
        Some(
            super::remote::Push
                .run(session, super::remote::PushParams { wait, ..super::remote::PushParams::default() })?,
        )
    } else {
        None
    };
    Ok(CommitResult { commit, message: message.text(), push })
}

fn commit_staged(session: &mut Session, summary: &str, params: &CommitParams) -> Result<CommitResult, CommandError> {
    let repository = repository(session)?;
    let status = repository.status().map_err(|error| CommandError::new(message(&error)))?;
    let staged = status.changes.iter().filter(|change| change.index.is_some()).count();
    if staged == 0 && !params.amend {
        return Err(CommandError::new(t!("vcs-error-nothing-staged")));
    }
    let staged_files: Vec<String> =
        status.changes.iter().filter(|change| change.index.is_some()).map(|change| change.path.clone()).collect();
    crate::hidden_files::guard_change(session, repository.root(), &staged_files)?;
    let recording = history::begin(session, &repository);
    let author = params.author.as_deref().map(str::trim).filter(|author| !author.is_empty()).map(str::to_owned);
    let subject = summary.lines().next().unwrap_or_default().to_owned();
    let told =
        zen_i18n::Message::new("vcs-commit-done").with_number("count", staged as f64).with("subject", subject.as_str());
    let (summary, amend, push, wait) = (summary.to_owned(), params.amend, params.push, params.wait);
    background::run(
        session,
        Step { kind: "commit", title: t!("vcs-progress-commit"), done: told.clone() },
        move |repository| repository.commit_staged_as(&summary, amend, author.as_deref()),
        move |session, result| committed(session, &repository, result, recording, (&subject, amend, push, wait), told),
    )
}

fn commit_done(subject: &str, amend: bool) -> Done {
    Done::new(if amend { Operation::Amend } else { Operation::Commit }, subject).mode(MoveMode::Soft)
}

pub(super) fn save_documents(
    session: &mut Session,
    repository: &Repository,
    paths: &[String],
) -> Result<(), CommandError> {
    let dirty: Vec<PathBuf> = session
        .state::<Workbench>()
        .into_iter()
        .flat_map(|workbench| &workbench.documents)
        .filter(|document| {
            document.is_dirty() && relative(repository, document.path()).is_ok_and(|path| paths.contains(&path))
        })
        .map(|document| document.path().to_owned())
        .collect();
    for path in &dirty {
        crate::on_save::before_save(session, path);
    }
    let workbench = session.state_mut::<Workbench>();
    let mut failed = Vec::new();
    for document in &mut workbench.documents {
        if document.is_dirty()
            && relative(repository, document.path()).is_ok_and(|path| paths.contains(&path))
            && let Err(error) = document.save()
        {
            failed.push((document.path().to_owned(), crate::closing::save_failed(&document.name(), &error)));
        }
    }
    crate::local_history::record_saves(session);
    crate::closing::failure(failed).map_or(Ok(()), Err)
}

/// The last commit's message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LastMessageInfo {
    /// The message; `None` before the first commit.
    pub message: Option<String>,
}

pub struct LastMessage;

impl Command for LastMessage {
    type Params = NoParams;
    type Output = LastMessageInfo;
    const ID: &'static str = "vcs.commit.lastMessage";
    const TITLE: &'static str = "Amend";
    const DESCRIPTION: &'static str = "The last commit's message, which amending it starts from.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<LastMessageInfo, CommandError> {
        let repository = repository(session)?;
        Ok(LastMessageInfo { message: repository.last_message().map_err(|error| CommandError::new(message(&error)))? })
    }
}

/// What to roll back.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RollbackParams {
    /// The changed files (absolute or relative to the repository).
    pub paths: Vec<PathBuf>,
    /// Also delete the files of added ones from the disk (otherwise they become unversioned).
    pub delete_added: bool,
}

pub struct Rollback;

impl Command for Rollback {
    type Params = RollbackParams;
    type Output = ();
    const ID: &'static str = "vcs.changes.rollback";
    const TITLE: &'static str = "Rollback…";
    const DESCRIPTION: &'static str = "Discards the changes of the given files: they get their committed content back (deleted \
         ones come back); added files stop being added and, with `deleteAdded`, are deleted. Open \
         files show the result.";

    fn run(&self, session: &mut Session, params: RollbackParams) -> Result<(), CommandError> {
        let repository = repository(session)?;
        let paths = params
            .paths
            .iter()
            .map(|path| relative(&repository, path))
            .collect::<Result<Vec<String>, CommandError>>()?;
        crate::hidden_files::guard_change(session, repository.root(), &paths)?;
        let recording = history::begin_with_files(session, &repository, paths.clone());
        let result = crate::local_history::rollback(session, &repository, &paths, params.delete_added);
        if result.is_ok() {
            let done = Done::new(Operation::Rollback, paths.first().cloned().unwrap_or_default()).count(paths.len());
            history::finish(session, &repository, recording, done);
        }
        changed(session);
        let workbench = session.state_mut::<Workbench>();
        for document in &mut workbench.documents {
            if relative(&repository, document.path()).is_ok_and(|path| paths.contains(&path)) {
                document.reload_from_disk();
            }
        }
        result.map_err(|error| CommandError::new(message(&error)))
    }
}

/// A part of a file's changes, as the Git workspace lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChangePart {
    /// What is not staged: the working tree (as the editor has it) against the index.
    Unstaged,
    /// What is staged: the index against HEAD.
    Staged,
}

/// A file to compare with its last committed version.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileParams {
    /// The file; the active document when absent.
    pub path: Option<PathBuf>,
    /// Only the unstaged or the staged part of its changes; all of them (the file against HEAD)
    /// when absent.
    pub part: Option<ChangePart>,
}

/// One change of a diff.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffHunk {
    /// `added`, `modified` or `deleted`.
    pub kind: String,
    /// The first line of the old text, from 0.
    pub before_start: usize,
    /// The line after the old text's last one.
    pub before_end: usize,
    /// The first line of the new text, from 0.
    pub after_start: usize,
    /// The line after the new text's last one.
    pub after_end: usize,
    /// For modified lines: the changed words of the old text, as `[line, startColumn, endColumn]`
    /// in characters.
    pub before_words: Vec<[usize; 3]>,
    /// The changed words of the new text, likewise.
    pub after_words: Vec<[usize; 3]>,
}

/// Both versions of a file and how they differ.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffView {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the repository.
    pub relative: String,
    /// The old version's title (`HEAD`, or a commit id).
    pub before_title: String,
    /// The new version's title (`Your version`, or a commit id).
    pub after_title: String,
    /// The old text; empty for a new file.
    pub before: String,
    /// The new text; empty for a deleted file.
    pub after: String,
    /// The changes, in order.
    pub hunks: Vec<DiffHunk>,
    /// When the file is not text: the diff viewer says so instead.
    pub binary: bool,
    /// For a part of a file's changes (vcs.diff.file with `part`): which one. The diff tab then
    /// offers to stage, unstage or discard its hunks and lines (vcs.index.stage, vcs.index.unstage,
    /// vcs.changes.discard, whose `hunks` are the indices of `hunks` here and whose lines are the
    /// sides' lines).
    pub part: Option<ChangePart>,
    /// Whether the old side can be typed in (a pane of the blank diff window).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub before_editable: bool,
    /// Whether the new side can be typed in: the file of Compare with Clipboard (edits go to its
    /// open document, `path`) or a pane of the blank diff window (no `path`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub after_editable: bool,
    /// When the new side is a selection of `path`'s document: its range, in characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_range: Option<[usize; 2]>,
}

pub(crate) fn diff_view(
    path: PathBuf,
    relative: String,
    titles: (String, String),
    before: &[u8],
    after: &[u8],
) -> DiffView {
    diff_view_with(path, relative, titles, before, after, Whitespace::None)
}

pub(crate) fn diff_view_with(
    path: PathBuf,
    relative: String,
    titles: (String, String),
    before: &[u8],
    after: &[u8],
    whitespace: Whitespace,
) -> DiffView {
    let (Ok(before), Ok(after)) = (std::str::from_utf8(before), std::str::from_utf8(after)) else {
        return DiffView {
            path,
            relative,
            before_title: titles.0,
            after_title: titles.1,
            binary: true,
            ..DiffView::default()
        };
    };
    DiffView {
        path,
        relative,
        before_title: titles.0,
        after_title: titles.1,
        before: before.to_owned(),
        after: after.to_owned(),
        hunks: diff_hunks(before, after, whitespace),
        ..DiffView::default()
    }
}

pub(crate) fn diff_hunks(before: &str, after: &str, whitespace: Whitespace) -> Vec<DiffHunk> {
    let hunks = if whitespace == Whitespace::None {
        diff::hunks(before, after)
    } else {
        diff::hunks(&whitespace.normalized(before), &whitespace.normalized(after))
    };
    let mut hunks: Vec<DiffHunk> = hunks.iter().map(|hunk| diff_hunk(before, after, hunk)).collect();
    if whitespace != Whitespace::None {
        let (old, new) = (diff::lines(before), diff::lines(after));
        for hunk in &mut hunks {
            hunk.before_words.retain(|word| !whitespace.ignores_word(&old, word));
            hunk.after_words.retain(|word| !whitespace.ignores_word(&new, word));
        }
    }
    hunks
}

fn diff_hunk(before: &str, after: &str, hunk: &Hunk) -> DiffHunk {
    let (old, new) = (diff::lines(before), diff::lines(after));
    let mut result = DiffHunk {
        kind: hunk.kind().as_str().to_owned(),
        before_start: hunk.before.start,
        before_end: hunk.before.end,
        after_start: hunk.after.start,
        after_end: hunk.after.end,
        ..DiffHunk::default()
    };
    if hunk.kind() == diff::LineKind::Modified {
        for (old_line, new_line) in hunk.before.clone().zip(hunk.after.clone()) {
            let (Some(a), Some(b)) = (old.get(old_line), new.get(new_line)) else { continue };
            let (removed, added) =
                diff::word_changes(a.trim_end_matches(['\n', '\r']), b.trim_end_matches(['\n', '\r']));
            result.before_words.extend(removed.into_iter().map(|range| [old_line, range.start, range.end]));
            result.after_words.extend(added.into_iter().map(|range| [new_line, range.start, range.end]));
        }
    }
    result
}

pub(super) fn current_text(session: &Session, path: &Path) -> Option<Vec<u8>> {
    let open = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| document.text().as_bytes().to_vec());
    open.or_else(|| std::fs::read(path).ok())
}

fn file_or_active(session: &Session, path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    path.or_else(|| {
        session.state::<Workbench>().and_then(Workbench::active_document).map(|document| document.path().to_owned())
    })
    .ok_or_else(|| CommandError::new(t!("vcs-error-no-file")))
}

pub struct FileDiff;

impl Command for FileDiff {
    type Params = FileParams;
    type Output = DiffView;
    const ID: &'static str = "vcs.diff.file";
    const TITLE: &'static str = "Show Diff";
    const DESCRIPTION: &'static str = "Compares a file (the active one by default), as the editor has it, with its version at \
         HEAD: both texts and the changed lines and words. With `part`, only the unstaged changes \
         (the file against the index) or only the staged ones (the index against HEAD).";

    fn run(&self, session: &mut Session, params: FileParams) -> Result<DiffView, CommandError> {
        let path = file_or_active(session, params.path)?;
        let repository = repository(session)?;
        let path = if path.is_absolute() { path } else { repository.root().join(path) };
        let relative = relative(&repository, &path)?;
        let failed = |error: zen_vcs::VcsError| CommandError::new(message(&error));
        let head = || repository.file_at_head(&relative).map(Option::unwrap_or_default).map_err(failed);
        let index = || repository.file_in_index(&relative).map(Option::unwrap_or_default).map_err(failed);
        let (before, after, titles) = match params.part {
            None => {
                (head()?, current_text(session, &path).unwrap_or_default(), (t!("vcs-diff-head"), t!("vcs-diff-yours")))
            }
            Some(ChangePart::Unstaged) => (
                index()?,
                current_text(session, &path).unwrap_or_default(),
                (t!("vcs-diff-staged"), t!("vcs-diff-yours")),
            ),
            Some(ChangePart::Staged) => (head()?, index()?, (t!("vcs-diff-head"), t!("vcs-diff-staged"))),
        };
        let mut view = diff_view(path, relative, titles, &before, &after);
        view.part = params.part;
        Ok(view)
    }
}

/// A change of an open file, as the gutter marks it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LineChange {
    /// `added`, `modified` or `deleted`.
    pub kind: String,
    /// The first changed line of the document, from 0 (for deleted lines, the line below them).
    pub start: usize,
    /// The line after the last changed one (equal to `start` for deleted lines).
    pub end: usize,
    /// The committed lines it replaced.
    pub before: String,
}

/// The change markers of a file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LineStatusInfo {
    /// The file.
    pub path: PathBuf,
    /// The document's version they are for.
    pub version: u64,
    /// Whether the file is tracked (has a committed version); untracked files have no markers.
    pub tracked: bool,
    /// The changes, in order.
    pub changes: Vec<LineChange>,
}

#[derive(Default)]
pub(super) struct LineCache {
    bases: std::collections::HashMap<PathBuf, (u64, Option<String>)>,
    markers: std::collections::HashMap<PathBuf, (u64, u64, LineStatusInfo)>,
}

fn base_text(session: &mut Session, path: &Path) -> Result<(u64, Option<String>), CommandError> {
    let repository = repository(session)?;
    let stamp = repository.stamp();
    let cache = &mut super::vcs(session).lines;
    if let Some((at, text)) = cache.bases.get(path)
        && *at == stamp
    {
        return Ok((stamp, text.clone()));
    }
    let relative = relative(&repository, path)?;
    let text = repository
        .file_at_head(&relative)
        .map_err(|error| CommandError::new(message(&error)))?
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let cache = &mut super::vcs(session).lines;
    cache.bases.insert(path.to_owned(), (stamp, text.clone()));
    Ok((stamp, text))
}

pub(crate) fn head_text(session: &mut Session, path: &Path) -> Option<String> {
    base_text(session, path).ok()?.1
}

fn document_version(session: &Session, path: &Path) -> Result<u64, CommandError> {
    session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .filter(|document| document.image().is_none())
        .map(Document::version)
        .ok_or_else(|| CommandError::new(t!("vcs-error-not-open")))
}

fn document(session: &Session, path: &Path) -> Result<(String, u64), CommandError> {
    session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .filter(|document| document.image().is_none())
        .map(|document| (document.text().to_owned(), document.version()))
        .ok_or_else(|| CommandError::new(t!("vcs-error-not-open")))
}

pub(crate) fn changed_lines(session: &mut Session, path: &Path, text: &str) -> Option<Vec<std::ops::Range<usize>>> {
    let (_, base) = base_text(session, path).ok()?;
    let base = base?;
    Some(diff::hunks(&base, text).iter().map(|hunk| hunk.after.clone()).collect())
}

pub struct LineStatus;

impl Command for LineStatus {
    type Params = FileParams;
    type Output = LineStatusInfo;
    const ID: &'static str = "vcs.lineStatus.list";
    const TITLE: &'static str = "Changed Lines";
    const DESCRIPTION: &'static str = "Lists the lines of an open file (the active one by default) that differ from its version \
         at HEAD, with unsaved edits: added, modified and deleted lines, each with the committed lines \
         it replaced.";

    fn run(&self, session: &mut Session, params: FileParams) -> Result<LineStatusInfo, CommandError> {
        let path = file_or_active(session, params.path)?;
        let version = document_version(session, &path)?;
        let stamp = repository(session)?.stamp();
        if let Some((at_version, at_stamp, info)) = super::vcs(session).lines.markers.get(&path)
            && *at_version == version
            && *at_stamp == stamp
        {
            return Ok(info.clone());
        }
        let (text, version) = document(session, &path)?;
        let (stamp, base) = base_text(session, &path)?;
        let info = match &base {
            None => LineStatusInfo { path: path.clone(), version, tracked: false, changes: Vec::new() },
            Some(base) => {
                let old = diff::lines(base);
                let changes = diff::hunks(base, &text)
                    .iter()
                    .map(|hunk| LineChange {
                        kind: hunk.kind().as_str().to_owned(),
                        start: hunk.after.start,
                        end: hunk.after.end,
                        before: old[hunk.before.start.min(old.len())..hunk.before.end.min(old.len())].concat(),
                    })
                    .collect();
                LineStatusInfo { path: path.clone(), version, tracked: true, changes }
            }
        };
        super::vcs(session).lines.markers.insert(path, (version, stamp, info.clone()));
        Ok(info)
    }
}

/// A change of an open file to roll back.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RollbackLinesParams {
    /// The file; the active document when absent.
    pub path: Option<PathBuf>,
    /// A line of the change (from 0); for deleted lines, the line below them.
    pub line: usize,
}

pub struct RollbackLines;

impl Command for RollbackLines {
    type Params = RollbackLinesParams;
    type Output = ();
    const ID: &'static str = "vcs.lineStatus.rollback";
    const TITLE: &'static str = "Rollback Lines";
    const DESCRIPTION: &'static str = "Puts back the committed lines of the change at `line` in an open file (the active one by \
         default), as one edit that Undo takes back.";

    fn run(&self, session: &mut Session, params: RollbackLinesParams) -> Result<(), CommandError> {
        let path = file_or_active(session, params.path)?;
        let (text, _) = document(session, &path)?;
        let (_, base) = base_text(session, &path)?;
        let Some(base) = base else { return Err(CommandError::new(t!("vcs-error-no-change"))) };
        let hunk = diff::hunks(&base, &text)
            .into_iter()
            .find(|hunk| {
                hunk.after.contains(&params.line) || (hunk.after.is_empty() && hunk.after.start == params.line)
            })
            .ok_or_else(|| CommandError::new(t!("vcs-error-no-change")))?;
        let new = diff::lines(&text);
        let old = diff::lines(&base);
        let chars = |lines: &[&str]| lines.iter().map(|line| line.chars().count()).sum::<usize>();
        let start = chars(&new[..hunk.after.start.min(new.len())]);
        let end = start + chars(&new[hunk.after.start.min(new.len())..hunk.after.end.min(new.len())]);
        let replacement = old[hunk.before.start.min(old.len())..hunk.before.end.min(old.len())].concat();
        let workbench = session.state_mut::<Workbench>();
        let document = workbench
            .documents
            .iter_mut()
            .find(|document| document.path() == path)
            .ok_or_else(|| CommandError::new(t!("vcs-error-not-open")))?;
        document.replace_ranges(vec![(start..end, replacement)]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    #[test]
    fn sign_off_adds_the_committers_line_once() {
        let ada = || (Some("Ada".to_owned()), Some("ada@example.com".to_owned()));
        let line = "Signed-off-by: Ada <ada@example.com>";
        assert_eq!(signed_off("Fix it", ada()).unwrap(), format!("Fix it\n\n{line}"));
        let trailer = "Fix it\n\nCo-Authored-By: Claude <noreply@anthropic.com>";
        assert_eq!(signed_off(trailer, ada()).unwrap(), format!("{trailer}\n{line}"), "beside the trailers");
        let signed = format!("Fix it\n\n{line}");
        assert_eq!(signed_off(&signed, ada()).unwrap(), signed, "once");
        assert!(signed_off("Fix it", (Some("Ada".to_owned()), None)).is_err(), "no e-mail");
    }

    #[test]
    fn commits_only_the_chosen_files_and_amends() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        std::fs::write(root.join("other.txt"), "other\n").unwrap();
        assert!(
            registry
                .dispatch(&mut session, "vcs.commit.create", json!({ "message": " ", "paths": ["a.txt"] }))
                .is_err()
        );
        let made = registry
            .dispatch(
                &mut session,
                "vcs.commit.create",
                json!({ "message": "Change a\n\nDetails.", "paths": [root.join("a.txt"), "new.txt"] }),
            )
            .unwrap();
        assert_eq!(made["commit"].as_str().map(str::len), Some(40));
        assert_eq!(git(&root, &["log", "-1", "--format=%s"]).trim(), "Change a");
        assert_eq!(git(&root, &["status", "--porcelain"]).trim(), "?? other.txt", "other.txt stays out");
        let last = registry.dispatch(&mut session, "vcs.commit.lastMessage", json!({})).unwrap();
        assert_eq!(last["message"], "Change a\n\nDetails.");
        registry
            .dispatch(&mut session, "vcs.commit.create", json!({ "message": "Change a and b", "amend": true }))
            .unwrap();
        assert_eq!(git(&root, &["log", "--format=%s"]).lines().collect::<Vec<_>>(), ["Change a and b", "First"]);
    }

    #[test]
    fn rollback_restores_files_and_unadds_new_ones() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(root.join("added.txt"), "added\n").unwrap();
        git(&root, &["add", "added.txt"]);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("a.txt") })).unwrap();
        registry
            .dispatch(&mut session, "vcs.changes.rollback", json!({ "paths": ["a.txt", root.join("added.txt")] }))
            .unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n");
        let workbench = session.state::<Workbench>().unwrap();
        assert_eq!(workbench.active_document().unwrap().text(), "one\ntwo\nthree\n", "the open file follows");
        assert_eq!(git(&root, &["status", "--porcelain"]).trim(), "?? added.txt", "kept on disk, no longer added");
        git(&root, &["add", "added.txt"]);
        registry
            .dispatch(&mut session, "vcs.changes.rollback", json!({ "paths": ["added.txt"], "deleteAdded": true }))
            .unwrap();
        assert!(!root.join("added.txt").exists());
        std::fs::remove_file(root.join("a.txt")).unwrap();
        registry.dispatch(&mut session, "vcs.changes.rollback", json!({ "paths": ["a.txt"] })).unwrap();
        assert!(root.join("a.txt").exists(), "a deleted file comes back");
    }

    #[test]
    fn diffs_and_change_markers_follow_unsaved_edits_and_roll_back() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let file = root.join("a.txt");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let none = registry.dispatch(&mut session, "vcs.lineStatus.list", json!({})).unwrap();
        assert_eq!((none["tracked"].as_bool(), none["changes"].as_array().map(Vec::len)), (Some(true), Some(0)));
        let workbench = session.state_mut::<Workbench>();
        let document = workbench.active_document_mut().unwrap();
        document.replace_ranges(vec![(4..7, "2".to_owned()), (14..14, "four\n".to_owned())]);
        assert_eq!(document.text(), "one\n2\nthree\nfour\n");
        let markers = registry.dispatch(&mut session, "vcs.lineStatus.list", json!({ "path": file })).unwrap();
        assert_eq!(
            markers["changes"],
            json!([
                { "kind": "modified", "start": 1, "end": 2, "before": "two\n" },
                { "kind": "added", "start": 3, "end": 4, "before": "" },
            ])
        );
        let view = registry.dispatch(&mut session, "vcs.diff.file", json!({})).unwrap();
        assert_eq!(view["before"], "one\ntwo\nthree\n");
        assert_eq!(view["after"], "one\n2\nthree\nfour\n");
        assert_eq!(view["hunks"][0]["beforeWords"], json!([[1, 0, 3]]));
        assert_eq!(view["hunks"][0]["afterWords"], json!([[1, 0, 1]]));
        assert_eq!(view["hunks"][1]["kind"], "added");

        registry.dispatch(&mut session, "vcs.lineStatus.rollback", json!({ "line": 1 })).unwrap();
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "one\ntwo\nthree\nfour\n");
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "one\n2\nthree\nfour\n", "one undo step");
        assert!(registry.dispatch(&mut session, "vcs.lineStatus.rollback", json!({ "line": 0 })).is_err());

        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("b.txt") })).unwrap();
        let markers = registry.dispatch(&mut session, "vcs.lineStatus.list", json!({})).unwrap();
        assert_eq!(markers["tracked"], false);
        let view = registry.dispatch(&mut session, "vcs.diff.file", json!({})).unwrap();
        assert_eq!(
            view["hunks"],
            json!([{ "kind": "added", "beforeStart": 0, "beforeEnd": 0, "afterStart": 0, "afterEnd": 1, "beforeWords": [], "afterWords": [] }])
        );
    }

    #[test]
    fn paths_leaving_the_repository_are_refused_and_vanished_ones_are_not() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let repository = Repository::discover(&root).unwrap();
        assert!(relative(&repository, Path::new("../x")).is_err());
        assert!(relative(&repository, &root.join("../x")).is_err());
        assert!(relative(&repository, &root.join("gone/../../x")).is_err());
        assert_eq!(relative(&repository, &root.join("a.txt")).unwrap(), "a.txt");
        assert_eq!(relative(&repository, &root.join("gone/deep/file.txt")).unwrap(), "gone/deep/file.txt");
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
            assert!(relative(&repository, Path::new("link/x.txt")).is_err());
        }
    }

    #[test]
    fn discarding_an_unversioned_file_outside_the_repository_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("repository");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "First"]);
        let victim = root.parent().unwrap().join("x.txt");
        std::fs::write(&victim, "precious\n").unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        assert!(registry.dispatch(&mut session, "vcs.changes.discard", json!({ "paths": ["../x.txt"] })).is_err());
        assert!(victim.exists());
    }

    #[test]
    fn a_branch_named_like_an_option_is_refused_in_the_language_of_the_window() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let params = json!({ "name": "-D", "startPoint": "main" });
        let failed = registry.dispatch(&mut session, "vcs.branch.create", params).unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(error.message, t!("vcs-error-invalid-branch", name = "-D"));
        assert_eq!(git(&root, &["branch", "--list", "main"]).trim(), "* main");
    }
}
