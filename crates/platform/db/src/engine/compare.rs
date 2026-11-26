use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use schemars::JsonSchema;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::script::Work;
use super::{Engine, OperationKind, OperationStatus, Outcome};
use crate::compare::Change;
use crate::compare::data::{DataCounts, RowDiff, differing, key_text, lookup_query, page_query};
use crate::dialect::SqlDialect;
use crate::driver::{BoxFuture, ContextKind, DbError, ErrorKind, RowSink, StreamOptions};
use crate::value::{Column, Value};

const KEPT_COMPARES: usize = 20;

pub const PAGE: usize = 1000;

pub const KEPT_ROWS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataSide {
    pub connection: String,
    pub table: String,
    pub dialect: SqlDialect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataCompareRequest {
    pub source: DataSide,
    pub target: DataSide,
    pub key: Vec<String>,
    pub columns: Vec<String>,
    pub page: usize,
    pub kept: usize,
}

/// Which side a data comparison is reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ComparePass {
    /// The source's rows, each page looked up on the target.
    Source,
    /// The target's rows, looked up on the source for those it lacks.
    Target,
    /// Finished.
    Done,
}

/// A data comparison as views and agents read it; never its rows.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataCompareState {
    /// Its id, the operation's.
    pub id: u64,
    /// Where it is.
    pub status: OperationStatus,
    /// Which side it is reading.
    pub pass: ComparePass,
    /// Milliseconds it ran, or has run so far.
    pub elapsed_ms: u64,
    /// The source connection.
    pub source: String,
    /// The source table.
    pub source_table: String,
    /// The target connection.
    pub target: String,
    /// The target table.
    pub target_table: String,
    /// The key columns.
    pub key: Vec<String>,
    /// The columns compared, the key first, in the order the rows hold them.
    pub columns: Vec<String>,
    /// The rows counted so far.
    pub counts: DataCounts,
    /// How many differing rows are kept.
    pub kept: usize,
    /// Whether more rows differ than were kept.
    pub truncated: bool,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
}

pub(super) struct Job {
    id: u64,
    state: Mutex<DataCompareState>,
    rows: Mutex<Vec<RowDiff>>,
    started: Instant,
    limit: usize,
}

impl Job {
    fn with(&self, change: impl FnOnce(&mut DataCompareState)) {
        change(&mut self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    }

    fn keep(&self, diff: RowDiff) {
        let mut rows = self.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if rows.len() < self.limit {
            rows.push(diff);
            let kept = rows.len();
            self.with(|state| state.kept = kept);
        } else {
            self.with(|state| state.truncated = true);
        }
    }

    fn snapshot(&self) -> DataCompareState {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if state.status == OperationStatus::Running {
            state.elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        }
        state
    }
}

struct Collect {
    rows: Vec<Vec<Value>>,
    token: CancellationToken,
    page: usize,
}

impl RowSink for Collect {
    fn columns(&mut self, _columns: Vec<Column>) {}

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> BoxFuture<'_, usize> {
        Box::pin(async move { if self.token.is_cancelled() { 0 } else { self.page.max(1) } })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        self.rows.extend(rows);
        !self.token.is_cancelled()
    }

    fn cap(&self) -> usize {
        usize::MAX
    }
}

fn cancelled() -> DbError {
    DbError::new(ErrorKind::Cancelled, "")
}

