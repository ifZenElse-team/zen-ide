use std::path::Path;
use std::time::Duration;

use zen_db::compare::data::data_script;
use zen_db::compare::{Change, CompareOptions, ScriptOptions, compare, sync_script};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::engine::{
    ComparePass, ConnectRequest, DataCompareRequest, DataSide, Engine, OperationState, OperationStatus, Timing,
};
use zen_db::registry::DataSource;
use zen_db::schema::{DdlOptions, IntrospectRequest, LARGE_SCHEMA, Object, Schema, Snapshot};
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(60);

fn engine() -> Engine {
    Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default())
}

fn connect(engine: &Engine, source: DataSource, password: Option<&str>) {
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let request = ConnectRequest {
        source,
        kind,
        password: password.map(|password| Secret::new(password.to_owned())),
        stored: None,
        ssh_secret: None,
    };
    ok(engine, engine.connect(request).unwrap());
}

fn ok(engine: &Engine, op: u64) -> OperationState {
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state
}

fn exec(engine: &Engine, id: &str, sql: &str) {
    ok(engine, engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap());
}

fn run_script(engine: &Engine, id: &str, language: &str, script: &str) {
    for statement in zen_db::sql::split(script, zen_db::sql::Dialect::of_language(language)) {
        if !statement.client {
            exec(engine, id, &script[statement.start..statement.end]);
        }
    }
}

fn introspect(engine: &Engine, id: &str, driver: &str, schemas: &[&str]) -> Snapshot {
    let request = IntrospectRequest {
        schemas: schemas.iter().map(|schema| (*schema).to_owned()).collect(),
        large_schema: LARGE_SCHEMA,
        force: true,
        ..IntrospectRequest::default()
    };
    let op = engine.introspect(id, request, Snapshot::new(id, driver), None).unwrap();
    ok(engine, op);
    engine.take_snapshot(op).unwrap()
}

fn schema<'a>(snapshot: &'a Snapshot, name: &str) -> &'a Schema {
    snapshot
        .catalogs
        .iter()
        .flat_map(|catalog| &catalog.schemas)
        .find(|schema| schema.name == name)
        .unwrap_or_else(|| panic!("no schema {name} in {snapshot:?}"))
}

fn catalog_of(snapshot: &Snapshot, name: &str) -> String {
    snapshot
        .catalogs
        .iter()
        .find(|catalog| catalog.schemas.iter().any(|schema| schema.name == name))
        .map(|catalog| catalog.name.clone())
        .unwrap()
}

fn script_for(
    engine: &Engine,
    driver: &str,
    (source, target): (&Snapshot, &Snapshot),
    schemas: (&str, &str),
) -> String {
    let (from, to) = (schema(source, schemas.0), schema(target, schemas.1));
    let diff = compare(from, to, &CompareOptions::default());
    let driver_impl = engine.driver(driver).unwrap();
    let catalog = catalog_of(target, schemas.1);
    let dialect = SqlDialect::of_language(&driver_impl.capabilities().language).unwrap();
    let create = |schema: &Schema, object: &Object| {
        let mut snapshot = Snapshot::new("compare", driver);
        snapshot.catalogs.push(zen_db::schema::Catalog {
            name: catalog.clone(),
            current: true,
            schemas_read: true,
            schemas: vec![schema.clone()],
            ..zen_db::schema::Catalog::default()
        });
        let path = vec![catalog.clone(), schema.name.clone(), object.kind.folder().to_owned(), object.key()];
        let mut found = schema.clone();
        if let Some(slot) =
            found.objects.iter_mut().find(|known| known.kind == object.kind && known.key() == object.key())
        {
            *slot = object.clone();
        }
        snapshot.catalogs[0].schemas = vec![found];
        driver_impl.ddl(&snapshot, &path, &DdlOptions { nested: true, ..DdlOptions::default() }).ok()
    };
    let describe = |note: &zen_db::compare::Note| format!("{:?} {} {}", note.kind, note.object, note.part);
    sync_script(&diff, (from, to), dialect, &create, &describe, &ScriptOptions { drop: true }).text
}

