use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::{Ctx, Engine, Event, OperationKind, OperationStatus, Outcome};
use crate::csv_import::{
    Cell, ColumnMapping, CsvFormat, CsvReader, LoadSettings, OnError, convert, copy_rows, copy_statement, insert_head,
    insert_statement,
};
use crate::dialect::SqlDialect;
use crate::driver::{Canceller, ContextKind, DbError, ErrorKind, ExecOptions};
use crate::history::{Provenance, Ran, Source};

const KEPT_IMPORTS: usize = 50;
const KEPT_ERRORS: usize = 1000;
const QUEUED: usize = 2;
const SAVEPOINT: &str = "zen_import";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportRequest {
    pub connection: String,
    pub file: PathBuf,
    pub format: CsvFormat,
    pub dialect: SqlDialect,
    pub table: String,
    pub name: String,
    pub columns: Vec<ColumnMapping>,
    pub prelude: Vec<String>,
    pub load: LoadSettings,
}

/// A row that was not imported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RowError {
    /// The row's number in the file, from 1, the header not counted.
    pub row: u64,
    /// The line it starts on.
    pub line: u64,
    /// The column whose field could not be converted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    /// Why: `columns` (another number of fields), `notANumber`, `notABoolean`, `notADate`, or the server's error code (`server` when it sent none).
    pub code: String,
    /// What the server said; empty for a field that could not be converted.
    pub detail: String,
}

/// An import as views and agents read it; never its rows.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportState {
    /// Its id, the operation's.
    pub id: u64,
    /// The connection's id.
    pub connection: String,
    /// The file.
    pub file: PathBuf,
    /// The target table's name.
    pub table: String,
    /// Where it is.
    pub status: OperationStatus,
    /// Milliseconds it ran, or has run so far.
    pub elapsed_ms: u64,
    /// The file's size.
    pub file_bytes: u64,
    /// Bytes of the file read so far.
    pub bytes_read: u64,
    /// Rows read from the file.
    pub rows_read: u64,
    /// Rows the server took, committed or not.
    pub rows_inserted: u64,
    /// Rows committed; those inserted after the last commit are rolled back when it fails or is cancelled.
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

#[derive(Default)]
pub(super) struct Tally {
    pub(super) bytes: AtomicU64,
    pub(super) read: AtomicU64,
    pub(super) inserted: AtomicU64,
    pub(super) committed: AtomicU64,
    pub(super) skipped: AtomicU64,
    pub(super) server_copy: AtomicBool,
    errors: Mutex<(Vec<RowError>, u64)>,
}

