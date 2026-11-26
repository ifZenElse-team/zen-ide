use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use crate::workspaces::Workspaces;

struct Window {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
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
        session.set_state(super::super::Secrets(std::sync::Arc::new(zen_secrets::MemoryStore::new())));
        let driver = std::sync::Arc::new(zen_db::memory::MemoryDriver::new(false));
        session.state_mut::<super::super::Database>().set_drivers(vec![driver]);
        Self { registry, session, dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn refused(&mut self, command: &str, params: Value) -> zen_core::DispatchError {
        self.registry.dispatch(&mut self.session, command, params).unwrap_err()
    }

    fn as_agent(&mut self, command: &str, params: Value) -> Value {
        let registry = &self.registry;
        zen_core::macros::as_agent(&mut self.session, |session| registry.dispatch(session, command, params)).unwrap()
    }

    fn settle(&self) {
        assert!(super::journal(&self.session).unwrap().flush(Duration::from_secs(10)));
    }

    fn finished(&mut self, started: &Value) -> Value {
        let done = self.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }));
        self.settle();
        done
    }

    fn console(&mut self, text: &str) -> PathBuf {
        let console = self.run("db.console.new", json!({ "text": text }));
        PathBuf::from(console["path"].as_str().unwrap())
    }

    fn list(&mut self, params: Value) -> Value {
        self.run("db.history.list", params)
    }

    fn texts(&mut self, params: Value) -> Vec<String> {
        let listed = self.list(params);
        listed["entries"].as_array().unwrap().iter().map(|entry| entry["text"].as_str().unwrap().to_owned()).collect()
    }
}

#[test]
fn statements_of_consoles_agents_and_transactions_are_recorded_and_found() {
    let mut window = Window::new();
    let added = window.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "pw" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let path = window.console("SET a 1\nGET a\n");
    let started = window.run("db.console.executeScript", json!({ "path": path }));
    assert_eq!(window.finished(&started)["status"], "done");
    window.run("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    let started = window.run("db.console.execute", json!({ "path": path }));
    window.finished(&started);
    let failing = window.console("FAIL here\n");
    let started = window.run("db.console.executeScript", json!({ "path": failing }));
    assert_eq!(window.finished(&started)["status"], "failed");
    let asked = window.as_agent("db.query.run", json!({ "connection": id, "sql": "GET a", "waitMs": 10_000 }));
    assert_eq!(asked["status"], "done", "{asked}");
    window.settle();
    window.run("db.transaction.mode", json!({ "path": path, "mode": "manual" }));
    let started = window.run("db.console.execute", json!({ "path": path }));
    window.finished(&started);
    let ended = window.run("db.transaction.commit", json!({ "path": path }));
    window.run("db.operation.state", json!({ "operation": ended["operation"], "waitMs": 10_000 }));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while window.list(json!({ "sources": ["transaction"] }))["total"] == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }

    let all = window.list(json!({}));
    let rows: Vec<String> = all["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            format!(
                "{} {} {} {}{}",
                entry["source"].as_str().unwrap(),
                entry["outcome"].as_str().unwrap(),
                entry["text"].as_str().unwrap(),
                entry["place"].as_str().unwrap_or("-"),
                if entry["agent"] == true { " agent" } else { "" }
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            "transaction done COMMIT -",
            "statement done SET a 1 console.sql",
            "query done GET a agent agent",
            "script failed FAIL here console.sql",
            "statement done SET a 1 console.sql",
            "script done GET a console.sql",
            "script done SET a 1 console.sql",
        ]
    );
    let newest = &all["entries"][0];
    assert_eq!((newest["connection"].as_str(), newest["connectionName"].as_str()), (Some(id.as_str()), Some("KV")));
    assert_eq!(all["entries"][5]["rows"], 1);
    assert!(all["entries"][5]["time"].as_str().unwrap().starts_with("20"), "{}", all["entries"][5]);
    let failed = window.list(json!({ "outcome": "failed" }));
    assert_eq!(failed["total"], 1);
    assert!(failed["entries"][0]["error"].as_str().unwrap().contains("syntax error"), "{failed}");
    assert_eq!(failed["entries"][0]["code"], "42601");
    assert_eq!(window.texts(json!({ "query": "get A" })), ["GET a", "GET a"]);
    assert_eq!(window.texts(json!({ "query": "near" })), ["FAIL here"], "the error is searched too");
    assert_eq!(window.texts(json!({ "agent": true })), ["GET a"]);
    assert_eq!(window.texts(json!({ "sources": ["script"], "period": "today", "limit": 2 })), ["FAIL here", "GET a"]);
    let page = window.list(json!({ "sources": ["script"], "limit": 2 }));
    assert_eq!((page["total"].as_u64(), page["truncated"].as_bool()), (Some(3), Some(true)));
    assert!(window.list(json!({ "connection": "elsewhere" }))["entries"].as_array().unwrap().is_empty());
    let short = window.list(json!({ "query": "fail", "textLimit": 4 }));
    assert_eq!(
        (short["entries"][0]["text"].as_str(), short["entries"][0]["textCut"].as_bool()),
        (Some("FAIL"), Some(true))
    );
    let whole = window.run("db.history.get", json!({ "id": short["entries"][0]["id"] }));
    assert_eq!(whole["text"], "FAIL here");
    let unknown = window.refused("db.history.get", json!({ "id": 7 })).to_string();
    assert!(unknown.contains("no statement 7"), "{unknown}");

    let agent_entry = window.list(json!({ "agent": true }))["entries"][0]["id"].clone();
    let opened = window.run("db.history.openInConsole", json!({ "id": agent_entry, "run": true }));
    assert_eq!(opened["connection"], id.as_str());
    assert_eq!(std::fs::read_to_string(opened["path"].as_str().unwrap()).unwrap(), "GET a");
    assert!(opened["path"].as_str().unwrap().contains(&format!("/{id}/")), "{opened}");
    window.finished(&opened["run"]);
    assert_eq!(window.list(json!({}))["entries"][0]["text"], "GET a");
    assert_eq!(window.list(json!({}))["entries"][0]["source"], "script");

    let journal = window.dir.path().join("data/databases/history.jsonl");
    let lines = std::fs::read_to_string(&journal).unwrap();
    assert_eq!(lines.lines().count(), 8, "{lines}");
    assert_eq!(zen_db::journal::decode::<Value>(lines.as_bytes()).records.len(), 8, "every record has its check");
}

