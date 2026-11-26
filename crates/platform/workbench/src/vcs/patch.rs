use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::Repository;
use zen_vcs::patch::{self, Binary, FileKind, FilePatch, Mail, Patch, PatchError};

use super::changelists;
use super::commit::{DiffView, diff_view, relative, save_documents};
use super::history::{self, Done, Operation};
use super::workspace::GitWorkspace;
use super::{absolute, changed, message, repository};
use crate::Workbench;

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CreatePatch)?;
    registry.register(ReadPatch)?;
    registry.register(PatchDiff)?;
    registry.register(ApplyPatch)?;
    registry.register(GetCommitDraft)?;
    Ok(())
}

/// What a patch is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PatchOf {
    /// Local changes: the files `paths`, or those of a changelist.
    #[default]
    Changes,
    /// A commit, against its first parent.
    Commit,
    /// Shelved changes.
    Shelf,
}

/// What to make a patch of and where to put it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CreatePatchParams {
    /// What the patch is of: `changes` (the default), `commit` or `shelf`.
    pub source: PatchOf,
    /// For `changes`: the changed files (absolute or relative to the repository), unversioned ones
    /// too; the files of `changelist`, or of the active changelist, when empty. For `shelf`: only
    /// these of its files.
    pub paths: Vec<PathBuf>,
    /// For `changes` without `paths`: the changelist.
    pub changelist: Option<String>,
    /// For `commit`: the commit (an id, a branch, `HEAD~1`).
    pub revision: Option<String>,
    /// For `shelf`: the shelf's id (vcs.shelf.list).
    pub shelf: Option<u64>,
    /// The file to write the patch to (absolute, or relative to the repository).
    pub output: Option<PathBuf>,
    /// Put the patch on the clipboard instead of in a file (Copy as Patch to Clipboard).
    pub clipboard: bool,
    /// Base path: the folder the patch's paths are relative to (absolute, or relative to the
    /// repository); the repository's root when absent. A folder inside the repository leaves out
    /// the files outside it; a folder above it puts the repository's path before every file.
    pub base_path: Option<PathBuf>,
    /// Reverse patch: a patch that undoes the changes.
    pub reverse: bool,
    /// The encoding of the file written (`UTF-8`, `windows-1251`…); UTF-8 when absent.
    pub encoding: Option<String>,
    /// The patch's subject for local and shelved changes: the changelist's or the shelf's name when
    /// absent.
    pub message: Option<String>,
}

/// A patch made.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CreatedPatch {
    /// The file written, when it was written to one.
    pub path: Option<PathBuf>,
    /// Whether it was put on the clipboard.
    pub clipboard: bool,
    /// Its files, as the patch names them.
    pub files: Vec<String>,
    /// Its size in bytes.
    pub bytes: usize,
    /// The encoding it was written in.
    pub encoding: String,
    /// The patch itself, when it was not written to a file.
    pub text: Option<String>,
    /// For an agent: how many files hidden from agents were left out of the patch (ADR-0143).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_files: Option<usize>,
}

pub struct CreatePatch;

impl Command for CreatePatch {
    type Params = CreatePatchParams;
    type Output = CreatedPatch;
    const ID: &'static str = "vcs.patch.create";
    const TITLE: &'static str = "Create Patch…";
    const DESCRIPTION: &'static str = "Makes a git patch (a unified diff `git apply` and `git am` take) of local changes \
         (`source: changes`, the files `paths` or of `changelist`, unsaved edits included), of a commit \
         (`source: commit`, `revision`) or of shelved changes (`source: shelf`, `shelf`), and writes it to \
         `output` or, with `clipboard`, puts it on the clipboard. `basePath` makes paths relative to \
         another folder, `reverse` makes a patch that undoes the changes, `encoding` writes the file in \
         another encoding. Answers with the files, the size and, unless written to a file, the text.";

