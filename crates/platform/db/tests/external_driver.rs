use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};
use zen_db::driver::{ContextKind, Driver, ErrorKind, ExecOptions};
use zen_db::engine::{
    ConnectRequest, Engine, OperationState, OperationStatus, QueryOptions, QueryStatement, Timing, TransactionPolicy,
};
use zen_db::external::protocol::{self, InitializeResult};
use zen_db::external::{DriverDeclaration, ExternalDriver, ExternalSpec, ProcessModel};
use zen_db::model::Capabilities;
use zen_db::registry::{AuthKind, DataSource};
use zen_db::schema::{IntrospectRequest, Snapshot};
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::{Column, Value};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(30);

fn custom() -> bool {
    std::env::var_os("ZEN_DB_DRIVER").is_some()
}

fn program() -> PathBuf {
    std::env::var_os("ZEN_DB_DRIVER")
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_zen-db-reference-driver")), PathBuf::from)
}

fn driver_id() -> String {
    std::env::var("ZEN_DB_DRIVER_ID").unwrap_or_else(|_| "zen-reference".to_owned())
}

fn args() -> Vec<String> {
    std::env::var("ZEN_DB_DRIVER_ARGS").ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn connect_settings() -> Json {
    std::env::var("ZEN_DB_DRIVER_CONNECT")
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({ "settings": { "seed": true } }))
}

fn password() -> Option<String> {
    std::env::var("ZEN_DB_DRIVER_PASSWORD").ok()
}

fn query_text() -> String {
    std::env::var("ZEN_DB_DRIVER_QUERY").unwrap_or_else(|_| "SELECT * FROM numbers(3)".to_owned())
}

fn slow_text() -> Option<String> {
    match std::env::var("ZEN_DB_DRIVER_SLOW") {
        Ok(text) if text.is_empty() => None,
        Ok(text) => Some(text),
        Err(_) if custom() => None,
        Err(_) => Some("SLEEP 10000".to_owned()),
    }
}

fn reference_only() -> bool {
    if custom() {
        let _ = writeln!(std::io::stderr(), "skipped: reference only");
    }
    !custom()
}

struct Raw {
    child: Child,
    input: Option<ChildStdin>,
    lines: mpsc::Receiver<Json>,
    next: u64,
}

impl Raw {
    fn start() -> Self {
        let mut child = Command::new(program())
            .args(args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the driver program starts");
        let input = child.stdin.take();
        let output: ChildStdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let message: Json = serde_json::from_str(&line).unwrap_or_else(|_| json!({ "unreadable": line }));
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Self { child, input, lines, next: 1 }
    }

    fn write(&mut self, text: &str) {
        let input = self.input.as_mut().unwrap();
        input.write_all(text.as_bytes()).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }

    fn send(&mut self, method: &str, params: Json) -> u64 {
        let id = self.next;
        self.next += 1;
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string());
        id
    }

