use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::history::{HistoryStore, Oid};

use super::commit::DiffView;
use super::index::IndexParams;
use super::log::{CommitDetails, CommitDetailsInfo, CommitDiff, CommitFileParams, CommitParams, GraphEdge};
use super::stash::StashRef;
pub use super::stash::{StashInfo, StashList};
use super::{ChangeInfo, message, repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(OpenWorkspace)?;
    registry.register(CloseWorkspace)?;
    registry.register(WorkspaceStateCommand)?;
    registry.register(SelectCommit)?;
    registry.register(OpenDiff)?;
    registry.register(GraphPageCommand)?;
    registry.register(FindCommit)?;
    registry.register(StashMenu)?;
    registry.register(StashActionCommand)?;
    registry.register(CommitMenu)?;
    registry.register(CommitActionCommand)?;
    registry.register(RefMenu)?;
    registry.register(RefActionCommand)?;
    registry.register(AskDiscard)?;
    Ok(())
}

const PAGE: usize = 200;
const MAX_PAGE: usize = 5000;

/// What kind of reference a label of the graph is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RefKind {
    /// A local branch.
    #[default]
    Local,
    /// A remote-tracking branch (`origin/main`).
    Remote,
    /// A tag.
    Tag,
    /// A detached `HEAD` (when `HEAD` is on a branch, that branch is marked current instead).
    Head,
}

/// A branch, tag or `HEAD` at a commit of the graph.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphRef {
    /// `main`, `origin/main`, `v1.0` or `HEAD`.
    pub name: String,
    /// What it is.
    pub kind: RefKind,
    /// Whether it is the branch `HEAD` is on (or the detached `HEAD`).
    pub current: bool,
}

/// One row of the commit graph.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphCommit {
    /// The row, from 0 for the newest commit.
    pub index: usize,
    /// The full id, as hex.
    pub id: String,
    /// The first seven characters of the id.
    pub short_id: String,
    /// The parents' ids; a stash's row has only the commit it was made on.
    pub parents: Vec<String>,
    /// For the row of a stash, its place in the list (`stash@{0}` is 0): the stash's own commit,
    /// right above the commit it was made on, in a lane of its own. `None` for a commit.
    pub stash: Option<usize>,
    /// For the work in progress of another worktree, its folder: a row of its own right above the
    /// commit that worktree is at, in a lane of its own (`id` is empty, `parents` that commit,
    /// `subject` its branch or folder name; vcs.worktree.list says how many files changed).
    /// `None` otherwise.
    pub worktree: Option<std::path::PathBuf>,
    /// The author's name.
    pub author: String,
    /// The author's e-mail.
    pub email: String,
    /// When it was authored, in seconds since 1970.
    pub time: i64,
    /// The author's time zone, in seconds east of UTC.
    pub offset: i32,
    /// The message's first line (a stash's message).
    pub subject: String,
    /// The branches, tags and `HEAD` at it: the current branch first, then local branches,
    /// remote ones and tags.
    pub refs: Vec<GraphRef>,
    /// The lane of the commit's dot, from 0 at the left.
    pub lane: usize,
    /// The colour index of the dot (and of its lane).
    pub color: usize,
    /// Lines in the row's upper half, from lanes at its top to lanes at its middle.
    pub up: Vec<GraphEdge>,
    /// Lines in the row's lower half, from lanes at its middle to lanes at its bottom.
    pub down: Vec<GraphEdge>,
}

#[derive(Default)]
pub(super) struct History {
    head: zen_vcs::Head,
    remote: HashSet<String>,
    local: HashSet<String>,
    tags: HashSet<String>,
    refs: HashMap<Oid, Vec<zen_vcs::RefLabel>>,
    store: HistoryStore,
    stashes: Vec<(zen_vcs::Stash, Option<zen_vcs::Commit>)>,
    worktrees: Vec<super::worktree::WipWorktree>,
}

const STASH_ROW: u32 = 1 << 31;
const WIP_ROW: u32 = 1 << 30;
const NO_ROW: u32 = u32::MAX;

#[derive(Default)]
pub(super) struct Graph {
    history: History,
    focus: super::graph_search::GraphFocus,
    layout: zen_vcs::graph::Layout<Oid>,
    rows: zen_vcs::graph::PackedRows,
    sources: Vec<u32>,
    commit_rows: Vec<u32>,
    stash_rows: HashMap<String, usize>,
    order: Option<Vec<u32>>,
    laid: usize,
    wanted: HashSet<Oid>,
    bases: HashMap<Oid, Vec<usize>>,
    placed_stashes: usize,
    wip_bases: HashMap<Oid, Vec<usize>>,
    placed_wips: usize,
    lanes: usize,
    shown: usize,
    complete: bool,
}

pub(super) struct RowParts<'a> {
    pub(super) id: RowId<'a>,
    pub(super) subject: &'a str,
    pub(super) body: &'a str,
    pub(super) author: &'a str,
    pub(super) email: &'a str,
}

pub(super) enum RowId<'a> {
    Commit(Oid),
    Stash(&'a str),
    None,
}

impl Graph {
    fn new(history: History, focus: super::graph_search::GraphFocus) -> Graph {
        let mut bases: HashMap<Oid, Vec<usize>> = HashMap::new();
        for (at, (stash, _)) in history.stashes.iter().enumerate() {
            if let Some(base) = Oid::from_hex(&stash.base) {
                bases.entry(base).or_default().push(at);
            }
        }
        let mut wip_bases: HashMap<Oid, Vec<usize>> = HashMap::new();
        for (at, worktree) in history.worktrees.iter().enumerate() {
            if let Some(head) = Oid::from_hex(&worktree.head) {
                wip_bases.entry(head).or_default().push(at);
            }
        }
        Graph { history, focus, bases, wip_bases, ..Graph::default() }
    }

    pub(super) fn total(&self) -> usize {
        self.rows.len()
    }

    pub(super) fn lanes(&self) -> usize {
        self.lanes
    }

    pub(super) fn hidden(&self) -> usize {
        let sides = if self.complete {
            self.history.stashes.len() - self.placed_stashes + self.history.worktrees.len() - self.placed_wips
        } else {
            0
        };
        self.laid - self.shown + sides
    }

    pub(super) fn row_of(&self, id: &str) -> Option<usize> {
        if let Some(&row) = self.stash_rows.get(id) {
            return Some(row);
        }
        let at = self.history.store.index_of(&Oid::from_hex(id)?)?;
        self.commit_rows.get(at).filter(|&&row| row != NO_ROW).map(|&row| row as usize)
    }

    pub(super) fn id(&self, row: usize) -> Option<String> {
        let source = *self.sources.get(row)?;
        Some(if source & STASH_ROW != 0 {
            self.history.stashes[(source & !STASH_ROW) as usize].0.commit.clone()
        } else if source & WIP_ROW != 0 {
            String::new()
        } else {
            self.history.store.id(source as usize).to_hex()
        })
    }

    pub(super) fn parts(&self, row: usize) -> RowParts<'_> {
        let source = self.sources[row];
        if source & STASH_ROW != 0 {
            let (stash, made) = &self.history.stashes[(source & !STASH_ROW) as usize];
            let (author, email) = made.as_ref().map_or(("", ""), |made| (made.author.as_str(), made.email.as_str()));
            return RowParts { id: RowId::Stash(&stash.commit), subject: &stash.message, body: "", author, email };
        }
        if source & WIP_ROW != 0 {
            let worktree = &self.history.worktrees[(source & !WIP_ROW) as usize];
            return RowParts { id: RowId::None, subject: &worktree.label, body: "", author: "", email: "" };
        }
        let store = &self.history.store;
        let at = source as usize;
        let (author, email) = store.author(at);
        RowParts { id: RowId::Commit(store.id(at)), subject: store.subject(at), body: store.body(at), author, email }
    }

    pub(super) fn commit(&self, row: usize) -> GraphCommit {
        let laid = self.rows.get(row);
        let edges = |edges: &[zen_vcs::graph::Edge]| {
            edges.iter().map(|edge| GraphEdge { from: edge.from, to: edge.to, color: edge.color }).collect()
        };
        let source = self.sources[row];
        if source & STASH_ROW != 0 {
            let (stash, made) = &self.history.stashes[(source & !STASH_ROW) as usize];
            let (author, email) =
                made.as_ref().map_or((String::new(), String::new()), |made| (made.author.clone(), made.email.clone()));
            return GraphCommit {
                index: row,
                id: stash.commit.clone(),
                short_id: stash.commit.chars().take(7).collect(),
                parents: vec![stash.base.clone()],
                stash: Some(stash.index),
                author,
                email,
                time: stash.time,
                offset: made.as_ref().map_or(0, |commit| commit.offset),
                subject: stash.message.clone(),
                refs: Vec::new(),
                lane: laid.lane,
                color: laid.color,
                up: edges(&laid.up),
                down: edges(&laid.down),
                worktree: None,
            };
        }
        if source & WIP_ROW != 0 {
            let worktree = &self.history.worktrees[(source & !WIP_ROW) as usize];
            return GraphCommit {
                index: row,
                parents: vec![worktree.head.clone()],
                subject: worktree.label.clone(),
                lane: laid.lane,
                color: laid.color,
                up: edges(&laid.up),
                down: edges(&laid.down),
                worktree: Some(worktree.path.clone()),
                ..GraphCommit::default()
            };
        }
        let store = &self.history.store;
        let at = source as usize;
        let id = store.id(at);
        let history = &self.history;
        let mut refs: Vec<GraphRef> = history
            .refs
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|label| {
                let name = &label.name;
                let (kind, current) = ref_kind(history, name)?;
                self.focus.label(name, kind).then(|| GraphRef { name: name.clone(), kind, current })
            })
            .collect();
        refs.sort_by_key(|label| {
            let order = match (label.current, label.kind) {
                (true, _) => 0,
                (_, RefKind::Head) => 0,
                (_, RefKind::Local) => 1,
                (_, RefKind::Remote) => 2,
                (_, RefKind::Tag) => 3,
            };
            (order, label.name.clone())
        });
        let (author, email) = store.author(at);
        let (time, offset) = store.time(at);
        let hex = id.to_hex();
        GraphCommit {
            index: row,
            short_id: hex[..7].to_owned(),
            id: hex,
            parents: store.parents(at).iter().map(Oid::to_hex).collect(),
            stash: None,
            author: author.to_owned(),
            email: email.to_owned(),
            time,
            offset,
            subject: store.subject(at).to_owned(),
            refs,
            lane: laid.lane,
            color: laid.color,
            up: edges(&laid.up),
            down: edges(&laid.down),
            worktree: None,
        }
    }

    fn shown_tip(&self, id: &Oid) -> bool {
        let history = &self.history;
        history
            .refs
            .get(id)
            .into_iter()
            .flatten()
            .any(|label| ref_kind(history, &label.name).is_some_and(|(kind, _)| self.focus.tip(&label.name, kind)))
    }

    pub(super) fn refs_containing(&self, id: &str) -> Option<Vec<zen_vcs::RefLabel>> {
        if !self.complete {
            return None;
        }
        let store = &self.history.store;
        let target = store.index_of(&Oid::from_hex(id)?)?;
        let reaches = store.descendants(target, self.order.as_deref());
        let mut found: Vec<zen_vcs::RefLabel> = self
            .history
            .refs
            .iter()
            .filter(|(id, _)| store.index_of(id).is_some_and(|at| reaches[at]))
            .flat_map(|(_, labels)| labels.iter().filter(|label| label.kind != zen_vcs::RefKind::Head).cloned())
            .collect();
        found.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        Some(found)
    }

    pub(super) fn tip_of(&self, name: &str) -> Option<Oid> {
        let history = &self.history;
        history.refs.iter().find(|(_, labels)| labels.iter().any(|label| label.name == name)).map(|(id, _)| *id)
    }

    fn row_commit(&self, row: usize) -> Option<usize> {
        let source = *self.sources.get(row)?;
        let store = &self.history.store;
        if source & STASH_ROW != 0 {
            let base = &self.history.stashes[(source & !STASH_ROW) as usize].0.base;
            store.index_of(&Oid::from_hex(base)?)
        } else if source & WIP_ROW != 0 {
            store.index_of(&Oid::from_hex(&self.history.worktrees[(source & !WIP_ROW) as usize].head)?)
        } else {
            Some(source as usize)
        }
    }

    pub(super) fn rows_in_history_of(&self, tip: &Oid) -> Vec<bool> {
        let Some(target) = self.history.store.index_of(tip) else { return vec![false; self.total()] };
        let reached = self.history.store.ancestors(target);
        (0..self.total()).map(|row| self.row_commit(row).is_some_and(|at| reached[at])).collect()
    }

    pub(super) fn nearest_branch(&self, id: &str) -> Option<RefTarget> {
        let store = &self.history.store;
        let target = match self.stash_rows.get(id) {
            Some(&row) => self.row_commit(row)?,
            None => store.index_of(&Oid::from_hex(id)?)?,
        };
        let distance = store.distances_to(target, self.order.as_deref());
        let history = &self.history;
        history
            .refs
            .iter()
            .filter_map(|(tip, labels)| Some((distance[store.index_of(tip)?], labels)))
            .filter(|(steps, _)| *steps != u32::MAX)
            .flat_map(|(steps, labels)| {
                labels.iter().filter_map(move |label| {
                    let (kind, _) = ref_kind(history, &label.name)?;
                    let branch = matches!(kind, RefKind::Local | RefKind::Remote);
                    (branch && self.focus.label(&label.name, kind)).then(|| (steps, kind, label.name.clone()))
                })
            })
            .min_by(|a, b| (a.0, a.1 != RefKind::Local, &a.2).cmp(&(b.0, b.1 != RefKind::Local, &b.2)))
            .map(|(_, kind, name)| RefTarget { name, kind })
    }

    fn lay_out(&mut self) {
        let count = self.history.store.len();
        self.commit_rows.resize(count, NO_ROW);
        while self.laid < count {
            let at = self.order.as_ref().map_or(self.laid, |order| order[self.laid] as usize);
            self.laid += 1;
            let id = self.history.store.id(at);
            let shown = self.focus.is_empty() || self.shown_tip(&id) || self.wanted.remove(&id);
            if !shown {
                continue;
            }
            let parents = self.history.store.parents(at);
            if !self.focus.is_empty() {
                self.wanted.extend(parents.iter().copied());
            }
            let mut row = self.layout.push(id, parents);
            let wips = self.wip_bases.get(&id).map_or(&[][..], Vec::as_slice);
            let stashes = self.bases.get(&id).map_or(&[][..], Vec::as_slice);
            let sources: Vec<u32> = wips
                .iter()
                .map(|&wip| WIP_ROW | wip as u32)
                .chain(stashes.iter().map(|&stash| STASH_ROW | stash as u32))
                .collect();
            let sides = zen_vcs::graph::attach_sides(&mut row, sources.len());
            for (source, side) in sources.into_iter().zip(sides) {
                if source & STASH_ROW != 0 {
                    let stash = (source & !STASH_ROW) as usize;
                    self.stash_rows.insert(self.history.stashes[stash].0.commit.clone(), self.rows.len());
                    self.placed_stashes += 1;
                } else {
                    self.placed_wips += 1;
                }
                self.lanes = self.lanes.max(side.width);
                self.rows.push(&side);
                self.sources.push(source);
            }
            self.lanes = self.lanes.max(row.width);
            self.commit_rows[at] = self.rows.len() as u32;
            self.rows.push(&row);
            self.sources.push(at as u32);
            self.shown += 1;
        }
    }

    fn relayout(&mut self, focus: super::graph_search::GraphFocus) {
        self.focus = focus;
        self.layout = zen_vcs::graph::Layout::default();
        self.rows.clear();
        self.sources.clear();
        self.commit_rows.clear();
        self.stash_rows.clear();
        self.wanted.clear();
        self.laid = 0;
        self.shown = 0;
        self.placed_stashes = 0;
        self.placed_wips = 0;
        self.lanes = 0;
        self.order = (self.complete && self.history.store.out_of_order())
            .then(|| self.history.store.children_first().into_iter().map(|at| at as u32).collect());
        self.lay_out();
    }

    pub(super) fn heap_bytes(&self) -> usize {
        let refs: usize = self
            .history
            .refs
            .values()
            .map(|labels| {
                labels.iter().map(|label| size_of::<zen_vcs::RefLabel>() + label.name.capacity()).sum::<usize>()
            })
            .sum();
        self.history.store.heap_bytes()
            + self.rows.heap_bytes()
            + self.sources.capacity() * size_of::<u32>()
            + self.commit_rows.capacity() * size_of::<u32>()
            + self.order.as_ref().map_or(0, |order| order.capacity() * size_of::<u32>())
            + zen_vcs::history::map_bytes(self.wanted.capacity(), size_of::<Oid>())
            + zen_vcs::history::map_bytes(self.history.refs.capacity(), size_of::<(Oid, Vec<zen_vcs::RefLabel>)>())
            + refs
    }
}