    fn run(&self, session: &mut Session, params: CreatePatchParams) -> Result<CreatedPatch, CommandError> {
        let repository = repository(session)?;
        let (bytes, mail) = match params.source {
            PatchOf::Changes => changes_patch(session, &repository, &params)?,
            PatchOf::Commit => {
                let revision = params.revision.as_deref().map(str::trim).filter(|revision| !revision.is_empty());
                let revision = revision.ok_or_else(|| CommandError::new(t!("vcs-patch-error-revision")))?;
                let (commit, bytes) = repository.commit_patch(revision).map_err(|error| failed(&error))?;
                let mail = Mail {
                    commit: Some(commit.id.clone()),
                    author: format!("{} <{}>", commit.author, commit.email),
                    date: patch::mail_date(commit.time, commit.offset),
                    subject: commit.subject.clone(),
                    body: commit.body.clone(),
                };
                (bytes, mail)
            }
            PatchOf::Shelf => {
                let id = params.shelf.ok_or_else(|| CommandError::new(t!("vcs-patch-error-shelf")))?;
                let (file, name) = super::shelf::patch_of(session, repository.root(), id)?;
                let bytes = std::fs::read(&file).map_err(|error| CommandError::new(error.to_string()))?;
                let subject = params.message.clone().filter(|text| !text.trim().is_empty()).unwrap_or(name);
                (bytes, local_mail(&repository, subject))
            }
        };
        let mut made =
            patch::parse(&bytes).map(|parsed| parsed.without_git_prefixes()).map_err(|error| patch_error(&error))?;
        if params.source == PatchOf::Shelf && !params.paths.is_empty() {
            let wanted = params
                .paths
                .iter()
                .map(|path| relative(&repository, path))
                .collect::<Result<Vec<_>, CommandError>>()?;
            made.files.retain(|file| wanted.iter().any(|path| path == file.path()));
            if made.files.is_empty() {
                return Err(CommandError::new(t!("vcs-patch-error-nothing")));
            }
        }
        let hidden_files = crate::hidden_files::for_caller(session).map(|patterns| {
            let before = made.files.len();
            made.files.retain(|file| {
                [file.old_path.as_deref(), file.new_path.as_deref(), Some(file.path())]
                    .into_iter()
                    .flatten()
                    .all(|path| !crate::hidden_files::is_hidden(&patterns, Some(repository.root()), Path::new(path)))
            });
            before - made.files.len()
        });
        made.mail = Some(mail);
        if let Some(base) = params.base_path.as_deref().filter(|base| !base.as_os_str().is_empty()) {
            made = rebase_paths(&repository, &made, base)?;
        }
        if params.reverse {
            made = made.reversed();
        }
        let files: Vec<String> = made.files.iter().map(|file| file.path().to_owned()).collect();
        let text = made.to_bytes();
        let label = params.encoding.as_deref().map(str::trim).filter(|label| !label.is_empty()).unwrap_or("UTF-8");
        let encoding = patch::encoding_name(label)
            .ok_or_else(|| CommandError::new(t!("vcs-patch-error-encoding", name = label)))?;
        let mut created =
            CreatedPatch { files, encoding: encoding.to_owned(), hidden_files, ..CreatedPatch::default() };
        if params.clipboard {
            let copied = String::from_utf8_lossy(&text).into_owned();
            created.bytes = copied.len();
            created.clipboard = crate::file_clipboard::set_clipboard_text(session, &copied);
            created.encoding = "UTF-8".to_owned();
            created.text = Some(copied);
            return Ok(created);
        }
        let encoded = patch::encode(&text, encoding).unwrap_or(text);
        created.bytes = encoded.len();
        match &params.output {
            Some(output) => {
                let output = if output.is_absolute() { output.clone() } else { repository.root().join(output) };
                if let Some(parent) = output.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| CommandError::new(error.to_string()))?;
                }
                std::fs::write(&output, &encoded)
                    .map_err(|error| CommandError::new(format!("{}: {error}", output.display())))?;
                created.path = Some(output);
            }
            None => created.text = Some(String::from_utf8_lossy(&encoded).into_owned()),
        }
        Ok(created)
    }
}

fn failed(error: &zen_vcs::VcsError) -> CommandError {
    CommandError::new(message(error))
}

pub(crate) fn patch_error(error: &PatchError) -> CommandError {
    CommandError::new(match error {
        PatchError::Empty => t!("vcs-patch-error-empty"),
        PatchError::BadHunk(line) => t!("vcs-patch-error-hunk", line = *line),
        PatchError::Truncated(line) => t!("vcs-patch-error-truncated", line = *line),
        PatchError::UnsafePath(path) => t!("vcs-patch-error-unsafe", path = path.as_str()),
        PatchError::Strip(path) => t!("vcs-patch-error-strip", path = path.as_str()),
        PatchError::OutsideBase(path) => t!("vcs-patch-error-outside", path = path.as_str()),
        PatchError::TooLarge => t!("vcs-patch-error-large"),
    })
}

fn local_mail(repository: &Repository, subject: String) -> Mail {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
    Mail {
        commit: None,
        author: repository.author_ident().unwrap_or_default(),
        date: patch::mail_date(now, zen_vcs::local_offset()),
        subject,
        body: String::new(),
    }
}

