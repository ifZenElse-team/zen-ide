use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::compare::data::{RowDiff, data_script};
use zen_db::compare::{
    CompareOptions, Note, NoteKind, ObjectDiff, SchemaDiff, ScriptOptions, compare, find as find_object, sync_script,
};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{DbError, Driver};
use zen_db::engine::{
    ConnectionState, DataCompareRequest, DataCompareState, DataSide, Engine, KEPT_ROWS, OperationStatus,
};
use zen_db::registry::Entry;
use zen_db::schema::{
    Catalog, ConstraintKind, DdlOptions, IntrospectRequest, Object, ObjectKind, Schema, Snapshot, qualified_for,
    quote_for,
};
use zen_i18n::t;

use super::connections::describe;
use super::consoles::{self, ensure_connected};
use super::sources::{Database, find};
use super::{NewConsole, NewConsoleParams, WORKSPACE};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};

pub const TAB: &str = "compare";

const LISTED: usize = 20;

const OBJECTS_ANSWERED: usize = 100;

const ITEMS_ANSWERED: usize = 50;

const ROWS_ANSWERED: usize = 20;

const TEXT_ANSWERED: usize = 100_000;

const DDL_ANSWERED: usize = 20_000;

const DETAILED_SCHEMA: usize = 20_000;

const CONNECT_WAIT: Duration = Duration::from_secs(60);

const SNAPSHOT_VERSION: u32 = 1;

/// One side of a schema comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SchemaSide {
    /// A schema of a connection: read again first (only what changed) when `refresh`, else as last read (the snapshot in the IDE's caches or an earlier comparison, whichever is newer).
    Connection {
        /// The connection's id.
        connection: String,
        /// The database (catalogue) of the schema; the one holding `schema` when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        catalog: Option<String>,
        /// The schema; for MySQL and MariaDB the database, for SQLite `main`.
        schema: String,
        /// Read the structure again before comparing (connecting with the stored password when needed); true when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh: Option<bool>,
    },
    /// A schema saved to a file with `db.compare.saveSnapshot`.
    Snapshot { path: PathBuf },
}

/// A schema's structure saved to a file: never data values or secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SavedSchema {
    /// The file's version, 1.
    pub version: u32,
    /// The connection it was saved from.
    pub connection: String,
    /// That connection's name.
    pub connection_name: String,
    /// The connection's driver.
    pub driver: String,
    /// The schema's database (catalogue).
    pub catalog: String,
    /// When its structure was read, in seconds since 1970.
    pub taken: u64,
    /// The schema with its objects.
    pub schema: Schema,
}

/// One side as a comparison read it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SideInfo {
    /// What it is called: the connection's name and the schema, or the file's name.
    pub label: String,
    /// The connection, for a connection side or the one a snapshot file was saved from.
    pub connection: Option<String>,
    /// The driver.
    pub driver: String,
    /// The database (catalogue).
    pub catalog: String,
    /// The schema.
    pub schema: String,
    /// Whether it is the snapshot kept before, because the connection was not read again.
    pub cached: bool,
    /// When its structure was read, in seconds since 1970.
    pub taken: u64,
}

struct Read {
    info: SideInfo,
    snapshot: Snapshot,
}

/// Where a comparison is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Reading or comparing.
    Running,
    /// Finished.
    Done,
    /// Failed; `error` says why.
    Failed,
    /// Cancelled.
    Cancelled,
}

impl From<OperationStatus> for Status {
    fn from(status: OperationStatus) -> Self {
        match status {
            OperationStatus::Running => Status::Running,
            OperationStatus::Done => Status::Done,
            OperationStatus::Failed => Status::Failed,
            OperationStatus::Cancelled => Status::Cancelled,
        }
    }
}

#[derive(Default)]
struct Progress {
    status: Option<Status>,
    error: Option<String>,
    code: Option<String>,
    sides: Option<Box<(Read, Read)>>,
    diff: Option<Arc<SchemaDiff>>,
}

struct SchemaJob {
    started: Instant,
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

impl SchemaJob {
    fn progress(&self) -> std::sync::MutexGuard<'_, Progress> {
        self.progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn status(&self) -> Status {
        self.progress().status.unwrap_or(Status::Running)
    }
}

enum Job {
    Schema(SchemaJob),
    Data(u64),
}

/// The sides a comparison was asked to compare, as given.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Request {
    /// Two schemas.
    Schemas {
        /// The schema whose structure is wanted.
        source: SchemaSide,
        /// The schema the script would change.
        target: SchemaSide,
    },
    /// Two tables.
    Data {
        /// The table whose rows are wanted.
        source: TableSide,
        /// The table the script would change.
        target: TableSide,
    },
}

#[derive(Default)]
pub struct Compares {
    next: u64,
    jobs: BTreeMap<u64, Job>,
    requests: BTreeMap<u64, Request>,
    revision: u64,
}

impl Compares {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn add(&mut self, job: Job, request: Request) -> u64 {
        self.next += 1;
        self.revision += 1;
        self.jobs.insert(self.next, job);
        self.requests.insert(self.next, request);
        while self.jobs.len() > LISTED * 2 {
            let Some(oldest) = self.jobs.keys().next().copied() else { break };
            self.jobs.remove(&oldest);
            self.requests.remove(&oldest);
        }
        self.next
    }
}

fn unknown(id: u64) -> CommandError {
    CommandError::with_code("unknownCompare", t!("database-compare-error-unknown", id = id))
}

fn db_failed(error: &DbError) -> CommandError {
    CommandError::with_code(error.code.as_deref().unwrap_or("databaseError"), describe(error))
        .with_data(serde_json::json!({ "error": error }))
}

fn engine(session: &mut Session) -> Engine {
    consoles::engine(session)
}

fn driver(session: &mut Session, id: &str) -> Result<Arc<dyn Driver>, CommandError> {
    session
        .state_mut::<Database>()
        .drivers()
        .into_iter()
        .find(|driver| driver.id() == id)
        .ok_or_else(|| CommandError::new(t!("database-error-unknown-driver", driver = id)))
}

