use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::driver::{
    BoxFuture, Canceller, ConnectSpec, ContextKind, DbError, Driver, DriverSession, ErrorKind, ExecOptions, Guards,
    KeyChange, KeyRequest, Param, TrackedState,
};
use crate::model::{Capabilities, DataModel, Node, NodeKind, SettingField, StatementKind};
use crate::value::{Column, Execution, KeyValue, Notice, Value};

pub const ID: &str = "memory-kv";

pub const EMBEDDED: &str = "memory-embedded";

pub const SQL: &str = "memory-sql";

pub const TABLE: u32 = 16384;

pub const PURCHASES: u32 = 16390;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Stored {
    Text(String),
    Hash(BTreeMap<String, String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    pub context: ContextKind,
    pub guards: Guards,
    pub state: TrackedState,
    pub password: Option<String>,
    pub via: Option<std::net::SocketAddr>,
}

#[derive(Default)]
struct Data {
    keys: BTreeMap<String, Stored>,
    expiring: BTreeMap<String, u64>,
    executed: Vec<String>,
    bound: Vec<(String, Vec<Param>)>,
    opened: Vec<Opened>,
    generation: usize,
    sessions: Vec<crate::sessions::ServerSession>,
    locks: Vec<crate::sessions::ServerLock>,
}

#[derive(Default)]
pub struct MemoryServer {
    data: Mutex<Data>,
    down: AtomicBool,
    password: Mutex<Option<String>>,
    cancels: AtomicUsize,
    ignore_cancel: AtomicBool,
    conflict: AtomicBool,
    fail: Mutex<Option<String>>,
    fail_reads: Mutex<Option<String>>,
    stop: tokio::sync::Notify,
    held: AtomicBool,
    released: tokio::sync::Notify,
}

impl MemoryServer {
    fn data(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    pub fn require_password(&self, password: &str) {
        *self.password.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(password.to_owned());
    }

    pub fn drop_connections(&self) {
        self.data().generation += 1;
        self.stop.notify_waiters();
    }

    pub fn ignore_cancel(&self, ignore: bool) {
        self.ignore_cancel.store(ignore, Ordering::SeqCst);
    }

    pub fn executed(&self) -> Vec<String> {
        self.data().executed.clone()
    }

    pub fn opened(&self) -> Vec<Opened> {
        self.data().opened.clone()
    }

    pub fn cancels(&self) -> usize {
        self.cancels.load(Ordering::SeqCst)
    }

    pub fn set_conflict(&self, conflict: bool) {
        self.conflict.store(conflict, Ordering::SeqCst);
    }

    pub fn fail_on(&self, text: Option<&str>) {
        *self.fail.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = text.map(str::to_owned);
    }

    pub fn fail_reads_with(&self, text: Option<&str>) {
        *self.fail_reads.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = text.map(str::to_owned);
    }

    pub fn hold(&self, held: bool) {
        self.held.store(held, Ordering::SeqCst);
        if !held {
            self.released.notify_waiters();
        }
    }

    async fn wait_while_held(&self) {
        loop {
            let released = self.released.notified();
            if !self.held.load(Ordering::SeqCst) {
                return;
            }
            released.await;
        }
    }

    pub fn set_sessions(&self, sessions: Vec<crate::sessions::ServerSession>, locks: Vec<crate::sessions::ServerLock>) {
        let mut data = self.data();
        data.sessions = sessions;
        data.locks = locks;
    }

    pub fn session_id(index: usize) -> String {
        (900 + index).to_string()
    }

    pub fn bound(&self) -> Vec<(String, Vec<Param>)> {
        self.data().bound.clone()
    }
}

pub struct MemoryDriver {
    id: &'static str,
    relational: bool,
    capabilities: Capabilities,
    pub server: Arc<MemoryServer>,
}

impl MemoryDriver {
    pub fn new(embedded: bool) -> Self {
        let capabilities = Capabilities {
            models: vec![DataModel::KeyValue],
            language: "redis".to_owned(),
            transactions: true,
            node_kinds: vec![NodeKind::KeyPrefix, NodeKind::Key],
            server_cancel: true,
            row_identity: Some("key".to_owned()),
            uri_schemes: vec!["memory".to_owned()],
            shared_connection: embedded,
            password: true,
            settings: vec![
                SettingField {
                    key: "mode".to_owned(),
                    kind: "choice".to_owned(),
                    choices: ["standalone", "sentinel", "cluster"].map(str::to_owned).to_vec(),
                    default: Some("standalone".to_owned()),
                },
                SettingField {
                    key: "separator".to_owned(),
                    kind: "text".to_owned(),
                    choices: Vec::new(),
                    default: Some(":".to_owned()),
                },
            ],
            ..Capabilities::default()
        };
        Self { id: if embedded { EMBEDDED } else { ID }, relational: false, capabilities, server: Arc::default() }
    }

    pub fn sql() -> Self {
        let mut driver = Self::new(false);
        driver.id = SQL;
        driver.relational = true;
        driver.capabilities.models = vec![DataModel::Relational];
        driver.capabilities.language = "sql:postgres".to_owned();
        driver.capabilities.bind_parameters = true;
        driver.capabilities.explain = true;
        driver.capabilities.sessions = true;
        driver.capabilities.row_identity = Some("primaryKey".to_owned());
        driver.capabilities.uri_schemes = vec!["memorysql".to_owned()];
        driver
    }

    pub fn without_password(mut self) -> Self {
        self.capabilities.password = false;
        self
    }
}

impl Driver for MemoryDriver {
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> &str {
        "Memory"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn default_port(&self) -> Option<u16> {
        None
    }

    fn guards_read_only(&self) -> bool {
        !self.relational
    }

    fn explain_options(&self) -> Vec<crate::explain::ExplainOption> {
        if self.relational { crate::explain::postgres_options() } else { Vec::new() }
    }

    fn explain(
        &self,
        statement: &str,
        options: &crate::explain::ExplainOptions,
    ) -> Result<crate::explain::ExplainRequest, DbError> {
        if !self.relational {
            return Err(DbError::new(ErrorKind::Unsupported, "explain"));
        }
        Ok(crate::explain::postgres(statement, options))
    }

    fn classify(&self, statement: &str) -> StatementKind {
        let verb = statement.split_whitespace().next().unwrap_or_default().to_ascii_uppercase();
        if matches!(verb.as_str(), "GET" | "KEYS" | "HGETALL" | "SLEEP" | "PING" | "ROWS" | "SELECT") {
            StatementKind::Read
        } else {
            StatementKind::Write
        }
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> BoxFuture<'a, Result<Box<dyn DriverSession>, DbError>> {
        Box::pin(async move {
            let server = &self.server;
            if server.down.load(Ordering::SeqCst) {
                return Err(DbError::new(ErrorKind::Connection, "connection refused"));
            }
            let wanted = server.password.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            if wanted.is_some() && wanted.as_deref() != spec.password.as_ref().map(|secret| secret.expose()) {
                return Err(DbError::new(ErrorKind::Authentication, "wrong password"));
            }
            let mut data = server.data();
            data.opened.push(Opened {
                context: spec.context.clone(),
                guards: spec.guards.clone(),
                state: spec.state.clone(),
                password: spec.password.as_ref().map(|secret| secret.expose().to_owned()),
                via: spec.via,
            });
            let generation = data.generation;
            let server_id = MemoryServer::session_id(data.opened.len() - 1);
            drop(data);
            Ok(Box::new(MemorySession {
                server: Arc::clone(server),
                relational: self.relational,
                generation,
                in_transaction: false,
                queued: Vec::new(),
                state: spec.state.clone(),
                server_id,
            }) as Box<dyn DriverSession>)
        })
    }
}

struct MemorySession {
    server: Arc<MemoryServer>,
    relational: bool,
    generation: usize,
    in_transaction: bool,
    queued: Vec<String>,
    state: TrackedState,
    server_id: String,
}

struct MemoryCancel(Arc<MemoryServer>);

impl Canceller for MemoryCancel {
    fn cancel(&self) -> BoxFuture<'static, Result<(), DbError>> {
        let server = Arc::clone(&self.0);
        Box::pin(async move {
            server.cancels.fetch_add(1, Ordering::SeqCst);
            if !server.ignore_cancel.load(Ordering::SeqCst) {
                server.stop.notify_waiters();
            }
            Ok(())
        })
    }
}

pub const PICTURE: [u8; 73] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x02, 0x00, 0x00, 0x00, 0x26, 0x93, 0x09, 0x29, 0x00, 0x00, 0x00, 0x10, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xd0, 0xa8, 0x38, 0x01, 0x47, 0x0c, 0xc4, 0x71, 0x00, 0x3f, 0x52, 0x16, 0x81,
    0xe7, 0xbe, 0x71, 0x04, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn table_rows(count: usize, max_rows: usize) -> Execution {
    let mut execution = Execution {
        columns: [
            ("id", "int8"),
            ("name", "text"),
            ("payload", "jsonb"),
            ("picture", "bytea"),
            ("tags", "_text"),
            ("created", "timestamptz"),
        ]
        .iter()
        .map(|(name, kind)| Column::new((*name).to_owned(), (*kind).to_owned()))
        .collect(),
        ..Execution::default()
    };
    for index in 0..count {
        if execution.rows.len() >= max_rows {
            execution.truncated = true;
            break;
        }
        let id = i64::try_from(index).unwrap_or(i64::MAX) + 1;
        execution.rows.push(vec![
            Value::Integer(id),
            Value::Text(format!("Customer {id}")),
            Value::Json(format!(
                r#"{{"id":{id},"tier":"gold","address":{{"city":"Lisbon","zip":"1100-148"}},"orders":[{id},{}]}}"#,
                id * 10
            )),
            Value::Bytes(PICTURE.to_vec()),
            Value::Array(vec![Value::Text("new".to_owned()), Value::Text("vip client".to_owned())]),
            Value::Temporal(crate::value::Typed {
                text: format!("2026-10-{:02} 09:30:00+08", 1 + index % 28),
                type_name: "timestamptz".to_owned(),
            }),
        ]);
    }
    execution
}

fn purchase_rows() -> Execution {
    let mut execution = Execution {
        columns: [("id", "int8"), ("customer_id", "int8"), ("total", "numeric")]
            .iter()
            .map(|(name, kind)| Column::new((*name).to_owned(), (*kind).to_owned()))
            .collect(),
        ..Execution::default()
    };
    for index in 1..=30_i64 {
        execution.rows.push(vec![
            Value::Integer(index),
            Value::Integer((index * 7) % 25 + 1),
            Value::Decimal(format!("{}.{:02}", 100 + index, index)),
        ]);
    }
    execution
}

pub fn place_area() -> Vec<u8> {
    let mut out = vec![1u8];
    out.extend(0x2000_0003u32.to_le_bytes());
    out.extend(4326u32.to_le_bytes());
    let rings: [&[(f64, f64)]; 2] = [
        &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0), (0.0, 0.0)],
        &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0), (1.0, 1.0)],
    ];
    out.extend((rings.len() as u32).to_le_bytes());
    for ring in rings {
        out.extend((ring.len() as u32).to_le_bytes());
        for (x, y) in ring {
            out.extend(x.to_le_bytes());
            out.extend(y.to_le_bytes());
        }
    }
    out
}