fn changes_patch(
    session: &mut Session,
    repository: &Repository,
    params: &CreatePatchParams,
) -> Result<(Vec<u8>, Mail), CommandError> {
    let (list, paths) = if params.paths.is_empty() {
        let list = match params.changelist.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => name.to_owned(),
            None => changelists::active(session)?,
        };
        let paths = changelists::files_of(session, &list)?;
        (Some(list), paths)
    } else {
        let paths =
            params.paths.iter().map(|path| relative(repository, path)).collect::<Result<Vec<_>, CommandError>>()?;
        (None, paths)
    };
    if paths.is_empty() {
        return Err(CommandError::new(t!("vcs-patch-error-no-changes")));
    }
    save_documents(session, repository, &paths)?;
    let bytes = repository.changes_patch(&paths).map_err(|error| failed(&error))?;
    if bytes.is_empty() {
        return Err(CommandError::new(t!("vcs-patch-error-no-changes")));
    }
    let subject = params
        .message
        .clone()
        .filter(|text| !text.trim().is_empty())
        .or(list)
        .unwrap_or_else(|| t!("vcs-changelist-default"));
    Ok((bytes, local_mail(repository, subject)))
}

fn rebase_paths(repository: &Repository, made: &Patch, base: &Path) -> Result<Patch, CommandError> {
    let root = repository.root();
    let base = if base.is_absolute() { base.to_owned() } else { root.join(base) };
    let base = base.canonicalize().unwrap_or(base);
    let joined = |path: &Path| {
        path.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
    };
    if let Ok(inside) = base.strip_prefix(root) {
        return made.relative_to(&joined(inside)).map_err(|error| patch_error(&error));
    }
    if let Ok(above) = root.strip_prefix(&base) {
        return made.under(&joined(above)).map_err(|error| patch_error(&error));
    }
    Err(CommandError::new(t!("vcs-patch-error-base", path = base.display().to_string())))
}

/// Where a patch to apply comes from, and how its paths map onto the repository.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PatchInput {
    /// The patch file (absolute, or relative to the repository).
    pub path: Option<PathBuf>,
    /// The patch itself, when it is not in a file.
    pub text: Option<String>,
    /// Read the patch from the clipboard (Apply Patch from Clipboard…).
    pub clipboard: bool,
    /// The patch file's encoding (`windows-1251`…); UTF-8 when absent.
    pub encoding: Option<String>,
    /// Map base directory: the folder the patch's paths start from (absolute, or relative to the
    /// repository); the repository's root when absent.
    pub base_dir: Option<PathBuf>,
    /// How many leading folders to strip from the patch's paths; when absent, git's `a/` and `b/`
    /// are stripped and other paths are kept as they are.
    pub strip: Option<usize>,
    /// Apply the patch backwards, undoing it.
    pub reverse: bool,
}

/// A hunk that does not apply as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FailedHunk {
    /// Which hunk of the file, from 1.
    pub hunk: usize,
    /// The line of the file (from 1) it was expected at.
    pub line: usize,
    /// Its header: `@@ -5,3 +5,3 @@`.
    pub header: String,
}

/// A file of a patch, as it would apply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PatchFile {
    /// The file it changes.
    pub path: PathBuf,
    /// The file, relative to the repository, with `/`.
    pub relative: String,
    /// For a rename or a copy: where it comes from, relative.
    pub old_relative: Option<String>,
    /// The path as the patch names it.
    pub patch_path: String,
    /// `added`, `modified`, `deleted`, `renamed` or `copied`.
    pub status: String,
    /// How it applies: `clean`; `merged` (three-way, from the blob the patch was made from);
    /// `conflict` (hunks left to merge); `binary` (applied by git); `missing` (the file to change is
    /// not there); `exists` (the file to add is there, with other content); `failed` (cannot apply:
    /// a binary change without data, a file outside the repository, text that is not UTF-8).
    pub state: String,
    /// How many hunks it has.
    pub hunks: usize,
    /// The hunks that do not apply as they are.
    pub failed: Vec<FailedHunk>,
    /// The hunks (from 1) that apply only where or how `git apply` would not take them: away from
    /// the start or the end of the file they must be at, with context lines left out, or with
    /// whitespace or line breaks ignored.
    pub approximate: Vec<usize>,
    /// Whether the patch makes the file executable (`true`) or no longer executable (`false`);
    /// absent when it leaves that as it is.
    pub executable: Option<bool>,
    /// Why it cannot apply or conflicts, when it is not just its hunks.
    pub reason: Option<String>,
}

/// A patch as it would apply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PatchPreview {
    /// Its files.
    pub files: Vec<PatchFile>,
    /// For a patch made from a commit: its author, `Name <email>`.
    pub author: Option<String>,
    /// For a patch made from a commit: its message.
    pub message: Option<String>,
    /// How many leading folders were stripped from its paths.
    pub strip: usize,
    /// The folder its paths start from, relative to the repository (empty for the root).
    pub base_dir: String,
    /// Where it was read from: the file's name, or `clipboard`.
    pub source: String,
}

pub(crate) struct Loaded {
    pub(crate) patch: Patch,
    raw: Patch,
    strip: usize,
    base_dir: String,
    source: String,
}

