use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_syntax::{HighlightKind, Highlighter, Span};

use super::super::tests::{Window, orders_connection, sql_connection};
use crate::Workbench;
use crate::brackets::Highlighters;

struct Quotes;

impl Highlighter for Quotes {
    fn handles(&self, _: &str) -> bool {
        true
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let bytes = text.as_bytes();
        let mut spans = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let quote = bytes[at];
            if quote != b'"' && quote != b'`' {
                at += 1;
                continue;
            }
            let mut end = at + 1;
            while end < bytes.len() && bytes[end] != quote {
                end += if quote == b'"' && bytes[end] == b'\\' { 2 } else { 1 };
            }
            let end = (end + 1).min(bytes.len());
            spans.push(Span { range: at..end, kind: HighlightKind::String });
            at = end;
        }
        spans
    }
}

pub(in crate::database) fn with_highlighting(window: &mut Window) {
    let highlighters = window.session.state_mut::<Highlighters>();
    highlighters.set_source(Arc::new(|_: &str| Some(Arc::new(Quotes) as Arc<dyn Highlighter>)));
    highlighters.set_injectors(Arc::new(|name: &str| {
        if zen_db::inject::hosts(name) {
            vec![Arc::new(zen_db::inject::SqlStrings::new(None)) as Arc<dyn zen_syntax::inject::Injector>]
        } else {
            Vec::new()
        }
    }));
}

pub(in crate::database) fn open(window: &mut Window, name: &str, text: &str) -> PathBuf {
    let path = window.dir.path().join("project").join(name);
    std::fs::write(&path, text).unwrap();
    window.run("file.document.open", json!({ "path": path }));
    path
}

fn caret(window: &mut Window, offset: usize) {
    window.run("editor.selection.set", json!({ "selections": [{ "head": offset }] }));
}

fn labels(list: &Value) -> Vec<String> {
    list["items"].as_array().unwrap().iter().map(|item| item["label"].as_str().unwrap().to_owned()).collect()
}

fn code(error: zen_core::DispatchError) -> Option<String> {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.code,
        other => panic!("{other}"),
    }
}

const GO: &str = "package main\n\nconst q = \"SELECT o.totl FROM orders o\"\n\nvar r = `SELECT *\n  FROM nothing`\n\nvar s = \"Select a file\"\n";

