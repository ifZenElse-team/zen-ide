use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::filter::TextMatcher;

use super::workspace::{GitWorkspace, Graph, RefKind, RowId, fresh_graph, read};
use super::{message, repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(SearchGraph)?;
    registry.register(HideRef)?;
    registry.register(SoloRef)?;
    registry.register(FocusGraph)?;
    registry.register(FilterPanel)?;
    Ok(())
}

const MATCHES: usize = 100;

/// What a Hide or a Solo is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FocusKind {
    /// A local branch (`main`).
    #[default]
    Local,
    /// A remote branch (`origin/main`).
    Remote,
    /// Every branch of a remote, by the remote's name (`origin`).
    WholeRemote,
}

/// A branch or a remote that is hidden or soloed.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FocusTarget {
    /// `main`, `origin/main`, or the remote `origin`.
    pub name: String,
    /// What it is: `local` (the default), `remote` or `wholeRemote`.
    pub kind: FocusKind,
}

impl FocusTarget {
    fn covers(&self, name: &str, kind: RefKind) -> bool {
        match (self.kind, kind) {
            (FocusKind::Local, RefKind::Local) | (FocusKind::Remote, RefKind::Remote) => self.name == name,
            (FocusKind::WholeRemote, RefKind::Remote) => {
                name.split_once('/').is_some_and(|(remote, _)| remote == self.name)
            }
            _ => false,
        }
    }
}

/// The branches and remotes hidden from the graph, and those soloed: while any is soloed, the graph
/// shows only them. Kept per repository by the window.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphFocus {
    /// Hidden: their labels and the commits only they reach are not shown.
    pub hidden: Vec<FocusTarget>,
    /// Soloed: only they (and the commits they reach) are shown.
    pub solo: Vec<FocusTarget>,
}

impl GraphFocus {
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.solo.is_empty()
    }

    pub fn is_hidden(&self, name: &str, kind: RefKind) -> bool {
        self.hidden.iter().any(|target| target.covers(name, kind))
    }

    pub fn is_soloed(&self, name: &str, kind: RefKind) -> bool {
        self.solo.iter().any(|target| target.covers(name, kind))
    }

    pub fn tip(&self, name: &str, kind: RefKind) -> bool {
        match kind {
            RefKind::Local | RefKind::Remote => {
                !self.is_hidden(name, kind) && (self.solo.is_empty() || self.is_soloed(name, kind))
            }
            RefKind::Tag | RefKind::Head => self.solo.is_empty(),
        }
    }

    pub fn label(&self, name: &str, kind: RefKind) -> bool {
        match kind {
            RefKind::Local | RefKind::Remote => self.tip(name, kind),
            RefKind::Tag | RefKind::Head => true,
        }
    }
}

fn switch(list: &mut Vec<FocusTarget>, target: FocusTarget, on: Option<bool>) -> bool {
    let at = list.iter().position(|listed| *listed == target);
    let on = on.unwrap_or(at.is_none());
    match (on, at) {
        (true, None) => list.push(target),
        (false, Some(at)) => {
            list.remove(at);
        }
        _ => {}
    }
    list.sort();
    on
}

struct Known {
    local: Vec<String>,
    remote: Vec<String>,
}

impl Known {
    fn read(session: &mut Session) -> Result<Self, CommandError> {
        let list = super::branches::ListBranches.run(session, NoParams::default())?;
        Ok(Known {
            local: list.local.into_iter().map(|branch| branch.name).collect(),
            remote: list.remote.into_iter().map(|branch| branch.name).collect(),
        })
    }

    fn has(&self, target: &FocusTarget) -> bool {
        match target.kind {
            FocusKind::Local => self.local.contains(&target.name),
            FocusKind::Remote => self.remote.contains(&target.name),
            FocusKind::WholeRemote => self.remote.iter().any(|name| target.covers(name, RefKind::Remote)),
        }
    }
}

/// What is hidden and soloed, and what the graph shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FocusInfo {
    /// The hidden branches and remotes.
    pub hidden: Vec<FocusTarget>,
    /// The soloed ones.
    pub solo: Vec<FocusTarget>,
    /// How many rows the graph shows.
    pub total: usize,
    /// How many rows Hide and Solo leave out.
    pub hidden_rows: usize,
}

fn change_focus(session: &mut Session, change: impl FnOnce(&mut GraphFocus)) -> Result<FocusInfo, CommandError> {
    repository(session)?;
    fresh_graph(session, false)?;
    {
        let vcs = vcs(session);
        let mut slot = vcs.graph.slot();
        let before = slot.focus.clone();
        change(&mut slot.focus);
        if slot.focus != before {
            slot.refocus();
        }
    }
    if session.state::<GitWorkspace>().is_some_and(|workspace| workspace.search.open) {
        run_search(session, SearchParams::default())?;
    }
    Ok(focus_info(session))
}

