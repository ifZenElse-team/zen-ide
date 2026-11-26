use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::driver::{ContextKind, DbError, ErrorKind};
use crate::journal::{self, Appender};

pub const JOURNAL: &str = "history.jsonl";

pub const SETTINGS: &str = "history-settings.json";

pub const MOST_TEXT: usize = 64 * 1024;

pub const MOST_BYTES: usize = 64 * 1024 * 1024;

const MOST_ERROR: usize = 2000;

const DAY_MS: i64 = 86_400_000;

/// What made a statement run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// Execute Statement in a console (⌘⏎), or a selection.
    #[default]
    Statement,
    /// Execute Script or Execute from Caret in a console.
    Script,
    /// A filter or sort that read a result again.
    Filter,
    /// The rows of an object tab's Data page.
    Data,
    /// Saving staged data edits.
    Save,
    /// Commit or Rollback.
    Transaction,
    /// SQL run by `db.query.run`.
    Query,
    /// The query or table read again to export its rows.
    Export,
    /// A CSV file imported into a table.
    Import,
    /// Cancelling a session's statement or ending a session from the Sessions tab.
    Session,
    /// Structure changes made in an object editor.
    Structure,
    /// A console's script run on several connections in turn.
    Connections,
    /// Find in Database: a schema's catalogue and its tables read for a text.
    Search,
    /// Tables copied from one connection into another.
    Transfer,
}

impl Source {
    pub const ALL: [Source; 14] = [
        Source::Statement,
        Source::Script,
        Source::Filter,
        Source::Data,
        Source::Save,
        Source::Transaction,
        Source::Query,
        Source::Export,
        Source::Import,
        Source::Session,
        Source::Structure,
        Source::Connections,
        Source::Search,
        Source::Transfer,
    ];
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Provenance {
    pub source: Source,
    pub agent: bool,
    pub place: Option<String>,
    pub confirmed: bool,
}

impl Provenance {
    pub fn of(source: Source) -> Self {
        Self { source, agent: false, place: None, confirmed: false }
    }
}

/// How a statement ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// It ran.
    #[default]
    Done,
    /// It failed.
    Failed,
    /// It was cancelled or ran past its deadline.
    Cancelled,
    /// It was never sent: refused on a read-only connection or by the rules for agents, or skipped by a person.
    Refused,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ran {
    pub connection: String,
    pub context: ContextKind,
    pub schema: Option<String>,
    pub text: String,
    pub started: SystemTime,
    pub duration: Duration,
    pub rows: Option<u64>,
    pub affected: Option<u64>,
    pub error: Option<DbError>,
    pub provenance: Provenance,
    pub params: Vec<crate::params::NamedValue>,
}

pub trait HistorySink: Send + Sync {
    // WHY: it is called on the engine's runtime, so waiting here holds up every connection.
    fn record(&self, ran: Ran);
}

/// One statement of the history, as kept on disk and answered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Its id, unique in the journal and growing with time.
    pub id: u64,
    /// When it started, in milliseconds since 1970 (UTC).
    pub at: i64,
    /// The connection's id.
    pub connection: String,
    /// The context it ran in.
    pub context: ContextKind,
    /// The schema the context was in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The statement, passwords masked.
    pub text: String,
    /// Whether the statement was longer than [`MOST_TEXT`] characters and was cut.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// Whether a password in it was masked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub redacted: bool,
    /// Milliseconds it ran.
    pub duration_ms: u64,
    /// The rows it returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    /// The rows it changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// How it ended.
    pub outcome: Outcome,
    /// Why it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
    /// What made it run.
    pub source: Source,
    /// Whether an agent asked for it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent: bool,
    /// Where it was run from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// Whether it ran after a confirmation its connection type or its risk asked for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirmed: bool,
    /// Its bind parameters: `:name` alone, or `:name = value` when the settings keep values; values of secret-looking names are masked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<String>,
}

/// The history's settings, kept beside its journal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Whether statements are recorded.
    pub enabled: bool,
    /// Days a statement is kept.
    pub retention_days: u32,
    /// The most statements kept.
    pub max_entries: usize,
    /// Whether values of bind parameters are kept with their names (secret-looking ones masked); off by default.
    pub parameter_values: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: true, retention_days: 90, max_entries: 10_000, parameter_values: false }
    }
}

