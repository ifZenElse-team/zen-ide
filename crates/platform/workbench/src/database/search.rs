use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Session};
use zen_db::history::{Entry as HistoryEntry, Journal, Query};
use zen_db::model::NodeKind;
use zen_db::registry::Entry;
use zen_db::schema::Snapshot;
use zen_i18n::t;
use zen_index::{MatchKind, NameMatch, NamePattern};
use zen_modules::search::{
    CancelToken, SearchContributor, SearchItem, SearchProgress, SearchQuery, SearchSink, SearchTarget,
};

use super::WORKSPACE;
use super::objects::kind_name;
use super::scripts::{ListParams as ScriptsParams, ListScripts, ScriptEntry};
use super::sources::files;
use super::structure::{self, Structure};

pub const CONTRIBUTOR: &str = "database";

const LISTED: usize = 50;

const MOST_LISTED: usize = 500;

const CHECK_EVERY: usize = 4096;

const NAME_SHOWN: usize = 100;

#[derive(Clone, Debug)]
struct Owner {
    connection: u32,
    name: String,
    detail: String,
    path: Vec<String>,
}

#[derive(Clone, Debug)]
struct Name {
    text: Box<str>,
    kind: Cow<'static, str>,
    owner: u32,
}

#[derive(Debug, Default)]
pub struct NameIndex {
    connections: Vec<(String, String)>,
    owners: Vec<Owner>,
    names: Vec<Name>,
}

