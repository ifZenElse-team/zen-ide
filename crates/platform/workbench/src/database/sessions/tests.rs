use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::memory::{MemoryDriver, MemoryServer};
use zen_db::sessions::{ServerLock, ServerSession};

use crate::workspaces::Workspaces;

struct Window {
    registry: Registry,
    session: Session,
    sql: Arc<MemoryDriver>,
    _dir: tempfile::TempDir,
}

impl Window {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        super::super::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let mut session = Session::new();
        super::super::install(&mut session);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
        session.set_state(super::super::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
        let sql = Arc::new(MemoryDriver::sql());
        let kv = Arc::new(MemoryDriver::new(false));
        let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::clone(&sql) as _, kv as _];
        session.state_mut::<super::super::Database>().set_drivers(drivers);
        sql.server.set_sessions(others(), locks());
        Self { registry, session, sql, _dir: dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn failed(result: Result<Value, zen_core::DispatchError>) -> (String, String, Value) {
        match result {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                (error.code.unwrap_or_default(), error.message, error.data.map(Value::Object).unwrap_or(Value::Null))
            }
            other => panic!("not refused: {other:?}"),
        }
    }

    fn refusal(&mut self, command: &str, params: Value) -> (String, String, Value) {
        Self::failed(self.registry.dispatch(&mut self.session, command, params))
    }

    fn agent(&mut self, command: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
        let registry = &self.registry;
        zen_core::macros::as_agent(&mut self.session, |session| registry.dispatch(session, command, params))
    }

    fn connection(&mut self, name: &str, kind: &str) -> String {
        let added = self.run(
            "db.dataSource.add",
            json!({ "name": name, "driver": zen_db::memory::SQL, "password": "pw", "type": kind }),
        );
        added["id"].as_str().unwrap().to_owned()
    }

    fn list(&mut self, connection: &str) -> Value {
        let listed = self.run("db.session.list", json!({ "connection": connection, "waitMs": 10_000 }));
        assert_eq!(listed["status"], "done", "{listed}");
        listed
    }
}

fn others() -> Vec<ServerSession> {
    let session = |id: &str, state: &str, blocked_by: &[&str], query: &str| ServerSession {
        id: id.to_owned(),
        user: Some("ann".to_owned()),
        host: Some("10.0.0.7:51544".to_owned()),
        database: Some("shop".to_owned()),
        application: Some("psql".to_owned()),
        state: Some(state.to_owned()),
        query: Some(query.to_owned()),
        blocked_by: blocked_by.iter().map(|id| (*id).to_owned()).collect(),
        ..ServerSession::default()
    };
    vec![
        session("41", "idle in transaction", &[], "UPDATE orders SET paid = true WHERE id = 7"),
        session("42", "active", &["41"], "UPDATE orders SET paid = false WHERE id = 7"),
        session("43", "idle", &[], "ALTER ROLE ann PASSWORD 'hunter2'"),
    ]
}

fn locks() -> Vec<ServerLock> {
    vec![ServerLock {
        session: "42".to_owned(),
        kind: "transactionid".to_owned(),
        mode: "ShareLock".to_owned(),
        granted: false,
        object: Some("1234".to_owned()),
        blocked_by: vec!["41".to_owned()],
    }]
}

#[test]
fn sessions_are_listed_sized_for_agents_and_never_with_passwords() {
    let mut window = Window::new();
    let shop = window.connection("Shop", zen_db::types::DEVELOPMENT);
    let listed = window.list(&shop);
    let sessions = listed["sessions"]["sessions"].as_array().unwrap();
    assert_eq!(sessions[0]["id"], "41");
    assert_eq!(sessions[0]["blocking"], json!(["42"]));
    assert_eq!(sessions[1]["blockedBy"], json!(["41"]));
    assert_eq!(listed["sessions"]["chains"], json!([["42", "41"]]));
    assert_eq!(listed["sessions"]["locks"][0]["mode"], "ShareLock");
    assert!(!listed.to_string().contains("hunter2"), "{listed}");
    assert_eq!(sessions[2]["redacted"], true);
    let own: Vec<&Value> = sessions.iter().filter(|session| session["own"] == true).collect();
    assert!(own.iter().any(|session| session["context"] == "metadata"), "{own:?}");

    let small = window.run(
        "db.session.list",
        json!({ "connection": shop, "limit": 1, "textLimit": 6, "showIdle": false, "locks": false, "waitMs": 10_000 }),
    );
    assert_eq!(small["sessions"]["sessions"].as_array().unwrap().len(), 1);
    assert_eq!((small["sessions"]["total"].as_u64(), small["sessions"]["truncated"].as_bool()), (Some(2), Some(true)));
    assert_eq!(small["sessions"]["sessions"][0]["query"], "UPDATE");
    assert_eq!(small["sessions"]["sessions"][0]["queryCut"], true);
    assert!(small["sessions"].get("locks").is_none_or(|locks| locks.as_array().unwrap().is_empty()));

    let state = window.run("db.session.state", json!({ "operation": listed["operation"] }));
    assert_eq!(state["sessions"]["sessions"][0]["id"], "41");
}