#[test]
fn passwords_are_masked_and_the_off_switch_retention_and_clear_work() {
    let mut window = Window::new();
    let added = window.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "pw" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let asked = window.run("db.query.run", json!({ "connection": id, "sql": "AUTH admin hunter2", "waitMs": 10_000 }));
    assert_eq!(asked["status"], "failed");
    window.settle();
    let listed = window.list(json!({}));
    assert_eq!(listed["entries"][0]["text"], "AUTH *** ***");
    assert_eq!(listed["entries"][0]["redacted"], true);
    assert_eq!(listed["entries"][0]["agent"], false);
    let journal = window.dir.path().join("data/databases/history.jsonl");
    assert!(!std::fs::read_to_string(&journal).unwrap().contains("hunter2"));
    let refused = window.refused("db.history.openInConsole", json!({ "id": listed["entries"][0]["id"], "run": true }));
    assert!(
        matches!(&refused, zen_core::DispatchError::Failed { error, .. } if error.code.as_deref() == Some("notWhole")),
        "{refused}"
    );

    let settings = window.run("db.history.settings", json!({}));
    assert_eq!(
        settings["settings"],
        json!({ "enabled": true, "retentionDays": 90, "maxEntries": 10_000, "parameterValues": false })
    );
    assert_eq!(settings["kept"], 1);
    let off = window.run("db.history.settings", json!({ "enabled": false, "retentionDays": 0, "maxEntries": 5 }));
    assert_eq!(
        off["settings"],
        json!({ "enabled": false, "retentionDays": 1, "maxEntries": 100, "parameterValues": false })
    );
    window.run("db.query.run", json!({ "connection": id, "sql": "GET a", "waitMs": 10_000 }));
    window.settle();
    let listed = window.list(json!({}));
    assert_eq!((listed["kept"].as_u64(), listed["enabled"].as_bool()), (Some(1), Some(false)));
    let kept = std::fs::read_to_string(window.dir.path().join("data/databases/history-settings.json")).unwrap();
    assert!(kept.contains("\"enabled\": false"), "{kept}");
    window.run("db.history.settings", json!({ "enabled": true }));
    window.run("db.query.run", json!({ "connection": id, "sql": "GET b", "waitMs": 10_000 }));
    window.settle();
    assert_eq!(window.list(json!({}))["kept"], 2);
    let cleared = window.run("db.history.clear", json!({ "connection": "other" }));
    assert_eq!(cleared["kept"], 2);
    let cleared = window.run("db.history.clear", json!({ "connection": id }));
    assert_eq!(cleared["kept"], 0);
    assert!(!journal.exists(), "a history cleared of everything leaves no file");
}

