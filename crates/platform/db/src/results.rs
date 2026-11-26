use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::driver::{BoxFuture, ContextKind, DbError, ErrorKind, RowSink};
use crate::edit::{Changes, Deleted, Inserted, Staged, Updated};
use crate::value::{Column, Typed, Value};

pub const NOT_REPEATABLE: &str = "notRepeatable";
pub const DEFAULT_BUDGET: usize = 256 * 1024 * 1024;

/// How results are read page by page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Paging {
    /// The rows of the first page, and of each Fetch Next.
    pub page: usize,
    /// The rows read before a portal outside the person's own transaction is closed and its transaction committed.
    pub row_cap: usize,
    /// Keep the portal open between pages outside the person's transaction too (never on Production connections).
    pub keep_open: bool,
}

impl Default for Paging {
    fn default() -> Self {
        Self { page: 200, row_cap: 10_000, keep_open: false }
    }
}

/// Why a result stopped reading before its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// The row cap was reached; Fetch Next or Fetch All reads on.
    Cap,
    /// The window's memory budget for results is used up.
    Budget,
    /// The result was closed or replaced, or its portal closed for another run.
    Closed,
    /// It was cancelled.
    Cancelled,
    /// The statement failed while its rows were read.
    Failed,
}

/// Where a result's reading is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReadState {
    /// Rows are being read.
    Reading,
    /// The portal is open and waits until more rows are wanted.
    Open,
    /// Every row was read.
    Done,
    /// Reading stopped before the end; `stoppedBecause` says why.
    Stopped,
}

/// A result as views and agents read it, without its rows.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultInfo {
    /// Its id.
    pub id: u64,
    /// The operation of the query that made it.
    pub query: u64,
    /// The statement's index in that query.
    pub statement: usize,
    /// The connection.
    pub connection: String,
    /// The execution context it was read in.
    pub context: ContextKind,
    /// The columns.
    pub columns: Vec<Column>,
    /// The rows kept.
    pub rows: u64,
    /// Where reading is.
    pub state: ReadState,
    /// Why it stopped before the end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_because: Option<StopReason>,
    /// Whether rows remain on the server: shown as `200+`.
    pub more: bool,
    /// The rows the query has in all, once counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Milliseconds until the first rows came.
    pub execution_ms: u64,
    /// Milliseconds spent reading rows after that.
    pub fetch_ms: u64,
    /// The memory its rows take, in bytes.
    pub bytes: usize,
    /// The table its rows come from, when the statement reads one table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
}

#[derive(Debug)]
pub struct Budget {
    limit: AtomicUsize,
    used: AtomicUsize,
}

