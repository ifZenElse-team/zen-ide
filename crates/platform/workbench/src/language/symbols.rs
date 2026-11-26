use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lsp_types::OneOf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_lsp::{LspError, Pending};
use zen_syntax::SymbolKind;

use super::structure::symbol_kind;
use super::{AbsentServer, Languages, MAX_WAIT, Reason, Server, WorkProgress, char_offset, with_languages};
use crate::Workbench;

pub const DEBOUNCE: Duration = Duration::from_millis(120);

pub(super) const DEFAULT_LIMIT: usize = 50;

pub(super) const MAX_LIMIT: usize = 500;

const MAX_FOUND: usize = 5_000;

const MAX_REAL_DIRS: usize = 10_000;

pub(super) const TYPE_KINDS: [u64; 4] = [5, 10, 11, 23];

const NOT_SYMBOLS: [u64; 4] = [1, 2, 3, 4];

struct Found {
    name: String,
    kind: u64,
    container: Option<String>,
    path: PathBuf,
    position: lsp_types::Position,
}

struct Request {
    language: String,
    pending: Pending,
    retry: bool,
}

struct Ranked {
    index: usize,
    in_project: bool,
    rank: zen_index::Rank,
    qualified: String,
    ranges: Vec<std::ops::Range<usize>>,
    place: Option<(usize, u32)>,
}

pub(crate) struct SymbolSearch {
    query: String,
    changed: Instant,
    waiting: Vec<Request>,
    found: Vec<Found>,
    answered: bool,
    last: Vec<Found>,
    asked: Vec<String>,
    ranked: Option<(String, Vec<Ranked>)>,
    real_dirs: RealDirs,
    stale: bool,
    providers: usize,
}

#[derive(Default)]
pub(super) struct Searched {
    pub(super) pending: bool,
    pub(super) reason: Option<Reason>,
    pub(super) progress: Option<WorkProgress>,
    pub(super) asked: bool,
}

impl Searched {
    pub(super) fn reason_if_empty(&self, empty: bool) -> Option<Reason> {
        match self.reason {
            Some(reason) => Some(reason),
            None if !empty => None,
            None if self.asked => Some(Reason::NoMatch),
            None => Some(Reason::NoServer),
        }
    }
}

pub(super) struct Matched {
    pub(super) name: String,
    pub(super) qualified: String,
    pub(super) kind: u64,
    pub(super) container: Option<String>,
    pub(super) path: PathBuf,
    pub(super) relative: String,
    pub(super) offset: usize,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) in_project: bool,
    pub(super) ranges: Vec<std::ops::Range<usize>>,
}

fn provides(client: &zen_lsp::Client) -> bool {
    client
        .capabilities()
        .and_then(|capabilities| capabilities.workspace_symbol_provider.as_ref())
        .is_some_and(|provider| !matches!(provider, OneOf::Left(false)))
}

pub(super) fn server_query(typed: &str) -> String {
    let (query, _) = zen_index::split_location(typed.trim());
    let letters: String = query.replace("::", ".").chars().filter(|c| *c != '*' && !c.is_whitespace()).collect();
    letters.split('.').filter(|part| !part.is_empty()).collect::<Vec<_>>().join(".")
}

fn last_part(query: &str) -> Option<&str> {
    let at = query.rfind(['.', ':'])?;
    Some(&query[at + 1..]).filter(|last| !last.is_empty())
}

impl Languages {
    pub(super) fn explain_no_server(
        &mut self,
        workbench: &Workbench,
        pending: bool,
        reason: Option<Reason>,
    ) -> (bool, Option<Reason>, Option<AbsentServer>) {
        let idle = !self.servers.values().any(|server| matches!(server, Server::Running { .. } | Server::Starting(_)));
        let explained = matches!(reason, Some(Reason::NoServer)) || (matches!(reason, Some(Reason::Searching)) && idle);
        let absence = workbench.project().filter(|_| explained).and_then(|root| self.project_absence(root, true));
        let absence =
            absence.filter(|absence| reason == Some(Reason::NoServer) || absence.reason == Reason::ServerMissing);
        if let Some(absence) = absence {
            let (reason, absent) = absence.split();
            return (pending && reason == Reason::Starting, Some(reason), Some(absent));
        }
        let answered = !pending && matches!(reason, Some(Reason::NoMatch | Reason::NoServer));
        if let Some(missing) =
            workbench.project().filter(|_| answered).and_then(|root| self.missing_project_server(root, true))
        {
            let (reason, absent) = missing.split();
            return (false, Some(reason), Some(absent));
        }
        (pending, reason, None)
    }

    pub(super) fn search_waiting(
        &mut self,
        workbench: &Workbench,
        typed: &str,
        asked: Duration,
        explicit: bool,
    ) -> Searched {
        let mut wait = if explicit { super::SearchWait::explicit(asked) } else { super::SearchWait::new(asked) };
        let mut searched = self.search_symbols(workbench, typed, asked.min(MAX_WAIT));
        while wait.keeps_waiting(searched.pending, searched.reason) {
            searched = self.search_symbols(workbench, typed, wait.left().min(Duration::from_millis(500)));
        }
        searched
    }