pub(super) type SharedGraph = Arc<RwLock<Graph>>;

pub(super) fn read(graph: &RwLock<Graph>) -> RwLockReadGuard<'_, Graph> {
    graph.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write(graph: &RwLock<Graph>) -> RwLockWriteGuard<'_, Graph> {
    graph.write().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
pub(super) struct GraphSlot {
    pub(super) graph: SharedGraph,
    pub(super) focus: super::graph_search::GraphFocus,
    stamp: Option<u64>,
    computing: bool,
    pub(super) revision: u64,
    rows_revision: u64,
    error: Option<String>,
    timing: GraphReading,
}

/// How the last reading of the graph went (or the one going on): how soon its first rows came,
/// how long it took to the end, and how much memory the graph takes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphReading {
    /// Milliseconds from the start of the reading to its first rows.
    pub first_rows_ms: Option<u64>,
    /// Milliseconds from its start to its end; `None` while it goes on.
    pub read_ms: Option<u64>,
    /// How many commits it read.
    pub commits: usize,
    /// The bytes of memory the graph takes (its history and rows), once read.
    pub memory: usize,
}

impl GraphSlot {
    pub(super) fn refocus(&mut self) {
        write(&self.graph).relayout(self.focus.clone());
        self.revision += 1;
        self.rows_revision += 1;
    }

    pub(super) fn graph(&self) -> SharedGraph {
        Arc::clone(&self.graph)
    }
}

#[derive(Default)]
pub(super) struct Graphs {
    slot: Arc<Mutex<GraphSlot>>,
}

impl Graphs {
    pub(super) fn slot(&self) -> MutexGuard<'_, GraphSlot> {
        self.slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

const FIRST_ROWS: std::time::Duration = std::time::Duration::from_millis(30);
const MORE_ROWS: std::time::Duration = std::time::Duration::from_millis(100);
const FIRST_COMMITS: usize = 500;

pub(super) fn fresh_graph(session: &mut Session, wait: bool) -> Result<(), CommandError> {
    let repository = repository(session)?;
    super::worktree::count_if_stale(session, false);
    let vcs = vcs(session);
    let worktrees = super::worktree::wip_worktrees(&vcs.shared);
    let stamp = repository.refs_stamp() ^ super::worktree::wip_stamp(&worktrees);
    let start = {
        let mut slot = vcs.graph.slot();
        let start = !slot.computing && slot.stamp != Some(stamp);
        if start {
            slot.computing = true;
        }
        start
    };
    if start {
        let shared = Arc::clone(&vcs.shared);
        let target = Arc::clone(&vcs.graph.slot);
        let compute = move || {
            let result = read_graph(&repository, worktrees, &target, &|| shared.wake());
            let mut slot = target.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            slot.error = result.err().map(|error| message(&error));
            slot.stamp = Some(stamp);
            slot.computing = false;
            slot.revision += 1;
            drop(slot);
            shared.wake();
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(compute);
        #[cfg(target_arch = "wasm32")]
        compute();
    }
    if wait {
        for _ in 0..12_000 {
            if !vcs.graph.slot().computing {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    Ok(())
}

fn read_graph(
    repository: &zen_vcs::Repository,
    worktrees: Vec<super::worktree::WipWorktree>,
    target: &Arc<Mutex<GraphSlot>>,
    wake: &dyn Fn(),
) -> Result<(), zen_vcs::VcsError> {
    let lock = || target.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let started = std::time::Instant::now();
    let mut history = read_references(repository)?;
    history.worktrees = worktrees;
    let (graph, first) = {
        let mut slot = lock();
        slot.timing = GraphReading::default();
        let graph: SharedGraph = Arc::new(RwLock::new(Graph::new(history, slot.focus.clone())));
        let first = slot.stamp.is_none();
        if first {
            slot.graph = Arc::clone(&graph);
            slot.revision += 1;
            slot.rows_revision += 1;
        }
        (graph, first)
    };
    let mut batch = HistoryStore::default();
    let mut flushed = started;
    let mut flushes = 0usize;
    let flush = |batch: &mut HistoryStore, flushes: usize| {
        {
            let mut graph = write(&graph);
            for at in 0..batch.len() {
                graph.history.store.push(&batch.record(at));
            }
            graph.lay_out();
        }
        batch.clear();
        let mut slot = lock();
        if flushes == 0 {
            slot.timing.first_rows_ms = Some(started.elapsed().as_millis() as u64);
        }
        if first {
            slot.revision += 1;
            drop(slot);
            wake();
        }
    };
    let mut abandoned = false;
    repository.read_history(&mut |record| {
        batch.push(&record);
        if batch.len() % 64 == 0 {
            let now = std::time::Instant::now();
            let due = if flushes == 0 {
                batch.len() >= FIRST_COMMITS || now - started >= FIRST_ROWS
            } else {
                now - flushed >= MORE_ROWS
            };
            if due {
                flush(&mut batch, flushes);
                flushes += 1;
                flushed = now;
                if Arc::strong_count(target) == 1 {
                    abandoned = true;
                    return false;
                }
            }
        }
        true
    })?;
    flush(&mut batch, flushes);
    if abandoned {
        return Ok(());
    }
    let mut slot = lock();
    {
        let mut graph = write(&graph);
        graph.complete = true;
        if graph.history.store.out_of_order() || graph.focus != slot.focus {
            graph.relayout(slot.focus.clone());
            slot.rows_revision += 1;
        }
        slot.timing.read_ms = Some(started.elapsed().as_millis() as u64);
        slot.timing.memory = graph.heap_bytes();
        slot.timing.commits = graph.history.store.len();
    }
    if !first {
        slot.graph = graph;
        slot.rows_revision += 1;
    }
    Ok(())
}

pub(super) fn refs_containing(
    session: &mut Session,
    repository: &zen_vcs::Repository,
    id: &str,
) -> Option<Vec<zen_vcs::RefLabel>> {
    let vcs = vcs(session);
    let stamp = repository.refs_stamp() ^ super::worktree::wip_stamp(&super::worktree::wip_worktrees(&vcs.shared));
    let graph = {
        let slot = vcs.graph.slot();
        if slot.computing || slot.stamp != Some(stamp) {
            return None;
        }
        slot.graph()
    };
    read(&graph).refs_containing(id)
}

fn read_references(repository: &zen_vcs::Repository) -> Result<History, zen_vcs::VcsError> {
    let head = repository.head()?;
    let branches = repository.branches()?;
    let tags = repository.tags()?.into_iter().map(|tag| tag.name).collect();
    let (remote, local) = branches.into_iter().partition::<Vec<_>, _>(|branch| branch.remote);
    let mut refs: HashMap<Oid, Vec<zen_vcs::RefLabel>> = HashMap::new();
    for (id, label) in repository.ref_labels()? {
        refs.entry(id).or_default().push(label);
    }
    let stashes = repository
        .stashes()
        .unwrap_or_default()
        .into_iter()
        .map(|stash| {
            let query =
                zen_vcs::LogQuery { branch: Some(stash.commit.clone()), limit: 1, ..zen_vcs::LogQuery::default() };
            let made = repository.log(&query).ok().and_then(|mut log| log.pop()).map(|entry| entry.commit);
            (stash, made)
        })
        .collect();
    Ok(History {
        head,
        remote: remote.into_iter().map(|branch| branch.name).collect(),
        local: local.into_iter().map(|branch| branch.name).collect(),
        tags,
        refs,
        store: HistoryStore::default(),
        stashes,
        worktrees: Vec::new(),
    })
}

fn ref_kind(history: &History, name: &str) -> Option<(RefKind, bool)> {
    Some(if name == "HEAD" {
        if history.head.branch.is_some() {
            return None;
        }
        (RefKind::Head, true)
    } else if history.remote.contains(name) {
        (RefKind::Remote, false)
    } else if history.tags.contains(name) && !history.local.contains(name) {
        (RefKind::Tag, false)
    } else {
        (RefKind::Local, history.head.branch.as_deref() == Some(name))
    })
}

/// Which rows of the graph to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphPageParams {
    /// The first row, from 0 for the newest commit.
    pub skip: usize,
    /// How many rows; 200 when absent, 5000 at most.
    pub limit: Option<usize>,
    /// Wait until the graph is read (it is read in the background after a branch moved).
    pub wait: bool,
}

/// A page of the commit graph.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphPage {
    /// How many commits the whole graph has (so far, while `loading`).
    pub total: usize,
    /// How many lanes the widest row uses.
    pub lanes: usize,
    /// The rows asked for.
    pub commits: Vec<GraphCommit>,
    /// Whether the graph is being read (again): unless `loading`, the rows are the previous
    /// reading's.
    pub computing: bool,
    /// Whether rows are still being added: the first reading of a history shows its first rows
    /// before it is read to the end, and `total` grows until this is false.
    pub loading: bool,
    /// Raised whenever the graph changes, rows added included, so that views can cache pages.
    pub revision: u64,
    /// Raised only when rows already given may have changed (a new reading, Hide and Solo): while
    /// it stays, rows only come in at the end and the pages kept stay right.
    pub rows_revision: u64,
    /// How many rows (commits and stashes) Hide and Solo leave out of `total`.
    pub hidden: usize,
    /// Why reading the graph failed, if it did.
    pub error: Option<String>,
    /// How the last reading went: how soon the first rows came, how long it took, the memory.
    pub reading: GraphReading,
}

pub struct GraphPageCommand;

impl Command for GraphPageCommand {
    type Params = GraphPageParams;
    type Output = GraphPage;
    const ID: &'static str = "vcs.graph.page";
    const TITLE: &'static str = "Commit Graph";
    const DESCRIPTION: &'static str = "Rows of the Git workspace's commit graph: every commit reachable from a branch, a tag or \
         HEAD, newest first, with its author, date, subject, the branches and tags at it, and its \
         lane, colour and lines; each stash is a row of its own (`stash` its place in the list) right \
         above the commit it was made on. Lanes are laid out over the whole history, so pages fit \
         together; `total` says how many rows there are. A long history is read in the background \
         and its first rows come at once: while `loading`, rows are added at the end and `total` \
         grows (rows given stay right while `rowsRevision` stays); `reading` says how soon the first \
         rows came, how long the reading took and how much memory the graph takes. Branches and remotes hidden \
         (vcs.graph.hide) or left out by a solo (vcs.graph.solo) are not shown, nor the commits only \
         they reach: `hidden` counts those rows. Set `wait` to wait for the graph to be read to the end.";

    fn run(&self, session: &mut Session, params: GraphPageParams) -> Result<GraphPage, CommandError> {
        fresh_graph(session, params.wait)?;
        let slot = vcs(session).graph.slot();
        let limit = params.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE);
        let graph = read(&slot.graph);
        let total = graph.total();
        let start = params.skip.min(total);
        let end = start.saturating_add(limit).min(total);
        Ok(GraphPage {
            total,
            lanes: graph.lanes,
            commits: (start..end).map(|row| graph.commit(row)).collect(),
            computing: slot.computing,
            loading: slot.computing && !graph.complete,
            revision: slot.revision,
            rows_revision: slot.rows_revision,
            error: slot.error.clone(),
            hidden: graph.hidden(),
            reading: slot.timing.clone(),
        })
    }
}

/// A commit to find.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FindParams {
    /// A revision: a branch (`main`, `origin/main`), a tag, `HEAD`, `HEAD~2`, an id or its start,
    /// `stash@{0}`.
    pub revision: String,
}

/// Where a commit is in the graph.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FoundCommit {
    /// The commit's full id.
    pub id: String,
    /// Its row in the graph (a stash's row for a stash's commit); `None` when the graph does not
    /// show it.
    pub index: Option<usize>,
}

