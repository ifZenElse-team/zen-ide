use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{DbError, ErrorKind};
use zen_db::engine::{ConnectionState, OperationStatus};
use zen_db::model::{DataModel, Node};
use zen_db::registry::{Entry, Scope};
use zen_db::schema::{
    CachedNode, DdlOptions, IntrospectRequest, LARGE_SCHEMA, Level, Object, Snapshot, TreeOptions, find_object,
    qualified_for, quote_for,
};
use zen_i18n::t;

use super::connections::describe;
use super::project_key;
use super::sources::{Database, find};
use crate::Workbench;
use crate::workspaces::Workspaces;

const LISTED: usize = 200;

const DEFINITION_ANSWERED: usize = 8000;

const GENERIC_REFRESHED: usize = 50;

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Saving {
    newest: Option<Snapshot>,
    writing: bool,
}

#[derive(Clone, Default)]
pub(super) struct Saver(Arc<Mutex<Saving>>);

impl Saver {
    fn saving(&self) -> MutexGuard<'_, Saving> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn save(&self, snapshot: Snapshot, file: &Path, start: impl FnOnce(Job) -> bool) {
        let mut saving = self.saving();
        saving.newest = Some(snapshot);
        if saving.writing {
            return;
        }
        saving.writing = true;
        drop(saving);
        let (saver, file) = (self.clone(), file.to_owned());
        if !start(Box::new(move || saver.write(&file))) {
            self.saving().writing = false;
        }
    }

    fn write(&self, file: &Path) {
        while let Some(snapshot) = self.next() {
            let _ = zen_db::schema::write_snapshot(file, &snapshot);
        }
    }

    fn next(&self) -> Option<Snapshot> {
        let mut saving = self.saving();
        let next = saving.newest.take();
        saving.writing = next.is_some();
        next
    }
}

#[derive(Default)]
struct Kept {
    snapshot: Snapshot,
    file: Option<PathBuf>,
    saver: Saver,
    loading: Option<Receiver<Option<Snapshot>>>,
    refresh: Option<(u64, Vec<Vec<String>>)>,
    queued: Option<(IntrospectRequest, Vec<Vec<String>>)>,
    children: HashMap<Vec<String>, u64>,
    errors: HashMap<Vec<String>, String>,
    relational: bool,
    expected: Option<IntrospectRequest>,
}

#[derive(Default)]
pub struct Structure {
    kept: HashMap<String, Kept>,
    states: HashMap<String, ConnectionState>,
    revision: u64,
}

impl Structure {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn snapshot(&self, connection: &str) -> Option<&Snapshot> {
        self.kept.get(connection).map(|kept| &kept.snapshot)
    }

    pub fn error(&self, connection: &str, path: &[String]) -> Option<&str> {
        self.kept.get(connection)?.errors.get(path).map(String::as_str)
    }

    pub fn clear_error(&mut self, connection: &str, path: &[String]) {
        if let Some(kept) = self.kept.get_mut(connection)
            && kept.errors.remove(path).is_some()
        {
            self.revision += 1;
        }
    }

    pub fn loading(&self, connection: &str) -> bool {
        self.kept.get(connection).is_some_and(|kept| kept.loading.is_some())
    }

    pub fn busy(&self, connection: &str) -> bool {
        self.kept.get(connection).is_some_and(|kept| kept.refresh.is_some() || !kept.children.is_empty())
    }

    pub fn expect(&mut self, connection: &str, request: IntrospectRequest) {
        if let Some(kept) = self.kept.get_mut(connection) {
            kept.expected = Some(request);
        }
    }

    pub fn forget_expected(&mut self, connection: &str) -> bool {
        self.kept.get_mut(connection).is_some_and(|kept| kept.expected.take().is_some())
    }
}

fn file_name(id: &str) -> String {
    id.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect()
}

pub fn snapshot_file(session: &Session, entry: &Entry) -> Option<PathBuf> {
    let data = session.state::<Workspaces>()?.data_dir()?;
    let scope = match entry.scope {
        Scope::Global => "global".to_owned(),
        Scope::Project => project_key(session.state::<Workbench>()?.project()?),
    };
    Some(
        data.join("caches")
            .join("databases")
            .join(scope)
            .join(format!("{}.snapshot", file_name(&entry.source.id).trim_start_matches('.'))),
    )
}

