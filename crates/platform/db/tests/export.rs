use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use zen_db::dialect::SqlDialect;
use zen_db::driver::ContextKind;
use zen_db::engine::{
    ConnectRequest, Engine, ExportItem, ExportState, ItemStatus, OperationStatus, Output, QueryOptions, QueryStatement,
    Timing,
};
use zen_db::export::{Format, LineEnding, Settings};
use zen_db::registry::DataSource;
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(300);
const MILLION: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000) \
                       SELECT x AS id, 'name ' || x AS name, x * 1.5 AS price, NULL AS empty, '=cmd' AS risky FROM c";

fn engine() -> Engine {
    Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default())
}

fn rss_kb() -> u64 {
    let pid = std::process::id().to_string();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse().ok())
        .unwrap_or(0)
}

fn watched(engine: &Engine, id: u64) -> (ExportState, u64) {
    let base = rss_kb();
    let peak = Arc::new(AtomicU64::new(base));
    let stop = Arc::new(AtomicBool::new(false));
    let sampler = {
        let (peak, stop) = (Arc::clone(&peak), Arc::clone(&stop));
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                peak.fetch_max(rss_kb(), Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    engine.wait(id, WAIT).unwrap();
    stop.store(true, Ordering::SeqCst);
    sampler.join().unwrap();
    let state = engine.export_state(id).unwrap();
    (state, peak.load(Ordering::SeqCst).saturating_sub(base) / 1024)
}

fn item(connection: &str, statement: &str, path: &Path, dialect: SqlDialect) -> ExportItem {
    ExportItem {
        connection: connection.to_owned(),
        context: None,
        statement: statement.to_owned(),
        name: "million".to_owned(),
        table: "million".to_owned(),
        dialect,
        output: Output::File { path: path.to_owned(), overwrite: false },
    }
}

fn lines(path: &Path) -> (usize, String, String) {
    let text = std::fs::read_to_string(path).unwrap();
    let count = text.lines().count();
    let first: String = text.lines().take(3).collect::<Vec<_>>().join("|");
    let last = text.lines().last().unwrap_or_default().to_owned();
    (count, first, last)
}

#[test]
fn sqlite_streams_a_million_rows_to_a_file_within_bounded_memory() {
    let Ok(_) = zen_db::sqlite::process::find_program(None) else {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.db");
    std::fs::File::create(&file).unwrap();
    let engine = engine();
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
    let out = dir.path().join("million.csv");
    let id = engine.export(vec![item("lite", MILLION, &out, SqlDialect::Sqlite)], Settings::default()).unwrap();
    let (state, grown) = watched(&engine, id);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!(state.rows, 1_000_000);
    let (count, first, last) = lines(&out);
    assert_eq!(count, 1_000_001);
    assert_eq!(first, "id,name,price,empty,risky|1,name 1,1.5,,'=cmd|2,name 2,3,,'=cmd");
    assert_eq!(last, "1000000,name 1000000,1500000,,'=cmd");
    assert_eq!(state.bytes, std::fs::metadata(&out).unwrap().len());
    assert!(grown < 64, "the rows are streamed, not kept: memory grew by {grown} MiB");

    let sql = dir.path().join("few.sql");
    let few = "SELECT 1 AS id, 'it''s' AS name, x'00ff' AS data UNION ALL SELECT 2, NULL, NULL";
    let settings = Settings { format: Format::Sql, table: Some("copy_of".to_owned()), ..Settings::default() };
    let id = engine.export(vec![item("lite", few, &sql, SqlDialect::Sqlite)], settings).unwrap();
    engine.wait(id, WAIT).unwrap();
    assert_eq!(
        std::fs::read_to_string(&sql).unwrap(),
        "INSERT INTO copy_of (\"id\", \"name\", \"data\") VALUES\n\t(1, 'it''s', X'00ff'),\n\t(2, NULL, NULL);\n"
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

fn pg_engine(id: &str) -> Engine {
    let engine = engine();
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: "postgresql".to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(pg_port()),
        database: Some("postgres".to_owned()),
        user: Some("postgres".to_owned()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let request =
        ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None };
    let op = engine.connect(request).unwrap();
    assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
    engine
}

const PG_MILLION: &str = "SELECT x AS id, 'name ' || x AS name, x * 1.5 AS price, NULL::text AS nothing, '=cmd' AS risky, \
                          x % 2 = 0 AS even FROM generate_series(1, 1000000) AS x";

#[test]
#[ignore = "needs PostgreSQL in Docker"]
fn postgres_copies_a_million_rows_with_copy_and_streams_them_without_it_within_bounded_memory() {
    let engine = pg_engine("pg");
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("copied.csv");
    let settings = Settings { line_ending: LineEnding::CrLf, ..Settings::default() };
    let id = engine.export(vec![item("pg", PG_MILLION, &out, SqlDialect::Postgres)], settings).unwrap();
    let (state, grown) = watched(&engine, id);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert!(state.items[0].server_copy, "CSV goes through COPY");
    assert_eq!(state.rows, 1_000_000);
    let (count, first, last) = lines(&out);
    assert_eq!(count, 1_000_001);
    assert_eq!(first, "id,name,price,nothing,risky,even|1,name 1,1.5,,'=cmd,f|2,name 2,3.0,,'=cmd,t");
    assert_eq!(last, "1000000,name 1000000,1500000.0,,'=cmd,t");
    let written = std::fs::read(&out).unwrap();
    assert_eq!(written.windows(2).filter(|pair| pair == b"\r\n").count(), 1_000_001, "every line ends with CRLF");
    assert!(grown < 64, "COPY streams: memory grew by {grown} MiB");

    let rows = dir.path().join("rows.csv");
    let settings = Settings { server_copy: false, ..Settings::default() };
    let id = engine.export(vec![item("pg", PG_MILLION, &rows, SqlDialect::Postgres)], settings).unwrap();
    let (state, grown) = watched(&engine, id);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert!(!state.items[0].server_copy);
    let (count, first, _) = lines(&rows);
    assert_eq!(count, 1_000_001);
    assert_eq!(first, "id,name,price,nothing,risky,even|1,name 1,1.5,,'=cmd,false|2,name 2,3.0,,'=cmd,true");
    assert!(grown < 64, "rows stream through the portal: memory grew by {grown} MiB");
}

#[test]
#[ignore = "needs PostgreSQL in Docker"]
fn postgres_exports_see_the_console_transaction_and_cancel_on_the_server() {
    let engine = pg_engine("pg2");
    let console = ContextKind::Console("export test".to_owned());
    let begin = vec![
        QueryStatement { text: "BEGIN".to_owned(), start: 0, end: 5, client: false, params: Vec::new(), kept: None },
        QueryStatement {
            text: "CREATE TEMP TABLE pending AS SELECT 1 AS id, 'a\"b,c' AS name".to_owned(),
            start: 6,
            end: 40,
            client: false,
            params: Vec::new(),
            kept: None,
        },
    ];
    let op = engine.run_query("pg2", console.clone(), begin, QueryOptions::default()).unwrap();
    engine.wait(op, WAIT).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("pending.json");
    let mut seen = item("pg2", "SELECT * FROM pending", &json, SqlDialect::Postgres);
    seen.context = Some(console.clone());
    let settings = Settings { format: Format::Json, ..Settings::default() };
    let id = engine.export(vec![seen], settings).unwrap();
    let (state, _) = watched(&engine, id);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    let parsed: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(parsed, serde_json::json!([{ "id": 1, "name": "a\"b,c" }]));
    assert!(engine.open_transactions("pg2").contains(&console), "the console's transaction stays open");

    let slow = dir.path().join("slow.csv");
    let statement = "SELECT x, pg_sleep(0.001) FROM generate_series(1, 100000) AS x";
    let id = engine.export(vec![item("pg2", statement, &slow, SqlDialect::Postgres)], Settings::default()).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let started = std::time::Instant::now();
    assert!(engine.cancel(id));
    engine.wait(id, WAIT).unwrap();
    assert!(started.elapsed() < Duration::from_secs(3), "cancelled on the server: {:?}", started.elapsed());
    let state = engine.export_state(id).unwrap();
    assert_eq!((state.status, state.items[0].status), (OperationStatus::Cancelled, ItemStatus::Cancelled));
    let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|entry| entry.file_name()).collect();
    assert_eq!(left, vec![std::ffi::OsString::from("pending.json")], "nothing of the cancelled export is left");
}
