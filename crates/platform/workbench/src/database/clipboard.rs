use std::cell::RefCell;
use std::io::Write;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::clip::{self, CodeColumns, CodeWriter, CopyFormat, CopySettings, InList};
use zen_db::dialect::SqlDialect;
use zen_db::edit::{self, Editing, Generate, Staged};
use zen_db::export::{self, Target};
use zen_db::results::{Pending, ResultSet};
use zen_db::value::Value;
use zen_i18n::t;

use super::edits::{editable, input_error, known, not_editable, state_of};
use super::filters::result_or_active;
use super::sources::Database;
use crate::workspaces::Workspaces;

const MOST_ROWS: usize = 100_000;
const MOST_BYTES: usize = 64 * 1024 * 1024;
const ANSWERED_CHARS: usize = 100_000;
const IN_BACKGROUND: usize = 50_000;
const PAGE: usize = 1_000;
const KEPT_JOBS: usize = 10;
const MOST_PASTED: usize = 100_000;
const PROBLEMS: usize = 50;
const PREVIEWED: usize = 20;

/// The options kept in `<data folder>/databases/copy.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Remembered {
    /// The file's version, 1.
    pub version: u32,
    /// Advanced Copy's options.
    pub settings: CopySettings,
    /// The format Copy As used last.
    pub format: CopyFormat,
    /// What a pasted field is read as `NULL` from; an empty field when empty.
    pub paste_null_text: String,
}

impl Default for Remembered {
    fn default() -> Self {
        Self {
            version: 1,
            settings: CopySettings { header: true, ..CopySettings::default() },
            format: CopyFormat::Csv,
            paste_null_text: String::new(),
        }
    }
}

/// How a copy ended early.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Cut {
    /// At 100 000 rows.
    Rows,
    /// At 64 MiB of text.
    Size,
    /// At the characters an answer holds (`limit`).
    Limit,
    /// Cancelled.
    Cancelled,
}

/// A copy running in the background or ended.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyJob {
    /// Its id.
    pub job: u64,
    /// The result it copies.
    pub result: u64,
    /// `running`, `done`, `failed` (a file could not be written) or `cancelled`.
    pub status: String,
    /// The rows written so far.
    pub rows: usize,
    /// The rows selected.
    pub total_rows: usize,
    /// Why it stopped before the last row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut: Option<Cut>,
    /// What to tell the person, when anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

struct Running {
    job: CopyJob,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
    handle: Option<JoinHandle<Rendered>>,
}

#[derive(Default)]
pub struct Copies {
    remembered: Option<Remembered>,
    jobs: Vec<Running>,
    next: u64,
}

fn file_path(session: &Session) -> Option<PathBuf> {
    session.state::<Workspaces>().and_then(Workspaces::data_dir).map(|data| data.join("databases").join("copy.json"))
}

pub fn remembered(session: &mut Session) -> Remembered {
    if let Some(remembered) = session.state::<Copies>().and_then(|copies| copies.remembered.clone()) {
        return remembered;
    }
    let read = file_path(session)
        .and_then(|file| zen_db::registry::read_json::<Remembered>(&file).ok().flatten())
        .unwrap_or_default();
    session.state_mut::<Copies>().remembered = Some(read.clone());
    read
}

fn remember(session: &mut Session, remembered: Remembered) -> Result<(), CommandError> {
    if session.state::<Copies>().and_then(|copies| copies.remembered.as_ref()) == Some(&remembered) {
        return Ok(());
    }
    if let Some(path) = file_path(session) {
        zen_db::registry::write_json(&path, &remembered)
            .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
    }
    session.state_mut::<Copies>().remembered = Some(remembered);
    Ok(())
}

fn settings_problem(code: &'static str) -> CommandError {
    let message = match code {
        "emptyDelimiter" => t!("database-copy-error-empty-delimiter"),
        _ => t!("database-copy-error-delimiter-quote"),
    };
    CommandError::with_code(code, message)
}

enum What {
    Text(CopySettings),
    Export(export::Settings),
    Update(Editing),
    In,
    Code(CopyFormat),
    Names,
}

struct Plan {
    result: Arc<ResultSet>,
    first: usize,
    count: usize,
    total: usize,
    columns: Range<usize>,
    names: Vec<String>,
    what: What,
    dialect: SqlDialect,
    target: Target,
    most_bytes: usize,
    file: Option<PathBuf>,
}

struct Rendered {
    text: String,
    rows: usize,
    cut: Option<Cut>,
    written: Option<Result<PathBuf, String>>,
}

