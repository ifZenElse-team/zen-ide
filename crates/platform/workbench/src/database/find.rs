use std::path::PathBuf;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{DbError, ErrorKind};
use zen_db::engine::{ConnectionState, SearchRequest, SearchState, TableHit, TableStatus};
use zen_db::registry::Entry;
use zen_db::schema::{ObjectKind, Snapshot};
use zen_db::search::{ColumnClass, catalog_query, filter_condition};
use zen_i18n::t;

use super::connections::describe;
use super::consoles::{ExecuteScript, RunStarted, ScriptParams, engine, ensure_connected};
use super::objects::{self, OpenParams, Page};
use super::sources::{Database, find};
use super::structure::{Structure, ensure};
use super::{NewConsole, NewConsoleParams, WORKSPACE};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};

pub const TAB: &str = "find";

const LISTED: usize = 50;
const MOST_LISTED: usize = 5000;
const LIMIT: usize = 100;
const MOST_LIMIT: usize = 10_000;
const TABLE_TIMEOUT_MS: u64 = 10_000;
const MOST_TABLE_TIMEOUT_MS: u64 = 600_000;
const MOST_WAIT_MS: u64 = 60_000;
const LARGE_SCHEMA_ROWS: i64 = 1_000_000;
const SCHEMAS_LISTED: usize = 500;

#[derive(Default)]
pub struct Searches {
    current: Option<u64>,
}

impl Searches {
    pub fn current(&self) -> Option<u64> {
        self.current
    }
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        _ => error.code.as_deref().unwrap_or("databaseError"),
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn dialect_of(session: &mut Session, entry: &Entry) -> Result<SqlDialect, CommandError> {
    super::filters::dialect_of(session, &entry.source.id).ok_or_else(|| {
        CommandError::with_code("notSql", t!("database-error-search-not-sql", name = entry.source.name.as_str()))
    })
}

fn snapshot<'a>(session: &'a mut Session, entry: &Entry) -> Option<&'a Snapshot> {
    ensure(session, entry, false);
    session.state::<Structure>().and_then(|structure| structure.snapshot(&entry.source.id))
}

fn schemas_of(snapshot: &Snapshot) -> Vec<String> {
    if snapshot.catalogs.is_empty() {
        return snapshot.cached(&[]).unwrap_or_default().iter().map(|node| node.label.clone()).collect();
    }
    let catalog = snapshot.catalogs.iter().find(|catalog| catalog.current).or_else(|| snapshot.catalogs.first());
    let mut schemas: Vec<(bool, String)> = catalog
        .map(|catalog| catalog.schemas.iter().map(|schema| (schema.system, schema.name.clone())).collect())
        .unwrap_or_default();
    schemas.sort();
    schemas.into_iter().map(|(_, name)| name).collect()
}

fn default_schema(entry: &Entry, schemas: &[String]) -> Option<String> {
    let has = |name: &str| schemas.iter().any(|known| known == name);
    match entry.source.driver.as_str() {
        "postgresql" if has("public") => Some("public".to_owned()),
        "sqlite" => Some("main".to_owned()),
        "mysql" | "mariadb" | "clickhouse" => {
            entry.source.database.clone().filter(|database| !database.is_empty()).or_else(|| schemas.first().cloned())
        }
        _ => schemas.first().cloned(),
    }
}

fn estimated_rows(snapshot: &Snapshot, schema: &str) -> Option<i64> {
    let catalog = snapshot.catalogs.iter().find(|catalog| catalog.current).or_else(|| snapshot.catalogs.first())?;
    let schema = catalog.schemas.iter().find(|known| known.name == schema)?;
    let rows: Vec<i64> = schema
        .objects
        .iter()
        .filter(|object| matches!(object.kind, ObjectKind::Table | ObjectKind::MaterializedView))
        .filter_map(|object| object.rows.filter(|rows| *rows > 0))
        .collect();
    (!rows.is_empty()).then(|| rows.iter().sum())
}

/// Something to know before a search: why it may load the server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    /// `production` (a connection whose type confirms SQL execution), `largeSchema` (more than a million rows by the server's estimates) or `views` (views run their queries).
    pub code: String,
    /// What it means, in the IDE's language.
    pub message: String,
}