#[test]
fn terminating_asks_first_and_touches_only_the_chosen_session() {
    let mut window = Window::new();
    let shop = window.connection("Shop", zen_db::types::DEVELOPMENT);
    window.list(&shop);
    let (code, message, data) = window.refusal("db.session.terminate", json!({ "connection": shop, "session": "41" }));
    assert_eq!(code, "confirmExecute");
    assert!(message.contains("41") && message.contains("ann@10.0.0.7:51544"), "{message}");
    let statement = &data["statements"][0];
    assert_eq!(statement["text"], "SELECT pg_terminate_backend(41)");
    assert!(!statement["dangers"].as_array().unwrap().is_empty(), "terminating is risky: {data}");
    assert!(statement["risks"].to_string().contains("UPDATE orders SET paid = true"), "{data}");
    assert!(window.sql.server.executed().iter().all(|text| !text.contains("pg_terminate_backend")));
    let (code, _, _) = window
        .refusal("db.session.terminate", json!({ "connection": shop, "session": "42", "confirmed": data["token"] }));
    assert_eq!(code, "confirmExecute", "a token confirms only the session it asked about");

    let retry = &data["retry"];
    let mut params = retry["params"].clone();
    params["waitMs"] = json!(10_000);
    let done = window.run(retry["command"].as_str().unwrap(), params);
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(done["ended"]["ended"], true);
    let ended: Vec<String> =
        window.sql.server.executed().into_iter().filter(|text| text.contains("pg_terminate_backend")).collect();
    assert_eq!(ended, ["SELECT pg_terminate_backend(41)"]);
    let left: Vec<String> = window.list(&shop)["sessions"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(!left.contains(&"41".to_owned()) && left.contains(&"42".to_owned()) && left.contains(&"43".to_owned()));

    let history = window.run("db.history.list", json!({ "sources": ["session"] }));
    assert_eq!(history["entries"][0]["text"], "SELECT pg_terminate_backend(41)");
    assert_eq!(history["entries"][0]["confirmed"], true);

    let (code, _, data) = window.refusal("db.session.cancelQuery", json!({ "connection": shop, "session": "42" }));
    assert_eq!(code, "confirmExecute");
    assert_eq!(data["statements"][0]["text"], "SELECT pg_cancel_backend(42)");
    assert!(data["statements"][0]["dangers"].as_array().unwrap().is_empty(), "cancelling is a plain question");
    let (code, _, _) = window.refusal(
        "db.session.cancelQuery",
        json!({ "connection": shop, "session": "42", "confirmed": data["token"], "skip": [0] }),
    );
    assert_eq!(code, "skipped");
    assert!(window.sql.server.executed().iter().all(|text| !text.contains("pg_cancel_backend")));
    let (code, _, _) = window.refusal("db.session.cancelQuery", json!({ "connection": shop, "session": "4 2" }));
    assert_eq!(code, "invalidSession");
}

#[test]
fn own_sessions_need_to_be_named_as_such_and_read_only_connections_refuse() {
    let mut window = Window::new();
    let shop = window.connection("Shop", zen_db::types::DEVELOPMENT);
    let listed = window.list(&shop);
    let own = listed["sessions"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["context"] == "metadata")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(own, MemoryServer::session_id(1), "{listed}");
    let (code, message, _) = window.refusal("db.session.terminate", json!({ "connection": shop, "session": own }));
    assert_eq!(code, "ownSession", "{message}");
    for spelled in [format!(" {own}"), format!("0{own}")] {
        let (code, message, _) =
            window.refusal("db.session.terminate", json!({ "connection": shop, "session": spelled }));
        assert_eq!(code, "ownSession", "{spelled:?} is the same session: {message}");
    }
    let (code, _, data) =
        window.refusal("db.session.terminate", json!({ "connection": shop, "session": own, "own": true }));
    assert_eq!(code, "confirmExecute");
    assert!(data["statements"][0]["risks"].to_string().contains("this IDE's own"), "{data}");
    assert_eq!(data["retry"]["params"]["own"], true);

    window.run("db.connection.readOnly", json!({ "connection": shop, "readOnly": true }));
    let (code, _, _) = window.refusal("db.session.cancelQuery", json!({ "connection": shop, "session": "42" }));
    assert_eq!(code, "readOnly");
    let history = window.run("db.history.list", json!({ "sources": ["session"], "outcome": "refused" }));
    assert_eq!(history["entries"][0]["text"], "SELECT pg_cancel_backend(42)");
    assert!(window.sql.server.executed().iter().all(|text| !text.contains("pg_cancel_backend")));
}

#[test]
fn agents_list_everywhere_but_end_sessions_only_where_they_may_change() {
    let mut window = Window::new();
    let prod = window.connection("Shop", zen_db::types::PRODUCTION);
    let listed = window.agent("db.session.list", json!({ "connection": prod, "waitMs": 10_000 })).unwrap();
    assert_eq!(listed["status"], "done");
    let (code, _, data) =
        Window::failed(window.agent("db.session.terminate", json!({ "connection": prod, "session": "41" })));
    assert_eq!(code, "agentReadOnly");
    assert_eq!(data["handOff"]["command"], "db.session.open");
    let dev = window.connection("Shop Dev", zen_db::types::DEVELOPMENT);
    window.list(&dev);
    let (code, _, data) =
        Window::failed(window.agent("db.session.terminate", json!({ "connection": dev, "session": "41" })));
    assert_eq!(code, "confirmExecute", "an agent confirms like a person, with the token");
    let mut params = data["retry"]["params"].clone();
    params["waitMs"] = json!(10_000);
    let done = window.agent("db.session.terminate", params).unwrap();
    assert_eq!(done["ended"]["ended"], true);
    let history = window.run("db.history.list", json!({ "sources": ["session"], "agent": true }));
    assert_eq!(history["total"], 2, "{history}");
}

#[test]
fn the_sessions_tab_opens_per_connection_and_other_drivers_are_unsupported() {
    let mut window = Window::new();
    let shop = window.connection("Shop", zen_db::types::DEVELOPMENT);
    let tab = window.run("db.session.open", json!({ "connection": shop }));
    assert_eq!(
        (tab["kind"].as_str(), tab["id"].as_str()),
        (Some("sessions"), Some(format!("sessions:{shop}").as_str()))
    );
    assert_eq!(tab["title"], "Sessions: Shop");
    assert_eq!((tab["data"]["showIdle"].as_bool(), tab["data"]["autoRefresh"].as_u64()), (Some(true), Some(0)));
    let changed = window.run("db.session.open", json!({ "connection": shop, "showIdle": false, "autoRefresh": 5 }));
    assert_eq!(
        (changed["data"]["showIdle"].as_bool(), changed["data"]["autoRefresh"].as_u64()),
        (Some(false), Some(5))
    );
    let again = window.run("db.session.open", json!({ "connection": shop }));
    assert_eq!(again["data"]["autoRefresh"], 5, "settings stay");
    let status = window.run("db.connection.status", json!({ "connection": shop }));
    assert_ne!(status["state"], "connected", "opening the tab connects nowhere: {status}");

    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Cache", "driver": zen_db::memory::ID, "password": "pw", "type": zen_db::types::DEVELOPMENT }),
    );
    let cache = added["id"].as_str().unwrap().to_owned();
    let (code, _, _) = window.refusal("db.session.open", json!({ "connection": cache }));
    assert_eq!(code, "unsupported");
    let (code, _, _) = window.refusal("db.session.list", json!({ "connection": cache }));
    assert_eq!(code, "unsupported");
}
