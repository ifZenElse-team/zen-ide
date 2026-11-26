use std::time::{Duration, Instant};

use zen_db::driver::{ContextKind, ErrorKind};
use zen_db::engine::{
    ConnectRequest, Engine, ExplainRun, OperationStatus, QueryOptions, QueryState, QueryStatement, Timing,
};
use zen_db::explain::{ExplainOptions, Measure, Plan, PlanFormat};
use zen_db::history::Provenance;
use zen_db::params::NamedValue;
use zen_db::registry::DataSource;
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(60);

fn pg_ports() -> Vec<u16> {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
}

fn mysql_servers() -> Vec<(&'static str, u16)> {
    std::env::var("ZEN_DB_MYSQL_SERVERS")
        .unwrap_or_else(|_| "mysql:33084,mariadb:33114".to_owned())
        .split(',')
        .filter_map(|pair| {
            let (driver, port) = pair.trim().split_once(':')?;
            let driver = if driver == "mariadb" { "mariadb" } else { "mysql" };
            Some((driver, port.parse().ok()?))
        })
        .collect()
}

fn new_engine() -> Engine {
    Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default())
}

fn connect(engine: &Engine, id: &str, driver: &str, port: u16) {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: driver.to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        database: (driver == "postgresql").then(|| "postgres".to_owned()),
        user: Some(if driver == "postgresql" { "postgres" } else { "root" }.to_owned()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let request =
        ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None };
    let op = engine.connect(request).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn run(engine: &Engine, id: &str, context: &ContextKind, texts: &[&str]) -> QueryState {
    let statements = texts
        .iter()
        .map(|text| QueryStatement {
            text: (*text).to_owned(),
            start: 0,
            end: text.len(),
            client: false,
            params: Vec::new(),
            kept: None,
        })
        .collect();
    let op = engine.run_query(id, context.clone(), statements, QueryOptions::default()).unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            assert_eq!(query.status, OperationStatus::Done, "{query:?}");
            return query;
        }
        assert!(Instant::now() < deadline, "{query:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn count(engine: &Engine, id: &str, context: &ContextKind, table: &str) -> i64 {
    let query = run(engine, id, context, &[&format!("SELECT count(*) FROM {table}")]);
    let result = engine.results().get(query.statements[0].result.unwrap()).unwrap();
    let count = match result.get(0, 0) {
        Value::Integer(count) => count,
        Value::Decimal(text) | Value::Text(text) => text.parse().unwrap(),
        other => panic!("{other:?}"),
    };
    engine.results().remove(result.id());
    count
}

fn explain(
    engine: &Engine,
    target: (&str, &str, &ContextKind),
    statement: &str,
    options: (ExplainOptions, bool),
) -> Result<Plan, zen_db::driver::DbError> {
    explain_bound(engine, target, (statement, Vec::new()), options)
}

fn explain_bound(
    engine: &Engine,
    (id, driver, context): (&str, &str, &ContextKind),
    (statement, params): (&str, Vec<NamedValue>),
    (options, read_only): (ExplainOptions, bool),
) -> Result<Plan, zen_db::driver::DbError> {
    let driver = engine.driver(driver).unwrap();
    let request = driver.explain(statement, &options).unwrap();
    let verb = if request.executes { "EXPLAIN ANALYZE" } else { "EXPLAIN" };
    let run = ExplainRun {
        judged: format!("{verb} {statement}"),
        rollback: request.executes,
        request,
        schema: None,
        read_only,
        timeout: None,
        provenance: Provenance::default(),
        params,
    };
    let op = engine.explain(id, context.clone(), run).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    match state.status {
        OperationStatus::Done => Ok(engine.take_plan(op).expect("a plan")),
        _ => Err(state.error.expect("an error")),
    }
}

fn analyze() -> ExplainOptions {
    ExplainOptions { analyze: true, ..ExplainOptions::default() }
}

fn bound(values: &[(&str, Value)]) -> Vec<NamedValue> {
    values.iter().map(|(name, value)| NamedValue { name: (*name).to_owned(), value: value.clone() }).collect()
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn postgres_explains_a_statement_with_bound_parameters() {
    for port in pg_ports() {
        let engine = new_engine();
        connect(&engine, "pg", "postgresql", port);
        let console = ContextKind::Console("bound".to_owned());
        run(
            &engine,
            "pg",
            &console,
            &[
                "DROP TABLE IF EXISTS explain_bound",
                "CREATE TABLE explain_bound (id int PRIMARY KEY, customer text, total numeric)",
                "INSERT INTO explain_bound SELECT g, 'c' || (g % 10), g FROM generate_series(1, 100) g",
            ],
        );
        let query = "SELECT * FROM explain_bound WHERE id > $1 AND customer = $2 ORDER BY total LIMIT $3";
        let values = bound(&[
            ("from", Value::Integer(5)),
            ("customer", Value::Text("c3".to_owned())),
            ("rows", Value::Integer(2)),
        ]);
        let target = ("pg", "postgresql", &console);
        let plan = explain_bound(&engine, target, (query, values.clone()), (ExplainOptions::default(), false)).unwrap();
        assert_eq!(plan.format, PlanFormat::PostgresJson);
        assert!(plan.nodes.iter().any(|node| node.entity.as_deref() == Some("explain_bound")), "{plan:?}");
        let plan = explain_bound(&engine, target, (query, values), (analyze(), false)).unwrap();
        assert!(plan.analyzed);
        assert_eq!(plan.nodes[0].node, "Limit", "{plan:?}");
        assert_eq!(plan.nodes[0].actual_rows, Some(2.0), "the bound values were used: {plan:?}");
        let raw = explain(&engine, target, query, (ExplainOptions::default(), false)).unwrap_err();
        assert!(raw.detail.contains("parameter"), "without values it is refused: {raw:?}");
        assert_eq!(raw.kind, ErrorKind::Server, "a statement's error, not a lost connection: {raw:?}");
        run(&engine, "pg", &ContextKind::Main, &["DROP TABLE explain_bound"]);
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn mysql_and_mariadb_explain_a_statement_with_bound_parameters() {
    for (driver, port) in mysql_servers() {
        let engine = new_engine();
        connect(&engine, "my", driver, port);
        let console = ContextKind::Console("bound".to_owned());
        run(
            &engine,
            "my",
            &console,
            &[
                "CREATE DATABASE IF NOT EXISTS zen_explain_bound",
                "DROP TABLE IF EXISTS zen_explain_bound.orders",
                "CREATE TABLE zen_explain_bound.orders (id int PRIMARY KEY, customer varchar(10))",
                "INSERT INTO zen_explain_bound.orders VALUES (1, 'a'), (2, 'b'), (3, 'a')",
            ],
        );
        let query = "SELECT * FROM zen_explain_bound.orders WHERE id > ? AND customer = ? LIMIT ?";
        let values =
            bound(&[("?1", Value::Integer(1)), ("?2", Value::Text("a".to_owned())), ("?3", Value::Integer(5))]);
        let plan =
            explain_bound(&engine, ("my", driver, &console), (query, values), (ExplainOptions::default(), false))
                .unwrap();
        assert!(
            plan.nodes.iter().any(|node| node.entity.as_deref().is_some_and(|entity| entity.starts_with("orders"))),
            "{driver}: {plan:?}"
        );
        run(&engine, "my", &ContextKind::Main, &["DROP DATABASE zen_explain_bound"]);
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn postgres_explains_and_analyzes_without_keeping_changes() {
    for port in pg_ports() {
        let engine = new_engine();
        connect(&engine, "pg", "postgresql", port);
        let console = ContextKind::Console("plan".to_owned());
        run(
            &engine,
            "pg",
            &console,
            &[
                "DROP TABLE IF EXISTS explain_orders",
                "CREATE TABLE explain_orders (id int PRIMARY KEY, customer int, total numeric)",
                "INSERT INTO explain_orders SELECT g, g % 10, g FROM generate_series(1, 1000) g",
                "ANALYZE explain_orders",
            ],
        );
        let query = "SELECT customer, sum(total) FROM explain_orders WHERE id > 10 GROUP BY customer ORDER BY 2 DESC;";
        let plan = explain(&engine, ("pg", "postgresql", &console), query, (ExplainOptions::default(), false)).unwrap();
        assert_eq!(plan.format, PlanFormat::PostgresJson);
        assert!(!plan.analyzed);
        assert_eq!(plan.measure, Measure::Cost);
        assert!(plan.nodes.iter().any(|node| node.entity.as_deref() == Some("explain_orders")), "{plan:?}");
        assert!(plan.nodes.iter().any(|node| node.hot));

        let options = ExplainOptions { analyze: true, verbose: true, buffers: true, settings: true, costs: true };
        let plan = explain(
            &engine,
            ("pg", "postgresql", &console),
            "DELETE FROM explain_orders WHERE id <= 10",
            (options, false),
        )
        .unwrap();
        assert!(plan.analyzed);
        assert_eq!(plan.measure, Measure::Time, "{plan:?}");
        assert!(plan.execution_ms.is_some());
        assert_eq!(plan.nodes[0].node, "Delete");
        assert_eq!(
            count(&engine, "pg", &console, "explain_orders"),
            1000,
            "ANALYZE ran inside a rolled-back transaction"
        );
        assert!(engine.open_transactions("pg").is_empty());

        run(&engine, "pg", &console, &["BEGIN", "INSERT INTO explain_orders VALUES (5000, 1, 1)"]);
        let plan =
            explain(&engine, ("pg", "postgresql", &console), "DELETE FROM explain_orders", (analyze(), false)).unwrap();
        assert!(plan.analyzed);
        assert_eq!(count(&engine, "pg", &console, "explain_orders"), 1001, "a savepoint undid only the explain");
        assert_eq!(
            engine.open_transactions("pg"),
            std::slice::from_ref(&console),
            "the person's transaction stays open"
        );
        run(&engine, "pg", &console, &["ROLLBACK"]);

        let failed = explain(&engine, ("pg", "postgresql", &console), "DELETE FROM no_such_table", (analyze(), false))
            .unwrap_err();
        assert_eq!(failed.kind, ErrorKind::Server);
        assert!(engine.open_transactions("pg").is_empty(), "a failed ANALYZE leaves no transaction open");

        let refused = explain(&engine, ("pg", "postgresql", &console), "DELETE FROM explain_orders", (analyze(), true))
            .unwrap_err();
        assert_eq!((refused.kind, refused.code.as_deref()), (ErrorKind::Refused, Some("readOnly")));
        let read = explain(&engine, ("pg", "postgresql", &console), "SELECT 1", (analyze(), true)).unwrap();
        assert!(read.analyzed, "a read is explained with ANALYZE in a read-only session");
        run(&engine, "pg", &ContextKind::Main, &["DROP TABLE explain_orders"]);
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn mysql_and_mariadb_explain_in_json_and_analyze() {
    for (driver, port) in mysql_servers() {
        let engine = new_engine();
        connect(&engine, "my", driver, port);
        let console = ContextKind::Console("plan".to_owned());
        run(
            &engine,
            "my",
            &console,
            &[
                "CREATE DATABASE IF NOT EXISTS zen_explain",
                "DROP TABLE IF EXISTS zen_explain.orders",
                "CREATE TABLE zen_explain.orders (id int PRIMARY KEY, customer int, total decimal(10, 2), KEY (customer))",
                "INSERT INTO zen_explain.orders VALUES (1, 1, 10), (2, 1, 20), (3, 2, 30), (4, 3, 40)",
            ],
        );
        let query = "SELECT customer, sum(total) FROM zen_explain.orders WHERE id > 1 GROUP BY customer";
        let plan = explain(&engine, ("my", driver, &console), query, (ExplainOptions::default(), false)).unwrap();
        assert_eq!(plan.format, PlanFormat::MysqlJson, "{driver}");
        assert_eq!(plan.nodes[0].node, "Query Block", "{driver}: {plan:?}");
        assert!(
            plan.nodes.iter().any(|node| node.entity.as_deref().is_some_and(|entity| entity.starts_with("orders")))
        );
        assert!(plan.total_cost.is_some(), "{driver}: {plan:?}");

        let plan = explain(&engine, ("my", driver, &console), query, (analyze(), false)).unwrap();
        assert!(plan.analyzed, "{driver}");
        assert_eq!(plan.measure, Measure::Time, "{driver}: {plan:?}");
        let expected = if driver == "mariadb" { PlanFormat::MysqlJson } else { PlanFormat::MysqlTree };
        assert_eq!(plan.format, expected);

        if driver == "mariadb" {
            let plan = explain(
                &engine,
                ("my", driver, &console),
                "DELETE FROM zen_explain.orders WHERE id > 2",
                (analyze(), false),
            )
            .unwrap();
            assert!(plan.analyzed);
        }
        assert_eq!(count(&engine, "my", &console, "zen_explain.orders"), 4, "{driver}: ANALYZE was rolled back");
        assert!(engine.open_transactions("my").is_empty());
        run(&engine, "my", &ContextKind::Main, &["DROP DATABASE zen_explain"]);
    }
}

#[test]
fn sqlite_explains_its_query_plan() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plan.db");
    std::fs::File::create(&file).unwrap();
    let engine = new_engine();
    let source = DataSource {
        id: "lite".to_owned(),
        name: "lite".to_owned(),
        driver: zen_db::sqlite::ID.to_owned(),
        database: Some(file.display().to_string()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let op = engine.connect(ConnectRequest { source, kind, password: None, stored: None, ssh_secret: None }).unwrap();
    assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
    let context = ContextKind::Main;
    run(
        &engine,
        "lite",
        &context,
        &[
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer INT, total REAL)",
            "CREATE INDEX orders_customer ON orders (customer)",
        ],
    );
    let plan = explain(
        &engine,
        ("lite", zen_db::sqlite::ID, &context),
        "SELECT * FROM orders o WHERE customer = 3 ORDER BY total",
        (analyze(), false),
    )
    .unwrap();
    assert_eq!(plan.format, PlanFormat::SqliteRows);
    assert!(!plan.analyzed, "SQLite has no ANALYZE: nothing ran");
    let search = plan.nodes.iter().find(|node| node.node == "SEARCH").expect("a search");
    assert_eq!(search.entity.as_deref(), Some("o using orders_customer"), "{plan:?}");
    assert!(plan.raw.starts_with("QUERY PLAN\n"), "{}", plan.raw);
}