fn dialect(driver: &dyn Driver) -> Result<SqlDialect, CommandError> {
    SqlDialect::of_language(&driver.capabilities().language)
        .ok_or_else(|| CommandError::with_code("notRelational", t!("database-compare-error-not-relational")))
}

fn snapshots_folder(session: &Session) -> Option<PathBuf> {
    session.state::<Workspaces>().and_then(Workspaces::data_dir).map(|data| data.join("databases").join("snapshots"))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn find_schema<'a>(snapshot: &'a Snapshot, catalog: Option<&str>, schema: &str) -> Option<(&'a Catalog, &'a Schema)> {
    snapshot
        .catalogs
        .iter()
        .filter(|known| catalog.is_none_or(|catalog| known.name == catalog))
        .find_map(|known| known.schemas.iter().find(|candidate| candidate.name == schema).map(|found| (known, found)))
}

fn read_saved(path: &Path) -> Result<SavedSchema, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let saved: SavedSchema = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if saved.version != SNAPSHOT_VERSION {
        return Err(t!("database-compare-error-snapshot-version", version = saved.version));
    }
    Ok(saved)
}

struct Planned {
    side: SchemaSide,
    entry: Option<Entry>,
    previous: Snapshot,
    refresh: bool,
}

struct Failure {
    code: &'static str,
    message: String,
}

