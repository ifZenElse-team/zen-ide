use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value as Json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{OnceCell, mpsc, oneshot};
use zen_secrets::Secret;
use zeroize::Zeroizing;

use super::protocol::{self, RpcError};
use super::{EXIT_GRACE, ExternalSpec, lock};
use crate::driver::{DbError, ErrorKind};
use crate::model::Capabilities;

const LOG_LINES: usize = 500;
const STDERR_TAIL: usize = 8;
const LONGEST_LOG_LINE: usize = 2000;
const LONGEST_STDERR_LINE: usize = 64 * 1024;
const KEPT_SECRETS: usize = 32;

#[derive(Clone, Default)]
pub struct DriverLog {
    lines: Arc<Mutex<VecDeque<String>>>,
}

impl DriverLog {
    pub fn push(&self, line: impl Into<String>) {
        let line = shortened(line.into());
        let mut lines = lock(&self.lines);
        lines.push_back(line);
        while lines.len() > LOG_LINES {
            lines.pop_front();
        }
    }

    pub fn tail(&self, limit: usize) -> (Vec<String>, bool) {
        let lines = lock(&self.lines);
        let skip = lines.len().saturating_sub(limit);
        (lines.iter().skip(skip).cloned().collect(), skip > 0)
    }
}

fn shortened(mut line: String) -> String {
    if line.len() > LONGEST_LOG_LINE {
        let mut end = LONGEST_LOG_LINE;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
        line.push('…');
    }
    line
}

enum Outgoing {
    Line(Zeroizing<Vec<u8>>),
    Close,
}

type Answer = Result<Json, RpcError>;

#[derive(Serialize)]
struct Request<'a, P> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: &'a P,
}

pub(crate) struct Process {
    key: String,
    pid: Option<u32>,
    outbox: mpsc::UnboundedSender<Outgoing>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
    next: AtomicU64,
    alive: AtomicBool,
    stopping: AtomicBool,
    sessions: AtomicUsize,
    ready: OnceCell<Result<Capabilities, DbError>>,
    log: DriverLog,
    secrets: Mutex<Vec<Secret>>,
    stderr: Mutex<VecDeque<String>>,
    ended: Mutex<Option<String>>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
    on_crash: Box<dyn Fn() + Send + Sync>,
}

struct Pending<'a> {
    process: &'a Process,
    id: u64,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        lock(&self.process.pending).remove(&self.id);
    }
}

