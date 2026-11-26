use std::collections::HashMap;
use std::io::{BufReader, PipeWriter};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::connection::{REQUEST_CANCELLED, read_message, write_message};
use crate::{Connection, Launched, Launcher};

pub type Handler = Arc<dyn Fn(&Value) -> Value + Send + Sync>;

#[derive(Clone)]
pub enum Reply {
    Result(Value),
    Error { code: i64, message: String },
    ErrorOnce { code: i64, then: Value },
    With(Handler),
    Delayed(Value, Duration),
    Hold,
    Ask { method: String, params: Handler, then: Value },
    AskAll { asks: Vec<(String, Handler)>, then: Value },
}

struct Asking {
    id: Value,
    params: Value,
    rest: std::collections::VecDeque<(String, Handler)>,
    then: Value,
}

type Writer = Arc<Mutex<PipeWriter>>;

#[derive(Default)]
struct Log {
    messages: Mutex<Vec<Value>>,
    grew: Condvar,
}

impl Log {
    fn push(&self, message: Value) {
        if let Ok(mut messages) = self.messages.lock() {
            messages.push(message);
        }
        self.grew.notify_all();
    }

    fn all(&self) -> Vec<Value> {
        self.messages.lock().map(|messages| messages.clone()).unwrap_or_default()
    }

    fn wait(&self, count: usize, timeout: Duration, wanted: impl Fn(&Value) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + timeout;
        let Ok(mut messages) = self.messages.lock() else { return Vec::new() };
        loop {
            let found: Vec<Value> = messages.iter().filter(|message| wanted(message)).cloned().collect();
            let left = deadline.saturating_duration_since(Instant::now());
            if found.len() >= count || left.is_zero() {
                return found;
            }
            match self.grew.wait_timeout(messages, left) {
                Ok((guard, _)) => messages = guard,
                Err(_) => return found,
            }
        }
    }
}

fn params_of(messages: Vec<Value>) -> Vec<Value> {
    messages.into_iter().map(|message| message.get("params").cloned().unwrap_or(Value::Null)).collect()
}

#[derive(Clone)]
pub struct FakeServer {
    capabilities: Arc<Mutex<Value>>,
    name: Arc<Mutex<Option<String>>>,
    replies: Arc<Mutex<HashMap<String, Reply>>>,
    publishes: Arc<Mutex<Vec<(String, String, Handler)>>>,
    log: Arc<Log>,
    launches: Arc<AtomicUsize>,
}

impl Default for FakeServer {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeServer {
    pub fn new() -> Self {
        let capabilities = json!({
            "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
            "completionProvider": { "triggerCharacters": ["."] },
            "hoverProvider": true,
            "signatureHelpProvider": { "triggerCharacters": ["(", ","] },
            "declarationProvider": true,
            "definitionProvider": true,
            "typeDefinitionProvider": true,
            "implementationProvider": true,
            "referencesProvider": true,
            "documentHighlightProvider": true
        });
        Self {
            capabilities: Arc::new(Mutex::new(capabilities)),
            name: Arc::default(),
            replies: Arc::default(),
            publishes: Arc::default(),
            log: Arc::default(),
            launches: Arc::default(),
        }
    }

    pub fn set_capabilities(&self, capabilities: Value) {
        if let Ok(mut slot) = self.capabilities.lock() {
            *slot = capabilities;
        }
    }

    pub fn set_name(&self, name: &str) {
        if let Ok(mut slot) = self.name.lock() {
            *slot = Some(name.to_owned());
        }
    }

    pub fn reply(&self, method: &str, reply: Reply) {
        if let Ok(mut replies) = self.replies.lock() {
            replies.insert(method.to_owned(), reply);
        }
    }

    pub fn on(&self, method: &str, result: Value) {
        self.reply(method, Reply::Result(result));
    }

    pub fn on_with(&self, method: &str, handler: impl Fn(&Value) -> Value + Send + Sync + 'static) {
        self.reply(method, Reply::With(Arc::new(handler)));
    }

    pub fn publish_on(
        &self,
        method: &str,
        notification: &str,
        params: impl Fn(&Value) -> Value + Send + Sync + 'static,
    ) {
        if let Ok(mut publishes) = self.publishes.lock() {
            publishes.push((method.to_owned(), notification.to_owned(), Arc::new(params)));
        }
    }

    pub fn launch(&self) -> std::io::Result<Launched> {
        let (from_client, to_server) = std::io::pipe()?;
        let (from_server, to_client) = std::io::pipe()?;
        self.launches.fetch_add(1, Ordering::SeqCst);
        let server = self.clone();
        let writer: Writer = Arc::new(Mutex::new(to_client));
        std::thread::spawn(move || server.serve(BufReader::new(from_client), writer));
        Ok(Launched { connection: Connection::new(from_server, to_server), process: None })
    }

    pub fn launcher(&self) -> Launcher {
        let server = self.clone();
        Box::new(move || server.launch())
    }

