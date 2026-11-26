use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use super::quote;
use crate::driver::{BoxFuture, Canceller, DbError, ErrorKind, RowSink};
use crate::value::{Column, Value};

pub const MISSING: &str = "sqliteMissing";

pub const OLDEST: (u32, u32, u32) = (3, 37, 0);

#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

pub fn find_program(set: Option<&str>) -> Result<PathBuf, DbError> {
    let missing = |detail: String| DbError::new(ErrorKind::Invalid, detail).with_code(MISSING);
    if let Some(set) = set.map(str::trim).filter(|set| !set.is_empty()) {
        let path = PathBuf::from(set);
        return if path.is_file() { Ok(path) } else { Err(missing(format!("{set} is not a program"))) };
    }
    let name = if cfg!(windows) { "sqlite3.exe" } else { "sqlite3" };
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|folder| folder.join(name));
    let usual = ["/usr/bin/sqlite3", "/opt/homebrew/bin/sqlite3", "/usr/local/bin/sqlite3"].map(PathBuf::from);
    on_path
        .chain(usual.into_iter().filter(|_| cfg!(unix)))
        .find(|path| path.is_file())
        .ok_or_else(|| missing("the sqlite3 program was not found".to_owned()))
}

pub fn target(path: &str, read_only: bool) -> String {
    let path = path.trim();
    if path.is_empty() || path == ":memory:" {
        return ":memory:".to_owned();
    }
    let mut uri = String::from("file:");
    let text = path.replace('\\', "/");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for c in text.chars() {
        match c {
            '%' | '?' | '#' | ' ' => uri.push_str(&format!("%{:02X}", c as u32)),
            _ => uri.push(c),
        }
    }
    uri.push_str(if read_only { "?mode=ro" } else { "?mode=rw" });
    uri
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Printed {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
    pub errors: Vec<String>,
    pub changes: (i64, i64),
}

pub struct Interrupt {
    #[cfg_attr(not(unix), allow(dead_code))]
    pid: Option<u32>,
    interrupted: Arc<AtomicBool>,
}

impl Interrupt {
    pub fn send(&self) -> bool {
        self.interrupted.store(true, Ordering::SeqCst);
        #[cfg(unix)]
        {
            let pid = self.pid.and_then(|pid| i32::try_from(pid).ok()).and_then(rustix::process::Pid::from_raw);
            pid.is_some_and(|pid| rustix::process::kill_process(pid, rustix::process::Signal::INT).is_ok())
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}

impl Canceller for Interrupt {
    fn cancel(&self) -> BoxFuture<'static, Result<(), DbError>> {
        let delivered = self.send();
        Box::pin(async move { if delivered { Ok(()) } else { Err(DbError::new(ErrorKind::Unsupported, "interrupt")) } })
    }
}

pub struct Process {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    err: BufReader<ChildStderr>,
    nonce: String,
    seq: u64,
    ended: bool,
    running: bool,
    interrupted: Arc<AtomicBool>,
    pub version: String,
}

fn nonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher
        .write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    format!("{:016x}", hasher.finish())
}

async fn next_line<R: AsyncRead + Unpin>(reader: &mut BufReader<R>, buffer: &mut Vec<u8>) -> Option<String> {
    buffer.clear();
    match reader.read_until(b'\n', buffer).await {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            while buffer.last().is_some_and(|byte| *byte == b'\n' || *byte == b'\r') {
                buffer.pop();
            }
            Some(String::from_utf8_lossy(buffer).into_owned())
        }
    }
}

async fn next_record<R: AsyncRead + Unpin>(reader: &mut BufReader<R>, buffer: &mut Vec<u8>) -> Option<String> {
    let mut record: Vec<u8> = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', buffer).await {
            Ok(0) | Err(_) if record.is_empty() => return None,
            Ok(0) | Err(_) => break,
            Ok(_) => record.extend_from_slice(buffer),
        }
        if record.iter().filter(|byte| **byte == b'\'').count() % 2 == 0 {
            break;
        }
    }
    if record.last() == Some(&b'\n') {
        record.pop();
        if record.last() == Some(&b'\r') {
            record.pop();
        }
    }
    Some(String::from_utf8_lossy(&record).into_owned())
}

struct Rows {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    truncated: bool,
    changes: (i64, i64),
    complete: bool,
}

