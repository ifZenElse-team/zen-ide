pub mod store;

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_text::Text;

pub use store::{Entry, Limits, Pruned, Store, VersionKind};

use crate::Workbench;
use crate::vcs::commit::DiffView;

pub const REPLACE_IN_FILES: &str = "replaceInFiles";
pub const RENAME: &str = "rename";
pub const ROLLBACK: &str = "rollback";
pub const REVERT: &str = "revert";
pub const APPLY_PATCH: &str = "applyPatch";
pub const LINE_SEPARATORS: &str = "lineSeparators";

pub const BYTE_ORDER_MARK: &str = "byteOrderMark";
pub const FILE_ENCODING: &str = "fileEncoding";
pub const EDITORCONFIG_EXPORT: &str = "editorConfigExport";
pub const PREVIEW_EDIT: &str = "previewEdit";
pub const FILE_EDIT: &str = "fileEdit";
pub const EXTRACT: &str = "extract";
pub const SAFE_DELETE: &str = "safeDelete";
pub const MOVE: &str = "move";
pub const CHANGE_SIGNATURE: &str = "changeSignature";
pub const PULL_MEMBERS_UP: &str = "pullMembersUp";
pub const PUSH_MEMBERS_DOWN: &str = "pushMembersDown";
pub const EXTRACT_SUPERCLASS: &str = "extractSuperclass";
pub const EXTRACT_INTERFACE: &str = "extractInterface";
pub const ENCAPSULATE_FIELDS: &str = "encapsulateFields";
pub const INTRODUCE_PARAMETER_OBJECT: &str = "introduceParameterObject";

pub(crate) type Written = Vec<(PathBuf, Option<Vec<u8>>, Option<Vec<u8>>)>;

#[cfg(not(target_arch = "wasm32"))]
const PRUNE_EVERY: Duration = Duration::from_secs(3600);
const QUERY_WAIT: Duration = Duration::from_secs(30);
pub const QUIT_WAIT: Duration = Duration::from_secs(2);
const DEFAULT_MAX_BYTES: usize = 1 << 20;
const DEFAULT_LIMIT: usize = 1000;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListVersions)?;
    registry.register(GetVersion)?;
    registry.register(DiffVersion)?;
    registry.register(RevertVersion)?;
    registry.register(PutLabel)?;
    registry.register(RecordChanges)?;
    Ok(())
}

type Query = Box<dyn FnOnce(&mut Store) + Send>;

enum Content {
    Bytes(Vec<u8>),
    Text(Text),
    Read(PathBuf),
}

impl Content {
    fn bytes(&mut self) -> Option<&[u8]> {
        if let Self::Text(text) = self {
            *self = Self::Bytes(text.to_string().into_bytes());
        }
        match self {
            Self::Bytes(bytes) => Some(bytes),
            Self::Text(_) | Self::Read(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SavedText {
    pub(crate) time: u64,
    pub(crate) before: Text,
    pub(crate) after: Text,
    pub(crate) encoding: Option<(crate::charsets::Charset, bool)>,
}

impl SavedText {
    fn content(&self, text: Text) -> Content {
        match self.encoding {
            Some((charset, bom)) => match charset.encode(&text.to_string(), bom) {
                Ok(bytes) => Content::Bytes(bytes),
                Err(_) => Content::Text(text),
            },
            None => Content::Text(text),
        }
    }
}

struct Record {
    root: PathBuf,
    relative: String,
    kind: VersionKind,
    label: Option<String>,
    content: Content,
    time: u64,
    dedupe: bool,
}

enum Job {
    Record(Record),
    Query(Query),
}

enum Backend {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Worker(Worker),
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Inline(Mutex<Store>),
}

struct Worker {
    sender: Option<std::sync::mpsc::Sender<Job>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != std::thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

#[derive(Clone, Default)]
pub struct LocalHistory {
    backend: Option<Arc<Backend>>,
    limits: Limits,
}

impl std::fmt::Debug for LocalHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHistory").field("enabled", &self.is_enabled()).finish()
    }
}

impl LocalHistory {
    pub fn open(folder: PathBuf, limits: Limits) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (sender, receiver) = std::sync::mpsc::channel::<Job>();
            let worker_folder = folder.clone();
            let worker = std::thread::Builder::new().name("local-history".to_owned()).spawn(move || {
                let mut store = Store::open(&worker_folder, limits);
                let _ = store.prune(now());
                let mut ahead = VecDeque::new();
                loop {
                    let job = match ahead.pop_front() {
                        Some(job) => job,
                        None => match receiver.recv_timeout(PRUNE_EVERY) {
                            Ok(job) => job,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                let _ = store.prune(now());
                                continue;
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        },
                    };
                    match job {
                        Job::Record(record) => record_now(&mut store, record, Some((&mut ahead, &receiver))),
                        Job::Query(query) => {
                            store.refresh();
                            query(&mut store);
                        }
                    }
                }
            });
            if let Ok(thread) = worker {
                let worker = Worker { sender: Some(sender), thread: Some(thread) };
                return Self { backend: Some(Arc::new(Backend::Worker(worker))), limits };
            }
        }
        let mut store = Store::open(&folder, limits);
        let _ = store.prune(now());
        Self { backend: Some(Arc::new(Backend::Inline(Mutex::new(store)))), limits }
    }

    pub fn is_enabled(&self) -> bool {
        self.backend.is_some()
    }

    fn submit(&self, job: Job) {
        match self.backend.as_deref() {
            Some(Backend::Worker(worker)) => {
                if let Some(sender) = &worker.sender {
                    let _ = sender.send(job);
                }
            }
            Some(Backend::Inline(store)) => {
                let mut store = store.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                run_job(&mut store, job);
            }
            None => {}
        }
    }

    pub fn query<R: Send + 'static>(&self, query: impl FnOnce(&mut Store) -> R + Send + 'static) -> Option<R> {
        self.query_within(QUERY_WAIT, query)
    }