impl NameIndex {
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn build<'a>(snapshots: impl IntoIterator<Item = (&'a str, &'a str, &'a Snapshot)>) -> Self {
        let mut index = Self::default();
        for (id, name, snapshot) in snapshots {
            let connection = u32::try_from(index.connections.len()).unwrap_or(u32::MAX);
            index.connections.push((id.to_owned(), name.to_owned()));
            index.add_relational(connection, snapshot);
            index.add_own_tree(connection, snapshot);
        }
        index
    }

    fn owner(&mut self, owner: Owner) -> u32 {
        self.owners.push(owner);
        u32::try_from(self.owners.len() - 1).unwrap_or(u32::MAX)
    }

    fn add_relational(&mut self, connection: u32, snapshot: &Snapshot) {
        let several =
            snapshot.catalogs.iter().filter(|catalog| !catalog.system && !catalog.schemas.is_empty()).count() > 1;
        for catalog in snapshot.catalogs.iter().filter(|catalog| !catalog.system) {
            for schema in catalog.schemas.iter().filter(|schema| !schema.system) {
                let within = if several { format!("{}.{}", catalog.name, schema.name) } else { schema.name.clone() };
                let detail = if several { catalog.name.clone() } else { String::new() };
                let path = vec![catalog.name.clone(), schema.name.clone()];
                let owner = self.owner(Owner { connection, name: catalog.name.clone(), detail, path });
                self.names.push(Name { text: schema.name.as_str().into(), kind: Cow::Borrowed("schema"), owner });
                for object in schema.objects.iter().filter(|object| !object.system) {
                    let path = vec![
                        catalog.name.clone(),
                        schema.name.clone(),
                        object.kind.folder().to_owned(),
                        object.name.clone(),
                    ];
                    let owner = self.owner(Owner {
                        connection,
                        name: schema.name.clone(),
                        detail: within.clone(),
                        path: path.clone(),
                    });
                    let kind = Cow::Borrowed(kind_name(object.kind));
                    self.names.push(Name { text: object.name.as_str().into(), kind, owner });
                    if object.columns.is_empty() {
                        continue;
                    }
                    let detail = format!("{within}.{}", object.name);
                    let owner = self.owner(Owner { connection, name: object.name.clone(), detail, path });
                    for column in &object.columns {
                        self.names.push(Name {
                            text: column.name.as_str().into(),
                            kind: Cow::Borrowed("column"),
                            owner,
                        });
                    }
                }
            }
        }
    }

    fn add_own_tree(&mut self, connection: u32, snapshot: &Snapshot) {
        for children in &snapshot.nodes {
            let parent = children.path.last().cloned().unwrap_or_default();
            let detail = children.path.join("/");
            for node in children.nodes.iter().filter(|node| !node.system) {
                let kind: Cow<'static, str> = match &node.kind {
                    NodeKind::Table => Cow::Borrowed("table"),
                    NodeKind::View => Cow::Borrowed("view"),
                    NodeKind::Key => Cow::Borrowed("key"),
                    NodeKind::Collection => Cow::Borrowed("collection"),
                    NodeKind::SearchIndex => Cow::Borrowed("searchIndex"),
                    NodeKind::Other(kind) => Cow::Owned(kind.clone()),
                    NodeKind::Catalog
                    | NodeKind::Schema
                    | NodeKind::Folder
                    | NodeKind::Column
                    | NodeKind::Index
                    | NodeKind::KeyPrefix => continue,
                };
                let path = node.path.clone();
                let owner = self.owner(Owner { connection, name: parent.clone(), detail: detail.clone(), path });
                self.names.push(Name { text: node.label.as_str().into(), kind, owner });
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Hit {
    name: u32,
    matched: NameMatch,
    outer: Option<(MatchKind, i64)>,
}

pub fn match_names(
    index: &NameIndex,
    query: &str,
    limit: usize,
    keep: &dyn Fn(&str, &str) -> bool,
    cancelled: &dyn Fn() -> bool,
) -> Option<(Vec<Hit>, usize)> {
    let query = query.trim();
    let (qualifier, pattern) = match query.rsplit_once('.') {
        Some((qualifier, name)) if !qualifier.trim().is_empty() => {
            (Some(NamePattern::new(qualifier.trim())), NamePattern::new(name))
        }
        _ => (None, NamePattern::new(query)),
    };
    if pattern.is_empty() && qualifier.is_none() {
        return Some((Vec::new(), 0));
    }
    let mut owners: Vec<Option<Option<(MatchKind, i64)>>> =
        vec![None; if qualifier.is_some() { index.owners.len() } else { 0 }];
    let mut hits = Vec::new();
    for (at, name) in index.names.iter().enumerate() {
        if at % CHECK_EVERY == 0 && cancelled() {
            return None;
        }
        let owner = &index.owners[name.owner as usize];
        let mut outer = None;
        if let Some(qualifier) = &qualifier {
            outer = *owners[name.owner as usize].get_or_insert_with(|| {
                qualifier
                    .matches(&owner.name)
                    .filter(|matched| matched.kind > MatchKind::Middle)
                    .map(|matched| (matched.kind, matched.degree))
            });
            if outer.is_none() {
                continue;
            }
        }
        let Some(matched) = pattern.matches(&name.text) else { continue };
        if !keep(&index.connections[owner.connection as usize].0, &name.kind) {
            continue;
        }
        hits.push(Hit { name: u32::try_from(at).unwrap_or(u32::MAX), matched, outer });
    }
    let total = hits.len();
    let order = |a: &Hit, b: &Hit| {
        let (left, right) = (&index.names[a.name as usize], &index.names[b.name as usize]);
        b.matched
            .kind
            .cmp(&a.matched.kind)
            .then((left.kind == "column").cmp(&(right.kind == "column")))
            .then(b.matched.degree.cmp(&a.matched.degree))
            .then(b.outer.cmp(&a.outer))
            .then(left.text.len().cmp(&right.text.len()))
            .then(left.text.cmp(&right.text))
            .then(a.name.cmp(&b.name))
    };
    if hits.len() > limit && limit > 0 {
        hits.select_nth_unstable_by(limit - 1, order);
        hits.truncate(limit);
    }
    hits.truncate(limit);
    hits.sort_by(order);
    Some((hits, total))
}

/// What a search of the Database tab or `db.object.goTo` finds.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GoToParams {
    /// A name, camel humps or `*` wildcards as Go to Class takes them; `schema.name` or `table.column` narrows by what holds it.
    pub query: String,
    /// The most items to answer; 50 when absent, at most 500.
    pub limit: Option<usize>,
    /// Only names of this connection.
    pub connection: Option<String>,
    /// Only these kinds: `table`, `view`, `materializedView`, `foreignTable`, `function`, `procedure`, `sequence`, `type`, `event`, `schema`, `column`, a driver's own (`key`, `collection`), `script`, `console`, `history`.
    pub kinds: Vec<String>,
}

/// A database object, script or statement of the history that matches.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FoundItem {
    /// Its name: an object's, a column's, a file's, or a statement's first line.
    pub name: String,
    /// What it is: an object's kind, `schema`, `column`, `script`, `console` or `history`.
    pub kind: String,
    /// The connection's id.
    pub connection: String,
    /// The connection's name.
    pub connection_name: String,
    /// Where it is: the schema (`catalog.schema` when the connection has several databases), `schema.table` for a column, a script's place.
    pub detail: String,
    /// The object's path for db.object.open (a column's is its table's).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    /// A script's or console's file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// A statement's id in the query history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// How the name matched: `exact`, `prefix`, `humps` or `middle`; absent for the history, matched by words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_kind: Option<String>,
    /// The characters of `name` matched, as `[start, end)` character indexes.
    pub ranges: Vec<[usize; 2]>,
    /// The command that opens it.
    pub command: String,
    /// That command's parameters.
    pub params: Value,
}