pub struct FindCommit;

impl Command for FindCommit {
    type Params = FindParams;
    type Output = FoundCommit;
    const ID: &'static str = "vcs.graph.find";
    const TITLE: &'static str = "Find Commit in Graph";
    const DESCRIPTION: &'static str = "Finds the commit a revision names (a branch, a tag, HEAD~2, an id or its start) and \
         its row in the Git workspace's commit graph.";

    fn run(&self, session: &mut Session, params: FindParams) -> Result<FoundCommit, CommandError> {
        find(session, &params.revision)
    }
}

pub(super) fn find(session: &mut Session, revision: &str) -> Result<FoundCommit, CommandError> {
    let repository = repository(session)?;
    let id = repository.resolve(revision).map_err(|error| CommandError::new(message(&error)))?;
    fresh_graph(session, true)?;
    let graph = vcs(session).graph.slot().graph();
    let index = read(&graph).row_of(&id);
    Ok(FoundCommit { id, index })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Selection {
    Commit(String),
    Wip,
}

#[derive(Default)]
pub struct GitWorkspace {
    open: bool,
    raised: u64,
    selection: Option<Selection>,
    revealed: u64,
    diff: Option<DiffView>,
    menu: Option<String>,
    ref_menu: Option<Vec<RefTarget>>,
    stash_menu: Option<String>,
    pub(super) drop_menu: Option<super::drag::DropMenuInfo>,
    pub(super) prompt: Option<Prompt>,
    conflicts: Option<String>,
    discard: Option<IndexParams>,
    pub(super) search: super::graph_search::SearchState,
    pub(super) filter: String,
    pub(super) view: super::workspace_view::WorkspaceView,
    pub(super) pull_menu: bool,
    pub(super) pull_mode: super::workspace_pull::PullMode,
    pub(super) columns_menu: bool,
    pub(super) hover: Option<super::workspace_columns::Hover>,
    picked: Vec<String>,
    pub(super) picked_wip: bool,
    pub(super) panel: Option<super::compare::RightPanel>,
    pub(super) file_menu: Option<super::compare::FileTarget>,
    pub(super) blamed: std::collections::BTreeSet<std::path::PathBuf>,
    pub(super) worktree_menu: Option<std::path::PathBuf>,
    pub(super) submodule_menu: Option<String>,
    pub(super) worktree_prompt: Option<super::worktree::WorktreePrompt>,
    pub(super) open_sections: std::collections::BTreeSet<super::workspace_view::PanelSection>,
    pub(super) details: bool,
    pub(super) graph_scroll: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    CreateBranch,
    CreateTag,
    Checkout,
    ResetHard,
    RenameBranch,
    SetUpstream,
    DeleteRemoteBranch,
    DropStash,
    RenameStash,
    CheckoutAndMerge,
    CheckoutAndRebase,
}

impl PromptKind {
    pub fn dialog(self) -> &'static str {
        match self {
            PromptKind::CreateBranch => "newBranch",
            PromptKind::CreateTag => "createTag",
            PromptKind::Checkout => "checkoutCommit",
            PromptKind::ResetHard => "resetHard",
            PromptKind::RenameBranch => "renameBranch",
            PromptKind::SetUpstream => "setUpstream",
            PromptKind::DeleteRemoteBranch => "deleteRemoteBranch",
            PromptKind::DropStash => "dropStash",
            PromptKind::RenameStash => "renameStash",
            PromptKind::CheckoutAndMerge => "checkoutAndMerge",
            PromptKind::CheckoutAndRebase => "checkoutAndRebase",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub id: String,
    pub branch: Option<String>,
    pub drop: Option<(RefTarget, RefTarget)>,
}

/// A branch or a tag, as the Git workspace's menus name it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefTarget {
    /// `main`, `origin/main` or `v1.0`.
    pub name: String,
    /// `local` (the default), `remote` or `tag`.
    #[serde(default)]
    pub kind: RefKind,
}

impl GitWorkspace {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn raised(&self) -> u64 {
        self.raised
    }

    pub fn selected_commit(&self) -> Option<&str> {
        match &self.selection {
            Some(Selection::Commit(id)) => Some(id),
            _ => None,
        }
    }

    pub fn wip_selected(&self) -> bool {
        self.selection == Some(Selection::Wip)
    }

    pub fn revealed(&self) -> u64 {
        self.revealed
    }

    pub fn take_diff(&mut self) -> Option<DiffView> {
        self.diff.take()
    }

    pub fn close(&mut self) {
        self.open = false;
        self.close_menu();
    }

    pub fn menu(&self) -> Option<&str> {
        self.menu.as_deref()
    }

    pub fn ref_menu(&self) -> Option<&[RefTarget]> {
        self.ref_menu.as_deref()
    }

    pub fn stash_menu(&self) -> Option<&str> {
        self.stash_menu.as_deref()
    }

    pub fn drop_menu(&self) -> Option<&super::drag::DropMenuInfo> {
        self.drop_menu.as_ref()
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
        self.ref_menu = None;
        self.stash_menu = None;
        self.drop_menu = None;
        self.pull_menu = false;
        self.columns_menu = false;
        self.worktree_menu = None;
        self.submodule_menu = None;
        self.file_menu = None;
    }

    pub fn take_prompt(&mut self) -> Option<Prompt> {
        self.prompt.take()
    }

    pub fn has_conflicts(&self) -> bool {
        self.conflicts.is_some()
    }

    pub fn take_conflicts(&mut self) -> Option<String> {
        self.conflicts.take()
    }

    pub fn take_discard(&mut self) -> Option<IndexParams> {
        self.discard.take()
    }

    pub(super) fn report_conflicts(&mut self, message: String) {
        self.conflicts = Some(message);
    }

    pub(super) fn select(&mut self, selection: Selection) {
        self.selection = Some(selection);
        self.revealed += 1;
        self.details = true;
    }

    pub(super) fn select_commit(&mut self, id: String) {
        self.select(Selection::Commit(id));
    }

    pub(super) fn select_wip(&mut self) {
        self.select(Selection::Wip);
    }

    pub fn picked(&self) -> &[String] {
        &self.picked
    }

    pub(super) fn clear_picked(&mut self) {
        self.picked.clear();
        self.picked_wip = false;
        if matches!(self.panel, Some(super::compare::RightPanel::Compare { .. })) {
            self.panel = None;
        }
    }

    pub fn picked_wip(&self) -> bool {
        self.picked_wip
    }

    pub fn panel(&self) -> Option<&super::compare::RightPanel> {
        self.panel.as_ref()
    }

    pub fn file_menu(&self) -> Option<&super::compare::FileTarget> {
        self.file_menu.as_ref()
    }

    pub fn blamed_here(&self, path: &std::path::Path) -> bool {
        self.blamed.contains(path)
    }

    pub fn search(&self) -> &super::graph_search::SearchState {
        &self.search
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn view(&self) -> &super::workspace_view::WorkspaceView {
        &self.view
    }

    pub fn pull_menu(&self) -> bool {
        self.pull_menu
    }

    pub fn pull_mode(&self) -> super::workspace_pull::PullMode {
        self.pull_mode
    }

    pub fn columns_menu(&self) -> bool {
        self.columns_menu
    }

    pub fn hover(&self) -> Option<&super::workspace_columns::Hover> {
        self.hover.as_ref()
    }

    pub fn worktree_menu(&self) -> Option<&std::path::Path> {
        self.worktree_menu.as_deref()
    }

    pub fn submodule_menu(&self) -> Option<&str> {
        self.submodule_menu.as_deref()
    }

    pub fn open_sections(&self) -> &std::collections::BTreeSet<super::workspace_view::PanelSection> {
        &self.open_sections
    }

    pub fn take_worktree_prompt(&mut self) -> Option<super::worktree::WorktreePrompt> {
        self.worktree_prompt.take()
    }

    pub fn has_details(&self) -> bool {
        self.selection.is_some() || self.panel.is_some()
    }

    pub fn details_open(&self) -> bool {
        self.details && self.has_details()
    }

    pub fn graph_scroll(&self) -> f32 {
        self.graph_scroll
    }
}

/// What the workspace tab shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceState {
    /// Whether the tab is open.
    pub open: bool,
    /// The selected commit's id.
    pub selected: Option<String>,
    /// Whether the work in progress (the WIP row) is selected.
    pub wip: bool,
    /// How many files of the working tree differ from HEAD: the WIP row shows when there are any.
    pub changes: usize,
    /// The commit whose context menu is open.
    pub menu: Option<String>,
    /// The branches and tags whose context menu is open (vcs.workspace.refMenu).
    pub ref_menu: Option<Vec<RefTarget>>,
    /// The commit of the stash whose context menu is open (vcs.workspace.stashMenu).
    pub stash_menu: Option<String>,
    /// The menu of a branch dropped on another (vcs.workspace.dropMenu): the two branches and
    /// what it offers.
    pub drop_menu: Option<super::drag::DropMenuInfo>,
    /// The search of the graph (vcs.graph.search): whether it is open, its text, the number of
    /// matches and the current one.
    pub search: super::graph_search::SearchSummary,
    /// The left panel's filter (vcs.workspace.filter).
    pub filter: String,
    /// The branches and remotes hidden and soloed (vcs.graph.hide, vcs.graph.solo).
    pub focus: super::graph_search::GraphFocus,
    /// The column widths and the right panel's list of files (vcs.workspace.view).
    pub view: super::workspace_view::WorkspaceView,
    /// Whether the menu of Pull's ▾ is open (vcs.workspace.pullMenu).
    pub pull_menu: bool,
    /// What Pull on the toolbar does (vcs.workspace.pullDefault).
    pub pull_mode: super::workspace_pull::PullMode,
    /// Whether the menu of the graph's column titles is open (vcs.workspace.columnsMenu).
    pub columns_menu: bool,
    /// The branch or the commit under the pointer (vcs.workspace.hover).
    pub hover: Option<super::workspace_columns::HoverInfo>,
    /// The commits picked with ⌘-click (vcs.workspace.select with `toggle`), in the order picked.
    pub picked: Vec<String>,
    /// Whether the work in progress is picked too (vcs.workspace.select with `wip` and `toggle`).
    pub picked_wip: bool,
    /// What the right panel shows instead of the selection: the comparison of two commits, or of
    /// a commit and the work in progress (vcs.diff.commits), or a file's history (vcs.file.history).
    pub panel: Option<super::compare::RightPanel>,
    /// The file of the right panel whose context menu is open (vcs.workspace.fileMenu).
    pub file_menu: Option<super::compare::FileTarget>,
    /// The folder of the worktree whose context menu is open (vcs.workspace.worktreeMenu).
    pub worktree_menu: Option<std::path::PathBuf>,
    /// The path of the submodule whose context menu is open (vcs.workspace.submoduleMenu).
    pub submodule_menu: Option<String>,
    /// The sections of the left panel that are unfolded (vcs.workspace.section).
    pub open_sections: Vec<super::workspace_view::PanelSection>,
    /// Whether the details panel floats over the graph (vcs.workspace.details).
    pub details: bool,
    /// How the graph fits its column: the stage (`lanes`, `gathered`, `dots`), its width, the
    /// lanes shown and how far they are scrolled sideways (vcs.workspace.view).
    pub graph: super::workspace_view::GraphFit,
}

pub(super) fn state(session: &mut Session) -> WorkspaceState {
    let changes = vcs(session).shared.snapshot().status.changes.len();
    let focus = vcs(session).graph.slot().focus.clone();
    let graph = super::workspace_view::graph_fit(session);
    let workspace = session.state_mut::<GitWorkspace>();
    WorkspaceState {
        open: workspace.open,
        selected: workspace.selected_commit().map(str::to_owned),
        wip: workspace.wip_selected(),
        changes,
        menu: workspace.menu.clone(),
        ref_menu: workspace.ref_menu.clone(),
        stash_menu: workspace.stash_menu.clone(),
        drop_menu: workspace.drop_menu.clone(),
        search: workspace.search.summary(),
        filter: workspace.filter.clone(),
        focus,
        view: workspace.view.clone(),
        pull_menu: workspace.pull_menu,
        pull_mode: workspace.pull_mode,
        columns_menu: workspace.columns_menu,
        hover: workspace.hover.as_ref().map(|hover| hover.info.clone()),
        picked: workspace.picked.clone(),
        picked_wip: workspace.picked_wip,
        panel: workspace.panel.clone(),
        file_menu: workspace.file_menu.clone(),
        worktree_menu: workspace.worktree_menu.clone(),
        submodule_menu: workspace.submodule_menu.clone(),
        open_sections: workspace.open_sections.iter().copied().collect(),
        details: workspace.details_open(),
        graph,
    }
}

pub const WORKSPACE: &str = "git";

pub const GRAPH: &str = "graph";

pub const DIFF_TAB: &str = "diff";

pub const REBASE_TAB: &str = "rebase";

pub const CONFLICT_TAB: &str = "conflict";

pub fn install(session: &mut Session) {
    let workspaces = session.state_mut::<crate::workspaces::Workspaces>();
    workspaces.register(crate::workspaces::WorkspaceInfo {
        id: WORKSPACE.to_owned(),
        title: t!("git-workspace-title"),
        icon: "branch".to_owned(),
    });
    let graph = crate::workspaces::InnerTab {
        id: GRAPH.to_owned(),
        kind: GRAPH.to_owned(),
        title: t!("git-workspace-graph-tab"),
        path: None,
        closable: false,
        data: serde_json::Value::Null,
        group: 0,
    };
    workspaces.pin(WORKSPACE, graph);
    workspaces.on_open(WORKSPACE, open_workspace);
    workspaces.on_close(WORKSPACE, |session| session.state_mut::<GitWorkspace>().close());
    workspaces.guard_close(WORKSPACE, close_tab);
}

pub fn open_workspace(session: &mut Session) -> Result<(), CommandError> {
    repository(session)?;
    fresh_graph(session, false)?;
    session.state_mut::<GitWorkspace>().open = true;
    Ok(())
}

fn close_tab(session: &mut Session, _: &str, tab: &crate::workspaces::InnerTab) -> Result<(), CommandError> {
    match tab.kind.as_str() {
        REBASE_TAB => session.state_mut::<super::rebase::RebaseEditor>().close(),
        CONFLICT_TAB => session.state_mut::<super::conflict::ConflictEditor>().close(),
        _ => {}
    }
    Ok(())
}

pub struct OpenWorkspace;

impl Command for OpenWorkspace {
    type Params = NoParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.open";
    const TITLE: &'static str = "Git Workspace";
    const DESCRIPTION: &'static str = "Opens the Git workspace, expanded over the editor area with its graph tab in front: the commit graph of \
         every branch, the branches, remotes, tags and stashes on the left, the selected commit on the \
         right. vcs.graph.page reads its rows.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<WorkspaceState, CommandError> {
        repository(session)?;
        fresh_graph(session, false)?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.open = true;
        workspace.raised += 1;
        crate::workspaces::bring(session, WORKSPACE, GRAPH);
        Ok(state(session))
    }
}

