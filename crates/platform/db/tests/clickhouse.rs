use std::time::{Duration, Instant};

use zen_db::driver::{ContextKind, ErrorKind, ExecOptions};
use zen_db::engine::{
    ConnectRequest, Engine, OperationState, OperationStatus, QueryOptions, QueryStatement, StatementStatus, Timing,
};
use zen_db::model::NodeKind;
use zen_db::registry::DataSource;
use zen_db::types::{ConnectionType, DEVELOPMENT, PRODUCTION};
use zen_db::value::{Execution, Typed, Value};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(30);

fn port() -> u16 {
    std::env::var("ZEN_DB_CLICKHOUSE_PORT").ok().and_then(|port| port.parse().ok()).unwrap_or(18123)
}

fn new_engine() -> Engine {
    Engine::with_timing(
        zen_db::drivers(),
        "Zen Test",
        Timing { backoff: [0, 100, 300].map(Duration::from_millis).to_vec(), ..Timing::default() },
    )
}

fn request(id: &str, kind: &str, read_only: bool, password: &str) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: "clickhouse".to_owned(),
        connection_type: kind.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port()),
        database: Some("default".to_owned()),
        user: Some("zen".to_owned()),
        read_only,
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == kind).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new(password.to_owned())), stored: None, ssh_secret: None }
}

fn done(engine: &Engine, op: u64) -> OperationState {
    let state = engine.wait(op, WAIT).unwrap();
    assert_ne!(state.status, OperationStatus::Running);
    state
}

fn ok(engine: &Engine, op: u64) -> OperationState {
    let state = done(engine, op);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state
}

