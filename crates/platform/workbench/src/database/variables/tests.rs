use serde_json::json;

use super::super::tests::{Window, sql_connection};

fn refusal(window: &mut Window, command: &str, params: serde_json::Value) -> zen_core::CommandError {
    match window.registry.dispatch(&mut window.session, command, params) {
        Err(zen_core::DispatchError::Failed { error, .. }) => error,
        other => panic!("expected a refusal: {other:?}"),
    }
}

fn console(window: &mut Window, text: &str) -> std::path::PathBuf {
    let console = window.run("db.console.new", json!({ "text": text }));
    std::path::PathBuf::from(console["path"].as_str().unwrap())
}

fn finished(window: &mut Window, started: &serde_json::Value) -> serde_json::Value {
    window.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }))
}

#[test]
fn placeholders_are_asked_once_bound_for_real_and_remembered_without_secrets() {
    let mut window = Window::new();
    let (_, driver) = sql_connection(&mut window, zen_db::types::DEVELOPMENT, true);
    let text = "@set tbl = customers\nSELECT * FROM ${tbl} WHERE id = :id AND name = :name AND note <> ':no';\n";
    let path = console(&mut window, text);
    let listed = window.run("db.console.parameters", json!({ "scope": "script" }));
    let names: Vec<&str> =
        listed["parameters"].as_array().unwrap().iter().map(|asked| asked["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["id", "name"]);
    assert_eq!(listed["variables"][0], json!({ "name": "tbl", "value": "customers", "secret": false }));
    assert_eq!(listed["inlined"], false);

    let asked = refusal(&mut window, "db.console.executeScript", json!({}));
    assert_eq!(asked.code.as_deref(), Some(super::PARAMETERS_NEEDED));
    let data = asked.data.unwrap();
    assert_eq!(data["parameters"].as_array().unwrap().len(), 2);
    assert_eq!(data["retry"]["command"], "db.console.executeScript");

    let started = window.run(
        "db.console.executeScript",
        json!({ "parameters": { "id": 1, "name": { "type": "text", "value": "O'Brien" } } }),
    );
    let done = finished(&mut window, &started);
    assert_eq!(done["status"], "done", "{done}");
    let bound = driver.server.bound();
    let (sent, values) = bound.last().unwrap();
    assert_eq!(sent, "SELECT * FROM customers WHERE id = $1 AND name = $2 AND note <> ':no'");
    assert_eq!(
        values,
        &[Some("1".to_owned()), Some("O'Brien".to_owned())],
        "values are bound, never written into the text"
    );

    let again = window.run("db.console.executeScript", json!({ "parameters": {} }));
    assert_eq!(finished(&mut window, &again)["status"], "done", "remembered values fill what is not given");

    window.run("db.console.setParameter", json!({ "name": "password", "value": "hunter2" }));
    let remembered = window.run("db.console.variables", json!({ "path": path }));
    assert_eq!(remembered["variables"][0]["value"], "customers");
    assert_eq!(
        remembered["parameters"][0],
        json!({ "name": "id", "value": { "type": "number", "value": "1" }, "secret": false })
    );
    let secret = remembered["parameters"].as_array().unwrap().iter().find(|param| param["name"] == "password").unwrap();
    assert_eq!(secret, &json!({ "name": "password", "secret": true }));
    let file = std::fs::read_to_string(window.dir.path().join("data/databases/variables.json")).unwrap();
    assert!(file.contains("O'Brien") && !file.contains("hunter2") && !file.contains("password"), "{file}");

    window.session.set_state(super::Variables::default());
    let reread = window.run("db.console.variables", json!({ "path": path }));
    assert_eq!(reread["parameters"].as_array().unwrap().len(), 2, "kept across restarts, secrets left out: {reread}");

    let wrong = refusal(
        &mut window,
        "db.console.setParameter",
        json!({ "name": "id", "value": { "type": "number", "value": "1; DROP" } }),
    );
    assert_eq!(wrong.code.as_deref(), Some("invalidParameter"));
    window.run("db.console.setVariable", json!({ "name": "tbl", "value": "orders" }));
    window.run("db.console.setVariable", json!({ "name": "gone", "value": "x" }));
    let removed = window.run("db.console.setVariable", json!({ "name": "gone" }));
    assert_eq!(removed["variables"].as_array().unwrap().len(), 1);
}

#[test]
fn includes_run_in_place_and_agents_pass_values() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, zen_db::types::DEVELOPMENT, true);
    std::fs::write(window.dir.path().join("project/setup.sql"), "SET search_path = ${schema};\nSELECT :n;\n").unwrap();
    console(&mut window, "@set schema = shop\n@include setup.sql\nSELECT 1;\n");
    let started = window.run("db.console.executeScript", json!({ "parameters": { "n": null } }));
    let done = finished(&mut window, &started);
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(done["statements"].as_array().unwrap().len(), 5);
    let executed = driver.server.executed();
    assert!(executed.contains(&"SET search_path = shop".to_owned()), "{executed:?}");
    assert_eq!(driver.server.bound().last().unwrap(), &("SELECT $1".to_owned(), vec![None]));

    console(&mut window, "@include missing.sql\n");
    let missing = refusal(&mut window, "db.console.executeScript", json!({}));
    assert_eq!(missing.code.as_deref(), Some("includeFailed"));
    std::fs::write(window.dir.path().join("project/loop.sql"), "@include loop.sql\n").unwrap();
    console(&mut window, "@include loop.sql\n");
    let cycle = refusal(&mut window, "db.console.executeScript", json!({}));
    assert_eq!(cycle.code.as_deref(), Some("includeFailed"));

    let needed = refusal(&mut window, "db.query.run", json!({ "connection": id, "sql": "SELECT :a, ${b}" }));
    assert_eq!(needed.code.as_deref(), Some(super::PARAMETERS_NEEDED));
    let data = needed.data.unwrap();
    assert_eq!(data["parameters"][0]["name"], "a");
    assert_eq!(data["variables"][0]["name"], "b");
    let answered = window.run(
        "db.query.run",
        json!({
            "connection": id,
            "sql": "SELECT :a, '${b}'",
            "parameters": { "a": true },
            "variables": { "b": "x" },
            "waitMs": 10_000,
        }),
    );
    assert_eq!(answered["status"], "done", "{answered}");
    assert_eq!(driver.server.bound().last().unwrap(), &("SELECT $1, 'x'".to_owned(), vec![Some("t".to_owned())]));
}

