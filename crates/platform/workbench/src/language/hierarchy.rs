use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::{LspError, Pending};
use zen_syntax::SymbolKind;

use super::places::{Place, TextCache};
use super::structure::symbol_kind;
use super::symbols::{ProjectRoot, RealDirs};
use super::usages::word_at;
use super::{Languages, LookupParams, Server, WaitParams, file_text, request_at, with_languages};
use crate::Workbench;
use crate::search_scopes::is_test;

const HOLDER: &str = "hierarchy";

const METHOD_NOT_FOUND: i64 = -32601;

/// What a hierarchy shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HierarchyKind {
    /// The calls of a function or method (Call Hierarchy).
    Call,
    /// The supertypes and subtypes of a type (Type Hierarchy).
    Type,
}

impl HierarchyKind {
    fn prepare(self) -> &'static str {
        match self {
            Self::Call => "textDocument/prepareCallHierarchy",
            Self::Type => "textDocument/prepareTypeHierarchy",
        }
    }

    fn capability(self) -> &'static str {
        match self {
            Self::Call => "callHierarchyProvider",
            Self::Type => "typeHierarchyProvider",
        }
    }

    fn unsupported(self, server: &str) -> String {
        match self {
            Self::Call => t!("workbench-hierarchy-unsupported-call", server = server.to_owned()),
            Self::Type => t!("workbench-hierarchy-unsupported-type", server = server.to_owned()),
        }
    }

    fn none(self) -> String {
        match self {
            Self::Call => t!("workbench-hierarchy-no-call"),
            Self::Type => t!("workbench-hierarchy-no-type"),
        }
    }

    fn fits(self, direction: HierarchyDirection) -> bool {
        match direction {
            HierarchyDirection::Callers | HierarchyDirection::Callees => self == Self::Call,
            HierarchyDirection::Supertypes | HierarchyDirection::Subtypes => self == Self::Type,
        }
    }
}

/// Which way a hierarchy goes from a node to its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HierarchyDirection {
    /// The functions that call the node (Caller Hierarchy).
    Callers,
    /// The functions the node calls (Callee Hierarchy).
    Callees,
    /// The types the node implements (Supertypes Hierarchy).
    Supertypes,
    /// The types that implement the node (Subtypes Hierarchy).
    Subtypes,
}

impl HierarchyDirection {
    fn method(self) -> &'static str {
        match self {
            Self::Callers => "callHierarchy/incomingCalls",
            Self::Callees => "callHierarchy/outgoingCalls",
            Self::Supertypes => "typeHierarchy/supertypes",
            Self::Subtypes => "typeHierarchy/subtypes",
        }
    }
}

/// Which nodes a hierarchy shows below its roots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HierarchyScope {
    /// The project's files that are not tests.
    Project,
    /// The project's test files, such as `*_test.go`.
    Test,
    /// Every file, the libraries' included.
    #[default]
    All,
}

impl HierarchyScope {
    fn shows(self, location: NodeLocation) -> bool {
        match self {
            Self::Project => location == NodeLocation::Project,
            Self::Test => location == NodeLocation::Test,
            Self::All => true,
        }
    }
}

/// Where a node's file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeLocation {
    /// In the project, not a test.
    Project,
    /// A test file of the project.
    Test,
    /// Outside the project: a library, the SDK.
    Library,
}

/// A function, method or type in a hierarchy.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyNode {
    /// Its id, for `navigate.hierarchy.expand`; never given to another node in the window, whatever its tab.
    pub id: usize,
    /// Its name.
    pub name: String,
    /// What it is.
    pub kind: SymbolKind,
    /// What the server tells besides the name: its package (gopls: `example.com/shop/cart • cart.go`).
    pub detail: Option<String>,
    /// The file it is declared in.
    pub path: PathBuf,
    /// Its whole declaration, as character offsets in the file.
    pub range: [usize; 2],
    /// Its name in the file, where to go to it.
    pub selection: Place,
    /// Whether its file is in the project, a test of the project, or outside the project.
    pub location: NodeLocation,
    /// The id of the node whose child it is; none for a root.
    pub parent: Option<usize>,
    /// In a call hierarchy, where the calls between it and its parent are, in the caller's file.
    pub calls: Vec<Place>,
    /// The ids of its children in the scope, once asked for; none until it is expanded; none listed for a recursive node.
    pub children: Option<Vec<usize>>,
    /// Whether its children are hidden (Collapse); they are then left out of `nodes`.
    pub collapsed: bool,
    /// Whether it is one of its own ancestors (a recursive call), which is never expanded.
    pub recursive: bool,
    /// Whether its children are being asked for.
    pub pending: bool,
    /// Why its children could not be had.
    pub error: Option<String>,
}

/// A tab of the Hierarchy tool window: one hierarchy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyTab {
    /// Its id, for the `tab` parameter of the hierarchy commands.
    pub tab: usize,
    /// What it shows.
    pub kind: HierarchyKind,
    /// Which way it goes.
    pub direction: Option<HierarchyDirection>,
    /// The name at the place it was asked for.
    pub symbol: Option<String>,
    /// Whether it is pinned: a new hierarchy never replaces it.
    pub pinned: bool,
}