fn relational(session: &mut Session, entry: &Entry) -> bool {
    session.state_mut::<Database>().drivers().iter().find(|driver| driver.id() == entry.source.driver).is_some_and(
        |driver| {
            let capabilities = driver.capabilities();
            capabilities.models.contains(&DataModel::Relational) && capabilities.introspection
        },
    )
}

fn wake(session: &Session) -> Option<zen_db::Waker> {
    session.state::<Database>().and_then(Database::waker)
}

pub fn ensure(session: &mut Session, entry: &Entry, wait: bool) {
    let id = entry.source.id.clone();
    if !session.state::<Structure>().is_some_and(|structure| structure.kept.contains_key(&id)) {
        let file = snapshot_file(session, entry);
        let relational = relational(session, entry);
        let waker = wake(session);
        let loading = file.clone().map(|file| {
            let (sender, receiver) = std::sync::mpsc::channel();
            let started = std::thread::Builder::new().name("zen-db-snapshot".to_owned()).spawn(move || {
                let _ = sender.send(zen_db::schema::read_snapshot(&file));
                if let Some(waker) = waker {
                    waker();
                }
            });
            if started.is_err() {
                let (sender, receiver) = std::sync::mpsc::channel();
                let _ = sender.send(None);
                return receiver;
            }
            receiver
        });
        let snapshot = Snapshot::new(&id, &entry.source.driver);
        session
            .state_mut::<Structure>()
            .kept
            .insert(id.clone(), Kept { snapshot, file, loading, relational, ..Kept::default() });
    }
    take_loaded(session, &id, wait);
}

fn take_loaded(session: &mut Session, id: &str, wait: bool) {
    let structure = session.state_mut::<Structure>();
    let Some(kept) = structure.kept.get_mut(id) else { return };
    let Some(receiver) = kept.loading.take() else { return };
    let loaded = if wait {
        receiver.recv().ok()
    } else {
        match receiver.try_recv() {
            Ok(loaded) => Some(loaded),
            Err(TryRecvError::Empty) => {
                kept.loading = Some(receiver);
                return;
            }
            Err(TryRecvError::Disconnected) => None,
        }
    };
    if let Some(Some(snapshot)) = loaded
        && snapshot.connection == id
        && kept.snapshot.taken == 0
        && kept.snapshot.nodes.is_empty()
    {
        kept.snapshot = snapshot;
    }
    structure.revision += 1;
}

fn connected(session: &Session, id: &str) -> bool {
    session
        .state::<Database>()
        .and_then(Database::started)
        .is_some_and(|engine| matches!(engine.status(id).state, ConnectionState::Connected | ConnectionState::Closed))
}

fn merge(into: &mut IntrospectRequest, more: &IntrospectRequest) {
    for schema in &more.schemas {
        if !into.schemas.contains(schema) {
            into.schemas.push(schema.clone());
        }
    }
    for object in &more.objects {
        if !into.objects.contains(object) {
            into.objects.push(object.clone());
        }
    }
    into.force |= more.force;
    into.system |= more.system;
}

pub fn full_request(force: bool) -> IntrospectRequest {
    IntrospectRequest { force, large_schema: LARGE_SCHEMA, ..IntrospectRequest::default() }
}

pub fn refresh(
    session: &mut Session,
    entry: &Entry,
    request: IntrospectRequest,
    path: Vec<String>,
) -> Result<Option<u64>, CommandError> {
    ensure(session, entry, true);
    let id = entry.source.id.clone();
    if !connected(session, &id) {
        return Ok(None);
    }
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else { return Ok(None) };
    let structure = session.state_mut::<Structure>();
    structure.revision += 1;
    let Some(kept) = structure.kept.get_mut(&id) else { return Ok(None) };
    kept.errors.retain(|failed, _| !failed.starts_with(&path));
    if !kept.relational {
        let mut paths: Vec<Vec<String>> = kept
            .snapshot
            .nodes
            .iter()
            .map(|cached| cached.path.clone())
            .filter(|cached| cached.starts_with(&path))
            .take(GENERIC_REFRESHED)
            .collect();
        if paths.is_empty() {
            paths.push(path);
        }
        let mut first = None;
        for path in paths {
            if kept.children.contains_key(&path) {
                continue;
            }
            let op = engine.children(&id, path.clone()).map_err(|error| failed(&error))?;
            first.get_or_insert(op);
            kept.children.insert(path, op);
        }
        return Ok(first.or_else(|| kept.children.values().next().copied()));
    }
    if let Some((running, waiting)) = &mut kept.refresh {
        let running = *running;
        waiting.push(path.clone());
        match &mut kept.queued {
            Some((queued, paths)) => {
                merge(queued, &request);
                paths.push(path);
            }
            None => kept.queued = Some((request, vec![path])),
        }
        return Ok(Some(running));
    }
    let op =
        engine.introspect(&id, request, kept.snapshot.clone(), kept.file.clone()).map_err(|error| failed(&error))?;
    kept.refresh = Some((op, vec![path]));
    Ok(Some(op))
}