#[test]
fn the_dialog_starts_with_the_type_read_from_where_a_placeholder_is_used() {
    let mut window = Window::new();
    sql_connection(&mut window, zen_db::types::DEVELOPMENT, true);
    console(&mut window, "SELECT * FROM customers WHERE id = :id AND name = :name AND :n + 1 > 2 LIMIT :rows;\n");
    let listed = window.run("db.console.parameters", json!({}));
    let inferred: Vec<(&str, &str)> = listed["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|asked| (asked["name"].as_str().unwrap(), asked["inferred"].as_str().unwrap()))
        .collect();
    assert_eq!(inferred, [("id", "number"), ("name", "text"), ("n", "number"), ("rows", "number")], "{listed}");

    window.run("db.console.setParameter", json!({ "name": "id", "value": "42" }));
    let asked = refusal(&mut window, "db.console.execute", json!({}));
    let data = asked.data.unwrap();
    assert_eq!(data["parameters"][0]["value"], json!({ "type": "text", "value": "42" }), "a remembered value wins");
    assert_eq!(data["parameters"][0]["inferred"], "number");
    assert_eq!(data["parameters"][3]["inferred"], "number");
}

fn with_sqlite(window: &mut Window) -> Option<(String, String)> {
    if zen_db::sqlite::process::find_program(None).is_err() {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return None;
    }
    let mut drivers: Vec<std::sync::Arc<dyn zen_db::driver::Driver>> =
        vec![std::sync::Arc::new(zen_db::memory::MemoryDriver::sql())];
    drivers.extend(zen_db::drivers().into_iter().filter(|driver| driver.id() == "sqlite"));
    window.session.state_mut::<super::super::Database>().set_drivers(drivers);
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "driver": zen_db::memory::SQL, "password": "pw", "type": zen_db::types::DEVELOPMENT }),
    );
    let memory = added["id"].as_str().unwrap().to_owned();
    let file = window.dir.path().join("project/shop.sqlite");
    window.run("db.databaseFile.create", json!({ "path": file }));
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Local", "driver": "sqlite", "database": file.display().to_string(), "scope": "project" }),
    );
    let sqlite = added["id"].as_str().unwrap().to_owned();
    let made = window.run(
        "db.query.run",
        json!({ "connection": sqlite, "sql": "CREATE TABLE rows (id INTEGER, name TEXT)", "waitMs": 30_000 }),
    );
    assert_eq!(made["status"], "done", "{made}");
    Some((memory, sqlite))
}

