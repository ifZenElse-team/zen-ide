use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde_json::{Value, json};

use crate::LspError;
use crate::log::ServerLog;
use crate::outbox::{Outbox, QUEUE_LIMIT};

pub const REQUEST_CANCELLED: i64 = -32800;
pub const CONTENT_MODIFIED: i64 = -32801;
pub const SERVER_CANCELLED: i64 = -32802;
pub const METHOD_NOT_FOUND: i64 = -32601;

pub const MAX_MESSAGE_BYTES: usize = 128 * 1024 * 1024;

pub fn write_message(writer: &mut dyn Write, message: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

pub fn read_message(reader: &mut dyn BufRead) -> std::io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.unwrap_or(0);
    if length > MAX_MESSAGE_BYTES {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(std::io::Error::other)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Notification { method: String, params: Value },
    Closed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerRequest {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

pub const FORWARDED_REQUESTS: [&str; 1] = ["workspace/applyEdit"];

type ResponseMap = Mutex<HashMap<i64, Sender<Result<Value, LspError>>>>;
type Responses = Arc<ResponseMap>;

type Configuration = Arc<Mutex<BTreeMap<String, Value>>>;

pub type ContentSource = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

type Content = Arc<Mutex<Option<ContentSource>>>;

pub const CONTENT_REQUESTS: [&str; 2] = ["custom/schema/content", "vscode/content"];

pub const INLAY_HINT_REFRESH: &str = "workspace/inlayHint/refresh";

pub const DIAGNOSTIC_REFRESH: &str = "workspace/diagnostic/refresh";

pub const RENAME_AT: &str = "_typescript.rename";

pub type Waker = Arc<dyn Fn() + Send + Sync>;

static SERIAL: AtomicUsize = AtomicUsize::new(0);

pub struct Connection {
    outbox: Arc<Outbox>,
    pending: Responses,
    serial: usize,
    next_id: AtomicI64,
    alive: Arc<AtomicBool>,
    incoming: Mutex<Receiver<Incoming>>,
    requests: Mutex<Receiver<ServerRequest>>,
    waker: Arc<Mutex<Option<Waker>>>,
    trace: Trace,
    configuration: Configuration,
    content: Content,
}

type Trace = Arc<Mutex<Option<ServerLog>>>;

fn record(trace: &Trace, outgoing: bool, message: &Value) {
    let Some(log) = trace.lock().ok().and_then(|slot| slot.clone()) else { return };
    log.push(describe(outgoing, message));
}

pub fn describe(outgoing: bool, message: &Value) -> String {
    let arrow = if outgoing { "-->" } else { "<--" };
    let method = message.get("method").and_then(Value::as_str);
    let id = message.get("id").map(Value::to_string);
    match (method, id) {
        (Some(method), Some(id)) => format!("{arrow} request {method} ({id})"),
        (Some(method), None) => match message["params"]["message"].as_str() {
            Some(text) if method.starts_with("window/") => format!("{arrow} {method}: {text}"),
            _ => format!("{arrow} notification {method}"),
        },
        (None, Some(id)) if message.get("error").is_some() => {
            format!("{arrow} error ({id}): {}", message["error"]["message"].as_str().unwrap_or_default())
        }
        (None, Some(id)) => format!("{arrow} response ({id})"),
        (None, None) => format!("{arrow} unknown message"),
    }
}

impl Connection {
    pub fn new(reader: impl Read + Send + 'static, writer: impl Write + Send + 'static) -> Self {
        Self::with_limit(reader, writer, QUEUE_LIMIT)
    }

    pub fn with_limit(reader: impl Read + Send + 'static, writer: impl Write + Send + 'static, limit: usize) -> Self {
        let pending: Responses = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let (events, incoming) = mpsc::channel();
        let (forward, requests) = mpsc::channel();
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let trace: Trace = Arc::default();
        let thread_trace = Arc::clone(&trace);
        let configuration: Configuration = Arc::default();
        let thread_configuration = Arc::clone(&configuration);
        let content: Content = Arc::default();
        let thread_content = Arc::clone(&content);
        let (thread_pending, thread_alive) = (Arc::clone(&pending), Arc::clone(&alive));
        let thread_waker = Arc::clone(&waker);
        let wake: Waker = Arc::new(move || {
            if let Some(wake) = thread_waker.lock().ok().and_then(|waker| waker.clone()) {
                wake();
            }
        });
        let written_trace = Arc::clone(&trace);
        let (stop_pending, stop_alive, stop_trace, stop_wake) =
            (Arc::clone(&pending), Arc::clone(&alive), Arc::clone(&trace), Arc::clone(&wake));
        let outbox = Arc::new(Outbox::new(
            Box::new(BufWriter::new(writer)),
            limit,
            Box::new(move |message| record(&written_trace, true, message)),
            Box::new(move |why| {
                stop_alive.store(false, Ordering::SeqCst);
                if let Some(log) = stop_trace.lock().ok().and_then(|slot| slot.clone()) {
                    log.push(format!("--- {why}"));
                }
                fail_pending(&stop_pending);
                stop_wake();
            }),
        ));
        let reply = Arc::downgrade(&outbox);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            while let Ok(Some(message)) = read_message(&mut reader) {
                record(&thread_trace, false, &message);
                let shared = Shared { configuration: &thread_configuration, content: &thread_content };
                if dispatch(message, &reply, &thread_pending, &events, shared, &forward) {
                    wake();
                }
            }
            thread_alive.store(false, Ordering::SeqCst);
            fail_pending(&thread_pending);
            let _ = events.send(Incoming::Closed);
            wake();
        });
        Self {
            outbox,
            pending,
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
            next_id: AtomicI64::new(1),
            alive,
            incoming: Mutex::new(incoming),
            requests: Mutex::new(requests),
            waker,
            trace,
            configuration,
            content,
        }
    }

    pub fn set_content_source(&self, source: Option<ContentSource>) {
        if let Ok(mut slot) = self.content.lock() {
            *slot = source;
        }
    }

    pub fn set_configuration(&self, section: &str, settings: Value) {
        if let Ok(mut configuration) = self.configuration.lock() {
            configuration.insert(section.to_owned(), settings);
        }
    }

    pub fn set_trace(&self, log: Option<ServerLog>) {
        if let Ok(mut slot) = self.trace.lock() {
            *slot = log;
        }
    }

    pub fn set_waker(&self, waker: Option<Waker>) {
        if let Ok(mut slot) = self.waker.lock() {
            *slot = waker;
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn send(&self, message: Value) -> Result<(), LspError> {
        if !self.is_alive() {
            return Err(LspError::Closed);
        }
        self.outbox.push(message).map_err(|_| LspError::Closed)
    }

    pub fn queued(&self) -> usize {
        self.outbox.queued()
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Pending, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, sender);
        }
        let sent = self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        if let Err(error) = sent {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&id);
            }
            return Err(error);
        }
        Ok(Pending { id, connection: self.serial, receiver, waiters: Arc::downgrade(&self.pending) })
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    pub fn cancel(&self, pending: &Pending) {
        if pending.connection != self.serial {
            return;
        }
        if self.outbox.withdraw(pending.id) {
            if let Some(waiting) = self.pending.lock().ok().and_then(|mut map| map.remove(&pending.id)) {
                let message = "cancelled before it was sent".to_owned();
                let _ = waiting.send(Err(LspError::Server { code: REQUEST_CANCELLED, message }));
            }
            return;
        }
        let _ = self.notify("$/cancelRequest", json!({ "id": pending.id }));
    }

    pub fn work_canceller(&self, token: Value) -> impl Fn() + Send + Sync + 'static {
        let outbox = Arc::downgrade(&self.outbox);
        let cancel = json!({
            "jsonrpc": "2.0",
            "method": "window/workDoneProgress/cancel",
            "params": { "token": token },
        });
        move || {
            if let Some(outbox) = outbox.upgrade() {
                let _ = outbox.push(cancel.clone());
            }
        }
    }

    pub fn respond(&self, id: &Value, result: Value) -> Result<(), LspError> {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    pub fn take_requests(&self) -> Vec<ServerRequest> {
        self.requests.lock().map(|requests| requests.try_iter().collect()).unwrap_or_default()
    }

    pub fn take_incoming(&self) -> Vec<Incoming> {
        self.incoming.lock().map(|incoming| incoming.try_iter().collect()).unwrap_or_default()
    }

    pub fn next_incoming(&self, timeout: Duration) -> Option<Incoming> {
        self.incoming.lock().ok()?.recv_timeout(timeout).ok()
    }
}