impl Settings {
    #[must_use]
    pub fn bounded(mut self) -> Self {
        self.retention_days = self.retention_days.clamp(1, 3650);
        self.max_entries = self.max_entries.clamp(100, 100_000);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub text: Option<String>,
    pub connections: Vec<String>,
    pub sources: Vec<Source>,
    pub agent: Option<bool>,
    pub outcome: Option<Outcome>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub entries: Vec<Entry>,
    pub total: usize,
    pub truncated: bool,
    pub kept: usize,
    pub revision: u64,
    pub loaded: bool,
}

struct Kept {
    entry: Entry,
    folded: String,
}

impl Kept {
    fn new(entry: Entry) -> Self {
        let mut folded = entry.text.to_lowercase();
        if let Some(error) = &entry.error {
            folded.push('\n');
            folded.push_str(&error.detail.to_lowercase());
        }
        Self { entry, folded }
    }

    fn bytes(&self) -> usize {
        self.entry.text.len()
    }
}

#[derive(Default)]
struct Inner {
    kept: VecDeque<Kept>,
    bytes: usize,
    settings: Settings,
    settings_chosen: bool,
    loaded: bool,
    revision: u64,
    last_id: u64,
    error: Option<String>,
}

enum Job {
    Append(Vec<u8>),
    Clear { connection: Option<String>, before: Option<i64> },
    Settings(Settings),
    Flush(Sender<()>),
}

pub struct Journal {
    folder: PathBuf,
    inner: Arc<Mutex<Inner>>,
    jobs: Mutex<Sender<Job>>,
}

fn now_ms() -> i64 {
    millis_since_epoch(SystemTime::now())
}

fn millis_since_epoch(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_millis()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

impl Journal {
    pub fn shared(folder: &Path) -> Arc<Journal> {
        static OPEN: OnceLock<Mutex<HashMap<PathBuf, Arc<Journal>>>> = OnceLock::new();
        let mut open = OPEN.get_or_init(Mutex::default).lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(open.entry(folder.to_owned()).or_insert_with(|| Arc::new(Journal::open(folder))))
    }

    pub fn open(folder: &Path) -> Journal {
        let (journal, writer, receiver) = Self::unstarted(folder);
        let spawned = std::thread::Builder::new().name("zen-db-history".to_owned()).spawn(move || writer.run(receiver));
        if let Err(error) = spawned {
            lock(&journal.inner).error = Some(error.to_string());
        }
        journal
    }

    fn unstarted(folder: &Path) -> (Journal, Writer, Receiver<Job>) {
        let inner = Arc::new(Mutex::new(Inner::default()));
        let (sender, receiver) = channel();
        let writer = Writer {
            folder: folder.to_owned(),
            inner: Arc::clone(&inner),
            appender: Appender::new(folder.join(JOURNAL)),
            lines: 0,
        };
        (Journal { folder: folder.to_owned(), inner, jobs: Mutex::new(sender) }, writer, receiver)
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    fn send(&self, job: Job) {
        let _ = self.jobs.lock().unwrap_or_else(PoisonError::into_inner).send(job);
    }

    pub fn flush(&self, timeout: Duration) -> bool {
        let (done, wait) = channel();
        self.send(Job::Flush(done));
        wait.recv_timeout(timeout).is_ok()
    }

    pub fn settings(&self) -> Settings {
        lock(&self.inner).settings.clone()
    }

    pub fn set_settings(&self, settings: Settings) {
        let settings = settings.bounded();
        {
            let mut inner = lock(&self.inner);
            inner.settings = settings.clone();
            inner.settings_chosen = true;
        }
        self.send(Job::Settings(settings));
    }

    pub fn clear(&self, connection: Option<String>, before: Option<i64>) {
        self.send(Job::Clear { connection, before });
    }

    pub fn get(&self, id: u64) -> Option<Entry> {
        let inner = lock(&self.inner);
        inner.kept.iter().rev().find(|kept| kept.entry.id == id).map(|kept| kept.entry.clone())
    }

    pub fn size(&self) -> (usize, usize) {
        let inner = lock(&self.inner);
        (inner.kept.len(), inner.bytes)
    }

    pub fn error(&self) -> Option<String> {
        lock(&self.inner).error.clone()
    }

    pub fn search(&self, query: &Query) -> Found {
        let words: Vec<String> =
            query.text.as_deref().unwrap_or_default().split_whitespace().map(str::to_lowercase).collect();
        let inner = lock(&self.inner);
        let mut entries = Vec::new();
        let mut total = 0;
        for kept in inner.kept.iter().rev() {
            let entry = &kept.entry;
            let matches = query.since.is_none_or(|since| entry.at >= since)
                && query.until.is_none_or(|until| entry.at < until)
                && (query.connections.is_empty() || query.connections.contains(&entry.connection))
                && (query.sources.is_empty() || query.sources.contains(&entry.source))
                && query.agent.is_none_or(|agent| entry.agent == agent)
                && query.outcome.is_none_or(|outcome| entry.outcome == outcome)
                && words.iter().all(|word| kept.folded.contains(word.as_str()));
            if !matches {
                continue;
            }
            if total >= query.offset && entries.len() < query.limit {
                entries.push(entry.clone());
            }
            total += 1;
        }
        let truncated = total > query.offset + entries.len();
        Found { entries, total, truncated, kept: inner.kept.len(), revision: inner.revision, loaded: inner.loaded }
    }

    pub fn revision(&self) -> u64 {
        lock(&self.inner).revision
    }
}

impl HistorySink for Journal {
    fn record(&self, ran: Ran) {
        let line = {
            let mut inner = lock(&self.inner);
            if !inner.settings.enabled {
                return;
            }
            let id = inner.last_id.saturating_add(1).max(u64::try_from(now_ms()).unwrap_or(0).saturating_mul(1000));
            inner.last_id = id;
            let entry = entry_of(id, ran, inner.settings.parameter_values);
            let line = journal::encode(&entry);
            let kept = Kept::new(entry);
            inner.bytes += kept.bytes();
            inner.kept.push_back(kept);
            inner.revision += 1;
            prune(&mut inner, now_ms());
            line
        };
        self.send(Job::Append(line));
    }
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Writer {
    folder: PathBuf,
    inner: Arc<Mutex<Inner>>,
    appender: Appender,
    lines: usize,
}

impl Writer {
    fn run(mut self, jobs: Receiver<Job>) {
        self.load();
        loop {
            let job = match self.appender.sync_due(Instant::now()) {
                None => match jobs.recv() {
                    Ok(job) => job,
                    Err(_) => break,
                },
                Some(left) => match jobs.recv_timeout(left) {
                    Ok(job) => job,
                    Err(RecvTimeoutError::Timeout) => {
                        self.sync();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                },
            };
            match job {
                Job::Append(line) => self.append(&line),
                Job::Clear { connection, before } => {
                    let changed = {
                        let mut inner = lock(&self.inner);
                        let count = inner.kept.len();
                        inner.kept.retain(|kept| {
                            let other =
                                connection.as_ref().is_some_and(|connection| kept.entry.connection != *connection);
                            let newer = before.is_some_and(|before| kept.entry.at >= before);
                            other || newer
                        });
                        inner.bytes = inner.kept.iter().map(Kept::bytes).sum();
                        inner.revision += 1;
                        count != inner.kept.len()
                    };
                    if changed {
                        self.rewrite();
                    }
                }
                Job::Settings(settings) => {
                    self.write_settings(&settings);
                    if prune(&mut lock(&self.inner), now_ms()) {
                        self.rewrite();
                    }
                }
                Job::Flush(done) => {
                    self.sync();
                    let _ = done.send(());
                }
            }
            if let Err(error) = self.appender.sync_if_due(Instant::now()) {
                self.fail(&error);
            }
        }
        self.sync();
    }

    fn sync(&mut self) {
        if let Err(error) = self.appender.sync() {
            self.fail(&error);
        }
    }

    fn journal(&self) -> PathBuf {
        self.folder.join(JOURNAL)
    }

    fn fail(&self, error: &std::io::Error) {
        lock(&self.inner).error = Some(error.to_string());
    }

    fn load(&mut self) {
        let settings = std::fs::read(self.folder.join(SETTINGS))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_default()
            .bounded();
        let mut read = VecDeque::new();
        let mut damaged = false;
        match std::fs::read(self.journal()) {
            Ok(bytes) => {
                let decoded = journal::decode::<Entry>(&bytes);
                damaged = decoded.damaged > 0 || decoded.unchecked > 0;
                read.extend(decoded.records.into_iter().map(Kept::new));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => self.fail(&error),
        }
        self.lines = read.len();
        let pruned = {
            let mut inner = lock(&self.inner);
            let settings = if inner.settings_chosen { inner.settings.clone() } else { settings };
            if settings.enabled {
                read.extend(std::mem::take(&mut inner.kept));
            }
            read.make_contiguous().sort_by_key(|kept| kept.entry.id);
            let mut seen = std::collections::HashSet::new();
            read.retain(|kept| seen.insert(kept.entry.id));
            inner.kept = read;
            inner.bytes = inner.kept.iter().map(Kept::bytes).sum();
            inner.last_id = inner.last_id.max(inner.kept.iter().map(|kept| kept.entry.id).max().unwrap_or(0));
            inner.settings = settings;
            inner.loaded = true;
            inner.revision += 1;
            prune(&mut inner, now_ms())
        };
        if pruned || damaged {
            self.rewrite();
        }
    }

    fn append(&mut self, line: &[u8]) {
        if !lock(&self.inner).settings.enabled {
            return;
        }
        if let Err(error) = self.appender.append(line) {
            return self.fail(&error);
        }
        self.lines += 1;
        let kept = lock(&self.inner).kept.len();
        if self.lines > kept + (kept / 10).max(100) {
            self.rewrite();
        }
    }

    fn rewrite(&mut self) {
        self.sync();
        self.appender.close();
        let text: Vec<u8> = {
            let inner = lock(&self.inner);
            let mut text = Vec::with_capacity(inner.bytes + inner.kept.len() * 200);
            for kept in &inner.kept {
                text.extend_from_slice(&journal::encode(&kept.entry));
            }
            self.lines = inner.kept.len();
            text
        };
        let written = if text.is_empty() {
            journal::remove(&self.journal())
        } else {
            journal::write_atomic(&self.journal(), &text)
        };
        if let Err(error) = written {
            self.fail(&error);
        }
    }

    fn write_settings(&self, settings: &Settings) {
        let text = serde_json::to_vec_pretty(settings).unwrap_or_default();
        if let Err(error) = journal::write_atomic(&self.folder.join(SETTINGS), &text) {
            self.fail(&error);
        }
    }
}

fn prune(inner: &mut Inner, now: i64) -> bool {
    let oldest = now.saturating_sub(i64::from(inner.settings.retention_days).saturating_mul(DAY_MS));
    let mut removed = false;
    while let Some(front) = inner.kept.front() {
        let old = front.entry.at < oldest;
        if !old && inner.kept.len() <= inner.settings.max_entries && inner.bytes <= MOST_BYTES {
            break;
        }
        let bytes = front.bytes();
        inner.kept.pop_front();
        inner.bytes -= bytes;
        removed = true;
    }
    if removed {
        inner.revision += 1;
    }
    removed
}

fn cut(text: &str, most: usize) -> (String, bool) {
    match text.char_indices().nth(most) {
        Some((end, _)) => (text[..end].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

fn parameters(params: &[crate::params::NamedValue], values: bool) -> Vec<String> {
    let shown = crate::params::shown(params, crate::dialect::SqlDialect::Postgres);
    if values {
        return shown.into_iter().map(|line| redact(&line).0).collect();
    }
    shown.into_iter().map(|line| line.split(" = ").next().unwrap_or_default().to_owned()).collect()
}

fn entry_of(id: u64, ran: Ran, values: bool) -> Entry {
    let (masked, redacted) = redact(&ran.text);
    let (text, cut_text) = cut(&masked, MOST_TEXT);
    let outcome = match &ran.error {
        None => Outcome::Done,
        Some(error) if matches!(error.kind, ErrorKind::Cancelled | ErrorKind::Timeout) => Outcome::Cancelled,
        Some(error) if error.kind == ErrorKind::Refused => Outcome::Refused,
        Some(_) => Outcome::Failed,
    };
    let error = ran.error.map(|mut error| {
        error.detail = cut(&redact(&error.detail).0, MOST_ERROR).0;
        error
    });
    Entry {
        id,
        at: millis_since_epoch(ran.started),
        connection: ran.connection,
        context: ran.context,
        schema: ran.schema,
        text,
        cut: cut_text,
        redacted,
        duration_ms: u64::try_from(ran.duration.as_millis()).unwrap_or(u64::MAX),
        rows: ran.rows,
        affected: ran.affected,
        outcome,
        error,
        source: ran.provenance.source,
        agent: ran.provenance.agent,
        confirmed: ran.provenance.confirmed,
        place: ran.provenance.place,
        parameters: parameters(&ran.params, values),
    }
}

pub const MASK: &str = "***";

pub fn redact(text: &str) -> (String, bool) {
    let uris = crate::registry::redact(text);
    let mut out = String::with_capacity(uris.len());
    let mut changed = uris != text;
    let words = redis_words(&uris);
    if let Some(spans) = words {
        let mut last = 0;
        for (start, end) in spans {
            out.push_str(&uris[last..start]);
            out.push_str(MASK);
            last = end;
            changed = true;
        }
        out.push_str(&uris[last..]);
        return (out, changed);
    }
    let bytes = uris.as_bytes();
    let mut at = 0;
    let mut copied = 0;
    while at < bytes.len() {
        let boundary = at == 0 || !is_word(bytes[at - 1]);
        let keyword = if boundary { keyword_at(bytes, at) } else { None };
        let Some((after, kind)) = keyword else {
            at += 1;
            continue;
        };
        if let Some((start, end)) = value_after(bytes, after, kind) {
            out.push_str(&uris[copied..start]);
            out.push_str(MASK);
            copied = end;
            at = end;
            changed = true;
        } else {
            at = after;
        }
    }
    out.push_str(&uris[copied..]);
    (out, changed)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keyword {
    Password,
    IdentifiedBy,
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn word_at(bytes: &[u8], at: usize, word: &str) -> bool {
    let end = at + word.len();
    end <= bytes.len()
        && bytes[at..end].eq_ignore_ascii_case(word.as_bytes())
        && bytes.get(end).is_none_or(|next| !is_word(*next))
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

fn keyword_at(bytes: &[u8], at: usize) -> Option<(usize, Keyword)> {
    if word_at(bytes, at, "password") {
        return Some((at + "password".len(), Keyword::Password));
    }
    if word_at(bytes, at, "identified") {
        let mut next = skip_space(bytes, at + "identified".len());
        if word_at(bytes, next, "with") {
            next = skip_space(bytes, next + 4);
            while next < bytes.len() && (is_word(bytes[next]) || matches!(bytes[next], b'\'' | b'"' | b'`')) {
                next += 1;
            }
            next = skip_space(bytes, next);
        }
        if word_at(bytes, next, "by") {
            return Some((next + 2, Keyword::IdentifiedBy));
        }
    }
    None
}

fn value_after(bytes: &[u8], after: usize, kind: Keyword) -> Option<(usize, usize)> {
    let mut at = skip_space(bytes, after);
    let mut assigned = false;
    if kind == Keyword::Password && word_at(bytes, at, "for") {
        let equals = at + bytes[at..].iter().take_while(|byte| **byte != b';').position(|byte| *byte == b'=')?;
        at = equals;
    }
    if bytes.get(at) == Some(&b'=') {
        assigned = true;
        at = skip_space(bytes, at + 1);
    }
    if word_at(bytes, at, "password") {
        at = skip_space(bytes, at + "password".len());
    }
    if bytes.get(at) == Some(&b'(') {
        at = skip_space(bytes, at + 1);
    }
    if matches!(bytes.get(at), Some(b'E' | b'e')) && bytes.get(at + 1) == Some(&b'\'') {
        at += 1;
    }
    match bytes.get(at)? {
        b'\'' | b'"' => {
            let quote = bytes[at];
            let start = at + 1;
            let mut end = start;
            while end < bytes.len() {
                if bytes[end] == quote {
                    if bytes.get(end + 1) == Some(&quote) {
                        end += 2;
                        continue;
                    }
                    return Some((start, end));
                }
                if bytes[end] == b'\\' && quote == b'\'' {
                    end += 1;
                }
                end += 1;
            }
            Some((start, bytes.len()))
        }
        b'$' => {
            let tag_end = at + 1 + bytes[at + 1..].iter().position(|byte| *byte == b'$')?;
            let tag = &bytes[at..=tag_end];
            let start = tag_end + 1;
            let end =
                bytes[start..].windows(tag.len()).position(|window| window == tag).map_or(bytes.len(), |at| start + at);
            Some((start, end))
        }
        _ if assigned || kind == Keyword::IdentifiedBy => {
            let start = at;
            let mut end = at;
            while end < bytes.len()
                && !bytes[end].is_ascii_whitespace()
                && !matches!(bytes[end], b';' | b',' | b')' | b'\'' | b'"')
            {
                end += 1;
            }
            (end > start && !word_at(bytes, start, "null")).then_some((start, end))
        }
        _ => None,
    }
}

fn redis_words(text: &str) -> Option<Vec<(usize, usize)>> {
    let mut words = Vec::new();
    let mut at = 0;
    let bytes = text.as_bytes();
    while at < bytes.len() {
        at = skip_space(bytes, at);
        if at >= bytes.len() {
            break;
        }
        let start = at;
        if matches!(bytes[at], b'"' | b'\'') {
            let quote = bytes[at];
            at += 1;
            while at < bytes.len() && bytes[at] != quote {
                if bytes[at] == b'\\' {
                    at += 1;
                }
                at += 1;
            }
            at = (at + 1).min(bytes.len());
        } else {
            while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                at += 1;
            }
        }
        words.push((start, at));
    }
    let word = |index: usize| words.get(index).map(|(start, end)| &text[*start..*end]);
    let is = |index: usize, name: &str| word(index).is_some_and(|word| word.eq_ignore_ascii_case(name));
    let mut masked = Vec::new();
    if is(0, "auth") {
        masked.extend(words.iter().skip(1).copied());
    } else if is(0, "hello") || is(0, "migrate") {
        let mut index = 1;
        while index < words.len() {
            if is(index, "auth") {
                masked.extend(words.iter().skip(index + 1).take(if is(0, "hello") { 2 } else { 1 }).copied());
            } else if is(index, "auth2") {
                masked.extend(words.iter().skip(index + 1).take(2).copied());
            }
            index += 1;
        }
    } else if is(0, "config") && is(1, "set") {
        let mut index = 2;
        while index + 1 < words.len() {
            if is(index, "requirepass") || is(index, "masterauth") {
                masked.push(words[index + 1]);
            }
            index += 2;
        }
    } else if is(0, "acl") && is(1, "setuser") {
        masked.extend(
            words
                .iter()
                .skip(3)
                .filter(|(start, _)| matches!(bytes[*start], b'>' | b'<' | b'#' | b'!'))
                .map(|(start, end)| (start + 1, *end)),
        );
    } else {
        return None;
    }
    masked.retain(|(start, end)| end > start);
    masked.sort_unstable();
    Some(masked)
}

pub fn local_midnight(at: i64) -> i64 {
    let Ok(timestamp) = jiff::Timestamp::from_millisecond(at) else { return at };
    let zoned = timestamp.to_zoned(jiff::tz::TimeZone::system());
    zoned.start_of_day().map_or(at, |start| start.timestamp().as_millisecond())
}

pub fn local_time(at: i64) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_millisecond(at) else { return String::new() };
    timestamp.to_zoned(jiff::tz::TimeZone::system()).strftime("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn now() -> i64 {
    now_ms()
}

#[cfg(test)]
mod tests;
