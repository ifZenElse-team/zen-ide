use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

const ID: &str = "zen-reference";
const PROTOCOL: u64 = 1;

const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_REQUEST: i64 = -32600;
const INVALID_PARAMS: i64 = -32602;
const PARSE_ERROR: i64 = -32700;
const DATABASE_ERROR: i64 = -32000;
const UNSUPPORTED_PROTOCOL: i64 = -32010;
const UNKNOWN_SESSION: i64 = -32011;
const UNKNOWN_CURSOR: i64 = -32012;

#[derive(Clone, Debug, PartialEq, PartialOrd)]
enum V {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
}

impl V {
    fn wire(&self) -> Json {
        match self {
            V::Null => json!({ "kind": "null" }),
            V::Bool(value) => json!({ "kind": "bool", "value": value }),
            V::Int(value) => json!({ "kind": "integer", "value": value }),
            V::Real(value) => json!({ "kind": "float", "value": value }),
            V::Text(value) => json!({ "kind": "text", "value": value }),
        }
    }

    fn from_param(param: &Json) -> V {
        match param {
            Json::String(text) => match text.parse::<i64>() {
                Ok(number) => V::Int(number),
                Err(_) => V::Text(text.clone()),
            },
            _ => V::Null,
        }
    }

    fn typed(self, type_name: &str) -> V {
        match (self, type_name) {
            (V::Text(text), "integer") => text.parse().map(V::Int).unwrap_or(V::Text(text)),
            (V::Int(number), "text") => V::Text(number.to_string()),
            (V::Int(number), "real") => V::Real(number as f64),
            (V::Int(number), "boolean") => V::Bool(number != 0),
            (value, _) => value,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Table {
    columns: Vec<(String, String)>,
    primary: Option<usize>,
    rows: Vec<Vec<V>>,
}

type Store = BTreeMap<String, Table>;

struct Failure {
    code: i64,
    message: String,
    data: Option<Json>,
}

impl Failure {
    fn database(kind: &str, message: impl Into<String>) -> Self {
        Failure { code: DATABASE_ERROR, message: message.into(), data: Some(json!({ "kind": kind })) }
    }

    fn syntax(message: impl Into<String>, position: usize) -> Self {
        Failure {
            code: DATABASE_ERROR,
            message: message.into(),
            data: Some(json!({ "kind": "server", "code": "42601", "position": position })),
        }
    }

    fn params(message: impl Into<String>) -> Self {
        Failure { code: INVALID_PARAMS, message: message.into(), data: None }
    }
}

type Answer = Result<Json, Failure>;

struct Out(Mutex<std::io::Stdout>);

impl Out {
    fn send(&self, message: &Json) {
        let mut line = serde_json::to_vec(message).unwrap_or_default();
        line.push(b'\n');
        let mut out = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = out.write_all(&line);
        let _ = out.flush();
    }

    fn answer(&self, id: &Json, answer: Answer) {
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
}

struct Cursor {
    rows: VecDeque<Vec<V>>,
    affected: Option<u64>,
}

struct SessionData {
    database: String,
    private: Option<Store>,
    auto_commit: bool,
    read_only: bool,
    cursors: HashMap<String, Cursor>,
    next_cursor: u64,
}

struct Handle {
    cancel: Arc<AtomicBool>,
    work: mpsc::Sender<(Json, String, Json)>,
}

struct Server {
    stores: Mutex<HashMap<String, Store>>,
    sessions: Mutex<HashMap<String, Handle>>,
    next_session: AtomicU64,
    password: Option<String>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn capabilities() -> Json {
    json!({
        "models": ["relational"],
        "language": "sql:sqlite",
        "transactions": true,
        "isolationLevels": ["serializable"],
        "catalogs": false,
        "schemas": false,
        "nodeKinds": ["table", "column"],
        "explain": false,
        "serverCancel": true,
        "bindParameters": true,
        "rowIdentity": "primaryKey",
        "ddlPreview": false,
        "serverPaging": true,
        "sessions": false,
        "readOnlySessions": true,
        "uriSchemes": [],
        "sharedConnection": false,
        "password": false,
        "settings": [{ "key": "seed", "kind": "bool", "default": "false" }],
        "introspection": true
    })
}

fn main() {
    let server = Arc::new(Server {
        stores: Mutex::new(HashMap::new()),
        sessions: Mutex::new(HashMap::new()),
        next_session: AtomicU64::new(1),
        password: std::env::var("ZEN_REFERENCE_PASSWORD").ok(),
    });
    let out = Arc::new(Out(Mutex::new(std::io::stdout())));
    let mut initialized = false;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let message: Json = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(error) => {
                out.answer(&Json::Null, Err(Failure { code: PARSE_ERROR, message: error.to_string(), data: None }));
                continue;
            }
        };
        let id = message.get("id").cloned().unwrap_or(Json::Null);
        let Some(method) = message.get("method").and_then(Json::as_str).map(str::to_owned) else {
            out.answer(&id, Err(Failure { code: INVALID_REQUEST, message: "no method".into(), data: None }));
            continue;
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        if method != "initialize" && !initialized {
            out.answer(&id, Err(Failure { code: INVALID_REQUEST, message: "initialize first".into(), data: None }));
            continue;
        }
        match method.as_str() {
            "initialize" => {
                let asked = params.get("protocol").and_then(Json::as_u64);
                if asked != Some(PROTOCOL) {
                    out.answer(
                        &id,
                        Err(Failure {
                            code: UNSUPPORTED_PROTOCOL,
                            message: format!("protocol {asked:?} is not supported"),
                            data: Some(json!({ "supported": [PROTOCOL] })),
                        }),
                    );
                    continue;
                }
                initialized = true;
                out.answer(
                    &id,
                    Ok(json!({
                        "protocol": PROTOCOL,
                        "driver": { "id": ID, "name": "Zen Reference", "version": env!("CARGO_PKG_VERSION") },
                        "capabilities": capabilities(),
                    })),
                );
            }
            "shutdown" => {
                lock(&server.sessions).clear();
                out.answer(&id, Ok(json!({})));
                break;
            }
            "connect" => {
                let answer = connect(&server, &out, &params);
                out.answer(&id, answer);
            }
            "close" => {
                let session = params.get("session").and_then(Json::as_str).unwrap_or_default();
                lock(&server.sessions).remove(session);
                if !id.is_null() {
                    out.answer(&id, Ok(json!({})));
                }
            }
            "cancel" => {
                let session = params.get("session").and_then(Json::as_str).unwrap_or_default();
                let answer = match lock(&server.sessions).get(session) {
                    Some(handle) => {
                        handle.cancel.store(true, Ordering::SeqCst);
                        Ok(json!({}))
                    }
                    None => {
                        Err(Failure { code: UNKNOWN_SESSION, message: format!("no session `{session}`"), data: None })
                    }
                };
                out.answer(&id, answer);
            }
            "ping" | "execute" | "query" | "fetch" | "closeCursor" | "apply" | "commit" | "rollback" | "children"
            | "introspect" => {
                let session = params.get("session").and_then(Json::as_str).unwrap_or_default().to_owned();
                let sent = lock(&server.sessions)
                    .get(&session)
                    .map(|handle| handle.work.send((id.clone(), method.clone(), params.clone())).is_ok());
                if sent != Some(true) {
                    out.answer(
                        &id,
                        Err(Failure { code: UNKNOWN_SESSION, message: format!("no session `{session}`"), data: None }),
                    );
                }
            }
            other => out.answer(
                &id,
                Err(Failure { code: METHOD_NOT_FOUND, message: format!("no method `{other}`"), data: None }),
            ),
        }
    }
}

fn connect(server: &Arc<Server>, out: &Arc<Out>, params: &Json) -> Answer {
    if let Some(expected) = &server.password {
        let given = params.get("password").and_then(Json::as_str).unwrap_or_default();
        if given != expected {
            return Err(Failure::database("authentication", "the password is wrong"));
        }
    }
    let database = params.get("database").and_then(Json::as_str).unwrap_or("main").to_owned();
    let state = params.get("state").cloned().unwrap_or_default();
    let seed = params.pointer("/settings/seed").and_then(Json::as_bool).unwrap_or(false);
    {
        let mut stores = lock(&server.stores);
        let store = stores.entry(database.clone()).or_default();
        if seed && !store.contains_key("people") {
            store.insert(
                "people".into(),
                Table {
                    columns: vec![("id".into(), "integer".into()), ("name".into(), "text".into())],
                    primary: Some(0),
                    rows: vec![vec![V::Int(1), V::Text("Ada".into())], vec![V::Int(2), V::Text("Grace".into())]],
                },
            );
        }
    }
    let id = format!("s{}", server.next_session.fetch_add(1, Ordering::SeqCst));
    let cancel = Arc::new(AtomicBool::new(false));
    let (work, receiver) = mpsc::channel::<(Json, String, Json)>();
    let mut data = SessionData {
        database,
        private: None,
        auto_commit: state.get("autoCommit").and_then(Json::as_bool).unwrap_or(true),
        read_only: params.get("readOnly").and_then(Json::as_bool).unwrap_or(false)
            || state.get("readOnly").and_then(Json::as_bool).unwrap_or(false),
        cursors: HashMap::new(),
        next_cursor: 1,
    };
    let worker_server = Arc::clone(server);
    let worker_out = Arc::clone(out);
    let worker_cancel = Arc::clone(&cancel);
    std::thread::spawn(move || {
        for (id, method, params) in receiver {
            worker_cancel.store(false, Ordering::SeqCst);
            let answer = serve(&worker_server, &mut data, &worker_cancel, &method, &params);
            worker_out.answer(&id, answer);
        }
    });
    lock(&server.sessions).insert(id.clone(), Handle { cancel, work });
    Ok(
        json!({ "session": id, "serverVersion": format!("Zen Reference {}", env!("CARGO_PKG_VERSION")), "inTransaction": false }),
    )
}

fn with_store<T>(server: &Server, data: &mut SessionData, work: impl FnOnce(&mut Store) -> T) -> T {
    if let Some(private) = data.private.as_mut() {
        return work(private);
    }
    let mut stores = lock(&server.stores);
    work(stores.entry(data.database.clone()).or_default())
}

fn state(data: &SessionData) -> Json {
    json!({ "inTransaction": data.private.is_some() })
}

fn serve(server: &Server, data: &mut SessionData, cancel: &AtomicBool, method: &str, params: &Json) -> Answer {
    match method {
        "ping" => Ok(json!({})),
        "execute" => {
            let statement =
                params.get("statement").and_then(Json::as_str).ok_or_else(|| Failure::params("no statement"))?;
            let max_rows = params.get("maxRows").and_then(Json::as_u64).unwrap_or(200) as usize;
            let bound: Vec<V> = params
                .get("params")
                .and_then(Json::as_array)
                .map(|list| list.iter().map(V::from_param).collect())
                .unwrap_or_default();
            let ran = run(server, data, cancel, statement, bound)?;
            let truncated = ran.rows.len() > max_rows;
            let rows: Vec<Json> = ran.rows.iter().take(max_rows).map(|row| row_wire(row)).collect();
            let mut answer = state(data);
            answer["columns"] = ran.columns;
            answer["rows"] = json!(rows);
            answer["truncated"] = json!(truncated);
            if let Some(affected) = ran.affected {
                answer["affected"] = json!(affected);
            }
            Ok(answer)
        }
        "query" => {
            let statement =
                params.get("statement").and_then(Json::as_str).ok_or_else(|| Failure::params("no statement"))?;
            let skip = params.get("skip").and_then(Json::as_u64).unwrap_or(0) as usize;
            let ran = run(server, data, cancel, statement, Vec::new())?;
            let mut answer = state(data);
            answer["columns"] = ran.columns.clone();
            if ran.columns.as_array().is_some_and(|columns| !columns.is_empty()) {
                let cursor = format!("c{}", data.next_cursor);
                data.next_cursor += 1;
                data.cursors.insert(
                    cursor.clone(),
                    Cursor { rows: ran.rows.into_iter().skip(skip).collect(), affected: ran.affected },
                );
                answer["cursor"] = json!(cursor);
            } else if let Some(affected) = ran.affected {
                answer["affected"] = json!(affected);
            }
            Ok(answer)
        }
        "fetch" => {
            let name = params.get("cursor").and_then(Json::as_str).unwrap_or_default().to_owned();
            let count = params.get("count").and_then(Json::as_u64).unwrap_or(200).max(1) as usize;
            let cursor = data.cursors.get_mut(&name).ok_or(Failure {
                code: UNKNOWN_CURSOR,
                message: format!("no cursor `{name}`"),
                data: None,
            })?;
            let take = count.min(cursor.rows.len());
            let rows: Vec<Json> = cursor.rows.drain(..take).map(|row| row_wire(&row)).collect();
            let done = cursor.rows.is_empty();
            let affected = cursor.affected;
            if done {
                data.cursors.remove(&name);
            }
            let mut answer = state(data);
            answer["rows"] = json!(rows);
            answer["done"] = json!(done);
            if done && let Some(affected) = affected {
                answer["affected"] = json!(affected);
            }
            Ok(answer)
        }
        "closeCursor" => {
            let name = params.get("cursor").and_then(Json::as_str).unwrap_or_default();
            data.cursors.remove(name);
            Ok(json!({}))
        }
        "apply" => {
            let state_asked = params.get("state").cloned().unwrap_or_default();
            if let Some(read_only) = state_asked.get("readOnly").and_then(Json::as_bool) {
                data.read_only = read_only;
            }
            if let Some(auto_commit) = state_asked.get("autoCommit").and_then(Json::as_bool) {
                data.auto_commit = auto_commit;
            }
            Ok(state(data))
        }
        "commit" => {
            if let Some(private) = data.private.take() {
                lock(&server.stores).insert(data.database.clone(), private);
            }
            Ok(state(data))
        }
        "rollback" => {
            data.private = None;
            Ok(state(data))
        }
        "children" => {
            let path: Vec<String> = params
                .get("path")
                .and_then(Json::as_array)
                .map(|list| list.iter().filter_map(Json::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            let nodes = with_store(server, data, |store| children(store, &path));
            Ok(json!({ "nodes": nodes }))
        }
        "introspect" => {
            let database = data.database.clone();
            let known =
                params.pointer(&format!("/fingerprints/{database}.main")).and_then(Json::as_str).map(str::to_owned);
            let force = params.get("force").and_then(Json::as_bool).unwrap_or(false);
            let (objects, fingerprint) = with_store(server, data, |store| (objects(store), fingerprint(store)));
            if !force && known.as_deref() == Some(fingerprint.as_str()) {
                return Ok(json!({
                    "catalogs": [{ "name": database, "current": true, "schemasRead": true, "schemas": [] }],
                    "read": [],
                    "unchanged": [format!("{database}.main")],
                }));
            }
            Ok(json!({
                "catalogs": [{
                    "name": database, "current": true, "schemasRead": true,
                    "schemas": [{ "name": "main", "fingerprint": fingerprint, "level": "details", "objects": objects }],
                }],
                "read": [format!("{database}.main")],
                "unchanged": [],
            }))
        }
        _ => Err(Failure { code: METHOD_NOT_FOUND, message: format!("no method `{method}`"), data: None }),
    }
}

fn row_wire(row: &[V]) -> Json {
    Json::Array(row.iter().map(V::wire).collect())
}

fn children(store: &Store, path: &[String]) -> Vec<Json> {
    match path {
        [] => store
            .iter()
            .map(|(name, table)| {
                json!({ "path": [name], "kind": "table", "label": name, "hasChildren": true, "rows": table.rows.len() })
            })
            .collect(),
        [table] => store
            .get(table)
            .map(|found| {
                found
                    .columns
                    .iter()
                    .map(|(column, type_name)| {
                        json!({ "path": [table, column], "kind": "column", "label": column, "hasChildren": false, "detail": type_name })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn objects(store: &Store) -> Vec<Json> {
    store
        .iter()
        .map(|(name, table)| {
            let columns: Vec<Json> = table
                .columns
                .iter()
                .enumerate()
                .map(|(at, (column, type_name))| {
                    json!({ "name": column, "position": at + 1, "typeName": type_name, "nullable": table.primary != Some(at) })
                })
                .collect();
            let constraints: Vec<Json> = table
                .primary
                .map(|at| {
                    vec![json!({
                        "name": format!("{name}_pkey"), "kind": "primaryKey", "columns": [table.columns[at].0],
                        "definition": format!("PRIMARY KEY ({})", table.columns[at].0),
                    })]
                })
                .unwrap_or_default();
            json!({
                "kind": "table", "name": name, "oid": oid(name), "detailed": true, "rows": table.rows.len(),
                "columns": columns, "constraints": constraints,
            })
        })
        .collect()
}

fn fingerprint(store: &Store) -> String {
    let mut text = String::new();
    for (name, table) in store {
        text.push_str(name);
        for (column, type_name) in &table.columns {
            text.push_str(column);
            text.push_str(type_name);
        }
        text.push(';');
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Quoted(String),
    Text(String),
    Number(String),
    Symbol(char),
    Placeholder,
}

fn tokens(statement: &str) -> Result<Vec<(Token, usize)>, Failure> {
    let chars: Vec<char> = statement.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let start = at;
        if c.is_whitespace() || c == ';' {
            at += 1;
        } else if c == '-' && chars.get(at + 1) == Some(&'-') {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            while at < chars.len() && (chars[at].is_ascii_alphanumeric() || chars[at] == '_') {
                at += 1;
            }
            found.push((Token::Word(chars[start..at].iter().collect::<String>().to_ascii_lowercase()), start + 1));
        } else if c.is_ascii_digit() || (c == '-' && chars.get(at + 1).is_some_and(char::is_ascii_digit)) {
            at += 1;
            while at < chars.len() && (chars[at].is_ascii_digit() || chars[at] == '.') {
                at += 1;
            }
            found.push((Token::Number(chars[start..at].iter().collect()), start + 1));
        } else if c == '\'' || c == '"' {
            let mut text = String::new();
            at += 1;
            loop {
                match chars.get(at) {
                    None => return Err(Failure::syntax("unterminated quote", start + 1)),
                    Some(&q) if q == c && chars.get(at + 1) == Some(&c) => {
                        text.push(c);
                        at += 2;
                    }
                    Some(&q) if q == c => {
                        at += 1;
                        break;
                    }
                    Some(&other) => {
                        text.push(other);
                        at += 1;
                    }
                }
            }
            found.push((if c == '\'' { Token::Text(text) } else { Token::Quoted(text) }, start + 1));
        } else if c == '?' {
            at += 1;
            found.push((Token::Placeholder, start + 1));
        } else {
            at += 1;
            found.push((Token::Symbol(c), start + 1));
        }
    }
    Ok(found)
}

struct Parser {
    tokens: Vec<(Token, usize)>,
    at: usize,
    params: VecDeque<V>,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|(token, _)| token)
    }

    fn position(&self) -> usize {
        self.tokens.get(self.at).map_or(self.end, |(_, position)| *position)
    }

    fn fail<T>(&self, message: &str) -> Result<T, Failure> {
        Err(Failure::syntax(format!("syntax error: {message}"), self.position()))
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).map(|(token, _)| token.clone());
        self.at += 1;
        token
    }

    fn word(&mut self, word: &str) -> bool {
        if self.peek() == Some(&Token::Word(word.to_owned())) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect_word(&mut self, word: &str) -> Result<(), Failure> {
        if self.word(word) { Ok(()) } else { self.fail(&format!("expected {}", word.to_ascii_uppercase())) }
    }

    fn symbol(&mut self, symbol: char) -> bool {
        if self.peek() == Some(&Token::Symbol(symbol)) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect_symbol(&mut self, symbol: char) -> Result<(), Failure> {
        if self.symbol(symbol) { Ok(()) } else { self.fail(&format!("expected `{symbol}`")) }
    }

    fn name(&mut self) -> Result<String, Failure> {
        let mut name = match self.next() {
            Some(Token::Word(word)) | Some(Token::Quoted(word)) => word,
            _ => {
                self.at = self.at.saturating_sub(1);
                return self.fail("expected a name");
            }
        };
        while self.symbol('.') {
            name = match self.next() {
                Some(Token::Word(word)) | Some(Token::Quoted(word)) => word,
                _ => return self.fail("expected a name"),
            };
        }
        Ok(name)
    }

    fn value(&mut self) -> Result<V, Failure> {
        match self.next() {
            Some(Token::Number(text)) if text.contains('.') => Ok(V::Real(text.parse().unwrap_or_default())),
            Some(Token::Number(text)) => Ok(V::Int(text.parse().unwrap_or_default())),
            Some(Token::Text(text)) => Ok(V::Text(text)),
            Some(Token::Word(word)) if word == "null" => Ok(V::Null),
            Some(Token::Word(word)) if word == "true" => Ok(V::Bool(true)),
            Some(Token::Word(word)) if word == "false" => Ok(V::Bool(false)),
            Some(Token::Placeholder) => Ok(self.params.pop_front().unwrap_or(V::Null)),
            _ => {
                self.at = self.at.saturating_sub(1);
                self.fail("expected a value")
            }
        }
    }

    fn done(&self) -> Result<(), Failure> {
        if self.at >= self.tokens.len() { Ok(()) } else { self.fail("unexpected text") }
    }
}

struct Ran {
    columns: Json,
    rows: Vec<Vec<V>>,
    affected: Option<u64>,
}

impl Ran {
    fn changed(count: usize) -> Self {
        Ran { columns: json!([]), rows: Vec::new(), affected: Some(count as u64) }
    }
}

type Condition = Vec<(usize, Option<V>)>;

fn condition(parser: &mut Parser, table: &Table) -> Result<Condition, Failure> {
    let mut found = Vec::new();
    if !parser.word("where") {
        return Ok(found);
    }
    loop {
        let name = parser.name()?;
        let Some(at) = table.columns.iter().position(|(column, _)| *column == name) else {
            return Err(Failure::database("server", format!("no column `{name}`")));
        };
        if parser.word("is") {
            parser.expect_word("null")?;
            found.push((at, None));
        } else {
            parser.expect_symbol('=')?;
            let value = parser.value()?.typed(&table.columns[at].1);
            found.push((at, Some(value)));
        }
        if !parser.word("and") {
            return Ok(found);
        }
    }
}

fn matches(row: &[V], condition: &Condition) -> bool {
    condition.iter().all(|(at, value)| match value {
        None => row[*at] == V::Null,
        Some(value) => row[*at] == *value,
    })
}

fn column_wire(table: &str, name: &str, type_name: &str) -> Json {
    if table.is_empty() {
        json!({ "name": name, "type": type_name })
    } else {
        json!({ "name": name, "type": type_name, "origin": name, "source": { "table": oid(table), "position": 0 } })
    }
}

fn oid(table: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in table.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash.max(1)
}

fn run(
    server: &Server,
    data: &mut SessionData,
    cancel: &AtomicBool,
    statement: &str,
    params: Vec<V>,
) -> Result<Ran, Failure> {
    let trimmed = statement.trim();
    let first = trimmed.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    let rest = trimmed.get(first.len()..).unwrap_or_default().trim();
    match first.as_str() {
        "sleep" => {
            let millis: u64 = rest.parse().map_err(|_| Failure::syntax("SLEEP takes milliseconds", 7))?;
            let until = Instant::now() + Duration::from_millis(millis);
            while Instant::now() < until {
                if cancel.load(Ordering::SeqCst) {
                    return Err(Failure::database("cancelled", "canceling statement due to user request"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            return Ok(Ran::changed(0));
        }
        "crash" => std::process::exit(3),
        "stderr" => {
            let mut err = std::io::stderr();
            let _ = writeln!(err, "{rest}");
            return Ok(Ran::changed(0));
        }
        "fail" => return Err(Failure::database("server", rest.to_owned())),
        _ => {}
    }
    let mut parser =
        Parser { tokens: tokens(statement)?, at: 0, params: params.into(), end: statement.chars().count() + 1 };
    if parser.tokens.is_empty() {
        return Ok(Ran::changed(0));
    }
    let writes = !matches!(parser.peek(), Some(Token::Word(word)) if word == "select");
    if writes
        && data.read_only
        && !matches!(parser.peek(), Some(Token::Word(word)) if matches!(word.as_str(), "begin" | "start" | "commit" | "rollback"))
    {
        return Err(Failure::database("refused", "cannot execute a change in a read-only transaction"));
    }
    if writes && !data.auto_commit && data.private.is_none() {
        data.private = Some(lock(&server.stores).get(&data.database).cloned().unwrap_or_default());
    }
    match parser.next() {
        Some(Token::Word(word)) if word == "begin" || word == "start" => {
            parser.word("transaction");
            parser.done()?;
            if data.private.is_none() {
                data.private = Some(lock(&server.stores).get(&data.database).cloned().unwrap_or_default());
            }
            Ok(Ran::changed(0))
        }
        Some(Token::Word(word)) if word == "commit" => {
            parser.done()?;
            if let Some(private) = data.private.take() {
                lock(&server.stores).insert(data.database.clone(), private);
            }
            Ok(Ran::changed(0))
        }
        Some(Token::Word(word)) if word == "rollback" => {
            parser.done()?;
            data.private = None;
            Ok(Ran::changed(0))
        }
        Some(Token::Word(word)) if word == "create" => {
            parser.expect_word("table")?;
            let name = parser.name()?;
            parser.expect_symbol('(')?;
            let mut table = Table::default();
            loop {
                let column = parser.name()?;
                let type_name = match parser.next() {
                    Some(Token::Word(word)) => match word.as_str() {
                        "int" | "integer" | "bigint" => "integer".to_owned(),
                        "real" | "float" | "double" => "real".to_owned(),
                        "bool" | "boolean" => "boolean".to_owned(),
                        _ => "text".to_owned(),
                    },
                    _ => return parser.fail("expected a type"),
                };
                if parser.word("primary") {
                    parser.expect_word("key")?;
                    table.primary = Some(table.columns.len());
                }
                table.columns.push((column, type_name));
                if parser.symbol(')') {
                    break;
                }
                parser.expect_symbol(',')?;
            }
            parser.done()?;
            with_store(server, data, |store| {
                if store.contains_key(&name) {
                    return Err(Failure::database("server", format!("table `{name}` already exists")));
                }
                store.insert(name, table);
                Ok(Ran::changed(0))
            })
        }
        Some(Token::Word(word)) if word == "drop" => {
            parser.expect_word("table")?;
            let name = parser.name()?;
            parser.done()?;
            with_store(server, data, |store| match store.remove(&name) {
                Some(_) => Ok(Ran::changed(0)),
                None => Err(Failure::database("server", format!("no table `{name}`"))),
            })
        }
        Some(Token::Word(word)) if word == "insert" => {
            parser.expect_word("into")?;
            let name = parser.name()?;
            let mut named = Vec::new();
            if parser.symbol('(') {
                loop {
                    named.push(parser.name()?);
                    if parser.symbol(')') {
                        break;
                    }
                    parser.expect_symbol(',')?;
                }
            }
            parser.expect_word("values")?;
            let mut tuples = Vec::new();
            loop {
                parser.expect_symbol('(')?;
                let mut values = Vec::new();
                loop {
                    values.push(parser.value()?);
                    if parser.symbol(')') {
                        break;
                    }
                    parser.expect_symbol(',')?;
                }
                tuples.push(values);
                if !parser.symbol(',') {
                    break;
                }
            }
            parser.done()?;
            with_store(server, data, |store| {
                let table =
                    store.get_mut(&name).ok_or_else(|| Failure::database("server", format!("no table `{name}`")))?;
                let order: Vec<usize> = if named.is_empty() {
                    (0..table.columns.len()).collect()
                } else {
                    named
                        .iter()
                        .map(|column| {
                            table
                                .columns
                                .iter()
                                .position(|(known, _)| known == column)
                                .ok_or_else(|| Failure::database("server", format!("no column `{column}`")))
                        })
                        .collect::<Result<_, _>>()?
                };
                let count = tuples.len();
                for values in tuples {
                    if values.len() != order.len() {
                        return Err(Failure::database("server", "the number of values does not match the columns"));
                    }
                    let mut row = vec![V::Null; table.columns.len()];
                    for (value, at) in values.into_iter().zip(&order) {
                        row[*at] = value.typed(&table.columns[*at].1);
                    }
                    if let Some(primary) = table.primary
                        && table.rows.iter().any(|existing| existing[primary] == row[primary])
                    {
                        return Err(Failure {
                            code: DATABASE_ERROR,
                            message: "UNIQUE constraint failed".into(),
                            data: Some(json!({ "kind": "server", "code": "23505" })),
                        });
                    }
                    table.rows.push(row);
                }
                Ok(Ran::changed(count))
            })
        }
        Some(Token::Word(word)) if word == "update" => {
            let name = parser.name()?;
            parser.expect_word("set")?;
            let table_columns = with_store(server, data, |store| store.get(&name).cloned())
                .ok_or_else(|| Failure::database("server", format!("no table `{name}`")))?;
            let mut sets = Vec::new();
            loop {
                let column = parser.name()?;
                let Some(at) = table_columns.columns.iter().position(|(known, _)| *known == column) else {
                    return Err(Failure::database("server", format!("no column `{column}`")));
                };
                parser.expect_symbol('=')?;
                sets.push((at, parser.value()?.typed(&table_columns.columns[at].1)));
                if !parser.symbol(',') {
                    break;
                }
            }
            let condition = condition(&mut parser, &table_columns)?;
            parser.done()?;
            with_store(server, data, |store| {
                let table =
                    store.get_mut(&name).ok_or_else(|| Failure::database("server", format!("no table `{name}`")))?;
                let mut count = 0;
                for row in table.rows.iter_mut().filter(|row| matches(row, &condition)) {
                    for (at, value) in &sets {
                        row[*at] = value.clone();
                    }
                    count += 1;
                }
                Ok(Ran::changed(count))
            })
        }
        Some(Token::Word(word)) if word == "delete" => {
            parser.expect_word("from")?;
            let name = parser.name()?;
            let table_columns = with_store(server, data, |store| store.get(&name).cloned())
                .ok_or_else(|| Failure::database("server", format!("no table `{name}`")))?;
            let condition = condition(&mut parser, &table_columns)?;
            parser.done()?;
            with_store(server, data, |store| {
                let table =
                    store.get_mut(&name).ok_or_else(|| Failure::database("server", format!("no table `{name}`")))?;
                let before = table.rows.len();
                table.rows.retain(|row| !matches(row, &condition));
                Ok(Ran::changed(before - table.rows.len()))
            })
        }
        Some(Token::Word(word)) if word == "select" => select(server, data, &mut parser),
        _ => {
            parser.at = 0;
            parser.fail("unknown statement")
        }
    }
}

enum Item {
    All,
    Column(String, Option<String>),
    Literal(V, String),
    Count(String),
}

fn select(server: &Server, data: &mut SessionData, parser: &mut Parser) -> Result<Ran, Failure> {
    let mut items = Vec::new();
    loop {
        let item = if parser.symbol('*') {
            Item::All
        } else if parser.word("count") {
            parser.expect_symbol('(')?;
            parser.expect_symbol('*')?;
            parser.expect_symbol(')')?;
            Item::Count("count".into())
        } else if matches!(parser.peek(), Some(Token::Word(_) | Token::Quoted(_)))
            && !matches!(parser.peek(), Some(Token::Word(word)) if matches!(word.as_str(), "null" | "true" | "false"))
        {
            Item::Column(parser.name()?, None)
        } else {
            let value = parser.value()?;
            Item::Literal(value, format!("?column{}?", items.len() + 1))
        };
        let item = if parser.word("as") {
            let alias = parser.name()?;
            match item {
                Item::Column(name, _) => Item::Column(name, Some(alias)),
                Item::Literal(value, _) => Item::Literal(value, alias),
                Item::Count(_) => Item::Count(alias),
                Item::All => return parser.fail("`*` cannot be named"),
            }
        } else {
            item
        };
        items.push(item);
        if !parser.symbol(',') {
            break;
        }
    }
    if !parser.word("from") {
        parser.done()?;
        let mut columns = Vec::new();
        let mut row = Vec::new();
        for item in items {
            match item {
                Item::Literal(value, name) => {
                    let type_name = match &value {
                        V::Int(_) => "integer",
                        V::Real(_) => "real",
                        V::Bool(_) => "boolean",
                        _ => "text",
                    };
                    columns.push(column_wire("", &name, type_name));
                    row.push(value);
                }
                _ => return parser.fail("a column needs FROM"),
            }
        }
        return Ok(Ran { columns: json!(columns), rows: vec![row], affected: None });
    }
    let name = parser.name()?;
    let (table_name, table) = if name == "numbers" && parser.symbol('(') {
        let count = match parser.value()? {
            V::Int(count) => count.max(0),
            _ => return parser.fail("numbers() takes a count"),
        };
        parser.expect_symbol(')')?;
        let rows = (1..=count).map(|n| vec![V::Int(n)]).collect();
        (String::new(), Table { columns: vec![("n".into(), "integer".into())], primary: None, rows })
    } else {
        let table = with_store(server, data, |store| store.get(&name).cloned())
            .ok_or_else(|| Failure::database("server", format!("no table `{name}`")))?;
        (name, table)
    };
    let condition = condition(parser, &table)?;
    let mut order = None;
    if parser.word("order") {
        parser.expect_word("by")?;
        let column = parser.name()?;
        let Some(at) = table.columns.iter().position(|(known, _)| *known == column) else {
            return Err(Failure::database("server", format!("no column `{column}`")));
        };
        let descending = parser.word("desc");
        if !descending {
            parser.word("asc");
        }
        order = Some((at, descending));
    }
    let mut limit = None;
    if parser.word("limit") {
        match parser.value()? {
            V::Int(count) => limit = Some(count.max(0) as usize),
            _ => return parser.fail("LIMIT takes a count"),
        }
    }
    parser.done()?;
    let mut rows: Vec<Vec<V>> = table.rows.iter().filter(|row| matches(row, &condition)).cloned().collect();
    if let Some((at, descending)) = order {
        rows.sort_by(|a, b| a[at].partial_cmp(&b[at]).unwrap_or(std::cmp::Ordering::Equal));
        if descending {
            rows.reverse();
        }
    }
    if let Some(limit) = limit {
        rows.truncate(limit);
    }
    if items.iter().any(|item| matches!(item, Item::Count(_))) {
        let label = items.iter().find_map(|item| match item {
            Item::Count(label) => Some(label.clone()),
            _ => None,
        });
        return Ok(Ran {
            columns: json!([column_wire("", &label.unwrap_or_default(), "integer")]),
            rows: vec![vec![V::Int(rows.len() as i64)]],
            affected: None,
        });
    }
    let mut columns = Vec::new();
    let mut picks: Vec<Result<usize, V>> = Vec::new();
    for item in &items {
        match item {
            Item::All => {
                for (at, (column, type_name)) in table.columns.iter().enumerate() {
                    columns.push(column_wire(&table_name, column, type_name));
                    picks.push(Ok(at));
                }
            }
            Item::Column(column, alias) => {
                let Some(at) = table.columns.iter().position(|(known, _)| known == column) else {
                    return Err(Failure::database("server", format!("no column `{column}`")));
                };
                let mut wire = column_wire(&table_name, alias.as_deref().unwrap_or(column), &table.columns[at].1);
                if !table_name.is_empty() {
                    wire["origin"] = json!(column);
                }
                columns.push(wire);
                picks.push(Ok(at));
            }
            Item::Literal(value, label) => {
                columns.push(column_wire("", label, "text"));
                picks.push(Err(value.clone()));
            }
            Item::Count(_) => {}
        }
    }
    let rows = rows
        .into_iter()
        .map(|row| {
            picks
                .iter()
                .map(|pick| match pick {
                    Ok(at) => row[*at].clone(),
                    Err(value) => value.clone(),
                })
                .collect()
        })
        .collect();
    Ok(Ran { columns: json!(columns), rows, affected: None })
}