    fn query_within<R: Send + 'static>(
        &self,
        wait: Duration,
        query: impl FnOnce(&mut Store) -> R + Send + 'static,
    ) -> Option<R> {
        match self.backend.as_deref()? {
            Backend::Worker(worker) => {
                let (answer, answered) = std::sync::mpsc::channel();
                let job: Query = Box::new(move |store| {
                    let _ = answer.send(query(store));
                });
                worker.sender.as_ref()?.send(Job::Query(job)).ok()?;
                answered.recv_timeout(wait).ok()
            }
            Backend::Inline(store) => {
                let mut store = store.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                store.refresh();
                Some(query(&mut store))
            }
        }
    }

    pub fn flush(&self) {
        let _ = self.query(|_| ());
    }

    pub fn flush_within(&self, wait: Duration) -> bool {
        self.query_within(wait, |_| ()).is_some()
    }

    pub fn prune(&self) -> Option<Pruned> {
        self.query(|store| store.prune(now()).unwrap_or_default())
    }

    fn record(&self, record: Record) {
        self.submit(Job::Record(record));
    }
}

fn run_job(store: &mut Store, job: Job) {
    match job {
        Job::Query(query) => {
            store.refresh();
            query(store);
        }
        Job::Record(record) => record_now(store, record, None),
    }
}

fn written_by_ide(ahead: &mut VecDeque<Job>, root: &Path, relative: &str, bytes: &[u8]) -> bool {
    ahead.iter_mut().any(|job| match job {
        Job::Record(record) => {
            record.kind != VersionKind::External
                && record.root == root
                && record.relative == relative
                && record.content.bytes() == Some(bytes)
        }
        Job::Query(_) => false,
    })
}

fn record_now(store: &mut Store, record: Record, queue: Option<(&mut VecDeque<Job>, &std::sync::mpsc::Receiver<Job>)>) {
    let Record { root, relative, kind, label, content, time, dedupe } = record;
    let external = kind == VersionKind::External && matches!(content, Content::Read(_));
    let bytes = match content {
        Content::Bytes(bytes) => bytes,
        Content::Text(text) => text.to_string().into_bytes(),
        Content::Read(path) => match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                if metadata.len() > store.limits().max_file_bytes {
                    return;
                }
                match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(_) => return,
                }
            }
            Ok(_) => return,
            Err(_) => {
                deleted(store, &root, &relative, time);
                return;
            }
        },
    };
    if external && let Some((ahead, receiver)) = queue {
        ahead.extend(receiver.try_iter());
        if written_by_ide(ahead, &root, &relative, &bytes) {
            return;
        }
    }
    let _ = store.record(&root, &relative, kind, label, Some(&bytes), time, dedupe);
}

