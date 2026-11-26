pub mod completion;
pub mod inspector;
pub mod sources;
pub mod versions;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::inspect::{FixWrite, Wake};

use crate::Workbench;
use crate::build_tools::BuildEnv;
use crate::plugin_projects::ProjectSource;
use crate::run::project_root;
use sources::{Advisory, Answer, Endpoints, IndexQuery, NoNetwork, Repository, ShownAdvisory, Transport, Validators};

pub const SEARCH_SETTING: &str = "dependencies.search";
pub const CHECK_UPDATES: &str = "dependencies.checkUpdates";
pub const CHECK_VULNERABILITIES: &str = "dependencies.checkVulnerabilities";

pub const FRESH: Duration = Duration::from_secs(60 * 60);
const RETRY: Duration = Duration::from_secs(5 * 60);
const WORKERS: usize = 4;
const IDLE: Duration = Duration::from_secs(20);
const MAX_WAIT: Duration = Duration::from_secs(60);

/// Where a declared dependency's version is written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VersionPlace {
    /// On the dependency itself.
    #[default]
    Literal,
    /// In a property or a variable of the build.
    Property {
        /// The property's name.
        name: String,
    },
    /// In the version catalog.
    Catalog {
        /// The library's alias there.
        alias: String,
    },
    /// In a parent or a BOM of the project itself: Upgrade edits that build file.
    ManagedHere {
        /// The build file that manages it.
        by: String,
    },
    /// In a parent or a BOM from a repository: the version cannot be changed here.
    Managed {
        /// The coordinates of what manages it.
        by: String,
    },
    /// Nowhere the IDE can see.
    Unknown,
}

impl VersionPlace {
    pub fn changes(&self) -> bool {
        !matches!(self, VersionPlace::Managed { .. } | VersionPlace::Unknown)
    }
}

/// What the network says about a declared dependency.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencyStatus {
    /// Whether answers are still on their way.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
    /// The latest stable version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// The versions newer than the current one that are offered, newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub newer: Vec<String>,
    /// The known vulnerabilities of the current version.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub advisories: Vec<ShownAdvisory>,
    /// The nearest newer version without them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe: Option<String>,
}

/// A dependency a module declares.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredDependency {
    /// Group id.
    pub group: String,
    /// Artifact id.
    pub artifact: String,
    /// The version in effect, as far as known; empty when it is not.
    pub version: String,
    /// The Maven scope or the Gradle configuration.
    pub scope: String,
    /// Where the version is written.
    pub version_place: VersionPlace,
    /// Whether a module of the project itself gives it: repositories are not asked about it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub local: bool,
    /// What repositories and advisories say about it.
    pub status: DependencyStatus,
}

/// A module of the open project, as the Dependencies tool window lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencyModule {
    /// Its id for the other `dependencies.*` commands: its build file, relative to the project.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its build file.
    pub file: PathBuf,
    /// The build system's plug-in, e.g. `maven`.
    pub provider: String,
    /// The Maven scopes or Gradle configurations a dependency can be added with.
    pub scopes: Vec<String>,
    /// The repositories its dependencies are looked up in.
    pub repositories: Vec<Repository>,
    /// The dependencies it declares.
    pub dependencies: Vec<DeclaredDependency>,
    /// Why its build file did not read, when it did not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencyChange {
    Add { module: String, group: String, artifact: String, version: String, scope: String },
    SetVersion { module: String, group: String, artifact: String, version: String },
    Remove { module: String, group: String, artifact: String, scope: Option<String> },
    Exclude { module: String, group: String, artifact: String, excluded: String },
    AddBom { module: String, group: String, artifact: String, version: String },
    AddRepository { module: String, id: String, name: String, url: String, snapshots: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionSite {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completing {
    Group { prefix: String, from: usize },
    Artifact { group: String, prefix: String, from: usize },
    Version { group: String, artifact: String, prefix: String, from: usize },
}

pub type RemotePom<'a> = &'a dyn Fn(&[Repository], &str, &str, &str) -> Option<String>;

pub struct BuildFiles<'a> {
    pub root: &'a Path,
    pub env: &'a BuildEnv,
    pub open: &'a HashMap<PathBuf, String>,
    pub remote_pom: RemotePom<'a>,
    pub seen: RefCell<BTreeMap<PathBuf, Option<String>>>,
}

impl BuildFiles<'_> {
    pub fn read(&self, path: &Path) -> Option<String> {
        let text = self.open.get(path).cloned().or_else(|| std::fs::read_to_string(path).ok());
        self.seen.borrow_mut().entry(path.to_owned()).or_insert_with(|| text.clone());
        text
    }

    pub(crate) fn keep_reads(&self, kept: &mut HashMap<PathBuf, String>, planned: impl Fn(&Path) -> bool) {
        for (path, text) in self.seen.borrow().iter().filter(|(path, _)| !planned(path)) {
            kept.entry(path.clone()).or_insert_with(|| text.clone().unwrap_or_default());
        }
    }

    pub fn trusted(&self) -> bool {
        crate::trust::is_trusted(self.env, self.root)
    }
}

pub trait DependencyProvider: Send + Sync {
    fn id(&self) -> &str;

    fn applies(&self, root: &Path) -> bool;

    fn handles(&self, file_name: &str) -> bool;

