use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::lsp_types::FileChangeType;
use zen_lsp::{Client, DocumentRequest, Edit, Event, Launcher, LspError};

use crate::Workbench;

pub mod activity;
#[cfg(test)]
mod agent_tests;
#[cfg(test)]
mod alongside_tests;
pub mod breadcrumbs;
pub mod classes;
pub mod code_vision;
pub mod compact;
pub mod completion;
pub mod docs;
pub mod edits;
pub mod extract;
pub(crate) mod file_operations;
pub mod format;
pub mod generate;
pub mod go_to_test;
pub mod goto;
pub mod hierarchy;
pub mod highlights;
pub mod inlay_hints;
pub mod intentions;
pub mod method_separators;
pub mod navigation_bar;
pub mod peek;
pub mod places;
mod power_save;
mod pull;
pub mod refactor;
pub mod refactoring_dialog;
pub mod safe_delete;
pub mod semantic;
mod server_tasks;
pub mod servers;
pub mod show_usages;
pub mod structure;
pub(crate) mod sweep;
pub mod symbols;
#[cfg(test)]
pub(crate) mod testing;
pub mod usages;
pub mod usages_settings;

pub const MAX_WAIT: Duration = Duration::from_secs(10);

pub const MAX_LOADING_WAIT: Duration = Duration::from_secs(120);

pub const LOADING_WAIT_COMMANDS: &[&str] = &["navigate.symbol.goTo", "navigate.class.goTo"];

#[derive(Clone, Copy, Debug)]
pub struct SearchWait {
    started: Instant,
    asked: Duration,
    loaded: Instant,
    explicit: bool,
}

impl SearchWait {
    pub fn new(asked: Duration) -> Self {
        let now = Instant::now();
        Self { started: now, asked, loaded: now, explicit: false }
    }

    pub fn explicit(asked: Duration) -> Self {
        Self { explicit: true, ..Self::new(asked) }
    }

    pub fn keeps_waiting(&mut self, pending: bool, reason: Option<Reason>) -> bool {
        self.keeps_waiting_at(Instant::now(), pending, reason)
    }

    pub fn keeps_waiting_at(&mut self, now: Instant, pending: bool, reason: Option<Reason>) -> bool {
        let loading = matches!(reason, Some(Reason::Scanning | Reason::Starting | Reason::Indexing));
        if loading {
            self.loaded = now;
        }
        let waited = now.saturating_duration_since(self.started);
        if !pending || waited >= self.asked.min(MAX_LOADING_WAIT) {
            return false;
        }
        self.explicit || loading || now.saturating_duration_since(self.loaded) < MAX_WAIT.min(self.asked)
    }

    pub fn left(&self) -> Duration {
        self.asked.min(MAX_LOADING_WAIT).saturating_sub(self.started.elapsed())
    }
}

pub type Launch = Arc<dyn Fn(&Path) -> Result<Launcher, String> + Send + Sync>;

pub(crate) type Options = Arc<dyn Fn(&Path) -> Value + Send + Sync>;

#[derive(Clone)]
pub struct ServerSpec {
    pub language_id: String,
    pub extensions: Vec<String>,
    pub launch: Launch,
    pub initialization_options: Value,
}

/// How bad a problem is, as the server rates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// An error: the code does not build.
    Error,
    /// A warning.
    Warning,
    /// A weak warning: drawn quieter than a warning and counted apart.
    /// Inspections give it (e.g. a dependency with a newer version); language servers do not.
    WeakWarning,
    /// A misspelt word, from the spelling inspection: underlined in the theme's typo colour, never
    /// an error, and left out of the project's problems.
    Typo,
    /// Information.
    Information,
    /// A hint, such as a possible simplification.
    Hint,
}

/// A problem in a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// The first character it covers in the file's text (a character offset, as everywhere in the
    /// platform; the protocol's positions are converted).
    pub start: usize,
    /// The character after the last it covers.
    pub end: usize,
    /// The line it starts on, one-based.
    pub line: u32,
    /// The column it starts at, one-based, in characters, as the editor shows it.
    pub column: u32,
    /// How bad it is.
    pub severity: Severity,
    /// What is wrong, in the server's words.
    pub message: String,
    /// Who found it, e.g. `compiler`.
    pub source: Option<String>,
    /// Its code, e.g. `UndeclaredName`.
    pub code: Option<String>,
    /// A paragraph of its popup under the message, e.g. a deprecation notice (inspections only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The rows of its popup under the message, e.g. one per vulnerability (inspections only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<zen_project::inspect::FindingRow>,
    /// More about it in Markdown, for "Show details…" (inspections only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// Its quick-fixes, the main one first (inspections only; `problems.fix.apply` applies them).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<zen_project::inspect::InspectionFix>,
    /// Whether what it covers is deprecated: struck out instead of underlined (inspections only).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
    /// Whether what it covers is unused (an inspection's unused finding, or a diagnostic the server
    /// tags `Unnecessary`): its text is drawn in the theme's unused colour, and only an error is
    /// still underlined.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildImport {
    Asked { server: String },
    Told,
    NoServer,
    Disabled { name: String },
    Stopped { id: String, name: String },
    Missing { id: String, name: String, installable: bool },
}

impl BuildImport {
    pub fn asked(&self) -> bool {
        matches!(self, BuildImport::Asked { .. } | BuildImport::Told)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportProgress {
    Running,
    Done,
    Failed { reason: String, java: Option<u32> },
}

const IMPORT_SETTLES: Duration = Duration::from_millis(1500);

#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) declaration: servers::ServerDeclaration,
    launch: Launch,
    options: Options,
    available: bool,
    enabled: bool,
    declared: bool,
}

impl Entry {
    fn from_spec(spec: &ServerSpec) -> Self {
        let declaration = servers::ServerDeclaration {
            id: spec.language_id.clone(),
            name: spec.language_id.clone(),
            languages: vec![spec.language_id.clone()],
            extensions: spec.extensions.clone(),
            names: Vec::new(),
            watches: Vec::new(),
            command: String::new(),
            args: Vec::new(),
            version_args: Vec::new(),
            install: None,
            initialization_options: spec.initialization_options.clone(),
            inlay_hints: None,
            language_ids: Default::default(),
            workspace_symbols: true,
            optimize_imports: Vec::new(),
            alongside: false,
            formats: false,
            fix_on_save: None,
            settings: Default::default(),
            from_project: false,
            when: None,
            power_save: None,
            java: None,
            notice: None,
            virtual_documents: None,
            generate: None,
            inspections: Vec::new(),
            inspection_settings: None,
            suppression: None,
            diagnostics_settle_ms: 0,
            schema_support: None,
        };
        let declared_options = spec.initialization_options.clone();
        let options: Options = Arc::new(move |_| declared_options.clone());
        Self { declaration, launch: Arc::clone(&spec.launch), options, available: true, enabled: true, declared: false }
    }

    fn declared(
        declaration: servers::ServerDeclaration,
        registry: &servers::LanguageServers,
        root: Option<&Path>,
    ) -> Self {
        let for_project = declaration.when.as_ref().is_none_or(|when| root.is_some_and(|root| when.matches(root)));
        let available = for_project && registry.executable_in(&declaration.id, root).is_some();
        let enabled = registry.is_enabled(&declaration.id);
        let (id, launcher) = (declaration.id.clone(), registry.clone());
        let launch: Launch = Arc::new(move |root| launcher.launcher(&id, root));
        let (id, settings) = (declaration.id.clone(), registry.clone());
        let options: Options = Arc::new(move |root| settings.initialization_options(&id, root));
        Self { declaration, launch, options, available, enabled, declared: true }
    }

    pub(crate) fn id(&self) -> &str {
        &self.declaration.id
    }

    fn serves(&self, path: &Path) -> bool {
        known_name(path).is_some_and(|name| self.declaration.serves(&name))
    }

    pub(crate) fn language_for(&self, path: &Path) -> String {
        self.declaration.language_for(&known_name(path).unwrap_or_default())
    }
}

type LaunchKey = (Option<PathBuf>, Vec<String>);

fn known_name(path: &Path) -> Option<String> {
    zen_vfs::file_types::known_name_at(path, None)
}

/// The state of a language server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ServerStatus {
    /// Being started.
    Starting,
    /// Running.
    Running,
    /// Stopped by the user.
    Stopped,
    /// It could not start or stopped for good.
    Failed {
        /// Why, in words.
        reason: String,
    },
    /// Not installed, while the project's files or an open file need it.
    Missing {
        /// How to install it (`language.servers.status` says; `language.servers.sync` leaves it out).
        #[serde(skip_serializing_if = "Option::is_none")]
        install: Option<ServerInstall>,
        /// What it means, in words.
        message: String,
    },
}

/// Work a language server reports under way with the protocol's work-done progress (`$/progress`),
/// such as gopls loading the workspace's packages: until it ends, its answers may be incomplete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkProgress {
    /// The server's id, e.g. `gopls`.
    pub server: String,
    /// What it does, in the server's words, e.g. `Loading packages`.
    pub title: String,
    /// Where it is, in the server's words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// How far it is, 0 to 100, when the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage: Option<u32>,
    /// Whether the server lets it be cancelled.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cancellable: bool,
}

/// Why a search or lookup over the language servers has no answer yet, or none at all: agents get
/// it with an empty answer instead of a bare empty list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// The project's files are still being listed, so the servers it needs are not known yet.
    Scanning,
    /// A server is starting.
    Starting,
    /// A server reports work under way (`$/progress`), such as loading the workspace: its answer
    /// may be incomplete, and it is asked again when the work ends.
    Indexing,
    /// A server has not answered yet.
    Searching,
    /// The servers answered and nothing matches.
    NoMatch,
    /// No running server of the project can answer this.
    NoServer,
    /// The server that would answer is not installed: `install` says how to install it.
    ServerMissing,
    /// The server that would answer is installed but does not run: it failed, was stopped or turned
    /// off, or nothing it serves is open yet.
    ServerNotRunning,
    /// The file's server has not checked the file yet: it is not open in the server, or the
    /// server's first diagnostics of it have not come (`problems.file.list`).
    NotChecked,
    /// The file changed since its server last checked it, and the new check has not come yet: the
    /// problems listed are of the text before (`problems.file.list`).
    Checking,
    /// Power Save Mode is on: files are not checked as they change (`problems.file.list`).
    PowerSave,
    /// The project is not trusted, so its language server does not start until the person trusts it.
    NotTrusted,
}

/// How to install a missing language server: the IDE's command when the IDE can install it, else
/// what to do by hand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerInstall {
    /// The command that installs it, `lsp.server.install`, when the IDE can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The parameters of `command`: `{"id": <server>}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// The command line it runs, e.g. `rustup component add rust-analyzer`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs: Option<String>,
    /// What to do when the IDE cannot install it, e.g. install the tool that does first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Why the language server that would answer a command is not there (ADR-0087: no silent empty
/// answers): an answer with nothing, or the error of a lookup, carries it instead of an empty list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerAbsence {
    /// `serverMissing`, `serverNotRunning` or `starting`.
    pub reason: Reason,
    /// The server's id, e.g. `rust-analyzer`.
    pub server: String,
    /// How to install it, when it is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<ServerInstall>,
    /// What it means, in words.
    pub message: String,
}

/// The facts of a [`ServerAbsence`] next to an answer's own `reason`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AbsentServer {
    /// The server's id, e.g. `rust-analyzer`.
    pub server: String,
    /// How to install it, when it is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<ServerInstall>,
    /// What it means, in words.
    pub message: String,
}

impl ServerAbsence {
    pub fn split(self) -> (Reason, AbsentServer) {
        (self.reason, AbsentServer { server: self.server, install: self.install, message: self.message })
    }

    pub fn error(&self) -> CommandError {
        let code = serde_json::to_value(self.reason).ok().and_then(|code| code.as_str().map(str::to_owned));
        let data = serde_json::to_value(self).unwrap_or_default();
        CommandError::with_code(code.as_deref().unwrap_or("serverNotRunning"), self.message.clone()).with_data(data)
    }
}

pub(crate) struct Synced {
    uri: String,
    version: u64,
    text: String,
    dirty: bool,
    saves: u64,
    tokens: semantic::FileTokens,
    symbols: structure::FileSymbols,
    hints: inlay_hints::FileHints,
    changed: Instant,
}

const QUIET_CHECK: Duration = Duration::from_secs(2);

impl Synced {
    fn lent(uri: String, text: String) -> Self {
        Synced {
            uri,
            version: u64::MAX,
            text,
            dirty: false,
            saves: 0,
            tokens: semantic::FileTokens::default(),
            symbols: structure::FileSymbols::default(),
            hints: inlay_hints::FileHints::default(),
            changed: Instant::now(),
        }
    }
}

enum Server {
    Starting(Receiver<Result<Client, String>>),
    Running { client: Box<Client>, files: HashMap<PathBuf, Synced> },
    Failed { reason: zen_i18n::Message, launch: LaunchKey },
    Stopped,
}

#[derive(Default)]
struct UnopenedFiles {
    asking: Option<std::sync::mpsc::Sender<PathBuf>>,
    read: Arc<Mutex<Vec<(PathBuf, String)>>>,
}

impl UnopenedFiles {
    fn read(&mut self, path: &Path, waker: Option<&zen_lsp::connection::Waker>) -> Option<String> {
        if self.asking.is_none() {
            let (asking, asked) = std::sync::mpsc::channel::<PathBuf>();
            let read = Arc::clone(&self.read);
            let waker = waker.cloned();
            let reader = std::thread::Builder::new().name("zen-unopened-files".to_owned()).spawn(move || {
                for path in asked {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    read.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((path, text));
                    if let Some(waker) = &waker {
                        waker();
                    }
                }
            });
            if reader.is_err() {
                return Some(std::fs::read_to_string(path).unwrap_or_default());
            }
            self.asking = Some(asking);
        }
        self.asking.as_ref().map(|asking| asking.send(path.to_owned()));
        None
    }