pub(crate) fn load(session: &mut Session, repository: &Repository, input: &PatchInput) -> Result<Loaded, CommandError> {
    let (bytes, source) = if let Some(path) = &input.path {
        let path = if path.is_absolute() { path.clone() } else { repository.root().join(path) };
        if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > patch::MAX_BYTES as u64) {
            return Err(patch_error(&PatchError::TooLarge));
        }
        let bytes = std::fs::read(&path).map_err(|error| CommandError::new(format!("{}: {error}", path.display())))?;
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        (bytes, name)
    } else if let Some(text) = &input.text {
        (text.clone().into_bytes(), String::new())
    } else if input.clipboard {
        (crate::file_clipboard::clipboard_text(session).into_bytes(), "clipboard".to_owned())
    } else {
        return Err(CommandError::new(t!("vcs-patch-error-no-input")));
    };
    let label = input.encoding.as_deref().map(str::trim).filter(|label| !label.is_empty()).unwrap_or("UTF-8");
    let bytes =
        patch::decode(&bytes, label).ok_or_else(|| CommandError::new(t!("vcs-patch-error-encoding", name = label)))?;
    let raw = patch::parse(&bytes).map_err(|error| patch_error(&error))?;
    let strip = input.strip.unwrap_or_else(|| raw.detected_strip());
    let mut mapped = raw.strip(strip).map_err(|error| patch_error(&error))?;
    let mut base_dir = String::new();
    if let Some(base) = input.base_dir.as_deref().filter(|base| !base.as_os_str().is_empty()) {
        let root = repository.root();
        let base = if base.is_absolute() { base.to_owned() } else { root.join(base) };
        let base = base.canonicalize().unwrap_or(base);
        let inside = base
            .strip_prefix(root)
            .map_err(|_| CommandError::new(t!("vcs-error-outside", path = base.display().to_string())))?;
        base_dir = inside
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        mapped = mapped.under(&base_dir).map_err(|error| patch_error(&error))?;
    }
    if input.reverse {
        mapped = mapped.reversed();
    }
    Ok(Loaded { patch: mapped, raw, strip, base_dir, source })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Clean,
    Merged,
    Conflict,
    Binary,
    Missing,
    Exists,
    Failed,
}

impl State {
    fn as_str(&self) -> &'static str {
        match self {
            State::Clean => "clean",
            State::Merged => "merged",
            State::Conflict => "conflict",
            State::Binary => "binary",
            State::Missing => "missing",
            State::Exists => "exists",
            State::Failed => "failed",
        }
    }
}

pub(crate) struct Planned {
    file: FilePatch,
    raw_path: String,
    target: String,
    source: String,
    ours: Option<Vec<u8>>,
    state: State,
    result: Option<Vec<u8>>,
    base: Option<Vec<u8>>,
    theirs: Option<Vec<u8>>,
    failed: Vec<FailedHunk>,
    approximate: Vec<usize>,
    reason: Option<String>,
}

impl Planned {
    fn info(&self, root: &Path) -> PatchFile {
        let renamed = matches!(self.file.kind, FileKind::Renamed | FileKind::Copied);
        PatchFile {
            path: absolute(root, &self.target),
            relative: self.target.clone(),
            old_relative: renamed.then(|| self.source.clone()),
            patch_path: self.raw_path.clone(),
            status: self.file.kind.as_str().to_owned(),
            state: self.state.as_str().to_owned(),
            hunks: self.file.hunks.len(),
            failed: self.failed.clone(),
            approximate: self.approximate.clone(),
            executable: self.file.executable_change(),
            reason: self.reason.clone(),
        }
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn source(&self) -> Option<&str> {
        matches!(self.file.kind, FileKind::Renamed | FileKind::Copied).then_some(self.source.as_str())
    }

    pub(crate) fn status(&self) -> &'static str {
        self.file.kind.as_str()
    }

    pub(crate) fn before(&self) -> Option<&[u8]> {
        self.ours.as_deref()
    }

    pub(crate) fn after(&self) -> Option<&[u8]> {
        self.result.as_deref().or(self.theirs.as_deref())
    }
}