#[derive(Clone)]
struct Shared(Rc<RefCell<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn render(plan: &Plan, cancel: &AtomicBool, progress: &AtomicUsize) -> Rendered {
    let mut cut = (plan.count < plan.total).then_some(Cut::Rows);
    let mut rows = 0;
    let pages = |each: &mut dyn FnMut(&[Value], usize) -> bool| -> Option<Cut> {
        let mut offset = 0;
        while offset < plan.count {
            if cancel.load(Ordering::Relaxed) {
                return Some(Cut::Cancelled);
            }
            let wanted = PAGE.min(plan.count - offset);
            let page = plan.result.page(plan.first + offset, wanted);
            if page.is_empty() {
                break;
            }
            for (at, row) in page.iter().enumerate() {
                if !each(row, plan.first + offset + at) {
                    return Some(Cut::Size);
                }
            }
            offset += page.len();
            progress.store(offset, Ordering::Relaxed);
        }
        None
    };
    let columns = |row: &[Value]| -> Vec<Value> { row.get(plan.columns.clone()).unwrap_or_default().to_vec() };
    let text = match &plan.what {
        What::Names => plan.names.join(", "),
        What::Text(settings) => {
            let mut out = String::new();
            if settings.header {
                out.push_str(&settings.header_line(&plan.names));
            }
            let stopped = pages(&mut |row, number| {
                if !out.is_empty() || rows > 0 {
                    out.push_str(&settings.row_delimiter);
                }
                out.push_str(&settings.row_line(number + 1, &columns(row), plan.dialect));
                rows += 1;
                out.len() <= plan.most_bytes
            });
            cut = stopped.or(cut);
            out
        }
        What::Export(settings) => {
            let buffer = Shared(Rc::new(RefCell::new(Vec::new())));
            let table_columns: Vec<zen_db::value::Column> =
                plan.result.columns().get(plan.columns.clone()).unwrap_or_default().to_vec();
            let written = export::Writer::new(buffer.clone(), settings, table_columns, plan.target.clone()).and_then(
                |mut writer| {
                    let mut failed = None;
                    let stopped = pages(&mut |row, _| {
                        if let Err(error) = writer.row(&columns(row)) {
                            failed = Some(error);
                            return false;
                        }
                        rows += 1;
                        buffer.0.borrow().len() <= plan.most_bytes
                    });
                    cut = stopped.or(cut);
                    if let Some(error) = failed {
                        return Err(error);
                    }
                    writer.finish().map(|_| ())
                },
            );
            let bytes = std::mem::take(&mut *buffer.0.borrow_mut());
            if written.is_err() { String::new() } else { String::from_utf8(bytes).unwrap_or_default() }
        }
        What::Update(editing) => {
            let mut out = String::new();
            let stopped = pages(&mut |row, _| {
                let sql = edit::generate(editing, Generate::Update, &[row.to_vec()]).unwrap_or_default();
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&sql);
                rows += 1;
                out.len() <= plan.most_bytes
            });
            cut = stopped.or(cut);
            out
        }
        What::In => {
            let mut list = InList::new(plan.dialect, &plan.names);
            let stopped = pages(&mut |row, _| {
                list.row(&columns(row));
                rows += 1;
                list.len() <= plan.most_bytes
            });
            cut = stopped.or(cut);
            if list.is_empty() { String::new() } else { list.finish() }
        }
        What::Code(format) => {
            let mut known = CodeColumns::new(plan.names.len());
            let learned = pages(&mut |row, _| {
                known.learn(&columns(row));
                true
            });
            if learned == Some(Cut::Cancelled) {
                cut = learned;
                String::new()
            } else {
                progress.store(0, Ordering::Relaxed);
                let writer = CodeWriter::new(*format, &plan.names, known);
                let mut out = writer.start();
                let stopped = pages(&mut |row, _| {
                    out.push_str(&writer.row(&columns(row)));
                    rows += 1;
                    out.len() <= plan.most_bytes
                });
                out.push_str(&writer.finish());
                cut = stopped.or(cut);
                out
            }
        }
    };
    if matches!(plan.what, What::Names) {
        rows = 0;
    }
    Rendered { text, rows, cut, written: None }
}

fn cut_message(cut: Option<Cut>, rows: usize, total: usize) -> Option<String> {
    match cut? {
        Cut::Rows => Some(t!("database-copy-cut-rows", rows = rows, total = total)),
        Cut::Size => Some(t!("database-copy-cut-size", rows = rows, total = total)),
        Cut::Limit => Some(t!("database-copy-cut-limit", rows = rows, total = total)),
        Cut::Cancelled => Some(t!("database-copy-cancelled")),
    }
}