async fn read_rows<R: AsyncRead + Unpin>(out: &mut BufReader<R>, marker: &str, max_rows: usize) -> Rows {
    let mut buffer = Vec::new();
    let mut read = Rows { columns: Vec::new(), rows: Vec::new(), truncated: false, changes: (0, 0), complete: false };
    let mut header = true;
    let mut trailer = false;
    while let Some(line) = next_record(out, &mut buffer).await {
        if line == marker {
            if trailer {
                read.complete = true;
                break;
            }
            trailer = true;
            header = true;
            continue;
        }
        let values = quote::row(&line).unwrap_or_else(|| vec![Value::Text(line.clone())]);
        if trailer {
            if !std::mem::replace(&mut header, false)
                && let [Value::Integer(changes), Value::Integer(total)] = values.as_slice()
            {
                read.changes = (*changes, *total);
            }
            continue;
        }
        if std::mem::replace(&mut header, false) {
            read.columns =
                values.into_iter().map(|value| value.display().unwrap_or_else(|| "NULL".to_owned())).collect();
            continue;
        }
        if read.rows.len() < max_rows {
            read.rows.push(values);
        } else {
            read.truncated = true;
        }
    }
    read
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Streamed {
    pub dropped: bool,
    pub changes: (i64, i64),
    pub named: bool,
    complete: bool,
}

pub type NameColumns<'a> = dyn Fn(&[String], &[Vec<Value>]) -> Vec<Column> + Sync + 'a;

pub struct StreamInto<'a> {
    pub sink: &'a mut dyn RowSink,
    pub skip: u64,
    pub page: usize,
    pub columns: &'a NameColumns<'a>,
}

struct Feed<'a, 'b> {
    into: &'b mut StreamInto<'a>,
    names: Vec<String>,
    first: Vec<Vec<Value>>,
    sent: bool,
    wanted: usize,
    keeping: bool,
    dropped: bool,
    batch: Vec<Vec<Value>>,
}

impl Feed<'_, '_> {
    async fn send_columns(&mut self) {
        self.sent = true;
        let columns = (self.into.columns)(&self.names, &self.first);
        self.into.sink.columns(columns);
        if !self.first.is_empty() {
            self.wanted = self.into.sink.want().await;
        }
        for row in std::mem::take(&mut self.first) {
            self.take(row).await;
        }
    }

    async fn take(&mut self, row: Vec<Value>) {
        if !self.keeping || self.wanted == 0 {
            self.dropped = true;
            return;
        }
        self.batch.push(row);
        self.wanted -= 1;
        if self.batch.len() >= self.into.page.max(1) || self.wanted == 0 {
            self.keeping = self.into.sink.push(std::mem::take(&mut self.batch));
            if self.keeping && self.wanted == 0 {
                self.wanted = self.into.sink.want().await;
            }
        }
    }

    async fn row(&mut self, row: Vec<Value>) {
        if self.into.skip > 0 {
            self.into.skip -= 1;
            return;
        }
        if self.sent {
            self.take(row).await;
            return;
        }
        self.first.push(row);
        if self.first.len() >= self.into.page.max(1) {
            self.send_columns().await;
        }
    }

    async fn end(&mut self) {
        if !self.sent && !self.names.is_empty() {
            self.send_columns().await;
        }
        if !self.batch.is_empty() && self.keeping {
            self.into.sink.push(std::mem::take(&mut self.batch));
        }
    }
}

async fn stream_rows<R: AsyncRead + Unpin>(
    out: &mut BufReader<R>,
    marker: &str,
    into: &mut StreamInto<'_>,
) -> Streamed {
    let mut buffer = Vec::new();
    let mut read = Streamed::default();
    let mut feed = Feed {
        into,
        names: Vec::new(),
        first: Vec::new(),
        sent: false,
        wanted: 0,
        keeping: true,
        dropped: false,
        batch: Vec::new(),
    };
    let mut header = true;
    let mut trailer = false;
    while let Some(line) = next_record(out, &mut buffer).await {
        if line == marker {
            if trailer {
                read.complete = true;
                break;
            }
            feed.end().await;
            trailer = true;
            header = true;
            continue;
        }
        let values = quote::row(&line).unwrap_or_else(|| vec![Value::Text(line.clone())]);
        if trailer {
            if !std::mem::replace(&mut header, false)
                && let [Value::Integer(changes), Value::Integer(total)] = values.as_slice()
            {
                read.changes = (*changes, *total);
            }
            continue;
        }
        if std::mem::replace(&mut header, false) {
            feed.names = values.into_iter().map(|value| value.display().unwrap_or_else(|| "NULL".to_owned())).collect();
            continue;
        }
        feed.row(values).await;
    }
    read.dropped = feed.dropped;
    read.named = feed.sent;
    read
}