impl Engine {
    pub fn compare_data(&self, request: DataCompareRequest) -> Result<u64, DbError> {
        if request.key.is_empty() {
            return Err(DbError::new(ErrorKind::Invalid, "no key columns").with_code("noKey"));
        }
        let mut columns = request.key.clone();
        columns.extend(request.columns.iter().filter(|column| !request.key.contains(column)).cloned());
        let (op, token) = self.begin(OperationKind::Compare, Some(&request.source.connection), None);
        let job = Arc::new(Job {
            id: op,
            state: Mutex::new(DataCompareState {
                id: op,
                status: OperationStatus::Running,
                pass: ComparePass::Source,
                elapsed_ms: 0,
                source: request.source.connection.clone(),
                source_table: request.source.table.clone(),
                target: request.target.connection.clone(),
                target_table: request.target.table.clone(),
                key: request.key.clone(),
                columns: columns.clone(),
                counts: DataCounts::default(),
                kept: 0,
                truncated: false,
                error: None,
            }),
            rows: Mutex::new(Vec::new()),
            started: Instant::now(),
            limit: request.kept,
        });
        {
            let mut state = self.state();
            state.compares.insert(op, Arc::clone(&job));
            while state.compares.len() > KEPT_COMPARES {
                let Some(oldest) = state
                    .compares
                    .iter()
                    .find(|(_, job)| job.state.lock().is_ok_and(|state| state.status != OperationStatus::Running))
                    .map(|(id, _)| *id)
                else {
                    break;
                };
                state.compares.remove(&oldest);
            }
        }
        let engine = self.clone();
        self.spawn(op, async move {
            let contexts = [
                ContextKind::Console(format!("compare-{op}-source")),
                ContextKind::Console(format!("compare-{op}-target")),
            ];
            let ran = engine.compare_rows(&request, &columns, &job, &contexts, &token).await;
            for (side, context) in [&request.source, &request.target].into_iter().zip(&contexts) {
                let mut state = engine.state();
                if let Some(conn) = state.connections.get_mut(&side.connection) {
                    conn.contexts.remove(context);
                    conn.info.remove(context);
                }
            }
            let status = match &ran {
                Ok(()) => OperationStatus::Done,
                Err(error) if error.kind == ErrorKind::Cancelled => OperationStatus::Cancelled,
                Err(_) => OperationStatus::Failed,
            };
            job.with(|state| {
                state.status = status;
                state.pass = ComparePass::Done;
                state.error = ran.as_ref().err().cloned();
                state.elapsed_ms = u64::try_from(job.started.elapsed().as_millis()).unwrap_or(u64::MAX);
            });
            ran.map(|()| Outcome::Nothing)
        });
        Ok(op)
    }

