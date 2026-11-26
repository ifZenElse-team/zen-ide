use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{Value as Json, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

pub mod tls;
pub mod value;

pub const PROTOCOL: u64 = 1;
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const DATABASE_ERROR: i64 = -32000;
pub const UNSUPPORTED_PROTOCOL: i64 = -32010;
pub const UNKNOWN_SESSION: i64 = -32011;
pub const UNKNOWN_CURSOR: i64 = -32012;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub code: i64,
    pub message: String,
    pub data: Option<Json>,
}

impl Failure {
    pub fn database(kind: &str, message: impl Into<String>) -> Self {
        Self { code: DATABASE_ERROR, message: message.into(), data: Some(json!({ "kind": kind })) }
    }

    pub fn server(message: impl Into<String>, code: Option<String>, position: Option<usize>) -> Self {
        let mut data = json!({ "kind": "server" });
        if let Some(code) = code {
            data["code"] = json!(code);
        }
        if let Some(position) = position {
            data["position"] = json!(position);
        }
        Self { code: DATABASE_ERROR, message: message.into(), data: Some(data) }
    }

    pub fn syntax(message: impl Into<String>, position: usize) -> Self {
        Self {
            code: DATABASE_ERROR,
            message: message.into(),
            data: Some(json!({ "kind": "invalid", "position": position })),
        }
    }

    pub fn params(message: impl Into<String>) -> Self {
        Self { code: INVALID_PARAMS, message: message.into(), data: None }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::database("unsupported", message)
    }

    pub fn read_only(message: impl Into<String>) -> Self {
        Self::database("refused", message)
    }

    pub fn cancelled() -> Self {
        Self::database("cancelled", "the statement was cancelled")
    }

    pub fn timeout() -> Self {
        Self::database("timeout", "the statement took longer than its timeout")
    }

    pub fn method(method: &str) -> Self {
        Self { code: METHOD_NOT_FOUND, message: format!("no method `{method}`"), data: None }
    }

    pub fn cursor(cursor: &str) -> Self {
        Self { code: UNKNOWN_CURSOR, message: format!("no cursor `{cursor}`"), data: None }
    }

    pub fn kind(&self) -> Option<&str> {
        self.data.as_ref().and_then(|data| data.get("kind")).and_then(Json::as_str)
    }
}

pub type Answer = Result<Json, Failure>;

#[derive(Clone, Debug)]
pub struct DriverInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub capabilities: Json,
}

pub struct Connected {
    pub server_version: Option<String>,
    pub in_transaction: bool,
    pub session: Box<dyn Session>,
}

pub trait Driver: Send + Sync + 'static {
    fn info(&self) -> DriverInfo;
    fn connect(&self, params: Json) -> BoxFuture<'static, Result<Connected, Failure>>;
}

pub trait Session: Send + 'static {
    fn run(self: Box<Self>, inbox: Inbox) -> BoxFuture<'static, ()>;
}

#[derive(Clone, Debug)]
pub struct Request {
    pub id: Json,
    pub method: String,
    pub params: Json,
}

impl Request {
    pub fn text(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(Json::as_str)
    }

    pub fn number(&self, key: &str) -> Option<u64> {
        self.params.get(key).and_then(Json::as_u64)
    }

    pub fn statement(&self) -> Result<&str, Failure> {
        self.text("statement").ok_or_else(|| Failure::params("no statement"))
    }
}

#[derive(Clone, Debug)]
pub struct Out(mpsc::UnboundedSender<Vec<u8>>);

impl Out {
    fn send(&self, message: &Json) {
        let mut line = serde_json::to_vec(message).unwrap_or_default();
        line.push(b'\n');
        let _ = self.0.send(line);
    }

    pub fn answer(&self, id: &Json, answer: Answer) {
        match answer {
            Ok(result) => self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result })),
            Err(failure) => {
                let mut error = json!({ "code": failure.code, "message": failure.message });
                if let Some(data) = failure.data {
                    error["data"] = data;
                }
                self.send(&json!({ "jsonrpc": "2.0", "id": id, "error": error }));
            }
        }
    }

    pub fn log(&self, level: &str, message: &str) {
        self.send(&json!({ "jsonrpc": "2.0", "method": "log", "params": { "level": level, "message": message } }));
    }
}

