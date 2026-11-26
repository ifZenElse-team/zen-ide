use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::super::tests::Window;

fn installed() -> bool {
    let found = zen_db::sqlite::process::find_program(None).is_ok();
    if !found {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
    }
    found
}

fn code(result: Result<Value, zen_core::DispatchError>) -> String {
    match result {
        Err(zen_core::DispatchError::Failed { error, .. }) => error.code.unwrap_or_default(),
        other => panic!("not refused: {other:?}"),
    }
}

fn agent(window: &mut Window, command: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
    let registry = &window.registry;
    zen_core::macros::as_agent(&mut window.session, |session| registry.dispatch(session, command, params))
}

fn shop(window: &mut Window, kind: &str) -> String {
    let file = window.dir.path().join(format!("{kind}.db"));
    std::fs::File::create(&file).unwrap();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": format!("Shop {kind}"), "driver": "sqlite", "database": file.display().to_string(), "type": kind }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let setup = window.run_confirmed(
        "db.query.run",
        json!({
            "connection": id,
            "waitMs": 20_000,
            "sql": "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, email TEXT);\n\
                    INSERT INTO customers (name, email) VALUES ('Ann', 'ann@zen.dev'), ('Bob', 'bob@x.org'), ('Zen', 'z@zen.dev');\n\
                    CREATE TABLE orders (id INTEGER PRIMARY KEY, code TEXT);\n\
                    INSERT INTO orders (code) VALUES ('A-1');",
        }),
    );
    assert_eq!(setup["status"], "done", "{setup}");
    id
}