/// What `db.object.goTo` found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoToAnswer {
    /// The items: objects and columns, then scripts, then statements of the history, at most `limit`.
    pub items: Vec<FoundItem>,
    /// How many match in all.
    pub total: usize,
    /// Whether items were left out because of `limit`.
    pub truncated: bool,
    /// Whether a connection's snapshot is still being read from disk, so more may match soon.
    pub loading: bool,
    /// How many names were searched.
    pub names: usize,
}

struct Prepared {
    index: Arc<NameIndex>,
    revision: u64,
    scripts: Vec<(ScriptEntry, String)>,
    journal: Option<Arc<Journal>>,
    loading: bool,
}

#[derive(Default)]
pub struct DatabaseNames {
    revision: Option<u64>,
    ids: Vec<(String, String)>,
    index: Arc<NameIndex>,
}

fn prepare(session: &mut Session, entries: &[Entry]) -> Prepared {
    for entry in entries {
        structure::ensure(session, entry, false);
    }
    let loading = entries
        .iter()
        .any(|entry| session.state::<Structure>().is_some_and(|structure| structure.loading(&entry.source.id)));
    let revision = session.state::<Structure>().map_or(0, Structure::revision);
    let ids: Vec<(String, String)> =
        entries.iter().map(|entry| (entry.source.id.clone(), entry.source.name.clone())).collect();
    let index = session
        .state::<DatabaseNames>()
        .filter(|cached| cached.revision == Some(revision) && cached.ids == ids)
        .map(|cached| Arc::clone(&cached.index));
    let index = index.unwrap_or_else(|| {
        let structure = session.state::<Structure>();
        let snapshots = ids.iter().filter_map(|(id, name)| {
            structure
                .and_then(|structure| structure.snapshot(id))
                .map(|snapshot| (id.as_str(), name.as_str(), snapshot))
        });
        let built = Arc::new(NameIndex::build(snapshots));
        session.set_state(DatabaseNames { revision: Some(revision), ids: ids.clone(), index: Arc::clone(&built) });
        built
    });
    let names: std::collections::HashMap<&str, &str> =
        ids.iter().map(|(id, name)| (id.as_str(), name.as_str())).collect();
    let scripts = ListScripts
        .run(session, ScriptsParams { connection: None, limit: Some(usize::MAX) })
        .map(|listed| listed.scripts)
        .unwrap_or_default()
        .into_iter()
        .filter(|script| script.exists)
        .map(|script| {
            let name = names
                .get(script.connection.as_str())
                .map_or_else(|| script.connection.clone(), |name| (*name).to_owned());
            (script, name)
        })
        .collect();
    Prepared { index, revision, scripts, journal: super::history::journal(session), loading }
}

