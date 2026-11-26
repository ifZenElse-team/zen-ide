use std::time::{Duration, Instant};

use zen_db::driver::{ContextKind, ErrorKind, ExecOptions};
use zen_db::engine::{ConnectRequest, Engine, OperationState, OperationStatus, Timing};
use zen_db::history::{Provenance, Source};
use zen_db::registry::DataSource;
use zen_db::sessions::{OWN_SESSION, ServerSessions, SessionEnd, SessionsRequest};
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(30);

struct Server {
    driver: &'static str,
    port: u16,
}

fn servers() -> Vec<Server> {
    let postgres = std::env::var("ZEN_DB_PG_PORTS").unwrap_or_else(|_| "54315,54318".to_owned());
    let mysql = std::env::var("ZEN_DB_MYSQL_SERVERS").unwrap_or_else(|_| "mysql:33084,mariadb:33114".to_owned());
    let mut found: Vec<Server> = postgres
        .split(',')
        .filter_map(|port| Some(Server { driver: "postgresql", port: port.trim().parse().ok()? }))
        .collect();
    found.extend(mysql.split(',').filter_map(|pair| {
        let (driver, port) = pair.trim().split_once(':')?;
        let driver = if driver == "mariadb" { "mariadb" } else { "mysql" };
        Some(Server { driver, port: port.parse().ok()? })
    }));
    found
}

fn new_engine(name: &str) -> Engine {
    Engine::with_timing(
        zen_db::drivers(),
        name,
        Timing { backoff: [0, 100, 300].map(Duration::from_millis).to_vec(), ..Timing::default() },
    )
}

fn request(id: &str, server: &Server) -> ConnectRequest {
    let postgres = server.driver == "postgresql";
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: server.driver.to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(server.port),
        database: postgres.then(|| "postgres".to_owned()),
        user: Some(if postgres { "postgres" } else { "root" }.to_owned()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None }
}

fn ok(engine: &Engine, op: u64) -> OperationState {
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state
}

fn run(engine: &Engine, id: &str, context: ContextKind, sql: &str) {
    ok(engine, engine.execute(id, context, sql, ExecOptions::default()).unwrap());
}

fn console(name: &str) -> ContextKind {
    ContextKind::Console(name.to_owned())
}

fn list(engine: &Engine, id: &str, request: SessionsRequest) -> ServerSessions {
    ok(engine, engine.sessions(id, request).unwrap()).sessions.unwrap()
}

fn server_id(engine: &Engine, id: &str, context: &ContextKind) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some((_, server)) = engine.own_sessions(id).into_iter().find(|(known, _)| known == context) {
            return server;
        }
        assert!(Instant::now() < deadline, "{context} never opened");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn all() -> SessionsRequest {
    SessionsRequest { limit: 1000, ..SessionsRequest::default() }
}

