mod introspect;
pub mod process;
mod quote;
mod statement;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::dialect::{SqlDialect, table_id};
use crate::driver::{
    BoxFuture, Canceller, ConnectSpec, DbError, Driver, DriverSession, ErrorKind, ExecOptions, Param, RowSink,
    StreamEnd, StreamOptions, TrackedState,
};
use crate::model::{Capabilities, DataModel, Node, NodeKind, SettingField, StatementKind};
use crate::schema::{DdlOptions, IntrospectRequest, Introspection, Snapshot, TreeOptions};
use crate::value::{Column, ColumnSource, Execution, Notice};

pub const ID: &str = "sqlite";

pub const PROGRAM: &str = "program";

pub const NO_FILE: &str = "fileNotFound";

const BUSY_MS: u64 = 5000;

#[derive(Default)]
pub struct Sqlite;

fn capabilities() -> &'static Capabilities {
    static CAPABILITIES: OnceLock<Capabilities> = OnceLock::new();
    CAPABILITIES.get_or_init(|| Capabilities {
        models: vec![DataModel::Relational],
        introspection: true,
        language: "sql:sqlite".to_owned(),
        transactions: true,
        isolation_levels: ["deferred", "immediate", "exclusive"].map(str::to_owned).to_vec(),
        catalogs: false,
        schemas: true,
        node_kinds: vec![
            NodeKind::Catalog,
            NodeKind::Schema,
            NodeKind::Folder,
            NodeKind::Table,
            NodeKind::View,
            NodeKind::Column,
            NodeKind::Index,
            NodeKind::Other("constraint".to_owned()),
            NodeKind::Other("foreignKey".to_owned()),
            NodeKind::Other("trigger".to_owned()),
            NodeKind::Other("reference".to_owned()),
        ],
        explain: true,
        server_cancel: cfg!(unix),
        bind_parameters: false,
        row_identity: Some("primaryKey".to_owned()),
        ddl_preview: false,
        server_paging: false,
        sessions: false,
        read_only_sessions: true,
        uri_schemes: vec!["sqlite".to_owned()],
        shared_connection: true,
        password: false,
        settings: vec![SettingField {
            key: PROGRAM.to_owned(),
            kind: "path".to_owned(),
            choices: Vec::new(),
            default: None,
        }],
    })
}

impl Driver for Sqlite {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "SQLite"
    }

    fn capabilities(&self) -> &Capabilities {
        capabilities()
    }

    fn default_port(&self) -> Option<u16> {
        None
    }

    fn classify(&self, statement: &str) -> StatementKind {
        crate::postgres::classify(statement)
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> BoxFuture<'a, Result<Box<dyn DriverSession>, DbError>> {
        Box::pin(async move { Ok(Box::new(Session::open(spec).await?) as Box<dyn DriverSession>) })
    }

    fn changes_structure(&self, statement: &str) -> bool {
        statement::prepare(statement).is_ok_and(|prepared| {
            matches!(
                prepared.words.first().map(String::as_str),
                Some("create" | "alter" | "drop" | "attach" | "detach" | "vacuum" | "reindex")
            )
        })
    }

    fn ddl(&self, snapshot: &Snapshot, path: &[String], options: &DdlOptions) -> Result<String, DbError> {
        introspect::ddl(snapshot, path, options.nested).ok_or_else(|| DbError::new(ErrorKind::Invalid, path.join("/")))
    }
    fn explain(
        &self,
        statement: &str,
        options: &crate::explain::ExplainOptions,
    ) -> Result<crate::explain::ExplainRequest, DbError> {
        Ok(crate::explain::sqlite(statement, options))
    }
}

pub fn database_path(database: Option<&str>) -> Result<String, DbError> {
    let path = database.map(str::trim).unwrap_or_default();
    if path.is_empty() || path == ":memory:" {
        return Ok(":memory:".to_owned());
    }
    if !Path::new(path).is_absolute() {
        return Err(DbError::new(ErrorKind::Invalid, format!("{path} is not an absolute path")));
    }
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(path.to_owned()),
        Ok(_) => Err(DbError::new(ErrorKind::Invalid, format!("{path} is not a file")).with_code(NO_FILE)),
        Err(_) => Err(DbError::new(ErrorKind::Invalid, format!("{path} does not exist")).with_code(NO_FILE)),
    }
}

fn storage_type(rows: &[Vec<crate::value::Value>], column: usize) -> String {
    let mut found: Option<&'static str> = None;
    for class in rows.iter().filter_map(|row| row.get(column)).filter_map(quote::storage_class) {
        match found {
            None => found = Some(class),
            Some(known) if known != class => return "ANY".to_owned(),
            Some(_) => {}
        }
    }
    found.unwrap_or_default().to_owned()
}

