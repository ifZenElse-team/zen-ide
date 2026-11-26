use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;
use crate::build_tools::{BuildEnv, LoadState};
use crate::language::{BuildImport, ImportProgress};
use crate::plugin_projects::PluginProjects;

pub const RELOAD_AFTER: &str = "build.reloadAfterChanges";

const SKIPPED_FOLDERS: [&str; 5] = ["target", "build", "node_modules", "out", "bin"];
const MAX_FOLDERS: usize = 4000;
const MAX_MESSAGES: usize = 20;
const KEPT_SYNCS: usize = 10;
const IMPORT_LIMIT: Duration = Duration::from_secs(600);

/// Which changes to build scripts load the project again by themselves; the others show Load Changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReloadAfter {
    /// Changes made outside the IDE, as a VCS update or another program; edits in the IDE show Load Changes.
    #[default]
    External,
    /// Any change once it is saved, edits in the IDE too.
    Any,
}

impl ReloadAfter {
    pub fn of(settings: &zen_core::Settings) -> Self {
        settings.get_or(RELOAD_AFTER, ReloadAfter::default())
    }

    pub fn name(self) -> &'static str {
        match self {
            ReloadAfter::External => "external",
            ReloadAfter::Any => "any",
        }
    }
}

/// A build system a plug-in loads the open project with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildSystem {
    /// The plug-in's id, e.g. `maven`.
    pub id: String,
    pub name: String,
    /// The names of its build scripts, e.g. `pom.xml`.
    pub files: Vec<String>,
    /// The command that loads the project again, e.g. `maven.project.reload`.
    pub reload: String,
}

impl BuildSystem {
    pub fn is_build_file(&self, path: &Path) -> bool {
        path.file_name().and_then(|name| name.to_str()).is_some_and(|name| self.files.iter().any(|file| file == name))
    }
}