fn deleted(store: &mut Store, root: &Path, relative: &str, time: u64) {
    let mut paths: Vec<String> = store
        .entries(root)
        .iter()
        .filter(|entry| entry.is_under(relative))
        .filter_map(|entry| entry.path.clone())
        .collect();
    paths.sort();
    paths.dedup();
    for path in paths {
        let _ = store.record(root, &path, VersionKind::Deleted, None, None, time, true);
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

struct Place {
    root: PathBuf,
    relative: String,
    path: PathBuf,
    consoles: bool,
}

fn consoles_root(session: &Session) -> Option<PathBuf> {
    session.state::<crate::workspaces::Workspaces>()?.consoles_folder()
}

fn place(session: &Session, path: &Path) -> Option<Place> {
    let workbench = session.state::<Workbench>()?;
    let project = workbench.project().map(Path::to_owned);
    let path = workbench.spelled(&workbench.resolve(path));
    let consoles = consoles_root(session).filter(|folder| path.starts_with(folder));
    let in_project = project.as_ref().is_some_and(|project| path.starts_with(project));
    let (root, consoles) = match (project, consoles) {
        (Some(project), _) if in_project => (project, false),
        (_, Some(folder)) => (folder, true),
        _ => return None,
    };
    let rest = path.strip_prefix(&root).ok()?;
    let mut parts = Vec::new();
    for component in rest.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    Some(Place { relative: parts.join("/"), root, path, consoles })
}

fn recordable(session: &Session, path: &Path) -> Option<Place> {
    session.state::<LocalHistory>().filter(|history| history.is_enabled())?;
    let place = place(session, path)?;
    if place.relative.is_empty() {
        return None;
    }
    let parts: Vec<&str> = place.relative.split('/').collect();
    if parts.iter().any(|part| zen_vfs::HIDDEN.contains(part) || zen_vfs::is_ignored(part))
        || place.path.parent().is_some_and(|folder| zen_vfs::excluded_folders::excludes(&place.root, folder))
    {
        return None;
    }
    if crate::guard::refuses_in(Some(&place.root), &place.path) || !resolves_inside(&place.root, &place.path) {
        return None;
    }
    let settings = session.state::<crate::project_settings::ProjectSettingsState>().map(|state| state.settings());
    if let Some(settings) = settings.filter(|_| !place.consoles) {
        let exclusions = zen_index::Exclusions::new(&settings.excluded, &settings.exclude_patterns);
        if exclusions.excludes(&place.root, &place.path) {
            return None;
        }
    }
    Some(place)
}

fn resolves_inside(root: &Path, path: &Path) -> bool {
    let Ok(root) = std::fs::canonicalize(root) else { return false };
    let mut existing = path;
    let mut missing = Vec::new();
    while std::fs::symlink_metadata(existing).is_err() {
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else { return false };
        missing.push(name);
        existing = parent;
    }
    let Ok(mut resolved) = std::fs::canonicalize(existing) else { return false };
    resolved.extend(missing.iter().rev());
    resolved.starts_with(&root)
}

fn history(session: &Session) -> Option<LocalHistory> {
    session.state::<LocalHistory>().filter(|history| history.is_enabled()).cloned()
}

pub(crate) fn record_saves(session: &mut Session) {
    if session.state::<Workbench>().is_none() {
        return;
    }
    let saves: Vec<(PathBuf, Vec<SavedText>)> = session
        .state_mut::<Workbench>()
        .documents
        .iter_mut()
        .filter(|document| !document.saves.is_empty())
        .map(|document| (document.path.clone(), std::mem::take(&mut document.saves)))
        .collect();
    let Some(history) = history(session) else { return };
    for (path, saves) in saves {
        let Some(place) = recordable(session, &path) else { continue };
        for saved in saves {
            history.record(Record {
                root: place.root.clone(),
                relative: place.relative.clone(),
                kind: VersionKind::External,
                label: None,
                content: saved.content(saved.before.clone()),
                time: saved.time.saturating_sub(1),
                dedupe: true,
            });
            history.record(Record {
                root: place.root.clone(),
                relative: place.relative.clone(),
                kind: VersionKind::Save,
                label: None,
                content: saved.content(saved.after.clone()),
                time: saved.time,
                dedupe: true,
            });
        }
    }
}

pub(crate) fn rollback(
    session: &Session,
    repository: &zen_vcs::Repository,
    paths: &[String],
    delete_added: bool,
) -> Result<(), zen_vcs::VcsError> {
    around(session, ROLLBACK, repository.root(), paths, || repository.rollback(paths, delete_added))
}

pub(crate) fn apply_patch<T>(session: &Session, root: &Path, paths: &[String], change: impl FnOnce() -> T) -> T {
    let Ok(value) = around(session, APPLY_PATCH, root, paths, || Ok::<T, std::convert::Infallible>(change()));
    value
}

fn around<T, E>(
    session: &Session,
    action: &str,
    root: &Path,
    paths: &[String],
    change: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let before = read_before(session, paths.iter().map(|path| root.join(path)));
    let result = change();
    if result.is_ok()
        && let Some(history) = history(session)
    {
        let time = now();
        for (place, before) in before {
            if let Some(before) = before {
                history.record(Record {
                    root: place.root.clone(),
                    relative: place.relative.clone(),
                    kind: VersionKind::Before,
                    label: Some(action.to_owned()),
                    content: before,
                    time: time.saturating_sub(1),
                    dedupe: false,
                });
            }
            history.record(Record {
                root: place.root,
                relative: place.relative,
                kind: VersionKind::Action,
                label: Some(action.to_owned()),
                content: Content::Read(place.path),
                time,
                dedupe: false,
            });
        }
    }
    result
}

pub(crate) fn changed_by(session: &Session, action: &str, files: Written) {
    let Some(history) = history(session) else { return };
    let time = now();
    for (path, before, after) in files {
        let Some(place) = recordable(session, &path) else { continue };
        if let Some(before) = before {
            history.record(Record {
                root: place.root.clone(),
                relative: place.relative.clone(),
                kind: VersionKind::Before,
                label: Some(action.to_owned()),
                content: Content::Bytes(before),
                time: time.saturating_sub(1),
                dedupe: false,
            });
        }
        history.record(Record {
            root: place.root,
            relative: place.relative,
            kind: VersionKind::Action,
            label: Some(action.to_owned()),
            content: after.map_or(Content::Read(place.path), Content::Bytes),
            time,
            dedupe: false,
        });
    }
}

fn read_before(session: &Session, paths: impl Iterator<Item = PathBuf>) -> Vec<(Place, Option<Content>)> {
    let Some(history) = history(session) else { return Vec::new() };
    let limit = history.limits.max_file_bytes;
    paths
        .filter_map(|path| recordable(session, &path))
        .map(|place| {
            let unsaved = session
                .state::<Workbench>()
                .and_then(|workbench| workbench.documents.iter().find(|document| document.path() == place.path))
                .filter(|document| document.is_dirty() && document.image().is_none())
                .map(|document| Content::Text(document.snapshot().clone()));
            if unsaved.is_some() {
                return (place, unsaved);
            }
            let small =
                std::fs::metadata(&place.path).is_ok_and(|metadata| metadata.is_file() && metadata.len() <= limit);
            let disk = small.then(|| std::fs::read(&place.path).ok()).flatten().map(Content::Bytes);
            (place, disk)
        })
        .collect()
}

/// A version as the commands show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// The version's id, to pass to the other `localHistory.*` commands.
    pub id: u64,
    /// When it was recorded, in milliseconds since 1970.
    pub time: u64,
    /// The file; absent for a label of the whole project.
    pub path: Option<PathBuf>,
    /// The file relative to the project, with `/`.
    pub relative: Option<String>,
    /// What made it.
    pub kind: VersionKind,
    /// The action's id (`replaceInFiles`, `rename`, `rollback`, `revert`, `lineSeparators`, `editorConfigExport`) or the label's text.
    pub label: Option<String>,
    /// How the history list names it, in the interface language.
    pub title: String,
    /// The content's size in bytes.
    pub size: u64,
    /// Whether the content is not text.
    pub binary: bool,
    /// Whether the version holds a content (deletions and labels do not).
    pub has_content: bool,
}

fn action_name(action: Option<&str>) -> String {
    match action {
        Some(REPLACE_IN_FILES) => t!("local-history-action-replace-in-files"),
        Some(RENAME) => t!("local-history-action-rename"),
        Some(ROLLBACK) => t!("local-history-action-rollback"),
        Some(REVERT) => t!("local-history-action-revert"),
        Some(APPLY_PATCH) => t!("local-history-action-apply-patch"),
        Some(LINE_SEPARATORS) => t!("local-history-action-line-separators"),
        Some(BYTE_ORDER_MARK) => t!("local-history-action-byte-order-mark"),
        Some(FILE_ENCODING) => t!("local-history-action-file-encoding"),
        Some(EDITORCONFIG_EXPORT) => t!("local-history-action-editorconfig-export"),
        Some(PREVIEW_EDIT) => t!("local-history-action-preview-edit"),
        Some(FILE_EDIT) => t!("local-history-action-file-edit"),
        Some(EXTRACT) => t!("local-history-action-extract"),
        Some(SAFE_DELETE) => t!("local-history-action-safe-delete"),
        Some(MOVE) => t!("local-history-action-move"),
        Some(CHANGE_SIGNATURE) => t!("local-history-action-change-signature"),
        Some(PULL_MEMBERS_UP) => t!("local-history-action-pull-members-up"),
        Some(PUSH_MEMBERS_DOWN) => t!("local-history-action-push-members-down"),
        Some(EXTRACT_SUPERCLASS) => t!("local-history-action-extract-superclass"),
        Some(EXTRACT_INTERFACE) => t!("local-history-action-extract-interface"),
        Some(ENCAPSULATE_FIELDS) => t!("local-history-action-encapsulate-fields"),
        Some(INTRODUCE_PARAMETER_OBJECT) => t!("local-history-action-introduce-parameter-object"),
        other => other.unwrap_or_default().to_owned(),
    }
}

pub fn title(entry: &Entry) -> String {
    match entry.kind {
        VersionKind::Save => t!("local-history-save"),
        VersionKind::External => t!("local-history-external"),
        VersionKind::Before => t!("local-history-before", action = action_name(entry.label.as_deref()).as_str()),
        VersionKind::Action => action_name(entry.label.as_deref()),
        VersionKind::Deleted => t!("local-history-deleted"),
        VersionKind::Label => entry.label.clone().unwrap_or_default(),
    }
}

fn info(root: &Path, entry: &Entry) -> VersionInfo {
    VersionInfo {
        id: entry.id,
        time: entry.time,
        path: entry.path.as_ref().map(|path| root.join(path)),
        relative: entry.path.clone(),
        kind: entry.kind,
        label: entry.label.clone(),
        title: title(entry),
        size: entry.size,
        binary: entry.binary,
        has_content: entry.hash.is_some(),
    }
}

fn label_applies(on: Option<&str>, relative: &str, folder: bool) -> bool {
    let Some(on) = on else { return true };
    let inside = |inner: &str, outer: &str| {
        outer.is_empty() || inner == outer || inner.strip_prefix(outer).is_some_and(|rest| rest.starts_with('/'))
    };
    inside(relative, on) || (folder && inside(on, relative))
}

fn content_at<'a>(entries: &'a [Entry], relative: &str, id: u64) -> Option<&'a Entry> {
    entries
        .iter()
        .rev()
        .find(|entry| entry.id <= id && entry.kind != VersionKind::Label && entry.path.as_deref() == Some(relative))
        .filter(|entry| entry.hash.is_some())
}

