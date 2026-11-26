use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::script::Work;
use super::{Engine, OperationKind, OperationStatus, Outcome};
use crate::driver::{BoxFuture, ByteSink, ContextKind, DbError, ErrorKind, RowSink, StreamOptions};
use crate::export::{CopyOptions, CopyRewriter, Format, Settings, Target, Writer};
use crate::history::{Provenance, Ran, Source};
use crate::parquet_writer::ParquetWriter;
use crate::value::{Column, Value};

const PAGE: usize = 2000;
const QUEUED: usize = 16;
const KEPT_EXPORTS: usize = 50;

pub const TEXT_LIMIT: usize = 64 * 1024 * 1024;

type StatementRun = (ContextKind, Option<String>, SystemTime, Duration);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    File { path: PathBuf, overwrite: bool },
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportItem {
    pub connection: String,
    pub context: Option<ContextKind>,
    pub statement: String,
    pub name: String,
    pub table: String,
    pub dialect: crate::dialect::SqlDialect,
    pub output: Output,
}

/// Where an exported item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    /// Not started.
    Pending,
    /// Being written.
    Running,
    /// Written.
    Done,
    /// Failed; `error` says why.
    Failed,
    /// Cancelled; nothing was left behind.
    Cancelled,
}

/// One item of an export as views and agents read it; never its rows.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportItemState {
    /// What it is.
    pub name: String,
    /// The connection's id.
    pub connection: String,
    /// The file it goes to; absent for text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Where it is.
    pub status: ItemStatus,
    /// Rows written so far.
    pub rows: u64,
    /// Bytes written so far.
    pub bytes: u64,
    /// Whether the server wrote it (PostgreSQL's `COPY`).
    pub server_copy: bool,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
}

/// An export as views and agents read it; never its rows.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportState {
    /// Its id, the operation's.
    pub id: u64,
    /// The format.
    pub format: Format,
    /// Where it is: running until its last item ends; failed when an item failed.
    pub status: OperationStatus,
    /// Milliseconds it ran, or has run so far.
    pub elapsed_ms: u64,
    /// The item being written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<usize>,
    /// Rows written by all items.
    pub rows: u64,
    /// Bytes written by all items.
    pub bytes: u64,
    /// Its items in order.
    pub items: Vec<ExportItemState>,
    /// Whether it went into text that is waiting to be taken.
    pub text: bool,
}

#[derive(Default)]
struct Counters {
    rows: AtomicU64,
    bytes: AtomicU64,
}

pub(super) struct Job {
    state: Mutex<ExportState>,
    counters: Vec<Arc<Counters>>,
    started: Instant,
    text: Mutex<Option<String>>,
    provenance: Provenance,
}

impl Job {
    fn with(&self, change: impl FnOnce(&mut ExportState)) {
        change(&mut self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    }

    fn snapshot(&self) -> ExportState {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        for (item, counters) in state.items.iter_mut().zip(&self.counters) {
            item.rows = counters.rows.load(Ordering::Relaxed);
            item.bytes = counters.bytes.load(Ordering::Relaxed);
        }
        state.rows = state.items.iter().map(|item| item.rows).sum();
        state.bytes = state.items.iter().map(|item| item.bytes).sum();
        if state.status == OperationStatus::Running {
            state.elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        }
        state.text = self.text.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some();
        state
    }
}

enum Chunk {
    Columns(Vec<Column>, bool),
    Rows(Vec<Vec<Value>>),
    Bytes(Vec<u8>),
}

#[derive(Default)]
struct Pipe {
    queued: AtomicUsize,
    notify: tokio::sync::Notify,
    failed: AtomicBool,
}

struct Feed {
    sender: std::sync::mpsc::Sender<Chunk>,
    pipe: Arc<Pipe>,
    token: CancellationToken,
    columns: bool,
}

impl Feed {
    fn stopped(&self) -> bool {
        self.token.is_cancelled() || self.pipe.failed.load(Ordering::SeqCst)
    }

    fn send(&mut self, chunk: Chunk) -> bool {
        if self.stopped() {
            return false;
        }
        self.pipe.queued.fetch_add(1, Ordering::SeqCst);
        if self.sender.send(chunk).is_err() {
            self.pipe.failed.store(true, Ordering::SeqCst);
            return false;
        }
        !self.stopped()
    }