fn warnings(session: &mut Session, entry: &Entry, schema: &str, views: bool) -> (Vec<Warning>, Option<i64>) {
    let mut found = Vec::new();
    let kind = super::safety::type_of(session, entry);
    if kind.confirm_execute {
        found.push(Warning {
            code: "production".to_owned(),
            message: t!("database-find-warning-production", kind = kind.name.as_str()),
        });
    }
    let rows = snapshot(session, entry).and_then(|snapshot| estimated_rows(snapshot, schema));
    if let Some(rows) = rows.filter(|rows| *rows > LARGE_SCHEMA_ROWS) {
        found.push(Warning {
            code: "largeSchema".to_owned(),
            message: t!("database-find-warning-large", rows = rows, schema = schema),
        });
    }
    if views {
        found.push(Warning { code: "views".to_owned(), message: t!("database-find-warning-views") });
    }
    (found, rows)
}

/// Where to search.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TargetsParams {
    /// The connection's id.
    pub connection: String,
    /// The schema the warnings are about; the default schema when absent.
    pub schema: Option<String>,
    /// Whether views would be searched, for the warnings.
    pub views: bool,
}

/// Where a connection can be searched, from its structure as known now.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Targets {
    /// The connection.
    pub connection: String,
    /// Whether its driver speaks SQL, which searching needs.
    pub sql: bool,
    /// Whether it is connected now.
    pub connected: bool,
    /// Its schemas as the navigator knows them, user schemas first; empty before its structure was read.
    pub schemas: Vec<String>,
    /// Whether schemas were left out (more than 500).
    pub truncated: bool,
    /// The schema a search uses when none is given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The rows of the schema's tables by the server's estimates, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_rows: Option<i64>,
    /// Why a search may load the server.
    pub warnings: Vec<Warning>,
}

pub struct SearchTargets;

impl Command for SearchTargets {
    type Params = TargetsParams;
    type Output = Targets;
    const ID: &'static str = "db.search.targets";
    const TITLE: &'static str = "Find in Database Targets";
    const DESCRIPTION: &'static str = "Where Find in Database can look on connection `connection`: whether its driver speaks SQL, its schemas as the navigator knows them (user schemas first, at most 500, with `truncated`), the default schema, the rows of `schema`'s tables by the server's estimates, and warnings about load (`production`, `largeSchema`, `views` when `views`). Reads the kept structure only; never connects.";

    fn run(&self, session: &mut Session, params: TargetsParams) -> Result<Targets, CommandError> {
        let entry = find(session, &params.connection)?;
        let sql = super::filters::dialect_of(session, &entry.source.id).is_some();
        let connected = session.state::<Database>().and_then(Database::started).is_some_and(|engine| {
            matches!(engine.status(&entry.source.id).state, ConnectionState::Connected | ConnectionState::Closed)
        });
        let mut schemas = snapshot(session, &entry).map(schemas_of).unwrap_or_default();
        let default = default_schema(&entry, &schemas);
        let schema = params.schema.clone().filter(|schema| !schema.is_empty()).or_else(|| default.clone());
        let (warnings, estimated_rows) = match &schema {
            Some(schema) => warnings(session, &entry, schema, params.views),
            None => (Vec::new(), None),
        };
        let truncated = schemas.len() > SCHEMAS_LISTED;
        schemas.truncate(SCHEMAS_LISTED);
        Ok(Targets {
            connection: entry.source.id,
            sql,
            connected,
            schemas,
            truncated,
            schema: default,
            estimated_rows,
            warnings,
        })
    }
}

/// What the Find in Database tab opens with.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenSearchParams {
    /// The connection to search.
    pub connection: Option<String>,
    /// The schema to search.
    pub schema: Option<String>,
    /// The text to find; never kept across restarts.
    pub text: Option<String>,
}

pub struct OpenSearch;

impl Command for OpenSearch {
    type Params = OpenSearchParams;
    type Output = InnerTab;
    const ID: &'static str = "db.search.open";
    const TITLE: &'static str = "Find in Database";
    const DESCRIPTION: &'static str = "Opens the Find in Database tab of the Database workspace (or brings it to front) and expands the workspace; `connection`, `schema` and `text` fill its form. Starts nothing (db.search.start does) and connects nowhere.";

