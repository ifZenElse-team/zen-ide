use std::path::Path;
use std::time::{Duration, Instant};

use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::engine::{ConnectRequest, Engine, OperationStatus, SearchRequest, SearchState, TableStatus, Timing};
use zen_db::history::{Provenance, Source};
use zen_db::registry::DataSource;
use zen_db::schema::ObjectKind;
use zen_db::search::ColumnClass;
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(60);

fn engine() -> Engine {
    Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default())
}

fn kind() -> ConnectionType {
    ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap()
}

fn connect(engine: &Engine, request: ConnectRequest) {
    let op = engine.connect(request).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn exec(engine: &Engine, id: &str, sql: &str) {
    let op = engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{sql}: {state:?}");
}

fn request(connection: &str, schema: &str, text: &str, dialect: SqlDialect) -> SearchRequest {
    SearchRequest {
        connection: connection.to_owned(),
        schema: schema.to_owned(),
        text: text.to_owned(),
        case_sensitive: false,
        data: true,
        metadata: false,
        classes: vec![ColumnClass::Text],
        columns: Vec::new(),
        tables: Vec::new(),
        views: false,
        limit: 100,
        table_timeout: Duration::from_secs(20),
        dialect,
    }
}

fn finished(engine: &Engine, id: u64) -> SearchState {
    let deadline = Instant::now() + WAIT;
    loop {
        let state = engine.search_state(id).unwrap();
        if state.status != OperationStatus::Running {
            return state;
        }
        assert!(Instant::now() < deadline, "{state:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn search(engine: &Engine, request: SearchRequest) -> SearchState {
    let id = engine.search(request, Provenance::of(Source::Search)).unwrap();
    let state = finished(engine, id);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state
}

type Found = (String, u64, Vec<(String, u64)>);

fn found(state: &SearchState) -> Vec<Found> {
    state
        .tables
        .iter()
        .filter(|table| table.rows > 0)
        .map(|table| {
            let columns = table.columns.iter().map(|column| (column.name.clone(), column.rows)).collect();
            (table.table.clone(), table.rows, columns)
        })
        .collect()
}

fn columns(list: &[(&str, u64)]) -> Vec<(String, u64)> {
    list.iter().map(|(name, rows)| ((*name).to_owned(), *rows)).collect()
}

fn sqlite_installed() -> bool {
    let found = zen_db::sqlite::process::find_program(None).is_ok();
    if !found {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
    }
    found
}

fn sqlite(file: &Path, id: &str) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: zen_db::sqlite::ID.to_owned(),
        database: Some(file.display().to_string()),
        ..DataSource::default()
    };
    ConnectRequest { source, kind: kind(), password: None, stored: None, ssh_secret: None }
}

#[test]
fn sqlite_tables_are_searched_with_counts_per_column_and_a_limit() {
    if !sqlite_installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("shop.db");
    std::fs::File::create(&file).unwrap();
    let engine = engine();
    connect(&engine, sqlite(&file, "s"));
    exec(&engine, "s", "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, email VARCHAR(80), note)");
    exec(
        &engine,
        "s",
        "INSERT INTO customers (name, email, note) VALUES ('Ann', 'ann@zen.dev', NULL), ('Bob', 'bob@x.org', 'zen fan'), ('Zed', 'z@y.org', 'it''s 50% ZEN')",
    );
    exec(&engine, "s", "CREATE TABLE orders (id INTEGER PRIMARY KEY, total REAL, code TEXT)");
    exec(&engine, "s", "INSERT INTO orders (total, code) VALUES (5, 'A-1'), (7, 'B-2')");
    exec(&engine, "s", "CREATE TABLE \"Odd Name\" (\"a b\" TEXT)");
    exec(&engine, "s", "INSERT INTO \"Odd Name\" VALUES ('zen'), ('zen'), ('zen')");
    exec(&engine, "s", "CREATE VIEW v_customers AS SELECT name, note FROM customers");

    let state = search(&engine, request("s", "main", "zen", SqlDialect::Sqlite));
    assert_eq!(
        found(&state),
        [
            ("Odd Name".to_owned(), 3, columns(&[("a b", 3)])),
            ("customers".to_owned(), 3, columns(&[("email", 1), ("note", 2)])),
        ]
    );
    assert!(state.tables.iter().all(|table| table.kind != ObjectKind::View), "views only when asked");
    let orders = state.tables.iter().find(|table| table.table == "orders").unwrap();
    assert_eq!((orders.status, orders.rows, orders.searched_columns), (TableStatus::Done, 0, 1));
    assert_eq!((state.found, state.rows, state.ended, state.tables.len()), (2, 6, 3, 3));

    let mut exact = request("s", "main", "50%", SqlDialect::Sqlite);
    exact.case_sensitive = true;
    assert_eq!(found(&search(&engine, exact)), [("customers".to_owned(), 1, columns(&[("note", 1)]))]);
    let mut upper = request("s", "main", "ZEN", SqlDialect::Sqlite);
    upper.case_sensitive = true;
    upper.tables = vec!["CUSTOMERS".to_owned()];
    assert_eq!(found(&search(&engine, upper)), [("customers".to_owned(), 1, columns(&[("note", 1)]))]);

    let mut limited = request("s", "main", "zen", SqlDialect::Sqlite);
    limited.limit = 2;
    limited.views = true;
    limited.columns = vec!["A B".to_owned(), "note".to_owned()];
    limited.classes = vec![ColumnClass::Text, ColumnClass::Other];
    let state = search(&engine, limited);
    let odd = state.tables.iter().find(|table| table.table == "Odd Name").unwrap();
    assert_eq!((odd.rows, odd.truncated), (2, true));
    let view = state.tables.iter().find(|table| table.table == "v_customers").unwrap();
    assert_eq!((view.kind, view.rows, view.searched_columns), (ObjectKind::View, 2, 1));
    let customers = state.tables.iter().find(|table| table.table == "customers").unwrap();
    assert_eq!((customers.searched_columns, customers.rows, customers.truncated), (1, 2, false));
    let orders = state.tables.iter().find(|table| table.table == "orders").unwrap();
    assert_eq!(orders.status, TableStatus::Skipped);

    let mut numbers = request("s", "main", "7", SqlDialect::Sqlite);
    numbers.classes = vec![ColumnClass::Number];
    assert_eq!(found(&search(&engine, numbers)), [("orders".to_owned(), 1, columns(&[("total", 1)]))]);

    let mut names = request("s", "main", "MAIL", SqlDialect::Sqlite);
    names.data = false;
    names.metadata = true;
    let state = search(&engine, names);
    let hits: Vec<(&str, bool, &[String])> = state
        .tables
        .iter()
        .filter(|table| table.found())
        .map(|table| (table.table.as_str(), table.name_matches, table.named_columns.as_slice()))
        .collect();
    assert_eq!(hits, [("customers", false, &["email".to_owned()][..])]);
}

#[test]
fn a_slow_table_stops_at_its_deadline_and_the_search_goes_on() {
    if !sqlite_installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("slow.db");
    std::fs::File::create(&file).unwrap();
    let engine = engine();
    connect(&engine, sqlite(&file, "slow"));
    exec(
        &engine,
        "slow",
        "CREATE VIEW a_endless AS WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT 'n' || x AS t FROM c",
    );
    exec(&engine, "slow", "CREATE TABLE b_after (t TEXT)");
    exec(&engine, "slow", "INSERT INTO b_after VALUES ('needle')");
    let mut slow = request("slow", "main", "needle", SqlDialect::Sqlite);
    slow.views = true;
    slow.table_timeout = Duration::from_millis(300);
    let state = search(&engine, slow);
    let endless = state.tables.iter().find(|table| table.table == "a_endless").unwrap();
    assert_eq!((endless.status, endless.truncated), (TableStatus::Timeout, true), "{state:?}");
    assert_eq!(found(&state), [("b_after".to_owned(), 1, columns(&[("t", 1)]))]);

    let mut cancelled = request("slow", "main", "needle", SqlDialect::Sqlite);
    cancelled.views = true;
    let id = engine.search(cancelled, Provenance::of(Source::Search)).unwrap();
    let deadline = Instant::now() + WAIT;
    while engine.search_state(id).unwrap().current.is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(engine.cancel(id));
    let state = finished(&engine, id);
    assert_eq!(state.status, OperationStatus::Cancelled, "{state:?}");
    assert!(state.tables.iter().all(|table| table.status == TableStatus::Cancelled), "{state:?}");
}

fn postgres(id: &str, port: u16) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: "postgresql".to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        database: Some("postgres".to_owned()),
        user: Some("postgres".to_owned()),
        ..DataSource::default()
    };
    ConnectRequest {
        source,
        kind: kind(),
        password: Some(Secret::new("zen".to_owned())),
        stored: None,
        ssh_secret: None,
    }
}

