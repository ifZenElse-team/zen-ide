use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::history::{HistorySink, Provenance, Ran, Source};
use crate::memory::{MemoryDriver, MemoryServer, SQL};
use crate::registry::AuthKind;
use crate::sessions::{OWN_SESSION, ServerLock, ServerSession, SessionEnd, SessionsRequest};
use crate::types::DEVELOPMENT;

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

fn connected(engine: &Engine, driver: &str, read_only: bool) {
    let source = DataSource {
        id: "db".to_owned(),
        name: "Shop".to_owned(),
        driver: driver.to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        auth: AuthKind::Password,
        read_only,
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let request =
        ConnectRequest { source, kind, password: Some(Secret::new("pw".to_owned())), stored: None, ssh_secret: None };
    let state = engine.wait(engine.connect(request).unwrap(), WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn others() -> (Vec<ServerSession>, Vec<ServerLock>) {
    let session = |id: &str, state: &str, blocked_by: &[&str], query: &str| ServerSession {
        id: id.to_owned(),
        user: Some("ann".to_owned()),
        state: Some(state.to_owned()),
        query: Some(query.to_owned()),
        blocked_by: blocked_by.iter().map(|id| (*id).to_owned()).collect(),
        ..ServerSession::default()
    };
    let sessions = vec![
        session("41", "idle in transaction", &[], "UPDATE orders SET paid = true WHERE id = 7"),
        session("42", "active", &["41"], "UPDATE orders SET paid = false WHERE id = 7"),
        session("43", "idle", &[], "ALTER ROLE ann PASSWORD 'hunter2'"),
    ];
    let locks = vec![ServerLock {
        session: "42".to_owned(),
        kind: "transactionid".to_owned(),
        mode: "ShareLock".to_owned(),
        granted: false,
        object: Some("1234".to_owned()),
        blocked_by: vec!["41".to_owned()],
    }];
    (sessions, locks)
}

fn listed(engine: &Engine, request: SessionsRequest) -> OperationState {
    engine.wait(engine.sessions("db", request).unwrap(), WAIT).unwrap()
}

#[test]
fn sessions_are_listed_with_chains_own_sessions_marked_and_passwords_masked() {
    let (engine, _, sql, _) = engine();
    let (sessions, locks) = others();
    sql.server.set_sessions(sessions, locks);
    assert_eq!(engine.sessions("db", SessionsRequest::default()).unwrap_err().kind, ErrorKind::NotConnected);
    connected(&engine, SQL, false);
    let state = listed(&engine, SessionsRequest::default());
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    let list = state.sessions.unwrap();
    let ids: Vec<&str> = list.sessions.iter().map(|session| session.id.as_str()).collect();
    assert_eq!(&ids[..3], ["41", "42", "43"]);
    assert_eq!(list.chains, vec![vec!["42".to_owned(), "41".to_owned()]]);
    assert_eq!(list.sessions[0].blocking, ["42"]);
    assert_eq!(list.locks.len(), 1);
    assert!(!list.sessions[2].query.as_deref().unwrap().contains("hunter2") && list.sessions[2].redacted);
    let own: Vec<&ServerSession> = list.sessions.iter().filter(|session| session.own).collect();
    assert!(!own.is_empty(), "{list:?}");
    assert!(own.iter().any(|session| session.context.as_deref() == Some("metadata")), "{own:?}");
    assert_eq!(own.len(), engine.own_sessions("db").len());

    let busy = listed(&engine, SessionsRequest { show_idle: false, locks: false, limit: 1, text_limit: 10 });
    let list = busy.sessions.unwrap();
    assert_eq!((list.sessions.len(), list.total, list.truncated), (1, 2, true));
    assert_eq!(list.sessions[0].query.as_deref(), Some("UPDATE ord"));
    assert!(list.sessions[0].query_cut && list.locks.is_empty());
}

#[test]
fn ending_a_session_touches_only_that_session_and_is_recorded() {
    let (engine, _, sql, kept) = engine();
    let (sessions, locks) = others();
    sql.server.set_sessions(sessions, locks);
    connected(&engine, SQL, false);
    let provenance = Provenance::of(Source::Session);
    let op = engine.end_session("db", "41", SessionEnd::Terminate, false, provenance.clone()).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    let ended = state.ended.unwrap();
    assert!(ended.ended);
    assert_eq!(ended.statement, "SELECT pg_terminate_backend(41)");
    let left = listed(&engine, SessionsRequest::default()).sessions.unwrap();
    assert!(left.sessions.iter().all(|session| session.id != "41"));
    assert!(
        left.sessions.iter().any(|session| session.id == "42")
            && left.sessions.iter().any(|session| session.id == "43")
    );
    let recorded = kept.0.lock().unwrap().clone();
    let last = recorded.last().unwrap();
    assert_eq!((last.text.as_str(), last.provenance.source), ("SELECT pg_terminate_backend(41)", Source::Session));
    assert_eq!(sql.server.executed().iter().filter(|text| text.contains("pg_terminate_backend")).count(), 1);

    let op = engine.end_session("db", "77", SessionEnd::CancelQuery, false, provenance.clone()).unwrap();
    let gone = engine.wait(op, WAIT).unwrap().ended.unwrap();
    assert!(!gone.ended, "a session the server does not know is reported, not ended");

    let (_, own) = engine.own_sessions("db").remove(0);
    let refused = engine.end_session("db", &own, SessionEnd::Terminate, false, provenance.clone()).unwrap_err();
    assert_eq!((refused.kind, refused.code.as_deref()), (ErrorKind::Refused, Some(OWN_SESSION)));
    let invalid = engine.end_session("db", "41; DROP TABLE t", SessionEnd::Terminate, false, provenance).unwrap_err();
    assert_eq!(invalid.kind, ErrorKind::Invalid);
    assert_eq!(MemoryServer::session_id(0), "900");
}

#[test]
fn a_read_only_connection_refuses_ending_sessions_and_other_drivers_are_unsupported() {
    let (engine, _, sql, _) = engine();
    let (sessions, locks) = others();
    sql.server.set_sessions(sessions, locks);
    connected(&engine, SQL, true);
    let refused =
        engine.end_session("db", "41", SessionEnd::CancelQuery, false, Provenance::of(Source::Session)).unwrap_err();
    assert_eq!((refused.kind, refused.code.as_deref()), (ErrorKind::Refused, Some("readOnly")));
    assert!(sql.server.executed().iter().all(|text| !text.contains("pg_cancel_backend")));
    assert_eq!(listed(&engine, SessionsRequest::default()).status, OperationStatus::Done);

    let (other, _, _, _) = self::engine();
    connected(&other, crate::memory::ID, false);
    assert_eq!(other.sessions("db", SessionsRequest::default()).unwrap_err().kind, ErrorKind::Unsupported);
}