fn entries_of(history: &LocalHistory, root: &Path, relative: &str) -> Result<Vec<Entry>, CommandError> {
    let (root, relative) = (root.to_owned(), relative.to_owned());
    history
        .query(move |store| {
            let entries = store.entries(&root).iter();
            entries.filter(|entry| entry.kind == VersionKind::Label || entry.is_under(&relative)).cloned().collect()
        })
        .ok_or_else(|| CommandError::new(t!("local-history-error-off")))
}

fn content_of(history: &LocalHistory, hash: &str) -> Result<Vec<u8>, CommandError> {
    let hash = hash.to_owned();
    history
        .query(move |store| store.content(&hash))
        .ok_or_else(|| CommandError::new(t!("local-history-error-off")))?
        .map_err(|error| CommandError::new(t!("local-history-error-read", error = error.to_string())))
}

struct Target {
    history: LocalHistory,
    place: Place,
    entries: Vec<Entry>,
    folder: bool,
}

fn target(session: &Session, path: &Path) -> Result<Target, CommandError> {
    let history = history(session).ok_or_else(|| CommandError::new(t!("local-history-error-off")))?;
    let place = place(session, path)
        .ok_or_else(|| CommandError::new(t!("local-history-error-outside", path = path.display().to_string())))?;
    let entries = entries_of(&history, &place.root, &place.relative)?;
    let exact = entries.iter().any(|entry| entry.path.as_deref() == Some(place.relative.as_str()));
    let folder = place.relative.is_empty()
        || place.path.is_dir()
        || (!exact && entries.iter().any(|entry| entry.kind != VersionKind::Label && entry.is_under(&place.relative)));
    Ok(Target { history, place, entries, folder })
}