fn failed(error: &DbError) -> CommandError {
    let code = if error.kind == ErrorKind::NotConnected { "notConnected" } else { "databaseError" };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn on_a_thread(job: Job) -> bool {
    std::thread::Builder::new().name("zen-db-snapshot".to_owned()).spawn(job).is_ok()
}

fn save_in_background(kept: &Kept) {
    if let Some(file) = &kept.file {
        kept.saver.save(kept.snapshot.clone(), file, on_a_thread);
    }
}

pub fn step(session: &mut Session, changed: &[String]) {
    let waiting: Vec<String> = session
        .state::<Structure>()
        .map(|structure| {
            structure.kept.iter().filter(|(_, kept)| kept.loading.is_some()).map(|(id, _)| id.clone()).collect()
        })
        .unwrap_or_default();
    for id in waiting {
        take_loaded(session, &id, false);
    }
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else { return };
    let mut connected_now = Vec::new();
    let known = engine.known();
    {
        let structure = session.state_mut::<Structure>();
        for id in &known {
            let state = engine.status(id).state;
            let before = structure.states.insert(id.clone(), state);
            if state == ConnectionState::Connected
                && !matches!(before, Some(ConnectionState::Connected | ConnectionState::Closed))
            {
                connected_now.push(id.clone());
            }
            if before != Some(state) {
                structure.revision += 1;
            }
        }
        let mut changes = false;
        let mut restart = Vec::new();
        for (id, kept) in &mut structure.kept {
            if let Some((op, paths)) = kept.refresh.clone()
                && let Some(state) = engine.operation(op)
                && state.status != OperationStatus::Running
            {
                kept.refresh = None;
                changes = true;
                match engine.take_snapshot(op) {
                    Some(snapshot) if state.status == OperationStatus::Done => kept.snapshot = snapshot,
                    _ => {
                        let message = state.error.as_ref().map_or_else(|| t!("database-error-cancelled"), describe);
                        for path in paths {
                            kept.errors.insert(path, message.clone());
                        }
                    }
                }
                if let Some(queued) = kept.queued.take() {
                    restart.push((id.clone(), queued));
                }
            }
            let finished: Vec<(Vec<String>, u64)> = kept
                .children
                .iter()
                .filter(|(_, op)| engine.operation(**op).is_none_or(|state| state.status != OperationStatus::Running))
                .map(|(path, op)| (path.clone(), *op))
                .collect();
            for (path, op) in finished {
                kept.children.remove(&path);
                changes = true;
                match engine.operation(op) {
                    Some(state) if state.status == OperationStatus::Done => {
                        let nodes = state.nodes.unwrap_or_default().into_iter().map(CachedNode::from).collect();
                        kept.snapshot.keep(&path, nodes);
                        kept.errors.remove(&path);
                        if kept.children.is_empty() {
                            save_in_background(kept);
                        }
                    }
                    state => {
                        let message = state
                            .and_then(|state| state.error)
                            .map_or_else(|| t!("database-error-cancelled"), |error| describe(&error));
                        kept.errors.insert(path, message);
                    }
                }
            }
        }
        if changes {
            structure.revision += 1;
        }
        for (id, (request, paths)) in restart {
            let Some(kept) = structure.kept.get_mut(&id) else { continue };
            if let Ok(op) = engine.introspect(&id, request, kept.snapshot.clone(), kept.file.clone()) {
                kept.refresh = Some((op, paths));
            }
        }
    }
    for id in connected_now {
        if let Ok(entry) = find(session, &id) {
            let _ = refresh(session, &entry, full_request(false), Vec::new());
        }
    }
    for id in changed {
        let expected = session.state_mut::<Structure>().kept.get_mut(id).and_then(|kept| kept.expected.take());
        if let Ok(entry) = find(session, id) {
            let _ = refresh(session, &entry, expected.unwrap_or_else(|| full_request(false)), Vec::new());
        }
    }
}

pub fn tree_options(entry: &Entry, show_system: bool, simple: bool) -> TreeOptions {
    TreeOptions {
        show_system: show_system || entry.source.navigator.show_system_objects,
        simple,
        filters: entry.source.filters.clone(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Known {
    pub nodes: Option<Vec<Node>>,
    pub loading: bool,
    pub error: Option<String>,
    pub operation: Option<u64>,
}

fn missing(snapshot: &Snapshot, path: &[String]) -> Result<IntrospectRequest, String> {
    let mut request = full_request(false);
    match path {
        [] => {}
        [catalog, rest @ ..] => {
            if snapshot.catalog(catalog).is_some_and(|known| !known.current && !known.schemas_read) {
                return Err(t!("database-navigator-other-database", database = catalog.as_str()));
            }
            if let Some(schema) = rest.first() {
                request.schemas = vec![schema.clone()];
                request.system = true;
                if snapshot.schema(catalog, schema).is_some_and(|schema| schema.level != Level::NotRead)
                    && let Some(key) = rest.get(2)
                {
                    request.schemas.clear();
                    request.objects = vec![(schema.clone(), key.clone())];
                }
            }
        }
    }
    Ok(request)
}

pub fn children_of(session: &mut Session, entry: &Entry, path: &[String], options: &TreeOptions, load: bool) -> Known {
    ensure(session, entry, false);
    let id = entry.source.id.as_str();
    let Some(kept) = session.state::<Structure>().and_then(|structure| structure.kept.get(id)) else {
        return Known::default();
    };
    if let Some(error) = kept.errors.get(path) {
        return Known { error: Some(error.clone()), ..Known::default() };
    }
    let mut operation = kept.children.get(path).copied().or_else(|| kept.refresh.as_ref().map(|(op, _)| *op));
    let loading_from_disk = kept.loading.is_some();
    if let Some(nodes) = zen_db::schema::children(&kept.snapshot, path, options) {
        let busy = kept.children.contains_key(path);
        return Known { nodes: Some(nodes), loading: busy, error: None, operation: operation.filter(|_| busy) };
    }
    let running = operation.is_some() && (kept.children.contains_key(path) || kept.refresh.is_some());
    if running || loading_from_disk {
        return Known { loading: true, operation, ..Known::default() };
    }
    if !load || !connected(session, id) {
        return Known::default();
    }
    let relational = kept.relational;
    let request = if relational {
        match missing(&kept.snapshot, path) {
            Ok(request) => request,
            Err(error) => return Known { error: Some(error), ..Known::default() },
        }
    } else {
        IntrospectRequest::default()
    };
    let started = if relational {
        refresh(session, entry, request, path.to_vec())
    } else {
        let engine = session.state::<Database>().and_then(Database::started).cloned();
        match engine.map(|engine| engine.children(id, path.to_vec())) {
            Some(Ok(op)) => {
                let structure = session.state_mut::<Structure>();
                structure.revision += 1;
                if let Some(kept) = structure.kept.get_mut(id) {
                    kept.children.insert(path.to_vec(), op);
                }
                Ok(Some(op))
            }
            Some(Err(error)) => Err(failed(&error)),
            None => Ok(None),
        }
    };
    match started {
        Ok(started) => {
            operation = started;
            Known { loading: started.is_some(), operation, ..Known::default() }
        }
        Err(error) => Known { error: Some(error.to_string()), ..Known::default() },
    }
}

/// A connection and a path in its tree.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PathParams {
    /// The connection's id.
    pub connection: String,
    /// The path in its tree: `[]` for its databases, `["shop", "public", "tables", "orders"]` for a table, `[…, "columns", "id"]` for a column.
    pub path: Vec<String>,
    /// The most nodes or items to answer; 200 when absent.
    pub limit: Option<usize>,
}

/// Children of a node.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChildrenAnswer {
    /// The children, at most `limit`, when they are known.
    pub nodes: Vec<Node>,
    /// How many children there are.
    pub total: usize,
    /// Whether children were left out because of `limit`.
    pub truncated: bool,
    /// Whether they are being read; ask again when `operation` ended.
    pub loading: bool,
    /// The operation reading them, for `db.operation.state`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
    /// Why they could not be read, or that the connection must be connected first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub struct SchemaChildren;

impl Command for SchemaChildren {
    type Params = PathParams;
    type Output = ChildrenAnswer;
    const ID: &'static str = "db.schema.children";
    const TITLE: &'static str = "Database Structure Children";
    const DESCRIPTION: &'static str = "Lists the children of a node of database connection `connection`'s tree at `path` (`[]`: databases; `[db]`: schemas; `[db, schema]`: folders such as `tables`, `views`, `functions`; `[db, schema, folder]`: objects; `[…, object]`: its folders `columns`, `constraints`, `foreignKeys`, `indexes`, `triggers`, `references`; `[…, folder]`: their items), at most `limit` (200) with `total` and `truncated`. Answers from the snapshot kept in the IDE's caches at once, also before connecting; when the node was not read and the connection is connected, starts reading it in the background and answers `loading` with the `operation`. Never connects; never data values.";

    fn run(&self, session: &mut Session, params: PathParams) -> Result<ChildrenAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        ensure(session, &entry, true);
        let show_system =
            session.state::<super::navigator::Navigator>().is_some_and(|navigator| navigator.show_system());
        let simple = session.state::<super::navigator::Navigator>().is_some_and(super::navigator::Navigator::simple);
        let options = tree_options(&entry, show_system, simple);
        let known = children_of(session, &entry, &params.path, &options, true);
        let nodes = known.nodes.unwrap_or_default();
        let total = nodes.len();
        let limit = params.limit.unwrap_or(LISTED);
        let message = known.error.or_else(|| {
            (!known.loading && total == 0 && !connected(session, &params.connection))
                .then(|| t!("database-navigator-not-read"))
        });
        Ok(ChildrenAnswer {
            nodes: nodes.into_iter().take(limit).collect(),
            total,
            truncated: total > limit,
            loading: known.loading,
            operation: known.operation,
            message,
        })
    }
}

/// What to read again.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RefreshParams {
    /// The connection's id.
    pub connection: String,
    /// The node to read again (`[db, schema]` reads that schema); the whole connection when absent or empty.
    pub path: Vec<String>,
    /// Read even what the fingerprints say did not change.
    pub force: bool,
}

/// A refresh started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAnswer {
    /// The operation reading, for `db.operation.state`; absent when the connection is not connected (nothing connects by itself).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
    /// Whether a read started (or one already running will cover it).
    pub started: bool,
}