fn focus_info(session: &mut Session) -> FocusInfo {
    let slot = vcs(session).graph.slot();
    let graph = read(&slot.graph);
    FocusInfo {
        hidden: slot.focus.hidden.clone(),
        solo: slot.focus.solo.clone(),
        total: graph.total(),
        hidden_rows: graph.hidden(),
    }
}

/// A branch or a remote to hide or show.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HideParams {
    /// The branch (`main`, `origin/main`) or the remote (`origin`).
    pub name: Option<String>,
    /// What `name` is: `local` (the default), `remote` or `wholeRemote`.
    pub kind: FocusKind,
    /// Hide it (true) or show it again (false); switched when absent.
    pub hidden: Option<bool>,
    /// Show every hidden branch and remote again.
    pub show_all: bool,
}

pub struct HideRef;

impl Command for HideRef {
    type Params = HideParams;
    type Output = FocusInfo;
    const ID: &'static str = "vcs.graph.hide";
    const TITLE: &'static str = "Hide in Graph";
    const DESCRIPTION: &'static str = "Hides a branch (`kind` local or remote) or every branch of a remote (`wholeRemote`, \
         `name` the remote) from the Git workspace's graph, as the eye on its row in the left panel \
         does: its label goes, and so do the commits only it reaches. `hidden` false shows it again, \
         absent switches it; `showAll` shows everything again. Kept per repository. Answers with what \
         is hidden and soloed and how many rows the graph shows.";

    fn run(&self, session: &mut Session, params: HideParams) -> Result<FocusInfo, CommandError> {
        if params.show_all {
            return change_focus(session, |focus| focus.hidden.clear());
        }
        let target = checked_target(session, params.name, params.kind)?;
        change_focus(session, |focus| {
            switch(&mut focus.hidden, target, params.hidden);
        })
    }
}

fn checked_target(session: &mut Session, name: Option<String>, kind: FocusKind) -> Result<FocusTarget, CommandError> {
    let name = name.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty());
    let Some(name) = name else { return Err(CommandError::new(t!("vcs-error-focus-name"))) };
    let target = FocusTarget { name, kind };
    if !Known::read(session)?.has(&target) {
        return Err(CommandError::new(t!("vcs-error-no-ref", name = target.name.as_str())));
    }
    Ok(target)
}

/// A branch or a remote to solo, or not.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SoloParams {
    /// The branch (`main`, `origin/main`) or the remote (`origin`).
    pub name: Option<String>,
    /// What `name` is: `local` (the default), `remote` or `wholeRemote`.
    pub kind: FocusKind,
    /// Solo it (true) or not (false); switched when absent.
    pub solo: Option<bool>,
    /// Solo nothing: the graph shows every branch again (except the hidden ones).
    pub clear: bool,
}

pub struct SoloRef;

impl Command for SoloRef {
    type Params = SoloParams;
    type Output = FocusInfo;
    const ID: &'static str = "vcs.graph.solo";
    const TITLE: &'static str = "Solo in Graph";
    const DESCRIPTION: &'static str = "Solos a branch (`kind` local or remote) or every branch of a remote (`wholeRemote`, \
         `name` the remote) in the Git workspace's graph: while anything is soloed, the graph shows \
         only the soloed branches and the commits they reach (tags and a detached HEAD do not keep \
         others). Several can be soloed; `solo` false takes one out, absent switches it; `clear` solos \
         nothing. Kept per repository. Answers with what is hidden and soloed and how many rows the \
         graph shows.";

    fn run(&self, session: &mut Session, params: SoloParams) -> Result<FocusInfo, CommandError> {
        if params.clear {
            return change_focus(session, |focus| focus.solo.clear());
        }
        let target = checked_target(session, params.name, params.kind)?;
        change_focus(session, |focus| {
            switch(&mut focus.solo, target, params.solo);
        })
    }
}

/// The focus to set, or nothing to read it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FocusParams {
    /// What to hide and solo, in place of what is: branches and remotes that are gone are left
    /// out. Absent: the focus is only read.
    pub set: Option<GraphFocus>,
}

pub struct FocusGraph;

