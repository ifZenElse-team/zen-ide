use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::memory::{EMBEDDED, ID, MemoryDriver};
use crate::model::DataModel;
use crate::registry::{AuthKind, TypeOverrides};
use crate::types::{DEVELOPMENT, PRODUCTION};
use crate::value::Value;

const WAIT: Duration = Duration::from_secs(10);

fn timing() -> Timing {
    Timing {
        backoff: [0, 10, 20].map(Duration::from_millis).to_vec(),
        cancel_wait: Duration::from_millis(300),
        tick: Duration::from_secs(3600),
        idle_countdown: Duration::from_millis(200),
        ..Timing::default()
    }
}

fn engine(embedded: bool) -> (Engine, Arc<MemoryDriver>) {
    let driver = Arc::new(MemoryDriver::new(embedded));
    let engine = Engine::with_timing(vec![Arc::clone(&driver) as Arc<dyn Driver>], "Zen Test", timing());
    (engine, driver)
}

fn request(id: &str, driver: &str, kind: &str) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: format!("Name of {id}"),
        driver: driver.to_owned(),
        connection_type: kind.to_owned(),
        auth: AuthKind::Password,
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == kind).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("pw".to_owned())), stored: None, ssh_secret: None }
}

fn done(engine: &Engine, op: u64) -> OperationState {
    let state = engine.wait(op, WAIT).unwrap();
    assert_ne!(state.status, OperationStatus::Running, "{state:?}");
    state
}

fn ok(engine: &Engine, op: u64) -> OperationState {
    let state = done(engine, op);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state
}

fn run(engine: &Engine, id: &str, context: ContextKind, statement: &str) -> OperationState {
    let op = engine.execute(id, context, statement, ExecOptions::default()).unwrap();
    done(engine, op)
}

fn connected(engine: &Engine, request: ConnectRequest) {
    let op = engine.connect(request).unwrap();
    ok(engine, op);
}

#[test]
fn a_key_value_driver_serves_the_same_interface() {
    let (engine, driver) = engine(false);
    assert_eq!(driver.capabilities().models, vec![DataModel::KeyValue]);
    assert_eq!(driver.capabilities().language, "redis");
    connected(&engine, request("kv", ID, DEVELOPMENT));
    assert_eq!(run(&engine, "kv", ContextKind::Main, "SET user:1 ann").status, OperationStatus::Done);
    run(&engine, "kv", ContextKind::Main, "SET user:2 bob");
    run(&engine, "kv", ContextKind::Main, "HSET session:9 user ann");
    let got = run(&engine, "kv", ContextKind::Main, "GET user:1").execution.unwrap();
    assert_eq!(
        got.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(),
        ["key", "type", "ttl", "value"]
    );
    assert_eq!(got.rows[0][3], Value::Text("ann".to_owned()));
    let hash = run(&engine, "kv", ContextKind::Main, "HGETALL session:9").execution.unwrap();
    assert_eq!(hash.rows[0][3], Value::Map(vec![("user".to_owned(), Value::Text("ann".to_owned()))]));
    let roots = ok(&engine, engine.children("kv", Vec::new()).unwrap()).nodes.unwrap();
    let labels: Vec<_> = roots.iter().map(|node| (node.label.as_str(), node.kind.clone())).collect();
    assert_eq!(labels, [("session", crate::model::NodeKind::KeyPrefix), ("user", crate::model::NodeKind::KeyPrefix)]);
    let users = ok(&engine, engine.children("kv", vec!["user".to_owned()]).unwrap()).nodes.unwrap();
    assert_eq!(users.iter().map(|node| node.label.as_str()).collect::<Vec<_>>(), ["1", "2"]);
}

#[test]
fn connect_opens_main_then_metadata_with_guards_and_consoles_get_their_own() {
    let (engine, driver) = engine(false);
    connected(&engine, request("prod", ID, PRODUCTION));
    let opened = driver.server.opened();
    assert_eq!(
        opened.iter().map(|open| open.context.clone()).collect::<Vec<_>>(),
        [ContextKind::Main, ContextKind::Metadata]
    );
    assert!(
        opened.iter().all(|open| open.guards.application_name == "Zen Test" && open.password.as_deref() == Some("pw"))
    );
    assert_eq!(
        opened[0].guards.statement_timeout,
        Some(Duration::from_secs(300)),
        "Production sets a statement timeout"
    );
    assert_eq!(opened[0].guards.lock_timeout, None, "the user's SQL gets no lock timeout");
    assert_eq!(opened[1].guards.lock_timeout, Some(Duration::from_secs(10)));
    assert_eq!(opened[1].guards.idle_in_transaction_timeout, Some(Duration::from_secs(60)));
    assert!(!opened[0].state.auto_commit, "Production starts in manual commit");
    assert!(opened[1].state.auto_commit && opened[1].state.read_only, "metadata reads in auto-commit, read-only");
    run(&engine, "prod", ContextKind::Console("console_1".into()), "GET a");
    assert_eq!(driver.server.opened().last().unwrap().context, ContextKind::Console("console_1".into()));
    let status = engine.status("prod");
    assert_eq!(status.state, ConnectionState::Connected);
    assert_eq!(status.contexts.len(), 3);
    assert!(!status.auto_commit);
}

#[test]
fn an_embedded_driver_shares_one_session() {
    let (engine, driver) = engine(true);
    connected(&engine, request("file", EMBEDDED, DEVELOPMENT));
    run(&engine, "file", ContextKind::Console("c".into()), "SET a 1");
    assert_eq!(driver.server.opened().len(), 1);
    assert_eq!(engine.status("file").contexts.len(), 1);
}

#[test]
fn a_connect_while_one_runs_waits_for_it_and_nothing_connects_without_a_password() {
    let (engine, _driver) = engine(false);
    let mut without = request("pw", ID, DEVELOPMENT);
    without.password = None;
    assert_eq!(engine.connect(without).unwrap_err().kind, ErrorKind::PasswordNeeded);
    assert_eq!(engine.status("pw").state, ConnectionState::Disconnected);
    let first = engine.connect(request("pw", ID, DEVELOPMENT)).unwrap();
    let second = engine.connect(request("pw", ID, DEVELOPMENT)).unwrap();
    assert!(second == first || engine.operation(second).unwrap().status == OperationStatus::Done);
    ok(&engine, first);
    let refused = engine.execute("other", ContextKind::Main, "GET a", ExecOptions::default()).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::NotConnected);
}

#[test]
fn a_wrong_password_fails_and_is_forgotten() {
    let (engine, driver) = engine(false);
    driver.server.require_password("right");
    let op = engine.connect(request("auth", ID, DEVELOPMENT)).unwrap();
    let failed = done(&engine, op);
    assert_eq!(failed.error.unwrap().kind, ErrorKind::Authentication);
    let status = engine.status("auth");
    assert_eq!(status.state, ConnectionState::Failed);
    assert!(!format!("{status:?}").contains("pw"));
}