fn checked_file(root: &Path, relative: &str) -> Option<PathBuf> {
    let file = absolute(root, relative);
    let mut at = root.to_path_buf();
    for part in relative.split('/') {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(metadata) if metadata.file_type().is_symlink() => return None,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Some(file)
}

fn read(root: &Path, relative: &str) -> Option<Vec<u8>> {
    let file = checked_file(root, relative)?;
    std::fs::metadata(&file).ok().filter(std::fs::Metadata::is_file)?;
    std::fs::read(file).ok()
}

fn failed_hunks(file: &FilePatch, applied: &patch::Applied) -> Vec<FailedHunk> {
    applied
        .hunks
        .iter()
        .enumerate()
        .filter(|(_, result)| !result.applied)
        .map(|(index, result)| {
            let hunk = &file.hunks[index];
            FailedHunk {
                hunk: index + 1,
                line: result.line,
                header: format!("@@ -{},{} +{},{} @@", hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines),
            }
        })
        .collect()
}

pub(crate) fn plan(repository: &Repository, loaded: &Loaded) -> Vec<Planned> {
    let root = repository.root();
    let labels = (t!("vcs-patch-conflict-current"), t!("vcs-patch-conflict-patch"));
    let labels = (labels.0.as_str(), labels.1.as_str());
    loaded
        .patch
        .files
        .iter()
        .zip(&loaded.raw.files)
        .map(|(file, raw)| {
            let target = file.path().to_owned();
            let source = file.old_path.clone().unwrap_or_else(|| target.clone());
            let mut planned = Planned {
                file: file.clone(),
                raw_path: raw.path().to_owned(),
                target: target.clone(),
                source: source.clone(),
                ours: None,
                state: State::Clean,
                result: None,
                base: None,
                theirs: None,
                failed: Vec::new(),
                approximate: Vec::new(),
                reason: None,
            };
            if checked_file(root, &target).is_none() || checked_file(root, &source).is_none() {
                planned.state = State::Failed;
                planned.reason = Some(t!("vcs-patch-reason-symlink"));
                return planned;
            }
            if file.is_special() {
                planned.state = State::Failed;
                planned.reason = Some(t!("vcs-patch-reason-special"));
                return planned;
            }
            let existing = read(root, if file.kind == FileKind::Added { &target } else { &source });
            planned.ours = existing.clone();
            match &file.binary {
                Some(Binary::Marker) => {
                    planned.state = State::Failed;
                    planned.reason = Some(t!("vcs-patch-reason-no-data"));
                    return planned;
                }
                Some(Binary::Data(_)) => {
                    planned.state = State::Binary;
                    return planned;
                }
                None => {}
            }
            let ours = match (file.kind, existing) {
                (FileKind::Added, Some(ours)) => {
                    let wanted = patch::apply(b"", file).theirs;
                    if ours == wanted {
                        planned.result = Some(wanted);
                        return planned;
                    }
                    planned.state = State::Exists;
                    planned.theirs = Some(wanted.clone());
                    if let Some((text, _)) = patch::merge_with_markers(b"", &ours, &wanted, labels) {
                        planned.result = Some(text);
                    } else {
                        planned.state = State::Failed;
                        planned.reason = Some(t!("vcs-patch-reason-not-text"));
                    }
                    return planned;
                }
                (FileKind::Added, None) => Vec::new(),
                (_, Some(ours)) => ours,
                (_, None) => {
                    planned.state = State::Missing;
                    planned.failed = file
                        .hunks
                        .iter()
                        .enumerate()
                        .map(|(index, hunk)| FailedHunk {
                            hunk: index + 1,
                            line: hunk.old_start,
                            header: format!(
                                "@@ -{},{} +{},{} @@",
                                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
                            ),
                        })
                        .collect();
                    return planned;
                }
            };
            let applied = patch::apply(&ours, file);
            planned.approximate = applied
                .hunks
                .iter()
                .enumerate()
                .filter(|(_, hunk)| hunk.applied && !hunk.exact)
                .map(|(index, _)| index + 1)
                .collect();
            let moved = matches!(file.kind, FileKind::Renamed | FileKind::Copied) && source != target;
            let occupied = if moved { read(root, &target) } else { None };
            if let Some(there) = occupied {
                if !applied.clean() {
                    planned.state = State::Failed;
                    planned.reason = Some(t!("vcs-patch-reason-target-exists"));
                    planned.failed = failed_hunks(file, &applied);
                    return planned;
                }
                let wanted = applied.theirs;
                if there == wanted {
                    planned.result = Some(wanted);
                    return planned;
                }
                planned.state = State::Exists;
                planned.reason = Some(t!("vcs-patch-reason-target-exists"));
                planned.theirs = Some(wanted.clone());
                planned.result = patch::merge_with_markers(b"", &there, &wanted, labels).map(|(text, _)| text);
                if planned.result.is_none() {
                    planned.state = State::Failed;
                }
                planned.ours = Some(there);
                return planned;
            }
            if applied.clean() {
                if file.kind == FileKind::Deleted && !applied.theirs.is_empty() {
                    planned.state = State::Failed;
                    planned.reason = Some(t!("vcs-patch-reason-delete-differs"));
                    return planned;
                }
                planned.result = (file.kind != FileKind::Deleted).then_some(applied.theirs);
                return planned;
            }
            planned.failed = failed_hunks(file, &applied);
            let from_blob = file.old_blob().and_then(|id| repository.blob_named(id)).and_then(|base| {
                let theirs = patch::apply(&base, file);
                theirs.clean().then_some((base, theirs.theirs))
            });
            let (base, theirs) = from_blob.unwrap_or((applied.base, applied.theirs));
            match patch::merge_with_markers(&base, &ours, &theirs, labels) {
                Some((text, 0)) => {
                    planned.state = State::Merged;
                    planned.result = Some(text);
                }
                Some((text, _)) => {
                    planned.state = State::Conflict;
                    planned.result = Some(text);
                    planned.base = Some(base);
                    planned.theirs = Some(theirs);
                }
                None => {
                    planned.state = State::Failed;
                    planned.reason = Some(t!("vcs-patch-reason-not-text"));
                }
            }
            planned
        })
        .collect()
}

fn preview(repository: &Repository, loaded: &Loaded, planned: &[Planned]) -> PatchPreview {
    let mail = loaded.patch.mail.as_ref();
    PatchPreview {
        files: planned.iter().map(|file| file.info(repository.root())).collect(),
        author: mail.map(|mail| mail.author.clone()).filter(|author| !author.is_empty()),
        message: mail.map(Mail::message).filter(|message| !message.is_empty()),
        strip: loaded.strip,
        base_dir: loaded.base_dir.clone(),
        source: loaded.source.clone(),
    }
}

pub struct ReadPatch;

impl Command for ReadPatch {
    type Params = PatchInput;
    type Output = PatchPreview;
    const ID: &'static str = "vcs.patch.read";
    const TITLE: &'static str = "Apply Patch…";
    const DESCRIPTION: &'static str = "Reads a patch (`path`, `text`, or the clipboard with `clipboard`) and tells how each \
         of its files would apply to the working tree, without changing anything: `clean`, `merged` \
         (three-way), `conflict` (with the hunks that do not apply), `binary`, `missing`, `exists` or \
         `failed`. `baseDir` maps the patch's paths onto a folder (Map base directory), `strip` takes \
         leading folders off them, `reverse` undoes the patch. A patch made from a commit answers \
         with its author and message. Paths that leave the repository or go into .git are refused.";

    fn run(&self, session: &mut Session, input: PatchInput) -> Result<PatchPreview, CommandError> {
        let repository = repository(session)?;
        let loaded = load(session, &repository, &input)?;
        let planned = plan(&repository, &loaded);
        Ok(preview(&repository, &loaded, &planned))
    }
}

/// A file of a patch to show.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PatchDiffParams {
    /// The patch.
    #[serde(flatten)]
    pub input: PatchInput,
    /// The file (as vcs.patch.read names it: `relative`, or absolute).
    pub file: PathBuf,
}