    fn take_read(&mut self) -> Vec<(PathBuf, String)> {
        std::mem::take(&mut *self.read.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

#[derive(Default)]
pub struct Languages {
    specs: Vec<ServerSpec>,
    registry: servers::LanguageServers,
    entries: Vec<Entry>,
    entries_generation: Option<u64>,
    plugin_stamps: BTreeMap<String, u64>,
    opened: BTreeSet<String>,
    scan: servers::Scan,
    servers: BTreeMap<String, Server>,
    problems: BTreeMap<PathBuf, Vec<Problem>>,
    diagnostics: BTreeMap<PathBuf, Vec<Diagnostic>>,
    server_requests: Vec<(String, zen_lsp::ServerRequest)>,
    rename_at: Option<Value>,
    executing: Option<String>,
    notices: Vec<String>,
    waker: Option<zen_lsp::connection::Waker>,
    unopened: UnopenedFiles,
    completion: Option<completion::Completion>,
    expression: Option<completion::ExpressionCompletion>,
    lookups: BTreeMap<LookupKind, Lookup>,
    asked: BTreeMap<LookupKind, (Value, u8)>,
    symbols: Option<symbols::SymbolSearch>,
    keep_running: BTreeSet<String>,
    outside_folders: Vec<PathBuf>,
    lent: BTreeSet<PathBuf>,
    hint_switches: inlay_hints::HintSwitches,
    style_sections: crate::code_style_options::ServerStyles,
    analyzer_switches: power_save::AnalyzerSwitches,
    power_save: bool,
    held: BTreeMap<&'static str, BTreeSet<PathBuf>>,
    activity: BTreeMap<String, activity::Activity>,
    pushed: BTreeMap<PathBuf, Vec<Value>>,
    pushed_versions: BTreeMap<PathBuf, i64>,
    pulled: BTreeMap<PathBuf, Vec<Value>>,
    pulls: BTreeMap<PathBuf, pull::Pull>,
    alongside: BTreeMap<PathBuf, BTreeMap<String, Vec<Value>>>,
    alongside_pulls: BTreeMap<String, BTreeMap<PathBuf, pull::Pull>>,
    entries_root: Option<PathBuf>,
    settled: u64,
    progress: BTreeMap<String, BTreeMap<String, WorkProgress>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Diagnostic {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) raw: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LookupKind {
    Navigation,
    Usages,
    Documentation,
    ParameterInfo,
    Peek,
    Highlights,
    PinnedHighlights,
    ShowUsages,
}

const ASK_AGAIN: u8 = 3;

#[derive(Debug)]
pub struct Lookup {
    pub method: String,
    pub path: PathBuf,
    pub offset: usize,
    pub uri: String,
    pub version: i32,
    pub request: Option<DocumentRequest>,
    pub answer: Option<Result<Value, String>>,
    pub busy_server: bool,
}

impl Lookup {
    pub fn poll(&mut self, wait: Duration) -> bool {
        if self.answer.is_some() {
            return true;
        }
        let Some(request) = &self.request else { return false };
        let answer = if wait.is_zero() {
            request.pending.try_answer()
        } else {
            match request.pending.wait(wait.min(MAX_WAIT)) {
                Err(LspError::Timeout) => None,
                other => Some(other),
            }
        };
        let Some(answer) = answer else { return false };
        self.busy_server = matches!(&answer, Err(LspError::Server { code, .. })
            if *code == zen_lsp::connection::CONTENT_MODIFIED || *code == zen_lsp::connection::SERVER_CANCELLED);
        self.answer = Some(answer.map_err(|error| error.to_string()));
        self.request = None;
        true
    }

    pub fn is_pending(&self) -> bool {
        self.answer.is_none()
    }
}

impl Languages {
    pub fn set_specs(&mut self, specs: Vec<ServerSpec>) {
        self.specs = specs;
        self.entries_generation = None;
        self.refresh_entries();
    }

    pub(crate) fn set_root(&mut self, root: Option<&Path>) {
        if self.entries_root.as_deref() != root {
            self.entries_root = root.map(Path::to_owned);
            if self.entries.iter().any(|entry| entry.declaration.from_project || entry.declaration.when.is_some()) {
                self.entries_generation = None;
            }
        }
    }

    fn refresh_entries(&mut self) {
        let generation = self.registry.generation();
        if self.entries_generation == Some(generation) {
            return;
        }
        let mut entries: Vec<Entry> = self.specs.iter().map(Entry::from_spec).collect();
        for declaration in self.registry.declarations() {
            if !entries.iter().any(|entry| entry.id() == declaration.id) {
                entries.push(Entry::declared(declaration, &self.registry, self.entries_root.as_deref()));
            }
        }
        self.entries = entries;
        self.entries_generation = Some(self.registry.generation());
    }

    fn set_registry(&mut self, registry: servers::LanguageServers) {
        if !self.registry.same(&registry) {
            self.entries_generation = None;
        }
        self.registry = registry;
        self.refresh_entries();
    }

    pub fn set_waker(&mut self, waker: zen_lsp::connection::Waker) {
        for server in self.servers.values_mut() {
            if let Server::Running { client, .. } = server {
                client.set_waker(Arc::clone(&waker));
            }
        }
        self.waker = Some(waker);
    }

    pub fn settled(&self) -> u64 {
        self.settled
    }

    pub(crate) fn settle_ms(&self, path: &Path) -> u64 {
        self.spec_for(path).map_or(0, |entry| entry.declaration.diagnostics_settle_ms)
    }

    pub fn problems(&self, path: &Path) -> &[Problem] {
        self.problems.get(path).map_or(&[], Vec::as_slice)
    }

    pub fn analysis(&self, path: &Path, document_version: Option<u64>) -> Option<Reason> {
        if !self.pushed.contains_key(path) && !self.pulled.contains_key(path) && !self.alongside.contains_key(path) {
            return Some(Reason::NotChecked);
        }
        let spec = self.spec_for(path)?;
        let Some(Server::Running { client, files }) = self.servers.get(spec.id()) else { return None };
        let synced = files.get(path)?;
        let unsent = document_version.is_some_and(|version| version != synced.version);
        let older = self
            .pushed_versions
            .get(path)
            .zip(client.version(&synced.uri))
            .is_some_and(|(published, current)| *published < i64::from(current))
            && (synced.changed.elapsed() < QUIET_CHECK
                || self.progress.get(spec.id()).is_some_and(|work| !work.is_empty()));
        let unpulled = pull::supported(client) && !pull::answered(&self.pulls, path, client.version(&synced.uri));
        (unsent || older || unpulled).then_some(Reason::Checking)
    }

    #[cfg(test)]
    pub(crate) fn declare_virtual_documents_for_test(&mut self, declared: &servers::VirtualDocuments) {
        for entry in &mut self.entries {
            entry.declaration.virtual_documents = Some(declared.clone());
        }
    }

    #[cfg(test)]
    pub(crate) fn publish_for_test(&mut self, path: &Path, text: &str, diagnostics: &[Value]) {
        let problems = diagnostics.iter().filter_map(|diagnostic| problem(text, diagnostic)).collect();
        self.problems.insert(path.to_owned(), problems);
    }

    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    pub(crate) fn server_languages(&mut self) -> Vec<String> {
        self.refresh_entries();
        let mut languages: Vec<String> = Vec::new();
        for entry in self.entries.iter().filter(|entry| entry.available && entry.enabled) {
            for language in &entry.declaration.languages {
                if !languages.contains(language) {
                    languages.push(language.clone());
                }
            }
        }
        languages
    }

    pub(crate) fn language_of(&mut self, path: &Path) -> Option<String> {
        self.refresh_entries();
        self.spec_for(path).map(|entry| entry.language_for(path))
    }

    pub(crate) fn spec_for(&self, path: &Path) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.available && entry.enabled && !entry.declaration.alongside && entry.serves(path))
    }

    pub(crate) fn alongside_for(&self, path: &Path) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.available && entry.enabled && entry.declaration.alongside && entry.serves(path))
            .collect()
    }

    pub(crate) fn client_of(&mut self, id: &str) -> Option<&mut Client> {
        match self.servers.get_mut(id)? {
            Server::Running { client, .. } => Some(client),
            _ => None,
        }
    }

    pub(crate) fn alongside_running(&self, path: &Path) -> Vec<String> {
        self.alongside_for(path)
            .into_iter()
            .filter(|entry| {
                matches!(self.servers.get(entry.id()), Some(Server::Running { client, files })
                    if client.is_running() && files.contains_key(path))
            })
            .map(|entry| entry.id().to_owned())
            .collect()
    }

    pub(crate) fn request_to(
        &mut self,
        id: &str,
        path: &Path,
        method: &str,
        params: impl FnOnce(&str, &str) -> Value,
    ) -> Result<DocumentRequest, String> {
        let Some(Server::Running { client, files }) = self.servers.get_mut(id) else {
            return Err(t!("workbench-language-not-running"));
        };
        let synced = files.get(path).ok_or_else(|| t!("workbench-language-not-running"))?;
        let params = params(&synced.uri, &synced.text);
        client.document_request(&synced.uri, method, params).map_err(|error| error.to_string())
    }

    pub(crate) fn virtual_documents(&self, path: &Path) -> Option<servers::VirtualDocuments> {
        self.spec_for(path)?.declaration.virtual_documents.clone()
    }

    pub(crate) fn request_contents(&mut self, path: &Path, uri: &str) -> Result<zen_lsp::Pending, String> {
        let declared = self.virtual_documents(path).filter(|declared| declared.serves(uri));
        let declared = declared.ok_or_else(|| t!("workbench-library-no-contents", uri = uri))?;
        let client = self.client_for(path).ok_or_else(|| t!("workbench-language-not-running"))?;
        client.request(&declared.request, json!({ "uri": uri })).map_err(|error| error.to_string())
    }

    pub(crate) fn restart_schema_servers(&mut self) {
        let running: Vec<String> = self
            .entries
            .iter()
            .filter(|entry| entry.declaration.schema_support.is_some() && self.servers.contains_key(entry.id()))
            .map(|entry| entry.id().to_owned())
            .collect();
        for id in running {
            self.restart_server(&id);
        }
    }

    pub fn restart(&mut self, language: Option<&str>) -> Vec<String> {
        self.refresh_entries();
        self.symbols = None;
        let matches = |entry: &Entry| {
            language.is_none_or(|language| {
                entry.id() == language || entry.declaration.languages.iter().any(|known| known == language)
            })
        };
        let stopped: Vec<String> = self
            .entries
            .iter()
            .filter(|entry| matches(entry) && self.servers.contains_key(entry.id()))
            .map(|entry| entry.id().to_owned())
            .collect();
        for id in &stopped {
            self.restart_server(id);
        }
        let entries: Vec<&Entry> =
            self.entries.iter().filter(|entry| stopped.iter().any(|id| id == entry.id())).collect();
        self.problems.retain(|path, _| !entries.iter().any(|entry| entry.serves(path)));
        self.diagnostics.retain(|path, _| !entries.iter().any(|entry| entry.serves(path)));
        self.pushed.retain(|path, _| !entries.iter().any(|entry| entry.serves(path)));
        self.pushed_versions.retain(|path, _| !entries.iter().any(|entry| entry.serves(path)));
        self.pulled.retain(|path, _| !entries.iter().any(|entry| entry.serves(path)));
        self.pulls.retain(|path, _| !entries.iter().any(|entry| entry.serves(path)));
        for id in &stopped {
            self.alongside_pulls.remove(id);
            for found in self.alongside.values_mut() {
                found.remove(id);
            }
        }
        self.alongside.retain(|_, found| !found.is_empty());
        stopped
    }

    pub fn serves(&self, path: &Path) -> bool {
        self.spec_for(path).is_some()
    }

    pub(crate) fn server_for(&mut self, path: &Path) -> Option<(&mut Client, &Synced)> {
        let id = self.spec_for(path)?.id().to_owned();
        match self.servers.get_mut(&id)? {
            Server::Running { client, files } => Some((client, files.get(path)?)),
            _ => None,
        }
    }

    pub fn build_changed(&mut self, paths: &[PathBuf]) -> bool {
        self.registry.forget_java_launches();
        let mut told = false;
        for entry in &self.entries {
            let changes: Vec<(String, FileChangeType)> = paths
                .iter()
                .filter(|path| path.file_name().is_some_and(|name| entry.declaration.hears_of(&name.to_string_lossy())))
                .map(|path| (zen_lsp::position::file_uri(path), FileChangeType::CHANGED))
                .collect();
            if changes.is_empty() {
                continue;
            }
            if let Some(Server::Running { client, .. }) = self.servers.get_mut(entry.id())
                && client.is_running()
                && client.did_change_watched_files(&changes).is_ok()
            {
                told = true;
            }
        }
        told
    }

    pub(crate) fn import_build(&mut self, workbench: &Workbench, files: &[PathBuf]) -> BuildImport {
        self.set_root(workbench.project());
        self.refresh_entries();
        let told = self.build_changed(files);
        let server = self.entries.iter().find(|entry| entry.declaration.languages.iter().any(|name| name == "java"));
        let Some(entry) = server.cloned() else {
            return if told { BuildImport::Told } else { BuildImport::NoServer };
        };
        let (id, name) = (entry.id().to_owned(), entry.declaration.name.clone());
        if !entry.enabled {
            return BuildImport::Disabled { name };
        }
        if !entry.available {
            let installable = entry.declared && self.registry.install_check(&id).is_ok();
            return BuildImport::Missing { id, name, installable };
        }
        match self.servers.get(&id) {
            Some(Server::Stopped) => return BuildImport::Stopped { id, name },
            Some(Server::Failed { .. }) => {
                self.servers.remove(&id);
            }
            _ => {}
        }
        self.keep_running.insert(id.clone());
        self.sync(workbench);
        BuildImport::Asked { server: id }
    }