/// A call or type hierarchy and its nodes expanded so far.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HierarchyState {
    /// Whether there is a hierarchy.
    pub open: bool,
    /// The id of its tab.
    pub tab: Option<usize>,
    /// Whether its tab is pinned.
    pub pinned: bool,
    /// What it shows.
    pub kind: Option<HierarchyKind>,
    /// Which way it goes; for a type hierarchy without one asked for, chosen once its root is known.
    pub direction: Option<HierarchyDirection>,
    /// Which nodes it shows below the roots.
    pub scope: HierarchyScope,
    /// Whether the children of each node are sorted by name (Sort Alphabetically) instead of as the server lists them.
    pub sort: bool,
    /// Whether a click on a node in the tool window goes to its code (Navigate with Single Click).
    pub single_click: bool,
    /// The name at the place it was asked for.
    pub symbol: Option<String>,
    /// Whether an answer of the server is still awaited.
    pub pending: bool,
    /// Whether the server offers this hierarchy; when it does not, `error` says so.
    pub supported: bool,
    /// The ids of the roots: what is at the place.
    pub roots: Vec<usize>,
    /// The nodes shown, in the tree's order: the roots, and the children in the scope of the nodes expanded and not collapsed.
    pub nodes: Vec<HierarchyNode>,
    /// How many children the scope hides.
    pub hidden: usize,
    /// Why there is nothing to show, in words for the user.
    pub error: Option<String>,
    /// Every tab of the Hierarchy tool window, in order.
    pub tabs: Vec<HierarchyTab>,
    /// The id of the tab selected in the tool window.
    pub active: Option<usize>,
}

struct Request {
    node: Option<usize>,
    server: String,
    pending: Pending,
}

struct Entry {
    node: HierarchyNode,
    item: Value,
}

type Key = Vec<(String, PathBuf)>;

const EXPAND_ALL_DEPTH: usize = 3;

const EXPAND_ALL_MAX: usize = 10;

const EXPAND_ALL_REQUESTS: usize = 256;

#[derive(Default)]
struct Hierarchy {
    tab: usize,
    pinned: bool,
    kind: Option<HierarchyKind>,
    direction: Option<HierarchyDirection>,
    scope: HierarchyScope,
    sort: bool,
    symbol: Option<String>,
    origin: Option<(PathBuf, usize)>,
    server: String,
    unsupported: bool,
    entries: BTreeMap<usize, Entry>,
    next: usize,
    roots: Vec<usize>,
    waiting: Vec<Request>,
    reopen: HashSet<Key>,
    expand_all: Option<usize>,
    budget: usize,
    error: Option<String>,
}

#[derive(Default)]
pub struct Hierarchies {
    tabs: BTreeMap<usize, Hierarchy>,
    active: Option<usize>,
    next_tab: usize,
    next_node: usize,
    scope: HierarchyScope,
    sort: bool,
    single_click: bool,
    revision: u64,
    started: u64,
}

impl Hierarchies {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn started(&self) -> u64 {
        self.started
    }

    pub fn pending(&self) -> bool {
        self.active.and_then(|tab| self.tabs.get(&tab)).is_some_and(|hierarchy| !hierarchy.waiting.is_empty())
    }

    pub fn scope(&self) -> HierarchyScope {
        self.scope
    }

    pub fn sort(&self) -> bool {
        self.sort
    }

    pub fn single_click(&self) -> bool {
        self.single_click
    }

    fn owner(&self, node: usize) -> Option<usize> {
        self.tabs.values().find(|hierarchy| hierarchy.entries.contains_key(&node)).map(|hierarchy| hierarchy.tab)
    }
}

fn cancel(languages: &Languages, requests: Vec<Request>) {
    for request in requests {
        if let Some(Server::Running { client, .. }) = languages.servers.get(&request.server) {
            client.cancel(&request.pending);
        }
    }
}

fn read_node(
    id: usize,
    item: &Value,
    parent: Option<usize>,
    languages: &Languages,
    cache: &mut TextCache,
    project: &mut ProjectRoot,
) -> Option<HierarchyNode> {
    let path = zen_lsp::position::uri_path(item["uri"].as_str()?)?;
    let range = cache.place(languages, &path, &item["range"])?;
    let selection = cache.place(languages, &path, &item["selectionRange"]).unwrap_or_else(|| range.clone());
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let location = if !project.contains(&path) {
        NodeLocation::Library
    } else if is_test(&name) {
        NodeLocation::Test
    } else {
        NodeLocation::Project
    };
    Some(HierarchyNode {
        id,
        name: item["name"].as_str()?.to_owned(),
        kind: symbol_kind(item["kind"].as_u64().unwrap_or(0)),
        detail: item["detail"].as_str().filter(|detail| !detail.is_empty()).map(str::to_owned),
        path,
        range: [range.start, range.end],
        selection,
        location,
        parent,
        calls: Vec::new(),
        children: None,
        collapsed: false,
        recursive: false,
        pending: false,
        error: None,
    })
}

fn call_places(path: &Path, ranges: &Value, languages: &Languages, cache: &mut TextCache) -> Vec<Place> {
    ranges
        .as_array()
        .map(|ranges| ranges.iter().filter_map(|range| cache.place(languages, path, range)).collect())
        .unwrap_or_default()
}