impl Command for FocusGraph {
    type Params = FocusParams;
    type Output = FocusInfo;
    const ID: &'static str = "vcs.graph.focus";
    const TITLE: &'static str = "Graph Focus";
    const DESCRIPTION: &'static str = "Reads what is hidden (vcs.graph.hide) and soloed (vcs.graph.solo) in the Git \
         workspace's graph and how many rows it shows; with `set`, replaces both lists at once (the \
         window restores a repository's focus this way), leaving out branches and remotes that are \
         gone.";

    fn run(&self, session: &mut Session, params: FocusParams) -> Result<FocusInfo, CommandError> {
        let Some(set) = params.set else {
            repository(session)?;
            return Ok(focus_info(session));
        };
        let known = Known::read(session)?;
        let keep = |list: Vec<FocusTarget>| {
            let mut list: Vec<FocusTarget> = list.into_iter().filter(|target| known.has(target)).collect();
            list.sort();
            list.dedup();
            list
        };
        let focus = GraphFocus { hidden: keep(set.hidden), solo: keep(set.solo) };
        change_focus(session, |current| *current = focus)
    }
}

/// Which way to move between the matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchStep {
    /// The next match down the graph (↓, ⏎), after the last the first.
    Next,
    /// The previous match up the graph (↑, ⇧⏎), before the first the last.
    Previous,
}

/// What to search for, and where to go.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchParams {
    /// The text: found in messages (the subject and the rest), at the start of ids, and in authors'
    /// names and e-mails. The text searched before when absent.
    pub text: Option<String>,
    /// The text is a regular expression (it does not look at ids then).
    pub regex: Option<bool>,
    /// The text matches only in the same case.
    pub match_case: Option<bool>,
    /// Go to the next or the previous match and select its commit.
    pub step: Option<SearchStep>,
    /// Close the search (Escape in its field); its text stays for the next ⌘F.
    pub close: bool,
    /// How many matches to list; 100 when absent (`count` says how many there are).
    pub limit: Option<usize>,
    /// Wait until the graph is read (it is read in the background after a branch moved).
    pub wait: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub(super) open: bool,
    text: String,
    regex: bool,
    match_case: bool,
    matches: Arc<Vec<usize>>,
    current: Option<usize>,
    current_id: Option<String>,
    error: Option<String>,
    revision: Option<u64>,
    micros: u64,
}

impl SearchState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn regex(&self) -> bool {
        self.regex
    }

    pub fn match_case(&self) -> bool {
        self.match_case
    }

    pub fn matches(&self) -> &[usize] {
        &self.matches
    }

    pub fn is_match(&self, row: usize) -> bool {
        self.matches.binary_search(&row).is_ok()
    }

    pub fn current_row(&self) -> Option<usize> {
        self.current.and_then(|at| self.matches.get(at).copied())
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn revision(&self) -> Option<u64> {
        self.revision
    }

    pub(super) fn summary(&self) -> SearchSummary {
        SearchSummary {
            open: self.open,
            text: self.text.clone(),
            regex: self.regex,
            match_case: self.match_case,
            count: self.matches.len(),
            current: self.current,
            error: self.error.clone(),
        }
    }
}

/// The search of the graph, as the workspace's state reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchSummary {
    /// Whether the search bar is open.
    pub open: bool,
    /// The text searched for.
    pub text: String,
    /// Whether it is a regular expression.
    pub regex: bool,
    /// Whether it matches only in the same case.
    pub match_case: bool,
    /// How many rows match.
    pub count: usize,
    /// The current match's place among them, from 0.
    pub current: Option<usize>,
    /// Why the text could not be searched for (a regular expression that is not valid).
    pub error: Option<String>,
}

/// A row that matches.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchMatch {
    /// Its row in the graph (vcs.graph.page).
    pub index: usize,
    /// The commit's full id (a stash's own commit for a stash's row).
    pub id: String,
    /// The message's first line.
    pub subject: String,
}

/// What a search found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphSearch {
    /// Whether the search bar is open.
    pub open: bool,
    /// The text searched for.
    pub text: String,
    /// Whether it is a regular expression.
    pub regex: bool,
    /// Whether it matches only in the same case.
    pub match_case: bool,
    /// How many rows match.
    pub count: usize,
    /// The current match's place among them, from 0: the bar shows it as "3 of 12".
    pub current: Option<usize>,
    /// The current match's row (its commit is selected after a step).
    pub current_index: Option<usize>,
    /// The first matches, top first (`limit` of them).
    pub matches: Vec<SearchMatch>,
    /// Why the text could not be searched for (a regular expression that is not valid).
    pub error: Option<String>,
    /// How long the search took, in microseconds.
    pub micros: u64,
}