pub struct CloseWorkspace;

impl Command for CloseWorkspace {
    type Params = NoParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.close";
    const TITLE: &'static str = "Close Git Workspace";
    const DESCRIPTION: &'static str =
        "Closes the Git workspace: collapses it and takes its tab out of the editor tab strip.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<WorkspaceState, CommandError> {
        session.state_mut::<GitWorkspace>().close();
        crate::workspaces::close(session, WORKSPACE);
        Ok(state(session))
    }
}

pub struct WorkspaceStateCommand;

impl Command for WorkspaceStateCommand {
    type Params = NoParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.state";
    const TITLE: &'static str = "Git Workspace State";
    const DESCRIPTION: &'static str = "Whether the Git workspace tab is open, the commit (or the work in progress) it shows, and how many files changed.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<WorkspaceState, CommandError> {
        Ok(state(session))
    }
}

/// What to select in the workspace.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectParams {
    /// The commit: a branch, a tag, `HEAD~2`, an id or its start, `stash@{0}`.
    pub revision: Option<String>,
    /// Select the work in progress (the WIP row) instead.
    pub wip: bool,
    /// Add the commit (or with `wip` the work in progress) to the ones picked, or take it out
    /// again, as ⌘-click does: the commit menu then cherry-picks every picked commit at once, and
    /// two picked (or a commit and the work in progress) are compared in the right panel. Without
    /// it, the picked ones are forgotten.
    pub toggle: bool,
}

/// What was selected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Selected {
    /// The commit's full id; `None` for the work in progress.
    pub id: Option<String>,
    /// Its row in the graph; `None` for the work in progress or a commit the graph does not show.
    pub index: Option<usize>,
    /// The commit's details: message, author, parents and changed files.
    pub details: Option<CommitDetailsInfo>,
    /// For the work in progress: the changed files of the working tree.
    pub changes: Vec<ChangeInfo>,
}

pub struct SelectCommit;

impl Command for SelectCommit {
    type Params = SelectParams;
    type Output = Selected;
    const ID: &'static str = "vcs.workspace.select";
    const TITLE: &'static str = "Select Commit";
    const DESCRIPTION: &'static str = "Selects a commit in the Git workspace (scrolling the graph to it) and answers with its \
         details and changed files; with `wip`, selects the work in progress and lists the changed files \
         of the working tree. With `toggle` (⌘-click) the commit (or with `wip` the work in progress) \
         is added to the ones picked, or taken out again (the selected one is picked first), which \
         the commit menu's Cherry-Pick Commits takes; two picked commits, or a commit and the work in \
         progress, are compared in the right panel (vcs.diff.commits; `panel` of vcs.workspace.state). \
         vcs.workspace.state lists them in `picked` and `pickedWip`.";

    fn run(&self, session: &mut Session, params: SelectParams) -> Result<Selected, CommandError> {
        if params.wip {
            let report = super::StatusList.run(session, NoParams::default())?;
            let workspace = session.state_mut::<GitWorkspace>();
            if params.toggle {
                if workspace.picked.is_empty()
                    && !workspace.picked_wip
                    && let Some(selected) = workspace.selected_commit().map(str::to_owned)
                {
                    workspace.picked.push(selected);
                }
                workspace.picked_wip = !workspace.picked_wip;
            } else {
                workspace.clear_picked();
                workspace.panel = None;
            }
            workspace.selection = Some(Selection::Wip);
            workspace.details = true;
            workspace.revealed += 1;
            super::compare::follow_picks(session);
            return Ok(Selected { changes: report.changes, ..Selected::default() });
        }
        let Some(revision) = params.revision.filter(|revision| !revision.trim().is_empty()) else {
            return Err(CommandError::new(t!("vcs-error-revision")));
        };
        let found = find(session, &revision)?;
        let details = CommitDetails.run(session, CommitParams { id: found.id.clone() })?;
        let workspace = session.state_mut::<GitWorkspace>();
        if params.toggle {
            if workspace.picked.is_empty() && !workspace.picked_wip {
                if let Some(selected) = workspace.selected_commit().map(str::to_owned)
                    && selected != found.id
                {
                    workspace.picked.push(selected);
                } else if workspace.wip_selected() {
                    workspace.picked_wip = true;
                }
            }
            match workspace.picked.iter().position(|id| *id == found.id) {
                Some(index) => {
                    workspace.picked.remove(index);
                }
                None => workspace.picked.push(found.id.clone()),
            }
        } else {
            workspace.clear_picked();
            workspace.panel = None;
        }
        workspace.selection = Some(Selection::Commit(found.id.clone()));
        workspace.details = true;
        workspace.revealed += 1;
        super::compare::follow_picks(session);
        Ok(Selected { id: Some(found.id), index: found.index, details: Some(details), changes: Vec::new() })
    }
}

/// A file whose diff to show.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenDiffParams {
    /// The commit; the work in progress (the file compared with HEAD) when absent.
    pub id: Option<String>,
    /// The file, absolute or relative to the repository.
    pub path: std::path::PathBuf,
    /// For the work in progress: only the unstaged or only the staged part of the file's changes
    /// (a file of the panel's Unstaged or Staged list), whose hunks and lines the diff tab then
    /// stages, unstages or discards.
    pub part: Option<super::commit::ChangePart>,
    /// Compare with this commit instead of the commit's first parent (or, without `id`, the
    /// working tree with this commit): how the file differs between two points, as a file of a
    /// comparison in the right panel opens (vcs.diff.commits).
    pub base: Option<String>,
}

pub struct OpenDiff;

impl Command for OpenDiff {
    type Params = OpenDiffParams;
    type Output = DiffView;
    const ID: &'static str = "vcs.workspace.openDiff";
    const TITLE: &'static str = "Show Diff";
    const DESCRIPTION: &'static str = "Opens the diff of a file in the diff tab, as a click on a file of the Git workspace does: \
         how a commit changed it (`id`), or, without `id`, how the working tree differs from HEAD \
         (with `part`, only its unstaged or only its staged changes); with `base`, how it differs \
         between that commit and `id` (or the working tree), as a file of a comparison opens. \
         Answers with both texts and the changes.";

    fn run(&self, session: &mut Session, params: OpenDiffParams) -> Result<DiffView, CommandError> {
        if let Some(base) = params.base.filter(|base| !base.trim().is_empty()) {
            let view = super::compare::file_diff(session, &base, params.id.as_deref(), &params.path)?;
            session.state_mut::<GitWorkspace>().diff = Some(view.clone());
            return Ok(view);
        }
        let view = match params.id {
            Some(revision) => {
                let id = repository(session)?.resolve(&revision).map_err(|error| CommandError::new(message(&error)))?;
                CommitDiff.run(session, CommitFileParams { id, path: params.path })?
            }
            None => {
                let repository = repository(session)?;
                let path = if params.path.is_absolute() {
                    params.path
                } else {
                    super::absolute(repository.root(), &params.path.to_string_lossy().replace('\\', "/"))
                };
                super::commit::FileDiff
                    .run(session, super::commit::FileParams { path: Some(path), part: params.part })?
            }
        };
        session.state_mut::<GitWorkspace>().diff = Some(view.clone());
        Ok(view)
    }
}

/// The commit whose menu to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MenuParams {
    /// The commit: an id (or the start of it), a branch, a tag, `HEAD~2`; the menu closes when
    /// absent.
    pub revision: Option<String>,
}

pub struct CommitMenu;

impl Command for CommitMenu {
    type Params = MenuParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.menu";
    const TITLE: &'static str = "Commit Menu";
    const DESCRIPTION: &'static str = "Opens the context menu of a commit in the Git workspace, as a right click on its row does: \
         the commit is selected and the menu shows at its row (vcs.workspace.commitAction runs an item); \
         without `revision` the menu closes. Answers with the tab's state, `menu` the commit.";

    fn run(&self, session: &mut Session, params: MenuParams) -> Result<WorkspaceState, CommandError> {
        match params.revision.filter(|revision| !revision.trim().is_empty()) {
            Some(revision) => {
                let found = find(session, &revision)?;
                let workspace = session.state_mut::<GitWorkspace>();
                if !workspace.picked.contains(&found.id) {
                    workspace.clear_picked();
                }
                workspace.select(Selection::Commit(found.id.clone()));
                workspace.close_menu();
                workspace.menu = Some(found.id);
            }
            None => session.state_mut::<GitWorkspace>().menu = None,
        }
        Ok(state(session))
    }
}

/// An item of the commit menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CommitAction {
    /// Create Branch Here…: opens the New Branch dialog, from the commit (vcs.branch.create).
    CreateBranch,
    /// Create Tag Here…: opens the Create Tag dialog (vcs.tag.create).
    CreateTag,
    /// Checkout…: asks first, then checks the commit out with a detached HEAD (vcs.revision.checkout).
    Checkout,
    /// Cherry-Pick: at once (vcs.commit.cherryPick).
    CherryPick,
    /// Cherry-Pick Commits…: every commit picked with ⌘-click, in the interactive rebase editor
    /// (vcs.rebase.plan with `cherryPick`).
    CherryPickPicked,
    /// Interactive Rebase from Here…: the current branch's commits after this one, in the
    /// interactive rebase editor (vcs.rebase.plan with `from`).
    InteractiveRebase,
    /// Compare: the two commits picked with ⌘-click (or the commit and the work in progress),
    /// their differing files in the right panel (vcs.diff.commits with `show`).
    Compare,
    /// Revert: at once (vcs.commit.revert).
    Revert,
    /// Reset Current Branch to Here, Soft: at once (vcs.branch.reset).
    ResetSoft,
    /// Reset Current Branch to Here, Mixed: at once.
    ResetMixed,
    /// Reset Current Branch to Here, Hard…: asks first.
    ResetHard,
    /// Copy Commit SHA (vcs.commit.copy).
    CopySha,
    /// Copy Commit Message.
    CopyMessage,
    /// Create Patch…: opens the Create Patch dialog for the commit (vcs.patch.create with
    /// `source: commit`).
    CreatePatch,
}

/// An item of the commit menu to run.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommitActionParams {
    /// The commit: an id (or the start of it), a branch, a tag, `HEAD~2`.
    pub revision: String,
    /// The item.
    pub action: CommitAction,
}

/// What an item of the commit menu did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommitActionResult {
    /// The item.
    pub action: CommitAction,
    /// The commit's full id.
    pub id: String,
    /// The dialog the item opened (`newBranch`, `createTag`, `checkoutCommit`, `resetHard`), where
    /// the user goes on; `get_window_state` lists it among the popups.
    pub dialog: Option<String>,
    /// Where HEAD is after a cherry-pick, a revert or a reset: that commit is selected.
    pub head: Option<String>,
    /// The text copied.
    pub text: Option<String>,
}

pub struct CommitActionCommand;

impl Command for CommitActionCommand {
    type Params = CommitActionParams;
    type Output = CommitActionResult;
    const ID: &'static str = "vcs.workspace.commitAction";
    const TITLE: &'static str = "Commit Menu Item";
    const DESCRIPTION: &'static str = "Runs an item of the Git workspace's commit menu as a click on it does, and closes the menu: \
         createBranch, createTag, checkout and resetHard open their dialog (named in `dialog`); \
         interactiveRebase and cherryPickPicked (the commits picked with ⌘-click) open the \
         interactive rebase editor (`dialog` is `interactiveRebase`, vcs.rebase.state shows its plan); \
         compare shows the two commits picked with ⌘-click (or the commit and the work in progress) \
         compared in the right panel; \
         createPatch opens the Create Patch dialog (`dialog` is `createPatch`); \
         cherryPick, revert, resetSoft and resetMixed run at once and select the new HEAD; copySha and \
         copyMessage put the text on the clipboard. A cherry-pick or a revert that stops on conflicts \
         fails with the code `conflicts`, leaves the repository as git leaves it and selects the work \
         in progress. The operations themselves are vcs.branch.create, vcs.tag.create, \
         vcs.revision.checkout, vcs.commit.cherryPick, vcs.commit.revert, vcs.branch.reset and \
         vcs.commit.copy.";

