use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::script::Work;
use super::{Engine, OperationKind, OperationStatus, Outcome};
use crate::dialect::SqlDialect;
use crate::driver::{BoxFuture, ContextKind, DbError, ErrorKind, RowSink, StreamOptions};
use crate::history::{Provenance, Ran};
use crate::schema::ObjectKind;
use crate::search::{CatalogTable, ColumnClass, catalog_query, catalog_tables, classify, data_query, name_matches};
use crate::value::{Column, Value};

const KEPT_SEARCHES: usize = 20;
const CATALOG_ROWS: usize = 200_000;
const CATALOG_TIMEOUT: Duration = Duration::from_secs(60);
const PAGE: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRequest {
    pub connection: String,
    pub schema: String,
    pub text: String,
    pub case_sensitive: bool,
    pub data: bool,
    pub metadata: bool,
    pub classes: Vec<ColumnClass>,
    pub columns: Vec<String>,
    pub tables: Vec<String>,
    pub views: bool,
    pub limit: usize,
    pub table_timeout: Duration,
    pub dialect: SqlDialect,
}

/// Where one table of a search is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TableStatus {
    /// Not searched yet.
    Pending,
    /// Being searched.
    Running,
    /// Searched.
    Done,
    /// Stopped at its deadline; the rows counted before it stay.
    Timeout,
    /// The server refused the query; `error` says why.
    Failed,
    /// Not searched: none of its columns is of a chosen class or name.
    Skipped,
    /// Not searched because the search was cancelled.
    Cancelled,
}

/// A column with rows that contain the text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ColumnHit {
    /// The column's name.
    pub name: String,
    /// How many of the rows counted have the text in this column.
    pub rows: u64,
}

/// One table of a search as views and agents read it; never its values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableHit {
    /// The table's name.
    pub table: String,
    /// Table, view or materialised view.
    pub kind: ObjectKind,
    /// Where its search is.
    pub status: TableStatus,
    /// Whether the table's name contains the text.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub name_matches: bool,
    /// Its columns whose names contain the text.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub named_columns: Vec<String>,
    /// How many columns were searched.
    pub searched_columns: usize,
    /// Rows with the text, counted up to the limit.
    pub rows: u64,
    /// Whether more rows may have the text than were counted: the limit was reached or the deadline passed.
    pub truncated: bool,
    /// The columns that have the text, and in how many of the rows counted.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ColumnHit>,
    /// Milliseconds its query ran.
    pub elapsed_ms: u64,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
}

impl TableHit {
    pub fn found(&self) -> bool {
        self.rows > 0 || self.name_matches || !self.named_columns.is_empty()
    }
}

/// A search as views and agents read it; never values.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchState {
    /// Its id, the operation's.
    pub id: u64,
    /// The connection.
    pub connection: String,
    /// The schema searched.
    pub schema: String,
    /// The text looked for.
    pub text: String,
    /// Whether case had to match.
    pub case_sensitive: bool,
    /// Whether rows were searched.
    pub data: bool,
    /// Whether names were searched.
    pub metadata: bool,
    /// The most rows counted per table.
    pub limit: usize,
    /// Milliseconds one table may take.
    pub table_timeout_ms: u64,
    /// Where it is: running until the last table ends; failed when the catalogue could not be read.
    pub status: OperationStatus,
    /// Milliseconds it ran, or has run so far.
    pub elapsed_ms: u64,
    /// Whether the tables are known yet (the catalogue was read).
    pub listed: bool,
    /// The table being searched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// How many tables ended (searched, stopped, failed or skipped).
    pub ended: usize,
    /// Rows with the text in every table, as counted.
    pub rows: u64,
    /// How many tables have the text in their rows or names.
    pub found: usize,
    /// Whether the catalogue had more columns than were read, so some tables were left out.
    pub catalog_truncated: bool,
    /// The schema's tables in name order, each with what was found.
    pub tables: Vec<TableHit>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
}

struct Place {
    kind: ContextKind,
    cell: super::CtxCell,
    own: bool,
}

pub(super) struct Job {
    state: Mutex<SearchState>,
    started: Instant,
    provenance: Provenance,
}

