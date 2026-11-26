use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::memory::MemoryDriver;
use zen_db::types::{DEVELOPMENT, PRODUCTION};

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
        Self { registry, session, sql, _dir: dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn refusal(result: Result<Value, zen_core::DispatchError>) -> (String, Value) {
        match result {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                (error.code.unwrap_or_default(), error.data.map(Value::Object).unwrap_or(Value::Null))
            }
            other => panic!("not refused: {other:?}"),
        }
    }

    fn refused(&mut self, command: &str, params: Value) -> (String, Value) {
        Self::refusal(self.registry.dispatch(&mut self.session, command, params))
    }

    fn agent(&mut self, command: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
        let registry = &self.registry;
        zen_core::macros::as_agent(&mut self.session, |session| registry.dispatch(session, command, params))
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

    fn plan(&mut self, tab: &str, params: Value) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut asked = params.clone();
            asked["tab"] = json!(tab);
            let state = self.run("db.plan.state", asked);
            if state["status"] != "running" {
                return state;
            }
            assert!(Instant::now() < deadline, "{state}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn explain_opens_a_plan_tab_with_the_tree_and_the_hottest_node_chosen() {
    let mut window = Window::new();
    let id = window.connection("Shop", zen_db::memory::SQL, DEVELOPMENT);
    let console = window.console(&id, "SELECT * FROM rows WHERE even;\n");
    let started = window.run("db.console.explain", json!({ "path": console }));
    let tab = started["tab"].as_str().unwrap().to_owned();
    assert_eq!(started["executes"], false);
    assert_eq!(started["statement"], "EXPLAIN (FORMAT JSON) SELECT * FROM rows WHERE even");
    let state = window.plan(&tab, json!({}));
    assert_eq!(state["status"], "done", "{state}");
    assert_eq!(state["measure"], "cost");
    let nodes: Vec<(&str, u64)> = state["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| (node["node"].as_str().unwrap(), node["depth"].as_u64().unwrap()))
        .collect();
    assert_eq!(nodes, [("Sort", 0), ("Seq Scan", 1)]);
    assert_eq!(state["nodes"][1]["entity"], "rows");
    assert_eq!(state["nodes"][1]["condition"], "(even)");
    assert_eq!(state["hot"], json!([1, 0]));
    assert_eq!(state["selected"]["id"], 1, "the hottest node is chosen first");
    assert!(state["nodes"][1]["details"].as_array().unwrap().is_empty(), "details only when asked");
    assert!(state["selected"]["details"].as_array().unwrap().iter().any(|detail| detail["key"] == "Filter"));
    assert_eq!(state["supported"], json!(["analyze", "verbose", "costs", "buffers", "settings"]));
    let workspaces = window.session.state::<Workspaces>().unwrap();
    let inner = workspaces.active_tab(super::super::WORKSPACE).unwrap();
    assert_eq!((inner.id.as_str(), inner.kind.as_str(), inner.title.as_str()), (tab.as_str(), "plan", "Plan: console"));
    assert!(window.sql.server.executed().contains(&"EXPLAIN (FORMAT JSON) SELECT * FROM rows WHERE even".to_owned()));

    let page = window.plan(&tab, json!({ "offset": 1, "limit": 1, "details": true, "raw": true, "rawLimit": 10 }));
    assert_eq!((page["total"].as_u64(), page["truncated"].as_bool()), (Some(2), Some(false)));
    assert_eq!(page["nodes"][0]["node"], "Seq Scan");
    assert!(!page["nodes"][0]["details"].as_array().unwrap().is_empty());
    assert_eq!((page["raw"].as_str().map(str::len), page["rawTruncated"].as_bool()), (Some(10), Some(true)));
    let first = window.plan(&tab, json!({ "limit": 1 }));
    assert_eq!(first["truncated"], true);

    let node = window.run("db.plan.select", json!({ "node": 0 }));
    assert_eq!(node["node"], "Sort");
    assert_eq!(window.plan(&tab, json!({}))["selected"]["id"], 0);
    window.run("db.plan.view", json!({ "view": "raw" }));
    assert_eq!(window.plan(&tab, json!({}))["view"], "raw");
    let (code, _) = window.refused("db.plan.select", json!({ "node": 9 }));
    assert_eq!(code, "noNode");

    let again = window.run("db.console.explain", json!({ "analyze": true }));
    assert_eq!(again["tab"], tab.as_str(), "⌥⌘E on a Plan tab explains its statement again");
    assert_eq!((again["executes"].as_bool(), again["rolledBack"].as_bool()), (Some(true), Some(true)));
    let analyzed = window.plan(&tab, json!({}));
    assert_eq!((analyzed["measure"].as_str(), analyzed["executionMs"].as_f64()), (Some("time"), Some(2.1)));
    assert_eq!(analyzed["view"], "raw", "the tab keeps what it shows");
    let executed = window.sql.server.executed();
    let from = executed.iter().position(|text| text == "START TRANSACTION").expect("ANALYZE runs in a transaction");
    assert!(executed[from + 1].starts_with("EXPLAIN (FORMAT JSON, ANALYZE"), "{executed:?}");
    assert_eq!(executed[from + 2], "DISCARD", "and it is rolled back: {executed:?}");
}

#[test]
fn analyze_is_checked_like_running_the_statement() {
    let mut window = Window::new();
    let id = window.connection("Prod", zen_db::memory::SQL, PRODUCTION);
    let console = window.console(&id, "DELETE FROM rows;\n");
    let plain = window.run("db.console.explain", json!({ "path": console }));
    assert_eq!(plain["executes"], false, "a plain EXPLAIN never runs the DELETE, so it does not ask");
    let (code, data) = window.refused("db.console.explain", json!({ "path": console, "analyze": true }));
    assert_eq!(code, "confirmExecute");
    assert_eq!(data["dangerous"], true, "{data}");
    assert_eq!(data["retry"]["command"], "db.console.explain");
    let started = window.run("db.console.explain", data["retry"]["params"].clone());
    assert_eq!(started["executes"], true);
    let tab = started["tab"].as_str().unwrap().to_owned();
    assert_eq!(window.plan(&tab, json!({}))["status"], "done");

    let (code, data) = Window::refusal(window.agent("db.console.explain", json!({ "path": console, "analyze": true })));
    assert_eq!(code, "agentReadOnly", "{data}");
    let read = window.agent("db.console.explain", json!({ "path": console })).unwrap();
    assert_eq!(read["executes"], false, "an agent may read plans on Production");

    window.run("db.connection.readOnly", json!({ "connection": id }));
    let (code, _) = window.refused("db.console.explain", json!({ "path": console, "analyze": true }));
    assert_eq!(code, "readOnly");
}

#[test]
fn explain_takes_one_statement_and_judges_what_analyze_runs() {
    let mut window = Window::new();
    let id = window.connection("Prod", zen_db::memory::SQL, PRODUCTION);
    let console = window.console(&id, "SELECT * FROM rows;\n");
    let smuggled = json!({ "path": console, "statement": "SELECT * FROM rows; DROP TABLE rows" });
    let (code, _) = window.refused("db.console.explain", smuggled.clone());
    assert_eq!(code, "oneStatement");
    let (code, _) = Window::refusal(window.agent("db.console.explain", smuggled));
    assert_eq!(code, "oneStatement", "an agent too");
    let (code, data) = window.refused(
        "db.console.explain",
        json!({ "path": console, "statement": "CREATE TABLE t AS SELECT 1", "analyze": true }),
    );
    assert_eq!(code, "confirmExecute", "{data}");
    let read = window
        .run("db.console.explain", json!({ "path": console, "statement": "SELECT * FROM rows", "analyze": true }));
    assert_eq!(read["executes"], true, "a read is analyzed without asking");
}

#[test]
fn a_driver_without_plans_says_so_and_a_restored_tab_explains_again() {
    let mut window = Window::new();
    let cache = window.connection("Cache", zen_db::memory::ID, DEVELOPMENT);
    let console = window.console(&cache, "GET user:1\n");
    let (code, _) = window.refused("db.console.explain", json!({ "path": console }));
    assert_eq!(code, "unsupported");
    let (code, _) = window.refused("db.plan.state", json!({}));
    assert_eq!(code, "noPlan");

    let id = window.connection("Shop", zen_db::memory::SQL, DEVELOPMENT);
    window.run(
        "workspace.tab.open",
        json!({
            "workspace": "database",
            "id": "plan:kept",
            "kind": "plan",
            "title": "Plan: kept",
            "data": { "connection": id, "statement": "SELECT * FROM rows", "options": { "verbose": true } },
        }),
    );
    let restored = window.run("db.plan.state", json!({ "tab": "plan:kept" }));
    assert_eq!(
        (restored["status"].as_str(), restored["statement"].as_str()),
        (Some("notRun"), Some("SELECT * FROM rows"))
    );
    assert!(window.sql.server.opened().is_empty(), "a restored tab connects nowhere");
    let started = window.run("db.console.explain", json!({ "tab": "plan:kept" }));
    assert_eq!(started["statement"], "EXPLAIN (FORMAT JSON, VERBOSE) SELECT * FROM rows");
    assert_eq!(started["connecting"], true);
    assert_eq!(window.plan("plan:kept", json!({}))["status"], "done");
}

#[test]
fn explain_asks_for_parameters_and_variables_and_binds_them_on_the_server() {
    let mut window = Window::new();
    let id = window.connection("Shop", zen_db::memory::SQL, DEVELOPMENT);
    let console = window.console(&id, "SELECT * FROM ${tbl} WHERE id = :id AND even = $1 LIMIT :n;\n");
    let (code, data) = window.refused("db.console.explain", json!({ "path": console }));
    assert_eq!(code, "parametersNeeded", "{data}");
    let asked: Vec<(&str, &str)> = data["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|asked| (asked["name"].as_str().unwrap(), asked["inferred"].as_str().unwrap()))
        .collect();
    assert_eq!(asked, [("id", "text"), ("$1", "text"), ("n", "number")]);
    assert_eq!(data["variables"][0]["name"], "tbl");
    assert_eq!(data["retry"]["command"], "db.console.explain");
    assert!(window.sql.server.executed().is_empty(), "nothing is sent before the values are given");

    let mut retry = data["retry"]["params"].clone();
    retry["parameters"] = json!({ "id": 5, "$1": true, "n": 10 });
    retry["variables"] = json!({ "tbl": "rows" });
    let started = window.run("db.console.explain", retry);
    let sent = "EXPLAIN (FORMAT JSON) SELECT * FROM rows WHERE id = $1 AND even = $2 LIMIT $3";
    assert_eq!(started["statement"], sent);
    assert_eq!(started["inlined"], json!([]));
    let tab = started["tab"].as_str().unwrap().to_owned();
    let state = window.plan(&tab, json!({}));
    assert_eq!(state["status"], "done", "{state}");
    assert_eq!(state["statement"], "SELECT * FROM ${tbl} WHERE id = :id AND even = $1 LIMIT :n");
    let bound = window.sql.server.bound();
    assert_eq!(
        bound.last().unwrap(),
        &(sent.to_owned(), vec![Some("5".to_owned()), Some("t".to_owned()), Some("10".to_owned())]),
        "values are bound, never written into the text"
    );

    let (code, _) = window.refused("db.console.explain", json!({ "tab": tab }));
    assert_eq!(code, "parametersNeeded", "explaining again asks again, as a run does");
    let again = window.run("db.console.explain", json!({ "tab": tab, "parameters": {}, "analyze": true }));
    let analyzed = again["statement"].as_str().unwrap();
    assert!(analyzed.starts_with("EXPLAIN (FORMAT JSON, ANALYZE"), "{analyzed}");
    assert!(
        analyzed.ends_with(" SELECT * FROM rows WHERE id = $1 AND even = $2 LIMIT $3"),
        "remembered values fill the rest"
    );
}
