use std::cmp::Ordering;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as Atomic};
use std::time::UNIX_EPOCH;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::{Workbench, display};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CompareFolders)?;
    registry.register(Synchronize)?;
    Ok(())
}

pub fn is_skipped(name: &str) -> bool {
    zen_vfs::HIDDEN.contains(&name) || zen_vfs::is_ignored(name)
}

/// How a file of one folder stands against the other folder's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EntryState {
    /// Only in the left folder.
    OnlyLeft,
    /// Only in the right folder.
    OnlyRight,
    /// In both, with different contents.
    Different,
    /// In both, with the same contents.
    Equal,
}

impl EntryState {
    pub const ALL: [EntryState; 4] =
        [EntryState::OnlyLeft, EntryState::OnlyRight, EntryState::Different, EntryState::Equal];
}

/// What an entry is on one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    /// A file.
    File,
    /// A folder, compared as a whole when it is on one side only.
    Folder,
    /// A symbolic link, compared by where it points.
    Link,
}

/// An entry on one side of the comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntrySide {
    /// What it is.
    pub kind: EntryKind,
    /// Its size in bytes (0 for a folder).
    pub size: u64,
    /// When it was last modified, in milliseconds since 1970.
    pub modified: Option<u64>,
    #[serde(skip)]
    #[schemars(skip)]
    stamp: Option<std::time::Duration>,
}

/// One row of the comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    /// The path relative to both folders, with `/` between names.
    pub relative: String,
    /// How the two sides stand.
    pub state: EntryState,
    /// The left side, when it has the entry.
    pub left: Option<EntrySide>,
    /// The right side, when it has the entry.
    pub right: Option<EntrySide>,
}

impl FolderEntry {
    pub fn name(&self) -> &str {
        self.relative.rsplit('/').next().unwrap_or(&self.relative)
    }

    pub fn default_action(&self) -> Option<SyncAction> {
        match self.state {
            EntryState::OnlyLeft | EntryState::Different => Some(SyncAction::ToRight),
            EntryState::OnlyRight => Some(SyncAction::ToLeft),
            EntryState::Equal => None,
        }
    }

    pub fn actions(&self) -> &'static [SyncAction] {
        match self.state {
            EntryState::OnlyLeft => &[SyncAction::ToRight, SyncAction::Delete],
            EntryState::OnlyRight => &[SyncAction::ToLeft, SyncAction::Delete],
            EntryState::Different => &[SyncAction::ToRight, SyncAction::ToLeft],
            EntryState::Equal => &[],
        }
    }
}

/// How many entries are in each state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StateCounts {
    /// Entries only in the left folder.
    pub only_left: usize,
    /// Entries only in the right folder.
    pub only_right: usize,
    /// Entries in both, different.
    pub different: usize,
    /// Entries in both, equal.
    pub equal: usize,
}

impl StateCounts {
    pub fn of(&self, state: EntryState) -> usize {
        match state {
            EntryState::OnlyLeft => self.only_left,
            EntryState::OnlyRight => self.only_right,
            EntryState::Different => self.different,
            EntryState::Equal => self.equal,
        }
    }

    fn add(&mut self, state: EntryState) {
        match state {
            EntryState::OnlyLeft => self.only_left += 1,
            EntryState::OnlyRight => self.only_right += 1,
            EntryState::Different => self.different += 1,
            EntryState::Equal => self.equal += 1,
        }
    }
}

/// Two folders compared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FolderComparison {
    /// The left folder.
    pub left: PathBuf,
    /// The right folder.
    pub right: PathBuf,
    /// The entries, in path order.
    pub entries: Vec<FolderEntry>,
    /// How many entries of the whole comparison are in each state, before any filter.
    pub counts: StateCounts,
    /// How many pairs of files had to be read to compare them (their size and time did not tell).
    pub contents_read: usize,
    /// How many entries hidden from agents were left out, for an agent only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_files: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Progress {
    cancel: AtomicBool,
    examined: AtomicUsize,
}

impl Progress {
    pub fn cancel(&self) {
        self.cancel.store(true, Atomic::Relaxed);
    }

    pub fn examined(&self) -> usize {
        self.examined.load(Atomic::Relaxed)
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Atomic::Relaxed)
    }
}

fn side_of(metadata: &fs::Metadata) -> EntrySide {
    let kind = if metadata.file_type().is_symlink() {
        EntryKind::Link
    } else if metadata.is_dir() {
        EntryKind::Folder
    } else {
        EntryKind::File
    };
    let stamp = metadata.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok());
    let modified = stamp.map(|since| since.as_millis() as u64);
    let size = if kind == EntryKind::Folder { 0 } else { metadata.len() };
    EntrySide { kind, size, modified, stamp }
}