pub(super) fn matching_rows(graph: &Graph, matcher: &TextMatcher) -> Vec<usize> {
    (0..graph.total())
        .filter(|&row| {
            let parts = graph.parts(row);
            let id = match parts.id {
                RowId::Commit(id) => matcher.matches_oid(&id),
                RowId::Stash(id) => matcher.matches_id(id),
                RowId::None => false,
            };
            id || matcher.matches(parts.subject)
                || matcher.matches(parts.body)
                || matcher.matches(parts.author)
                || matcher.matches(parts.email)
        })
        .collect()
}

pub struct SearchGraph;

impl Command for SearchGraph {
    type Params = SearchParams;
    type Output = GraphSearch;
    const ID: &'static str = "vcs.graph.search";
    const TITLE: &'static str = "Search Commits";
    const DESCRIPTION: &'static str = "Searches the Git workspace's graph as its search bar (⌘F in the tab) does, and opens the \
         bar: `text` is found in commit messages, at the start of ids (SHA) and in authors' names and \
         e-mails, in any case unless `matchCase`, or as a regular expression (`regex`, the Log's \
         rules). The matching rows are highlighted; a new text selects the first match from the \
         selected commit down; `step` next or previous moves to the next or the previous match \
         (wrapping) and selects it. Without parameters it searches again (after the graph changed). \
         `close` closes the bar (Escape). Answers with the count, the current match (`current`, from \
         0, and its row) and the first matches.";

    fn run(&self, session: &mut Session, params: SearchParams) -> Result<GraphSearch, CommandError> {
        run_search(session, params)
    }
}

fn run_search(session: &mut Session, params: SearchParams) -> Result<GraphSearch, CommandError> {
    let limit = params.limit.unwrap_or(MATCHES);
    if params.close {
        let search = &mut session.state_mut::<GitWorkspace>().search;
        search.open = false;
        search.matches = Arc::default();
        search.current = None;
        search.current_id = None;
        search.error = None;
        search.revision = None;
        return Ok(answer(session.state_mut::<GitWorkspace>(), &Graph::default(), 0));
    }
    repository(session)?;
    fresh_graph(session, params.wait)?;
    let (shared, revision) = {
        let slot = vcs(session).graph.slot();
        (slot.graph(), slot.revision)
    };
    let graph = read(&shared);
    let workspace = session.state_mut::<GitWorkspace>();
    let selected = workspace.selected_commit().and_then(|id| graph.row_of(id));
    let search = &mut workspace.search;
    let mut typed = !search.open;
    search.open = true;
    if let Some(text) = params.text
        && text != search.text
    {
        search.text = text;
        typed = true;
    }
    if let Some(regex) = params.regex
        && regex != search.regex
    {
        search.regex = regex;
        typed = true;
    }
    if let Some(match_case) = params.match_case
        && match_case != search.match_case
    {
        search.match_case = match_case;
        typed = true;
    }
    if typed || search.revision != Some(revision) {
        let started = std::time::Instant::now();
        (search.matches, search.error) = match TextMatcher::new(&search.text, search.regex, search.match_case) {
            Ok(Some(matcher)) => (Arc::new(matching_rows(&graph, &matcher)), None),
            Ok(None) => (Arc::default(), None),
            Err(error) => (Arc::default(), Some(message(&error))),
        };
        search.micros = started.elapsed().as_micros() as u64;
        search.revision = Some(revision);
        let matches = &search.matches;
        search.current =
            search.current_id.as_ref().and_then(|id| graph.row_of(id)).and_then(|row| matches.binary_search(&row).ok());
        if typed {
            search.current = (!matches.is_empty())
                .then(|| selected.map_or(0, |row| matches.partition_point(|&at| at < row) % matches.len()));
        }
    }
    let count = search.matches.len();
    let mut select = typed && search.current.is_some();
    if let Some(step) = params.step
        && count > 0
    {
        let matches = &search.matches;
        search.current = Some(match (step, search.current) {
            (SearchStep::Next, Some(at)) => (at + 1) % count,
            (SearchStep::Previous, Some(at)) => (at + count - 1) % count,
            (SearchStep::Next, None) => selected.map_or(0, |row| matches.partition_point(|&at| at <= row) % count),
            (SearchStep::Previous, None) => selected
                .map_or(count - 1, |row| matches.partition_point(|&at| at < row).checked_sub(1).unwrap_or(count - 1)),
        });
        select = true;
    }
    search.current_id = search.current_row().and_then(|row| graph.id(row));
    if select && let Some(id) = search.current_id.clone() {
        workspace.select_commit(id);
    }
    Ok(answer(workspace, &graph, limit))
}