    async fn room(&mut self) -> bool {
        loop {
            let notified = self.pipe.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.stopped() {
                return false;
            }
            if self.pipe.queued.load(Ordering::SeqCst) < QUEUED {
                return true;
            }
            tokio::select! {
                () = notified => {}
                () = self.token.cancelled() => {}
            }
        }
    }

    fn columns(&mut self, columns: Vec<Column>, copy: bool) {
        if !columns.is_empty() && !std::mem::replace(&mut self.columns, true) {
            self.send(Chunk::Columns(columns, copy));
        }
    }
}

struct RowsFeed(Feed);

impl RowSink for RowsFeed {
    fn columns(&mut self, columns: Vec<Column>) {
        self.0.columns(columns, false);
    }

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> BoxFuture<'_, usize> {
        Box::pin(async move { if self.0.room().await { PAGE } else { 0 } })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        self.0.send(Chunk::Rows(rows))
    }

    fn cap(&self) -> usize {
        if self.0.stopped() { 0 } else { usize::MAX }
    }
}

struct BytesFeed(Feed);

impl ByteSink for BytesFeed {
    fn columns(&mut self, columns: Vec<Column>) {
        self.0.columns(columns, true);
    }

    fn want(&mut self) -> BoxFuture<'_, bool> {
        Box::pin(self.0.room())
    }

    fn push(&mut self, bytes: Vec<u8>) -> bool {
        self.0.send(Chunk::Bytes(bytes))
    }
}

struct Counted<W> {
    out: W,
    counters: Arc<Counters>,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.out.write(bytes)?;
        self.counters.bytes.fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

struct Limited(Vec<u8>);

impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + bytes.len() > TEXT_LIMIT {
            return Err(std::io::Error::other("tooLarge"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

enum Sink {
    File(std::io::BufWriter<std::fs::File>),
    Text(Limited),
}

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Sink::File(file) => file.write(bytes),
            Sink::Text(text) => text.write(bytes),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Sink::File(file) => file.flush(),
            Sink::Text(text) => text.flush(),
        }
    }
}

enum Written {
    Nothing,
    Rows(Writer<Counted<Sink>>),
    Parquet(Box<ParquetWriter<Counted<Sink>>>),
    Bytes(CopyRewriter, Counted<Sink>),
}

struct Finished {
    sink: Option<Sink>,
    columns: bool,
}

fn write_all(
    chunks: &std::sync::mpsc::Receiver<Chunk>,
    sink: Sink,
    (settings, target): (&Settings, Target),
    pipe: &Pipe,
    counters: &Arc<Counters>,
) -> std::io::Result<Finished> {
    let mut written = Written::Nothing;
    let mut sink = Some(sink);
    let mut failure = None;
    let mut buffer = Vec::new();
    let crlf = settings.line_ending == crate::export::LineEnding::CrLf;
    for chunk in chunks {
        if failure.is_none() {
            let step: std::io::Result<()> = (|| {
                match (chunk, &mut written) {
                    (Chunk::Columns(columns, copy), Written::Nothing) => {
                        let out = Counted {
                            out: sink.take().ok_or_else(|| std::io::Error::other("sink"))?,
                            counters: Arc::clone(counters),
                        };
                        if copy && let Some(options) = settings.copy() {
                            let escape = columns
                                .iter()
                                .map(|column| settings.escape_formulas && crate::export::text_type(&column.type_name))
                                .collect();
                            let mut out = out;
                            if settings.bom {
                                out.write_all("\u{feff}".as_bytes())?;
                            }
                            written = Written::Bytes(CopyRewriter::new(&options, escape, crlf), out);
                        } else if settings.format == Format::Parquet {
                            written = Written::Parquet(Box::new(ParquetWriter::new(out, settings, columns)?));
                        } else {
                            written = Written::Rows(Writer::new(out, settings, columns, target.clone())?);
                        }
                    }
                    (Chunk::Rows(rows), Written::Rows(writer)) => {
                        for row in &rows {
                            writer.row(row)?;
                        }
                        counters.rows.fetch_add(rows.len() as u64, Ordering::Relaxed);
                    }
                    (Chunk::Rows(rows), Written::Parquet(writer)) => {
                        for row in &rows {
                            writer.row(row)?;
                        }
                        counters.rows.fetch_add(rows.len() as u64, Ordering::Relaxed);
                    }
                    (Chunk::Bytes(bytes), Written::Bytes(rewriter, out)) => {
                        let before = rewriter.records();
                        buffer.clear();
                        rewriter.feed(&bytes, &mut buffer);
                        out.write_all(&buffer)?;
                        counters.rows.fetch_add(rewriter.records() - before, Ordering::Relaxed);
                    }
                    _ => {}
                }
                Ok(())
            })();
            if let Err(error) = step {
                pipe.failed.store(true, Ordering::SeqCst);
                failure = Some(error);
            }
        }
        pipe.queued.fetch_sub(1, Ordering::SeqCst);
        pipe.notify.notify_one();
    }
    if let Some(error) = failure {
        return Err(error);
    }
    let (columns, out) = match written {
        Written::Nothing => (false, sink),
        Written::Rows(writer) => (true, Some(writer.finish()?.out)),
        Written::Parquet(writer) => (true, Some(writer.finish()?.out)),
        Written::Bytes(_, mut out) => {
            out.flush()?;
            (true, Some(out.out))
        }
    };
    Ok(Finished { sink: out, columns })
}

pub fn now_stamps() -> (String, String) {
    let now = jiff::Zoned::now();
    (now.strftime("%Y%m%d").to_string(), now.strftime("%Y%m%d%H%M%S").to_string())
}

fn part_path(path: &Path, op: u64) -> PathBuf {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.{op}.part"))
}

fn io_error(error: &std::io::Error) -> DbError {
    if error.to_string() == "tooLarge" {
        return DbError::new(ErrorKind::Invalid, "the text is larger than 64 MiB").with_code("tooLarge");
    }
    DbError::new(ErrorKind::Invalid, error.to_string()).with_code("io")
}

impl Engine {
    pub fn export(&self, items: Vec<ExportItem>, settings: Settings) -> Result<u64, DbError> {
        self.export_from(items, settings, Provenance::of(Source::Export))
    }