fn started(driver: &MemoryDriver, statement: &str, count: usize) {
    let since = Instant::now();
    while driver.server.executed().iter().filter(|executed| *executed == statement).count() < count {
        assert!(since.elapsed() < Duration::from_secs(30), "{statement} never reached the server");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_lost_connection_retries_a_plain_read_once_and_never_replays_a_write() {
    let (engine, driver) = engine(false);
    connected(&engine, request("lost", ID, DEVELOPMENT));
    run(&engine, "lost", ContextKind::Main, "SET a 1");
    driver.server.drop_connections();
    let read = run(&engine, "lost", ContextKind::Main, "GET a");
    assert_eq!(read.status, OperationStatus::Done, "a closed session is noticed before it is used: {read:?}");
    assert!(read.reconnected);

    driver.server.hold(true);
    let op = engine.execute("lost", ContextKind::Main, "SLEEP 200", ExecOptions::default()).unwrap();
    started(&driver, "SLEEP 200", 1);
    driver.server.drop_connections();
    driver.server.hold(false);
    let read = done(&engine, op);
    assert_eq!(read.status, OperationStatus::Done, "a read lost while running is run once more: {read:?}");
    assert_eq!(driver.server.executed().iter().filter(|statement| *statement == "SLEEP 200").count(), 2);

    driver.server.hold(true);
    let op = engine.execute("lost", ContextKind::Main, "SLOWSET a 2", ExecOptions::default()).unwrap();
    started(&driver, "SLOWSET a 2", 1);
    driver.server.drop_connections();
    driver.server.hold(false);
    let write = done(&engine, op);
    assert_eq!(write.status, OperationStatus::Failed);
    let error = write.error.unwrap();
    assert_eq!((error.kind, error.code.as_deref()), (ErrorKind::Connection, Some("notReplayed")));
    assert_eq!(driver.server.executed().iter().filter(|statement| *statement == "SLOWSET a 2").count(), 1);
    assert_eq!(engine.status("lost").state, ConnectionState::Connected, "the connection itself is open again");
    assert_eq!(run(&engine, "lost", ContextKind::Main, "GET a").execution.unwrap().rows[0][3], Value::Text("1".into()));
}

#[test]
fn a_reconnect_restores_only_tracked_state() {
    let (engine, driver) = engine(false);
    connected(&engine, request("state", ID, DEVELOPMENT));
    let tracked = TrackedState {
        schema: Some("sales".into()),
        isolation: Some("serializable".into()),
        auto_commit: true,
        ..TrackedState::default()
    };
    ok(&engine, engine.apply("state", ContextKind::Main, tracked.clone()).unwrap());
    driver.server.drop_connections();
    ok(&engine, engine.execute("state", ContextKind::Main, "GET a", ExecOptions::default()).unwrap());
    let reopened = driver.server.opened().last().cloned().unwrap();
    assert_eq!(reopened.context, ContextKind::Main);
    assert_eq!(reopened.state, tracked);
    assert!(engine.take_events().iter().any(|event| matches!(event, Event::Reconnected { .. })));
}

#[test]
fn reconnecting_gives_up_after_its_backoff() {
    let (engine, driver) = engine(false);
    connected(&engine, request("down", ID, DEVELOPMENT));
    driver.server.set_down(true);
    driver.server.drop_connections();
    let started = Instant::now();
    let failed = run(&engine, "down", ContextKind::Main, "GET a");
    assert_eq!(failed.error.unwrap().kind, ErrorKind::Connection);
    assert!(started.elapsed() >= Duration::from_millis(30), "waited through the backoff");
    driver.server.set_down(false);
    assert_eq!(run(&engine, "down", ContextKind::Main, "GET a").status, OperationStatus::Done);
}

#[test]
fn cancel_asks_the_server_and_ends_within_the_wait() {
    let (engine, driver) = engine(false);
    connected(&engine, request("cancel", ID, DEVELOPMENT));
    let op = engine.execute("cancel", ContextKind::Main, "SLEEP 20000", ExecOptions::default()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    assert_eq!(engine.cancel_connection("cancel", None), vec![op]);
    let state = done(&engine, op);
    if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
    }
    assert_eq!(state.status, OperationStatus::Cancelled);
    assert_eq!(state.error.unwrap().code, None);
    assert_eq!(driver.server.cancels(), 1);
    assert_eq!(run(&engine, "cancel", ContextKind::Main, "GET a").status, OperationStatus::Done, "the session goes on");

    driver.server.ignore_cancel(true);
    let op = engine.execute("cancel", ContextKind::Main, "SLEEP 20000", ExecOptions::default()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert!(engine.cancel(op));
    let state = done(&engine, op);
    assert_eq!(state.error.unwrap().code.as_deref(), Some("cancelUnconfirmed"));
    assert!(
        !engine.status("cancel").contexts.iter().find(|context| context.context == ContextKind::Main).unwrap().open
    );
    driver.server.ignore_cancel(false);
    driver.server.drop_connections();
    assert_eq!(run(&engine, "cancel", ContextKind::Main, "GET a").status, OperationStatus::Done);
}

#[test]
fn a_change_that_finishes_while_its_cancel_waits_reports_its_result() {
    let (engine, driver) = engine(false);
    connected(&engine, request("late", ID, DEVELOPMENT));
    let op = engine.execute("late", ContextKind::Main, "SLOWSET a 2", ExecOptions::default()).unwrap();
    wait_until(|| driver.server.executed().iter().any(|sent| sent == "SLOWSET a 2"));
    assert!(engine.cancel(op));
    let state = done(&engine, op);
    assert_eq!(state.status, OperationStatus::Done, "the change took effect: {:?}", state.error);
    assert_eq!(state.execution.unwrap().affected, Some(1));
    assert_eq!(run(&engine, "late", ContextKind::Main, "GET a").execution.unwrap().rows[0][3], Value::Text("2".into()));
    let read = engine.execute("late", ContextKind::Main, "SLEEP 20000", ExecOptions::default()).unwrap();
    wait_until(|| driver.server.executed().iter().any(|sent| sent == "SLEEP 20000"));
    assert!(engine.cancel(read));
    assert_eq!(done(&engine, read).status, OperationStatus::Cancelled, "a read the cancel ended stays cancelled");
}

#[test]
fn a_deadline_cancels_like_cancel() {
    let (engine, _driver) = engine(false);
    connected(&engine, request("deadline", ID, DEVELOPMENT));
    let options = ExecOptions { timeout: Some(Duration::from_millis(50)), ..ExecOptions::default() };
    let op = engine.execute("deadline", ContextKind::Main, "SLEEP 20000", options).unwrap();
    let state = done(&engine, op);
    assert_eq!(state.error.unwrap().kind, ErrorKind::Timeout);
}

#[test]
fn init_statements_that_write_never_run_on_a_read_only_connection() {
    let (engine, driver) = engine(false);
    let mut locked = request("init", ID, DEVELOPMENT);
    locked.source.read_only = true;
    locked.source.bootstrap.init_statements = vec!["GET a".to_owned(), "SET a 1".to_owned()];
    let state = done(&engine, engine.connect(locked.clone()).unwrap());
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    let error = state.error.unwrap();
    assert_eq!(
        (error.kind, error.code.as_deref(), error.detail.as_str()),
        (ErrorKind::Refused, Some("readOnly"), "SET")
    );
    assert!(driver.server.opened().is_empty(), "nothing was sent");
    locked.source.bootstrap.init_statements = vec!["GET a".to_owned()];
    connected(&engine, locked);
}

#[test]
fn disconnect_asks_about_open_transactions() {
    let (engine, driver) = engine(false);
    connected(&engine, request("tx", ID, PRODUCTION));
    run(&engine, "tx", ContextKind::Main, "SET a 1");
    assert_eq!(engine.open_transactions("tx"), vec![ContextKind::Main]);
    let refused = engine.disconnect("tx", TransactionPolicy::Ask).unwrap_err();
    assert_eq!((refused.kind, refused.detail.as_str()), (ErrorKind::OpenTransactions, "main"));
    assert_eq!(engine.disconnect_all(TransactionPolicy::Ask).unwrap_err().kind, ErrorKind::OpenTransactions);
    ok(&engine, engine.disconnect("tx", TransactionPolicy::Commit).unwrap());
    assert_eq!(engine.status("tx").state, ConnectionState::Disconnected);
    assert!(driver.server.executed().contains(&"EXEC".to_owned()));
    assert_eq!(
        engine.execute("tx", ContextKind::Main, "GET a", ExecOptions::default()).unwrap_err().kind,
        ErrorKind::NotConnected
    );
}

#[test]
fn a_pooler_never_holds_transactions() {
    let (engine, driver) = engine(false);
    let mut pooled = request("pool", ID, PRODUCTION);
    pooled.source.settings.insert(POOLER.to_owned(), true.into());
    connected(&engine, pooled);
    assert!(driver.server.opened().iter().all(|open| open.state.auto_commit));
    run(&engine, "pool", ContextKind::Main, "SET a 1");
    assert!(engine.open_transactions("pool").is_empty());
    assert!(engine.status("pool").pooler);
}

#[test]
fn idle_connections_close_and_reopen_on_next_use() {
    let (engine, driver) = engine(false);
    let mut idle = request("idle", ID, DEVELOPMENT);
    idle.source.overrides = TypeOverrides { close_idle_after: Some(1), ..TypeOverrides::default() };
    connected(&engine, idle);
    std::thread::sleep(Duration::from_millis(1100));
    runtime().block_on(engine.check_idle());
    assert_eq!(engine.status("idle").state, ConnectionState::Closed);
    assert_eq!(engine.status("idle").closed_because, Some(CloseReason::Idle));
    assert!(
        engine.take_events().contains(&Event::IdleClosed { connection: "idle".into(), name: "Name of idle".into() })
    );
    let before = driver.server.opened().len();
    assert_eq!(run(&engine, "idle", ContextKind::Main, "GET a").status, OperationStatus::Done);
    assert_eq!(engine.status("idle").state, ConnectionState::Connected);
    assert_eq!(driver.server.opened().len(), before + 1);
}

#[test]
fn idle_transactions_end_after_the_type_allows() {
    let (engine, _driver) = engine(false);
    let mut slow = request("slow", ID, PRODUCTION);
    slow.source.overrides = TypeOverrides { end_idle_transactions_after: Some(1), ..TypeOverrides::default() };
    connected(&engine, slow);
    run(&engine, "slow", ContextKind::Main, "SET a 1");
    std::thread::sleep(Duration::from_millis(1100));
    runtime().block_on(engine.check_idle());
    assert_eq!(engine.open_transactions("slow"), vec![ContextKind::Main]);
    let announced = engine.take_events();
    assert!(announced.iter().any(|event| matches!(event, Event::TransactionIdle { seconds: 0, .. })), "{announced:?}");
    let slow = request("slow", ID, PRODUCTION);
    assert!(engine.transaction(&slow, &ContextKind::Main, 10).ending_in_seconds.is_some());
    wait_until(|| engine.open_transactions("slow").is_empty());
    assert!(engine.take_events().iter().any(|event| matches!(event, Event::TransactionEnded { .. })));
}

#[test]
fn an_idle_transaction_kept_open_is_not_rolled_back() {
    let (engine, _driver) = engine(false);
    let mut slow = request("keep", ID, PRODUCTION);
    slow.source.overrides = TypeOverrides { end_idle_transactions_after: Some(1), ..TypeOverrides::default() };
    connected(&engine, slow);
    run(&engine, "keep", ContextKind::Main, "SET a 1");
    std::thread::sleep(Duration::from_millis(1100));
    runtime().block_on(engine.check_idle());
    assert!(engine.postpone("keep", &ContextKind::Main));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(engine.open_transactions("keep"), vec![ContextKind::Main]);
    assert!(!engine.take_events().iter().any(|event| matches!(event, Event::TransactionEnded { .. })));
    assert!(!engine.postpone("keep", &ContextKind::Main));
}

fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "the condition never held");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_transaction_log_counts_statements_and_changes_until_commit() {
    let (engine, driver) = engine(false);
    let prod = request("log", ID, PRODUCTION);
    connected(&engine, prod.clone());
    let main = ContextKind::Main;
    let idle = engine.transaction(&prod, &main, 10);
    assert_eq!(
        (idle.mode, idle.auto_commit, idle.in_transaction, idle.statements),
        (CommitMode::Manual, false, false, 0)
    );
    run(&engine, "log", main.clone(), "SET a 1");
    run(&engine, "log", main.clone(), "GET a");
    run(&engine, "log", main.clone(), "SET b 2");
    let open = engine.transaction(&prod, &main, 2);
    assert!(open.in_transaction);
    assert_eq!((open.statements, open.modifying, open.log.len(), open.truncated), (3, 2, 2, true));
    assert_eq!(open.log[1].summary, "SET b 2");
    assert_eq!(engine.pending().len(), 1);
    assert_eq!(engine.changes_pending("log"), vec![main.clone()]);
    ok(&engine, engine.end_transaction("log", main.clone(), true).unwrap());
    let ended = engine.transaction(&prod, &main, 10);
    assert_eq!((ended.in_transaction, ended.statements, ended.log.len()), (false, 0, 0));
    assert!(engine.pending().is_empty());
    assert!(driver.server.executed().contains(&"EXEC".to_owned()));
    run(&engine, "log", main.clone(), "SET c 3");
    ok(&engine, engine.end_transaction("log", main.clone(), false).unwrap());
    assert!(driver.server.executed().contains(&"DISCARD".to_owned()));
}

#[test]
fn smart_commit_switches_to_manual_on_a_change_and_back_after_commit() {
    let (engine, driver) = engine(false);
    let mut smart = request("smart", ID, DEVELOPMENT);
    smart.kind.smart_commit = true;
    connected(&engine, smart.clone());
    let main = ContextKind::Main;
    assert_eq!(engine.transaction(&smart, &main, 0).mode, CommitMode::Smart);
    run(&engine, "smart", main.clone(), "GET a");
    assert!(engine.transaction(&smart, &main, 0).auto_commit);
    run(&engine, "smart", main.clone(), "SET a 1");
    let switched = engine.transaction(&smart, &main, 0);
    assert_eq!((switched.mode, switched.auto_commit, switched.statements), (CommitMode::Smart, false, 1));
    assert!(driver.server.opened().len() >= 2);
    ok(&engine, engine.end_transaction("smart", main.clone(), true).unwrap());
    let back = engine.transaction(&smart, &main, 0);
    assert!(back.auto_commit && !back.in_transaction);
    assert!(engine.status("smart").auto_commit);
}

#[test]
fn smart_commit_goes_back_to_auto_when_the_switching_statement_fails() {
    let (engine, _driver) = engine(false);
    let mut smart = request("fails", ID, DEVELOPMENT);
    smart.kind.smart_commit = true;
    connected(&engine, smart.clone());
    let main = ContextKind::Main;
    let failed = run(&engine, "fails", main.clone(), "FAIL x");
    assert_eq!(failed.status, OperationStatus::Failed, "{failed:?}");
    let state = engine.transaction(&smart, &main, 0);
    assert!(state.auto_commit && !state.in_transaction, "{state:?}");
}

#[test]
fn the_mode_and_isolation_are_kept_per_context_and_leaving_manual_asks() {
    let (engine, driver) = engine(false);
    let dev = request("mode", ID, DEVELOPMENT);
    let console = ContextKind::Console("c/console".to_owned());
    let manual = ModeChange { mode: Some(CommitMode::Manual), isolation: None };
    assert_eq!(engine.set_mode("mode", console.clone(), manual.clone(), TransactionPolicy::Ask).unwrap(), None);
    connected(&engine, dev.clone());
    assert_eq!(engine.transaction(&dev, &console, 0).mode, CommitMode::Manual);
    assert_eq!(engine.transaction(&dev, &ContextKind::Main, 0).mode, CommitMode::Auto);
    run(&engine, "mode", console.clone(), "SET a 1");
    assert!(engine.transaction(&dev, &console, 0).in_transaction);
    let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
    let refused = engine.set_mode("mode", console.clone(), auto.clone(), TransactionPolicy::Ask).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::OpenTransactions);
    let op = engine.set_mode("mode", console.clone(), auto, TransactionPolicy::Commit).unwrap().unwrap();
    ok(&engine, op);
    let state = engine.transaction(&dev, &console, 0);
    assert!(state.auto_commit && !state.in_transaction && state.mode == CommitMode::Auto);
    assert!(driver.server.executed().contains(&"EXEC".to_owned()));
    let wrong = ModeChange { mode: None, isolation: Some(Some("chaotic".to_owned())) };
    assert_eq!(
        engine.set_mode("mode", console.clone(), wrong, TransactionPolicy::Ask).unwrap_err().kind,
        ErrorKind::Invalid
    );
    assert!(engine.set_mode("mode", ContextKind::Metadata, ModeChange::default(), TransactionPolicy::Ask).is_err());
}

#[test]
fn closing_a_console_asks_only_when_it_changed_data() {
    let (engine, _driver) = engine(false);
    connected(&engine, request("close", ID, PRODUCTION));
    let console = ContextKind::Console("c/one".to_owned());
    run(&engine, "close", console.clone(), "GET a");
    assert!(engine.close_context("close", console.clone(), TransactionPolicy::Ask).unwrap().is_some());
    run(&engine, "close", console.clone(), "SET a 1");
    let refused = engine.close_context("close", console.clone(), TransactionPolicy::Ask).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::OpenTransactions);
    let refused = engine.invalidate("close", TransactionPolicy::Ask).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::OpenTransactions);
    ok(&engine, engine.close_context("close", console.clone(), TransactionPolicy::Rollback).unwrap().unwrap());
    assert!(engine.open_transactions("close").is_empty());
    assert!(engine.close_context("close", ContextKind::Main, TransactionPolicy::Ask).is_err());
}