/// Which cells to copy.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Selection {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The first and last row, both included; every row kept when absent.
    pub rows: Option<(usize, usize)>,
    /// The first and last column, both included; every column when absent.
    pub columns: Option<(usize, usize)>,
    /// Put the text on the window's clipboard instead of answering it; a large selection is copied in the background (follow it with db.result.copyState).
    pub clipboard: bool,
    /// The most characters of text answered; 100 000 when absent.
    pub limit: Option<usize>,
}

/// Cells copied.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    /// The text; absent when it went to the clipboard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The rows written.
    pub rows: usize,
    /// The rows selected.
    pub total_rows: usize,
    /// The characters written.
    pub chars: usize,
    /// Whether rows or text were left out.
    pub truncated: bool,
    /// Why: `rows` (100 000 rows), `size` (64 MiB), `limit` (the characters answered) or `cancelled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut: Option<Cut>,
    /// What to tell the person, when anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The background copy, for db.result.copyState and its cancel, when the selection was large.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<u64>,
}

fn last_index(len: usize) -> usize {
    len.saturating_sub(1)
}

fn plan_of(session: &mut Session, selection: &Selection, what: What) -> Result<(Plan, bool, usize), CommandError> {
    let result = result_or_active(session, selection.id)?;
    let columns = result.columns();
    let kept = result.len();
    let (first, last) = selection.rows.unwrap_or((0, last_index(kept)));
    let (left, right) = selection.columns.unwrap_or((0, last_index(columns.len())));
    let last = last.min(last_index(kept));
    let right = right.min(last_index(columns.len()));
    let total = if kept == 0 || first > last { 0 } else { last - first + 1 };
    let range = if columns.is_empty() || left > right { 0..0 } else { left..right + 1 };
    let names: Vec<String> = columns[range.clone()].iter().map(|column| column.name.clone()).collect();
    let dialect = super::filters::dialect_of(session, result.connection()).unwrap_or_default();
    let table = match &what {
        What::Export(_) => {
            let found = known(session, &result);
            found
                .editing
                .table
                .clone()
                .or_else(|| result.table().filter(|table| !table.ends_with("(+)")).map(str::to_owned))
                .unwrap_or_else(|| "table_name".to_owned())
        }
        _ => String::new(),
    };
    let title = result.table().unwrap_or("result").to_owned();
    let limit = if selection.clipboard { usize::MAX } else { selection.limit.unwrap_or(ANSWERED_CHARS) };
    let count = total.min(MOST_ROWS);
    let width = range.len();
    let plan = Plan {
        result,
        first,
        count,
        total,
        columns: range,
        names,
        what,
        dialect,
        target: Target { dialect, table, title },
        most_bytes: MOST_BYTES.min(limit.saturating_mul(4)),
        file: None,
    };
    let background = selection.clipboard && count.saturating_mul(width.max(1)) > IN_BACKGROUND;
    Ok((plan, background, limit))
}

fn copy(session: &mut Session, selection: &Selection, what: What) -> Result<Copied, CommandError> {
    let (plan, background, limit) = plan_of(session, selection, what)?;
    if background {
        return Ok(start_job(session, plan));
    }
    let rendered = render(&plan, &AtomicBool::new(false), &AtomicUsize::new(0));
    let mut text = rendered.text;
    let mut cut = rendered.cut;
    if cut == Some(Cut::Size) && plan.most_bytes < MOST_BYTES {
        cut = Some(Cut::Limit);
    }
    let mut chars = text.chars().count();
    if chars > limit {
        text = text.chars().take(limit).collect();
        chars = limit;
        cut = Some(Cut::Limit);
    }
    let message = cut_message(cut, rendered.rows, plan.total);
    let text = if selection.clipboard {
        crate::file_clipboard::set_clipboard_text(session, &text);
        None
    } else {
        Some(text)
    };
    Ok(Copied {
        text,
        rows: rendered.rows,
        total_rows: plan.total,
        chars,
        truncated: cut.is_some(),
        cut,
        message,
        job: None,
    })
}

