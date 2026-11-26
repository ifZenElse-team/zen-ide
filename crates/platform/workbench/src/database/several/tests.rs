use std::path::{Path, PathBuf};
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

    fn connection(&mut self, name: &str, driver: &str, kind: &str) -> String {
        let added =
            self.run("db.dataSource.add", json!({ "name": name, "driver": driver, "password": "pw", "type": kind }));
        added["id"].as_str().unwrap().to_owned()
    }

    fn console(&mut self, connection: &str, text: &str) -> PathBuf {
        let console = self.run("db.console.new", json!({ "connection": connection, "text": text }));
        PathBuf::from(console["path"].as_str().unwrap())
    }

    fn finished(&mut self, path: &Path) -> Value {
        let state = self.run("db.script.connectionsState", json!({ "path": path, "waitMs": 10_000 }));
        assert_eq!(state["running"], false, "{state}");
        state
    }

    fn history(&mut self) -> Vec<Value> {
        assert!(super::super::history::journal(&self.session).unwrap().flush(Duration::from_secs(10)));
        self.run("db.history.list", json!({ "limit": 100 }))["entries"].as_array().unwrap().clone()
    }
}

fn statuses(state: &Value) -> Vec<String> {
    state["targets"].as_array().unwrap().iter().map(|target| target["status"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn a_script_runs_on_each_connection_in_turn_with_its_results_grouped_per_connection() {
    let mut window = Window::new();
    let first = window.connection("Orders EU", zen_db::memory::SQL, "dev");
    let second = window.connection("Orders US", zen_db::memory::SQL, "test");
    let path = window.console(&first, "SELECT 1;\nSELECT 2;\n");
    let started = window.run("db.script.runOnConnections", json!({ "path": path, "connections": [first, second] }));
    assert_eq!(started["targets"].as_array().unwrap().len(), 2, "{started}");
    assert_eq!(started["targets"][0]["status"], "running", "the first starts at once: {started}");
    assert_eq!(started["targets"][1]["status"], "waiting", "the second waits for its turn: {started}");
    assert_eq!(started["targets"][1]["typeName"], "Test");
    assert_eq!(started["tab"]["path"], json!(path));

    let state = window.finished(&path);
    assert_eq!(statuses(&state), ["done", "done"], "{state}");
    for target in state["targets"].as_array().unwrap() {
        assert_eq!((target["statements"].as_u64(), target["executed"].as_u64()), (Some(2), Some(2)), "{target}");
        assert_eq!(target["results"].as_array().unwrap().len(), 2, "{target}");
    }
    let console = window.run("db.console.state", json!({ "path": path }));
    let titles: Vec<&str> =
        console["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap()).collect();
    assert_eq!(titles.len(), 4, "{console}");
    assert!(titles[..2].iter().all(|title| title.starts_with("Orders EU · ")), "{titles:?}");
    assert!(titles[2..].iter().all(|title| title.starts_with("Orders US · ")), "{titles:?}");
    assert_eq!(console["tabs"][3]["connection"], json!(second));
    assert_eq!(console["panel"]["active"], json!({ "kind": "connections" }), "the outcome per connection shows");
    assert_eq!(console["connection"], json!(first), "the console keeps its own connection");
    assert_eq!(console["connections"]["targets"][1]["name"], "Orders US");
    let page = window.run("db.result.page", json!({ "id": state["targets"][1]["results"][0], "limit": 1 }));
    assert!(page.is_object(), "{page}");

    let entries = window.history();
    let ran = entries.iter().filter(|entry| entry["source"] == "connections").count();
    assert_eq!(ran, 4, "{entries:?}");

    let started = window.run("db.console.execute", json!({ "path": path }));
    window.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }));
    let console = window.run("db.console.state", json!({ "path": path }));
    assert_ne!(console["panel"]["active"], json!({ "kind": "connections" }), "a run of its own shows its results");
}

#[test]
fn the_first_error_stops_the_run_unless_asked_to_go_on() {
    let mut window = Window::new();
    let cache = window.connection("Cache", zen_db::memory::ID, "dev");
    let shop = window.connection("Shop", zen_db::memory::SQL, "dev");
    let path = window.console(&shop, "ROWS 2\n");
    window.kv.server.set_down(true);
    window.run("db.script.runOnConnections", json!({ "path": path, "connections": [cache, shop] }));
    let state = window.finished(&path);
    assert_eq!(statuses(&state), ["failed", "skipped"], "{state}");
    assert!(state["targets"][0]["error"].is_string(), "{state}");
    assert!(
        window.sql.server.executed().is_empty(),
        "a skipped connection never runs: {:?}",
        window.sql.server.executed()
    );

    window
        .run("db.script.runOnConnections", json!({ "path": path, "connections": [cache, shop], "stopOnError": false }));
    let state = window.finished(&path);
    assert_eq!(statuses(&state), ["failed", "done"], "{state}");
    assert_eq!(state["stopOnError"], false);
    assert_eq!(window.sql.server.executed(), ["ROWS 2"]);
}

#[test]
fn production_and_risks_are_confirmed_once_for_every_connection_before_anything_runs() {
    let mut window = Window::new();
    let dev = window.connection("Local", zen_db::memory::SQL, "dev");
    let prod = window.connection("Live", zen_db::memory::SQL, "prod");
    let path = window.console(&dev, "SELECT 1;\nUPDATE orders SET paid = true WHERE id = 1;\n");
    let (code, message, data) =
        window.refusal("db.script.runOnConnections", json!({ "path": path, "connections": [dev, prod] }));
    assert_eq!(code, "confirmExecute", "{message}");
    assert!(message.contains("Live (Production)"), "{message}");
    let targets = data["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2, "every connection is listed: {data}");
    let shown = |at: usize| (targets[at]["target"]["typeName"].as_str(), targets[at]["asks"].as_bool());
    assert_eq!(shown(0), (Some("Development"), Some(false)));
    assert_eq!(shown(1), (Some("Production"), Some(true)));
    assert_eq!(targets[1]["statements"][0]["text"], "UPDATE orders SET paid = true WHERE id = 1");
    assert_eq!(targets[1]["total"], 2);
    assert_eq!(data["retry"]["command"], "db.script.runOnConnections");
    assert_eq!(data["retry"]["params"]["confirmed"], data["token"]);
    assert!(window.sql.server.executed().is_empty(), "nothing ran, nothing connected");
    let wrong = window
        .refusal("db.script.runOnConnections", json!({ "path": path, "connections": [dev, prod], "confirmed": "0" }));
    assert_eq!(wrong.0, "confirmExecute");
    let reordered = window.refusal(
        "db.script.runOnConnections",
        json!({ "path": path, "connections": [prod, dev], "confirmed": data["token"] }),
    );
    assert_eq!(reordered.0, "confirmExecute", "a token is for the connections it listed, in their order");

    let mut params = data["retry"]["params"].clone();
    params["stopOnError"] = json!(false);
    window.run("db.script.runOnConnections", params);
    let state = window.finished(&path);
    let executed = |at: usize| state["targets"][at]["executed"].as_u64();
    assert_eq!((executed(0), executed(1)), (Some(2), Some(2)), "{state}");
    let confirmed = |at: usize| state["targets"][at]["confirmed"].as_bool();
    assert_eq!((confirmed(0), confirmed(1)), (Some(false), Some(true)));
    let updates = window.sql.server.executed().into_iter().filter(|text| text.starts_with("UPDATE")).count();
    assert_eq!(updates, 2);
    let entries = window.history();
    let confirmed = entries
        .iter()
        .filter(|entry| entry["text"].as_str().unwrap().starts_with("UPDATE") && entry["confirmed"] == true)
        .count();
    assert_eq!(confirmed, 1, "{entries:?}");

    let path = window.console(&dev, "DELETE FROM orders;\n");
    let (code, _, data) =
        window.refusal("db.script.runOnConnections", json!({ "path": path, "connections": [dev, prod] }));
    assert_eq!(code, "confirmExecute");
    assert_eq!(data["dangerous"], true);
    let first = (data["targets"][0]["asks"].as_bool(), data["targets"][0]["dangerous"].as_bool());
    assert_eq!(first, (Some(true), Some(true)), "a risk asks on every type");
}

#[test]
fn a_read_only_connection_refuses_the_whole_run_and_agents_hand_production_changes_to_a_person() {
    let mut window = Window::new();
    let dev = window.connection("Local", zen_db::memory::SQL, "dev");
    let audit = window.connection("Audit", zen_db::memory::SQL, "dev");
    window.run("db.connection.readOnly", json!({ "connection": audit, "readOnly": true }));
    let path = window.console(&dev, "UPDATE orders SET paid = true WHERE id = 1;\n");
    let (code, message, data) =
        window.refusal("db.script.runOnConnections", json!({ "path": path, "connections": [dev, audit] }));
    assert_eq!(code, "readOnly", "{message}");
    assert!(message.contains("Audit"), "{message}");
    assert_eq!(data["targets"].as_array().unwrap().len(), 1, "{data}");
    assert_eq!(data["targets"][0]["target"]["name"], "Audit");
    assert!(window.sql.server.executed().is_empty(), "nothing ran on any connection");

    let prod = window.connection("Live", zen_db::memory::SQL, "prod");
    let (code, _, data) =
        Window::failed(window.agent("db.script.runOnConnections", json!({ "path": path, "connections": [dev, prod] })));
    assert_eq!(code, "agentReadOnly");
    assert_eq!(data["targets"][0]["handOff"]["command"], "db.console.new", "{data}");
    assert!(window.sql.server.executed().is_empty());

    let reads = window.console(&dev, "SELECT 1;\n");
    let started =
        window.agent("db.script.runOnConnections", json!({ "path": reads, "connections": [dev, prod] })).unwrap();
    assert_eq!(started["targets"][1]["readOnly"], true, "an agent reads Production in a read-only session: {started}");
    let state = window.finished(&reads);
    assert_eq!(statuses(&state), ["done", "done"], "{state}");
}

#[test]
fn cancel_stops_the_running_connection_and_the_rest_never_start() {
    let mut window = Window::new();
    let first = window.connection("One", zen_db::memory::SQL, "dev");
    let second = window.connection("Two", zen_db::memory::SQL, "dev");
    let path = window.console(&first, "SLEEP 5000\n");
    window.run("db.script.runOnConnections", json!({ "path": path, "connections": [first, second] }));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while window.sql.server.executed().is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let cancelled = window.run("db.operation.cancel", json!({ "path": path }));
    assert_eq!(cancelled["several"], true, "{cancelled}");
    let state = window.finished(&path);
    assert_eq!(statuses(&state), ["cancelled", "cancelled"], "{state}");
    assert_eq!(window.sql.server.executed(), ["SLEEP 5000"], "the second connection never ran");
}

#[test]
fn the_connections_are_checked_before_anything_starts() {
    let mut window = Window::new();
    let id = window.connection("Local", zen_db::memory::SQL, "dev");
    let path = window.console(&id, "SELECT 1;\n");
    let (code, ..) = window.refusal("db.script.runOnConnections", json!({ "path": path, "connections": [] }));
    assert_eq!(code, "noTargets");
    let many: Vec<String> = (0..=super::MOST_TARGETS).map(|index| format!("c{index}")).collect();
    let (code, ..) = window.refusal("db.script.runOnConnections", json!({ "path": path, "connections": many }));
    assert_eq!(code, "tooManyTargets");
    let unknown = window.registry.dispatch(
        &mut window.session,
        "db.script.runOnConnections",
        json!({ "path": path, "connections": [id, "nowhere"] }),
    );
    assert!(unknown.is_err());
    assert!(window.sql.server.executed().is_empty());
    let (code, ..) = window.refusal("db.script.connectionsState", json!({ "path": path }));
    assert_eq!(code, "noRun");
    let started = window.run("db.script.runOnConnections", json!({ "path": path, "connections": [id, id] }));
    assert_eq!(started["targets"].as_array().unwrap().len(), 1, "a connection runs once: {started}");
    window.finished(&path);
}

fn real_servers() -> (String, String) {
    let pg: u16 = std::env::var("ZEN_DB_PG_PORTS")
        .ok()
        .and_then(|ports| ports.split(',').filter_map(|port| port.trim().parse().ok()).next_back())
        .unwrap_or(54318);
    let mysql: u16 = std::env::var("ZEN_DB_MYSQL_SERVERS")
        .ok()
        .and_then(|servers| {
            servers.split(',').find_map(|pair| {
                let (driver, port) = pair.trim().split_once(':')?;
                (driver == "mysql").then(|| port.parse().ok()).flatten()
            })
        })
        .unwrap_or(33084);
    (
        format!("postgresql://postgres:zen@127.0.0.1:{pg}/postgres?sslmode=disable"),
        format!("mysql://root:zen@127.0.0.1:{mysql}/mysql"),
    )
}

#[test]
#[ignore = "needs PostgreSQL and MySQL in Docker (docs/development.md)"]
fn one_script_runs_on_postgresql_and_mysql_each_in_its_dialect() {
    let mut window = Window::new();
    window.session.set_state(super::super::Database::default());
    let (pg_url, mysql_url) = real_servers();
    let pg = window.run("db.dataSource.add", json!({ "name": "PG", "url": pg_url }))["id"].as_str().unwrap().to_owned();
    let mysql = window.run("db.dataSource.add", json!({ "name": "MySQL", "url": mysql_url }))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let script = "CREATE TEMPORARY TABLE several_items (id int, label varchar(20));\n\
                  INSERT INTO several_items VALUES (1, 'a;b'), (2, 'c');\n\
                  SELECT id, label FROM several_items ORDER BY id;\n";
    let path = window.console(&pg, script);
    window.run("db.script.runOnConnections", json!({ "path": path, "connections": [pg, mysql] }));
    let state = window.run("db.script.connectionsState", json!({ "path": path, "waitMs": 60_000 }));
    assert_eq!(statuses(&state), ["done", "done"], "{state}");
    for target in state["targets"].as_array().unwrap() {
        assert_eq!((target["executed"].as_u64(), target["rows"].as_u64()), (Some(3), Some(2)), "{target}");
        let result = target["results"][0].clone();
        let page = window.run("db.result.page", json!({ "id": result, "limit": 10 }));
        assert_eq!(page["page"][0][1]["value"], "a;b", "a delimiter in a string stays in it: {page}");
    }

    let failing = window.console(&pg, "SELECT missing_column FROM several_nowhere;\nSELECT 1;\n");
    window.run("db.script.runOnConnections", json!({ "path": failing, "connections": [pg, mysql] }));
    let state = window.run("db.script.connectionsState", json!({ "path": failing, "waitMs": 60_000 }));
    assert_eq!(statuses(&state), ["failed", "skipped"], "{state}");
    assert!(state["targets"][0]["error"].as_str().is_some_and(|error| error.contains("several_nowhere")), "{state}");
    for id in [pg, mysql] {
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}

#[test]
fn parameters_and_variables_are_asked_once_for_every_connection_and_bound_on_each() {
    let mut window = Window::new();
    let first = window.connection("Orders EU", zen_db::memory::SQL, "dev");
    let second = window.connection("Orders US", zen_db::memory::SQL, "dev");
    let path = window.console(&first, "SELECT * FROM ${tbl} WHERE id = :id;\n");
    let (code, _, data) =
        window.refusal("db.script.runOnConnections", json!({ "path": path, "connections": [first, second] }));
    assert_eq!(code, "parametersNeeded", "{data}");
    let names: Vec<&str> =
        data["parameters"].as_array().unwrap().iter().map(|asked| asked["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["id"], "each placeholder once for every connection: {data}");
    assert_eq!(data["variables"][0]["name"], "tbl");
    assert_eq!(data["retry"]["command"], "db.script.runOnConnections");
    assert!(window.sql.server.executed().is_empty(), "nothing runs before the values are given");

    let mut retry = data["retry"]["params"].clone();
    retry["parameters"] = json!({ "id": 7 });
    retry["variables"] = json!({ "tbl": "rows" });
    let started = window.run("db.script.runOnConnections", retry);
    assert_eq!(started["targets"][0]["inlined"], json!([]), "{started}");
    let state = window.finished(&path);
    assert_eq!(statuses(&state), ["done", "done"], "{state}");
    let bound = window.sql.server.bound();
    let sent: Vec<&(String, Vec<Option<String>>)> =
        bound.iter().filter(|(text, _)| text == "SELECT * FROM rows WHERE id = $1").collect();
    assert_eq!(sent.len(), 2, "bound on each connection: {bound:?}");
    assert!(sent.iter().all(|(_, values)| values == &[Some("7".to_owned())]), "{bound:?}");

    let again = window.run(
        "db.script.runOnConnections",
        json!({ "path": path, "connections": [first, second], "parameters": {}, "newTab": true }),
    );
    assert_eq!(again["targets"].as_array().unwrap().len(), 2, "remembered values fill the rest: {again}");
    assert_eq!(statuses(&window.finished(&path)), ["done", "done"]);
}