fn place_rows() -> Execution {
    let geometry = |bytes: Vec<u8>| {
        Value::Other(crate::value::Typed {
            text: format!("\\x{}", crate::value::hex(&bytes)),
            type_name: "geometry".to_owned(),
        })
    };
    let mut point = vec![1u8];
    point.extend(0x2000_0001u32.to_le_bytes());
    point.extend(4326u32.to_le_bytes());
    point.extend(5.5f64.to_le_bytes());
    point.extend(2.0f64.to_le_bytes());
    Execution {
        columns: [("id", "int8"), ("name", "text"), ("area", "geometry"), ("route", "text")]
            .iter()
            .map(|(name, kind)| Column::new((*name).to_owned(), (*kind).to_owned()))
            .collect(),
        rows: vec![
            vec![
                Value::Integer(1),
                Value::Text("Park".to_owned()),
                geometry(place_area()),
                Value::Text("LINESTRING(0 5, 3 6, 6 5)".to_owned()),
            ],
            vec![Value::Integer(2), Value::Text("Well".to_owned()), geometry(point), Value::Null],
            vec![
                Value::Integer(3),
                Value::Text("Nowhere".to_owned()),
                Value::Null,
                Value::Text("not a shape".to_owned()),
            ],
        ],
        ..Execution::default()
    }
}

