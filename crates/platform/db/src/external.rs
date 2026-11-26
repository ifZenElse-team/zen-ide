use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::driver::{BoxFuture, ConnectSpec, DbError, Driver, DriverSession, ErrorKind};
use crate::model::{Capabilities, StatementKind};

mod process;
pub mod protocol;
mod session;

pub use process::DriverLog;
use process::Process;

pub const CRASH_LIMIT: usize = 3;

pub const CRASH_WINDOW: Duration = Duration::from_secs(60);

pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);

pub const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Whether a driver runs one program for each connection or one for all of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessModel {
    /// One program per connection, for all of its contexts.
    #[default]
    PerConnection,
    /// One program for every connection of the driver.
    Shared,
}

/// One platform's driver program as a plug-in's manifest lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriverBinary {
    /// Where it is downloaded from, absolute or relative to the plug-in catalog.
    pub url: String,
    /// Its SHA-256, lowercase hexadecimal; checked when it is installed and before every start.
    pub sha256: String,
    /// The file's name in the driver's folder; the last part of the URL when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Its size in bytes, when the catalog gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

impl DriverBinary {
    pub fn file_name(&self) -> Option<String> {
        let name = match &self.file {
            Some(file) => file.clone(),
            None => {
                self.url.rsplit('/').next().unwrap_or_default().split(['?', '#']).next().unwrap_or_default().to_owned()
            }
        };
        let plain = !name.is_empty()
            && name != "."
            && name != ".."
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        plain.then_some(name)
    }
}

/// A database driver as a plug-in's `plugin.json` declares it (`"databaseDrivers"`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriverDeclaration {
    /// Its stable id (`duckdb`), unique among every driver of the IDE.
    pub id: String,
    /// Its name as shown (`DuckDB`).
    pub name: String,
    /// The icon the connection dialog shows for it, by the IDE's icon names; a generic database icon when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The protocol version its program speaks.
    #[serde(default = "protocol_one")]
    pub protocol: u32,
    /// Whether it runs one program per connection or one for all.
    #[serde(default)]
    pub process: ProcessModel,
    /// Its default port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u16>,
    /// What it can do, shown before its program first runs; `initialize` answers the same.
    pub capabilities: Capabilities,
    /// The first words of statements that only read, for a language that is not SQL (`find`, `count`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_verbs: Vec<String>,
    /// Whether a read-only session of it refuses every change by itself on the server.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guards_read_only: bool,
    /// The arguments its program is started with; never secrets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Its program for each platform (`macos-aarch64`, `linux-x86_64`, `windows-x86_64`).
    #[serde(default)]
    pub platforms: BTreeMap<String, DriverBinary>,
}

fn protocol_one() -> u32 {
    protocol::VERSION
}

pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

impl DriverDeclaration {
    pub fn parse_list(json: &serde_json::Value) -> Result<Vec<Self>, String> {
        let list: Vec<Self> = serde_json::from_value(json.clone()).map_err(|error| error.to_string())?;
        for declaration in &list {
            declaration.check()?;
        }
        Ok(list)
    }

    pub fn check(&self) -> Result<(), String> {
        let plain = |text: &str| {
            !text.is_empty()
                && text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
        };
        if !plain(&self.id) {
            return Err(format!("`{}` is not a valid driver id", self.id));
        }
        if self.protocol != protocol::VERSION {
            return Err(format!(
                "{}: protocol {} is not supported, this IDE speaks {}",
                self.id,
                self.protocol,
                protocol::VERSION
            ));
        }
        if self.capabilities.models.is_empty() {
            return Err(format!("{}: no data model is declared", self.id));
        }
        for (platform, binary) in &self.platforms {
            if binary.file_name().is_none() {
                return Err(format!("{}: the program for {platform} has no plain file name", self.id));
            }
            if binary.sha256.len() != 64 || !binary.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("{}: the program for {platform} has no valid SHA-256", self.id));
            }
        }
        Ok(())
    }

    pub fn binary(&self) -> Option<&DriverBinary> {
        self.platforms.get(&platform())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalSpec {
    pub declaration: DriverDeclaration,
    pub program: PathBuf,
    pub sha256: Option<String>,
    pub plugin: Option<String>,
}

/// What an external driver is, for the driver list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalInfo {
    /// The plug-in that declares it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    /// Whether its program is installed on this computer.
    pub installed: bool,
    /// Its icon's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Whether it runs one program per connection or one for all.
    pub process: ProcessModel,
    /// The protocol version it speaks.
    pub protocol: u32,
    /// How many of its programs run now.
    pub running: usize,
}