pub struct Inbox {
    requests: mpsc::UnboundedReceiver<Request>,
    cancel: watch::Receiver<u64>,
    busy: Arc<AtomicBool>,
    out: Out,
}

impl Inbox {
    pub async fn next(&mut self) -> Option<Request> {
        self.busy.store(false, Ordering::SeqCst);
        let request = self.requests.recv().await?;
        self.busy.store(true, Ordering::SeqCst);
        self.cancel.borrow_and_update();
        Some(request)
    }

    pub fn answer(&self, id: &Json, answer: Answer) {
        self.out.answer(id, answer);
    }

    pub fn out(&self) -> &Out {
        &self.out
    }

    pub fn stop(&self, timeout_ms: Option<u64>) -> Stop {
        Stop {
            cancel: self.cancel.clone(),
            deadline: timeout_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms)),
        }
    }
}

pub struct Stop {
    cancel: watch::Receiver<u64>,
    deadline: Option<Instant>,
}

impl Stop {
    pub async fn fired(&mut self) -> Failure {
        let deadline = self.deadline;
        let timeout = async move {
            match deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            changed = self.cancel.changed() => {
                if changed.is_err() {
                    std::future::pending::<()>().await;
                }
                Failure::cancelled()
            }
            () = timeout => Failure::timeout(),
        }
    }

    pub async fn run<T>(&mut self, work: impl Future<Output = Result<T, Failure>>) -> Result<T, Failure> {
        tokio::select! {
            result = work => result,
            failure = self.fired() => Err(failure),
        }
    }
}

struct Handle {
    requests: mpsc::UnboundedSender<Request>,
    cancel: watch::Sender<u64>,
    busy: Arc<AtomicBool>,
}

impl Handle {
    fn cancel(&self) {
        self.cancel.send_modify(|generation| *generation += 1);
    }
}

type Sessions = Arc<Mutex<HashMap<String, Handle>>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub const THIRD_PARTY_NOTICES: &str = include_str!(concat!(env!("OUT_DIR"), "/THIRD-PARTY-NOTICES"));