fn pg_ports() -> Vec<u16> {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
}

#[test]
#[ignore = "needs local PostgreSQL servers in Docker"]
fn postgres_tables_are_searched_without_reading_values() {
    for port in pg_ports() {
        let engine = engine();
        let id = format!("pg{port}");
        connect(&engine, postgres(&id, port));
        exec(&engine, &id, "DROP SCHEMA IF EXISTS zen_find CASCADE");
        exec(&engine, &id, "CREATE SCHEMA zen_find");
        exec(
            &engine,
            &id,
            "CREATE TABLE zen_find.people (id serial PRIMARY KEY, name text, \"E-mail\" varchar(80), born date, meta jsonb)",
        );
        exec(
            &engine,
            &id,
            "INSERT INTO zen_find.people (name, \"E-mail\", born, meta) VALUES ('Straße', 'a@zen.dev', '2020-01-02', '{\"k\": \"zen\"}'), ('ZENITH', NULL, NULL, NULL), ('x', 'y', '1999-12-31', NULL)",
        );
        exec(&engine, &id, "CREATE TABLE zen_find.parted (k int, t text) PARTITION BY RANGE (k)");
        exec(&engine, &id, "CREATE TABLE zen_find.parted_1 PARTITION OF zen_find.parted FOR VALUES FROM (0) TO (100)");
        exec(&engine, &id, "INSERT INTO zen_find.parted VALUES (1, 'zen'), (2, 'zen')");
        exec(&engine, &id, "CREATE MATERIALIZED VIEW zen_find.summary AS SELECT name FROM zen_find.people");

        let state = search(&engine, request(&id, "zen_find", "zen", SqlDialect::Postgres));
        assert_eq!(
            found(&state),
            [
                ("parted".to_owned(), 2, columns(&[("t", 2)])),
                ("people".to_owned(), 2, columns(&[("name", 1), ("E-mail", 1)])),
                ("summary".to_owned(), 1, columns(&[("name", 1)])),
            ],
            "partitions are searched through their parent: {state:?}"
        );
        let mut dates = request(&id, "zen_find", "12-31", SqlDialect::Postgres);
        dates.classes = vec![ColumnClass::Temporal];
        assert_eq!(found(&search(&engine, dates)), [("people".to_owned(), 1, columns(&[("born", 1)]))]);
        let mut json = request(&id, "zen_find", "\"k\"", SqlDialect::Postgres);
        json.classes = vec![ColumnClass::Other];
        assert_eq!(found(&search(&engine, json)), [("people".to_owned(), 1, columns(&[("meta", 1)]))]);
        let mut unicode = request(&id, "zen_find", "STRASSE", SqlDialect::Postgres);
        unicode.tables = vec!["people".to_owned()];
        assert!(found(&search(&engine, unicode)).is_empty());
        let mut sharp = request(&id, "zen_find", "STRAßE", SqlDialect::Postgres);
        sharp.tables = vec!["people".to_owned()];
        assert_eq!(found(&search(&engine, sharp)), [("people".to_owned(), 1, columns(&[("name", 1)]))]);

        let mut slow = request(&id, "zen_find", "zen", SqlDialect::Postgres);
        slow.views = true;
        slow.table_timeout = Duration::from_millis(500);
        exec(&engine, &id, "CREATE VIEW zen_find.a_sleepy AS SELECT pg_sleep(30)::text AS t");
        let state = search(&engine, slow);
        let sleepy = state.tables.iter().find(|table| table.table == "a_sleepy").unwrap();
        assert_eq!(sleepy.status, TableStatus::Timeout, "{state:?}");
        assert_eq!(state.found, 3, "{state:?}");
        exec(&engine, &id, "DROP SCHEMA zen_find CASCADE");
    }
}