    fn run(&self, session: &mut Session, params: CommitActionParams) -> Result<CommitActionResult, CommandError> {
        use super::operations::{
            ApplyParams, CherryPick, CopyCommit, CopyParams, CopyWhat, ResetBranch, ResetMode, ResetParams, Revert,
        };
        let id = repository(session)?.resolve(&params.revision).map_err(|error| CommandError::new(message(&error)))?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.menu = None;
        if workspace.selected_commit() != Some(id.as_str()) {
            workspace.select(Selection::Commit(id.clone()));
        }
        let mut result =
            CommitActionResult { action: params.action, id: id.clone(), dialog: None, head: None, text: None };
        let prompt = |session: &mut Session, kind: PromptKind, mut result: CommitActionResult| {
            session.state_mut::<GitWorkspace>().prompt =
                Some(Prompt { kind, id: result.id.clone(), branch: None, drop: None });
            result.dialog = Some(kind.dialog().to_owned());
            Ok(result)
        };
        let apply = |id: &String| ApplyParams { id: id.clone(), mainline: None };
        let reset = |mode| ResetParams { revision: id.clone(), mode };
        let moved = match params.action {
            CommitAction::CreateBranch => return prompt(session, PromptKind::CreateBranch, result),
            CommitAction::CreateTag => return prompt(session, PromptKind::CreateTag, result),
            CommitAction::Checkout => return prompt(session, PromptKind::Checkout, result),
            CommitAction::ResetHard => return prompt(session, PromptKind::ResetHard, result),
            CommitAction::CreatePatch => {
                session.state_mut::<super::patch::PatchDialogRequest>().ask(id.clone());
                result.dialog = Some(CREATE_PATCH.to_owned());
                return Ok(result);
            }
            CommitAction::CopySha | CommitAction::CopyMessage => {
                let what = if params.action == CommitAction::CopySha { CopyWhat::Sha } else { CopyWhat::Message };
                let copied = CopyCommit.run(session, CopyParams { revision: id.clone(), what, clipboard: true })?;
                result.text = Some(copied.text);
                return Ok(result);
            }
            CommitAction::InteractiveRebase | CommitAction::CherryPickPicked => {
                use super::rebase::{PlanParams, PlanRebase};
                let params = if params.action == CommitAction::InteractiveRebase {
                    PlanParams { from: Some(id.clone()), ..PlanParams::default() }
                } else {
                    let mut picked = session.state::<GitWorkspace>().map(|w| w.picked.clone()).unwrap_or_default();
                    if !picked.contains(&id) {
                        picked.push(id.clone());
                    }
                    PlanParams { cherry_pick: picked, ..PlanParams::default() }
                };
                PlanRebase.run(session, params)?;
                result.dialog = Some(INTERACTIVE_REBASE.to_owned());
                return Ok(result);
            }
            CommitAction::Compare => {
                super::compare::compare_picked(session, &id)?;
                return Ok(result);
            }
            CommitAction::CherryPick => CherryPick.run(session, apply(&id)),
            CommitAction::Revert => Revert.run(session, apply(&id)),
            CommitAction::ResetSoft => ResetBranch.run(session, reset(ResetMode::Soft)),
            CommitAction::ResetMixed => ResetBranch.run(session, reset(ResetMode::Mixed)),
        };
        match moved {
            Ok(moved) => {
                session.state_mut::<GitWorkspace>().select(Selection::Commit(moved.head.clone()));
                result.head = Some(moved.head);
                Ok(result)
            }
            Err(error) => Err(after_conflicts(session, error)),
        }
    }
}

pub const INTERACTIVE_REBASE: &str = "interactiveRebase";

pub const CREATE_PATCH: &str = "createPatch";

pub(super) fn after_conflicts(session: &mut Session, error: CommandError) -> CommandError {
    if error.code.as_deref() == Some(super::CONFLICTS) {
        session.state_mut::<GitWorkspace>().select(Selection::Wip);
    }
    error
}

/// The branches or tags whose menu to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RefMenuParams {
    /// A branch or a tag: `main`, `origin/main`, `v1.0` (with `kind`), as a right click on its row
    /// in the left panel opens it.
    pub name: Option<String>,
    /// What `name` is: `local` (the default), `remote` or `tag`.
    pub kind: Option<RefKind>,
    /// Or a commit: the menu of every branch and tag at it, as a right click on its labels in the
    /// graph opens it.
    pub revision: Option<String>,
}

pub(super) fn ref_target(
    session: &mut Session,
    name: &str,
    kind: RefKind,
) -> Result<(RefTarget, String), CommandError> {
    let list = super::branches::ListBranches.run(session, NoParams::default())?;
    let commit = match kind {
        RefKind::Local => list.local.iter().find(|branch| branch.name == name).map(|branch| branch.commit.clone()),
        RefKind::Remote => list.remote.iter().find(|branch| branch.name == name).map(|branch| branch.commit.clone()),
        RefKind::Tag => list.tags.iter().find(|tag| tag.name == name).map(|tag| tag.commit.clone()),
        RefKind::Head => None,
    };
    commit
        .map(|commit| (RefTarget { name: name.to_owned(), kind }, commit))
        .ok_or_else(|| CommandError::new(t!("vcs-error-no-ref", name = name)))
}

pub struct RefMenu;

impl Command for RefMenu {
    type Params = RefMenuParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.refMenu";
    const TITLE: &'static str = "Branch Menu";
    const DESCRIPTION: &'static str = "Opens the context menu of a branch or a tag in the Git workspace, as a right click does: \
         `name` and `kind` (local, remote, tag) for a row of the left panel, or `revision` for the labels \
         of a commit in the graph (every branch and tag at it, each a submenu when there are several). \
         The commit is selected; vcs.workspace.refAction runs an item; without parameters the menu \
         closes. Answers with the tab's state, `refMenu` the branches and tags.";

    fn run(&self, session: &mut Session, params: RefMenuParams) -> Result<WorkspaceState, CommandError> {
        let name = params.name.as_deref().map(str::trim).filter(|name| !name.is_empty());
        let revision = params.revision.as_deref().map(str::trim).filter(|revision| !revision.is_empty());
        let (refs, commit) = match (name, revision) {
            (Some(name), _) => {
                let (target, commit) = ref_target(session, name, params.kind.unwrap_or_default())?;
                (vec![target], commit)
            }
            (None, Some(revision)) => {
                let found = find(session, revision)?;
                let refs: Vec<RefTarget> = found
                    .index
                    .and_then(|index| {
                        let graph = vcs(session).graph.slot().graph();
                        let graph = read(&graph);
                        (index < graph.total()).then(|| graph.commit(index).refs)
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|label| label.kind != RefKind::Head)
                    .map(|label| RefTarget { name: label.name, kind: label.kind })
                    .collect();
                if refs.is_empty() {
                    return Err(CommandError::new(t!("vcs-error-no-refs")));
                }
                (refs, found.id)
            }
            (None, None) => {
                session.state_mut::<GitWorkspace>().ref_menu = None;
                return Ok(state(session));
            }
        };
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.select(Selection::Commit(commit));
        workspace.close_menu();
        workspace.ref_menu = Some(refs);
        Ok(state(session))
    }
}

/// An item of a branch's or a tag's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RefAction {
    /// Checkout: a local branch (vcs.branch.checkout); a remote one through the local branch that
    /// tracks it, created when needed; a tag asks first, then detaches HEAD (vcs.revision.checkout).
    Checkout,
    /// Merge into Current (vcs.branch.merge).
    Merge,
    /// Rebase Current onto it (vcs.branch.rebase).
    Rebase,
    /// Interactive Rebase Current onto it…: the interactive rebase editor (vcs.rebase.plan with
    /// `onto`).
    InteractiveRebase,
    /// Fast-forward: a local branch to its upstream; a remote branch's local branch to it
    /// (vcs.branch.fastForward).
    FastForward,
    /// Rename…: a local branch; opens the Rename Branch dialog (vcs.branch.rename).
    Rename,
    /// Delete: a local branch at once (vcs.branch.delete); a remote one asks first
    /// (vcs.branch.deleteRemote).
    Delete,
    /// Set Upstream…: a local branch; opens the Set Upstream dialog (vcs.branch.setUpstream).
    SetUpstream,
    /// Push: a local branch (vcs.remote.push with `branch`) or a tag (vcs.tag.push), in the
    /// background.
    Push,
    /// Delete Tag: a tag, here only (vcs.tag.delete).
    DeleteTag,
    /// Create Worktree…: a local or a remote branch; opens the Create Worktree dialog
    /// (vcs.worktree.create).
    CreateWorktree,
}

/// An item of a branch's or a tag's menu to run.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefActionParams {
    /// The branch or the tag: `main`, `origin/main`, `v1.0`.
    pub name: String,
    /// What it is: `local` (the default), `remote` or `tag`.
    #[serde(default)]
    pub kind: RefKind,
    /// The item.
    pub action: RefAction,
    /// For Push and the deletion of a remote branch: wait until it is over (it runs in the
    /// background otherwise; vcs.operation.status follows it).
    #[serde(default)]
    pub wait: bool,
}

/// What an item of a branch's or a tag's menu did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefActionResult {
    /// The item.
    pub action: RefAction,
    /// The branch or the tag.
    pub name: String,
    /// What it is.
    pub kind: RefKind,
    /// The dialog the item opened (`renameBranch`, `setUpstream`, `deleteRemoteBranch`,
    /// `checkoutCommit`), where the user goes on; `get_window_state` lists it among the popups.
    pub dialog: Option<String>,
    /// Where HEAD is after a checkout, a merge, a rebase or a fast-forward of the current branch:
    /// that commit is selected.
    pub head: Option<String>,
    /// The branch after a fast-forward.
    pub branch: Option<super::BranchInfo>,
    /// The background operation of a push.
    pub operation: Option<super::OperationInfo>,
}

pub struct RefActionCommand;

impl Command for RefActionCommand {
    type Params = RefActionParams;
    type Output = RefActionResult;
    const ID: &'static str = "vcs.workspace.refAction";
    const TITLE: &'static str = "Branch Menu Item";
    const DESCRIPTION: &'static str = "Runs an item of the Git workspace's branch or tag menu as a click on it does, and closes \
         the menu. Local branches: checkout, merge, rebase, fastForward (to the upstream), rename and \
         setUpstream (their dialogs), delete, push. Remote branches: checkout (a local branch tracking \
         it), merge, rebase, fastForward (the local branch that tracks it), delete (asks first in the \
         `deleteRemoteBranch` dialog). Tags: checkout (asks first: HEAD is detached), merge, rebase, \
         push, deleteTag. createWorktree (a local or a remote branch) opens the `createWorktree` dialog. \
         interactiveRebase (any branch or tag but the current branch) opens the \
         interactive rebase editor for the current branch onto it (`dialog` is `interactiveRebase`). \
         Checkout, merge, rebase and fast-forward select the new HEAD; a merge or a \
         rebase that stops on conflicts fails with the code `conflicts` and selects the work in \
         progress. An item that does not apply (renaming a tag, deleting the current branch) is \
         refused. The operations themselves are vcs.branch.checkout, vcs.revision.checkout, \
         vcs.branch.merge, vcs.branch.rebase, vcs.branch.fastForward, vcs.branch.rename, \
         vcs.branch.setUpstream, vcs.branch.delete, vcs.branch.deleteRemote, vcs.remote.push, \
         vcs.tag.push, vcs.tag.delete and vcs.rebase.plan.";

    fn run(&self, session: &mut Session, params: RefActionParams) -> Result<RefActionResult, CommandError> {
        use super::branches::{
            BranchParams, CheckoutBranch, DeleteBranch, DeleteParams, DeleteTag, FastForwardBranch, FastForwardParams,
            MergeBranch, MergeParams, RebaseBranch, RebaseParams, TagNameParams,
        };
        use super::remote::{Push, PushParams, PushTag, PushTagParams};
        let (target, commit) = ref_target(session, params.name.trim(), params.kind)?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        if workspace.selected_commit() != Some(commit.as_str()) {
            workspace.select(Selection::Commit(commit.clone()));
        }
        let list = super::branches::ListBranches.run(session, NoParams::default())?;
        let local = list.local.iter().find(|branch| target.kind == RefKind::Local && branch.name == target.name);
        let current = local.is_some_and(|branch| branch.current);
        let (action, kind, name) = (params.action, target.kind, target.name.clone());
        let mut result =
            RefActionResult { action, name, kind, dialog: None, head: None, branch: None, operation: None };
        let refused = || Err(CommandError::new(t!("vcs-error-ref-action")));
        let prompt = |session: &mut Session, kind: PromptKind, branch: Option<String>, mut result: RefActionResult| {
            session.state_mut::<GitWorkspace>().prompt = Some(Prompt { kind, id: commit.clone(), branch, drop: None });
            result.dialog = Some(kind.dialog().to_owned());
            Ok(result)
        };
        let name = target.name.clone();
        let moved: Result<(), CommandError> = match (action, kind) {
            (_, RefKind::Head) => return refused(),
            (RefAction::Checkout, RefKind::Tag) => return prompt(session, PromptKind::Checkout, None, result),
            (RefAction::Checkout, _) if current => return refused(),
            (RefAction::Checkout, _) => CheckoutBranch.run(session, BranchParams { name }),
            (RefAction::Merge | RefAction::Rebase, _) if current => return refused(),
            (RefAction::Merge, _) => MergeBranch.run(session, MergeParams { name, target: None }),
            (RefAction::Rebase, _) => RebaseBranch.run(session, RebaseParams { onto: name, branch: None }).map(drop),
            (RefAction::InteractiveRebase, _) if current => return refused(),
            (RefAction::InteractiveRebase, _) => {
                let params = super::rebase::PlanParams { onto: Some(name), ..Default::default() };
                super::rebase::PlanRebase.run(session, params)?;
                result.dialog = Some(INTERACTIVE_REBASE.to_owned());
                return Ok(result);
            }
            (RefAction::FastForward, RefKind::Tag) => return refused(),
            (RefAction::FastForward, RefKind::Local) => {
                let branch = FastForwardBranch.run(session, FastForwardParams { name, onto: None })?;
                let moves_head = branch.current;
                result.branch = Some(branch);
                if !moves_head {
                    return Ok(result);
                }
                Ok(())
            }
            (RefAction::FastForward, RefKind::Remote) => {
                let Some(tracking) = list.tracking(&name) else {
                    return Err(CommandError::new(t!("vcs-error-no-tracking", branch = name.as_str())));
                };
                let params = FastForwardParams { name: tracking.name.clone(), onto: Some(name) };
                let branch = FastForwardBranch.run(session, params)?;
                let moves_head = branch.current;
                result.branch = Some(branch);
                if !moves_head {
                    return Ok(result);
                }
                Ok(())
            }
            (RefAction::Rename, RefKind::Local) => {
                return prompt(session, PromptKind::RenameBranch, Some(name), result);
            }
            (RefAction::SetUpstream, RefKind::Local) => {
                return prompt(session, PromptKind::SetUpstream, Some(name), result);
            }
            (RefAction::Rename | RefAction::SetUpstream, _) => return refused(),
            (RefAction::Delete, RefKind::Local) if current => return refused(),
            (RefAction::Delete, RefKind::Local) => {
                DeleteBranch.run(session, DeleteParams { name, force: false })?;
                return Ok(result);
            }
            (RefAction::Delete, RefKind::Remote) => {
                return prompt(session, PromptKind::DeleteRemoteBranch, Some(name), result);
            }
            (RefAction::Push, RefKind::Local) => {
                let params = PushParams { branch: Some(name), wait: params.wait, ..PushParams::default() };
                result.operation = Some(Push.run(session, params)?);
                return Ok(result);
            }
            (RefAction::Push, RefKind::Tag) => {
                let params = PushTagParams { name, remote: None, wait: params.wait };
                result.operation = Some(PushTag.run(session, params)?);
                return Ok(result);
            }
            (RefAction::DeleteTag, RefKind::Tag) => {
                DeleteTag.run(session, TagNameParams { name })?;
                return Ok(result);
            }
            (RefAction::Delete | RefAction::Push | RefAction::DeleteTag, _) => return refused(),
            (RefAction::CreateWorktree, RefKind::Local | RefKind::Remote) => {
                session.state_mut::<GitWorkspace>().worktree_prompt =
                    Some(super::worktree::WorktreePrompt::Create { branch: target.clone() });
                result.dialog = Some(super::worktree::CREATE_DIALOG.to_owned());
                return Ok(result);
            }
            (RefAction::CreateWorktree, _) => return refused(),
        };
        match moved {
            Ok(()) => {
                let head = repository(session)?.head().ok().and_then(|head| head.commit).unwrap_or_default();
                session.state_mut::<GitWorkspace>().select(Selection::Commit(head.clone()));
                result.head = Some(head);
                Ok(result)
            }
            Err(error) => Err(after_conflicts(session, error)),
        }
    }
}