impl Tally {
    fn note(&self, error: RowError) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
        let mut errors = self.errors.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        errors.1 += 1;
        if errors.0.len() < KEPT_ERRORS {
            errors.0.push(error);
        }
    }

    pub(super) fn errors(&self) -> (Vec<RowError>, u64) {
        self.errors.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

pub(super) struct Job {
    state: Mutex<ImportState>,
    tally: Tally,
    started: Instant,
}

impl Job {
    fn with(&self, change: impl FnOnce(&mut ImportState)) {
        change(&mut self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    }

    fn snapshot(&self) -> ImportState {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let tally = &self.tally;
        state.bytes_read = tally.bytes.load(Ordering::Relaxed);
        state.rows_read = tally.read.load(Ordering::Relaxed);
        state.rows_inserted = tally.inserted.load(Ordering::Relaxed);
        state.rows_committed = tally.committed.load(Ordering::Relaxed);
        state.rows_skipped = tally.skipped.load(Ordering::Relaxed);
        state.server_copy = tally.server_copy.load(Ordering::Relaxed);
        (state.errors, state.errors_total) = tally.errors();
        if state.status == OperationStatus::Running {
            state.elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        }
        state
    }
}

pub(super) struct Row {
    pub(super) row: u64,
    pub(super) line: u64,
    pub(super) cells: Vec<Cell>,
}

pub(super) enum Fed {
    Batch { rows: Vec<Row>, errors: Vec<RowError>, bytes: u64, read: u64 },
    Failed(DbError),
}

pub(super) struct Load<'a> {
    pub(super) connection: &'a str,
    pub(super) dialect: SqlDialect,
    pub(super) table: &'a str,
    pub(super) names: Vec<String>,
    pub(super) prelude: &'a [String],
    pub(super) load: &'a LoadSettings,
    pub(super) context: &'a str,
}

pub(super) const QUEUED_BATCHES: usize = QUEUED;

fn io_error(error: &std::io::Error) -> DbError {
    DbError::new(ErrorKind::Invalid, error.to_string()).with_code("io")
}

fn row_failed(error: RowError) -> DbError {
    let detail = if error.detail.is_empty() { error.code.clone() } else { error.detail.clone() };
    DbError::new(ErrorKind::Invalid, format!("row {}: {detail}", error.row)).with_code("rowFailed")
}

fn fatal(error: &DbError) -> bool {
    matches!(
        error.kind,
        ErrorKind::Connection
            | ErrorKind::Cancelled
            | ErrorKind::Timeout
            | ErrorKind::NotConnected
            | ErrorKind::Refused
    )
}

struct Reading {
    file: PathBuf,
    format: CsvFormat,
    columns: Vec<ColumnMapping>,
    batch: usize,
    stop: bool,
}

fn read_file(reading: &Reading, sender: &tokio::sync::mpsc::Sender<Fed>, token: &CancellationToken) {
    let file = match std::fs::File::open(&reading.file) {
        Ok(file) => file,
        Err(error) => {
            let _ = sender.blocking_send(Fed::Failed(io_error(&error)));
            return;
        }
    };
    let mut reader = CsvReader::new(std::io::BufReader::with_capacity(256 * 1024, file), &reading.format);
    let pattern = reading.format.pattern();
    let mut width = None;
    if reading.format.header {
        match reader.next_record() {
            Ok(Some(header)) => width = Some(header.fields.len()),
            Ok(None) => {}
            Err(error) => {
                let _ = sender.blocking_send(Fed::Failed(io_error(&error)));
                return;
            }
        }
    }
    let (mut rows, mut errors) = (Vec::new(), Vec::new());
    let (mut number, mut sent_bytes, mut read) = (0u64, 0u64, 0u64);
    let mut stopped = false;
    loop {
        if token.is_cancelled() {
            return;
        }
        let record = match reader.next_record() {
            Ok(Some(record)) => record,
            Ok(None) => break,
            Err(error) => {
                let _ = sender.blocking_send(Fed::Failed(io_error(&error)));
                return;
            }
        };
        number += 1;
        read += 1;
        let expected = *width.get_or_insert(record.fields.len());
        let converted = if record.fields.len() == expected {
            reading
                .columns
                .iter()
                .map(|column| {
                    convert(column.kind, record.value(column.source, &reading.format), pattern.as_deref()).map_err(
                        |failed| RowError {
                            row: number,
                            line: record.line,
                            column: Some(column.target.clone()),
                            code: failed.code.to_owned(),
                            detail: String::new(),
                        },
                    )
                })
                .collect::<Result<Vec<Cell>, RowError>>()
        } else {
            Err(RowError {
                row: number,
                line: record.line,
                column: None,
                code: "columns".to_owned(),
                detail: String::new(),
            })
        };
        match converted {
            Ok(cells) => rows.push(Row { row: number, line: record.line, cells }),
            Err(error) => {
                errors.push(error);
                stopped = reading.stop;
            }
        }
        if rows.len() >= reading.batch || stopped {
            let bytes = reader.bytes() - std::mem::replace(&mut sent_bytes, reader.bytes());
            let batch = Fed::Batch {
                rows: std::mem::take(&mut rows),
                errors: std::mem::take(&mut errors),
                bytes,
                read: std::mem::take(&mut read),
            };
            if sender.blocking_send(batch).is_err() || stopped {
                return;
            }
        }
    }
    let bytes = reader.bytes() - sent_bytes;
    let _ = sender.blocking_send(Fed::Batch { rows, errors, bytes, read });
}

struct Plan<'a> {
    target: &'a Load<'a>,
    copy: bool,
    transactional: bool,
}

