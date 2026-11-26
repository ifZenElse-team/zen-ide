use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::history::{HistorySink, Provenance, Ran};
use crate::memory::{ID, MemoryDriver, SQL};
use crate::registry::{AuthKind, TypeOverrides};
use crate::types::{DEVELOPMENT, PRODUCTION};

const WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Kept(Mutex<Vec<Ran>>);

impl HistorySink for Kept {
    fn record(&self, ran: Ran) {
        self.0.lock().unwrap().push(ran);
    }
}

fn engine() -> (Engine, Arc<MemoryDriver>, Arc<MemoryDriver>, Arc<Kept>) {
    let kv = Arc::new(MemoryDriver::new(false));
    let sql = Arc::new(MemoryDriver::sql());
    let drivers: Vec<Arc<dyn Driver>> = vec![Arc::clone(&kv) as _, Arc::clone(&sql) as _];
    let engine = Engine::new(drivers, "Zen Test");
    let kept = Arc::new(Kept::default());
    engine.set_history(Some(Arc::clone(&kept) as Arc<dyn HistorySink>));
    (engine, kv, sql, kept)
}

fn request(driver: &str, kind: &str, read_only: bool) -> ConnectRequest {
    let source = DataSource {
        id: "db".to_owned(),
        name: "Shop".to_owned(),
        driver: driver.to_owned(),
        connection_type: kind.to_owned(),
        auth: AuthKind::Password,
        read_only,
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == kind).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("pw".to_owned())), stored: None, ssh_secret: None }
}

