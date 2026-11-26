use std::path::{Path, PathBuf};
use std::time::Duration;

use zen_db::csv_import::{ColumnMapping, CsvFormat, Kind, LoadSettings, OnError, create_statement, truncate_statement};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::engine::{ConnectRequest, Engine, ImportRequest, ImportState, OperationStatus};
use zen_db::history::Provenance;
use zen_db::registry::DataSource;
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(300);

fn connect(source: DataSource, password: Option<&str>) -> Engine {
    let engine = Engine::new(zen_db::drivers(), "Zen Test");
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let password = password.map(|password| Secret::new(password.to_owned()));
    let op = engine.connect(ConnectRequest { source, kind, password, stored: None, ssh_secret: None }).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    engine
}

fn run(engine: &Engine, id: &str, sql: &str) -> Vec<Vec<Value>> {
    let op = engine.execute(id, ContextKind::Main, sql, ExecOptions { max_rows: 1000, timeout: None }).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{sql}: {state:?}");
    state.execution.map(|execution| execution.rows).unwrap_or_default()
}

fn count(engine: &Engine, id: &str, table: &str) -> i64 {
    match run(engine, id, &format!("SELECT count(*) FROM {table}")).first().and_then(|row| row.first()) {
        Some(Value::Integer(count)) => *count,
        Some(Value::Decimal(text)) => text.parse().unwrap(),
        other => panic!("{other:?}"),
    }
}

fn people(dir: &Path, rows: usize, broken: &[(usize, &str)]) -> PathBuf {
    let path = dir.join("people.csv");
    let mut text = String::from("\u{feff}id;name;born;active\n");
    for at in 1..=rows {
        match broken.iter().find(|(row, _)| *row == at) {
            Some((_, line)) => text.push_str(line),
            None => {
                text.push_str(&format!("{at};\"Person \"\"{at}\"\"\";27.10.2026 09:{:02};{}", at % 60, at % 2 == 0))
            }
        }
        text.push('\n');
    }
    std::fs::write(&path, text).unwrap();
    path
}

fn columns() -> Vec<ColumnMapping> {
    [("id", Kind::Integer), ("name", Kind::Text), ("born", Kind::Timestamp), ("active", Kind::Boolean)]
        .iter()
        .enumerate()
        .map(|(at, (name, kind))| ColumnMapping { source: at, target: (*name).to_owned(), kind: *kind, sql_type: None })
        .collect()
}

fn format() -> CsvFormat {
    CsvFormat { delimiter: ';', timestamp_format: Some("dd.MM.yyyy HH:mm".to_owned()), ..CsvFormat::default() }
}

fn import(engine: &Engine, request: ImportRequest) -> ImportState {
    let id = engine.import_csv(request, Provenance::default()).unwrap();
    engine.wait(id, WAIT).unwrap();
    engine.import_state(id).unwrap()
}

fn request(
    id: &str,
    file: &Path,
    dialect: SqlDialect,
    table: &str,
    prelude: Vec<String>,
    load: LoadSettings,
) -> ImportRequest {
    ImportRequest {
        connection: id.to_owned(),
        file: file.to_path_buf(),
        format: format(),
        dialect,
        table: table.to_owned(),
        name: table.to_owned(),
        columns: columns(),
        prelude,
        load,
    }
}