fn start_job(session: &mut Session, plan: Plan) -> Copied {
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicUsize::new(0));
    let waker = session.state::<Database>().and_then(Database::waker);
    let copies = session.state_mut::<Copies>();
    copies.next += 1;
    let id = copies.next;
    let job = CopyJob {
        job: id,
        result: plan.result.id(),
        status: "running".to_owned(),
        rows: 0,
        total_rows: plan.total,
        cut: None,
        message: None,
    };
    let total = plan.total;
    let (thread_cancel, thread_progress) = (Arc::clone(&cancel), Arc::clone(&progress));
    let handle = std::thread::Builder::new()
        .name("zen-db-copy".to_owned())
        .spawn(move || {
            let mut rendered = render(&plan, &thread_cancel, &thread_progress);
            if let Some(path) = &plan.file
                && rendered.cut != Some(Cut::Cancelled)
            {
                rendered.written = Some(
                    std::fs::write(path, &rendered.text).map(|()| path.clone()).map_err(|error| error.to_string()),
                );
                rendered.text.clear();
            }
            if let Some(waker) = waker {
                waker();
            }
            rendered
        })
        .ok();
    copies.jobs.push(Running { job, cancel, progress, handle });
    Copied {
        text: None,
        rows: 0,
        total_rows: total,
        chars: 0,
        truncated: false,
        cut: None,
        message: Some(t!("database-copy-running", total = total)),
        job: Some(id),
    }
}

pub fn step(session: &mut Session) {
    if session.state::<Copies>().is_none_or(|copies| copies.jobs.is_empty()) {
        return;
    }
    let copies = session.state_mut::<Copies>();
    let mut texts = Vec::new();
    let mut files = Vec::new();
    for running in &mut copies.jobs {
        running.job.rows = running.progress.load(Ordering::Relaxed);
        if !running.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            continue;
        }
        let Some(rendered) = running.handle.take().and_then(|handle| handle.join().ok()) else {
            running.job.status = "cancelled".to_owned();
            continue;
        };
        running.job.rows = rendered.rows;
        running.job.cut = rendered.cut;
        running.job.message = cut_message(rendered.cut, rendered.rows, running.job.total_rows);
        if rendered.cut == Some(Cut::Cancelled) {
            running.job.status = "cancelled".to_owned();
        } else if let Some(written) = rendered.written {
            running.job.status = if written.is_ok() { "done" } else { "failed" }.to_owned();
            files.push((rendered.rows, written));
        } else {
            running.job.status = "done".to_owned();
            texts.push(rendered.text);
        }
    }
    let ended = copies.jobs.iter().filter(|running| running.handle.is_none()).count();
    if ended > KEPT_JOBS {
        let mut drop = ended - KEPT_JOBS;
        copies.jobs.retain(|running| {
            if drop > 0 && running.handle.is_none() {
                drop -= 1;
                false
            } else {
                true
            }
        });
    }
    for text in texts {
        crate::file_clipboard::set_clipboard_text(session, &text);
    }
    for (rows, written) in files {
        super::exports::announce_cells(session, rows, written);
    }
}

pub(crate) fn export_cells(
    session: &mut Session,
    selection: &Selection,
    mut settings: export::Settings,
    path: Option<PathBuf>,
) -> Result<Copied, CommandError> {
    settings.server_copy = false;
    let Some(path) = path else {
        let selection = Selection { clipboard: true, ..selection.clone() };
        return copy(session, &selection, What::Export(settings));
    };
    let (mut plan, _, _) =
        plan_of(session, &Selection { clipboard: true, ..selection.clone() }, What::Export(settings))?;
    plan.file = Some(path);
    Ok(start_job(session, plan))
}

/// The cells to copy and how.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CopyParams {
    /// Which cells.
    #[serde(flatten)]
    pub selection: Selection,
    /// Put the columns' names first.
    pub header: bool,
    /// Advanced Copy: use the remembered options (db.copy.settings).
    pub advanced: bool,
    /// Advanced Copy with these options.
    pub settings: Option<CopySettings>,
    /// Keep `settings` as Advanced Copy's options.
    pub remember: bool,
}

pub struct CopyCells;

impl Command for CopyCells {
    type Params = CopyParams;
    type Output = Copied;
    const ID: &'static str = "db.result.copy";
    const TITLE: &'static str = "Copy";
    const DESCRIPTION: &'static str = "A rectangle of a result's cells (`id`, else the one shown; `rows` and `columns` as [first, last], every one when absent) as text, staged changes included: plain, as ⌘C copies them, tab-separated with one row a line, a value with a tab, a quote or a line break in quotes, NULL as nothing, the columns' names first when `header`. Advanced Copy (⇧⌘C) takes `settings` (`header`, `rowNumbers`, `columnDelimiter`, `rowDelimiter`, `quote`, `quoting`: needed, always, never; `nullText`, `values`: display, sql, json; kept with `remember`) or the remembered ones with `advanced`. At most 100 000 rows and 64 MiB; the answer holds at most `limit` characters (100 000); `cut` and `message` say what was left out. With `clipboard` the text goes to the clipboard, a large selection in the background (`job`, follow it with db.result.copyState). Refused with code `emptyDelimiter` or `delimiterQuote`.";