pub struct SchemaRefresh;

impl Command for SchemaRefresh {
    type Params = RefreshParams;
    type Output = RefreshAnswer;
    const ID: &'static str = "db.schema.refresh";
    const TITLE: &'static str = "Refresh Database Structure";
    const DESCRIPTION: &'static str = "Reads database connection `connection`'s structure again in the background, in its Metadata context: per-schema fingerprints over the system catalogues decide which schemas changed, and only those are read (`force` reads them anyway); `path` limits it to one schema or object. The snapshot in the IDE's caches is replaced when it ends. Answers the operation at once; never connects (a disconnected connection answers `started: false`).";

    fn run(&self, session: &mut Session, params: RefreshParams) -> Result<RefreshAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        let mut request = full_request(params.force);
        if let [_, schema, rest @ ..] = params.path.as_slice() {
            request.system = true;
            match rest {
                [_, key, ..] => request.objects = vec![(schema.clone(), key.clone())],
                _ => {
                    request.schemas = vec![schema.clone()];
                    request.force = true;
                }
            }
        }
        let operation = refresh(session, &entry, request, params.path)?;
        Ok(RefreshAnswer { operation, started: operation.is_some() })
    }
}

/// An object as agents read it, its lists cut at `limit`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectAnswer {
    /// The connection's id.
    pub connection: String,
    /// The object's path.
    pub path: Vec<String>,
    /// Its database.
    pub catalog: String,
    /// Its schema.
    pub schema: String,
    /// Its name qualified by its schema, quoted as its connection's SQL needs it.
    pub qualified: String,
    /// The connection's driver, whose SQL the names are quoted for.
    pub driver: String,
    /// The object with what was read of it; columns, keys, indexes and the rest at most `limit` each.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<Object>,
    /// Whether its details are being read; ask again when `operation` ended.
    pub loading: bool,
    /// The operation reading them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
    /// Whether a list or a definition was cut short.
    pub truncated: bool,
}