struct Verified {
    len: u64,
    modified: Option<SystemTime>,
}

pub(crate) struct Inner {
    spec: ExternalSpec,
    handshake: OnceLock<Capabilities>,
    processes: Mutex<HashMap<String, Arc<Process>>>,
    crashes: Mutex<VecDeque<Instant>>,
    verified: Mutex<Option<Verified>>,
    log: DriverLog,
    client: protocol::ClientInfo,
    initialize_timeout: Duration,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone)]
pub struct ExternalDriver {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ExternalDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalDriver").field("id", &self.inner.spec.declaration.id).finish_non_exhaustive()
    }
}

impl ExternalDriver {
    pub fn new(spec: ExternalSpec) -> Self {
        Self::named(
            spec,
            protocol::ClientInfo { name: "Zen IDE".to_owned(), version: env!("CARGO_PKG_VERSION").to_owned() },
        )
    }

    pub fn named(spec: ExternalSpec, client: protocol::ClientInfo) -> Self {
        Self {
            inner: Arc::new(Inner {
                spec,
                handshake: OnceLock::new(),
                processes: Mutex::new(HashMap::new()),
                crashes: Mutex::new(VecDeque::new()),
                verified: Mutex::new(None),
                log: DriverLog::default(),
                client,
                initialize_timeout: INITIALIZE_TIMEOUT,
            }),
        }
    }

    #[must_use]
    pub fn initialize_within(mut self, timeout: Duration) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.initialize_timeout = timeout;
        }
        self
    }

    pub fn spec(&self) -> &ExternalSpec {
        &self.inner.spec
    }

    pub fn info(&self) -> ExternalInfo {
        let declaration = &self.inner.spec.declaration;
        ExternalInfo {
            plugin: self.inner.spec.plugin.clone(),
            installed: self.inner.spec.program.is_file(),
            icon: declaration.icon.clone(),
            process: declaration.process,
            protocol: declaration.protocol,
            running: self.running(),
        }
    }

    pub fn running(&self) -> usize {
        lock(&self.inner.processes).values().filter(|process| process.alive()).count()
    }

    pub fn log(&self) -> &DriverLog {
        &self.inner.log
    }

    pub fn stop_all(&self) {
        let processes: Vec<Arc<Process>> = lock(&self.inner.processes).drain().map(|(_, process)| process).collect();
        for process in processes {
            process.shutdown();
        }
    }
}

impl Inner {
    fn key(&self, spec: &ConnectSpec) -> String {
        match self.spec.declaration.process {
            ProcessModel::Shared => self.spec.declaration.id.clone(),
            ProcessModel::PerConnection => spec.source.id.clone(),
        }
    }

    fn crashed(&self) {
        let mut crashes = lock(&self.crashes);
        crashes.push_back(Instant::now());
        while crashes.len() > CRASH_LIMIT {
            crashes.pop_front();
        }
    }

    fn crash_looping(&self) -> bool {
        let crashes = lock(&self.crashes);
        crashes.len() >= CRASH_LIMIT && crashes.front().is_some_and(|first| first.elapsed() < CRASH_WINDOW)
    }

