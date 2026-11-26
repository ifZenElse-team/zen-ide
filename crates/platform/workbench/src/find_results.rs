use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_index::TextMatch;
use zen_syntax::{HighlightKind, OutlineItem, Outliner, SymbolKind};

use crate::Workbench;
use crate::language::show_usages::{UsageAccess, UsagesScope};
use crate::language::structure::Outliners;
use crate::search::{DEFAULT_TOO_MANY, Replaced, SearchParams, TOO_MANY};
use crate::search_scopes::{ModuleScope, PROJECT_FILES};
use crate::tasks::{TaskHandle, Tasks};

pub const OPTIONS: &str = "search.results.options";
pub const TASK_SOURCE: &str = "search";
const PROGRESS_EVERY: Duration = Duration::from_millis(80);
const MAX_WAIT: u64 = 10_000;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(OpenOccurrences)?;
    registry.register(ListResults)?;
    registry.register(ResultsTreeCommand)?;
    registry.register(TooManyResults)?;
    registry.register(StopResults)?;
    registry.register(RerunResults)?;
    registry.register(SetOption)?;
    registry.register(ExcludeNodes)?;
    registry.register(IncludeNodes)?;
    registry.register(SelectNode)?;
    registry.register(NextOccurrence)?;
    registry.register(PreviousOccurrence)?;
    registry.register(ExpandNodes)?;
    registry.register(PinResults)?;
    registry.register(CloseResults)?;
    registry.register(ReplaceResults)?;
    usages::register(registry)?;
    Ok(())
}

/// How a usage relates to the code around it, the Usage Type group it falls in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UsageType {
    /// Code, or a file without highlighting, or a usage whose access the language server did not tell.
    Unclassified,
    /// A usage that reads the symbol, as the language server tells it.
    Read,
    /// A usage that writes the symbol, as the language server tells it.
    Write,
    /// A usage in an import.
    Import,
    /// In a comment.
    Comment,
    /// In a string literal.
    String,
}

/// The Find tool window's Group By and view options, kept in the setting [`OPTIONS`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewOptions {
    /// Directory Structure (⌃D): folders as a tree, chains of single folders compacted.
    pub directory_structure: bool,
    /// File Structure (⌃S): the usages of a file under the declarations that hold them.
    pub file_structure: bool,
    /// Module (⌃M): the files under their module.
    pub module: bool,
    /// Flatten Modules (⌃O): modules side by side rather than inside the modules that hold them.
    pub flatten_modules: bool,
    /// Group by Directory (⌃P): the files under their folder, one node per folder.
    pub directory: bool,
    /// Production/Test: the files under Production or Test.
    pub production_test: bool,
    /// Usage Type (⌃T): the usages under Unclassified, Usage in comments and Usage in string constants.
    pub usage_type: bool,
    /// Merge Usages from the Same Line (⌃L): one node per line.
    pub merge_same_line: bool,
    /// Show Usages In Comments: the usages in comments are listed.
    pub show_in_comments: bool,
    /// Preview Source: the source of the selected usage is shown next to the tree.
    pub preview_source: bool,
    /// Show Read Access: the usages that read the symbol are listed (where the language server tells).
    pub show_read: bool,
    /// Show Write Access: the usages that write the symbol are listed (where the language server tells).
    pub show_write: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            directory_structure: true,
            file_structure: false,
            module: true,
            flatten_modules: false,
            directory: false,
            production_test: false,
            usage_type: true,
            merge_same_line: true,
            show_in_comments: true,
            preview_source: true,
            show_read: true,
            show_write: true,
        }
    }
}

/// One of [`ViewOptions`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ViewOption {
    /// Directory Structure (⌃D).
    DirectoryStructure,
    /// File Structure (⌃S).
    FileStructure,
    /// Module (⌃M).
    Module,
    /// Flatten Modules (⌃O).
    FlattenModules,
    /// Group by Directory (⌃P).
    Directory,
    /// Production/Test.
    ProductionTest,
    /// Usage Type (⌃T).
    UsageType,
    /// Merge Usages from the Same Line (⌃L).
    MergeSameLine,
    /// Show Usages In Comments.
    ShowInComments,
    /// Preview Source.
    PreviewSource,
    /// Show Read Access.
    ShowRead,
    /// Show Write Access.
    ShowWrite,
}

impl ViewOptions {
    pub fn get(&self, option: ViewOption) -> bool {
        match option {
            ViewOption::DirectoryStructure => self.directory_structure,
            ViewOption::FileStructure => self.file_structure,
            ViewOption::Module => self.module,
            ViewOption::FlattenModules => self.flatten_modules,
            ViewOption::Directory => self.directory,
            ViewOption::ProductionTest => self.production_test,
            ViewOption::UsageType => self.usage_type,
            ViewOption::MergeSameLine => self.merge_same_line,
            ViewOption::ShowInComments => self.show_in_comments,
            ViewOption::PreviewSource => self.preview_source,
            ViewOption::ShowRead => self.show_read,
            ViewOption::ShowWrite => self.show_write,
        }
    }

    pub fn set(&mut self, option: ViewOption, on: bool) {
        let slot = match option {
            ViewOption::DirectoryStructure => &mut self.directory_structure,
            ViewOption::FileStructure => &mut self.file_structure,
            ViewOption::Module => &mut self.module,
            ViewOption::FlattenModules => &mut self.flatten_modules,
            ViewOption::Directory => &mut self.directory,
            ViewOption::ProductionTest => &mut self.production_test,
            ViewOption::UsageType => &mut self.usage_type,
            ViewOption::MergeSameLine => &mut self.merge_same_line,
            ViewOption::ShowInComments => &mut self.show_in_comments,
            ViewOption::PreviewSource => &mut self.preview_source,
            ViewOption::ShowRead => &mut self.show_read,
            ViewOption::ShowWrite => &mut self.show_write,
        };
        *slot = on;
        if on && option == ViewOption::Directory {
            self.directory_structure = false;
        }
        if on && option == ViewOption::DirectoryStructure {
            self.directory = false;
        }
    }

    pub fn of(session: &Session) -> Self {
        session.settings().get_or(OPTIONS, Self::default())
    }
}

#[derive(Clone, Debug)]
struct Found {
    found: TextMatch,
    usage_type: UsageType,
    members: Vec<(usize, String)>,
    access: Option<UsageAccess>,
    holder: Option<Arc<provided::Holder>>,
}

type Key = (PathBuf, usize);

#[derive(Default)]
struct Gate {
    limit: Option<usize>,
    asking: bool,
}

#[derive(Default)]
struct Progress {
    found: Mutex<Vec<Found>>,
    count: AtomicU64,
    done: AtomicBool,
    classifying: AtomicBool,
    cancel: Arc<AtomicBool>,
    current: Mutex<String>,
    error: Mutex<Option<String>>,
    gate: Mutex<Gate>,
    decided: Condvar,
}

impl Progress {
    fn asking(&self) -> bool {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner).asking && !self.cancel.load(Ordering::Relaxed)
    }

    fn decide(&self, go_on: bool) -> bool {
        let mut gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);
        if !gate.asking {
            return false;
        }
        gate.asking = false;
        gate.limit = None;
        if !go_on {
            self.cancel.store(true, Ordering::Relaxed);
        }
        self.decided.notify_all();
        true
    }

    fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let mut gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);
        gate.asking = false;
        self.decided.notify_all();
    }

    fn hold(&self, count: usize, task: &TaskHandle) {
        let mut gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);
        if gate.limit.is_some_and(|limit| count >= limit) {
            gate.limit = None;
            gate.asking = true;
            task.progress(None, "");
        }
        while gate.asking && !self.cancel.load(Ordering::Relaxed) {
            gate = self.decided.wait_timeout(gate, Duration::from_millis(50)).unwrap_or_else(PoisonError::into_inner).0;
        }
        gate.asking = false;
    }

    fn found(&self) -> Vec<Found> {
        self.found.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Text,
    Usages(UsagesTarget),
    File(PathBuf),
    Provided(provided::Finder),
}