    pub(crate) fn import_progress(&self, id: &str, asked: Instant) -> ImportProgress {
        let not_running = || ImportProgress::Failed { reason: t!("workbench-language-not-running"), java: None };
        match self.servers.get(id) {
            Some(Server::Starting(_)) => ImportProgress::Running,
            Some(Server::Failed { reason, .. }) => {
                let root = self.entries_root.as_deref();
                let java = self.registry.declaration(id).and_then(|server| server.java);
                ImportProgress::Failed {
                    reason: reason.text(),
                    java: java.filter(|version| self.registry.java_home(root, *version).is_none()),
                }
            }
            Some(Server::Running { client, .. }) if client.is_running() => {
                let activity = self.activity.get(id);
                match activity.and_then(activity::Activity::import) {
                    Some(activity::Import::Failed(reason)) => {
                        ImportProgress::Failed { reason: reason.clone(), java: None }
                    }
                    Some(activity::Import::Done) if !activity.is_some_and(activity::Activity::busy) => {
                        ImportProgress::Done
                    }
                    None if !activity.is_some_and(activity::Activity::busy) && asked.elapsed() >= IMPORT_SETTLES => {
                        ImportProgress::Done
                    }
                    _ => ImportProgress::Running,
                }
            }
            _ => not_running(),
        }
    }

    pub fn files_changed(&mut self, paths: &[PathBuf]) {
        if paths.iter().any(|path| crate::java::processing::is_build_file(path)) {
            self.registry.forget_java_launches();
        }
        for entry in &self.entries {
            let changes: Vec<(String, FileChangeType)> = paths
                .iter()
                .filter(|path| path.file_name().is_some_and(|name| entry.declaration.hears_of(&name.to_string_lossy())))
                .map(|path| {
                    let change = if path.exists() { FileChangeType::CHANGED } else { FileChangeType::DELETED };
                    (zen_lsp::position::file_uri(path), change)
                })
                .collect();
            if changes.is_empty() {
                continue;
            }
            if let Some(Server::Running { client, .. }) = self.servers.get_mut(entry.id()) {
                let _ = client.did_change_watched_files(&changes);
            }
        }
    }