#[test]
fn sleep_closes_every_connection_until_next_use() {
    let (engine, _driver) = engine(false);
    connected(&engine, request("a", ID, DEVELOPMENT));
    connected(&engine, request("b", ID, DEVELOPMENT));
    assert_eq!(engine.suspend_all(CloseReason::Sleep), 2);
    assert_eq!(engine.status("a").closed_because, Some(CloseReason::Sleep));
    assert_eq!(run(&engine, "a", ContextKind::Main, "GET a").status, OperationStatus::Done);
    assert_eq!(engine.status("b").state, ConnectionState::Closed);
}

#[test]
fn invalidate_reopens_every_context() {
    let (engine, driver) = engine(false);
    connected(&engine, request("inv", ID, DEVELOPMENT));
    let state = ok(&engine, engine.invalidate("inv", TransactionPolicy::Ask).unwrap());
    assert!(state.reconnected);
    assert_eq!(driver.server.opened().len(), 4);
}

#[test]
fn invalidating_asks_before_it_loses_a_transaction_with_changes() {
    let (engine, driver) = engine(false);
    connected(&engine, request("inv", ID, PRODUCTION));
    run(&engine, "inv", ContextKind::Main, "SET a 1");
    let refused = engine.invalidate("inv", TransactionPolicy::Ask).unwrap_err();
    assert_eq!((refused.kind, refused.detail.as_str()), (ErrorKind::OpenTransactions, "main"));
    assert!(!driver.server.executed().contains(&"EXEC".to_owned()), "nothing was committed for the person");
    ok(&engine, engine.invalidate("inv", TransactionPolicy::Commit).unwrap());
    assert!(driver.server.executed().contains(&"EXEC".to_owned()));
}