struct ResultsTab {
    id: u64,
    replaces: Option<u64>,
    source: Source,
    lookup: u64,
    params: SearchParams,
    replacement: Option<String>,
    scope: String,
    progress: Arc<Progress>,
    excluded: HashSet<Key>,
    collapsed: HashSet<String>,
    selected: Option<String>,
    pinned: bool,
    number: usize,
}

#[derive(Default)]
pub struct FindResults {
    tabs: Vec<ResultsTab>,
    next: u64,
    generation: u64,
    active: Option<u64>,
    waiting: Option<usages::Waiting>,
    recent: Vec<UsagesTarget>,
    access: Vec<usages::Access>,
}

impl FindResults {
    pub fn signature(&self) -> u64 {
        self.tabs.iter().fold(self.generation.wrapping_mul(31), |sum, tab| {
            let progress = &tab.progress;
            let state = progress.count.load(Ordering::Relaxed) * 4
                + u64::from(progress.done()) * 2
                + u64::from(progress.asking());
            sum.wrapping_mul(1_000_003).wrapping_add(tab.id * 7919 + state)
        })
    }

    pub fn tabs(&self) -> Vec<(u64, Option<u64>)> {
        self.tabs.iter().map(|tab| (tab.id, tab.replaces)).collect()
    }

    pub fn active(&self) -> Option<u64> {
        self.active
    }

    pub fn asking(&self) -> Option<u64> {
        self.tabs.iter().find(|tab| tab.progress.asking()).map(|tab| tab.id)
    }

    pub fn searching(&self, id: u64) -> bool {
        self.tabs.iter().any(|tab| tab.id == id && !tab.progress.done())
    }

    pub fn classifying(&self) -> bool {
        !self.access.is_empty()
    }

    pub fn running(&self) -> bool {
        self.waiting.is_some() || self.tabs.iter().any(|tab| !tab.progress.done())
    }

    fn tab(&self, id: Option<u64>) -> Result<&ResultsTab, CommandError> {
        let id = id.or(self.active).or_else(|| self.tabs.last().map(|tab| tab.id));
        self.tabs.iter().find(|tab| Some(tab.id) == id).ok_or_else(|| CommandError::new(t!("workbench-results-none")))
    }

    fn tab_mut(&mut self, id: Option<u64>) -> Result<&mut ResultsTab, CommandError> {
        let id = self.tab(id)?.id;
        self.generation += 1;
        self.tabs.iter_mut().find(|tab| tab.id == id).ok_or_else(|| CommandError::new(t!("workbench-results-none")))
    }
}

impl Drop for FindResults {
    fn drop(&mut self) {
        for tab in &self.tabs {
            tab.progress.stop();
        }
    }
}

/// What kind of node of the results tree a node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    /// "Targets", over what was searched for.
    Targets,
    /// What was searched for: "Occurrences of '…'".
    Target,
    /// "Found occurrences in `<scope>`", over every usage.
    Found,
    /// A Usage Type group.
    UsageType,
    /// Production or Test.
    Scope,
    /// A module.
    Module,
    /// A folder, or a chain of single folders.
    Directory,
    /// A file.
    File,
    /// A declaration of the file that holds usages (File Structure).
    Member,
    /// A usage, or with Merge Usages from the Same Line the usages of a line.
    Usage,
}

/// A node of the results tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultNode {
    /// The node's id within its tab, which the other `search.results.*` commands take.
    pub id: String,
    /// What it is.
    pub kind: NodeKind,
    /// Its text: a name, a folder path, or a usage's line, trimmed.
    pub label: String,
    /// What is shown after it, greyed: a file's folder when the files are not grouped by folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The usages under it (1 for a usage, the line's for a merged line).
    pub count: usize,
    /// Whether it is excluded: a group when every usage under it is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub excluded: bool,
    /// Whether it is collapsed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
    /// Whether it is a test file or under one, as the Project tool window tints them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub test: bool,
    /// A usage's or a file's file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// A usage's line, one-based.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// A usage's first character as a character offset in its file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    /// The character after a usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<usize>,
    /// The matched characters within `label`, as character ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<[usize; 2]>,
    /// For a usage in no file (a database object), the command that opens what holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<NodeCommand>,
    /// The nodes under it, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ResultNode>,
    #[serde(skip)]
    #[schemars(skip)]
    keys: Vec<Key>,
}

impl ResultNode {
    fn new(id: String, kind: NodeKind, label: String) -> Self {
        Self {
            id,
            kind,
            label,
            detail: None,
            count: 0,
            excluded: false,
            collapsed: false,
            test: false,
            path: None,
            line: None,
            offset: None,
            end: None,
            highlights: Vec::new(),
            command: None,
            children: Vec::new(),
            keys: Vec::new(),
        }
    }

    fn find(&self, id: &str) -> Option<&ResultNode> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    fn keys(&self) -> Vec<Key> {
        let mut keys = self.keys.clone();
        for child in &self.children {
            keys.extend(child.keys());
        }
        keys
    }

    fn walk<'a>(&'a self, out: &mut Vec<&'a ResultNode>) {
        out.push(self);
        for child in &self.children {
            child.walk(out);
        }
    }

    fn prune(&mut self, depth: usize) {
        if depth == 0 {
            self.children.clear();
        } else {
            for child in &mut self.children {
                child.prune(depth - 1);
            }
        }
    }
}

/// The parameters of Open in Find Window.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OccurrencesParams {
    /// What and where, as `search.text.start` takes it; `limit` is where Too Many Results asks
    /// (the setting search.findInFiles.tooManyResults, 1000, when absent; 0 never asks).
    #[serde(flatten)]
    pub search: SearchParams,
    /// The replacement, for Replace in Files' Open in Find Window: the tab then offers Replace
    /// Selected and Do Replace All.
    pub replacement: Option<String>,
    /// Whether the results open on a tab of their own (Open results in new tab); otherwise they take
    /// the place of the last tab of results that is not pinned.
    pub new_tab: Option<bool>,
    /// How long to wait for the search to end or to ask Too Many Results, at most 10 000 ms; none by default.
    pub wait_ms: Option<u64>,
}

/// What a Find Usages looked for: the symbol at a place, and where its usages were looked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsagesTarget {
    /// The file the search started in.
    pub path: PathBuf,
    /// The character offset it started at.
    pub offset: usize,
    /// The symbol, as written there.
    pub symbol: String,
    /// What the symbol is.
    pub kind: crate::language::show_usages::DeclarationKind,
    /// Where the usages were looked for.
    pub scope: UsagesScope,
    /// The text of the line it started at, trimmed, which finds the symbol again after edits.
    #[serde(skip)]
    #[schemars(skip)]
    pub line: String,
    /// Its column in that trimmed line, in characters.
    #[serde(skip)]
    #[schemars(skip)]
    pub column: usize,
}

/// What a tab of the Find tool window lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ResultsKind {
    /// The occurrences of a Find in Files or Replace in Files search ("Occurrences of '…'").
    Text,
    /// The usages of a symbol, from Find Usages ("Usages of …").
    Usages,
    /// The occurrences of the search bar's query in one file (Open Results in Find Window).
    File,
    /// The usages of a database table, view, routine or column (db.usages.find, db.usages.rename).
    Database,
}

/// A tab of results, as the commands report it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultsInfo {
    /// The tab's id, which the other `search.results.*` commands take as `tab`.
    pub tab: u64,
    /// Kept for agents of the earlier Open in Find Window: the same as `tab`.
    pub search: u64,
    /// The tab's title: "Occurrences of '…'", or "Usages of …".
    pub title: String,
    /// What the tab lists.
    pub kind: ResultsKind,
    /// For a tab of usages, what Find Usages looked for (Rerun and Settings take it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usages: Option<UsagesTarget>,
    /// Whether the language server told the access (read or write) of some usage: Show Read Access
    /// and Show Write Access then filter them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub access_known: bool,
    /// Whether the language server is still being asked for the access of the usages, file by file:
    /// until then some usages stay Unclassified.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub access_pending: bool,
    /// What was searched for.
    pub query: String,
    /// The replacement of a Replace in Files tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    /// How many usages it lists (those in comments too, whether shown or not).
    pub total: usize,
    /// In how many files.
    pub files: usize,
    /// How many of them are in no file, such as in a database's own objects.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub elsewhere: usize,
    /// Whether the search is over.
    pub done: bool,
    /// Whether the search is paused at Too Many Results, waiting for search.results.tooMany.
    pub more: bool,
    /// The file being searched while the search runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub searching: Option<String>,
    /// Whether the tab is pinned: new results never take its place.
    pub pinned: bool,
    /// Why the query cannot be used, if it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The parameters of the search, as search.text.occurrences took them (Rerun, Settings).
    pub request: serde_json::Value,
}