fn read_structure(window: &mut Window, id: &str, database: &str) {
    window.run("db.schema.refresh", json!({ "connection": id, "path": [database, "main"], "force": true }));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        super::super::connections::step(&mut window.session);
        let children =
            window.run("db.schema.children", json!({ "connection": id, "path": [database, "main", "tables"] }));
        if children["nodes"].as_array().unwrap().iter().any(|node| node["label"] == "customers") {
            return;
        }
        assert!(Instant::now() < deadline, "{children}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn finished(window: &mut Window, id: u64, params: Value) -> Value {
    let mut params = params;
    params["id"] = json!(id);
    params["waitMs"] = json!(20_000);
    let state = window.run("db.search.state", params);
    assert_ne!(state["search"]["status"], "running", "{state}");
    state
}

fn names(state: &Value) -> Vec<String> {
    state["search"]["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|table| table["table"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_schema_is_searched_in_the_background_and_a_table_opens_filtered() {
    if !installed() {
        return;
    }
    let mut window = Window::new();
    let id = shop(&mut window, "dev");
    read_structure(&mut window, &id, "dev.db");
    let targets = window.run("db.search.targets", json!({ "connection": id }));
    assert_eq!((targets["sql"].as_bool(), targets["schema"].as_str()), (Some(true), Some("main")), "{targets}");
    assert!(targets["schemas"].as_array().unwrap().contains(&json!("main")), "{targets}");
    assert_eq!(targets["warnings"], json!([]));

    let started = window.run("db.search.start", json!({ "connection": id, "text": "ZEN" }));
    assert_eq!(started["schema"], "main");
    let search = started["id"].as_u64().unwrap();
    let state = finished(&mut window, search, json!({}));
    assert_eq!(state["search"]["status"], "done", "{state}");
    assert_eq!(names(&state), ["customers"], "only tables with the text unless all: {state}");
    let customers = &state["search"]["tables"][0];
    assert_eq!((customers["rows"].as_u64(), customers["truncated"].as_bool()), (Some(2), Some(false)));
    assert_eq!(customers["columns"], json!([{ "name": "name", "rows": 1 }, { "name": "email", "rows": 2 }]));
    let text = state.to_string();
    assert!(!text.contains("ann@zen.dev") && !text.contains("z@zen.dev"), "never values: {text}");
    let all = finished(&mut window, search, json!({ "all": true, "limit": 1 }));
    assert_eq!((all["tables"].as_u64(), all["truncated"].as_bool()), (Some(2), Some(true)));
    assert_eq!(names(&all), ["customers"]);

    let opened = window.run("db.search.openResult", json!({ "table": "customers", "columns": ["email"] }));
    assert_eq!(opened["condition"], "CAST(\"email\" AS TEXT) LIKE '%ZEN%' ESCAPE '!'");
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + Duration::from_secs(20);
    let result = loop {
        let state = window.run("db.object.state", json!({ "tab": tab }));
        if let Some(result) = state["data"]["result"].as_u64()
            && state["data"]["reading"] == false
        {
            assert_eq!(state["page"], "data", "{state}");
            break result;
        }
        assert!(Instant::now() < deadline, "{state}");
        std::thread::sleep(Duration::from_millis(10));
    };
    let filter = window.run("db.result.filterState", json!({ "id": result }));
    assert_eq!(filter["condition"], "CAST(\"email\" AS TEXT) LIKE '%ZEN%' ESCAPE '!'", "{filter}");
    assert_eq!(window.run("db.result.page", json!({ "id": result }))["rows"], 2);
    let back = window.run("db.result.filterBack", json!({ "id": result }));
    assert_eq!(back["condition"], "", "Back returns to the table unfiltered: {back}");

    let names_only =
        window.run("db.search.start", json!({ "connection": id, "text": "mail", "data": false, "metadata": true }));
    let state = finished(&mut window, names_only["id"].as_u64().unwrap(), json!({}));
    assert_eq!(state["search"]["tables"][0]["namedColumns"], json!(["email"]), "{state}");
    let opened = window.run("db.search.openResult", json!({ "table": "customers" }));
    assert!(opened["condition"].is_null(), "names alone open the table unfiltered: {opened}");

    let refused = window.registry.dispatch(
        &mut window.session,
        "db.search.openResult",
        json!({ "id": search, "table": "customers", "columns": ["id"] }),
    );
    assert_eq!(code(refused), "noMatchingColumns");
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    let deadline = Instant::now() + Duration::from_secs(20);
    while window.run("db.operation.state", json!({ "operation": closing["operation"] }))["status"] == "running" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_table_the_navigator_does_not_know_opens_in_a_console() {
    if !installed() {
        return;
    }
    let mut window = Window::new();
    let id = shop(&mut window, "test");
    let started = window.run("db.search.start", json!({ "connection": id, "schema": "main", "text": "a-1" }));
    let state = finished(&mut window, started["id"].as_u64().unwrap(), json!({}));
    assert_eq!(names(&state), ["orders"], "{state}");
    let opened = window.run("db.search.openResult", json!({ "table": "orders" }));
    let path = opened["path"].as_str().expect("a console");
    let text = std::fs::read_to_string(path).unwrap();
    assert_eq!(text, "SELECT * FROM \"main\".\"orders\" WHERE CAST(\"code\" AS TEXT) LIKE '%a-1%' ESCAPE '!';\n");
    assert!(opened["run"].is_object(), "{opened}");
}

#[test]
fn agents_search_by_their_rules_and_production_is_warned_about() {
    if !installed() {
        return;
    }
    let mut window = Window::new();
    let id = shop(&mut window, "prod");
    let targets = window.run("db.search.targets", json!({ "connection": id, "schema": "main", "views": true }));
    let codes: Vec<&str> =
        targets["warnings"].as_array().unwrap().iter().map(|warning| warning["code"].as_str().unwrap()).collect();
    assert_eq!(codes, ["production", "views"], "{targets}");
    let started = agent(&mut window, "db.search.start", json!({ "connection": id, "schema": "main", "text": "zen" }))
        .expect("agents read Production");
    assert_eq!(started["warnings"][0]["code"], "production");
    let state = finished(&mut window, started["id"].as_u64().unwrap(), json!({}));
    assert_eq!(names(&state), ["customers"]);

    window.run("db.connectionType.update", json!({ "id": "prod", "agents": "none" }));
    let refused = agent(&mut window, "db.search.start", json!({ "connection": id, "schema": "main", "text": "zen" }));
    assert_eq!(code(refused), "agentNotAllowed");
}

#[test]
fn a_search_needs_a_text_sql_and_something_to_search() {
    let mut window = Window::new();
    let added = window.run("db.dataSource.add", json!({ "name": "Cache", "driver": "redis", "auth": "none" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let refused =
        window.registry.dispatch(&mut window.session, "db.search.start", json!({ "connection": id, "text": "x" }));
    assert_eq!(code(refused), "notSql");
    let refused =
        window.registry.dispatch(&mut window.session, "db.search.start", json!({ "connection": id, "text": " " }));
    assert_eq!(code(refused), "noText");
    let refused = window.registry.dispatch(
        &mut window.session,
        "db.search.start",
        json!({ "connection": id, "text": "x", "data": false }),
    );
    assert_eq!(code(refused), "nothingToSearch");
    assert_eq!(window.run("db.search.state", json!({})), json!({ "tables": 0, "truncated": false }));
    assert_eq!(window.run("db.search.cancel", json!({}))["cancelled"], false);
    let tab = window.run("db.search.open", json!({ "connection": id, "text": "secret value" }));
    assert_eq!((tab["kind"].as_str(), tab["data"]["connection"].as_str()), (Some("find"), Some(id.as_str())));
    let kept = window.run("workspace.host.state", json!({ "id": "database" }));
    assert!(!kept.to_string().contains("secret value"), "the text is never kept: {kept}");
}