#[test]
fn sql_in_go_strings_is_listed_checked_and_completed_from_the_snapshot() {
    let mut window = Window::new();
    let id = orders_connection(&mut window, true);
    with_highlighting(&mut window);
    let path = open(&mut window, "repo.go", GO);

    let listed = window.run("db.injection.list", json!({}));
    assert_eq!(listed["binding"], "none", "{listed}");
    assert_eq!(listed["total"], 2);
    assert_eq!(listed["fragments"][0]["text"], "SELECT o.totl FROM orders o");
    assert_eq!(listed["fragments"][0]["line"], 3);
    assert_eq!(listed["fragments"][0]["start"], GO.find("SELECT").unwrap());
    assert_eq!(listed["fragments"][1]["text"], "SELECT *\n  FROM nothing");
    let limited = window.run("db.injection.list", json!({ "limit": 1 }));
    assert_eq!((limited["fragments"].as_array().unwrap().len(), &limited["truncated"]), (1, &json!(true)));

    let unbound = window.registry.dispatch(&mut window.session, "db.sql.complete", json!({ "offset": 30 }));
    assert_eq!(code(unbound.unwrap_err()).as_deref(), Some("noBinding"));
    let keyword_at = GO.find("FROM orders").unwrap() + 2;
    caret(&mut window, keyword_at);
    let keywords = window.run("editor.completion.start", json!({}));
    assert!(labels(&keywords).iter().any(|label| label == "FROM"), "keywords without a connection: {keywords}");

    let set = window.run("db.injection.setDefault", json!({ "connection": id }));
    assert_eq!(set["connection"], id.as_str());
    assert_eq!(window.run("db.injection.list", json!({}))["binding"], "project");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let problems = loop {
        let answer = window.run("problems.file.list", json!({ "path": path }));
        let problems = crate::language::listed_problems(&answer);
        if !problems.is_empty() || std::time::Instant::now() > deadline {
            break problems;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let messages: Vec<&str> = problems.iter().map(|problem| problem["message"].as_str().unwrap()).collect();
    assert_eq!(messages, ["Unknown column totl in o", "Unknown table nothing"], "{problems:?}");
    assert_eq!((problems[0]["line"].as_u64(), problems[0]["column"].as_u64()), (Some(3), Some(21)));
    assert_eq!((problems[1]["line"].as_u64(), problems[1]["column"].as_u64()), (Some(6), Some(8)));

    let after_dot = GO.find("o.totl").unwrap() + 2;
    let answer = window.run("db.sql.complete", json!({ "path": path, "offset": after_dot }));
    assert_eq!(answer["connection"], id.as_str());
    assert_eq!(labels(&answer), ["id", "customer_id", "total"], "{answer}");
    assert_eq!(answer["from"], after_dot);

    let resolved = window.run("db.sql.resolve", json!({ "offset": GO.find("orders").unwrap() + 1 }));
    assert_eq!(resolved["target"]["path"], json!(["shop", "public", "tables", "orders"]));
    assert_eq!(resolved["range"], json!([GO.find("orders").unwrap(), GO.find("orders").unwrap() + 6]));

    window.run("editor.selection.set", json!({ "selections": [{ "anchor": after_dot, "head": after_dot + 4 }] }));
    window.run("editor.edit.insert", json!({ "text": "to" }));
    let listed = window.run("editor.completion.start", json!({}));
    let total =
        listed["items"].as_array().unwrap().iter().find(|item| item["label"] == "total").unwrap()["index"].clone();
    window.run("editor.completion.accept", json!({ "index": total }));
    let text = window.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert!(text.contains("\"SELECT o.total FROM orders o\""), "{text}");
    assert_ne!(window.run("db.connection.status", json!({ "connection": id }))["state"], "connected");

    let own = window.run("db.injection.bind", json!({ "connection": id, "schema": "public" }));
    assert_eq!((own["binding"].as_str(), own["schema"].as_str()), (Some("file"), Some("public")));
    let back = window.run("db.injection.bind", json!({}));
    assert_eq!(back["binding"], "project");
    let cleared = window.run("db.injection.setDefault", json!({}));
    assert!(cleared["connection"].is_null());
    assert_eq!(window.run("db.injection.list", json!({}))["binding"], "none");
}

#[test]
fn rust_and_typescript_strings_are_found_and_other_files_refused() {
    let mut window = Window::new();
    with_highlighting(&mut window);
    open(
        &mut window,
        "db.rs",
        "let row = sqlx::query!(\"SELECT id FROM users WHERE id = $1\", id);\nlet s = \"plain text\";\n",
    );
    let listed = window.run("db.injection.list", json!({}));
    assert_eq!(listed["total"], 1, "{listed}");
    assert_eq!(listed["fragments"][0]["text"], "SELECT id FROM users WHERE id = $1");
    open(
        &mut window,
        "repo.ts",
        "// language=SQL\nconst a = \"pick everything\";\nconst b = sql`select * from t where id = ${id}`;\n",
    );
    let listed = window.run("db.injection.list", json!({}));
    assert_eq!(listed["total"], 2, "{listed}");
    open(&mut window, "notes.md", "SELECT 1");
    let refused = window.registry.dispatch(&mut window.session, "db.injection.list", json!({}));
    assert_eq!(code(refused.unwrap_err()).as_deref(), Some("notCode"));
    let unknown =
        window.registry.dispatch(&mut window.session, "db.injection.setDefault", json!({ "connection": "nope" }));
    assert!(unknown.is_err());
}

const RUN: &str = "package main\n\nfunc load() {\n\tdb.Query(\"SELECT id FROM customers WHERE id = $1\", id)\n\tdb.Exec(\"UPDATE customers SET name = \\\"x\\\" WHERE id = 2; SELECT 2\")\n}\n";

fn refusal(window: &mut Window, command: &str, params: Value) -> (Option<String>, Value) {
    match window.registry.dispatch(&mut window.session, command, params) {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            (error.code, Value::Object(error.data.unwrap_or_default()))
        }
        other => panic!("{other:?}"),
    }
}

fn finished(window: &mut Window, started: &Value) -> Value {
    window.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }))
}