fn info(tab: &ResultsTab) -> ResultsInfo {
    let found = tab.progress.found();
    let files =
        found.iter().filter(|one| one.holder.is_none()).map(|one| &one.found.path).collect::<HashSet<_>>().len();
    let elsewhere = found.iter().filter(|one| one.holder.is_some()).count();
    let done = tab.progress.done();
    let current = tab.progress.current.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let mut request = serde_json::to_value(&tab.params).unwrap_or_default();
    if let Some(replacement) = &tab.replacement {
        request["replacement"] = serde_json::json!(replacement);
    }
    let (kind, usages) = match &tab.source {
        Source::Text => (ResultsKind::Text, None),
        Source::Usages(target) => {
            request = serde_json::to_value(target).unwrap_or_default();
            (ResultsKind::Usages, Some(target.clone()))
        }
        Source::File(path) => {
            request["path"] = serde_json::json!(path);
            (ResultsKind::File, None)
        }
        Source::Provided(finder) => {
            request = finder.0.request();
            if let Some(replacement) = &tab.replacement {
                request["newName"] = serde_json::json!(replacement);
            }
            (ResultsKind::Database, None)
        }
    };
    ResultsInfo {
        tab: tab.id,
        search: tab.id,
        title: title(tab),
        kind,
        usages,
        access_known: found.iter().any(|one| one.access.is_some()),
        access_pending: tab.progress.classifying.load(Ordering::Relaxed),
        query: tab.params.query.text.clone(),
        replacement: tab.replacement.clone(),
        total: found.len(),
        files,
        elsewhere,
        done,
        more: tab.progress.asking(),
        searching: (!done && !current.is_empty()).then_some(current),
        pinned: tab.pinned,
        error: tab.progress.error.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        request,
    }
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

fn shown_text(text: &str) -> String {
    text.replace('\n', " ⏎ ")
}

fn title(tab: &ResultsTab) -> String {
    let title = base_title(tab);
    if tab.number > 1 { t!("workbench-results-numbered", title = title, number = tab.number) } else { title }
}

fn base_title(tab: &ResultsTab) -> String {
    match &tab.source {
        Source::Usages(target) => t!("workbench-results-usages-title", symbol = target.symbol.as_str()),
        Source::Provided(finder) => finder.0.title(),
        Source::Text | Source::File(_) => t!("workbench-results-title", text = shown_text(&tab.params.query.text)),
    }
}

fn tab_scope(session: &Session, tab: &ResultsTab) -> String {
    match &tab.source {
        Source::Text => scope_name(session, &tab.params),
        Source::Usages(target) => usages::scope_label(target.scope),
        Source::Provided(finder) => finder.0.scope(),
        Source::File(path) => path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

fn scope_name(session: &Session, params: &SearchParams) -> String {
    if !params.directory.trim().is_empty() {
        return t!("workbench-results-in-directory", directory = params.directory.trim().trim_matches('/'));
    }
    match params.scope.as_deref() {
        None | Some(PROJECT_FILES) => t!("workbench-results-in-project"),
        Some(id) => crate::search_scopes::scopes(session, true)
            .into_iter()
            .find(|scope| scope.id == id)
            .map_or_else(|| id.to_owned(), |scope| scope.name),
    }
}

fn usage_type_of(kind: Option<HighlightKind>) -> UsageType {
    match kind {
        Some(kind) if kind.is_comment() => UsageType::Comment,
        Some(kind) if kind.is_string() => UsageType::String,
        _ => UsageType::Unclassified,
    }
}

fn holds_members(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Struct
            | SymbolKind::Interface
            | SymbolKind::Type
            | SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Class
    )
}

fn members_at(items: &[OutlineItem], offset: usize, line_of: &dyn Fn(usize) -> usize, out: &mut Vec<(usize, String)>) {
    if let Some(item) = items.iter().find(|item| item.start <= offset && offset < item.end) {
        if holds_members(item.kind) {
            out.push((line_of(item.start), item.name.clone()));
        }
        members_at(&item.children, offset, line_of, out);
    }
}

type Infos = Mutex<BTreeMap<Key, (UsageType, Vec<(usize, String)>)>>;

fn classify(
    path: &Path,
    text: &str,
    ranges: &[Range<usize>],
    source: Option<&crate::brackets::HighlighterSource>,
    outliners: &[Arc<dyn Outliner>],
    infos: &Infos,
) {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let spans = source.and_then(|source| source(&name)).map(|found| found.highlight(text)).unwrap_or_default();
    let outline = outliners
        .iter()
        .find(|outliner| outliner.handles(&name))
        .map(|outliner| outliner.outline(text))
        .unwrap_or_default();
    let mut line_starts = vec![0];
    let mut bytes = Vec::with_capacity(ranges.len());
    let mut next = ranges.iter().map(|range| range.start).peekable();
    for (index, (byte, c)) in text.char_indices().enumerate() {
        while next.peek().is_some_and(|start| *start == index) {
            bytes.push(byte);
            next.next();
        }
        if c == '\n' {
            line_starts.push(index + 1);
        }
    }
    bytes.resize(ranges.len(), text.len());
    let line_of = |offset: usize| line_starts.partition_point(|start| *start <= offset).saturating_sub(1);
    let mut infos = infos.lock().unwrap_or_else(PoisonError::into_inner);
    for (range, byte) in ranges.iter().zip(bytes) {
        let mut members = Vec::new();
        members_at(&outline, range.start, &line_of, &mut members);
        infos.insert((path.to_owned(), range.start), (usage_type_of(zen_syntax::kind_at(&spans, byte)), members));
    }
}

fn start(session: &mut Session, tab: &mut ResultsTab) -> Result<(), CommandError> {
    match tab.source.clone() {
        Source::Text => start_text(session, tab),
        Source::Usages(target) => usages::restart(session, tab, &target),
        Source::File(path) => usages::file_occurrences(session, tab, &path),
        Source::Provided(finder) => provided::start(session, tab, &finder.0),
    }
}

fn start_text(session: &mut Session, tab: &mut ResultsTab) -> Result<(), CommandError> {
    let keep = crate::search::keeping(crate::search::highlighters(session), tab.params.query.context);
    let source = crate::search::highlighters(session);
    let outliners = session.state::<Outliners>().map(Outliners::all).unwrap_or_default();
    let limit = tab.params.limit.unwrap_or_else(|| session.settings().get_or(TOO_MANY, DEFAULT_TOO_MANY));
    let mut search = tab.params.search(session, 0)?;
    search.limit = 0;
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let index = crate::search::index(workbench)?;
    let open = crate::search::open_texts(workbench);
    let progress = Arc::new(Progress::default());
    progress.gate.lock().unwrap_or_else(PoisonError::into_inner).limit = (limit > 0).then_some(limit);
    tab.progress = Arc::clone(&progress);
    let text = shown_text(&tab.params.query.text);
    let task =
        session.state_mut::<Tasks>().start(&t!("workbench-results-task", text = text.as_str()), TASK_SOURCE, false);
    task.cancel_with(Arc::clone(&progress.cancel));
    let run = move || {
        let infos: Arc<Infos> = Arc::new(Mutex::new(BTreeMap::new()));
        let classified = Arc::clone(&infos);
        let keeping = move |path: &Path, text: &str, ranges: Vec<Range<usize>>| {
            let ranges = keep(path, text, ranges);
            if !ranges.is_empty() {
                classify(path, text, &ranges, source.as_ref(), &outliners, &classified);
            }
            ranges
        };
        let shown = Mutex::new(Instant::now() - PROGRESS_EVERY);
        let visit = |relative: &str| {
            *progress.current.lock().unwrap_or_else(PoisonError::into_inner) = relative.to_owned();
            let mut shown = shown.lock().unwrap_or_else(PoisonError::into_inner);
            if shown.elapsed() >= PROGRESS_EVERY {
                *shown = Instant::now();
                task.progress(None, &t!("workbench-results-task-in", file = relative));
            }
        };
        let sink = |matches: Vec<TextMatch>| {
            let count = {
                let mut infos = infos.lock().unwrap_or_else(PoisonError::into_inner);
                let mut found = progress.found.lock().unwrap_or_else(PoisonError::into_inner);
                for one in matches {
                    let (usage_type, members) = infos
                        .remove(&(one.path.clone(), one.range.start))
                        .unwrap_or((UsageType::Unclassified, Vec::new()));
                    found.push(Found { found: one, usage_type, members, access: None, holder: None });
                }
                found.len()
            };
            progress.count.fetch_max(count as u64, Ordering::Relaxed);
            progress.hold(count, &task);
        };
        let result = index.search_text_visiting(&search, &open, &progress.cancel, &sink, &keeping, &visit);
        {
            let mut found = progress.found.lock().unwrap_or_else(PoisonError::into_inner);
            let sorted = crate::search::sorted(found.iter().map(|one| one.found.clone()).collect());
            let mut by_key: BTreeMap<Key, Found> =
                found.drain(..).map(|one| ((one.found.path.clone(), one.found.range.start), one)).collect();
            found.extend(sorted.iter().filter_map(|one| by_key.remove(&(one.path.clone(), one.range.start))));
            progress.count.store(found.len() as u64, Ordering::Relaxed);
        }
        let count = progress.count.load(Ordering::Relaxed);
        let stopped = progress.cancel.load(Ordering::Relaxed);
        match result {
            Err(error) => {
                *progress.error.lock().unwrap_or_else(PoisonError::into_inner) = Some(error.clone());
                task.finish(Err(error));
            }
            Ok(_) if stopped => task.finish(Ok(t!("workbench-results-task-stopped", count = count))),
            Ok(_) => task.finish(Ok(t!("workbench-results-task-done", count = count))),
        }
        progress.done.store(true, Ordering::Relaxed);
        progress.current.lock().unwrap_or_else(PoisonError::into_inner).clear();
        task.progress(None, "");
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(run);
    #[cfg(target_arch = "wasm32")]
    run();
    Ok(())
}

fn wait_for(session: &mut Session, id: u64, wait_ms: Option<u64>) {
    let wait = Duration::from_millis(wait_ms.unwrap_or(0).min(MAX_WAIT));
    let Some((progress, lookup)) = session
        .state::<FindResults>()
        .and_then(|results| results.tabs.iter().find(|tab| tab.id == id))
        .map(|tab| (Arc::clone(&tab.progress), matches!(tab.source, Source::Usages(_))))
    else {
        return;
    };
    let until = Instant::now() + wait;
    if lookup {
        if !progress.done() {
            crate::language::usages::poll_usages(session, wait);
        }
        usages::settle(session, id, until);
        return;
    }
    while !progress.done() && !progress.asking() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub struct OpenOccurrences;

impl Command for OpenOccurrences {
    type Params = OccurrencesParams;
    type Output = ResultsInfo;
    const ID: &'static str = "search.text.occurrences";
    const TITLE: &'static str = "Open in Find Window";
    const DESCRIPTION: &'static str = "Searches the project as Find in Files does, in the background (a cancellable \
         task, \"Searching for '…' in <file>…\" in the status bar), and lists the matches on a tab \"Occurrences of \
         '…'\" of the Find tool window, in place of the last tab that is not pinned unless `newTab`; with \
         `replacement` the tab replaces them (search.results.replace). Past `limit` (the setting \
         search.findInFiles.tooManyResults, 1000) the search pauses and asks Too Many Results (`more`): \
         search.results.tooMany continues or aborts. `waitMs` waits for the end or the question. \
         search.results.tree reads the tree.";

    fn run(&self, session: &mut Session, params: OccurrencesParams) -> Result<ResultsInfo, CommandError> {
        if params.search.query.text.is_empty() {
            return Err(CommandError::new(t!("workbench-error-find-no-query")));
        }
        zen_text::search::find_all("", &params.search.query)
            .map_err(|error| CommandError::new(t!("workbench-error-search", error = error)))?;
        let scope = scope_name(session, &params.search);
        let id = {
            let results = session.state_mut::<FindResults>();
            results.next += 1;
            results.next
        };
        let mut tab = ResultsTab {
            id,
            replaces: None,
            source: Source::Text,
            lookup: 0,
            params: params.search,
            replacement: params.replacement,
            scope,
            progress: Arc::new(Progress::default()),
            excluded: HashSet::new(),
            collapsed: HashSet::new(),
            selected: None,
            pinned: false,
            number: 1,
        };
        start(session, &mut tab)?;
        place(session.state_mut::<FindResults>(), tab, params.new_tab.unwrap_or(false));
        wait_for(session, id, params.wait_ms);
        let results = session.state_mut::<FindResults>();
        Ok(info(results.tab(Some(id))?))
    }
}

fn place(results: &mut FindResults, mut tab: ResultsTab, new_tab: bool) {
    let id = tab.id;
    let reuse = if new_tab { None } else { results.tabs.iter().rposition(|tab| !tab.pinned) };
    let title = base_title(&tab);
    let taken: HashSet<usize> = results
        .tabs
        .iter()
        .enumerate()
        .filter(|(index, other)| Some(*index) != reuse && base_title(other) == title)
        .map(|(_, other)| other.number)
        .collect();
    tab.number = (1..).find(|number| !taken.contains(number)).unwrap_or(1);
    match reuse {
        Some(index) => {
            let old = std::mem::replace(&mut results.tabs[index], tab);
            old.progress.stop();
            results.tabs[index].replaces = Some(old.id);
        }
        None => results.tabs.push(tab),
    }
    results.active = Some(id);
    results.generation += 1;
}

/// The parameters of the commands on a tab.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TabParams {
    /// The tab; the one shown in the Find tool window, else the latest, when absent.
    pub tab: Option<u64>,
}

/// What `search.results.list` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultsList {
    /// The tabs, in order.
    pub tabs: Vec<ResultsInfo>,
    /// The tab shown.
    pub active: Option<u64>,
    /// The Group By and view options.
    pub options: ViewOptions,
}

pub struct ListResults;

impl Command for ListResults {
    type Params = NoParams;
    type Output = ResultsList;
    const ID: &'static str = "search.results.list";
    const TITLE: &'static str = "Find Window Occurrences Tabs";
    const DESCRIPTION: &'static str = "The \"Occurrences of '…'\" tabs of the Find tool window with each search's state \
         (`done`, `more` while Too Many Results asks, `searching` the file being read), and the Group By options.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ResultsList, CommandError> {
        crate::language::usages::poll_usages(session, Duration::ZERO);
        let options = ViewOptions::of(session);
        let results = session.state_mut::<FindResults>();
        Ok(ResultsList { tabs: results.tabs.iter().map(info).collect(), active: results.active, options })
    }
}

/// The parameters of `search.results.tree`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TreeParams {
    /// The tab; the one shown, else the latest, when absent.
    pub tab: Option<u64>,
    /// Only this many levels of nodes (0 is the top ones); every level when absent.
    pub depth: Option<usize>,
    /// How long to wait for the search to end or to ask Too Many Results, at most 10 000 ms.
    pub wait_ms: Option<u64>,
}

/// What `search.results.tree` answers.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultsTree {
    /// The tab and its search.
    #[serde(flatten)]
    pub info: ResultsInfo,
    /// The Group By and view options it is grouped by.
    pub options: ViewOptions,
    /// The node selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// The top nodes: Targets, then "Found occurrences in `<scope>`" with the usages under it.
    pub nodes: Vec<ResultNode>,
}