fn first_line(text: &str) -> (String, bool) {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let several = text.trim().lines().count() > 1;
    match line.char_indices().nth(NAME_SHOWN) {
        Some((end, _)) => (format!("{}…", &line[..end]), true),
        None if several => (format!("{line} …"), true),
        None => (line.to_owned(), false),
    }
}

fn word_ranges(name: &str, query: &str) -> Vec<[usize; 2]> {
    let folded: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    if folded.len() != name.chars().count() {
        return Vec::new();
    }
    let mut ranges: Vec<[usize; 2]> = Vec::new();
    for word in query.split_whitespace() {
        let word: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
        if word.is_empty() || word.len() > folded.len() {
            continue;
        }
        if let Some(start) = (0..=folded.len() - word.len()).find(|&at| folded[at..at + word.len()] == word[..]) {
            ranges.push([start, start + word.len()]);
        }
    }
    ranges.sort_unstable();
    ranges
}

fn ranges_of(matched: &NameMatch) -> Vec<[usize; 2]> {
    matched.ranges.iter().map(|range| [range.start, range.end]).collect()
}

fn wanted(params: &GoToParams, connection: &str, kind: &str) -> bool {
    params.connection.as_deref().is_none_or(|id| id == connection)
        && (params.kinds.is_empty() || params.kinds.iter().any(|wanted| wanted == kind))
}

fn search(prepared: &Prepared, params: &GoToParams, cancelled: &dyn Fn() -> bool) -> Option<GoToAnswer> {
    let limit = params.limit.unwrap_or(LISTED).clamp(1, MOST_LISTED);
    let index = &prepared.index;
    let keep = |connection: &str, kind: &str| wanted(params, connection, kind);
    let (hits, mut total) = match_names(index, &params.query, limit, &keep, cancelled)?;
    let mut items: Vec<FoundItem> = hits
        .into_iter()
        .map(|hit| {
            let name = &index.names[hit.name as usize];
            let owner = &index.owners[name.owner as usize];
            let (connection, connection_name) = index.connections[owner.connection as usize].clone();
            let params = if name.kind == "column" {
                json!({ "connection": connection, "path": owner.path, "page": "properties" })
            } else {
                json!({ "connection": connection, "path": owner.path })
            };
            FoundItem {
                name: name.text.to_string(),
                kind: name.kind.to_string(),
                connection,
                connection_name,
                detail: owner.detail.clone(),
                path: Some(owner.path.clone()),
                file: None,
                id: None,
                match_kind: Some(hit.matched.kind.as_str().to_owned()),
                ranges: ranges_of(&hit.matched),
                command: "db.object.open".to_owned(),
                params,
            }
        })
        .collect();
    let pattern = NamePattern::new(params.query.trim());
    let mut scripts: Vec<(NameMatch, &ScriptEntry, &String)> = prepared
        .scripts
        .iter()
        .filter(|(script, _)| wanted(params, &script.connection, if script.console { "console" } else { "script" }))
        .filter_map(|(script, name)| pattern.matches_file(&script.name).map(|matched| (matched, script, name)))
        .collect();
    scripts.sort_by(|a, b| b.0.kind.cmp(&a.0.kind).then(b.0.degree.cmp(&a.0.degree)).then(a.1.name.cmp(&b.1.name)));
    total += scripts.len();
    for (matched, script, connection_name) in scripts {
        if items.len() >= limit {
            break;
        }
        items.push(FoundItem {
            name: script.name.clone(),
            kind: if script.console { "console" } else { "script" }.to_owned(),
            connection: script.connection.clone(),
            connection_name: connection_name.clone(),
            detail: script.place.clone(),
            path: None,
            file: Some(script.path.clone()),
            id: None,
            match_kind: Some(matched.kind.as_str().to_owned()),
            ranges: ranges_of(&matched),
            command: "workspace.tab.open".to_owned(),
            params: json!({ "workspace": WORKSPACE, "path": script.path }),
        });
    }
    if cancelled() {
        return None;
    }
    let history = params.kinds.is_empty() || params.kinds.iter().any(|kind| kind == "history");
    if let Some(journal) = prepared.journal.as_ref().filter(|_| history) {
        let query = Query {
            text: Some(params.query.trim().to_owned()),
            connections: params.connection.iter().cloned().collect(),
            limit: limit.saturating_sub(items.len()),
            ..Query::default()
        };
        let found = journal.search(&query);
        total += found.total;
        let names: std::collections::HashMap<&str, &str> =
            index.connections.iter().map(|(id, name)| (id.as_str(), name.as_str())).collect();
        items.extend(found.entries.into_iter().map(|entry: HistoryEntry| {
            let (name, _) = first_line(&entry.text);
            FoundItem {
                ranges: word_ranges(&name, &params.query),
                name,
                kind: "history".to_owned(),
                connection_name: names
                    .get(entry.connection.as_str())
                    .map_or_else(|| entry.connection.clone(), |name| (*name).to_owned()),
                detail: entry.schema.clone().unwrap_or_default(),
                connection: entry.connection,
                path: None,
                file: None,
                id: Some(entry.id),
                match_kind: None,
                command: "db.history.openInConsole".to_owned(),
                params: json!({ "id": entry.id }),
            }
        }));
    }
    Some(GoToAnswer { truncated: total > items.len(), items, total, loading: prepared.loading, names: index.len() })
}