#[test]
fn sql_in_a_go_string_runs_on_the_files_connection_with_bind_parameters() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    with_highlighting(&mut window);
    let path = open(&mut window, "load.go", RUN);
    let first = RUN.find("SELECT id").unwrap() + 3;

    caret(&mut window, 2);
    assert_eq!(refusal(&mut window, "db.injection.run", json!({})).0.as_deref(), Some("noFragment"));
    caret(&mut window, first);
    let (code_of, data) = refusal(&mut window, "db.injection.run", json!({}));
    assert_eq!(code_of.as_deref(), Some("noBinding"));
    assert_eq!(data["connections"][0]["id"], id.as_str(), "{data}");
    assert!(driver.server.executed().is_empty(), "a refused run connects nowhere");
    let listed = window.run("editor.intentions.list", json!({}));
    let titles: Vec<&str> =
        listed["actions"].as_array().unwrap().iter().map(|a| a["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Run SQL", "Choose SQL Connection…"], "{listed}");
    let (code_of, data) = refusal(&mut window, "editor.intentions.apply", json!({ "id": "db.injection.choose" }));
    assert_eq!((code_of.as_deref(), &data["connections"][0]["id"]), (Some("chooseConnection"), &json!(id)));

    window.run("db.injection.bind", json!({ "connection": id }));
    let (code_of, data) = refusal(&mut window, "db.injection.run", json!({}));
    assert_eq!(code_of.as_deref(), Some("parametersNeeded"), "{data}");
    assert_eq!(data["parameters"][0]["name"], "$1");
    assert_eq!(data["retry"]["command"], "db.injection.run");
    let mut again = data["retry"]["params"].clone();
    again["parameters"] = json!({ "$1": 7 });
    let started = window.run("db.injection.run", again);
    assert_eq!((started["statements"].as_u64(), started["connection"].as_str()), (Some(1), Some(id.as_str())));
    assert_eq!(started["tab"]["path"], json!(path));
    let host = window.run("workspace.host.state", json!({ "id": super::WORKSPACE }));
    assert_eq!(host["expanded"], true, "the results show in the Database workspace");
    assert_eq!(finished(&mut window, &started)["status"], "done");
    let bound = driver.server.bound();
    assert_eq!(bound.last().map(|(text, _)| text.as_str()), Some("SELECT id FROM customers WHERE id = $1"));
    let state = window.run("db.console.state", json!({ "path": path }));
    assert_eq!(state["last"]["statements"], 1, "{state}");
    assert_eq!(state["tabs"].as_array().map(Vec::len), Some(1), "one result tab: {state}");
    let quotes = RUN.find("\"SELECT id").unwrap();
    assert_eq!(state["statement"], json!([quotes + 1, quotes + 39]), "the band is the SQL inside the quotes");

    let second = RUN.find("SELECT 2").unwrap();
    caret(&mut window, second);
    let statement = window.run("db.console.statement", json!({ "path": path }));
    assert_eq!(statement["statement"]["text"], "SELECT 2");
    let started = window.run("db.injection.run", json!({ "scope": "script", "offset": second }));
    assert_eq!(started["statements"], 2);
    let state = finished(&mut window, &started);
    assert_eq!(state["statements"][0]["start"], RUN.find("UPDATE").unwrap(), "marks land in the file: {state}");
    assert_eq!(state["statements"][1]["start"], second);
    assert!(
        driver.server.executed().iter().any(|text| text == "UPDATE customers SET name = \"x\" WHERE id = 2"),
        "escapes are read: {:?}",
        driver.server.executed()
    );
    assert_eq!(window.run("db.injection.list", json!({}))["binding"], "file");
}

#[test]
fn production_asks_before_a_change_from_code_and_runs_it_with_the_token() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "prod", true);
    with_highlighting(&mut window);
    open(&mut window, "load.go", RUN);
    window.run("db.injection.setDefault", json!({ "connection": id }));
    let change = RUN.find("UPDATE").unwrap();
    let (code_of, data) = refusal(&mut window, "db.injection.run", json!({ "offset": change }));
    assert_eq!(code_of.as_deref(), Some("confirmExecute"), "{data}");
    assert_eq!(data["retry"]["command"], "db.injection.run");
    assert!(driver.server.executed().is_empty(), "nothing ran");
    let started = window.run("db.injection.run", data["retry"]["params"].clone());
    assert_eq!(finished(&mut window, &started)["status"], "done");
    assert!(
        driver.server.executed().iter().any(|text| text.starts_with("UPDATE customers")),
        "{:?}",
        driver.server.executed()
    );
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn sql_in_a_go_string_runs_on_postgresql_with_a_bound_parameter() {
    let port: u16 = std::env::var("ZEN_DB_PG_PORTS")
        .ok()
        .and_then(|ports| ports.split(',').filter_map(|port| port.trim().parse().ok()).next_back())
        .unwrap_or(54318);
    let mut window = Window::new();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "PG", "url": format!("postgresql://postgres:zen@127.0.0.1:{port}/postgres?sslmode=disable") }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    with_highlighting(&mut window);
    let text = "package main\n\nvar q = \"SELECT $1::int + 1 AS answer\"\nvar e = \"SELECT nope FROM pg_class\"\n";
    open(&mut window, "pg.go", text);
    window.run("db.injection.bind", json!({ "connection": id }));
    let at = text.find("$1::int").unwrap();
    let started = window.run("db.injection.run", json!({ "offset": at, "parameters": { "$1": 41 } }));
    let state = finished(&mut window, &started);
    assert_eq!(state["status"], "done", "{state}");
    let result = state["results"][0]["id"].clone();
    let page = window.run("db.result.page", json!({ "id": result }));
    assert_eq!(page["page"][0][0], json!({ "kind": "integer", "value": 42 }), "{page}");
    let wrong = text.find("nope").unwrap();
    let started = window.run("db.injection.run", json!({ "offset": wrong }));
    let state = finished(&mut window, &started);
    assert_eq!(state["status"], "failed", "{state}");
    let console = window.run("db.console.state", json!({}));
    assert!(console["hints"].as_array().unwrap().iter().any(|hint| hint["error"] == true), "{console}");
    let path = window.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
    let problems = super::super::consoles::problems(&window.session, &path);
    assert_eq!(problems.first().map(|problem| problem.start), Some(wrong), "the error is marked on the column");
    window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
}