struct Shape {
    options: ViewOptions,
    modules: Vec<ModuleScope>,
    project: String,
}

impl Shape {
    fn of(session: &Session) -> Self {
        let mut modules = crate::search_scopes::modules(session);
        modules.sort_by_key(|module| std::cmp::Reverse(module.folder.len()));
        let project = session
            .state::<Workbench>()
            .and_then(Workbench::project)
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self { options: ViewOptions::of(session), modules, project }
    }

    fn modules_of(&self, relative: &str) -> Vec<&ModuleScope> {
        let inside = |module: &&ModuleScope| {
            module.folder.is_empty()
                || relative == module.folder
                || relative.starts_with(&format!("{}/", module.folder))
        };
        let mut chain: Vec<&ModuleScope> = self.modules.iter().filter(inside).collect();
        chain.reverse();
        chain
    }
}

struct Segment {
    id: String,
    kind: NodeKind,
    label: String,
    detail: Option<String>,
    path: Option<PathBuf>,
}

fn segment(id: String, kind: NodeKind, label: String) -> Segment {
    Segment { id, kind, label, detail: None, path: None }
}

fn group_path(found: &Found, shape: &Shape, test: bool) -> Vec<Segment> {
    let options = &shape.options;
    let one = &found.found;
    let mut path = Vec::new();
    if options.usage_type {
        let (id, label) = match found.usage_type {
            UsageType::Unclassified => ("type:unclassified", t!("workbench-results-unclassified")),
            UsageType::Read => ("type:read", t!("workbench-results-read")),
            UsageType::Write => ("type:write", t!("workbench-results-write")),
            UsageType::Import => ("type:import", t!("workbench-results-import")),
            UsageType::Comment => ("type:comment", t!("workbench-results-in-comments")),
            UsageType::String => ("type:string", t!("workbench-results-in-strings")),
        };
        path.push(segment(id.to_owned(), NodeKind::UsageType, label));
    }
    if options.production_test {
        let (id, label) = if test {
            ("scope:test", t!("workbench-results-test"))
        } else {
            ("scope:production", t!("workbench-results-production"))
        };
        path.push(segment(id.to_owned(), NodeKind::Scope, label));
    }
    if let Some(holder) = &found.holder {
        path.push(segment(format!("db:{}", holder.group), NodeKind::Module, holder.group.clone()));
        path.push(segment(format!("holder:{}", holder.key), NodeKind::Member, holder.holder.clone()));
        return path;
    }
    let mut folder = one.relative.rsplit_once('/').map_or("", |(folder, _)| folder).to_owned();
    if options.module {
        let chain = shape.modules_of(&one.relative);
        let chain: Vec<&ModuleScope> =
            if options.flatten_modules { chain.last().copied().into_iter().collect() } else { chain };
        for module in &chain {
            let label = if module.name.is_empty() { shape.project.clone() } else { module.name.clone() };
            path.push(segment(format!("module:{}", module.name), NodeKind::Module, label));
        }
        if let Some(module) = chain.last()
            && !module.folder.is_empty()
        {
            folder = folder.strip_prefix(&module.folder).unwrap_or(&folder).trim_start_matches('/').to_owned();
        }
    }
    let name = one.relative.rsplit('/').next().unwrap_or(&one.relative).to_owned();
    let mut file = segment(format!("file:{}", one.relative), NodeKind::File, name);
    file.path = Some(one.path.clone());
    if options.directory_structure && !folder.is_empty() {
        let mut at = String::new();
        for part in folder.split('/') {
            at = if at.is_empty() { part.to_owned() } else { format!("{at}/{part}") };
            path.push(segment(format!("dir:{at}"), NodeKind::Directory, part.to_owned()));
        }
    } else if options.directory && !folder.is_empty() {
        path.push(segment(format!("dir:{folder}"), NodeKind::Directory, folder.clone()));
    } else if !options.directory_structure && !options.directory && !folder.is_empty() {
        file.detail = Some(folder.clone());
    }
    path.push(file);
    if options.file_structure {
        for (line, name) in &found.members {
            path.push(segment(format!("member:{line}"), NodeKind::Member, name.clone()));
        }
    }
    path
}