fn column_named(execution: &Execution, name: &str) -> Option<usize> {
    let name = name.trim_matches(|c: char| c == '(' || c == ')' || c == '"' || c == '`');
    execution.columns.iter().position(|column| column.name.eq_ignore_ascii_case(name))
}

fn narrow(execution: &mut Execution, statement: &str) {
    let upper = statement.to_ascii_uppercase();
    let order_at = upper.rfind(" ORDER BY ");
    if let Some(at) = upper.find(" LIKE '") {
        let column = statement[..at].split_whitespace().last().and_then(|name| column_named(execution, name));
        let rest = &statement[at + " LIKE '".len()..];
        if let (Some(column), Some(end)) = (column, rest.find('\'')) {
            let pattern = &rest[..end];
            let (text, prefix) = pattern.strip_suffix('%').map_or((pattern, false), |text| (text, true));
            execution.rows.retain(|row| match row.get(column) {
                Some(Value::Text(value)) => {
                    if prefix {
                        value.starts_with(text)
                    } else {
                        value == text
                    }
                }
                _ => false,
            });
        }
    }
    let Some(at) = order_at else { return };
    let first = statement[at + " ORDER BY ".len()..].split(',').next().unwrap_or_default();
    let mut words = first.split_whitespace();
    let Some(column) = words.next().and_then(|name| column_named(execution, name)) else { return };
    let descending = words.next().is_some_and(|word| word.eq_ignore_ascii_case("DESC"));
    execution.rows.sort_by(|a, b| {
        let order = match (a.get(column), b.get(column)) {
            (Some(Value::Integer(a)), Some(Value::Integer(b))) => a.cmp(b),
            (Some(Value::Text(a)), Some(Value::Text(b))) => a.cmp(b),
            _ => std::cmp::Ordering::Equal,
        };
        if descending { order.reverse() } else { order }
    });
}

