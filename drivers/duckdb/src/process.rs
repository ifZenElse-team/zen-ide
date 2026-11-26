use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::Value as Json;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use zen_db_driver_kit::{Failure, value};

use crate::statement::Checked;
use crate::values;

#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

pub(crate) const MISSING: &str = "duckdbMissing";

fn missing(detail: String) -> Failure {
    let mut failure = Failure::database("invalid", detail);
    if let Some(data) = failure.data.as_mut() {
        data["code"] = serde_json::json!(MISSING);
    }
    failure
}

pub(crate) fn find_program(set: Option<&str>) -> Result<PathBuf, Failure> {
    find_in(set, std::env::var_os("PATH"), std::env::var_os("HOME"))
}

fn find_in(
    set: Option<&str>,
    path: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Result<PathBuf, Failure> {
    if let Some(set) = set.map(str::trim).filter(|set| !set.is_empty()) {
        let path = PathBuf::from(set);
        return if path.is_file() { Ok(path) } else { Err(missing(format!("{set} is not a program"))) };
    }
    let name = if cfg!(windows) { "duckdb.exe" } else { "duckdb" };
    let on_path = path
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .filter(|folder| folder.is_absolute())
        .map(|folder| folder.join(name));
    let home = home
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".duckdb/cli/latest").join(name));
    let usual = ["/opt/homebrew/bin/duckdb", "/usr/local/bin/duckdb", "/usr/bin/duckdb"].map(PathBuf::from);
    on_path
        .chain(home)
        .chain(usual.into_iter().filter(|_| cfg!(unix)))
        .find(|path| path.is_file())
        .ok_or_else(|| missing("the duckdb program was not found: install DuckDB's command-line program".to_owned()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) name: String,
    pub(crate) type_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Printed {
    pub(crate) columns: Vec<Column>,
    pub(crate) rows: Vec<Vec<Json>>,
    pub(crate) truncated: bool,
    pub(crate) errors: Vec<String>,
}

pub(crate) struct Process {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    err: BufReader<ChildStderr>,
    nonce: String,
    seq: u64,
    ended: bool,
    pub(crate) version: String,
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

struct Lines {
    lines: Vec<String>,
    complete: bool,
}

async fn until_marker<R: AsyncRead + Unpin>(
    out: &mut BufReader<R>,
    marker: &str,
    mut keep: impl FnMut(String),
) -> bool {
    let mut buffer = Vec::new();
    while let Some(line) = next_line(out, &mut buffer).await {
        if line == marker {
            return true;
        }
        if !line.is_empty() {
            keep(line);
        }
    }
    false
}

fn noise(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.starts_with("Did you mean: \".") || line.starts_with("Run '.help'")
}

async fn errors_until<R: AsyncRead + Unpin>(err: &mut BufReader<R>, marker: &str) -> Lines {
    let mut buffer = Vec::new();
    let mut lines = Vec::new();
    while let Some(line) = next_line(err, &mut buffer).await {
        if line.contains(marker) {
            return Lines { lines, complete: true };
        }
        if lines.is_empty() && noise(&line) {
            continue;
        }
        lines.push(line);
    }
    Lines { lines, complete: false }
}

fn described(lines: &[String]) -> Vec<Column> {
    lines
        .iter()
        .filter_map(|line| values::row(line))
        .filter_map(|fields| {
            let get = |key: &str| fields.iter().find(|(name, _)| name == key).and_then(|(_, value)| value.as_str());
            Some(Column { name: get("column_name")?.to_owned(), type_name: get("column_type").map(str::to_owned) })
        })
        .collect()
}

fn file_argument(file: &str) -> String {
    match file {
        "" => ":memory:".to_owned(),
        _ if file.starts_with('-') => format!("./{file}"),
        _ => file.to_owned(),
    }
}

const OLDEST: (u32, u32) = (1, 0);

pub(crate) fn parse_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.trim().strip_prefix('v')?.split_whitespace().next()?.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

async fn require_version(program: &Path) -> Result<(), Failure> {
    let mut command = Command::new(program);
    command.arg("-version").env("DUCKDB_AGENT_MODE", "0").stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
        .await
        .map_err(|_| missing(format!("{}: -version did not answer", program.display())))?
        .map_err(|error| missing(format!("{}: {error}", program.display())))?;
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if parse_version(&reported).is_some_and(|version| version >= OLDEST) {
        return Ok(());
    }
    let found = if reported.is_empty() { program.display().to_string() } else { reported };
    let mut failure = Failure::unsupported(format!(
        "duckdb {found} is not supported: DuckDB {}.{} or newer is needed",
        OLDEST.0, OLDEST.1
    ));
    if let Some(data) = failure.data.as_mut() {
        data["code"] = serde_json::json!(MISSING);
    }
    Err(failure)
}

impl Process {
    pub(crate) async fn start(program: &Path, file: &str, read_only: bool) -> Result<Self, Failure> {
        require_version(program).await?;
        let mut command = Command::new(program);
        command.env("DUCKDB_AGENT_MODE", "0").arg("-batch").arg("-init").arg(NULL_DEVICE);
        let file = file.trim();
        if read_only && !file.is_empty() && file != ":memory:" {
            command.arg("-readonly");
        }
        command.arg(file_argument(file));
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| missing(format!("{}: {error}", program.display())))?;
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(Failure::database("connection", "duckdb has no pipes"));
        };
        let mut process = Process {
            child,
            stdin,
            out: BufReader::new(stdout),
            err: BufReader::new(stderr),
            nonce: nonce(),
            seq: 0,
            ended: false,
            version: String::new(),
        };
        process
            .stdin
            .write_all(b".mode jsonlines\n.headers off\n.bail off\n.timer off\n")
            .await
            .map_err(|error| Failure::database("connection", error.to_string()))?;
        let quiet = crate::statement::check("SET autoinstall_known_extensions = false")?;
        let printed = process.run(&quiet, 0).await?;
        if let Some(first) = printed.errors.first() {
            return Err(Failure::database("connection", first.clone()));
        }
        let probe = crate::statement::check("SELECT version() AS version")?;
        let printed = process.run(&probe, 1).await?;
        if let Some(first) = printed.errors.first() {
            return Err(Failure::database("connection", first.clone()));
        }
        process.version = printed
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(|cell| cell["value"].as_str())
            .unwrap_or_default()
            .to_owned();
        Ok(process)
    }

    pub(crate) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }

    pub(crate) async fn run(&mut self, statement: &Checked, max_rows: usize) -> Result<Printed, Failure> {
        if self.ended {
            return Err(Failure::database("connection", "duckdb has ended"));
        }
        self.seq += 1;
        let marker = format!("zen_{}_{}", self.nonce, self.seq);
        let (describe, run) = (format!("{marker}_d"), format!("{marker}_s"));
        let text = &statement.text;
        let mut input = String::new();
        if statement.describable() {
            input.push_str(&format!("DESCRIBE {text}\n;\n.print {describe}\n.{describe}\n"));
        }
        input.push_str(&format!("{text}\n;\n.print {run}\n.{run}\n"));
        let describable = statement.describable();
        let Process { stdin, out, err, .. } = self;
        let write = async {
            stdin.write_all(input.as_bytes()).await?;
            stdin.flush().await
        };
        let read_out = async {
            let mut description = Vec::new();
            if describable && !until_marker(out, &describe, |line| description.push(line)).await {
                return None;
            }
            let mut rows: Vec<Vec<(String, Json)>> = Vec::new();
            let mut truncated = false;
            let complete = until_marker(out, &run, |line| match values::row(&line) {
                Some(_) if rows.len() >= max_rows => truncated = true,
                Some(fields) => rows.push(fields),
                None => {}
            })
            .await;
            complete.then_some((description, rows, truncated))
        };
        let read_err = async {
            if describable {
                let described = errors_until(err, &describe).await;
                if !described.complete {
                    return described;
                }
            }
            errors_until(err, &run).await
        };
        let (written, read, errors) = tokio::join!(write, read_out, read_err);
        let Some((description, rows, truncated)) = read.filter(|_| errors.complete && written.is_ok()) else {
            self.ended = true;
            let detail = if errors.lines.is_empty() { "duckdb ended".to_owned() } else { errors.lines.join("\n") };
            return Err(Failure::database("connection", detail));
        };
        let mut columns = described(&description);
        if columns.is_empty()
            && let Some(first) = rows.first()
        {
            columns = first.iter().map(|(name, _)| Column { name: name.clone(), type_name: None }).collect();
        }
        let rows = rows
            .into_iter()
            .map(|fields| {
                let positional = fields.len() == columns.len();
                columns
                    .iter()
                    .enumerate()
                    .map(|(at, column)| {
                        let found = if positional {
                            fields.get(at).map(|(_, value)| value)
                        } else {
                            fields.iter().find(|(name, _)| *name == column.name).map(|(_, value)| value)
                        };
                        found.map_or_else(value::null, |json| values::cell(json, column.type_name.as_deref()))
                    })
                    .collect()
            })
            .collect();
        Ok(Printed { columns, rows, truncated, errors: errors.lines })
    }

    pub(crate) async fn close(mut self) {
        let _ = self.stdin.shutdown().await;
        drop(self.stdin);
        if tokio::time::timeout(std::time::Duration::from_secs(2), self.child.wait()).await.is_err() {
            let _ = self.child.start_kill();
        }
    }
}