fn usage_label(found: &TextMatch, extra: &[&TextMatch]) -> (String, Vec<[usize; 2]>) {
    let line = found.line_text.as_str();
    let lead = line.chars().take_while(|c| c.is_whitespace()).count();
    let label = line.trim().to_owned();
    let length = label.chars().count();
    let highlights = std::iter::once(found)
        .chain(extra.iter().copied())
        .map(|one| {
            let from = one.column.saturating_sub(lead).min(length);
            [from, (from + one.range.len()).min(length)]
        })
        .collect();
    (label, highlights)
}

fn insert(children: &mut Vec<ResultNode>, parent: &str, path: &[Segment], leaf: ResultNode, test: bool) {
    let Some((first, rest)) = path.split_first() else {
        children.push(leaf);
        return;
    };
    let id = format!("{parent}/{}", first.id);
    let index = match children.iter().position(|child| child.id == id) {
        Some(index) => index,
        None => {
            let mut node = ResultNode::new(id.clone(), first.kind, first.label.clone());
            node.detail = first.detail.clone();
            node.path = first.path.clone();
            children.push(node);
            children.len() - 1
        }
    };
    let node = &mut children[index];
    node.test |= test && matches!(node.kind, NodeKind::File | NodeKind::Member);
    insert(&mut node.children, &id, rest, leaf, test);
}

fn rank(node: &ResultNode) -> (u8, String) {
    let order = match node.kind {
        NodeKind::UsageType => match node.id.rsplit('/').next() {
            Some("type:unclassified") => 0,
            Some("type:read") => 1,
            Some("type:write") => 2,
            Some("type:import") => 3,
            Some("type:comment") => 4,
            _ => 5,
        },
        NodeKind::Scope => u8::from(node.id.ends_with("scope:test")),
        NodeKind::Module => 0,
        NodeKind::Directory => 1,
        NodeKind::File => 2,
        _ => return (3, String::new()),
    };
    (order, node.label.to_lowercase())
}

fn finish(node: &mut ResultNode, excluded: &HashSet<Key>, collapsed: &HashSet<String>) {
    for child in &mut node.children {
        finish(child, excluded, collapsed);
    }
    if node.kind == NodeKind::Directory {
        while node.children.len() == 1 && node.children[0].kind == NodeKind::Directory {
            let child = node.children.remove(0);
            node.label = format!("{}/{}", node.label, child.label);
            node.id = child.id;
            node.children = child.children;
        }
    }
    node.children.sort_by_key(rank);
    if node.kind != NodeKind::Usage {
        node.count = node.children.iter().map(|child| child.count).sum();
        node.excluded = !node.children.is_empty() && node.children.iter().all(|child| child.excluded);
    } else {
        node.count = node.keys.len();
        node.excluded = node.keys.iter().all(|key| excluded.contains(key));
    }
    node.collapsed = collapsed.contains(&node.id) && !node.children.is_empty();
}

fn found_label(tab: &ResultsTab, scope: &str) -> String {
    match tab.source {
        Source::Usages(_) | Source::Provided(_) => t!("workbench-results-found-usages", scope = scope),
        Source::Text | Source::File(_) => t!("workbench-results-found", scope = scope),
    }
}