    pub(super) fn search_symbols(&mut self, workbench: &Workbench, typed: &str, wait: Duration) -> Searched {
        let deadline = Instant::now() + wait;
        let query = server_query(typed);
        let query = query.as_str();
        if self.symbols.as_ref().is_none_or(|search| search.query != query) {
            let old = self.symbols.take();
            let (waiting, last, mut real_dirs) = old
                .map(|old| (old.waiting, if old.answered { old.found } else { old.last }, old.real_dirs))
                .unwrap_or_default();
            if real_dirs.len() > MAX_REAL_DIRS {
                real_dirs.clear();
            }
            for request in waiting {
                if let Some(Server::Running { client, .. }) = self.servers.get(&request.language) {
                    client.cancel(&request.pending);
                }
            }
            self.symbols = Some(SymbolSearch {
                query: query.to_owned(),
                changed: Instant::now(),
                waiting: Vec::new(),
                found: Vec::new(),
                answered: false,
                last,
                asked: Vec::new(),
                ranked: None,
                real_dirs,
                stale: false,
                providers: 0,
            });
        }
        if wait.is_zero() && self.symbols.as_ref().is_some_and(|search| search.changed.elapsed() < DEBOUNCE) {
            return Searched { pending: true, reason: Some(Reason::Searching), progress: None, asked: false };
        }
        let last = last_part(query);
        loop {
            let mut scanning = false;
            if let Some(root) = workbench.project() {
                let servers = self.project_servers(root);
                scanning = self.scanning(root);
                self.keep_running.extend(servers);
            }
            self.sync(workbench);
            let mut starting = false;
            let progress = self.work_progress();
            let Some(search) = self.symbols.as_mut() else {
                return Searched::default();
            };
            if search.stale && progress.is_none() && search.waiting.is_empty() {
                search.stale = false;
                search.asked.clear();
                search.last = std::mem::take(&mut search.found);
                search.answered = false;
                search.ranked = None;
                search.providers = 0;
            }
            for (language, server) in &mut self.servers {
                match server {
                    Server::Starting(_) => starting = true,
                    Server::Running { client, .. } if !search.asked.contains(language) => {
                        search.asked.push(language.clone());
                        if !provides(client) {
                            continue;
                        }
                        search.providers += 1;
                        let keeps_dotted = client.server_name() == Some("gopls");
                        let mut ask = |text: &str, retry: bool| {
                            if let Ok(pending) = client.request("workspace/symbol", json!({ "query": text })) {
                                search.waiting.push(Request { language: language.clone(), pending, retry });
                            }
                        };
                        ask(query, keeps_dotted && last.is_some());
                        if let Some(last) = last.filter(|_| !keeps_dotted) {
                            ask(last, false);
                        }
                    }
                    _ => {}
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let mut still = Vec::new();
            let mut answered_by = Vec::new();
            for request in std::mem::take(&mut search.waiting) {
                let answer = if left.is_zero() {
                    request.pending.try_answer()
                } else {
                    match request.pending.wait(left.min(Duration::from_millis(20))) {
                        Err(LspError::Timeout) => None,
                        other => Some(other),
                    }
                };
                let Some(answer) = answer else {
                    still.push(request);
                    continue;
                };
                let symbols = answer.as_ref().map(read).unwrap_or_default();
                if symbols.is_empty()
                    && request.retry
                    && let Some(last) = last
                    && let Some(Server::Running { client, .. }) = self.servers.get_mut(&request.language)
                    && let Ok(pending) = client.request("workspace/symbol", json!({ "query": last }))
                {
                    still.push(Request { language: request.language, pending, retry: false });
                    continue;
                }
                answered_by.push(request.language);
                let room = MAX_FOUND.saturating_sub(search.found.len());
                search.found.extend(symbols.into_iter().take(room));
                search.answered = true;
                search.ranked = None;
            }
            search.waiting = still;
            let mut progress = progress;
            if !answered_by.is_empty() {
                self.sync(workbench);
                progress = self.work_progress();
                let working = answered_by.iter().any(|id| self.progress.get(id).is_some_and(|works| !works.is_empty()));
                let Some(search) = self.symbols.as_mut() else { return Searched::default() };
                search.stale |= working;
            }
            let Some(search) = self.symbols.as_mut() else { return Searched::default() };
            let indexing = search.stale && progress.is_some();
            let reason = if scanning {
                Some(Reason::Scanning)
            } else if starting {
                Some(Reason::Starting)
            } else if indexing || (!search.waiting.is_empty() && progress.is_some()) {
                Some(Reason::Indexing)
            } else if !search.waiting.is_empty() {
                Some(Reason::Searching)
            } else {
                None
            };
            let busy = reason.is_some();
            if !busy && !search.answered {
                search.answered = true;
                search.ranked = None;
            }
            if !busy || Instant::now() >= deadline {
                let progress = progress.filter(|_| reason == Some(Reason::Indexing));
                return Searched { pending: busy, reason, progress, asked: search.providers > 0 };
            }
            if search.waiting.is_empty() {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    pub(super) fn matched_symbols(
        &mut self,
        workbench: &Workbench,
        typed: &str,
        keep: impl Fn(u64) -> bool,
        limit: usize,
    ) -> Vec<Matched> {
        let query = server_query(typed);
        let Some(search) = self.symbols.as_mut().filter(|search| search.query == query) else { return Vec::new() };
        let SymbolSearch { found, last, answered, ranked, real_dirs, .. } = search;
        let symbols = if *answered { &*found } else { &*last };
        if ranked.as_ref().is_none_or(|(text, _)| text != typed) {
            *ranked = Some((typed.to_owned(), rank(typed, symbols, workbench.project(), real_dirs)));
        }
        let Some((_, ranked)) = ranked.as_mut() else { return Vec::new() };
        ranked
            .iter_mut()
            .filter(|entry| keep(symbols[entry.index].kind))
            .take(limit.min(MAX_LIMIT))
            .map(|entry| {
                let found = &symbols[entry.index];
                let (offset, column) = *entry.place.get_or_insert_with(|| place(workbench, found));
                Matched {
                    name: entry.qualified.rsplit('.').next().unwrap_or(&entry.qualified).to_owned(),
                    qualified: entry.qualified.clone(),
                    kind: found.kind,
                    container: found.container.clone(),
                    path: found.path.clone(),
                    relative: super::project_relative(&found.path, workbench.project()),
                    offset,
                    line: found.position.line + 1,
                    column,
                    in_project: entry.in_project,
                    ranges: entry.ranges.clone(),
                }
            })
            .collect()
    }
}

fn rank(query: &str, symbols: &[Found], root: Option<&Path>, real_dirs: &mut RealDirs) -> Vec<Ranked> {
    let pattern = zen_index::Pattern::qualified(query);
    if pattern.name().is_empty() {
        return Vec::new();
    }
    let mut project = ProjectRoot::new(root, real_dirs);
    let mut seen = HashSet::new();
    let mut ranked: Vec<Ranked> = symbols
        .iter()
        .enumerate()
        .filter(|(_, found)| {
            seen.insert((found.name.as_str(), found.path.as_path(), found.position.line, found.position.character))
        })
        .filter_map(|(index, found)| {
            let qualified = qualified_name(&found.name, found.container.as_deref(), found.kind);
            let outer: Vec<&str> = found
                .container
                .as_deref()
                .map(|container| container.split(['/', '.', ':']).filter(|part| !part.is_empty()).collect())
                .unwrap_or_default();
            let matched = pattern.matches_qualified(&qualified, &outer)?;
            let in_project = project.contains(&found.path);
            let length = qualified.rsplit('.').next().unwrap_or(&qualified).chars().count();
            let rank = zen_index::Rank::new(&matched, length).in_project(in_project);
            Some(Ranked { index, in_project, rank, qualified, ranges: matched.ranges, place: None })
        })
        .collect();
    ranked.sort_by_key(|entry| entry.rank);
    ranked
}

pub(super) type RealDirs = HashMap<PathBuf, Option<PathBuf>>;

pub(super) struct ProjectRoot<'a> {
    roots: Vec<PathBuf>,
    real_dirs: &'a mut RealDirs,
}

impl<'a> ProjectRoot<'a> {
    pub(super) fn new(root: Option<&Path>, real_dirs: &'a mut RealDirs) -> Self {
        let mut roots: Vec<PathBuf> = root.map(Path::to_path_buf).into_iter().collect();
        if let Some(real) = root.and_then(|root| root.canonicalize().ok())
            && !roots.contains(&real)
        {
            roots.push(real);
        }
        Self { roots, real_dirs }
    }

    pub(super) fn contains(&mut self, path: &Path) -> bool {
        if self.roots.iter().any(|root| path.starts_with(root)) {
            return true;
        }
        let Some(dir) = path.parent().filter(|_| !self.roots.is_empty()) else { return false };
        let real = self.real_dirs.entry(dir.to_path_buf()).or_insert_with(|| dir.canonicalize().ok());
        real.as_ref().is_some_and(|real| self.roots.iter().any(|root| real.starts_with(root)))
    }
}

fn place(workbench: &Workbench, found: &Found) -> (usize, u32) {
    let text = text_of(workbench, &found.path);
    let byte = zen_lsp::position::offset(&text, found.position).unwrap_or(0);
    let line_start = text[..byte].rfind('\n').map_or(0, |at| at + 1);
    (char_offset(&text, byte), text[line_start..byte].chars().count() as u32 + 1)
}

fn read(answer: &Value) -> Vec<Found> {
    let Some(items) = answer.as_array() else { return Vec::new() };
    items
        .iter()
        .filter_map(|item| {
            let kind = item["kind"].as_u64()?;
            if NOT_SYMBOLS.contains(&kind) {
                return None;
            }
            let location = &item["location"];
            let path = zen_lsp::position::uri_path(location["uri"].as_str()?)?;
            let start = &location["range"]["start"];
            let position = lsp_types::Position {
                line: start["line"].as_u64().unwrap_or(0) as u32,
                character: start["character"].as_u64().unwrap_or(0) as u32,
            };
            Some(Found {
                name: item["name"].as_str()?.to_owned(),
                kind,
                container: item["containerName"].as_str().filter(|name| !name.is_empty()).map(str::to_owned),
                path,
                position,
            })
        })
        .collect()
}

const MEMBER_KINDS: [u64; 8] = [6, 7, 8, 9, 10, 12, 14, 22];

fn owner_type(container: &str) -> Option<&str> {
    let owner = container.trim().strip_prefix("impl ").unwrap_or(container.trim());
    let owner = owner.rsplit_once(" for ").map_or(owner, |(_, owner)| owner);
    let owner = owner.split('<').next().unwrap_or_default().trim();
    let owner = owner.rsplit("::").next().unwrap_or_default();
    owner.chars().next().is_some_and(char::is_uppercase).then_some(owner)
}

fn qualified_name(name: &str, container: Option<&str>, kind: u64) -> String {
    let qualified = in_container(name, container);
    match container.and_then(owner_type) {
        Some(owner) if MEMBER_KINDS.contains(&kind) && !qualified.contains('.') => format!("{owner}.{qualified}"),
        _ => qualified,
    }
}

fn in_container(name: &str, container: Option<&str>) -> String {
    let name = plain_name(name);
    let Some(container) = container else { return name };
    let prefixes = std::iter::once(container).chain(container.match_indices('/').map(|(at, _)| &container[at + 1..]));
    prefixes
        .filter(|prefix| !prefix.is_empty())
        .find_map(|prefix| name.strip_prefix(prefix).and_then(|rest| rest.strip_prefix('.')))
        .filter(|rest| !rest.is_empty())
        .map_or_else(|| name.clone(), str::to_owned)
}

fn plain_name(name: &str) -> String {
    if !name.contains(['[', '(']) {
        return name.to_owned();
    }
    let mut plain = String::with_capacity(name.len());
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '[' if depth > 0 || plain.ends_with(|previous: char| previous.is_alphanumeric() || previous == '_') => {
                depth += 1
            }
            ']' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ => plain.push(c),
        }
    }
    let parts: Vec<&str> = plain
        .split('.')
        .map(|part| {
            part.strip_prefix('(')
                .and_then(|part| part.strip_suffix(')'))
                .map(|part| part.trim_start_matches('*'))
                .filter(|part| !part.is_empty() && !part.contains(['(', ')']))
                .unwrap_or(part)
        })
        .collect();
    parts.join(".")
}

fn text_of(workbench: &Workbench, path: &Path) -> String {
    workbench
        .documents()
        .iter()
        .find(|document| document.path() == path)
        .map(|document| document.text().to_owned())
        .unwrap_or_else(|| std::fs::read_to_string(path).unwrap_or_default())
}

/// The parameters of `navigate.symbol.goTo`.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SymbolParams {
    /// The name, its camel humps (`iSt`), or a dotted name with what it belongs to (`Store.get`, `model.Store.get`).
    pub query: String,
    /// How many symbols to return; 50 by default, at most 500.
    #[serde(default)]
    pub limit: Option<usize>,
    /// How long to wait for the servers, in milliseconds: at most 10 000, or 120 000 while a
    /// server is still loading the project (`scanning`, `starting`, `indexing`); not at all when absent.
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// Whether types are listed too; true by default.
    #[serde(default)]
    pub types: Option<bool>,
    /// Whether to answer `rows`, one line of text per symbol (`path:line:column kind name`),
    /// instead of `{symbols}` objects with offsets and matched ranges; false by default.
    #[serde(default)]
    pub compact: Option<bool>,
}

/// A declaration found by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SymbolItem {
    /// Its name.
    pub name: String,
    /// Its name with the type it belongs to, as `Store.Get`.
    pub qualified_name: String,
    /// What it is.
    pub kind: SymbolKind,
    /// Where it is declared, as the server says it: for Go, the package path.
    pub container: Option<String>,
    /// The file it is declared in.
    #[serde(skip)]
    pub path: PathBuf,
    /// The file relative to the project, with `/`, or the whole path outside it (card WXtO7EzR).
    #[serde(rename = "path")]
    pub relative: String,
    /// The character offset of its name in the file, for `navigate.place.goTo`.
    pub offset: usize,
    /// The line of its name, one-based.
    pub line: u32,
    /// The column of its name, one-based, in characters.
    pub column: u32,
    /// Whether the file is in the project (not in the SDK or a library).
    pub in_project: bool,
    /// The matched characters of `qualifiedName`, as `[start, end)` character indexes.
    pub ranges: Vec<[usize; 2]>,
}