    fn run(&self, session: &mut Session, params: OpenSearchParams) -> Result<InnerTab, CommandError> {
        if let Some(connection) = &params.connection {
            find(session, connection)?;
        }
        let existing = session
            .state::<Workspaces>()
            .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|tab| tab.id == TAB).cloned());
        let mut data =
            existing.map(|tab| tab.data).filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
        if let Some(connection) = params.connection {
            data["connection"] = serde_json::Value::String(connection);
            if params.schema.is_none() {
                data["schema"] = serde_json::Value::Null;
            }
        }
        if let Some(schema) = params.schema {
            data["schema"] = serde_json::Value::String(schema);
        }
        let mut tab = OpenTab.run(
            session,
            OpenTabParams {
                workspace: WORKSPACE.to_owned(),
                id: Some(TAB.to_owned()),
                kind: Some(TAB.to_owned()),
                title: Some(t!("database-find-title")),
                data: Some(data),
                ..OpenTabParams::default()
            },
        )?;
        if let Some(text) = params.text {
            session.state_mut::<Preset>().text = Some((text.clone(), preset_serial(session)));
            tab.data["text"] = serde_json::Value::String(text);
        }
        Ok(tab)
    }
}

#[derive(Default)]
pub struct Preset {
    text: Option<(String, u64)>,
    serial: u64,
}

impl Preset {
    pub fn take(&mut self) -> Option<(String, u64)> {
        self.text.take()
    }
}

fn preset_serial(session: &mut Session) -> u64 {
    let preset = session.state_mut::<Preset>();
    preset.serial += 1;
    preset.serial
}

fn default_true() -> bool {
    true
}

/// What to find and where.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartParams {
    /// The connection's id.
    pub connection: String,
    /// The schema whose tables are searched; the default schema (db.search.targets) when absent.
    #[serde(default)]
    pub schema: Option<String>,
    /// The text to find, as a part of a value or a name.
    pub text: String,
    /// Look in the rows; true when absent.
    #[serde(default = "default_true")]
    pub data: bool,
    /// Look in the names of tables and columns.
    #[serde(default)]
    pub metadata: bool,
    /// Letters must match in case.
    #[serde(default)]
    pub case_sensitive: bool,
    /// The kinds of columns searched: `text`, `number`, `temporal`, `other`; `[text]` when absent.
    #[serde(default)]
    pub column_kinds: Option<Vec<ColumnClass>>,
    /// Only columns with these names, in any case; every column of the chosen kinds when empty.
    #[serde(default)]
    pub columns: Vec<String>,
    /// Only tables with these names, in any case; every table when empty.
    #[serde(default)]
    pub tables: Vec<String>,
    /// Search views too; each runs its query.
    #[serde(default)]
    pub views: bool,
    /// The most rows counted per table; 100 when absent, at most 10 000.
    #[serde(default)]
    pub limit_per_table: Option<usize>,
    /// How long one table may take, in milliseconds, before it is stopped and the search goes on; 10 000 when absent, at most 600 000.
    #[serde(default)]
    pub table_timeout_ms: Option<u64>,
}

/// A search started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The search's id, for db.search.state, db.search.cancel and db.search.openResult.
    pub id: u64,
    /// The connection.
    pub connection: String,
    /// The schema searched.
    pub schema: String,
    /// Whether the connection was connected first.
    pub connecting: bool,
    /// The search it replaced, cancelled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<u64>,
    /// Why it may load the server.
    pub warnings: Vec<Warning>,
}

pub struct StartSearch;