fn answer(workspace: &GitWorkspace, graph: &Graph, limit: usize) -> GraphSearch {
    let search = &workspace.search;
    GraphSearch {
        open: search.open,
        text: search.text.clone(),
        regex: search.regex,
        match_case: search.match_case,
        count: search.matches.len(),
        current: search.current,
        current_index: search.current_row(),
        matches: search
            .matches
            .iter()
            .take(limit)
            .filter(|&&row| row < graph.total())
            .filter_map(|&row| {
                Some(SearchMatch { index: row, id: graph.id(row)?, subject: graph.parts(row).subject.to_owned() })
            })
            .collect(),
        error: search.error.clone(),
        micros: search.micros,
    }
}

pub struct PanelFilter(Option<TextMatcher>);

impl PanelFilter {
    pub fn new(text: &str) -> Self {
        PanelFilter(TextMatcher::new(text, false, false).ok().flatten())
    }

    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }

    pub fn shows(&self, name: &str) -> bool {
        self.0.as_ref().is_none_or(|matcher| matcher.matches(name))
    }
}

/// The left panel's filter text.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterParams {
    /// The text; empty shows everything.
    pub text: String,
}

/// What the left panel shows with its filter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PanelList {
    /// The filter's text.
    pub text: String,
    /// The local branches that show.
    pub local: Vec<String>,
    /// The remote branches that show (`origin/main`; the remote's name matches too).
    pub remote: Vec<String>,
    /// The tags that show.
    pub tags: Vec<String>,
    /// The stashes that show (`stash@{0}`; their messages match too).
    pub stashes: Vec<String>,
    /// The worktrees that show, by folder (their folder's name and their branch match).
    pub worktrees: Vec<std::path::PathBuf>,
    /// The submodules that show, by path (their path and their name match).
    pub submodules: Vec<String>,
}

pub struct FilterPanel;

impl Command for FilterPanel {
    type Params = FilterParams;
    type Output = PanelList;
    const ID: &'static str = "vcs.workspace.filter";
    const TITLE: &'static str = "Filter Branches";
    const DESCRIPTION: &'static str = "Filters the left panel of the Git workspace as typing in its filter field does: only the \
         local branches, remote branches (by `origin/name`), tags, stashes (by name or message), \
         worktrees (by folder name or branch) and submodules (by path or name) whose names contain `text`, in any case, are listed; an empty text lists everything. The \
         graph is not changed (vcs.graph.hide and vcs.graph.solo do that). Answers with what is \
         listed.";