    async fn verify(&self) -> Result<(), DbError> {
        let program = self.spec.program.clone();
        let meta = std::fs::metadata(&program).map_err(|_| {
            DbError::new(ErrorKind::Invalid, format!("the driver program is not installed: {}", program.display()))
        })?;
        let Some(expected) = self.spec.sha256.clone() else { return Ok(()) };
        let modified = meta.modified().ok();
        if lock(&self.verified).as_ref().is_some_and(|known| known.len == meta.len() && known.modified == modified) {
            return Ok(());
        }
        let found = tokio::task::spawn_blocking(move || sha256_of(&program))
            .await
            .map_err(|error| DbError::new(ErrorKind::Invalid, error.to_string()))?
            .map_err(|error| DbError::new(ErrorKind::Invalid, error.to_string()))?;
        if !found.eq_ignore_ascii_case(expected.trim()) {
            self.log.push(format!(
                "refused to start {}: its SHA-256 is {found}, not {expected}",
                self.spec.program.display()
            ));
            return Err(DbError::new(
                ErrorKind::Invalid,
                "the driver program does not match its checksum; reinstall the plug-in that provides it",
            ));
        }
        *lock(&self.verified) = Some(Verified { len: meta.len(), modified });
        Ok(())
    }

    fn acquire(self: &Arc<Self>, key: &str) -> Result<Arc<Process>, DbError> {
        let mut processes = lock(&self.processes);
        if let Some(process) = processes.get(key).filter(|process| process.usable()) {
            process.add_session();
            return Ok(Arc::clone(process));
        }
        if self.crash_looping() {
            return Err(DbError::new(
                ErrorKind::Connection,
                format!("the driver program stopped {CRASH_LIMIT} times within a minute; try again later"),
            ));
        }
        let weak = Arc::downgrade(self);
        let on_crash = Box::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.crashed();
            }
        });
        let process = Process::spawn(&self.spec, key, self.log.clone(), on_crash)?;
        process.add_session();
        if let Some(old) = processes.insert(key.to_owned(), Arc::clone(&process)) {
            old.shutdown();
        }
        Ok(process)
    }

    pub(crate) fn release(&self, process: &Arc<Process>) {
        let mut processes = lock(&self.processes);
        if process.remove_session() > 0 {
            return;
        }
        if processes.get(process.key()).is_some_and(|known| Arc::ptr_eq(known, process)) {
            processes.remove(process.key());
        }
        drop(processes);
        process.shutdown();
    }

    async fn initialize(&self, process: &Arc<Process>) -> Result<Capabilities, DbError> {
        let params = protocol::InitializeParams {
            protocol: protocol::VERSION,
            driver: self.spec.declaration.id.clone(),
            client: self.client.clone(),
        };
        let answer: protocol::InitializeResult =
            process.request("initialize", &params, Some(self.initialize_timeout)).await?;
        if answer.protocol != protocol::VERSION {
            return Err(DbError::new(
                ErrorKind::Invalid,
                format!("the driver speaks protocol {}, this IDE speaks {}", answer.protocol, protocol::VERSION),
            ));
        }
        if answer.driver.id != self.spec.declaration.id {
            return Err(DbError::new(
                ErrorKind::Invalid,
                format!("the program is the driver `{}`, not `{}`", answer.driver.id, self.spec.declaration.id),
            ));
        }
        if answer.capabilities.models.is_empty() {
            return Err(DbError::new(ErrorKind::Invalid, "the driver declares no data model"));
        }
        self.log.push(format!("{} {} speaks protocol {}", answer.driver.name, answer.driver.version, answer.protocol));
        let _ = self.handshake.set(answer.capabilities.clone());
        Ok(answer.capabilities)
    }
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(crate::value::hex(&hasher.finalize()))
}

pub fn command_verb(statement: &str) -> &str {
    let statement = statement.trim_start();
    let first = statement.split_whitespace().next().unwrap_or_default();
    let Some(open) = statement.find('(') else { return first };
    let chain = statement[..open].trim_end();
    if chain.is_empty() || chain.contains(char::is_whitespace) {
        return first;
    }
    chain.rsplit(['.', ']']).next().unwrap_or(first)
}

impl Driver for ExternalDriver {
    fn id(&self) -> &str {
        &self.inner.spec.declaration.id
    }