fn wait_connected(engine: &Engine, id: &str, cancel: &AtomicBool) -> Result<(), Failure> {
    let deadline = Instant::now() + CONNECT_WAIT;
    loop {
        match engine.status(id).state {
            ConnectionState::Connected | ConnectionState::Closed => return Ok(()),
            ConnectionState::Connecting if Instant::now() < deadline => {}
            _ => {
                return Err(Failure {
                    code: "notConnected",
                    message: t!("database-compare-error-not-connected", connection = id),
                });
            }
        }
        if cancel.load(Ordering::SeqCst) {
            return Err(Failure { code: "cancelled", message: t!("database-error-cancelled") });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn read_side(engine: &Engine, planned: Planned, cancel: &AtomicBool) -> Result<Read, Failure> {
    match planned.side {
        SchemaSide::Snapshot { path } => {
            let saved = read_saved(&path).map_err(|error| Failure {
                code: "badSnapshot",
                message: t!("database-compare-error-snapshot", path = crate::display(&path), error = error),
            })?;
            let label = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let mut snapshot = Snapshot::new(&saved.connection, &saved.driver);
            snapshot.taken = saved.taken;
            snapshot.catalogs.push(Catalog {
                name: saved.catalog.clone(),
                current: true,
                schemas_read: true,
                schemas: vec![saved.schema.clone()],
                ..Catalog::default()
            });
            Ok(Read {
                info: SideInfo {
                    label,
                    connection: Some(saved.connection),
                    driver: saved.driver,
                    catalog: saved.catalog,
                    schema: saved.schema.name,
                    cached: false,
                    taken: saved.taken,
                },
                snapshot,
            })
        }
        SchemaSide::Connection { connection, catalog, schema, .. } => {
            let entry = planned.entry.ok_or_else(|| Failure {
                code: "unknownConnection",
                message: t!("database-error-unknown-connection", id = connection.as_str()),
            })?;
            let mut snapshot = planned.previous;
            let mut cached = true;
            if planned.refresh {
                wait_connected(engine, &connection, cancel)?;
                let request = IntrospectRequest {
                    schemas: vec![schema.clone()],
                    large_schema: DETAILED_SCHEMA,
                    system: true,
                    ..IntrospectRequest::default()
                };
                let op = engine
                    .introspect(&connection, request, snapshot.clone(), None)
                    .map_err(|error| Failure { code: "databaseError", message: describe(&error) })?;
                loop {
                    if cancel.load(Ordering::SeqCst) {
                        engine.cancel(op);
                    }
                    let Some(state) = engine.wait(op, Duration::from_millis(100)) else {
                        return Err(Failure { code: "databaseError", message: t!("database-error-cancelled") });
                    };
                    match state.status {
                        OperationStatus::Running => continue,
                        OperationStatus::Done => {
                            if let Some(read) = engine.take_snapshot(op) {
                                snapshot = read;
                                cached = false;
                            }
                            break;
                        }
                        OperationStatus::Cancelled => {
                            return Err(Failure { code: "cancelled", message: t!("database-error-cancelled") });
                        }
                        OperationStatus::Failed => {
                            let message = state.error.as_ref().map(describe).unwrap_or_default();
                            return Err(Failure { code: "databaseError", message });
                        }
                    }
                }
            }
            let Some((found_catalog, found)) = find_schema(&snapshot, catalog.as_deref(), &schema) else {
                return Err(Failure {
                    code: "unknownSchema",
                    message: t!(
                        "database-compare-error-no-schema",
                        schema = schema.as_str(),
                        connection = entry.source.name.as_str()
                    ),
                });
            };
            let info = SideInfo {
                label: format!("{} · {}", entry.source.name, found.name),
                connection: Some(connection.clone()),
                driver: entry.source.driver.clone(),
                catalog: found_catalog.name.clone(),
                schema: found.name.clone(),
                cached,
                taken: snapshot.taken,
            };
            Ok(Read { info, snapshot })
        }
    }
}

fn schema_of(read: &Read) -> Option<&Schema> {
    find_schema(&read.snapshot, Some(&read.info.catalog), &read.info.schema).map(|(_, schema)| schema)
}

fn plan(session: &mut Session, side: &SchemaSide) -> Result<Planned, CommandError> {
    match side {
        SchemaSide::Snapshot { path } => {
            super::guard_agent_file(session, path)?;
            if !path.is_file() {
                return Err(CommandError::with_code(
                    "noSnapshot",
                    t!("database-compare-error-no-file", path = crate::display(path)),
                ));
            }
            if zen_core::macros::by_agent(session)
                && let Ok(saved) = read_saved(path)
                && let Ok(entry) = find(session, &saved.connection)
                && super::safety::agent_access(session, &entry) == zen_db::types::AgentAccess::None
            {
                return Err(CommandError::with_code(
                    super::safety::AGENT_NOT_ALLOWED,
                    t!(
                        "database-error-agent-closed",
                        name = entry.source.name.as_str(),
                        kind = super::safety::type_of(session, &entry).name.as_str()
                    ),
                ));
            }
            Ok(Planned { side: side.clone(), entry: None, previous: Snapshot::default(), refresh: false })
        }
        SchemaSide::Connection { connection, catalog, schema, refresh } => {
            let entry = find(session, connection)?;
            dialect(driver(session, &entry.source.driver)?.as_ref())?;
            super::safety::agent_reads(session, &entry, "SELECT 1")?;
            let mut previous = super::structure::snapshot_of(session, &entry);
            if let Some(read) = freshest_read(session, connection, schema)
                && (read.taken > previous.taken || find_schema(&previous, catalog.as_deref(), schema).is_none())
            {
                previous = read;
            }
            let refresh = refresh.unwrap_or(true);
            if refresh {
                ensure_connected(session, connection)?;
            }
            Ok(Planned { side: side.clone(), entry: Some(entry), previous, refresh })
        }
    }
}

/// What `db.compare.schemas` compares.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemasParams {
    /// The schema whose structure is wanted.
    pub source: SchemaSide,
    /// The schema the script would change.
    pub target: SchemaSide,
    /// What to compare.
    #[serde(default)]
    pub options: CompareOptions,
}

/// A comparison started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// Its id, for `db.compare.state`, `.cancel`, `.ddl` and `.script`.
    pub id: u64,
}

pub struct CompareSchemas;

impl Command for CompareSchemas {
    type Params = SchemasParams;
    type Output = Started;
    const ID: &'static str = "db.compare.schemas";
    const TITLE: &'static str = "Compare Schemas";
    const DESCRIPTION: &'static str = "Compares two schemas' structure in the background: `source` and `target` are each `{ kind: connection, connection, schema, catalog?, refresh? }` (read again first, only what changed, connecting with the stored password when needed; `refresh: false` compares the snapshot kept in the IDE's caches, offline) or `{ kind: snapshot, path }` (a file from db.compare.saveSnapshot). `options`: `kinds` (table, view, function…; all when empty), `ignoreComments`, `system`. Tables, views, routines, sequences and types are compared with their columns, keys, foreign keys, indexes, triggers, policies, definitions and comments. Answers the comparison's id at once; follow it with db.compare.state, then db.compare.ddl and db.compare.script. Never data values.";

    fn run(&self, session: &mut Session, params: SchemasParams) -> Result<Started, CommandError> {
        let source = plan(session, &params.source)?;
        let target = plan(session, &params.target)?;
        let engine = engine(session);
        let waker = session.state::<Database>().and_then(Database::waker);
        let progress = Arc::new(Mutex::new(Progress::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let job = SchemaJob { started: Instant::now(), progress: Arc::clone(&progress), cancel: Arc::clone(&cancel) };
        let request = Request::Schemas { source: params.source, target: params.target };
        let id = session.state_mut::<Compares>().add(Job::Schema(job), request);
        let options = params.options;
        let thread = std::thread::Builder::new().name("zen-db-compare".to_owned()).spawn(move || {
            let read = read_side(&engine, source, &cancel)
                .and_then(|source| Ok((source, read_side(&engine, target, &cancel)?)));
            let mut progress = progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match read {
                Ok((source, target)) => {
                    if let (Some(from), Some(to)) = (schema_of(&source), schema_of(&target)) {
                        progress.diff = Some(Arc::new(compare(from, to, &options)));
                    }
                    progress.sides = Some(Box::new((source, target)));
                    progress.status = Some(Status::Done);
                }
                Err(failure) => {
                    progress.status =
                        Some(if failure.code == "cancelled" { Status::Cancelled } else { Status::Failed });
                    progress.code = Some(failure.code.to_owned());
                    progress.error = Some(failure.message);
                }
            }
            drop(progress);
            if let Some(waker) = waker {
                waker();
            }
        });
        thread.map_err(|error| CommandError::new(error.to_string()))?;
        Ok(Started { id })
    }
}

/// One side of a data comparison.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TableSide {
    /// The connection's id.
    pub connection: String,
    /// The table's schema (for MySQL and MariaDB its database); unqualified when absent.
    pub schema: Option<String>,
    /// The table.
    pub table: String,
}

/// What `db.compare.data` compares.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataParams {
    /// The table whose rows are wanted.
    pub source: TableSide,
    /// The table the script would change.
    pub target: TableSide,
    /// The key columns; the source table's primary key from its structure when absent.
    #[serde(default)]
    pub key: Option<Vec<String>>,
    /// The columns compared besides the key; the columns both tables have (as their structure knows them) when absent.
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    /// Rows read at a time; 1000 when absent.
    #[serde(default)]
    pub page: Option<usize>,
    /// The most differing rows kept for the view and the script; 10 000 when absent.
    #[serde(default)]
    pub kept: Option<usize>,
}

fn table_in(snapshot: &Snapshot, side: &TableSide) -> Option<Object> {
    snapshot
        .catalogs
        .iter()
        .flat_map(|catalog| &catalog.schemas)
        .filter(|schema| side.schema.as_ref().is_none_or(|wanted| &schema.name == wanted))
        .flat_map(|schema| &schema.objects)
        .find(|object| object.name == side.table && object.kind.has_columns() && object.detailed)
        .cloned()
}

fn table_object(session: &mut Session, entry: &Entry, side: &TableSide) -> Option<Object> {
    let kept = super::structure::snapshot_of(session, entry);
    let read = session.state::<Compares>().and_then(|compares| {
        compares
            .jobs
            .values()
            .filter_map(|job| match job {
                Job::Schema(job) => job.progress().sides.as_ref().map(|sides| {
                    [&sides.0, &sides.1]
                        .into_iter()
                        .filter(|read| read.info.connection.as_deref() == Some(side.connection.as_str()))
                        .map(|read| read.snapshot.clone())
                        .collect::<Vec<_>>()
                }),
                Job::Data(_) => None,
            })
            .flatten()
            .filter(|snapshot| table_in(snapshot, side).is_some())
            .max_by_key(|snapshot| snapshot.taken)
    });
    match (table_in(&kept, side), read) {
        (Some(found), Some(read)) if kept.taken >= read.taken => Some(found),
        (_, Some(read)) => table_in(&read, side),
        (found, None) => found,
    }
}

fn data_side(session: &mut Session, side: &TableSide) -> Result<(DataSide, Option<Object>), CommandError> {
    let entry = find(session, &side.connection)?;
    let dialect = dialect(driver(session, &entry.source.driver)?.as_ref())?;
    super::safety::agent_reads(session, &entry, "SELECT 1")?;
    if side.table.trim().is_empty() {
        return Err(CommandError::with_code("noTable", t!("database-compare-error-no-table")));
    }
    let table = match side.schema.as_deref().filter(|schema| !schema.is_empty()) {
        Some(schema) => qualified_for(&entry.source.driver, schema, &side.table),
        None => quote_for(&entry.source.driver, &side.table),
    };
    let object = table_object(session, &entry, side);
    Ok((DataSide { connection: side.connection.clone(), table, dialect }, object))
}

pub struct CompareData;

impl Command for CompareData {
    type Params = DataParams;
    type Output = Started;
    const ID: &'static str = "db.compare.data";
    const TITLE: &'static str = "Compare Data";
    const DESCRIPTION: &'static str = "Compares the rows of two tables by key in the background: `source` and `target` are `{ connection, schema?, table }`; `key` (the source's primary key when absent; never NULL) and `columns` (those both tables have when absent) name columns the same on both sides. The source is read page by page (`page`, 1000) ordered by key and each page is looked up by key on the target, then the target is read for the rows the source lacks; every page is a statement of its own in a read-only, auto-commit session of the comparison's own, so no lock outlives a page. Connects first when needed with the stored password. Answers the comparison's id at once; follow it with db.compare.state (counts and the differing rows, at most `kept`, 10 000) and db.compare.script.";

    fn run(&self, session: &mut Session, params: DataParams) -> Result<Started, CommandError> {
        let (source, source_object) = data_side(session, &params.source)?;
        let (target, target_object) = data_side(session, &params.target)?;
        let key = match params.key.filter(|key| !key.is_empty()) {
            Some(key) => key,
            None => source_object
                .as_ref()
                .and_then(|object| {
                    object.constraints.iter().find(|constraint| constraint.kind == ConstraintKind::PrimaryKey)
                })
                .map(|constraint| constraint.columns.clone())
                .ok_or_else(|| CommandError::with_code("noKey", t!("database-compare-error-no-key")))?,
        };
        let columns = match params.columns {
            Some(columns) => columns,
            None => {
                let names = |object: &Option<Object>| {
                    object
                        .as_ref()
                        .map(|object| object.columns.iter().map(|column| column.name.clone()).collect::<Vec<_>>())
                };
                match (names(&source_object), names(&target_object)) {
                    (Some(from), Some(to)) => from.into_iter().filter(|name| to.contains(name)).collect(),
                    (Some(from), None) => from,
                    _ => return Err(CommandError::with_code("noColumns", t!("database-compare-error-no-columns"))),
                }
            }
        };
        ensure_connected(session, &source.connection)?;
        ensure_connected(session, &target.connection)?;
        let request = DataCompareRequest {
            source,
            target,
            key,
            columns,
            page: params.page.unwrap_or(zen_db::engine::COMPARE_PAGE).clamp(1, 50_000),
            kept: params.kept.unwrap_or(KEPT_ROWS).min(100_000),
        };
        let op = engine(session).compare_data(request).map_err(|error| db_failed(&error))?;
        let asked = Request::Data { source: params.source, target: params.target };
        let id = session.state_mut::<Compares>().add(Job::Data(op), asked);
        Ok(Started { id })
    }
}

/// Which comparison to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// The comparison; the window's comparisons, newest first, when absent.
    pub id: Option<u64>,
    /// How long to wait for it to end, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
    /// The most objects (schemas, 100) or rows (data, 20) to answer, or comparisons to list (20).
    pub limit: Option<usize>,
    /// The first object or row to answer, for paging.
    pub offset: Option<usize>,
}