/// The stash whose menu to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StashMenuParams {
    /// The stash's place in the list (`stash@{0}` is 0), as a right click on its row opens it.
    pub index: Option<usize>,
    /// Or its commit. The menu closes when neither is given.
    pub commit: Option<String>,
}

pub struct StashMenu;

impl Command for StashMenu {
    type Params = StashMenuParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.stashMenu";
    const TITLE: &'static str = "Stash Menu";
    const DESCRIPTION: &'static str = "Opens the context menu of a stash in the Git workspace, as a right click on its row in the \
         left panel or in the graph does: the stash at `index` or the one whose commit is `commit`. \
         The stash is selected; vcs.workspace.stashAction runs an item; without parameters the menu \
         closes. Answers with the tab's state, `stashMenu` the stash's commit.";

    fn run(&self, session: &mut Session, params: StashMenuParams) -> Result<WorkspaceState, CommandError> {
        let commit = params.commit.filter(|commit| !commit.trim().is_empty());
        if params.index.is_none() && commit.is_none() {
            session.state_mut::<GitWorkspace>().stash_menu = None;
            return Ok(state(session));
        }
        let stash = super::stash::find_stash(&*repository(session)?, &StashRef { index: params.index, commit })?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.select(Selection::Commit(stash.commit.clone()));
        workspace.close_menu();
        workspace.stash_menu = Some(stash.commit);
        Ok(state(session))
    }
}

/// An item of a stash's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StashAction {
    /// Apply Stash: its changes back to the working tree; the stash stays (vcs.stash.apply).
    Apply,
    /// Pop Stash: the same, and the stash dropped (vcs.stash.pop).
    Pop,
    /// Drop Stash…: asks first (vcs.stash.drop).
    Drop,
    /// Rename Stash…: opens the Rename Stash dialog (vcs.stash.rename).
    Rename,
}

/// An item of a stash's menu to run.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StashActionParams {
    /// The stash's place in the list: 0 (the newest) when absent.
    #[serde(default)]
    pub index: Option<usize>,
    /// Or its commit.
    #[serde(default)]
    pub commit: Option<String>,
    /// The item.
    pub action: StashAction,
}

/// What an item of a stash's menu did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StashActionResult {
    /// The item.
    pub action: StashAction,
    /// The stash, as it was.
    pub stash: StashInfo,
    /// The dialog the item opened (`dropStash`, `renameStash`), where the user goes on;
    /// `get_window_state` lists it among the popups.
    pub dialog: Option<String>,
    /// Whether the stash was dropped (a pop that applied cleanly).
    pub dropped: bool,
}

pub struct StashActionCommand;

impl Command for StashActionCommand {
    type Params = StashActionParams;
    type Output = StashActionResult;
    const ID: &'static str = "vcs.workspace.stashAction";
    const TITLE: &'static str = "Stash Menu Item";
    const DESCRIPTION: &'static str = "Runs an item of the Git workspace's stash menu as a click on it does, and closes the menu: \
         apply and pop run at once and select the work in progress; drop asks first in the \
         `dropStash` dialog; rename opens the `renameStash` dialog. A stash that does not apply \
         cleanly fails with the code `conflicts`, leaves the files conflicted and keeps the stash. \
         The operations themselves are vcs.stash.apply, vcs.stash.pop, vcs.stash.drop and \
         vcs.stash.rename.";

    fn run(&self, session: &mut Session, params: StashActionParams) -> Result<StashActionResult, CommandError> {
        let which = StashRef { index: params.index, commit: params.commit.filter(|commit| !commit.trim().is_empty()) };
        let stash = super::stash::find_stash(&*repository(session)?, &which)?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        if workspace.selected_commit() != Some(stash.commit.as_str()) {
            workspace.select(Selection::Commit(stash.commit.clone()));
        }
        let mut result =
            StashActionResult { action: params.action, stash: stash.clone(), dialog: None, dropped: false };
        let kind = match params.action {
            StashAction::Drop => PromptKind::DropStash,
            StashAction::Rename => PromptKind::RenameStash,
            StashAction::Apply | StashAction::Pop => {
                let which = StashRef { index: None, commit: Some(stash.commit.clone()) };
                let applied = if params.action == StashAction::Pop {
                    super::stash::PopStash.run(session, which)
                } else {
                    super::stash::ApplyStash.run(session, which)
                };
                session.state_mut::<GitWorkspace>().select(Selection::Wip);
                result.dropped = applied?.dropped;
                return Ok(result);
            }
        };
        session.state_mut::<GitWorkspace>().prompt =
            Some(Prompt { kind, id: stash.commit.clone(), branch: None, drop: None });
        result.dialog = Some(kind.dialog().to_owned());
        Ok(result)
    }
}

/// What vcs.workspace.discard asked.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscardPrompt {
    /// The dialog that asks (`discardChanges`); `get_window_state` lists it among the popups.
    pub dialog: String,
    /// The files it names, relative to the repository (empty for every file).
    pub files: Vec<String>,
}

pub struct AskDiscard;

impl Command for AskDiscard {
    type Params = IndexParams;
    type Output = DiscardPrompt;
    const ID: &'static str = "vcs.workspace.discard";
    const TITLE: &'static str = "Discard Changes…";
    const DESCRIPTION: &'static str = "Opens the `discardChanges` dialog, which asks before discarding unstaged changes: the \
         parameters are those of vcs.changes.discard (files, `all`, or hunks and lines of one file), \
         which the dialog's Discard button runs. Agents that need no confirmation run \
         vcs.changes.discard themselves.";

