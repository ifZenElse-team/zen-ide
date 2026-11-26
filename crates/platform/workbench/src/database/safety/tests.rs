use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::memory::MemoryDriver;

use crate::workspaces::Workspaces;

struct Window {
    registry: Registry,
    session: Session,
    sql: Arc<MemoryDriver>,
    kv: Arc<MemoryDriver>,
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
        let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::clone(&sql) as _, Arc::clone(&kv) as _];
        session.state_mut::<super::super::Database>().set_drivers(drivers);
        Self { registry, session, sql, kv, _dir: dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn refusal(&mut self, command: &str, params: Value) -> (String, String, Value) {
        Self::failed(self.registry.dispatch(&mut self.session, command, params))
    }

    fn failed(result: Result<Value, zen_core::DispatchError>) -> (String, String, Value) {
        match result {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                (error.code.unwrap_or_default(), error.message, error.data.map(Value::Object).unwrap_or(Value::Null))
            }
            other => panic!("not refused: {other:?}"),
        }
    }

    fn agent(&mut self, command: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
        let registry = &self.registry;
        zen_core::macros::as_agent(&mut self.session, |session| registry.dispatch(session, command, params))
    }

    fn agent_refusal(&mut self, command: &str, params: Value) -> (String, String, Value) {
        Self::failed(self.agent(command, params))
    }

    fn connection(&mut self, name: &str, driver: &str, kind: &str) -> String {
        let added =
            self.run("db.dataSource.add", json!({ "name": name, "driver": driver, "password": "pw", "type": kind }));
        added["id"].as_str().unwrap().to_owned()
    }

    fn console(&mut self, connection: &str, text: &str) -> PathBuf {
        let console = self.run("db.console.new", json!({ "connection": connection, "text": text }));
        PathBuf::from(console["path"].as_str().unwrap())
    }

    fn finished(&mut self, started: &Value) -> Value {
        let done = self.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }));
        self.settle();
        done
    }

    fn settle(&self) {
        assert!(super::super::history::journal(&self.session).unwrap().flush(Duration::from_secs(10)));
    }

    fn history(&mut self) -> Vec<Value> {
        self.settle();
        self.run("db.history.list", json!({ "limit": 100 }))["entries"].as_array().unwrap().clone()
    }
}