/// What a schema comparison found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSummary {
    /// Objects only the source has.
    pub added: usize,
    /// Objects only the target has.
    pub removed: usize,
    /// Objects both have, different.
    pub changed: usize,
    /// Objects both have, the same.
    pub same: usize,
    /// Objects compared by name only, their details not read.
    pub partial: usize,
}

/// A comparison as agents and the Compare tab read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompareState {
    /// Its id.
    pub id: u64,
    /// `schemas` or `data`.
    pub kind: String,
    /// The sides it was asked to compare, as given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Request>,
    /// Where it is.
    pub status: Status,
    /// Milliseconds it ran, or has run so far.
    pub elapsed_ms: u64,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The error's code (`notConnected`, `unknownSchema`, `badSnapshot`…).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The source as read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SideInfo>,
    /// The target as read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<SideInfo>,
    /// How many objects differ which way, for schemas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SchemaSummary>,
    /// The objects that differ from `offset`, at most `limit`, each with at most 50 parts, for schemas.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<ObjectDiff>,
    /// The data comparison's state, for data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DataCompareState>,
    /// The differing rows from `offset`, at most `limit`, for data.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<RowDiff>,
    /// How many objects or rows there are to answer.
    pub total: usize,
    /// Whether objects, parts or rows were left out because of `limit`.
    pub truncated: bool,
}

/// The window's comparisons, or one of them.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StateAnswer {
    /// The comparisons, newest first; one when `id` was given.
    pub compares: Vec<CompareState>,
    /// Whether comparisons were left out because of `limit`.
    pub truncated: bool,
}

