use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex};

use serde_json::Value;

use crate::connection::write_message;

pub const QUEUE_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Stopped,
    Full,
}

pub type OnStop = Box<dyn FnOnce(&str) + Send>;

pub type OnWrite = Box<dyn Fn(&Value) + Send>;

#[derive(Default)]
struct State {
    queue: VecDeque<Queued>,
    bytes: usize,
    closing: bool,
    stopped: bool,
}

struct Queued {
    message: Value,
    weight: usize,
}

const DID_CHANGE: &str = "textDocument/didChange";
const DID_SAVE: &str = "textDocument/didSave";

fn is<'a>(message: &'a Value, method: &str) -> Option<&'a str> {
    if message.get("method").and_then(Value::as_str) != Some(method) || message.get("id").is_some() {
        return None;
    }
    message["params"]["textDocument"]["uri"].as_str()
}

impl State {
    fn add(&mut self, message: Value) {
        let weight = weight(&message);
        self.bytes += weight;
        self.queue.push_back(Queued { message, weight });
        self.settle(self.queue.len() - 1);
    }

    fn withdraw(&mut self, id: i64) -> bool {
        let Some(at) = self.queue.iter().rposition(|queued| {
            queued.message.get("method").is_some() && queued.message.get("id").and_then(Value::as_i64) == Some(id)
        }) else {
            return false;
        };
        self.remove(at);
        self.settle(at.saturating_sub(3));
        true
    }

    fn remove(&mut self, at: usize) -> Option<Queued> {
        let removed = self.queue.remove(at)?;
        self.bytes -= removed.weight;
        Some(removed)
    }

    fn settle(&mut self, from: usize) {
        let mut at = from.max(1);
        while at < self.queue.len() {
            if self.merge(at) {
                continue;
            }
            if at >= 3 && self.supersede_save(at) {
                at -= 2;
                continue;
            }
            at += 1;
        }
    }

    fn merge(&mut self, at: usize) -> bool {
        let (Some(before), Some(next)) = (self.queue.get(at - 1), self.queue.get(at)) else { return false };
        let document = is(&before.message, DID_CHANGE);
        if document.is_none() || document != is(&next.message, DID_CHANGE) {
            return false;
        }
        let Some(next) = self.remove(at) else { return false };
        let changes =
            next.message["params"]["contentChanges"].as_array().map_or(0, |changes| changes.iter().map(weight).sum());
        let last = &mut self.queue[at - 1];
        let old = last.weight;
        match coalesce(&mut last.message, next.message) {
            Ok(whole) => {
                last.weight = if whole { weight(&last.message) } else { old + changes };
                self.bytes = self.bytes - old + last.weight;
                true
            }
            Err(message) => {
                self.bytes += next.weight;
                self.queue.insert(at, Queued { message, weight: next.weight });
                false
            }
        }
    }

    fn supersede_save(&mut self, at: usize) -> bool {
        let Some(uri) = is(&self.queue[at].message, DID_SAVE) else { return false };
        let of = |offset: usize, method: &str| is(&self.queue[at - offset].message, method) == Some(uri);
        if !(of(3, DID_CHANGE) && of(2, DID_SAVE) && of(1, DID_CHANGE)) {
            return false;
        }
        self.remove(at - 2);
        self.merge(at - 2)
    }
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    on_stop: Mutex<Option<OnStop>>,
    limit: usize,
}

impl Shared {
    fn stop(&self, why: &str) {
        if let Some(on_stop) = self.on_stop.lock().ok().and_then(|mut slot| slot.take()) {
            on_stop(why);
        }
    }
}

pub struct Outbox {
    shared: Arc<Shared>,
}