fn cut<T>(list: &mut Vec<T>, limit: usize) -> bool {
    let cut = list.len() > limit;
    list.truncate(limit);
    cut
}

fn short(text: &mut Option<String>) -> bool {
    match text {
        Some(text) if text.len() > DEFINITION_ANSWERED => {
            let mut end = DEFINITION_ANSWERED;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            true
        }
        _ => false,
    }
}

fn not_found(params: &PathParams) -> CommandError {
    CommandError::with_code(
        "unknownObject",
        t!("database-error-unknown-object", path = params.path.join("/"), connection = params.connection.as_str()),
    )
}

pub(super) fn snapshot_of(session: &mut Session, entry: &Entry) -> Snapshot {
    ensure(session, entry, true);
    session.state::<Structure>().and_then(|structure| structure.snapshot(&entry.source.id)).cloned().unwrap_or_default()
}

pub fn details(session: &mut Session, entry: &Entry, path: &[String]) -> Result<Option<u64>, CommandError> {
    let [_, schema, _, key, ..] = path else { return Ok(None) };
    let request = IntrospectRequest {
        objects: vec![(schema.clone(), key.clone())],
        large_schema: LARGE_SCHEMA,
        system: true,
        ..IntrospectRequest::default()
    };
    refresh(session, entry, request, path.to_vec())
}