fn tree(tab: &ResultsTab, shape: &Shape) -> Vec<ResultNode> {
    let found = tab.progress.found();
    let mut targets = ResultNode::new("targets".to_owned(), NodeKind::Targets, t!("workbench-results-targets"));
    let target_label = match (&tab.source, &tab.replacement) {
        (Source::Usages(target), _) => usages::target_label(target),
        (Source::Provided(finder), _) => finder.0.target(),
        (_, Some(replacement)) => t!(
            "workbench-results-target-replace",
            text = shown_text(&tab.params.query.text),
            replacement = shown_text(replacement)
        ),
        (_, None) => title(tab),
    };
    targets.children.push(ResultNode::new("targets/target".to_owned(), NodeKind::Target, target_label));
    let mut root = ResultNode::new("found".to_owned(), NodeKind::Found, found_label(tab, &tab.scope));
    let mut index = 0;
    let options = &shape.options;
    let live: Vec<&Found> = found
        .iter()
        .filter(|one| options.show_in_comments || one.usage_type != UsageType::Comment)
        .filter(|one| match one.access {
            Some(UsageAccess::Read) => options.show_read,
            Some(UsageAccess::Write) => options.show_write,
            None => true,
        })
        .collect();
    while index < live.len() {
        let first = live[index];
        let mut same = 1;
        while shape.options.merge_same_line
            && index + same < live.len()
            && live[index + same].found.path == first.found.path
            && live[index + same].found.line == first.found.line
            && live[index + same].usage_type == first.usage_type
        {
            same += 1;
        }
        let group = &live[index..index + same];
        index += same;
        let one = &first.found;
        let test = crate::search_scopes::is_test(&one.relative);
        let others: Vec<&TextMatch> = group[1..].iter().map(|other| &other.found).collect();
        let (label, highlights) = usage_label(one, &others);
        let id_part = if shape.options.merge_same_line {
            format!("line:{}", one.line + 1)
        } else {
            format!("usage:{}", one.range.start)
        };
        let path = group_path(first, shape, test);
        let parent = std::iter::once("found".to_owned())
            .chain(path.iter().map(|segment| segment.id.clone()))
            .collect::<Vec<_>>()
            .join("/");
        let mut leaf = ResultNode::new(format!("{parent}/{id_part}"), NodeKind::Usage, label);
        leaf.highlights = highlights;
        leaf.test = test;
        match &first.holder {
            Some(holder) => leaf.command = Some(holder.open.clone()),
            None => leaf.path = Some(one.path.clone()),
        }
        leaf.line = Some(one.line + 1);
        leaf.offset = Some(one.range.start);
        leaf.end = Some(one.range.end);
        leaf.keys = group.iter().map(|other| (other.found.path.clone(), other.found.range.start)).collect();
        insert(&mut root.children, "found", &path, leaf, test);
    }
    finish(&mut root, &tab.excluded, &tab.collapsed);
    finish(&mut targets, &tab.excluded, &tab.collapsed);
    targets.count = 0;
    vec![targets, root]
}

fn tree_of(
    session: &mut Session,
    tab: Option<u64>,
) -> Result<(ResultsInfo, Vec<ResultNode>, Option<String>), CommandError> {
    let shape = Shape::of(session);
    let scope = {
        let results = session.state::<FindResults>().ok_or_else(|| CommandError::new(t!("workbench-results-none")))?;
        tab_scope(session, results.tab(tab)?)
    };
    let results = session.state_mut::<FindResults>();
    let tab = results.tab(tab)?;
    let mut nodes = tree(tab, &shape);
    if let Some(found) = nodes.iter_mut().find(|node| node.kind == NodeKind::Found) {
        found.label = found_label(tab, &scope);
    }
    Ok((info(tab), nodes, tab.selected.clone()))
}

pub struct ResultsTreeCommand;

impl Command for ResultsTreeCommand {
    type Params = TreeParams;
    type Output = ResultsTree;
    const ID: &'static str = "search.results.tree";
    const TITLE: &'static str = "Find Window Occurrences Tree";
    const DESCRIPTION: &'static str = "The tree of an \"Occurrences of '…'\" tab as the Find tool window shows it: \
         Targets, then \"Found occurrences in <scope>\" with its usages grouped by usage type, Production/Test, \
         module, folder, file and declaration as the Group By options say, each node with an `id` the other \
         search.results.* commands take, its usage `count`, and whether it is `excluded`. `depth` limits the levels.";

    fn run(&self, session: &mut Session, params: TreeParams) -> Result<ResultsTree, CommandError> {
        crate::language::usages::poll_usages(session, Duration::ZERO);
        let id = session.state_mut::<FindResults>().tab(params.tab)?.id;
        wait_for(session, id, params.wait_ms);
        let (info, mut nodes, selected) = tree_of(session, Some(id))?;
        if let Some(depth) = params.depth {
            for node in &mut nodes {
                node.prune(depth);
            }
        }
        Ok(ResultsTree { info, options: ViewOptions::of(session), selected, nodes })
    }
}

/// The parameters of `search.results.tooMany`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TooManyParams {
    /// The tab; the one asking when absent.
    pub tab: Option<u64>,
    /// Continue (true) lists every match; Abort (false) stops the search and keeps those listed.
    #[serde(rename = "continue")]
    pub go_on: bool,
}

pub struct TooManyResults;

impl Command for TooManyResults {
    type Params = TooManyParams;
    type Output = ResultsInfo;
    const ID: &'static str = "search.results.tooMany";
    const TITLE: &'static str = "Too Many Results";
    const DESCRIPTION: &'static str = "Answers Too Many Results of a search paused at its limit (`more`): `continue: true` \
         goes on and lists every match, `continue: false` (Abort) stops it and keeps the matches listed so far.";

    fn run(&self, session: &mut Session, params: TooManyParams) -> Result<ResultsInfo, CommandError> {
        let results = session.state_mut::<FindResults>();
        let asking = params.tab.or_else(|| results.asking());
        let tab = results.tab(asking)?;
        let id = tab.id;
        if !tab.progress.decide(params.go_on) {
            return Err(CommandError::new(t!("workbench-results-not-asking")));
        }
        results.generation += 1;
        Ok(info(results.tab(Some(id))?))
    }
}

pub struct StopResults;

impl Command for StopResults {
    type Params = TabParams;
    type Output = ResultsInfo;
    const ID: &'static str = "search.results.stop";
    const TITLE: &'static str = "Stop Find Window Search";
    const DESCRIPTION: &'static str = "Stops the search of an \"Occurrences of '…'\" tab, as the × of its progress in the \
         status bar does; the usages found so far stay.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<ResultsInfo, CommandError> {
        let results = session.state_mut::<FindResults>();
        let tab = results.tab_mut(params.tab)?;
        tab.progress.stop();
        let lookup = matches!(tab.source, Source::Usages(_)) && !tab.progress.done();
        if lookup {
            tab.progress.done.store(true, Ordering::Relaxed);
        }
        let id = tab.id;
        if lookup {
            session.state_mut::<crate::language::Languages>().end_lookup(crate::language::LookupKind::Usages);
        }
        usages::stop_access(session, id);
        Ok(info(session.state_mut::<FindResults>().tab(Some(id))?))
    }
}

pub struct RerunResults;

impl Command for RerunResults {
    type Params = TreeParams;
    type Output = ResultsInfo;
    const ID: &'static str = "search.results.rerun";
    const TITLE: &'static str = "Rerun Find Window Search";
    const DESCRIPTION: &'static str = "Rerun (⌘R in the Find tool window): searches again for what a tab searched for, \
         in the same tab, forgetting what was excluded. `waitMs` waits for the end or for Too Many Results.";

    fn run(&self, session: &mut Session, params: TreeParams) -> Result<ResultsInfo, CommandError> {
        let mut tab = {
            let results = session.state_mut::<FindResults>();
            let id = results.tab(params.tab)?.id;
            let index = results.tabs.iter().position(|tab| tab.id == id).unwrap_or(0);
            let tab = &mut results.tabs[index];
            tab.progress.stop();
            ResultsTab {
                id: tab.id,
                replaces: tab.replaces,
                source: tab.source.clone(),
                lookup: 0,
                params: tab.params.clone(),
                replacement: tab.replacement.clone(),
                scope: tab.scope.clone(),
                progress: Arc::new(Progress::default()),
                excluded: HashSet::new(),
                collapsed: HashSet::new(),
                selected: None,
                pinned: tab.pinned,
                number: tab.number,
            }
        };
        start(session, &mut tab)?;
        let id = tab.id;
        let results = session.state_mut::<FindResults>();
        if let Some(slot) = results.tabs.iter_mut().find(|slot| slot.id == id) {
            *slot = tab;
        }
        results.generation += 1;
        wait_for(session, id, params.wait_ms);
        Ok(info(session.state_mut::<FindResults>().tab(Some(id))?))
    }
}

/// The parameters of `search.results.setOption`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionParams {
    /// The option.
    pub option: ViewOption,
    /// On or off; it turns over when absent.
    #[serde(default)]
    pub on: Option<bool>,
}

