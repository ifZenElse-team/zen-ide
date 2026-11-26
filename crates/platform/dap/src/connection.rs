use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::marker::PhantomData;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::DapError;
use crate::protocol::{DebugEvent, Message, Response};

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
    Event(DebugEvent),
    Closed,
}

pub type Waker = Arc<dyn Fn() + Send + Sync>;

type AnswerMap = Mutex<HashMap<i64, Sender<Result<Value, DapError>>>>;
type Answers = Arc<AnswerMap>;
type SharedWriter = Arc<Mutex<Option<Box<dyn Write + Send>>>>;
type Closer = Box<dyn Fn() + Send + Sync>;

pub struct Connection {
    writer: SharedWriter,
    pending: Answers,
    seq: Arc<AtomicI64>,
    alive: Arc<AtomicBool>,
    incoming: Mutex<Receiver<Incoming>>,
    waker: Arc<Mutex<Option<Waker>>>,
    closer: Option<Closer>,
}

impl Connection {
    pub fn new(reader: impl Read + Send + 'static, writer: impl Write + Send + 'static) -> Self {
        Self::start(reader, Box::new(writer), None)
    }

    pub fn tcp(stream: TcpStream) -> std::io::Result<Self> {
        let reader = stream.try_clone()?;
        let closing = stream.try_clone()?;
        let closer: Closer = Box::new(move || {
            let _ = closing.shutdown(Shutdown::Both);
        });
        Ok(Self::start(reader, Box::new(stream), Some(closer)))
    }

    fn start(reader: impl Read + Send + 'static, writer: Box<dyn Write + Send>, closer: Option<Closer>) -> Self {
        let writer: SharedWriter = Arc::new(Mutex::new(Some(writer)));
        let pending: Answers = Arc::default();
        let seq = Arc::new(AtomicI64::new(1));
        let alive = Arc::new(AtomicBool::new(true));
        let (events, incoming) = mpsc::channel();
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let reading = Reading {
            writer: Arc::clone(&writer),
            pending: Arc::clone(&pending),
            seq: Arc::clone(&seq),
            events,
            waker: Arc::clone(&waker),
        };
        let thread_alive = Arc::clone(&alive);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            while let Ok(Some(message)) = read_message(&mut reader) {
                if reading.dispatch(message) {
                    reading.wake();
                }
            }
            thread_alive.store(false, Ordering::SeqCst);
            for (_, waiting) in reading.pending.lock().map(|mut map| std::mem::take(&mut *map)).unwrap_or_default() {
                let _ = waiting.send(Err(DapError::Closed));
            }
            let _ = reading.events.send(Incoming::Closed);
            reading.wake();
        });
        Self { writer, pending, seq, alive, incoming: Mutex::new(incoming), waker, closer }
    }

    pub fn set_waker(&self, waker: Option<Waker>) {
        if let Ok(mut slot) = self.waker.lock() {
            *slot = waker;
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn request(&self, command: &str, arguments: Value) -> Result<Pending<Value>, DapError> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(seq, sender);
        }
        let mut message = json!({ "seq": seq, "type": "request", "command": command });
        if !arguments.is_null() {
            message["arguments"] = arguments;
        }
        if let Err(error) = self.send(&message) {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&seq);
            }
            return Err(error);
        }
        let waiter = Waiter { seq, answers: Arc::downgrade(&self.pending) };
        Ok(Pending { request_seq: seq, receiver, answer: PhantomData, waiter })
    }

    fn send(&self, message: &Value) -> Result<(), DapError> {
        if !self.is_alive() {
            return Err(DapError::Closed);
        }
        let mut writer = self.writer.lock().map_err(|_| DapError::Closed)?;
        let writer = writer.as_mut().ok_or(DapError::Closed)?;
        write_message(&mut **writer, message).map_err(|error| DapError::Io(error.to_string()))
    }

    pub fn take_incoming(&self) -> Vec<Incoming> {
        self.incoming.lock().map(|incoming| incoming.try_iter().collect()).unwrap_or_default()
    }

    pub fn next_incoming(&self, timeout: Duration) -> Option<Incoming> {
        self.incoming.lock().ok()?.recv_timeout(timeout).ok()
    }

    pub fn close(&self) {
        if let Ok(mut writer) = self.writer.lock() {
            *writer = None;
        }
        if let Some(closer) = &self.closer {
            closer();
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug)]
pub struct Pending<T = Value> {
    request_seq: i64,
    receiver: Receiver<Result<Value, DapError>>,
    answer: PhantomData<fn() -> T>,
    waiter: Waiter,
}