pub struct ObjectGet;

impl Command for ObjectGet {
    type Params = PathParams;
    type Output = ObjectAnswer;
    const ID: &'static str = "db.object.get";
    const TITLE: &'static str = "Database Object";
    const DESCRIPTION: &'static str = "Answers the database object at `path` of connection `connection` (`[db, schema, folder, object]`, as db.schema.children gives them) from the snapshot: kind, owner, comment, size and row estimate, and with details its columns (type, nullability, default, identity, generated, collation, comment), constraints, foreign keys, indexes, triggers, policies, partitioning, a view's query, a routine's signature and source, a sequence's or a type's settings; each list at most `limit` (200), definitions cut at 8000 characters (`truncated`). When the details were not read and the connection is connected, starts reading them and answers `loading` with the `operation`. Never data values.";

    fn run(&self, session: &mut Session, params: PathParams) -> Result<ObjectAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        let snapshot = snapshot_of(session, &entry);
        let found = find_object(&snapshot, &params.path).ok_or_else(|| not_found(&params))?;
        let limit = params.limit.unwrap_or(LISTED);
        let (catalog, schema) = (found.catalog.name.clone(), found.schema.name.clone());
        let qualified = qualified_for(&entry.source.driver, &schema, &found.object.name);
        let mut object = found.object.clone();
        let mut operation = None;
        if !object.detailed {
            operation = details(session, &entry, &params.path)?;
        }
        let mut truncated = cut(&mut object.columns, limit);
        truncated |= cut(&mut object.constraints, limit);
        truncated |= cut(&mut object.indexes, limit);
        truncated |= cut(&mut object.triggers, limit);
        truncated |= cut(&mut object.policies, limit);
        truncated |= cut(&mut object.acl, limit);
        truncated |= short(&mut object.definition);
        if let Some(routine) = object.routine.as_mut() {
            truncated |= short(&mut routine.definition);
        }
        Ok(ObjectAnswer {
            connection: params.connection,
            path: params.path,
            catalog,
            schema,
            qualified,
            driver: entry.source.driver.clone(),
            loading: operation.is_some(),
            operation,
            object: Some(object),
            truncated,
        })
    }
}

/// What DDL to generate.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DdlParams {
    /// The connection's id.
    pub connection: String,
    /// The object's path, or an index's, constraint's or trigger's (`[…, "indexes", name]`).
    pub path: Vec<String>,
    /// The toggles, all off by default.
    #[serde(flatten)]
    pub options: DdlOptions,
}

/// Generated DDL.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DdlAnswer {
    /// The statements, when the details were read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ddl: Option<String>,
    /// Whether the details are being read; ask again when `operation` ended.
    pub loading: bool,
    /// The operation reading them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
}

pub struct ObjectDdl;

