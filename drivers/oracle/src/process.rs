use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use zen_db_driver_kit::Failure;

use crate::program;
use crate::target::Target;

const KEEP_ERRORS: usize = 40;
const MOST_BYTES: usize = 64 * 1024 * 1024;

fn most_bytes() -> usize {
    std::env::var("ZEN_DB_ORACLE_MOST_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .map_or(MOST_BYTES, |bytes: usize| bytes.min(MOST_BYTES))
}

const SETTINGS: &str =
    "SET ECHO OFF\nSET DEFINE OFF\nSET SQLBLANKLINES ON\nSET SQLTERMINATOR OFF\nSET EXITCOMMIT OFF\n";

const OUTPUT: &str = "SET TERMOUT ON\nSET FEEDBACK ON\nSET HEADING ON\nSET VERIFY OFF\nSET TIMING OFF\nSET SERVEROUTPUT OFF\nSET LONG 1000000\nSET ARRAYSIZE 500\nSET SQLFORMAT JSON\n";

pub(crate) struct Process {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    errors: Arc<Mutex<VecDeque<String>>>,
    nonce: String,
    seq: u64,
    ended: bool,
    closed: bool,
    folder: Option<std::path::PathBuf>,
}

fn private_folder() -> std::io::Result<std::path::PathBuf> {
    let folder = std::env::temp_dir().join(format!("zen-sqlcl-{}-{}", std::process::id(), nonce()));
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700).create(&folder)?;
    #[cfg(not(unix))]
    std::fs::DirBuilder::new().create(&folder)?;
    Ok(folder)
}

fn nonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher
        .write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    format!("{:016x}", hasher.finish())
}

fn keep(errors: &Mutex<VecDeque<String>>, line: String) {
    let mut errors = errors.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if errors.len() == KEEP_ERRORS {
        errors.pop_front();
    }
    errors.push_back(line);
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

pub(crate) struct Block {
    pub(crate) lines: Vec<String>,
    pub(crate) overflowed: bool,
    pub(crate) complete: bool,
}

impl Process {
    pub(crate) async fn start(target: &Target, autocommit: bool) -> Result<(Self, Vec<String>), Failure> {
        let folder = private_folder()
            .map_err(|error| Failure::database("connection", format!("no folder for SQLcl to run in: {error}")))?;
        let mut command = Command::new(&target.program);
        command.args(["-S", "-R", "3", "-NOLOGINTIME", "/NOLOG"]);
        command.current_dir(&folder);
        command.env_remove("SQLPATH").env_remove("ORACLE_PATH");
        if let Some(home) = &target.java_home {
            command.env("JAVA_HOME", home);
        }
        if let Some(tns) = &target.tns_admin {
            command.env("TNS_ADMIN", tns);
        }
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&folder);
                return Err(program::sqlcl_missing(&format!("{} does not start: {error}", target.program.display())));
            }
        };
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(Failure::database("connection", "SQLcl has no pipes"));
        };
        let errors = Arc::new(Mutex::new(VecDeque::new()));
        let kept = Arc::clone(&errors);
        let reading = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut buffer = Vec::new();
            while let Some(line) = next_line(&mut reader, &mut buffer).await {
                keep(&kept, line);
            }
        });
        let mut process = Process {
            child,
            stdin,
            out: BufReader::new(stdout),
            errors,
            nonce: nonce(),
            seq: 0,
            ended: false,
            closed: false,
            folder: Some(folder),
        };
        let mut input = String::from(SETTINGS);
        input.push_str(&target.connect_lines());
        input.push_str(SETTINGS);
        input.push_str(OUTPUT);
        input.push_str(if autocommit { "SET AUTOCOMMIT ON\n" } else { "SET AUTOCOMMIT OFF\n" });
        let block = process.exchange(&input).await;
        if block.complete {
            return Ok((process, block.lines.iter().map(|line| target.redact(line)).collect()));
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), process.child.wait()).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), reading).await;
        let mut printed = block.lines;
        printed.extend(process.errors());
        printed.retain(|line| !line.trim().is_empty());
        let redacted: Vec<String> = printed.iter().map(|line| target.redact(line)).collect();
        process.closed = true;
        Err(program::ended(&redacted, target.java_home.is_some()))
    }

    pub(crate) fn errors(&self) -> Vec<String> {
        self.errors.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().cloned().collect()
    }

    pub(crate) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }

    async fn send(&mut self, input: &str) -> Result<Block, Failure> {
        if self.ended {
            return Err(Failure::database("connection", "SQLcl has ended"));
        }
        let block = self.exchange(input).await;
        if block.complete {
            return Ok(block);
        }
        self.ended = true;
        let mut said = block.lines;
        said.extend(self.errors());
        let detail = said.iter().rev().find(|line| !line.trim().is_empty()).cloned();
        Err(Failure::database("connection", detail.unwrap_or_else(|| "SQLcl ended".to_owned())))
    }

    async fn exchange(&mut self, input: &str) -> Block {
        self.seq += 1;
        let marker = format!("zen_{}_{}", self.nonce, self.seq);
        let text = format!("{input}PROMPT {marker}\n");
        let marker = marker.as_str();
        let Process { stdin, out, .. } = self;
        let write = async {
            stdin.write_all(text.as_bytes()).await?;
            stdin.flush().await
        };
        let read = async {
            let mut buffer = Vec::new();
            let mut block = Block { lines: Vec::new(), overflowed: false, complete: false };
            let mut bytes = 0;
            let most = most_bytes();
            while let Some(line) = next_line(out, &mut buffer).await {
                if line.trim_end().ends_with(marker) {
                    block.complete = true;
                    return block;
                }
                bytes += line.len();
                if bytes > most {
                    block.overflowed = true;
                } else {
                    block.lines.push(line);
                }
            }
            block
        };
        let (written, mut block) = tokio::join!(write, read);
        block.complete &= written.is_ok();
        block
    }

    pub(crate) async fn command(&mut self, lines: &str) -> Result<Block, Failure> {
        self.send(lines).await
    }

    pub(crate) async fn statement(&mut self, commands: &str, text: &str) -> Result<Block, Failure> {
        if !commands.is_empty() {
            self.send(commands).await?;
        }
        self.send(&format!("{text}\n/\n")).await
    }

    pub(crate) async fn close(mut self) {
        self.closed = true;
        if !self.ended {
            let _ = self.stdin.write_all(b"EXIT ROLLBACK\n").await;
            let _ = self.stdin.flush().await;
        }
        let _ = self.stdin.shutdown().await;
        if tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await.is_err() {
            kill(self.child.id());
            let _ = self.child.start_kill();
        }
        self.remove_folder();
    }

    fn remove_folder(&mut self) {
        if let Some(folder) = self.folder.take() {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if !self.closed {
            kill(self.child.id());
        }
        self.remove_folder();
    }
}

#[cfg(unix)]
fn signal(pid: Option<u32>, signal: rustix::process::Signal) -> bool {
    let pid = pid.and_then(|pid| i32::try_from(pid).ok()).and_then(rustix::process::Pid::from_raw);
    pid.is_some_and(|pid| rustix::process::kill_process_group(pid, signal).is_ok())
}

pub(crate) fn interrupt(pid: Option<u32>) -> bool {
    #[cfg(unix)]
    {
        signal(pid, rustix::process::Signal::INT)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

pub(crate) fn kill(pid: Option<u32>) {
    #[cfg(unix)]
    {
        signal(pid, rustix::process::Signal::KILL);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}