/// What `navigate.symbol.goTo` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SymbolResult {
    /// The symbols found, best kind of match first, then the project's, then the closer matches, then the shorter names.
    pub symbols: Vec<SymbolItem>,
    /// With `compact`: the symbols as rows, `path:line:column kind qualifiedName`, and `symbols` empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<String>,
    /// Whether a server is still starting or searching: call again for more.
    pub pending: bool,
    /// Why more may come (`scanning`, `starting`, `indexing`, `searching`), or why nothing came
    /// (`noMatch`, `noServer`, `serverMissing`, `serverNotRunning`); absent when symbols came and
    /// the search is done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// The work a server reports under way while `reason` is `indexing`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<WorkProgress>,
    /// With `reason` `serverMissing`, `serverNotRunning` or `starting`: the server the project
    /// needs, how to install it and what it means.
    #[serde(flatten)]
    pub absent: Option<AbsentServer>,
}

pub struct GoToSymbol;

impl Command for GoToSymbol {
    type Params = SymbolParams;
    type Output = SymbolResult;
    const ID: &'static str = "navigate.symbol.goTo";
    const TITLE: &'static str = "Go to Symbol";
    const DESCRIPTION: &'static str = "Finds the project's declarations (functions, methods, fields, constants, variables and \
         types) by name with the language servers' workspace symbols, matched by the shared name matcher \
         (navigate.names.match): camel humps (`iSt`), `*`, and dotted names (`Store.get`, `model.Store.get`); best \
         matches first, with their matched ranges (`compact`: one row of text each, `path:line:column kind name`); starts the servers when needed. Answers with what has come and \
         `pending` while a server works; `waitMs` waits for it, up to 120 s. Open one with navigate.place.goTo and its path and offset.";