fn connected(engine: &Engine, request: ConnectRequest) {
    let op = engine.connect(request).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn query(engine: &Engine, context: ContextKind, texts: &[&str], options: QueryOptions) -> QueryState {
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
    let op = engine.run_query("db", context, statements, options).unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        let state = engine.query(op).unwrap();
        if state.status != OperationStatus::Running || Instant::now() > deadline {
            return state;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn console() -> ContextKind {
    ContextKind::Console("db/console".to_owned())
}

fn refused(state: &QueryState, index: usize) -> bool {
    state.statements[index].error.as_ref().is_some_and(|error| error.kind == ErrorKind::Refused)
}

#[test]
fn a_read_only_connection_refuses_changes_before_the_server_sees_them_and_records_them() {
    let (engine, _, sql, kept) = engine();
    connected(&engine, request(SQL, DEVELOPMENT, true));
    let ran =
        query(&engine, console(), &["SELECT 1", "UPDATE orders SET paid = 1", "SELECT 2"], QueryOptions::default());
    assert_eq!(ran.status, OperationStatus::Failed, "{ran:?}");
    assert_eq!(ran.statements[0].status, StatementStatus::Done);
    let error = ran.statements[1].error.clone().unwrap();
    assert_eq!(
        (error.kind, error.code.as_deref(), error.detail.as_str()),
        (ErrorKind::Refused, Some("readOnly"), "UPDATE")
    );
    assert_eq!(ran.statements[2].status, StatementStatus::Cancelled);
    assert!(!sql.server.executed().iter().any(|text| text.starts_with("UPDATE")), "{:?}", sql.server.executed());
    let opened = sql.server.opened();
    assert!(opened.iter().all(|session| session.state.read_only), "every session is read-only on the server");
    let refused: Vec<_> = kept.0.lock().unwrap().iter().filter(|ran| ran.error.is_some()).cloned().collect();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].text, "UPDATE orders SET paid = 1");
}

#[test]
fn a_driver_that_judges_writes_itself_keeps_judging_them_on_a_read_only_connection() {
    let (engine, kv, _, _) = engine();
    connected(&engine, request(ID, DEVELOPMENT, true));
    let ran = query(&engine, console(), &["SET a 1"], QueryOptions::default());
    let error = ran.statements[0].error.clone().unwrap();
    assert_eq!(error.kind, ErrorKind::Server, "the server refused it: {error:?}");
    assert!(kv.server.opened().iter().all(|session| session.state.read_only));
}

#[test]
fn a_read_only_type_makes_its_connections_read_only() {
    let (engine, _, sql, _) = engine();
    let mut request = request(SQL, PRODUCTION, false);
    request.kind.read_only = true;
    connected(&engine, request);
    assert!(engine.read_only("db"));
    let ran = query(&engine, console(), &["TRUNCATE orders"], QueryOptions::default());
    assert!(refused(&ran, 0), "{ran:?}");
    assert!(sql.server.executed().iter().all(|text| text != "TRUNCATE orders"));
}

#[test]
fn a_query_run_read_only_turns_its_session_read_only_on_the_server() {
    let (engine, kv, _, _) = engine();
    connected(&engine, request(ID, DEVELOPMENT, false));
    let agent = ContextKind::Console("agent".to_owned());
    let options = QueryOptions { read_only: true, ..QueryOptions::default() };
    let ran = query(&engine, agent.clone(), &["GET a"], options.clone());
    assert_eq!(ran.status, OperationStatus::Done, "{ran:?}");
    let ran = query(&engine, agent.clone(), &["SET a 1"], options);
    assert_eq!(ran.statements[0].error.as_ref().and_then(|error| error.code.as_deref()), Some("readOnly"));
    assert!(kv.server.executed().iter().all(|text| text != "SET a 1"));
    let ran = query(&engine, agent, &["SET a 1"], QueryOptions::default());
    assert_eq!(ran.status, OperationStatus::Done, "allowed to change again, the session is writable again: {ran:?}");
    let ran = query(&engine, console(), &["SET b 1"], QueryOptions::default());
    assert_eq!(ran.status, OperationStatus::Done, "other sessions are not touched: {ran:?}");
}

#[test]
fn read_only_changes_while_connected() {
    let (engine, _, sql, _) = engine();
    connected(&engine, request(SQL, DEVELOPMENT, false));
    let sent = |text: &str| sql.server.executed().iter().filter(|known| *known == text).count();
    query(&engine, console(), &["UPDATE t SET a = 1"], QueryOptions::default());
    assert_eq!(sent("UPDATE t SET a = 1"), 1);
    for op in engine.set_read_only("db", true) {
        assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
    }
    assert!(engine.read_only("db"));
    assert!(refused(&query(&engine, console(), &["UPDATE t SET a = 1"], QueryOptions::default()), 0));
    let fresh = ContextKind::Console("db/console_1".to_owned());
    assert!(refused(&query(&engine, fresh.clone(), &["UPDATE t SET a = 1"], QueryOptions::default()), 0));
    assert_eq!(sent("UPDATE t SET a = 1"), 1, "nothing reached the server while locked");
    for op in engine.set_read_only("db", false) {
        engine.wait(op, WAIT).unwrap();
    }
    query(&engine, console(), &["UPDATE t SET a = 1"], QueryOptions::default());
    query(&engine, fresh, &["UPDATE t SET a = 1"], QueryOptions::default());
    assert_eq!(sent("UPDATE t SET a = 1"), 3);
}

#[test]
fn a_new_connection_type_applies_to_the_open_sessions_at_once() {
    let (engine, _, sql, _) = engine();
    connected(&engine, request(SQL, DEVELOPMENT, false));
    query(&engine, console(), &["SELECT 1"], QueryOptions::default());
    assert!(engine.status("db").auto_commit, "Development commits on its own");
    let production = ConnectionType::built_in().into_iter().find(|known| known.id == PRODUCTION).unwrap();
    let (operations, kept) = engine.set_kind("db", production, TypeOverrides::default());
    assert!(kept.is_empty() && !operations.is_empty(), "{kept:?}");
    for op in operations {
        assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
    }
    assert!(!engine.status("db").auto_commit, "Production's manual commit applies without a reconnect");
    let request = request(SQL, PRODUCTION, false);
    assert!(!engine.transaction(&request, &console(), 0).auto_commit, "the console's session too");
    let before = sql.server.executed().len();
    query(&engine, console(), &["UPDATE t SET a = 1"], QueryOptions::default());
    assert!(engine.transaction(&request, &console(), 0).in_transaction, "the change waits for Commit");
    let after: Vec<String> = sql.server.executed().into_iter().skip(before).collect();
    assert!(!after.iter().any(|text| text.starts_with("COMMIT")), "{after:?}");
    let development = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let (operations, kept) = engine.set_kind("db", development, TypeOverrides::default());
    assert_eq!(kept, vec![console()], "a transaction with changes keeps manual commit");
    for op in operations {
        engine.wait(op, WAIT).unwrap();
    }
    assert!(engine.status("db").auto_commit, "the main session follows the type");
    assert!(!engine.transaction(&request, &console(), 0).auto_commit, "the console keeps its open changes");
    assert!(!sql.server.executed().iter().skip(before).any(|text| text.starts_with("COMMIT")), "nothing was committed");
}

#[test]
fn refused_statements_are_recorded_with_their_provenance() {
    let (engine, _, _, kept) = engine();
    connected(&engine, request(ID, PRODUCTION, false));
    let provenance = Provenance { agent: true, ..Provenance::of(crate::history::Source::Query) };
    let error = DbError::new(ErrorKind::Refused, "SET").with_code("agentReadOnly");
    engine.record_refused(
        "kv",
        &ContextKind::Console("agent".to_owned()),
        &["SET a 1".to_owned()],
        &error,
        &provenance,
    );
    let kept = kept.0.lock().unwrap();
    assert_eq!(kept.len(), 1);
    assert!(kept[0].provenance.agent);
    assert_eq!(kept[0].error.as_ref().and_then(|error| error.code.as_deref()), Some("agentReadOnly"));
}

#[test]
fn a_built_in_connection_type_is_never_removed() {
    let mut types = crate::types::ConnectionTypes::load(std::path::Path::new(""));
    assert!(!types.remove(PRODUCTION));
    assert!(types.get(PRODUCTION).is_some());
    types.put(ConnectionType { id: "mine".to_owned(), ..types.resolve(DEVELOPMENT) });
    assert!(types.remove("mine"));
}