fn mysql_servers() -> Vec<(String, u16)> {
    std::env::var("ZEN_DB_MYSQL_SERVERS")
        .unwrap_or_else(|_| "mysql:33084,mariadb:33114".to_owned())
        .split(',')
        .filter_map(|server| {
            let (driver, port) = server.trim().split_once(':')?;
            Some((driver.to_owned(), port.parse().ok()?))
        })
        .collect()
}

#[test]
#[ignore = "needs local MySQL and MariaDB servers in Docker"]
fn mysql_and_mariadb_tables_are_searched_in_their_dialect() {
    for (driver, port) in mysql_servers() {
        let engine = engine();
        let id = format!("{driver}{port}");
        let source = DataSource {
            id: id.clone(),
            name: id.clone(),
            driver: driver.clone(),
            host: Some("127.0.0.1".to_owned()),
            port: Some(port),
            user: Some("root".to_owned()),
            ..DataSource::default()
        };
        let request_of = ConnectRequest {
            source,
            kind: kind(),
            password: Some(Secret::new("zen".to_owned())),
            stored: None,
            ssh_secret: None,
        };
        connect(&engine, request_of);
        let dialect = if driver == "mariadb" { SqlDialect::MariaDb } else { SqlDialect::MySql };
        exec(&engine, &id, "DROP DATABASE IF EXISTS zen_find");
        exec(&engine, &id, "CREATE DATABASE zen_find");
        exec(
            &engine,
            &id,
            "CREATE TABLE zen_find.`order` (id INT PRIMARY KEY, note VARCHAR(40), raw VARBINARY(10), kind ENUM('Zen', 'other'))",
        );
        exec(
            &engine,
            &id,
            "INSERT INTO zen_find.`order` VALUES (1, 'a\\\\zen', NULL, 'Zen'), (2, 'ZEN', 'zen', 'other'), (3, NULL, NULL, 'other')",
        );
        let state = search(&engine, request(&id, "zen_find", "zen", dialect));
        assert_eq!(found(&state), [("order".to_owned(), 2, columns(&[("note", 2), ("kind", 1)]))], "{driver}");
        let mut exact = request(&id, "zen_find", "ZEN", dialect);
        exact.case_sensitive = true;
        assert_eq!(found(&search(&engine, exact)), [("order".to_owned(), 1, columns(&[("note", 1)]))], "{driver}");
        let mut slash = request(&id, "zen_find", "a\\zen", dialect);
        slash.case_sensitive = true;
        assert_eq!(found(&search(&engine, slash)), [("order".to_owned(), 1, columns(&[("note", 1)]))], "{driver}");
        exec(&engine, &id, "DROP DATABASE zen_find");
    }
}

#[test]
fn nothing_is_searched_without_a_text_or_a_target() {
    let engine = engine();
    let mut empty = request("none", "main", "", SqlDialect::Sqlite);
    assert_eq!(
        engine.search(empty.clone(), Provenance::of(Source::Search)).unwrap_err().code.as_deref(),
        Some("noText")
    );
    empty.text = "x".to_owned();
    empty.data = false;
    assert_eq!(
        engine.search(empty.clone(), Provenance::of(Source::Search)).unwrap_err().code.as_deref(),
        Some("nothingToSearch")
    );
    empty.data = true;
    let id = engine.search(empty, Provenance::of(Source::Search)).unwrap();
    let state = finished(&engine, id);
    assert_eq!(state.status, OperationStatus::Failed, "a connection that is not connected fails the search");
}