impl Engine {
    pub fn import_csv(&self, request: ImportRequest, provenance: Provenance) -> Result<u64, DbError> {
        if request.columns.is_empty() {
            return Err(DbError::new(ErrorKind::Invalid, "no column to import").with_code("noColumns"));
        }
        if let Some(problem) = request.format.problem() {
            return Err(DbError::new(ErrorKind::Invalid, problem).with_code(problem));
        }
        if self.read_only(&request.connection) {
            return Err(DbError::new(ErrorKind::Refused, "INSERT").with_code("readOnly"));
        }
        let file_bytes = std::fs::metadata(&request.file).map_err(|error| io_error(&error))?.len();
        let (op, token) = self.begin(OperationKind::Import, Some(&request.connection), None);
        let job = Arc::new(Job {
            state: Mutex::new(ImportState {
                id: op,
                connection: request.connection.clone(),
                file: request.file.clone(),
                table: request.name.clone(),
                status: OperationStatus::Running,
                elapsed_ms: 0,
                file_bytes,
                bytes_read: 0,
                rows_read: 0,
                rows_inserted: 0,
                rows_committed: 0,
                rows_skipped: 0,
                server_copy: false,
                errors: Vec::new(),
                errors_total: 0,
                error: None,
            }),
            tally: Tally::default(),
            started: Instant::now(),
        });
        {
            let mut state = self.state();
            state.imports.insert(op, Arc::clone(&job));
            while state.imports.len() > KEPT_IMPORTS {
                let Some(oldest) = state
                    .imports
                    .iter()
                    .find(|(_, job)| job.state.lock().is_ok_and(|state| state.status != OperationStatus::Running))
                    .map(|(id, _)| *id)
                else {
                    break;
                };
                state.imports.remove(&oldest);
            }
        }
        let engine = self.clone();
        self.spawn(op, async move {
            let ran = engine.import_run(op, &request, &job, &token, provenance).await;
            let status = match &ran {
                Ok(()) => OperationStatus::Done,
                Err(error) if error.kind == ErrorKind::Cancelled => OperationStatus::Cancelled,
                Err(_) => OperationStatus::Failed,
            };
            job.with(|state| {
                state.status = status;
                state.elapsed_ms = u64::try_from(job.started.elapsed().as_millis()).unwrap_or(u64::MAX);
                state.error = ran.as_ref().err().filter(|error| error.kind != ErrorKind::Cancelled).cloned();
            });
            ran.map(|()| Outcome::Nothing)
        });
        Ok(op)
    }

    async fn import_run(
        &self,
        op: u64,
        request: &ImportRequest,
        job: &Arc<Job>,
        token: &CancellationToken,
        provenance: Provenance,
    ) -> Result<(), DbError> {
        let (sender, receiver) = tokio::sync::mpsc::channel(QUEUED);
        let reading = Reading {
            file: request.file.clone(),
            format: request.format.clone(),
            columns: request.columns.clone(),
            batch: request.load.batch.clamp(1, 100_000),
            stop: request.load.on_error == OnError::Stop,
        };
        let reader_token = token.child_token();
        let stop_reader = reader_token.clone();
        std::thread::Builder::new()
            .name("zen-db-import".to_owned())
            .spawn(move || read_file(&reading, &sender, &reader_token))
            .map_err(|error| io_error(&error))?;
        let mut provenance = provenance;
        provenance.source = Source::Import;
        provenance.place =
            provenance.place.or_else(|| request.file.file_name().map(|name| name.to_string_lossy().into_owned()));
        let target = Load {
            connection: &request.connection,
            dialect: request.dialect,
            table: &request.table,
            names: request.columns.iter().map(|column| column.target.clone()).collect(),
            prelude: &request.prelude,
            load: &request.load,
            context: "import",
        };
        let loaded = self.load_rows(op, &target, receiver, &job.tally, token, &provenance).await;
        stop_reader.cancel();
        loaded
    }