impl Target {
    fn versions(&self) -> Vec<&Entry> {
        let relative = self.place.relative.as_str();
        let own = |entry: &&Entry| {
            entry.kind != VersionKind::Label
                && if self.folder { entry.is_under(relative) } else { entry.path.as_deref() == Some(relative) }
        };
        let first = self.entries.iter().find(own).map_or(u64::MAX, |entry| entry.time);
        self.entries
            .iter()
            .filter(|entry| {
                own(entry)
                    || (entry.kind == VersionKind::Label
                        && entry.time >= first
                        && label_applies(entry.path.as_deref(), relative, self.folder))
            })
            .collect()
    }

    fn version(&self, id: u64) -> Result<&Entry, CommandError> {
        self.versions().into_iter().find(|entry| entry.id == id).ok_or_else(|| {
            CommandError::new(t!("local-history-error-no-version", path = self.place.path.display().to_string()))
        })
    }

    fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .entries
            .iter()
            .filter(|entry| entry.kind != VersionKind::Label && entry.is_under(&self.place.relative))
            .filter_map(|entry| entry.path.clone())
            .collect();
        files.sort();
        files.dedup();
        files
    }

    fn changed_files(&self, id: u64) -> Vec<ChangedFile> {
        let mut changed = Vec::new();
        for relative in self.files() {
            let then = content_at(&self.entries, &relative, id).and_then(|entry| entry.hash.clone());
            let now = content_at(&self.entries, &relative, u64::MAX).and_then(|entry| entry.hash.clone());
            let status = match (&then, &now) {
                (Some(then), Some(now)) if then != now => FileStatus::Modified,
                (Some(_), None) => FileStatus::Deleted,
                (None, Some(_)) => FileStatus::Added,
                _ => continue,
            };
            changed.push(ChangedFile { path: self.place.root.join(&relative), relative, status });
        }
        changed
    }
}

