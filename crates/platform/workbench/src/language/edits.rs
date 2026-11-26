use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::Value;
use zen_i18n::t;
use zen_lsp::lsp_types::FileChangeType;

use super::places::{Place, TextCache, place};
use super::{Languages, byte_offset};
use crate::{Document, Workbench};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    Unsupported,
    Overlapping,
    FilesChanged,
    Read { path: PathBuf, error: String },
    Write { path: PathBuf, error: String },
    ForeignSettings { path: PathBuf },
    OutsideProject { path: PathBuf },
    ReadOnly { reason: String },
    Hidden { path: PathBuf },
    Exists { path: PathBuf },
    Missing { path: PathBuf },
    NotEmpty { path: PathBuf },
    TooLarge { path: PathBuf },
}

impl EditError {
    pub fn message(&self) -> String {
        match self {
            Self::Unsupported => t!("workbench-rename-unsupported-change"),
            Self::Overlapping => t!("workbench-rename-overlapping"),
            Self::FilesChanged => t!("workbench-rename-files-changed"),
            Self::Read { path, error } => {
                t!("workbench-error-open", path = path.display().to_string(), error = error.as_str())
            }
            Self::Write { path, error } => {
                t!("workbench-error-save", file = path.display().to_string(), error = error.as_str())
            }
            Self::ForeignSettings { path } => crate::guard::refusal(path),
            Self::OutsideProject { path } => {
                t!("workbench-edit-outside-project", path = path.display().to_string())
            }
            Self::ReadOnly { reason } => reason.clone(),
            Self::Hidden { path } => t!("workbench-hidden-change", file = path.display().to_string()),
            Self::Exists { path } => t!("workbench-edit-file-exists", path = path.display().to_string()),
            Self::Missing { path } => t!("workbench-edit-file-missing", path = path.display().to_string()),
            Self::NotEmpty { path } => t!("workbench-edit-folder-not-empty", path = path.display().to_string()),
            Self::TooLarge { path } => t!("workbench-edit-delete-too-large", path = path.display().to_string()),
        }
    }
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for EditError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChange {
    pub place: Place,
    pub new_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEdit {
    pub path: PathBuf,
    pub uri: String,
    pub before: String,
    pub server_version: Option<i32>,
    pub changes: Vec<TextChange>,
}

impl FileEdit {
    pub fn replacements(&self) -> Vec<(Range<usize>, String)> {
        self.changes.iter().map(|change| (change.place.start..change.place.end, change.new_text.clone())).collect()
    }

    pub fn after(&self) -> String {
        replaced(&self.before, &self.replacements())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOperation {
    Create { path: PathBuf, overwrite: bool },
    Rename { from: PathBuf, to: PathBuf, overwrite: bool },
    Delete { path: PathBuf, recursive: bool },
}

impl FileOperation {
    pub fn path(&self) -> &Path {
        match self {
            Self::Create { path, .. } | Self::Delete { path, .. } => path,
            Self::Rename { from, .. } => from,
        }
    }

    fn paths(&self) -> Vec<&Path> {
        match self {
            Self::Create { path, .. } | Self::Delete { path, .. } => vec![path],
            Self::Rename { from, to, .. } => vec![from, to],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Text(FileEdit),
    File(FileOperation),
}

pub fn text_files(steps: &[Step]) -> impl Iterator<Item = &FileEdit> {
    steps.iter().filter_map(|step| match step {
        Step::Text(file) => Some(file),
        Step::File(_) => None,
    })
}

pub fn file_operations(steps: &[Step]) -> impl Iterator<Item = &FileOperation> {
    steps.iter().filter_map(|step| match step {
        Step::File(operation) => Some(operation),
        Step::Text(_) => None,
    })
}

pub fn changed_paths(steps: &[Step]) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = steps
        .iter()
        .map(|step| match step {
            Step::Text(file) => file.path.clone(),
            Step::File(FileOperation::Rename { to, .. }) => to.clone(),
            Step::File(operation) => operation.path().to_owned(),
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

pub type Saved = Vec<(PathBuf, Option<Vec<u8>>)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    Document { path: PathBuf, version: u64 },
    Disk { path: PathBuf, before: String, after: String },
    Created { path: PathBuf, replaced: Option<Vec<u8>>, folders: Vec<PathBuf> },
    Renamed { from: PathBuf, to: PathBuf, replaced: Option<Vec<u8>>, folders: Vec<PathBuf>, version: Option<u64> },
    Deleted { path: PathBuf, saved: Saved },
}

impl Applied {
    pub fn path(&self) -> &Path {
        match self {
            Self::Document { path, .. } | Self::Disk { path, .. } | Self::Created { path, .. } => path,
            Self::Deleted { path, .. } => path,
            Self::Renamed { to, .. } => to,
        }
    }

    fn is_operation(&self) -> bool {
        matches!(self, Self::Created { .. } | Self::Renamed { .. } | Self::Deleted { .. })
    }

    pub(crate) fn written(&self) -> crate::local_history::Written {
        match self {
            Self::Disk { path, before, after } => {
                vec![(path.clone(), Some(before.clone().into_bytes()), Some(after.clone().into_bytes()))]
            }
            Self::Deleted { path, saved } => saved
                .iter()
                .filter_map(|(relative, bytes)| {
                    let file = if relative.as_os_str().is_empty() { path.clone() } else { path.join(relative) };
                    bytes.as_ref().map(|bytes| (file, Some(bytes.clone()), None))
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

const MAX_SAVED_BYTES: u64 = 64 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Origin {
    Real(PathBuf),
    Created(usize),
}

#[derive(Default)]
struct Planned {
    operations: Vec<FileOperation>,
}

impl Planned {
    fn origin(&self, path: &Path) -> Option<Origin> {
        let mut path = path.to_owned();
        for (index, operation) in self.operations.iter().enumerate().rev() {
            match operation {
                FileOperation::Create { path: made, .. } => {
                    if path == *made {
                        return Some(Origin::Created(index));
                    }
                    if path.starts_with(made) {
                        return None;
                    }
                }
                FileOperation::Rename { from, to, .. } => {
                    if let Ok(rest) = path.strip_prefix(to) {
                        path = joined(from, rest);
                    } else if path.starts_with(from) {
                        return None;
                    }
                }
                FileOperation::Delete { path: gone, .. } => {
                    if path.starts_with(gone) {
                        return None;
                    }
                }
            }
        }
        Some(Origin::Real(path))
    }

    fn exists(&self, path: &Path) -> bool {
        match self.origin(path) {
            Some(Origin::Real(real)) => std::fs::symlink_metadata(real).is_ok(),
            Some(Origin::Created(_)) => true,
            None => false,
        }
    }

    fn accept(&mut self, change: &Value) -> Result<Option<FileOperation>, EditError> {
        let options = &change["options"];
        let flag = |name: &str| options[name].as_bool() == Some(true);
        let path_of =
            |key: &str| change[key].as_str().and_then(zen_lsp::position::uri_path).ok_or(EditError::Unsupported);
        let operation = match change["kind"].as_str() {
            Some("create") => {
                let path = path_of("uri")?;
                if self.exists(&path) && !flag("overwrite") {
                    if flag("ignoreIfExists") {
                        return Ok(None);
                    }
                    return Err(EditError::Exists { path });
                }
                if path.is_dir() {
                    return Err(EditError::Exists { path });
                }
                FileOperation::Create { overwrite: self.exists(&path), path }
            }
            Some("rename") => {
                let (from, to) = (path_of("oldUri")?, path_of("newUri")?);
                if !self.exists(&from) {
                    return Err(EditError::Missing { path: from });
                }
                if from == to {
                    return Ok(None);
                }
                let same_entry = same_file(&from, &to);
                let taken = self.exists(&to) && !same_entry;
                if taken && !flag("overwrite") {
                    if flag("ignoreIfExists") {
                        return Ok(None);
                    }
                    return Err(EditError::Exists { path: to });
                }
                if taken && to.is_dir() {
                    return Err(EditError::Exists { path: to });
                }
                FileOperation::Rename { from, to, overwrite: taken }
            }
            Some("delete") => {
                let path = path_of("uri")?;
                if !self.exists(&path) {
                    if flag("ignoreIfNotExists") {
                        return Ok(None);
                    }
                    return Err(EditError::Missing { path });
                }
                let recursive = flag("recursive");
                if let Some(Origin::Real(real)) = self.origin(&path)
                    && !recursive
                    && std::fs::read_dir(&real).is_ok_and(|mut entries| entries.next().is_some())
                {
                    return Err(EditError::NotEmpty { path });
                }
                FileOperation::Delete { path, recursive }
            }
            _ => return Err(EditError::Unsupported),
        };
        self.operations.push(operation.clone());
        Ok(Some(operation))
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn joined(base: &Path, rest: &Path) -> PathBuf {
    if rest.as_os_str().is_empty() { base.to_owned() } else { base.join(rest) }
}

fn followed(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    path.strip_prefix(from).ok().map(|rest| joined(to, rest))
}

pub fn parse(answer: &Value, languages: &mut Languages) -> Result<Vec<Step>, EditError> {
    let operations = answer
        .get("documentChanges")
        .and_then(Value::as_array)
        .is_some_and(|changes| changes.iter().any(|change| change.get("kind").is_some()));
    if operations {
        return parse_in_order(&answer["documentChanges"], languages);
    }
    let mut entries: Vec<(&str, Option<i64>, &Value)> = Vec::new();
    if let Some(document_changes) = answer.get("documentChanges").filter(|changes| !changes.is_null()) {
        let list = document_changes.as_array().ok_or(EditError::Unsupported)?;
        for change in list {
            let uri = change["textDocument"]["uri"].as_str().ok_or(EditError::Unsupported)?;
            entries.push((uri, change["textDocument"]["version"].as_i64(), &change["edits"]));
        }
    } else if let Some(changes) = answer.get("changes").and_then(Value::as_object) {
        entries.extend(changes.iter().map(|(uri, edits)| (uri.as_str(), None, edits)));
    }
    let mut cache = TextCache::new();
    let mut files: Vec<FileEdit> = Vec::new();
    for (uri, version, edits) in entries {
        let path = zen_lsp::position::uri_path(uri).ok_or(EditError::Unsupported)?;
        let server_version = checked_version(languages, &path, version)?;
        let Some(text) = cache.text(languages, &path).map(str::to_owned) else {
            let error = zen_vfs::read_text(&path).err().map(|error| error.to_string()).unwrap_or_default();
            return Err(EditError::Read { path, error });
        };
        let mut changes = text_changes(&path, &text, edits)?;
        match files.iter_mut().find(|file| file.path == path) {
            Some(file) => file.changes.append(&mut changes),
            None => files.push(FileEdit { path, uri: uri.to_owned(), before: text, server_version, changes }),
        }
    }
    for file in &mut files {
        sort_checked(&mut file.changes)?;
    }
    files.retain(|file| file.after() != file.before);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files.into_iter().map(Step::Text).collect())
}

fn checked_version(languages: &Languages, path: &Path, version: Option<i64>) -> Result<Option<i32>, EditError> {
    let server_version = languages.server_version(path);
    if let (Some(version), Some(server_version)) = (version, server_version)
        && version != i64::from(server_version)
    {
        return Err(EditError::FilesChanged);
    }
    Ok(server_version)
}

fn parse_in_order(changes: &Value, languages: &mut Languages) -> Result<Vec<Step>, EditError> {
    let list = changes.as_array().ok_or(EditError::Unsupported)?;
    let mut planned = Planned::default();
    let mut cache = TextCache::new();
    let mut texts: HashMap<Origin, String> = HashMap::new();
    let mut steps = Vec::new();
    for change in list {
        if change.get("kind").is_some() {
            if let Some(operation) = planned.accept(change)? {
                steps.push(Step::File(operation));
            }
            continue;
        }
        let uri = change["textDocument"]["uri"].as_str().ok_or(EditError::Unsupported)?;
        let path = zen_lsp::position::uri_path(uri).ok_or(EditError::Unsupported)?;
        let origin = planned.origin(&path).ok_or_else(|| EditError::Missing { path: path.clone() })?;
        let server_version = match &origin {
            Origin::Real(real) if *real == path && planned.operations.is_empty() => {
                checked_version(languages, &path, change["textDocument"]["version"].as_i64())?
            }
            _ => None,
        };
        let before = match texts.get(&origin) {
            Some(text) => text.clone(),
            None => match &origin {
                Origin::Created(_) => String::new(),
                Origin::Real(real) => match cache.text(languages, real) {
                    Some(text) => text.to_owned(),
                    None => {
                        let error = zen_vfs::read_text(real).err().map(|error| error.to_string()).unwrap_or_default();
                        return Err(EditError::Read { path: real.clone(), error });
                    }
                },
            },
        };
        let mut changes = text_changes(&path, &before, &change["edits"])?;
        sort_checked(&mut changes)?;
        let file = FileEdit { path, uri: uri.to_owned(), before, server_version, changes };
        let after = file.after();
        if after != file.before {
            texts.insert(origin, after);
            steps.push(Step::Text(file));
        }
    }
    Ok(steps)
}

pub fn text_edits(text: &str, edits: &Value) -> Result<Vec<(Range<usize>, String)>, EditError> {
    let mut changes = text_changes(Path::new(""), text, edits)?;
    sort_checked(&mut changes)?;
    Ok(changes.into_iter().map(|change| (change.place.start..change.place.end, change.new_text)).collect())
}

pub fn apply_text_edits(document: &mut Document, edits: &Value) -> Result<bool, EditError> {
    let replacements = text_edits(document.text(), edits)?;
    Ok(document.replace_ranges(replacements))
}

pub fn apply(steps: &[Step], languages: &mut Languages, workbench: &mut Workbench) -> Result<Vec<Applied>, EditError> {
    check(steps, languages, workbench)?;
    let mut log = Vec::new();
    for step in steps {
        let result = match step {
            Step::Text(file) => apply_text(file, workbench),
            Step::File(operation) => apply_operation(operation, workbench),
        };
        match result {
            Ok(applied) => log.push(applied),
            Err(error) => {
                revert(&log, languages, workbench, false);
                return Err(error);
            }
        }
    }
    report(languages, &log, false);
    if log.iter().any(|applied| !matches!(applied, Applied::Document { .. })) {
        workbench.revision += 1;
    }
    languages.sync(workbench);
    Ok(log)
}

fn check(steps: &[Step], languages: &Languages, workbench: &Workbench) -> Result<(), EditError> {
    let root = workbench.project();
    let mut touched: Vec<&Path> = text_files(steps).map(|file| file.path.as_path()).collect();
    touched.extend(file_operations(steps).flat_map(FileOperation::paths));
    if let Some(path) = touched.iter().find(|path| crate::guard::refuses_in(root, path)) {
        return Err(EditError::ForeignSettings { path: path.to_path_buf() });
    }
    if let Some(patterns) = &workbench.agent_hidden {
        if let Some(path) = touched.iter().find(|path| crate::hidden_files::is_hidden(patterns, root, path)) {
            return Err(EditError::Hidden { path: path.to_path_buf() });
        }
        let folders = file_operations(steps).flat_map(FileOperation::paths);
        if let Some(inside) = folders.filter_map(|path| crate::hidden_files::hidden_inside(patterns, root, path)).next()
        {
            return Err(EditError::Hidden { path: PathBuf::from(inside) });
        }
    }
    for operation in file_operations(steps) {
        for path in operation.paths() {
            if !inside(root, path) {
                return Err(EditError::OutsideProject { path: path.to_owned() });
            }
            if let Some(reason) = crate::read_only::lock_reason(path) {
                return Err(EditError::ReadOnly { reason });
            }
        }
    }
    let unmoved = steps.iter().take_while(|step| matches!(step, Step::Text(_)));
    for step in unmoved {
        let Step::Text(file) = step else { continue };
        check_text(file, workbench)?;
        if let Some(version) = file.server_version
            && workbench.documents.iter().any(|document| document.path() == file.path)
            && languages.server_version(&file.path) != Some(version)
        {
            return Err(EditError::FilesChanged);
        }
    }
    Ok(())
}

fn inside(root: Option<&Path>, path: &Path) -> bool {
    let Some(root) = root.and_then(|root| std::fs::canonicalize(root).ok()) else { return false };
    let mut existing = path;
    let mut rest = Vec::new();
    while std::fs::symlink_metadata(existing).is_err() {
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else { return false };
        rest.push(name);
        existing = parent;
    }
    let Ok(real) = std::fs::canonicalize(existing) else { return false };
    let real = rest.iter().rev().fold(real, |path, name| path.join(name));
    real.starts_with(&root)
        && real != root
        && real
            .strip_prefix(&root)
            .is_ok_and(|rest| rest.components().all(|part| matches!(part, std::path::Component::Normal(_))))
}

fn check_text(file: &FileEdit, workbench: &Workbench) -> Result<(), EditError> {
    match workbench.documents.iter().find(|document| document.path() == file.path) {
        Some(document) => {
            if let Some(reason) = document.read_only() {
                return Err(EditError::ReadOnly { reason: reason.to_owned() });
            }
            if document.image().is_some() || document.text() != file.before {
                return Err(EditError::FilesChanged);
            }
        }
        None => {
            if let Some(reason) = crate::read_only::lock_reason(&file.path) {
                return Err(EditError::ReadOnly { reason });
            }
            if zen_vfs::read_text(&file.path).ok().as_deref() != Some(file.before.as_str()) {
                return Err(EditError::FilesChanged);
            }
        }
    }
    Ok(())
}

fn apply_text(file: &FileEdit, workbench: &mut Workbench) -> Result<Applied, EditError> {
    check_text(file, workbench)?;
    if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == file.path) {
        if !document.replace_ranges(file.replacements()) {
            return Err(EditError::FilesChanged);
        }
        return Ok(Applied::Document { path: file.path.clone(), version: document.version() });
    }
    let after = file.after();
    zen_vfs::replace_unchanged(&file.path, Some(file.before.as_bytes()), after.as_bytes()).map_err(|error| {
        if error.is_changed_on_disk() {
            EditError::FilesChanged
        } else {
            EditError::Write { path: file.path.clone(), error: error.to_string() }
        }
    })?;
    Ok(Applied::Disk { path: file.path.clone(), before: file.before.clone(), after })
}

fn write_error(path: &Path) -> impl Fn(std::io::Error) -> EditError + '_ {
    move |error| EditError::Write { path: path.to_owned(), error: error.to_string() }
}

fn make_parents(path: &Path) -> Result<Vec<PathBuf>, EditError> {
    let mut missing = Vec::new();
    let mut parent = path.parent();
    while let Some(folder) = parent.filter(|folder| std::fs::symlink_metadata(folder).is_err()) {
        missing.push(folder.to_owned());
        parent = folder.parent();
    }
    if let Some(top) = missing.first() {
        std::fs::create_dir_all(top).map_err(write_error(top))?;
    }
    Ok(missing)
}

fn remove_folders(folders: &[PathBuf]) {
    for folder in folders {
        let _ = std::fs::remove_dir(folder);
    }
}

fn apply_operation(operation: &FileOperation, workbench: &mut Workbench) -> Result<Applied, EditError> {
    match operation {
        FileOperation::Create { path, overwrite } => {
            let replaced =
                if *overwrite && path.is_file() { Some(std::fs::read(path).map_err(write_error(path))?) } else { None };
            let folders = make_parents(path)?;
            let made = match replaced {
                Some(_) => zen_vfs::write_atomic(path, b"").map_err(|error| error.to_string()),
                None => zen_vfs::create_file(path).map_err(|error| error.source.to_string()),
            };
            if let Err(error) = made {
                remove_folders(&folders);
                return Err(EditError::Write { path: path.clone(), error });
            }
            if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path) {
                document.sync_with_disk();
            }
            Ok(Applied::Created { path: path.clone(), replaced, folders })
        }
        FileOperation::Rename { from, to, overwrite } => {
            let replaced = if *overwrite && to.is_file() && !same_file(from, to) {
                Some(std::fs::read(to).map_err(write_error(to))?)
            } else {
                None
            };
            let folders = make_parents(to)?;
            if let Err(error) = std::fs::rename(from, to) {
                remove_folders(&folders);
                return Err(EditError::Write { path: from.clone(), error: error.to_string() });
            }
            if replaced.is_some() {
                close_under(workbench, to);
            }
            workbench.relocated(from, to);
            let version = workbench.documents.iter().find(|document| document.path() == to).map(Document::version);
            Ok(Applied::Renamed { from: from.clone(), to: to.clone(), replaced, folders, version })
        }
        FileOperation::Delete { path, .. } => {
            let saved = saved(path)?;
            let removed = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
            removed.map_err(write_error(path))?;
            close_under(workbench, path);
            workbench.revision += 1;
            Ok(Applied::Deleted { path: path.clone(), saved })
        }
    }
}

fn close_under(workbench: &mut Workbench, gone: &Path) {
    workbench.documents.retain(|document| !document.path().starts_with(gone));
    workbench.groups.retain(|path| !path.starts_with(gone));
    workbench.sync_groups();
}

fn saved(path: &Path) -> Result<Saved, EditError> {
    let mut saved = Vec::new();
    let mut total = 0;
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let full = joined(path, &relative);
        let metadata = std::fs::symlink_metadata(&full).map_err(write_error(&full))?;
        if metadata.is_dir() {
            saved.push((relative.clone(), None));
            let mut names: Vec<PathBuf> = std::fs::read_dir(&full)
                .map_err(write_error(&full))?
                .filter_map(Result::ok)
                .map(|entry| relative.join(entry.file_name()))
                .collect();
            names.sort();
            stack.extend(names.into_iter().rev());
        } else {
            total += metadata.len();
            if total > MAX_SAVED_BYTES {
                return Err(EditError::TooLarge { path: path.to_owned() });
            }
            saved.push((relative, Some(std::fs::read(&full).map_err(write_error(&full))?)));
        }
    }
    Ok(saved)
}

pub(crate) fn revert(
    log: &[Applied],
    languages: &mut Languages,
    workbench: &mut Workbench,
    strict: bool,
) -> (bool, Vec<PathBuf>) {
    let mut changed = false;
    let mut skipped = Vec::new();
    let mut reverted = Vec::new();
    for applied in log.iter().rev() {
        let done = match applied {
            Applied::Document { path, version } => {
                match workbench
                    .documents
                    .iter_mut()
                    .find(|document| document.path() == path && (!strict || document.version() == *version))
                {
                    Some(document) => {
                        changed |= document.undo();
                        true
                    }
                    None => false,
                }
            }
            Applied::Disk { path, before, after } => {
                let done = zen_vfs::replace_unchanged(path, Some(after.as_bytes()), before.as_bytes()).is_ok();
                if done && let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path)
                {
                    document.sync_with_disk();
                }
                done
            }
            Applied::Created { path, replaced, folders } => {
                let done = match replaced {
                    Some(bytes) => zen_vfs::write_atomic(path, bytes).is_ok(),
                    None => {
                        std::fs::read(path).is_ok_and(|bytes| bytes.is_empty()) && std::fs::remove_file(path).is_ok()
                    }
                };
                if done {
                    remove_folders(folders);
                    if replaced.is_none() {
                        workbench.documents.retain(|document| document.path() != path || document.is_dirty());
                        workbench.sync_groups();
                    } else if let Some(document) =
                        workbench.documents.iter_mut().find(|document| document.path() == path)
                    {
                        document.sync_with_disk();
                    }
                }
                done
            }
            Applied::Renamed { from, to, replaced, folders, .. } => {
                let free = std::fs::symlink_metadata(from).is_err() || same_file(from, to);
                let done = free && std::fs::symlink_metadata(to).is_ok() && std::fs::rename(to, from).is_ok();
                if done {
                    workbench.relocated(to, from);
                    if let Some(bytes) = replaced {
                        let _ = zen_vfs::write_atomic(to, bytes);
                    }
                    remove_folders(folders);
                }
                done
            }
            Applied::Deleted { path, saved } => {
                let free = std::fs::symlink_metadata(path).is_err();
                free && saved.iter().all(|(relative, bytes)| {
                    let full = joined(path, relative);
                    match bytes {
                        None => std::fs::create_dir_all(&full).is_ok(),
                        Some(bytes) => {
                            full.parent().is_none_or(|parent| std::fs::create_dir_all(parent).is_ok())
                                && std::fs::write(&full, bytes).is_ok()
                        }
                    }
                })
            }
        };
        if done {
            if !matches!(applied, Applied::Document { .. }) {
                reverted.push(applied.clone());
            }
        } else {
            skipped.push(applied.path().to_owned());
        }
    }
    report(languages, &reverted, true);
    if !reverted.is_empty() {
        workbench.revision += 1;
        changed = true;
    }
    (changed, skipped)
}

fn report(languages: &mut Languages, log: &[Applied], undone: bool) {
    let mut changes: Vec<(PathBuf, FileChangeType)> = Vec::new();
    let (made, gone) = if undone {
        (FileChangeType::DELETED, FileChangeType::CREATED)
    } else {
        (FileChangeType::CREATED, FileChangeType::DELETED)
    };
    for applied in log {
        match applied {
            Applied::Document { .. } => {}
            Applied::Disk { path, .. } => changes.push((path.clone(), FileChangeType::CHANGED)),
            Applied::Created { path, replaced, .. } => {
                changes.push((path.clone(), if replaced.is_some() { FileChangeType::CHANGED } else { made }));
            }
            Applied::Renamed { from, to, .. } => {
                changes.push((from.clone(), gone));
                changes.push((to.clone(), made));
            }
            Applied::Deleted { path, .. } => changes.push((path.clone(), gone)),
        }
    }
    languages.report_files(&changes);
}

pub(crate) fn forward(log: &[Applied], index: usize, path: &Path) -> PathBuf {
    log.iter().skip(index + 1).fold(path.to_owned(), |path, applied| match applied {
        Applied::Renamed { from, to, .. } => followed(&path, from, to).unwrap_or(path),
        _ => path,
    })
}

pub(crate) fn journal(log: Vec<Applied>, workbench: &mut Workbench) -> Vec<Applied> {
    let operations = log.iter().any(Applied::is_operation);
    let mut kept: Vec<Applied> = Vec::new();
    let mut documents: Vec<(usize, PathBuf, usize)> = Vec::new();
    for change in log {
        match change {
            Applied::Document { path, version } => {
                match documents.iter_mut().find(|(_, current, _)| *current == path) {
                    Some((index, _, count)) => {
                        if let Some(Applied::Document { version: last, .. }) = kept.get_mut(*index) {
                            *last = version;
                        }
                        *count += 1;
                    }
                    None => {
                        documents.push((kept.len(), path.clone(), 1));
                        kept.push(Applied::Document { path, version });
                    }
                }
            }
            Applied::Disk { path, before, after } if !operations => {
                match kept.iter_mut().find(|kept| matches!(kept, Applied::Disk { path: same, .. } if *same == path)) {
                    Some(Applied::Disk { after: last, .. }) => *last = after,
                    _ => kept.push(Applied::Disk { path, before, after }),
                }
            }
            Applied::Renamed { from, to, replaced, folders, version } => {
                for (_, current, _) in &mut documents {
                    if let Some(moved) = followed(current, &from, &to) {
                        *current = moved;
                    }
                }
                kept.push(Applied::Renamed { from, to, replaced, folders, version });
            }
            other => kept.push(other),
        }
    }
    for (_, current, count) in documents.iter().filter(|(_, _, count)| *count > 1) {
        if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == current) {
            document.merge_undo_steps(*count);
        }
    }
    let finals: Vec<Option<u64>> = (0..kept.len())
        .map(|index| match &kept[index] {
            Applied::Renamed { to, version: Some(_), .. } => {
                let at = forward(&kept, index, to);
                workbench.documents.iter().find(|document| document.path() == at).map(Document::version)
            }
            _ => None,
        })
        .collect();
    for (applied, last) in kept.iter_mut().zip(finals) {
        if let (Applied::Renamed { version, .. }, Some(last)) = (applied, last) {
            *version = Some(last);
        }
    }
    kept
}

pub(crate) fn matches_document(log: &[Applied], path: &Path, version: u64) -> bool {
    log.iter().enumerate().any(|(index, applied)| match applied {
        Applied::Document { path: changed, version: after } => {
            *after == version && forward(log, index, changed) == path
        }
        Applied::Renamed { to, version: Some(after), .. } => *after == version && forward(log, index, to) == path,
        _ => false,
    })
}

pub fn confine(steps: &[Step], workbench: &Workbench, allowed: &[PathBuf]) -> Result<(), EditError> {
    let root = workbench.project().and_then(|root| std::fs::canonicalize(root).ok());
    let allowed: Vec<PathBuf> = allowed.iter().filter_map(|folder| std::fs::canonicalize(folder).ok()).collect();
    for file in text_files(steps) {
        let outside = || EditError::OutsideProject { path: file.path.clone() };
        let real = std::fs::canonicalize(&file.path).ok();
        if real.as_ref().is_some_and(|real| allowed.iter().any(|folder| real.starts_with(folder))) {
            continue;
        }
        let Some(root) = &root else {
            if workbench.documents.iter().any(|document| document.path() == file.path) {
                continue;
            }
            return Err(outside());
        };
        let Some(real) = real else {
            if inside(Some(root), &file.path) {
                continue;
            }
            return Err(outside());
        };
        if !real.starts_with(root) {
            return Err(outside());
        }
        if crate::guard::refuses_in(Some(root), &real) {
            return Err(EditError::ForeignSettings { path: file.path.clone() });
        }
    }
    Ok(())
}

fn text_changes(path: &Path, text: &str, edits: &Value) -> Result<Vec<TextChange>, EditError> {
    let Some(list) = edits.as_array() else {
        return if edits.is_null() { Ok(Vec::new()) } else { Err(EditError::Unsupported) };
    };
    list.iter()
        .map(|edit| {
            let place = place(path.to_owned(), text, &edit["range"]).ok_or(EditError::FilesChanged)?;
            let new_text = match (edit["snippet"]["value"].as_str(), edit["newText"].as_str()) {
                (Some(snippet), _) => super::completion::expand_snippet(snippet).0,
                (None, Some(text)) if edit["insertTextFormat"] == 2 => super::completion::expand_snippet(text).0,
                (None, Some(text)) => text.to_owned(),
                (None, None) => return Err(EditError::Unsupported),
            };
            Ok(TextChange { place, new_text })
        })
        .collect()
}

fn sort_checked(changes: &mut [TextChange]) -> Result<(), EditError> {
    changes.sort_by_key(|change| (change.place.start, change.place.end));
    if changes.windows(2).any(|pair| pair[0].place.end > pair[1].place.start) {
        return Err(EditError::Overlapping);
    }
    Ok(())
}

fn replaced(text: &str, replacements: &[(Range<usize>, String)]) -> String {
    let mut result = text.to_owned();
    for (range, new_text) in replacements.iter().rev() {
        let start = byte_offset(text, range.start);
        let end = byte_offset(text, range.end);
        result.replace_range(start..end, new_text);
    }
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn range(line: u32, start: u32, end: u32) -> Value {
        json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
    }

    #[test]
    fn text_edits_count_utf16_and_come_back_sorted_in_characters() {
        let text = "ё😀 a := 1\nb := a\n";
        let edits = json!([
            { "range": range(1, 5, 6), "newText": "név" },
            { "range": range(0, 4, 5), "newText": "név" },
            { "range": range(0, 0, 0), "newText": "// x\n" }
        ]);
        let ranges = text_edits(text, &edits).unwrap();
        assert_eq!(ranges, vec![(0..0, "// x\n".to_owned()), (3..4, "név".to_owned()), (15..16, "név".to_owned())]);
        assert_eq!(replaced(text, &ranges), "// x\nё😀 név := 1\nb := név\n");
    }

    #[test]
    fn overlapping_edits_are_refused_and_inserts_at_one_place_keep_their_order() {
        let text = "abcdef";
        let overlapping = json!([
            { "range": range(0, 1, 4), "newText": "x" },
            { "range": range(0, 3, 5), "newText": "y" }
        ]);
        assert_eq!(text_edits(text, &overlapping), Err(EditError::Overlapping));
        let inserts = json!([
            { "range": range(0, 2, 2), "newText": "1" },
            { "range": range(0, 2, 2), "newText": "2" },
            { "range": range(0, 2, 4), "newText": "" }
        ]);
        assert_eq!(replaced(text, &text_edits(text, &inserts).unwrap()), "ab12ef");
    }

    #[test]
    fn a_text_edit_is_one_undo_step_of_a_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.go");
        std::fs::write(&path, "x := 1\n").unwrap();
        let mut document = Document::open(&path).unwrap();
        let edits = json!([
            { "range": range(0, 0, 1), "newText": "y" },
            { "range": range(0, 5, 6), "newText": "2" }
        ]);
        assert_eq!(apply_text_edits(&mut document, &edits), Ok(true));
        assert_eq!(document.text(), "y := 2\n");
        assert!(document.is_dirty());
        assert!(document.undo());
        assert_eq!(document.text(), "x := 1\n");
    }
}