#[derive(Debug)]
pub struct Pending {
    id: i64,
    connection: usize,
    receiver: Receiver<Result<Value, LspError>>,
    waiters: Weak<ResponseMap>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(waiters) = self.waiters.upgrade()
            && let Ok(mut waiters) = waiters.lock()
        {
            waiters.remove(&self.id);
        }
    }
}

impl Pending {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn wait(&self, timeout: Duration) -> Result<Value, LspError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout) => Err(LspError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(LspError::Closed),
        }
    }

    pub fn try_answer(&self) -> Option<Result<Value, LspError>> {
        self.receiver.try_recv().ok()
    }
}

fn fail_pending(pending: &Responses) {
    for (_, waiting) in pending.lock().map(|mut map| std::mem::take(&mut *map)).unwrap_or_default() {
        let _ = waiting.send(Err(LspError::Closed));
    }
}

#[derive(Clone, Copy)]
struct Shared<'a> {
    configuration: &'a Mutex<BTreeMap<String, Value>>,
    content: &'a Mutex<Option<ContentSource>>,
}

fn content_answer(content: &Mutex<Option<ContentSource>>, params: &Value) -> Result<Value, String> {
    let uri = match params {
        Value::String(uri) => uri.as_str(),
        Value::Array(items) => items.first().and_then(Value::as_str).unwrap_or_default(),
        _ => "",
    };
    let source = content.lock().ok().and_then(|slot| slot.clone()).ok_or_else(|| format!("{uri}: no content here"))?;
    source(uri).map(Value::String)
}