fn job_state(
    session: &mut Session,
    id: u64,
    (offset, limit): (usize, Option<usize>),
    listing: bool,
) -> Option<CompareState> {
    let engine = engine(session);
    let compares = session.state::<Compares>()?;
    match compares.jobs.get(&id)? {
        Job::Schema(job) => {
            let progress = job.progress();
            let status = progress.status.unwrap_or(Status::Running);
            let mut state = CompareState {
                id,
                kind: "schemas".to_owned(),
                request: compares.requests.get(&id).cloned(),
                status,
                elapsed_ms: u64::try_from(job.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                error: progress.error.clone(),
                code: progress.code.clone(),
                source: progress.sides.as_ref().map(|sides| sides.0.info.clone()),
                target: progress.sides.as_ref().map(|sides| sides.1.info.clone()),
                summary: None,
                objects: Vec::new(),
                data: None,
                rows: Vec::new(),
                total: 0,
                truncated: false,
            };
            if let Some(diff) = &progress.diff {
                let (added, removed, changed) = diff.counts();
                state.summary = Some(SchemaSummary { added, removed, changed, same: diff.same, partial: diff.partial });
                state.total = diff.objects.len();
                if !listing {
                    let limit = limit.unwrap_or(OBJECTS_ANSWERED);
                    let mut cut = false;
                    state.objects = diff
                        .objects
                        .iter()
                        .skip(offset)
                        .take(limit)
                        .cloned()
                        .map(|mut object| {
                            cut |= object.items.len() > ITEMS_ANSWERED;
                            object.items.truncate(ITEMS_ANSWERED);
                            object
                        })
                        .collect();
                    state.truncated = cut || offset + state.objects.len() < state.total;
                }
            }
            Some(state)
        }
        Job::Data(op) => {
            let data = engine.data_compare_state(*op)?;
            let mut state = CompareState {
                id,
                kind: "data".to_owned(),
                request: compares.requests.get(&id).cloned(),
                status: data.status.into(),
                elapsed_ms: data.elapsed_ms,
                error: data.error.as_ref().map(describe),
                code: data.error.as_ref().and_then(|error| error.code.clone()),
                source: None,
                target: None,
                summary: None,
                objects: Vec::new(),
                data: None,
                rows: Vec::new(),
                total: data.kept,
                truncated: false,
            };
            if !listing {
                let rows = engine.data_compare_rows(*op).unwrap_or_default();
                let limit = limit.unwrap_or(ROWS_ANSWERED);
                state.rows = rows.into_iter().skip(offset).take(limit).collect();
                state.truncated = data.truncated || offset + state.rows.len() < data.kept;
            }
            state.data = Some(data);
            Some(state)
        }
    }
}

fn wait_for(session: &mut Session, id: u64, wait: Duration) {
    let deadline = Instant::now() + wait;
    let engine = engine(session);
    let target = match session.state::<Compares>().and_then(|compares| compares.jobs.get(&id)) {
        Some(Job::Schema(job)) => Some(Arc::clone(&job.progress)),
        Some(Job::Data(op)) => {
            engine.wait(*op, wait);
            return;
        }
        None => None,
    };
    let Some(progress) = target else { return };
    while Instant::now() < deadline {
        if progress.lock().is_ok_and(|progress| progress.status.is_some()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub struct CompareStateCommand;

impl Command for CompareStateCommand {
    type Params = StateParams;
    type Output = StateAnswer;
    const ID: &'static str = "db.compare.state";
    const TITLE: &'static str = "Compare State";
    const DESCRIPTION: &'static str = "The state of comparison `id` (waiting up to `waitMs`, 60 000 at most, for it to end), or the window's comparisons newest first (at most `limit`, 20, without their findings), each with `request`, its sides as given. Schemas: status, both sides as read (`cached` when a connection was not read again), counts (added: only in the source; removed: only in the target; changed; same; partial: compared by name only) and the objects that differ from `offset`, at most `limit` (100), each with its parts (columns, constraints, foreignKeys, indexes, triggers, policies, definition, properties: at most 50) and which fields differ. Data: status, the side being read, counts (sourceRows, targetRows, same, changed, added, removed) and the differing rows kept from `offset`, at most `limit` (20), with the key, the columns that differ and both sides' values. `total` and `truncated` say what was left out.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<StateAnswer, CommandError> {
        let offset = params.offset.unwrap_or(0);
        if let Some(id) = params.id {
            if let Some(wait) = params.wait_ms {
                wait_for(session, id, Duration::from_millis(wait.min(60_000)));
            }
            let state = job_state(session, id, (offset, params.limit), false).ok_or_else(|| unknown(id))?;
            return Ok(StateAnswer { compares: vec![state], truncated: false });
        }
        let ids: Vec<u64> =
            session.state::<Compares>().map(|compares| compares.jobs.keys().copied().collect()).unwrap_or_default();
        let limit = params.limit.unwrap_or(LISTED);
        let compares: Vec<CompareState> =
            ids.iter().rev().take(limit).filter_map(|id| job_state(session, *id, (0, None), true)).collect();
        Ok(StateAnswer { truncated: ids.len() > compares.len(), compares })
    }
}

/// Which comparison to cancel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IdParams {
    /// The comparison.
    pub id: u64,
}

/// Whether a comparison was cancelled.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Whether it was running and is being cancelled.
    pub cancelled: bool,
}

pub struct CancelCompare;

impl Command for CancelCompare {
    type Params = IdParams;
    type Output = Cancelled;
    const ID: &'static str = "db.compare.cancel";
    const TITLE: &'static str = "Cancel Compare";
    const DESCRIPTION: &'static str = "Cancels comparison `id`: reading the structure or the rows stops, and the statement running on the server is cancelled. Answers whether it was running.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<Cancelled, CommandError> {
        let engine = engine(session);
        let compares = session.state::<Compares>().ok_or_else(|| unknown(params.id))?;
        let cancelled = match compares.jobs.get(&params.id).ok_or_else(|| unknown(params.id))? {
            Job::Schema(job) => {
                let running = job.status() == Status::Running;
                job.cancel.store(true, Ordering::SeqCst);
                running
            }
            Job::Data(op) => engine.cancel(*op),
        };
        Ok(Cancelled { cancelled })
    }
}