impl Budget {
    pub fn new(limit: usize) -> Self {
        Self { limit: AtomicUsize::new(limit), used: AtomicUsize::new(0) }
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::SeqCst)
    }

    pub fn limit(&self) -> usize {
        self.limit.load(Ordering::SeqCst)
    }

    pub fn set_limit(&self, limit: usize) {
        self.limit.store(limit, Ordering::SeqCst);
    }

    fn exceeded(&self) -> bool {
        self.used() >= self.limit()
    }

    fn take(&self, bytes: usize) {
        self.used.fetch_add(bytes, Ordering::SeqCst);
    }

    fn give_back(&self, bytes: usize) {
        let _ = self.used.try_update(Ordering::SeqCst, Ordering::SeqCst, |used| Some(used.saturating_sub(bytes)));
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::new(DEFAULT_BUDGET)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TextKind {
    Text,
    Decimal,
    Interval,
    Uuid,
    Json,
    Temporal(String),
    Other(String),
}

impl TextKind {
    fn of(value: &Value) -> Option<(TextKind, &str)> {
        Some(match value {
            Value::Text(text) => (TextKind::Text, text),
            Value::Decimal(text) => (TextKind::Decimal, text),
            Value::Interval(text) => (TextKind::Interval, text),
            Value::Uuid(text) => (TextKind::Uuid, text),
            Value::Json(text) => (TextKind::Json, text),
            Value::Temporal(typed) => (TextKind::Temporal(typed.type_name.clone()), &typed.text),
            Value::Other(typed) => (TextKind::Other(typed.type_name.clone()), &typed.text),
            _ => return None,
        })
    }

    fn value(&self, text: &str) -> Value {
        let text = text.to_owned();
        match self {
            TextKind::Text => Value::Text(text),
            TextKind::Decimal => Value::Decimal(text),
            TextKind::Interval => Value::Interval(text),
            TextKind::Uuid => Value::Uuid(text),
            TextKind::Json => Value::Json(text),
            TextKind::Temporal(type_name) => Value::Temporal(Typed { text, type_name: type_name.clone() }),
            TextKind::Other(type_name) => Value::Other(Typed { text, type_name: type_name.clone() }),
        }
    }
}

#[derive(Debug)]
enum Data {
    Empty,
    Bool(Vec<bool>),
    Int(Vec<i64>),
    Float(Vec<f64>),
    Text { kind: TextKind, ends: Vec<usize>, bytes: String },
    Values(Vec<Value>),
}

#[derive(Debug)]
struct Store {
    data: Data,
    nulls: Vec<u64>,
    len: usize,
}

impl Store {
    fn new() -> Self {
        Self { data: Data::Empty, nulls: Vec::new(), len: 0 }
    }

    fn is_null(&self, row: usize) -> bool {
        self.nulls.get(row / 64).is_some_and(|word| word >> (row % 64) & 1 == 1)
    }

    fn mark_null(&mut self, row: usize) {
        let word = row / 64;
        if self.nulls.len() <= word {
            self.nulls.resize(word + 1, 0);
        }
        self.nulls[word] |= 1 << (row % 64);
    }

    fn get(&self, row: usize) -> Value {
        if row >= self.len || self.is_null(row) {
            return Value::Null;
        }
        match &self.data {
            Data::Empty => Value::Null,
            Data::Bool(values) => Value::Bool(values[row]),
            Data::Int(values) => Value::Integer(values[row]),
            Data::Float(values) => Value::Float(values[row]),
            Data::Text { kind, ends, bytes } => kind.value(text_at(ends, bytes, row)),
            Data::Values(values) => values[row].clone(),
        }
    }

    fn text(&self, row: usize) -> Option<&str> {
        match &self.data {
            Data::Text { ends, bytes, .. } if row < self.len && !self.is_null(row) => Some(text_at(ends, bytes, row)),
            _ => None,
        }
    }

    fn fits(&self, value: &Value) -> bool {
        match (&self.data, value) {
            (Data::Empty, _) | (Data::Values(_), _) => true,
            (Data::Bool(_), Value::Bool(_)) | (Data::Int(_), Value::Integer(_)) | (Data::Float(_), Value::Float(_)) => {
                true
            }
            (Data::Text { kind, .. }, value) => TextKind::of(value).is_some_and(|(found, _)| found == *kind),
            _ => false,
        }
    }

    fn widen(&mut self) {
        let values: Vec<Value> = (0..self.len).map(|row| self.get(row)).collect();
        self.data = Data::Values(values);
    }

    fn start(&mut self, value: &Value) {
        let len = self.len;
        self.data = match value {
            Value::Bool(_) => Data::Bool(vec![false; len]),
            Value::Integer(_) => Data::Int(vec![0; len]),
            Value::Float(_) => Data::Float(vec![0.0; len]),
            value => match TextKind::of(value) {
                Some((kind, _)) => Data::Text { kind, ends: vec![0; len], bytes: String::new() },
                None => Data::Values(vec![Value::Null; len]),
            },
        };
    }

    fn push(&mut self, value: Value) -> usize {
        let row = self.len;
        if value == Value::Null {
            self.mark_null(row);
            self.len += 1;
            let size = match &mut self.data {
                Data::Empty => 0,
                Data::Bool(values) => {
                    values.push(false);
                    1
                }
                Data::Int(values) => {
                    values.push(0);
                    8
                }
                Data::Float(values) => {
                    values.push(0.0);
                    8
                }
                Data::Text { ends, bytes, .. } => {
                    ends.push(bytes.len());
                    8
                }
                Data::Values(values) => {
                    values.push(Value::Null);
                    size_of::<Value>()
                }
            };
            return size;
        }
        if matches!(self.data, Data::Empty) {
            self.start(&value);
        } else if !self.fits(&value) {
            self.widen();
        }
        self.len += 1;
        match (&mut self.data, value) {
            (Data::Bool(values), Value::Bool(value)) => {
                values.push(value);
                1
            }
            (Data::Int(values), Value::Integer(value)) => {
                values.push(value);
                8
            }
            (Data::Float(values), Value::Float(value)) => {
                values.push(value);
                8
            }
            (Data::Values(values), value) => {
                let size = value_size(&value);
                values.push(value);
                size
            }
            (Data::Text { ends, bytes, .. }, value) => {
                let text = TextKind::of(&value).map(|(_, text)| text).unwrap_or_default();
                bytes.push_str(text);
                ends.push(bytes.len());
                text.len() + 8
            }
            (_, value) => {
                self.widen();
                self.len -= 1;
                self.push(value)
            }
        }
    }
}

fn text_at<'a>(ends: &[usize], bytes: &'a str, row: usize) -> &'a str {
    let start = if row == 0 { 0 } else { ends[row - 1] };
    &bytes[start..ends[row]]
}