    fn run(&self, session: &mut Session, params: SymbolParams) -> Result<SymbolResult, CommandError> {
        let typed = params.query.trim_start().to_owned();
        let wait = Duration::from_millis(params.wait_ms.unwrap_or(0));
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
        let types = params.types.unwrap_or(true);
        Ok(with_languages(session, |languages, workbench| {
            if server_query(&typed).is_empty() {
                return SymbolResult {
                    symbols: Vec::new(),
                    rows: Vec::new(),
                    pending: false,
                    reason: Some(Reason::NoMatch),
                    progress: None,
                    absent: None,
                };
            }
            let searched = languages.search_waiting(workbench, &typed, wait, params.wait_ms.is_some());
            let keep = |kind: u64| types || !TYPE_KINDS.contains(&kind);
            let symbols = languages
                .matched_symbols(workbench, &typed, keep, limit)
                .into_iter()
                .map(|found| SymbolItem {
                    name: found.name,
                    qualified_name: found.qualified,
                    kind: symbol_kind(found.kind),
                    container: found.container,
                    path: found.path,
                    relative: found.relative,
                    offset: found.offset,
                    line: found.line,
                    column: found.column,
                    in_project: found.in_project,
                    ranges: crate::navigate::spans(&found.ranges),
                })
                .collect::<Vec<_>>();
            let reason = searched.reason_if_empty(symbols.is_empty());
            let (pending, reason, absent) = languages.explain_no_server(workbench, searched.pending, reason);
            let (symbols, rows) = if params.compact == Some(true) {
                let rows = symbols
                    .iter()
                    .map(|symbol| {
                        let mut what = format!("{} {}", super::compact::name_of(&symbol.kind), symbol.qualified_name);
                        if let Some(container) = symbol.container.as_deref().filter(|container| {
                            !container.is_empty()
                                && !container.contains('/')
                                && !symbol.qualified_name.starts_with(&format!("{container}."))
                        }) {
                            what.push_str(" in ");
                            what.push_str(container);
                        }
                        super::compact::row(&symbol.relative, symbol.line, symbol.column, &what)
                    })
                    .collect();
                (Vec::new(), rows)
            } else {
                (symbols, Vec::new())
            };
            SymbolResult { symbols, rows, pending, reason, progress: searched.progress, absent }
        }))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GoToSymbol)
}

#[cfg(test)]
mod tests {
    use zen_lsp::fake::Reply;

    use super::super::testing;
    use super::*;

    const MODEL: &str = "package model\n\ntype ItemStore struct {\n\tcount int\n}\n\nfunc (s *ItemStore) GetItem() {}\n\nconst MaxItems = 10\n\nfunc NewItemStore() *ItemStore { return nil }\n";

    fn symbol(name: &str, kind: u64, container: &str, uri: &str, line: u32, character: u32) -> Value {
        json!({
            "name": name,
            "kind": kind,
            "containerName": container,
            "location": { "uri": uri, "range": {
                "start": { "line": line, "character": character },
                "end": { "line": line, "character": character + 4 } } }
        })
    }

    fn names(found: &Value, field: &str) -> Vec<String> {
        found["symbols"].as_array().unwrap().iter().map(|s| s[field].as_str().unwrap().to_owned()).collect()
    }

    fn fixture() -> testing::Fixture {
        let fixture = testing::fixture(&[("main.go", "package main\n"), ("model/model.go", MODEL)]);
        fixture.server.set_capabilities(json!({ "workspaceSymbolProvider": true }));
        let all = every_symbol(&fixture);
        fixture.server.on_with("workspace/symbol", move |_| all.clone());
        fixture
    }

    fn every_symbol(fixture: &testing::Fixture) -> Value {
        let uri = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
        let package = "example.com/shop/model";
        json!([
            symbol("ItemStore", 23, package, &uri, 2, 5),
            symbol("ItemStore.count", 8, package, &uri, 3, 1),
            symbol("model.ItemStore.GetItem", 6, package, &uri, 6, 20),
            symbol("MaxItems", 14, package, &uri, 8, 6),
            symbol("NewItemStore", 12, package, &uri, 10, 5),
            symbol("Handler", 11, "net/http", "file:///usr/local/go/src/net/http/server.go", 80, 5),
        ])
    }