pub fn systems(session: &Session, root: &Path) -> Vec<BuildSystem> {
    session
        .state::<PluginProjects>()
        .map(|projects| projects.supporting(root))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|plugin| {
            let build = plugin.support.build?;
            Some(BuildSystem { id: plugin.id, name: plugin.name, files: build.files, reload: build.reload })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Seen {
    stamp: Option<(SystemTime, u64)>,
    hash: Option<u64>,
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

fn seen(path: &Path) -> Seen {
    let hash = std::fs::read(path).ok().map(|bytes| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        hasher.finish()
    });
    Seen { stamp: stamp(path), hash }
}

#[derive(Clone, Debug)]
struct Snapshot {
    root: PathBuf,
    files: BTreeMap<PathBuf, Seen>,
    saves: BTreeMap<PathBuf, u64>,
}

#[derive(Default)]
pub struct BuildSync {
    loaded: BTreeMap<String, Snapshot>,
    syncs: Vec<SyncRecord>,
    next: u64,
}

pub fn find_build_files(root: &Path, system: &BuildSystem) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_owned()];
    let mut visited = 0;
    while let Some(folder) = folders.pop() {
        visited += 1;
        if visited > MAX_FOLDERS {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIPPED_FOLDERS.contains(&name.as_str()) {
                    folders.push(path);
                }
            } else if system.is_build_file(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn open_saves(session: &Session, system: &BuildSystem, root: &Path) -> BTreeMap<PathBuf, u64> {
    session
        .state::<Workbench>()
        .map(|workbench| {
            workbench
                .documents()
                .iter()
                .filter(|document| document.path().starts_with(root) && system.is_build_file(document.path()))
                .map(|document| (document.path().to_owned(), document.save_count()))
                .collect()
        })
        .unwrap_or_default()
}

fn take_snapshot(session: &mut Session, system: &BuildSystem, root: &Path) {
    let mut paths = find_build_files(root, system);
    let saves = open_saves(session, system, root);
    paths.extend(saves.keys().filter(|path| !paths.contains(path)).cloned().collect::<Vec<_>>());
    let files = paths.into_iter().map(|path| (seen(&path), path)).map(|(seen, path)| (path, seen)).collect();
    session.state_mut::<BuildSync>().loaded.insert(system.id.clone(), Snapshot { root: root.to_owned(), files, saves });
}

pub fn mark_loaded(session: &mut Session, system: &str, root: &Path) {
    if let Some(found) = systems(session, root).into_iter().find(|known| known.id == system) {
        take_snapshot(session, &found, root);
    }
}

/// A build system whose scripts changed since the project was last loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingChanges {
    #[serde(flatten)]
    pub system: BuildSystem,
    /// The changed build scripts.
    pub changed: Vec<PathBuf>,
    /// Whether the changes were all made outside the IDE (a VCS update, another program).
    pub external: bool,
    /// Whether some of them are edits not saved yet.
    pub unsaved: bool,
    /// Whether the IDE loads them by itself (build.reloadAfterChanges); otherwise Load Changes shows.
    pub auto_reload: bool,
}

pub fn pending(session: &mut Session) -> Vec<PendingChanges> {
    let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
        return Vec::new();
    };
    let setting = ReloadAfter::of(session.settings());
    let mut found = Vec::new();
    for system in systems(session, &root) {
        let fresh =
            session.state::<BuildSync>().and_then(|sync| sync.loaded.get(&system.id)).filter(|s| s.root == root);
        let Some(mut snapshot) = fresh.cloned() else {
            take_snapshot(session, &system, &root);
            continue;
        };
        let open: Vec<(PathBuf, bool, u64)> = session
            .state::<Workbench>()
            .map(|workbench| {
                workbench
                    .documents()
                    .iter()
                    .filter(|document| document.path().starts_with(&root) && system.is_build_file(document.path()))
                    .map(|document| (document.path().to_owned(), document.is_dirty(), document.save_count()))
                    .collect()
            })
            .unwrap_or_default();
        let mut changed = Vec::new();
        let mut refreshed = Vec::new();
        let (mut edited, mut unsaved) = (false, false);
        for (path, _, saves) in &open {
            snapshot.saves.entry(path.clone()).or_insert(*saves);
            if !snapshot.files.contains_key(path) {
                refreshed.push((path.clone(), seen(path)));
            }
        }
        for (path, before) in &snapshot.files {
            let document = open.iter().find(|(open_path, _, _)| open_path == path);
            let dirty = document.is_some_and(|(_, dirty, _)| *dirty);
            let saved = document.is_some_and(|(_, _, saves)| snapshot.saves.get(path).is_some_and(|base| saves > base));
            let now = (stamp(path) != before.stamp).then(|| seen(path));
            let on_disk = now.as_ref().is_some_and(|now| now.hash != before.hash);
            if let Some(now) = now.filter(|now| now.hash == before.hash) {
                refreshed.push((path.clone(), now));
            }
            if dirty || on_disk {
                changed.push(path.clone());
                edited |= dirty || saved;
                unsaved |= dirty;
            }
        }
        if let Some(stored) = session.state_mut::<BuildSync>().loaded.get_mut(&system.id) {
            stored.saves = snapshot.saves;
            stored.files.extend(refreshed);
        }
        if changed.is_empty() {
            continue;
        }
        let auto_reload =
            (!edited || (setting == ReloadAfter::Any && !unsaved)) && crate::trust::trusted(session, &root);
        found.push(PendingChanges { system, changed, external: !edited, unsaved, auto_reload });
    }
    found
}

/// How bad a message of a sync is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// The obvious fix of a sync's message: a command to run, and whether the project loads again after it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncFix {
    pub title: String,
    pub command: String,
    pub params: Value,
    /// Load the project again once the command ran.
    pub reload: bool,
}

/// One message of a sync: what went wrong, where, and its obvious fix.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncMessage {
    pub severity: Severity,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// The line in `file`, from 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column in `line`, from 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// The character offset of the place in `file`, for navigate.place.goTo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<SyncFix>,
}

impl SyncMessage {
    pub fn new(severity: Severity, text: impl Into<String>) -> Self {
        Self { severity, text: text.into(), file: None, line: None, column: None, offset: None, fix: None }
    }

