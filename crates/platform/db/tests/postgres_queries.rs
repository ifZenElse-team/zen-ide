use std::time::{Duration, Instant};

use zen_db::driver::{ContextKind, ErrorKind};
use zen_db::engine::{
    ConnectRequest, Engine, OperationStatus, QueryOptions, QueryState, QueryStatement, StatementStatus, Timing,
};
use zen_db::registry::DataSource;
use zen_db::results::{Paging, ReadState, StopReason};
use zen_db::types::{ConnectionType, DEVELOPMENT, PRODUCTION};
use zen_db::value::{Typed, Value};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(60);

fn ports() -> Vec<u16> {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
}

fn new_engine() -> Engine {
    Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default())
}

fn request(id: &str, port: u16, kind: &str) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: "postgresql".to_owned(),
        connection_type: kind.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        database: Some("postgres".to_owned()),
        user: Some("postgres".to_owned()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == kind).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None }
}

fn connect(engine: &Engine, request: ConnectRequest) {
    let op = engine.connect(request).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn statements(texts: &[&str]) -> Vec<QueryStatement> {
    let mut at = 0;
    texts
        .iter()
        .map(|text| {
            let statement = QueryStatement {
                text: (*text).to_owned(),
                start: at,
                end: at + text.len(),
                client: false,
                params: Vec::new(),
                kept: None,
            };
            at += text.len() + 2;
            statement
        })
        .collect()
}

fn finished(engine: &Engine, op: u64) -> QueryState {
    let deadline = Instant::now() + WAIT;
    loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            return query;
        }
        assert!(Instant::now() < deadline, "{query:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn run(engine: &Engine, id: &str, context: &ContextKind, texts: &[&str]) -> QueryState {
    let op = engine.run_query(id, context.clone(), statements(texts), QueryOptions::default()).unwrap();
    finished(engine, op)
}

fn console(name: &str) -> ContextKind {
    ContextKind::Console(name.to_owned())
}

fn locks_on(engine: &Engine, id: &str, table: &str) -> i64 {
    let query = run(
        engine,
        id,
        &ContextKind::Main,
        &[&format!("SELECT count(*) FROM pg_locks WHERE relation = '{table}'::regclass AND pid <> pg_backend_pid()")],
    );
    let result = engine.results().get(query.statements[0].result.unwrap()).unwrap();
    let Value::Integer(count) = result.get(0, 0) else { panic!("{:?}", result.get(0, 0)) };
    engine.results().remove(result.id());
    count
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_script_runs_its_statements_with_their_notices_and_results() {
    for port in ports() {
        let engine = new_engine();
        connect(&engine, request("s", port, DEVELOPMENT));
        let query = run(
            &engine,
            "s",
            &console("one"),
            &[
                "CREATE TEMP TABLE q (id int, name text)",
                "INSERT INTO q SELECT g, 'n' || g FROM generate_series(1, 5) g",
                "DO $$ BEGIN RAISE NOTICE 'hello %', 42; RAISE WARNING 'careful'; END $$",
                "SELECT * FROM q ORDER BY id",
                "UPDATE q SET name = 'x' WHERE id < 3 RETURNING id",
                "SELECT 'one' AS a; ",
            ],
        );
        assert_eq!(query.status, OperationStatus::Done, "{query:?}");
        assert!(query.statements.iter().all(|statement| statement.status == StatementStatus::Done));
        assert_eq!(query.statements[1].affected, Some(5));
        assert_eq!(query.updated, 5, "rows changed by statements without rows");
        let notices: Vec<(&str, &str)> =
            query.messages.iter().map(|message| (message.severity.as_str(), message.text.as_str())).collect();
        assert_eq!(notices, [("NOTICE", "hello 42"), ("WARNING", "careful")]);
        assert!(query.messages.iter().all(|message| message.statement == Some(2)));
        let rows = engine.results().get(query.statements[3].result.unwrap()).unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows.get(4, 1), Value::Text("n5".into()));
        assert_eq!(rows.info().table.as_deref(), Some("q"));
        assert_eq!(rows.info().state, ReadState::Done);
        let returning = engine.results().get(query.statements[4].result.unwrap()).unwrap();
        assert_eq!(returning.len(), 2, "RETURNING is read whole, never paged");
        assert_eq!(query.statements[4].affected, Some(2));
        assert!(query.statements[5].result.is_some());
        assert!(engine.open_transactions("s").is_empty());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn an_error_says_where_it_is_and_rolls_back() {
    for port in ports() {
        let engine = new_engine();
        connect(&engine, request("e", port, PRODUCTION));
        let context = console("e");
        let query = run(
            &engine,
            "e",
            &context,
            &["CREATE TEMP TABLE r (id int)", "INSERT INTO r VALUES (1)", "SELECT idd FROM r", "SELECT 2"],
        );
        assert_eq!(query.status, OperationStatus::Failed);
        let error = query.statements[2].error.clone().unwrap();
        assert_eq!(error.kind, ErrorKind::Server);
        assert_eq!(error.code.as_deref(), Some("42703"));
        assert_eq!(error.position, Some(8), "{error:?}");
        assert_eq!(query.statements[3].status, StatementStatus::Cancelled);
        assert!(query.messages.iter().any(|message| message.code.as_deref() == Some("rolledBack")));
        assert!(engine.open_transactions("e").iter().all(|open| *open != context), "rolled back");
        let syntax = run(&engine, "e", &context, &["SELEC 1"]);
        let error = syntax.statements[0].error.clone().unwrap();
        assert_eq!((error.code.as_deref(), error.position), (Some("42601"), Some(1)));
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn cancel_stops_a_running_statement_within_a_second() {
    for port in ports() {
        let engine = new_engine();
        connect(&engine, request("c", port, DEVELOPMENT));
        let context = console("c");
        let op = engine
            .run_query("c", context.clone(), statements(&["SELECT pg_sleep(30)", "SELECT 1"]), QueryOptions::default())
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        assert!(engine.cancel_query(op));
        let query = finished(&engine, op);
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert_eq!(query.status, OperationStatus::Cancelled, "{query:?}");
        assert_eq!(query.statements[1].status, StatementStatus::Cancelled);
        let again = run(&engine, "c", &context, &["SELECT 1"]);
        assert_eq!(again.status, OperationStatus::Done, "the console works on: {again:?}");
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_hundred_thousand_rows_page_within_the_budget_and_leave_no_lock() {
    for port in ports() {
        let engine = new_engine();
        engine.results().set_paging(Paging { page: 200, row_cap: 10_000, keep_open: false });
        connect(&engine, request("p", port, DEVELOPMENT));
        let context = console("p");
        run(
            &engine,
            "p",
            &context,
            &[
                "DROP TABLE IF EXISTS zen_big",
                "CREATE TABLE zen_big AS SELECT g AS id, md5(g::text) AS hash FROM generate_series(1, 100000) g",
            ],
        );
        let query = run(&engine, "p", &context, &["SELECT * FROM zen_big ORDER BY id"]);
        let id = query.statements[0].result.unwrap();
        let result = engine.results().get(id).unwrap();
        let info = result.info();
        assert_eq!(info.rows, 10_000, "{info:?}");
        assert_eq!(info.stopped_because, Some(StopReason::Cap));
        assert!(info.more);
        assert!(engine.open_transactions("p").is_empty(), "the portal is closed and committed");
        assert_eq!(locks_on(&engine, "p", "zen_big"), 0, "no lock is left on the table");
        let op = engine.fetch_more(id, Some(200)).unwrap().expect("read again past the cap");
        assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
        assert_eq!(result.len(), 10_200);
        assert_eq!(result.get(10_199, 0), Value::Integer(10_200));
        let op = engine.fetch_more(id, None).unwrap().expect("Fetch All reads the rest");
        assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
        let info = result.info();
        assert_eq!((info.rows, info.more, info.state), (100_000, false, ReadState::Done));
        assert_eq!(result.get(99_999, 0), Value::Integer(100_000));
        let budget = engine.results().budget();
        assert!(budget.used() < budget.limit());
        assert!(info.bytes < 16 * 1024 * 1024, "typed columns keep 100 000 rows small: {}", info.bytes);
        let count = engine.count_rows(id).unwrap();
        assert_eq!(engine.wait(count, WAIT).unwrap().status, OperationStatus::Done);
        assert_eq!(result.info().total, Some(100_000));
        engine.results().remove(id);
        assert_eq!(budget.used(), 0);

        budget.set_limit(1024 * 1024);
        engine.results().set_paging(Paging { page: 200, row_cap: 1_000_000, keep_open: false });
        let query = run(&engine, "p", &context, &["SELECT * FROM zen_big"]);
        let limited = engine.results().get(query.statements[0].result.unwrap()).unwrap();
        let info = limited.info();
        assert_eq!(info.stopped_because, Some(StopReason::Budget), "{info:?}");
        assert!(info.rows > 0 && info.rows < 100_000);
        assert!(engine.open_transactions("p").is_empty());
        engine.results().remove(limited.id());
        budget.set_limit(zen_db::results::DEFAULT_BUDGET);
        run(&engine, "p", &context, &["DROP TABLE zen_big"]);
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn in_the_persons_transaction_the_portal_stays_open_until_the_next_run() {
    for port in ports() {
        let engine = new_engine();
        engine.results().set_paging(Paging { page: 200, row_cap: 10_000, keep_open: false });
        connect(&engine, request("t", port, PRODUCTION));
        let context = console("t");
        let query = run(&engine, "t", &context, &["SELECT g FROM generate_series(1, 5000) g"]);
        assert_eq!(query.status, OperationStatus::Done, "a waiting portal reads as done: {query:?}");
        let id = query.statements[0].result.unwrap();
        let result = engine.results().get(id).unwrap();
        assert_eq!(result.info().state, ReadState::Open);
        assert_eq!(result.len(), 200);
        assert!(!result.want_more(Some(300), true).unwrap(), "the open portal reads on");
        let deadline = Instant::now() + WAIT;
        while result.len() < 500 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(result.get(499, 0), Value::Integer(500));
        let next = run(&engine, "t", &context, &["SELECT 1"]);
        assert_eq!(next.status, OperationStatus::Done);
        assert!(!result.portal_open(), "the next run closed it");
        assert_eq!(result.len(), 500, "its rows stay");
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn commit_and_rollback_close_an_open_portal_instead_of_waiting_behind_it() {
    for port in ports() {
        let engine = new_engine();
        engine.results().set_paging(Paging { page: 200, row_cap: 10_000, keep_open: false });
        connect(&engine, request("w", port, PRODUCTION));
        let context = console("w");
        for commit in [true, false] {
            let query = run(&engine, "w", &context, &["SELECT g FROM generate_series(1, 5000) g"]);
            let result = engine.results().get(query.statements[0].result.unwrap()).unwrap();
            assert!(result.portal_open(), "{query:?}");
            let op = engine.end_transaction("w", context.clone(), commit).unwrap();
            let state = engine.wait(op, Duration::from_secs(10)).unwrap();
            assert_eq!(state.status, OperationStatus::Done, "{state:?}");
            assert!(!result.portal_open());
            assert!(engine.open_transactions("w").is_empty());
        }
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_statement_stops_reading_at_its_row_limit_even_when_its_rows_never_end() {
    for port in ports() {
        let engine = new_engine();
        connect(&engine, request("cap", port, DEVELOPMENT));
        let endless = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT x FROM c";
        let options = zen_db::driver::ExecOptions { max_rows: 5, timeout: Some(Duration::from_secs(20)) };
        let op = engine.execute("cap", ContextKind::Main, endless, options).unwrap();
        let state = engine.wait(op, WAIT).unwrap();
        assert_eq!(state.status, OperationStatus::Done, "{:?}", state.error);
        let execution = state.execution.unwrap();
        assert_eq!((execution.rows.len(), execution.truncated), (5, true));
        assert_eq!(execution.rows[4], [Value::Integer(5)]);
        assert!(engine.open_transactions("cap").is_empty());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn timestamps_with_a_zone_are_shown_in_the_sessions_zone() {
    for port in ports() {
        let engine = new_engine();
        connect(&engine, request("z", port, DEVELOPMENT));
        let context = console("z");
        let query = run(
            &engine,
            "z",
            &context,
            &[
                "SET TimeZone = 'Asia/Makassar'",
                "SELECT timestamptz '2024-01-01 00:00:00+00', ARRAY[timestamptz '2024-06-01 12:00:00+00']",
                "SET TimeZone = 'America/New_York'",
                "SELECT timestamptz '2024-01-01 00:00:00+00', timestamptz '2024-07-01 00:00:00+00'",
            ],
        );
        let at = |text: &str| Value::Temporal(Typed { text: text.into(), type_name: "timestamptz".into() });
        let first = engine.results().get(query.statements[1].result.unwrap()).unwrap();
        assert_eq!(first.get(0, 0), at("2024-01-01 08:00:00+08"));
        assert_eq!(first.get(0, 1), Value::Array(vec![at("2024-06-01 20:00:00+08")]));
        let second = engine.results().get(query.statements[3].result.unwrap()).unwrap();
        assert_eq!(second.get(0, 0), at("2023-12-31 19:00:00-05"));
        assert_eq!(second.get(0, 1), at("2024-06-30 20:00:00-04"));
    }
}

#[test]
#[ignore = "needs PostgreSQL servers in Docker (docs/development.md)"]
fn read_only_is_kept_by_the_server_and_by_the_engine() {
    for port in ports() {
        let engine = new_engine();
        let id = format!("ro{port}");
        connect(&engine, request(&id, port, DEVELOPMENT));
        let writer = console("writer");
        let setup = run(
            &engine,
            &id,
            &writer,
            &[
                "DROP TABLE IF EXISTS zen_ro",
                "CREATE TABLE zen_ro (n int)",
                "CREATE OR REPLACE FUNCTION zen_ro_write() RETURNS int LANGUAGE sql AS $$ INSERT INTO zen_ro VALUES (1) RETURNING n $$",
            ],
        );
        assert_eq!(setup.status, OperationStatus::Done, "{setup:?}");
        let agent = console("agent");
        let reads_only = QueryOptions { read_only: true, ..QueryOptions::default() };
        let op =
            engine.run_query(&id, agent.clone(), statements(&["SELECT zen_ro_write()"]), reads_only.clone()).unwrap();
        let ran = finished(&engine, op);
        let error = ran.statements[0].error.clone().unwrap();
        assert_eq!(error.code.as_deref(), Some("25006"), "the server stops a write hidden in a SELECT: {error:?}");
        let op = engine.run_query(&id, agent, statements(&["INSERT INTO zen_ro VALUES (2)"]), reads_only).unwrap();
        let error = finished(&engine, op).statements[0].error.clone().unwrap();
        assert_eq!((error.kind, error.code.as_deref()), (ErrorKind::Refused, Some("readOnly")));
        for op in engine.set_read_only(&id, true) {
            assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
        }
        for statement in
            ["INSERT INTO zen_ro VALUES (3)", "SET default_transaction_read_only = off", "BEGIN READ WRITE"]
        {
            let error = run(&engine, &id, &writer, &[statement]).statements[0].error.clone().unwrap();
            assert_eq!(error.kind, ErrorKind::Refused, "{statement}: {error:?}");
        }
        let error = run(&engine, &id, &writer, &["SELECT zen_ro_write()"]).statements[0].error.clone().unwrap();
        assert_eq!(error.code.as_deref(), Some("25006"), "{error:?}");
        assert_eq!(run(&engine, &id, &writer, &["SELECT count(*) FROM zen_ro"]).status, OperationStatus::Done);
        for op in engine.set_read_only(&id, false) {
            engine.wait(op, WAIT).unwrap();
        }
        let ran = run(
            &engine,
            &id,
            &writer,
            &["INSERT INTO zen_ro VALUES (4)", "DROP FUNCTION zen_ro_write()", "DROP TABLE zen_ro"],
        );
        assert_eq!(ran.status, OperationStatus::Done, "{ran:?}");
    }
}