pub struct SetOption;

impl Command for SetOption {
    type Params = OptionParams;
    type Output = ViewOptions;
    const ID: &'static str = "search.results.setOption";
    const TITLE: &'static str = "Find Window Group By";
    const DESCRIPTION: &'static str = "Turns a Group By or view option of the Find tool window on or off (over when `on` \
         is absent): directoryStructure ⌃D, fileStructure ⌃S, module ⌃M, flattenModules ⌃O, directory ⌃P \
         (Group by Directory), productionTest, usageType ⌃T, mergeSameLine ⌃L, showInComments, previewSource. \
         Directory Structure and Group by Directory exclude each other. Answers every option.";

    fn run(&self, session: &mut Session, params: OptionParams) -> Result<ViewOptions, CommandError> {
        let mut options = ViewOptions::of(session);
        options.set(params.option, params.on.unwrap_or(!options.get(params.option)));
        session.settings_mut().set(OPTIONS, &options).map_err(|error| CommandError::new(error.to_string()))?;
        session.state_mut::<FindResults>().generation += 1;
        Ok(options)
    }
}

/// The parameters of the commands on nodes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NodesParams {
    /// The tab; the one shown, else the latest, when absent.
    pub tab: Option<u64>,
    /// The nodes, by the ids search.results.tree gives; the selected node when empty.
    pub nodes: Vec<String>,
}

/// What a command on nodes changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodesChanged {
    /// How many usages it changed.
    pub usages: usize,
}

fn keys_of(session: &mut Session, params: &NodesParams) -> Result<(u64, Vec<Key>), CommandError> {
    let shape = Shape::of(session);
    let results = session.state_mut::<FindResults>();
    let tab = results.tab(params.tab)?;
    let nodes = tree(tab, &shape);
    let ids: Vec<String> =
        if params.nodes.is_empty() { tab.selected.clone().into_iter().collect() } else { params.nodes.clone() };
    if ids.is_empty() {
        return Err(CommandError::new(t!("workbench-results-no-node")));
    }
    let mut keys = Vec::new();
    for id in &ids {
        let node = nodes
            .iter()
            .find_map(|node| node.find(id))
            .ok_or_else(|| CommandError::new(t!("workbench-results-no-such-node", node = id.as_str())))?;
        keys.extend(node.keys());
    }
    Ok((tab.id, keys))
}

pub struct ExcludeNodes;

impl Command for ExcludeNodes {
    type Params = NodesParams;
    type Output = NodesChanged;
    const ID: &'static str = "search.results.exclude";
    const TITLE: &'static str = "Exclude from Find Window";
    const DESCRIPTION: &'static str = "Exclude (⌫ in the Find tool window): leaves nodes of a tab (and every usage under \
         them) out of Replace Selected and Do Replace All; they stay listed, struck through. search.results.include \
         takes them back.";

    fn run(&self, session: &mut Session, params: NodesParams) -> Result<NodesChanged, CommandError> {
        let (id, keys) = keys_of(session, &params)?;
        let tab = session.state_mut::<FindResults>().tab_mut(Some(id))?;
        let usages = keys.into_iter().filter(|key| tab.excluded.insert(key.clone())).count();
        Ok(NodesChanged { usages })
    }
}

pub struct IncludeNodes;

impl Command for IncludeNodes {
    type Params = NodesParams;
    type Output = NodesChanged;
    const ID: &'static str = "search.results.include";
    const TITLE: &'static str = "Include in Find Window";
    const DESCRIPTION: &'static str = "Include: takes excluded nodes of a tab (and every usage under them) back into Replace \
         Selected and Do Replace All.";

    fn run(&self, session: &mut Session, params: NodesParams) -> Result<NodesChanged, CommandError> {
        let (id, keys) = keys_of(session, &params)?;
        let tab = session.state_mut::<FindResults>().tab_mut(Some(id))?;
        let usages = keys.into_iter().filter(|key| tab.excluded.remove(key)).count();
        Ok(NodesChanged { usages })
    }
}

/// The parameters of `search.results.select`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectParams {
    /// The tab, which becomes the one shown; the one shown, else the latest, when absent.
    pub tab: Option<u64>,
    /// The node; the selection stays when absent.
    pub node: Option<String>,
}

pub struct SelectNode;

impl Command for SelectNode {
    type Params = SelectParams;
    type Output = ();
    const ID: &'static str = "search.results.select";
    const TITLE: &'static str = "Select in Find Window";
    const DESCRIPTION: &'static str = "Shows a tab of the Find tool window and selects a node of it; Exclude, Include and \
         Replace Selected act on the selected node when given none.";

    fn run(&self, session: &mut Session, params: SelectParams) -> Result<(), CommandError> {
        let results = session.state_mut::<FindResults>();
        let tab = results.tab_mut(params.tab)?;
        if params.node.is_some() {
            tab.selected = params.node;
        }
        let id = tab.id;
        results.active = Some(id);
        Ok(())
    }
}

/// Where Next or Previous Occurrence went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Stepped {
    /// The usage's node, now selected.
    pub node: String,
    /// Its file.
    pub path: PathBuf,
    /// Its first character as a character offset.
    pub offset: usize,
    /// Its line, one-based.
    pub line: usize,
}

fn step(session: &mut Session, params: TabParams, forward: bool) -> Result<Stepped, CommandError> {
    let shape = Shape::of(session);
    let results = session.state_mut::<FindResults>();
    let tab = results.tab(params.tab)?;
    let nodes = tree(tab, &shape);
    let mut all = Vec::new();
    for node in &nodes {
        node.walk(&mut all);
    }
    let at = tab.selected.as_ref().and_then(|selected| all.iter().position(|node| &node.id == selected));
    let usages: Vec<(usize, &ResultNode)> = all
        .iter()
        .enumerate()
        .filter(|(_, node)| node.kind == NodeKind::Usage)
        .map(|(index, node)| (index, *node))
        .collect();
    let next = match (at, forward) {
        (None, true) => usages.first(),
        (None, false) => usages.last(),
        (Some(at), true) => usages.iter().find(|(index, _)| *index > at).or(usages.first()),
        (Some(at), false) => usages.iter().rev().find(|(index, _)| *index < at).or(usages.last()),
    }
    .map(|(_, node)| *node)
    .ok_or_else(|| CommandError::new(t!("workbench-results-no-usages")))?;
    let stepped = Stepped {
        node: next.id.clone(),
        path: next.path.clone().unwrap_or_default(),
        offset: next.offset.unwrap_or(0),
        line: next.line.unwrap_or(1),
    };
    let ancestors: Vec<String> = stepped.node.match_indices('/').map(|(at, _)| stepped.node[..at].to_owned()).collect();
    let id = tab.id;
    let tab = results.tab_mut(Some(id))?;
    for ancestor in ancestors {
        tab.collapsed.remove(&ancestor);
    }
    tab.selected = Some(stepped.node.clone());
    crate::history::NavigateTo.run(
        session,
        crate::history::NavigateToParams { path: stepped.path.clone(), offset: stepped.offset, end: None },
    )?;
    Ok(stepped)
}

pub struct NextOccurrence;

impl Command for NextOccurrence {
    type Params = TabParams;
    type Output = Stepped;
    const ID: &'static str = "search.results.next";
    const TITLE: &'static str = "Next Occurrence in Find Window";
    const DESCRIPTION: &'static str = "Next Occurrence (⌥⌘↓, the ↓ of the Find tool window): selects the usage after the \
         selected node, round to the first, and goes to it in the editor.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<Stepped, CommandError> {
        step(session, params, true)
    }
}

pub struct PreviousOccurrence;

impl Command for PreviousOccurrence {
    type Params = TabParams;
    type Output = Stepped;
    const ID: &'static str = "search.results.previous";
    const TITLE: &'static str = "Previous Occurrence in Find Window";
    const DESCRIPTION: &'static str = "Previous Occurrence (⌥⌘↑, the ↑ of the Find tool window): selects the usage before \
         the selected node, round to the last, and goes to it in the editor.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<Stepped, CommandError> {
        step(session, params, false)
    }
}