/// Lines of a text, from 0, the end left out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LineRange {
    /// The first line.
    pub start: usize,
    /// The line after the last one.
    pub end: usize,
}

/// A file of a patch: the file now and after the patch, and where conflicts are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PatchDiffView {
    /// The diff: the file now (`before`) and with the patch (`after`, conflicts with markers).
    #[serde(flatten)]
    pub view: DiffView,
    /// The lines of `after` that are conflicts, markers included.
    pub conflicts: Vec<LineRange>,
    /// How it applies (as vcs.patch.read says).
    pub state: String,
}

pub fn conflict_lines(text: &str) -> Vec<LineRange> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, line) in text.lines().enumerate() {
        if line.starts_with("<<<<<<< ") && start.is_none() {
            start = Some(index);
        } else if line.starts_with(">>>>>>> ")
            && let Some(first) = start.take()
        {
            ranges.push(LineRange { start: first, end: index + 1 });
        }
    }
    ranges
}

pub struct PatchDiff;

impl Command for PatchDiff {
    type Params = PatchDiffParams;
    type Output = PatchDiffView;
    const ID: &'static str = "vcs.patch.diff";
    const TITLE: &'static str = "Show Diff";
    const DESCRIPTION: &'static str = "Compares a file of a patch (as vcs.patch.read names it) as it is now with how the \
         patch leaves it: both texts and the changed lines, as vcs.diff.file does, with `conflicts`, the \
         lines of the hunks that do not apply, between conflict markers.";

    fn run(&self, session: &mut Session, params: PatchDiffParams) -> Result<PatchDiffView, CommandError> {
        let repository = repository(session)?;
        let loaded = load(session, &repository, &params.input)?;
        let wanted = relative(&repository, &params.file)?;
        let planned = plan(&repository, &loaded);
        let file = planned
            .iter()
            .find(|file| file.target == wanted)
            .ok_or_else(|| CommandError::new(t!("vcs-patch-error-not-in-patch", path = wanted.as_str())))?;
        let before = file.ours.clone().unwrap_or_default();
        let after = if file.state == State::Binary { vec![0u8] } else { file.after().unwrap_or_default().to_vec() };
        let titles = (t!("vcs-patch-diff-current"), t!("vcs-patch-diff-patched"));
        let view = diff_view(absolute(repository.root(), &wanted), wanted, titles, &before, &after);
        let conflicts = conflict_lines(&view.after);
        Ok(PatchDiffView { view, conflicts, state: file.state.as_str().to_owned() })
    }
}