#[derive(Debug)]
struct Waiter {
    seq: i64,
    answers: std::sync::Weak<AnswerMap>,
}

impl Drop for Waiter {
    fn drop(&mut self) {
        if let Some(answers) = self.answers.upgrade()
            && let Ok(mut answers) = answers.lock()
        {
            answers.remove(&self.seq);
        }
    }
}

impl<T: DeserializeOwned> Pending<T> {
    pub fn request_seq(&self) -> i64 {
        self.request_seq
    }

    pub fn typed<U: DeserializeOwned>(self) -> Pending<U> {
        Pending { request_seq: self.request_seq, receiver: self.receiver, answer: PhantomData, waiter: self.waiter }
    }

    pub fn wait(&self, timeout: Duration) -> Result<T, DapError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(answer) => answer.and_then(decode),
            Err(RecvTimeoutError::Timeout) => Err(DapError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(DapError::Closed),
        }
    }

    pub fn try_answer(&self) -> Option<Result<T, DapError>> {
        self.receiver.try_recv().ok().map(|answer| answer.and_then(decode))
    }
}

fn decode<T: DeserializeOwned>(body: Value) -> Result<T, DapError> {
    match serde_json::from_value(body.clone()) {
        Ok(answer) => Ok(answer),
        Err(error) if body.is_null() => serde_json::from_value(json!({})).map_err(|_| DapError::Io(error.to_string())),
        Err(error) => Err(DapError::Io(zen_i18n::t!("dap-answer-unreadable", error = error.to_string()))),
    }
}

pub fn failure_message(response: &Response) -> String {
    let error = response.body.as_ref().and_then(|body| body.get("error"));
    if let Some(format) = error.and_then(|error| error["format"].as_str()) {
        let mut text = format.to_owned();
        if let Some(variables) = error.and_then(|error| error["variables"].as_object()) {
            for (name, value) in variables {
                text = text.replace(&format!("{{{name}}}"), value.as_str().unwrap_or_default());
            }
        }
        return text;
    }
    response
        .message
        .clone()
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| zen_i18n::t!("dap-request-failed", command = response.command.as_str()))
}

