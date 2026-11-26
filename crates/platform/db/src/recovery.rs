use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::SqlDialect;
use crate::driver::ContextKind;
use crate::edit::{Changes, Editing};
use crate::journal::{self, Appender};

pub const FILE: &str = "recovery.jsonl";

const PREFIX: &str = "recovery-";

static OPENED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub const MOST_RECORD: usize = 16 * 1024 * 1024;

const COMPACT_SLACK: u64 = 1024 * 1024;

/// The unsaved text of a console.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleText {
    /// The console's file.
    pub path: PathBuf,
    /// Its text as it was in the editor.
    pub text: String,
    /// When it was kept, in milliseconds since 1970.
    pub at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedEdits {
    pub connection: String,
    pub connection_name: String,
    pub dialect: String,
    pub editing: Editing,
    pub changes: Changes,
    pub statement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    pub at: i64,
}

impl StagedEdits {
    pub fn editing(&self) -> Editing {
        let mut editing = self.editing.clone();
        editing.dialect = dialect_of(&self.dialect).unwrap_or_default();
        editing
    }
}

/// A transaction that was open, which the server rolled back when the IDE's connection ended.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenTransaction {
    /// The connection's id.
    pub connection: String,
    /// The connection's name.
    pub connection_name: String,
    /// The context it was open in.
    pub context: ContextKind,
    /// Statements run in it.
    pub statements: usize,
    /// Of those, statements that may have changed data.
    pub modifying: usize,
    /// The first line of its last statements, passwords masked, the most recent last.
    pub log: Vec<String>,
    /// When it was seen last, in milliseconds since 1970.
    pub at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Item {
    Console(ConsoleText),
    Edits(Box<StagedEdits>),
    Transaction(OpenTransaction),
}

pub fn dialect_name(dialect: SqlDialect) -> &'static str {
    match dialect {
        SqlDialect::Postgres => "postgres",
        SqlDialect::MySql => "mysql",
        SqlDialect::MariaDb => "mariadb",
        SqlDialect::Sqlite => "sqlite",
        SqlDialect::ClickHouse => "clickhouse",
        SqlDialect::SqlServer => "sqlserver",
        SqlDialect::Oracle => "oracle",
    }
}

pub fn dialect_of(name: &str) -> Option<SqlDialect> {
    Some(match name {
        "postgres" => SqlDialect::Postgres,
        "mysql" => SqlDialect::MySql,
        "mariadb" => SqlDialect::MariaDb,
        "sqlite" => SqlDialect::Sqlite,
        "clickhouse" => SqlDialect::ClickHouse,
        "sqlserver" => SqlDialect::SqlServer,
        "oracle" => SqlDialect::Oracle,
        _ => return None,
    })
}

#[derive(Serialize)]
struct Written<'a> {
    key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<&'a Item>,
}

#[derive(Deserialize)]
struct Read {
    key: String,
    #[serde(default)]
    item: Option<Item>,
}

#[derive(Default)]
struct Inner {
    items: BTreeMap<String, Arc<Item>>,
    recovered: BTreeMap<u64, String>,
    next: u64,
    loaded: bool,
    announced: bool,
    revision: u64,
    error: Option<String>,
}

enum Job {
    Put(String),
    Remove(String),
    Flush(Sender<()>),
}

pub struct Recovery {
    folder: PathBuf,
    file: PathBuf,
    inner: Arc<Mutex<Inner>>,
    jobs: Mutex<Sender<Job>>,
    writer: Mutex<Option<std::thread::JoinHandle<()>>>,
    lock: Option<(PathBuf, std::fs::File)>,
}

