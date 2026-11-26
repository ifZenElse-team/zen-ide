use std::time::Duration;

use zen_db::driver::{ContextKind, ErrorKind};
use zen_db::engine::{
    CommitMode, ConnectRequest, Engine, ModeChange, OperationStatus, QueryOptions, QueryStatement, Timing,
    TransactionPolicy,
};
use zen_db::registry::DataSource;
use zen_db::types::{ConnectionType, DEVELOPMENT, PRODUCTION};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(30);

fn ports() -> Vec<u16> {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
}

fn request(id: &str, port: u16, kind: &str, smart: bool) -> ConnectRequest {
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
    let mut kind = ConnectionType::built_in().into_iter().find(|known| known.id == kind).unwrap();
    kind.smart_commit = smart;
    ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None }
}

fn ok(engine: &Engine, op: u64) {
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn run(engine: &Engine, id: &str, context: &ContextKind, sql: &str) -> OperationStatus {
    let statement = QueryStatement {
        text: sql.to_owned(),
        start: 0,
        end: sql.len(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let options = QueryOptions { rollback_on_error: false, ..QueryOptions::default() };
    let op = engine.run_query(id, context.clone(), vec![statement], options).unwrap();
    engine.wait(op, WAIT).unwrap();
    for _ in 0..500 {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            return query.status;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the query did not end");
}

fn count(engine: &Engine, id: &str, table: &str) -> i64 {
    let op = engine
        .execute(id, ContextKind::Metadata, &format!("SELECT count(*) FROM {table}"), Default::default())
        .unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    match state.execution.unwrap().rows[0][0] {
        Value::Integer(count) => count,
        ref other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn manual_commit_keeps_changes_until_commit_and_counts_them() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        let prod = request("tx", port, PRODUCTION, false);
        ok(&engine, engine.connect(prod.clone()).unwrap());
        let console = ContextKind::Console("tx/console".to_owned());
        let setup = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
        assert_eq!(engine.set_mode("tx", console.clone(), setup, TransactionPolicy::Ask).unwrap(), None);
        assert_eq!(run(&engine, "tx", &console, "DROP TABLE IF EXISTS zen_tx"), OperationStatus::Done);
        assert_eq!(run(&engine, "tx", &console, "CREATE TABLE zen_tx (a int)"), OperationStatus::Done);
        let manual = ModeChange { mode: Some(CommitMode::Manual), isolation: Some(Some("serializable".to_owned())) };
        ok(&engine, engine.set_mode("tx", console.clone(), manual, TransactionPolicy::Ask).unwrap().unwrap());
        run(&engine, "tx", &console, "INSERT INTO zen_tx VALUES (1)");
        run(&engine, "tx", &console, "SELECT * FROM zen_tx");
        run(&engine, "tx", &console, "UPDATE zen_tx SET a = 2");
        let state = engine.transaction(&prod, &console, 10);
        assert!(state.in_transaction && !state.auto_commit, "{state:?}");
        assert_eq!((state.statements, state.modifying, state.isolation.as_deref()), (3, 2, Some("serializable")));
        assert_eq!(count(&engine, "tx", "zen_tx"), 0, "another session does not see uncommitted rows");
        let refused = engine.disconnect("tx", TransactionPolicy::Ask).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::OpenTransactions);
        ok(&engine, engine.end_transaction("tx", console.clone(), true).unwrap());
        assert_eq!(count(&engine, "tx", "zen_tx"), 1);
        run(&engine, "tx", &console, "DELETE FROM zen_tx");
        ok(&engine, engine.end_transaction("tx", console.clone(), false).unwrap());
        assert_eq!(count(&engine, "tx", "zen_tx"), 1, "the delete was rolled back");
        assert!(engine.pending().is_empty());
        run(&engine, "tx", &console, "INSERT INTO zen_tx VALUES (3)");
        ok(&engine, engine.disconnect("tx", TransactionPolicy::Commit).unwrap());
        ok(&engine, engine.connect(prod.clone()).unwrap());
        assert_eq!(count(&engine, "tx", "zen_tx"), 2, "disconnecting with commit kept the insert");
        let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
        assert!(engine.set_mode("tx", console.clone(), auto, TransactionPolicy::Ask).is_ok());
        run(&engine, "tx", &console, "DROP TABLE zen_tx");
        ok(&engine, engine.disconnect("tx", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn smart_commit_switches_on_the_first_change_and_back_on_commit_or_failure() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        let dev = request("smart", port, DEVELOPMENT, true);
        ok(&engine, engine.connect(dev.clone()).unwrap());
        let console = ContextKind::Console("smart/console".to_owned());
        run(&engine, "smart", &console, "DROP TABLE IF EXISTS zen_smart");
        assert_eq!(engine.transaction(&dev, &console, 0).mode, CommitMode::Smart);
        let state = engine.transaction(&dev, &console, 0);
        assert!(!state.auto_commit && state.in_transaction && state.modifying == 1, "DROP switched: {state:?}");
        ok(&engine, engine.end_transaction("smart", console.clone(), true).unwrap());
        run(&engine, "smart", &console, "SELECT 1");
        assert!(engine.transaction(&dev, &console, 0).auto_commit, "a read does not switch");
        run(&engine, "smart", &console, "CREATE TABLE zen_smart (a int)");
        let switched = engine.transaction(&dev, &console, 0);
        assert!(!switched.auto_commit && switched.in_transaction, "{switched:?}");
        ok(&engine, engine.end_transaction("smart", console.clone(), true).unwrap());
        assert!(engine.transaction(&dev, &console, 0).auto_commit, "Development returns to auto-commit");
        assert_eq!(run(&engine, "smart", &console, "INSERT INTO zen_smart VALUES ('x')"), OperationStatus::Failed);
        let back = engine.transaction(&dev, &console, 0);
        assert!(back.auto_commit && !back.in_transaction, "a failed switching statement goes back: {back:?}");
        run(&engine, "smart", &console, "DROP TABLE zen_smart");
        ok(&engine, engine.end_transaction("smart", console.clone(), true).unwrap());
        ok(&engine, engine.disconnect("smart", TransactionPolicy::Rollback).unwrap());
    }
}

fn first_cell(engine: &Engine, id: &str, context: &ContextKind, sql: &str) -> Value {
    let op = engine.execute(id, context.clone(), sql, Default::default()).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{sql}: {state:?}");
    state.execution.unwrap().rows[0][0].clone()
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn committing_an_aborted_transaction_fails_and_says_it_was_rolled_back() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        let prod = request("abort", port, PRODUCTION, false);
        ok(&engine, engine.connect(prod.clone()).unwrap());
        let console = ContextKind::Console("abort/console".to_owned());
        let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
        assert_eq!(engine.set_mode("abort", console.clone(), auto, TransactionPolicy::Ask).unwrap(), None);
        run(&engine, "abort", &console, "DROP TABLE IF EXISTS zen_abort");
        assert_eq!(run(&engine, "abort", &console, "CREATE TABLE zen_abort (a int)"), OperationStatus::Done);
        let manual = ModeChange { mode: Some(CommitMode::Manual), isolation: None };
        ok(&engine, engine.set_mode("abort", console.clone(), manual, TransactionPolicy::Ask).unwrap().unwrap());
        assert_eq!(run(&engine, "abort", &console, "INSERT INTO zen_abort VALUES (1)"), OperationStatus::Done);
        assert_eq!(run(&engine, "abort", &console, "SELECT 1 / 0"), OperationStatus::Failed);
        let op = engine.end_transaction("abort", console.clone(), true).unwrap();
        let state = engine.wait(op, WAIT).unwrap();
        assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
        assert_eq!(state.error.unwrap().code.as_deref(), Some("rolledBack"));
        assert!(!engine.transaction(&prod, &console, 0).in_transaction);
        assert_eq!(count(&engine, "abort", "zen_abort"), 0);
        let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
        ok(&engine, engine.set_mode("abort", console.clone(), auto, TransactionPolicy::Rollback).unwrap().unwrap());
        run(&engine, "abort", &console, "DROP TABLE zen_abort");
        ok(&engine, engine.disconnect("abort", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_schema_and_read_only_chosen_inside_a_transaction_survive_its_rollback() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        let prod = request("path", port, PRODUCTION, false);
        ok(&engine, engine.connect(prod.clone()).unwrap());
        let console = ContextKind::Console("path/console".to_owned());
        let manual = ModeChange { mode: Some(CommitMode::Manual), isolation: None };
        if let Some(op) = engine.set_mode("path", console.clone(), manual, TransactionPolicy::Ask).unwrap() {
            ok(&engine, op);
        }
        run(&engine, "path", &ContextKind::Main, "CREATE SCHEMA IF NOT EXISTS zen_path");
        ok(&engine, engine.end_transaction("path", ContextKind::Main, true).unwrap());
        assert_eq!(run(&engine, "path", &console, "SELECT 1"), OperationStatus::Done);
        assert!(engine.transaction(&prod, &console, 0).in_transaction);
        let tracked = zen_db::driver::TrackedState {
            schema: Some("zen_path".to_owned()),
            auto_commit: false,
            ..zen_db::driver::TrackedState::default()
        };
        ok(&engine, engine.apply("path", console.clone(), tracked).unwrap());
        ok(&engine, engine.end_transaction("path", console.clone(), false).unwrap());
        assert_eq!(first_cell(&engine, "path", &console, "SELECT current_schema()"), Value::Text("zen_path".into()));
        ok(&engine, engine.end_transaction("path", console.clone(), false).unwrap());
        for op in engine.set_read_only("path", true) {
            ok(&engine, op);
        }
        assert_eq!(run(&engine, "path", &console, "SELECT 1"), OperationStatus::Done);
        ok(&engine, engine.end_transaction("path", console.clone(), false).unwrap());
        assert_eq!(
            first_cell(&engine, "path", &console, "SHOW default_transaction_read_only"),
            Value::Text("on".into())
        );
        ok(&engine, engine.end_transaction("path", console.clone(), false).unwrap());
        for op in engine.set_read_only("path", false) {
            ok(&engine, op);
        }
        run(&engine, "path", &ContextKind::Main, "DROP SCHEMA zen_path");
        ok(&engine, engine.disconnect("path", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn changing_only_the_isolation_keeps_the_open_transaction() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        let dev = request("iso", port, DEVELOPMENT, true);
        ok(&engine, engine.connect(dev.clone()).unwrap());
        let console = ContextKind::Console("iso/console".to_owned());
        run(&engine, "iso", &console, "DROP TABLE IF EXISTS zen_iso");
        ok(&engine, engine.end_transaction("iso", console.clone(), true).unwrap());
        run(&engine, "iso", &console, "CREATE TABLE zen_iso (a int)");
        assert!(engine.transaction(&dev, &console, 0).in_transaction, "smart commit switched to manual");
        let isolation = ModeChange { mode: None, isolation: Some(Some("serializable".to_owned())) };
        let changed = engine.set_mode("iso", console.clone(), isolation, TransactionPolicy::Ask);
        if let Ok(Some(op)) = changed {
            ok(&engine, op);
        }
        let state = engine.transaction(&dev, &console, 0);
        assert!(state.in_transaction && state.modifying == 1, "the change was kept: {state:?}");
        ok(&engine, engine.end_transaction("iso", console.clone(), true).unwrap());
        assert_eq!(count(&engine, "iso", "zen_iso"), 0);
        run(&engine, "iso", &console, "DROP TABLE zen_iso");
        ok(&engine, engine.end_transaction("iso", console.clone(), true).unwrap());
        ok(&engine, engine.disconnect("iso", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn changing_only_the_isolation_keeps_a_transaction_begun_in_auto_mode() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        let dev = request("auto", port, DEVELOPMENT, true);
        ok(&engine, engine.connect(dev.clone()).unwrap());
        let console = ContextKind::Console("auto/console".to_owned());
        let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
        if let Ok(Some(op)) = engine.set_mode("auto", console.clone(), auto, TransactionPolicy::Ask) {
            ok(&engine, op);
        }
        run(&engine, "auto", &console, "DROP TABLE IF EXISTS zen_auto");
        run(&engine, "auto", &console, "CREATE TABLE zen_auto (a int)");
        run(&engine, "auto", &console, "BEGIN");
        run(&engine, "auto", &console, "INSERT INTO zen_auto VALUES (1)");
        assert!(engine.transaction(&dev, &console, 0).in_transaction, "BEGIN opened one");
        let isolation = ModeChange { mode: None, isolation: Some(Some("serializable".to_owned())) };
        if let Ok(Some(op)) = engine.set_mode("auto", console.clone(), isolation, TransactionPolicy::Ask) {
            ok(&engine, op);
        }
        assert!(engine.transaction(&dev, &console, 0).in_transaction, "neither committed nor rolled back");
        run(&engine, "auto", &console, "ROLLBACK");
        assert_eq!(count(&engine, "auto", "zen_auto"), 0, "the insert was still open, so it rolled back");
        run(&engine, "auto", &console, "DROP TABLE zen_auto");
        ok(&engine, engine.disconnect("auto", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn init_statements_of_a_read_only_connection_never_change_data() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        let dev = request("init", port, DEVELOPMENT, false);
        ok(&engine, engine.connect(dev.clone()).unwrap());
        run(&engine, "init", &ContextKind::Main, "DROP SEQUENCE IF EXISTS zen_init_seq");
        run(&engine, "init", &ContextKind::Main, "CREATE SEQUENCE zen_init_seq");
        for init in ["CREATE TABLE zen_init (a int)", "SELECT nextval('zen_init_seq')"] {
            let mut locked = request("locked", port, DEVELOPMENT, false);
            locked.source.read_only = true;
            locked.source.bootstrap.init_statements = vec![init.to_owned()];
            let state = engine.wait(engine.connect(locked).unwrap(), WAIT).unwrap();
            assert_eq!(state.status, OperationStatus::Failed, "{init}: {state:?}");
        }
        let next = first_cell(&engine, "init", &ContextKind::Main, "SELECT nextval('zen_init_seq')");
        assert_eq!(next, Value::Integer(1), "the read-only connection never advanced the sequence");
        let exists = first_cell(&engine, "init", &ContextKind::Main, "SELECT to_regclass('zen_init') IS NULL");
        assert_eq!(exists, Value::Bool(true));
        run(&engine, "init", &ContextKind::Main, "DROP SEQUENCE zen_init_seq");
        ok(&engine, engine.disconnect("init", TransactionPolicy::Rollback).unwrap());
    }
}
