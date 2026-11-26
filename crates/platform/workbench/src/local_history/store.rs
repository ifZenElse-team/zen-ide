use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const FORMAT: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_age: Duration,
    pub max_bytes: u64,
    pub max_file_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_age: Duration::from_secs(5 * 24 * 3600), max_bytes: 512 << 20, max_file_bytes: 16 << 20 }
    }
}

/// What made a version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum VersionKind {
    /// The file was saved in the IDE.
    Save,
    /// The file changed outside the IDE, or the IDE saw its content for the first time.
    External,
    /// The content just before an IDE action changed the file; the label names the action.
    Before,
    /// The content an IDE action wrote; the label names the action.
    Action,
    /// The file was deleted.
    Deleted,
    /// A label put by the user; it has no content.
    Label,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: u64,
    pub time: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub kind: VersionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub binary: bool,
}

impl Entry {
    pub fn is_under(&self, relative: &str) -> bool {
        match &self.path {
            Some(path) => {
                relative.is_empty()
                    || path == relative
                    || path.strip_prefix(relative).is_some_and(|rest| rest.starts_with('/'))
            }
            None => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    pub entries: usize,
    pub contents: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectFile {
    root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoreFile {
    format: u32,
}

#[derive(Debug)]
struct Project {
    dir: PathBuf,
    entries: Vec<Entry>,
    next_id: u64,
    log: Option<zen_vfs::Stamp>,
}

#[derive(Debug)]
pub struct Store {
    folder: PathBuf,
    limits: Limits,
    projects: BTreeMap<PathBuf, Project>,
    objects: HashMap<String, u64>,
    writable: bool,
}

const LOG: &str = "log.jsonl";
const PROJECT: &str = "project.json";
const LOCK: &str = "store.lock";

impl Store {
    pub fn open(folder: &Path, limits: Limits) -> Self {
        let mut store = Self {
            folder: folder.to_owned(),
            limits,
            projects: BTreeMap::new(),
            objects: HashMap::new(),
            writable: true,
        };
        let format_file = folder.join("store.json");
        match std::fs::read(&format_file) {
            Ok(bytes) => {
                let format = serde_json::from_slice::<StoreFile>(&bytes).map_or(FORMAT, |file| file.format);
                store.writable = format <= FORMAT;
            }
            Err(_) => {
                if std::fs::create_dir_all(folder).is_ok() {
                    let json = serde_json::to_vec(&StoreFile { format: FORMAT }).unwrap_or_default();
                    let _ = zen_vfs::write_atomic(&format_file, &json);
                }
            }
        }
        if !store.writable {
            return store;
        }
        let lock = store.lock().ok();
        store.read_objects(lock.is_some());
        store.reload(lock.is_some());
        store
    }

    // WHY: several IDE processes share this folder; every write takes the lock and reads what the others wrote.
    fn lock(&self) -> io::Result<std::fs::File> {
        std::fs::create_dir_all(&self.folder)?;
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.folder.join(LOCK))?;
        lock.lock()?;
        Ok(lock)
    }

    fn read_objects(&mut self, exclusive: bool) {
        let Ok(prefixes) = std::fs::read_dir(self.folder.join("objects")) else { return };
        for prefix in prefixes.flatten() {
            let name = prefix.file_name().to_string_lossy().into_owned();
            let Ok(files) = std::fs::read_dir(prefix.path()) else { continue };
            for file in files.flatten() {
                let rest = file.file_name().to_string_lossy().into_owned();
                if rest.starts_with('.') {
                    if exclusive {
                        let _ = std::fs::remove_file(file.path());
                    }
                    continue;
                }
                let hash = format!("{name}{rest}");
                if !is_hash(&hash) {
                    continue;
                }
                let size = file.metadata().map_or(0, |metadata| metadata.len());
                self.objects.insert(hash, size);
            }
        }
    }

    pub fn refresh(&mut self) {
        if self.writable {
            self.reload(false);
        }
    }

    fn reload(&mut self, repair: bool) {
        let mut known: HashMap<PathBuf, (PathBuf, Project)> = std::mem::take(&mut self.projects)
            .into_iter()
            .map(|(root, project)| (project.dir.clone(), (root, project)))
            .collect();
        let Ok(dirs) = std::fs::read_dir(self.folder.join("projects")) else { return };
        for dir in dirs.flatten() {
            let dir = dir.path();
            let log = zen_vfs::stamp(&dir.join(LOG));
            let (root, project) = match known.remove(&dir) {
                Some((root, project)) if project.log == log => (root, project),
                earlier => {
                    let Some(root) = earlier.map(|(root, _)| root).or_else(|| project_root(&dir)) else { continue };
                    let (entries, next_id) = read_log(&dir, repair);
                    (root, Project { dir, entries, next_id, log })
                }
            };
            self.projects.insert(root, project);
        }
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn projects(&self) -> impl Iterator<Item = &Path> {
        self.projects.keys().map(PathBuf::as_path)
    }

    pub fn entries(&self, root: &Path) -> &[Entry] {
        self.projects.get(root).map_or(&[], |project| project.entries.as_slice())
    }

    pub fn latest(&self, root: &Path, relative: &str) -> Option<&Entry> {
        self.entries(root)
            .iter()
            .rev()
            .find(|entry| entry.kind != VersionKind::Label && entry.path.as_deref() == Some(relative))
    }

    pub fn stored_bytes(&self) -> u64 {
        self.objects.values().sum()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        root: &Path,
        relative: &str,
        kind: VersionKind,
        label: Option<String>,
        content: Option<&[u8]>,
        time: u64,
        dedupe: bool,
    ) -> io::Result<Option<u64>> {
        if !self.writable || content.is_some_and(|content| content.len() as u64 > self.limits.max_file_bytes) {
            return Ok(None);
        }
        let lock = self.lock().ok();
        self.reload(lock.is_some());
        let hash = content.map(hash_of);
        if dedupe {
            let latest = self.latest(root, relative);
            let same = match (&hash, latest) {
                (None, None) => true,
                (None, Some(latest)) => latest.kind == VersionKind::Deleted,
                (Some(hash), Some(latest)) => latest.hash.as_ref() == Some(hash),
                (Some(_), None) => false,
            };
            if same {
                return Ok(None);
            }
        }
        if let (Some(hash), Some(content)) = (&hash, content) {
            self.write_object(hash, content)?;
        }
        let entry = Entry {
            id: 0,
            time,
            path: Some(relative.to_owned()),
            kind,
            label,
            hash,
            size: content.map_or(0, |content| content.len() as u64),
            binary: content.is_some_and(is_binary),
        };
        self.append(root, entry).map(Some)
    }

    pub fn label(&mut self, root: &Path, relative: Option<&str>, text: &str, time: u64) -> io::Result<u64> {
        if !self.writable {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the local history is of a newer format"));
        }
        let lock = self.lock().ok();
        self.reload(lock.is_some());
        let entry = Entry {
            id: 0,
            time,
            path: relative.map(str::to_owned),
            kind: VersionKind::Label,
            label: Some(text.to_owned()),
            hash: None,
            size: 0,
            binary: false,
        };
        self.append(root, entry)
    }

    fn append(&mut self, root: &Path, mut entry: Entry) -> io::Result<u64> {
        let project = self.project(root)?;
        entry.id = project.next_id;
        let mut line = serde_json::to_vec(&entry).map_err(io::Error::other)?;
        line.push(b'\n');
        let mut log = std::fs::OpenOptions::new().create(true).append(true).open(project.dir.join(LOG))?;
        log.write_all(&line)?;
        project.next_id += 1;
        project.log = zen_vfs::stamp(&project.dir.join(LOG));
        let id = entry.id;
        project.entries.push(entry);
        Ok(id)
    }

    fn project(&mut self, root: &Path) -> io::Result<&mut Project> {
        if !self.projects.contains_key(root) {
            let dir = self.folder.join("projects").join(&hash_of(root.to_string_lossy().as_bytes())[..16]);
            std::fs::create_dir_all(&dir)?;
            let json = serde_json::to_vec_pretty(&ProjectFile { root: root.to_owned() }).map_err(io::Error::other)?;
            zen_vfs::write_atomic(&dir.join(PROJECT), &json).map_err(|error| error.source)?;
            let (entries, next_id) = read_log(&dir, false);
            let log = zen_vfs::stamp(&dir.join(LOG));
            self.projects.insert(root.to_owned(), Project { dir, entries, next_id, log });
        }
        self.projects.get_mut(root).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn object_path(&self, hash: &str) -> PathBuf {
        self.folder.join("objects").join(&hash[..2]).join(&hash[2..])
    }

    fn write_object(&mut self, hash: &str, content: &[u8]) -> io::Result<()> {
        let path = self.object_path(hash);
        if self.objects.contains_key(hash) && path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let encoded = encode(content);
        zen_vfs::write_atomic(&path, &encoded).map_err(|error| error.source)?;
        self.objects.insert(hash.to_owned(), encoded.len() as u64);
        Ok(())
    }

    pub fn content(&self, hash: &str) -> io::Result<Vec<u8>> {
        if !is_hash(hash) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        decode(&std::fs::read(self.object_path(hash))?)
    }

    pub fn prune(&mut self, now: u64) -> io::Result<Pruned> {
        if !self.writable {
            return Ok(Pruned::default());
        }
        let Ok(_lock) = self.lock() else { return Ok(Pruned::default()) };
        self.objects.clear();
        self.read_objects(true);
        self.reload(true);
        let cutoff = now.saturating_sub(u64::try_from(self.limits.max_age.as_millis()).unwrap_or(u64::MAX));
        let mut pruned = Pruned::default();
        let mut changed: HashSet<PathBuf> = HashSet::new();
        for (root, project) in &mut self.projects {
            let before = project.entries.len();
            let kept = contents_before_deletions(&project.entries, cutoff);
            project.entries.retain(|entry| entry.time >= cutoff || kept.contains(&entry.id));
            if project.entries.len() != before {
                pruned.entries += before - project.entries.len();
                changed.insert(root.clone());
            }
        }
        let mut references: HashMap<String, usize> = HashMap::new();
        for entry in self.projects.values().flat_map(|project| &project.entries) {
            if let Some(hash) = &entry.hash {
                *references.entry(hash.clone()).or_default() += 1;
            }
        }
        let size = |hash: &str| self.objects.get(hash).copied().unwrap_or(0);
        let mut total: u64 = references.keys().map(|hash| size(hash)).sum();
        if total > self.limits.max_bytes {
            let mut oldest: Vec<(u64, u64, &Path, &str)> = self
                .projects
                .iter()
                .flat_map(|(root, project)| {
                    project
                        .entries
                        .iter()
                        .filter_map(|entry| Some((entry.time, entry.id, root.as_path(), entry.hash.as_deref()?)))
                })
                .collect();
            oldest.sort_unstable();
            let mut dropped: HashMap<PathBuf, HashSet<u64>> = HashMap::new();
            for (_, id, root, hash) in oldest {
                if total <= self.limits.max_bytes {
                    break;
                }
                if let Some(count) = references.get_mut(hash) {
                    *count -= 1;
                    if *count == 0 {
                        total = total.saturating_sub(size(hash));
                        references.remove(hash);
                    }
                }
                dropped.entry(root.to_owned()).or_default().insert(id);
            }
            for (root, ids) in dropped {
                if let Some(project) = self.projects.get_mut(&root) {
                    project.entries.retain(|entry| !ids.contains(&entry.id));
                    pruned.entries += ids.len();
                    changed.insert(root);
                }
            }
        }
        for root in changed {
            let Some(project) = self.projects.get_mut(&root) else { continue };
            if project.entries.is_empty() {
                let _ = std::fs::remove_dir_all(&project.dir);
                self.projects.remove(&root);
                continue;
            }
            let mut log = Vec::new();
            for entry in &project.entries {
                log.extend(serde_json::to_vec(entry).map_err(io::Error::other)?);
                log.push(b'\n');
            }
            zen_vfs::write_atomic(&project.dir.join(LOG), &log).map_err(|error| error.source)?;
            project.log = zen_vfs::stamp(&project.dir.join(LOG));
        }
        let unused: Vec<String> = self.objects.keys().filter(|hash| !references.contains_key(*hash)).cloned().collect();
        for hash in unused {
            let path = self.object_path(&hash);
            if std::fs::remove_file(&path).is_ok() || !path.exists() {
                self.objects.remove(&hash);
                pruned.contents += 1;
            }
        }
        Ok(pruned)
    }
}

fn project_root(dir: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(dir.join(PROJECT)).ok()?;
    serde_json::from_slice::<ProjectFile>(&bytes).ok().map(|project| project.root)
}

fn read_log(dir: &Path, repair: bool) -> (Vec<Entry>, u64) {
    let path = dir.join(LOG);
    let mut bytes = std::fs::read(&path).unwrap_or_default();
    if repair && bytes.last().is_some_and(|byte| *byte != b'\n') {
        let whole = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
        bytes.truncate(whole);
        if let Ok(log) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = log.set_len(whole as u64);
        }
    }
    let entries: Vec<Entry> =
        bytes.split(|byte| *byte == b'\n').filter_map(|line| serde_json::from_slice(line).ok()).collect();
    let next_id = entries.iter().map(|entry| entry.id.saturating_add(1)).max().unwrap_or(1);
    (entries, next_id)
}

fn contents_before_deletions(entries: &[Entry], cutoff: u64) -> HashSet<u64> {
    let mut last: HashMap<&str, &Entry> = HashMap::new();
    let mut kept = HashSet::new();
    for entry in entries.iter().filter(|entry| entry.kind != VersionKind::Label) {
        let Some(path) = entry.path.as_deref() else { continue };
        if entry.time < cutoff {
            last.insert(path, entry);
        } else if let Some(previous) = last.remove(path)
            && entry.kind == VersionKind::Deleted
            && previous.hash.is_some()
        {
            kept.insert(previous.id);
        }
    }
    kept
}

fn is_hash(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn hash_of(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
}

fn encode(content: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(vec![b'z'], flate2::Compression::default());
    let compressed = encoder.write_all(content).and_then(|()| encoder.finish());
    match compressed {
        Ok(compressed) if compressed.len() < content.len() + 1 => compressed,
        _ => {
            let mut raw = Vec::with_capacity(content.len() + 1);
            raw.push(b'r');
            raw.extend_from_slice(content);
            raw
        }
    }
}

fn decode(stored: &[u8]) -> io::Result<Vec<u8>> {
    match stored.split_first() {
        Some((b'r', rest)) => Ok(rest.to_vec()),
        Some((b'z', rest)) => {
            let mut content = Vec::new();
            flate2::read::ZlibDecoder::new(rest).read_to_end(&mut content)?;
            Ok(content)
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "unknown content encoding")),
    }
}

#[cfg(test)]
mod tests;