/// What to apply.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ApplyPatchParams {
    /// The patch.
    #[serde(flatten)]
    pub input: PatchInput,
    /// Only these of its files (as vcs.patch.read names them); all of them when empty.
    pub paths: Vec<PathBuf>,
    /// The changelist the changed files go into, made when there is none of that name; the active
    /// one when absent.
    pub changelist: Option<String>,
}

/// What a patch did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct AppliedPatch {
    /// The files changed, relative.
    pub applied: Vec<String>,
    /// The files left conflicted (their hunks to merge in the merge conflict editor), relative.
    pub conflicts: Vec<String>,
    /// The files not changed, with why.
    pub failed: Vec<PatchFile>,
    /// The changelist the changed files went into.
    pub changelist: String,
    /// For a patch made from a commit: its author, put in the commit window.
    pub author: Option<String>,
    /// For a patch made from a commit: its message, put in the commit window.
    pub message: Option<String>,
}

/// The commit message and author a patch made from a commit fills in, which the commit window
/// takes when `generation` changes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitDraft {
    /// The message.
    pub message: Option<String>,
    /// The author, `Name <email>`.
    pub author: Option<String>,
    /// Grows each time a patch fills them in.
    pub generation: u64,
}

pub struct ApplyPatch;

impl Command for ApplyPatch {
    type Params = ApplyPatchParams;
    type Output = AppliedPatch;
    const ID: &'static str = "vcs.patch.apply";
    const TITLE: &'static str = "Apply Patch";
    const DESCRIPTION: &'static str = "Applies a patch (`path`, `text` or `clipboard`, mapped as vcs.patch.read maps it) to \
         the working tree, all its files or only `paths`, into the changelist `changelist` (made when \
         missing; the active one by default). Hunks that do not apply as they are leave their file \
         conflicted, to merge in the merge conflict editor (vcs.conflict.open), and the window says so; \
         binary changes are applied by git. A patch made from a commit fills the commit window's \
         message and author in (vcs.commit.draft). Undo (vcs.history.undo) takes a clean apply back.";

    fn run(&self, session: &mut Session, params: ApplyPatchParams) -> Result<AppliedPatch, CommandError> {
        let repository = repository(session)?;
        let root = repository.root().to_owned();
        let loaded = load(session, &repository, &params.input)?;
        let wanted =
            params.paths.iter().map(|path| relative(&repository, path)).collect::<Result<Vec<_>, CommandError>>()?;
        let mut planned = plan(&repository, &loaded);
        if let Some(path) = wanted.iter().find(|path| !planned.iter().any(|file| &file.target == *path)) {
            return Err(CommandError::new(t!("vcs-patch-error-not-in-patch", path = path.as_str())));
        }
        planned.retain(|file| wanted.is_empty() || wanted.contains(&file.target));
        let (lists, _) = changelists::current(session)?;
        let target = match params.changelist.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) if lists.has(name) => name.to_owned(),
            Some(name) => {
                let create =
                    changelists::CreateParams { name: name.to_owned(), ..changelists::CreateParams::default() };
                changelists::CreateChangelist.run(session, create)?;
                name.to_owned()
            }
            None => lists.active().to_owned(),
        };
        let mut touched: Vec<String> = Vec::new();
        for file in &planned {
            for path in [&file.target, &file.source] {
                if !touched.contains(path) {
                    touched.push(path.clone());
                }
            }
        }
        save_documents(session, &repository, &touched)?;
        let targets: Vec<String> = planned.iter().map(|file| file.target.clone()).collect();
        changelists::assign(session, &root, &targets, &target);
        crate::hidden_files::guard_change(session, repository.root(), &touched)?;
        let recording = history::begin_with_files(session, &repository, touched.clone());
        let mut result = AppliedPatch { changelist: target, ..AppliedPatch::default() };
        crate::local_history::apply_patch(session, &root, &touched, || {
            let mut binary = Patch::default();
            for file in &planned {
                match file.state {
                    State::Clean | State::Merged => match write(&root, file) {
                        Ok(()) => result.applied.push(file.target.clone()),
                        Err(_) => result.failed.push(file.info(&root)),
                    },
                    State::Conflict | State::Exists => {
                        let mode = file.file.new_mode.as_deref().unwrap_or("100644");
                        let sides = [file.base.as_deref(), file.ours.as_deref(), file.theirs.as_deref()];
                        let text = file.result.as_deref().unwrap_or_default();
                        match repository.leave_conflicted(&file.target, sides, mode, text) {
                            Ok(()) => {
                                if file.file.kind == FileKind::Renamed && file.source != file.target {
                                    let _ = std::fs::remove_file(absolute(&root, &file.source));
                                }
                                result.conflicts.push(file.target.clone());
                            }
                            Err(_) => result.failed.push(file.info(&root)),
                        }
                    }
                    State::Binary => binary.files.push(file.file.clone()),
                    State::Missing | State::Failed => result.failed.push(file.info(&root)),
                }
            }
            if !binary.files.is_empty() {
                match repository.apply_git_patch(&binary.to_bytes()) {
                    Ok(()) => result.applied.extend(binary.files.iter().map(|file| file.path().to_owned())),
                    Err(_) => {
                        result.failed.extend(planned.iter().filter(|file| file.state == State::Binary).map(|file| {
                            let mut info = file.info(&root);
                            info.state = State::Failed.as_str().to_owned();
                            info
                        }))
                    }
                }
            }
        });
        let name = loaded
            .patch
            .mail
            .as_ref()
            .map(|mail| mail.subject.clone())
            .filter(|subject| !subject.is_empty())
            .unwrap_or_else(|| if loaded.source.is_empty() { t!("vcs-patch-name") } else { loaded.source.clone() });
        if result.conflicts.is_empty() && !result.applied.is_empty() {
            history::finish(
                session,
                &repository,
                recording,
                Done::new(Operation::ApplyPatch, name).count(result.applied.len()),
            );
        }
        reload(session, &repository, &touched);
        changed(session);
        if let Some(mail) = &loaded.patch.mail {
            result.author = Some(mail.author.clone()).filter(|author| !author.is_empty());
            result.message = Some(mail.message()).filter(|message| !message.is_empty());
            let draft = session.state_mut::<CommitDraft>();
            draft.message = result.message.clone();
            draft.author = result.author.clone();
            draft.generation += 1;
        }
        if !result.conflicts.is_empty() {
            let error =
                zen_vcs::VcsError::Conflicts { operation: zen_vcs::Applying::Patch, files: result.conflicts.clone() };
            session.state_mut::<GitWorkspace>().report_conflicts(message(&error));
        }
        if result.applied.is_empty() && result.conflicts.is_empty() {
            let names: Vec<&str> = result.failed.iter().map(|file| file.relative.as_str()).collect();
            return Err(CommandError::new(t!("vcs-patch-error-not-applied", files = names.join(", "))));
        }
        Ok(result)
    }
}

