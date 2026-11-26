use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use schemars::JsonSchema;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::csv_import::{Fed, Load, QUEUED_BATCHES, Row, Tally};
use super::export::ItemStatus;
use super::script::Work;
use super::{Engine, OperationKind, OperationStatus, Outcome};
use crate::csv_import::{LoadSettings, OnError};
use crate::dialect::SqlDialect;
use crate::driver::{BoxFuture, ContextKind, DbError, ErrorKind, RowSink, StreamOptions};
use crate::history::{Provenance, Ran, Source};
use crate::transfer::{SqlType, cell};
use crate::value::{Column, Value};

use super::RowError;

const KEPT_TRANSFERS: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferTable {
    pub name: String,
    pub select: String,
    pub table: String,
    pub columns: Vec<String>,
    pub types: Vec<SqlType>,
    pub prelude: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferRequest {
    pub source: String,
    pub target: String,
    pub dialect: SqlDialect,
    pub tables: Vec<TransferTable>,
    pub load: LoadSettings,
}

/// One table of a transfer as views and agents read it; never its rows.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferTableState {
    /// What it is.
    pub name: String,
    /// The target table, quoted and qualified.
    pub table: String,
    /// Where it is.
    pub status: ItemStatus,
    /// Rows read from the source.
    pub rows_read: u64,
    /// Rows the target took, committed or not.
    pub rows_inserted: u64,
    /// Rows committed.
    pub rows_committed: u64,
    /// Rows left out because they failed.
    pub rows_skipped: u64,
    /// Whether the rows went through PostgreSQL's `COPY`.
    pub server_copy: bool,
    /// The rows that failed, in order, at most 1 000 kept.
    pub errors: Vec<RowError>,
    /// How many rows failed in all.
    pub errors_total: u64,
    /// Why it stopped, when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
}

/// A transfer as views and agents read it; never its rows.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferState {
    /// Its id, the operation's.
    pub id: u64,
    /// The source connection's id.
    pub source: String,
    /// The target connection's id.
    pub target: String,
    /// Where it is: running until its last table ends; failed when a table failed.
    pub status: OperationStatus,
    /// Milliseconds it ran, or has run so far.
    pub elapsed_ms: u64,
    /// The table being copied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<usize>,
    /// Rows read from the source by all tables.
    pub rows_read: u64,
    /// Rows the target took by all tables.
    pub rows_inserted: u64,
    /// Rows committed by all tables.
    pub rows_committed: u64,
    /// Rows left out by all tables.
    pub rows_skipped: u64,
    /// Its tables in order.
    pub tables: Vec<TransferTableState>,
}

pub(super) struct Job {
    state: Mutex<TransferState>,
    tallies: Vec<Tally>,
    started: Instant,
}

impl Job {
    fn with(&self, change: impl FnOnce(&mut TransferState)) {
        change(&mut self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    }

    fn snapshot(&self) -> TransferState {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        for (table, tally) in state.tables.iter_mut().zip(&self.tallies) {
            table.rows_read = tally.read.load(Ordering::Relaxed);
            table.rows_inserted = tally.inserted.load(Ordering::Relaxed);
            table.rows_committed = tally.committed.load(Ordering::Relaxed);
            table.rows_skipped = tally.skipped.load(Ordering::Relaxed);
            table.server_copy = tally.server_copy.load(Ordering::Relaxed);
            (table.errors, table.errors_total) = tally.errors();
        }
        state.rows_read = state.tables.iter().map(|table| table.rows_read).sum();
        state.rows_inserted = state.tables.iter().map(|table| table.rows_inserted).sum();
        state.rows_committed = state.tables.iter().map(|table| table.rows_committed).sum();
        state.rows_skipped = state.tables.iter().map(|table| table.rows_skipped).sum();
        if state.status == OperationStatus::Running {
            state.elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        }
        state
    }
}

struct Pages {
    sender: tokio::sync::mpsc::Sender<Fed>,
    permit: Option<tokio::sync::mpsc::OwnedPermit<Fed>>,
    pending: VecDeque<Fed>,
    columns: Vec<String>,
    types: Vec<SqlType>,
    dialect: SqlDialect,
    page: usize,
    rows: u64,
    stop: bool,
    stopped: bool,
    token: CancellationToken,
}

impl Pages {
    fn convert(&mut self, rows: Vec<Vec<Value>>) -> Fed {
        let read = rows.len() as u64;
        let (mut converted, mut errors) = (Vec::with_capacity(rows.len()), Vec::new());
        for values in rows {
            self.rows += 1;
            let number = self.rows;
            let cells = self
                .types
                .iter()
                .enumerate()
                .map(|(at, kind)| {
                    cell(values.get(at).unwrap_or(&Value::Null), *kind, self.dialect).map_err(|code| RowError {
                        row: number,
                        line: number,
                        column: self.columns.get(at).cloned(),
                        code: code.to_owned(),
                        detail: String::new(),
                    })
                })
                .collect();
            match cells {
                Ok(cells) => converted.push(Row { row: number, line: number, cells }),
                Err(error) => {
                    errors.push(error);
                    if self.stop {
                        self.stopped = true;
                        break;
                    }
                }
            }
        }
        Fed::Batch { rows: converted, errors, bytes: 0, read }
    }