#[test]
fn explain_and_several_connections_write_values_as_literals_only_where_a_driver_cannot_bind() {
    let mut window = Window::new();
    let Some((memory, sqlite)) = with_sqlite(&mut window) else { return };
    let created =
        window.run("db.console.new", json!({ "connection": sqlite, "text": "SELECT * FROM rows WHERE id = :id;\n" }));
    let path = std::path::PathBuf::from(created["path"].as_str().unwrap());
    let needed = refusal(&mut window, "db.console.explain", json!({ "path": path }));
    assert_eq!(needed.code.as_deref(), Some(super::PARAMETERS_NEEDED));
    assert_eq!(needed.data.unwrap()["inlined"], true, "the dialog says the values are written as literals");
    let started = window.run("db.console.explain", json!({ "path": path, "parameters": { "id": 3 } }));
    assert_eq!(started["statement"], "EXPLAIN QUERY PLAN SELECT * FROM rows WHERE id = 3");
    assert_eq!(started["inlined"], json!([":id = 3"]));
    let tab = started["tab"].as_str().unwrap().to_owned();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let plan = loop {
        let plan = window.run("db.plan.state", json!({ "tab": tab }));
        if plan["status"] != "running" || std::time::Instant::now() > deadline {
            break plan;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!((plan["status"].as_str(), &plan["inlined"]), (Some("done"), &json!([":id = 3"])), "{plan}");

    let run = window.run(
        "db.script.runOnConnections",
        json!({ "path": path, "connections": [memory, sqlite], "parameters": { "id": 3 } }),
    );
    assert_eq!(run["targets"][0]["inlined"], json!([]), "{run}");
    assert_eq!(run["targets"][1]["inlined"], json!([":id = 3"]), "{run}");
    let state = window.run("db.script.connectionsState", json!({ "path": path, "waitMs": 30_000 }));
    let statuses: Vec<&str> =
        state["targets"].as_array().unwrap().iter().map(|target| target["status"].as_str().unwrap()).collect();
    assert_eq!(statuses, ["done", "done"], "{state}");
    let query = state["targets"][1]["query"].clone();
    let output = window.run("db.query.state", json!({ "query": query }));
    assert!(output.to_string().contains("parametersInlined"), "the output says so: {output}");
}

#[test]
fn sqlite_runs_numbered_question_and_named_parameters() {
    let mut window = Window::new();
    let Some((_, sqlite)) = with_sqlite(&mut window) else { return };
    let filled = window.run(
        "db.query.run",
        json!({ "connection": sqlite, "sql": "INSERT INTO rows VALUES (1, 'a'), (2, 'b'), (2, 'c')", "waitMs": 30_000 }),
    );
    assert_eq!(filled["status"], "done", "{filled}");
    for (sql, name) in [
        ("SELECT * FROM rows WHERE id = $1", "$1"),
        ("SELECT * FROM rows WHERE id = ?1", "?1"),
        ("SELECT * FROM rows WHERE id = ?", "?1"),
        ("SELECT * FROM rows WHERE id = :id", "id"),
    ] {
        let run = window.run(
            "db.query.run",
            json!({ "connection": sqlite, "sql": sql, "parameters": { name: 2 }, "waitMs": 30_000 }),
        );
        assert_eq!(run["status"], "done", "{sql}: {run}");
        assert_eq!(run["statements"][0]["rows"], 2, "{sql}: {run}");
    }
}

#[test]
fn values_written_as_literals_and_secret_variables_stay_out_of_the_history() {
    let mut window = Window::new();
    let Some((memory, sqlite)) = with_sqlite(&mut window) else { return };
    let inlined = window.run(
        "db.query.run",
        json!({ "connection": sqlite, "sql": "SELECT count(*) FROM rows WHERE name = :token", "parameters": { "token": "s3cr3t-tok" }, "waitMs": 30_000 }),
    );
    assert_eq!(inlined["status"], "done", "{inlined}");
    let expanded = window.run(
        "db.query.run",
        json!({ "connection": memory, "sql": "SELECT '${password}', '${schema}'",
                "variables": { "password": "hunter2", "schema": "shop" }, "waitMs": 10_000 }),
    );
    assert_eq!(expanded["status"], "done", "{expanded}");
    let answers = format!("{}{}{}", inlined["statements"], inlined["messages"], expanded["statements"]);
    assert!(!answers.contains("s3cr3t-tok") && !answers.contains("hunter2"), "{answers}");
    let history = window.run("db.history.list", json!({})).to_string();
    assert!(!history.contains("s3cr3t-tok") && !history.contains("hunter2"), "{history}");
    assert!(history.contains("WHERE name = :token"), "the placeholder as written: {history}");
    assert!(history.contains("SELECT '***', 'shop'"), "secret variables masked: {history}");
}

#[test]
fn clickhouse_strings_with_backslash_escapes_hold_no_placeholder() {
    let mut window = Window::new();
    window.session.state_mut::<super::super::Database>().set_drivers(zen_db::drivers());
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Events", "driver": "clickhouse", "host": "ch.local", "type": zen_db::types::DEVELOPMENT }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    window.run("db.console.new", json!({ "connection": id, "text": "SELECT 'it\\'s :v', `a:b`, :w;\n" }));
    let listed = window.run("db.console.parameters", json!({}));
    let names: Vec<&str> =
        listed["parameters"].as_array().unwrap().iter().map(|asked| asked["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["w"], "a quote escaped with a backslash does not end the string: {listed}");
    assert_eq!(listed["inlined"], true);
}