fn relocate(text: &str, name: &str, offset: usize) -> usize {
    if name.is_empty() {
        return offset;
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    text.match_indices(name)
        .filter(|(at, _)| !word(text[..*at].chars().next_back()) && !word(text[at + name.len()..].chars().next()))
        .map(|(at, _)| text[..at].chars().count())
        .min_by_key(|start| start.abs_diff(offset))
        .unwrap_or(offset)
}

impl Hierarchy {
    fn fresh(&self) -> Self {
        Self {
            tab: self.tab,
            pinned: self.pinned,
            scope: self.scope,
            sort: self.sort,
            next: self.next,
            ..Self::default()
        }
    }

    fn summary(&self) -> Option<HierarchyTab> {
        Some(HierarchyTab {
            tab: self.tab,
            kind: self.kind?,
            direction: self.direction,
            symbol: self.symbol.clone(),
            pinned: self.pinned,
        })
    }

    fn key(&self, id: usize) -> Key {
        let mut key = Vec::new();
        let mut at = Some(id);
        while let Some(entry) = at.and_then(|id| self.entries.get(&id)) {
            key.push((entry.node.name.clone(), entry.node.path.clone()));
            at = entry.node.parent;
        }
        key.reverse();
        key
    }

    fn depth(&self, id: usize) -> usize {
        let mut depth = 0;
        let mut at = self.entries.get(&id).and_then(|entry| entry.node.parent);
        while let Some(parent) = at {
            depth += 1;
            at = self.entries.get(&parent).and_then(|entry| entry.node.parent);
        }
        depth
    }

    fn is_ancestor(&self, parent: usize, node: &HierarchyNode) -> bool {
        let mut at = Some(parent);
        while let Some(entry) = at.and_then(|id| self.entries.get(&id)) {
            let same = entry.node.name == node.name
                && entry.node.path == node.path
                && entry.node.selection.start == node.selection.start;
            if same {
                return true;
            }
            at = entry.node.parent;
        }
        false
    }

    fn expanded(&self) -> HashSet<Key> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.node.children.is_some() && !entry.node.recursive)
            .map(|(id, _)| self.key(*id))
            .collect()
    }

    fn asked(&self, id: usize) -> bool {
        self.entries.get(&id).is_none_or(|entry| entry.node.children.is_some())
            || self.waiting.iter().any(|request| request.node == Some(id))
    }

    fn ordered(&self, ids: &[usize]) -> Vec<usize> {
        let mut ids = ids.to_vec();
        if self.sort {
            ids.sort_by_cached_key(|id| {
                self.entries.get(id).map(|entry| entry.node.name.to_lowercase()).unwrap_or_default()
            });
        }
        ids
    }

    fn view(&self) -> HierarchyState {
        let mut nodes = Vec::new();
        let mut hidden = 0;
        let mut stack: Vec<usize> = self.ordered(&self.roots).into_iter().rev().collect();
        while let Some(id) = stack.pop() {
            let Some(entry) = self.entries.get(&id) else { continue };
            let mut shown = entry.node.clone();
            shown.pending = self.waiting.iter().any(|request| request.node == Some(id));
            if let Some(children) = &shown.children {
                let visible: Vec<usize> = self
                    .ordered(children)
                    .into_iter()
                    .filter(|child| self.entries.get(child).is_some_and(|child| self.scope.shows(child.node.location)))
                    .collect();
                hidden += children.len() - visible.len();
                if !shown.collapsed {
                    stack.extend(visible.iter().rev());
                }
                shown.children = Some(visible);
            }
            nodes.push(shown);
        }
        HierarchyState {
            open: self.kind.is_some(),
            tab: Some(self.tab),
            pinned: self.pinned,
            kind: self.kind,
            direction: self.direction,
            scope: self.scope,
            sort: self.sort,
            symbol: self.symbol.clone(),
            pending: !self.waiting.is_empty(),
            supported: !self.unsupported,
            roots: self.ordered(&self.roots),
            nodes,
            hidden,
            error: self.error.clone(),
            ..HierarchyState::default()
        }
    }

    fn ask(&mut self, languages: &mut Languages, id: usize) {
        let Some(direction) = self.direction else { return };
        let Some(entry) = self.entries.get_mut(&id) else { return };
        if entry.node.recursive {
            return;
        }
        let sent = match languages.servers.get_mut(&self.server) {
            Some(Server::Running { client, .. }) => {
                client.request(direction.method(), json!({ "item": entry.item })).map_err(|error| error.to_string())
            }
            _ => Err(t!("workbench-language-not-running")),
        };
        match sent {
            Ok(pending) => {
                entry.node.error = None;
                self.waiting.push(Request { node: Some(id), server: self.server.clone(), pending });
            }
            Err(error) => entry.node.error = Some(error),
        }
    }

    fn add(&mut self, node: HierarchyNode, item: Value) -> usize {
        let id = node.id;
        self.next = id + 1;
        self.entries.insert(id, Entry { node, item });
        id
    }

    fn poll(&mut self, languages: &mut Languages, workbench: &Workbench, deadline: Instant) -> bool {
        let mut cache = TextCache::new();
        let mut real_dirs = RealDirs::new();
        let mut any = false;
        loop {
            let mut answered = false;
            let mut index = 0;
            while index < self.waiting.len() {
                let wait = deadline.saturating_duration_since(Instant::now());
                let answer = if wait.is_zero() {
                    self.waiting[index].pending.try_answer()
                } else {
                    match self.waiting[index].pending.wait(wait) {
                        Err(LspError::Timeout) => None,
                        other => Some(other),
                    }
                };
                let Some(answer) = answer else {
                    index += 1;
                    continue;
                };
                let request = self.waiting.remove(index);
                let mut project = ProjectRoot::new(workbench.project(), &mut real_dirs);
                self.take(languages, request, answer, &mut cache, &mut project);
                answered = true;
                any = true;
            }
            if !answered || self.waiting.is_empty() {
                break;
            }
        }
        if self.waiting.is_empty() {
            self.expand_all = None;
        }
        any
    }

    fn take(
        &mut self,
        languages: &mut Languages,
        request: Request,
        answer: Result<Value, LspError>,
        cache: &mut TextCache,
        project: &mut ProjectRoot,
    ) {
        let Some(kind) = self.kind else { return };
        let answer = answer.map_err(|error| match error {
            LspError::Server { code: METHOD_NOT_FOUND, .. } => {
                self.unsupported = true;
                kind.unsupported(&request.server)
            }
            LspError::Server { .. } if request.node.is_none() => kind.none(),
            LspError::Closed => t!("workbench-language-not-running"),
            other => other.to_string(),
        });
        let Some(id) = request.node else {
            languages.hold(HOLDER, BTreeSet::new());
            let items = match answer {
                Ok(items) => items,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            };
            for item in items.as_array().into_iter().flatten() {
                if let Some(node) = read_node(self.next, item, None, languages, cache, project) {
                    let id = self.add(node, item.clone());
                    self.roots.push(id);
                }
            }
            let Some(first) = self.roots.first().and_then(|first| self.entries.get(first)) else {
                self.error = Some(kind.none());
                return;
            };
            if self.direction.is_none() {
                self.direction = Some(if first.node.kind == SymbolKind::Interface {
                    HierarchyDirection::Subtypes
                } else {
                    HierarchyDirection::Supertypes
                });
            }
            for root in self.roots.clone() {
                self.ask(languages, root);
            }
            return;
        };
        let answer = match answer {
            Ok(answer) => answer,
            Err(error) => {
                if let Some(entry) = self.entries.get_mut(&id) {
                    entry.node.error = Some(error);
                }
                return;
            }
        };
        let (Some(direction), Some(parent)) = (self.direction, self.entries.get(&id)) else { return };
        let parent_path = parent.node.path.clone();
        let parent_key = self.key(id);
        let deeper = self.expand_all.is_some_and(|limit| self.depth(id) + 1 < limit);
        let mut children = Vec::new();
        let mut reopen = Vec::new();
        for entry in answer.as_array().into_iter().flatten() {
            let (item, calls) = match direction {
                HierarchyDirection::Callers => (&entry["from"], Some(None)),
                HierarchyDirection::Callees => (&entry["to"], Some(Some(parent_path.as_path()))),
                HierarchyDirection::Supertypes | HierarchyDirection::Subtypes => (entry, None),
            };
            let Some(mut node) = read_node(self.next, item, Some(id), languages, cache, project) else { continue };
            if let Some(caller) = calls {
                let path = caller.map_or_else(|| node.path.clone(), Path::to_path_buf);
                node.calls = call_places(&path, &entry["fromRanges"], languages, cache);
            }
            if self.is_ancestor(id, &node) {
                node.recursive = true;
                node.children = Some(Vec::new());
            }
            let mut key = parent_key.clone();
            key.push((node.name.clone(), node.path.clone()));
            let wanted = self.scope.shows(node.location) && !node.recursive;
            let child = self.add(node, item.clone());
            if self.reopen.remove(&key) {
                reopen.push(child);
            } else if deeper && wanted && self.budget > 0 {
                self.budget -= 1;
                reopen.push(child);
            }
            children.push(child);
        }
        if let Some(parent) = self.entries.get_mut(&id) {
            parent.node.children = Some(children);
        }
        for child in reopen {
            self.ask(languages, child);
        }
    }
}