#[test]
#[ignore = "needs PostgreSQL, MySQL and MariaDB in Docker (docs/development.md)"]
fn blocking_chains_are_listed_and_only_the_chosen_session_is_ended() {
    for server in servers() {
        let postgres = server.driver == "postgresql";
        let table = if postgres { "zen_sessions" } else { "zen_sessions.t" };
        let watcher = new_engine("Zen Sessions Watcher");
        ok(&watcher, watcher.connect(request("w", &server)).unwrap());
        let clients = new_engine("Zen Sessions Clients");
        ok(&clients, clients.connect(request("c", &server)).unwrap());
        if !postgres {
            run(&clients, "c", ContextKind::Main, "CREATE DATABASE IF NOT EXISTS zen_sessions");
        }
        run(&clients, "c", ContextKind::Main, &format!("DROP TABLE IF EXISTS {table}"));
        run(&clients, "c", ContextKind::Main, &format!("CREATE TABLE {table} (id int primary key, v int)"));
        run(&clients, "c", ContextKind::Main, &format!("INSERT INTO {table} VALUES (1, 0)"));
        let (holder, waiter) = (console("holder"), console("waiter"));
        run(&clients, "c", holder.clone(), if postgres { "BEGIN" } else { "START TRANSACTION" });
        run(&clients, "c", holder.clone(), &format!("UPDATE {table} SET v = 1 WHERE id = 1"));
        let waiting = clients
            .execute("c", waiter.clone(), &format!("UPDATE {table} SET v = 2 WHERE id = 1"), ExecOptions::default())
            .unwrap();
        let (holder_id, waiter_id) = (server_id(&clients, "c", &holder), server_id(&clients, "c", &waiter));

        let deadline = Instant::now() + WAIT;
        let listed = loop {
            let listed = list(&watcher, "w", all());
            let blocked =
                listed.sessions.iter().any(|session| session.id == waiter_id && !session.blocked_by.is_empty());
            if blocked {
                break listed;
            }
            assert!(Instant::now() < deadline, "{}: the waiter is never listed as blocked: {listed:?}", server.driver);
            std::thread::sleep(Duration::from_millis(100));
        };
        let waiting_session = listed.sessions.iter().find(|session| session.id == waiter_id).unwrap();
        assert_eq!(
            waiting_session.blocked_by,
            std::slice::from_ref(&holder_id),
            "{}: {waiting_session:?}",
            server.driver
        );
        assert!(waiting_session.query.as_deref().unwrap_or_default().contains("SET v = 2"), "{waiting_session:?}");
        let holding = listed.sessions.iter().find(|session| session.id == holder_id).unwrap();
        assert_eq!(holding.blocking, std::slice::from_ref(&waiter_id));
        assert!(listed.chains.contains(&vec![waiter_id.clone(), holder_id.clone()]), "{:?}", listed.chains);
        assert!(listed.locks_error.is_none(), "{}: {:?}", server.driver, listed.locks_error);
        let waited = listed.locks.iter().find(|lock| lock.session == waiter_id && !lock.granted);
        assert!(
            waited.is_some_and(|lock| lock.blocked_by == [holder_id.clone()]),
            "{}: {:?}",
            server.driver,
            listed.locks
        );
        assert!(listed.locks.iter().any(|lock| lock.session == holder_id && lock.granted), "{:?}", listed.locks);
        let watcher_own: Vec<_> = listed.sessions.iter().filter(|session| session.own).collect();
        assert!(watcher_own.iter().any(|session| session.context.as_deref() == Some("metadata")), "{watcher_own:?}");
        assert!(listed.sessions.iter().filter(|session| session.id == holder_id).all(|session| !session.own));
        if postgres {
            assert_eq!(holding.state.as_deref(), Some("idle in transaction"));
            assert_eq!(holding.application.as_deref(), Some("Zen Sessions Clients"));
            assert!(waiting_session.query_start.is_some() && waiting_session.wait.is_some(), "{waiting_session:?}");
        } else {
            assert_eq!(holding.command.as_deref(), Some("Sleep"));
            assert!(waiting_session.seconds.is_some() && waiting_session.host.is_some(), "{waiting_session:?}");
        }
        let idle = server_id(&watcher, "w", &ContextKind::Main);
        let busy = list(&watcher, "w", SessionsRequest { show_idle: false, ..all() });
        assert!(busy.sessions.iter().any(|session| session.id == waiter_id));
        assert!(
            busy.sessions.iter().any(|session| session.id == holder_id),
            "a holder blocking others is kept: {busy:?}"
        );
        assert!(busy.sessions.iter().all(|session| session.id != idle), "an idle session is left out: {busy:?}");

        let own = server_id(&watcher, "w", &ContextKind::Metadata);
        let refused =
            watcher.end_session("w", &own, SessionEnd::Terminate, false, Provenance::of(Source::Session)).unwrap_err();
        assert_eq!(refused.code.as_deref(), Some(OWN_SESSION));

        let cancel = watcher
            .end_session("w", &waiter_id, SessionEnd::CancelQuery, false, Provenance::of(Source::Session))
            .unwrap();
        assert!(ok(&watcher, cancel).ended.unwrap().ended);
        let cancelled = clients.wait(waiting, WAIT).unwrap();
        assert!(matches!(cancelled.status, OperationStatus::Failed | OperationStatus::Cancelled), "{cancelled:?}");
        assert!(matches!(cancelled.error.unwrap().kind, ErrorKind::Server | ErrorKind::Cancelled));
        let after = list(&watcher, "w", all());
        assert!(after.sessions.iter().any(|session| session.id == waiter_id), "cancelling keeps the session");
        assert!(after.sessions.iter().any(|session| session.id == holder_id));

        let terminate = watcher
            .end_session("w", &holder_id, SessionEnd::Terminate, false, Provenance::of(Source::Session))
            .unwrap();
        assert!(ok(&watcher, terminate).ended.unwrap().ended);
        let deadline = Instant::now() + WAIT;
        loop {
            let after = list(&watcher, "w", all());
            if after.sessions.iter().all(|session| session.id != holder_id) {
                assert!(after.sessions.iter().any(|session| session.id == waiter_id), "only the holder ended");
                break;
            }
            assert!(Instant::now() < deadline, "{}: the holder is still listed", server.driver);
            std::thread::sleep(Duration::from_millis(100));
        }
        let gone = watcher
            .end_session("w", &holder_id, SessionEnd::Terminate, false, Provenance::of(Source::Session))
            .unwrap();
        assert!(!ok(&watcher, gone).ended.unwrap().ended, "{}: an ended session is not found again", server.driver);
        run(&clients, "c", waiter, &format!("SELECT v FROM {table} WHERE id = 1"));
        run(&clients, "c", ContextKind::Main, &format!("DROP TABLE {table}"));
        if !postgres {
            run(&clients, "c", ContextKind::Main, "DROP DATABASE zen_sessions");
        }
    }
}