pub(crate) fn interrupt(pid: Option<u32>) -> bool {
    #[cfg(unix)]
    {
        let pid = pid.and_then(|pid| i32::try_from(pid).ok()).and_then(rustix::process::Pid::from_raw);
        pid.is_some_and(|pid| rustix::process::kill_process(pid, rustix::process::Signal::INT).is_ok())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

pub(crate) fn error(lines: &[String], statement: &str) -> Failure {
    let context = lines.iter().position(|line| line.starts_with("LINE "));
    let message_lines = &lines[..context.unwrap_or(lines.len())];
    let message: Vec<&str> = message_lines.iter().map(|line| line.trim()).filter(|line| !line.is_empty()).collect();
    let message = message.join(" ");
    let position = context.and_then(|at| {
        let line = &lines[at];
        let (head, shown) = line.split_once(": ")?;
        let number: usize = head.strip_prefix("LINE ")?.trim().parse().ok()?;
        let caret = lines.get(at + 1)?.find('^')?.checked_sub(head.len() + 2)?;
        let source = statement.split('\n').nth(number.checked_sub(1)?)?;
        let shown = shown.strip_prefix("...").unwrap_or(shown);
        let skipped = line.len() - head.len() - 2 - shown.len();
        let offset = caret.checked_sub(skipped)?;
        let found = source.find(shown.trim_end_matches("...")).or_else(|| (skipped == 0).then_some(0))?;
        let before: usize = statement.split('\n').take(number - 1).map(|line| line.chars().count() + 1).sum();
        let column = source.get(..(found + offset).min(source.len()))?.chars().count();
        Some(before + column + 1)
    });
    Failure::server(if message.is_empty() { "duckdb failed".to_owned() } else { message }, None, position)
}

#[cfg(test)]
mod tests;