fn changed(session: &mut Session) {
    session.state_mut::<Hierarchies>().revision += 1;
}

fn take_tab(session: &mut Session, tab: Option<usize>) -> Result<Hierarchy, CommandError> {
    let all = session.state_mut::<Hierarchies>();
    let Some(id) = tab.or(all.active) else { return Err(CommandError::new(t!("workbench-hierarchy-not-open"))) };
    let Some(mut hierarchy) = all.tabs.remove(&id) else {
        return Err(CommandError::new(t!("workbench-hierarchy-no-tab", tab = id.to_string())));
    };
    hierarchy.next = hierarchy.next.max(all.next_node);
    Ok(hierarchy)
}

fn put(session: &mut Session, hierarchy: Hierarchy) {
    let all = session.state_mut::<Hierarchies>();
    all.next_node = all.next_node.max(hierarchy.next);
    all.tabs.insert(hierarchy.tab, hierarchy);
}

fn finish(session: &mut Session, mut state: HierarchyState) -> HierarchyState {
    let all = session.state_mut::<Hierarchies>();
    if state.tab.is_none() {
        state.scope = all.scope;
        state.sort = all.sort;
        state.supported = true;
    }
    state.single_click = all.single_click;
    state.tabs = all.tabs.values().filter_map(Hierarchy::summary).collect();
    state.active = all.active;
    state
}

/// The parameters of `navigate.hierarchy.call` and `navigate.hierarchy.type`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The character offset in the file; `line` and `column` instead, else the primary caret, when
    /// absent.
    #[serde(default)]
    pub offset: Option<usize>,
    /// The line, one-based: with `column`, the place instead of `offset` (ADR-0092).
    #[serde(default)]
    pub line: Option<u32>,
    /// The column on `line`, one-based, in characters; 1 when absent.
    #[serde(default)]
    pub column: Option<u32>,
    /// How long to wait for the server's answers in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// Which way to go: `callers` (default) or `callees`; `supertypes` or `subtypes` (default: subtypes of an interface, else supertypes).
    #[serde(default)]
    pub direction: Option<HierarchyDirection>,
    /// Which nodes to show below the roots: `project`, `test` or `all`; the last one chosen when absent.
    #[serde(default)]
    pub scope: Option<HierarchyScope>,
}