impl Command for StartSearch {
    type Params = StartParams;
    type Output = Started;
    const ID: &'static str = "db.search.start";
    const TITLE: &'static str = "Start Find in Database";
    const DESCRIPTION: &'static str = "Find in Database: looks for `text` in the tables of `schema` (the default schema when absent) of connection `connection`, on the server, in a read-only session of its own, in the background: the server's catalogue lists the tables (views only with `views`; foreign tables never), then each table is read with a generated query in the driver's dialect that counts, per column, the rows whose `columnKinds` columns (`text` by default; `number`, `temporal`, `other`), or only `columns` by name, contain the text (case-insensitive unless `caseSensitive`), at most `limitPerTable` rows (100) and `tableTimeoutMs` (10 000) per table; a table past its deadline is stopped and the search goes on. `metadata` also matches the names of tables and columns; `data: false` only names. `tables` limits it to some tables. Connects first when needed with the stored password; one search per window, so a running one is cancelled. Answers the id at once and `warnings` about load (`production`, `largeSchema`, `views`); follow it with db.search.state. Never reads or answers values.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<Started, CommandError> {
        if params.text.trim().is_empty() {
            return Err(CommandError::with_code("noText", t!("database-error-search-no-text")));
        }
        if !params.data && !params.metadata {
            return Err(CommandError::with_code("nothingToSearch", t!("database-error-search-nothing")));
        }
        let entry = find(session, &params.connection)?;
        let dialect = dialect_of(session, &entry)?;
        let schema = match params.schema.clone().filter(|schema| !schema.trim().is_empty()) {
            Some(schema) => schema,
            None => {
                let schemas = snapshot(session, &entry).map(schemas_of).unwrap_or_default();
                default_schema(&entry, &schemas)
                    .ok_or_else(|| CommandError::with_code("noSchema", t!("database-error-search-no-schema")))?
            }
        };
        super::safety::agent_reads(session, &entry, &catalog_query(dialect, &schema))?;
        let classes = params.column_kinds.clone().filter(|kinds| !kinds.is_empty()).unwrap_or(vec![ColumnClass::Text]);
        let limit = params.limit_per_table.unwrap_or(LIMIT).clamp(1, MOST_LIMIT);
        let timeout = params.table_timeout_ms.unwrap_or(TABLE_TIMEOUT_MS).clamp(100, MOST_TABLE_TIMEOUT_MS);
        let (warnings, _) = warnings(session, &entry, &schema, params.views);
        let connecting = ensure_connected(session, &entry.source.id)?;
        let engine = engine(session);
        let replaced = session.state::<Searches>().and_then(|searches| searches.current);
        let cancelled = replaced.filter(|id| engine.cancel(*id));
        let request = SearchRequest {
            connection: entry.source.id.clone(),
            schema: schema.clone(),
            text: params.text.clone(),
            case_sensitive: params.case_sensitive,
            data: params.data,
            metadata: params.metadata,
            classes: classes.clone(),
            columns: params.columns.iter().filter(|name| !name.trim().is_empty()).cloned().collect(),
            tables: params.tables.iter().filter(|name| !name.trim().is_empty()).cloned().collect(),
            views: params.views,
            limit,
            table_timeout: Duration::from_millis(timeout),
            dialect,
        };
        let provenance = super::history::provenance(session, zen_db::history::Source::Search, Some(schema.clone()));
        let id = engine.search(request, provenance).map_err(|error| failed(&error))?;
        session.state_mut::<Searches>().current = Some(id);
        remember_form(session, &entry.source.id, &schema, &params, (&classes, limit, timeout));
        Ok(Started { id, connection: entry.source.id, schema, connecting, cancelled, warnings })
    }
}

fn remember_form(
    session: &mut Session,
    connection: &str,
    schema: &str,
    params: &StartParams,
    (classes, limit, timeout): (&[ColumnClass], usize, u64),
) {
    let open = session
        .state::<Workspaces>()
        .is_some_and(|workspaces| workspaces.tabs(WORKSPACE).iter().any(|tab| tab.id == TAB));
    if !open {
        return;
    }
    let data = serde_json::json!({
        "connection": connection,
        "schema": schema,
        "data": params.data,
        "metadata": params.metadata,
        "caseSensitive": params.case_sensitive,
        "columnKinds": classes,
        "columns": params.columns,
        "views": params.views,
        "limitPerTable": limit,
        "tableTimeoutMs": timeout,
    });
    session.state_mut::<Workspaces>().set_kind_data(WORKSPACE, TAB, data);
}

/// Which search to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// The search; the window's last one when absent.
    pub id: Option<u64>,
    /// How long to wait for it to end, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
    /// The most tables to list; 50 when absent, at most 5 000.
    pub limit: Option<usize>,
    /// List every table of the schema, also those without the text; only those with the text, stopped or failed when absent.
    pub all: bool,
}

/// A search as agents read it; never values.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchAnswer {
    /// The search with the tables listed; absent when the window has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchState>,
    /// How many tables the search has in all.
    pub tables: usize,
    /// Whether tables were left out because of `limit`.
    pub truncated: bool,
}

fn listed(table: &TableHit, all: bool) -> bool {
    all || table.found() || matches!(table.status, TableStatus::Timeout | TableStatus::Failed)
}