    pub fn at(mut self, file: PathBuf, line: Option<u32>, column: Option<u32>) -> Self {
        self.file = Some(file);
        self.line = line;
        self.column = column;
        self
    }

    pub fn with_fix(mut self, fix: Option<SyncFix>) -> Self {
        self.fix = fix;
        self
    }
}

pub type Problems = fn(&str, &Path) -> Vec<SyncMessage>;

#[derive(Clone)]
enum StepRecord {
    Done { title: String, took: Duration, messages: Vec<SyncMessage> },
    Background { title: String, key: String, problems: Problems },
    Import { title: String, server: String, started: Instant, ended: Option<(Duration, Vec<SyncMessage>)> },
}

#[derive(Clone)]
struct SyncRecord {
    id: u64,
    system: String,
    name: String,
    root: PathBuf,
    started: Instant,
    at: SystemTime,
    steps: Vec<StepRecord>,
}

pub struct SyncRun {
    record: SyncRecord,
}

impl SyncRun {
    pub fn start(system: &str, name: &str, root: &Path) -> Self {
        Self {
            record: SyncRecord {
                id: 0,
                system: system.to_owned(),
                name: name.to_owned(),
                root: root.to_owned(),
                started: Instant::now(),
                at: SystemTime::now(),
                steps: Vec::new(),
            },
        }
    }

    pub fn step(&mut self, title: String, started: Instant, messages: Vec<SyncMessage>) {
        self.record.steps.push(StepRecord::Done { title, took: started.elapsed(), messages });
    }

    pub fn background(&mut self, title: String, key: String, problems: Problems) {
        self.record.steps.push(StepRecord::Background { title, key, problems });
    }

    pub fn import(&mut self, title: String, server: String, started: Instant) {
        self.record.steps.push(StepRecord::Import { title, server, started, ended: None });
    }

    pub fn finish(mut self, session: &mut Session) {
        let root = self.record.root.clone();
        let system = self.record.system.clone();
        let sync = session.state_mut::<BuildSync>();
        sync.next += 1;
        self.record.id = sync.next;
        sync.syncs.push(self.record);
        let extra = sync.syncs.len().saturating_sub(KEPT_SYNCS);
        sync.syncs.drain(..extra);
        mark_loaded(session, &system, &root);
    }
}

/// Where a sync or one of its steps is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StepState {
    Running,
    Done,
    /// Done, with warnings.
    Warning,
    Failed,
}

fn state_of(messages: &[SyncMessage]) -> StepState {
    match messages.iter().map(|message| message.severity).min() {
        Some(Severity::Error) => StepState::Failed,
        Some(Severity::Warning) => StepState::Warning,
        _ => StepState::Done,
    }
}

/// A step of a sync, as the Sync tab of the Build tool window shows it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncStep {
    pub title: String,
    pub state: StepState,
    /// How long it took; absent while it runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<SyncMessage>,
    /// The background task that runs it (`tasks.log.read`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

/// One load of the project by a build system, as the Sync tab of the Build tool window shows it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub id: u64,
    /// The build system's plug-in, e.g. `maven`.
    pub system: String,
    pub name: String,
    pub state: StepState,
    /// When it started, in milliseconds since the Unix epoch.
    pub started_at_ms: u64,
    /// How long it took; absent while it runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    pub errors: usize,
    pub warnings: usize,
    pub steps: Vec<SyncStep>,
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn offset_of(file: &Path, line: u32, column: Option<u32>) -> Option<usize> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut offset = 0;
    for (index, text_line) in text.split('\n').enumerate() {
        if index + 1 == line as usize {
            let column = column.unwrap_or(1).saturating_sub(1) as usize;
            return Some(offset + column.min(text_line.chars().count()));
        }
        offset += text_line.chars().count() + 1;
    }
    None
}

fn located(mut messages: Vec<SyncMessage>) -> Vec<SyncMessage> {
    messages.truncate(MAX_MESSAGES);
    for message in &mut messages {
        if let (Some(file), Some(line)) = (&message.file, message.line) {
            message.offset = offset_of(file, line, message.column);
        }
    }
    messages
}