pub fn value_size(value: &Value) -> usize {
    size_of::<Value>()
        + match value {
            Value::Decimal(text)
            | Value::Text(text)
            | Value::Interval(text)
            | Value::Uuid(text)
            | Value::Json(text) => text.len(),
            Value::Bytes(bytes) => bytes.len(),
            Value::Temporal(typed) | Value::Other(typed) => typed.text.len() + typed.type_name.len(),
            Value::Array(items) => items.iter().map(value_size).sum(),
            Value::Map(fields) | Value::Document(fields) => {
                fields.iter().map(|(key, value)| key.len() + 24 + value_size(value)).sum()
            }
            Value::Null | Value::Bool(_) | Value::Integer(_) | Value::Float(_) => 0,
        }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellText {
    pub text: String,
    pub null: bool,
    pub cut: bool,
    pub number: bool,
    pub boolean: Option<bool>,
    pub changed: bool,
    pub default: bool,
}

fn cut(text: &str, chars: usize) -> (String, bool) {
    let line_end = text.find(['\n', '\r']);
    let mut shown: String = text.chars().take(chars).collect();
    let mut cut = shown.len() < text.len();
    if let Some(end) = line_end
        && end < shown.len()
    {
        shown.truncate(end);
        cut = true;
    }
    (shown, cut)
}

impl CellText {
    fn staged(staged: &Staged, chars: usize) -> Self {
        match staged {
            Staged::Value(value) => Self { changed: true, ..Self::of(value, chars) },
            Staged::Default => Self {
                text: String::new(),
                null: false,
                cut: false,
                number: false,
                boolean: None,
                changed: true,
                default: true,
            },
        }
    }

    pub fn of(value: &Value, chars: usize) -> Self {
        let number = matches!(value, Value::Integer(_) | Value::Float(_) | Value::Decimal(_));
        let boolean = match value {
            Value::Bool(value) => Some(*value),
            _ => None,
        };
        match value.display() {
            None => {
                Self { text: String::new(), null: true, cut: false, number, boolean, changed: false, default: false }
            }
            Some(text) => {
                let (text, cut) = cut(&text, chars);
                Self { text, null: false, cut, number, boolean, changed: false, default: false }
            }
        }
    }
}

struct Inner {
    columns: Vec<Column>,
    stores: Vec<Store>,
    rows: usize,
    bytes: usize,
    staged: HashMap<(usize, usize), Staged>,
    deleted: BTreeSet<usize>,
    inserted: Vec<BTreeMap<usize, Staged>>,
    positions: Vec<usize>,
    revision: u64,
    state: ReadState,
    stopped: Option<StopReason>,
    more: bool,
    total: Option<u64>,
    started: Instant,
    first_rows: Option<Duration>,
    finished: Option<Duration>,
    wanted: usize,
    cap: usize,
    keep_open: bool,
    portal: bool,
    close: bool,
    rerun: bool,
}

impl Inner {
    fn value(&self, row: usize, column: usize) -> Value {
        if row >= self.rows {
            return match self.inserted.get(row - self.rows).and_then(|cells| cells.get(&column)) {
                Some(Staged::Value(value)) => value.clone(),
                _ => Value::Null,
            };
        }
        match self.staged.get(&(row, column)) {
            Some(Staged::Value(value)) => value.clone(),
            Some(Staged::Default) => Value::Null,
            None => self.stores.get(column).map_or(Value::Null, |store| store.get(row)),
        }
    }

    fn refill(&mut self, rows: Vec<Vec<Value>>) {
        let width = self.stores.len();
        let mut stores: Vec<Store> = (0..width).map(|_| Store::new()).collect();
        let mut bytes = 0;
        let count = rows.len();
        for row in rows {
            let mut values = row.into_iter();
            for store in &mut stores {
                bytes += store.push(values.next().unwrap_or(Value::Null));
            }
        }
        self.stores = stores;
        self.rows = count;
        self.bytes = bytes;
    }

    fn reorder(&mut self, order: &[usize]) {
        let width = self.stores.len();
        let rows: Vec<Vec<Value>> =
            order.iter().map(|row| (0..width).map(|column| self.stores[column].get(*row)).collect()).collect();
        let mut at = vec![0; order.len()];
        for (new, old) in order.iter().enumerate() {
            at[*old] = new;
        }
        self.positions = order.iter().map(|old| self.positions.get(*old).copied().unwrap_or(*old)).collect();
        self.staged = std::mem::take(&mut self.staged)
            .into_iter()
            .map(|((row, column), staged)| ((at.get(row).copied().unwrap_or(row), column), staged))
            .collect();
        self.deleted =
            std::mem::take(&mut self.deleted).into_iter().map(|row| at.get(row).copied().unwrap_or(row)).collect();
        let positions = std::mem::take(&mut self.positions);
        self.refill(rows);
        self.positions = positions;
    }
}

pub fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    sort_key(a).cmp(&sort_key(b))
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Number(Number, u8, i64),
    Text(String),
    Null,
}

#[derive(PartialEq)]
struct Number(f64);

impl Eq for Number {}

impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Number {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

fn sort_key(value: &Value) -> SortKey {
    match value {
        Value::Null => SortKey::Null,
        Value::Integer(value) => SortKey::Number(Number(*value as f64), 2, *value),
        Value::Bool(value) => SortKey::Number(Number(f64::from(u8::from(*value))), 1, i64::from(*value)),
        Value::Float(value) => SortKey::Number(Number(*value), 0, 0),
        Value::Decimal(text) => match text.parse::<f64>() {
            Ok(number) => SortKey::Number(Number(number), 0, 0),
            Err(_) => SortKey::Text(text.clone()),
        },
        _ => SortKey::Text(value.display().unwrap_or_default()),
    }
}

/// How a row of a result is changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RowMark {
    /// Not changed.
    Clean,
    /// Some of its cells are changed.
    Changed,
    /// A new row.
    Inserted,
    /// To be deleted.
    Deleted,
}

/// How much of a result is staged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// Cells changed, in rows read and in new rows.
    pub cells: usize,
    /// Rows read with changed cells.
    pub updated: usize,
    /// New rows.
    pub inserted: usize,
    /// Rows to delete.
    pub deleted: usize,
}

impl Pending {
    pub fn any(&self) -> bool {
        self.updated + self.inserted + self.deleted > 0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedRows {
    pub updated: Vec<(usize, Vec<(usize, Value)>)>,
    pub inserted: Vec<Vec<(usize, Value)>>,
}

pub struct ResultSet {
    id: u64,
    query: u64,
    statement: usize,
    connection: String,
    context: ContextKind,
    text: String,
    params: Vec<Value>,
    table: Option<String>,
    paging: Paging,
    inner: Mutex<Inner>,
    budget: Arc<Budget>,
    notify: tokio::sync::Notify,
    waker: Option<crate::Waker>,
}

impl std::fmt::Debug for ResultSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultSet").field("id", &self.id).field("rows", &self.inner().rows).finish()
    }
}

#[derive(Clone, Debug)]
pub struct Origin {
    pub query: u64,
    pub statement: usize,
    pub connection: String,
    pub context: ContextKind,
    pub text: String,
    pub params: Vec<Value>,
}