fn traced(
    statement: &str,
    names: &[String],
    rows: &[Vec<crate::value::Value>],
    declared: &HashMap<String, String>,
) -> Vec<Column> {
    let read = crate::filter::table_read(statement, SqlDialect::Sqlite);
    let source = read.as_ref().map(|read| ColumnSource {
        table: table_id(read.schema.as_deref().unwrap_or("main"), &read.table),
        position: 0,
    });
    let origins: Option<Vec<String>> = read.and_then(|read| match read.columns {
        None => Some(names.to_vec()),
        Some(columns) => (columns.len() == names.len()).then_some(columns),
    });
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let origin = origins.as_ref().and_then(|origins| origins.get(index)).cloned();
            let type_name = origin
                .as_ref()
                .and_then(|origin| declared.get(&origin.to_ascii_lowercase()))
                .filter(|declared| !declared.is_empty())
                .cloned()
                .unwrap_or_else(|| storage_type(rows, index));
            Column { source: origin.as_ref().and(source), origin, ..Column::new(name.clone(), type_name) }
        })
        .collect()
}

fn control_words(sql: &str) -> Vec<String> {
    sql.split_whitespace()
        .take(6)
        .map(|word| {
            let word = word.trim_end_matches(';');
            let unquoted = match word.as_bytes() {
                [b'"', .., b'"'] | [b'`', .., b'`'] | [b'\'', .., b'\''] | [b'[', .., b']'] => &word[1..word.len() - 1],
                _ => word,
            };
            unquoted.to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

fn queries(first: Option<&str>) -> bool {
    matches!(first, Some("select" | "with" | "values"))
}

struct Session {
    process: process::Process,
    catalog: String,
    state: TrackedState,
    begin: String,
    in_transaction: bool,
    savepoints: Vec<String>,
    opened_by_savepoint: bool,
    timeout: Option<Duration>,
    show_system: bool,
}

impl Session {
    async fn open(spec: &ConnectSpec) -> Result<Self, DbError> {
        let source = &spec.source;
        let program = process::find_program(source.settings.get(PROGRAM).and_then(serde_json::Value::as_str))?;
        let path = database_path(source.database.as_deref())?;
        let target = process::target(&path, source.read_only);
        let process = process::Process::start(&program, &target, source.read_only, BUSY_MS).await?;
        let catalog =
            Path::new(&path).file_name().map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned());
        let mut session = Session {
            process,
            catalog,
            state: TrackedState { auto_commit: true, ..TrackedState::default() },
            begin: "BEGIN".to_owned(),
            in_transaction: false,
            savepoints: Vec::new(),
            opened_by_savepoint: false,
            timeout: spec.guards.statement_timeout,
            show_system: source.navigator.show_system_objects,
        };
        if spec.state.read_only {
            session.plain("PRAGMA query_only = 1").await?;
        }
        for statement in &source.bootstrap.init_statements {
            session.run(statement, 0).await?;
        }
        session.apply_state(&spec.state.clone()).await?;
        Ok(session)
    }

    async fn send(&mut self, sql: &str, max_rows: usize) -> Result<process::Printed, DbError> {
        let Some(limit) = self.timeout else { return self.process.run(sql, max_rows).await };
        let interrupt = self.process.interrupt();
        let run = self.process.run(sql, max_rows);
        tokio::pin!(run);
        tokio::select! {
            printed = &mut run => printed,
            () = tokio::time::sleep(limit) => {
                interrupt.send();
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut run).await;
                Err(DbError::new(ErrorKind::Timeout, ""))
            }
        }
    }

    async fn declared_types(&mut self, sql: &str) -> Result<HashMap<String, String>, DbError> {
        let Some(read) = crate::filter::table_read(sql, SqlDialect::Sqlite) else { return Ok(HashMap::new()) };
        let literal = |text: &str| format!("'{}'", text.replace('\'', "''"));
        let query = format!(
            "SELECT name, type FROM pragma_table_info({}, {})",
            literal(&read.table),
            literal(read.schema.as_deref().unwrap_or("main"))
        );
        let printed = self.send(&query, 10_000).await?;
        Ok(printed
            .rows
            .iter()
            .filter_map(|row| match (row.first(), row.get(1)) {
                (Some(crate::value::Value::Text(name)), Some(crate::value::Value::Text(declared))) => {
                    Some((name.to_ascii_lowercase(), declared.clone()))
                }
                _ => None,
            })
            .collect())
    }

    async fn begin_if_manual(&mut self, first: Option<&str>) -> Result<(), DbError> {
        let control = matches!(first, Some("begin" | "commit" | "end" | "rollback" | "release"));
        if !self.state.auto_commit && !self.in_transaction && !control {
            let begin = self.begin.clone();
            self.plain(&begin).await?;
            self.in_transaction = true;
        }
        Ok(())
    }

    fn ended(&mut self) {
        self.in_transaction = false;
        self.savepoints.clear();
        self.opened_by_savepoint = false;
    }

    fn track(&mut self, sql: &str) {
        let words = control_words(sql);
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["savepoint", name] => {
                if !self.in_transaction {
                    self.opened_by_savepoint = true;
                }
                self.in_transaction = true;
                self.savepoints.push((*name).to_owned());
            }
            ["release", "savepoint", name] | ["release", name] => {
                if let Some(at) = self.savepoints.iter().rposition(|known| known == name) {
                    self.savepoints.truncate(at);
                    if self.savepoints.is_empty() && self.opened_by_savepoint {
                        self.ended();
                    }
                }
            }
            ["rollback", "transaction", "to", "savepoint", name]
            | ["rollback", "transaction", "to", name]
            | ["rollback", "to", "savepoint", name]
            | ["rollback", "to", name] => {
                if let Some(at) = self.savepoints.iter().rposition(|known| known == name) {
                    self.savepoints.truncate(at + 1);
                }
            }
            ["begin", ..] => {
                self.ended();
                self.in_transaction = true;
            }
            ["commit" | "end" | "rollback", ..] => self.ended(),
            _ => {}
        }
    }

    async fn plain(&mut self, sql: &str) -> Result<(), DbError> {
        let printed = self.send(sql, 0).await?;
        if printed.errors.is_empty() { Ok(()) } else { Err(process::error(&printed.errors, sql)) }
    }

    async fn run(&mut self, text: &str, max_rows: usize) -> Result<Execution, DbError> {
        let prepared = statement::prepare(text)?;
        if prepared.sql.is_empty() {
            return Ok(Execution::default());
        }
        let first = prepared.words.first().map(String::as_str);
        self.begin_if_manual(first).await?;
        let declared = if first == Some("select") { self.declared_types(&prepared.sql).await? } else { HashMap::new() };
        let printed = self.send(&prepared.sql, max_rows).await?;
        if !printed.errors.is_empty() {
            return Err(process::error(&printed.errors, text));
        }
        self.track(&prepared.sql);
        let names = if printed.columns.is_empty() && queries(first) {
            self.process.columns_of(&prepared.sql, self.state.read_only).await?
        } else {
            printed.columns
        };
        let columns = if first == Some("select") {
            traced(&prepared.sql, &names, &printed.rows, &declared)
        } else {
            names
                .iter()
                .enumerate()
                .map(|(index, name)| Column::new(name.clone(), storage_type(&printed.rows, index)))
                .collect()
        };
        let writes = matches!(first, Some("insert" | "update" | "delete" | "replace"))
            || (first == Some("with") && printed.changes.0 > 0);
        Ok(Execution {
            columns,
            rows: printed.rows,
            truncated: printed.truncated,
            affected: writes.then(|| u64::try_from(printed.changes.0).unwrap_or(0)),
            notices: Vec::new(),
        })
    }

    async fn streamed(
        &mut self,
        text: &str,
        options: &StreamOptions,
        sink: &mut dyn RowSink,
    ) -> Result<StreamEnd, DbError> {
        let prepared = statement::prepare(text)?;
        if prepared.sql.is_empty() {
            return Ok(StreamEnd { affected: None, complete: true, notices: Vec::new() });
        }
        let first = prepared.words.first().map(String::as_str);
        self.begin_if_manual(first).await?;
        let select = first == Some("select");
        let declared = if select { self.declared_types(&prepared.sql).await? } else { HashMap::new() };
        let sql = prepared.sql.clone();
        let name = |names: &[String], rows: &[Vec<crate::value::Value>]| -> Vec<Column> {
            if select {
                traced(&sql, names, rows, &declared)
            } else {
                names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| Column::new(name.clone(), storage_type(rows, index)))
                    .collect()
            }
        };
        let mut into = process::StreamInto { sink, skip: options.skip, page: options.page.max(1), columns: &name };
        let interrupt = self.process.interrupt();
        let run = self.process.run_streamed(&prepared.sql, &mut into);
        let ran = match self.timeout {
            None => run.await,
            Some(limit) => {
                tokio::pin!(run);
                tokio::select! {
                    ran = &mut run => ran,
                    () = tokio::time::sleep(limit) => {
                        interrupt.send();
                        let _ = tokio::time::timeout(Duration::from_secs(2), &mut run).await;
                        Err(DbError::new(ErrorKind::Timeout, ""))
                    }
                }
            }
        };
        let (streamed, errors) = ran?;
        if !errors.is_empty() {
            return Err(process::error(&errors, text));
        }
        if !streamed.named && queries(first) {
            let names = self.process.columns_of(&prepared.sql, self.state.read_only).await?;
            if !names.is_empty() {
                sink.columns(name(&names, &[]));
            }
        }
        self.track(&prepared.sql);
        let writes = matches!(first, Some("insert" | "update" | "delete" | "replace"))
            || (first == Some("with") && streamed.changes.0 > 0);
        Ok(StreamEnd {
            affected: writes.then(|| u64::try_from(streamed.changes.0).unwrap_or(0)),
            complete: !streamed.dropped,
            notices: Vec::new(),
        })
    }

    async fn apply_state(&mut self, state: &TrackedState) -> Result<(), DbError> {
        if let Some(mode) = &state.isolation {
            let mode = capabilities()
                .isolation_levels
                .iter()
                .find(|level| level.eq_ignore_ascii_case(mode))
                .ok_or_else(|| DbError::new(ErrorKind::Invalid, mode))?;
            self.begin = format!("BEGIN {}", mode.to_uppercase());
        }
        self.plain(&format!("PRAGMA query_only = {}", i32::from(state.read_only))).await?;
        let was_auto = self.state.auto_commit;
        self.state = state.clone();
        if self.state.auto_commit && !was_auto && self.in_transaction {
            self.plain("COMMIT").await?;
            self.ended();
        }
        Ok(())
    }

    async fn nodes(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        let request = IntrospectRequest { force: true, system: true, ..IntrospectRequest::default() };
        let read = introspect::introspect(&mut self.process, &self.catalog, &request, None).await?;
        let mut snapshot = Snapshot::new("", ID);
        snapshot.catalogs = read.catalogs;
        let options = TreeOptions { show_system: self.show_system, ..TreeOptions::default() };
        let mut path = path.to_vec();
        if let Some(first) = path.first_mut() {
            first.clone_from(&self.catalog);
        }
        Ok(crate::schema::children(&snapshot, &path, &options).unwrap_or_default())
    }

    async fn end_transaction(&mut self, statement: &str) -> Result<(), DbError> {
        if self.in_transaction {
            self.plain(statement).await?;
            self.ended();
        }
        Ok(())
    }
}