fn current_bytes(session: &Session, path: &Path) -> Option<Vec<u8>> {
    let open = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents.iter().find(|document| document.path() == path))
        .filter(|document| document.image().is_none())
        .map(|document| document.text().as_bytes().to_vec());
    open.or_else(|| std::fs::read(path).ok())
}

/// A file or folder whose local history to read.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    /// The file or folder, an absolute path in the project or a console of the Database workspace; the project's root lists everything.
    pub path: PathBuf,
    /// At most this many versions, the newest first (1000 by default).
    pub limit: Option<usize>,
}

/// The versions of a file or folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionList {
    /// The file or folder.
    pub path: PathBuf,
    /// Whether it is a folder: the versions are of the files in it.
    pub folder: bool,
    /// The versions and labels, the newest first.
    pub versions: Vec<VersionInfo>,
    /// For an agent: how many of the project's files are hidden from agents, a number that does not
    /// depend on the folder asked for (ADR-0143); their versions are left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_files: Option<usize>,
}

pub struct ListVersions;

impl Command for ListVersions {
    type Params = ListParams;
    type Output = VersionList;
    const ID: &'static str = "localHistory.versions.list";
    const TITLE: &'static str = "Show History";
    const DESCRIPTION: &'static str = "Lists the versions Local History keeps of a file, or of the files in a folder, with \
         the labels put on them, the newest first.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<VersionList, CommandError> {
        let target = target(session, &params.path)?;
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
        let patterns = crate::hidden_files::for_caller(session);
        let root = target.place.root.clone();
        let hidden = |entry: &&Entry| {
            patterns.as_ref().is_some_and(|patterns| {
                entry
                    .path
                    .as_deref()
                    .is_some_and(|path| crate::hidden_files::is_hidden(patterns, Some(&root), Path::new(path)))
            })
        };
        let versions = target
            .versions()
            .into_iter()
            .filter(|entry| !hidden(entry))
            .rev()
            .take(limit)
            .map(|entry| info(&target.place.root, entry))
            .collect();
        let hidden_files = crate::hidden_files::count(session);
        Ok(VersionList { path: target.place.path.clone(), folder: target.folder, versions, hidden_files })
    }
}

/// A version of a file.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionParams {
    /// The file (or, where a folder is allowed, the folder), an absolute path in the project or a console of the Database workspace.
    pub path: PathBuf,
    /// The version's id from `localHistory.versions.list`; a label or deletion means the file then.
    pub id: u64,
    /// At most this many bytes of the text (1 MiB by default), for `localHistory.version.get`.
    pub max_bytes: Option<usize>,
}

/// A version's content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionContent {
    /// The version asked for.
    pub version: VersionInfo,
    /// The id of the version that holds the content (the one asked for, or the one before a label).
    pub content_id: u64,
    /// The text; absent for a binary file.
    pub content: Option<String>,
    /// Whether the content is not text.
    pub binary: bool,
    /// The content's size in bytes.
    pub size: u64,
    /// Whether the text was cut at `maxBytes`.
    pub truncated: bool,
}

pub struct GetVersion;

impl Command for GetVersion {
    type Params = VersionParams;
    type Output = VersionContent;
    const ID: &'static str = "localHistory.version.get";
    const TITLE: &'static str = "Local History Version";
    const DESCRIPTION: &'static str = "Returns the text of a file as a Local History version keeps it (at most `maxBytes`, \
         1 MiB by default); a binary file's content is not returned.";

    fn run(&self, session: &mut Session, params: VersionParams) -> Result<VersionContent, CommandError> {
        let target = target(session, &params.path)?;
        let version = target.version(params.id)?;
        let source = content_at(&target.entries, &target.place.relative, params.id)
            .filter(|_| !target.folder)
            .ok_or_else(|| {
                CommandError::new(t!("local-history-error-no-content", path = target.place.path.display().to_string()))
            })?;
        let bytes = content_of(&target.history, source.hash.as_deref().unwrap_or_default())?;
        let binary = store::is_binary(&bytes);
        let limit = params.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
        let (content, truncated) = if binary {
            (None, false)
        } else {
            let text = String::from_utf8_lossy(&bytes);
            let mut end = text.len().min(limit);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            (Some(text[..end].to_owned()), end < text.len())
        };
        Ok(VersionContent {
            version: info(&target.place.root, version),
            content_id: source.id,
            content,
            binary,
            size: bytes.len() as u64,
            truncated,
        })
    }
}