    async fn flush(&mut self) -> bool {
        while let Some(fed) = self.pending.pop_front() {
            let sent = tokio::select! {
                sent = self.sender.send(fed) => sent.is_ok(),
                () = self.token.cancelled() => false,
            };
            if !sent {
                return false;
            }
        }
        true
    }
}

impl RowSink for Pages {
    fn columns(&mut self, _columns: Vec<Column>) {}

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            if self.stopped || !self.flush().await {
                return 0;
            }
            let permit = tokio::select! {
                permit = self.sender.clone().reserve_owned() => permit.ok(),
                () = self.token.cancelled() => None,
            };
            match permit {
                Some(permit) => {
                    self.permit = Some(permit);
                    self.page
                }
                None => 0,
            }
        })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        if self.token.is_cancelled() {
            return false;
        }
        let fed = self.convert(rows);
        match self.permit.take() {
            Some(permit) => {
                permit.send(fed);
            }
            None => self.pending.push_back(fed),
        }
        !self.stopped
    }

    fn cap(&self) -> usize {
        if self.stopped || self.token.is_cancelled() { 0 } else { usize::MAX }
    }
}

impl Engine {
    pub fn transfer(&self, request: TransferRequest, provenance: Provenance) -> Result<u64, DbError> {
        if request.tables.is_empty() {
            return Err(DbError::new(ErrorKind::Invalid, "nothing to transfer").with_code("noTables"));
        }
        if let Some(table) =
            request.tables.iter().find(|table| table.columns.is_empty() || table.columns.len() != table.types.len())
        {
            return Err(DbError::new(ErrorKind::Invalid, table.name.clone()).with_code("noColumns"));
        }
        if self.read_only(&request.target) {
            return Err(DbError::new(ErrorKind::Refused, "INSERT").with_code("readOnly"));
        }
        let shared = self
            .state()
            .connections
            .get(&request.target)
            .is_some_and(|conn| conn.driver.capabilities().shared_connection);
        if shared && request.source == request.target {
            return Err(DbError::new(ErrorKind::Invalid, "one session reads and writes").with_code("sameSession"));
        }
        let (op, token) = self.begin(OperationKind::Transfer, Some(&request.target), None);
        let job = Arc::new(Job {
            state: Mutex::new(TransferState {
                id: op,
                source: request.source.clone(),
                target: request.target.clone(),
                status: OperationStatus::Running,
                elapsed_ms: 0,
                current: None,
                rows_read: 0,
                rows_inserted: 0,
                rows_committed: 0,
                rows_skipped: 0,
                tables: request
                    .tables
                    .iter()
                    .map(|table| TransferTableState {
                        name: table.name.clone(),
                        table: table.table.clone(),
                        status: ItemStatus::Pending,
                        rows_read: 0,
                        rows_inserted: 0,
                        rows_committed: 0,
                        rows_skipped: 0,
                        server_copy: false,
                        errors: Vec::new(),
                        errors_total: 0,
                        error: None,
                    })
                    .collect(),
            }),
            tallies: request.tables.iter().map(|_| Tally::default()).collect(),
            started: Instant::now(),
        });
        {
            let mut state = self.state();
            state.transfers.insert(op, Arc::clone(&job));
            while state.transfers.len() > KEPT_TRANSFERS {
                let Some(oldest) = state
                    .transfers
                    .iter()
                    .find(|(_, job)| job.state.lock().is_ok_and(|state| state.status != OperationStatus::Running))
                    .map(|(id, _)| *id)
                else {
                    break;
                };
                state.transfers.remove(&oldest);
            }
        }
        let engine = self.clone();
        self.spawn(op, async move {
            let mut failed = None;
            let mut cancelled = false;
            for index in 0..request.tables.len() {
                if token.is_cancelled() {
                    cancelled = true;
                    break;
                }
                job.with(|state| {
                    state.current = Some(index);
                    state.tables[index].status = ItemStatus::Running;
                });
                engine.wake();
                let ran = engine.transfer_table(op, &request, index, &job.tallies[index], &token, &provenance).await;
                let status = match &ran {
                    Ok(()) => ItemStatus::Done,
                    Err(error) if error.kind == ErrorKind::Cancelled => ItemStatus::Cancelled,
                    Err(_) => ItemStatus::Failed,
                };
                job.with(|state| {
                    state.tables[index].status = status;
                    state.tables[index].error =
                        ran.as_ref().err().filter(|error| error.kind != ErrorKind::Cancelled).cloned();
                });
                match ran {
                    Ok(()) => {}
                    Err(error) if error.kind == ErrorKind::Cancelled => {
                        cancelled = true;
                        break;
                    }
                    Err(error) => {
                        failed = failed.or(Some(error));
                        if request.load.on_error == OnError::Stop {
                            break;
                        }
                    }
                }
            }
            let status = if cancelled {
                OperationStatus::Cancelled
            } else if failed.is_some() {
                OperationStatus::Failed
            } else {
                OperationStatus::Done
            };
            job.with(|state| {
                state.current = None;
                state.status = status;
                state.elapsed_ms = u64::try_from(job.started.elapsed().as_millis()).unwrap_or(u64::MAX);
                for table in &mut state.tables {
                    if matches!(table.status, ItemStatus::Pending | ItemStatus::Running) {
                        table.status = ItemStatus::Cancelled;
                    }
                }
            });
            if cancelled {
                return Err(DbError::new(ErrorKind::Cancelled, ""));
            }
            failed.map_or(Ok(Outcome::Nothing), Err)
        });
        Ok(op)
    }