    fn run(&self, session: &mut Session, params: CopyParams) -> Result<Copied, CommandError> {
        let mut settings = match (params.settings.clone(), params.advanced) {
            (Some(settings), _) => settings,
            (None, true) => remembered(session).settings,
            (None, false) => CopySettings::default(),
        };
        if let Some(code) = settings.problem() {
            return Err(settings_problem(code));
        }
        if params.remember
            && let Some(kept) = params.settings
        {
            let mut known = remembered(session);
            known.settings = kept;
            remember(session, known)?;
        }
        settings.header |= params.header;
        copy(session, &params.selection, What::Text(settings))
    }
}

/// The cells to copy and the format.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyAsParams {
    /// Which cells.
    #[serde(flatten)]
    pub selection: Selection,
    /// The format.
    pub format: CopyFormat,
    /// Export settings for CSV, TSV, JSON, Markdown, HTML, XML and SQL INSERT (`delimiter`, `quote`, `quoteAll`, `header`, `nullText`, `lineEnding`, `json`, `table`, `batch`); a header, nothing escaped and one INSERT per 100 rows when absent.
    #[serde(default)]
    pub settings: Option<export::Settings>,
}

pub struct CopyAs;

impl Command for CopyAs {
    type Params = CopyAsParams;
    type Output = Copied;
    const ID: &'static str = "db.result.copyAs";
    const TITLE: &'static str = "Copy As";
    const DESCRIPTION: &'static str = "A rectangle of a result's cells (`id`, `rows`, `columns` as db.result.copy) in `format`: `csv`, `tsv`, `json`, `markdown`, `html`, `xml` and `sqlInsert` written as Export writes them (`settings` as db.export.start's, the result's table for INSERTs), `sqlUpdate` (UPDATE of each whole row by its key, needs the rows' table: code `readOnly` otherwise), `sqlIn` (`column IN (…)`, or `(a, b) IN ((…), …)` for several columns, repeats left out), code literals `go`, `rust`, `typeScript`, `python`, `kotlin`, or `columnNames`; values in the connection's dialect. Limits, `clipboard`, `job` and the answer as db.result.copy. The format is remembered for the menu.";

    fn run(&self, session: &mut Session, params: CopyAsParams) -> Result<Copied, CommandError> {
        let what = match params.format {
            CopyFormat::SqlUpdate => {
                let result = result_or_active(session, params.selection.id)?;
                let found = known(session, &result);
                if found.editing.table.is_none() {
                    return Err(not_editable(&found.editing));
                }
                What::Update(found.editing)
            }
            CopyFormat::SqlIn => What::In,
            CopyFormat::ColumnNames => What::Names,
            format if format.code() => What::Code(format),
            format => {
                let export_format = format.export().unwrap_or_default();
                let mut settings = params.settings.clone().unwrap_or_else(|| clip::export_settings(export_format));
                settings.format = export_format;
                settings.server_copy = false;
                if let Some(code) = settings.problem() {
                    return Err(settings_problem(code));
                }
                What::Export(settings)
            }
        };
        let copied = copy(session, &params.selection, what)?;
        if params.format != CopyFormat::ColumnNames {
            let mut known = remembered(session);
            known.format = params.format;
            remember(session, known)?;
        }
        Ok(copied)
    }
}

/// Which background copy.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CopyStateParams {
    /// The copy; every one kept when absent.
    pub job: Option<u64>,
    /// Stop it.
    pub cancel: bool,
}

/// Background copies.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyStates {
    /// The copies, the last 10 that ended and those running.
    pub jobs: Vec<CopyJob>,
}

pub struct CopyState;

impl Command for CopyState {
    type Params = CopyStateParams;
    type Output = CopyStates;
    const ID: &'static str = "db.result.copyState";
    const TITLE: &'static str = "Copy State";
    const DESCRIPTION: &'static str = "The copies db.result.copy and db.result.copyAs run in the background for large selections: `status` (running, done, cancelled), rows written of `totalRows`, `cut` and `message`; a done copy's text is on the clipboard. `job` picks one; `cancel: true` stops it, leaving the clipboard as it was. Never the text.";