fn finished_schemas(session: &Session, id: u64) -> Result<(Arc<SchemaDiff>, Arc<Mutex<Progress>>), CommandError> {
    let compares = session.state::<Compares>().ok_or_else(|| unknown(id))?;
    let Some(Job::Schema(job)) = compares.jobs.get(&id) else { return Err(unknown(id)) };
    let diff = job
        .progress()
        .diff
        .clone()
        .ok_or_else(|| CommandError::with_code("notFinished", t!("database-compare-error-not-finished")))?;
    Ok((diff, Arc::clone(&job.progress)))
}

fn ddl_of(driver: &dyn Driver, read: &Read, object: &Object) -> Option<String> {
    let schema = schema_of(read)?;
    let path = vec![read.info.catalog.clone(), schema.name.clone(), object.kind.folder().to_owned(), object.key()];
    let options = DdlOptions { nested: true, comments: true, ..DdlOptions::default() };
    driver.ddl(&read.snapshot, &path, &options).ok()
}

fn cut(text: String, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text, false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

/// Which object's DDL to show.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DdlParams {
    /// The schema comparison.
    pub id: u64,
    /// The object's kind.
    pub kind: ObjectKind,
    /// The object's key, as `objects[].key` of db.compare.state names it.
    pub key: String,
    /// The most characters of each side; 20 000 when absent.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// An object's DDL on both sides.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DdlAnswer {
    /// The source's DDL, empty when only the target has it.
    pub source: String,
    /// The target's DDL, empty when only the source has it.
    pub target: String,
    /// Whether a side was cut at `limit`.
    pub truncated: bool,
}

pub struct CompareDdl;

impl Command for CompareDdl {
    type Params = DdlParams;
    type Output = DdlAnswer;
    const ID: &'static str = "db.compare.ddl";
    const TITLE: &'static str = "Compared DDL";
    const DESCRIPTION: &'static str = "The DDL of object `kind`/`key` of finished schema comparison `id` on both sides, each generated offline from its side's structure by its driver (with indexes, triggers and comments), for showing side by side; each cut at `limit` characters (20 000). Empty for the side that lacks it.";

    fn run(&self, session: &mut Session, params: DdlParams) -> Result<DdlAnswer, CommandError> {
        let (_, progress) = finished_schemas(session, params.id)?;
        let progress = progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(sides) = progress.sides.as_ref() else { return Err(unknown(params.id)) };
        let (source, target) = (&sides.0, &sides.1);
        let mut texts = Vec::new();
        for read in [source, target] {
            let driver = driver(session, &read.info.driver)?;
            let object = schema_of(read).and_then(|schema| find_object(schema, params.kind, &params.key));
            texts.push(object.and_then(|object| ddl_of(driver.as_ref(), read, object)).unwrap_or_default());
        }
        let limit = params.limit.unwrap_or(DDL_ANSWERED);
        let (target_text, target_cut) = cut(texts.pop().unwrap_or_default(), limit);
        let (source_text, source_cut) = cut(texts.pop().unwrap_or_default(), limit);
        Ok(DdlAnswer { source: source_text, target: target_text, truncated: source_cut || target_cut })
    }
}

/// Which script to write.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptParams {
    /// The comparison.
    pub id: u64,
    /// Drop (or delete) what only the target has; left as notes otherwise.
    pub drop: bool,
    /// Open the script in a new console of the target's connection instead of answering its text; nothing runs it.
    pub open: bool,
    /// The most characters of the text to answer; 100 000 when absent.
    pub limit: Option<usize>,
}

/// A script written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptAnswer {
    /// The script, cut at `limit`; absent when it was opened in a console.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// How many statements it has.
    pub statements: usize,
    /// How many parts are notes instead of statements (what drops were not asked for, what the dialect cannot change).
    pub notes: usize,
    /// The console it was opened in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console: Option<PathBuf>,
    /// Whether the text was cut at `limit`.
    pub truncated: bool,
}

fn describe_note(note: &Note) -> String {
    let object = note.object.as_str();
    let part = note.part.as_str();
    match (note.kind, part.is_empty()) {
        (NoteKind::NotDropped, true) => t!("database-compare-note-not-dropped", object = object),
        (NoteKind::NotDropped, false) => t!("database-compare-note-not-dropped-part", object = object, part = part),
        (NoteKind::CannotAlter, _) => t!("database-compare-note-cannot-alter", object = object, part = part),
        (NoteKind::ByHand, _) => t!("database-compare-note-by-hand", object = object, part = part),
        (NoteKind::NoDdl, _) => t!("database-compare-note-no-ddl", object = object),
    }
}

fn schema_script(
    session: &mut Session,
    id: u64,
    drop: bool,
) -> Result<(String, usize, usize, Option<String>), CommandError> {
    let (diff, progress) = finished_schemas(session, id)?;
    let progress = progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(sides) = progress.sides.as_ref() else { return Err(unknown(id)) };
    let (source, target) = (&sides.0, &sides.1);
    let (Some(from), Some(to)) = (schema_of(source), schema_of(target)) else { return Err(unknown(id)) };
    let target_driver = driver(session, &target.info.driver)?;
    let dialect = dialect(target_driver.as_ref())?;
    let catalog = target.info.catalog.clone();
    let create = |schema: &Schema, object: &Object| {
        let mut snapshot = Snapshot::new("compare", target_driver.id());
        let mut copy = schema.clone();
        if let Some(slot) =
            copy.objects.iter_mut().find(|known| known.kind == object.kind && known.key() == object.key())
        {
            *slot = object.clone();
        }
        snapshot.catalogs.push(Catalog {
            name: catalog.clone(),
            current: true,
            schemas_read: true,
            schemas: vec![copy],
            ..Catalog::default()
        });
        let path = vec![catalog.clone(), schema.name.clone(), object.kind.folder().to_owned(), object.key()];
        let options = DdlOptions { nested: true, comments: true, ..DdlOptions::default() };
        target_driver.ddl(&snapshot, &path, &options).ok()
    };
    let script = sync_script(&diff, (from, to), dialect, &create, &describe_note, &ScriptOptions { drop });
    Ok((script.text, script.statements, script.notes.len(), target.info.connection.clone()))
}