impl Outbox {
    pub fn new(writer: Box<dyn Write + Send>, limit: usize, on_write: OnWrite, on_stop: OnStop) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            ready: Condvar::new(),
            on_stop: Mutex::new(Some(on_stop)),
            limit,
        });
        let thread_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("lsp-writer".to_owned())
            .spawn(move || write_all(&thread_shared, writer, &on_write))
            .expect("cannot start the language server writer thread");
        Self { shared }
    }

    pub fn push(&self, message: Value) -> Result<(), Refused> {
        let mut guard = self.shared.state.lock().map_err(|_| Refused::Stopped)?;
        let state = &mut *guard;
        if state.stopped || state.closing {
            return Err(Refused::Stopped);
        }
        state.add(message);
        if state.bytes > self.shared.limit {
            state.stopped = true;
            state.queue.clear();
            state.bytes = 0;
            drop(guard);
            self.shared.ready.notify_all();
            self.shared.stop("the server stopped reading its input");
            return Err(Refused::Full);
        }
        drop(guard);
        self.shared.ready.notify_one();
        Ok(())
    }

    pub fn withdraw(&self, id: i64) -> bool {
        self.shared.state.lock().is_ok_and(|mut state| state.withdraw(id))
    }

    pub fn queued(&self) -> usize {
        self.shared.state.lock().map(|state| state.queue.len()).unwrap_or(0)
    }
}

impl Drop for Outbox {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.closing = true;
        }
        self.shared.ready.notify_all();
    }
}