#[test]
fn testing_settings_leaves_the_registry_alone() {
    let (engine, driver) = engine(false);
    ok(&engine, engine.test(request("t", ID, DEVELOPMENT)).unwrap());
    assert_eq!(engine.status("t").state, ConnectionState::Disconnected);
    driver.server.set_down(true);
    let failed = done(&engine, engine.test(request("t", ID, DEVELOPMENT)).unwrap());
    assert_eq!(failed.error.unwrap().kind, ErrorKind::Connection);
}

#[test]
fn a_saved_password_is_read_on_the_engines_runtime_and_only_when_needed() {
    let (engine, driver) = engine(false);
    driver.server.require_password("pw");
    let store = Arc::new(zen_secrets::MemoryStore::new());
    let id = zen_secrets::database::id(None, "kv", zen_secrets::database::Kind::Password).unwrap();
    let stored = StoredSecret { store: Arc::clone(&store) as Arc<dyn SecretStore>, id: id.clone() };
    let mut missing = request("kv", ID, DEVELOPMENT);
    missing.password = None;
    missing.stored = Some(stored.clone());
    let failed = done(&engine, engine.connect(missing.clone()).unwrap());
    assert_eq!(failed.error.map(|error| error.kind), Some(ErrorKind::PasswordNeeded));
    store.set(&id, &Secret::new("pw".to_owned())).unwrap();
    connected(&engine, missing.clone());
    assert_eq!(engine.status("kv").state, ConnectionState::Connected);
    let reads = store.accesses();
    let mut given = request("kv2", ID, DEVELOPMENT);
    given.stored = Some(stored.clone());
    connected(&engine, given);
    assert_eq!(store.accesses(), reads, "a given password does not read the store");
    store.deny(&id);
    let mut denied = request("kv3", ID, DEVELOPMENT);
    denied.password = None;
    denied.stored = Some(stored);
    let state = done(&engine, engine.test(denied).unwrap());
    assert_eq!(state.error.and_then(|error| error.code).as_deref(), Some("secretDenied"));
}