fn data_script_of(
    session: &mut Session,
    op: u64,
    drop: bool,
) -> Result<(String, usize, usize, Option<String>), CommandError> {
    let engine = engine(session);
    let state = engine.data_compare_state(op).ok_or_else(|| unknown(op))?;
    if state.status != OperationStatus::Done {
        return Err(CommandError::with_code("notFinished", t!("database-compare-error-not-finished")));
    }
    let rows = engine.data_compare_rows(op).unwrap_or_default();
    let target = find(session, &state.target)?;
    let dialect = dialect(driver(session, &target.source.driver)?.as_ref())?;
    let script = data_script(dialect, &state.target_table, (&state.key, &state.columns), &rows, drop);
    let mut text = script.text;
    if script.not_deleted > 0 {
        text.push_str(&format!("-- {}\n", t!("database-compare-note-rows-not-deleted", count = script.not_deleted)));
    }
    if state.truncated {
        text.push_str(&format!("-- {}\n", t!("database-compare-note-rows-truncated", kept = state.kept)));
    }
    Ok((text, script.statements, script.not_deleted, Some(state.target)))
}

pub struct CompareScript;

impl Command for CompareScript {
    type Params = ScriptParams;
    type Output = ScriptAnswer;
    const ID: &'static str = "db.compare.script";
    const TITLE: &'static str = "Compare Script";
    const DESCRIPTION: &'static str = "Writes the script that makes the target of finished comparison `id` like its source, in the target's dialect: for schemas the DDL (drops, new types, sequences and tables, column changes, keys, indexes, triggers, views and routines, foreign keys last); for data the INSERT, UPDATE and DELETE statements of the rows kept. What only the target has is dropped or deleted only with `drop`, else it is a `--` note, as is what the dialect cannot change in place. Never runs it: answers the text (at most `limit`, 100 000 characters) or, with `open`, opens it in a new console of the target's connection for a person to review and run (production safety applies there).";

    fn run(&self, session: &mut Session, params: ScriptParams) -> Result<ScriptAnswer, CommandError> {
        let job = match session.state::<Compares>().and_then(|compares| compares.jobs.get(&params.id)) {
            Some(Job::Schema(_)) => None,
            Some(Job::Data(op)) => Some(*op),
            None => return Err(unknown(params.id)),
        };
        let (text, statements, notes, connection) = match job {
            None => schema_script(session, params.id, params.drop)?,
            Some(op) => data_script_of(session, op, params.drop)?,
        };
        if params.open {
            let opened = NewConsole.run(session, NewConsoleParams { connection, text: Some(text) })?;
            return Ok(ScriptAnswer { text: None, statements, notes, console: Some(opened.path), truncated: false });
        }
        let (text, truncated) = cut(text, params.limit.unwrap_or(TEXT_ANSWERED));
        Ok(ScriptAnswer { text: Some(text), statements, notes, console: None, truncated })
    }
}

/// What the Compare tab starts with.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenParams {
    /// The source connection.
    pub source: Option<String>,
    /// The source schema.
    pub source_schema: Option<String>,
    /// The target connection.
    pub target: Option<String>,
    /// The target schema.
    pub target_schema: Option<String>,
    /// A comparison to show.
    pub id: Option<u64>,
}

pub struct OpenCompare;

impl Command for OpenCompare {
    type Params = OpenParams;
    type Output = InnerTab;
    const ID: &'static str = "db.compare.open";
    const TITLE: &'static str = "Compare Schemas and Data";
    const DESCRIPTION: &'static str = "Opens the Compare tab of the Database workspace (or brings it to front) and expands the workspace: pick a source and a target (a connection's schema or a saved snapshot), compare them, read the differences as a tree with each object's DDL side by side, compare a table's rows, and open the script in a console. `source`, `sourceSchema`, `target`, `targetSchema` preset its pickers; `id` shows a comparison. Connects nowhere.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<InnerTab, CommandError> {
        let existing = session
            .state::<Workspaces>()
            .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|tab| tab.id == TAB).cloned());
        let mut data =
            existing.map(|tab| tab.data).filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
        for (field, value) in [
            ("source", params.source),
            ("sourceSchema", params.source_schema),
            ("target", params.target),
            ("targetSchema", params.target_schema),
        ] {
            if let Some(value) = value {
                data[field] = serde_json::Value::String(value);
            }
        }
        if let Some(id) = params.id {
            data["id"] = serde_json::json!(id);
        }
        let open = OpenTabParams {
            workspace: WORKSPACE.to_owned(),
            id: Some(TAB.to_owned()),
            kind: Some(TAB.to_owned()),
            title: Some(t!("database-compare-title")),
            data: Some(data),
            ..OpenTabParams::default()
        };
        OpenTab.run(session, open)
    }
}

/// Which schema to save.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveParams {
    /// The connection's id.
    pub connection: String,
    /// The database (catalogue); the one holding `schema` when absent.
    pub catalog: Option<String>,
    /// The schema.
    pub schema: String,
    /// The file; `<data folder>/databases/snapshots/<connection>-<schema>-<timestamp>.json` when absent.
    pub path: Option<PathBuf>,
    /// Replace a file that exists; refused with code `exists` otherwise.
    pub overwrite: bool,
}

/// A snapshot saved.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// The file.
    pub path: PathBuf,
    /// How many objects it holds.
    pub objects: usize,
    /// How many of them were saved by name only, their details not read.
    pub partial: usize,
}

pub(super) fn file_part(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect::<String>()
        .trim_start_matches('.')
        .to_owned()
}