    fn name(&self) -> &str {
        &self.inner.spec.declaration.name
    }

    fn capabilities(&self) -> &Capabilities {
        self.inner.handshake.get().unwrap_or(&self.inner.spec.declaration.capabilities)
    }

    fn default_port(&self) -> Option<u16> {
        self.inner.spec.declaration.default_port
    }

    fn classify(&self, statement: &str) -> StatementKind {
        if self.capabilities().language.starts_with("sql") {
            return crate::postgres::classify(statement);
        }
        let verb = command_verb(statement);
        let reads = self.inner.spec.declaration.read_verbs.iter().any(|read| read.eq_ignore_ascii_case(verb));
        if reads && !verb.is_empty() { StatementKind::Read } else { StatementKind::Write }
    }

    fn open_text(&self, path: &[String], kind: &crate::model::NodeKind) -> Option<String> {
        use crate::model::NodeKind;
        match (self.capabilities().language.as_str(), path, kind) {
            ("mongo", [database, collection], NodeKind::Collection | NodeKind::View) => {
                Some(crate::edit::document::find_text(database, collection))
            }
            (language, [database, schema, table], NodeKind::Table | NodeKind::View)
                if language.starts_with("sql") && !self.capabilities().introspection =>
            {
                let quote = |name: &str| match crate::dialect::SqlDialect::of_language(language) {
                    Some(dialect) => dialect.quote(name),
                    None => format!("\"{}\"", name.replace('"', "\"\"")),
                };
                Some(format!("SELECT * FROM {}.{}.{}", quote(database), quote(schema), quote(table)))
            }
            _ => None,
        }
    }

    fn changes_structure(&self, statement: &str) -> bool {
        self.capabilities().language.starts_with("sql")
            && self.assess(statement).effect == crate::safety::Effect::Structure
    }

    fn guards_read_only(&self) -> bool {
        self.inner.spec.declaration.guards_read_only
    }

    fn external(&self) -> Option<&ExternalDriver> {
        Some(self)
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> BoxFuture<'a, Result<Box<dyn DriverSession>, DbError>> {
        Box::pin(async move {
            self.inner.verify().await?;
            let key = self.inner.key(spec);
            let process = self.inner.acquire(&key)?;
            let capabilities = match process.ready(|| self.inner.initialize(&process)).await {
                Ok(capabilities) => capabilities,
                Err(error) => {
                    self.inner.release(&process);
                    return Err(error);
                }
            };
            match session::Session::open(Arc::clone(&self.inner), Arc::clone(&process), capabilities, spec).await {
                Ok(session) => Ok(Box::new(session) as Box<dyn DriverSession>),
                Err(error) => {
                    self.inner.release(&process);
                    Err(error)
                }
            }
        })
    }
}

#[derive(Clone, Default)]
pub struct Declared {
    drivers: Arc<Mutex<Vec<Arc<ExternalDriver>>>>,
}

impl Declared {
    pub fn set(&self, specs: Vec<ExternalSpec>) {
        let mut declared = lock(&self.drivers);
        let mut next: Vec<Arc<ExternalDriver>> = Vec::with_capacity(specs.len());
        for spec in specs {
            if next.iter().any(|known| known.id() == spec.declaration.id) {
                continue;
            }
            match declared.iter().position(|known| *known.spec() == spec) {
                Some(at) => next.push(declared.remove(at)),
                None => next.push(Arc::new(ExternalDriver::new(spec))),
            }
        }
        let gone = std::mem::replace(&mut *declared, next);
        drop(declared);
        for driver in gone {
            driver.stop_all();
        }
    }

    pub fn drivers(&self) -> Vec<Arc<ExternalDriver>> {
        lock(&self.drivers).clone()
    }

    pub fn as_drivers(&self) -> Vec<Arc<dyn Driver>> {
        self.drivers().into_iter().map(|driver| driver as Arc<dyn Driver>).collect()
    }
}

#[cfg(test)]
mod tests;