    fn answer_to(&mut self, id: u64) -> Json {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let message = self.lines.recv_timeout(left).unwrap_or_else(|_| panic!("no answer to request {id}"));
            assert_eq!(message["jsonrpc"], "2.0", "every message is JSON-RPC 2.0: {message}");
            if message.get("method").is_some() && message.get("id").is_none() {
                continue;
            }
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    fn call(&mut self, method: &str, params: Json) -> Json {
        let id = self.send(method, params);
        let answer = self.answer_to(id);
        assert!(answer.get("error").is_none(), "`{method}` failed: {answer}");
        answer["result"].clone()
    }

    fn initialized() -> (Self, InitializeResult) {
        let mut raw = Self::start();
        let result = raw.call(
            "initialize",
            json!({ "protocol": protocol::VERSION, "driver": driver_id(), "client": { "name": "Zen Conformance", "version": "1" } }),
        );
        let result: InitializeResult = serde_json::from_value(result).expect("initialize answers the protocol's shape");
        (raw, result)
    }

    fn session(&mut self) -> String {
        let mut params = connect_settings();
        params["connection"] = json!("conformance");
        params["context"] = json!("main");
        params["timeoutMs"] = json!(10_000);
        if let Some(password) = password() {
            params["password"] = json!(password);
        }
        let result = self.call("connect", params);
        result["session"].as_str().expect("connect answers a session id").to_owned()
    }

    fn exits_within(&mut self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if self.child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn error_code(answer: &Json) -> i64 {
    answer["error"]["code"].as_i64().unwrap_or_else(|| panic!("an error is answered: {answer}"))
}

#[test]
fn initialize_answers_the_protocol_the_driver_and_its_capabilities() {
    let (_raw, result) = Raw::initialized();
    assert_eq!(result.protocol, protocol::VERSION);
    assert_eq!(result.driver.id, driver_id());
    assert!(!result.driver.name.is_empty());
    assert!(!result.capabilities.models.is_empty(), "a driver serves at least one data model");
    assert!(!result.capabilities.language.is_empty(), "a driver names its query language");
}

#[test]
fn another_protocol_version_is_refused() {
    let mut raw = Raw::start();
    let id = raw.send(
        "initialize",
        json!({ "protocol": 99, "driver": driver_id(), "client": { "name": "Zen", "version": "1" } }),
    );
    assert_eq!(error_code(&raw.answer_to(id)), protocol::UNSUPPORTED_PROTOCOL);
}

#[test]
fn an_unknown_method_is_answered_with_method_not_found() {
    let (mut raw, _) = Raw::initialized();
    let id = raw.send("zen/noSuchMethod", json!({}));
    assert_eq!(error_code(&raw.answer_to(id)), protocol::METHOD_NOT_FOUND);
    let session = raw.session();
    raw.call("ping", json!({ "session": session }));
}

#[test]
fn a_line_that_is_not_json_is_answered_with_a_parse_error_and_the_driver_goes_on() {
    let (mut raw, _) = Raw::initialized();
    raw.write("{ this is not json");
    let answer = raw.lines.recv_timeout(WAIT).expect("an answer to a broken line");
    assert_eq!(error_code(&answer), protocol::PARSE_ERROR);
    let session = raw.session();
    raw.call("ping", json!({ "session": session }));
}

#[test]
fn a_session_pings_runs_a_query_with_typed_values_and_closes() {
    let (mut raw, _) = Raw::initialized();
    let session = raw.session();
    raw.call("ping", json!({ "session": session }));
    let result = raw.call("execute", json!({ "session": session, "statement": query_text(), "maxRows": 100 }));
    let columns: Vec<Column> = serde_json::from_value(result["columns"].clone()).expect("columns have name and type");
    assert!(!columns.is_empty(), "the query returns columns: {result}");
    let rows: Vec<Vec<Value>> =
        serde_json::from_value(result["rows"].clone()).expect("rows hold the protocol's values");
    assert!(!rows.is_empty(), "the query returns rows: {result}");
    assert!(rows.iter().all(|row| row.len() == columns.len()), "every row has a value per column");
    let id = raw.send("execute", json!({ "session": session, "statement": query_text(), "maxRows": 0 }));
    let limited = raw.answer_to(id);
    assert_eq!(limited["result"]["rows"].as_array().map(Vec::len), Some(0), "maxRows is kept: {limited}");
    assert_eq!(limited["result"]["truncated"], true, "a cut result says so: {limited}");
    raw.call("close", json!({ "session": session }));
    let id = raw.send("ping", json!({ "session": session }));
    assert_eq!(error_code(&raw.answer_to(id)), protocol::UNKNOWN_SESSION, "a closed session is gone");
}

#[test]
fn a_statement_error_carries_the_database_error_kind() {
    let (mut raw, _) = Raw::initialized();
    let session = raw.session();
    let id =
        raw.send("execute", json!({ "session": session, "statement": "THIS IS NOT A STATEMENT 42", "maxRows": 10 }));
    let answer = raw.answer_to(id);
    assert_eq!(error_code(&answer), protocol::DATABASE_ERROR, "{answer}");
    let data: protocol::ErrorData = serde_json::from_value(answer["error"]["data"].clone()).expect("error data");
    assert!(data.kind.is_some(), "the error names its kind: {answer}");
    raw.call("ping", json!({ "session": session }));
}

#[test]
fn server_paging_reads_pages_through_a_cursor() {
    let (mut raw, init) = Raw::initialized();
    if !init.capabilities.server_paging {
        return;
    }
    let session = raw.session();
    let opened = raw.call("query", json!({ "session": session, "statement": query_text(), "skip": 0 }));
    let cursor = opened["cursor"].as_str().expect("a query with rows opens a cursor").to_owned();
    let page = raw.call("fetch", json!({ "session": session, "cursor": cursor, "count": 1 }));
    assert!(page["rows"].as_array().is_some_and(|rows| rows.len() <= 1), "a page holds at most the count: {page}");
    let mut done = page["done"] == true;
    let mut pages = 0;
    while !done && pages < 10_000 {
        let page = raw.call("fetch", json!({ "session": session, "cursor": cursor, "count": 1000 }));
        done = page["done"] == true;
        pages += 1;
    }
    assert!(done, "the cursor ends");
    let again = raw.call("query", json!({ "session": session, "statement": query_text(), "skip": 0 }));
    if let Some(cursor) = again["cursor"].as_str() {
        raw.call("closeCursor", json!({ "session": session, "cursor": cursor }));
        let id = raw.send("fetch", json!({ "session": session, "cursor": cursor, "count": 1 }));
        assert_eq!(error_code(&raw.answer_to(id)), protocol::UNKNOWN_CURSOR, "a closed cursor is gone");
    }
}

#[test]
fn cancel_stops_a_running_statement_and_the_session_goes_on() {
    let (mut raw, init) = Raw::initialized();
    let Some(slow) = slow_text().filter(|_| init.capabilities.server_cancel) else { return };
    let session = raw.session();
    let started = Instant::now();
    let running = raw.send("execute", json!({ "session": session, "statement": slow, "maxRows": 10 }));
    std::thread::sleep(Duration::from_millis(200));
    let cancel = raw.send("cancel", json!({ "session": session }));
    assert!(raw.answer_to(cancel).get("error").is_none(), "cancel is answered");
    let answer = raw.answer_to(running);
    assert!(started.elapsed() < Duration::from_secs(5), "the statement stopped early");
    let data: protocol::ErrorData = serde_json::from_value(answer["error"]["data"].clone()).expect("an error");
    assert_eq!(data.kind, Some(ErrorKind::Cancelled), "{answer}");
    raw.call("ping", json!({ "session": session }));
}

#[test]
fn introspection_answers_the_snapshot_model() {
    let (mut raw, init) = Raw::initialized();
    if !init.capabilities.introspection {
        return;
    }
    let session = raw.session();
    let result = raw.call(
        "introspect",
        json!({ "session": session, "schemas": [], "objects": [], "force": true, "system": false, "largeSchema": 10_000, "outline": false, "fingerprints": {} }),
    );
    let read: protocol::IntrospectResult = serde_json::from_value(result).expect("catalogues of the snapshot model");
    assert!(!read.catalogs.is_empty(), "the connected catalogue is listed");
}

#[test]
fn the_tree_answers_nodes() {
    let (mut raw, _) = Raw::initialized();
    let session = raw.session();
    let result = raw.call("children", json!({ "session": session, "path": [] }));
    let _: protocol::ChildrenResult = serde_json::from_value(result).expect("nodes with path, kind, label");
}

#[test]
fn shutdown_is_answered_and_the_program_exits_when_its_input_closes() {
    let (mut raw, _) = Raw::initialized();
    raw.session();
    raw.call("shutdown", json!({}));
    raw.input = None;
    assert!(raw.exits_within(Duration::from_secs(5)), "the program exits after shutdown");
}

#[test]
fn the_end_of_input_alone_makes_it_exit() {
    let (mut raw, _) = Raw::initialized();
    raw.input = None;
    assert!(raw.exits_within(Duration::from_secs(5)), "the program exits when its input closes");
}

fn declaration(process: ProcessModel) -> DriverDeclaration {
    DriverDeclaration {
        id: driver_id(),
        name: "Conformance".to_owned(),
        icon: None,
        protocol: protocol::VERSION,
        process,
        default_port: None,
        capabilities: Capabilities { models: vec![zen_db::model::DataModel::Relational], ..Capabilities::default() },
        read_verbs: Vec::new(),
        guards_read_only: false,
        args: args(),
        platforms: Default::default(),
    }
}

fn external(process: ProcessModel) -> ExternalDriver {
    ExternalDriver::new(ExternalSpec {
        declaration: declaration(process),
        program: program(),
        sha256: None,
        plugin: None,
    })
}

fn engine_with(driver: &ExternalDriver) -> Engine {
    let timing = Timing { backoff: vec![Duration::ZERO, Duration::from_millis(50)], ..Timing::default() };
    Engine::with_timing(vec![Arc::new(driver.clone()) as Arc<dyn Driver>], "Zen Test", timing)
}

fn source(id: &str) -> DataSource {
    let settings = connect_settings();
    let text = |key: &str| settings[key].as_str().map(str::to_owned);
    DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: driver_id(),
        host: text("host"),
        port: settings["port"].as_u64().and_then(|port| u16::try_from(port).ok()),
        database: text("database"),
        user: text("user"),
        auth: if password().is_some() { AuthKind::Password } else { AuthKind::None },
        settings: settings["settings"].as_object().cloned().unwrap_or_default(),
        ..DataSource::default()
    }
}

fn request(id: &str) -> ConnectRequest {
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    ConnectRequest { source: source(id), kind, password: password().map(Secret::new), stored: None, ssh_secret: None }
}

fn wait(engine: &Engine, op: u64) -> OperationState {
    let state = engine.wait(op, WAIT).unwrap();
    assert_ne!(state.status, OperationStatus::Running);
    state
}

fn ok(engine: &Engine, op: u64) -> OperationState {
    let state = wait(engine, op);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state
}

fn exec(engine: &Engine, id: &str, sql: &str) -> zen_db::value::Execution {
    ok(engine, engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap()).execution.unwrap()
}

fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !check() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_engine_connects_runs_streams_and_reads_the_tree_through_an_external_driver() {
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    ok(&engine, engine.connect(request("one")).unwrap());
    assert_eq!(driver.running(), 1, "a program starts with the first connection");
    assert!(driver.capabilities().language.len() > 1, "the handshake's capabilities replace the manifest's");
    let ran = exec(&engine, "one", &query_text());
    assert!(!ran.columns.is_empty() && !ran.rows.is_empty(), "{ran:?}");
    let statement = QueryStatement {
        text: query_text(),
        start: 0,
        end: query_text().len(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let op = engine.run_query("one", ContextKind::Main, vec![statement], QueryOptions::default()).unwrap();
    until("the query ends", || engine.query(op).is_some_and(|query| query.status != OperationStatus::Running));
    let query = engine.query(op).unwrap();
    assert_eq!(query.status, OperationStatus::Done, "{query:?}");
    let result = engine.results().get(query.statements[0].result.unwrap()).unwrap();
    assert!(!result.is_empty(), "the result has rows");
    let nodes = ok(&engine, engine.children("one", Vec::new()).unwrap());
    assert!(nodes.nodes.is_some());
    ok(&engine, engine.disconnect("one", TransactionPolicy::Rollback).unwrap());
    until("the program stops after the last session", || driver.running() == 0);
}

fn read_result(engine: &Engine, id: &str, text: &str) -> Arc<zen_db::results::ResultSet> {
    let statement = QueryStatement {
        text: text.to_owned(),
        start: 0,
        end: text.len(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let op = engine.run_query(id, ContextKind::Main, vec![statement], QueryOptions::default()).unwrap();
    until("the query ends", || engine.query(op).is_some_and(|query| query.status != OperationStatus::Running));
    let query = engine.query(op).unwrap();
    assert_eq!(query.status, OperationStatus::Done, "{query:?}");
    let result = engine.results().get(query.statements[0].result.unwrap()).unwrap();
    until("the rows are read", || !matches!(result.info().state, zen_db::results::ReadState::Reading));
    result
}

#[test]
fn documents_staged_in_the_grid_are_saved_by_id_without_a_transaction() {
    use zen_db::edit::{self, Staged, document};
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    ok(&engine, engine.connect(request("documents")).unwrap());
    if !edit::edits_documents(driver.capabilities()) {
        return;
    }
    let target = "db.getSiblingDB(\"zen_ide_edits\").getCollection(\"people\")";
    exec(&engine, "documents", &format!("{target}.drop()"));
    exec(
        &engine,
        "documents",
        &format!("{target}.insertMany([{{ _id: 1, name: 'Ada', age: 36 }}, {{ _id: 2, name: 'Bob', age: 40 }}])"),
    );
    let text = format!("{target}.find({{}}).sort({{ _id: 1 }})");
    let set = read_result(&engine, "documents", &text);
    let found = document::collection_of(set.statement_text()).unwrap();
    let editing = document::identify(&set.columns(), &found);
    assert!(editing.editable && editing.documents, "{editing:?}");
    let age = set.columns().iter().position(|column| column.name == "age").unwrap();
    set.stage(0, age, Staged::Value(edit::parse_cell(&editing, "37", "int").unwrap()));
    set.delete_rows(&[1]);
    let added = set.add_row(vec![(1, Staged::Value(Value::Text("Cy".into())))]);
    let planned = edit::plan(&editing, &set.changes());
    assert_eq!(planned.len(), 3, "{planned:?}");
    let state = engine.wait(engine.save_edits(set.id(), planned).unwrap(), WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    let report = state.saved.unwrap();
    assert_eq!((report.updated, report.deleted, report.inserted, report.in_transaction), (1, 1, 1, false));
    assert!(matches!(set.get(added - 1, 0), Value::Other(_)), "the new document's ObjectId is read back");
    let again = read_result(&engine, "documents", &text);
    assert_eq!(again.len(), 2);
    assert_eq!(again.get(0, age), Value::Integer(37));
    set.stage(0, age, Staged::Value(Value::Integer(50)));
    exec(&engine, "documents", &format!("{target}.updateOne({{ _id: 1 }}, {{ $set: {{ age: 99 }} }})"));
    let planned = edit::plan(&editing, &set.changes());
    let state = engine.wait(engine.save_edits(set.id(), planned).unwrap(), WAIT).unwrap();
    assert_eq!(
        state.error.and_then(|error| error.code).as_deref(),
        Some("conflict"),
        "a changed document is a conflict"
    );
    exec(&engine, "documents", "db.getSiblingDB(\"zen_ide_edits\").dropDatabase()");
}

#[test]
fn reference_only_paging_stops_at_the_row_cap_and_reads_on_from_where_it_stopped() {
    if !reference_only() {
        return;
    }
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    ok(&engine, engine.connect(request("paged")).unwrap());
    let text = "SELECT * FROM numbers(25000)".to_owned();
    let statement = QueryStatement { end: text.len(), text, start: 0, client: false, params: Vec::new(), kept: None };
    let op = engine.run_query("paged", ContextKind::Main, vec![statement], QueryOptions::default()).unwrap();
    until("the first page arrives", || {
        engine
            .query(op)
            .is_some_and(|query| query.statements[0].result.is_some() && query.status != OperationStatus::Running)
    });
    let id = engine.query(op).unwrap().statements[0].result.unwrap();
    let result = engine.results().get(id).unwrap();
    let first = result.len();
    assert!(first > 0 && first < 25_000, "reading stops at the row cap: {first}");
    if let Some(more) = engine.fetch_more(id, None).unwrap() {
        wait(&engine, more);
    }
    until("more rows arrive", || engine.results().get(id).is_some_and(|result| result.len() > first));
    let result = engine.results().get(id).unwrap();
    assert_eq!(result.get(first, 0), Value::Integer(i64::try_from(first).unwrap() + 1), "it went on where it stopped");
}

#[test]
fn reference_only_transactions_commit_and_roll_back_through_the_driver() {
    if !reference_only() {
        return;
    }
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    ok(&engine, engine.connect(request("tx")).unwrap());
    exec(&engine, "tx", "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");
    exec(&engine, "tx", "BEGIN");
    exec(&engine, "tx", "INSERT INTO t VALUES (1, 'kept')");
    ok(&engine, engine.end_transaction("tx", ContextKind::Main, true).unwrap());
    exec(&engine, "tx", "BEGIN");
    exec(&engine, "tx", "INSERT INTO t VALUES (2, 'dropped')");
    ok(&engine, engine.end_transaction("tx", ContextKind::Main, false).unwrap());
    let read = exec(&engine, "tx", "SELECT * FROM t");
    assert_eq!(read.rows, vec![vec![Value::Integer(1), Value::Text("kept".into())]]);
    let error = wait(
        &engine,
        engine.execute("tx", ContextKind::Main, "FAIL broken on purpose", ExecOptions::default()).unwrap(),
    )
    .error
    .unwrap();
    assert_eq!((error.kind, error.detail.as_str()), (ErrorKind::Server, "broken on purpose"));
}

#[test]
fn reference_only_cancel_through_the_engine_stops_a_statement() {
    if !reference_only() {
        return;
    }
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    ok(&engine, engine.connect(request("cancel")).unwrap());
    let op = engine.execute("cancel", ContextKind::Main, "SLEEP 10000", ExecOptions::default()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    assert!(engine.cancel(op));
    let state = wait(&engine, op);
    assert_ne!(state.status, OperationStatus::Done);
    assert!(started.elapsed() < Duration::from_secs(5));
    exec(&engine, "cancel", "SELECT 1");
}

#[test]
fn reference_only_introspection_fills_the_snapshot_and_keeps_unchanged_schemas() {
    if !reference_only() {
        return;
    }
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    ok(&engine, engine.connect(request("structure")).unwrap());
    let op = engine
        .introspect("structure", IntrospectRequest::default(), Snapshot::new("structure", driver_id()), None)
        .unwrap();
    ok(&engine, op);
    let snapshot = engine.take_snapshot(op).unwrap();
    let schema = snapshot.schema("main", "main").expect("the reference driver's schema");
    assert!(schema.objects.iter().any(|object| object.name == "people" && !object.columns.is_empty()));
    let again = engine.introspect("structure", IntrospectRequest::default(), snapshot.clone(), None).unwrap();
    let state = ok(&engine, again);
    assert_eq!(state.introspected.map(|read| read.unchanged), Some(1), "an unchanged schema is kept");
    let kept = engine.take_snapshot(again).unwrap();
    assert_eq!(kept.schema("main", "main"), snapshot.schema("main", "main"));
}

#[test]
fn reference_only_a_crashed_program_closes_its_sessions_and_the_next_connect_starts_it_again() {
    if !reference_only() {
        return;
    }
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    ok(&engine, engine.connect(request("crash")).unwrap());
    let state = wait(&engine, engine.execute("crash", ContextKind::Main, "CRASH", ExecOptions::default()).unwrap());
    assert_eq!(state.status, OperationStatus::Failed);
    assert_eq!(state.error.unwrap().kind, ErrorKind::Connection);
    until("the crash is logged", || crashes(&driver) == 1);
    let read = exec(&engine, "crash", "SELECT 1");
    assert_eq!(read.rows, vec![vec![Value::Integer(1)]], "the engine reconnected and a new program serves it");
}

fn crashes(driver: &ExternalDriver) -> usize {
    driver.log().tail(500).0.iter().filter(|line| line.contains("stopped by itself")).count()
}

#[test]
fn reference_only_a_program_that_keeps_crashing_is_not_started_again_for_a_while() {
    if !reference_only() {
        return;
    }
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    for round in 0..zen_db::external::CRASH_LIMIT {
        ok(&engine, engine.connect(request(&format!("loop{round}"))).unwrap());
        wait(
            &engine,
            engine.execute(&format!("loop{round}"), ContextKind::Main, "CRASH", ExecOptions::default()).unwrap(),
        );
        until("the crash is logged", || crashes(&driver) > round);
    }
    let state = wait(&engine, engine.connect(request("after")).unwrap());
    assert_eq!(state.status, OperationStatus::Failed);
    assert!(state.error.unwrap().detail.contains("within a minute"));
}

#[test]
fn reference_only_one_program_per_connection_or_one_for_all() {
    if !reference_only() {
        return;
    }
    let per = external(ProcessModel::PerConnection);
    let engine = engine_with(&per);
    ok(&engine, engine.connect(request("a")).unwrap());
    ok(&engine, engine.connect(request("b")).unwrap());
    exec(&engine, "a", "SELECT 1");
    assert_eq!(per.running(), 2);
    let shared = external(ProcessModel::Shared);
    let engine = engine_with(&shared);
    ok(&engine, engine.connect(request("c")).unwrap());
    ok(&engine, engine.connect(request("d")).unwrap());
    assert_eq!(shared.running(), 1);
    ok(&engine, engine.disconnect("c", TransactionPolicy::Rollback).unwrap());
    assert_eq!(shared.running(), 1, "the other connection still uses it");
    ok(&engine, engine.disconnect("d", TransactionPolicy::Rollback).unwrap());
    until("the shared program stops with its last connection", || shared.running() == 0);
}

#[test]
fn reference_only_a_program_that_does_not_match_its_checksum_never_starts() {
    if !reference_only() {
        return;
    }
    let driver = ExternalDriver::new(ExternalSpec {
        declaration: declaration(ProcessModel::PerConnection),
        program: program(),
        sha256: Some("0".repeat(64)),
        plugin: Some("reference".to_owned()),
    });
    let engine = engine_with(&driver);
    let state = wait(&engine, engine.connect(request("sum")).unwrap());
    assert_eq!(state.status, OperationStatus::Failed);
    assert!(state.error.unwrap().detail.contains("checksum"));
    assert_eq!(driver.running(), 0);
}

#[test]
fn reference_only_a_missing_program_says_it_is_not_installed() {
    if !reference_only() {
        return;
    }
    let driver = ExternalDriver::new(ExternalSpec {
        declaration: declaration(ProcessModel::PerConnection),
        program: PathBuf::from("/nonexistent/zen-driver"),
        sha256: None,
        plugin: None,
    });
    assert!(!driver.info().installed);
    let engine = engine_with(&driver);
    let state = wait(&engine, engine.connect(request("missing")).unwrap());
    assert!(state.error.unwrap().detail.contains("not installed"));
}

#[cfg(unix)]
#[test]
fn reference_only_a_program_that_never_answers_initialize_times_out_and_is_stopped() {
    if !reference_only() {
        return;
    }
    let mut declaration = declaration(ProcessModel::PerConnection);
    declaration.args = vec!["30".to_owned()];
    let driver = ExternalDriver::new(ExternalSpec {
        declaration,
        program: PathBuf::from("/bin/sleep"),
        sha256: None,
        plugin: None,
    })
    .initialize_within(Duration::from_millis(300));
    let engine = engine_with(&driver);
    let state = wait(&engine, engine.connect(request("silent")).unwrap());
    assert_eq!(state.error.unwrap().kind, ErrorKind::Timeout);
    until("the silent program is stopped", || driver.running() == 0);
}

#[test]
fn reference_only_the_password_reaches_the_driver_in_memory_and_never_its_log() {
    if !reference_only() {
        return;
    }
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    let mut asked = request("secret");
    asked.source.auth = AuthKind::Password;
    asked.password = Some(Secret::new("hunter2-zen"));
    ok(&engine, engine.connect(asked).unwrap());
    exec(&engine, "secret", "STDERR the password is hunter2-zen");
    until("the driver's standard error is logged", || {
        driver.log().tail(100).0.iter().any(|line| line.contains("the password is"))
    });
    let (log, _) = driver.log().tail(500);
    assert!(log.iter().all(|line| !line.contains("hunter2-zen")), "{log:?}");
    assert!(log.iter().any(|line| line.contains("the password is ***")), "{log:?}");
    let error = wait(
        &engine,
        engine.execute("secret", ContextKind::Main, "FAIL bad hunter2-zen", ExecOptions::default()).unwrap(),
    )
    .error
    .unwrap();
    assert_eq!(error.detail, "bad ***", "an error the driver sends is redacted too");
}

#[test]
fn reference_only_a_read_only_connection_refuses_changes_through_the_drivers_language() {
    if !reference_only() {
        return;
    }
    let driver = external(ProcessModel::PerConnection);
    let engine = engine_with(&driver);
    let mut asked = request("locked");
    asked.source.read_only = true;
    ok(&engine, engine.connect(asked).unwrap());
    assert_eq!(exec(&engine, "locked", "SELECT * FROM people").rows.len(), 2);
    let refused = wait(
        &engine,
        engine.execute("locked", ContextKind::Main, "DELETE FROM people", ExecOptions::default()).unwrap(),
    );
    assert_eq!(refused.error.unwrap().kind, ErrorKind::Refused);
    assert_eq!(exec(&engine, "locked", "SELECT count(*) FROM people").rows, vec![vec![Value::Integer(2)]]);
}

#[cfg(unix)]
#[test]
fn reference_only_a_failed_connect_is_logged_and_a_query_reports_why() {
    if !reference_only() {
        return;
    }
    let mut declaration = declaration(ProcessModel::PerConnection);
    declaration.args = vec!["ZEN_REFERENCE_PASSWORD=right".to_owned(), program().display().to_string()];
    let driver = ExternalDriver::new(ExternalSpec {
        declaration,
        program: PathBuf::from("/usr/bin/env"),
        sha256: None,
        plugin: None,
    });
    let engine = engine_with(&driver);
    let state = wait(&engine, engine.connect(request("refused")).unwrap());
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    let (log, _) = driver.log().tail(100);
    assert!(log.iter().any(|line| line.contains("connecting refused failed: the password is wrong")), "{log:?}");
    let statement =
        QueryStatement { text: "SELECT 1".to_owned(), start: 0, end: 8, client: false, params: Vec::new(), kept: None };
    let op = engine.run_query("refused", ContextKind::Main, vec![statement], QueryOptions::default()).unwrap();
    until("the query ends", || engine.query(op).is_some_and(|query| query.status != OperationStatus::Running));
    let query = engine.query(op).unwrap();
    let error = query.error.unwrap();
    assert_eq!(error.detail, "the password is wrong", "the connect's reason, not \"not connected\"");
}
