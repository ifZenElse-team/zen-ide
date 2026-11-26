use std::time::Duration;

use zen_db::csv_import::LoadSettings;
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::engine::{
    ConnectRequest, Engine, ExportItem, OperationStatus, Output, TransferRequest, TransferState, TransferTable,
};
use zen_db::export::{Format, Settings};
use zen_db::history::Provenance;
use zen_db::registry::DataSource;
use zen_db::transfer::{NewColumn, SqlType, create_table, select_statement, target_type};
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(300);
const ROWS: i64 = 50_000;

fn connect(engine: &Engine, source: DataSource) {
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let password = Some(Secret::new("zen".to_owned()));
    let op = engine.connect(ConnectRequest { source, kind, password, stored: None, ssh_secret: None }).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn run(engine: &Engine, id: &str, sql: &str) -> Vec<Vec<Value>> {
    let op = engine.execute(id, ContextKind::Main, sql, ExecOptions { max_rows: 1000, timeout: None }).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{sql}: {state:?}");
    state.execution.map(|execution| execution.rows).unwrap_or_default()
}

fn servers() -> Engine {
    let pg_port = std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .rev()
        .find_map(|port| port.trim().parse().ok())
        .unwrap_or(54318);
    let my_port = std::env::var("ZEN_DB_MYSQL_SERVERS")
        .unwrap_or_else(|_| "mysql:33084".to_owned())
        .split(',')
        .find_map(|pair| pair.trim().strip_prefix("mysql:").and_then(|port| port.parse().ok()))
        .unwrap_or(33084);
    let engine = Engine::new(zen_db::drivers(), "Zen Test");
    connect(
        &engine,
        DataSource {
            id: "pg".to_owned(),
            name: "pg".to_owned(),
            driver: "postgresql".to_owned(),
            host: Some("127.0.0.1".to_owned()),
            port: Some(pg_port),
            database: Some("postgres".to_owned()),
            user: Some("postgres".to_owned()),
            ..DataSource::default()
        },
    );
    connect(
        &engine,
        DataSource {
            id: "my".to_owned(),
            name: "my".to_owned(),
            driver: "mysql".to_owned(),
            host: Some("127.0.0.1".to_owned()),
            port: Some(my_port),
            user: Some("root".to_owned()),
            ..DataSource::default()
        },
    );
    engine
}

const SOURCE: [(&str, &str, bool); 9] = [
    ("id", "bigint", false),
    ("name", "character varying(40)", true),
    ("price", "numeric(10,2)", true),
    ("active", "boolean", true),
    ("seen", "timestamp with time zone", true),
    ("born", "date", true),
    ("picture", "bytea", true),
    ("doc", "jsonb", true),
    ("ref", "uuid", true),
];

fn names() -> Vec<String> {
    SOURCE.iter().map(|(name, _, _)| (*name).to_owned()).collect()
}

fn moved(
    engine: &Engine,
    (source, target): (&str, &str),
    (from, to): (SqlDialect, SqlDialect),
    (read, write): (&str, &str),
    types: &[(&str, &str, bool)],
) -> TransferState {
    let columns: Vec<NewColumn> = types
        .iter()
        .map(|(name, kind, nullable)| NewColumn {
            name: (*name).to_owned(),
            sql_type: target_type(kind, from, to, *nullable),
            nullable: *nullable,
        })
        .collect();
    let create = create_table(to, write, &columns, &["id".to_owned()]);
    let table = TransferTable {
        name: format!("{read} → {write}"),
        select: select_statement(from, read, &names()),
        table: write.to_owned(),
        columns: names(),
        types: columns.iter().map(|column| SqlType::parse(&column.sql_type)).collect(),
        prelude: vec![create],
    };
    let load = LoadSettings { batch: 5_000, commit_every: 20_000, ..LoadSettings::default() };
    let request = TransferRequest {
        source: source.to_owned(),
        target: target.to_owned(),
        dialect: to,
        tables: vec![table],
        load,
    };
    let id = engine.transfer(request, Provenance::default()).unwrap();
    engine.wait(id, WAIT).unwrap();
    engine.transfer_state(id).unwrap()
}

fn count(engine: &Engine, id: &str, table: &str) -> i64 {
    match run(engine, id, &format!("SELECT count(*) FROM {table}")).first().and_then(|row| row.first()) {
        Some(Value::Integer(count)) => *count,
        Some(Value::Decimal(text)) => text.parse().unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "needs PostgreSQL and MySQL in Docker"]
fn tables_move_from_postgres_to_mysql_and_back_with_their_types() {
    let engine = servers();
    run(&engine, "pg", "DROP TABLE IF EXISTS public.zen_transfer, public.zen_transfer_back");
    run(&engine, "my", "DROP DATABASE IF EXISTS zen_transfer");
    run(&engine, "my", "CREATE DATABASE zen_transfer");
    run(
        &engine,
        "pg",
        "CREATE TABLE public.zen_transfer (id bigint PRIMARY KEY, name varchar(40), price numeric(10,2), \
         active boolean, seen timestamptz, born date, picture bytea, doc jsonb, ref uuid)",
    );
    run(
        &engine,
        "pg",
        &format!(
            "INSERT INTO public.zen_transfer SELECT n, 'name ' || n || ' \\ ''q''', n * 1.25, n % 2 = 0, \
             timestamptz '2026-10-27 09:30:00+03' + n * interval '1 minute', date '2026-01-01' + (n % 300), \
             '\\x00ff'::bytea, jsonb_build_object('n', n), md5(n::text)::uuid FROM generate_series(1, {ROWS}) n"
        ),
    );
    run(&engine, "pg", "UPDATE public.zen_transfer SET name = NULL, doc = NULL WHERE id = 7");

    let state = moved(
        &engine,
        ("pg", "my"),
        (SqlDialect::Postgres, SqlDialect::MySql),
        ("public.zen_transfer", "zen_transfer.copied"),
        &SOURCE,
    );
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!((state.rows_read, state.rows_inserted, state.rows_committed), (50_000, 50_000, 50_000));
    assert!(!state.tables[0].server_copy);
    assert_eq!(count(&engine, "my", "zen_transfer.copied"), ROWS);
    let types = run(
        &engine,
        "my",
        "SELECT COLUMN_NAME, COLUMN_TYPE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = 'zen_transfer' \
         ORDER BY ORDINAL_POSITION",
    );
    let types: Vec<String> = types.iter().map(|row| format!("{:?}", row[1])).collect();
    assert!(types[1].contains("varchar(40)") && types[2].contains("decimal(10,2)"), "{types:?}");
    let rows = run(
        &engine,
        "my",
        "SELECT name, price, active, CAST(seen AS CHAR), CAST(born AS CHAR), HEX(picture), doc, ref \
         FROM zen_transfer.copied WHERE id IN (2, 7) ORDER BY id",
    );
    let shown: Vec<Vec<Option<String>>> = rows.iter().map(|row| row.iter().map(Value::display).collect()).collect();
    assert_eq!(
        shown[0],
        [
            Some("name 2 \\ 'q'".to_owned()),
            Some("2.50".to_owned()),
            Some("1".to_owned()),
            Some("2026-10-27 06:32:00.000000".to_owned()),
            Some("2026-01-03".to_owned()),
            Some("00FF".to_owned()),
            Some("{\"n\": 2}".to_owned()),
            Some("c81e728d-9d4c-2f63-6f06-7f89cc14862c".to_owned()),
        ]
    );
    assert_eq!((shown[1][0].clone(), shown[1][6].clone()), (None, None));

    let back: Vec<(&str, &str, bool)> = vec![
        ("id", "bigint", false),
        ("name", "varchar(40)", true),
        ("price", "decimal(10,2)", true),
        ("active", "tinyint(1)", true),
        ("seen", "datetime(6)", true),
        ("born", "date", true),
        ("picture", "longblob", true),
        ("doc", "json", true),
        ("ref", "char(36)", true),
    ];
    let state = moved(
        &engine,
        ("my", "pg"),
        (SqlDialect::MySql, SqlDialect::Postgres),
        ("zen_transfer.copied", "public.zen_transfer_back"),
        &back,
    );
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert!(state.tables[0].server_copy, "PostgreSQL takes the rows through COPY");
    assert_eq!(count(&engine, "pg", "public.zen_transfer_back"), ROWS);
    let differ = run(
        &engine,
        "pg",
        "SELECT count(*) FROM public.zen_transfer a JOIN public.zen_transfer_back b USING (id) \
         WHERE a.name IS DISTINCT FROM b.name OR a.price <> b.price OR a.active <> b.active \
         OR a.seen <> (b.seen AT TIME ZONE 'UTC') OR a.born <> b.born OR a.picture <> b.picture \
         OR a.doc IS DISTINCT FROM b.doc OR a.ref::text <> b.ref",
    );
    assert_eq!(differ, [vec![Value::Integer(0)]], "every value comes back as it left");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zen_transfer.parquet");
    let item = ExportItem {
        connection: "pg".to_owned(),
        context: None,
        statement: "SELECT * FROM public.zen_transfer ORDER BY id".to_owned(),
        name: "zen_transfer".to_owned(),
        table: "public.zen_transfer".to_owned(),
        dialect: SqlDialect::Postgres,
        output: Output::File { path: path.clone(), overwrite: false },
    };
    let settings = Settings { format: Format::Parquet, row_group: 10_000, ..Settings::default() };
    let id = engine.export(vec![item], settings).unwrap();
    engine.wait(id, WAIT).unwrap();
    let state = engine.export_state(id).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!(state.rows, 50_000);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), state.bytes);
    assert!(std::fs::read(&path).unwrap().starts_with(b"PAR1"));

    run(&engine, "pg", "DROP TABLE public.zen_transfer, public.zen_transfer_back");
    run(&engine, "my", "DROP DATABASE zen_transfer");
}