fn start(session: &mut Session, kind: HierarchyKind, params: HierarchyParams) -> Result<HierarchyState, CommandError> {
    let lookup = LookupParams {
        path: params.path.clone(),
        offset: params.offset,
        line: params.line,
        column: params.column,
        wait_ms: params.wait_ms,
    };
    let deadline = Instant::now() + lookup.wait();
    let direction = match (kind, params.direction) {
        (_, Some(direction)) if !kind.fits(direction) => {
            return Err(CommandError::new(t!("workbench-hierarchy-direction")));
        }
        (HierarchyKind::Call, None) => Some(HierarchyDirection::Callers),
        (_, direction) => direction,
    };
    let (path, offset) = lookup.place(session.state_mut::<Workbench>())?;
    let all = session.state_mut::<Hierarchies>();
    if let Some(scope) = params.scope {
        all.scope = scope;
    }
    let replaced = all
        .active
        .filter(|tab| all.tabs.get(tab).is_some_and(|hierarchy| !hierarchy.pinned))
        .or_else(|| all.tabs.values().find(|hierarchy| !hierarchy.pinned).map(|hierarchy| hierarchy.tab));
    let old = match replaced.and_then(|tab| all.tabs.remove(&tab)) {
        Some(old) => old,
        None => Hierarchy { tab: all.next_tab, ..Hierarchy::default() },
    };
    all.next_tab = all.next_tab.max(old.tab + 1);
    all.active = Some(old.tab);
    all.started += 1;
    let hierarchy = Hierarchy {
        tab: old.tab,
        kind: Some(kind),
        direction,
        scope: all.scope,
        sort: all.sort,
        next: all.next_node.max(old.next),
        ..Hierarchy::default()
    };
    changed(session);
    prepare(session, old.waiting, hierarchy, path, offset, deadline)
}

fn prepare(
    session: &mut Session,
    old: Vec<Request>,
    mut hierarchy: Hierarchy,
    path: PathBuf,
    offset: usize,
    deadline: Instant,
) -> Result<HierarchyState, CommandError> {
    let Some(kind) = hierarchy.kind else { return Err(CommandError::new(t!("workbench-hierarchy-not-open"))) };
    hierarchy.symbol = file_text(session.state_mut::<Workbench>(), &path).ok().and_then(|text| word_at(&text, offset));
    hierarchy.origin = Some((path.clone(), offset));
    let sent: Result<Result<Request, String>, String> = with_languages(session, |languages, workbench| {
        cancel(languages, old);
        languages.hold(HOLDER, BTreeSet::new());
        languages.sync(workbench);
        let server = languages.server_id(&path).unwrap_or_default();
        if languages.client_for(&path).is_some_and(|client| !client.provides(kind.capability())) {
            return Ok(Err(server));
        }
        let request = request_at(languages, workbench, &path, offset, kind.prepare(), json!({}))?;
        languages.hold(HOLDER, BTreeSet::from([path.clone()]));
        Ok(Ok(Request { node: None, server, pending: request.pending }))
    });
    let tab = hierarchy.tab;
    match sent {
        Err(error) => {
            hierarchy.error = Some(error.clone());
            put(session, hierarchy);
            Err(CommandError::new(error))
        }
        Ok(Err(server)) => {
            hierarchy.unsupported = true;
            hierarchy.error = Some(kind.unsupported(&server));
            hierarchy.server = server;
            let state = hierarchy.view();
            put(session, hierarchy);
            Ok(finish(session, state))
        }
        Ok(Ok(request)) => {
            hierarchy.server = request.server.clone();
            hierarchy.waiting.push(request);
            put(session, hierarchy);
            Ok(advance(session, tab, deadline))
        }
    }
}

fn advance(session: &mut Session, tab: usize, deadline: Instant) -> HierarchyState {
    let Ok(mut hierarchy) = take_tab(session, Some(tab)) else { return finish(session, HierarchyState::default()) };
    let (answered, state) = with_languages(session, |languages, workbench| {
        let answered = hierarchy.poll(languages, workbench, deadline);
        (answered, hierarchy.view())
    });
    put(session, hierarchy);
    if answered {
        changed(session);
    }
    finish(session, state)
}

fn current(session: &mut Session, tab: Option<usize>, deadline: Instant) -> Result<HierarchyState, CommandError> {
    match tab.or(session.state_mut::<Hierarchies>().active) {
        Some(tab) if session.state_mut::<Hierarchies>().tabs.contains_key(&tab) => Ok(advance(session, tab, deadline)),
        Some(tab) => Err(CommandError::new(t!("workbench-hierarchy-no-tab", tab = tab.to_string()))),
        None => Ok(finish(session, HierarchyState::default())),
    }
}

fn no_node(node: usize) -> CommandError {
    CommandError::new(t!("workbench-hierarchy-no-node", node = node.to_string()))
}

pub struct CallHierarchy;

impl Command for CallHierarchy {
    type Params = HierarchyParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.call";
    const TITLE: &'static str = "Call Hierarchy";
    const DESCRIPTION: &'static str = "Asks the language server for the call hierarchy of the function or method at a place (the \
         caret by default): its callers (or callees with direction), each root's children asked for at once and deeper \
         nodes with navigate.hierarchy.expand. Shows it in the Hierarchy tool window, in place of the selected tab unless \
         it is pinned (then of another unpinned tab, else in a new tab), cancelling the requests of the hierarchy it \
         replaces; navigate.hierarchy.get reads the answers. A server without call hierarchy is reported in supported and \
         error. Calls through interfaces or function values are not listed by gopls.";

    fn run(&self, session: &mut Session, params: HierarchyParams) -> Result<HierarchyState, CommandError> {
        start(session, HierarchyKind::Call, params)
    }
}

pub struct TypeHierarchy;

impl Command for TypeHierarchy {
    type Params = HierarchyParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.type";
    const TITLE: &'static str = "Type Hierarchy";
    const DESCRIPTION: &'static str = "Asks the language server for the type hierarchy of the type at a place (the caret by \
         default): the subtypes of an interface, the supertypes (the interfaces it implements) of another type, or the \
         direction given; each root's children asked for at once and deeper nodes with navigate.hierarchy.expand. Shows \
         it in the Hierarchy tool window, in place of the selected tab unless it is pinned (then of another unpinned tab, \
         else in a new tab). A server without type hierarchy (gopls before v0.19) is reported in supported and error.";