    #[test]
    fn a_rust_method_is_named_with_its_type_as_rust_analyzer_gives_it() {
        assert_eq!(
            qualified_name("route_call", Some("IdeApp"), 12),
            "IdeApp.route_call",
            "rust-analyzer says function"
        );
        assert_eq!(qualified_name("route_call", Some("IdeApp"), 6), "IdeApp.route_call");
        assert_eq!(qualified_name("fmt", Some("impl Display for Report<'_>"), 6), "Report.fmt");
        assert_eq!(qualified_name("new", Some("zen_core::Registry"), 6), "Registry.new");
        assert_eq!(qualified_name("MAX", Some("Limits"), 14), "Limits.MAX");
        assert_eq!(qualified_name("route_call", Some("app"), 12), "route_call", "a module's function keeps its name");
        assert_eq!(qualified_name("helper", Some("app"), 6), "helper", "a lower-case container is no type");
        assert_eq!(
            qualified_name("ItemStore.GetItem", Some("example.com/shop/model"), 6),
            "ItemStore.GetItem",
            "Go's methods keep the type gopls gives them"
        );
    }

    #[test]
    fn symbols_of_every_kind_are_found_with_their_type() {
        let mut fixture = fixture();
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "item", "waitMs": 5000 })).unwrap();
        assert_eq!(found["pending"], false, "{found}");
        assert_eq!(names(&found, "qualifiedName"), ["ItemStore", "ItemStore.GetItem", "MaxItems", "NewItemStore"]);
        let method = &found["symbols"][1];
        assert_eq!(method["name"], "GetItem");
        assert_eq!(method["kind"], "method");
        assert_eq!(method["container"], "example.com/shop/model");
        assert_eq!((method["line"].as_u64(), method["column"].as_u64()), (Some(7), Some(21)));
        assert_eq!(method["path"], "model/model.go", "relative to the project (card WXtO7EzR)");
        let offset = method["offset"].as_u64().unwrap() as usize;
        assert!(MODEL.chars().skip(offset).collect::<String>().starts_with("GetItem"));
        let without_types =
            fixture.dispatch("navigate.symbol.goTo", json!({ "query": "item", "types": false })).unwrap();
        assert_eq!(names(&without_types, "name"), ["GetItem", "MaxItems", "NewItemStore"]);
        assert_eq!(fixture.server.requests("workspace/symbol").len(), 1, "one request for the query");
        let sdk = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "Handler", "waitMs": 5000 })).unwrap();
        assert_eq!(sdk["symbols"][0]["path"], "/usr/local/go/src/net/http/server.go", "outside the project: {sdk}");
    }

    #[test]
    fn camel_humps_and_dotted_names_match() {
        let mut fixture = fixture();
        let find = |fixture: &mut testing::Fixture, query: &str| {
            let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": query, "waitMs": 5000 })).unwrap();
            names(&found, "qualifiedName")
        };
        assert_eq!(find(&mut fixture, "NIS"), ["NewItemStore"]);
        assert_eq!(find(&mut fixture, "nItSt"), ["NewItemStore"]);
        assert_eq!(find(&mut fixture, "ItemStore.gi"), ["ItemStore.GetItem"]);
        assert_eq!(find(&mut fixture, "model.IS.count"), ["ItemStore.count"]);
        assert_eq!(find(&mut fixture, "shop.ItemStore"), ["ItemStore", "NewItemStore"]);
        assert_eq!(find(&mut fixture, "http.Handler"), ["Handler"]);
        assert!(find(&mut fixture, "zebra").is_empty());
    }

    #[test]
    fn a_new_query_waits_for_the_typing_to_stop_and_cancels_the_last() {
        let mut fixture = fixture();
        fixture.dispatch("navigate.symbol.goTo", json!({ "query": "warm", "waitMs": 5000 })).unwrap();
        let first = fixture.server.requests("workspace/symbol").len();
        let typing = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "ite" })).unwrap();
        assert_eq!(typing["pending"], true);
        assert_eq!(names(&typing, "qualifiedName")[0], "ItemStore", "the last query's symbols, matched again");
        assert_eq!(fixture.server.requests("workspace/symbol").len(), first, "not sent while typing");
        std::thread::sleep(DEBOUNCE);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "ite" })).unwrap();
            if found["pending"] == false {
                assert_eq!(names(&found, "qualifiedName")[0], "ItemStore");
                break;
            }
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fixture.server.requests("workspace/symbol").len(), first + 1);
        let classes = fixture.dispatch("navigate.class.goTo", json!({ "query": "ite" })).unwrap();
        assert_eq!(classes["classes"][0]["name"], "ItemStore");
        assert_eq!(fixture.server.requests("workspace/symbol").len(), first + 1, "Go to Class shares the search");
    }

    fn queries(fixture: &testing::Fixture) -> Vec<String> {
        fixture
            .server
            .requests("workspace/symbol")
            .iter()
            .map(|params| params["query"].as_str().unwrap().to_owned())
            .collect()
    }

    fn by_query(fixture: &testing::Fixture, dotted: Value) -> Value {
        let uri = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
        json!({ "dotted": dotted, "plain": [
            symbol("ItemStore", 23, "example.com/shop/model", &uri, 2, 5),
            symbol("ItemStore.GetItem", 6, "example.com/shop/model", &uri, 6, 20),
        ] })
    }

    #[test]
    fn a_dotted_query_also_asks_for_its_last_part() {
        let mut fixture = fixture();
        let found =
            fixture.dispatch("navigate.symbol.goTo", json!({ "query": "ItemStore.gi", "waitMs": 5000 })).unwrap();
        assert_eq!(names(&found, "qualifiedName"), ["ItemStore.GetItem"], "{found}");
        let mut asked = queries(&fixture);
        asked.sort();
        assert_eq!(asked, ["ItemStore.gi", "gi"]);
    }

    #[test]
    fn gopls_gets_a_dotted_query_as_typed_and_its_last_part_when_it_finds_nothing() {
        let mut fixture = fixture();
        fixture.server.set_name("gopls");
        let answers = by_query(&fixture, json!([]));
        fixture.server.on_with("workspace/symbol", move |params| {
            let dotted = params["query"].as_str().unwrap().contains('.');
            answers[if dotted { "dotted" } else { "plain" }].clone()
        });
        let found =
            fixture.dispatch("navigate.symbol.goTo", json!({ "query": "ItemStore.gi", "waitMs": 5000 })).unwrap();
        assert_eq!(names(&found, "qualifiedName"), ["ItemStore.GetItem"], "{found}");
        assert_eq!(queries(&fixture), ["ItemStore.gi", "gi"]);
        let uri = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
        let answers =
            by_query(&fixture, json!([symbol("model.ItemStore.GetItem", 6, "example.com/shop/model", &uri, 6, 20)]));
        fixture.server.on_with("workspace/symbol", move |params| {
            let dotted = params["query"].as_str().unwrap().contains('.');
            answers[if dotted { "dotted" } else { "plain" }].clone()
        });
        let found = fixture
            .dispatch("navigate.symbol.goTo", json!({ "query": "model.ItemStore.Get", "waitMs": 5000 }))
            .unwrap();
        assert_eq!(names(&found, "qualifiedName"), ["ItemStore.GetItem"], "{found}");
        assert_eq!(queries(&fixture)[2..], ["model.ItemStore.Get"], "found as typed: no second request");
    }

    #[test]
    fn a_superseded_request_is_cancelled_and_its_late_answer_dropped() {
        let mut fixture = fixture();
        let uri = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
        let late = json!([symbol("ItemZebra", 23, "example.com/shop/model", &uri, 2, 5)]);
        fixture.server.reply("workspace/symbol", Reply::Delayed(late, Duration::from_millis(300)));
        let first = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "itemz", "waitMs": 1 })).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while queries(&fixture).is_empty() {
            assert!(Instant::now() < deadline, "the first query was not sent: {first}");
            fixture.dispatch("navigate.symbol.goTo", json!({ "query": "itemz", "waitMs": 20 })).unwrap();
        }
        let uri_all = uri.clone();
        fixture.server.on_with("workspace/symbol", move |_| {
            json!([symbol("ItemStore", 23, "example.com/shop/model", &uri_all, 2, 5)])
        });
        let started = Instant::now();
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "item", "waitMs": 5000 })).unwrap();
        if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
            assert!(started.elapsed() < Duration::from_millis(250), "waitMs is not slowed by the debounce");
        }
        assert_eq!(names(&found, "qualifiedName"), ["ItemStore"]);
        let cancelled: Vec<Value> = fixture.server.requests("$/cancelRequest");
        assert_eq!(cancelled.len(), 1, "the first query's request is cancelled");
        std::thread::sleep(Duration::from_millis(400));
        let again = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "item" })).unwrap();
        assert_eq!(names(&again, "qualifiedName"), ["ItemStore"], "the late answer is dropped");
        assert_eq!(again["pending"], false);
    }

    #[test]
    fn exact_names_come_first_then_prefixes_humps_and_parts_then_the_project_then_shorter() {
        let root = Path::new("/project");
        let found = |name: &str, path: &str| Found {
            name: name.to_owned(),
            kind: 12,
            container: None,
            path: PathBuf::from(path),
            position: lsp_types::Position { line: 0, character: 0 },
        };
        let symbols = [
            found("NewItemStore", "/project/a.go"),
            found("StoreItem", "/project/a.go"),
            found("Item", "/sdk/item.go"),
            found("ItemStoreLong", "/project/a.go"),
            found("Item", "/project/b.go"),
            found("ItemStore", "/project/a.go"),
            found("Item", "/project/b.go"),
            found("ItemX", "/sdk/x.go"),
            found("IP_RECVIF", "/sdk/syscall.go"),
            found("Zebra", "/project/a.go"),
        ];
        let ranked: Vec<(&str, &str)> = rank("item", &symbols, Some(root), &mut RealDirs::new())
            .iter()
            .map(|entry| (symbols[entry.index].name.as_str(), symbols[entry.index].path.to_str().unwrap()))
            .collect();
        assert_eq!(
            ranked,
            [
                ("Item", "/project/b.go"),
                ("Item", "/sdk/item.go"),
                ("ItemStore", "/project/a.go"),
                ("ItemStoreLong", "/project/a.go"),
                ("ItemX", "/sdk/x.go"),
                ("StoreItem", "/project/a.go"),
                ("NewItemStore", "/project/a.go"),
            ]
        );
        let humps: Vec<&str> = rank("iS", &symbols, Some(root), &mut RealDirs::new())
            .iter()
            .map(|entry| symbols[entry.index].name.as_str())
            .collect();
        assert_eq!(humps, ["ItemStore", "ItemStoreLong", "NewItemStore"]);
        let star: Vec<&str> = rank("i*re", &symbols, Some(root), &mut RealDirs::new())
            .iter()
            .map(|entry| symbols[entry.index].name.as_str())
            .collect();
        assert_eq!(star, ["ItemStore", "ItemStoreLong", "NewItemStore", "IP_RECVIF"], "humps: the project's first");
    }

    #[cfg(unix)]
    #[test]
    fn a_path_through_a_link_to_the_project_is_in_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap().join("project");
        std::fs::create_dir_all(real.join("pkg")).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let elsewhere = dir.path().join("sdk");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let mut real_dirs = RealDirs::new();
        let mut project = ProjectRoot::new(Some(&real), &mut real_dirs);
        assert!(project.contains(&link.join("pkg/a.go")), "a server's path through the link");
        assert!(project.contains(&real.join("pkg/a.go")));
        assert!(!project.contains(&elsewhere.join("b.go")));
        let mut real_dirs = RealDirs::new();
        let mut project = ProjectRoot::new(Some(&link), &mut real_dirs);
        assert!(project.contains(&real.join("pkg/a.go")), "a project opened through the link");
        assert!(!ProjectRoot::new(None, &mut RealDirs::new()).contains(&real.join("pkg/a.go")));
        assert!(!ProjectRoot::new(Some(&real), &mut RealDirs::new()).contains(&dir.path().join("gone/c.go")));
    }

    #[test]
    fn five_thousand_symbols_are_ranked_once_per_query() {
        let mut fixture = fixture();
        let uri = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
        let many: Vec<Value> = (0..MAX_FOUND)
            .map(|index| symbol(&format!("Item{index}StoreHandler"), 12, "example.com/shop/model", &uri, 2, 5))
            .collect();
        let many = Value::Array(many);
        fixture.server.on_with("workspace/symbol", move |_| many.clone());
        let started = Instant::now();
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "iSH", "waitMs": 5000 })).unwrap();
        assert_eq!(found["symbols"].as_array().unwrap().len(), DEFAULT_LIMIT);
        let first = started.elapsed();
        let started = Instant::now();
        for _ in 0..20 {
            fixture.dispatch("navigate.symbol.goTo", json!({ "query": "iSH" })).unwrap();
        }
        if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
            assert!(started.elapsed() < first.max(Duration::from_millis(50)), "the frames after reuse the ranking");
        }
    }

    #[test]
    fn gopls_names_lose_their_package_type_parameters_and_receiver() {
        let package = Some("example.com/shop/model");
        assert_eq!(qualified_name("Store.Get", package, 6), "Store.Get");
        assert_eq!(qualified_name("shop/model.Store.Get", package, 6), "Store.Get");
        assert_eq!(qualified_name("(*Store).Get", package, 6), "Store.Get");
        assert_eq!(qualified_name("Store[T].Get", package, 6), "Store.Get");
        assert_eq!(qualified_name("model.(*Store[K, V]).Get", package, 6), "Store.Get");
        assert_eq!(qualified_name("operator()", None, 6), "operator()");
        assert_eq!(qualified_name("[Symbol.iterator]", None, 6), "[Symbol.iterator]");
        assert_eq!(last_part("Store.get"), Some("get"));
        assert_eq!(last_part("Store::get"), Some("get"));
        assert_eq!(last_part("get"), None);
        assert_eq!(last_part("Store."), None);
        assert_eq!(server_query("i*re"), "ire");
        assert_eq!(server_query(" item store "), "itemstore");
        assert_eq!(server_query("*.Store::get:12:3"), "Store.get");
        assert_eq!(server_query("*"), "");
    }

    #[test]
    fn a_star_spaces_and_a_line_go_to_the_server_as_letters_and_the_matcher_filters_the_answer() {
        let mut fixture = fixture();
        fixture.server.set_name("gopls");
        let uri = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
        let package = "example.com/shop/model";
        let all = [
            symbol("ItemStore", 23, package, &uri, 2, 5),
            symbol("ItemStore.count", 8, package, &uri, 3, 1),
            symbol("MaxItems", 14, package, &uri, 8, 6),
            symbol("NewItemStore", 12, package, &uri, 10, 5),
            symbol("Iterator", 11, "iter", "file:///usr/local/go/src/iter/iter.go", 10, 5),
        ];
        fixture.server.on_with("workspace/symbol", move |params| {
            let query = params["query"].as_str().unwrap().to_lowercase();
            let fuzzy = |name: &str| {
                let mut letters = name.chars().flat_map(char::to_lowercase);
                !query.is_empty() && query.chars().all(|wanted| letters.any(|letter| letter == wanted))
            };
            Value::Array(all.iter().filter(|item| fuzzy(item["name"].as_str().unwrap())).cloned().collect())
        });
        let find = |fixture: &mut testing::Fixture, command: &str, query: &str| {
            let found = fixture.dispatch(command, json!({ "query": query, "waitMs": 5000 })).unwrap();
            let list = if command == "navigate.class.goTo" { &found["classes"] } else { &found["symbols"] };
            let field = if command == "navigate.class.goTo" { "name" } else { "qualifiedName" };
            list.as_array().unwrap().iter().map(|item| item[field].as_str().unwrap().to_owned()).collect::<Vec<_>>()
        };
        assert_eq!(find(&mut fixture, "navigate.class.goTo", "i*re"), ["ItemStore"]);
        assert_eq!(find(&mut fixture, "navigate.symbol.goTo", "i*re"), ["ItemStore", "NewItemStore"]);
        assert_eq!(
            find(&mut fixture, "navigate.symbol.goTo", "ire"),
            Vec::<String>::new(),
            "the same answer, matched again"
        );
        assert_eq!(find(&mut fixture, "navigate.symbol.goTo", "i*re"), ["ItemStore", "NewItemStore"]);
        assert_eq!(find(&mut fixture, "navigate.symbol.goTo", "*store"), ["ItemStore", "NewItemStore"]);
        assert_eq!(find(&mut fixture, "navigate.symbol.goTo", "item store"), ["ItemStore", "NewItemStore"]);
        assert_eq!(
            find(&mut fixture, "navigate.symbol.goTo", "items "),
            ["MaxItems"],
            "a trailing space: the name ends"
        );
        assert_eq!(find(&mut fixture, "navigate.symbol.goTo", "ItemStore:12"), ["ItemStore", "NewItemStore"]);
        assert_eq!(find(&mut fixture, "navigate.class.goTo", "ite*or"), ["ItemStore", "Iterator"]);
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "i*re" })).unwrap();
        assert_eq!(found["symbols"][0]["ranges"], json!([[0, 1], [7, 9]]), "the letters the pattern matched");
        assert_eq!(queries(&fixture), ["ire", "store", "itemstore", "items", "ItemStore", "iteor"]);
    }

    #[test]
    fn an_answer_given_while_the_server_loads_is_pending_and_asked_again_after() {
        let mut fixture = fixture();
        let begin =
            json!({ "token": "load", "value": { "kind": "begin", "title": "Loading packages", "percentage": 10 } });
        fixture.server.publish_on("initialized", "$/progress", move |_| begin.clone());
        fixture.server.publish_on(
            "textDocument/didOpen",
            "$/progress",
            |_| json!({ "token": "load", "value": { "kind": "end" } }),
        );
        let uri = zen_lsp::position::file_uri(&fixture.path("model/model.go"));
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&asked);
        fixture.server.on_with("workspace/symbol", move |_| {
            if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                json!([])
            } else {
                json!([symbol("ItemStore", 23, "example.com/shop/model", &uri, 2, 5)])
            }
        });
        let loading = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "item", "waitMs": 400 })).unwrap();
        assert_eq!(loading["symbols"], json!([]), "{loading}");
        assert_eq!((&loading["pending"], &loading["reason"]), (&json!(true), &json!("indexing")), "{loading}");
        assert_eq!(loading["progress"], json!({ "server": "go", "title": "Loading packages", "percentage": 10 }));
        fixture.open("main.go");
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "item", "waitMs": 5000 })).unwrap();
        assert_eq!(names(&found, "qualifiedName"), ["ItemStore"], "{found}");
        assert_eq!(found["pending"], false);
        assert!(found.get("reason").is_none() && found.get("progress").is_none(), "{found}");
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn a_wait_longer_than_ten_seconds_goes_on_while_the_server_loads() {
        let mut fixture = fixture();
        let begin = json!({ "token": "load", "value": { "kind": "begin", "title": "Loading packages" } });
        fixture.server.publish_on("initialized", "$/progress", move |_| begin.clone());
        fixture.server.on("workspace/symbol", json!([]));
        let started = Instant::now();
        let wait = Duration::from_millis(10_600);
        let loading = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "item", "waitMs": 10_600 })).unwrap();
        assert_eq!((&loading["pending"], &loading["reason"]), (&json!(true), &json!("indexing")), "{loading}");
        assert!(started.elapsed() >= wait, "waited past 10 s while loading: {:?}", started.elapsed());
        assert!(started.elapsed() < wait + Duration::from_secs(60), "{:?}", started.elapsed());

        let (second, minute) = (Duration::from_secs(1), Duration::from_secs(60));
        let indexing = Some(Reason::Indexing);
        let searching = Some(Reason::Searching);
        let keeps = |asked: Duration, answers: &[(Duration, bool, Option<Reason>)]| {
            let mut wait = crate::language::SearchWait::new(asked);
            let start = Instant::now();
            answers.iter().map(|(at, pending, reason)| wait.keeps_waiting_at(start + *at, *pending, *reason)).last()
        };
        assert_eq!(keeps(3 * minute, &[(minute, true, indexing)]), Some(true));
        assert_eq!(keeps(3 * minute, &[(crate::language::MAX_LOADING_WAIT, true, indexing)]), Some(false));
        assert_eq!(keeps(minute, &[(11 * second, true, searching)]), Some(false));
        assert_eq!(keeps(minute, &[(9 * second, true, searching)]), Some(true));
        assert_eq!(keeps(minute, &[(second, false, indexing)]), Some(false), "an answer that came is given");
        assert_eq!(keeps(second, &[(2 * second, true, indexing)]), Some(false), "never past the wait asked");
    }

    #[test]
    fn a_query_asked_again_after_loading_gets_its_own_wait() {
        let (second, minute) = (Duration::from_secs(1), Duration::from_secs(60));
        let mut wait = crate::language::SearchWait::new(minute);
        let start = Instant::now();
        assert!(wait.keeps_waiting_at(start + 13 * second, true, Some(Reason::Indexing)));
        let asked_again = start + 13 * second + Duration::from_millis(100);
        assert!(wait.keeps_waiting_at(asked_again, true, Some(Reason::Searching)), "its answer is waited for");
        assert!(wait.keeps_waiting_at(start + 22 * second, true, Some(Reason::Searching)));
        assert!(!wait.keeps_waiting_at(start + 24 * second, true, Some(Reason::Searching)), "10 s after loading");
        let mut explicit = crate::language::SearchWait::explicit(2 * minute);
        assert!(explicit.keeps_waiting_at(start + 13 * second, true, Some(Reason::Indexing)));
        assert!(
            explicit.keeps_waiting_at(start + 40 * second, true, Some(Reason::Searching)),
            "a caller's waitMs is waited for after loading: ADR-0283"
        );
        assert!(!explicit.keeps_waiting_at(start + 40 * second, false, None), "an answer that came is given");
        assert!(!explicit.keeps_waiting_at(start + 121 * second, true, Some(Reason::Searching)), "never past 120 s");
        let mut short = crate::language::SearchWait::new(15 * second);
        assert!(short.keeps_waiting_at(start + 13 * second, true, Some(Reason::Indexing)));
        assert!(!short.keeps_waiting_at(start + 16 * second, true, Some(Reason::Searching)), "never past waitMs");
    }

    #[test]
    fn an_empty_answer_says_why() {
        let mut fixture = fixture();
        let held = std::sync::Arc::new((std::sync::Mutex::new(true), std::sync::Condvar::new()));
        let gate = held.clone();
        let all = every_symbol(&fixture);
        fixture.server.on_with("workspace/symbol", move |_| {
            let (holding, released) = &*gate;
            let guard = holding.lock().unwrap();
            let _guard = released.wait_timeout_while(guard, Duration::from_secs(30), |holding| *holding).unwrap();
            all.clone()
        });
        let first = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "zebra", "waitMs": 1 })).unwrap();
        assert_eq!(first["pending"], true, "{first}");
        assert!(["scanning", "starting", "searching"].contains(&first["reason"].as_str().unwrap()), "{first}");
        *held.0.lock().unwrap() = false;
        held.1.notify_all();
        let none = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "zebra", "waitMs": 5000 })).unwrap();
        assert_eq!((&none["pending"], &none["reason"]), (&json!(false), &json!("noMatch")), "{none}");
        let classes = fixture.dispatch("navigate.class.goTo", json!({ "query": "zebra", "waitMs": 5000 })).unwrap();
        assert_eq!(classes["reason"], "noMatch", "{classes}");
        let found = fixture.dispatch("navigate.symbol.goTo", json!({ "query": "item", "waitMs": 5000 })).unwrap();
        assert!(found.get("reason").is_none(), "{found}");
    }

    fn name_rank(query: &str, name: &str) -> Option<zen_index::Rank> {
        let found = zen_index::NamePattern::new(query).matches(name)?;
        Some(zen_index::Rank::new(&found, name.chars().count()))
    }

    #[test]
    fn names_match_by_humps() {
        assert!(name_rank("IS", "ItemStore").is_some());
        assert!(name_rank("IS", "Items").is_none(), "a capital starts a word");
        assert!(name_rank("iSt", "ItemStore").is_some());
        assert!(name_rank("httpS", "HTTPServer").is_some());
        assert!(name_rank("store", "ItemStore").is_some(), "a word in the middle");
        assert!(name_rank("tem", "ItemStore").is_some(), "a part of the name");
        assert!(name_rank("xyz", "ItemStore").is_none());
        assert!(name_rank("item", "Item") < name_rank("item", "ItemStore"), "the exact name first");
        assert!(name_rank("item", "ItemStore") < name_rank("IS", "ItemStore"), "then a prefix");
        assert_eq!(qualified_name("example.com/shop/model.Store.Get", Some("example.com/shop/model"), 6), "Store.Get");
        assert_eq!(qualified_name("model.Store", Some("example.com/shop/model"), 5), "Store");
        assert_eq!(qualified_name("get", Some("Store"), 6), "Store.get", "a method of a class in any language");
        assert_eq!(qualified_name("get", Some("store"), 12), "get", "a module is no type");
    }
}