fn write_all(shared: &Shared, mut writer: Box<dyn Write + Send>, on_write: &OnWrite) {
    loop {
        let message = {
            let Ok(mut state) = shared.state.lock() else { return };
            loop {
                if state.stopped {
                    return;
                }
                if let Some(next) = state.queue.pop_front() {
                    state.bytes -= next.weight;
                    break next.message;
                }
                if state.closing {
                    return;
                }
                state = match shared.ready.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
        };
        on_write(&message);
        if let Err(error) = write_message(&mut *writer, &message) {
            if let Ok(mut state) = shared.state.lock() {
                state.stopped = true;
                state.queue.clear();
                state.bytes = 0;
            }
            shared.stop(&format!("cannot write to the server: {error}"));
            return;
        }
    }
}

pub fn weight(message: &Value) -> usize {
    match message {
        Value::String(text) => text.len() + 2,
        Value::Array(items) => 2 + items.iter().map(weight).sum::<usize>(),
        Value::Object(fields) => 2 + fields.iter().map(|(name, value)| name.len() + 4 + weight(value)).sum::<usize>(),
        _ => 8,
    }
}

pub fn coalesce(last: &mut Value, mut next: Value) -> Result<bool, Value> {
    let is_change = |message: &Value| message.get("method").and_then(Value::as_str) == Some(DID_CHANGE);
    let uri = |message: &Value| message["params"]["textDocument"]["uri"].as_str().map(str::to_owned);
    if !is_change(last)
        || !is_change(&next)
        || uri(last).is_none()
        || uri(last) != uri(&next)
        || !last["params"]["contentChanges"].is_array()
        || !next["params"]["contentChanges"].is_array()
    {
        return Err(next);
    }
    let added = match next["params"]["contentChanges"].take() {
        Value::Array(added) => added,
        _ => Vec::new(),
    };
    let whole = added.iter().any(|change| change.get("range").is_none());
    last["params"]["textDocument"] = next["params"]["textDocument"].take();
    let Some(changes) = last["params"]["contentChanges"].as_array_mut() else { return Ok(whole) };
    changes.extend(added);
    if let Some(at) = changes.iter().rposition(|change| change.get("range").is_none()) {
        changes.drain(..at);
    }
    Ok(whole)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::connection::read_message;

    fn change(uri: &str, version: i32, changes: Value) -> Value {
        json!({ "jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
            "textDocument": { "uri": uri, "version": version }, "contentChanges": changes } })
    }

    fn ranged(text: &str) -> Value {
        json!({ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }, "text": text })
    }

    #[test]
    fn changes_of_one_document_are_merged_in_order() {
        let mut merged = change("a", 1, json!([ranged("x")]));
        assert_eq!(coalesce(&mut merged, change("a", 2, json!([ranged("y")]))), Ok(false));
        assert_eq!(merged, change("a", 2, json!([ranged("x"), ranged("y")])));
        let next = change("a", 3, json!([ranged("z"), { "text": "whole" }, ranged("w")]));
        assert_eq!(coalesce(&mut merged, next), Ok(true));
        assert_eq!(merged, change("a", 3, json!([{ "text": "whole" }, ranged("w")])));
        let other = change("b", 1, json!([]));
        assert_eq!(coalesce(&mut change("a", 1, json!([])), other.clone()), Err(other));
        let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "textDocument/inlayHint", "params": {} });
        assert_eq!(coalesce(&mut change("a", 1, json!([])), request.clone()), Err(request.clone()));
        let mut queued = request.clone();
        assert!(coalesce(&mut queued, change("a", 1, json!([]))).is_err());
        assert_eq!(queued, request);
    }

    fn methods(state: &State) -> Vec<String> {
        let name = |message: &Value| {
            message["method"].as_str().unwrap_or("response").trim_start_matches("textDocument/").to_owned()
        };
        state.queue.iter().map(|queued| name(&queued.message)).collect()
    }

    #[test]
    fn a_save_followed_by_a_change_and_another_save_is_superseded() {
        let save =
            |uri: &str| json!({ "jsonrpc": "2.0", "method": DID_SAVE, "params": { "textDocument": { "uri": uri } } });
        let mut state = State::default();
        for version in 1..=50 {
            state.add(change("a", version * 2 - 1, json!([ranged("x")])));
            state.add(change("a", version * 2, json!([ranged("")])));
            state.add(save("a"));
        }
        assert_eq!(methods(&state), ["didChange", "didSave"]);
        assert_eq!(state.queue[0].message["params"]["contentChanges"].as_array().unwrap().len(), 100);
        assert_eq!(state.queue[0].message["params"]["textDocument"]["version"], 100);
        assert_eq!(state.bytes, state.queue.iter().map(|queued| weight(&queued.message)).sum::<usize>());
        let hints = |id: i64| json!({ "jsonrpc": "2.0", "id": id, "method": "textDocument/inlayHint", "params": {} });
        let mut asked = 1000;
        state.add(hints(asked));
        for version in 101..=150 {
            state.add(change("a", version, json!([ranged(if version % 2 == 1 { "x" } else { "" })])));
            if version % 2 == 0 {
                state.add(save("a"));
            }
            assert!(state.withdraw(asked));
            asked += 1;
            state.add(hints(asked));
        }
        assert_eq!(methods(&state), ["didChange", "didSave", "inlayHint"]);
        assert_eq!(state.queue[0].message["params"]["textDocument"]["version"], 150);
        state.withdraw(asked);
        state.add(change("b", 1, json!([ranged("x")])));
        state.add(save("a"));
        assert_eq!(methods(&state), ["didChange", "didSave", "didChange", "didSave"]);
    }

    #[test]
    fn a_withdrawn_request_lets_the_changes_around_it_merge() {
        let mut state = State::default();
        state.add(change("a", 1, json!([ranged("x")])));
        state.add(json!({ "jsonrpc": "2.0", "id": 5, "method": "textDocument/completion", "params": {} }));
        state.add(change("a", 2, json!([ranged("y")])));
        state.add(json!({ "jsonrpc": "2.0", "id": 6, "method": "textDocument/completion", "params": {} }));
        assert_eq!(methods(&state), ["didChange", "completion", "didChange", "completion"]);
        assert!(state.withdraw(5));
        assert!(!state.withdraw(5), "once");
        assert_eq!(methods(&state), ["didChange", "completion"]);
        assert_eq!(state.queue[0].message, change("a", 2, json!([ranged("x"), ranged("y")])));
        assert_eq!(state.queue[1].message["id"], 6);
        assert_eq!(state.bytes, state.queue.iter().map(|queued| weight(&queued.message)).sum::<usize>());
    }

    #[test]
    fn the_weight_follows_the_text() {
        let small = weight(&change("a", 1, json!([ranged("x")])));
        let large = weight(&change("a", 1, json!([ranged(&"x".repeat(10_000))])));
        assert!(small < 300, "{small}");
        assert!((10_000..10_300).contains(&large), "{large}");
    }

    fn slow_server(reader: std::io::PipeReader, pause: Duration) -> std::thread::JoinHandle<Vec<Value>> {
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut seen = Vec::new();
            while let Ok(Some(message)) = read_message(&mut reader as &mut dyn BufRead) {
                seen.push(message);
                std::thread::sleep(pause);
            }
            seen
        })
    }

    #[test]
    fn a_slow_server_does_not_slow_the_sender_and_order_is_kept() {
        let (reader, writer) = std::io::pipe().unwrap();
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            wait.recv().unwrap();
            slow_server(reader, Duration::from_millis(2)).join().unwrap()
        });
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stopped);
        let outbox = Outbox::new(
            Box::new(writer),
            QUEUE_LIMIT,
            Box::new(|_| {}),
            Box::new(move |_| flag.store(true, Ordering::SeqCst)),
        );
        let filler = "x".repeat(16 * 1024);
        for index in 0..200 {
            outbox
                .push(json!({ "jsonrpc": "2.0", "id": index, "method": "test/echo", "params": { "filler": filler } }))
                .unwrap();
        }
        assert!(outbox.queued() > 0, "the server has read nothing yet");
        go.send(()).unwrap();
        drop(outbox);
        let seen = server.join().unwrap();
        let ids: Vec<i64> = seen.iter().map(|message| message["id"].as_i64().unwrap()).collect();
        assert_eq!(ids, (0..200).collect::<Vec<_>>());
        assert!(!stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn changes_wait_merged_while_the_server_is_busy() {
        let (reader, writer) = std::io::pipe().unwrap();
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            wait.recv().unwrap();
            slow_server(reader, Duration::ZERO).join().unwrap()
        });
        let outbox = Outbox::new(Box::new(writer), QUEUE_LIMIT, Box::new(|_| {}), Box::new(|_| {}));
        let large = json!({ "jsonrpc": "2.0", "method": "test/large", "params": "x".repeat(1024 * 1024) });
        outbox.push(large).unwrap();
        while outbox.queued() > 0 {
            std::thread::yield_now();
        }
        for version in 1..=100 {
            outbox.push(change("file:///a.go", version, json!([ranged(&version.to_string())]))).unwrap();
        }
        assert_eq!(outbox.queued(), 1, "the changes wait as one");
        let request = json!({ "jsonrpc": "2.0", "id": 7, "method": "textDocument/inlayHint", "params": {} });
        outbox.push(request.clone()).unwrap();
        drop(outbox);
        go.send(()).unwrap();
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 3, "the large message, the changes as one, the request");
        assert_eq!(seen[0]["method"], "test/large");
        assert_eq!(seen.last(), Some(&request), "the request stays behind the changes");
        let changes = &seen[1..seen.len() - 1];
        assert_eq!(changes.last().unwrap()["params"]["textDocument"]["version"], 100);
        let texts: Vec<String> = changes
            .iter()
            .flat_map(|message| message["params"]["contentChanges"].as_array().unwrap().clone())
            .map(|change| change["text"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(texts, (1..=100).map(|version| version.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn a_server_that_stops_reading_is_stopped_at_the_limit() {
        let (reader, writer) = std::io::pipe().unwrap();
        let why = Arc::new(Mutex::new(None::<String>));
        let told = Arc::clone(&why);
        let outbox = Outbox::new(
            Box::new(writer),
            1024 * 1024,
            Box::new(|_| {}),
            Box::new(move |reason| *told.lock().unwrap() = Some(reason.to_owned())),
        );
        let filler = "x".repeat(64 * 1024);
        let mut refused = None;
        for index in 0..100 {
            let result = outbox.push(json!({ "jsonrpc": "2.0", "id": index, "method": "test/echo", "params": filler }));
            if let Err(error) = result {
                refused = Some((index, error));
                break;
            }
        }
        let (index, error) = refused.expect("the queue is bounded");
        assert_eq!(error, Refused::Full);
        assert!((12..40).contains(&index), "refused at {index}");
        assert_eq!(why.lock().unwrap().as_deref(), Some("the server stopped reading its input"));
        assert_eq!(outbox.push(json!({})), Err(Refused::Stopped));
        assert_eq!(outbox.queued(), 0, "the queue is dropped");
        drop(reader);
    }

    #[test]
    fn a_closed_input_stops_the_writer() {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let (sender, receiver) = std::sync::mpsc::channel();
        let outbox = Outbox::new(
            Box::new(writer),
            QUEUE_LIMIT,
            Box::new(|_| {}),
            Box::new(move |why| sender.send(why.to_owned()).unwrap()),
        );
        outbox.push(json!({ "jsonrpc": "2.0", "method": "exit" })).unwrap();
        let why = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(why.starts_with("cannot write to the server"), "{why}");
        assert_eq!(outbox.push(json!({})), Err(Refused::Stopped));
    }
}