impl Command for ObjectDdl {
    type Params = DdlParams;
    type Output = DdlAnswer;
    const ID: &'static str = "db.object.ddl";
    const TITLE: &'static str = "Generate DDL";
    const DESCRIPTION: &'static str = "Generates the DDL of the database object at `path` of connection `connection` (schemas by `[db, schema]`, tables, foreign tables, views, materialised views, functions, procedures, sequences, types; or an index, constraint or trigger by `[…, \"indexes\", name]`) from the snapshot, offline. Toggles, all off by default: `nested` (a table's indexes, triggers and policies), `permissions` (GRANTs), `separateForeignKeys`, `separateConstraintIndexes` (keys as ALTER TABLE after the table), `comments`, `partitions`. When the details were not read and the connection is connected, starts reading them and answers `loading` with the `operation`.";

    fn run(&self, session: &mut Session, params: DdlParams) -> Result<DdlAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        let snapshot = snapshot_of(session, &entry);
        let schema_path = params.path.len() == 2 && snapshot.schema(&params.path[0], &params.path[1]).is_some();
        let detailed = match find_object(&snapshot, &params.path) {
            Some(found) => found.object.detailed,
            None if schema_path => true,
            None => {
                return Err(not_found(&PathParams {
                    connection: params.connection.clone(),
                    path: params.path.clone(),
                    limit: None,
                }));
            }
        };
        if !detailed {
            let operation = details(session, &entry, &params.path)?;
            if operation.is_some() {
                return Ok(DdlAnswer { ddl: None, loading: true, operation });
            }
        }
        let driver =
            session.state_mut::<Database>().drivers().into_iter().find(|driver| driver.id() == entry.source.driver);
        let driver = driver.ok_or_else(|| {
            CommandError::new(t!("database-error-unknown-driver", driver = entry.source.driver.as_str()))
        })?;
        let ddl = driver.ddl(&snapshot, &params.path, &params.options).map_err(|error| failed(&error))?;
        Ok(DdlAnswer { ddl: Some(ddl), loading: false, operation: None })
    }
}

pub(super) struct OwnTreeOpened {
    pub tab: crate::workspaces::InnerTab,
    pub kind: String,
    pub console: Option<(PathBuf, String)>,
}

pub(super) fn open_own_tree(
    session: &mut Session,
    entry: &Entry,
    params: &PathParams,
) -> Result<Option<OwnTreeOpened>, CommandError> {
    let snapshot = snapshot_of(session, entry);
    if !snapshot.catalogs.is_empty() {
        return Ok(None);
    }
    let (_, parent) = params.path.split_last().ok_or_else(|| not_found(params))?;
    let node = snapshot
        .cached(parent)
        .and_then(|nodes| nodes.iter().find(|node| node.path == params.path))
        .ok_or_else(|| not_found(params))?;
    let kind =
        serde_json::to_value(&node.kind).ok().and_then(|kind| kind.as_str().map(str::to_owned)).unwrap_or_default();
    if node.kind == zen_db::model::NodeKind::Key {
        let tab = super::keys::KeyOpen.run(
            session,
            super::keys::KeyOpenParams { connection: entry.source.id.clone(), path: params.path.clone() },
        )?;
        return Ok(Some(OwnTreeOpened { tab, kind, console: None }));
    }
    let driver =
        session.state_mut::<Database>().drivers().into_iter().find(|driver| driver.id() == entry.source.driver);
    let text = driver.and_then(|driver| driver.open_text(&params.path, &node.kind)).ok_or_else(|| not_found(params))?;
    let opened = super::NewConsole.run(
        session,
        super::NewConsoleParams { connection: Some(entry.source.id.clone()), text: Some(text.clone()) },
    )?;
    Ok(Some(OwnTreeOpened { tab: opened.tab, kind, console: Some((opened.path, text)) }))
}

/// Which name to copy.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CopyNameParams {
    /// The connection's id.
    pub connection: String,
    /// The node's path.
    pub path: Vec<String>,
    /// The name qualified by its schema (and table, for a column), quoted as SQL needs it.
    pub qualified: bool,
}

/// A name to copy.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NameAnswer {
    /// The name.
    pub name: String,
}

pub fn node_name(driver: &str, path: &[String], qualified_name: bool) -> Option<String> {
    let last = path.last()?.clone();
    if !qualified_name {
        return Some(last.split_once('(').map_or(last.clone(), |(name, _)| name.to_owned()));
    }
    Some(match path {
        [catalog] => quote_for(driver, catalog),
        [_, schema] => quote_for(driver, schema),
        [_, schema, _, key] => {
            qualified_for(driver, schema, key.split_once('(').map_or(key.as_str(), |(name, _)| name))
        }
        [_, schema, _, table, _, item] => {
            format!("{}.{}", qualified_for(driver, schema, table), quote_for(driver, item))
        }
        _ => last,
    })
}