    pub(crate) fn report_files(&mut self, changes: &[(PathBuf, FileChangeType)]) {
        for (path, change) in changes {
            let told = [(zen_lsp::position::file_uri(path), *change)];
            match self.spec_for(path).map(|spec| spec.id().to_owned()) {
                Some(id) => {
                    if let Some(client) = self.client_of(&id) {
                        let _ = client.did_change_watched_files(&told);
                    }
                }
                None => {
                    for server in self.servers.values_mut() {
                        if let Server::Running { client, .. } = server {
                            let _ = client.did_change_watched_files(&told);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn running_servers(&self) -> Vec<String> {
        self.servers
            .iter()
            .filter(|(_, server)| matches!(server, Server::Running { client, .. } if client.is_running()))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(crate) fn client_for(&mut self, path: &Path) -> Option<&mut Client> {
        let id = self.spec_for(path)?.id().to_owned();
        match self.servers.get_mut(&id)? {
            Server::Running { client, .. } => Some(client),
            _ => None,
        }
    }

    pub fn server_version(&self, path: &Path) -> Option<i32> {
        let spec = self.spec_for(path)?;
        match self.servers.get(spec.id())? {
            Server::Running { client, files } => client.version(&files.get(path)?.uri),
            _ => None,
        }
    }

    pub fn synced_text(&self, path: &Path) -> Option<&str> {
        self.servers.values().find_map(|server| match server {
            Server::Running { files, .. } => files.get(path).map(|synced| synced.text.as_str()),
            _ => None,
        })
    }

    pub fn start_lookup(
        &mut self,
        workbench: &Workbench,
        kind: LookupKind,
        path: &Path,
        offset: usize,
        method: &str,
        extra: Value,
    ) -> Result<&mut Lookup, String> {
        self.end_lookup(kind);
        let extra_kept = extra.clone();
        let request = request_at(self, workbench, path, offset, method, extra)?;
        let lookup = Lookup {
            method: method.to_owned(),
            path: path.to_owned(),
            offset,
            uri: request.uri.clone(),
            version: request.version,
            request: Some(request),
            answer: None,
            busy_server: false,
        };
        self.asked.insert(kind, (extra_kept, 0));
        Ok(self.lookups.entry(kind).or_insert(lookup))
    }

    pub(crate) fn ask_again(&mut self, workbench: &Workbench, kind: LookupKind) -> bool {
        let Some(lookup) = self.lookups.get(&kind).filter(|lookup| lookup.busy_server) else { return false };
        let Some((extra, tries)) = self.asked.get(&kind).cloned().filter(|(_, tries)| *tries < ASK_AGAIN) else {
            return false;
        };
        let (path, offset, method) = (lookup.path.clone(), lookup.offset, lookup.method.clone());
        if self.start_lookup(workbench, kind, &path, offset, &method, extra.clone()).is_err() {
            return false;
        }
        self.asked.insert(kind, (extra, tries + 1));
        true
    }

    pub fn lookup(&self, kind: LookupKind) -> Option<&Lookup> {
        self.lookups.get(&kind)
    }

    pub fn lookup_mut(&mut self, kind: LookupKind) -> Option<&mut Lookup> {
        self.lookups.get_mut(&kind)
    }

    pub fn end_lookup(&mut self, kind: LookupKind) -> Option<Lookup> {
        let mut lookup = self.lookups.remove(&kind)?;
        if let Some(request) = lookup.request.take()
            && let Some((client, _)) = self.server_for(&lookup.path)
        {
            client.cancel(&request.pending);
        }
        Some(lookup)
    }

    pub fn is_outdated(&self, kind: LookupKind) -> bool {
        let Some(lookup) = self.lookups.get(&kind) else { return false };
        let Some(spec) = self.spec_for(&lookup.path) else { return true };
        match self.servers.get(spec.id()) {
            Some(Server::Running { client, .. }) => client.version(&lookup.uri) != Some(lookup.version),
            _ => true,
        }
    }

    pub(crate) fn hold(&mut self, owner: &'static str, paths: BTreeSet<PathBuf>) {
        self.held.insert(owner, paths);
    }

    pub(crate) fn diagnostics(&self, path: &Path) -> &[Diagnostic] {
        self.diagnostics.get(path).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn take_server_requests(&mut self, server: &str) -> Vec<zen_lsp::ServerRequest> {
        let (taken, kept) = std::mem::take(&mut self.server_requests).into_iter().partition(|(id, _)| id == server);
        self.server_requests = kept;
        taken.into_iter().map(|(_, request)| request).collect()
    }

    pub(crate) fn take_rename_at(&mut self) -> Option<Value> {
        self.rename_at.take()
    }

    pub(crate) fn set_executing(&mut self, server: Option<&str>) {
        self.executing = server.map(str::to_owned);
    }

    pub(crate) fn refuse_server_requests(&mut self, server: &str) {
        if let Some(Server::Running { client, .. }) = self.servers.get(server) {
            let unread = client.take_requests();
            self.server_requests.extend(unread.into_iter().map(|request| (server.to_owned(), request)));
        }
        for request in self.take_server_requests(server) {
            let reason = t!("workbench-intentions-unasked-edit");
            self.respond(server, &request.id, json!({ "applied": false, "failureReason": reason }));
        }
    }

    pub(crate) fn respond(&mut self, server: &str, id: &Value, result: Value) {
        if let Some(Server::Running { client, .. }) = self.servers.get_mut(server) {
            let _ = client.respond(id, result);
        }
    }

    pub(crate) fn suppression_for(
        &self,
        path: &Path,
        source: &str,
    ) -> Option<zen_lsp::servers::SuppressionDeclaration> {
        self.entries.iter().filter(|entry| entry.serves(path)).find_map(|entry| {
            let suppression = entry.declaration.suppression.as_ref()?;
            let applies = suppression.sources.is_empty() || suppression.sources.iter().any(|known| known == source);
            applies.then(|| suppression.clone())
        })
    }

    pub(crate) fn declaration_of(&self, id: &str) -> Option<&servers::ServerDeclaration> {
        self.entries.iter().find(|entry| entry.id() == id).map(|entry| &entry.declaration)
    }

    #[cfg(test)]
    pub(crate) fn declare_generate_for_test(&mut self, declared: &zen_lsp::GenerateDeclaration) {
        for entry in &mut self.entries {
            entry.declaration.generate = Some(declared.clone());
        }
    }

    pub(crate) fn server_id(&self, path: &Path) -> Option<String> {
        self.spec_for(path).map(|spec| spec.id().to_owned())
    }

    pub fn absence_for(&mut self, path: &Path) -> Option<ServerAbsence> {
        self.refresh_entries();
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.enabled && !entry.declaration.alongside && entry.serves(path))
            .cloned()
            .or_else(|| {
                self.entries.iter().find(|entry| !entry.declaration.alongside && entry.serves(path)).cloned()
            })?;
        self.absence_of(&entry)
    }

    pub fn missing_project_server(&mut self, root: &Path, symbols: bool) -> Option<ServerAbsence> {
        self.refresh_entries();
        let project = self.project_files(root)?;
        let missing: Vec<Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.declared && entry.enabled && !entry.available)
            .filter(|entry| !symbols || entry.declaration.workspace_symbols)
            .filter(|entry| self.opened.contains(entry.id()) || project.has_files_of(&entry.declaration))
            .cloned()
            .collect();
        let mut absences: Vec<(bool, ServerAbsence)> = missing
            .iter()
            .filter_map(|entry| Some((!self.opened.contains(entry.id()), self.absence_of(entry)?)))
            .collect();
        absences.sort_by_key(|(not_opened, _)| *not_opened);
        absences.into_iter().map(|(_, absence)| absence).next()
    }

    pub fn project_absence(&mut self, root: &Path, symbols: bool) -> Option<ServerAbsence> {
        self.refresh_entries();
        let project = self.project_files(root)?;
        let needed: Vec<Entry> = self
            .entries
            .iter()
            .filter(|entry| {
                self.opened.contains(entry.id()) || !entry.declared || project.has_files_of(&entry.declaration)
            })
            .filter(|entry| !symbols || entry.declaration.workspace_symbols)
            .cloned()
            .collect();
        let mut absences: Vec<(bool, ServerAbsence)> = Vec::new();
        for entry in needed {
            let startable = entry.available && entry.enabled && !self.servers.contains_key(entry.id());
            match self.absence_of(&entry) {
                None => return None,
                Some(_) if startable => return None,
                Some(absence) if absence.reason == Reason::Starting => return None,
                Some(absence) => absences.push((!self.opened.contains(entry.id()), absence)),
            }
        }
        absences.sort_by_key(|(not_opened, absence)| (*not_opened, absence.reason != Reason::ServerMissing));
        absences.into_iter().next().map(|(_, absence)| absence)
    }

    pub fn lookup_error(&mut self, path: &Path, message: String) -> CommandError {
        match self.absence_for(path) {
            Some(absence) => absence.error(),
            None => CommandError::new(message),
        }
    }

    fn absence_of(&self, entry: &Entry) -> Option<ServerAbsence> {
        let id = entry.id();
        let server = entry.declaration.name.as_str();
        let absence = |reason, message| Some(ServerAbsence { reason, server: id.to_owned(), install: None, message });
        if !entry.enabled {
            return absence(Reason::ServerNotRunning, t!("workbench-language-server-disabled", server = server));
        }
        if !entry.available {
            return Some(ServerAbsence {
                reason: Reason::ServerMissing,
                server: id.to_owned(),
                install: entry.declared.then(|| self.install_of(entry)),
                message: t!("workbench-language-server-missing", server = server),
            });
        }
        match self.servers.get(id) {
            Some(Server::Running { client, .. }) if client.is_running() => None,
            Some(Server::Starting(_)) => {
                absence(Reason::Starting, t!("workbench-language-server-starting", server = server))
            }
            Some(Server::Running { .. }) => absence(
                Reason::ServerNotRunning,
                t!("workbench-language-server-failed", server = server, reason = t!("workbench-language-not-running")),
            ),
            Some(Server::Failed { reason, .. }) => absence(
                Reason::ServerNotRunning,
                t!("workbench-language-server-failed", server = server, reason = reason.text()),
            ),
            Some(Server::Stopped) => {
                absence(Reason::ServerNotRunning, t!("workbench-language-server-stopped", server = server))
            }
            None if self.keep_running.contains(id) => {
                absence(Reason::Starting, t!("workbench-language-server-starting", server = server))
            }
            None if entry.declared && !self.trusted_root() => {
                absence(Reason::NotTrusted, t!("workbench-language-server-untrusted", server = server))
            }
            None => absence(Reason::ServerNotRunning, t!("workbench-language-server-not-started", server = server)),
        }
    }

    fn trusted_root(&self) -> bool {
        self.entries_root
            .as_deref()
            .is_none_or(|root| crate::trust::is_trusted(&crate::build_tools::BuildEnv::global(), root))
    }

    fn install_of(&self, entry: &Entry) -> ServerInstall {
        match self.registry.install_line(entry.id()) {
            Ok(runs) => ServerInstall {
                command: Some(servers::INSTALL_COMMAND.to_owned()),
                params: Some(json!({ "id": entry.id() })),
                runs: Some(runs),
                hint: None,
            },
            Err(why) => ServerInstall {
                command: None,
                params: None,
                runs: None,
                hint: Some(servers::not_installable(&entry.declaration, &why)),
            },
        }
    }

    fn refuse_requests(&mut self) {
        let executing = self.executing.clone();
        let (kept, refused) = std::mem::take(&mut self.server_requests)
            .into_iter()
            .partition(|(server, _)| Some(server) == executing.as_ref());
        self.server_requests = kept;
        for (server, request) in refused {
            let reason = t!("workbench-intentions-unasked-edit");
            self.respond(&server, &request.id, json!({ "applied": false, "failureReason": reason }));
        }
    }

    pub(crate) fn start_for(&mut self, path: &Path) {
        if let Some(server) = self.spec_for(path).map(|spec| spec.id().to_owned()) {
            self.keep_running.insert(server);
        }
    }

    pub(crate) fn lend(&mut self, path: &Path) -> Result<(), String> {
        let text = disk_text(path)?;
        let (server, language) =
            self.spec_for(path).map(|spec| (spec.id().to_owned(), spec.language_for(path))).unwrap_or_default();
        let Some(Server::Running { client, files }) = self.servers.get_mut(&server) else {
            self.start_for(path);
            return Err(t!("workbench-language-not-running"));
        };
        if !files.contains_key(path) {
            let uri = zen_lsp::position::file_uri(path);
            client.did_open(&uri, &language, &text).map_err(|error| error.to_string())?;
            files.insert(path.to_owned(), Synced::lent(uri, text));
        }
        self.lent.insert(path.to_owned());
        Ok(())
    }

    pub(crate) fn sync(&mut self, workbench: &Workbench) {
        self.set_root(workbench.project());
        self.refresh_entries();
        let Some(root) = workbench.project().map(Path::to_owned) else { return };
        let active = workbench.active_document().map(|document| document.path().to_owned());
        let lookups: BTreeSet<&Path> = self.lookups.values().map(|lookup| lookup.path.as_path()).collect();
        let held = &self.held;
        self.lent.retain(|path| {
            (lookups.contains(path.as_path()) || held.values().any(|paths| paths.contains(path)))
                && !workbench.documents().iter().any(|document| document.path() == path)
        });
        let types = zen_vfs::file_types::current();
        if !types.overrides().is_empty() {
            let gone: Vec<PathBuf> =
                self.problems.keys().filter(|path| self.overridden_away(&types, path)).cloned().collect();
            for path in gone {
                self.problems.remove(&path);
                self.diagnostics.remove(&path);
                self.pushed.remove(&path);
                self.pushed_versions.remove(&path);
                self.pulled.remove(&path);
                self.pulls.remove(&path);
            }
        }
        let mut wanted: BTreeMap<String, Vec<&crate::Document>> = BTreeMap::new();
        for document in workbench
            .documents()
            .iter()
            .filter(|document| document.image().is_none() && crate::virtual_documents::served(document.path()))
        {
            if let Some(entry) = self.entries.iter().find(|entry| entry.serves(document.path())) {
                self.opened.insert(entry.id().to_owned());
            }
            if let Some(spec) = self.spec_for(document.path()) {
                wanted.entry(spec.id().to_owned()).or_default().push(document);
            }
            for entry in self.alongside_for(document.path()) {
                wanted.entry(entry.id().to_owned()).or_default().push(document);
            }
        }
        let safe_mode = !crate::trust::is_trusted(&crate::build_tools::BuildEnv::global(), &root);
        for entry in self.entries.clone() {
            if !entry.enabled {
                if let Some(server) = self.servers.remove(entry.id()) {
                    std::thread::spawn(move || drop(server));
                }
                continue;
            }
            if safe_mode && entry.declared && !self.servers.contains_key(entry.id()) {
                continue;
            }
            let stamp = self.registry.plugins_stamp(entry.id());
            let before = self.plugin_stamps.insert(entry.id().to_owned(), stamp);
            if before.is_some_and(|before| before != stamp) && self.servers.contains_key(entry.id()) {
                self.restart_server(entry.id());
            }
            if matches!(self.servers.get(entry.id()), Some(Server::Running { .. } | Server::Starting(_)))
                && let Some(notice) = self.registry.java_launch_change(entry.id(), &root)
            {
                self.restart_server(entry.id());
                self.notices.push(notice);
            }
            if entry.declared
                && entry.declaration.java.is_some()
                && matches!(self.servers.get(entry.id()), Some(Server::Running { .. }))
                && let Some(notice) = self.registry.project_jdk_change(entry.id(), &root)
            {
                self.restart_server(entry.id());
                self.notices.push(notice);
            }
            let documents = wanted.remove(entry.id()).unwrap_or_default();
            if documents.is_empty() && !self.servers.contains_key(entry.id()) && !self.keep_running.contains(entry.id())
            {
                continue;
            }
            let lent = self.lent.clone();
            self.step(&entry, &root, &documents, &lent, active.as_deref());
        }
        self.refuse_requests();
        self.poll_expression(Duration::ZERO);
    }

    fn launch_key(&self, entry: &Entry) -> LaunchKey {
        if entry.declared {
            let mut args = self.registry.args(entry.id());
            if let Some(version) = entry.declaration.java {
                let home = self.registry.java_home(self.entries_root.as_deref(), version);
                args.push(format!("java:{}", home.map(|home| home.display().to_string()).unwrap_or_default()));
            }
            (self.registry.executable(entry.id()), args)
        } else {
            (None, Vec::new())
        }
    }

    fn step(
        &mut self,
        spec: &Entry,
        root: &Path,
        documents: &[&crate::Document],
        lent: &BTreeSet<PathBuf>,
        active: Option<&Path>,
    ) {
        self.take_unopened_texts();
        let language = spec.id().to_owned();
        let server = self.servers.remove(&language);
        let server = match server {
            None => {
                self.activity.insert(language.clone(), activity::Activity::default());
                Some(start(
                    spec,
                    root,
                    self.registry.log(&language),
                    (&self.hint_switches, &self.analyzer_switches),
                    &self.style_sections,
                    self.power_save,
                ))
            }
            Some(Server::Failed { launch, .. }) if !documents.is_empty() && launch != self.launch_key(spec) => {
                self.activity.insert(language.clone(), activity::Activity::default());
                Some(start(
                    spec,
                    root,
                    self.registry.log(&language),
                    (&self.hint_switches, &self.analyzer_switches),
                    &self.style_sections,
                    self.power_save,
                ))
            }
            Some(Server::Starting(receiver)) => match receiver.try_recv() {
                Ok(Ok(mut client)) => {
                    if let Some(waker) = &self.waker {
                        client.set_waker(Arc::clone(waker));
                    }
                    Some(Server::Running { client: Box::new(client), files: HashMap::new() })
                }
                Ok(Err(reason)) => {
                    self.registry.log(&language).push(reason.clone());
                    Some(Server::Failed { reason: zen_i18n::Message::plain(reason), launch: self.launch_key(spec) })
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => Some(Server::Starting(receiver)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let reason = zen_i18n::Message::new("workbench-language-not-started");
                    self.registry.log(&language).push(reason.text());
                    Some(Server::Failed { reason, launch: self.launch_key(spec) })
                }
            },
            other => other,
        };
        let Some(mut server) = server else { return };
        if let Server::Running { client, files } = &mut server {
            for document in documents {
                if let Some(folder) = self.outside_folders.iter().find(|folder| document.path().starts_with(folder)) {
                    let _ = client.add_workspace_folder(folder);
                }
            }
            sync_files(client, files, spec, documents, lent);
            let alongside = spec.declaration.alongside;
            if !alongside {
                power_save::configure(
                    client,
                    spec,
                    &self.hint_switches,
                    &self.style_sections,
                    &self.analyzer_switches,
                    self.power_save,
                );
                for synced in files.values_mut() {
                    semantic::step(client, synced);
                    inlay_hints::step(client, synced, &self.hint_switches, true);
                }
                if let Some(synced) = active.and_then(|path| files.get_mut(path)) {
                    structure::step(client, synced);
                }
            }
            let pulls = pull::supported(client);
            if pulls && !self.power_save {
                let own =
                    if alongside { self.alongside_pulls.entry(language.clone()).or_default() } else { &mut self.pulls };
                for (path, items) in pull::step(own, client, files) {
                    if alongside {
                        self.alongside.entry(path.clone()).or_default().insert(language.clone(), items);
                    } else {
                        self.pulled.insert(path.clone(), items);
                    }
                    self.rebuild(&path, files);
                }
            }
            let was_busy = self.activity.get(&language).is_some_and(activity::Activity::busy);
            for event in client.poll() {
                match event {
                    Event::Notification { method, params }
                        if method == "textDocument/publishDiagnostics"
                            && params["uri"].as_str().is_some_and(completion::is_hidden_copy) => {}
                    Event::Notification { method, params }
                        if method == "textDocument/publishDiagnostics"
                            && params["uri"].as_str().is_some_and(left_behind) => {}
                    Event::Notification { method, params } if method == "textDocument/publishDiagnostics" => {
                        if alongside {
                            self.publish_alongside(&language, &params, files);
                        } else {
                            self.publish(&params, files);
                        }
                    }
                    Event::Notification { method, params } if method == zen_lsp::connection::RENAME_AT => {
                        self.rename_at = Some(params);
                    }
                    Event::Notification { method, .. } if method == zen_lsp::connection::INLAY_HINT_REFRESH => {
                        files.values_mut().for_each(inlay_hints::refresh);
                    }
                    Event::Notification { method, .. } if method == zen_lsp::connection::DIAGNOSTIC_REFRESH => {
                        if pulls && alongside {
                            pull::refresh(self.alongside_pulls.entry(language.clone()).or_default());
                        } else if pulls {
                            pull::refresh(&mut self.pulls);
                        } else {
                            for synced in files.values() {
                                let _ = client.reopen(&synced.uri);
                            }
                        }
                    }
                    Event::Notification { method, params } => {
                        if method == "$/progress" {
                            note_progress(self.progress.entry(language.clone()).or_default(), &language, &params);
                        }
                        self.activity.entry(language.clone()).or_default().notification(&method, &params);
                    }
                    Event::Restarted { .. } => {
                        self.activity.insert(language.clone(), activity::Activity::default());
                        self.progress.remove(&language);
                        self.notices.push(t!("workbench-language-restarted", server = language.as_str()))
                    }
                    Event::GaveUp { reason } => {
                        self.notices.push(t!(
                            "workbench-language-stopped",
                            server = language.as_str(),
                            reason = reason.as_str()
                        ));
                    }
                }
            }
            if was_busy && !self.activity.get(&language).is_some_and(activity::Activity::busy) {
                self.settled += 1;
            }
            self.server_requests.extend(client.take_requests().into_iter().map(|request| (language.clone(), request)));
            if !client.is_running() && client.capabilities().is_none() {
                let reason = zen_i18n::Message::new("workbench-language-not-running");
                server = Server::Failed { reason, launch: self.launch_key(spec) };
            }
        }
        if !matches!(server, Server::Running { .. }) {
            self.progress.remove(&language);
        }
        self.servers.insert(language, server);
    }

    fn overridden_away(&self, types: &zen_vfs::file_types::Registry, path: &Path) -> bool {
        types.overrides().contains_key(path) && !self.entries.iter().any(|entry| entry.serves(path))
    }

    fn publish(&mut self, params: &Value, files: &HashMap<PathBuf, Synced>) {
        let Some(path) = params["uri"].as_str().and_then(zen_lsp::position::uri_path) else { return };
        if self.overridden_away(&zen_vfs::file_types::current(), &path) {
            return;
        }
        let published = params["diagnostics"].as_array().cloned().unwrap_or_default();
        self.pushed.insert(path.clone(), published);
        match params["version"].as_i64() {
            Some(version) => self.pushed_versions.insert(path.clone(), version),
            None => self.pushed_versions.remove(&path),
        };
        self.rebuild(&path, files);
    }

    fn publish_alongside(&mut self, server: &str, params: &Value, files: &HashMap<PathBuf, Synced>) {
        let Some(path) = params["uri"].as_str().and_then(zen_lsp::position::uri_path) else { return };
        let published = params["diagnostics"].as_array().cloned().unwrap_or_default();
        self.alongside.entry(path.clone()).or_default().insert(server.to_owned(), published);
        self.rebuild(&path, files);
    }

    fn rebuild(&mut self, path: &Path, files: &HashMap<PathBuf, Synced>) {
        let raw = self.raw_diagnostics(path);
        match files.get(path) {
            Some(synced) => self.set_problems(path.to_owned(), &raw, synced.text.as_str()),
            None if raw.is_empty() => self.set_problems(path.to_owned(), &raw, ""),
            None => {
                if let Some(text) = self.unopened.read(path, self.waker.as_ref()) {
                    self.set_problems(path.to_owned(), &raw, &text);
                }
            }
        }
    }

    fn take_unopened_texts(&mut self) {
        for (path, text) in self.unopened.take_read() {
            let raw = self.raw_diagnostics(&path);
            self.set_problems(path, &raw, &text);
        }
    }

    fn raw_diagnostics(&self, path: &Path) -> Vec<Value> {
        let mut raw: Vec<Value> = self.pushed.get(path).cloned().unwrap_or_default();
        for pulled in self.pulled.get(path).into_iter().flatten() {
            if !raw.contains(pulled) {
                raw.push(pulled.clone());
            }
        }
        for found in self.alongside.get(path).into_iter().flat_map(BTreeMap::values).flatten() {
            if !raw.contains(found) {
                raw.push(found.clone());
            }
        }
        raw
    }

    fn set_problems(&mut self, path: PathBuf, raw: &[Value], text: &str) {
        let found: Vec<(Problem, &Value)> =
            raw.iter().filter_map(|diagnostic| Some((problem(text, diagnostic)?, diagnostic))).collect();
        let diagnostics: Vec<Diagnostic> = found
            .iter()
            .map(|(problem, raw)| Diagnostic { start: problem.start, end: problem.end, raw: (*raw).clone() })
            .collect();
        let mut problems: Vec<Problem> = found.into_iter().map(|(problem, _)| problem).collect();
        problems.sort_by_key(|problem| (problem.start, problem.severity));
        if problems.is_empty() {
            self.problems.remove(&path);
            self.diagnostics.remove(&path);
        } else {
            self.problems.insert(path.clone(), problems);
            self.diagnostics.insert(path, diagnostics);
        }
    }

    pub(crate) fn works(&self) -> Vec<(String, WorkProgress)> {
        self.progress
            .iter()
            .filter(|(id, _)| matches!(self.servers.get(*id), Some(Server::Running { .. })))
            .flat_map(|(_, works)| works.iter().map(|(token, work)| (token.clone(), work.clone())))
            .collect()
    }

    pub(crate) fn work_canceller(&self, server: &str, token: &str) -> Option<impl Fn() + Send + Sync + 'static> {
        let Some(Server::Running { client, .. }) = self.servers.get(server) else { return None };
        client.work_canceller(serde_json::from_str(token).ok()?)
    }

    pub fn work_progress(&self) -> Option<WorkProgress> {
        self.progress
            .iter()
            .filter(|(id, _)| matches!(self.servers.get(*id), Some(Server::Running { .. })))
            .find_map(|(_, works)| works.values().next().cloned())
    }

    pub fn busy(&self) -> Option<(Reason, Option<WorkProgress>)> {
        if self.servers.values().any(|server| matches!(server, Server::Starting(_))) {
            return Some((Reason::Starting, None));
        }
        self.work_progress().map(|progress| (Reason::Indexing, Some(progress)))
    }

    fn status(&mut self, root: Option<&Path>, install: bool) -> BTreeMap<String, ServerStatus> {
        let mut status = self.started_status();
        self.refresh_entries();
        let project = root.and_then(|root| self.project_files(root));
        let missing: Vec<Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.declared && entry.enabled && !entry.available)
            .filter(|entry| {
                self.opened.contains(entry.id())
                    || project.as_ref().is_some_and(|files| files.has_files_of(&entry.declaration))
            })
            .cloned()
            .collect();
        for entry in missing {
            let message = t!("workbench-language-server-missing", server = entry.declaration.name.as_str());
            let install = install.then(|| self.install_of(&entry));
            status.entry(entry.id().to_owned()).or_insert(ServerStatus::Missing { install, message });
        }
        status
    }

    fn started_status(&self) -> BTreeMap<String, ServerStatus> {
        self.servers
            .iter()
            .map(|(language, server)| {
                let status = match server {
                    Server::Starting(_) => ServerStatus::Starting,
                    Server::Running { client, .. } if client.is_running() => ServerStatus::Running,
                    Server::Running { .. } => ServerStatus::Failed { reason: t!("workbench-language-not-running") },
                    Server::Failed { reason, .. } => ServerStatus::Failed { reason: reason.text() },
                    Server::Stopped => ServerStatus::Stopped,
                };
                (language.clone(), status)
            })
            .collect()
    }
}

fn note_progress(works: &mut BTreeMap<String, WorkProgress>, server: &str, params: &Value) {
    let token = params["token"].to_string();
    let value = &params["value"];
    let text = |field: &str| value[field].as_str().filter(|text| !text.is_empty()).map(str::to_owned);
    let percentage = value["percentage"].as_u64().map(|percentage| percentage.min(100) as u32);
    match value["kind"].as_str() {
        Some("begin") => {
            let title = text("title").unwrap_or_default();
            let server = server.to_owned();
            let cancellable = value["cancellable"].as_bool().unwrap_or(false);
            works.insert(token, WorkProgress { server, title, message: text("message"), percentage, cancellable });
        }
        Some("report") => {
            if let Some(work) = works.get_mut(&token) {
                if let Some(message) = text("message") {
                    work.message = Some(message);
                }
                if percentage.is_some() {
                    work.percentage = percentage;
                }
                if let Some(cancellable) = value["cancellable"].as_bool() {
                    work.cancellable = cancellable;
                }
            }
        }
        Some("end") => {
            works.remove(&token);
        }
        _ => {}
    }
}

fn start(
    spec: &Entry,
    root: &Path,
    log: servers::ServerLog,
    (switches, analyzers): (&inlay_hints::HintSwitches, &power_save::AnalyzerSwitches),
    style: &crate::code_style_options::ServerStyles,
    power_save: bool,
) -> Server {
    let (sender, receiver) = std::sync::mpsc::channel();
    let style = style.clone();
    let launch = Arc::clone(&spec.launch);
    let settings = Arc::clone(&spec.options);
    let entry = spec.clone();
    let switches = switches.clone();
    let analyzers = analyzers.clone();
    let root = root.to_owned();
    std::thread::spawn(move || {
        let base = settings(&root);
        let options = power_save::options(&entry, &base, &switches, &analyzers, power_save);
        let mut configuration: BTreeMap<String, Value> = settings_for(&entry.declaration.settings, &root);
        configuration.extend(power_save::sections(&entry, &base, &switches, &style, &analyzers, power_save));
        let (content, after) = crate::schemas::serve(&entry.declaration, &mut configuration, Some(&root));
        let started = launch(&root).and_then(|launcher| {
            Client::start_serving(launcher, &root, options, Some(log), configuration, content, after)
                .map_err(|error| error.to_string())
        });
        let _ = sender.send(started);
    });
    Server::Starting(receiver)
}

pub(crate) fn settings_for(settings: &BTreeMap<String, Value>, root: &Path) -> BTreeMap<String, Value> {
    fn expand(value: &Value, root: &Path) -> Value {
        match value {
            Value::String(text) => {
                let name = root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                Value::String(
                    text.replace("${projectRoot}", &root.to_string_lossy())
                        .replace("${projectUri}", &zen_lsp::position::file_uri(root))
                        .replace("${projectName}", &name),
                )
            }
            Value::Array(items) => Value::Array(items.iter().map(|item| expand(item, root)).collect()),
            Value::Object(fields) => {
                Value::Object(fields.iter().map(|(key, value)| (key.clone(), expand(value, root))).collect())
            }
            _ => value.clone(),
        }
    }
    settings.iter().map(|(section, value)| (section.clone(), expand(value, root))).collect()
}

fn sync_files(
    client: &mut Client,
    files: &mut HashMap<PathBuf, Synced>,
    spec: &Entry,
    documents: &[&crate::Document],
    lent: &BTreeSet<PathBuf>,
) {
    for document in documents {
        let path = document.path().to_owned();
        let uri = zen_lsp::position::file_uri(&path);
        if files.get(&path).is_some_and(|synced| synced.uri != uri)
            && let Some(moved) = files.remove(&path)
        {
            let _ = client.did_close(&moved.uri);
        }
        match files.get_mut(&path) {
            None => {
                if client.did_open(&uri, &spec.language_for(&path), document.text()).is_ok() {
                    files.insert(
                        path,
                        Synced {
                            uri,
                            version: document.version(),
                            text: document.text().to_owned(),
                            dirty: document.is_dirty(),
                            saves: document.save_count(),
                            tokens: semantic::FileTokens::default(),
                            symbols: structure::FileSymbols::default(),
                            hints: inlay_hints::FileHints::default(),
                            changed: Instant::now(),
                        },
                    );
                }
            }
            Some(synced) => {
                if synced.version != document.version() {
                    let edit = difference(&synced.text, document.text());
                    semantic::follow_edit(&mut synced.tokens, &synced.text, &edit);
                    inlay_hints::follow_edit(&mut synced.hints, &synced.text, &edit);
                    if client.did_change(&synced.uri, &[edit]).is_ok() {
                        synced.text = document.text().to_owned();
                        synced.version = document.version();
                        synced.changed = Instant::now();
                    }
                }
                if (synced.dirty && !document.is_dirty()) || synced.saves != document.save_count() {
                    let _ = client.did_save(&synced.uri);
                }
                synced.dirty = document.is_dirty();
                synced.saves = document.save_count();
            }
        }
    }
    let open: Vec<&Path> = documents.iter().map(|document| document.path()).collect();
    for path in lent.iter().filter(|path| !open.contains(&path.as_path())) {
        if let Some(synced) = files.get_mut(path)
            && let Ok(text) = zen_vfs::read_text(path)
            && text != synced.text
            && client.did_change(&synced.uri, &[difference(&synced.text, &text)]).is_ok()
        {
            synced.text = text;
            synced.changed = Instant::now();
        }
    }
    files.retain(|path, synced| {
        let keep = open.contains(&path.as_path()) || lent.contains(path);
        if !keep {
            let _ = client.did_close(&synced.uri);
        }
        keep
    });
}

fn left_behind(uri: &str) -> bool {
    let Some(path) = zen_lsp::position::uri_path(uri) else { return false };
    zen_lsp::position::mapped_uri(&path).is_some_and(|mapped| mapped != uri)
}

fn difference(old: &str, new: &str) -> Edit {
    let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = old[start..].bytes().rev().zip(new[start..].bytes().rev()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(old.len() - end) || !new.is_char_boundary(new.len() - end) {
        end -= 1;
    }
    Edit { range: start..old.len() - end, text: new[start..new.len() - end].to_owned() }
}

pub(crate) fn char_offset(text: &str, byte: usize) -> usize {
    text.get(..byte.min(text.len())).map_or_else(|| text.chars().count(), |before| before.chars().count())
}

pub(crate) fn byte_offset(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(byte, _)| byte)
}

pub(crate) fn request_at(
    languages: &mut Languages,
    workbench: &Workbench,
    path: &Path,
    offset: usize,
    method: &str,
    extra: Value,
) -> Result<DocumentRequest, String> {
    request_in(languages, workbench, path, method, |uri, text| {
        let position = zen_lsp::position::position(text, byte_offset(text, offset));
        let mut params = json!({ "textDocument": { "uri": uri }, "position": position });
        if let (Some(params), Value::Object(extra)) = (params.as_object_mut(), extra) {
            params.extend(extra);
        }
        params
    })
}

pub(crate) fn request_in(
    languages: &mut Languages,
    workbench: &Workbench,
    path: &Path,
    method: &str,
    params: impl FnOnce(&str, &str) -> Value,
) -> Result<DocumentRequest, String> {
    let open = workbench.documents().iter().any(|document| document.path() == path);
    if !open && !path.is_file() {
        return Err(t!("workbench-nav-no-file", path = path.display().to_string()));
    }
    languages.sync(workbench);
    if !languages.serves(path) {
        return Err(t!("workbench-language-not-running"));
    }
    if !open {
        languages.lend(path)?;
    }
    let (client, synced) = languages.server_for(path).ok_or_else(|| t!("workbench-language-not-running"))?;
    let uri = synced.uri.clone();
    let params = params(&uri, &synced.text);
    client.document_request(&uri, method, params).map_err(|error| error.to_string())
}

pub(crate) fn file_text(workbench: &Workbench, path: &Path) -> Result<String, String> {
    match workbench.documents().iter().find(|document| document.path() == path) {
        Some(document) => Ok(document.text().to_owned()),
        None => disk_text(path),
    }
}

fn disk_text(path: &Path) -> Result<String, String> {
    zen_vfs::read_text(path).map_err(|_| t!("workbench-nav-no-file", path = path.display().to_string()))
}

/// The parameters of a lookup at a place: the active file's caret by default.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LookupParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset in the file; `line` and `column` instead, else the primary caret, when absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// The line, one-based as answers count them: with `column`, the place instead of `offset`.
    #[serde(default)]
    pub line: Option<u32>,
    /// The column on `line`, one-based, in characters; 1 when absent.
    #[serde(default)]
    pub column: Option<u32>,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

impl LookupParams {
    pub fn wait(&self) -> Duration {
        wait(self.wait_ms)
    }

    pub fn place(&self, workbench: &Workbench) -> Result<(PathBuf, usize), CommandError> {
        let active = workbench.active_document();
        let path = self
            .path
            .as_deref()
            .map(|path| workbench.resolve(path))
            .or_else(|| active.map(|document| document.path().to_owned()))
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let offset = match (self.offset, self.line) {
            (Some(offset), _) => offset,
            (None, Some(line)) => {
                let text = file_text(workbench, &path).map_err(CommandError::new)?;
                line_column_offset(&text, line, self.column.unwrap_or(1)).ok_or_else(|| {
                    CommandError::new(t!(
                        "workbench-error-no-line",
                        line = line,
                        lines = text.split('\n').count(),
                        path = project_relative(&path, workbench.project())
                    ))
                })?
            }
            (None, None) => workbench
                .documents()
                .iter()
                .find(|document| document.path() == path)
                .map(|document| document.selections().primary().head)
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?,
        };
        Ok((path, offset))
    }
}

pub fn line_column_offset(text: &str, line: u32, column: u32) -> Option<usize> {
    let index = usize::try_from(line.checked_sub(1)?).ok()?;
    let mut start = 0;
    for (number, content) in text.split('\n').enumerate() {
        if number == index {
            let content = content.strip_suffix('\r').unwrap_or(content);
            let length = content.chars().count();
            let column = usize::try_from(column.max(1) - 1).unwrap_or(usize::MAX).min(length);
            return Some(start + column);
        }
        start += content.chars().count() + 1;
    }
    None
}

pub fn project_relative(path: &Path, root: Option<&Path>) -> String {
    let real = || root.and_then(|root| root.canonicalize().ok());
    let inside = root.and_then(|root| path.strip_prefix(root).ok().map(Path::to_path_buf)).or_else(|| {
        let real = real()?;
        path.strip_prefix(&real).ok().map(Path::to_path_buf)
    });
    match inside {
        Some(inside) => {
            inside.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
        }
        None => path.display().to_string(),
    }
}

/// The parameters of a command that reads a lookup's state.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitParams {
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

impl WaitParams {
    pub fn wait(&self) -> Duration {
        wait(self.wait_ms)
    }
}

fn wait(wait_ms: Option<u64>) -> Duration {
    Duration::from_millis(wait_ms.unwrap_or(0)).min(MAX_WAIT)
}

fn problem(text: &str, diagnostic: &Value) -> Option<Problem> {
    let position = |at: &Value| -> Option<lsp_position::Position> {
        Some(lsp_position::Position { line: at["line"].as_u64()? as u32, character: at["character"].as_u64()? as u32 })
    };
    let start = zen_lsp::position::offset(text, position(&diagnostic["range"]["start"])?).unwrap_or(text.len());
    let end = zen_lsp::position::offset(text, position(&diagnostic["range"]["end"])?).unwrap_or(text.len()).max(start);
    let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
    let severity = match diagnostic["severity"].as_u64() {
        Some(2) => Severity::Warning,
        Some(3) => Severity::Information,
        Some(4) => Severity::Hint,
        _ => Severity::Error,
    };
    let code = match &diagnostic["code"] {
        Value::String(code) => Some(code.clone()),
        Value::Number(code) => Some(code.to_string()),
        _ => None,
    };
    Some(Problem {
        start: char_offset(text, start),
        end: char_offset(text, end),
        line: text[..start].matches('\n').count() as u32 + 1,
        column: text[line_start..start].chars().count() as u32 + 1,
        severity,
        message: diagnostic["message"].as_str().unwrap_or_default().to_owned(),
        source: diagnostic["source"].as_str().map(str::to_owned),
        code,
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        deprecated: tagged(diagnostic, DIAGNOSTIC_TAG_DEPRECATED),
        unused: tagged(diagnostic, DIAGNOSTIC_TAG_UNNECESSARY),
    })
}

const DIAGNOSTIC_TAG_UNNECESSARY: u64 = 1;
const DIAGNOSTIC_TAG_DEPRECATED: u64 = 2;

fn tagged(diagnostic: &Value, tag: u64) -> bool {
    diagnostic["tags"].as_array().is_some_and(|tags| tags.iter().any(|value| value.as_u64() == Some(tag)))
}

use zen_lsp::lsp_types as lsp_position;

pub(crate) fn execute_server_command(
    languages: &mut Languages,
    workbench: &mut Workbench,
    id: &str,
    command: &str,
    arguments: Value,
    deadline: Instant,
) -> Result<Value, String> {
    let started = Instant::now();
    languages.keep_running.insert(id.to_owned());
    let pending = loop {
        languages.sync(workbench);
        match languages.servers.get(id) {
            Some(Server::Running { .. }) => {}
            Some(Server::Failed { reason, .. }) => return Err(reason.text()),
            Some(Server::Starting(_)) => {}
            Some(Server::Stopped) | None
                if started.elapsed() >= Duration::from_secs(3) || Instant::now() >= deadline =>
            {
                return Err(t!("workbench-language-server-not-started", server = id));
            }
            Some(Server::Stopped) | None => {}
        }
        if let Some(client) = languages.client_of(id) {
            let request = json!({ "command": command, "arguments": arguments });
            break client.request("workspace/executeCommand", request).map_err(|error| error.to_string())?;
        }
        if Instant::now() >= deadline {
            return Err(t!("workbench-language-server-starting", server = id));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    loop {
        match pending.wait(Duration::from_millis(20)) {
            Err(LspError::Timeout) if Instant::now() < deadline => languages.sync(workbench),
            Err(LspError::Timeout) => {
                if let Some(client) = languages.client_of(id) {
                    client.cancel(&pending);
                }
                return Err(LspError::Timeout.to_string());
            }
            answer => return answer.map_err(|error| error.to_string()),
        }
    }
}

pub(crate) fn with_languages<T>(session: &mut Session, f: impl FnOnce(&mut Languages, &mut Workbench) -> T) -> T {
    crate::scratch::homes::place(session);
    let registry = session.state_mut::<servers::LanguageServers>().clone();
    let outside =
        session.state::<crate::scratch::Scratches>().map(crate::scratch::Scratches::folders).unwrap_or_default();
    let mut languages = std::mem::take(session.state_mut::<Languages>());
    languages.set_registry(registry);
    languages.outside_folders = outside;
    let mut workbench = std::mem::take(session.state_mut::<Workbench>());
    let result = f(&mut languages, &mut workbench);
    session.set_state(workbench);
    session.set_state(languages);
    result
}

/// What `language.servers.sync` reports.
#[derive(Serialize, JsonSchema)]
pub struct SyncReport {
    /// Each started server's state, and each missing one the project needs, by server id.
    pub servers: BTreeMap<String, ServerStatus>,
    /// What happened that the user should be told (a restart, a stop), once.
    pub notices: Vec<String>,
}

pub struct SyncServers;

impl Command for SyncServers {
    type Params = NoParams;
    type Output = SyncReport;
    const ID: &'static str = "language.servers.sync";
    const TITLE: &'static str = "Synchronize Language Servers";
    const DESCRIPTION: &'static str = "Starts the language servers the open files need, sends them the files' changes, saves and \
         closes, and collects their problems. The window runs it every frame; returns each server's state.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Self::Output, CommandError> {
        let switches = inlay_hints::HintSwitches::from_settings(session.settings());
        let power_save = crate::power_save::is_on(session);
        let style = crate::code_style_options::server_sections(session);
        let analyzers = crate::inspection_profiles::analyzer_switches(session);
        let report = with_languages(session, |languages, workbench| {
            languages.set_hint_switches(switches);
            languages.style_sections = style;
            languages.set_analyzer_switches(analyzers);
            languages.set_power_save(power_save);
            if let Some(index) = &workbench.index {
                index.set_paused(power_save);
            }
            languages.sync(workbench);
            SyncReport { servers: languages.status(workbench.project(), false), notices: languages.take_notices() }
        });
        server_tasks::publish(session);
        crate::schemas::announce(session);
        Ok(report)
    }
}

/// The parameters of `language.servers.restart`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct RestartParams {
    /// The language id of the servers, e.g. `go`, or a server id, e.g. `gopls`; every server when absent.
    #[serde(default)]
    pub language: Option<String>,
}

pub struct RestartServers;

impl Command for RestartServers {
    type Params = RestartParams;
    type Output = Vec<String>;
    const ID: &'static str = "language.servers.restart";
    const TITLE: &'static str = "Restart Language Servers";
    const DESCRIPTION: &'static str = "Stops the language servers of a language (by language id or server id, or every one) and \
         forgets their problems; the next language.servers.sync starts them again. Returns the server ids stopped.";

    fn run(&self, session: &mut Session, params: RestartParams) -> Result<Self::Output, CommandError> {
        Ok(with_languages(session, |languages, _| languages.restart(params.language.as_deref())))
    }
}

pub struct ServersStatus;

impl Command for ServersStatus {
    type Params = NoParams;
    type Output = BTreeMap<String, ServerStatus>;
    const ID: &'static str = "language.servers.status";
    const TITLE: &'static str = "Language Servers";
    const DESCRIPTION: &'static str = "Each started language server of the project (by server id) and whether it is starting, \
         running, stopped or failed, with why, and each server the project needs that is missing (not installed), with how \
         to install it (`install`: the command lsp.server.install and what it runs); lsp.servers.list tells more.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Self::Output, CommandError> {
        Ok(with_languages(session, |languages, workbench| languages.status(workbench.project(), true)))
    }
}

pub(crate) fn wait_checked(session: &mut Session, path: &Path, wait: Duration) -> Option<Reason> {
    let deadline = Instant::now() + wait;
    loop {
        let reason = with_languages(session, |languages, workbench| {
            languages.sync(workbench);
            if let Some(absence) = languages.absence_for(path) {
                return Some(absence.reason);
            }
            if !languages.serves(path) {
                return Some(Reason::NoServer);
            }
            let version =
                workbench.documents().iter().find(|document| document.path() == path).map(crate::Document::version);
            languages.analysis(path, version)
        });
        let waits = matches!(reason, Some(Reason::Checking | Reason::NotChecked | Reason::Starting));
        if !waits || Instant::now() >= deadline {
            return reason;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The parameters of `problems.file.list`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct FileProblemsParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Which problems, how many and in what form.
    #[serde(flatten)]
    pub filter: ProblemFilter,
}

/// What an agent keeps of a list of problems (ADR-0139): the least severity, the sources, a limit
/// and the compact form.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProblemFilter {
    /// The least severity listed: `error` lists errors only, `warning` errors and warnings,
    /// `weakWarning` those and weak warnings; every problem when absent.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// Only the problems of these sources (`rustc`, `clippy`, `compiler`, `eslint`...), ignoring case.
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    /// At most this many problems; `truncated` then counts those left out.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Whether each problem is one row of text, `line:column severity message [source code]`
    /// (`path:line:column ...` in problems.project.list), without offsets, popups or fixes.
    #[serde(default)]
    pub compact: Option<bool>,
}

impl ProblemFilter {
    pub fn keeps(&self, problem: &Problem) -> bool {
        self.severity.is_none_or(|least| problem.severity <= least)
            && self.sources.as_ref().is_none_or(|sources| {
                problem
                    .source
                    .as_deref()
                    .is_some_and(|source| sources.iter().any(|wanted| wanted.eq_ignore_ascii_case(source)))
            })
    }

    pub fn is_active(&self) -> bool {
        self.severity.is_some() || self.sources.is_some() || self.limit.is_some() || self.compact == Some(true)
    }

    fn compact(&self) -> bool {
        self.compact == Some(true)
    }
}

pub fn problem_row(problem: &Problem) -> String {
    let mut row = format!(
        "{}:{} {} {}",
        problem.line,
        problem.column,
        compact::name_of(&problem.severity),
        problem.message.lines().next().unwrap_or_default()
    );
    let tag: Vec<&str> = [problem.source.as_deref(), problem.code.as_deref()].into_iter().flatten().collect();
    if !tag.is_empty() {
        row.push_str(&format!(" [{}]", tag.join(" ")));
    }
    row
}

/// A filtered answer of `problems.file.list`: the problems kept, as objects or rows, and how
/// many the limit left out.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilteredProblems {
    /// The problems kept, in order: objects, or rows with `compact`.
    pub problems: Value,
    /// Whether the file's server has checked the file as it is now.
    pub checked: bool,
    /// Why it is not checked, as in the unfiltered answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// The server the file needs, when it is not there.
    #[serde(flatten)]
    pub absent: Option<AbsentServer>,
    /// What to do, in words, when the file is not checked for another reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// How many problems the limit left out.
    #[serde(skip_serializing_if = "is_zero")]
    pub truncated: usize,
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// What `problems.file.list` answers: the problems of a file its server has checked as it is now;
/// otherwise, and whenever there are none, whether the file was checked and why not (card
/// ZneOu428: an empty list never leaves "clean" and "not checked yet" to guess).
#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FileProblemsAnswer {
    /// The problems, in order, of the file as its server checked it last.
    Problems(Vec<Problem>),
    /// What a filter (`severity`, `sources`, `limit`, `compact`) kept, always in this form.
    Filtered(FilteredProblems),
    /// No problems, or problems of a file that is not checked as it is now.
    Status {
        /// The problems known, in order; empty when there are none.
        problems: Vec<Problem>,
        /// Whether the file's server (or inspection) has checked the file as it is now: with no
        /// problems, the file is clean.
        checked: bool,
        /// Why it is not checked: `notChecked`, `checking`, `noServer`, `serverMissing`,
        /// `serverNotRunning` or `starting`.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<Reason>,
        /// The server the file needs (with its `message`), when it is not there.
        #[serde(flatten)]
        absent: Option<AbsentServer>,
        /// What to do, in words, when the file is not checked for another reason.
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

impl FileProblemsAnswer {
    fn new(problems: Vec<Problem>, analysis: Option<Reason>) -> Self {
        let message = match analysis {
            Some(Reason::NotChecked) => Some(t!("workbench-problems-not-checked")),
            Some(Reason::Checking) => Some(t!("workbench-problems-checking")),
            Some(Reason::NoServer) => Some(t!("workbench-problems-no-server")),
            Some(Reason::PowerSave) => Some(t!("workbench-problems-power-save")),
            _ => None,
        };
        match analysis {
            None if !problems.is_empty() => Self::Problems(problems),
            _ => Self::Status { problems, checked: analysis.is_none(), reason: analysis, absent: None, message },
        }
    }

    fn filtered(self, filter: &ProblemFilter) -> Self {
        if !filter.is_active() {
            return self;
        }
        let (problems, checked, reason, absent, message) = match self {
            Self::Problems(problems) => (problems, true, None, None, None),
            Self::Status { problems, checked, reason, absent, message } => (problems, checked, reason, absent, message),
            filtered @ Self::Filtered(_) => return filtered,
        };
        let mut kept: Vec<Problem> = problems.into_iter().filter(|problem| filter.keeps(problem)).collect();
        let truncated = filter.limit.map_or(0, |limit| kept.len().saturating_sub(limit));
        kept.truncate(kept.len() - truncated);
        let problems = if filter.compact() {
            Value::from(kept.iter().map(problem_row).collect::<Vec<_>>())
        } else {
            serde_json::to_value(&kept).unwrap_or_default()
        };
        Self::Filtered(FilteredProblems { problems, checked, reason, absent, message, truncated })
    }
}

pub fn listed_problems(answer: &Value) -> Vec<Value> {
    answer.as_array().or_else(|| answer["problems"].as_array()).cloned().unwrap_or_default()
}

pub struct FileProblems;

impl Command for FileProblems {
    type Params = FileProblemsParams;
    type Output = FileProblemsAnswer;
    const ID: &'static str = "problems.file.list";
    const TITLE: &'static str = "File Problems";
    const DESCRIPTION: &'static str = "The problems the language servers, the inspections (go.mod's dependency checks, for \
         one) and the project model (a Cargo.toml, go.mod or go.work it cannot read, with `source: \"project model\"`) found in a file (the active \
         one by default), in order, with their quick-fixes. A list when the file's server has checked the file as \
         it is now; otherwise, and whenever there are none, {problems, checked, reason?, message?}: `checked: true` with no problems is a clean file, `checked: false` says why in \
         `reason` (notChecked: the server has not checked it yet, and opening it with file.document.open makes it; \
         checking: it changed and the new check has not come, so call again; noServer: nothing checks such files; \
         serverMissing|serverNotRunning|starting, with `server`, `install` and `message`). A problem with \
         `deprecated` is struck out in the editor and one with `unused` (an unused requirement, a diagnostic tagged \
         Unnecessary, gopls's 'is not used in this module') is greyed out instead of underlined; a server's unused \
         diagnostic that an unused inspection finding covers is listed once, as the finding. Misspelt words are \
         problems of severity typo (source spelling), once the background spelling check of the text as it is now \
         is done; spelling.typos.list waits for it. `severity` (error: errors only; warning: errors and warnings), \
         `sources`, `limit` and `compact` (one row of text per problem, `line:column severity message [source code]`) \
         narrow the answer, which is then always {problems, checked, reason?, truncated?}.";

    fn run(&self, session: &mut Session, params: FileProblemsParams) -> Result<FileProblemsAnswer, CommandError> {
        let path = params
            .path
            .map(|path| session.state::<Workbench>().map_or_else(|| path.clone(), |workbench| workbench.resolve(&path)))
            .or_else(|| {
                session
                    .state::<Workbench>()
                    .and_then(Workbench::active_document)
                    .map(|document| document.path().to_owned())
            })
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        if crate::power_save::is_on(session) {
            return Ok(FileProblemsAnswer::new(Vec::new(), Some(Reason::PowerSave)));
        }
        let problems = crate::inspections::all_problems(session, &path);
        let inspected = crate::inspections::inspects(session, &path)
            || crate::plugins::checks(session, &path)
            || crate::spelling::checks(session, &path)
            || session.state::<crate::project_model::ProjectModels>().is_some_and(|models| models.checks(&path));
        let analysis = with_languages(session, |languages, workbench| {
            if problems.is_empty()
                && let Some(absence) = languages.absence_for(&path)
            {
                return Err(Box::new(absence));
            }
            if !languages.serves(&path) {
                return Ok((!inspected).then_some(Reason::NoServer));
            }
            let version =
                workbench.documents().iter().find(|document| document.path() == path).map(crate::Document::version);
            Ok(languages.analysis(&path, version))
        });
        let answer = match analysis {
            Ok(analysis) => FileProblemsAnswer::new(problems, analysis),
            Err(absence) => {
                let (reason, absent) = (*absence).split();
                let reason = Some(reason);
                FileProblemsAnswer::Status { problems, checked: false, reason, absent: Some(absent), message: None }
            }
        };
        Ok(answer.filtered(&params.filter))
    }
}

/// The problems of one file, for `problems.project.list`.
#[derive(Serialize, JsonSchema)]
pub struct FileWithProblems {
    /// The file.
    pub path: PathBuf,
    /// Its problems, in order.
    pub problems: Vec<Problem>,
}

/// The parameters of `problems.project.list`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct ProjectProblemsParams {
    /// Only the files that match one of these patterns: a path relative to the project with `*`
    /// (`crates/*/src/*.rs`), a file name pattern (`*.go`) or a folder (`crates/platform/core`).
    #[serde(default)]
    pub paths: Option<Vec<String>>,
    /// Leaves out the files that match one of these patterns, as `paths` reads them (`go.mod`, `vendor`).
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
    /// Which problems, how many and in what form.
    #[serde(flatten)]
    pub filter: ProblemFilter,
}

impl ProjectProblemsParams {
    fn is_active(&self) -> bool {
        self.filter.is_active() || self.paths.is_some() || self.exclude.is_some()
    }

    fn keeps_file(&self, relative: &str) -> bool {
        let any = |patterns: &Option<Vec<String>>| {
            patterns.as_ref().map(|patterns| patterns.iter().any(|pattern| compact::path_matches(pattern, relative)))
        };
        any(&self.paths).unwrap_or(true) && !any(&self.exclude).unwrap_or(false)
    }
}

/// The problems of one file in a filtered `problems.project.list`, its path relative to the project.
#[derive(Serialize, JsonSchema)]
pub struct RelativeFileProblems {
    /// The file, relative to the project with `/` (the whole path outside it).
    pub path: String,
    /// Its problems kept, in order.
    pub problems: Vec<Problem>,
}

/// What `problems.project.list` answers: the files with problems; or, when a server the project
/// needs is missing (the problems of its files are unknown) or none can answer while there are no
/// problems, the files with why, as `problems.file.list` says it (card AOiQ4Q4b).
#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ProjectProblemsAnswer {
    /// The files with problems.
    Files(Vec<FileWithProblems>),
    /// What a filter (`paths`, `exclude`, `severity`, `sources`, `limit`, `compact`) kept,
    /// always in this form.
    Filtered {
        /// The files with problems kept ([`RelativeFileProblems`]), or with `compact` the problems as rows,
        /// `path:line:column severity message [source code]`.
        problems: Value,
        /// How many problems the filter kept, the limit's included.
        total: usize,
        /// How many problems the limit left out.
        #[serde(skip_serializing_if = "is_zero")]
        truncated: usize,
        /// `serverMissing`, `serverNotRunning` or `starting`, when a server the project needs is not there.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<Reason>,
        /// The server, how to install it and what it means.
        #[serde(flatten)]
        absent: Option<AbsentServer>,
    },
    /// The files with problems known, and the server that is not there.
    Absent {
        /// The files with problems known, in path order.
        problems: Vec<FileWithProblems>,
        /// `serverMissing`, `serverNotRunning` or `starting`.
        reason: Reason,
        /// The server, how to install it and what it means.
        #[serde(flatten)]
        absent: AbsentServer,
    },
}

pub struct ProjectProblems;

impl Command for ProjectProblems {
    type Params = ProjectProblemsParams;
    type Output = ProjectProblemsAnswer;
    const ID: &'static str = "problems.project.list";
    const TITLE: &'static str = "Project Problems";
    const DESCRIPTION: &'static str = "Every file with problems the language servers, the inspections of the open files or \
         the project model (a module file it cannot read) reported, with its problems; typos are not among them \
         (problems.file.list and spelling.typos.list list them). When a server the project needs is missing, or none \
         runs while nothing is reported, {problems: [files], reason: serverMissing|serverNotRunning|starting, server, \
         install, message} instead, as problems.file.list says it. `paths` and `exclude` (patterns of project-relative \
         paths, file names or folders), `severity` (error: errors only), `sources`, `limit` and `compact` (one row per \
         problem, `path:line:column severity message [source code]`) narrow it to {problems, total, truncated?, reason?}.";

    fn run(&self, session: &mut Session, params: ProjectProblemsParams) -> Result<Self::Output, CommandError> {
        let mut paths: BTreeSet<PathBuf> = session
            .state::<Languages>()
            .map(|languages| languages.problems.keys().cloned().collect())
            .unwrap_or_default();
        paths.extend(crate::inspections::inspected_files(session));
        paths.extend(crate::file_watchers::problem_files(session));
        paths.extend(
            session
                .state::<crate::project_model::ProjectModels>()
                .map(crate::project_model::ProjectModels::problem_files)
                .unwrap_or_default(),
        );
        let files: Vec<FileWithProblems> = paths
            .into_iter()
            .map(|path| {
                let mut problems = crate::inspections::all_problems(session, &path);
                problems.retain(|problem| problem.severity != Severity::Typo);
                FileWithProblems { path, problems }
            })
            .filter(|file| !file.problems.is_empty())
            .collect();
        let empty = files.is_empty();
        let absence = with_languages(session, |languages, workbench| {
            let root = workbench.project()?.to_owned();
            languages
                .missing_project_server(&root, false)
                .or_else(|| empty.then(|| languages.project_absence(&root, false)).flatten())
        });
        if params.is_active() {
            let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
            let mut kept: Vec<RelativeFileProblems> = files
                .into_iter()
                .map(|file| RelativeFileProblems {
                    path: project_relative(&file.path, root.as_deref()),
                    problems: file.problems.into_iter().filter(|problem| params.filter.keeps(problem)).collect(),
                })
                .filter(|file| !file.problems.is_empty() && params.keeps_file(&file.path))
                .collect();
            let total: usize = kept.iter().map(|file| file.problems.len()).sum();
            let mut left = params.filter.limit.unwrap_or(usize::MAX);
            for file in &mut kept {
                file.problems.truncate(left);
                left -= file.problems.len();
            }
            kept.retain(|file| !file.problems.is_empty());
            let truncated = total - kept.iter().map(|file| file.problems.len()).sum::<usize>();
            let problems = if params.filter.compact() {
                let rows: Vec<String> = kept
                    .iter()
                    .flat_map(|file| {
                        file.problems.iter().map(|problem| format!("{}:{}", file.path, problem_row(problem)))
                    })
                    .collect();
                Value::from(rows)
            } else {
                serde_json::to_value(&kept).unwrap_or_default()
            };
            let (reason, absent) = absence.map(ServerAbsence::split).unzip();
            return Ok(ProjectProblemsAnswer::Filtered { problems, total, truncated, reason, absent });
        }
        Ok(match absence {
            Some(absence) => {
                let (reason, absent) = absence.split();
                ProjectProblemsAnswer::Absent { problems: files, reason, absent }
            }
            None => ProjectProblemsAnswer::Files(files),
        })
    }
}

pub struct GoToProblem<const NEXT: bool>;

impl<const NEXT: bool> Command for GoToProblem<NEXT> {
    type Params = NoParams;
    type Output = Option<Problem>;
    const ID: &'static str = if NEXT { "editor.problem.next" } else { "editor.problem.previous" };
    const TITLE: &'static str = if NEXT { "Next Highlighted Error" } else { "Previous Highlighted Error" };
    const DESCRIPTION: &'static str = if NEXT {
        "Moves the caret to the next problem in the active file, going round to the first."
    } else {
        "Moves the caret to the previous problem in the active file, going round to the last."
    };

    fn is_available(&self, session: &Session) -> bool {
        let path = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned());
        path.is_some_and(|path| !crate::inspections::known_problems(session, &path).is_empty())
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Option<Problem>, CommandError> {
        let origin = crate::history::caret(session);
        let document = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let caret = document.selections().primary().head;
        let path = document.path().to_owned();
        let problems = crate::inspections::all_problems(session, &path);
        let target = if NEXT {
            problems.iter().find(|problem| problem.start > caret).or_else(|| problems.first())
        } else {
            problems.iter().rev().find(|problem| problem.start < caret).or_else(|| problems.last())
        }
        .cloned();
        if let Some(problem) = &target
            && let Some(document) = session.state_mut::<Workbench>().active_document_mut()
        {
            document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(problem.start)));
        }
        crate::history::jumped(session, origin);
        Ok(target)
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(SyncServers)?;
    registry.register(ServersStatus)?;
    registry.register(RestartServers)?;
    registry.register(FileProblems)?;
    registry.register(ProjectProblems)?;
    registry.register(GoToProblem::<true>)?;
    registry.register(GoToProblem::<false>)?;
    crate::inspections::register(registry)?;
    servers::register(registry)?;
    completion::register(registry)?;
    classes::register(registry)?;
    symbols::register(registry)?;
    semantic::register(registry)?;
    inlay_hints::register(registry)?;
    structure::register(registry)?;
    breadcrumbs::register(registry)?;
    method_separators::register(registry)?;
    navigation_bar::register(registry)?;
    code_vision::register(registry)?;
    highlights::register(registry)?;
    goto::register(registry)?;
    go_to_test::register(registry)?;
    hierarchy::register(registry)?;
    usages::register(registry)?;
    show_usages::register(registry)?;
    usages_settings::register(registry)?;
    docs::register(registry)?;
    peek::register(registry)?;
    extract::register(registry)?;
    refactor::register(registry)?;
    safe_delete::register(registry)?;
    refactoring_dialog::register(registry)?;
    format::register(registry)?;
    intentions::register(registry)?;
    generate::register(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_difference_is_the_changed_middle() {
        assert_eq!(difference("hello world", "hello brave world"), Edit { range: 6..6, text: "brave ".to_owned() });
        assert_eq!(difference("abcdef", "abXYef"), Edit { range: 2..4, text: "XY".to_owned() });
        assert_eq!(difference("same", "same"), Edit { range: 4..4, text: String::new() });
        let edit = difference("café", "cafè");
        assert_eq!(edit, Edit { range: 3..5, text: "è".to_owned() });
        let mut text = "ab\nab".to_owned();
        let edit = difference(&text, "ab\nb");
        text.replace_range(edit.range.clone(), &edit.text);
        assert_eq!(text, "ab\nb");
    }

    #[test]
    fn problems_of_an_unopened_file_are_placed_after_a_read_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("other.go");
        std::fs::write(&file, "package main\n\nfunc broken\n").unwrap();
        let uri = zen_lsp::position::file_uri(&file);
        let range = json!({ "start": { "line": 2, "character": 5 }, "end": { "line": 2, "character": 11 } });
        let params = json!({ "uri": uri, "diagnostics": [{ "range": range, "message": "broken" }] });
        let mut languages = Languages::default();
        languages.publish(&params, &HashMap::new());
        assert!(languages.problems.is_empty(), "the frame does not read the file");
        let start = Instant::now();
        while languages.problems.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(60), "the file was not read");
            std::thread::sleep(Duration::from_millis(5));
            languages.take_unopened_texts();
        }
        assert_eq!(languages.problems[&file].len(), 1);
    }

    #[test]
    fn an_empty_publish_for_an_unopened_file_reads_nothing() {
        let params = json!({ "uri": "file:///gone/other.go", "diagnostics": [] });
        let mut languages = Languages::default();
        languages.publish(&params, &HashMap::new());
        assert!(languages.unopened.asking.is_none());
    }

    #[test]
    fn files_changed_on_disk_are_told_to_the_server_that_serves_them() {
        let mut fixture = testing::fixture(&[("main.go", "package main\n"), ("util.go", "package main\n")]);
        fixture.open("main.go");
        let util = fixture.path("util.go");
        std::fs::remove_file(&util).unwrap();
        let paths = [fixture.path("main.go"), util.clone(), fixture.path("notes.txt")];
        fixture.session.state_mut::<Languages>().files_changed(&paths);
        let told = fixture.server.wait_for("workspace/didChangeWatchedFiles", 1, Duration::from_secs(10));
        let changes = json!([
            { "uri": zen_lsp::position::file_uri(&fixture.path("main.go")), "type": 2 },
            { "uri": zen_lsp::position::file_uri(&util), "type": 3 }
        ]);
        assert_eq!(told, vec![json!({ "changes": changes })], "a file it does not serve is not told");
    }

    fn java_fixture(files: &[(&str, &str)]) -> testing::Fixture {
        let mut fixture = testing::fixture(files);
        let launched = fixture.server.clone();
        fixture.session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "java".to_owned(),
            extensions: vec!["java".to_owned()],
            launch: Arc::new(move |_| Ok(launched.launcher())),
            initialization_options: json!({}),
        }]);
        let root = fixture.path("");
        crate::trust::trust_session_in_tests(&mut fixture.session, &root);
        fixture
    }

    fn import(fixture: &mut testing::Fixture, pom: &Path) -> BuildImport {
        fixture.with(|languages, workbench| languages.import_build(workbench, std::slice::from_ref(&pom.to_owned())))
    }

    fn progress_of(fixture: &mut testing::Fixture, asked: Instant) -> ImportProgress {
        fixture.with(|languages, workbench| {
            languages.sync(workbench);
            languages.import_progress("java", asked)
        })
    }

    fn settled_at(fixture: &mut testing::Fixture, asked: Instant) -> ImportProgress {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let progress = progress_of(fixture, asked);
            if progress != ImportProgress::Running || Instant::now() >= deadline {
                return progress;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_reload_starts_the_java_server_without_an_open_file() {
        let mut fixture = java_fixture(&[("pom.xml", "<project/>"), ("src/App.java", "class App {}\n")]);
        let pom = fixture.path("pom.xml");
        assert_eq!(fixture.open_files(), Vec::<PathBuf>::new());
        assert_eq!(import(&mut fixture, &pom), BuildImport::Asked { server: "java".to_owned() });
        let asked = Instant::now() - IMPORT_SETTLES;
        assert_eq!(settled_at(&mut fixture, asked), ImportProgress::Done);
        assert_eq!(fixture.server.launches(), 1);
    }

    #[test]
    fn the_import_ends_when_the_server_says_so_and_not_before() {
        let mut fixture = java_fixture(&[("pom.xml", "<project/>")]);
        let pom = fixture.path("pom.xml");
        import(&mut fixture, &pom);
        let asked = Instant::now() - IMPORT_SETTLES;
        settled_at(&mut fixture, asked);
        let status = |fixture: &mut testing::Fixture, kind: &str, message: &str| {
            let params = json!({ "type": kind, "message": message });
            fixture.with(|languages, _| {
                languages.activity.entry("java".to_owned()).or_default().notification("language/status", &params)
            })
        };
        status(&mut fixture, "Starting", "Init...");
        assert_eq!(progress_of(&mut fixture, asked), ImportProgress::Running);
        status(&mut fixture, "Started", "Ready");
        assert_eq!(progress_of(&mut fixture, Instant::now()), ImportProgress::Done, "it said so: no need to wait");
        status(&mut fixture, "Error", "Maven import failed");
        assert_eq!(
            progress_of(&mut fixture, asked),
            ImportProgress::Failed { reason: "Maven import failed".to_owned(), java: None }
        );
    }

    #[test]
    fn a_running_server_is_asked_again_and_not_started_twice() {
        let mut fixture = java_fixture(&[("pom.xml", "<project/>"), ("App.java", "class App {}\n")]);
        fixture.open("App.java");
        let pom = fixture.path("App.java");
        assert_eq!(import(&mut fixture, &pom), BuildImport::Asked { server: "java".to_owned() });
        fixture.server.wait_for("workspace/didChangeWatchedFiles", 1, Duration::from_secs(10));
        assert_eq!(fixture.server.launches(), 1);
    }

    #[test]
    fn a_server_that_failed_to_start_says_why_and_a_new_reload_tries_again() {
        let mut fixture = java_fixture(&[("pom.xml", "<project/>")]);
        fixture.session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "java".to_owned(),
            extensions: vec!["java".to_owned()],
            launch: Arc::new(|_| Err("no java to run it".to_owned())),
            initialization_options: json!({}),
        }]);
        let pom = fixture.path("pom.xml");
        import(&mut fixture, &pom);
        let progress = settled_at(&mut fixture, Instant::now());
        assert_eq!(progress, ImportProgress::Failed { reason: "no java to run it".to_owned(), java: None });
        let launched = fixture.server.clone();
        fixture.session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "java".to_owned(),
            extensions: vec!["java".to_owned()],
            launch: Arc::new(move |_| Ok(launched.launcher())),
            initialization_options: json!({}),
        }]);
        import(&mut fixture, &pom);
        let asked = Instant::now() - IMPORT_SETTLES;
        assert_eq!(settled_at(&mut fixture, asked), ImportProgress::Done);
    }

    fn sync_step(fixture: &mut testing::Fixture) -> Value {
        let syncs = fixture.dispatch("build.sync.get", json!({})).unwrap();
        syncs["syncs"][0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["title"] == "Import into the Java language server")
            .cloned()
            .unwrap()
    }

    fn step_until(fixture: &mut testing::Fixture, state: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let step = sync_step(fixture);
            if step["state"] == state || Instant::now() >= deadline {
                return step;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_sync_step_of_a_maven_reload_waits_for_the_import_and_reports_it() {
        let mut fixture = java_fixture(&[("pom.xml", "<project><artifactId>a</artifactId></project>")]);
        let reloaded = fixture.dispatch("maven.project.reload", json!({ "dependencies": false })).unwrap();
        assert_eq!(reloaded["reimported"], true);
        fixture.with(|languages, _| {
            let params = json!({ "type": "Starting" });
            languages.activity.entry("java".to_owned()).or_default().notification("language/status", &params)
        });
        let step = sync_step(&mut fixture);
        assert_eq!(step["state"], "running", "{step}");
        assert_eq!(fixture.dispatch("build.sync.get", json!({})).unwrap()["syncs"][0]["state"], "running");
        fixture.with(|languages, _| {
            let params = json!({ "type": "Started" });
            languages.activity.entry("java".to_owned()).or_default().notification("language/status", &params)
        });
        let step = step_until(&mut fixture, "done");
        assert!(step["tookMs"].is_u64(), "{step}");
        assert_ne!(fixture.dispatch("build.sync.get", json!({})).unwrap()["syncs"][0]["state"], "running");
        let frozen = sync_step(&mut fixture);
        assert_eq!(frozen["tookMs"], step["tookMs"], "the result stays as it was");
    }

    #[test]
    fn a_gradle_reload_starts_the_server_and_a_failure_shows_in_its_sync_step() {
        let mut fixture = java_fixture(&[("build.gradle", "plugins { id 'java' }\n")]);
        fixture.session.state_mut::<Languages>().set_specs(vec![ServerSpec {
            language_id: "java".to_owned(),
            extensions: vec!["java".to_owned()],
            launch: Arc::new(|_| Err("no java to run it".to_owned())),
            initialization_options: json!({}),
        }]);
        let reloaded = fixture.dispatch("gradle.project.reload", json!({ "tasks": false })).unwrap();
        assert_eq!(reloaded["reimported"], true);
        let step = step_until(&mut fixture, "failed");
        assert_eq!(step["messages"][0]["text"], "no java to run it");
        assert_eq!(step["messages"][0]["severity"], "error");
    }

    #[test]
    fn waiting_on_a_reload_waits_for_the_import_too() {
        let mut fixture = java_fixture(&[("pom.xml", "<project><artifactId>a</artifactId></project>")]);
        let launched = fixture.server.clone();
        fixture.dispatch("maven.project.reload", json!({ "dependencies": false, "waitMs": 60000 })).unwrap();
        assert_eq!(launched.launches(), 1);
        assert_eq!(sync_step(&mut fixture)["state"], "done");
    }

    #[test]
    fn a_stopped_server_stays_stopped() {
        let mut fixture = java_fixture(&[("pom.xml", "<project/>"), ("App.java", "class App {}\n")]);
        fixture.open("App.java");
        fixture.with(|languages, _| languages.stop_server("java"));
        let pom = fixture.path("pom.xml");
        let stopped = BuildImport::Stopped { id: "java".to_owned(), name: "java".to_owned() };
        assert_eq!(import(&mut fixture, &pom), stopped);
    }

    #[test]
    fn a_project_without_a_java_server_says_so() {
        let mut fixture = testing::fixture(&[("pom.xml", "<project/>")]);
        let pom = fixture.path("pom.xml");
        assert_eq!(import(&mut fixture, &pom), BuildImport::NoServer);
    }

    #[test]
    fn a_build_that_changed_is_told_to_the_running_servers_that_watch_it() {
        let mut fixture = testing::fixture(&[("main.go", "package main\n"), ("pom.xml", "<project/>")]);
        let (main, pom) = (fixture.path("main.go"), fixture.path("pom.xml"));
        assert!(
            !fixture.session.state_mut::<Languages>().build_changed(std::slice::from_ref(&main)),
            "no server runs yet"
        );
        fixture.open("main.go");
        let languages = fixture.session.state_mut::<Languages>();
        assert!(!languages.build_changed(&[pom]), "a file no server hears of");
        assert!(languages.build_changed(std::slice::from_ref(&main)));
        let told = fixture.server.wait_for("workspace/didChangeWatchedFiles", 1, Duration::from_secs(10));
        let changes = json!([{ "uri": zen_lsp::position::file_uri(&main), "type": 2 }]);
        assert_eq!(told, vec![json!({ "changes": changes })]);
    }

    #[test]
    fn a_late_report_of_the_place_a_file_moved_from_is_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("scratch.go");
        let plain = zen_lsp::position::file_uri(&file);
        let home = zen_lsp::position::file_uri(&dir.path().join("module/scratch.go"));
        assert!(!left_behind(&plain));
        zen_lsp::position::map_uri(&home, &file);
        assert!(left_behind(&plain));
        assert!(!left_behind(&home));
        assert!(!left_behind("file:///a/stranger.go"));
        zen_lsp::position::unmap_uri(&file);
        assert!(!left_behind(&plain));
    }

    #[test]
    fn diagnostics_become_problems() {
        let text = "package main\n\nfunc main() {\n\tundefinedName()\n}\n";
        let diagnostic = serde_json::json!({
            "range": { "start": { "line": 3, "character": 1 }, "end": { "line": 3, "character": 14 } },
            "severity": 1, "message": "undefined: undefinedName", "source": "compiler", "code": "UndeclaredName"
        });
        let problem = problem(text, &diagnostic).unwrap();
        let chars: Vec<char> = text.chars().collect();
        assert_eq!(chars[problem.start..problem.end].iter().collect::<String>(), "undefinedName");
        assert_eq!((problem.line, problem.column), (4, 2));
        assert_eq!(problem.severity, Severity::Error);
        assert_eq!(problem.code.as_deref(), Some("UndeclaredName"));
        assert!(!problem.unused && !problem.deprecated);
        let json = serde_json::to_value(&problem).unwrap();
        assert!(json.get("unused").is_none() && json.get("deprecated").is_none());
    }

    #[test]
    fn tagged_diagnostics_are_unused_or_deprecated() {
        let text = "package main\n\nimport \"os\"\n";
        let diagnostic = |tags: Value| {
            let diagnostic = serde_json::json!({
                "range": { "start": { "line": 2, "character": 7 }, "end": { "line": 2, "character": 11 } },
                "severity": 1, "message": "\"os\" imported and not used", "source": "compiler",
                "code": "UnusedImport", "tags": tags
            });
            problem(text, &diagnostic).unwrap()
        };
        let unused = diagnostic(serde_json::json!([1]));
        assert!(unused.unused && !unused.deprecated);
        assert_eq!(serde_json::to_value(&unused).unwrap()["unused"], true, "problems.file.list says so");
        let deprecated = diagnostic(serde_json::json!([2]));
        assert!(deprecated.deprecated && !deprecated.unused);
        let both = diagnostic(serde_json::json!([2, 1]));
        assert!(both.deprecated && both.unused);
        assert!(!diagnostic(serde_json::json!([])).unused);
    }

    #[test]
    fn a_restarted_server_starts_again_without_its_old_problems() {
        let mut fixture = testing::fixture(&[("main.go", "package main\n"), ("notes.txt", "")]);
        fixture.open("main.go");
        assert_eq!(fixture.server.launches(), 1);
        let main = fixture.path("main.go");
        let notes = fixture.path("notes.txt");
        let old = Problem {
            start: 0,
            end: 1,
            line: 1,
            column: 1,
            severity: Severity::Error,
            message: "old".to_owned(),
            source: None,
            code: None,
            note: None,
            rows: Vec::new(),
            details: None,
            fixes: Vec::new(),
            deprecated: false,
            unused: false,
        };
        fixture.with(|languages, _| {
            languages.problems.insert(main.clone(), vec![old.clone()]);
            languages.problems.insert(notes.clone(), vec![old.clone()]);
        });
        assert_eq!(fixture.dispatch("language.servers.restart", json!({ "language": "rust" })).unwrap(), json!([]));
        assert_eq!(fixture.dispatch("language.servers.restart", json!({ "language": "go" })).unwrap(), json!(["go"]));
        fixture.with(|languages, _| {
            assert!(languages.problems(&main).is_empty());
            assert_eq!(languages.problems(&notes), [old]);
            assert!(languages.synced_text(&main).is_none());
        });
        fixture.open("main.go");
        assert_eq!(fixture.server.launches(), 2);
        assert_eq!(fixture.dispatch("language.servers.restart", json!({})).unwrap(), json!(["go"]));
    }

    #[test]
    fn problems_count_characters_where_the_text_is_not_ascii() {
        let text = "package main\n\n// привет\nfunc main() {\n\tundefinedName()\n}\n";
        let diagnostic = serde_json::json!({
            "range": { "start": { "line": 4, "character": 1 }, "end": { "line": 4, "character": 14 } },
            "severity": 1, "message": "undefined: undefinedName"
        });
        let problem = problem(text, &diagnostic).unwrap();
        let chars: Vec<char> = text.chars().collect();
        assert_eq!(chars[problem.start..problem.end].iter().collect::<String>(), "undefinedName");
        assert_eq!((problem.line, problem.column), (5, 2));
        assert_eq!(byte_offset(text, char_offset(text, 30)), 30);
    }
}