impl Drop for Recovery {
    fn drop(&mut self) {
        drop(std::mem::replace(&mut *self.jobs.lock().unwrap_or_else(PoisonError::into_inner), channel().0));
        if let Some(writer) = self.writer.lock().unwrap_or_else(PoisonError::into_inner).take() {
            let _ = writer.join();
        }
        if let Some((path, file)) = self.lock.take() {
            drop(file);
            if !self.file.exists() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn lock_of(file: &Path) -> PathBuf {
    file.with_extension("lock")
}

fn try_lock(file: &Path) -> Option<std::fs::File> {
    let path = lock_of(file);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).ok()?;
    }
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).ok()?;
    lock.try_lock().ok()?;
    Some(lock)
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Recovery {
    pub fn shared(folder: &Path) -> Arc<Recovery> {
        static OPEN: OnceLock<Mutex<HashMap<PathBuf, Arc<Recovery>>>> = OnceLock::new();
        let mut open = OPEN.get_or_init(Mutex::default).lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(open.entry(folder.to_owned()).or_insert_with(|| Arc::new(Recovery::open(folder))))
    }

    pub fn open(folder: &Path) -> Recovery {
        let n = OPENED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let file = folder.join(format!("{PREFIX}{}-{n}.jsonl", std::process::id()));
        let held = try_lock(&file).map(|held| (lock_of(&file), held));
        let inner = Arc::new(Mutex::new(Inner::default()));
        if held.is_none() {
            lock(&inner).error = Some(format!("{}: the lock could not be taken", lock_of(&file).display()));
        }
        let (sender, receiver) = channel();
        let writer = Writer {
            inner: Arc::clone(&inner),
            appender: Appender::new(file.clone()),
            file_bytes: 0,
            sizes: HashMap::new(),
        };
        let spawned =
            std::thread::Builder::new().name("zen-db-recovery".to_owned()).spawn(move || writer.run(receiver));
        let writer = match spawned {
            Ok(handle) => Some(handle),
            Err(error) => {
                lock(&inner).error = Some(error.to_string());
                None
            }
        };
        Recovery {
            folder: folder.to_owned(),
            file,
            inner,
            jobs: Mutex::new(sender),
            writer: Mutex::new(writer),
            lock: held,
        }
    }

    pub fn file(&self) -> PathBuf {
        self.file.clone()
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    fn send(&self, job: Job) {
        let _ = self.jobs.lock().unwrap_or_else(PoisonError::into_inner).send(job);
    }

    pub fn put(&self, key: &str, item: Item) {
        {
            let mut inner = lock(&self.inner);
            inner.items.insert(key.to_owned(), Arc::new(item));
            inner.revision += 1;
        }
        self.send(Job::Put(key.to_owned()));
    }

    pub fn remove(&self, key: &str) {
        {
            let mut inner = lock(&self.inner);
            if inner.items.remove(key).is_none() {
                return;
            }
            inner.recovered.retain(|_, known| known != key);
            inner.revision += 1;
        }
        self.send(Job::Remove(key.to_owned()));
    }

    pub fn get(&self, key: &str) -> Option<Arc<Item>> {
        lock(&self.inner).items.get(key).cloned()
    }

    pub fn loaded(&self) -> bool {
        lock(&self.inner).loaded
    }

    pub fn recovered(&self) -> Vec<(u64, String, Arc<Item>)> {
        let inner = lock(&self.inner);
        inner
            .recovered
            .iter()
            .filter_map(|(id, key)| inner.items.get(key).map(|item| (*id, key.clone(), Arc::clone(item))))
            .collect()
    }

    pub fn recovered_item(&self, id: u64) -> Option<(String, Arc<Item>)> {
        let inner = lock(&self.inner);
        let key = inner.recovered.get(&id)?;
        inner.items.get(key).map(|item| (key.clone(), Arc::clone(item)))
    }

    pub fn discard(&self, id: u64) -> bool {
        let key = lock(&self.inner).recovered.get(&id).cloned();
        match key {
            Some(key) => {
                self.remove(&key);
                true
            }
            None => false,
        }
    }

    pub fn adopt(&self, id: u64) {
        let mut inner = lock(&self.inner);
        if inner.recovered.remove(&id).is_some() {
            inner.revision += 1;
        }
    }

    pub fn announce(&self) -> bool {
        let mut inner = lock(&self.inner);
        if inner.announced || !inner.loaded || inner.recovered.is_empty() {
            return false;
        }
        inner.announced = true;
        true
    }

    pub fn flush(&self, timeout: Duration) -> bool {
        let (done, wait) = channel();
        self.send(Job::Flush(done));
        wait.recv_timeout(timeout).is_ok()
    }

    pub fn revision(&self) -> u64 {
        lock(&self.inner).revision
    }

    pub fn error(&self) -> Option<String> {
        lock(&self.inner).error.clone()
    }
}

struct Writer {
    inner: Arc<Mutex<Inner>>,
    appender: Appender,
    file_bytes: u64,
    sizes: HashMap<String, u64>,
}

impl Writer {
    fn run(mut self, jobs: Receiver<Job>) {
        self.load();
        loop {
            let first = match self.appender.sync_due(Instant::now()) {
                None => match jobs.recv() {
                    Ok(job) => job,
                    Err(_) => break,
                },
                Some(left) => match jobs.recv_timeout(left) {
                    Ok(job) => job,
                    Err(RecvTimeoutError::Timeout) => {
                        self.sync();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                },
            };
            let mut keys: Vec<String> = Vec::new();
            let mut flushes = Vec::new();
            for job in std::iter::once(first).chain(jobs.try_iter()) {
                match job {
                    Job::Put(key) | Job::Remove(key) => {
                        if !keys.contains(&key) {
                            keys.push(key);
                        }
                    }
                    Job::Flush(done) => flushes.push(done),
                }
            }
            for key in keys {
                self.write(&key);
            }
            self.settle();
            if flushes.is_empty() {
                if let Err(error) = self.appender.sync_if_due(Instant::now()) {
                    self.fail(&error);
                }
            } else {
                self.sync();
                for done in flushes {
                    let _ = done.send(());
                }
            }
        }
        self.sync();
    }

    fn fail(&self, error: &std::io::Error) {
        lock(&self.inner).error = Some(error.to_string());
    }

    fn sync(&mut self) {
        if let Err(error) = self.appender.sync() {
            self.fail(&error);
        }
    }

    fn orphans(&self) -> Vec<(PathBuf, Option<std::fs::File>)> {
        let own = self.appender.path().to_owned();
        let Some(folder) = own.parent() else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                name == FILE || (name.starts_with(PREFIX) && name.ends_with(".jsonl"))
            })
            .collect();
        found.sort();
        found
            .into_iter()
            .filter_map(|path| {
                if path == own || path.file_name().is_some_and(|name| name == FILE) {
                    return Some((path, None));
                }
                try_lock(&path).map(|held| (path, Some(held)))
            })
            .collect()
    }

    fn load(&mut self) {
        let own = self.appender.path().to_owned();
        let orphans = self.orphans();
        let mut items: BTreeMap<String, Arc<Item>> = BTreeMap::new();
        let mut read_all = true;
        for (path, _) in &orphans {
            match std::fs::read(path) {
                Ok(bytes) => {
                    let decoded = journal::decode::<Read>(&bytes);
                    let mut kept: BTreeMap<String, Arc<Item>> = BTreeMap::new();
                    for read in decoded.records {
                        match read.item {
                            Some(item) => kept.insert(read.key, Arc::new(item)),
                            None => kept.remove(&read.key),
                        };
                    }
                    items.extend(kept);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    read_all = false;
                    self.fail(&error);
                }
            }
        }
        {
            let mut inner = lock(&self.inner);
            for (key, item) in items {
                inner.next += 1;
                let id = inner.next;
                inner.recovered.insert(id, key.clone());
                inner.items.entry(key).or_insert(item);
            }
            inner.loaded = true;
            inner.revision += 1;
        }
        if !orphans.is_empty() && self.compact() && read_all {
            for (path, held) in orphans {
                if path == own {
                    continue;
                }
                drop(held);
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_file(lock_of(&path));
            }
        }
        self.forget_stale_locks();
    }

    fn forget_stale_locks(&self) {
        let own = lock_of(self.appender.path());
        let Some(folder) = own.parent() else { return };
        let Ok(entries) = std::fs::read_dir(folder) else { return };
        for path in entries.flatten().map(|entry| entry.path()) {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            if path == own || !name.starts_with(PREFIX) || !name.ends_with(".lock") {
                continue;
            }
            let journal = path.with_extension("jsonl");
            if journal.exists() {
                continue;
            }
            if let Some(held) = try_lock(&journal) {
                drop(held);
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    fn write(&mut self, key: &str) {
        let item = lock(&self.inner).items.get(key).cloned();
        if item.is_none() && !self.sizes.contains_key(key) {
            return;
        }
        let bytes = journal::encode(&Written { key, item: item.as_deref() });
        if bytes.is_empty() {
            return;
        }
        if bytes.len() > MOST_RECORD {
            lock(&self.inner).error = Some(format!("{key}: {} bytes is more than a record may take", bytes.len()));
            return;
        }
        if let Err(error) = self.appender.append(&bytes) {
            return self.fail(&error);
        }
        self.file_bytes += bytes.len() as u64;
        match item {
            Some(_) => self.sizes.insert(key.to_owned(), bytes.len() as u64),
            None => self.sizes.remove(key),
        };
    }

    fn settle(&mut self) {
        if self.sizes.is_empty() && lock(&self.inner).items.is_empty() {
            if self.file_bytes > 0 {
                self.appender.close();
                if let Err(error) = journal::remove(self.appender.path()) {
                    self.fail(&error);
                }
                self.file_bytes = 0;
            }
            return;
        }
        let live: u64 = self.sizes.values().sum();
        if self.file_bytes > live * 2 + COMPACT_SLACK {
            let _ = self.compact();
        }
    }

    fn compact(&mut self) -> bool {
        self.sync();
        self.appender.close();
        let items: Vec<(String, Arc<Item>)> =
            lock(&self.inner).items.iter().map(|(key, item)| (key.clone(), Arc::clone(item))).collect();
        self.sizes.clear();
        let mut bytes = Vec::new();
        for (key, item) in &items {
            let record = journal::encode(&Written { key, item: Some(item) });
            if record.is_empty() || record.len() > MOST_RECORD {
                continue;
            }
            self.sizes.insert(key.clone(), record.len() as u64);
            bytes.extend_from_slice(&record);
        }
        let path = self.appender.path().to_owned();
        let written = if bytes.is_empty() { journal::remove(&path) } else { journal::write_atomic(&path, &bytes) };
        match written {
            Ok(()) => {
                self.file_bytes = bytes.len() as u64;
                true
            }
            Err(error) => {
                self.fail(&error);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests;
