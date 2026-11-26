use serde_json::{Value, json};

use super::OPEN_TO_AGENTS;
use crate::database::tests::{Window, open_customers, sql_connection};

const PRIVATE: [&str; 5] = ["customers", "picture", "payload", "customers_pkey", "zz-private"];

fn as_agent(window: &mut Window, command: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
    let registry = &window.registry;
    zen_core::macros::as_agent(&mut window.session, |session| registry.dispatch(session, command, params))
}

fn leaked(text: &str) -> Option<&'static str> {
    PRIVATE.into_iter().find(|word| text.contains(word))
}

struct Closed {
    window: Window,
    id: String,
    console: String,
    tab: String,
    result: u64,
}

fn closed() -> Closed {
    let mut window = Window::new();
    window.run("db.connectionType.create", json!({ "id": "secret", "name": "Secret", "agents": "none" }));
    let (id, _driver) = sql_connection(&mut window, "secret", true);
    window.run("db.connection.connect", json!({ "connection": id }));
    window.run("db.navigator.expand", json!({ "connection": id, "path": ["shop", "public", "tables"] }));
    let opened =
        window.run("db.console.new", json!({ "connection": id, "text": "SELECT * FROM customers; -- zz-private\n" }));
    let console = opened["path"].as_str().unwrap().to_owned();
    let (tab, result) = open_customers(&mut window, &id);
    Closed { window, id, console, tab, result }
}

#[test]
fn every_command_open_to_agents_exists() {
    let window = Window::new();
    for id in OPEN_TO_AGENTS {
        assert!(window.registry.info(id).is_some(), "{id} is not a command");
    }
}

#[test]
fn an_agent_learns_nothing_of_a_closed_connection_from_any_database_command() {
    let mut closed = closed();
    let (id, console, tab, result) = (closed.id.clone(), closed.console.clone(), closed.tab.clone(), closed.result);
    let person = closed.window.run("db.object.state", json!({ "tab": tab }));
    assert!(leaked(&person.to_string()).is_some(), "the person sees the table: {person}");
    let commands: Vec<String> = closed
        .window
        .registry
        .commands()
        .map(|info| info.id.as_str().to_owned())
        .filter(|command| command.starts_with("db.") && !OPEN_TO_AGENTS.contains(&command.as_str()))
        .collect();
    assert!(commands.len() > 150, "{}", commands.len());
    let shapes = [
        json!({}),
        json!({ "connection": id }),
        json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }),
        json!({ "path": console }),
        json!({ "tab": tab }),
        json!({ "id": result }),
        json!({ "selection": { "id": result } }),
        json!({ "query": "custom" }),
        json!({ "connections": [id] }),
    ];
    let mut refused = 0;
    for command in &commands {
        for params in &shapes {
            match as_agent(&mut closed.window, command, params.clone()) {
                Ok(answer) => {
                    let text = answer.to_string();
                    assert!(leaked(&text).is_none(), "{command} {params} answers {:?}: {text}", leaked(&text));
                }
                Err(zen_core::DispatchError::Failed { error, .. }) => {
                    let text = format!("{} {:?}", error.message, error.data);
                    assert!(leaked(&text).is_none(), "{command} {params} refuses with {text}");
                    refused += usize::from(error.code.as_deref() == Some("agentNotAllowed"));
                }
                Err(_) => {}
            }
        }
    }
    assert!(refused > 100, "refused as closed {refused} times");
    let state = closed.window.run("db.object.state", json!({ "tab": closed.tab }));
    assert!(leaked(&state.to_string()).is_some(), "{state}");
    let statement = closed.window.run("db.console.statement", json!({ "path": closed.console, "offset": 3 }));
    assert!(leaked(&statement.to_string()).is_some(), "{statement}");
}

#[test]
fn the_named_commands_refuse_with_agent_not_allowed() {
    let mut closed = closed();
    let id = closed.id.clone();
    let path = json!(["shop", "public", "tables", "customers"]);
    let calls = [
        ("db.object.goTo", json!({ "query": "custom", "connection": id })),
        ("db.object.generateCode", json!({ "connection": id, "path": path, "language": "go" })),
        ("db.object.get", json!({ "connection": id, "path": path })),
        ("db.object.ddl", json!({ "connection": id, "path": path })),
        ("db.schema.children", json!({ "connection": id, "path": ["shop", "public"] })),
        ("db.navigator.expand", json!({ "connection": id, "path": ["shop"] })),
        ("db.object.open", json!({ "connection": id, "path": path })),
        ("db.object.state", json!({ "tab": closed.tab })),
        ("db.sql.complete", json!({ "path": closed.console, "offset": 14 })),
        ("db.console.state", json!({ "path": closed.console })),
        ("db.object.properties", json!({ "connection": id, "path": path })),
        ("db.search.targets", json!({ "connection": id })),
        ("db.result.page", json!({ "id": closed.result })),
    ];
    for (command, params) in calls {
        match as_agent(&mut closed.window, command, params.clone()) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some("agentNotAllowed"), "{command}: {}", error.message)
            }
            other => panic!("{command} {params}: {other:?}"),
        }
    }
}