    fn run(&self, session: &mut Session, params: CopyStateParams) -> Result<CopyStates, CommandError> {
        if params.cancel {
            for running in &session.state_mut::<Copies>().jobs {
                if Some(running.job.job) == params.job || params.job.is_none() {
                    running.cancel.store(true, Ordering::Relaxed);
                }
            }
        }
        step(session);
        let jobs = session
            .state::<Copies>()
            .map(|copies| {
                copies
                    .jobs
                    .iter()
                    .filter(|running| params.job.is_none_or(|job| running.job.job == job))
                    .map(|running| running.job.clone())
                    .collect()
            })
            .unwrap_or_default();
        if let Some(job) = params.job
            && !session.state::<Copies>().is_some_and(|copies| copies.jobs.iter().any(|running| running.job.job == job))
        {
            return Err(CommandError::with_code("unknownJob", t!("database-copy-error-unknown-job", job = job)));
        }
        Ok(CopyStates { jobs })
    }
}

/// Copy options to keep.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CopySettingsParams {
    /// Advanced Copy's options to keep.
    pub settings: Option<CopySettings>,
    /// The Copy As format to offer first.
    pub format: Option<CopyFormat>,
    /// What a pasted field is read as `NULL` from.
    pub paste_null_text: Option<String>,
}

/// A Copy As format as menus list it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormatEntry {
    /// Its id.
    pub id: CopyFormat,
    /// Its name.
    pub name: String,
}

/// The copy options and the formats.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyOptions {
    /// The remembered options.
    #[serde(flatten)]
    pub remembered: Remembered,
    /// The Copy As formats, in the menu's order.
    pub formats: Vec<FormatEntry>,
}

pub struct CopySettingsCommand;

impl Command for CopySettingsCommand {
    type Params = CopySettingsParams;
    type Output = CopyOptions;
    const ID: &'static str = "db.copy.settings";
    const TITLE: &'static str = "Copy Settings";
    const DESCRIPTION: &'static str = "Advanced Copy's options (`settings`: header, rowNumbers, columnDelimiter, rowDelimiter, quote, quoting, nullText, values), the Copy As format used last and the text a pasted field is read as NULL from, kept in `<data folder>/databases/copy.json`; any given is kept. Answers them with the Copy As formats.";

    fn run(&self, session: &mut Session, params: CopySettingsParams) -> Result<CopyOptions, CommandError> {
        let mut kept = remembered(session);
        if let Some(settings) = params.settings {
            if let Some(code) = settings.problem() {
                return Err(settings_problem(code));
            }
            kept.settings = settings;
        }
        if let Some(format) = params.format {
            kept.format = format;
        }
        if let Some(null_text) = params.paste_null_text {
            kept.paste_null_text = null_text;
        }
        remember(session, kept.clone())?;
        let formats =
            CopyFormat::ALL.iter().map(|format| FormatEntry { id: *format, name: format.name().to_owned() }).collect();
        Ok(CopyOptions { remembered: kept, formats })
    }
}

/// Where pasted rows go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PasteMode {
    /// Over the cells from the anchor right and down; rows past the last are added.
    #[default]
    Overwrite,
    /// As new rows after the others, from the anchor's column.
    Insert,
}

/// Text to paste into a result.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PasteParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The text, TSV or CSV as spreadsheets copy it; the clipboard's when absent.
    pub text: Option<String>,
    /// The anchor's row, from 0.
    pub row: usize,
    /// The anchor's column, from 0.
    pub column: usize,
    /// `overwrite` (the default) or `insert`.
    pub mode: PasteMode,
    /// The delimiter, one character (`\t` for a tab); found by itself when absent.
    pub delimiter: Option<String>,
    /// Whether the first line names the columns and is left out; found by itself when absent.
    pub header: Option<bool>,
    /// What a field not in quotes is read as NULL from; the remembered text (an empty field at first) when absent.
    pub null_text: Option<String>,
    /// Check and count only; stage nothing.
    pub dry_run: bool,
    /// Stage the valid cells and leave out the others; refused with code `invalidPaste` otherwise when a value does not fit.
    pub skip_invalid: bool,
    /// The most problems answered; 50 when absent.
    pub limit: Option<usize>,
    /// The first rows of the text to answer as read, at most 20, for a preview.
    pub preview: usize,
}