pub struct GoToObject;

impl Command for GoToObject {
    type Params = GoToParams;
    type Output = GoToAnswer;
    const ID: &'static str = "db.object.goTo";
    const TITLE: &'static str = "Go to Database Object";
    const DESCRIPTION: &'static str = "Finds database objects by name as Go to Database Object (⌘O in the Database workspace) and the Database tab of Search Everywhere do: tables, views, functions, procedures, sequences, types, schemas and columns of every connection's snapshot (what the navigator has read, also offline; system objects left out; nothing is asked of a server and nothing connects), then the connections' scripts and consoles by file name, then statements of the query history holding the query's words. `query` is matched as Go to Class matches (camel humps, `*`); `schema.name` or `table.column` narrows by what holds it. `connection` and `kinds` narrow it. Answers at most `limit` (50, ≤ 500) items, best first, each with its kind, connection, place, the matched characters and the command with parameters that opens it (db.object.open, a column on its table's Properties; workspace.tab.open; db.history.openInConsole), with `total`, `truncated`, and `loading` while snapshots are read from disk.";

    fn run(&self, session: &mut Session, params: GoToParams) -> Result<GoToAnswer, CommandError> {
        let entries = files(session).entries().unwrap_or_default();
        let prepared = prepare(session, &entries);
        Ok(search(&prepared, &params, &|| false).unwrap_or(GoToAnswer {
            items: Vec::new(),
            total: 0,
            truncated: false,
            loading: prepared.loading,
            names: prepared.index.len(),
        }))
    }
}

fn kind_label(kind: &str) -> String {
    match kind {
        "table" => t!("database-sql-kind-table"),
        "foreignTable" => t!("database-sql-kind-foreign-table"),
        "view" => t!("database-sql-kind-view"),
        "materializedView" => t!("database-sql-kind-materialized-view"),
        "function" => t!("database-sql-kind-function"),
        "procedure" => t!("database-sql-kind-procedure"),
        "sequence" => t!("database-sql-kind-sequence"),
        "type" => t!("database-sql-kind-type"),
        "event" => t!("database-sql-kind-event"),
        "column" => t!("database-sql-kind-column"),
        "schema" => t!("database-search-kind-schema"),
        "key" => t!("database-search-kind-key"),
        "collection" => t!("database-search-kind-collection"),
        "script" => t!("database-search-kind-script"),
        "console" => t!("database-search-kind-console"),
        "history" => t!("database-search-kind-history"),
        other => other.to_owned(),
    }
}

fn item_of(found: FoundItem) -> SearchItem {
    let detail = if found.detail.is_empty() {
        found.connection_name.clone()
    } else {
        t!("database-search-detail", place = found.detail.as_str(), connection = found.connection_name.as_str())
    };
    SearchItem {
        hint: Some(kind_label(&found.kind)),
        detail,
        ranges: found.ranges,
        detail_ranges: Vec::new(),
        in_project: true,
        available: true,
        target: SearchTarget::Run { command: found.command, params: found.params, icon: found.kind },
        name: found.name,
    }
}

struct Running {
    text: String,
    limit: usize,
    revision: u64,
    loading: bool,
    entries: Vec<Entry>,
    cancel: CancelToken,
    answer: Arc<Mutex<Option<GoToAnswer>>>,
}

#[derive(Default)]
pub struct DatabaseContributor {
    running: Mutex<Option<Running>>,
}

impl DatabaseContributor {
    fn start(session: &mut Session, query: &SearchQuery, entries: Vec<Entry>) -> Running {
        let prepared = prepare(session, &entries);
        let cancel = CancelToken::default();
        let answer = Arc::new(Mutex::new(None));
        let (mine, theirs, kept) = (cancel.clone(), query.cancel.clone(), Arc::clone(&answer));
        let params = GoToParams { query: query.text.clone(), limit: Some(query.limit), ..GoToParams::default() };
        let (revision, loading) = (prepared.revision, prepared.loading);
        let spawned = std::thread::Builder::new().name("zen-db-search".to_owned()).spawn(move || {
            let cancelled = || mine.is_cancelled() || theirs.is_cancelled();
            if let Some(found) = search(&prepared, &params, &cancelled) {
                *kept.lock().unwrap_or_else(PoisonError::into_inner) = Some(found);
            }
        });
        if spawned.is_err() {
            cancel.cancel();
        }
        Running { text: query.text.clone(), limit: query.limit, revision, loading, entries, cancel, answer }
    }
}

impl SearchContributor for DatabaseContributor {
    fn id(&self) -> &str {
        CONTRIBUTOR
    }