fn unsuffixed(names: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::with_capacity(names.len());
    for name in names {
        let base = name
            .rsplit_once(':')
            .filter(|(base, count)| {
                !count.is_empty()
                    && count.bytes().all(|byte| byte.is_ascii_digit())
                    && seen.iter().any(|known| known == base)
            })
            .map(|(base, _)| base.to_owned());
        seen.push(base.unwrap_or(name));
    }
    seen
}

async fn read_errors<R: AsyncRead + Unpin>(err: &mut BufReader<R>, marker: &str) -> (Vec<String>, bool) {
    let quoted = format!("\"{marker}\"");
    let mut buffer = Vec::new();
    let mut lines = Vec::new();
    while let Some(line) = next_line(err, &mut buffer).await {
        if line.contains(&quoted) || line.ends_with(marker) {
            return (lines, true);
        }
        lines.push(line);
    }
    (lines, false)
}

pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split_whitespace().next()?.split('.').map(|part| part.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
}

impl Process {
    pub async fn start(program: &Path, target: &str, read_only: bool, busy_ms: u64) -> Result<Self, DbError> {
        let reported =
            Command::new(program).arg("-version").stdin(Stdio::null()).kill_on_drop(true).output().await.map_err(
                |error| DbError::new(ErrorKind::Invalid, format!("{}: {error}", program.display())).with_code(MISSING),
            )?;
        let reported = String::from_utf8_lossy(&reported.stdout).trim().to_owned();
        if parse_version(&reported).is_some_and(|version| version < OLDEST) {
            return Err(DbError::new(
                ErrorKind::Unsupported,
                format!("sqlite3 {reported} is older than {}.{}.{}", OLDEST.0, OLDEST.1, OLDEST.2),
            )
            .with_code(MISSING));
        }
        let mut command = Command::new(program);
        command.arg("-safe").arg("-batch").arg("-init").arg(NULL_DEVICE);
        if read_only {
            command.arg("-readonly");
        }
        command.arg(target).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| {
            DbError::new(ErrorKind::Invalid, format!("{}: {error}", program.display())).with_code(MISSING)
        })?;
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(DbError::new(ErrorKind::Connection, "sqlite3 has no pipes"));
        };
        let mut process = Process {
            child,
            stdin,
            out: BufReader::new(stdout),
            err: BufReader::new(stderr),
            nonce: nonce(),
            seq: 0,
            ended: false,
            running: false,
            interrupted: Arc::default(),
            version: String::new(),
        };
        let setup = format!(".mode quote\n.headers on\n.explain off\n.bail off\n.timeout {busy_ms}\n");
        process
            .stdin
            .write_all(setup.as_bytes())
            .await
            .map_err(|error| DbError::new(ErrorKind::Connection, error.to_string()))?;
        let probe = process.run("SELECT sqlite_version(), (SELECT count(*) FROM sqlite_master)", 1).await?;
        if !probe.errors.is_empty() {
            let mut failed = error(&probe.errors, "");
            failed.kind = ErrorKind::Connection;
            return Err(failed);
        }
        process.version = probe.rows.first().and_then(|row| row.first()).and_then(Value::display).unwrap_or_default();
        Ok(process)
    }

    pub fn interrupt(&self) -> Interrupt {
        Interrupt { pid: self.child.id(), interrupted: Arc::clone(&self.interrupted) }
    }

    pub fn ended(&self) -> bool {
        self.ended || self.running || self.interrupted.load(Ordering::SeqCst)
    }

    fn retire(&mut self) {
        self.ended = true;
        self.running = false;
        let _ = self.child.start_kill();
    }

    fn begin(&mut self) -> Result<String, DbError> {
        if self.ended() {
            self.retire();
            return Err(DbError::new(ErrorKind::Connection, "sqlite3 has ended"));
        }
        self.running = true;
        self.seq += 1;
        Ok(format!("zen_{}_{}", self.nonce, self.seq))
    }

    fn finish(&mut self, complete: bool, errors: &[String]) -> Result<(), DbError> {
        let interrupted = self.interrupted.swap(false, Ordering::SeqCst);
        if complete && !interrupted {
            self.running = false;
            return Ok(());
        }
        self.retire();
        if complete {
            return Ok(());
        }
        if interrupted {
            return Err(DbError::new(ErrorKind::Cancelled, ""));
        }
        let mut detail = errors.join("\n");
        if detail.is_empty() {
            detail = "sqlite3 ended".to_owned();
        }
        Err(DbError::new(ErrorKind::Connection, detail))
    }

    pub async fn run(&mut self, sql: &str, max_rows: usize) -> Result<Printed, DbError> {
        let marker = self.begin()?;
        let input =
            format!("{sql}\n;\n.print {marker}\nSELECT changes(), total_changes();\n.print {marker}\n.{marker}\n");
        let Process { stdin, out, err, .. } = self;
        let write = async {
            stdin.write_all(input.as_bytes()).await?;
            stdin.flush().await
        };
        let (_, rows, (errors, finished)) =
            tokio::join!(write, read_rows(out, &marker, max_rows), read_errors(err, &marker));
        self.finish(rows.complete && finished, &errors)?;
        Ok(Printed { columns: rows.columns, rows: rows.rows, truncated: rows.truncated, errors, changes: rows.changes })
    }

    pub async fn run_streamed(
        &mut self,
        sql: &str,
        into: &mut StreamInto<'_>,
    ) -> Result<(Streamed, Vec<String>), DbError> {
        let marker = self.begin()?;
        let input =
            format!("{sql}\n;\n.print {marker}\nSELECT changes(), total_changes();\n.print {marker}\n.{marker}\n");
        let Process { stdin, out, err, .. } = self;
        let write = async {
            stdin.write_all(input.as_bytes()).await?;
            stdin.flush().await
        };
        let (_, streamed, (errors, finished)) =
            tokio::join!(write, stream_rows(out, &marker, into), read_errors(err, &marker));
        self.finish(streamed.complete && finished, &errors)?;
        Ok((streamed, errors))
    }

    pub async fn columns_of(&mut self, sql: &str, query_only: bool) -> Result<Vec<String>, DbError> {
        let view = format!("zen_columns_{}_{}", self.nonce, self.seq + 1);
        let (open, close) =
            if query_only { ("PRAGMA query_only = 0;\n", ";\nPRAGMA query_only = 1") } else { ("", "") };
        let probe = format!(
            "{open}CREATE TEMP VIEW \"{view}\" AS\n{sql}\n;\nSELECT name FROM pragma_table_info('{view}', 'temp');\nDROP VIEW IF EXISTS temp.\"{view}\"{close}"
        );
        let printed = self.run(&probe, usize::MAX).await?;
        let names = printed.rows.into_iter().filter_map(|row| row.into_iter().next()?.display()).collect();
        Ok(unsuffixed(names))
    }

    pub async fn close(mut self) {
        let _ = self.stdin.shutdown().await;
        drop(self.stdin);
        if tokio::time::timeout(std::time::Duration::from_secs(2), self.child.wait()).await.is_err() {
            let _ = self.child.start_kill();
        }
    }
}