impl Job {
    fn with(&self, change: impl FnOnce(&mut SearchState)) {
        change(&mut self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    }

    fn snapshot(&self) -> SearchState {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if state.status == OperationStatus::Running {
            state.elapsed_ms = super::millis(self.started.elapsed());
        }
        state
    }
}

struct Collect {
    rows: Vec<Vec<Value>>,
    cap: usize,
    truncated: bool,
}

impl RowSink for Collect {
    fn columns(&mut self, _columns: Vec<Column>) {}

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> BoxFuture<'_, usize> {
        let left = self.cap.saturating_sub(self.rows.len());
        Box::pin(async move { left.min(PAGE) })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        let room = self.cap.saturating_sub(self.rows.len());
        self.truncated |= rows.len() > room;
        self.rows.extend(rows.into_iter().take(room));
        self.rows.len() < self.cap
    }

    fn cap(&self) -> usize {
        self.cap
    }
}

struct Count {
    counts: Vec<u64>,
    rows: u64,
    cap: u64,
}

fn flagged(value: &Value) -> bool {
    match value {
        Value::Integer(number) => *number != 0,
        Value::Bool(flag) => *flag,
        Value::Decimal(text) | Value::Text(text) => text.trim() != "0" && !text.trim().is_empty(),
        Value::Float(number) => *number != 0.0,
        _ => false,
    }
}

impl RowSink for Count {
    fn columns(&mut self, _columns: Vec<Column>) {}

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> BoxFuture<'_, usize> {
        let left = usize::try_from(self.cap.saturating_sub(self.rows)).unwrap_or(usize::MAX);
        Box::pin(async move { left.min(PAGE) })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        for row in rows {
            if self.rows >= self.cap {
                break;
            }
            self.rows += 1;
            for (count, value) in self.counts.iter_mut().zip(&row) {
                if flagged(value) {
                    *count += 1;
                }
            }
        }
        self.rows < self.cap
    }

    fn cap(&self) -> usize {
        usize::try_from(self.cap).unwrap_or(usize::MAX)
    }
}

fn listed(names: &[String], name: &str) -> bool {
    let name = name.to_lowercase();
    names.is_empty() || names.iter().any(|wanted| wanted.trim().to_lowercase() == name)
}

fn searched_columns(request: &SearchRequest, table: &CatalogTable) -> Vec<String> {
    table
        .columns
        .iter()
        .filter(|column| request.classes.contains(&classify(&column.type_name)))
        .filter(|column| listed(&request.columns, &column.name))
        .map(|column| column.name.clone())
        .collect()
}

impl Engine {
    pub fn search(&self, request: SearchRequest, provenance: Provenance) -> Result<u64, DbError> {
        if request.text.is_empty() {
            return Err(DbError::new(ErrorKind::Invalid, "nothing to find").with_code("noText"));
        }
        if !request.data && !request.metadata {
            return Err(DbError::new(ErrorKind::Invalid, "neither data nor names").with_code("nothingToSearch"));
        }
        let (op, token) = self.begin(OperationKind::Search, Some(&request.connection), None);
        let job = Arc::new(Job {
            state: Mutex::new(SearchState {
                id: op,
                connection: request.connection.clone(),
                schema: request.schema.clone(),
                text: request.text.clone(),
                case_sensitive: request.case_sensitive,
                data: request.data,
                metadata: request.metadata,
                limit: request.limit,
                table_timeout_ms: super::millis(request.table_timeout),
                status: OperationStatus::Running,
                elapsed_ms: 0,
                listed: false,
                current: None,
                ended: 0,
                rows: 0,
                found: 0,
                catalog_truncated: false,
                tables: Vec::new(),
                error: None,
            }),
            started: Instant::now(),
            provenance,
        });
        {
            let mut state = self.state();
            state.searches.insert(op, Arc::clone(&job));
            while state.searches.len() > KEPT_SEARCHES {
                let Some(oldest) = state
                    .searches
                    .iter()
                    .find(|(_, job)| job.state.lock().is_ok_and(|state| state.status != OperationStatus::Running))
                    .map(|(id, _)| *id)
                else {
                    break;
                };
                state.searches.remove(&oldest);
            }
        }
        let engine = self.clone();
        self.spawn(op, async move {
            let ran = engine.search_task(op, &request, &job, &token).await;
            let cancelled = token.is_cancelled() || ran.as_ref().is_err_and(|error| error.kind == ErrorKind::Cancelled);
            let status = match &ran {
                _ if cancelled => OperationStatus::Cancelled,
                Ok(()) => OperationStatus::Done,
                Err(_) => OperationStatus::Failed,
            };
            job.with(|state| {
                state.status = status;
                state.current = None;
                state.elapsed_ms = super::millis(job.started.elapsed());
                state.error = ran.as_ref().err().filter(|error| error.kind != ErrorKind::Cancelled).cloned();
                for table in &mut state.tables {
                    if matches!(table.status, TableStatus::Pending | TableStatus::Running) {
                        table.status = TableStatus::Cancelled;
                    }
                }
            });
            if cancelled {
                return Err(DbError::new(ErrorKind::Cancelled, ""));
            }
            ran.map(|()| Outcome::Nothing)
        });
        Ok(op)
    }