    pub(super) async fn load_rows(
        &self,
        op: u64,
        target: &Load<'_>,
        receiver: tokio::sync::mpsc::Receiver<Fed>,
        tally: &Tally,
        token: &CancellationToken,
        provenance: &Provenance,
    ) -> Result<(), DbError> {
        let id = target.connection;
        let wanted = ContextKind::Console(format!("{}-{op}", target.context));
        let (kind, cell) = self.connected_cell(id, &wanted, token).await?;
        let own = kind == wanted;
        let driver = self.state().connections.get(id).map(|conn| Arc::clone(&conn.driver));
        let driver = driver.ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        self.busy(id, &kind, 1);
        let ran = async {
            let mut ctx = tokio::select! {
                ctx = cell.lock_owned() => ctx,
                () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
            };
            if own && ctx.session.is_none() {
                ctx.tracked.auto_commit = true;
                ctx.tracked.read_only = false;
            }
            self.ready(id, &kind, &mut ctx, op).await?;
            if !own && ctx.session.as_ref().is_some_and(|session| session.in_transaction()) {
                return Err(DbError::new(ErrorKind::Invalid, "a transaction is open").with_code("inTransaction"));
            }
            let plan = Plan {
                target,
                copy: target.load.server_copy && target.dialect == SqlDialect::Postgres,
                transactional: driver.capabilities().transactions,
            };
            let loaded = self.import_load(&mut ctx, &kind, plan, receiver, tally, token, provenance).await;
            if let Some(session) = ctx.session.as_mut() {
                if session.in_transaction() {
                    let _ = session.rollback().await;
                }
                let _ = session.take_notices();
            }
            ctx.last_used = Instant::now();
            if !own {
                self.update_info(id, &kind, &ctx);
            }
            loaded
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
        if !target.prelude.is_empty() {
            self.push_event(Event::StructureChanged { connection: id.to_owned() });
        }
        ran
    }

    #[allow(clippy::too_many_arguments, reason = "one load into its session and where its rows come from")]
    async fn import_load(
        &self,
        ctx: &mut Ctx,
        kind: &ContextKind,
        mut plan: Plan<'_>,
        mut receiver: tokio::sync::mpsc::Receiver<Fed>,
        tally: &Tally,
        token: &CancellationToken,
        provenance: &Provenance,
    ) -> Result<(), DbError> {
        let target = plan.target;
        let schema = ctx.tracked.schema.clone();
        let record = |text: String, started: SystemTime, duration: Duration, affected, error| {
            self.record(Ran {
                connection: target.connection.to_owned(),
                context: kind.clone(),
                schema: schema.clone(),
                text,
                started,
                duration,
                rows: None,
                affected,
                error,
                provenance: provenance.clone(),
                params: Vec::new(),
            });
        };
        let ddl_commits = matches!(target.dialect, SqlDialect::MySql | SqlDialect::MariaDb);
        for (at, statement) in target.prelude.iter().enumerate() {
            if at == 0 && !ddl_commits {
                self.import_begin(ctx, &mut plan, token).await?;
            }
            let (started, clock) = (SystemTime::now(), Instant::now());
            let ran = self.import_exec(ctx, statement, token).await;
            record(statement.clone(), started, clock.elapsed(), None, ran.as_ref().err().cloned());
            ran?;
        }
        if target.prelude.is_empty() || ddl_commits {
            self.import_begin(ctx, &mut plan, token).await?;
        }
        let (started, clock) = (SystemTime::now(), Instant::now());
        let mut since_commit = 0u64;
        let stop = target.load.on_error == OnError::Stop;
        let loaded: Result<(), DbError> = async {
            loop {
                let fed = tokio::select! {
                    biased;
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                    fed = receiver.recv() => fed,
                };
                let Some(fed) = fed else { return Ok(()) };
                let (rows, errors, bytes, read) = match fed {
                    Fed::Batch { rows, errors, bytes, read } => (rows, errors, bytes, read),
                    Fed::Failed(error) => return Err(error),
                };
                tally.bytes.fetch_add(bytes, Ordering::Relaxed);
                tally.read.fetch_add(read, Ordering::Relaxed);
                let first = errors.first().cloned();
                for error in errors {
                    tally.note(error);
                }
                if !rows.is_empty() {
                    let inserted = self.import_batch(ctx, &mut plan, &rows, tally, token).await?;
                    since_commit += inserted;
                }
                if let Some(first) = first.filter(|_| stop) {
                    return Err(row_failed(first));
                }
                if plan.transactional && target.load.commit_every > 0 && since_commit >= target.load.commit_every {
                    self.import_commit(ctx, tally, &mut since_commit, token).await?;
                    self.import_exec(ctx, "BEGIN", token).await?;
                }
                if !plan.transactional {
                    tally.committed.fetch_add(std::mem::take(&mut since_commit), Ordering::Relaxed);
                }
            }
        }
        .await;
        drop(receiver);
        let ended = match loaded {
            Ok(()) if plan.transactional => self.import_commit(ctx, tally, &mut since_commit, token).await,
            other => other,
        };
        let head = if plan.copy {
            copy_statement(target.table, &target.names)
        } else {
            insert_head(target.dialect, target.table, &target.names)
        };
        tally.server_copy.store(plan.copy, Ordering::Relaxed);
        let inserted = tally.inserted.load(Ordering::Relaxed);
        record(head, started, clock.elapsed(), Some(inserted), ended.as_ref().err().cloned());
        ended
    }

    async fn import_begin(&self, ctx: &mut Ctx, plan: &mut Plan<'_>, token: &CancellationToken) -> Result<(), DbError> {
        if !plan.transactional {
            return Ok(());
        }
        match self.import_exec(ctx, "BEGIN", token).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind == ErrorKind::Unsupported => {
                plan.transactional = false;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn import_commit(
        &self,
        ctx: &mut Ctx,
        tally: &Tally,
        since_commit: &mut u64,
        token: &CancellationToken,
    ) -> Result<(), DbError> {
        if token.is_cancelled() {
            return Err(DbError::new(ErrorKind::Cancelled, ""));
        }
        self.import_exec(ctx, "COMMIT", token).await?;
        tally.committed.fetch_add(std::mem::take(since_commit), Ordering::Relaxed);
        Ok(())
    }

    async fn import_batch(
        &self,
        ctx: &mut Ctx,
        plan: &mut Plan<'_>,
        rows: &[Row],
        tally: &Tally,
        token: &CancellationToken,
    ) -> Result<u64, DbError> {
        let savepoint = plan.transactional;
        if savepoint {
            self.import_exec(ctx, &format!("SAVEPOINT {SAVEPOINT}"), token).await?;
        }
        let whole = self.import_write(ctx, plan, rows, token).await;
        match whole {
            Ok(()) => {
                if savepoint {
                    self.import_exec(ctx, &format!("RELEASE SAVEPOINT {SAVEPOINT}"), token).await?;
                }
                tally.inserted.fetch_add(rows.len() as u64, Ordering::Relaxed);
                return Ok(rows.len() as u64);
            }
            Err(error) if fatal(&error) || ctx.session.is_none() => return Err(error),
            Err(_) => {
                if savepoint {
                    self.import_exec(ctx, &format!("ROLLBACK TO SAVEPOINT {SAVEPOINT}"), token).await?;
                    self.import_exec(ctx, &format!("RELEASE SAVEPOINT {SAVEPOINT}"), token).await?;
                }
            }
        }
        let mut inserted = 0;
        for row in rows {
            if savepoint {
                self.import_exec(ctx, &format!("SAVEPOINT {SAVEPOINT}"), token).await?;
            }
            let written = self.import_write(ctx, plan, std::slice::from_ref(row), token).await;
            match written {
                Ok(()) => {
                    if savepoint {
                        self.import_exec(ctx, &format!("RELEASE SAVEPOINT {SAVEPOINT}"), token).await?;
                    }
                    inserted += 1;
                    tally.inserted.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) if fatal(&error) || ctx.session.is_none() => return Err(error),
                Err(error) => {
                    if savepoint {
                        self.import_exec(ctx, &format!("ROLLBACK TO SAVEPOINT {SAVEPOINT}"), token).await?;
                        self.import_exec(ctx, &format!("RELEASE SAVEPOINT {SAVEPOINT}"), token).await?;
                    }
                    let failed = RowError {
                        row: row.row,
                        line: row.line,
                        column: None,
                        code: error.code.clone().unwrap_or_else(|| "server".to_owned()),
                        detail: error.detail.clone(),
                    };
                    tally.note(failed.clone());
                    if plan.target.load.on_error == OnError::Stop {
                        return Err(row_failed(failed));
                    }
                }
            }
        }
        Ok(inserted)
    }

    async fn import_write(
        &self,
        ctx: &mut Ctx,
        plan: &mut Plan<'_>,
        rows: &[Row],
        token: &CancellationToken,
    ) -> Result<(), DbError> {
        let cells: Vec<Vec<Cell>> = rows.iter().map(|row| row.cells.clone()).collect();
        let target = plan.target;
        if plan.copy {
            let statement = copy_statement(target.table, &target.names);
            match self.import_copy(ctx, &statement, copy_rows(&cells), token).await {
                Err(error) if error.kind == ErrorKind::Unsupported => plan.copy = false,
                other => return other,
            }
        }
        let statement = insert_statement(target.dialect, target.table, &target.names, &cells);
        self.import_exec(ctx, &statement, token).await
    }

    async fn import_exec(&self, ctx: &mut Ctx, statement: &str, token: &CancellationToken) -> Result<(), DbError> {
        let options = ExecOptions { max_rows: 0, timeout: None };
        self.cancellable(ctx, statement, &options, (token, true)).await.map(|_| ())
    }

    async fn import_copy(
        &self,
        ctx: &mut Ctx,
        statement: &str,
        data: Vec<u8>,
        token: &CancellationToken,
    ) -> Result<(), DbError> {
        let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        let canceller: Option<Arc<dyn Canceller>> = session.canceller();
        let wait = self.shared.timing.cancel_wait;
        let mut close = false;
        let result = {
            let run = session.copy_in(statement, data);
            tokio::pin!(run);
            tokio::select! {
                result = &mut run => result.map(|_| ()),
                () = token.cancelled() => {
                    let started = Instant::now();
                    let asked = match &canceller {
                        Some(canceller) => matches!(tokio::time::timeout(wait, canceller.cancel()).await, Ok(Ok(()))),
                        None => false,
                    };
                    let left = wait.saturating_sub(started.elapsed());
                    if !(asked && tokio::time::timeout(left, &mut run).await.is_ok()) {
                        close = true;
                    }
                    Err(DbError::new(ErrorKind::Cancelled, ""))
                }
            }
        };
        if close {
            ctx.session = None;
        }
        result
    }

    pub fn import_state(&self, id: u64) -> Option<ImportState> {
        let job = self.state().imports.get(&id).cloned()?;
        Some(job.snapshot())
    }

    pub fn imports(&self) -> Vec<u64> {
        self.state().imports.keys().copied().collect()
    }
}