/// A value that does not fit its cell.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// The row of the text, from 1, the header not counted.
    pub line: usize,
    /// The result's column, from 0.
    pub column: usize,
    /// The column's name.
    pub name: String,
    /// `invalidValue`, `readOnlyColumn`, `notNull`, `noColumn` or `deletedRow`.
    pub code: String,
    /// Why, in words; never the value.
    pub message: String,
}

/// What a paste did or would do.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Pasted {
    /// The delimiter read with: `\t`, `,`, `;` or another character.
    pub delimiter: String,
    /// Whether the first line was taken for the columns' names.
    pub header: bool,
    /// Rows of the text, the header not counted.
    pub rows: usize,
    /// The most fields a row of the text has.
    pub columns: usize,
    /// Cells staged, or to stage on a dry run.
    pub cells: usize,
    /// Rows read that get new values.
    pub updated_rows: usize,
    /// New rows added.
    pub new_rows: usize,
    /// Fields left out because they do not fit.
    pub skipped: usize,
    /// The first problems.
    pub problems: Vec<Problem>,
    /// How many problems there are.
    pub problems_total: usize,
    /// Whether problems were left out because of `limit`.
    pub truncated: bool,
    /// Whether anything was staged.
    pub applied: bool,
    /// The first rows of the text as read (`preview`), NULL as null.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub preview: Vec<Vec<Option<String>>>,
    /// What is staged now.
    pub pending: Pending,
    /// Whether anything is staged.
    pub dirty: bool,
}

fn delimiter_of(text: Option<&str>) -> Option<char> {
    let text = text?;
    match text {
        "\\t" | "tab" => Some('\t'),
        _ => {
            let mut chars = text.chars();
            chars.next().filter(|_| chars.next().is_none())
        }
    }
}

pub struct Paste;