struct SilentStore {
    answer: Mutex<bool>,
    answered: std::sync::Condvar,
}

impl SilentStore {
    fn new() -> Self {
        Self { answer: Mutex::new(false), answered: std::sync::Condvar::new() }
    }

    fn answer(&self) {
        *self.answer.lock().unwrap() = true;
        self.answered.notify_all();
    }
}

impl SecretStore for SilentStore {
    fn get(&self, _: &SecretId) -> Result<Option<Secret>, SecretError> {
        let answer = self.answer.lock().unwrap();
        drop(self.answered.wait_while(answer, |answer| !*answer).unwrap());
        Ok(Some(Secret::new("pw")))
    }

    fn set(&self, _: &SecretId, _: &Secret) -> Result<(), SecretError> {
        Ok(())
    }

    fn delete(&self, _: &SecretId) -> Result<bool, SecretError> {
        Ok(false)
    }
}

#[test]
fn a_secret_store_that_never_answers_ends_connecting_with_a_timeout_and_can_be_cancelled() {
    let driver = Arc::new(MemoryDriver::new(false));
    driver.server.require_password("pw");
    let timing = Timing { secret_timeout: Duration::from_secs(2), ..timing() };
    let engine = Engine::with_timing(vec![Arc::clone(&driver) as Arc<dyn Driver>], "Zen Test", timing);
    let store = Arc::new(SilentStore::new());
    let id = zen_secrets::database::id(None, "silent", zen_secrets::database::Kind::Password).unwrap();
    let mut asked = request("silent", ID, DEVELOPMENT);
    asked.password = None;
    asked.stored = Some(StoredSecret { store: Arc::clone(&store) as Arc<dyn SecretStore>, id });

    let op = engine.connect(asked.clone()).unwrap();
    let deadline = Instant::now() + WAIT;
    while engine.status("silent").waiting_for.is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let status = engine.status("silent");
    assert_eq!((status.state, status.waiting_for), (ConnectionState::Connecting, Some(Waiting::Keychain)));
    let failed = done(&engine, op);
    let error = failed.error.unwrap();
    assert_eq!((error.kind, error.code.as_deref()), (ErrorKind::PasswordNeeded, Some("secretTimeout")));
    let status = engine.status("silent");
    assert_eq!((status.state, status.waiting_for), (ConnectionState::Failed, None));

    let again = engine.connect(asked.clone()).unwrap();
    assert!(engine.cancel(again));
    assert_eq!(done(&engine, again).error.unwrap().kind, ErrorKind::Cancelled);

    store.answer();
    connected(&engine, asked);
    assert_eq!(engine.status("silent").state, ConnectionState::Connected);
}

#[test]
fn secret_work_runs_off_the_callers_thread_in_order_and_reports_a_failure() {
    use zen_secrets::SecretStore as _;
    let (engine, driver) = engine(false);
    driver.server.require_password("pw");
    let secrets = Arc::new(zen_secrets::MemoryStore::new());
    let id = SecretId::new("space.ifzenelse.zen.test", "kv/password").unwrap();
    let (release, waiting) = std::sync::mpsc::channel::<()>();
    let caller = std::thread::current().id();
    let (store, item) = (Arc::clone(&secrets), id.clone());
    let op = engine.secret_work(OperationKind::StoreSecret, "kv", move || {
        assert_ne!(std::thread::current().id(), caller, "never on the caller's thread");
        let _ = waiting.recv();
        store.set(&item, &Secret::new("pw")).map_err(|error| error.to_string())
    });
    assert_eq!(engine.operation(op).unwrap().status, OperationStatus::Running);
    let mut asked = request("kv", "memory-kv", "dev");
    asked.password = None;
    asked.stored = Some(StoredSecret { store: secrets, id });
    let connect = engine.connect(asked).unwrap();
    release.send(()).unwrap();
    ok(&engine, op);
    ok(&engine, connect);

    let failed = engine.secret_work(OperationKind::DeleteSecrets, "kv", || Err("The Keychain said no.".to_owned()));
    let state = done(&engine, failed);
    let error = state.error.unwrap();
    assert_eq!((error.kind, error.detail.as_str()), (ErrorKind::Secret, "The Keychain said no."));
    assert!(engine.take_events().contains(&Event::SecretFailed {
        connection: "kv".to_owned(),
        stored: false,
        detail: "The Keychain said no.".to_owned()
    }));
}

fn statements(texts: &[&str]) -> Vec<QueryStatement> {
    texts
        .iter()
        .enumerate()
        .map(|(index, text)| QueryStatement {
            text: (*text).to_owned(),
            start: index * 100,
            end: index * 100 + text.len(),
            client: false,
            params: Vec::new(),
            kept: None,
        })
        .collect()
}