    fn run(&self, session: &mut Session, params: IndexParams) -> Result<DiscardPrompt, CommandError> {
        let repository = repository(session)?;
        let files = params
            .paths
            .iter()
            .map(|path| super::commit::relative(&repository, path))
            .collect::<Result<Vec<String>, CommandError>>()?;
        if files.is_empty() && !params.all {
            return Err(CommandError::new(t!("vcs-error-nothing-to-commit")));
        }
        session.state_mut::<GitWorkspace>().discard = Some(params);
        Ok(DiscardPrompt { dialog: "discardChanges".to_owned(), files })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    fn session(dir: &std::path::Path) -> (Registry, Session) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir })).unwrap();
        (registry, session)
    }

    fn history() -> tempfile::TempDir {
        let dir = test_repository();
        let root = dir.path();
        git(root, &["switch", "-q", "-c", "feature"]);
        std::fs::write(root.join("f.txt"), "feature\n").unwrap();
        git(root, &["add", "f.txt"]);
        git(root, &["commit", "-q", "-m", "feature work"]);
        git(root, &["update-ref", "refs/remotes/origin/feature", "HEAD"]);
        git(root, &["switch", "-q", "main"]);
        std::fs::write(root.join("m.txt"), "main\n").unwrap();
        git(root, &["add", "m.txt"]);
        git(root, &["commit", "-q", "-m", "main work"]);
        git(root, &["merge", "-q", "--no-ff", "-m", "merge", "feature"]);
        git(root, &["tag", "v1"]);
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        git(root, &["stash", "push", "-q", "-m", "aside"]);
        dir
    }

    #[test]
    fn the_graph_lists_every_branch_with_lanes_and_labels() {
        let dir = history();
        let (registry, mut session) = session(dir.path());
        let page = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        let page: GraphPage = serde_json::from_value(page).unwrap();
        assert!(!page.computing);
        assert_eq!(page.total, 5, "four commits and the stash");
        assert_eq!(page.lanes, 2);
        let subjects: Vec<&str> = page.commits.iter().map(|commit| commit.subject.as_str()).collect();
        assert_eq!(subjects[0], "On main: aside", "the stash right above the commit it was made on");
        assert_eq!(subjects[1], "merge");
        assert_eq!(subjects[4], "First");
        let stash = &page.commits[0];
        assert_eq!((stash.stash, stash.lane, stash.parents.len()), (Some(0), 1, 1), "in a lane of its own");
        assert_eq!(stash.parents[0], page.commits[1].id);
        assert_eq!(stash.author, "Ada Lovelace");
        assert!(page.commits[1..].iter().all(|commit| commit.stash.is_none()));
        let merge = &page.commits[1];
        assert!(merge.up.iter().any(|edge| edge.from == 1 && edge.to == 0), "the stash's line joins the merge");
        let labels: Vec<(&str, RefKind, bool)> =
            merge.refs.iter().map(|label| (label.name.as_str(), label.kind, label.current)).collect();
        assert_eq!(labels, [("main", RefKind::Local, true), ("v1", RefKind::Tag, false)]);
        assert_eq!(merge.down.len(), 2, "a merge has two lines down");
        let feature = page.commits.iter().find(|commit| commit.subject == "feature work").unwrap();
        assert_eq!(feature.lane, 1);
        let names: Vec<&str> = feature.refs.iter().map(|label| label.name.as_str()).collect();
        assert_eq!(names, ["feature", "origin/feature"], "local branches before remote ones");
        assert_eq!(feature.refs[1].kind, RefKind::Remote);
        assert_eq!(page.commits.iter().map(|commit| commit.index).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);

        let second = registry.dispatch(&mut session, "vcs.graph.page", json!({ "skip": 2, "limit": 1 })).unwrap();
        let second: GraphPage = serde_json::from_value(second).unwrap();
        assert_eq!(second.commits, page.commits[2..3]);
        assert_eq!(second.total, 5);
        let past = registry.dispatch(&mut session, "vcs.graph.page", json!({ "skip": 10 })).unwrap();
        assert_eq!(past["commits"], json!([]));

        std::fs::write(dir.path().join("a.txt"), "staged\n").unwrap();
        git(dir.path(), &["add", "a.txt"]);
        let again = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        assert_eq!(again["revision"], json!(page.revision));
        git(dir.path(), &["commit", "-q", "-m", "after"]);
        let again = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        assert_eq!(again["total"], json!(6));
        assert_eq!(again["commits"][0]["subject"], json!("after"), "the stash stays by the merge");
        assert_eq!(again["commits"][1]["stash"], json!(0));
    }

    #[test]
    fn find_select_and_diff_drive_the_tab() {
        let dir = history();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = session(&root);
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["open"], json!(false));
        let state = registry.dispatch(&mut session, "vcs.workspace.open", json!({})).unwrap();
        assert_eq!(state["open"], json!(true));
        assert_eq!(session.state::<GitWorkspace>().unwrap().raised(), 1);

        let page = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        let row = page["commits"].as_array().unwrap().iter().position(|row| row["subject"] == "feature work");
        let found = registry.dispatch(&mut session, "vcs.graph.find", json!({ "revision": "feature" })).unwrap();
        assert_eq!(found["index"], json!(row));
        let selected =
            registry.dispatch(&mut session, "vcs.workspace.select", json!({ "revision": "feature" })).unwrap();
        assert_eq!(selected["index"], json!(row));
        assert_eq!(selected["details"]["subject"], json!("feature work"));
        assert_eq!(selected["details"]["files"][0]["relative"], json!("f.txt"));
        let id = selected["id"].as_str().unwrap().to_owned();
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["selected"], json!(id));

        let stash = registry.dispatch(&mut session, "vcs.graph.find", json!({ "revision": "stash@{0}" })).unwrap();
        assert_eq!(stash["index"], json!(0));
        let stashes = registry.dispatch(&mut session, "vcs.stash.list", json!({})).unwrap();
        assert_eq!(stashes["stashes"][0]["name"], json!("stash@{0}"));
        assert_eq!(stashes["stashes"][0]["message"], json!("On main: aside"));
        let stash_files =
            registry.dispatch(&mut session, "vcs.commit.details", json!({ "id": stashes["stashes"][0]["commit"] }));
        assert_eq!(stash_files.unwrap()["files"][0]["relative"], json!("a.txt"), "a stash's details list its files");

        let diff =
            registry.dispatch(&mut session, "vcs.workspace.openDiff", json!({ "id": id, "path": "f.txt" })).unwrap();
        assert_eq!(diff["after"], json!("feature\n"));
        let queued = session.state_mut::<GitWorkspace>().take_diff().unwrap();
        assert_eq!(queued.relative, "f.txt");
        assert!(session.state_mut::<GitWorkspace>().take_diff().is_none(), "taken once");

        std::fs::write(root.join("a.txt"), "now\n").unwrap();
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let wip = registry.dispatch(&mut session, "vcs.workspace.select", json!({ "wip": true })).unwrap();
        assert_eq!(wip["changes"][0]["relative"], json!("a.txt"));
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!((state["wip"].clone(), state["changes"].clone()), (json!(true), json!(1)));
        let diff = registry.dispatch(&mut session, "vcs.workspace.openDiff", json!({ "path": "a.txt" })).unwrap();
        assert_eq!(diff["after"], json!("now\n"));

        assert!(registry.dispatch(&mut session, "vcs.workspace.select", json!({})).is_err(), "nothing to select");
        assert!(registry.dispatch(&mut session, "vcs.graph.find", json!({ "revision": "nope" })).is_err());
        let state = registry.dispatch(&mut session, "vcs.workspace.close", json!({})).unwrap();
        assert_eq!(state["open"], json!(false));
    }

    #[test]
    fn the_commit_menu_opens_dialogs_and_runs_operations() {
        let dir = history();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = session(&root);
        let s = &mut session;
        let run = |s: &mut Session, command: &str, params: serde_json::Value| registry.dispatch(s, command, params);
        run(s, "vcs.workspace.open", json!({})).unwrap();
        let first = git(&root, &["rev-parse", "main~1~1"]).trim().to_owned();
        let state = run(s, "vcs.workspace.menu", json!({ "revision": "main~1~1" })).unwrap();
        assert_eq!((state["menu"].as_str(), state["selected"].as_str()), (Some(first.as_str()), Some(first.as_str())));

        for (action, dialog, kind) in [
            ("createBranch", "newBranch", PromptKind::CreateBranch),
            ("createTag", "createTag", PromptKind::CreateTag),
            ("checkout", "checkoutCommit", PromptKind::Checkout),
            ("resetHard", "resetHard", PromptKind::ResetHard),
        ] {
            run(s, "vcs.workspace.menu", json!({ "revision": first })).unwrap();
            let done = run(s, "vcs.workspace.commitAction", json!({ "revision": first, "action": action })).unwrap();
            assert_eq!(done["dialog"], dialog);
            let state = run(s, "vcs.workspace.state", json!({})).unwrap();
            assert_eq!(state["menu"], json!(null), "{action} closes the menu");
            let prompt = s.state_mut::<GitWorkspace>().take_prompt().unwrap();
            assert_eq!(prompt, Prompt { kind, id: first.clone(), branch: None, drop: None });
        }
        assert_eq!(git(&root, &["rev-parse", "HEAD"]).trim(), git(&root, &["rev-parse", "main"]).trim(), "nothing ran");

        let copied =
            run(s, "vcs.workspace.commitAction", json!({ "revision": "feature", "action": "copyMessage" })).unwrap();
        assert_eq!(copied["text"], "feature work");
        assert_eq!(s.state_mut::<super::super::ClipboardRequest>().take().as_deref(), Some("feature work"));

        let done =
            run(s, "vcs.workspace.commitAction", json!({ "revision": "main~1", "action": "resetSoft" })).unwrap();
        let target = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
        assert_eq!(done["head"], target);
        assert_eq!(run(s, "vcs.workspace.state", json!({})).unwrap()["selected"], target);
        assert!(!git(&root, &["diff", "--cached", "--name-only"]).trim().is_empty(), "soft: staged");
        run(s, "vcs.branch.reset", json!({ "revision": "v1", "mode": "hard" })).unwrap();

        let done = run(s, "vcs.workspace.commitAction", json!({ "revision": "v1", "action": "revert" })).unwrap();
        assert_eq!(done["head"], git(&root, &["rev-parse", "HEAD"]).trim());
        assert!(
            run(s, "vcs.workspace.commitAction", json!({ "revision": "feature", "action": "cherryPick" })).is_err()
        );
        assert!(run(s, "vcs.workspace.commitAction", json!({ "revision": "nope", "action": "revert" })).is_err());
        assert!(run(s, "vcs.workspace.commitAction", json!({ "revision": "main", "action": "rebase" })).is_err());
        let state = run(s, "vcs.workspace.menu", json!({})).unwrap();
        assert_eq!(state["menu"], json!(null));
    }

    #[test]
    fn a_conflict_from_the_menu_selects_the_work_in_progress() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["switch", "-q", "-c", "other"]);
        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Other"]);
        git(&root, &["switch", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "one\n2\nthree\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Mine"]);
        let (registry, mut session) = session(&root);
        registry.dispatch(&mut session, "vcs.workspace.open", json!({})).unwrap();
        let failed = registry
            .dispatch(
                &mut session,
                "vcs.workspace.commitAction",
                json!({ "revision": "other", "action": "cherryPick" }),
            )
            .unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(error.code.as_deref(), Some(super::super::CONFLICTS));
        assert!(error.message.contains("a.txt"), "{}", error.message);
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["wip"], json!(true));
        assert!(root.join(".git/CHERRY_PICK_HEAD").exists());
        git(&root, &["cherry-pick", "--abort"]);

        for (action, file) in [("merge", "MERGE_HEAD"), ("rebase", "rebase-merge")] {
            registry.dispatch(&mut session, "vcs.workspace.select", json!({ "revision": "main" })).unwrap();
            let failed = registry
                .dispatch(&mut session, "vcs.workspace.refAction", json!({ "name": "other", "action": action }))
                .unwrap_err();
            let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
            assert_eq!(error.code.as_deref(), Some(super::super::CONFLICTS), "{action}");
            let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
            assert_eq!(state["wip"], json!(true), "{action}");
            assert!(root.join(".git").join(file).exists(), "{action} is in progress");
            git(&root, &[action, "--abort"]);
        }
    }

    #[test]
    fn branch_and_tag_menus_open_dialogs_and_run_operations() {
        let dir = history();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = session(&root);
        let s = &mut session;
        let run = |s: &mut Session, command: &str, params: serde_json::Value| registry.dispatch(s, command, params);
        let rev = |revision: &str| git(&root, &["rev-parse", revision]).trim().to_owned();
        run(s, "vcs.workspace.open", json!({})).unwrap();

        let state = run(s, "vcs.workspace.refMenu", json!({ "name": "feature" })).unwrap();
        assert_eq!(state["refMenu"], json!([{ "name": "feature", "kind": "local" }]));
        assert_eq!(state["selected"], rev("feature"));
        run(s, "vcs.workspace.menu", json!({ "revision": "main" })).unwrap();
        let state = run(s, "vcs.workspace.refMenu", json!({ "revision": "main" })).unwrap();
        assert_eq!(state["refMenu"], json!([{ "name": "main", "kind": "local" }, { "name": "v1", "kind": "tag" }]));
        assert_eq!(state["menu"], json!(null), "one menu at a time");
        assert!(run(s, "vcs.workspace.refMenu", json!({ "name": "nope" })).is_err());
        assert!(run(s, "vcs.workspace.refMenu", json!({ "name": "feature", "kind": "tag" })).is_err());
        assert!(run(s, "vcs.workspace.refMenu", json!({ "revision": "main~1" })).is_err(), "no labels there");
        let state = run(s, "vcs.workspace.refMenu", json!({})).unwrap();
        assert_eq!(state["refMenu"], json!(null));

        for (name, kind, action, dialog, prompt) in [
            ("feature", "local", "rename", "renameBranch", PromptKind::RenameBranch),
            ("feature", "local", "setUpstream", "setUpstream", PromptKind::SetUpstream),
            ("origin/feature", "remote", "delete", "deleteRemoteBranch", PromptKind::DeleteRemoteBranch),
            ("v1", "tag", "checkout", "checkoutCommit", PromptKind::Checkout),
        ] {
            run(s, "vcs.workspace.refMenu", json!({ "name": name, "kind": kind })).unwrap();
            let done =
                run(s, "vcs.workspace.refAction", json!({ "name": name, "kind": kind, "action": action })).unwrap();
            assert_eq!(done["dialog"], dialog);
            assert_eq!(run(s, "vcs.workspace.state", json!({})).unwrap()["refMenu"], json!(null), "{action}");
            let asked = s.state_mut::<GitWorkspace>().take_prompt().unwrap();
            let branch = (kind != "tag").then(|| name.to_owned());
            assert_eq!(asked, Prompt { kind: prompt, id: rev(name), branch, drop: None }, "{action}");
        }
        for (name, kind, action) in [
            ("v1", "tag", "rename"),
            ("v1", "tag", "fastForward"),
            ("origin/feature", "remote", "push"),
            ("main", "local", "delete"),
            ("main", "local", "merge"),
            ("main", "local", "checkout"),
            ("feature", "local", "deleteTag"),
        ] {
            let refused = run(s, "vcs.workspace.refAction", json!({ "name": name, "kind": kind, "action": action }));
            assert!(refused.is_err(), "{name} {action}");
        }

        let done = run(s, "vcs.workspace.refAction", json!({ "name": "feature", "action": "checkout" })).unwrap();
        assert_eq!(done["head"], rev("feature"));
        assert_eq!(git(&root, &["branch", "--show-current"]).trim(), "feature");
        run(s, "vcs.workspace.refAction", json!({ "name": "main", "action": "checkout" })).unwrap();
        git(&root, &["switch", "-q", "-c", "side", "main~1"]);
        std::fs::write(root.join("s.txt"), "side\n").unwrap();
        git(&root, &["add", "s.txt"]);
        git(&root, &["commit", "-q", "-m", "side work"]);
        git(&root, &["switch", "-q", "main"]);
        let done = run(s, "vcs.workspace.refAction", json!({ "name": "side", "action": "merge" })).unwrap();
        assert_eq!(done["head"], rev("HEAD"));
        assert_eq!(run(s, "vcs.workspace.state", json!({})).unwrap()["selected"], rev("HEAD"));
        assert_eq!(git(&root, &["rev-list", "--count", "--merges", "-1", "HEAD"]).trim(), "1", "a merge commit");

        git(&root, &["remote", "add", "origin", root.join("no-such-remote").to_str().unwrap()]);
        git(&root, &["update-ref", "refs/remotes/origin/feature", "v1"]);
        git(&root, &["branch", "-q", "--set-upstream-to=origin/feature", "feature"]);
        let done = run(
            s,
            "vcs.workspace.refAction",
            json!({ "name": "origin/feature", "kind": "remote", "action": "fastForward" }),
        )
        .unwrap();
        assert_eq!((done["branch"]["name"].as_str(), done["head"].as_str()), (Some("feature"), None));
        assert_eq!(rev("feature"), rev("v1"));

        run(s, "vcs.workspace.refAction", json!({ "name": "side", "action": "delete" })).unwrap();
        run(s, "vcs.workspace.refAction", json!({ "name": "v1", "kind": "tag", "action": "deleteTag" })).unwrap();
        let list = run(s, "vcs.branch.list", json!({})).unwrap();
        assert!(!list["local"].as_array().unwrap().iter().any(|branch| branch["name"] == "side"));
        assert_eq!(list["tags"], json!([]));
        let pushed =
            run(s, "vcs.workspace.refAction", json!({ "name": "feature", "action": "push", "wait": true })).unwrap();
        assert_eq!(pushed["operation"]["succeeded"], false, "no such remote: it says why");
    }

    #[test]
    fn the_stash_menu_opens_dialogs_and_brings_stashes_back() {
        let dir = history();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = session(&root);
        let s = &mut session;
        let run = |s: &mut Session, command: &str, params: serde_json::Value| registry.dispatch(s, command, params);
        run(s, "vcs.workspace.open", json!({})).unwrap();
        std::fs::write(root.join("m.txt"), "newer\n").unwrap();
        let newer = run(s, "vcs.stash.create", json!({ "message": "newer" })).unwrap();
        let aside = git(&root, &["rev-parse", "stash@{1}"]).trim().to_owned();

        run(s, "vcs.workspace.menu", json!({ "revision": "main" })).unwrap();
        let state = run(s, "vcs.workspace.stashMenu", json!({ "index": 1 })).unwrap();
        assert_eq!(
            (state["stashMenu"].as_str(), state["selected"].as_str()),
            (Some(aside.as_str()), Some(aside.as_str()))
        );
        assert_eq!(state["menu"], json!(null));
        assert!(run(s, "vcs.workspace.stashMenu", json!({ "index": 5 })).is_err());
        let state = run(s, "vcs.workspace.stashMenu", json!({})).unwrap();
        assert_eq!(state["stashMenu"], json!(null));

        for (action, dialog, kind) in
            [("drop", "dropStash", PromptKind::DropStash), ("rename", "renameStash", PromptKind::RenameStash)]
        {
            run(s, "vcs.workspace.stashMenu", json!({ "commit": aside })).unwrap();
            let done = run(s, "vcs.workspace.stashAction", json!({ "commit": aside, "action": action })).unwrap();
            assert_eq!((done["dialog"].as_str(), done["stash"]["index"].as_u64()), (Some(dialog), Some(1)));
            assert_eq!(run(s, "vcs.workspace.state", json!({})).unwrap()["stashMenu"], json!(null), "{action}");
            let asked = s.state_mut::<GitWorkspace>().take_prompt().unwrap();
            assert_eq!(asked, Prompt { kind, id: aside.clone(), branch: None, drop: None });
        }
        assert_eq!(run(s, "vcs.stash.list", json!({})).unwrap()["stashes"].as_array().unwrap().len(), 2, "nothing ran");

        let done = run(s, "vcs.workspace.stashAction", json!({ "index": 1, "action": "apply" })).unwrap();
        assert_eq!(done["dropped"], false);
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "changed\n");
        registry.dispatch(s, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let state = run(s, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!((state["wip"].as_bool(), state["changes"].as_u64()), (Some(true), Some(1)));
        git(&root, &["checkout", "-q", "--", "a.txt"]);
        let done = run(s, "vcs.workspace.stashAction", json!({ "commit": newer["commit"], "action": "pop" })).unwrap();
        assert_eq!(done["dropped"], true);
        assert_eq!(std::fs::read_to_string(root.join("m.txt")).unwrap(), "newer\n");
        let left = run(s, "vcs.stash.list", json!({})).unwrap();
        assert_eq!(left["stashes"][0]["commit"], aside.as_str(), "the other one is left");
        assert!(run(s, "vcs.workspace.stashAction", json!({ "index": 3, "action": "pop" })).is_err());
        assert!(run(s, "vcs.workspace.stashAction", json!({ "action": "hide" })).is_err());
    }

    #[test]
    fn a_stash_that_conflicts_from_the_menu_selects_the_work_in_progress() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = session(&root);
        std::fs::write(root.join("a.txt"), "one\nmine\nthree\n").unwrap();
        registry.dispatch(&mut session, "vcs.stash.create", json!({})).unwrap();
        std::fs::write(root.join("a.txt"), "one\ntheirs\nthree\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Theirs"]);
        registry.dispatch(&mut session, "vcs.workspace.open", json!({})).unwrap();
        let failed =
            registry.dispatch(&mut session, "vcs.workspace.stashAction", json!({ "action": "apply" })).unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(error.code.as_deref(), Some(super::super::CONFLICTS));
        assert_eq!(registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap()["wip"], json!(true));
        assert!(session.state_mut::<GitWorkspace>().take_conflicts().is_some(), "the conflicts dialog is asked for");
    }

    #[test]
    fn ten_thousand_commits_read_and_page_quickly() {
        isolate();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        let mut stream = String::new();
        let mut mark = 0;
        let mut main_tip = 0;
        let commit = |stream: &mut String, branch: &str, mark: usize, time: usize, text: &str, parents: &[usize]| {
            stream.push_str(&format!(
                "commit refs/heads/{branch}\nmark :{mark}\nauthor Ada <ada@example.com> {time} +0000\n\
                 committer Ada <ada@example.com> {time} +0000\ndata {}\n{text}\n",
                text.len()
            ));
            for (number, parent) in parents.iter().enumerate() {
                let kind = if number == 0 { "from" } else { "merge" };
                stream.push_str(&format!("{kind} :{parent}\n"));
            }
            stream.push_str(&format!("M 644 inline file.txt\ndata {}\n{text}\n\n", text.len()));
        };
        let mut total = 0;
        for index in 0..9_980 {
            let time = 1_700_000_000 + index * 60;
            let mut parents: Vec<usize> = (main_tip > 0).then_some(main_tip).into_iter().collect();
            if index % 500 == 250 {
                mark += 1;
                commit(&mut stream, "side", mark, time, &format!("side {index}"), &[main_tip]);
                parents.push(mark);
                total += 1;
            }
            mark += 1;
            commit(&mut stream, "main", mark, time + 30, &format!("commit {index:03}"), &parents);
            main_tip = mark;
            total += 1;
        }
        assert!(total >= 10_000);
        let mut child = std::process::Command::new("git")
            .current_dir(root)
            .args(["fast-import", "--quiet"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(stream.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        git(root, &["reset", "-q", "--hard", "main"]);
        let (registry, mut session) = session(root);
        let started = std::time::Instant::now();
        let page = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        let read = started.elapsed();
        assert_eq!(page["total"], json!(10_000));
        assert_eq!(page["commits"].as_array().unwrap().len(), PAGE);
        let started = std::time::Instant::now();
        let last = registry.dispatch(&mut session, "vcs.graph.page", json!({ "skip": 9_950, "limit": 50 })).unwrap();
        let paged = started.elapsed();
        assert_eq!(last["commits"][49]["subject"], json!("commit 000"));
        assert!(read < std::time::Duration::from_secs(20), "reading took {read:?}");
        assert!(paged < std::time::Duration::from_millis(200), "a page took {paged:?}");
    }

    fn record<'a>(id: &'a Oid, parents: &'a [Oid], subject: &'a str) -> zen_vcs::history::CommitRecord<'a> {
        zen_vcs::history::CommitRecord {
            id: *id,
            parents,
            author: "Ada",
            email: "ada@example.com",
            time: 0,
            offset: 0,
            subject,
            body: "",
        }
    }

    #[test]
    fn rows_laid_out_while_reading_stay_as_they_are() {
        let [m, f, b, y, a] = [1u8, 2, 3, 4, 5].map(|byte| Oid([byte; 20]));
        let mut history = History::default();
        let local = |name: &str| zen_vcs::RefLabel { name: name.to_owned(), kind: zen_vcs::RefKind::Local };
        history.refs.insert(m, vec![local("main")]);
        history.refs.insert(y, vec![local("other")]);
        history.local = ["main", "other"].map(str::to_owned).into();
        let mut graph = Graph::new(history, Default::default());
        graph.history.store.push(&record(&m, &[b, f], "m"));
        graph.history.store.push(&record(&f, &[a], "f"));
        graph.lay_out();
        assert_eq!(graph.total(), 2);
        let first: Vec<GraphCommit> = (0..2).map(|row| graph.commit(row)).collect();
        assert_eq!(first[0].refs, [GraphRef { name: "main".into(), kind: RefKind::Local, current: false }]);
        assert_eq!(first[1].lane, 1, "the merged branch's commit in its own lane");
        graph.history.store.push(&record(&b, &[a], "b"));
        graph.history.store.push(&record(&y, &[a], "y"));
        graph.history.store.push(&record(&a, &[], "a"));
        graph.lay_out();
        assert_eq!(graph.total(), 5);
        assert_eq!((0..2).map(|row| graph.commit(row)).collect::<Vec<_>>(), first, "the rows shown do not change");
        let hex = |id: &Oid| id.to_hex();
        let shape: Vec<(String, Vec<String>)> =
            [(m, vec![b, f]), (f, vec![a]), (b, vec![a]), (y, vec![a]), (a, vec![])]
                .iter()
                .map(|(id, parents)| (hex(id), parents.iter().map(hex).collect()))
                .collect();
        let rows = zen_vcs::graph::layout(&shape);
        for (row, laid) in rows.iter().enumerate() {
            let commit = graph.commit(row);
            assert_eq!((commit.lane, commit.color, commit.id.clone()), (laid.lane, laid.color, shape[row].0.clone()));
            assert_eq!(commit.down.len(), laid.down.len());
        }
        assert_eq!(graph.row_of(&hex(&y)), Some(3));
        assert_eq!(graph.id(4), Some(hex(&a)));
        assert_eq!(graph.hidden(), 0);
        assert!(graph.heap_bytes() > 0);
    }

    #[test]
    fn a_clock_set_wrong_still_puts_children_first() {
        let who = ("Ada Lovelace", "ada");
        let stream = [
            imported_commit("refs/heads/main", 1, who, 1_000, "a", &[], "a.txt"),
            imported_commit("refs/heads/main", 2, who, 3_000, "p", &[1], "a.txt"),
            imported_commit("refs/heads/main", 3, who, 4_000, "x", &[2], "a.txt"),
            imported_commit("refs/heads/other", 4, who, 2_000, "y", &[2], "b.txt"),
        ]
        .concat();
        let dir = fast_import(&stream);
        let (registry, mut session) = session(dir.path());
        let page = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        let subjects: Vec<&str> =
            page["commits"].as_array().unwrap().iter().map(|row| row["subject"].as_str().unwrap()).collect();
        assert_eq!(subjects, ["x", "y", "p", "a"]);
        assert_eq!(page["loading"], json!(false));
        assert!(
            page["commits"][2]["up"].as_array().unwrap().iter().all(|edge| edge["to"] == page["commits"][2]["lane"])
        );
        assert_same_containing(&registry, &mut session, dir.path(), &[0, 1, 2, 3]);
    }

    #[test]
    fn a_long_history_is_read_in_the_background_and_kept_small() {
        let dir = large_history(6_000);
        let commits: usize = git(dir.path(), &["rev-list", "--all", "--count"]).trim().parse().unwrap();
        let (registry, mut session) = session(dir.path());
        let page = |session: &mut Session, params| -> GraphPage {
            serde_json::from_value(registry.dispatch(session, "vcs.graph.page", params).unwrap()).unwrap()
        };
        let mut seen = page(&mut session, json!({ "limit": 50 }));
        let mut first: Option<GraphPage> = None;
        while seen.computing {
            if first.is_none() && seen.total > 0 {
                first = Some(seen.clone());
            }
            if let Some(first) = &first {
                assert_eq!(seen.rows_revision, first.rows_revision, "rows given stay right while it is read");
                assert!(seen.total >= first.total);
                assert!(seen.loading || seen.total == commits, "it stops reading only with every commit read");
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            seen = page(&mut session, json!({ "limit": 50 }));
        }
        assert!(!seen.loading && seen.error.is_none());
        assert_eq!(seen.total, commits);
        if let Some(first) = first {
            assert_eq!(first.commits, seen.commits, "the first rows are the same at the end");
        }
        let reading = &seen.reading;
        assert_eq!(reading.commits, commits);
        assert!(reading.first_rows_ms.unwrap() <= reading.read_ms.unwrap());
        assert!(reading.memory / commits < 1024, "{} bytes a commit", reading.memory / commits);
        let last = page(&mut session, json!({ "skip": commits - 1 }));
        assert!(last.commits[0].parents.is_empty());
        let tags = (0..commits)
            .step_by(MAX_PAGE)
            .flat_map(|skip| page(&mut session, json!({ "skip": skip, "limit": MAX_PAGE })).commits);
        let labels: usize = tags.map(|row| row.refs.len()).sum();
        let refs = git(dir.path(), &["for-each-ref", "--format=%(refname)"]).lines().count();
        assert_eq!(labels, refs, "every branch and tag is on its commit (HEAD is on main)");
        assert_same_containing(&registry, &mut session, dir.path(), &[0, 37, 41, commits / 2, commits - 1]);
    }

    fn assert_same_containing(registry: &Registry, session: &mut Session, root: &std::path::Path, rows: &[usize]) {
        let repository = zen_vcs::Repository::discover(root).unwrap();
        for &row in rows {
            let page = registry.dispatch(session, "vcs.graph.page", json!({ "skip": row, "limit": 1 })).unwrap();
            let id = page["commits"][0]["id"].as_str().unwrap().to_owned();
            assert!(refs_containing(session, &repository, &id).is_some(), "from the graph");
            let details = registry.dispatch(session, "vcs.commit.details", json!({ "id": id })).unwrap();
            let names: Vec<&str> = details["containedIn"]
                .as_array()
                .unwrap()
                .iter()
                .map(|label| label["name"].as_str().unwrap())
                .collect();
            let by_git: Vec<String> =
                repository.refs_containing(&id).unwrap().into_iter().map(|label| label.name).collect();
            assert_eq!(names, by_git, "row {row}");
        }
    }

    #[test]
    #[ignore = "a benchmark: run it with --release -- --ignored"]
    #[allow(clippy::print_stderr, reason = "a benchmark, run from a terminal: it prints what it measured")]
    fn benchmark_a_large_history() {
        let count: usize =
            std::env::var("ZEN_BENCH_COMMITS").ok().and_then(|count| count.parse().ok()).unwrap_or(200_000);
        let made = std::time::Instant::now();
        let dir = large_history(count);
        let commits: usize = git(dir.path(), &["rev-list", "--all", "--count"]).trim().parse().unwrap();
        eprintln!("made {commits} commits in {:?}", made.elapsed());
        let (registry, mut session) = session(dir.path());
        let mut page = |params| -> (GraphPage, std::time::Duration) {
            let asked = std::time::Instant::now();
            let page =
                serde_json::from_value(registry.dispatch(&mut session, "vcs.graph.page", params).unwrap()).unwrap();
            (page, asked.elapsed())
        };
        let started = std::time::Instant::now();
        let (mut seen, mut slowest) = page(json!({ "limit": 200 }));
        let mut first_rows = None;
        let mut pages = 0;
        while seen.computing {
            if first_rows.is_none() && seen.total > 0 {
                first_rows = Some(started.elapsed());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
            let (next, took) = page(json!({ "skip": seen.total.saturating_sub(100), "limit": 200 }));
            slowest = slowest.max(took);
            pages += 1;
            seen = next;
        }
        let full = started.elapsed();
        let first_rows = first_rows.unwrap_or(full);
        let memory = seen.reading.memory as f64 / commits as f64;
        eprintln!(
            "{commits} commits: first rows in {first_rows:?}, all in {full:?} (worker {:?} ms), {memory:.0} bytes a \
             commit, the slowest of {pages} pages read meanwhile took {slowest:?}",
            seen.reading.read_ms
        );
        let searched = std::time::Instant::now();
        let found = registry.dispatch(&mut session, "vcs.graph.search", json!({ "text": "Case 7" })).unwrap();
        let searched = searched.elapsed();
        eprintln!("searching for \"Case 7\" found {} rows in {searched:?}", found["count"]);
        let deep =
            registry.dispatch(&mut session, "vcs.graph.page", json!({ "skip": commits / 2, "limit": 1 })).unwrap();
        let selected = std::time::Instant::now();
        let details = registry.dispatch(&mut session, "vcs.commit.details", json!({ "id": deep["commits"][0]["id"] }));
        let selected = selected.elapsed();
        eprintln!(
            "the details of a commit half way down, in {} branches and tags, in {selected:?}",
            details.unwrap()["containedIn"].as_array().unwrap().len()
        );
        assert_eq!(seen.total, commits);
        assert!(found["count"].as_u64().unwrap() > 0);
        assert!(memory < 1024.0, "{memory} bytes a commit");
        assert!(first_rows < full, "the first rows come before the whole history is read");
        if !cfg!(debug_assertions) {
            assert!(full < std::time::Duration::from_secs(5), "reading took {full:?}");
            assert!(first_rows < std::time::Duration::from_millis(500), "the first rows took {first_rows:?}");
            assert!(slowest < std::time::Duration::from_millis(100), "a page took {slowest:?}");
            assert!(searched < std::time::Duration::from_millis(200), "searching took {searched:?}");
            assert!(selected < std::time::Duration::from_millis(100), "the details took {selected:?}");
        }
    }
}