fn text_rows(columns: &[&str], rows: Vec<Vec<Value>>) -> Execution {
    Execution {
        columns: columns.iter().map(|name| Column::new((*name).to_owned(), "string".to_owned())).collect(),
        rows,
        ..Execution::default()
    }
}

impl MemorySession {
    fn alive(&self) -> bool {
        self.server.data().generation == self.generation
    }

    fn lost() -> DbError {
        DbError::new(ErrorKind::Connection, "connection reset by peer")
    }

    fn apply_one(&self, data: &mut Data, words: &[&str]) -> Result<Execution, DbError> {
        let verb = words.first().map(|verb| verb.to_ascii_uppercase()).unwrap_or_default();
        let arg = |at: usize| {
            words
                .get(at)
                .map(|word| (*word).to_owned())
                .ok_or_else(|| DbError::new(ErrorKind::Server, "wrong number of arguments"))
        };
        match verb.as_str() {
            "SET" => {
                data.keys.insert(arg(1)?, Stored::Text(arg(2)?));
                Ok(Execution { affected: Some(1), ..Execution::default() })
            }
            "DEL" => Ok(Execution {
                affected: Some(u64::from(data.keys.remove(&arg(1)?).is_some())),
                ..Execution::default()
            }),
            "HSET" => {
                let key = arg(1)?;
                let entry = data.keys.entry(key).or_insert_with(|| Stored::Hash(BTreeMap::new()));
                let Stored::Hash(fields) = entry else { return Err(DbError::new(ErrorKind::Server, "WRONGTYPE")) };
                fields.insert(arg(2)?, arg(3)?);
                Ok(Execution { affected: Some(1), ..Execution::default() })
            }
            _ => Err(DbError::new(ErrorKind::Server, format!("unknown command '{verb}'"))),
        }
    }

