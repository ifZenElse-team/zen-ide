pub mod assist;
pub mod env;
pub mod local;
pub mod proxy;
pub mod releases;
pub mod update;
pub mod vulndb;

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_project::inspect::{
    DeclaredInspection, DeclaredSetting, Finding, FindingRow, FindingSeverity, FixJob, FixKind, InspectContext,
    InspectionFix, Inspector, Wake,
};

use crate::compare_versions;
use crate::gomod::{GoMod, parse_go_mod, parse_go_work};
use env::GoEnv;
use local::LocalReport;
use proxy::{Fetch, Versions};
use vulndb::{Entry, Index, Severity};

pub const CHECK_UPDATES: &str = "go.dependencies.checkUpdates";
pub const CHECK_VULNERABILITIES: &str = "go.dependencies.checkVulnerabilities";

pub const FRESH: Duration = Duration::from_secs(60 * 60);
const RETRY: Duration = Duration::from_secs(5 * 60);
const WORKERS: usize = 4;
const IDLE: Duration = Duration::from_secs(20);
const MAJORS: usize = 5;

const BATCH: &str = "go.dependencies.update";

#[derive(Clone, Debug)]
pub struct Services {
    pub vulndb: String,
    pub osv: String,
    pub releases: String,
}

impl Default for Services {
    fn default() -> Self {
        let vulndb = std::env::var("GOVULNDB")
            .ok()
            .filter(|url| url.starts_with("http"))
            .unwrap_or_else(|| vulndb::DEFAULT_DATABASE.to_owned());
        Services {
            vulndb: vulndb.trim_end_matches('/').to_owned(),
            osv: vulndb::DEFAULT_OSV.to_owned(),
            releases: releases::DEFAULT_RELEASES.to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct GoModInspector {
    shared: Arc<Shared>,
}

struct Shared {
    fetch: Arc<dyn Fetch>,
    services: Services,
    state: Mutex<State>,
    queue: Mutex<Queue>,
    work: Condvar,
    generation: AtomicU64,
    wake: Mutex<Option<Wake>>,
}

#[derive(Default)]
struct Queue {
    tasks: VecDeque<Task>,
    workers: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Task {
    Env { tool: PathBuf, env: Vec<(String, String)>, dir: PathBuf },
    Versions { proxy: String, module: String },
    Index,
    Entry(String),
    Severity(String),
    ModFile { proxy: String, module: String, version: String },
    Major { proxy: String, module: String },
    Releases { all: bool },
    Local { tool: PathBuf, env: Vec<(String, String)>, dir: PathBuf, stamp: u64, network: bool, tidy: bool },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Slot<T> {
    value: Option<T>,
    at: u64,
    failed: bool,
}

impl<T> Slot<T> {
    fn age(&self) -> Duration {
        Duration::from_secs(now().saturating_sub(self.at))
    }

    fn stale(&self) -> bool {
        self.age() >= if self.failed { RETRY } else { FRESH }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retracted: Vec<(String, String, Option<String>)>,
}

impl ModInfo {
    pub fn parse(text: &[u8]) -> ModInfo {
        let parsed = parse_go_mod(Path::new("go.mod"), &String::from_utf8_lossy(text));
        ModInfo {
            deprecated: parsed.module.and_then(|module| module.deprecated).filter(|text| !text.trim().is_empty()),
            retracted: parsed
                .retracts
                .into_iter()
                .map(|retract| (retract.low, retract.high, retract.rationale))
                .collect(),
        }
    }

    pub fn retraction(&self, version: &str) -> Option<String> {
        self.retracted
            .iter()
            .find(|(low, high, _)| compare_versions(version, low).is_ge() && compare_versions(version, high).is_le())
            .map(|(_, _, why)| why.clone().unwrap_or_default())
    }
}

#[derive(Default)]
struct State {
    cache: Option<PathBuf>,
    loaded: bool,
    envs: HashMap<(PathBuf, Vec<(String, String)>), GoEnv>,
    versions: HashMap<String, Slot<Option<Versions>>>,
    index: Option<Slot<Index>>,
    entries: HashMap<String, Slot<Entry>>,
    severities: HashMap<String, Slot<Option<Severity>>>,
    modfiles: HashMap<String, Slot<Option<ModInfo>>>,
    majors: HashMap<String, Slot<Option<String>>>,
    go_releases: Option<Slot<Option<Vec<String>>>>,
    all_go_releases: Option<Slot<Option<Vec<String>>>>,
    locals: HashMap<PathBuf, (u64, LocalReport)>,
    queued: HashSet<Task>,
}

#[derive(Default, Serialize, Deserialize)]
struct Disk {
    #[serde(default)]
    versions: HashMap<String, Slot<Option<Versions>>>,
    #[serde(default)]
    severities: HashMap<String, Slot<Option<Severity>>>,
    #[serde(default)]
    index_at: Option<u64>,
    #[serde(default)]
    modfiles: HashMap<String, Slot<Option<ModInfo>>>,
    #[serde(default)]
    majors: HashMap<String, Slot<Option<String>>>,
    #[serde(default)]
    go_releases: Option<Slot<Option<Vec<String>>>>,
    #[serde(default)]
    all_go_releases: Option<Slot<Option<Vec<String>>>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Default for GoModInspector {
    fn default() -> Self {
        Self::new(Arc::new(proxy::Http), Services::default())
    }
}

impl GoModInspector {
    pub fn new(fetch: Arc<dyn Fetch>, services: Services) -> Self {
        GoModInspector {
            shared: Arc::new(Shared {
                fetch,
                services,
                state: Mutex::new(State::default()),
                queue: Mutex::new(Queue::default()),
                work: Condvar::new(),
                generation: AtomicU64::new(0),
                wake: Mutex::new(None),
            }),
        }
    }

    pub fn busy(&self) -> bool {
        !lock(&self.shared.state).queued.is_empty()
    }
}

impl Shared {
    fn queue(self: &Arc<Self>, task: Task) {
        {
            let mut state = lock(&self.state);
            if !state.queued.insert(task.clone()) {
                return;
            }
        }
        let mut queue = lock(&self.queue);
        queue.tasks.push_back(task);
        if queue.workers < WORKERS && queue.workers < queue.tasks.len() {
            queue.workers += 1;
            let shared = Arc::clone(self);
            if std::thread::Builder::new().name("go-deps".to_owned()).spawn(move || shared.work()).is_err() {
                queue.workers -= 1;
            }
        }
        self.work.notify_one();
    }

    fn work(self: Arc<Self>) {
        loop {
            let task = {
                let mut queue = lock(&self.queue);
                loop {
                    if let Some(task) = queue.tasks.pop_front() {
                        break Some(task);
                    }
                    let (next, timeout) = self.work.wait_timeout(queue, IDLE).unwrap_or_else(PoisonError::into_inner);
                    queue = next;
                    if timeout.timed_out() && queue.tasks.is_empty() {
                        queue.workers -= 1;
                        break None;
                    }
                }
            };
            let Some(task) = task else { return };
            self.run(&task);
            lock(&self.state).queued.remove(&task);
            self.generation.fetch_add(1, Ordering::SeqCst);
            if let Some(wake) = lock(&self.wake).clone() {
                wake();
            }
        }
    }

    fn run(&self, task: &Task) {
        let fetch = self.fetch.as_ref();
        let steps = |proxy: &str| GoEnv { proxy: proxy.to_owned(), ..GoEnv::default() }.proxies();
        match task {
            Task::Env { tool, env, dir } => {
                let read = GoEnv::read(tool, env, dir);
                lock(&self.state).envs.insert((tool.clone(), env.clone()), read);
            }
            Task::Versions { proxy, module } => {
                let answer = proxy::versions(fetch, &steps(proxy), module);
                let mut state = lock(&self.state);
                let key = versions_key(proxy, module);
                let slot = match answer {
                    Ok(value) => Slot { value: Some(value), at: now(), failed: false },
                    Err(_) => {
                        let old = state.versions.remove(&key).and_then(|slot| slot.value);
                        Slot { value: old, at: now(), failed: true }
                    }
                };
                state.versions.insert(key, slot);
                state.save();
            }
            Task::Index => {
                let known = lock(&self.state).index.as_ref().and_then(|slot| slot.value.clone());
                let answer = vulndb::fetch_index(fetch, &self.services.vulndb, known.as_ref());
                let mut state = lock(&self.state);
                let slot = match answer {
                    Ok(index) => {
                        if known.as_ref() != Some(&index) {
                            state.store_index(&index);
                        }
                        Slot { value: Some(index), at: now(), failed: false }
                    }
                    Err(_) => Slot { value: known, at: now(), failed: true },
                };
                state.index = Some(slot);
                state.save();
            }
            Task::Entry(id) => {
                let answer = fetch.get(&format!("{}/ID/{id}.json", self.services.vulndb));
                let mut state = lock(&self.state);
                let slot = match answer.map(|json| json.map(|json| (Entry::parse(&json), json))) {
                    Ok(Some((Ok(entry), json))) => {
                        if let Some(dir) = state.cache.as_ref().map(|cache| cache.join("vulndb").join("ID")) {
                            let _ = std::fs::create_dir_all(&dir);
                            let _ = zen_vfs_write(&dir.join(format!("{id}.json")), &json);
                        }
                        Slot { value: Some(entry), at: now(), failed: false }
                    }
                    _ => Slot { value: state.entries.remove(id).and_then(|slot| slot.value), at: now(), failed: true },
                };
                state.entries.insert(id.clone(), slot);
            }
            Task::Severity(advisory) => {
                let answer = vulndb::fetch_severity(fetch, &self.services.osv, advisory);
                let mut state = lock(&self.state);
                let slot = match answer {
                    Ok(value) => Slot { value: Some(value), at: now(), failed: false },
                    Err(_) => Slot { value: None, at: now(), failed: true },
                };
                state.severities.insert(advisory.clone(), slot);
                state.save();
            }
            Task::ModFile { proxy, module, version } => {
                let answer = proxy::go_mod(fetch, &steps(proxy), module, version);
                let key = format!("{} {version}", versions_key(proxy, module));
                let mut state = lock(&self.state);
                let slot = match answer {
                    Ok(text) => Slot { value: Some(text.map(|text| ModInfo::parse(&text))), at: now(), failed: false },
                    Err(_) => {
                        Slot { value: state.modfiles.remove(&key).and_then(|slot| slot.value), at: now(), failed: true }
                    }
                };
                state.modfiles.insert(key, slot);
                state.save();
            }
            Task::Major { proxy, module } => {
                let answer = proxy::versions(fetch, &steps(proxy), module);
                let key = versions_key(proxy, module);
                let mut state = lock(&self.state);
                let slot = match answer {
                    Ok(versions) => {
                        Slot { value: Some(versions.and_then(|versions| versions.latest())), at: now(), failed: false }
                    }
                    Err(_) => {
                        Slot { value: state.majors.remove(&key).and_then(|slot| slot.value), at: now(), failed: true }
                    }
                };
                state.majors.insert(key, slot);
                state.save();
            }
            Task::Releases { all } => {
                let url =
                    if *all { releases::all_releases(&self.services.releases) } else { self.services.releases.clone() };
                let answer = fetch.get(&url);
                let mut state = lock(&self.state);
                let kept = if *all { &mut state.all_go_releases } else { &mut state.go_releases };
                let slot = match answer {
                    Ok(Some(json)) => {
                        let stable = releases::stable(&json);
                        Slot { value: Some((!stable.is_empty()).then_some(stable)), at: now(), failed: false }
                    }
                    _ => Slot { value: kept.take().and_then(|slot| slot.value), at: now(), failed: true },
                };
                *kept = Some(slot);
                state.save();
            }
            Task::Local { tool, env, dir, stamp, network, tidy } => {
                let report = local::run(tool, env, dir, *network, *tidy);
                lock(&self.state).locals.insert(dir.clone(), (*stamp, report));
            }
        }
    }
}

fn zen_vfs_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("part");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}

fn versions_key(proxy: &str, module: &str) -> String {
    format!("{proxy} {module}")
}

impl State {
    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(cache) = self.cache.clone() else { return };
        let Some(disk) =
            std::fs::read(cache.join("checks.json")).ok().and_then(|bytes| serde_json::from_slice::<Disk>(&bytes).ok())
        else {
            return;
        };
        self.versions = disk.versions;
        self.severities = disk.severities;
        self.modfiles = disk.modfiles;
        self.majors = disk.majors;
        self.go_releases = disk.go_releases;
        self.all_go_releases = disk.all_go_releases;
        if let Some(at) = disk.index_at
            && let Ok(bytes) = std::fs::read(cache.join("vulndb").join("index.json"))
            && let Ok(index) = serde_json::from_slice::<Index>(&bytes)
        {
            self.index = Some(Slot { value: Some(index), at, failed: false });
        }
    }

    fn save(&self) {
        let Some(cache) = &self.cache else { return };
        let disk = Disk {
            versions: self.versions.clone(),
            severities: self.severities.clone(),
            index_at: self.index.as_ref().filter(|slot| slot.value.is_some()).map(|slot| slot.at),
            modfiles: self.modfiles.clone(),
            majors: self.majors.clone(),
            go_releases: self.go_releases.clone(),
            all_go_releases: self.all_go_releases.clone(),
        };
        let _ = std::fs::create_dir_all(cache);
        let _ = zen_vfs_write(&cache.join("checks.json"), &serde_json::to_vec(&disk).unwrap_or_default());
    }

    fn store_index(&self, index: &Index) {
        let Some(dir) = self.cache.as_ref().map(|cache| cache.join("vulndb")) else { return };
        let _ = std::fs::create_dir_all(&dir);
        let _ = zen_vfs_write(&dir.join("index.json"), &serde_json::to_vec(index).unwrap_or_default());
    }

    fn entry(&mut self, id: &str, modified: &str) -> Option<&Slot<Entry>> {
        if !self.entries.contains_key(id)
            && let Some(cache) = &self.cache
            && let Ok(bytes) = std::fs::read(cache.join("vulndb").join("ID").join(format!("{id}.json")))
            && let Ok(entry) = Entry::parse(&bytes)
        {
            self.entries.insert(id.to_owned(), Slot { value: Some(entry), at: 0, failed: false });
        }
        let slot = self.entries.get(id)?;
        let outdated = slot.value.as_ref().is_some_and(|entry| entry.modified.as_str() < modified);
        if slot.stale() && (outdated || slot.value.is_none()) {
            return None;
        }
        self.entries.get(id)
    }

    fn known<T: Clone>(
        map: &HashMap<String, Slot<Option<T>>>,
        key: &str,
        ask: bool,
        task: impl FnOnce() -> Task,
        tasks: &mut Vec<Task>,
    ) -> Option<Option<T>> {
        match map.get(key) {
            Some(slot) => {
                if ask && slot.stale() {
                    tasks.push(task());
                }
                slot.value.clone()
            }
            None => {
                if ask {
                    tasks.push(task());
                }
                None
            }
        }
    }
}

fn place(text: &str, line: u32, module: &str, version: &str) -> Option<(usize, usize, usize, usize, usize)> {
    let start = line_start(text, line);
    let line_text = text[start..].split('\n').next()?;
    let path_at = line_text.find(module)?;
    let version_at = path_at + module.len() + line_text[path_at + module.len()..].find(version)?;
    let end = line_text.trim_end().len();
    Some((
        start + path_at,
        start + path_at + module.len(),
        start + version_at,
        start + version_at + version.len(),
        start + end,
    ))
}

fn line_start(text: &str, line: u32) -> usize {
    text.split_inclusive('\n').take(line.saturating_sub(1) as usize).map(str::len).sum::<usize>()
}

fn token_on_line(text: &str, line: u32, token: &str) -> Option<(usize, usize)> {
    let start = line_start(text, line);
    let line_text = text.get(start..)?.split('\n').next()?;
    match line_text.find(token) {
        Some(at) => Some((start + at, start + at + token.len())),
        None => {
            let code = line_text.split("//").next().unwrap_or_default().trim_end();
            let first = code.len() - code.trim_start().len();
            (code.len() > first).then_some((start + first, start + code.len()))
        }
    }
}

fn next_major(module: &str) -> Option<String> {
    if module.starts_with("gopkg.in/") {
        return None;
    }
    if let Some((base, last)) = module.rsplit_once('/')
        && let Some(number) = last.strip_prefix('v').and_then(|number| number.parse::<u32>().ok())
        && number >= 2
    {
        return Some(format!("{base}/v{}", number + 1));
    }
    Some(format!("{module}/v2"))
}

fn disk_stamp(dir: &Path) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for name in ["go.mod", "go.sum", "go.work"] {
        std::fs::read(dir.join(name)).ok().hash(&mut hasher);
    }
    hasher.finish()
}

struct Checks<'a> {
    updates: bool,
    vulnerabilities: bool,
    go_env: &'a GoEnv,
    proxied: bool,
}

impl Inspector for GoModInspector {
    fn id(&self) -> &str {
        "go.mod"
    }

    fn handles(&self, file_name: &str) -> bool {
        file_name == "go.mod" || file_name == "go.work"
    }

    fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::SeqCst)
    }

    fn runs_programs(&self) -> bool {
        true
    }

    fn inspections(&self) -> Vec<DeclaredInspection> {
        declared_inspections()
    }

    fn unused_diagnostic(&self, source: Option<&str>, message: &str) -> bool {
        unused_by_gopls(source, message)
    }

    fn inputs(&self, path: &Path) -> Vec<PathBuf> {
        let dir = path.parent().unwrap_or(Path::new(""));
        ["go.mod", "go.sum", "go.work", "go.work.sum"].iter().map(|name| dir.join(name)).collect()
    }

    fn set_wake(&self, wake: Wake) {
        *lock(&self.shared.wake) = Some(wake);
    }

    fn set_cache_dir(&self, dir: PathBuf) {
        let mut state = lock(&self.shared.state);
        state.cache = Some(dir);
        state.loaded = false;
    }

    fn refresh(&self) {
        let mut state = lock(&self.shared.state);
        state.envs.clear();
        state.locals.clear();
        state.versions.values_mut().for_each(|slot| slot.at = 0);
        state.modfiles.values_mut().for_each(|slot| slot.at = 0);
        state.majors.values_mut().for_each(|slot| slot.at = 0);
        if let Some(index) = &mut state.index {
            index.at = 0;
        }
        let kept = &mut *state;
        for releases in [&mut kept.go_releases, &mut kept.all_go_releases].into_iter().flatten() {
            releases.at = 0;
        }
        state.severities.values_mut().for_each(|slot| slot.at = 0);
        drop(state);
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn settings(&self) -> Vec<DeclaredSetting> {
        let group = t!("go-deps-group");
        vec![
            DeclaredSetting {
                key: CHECK_UPDATES.to_owned(),
                page: "go".to_owned(),
                group: group.clone(),
                title: t!("go-deps-check-updates"),
                description: t!("go-deps-check-updates-description"),
                default: true,
                network: vec!["GOPROXY (proxy.golang.org)".to_owned(), "go.dev".to_owned()],
            },
            DeclaredSetting {
                key: CHECK_VULNERABILITIES.to_owned(),
                page: "go".to_owned(),
                group,
                title: t!("go-deps-check-vulnerabilities"),
                description: t!("go-deps-check-vulnerabilities-description"),
                default: true,
                network: vec!["vuln.go.dev".to_owned(), "api.osv.dev".to_owned()],
            },
        ]
    }

    fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
        let enabled = |key: &str| (context.setting)(key).and_then(|value| value.as_bool()).unwrap_or(true);
        let (updates, vulnerabilities) = (enabled(CHECK_UPDATES), enabled(CHECK_VULNERABILITIES));
        let dir = context.path.parent().map(Path::to_owned).unwrap_or_default();
        let work = context.path.file_name().is_some_and(|name| name == "go.work");
        let mut tasks = Vec::new();
        let mut findings = Vec::new();
        {
            let mut state = lock(&self.shared.state);
            state.load();
            let go_env = if context.tool.as_os_str().is_empty() {
                Some(GoEnv::from_vars(|key| std::env::var(key).ok()))
            } else {
                state.envs.get(&(context.tool.to_owned(), context.env.to_vec())).cloned()
            };
            let Some(go_env) = go_env else {
                drop(state);
                self.shared.queue(Task::Env { tool: context.tool.to_owned(), env: context.env.to_vec(), dir });
                return Vec::new();
            };
            let proxied = go_env.proxies().iter().any(|step| matches!(step.proxy, env::Proxy::Url(_)));
            let checks = Checks { updates, vulnerabilities, go_env: &go_env, proxied };
            if work {
                let parsed = parse_go_work(context.path, context.text);
                go_versions(
                    &mut state,
                    &checks,
                    context.text,
                    (parsed.go.as_deref(), parsed.go_line),
                    (parsed.toolchain.as_deref(), parsed.toolchain_line),
                    "work",
                    &mut tasks,
                    &mut findings,
                );
                for used in &parsed.uses {
                    let folder = dir.join(&used.path);
                    let problem = if !folder.is_dir() {
                        t!("go-work-use-missing", path = used.path.as_str())
                    } else if !folder.join("go.mod").is_file() {
                        t!("go-work-use-no-module", path = used.path.as_str())
                    } else {
                        continue;
                    };
                    if let Some((start, end)) = token_on_line(context.text, used.line, &used.path) {
                        findings.push(finding(start, end, FindingSeverity::Error, problem, "useDirectory"));
                    }
                }
            } else {
                let parsed = parse_go_mod(context.path, context.text);
                go_versions(
                    &mut state,
                    &checks,
                    context.text,
                    (parsed.go.as_deref(), parsed.go_line),
                    (parsed.toolchain.as_deref(), parsed.toolchain_line),
                    "mod",
                    &mut tasks,
                    &mut findings,
                );
                self.requirements(&mut state, &checks, context, &parsed, &mut tasks, &mut findings);
                local_findings(&mut state, &checks, context, &parsed, &dir, &mut tasks, &mut findings);
            }
        }
        for task in tasks {
            self.shared.queue(task);
        }
        findings
    }

    fn apply(&self, context: &InspectContext<'_>, fixes: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        let mut targets: Vec<(String, String)> = Vec::new();
        let mut edits: Vec<Vec<String>> = Vec::new();
        let mut tidy = false;
        for fix in fixes {
            tidy |= fix.args["tidy"] == true;
            if let Some(run) = fix.args["run"].as_array() {
                let line: Vec<String> = run.iter().filter_map(Value::as_str).map(str::to_owned).collect();
                if !line.is_empty() && !edits.contains(&line) {
                    edits.push(line);
                }
                continue;
            }
            match (fix.args["module"].as_str(), fix.args["version"].as_str()) {
                (Some(module), Some(version)) => match targets.iter_mut().find(|(kept, _)| kept == module) {
                    Some((_, kept)) if compare_versions(version, kept).is_gt() => *kept = version.to_owned(),
                    Some(_) => {}
                    None => targets.push((module.to_owned(), version.to_owned())),
                },
                _ if fix.args["tidy"] == true => {}
                _ => return Err(t!("go-deps-fix-invalid", id = fix.id.as_str())),
            }
        }
        let mut lines = edits;
        lines.extend(update::command_lines(&targets, tidy));
        let key = (context.tool.to_owned(), context.env.to_vec());
        let flags = lock(&self.shared.state).envs.get(&key).map(|go_env| go_env.flags.clone()).unwrap_or_default();
        let wake = lock(&self.shared.wake).clone();
        let job = update::start(context.tool, context.env, &flags, context.path, lines, wake)?;
        Ok(Box::new(job))
    }
}

impl GoModInspector {
    fn requirements(
        &self,
        state: &mut State,
        checks: &Checks<'_>,
        context: &InspectContext<'_>,
        parsed: &GoMod,
        tasks: &mut Vec<Task>,
        findings: &mut Vec<Finding>,
    ) {
        let Checks { updates, vulnerabilities, go_env, proxied } = *checks;
        if !updates && !vulnerabilities {
            return;
        }
        if vulnerabilities && state.index.as_ref().is_none_or(Slot::stale) {
            tasks.push(Task::Index);
        }
        let proxy = &go_env.proxy;
        for requirement in &parsed.requires {
            let (module, current) = (&requirement.path, &requirement.version);
            if go_env.is_private(module) || parsed.replaces.iter().any(|replace| replace.matches(module, current)) {
                continue;
            }
            let Some((path_start, path_end, version_start, version_end, line_end)) =
                place(context.text, requirement.line, module, current)
            else {
                continue;
            };
            let key = versions_key(proxy, module);
            let versions = State::known(
                &state.versions,
                &key,
                proxied,
                || Task::Versions { proxy: proxy.clone(), module: module.clone() },
                tasks,
            )
            .flatten();
            let info = match versions.as_ref().and_then(Versions::latest).filter(|_| updates) {
                Some(latest) => {
                    let key = format!("{key} {latest}");
                    let task =
                        || Task::ModFile { proxy: proxy.clone(), module: module.clone(), version: latest.clone() };
                    State::known(&state.modfiles, &key, proxied, task, tasks).flatten()
                }
                None => None,
            };
            let excluded = |version: &str| {
                parsed.excludes.iter().any(|exclude| &exclude.path == module && exclude.version == version)
            };
            let retraction = |version: &str| info.as_ref().and_then(|info| info.retraction(version));
            let skip = |version: &str| excluded(version) || retraction(version).is_some();
            let newer = versions.as_ref().and_then(|versions| versions.newer_than_skipping(current, &skip));
            if updates {
                if let Some(notice) = info.as_ref().and_then(|info| info.deprecated.clone()) {
                    findings.push(deprecated_finding(module, &notice, path_start, path_end));
                }
                let span = (version_start, version_end);
                if let Some(why) = retraction(current) {
                    findings.push(retracted_finding(module, current, &why, newer.as_deref(), span));
                } else if excluded(current) {
                    findings.push(excluded_finding(module, current, newer.as_deref(), span));
                } else if let Some(newer) = &newer {
                    findings.push(newer_finding(module, current, newer, version_start, version_end));
                }
                if !requirement.indirect
                    && let Some((path, version)) = self.newest_major(state, proxy, module, proxied, tasks)
                {
                    findings.push(major_finding(&path, &version, path_start, path_end));
                }
            }
            if !vulnerabilities {
                continue;
            }
            let Some(index) = state.index.as_ref().and_then(|slot| slot.value.clone()) else { continue };
            let mut affecting: Vec<Entry> = Vec::new();
            let mut complete = true;
            for vuln in index.candidates(module, current) {
                match state.entry(&vuln.id, &vuln.modified).map(|slot| slot.value.as_ref()) {
                    Some(Some(entry)) => {
                        if entry.affects(module, current) {
                            affecting.push(entry.clone());
                        }
                    }
                    Some(None) => complete = false,
                    None => {
                        complete = false;
                        tasks.push(Task::Entry(vuln.id.clone()));
                    }
                }
            }
            if affecting.is_empty() || !complete {
                continue;
            }
            let mut severities = Vec::new();
            for entry in &affecting {
                let severity = entry.advisory().and_then(|advisory| match state.severities.get(advisory) {
                    Some(slot) => {
                        if slot.stale() && slot.failed {
                            tasks.push(Task::Severity(advisory.to_owned()));
                        }
                        slot.value.clone().flatten()
                    }
                    None => {
                        tasks.push(Task::Severity(advisory.to_owned()));
                        None
                    }
                });
                severities.push(severity);
            }
            let entries: Vec<&Entry> = affecting.iter().collect();
            let fixed = vulndb::unaffected(module, current, &entries, newer.as_deref()).map(|fixed| {
                versions
                    .as_ref()
                    .and_then(|versions| versions.release_at_or_after_skipping(&fixed, &skip))
                    .unwrap_or(fixed)
            });
            findings.push(vulnerable_finding(
                module,
                current,
                &affecting,
                &severities,
                fixed.as_deref(),
                newer.is_some(),
                (path_start, line_end),
            ));
        }
    }