    fn modules(&self, files: &BuildFiles<'_>) -> Vec<DependencyModule>;

    fn change(&self, files: &BuildFiles<'_>, change: &DependencyChange) -> Result<Vec<FileChange>, String>;

    fn sites(&self, files: &BuildFiles<'_>, path: &Path, text: &str) -> Vec<VersionSite>;

    fn completing(&self, path: &Path, text: &str, caret: usize) -> Option<Completing>;

    fn repositories_for(&self, files: &BuildFiles<'_>, path: &Path) -> Vec<Repository>;

    fn offline(&self, _files: &BuildFiles<'_>) -> bool {
        false
    }

    fn reload(&self, session: &mut Session) -> Result<(), CommandError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allowed {
    pub search: bool,
    pub updates: bool,
    pub advisories: bool,
}

impl Allowed {
    pub fn of(setting: &dyn Fn(&str) -> Option<Value>, offline: bool) -> Self {
        let on = |key: &str| !offline && setting(key).and_then(|value| value.as_bool()).unwrap_or(false);
        Self { search: on(SEARCH_SETTING), updates: on(CHECK_UPDATES), advisories: on(CHECK_VULNERABILITIES) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Task {
    Search(IndexQuery),
    Metadata { repository: Repository, group: String, artifact: String },
    Advisories { group: String, artifact: String },
    Pom { repositories: Vec<Repository>, group: String, artifact: String, version: String },
}

struct Slot<T> {
    value: Option<T>,
    at: Instant,
    failed: bool,
}

impl<T> Slot<T> {
    fn stale(&self) -> bool {
        self.at.elapsed() >= if self.failed { RETRY } else { FRESH }
    }
}

#[derive(Default)]
struct State {
    searches: HashMap<IndexQuery, Slot<Vec<(String, String)>>>,
    lists: HashMap<(String, String, String), Slot<Vec<String>>>,
    advisories: HashMap<(String, String), Slot<Vec<Advisory>>>,
    poms: HashMap<(String, String, String), Slot<String>>,
    read: Option<ReadModules>,
}

#[derive(Clone)]
struct ReadModules {
    root: PathBuf,
    poms: u64,
    open: u64,
    files: Vec<(PathBuf, Stamp)>,
    modules: Vec<DependencyModule>,
}

type Stamp = Option<(SystemTime, u64)>;

fn stamp(path: &Path) -> Stamp {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

#[derive(Default)]
struct Queue {
    tasks: VecDeque<Task>,
    known: HashSet<Task>,
    workers: usize,
    running: usize,
}

struct Shared {
    transport: RwLock<Arc<dyn Transport>>,
    endpoints: RwLock<Endpoints>,
    providers: RwLock<Vec<Arc<dyn DependencyProvider>>>,
    plugins: RwLock<Option<ProjectSource>>,
    cache: RwLock<Option<PathBuf>>,
    env: RwLock<Option<BuildEnv>>,
    state: Mutex<State>,
    queue: Mutex<Queue>,
    work: Condvar,
    idle: Condvar,
    generation: AtomicU64,
    forgotten: AtomicU64,
    poms: AtomicU64,
    wake: Mutex<Option<Wake>>,
}

#[derive(Clone)]
pub struct Dependencies {
    shared: Arc<Shared>,
}

impl Default for Dependencies {
    fn default() -> Self {
        let providers: Vec<Arc<dyn DependencyProvider>> = vec![
            Arc::new(crate::maven::dependencies::MavenDependencies),
            Arc::new(crate::gradle::dependencies::GradleDependencies),
        ];
        Self {
            shared: Arc::new(Shared {
                transport: RwLock::new(Arc::new(NoNetwork)),
                endpoints: RwLock::new(Endpoints::default()),
                providers: RwLock::new(providers),
                plugins: RwLock::new(None),
                cache: RwLock::new(None),
                env: RwLock::new(None),
                state: Mutex::default(),
                queue: Mutex::default(),
                work: Condvar::new(),
                idle: Condvar::new(),
                generation: AtomicU64::new(0),
                forgotten: AtomicU64::new(0),
                poms: AtomicU64::new(0),
                wake: Mutex::new(None),
            }),
        }
    }
}

impl std::fmt::Debug for Dependencies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dependencies").finish_non_exhaustive()
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Kept {
    at: u64,
    validators: Validators,
    body: Option<String>,
}

fn key_file(key: &str) -> String {
    use sha2::Digest as _;
    format!("{}.json", hex::encode(sha2::Sha256::digest(key.as_bytes())))
}

impl Dependencies {
    pub fn set_transport(&self, transport: Arc<dyn Transport>) {
        *self.shared.transport.write().unwrap_or_else(PoisonError::into_inner) = transport;
    }

    pub fn set_endpoints(&self, endpoints: Endpoints) {
        *self.shared.endpoints.write().unwrap_or_else(PoisonError::into_inner) = endpoints;
    }

    pub fn set_providers(&self, providers: Vec<Arc<dyn DependencyProvider>>) {
        *self.shared.providers.write().unwrap_or_else(PoisonError::into_inner) = providers;
    }

    pub fn set_plugins(&self, source: Option<ProjectSource>) {
        *self.shared.plugins.write().unwrap_or_else(PoisonError::into_inner) = source;
    }

    pub fn set_cache_dir(&self, dir: PathBuf) {
        *self.shared.cache.write().unwrap_or_else(PoisonError::into_inner) = Some(dir);
    }

    pub fn set_env(&self, env: BuildEnv) {
        *self.shared.env.write().unwrap_or_else(PoisonError::into_inner) = Some(env);
    }

    pub fn env(&self) -> BuildEnv {
        let env = self.shared.env.read().unwrap_or_else(PoisonError::into_inner).clone();
        env.unwrap_or_else(BuildEnv::global)
    }

    pub fn set_wake(&self, wake: Wake) {
        *locked(&self.shared.wake) = Some(wake);
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::SeqCst)
    }

    pub fn inspector(&self) -> Arc<dyn zen_project::inspect::Inspector> {
        Arc::new(inspector::DependencyInspector::new(self.clone()))
    }

    pub fn providers_for(&self, root: &Path) -> Vec<Arc<dyn DependencyProvider>> {
        let plugins = self.shared.plugins.read().unwrap_or_else(PoisonError::into_inner).clone();
        let supporting: Option<Vec<String>> = plugins.map(|source| {
            source().into_iter().filter(|plugin| plugin.support.supports(root)).map(|plugin| plugin.id).collect()
        });
        let providers = self.shared.providers.read().unwrap_or_else(PoisonError::into_inner);
        providers
            .iter()
            .filter(|provider| supporting.as_ref().is_none_or(|ids| ids.iter().any(|id| id == provider.id())))
            .cloned()
            .collect()
    }

    pub fn providers(&self) -> Vec<Arc<dyn DependencyProvider>> {
        self.shared.providers.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn changed(&self) {
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.wake();
    }

    fn wake(&self) {
        let wake = locked(&self.shared.wake).clone();
        if let Some(wake) = wake {
            wake();
        }
    }

    pub fn refresh(&self) {
        self.shared.forgotten.store(now().saturating_add(1), Ordering::SeqCst);
        *locked(&self.shared.state) = State::default();
        self.changed();
    }

    fn enqueue(&self, task: Task) {
        let mut queue = locked(&self.shared.queue);
        if !queue.known.insert(task.clone()) {
            return;
        }
        queue.tasks.push_back(task);
        if queue.workers < WORKERS {
            queue.workers += 1;
            let service = self.clone();
            std::thread::spawn(move || service.work());
        } else {
            self.shared.work.notify_one();
        }
    }

    fn is_known(&self, task: &Task) -> bool {
        locked(&self.shared.queue).known.contains(task)
    }

    fn work(&self) {
        let mut queue = locked(&self.shared.queue);
        loop {
            let Some(task) = queue.tasks.pop_front() else {
                if queue.running == 0 {
                    self.shared.idle.notify_all();
                }
                let (next, timeout) =
                    self.shared.work.wait_timeout(queue, IDLE).unwrap_or_else(PoisonError::into_inner);
                queue = next;
                if timeout.timed_out() && queue.tasks.is_empty() {
                    queue.workers -= 1;
                    return;
                }
                continue;
            };
            queue.running += 1;
            drop(queue);
            self.run(&task);
            queue = locked(&self.shared.queue);
            queue.running -= 1;
            queue.known.remove(&task);
            self.shared.generation.fetch_add(1, Ordering::SeqCst);
            drop(queue);
            self.wake();
            queue = locked(&self.shared.queue);
        }
    }

    pub fn wait_idle(&self, wait: Duration) {
        let deadline = Instant::now() + wait.min(MAX_WAIT);
        let mut queue = locked(&self.shared.queue);
        while !queue.known.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            queue = self.shared.idle.wait_timeout(queue, left).unwrap_or_else(PoisonError::into_inner).0;
        }
    }

    fn transport(&self) -> Arc<dyn Transport> {
        self.shared.transport.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn endpoints(&self) -> Endpoints {
        self.shared.endpoints.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn kept(&self, key: &str) -> Option<Kept> {
        let dir = self.shared.cache.read().unwrap_or_else(PoisonError::into_inner).clone()?;
        serde_json::from_slice(&std::fs::read(dir.join(key_file(key))).ok()?).ok()
    }

    fn keep(&self, key: &str, kept: &Kept) {
        let Some(dir) = self.shared.cache.read().unwrap_or_else(PoisonError::into_inner).clone() else { return };
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(bytes) = serde_json::to_vec(kept) {
            let _ = zen_vfs::write_atomic(&dir.join(key_file(key)), &bytes);
        }
    }

    fn fresh(&self, kept: &Kept) -> bool {
        kept.at >= self.shared.forgotten.load(Ordering::SeqCst)
            && Duration::from_secs(now().saturating_sub(kept.at)) < FRESH
    }

    fn document(&self, url: &str, authorization: Option<&str>) -> (Result<Option<String>, String>, bool) {
        let kept = self.kept(url);
        if let Some(kept) = kept.as_ref().filter(|kept| self.fresh(kept)) {
            return (Ok(kept.body.clone()), false);
        }
        let validators = kept.as_ref().map(|kept| kept.validators.clone()).unwrap_or_default();
        match self.transport().get(url, &validators, authorization) {
            Ok(Answer::Unchanged) => {
                let kept = kept.unwrap_or_default();
                self.keep(url, &Kept { at: now(), ..kept.clone() });
                (Ok(kept.body), false)
            }
            Ok(Answer::Missing) => {
                self.keep(url, &Kept { at: now(), ..Kept::default() });
                (Ok(None), false)
            }
            Ok(Answer::Changed { body, validators }) => {
                let body = String::from_utf8_lossy(&body).into_owned();
                self.keep(url, &Kept { at: now(), validators, body: Some(body.clone()) });
                (Ok(Some(body)), false)
            }
            Err(error) => match kept {
                Some(kept) => (Ok(kept.body), true),
                None => (Err(error), true),
            },
        }
    }

    fn run(&self, task: &Task) {
        match task {
            Task::Search(query) => self.run_search(query),
            Task::Metadata { repository, group, artifact } => self.run_metadata(repository, group, artifact),
            Task::Advisories { group, artifact } => self.run_advisories(group, artifact),
            Task::Pom { repositories, group, artifact, version } => {
                self.run_pom(repositories, group, artifact, version);
            }
        }
    }

    fn run_search(&self, query: &IndexQuery) {
        let endpoints = self.endpoints();
        let Some(url) = query.url(&endpoints.search) else { return };
        let (answer, failed) = self.document(&url, None);
        let value = answer.ok().flatten().and_then(|body| sources::parse_search(body.as_bytes()).ok());
        let slot = Slot { failed: failed || value.is_none(), value, at: Instant::now() };
        locked(&self.shared.state).searches.insert(query.clone(), slot);
    }

    fn run_metadata(&self, repository: &Repository, group: &str, artifact: &str) {
        let url = repository.metadata_url(group, artifact);
        let (answer, failed) = self.document(&url, repository.authorization.as_deref());
        let value = match answer {
            Ok(Some(body)) => sources::parse_metadata(body.as_bytes()).ok(),
            Ok(None) => Some(Vec::new()),
            Err(_) => None,
        };
        let key = (repository.url.clone(), group.to_owned(), artifact.to_owned());
        let slot = Slot { failed: failed || value.is_none(), value, at: Instant::now() };
        locked(&self.shared.state).lists.insert(key, slot);
    }

    fn run_advisories(&self, group: &str, artifact: &str) {
        let key = format!("osv\n{group}:{artifact}");
        let kept: Option<Vec<Advisory>> =
            self.kept(&key).filter(|kept| self.fresh(kept)).and_then(|kept| serde_json::from_str(&kept.body?).ok());
        let (value, failed) = match kept {
            Some(kept) => (Some(kept), false),
            None => {
                let endpoints = self.endpoints();
                match sources::fetch_advisories(self.transport().as_ref(), &endpoints.osv, group, artifact) {
                    Ok(found) => {
                        let body = serde_json::to_string(&found).ok();
                        self.keep(&key, &Kept { at: now(), validators: Validators::default(), body });
                        (Some(found), false)
                    }
                    Err(_) => {
                        let old = self.kept(&key).and_then(|kept| serde_json::from_str(&kept.body?).ok());
                        (old, true)
                    }
                }
            }
        };
        let slot = Slot { failed: failed || value.is_none(), value, at: Instant::now() };
        locked(&self.shared.state).advisories.insert((group.to_owned(), artifact.to_owned()), slot);
    }

    fn run_pom(&self, repositories: &[Repository], group: &str, artifact: &str, version: &str) {
        let mut found = None;
        let mut failed = false;
        for repository in repositories {
            let url = repository.pom_url(group, artifact, version);
            let (answer, failure) = self.document(&url, repository.authorization.as_deref());
            failed |= failure;
            if let Ok(Some(body)) = answer {
                found = Some(body);
                break;
            }
        }
        let key = (group.to_owned(), artifact.to_owned(), version.to_owned());
        let slot = Slot { failed: failed && found.is_none(), value: found, at: Instant::now() };
        locked(&self.shared.state).poms.insert(key, slot);
        self.shared.poms.fetch_add(1, Ordering::SeqCst);
    }

    pub fn pom(&self, repositories: &[Repository], group: &str, artifact: &str, version: &str) -> Option<String> {
        if !sources::safe_coordinates(&[group, artifact, version]) {
            return None;
        }
        let key = (group.to_owned(), artifact.to_owned(), version.to_owned());
        {
            let state = locked(&self.shared.state);
            if let Some(slot) = state.poms.get(&key).filter(|slot| !slot.failed || !slot.stale()) {
                return slot.value.clone();
            }
        }
        self.enqueue(Task::Pom {
            repositories: repositories.to_vec(),
            group: group.to_owned(),
            artifact: artifact.to_owned(),
            version: version.to_owned(),
        });
        None
    }

    pub fn offline_pom(
        &self,
        repositories: &[Repository],
        group: &str,
        artifact: &str,
        version: &str,
    ) -> Option<String> {
        let key = (group.to_owned(), artifact.to_owned(), version.to_owned());
        if let Some(slot) = locked(&self.shared.state).poms.get(&key) {
            return slot.value.clone();
        }
        repositories
            .iter()
            .find_map(|repository| self.kept(&repository.pom_url(group, artifact, version)).and_then(|kept| kept.body))
    }

    fn index(&self, query: &IndexQuery) -> (Option<Vec<(String, String)>>, bool, bool) {
        let task = Task::Search(query.clone());
        let known =
            locked(&self.shared.state).searches.get(query).map(|slot| (slot.value.clone(), slot.failed, slot.stale()));
        match known {
            Some((value, failed, stale)) => {
                if stale {
                    self.enqueue(task);
                }
                (value, false, failed)
            }
            None => {
                self.enqueue(task);
                (None, true, false)
            }
        }
    }

    fn listed(&self, group: &str, artifact: &str, repositories: &[Repository]) -> Listed {
        if !sources::safe_coordinates(&[group, artifact]) {
            return Listed::default();
        }
        let mut listed = Listed::default();
        for repository in repositories.iter().filter(|repository| repository.is_secure()) {
            let key = (repository.url.clone(), group.to_owned(), artifact.to_owned());
            let task = Task::Metadata {
                repository: repository.clone(),
                group: group.to_owned(),
                artifact: artifact.to_owned(),
            };
            let known =
                locked(&self.shared.state).lists.get(&key).map(|slot| (slot.value.clone(), slot.failed, slot.stale()));
            match known {
                Some((value, failed, stale)) => {
                    if stale {
                        self.enqueue(task.clone());
                    }
                    listed.failed |= failed;
                    let value = value.unwrap_or_default();
                    listed.public |= repository.is_public() && !value.is_empty();
                    listed.versions.extend(value);
                }
                None => {
                    self.enqueue(task.clone());
                    listed.pending = true;
                }
            }
            listed.pending |= self.is_known(&task);
        }
        listed.versions = versions::newest_first(listed.versions);
        listed
    }

    fn advisories_of(&self, group: &str, artifact: &str) -> (Vec<Advisory>, bool, bool) {
        let key = (group.to_owned(), artifact.to_owned());
        let task = Task::Advisories { group: group.to_owned(), artifact: artifact.to_owned() };
        let known =
            locked(&self.shared.state).advisories.get(&key).map(|slot| (slot.value.clone(), slot.failed, slot.stale()));
        match known {
            Some((value, failed, stale)) => {
                if stale {
                    self.enqueue(task);
                }
                (value.unwrap_or_default(), false, failed)
            }
            None => {
                self.enqueue(task);
                (Vec::new(), true, false)
            }
        }
    }

    pub fn status(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
        repositories: &[Repository],
        allowed: Allowed,
    ) -> (DependencyStatus, bool) {
        if !allowed.updates && !allowed.advisories
            || group.is_empty()
            || artifact.is_empty()
            || !sources::safe_coordinates(&[group, artifact])
        {
            return (DependencyStatus::default(), false);
        }
        let listed = self.listed(group, artifact, repositories);
        let mut status = DependencyStatus { pending: listed.pending, ..DependencyStatus::default() };
        let mut failed = listed.failed;
        if allowed.updates {
            status.latest = versions::latest_stable(&listed.versions);
            if !version.is_empty() {
                status.newer = versions::newer_than(version, &listed.versions);
            }
        }
        if allowed.advisories && listed.public && !version.is_empty() {
            let (advisories, pending, trouble) = self.advisories_of(group, artifact);
            status.pending |= pending;
            failed |= trouble;
            let affecting: Vec<&Advisory> = advisories.iter().filter(|advisory| advisory.affects(version)).collect();
            if !affecting.is_empty() {
                status.safe = sources::nearest_safe(version, &listed.versions, &affecting);
            }
            status.advisories = affecting.iter().map(|advisory| advisory.shown()).collect();
        }
        (status, failed)
    }

    pub fn versions(&self, group: &str, artifact: &str, repositories: &[Repository]) -> (Vec<String>, bool, bool) {
        let listed = self.listed(group, artifact, repositories);
        (listed.versions, listed.pending, listed.failed)
    }

    pub fn search(&self, typed: &str, repositories: &[Repository], allowed: Allowed) -> SearchAnswer {
        let mut answer = SearchAnswer { query: typed.to_owned(), ..SearchAnswer::default() };
        if !allowed.search {
            answer.disabled = true;
            return answer;
        }
        let Some(query) = IndexQuery::parse(typed) else { return answer };
        let (found, pending, failed) = self.index(&query);
        answer.pending = pending;
        answer.failed = failed;
        let mut found = found.unwrap_or_default();
        if let IndexQuery::Coordinates { group, artifact } = &query
            && !found.iter().any(|(g, a)| g == group && a == artifact)
        {
            found.insert(0, (group.clone(), artifact.clone()));
        }
        for (group, artifact) in found {
            let listed = self.listed(&group, &artifact, repositories);
            answer.pending |= listed.pending;
            answer.failed |= listed.failed;
            if !listed.pending && !listed.failed && listed.versions.is_empty() {
                continue;
            }
            answer.results.push(SearchResult {
                latest: versions::latest_stable(&listed.versions),
                versions: listed.versions,
                group,
                artifact,
            });
        }
        answer
    }

    pub fn completion_index(&self, query: &IndexQuery) -> (Vec<(String, String)>, bool) {
        let (found, pending, _) = self.index(query);
        (found.unwrap_or_default(), pending)
    }
}

#[derive(Default)]
struct Listed {
    versions: Vec<String>,
    pending: bool,
    failed: bool,
    public: bool,
}

/// One artifact a search found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Group id.
    pub group: String,
    /// Artifact id.
    pub artifact: String,
    /// The latest stable version, from `maven-metadata.xml`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// Every version the repositories list, newest first.
    pub versions: Vec<String>,
}

/// What `dependencies.repository.search` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchAnswer {
    /// What was searched for.
    pub query: String,
    /// Whether answers are still on their way: read again.
    pub pending: bool,
    /// Whether search is turned off in the settings or Maven works offline.
    pub disabled: bool,
    /// Whether a server could not be reached: search needs the network.
    pub failed: bool,
    /// What was found, best first.
    pub results: Vec<SearchResult>,
}

pub fn service(session: &mut Session) -> Dependencies {
    let source = session.state::<crate::plugin_projects::PluginProjects>().and_then(|projects| projects.source());
    let service = session.state_mut::<Dependencies>().clone();
    if source.is_some() {
        service.set_plugins(source);
    }
    service
}

pub fn offered(session: &mut Session, root: &Path) -> bool {
    service(session).providers_for(root).iter().any(|provider| provider.applies(root))
}

pub(crate) fn open_build_files(session: &Session, service: &Dependencies) -> HashMap<PathBuf, String> {
    let providers = service.providers();
    session
        .state::<Workbench>()
        .map(|workbench| {
            workbench
                .documents()
                .iter()
                .filter(|document| document.image().is_none())
                .filter(|document| {
                    let name = document.name();
                    providers.iter().any(|provider| provider.handles(&name))
                })
                .map(|document| (document.path().to_owned(), document.text().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn with_files<T>(session: &Session, service: &Dependencies, root: &Path, f: impl FnOnce(&BuildFiles<'_>) -> T) -> T {
    with_edited_files(session, service, root, &HashMap::new(), f)
}

fn with_edited_files<T>(
    session: &Session,
    service: &Dependencies,
    root: &Path,
    edited: &HashMap<PathBuf, String>,
    f: impl FnOnce(&BuildFiles<'_>) -> T,
) -> T {
    let mut open = open_build_files(session, service);
    open.extend(edited.iter().map(|(path, text)| (path.clone(), text.clone())));
    let env = session.state::<BuildEnv>().cloned().unwrap_or_default();
    let settings = session.settings();
    let setting = |key: &str| settings.get::<Value>(key);
    let on = Allowed::of(&setting, false);
    let network = on.search || on.updates || on.advisories;
    let remote = |repositories: &[Repository], group: &str, artifact: &str, version: &str| {
        if network {
            service.pom(repositories, group, artifact, version)
        } else {
            service.offline_pom(repositories, group, artifact, version)
        }
    };
    f(&BuildFiles { root, env: &env, open: &open, remote_pom: &remote, seen: RefCell::default() })
}

fn hash_of(open: &HashMap<PathBuf, String>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let sorted: BTreeMap<_, _> = open.iter().collect();
    sorted.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn project_modules(session: &mut Session) -> Result<Vec<DependencyModule>, CommandError> {
    let root = project_root(session)?;
    let service = service(session);
    let providers = service.providers_for(&root);
    Ok(read_modules(session, &service, &root, &providers))
}

fn read_modules(
    session: &Session,
    service: &Dependencies,
    root: &Path,
    providers: &[Arc<dyn DependencyProvider>],
) -> Vec<DependencyModule> {
    let mut modules = all_modules(session, service, root, providers);
    if let Some(patterns) = crate::hidden_files::for_caller(session) {
        modules.retain(|module| !crate::hidden_files::is_hidden(&patterns, Some(root), &module.file));
    }
    modules
}

fn all_modules(
    session: &Session,
    service: &Dependencies,
    root: &Path,
    providers: &[Arc<dyn DependencyProvider>],
) -> Vec<DependencyModule> {
    let open = hash_of(&open_build_files(session, service));
    let poms = service.shared.poms.load(Ordering::SeqCst);
    let known = locked(&service.shared.state).read.clone();
    if let Some(known) = known.filter(|known| {
        known.root == root
            && known.open == open
            && known.poms == poms
            && known.files.iter().all(|(path, seen)| stamp(path) == *seen)
    }) {
        return known.modules;
    }
    with_files(session, service, root, |files| {
        let modules: Vec<DependencyModule> = providers.iter().flat_map(|provider| provider.modules(files)).collect();
        let seen = files.seen.borrow().keys().map(|path| (path.clone(), stamp(path))).collect();
        locked(&service.shared.state).read =
            Some(ReadModules { root: root.to_owned(), poms, open, files: seen, modules: modules.clone() });
        modules
    })
}

fn allowed(session: &Session, files: &BuildFiles<'_>, providers: &[Arc<dyn DependencyProvider>]) -> Allowed {
    let settings = session.settings();
    let setting = |key: &str| settings.get::<Value>(key);
    Allowed::of(&setting, providers.iter().any(|provider| provider.offline(files)))
}

/// What `dependencies.project.list` answers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencyList {
    /// The project's modules with their declared dependencies.
    pub modules: Vec<DependencyModule>,
    /// Whether answers are still on their way: read again.
    pub pending: bool,
    /// Whether a server could not be reached: what is shown may be old, and checks need the network.
    pub failed: bool,
    /// Whether search and the checks are on: all off, nothing is asked from the network.
    pub network: bool,
    /// Goes up whenever answers came in.
    pub generation: u64,
}

fn list(session: &mut Session) -> Result<DependencyList, CommandError> {
    let root = project_root(session)?;
    let service = service(session);
    let generation = service.generation();
    let providers = service.providers_for(&root);
    let allowed = with_files(session, &service, &root, |files| allowed(session, files, &providers));
    let mut answer = DependencyList {
        network: allowed.search || allowed.updates || allowed.advisories,
        ..DependencyList::default()
    };
    for mut module in read_modules(session, &service, &root, &providers) {
        for dependency in module.dependencies.iter_mut().filter(|dependency| !dependency.local) {
            let (status, failed) = service.status(
                &dependency.group,
                &dependency.artifact,
                &dependency.version,
                &module.repositories,
                allowed,
            );
            answer.pending |= status.pending;
            answer.failed |= failed;
            dependency.status = status;
        }
        answer.modules.push(module);
    }
    Ok(DependencyList { generation, ..answer })
}

fn settled<T>(
    service: &Dependencies,
    wait_ms: Option<u64>,
    mut ask: impl FnMut() -> Result<T, CommandError>,
    pending: impl Fn(&T) -> bool,
) -> Result<T, CommandError> {
    let mut answer = ask()?;
    let Some(wait) = wait_ms.map(Duration::from_millis) else { return Ok(answer) };
    let deadline = Instant::now() + wait.min(MAX_WAIT);
    while pending(&answer) && Instant::now() < deadline {
        service.wait_idle(deadline.saturating_duration_since(Instant::now()));
        answer = ask()?;
    }
    Ok(answer)
}

/// The parameters of `dependencies.project.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListParams {
    /// Waits up to this long (at most a minute) for the answers from the network.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct ListDependencies;

impl Command for ListDependencies {
    type Params = ListParams;
    type Output = DependencyList;
    const ID: &'static str = "dependencies.project.list";
    const TITLE: &'static str = "Project Dependencies";
    const DESCRIPTION: &'static str = "The modules of the open project as the Dependencies tool window shows them, from \
         every build system's plug-in: their repositories, the scopes or configurations a dependency can have, and each \
         declared dependency with its version, scope, where the version is written, and what is known about it: the \
         latest stable version and newer ones (from maven-metadata.xml of each repository), and known vulnerabilities \
         with the nearest safe version (from OSV). Never blocks: answers come in the background unless waitMs is given; \
         pending says more is coming. Offline, or with the checks turned off, the dependencies are still listed.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<DependencyList, CommandError> {
        let service = service(session);
        settled(&service, params.wait_ms, || list(session), |answer| answer.pending)
    }
}

/// The parameters of `dependencies.repository.search`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchParams {
    /// A name, or coordinates as `group:artifact`.
    pub query: String,
    /// The module whose repositories are asked for versions; the first module when absent.
    #[serde(default)]
    pub module: Option<String>,
    /// Waits up to this long (at most a minute) for the answers from the network.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

fn module_repositories(
    session: &mut Session,
    module: Option<&str>,
) -> Result<(Vec<Repository>, Allowed), CommandError> {
    let root = project_root(session)?;
    let service = service(session);
    let providers = service.providers_for(&root);
    let allowed = with_files(session, &service, &root, |files| allowed(session, files, &providers));
    let modules = read_modules(session, &service, &root, &providers);
    let fallback = vec![Repository::new("central", &service.endpoints().central)];
    let repositories = match module {
        Some(id) => modules
            .into_iter()
            .find(|found| found.id == id)
            .map(|found| found.repositories)
            .ok_or_else(|| CommandError::new(t!("workbench-deps-no-module", module = id)))?,
        None => modules.into_iter().next().map_or(fallback, |found| found.repositories),
    };
    Ok((repositories, allowed))
}

pub struct SearchDependencies;

impl Command for SearchDependencies {
    type Params = SearchParams;
    type Output = SearchAnswer;
    const ID: &'static str = "dependencies.repository.search";
    const TITLE: &'static str = "Search Dependencies";
    const DESCRIPTION: &'static str = "Searches for libraries by name or by coordinates (`group:artifact`), as the search \
         field of the Dependencies tool window: the Maven Central search index finds artifacts, and each result's \
         versions and latest stable version come from maven-metadata.xml of the module's repositories, never from the \
         index. Never blocks: read again while pending is true, or give waitMs. disabled says search is turned off or \
         the build works offline; failed that a server could not be reached.";

    fn run(&self, session: &mut Session, params: SearchParams) -> Result<SearchAnswer, CommandError> {
        let service = service(session);
        let (repositories, allowed) = module_repositories(session, params.module.as_deref())?;
        settled(
            &service,
            params.wait_ms,
            || Ok(service.search(&params.query, &repositories, allowed)),
            |answer| answer.pending,
        )
    }
}

/// The parameters of `dependencies.versions.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionsParams {
    /// Group id.
    pub group: String,
    /// Artifact id.
    pub artifact: String,
    /// The module whose repositories are asked; the first module when absent.
    #[serde(default)]
    pub module: Option<String>,
    /// Waits up to this long (at most a minute) for the answers from the network.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What `dependencies.versions.get` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionList {
    /// Every version the repositories list, newest first, pre-releases too.
    pub versions: Vec<String>,
    /// The latest stable version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// Whether answers are still on their way.
    pub pending: bool,
    /// Whether a repository could not be reached.
    pub failed: bool,
}

pub struct GetVersions;

impl Command for GetVersions {
    type Params = VersionsParams;
    type Output = VersionList;
    const ID: &'static str = "dependencies.versions.get";
    const TITLE: &'static str = "Dependency Versions";
    const DESCRIPTION: &'static str = "Every version of an artifact that maven-metadata.xml of the module's repositories \
         lists, newest first, and the latest stable one. Never blocks unless waitMs is given.";

    fn run(&self, session: &mut Session, params: VersionsParams) -> Result<VersionList, CommandError> {
        let service = service(session);
        let (repositories, allowed) = module_repositories(session, params.module.as_deref())?;
        if !allowed.search && !allowed.updates && !allowed.advisories {
            return Ok(VersionList::default());
        }
        settled(
            &service,
            params.wait_ms,
            || {
                let (versions, pending, failed) = service.versions(&params.group, &params.artifact, &repositories);
                Ok(VersionList { latest: versions::latest_stable(&versions), versions, pending, failed })
            },
            |answer| answer.pending,
        )
    }
}

/// What an edit of the dependencies did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Edited {
    /// The build files changed, saved.
    pub files: Vec<PathBuf>,
    /// Whether the project was asked to load again.
    pub reloaded: bool,
    /// Why it was not: the change then waits for Load Changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_error: Option<String>,
}

pub(crate) fn write_changes(session: &mut Session, changes: &[FixWrite]) -> Result<Vec<PathBuf>, CommandError> {
    if changes.is_empty() {
        return Ok(Vec::new());
    }
    let root = project_root(session)?;
    let paths: Vec<&PathBuf> = changes.iter().map(|change| &change.path).collect();
    crate::hidden_files::guard_change(session, &root, &paths)?;
    if let Some(path) = paths.iter().find(|path| crate::guard::refuses_in(Some(&root), path)) {
        return Err(CommandError::new(crate::guard::refusal(path)));
    }
    for change in changes {
        let mut moved = false;
        crate::disk::edit_file(session, &change.path, |current| {
            // WHY: the editor keeps one line ending, so an open file's CRLF on disk is no outside change.
            moved =
                change.read.as_ref().is_some_and(|read| read.replace("\r\n", "\n") != current.replace("\r\n", "\n"));
            (!moved).then(|| change.text.clone())
        })?;
        if moved {
            return Err(CommandError::new(t!("workbench-deps-changed-outside", file = crate::display(&change.path))));
        }
    }
    Ok(paths.into_iter().cloned().collect())
}

fn provider_of(session: &mut Session, root: &Path, module: &str) -> Result<Arc<dyn DependencyProvider>, CommandError> {
    let service = service(session);
    let providers = service.providers_for(root);
    read_modules(session, &service, root, &providers)
        .into_iter()
        .find(|found| found.id == module)
        .and_then(|found| providers.iter().find(|provider| provider.id() == found.provider).cloned())
        .ok_or_else(|| CommandError::new(t!("workbench-deps-no-module", module = module)))
}

fn change(session: &mut Session, module: &str, change: DependencyChange) -> Result<Edited, CommandError> {
    change_all(session, module, &[change])
}

pub(crate) fn change_all(
    session: &mut Session,
    module: &str,
    wanted: &[DependencyChange],
) -> Result<Edited, CommandError> {
    let root = project_root(session)?;
    let service = service(session);
    let provider = provider_of(session, &root, module)?;
    let mut planned: HashMap<PathBuf, String> = HashMap::new();
    let mut reads: HashMap<PathBuf, String> = HashMap::new();
    for change in wanted {
        let changed = with_edited_files(session, &service, &root, &planned, |files| {
            let changed = provider.change(files, change);
            files.keep_reads(&mut reads, |path| planned.contains_key(path));
            changed
        })
        .map_err(CommandError::new)?;
        planned.extend(changed.into_iter().map(|file| (file.path, file.text)));
    }
    let changes: Vec<FixWrite> =
        planned.into_iter().map(|(path, text)| FixWrite { read: reads.get(&path).cloned(), path, text }).collect();
    let files = write_changes(session, &changes)?;
    let mut edited = Edited { files, ..Edited::default() };
    if crate::trust::trusted(session, &root) {
        match provider.reload(session) {
            Ok(()) => edited.reloaded = true,
            Err(error) => edited.reload_error = Some(error.to_string()),
        }
    } else {
        edited.reload_error = Some(t!("workbench-deps-reload-untrusted"));
    }
    service.changed();
    Ok(edited)
}

/// The parameters of `dependencies.module.add`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddParams {
    /// The module's id from `dependencies.project.list`.
    pub module: String,
    /// Group id.
    pub group: String,
    /// Artifact id.
    pub artifact: String,
    /// The version. Left out of the build file when a parent or a BOM manages this artifact.
    pub version: String,
    /// The Maven scope or the Gradle configuration; the module's first one when absent.
    #[serde(default)]
    pub scope: Option<String>,
}

pub struct AddDependency;

impl Command for AddDependency {
    type Params = AddParams;
    type Output = Edited;
    const ID: &'static str = "dependencies.module.add";
    const TITLE: &'static str = "Add Dependency";
    const DESCRIPTION: &'static str = "Adds a dependency to a module with a chosen scope (Maven) or configuration \
         (Gradle) and version. The build file is edited in place, keeping its formatting and comments: pom.xml gets a \
         <dependency> without a version when a parent or a BOM manages it; a Gradle project with a version catalog \
         gets the library there and libs.<alias> in the script; otherwise a line in the script's own style. The \
         project then loads again.";

    fn run(&self, session: &mut Session, params: AddParams) -> Result<Edited, CommandError> {
        let module = params.module.clone();
        let scope = params.scope.unwrap_or_default();
        change(
            session,
            &module,
            DependencyChange::Add {
                module: params.module,
                group: params.group,
                artifact: params.artifact,
                version: params.version,
                scope,
            },
        )
    }
}

/// The parameters of `dependencies.version.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetVersionParams {
    /// The module's id from `dependencies.project.list`.
    pub module: String,
    /// Group id.
    pub group: String,
    /// Artifact id.
    pub artifact: String,
    /// The new version.
    pub version: String,
}

pub struct SetVersion;

impl Command for SetVersion {
    type Params = SetVersionParams;
    type Output = Edited;
    const ID: &'static str = "dependencies.version.set";
    const TITLE: &'static str = "Upgrade Dependency";
    const DESCRIPTION: &'static str = "Changes a declared dependency's version where it is written: on the dependency \
         itself, in a property, in the version catalog, or in the project's own parent or BOM. Refused when a parent \
         or a BOM from a repository manages the version. The project then loads again.";

    fn run(&self, session: &mut Session, params: SetVersionParams) -> Result<Edited, CommandError> {
        let module = params.module.clone();
        change(
            session,
            &module,
            DependencyChange::SetVersion {
                module: params.module,
                group: params.group,
                artifact: params.artifact,
                version: params.version,
            },
        )
    }
}

/// The parameters of `dependencies.module.remove`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveParams {
    /// The module's id from `dependencies.project.list`.
    pub module: String,
    /// Group id.
    pub group: String,
    /// Artifact id.
    pub artifact: String,
    /// The scope or configuration, when the module declares the artifact more than once.
    #[serde(default)]
    pub scope: Option<String>,
}

pub struct RemoveDependency;

impl Command for RemoveDependency {
    type Params = RemoveParams;
    type Output = Edited;
    const ID: &'static str = "dependencies.module.remove";
    const TITLE: &'static str = "Remove Dependency";
    const DESCRIPTION: &'static str = "Removes a declared dependency from a module's build file, keeping the rest as it \
         is written; a catalog entry other modules use stays. The project then loads again.";

    fn run(&self, session: &mut Session, params: RemoveParams) -> Result<Edited, CommandError> {
        let module = params.module.clone();
        change(
            session,
            &module,
            DependencyChange::Remove {
                module: params.module,
                group: params.group,
                artifact: params.artifact,
                scope: params.scope,
            },
        )
    }
}

pub struct RefreshDependencies;

impl Command for RefreshDependencies {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "dependencies.project.refresh";
    const TITLE: &'static str = "Refresh Dependency Checks";
    const DESCRIPTION: &'static str = "Forgets what repositories and advisories answered, so that search, versions and \
         vulnerabilities are asked again. Their HTTP validators stay, so what did not change is not downloaded again.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        service(session).refresh();
        Ok(())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListDependencies)?;
    registry.register(SearchDependencies)?;
    registry.register(GetVersions)?;
    registry.register(AddDependency)?;
    registry.register(SetVersion)?;
    registry.register(RemoveDependency)?;
    registry.register(RefreshDependencies)
}

#[cfg(test)]
mod tests;