fn texts(statements: &Value) -> Vec<String> {
    statements.as_array().unwrap().iter().map(|statement| statement["text"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn making_a_connected_connection_production_applies_at_once_and_the_question_says_how_it_commits() {
    let mut window = Window::new();
    let id = window.connection("Shop", zen_db::memory::SQL, "dev");
    let path = window.console(&id, "SELECT 1;\n");
    let started = window.run("db.console.executeScript", json!({ "path": path }));
    assert_eq!(window.finished(&started)["status"], "done");
    let status = window.run("db.transaction.status", json!({ "path": path }));
    assert_eq!(status["autoCommit"], true, "{status}");
    window.run("db.dataSource.update", json!({ "id": id, "type": "prod" }));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        let status = window.run("db.transaction.status", json!({ "path": path }));
        if status["autoCommit"] == false || std::time::Instant::now() > deadline {
            break status;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!((status["mode"].as_str(), status["autoCommit"].as_bool()), (Some("manual"), Some(false)), "{status}");
    let path = window.console(&id, "UPDATE orders SET paid = true WHERE id = 1;\n");
    let (code, message, data) = window.refusal("db.console.executeScript", json!({ "path": path }));
    assert_eq!(code, "confirmExecute", "{message}");
    assert!(message.contains("the changes stay in the open transaction"), "{message}");
    assert_eq!(data["autoCommit"], false);
    window.run("db.dataSource.update", json!({ "id": id, "type": "dev", "overrides": { "confirmExecute": true } }));
    let (_, message, data) = window.refusal("db.console.executeScript", json!({ "path": path }));
    assert!(message.contains("each change is committed at once"), "{message}");
    assert_eq!(data["autoCommit"], true);
}

#[test]
fn production_asks_before_changes_and_runs_them_with_the_token() {
    let mut window = Window::new();
    let id = window.connection("Shop", zen_db::memory::SQL, "prod");
    let path =
        window.console(&id, "SELECT 1;\nUPDATE orders SET paid = true WHERE id = 1;\nSET search_path TO shop;\n");
    let (code, message, data) = window.refusal("db.console.executeScript", json!({ "path": path }));
    assert_eq!(code, "confirmExecute", "{message}");
    assert!(message.contains("Shop") && message.contains("Production"), "{message}");
    assert_eq!(texts(&data["statements"]), ["UPDATE orders SET paid = true WHERE id = 1"], "only the change asks");
    assert_eq!(
        (data["asked"].as_u64(), data["total"].as_u64(), data["dangerous"].as_bool()),
        (Some(1), Some(3), Some(false))
    );
    assert_eq!(data["target"]["type"], "prod");
    assert_eq!(data["target"]["color"], "red");
    assert_eq!(data["retry"]["command"], "db.console.executeScript");
    assert_eq!(data["retry"]["params"]["confirmed"], data["token"]);
    assert!(window.sql.server.executed().is_empty(), "nothing ran, nothing connected");
    let again = window.refusal("db.console.executeScript", json!({ "path": path }));
    assert_eq!(again.0, "confirmExecute", "running twice is not a confirmation");
    let wrong = window.refusal("db.console.executeScript", json!({ "path": path, "confirmed": "0000" }));
    assert_eq!(wrong.0, "confirmExecute");
    let started = window.run("db.console.executeScript", data["retry"]["params"].clone());
    assert_eq!(started["statements"], 3);
    window.finished(&started);
    assert!(window.sql.server.executed().iter().any(|text| text.starts_with("UPDATE orders")));
    let entries = window.history();
    let update = entries.iter().find(|entry| entry["text"].as_str().unwrap().starts_with("UPDATE")).unwrap();
    assert_eq!(update["confirmed"], true, "{update}");
    window.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    window.run("editor.edit.insert", json!({ "text": "DELETE FROM orders WHERE id = 2;\n" }));
    let changed = window.refusal("db.console.executeScript", json!({ "path": path, "confirmed": data["token"] }));
    assert_eq!(changed.0, "confirmExecute", "a token is for the statements it was given for");
    assert_ne!(changed.2["token"], data["token"]);
    assert_eq!(changed.2["asked"], 2);
}

#[test]
fn risks_ask_on_every_type_and_name_what_they_hit() {
    let mut window = Window::new();
    let id = window.connection("Local", zen_db::memory::SQL, "dev");
    let path = window
        .console(&id, "SELECT 1;\nDELETE FROM orders;\nDROP TABLE customers;\nUPDATE orders SET a = 1 WHERE id = 2;\n");
    window.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let started = window.run("db.console.execute", json!({ "path": path }));
    assert_eq!(window.finished(&started)["status"], "done", "a read on Development runs at once");
    let (code, message, data) = window.refusal("db.console.executeScript", json!({ "path": path }));
    assert_eq!(code, "confirmExecute");
    assert!(message.contains("DELETE without WHERE removes every row of orders"), "{message}");
    assert_eq!(data["dangerous"], true);
    assert_eq!(texts(&data["statements"]), ["DELETE FROM orders", "DROP TABLE customers"]);
    assert_eq!(data["statements"][0]["index"], 1);
    assert_eq!(data["statements"][1]["risks"][0], "DROP removes TABLE customers with its data");
    let mut params = data["retry"]["params"].clone();
    params["skip"] = json!([2]);
    let started = window.run("db.console.executeScript", params);
    assert_eq!(started["statements"], 3, "the skipped DROP is left out");
    window.finished(&started);
    let executed = window.sql.server.executed();
    assert!(executed.iter().any(|text| text == "DELETE FROM orders"), "{executed:?}");
    assert!(!executed.iter().any(|text| text.starts_with("DROP")), "{executed:?}");
    let entries = window.history();
    let dropped = entries.iter().find(|entry| entry["text"] == "DROP TABLE customers").unwrap();
    assert_eq!((dropped["outcome"].as_str(), dropped["code"].as_str()), (Some("refused"), Some("skipped")));
    let mut params = data["retry"]["params"].clone();
    params["skip"] = json!([1, 2]);
    params["path"] = json!(path);
    let only_reads = window.run("db.console.executeScript", params);
    assert_eq!(only_reads["statements"], 2);
}

#[test]
fn read_only_connections_refuse_changes_and_the_lock_is_a_command() {
    let mut window = Window::new();
    let id = window.connection("Ledger", zen_db::memory::SQL, "dev");
    let path = window.console(&id, "SELECT 1;\nUPDATE orders SET paid = true WHERE id = 1;\n");
    let locked = window.run("db.connection.readOnly", json!({ "connection": id }));
    assert_eq!((locked["readOnly"].as_bool(), locked["own"].as_bool()), (Some(true), Some(true)));
    assert_eq!(window.run("db.dataSource.get", json!({ "id": id }))["readOnly"], true);
    let (code, message, data) = window.refusal("db.console.executeScript", json!({ "path": path }));
    assert_eq!(code, "readOnly");
    assert!(message.contains("Ledger is read-only") && message.contains("UPDATE orders"), "{message}");
    assert_eq!(texts(&data["statements"]), ["UPDATE orders SET paid = true WHERE id = 1"]);
    assert!(data["retry"].is_null(), "a read-only refusal cannot be confirmed");
    assert!(window.sql.server.executed().is_empty());
    let entries = window.history();
    assert_eq!(entries[0]["outcome"], "refused");
    assert_eq!(entries[0]["code"], "readOnly");
    let (code, ..) = window.agent_refusal("db.connection.readOnly", json!({ "connection": id, "readOnly": false }));
    assert_eq!(code, "agentNotAllowed", "an agent never unlocks");
    let (code, ..) = window.agent_refusal("db.dataSource.update", json!({ "id": id, "readOnly": false }));
    assert_eq!(code, "agentNotAllowed");
    window.run("db.connection.readOnly", json!({ "connection": id, "readOnly": false }));
    let started = window.run("db.console.executeScript", json!({ "path": path }));
    window.finished(&started);
    assert!(window.sql.server.executed().iter().any(|text| text.starts_with("UPDATE orders")));
    window.agent("db.connection.readOnly", json!({ "connection": id, "readOnly": true })).unwrap();
    window.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let started = window.run("db.console.execute", json!({ "path": path }));
    assert_eq!(window.finished(&started)["status"], "done", "a read still runs");
    let (code, ..) = window.refusal("db.console.executeScript", json!({ "path": path }));
    assert_eq!(code, "readOnly", "locked again at once, while connected");
}

#[test]
fn a_key_value_server_judges_its_own_commands_on_a_read_only_connection() {
    let mut window = Window::new();
    let id = window.connection("Cache", zen_db::memory::ID, "dev");
    window.run("db.connection.readOnly", json!({ "connection": id, "readOnly": true }));
    let path = window.console(&id, "GET a\nSET a 1\n");
    let started = window.run("db.console.executeScript", json!({ "path": path }));
    let done = window.finished(&started);
    assert_eq!(done["status"], "failed", "{done}");
    assert_eq!(done["statements"][0]["status"], "done");
    assert_eq!(done["statements"][1]["error"]["kind"], "server", "the server refused the write: {done}");
    assert!(window.kv.server.executed().iter().any(|text| text == "SET a 1"));
}

#[test]
fn retyping_a_connected_key_value_connection_to_a_read_only_type_locks_its_open_session() {
    let mut window = Window::new();
    window.run(
        "db.connectionType.create",
        json!({ "id": "audit", "name": "Audit", "readOnly": true, "autoCommit": false }),
    );
    let id = window.connection("Cache", zen_db::memory::ID, "dev");
    let path = window.console(&id, "SET a 1\nSET a 2\n");
    window.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let started = window.run("db.console.execute", json!({}));
    assert_eq!(window.finished(&started)["status"], "done");
    window.run("db.dataSource.update", json!({ "id": id, "type": "audit" }));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while window.run("db.transaction.status", json!({ "path": path }))["autoCommit"] != false {
        assert!(std::time::Instant::now() < deadline, "the type never reached the open session");
        std::thread::sleep(Duration::from_millis(5));
    }
    window.run("editor.selection.set", json!({ "selections": [{ "head": 9 }] }));
    let started = window.run("db.console.execute", json!({}));
    let done = window.finished(&started);
    assert_eq!(done["statements"][0]["error"]["kind"], "server", "the open session is read-only now: {done}");
    assert_eq!(window.kv.server.executed().iter().filter(|text| text.starts_with("SET a")).count(), 2);
}

#[test]
fn a_read_only_type_makes_every_connection_of_it_read_only() {
    let mut window = Window::new();
    window.run("db.connectionType.create", json!({ "id": "audit", "name": "Audit", "readOnly": true }));
    let id = window.connection("Ledger", zen_db::memory::SQL, "audit");
    let listed = window.run("db.connectionType.list", json!({}));
    let audit = listed["types"].as_array().unwrap().iter().find(|kind| kind["id"] == "audit").unwrap().clone();
    assert_eq!(audit["readOnly"], true);
    let (code, ..) = window.refusal("db.query.run", json!({ "connection": id, "sql": "TRUNCATE orders" }));
    assert_eq!(code, "readOnly");
    let answer = window.run("db.query.run", json!({ "connection": id, "sql": "SELECT 1", "waitMs": 10_000 }));
    assert_eq!(answer["status"], "done", "{answer}");
    let state = window.run("db.connection.readOnly", json!({ "connection": id, "readOnly": false }));
    assert_eq!((state["readOnly"].as_bool(), state["byType"].as_bool()), (Some(true), Some(true)));
}

#[test]
fn agents_only_read_on_production_and_hand_changes_to_a_person() {
    let mut window = Window::new();
    let id = window.connection("Shop", zen_db::memory::SQL, "prod");
    let types = window.run("db.connectionType.list", json!({}));
    let prod = types["types"].as_array().unwrap().iter().find(|kind| kind["id"] == "prod").unwrap().clone();
    assert_eq!(prod["agents"], "readOnly");
    let read = window.agent("db.query.run", json!({ "connection": id, "sql": "SELECT 1", "waitMs": 10_000 })).unwrap();
    assert_eq!(read["status"], "done", "{read}");
    let (code, message, data) = window.agent_refusal(
        "db.query.run",
        json!({ "connection": id, "sql": "UPDATE orders SET paid = true WHERE id = 1" }),
    );
    assert_eq!(code, "agentReadOnly", "{message}");
    assert!(message.contains("Production"), "{message}");
    assert_eq!(data["handOff"]["command"], "db.console.new");
    assert_eq!(data["handOff"]["params"]["text"], "UPDATE orders SET paid = true WHERE id = 1;\n");
    assert!(data["token"].is_null() && data["retry"].is_null(), "nothing an agent could confirm");
    let (code, ..) = window.agent_refusal(
        "db.query.run",
        json!({ "connection": id, "sql": "UPDATE orders SET paid = true WHERE id = 1", "confirmed": "anything" }),
    );
    assert_eq!(code, "agentReadOnly");
    let (code, ..) = window.agent_refusal("db.query.run", json!({ "connection": id, "sql": "SELECT nextval('s')" }));
    assert_eq!(code, "agentReadOnly", "a read with side effects is a change");
    assert!(!window.sql.server.executed().iter().any(|text| text.starts_with("UPDATE") || text.contains("nextval")));
    let opened = window.agent("db.console.new", data["handOff"]["params"].clone()).unwrap();
    assert!(PathBuf::from(opened["path"].as_str().unwrap()).is_file());
    let (code, ..) = window.agent_refusal("db.transaction.commit", json!({ "connection": id }));
    assert_eq!(code, "agentReadOnly", "a commit is the person's on Production");
    let (code, ..) = window.agent_refusal("db.dataSource.update", json!({ "id": id, "type": "dev" }));
    assert_eq!(code, "agentNotAllowed");
    let (code, ..) =
        window.agent_refusal("db.dataSource.update", json!({ "id": id, "overrides": { "agents": "full" } }));
    assert_eq!(code, "agentNotAllowed");
    let (code, ..) = window.agent_refusal("db.dataSource.update", json!({ "id": id, "profile": "redshift" }));
    assert_eq!(code, "agentNotAllowed", "a profile without the server's guards loosens the connection");
    let (code, ..) = window.agent_refusal("db.connectionType.update", json!({ "id": "prod", "agents": "full" }));
    assert_eq!(code, "agentNotAllowed");
    let (code, ..) = window.agent_refusal("db.connectionType.update", json!({ "id": "prod", "confirmExecute": false }));
    assert_eq!(code, "agentNotAllowed");
    window.agent("db.connectionType.update", json!({ "id": "prod", "statementTimeout": 60 })).unwrap();
    window.agent("db.connectionType.update", json!({ "id": "dev", "agents": "readOnly" })).unwrap();
    let entries = window.history();
    let refused: Vec<&Value> = entries.iter().filter(|entry| entry["outcome"] == "refused").collect();
    assert_eq!(refused.len(), 3, "{entries:?}");
    assert!(refused.iter().all(|entry| entry["agent"] == true && entry["code"] == "agentReadOnly"));
    window.run("db.connectionType.update", json!({ "id": "prod", "agents": "full" }));
    let (code, _, data) = window.agent_refusal(
        "db.query.run",
        json!({ "connection": id, "sql": "UPDATE orders SET paid = true WHERE id = 1" }),
    );
    assert_eq!(code, "confirmExecute", "with full access the type still asks");
    let answer = window
        .agent("db.query.run", json!({ "connection": id, "sql": "UPDATE orders SET paid = true WHERE id = 1", "confirmed": data["token"], "waitMs": 10_000 }))
        .unwrap();
    assert_ne!(answer["status"], "running", "{answer}");
    let entries = window.history();
    let update = entries
        .iter()
        .find(|entry| entry["outcome"] != "refused" && entry["text"].as_str().unwrap().starts_with("UPDATE"))
        .unwrap();
    assert_eq!((update["agent"].as_bool(), update["confirmed"].as_bool()), (Some(true), Some(true)));
}

#[test]
fn agents_on_a_type_without_access_run_nothing_and_a_person_still_can() {
    let mut window = Window::new();
    window.run("db.connectionType.create", json!({ "id": "secret", "name": "Secret", "agents": "none" }));
    let id = window.connection("Payroll", zen_db::memory::ID, "secret");
    let (code, message, _) = window.agent_refusal("db.query.run", json!({ "connection": id, "sql": "GET a" }));
    assert_eq!(code, "agentNotAllowed", "{message}");
    let (code, ..) =
        window.agent_refusal("db.key.expire", json!({ "connection": id, "path": ["user", "1"], "seconds": 5 }));
    assert_eq!(code, "agentNotAllowed");
    let (code, ..) = window.agent_refusal("db.key.get", json!({ "connection": id, "path": ["user", "1"] }));
    assert_eq!(code, "agentNotAllowed", "a key's value is a read");
    let folder = window._dir.path().join("exports");
    let export = json!({ "source": { "kind": "query", "connection": id, "sql": "GET a" }, "folder": folder });
    let (code, ..) = window.agent_refusal("db.export.start", export);
    assert_eq!(code, "agentNotAllowed", "an export reads rows: not for an agent where it may run nothing");
    let answer = window.run("db.query.run", json!({ "connection": id, "sql": "GET a", "waitMs": 10_000 }));
    assert_eq!(answer["status"], "done", "{answer}");
    let entries = window.history();
    let ran = entries.iter().find(|entry| entry["text"] == "GET a" && entry["outcome"] != "refused").unwrap();
    let listed = window.agent("db.history.list", json!({ "limit": 100 })).unwrap();
    assert_eq!(listed["total"], 0, "its history is not an agent's either: {listed}");
    let (code, ..) = window.agent_refusal("db.history.get", json!({ "id": ran["id"] }));
    assert_eq!(code, "unknownEntry");
    let (code, ..) = window.agent_refusal("db.query.state", json!({ "query": answer["id"] }));
    assert_eq!(code, "agentNotAllowed", "nor are the rows a person read there: {answer}");
}

#[test]
fn the_safety_of_a_connection_is_in_its_console_state() {
    let mut window = Window::new();
    let id = window.connection("Shop", zen_db::memory::SQL, "prod");
    let path = window.console(&id, "SELECT 1;\n");
    let state = window.run("db.console.state", json!({ "path": path }));
    assert_eq!(state["safety"]["type"], "prod");
    assert_eq!(state["safety"]["typeName"], "Production");
    assert_eq!(state["safety"]["confirmExecute"], true);
    assert_eq!(state["safety"]["readOnly"], false);
    assert_eq!(state["safety"]["agents"], "readOnly");
}