    fn run(&self, session: &mut Session, params: HierarchyParams) -> Result<HierarchyState, CommandError> {
        start(session, HierarchyKind::Type, params)
    }
}

/// The parameters of `navigate.hierarchy.expand`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpandParams {
    /// The id of the node.
    pub node: usize,
    /// How long to wait for the server's answer in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct ExpandHierarchy;

impl Command for ExpandHierarchy {
    type Params = ExpandParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.expand";
    const TITLE: &'static str = "Expand Hierarchy Node";
    const DESCRIPTION: &'static str = "Shows the children of a hierarchy node (by its id, in whichever tab it is): asks the \
         language server for them in the hierarchy's direction unless they were asked for already, and undoes a collapse. \
         A recursive node is never expanded. Returns the node's tab. Node ids are never reused in a window: an id from a \
         hierarchy that was replaced, refreshed, closed or turned the other way is an error.";

    fn run(&self, session: &mut Session, params: ExpandParams) -> Result<HierarchyState, CommandError> {
        let deadline = Instant::now() + WaitParams { wait_ms: params.wait_ms }.wait();
        let tab = session.state_mut::<Hierarchies>().owner(params.node).ok_or_else(|| no_node(params.node))?;
        let mut hierarchy = take_tab(session, Some(tab))?;
        if let Some(entry) = hierarchy.entries.get_mut(&params.node) {
            entry.node.collapsed = false;
        }
        if !hierarchy.asked(params.node) {
            with_languages(session, |languages, _| hierarchy.ask(languages, params.node));
        }
        put(session, hierarchy);
        changed(session);
        Ok(advance(session, tab, deadline))
    }
}

/// The parameters of `navigate.hierarchy.collapse`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollapseParams {
    /// The id of the node whose children to hide; every node of the tab when absent (Collapse All).
    #[serde(default)]
    pub node: Option<usize>,
    /// The tab, for Collapse All; the selected one when absent.
    #[serde(default)]
    pub tab: Option<usize>,
}

pub struct CollapseHierarchy;

impl Command for CollapseHierarchy {
    type Params = CollapseParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.collapse";
    const TITLE: &'static str = "Collapse Hierarchy";
    const DESCRIPTION: &'static str = "Hides the children of a hierarchy node, or of every node of a tab (Collapse All), \
         keeping what the server answered: navigate.hierarchy.expand shows them again without asking it.";

    fn run(&self, session: &mut Session, params: CollapseParams) -> Result<HierarchyState, CommandError> {
        let tab = match params.node {
            Some(node) => Some(session.state_mut::<Hierarchies>().owner(node).ok_or_else(|| no_node(node))?),
            None => params.tab,
        };
        let mut hierarchy = take_tab(session, tab)?;
        for (id, entry) in &mut hierarchy.entries {
            if params.node.is_none_or(|node| node == *id) {
                entry.node.collapsed = true;
            }
        }
        let tab = hierarchy.tab;
        put(session, hierarchy);
        changed(session);
        Ok(advance(session, tab, Instant::now()))
    }
}

/// The parameters of `navigate.hierarchy.expandAll`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpandAllParams {
    /// The tab; the selected one when absent.
    #[serde(default)]
    pub tab: Option<usize>,
    /// How many levels below the roots to show, 1 to 10; 3 by default.
    #[serde(default)]
    pub depth: Option<usize>,
    /// How long to wait for the server's answers in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct ExpandAllHierarchy;

impl Command for ExpandAllHierarchy {
    type Params = ExpandAllParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.expandAll";
    const TITLE: &'static str = "Expand All";
    const DESCRIPTION: &'static str = "Expands every node of a tab down to depth levels below the roots (3 by default, at most \
         10): undoes the collapses and asks the language server for the children of the nodes in the scope not asked \
         for yet, and of theirs as they come, at most 256 requests in all; the nodes left still expand one by one. \
         Recursive nodes are never expanded, so a recursion ends.";

    fn run(&self, session: &mut Session, params: ExpandAllParams) -> Result<HierarchyState, CommandError> {
        let deadline = Instant::now() + WaitParams { wait_ms: params.wait_ms }.wait();
        let limit = params.depth.unwrap_or(EXPAND_ALL_DEPTH).clamp(1, EXPAND_ALL_MAX);
        let mut hierarchy = take_tab(session, params.tab)?;
        for entry in hierarchy.entries.values_mut() {
            entry.node.collapsed = false;
        }
        hierarchy.expand_all = Some(limit);
        hierarchy.budget = EXPAND_ALL_REQUESTS;
        let shown: Vec<usize> = hierarchy.view().nodes.iter().map(|node| node.id).collect();
        with_languages(session, |languages, _| {
            for id in shown {
                if hierarchy.budget > 0 && hierarchy.depth(id) < limit && !hierarchy.asked(id) {
                    hierarchy.budget -= 1;
                    hierarchy.ask(languages, id);
                }
            }
        });
        let tab = hierarchy.tab;
        put(session, hierarchy);
        changed(session);
        Ok(advance(session, tab, deadline))
    }
}

/// The parameters of `navigate.hierarchy.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetHierarchyParams {
    /// The tab; the selected one when absent.
    #[serde(default)]
    pub tab: Option<usize>,
    /// How long to wait for the server's answers in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct GetHierarchy;

impl Command for GetHierarchy {
    type Params = GetHierarchyParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.get";
    const TITLE: &'static str = "Hierarchy State";
    const DESCRIPTION: &'static str = "The call or type hierarchy of a tab (the selected one by default), waiting up to waitMs \
         for the server's answers that are due: its roots and the nodes expanded so far, in the tree's order, filtered by \
         the scope; and the list of tabs.";