fn run(engine: &Engine, id: &str, sql: &str) -> Execution {
    ok(engine, engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap()).execution.unwrap()
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

#[test]
#[ignore = "needs ClickHouse in Docker (docs/development.md)"]
fn tables_values_streaming_cancel_and_guards() {
    let engine = new_engine();
    let id = "ch";
    let tested = ok(&engine, engine.test(request("probe", DEVELOPMENT, false, "zen")).unwrap()).test.unwrap();
    assert!(tested.server_version.unwrap().starts_with("ClickHouse 25."));
    let wrong = done(&engine, engine.test(request("wrong", DEVELOPMENT, false, "nope")).unwrap());
    assert_eq!(wrong.error.unwrap().kind, ErrorKind::Authentication);

    ok(&engine, engine.connect(request(id, DEVELOPMENT, false, "zen")).unwrap());
    run(&engine, id, "DROP DATABASE IF EXISTS zen_test");
    run(&engine, id, "CREATE DATABASE zen_test COMMENT 'tests'");
    run(
        &engine,
        id,
        "CREATE TABLE zen_test.events (id UInt64, price Decimal(10, 2), at DateTime64(3, 'UTC'), tags Array(String), \
         ok Bool, note Nullable(String), attrs Map(String, Int32), big UInt64) ENGINE = MergeTree ORDER BY id",
    );
    run(&engine, id, "CREATE VIEW zen_test.recent AS SELECT * FROM zen_test.events WHERE id > 1");
    run(
        &engine,
        id,
        "INSERT INTO zen_test.events VALUES (1, 12.30, '2026-10-26 10:00:00.250', ['a', 'b'], true, NULL, {'k': 1}, \
         18446744073709551615)",
    );
    let got = run(&engine, id, "SELECT * FROM zen_test.events");
    assert_eq!(
        got.rows[0],
        vec![
            Value::Integer(1),
            Value::Decimal("12.30".into()),
            Value::Temporal(Typed { text: "2026-10-26 10:00:00.250".into(), type_name: "DateTime64(3, 'UTC')".into() }),
            Value::Array(vec![Value::Text("a".into()), Value::Text("b".into())]),
            Value::Bool(true),
            Value::Null,
            Value::Map(vec![("k".into(), Value::Integer(1))]),
            Value::Decimal("18446744073709551615".into()),
        ]
    );
    let failed =
        done(&engine, engine.execute(id, ContextKind::Main, "SELECT FROM nowhere", ExecOptions::default()).unwrap());
    let error = failed.error.unwrap();
    assert_eq!((error.kind, error.code.as_deref()), (ErrorKind::Server, Some("62")));

    let databases = ok(&engine, engine.children(id, Vec::new()).unwrap()).nodes.unwrap();
    assert!(databases.iter().any(|node| node.label == "zen_test" && node.kind == NodeKind::Catalog));
    assert!(databases.iter().any(|node| node.label == "system" && node.system));
    let tables = ok(&engine, engine.children(id, path(&["zen_test", "tables"])).unwrap()).nodes.unwrap();
    assert_eq!(
        (tables[0].label.as_str(), tables[0].detail.as_deref(), tables[0].rows),
        ("events", Some("MergeTree"), Some(1))
    );
    let views = ok(&engine, engine.children(id, path(&["zen_test", "views"])).unwrap()).nodes.unwrap();
    assert_eq!((views[0].label.as_str(), &views[0].kind), ("recent", &NodeKind::View));
    let columns = ok(&engine, engine.children(id, path(&["zen_test", "tables", "events"])).unwrap()).nodes.unwrap();
    assert_eq!((columns[1].label.as_str(), columns[1].detail.as_deref()), ("price", Some("Decimal(10, 2)")));

    let console = ContextKind::Console("c".to_owned());
    let text = "SELECT number FROM numbers(100000)";
    let statement = QueryStatement {
        text: text.to_owned(),
        start: 0,
        end: text.len(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let query = engine.run_query(id, console.clone(), vec![statement], QueryOptions::default()).unwrap();
    let started = Instant::now();
    let ran = loop {
        let ran = engine.query(query).unwrap();
        if ran.status != OperationStatus::Running || started.elapsed() > WAIT {
            break ran;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(ran.statements[0].status, StatementStatus::Done);
    let result = engine.results().get(ran.statements[0].result.unwrap()).unwrap();
    assert!(result.len() >= 200 && result.len() <= 10_000, "{}", result.len());
    assert_eq!(result.get(5, 0), Value::Integer(5));

    run(&engine, id, "SET max_threads = 1");
    let setting = run(&engine, id, "SELECT getSetting('max_threads')");
    assert_eq!(setting.rows[0][0], Value::Integer(1), "SET lasts in the HTTP session");

    let long = engine
        .execute(id, console.clone(), "SELECT count() FROM numbers_mt(1000000000000)", ExecOptions::default())
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let asked = Instant::now();
    assert!(engine.cancel(long));
    let cancelled = done(&engine, long);
    assert_eq!(cancelled.status, OperationStatus::Cancelled, "{cancelled:?}");
    assert!(asked.elapsed() < Duration::from_secs(3));
    let running = run(
        &engine,
        id,
        "SELECT count() FROM system.processes WHERE query LIKE '%numbers_mt(1000000000000)%' AND query NOT LIKE '%system.processes%'",
    );
    assert_eq!(running.rows[0][0], Value::Integer(0), "the server stopped the query");

    ok(&engine, engine.connect(request("locked", DEVELOPMENT, true, "zen")).unwrap());
    let refused = done(
        &engine,
        engine
            .execute("locked", ContextKind::Main, "INSERT INTO zen_test.events (id) VALUES (2)", ExecOptions::default())
            .unwrap(),
    );
    assert_eq!(refused.error.unwrap().code.as_deref(), Some("164"));
    ok(&engine, engine.connect(request("prod", PRODUCTION, false, "zen")).unwrap());
    let limit = ok(
        &engine,
        engine
            .execute("prod", ContextKind::Main, "SELECT getSetting('max_execution_time')", ExecOptions::default())
            .unwrap(),
    );
    assert_eq!(limit.execution.unwrap().rows[0][0].display().as_deref(), Some("300"));
    run(&engine, id, "DROP DATABASE zen_test");
}