    pub fn export_from(
        &self,
        items: Vec<ExportItem>,
        settings: Settings,
        provenance: Provenance,
    ) -> Result<u64, DbError> {
        if items.is_empty() {
            return Err(DbError::new(ErrorKind::Invalid, "nothing to export"));
        }
        if let Some(problem) = settings.problem() {
            return Err(DbError::new(ErrorKind::Invalid, problem).with_code(problem));
        }
        if settings.format.binary() && items.iter().any(|item| item.output == Output::Text) {
            return Err(DbError::new(ErrorKind::Invalid, "a binary format cannot go to text").with_code("binary"));
        }
        let first = items[0].connection.clone();
        let (op, token) = self.begin(OperationKind::Export, Some(&first), None);
        let job = Arc::new(Job {
            state: Mutex::new(ExportState {
                id: op,
                format: settings.format,
                status: OperationStatus::Running,
                elapsed_ms: 0,
                current: None,
                rows: 0,
                bytes: 0,
                items: items
                    .iter()
                    .map(|item| ExportItemState {
                        name: item.name.clone(),
                        connection: item.connection.clone(),
                        path: match &item.output {
                            Output::File { path, .. } => Some(path.clone()),
                            Output::Text => None,
                        },
                        status: ItemStatus::Pending,
                        rows: 0,
                        bytes: 0,
                        server_copy: false,
                        error: None,
                    })
                    .collect(),
                text: false,
            }),
            counters: items.iter().map(|_| Arc::default()).collect(),
            started: Instant::now(),
            text: Mutex::new(None),
            provenance,
        });
        {
            let mut state = self.state();
            state.exports.insert(op, Arc::clone(&job));
            while state.exports.len() > KEPT_EXPORTS {
                let Some(oldest) = state
                    .exports
                    .iter()
                    .find(|(_, job)| job.state.lock().is_ok_and(|state| state.status != OperationStatus::Running))
                    .map(|(id, _)| *id)
                else {
                    break;
                };
                state.exports.remove(&oldest);
            }
        }
        let engine = self.clone();
        self.spawn(op, async move {
            let mut texts = Vec::new();
            let mut failed = None;
            let mut cancelled = false;
            for (index, item) in items.iter().enumerate() {
                if token.is_cancelled() {
                    cancelled = true;
                    break;
                }
                job.with(|state| {
                    state.current = Some(index);
                    state.items[index].status = ItemStatus::Running;
                });
                engine.wake();
                let ran = engine.export_item(op, item, &settings, &job, index, &token).await;
                let status = match &ran {
                    Ok(_) => ItemStatus::Done,
                    Err(error) if error.kind == ErrorKind::Cancelled => ItemStatus::Cancelled,
                    Err(_) => ItemStatus::Failed,
                };
                job.with(|state| {
                    state.items[index].status = status;
                    state.items[index].error = ran.as_ref().err().cloned();
                });
                match ran {
                    Ok(Some(text)) => texts.push(text),
                    Ok(None) => {}
                    Err(error) if error.kind == ErrorKind::Cancelled => {
                        cancelled = true;
                        break;
                    }
                    Err(error) => failed = failed.or(Some(error)),
                }
            }
            if !texts.is_empty() && !cancelled {
                let eol = if settings.line_ending == crate::export::LineEnding::CrLf { "\r\n" } else { "\n" };
                *job.text.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(texts.join(eol));
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
                for item in &mut state.items {
                    if matches!(item.status, ItemStatus::Pending | ItemStatus::Running) {
                        item.status = ItemStatus::Cancelled;
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

    async fn export_item(
        &self,
        op: u64,
        item: &ExportItem,
        settings: &Settings,
        job: &Job,
        index: usize,
        token: &CancellationToken,
    ) -> Result<Option<String>, DbError> {
        let (sink, part) = match &item.output {
            Output::File { path, overwrite } => {
                if !overwrite && path.exists() {
                    return Err(DbError::new(ErrorKind::Invalid, path.display().to_string()).with_code("exists"));
                }
                let part = part_path(path, op);
                let file = std::fs::File::create(&part).map_err(|error| io_error(&error))?;
                (Sink::File(std::io::BufWriter::with_capacity(256 * 1024, file)), Some(part))
            }
            Output::Text => (Sink::Text(Limited(Vec::new())), None),
        };
        let ran = self.export_rows(op, item, settings, job, index, sink, token).await;
        let cleanup = |part: &Option<PathBuf>| {
            if let Some(part) = part {
                let _ = std::fs::remove_file(part);
            }
        };
        let finished = match ran {
            Ok(finished) => finished,
            Err(error) => {
                cleanup(&part);
                return Err(error);
            }
        };
        match (finished, &item.output, part) {
            (Sink::File(file), Output::File { path, .. }, Some(part)) => {
                let synced = file.into_inner().map_err(|error| io_error(error.error())).and_then(|file| {
                    file.sync_all().map_err(|error| io_error(&error))?;
                    std::fs::rename(&part, path).map_err(|error| io_error(&error))
                });
                if let Err(error) = synced {
                    cleanup(&Some(part));
                    return Err(error);
                }
                Ok(None)
            }
            (Sink::Text(text), _, _) => Ok(Some(String::from_utf8_lossy(&text.0).into_owned())),
            (_, _, part) => {
                cleanup(&part);
                Err(DbError::new(ErrorKind::Invalid, "output"))
            }
        }
    }

    #[allow(clippy::too_many_arguments, reason = "one item of a running export and where it goes")]
    async fn export_rows(
        &self,
        op: u64,
        item: &ExportItem,
        settings: &Settings,
        job: &Job,
        index: usize,
        sink: Sink,
        token: &CancellationToken,
    ) -> Result<Sink, DbError> {
        let counters = Arc::clone(&job.counters[index]);
        let pipe = Arc::new(Pipe::default());
        let (sender, receiver) = std::sync::mpsc::channel();
        let (done, finished) = tokio::sync::oneshot::channel();
        let target = Target { dialect: item.dialect, table: item.table.clone(), title: item.name.clone() };
        {
            let (settings, pipe, counters) = (settings.clone(), Arc::clone(&pipe), Arc::clone(&counters));
            std::thread::Builder::new()
                .name("zen-db-export".to_owned())
                .spawn(move || {
                    let written = write_all(&receiver, sink, (&settings, target), &pipe, &counters);
                    let _ = done.send(written);
                })
                .map_err(|error| io_error(&error))?;
        }
        let feed = |sender: std::sync::mpsc::Sender<Chunk>| Feed {
            sender,
            pipe: Arc::clone(&pipe),
            token: token.child_token(),
            columns: false,
        };
        let (streamed, run) =
            self.export_stream(op, item, settings, job, index, (feed(sender.clone()), feed(sender)), token).await;
        let written = finished.await.map_err(|_| DbError::new(ErrorKind::Invalid, "the writer stopped"));
        if let Some((context, schema, started, duration)) = run {
            let mut provenance = job.provenance.clone();
            provenance.place = provenance.place.or_else(|| Some(item.name.clone()));
            self.record(Ran {
                connection: item.connection.clone(),
                context,
                schema,
                text: item.statement.clone(),
                started,
                duration,
                rows: streamed.is_ok().then(|| counters.rows.load(Ordering::Relaxed)),
                affected: None,
                error: streamed.as_ref().err().cloned(),
                provenance,
                params: Vec::new(),
            });
        }
        let written = written?;
        streamed?;
        let written = written.map_err(|error| io_error(&error))?;
        if !written.columns {
            return Err(DbError::new(ErrorKind::Invalid, "the statement returns no rows").with_code("noRows"));
        }
        written.sink.ok_or_else(|| DbError::new(ErrorKind::Invalid, "output"))
    }

    #[allow(clippy::too_many_arguments, reason = "one item of a running export and its two feeds")]
    async fn export_stream(
        &self,
        op: u64,
        item: &ExportItem,
        settings: &Settings,
        job: &Job,
        index: usize,
        (copy_feed, rows_feed): (Feed, Feed),
        token: &CancellationToken,
    ) -> (Result<(), DbError>, Option<StatementRun>) {
        let id = item.connection.as_str();
        let wanted = item.context.clone().unwrap_or_else(|| ContextKind::Console(format!("export-{op}")));
        if item.context.is_some() {
            for open in self.shared.results.open_in(id, &wanted) {
                open.close();
            }
        }
        let (kind, cell) = match self.connected_cell(id, &wanted, token).await {
            Ok(found) => found,
            Err(error) => return (Err(error), None),
        };
        let own = item.context.is_none() && kind == wanted;
        self.busy(id, &kind, 1);
        let mut run = None;
        let ran = async {
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
            let mut done = None;
            let mut rows_feed = Some(rows_feed);
            if let Some(options) = settings.copy() {
                let mut sink = BytesFeed(copy_feed);
                let copied = self.copy_guarded(&mut ctx, &item.statement, &options, &mut sink, token).await;
                match copied {
                    Err(error) if error.kind == ErrorKind::Unsupported && !sink.0.columns => drop(sink),
                    other => {
                        job.with(|state| state.items[index].server_copy = true);
                        done = Some(other);
                        rows_feed = None;
                    }
                }
            } else {
                drop(copy_feed);
            }
            let ended = match (done, rows_feed) {
                (Some(done), _) => done,
                (None, Some(feed)) => {
                    let mut sink = RowsFeed(feed);
                    let options = StreamOptions { page: PAGE, skip: 0 };
                    let streamed = self
                        .stream_guarded(&mut ctx, &item.statement, Work::Rows(&options, &mut sink), None, token)
                        .await;
                    drop(sink);
                    streamed
                }
                (None, None) => Err(DbError::new(ErrorKind::Invalid, "export")),
            };
            run = Some((kind.clone(), ctx.tracked.schema.clone(), started, clock.elapsed()));
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
            let ended = ended?;
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
        (ran, run)
    }

    async fn copy_guarded(
        &self,
        ctx: &mut super::Ctx,
        statement: &str,
        options: &CopyOptions,
        sink: &mut BytesFeed,
        token: &CancellationToken,
    ) -> Result<crate::driver::StreamEnd, DbError> {
        self.stream_guarded(ctx, statement, Work::Copy(options, sink), None, token).await
    }

    pub fn export_state(&self, id: u64) -> Option<ExportState> {
        let job = self.state().exports.get(&id).cloned()?;
        Some(job.snapshot())
    }

    pub fn exports(&self) -> Vec<u64> {
        self.state().exports.keys().copied().collect()
    }

    pub fn take_export_text(&self, id: u64) -> Option<String> {
        let job = self.state().exports.get(&id).cloned()?;
        job.text.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }
}