#[cfg(unix)]
fn set_mode(file: &Path, mode: Option<&str>) {
    use std::os::unix::fs::PermissionsExt;
    let Some(mode) = mode else { return };
    let Ok(metadata) = std::fs::metadata(file) else { return };
    let mut permissions = metadata.permissions();
    let bits = permissions.mode();
    let executable = mode == "100755";
    let wanted = if executable { bits | ((bits & 0o444) >> 2) } else { bits & !0o111 };
    if wanted != bits {
        permissions.set_mode(wanted);
        let _ = std::fs::set_permissions(file, permissions);
    }
}

#[cfg(not(unix))]
fn set_mode(_file: &Path, _mode: Option<&str>) {}

fn write(root: &Path, planned: &Planned) -> std::io::Result<()> {
    let refused = || std::io::Error::other("outside the repository");
    let target = checked_file(root, &planned.target).ok_or_else(refused)?;
    match &planned.result {
        Some(text) => {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if checked_file(root, &planned.target).is_none() {
                return Err(refused());
            }
            std::fs::write(&target, text)?;
            set_mode(&target, planned.file.new_mode.as_deref());
            if planned.file.kind == FileKind::Renamed && planned.source != planned.target {
                let source = checked_file(root, &planned.source).ok_or_else(refused)?;
                std::fs::remove_file(source)?;
            }
        }
        None => match std::fs::remove_file(&target) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        },
    }
    Ok(())
}

fn reload(session: &mut Session, repository: &Repository, paths: &[String]) {
    let workbench = session.state_mut::<Workbench>();
    for document in &mut workbench.documents {
        if relative(repository, document.path()).is_ok_and(|path| paths.contains(&path)) {
            document.reload_from_disk();
        }
    }
}

#[derive(Debug, Default)]
pub struct PatchDialogRequest {
    revision: Option<String>,
}

impl PatchDialogRequest {
    pub fn take(&mut self) -> Option<String> {
        self.revision.take()
    }

    pub(crate) fn ask(&mut self, revision: String) {
        self.revision = Some(revision);
    }
}

pub struct GetCommitDraft;

impl Command for GetCommitDraft {
    type Params = NoParams;
    type Output = CommitDraft;
    const ID: &'static str = "vcs.commit.draft";
    const TITLE: &'static str = "Commit Message from Patch";
    const DESCRIPTION: &'static str = "The commit message and author the last applied patch made from a commit filled into the \
         commit window (vcs.patch.apply); `generation` grows with each.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<CommitDraft, CommandError> {
        Ok(session.state::<CommitDraft>().cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests;