    fn title(&self) -> &str {
        "database-search-title"
    }

    fn weight(&self) -> i32 {
        450
    }

    fn primary(&self) -> bool {
        false
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress {
        if query.text.trim().is_empty() {
            return SearchProgress::Done;
        }
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        let fresh = running.as_ref().is_none_or(|run| run.text != query.text || run.limit != query.limit);
        if fresh {
            if let Some(old) = running.take() {
                old.cancel.cancel();
            }
            let entries = files(session).entries().unwrap_or_default();
            *running = Some(Self::start(session, query, entries));
        } else if let Some(run) = running.as_mut().filter(|run| run.loading) {
            for entry in &run.entries {
                structure::ensure(session, entry, false);
            }
            let revision = session.state::<Structure>().map_or(0, Structure::revision);
            if revision != run.revision {
                run.cancel.cancel();
                let entries = std::mem::take(&mut run.entries);
                *run = Self::start(session, query, entries);
            }
        }
        let Some(run) = running.as_ref() else { return SearchProgress::Done };
        if query.cancel.is_cancelled() {
            run.cancel.cancel();
            return SearchProgress::Done;
        }
        let answer = run.answer.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(answer) = answer else {
            if run.cancel.is_cancelled() {
                return SearchProgress::Done;
            }
            return SearchProgress::Pending("database-search-searching");
        };
        for found in answer.items {
            if !sink.push(item_of(found)) {
                break;
            }
        }
        if run.loading { SearchProgress::Pending("database-search-reading") } else { SearchProgress::Done }
    }
}

impl Drop for DatabaseContributor {
    fn drop(&mut self) {
        if let Some(run) = self.running.get_mut().unwrap_or_else(PoisonError::into_inner).take() {
            run.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests;