#[test]
fn sqlite_imports_with_a_timestamp_pattern_and_skips_the_rows_that_fail() {
    let Ok(_) = zen_db::sqlite::process::find_program(None) else {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("data.db");
    std::fs::File::create(&database).unwrap();
    let source = DataSource {
        id: "lite".to_owned(),
        name: "lite".to_owned(),
        driver: zen_db::sqlite::ID.to_owned(),
        database: Some(database.display().to_string()),
        ..DataSource::default()
    };
    let engine = connect(source, None);
    run(&engine, "lite", "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, born TIMESTAMP, active BOOLEAN)");
    run(&engine, "lite", "INSERT INTO people VALUES (999999, 'old', NULL, NULL)");
    let file = people(dir.path(), 500, &[(7, "1;dup;27.10.2026 09:00;true"), (9, "9;x;yesterday;true")]);
    let prelude = vec![truncate_statement(SqlDialect::Sqlite, "people")];
    let load = LoadSettings { batch: 64, on_error: OnError::Skip, commit_every: 200, ..LoadSettings::default() };
    let state = import(&engine, request("lite", &file, SqlDialect::Sqlite, "people", prelude, load));
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!((state.rows_read, state.rows_inserted, state.rows_skipped), (500, 498, 2));
    let noted: Vec<(u64, &str)> = state.errors.iter().map(|error| (error.row, error.code.as_str())).collect();
    assert_eq!(noted, [(9, "notADate"), (7, "server")], "{state:?}");
    assert!(state.errors[1].detail.contains("UNIQUE"), "{state:?}");
    assert_eq!(count(&engine, "lite", "people"), 498, "the old row was deleted first");
    let rows = run(&engine, "lite", "SELECT name, born, active FROM people WHERE id = 2");
    assert_eq!(
        rows,
        [vec![
            Value::Text("Person \"2\"".to_owned()),
            Value::Text("2026-10-27 09:02:00".to_owned()),
            Value::Integer(1),
        ]]
    );
}

fn pg_port() -> u16 {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .rev()
        .find_map(|port| port.trim().parse().ok())
        .unwrap_or(54318)
}

#[test]
#[ignore = "needs PostgreSQL in Docker"]
fn postgres_copies_a_hundred_thousand_rows_and_finds_the_row_that_fails() {
    let source = DataSource {
        id: "pg".to_owned(),
        name: "pg".to_owned(),
        driver: "postgresql".to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(pg_port()),
        database: Some("postgres".to_owned()),
        user: Some("postgres".to_owned()),
        ..DataSource::default()
    };
    let engine = connect(source, Some("zen"));
    run(&engine, "pg", "DROP TABLE IF EXISTS public.csv_people");
    let dir = tempfile::tempdir().unwrap();
    let file = people(dir.path(), 100_000, &[]);
    let table = SqlDialect::Postgres.qualified("public", "csv_people");
    let defined: Vec<(String, String)> = columns()
        .iter()
        .map(|column| (column.target.clone(), column.kind.sql_type(SqlDialect::Postgres).to_owned()))
        .collect();
    let mut create = create_statement(SqlDialect::Postgres, &table, &defined);
    create = create.replacen("\"id\" bigint", "\"id\" bigint PRIMARY KEY", 1);
    let load = LoadSettings { batch: 10_000, ..LoadSettings::default() };
    let state = import(&engine, request("pg", &file, SqlDialect::Postgres, &table, vec![create], load));
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert!(state.server_copy, "{state:?}");
    assert_eq!((state.rows_inserted, state.rows_committed), (100_000, 100_000));
    assert_eq!(count(&engine, "pg", &table), 100_000);
    let rows = run(&engine, "pg", "SELECT name, born::text, active FROM public.csv_people WHERE id = 61");
    assert_eq!(
        rows,
        [vec![
            Value::Text("Person \"61\"".to_owned()),
            Value::Text("2026-10-27 09:01:00".to_owned()),
            Value::Bool(false)
        ]]
    );

    let file = people(dir.path(), 30, &[(17, "5;dup;27.10.2026 09:00;true")]);
    let truncate = vec![truncate_statement(SqlDialect::Postgres, &table)];
    let load = LoadSettings { batch: 10, on_error: OnError::Stop, ..LoadSettings::default() };
    let state = import(&engine, request("pg", &file, SqlDialect::Postgres, &table, truncate.clone(), load));
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    assert_eq!((state.errors[0].row, state.errors[0].code.as_str()), (17, "23505"), "{state:?}");
    assert_eq!(count(&engine, "pg", &table), 100_000, "the truncate is rolled back with the rest");

    let load = LoadSettings { batch: 10, on_error: OnError::Skip, ..LoadSettings::default() };
    let state = import(&engine, request("pg", &file, SqlDialect::Postgres, &table, truncate, load));
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!((state.rows_inserted, state.rows_skipped), (29, 1));
    assert_eq!(count(&engine, "pg", &table), 29);
    run(&engine, "pg", "DROP TABLE public.csv_people");
}

#[test]
#[ignore = "needs MySQL in Docker"]
fn mysql_imports_through_inserts_and_finds_the_row_that_fails() {
    let port = std::env::var("ZEN_DB_MYSQL_SERVERS")
        .unwrap_or_else(|_| "mysql:33084".to_owned())
        .split(',')
        .find_map(|pair| pair.trim().strip_prefix("mysql:").and_then(|port| port.parse().ok()))
        .unwrap_or(33084);
    let source = DataSource {
        id: "my".to_owned(),
        name: "my".to_owned(),
        driver: "mysql".to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        user: Some("root".to_owned()),
        ..DataSource::default()
    };
    let engine = connect(source, Some("zen"));
    run(&engine, "my", "CREATE DATABASE IF NOT EXISTS zen_csv");
    run(&engine, "my", "DROP TABLE IF EXISTS zen_csv.people");
    run(
        &engine,
        "my",
        "CREATE TABLE zen_csv.people (id BIGINT PRIMARY KEY, name TEXT, born DATETIME(6), active BOOLEAN)",
    );
    run(&engine, "my", "INSERT INTO zen_csv.people VALUES (999999, 'old', NULL, NULL)");
    let dir = tempfile::tempdir().unwrap();
    let file = people(dir.path(), 2_000, &[(1500, "3;dup;27.10.2026 09:00;true")]);
    let table = SqlDialect::MySql.qualified("zen_csv", "people");
    let truncate = vec![truncate_statement(SqlDialect::MySql, &table)];
    let load = LoadSettings { batch: 500, on_error: OnError::Skip, commit_every: 1000, ..LoadSettings::default() };
    let state = import(&engine, request("my", &file, SqlDialect::MySql, &table, truncate, load));
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert!(!state.server_copy);
    assert_eq!((state.rows_inserted, state.rows_skipped, state.rows_committed), (1999, 1, 1999));
    assert_eq!((state.errors[0].row, state.errors[0].code.as_str()), (1500, "1062"), "{state:?}");
    assert_eq!(count(&engine, "my", &table), 1999, "the old row was truncated and every batch went in");
    run(&engine, "my", "DROP DATABASE zen_csv");
}