    async fn run(&mut self, statement: &str, options: &ExecOptions) -> Result<Execution, DbError> {
        self.server.data().executed.push(statement.to_owned());
        self.server.wait_while_held().await;
        if !self.alive() {
            return Err(Self::lost());
        }
        let words: Vec<&str> = statement.split_whitespace().collect();
        let verb = words.first().map(|verb| verb.to_ascii_uppercase()).unwrap_or_default();
        let key = |at: usize| words.get(at).map(|word| (*word).to_owned()).unwrap_or_default();
        let mut execution = match verb.as_str() {
            "PING" => text_rows(&["reply"], vec![vec![Value::Text("PONG".to_owned())]]),
            "PLACES" => place_rows(),
            "GET" => {
                let data = self.server.data();
                let (kind, value) = match data.keys.get(&key(1)) {
                    Some(Stored::Text(text)) => ("string", Value::Text(text.clone())),
                    Some(Stored::Hash(_)) => return Err(DbError::new(ErrorKind::Server, "WRONGTYPE")),
                    None => ("none", Value::Null),
                };
                text_rows(
                    &["key", "type", "ttl", "value"],
                    vec![vec![Value::Text(key(1)), Value::Text(kind.to_owned()), Value::Integer(-1), value]],
                )
            }
            "HGETALL" => {
                let data = self.server.data();
                let value = match data.keys.get(&key(1)) {
                    Some(Stored::Hash(fields)) => Value::Map(
                        fields.iter().map(|(field, value)| (field.clone(), Value::Text(value.clone()))).collect(),
                    ),
                    _ => Value::Null,
                };
                text_rows(
                    &["key", "type", "ttl", "value"],
                    vec![vec![Value::Text(key(1)), Value::Text("hash".to_owned()), Value::Integer(-1), value]],
                )
            }
            "KEYS" => {
                let data = self.server.data();
                let mut execution = Execution::default();
                for name in data.keys.keys() {
                    if execution.rows.len() >= options.max_rows {
                        execution.truncated = true;
                        break;
                    }
                    execution.rows.push(vec![Value::Text(name.clone())]);
                }
                execution.columns = vec![Column::new("key".to_owned(), "string".to_owned())];
                execution
            }
            "ROWS" | "LPOP" => {
                let count: usize = key(if verb == "LPOP" { 2 } else { 1 }).parse().unwrap_or(0);
                let mut execution = Execution {
                    columns: [
                        ("id", "int"),
                        ("name", "string"),
                        ("even", "bool"),
                        ("nothing", "string"),
                        ("doc", "document"),
                    ]
                    .iter()
                    .map(|(name, kind)| Column::new((*name).to_owned(), (*kind).to_owned()))
                    .collect(),
                    ..Execution::default()
                };
                for index in 0..count {
                    if execution.rows.len() >= options.max_rows {
                        execution.truncated = true;
                        break;
                    }
                    let id = i64::try_from(index).unwrap_or(i64::MAX);
                    execution.rows.push(vec![
                        Value::Integer(id),
                        Value::Text(format!("row {index}")),
                        Value::Bool(index % 2 == 0),
                        Value::Null,
                        Value::Document(vec![
                            ("n".to_owned(), Value::Integer(id)),
                            ("tag".to_owned(), Value::Text("x".to_owned())),
                        ]),
                    ]);
                }
                execution
            }
            "UPDATE" | "DELETE" if self.relational && statement.to_ascii_uppercase().contains(" RETURNING ") => {
                table_rows(25, options.max_rows)
            }
            "SELECT" if self.relational => {
                let failing = self.server.fail_reads.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                if let Some(text) = failing.filter(|text| statement.contains(text.as_str())) {
                    return Err(
                        DbError::new(ErrorKind::Server, format!("column \"{text}\" does not exist")).with_code("42703")
                    );
                }
                let purchases = statement.to_ascii_lowercase().contains("purchases");
                let mut execution = if purchases { purchase_rows() } else { table_rows(25, usize::MAX) };
                narrow(&mut execution, statement);
                if execution.rows.len() > options.max_rows {
                    execution.rows.truncate(options.max_rows);
                    execution.truncated = true;
                }
                let table = if purchases { PURCHASES } else { TABLE };
                for (at, column) in execution.columns.iter_mut().enumerate() {
                    let position = i16::try_from(at + 1).unwrap_or(i16::MAX);
                    column.source = Some(crate::value::ColumnSource { table, position });
                }
                execution
            }
            "SELECT" => table_rows(25, options.max_rows),
            "EXPLAIN" if self.relational => {
                let analyzed = statement.contains("ANALYZE");
                let (root, scan, total) = if analyzed {
                    (
                        r#", "Actual Total Time": 2.0, "Actual Rows": 25, "Actual Loops": 1"#,
                        r#", "Actual Total Time": 1.5, "Actual Rows": 25, "Actual Loops": 1"#,
                        r#", "Execution Time": 2.1"#,
                    )
                } else {
                    ("", "", "")
                };
                let plan = format!(
                    r#"[{{"Plan": {{"Node Type": "Sort", "Startup Cost": 38.0, "Total Cost": 40.0, "Plan Rows": 25, "Sort Key": ["id"]{root}, "Plans": [{{"Node Type": "Seq Scan", "Relation Name": "rows", "Alias": "rows", "Startup Cost": 0.0, "Total Cost": 30.0, "Plan Rows": 25, "Filter": "(even)"{scan}}}]}}, "Planning Time": 0.1{total}}}]"#
                );
                text_rows(&["QUERY PLAN"], vec![vec![Value::Json(plan)]])
            }
            "START" if self.relational => {
                self.in_transaction = true;
                Execution::default()
            }
            "BEGIN" | "SAVEPOINT" if self.relational => {
                self.in_transaction = true;
                Execution::default()
            }
            "COMMIT" if self.relational => {
                self.in_transaction = false;
                Execution::default()
            }
            "ROLLBACK" if self.relational => {
                if !words.get(1).is_some_and(|word| word.eq_ignore_ascii_case("to")) {
                    self.in_transaction = false;
                }
                Execution::default()
            }
            "RELEASE" if self.relational => Execution::default(),
            "INSERT" | "TRUNCATE" | "CREATE" if self.relational => {
                let failing = self.server.fail.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                if failing.is_some_and(|text| statement.contains(&text)) {
                    return Err(DbError::new(ErrorKind::Server, "duplicate key value violates unique constraint")
                        .with_code("23505"));
                }
                if !self.state.auto_commit {
                    self.in_transaction = true;
                }
                let rows = statement.lines().filter(|line| line.starts_with('(')).count();
                Execution { affected: Some(rows as u64), ..Execution::default() }
            }
            "FAIL" => {
                return Err(DbError::new(ErrorKind::Server, format!("syntax error at or near \"{}\"", key(1)))
                    .with_code("42601")
                    .at(6));
            }
            "NOTICE" => Execution {
                notices: vec![Notice {
                    severity: "NOTICE".to_owned(),
                    message: words[1..].join(" "),
                    code: Some("00000".to_owned()),
                }],
                ..Execution::default()
            },
            "SLEEP" => {
                let millis: u64 = key(1).parse().unwrap_or(1000);
                let stopped = self.server.stop.notified();
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(millis)) => {}
                    () = stopped => {
                        return Err(if self.alive() { DbError::new(ErrorKind::Server, "query interrupted") } else { Self::lost() });
                    }
                }
                text_rows(&["slept"], vec![vec![Value::Integer(i64::try_from(millis).unwrap_or(i64::MAX))]])
            }
            "SLOWSET" => {
                let stopped = self.server.stop.notified();
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(200)) => {}
                    () = stopped => {}
                }
                if !self.alive() {
                    return Err(Self::lost());
                }
                let mut data = self.server.data();
                data.keys.insert(key(1), Stored::Text(key(2)));
                Execution { affected: Some(1), ..Execution::default() }
            }
            "MULTI" => {
                self.in_transaction = true;
                Execution::default()
            }
            "EXEC" => {
                let queued = std::mem::take(&mut self.queued);
                self.in_transaction = false;
                let mut data = self.server.data();
                for command in queued {
                    let words: Vec<&str> = command.split_whitespace().collect();
                    self.apply_one(&mut data, &words)?;
                }
                Execution::default()
            }
            "DISCARD" => {
                self.queued.clear();
                self.in_transaction = false;
                Execution::default()
            }
            _ if self.state.read_only => return Err(DbError::new(ErrorKind::Server, "READONLY")),
            _ if self.in_transaction || !self.state.auto_commit => {
                self.in_transaction = true;
                self.queued.push(statement.to_owned());
                Execution::default()
            }
            _ => {
                let mut data = self.server.data();
                self.apply_one(&mut data, &words)?
            }
        };
        if verb == "SET" && words.get(1) == Some(&"notice") {
            execution.notices.push(Notice { severity: "NOTICE".to_owned(), message: "noticed".to_owned(), code: None });
        }
        Ok(execution)
    }
}