impl Process {
    pub(crate) fn spawn(
        spec: &ExternalSpec,
        key: &str,
        log: DriverLog,
        on_crash: Box<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>, DbError> {
        let mut command = tokio::process::Command::new(&spec.program);
        command
            .args(&spec.declaration.args)
            .env("ZEN_DB_DRIVER_PROTOCOL", protocol::VERSION.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(folder) = spec.program.parent().filter(|folder| folder.is_dir()) {
            command.current_dir(folder);
        }
        let mut child = command.spawn().map_err(|error| {
            log.push(format!("cannot start {}: {error}", spec.program.display()));
            DbError::new(ErrorKind::Connection, format!("cannot start the driver program: {error}"))
        })?;
        let pid = child.id();
        log.push(format!("started {} (process {})", spec.program.display(), pid.unwrap_or_default()));
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(DbError::new(ErrorKind::Connection, "the driver program has no standard input or output"));
        };
        let (outbox, mut receiver) = mpsc::unbounded_channel::<Outgoing>();
        let (kill, killed) = oneshot::channel::<()>();
        let process = Arc::new(Self {
            key: key.to_owned(),
            pid,
            outbox,
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            alive: AtomicBool::new(true),
            stopping: AtomicBool::new(false),
            sessions: AtomicUsize::new(0),
            ready: OnceCell::new(),
            log,
            secrets: Mutex::new(Vec::new()),
            stderr: Mutex::new(VecDeque::new()),
            ended: Mutex::new(None),
            kill: Mutex::new(Some(kill)),
            on_crash,
        });
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(message) = receiver.recv().await {
                match message {
                    Outgoing::Line(line) => {
                        if stdin.write_all(&line).await.is_err() || stdin.flush().await.is_err() {
                            break;
                        }
                    }
                    Outgoing::Close => break,
                }
            }
            let _ = stdin.shutdown().await;
        });
        let reading = Arc::clone(&process);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let reason = loop {
                let mut line = Vec::new();
                let limit = u64::try_from(protocol::MAX_LINE).unwrap_or(u64::MAX).saturating_add(1);
                match (&mut reader).take(limit).read_until(b'\n', &mut line).await {
                    Ok(0) => break "its standard output closed".to_owned(),
                    Ok(_) if line.len() > protocol::MAX_LINE => {
                        break format!("it sent a line longer than {} bytes", protocol::MAX_LINE);
                    }
                    Ok(_) => reading.dispatch(&line),
                    Err(error) => break format!("its standard output failed: {error}"),
                }
            };
            reading.end(reason);
            if let Some(kill) = lock(&reading.kill).take() {
                let _ = kill.send(());
            }
        });
        let reading = Arc::clone(&process);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            loop {
                let mut bytes = Vec::new();
                let limit = u64::try_from(LONGEST_STDERR_LINE).unwrap_or(u64::MAX);
                match (&mut reader).take(limit).read_until(b'\n', &mut bytes).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if bytes.len() >= LONGEST_STDERR_LINE && !bytes.ends_with(b"\n") {
                    let mut rest = Vec::new();
                    loop {
                        rest.clear();
                        match (&mut reader).take(limit).read_until(b'\n', &mut rest).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) if rest.ends_with(b"\n") => break,
                            Ok(_) => {}
                        }
                    }
                }
                let text = String::from_utf8_lossy(&bytes);
                let line = shortened(reading.redact(text.trim_end_matches(['\r', '\n'])));
                reading.log.push(format!("stderr: {line}"));
                let mut tail = lock(&reading.stderr);
                tail.push_back(line);
                while tail.len() > STDERR_TAIL {
                    tail.pop_front();
                }
            }
        });
        let waiting = Arc::clone(&process);
        tokio::spawn(async move {
            let status = tokio::select! {
                status = child.wait() => status,
                _ = killed => {
                    let exited = tokio::time::timeout(EXIT_GRACE, child.wait()).await;
                    match exited {
                        Ok(status) => status,
                        Err(_) => {
                            let _ = child.start_kill();
                            waiting.log.push(format!("killed process {} that did not exit", waiting.pid.unwrap_or_default()));
                            child.wait().await
                        }
                    }
                }
            };
            let status = status.map(|status| status.to_string()).unwrap_or_else(|error| error.to_string());
            waiting.log.push(format!("process {} ended: {status}", waiting.pid.unwrap_or_default()));
            waiting.end(format!("it exited ({status})"));
        });
        Ok(process)
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub(crate) fn usable(&self) -> bool {
        self.alive() && !self.stopping.load(Ordering::SeqCst) && !matches!(self.ready.get(), Some(Err(_)))
    }

    pub(crate) fn add_session(&self) {
        self.sessions.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn remove_session(&self) -> usize {
        let left = self.sessions.load(Ordering::SeqCst).saturating_sub(1);
        self.sessions.store(left, Ordering::SeqCst);
        left
    }

    pub(crate) async fn ready<F, Fut>(&self, initialize: F) -> Result<Capabilities, DbError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Capabilities, DbError>>,
    {
        self.ready.get_or_init(initialize).await.clone()
    }

    pub(crate) fn remember(&self, secret: &Secret) {
        if secret.is_empty() {
            return;
        }
        let mut secrets = lock(&self.secrets);
        if !secrets.iter().any(|known| known.expose() == secret.expose()) {
            secrets.push(secret.clone());
            if secrets.len() > KEPT_SECRETS {
                secrets.remove(0);
            }
        }
    }

    pub(crate) fn redact(&self, text: &str) -> String {
        let mut text = text.to_owned();
        for secret in lock(&self.secrets).iter() {
            text = text.replace(secret.expose(), "***");
        }
        crate::registry::redact(&text)
    }

    fn end(&self, reason: String) {
        if !self.alive.swap(false, Ordering::SeqCst) {
            return;
        }
        let stopping = self.stopping.load(Ordering::SeqCst);
        let tail: Vec<String> = lock(&self.stderr).iter().cloned().collect();
        let mut detail = format!("the driver program stopped: {reason}");
        if !tail.is_empty() {
            detail.push_str("; it wrote: ");
            detail.push_str(&tail.join(" | "));
        }
        *lock(&self.ended) = Some(detail.clone());
        if !stopping {
            self.log.push(format!("process {} stopped by itself: {reason}", self.pid.unwrap_or_default()));
            (self.on_crash)();
        }
        let waiting: Vec<oneshot::Sender<Answer>> = lock(&self.pending).drain().map(|(_, sender)| sender).collect();
        for sender in waiting {
            let _ = sender.send(Err(RpcError { code: protocol::INTERNAL_ERROR, message: detail.clone(), data: None }));
        }
    }

    pub(crate) fn lost(&self) -> DbError {
        let detail = lock(&self.ended).clone().unwrap_or_else(|| "the driver program stopped".to_owned());
        DbError::new(ErrorKind::Connection, detail)
    }

    fn dispatch(&self, line: &[u8]) {
        let text = line.trim_ascii();
        if text.is_empty() {
            return;
        }
        let Ok(message) = serde_json::from_slice::<Json>(text) else {
            self.log.push("the driver sent a line that is not JSON");
            return;
        };
        let id = message.get("id").and_then(Json::as_u64);
        let method = message.get("method").and_then(Json::as_str);
        match (id, method) {
            (Some(id), None) => {
                let answer = match message.get("error") {
                    Some(error) => Err(serde_json::from_value(error.clone()).unwrap_or(RpcError {
                        code: protocol::INTERNAL_ERROR,
                        message: "the driver sent an unreadable error".to_owned(),
                        data: None,
                    })),
                    None => Ok(message.get("result").cloned().unwrap_or(Json::Null)),
                };
                if let Some(sender) = lock(&self.pending).remove(&id) {
                    let _ = sender.send(answer);
                }
            }
            (None, Some("log")) => {
                let text = message.pointer("/params/message").and_then(Json::as_str).unwrap_or_default();
                let level = message.pointer("/params/level").and_then(Json::as_str).unwrap_or("info");
                self.log.push(format!("{level}: {}", self.redact(text)));
            }
            (Some(_), Some(method)) => {
                let answer = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "error": { "code": protocol::METHOD_NOT_FOUND, "message": format!("the IDE has no method `{method}`") },
                });
                self.send(serde_json::to_vec(&answer).unwrap_or_default());
            }
            _ => {}
        }
    }

    fn send(&self, mut bytes: Vec<u8>) {
        bytes.push(b'\n');
        let _ = self.outbox.send(Outgoing::Line(Zeroizing::new(bytes)));
    }

    pub(crate) async fn request<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
        deadline: Option<Duration>,
    ) -> Result<T, DbError> {
        if !self.alive() {
            return Err(self.lost());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let mut bytes = Zeroizing::new(
            serde_json::to_vec(&Request { jsonrpc: "2.0", id, method, params })
                .map_err(|error| DbError::new(ErrorKind::Invalid, error.to_string()))?,
        );
        bytes.push(b'\n');
        let (sender, receiver) = oneshot::channel();
        lock(&self.pending).insert(id, sender);
        let _pending = Pending { process: self, id };
        if self.outbox.send(Outgoing::Line(bytes)).is_err() {
            return Err(self.lost());
        }
        let answer = match deadline {
            Some(deadline) => match tokio::time::timeout(deadline, receiver).await {
                Ok(answer) => answer,
                Err(_) => {
                    self.log.push(format!("`{method}` got no answer within {} ms", deadline.as_millis()));
                    return Err(DbError::new(
                        ErrorKind::Timeout,
                        format!("the driver did not answer `{method}` in time"),
                    ));
                }
            },
            None => receiver.await,
        };
        match answer {
            Ok(Ok(result)) => serde_json::from_value(result).map_err(|error| {
                self.log.push(format!("`{method}` answered something unreadable: {error}"));
                DbError::new(ErrorKind::Invalid, format!("the driver's answer to `{method}` is not valid: {error}"))
            }),
            Ok(Err(error)) if !self.alive() && error.code == protocol::INTERNAL_ERROR && error.data.is_none() => {
                Err(self.lost())
            }
            Ok(Err(error)) => {
                let mut error = error.to_db_error();
                error.detail = self.redact(&error.detail);
                Err(error)
            }
            Err(_) => Err(self.lost()),
        }
    }

    pub(crate) fn notify<P: Serialize>(&self, method: &str, params: &P) {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let request = serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.send(serde_json::to_vec(&request).unwrap_or_default());
    }

    pub(crate) fn shutdown(self: &Arc<Self>) {
        if self.stopping.swap(true, Ordering::SeqCst) || !self.alive() {
            return;
        }
        self.log.push(format!("stopping process {}", self.pid.unwrap_or_default()));
        let process = Arc::clone(self);
        crate::engine::runtime().spawn(async move {
            let _: Result<Json, DbError> = process.request("shutdown", &serde_json::json!({}), Some(EXIT_GRACE)).await;
            let _ = process.outbox.send(Outgoing::Close);
            if let Some(kill) = lock(&process.kill).take() {
                let _ = kill.send(());
            }
        });
    }
}