    fn run(&self, session: &mut Session, params: FilterParams) -> Result<PanelList, CommandError> {
        let branches = super::branches::ListBranches.run(session, NoParams::default())?;
        let stashes = super::stash::ListStashes.run(session, NoParams::default())?;
        let filter = PanelFilter::new(&params.text);
        let names = |names: Vec<String>| names.into_iter().filter(|name| filter.shows(name)).collect::<Vec<_>>();
        let list = PanelList {
            local: names(branches.local.into_iter().map(|branch| branch.name).collect()),
            remote: names(branches.remote.into_iter().map(|branch| branch.name).collect()),
            tags: names(branches.tags.into_iter().map(|tag| tag.name).collect()),
            stashes: stashes
                .stashes
                .into_iter()
                .filter(|stash| filter.shows(&stash.name) || filter.shows(&stash.message))
                .map(|stash| stash.name)
                .collect(),
            worktrees: super::worktree::ListWorktrees
                .run(session, super::worktree::ListParams::default())?
                .worktrees
                .into_iter()
                .filter(|worktree| {
                    filter.shows(&worktree.name) || worktree.branch.as_ref().is_some_and(|b| filter.shows(b))
                })
                .map(|worktree| worktree.path)
                .collect(),
            submodules: super::worktree::ListSubmodules
                .run(session, Default::default())
                .map(|list| list.submodules)
                .unwrap_or_default()
                .into_iter()
                .filter(|submodule| filter.shows(&submodule.path) || filter.shows(&submodule.name))
                .map(|submodule| submodule.path)
                .collect(),
            text: params.text.clone(),
        };
        session.state_mut::<GitWorkspace>().filter = params.text;
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::testing::*;
    use super::*;

    fn session(dir: &std::path::Path) -> (Registry, Session) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir })).unwrap();
        (registry, session)
    }

    fn repository() -> tempfile::TempDir {
        let dir = test_repository();
        let root = dir.path();
        git(root, &["switch", "-q", "-c", "old"]);
        std::fs::write(root.join("o.txt"), "old\n").unwrap();
        git(root, &["add", "o.txt"]);
        git(root, &["commit", "-q", "-m", "old experiment"]);
        git(root, &["update-ref", "refs/remotes/upstream/old", "HEAD"]);
        git(root, &["switch", "-q", "main"]);
        git(root, &["branch", "-q", "-D", "old"]);
        git(root, &["switch", "-q", "-c", "feature"]);
        std::fs::write(root.join("f.txt"), "feature\n").unwrap();
        git(root, &["add", "f.txt"]);
        git(root, &["commit", "-q", "--author", "Grace Hopper <grace@navy.example>", "-m", "feature work"]);
        git(root, &["update-ref", "refs/remotes/origin/feature", "HEAD"]);
        git(root, &["switch", "-q", "main"]);
        std::fs::write(root.join("m.txt"), "main\n").unwrap();
        git(root, &["add", "m.txt"]);
        git(root, &["commit", "-q", "-m", "Fix the parser", "-m", "The lexer stays."]);
        git(root, &["tag", "v1"]);
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        git(root, &["stash", "push", "-q", "-m", "halfway"]);
        dir
    }

    fn subjects(registry: &Registry, session: &mut Session) -> Vec<String> {
        let page = registry.dispatch(session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        page["commits"].as_array().unwrap().iter().map(|row| row["subject"].as_str().unwrap().to_owned()).collect()
    }

    fn matched(found: &Value) -> Vec<String> {
        found["matches"].as_array().unwrap().iter().map(|row| row["subject"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn search_finds_messages_ids_and_authors_and_steps_between_matches() {
        let dir = repository();
        let root = dir.path();
        let (registry, mut session) = session(root);
        let s = &mut session;
        let run = |s: &mut Session, params: Value| registry.dispatch(s, "vcs.graph.search", params).unwrap();
        let rows = subjects(&registry, s);
        let row = |subject: &str| rows.iter().position(|row| row == subject).unwrap();

        let found = run(s, json!({ "text": "PARSER", "wait": true }));
        assert_eq!((found["open"].clone(), found["count"].clone()), (json!(true), json!(1)));
        assert_eq!(matched(&found), ["Fix the parser"]);
        assert_eq!(matched(&run(s, json!({ "text": "lexer" }))), ["Fix the parser"], "the body");
        assert_eq!(matched(&run(s, json!({ "text": "grace" }))), ["feature work"], "the author");
        assert_eq!(matched(&run(s, json!({ "text": "navy.example" }))), ["feature work"], "the e-mail");
        let first = git(root, &["rev-parse", "main~1"]).trim().to_owned();
        let by_id = run(s, json!({ "text": &first[..8] }));
        assert_eq!(matched(&by_id), ["First"], "the start of the id");
        assert_eq!(by_id["matches"][0]["id"], json!(first));
        let state = registry.dispatch(s, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["selected"], json!(first));
        assert_eq!(state["search"]["text"], json!(&first[..8]));

        assert_eq!(run(s, json!({ "text": "fix", "matchCase": true }))["count"], json!(0));
        assert_eq!(run(s, json!({ "text": "Fix" }))["count"], json!(1));
        let regex = run(s, json!({ "text": "^(old|feature) ", "regex": true, "matchCase": false }));
        let mut expected = vec!["feature work", "old experiment"];
        expected.sort_by_key(|subject| row(subject));
        assert_eq!(matched(&regex), expected);
        let bad = run(s, json!({ "text": "(" }));
        assert_eq!(bad["count"], json!(0));
        assert!(bad["error"].is_string(), "an invalid expression is said: {bad}");

        let all = run(s, json!({ "text": "e", "regex": false }));
        let count = all["count"].as_u64().unwrap() as usize;
        assert!(count >= 4, "{all}");
        let start = all["current"].as_u64().unwrap() as usize;
        let next = run(s, json!({ "step": "next" }));
        assert_eq!(next["current"], json!((start + 1) % count));
        let id = next["matches"][next["current"].as_u64().unwrap() as usize]["id"].clone();
        assert_eq!(registry.dispatch(s, "vcs.workspace.state", json!({})).unwrap()["selected"], id, "selected");
        let back = run(s, json!({ "step": "previous" }));
        assert_eq!(back["current"], json!(start));
        for _ in 0..count {
            run(s, json!({ "step": "next" }));
        }
        assert_eq!(run(s, json!({}))["current"], json!(start), "around and back");

        let closed = run(s, json!({ "close": true }));
        assert_eq!((closed["open"].clone(), closed["count"].clone()), (json!(false), json!(0)));
        let state = registry.dispatch(s, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!((state["search"]["open"].clone(), state["search"]["text"].clone()), (json!(false), json!("e")));
        assert_eq!(run(s, json!({}))["count"], json!(count), "opens again with it");
    }

    #[test]
    fn hide_and_solo_focus_the_graph_on_branches_and_remotes() {
        let dir = repository();
        let (registry, mut session) = session(dir.path());
        let s = &mut session;
        let run = |s: &mut Session, command: &str, params: Value| registry.dispatch(s, command, params);
        let all = subjects(&registry, s);
        assert_eq!(all.len(), 5, "four commits and the stash: {all:?}");
        let labels = |s: &mut Session| -> Vec<String> {
            let page = registry.dispatch(s, "vcs.graph.page", json!({})).unwrap();
            let mut names: Vec<String> = page["commits"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|row| {
                    row["refs"].as_array().unwrap().iter().map(|label| label["name"].as_str().unwrap().to_owned())
                })
                .collect();
            names.sort();
            names
        };

        let info = run(s, "vcs.graph.hide", json!({ "name": "feature" })).unwrap();
        assert_eq!(info["hidden"], json!([{ "name": "feature", "kind": "local" }]));
        assert_eq!(subjects(&registry, s).len(), 5);
        assert!(!labels(s).contains(&"feature".to_owned()));
        let info =
            run(s, "vcs.graph.hide", json!({ "name": "origin", "kind": "wholeRemote", "hidden": true })).unwrap();
        assert_eq!((info["total"].clone(), info["hiddenRows"].clone()), (json!(4), json!(1)));
        assert!(!subjects(&registry, s).contains(&"feature work".to_owned()));
        let page = run(s, "vcs.graph.page", json!({})).unwrap();
        assert_eq!(page["hidden"], json!(1));
        run(s, "vcs.graph.hide", json!({ "name": "origin", "kind": "wholeRemote" })).unwrap();
        assert!(subjects(&registry, s).contains(&"feature work".to_owned()));
        run(s, "vcs.graph.hide", json!({ "name": "upstream/old", "kind": "remote" })).unwrap();
        assert!(!subjects(&registry, s).contains(&"old experiment".to_owned()));
        let info = run(s, "vcs.graph.hide", json!({ "showAll": true })).unwrap();
        assert_eq!((info["hidden"].clone(), info["total"].clone()), (json!([]), json!(5)));

        let info = run(s, "vcs.graph.solo", json!({ "name": "main" })).unwrap();
        assert_eq!(info["solo"], json!([{ "name": "main", "kind": "local" }]));
        assert_eq!(subjects(&registry, s), ["On main: halfway", "Fix the parser", "First"]);
        assert_eq!(labels(s), ["main", "v1"], "the tag at a shown commit still shows");
        run(s, "vcs.graph.solo", json!({ "name": "upstream", "kind": "wholeRemote" })).unwrap();
        let shown = subjects(&registry, s);
        assert!(
            shown.contains(&"old experiment".to_owned()) && !shown.contains(&"feature work".to_owned()),
            "{shown:?}"
        );
        run(s, "vcs.graph.hide", json!({ "name": "main" })).unwrap();
        assert!(!subjects(&registry, s).contains(&"Fix the parser".to_owned()));
        run(s, "vcs.graph.hide", json!({ "showAll": true })).unwrap();
        let info = run(s, "vcs.graph.solo", json!({ "clear": true })).unwrap();
        assert_eq!((info["solo"].clone(), info["total"].clone()), (json!([]), json!(5)));

        let found = run(s, "vcs.graph.search", json!({ "text": "work" })).unwrap();
        assert_eq!(found["count"], json!(1));
        run(s, "vcs.graph.solo", json!({ "name": "main", "solo": true })).unwrap();
        let state = run(s, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["search"]["count"], json!(0), "feature work is out of the graph");
        assert_eq!(state["focus"]["solo"][0]["name"], json!("main"));

        assert!(run(s, "vcs.graph.hide", json!({ "name": "nope" })).is_err());
        assert!(run(s, "vcs.graph.solo", json!({ "name": "origin", "kind": "local" })).is_err());
        assert!(run(s, "vcs.graph.hide", json!({})).is_err(), "nothing named");
        let set = json!({ "set": {
            "hidden": [{ "name": "gone" }, { "name": "origin/feature", "kind": "remote" }],
            "solo": [],
        } });
        let info = run(s, "vcs.graph.focus", set).unwrap();
        assert_eq!(info["hidden"], json!([{ "name": "origin/feature", "kind": "remote" }]));
        assert_eq!(info["solo"], json!([]));
        assert_eq!(run(s, "vcs.graph.focus", json!({})).unwrap(), info, "read back");
        let focus = s.state::<super::super::Vcs>().and_then(super::super::Vcs::graph_focus).unwrap();
        assert_eq!(focus.0.canonicalize().unwrap(), dir.path().canonicalize().unwrap());
        assert_eq!(focus.1.hidden.len(), 1);
    }

    #[test]
    fn the_filter_lists_the_left_panel_by_name() {
        let dir = repository();
        let (registry, mut session) = session(dir.path());
        let filter = |s: &mut Session, text: &str| {
            registry.dispatch(s, "vcs.workspace.filter", json!({ "text": text })).unwrap()
        };
        let list = filter(&mut session, "FEAT");
        assert_eq!((list["local"].clone(), list["remote"].clone()), (json!(["feature"]), json!(["origin/feature"])));
        assert_eq!(list["tags"], json!([]));
        assert_eq!(filter(&mut session, "upstream")["remote"], json!(["upstream/old"]), "by the remote's name");
        assert_eq!(filter(&mut session, "halfway")["stashes"], json!(["stash@{0}"]), "a stash by its message");
        let everything = filter(&mut session, "");
        assert_eq!(everything["local"], json!(["feature", "main"]));
        assert_eq!(everything["tags"], json!(["v1"]));
        filter(&mut session, "v");
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["filter"], json!("v"));
        assert!(PanelFilter::new("Ma").shows("main") && !PanelFilter::new("x").shows("main"));
        assert!(!PanelFilter::new(" ").is_active());
    }

    #[test]
    fn the_focus_rules_follow_hidden_and_soloed_targets() {
        let target = |name: &str, kind| FocusTarget { name: name.into(), kind };
        let mut focus = GraphFocus::default();
        assert!(focus.tip("main", RefKind::Local) && focus.tip("v1", RefKind::Tag) && focus.tip("HEAD", RefKind::Head));
        focus.hidden.push(target("origin", FocusKind::WholeRemote));
        assert!(!focus.tip("origin/main", RefKind::Remote) && focus.tip("upstream/main", RefKind::Remote));
        assert!(focus.tip("origin", RefKind::Local), "a local branch named like the remote is not on it");
        assert!(!focus.label("origin/main", RefKind::Remote) && focus.label("v1", RefKind::Tag));
        focus.solo.push(target("main", FocusKind::Local));
        assert!(focus.tip("main", RefKind::Local) && !focus.tip("topic", RefKind::Local));
        assert!(!focus.tip("v1", RefKind::Tag) && focus.label("v1", RefKind::Tag), "a tag shows, but keeps nothing");
        let mut list = Vec::new();
        assert!(switch(&mut list, target("b", FocusKind::Local), None));
        assert!(switch(&mut list, target("a", FocusKind::Local), Some(true)));
        assert_eq!(list.iter().map(|target| target.name.as_str()).collect::<Vec<_>>(), ["a", "b"], "sorted");
        assert!(!switch(&mut list, target("b", FocusKind::Local), None));
        assert!(!switch(&mut list, target("c", FocusKind::Local), Some(false)));
        assert_eq!(list.len(), 1);
    }

    fn long_history(count: usize) -> tempfile::TempDir {
        let mut stream = String::new();
        for index in 0..count {
            let who = [("Ada Lovelace", "ada"), ("Grace Hopper", "grace"), ("Alan Turing", "alan")][index % 3];
            let message = format!("Change number {index}\n\nTouches the module {}.\n", index % 97);
            let parents: Vec<usize> = (index > 0).then_some(index).into_iter().collect();
            let time = 1_700_000_000 + index as i64;
            stream.push_str(&imported_commit("refs/heads/main", index + 1, who, time, &message, &parents, "f.txt"));
        }
        fast_import(&stream)
    }

    #[test]
    fn searching_twelve_thousand_commits_answers_at_once() {
        let dir = long_history(12_000);
        let (registry, mut session) = session(dir.path());
        let page = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true, "limit": 1 })).unwrap();
        assert_eq!(page["total"], json!(12_000));
        let bound = if cfg!(debug_assertions) { 2_000_000 } else { 200_000 };
        for (text, regex, count) in [
            ("module 7.", false, 124),
            ("GRACE", false, 4_000),
            ("number 1199", false, 11),
            ("^Change number 9+$", true, 4),
        ] {
            let started = std::time::Instant::now();
            let found =
                registry.dispatch(&mut session, "vcs.graph.search", json!({ "text": text, "regex": regex })).unwrap();
            let took = started.elapsed().as_micros() as u64;
            assert_eq!(found["count"], json!(count), "{text}");
            assert!(took < bound, "{text}: {took} µs");
            assert!(found["micros"].as_u64().unwrap() <= took);
        }
        let now = registry.dispatch(&mut session, "vcs.graph.search", json!({})).unwrap()["current"].as_u64().unwrap();
        let next = registry.dispatch(&mut session, "vcs.graph.search", json!({ "step": "next" })).unwrap();
        assert_eq!(next["current"], json!((now + 1) % 4));
    }
}