/// How a file of a folder differs between a version and now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    /// Its content changed since.
    Modified,
    /// It was deleted since.
    Deleted,
    /// It did not exist then.
    Added,
}

/// A file of a folder that changed since a version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    /// The file.
    pub path: PathBuf,
    /// The file relative to the project, with `/`.
    pub relative: String,
    /// How it changed since.
    pub status: FileStatus,
}

/// A version compared with now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionDiff {
    /// For a file: the version (before) against its current text (after).
    pub diff: Option<DiffView>,
    /// For a folder: the files that changed since the version.
    pub files: Vec<ChangedFile>,
}

pub struct DiffVersion;

impl Command for DiffVersion {
    type Params = VersionParams;
    type Output = VersionDiff;
    const ID: &'static str = "localHistory.version.diff";
    const TITLE: &'static str = "Compare with Current";
    const DESCRIPTION: &'static str = "Compares a Local History version of a file with its current text (the editor's, \
         unsaved changes included); for a folder, lists the files that changed since that version.";

    fn run(&self, session: &mut Session, params: VersionParams) -> Result<VersionDiff, CommandError> {
        let target = target(session, &params.path)?;
        let version = target.version(params.id)?;
        if target.folder {
            let mut files = target.changed_files(params.id);
            if let Some(patterns) = crate::hidden_files::for_caller(session) {
                files.retain(|file| !crate::hidden_files::is_hidden(&patterns, Some(&target.place.root), &file.path));
            }
            return Ok(VersionDiff { diff: None, files });
        }
        let before = match content_at(&target.entries, &target.place.relative, params.id) {
            Some(source) => content_of(&target.history, source.hash.as_deref().unwrap_or_default())?,
            None => Vec::new(),
        };
        let after = current_bytes(session, &target.place.path).unwrap_or_default();
        let binary = store::is_binary(&before) || store::is_binary(&after);
        let mut diff = crate::vcs::commit::diff_view(
            target.place.path.clone(),
            target.place.relative.clone(),
            (title(version), t!("local-history-current")),
            &before,
            &after,
        );
        if binary {
            diff = DiffView { binary: true, before: String::new(), after: String::new(), hunks: Vec::new(), ..diff };
        }
        Ok(VersionDiff { diff: Some(diff), files: Vec::new() })
    }
}

/// What a revert changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reverted {
    /// The files given their content back.
    pub files: Vec<PathBuf>,
    /// The files of a folder that did not exist then, left as they are.
    pub kept: Vec<PathBuf>,
}

pub struct RevertVersion;

impl Command for RevertVersion {
    type Params = VersionParams;
    type Output = Reverted;
    const ID: &'static str = "localHistory.version.revert";
    const TITLE: &'static str = "Revert";
    const DESCRIPTION: &'static str = "Gives a file the content a Local History version keeps; for a folder, gives every \
         file in it that changed since the version its content of then (files created since are kept). Open files \
         show it as an edit that Undo takes back.";

    fn run(&self, session: &mut Session, params: VersionParams) -> Result<Reverted, CommandError> {
        let target = target(session, &params.path)?;
        target.version(params.id)?;
        let mut restore = Vec::new();
        let mut reverted = Reverted::default();
        if target.folder {
            let changing: Vec<PathBuf> = target.changed_files(params.id).into_iter().map(|file| file.path).collect();
            crate::hidden_files::guard_change(session, &target.place.root, &changing)?;
            for file in target.changed_files(params.id) {
                match file.status {
                    FileStatus::Added => reverted.kept.push(file.path),
                    FileStatus::Modified | FileStatus::Deleted => restore.push(file.relative),
                }
            }
        } else {
            if content_at(&target.entries, &target.place.relative, params.id).is_none() {
                return Err(CommandError::new(t!(
                    "local-history-error-no-content",
                    path = target.place.path.display().to_string()
                )));
            }
            restore.push(target.place.relative.clone());
        }
        let mut written = Vec::new();
        for relative in restore {
            let path = target.place.root.join(&relative);
            if crate::guard::refuses_in(Some(&target.place.root), &path) {
                return Err(CommandError::new(crate::guard::refusal(&path)));
            }
            if !resolves_inside(&target.place.root, &path) {
                return Err(CommandError::new(t!("local-history-error-outside", path = path.display().to_string())));
            }
            let Some(source) = content_at(&target.entries, &relative, params.id) else { continue };
            let bytes = content_of(&target.history, source.hash.as_deref().unwrap_or_default())?;
            let before = std::fs::read(&path).ok();
            if before.as_deref() == Some(bytes.as_slice()) {
                continue;
            }
            let failed = |error: String| {
                CommandError::new(t!("local-history-error-write", path = path.display().to_string(), error = error))
            };
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| failed(error.to_string()))?;
            }
            zen_vfs::replace_unchanged(&path, before.as_deref(), &bytes)
                .map_err(|error| failed(error.source.to_string()))?;
            written.push((path.clone(), before, Some(bytes)));
            reverted.files.push(path);
        }
        let workbench = session.state_mut::<Workbench>();
        for (path, _, bytes) in &written {
            if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path)
                && document.image().is_none()
                && let Some(text) = bytes.as_deref().and_then(|bytes| document.decoded(bytes))
            {
                document.load(text);
            }
        }
        if !written.is_empty() {
            workbench.revision += 1;
        }
        changed_by(session, REVERT, written);
        Ok(reverted)
    }
}