pub struct SearchStateCommand;

impl Command for SearchStateCommand {
    type Params = StateParams;
    type Output = SearchAnswer;
    const ID: &'static str = "db.search.state";
    const TITLE: &'static str = "Find in Database State";
    const DESCRIPTION: &'static str = "The progress of search `id` (the window's last one when absent; waiting up to `waitMs`, 60 000 at most, for it to end): its status (running, done, failed, cancelled), the table being searched, how many tables ended, rows found in all, and its tables in name order (only those with the text, stopped at their deadline or failed unless `all`; at most `limit`, 50, with `truncated`), each with its status (pending, running, done, timeout, failed, skipped, cancelled), rows with the text up to the limit (`truncated` when there may be more), the columns that have it with their counts, and with `metadata` whether its name or its columns' names match. Never values.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<SearchAnswer, CommandError> {
        let id = params.id.or_else(|| session.state::<Searches>().and_then(|searches| searches.current));
        let Some(id) = id else {
            return Ok(SearchAnswer { search: None, tables: 0, truncated: false });
        };
        let engine = engine(session);
        if let Some(wait) = params.wait_ms {
            engine.wait(id, Duration::from_millis(wait.min(MOST_WAIT_MS)));
        }
        let mut state = engine
            .search_state(id)
            .ok_or_else(|| CommandError::with_code("unknownSearch", t!("database-error-unknown-search", id = id)))?;
        let tables = state.tables.len();
        let limit = params.limit.unwrap_or(LISTED).min(MOST_LISTED);
        let shown: Vec<TableHit> = state.tables.drain(..).filter(|table| listed(table, params.all)).collect();
        let truncated = shown.len() > limit;
        state.tables = shown.into_iter().take(limit).collect();
        Ok(SearchAnswer { search: Some(state), tables, truncated })
    }
}

/// Which search to cancel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CancelParams {
    /// The search; the window's last one when absent.
    pub id: Option<u64>,
}

/// Whether a search was cancelled.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Whether it was running and is being cancelled.
    pub cancelled: bool,
}

pub struct CancelSearch;

impl Command for CancelSearch {
    type Params = CancelParams;
    type Output = Cancelled;
    const ID: &'static str = "db.search.cancel";
    const TITLE: &'static str = "Cancel Find in Database";
    const DESCRIPTION: &'static str = "Cancels search `id` (the window's last one when absent): the table being read stops on the server and the tables not yet searched are left. What was found stays readable with db.search.state. Answers whether it was running.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<Cancelled, CommandError> {
        let id = params.id.or_else(|| session.state::<Searches>().and_then(|searches| searches.current));
        let Some(id) = id else { return Ok(Cancelled { cancelled: false }) };
        let engine = engine(session);
        Ok(Cancelled { cancelled: engine.cancel(id) })
    }
}

/// Which table of a search to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenResultParams {
    /// The search; the window's last one when absent.
    pub id: Option<u64>,
    /// The table's name, as db.search.state lists it.
    pub table: String,
    /// Filter by these of its columns that have the text; every one that has it when empty.
    pub columns: Vec<String>,
}

/// A table of a search opened.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenedResult {
    /// The object tab it opened in, when the navigator knows the table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<InnerTab>,
    /// The filter the rows are read with; absent when only names matched and the table opened unfiltered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// The query reading the rows, for db.query.state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// The console it opened in instead, when the navigator does not know the table or the driver has its own tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The run of that console's query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunStarted>,
}

fn object_path(snapshot: &Snapshot, schema: &str, table: &str, kind: ObjectKind) -> Option<Vec<String>> {
    let mut catalogs: Vec<_> = snapshot.catalogs.iter().collect();
    catalogs.sort_by_key(|catalog| !catalog.current);
    catalogs.into_iter().find_map(|catalog| {
        let known = catalog.schemas.iter().find(|known| known.name == schema)?;
        let object = known
            .objects
            .iter()
            .find(|object| object.name == table && object.kind == kind)
            .or_else(|| known.objects.iter().find(|object| object.name == table && object.kind.has_columns()))?;
        Some(vec![catalog.name.clone(), schema.to_owned(), object.kind.folder().to_owned(), table.to_owned()])
    })
}

pub struct OpenSearchResult;