impl DriverSession for Session {
    fn execute<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a ExecOptions,
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(self.run(statement, options.max_rows))
    }

    fn stream<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a StreamOptions,
        sink: &'a mut dyn RowSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        Box::pin(self.streamed(statement, options, sink))
    }

    fn execute_bound<'a>(
        &'a mut self,
        statement: &'a str,
        params: &'a [Param],
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(async move {
            if !params.is_empty() {
                return Err(DbError::new(ErrorKind::Unsupported, "sqlite3 takes values as quoted literals"));
            }
            self.run(statement, 1000).await
        })
    }

    fn children<'a>(&'a mut self, path: &'a [String]) -> BoxFuture<'a, Result<Vec<Node>, DbError>> {
        Box::pin(self.nodes(path))
    }

    fn introspect<'a>(
        &'a mut self,
        request: &'a IntrospectRequest,
        previous: Option<&'a Snapshot>,
    ) -> BoxFuture<'a, Result<Introspection, DbError>> {
        Box::pin(async move { introspect::introspect(&mut self.process, &self.catalog, request, previous).await })
    }

    fn ping(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(self.plain("SELECT 1"))
    }

    fn server_version(&mut self) -> BoxFuture<'_, Result<Option<String>, DbError>> {
        let version = format!("SQLite {}", self.process.version);
        Box::pin(async move { Ok(Some(version)) })
    }

    fn apply<'a>(&'a mut self, state: &'a TrackedState) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(self.apply_state(state))
    }

    fn commit(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(self.end_transaction("COMMIT"))
    }

    fn rollback(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(self.end_transaction("ROLLBACK"))
    }

    fn in_transaction(&self) -> bool {
        self.in_transaction && !self.process.ended()
    }

    fn is_closed(&self) -> bool {
        self.process.ended()
    }

    fn canceller(&self) -> Option<Arc<dyn Canceller>> {
        cfg!(unix).then(|| Arc::new(self.process.interrupt()) as Arc<dyn Canceller>)
    }

    fn take_notices(&mut self) -> Vec<Notice> {
        Vec::new()
    }
}