pub struct ObjectCopyName;

impl Command for ObjectCopyName {
    type Params = CopyNameParams;
    type Output = NameAnswer;
    const ID: &'static str = "db.object.copyName";
    const TITLE: &'static str = "Copy Name";
    const DESCRIPTION: &'static str = "Answers the name of the node at `path` of connection `connection`, or with `qualified` its name qualified by schema (and table, for a column), quoted as SQL needs it; of a key-value database, the whole key or prefix. The navigator's Copy Name puts it on the clipboard.";

    fn run(&self, session: &mut Session, params: CopyNameParams) -> Result<NameAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        let key_value = session
            .state_mut::<Database>()
            .drivers()
            .into_iter()
            .find(|driver| driver.id() == entry.source.driver)
            .is_some_and(|driver| driver.capabilities().models.contains(&DataModel::KeyValue));
        let name = if params.path.is_empty() {
            Some(entry.source.name.clone())
        } else if key_value && params.path.len() >= 2 {
            Some(params.path[1..].concat())
        } else {
            node_name(&entry.source.driver, &params.path, params.qualified)
        };
        let name = name.ok_or_else(|| {
            CommandError::new(t!("database-error-unknown-object", path = "", connection = params.connection.as_str()))
        })?;
        Ok(NameAnswer { name })
    }
}

/// Which type to generate from a table.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CodeParams {
    /// The connection's id.
    pub connection: String,
    /// The table's or view's path, `[db, schema, folder, object]`.
    pub path: Vec<String>,
    /// The language: `go`, `rust` or `typescript`.
    pub language: zen_db::codegen::CodeLanguage,
}

/// A type generated from a table.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodeAnswer {
    /// The type's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// The code, when the details were read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// How many columns became fields.
    pub fields: usize,
    /// Whether columns past the most a type takes (1000) were left out.
    pub truncated: bool,
    /// Whether the details are being read; ask again when `operation` ended.
    pub loading: bool,
    /// The operation reading them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
}

const MOST_FIELDS: usize = 1000;

pub struct GenerateCode;

impl Command for GenerateCode {
    type Params = CodeParams;
    type Output = CodeAnswer;
    const ID: &'static str = "db.object.generateCode";
    const TITLE: &'static str = "Generate Code";
    const DESCRIPTION: &'static str = "Generates a type from the columns of the table or view at `path` of connection `connection`, offline from the snapshot: `language` `go` (a struct with `db` and `json` tags, pointers for nullable columns), `rust` (a struct, `Option` for nullable columns) or `typescript` (an interface, `| null` for nullable columns); the name is the table's, singular, column comments become doc comments, at most 1000 fields (`truncated`). Answers `typeName` and `code`; when the details were not read and the connection is connected, starts reading them and answers `loading` with the `operation`. Never data values.";

    fn run(&self, session: &mut Session, params: CodeParams) -> Result<CodeAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        let snapshot = snapshot_of(session, &entry);
        let path_params = PathParams { connection: params.connection.clone(), path: params.path.clone(), limit: None };
        let found = find_object(&snapshot, &params.path).ok_or_else(|| not_found(&path_params))?;
        if !found.object.kind.has_columns() {
            return Err(CommandError::with_code(
                "noColumns",
                t!("database-error-no-columns", name = found.object.name.as_str()),
            ));
        }
        if !found.object.detailed {
            let operation = details(session, &entry, &params.path)?;
            if operation.is_some() {
                return Ok(CodeAnswer {
                    type_name: None,
                    code: None,
                    fields: 0,
                    truncated: false,
                    loading: true,
                    operation,
                });
            }
        }
        let object = &found.object;
        if !object.detailed && object.columns.is_empty() {
            return Err(CommandError::with_code(
                "notRead",
                t!("database-error-columns-not-read", name = object.name.as_str()),
            ));
        }
        let truncated = object.columns.len() > MOST_FIELDS;
        let columns = &object.columns[..object.columns.len().min(MOST_FIELDS)];
        let generated = zen_db::codegen::generate(
            params.language,
            &entry.source.driver,
            &object.name,
            object.comment.as_deref(),
            columns,
        );
        Ok(CodeAnswer {
            type_name: Some(generated.type_name),
            code: Some(generated.code),
            fields: columns.len(),
            truncated,
            loading: false,
            operation: None,
        })
    }
}