fn compare_data(
    engine: &Engine,
    source: DataSide,
    target: DataSide,
) -> (zen_db::engine::DataCompareState, Vec<zen_db::compare::data::RowDiff>) {
    let request = DataCompareRequest {
        source,
        target,
        key: vec!["id".to_owned()],
        columns: vec!["name".to_owned(), "qty".to_owned()],
        page: 2,
        kept: 100,
    };
    let op = engine.compare_data(request).unwrap();
    ok(engine, op);
    let state = engine.data_compare_state(op).unwrap();
    assert_eq!((state.status, state.pass), (OperationStatus::Done, ComparePass::Done), "{state:?}");
    (state, engine.data_compare_rows(op).unwrap())
}

fn check_rows(state: &zen_db::engine::DataCompareState, rows: &[zen_db::compare::data::RowDiff]) {
    let counts = state.counts;
    assert_eq!(
        (counts.source_rows, counts.target_rows, counts.same, counts.changed, counts.added, counts.removed),
        (5, 4, 2, 1, 2, 1),
        "{state:?}"
    );
    let changes: Vec<(Change, Value)> = rows.iter().map(|row| (row.change, row.key[0].clone())).collect();
    assert!(changes.contains(&(Change::Changed, Value::Integer(2))), "{changes:?}");
    assert!(changes.contains(&(Change::Added, Value::Integer(4))), "{changes:?}");
    assert!(changes.contains(&(Change::Added, Value::Integer(5))), "{changes:?}");
    assert!(changes.contains(&(Change::Removed, Value::Integer(9))), "{changes:?}");
    let changed = rows.iter().find(|row| row.change == Change::Changed).unwrap();
    assert_eq!(changed.columns, vec!["qty".to_owned()]);
}

const ROWS_SOURCE: &str =
    "INSERT INTO items (id, name, qty) VALUES (1, 'a', 1), (2, 'b', 2), (3, 'c', 3), (4, 'd', 4), (5, 'e', NULL)";
const ROWS_TARGET: &str =
    "INSERT INTO items (id, name, qty) VALUES (1, 'a', 1), (2, 'b', 20), (3, 'c', 3), (9, 'z', 9)";

fn sqlite_installed() -> bool {
    let found = zen_db::sqlite::process::find_program(None).is_ok();
    if !found {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
    }
    found
}

fn sqlite_source(id: &str, file: &Path) -> DataSource {
    DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: zen_db::sqlite::ID.to_owned(),
        database: Some(file.display().to_string()),
        ..DataSource::default()
    }
}

#[test]
fn sqlite_schemas_and_rows_compare_and_the_scripts_make_the_target_like_the_source() {
    if !sqlite_installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.db"), dir.path().join("b.db"));
    std::fs::File::create(&a).unwrap();
    std::fs::File::create(&b).unwrap();
    let engine = engine();
    connect(&engine, sqlite_source("a", &a), None);
    connect(&engine, sqlite_source("b", &b), None);
    exec(&engine, "a", "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER, note TEXT)");
    exec(&engine, "a", "CREATE INDEX items_name ON items (name)");
    exec(&engine, "a", "CREATE TABLE extra (id INTEGER PRIMARY KEY)");
    exec(&engine, "b", "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER)");
    exec(&engine, "b", "CREATE TABLE old (id INTEGER)");
    exec(&engine, "a", ROWS_SOURCE);
    exec(&engine, "b", ROWS_TARGET);
    let (from, to) =
        (introspect(&engine, "a", zen_db::sqlite::ID, &[]), introspect(&engine, "b", zen_db::sqlite::ID, &[]));
    let diff = compare(schema(&from, "main"), schema(&to, "main"), &CompareOptions::default());
    let summary: Vec<(&str, Change)> =
        diff.objects.iter().map(|object| (object.name.as_str(), object.change)).collect();
    assert_eq!(summary, vec![("extra", Change::Added), ("items", Change::Changed), ("old", Change::Removed)]);
    let script = script_for(&engine, zen_db::sqlite::ID, (&from, &to), ("main", "main"));
    run_script(&engine, "b", "sql:sqlite", &script);
    let again = introspect(&engine, "b", zen_db::sqlite::ID, &[]);
    let after = compare(schema(&from, "main"), schema(&again, "main"), &CompareOptions::default());
    assert!(after.objects.is_empty(), "the script made the target like the source: {script}\n{after:?}");

    let side =
        |id: &str| DataSide { connection: id.to_owned(), table: "items".to_owned(), dialect: SqlDialect::Sqlite };
    let (state, rows) = compare_data(&engine, side("a"), side("b"));
    check_rows(&state, &rows);
    let data = data_script(SqlDialect::Sqlite, "items", (&state.key, &state.columns), &rows, true);
    run_script(&engine, "b", "sql:sqlite", &data.text);
    let (state, rows) = compare_data(&engine, side("a"), side("b"));
    assert!(rows.is_empty(), "{state:?} {rows:?}");
    assert_eq!(state.counts.same, 5);
}