/// The parameters of `search.results.expand`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExpandParams {
    /// The tab; the one shown, else the latest, when absent.
    pub tab: Option<u64>,
    /// The nodes; every node when empty (Expand All, Collapse All).
    pub nodes: Vec<String>,
    /// Expand (true) or collapse (false).
    pub expanded: bool,
}

pub struct ExpandNodes;

impl Command for ExpandNodes {
    type Params = ExpandParams;
    type Output = ();
    const ID: &'static str = "search.results.expand";
    const TITLE: &'static str = "Expand or Collapse in Find Window";
    const DESCRIPTION: &'static str = "Expands or collapses nodes of a tab of the Find tool window; without `nodes`, every \
         node (Expand All ⌘+, Collapse All ⌘−: the top nodes stay).";

    fn run(&self, session: &mut Session, params: ExpandParams) -> Result<(), CommandError> {
        let shape = Shape::of(session);
        let results = session.state_mut::<FindResults>();
        let nodes = tree(results.tab(params.tab)?, &shape);
        let tab = results.tab_mut(params.tab)?;
        let ids: Vec<String> = if params.nodes.is_empty() {
            let mut all = Vec::new();
            for node in &nodes {
                for child in &node.children {
                    child.walk(&mut all);
                }
            }
            all.into_iter().filter(|node| !node.children.is_empty()).map(|node| node.id.clone()).collect()
        } else {
            params.nodes
        };
        if params.expanded {
            if ids.is_empty() {
                tab.collapsed.clear();
            }
            for id in ids {
                tab.collapsed.remove(&id);
            }
        } else {
            tab.collapsed.extend(ids);
        }
        Ok(())
    }
}

/// The parameters of `search.results.pin`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PinParams {
    /// The tab; the one shown, else the latest, when absent.
    pub tab: Option<u64>,
    /// Pinned or not; it turns over when absent.
    pub on: Option<bool>,
}

pub struct PinResults;

impl Command for PinResults {
    type Params = PinParams;
    type Output = bool;
    const ID: &'static str = "search.results.pin";
    const TITLE: &'static str = "Pin Find Window Tab";
    const DESCRIPTION: &'static str = "Pins or unpins a tab of the Find tool window (over when `on` is absent): new results \
         never take the place of a pinned tab. Answers whether it is pinned.";

    fn run(&self, session: &mut Session, params: PinParams) -> Result<bool, CommandError> {
        let tab = session.state_mut::<FindResults>().tab_mut(params.tab)?;
        tab.pinned = params.on.unwrap_or(!tab.pinned);
        Ok(tab.pinned)
    }
}

pub struct CloseResults;

impl Command for CloseResults {
    type Params = TabParams;
    type Output = ();
    const ID: &'static str = "search.results.close";
    const TITLE: &'static str = "Close Find Window Tab";
    const DESCRIPTION: &'static str = "Closes a tab of the Find tool window, stopping its search.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<(), CommandError> {
        let results = session.state_mut::<FindResults>();
        let id = results.tab(params.tab)?.id;
        let mut lookup = None;
        if let Some(index) = results.tabs.iter().position(|tab| tab.id == id) {
            let tab = results.tabs.remove(index);
            tab.progress.stop();
            if matches!(tab.source, Source::Usages(_)) && !tab.progress.done() {
                lookup = Some(tab.lookup);
            }
        }
        if results.active == Some(id) {
            results.active = results.tabs.last().map(|tab| tab.id);
        }
        results.generation += 1;
        if lookup.is_some()
            && lookup == session.state::<crate::language::usages::UsagesState>().map(|state| state.search)
        {
            session.state_mut::<crate::language::Languages>().end_lookup(crate::language::LookupKind::Usages);
        }
        usages::poll_access(session);
        Ok(())
    }
}

pub struct ReplaceResults;

impl Command for ReplaceResults {
    type Params = NodesParams;
    type Output = Replaced;
    const ID: &'static str = "search.results.replace";
    const TITLE: &'static str = "Replace in Find Window";
    const DESCRIPTION: &'static str = "Replace Selected (with `nodes`, or the selected node) or Do Replace All (with \
         `nodes: [\"found\"]`, every usage listed) of a Replace in Files tab: replaces the usages under the nodes \
         that are not excluded (nor hidden by Show Usages In Comments) with the tab's replacement. Each open file \
         changes in its editor as one undo step, any other on disk with a Local History version, keeping CRLF; a \
         read-only file, one not in UTF-8, or one changed since the search refuses the whole replacement. The \
         replaced usages leave the tree.";

    fn run(&self, session: &mut Session, params: NodesParams) -> Result<Replaced, CommandError> {
        let (id, keys) = keys_of(session, &params)?;
        let results = session.state_mut::<FindResults>();
        let tab = results.tab(Some(id))?;
        let replacement =
            tab.replacement.clone().ok_or_else(|| CommandError::new(t!("workbench-results-not-replace")))?;
        if !tab.progress.done() {
            return Err(CommandError::new(t!("workbench-results-replace-running")));
        }
        let query = tab.params.query.clone();
        let provided = matches!(tab.source, Source::Provided(_));
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let tab = session.state_mut::<FindResults>().tab(Some(id))?;
        let wanted: HashSet<Key> = keys.into_iter().filter(|key| !tab.excluded.contains(key)).collect();
        let mut by_file: BTreeMap<PathBuf, Vec<TextMatch>> = BTreeMap::new();
        for one in tab.progress.found() {
            let in_project = root.as_deref().is_some_and(|root| one.found.path.starts_with(root));
            if provided && (one.holder.is_some() || !in_project) {
                continue;
            }
            if wanted.contains(&(one.found.path.clone(), one.found.range.start)) {
                by_file.entry(one.found.path.clone()).or_default().push(one.found);
            }
        }
        if by_file.is_empty() {
            return Err(CommandError::new(t!("workbench-results-nothing-to-replace")));
        }
        let chosen: HashSet<Key> = by_file.values().flatten().map(|one| (one.path.clone(), one.range.start)).collect();
        let (replaced, rewritten) =
            crate::search::replace_matches(session, by_file.into_iter().collect(), &query, &replacement)?;
        let tab = session.state_mut::<FindResults>().tab_mut(Some(id))?;
        let mut found = tab.progress.found.lock().unwrap_or_else(PoisonError::into_inner);
        found.retain(|one| !chosen.contains(&(one.found.path.clone(), one.found.range.start)));
        for file in &rewritten {
            let lines = line_starts(&file.text);
            for one in found.iter_mut().filter(|one| one.found.path == file.path) {
                let start = shifted(&file.edits, one.found.range.start);
                let end = shifted(&file.edits, one.found.range.end);
                let line = lines.partition_point(|at| *at <= start).saturating_sub(1);
                one.found.range = start..end;
                one.found.line = line;
                one.found.column = start - lines[line];
                one.found.line_text = crate::search::line_text_of(&file.text, line).unwrap_or_default().to_owned();
            }
            tab.excluded = std::mem::take(&mut tab.excluded)
                .into_iter()
                .map(|(path, start)| {
                    let start = if path == file.path { shifted(&file.edits, start) } else { start };
                    (path, start)
                })
                .collect();
        }
        tab.progress.count.store(found.len() as u64, Ordering::Relaxed);
        drop(found);
        tab.selected = None;
        Ok(replaced)
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0).chain(text.chars().enumerate().filter(|(_, c)| *c == '\n').map(|(index, _)| index + 1)).collect()
}

fn shifted(edits: &[(Range<usize>, usize)], offset: usize) -> usize {
    let mut at = offset;
    for (range, length) in edits {
        if range.end <= offset {
            at = at + length - range.len();
        }
    }
    at
}

pub mod provided;
pub(crate) mod usages;

pub use provided::{Elsewhere, NodeCommand, UsageFinder, UsageRow};

pub use usages::{
    FindUsagesOptions, OPTIONS as USAGES_OPTIONS, OpenRecentFindUsages, OpenRecentParams, RecentFindUsages,
    RecentUsage, RecentUsages,
};

#[cfg(test)]
mod tests;