fn dispatch(
    message: Value,
    writer: &Weak<Outbox>,
    pending: &Mutex<HashMap<i64, Sender<Result<Value, LspError>>>>,
    events: &Sender<Incoming>,
    shared: Shared<'_>,
    forward: &Sender<ServerRequest>,
) -> bool {
    let configuration = shared.configuration;
    let method = message.get("method").and_then(Value::as_str).map(str::to_owned);
    let id = message.get("id").cloned();
    match (method, id) {
        (None, Some(id)) => {
            let Some(id) = id.as_i64() else { return false };
            let Some(waiting) = pending.lock().ok().and_then(|mut map| map.remove(&id)) else { return false };
            let answer = match message.get("error") {
                Some(error) => Err(LspError::Server {
                    code: error["code"].as_i64().unwrap_or(0),
                    message: error["message"].as_str().unwrap_or_default().to_owned(),
                }),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = waiting.send(answer);
            true
        }
        (Some(method), Some(id)) if FORWARDED_REQUESTS.contains(&method.as_str()) => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let _ = forward.send(ServerRequest { id, method, params });
            true
        }
        (Some(method), Some(id)) => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let mut refreshed = false;
            let answer = match method.as_str() {
                "workspace/configuration" => {
                    let known = configuration.lock().map(|known| known.clone()).unwrap_or_default();
                    let result: Vec<Value> = params["items"]
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .map(|item| {
                                    known.get(item["section"].as_str().unwrap_or("")).cloned().unwrap_or(Value::Null)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    json!({ "jsonrpc": "2.0", "id": id, "result": result })
                }
                INLAY_HINT_REFRESH => {
                    let _ = events
                        .send(Incoming::Notification { method: INLAY_HINT_REFRESH.to_owned(), params: Value::Null });
                    refreshed = true;
                    json!({ "jsonrpc": "2.0", "id": id, "result": null })
                }
                DIAGNOSTIC_REFRESH => {
                    let _ = events
                        .send(Incoming::Notification { method: DIAGNOSTIC_REFRESH.to_owned(), params: Value::Null });
                    refreshed = true;
                    json!({ "jsonrpc": "2.0", "id": id, "result": null })
                }
                RENAME_AT => {
                    let params = message.get("params").cloned().unwrap_or(Value::Null);
                    let _ = events.send(Incoming::Notification { method: RENAME_AT.to_owned(), params });
                    json!({ "jsonrpc": "2.0", "id": id, "result": null })
                }
                method if CONTENT_REQUESTS.contains(&method) => match content_answer(shared.content, &params) {
                    Ok(text) => json!({ "jsonrpc": "2.0", "id": id, "result": text }),
                    Err(why) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": why } }),
                },
                "window/workDoneProgress/create"
                | "client/registerCapability"
                | "client/unregisterCapability"
                | "window/showMessageRequest"
                | "workspace/semanticTokens/refresh"
                | "workspace/codeLens/refresh" => json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                _ => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": METHOD_NOT_FOUND, "message": format!("{method} is not supported") },
                }),
            };
            if let Some(outbox) = writer.upgrade() {
                let _ = outbox.push(answer);
            }
            refreshed
        }
        (Some(method), None) => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let _ = events.send(Incoming::Notification { method, params });
            true
        }
        (None, None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slow_server(limit: usize, pause: Duration) -> (Connection, Sender<()>, std::thread::JoinHandle<Vec<Value>>) {
        let (from_client, to_server) = std::io::pipe().unwrap();
        let (from_server, _to_client) = std::io::pipe().unwrap();
        let (go, wait) = mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let _keep_open = _to_client;
            let _ = wait.recv();
            let mut reader = BufReader::new(from_client);
            let mut seen = Vec::new();
            while let Ok(Some(message)) = read_message(&mut reader) {
                seen.push(message);
                std::thread::sleep(pause);
            }
            seen
        });
        (Connection::with_limit(from_server, to_server, limit), go, server)
    }

    #[test]
    fn a_server_that_reads_slowly_does_not_slow_the_sender() {
        let (connection, go, server) = slow_server(QUEUE_LIMIT, Duration::from_millis(1));
        let text = "x".repeat(8 * 1024);
        for index in 0..300 {
            if index % 3 == 2 {
                connection.request("textDocument/inlayHint", json!({ "index": index })).unwrap();
            } else {
                connection.notify("test/note", json!({ "index": index, "text": text })).unwrap();
            }
        }
        assert!(connection.queued() > 200);
        go.send(()).unwrap();
        drop(connection);
        let seen = server.join().unwrap();
        let order: Vec<i64> = seen.iter().map(|message| message["params"]["index"].as_i64().unwrap()).collect();
        assert_eq!(order, (0..300).collect::<Vec<_>>());
    }

    #[test]
    fn typing_while_the_server_is_busy_keeps_the_queue_short() {
        let (connection, go, server) = slow_server(QUEUE_LIMIT, Duration::ZERO);
        connection.notify("test/large", json!("x".repeat(1024 * 1024))).unwrap();
        while connection.queued() > 0 {
            std::thread::yield_now();
        }
        let range = json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } });
        let mut asked: Option<Pending> = None;
        for version in 1..=1000 {
            let change = json!({ "textDocument": { "uri": "file:///a.go", "version": version },
                "contentChanges": [{ "range": range, "text": "x" }] });
            connection.notify("textDocument/didChange", change).unwrap();
            if let Some(before) = asked.take() {
                connection.cancel(&before);
                let answer = before.wait(Duration::ZERO);
                assert!(matches!(answer, Err(LspError::Server { code: REQUEST_CANCELLED, .. })), "{answer:?}");
            }
            asked = Some(connection.request("textDocument/completion", json!({ "version": version })).unwrap());
            assert!(connection.queued() <= 2, "{} queued", connection.queued());
        }
        go.send(()).unwrap();
        drop(connection);
        let seen = server.join().unwrap();
        let methods: Vec<&str> = seen.iter().map(|message| message["method"].as_str().unwrap()).collect();
        assert_eq!(methods, ["test/large", "textDocument/didChange", "textDocument/completion"]);
        assert_eq!(seen[1]["params"]["textDocument"]["version"], 1000);
        assert_eq!(seen[1]["params"]["contentChanges"].as_array().unwrap().len(), 1000);
        assert_eq!(seen[2]["params"]["version"], 1000);
    }

    #[test]
    fn a_server_that_stops_reading_is_treated_as_stopped() {
        let (connection, _go, _server) = slow_server(256 * 1024, Duration::ZERO);
        let pending = connection.request("textDocument/hover", json!({})).unwrap();
        let text = "x".repeat(32 * 1024);
        let mut failed = false;
        for _ in 0..100 {
            if connection.notify("test/note", json!({ "text": text })).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed, "the queue is bounded");
        assert!(!connection.is_alive());
        assert_eq!(pending.wait(Duration::from_secs(1)), Err(LspError::Closed));
        assert_eq!(connection.notify("test/note", json!({})), Err(LspError::Closed));
    }

    #[test]
    fn a_request_of_another_connection_is_not_cancelled() {
        let (first, _go_first, _first_server) = slow_server(QUEUE_LIMIT, Duration::ZERO);
        let (second, go, server) = slow_server(QUEUE_LIMIT, Duration::ZERO);
        let old = first.request("callHierarchy/incomingCalls", json!({})).unwrap();
        let new = second.request("textDocument/hover", json!({})).unwrap();
        assert_eq!(old.id(), new.id());
        second.cancel(&old);
        assert_eq!(new.try_answer(), None);
        go.send(()).unwrap();
        drop(second);
        let seen = server.join().unwrap();
        let methods: Vec<&str> = seen.iter().map(|message| message["method"].as_str().unwrap()).collect();
        assert_eq!(methods, ["textDocument/hover"]);
    }

    #[test]
    fn a_huge_length_is_refused_and_a_dropped_request_leaves_no_waiter() {
        let header = format!("Content-Length: {}\r\n\r\n{{}}", usize::MAX / 2);
        let error = read_message(&mut std::io::Cursor::new(header.into_bytes())).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let (connection, _go, _server) = slow_server(QUEUE_LIMIT, Duration::ZERO);
        let pending = connection.request("textDocument/hover", json!({})).unwrap();
        assert_eq!(connection.pending.lock().unwrap().len(), 1);
        drop(pending);
        assert!(connection.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn frames_round_trip() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &json!({ "a": "é" })).unwrap();
        assert!(bytes.starts_with(b"Content-Length: 10\r\n\r\n"));
        let mut with_type = b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n".to_vec();
        with_type.extend_from_slice(&bytes);
        let mut reader = std::io::Cursor::new(with_type);
        assert_eq!(read_message(&mut reader).unwrap(), Some(json!({ "a": "é" })));
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }
}