pub fn error(lines: &[String], statement: &str) -> DbError {
    let first = lines.first().map(String::as_str).unwrap_or_default();
    let message = match first.find("near line ") {
        Some(at) => first[at..].split_once(": ").map_or(first, |(_, message)| message),
        None => first.strip_prefix("Error: ").unwrap_or(first),
    };
    let mut failed = DbError::new(ErrorKind::Server, message.trim());
    let caret = lines.iter().position(|line| line.trim_start().starts_with('^') || line.trim_end().ends_with('^'));
    if let Some(caret) = caret.filter(|caret| *caret >= 1)
        && let Some(context) = lines[caret - 1].strip_prefix("  ")
        && let Some(column) = lines[caret].find('^').and_then(|column| column.checked_sub(2))
    {
        let flat: String = statement
            .chars()
            .map(|c| if matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r') { ' ' } else { c })
            .collect();
        let found = (column.min(context.len())..=context.len())
            .rev()
            .filter(|end| *end > 0 && context.is_char_boundary(*end))
            .find_map(|end| flat.find(&context[..end]));
        if let Some(before) = found.and_then(|found| statement.get(..found + column)) {
            failed = failed.at(before.chars().count() + 1);
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_whose_text_breaks_lines_is_read_as_one() {
        let printed = "'s','n'\n'a''b\nc\n',1\n'x',2\nm\n'changes()','total_changes()'\n0,0\nm\n";
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let read = runtime.block_on(read_rows(&mut BufReader::new(printed.as_bytes()), "m", 10));
        assert!(read.complete);
        assert_eq!(read.columns, ["s", "n"]);
        assert_eq!(
            read.rows,
            [vec![Value::Text("a'b\nc\n".into()), Value::Integer(1)], vec![Value::Text("x".into()), Value::Integer(2)]]
        );
    }

    #[test]
    fn an_interrupt_always_retires_the_process_it_reached() {
        let Ok(program) = find_program(None) else { return };
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let mut idle = Process::start(&program, ":memory:", false, 1000).await.unwrap();
            assert_eq!(idle.run("SELECT 1", 10).await.unwrap().rows, [[Value::Integer(1)]]);
            assert!(idle.interrupt().send());
            assert!(idle.ended(), "sqlite3 ends at its next input after an interrupt reached it idle");
            assert_eq!(idle.run("SELECT 2", 10).await.unwrap_err().detail, "sqlite3 has ended");
            let sql =
                "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 20000) SELECT max(x) FROM c";
            for delay in 0..40u64 {
                let mut process = Process::start(&program, ":memory:", false, 1000).await.unwrap();
                let interrupt = process.interrupt();
                let signal = async {
                    tokio::time::sleep(std::time::Duration::from_micros(delay * 250)).await;
                    interrupt.send();
                };
                let (ran, ()) = tokio::join!(process.run(sql, 10), signal);
                match ran {
                    Ok(printed) => assert_eq!(printed.rows, [[Value::Integer(20000)]]),
                    Err(error) => assert_eq!(error.kind, ErrorKind::Cancelled, "{error:?}"),
                }
                assert!(
                    process.ended(),
                    "after {delay} steps the interrupt left a process that ends at the next statement"
                );
            }
        });
    }

    #[test]
    fn duplicate_names_lose_the_suffix_a_view_gives_them() {
        let names = ["a", "b", "a:1", "a:2", "c:1", "x:y"].map(str::to_owned).to_vec();
        assert_eq!(unsuffixed(names), ["a", "b", "a", "a", "c:1", "x:y"]);
    }

    #[test]
    fn paths_become_uris_that_never_create() {
        assert_eq!(target("", false), ":memory:");
        assert_eq!(target(":memory:", true), ":memory:");
        assert_eq!(target("/tmp/a b?#%.db", false), "file:/tmp/a%20b%3F%23%25.db?mode=rw");
        assert_eq!(target("/tmp/x.db", true), "file:/tmp/x.db?mode=ro");
    }

    #[test]
    fn errors_keep_their_message_and_position() {
        let lines = [
            "Parse error near line 4: no such column: frm".to_owned(),
            "  select 1,   2,   frm x;".to_owned(),
            "                   ^--- error here".to_owned(),
        ];
        let failed = error(&lines, "select 1,\n  2,\n  frm x");
        assert_eq!(failed.detail, "no such column: frm");
        assert_eq!(failed.position, Some(18));
        let framed = [
            "Parse error near line 9: no such column: frm".to_owned(),
            "  select 1,   2,   frm x ;".to_owned(),
            "                   ^--- error here".to_owned(),
        ];
        assert_eq!(
            error(&framed, "select 1,\n  2,\n  frm x").position,
            Some(18),
            "the sent `;` is not in the statement"
        );
        let late = [
            "Parse error near line 1: no such column: zzz".to_owned(),
            format!("  {} zzz", "x".repeat(40)),
            format!("  {}error here ---^", " ".repeat(27)),
        ];
        let statement = format!("SELECT 1 FROM t WHERE {} zzz", "x".repeat(40));
        assert_eq!(error(&late, &statement).position, Some(23 + 41));
        let runtime = error(&["Error near line 3: UNIQUE constraint failed: u.a".to_owned()], "insert");
        assert_eq!((runtime.detail.as_str(), runtime.position), ("UNIQUE constraint failed: u.a", None));
        assert_eq!(parse_version("3.54.0 2026-04-09 x"), Some((3, 54, 0)));
    }
}