impl ResultSet {
    pub fn new(id: u64, origin: Origin, paging: Paging, budget: Arc<Budget>, waker: Option<crate::Waker>) -> Self {
        let table = table_of(&origin.text);
        let page = paging.page.max(1);
        let cap = paging.row_cap.max(page);
        Self {
            id,
            query: origin.query,
            statement: origin.statement,
            connection: origin.connection,
            context: origin.context,
            text: origin.text,
            params: origin.params,
            table,
            paging,
            inner: Mutex::new(Inner {
                columns: Vec::new(),
                stores: Vec::new(),
                rows: 0,
                bytes: 0,
                staged: HashMap::new(),
                deleted: BTreeSet::new(),
                inserted: Vec::new(),
                positions: Vec::new(),
                revision: 0,
                state: ReadState::Reading,
                stopped: None,
                more: false,
                total: None,
                started: Instant::now(),
                first_rows: None,
                finished: None,
                wanted: page,
                cap,
                keep_open: false,
                portal: false,
                close: false,
                rerun: false,
            }),
            budget,
            notify: tokio::sync::Notify::new(),
            waker,
        }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wake(&self) {
        if let Some(waker) = &self.waker {
            waker();
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn statement_text(&self) -> &str {
        &self.text
    }

    pub fn statement_params(&self) -> &[Value] {
        &self.params
    }

    pub fn connection(&self) -> &str {
        &self.connection
    }

    pub fn context(&self) -> &ContextKind {
        &self.context
    }

    pub fn table(&self) -> Option<&str> {
        self.table.as_deref()
    }

    pub fn columns(&self) -> Vec<Column> {
        self.inner().columns.clone()
    }

    pub fn len(&self) -> usize {
        let inner = self.inner();
        inner.rows + inner.inserted.len()
    }

    pub fn stored(&self) -> usize {
        self.inner().rows
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn revision(&self) -> u64 {
        self.inner().revision
    }
    pub fn info(&self) -> ResultInfo {
        let inner = self.inner();
        let elapsed = inner.finished.unwrap_or_else(|| inner.started.elapsed());
        let execution = inner.first_rows.unwrap_or(elapsed);
        ResultInfo {
            id: self.id,
            query: self.query,
            statement: self.statement,
            connection: self.connection.clone(),
            context: self.context.clone(),
            columns: inner.columns.clone(),
            rows: inner.rows as u64,
            state: inner.state,
            stopped_because: inner.stopped,
            more: inner.more,
            total: inner.total,
            execution_ms: millis(execution),
            fetch_ms: millis(elapsed.saturating_sub(execution)),
            bytes: inner.bytes,
            table: self.table.clone(),
        }
    }

    pub fn get(&self, row: usize, column: usize) -> Value {
        let inner = self.inner();
        inner.value(row, column)
    }

    pub fn original(&self, row: usize, column: usize) -> Value {
        let inner = self.inner();
        inner.stores.get(column).map_or(Value::Null, |store| store.get(row))
    }

    pub fn page(&self, offset: usize, limit: usize) -> Vec<Vec<Value>> {
        let inner = self.inner();
        let shown = inner.rows + inner.inserted.len();
        let end = offset.saturating_add(limit).min(shown);
        (offset.min(end)..end)
            .map(|row| (0..inner.stores.len()).map(|column| inner.value(row, column)).collect())
            .collect()
    }

    pub fn calc(
        &self,
        rows: std::ops::Range<usize>,
        columns: std::ops::Range<usize>,
        cap: usize,
    ) -> crate::present::Calc {
        let width = columns.len();
        let wanted = rows.len().min(cap.div_ceil(width.max(1)).saturating_add(1));
        let page = self.page(rows.start, wanted);
        let cells = page.into_iter().flat_map(|row| row.into_iter().skip(columns.start).take(width));
        crate::present::calc(cells, cap)
    }

    pub fn cells(
        &self,
        rows: std::ops::Range<usize>,
        columns: std::ops::Range<usize>,
        chars: usize,
    ) -> Vec<Vec<CellText>> {
        let inner = self.inner();
        let end = rows.end.min(inner.rows + inner.inserted.len());
        (rows.start.min(end)..end)
            .map(|row| {
                columns
                    .clone()
                    .filter(|column| *column < inner.stores.len())
                    .map(|column| {
                        if row >= inner.rows {
                            return match inner.inserted[row - inner.rows].get(&column) {
                                Some(staged) => CellText::staged(staged, chars),
                                None => CellText { default: true, ..CellText::of(&Value::Null, chars) },
                            };
                        }
                        if let Some(staged) = inner.staged.get(&(row, column)) {
                            return CellText::staged(staged, chars);
                        }
                        let store = &inner.stores[column];
                        match store.text(row) {
                            Some(text) => {
                                let (text, cut) = cut(text, chars);
                                let number = matches!(store.data, Data::Text { kind: TextKind::Decimal, .. });
                                CellText {
                                    text,
                                    null: false,
                                    cut,
                                    number,
                                    boolean: None,
                                    changed: false,
                                    default: false,
                                }
                            }
                            None => CellText::of(&store.get(row), chars),
                        }
                    })
                    .collect()
            })
            .collect()
    }

    pub fn marks(&self, rows: std::ops::Range<usize>) -> Vec<RowMark> {
        let inner = self.inner();
        let changed: BTreeSet<usize> = inner.staged.keys().map(|(row, _)| *row).collect();
        let end = rows.end.min(inner.rows + inner.inserted.len());
        (rows.start.min(end)..end)
            .map(|row| {
                if row >= inner.rows {
                    RowMark::Inserted
                } else if inner.deleted.contains(&row) {
                    RowMark::Deleted
                } else if changed.contains(&row) {
                    RowMark::Changed
                } else {
                    RowMark::Clean
                }
            })
            .collect()
    }

    pub fn stage(&self, row: usize, column: usize, staged: Staged) -> bool {
        let mut inner = self.inner();
        if column >= inner.stores.len() || inner.deleted.contains(&row) {
            return false;
        }
        if row >= inner.rows {
            let at = row - inner.rows;
            let Some(cells) = inner.inserted.get_mut(at) else { return false };
            cells.insert(column, staged);
        } else if staged == Staged::Value(inner.stores[column].get(row)) {
            inner.staged.remove(&(row, column));
        } else {
            inner.staged.insert((row, column), staged);
        }
        inner.revision += 1;
        drop(inner);
        self.wake();
        true
    }

    pub fn unstage(&self, row: usize, column: usize) {
        let mut inner = self.inner();
        if row >= inner.rows {
            let at = row - inner.rows;
            if let Some(cells) = inner.inserted.get_mut(at) {
                cells.remove(&column);
            }
        } else {
            inner.staged.remove(&(row, column));
        }
        inner.revision += 1;
    }

    pub fn add_row(&self, cells: Vec<(usize, Staged)>) -> usize {
        let mut inner = self.inner();
        let width = inner.stores.len();
        inner.inserted.push(cells.into_iter().filter(|(column, _)| *column < width).collect());
        inner.revision += 1;
        inner.rows + inner.inserted.len() - 1
    }

    pub fn delete_rows(&self, rows: &[usize]) -> usize {
        let mut inner = self.inner();
        let stored = inner.rows;
        let mut count = 0;
        let mut new: Vec<usize> = rows.iter().filter(|row| **row >= stored).map(|row| row - stored).collect();
        new.sort_unstable();
        new.dedup();
        for at in new.into_iter().rev() {
            if at < inner.inserted.len() {
                inner.inserted.remove(at);
                count += 1;
            }
        }
        for row in rows.iter().filter(|row| **row < stored) {
            if inner.deleted.insert(*row) {
                count += 1;
            }
        }
        inner.revision += 1;
        count
    }

    pub fn restore_rows(&self, rows: &[usize]) {
        let mut inner = self.inner();
        for row in rows {
            inner.deleted.remove(row);
        }
        inner.revision += 1;
    }

    pub fn drop_edits(&self) {
        let mut inner = self.inner();
        inner.staged.clear();
        inner.deleted.clear();
        inner.inserted.clear();
        inner.revision += 1;
    }

    pub fn pending(&self) -> Pending {
        let inner = self.inner();
        let mut changed: BTreeSet<usize> = inner.staged.keys().map(|(row, _)| *row).collect();
        for row in &inner.deleted {
            changed.remove(row);
        }
        Pending {
            cells: inner.staged.len() + inner.inserted.iter().map(BTreeMap::len).sum::<usize>(),
            updated: changed.len(),
            inserted: inner.inserted.len(),
            deleted: inner.deleted.len(),
        }
    }

    pub fn changes(&self) -> Changes {
        let inner = self.inner();
        let width = inner.stores.len();
        let original = |row: usize| -> Vec<Value> { (0..width).map(|column| inner.stores[column].get(row)).collect() };
        let deleted: Vec<Deleted> =
            inner.deleted.iter().map(|row| Deleted { row: *row, original: original(*row) }).collect();
        let mut by_row: BTreeMap<usize, Vec<(usize, Staged)>> = BTreeMap::new();
        for ((row, column), staged) in &inner.staged {
            if !inner.deleted.contains(row) {
                by_row.entry(*row).or_default().push((*column, staged.clone()));
            }
        }
        let updated = by_row
            .into_iter()
            .map(|(row, mut cells)| {
                cells.sort_by_key(|(column, _)| *column);
                Updated { row, original: original(row), cells }
            })
            .collect();
        let inserted = inner
            .inserted
            .iter()
            .enumerate()
            .map(|(at, cells)| Inserted {
                row: inner.rows + at,
                cells: cells.iter().map(|(column, staged)| (*column, staged.clone())).collect(),
            })
            .collect();
        Changes { deleted, inserted, updated }
    }

    pub fn apply_saved(&self, saved: &SavedRows) {
        let mut inner = self.inner();
        let width = inner.stores.len();
        let rows = inner.rows;
        let updated: HashMap<usize, &Vec<(usize, Value)>> =
            saved.updated.iter().map(|(row, values)| (*row, values)).collect();
        let mut kept: Vec<Vec<Value>> = Vec::with_capacity(rows + saved.inserted.len());
        for row in 0..rows {
            if inner.deleted.contains(&row) {
                continue;
            }
            let mut values: Vec<Value> = (0..width).map(|column| inner.value(row, column)).collect();
            if let Some(returned) = updated.get(&row) {
                for (column, value) in returned.iter() {
                    if let Some(slot) = values.get_mut(*column) {
                        *slot = value.clone();
                    }
                }
            }
            kept.push(values);
        }
        for (at, returned) in saved.inserted.iter().enumerate() {
            let mut values: Vec<Value> = (0..width).map(|column| inner.value(rows + at, column)).collect();
            for (column, value) in returned {
                if let Some(slot) = values.get_mut(*column) {
                    *slot = value.clone();
                }
            }
            kept.push(values);
        }
        let old = inner.bytes;
        inner.refill(kept);
        let new = inner.bytes;
        inner.staged.clear();
        inner.deleted.clear();
        inner.inserted.clear();
        inner.positions.clear();
        inner.revision += 1;
        drop(inner);
        self.budget.give_back(old);
        self.budget.take(new);
        self.wake();
    }

    pub fn sort_rows(&self, sorts: &[(usize, bool)]) -> bool {
        let mut inner = self.inner();
        if matches!(inner.state, ReadState::Reading | ReadState::Open) {
            return false;
        }
        let rows = inner.rows;
        let width = inner.stores.len();
        if inner.positions.len() != rows {
            inner.positions = (0..rows).collect();
        }
        let mut order: Vec<usize> = (0..rows).collect();
        if sorts.is_empty() {
            order.sort_by_key(|row| inner.positions[*row]);
        } else {
            let keys: Vec<Vec<Value>> = (0..rows)
                .map(|row| {
                    sorts
                        .iter()
                        .map(|(column, _)| if *column < width { inner.value(row, *column) } else { Value::Null })
                        .collect()
                })
                .collect();
            order.sort_by(|a, b| {
                for (at, (_, descending)) in sorts.iter().enumerate() {
                    let ordering = compare_values(&keys[*a][at], &keys[*b][at]);
                    let ordering = if *descending { ordering.reverse() } else { ordering };
                    if ordering != std::cmp::Ordering::Equal {
                        return ordering;
                    }
                }
                inner.positions[*a].cmp(&inner.positions[*b])
            });
        }
        inner.reorder(&order);
        inner.revision += 1;
        true
    }

    pub fn find(&self, text: &str, match_case: bool, limit: usize, cap: usize) -> (Vec<(usize, usize)>, bool) {
        let inner = self.inner();
        let wanted = if match_case { text.to_owned() } else { text.to_lowercase() };
        let shown = inner.rows + inner.inserted.len();
        let width = inner.stores.len();
        let mut found = Vec::new();
        let mut looked = 0;
        for row in 0..shown {
            for column in 0..width {
                looked += 1;
                if looked > cap {
                    return (found, true);
                }
                let Some(cell) = inner.value(row, column).display() else { continue };
                let hit = if match_case { cell.contains(&wanted) } else { cell.to_lowercase().contains(&wanted) };
                if hit {
                    found.push((row, column));
                    if found.len() >= limit {
                        return (found, row + 1 < shown || column + 1 < width);
                    }
                }
            }
        }
        (found, false)
    }

    pub fn set_total(&self, total: u64) {
        self.inner().total = Some(total);
        self.wake();
    }

    pub fn want_more(&self, rows: Option<usize>, repeatable: bool) -> Result<bool, DbError> {
        let mut inner = self.inner();
        let more = rows.unwrap_or(usize::MAX);
        let open = inner.portal && matches!(inner.state, ReadState::Open | ReadState::Reading);
        if open {
            inner.wanted = inner.rows.saturating_add(more).max(inner.wanted);
            inner.cap = inner.cap.max(inner.wanted);
            if inner.state == ReadState::Open {
                inner.state = ReadState::Reading;
            }
            drop(inner);
            self.notify.notify_waiters();
            return Ok(false);
        }
        if inner.state == ReadState::Reading {
            inner.cap = inner.rows.saturating_add(more).max(inner.cap);
            inner.wanted = inner.cap;
            return Ok(false);
        }
        if !inner.more {
            return Ok(false);
        }
        if !repeatable {
            return Err(DbError::new(ErrorKind::Unsupported, "fetch").with_code(NOT_REPEATABLE));
        }
        inner.cap = inner.rows.saturating_add(more);
        inner.wanted = inner.cap;
        inner.rerun = true;
        inner.state = ReadState::Reading;
        inner.stopped = None;
        Ok(true)
    }

    pub fn close(&self) {
        self.inner().close = true;
        self.notify.notify_waiters();
    }

    pub fn portal_open(&self) -> bool {
        let inner = self.inner();
        inner.portal && matches!(inner.state, ReadState::Open | ReadState::Reading)
    }

    pub fn reader(self: &Arc<Self>, last: bool, token: tokio_util::sync::CancellationToken) -> Reader {
        let skip = {
            let mut inner = self.inner();
            inner.close = false;
            if inner.rerun {
                inner.rerun = false;
                inner.rows
            } else {
                0
            }
        };
        Reader { result: Arc::clone(self), last, token, skip, columns_known: skip > 0 }
    }

    fn finish(&self, complete: bool, error: bool) {
        let mut inner = self.inner();
        let elapsed = inner.started.elapsed();
        inner.finished = Some(elapsed);
        inner.portal = false;
        if complete {
            inner.state = ReadState::Done;
            inner.more = false;
            inner.stopped = None;
        } else {
            inner.state = ReadState::Stopped;
            inner.more = true;
            if error {
                inner.stopped = Some(StopReason::Failed);
                inner.more = false;
            } else if inner.stopped.is_none() {
                let capped = !inner.keep_open && inner.rows >= inner.cap;
                inner.stopped = Some(if capped { StopReason::Cap } else { StopReason::Closed });
            }
        }
        drop(inner);
        self.wake();
    }

    fn release(&self) {
        let mut inner = self.inner();
        let bytes = std::mem::take(&mut inner.bytes);
        inner.stores.clear();
        inner.rows = 0;
        inner.staged.clear();
        inner.deleted.clear();
        inner.inserted.clear();
        inner.close = true;
        drop(inner);
        self.budget.give_back(bytes);
        self.notify.notify_waiters();
    }
}

impl Drop for ResultSet {
    fn drop(&mut self) {
        let bytes = self.inner().bytes;
        self.budget.give_back(bytes);
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub fn table_of(statement: &str) -> Option<String> {
    let plain = without_literals(statement);
    let lower = plain.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let original: Vec<&str> = plain.split_whitespace().collect();
    let from = words.iter().position(|word| *word == "from")?;
    let name = original.get(from + 1)?.trim_end_matches([';', ',', ')']);
    if name.is_empty() || name.starts_with('(') {
        return None;
    }
    let joined = words[from + 1..].iter().any(|word| *word == "join" || word.ends_with(','));
    Some(if joined { format!("{name}(+)") } else { name.to_owned() })
}

fn without_literals(statement: &str) -> String {
    let mut out = String::with_capacity(statement.len());
    let mut chars = statement.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                out.push(' ');
                while let Some(inner) = chars.next() {
                    if inner == '\'' && chars.peek() != Some(&'\'') {
                        break;
                    }
                    if inner == '\'' {
                        chars.next();
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        break;
                    }
                }
                out.push('\n');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for inner in chars.by_ref() {
                    if last == '*' && inner == '/' {
                        break;
                    }
                    last = inner;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

pub struct Reader {
    result: Arc<ResultSet>,
    last: bool,
    token: tokio_util::sync::CancellationToken,
    skip: usize,
    columns_known: bool,
}

impl Reader {
    pub fn result(&self) -> &Arc<ResultSet> {
        &self.result
    }

    pub fn skip(&self) -> u64 {
        self.skip as u64
    }

    pub fn finish(&self, complete: bool, error: bool) {
        self.result.finish(complete, error);
    }

    fn next_want(&self) -> Option<usize> {
        let result = &self.result;
        let mut inner = result.inner();
        if inner.close || self.token.is_cancelled() {
            if inner.stopped.is_none() {
                inner.stopped =
                    Some(if self.token.is_cancelled() { StopReason::Cancelled } else { StopReason::Closed });
            }
            return Some(0);
        }
        if result.budget.exceeded() {
            inner.stopped = Some(StopReason::Budget);
            return Some(0);
        }
        let page = result.paging.page.max(1);
        if inner.keep_open {
            if inner.rows < inner.wanted {
                inner.state = ReadState::Reading;
                return Some((inner.wanted - inner.rows).min(page.max(1000)));
            }
            inner.state = ReadState::Open;
            return None;
        }
        if inner.rows < inner.cap {
            let chunk = if inner.rows == 0 { page } else { page.max(1000) };
            return Some((inner.cap - inner.rows).min(chunk));
        }
        inner.stopped = Some(StopReason::Cap);
        Some(0)
    }
}

impl RowSink for Reader {
    fn columns(&mut self, columns: Vec<Column>) {
        let mut inner = self.result.inner();
        if self.columns_known && inner.columns.len() == columns.len() {
            return;
        }
        inner.stores = columns.iter().map(|_| Store::new()).collect();
        inner.columns = columns;
        drop(inner);
        self.result.wake();
    }

    fn portal(&mut self, in_transaction: bool) {
        let keep = self.last && (in_transaction || self.result.paging.keep_open);
        let mut inner = self.result.inner();
        inner.portal = true;
        inner.keep_open = keep;
    }

    fn want(&mut self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            loop {
                let notified = self.result.notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if let Some(wanted) = self.next_want() {
                    if wanted == 0 {
                        self.result.inner().portal = false;
                    }
                    return wanted;
                }
                self.result.wake();
                tokio::select! {
                    () = notified => {}
                    () = self.token.cancelled() => {}
                }
            }
        })
    }

    fn cap(&self) -> usize {
        let inner = self.result.inner();
        if inner.close || self.token.is_cancelled() || self.result.budget.exceeded() {
            return 0;
        }
        inner.cap.saturating_sub(inner.rows)
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        let mut added = 0;
        {
            let mut inner = self.result.inner();
            if inner.first_rows.is_none() {
                inner.first_rows = Some(inner.started.elapsed());
            }
            let width = inner.stores.len();
            for row in rows {
                for (column, value) in row.into_iter().enumerate().take(width) {
                    added += inner.stores[column].push(value);
                }
                let rows = inner.rows + 1;
                for store in inner.stores.iter_mut() {
                    while store.len < rows {
                        added += store.push(Value::Null);
                    }
                }
                inner.rows = rows;
            }
            inner.bytes += added;
        }
        self.result.budget.take(added);
        let stop = if self.token.is_cancelled() {
            Some(StopReason::Cancelled)
        } else if self.result.budget.exceeded() {
            Some(StopReason::Budget)
        } else {
            None
        };
        if let Some(stop) = stop {
            self.result.inner().stopped.get_or_insert(stop);
        }
        self.result.wake();
        stop.is_none()
    }
}

pub struct ResultStore {
    results: Mutex<Vec<Arc<ResultSet>>>,
    budget: Arc<Budget>,
    paging: Mutex<Paging>,
}

impl Default for ResultStore {
    fn default() -> Self {
        Self {
            results: Mutex::new(Vec::new()),
            budget: Arc::new(Budget::default()),
            paging: Mutex::new(Paging::default()),
        }
    }
}

impl ResultStore {
    fn results(&self) -> MutexGuard<'_, Vec<Arc<ResultSet>>> {
        self.results.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn budget(&self) -> &Arc<Budget> {
        &self.budget
    }

    pub fn paging(&self) -> Paging {
        self.paging.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub fn set_paging(&self, paging: Paging) {
        *self.paging.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = paging;
    }

    pub fn insert(&self, result: Arc<ResultSet>) {
        self.results().push(result);
    }

    pub fn get(&self, id: u64) -> Option<Arc<ResultSet>> {
        self.results().iter().find(|result| result.id == id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<ResultSet>> {
        self.results().clone()
    }

    pub fn remove(&self, id: u64) -> bool {
        let removed = {
            let mut results = self.results();
            let at = results.iter().position(|result| result.id == id);
            at.map(|at| results.remove(at))
        };
        match removed {
            Some(result) => {
                result.release();
                true
            }
            None => false,
        }
    }

    pub fn open_in(&self, connection: &str, context: &ContextKind) -> Vec<Arc<ResultSet>> {
        self.results()
            .iter()
            .filter(|result| result.connection == connection && result.context == *context && result.portal_open())
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests;