    async fn transfer_table(
        &self,
        op: u64,
        request: &TransferRequest,
        index: usize,
        tally: &Tally,
        token: &CancellationToken,
        provenance: &Provenance,
    ) -> Result<(), DbError> {
        let table = &request.tables[index];
        let mut provenance = provenance.clone();
        provenance.source = Source::Transfer;
        provenance.place = provenance.place.or_else(|| Some(table.name.clone()));
        let (sender, receiver) = tokio::sync::mpsc::channel(QUEUED_BATCHES);
        let stop = token.child_token();
        let pages = Pages {
            sender,
            permit: None,
            pending: VecDeque::new(),
            columns: table.columns.clone(),
            types: table.types.clone(),
            dialect: request.dialect,
            page: request.load.batch.clamp(1, 100_000),
            rows: 0,
            stop: request.load.on_error == OnError::Stop,
            stopped: false,
            token: stop.clone(),
        };
        let target = Load {
            connection: &request.target,
            dialect: request.dialect,
            table: &table.table,
            names: table.columns.clone(),
            prelude: &table.prelude,
            load: &request.load,
            context: "transfer",
        };
        let reading = self.transfer_read(op, (&request.source, &table.select), pages, &stop, &provenance);
        let loading = async {
            let loaded = self.load_rows(op, &target, receiver, tally, token, &provenance).await;
            stop.cancel();
            loaded
        };
        let ((), loaded) = tokio::join!(reading, loading);
        loaded
    }

    async fn transfer_read(
        &self,
        op: u64,
        (id, statement): (&str, &str),
        mut pages: Pages,
        token: &CancellationToken,
        provenance: &Provenance,
    ) {
        let wanted = ContextKind::Console(format!("transfer-read-{op}"));
        let (kind, cell) = match self.connected_cell(id, &wanted, token).await {
            Ok(found) => found,
            Err(error) => {
                let _ = pages.sender.send(Fed::Failed(error)).await;
                return;
            }
        };
        let own = kind == wanted;
        self.busy(id, &kind, 1);
        let mut run = None;
        let read = async {
            let mut ctx = tokio::select! {
                ctx = cell.lock_owned() => ctx,
                () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
            };
            if own && ctx.session.is_none() {
                ctx.tracked.auto_commit = true;
                ctx.tracked.read_only = true;
            }
            self.ready(id, &kind, &mut ctx, op).await?;
            let (started, clock) = (SystemTime::now(), Instant::now());
            let options = StreamOptions { page: pages.page, skip: 0 };
            let streamed =
                self.stream_guarded(&mut ctx, statement, Work::Rows(&options, &mut pages), None, token).await;
            run = Some((ctx.tracked.schema.clone(), started, clock.elapsed()));
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
                return Err(DbError::new(ErrorKind::Cancelled, ""));
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
        if let Some((schema, started, duration)) = run {
            self.record(Ran {
                connection: id.to_owned(),
                context: kind.clone(),
                schema,
                text: statement.to_owned(),
                started,
                duration,
                rows: read.is_ok().then_some(pages.rows),
                affected: None,
                error: read.as_ref().err().filter(|_| !token.is_cancelled()).cloned(),
                provenance: provenance.clone(),
                params: Vec::new(),
            });
        }
        match read {
            Ok(()) => {
                pages.flush().await;
            }
            Err(_) if token.is_cancelled() => {}
            Err(error) => {
                pages.flush().await;
                let _ = pages.sender.send(Fed::Failed(error)).await;
            }
        }
    }

    pub fn transfer_state(&self, id: u64) -> Option<TransferState> {
        let job = self.state().transfers.get(&id).cloned()?;
        Some(job.snapshot())
    }

    pub fn transfers(&self) -> Vec<u64> {
        self.state().transfers.keys().copied().collect()
    }
}