pub fn main(driver: impl Driver) {
    if std::env::args().nth(1).as_deref() == Some("--licences") {
        let mut out = std::io::stdout();
        let written = std::io::Write::write_all(&mut out, THIRD_PARTY_NOTICES.as_bytes());
        std::process::exit(i32::from(written.is_err()));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
    let Ok(runtime) = runtime else {
        std::process::exit(1);
    };
    runtime.block_on(serve(Arc::new(driver), tokio::io::stdin(), tokio::io::stdout()));
    runtime.shutdown_timeout(Duration::from_millis(200));
    std::process::exit(0);
}

pub async fn serve<R, W>(driver: Arc<dyn Driver>, input: R, mut output: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (sender, mut lines_out) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer = tokio::spawn(async move {
        while let Some(line) = lines_out.recv().await {
            if output.write_all(&line).await.is_err() || output.flush().await.is_err() {
                break;
            }
        }
    });
    let out = Out(sender);
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let next_session = Arc::new(AtomicU64::new(1));
    let mut initialized = false;
    let mut reader = BufReader::new(input);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let message: Json = match serde_json::from_slice(&line) {
            Ok(message) => message,
            Err(error) => {
                out.answer(&Json::Null, Err(Failure { code: PARSE_ERROR, message: error.to_string(), data: None }));
                continue;
            }
        };
        let id = message.get("id").cloned().unwrap_or(Json::Null);
        let Some(method) = message.get("method").and_then(Json::as_str).map(str::to_owned) else {
            if !id.is_null() {
                out.answer(&id, Err(Failure { code: INVALID_REQUEST, message: "no method".into(), data: None }));
            }
            continue;
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        if method != "initialize" && !initialized {
            out.answer(&id, Err(Failure { code: INVALID_REQUEST, message: "initialize first".into(), data: None }));
            continue;
        }
        let session_of = |params: &Json| params.get("session").and_then(Json::as_str).unwrap_or_default().to_owned();
        match method.as_str() {
            "initialize" => {
                let answer = initialize(driver.as_ref(), &params);
                initialized |= answer.is_ok();
                out.answer(&id, answer);
            }
            "shutdown" => {
                for (_, handle) in lock(&sessions).drain() {
                    handle.cancel();
                }
                out.answer(&id, Ok(json!({})));
                break;
            }
            "connect" => {
                let driver = Arc::clone(&driver);
                let out = out.clone();
                let sessions = Arc::clone(&sessions);
                let number = next_session.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let answer = connect(driver.as_ref(), params, &out, &sessions, format!("s{number}")).await;
                    out.answer(&id, answer);
                });
            }
            "close" => {
                if let Some(handle) = lock(&sessions).remove(&session_of(&params)) {
                    handle.cancel();
                }
                if !id.is_null() {
                    out.answer(&id, Ok(json!({})));
                }
            }
            "cancel" => {
                let session = session_of(&params);
                let answer = match lock(&sessions).get(&session) {
                    Some(handle) => {
                        handle.cancel();
                        Ok(json!({}))
                    }
                    None => Err(unknown_session(&session)),
                };
                out.answer(&id, answer);
            }
            "ping" | "execute" | "query" | "fetch" | "closeCursor" | "apply" | "commit" | "rollback" | "children"
            | "introspect" | "key" => {
                let session = session_of(&params);
                let sessions = lock(&sessions);
                let Some(handle) = sessions.get(&session) else {
                    out.answer(&id, Err(unknown_session(&session)));
                    continue;
                };
                if method == "ping" && handle.busy.load(Ordering::SeqCst) {
                    out.answer(&id, Ok(json!({})));
                    continue;
                }
                if handle.requests.send(Request { id: id.clone(), method, params }).is_err() {
                    out.answer(&id, Err(unknown_session(&session)));
                }
            }
            other => out.answer(&id, Err(Failure::method(other))),
        }
    }
    for (_, handle) in lock(&sessions).drain() {
        handle.cancel();
    }
    drop(out);
    let _ = writer.await;
}

fn unknown_session(session: &str) -> Failure {
    Failure { code: UNKNOWN_SESSION, message: format!("no session `{session}`"), data: None }
}

fn initialize(driver: &dyn Driver, params: &Json) -> Answer {
    let asked = params.get("protocol").and_then(Json::as_u64);
    if asked != Some(PROTOCOL) {
        return Err(Failure {
            code: UNSUPPORTED_PROTOCOL,
            message: format!("protocol {} is not supported", asked.map_or_else(|| "?".to_owned(), |v| v.to_string())),
            data: Some(json!({ "supported": [PROTOCOL] })),
        });
    }
    let info = driver.info();
    Ok(json!({
        "protocol": PROTOCOL,
        "driver": { "id": info.id, "name": info.name, "version": info.version },
        "capabilities": info.capabilities,
    }))
}

async fn connect(driver: &dyn Driver, params: Json, out: &Out, sessions: &Sessions, id: String) -> Answer {
    let timeout = params.get("timeoutMs").and_then(Json::as_u64).filter(|ms| *ms > 0).unwrap_or(15_000);
    let connected = match tokio::time::timeout(Duration::from_millis(timeout), driver.connect(params)).await {
        Ok(connected) => connected?,
        Err(_) => return Err(Failure::database("timeout", "connecting took longer than the connection's timeout")),
    };
    let (requests, receiver) = mpsc::unbounded_channel();
    let (cancel, cancel_receiver) = watch::channel(0);
    let busy = Arc::new(AtomicBool::new(false));
    let inbox = Inbox { requests: receiver, cancel: cancel_receiver, busy: Arc::clone(&busy), out: out.clone() };
    tokio::spawn(connected.session.run(inbox));
    lock(sessions).insert(id.clone(), Handle { requests, cancel, busy });
    Ok(json!({
        "session": id,
        "serverVersion": connected.server_version,
        "inTransaction": connected.in_transaction,
    }))
}

pub fn note(text: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{text}");
}

#[cfg(test)]
mod tests;