/// A label to put.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabelParams {
    /// The file or folder it is put on, an absolute path in the project; the whole project when absent.
    pub path: Option<PathBuf>,
    /// The label's text.
    pub label: String,
}

/// The label put.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabelPut {
    /// The label's id in the history lists.
    pub id: u64,
}

pub struct PutLabel;

impl Command for PutLabel {
    type Params = LabelParams;
    type Output = LabelPut;
    const ID: &'static str = "localHistory.label.put";
    const TITLE: &'static str = "Put Label…";
    const DESCRIPTION: &'static str = "Puts a label in Local History at this moment, on the whole project or on one file or \
         folder; it shows in the history of every file it applies to.";

    fn run(&self, session: &mut Session, params: LabelParams) -> Result<LabelPut, CommandError> {
        let label = params.label.trim().to_owned();
        if label.is_empty() {
            return Err(CommandError::new(t!("local-history-error-label")));
        }
        let history = history(session).ok_or_else(|| CommandError::new(t!("local-history-error-off")))?;
        let root = session
            .state::<Workbench>()
            .and_then(Workbench::project)
            .map(Path::to_owned)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let relative = match &params.path {
            Some(path) => Some(
                place(session, path)
                    .ok_or_else(|| {
                        CommandError::new(t!("local-history-error-outside", path = path.display().to_string()))
                    })?
                    .relative,
            ),
            None => None,
        };
        let time = now();
        let id = history
            .query(move |store| store.label(&root, relative.as_deref(), &label, time))
            .ok_or_else(|| CommandError::new(t!("local-history-error-off")))?
            .map_err(|error| CommandError::new(t!("local-history-error-read", error = error.to_string())))?;
        Ok(LabelPut { id })
    }
}

/// Files that changed on disk.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordParams {
    /// The changed files and folders, absolute paths in the project.
    pub paths: Vec<PathBuf>,
}

/// How many changes were queued.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Recorded {
    /// The paths whose new content (or deletion) will be recorded.
    pub queued: usize,
}

pub struct RecordChanges;

impl Command for RecordChanges {
    type Params = RecordParams;
    type Output = Recorded;
    const ID: &'static str = "localHistory.changes.record";
    const TITLE: &'static str = "Record External Changes";
    const DESCRIPTION: &'static str = "Records in Local History the content of files that changed on disk outside the IDE \
         (the window calls it for what the file watcher reports): a new version labelled External change, or a \
         deletion; an open file's previous text is kept first. Files Git ignores (build output) are left out, as the \
         index leaves them out. Call it before file.document.syncWithDisk.";

    fn run(&self, session: &mut Session, params: RecordParams) -> Result<Recorded, CommandError> {
        record_saves(session);
        let Some(history) = history(session) else { return Ok(Recorded { queued: 0 }) };
        let time = now();
        let mut queued = 0;
        let index = session.state::<Workbench>().and_then(|workbench| workbench.index.clone());
        for path in &params.paths {
            if index.as_ref().is_some_and(|index| index.git_ignores(path)) {
                continue;
            }
            let Some(place) = recordable(session, path) else { continue };
            let previous = session
                .state::<Workbench>()
                .and_then(|workbench| workbench.documents.iter().find(|document| document.path() == place.path))
                .filter(|document| document.image().is_none() && document.read_only().is_none())
                .map(|document| document.saved.clone());
            if let Some(previous) = previous {
                history.record(Record {
                    root: place.root.clone(),
                    relative: place.relative.clone(),
                    kind: VersionKind::External,
                    label: None,
                    content: Content::Text(previous),
                    time: time.saturating_sub(1),
                    dedupe: true,
                });
            }
            history.record(Record {
                root: place.root,
                relative: place.relative,
                kind: VersionKind::External,
                label: None,
                content: Content::Read(place.path),
                time,
                dedupe: true,
            });
            queued += 1;
        }
        Ok(Recorded { queued })
    }
}

#[cfg(test)]
mod tests;