impl Command for Paste {
    type Params = PasteParams;
    type Output = Pasted;
    const ID: &'static str = "db.result.paste";
    const TITLE: &'static str = "Paste";
    const DESCRIPTION: &'static str = "Stages text from spreadsheets (TSV or CSV from Excel, Numbers, Google Sheets or ⌘C in a grid; `text`, else the clipboard's) in a result (`id`, else the one shown): the delimiter (`\\t`, `,`, `;`, `|`) and a header line naming the columns are found unless `delimiter` and `header` are given; with a header naming the result's columns each value goes to the column it names, in any order, else from `column` on; quoted fields keep delimiters, quotes and line breaks; an unquoted field equal to `nullText` (empty at first) is NULL. `mode: overwrite` writes from the anchor (`row`, `column`) right and down and adds rows past the last; `insert` adds every row as a new row from column `column`. Each value is checked against its column's type, and read-only, NOT NULL and missing columns and deleted rows are reported as `problems` (line, column, code, message; never values); with problems nothing is staged (code `invalidPaste`) unless `skipInvalid`. `dryRun` checks only; `preview` answers the first rows of the text as read. Nothing is written to the server until db.result.save. At most 100 000 cells (code `tooLarge`).";

    fn run(&self, session: &mut Session, params: PasteParams) -> Result<Pasted, CommandError> {
        let result = result_or_active(session, params.id)?;
        let found = editable(session, &result)?;
        let text = match params.text.clone() {
            Some(text) => text,
            None => crate::file_clipboard::clipboard_text(session),
        };
        let null_text = params.null_text.clone().unwrap_or_else(|| remembered(session).paste_null_text);
        let table = clip::parse(&text, delimiter_of(params.delimiter.as_deref()), '"', MOST_PASTED + 2);
        let names: Vec<String> = found.editing.columns.iter().map(|column| column.name.clone()).collect();
        let header = params.header.unwrap_or_else(|| {
            table.rows.len() > 1 && table.rows.first().is_some_and(|row| clip::names_columns(row, &names))
        });
        let named: Option<Vec<usize>> =
            table.rows.first().filter(|row| header && clip::names_columns(row, &names)).map(|row| {
                row.iter()
                    .map(|field| {
                        names
                            .iter()
                            .position(|name| name.eq_ignore_ascii_case(field.text.trim()))
                            .unwrap_or(names.len())
                    })
                    .collect()
            });
        let body = &table.rows[usize::from(header && !table.rows.is_empty()).min(table.rows.len())..];
        let cells: usize = body.iter().map(Vec::len).sum();
        if body.is_empty() || cells == 0 {
            return Err(CommandError::with_code("emptyPaste", t!("database-paste-error-empty")));
        }
        if cells > MOST_PASTED || body.len() > MOST_PASTED {
            return Err(CommandError::with_code("tooLarge", t!("database-paste-error-too-large", most = MOST_PASTED)));
        }
        let deleted: std::collections::BTreeSet<usize> = result
            .marks(0..result.len())
            .iter()
            .enumerate()
            .filter_map(|(row, mark)| matches!(mark, zen_db::results::RowMark::Deleted).then_some(row))
            .collect();
        let shown = result.len();
        let anchor_row = params.row.min(shown);
        let mut problems: Vec<Problem> = Vec::new();
        let mut staged: Vec<(usize, Vec<(usize, Staged)>)> = Vec::new();
        let mut added: Vec<Vec<(usize, Staged)>> = Vec::new();
        let mut skipped = 0;
        for (line, fields) in body.iter().enumerate() {
            let target_row = match params.mode {
                PasteMode::Overwrite if anchor_row + line < shown => Some(anchor_row + line),
                _ => None,
            };
            let mut row_cells = Vec::new();
            for (at, field) in fields.iter().enumerate() {
                let index = match &named {
                    Some(named) => named.get(at).copied().unwrap_or(names.len() + at),
                    None => params.column + at,
                };
                let mut problem = |code: &str, message: String| {
                    problems.push(Problem {
                        line: line + 1,
                        column: index,
                        name: names.get(index).cloned().unwrap_or_default(),
                        code: code.to_owned(),
                        message,
                    });
                };
                let Some(column) = found.editing.columns.get(index) else {
                    if !field.text.is_empty() || field.quoted {
                        problem("noColumn", t!("database-paste-no-column", column = index + 1));
                    }
                    skipped += 1;
                    continue;
                };
                if target_row.is_some_and(|row| deleted.contains(&row)) {
                    problem("deletedRow", t!("database-paste-deleted-row"));
                    skipped += 1;
                    continue;
                }
                if !column.editable {
                    problem("readOnlyColumn", t!("database-error-read-only-column", column = column.name.as_str()));
                    skipped += 1;
                    continue;
                }
                let value = match field.value(&null_text) {
                    None if !column.nullable => {
                        problem("notNull", t!("database-error-not-null", column = column.name.as_str()));
                        skipped += 1;
                        continue;
                    }
                    None => Value::Null,
                    Some(text) => match edit::parse_cell(&found.editing, text, &column.type_name) {
                        Ok(value) => value,
                        Err(error) => {
                            problem("invalidValue", input_error(error));
                            skipped += 1;
                            continue;
                        }
                    },
                };
                row_cells.push((index, Staged::Value(value)));
            }
            match target_row {
                Some(row) => staged.push((row, row_cells)),
                None => added.push(row_cells),
            }
        }
        let problems_total = problems.len();
        let limit = params.limit.unwrap_or(PROBLEMS);
        let truncated = problems.len() > limit;
        problems.truncate(limit);
        let preview: Vec<Vec<Option<String>>> = table
            .rows
            .iter()
            .take(params.preview.min(PREVIEWED))
            .map(|row| row.iter().map(|field| field.value(&null_text).map(str::to_owned)).collect())
            .collect();
        let staged_cells: usize =
            staged.iter().map(|(_, cells)| cells.len()).sum::<usize>() + added.iter().map(Vec::len).sum::<usize>();
        let updated_rows = staged.iter().filter(|(_, cells)| !cells.is_empty()).count();
        let new_rows = added.len();
        if problems_total > 0 && !params.skip_invalid && !params.dry_run {
            return Err(CommandError::with_code(
                "invalidPaste",
                t!("database-paste-error-invalid", count = problems_total),
            )
            .with_data(serde_json::json!({ "problems": problems, "problemsTotal": problems_total })));
        }
        let apply = !params.dry_run && staged_cells + new_rows > 0;
        if apply {
            for (row, cells) in staged {
                for (column, value) in cells {
                    result.stage(row, column, value);
                }
            }
            for cells in added {
                result.add_row(cells);
            }
        }
        let state = state_of(session, &result);
        Ok(Pasted {
            delimiter: table.delimiter.to_string(),
            header,
            rows: body.len(),
            columns: table.width(),
            cells: staged_cells,
            updated_rows,
            new_rows,
            skipped,
            problems,
            problems_total,
            truncated,
            applied: apply,
            preview,
            pending: state.pending,
            dirty: state.dirty,
        })
    }
}

#[cfg(test)]
mod tests;