impl DriverSession for MemorySession {
    fn execute<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a ExecOptions,
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(self.run(statement, options))
    }

    fn stream_bound<'a>(
        &'a mut self,
        statement: &'a str,
        params: &'a [Value],
        options: &'a crate::driver::StreamOptions,
        sink: &'a mut dyn crate::driver::RowSink,
    ) -> BoxFuture<'a, Result<crate::driver::StreamEnd, DbError>> {
        if params.is_empty() {
            return self.stream(statement, options, sink);
        }
        Box::pin(async move {
            if !self.relational {
                return Err(DbError::new(ErrorKind::Unsupported, "bound parameters"));
            }
            let texts = params.iter().map(crate::edit::param_text).collect();
            self.server.data().bound.push((statement.to_owned(), texts));
            self.stream(statement, options, sink).await
        })
    }

    fn execute_bound<'a>(
        &'a mut self,
        statement: &'a str,
        params: &'a [Param],
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(async move {
            if !self.relational {
                return Err(DbError::new(ErrorKind::Unsupported, "bound parameters"));
            }
            if !self.alive() {
                return Err(Self::lost());
            }
            let mut data = self.server.data();
            data.executed.push(statement.to_owned());
            data.bound.push((statement.to_owned(), params.to_vec()));
            drop(data);
            let failing = self.server.fail.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            if failing.is_some_and(|text| statement.contains(&text)) {
                return Err(DbError::new(ErrorKind::Server, "duplicate key value violates unique constraint")
                    .with_code("23505"));
            }
            let writes_rows = statement.starts_with("UPDATE") || statement.starts_with("DELETE");
            let affected = u64::from(!(writes_rows && self.server.conflict.load(Ordering::SeqCst)));
            Ok(Execution { affected: Some(affected), ..Execution::default() })
        })
    }

    fn children<'a>(&'a mut self, path: &'a [String]) -> BoxFuture<'a, Result<Vec<Node>, DbError>> {
        Box::pin(async move {
            if !self.alive() {
                return Err(Self::lost());
            }
            let prefix: String = path.iter().map(|part| format!("{part}:")).collect();
            let data = self.server.data();
            let mut nodes: Vec<Node> = Vec::new();
            for key in data.keys.keys().filter(|key| key.starts_with(&prefix)) {
                let rest = &key[prefix.len()..];
                let (label, kind, more) = match rest.split_once(':') {
                    Some((group, _)) => (group.to_owned(), NodeKind::KeyPrefix, true),
                    None => (rest.to_owned(), NodeKind::Key, false),
                };
                if nodes.iter().any(|node| node.label == label && node.kind == kind) {
                    continue;
                }
                let mut node_path = path.to_vec();
                node_path.push(label.clone());
                nodes.push(Node::new(node_path, kind, label, more));
            }
            Ok(nodes)
        })
    }

    fn key<'a>(&'a mut self, path: &'a [String], request: &'a KeyRequest) -> BoxFuture<'a, Result<KeyValue, DbError>> {
        Box::pin(async move {
            if !self.alive() {
                return Err(Self::lost());
            }
            let key = path.join(":");
            let mut data = self.server.data();
            if let Some(change) = request.change {
                if self.state.read_only {
                    return Err(DbError::new(ErrorKind::Server, "READONLY").with_code("readOnly"));
                }
                match change {
                    KeyChange::Expire(seconds) => data.expiring.insert(key.clone(), seconds),
                    KeyChange::Persist => data.expiring.remove(&key),
                };
            }
            let ttl_ms = data.expiring.get(&key).map(|seconds| i64::try_from(seconds * 1000).unwrap_or(i64::MAX));
            let column = |name: &str| Column::new(name.to_owned(), "string".to_owned());
            let value = match data.keys.get(&key) {
                Some(Stored::Text(text)) => KeyValue {
                    type_name: "string".to_owned(),
                    length: Some(text.len() as u64),
                    columns: vec![column("value")],
                    rows: vec![vec![Value::Text(text.clone())]],
                    ..KeyValue::default()
                },
                Some(Stored::Hash(fields)) => KeyValue {
                    type_name: "hash".to_owned(),
                    length: Some(fields.len() as u64),
                    columns: vec![column("field"), column("value")],
                    truncated: fields.len() > request.limit,
                    rows: fields
                        .iter()
                        .take(request.limit)
                        .map(|(field, value)| vec![Value::Text(field.clone()), Value::Text(value.clone())])
                        .collect(),
                    ..KeyValue::default()
                },
                None => KeyValue { type_name: "none".to_owned(), ..KeyValue::default() },
            };
            Ok(KeyValue { key, ttl_ms, ..value })
        })
    }

    fn sessions<'a>(
        &'a mut self,
        request: &'a crate::sessions::SessionsRequest,
    ) -> BoxFuture<'a, Result<crate::sessions::ServerSessions, DbError>> {
        Box::pin(async move {
            if !self.relational {
                return Err(DbError::new(ErrorKind::Unsupported, "sessions"));
            }
            if !self.alive() {
                return Err(Self::lost());
            }
            let data = self.server.data();
            let own = (0..data.opened.len()).map(|index| crate::sessions::ServerSession {
                id: MemoryServer::session_id(index),
                user: Some("memory".to_owned()),
                application: Some(data.opened[index].guards.application_name.clone()),
                state: Some("idle".to_owned()),
                ..crate::sessions::ServerSession::default()
            });
            let all: Vec<_> = data
                .sessions
                .iter()
                .cloned()
                .chain(own)
                .filter(|session| request.show_idle || session.state.as_deref() != Some("idle"))
                .collect();
            let total = all.len();
            Ok(crate::sessions::ServerSessions {
                sessions: all.into_iter().take(request.limit).collect(),
                total,
                truncated: total > request.limit,
                locks: if request.locks { data.locks.clone() } else { Vec::new() },
                ..crate::sessions::ServerSessions::default()
            })
        })
    }

    fn end_session<'a>(
        &'a mut self,
        id: &'a str,
        end: crate::sessions::SessionEnd,
    ) -> BoxFuture<'a, Result<crate::sessions::Ended, DbError>> {
        Box::pin(async move {
            if !self.relational {
                return Err(DbError::new(ErrorKind::Unsupported, "sessions"));
            }
            let statement = crate::sessions::end_statement("sql:postgres", id, end)
                .ok_or_else(|| DbError::new(ErrorKind::Invalid, id))?;
            let mut data = self.server.data();
            data.executed.push(statement.clone());
            let ended = data.sessions.iter().any(|session| session.id == id);
            if end == crate::sessions::SessionEnd::Terminate {
                data.sessions.retain(|session| session.id != id);
            }
            Ok(crate::sessions::Ended { statement, ended })
        })
    }

    fn server_id(&self) -> Option<String> {
        Some(self.server_id.clone())
    }

    fn ping(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move { if self.alive() { Ok(()) } else { Err(Self::lost()) } })
    }

    fn server_version(&mut self) -> BoxFuture<'_, Result<Option<String>, DbError>> {
        Box::pin(async { Ok(Some("Memory 1.0".to_owned())) })
    }

    fn apply<'a>(&'a mut self, state: &'a TrackedState) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            if !self.alive() {
                return Err(Self::lost());
            }
            self.state = state.clone();
            Ok(())
        })
    }

    fn commit(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move { self.run("EXEC", &ExecOptions::default()).await.map(|_| ()) })
    }

    fn rollback(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move { self.run("DISCARD", &ExecOptions::default()).await.map(|_| ()) })
    }

    fn in_transaction(&self) -> bool {
        self.in_transaction
    }

    fn is_closed(&self) -> bool {
        !self.alive()
    }

    fn canceller(&self) -> Option<Arc<dyn Canceller>> {
        Some(Arc::new(MemoryCancel(Arc::clone(&self.server))))
    }

    fn take_notices(&mut self) -> Vec<Notice> {
        Vec::new()
    }
}