    async fn search_task(
        &self,
        op: u64,
        request: &SearchRequest,
        job: &Job,
        token: &CancellationToken,
    ) -> Result<(), DbError> {
        let id = request.connection.as_str();
        let wanted = ContextKind::Console(format!("search-{op}"));
        let (kind, cell) = self.connected_cell(id, &wanted, token).await?;
        let own = kind == wanted;
        self.busy(id, &kind, 1);
        let place = Place { kind: kind.clone(), cell, own };
        let ran = async {
            let catalog = catalog_query(request.dialect, &request.schema);
            let mut sink = Collect { rows: Vec::new(), cap: CATALOG_ROWS, truncated: false };
            let timeout = request.table_timeout.max(CATALOG_TIMEOUT);
            let read = self.read_guarded(op, request, &place, &catalog, &mut sink, (timeout, token), job).await;
            read?;
            if token.is_cancelled() {
                return Err(DbError::new(ErrorKind::Cancelled, ""));
            }
            let tables: Vec<CatalogTable> = catalog_tables(request.dialect, &sink.rows)
                .into_iter()
                .filter(|table| request.views || table.kind != ObjectKind::View)
                .filter(|table| listed(&request.tables, &table.name))
                .collect();
            let plans: Vec<Vec<String>> = tables.iter().map(|table| searched_columns(request, table)).collect();
            job.with(|state| {
                state.listed = true;
                state.catalog_truncated = sink.truncated;
                state.tables = tables
                    .iter()
                    .zip(&plans)
                    .map(|(table, columns)| {
                        let named: Vec<String> = if request.metadata {
                            table
                                .columns
                                .iter()
                                .filter(|column| name_matches(&column.name, &request.text, request.case_sensitive))
                                .map(|column| column.name.clone())
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let searchable = request.data && !columns.is_empty();
                        TableHit {
                            table: table.name.clone(),
                            kind: table.kind,
                            status: if searchable {
                                TableStatus::Pending
                            } else if request.data {
                                TableStatus::Skipped
                            } else {
                                TableStatus::Done
                            },
                            name_matches: request.metadata
                                && name_matches(&table.name, &request.text, request.case_sensitive),
                            named_columns: named,
                            searched_columns: if request.data { columns.len() } else { 0 },
                            rows: 0,
                            truncated: false,
                            columns: Vec::new(),
                            elapsed_ms: 0,
                            error: None,
                        }
                    })
                    .collect();
                state.ended = state.tables.iter().filter(|table| table.status != TableStatus::Pending).count();
                state.found = state.tables.iter().filter(|table| table.found()).count();
            });
            self.wake();
            if !request.data {
                return Ok(());
            }
            for (index, (table, columns)) in tables.iter().zip(&plans).enumerate() {
                if columns.is_empty() {
                    continue;
                }
                if token.is_cancelled() {
                    return Err(DbError::new(ErrorKind::Cancelled, ""));
                }
                job.with(|state| {
                    state.current = Some(table.name.clone());
                    state.tables[index].status = TableStatus::Running;
                });
                self.wake();
                let limit = request.limit.max(1);
                let sql = data_query(
                    request.dialect,
                    (&request.schema, &table.name),
                    columns,
                    &request.text,
                    request.case_sensitive,
                    limit.saturating_add(1),
                );
                let mut count = Count { counts: vec![0; columns.len()], rows: 0, cap: limit as u64 + 1 };
                let clock = Instant::now();
                let read =
                    self.read_guarded(op, request, &place, &sql, &mut count, (request.table_timeout, token), job).await;
                let elapsed = super::millis(clock.elapsed());
                if token.is_cancelled() {
                    job.with(|state| state.tables[index].status = TableStatus::Cancelled);
                    return Err(DbError::new(ErrorKind::Cancelled, ""));
                }
                let status = match &read {
                    Ok(()) => TableStatus::Done,
                    Err(error) if error.kind == ErrorKind::Timeout => TableStatus::Timeout,
                    Err(error) if matches!(error.kind, ErrorKind::Connection | ErrorKind::NotConnected) => {
                        return Err(error.clone());
                    }
                    Err(_) => TableStatus::Failed,
                };
                let limit = limit as u64;
                job.with(|state| {
                    let hit = &mut state.tables[index];
                    hit.status = status;
                    hit.elapsed_ms = elapsed;
                    hit.rows = count.rows.min(limit);
                    hit.truncated = count.rows > limit || status == TableStatus::Timeout;
                    hit.columns = columns
                        .iter()
                        .zip(&count.counts)
                        .filter(|(_, rows)| **rows > 0)
                        .map(|(name, rows)| ColumnHit { name: name.clone(), rows: (*rows).min(limit) })
                        .collect();
                    hit.error = read.as_ref().err().filter(|_| status == TableStatus::Failed).cloned();
                    state.ended += 1;
                    state.rows += hit.rows;
                    state.found = state.tables.iter().filter(|table| table.found()).count();
                    state.current = None;
                });
                self.wake();
            }
            Ok(())
        }
        .await;
        self.busy(id, &kind, -1);
        if own {
            let mut state = self.state();
            if let Some(conn) = state.connections.get_mut(id) {
                conn.contexts.remove(&kind);
                conn.info.remove(&kind);
            }
        }
        ran
    }

    #[allow(clippy::too_many_arguments, reason = "one statement of a running search and where its rows go")]
    async fn read_guarded(
        &self,
        op: u64,
        request: &SearchRequest,
        place: &Place,
        statement: &str,
        sink: &mut dyn RowSink,
        (timeout, token): (Duration, &CancellationToken),
        job: &Job,
    ) -> Result<(), DbError> {
        let kind = &place.kind;
        let mut ctx = tokio::select! {
            ctx = Arc::clone(&place.cell).lock_owned() => ctx,
            () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
        };
        if place.own && ctx.session.is_none() {
            ctx.tracked.auto_commit = true;
            ctx.tracked.read_only = true;
        }
        self.ready(&request.connection, kind, &mut ctx, op).await?;
        let (started, clock) = (SystemTime::now(), Instant::now());
        let options = StreamOptions { page: PAGE, skip: 0 };
        let streamed = self.stream_guarded(&mut ctx, statement, Work::Rows(&options, sink), Some(timeout), token).await;
        if let Some(session) = ctx.session.as_mut() {
            let _ = session.take_notices();
            if place.own && session.in_transaction() {
                let _ = session.rollback().await;
            }
        }
        ctx.last_used = Instant::now();
        if !place.own {
            self.update_info(&request.connection, kind, &ctx);
        }
        let mut provenance = job.provenance.clone();
        provenance.place = provenance.place.or_else(|| Some(request.schema.clone()));
        self.record(Ran {
            connection: request.connection.clone(),
            context: kind.clone(),
            schema: ctx.tracked.schema.clone(),
            text: statement.to_owned(),
            started,
            duration: clock.elapsed(),
            rows: None,
            affected: None,
            error: streamed.as_ref().err().cloned(),
            provenance,
            params: Vec::new(),
        });
        let end = streamed?;
        if !end.complete && token.is_cancelled() {
            return Err(DbError::new(ErrorKind::Cancelled, ""));
        }
        Ok(())
    }

    pub fn search_state(&self, id: u64) -> Option<SearchState> {
        let job = self.state().searches.get(&id).cloned()?;
        Some(job.snapshot())
    }

    pub fn searches(&self) -> Vec<u64> {
        self.state().searches.keys().copied().collect()
    }
}