    async fn compare_rows(
        &self,
        request: &DataCompareRequest,
        columns: &[String],
        job: &Job,
        contexts: &[ContextKind; 2],
        token: &CancellationToken,
    ) -> Result<(), DbError> {
        let key = request.key.as_slice();
        let width = key.len();
        let page = request.page.max(1);
        let (source, target) = (&request.source, &request.target);
        let mut after: Option<Vec<Value>> = None;
        loop {
            let query = page_query(source.dialect, &source.table, (key, columns), after.as_deref(), page);
            let rows = self.read_page(job.id, (source, &contexts[0]), &query, token, page).await?;
            if rows.is_empty() {
                break;
            }
            let keys: Vec<Vec<Value>> = rows.iter().map(|row| row[..width.min(row.len())].to_vec()).collect();
            let lookup = lookup_query(target.dialect, &target.table, (key, columns), &keys);
            let found = self.read_page(job.id, (target, &contexts[1]), &lookup, token, page).await?;
            let by_key: HashMap<String, &Vec<Value>> =
                found.iter().map(|row| (key_text(&row[..width.min(row.len())]), row)).collect();
            let mut counts = DataCounts::default();
            for (row, values) in rows.iter().zip(&keys) {
                match by_key.get(&key_text(values)) {
                    None => {
                        counts.added += 1;
                        job.keep(RowDiff {
                            change: Change::Added,
                            key: values.clone(),
                            columns: Vec::new(),
                            source: Some(row.clone()),
                            target: None,
                        });
                    }
                    Some(other) => {
                        let differ = differing(columns, row, other);
                        if differ.is_empty() {
                            counts.same += 1;
                        } else {
                            counts.changed += 1;
                            job.keep(RowDiff {
                                change: Change::Changed,
                                key: values.clone(),
                                columns: differ,
                                source: Some(row.clone()),
                                target: Some((*other).clone()),
                            });
                        }
                    }
                }
            }
            let read = rows.len();
            job.with(|state| {
                state.counts.source_rows += read as u64;
                state.counts.same += counts.same;
                state.counts.changed += counts.changed;
                state.counts.added += counts.added;
            });
            self.wake();
            after = keys.last().cloned();
            if read < page {
                break;
            }
        }
        job.with(|state| state.pass = ComparePass::Target);
        let mut after: Option<Vec<Value>> = None;
        loop {
            let query = page_query(target.dialect, &target.table, (key, columns), after.as_deref(), page);
            let rows = self.read_page(job.id, (target, &contexts[1]), &query, token, page).await?;
            if rows.is_empty() {
                break;
            }
            let keys: Vec<Vec<Value>> = rows.iter().map(|row| row[..width.min(row.len())].to_vec()).collect();
            let lookup = lookup_query(source.dialect, &source.table, (key, key), &keys);
            let found = self.read_page(job.id, (source, &contexts[0]), &lookup, token, page).await?;
            let known: std::collections::HashSet<String> =
                found.iter().map(|row| key_text(&row[..width.min(row.len())])).collect();
            let mut removed = 0;
            for (row, values) in rows.iter().zip(&keys) {
                if !known.contains(&key_text(values)) {
                    removed += 1;
                    job.keep(RowDiff {
                        change: Change::Removed,
                        key: values.clone(),
                        columns: Vec::new(),
                        source: None,
                        target: Some(row.clone()),
                    });
                }
            }
            let read = rows.len();
            job.with(|state| {
                state.counts.target_rows += read as u64;
                state.counts.removed += removed;
            });
            self.wake();
            after = keys.last().cloned();
            if read < page {
                break;
            }
        }
        Ok(())
    }

    async fn read_page(
        &self,
        op: u64,
        (side, wanted): (&DataSide, &ContextKind),
        statement: &str,
        token: &CancellationToken,
        page: usize,
    ) -> Result<Vec<Vec<Value>>, DbError> {
        if token.is_cancelled() {
            return Err(cancelled());
        }
        let id = side.connection.as_str();
        let (kind, cell) = self.connected_cell(id, wanted, token).await?;
        let own = kind == *wanted;
        self.busy(id, &kind, 1);
        let read = async {
            let mut ctx = tokio::select! {
                ctx = cell.lock_owned() => ctx,
                () = token.cancelled() => return Err(cancelled()),
            };
            if own && ctx.session.is_none() {
                ctx.tracked.auto_commit = true;
                ctx.tracked.read_only = true;
            }
            self.ready(id, &kind, &mut ctx, op).await?;
            let mut sink = Collect { rows: Vec::new(), token: token.child_token(), page };
            let options = StreamOptions { page, skip: 0 };
            let streamed = self.stream_guarded(&mut ctx, statement, Work::Rows(&options, &mut sink), None, token).await;
            if let Some(session) = ctx.session.as_mut() {
                let _ = session.take_notices();
                if own && session.in_transaction() {
                    let _ = session.rollback().await;
                }
            }
            ctx.last_used = Instant::now();
            if !own {
                self.update_info(id, &kind, &ctx);
            }
            let ended = streamed?;
            if !ended.complete && token.is_cancelled() {
                return Err(cancelled());
            }
            Ok(sink.rows)
        }
        .await;
        self.busy(id, &kind, -1);
        read
    }

    pub fn data_compare_state(&self, id: u64) -> Option<DataCompareState> {
        let job = self.state().compares.get(&id).cloned()?;
        Some(job.snapshot())
    }

    pub fn data_compare_rows(&self, id: u64) -> Option<Vec<RowDiff>> {
        let job = self.state().compares.get(&id).cloned()?;
        Some(job.rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone())
    }
}