    pub fn launches(&self) -> usize {
        self.launches.load(Ordering::SeqCst)
    }

    pub fn received(&self) -> Vec<Value> {
        self.log.all()
    }

    pub fn requests(&self, method: &str) -> Vec<Value> {
        params_of(self.received().into_iter().filter(|message| message["method"] == method).collect())
    }

    pub fn responses(&self) -> Vec<Value> {
        self.received().into_iter().filter(|message| message.get("method").is_none()).collect()
    }

    pub fn wait_for_responses(&self, count: usize, timeout: Duration) -> Vec<Value> {
        self.log.wait(count, timeout, |message| message.get("method").is_none())
    }

    pub fn wait_for(&self, method: &str, count: usize, timeout: Duration) -> Vec<Value> {
        params_of(self.log.wait(count, timeout, |message| message["method"] == method))
    }

    fn serve(&self, mut reader: BufReader<std::io::PipeReader>, writer: Writer) {
        let mut held: Vec<Value> = Vec::new();
        let mut asked: HashMap<String, Asking> = HashMap::new();
        let mut own_ids = 0_u64;
        let mut ask = |mut asking: Asking, asked: &mut HashMap<String, Asking>| match asking.rest.pop_front() {
            Some((method, make)) => {
                own_ids += 1;
                let own = format!("fake-{own_ids}");
                let request = json!({ "jsonrpc": "2.0", "id": own, "method": method, "params": make(&asking.params) });
                asked.insert(own, asking);
                send(&writer, &request);
            }
            None => send(&writer, &json!({ "jsonrpc": "2.0", "id": asking.id, "result": asking.then })),
        };
        while let Ok(Some(message)) = read_message(&mut reader) {
            self.log.push(message.clone());
            if message.get("method").is_none() {
                if let Some(asking) = message["id"].as_str().and_then(|id| asked.remove(id)) {
                    ask(asking, &mut asked);
                }
                continue;
            }
            let method = message["method"].as_str().unwrap_or_default().to_owned();
            let Some(id) = message.get("id").cloned() else {
                match method.as_str() {
                    "exit" => return,
                    "$/cancelRequest" => {
                        let cancelled = message["params"]["id"].clone();
                        if let Some(index) = held.iter().position(|id| *id == cancelled) {
                            held.remove(index);
                            let error = json!({ "code": REQUEST_CANCELLED, "message": "cancelled" });
                            send(&writer, &json!({ "jsonrpc": "2.0", "id": cancelled, "error": error }));
                        }
                    }
                    _ => {
                        let publishes = self.publishes.lock().map(|list| list.clone()).unwrap_or_default();
                        let params = message.get("params").cloned().unwrap_or(Value::Null);
                        for (_, notification, make) in publishes.iter().filter(|(on, _, _)| *on == method) {
                            let sent = json!({ "jsonrpc": "2.0", "method": notification, "params": make(&params) });
                            send(&writer, &sent);
                        }
                    }
                }
                continue;
            };
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let reply = match method.as_str() {
                "initialize" => {
                    let capabilities = self.capabilities.lock().map(|value| value.clone()).unwrap_or_default();
                    let name = self.name.lock().ok().and_then(|name| name.clone());
                    Reply::Result(match name {
                        Some(name) => json!({ "capabilities": capabilities, "serverInfo": { "name": name } }),
                        None => json!({ "capabilities": capabilities }),
                    })
                }
                "shutdown" => Reply::Result(Value::Null),
                _ => self
                    .replies
                    .lock()
                    .ok()
                    .and_then(|replies| replies.get(&method).cloned())
                    .unwrap_or(Reply::Result(Value::Null)),
            };
            match reply {
                Reply::Result(result) => send(&writer, &json!({ "jsonrpc": "2.0", "id": id, "result": result })),
                Reply::With(handler) => {
                    send(&writer, &json!({ "jsonrpc": "2.0", "id": id, "result": handler(&params) }));
                }
                Reply::Error { code, message } => {
                    send(&writer, &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }))
                }
                Reply::ErrorOnce { code, then } => {
                    self.reply(&method, Reply::Result(then));
                    send(&writer, &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": "once" } }))
                }
                Reply::Delayed(result, delay) => {
                    let writer = Arc::clone(&writer);
                    std::thread::spawn(move || {
                        std::thread::sleep(delay);
                        send(&writer, &json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                    });
                }
                Reply::Hold => held.push(id),
                Reply::Ask { method, params: make, then } => {
                    let rest = std::iter::once((method, make)).collect();
                    ask(Asking { id, params, rest, then }, &mut asked);
                }
                Reply::AskAll { asks, then } => ask(Asking { id, params, rest: asks.into(), then }, &mut asked),
            }
        }
    }
}

fn send(writer: &Writer, message: &Value) {
    if let Ok(mut writer) = writer.lock() {
        let _ = write_message(&mut *writer, message);
    }
}