fn listing(folder: &Path) -> Vec<(String, EntrySide)> {
    let Ok(read) = fs::read_dir(folder) else { return Vec::new() };
    let mut entries: Vec<(String, EntrySide)> = read
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_skipped(&name) {
                return None;
            }
            let metadata = fs::symlink_metadata(entry.path()).ok()?;
            Some((name, side_of(&metadata)))
        })
        .collect();
    entries.sort_by(|a, b| by_name(&a.0, &b.0));
    entries
}

fn by_name(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

fn fill(file: &mut fs::File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

fn same_contents(left: &Path, right: &Path) -> bool {
    let (Ok(mut a), Ok(mut b)) = (fs::File::open(left), fs::File::open(right)) else { return false };
    let (mut first, mut second) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let (Ok(read_a), Ok(read_b)) = (fill(&mut a, &mut first), fill(&mut b, &mut second)) else { return false };
        if read_a != read_b || first[..read_a] != second[..read_b] {
            return false;
        }
        if read_a == 0 {
            return true;
        }
    }
}

struct Walk<'a> {
    progress: &'a Progress,
    entries: Vec<FolderEntry>,
    counts: StateCounts,
    contents_read: usize,
}

impl Walk<'_> {
    fn push(&mut self, relative: String, state: EntryState, left: Option<EntrySide>, right: Option<EntrySide>) {
        self.counts.add(state);
        self.entries.push(FolderEntry { relative, state, left, right });
    }

    fn folder(&mut self, left: &Path, right: &Path, prefix: &str) {
        let (lefts, rights) = (listing(left), listing(right));
        let (mut i, mut j) = (0, 0);
        while i < lefts.len() || j < rights.len() {
            if self.progress.cancelled() {
                return;
            }
            self.progress.examined.fetch_add(1, Atomic::Relaxed);
            let order = match (lefts.get(i), rights.get(j)) {
                (Some(a), Some(b)) => by_name(&a.0, &b.0),
                (Some(_), None) => Ordering::Less,
                _ => Ordering::Greater,
            };
            match order {
                Ordering::Less => {
                    let (name, side) = &lefts[i];
                    self.push(format!("{prefix}{name}"), EntryState::OnlyLeft, Some(*side), None);
                    i += 1;
                }
                Ordering::Greater => {
                    let (name, side) = &rights[j];
                    self.push(format!("{prefix}{name}"), EntryState::OnlyRight, None, Some(*side));
                    j += 1;
                }
                Ordering::Equal => {
                    let ((name, a), (_, b)) = (&lefts[i], &rights[j]);
                    let relative = format!("{prefix}{name}");
                    let (left_path, right_path) = (left.join(name), right.join(name));
                    match (a.kind, b.kind) {
                        (EntryKind::Folder, EntryKind::Folder) => {
                            self.folder(&left_path, &right_path, &format!("{relative}/"));
                        }
                        (EntryKind::File, EntryKind::File) => {
                            let state = self.files(&left_path, &right_path, a, b);
                            self.push(relative, state, Some(*a), Some(*b));
                        }
                        (EntryKind::Link, EntryKind::Link) => {
                            let same = fs::read_link(&left_path).ok() == fs::read_link(&right_path).ok();
                            let state = if same { EntryState::Equal } else { EntryState::Different };
                            self.push(relative, state, Some(*a), Some(*b));
                        }
                        _ => self.push(relative, EntryState::Different, Some(*a), Some(*b)),
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
    }

    fn files(&mut self, left: &Path, right: &Path, a: &EntrySide, b: &EntrySide) -> EntryState {
        if a.size != b.size {
            return EntryState::Different;
        }
        if a.stamp.is_some() && a.stamp == b.stamp {
            return EntryState::Equal;
        }
        self.contents_read += 1;
        if same_contents(left, right) { EntryState::Equal } else { EntryState::Different }
    }
}

fn not_a_folder(path: &Path) -> String {
    t!("workbench-error-compare-not-folder", path = display(path))
}

pub fn compare(left: &Path, right: &Path, progress: &Progress) -> Result<FolderComparison, String> {
    for root in [left, right] {
        if !root.is_dir() {
            return Err(not_a_folder(root));
        }
    }
    let mut walk = Walk { progress, entries: Vec::new(), counts: StateCounts::default(), contents_read: 0 };
    walk.folder(left, right, "");
    Ok(FolderComparison {
        left: left.to_owned(),
        right: right.to_owned(),
        entries: walk.entries,
        counts: walk.counts,
        contents_read: walk.contents_read,
        hidden_files: None,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameFilter {
    alternatives: Vec<Vec<(bool, String)>>,
}

impl NameFilter {
    pub fn parse(text: &str) -> Self {
        let alternatives = text
            .split('|')
            .map(|alternative| {
                alternative
                    .split('&')
                    .map(str::trim)
                    .filter(|term| !term.is_empty() && *term != "!")
                    .map(|term| match term.strip_prefix('!') {
                        Some(rest) => (true, rest.trim().to_lowercase()),
                        None => (false, term.to_lowercase()),
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|terms| !terms.is_empty())
            .collect();
        Self { alternatives }
    }

    pub fn matches(&self, relative: &str) -> bool {
        if self.alternatives.is_empty() {
            return true;
        }
        let path = relative.to_lowercase();
        let name = path.rsplit('/').next().unwrap_or(&path);
        self.alternatives.iter().any(|terms| {
            terms.iter().all(|(negated, term)| {
                let found = if term.contains(['*', '?']) {
                    wildcard(term.as_bytes(), if term.contains('/') { path.as_bytes() } else { name.as_bytes() })
                } else {
                    path.contains(term.as_str())
                };
                found != *negated
            })
        })
    }
}

fn wildcard(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if let Some((at, from)) = star {
            p = at + 1;
            t = from + 1;
            star = Some((at, from + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|byte| *byte == b'*')
}

/// Two folders to compare, and which of their entries to answer with.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FoldersParams {
    /// The left folder: absolute, or relative to the project.
    pub left: PathBuf,
    /// The right folder: absolute, or relative to the project.
    pub right: PathBuf,
    /// Only the entries in these states; every state when absent.
    #[serde(default)]
    pub states: Option<Vec<EntryState>>,
    /// Only the entries that pass this filter (`|` or, `&` and, `!` not, `*` and `?` wildcards).
    #[serde(default)]
    pub filter: String,
}

pub struct CompareFolders;

impl Command for CompareFolders {
    type Params = FoldersParams;
    type Output = FolderComparison;
    const ID: &'static str = "file.compare.folders";
    const TITLE: &'static str = "Compare Directories";
    const DESCRIPTION: &'static str = "Compares two folders and everything in them, as Compare Directories shows them: \
         each entry's `relative` path, its `state` (`onlyLeft`, `onlyRight`, `different`, `equal`) and each side's \
         kind, size and modification time; `counts` of every state. Files compare by contents, size and time \
         first; a folder on one side only is one entry; names the IDE ignores are left out; links are not \
         followed. `states` and `filter` (`*.go & !*_test.go | README`) narrow the entries answered.";

    fn run(&self, session: &mut Session, params: FoldersParams) -> Result<FolderComparison, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let (left, right) = (workbench.resolve(&params.left), workbench.resolve(&params.right));
        let root = workbench.project().map(Path::to_path_buf);
        let mut comparison = compare(&left, &right, &Progress::default()).map_err(CommandError::new)?;
        if let Some(patterns) = crate::hidden_files::for_caller(session) {
            leave_hidden_out(&mut comparison, &patterns, root.as_deref());
        }
        let filter = NameFilter::parse(&params.filter);
        comparison.entries.retain(|entry| {
            params.states.as_ref().is_none_or(|states| states.contains(&entry.state)) && filter.matches(&entry.relative)
        });
        Ok(comparison)
    }
}

pub fn is_hidden_entry(left: &Path, right: &Path, relative: &str, patterns: &[String], root: Option<&Path>) -> bool {
    [left, right].into_iter().any(|base| {
        let path = base.join(relative);
        crate::hidden_files::is_hidden(patterns, root, &path)
            || crate::hidden_files::hidden_inside(patterns, root, &path).is_some()
    })
}

pub fn leave_hidden_out(comparison: &mut FolderComparison, patterns: &[String], root: Option<&Path>) {
    let (left, right) = (comparison.left.clone(), comparison.right.clone());
    let before = comparison.entries.len();
    comparison.entries.retain(|entry| !is_hidden_entry(&left, &right, &entry.relative, patterns, root));
    comparison.counts = StateCounts::default();
    for entry in &comparison.entries {
        comparison.counts.add(entry.state);
    }
    comparison.hidden_files = Some(before - comparison.entries.len());
}

/// What synchronising does to an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SyncAction {
    /// Copies the left entry over to the right folder (→).
    ToRight,
    /// Copies the right entry over to the left folder (←).
    ToLeft,
    /// Deletes the entry from the one side that has it.
    Delete,
}

/// An entry and what to do with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncItem {
    /// The path relative to both folders, with `/` between names.
    pub relative: String,
    /// What to do.
    pub action: SyncAction,
}

/// What to synchronise between two folders.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncParams {
    /// The left folder: absolute, or relative to the project.
    pub left: PathBuf,
    /// The right folder: absolute, or relative to the project.
    pub right: PathBuf,
    /// The entries and their actions.
    pub items: Vec<SyncItem>,
    /// Overwrite and delete without asking; without it, nothing is done when an item would.
    #[serde(default)]
    pub confirm: bool,
}

/// An entry that could not be synchronised.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncFailure {
    /// The entry's path.
    pub relative: String,
    /// Why.
    pub error: String,
}

/// What synchronising did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    /// The items done.
    pub applied: Vec<SyncItem>,
    /// The items that would overwrite or delete, when nothing was done for want of `confirm`.
    pub needs_confirmation: Vec<SyncItem>,
    /// The items that failed.
    pub failed: Vec<SyncFailure>,
}

pub fn inside(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    let normal = !relative.is_empty() && path.components().all(|part| matches!(part, Component::Normal(_)));
    if !normal || relative.contains('\\') {
        return Err(t!("workbench-error-sync-outside", path = relative));
    }
    let mut at = root.to_owned();
    for part in path.components() {
        if let Ok(metadata) = fs::symlink_metadata(&at)
            && at != root
            && (metadata.file_type().is_symlink() || !metadata.is_dir())
        {
            return Err(t!("workbench-error-sync-outside", path = relative));
        }
        at.push(part);
    }
    Ok(at)
}

fn remove(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) }
}

fn copy(from: &Path, to: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(from)?;
    let target = fs::symlink_metadata(to).ok();
    let folder = metadata.is_dir();
    let replaced = target.as_ref().is_some_and(|target| {
        if folder {
            !target.is_dir()
        } else if metadata.is_file() {
            !target.is_file()
        } else {
            true
        }
    });
    if replaced {
        remove(to)?;
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if metadata.file_type().is_symlink() {
        let pointed = fs::read_link(from)?;
        #[cfg(unix)]
        return std::os::unix::fs::symlink(pointed, to);
        #[cfg(not(unix))]
        return Err(io::Error::new(io::ErrorKind::Unsupported, format!("a link to {}", pointed.display())));
    }
    if metadata.is_dir() {
        if !to.is_dir() {
            fs::create_dir(to)?;
        }
        for entry in fs::read_dir(to)?.flatten() {
            let name = entry.file_name();
            if !is_skipped(&name.to_string_lossy()) && fs::symlink_metadata(from.join(&name)).is_err() {
                remove(&entry.path())?;
            }
        }
        for entry in fs::read_dir(from)?.flatten() {
            let name = entry.file_name();
            if !is_skipped(&name.to_string_lossy()) {
                copy(&entry.path(), &to.join(&name))?;
            }
        }
        return Ok(());
    }
    let name = to.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let temporary = to.with_file_name(format!(".{name}.zen-sync"));
    fs::copy(from, &temporary)?;
    let stamped =
        metadata.modified().and_then(|time| fs::File::options().write(true).open(&temporary)?.set_modified(time));
    let renamed = stamped.and_then(|()| fs::rename(&temporary, to));
    if renamed.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    renamed
}

struct Planned {
    item: SyncItem,
    from: Option<PathBuf>,
    to: PathBuf,
    destructive: bool,
}

fn plan(left: &Path, right: &Path, item: &SyncItem) -> Result<Planned, String> {
    let (on_left, on_right) = (inside(left, &item.relative)?, inside(right, &item.relative)?);
    let exists = |path: &Path| fs::symlink_metadata(path).is_ok();
    let missing = || t!("workbench-error-sync-missing", path = item.relative.as_str());
    match item.action {
        SyncAction::ToRight | SyncAction::ToLeft => {
            let (from, to) = if item.action == SyncAction::ToRight { (on_left, on_right) } else { (on_right, on_left) };
            if !exists(&from) {
                return Err(missing());
            }
            let destructive = exists(&to);
            Ok(Planned { item: item.clone(), from: Some(from), to, destructive })
        }
        SyncAction::Delete => match (exists(&on_left), exists(&on_right)) {
            (true, false) => Ok(Planned { item: item.clone(), from: None, to: on_left, destructive: true }),
            (false, true) => Ok(Planned { item: item.clone(), from: None, to: on_right, destructive: true }),
            (true, true) => Err(t!("workbench-error-sync-delete-both", path = item.relative.as_str())),
            (false, false) => Err(missing()),
        },
    }
}

pub fn synchronize(left: &Path, right: &Path, items: &[SyncItem], confirm: bool) -> SyncResult {
    let mut result = SyncResult::default();
    let mut planned = Vec::new();
    for item in items {
        match plan(left, right, item) {
            Ok(step) => planned.push(step),
            Err(error) => result.failed.push(SyncFailure { relative: item.relative.clone(), error }),
        }
    }
    if !confirm {
        result.needs_confirmation =
            planned.iter().filter(|step| step.destructive).map(|step| step.item.clone()).collect();
        if !result.needs_confirmation.is_empty() {
            return result;
        }
    }
    for step in planned {
        let done = match &step.from {
            Some(from) => copy(from, &step.to),
            None => remove(&step.to),
        };
        match done {
            Ok(()) => result.applied.push(step.item),
            Err(error) => {
                result.failed.push(SyncFailure { relative: step.item.relative.clone(), error: error.to_string() })
            }
        }
    }
    result
}

pub struct Synchronize;

impl Command for Synchronize {
    type Params = SyncParams;
    type Output = SyncResult;
    const ID: &'static str = "file.compare.synchronize";
    const TITLE: &'static str = "Synchronize Directories";
    const DESCRIPTION: &'static str = "Synchronises entries of two compared folders (file.compare.folders): each item's \
         `action` copies it to the right (`toRight`, →) or to the left (`toLeft`, ←), replacing what is there, or \
         deletes it from the one side that has it (`delete`). Without `confirm: true` nothing is done when an item \
         would overwrite or delete: they are answered in `needsConfirmation` for the user to confirm. Paths stay \
         inside the two folders and links are copied as links, never followed. Answers `applied` and `failed`.";

    fn run(&self, session: &mut Session, params: SyncParams) -> Result<SyncResult, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let (left, right) = (workbench.resolve(&params.left), workbench.resolve(&params.right));
        let project = workbench.project().map(Path::to_path_buf).unwrap_or_default();
        for root in [&left, &right] {
            if !root.is_dir() {
                return Err(CommandError::new(not_a_folder(root)));
            }
        }
        let touched: Vec<PathBuf> =
            params.items.iter().flat_map(|item| [left.join(&item.relative), right.join(&item.relative)]).collect();
        crate::hidden_files::guard_change(session, &project, &touched)?;
        Ok(synchronize(&left, &right, &params.items, params.confirm))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn age(path: &Path, seconds: u64) {
        let time = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
        fs::File::options().write(true).open(path).unwrap().set_modified(time).unwrap();
    }

    fn state(comparison: &FolderComparison, relative: &str) -> Option<EntryState> {
        comparison.entries.iter().find(|entry| entry.relative == relative).map(|entry| entry.state)
    }

    fn pair() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (left, right) = (dir.path().join("left"), dir.path().join("right"));
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        (dir, left, right)
    }

    #[test]
    fn a_folder_copied_over_a_folder_keeps_what_the_comparison_skips() {
        let (_dir, left, right) = pair();
        write(&left, "app/main.go", "package main\n");
        write(&left, "app/sub/util.go", "package sub\n");
        write(&right, "app/main.go", "package old\n");
        write(&right, "app/stale.go", "package stale\n");
        write(&right, "app/.git/HEAD", "ref: refs/heads/main\n");
        copy(&left.join("app"), &right.join("app")).unwrap();
        assert_eq!(fs::read_to_string(right.join("app/main.go")).unwrap(), "package main\n");
        assert_eq!(fs::read_to_string(right.join("app/sub/util.go")).unwrap(), "package sub\n");
        assert!(!right.join("app/stale.go").exists(), "what the source does not have goes");
        assert!(right.join("app/.git/HEAD").is_file(), "the skipped .git stays");
        write(&left, "both", "file\n");
        write(&right, "both/inner.txt", "inner\n");
        copy(&left.join("both"), &right.join("both")).unwrap();
        assert_eq!(fs::read_to_string(right.join("both")).unwrap(), "file\n");
        write(&right, "lib", "a file\n");
        write(&left, "lib/a.go", "package lib\n");
        copy(&left.join("lib"), &right.join("lib")).unwrap();
        assert!(right.join("lib/a.go").is_file());
    }

    #[test]
    fn entries_are_only_left_only_right_different_or_equal_and_ignored_names_are_skipped() {
        let (_dir, left, right) = pair();
        write(&left, "same.txt", "one");
        write(&right, "same.txt", "one");
        write(&left, "changed.txt", "one");
        write(&right, "changed.txt", "two");
        age(&right.join("changed.txt"), 60);
        write(&left, "longer.txt", "one");
        write(&right, "longer.txt", "one two");
        write(&left, "src/main.go", "package main");
        write(&right, "src/main.go", "package main");
        write(&left, "src/only.go", "x");
        write(&right, "docs/readme.md", "y");
        write(&left, ".git/HEAD", "ref");
        write(&right, ".DS_Store", "z");
        let comparison = compare(&left, &right, &Progress::default()).unwrap();
        assert_eq!(state(&comparison, "same.txt"), Some(EntryState::Equal));
        assert_eq!(state(&comparison, "changed.txt"), Some(EntryState::Different));
        assert_eq!(state(&comparison, "longer.txt"), Some(EntryState::Different));
        assert_eq!(state(&comparison, "src/main.go"), Some(EntryState::Equal));
        assert_eq!(state(&comparison, "src/only.go"), Some(EntryState::OnlyLeft));
        assert_eq!(state(&comparison, "docs"), Some(EntryState::OnlyRight), "a folder on one side is one entry");
        assert_eq!(state(&comparison, "docs/readme.md"), None);
        assert_eq!(state(&comparison, ".git"), None);
        assert_eq!(state(&comparison, ".DS_Store"), None);
        assert_eq!(
            comparison.counts,
            StateCounts { only_left: 1, only_right: 1, different: 2, equal: 2 },
            "{:?}",
            comparison.entries
        );
        let order: Vec<&str> = comparison.entries.iter().map(|entry| entry.relative.as_str()).collect();
        assert_eq!(order, ["changed.txt", "docs", "longer.txt", "same.txt", "src/main.go", "src/only.go"]);
        assert!(compare(&left, &left.join("same.txt"), &Progress::default()).is_err(), "a file is not a folder");
    }

    #[test]
    fn size_and_time_are_the_fast_path_and_contents_decide_the_rest() {
        let (_dir, left, right) = pair();
        write(&left, "a.txt", "abc");
        write(&right, "a.txt", "abd");
        write(&left, "b.txt", "same");
        write(&right, "b.txt", "same");
        write(&left, "c.txt", "short");
        write(&right, "c.txt", "longer");
        let stamp = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        for root in [&left, &right] {
            for name in ["a.txt", "b.txt"] {
                fs::File::options().write(true).open(root.join(name)).unwrap().set_modified(stamp).unwrap();
            }
        }
        let comparison = compare(&left, &right, &Progress::default()).unwrap();
        assert_eq!(state(&comparison, "a.txt"), Some(EntryState::Equal), "same size and time: not read");
        assert_eq!(state(&comparison, "c.txt"), Some(EntryState::Different));
        assert_eq!(comparison.contents_read, 0, "sizes differ or times match: no file was read");
        let later = stamp + std::time::Duration::from_secs(60);
        fs::File::options().write(true).open(right.join("a.txt")).unwrap().set_modified(later).unwrap();
        fs::File::options().write(true).open(right.join("b.txt")).unwrap().set_modified(later).unwrap();
        let comparison = compare(&left, &right, &Progress::default()).unwrap();
        assert_eq!(state(&comparison, "a.txt"), Some(EntryState::Different), "times differ: the contents decide");
        assert_eq!(state(&comparison, "b.txt"), Some(EntryState::Equal));
        assert_eq!(comparison.contents_read, 2);
    }

    #[test]
    fn a_cancelled_comparison_stops() {
        let (_dir, left, right) = pair();
        for index in 0..50 {
            write(&left, &format!("f{index}.txt"), "x");
        }
        let progress = Progress::default();
        progress.cancel();
        let comparison = compare(&left, &right, &progress).unwrap();
        assert!(comparison.entries.is_empty());
    }

    #[test]
    fn the_text_filter_takes_not_and_or() {
        let filter = NameFilter::parse("*.go & !*_test.go | readme");
        assert!(filter.matches("src/main.go"));
        assert!(!filter.matches("src/main_test.go"));
        assert!(filter.matches("docs/README.md"));
        assert!(!filter.matches("Cargo.toml"));
        assert!(NameFilter::parse("").matches("anything"));
        assert!(NameFilter::parse("  |  ").matches("anything"));
        assert!(NameFilter::parse("!*.txt").matches("a.md") && !NameFilter::parse("!*.txt").matches("a.txt"));
        assert!(NameFilter::parse("src/*.go").matches("src/a.go") && !NameFilter::parse("src/*.go").matches("a.go"));
        assert!(NameFilter::parse("a?c").matches("x/abc") && !NameFilter::parse("a?c").matches("x/abbc"));
        assert!(NameFilter::parse("SRC").matches("src/x"), "case does not count");
    }

    #[test]
    fn paths_outside_the_folders_or_through_links_are_refused() {
        let (dir, left, _right) = pair();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        for bad in ["", "../outside/x", "/etc/passwd", "a/../../x", "./a", "a\\b"] {
            assert!(inside(&left, bad).is_err(), "{bad}");
        }
        assert_eq!(inside(&left, "a/b.txt").unwrap(), left.join("a/b.txt"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, left.join("link")).unwrap();
            assert!(inside(&left, "link/x.txt").is_err(), "a link is not followed out of the folder");
            assert_eq!(inside(&left, "link").unwrap(), left.join("link"), "the link itself is an entry");
        }
    }

    #[test]
    fn synchronising_copies_and_deletes_and_asks_before_overwriting_or_deleting() {
        let (_dir, left, right) = pair();
        write(&left, "new.txt", "new");
        write(&left, "tree/a.txt", "a");
        write(&left, "tree/.DS_Store", "junk");
        write(&left, "changed.txt", "left");
        write(&right, "changed.txt", "right!");
        write(&right, "gone.txt", "bye");
        let items = |pairs: &[(&str, SyncAction)]| -> Vec<SyncItem> {
            pairs
                .iter()
                .map(|(relative, action)| SyncItem { relative: (*relative).to_owned(), action: *action })
                .collect()
        };
        let plain = synchronize(
            &left,
            &right,
            &items(&[("new.txt", SyncAction::ToRight), ("tree", SyncAction::ToRight)]),
            false,
        );
        assert_eq!(plain.applied.len(), 2, "nothing overwritten: done at once: {plain:?}");
        assert_eq!(fs::read_to_string(right.join("new.txt")).unwrap(), "new");
        assert_eq!(fs::read_to_string(right.join("tree/a.txt")).unwrap(), "a");
        assert!(!right.join("tree/.DS_Store").exists(), "ignored names are not copied");
        let comparison = compare(&left, &right, &Progress::default()).unwrap();
        assert_eq!(state(&comparison, "new.txt"), Some(EntryState::Equal));
        assert_eq!(comparison.contents_read, 0, "a copy keeps its time: equal by the fast path");

        let risky = items(&[("changed.txt", SyncAction::ToRight), ("gone.txt", SyncAction::Delete)]);
        let asked = synchronize(&left, &right, &risky, false);
        assert!(asked.applied.is_empty());
        assert_eq!(asked.needs_confirmation, risky);
        assert_eq!(fs::read_to_string(right.join("changed.txt")).unwrap(), "right!", "nothing done before confirming");
        assert!(right.join("gone.txt").exists());
        let done = synchronize(&left, &right, &risky, true);
        assert_eq!(done.applied, risky);
        assert_eq!(fs::read_to_string(right.join("changed.txt")).unwrap(), "left");
        assert!(!right.join("gone.txt").exists());
        assert!(!right.join(".changed.txt.zen-sync").exists());

        let back = synchronize(&left, &right, &items(&[("changed.txt", SyncAction::ToLeft)]), true);
        assert_eq!(back.applied.len(), 1);
        let failed = synchronize(
            &left,
            &right,
            &items(&[
                ("../escape.txt", SyncAction::ToRight),
                ("changed.txt", SyncAction::Delete),
                ("nope", SyncAction::ToLeft),
            ]),
            true,
        );
        assert_eq!(failed.failed.len(), 3, "{failed:?}");
        assert!(!left.parent().unwrap().join("escape.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn links_are_compared_and_copied_as_links_and_never_written_through() {
        let (dir, left, right) = pair();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        write(&outside, "secret.txt", "keep");
        std::os::unix::fs::symlink(outside.join("secret.txt"), right.join("note.txt")).unwrap();
        write(&left, "note.txt", "overwrite?");
        std::os::unix::fs::symlink(&outside, left.join("out")).unwrap();
        let comparison = compare(&left, &right, &Progress::default()).unwrap();
        assert_eq!(state(&comparison, "note.txt"), Some(EntryState::Different));
        assert_eq!(state(&comparison, "out"), Some(EntryState::OnlyLeft));
        assert_eq!(
            comparison.entries.iter().find(|entry| entry.relative == "out").unwrap().left.unwrap().kind,
            EntryKind::Link
        );
        let done = synchronize(
            &left,
            &right,
            &[
                SyncItem { relative: "note.txt".to_owned(), action: SyncAction::ToRight },
                SyncItem { relative: "out".to_owned(), action: SyncAction::ToRight },
            ],
            true,
        );
        assert_eq!(done.applied.len(), 2, "{done:?}");
        assert_eq!(
            fs::read_to_string(outside.join("secret.txt")).unwrap(),
            "keep",
            "the link was replaced, not written through"
        );
        assert_eq!(fs::read_to_string(right.join("note.txt")).unwrap(), "overwrite?");
        assert_eq!(fs::read_link(right.join("out")).unwrap(), outside);
        let deleted =
            synchronize(&left, &right, &[SyncItem { relative: "out".to_owned(), action: SyncAction::ToLeft }], true);
        assert_eq!(deleted.applied.len(), 1);
        assert!(outside.join("secret.txt").exists(), "replacing a link to a folder leaves the folder alone");
    }

    #[test]
    fn the_commands_compare_filter_and_synchronise() {
        let (_dir, left, right) = pair();
        write(&left, "main.go", "a");
        write(&left, "main_test.go", "t");
        write(&right, "main.go", "b");
        age(&right.join("main.go"), 60);
        write(&right, "README.md", "r");
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let all =
            registry.dispatch(&mut session, "file.compare.folders", json!({ "left": left, "right": right })).unwrap();
        assert_eq!(all["counts"], json!({ "onlyLeft": 1, "onlyRight": 1, "different": 1, "equal": 0 }));
        assert_eq!(all["entries"].as_array().unwrap().len(), 3);
        let narrowed = registry
            .dispatch(
                &mut session,
                "file.compare.folders",
                json!({ "left": left, "right": right, "states": ["onlyLeft", "different"], "filter": "*.go & !*_test*" }),
            )
            .unwrap();
        assert_eq!(narrowed["entries"].as_array().unwrap().len(), 1, "{narrowed}");
        assert_eq!(narrowed["entries"][0]["relative"], "main.go");
        assert_eq!(narrowed["counts"]["onlyRight"], 1, "counts are of the whole comparison");
        let asked = registry
            .dispatch(
                &mut session,
                "file.compare.synchronize",
                json!({ "left": left, "right": right, "items": [{ "relative": "main.go", "action": "toRight" }] }),
            )
            .unwrap();
        assert_eq!(asked["needsConfirmation"].as_array().unwrap().len(), 1);
        let done = registry
            .dispatch(
                &mut session,
                "file.compare.synchronize",
                json!({ "left": left, "right": right, "confirm": true, "items": [{ "relative": "main.go", "action": "toRight" }] }),
            )
            .unwrap();
        assert_eq!(done["applied"].as_array().unwrap().len(), 1);
        assert_eq!(fs::read_to_string(right.join("main.go")).unwrap(), "a");
        assert!(
            registry
                .dispatch(&mut session, "file.compare.folders", json!({ "left": left, "right": right.join("main.go") }))
                .is_err()
        );
    }

    #[test]
    fn an_agent_is_told_nothing_of_hidden_files_and_cannot_synchronise_them() {
        let (_dir, left, right) = pair();
        write(&left, ".env", "TOKEN=a");
        write(&right, ".env", "TOKEN=b");
        write(&left, "deploy/.env", "TOKEN=c");
        write(&left, "main.go", "a");
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
        let params = json!({ "left": left, "right": right });
        let user = registry.dispatch(&mut session, "file.compare.folders", params.clone()).unwrap();
        assert_eq!(user["entries"].as_array().unwrap().len(), 3, "{user}");
        assert!(user.get("hiddenFiles").is_none());
        let (told, refused) = zen_core::macros::as_agent(&mut session, |session| {
            let told = registry.dispatch(session, "file.compare.folders", params.clone()).unwrap();
            let items = json!([{ "relative": ".env", "action": "toRight" }]);
            let refused = registry.dispatch(
                session,
                "file.compare.synchronize",
                json!({ "left": left, "right": right, "confirm": true, "items": items }),
            );
            (told, refused)
        });
        assert!(!told.to_string().contains(".env") && !told.to_string().contains("deploy"), "{told}");
        assert_eq!(told["hiddenFiles"], 2);
        assert_eq!(told["counts"], json!({ "onlyLeft": 1, "onlyRight": 0, "different": 0, "equal": 0 }));
        assert!(refused.is_err());
        assert_eq!(fs::read_to_string(right.join(".env")).unwrap(), "TOKEN=b");
    }
}