impl Command for OpenSearchResult {
    type Params = OpenResultParams;
    type Output = OpenedResult;
    const ID: &'static str = "db.search.openResult";
    const TITLE: &'static str = "Open Find in Database Result";
    const DESCRIPTION: &'static str = "Opens table `table` of search `id` (the window's last one when absent): as an object tab on its Data page filtered (WHERE filter, db.result.filter) to the rows where one of the columns that have the text (or `columns` of them) contains it, read in the background; the filter goes into the table's filter history and Back returns to the one before. A table found only by its names opens unfiltered. When the navigator does not know the table (its structure not read since it was created, or a driver with its own tree such as ClickHouse) a new console runs the filtered SELECT instead. Connects first when needed.";

    fn run(&self, session: &mut Session, params: OpenResultParams) -> Result<OpenedResult, CommandError> {
        let id = params
            .id
            .or_else(|| session.state::<Searches>().and_then(|searches| searches.current))
            .ok_or_else(|| CommandError::with_code("noSearch", t!("database-error-no-search")))?;
        let engine = engine(session);
        let state = engine
            .search_state(id)
            .ok_or_else(|| CommandError::with_code("unknownSearch", t!("database-error-unknown-search", id = id)))?;
        let hit = state.tables.iter().find(|table| table.table == params.table).cloned().ok_or_else(|| {
            CommandError::with_code(
                "unknownTable",
                t!("database-error-search-unknown-table", table = params.table.as_str()),
            )
        })?;
        let entry = find(session, &state.connection)?;
        let dialect = dialect_of(session, &entry)?;
        let found: Vec<String> = hit.columns.iter().map(|column| column.name.clone()).collect();
        let columns: Vec<String> = if params.columns.is_empty() {
            found
        } else {
            params.columns.iter().filter(|name| found.contains(name)).cloned().collect()
        };
        if !params.columns.is_empty() && columns.is_empty() {
            return Err(CommandError::with_code("noMatchingColumns", t!("database-error-search-no-columns")));
        }
        let condition =
            (!columns.is_empty()).then(|| filter_condition(dialect, &columns, &state.text, state.case_sensitive));
        let path =
            snapshot(session, &entry).and_then(|snapshot| object_path(snapshot, &state.schema, &hit.table, hit.kind));
        let Some(path) = path else {
            return open_in_console(session, &entry, dialect, (&state, &hit), condition);
        };
        let open = |session: &mut Session| -> Result<OpenedResult, CommandError> {
            let page = objects::has_data(objects::kind_name(hit.kind)).then_some(Page::Data);
            let opened = objects::ObjectOpen
                .run(session, OpenParams { connection: entry.source.id.clone(), path: path.clone(), page })?;
            let query = match (opened.query, page) {
                (Some(query), _) => Some(query),
                (None, Some(Page::Data)) => objects::read_tab(session, &opened.tab.id)?.query,
                (None, _) => None,
            };
            Ok(OpenedResult { tab: Some(opened.tab), condition: None, query, path: None, run: None })
        };
        let Some(condition) = condition else { return open(session) };
        let tab = objects::tab_id(&entry.source.id, &path);
        let table = format!("{}|{}", entry.source.id, zen_db::schema::qualified(&state.schema, &hit.table));
        let mut opened = super::filters::filter_tab(session, (&tab, &table), &condition, open)?;
        opened.condition = Some(condition);
        Ok(opened)
    }
}

fn open_in_console(
    session: &mut Session,
    entry: &Entry,
    dialect: SqlDialect,
    (state, hit): (&SearchState, &TableHit),
    condition: Option<String>,
) -> Result<OpenedResult, CommandError> {
    let table = dialect.qualified(&state.schema, &hit.table);
    let text = match &condition {
        Some(condition) => format!("SELECT * FROM {table} WHERE {condition};\n"),
        None => format!("SELECT * FROM {table};\n"),
    };
    let opened =
        NewConsole.run(session, NewConsoleParams { connection: Some(entry.source.id.clone()), text: Some(text) })?;
    let run =
        ExecuteScript.run(session, ScriptParams { path: Some(opened.path.clone()), ..ScriptParams::default() })?;
    Ok(OpenedResult { tab: Some(opened.tab), condition, query: None, path: Some(opened.path), run: Some(run) })
}

#[cfg(test)]
mod tests;