#[test]
fn lists_leave_a_closed_connection_out_for_agents() {
    let mut closed = closed();
    let state = as_agent(&mut closed.window, "db.navigator.state", json!({})).unwrap();
    let text = state.to_string();
    assert!(leaked(&text).is_none(), "{text}");
    assert_eq!(state["rows"], json!([]), "left out, not refused: {text}");
    let found = as_agent(&mut closed.window, "db.object.goTo", json!({ "query": "custom" })).unwrap();
    assert!(leaked(&found.to_string()).is_none(), "{found}");
    let results = as_agent(&mut closed.window, "db.results.list", json!({})).unwrap();
    assert!(!results.to_string().contains(&format!("\"{}\"", closed.id)), "{results}");
    let consoles = as_agent(&mut closed.window, "db.console.list", json!({})).unwrap();
    assert!(leaked(&consoles.to_string()).is_none(), "{consoles}");
    let sources = as_agent(&mut closed.window, "db.dataSource.list", json!({})).unwrap();
    assert!(sources.to_string().contains(&closed.id), "{sources}");
    let person = closed.window.run("db.navigator.state", json!({}));
    assert_eq!(person["rows"][0]["key"]["connection"], json!(closed.id), "{person}");
}

#[test]
fn a_saved_snapshot_of_a_closed_connection_is_closed_too() {
    let mut closed = closed();
    let saved = closed.window.run("db.compare.saveSnapshot", json!({ "connection": closed.id, "schema": "public" }));
    let file = std::path::PathBuf::from(saved["path"].as_str().unwrap());
    let elsewhere = closed.window.dir.path().join("project/schema.json");
    std::fs::copy(&file, &elsewhere).unwrap();
    for path in [file, elsewhere] {
        let side = json!({ "kind": "snapshot", "path": path });
        match as_agent(&mut closed.window, "db.compare.schemas", json!({ "source": side, "target": side })) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                let code = error.code.as_deref().unwrap_or_default();
                assert!(code == "agentNotAllowed" || code == "redacted", "{path:?}: {error:?}");
                assert!(leaked(&error.message).is_none(), "{}", error.message);
            }
            other => panic!("{path:?}: {other:?}"),
        }
    }
    let listed = as_agent(&mut closed.window, "db.compare.snapshots", json!({})).unwrap();
    assert_eq!(listed["snapshots"], json!([]), "{listed}");
}

#[test]
fn the_workspace_shows_an_agent_no_tab_of_a_closed_connection() {
    let mut closed = closed();
    let person = closed.window.run("workspace.host.state", json!({ "id": "database" }));
    assert!(leaked(&person.to_string()).is_some(), "{person}");
    let agent = as_agent(&mut closed.window, "workspace.host.state", json!({ "id": "database" })).unwrap();
    assert!(leaked(&agent.to_string()).is_none(), "{agent}");
    assert!(!agent.to_string().contains(&closed.tab), "{agent}");
    match as_agent(&mut closed.window, "workspace.tab.select", json!({ "workspace": "database", "tab": closed.tab })) {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some("agentNotAllowed"))
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_agent_cannot_transfer_from_or_into_a_closed_connection() {
    let mut closed = closed();
    let (open, _) = sql_connection(&mut closed.window, "open", true);
    let table = json!(["shop", "public", "tables", "customers"]);
    for (source, target) in [(closed.id.clone(), open.clone()), (open.clone(), closed.id.clone())] {
        for command in ["db.transfer.preview", "db.transfer.start"] {
            let tables = if command.ends_with("preview") { json!([table]) } else { json!([{ "source": table }]) };
            let params = json!({ "source": source, "target": target, "schema": ["shop", "public"], "tables": tables });
            let refused = as_agent(&mut closed.window, command, params).unwrap_err();
            let zen_core::DispatchError::Failed { error, .. } = refused else { panic!("{refused:?}") };
            assert_eq!(error.code.as_deref(), Some("agentNotAllowed"), "{command}");
        }
    }
}

#[test]
fn an_agent_neither_removes_nor_duplicates_a_closed_connection() {
    let mut closed = closed();
    for command in ["db.dataSource.remove", "db.dataSource.removal", "db.dataSource.duplicate"] {
        match as_agent(&mut closed.window, command, json!({ "id": closed.id })) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some("agentNotAllowed"), "{command}: {}", error.message)
            }
            other => panic!("{command}: {other:?}"),
        }
    }
    let sources = closed.window.run("db.dataSource.list", json!({}));
    assert_eq!(sources.to_string().matches("Shop secret").count(), 1, "still there, and only once: {sources}");
    let copy = closed.window.run("db.dataSource.duplicate", json!({ "id": closed.id }));
    closed.window.run("db.dataSource.remove", json!({ "id": copy["id"] }));
}