    fn newest_major(
        &self,
        state: &State,
        proxy: &str,
        module: &str,
        proxied: bool,
        tasks: &mut Vec<Task>,
    ) -> Option<(String, String)> {
        let mut best = None;
        let mut candidate = next_major(module)?;
        for _ in 0..MAJORS {
            let key = versions_key(proxy, &candidate);
            let task = || Task::Major { proxy: proxy.to_owned(), module: candidate.clone() };
            match State::known(&state.majors, &key, proxied, task, tasks) {
                Some(Some(version)) => {
                    best = Some((candidate.clone(), version));
                    match next_major(&candidate) {
                        Some(next) => candidate = next,
                        None => break,
                    }
                }
                _ => break,
            }
        }
        best
    }
}

#[allow(clippy::too_many_arguments)]
fn go_versions(
    state: &mut State,
    checks: &Checks<'_>,
    text: &str,
    (go, go_line): (Option<&str>, Option<u32>),
    (toolchain, toolchain_line): (Option<&str>, Option<u32>),
    file: &str,
    tasks: &mut Vec<Task>,
    findings: &mut Vec<Finding>,
) {
    let sdk = checks.go_env.version.trim_start_matches("go");
    go_sdk(sdk, text, (go, go_line), findings);
    let known = |slot: &Option<Slot<Option<Vec<String>>>>, all: bool, tasks: &mut Vec<Task>| match slot {
        Some(slot) => {
            if slot.stale() {
                tasks.push(Task::Releases { all });
            }
            slot.value.clone().flatten().unwrap_or_default()
        }
        None => {
            tasks.push(Task::Releases { all });
            Vec::new()
        }
    };
    if !checks.updates {
        return;
    }
    let supported = known(&state.go_releases, false, tasks);
    let Some(latest) = releases::latest(&supported).map(str::to_owned) else { return };
    let oldest = supported.iter().map(|release| releases::line(release)).min().unwrap_or_default();
    let mut all: Option<Vec<String>> = None;
    let mut patch_of = |version: &str, tasks: &mut Vec<Task>| -> Option<String> {
        if let Some(patch) = releases::newest_patch(&supported, version) {
            return Some(patch.to_owned());
        }
        if releases::line(version) >= oldest {
            return None;
        }
        let all = all.get_or_insert_with(|| known(&state.all_go_releases, true, tasks));
        releases::newest_patch(all, version).map(str::to_owned)
    };
    let edit = |flag: String| json!({ "run": [file, "edit", flag] });
    if let (Some(go), Some(line)) = (go, go_line)
        && let Some((start, end)) = token_on_line(text, line, go)
    {
        let (to_latest, to_patch) = targets(go, &latest, patch_of(go, tasks));
        if to_latest || to_patch.is_some() {
            let mut newer = finding(
                start,
                end,
                FindingSeverity::WeakWarning,
                t!("go-version-newer", version = latest.as_str()),
                "newerGoVersion",
            );
            if to_latest {
                newer.fixes.push(directive_fix(
                    format!("go.goDirective:{latest}"),
                    t!("go-version-update-directive", version = latest.as_str()),
                    edit(format!("-go={latest}")),
                ));
            }
            if let Some(patch) = to_patch {
                newer.fixes.push(directive_fix(
                    format!("go.goDirectivePatch:{patch}"),
                    t!("go-version-update-patch", version = patch.as_str()),
                    edit(format!("-go={patch}")),
                ));
            }
            findings.push(newer);
        }
    }
    if let (Some(toolchain), Some(line)) = (toolchain, toolchain_line)
        && toolchain != "default"
        && let Some((start, end)) = token_on_line(text, line, toolchain)
    {
        let (to_latest, to_patch) = targets(toolchain, &latest, patch_of(toolchain, tasks));
        if to_latest || to_patch.is_some() {
            let mut newer = finding(
                start,
                end,
                FindingSeverity::WeakWarning,
                t!("go-version-toolchain-newer", version = format!("go{latest}")),
                "newerToolchain",
            );
            if to_latest {
                newer.fixes.push(directive_fix(
                    format!("go.toolchain:go{latest}"),
                    t!("go-version-update-toolchain", version = format!("go{latest}")),
                    edit(format!("-toolchain=go{latest}")),
                ));
            }
            if let Some(patch) = to_patch {
                newer.fixes.push(directive_fix(
                    format!("go.toolchainPatch:go{patch}"),
                    t!("go-version-update-patch", version = format!("go{patch}")),
                    edit(format!("-toolchain=go{patch}")),
                ));
            }
            findings.push(newer);
        }
    }
}

fn targets(version: &str, latest: &str, patch: Option<String>) -> (bool, Option<String>) {
    let to_latest = releases::compare(version, latest).is_lt() && releases::line(version) != releases::line(latest);
    let to_patch = patch.filter(|patch| releases::compare(version, patch).is_lt());
    (to_latest, to_patch)
}

fn directive_fix(id: String, title: String, args: Value) -> InspectionFix {
    InspectionFix { id, title, kind: FixKind::Apply, batch: None, label: None, inline: true, args }
}

fn go_sdk(sdk: &str, text: &str, (go, go_line): (Option<&str>, Option<u32>), findings: &mut Vec<Finding>) {
    let (Some(go), Some(line)) = (go, go_line) else { return };
    let Some((start, end)) = token_on_line(text, line, go) else { return };
    if sdk.is_empty() || !releases::compare(go, sdk).is_gt() {
        return;
    }
    let message = t!("go-version-sdk-older", sdk = sdk, go = go);
    let mut sdk_finding = finding(start, end, FindingSeverity::Warning, message, "goNewerThanSdk");
    sdk_finding.fixes.push(InspectionFix {
        id: "go.configureSdk".to_owned(),
        title: t!("go-version-configure-sdk"),
        kind: FixKind::Action,
        batch: None,
        label: None,
        inline: false,
        args: json!({ "action": "view.settings.open", "params": { "page": "go" } }),
    });
    findings.push(sdk_finding);
}

fn local_findings(
    state: &mut State,
    checks: &Checks<'_>,
    context: &InspectContext<'_>,
    parsed: &GoMod,
    dir: &Path,
    tasks: &mut Vec<Task>,
    findings: &mut Vec<Finding>,
) {
    let places = |requirement: &crate::gomod::Requirement| {
        place(context.text, requirement.line, &requirement.path, &requirement.version)
    };
    if let Ok(go_sum) = std::fs::read_to_string(dir.join("go.sum"))
        .or_else(|_| if parsed.requires.is_empty() { Err(()) } else { Ok(String::new()) })
    {
        for requirement in local::missing_from_go_sum(parsed, &go_sum) {
            let Some((_, _, start, end, _)) = places(requirement) else { continue };
            let message =
                t!("go-local-missing-sum", module = requirement.path.as_str(), version = requirement.version.as_str());
            let mut missing = finding(start, end, FindingSeverity::Error, message, "missingGoSum");
            missing.fixes.push(tidy_fix(&requirement.path));
            findings.push(missing);
        }
    }
    if context.tool.as_os_str().is_empty() {
        return;
    }
    let stamp = disk_stamp(dir);
    match state.locals.get(dir) {
        Some((known, report)) => {
            if *known != stamp {
                tasks.push(local_task(checks, context, dir, stamp));
            }
            for requirement in &parsed.requires {
                let Some((path_start, _, start, end, line_end)) = places(requirement) else { continue };
                if let Some(error) = report.errors.get(&local::requirement_key(&requirement.path, &requirement.version))
                {
                    let message = t!(
                        "go-local-list-error",
                        module = requirement.path.as_str(),
                        version = requirement.version.as_str(),
                        error = error.as_str()
                    );
                    findings.push(finding(start, end, FindingSeverity::Error, message, "goListError"));
                }
                if requirement.indirect && report.unused.contains(&requirement.path) {
                    let mut unused = finding(
                        path_start,
                        line_end,
                        FindingSeverity::WeakWarning,
                        t!("go-local-unused-indirect"),
                        "unusedRequirement",
                    );
                    unused.unused = true;
                    unused.fixes.push(tidy_fix(&requirement.path));
                    findings.push(unused);
                }
            }
        }
        None => tasks.push(local_task(checks, context, dir, stamp)),
    }
}

fn local_task(checks: &Checks<'_>, context: &InspectContext<'_>, dir: &Path, stamp: u64) -> Task {
    let version = checks.go_env.version.trim_start_matches("go");
    let tidy = !version.is_empty() && releases::compare(version, "1.23").is_ge();
    Task::Local {
        tool: context.tool.to_owned(),
        env: context.env.to_vec(),
        dir: dir.to_owned(),
        stamp,
        network: checks.updates,
        tidy,
    }
}

fn unused_by_gopls(source: Option<&str>, message: &str) -> bool {
    source == Some("go mod tidy") && message.ends_with(" is not used in this module")
}

fn inspection(id: &str, title: String, description: String) -> DeclaredInspection {
    DeclaredInspection { id: id.to_owned(), title, group: "go.mod".to_owned(), description, cleanup: false, off: false }
}

fn declared_inspections() -> Vec<DeclaredInspection> {
    vec![
        inspection("newerVersion", t!("go-inspection-newer-version"), t!("go-inspection-newer-version-description")),
        inspection(
            "newerMajorVersion",
            t!("go-inspection-newer-major-version"),
            t!("go-inspection-newer-major-version-description"),
        ),
        inspection("vulnerability", t!("go-inspection-vulnerability"), t!("go-inspection-vulnerability-description")),
        inspection(
            "deprecatedModule",
            t!("go-inspection-deprecated-module"),
            t!("go-inspection-deprecated-module-description"),
        ),
        inspection(
            "retractedVersion",
            t!("go-inspection-retracted-version"),
            t!("go-inspection-retracted-version-description"),
        ),
        inspection(
            "excludedVersion",
            t!("go-inspection-excluded-version"),
            t!("go-inspection-excluded-version-description"),
        ),
        inspection(
            "newerGoVersion",
            t!("go-inspection-newer-go-version"),
            t!("go-inspection-newer-go-version-description"),
        ),
        inspection(
            "newerToolchain",
            t!("go-inspection-newer-toolchain"),
            t!("go-inspection-newer-toolchain-description"),
        ),
        inspection(
            "goNewerThanSdk",
            t!("go-inspection-go-newer-than-sdk"),
            t!("go-inspection-go-newer-than-sdk-description"),
        ),
        inspection("missingGoSum", t!("go-inspection-missing-go-sum"), t!("go-inspection-missing-go-sum-description")),
        inspection("goListError", t!("go-inspection-go-list-error"), t!("go-inspection-go-list-error-description")),
        inspection(
            "unusedRequirement",
            t!("go-inspection-unused-requirement"),
            t!("go-inspection-unused-requirement-description"),
        ),
        inspection("useDirectory", t!("go-inspection-use-directory"), t!("go-inspection-use-directory-description")),
    ]
}

fn finding(start: usize, end: usize, severity: FindingSeverity, message: String, code: &str) -> Finding {
    Finding {
        start,
        end,
        severity,
        message,
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        source: "go.mod".to_owned(),
        code: code.to_owned(),
        deprecated: false,
        unused: false,
    }
}

fn tidy_fix(module: &str) -> InspectionFix {
    InspectionFix {
        id: format!("go.tidy:{module}"),
        title: t!("go-local-run-tidy"),
        kind: FixKind::Apply,
        batch: None,
        label: None,
        inline: false,
        args: json!({ "tidy": true }),
    }
}

fn update_fix(module: &str, current: &str, version: &str, title: String, tidy: bool, batch: bool) -> InspectionFix {
    let kind = if tidy { "go.updateTidy" } else { "go.update" };
    InspectionFix {
        id: format!("{kind}:{module}@{version}"),
        title,
        kind: FixKind::Apply,
        batch: (batch && !tidy).then(|| BATCH.to_owned()),
        label: (batch && !tidy).then(|| t!("go-deps-batch-row", module = module, from = current, to = version)),
        inline: false,
        args: json!({ "module": module, "version": version, "tidy": tidy }),
    }
}

fn batch_fix(module: &str) -> InspectionFix {
    InspectionFix {
        id: format!("go.updateDependencies:{module}"),
        title: t!("go-deps-update-dependencies"),
        kind: FixKind::Batch,
        batch: Some(BATCH.to_owned()),
        label: None,
        inline: false,
        args: json!({ "module": module }),
    }
}

fn update_fixes(module: &str, current: &str, newer: &str) -> Vec<InspectionFix> {
    vec![
        update_fix(module, current, newer, t!("go-deps-update-to", version = newer), false, true),
        update_fix(module, current, newer, t!("go-deps-update-to-tidy", version = newer), true, false),
        batch_fix(module),
    ]
}

fn newer_finding(module: &str, current: &str, newer: &str, start: usize, end: usize) -> Finding {
    let mut newer_version =
        finding(start, end, FindingSeverity::WeakWarning, t!("go-deps-newer", version = newer), "newerVersion");
    newer_version.fixes = vec![
        batch_fix(module),
        update_fix(module, current, newer, t!("go-deps-update-to", version = newer), false, true),
        update_fix(module, current, newer, t!("go-deps-update-to-tidy", version = newer), true, false),
    ];
    newer_version
}

fn deprecated_finding(module: &str, notice: &str, start: usize, end: usize) -> Finding {
    let mut deprecated =
        finding(start, end, FindingSeverity::Warning, t!("go-deps-deprecated", module = module), "deprecatedModule");
    deprecated.deprecated = true;
    deprecated.note = Some(t!("go-deps-deprecation-notice", notice = notice.trim()));
    deprecated.details = Some(format!(
        "## {}\n\n{}\n\n[pkg.go.dev/{module}](https://pkg.go.dev/{module})\n",
        t!("go-deps-deprecated", module = module),
        notice.trim()
    ));
    deprecated.fixes.push(InspectionFix {
        id: format!("go.deprecation:{module}"),
        title: t!("go-deps-show-deprecation", module = module),
        kind: FixKind::Details,
        batch: None,
        label: None,
        inline: false,
        args: json!({ "module": module }),
    });
    deprecated
}

fn retracted_finding(
    module: &str,
    current: &str,
    why: &str,
    newer: Option<&str>,
    (start, end): (usize, usize),
) -> Finding {
    let mut retracted =
        finding(start, end, FindingSeverity::Warning, t!("go-deps-retracted", version = current), "retractedVersion");
    if !why.trim().is_empty() {
        retracted.note = Some(t!("go-deps-retraction-reason", reason = why.trim()));
    }
    if let Some(newer) = newer {
        retracted.fixes = update_fixes(module, current, newer);
    }
    retracted
}

fn excluded_finding(module: &str, current: &str, newer: Option<&str>, (start, end): (usize, usize)) -> Finding {
    let mut excluded =
        finding(start, end, FindingSeverity::Warning, t!("go-deps-excluded", version = current), "excludedVersion");
    if let Some(newer) = newer {
        excluded.fixes = update_fixes(module, current, newer);
    }
    excluded
}

fn major_finding(path: &str, version: &str, start: usize, end: usize) -> Finding {
    let mut major = finding(
        start,
        end,
        FindingSeverity::Information,
        t!("go-deps-newer-major", module = path, version = version),
        "newerMajorVersion",
    );
    major.rows.push(FindingRow {
        label: format!("{path} {version}"),
        url: Some(format!("https://pkg.go.dev/{path}")),
        badge: None,
        text: String::new(),
    });
    major
}

fn vulnerable_finding(
    module: &str,
    current: &str,
    entries: &[Entry],
    severities: &[Option<Severity>],
    fixed: Option<&str>,
    outdated: bool,
    (start, end): (usize, usize),
) -> Finding {
    let rows = entries
        .iter()
        .zip(severities)
        .map(|(entry, severity)| FindingRow {
            label: entry.shown_id().to_owned(),
            url: Some(entry.url.clone()),
            badge: severity.as_ref().and_then(|severity| severity.score).map(|score| format!("{score:.1}")),
            text: entry.summary.clone(),
        })
        .collect();
    let mut fixes = Vec::new();
    if let Some(fixed) = fixed {
        fixes.push(update_fix(module, current, fixed, t!("go-deps-update-unaffected", version = fixed), false, false));
        fixes.push(update_fix(module, current, fixed, t!("go-deps-update-to-tidy", version = fixed), true, false));
    }
    if outdated {
        fixes.push(batch_fix(module));
    }
    let mut vulnerable =
        finding(start, end, FindingSeverity::Warning, t!("go-deps-vulnerable", count = entries.len()), "vulnerability");
    vulnerable.rows = rows;
    vulnerable.details = Some(details(module, current, entries, severities, fixed));
    vulnerable.fixes = fixes;
    vulnerable
}

fn details(
    module: &str,
    current: &str,
    entries: &[Entry],
    severities: &[Option<Severity>],
    fixed: Option<&str>,
) -> String {
    let mut text = format!("## {}\n\n", t!("go-deps-details-title", module = module, version = current));
    if let Some(fixed) = fixed {
        text.push_str(&format!("{}\n\n", t!("go-deps-details-unaffected", version = fixed)));
    }
    for (entry, severity) in entries.iter().zip(severities) {
        text.push_str(&format!("### {} {}\n\n", entry.id, entry.summary));
        let mut facts = Vec::new();
        if !entry.aliases.is_empty() {
            facts.push(t!("go-deps-details-aliases", aliases = entry.aliases.join(", ")));
        }
        if let Some(severity) = severity {
            let score = severity.score.map(|score| format!("{score:.1}")).unwrap_or_default();
            let rating = severity.rating.as_deref().map(str::to_lowercase).unwrap_or_default();
            facts.push(t!("go-deps-details-severity", score = score, rating = rating).trim().to_owned());
        }
        let fixes = entry.fixes(module);
        facts.push(if fixes.is_empty() {
            t!("go-deps-details-not-fixed")
        } else {
            t!("go-deps-details-fixed", versions = fixes.join(", "))
        });
        for fact in facts {
            text.push_str(&format!("- {fact}\n"));
        }
        text.push_str(&format!("\n{}\n\n[{}]({})\n\n", entry.details.trim(), entry.url, entry.url));
    }
    text.push_str(&t!("go-deps-details-source"));
    text.push('\n');
    text
}

#[cfg(test)]
mod tests;