fn pg_port() -> u16 {
    std::env::var("ZEN_DB_PG_PORTS")
        .ok()
        .and_then(|ports| ports.split(',').next_back().and_then(|port| port.trim().parse().ok()))
        .unwrap_or(54318)
}

fn pg_source(id: &str) -> DataSource {
    DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: "postgresql".to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(pg_port()),
        database: Some("postgres".to_owned()),
        user: Some("postgres".to_owned()),
        ..DataSource::default()
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn postgres_schemas_compare_and_the_sync_script_makes_the_target_like_the_source() {
    let engine = engine();
    connect(&engine, pg_source("pg"), Some("zen"));
    for statement in [
        "DROP SCHEMA IF EXISTS cmp_a CASCADE",
        "DROP SCHEMA IF EXISTS cmp_b CASCADE",
        "CREATE SCHEMA cmp_a",
        "CREATE SCHEMA cmp_b",
        "CREATE TABLE cmp_a.customers (id integer PRIMARY KEY, name text NOT NULL)",
        "CREATE TABLE cmp_a.items (id integer PRIMARY KEY, name varchar(80) NOT NULL, qty integer DEFAULT 0, customer integer REFERENCES cmp_a.customers(id))",
        "CREATE INDEX items_name ON cmp_a.items (name)",
        "COMMENT ON TABLE cmp_a.items IS 'Things'",
        "CREATE VIEW cmp_a.big AS SELECT id FROM cmp_a.items WHERE qty > 10",
        "CREATE TABLE cmp_b.items (id integer PRIMARY KEY, name varchar(40), qty integer, gone text)",
        "CREATE TABLE cmp_b.old (id integer)",
        "CREATE VIEW cmp_b.big AS SELECT id FROM cmp_b.items",
        "INSERT INTO cmp_a.customers VALUES (1, 'x')",
        "INSERT INTO cmp_a.items (id, name, qty) VALUES (1, 'a', 1), (2, 'b', 2), (3, 'c', 3), (4, 'd', 4), (5, 'e', NULL)",
        "INSERT INTO cmp_b.items (id, name, qty) VALUES (1, 'a', 1), (2, 'b', 20), (3, 'c', 3), (9, 'z', 9)",
    ] {
        exec(&engine, "pg", statement);
    }
    let snapshot = introspect(&engine, "pg", "postgresql", &["cmp_a", "cmp_b"]);
    let diff = compare(schema(&snapshot, "cmp_a"), schema(&snapshot, "cmp_b"), &CompareOptions::default());
    let summary: Vec<(&str, Change)> =
        diff.objects.iter().map(|object| (object.name.as_str(), object.change)).collect();
    assert_eq!(
        summary,
        vec![
            ("customers", Change::Added),
            ("items", Change::Changed),
            ("old", Change::Removed),
            ("big", Change::Changed)
        ]
    );
    let side =
        |table: &str| DataSide { connection: "pg".to_owned(), table: table.to_owned(), dialect: SqlDialect::Postgres };
    let (state, rows) = compare_data(&engine, side("cmp_a.items"), side("cmp_b.items"));
    check_rows(&state, &rows);
    let script = script_for(&engine, "postgresql", (&snapshot, &snapshot), ("cmp_a", "cmp_b"));
    run_script(&engine, "pg", "sql:postgres", &script);
    let again = introspect(&engine, "pg", "postgresql", &["cmp_a", "cmp_b"]);
    let after = compare(schema(&again, "cmp_a"), schema(&again, "cmp_b"), &CompareOptions::default());
    assert!(after.objects.is_empty(), "the script made the target like the source:\n{script}\n{after:#?}");
    let data = data_script(SqlDialect::Postgres, "cmp_b.items", (&state.key, &state.columns), &rows, true);
    run_script(&engine, "pg", "sql:postgres", &data.text);
    let (state, rows) = compare_data(&engine, side("cmp_a.items"), side("cmp_b.items"));
    assert!(rows.is_empty(), "{state:?} {rows:?}");
    run_script(&engine, "pg", "sql:postgres", "DROP SCHEMA cmp_a CASCADE; DROP SCHEMA cmp_b CASCADE");
}

fn mysql_port() -> u16 {
    std::env::var("ZEN_DB_MYSQL_SERVERS")
        .ok()
        .and_then(|servers| {
            servers.split(',').find_map(|pair| pair.trim().strip_prefix("mysql:").and_then(|port| port.parse().ok()))
        })
        .unwrap_or(33084)
}

#[test]
#[ignore = "needs MySQL in Docker (docs/development.md)"]
fn mysql_schemas_compare_and_the_sync_script_makes_the_target_like_the_source() {
    let engine = engine();
    let source = DataSource {
        id: "my".to_owned(),
        name: "my".to_owned(),
        driver: "mysql".to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(mysql_port()),
        user: Some("root".to_owned()),
        ..DataSource::default()
    };
    connect(&engine, source, Some("zen"));
    for statement in [
        "DROP DATABASE IF EXISTS cmp_a",
        "DROP DATABASE IF EXISTS cmp_b",
        "CREATE DATABASE cmp_a",
        "CREATE DATABASE cmp_b",
        "CREATE TABLE cmp_a.items (id int PRIMARY KEY, name varchar(80) NOT NULL, qty int DEFAULT 0, note text)",
        "CREATE INDEX items_name ON cmp_a.items (name)",
        "CREATE TABLE cmp_a.extra (id int PRIMARY KEY)",
        "CREATE TABLE cmp_b.items (id int PRIMARY KEY, name varchar(40), qty int, gone text)",
        "INSERT INTO cmp_a.items (id, name, qty) VALUES (1, 'a', 1), (2, 'b', 2), (3, 'c', 3), (4, 'd', 4), (5, 'e', NULL)",
        "INSERT INTO cmp_b.items (id, name, qty) VALUES (1, 'a', 1), (2, 'b', 20), (3, 'c', 3), (9, 'z', 9)",
    ] {
        exec(&engine, "my", statement);
    }
    let snapshot = introspect(&engine, "my", "mysql", &["cmp_a", "cmp_b"]);
    let diff = compare(schema(&snapshot, "cmp_a"), schema(&snapshot, "cmp_b"), &CompareOptions::default());
    let summary: Vec<(&str, Change)> =
        diff.objects.iter().map(|object| (object.name.as_str(), object.change)).collect();
    assert_eq!(summary, vec![("extra", Change::Added), ("items", Change::Changed)]);
    let side =
        |table: &str| DataSide { connection: "my".to_owned(), table: table.to_owned(), dialect: SqlDialect::MySql };
    let (state, rows) = compare_data(&engine, side("cmp_a.items"), side("cmp_b.items"));
    check_rows(&state, &rows);
    let script = script_for(&engine, "mysql", (&snapshot, &snapshot), ("cmp_a", "cmp_b"));
    run_script(&engine, "my", "sql:mysql", &script);
    let again = introspect(&engine, "my", "mysql", &["cmp_a", "cmp_b"]);
    let after = compare(schema(&again, "cmp_a"), schema(&again, "cmp_b"), &CompareOptions::default());
    assert!(after.objects.is_empty(), "the script made the target like the source:\n{script}\n{after:#?}");
    exec(&engine, "my", "DROP DATABASE cmp_a");
    exec(&engine, "my", "DROP DATABASE cmp_b");
}