fn query_done(engine: &Engine, op: u64) -> QueryState {
    let deadline = Instant::now() + WAIT;
    loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running || Instant::now() > deadline {
            return query;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_query_runs_its_statements_and_keeps_their_results_and_output() {
    let (engine, _) = engine(false);
    connected(&engine, request("kv", ID, DEVELOPMENT));
    let console = ContextKind::Console("console".to_owned());
    let op = engine
        .run_query(
            "kv",
            console.clone(),
            statements(&["SET a 1", "NOTICE hello there", "ROWS 3", "GET a"]),
            QueryOptions::default(),
        )
        .unwrap();
    let query = query_done(&engine, op);
    assert_eq!(query.status, OperationStatus::Done, "{query:?}");
    assert_eq!(query.updated, 1);
    let kinds: Vec<StatementStatus> = query.statements.iter().map(|statement| statement.status).collect();
    assert_eq!(kinds, [StatementStatus::Done; 4]);
    assert_eq!(query.statements[0].result, None, "a statement without rows has no result");
    assert_eq!(query.statements[0].affected, Some(1));
    assert_eq!(query.messages.len(), 1);
    assert_eq!(query.messages[0].text, "hello there");
    assert_eq!(query.messages[0].statement, Some(1));
    let rows = query.statements[2].result.unwrap();
    let result = engine.results().get(rows).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.columns().len(), 5);
    assert_eq!(result.get(2, 0), Value::Integer(2));
    assert!(matches!(result.get(0, 4), Value::Document(_)));
    assert_eq!(result.info().state, crate::results::ReadState::Done);
    assert_eq!(query.statements[2].rows, Some(3));
    assert!(engine.results().get(query.statements[3].result.unwrap()).is_some());
}

#[test]
fn an_error_stops_the_script_rolls_back_and_says_where() {
    let (engine, driver) = engine(false);
    let mut manual = request("kv", ID, DEVELOPMENT);
    manual.source.overrides = TypeOverrides { auto_commit: Some(false), ..TypeOverrides::default() };
    connected(&engine, manual);
    let console = ContextKind::Console("c".to_owned());
    let op = engine
        .run_query("kv", console, statements(&["SET a 1", "FAIL here", "SET b 2"]), QueryOptions::default())
        .unwrap();
    let query = query_done(&engine, op);
    assert_eq!(query.status, OperationStatus::Failed);
    assert_eq!(query.statements[1].status, StatementStatus::Failed);
    let error = query.statements[1].error.clone().unwrap();
    assert_eq!(error.position, Some(6));
    assert_eq!(error.code.as_deref(), Some("42601"));
    assert_eq!(query.statements[2].status, StatementStatus::Cancelled, "the script stopped");
    assert!(query.messages.iter().any(|message| message.code.as_deref() == Some("rolledBack")));
    assert!(driver.server.executed().contains(&"DISCARD".to_owned()));
    assert!(!driver.server.executed().contains(&"SET b 2".to_owned()));
}

#[test]
fn going_on_after_errors_runs_every_statement() {
    let (engine, _) = engine(false);
    connected(&engine, request("kv", ID, DEVELOPMENT));
    let options = QueryOptions { stop_on_error: false, ..QueryOptions::default() };
    let op = engine.run_query("kv", ContextKind::Main, statements(&["FAIL x", "SET b 2"]), options).unwrap();
    let query = query_done(&engine, op);
    assert_eq!(query.statements[1].status, StatementStatus::Done);
}

#[test]
fn client_commands_are_not_sent() {
    let (engine, driver) = engine(false);
    connected(&engine, request("kv", ID, DEVELOPMENT));
    let mut list = statements(&["\\set x 1", "SET a 1"]);
    list[0].client = true;
    let query = query_done(&engine, engine.run_query("kv", ContextKind::Main, list, QueryOptions::default()).unwrap());
    assert_eq!(query.statements[0].status, StatementStatus::Skipped);
    assert!(!driver.server.executed().iter().any(|sent| sent.starts_with('\\')));
}

#[test]
fn a_statement_shows_a_result_only_once_it_returned_columns() {
    let (engine, _driver) = engine(false);
    connected(&engine, request("kv", ID, DEVELOPMENT));
    let op = engine
        .run_query("kv", ContextKind::Main, statements(&["SLOWSET a 1", "GET a"]), QueryOptions::default())
        .unwrap();
    loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            break;
        }
        if query.statements[0].status == StatementStatus::Running {
            assert_eq!(query.statements[0].result, None, "a running SET has no result yet");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let query = query_done(&engine, op);
    assert_eq!(query.statements[0].result, None);
    assert!(query.statements[1].result.is_some(), "GET returned a row");
}

#[test]
fn cancelling_a_query_stops_its_statement_on_the_server() {
    let (engine, driver) = engine(false);
    connected(&engine, request("kv", ID, DEVELOPMENT));
    let op = engine
        .run_query("kv", ContextKind::Main, statements(&["SLEEP 5000", "SET a 1"]), QueryOptions::default())
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(engine.cancel_query(op));
    let query = query_done(&engine, op);
    assert_eq!(query.status, OperationStatus::Cancelled, "{query:?}");
    assert_eq!(query.statements[0].status, StatementStatus::Cancelled);
    assert_eq!(query.statements[1].status, StatementStatus::Cancelled);
    assert!(driver.server.cancels() >= 1);
}

#[test]
fn a_query_on_a_connection_that_is_not_connected_fails() {
    let (engine, _) = engine(false);
    let op = engine.run_query("nowhere", ContextKind::Main, statements(&["PING"]), QueryOptions::default()).unwrap();
    let query = query_done(&engine, op);
    assert_eq!(query.status, OperationStatus::Failed);
    assert_eq!(query.error.map(|error| error.kind), Some(ErrorKind::NotConnected));
}

#[test]
fn rows_past_the_cap_are_read_again_on_fetch_next() {
    let (engine, _) = engine(false);
    engine.results().set_paging(crate::results::Paging { page: 10, row_cap: 25, keep_open: false });
    connected(&engine, request("kv", ID, DEVELOPMENT));
    let query = query_done(
        &engine,
        engine.run_query("kv", ContextKind::Main, statements(&["ROWS 60"]), QueryOptions::default()).unwrap(),
    );
    let id = query.statements[0].result.unwrap();
    let result = engine.results().get(id).unwrap();
    assert_eq!(result.len(), 25);
    assert!(result.info().more);
    let op = engine.fetch_more(id, Some(10)).unwrap().expect("read again");
    ok(&engine, op);
    assert_eq!(result.len(), 35);
    assert_eq!(result.get(34, 0), Value::Integer(34));
    let op = engine.fetch_more(id, None).unwrap().expect("read again");
    ok(&engine, op);
    assert_eq!(result.len(), 60);
    assert!(!result.info().more);
    assert_eq!(engine.fetch_more(id, None).unwrap(), None);
}

#[test]
fn fetching_more_never_runs_a_change_again() {
    let (engine, driver) = engine(false);
    engine.results().set_paging(crate::results::Paging { page: 10, row_cap: 25, keep_open: false });
    connected(&engine, request("kv", ID, DEVELOPMENT));
    let query = query_done(
        &engine,
        engine.run_query("kv", ContextKind::Main, statements(&["LPOP queue 60"]), QueryOptions::default()).unwrap(),
    );
    let id = query.statements[0].result.unwrap();
    let result = engine.results().get(id).unwrap();
    assert_eq!((result.len(), result.info().more), (25, true));
    let refused = engine.fetch_more(id, Some(10)).unwrap_err();
    assert_eq!((refused.kind, refused.code.as_deref()), (ErrorKind::Unsupported, Some(crate::results::NOT_REPEATABLE)));
    let refused = engine.fetch_more(id, None).unwrap_err();
    assert_eq!(refused.code.as_deref(), Some(crate::results::NOT_REPEATABLE));
    let popped = driver.server.executed().iter().filter(|sql| sql.starts_with("LPOP")).count();
    assert_eq!(popped, 1, "{:?}", driver.server.executed());
    assert_eq!(result.len(), 25);
}

#[test]
fn only_a_pageable_read_is_repeatable() {
    let sql = MemoryDriver::sql();
    assert!(sql.repeatable("SELECT * FROM orders WHERE id > 1"));
    assert!(!sql.repeatable("UPDATE orders SET note = 'x' RETURNING *"));
    assert!(!sql.repeatable("DELETE FROM orders RETURNING id"));
    assert!(!sql.repeatable("WITH gone AS (DELETE FROM orders RETURNING *) SELECT * FROM gone"));
    assert!(!sql.repeatable("SELECT * FROM orders FOR UPDATE"));
    assert!(!sql.repeatable("SHOW search_path"), "a read that is not pageable");
    let kv = MemoryDriver::new(false);
    assert!(kv.repeatable("ROWS 60"));
    assert!(!kv.repeatable("LPOP queue 60"));
}

#[test]
fn a_new_run_closes_what_the_last_one_left_open() {
    let (engine, _) = engine(false);
    connected(&engine, request("kv", ID, DEVELOPMENT));
    let console = ContextKind::Console("c".to_owned());
    let first = query_done(
        &engine,
        engine.run_query("kv", console.clone(), statements(&["ROWS 3"]), QueryOptions::default()).unwrap(),
    );
    let second =
        query_done(&engine, engine.run_query("kv", console, statements(&["ROWS 2"]), QueryOptions::default()).unwrap());
    assert_eq!(second.status, OperationStatus::Done);
    assert_ne!(first.statements[0].result, second.statements[0].result);
    assert!(engine.results().remove(first.statements[0].result.unwrap()));
}

fn sql_engine() -> (Engine, Arc<MemoryDriver>) {
    let driver = Arc::new(MemoryDriver::sql());
    let engine = Engine::with_timing(vec![Arc::clone(&driver) as Arc<dyn Driver>], "Zen Test", timing());
    (engine, driver)
}

fn customers() -> crate::schema::Object {
    use crate::schema::{Column, Constraint, ConstraintKind, Object, ObjectKind};
    let mut object = Object::new(ObjectKind::Table, "customers");
    object.oid = crate::memory::TABLE;
    object.columns = ["id", "name", "payload", "picture", "tags", "created"]
        .iter()
        .enumerate()
        .map(|(at, name)| Column {
            name: (*name).to_owned(),
            position: i32::try_from(at + 1).unwrap(),
            type_name: "text".to_owned(),
            nullable: at > 0,
            ..Column::default()
        })
        .collect();
    object.constraints = vec![Constraint {
        name: "customers_pkey".into(),
        kind: ConstraintKind::PrimaryKey,
        columns: vec!["id".into()],
        definition: String::new(),
        references: None,
        index: None,
        comment: None,
    }];
    object
}

fn read_rows(engine: &Engine, id: &str) -> Arc<crate::results::ResultSet> {
    let statement = QueryStatement {
        text: "SELECT * FROM customers".into(),
        start: 0,
        end: 23,
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let op = engine.run_query(id, ContextKind::Main, vec![statement], QueryOptions::default()).unwrap();
    ok(engine, op);
    let result = engine.query(op).unwrap().statements[0].result.unwrap();
    engine.results().get(result).unwrap()
}

#[test]
fn saving_runs_the_planned_statements_in_one_transaction_and_a_conflict_rolls_back() {
    use crate::dialect::SqlDialect;
    use crate::edit::{Staged, identify, plan};
    let (engine, driver) = sql_engine();
    connected(&engine, request("sql", crate::memory::SQL, DEVELOPMENT));
    let result = read_rows(&engine, "sql");
    let editing = identify(&result.columns(), "public", &customers(), None, SqlDialect::Postgres);
    assert!(editing.editable, "{editing:?}");
    result.stage(0, 1, Staged::Value(Value::Text("Ann".into())));
    result.delete_rows(&[1]);
    result.add_row(vec![(1, Staged::Value(Value::Text("New".into())))]);
    let planned = plan(&editing, &result.changes());
    let op = engine.save_edits(result.id(), planned).unwrap();
    let state = ok(&engine, op);
    let report = state.saved.unwrap();
    assert_eq!((report.deleted, report.inserted, report.updated, report.in_transaction), (1, 1, 1, false));
    let executed = driver.server.executed();
    let tail: Vec<&str> = executed.iter().rev().take(5).rev().map(String::as_str).collect();
    assert_eq!(tail[0], "BEGIN");
    assert!(tail[1].starts_with("DELETE FROM \"public\".\"customers\" WHERE \"id\" = $1"), "{tail:?}");
    assert!(tail[2].starts_with("INSERT INTO \"public\".\"customers\" (\"name\") VALUES ($1)"), "{tail:?}");
    assert!(tail[3].starts_with("UPDATE \"public\".\"customers\" SET \"name\" = $1 WHERE \"id\" = $2"), "{tail:?}");
    assert_eq!(tail[4], "COMMIT");
    let bound = driver.server.bound();
    assert_eq!(bound[0].1, [Some("2".to_owned())], "values are bound, never written into the text");
    assert!(!result.pending().any());
    assert_eq!((result.len(), result.get(0, 1)), (25, Value::Text("Ann".into())));

    driver.server.set_conflict(true);
    result.stage(0, 1, Staged::Value(Value::Text("Bob".into())));
    let planned = plan(&editing, &result.changes());
    let state = done(&engine, engine.save_edits(result.id(), planned).unwrap());
    assert_eq!(state.status, OperationStatus::Failed);
    assert_eq!(state.error.unwrap().code.as_deref(), Some("conflict"));
    let report = state.saved.unwrap();
    assert_eq!((report.failed_row, report.conflict), (Some(0), true));
    assert_eq!(driver.server.executed().last().map(String::as_str), Some("ROLLBACK"));
    assert!(result.pending().any(), "a failed save keeps the staged changes");
    driver.server.set_conflict(false);

    connected(&engine, request("sql", crate::memory::SQL, DEVELOPMENT));
    ok(&engine, engine.execute("sql", ContextKind::Main, "BEGIN", ExecOptions::default()).unwrap());
    driver.server.fail_on(Some("UPDATE"));
    let planned = plan(&editing, &result.changes());
    let state = done(&engine, engine.save_edits(result.id(), planned).unwrap());
    assert_eq!(state.error.unwrap().code.as_deref(), Some("23505"));
    let executed = driver.server.executed();
    let tail: Vec<&str> = executed.iter().rev().take(4).rev().map(String::as_str).collect();
    assert_eq!(tail[0], "SAVEPOINT zen_save", "inside the person's transaction a savepoint is used");
    assert_eq!(&tail[2..], ["ROLLBACK TO SAVEPOINT zen_save", "RELEASE SAVEPOINT zen_save"]);
    assert_eq!(engine.open_transactions("sql"), [ContextKind::Main], "the person's transaction stays open");
}

#[test]
fn grid_edits_saved_inside_a_transaction_count_as_changes_that_would_be_lost() {
    use crate::dialect::SqlDialect;
    use crate::edit::{Staged, identify, plan};
    let (engine, _driver) = sql_engine();
    let dev = request("pending", crate::memory::SQL, DEVELOPMENT);
    connected(&engine, dev.clone());
    let result = read_rows(&engine, "pending");
    let editing = identify(&result.columns(), "public", &customers(), None, SqlDialect::Postgres);
    ok(&engine, engine.execute("pending", ContextKind::Main, "BEGIN", ExecOptions::default()).unwrap());
    result.stage(0, 1, Staged::Value(Value::Text("Ann".into())));
    ok(&engine, engine.save_edits(result.id(), plan(&editing, &result.changes())).unwrap());
    let state = engine.transaction(&dev, &ContextKind::Main, 10);
    assert!(state.in_transaction && state.modifying == 1 && state.statements == 1, "{state:?}");
    assert!(state.log[0].summary.starts_with("UPDATE"), "{state:?}");
    assert_eq!(engine.changes_pending("pending"), [ContextKind::Main]);
    let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
    let refused = engine.set_mode("pending", ContextKind::Main, auto, TransactionPolicy::Ask).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::OpenTransactions);
    assert_eq!(engine.disconnect("pending", TransactionPolicy::Ask).unwrap_err().kind, ErrorKind::OpenTransactions);
}

#[test]
fn cancelling_a_count_asks_the_server_and_never_keeps_a_session_mid_statement() {
    let (engine, driver) = sql_engine();
    connected(&engine, request("count", crate::memory::SQL, DEVELOPMENT));
    let result = read_rows(&engine, "count");
    driver.server.hold(true);
    let op = engine.count_rows(result.id()).unwrap();
    wait_until(|| driver.server.executed().iter().any(|sent| sent.starts_with("SELECT count(*)")));
    assert!(engine.cancel(op));
    let state = done(&engine, op);
    driver.server.hold(false);
    assert_eq!(state.status, OperationStatus::Cancelled, "{state:?}");
    assert_eq!(driver.server.cancels(), 1, "the server was asked to stop");
    let main = engine.status("count").contexts.into_iter().find(|context| context.context == ContextKind::Main);
    assert!(!main.unwrap().open, "a session that never stopped is closed, not reused");
    assert_eq!(run(&engine, "count", ContextKind::Main, "SELECT 1").status, OperationStatus::Done);
}

#[test]
fn a_lost_connection_never_runs_a_select_of_a_function_twice() {
    let (engine, driver) = sql_engine();
    connected(&engine, request("replay", crate::memory::SQL, DEVELOPMENT));
    let lose_while_running = |sql: &str| {
        driver.server.hold(true);
        let op = engine.execute("replay", ContextKind::Main, sql, ExecOptions::default()).unwrap();
        wait_until(|| driver.server.executed().iter().any(|sent| sent == sql));
        driver.server.drop_connections();
        driver.server.hold(false);
        let state = done(&engine, op);
        (state, driver.server.executed().iter().filter(|sent| *sent == sql).count())
    };
    let (read, sent) = lose_while_running("SELECT * FROM customers");
    assert_eq!((read.status, sent), (OperationStatus::Done, 2), "a plain read runs once more: {:?}", read.error);
    let (call, sent) = lose_while_running("SELECT zen_archive_orders()");
    assert_eq!((call.status, sent), (OperationStatus::Failed, 1), "a function may have written: {:?}", call.error);
    assert_eq!(call.error.unwrap().code.as_deref(), Some("notReplayed"));
}

#[test]
fn bound_statements_send_their_values_and_read_again_with_them() {
    let (engine, driver) = sql_engine();
    connected(&engine, request("sql", crate::memory::SQL, DEVELOPMENT));
    engine.results().set_paging(crate::results::Paging { page: 10, row_cap: 10, keep_open: false });
    let text = "SELECT * FROM customers WHERE id > $1";
    let statement = QueryStatement {
        text: text.into(),
        start: 0,
        end: text.len(),
        client: false,
        params: vec![crate::params::NamedValue { name: "id".into(), value: Value::Integer(0) }],
        kept: None,
    };
    let op = engine.run_query("sql", ContextKind::Main, vec![statement], QueryOptions::default()).unwrap();
    ok(&engine, op);
    let bound = driver.server.bound();
    assert_eq!(bound.last().unwrap(), &(text.to_owned(), vec![Some("0".to_owned())]));
    let id = engine.query(op).unwrap().statements[0].result.unwrap();
    let result = engine.results().get(id).unwrap();
    assert_eq!(result.statement_params(), [Value::Integer(0)]);
    assert_eq!(result.len(), 10);
    let again = engine.fetch_more(id, None).unwrap().expect("read again past the cap");
    ok(&engine, again);
    assert_eq!(driver.server.bound().len(), bound.len() + 1, "the values are bound when it is read again");
    assert_eq!(result.len(), 25);
}

#[test]
fn a_query_whose_statement_was_cancelled_reads_as_cancelled_before_its_task_ends() {
    let (engine, _) = engine(false);
    let statement = StatementState {
        index: 0,
        start: 0,
        end: 10,
        summary: "SLEEP 5000".to_owned(),
        status: StatementStatus::Cancelled,
        execution_ms: 0,
        fetch_ms: 0,
        affected: None,
        rows: None,
        result: None,
        error: Some(DbError::new(ErrorKind::Cancelled, "")),
    };
    let query = QueryState {
        id: 77,
        connection: "kv".to_owned(),
        context: ContextKind::Console("console.sql".to_owned()),
        status: OperationStatus::Running,
        elapsed_ms: 0,
        statements: vec![statement],
        messages: Vec::new(),
        updated: 0,
        error: None,
        started: Some(Instant::now()),
    };
    engine.state().queries.insert(77, query);
    assert_eq!(engine.query(77).unwrap().status, OperationStatus::Cancelled, "never `done` with nothing run");
}

#[test]
fn a_detected_profile_is_said_once_and_never_changes_the_connection() {
    let (engine, _) = engine(false);
    connected(&engine, request("roach", ID, DEVELOPMENT));
    engine.take_events();
    engine.detected("roach", None, Some("cockroachdb".to_owned()));
    engine.detected("roach", None, Some("cockroachdb".to_owned()));
    let events = engine.take_events();
    let said: Vec<&Event> = events.iter().filter(|event| matches!(event, Event::ProfileDetected { .. })).collect();
    assert_eq!(
        said,
        [&Event::ProfileDetected {
            connection: "roach".to_owned(),
            name: "Name of roach".to_owned(),
            profile: "cockroachdb".to_owned()
        }]
    );
    assert_eq!(engine.status("roach").detected_profile.as_deref(), Some("cockroachdb"));
    let status = serde_json::to_value(engine.status("roach")).unwrap();
    assert_eq!(status["detectedProfile"], "cockroachdb");
    engine.detected("roach", Some("cockroachdb"), Some("cockroachdb".to_owned()));
    assert_eq!(engine.status("roach").detected_profile, None, "the connection's own profile is not news");
    assert!(engine.take_events().is_empty());
}