fn freshest_read(session: &Session, connection: &str, schema: &str) -> Option<Snapshot> {
    let compares = session.state::<Compares>()?;
    compares
        .jobs
        .values()
        .filter_map(|job| match job {
            Job::Schema(job) => Some(job),
            Job::Data(_) => None,
        })
        .filter_map(|job| {
            let progress = job.progress();
            let sides = progress.sides.as_ref()?;
            [&sides.0, &sides.1]
                .into_iter()
                .find(|read| {
                    !read.info.cached
                        && read.info.connection.as_deref() == Some(connection)
                        && read.info.schema == schema
                        && read.snapshot.connection == connection
                })
                .map(|read| read.snapshot.clone())
        })
        .max_by_key(|snapshot| snapshot.taken)
}

pub struct SaveSnapshot;

impl Command for SaveSnapshot {
    type Params = SaveParams;
    type Output = Saved;
    const ID: &'static str = "db.compare.saveSnapshot";
    const TITLE: &'static str = "Save Schema Snapshot";
    const DESCRIPTION: &'static str = "Saves schema `schema` of connection `connection`, as last read (by the snapshot in the IDE's caches or a comparison of this window, whichever is newer; refresh it first with db.schema.refresh), to a JSON file: its objects with columns, keys, indexes, triggers and definitions, never data values or secrets. The file goes to `path` or `<data folder>/databases/snapshots/`; an existing file is refused with code `exists` unless `overwrite`. Compare with it later through `{ kind: snapshot, path }`. Connects nowhere.";

    fn run(&self, session: &mut Session, params: SaveParams) -> Result<Saved, CommandError> {
        let entry = find(session, &params.connection)?;
        let mut snapshot = super::structure::snapshot_of(session, &entry);
        if let Some(read) = freshest_read(session, &params.connection, &params.schema)
            && (read.taken > snapshot.taken
                || find_schema(&snapshot, params.catalog.as_deref(), &params.schema).is_none())
        {
            snapshot = read;
        }
        let (catalog, schema) = find_schema(&snapshot, params.catalog.as_deref(), &params.schema).ok_or_else(|| {
            CommandError::with_code(
                "unknownSchema",
                t!(
                    "database-compare-error-no-schema",
                    schema = params.schema.as_str(),
                    connection = entry.source.name.as_str()
                ),
            )
        })?;
        if schema.level == zen_db::schema::Level::NotRead {
            return Err(CommandError::with_code(
                "notRead",
                t!("database-compare-error-not-read", schema = schema.name.as_str()),
            ));
        }
        let path = match params.path {
            Some(path) => path,
            None => {
                let folder =
                    snapshots_folder(session).ok_or_else(|| CommandError::new(t!("database-error-no-data-folder")))?;
                let (_, stamp) = zen_db::engine::now_stamps();
                folder.join(format!("{}-{}-{stamp}.json", file_part(&entry.source.name), file_part(&schema.name)))
            }
        };
        super::guard_agent_file(session, &path)?;
        if path.exists() && !params.overwrite {
            return Err(CommandError::with_code(
                "exists",
                t!("database-error-export-exists", count = 1, path = crate::display(&path)),
            ));
        }
        let saved = SavedSchema {
            version: SNAPSHOT_VERSION,
            connection: entry.source.id.clone(),
            connection_name: entry.source.name.clone(),
            driver: entry.source.driver.clone(),
            catalog: catalog.name.clone(),
            taken: if snapshot.taken == 0 { now() } else { snapshot.taken },
            schema: schema.clone(),
        };
        zen_db::registry::write_json(&path, &saved)
            .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
        let partial = schema.objects.iter().filter(|object| !object.detailed).count();
        Ok(Saved { path, objects: schema.objects.len(), partial })
    }
}

/// Which snapshots to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SnapshotsParams {
    /// The most snapshots to list; 50 when absent.
    pub limit: Option<usize>,
}

/// A saved snapshot.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    /// The file.
    pub path: PathBuf,
    /// The connection's name it was saved from.
    pub connection: String,
    /// The schema.
    pub schema: String,
    /// When its structure was read, in seconds since 1970.
    pub taken: u64,
    /// How many objects it holds.
    pub objects: usize,
}

/// The saved snapshots.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotList {
    /// The snapshots, the newest first, at most `limit`.
    pub snapshots: Vec<SnapshotEntry>,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct ListSnapshots;

impl Command for ListSnapshots {
    type Params = SnapshotsParams;
    type Output = SnapshotList;
    const ID: &'static str = "db.compare.snapshots";
    const TITLE: &'static str = "Schema Snapshots";
    const DESCRIPTION: &'static str = "Lists the schema snapshots saved with db.compare.saveSnapshot in `<data folder>/databases/snapshots/`, the newest first (at most `limit`, 50, with `total` and `truncated`): each file, the connection's name, the schema, when it was read and how many objects it holds.";

    fn run(&self, session: &mut Session, params: SnapshotsParams) -> Result<SnapshotList, CommandError> {
        let mut found = Vec::new();
        let closed =
            if zen_core::macros::by_agent(session) { super::agent_access::closed(session) } else { Default::default() };
        if let Some(folder) = snapshots_folder(session) {
            for file in std::fs::read_dir(folder).into_iter().flatten().flatten() {
                let path = file.path();
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                let Ok(saved) = read_saved(&path) else { continue };
                if closed.contains(&saved.connection) {
                    continue;
                }
                found.push(SnapshotEntry {
                    path,
                    connection: saved.connection_name,
                    schema: saved.schema.name.clone(),
                    taken: saved.taken,
                    objects: saved.schema.objects.len(),
                });
            }
        }
        found.sort_by(|a, b| b.taken.cmp(&a.taken).then_with(|| a.path.cmp(&b.path)));
        let total = found.len();
        let limit = params.limit.unwrap_or(50);
        found.truncate(limit);
        Ok(SnapshotList { snapshots: found, total, truncated: total > limit })
    }
}

#[cfg(test)]
mod tests;