    fn run(&self, session: &mut Session, params: GetHierarchyParams) -> Result<HierarchyState, CommandError> {
        current(session, params.tab, Instant::now() + WaitParams { wait_ms: params.wait_ms }.wait())
    }
}

pub struct RefreshHierarchy;

impl Command for RefreshHierarchy {
    type Params = GetHierarchyParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.refresh";
    const TITLE: &'static str = "Refresh Hierarchy";
    const DESCRIPTION: &'static str = "Builds a tab's hierarchy (the selected one by default) again after edits, with the same \
         kind, direction and scope: asks the language server about its root again (found by its name nearest to where it \
         was) and expands again the nodes that were expanded, matched by their names and files; the new nodes get new \
         ids. The requests not answered yet are cancelled.";

    fn run(&self, session: &mut Session, params: GetHierarchyParams) -> Result<HierarchyState, CommandError> {
        let deadline = Instant::now() + WaitParams { wait_ms: params.wait_ms }.wait();
        let mut old = take_tab(session, params.tab)?;
        let Some((path, offset)) = old.origin.clone() else {
            put(session, old);
            return Err(CommandError::new(t!("workbench-hierarchy-not-open")));
        };
        let root = old.roots.first().and_then(|root| old.entries.get(root)).map(|root| &root.node);
        let (path, offset, name) = match root {
            Some(root) => (root.path.clone(), root.selection.start, root.name.clone()),
            None => (path, offset, old.symbol.clone().unwrap_or_default()),
        };
        let offset = match file_text(session.state_mut::<Workbench>(), &path) {
            Ok(text) => relocate(&text, &name, offset),
            Err(_) => offset,
        };
        let mut hierarchy = old.fresh();
        hierarchy.kind = old.kind;
        hierarchy.direction = old.direction;
        hierarchy.reopen = old.expanded();
        changed(session);
        prepare(session, std::mem::take(&mut old.waiting), hierarchy, path, offset, deadline)
    }
}

/// The parameters of `navigate.hierarchy.setOptions`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyOptionsParams {
    /// The tab; the selected one when absent.
    #[serde(default)]
    pub tab: Option<usize>,
    /// Which way to go: `callers` or `callees` for calls, `supertypes` or `subtypes` for types.
    #[serde(default)]
    pub direction: Option<HierarchyDirection>,
    /// Which nodes to show below the roots: `project`, `test` or `all`.
    #[serde(default)]
    pub scope: Option<HierarchyScope>,
    /// Whether to sort the children of each node by name (Sort Alphabetically).
    #[serde(default)]
    pub sort: Option<bool>,
    /// Whether a click on a node in the tool window goes to its code (Navigate with Single Click).
    #[serde(default)]
    pub single_click: Option<bool>,
    /// How long to wait for the server's answers in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct SetHierarchyOptions;

impl Command for SetHierarchyOptions {
    type Params = HierarchyOptionsParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.setOptions";
    const TITLE: &'static str = "Hierarchy Options";
    const DESCRIPTION: &'static str = "Changes a tab's options (the selected tab by default; with none open, only those kept \
         for the next hierarchy): its scope (project, test or all) and Sort Alphabetically, which only filter and order \
         the nodes had and are kept for the next hierarchy; Navigate with Single Click of the tool window; and its \
         direction (callers or callees, supertypes or subtypes), which keeps the roots and asks for their children again, \
         the other nodes dropped.";

    fn run(&self, session: &mut Session, params: HierarchyOptionsParams) -> Result<HierarchyState, CommandError> {
        let deadline = Instant::now() + WaitParams { wait_ms: params.wait_ms }.wait();
        let all = session.state_mut::<Hierarchies>();
        if let Some(scope) = params.scope {
            all.scope = scope;
        }
        if let Some(sort) = params.sort {
            all.sort = sort;
        }
        if let Some(single_click) = params.single_click {
            all.single_click = single_click;
        }
        let open = params.tab.or(all.active).is_some_and(|tab| all.tabs.contains_key(&tab));
        changed(session);
        if !open && params.tab.is_none() && params.direction.is_none() {
            return Ok(finish(session, HierarchyState::default()));
        }
        let mut hierarchy = take_tab(session, params.tab)?;
        if let Some(direction) = params.direction
            && !hierarchy.kind.is_some_and(|kind| kind.fits(direction))
        {
            put(session, hierarchy);
            return Err(CommandError::new(t!("workbench-hierarchy-direction")));
        }
        if let Some(scope) = params.scope {
            hierarchy.scope = scope;
        }
        if let Some(sort) = params.sort {
            hierarchy.sort = sort;
        }
        if let Some(direction) = params.direction
            && hierarchy.direction != Some(direction)
        {
            hierarchy.direction = Some(direction);
            let roots = hierarchy.roots.clone();
            hierarchy.entries.retain(|id, _| roots.contains(id));
            with_languages(session, |languages, _| {
                let (prepare, children): (Vec<Request>, Vec<Request>) =
                    std::mem::take(&mut hierarchy.waiting).into_iter().partition(|request| request.node.is_none());
                cancel(languages, children);
                hierarchy.waiting = prepare;
                for root in roots {
                    if let Some(entry) = hierarchy.entries.get_mut(&root) {
                        entry.node.children = None;
                        entry.node.collapsed = false;
                    }
                    hierarchy.ask(languages, root);
                }
            });
        }
        let tab = hierarchy.tab;
        put(session, hierarchy);
        Ok(advance(session, tab, deadline))
    }
}

/// The parameters of `navigate.hierarchy.cancel`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelHierarchyParams {
    /// The id of the node whose children are no longer wanted; every request of the tab when absent.
    #[serde(default)]
    pub node: Option<usize>,
    /// The tab whose requests to cancel; the selected one when absent.
    #[serde(default)]
    pub tab: Option<usize>,
}