fn report(record: &SyncRecord, env: &BuildEnv) -> SyncReport {
    let mut ends = Vec::new();
    let mut running = false;
    let mut steps = Vec::new();
    let mut elapsed = Duration::ZERO;
    for step in &record.steps {
        let shown = match step {
            StepRecord::Done { title, took, messages } => {
                elapsed += *took;
                ends.push(elapsed);
                SyncStep {
                    title: title.clone(),
                    state: state_of(messages),
                    took_ms: Some(millis(*took)),
                    messages: located(messages.clone()),
                    task: None,
                }
            }
            StepRecord::Background { title, key, problems } => match env.load(key) {
                Some(load) if load.state == LoadState::Loading => {
                    running = true;
                    SyncStep {
                        title: title.clone(),
                        state: StepState::Running,
                        took_ms: None,
                        messages: Vec::new(),
                        task: load.task,
                    }
                }
                Some(load) => {
                    let took = load.took.unwrap_or_default();
                    ends.push(load.started.saturating_duration_since(record.started) + took);
                    let messages = match (&load.state, &load.error) {
                        (LoadState::Failed, Some(error)) => {
                            let text = format!("{error}\n{}", load.output);
                            let mut found = problems(&text, &record.root);
                            if found.is_empty() {
                                found.push(SyncMessage::new(Severity::Error, error.lines().next().unwrap_or_default()));
                            }
                            found
                        }
                        _ => Vec::new(),
                    };
                    SyncStep {
                        title: title.clone(),
                        state: state_of(&messages),
                        took_ms: Some(millis(took)),
                        messages: located(messages),
                        task: load.task,
                    }
                }
                None => SyncStep {
                    title: title.clone(),
                    state: StepState::Done,
                    took_ms: None,
                    messages: Vec::new(),
                    task: None,
                },
            },
            StepRecord::Import { title, started, ended, .. } => match ended {
                None => {
                    running = true;
                    SyncStep {
                        title: title.clone(),
                        state: StepState::Running,
                        took_ms: None,
                        messages: Vec::new(),
                        task: None,
                    }
                }
                Some((took, messages)) => {
                    ends.push(started.saturating_duration_since(record.started) + *took);
                    SyncStep {
                        title: title.clone(),
                        state: state_of(messages),
                        took_ms: Some(millis(*took)),
                        messages: located(messages.clone()),
                        task: None,
                    }
                }
            },
        };
        steps.push(shown);
    }
    let all = || steps.iter().flat_map(|step| &step.messages);
    let errors = all().filter(|message| message.severity == Severity::Error).count();
    let warnings = all().filter(|message| message.severity == Severity::Warning).count();
    let state = if running {
        StepState::Running
    } else if steps.iter().any(|step| step.state == StepState::Failed) {
        StepState::Failed
    } else if steps.iter().any(|step| step.state == StepState::Warning) {
        StepState::Warning
    } else {
        StepState::Done
    };
    SyncReport {
        id: record.id,
        system: record.system.clone(),
        name: record.name.clone(),
        state,
        started_at_ms: record.at.duration_since(SystemTime::UNIX_EPOCH).map_or(0, millis),
        took_ms: (!running).then(|| ends.into_iter().max().map_or(0, millis)),
        errors,
        warnings,
        steps,
    }
}

pub fn reports(session: &Session) -> Vec<SyncReport> {
    let env = session.state::<BuildEnv>().cloned().unwrap_or_default();
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    session
        .state::<BuildSync>()
        .map(|sync| {
            sync.syncs
                .iter()
                .rev()
                .filter(|record| root.as_ref().is_none_or(|root| *root == record.root))
                .map(|record| report(record, &env))
                .collect()
        })
        .unwrap_or_default()
}

pub fn jdk_trouble(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "java_home",
        "unsupported class file major version",
        "unsupportedclassversionerror",
        "invalid target release",
        "no compiler is provided",
        "could not determine java version",
        "requires java",
        "incompatible java",
        "toolchain",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || (lower.contains("release version") && lower.contains("not supported"))
}