struct Reading {
    writer: SharedWriter,
    pending: Answers,
    seq: Arc<AtomicI64>,
    events: Sender<Incoming>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl Reading {
    fn wake(&self) {
        if let Some(wake) = self.waker.lock().ok().and_then(|waker| waker.clone()) {
            wake();
        }
    }

    fn dispatch(&self, message: Value) -> bool {
        let Ok(message) = serde_json::from_value::<Message>(message) else { return false };
        match message {
            Message::Response(response) => {
                let Some(waiting) = self.pending.lock().ok().and_then(|mut map| map.remove(&response.request_seq))
                else {
                    return false;
                };
                let answer = if response.success {
                    Ok(response.body.clone().unwrap_or(Value::Null))
                } else {
                    Err(DapError::Adapter { message: failure_message(&response) })
                };
                let _ = waiting.send(answer);
                true
            }
            Message::Event(event) => {
                let body = event.body.unwrap_or(Value::Null);
                let _ = self.events.send(Incoming::Event(DebugEvent::from_event(&event.event, body)));
                true
            }
            Message::Request(request) => {
                let answer = json!({
                    "seq": self.seq.fetch_add(1, Ordering::SeqCst),
                    "type": "response",
                    "request_seq": request.seq,
                    "success": false,
                    "command": request.command,
                    "message": "not supported",
                });
                if let Ok(mut writer) = self.writer.lock()
                    && let Some(writer) = writer.as_mut()
                {
                    let _ = write_message(&mut **writer, &answer);
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct Trickle {
        bytes: Vec<u8>,
        at: usize,
    }

    impl Read for Trickle {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.at >= self.bytes.len() || buffer.is_empty() {
                return Ok(0);
            }
            buffer[0] = self.bytes[self.at];
            self.at += 1;
            Ok(1)
        }
    }

    #[test]
    fn frames_round_trip() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &json!({ "a": "é" })).unwrap();
        assert!(bytes.starts_with(b"Content-Length: 10\r\n\r\n"));
        write_message(&mut bytes, &json!({ "b": 2 })).unwrap();
        let mut reader = Cursor::new(bytes);
        assert_eq!(read_message(&mut reader).unwrap(), Some(json!({ "a": "é" })));
        assert_eq!(read_message(&mut reader).unwrap(), Some(json!({ "b": 2 })));
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }

    #[test]
    fn frames_are_read_from_partial_headers_and_bodies() {
        let mut bytes = b"content-length: 8\r\nX-Other: 1\r\n\r\n".to_vec();
        bytes.extend_from_slice(br#"{"c":31}"#);
        let mut reader = BufReader::with_capacity(1, Trickle { bytes, at: 0 });
        assert_eq!(read_message(&mut reader).unwrap(), Some(json!({ "c": 31 })));
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }

    #[test]
    fn a_huge_length_is_refused_without_reserving_it() {
        let header = format!("Content-Length: {}\r\n\r\n{{}}", usize::MAX / 2);
        let error = read_message(&mut Cursor::new(header.into_bytes())).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_dropped_request_leaves_no_waiter() {
        let (_from_client, to_adapter) = std::io::pipe().unwrap();
        let (from_adapter, _to_client) = std::io::pipe().unwrap();
        let connection = Connection::new(from_adapter, to_adapter);
        let pending = connection.request("threads", Value::Null).unwrap().typed::<Value>();
        assert_eq!(connection.pending.lock().unwrap().len(), 1);
        assert_eq!(pending.wait(Duration::from_millis(1)), Err(DapError::Timeout));
        drop(pending);
        assert!(connection.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn a_truncated_body_is_an_error() {
        let mut reader = Cursor::new(b"Content-Length: 20\r\n\r\n{}".to_vec());
        assert!(read_message(&mut reader).is_err());
    }

    #[test]
    fn answers_find_their_requests_and_events_keep_their_order() {
        let (from_client, to_adapter) = std::io::pipe().unwrap();
        let (from_adapter, mut to_client) = std::io::pipe().unwrap();
        let connection = Connection::new(from_adapter, to_adapter);
        let first = connection.request("threads", Value::Null).unwrap();
        let second = connection.request("scopes", json!({ "frameId": 1 })).unwrap();
        let mut reader = BufReader::new(from_client);
        let sent = [read_message(&mut reader).unwrap().unwrap(), read_message(&mut reader).unwrap().unwrap()];
        assert_eq!(sent[0], json!({ "seq": 1, "type": "request", "command": "threads" }));
        assert_eq!(sent[1]["arguments"], json!({ "frameId": 1 }));
        for (index, name) in ["output", "stopped", "thread"].iter().enumerate() {
            write_message(&mut to_client, &json!({ "seq": index, "type": "event", "event": name, "body": {} }))
                .unwrap();
        }
        let answer = |seq: i64, body: Value| json!({ "seq": 9, "type": "response", "request_seq": seq, "success": true, "command": "x", "body": body });
        write_message(&mut to_client, &answer(2, json!({ "scopes": [] }))).unwrap();
        write_message(&mut to_client, &answer(1, json!({ "threads": [] }))).unwrap();
        assert_eq!(second.wait(Duration::from_secs(5)).unwrap(), json!({ "scopes": [] }));
        assert_eq!(first.wait(Duration::from_secs(5)).unwrap(), json!({ "threads": [] }));
        let names: Vec<String> = connection
            .take_incoming()
            .into_iter()
            .map(|incoming| match incoming {
                Incoming::Event(DebugEvent::Other { event, .. }) => event,
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(names, ["output", "stopped", "thread"]);
        drop(to_client);
        assert_eq!(connection.next_incoming(Duration::from_secs(5)), Some(Incoming::Closed));
        assert!(!connection.is_alive());
    }

    #[test]
    fn failures_use_the_error_format() {
        let response = Response {
            seq: 4,
            request_seq: 2,
            success: false,
            command: "launch".to_owned(),
            message: Some("Failed to launch".to_owned()),
            body: Some(
                json!({ "error": { "id": 3000, "format": "cannot build {what}", "variables": { "what": "main" } } }),
            ),
        };
        assert_eq!(failure_message(&response), "cannot build main");
        let bare = Response { body: None, message: None, ..response };
        zen_i18n::register(crate::LOCALES);
        assert_eq!(failure_message(&bare), zen_i18n::t!("dap-request-failed", command = "launch"));
        assert_ne!(failure_message(&bare), "dap-request-failed");
    }
}