#[test]
fn the_history_tab_opens_once_with_its_search_and_comes_back_after_a_restart() {
    let mut window = Window::new();
    let tab = window.run("db.history.open", json!({ "query": "orders" }));
    assert_eq!((tab["id"].as_str(), tab["kind"].as_str()), (Some(super::TAB), Some(super::TAB)));
    assert_eq!(tab["title"], "Query History");
    assert_eq!(tab["data"]["query"], "orders");
    let again = window.run("db.history.open", json!({ "connection": "pg" }));
    assert_eq!(again["data"], json!({ "query": "orders", "connection": "pg" }));
    let state = window.run("workspace.host.state", json!({ "id": super::super::WORKSPACE }));
    assert_eq!(state["expanded"], true);
    assert_eq!(state["active"], super::TAB);
    let titles: Vec<&str> =
        state["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Overview", "Query History"]);
    let saved = crate::workspaces::saved(&mut window.session);
    let mut restarted = Window::new();
    crate::workspaces::restore(&mut restarted.session, saved);
    let state = restarted.run("workspace.host.state", json!({ "id": super::super::WORKSPACE }));
    assert_eq!(state["tabs"][1]["data"]["query"], "orders");
    let listed = restarted.list(json!({}));
    assert_eq!(listed["total"], 0);
    let status = restarted.run("db.connection.status", json!({}));
    assert!(status["connections"].as_array().unwrap().iter().all(|connection| connection["state"] == "disconnected"));
}

#[test]
fn the_query_an_export_reads_is_recorded_with_its_rows() {
    let mut window = Window::new();
    let driver = std::sync::Arc::new(zen_db::memory::MemoryDriver::sql());
    window.session.state_mut::<super::super::Database>().set_drivers(vec![driver]);
    let added =
        window.run("db.dataSource.add", json!({ "name": "Shop", "driver": zen_db::memory::SQL, "password": "pw" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let source =
        json!({ "kind": "query", "connection": id, "sql": "SELECT * FROM public.customers", "name": "people" });
    let started = window.as_agent(
        "db.export.start",
        json!({ "source": source, "clipboard": true, "settings": { "format": "csv" }, "remember": false }),
    );
    let state = window.run("db.export.state", json!({ "id": started["id"], "waitMs": 10_000 }));
    assert_eq!(state["exports"][0]["status"], "done", "{state}");
    window.settle();
    let listed = window.list(json!({ "sources": ["export"] }));
    assert_eq!(listed["total"], 1, "{listed}");
    let entry = &listed["entries"][0];
    assert_eq!(entry["text"], "SELECT * FROM public.customers");
    assert_eq!((entry["source"].as_str(), entry["agent"].as_bool()), (Some("export"), Some(true)));
    assert_eq!((entry["outcome"].as_str(), entry["rows"].as_u64()), (Some("done"), Some(25)), "{entry}");
    assert_eq!(entry["place"], "people");
}

#[test]
fn run_again_never_runs_a_statement_of_a_connection_not_in_the_project_on_another_one() {
    let mut window = Window::new();
    let gone = window.run("db.dataSource.add", json!({ "name": "Old", "driver": "memory-kv", "password": "pw" }));
    let gone = gone["id"].as_str().unwrap().to_owned();
    window.run("db.query.run", json!({ "connection": gone, "sql": "SET a 1", "waitMs": 10_000 }));
    window.settle();
    let entry = window.list(json!({}))["entries"][0]["id"].clone();
    window.run("db.dataSource.remove", json!({ "id": gone }));
    window.run("db.dataSource.add", json!({ "name": "Only", "driver": "memory-kv", "password": "pw" }));

    let refused = window.refused("db.history.openInConsole", json!({ "id": entry, "run": true }));
    assert!(
        matches!(&refused, zen_core::DispatchError::Failed { error, .. } if error.code.as_deref() == Some("unknownConnection")),
        "{refused}"
    );
    assert_eq!(window.run("db.console.list", json!({}))["total"], 0, "no console is made for a refused run");
    let opened = window.run("db.history.openInConsole", json!({ "id": entry }));
    let state = window.run("db.console.state", json!({ "path": opened["path"] }));
    assert!(state["connection"].is_null(), "its console takes no other connection by itself: {state}");
}