pub fn network_trouble(text: &str) -> bool {
    let lower = text.to_lowercase();
    !lower.contains("offline")
        && [
            "could not transfer",
            "unknownhostexception",
            "unknown host",
            "connection refused",
            "connect timed out",
            "read timed out",
            "network is unreachable",
            "no route to host",
            "could not get resource",
            "could not resolve",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn import_messages(import: &BuildImport) -> Vec<SyncMessage> {
    let fix = |title: String, command: &str, id: &str| SyncFix {
        title,
        command: command.to_owned(),
        params: serde_json::json!({ "id": id }),
        reload: false,
    };
    match import {
        BuildImport::Asked { .. } | BuildImport::Told => Vec::new(),
        BuildImport::NoServer => vec![SyncMessage::new(Severity::Info, t!("workbench-sync-no-java-server"))],
        BuildImport::Disabled { name } => {
            vec![SyncMessage::new(Severity::Info, t!("workbench-sync-java-server-off", server = name.as_str()))]
        }
        BuildImport::Stopped { id, name } => {
            let message =
                SyncMessage::new(Severity::Info, t!("workbench-sync-java-server-stopped", server = name.as_str()));
            vec![message.with_fix(Some(fix(t!("workbench-sync-fix-restart-server"), "lsp.server.restart", id)))]
        }
        BuildImport::Missing { id, name, installable } => {
            let text = t!("workbench-sync-java-server-missing", server = name.as_str());
            let title = t!("workbench-sync-fix-install-server", server = name.as_str());
            let install = installable.then(|| fix(title, "lsp.server.install", id));
            vec![SyncMessage::new(Severity::Warning, text).with_fix(install)]
        }
    }
}

pub fn java_import(session: &mut Session, sync: &mut SyncRun, env: &BuildEnv, root: &Path, files: &[PathBuf]) -> bool {
    let started = Instant::now();
    let title = t!("workbench-sync-import");
    let import =
        crate::language::with_languages(session, |languages, workbench| languages.import_build(workbench, files));
    let hooked = env.reimport(root);
    match import.clone() {
        BuildImport::Asked { server } => sync.import(title, server, started),
        _ if hooked => sync.step(title, started, Vec::new()),
        other => sync.step(title, started, import_messages(&other)),
    }
    hooked || import.asked()
}

fn import_outcome(progress: ImportProgress, waited: Duration) -> Option<Vec<SyncMessage>> {
    match progress {
        ImportProgress::Done => Some(Vec::new()),
        ImportProgress::Running if waited >= IMPORT_LIMIT => {
            Some(vec![SyncMessage::new(Severity::Warning, t!("workbench-sync-import-long"))])
        }
        ImportProgress::Running => None,
        ImportProgress::Failed { reason, java } => {
            let fix = java.map(|version| SyncFix {
                title: t!("jdk-no-match-download", level = version),
                command: "toolchain.jdk.download".to_owned(),
                params: serde_json::json!({ "version": version }),
                reload: false,
            });
            Some(vec![SyncMessage::new(Severity::Error, reason).with_fix(fix)])
        }
    }
}

pub fn settle_imports(session: &mut Session) {
    let waiting = |step: &StepRecord| matches!(step, StepRecord::Import { ended: None, .. });
    let pending = session
        .state::<BuildSync>()
        .is_some_and(|sync| sync.syncs.iter().any(|record| record.steps.iter().any(waiting)));
    if !pending {
        return;
    }
    crate::language::with_languages(session, |languages, workbench| languages.sync(workbench));
    let Some(languages) = session.state::<crate::language::Languages>() else { return };
    let mut ended = Vec::new();
    let syncs = session.state::<BuildSync>().map(|sync| sync.syncs.as_slice()).unwrap_or_default();
    for (at, record) in syncs.iter().enumerate() {
        for (place, step) in record.steps.iter().enumerate() {
            if let StepRecord::Import { server, started, ended: None, .. } = step {
                let waited = started.elapsed();
                if let Some(messages) = import_outcome(languages.import_progress(server, *started), waited) {
                    ended.push((at, place, waited, messages));
                }
            }
        }
    }
    let sync = session.state_mut::<BuildSync>();
    for (at, place, took, messages) in ended {
        if let StepRecord::Import { ended, .. } = &mut sync.syncs[at].steps[place] {
            *ended = Some((took, messages));
        }
    }
}

pub fn wait_imports(session: &mut Session, wait: Duration) {
    let end = Instant::now() + wait.min(crate::run::MAX_RUN_WAIT);
    loop {
        settle_imports(session);
        let waiting = session.state::<BuildSync>().is_some_and(|sync| {
            sync.syncs
                .iter()
                .any(|record| record.steps.iter().any(|step| matches!(step, StepRecord::Import { ended: None, .. })))
        });
        if !waiting || Instant::now() >= end {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn jdk_fix() -> SyncFix {
    SyncFix {
        title: t!("workbench-sync-fix-jdk"),
        command: "view.settings.open".to_owned(),
        params: serde_json::json!({ "page": "projectStructure" }),
        reload: false,
    }
}

pub type Trouble = fn(&str) -> bool;

pub fn attach_fixes(messages: &mut [SyncMessage], text: &str, fixes: &[(Trouble, SyncFix)]) {
    for (trouble, fix) in fixes {
        if !trouble(text) {
            continue;
        }
        let own = messages.iter().position(|message| message.fix.is_none() && trouble(&message.text));
        let first = messages.iter().position(|message| message.fix.is_none() && message.severity == Severity::Error);
        if let Some(at) = own.or(first) {
            messages[at].fix = Some(fix.clone());
        }
    }
}

/// What `build.changes.get` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildChanges {
    /// The build systems the open project is loaded with (from their plug-ins).
    pub systems: Vec<BuildSystem>,
    /// Those whose build scripts changed since the project was last loaded.
    pub pending: Vec<PendingChanges>,
    /// The setting build.reloadAfterChanges.
    pub reload_after: ReloadAfter,
}

pub struct GetChanges;

impl Command for GetChanges {
    type Params = NoParams;
    type Output = BuildChanges;
    const ID: &'static str = "build.changes.get";
    const TITLE: &'static str = "Build Script Changes";
    const DESCRIPTION: &'static str = "The build systems the open project is loaded with (Maven, Gradle: from their \
         plug-ins) and those whose build scripts (pom.xml, build.gradle(.kts), settings.gradle(.kts)) changed since \
         the project was last loaded: the files, whether the changes came from outside the IDE, and whether the IDE \
         loads them by itself (setting build.reloadAfterChanges: external or any). Otherwise the editor shows Load \
         Changes (view.buildChanges.load, ⇧⌘I), which runs each one's reload command.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<BuildChanges, CommandError> {
        let root = session
            .state::<Workbench>()
            .and_then(Workbench::project)
            .map(Path::to_owned)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        Ok(BuildChanges {
            systems: systems(session, &root),
            pending: pending(session),
            reload_after: ReloadAfter::of(session.settings()),
        })
    }
}

/// What `build.sync.get` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncList {
    /// The loads of the open project, newest first.
    pub syncs: Vec<SyncReport>,
}

pub struct GetSyncs;

impl Command for GetSyncs {
    type Params = NoParams;
    type Output = SyncList;
    const ID: &'static str = "build.sync.get";
    const TITLE: &'static str = "Sync";
    const DESCRIPTION: &'static str = "The Sync tab of the Build tool window: each load of the open project by a build \
         system (maven.project.reload, gradle.project.reload, Load Changes), newest first, with its steps, how long \
         each took, and their errors and warnings: the step Import into the Java language server runs until the \
         server says it finished, and fails with its reason; the file, line and column (offset for navigate.place.goTo) and \
         the obvious fix, a command to run (download the tool, choose a JDK, work offline) and whether to load again \
         after it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<SyncList, CommandError> {
        settle_imports(session);
        Ok(SyncList { syncs: reports(session) })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetChanges)?;
    registry.register(GetSyncs)?;
    Ok(())
}

#[cfg(test)]
mod tests;