pub struct CancelHierarchy;

impl Command for CancelHierarchy {
    type Params = CancelHierarchyParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.cancel";
    const TITLE: &'static str = "Cancel Hierarchy Requests";
    const DESCRIPTION: &'static str = "Cancels the hierarchy's requests the language server has not answered: those of one node \
         (its children can be asked for again) or all of a tab's (the selected one by default); answers that come later \
         are dropped. Returns the tab.";

    fn run(&self, session: &mut Session, params: CancelHierarchyParams) -> Result<HierarchyState, CommandError> {
        let all = session.state_mut::<Hierarchies>();
        let tab = params.node.and_then(|node| all.owner(node)).or(params.tab).or(all.active);
        let Some(mut hierarchy) = tab.and_then(|tab| take_tab(session, Some(tab)).ok()) else {
            return Ok(finish(session, HierarchyState::default()));
        };
        let (cancelled, kept): (Vec<Request>, Vec<Request>) = std::mem::take(&mut hierarchy.waiting)
            .into_iter()
            .partition(|request| params.node.is_none_or(|node| request.node == Some(node)));
        hierarchy.waiting = kept;
        if hierarchy.waiting.is_empty() {
            hierarchy.expand_all = None;
        }
        with_languages(session, |languages, _| {
            if cancelled.iter().any(|request| request.node.is_none()) {
                languages.hold(HOLDER, BTreeSet::new());
            }
            cancel(languages, cancelled);
        });
        let state = hierarchy.view();
        put(session, hierarchy);
        changed(session);
        Ok(finish(session, state))
    }
}

/// The parameters of the commands on one tab: `navigate.hierarchy.close`, `navigate.hierarchy.select`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabParams {
    /// The tab; the selected one when absent.
    #[serde(default)]
    pub tab: Option<usize>,
}

pub struct CloseHierarchy;

impl Command for CloseHierarchy {
    type Params = TabParams;
    type Output = ();
    const ID: &'static str = "navigate.hierarchy.close";
    const TITLE: &'static str = "Close Hierarchy";
    const DESCRIPTION: &'static str = "Closes a tab of the Hierarchy tool window (the selected one by default), cancelling its \
         requests, and selects the tab before it, else the one after; the options are kept.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<(), CommandError> {
        let all = session.state_mut::<Hierarchies>();
        let Some(tab) = params.tab.or(all.active) else { return Ok(()) };
        let Some(hierarchy) = all.tabs.remove(&tab) else {
            return Err(CommandError::new(t!("workbench-hierarchy-no-tab", tab = tab.to_string())));
        };
        all.next_node = all.next_node.max(hierarchy.next);
        if all.active == Some(tab) {
            all.active = all.tabs.range(..tab).next_back().or_else(|| all.tabs.range(tab..).next()).map(|(id, _)| *id);
        }
        changed(session);
        with_languages(session, |languages, _| {
            languages.hold(HOLDER, BTreeSet::new());
            cancel(languages, hierarchy.waiting);
        });
        Ok(())
    }
}

pub struct SelectHierarchy;

impl Command for SelectHierarchy {
    type Params = TabParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.select";
    const TITLE: &'static str = "Select Hierarchy Tab";
    const DESCRIPTION: &'static str = "Selects a tab of the Hierarchy tool window by its id, as a click on its title does; \
         returns its hierarchy.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<HierarchyState, CommandError> {
        let all = session.state_mut::<Hierarchies>();
        let Some(tab) = params.tab.or(all.active) else {
            return Err(CommandError::new(t!("workbench-hierarchy-not-open")));
        };
        if !all.tabs.contains_key(&tab) {
            return Err(CommandError::new(t!("workbench-hierarchy-no-tab", tab = tab.to_string())));
        }
        all.active = Some(tab);
        changed(session);
        Ok(advance(session, tab, Instant::now()))
    }
}

/// The parameters of `navigate.hierarchy.pin`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PinParams {
    /// The tab; the selected one when absent.
    #[serde(default)]
    pub tab: Option<usize>,
    /// Whether to pin it (true by default) or unpin it.
    #[serde(default = "pin_default")]
    pub pinned: bool,
}

fn pin_default() -> bool {
    true
}

pub struct PinHierarchy;

impl Command for PinHierarchy {
    type Params = PinParams;
    type Output = HierarchyState;
    const ID: &'static str = "navigate.hierarchy.pin";
    const TITLE: &'static str = "Pin Hierarchy Tab";
    const DESCRIPTION: &'static str = "Pins a tab of the Hierarchy tool window (the selected one by default), or unpins it with \
         pinned false: a new Call or Type Hierarchy never replaces a pinned tab, it opens in an unpinned tab or a new one. \
         A pinned tab keeps working: its nodes expand, refresh and change direction as before.";

    fn run(&self, session: &mut Session, params: PinParams) -> Result<HierarchyState, CommandError> {
        let mut hierarchy = take_tab(session, params.tab)?;
        hierarchy.pinned = params.pinned;
        let tab = hierarchy.tab;
        put(session, hierarchy);
        changed(session);
        Ok(advance(session, tab, Instant::now()))
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CallHierarchy)?;
    registry.register(TypeHierarchy)?;
    registry.register(ExpandHierarchy)?;
    registry.register(GetHierarchy)?;
    registry.register(RefreshHierarchy)?;
    registry.register(SetHierarchyOptions)?;
    registry.register(CancelHierarchy)?;
    registry.register(CloseHierarchy)?;
    registry.register(CollapseHierarchy)?;
    registry.register(ExpandAllHierarchy)?;
    registry.register(SelectHierarchy)?;
    registry.register(PinHierarchy)?;
    Ok(())
}

#[cfg(test)]
mod tests;
