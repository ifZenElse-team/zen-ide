use std::path::PathBuf;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::{LOCAL_CONNECTION, OVERVIEW, WORKSPACE, project_key};
use crate::Workbench;
use crate::workspaces::Workspaces;

pub(super) struct Window {
    pub(super) registry: Registry,
    pub(super) session: Session,
    pub(super) dir: tempfile::TempDir,
    secrets: std::sync::Arc<zen_secrets::MemoryStore>,
}

impl Window {
    pub(super) fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        super::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("main.go"), "package main\n").unwrap();
        let mut session = Session::new();
        super::install(&mut session);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
        let secrets = std::sync::Arc::new(zen_secrets::MemoryStore::new());
        session.set_state(super::Secrets(secrets.clone()));
        let mut window = Self { registry, session, dir, secrets };
        window.run("file.document.open", json!({ "path": window.dir.path().join("project/main.go") }));
        window
    }

    pub(super) fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    pub(super) fn run_confirmed(&mut self, command: &str, params: Value) -> Value {
        match self.registry.dispatch(&mut self.session, command, params) {
            Ok(answer) => answer,
            Err(zen_core::DispatchError::Failed { error, .. }) if error.code.as_deref() == Some("confirmExecute") => {
                let retry = error.data.unwrap()["retry"].clone();
                self.run(retry["command"].as_str().unwrap(), retry["params"].clone())
            }
            Err(error) => panic!("{error}"),
        }
    }

    pub(super) fn refused(&mut self, command: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, command, params).unwrap_err().to_string()
    }

    fn folder(&self) -> PathBuf {
        let project = self.session.state::<Workbench>().unwrap().project().unwrap().to_owned();
        self.dir.path().join("data/consoles").join(project_key(&project)).join(LOCAL_CONNECTION)
    }

    fn with_connection(mut self) -> Self {
        with_drivers(&mut self);
        self.run("db.dataSource.add", json!({ "name": "Local", "url": "postgresql://ann@localhost:5432/shop" }));
        self
    }

    fn state(&mut self) -> Value {
        self.run("workspace.host.state", json!({ "id": WORKSPACE }))
    }

    fn titles(&mut self) -> Vec<String> {
        let state = self.state();
        state["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap().to_owned()).collect()
    }
}

#[test]
fn the_workspace_starts_with_an_overview_that_never_closes() {
    let mut window = Window::new();
    let listed = window.run("workspace.host.list", json!({}));
    assert_eq!(listed["workspaces"][0]["id"], WORKSPACE);
    assert_eq!(listed["workspaces"][0]["title"], "Database");
    assert_eq!(window.titles(), ["Overview"]);
    let state = window.run("workspace.host.expand", json!({ "id": WORKSPACE }));
    assert_eq!(state["active"], OVERVIEW);
    assert_eq!(state["tabs"][0]["closable"], false);
    let refused = window.refused("workspace.tab.close", json!({ "workspace": WORKSPACE, "tab": OVERVIEW }));
    assert!(refused.contains("Overview"), "{refused}");
    assert!(super::super::workspaces::saved(&mut window.session).len() == 1, "kept while expanded");
    window.run("workspace.host.collapse", json!({}));
    let saved = super::super::workspaces::saved(&mut window.session);
    assert!(saved.len() == 1 && saved[0].open, "collapsed, it stays open with its tab in the strip");
    window.run("workspace.host.close", json!({ "id": WORKSPACE }));
    assert!(super::super::workspaces::saved(&mut window.session).is_empty(), "nothing to keep with only the Overview");
}

#[test]
fn a_new_console_takes_the_navigators_connection_else_the_one_in_front() {
    let mut window = Window::new().with_connection();
    window.run("db.dataSource.add", json!({ "name": "Reports", "url": "postgresql://ann@localhost:5432/reports" }));
    let listed = window.run("db.dataSource.list", json!({}));
    let id = |name: &str| {
        listed["connections"].as_array().unwrap().iter().find(|known| known["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (local, reports) = (id("Local"), id("Reports"));
    let first = window.run("db.console.new", json!({ "connection": local }));
    assert_eq!(first["connection"], local.as_str());
    let second = window.run("db.console.new", json!({}));
    assert_eq!(second["connection"], local.as_str(), "the connection of the console in front");
    window.run("db.navigator.select", json!({ "connection": reports }));
    let third = window.run("db.console.new", json!({}));
    assert_eq!(third["connection"], reports.as_str(), "the connection selected in the navigator");
}

#[test]
fn the_database_workspace_hosts_sql_files_and_leaves_code_to_the_editor() {
    let mut window = Window::new().with_connection();
    let code = window.dir.path().join("project/main.rs");
    std::fs::write(&code, "fn main() {}\n").unwrap();
    let refused = window.registry.dispatch(
        &mut window.session,
        "workspace.tab.open",
        json!({ "workspace": WORKSPACE, "path": code }),
    );
    let Err(zen_core::DispatchError::Failed { error, .. }) = refused else { panic!("main.rs opened in the workspace") };
    assert_eq!(error.code.as_deref(), Some("notHosted"));
    assert!(error.message.contains("main.rs"), "{}", error.message);
    assert_eq!(window.titles(), ["Overview"]);
    let script = window.dir.path().join("project/report.sql");
    std::fs::write(&script, "select 1;\n").unwrap();
    window.run("workspace.tab.open", json!({ "workspace": WORKSPACE, "path": script }));
    assert_eq!(window.titles(), ["Overview", "report.sql"]);
}

#[test]
fn new_consoles_are_numbered_in_the_data_folder_and_open_in_the_workspace() {
    let mut window = Window::new().with_connection();
    let first = window.run("db.console.new", json!({}));
    let path = PathBuf::from(first["path"].as_str().unwrap());
    assert_eq!(path, window.folder().join("console.sql"));
    assert!(path.is_file());
    assert_eq!(first["connection"], LOCAL_CONNECTION);
    let project = window.dir.path().join("project");
    assert!(!path.starts_with(&project), "never in the project");
    let state = window.state();
    assert_eq!(state["expanded"], true);
    assert_eq!(state["active"], first["tab"]["id"]);
    let active = window.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
    assert_eq!(active, path, "the console is the document in front");

    window.run("editor.edit.insert", json!({ "text": "select 1;" }));
    window.run("editor.edit.undo", json!({}));
    window.run("editor.edit.insert", json!({ "text": "select 2;" }));
    let second = window.run("db.console.new", json!({ "connection": LOCAL_CONNECTION, "text": "select 3;" }));
    assert_eq!(PathBuf::from(second["path"].as_str().unwrap()), window.folder().join("console_1.sql"));
    assert_eq!(window.titles(), ["Overview", "console.sql", "console_1.sql"]);
    let text = std::fs::read_to_string(window.folder().join("console_1.sql")).unwrap();
    assert_eq!(text, "select 3;");

    let other = window.run("db.console.new", json!({ "connection": "reports" }));
    assert!(other["path"].as_str().unwrap().ends_with("reports/console.sql"), "{other}");
    let refused = window.refused("db.console.new", json!({ "connection": "../x" }));
    assert!(refused.contains("../x"), "{refused}");
    let mut empty = Window::new();
    let refused = empty.registry.dispatch(&mut empty.session, "db.console.new", json!({})).unwrap_err();
    let zen_core::DispatchError::Failed { error, .. } = refused else { panic!("{refused}") };
    assert_eq!(error.code.as_deref(), Some(super::NO_CONNECTION), "no console without a connection");
    assert!(!error.message.contains("db."), "a person reads no command id: {}", error.message);
    assert!(error.message.contains("New Connection…"), "{}", error.message);
    let registry = &empty.registry;
    let refused = zen_core::macros::as_agent(&mut empty.session, |session| {
        registry.dispatch(session, "db.console.new", json!({})).unwrap_err()
    });
    let zen_core::DispatchError::Failed { error, .. } = refused else { panic!("{refused}") };
    assert!(error.message.contains("`db.dataSource.add`"), "an agent gets the command: {}", error.message);

    let listed = window.run("db.console.list", json!({ "limit": 2 }));
    assert_eq!(listed["total"], 3);
    assert_eq!(listed["truncated"], true);
    assert_eq!(listed["consoles"].as_array().unwrap().len(), 2);
    let only = window.run("db.console.list", json!({ "connection": "reports" }));
    assert_eq!(only["total"], 1);
    assert_eq!(only["consoles"][0]["open"], true);
}

#[test]
fn closing_an_empty_console_deletes_it_and_a_written_one_stays() {
    let mut window = Window::new().with_connection();
    let empty = window.run("db.console.new", json!({}));
    let empty_path = PathBuf::from(empty["path"].as_str().unwrap());
    let written = window.run("db.console.new", json!({}));
    let written_path = PathBuf::from(written["path"].as_str().unwrap());
    window.run("editor.edit.insert", json!({ "text": "select 1;" }));
    window.run("workspace.tab.close", json!({ "workspace": WORKSPACE, "tab": written["tab"]["id"] }));
    assert_eq!(std::fs::read_to_string(&written_path).unwrap(), "select 1;", "saved on close and kept");
    window.run("workspace.tab.close", json!({ "workspace": WORKSPACE, "tab": empty["tab"]["id"] }));
    assert!(!empty_path.exists(), "an empty console is deleted");
    assert_eq!(window.titles(), ["Overview"]);
    let recent = &window.session.state::<Workbench>().unwrap().recent;
    assert!(recent.contains(&written_path), "consoles are recent files: {recent:?}");
    assert!(!recent.contains(&empty_path), "a deleted console leaves Recent Files");
}

#[test]
fn consoles_and_the_overview_come_back_after_a_restart() {
    let mut window = Window::new().with_connection();
    let console = window.run("db.console.new", json!({ "text": "select 1;" }));
    let saved = super::super::workspaces::saved(&mut window.session);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].tabs.len(), 2);

    let mut restarted = Window::new();
    super::super::workspaces::restore(&mut restarted.session, saved);
    assert_eq!(restarted.titles(), ["Overview", "console.sql"]);
    let state = restarted.state();
    assert_eq!(state["expanded"], true);
    assert_eq!(state["active"], console["tab"]["id"]);
    let key = project_key(std::path::Path::new("/Users/someone/My Project"));
    assert!(key.starts_with("My_Project-") && key.len() == "My_Project-".len() + 8, "{key}");
}

fn with_drivers(window: &mut Window) -> std::sync::Arc<zen_db::memory::MemoryDriver> {
    let driver = std::sync::Arc::new(zen_db::memory::MemoryDriver::new(false));
    let drivers: Vec<std::sync::Arc<dyn zen_db::driver::Driver>> =
        vec![std::sync::Arc::new(zen_db::postgres::Postgres), std::sync::Arc::clone(&driver) as _];
    window.session.state_mut::<super::Database>().set_drivers(drivers);
    driver
}

pub(super) fn wait(window: &mut Window, operation: &Value) -> Value {
    let id = operation.as_u64().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let state = window.run("db.operation.state", json!({ "operation": id }));
        if state["status"] != "running" || std::time::Instant::now() > deadline {
            return state;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn consoles_of_the_same_file_name_are_told_apart_by_their_connection() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let shop = window.run("db.dataSource.add", json!({ "name": "Shop", "driver": "memory-kv", "password": "a" }));
    let billing = window.run("db.dataSource.add", json!({ "name": "Billing", "driver": "memory-kv", "password": "b" }));
    let first = window.run("db.console.new", json!({ "connection": shop["id"] }));
    assert_eq!(window.titles(), ["Overview", "console.sql"], "a single console keeps its file name");
    let second = window.run("db.console.new", json!({ "connection": billing["id"] }));
    assert_eq!(window.titles(), ["Overview", "console.sql (Shop)", "console.sql (Billing)"]);
    window.run("db.console.new", json!({ "connection": shop["id"] }));
    assert_eq!(window.titles(), ["Overview", "console.sql (Shop)", "console.sql (Billing)", "console_1.sql"]);
    window.run("workspace.tab.close", json!({ "workspace": WORKSPACE, "tab": second["tab"]["id"] }));
    assert_eq!(window.titles(), ["Overview", "console.sql", "console_1.sql"], "alone again, the file name is enough");
    assert!(first["path"].as_str().unwrap().ends_with("console.sql"));
}

#[test]
fn connections_are_kept_without_secrets_in_both_scopes() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Local PG", "url": "postgresql://ann:s3cret@db.local:6543/shop?sslmode=require", "folder": "Team/Reporting" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("local-pg-"), "{id}");
    assert_eq!(added["scope"], "global");
    assert_eq!(added["driver"], "postgresql");
    assert_eq!(
        (added["host"].as_str(), added["port"].as_u64(), added["database"].as_str()),
        (Some("db.local"), Some(6543), Some("shop"))
    );
    assert_eq!(added["user"], "ann");
    assert_eq!(added["url"], "postgresql://ann@db.local:6543/shop?sslmode=require");
    assert_eq!(added["tls"]["mode"], "require");
    assert_eq!(added["type"], "dev", "new connections are Development");
    assert_eq!(added["hasPassword"], true);
    assert_eq!(added["state"], "disconnected");
    let global = window.dir.path().join("data/databases/data-sources.json");
    let text = std::fs::read_to_string(&global).unwrap();
    assert!(!text.contains("s3cret") && text.contains("\"version\": 1"), "{text}");
    use zen_secrets::SecretStore as _;
    use zen_secrets::database::Kind;
    let key = zen_secrets::database::id(None, &id, Kind::Password).unwrap();
    let secrets = window.secrets.clone();
    assert_eq!(wait(&mut window, &added["secretOperation"])["kind"], "storeSecret");
    assert_eq!(secrets.get(&key).unwrap().unwrap().expose(), "s3cret");

    let shared = window.run(
        "db.dataSource.add",
        json!({ "scope": "project", "name": "Team DB", "host": "localhost", "user": "me", "port": 5433, "password": "p2", "type": "test" }),
    );
    let shared_id = shared["id"].as_str().unwrap().to_owned();
    wait(&mut window, &shared["secretOperation"]);
    assert_eq!(
        (shared["user"].as_str(), shared["port"].as_u64(), shared["type"].as_str()),
        (Some("me"), Some(5433), Some("test"))
    );
    let project_file = std::fs::read_to_string(window.dir.path().join("project/.zen/data-sources.json")).unwrap();
    assert!(
        !project_file.contains("\"me\"") && !project_file.contains("p2") && !project_file.contains("5433"),
        "{project_file}"
    );
    let project = window.session.state::<Workbench>().unwrap().project().unwrap().to_owned();
    let overrides = window.dir.path().join("data/databases").join(project_key(&project)).join("overrides.json");
    assert!(std::fs::read_to_string(overrides).unwrap().contains("\"me\""));
    let project_key = project_key(&project);
    let key = zen_secrets::database::id(Some(&project_key), &shared_id, Kind::Password).unwrap();
    assert_eq!(key.account, format!("{project_key}/{shared_id}/password"));
    assert!(secrets.get(&key).unwrap().is_some());
    assert_eq!(shared["hasPassword"], true);
    assert!(!project_file.contains("passwordSaved"), "{project_file}");

    let listed = window.run("db.dataSource.list", json!({}));
    assert_eq!(listed["total"], 2);
    assert_eq!(listed["connections"][0]["name"], "Team DB", "project connections come first");
    assert_eq!(listed["folders"][0]["path"], "Team/Reporting");
    let only = window.run("db.dataSource.list", json!({ "folder": "Team" }));
    assert_eq!(only["total"], 1);
    window.run("db.dataSource.update", json!({ "id": id, "readOnly": true, "settings": { "pooler": true } }));
    let got = window.run("db.dataSource.get", json!({ "id": id }));
    assert_eq!((got["readOnly"].as_bool(), got["settings"]["pooler"].as_bool()), (Some(true), Some(true)));
    assert!(!format!("{listed}{got}{added}{shared}").contains("s3cret"));

    let removed = window.run("db.dataSource.remove", json!({ "id": id }));
    assert_eq!(wait(&mut window, &removed["secretOperation"])["status"], "done");
    assert!(secrets.get(&zen_secrets::database::id(None, &id, Kind::Password).unwrap()).unwrap().is_none());
    assert_eq!(window.run("db.dataSource.list", json!({}))["total"], 1);
    let refused = window.refused("db.dataSource.add", json!({ "name": "X", "driver": "oracle" }));
    assert!(refused.contains("oracle"), "{refused}");
}

#[test]
fn a_password_among_the_urls_parameters_goes_to_the_store_and_never_to_a_file_or_an_answer() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let added = window.run(
        "db.dataSource.add",
        json!({ "scope": "project", "name": "Q", "url": "postgresql://ann@db.local/shop?sslmode=require&password=s3cret&sslpassword=k3y&pooler=true" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    assert_eq!(added["url"], "postgresql://ann@db.local/shop?sslmode=require&pooler=true");
    assert_eq!(added["hasPassword"], true);
    wait(&mut window, &added["secretOperation"]);
    let project = window.session.state::<Workbench>().unwrap().project().unwrap().to_owned();
    let key =
        zen_secrets::database::id(Some(&project_key(&project)), &id, zen_secrets::database::Kind::Password).unwrap();
    use zen_secrets::SecretStore as _;
    assert_eq!(window.secrets.get(&key).unwrap().unwrap().expose(), "s3cret");
    let file = window.dir.path().join("project/.zen/data-sources.json");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("s3cret") && !text.contains("k3y") && !text.contains("sslpassword"), "{text}");

    window.run("db.dataSource.update", json!({ "id": id, "settings": { "sslpassword": "k4y", "pooler": false } }));
    let stored = r#"{"version":1,"connections":[{"id":"old","name":"Old","driver":"postgresql","url":"postgresql://ann@h/db?password=0ld&sslmode=require","settings":{"sslpassword":"0ldkey","pooler":true}}]}"#;
    std::fs::write(window.dir.path().join("data/databases/data-sources.json"), stored).unwrap();
    let listed = window.run("db.dataSource.list", json!({}));
    let got = window.run("db.dataSource.get", json!({ "id": "old" }));
    assert_eq!(got["url"], "postgresql://ann@h/db?sslmode=require");
    assert_eq!(got["settings"], json!({ "pooler": true }));
    let answers = format!("{listed}{got}");
    assert!(!answers.contains("0ld") && !answers.contains("k4y") && !answers.contains("sslpassword"), "{answers}");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("k4y") && !text.contains("sslpassword"), "{text}");
}

#[test]
fn a_sqlite_connection_from_a_url_keeps_its_files_path() {
    let mut window = Window::new();
    let drivers: Vec<std::sync::Arc<dyn zen_db::driver::Driver>> =
        zen_db::drivers().into_iter().filter(|driver| driver.id() == "sqlite").collect();
    window.session.state_mut::<super::Database>().set_drivers(drivers);
    for (url, path) in [
        ("sqlite:///abs/path.db", "/abs/path.db"),
        ("sqlite:data/shop.db", "data/shop.db"),
        ("file:///abs/other.db", "/abs/other.db"),
    ] {
        let added = window.run("db.dataSource.add", json!({ "name": "File", "url": url }));
        let id = added["id"].as_str().unwrap();
        let source = window.run("db.dataSource.get", json!({ "id": id }));
        assert_eq!(
            (source["driver"].as_str(), source["database"].as_str()),
            (Some("sqlite"), Some(path)),
            "{url}: {source}"
        );
        assert!(source.get("host").is_none_or(Value::is_null), "{url}: {source}");
    }
}

#[test]
fn a_url_without_a_password_and_a_server_that_needs_none_connect_without_one() {
    let mut window = Window::new();
    let drivers: Vec<std::sync::Arc<dyn zen_db::driver::Driver>> = vec![
        std::sync::Arc::new(zen_db::postgres::Postgres),
        std::sync::Arc::new(zen_db::redis::Redis),
        std::sync::Arc::new(zen_db::clickhouse::ClickHouse),
    ];
    window.session.state_mut::<super::Database>().set_drivers(drivers);
    let mut auth = |params: Value| window.run("db.dataSource.add", params)["auth"].as_str().unwrap().to_owned();
    assert_eq!(auth(json!({ "name": "R", "url": "redis://localhost:6379/0" })), "none");
    assert_eq!(auth(json!({ "name": "R", "url": "redis://default:pw@localhost:6379/0" })), "password");
    assert_eq!(auth(json!({ "name": "P", "url": "postgresql://ann@db.local/shop" })), "password");
    assert_eq!(auth(json!({ "name": "C", "url": "clickhouse://ch.local:8123/default" })), "none");
    assert_eq!(auth(json!({ "name": "P", "url": "postgresql://ann:pw@db.local/shop" })), "password");
    assert_eq!(auth(json!({ "name": "R", "driver": "redis", "host": "cache" })), "none");
    assert_eq!(auth(json!({ "name": "C", "driver": "clickhouse", "host": "ch" })), "none");
    assert_eq!(auth(json!({ "name": "C", "driver": "clickhouse", "host": "ch", "user": "analyst" })), "password");
    assert_eq!(auth(json!({ "name": "P", "host": "db.local", "user": "ann" })), "password");
    assert_eq!(auth(json!({ "name": "R", "url": "redis://localhost", "auth": "password" })), "password");
    assert_eq!(auth(json!({ "name": "P", "host": "db.local", "auth": "none" })), "none");
}

#[test]
fn connection_types_keep_their_defaults() {
    let mut window = Window::new();
    let listed = window.run("db.connectionType.list", json!({}));
    let types = listed["types"].as_array().unwrap();
    let ids: Vec<_> = types.iter().map(|kind| kind["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["dev", "test", "prod"]);
    assert_eq!((types[0]["autoCommit"].as_bool(), types[0]["closeIdleAfter"].as_u64()), (Some(true), Some(14_400)));
    assert_eq!((types[1]["color"].as_str(), types[1]["confirmDataChange"].as_bool()), (Some("green"), Some(true)));
    assert_eq!(types[2]["autoCommit"], false);
    assert_eq!(types[2]["confirmExecute"], true);
    assert_eq!(
        (types[2]["endIdleTransactionsAfter"].as_u64(), types[2]["closeIdleAfter"].as_u64()),
        (Some(600), Some(3600))
    );
    let mine =
        window.run("db.connectionType.create", json!({ "name": "Staging", "color": "orange", "autoCommit": false }));
    assert_eq!((mine["builtIn"].as_bool(), mine["color"].as_str()), (Some(false), Some("orange")));
    window.run("db.connectionType.update", json!({ "id": "prod", "statementTimeout": 60 }));
    assert_eq!(window.run("db.connectionType.list", json!({}))["types"][2]["statementTimeout"], 60);
    assert!(window.refused("db.connectionType.delete", json!({ "id": "prod" })).contains("Production"));
    assert!(
        window.refused("db.connectionType.create", json!({ "name": "Bad", "color": "#ff0000" })).contains("#ff0000")
    );
    window.run("db.dataSource.add", json!({ "name": "S", "host": "h", "type": mine["id"] }));
    assert!(window.refused("db.connectionType.delete", json!({ "id": mine["id"] })).contains('S'));
    let file = std::fs::read_to_string(window.dir.path().join("data/databases/connection-types.json")).unwrap();
    assert!(file.contains("Staging") && !file.contains("Development"), "{file}");
}

#[test]
fn connecting_needs_a_password_runs_in_the_background_and_disconnects() {
    let mut window = Window::new();
    let driver = with_drivers(&mut window);
    driver.server.require_password("pw");
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "KV", "driver": "memory-kv", "savePassword": false, "type": "prod" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let refused = window
        .registry
        .dispatch(&mut window.session, "db.connection.connect", json!({ "connection": id }))
        .unwrap_err();
    assert!(format!("{refused:?}").contains("passwordNeeded"), "{refused:?}");
    let started = window.run("db.connection.connect", json!({ "connection": id, "password": "pw" }));
    let done = wait(&mut window, &started["operation"]);
    assert_eq!(done["status"], "done", "{done}");
    let status = window.run("db.connection.status", json!({ "connection": id }));
    assert_eq!(status["connections"][0]["state"], "connected");
    assert_eq!(status["connections"][0]["autoCommit"], false, "Production");
    assert_eq!(
        window.run("db.dataSource.get", json!({ "id": id }))["hasPassword"],
        false,
        "a password given at connect is not stored"
    );
    assert!(driver.server.opened().iter().all(|open| open.guards.application_name == "Zen IDE"));
    let engine = window.session.state::<super::Database>().unwrap().started().unwrap().clone();
    let op = engine
        .execute(&id, zen_db::driver::ContextKind::Main, "SET a 1", zen_db::driver::ExecOptions::default())
        .unwrap();
    engine.wait(op, std::time::Duration::from_secs(5));
    let open = window
        .registry
        .dispatch(&mut window.session, "db.connection.disconnect", json!({ "connection": id }))
        .unwrap_err();
    assert!(format!("{open:?}").contains("openTransactions"), "{open:?}");
    let all = window.registry.dispatch(&mut window.session, "db.connection.disconnectAll", json!({})).unwrap_err();
    assert!(format!("{all:?}").contains("openTransactions"));
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    assert_eq!(wait(&mut window, &closing["operation"])["status"], "done");
    let status = window.run("db.connection.status", json!({}));
    assert_eq!(status["connections"][0]["state"], "disconnected");
    let tested = window.run("db.dataSource.test", json!({ "connection": id, "password": "wrong" }));
    let tested = wait(&mut window, &tested["operation"]);
    assert_eq!(tested["status"], "failed");
    assert!(tested["message"].as_str().unwrap().contains("refused"), "{tested}");
    assert!(!tested.to_string().contains("\"pw\""));
}

#[test]
fn a_project_connection_of_an_unknown_type_is_production() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let added = window.run(
        "db.dataSource.add",
        json!({ "scope": "project", "name": "P", "driver": "memory-kv", "type": "theirs", "shared": true }),
    );
    let entry = super::sources::find(&window.session, added["id"].as_str().unwrap()).unwrap();
    let request = super::connections::request(&mut window.session, &entry, None);
    assert_eq!(request.kind.id, "prod");
}

#[test]
fn a_secret_goes_to_the_store_and_is_never_echoed() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let refused = window
        .refused("db.connection.setSecret", json!({ "connection": "c1", "kind": "password", "value": "hunter2" }));
    assert!(refused.contains("no database connection"), "{refused}");
    let memory = window.secrets.clone();
    let c1 = window.run("db.dataSource.add", json!({ "name": "c1", "host": "h", "savePassword": false }));
    let c1_id = c1["id"].as_str().unwrap().to_owned();
    let c2 = window.run("db.dataSource.add", json!({ "name": "c2", "host": "h" }));
    let c2_id = c2["id"].as_str().unwrap().to_owned();
    let project = window.run("db.dataSource.add", json!({ "scope": "project", "name": "p1", "host": "h" }));
    let project_id = project["id"].as_str().unwrap().to_owned();
    let (c1, c2) = (c1_id.as_str(), c2_id.as_str());

    let answer =
        window.run("db.connection.setSecret", json!({ "connection": c1, "kind": "password", "value": "hunter2" }));
    let operation = answer["operation"].clone();
    assert_eq!(answer, json!({ "connection": c1, "kind": "password", "scope": "global", "operation": operation }));
    assert!(!answer.to_string().contains("hunter2"));
    let stored = wait(&mut window, &operation);
    assert_eq!((stored["kind"].as_str(), stored["status"].as_str()), (Some("storeSecret"), Some("done")), "{stored}");
    let id = zen_secrets::database::id(None, c1, zen_secrets::database::Kind::Password).unwrap();
    use zen_secrets::SecretStore as _;
    assert_eq!(memory.get(&id).unwrap().unwrap().expose(), "hunter2");
    let got = window.run("db.dataSource.get", json!({ "id": c1 }));
    assert_eq!((got["hasPassword"].as_bool(), got["savePassword"].as_bool()), (Some(true), Some(true)));

    let token =
        window.run("db.connection.setSecret", json!({ "connection": project_id, "kind": "token", "value": "t" }));
    wait(&mut window, &token["operation"]);
    let project = window.session.state::<Workbench>().unwrap().project().unwrap().to_owned();
    let key = project_key(&project);
    assert!(memory.ids().iter().any(|id| id.account == format!("{key}/{project_id}/token")), "{:?}", memory.ids());
    let wrong_scope = window.refused(
        "db.connection.setSecret",
        json!({ "connection": c1, "kind": "token", "value": "t", "scope": "project" }),
    );
    assert!(wrong_scope.contains(c1), "{wrong_scope}");

    let empty = window.refused("db.connection.setSecret", json!({ "connection": c1, "kind": "ssh", "value": "" }));
    assert!(empty.contains("cannot be empty"), "{empty}");
    let slash =
        window.refused("db.connection.setSecret", json!({ "connection": "a/b", "kind": "ssh", "value": "secret-x" }));
    assert!(slash.contains("a/b") && !slash.contains("secret-x"), "{slash}");
    let ssh = zen_secrets::database::id(None, c2, zen_secrets::database::Kind::Ssh).unwrap();
    memory.deny(&ssh);
    let asked = window.run("db.connection.setSecret", json!({ "connection": c2, "kind": "ssh", "value": "secret-y" }));
    let denied = wait(&mut window, &asked["operation"]);
    assert_eq!((denied["status"].as_str(), denied["error"]["kind"].as_str()), (Some("failed"), Some("secret")));
    assert!(denied["message"].as_str().unwrap().contains("denied"), "{denied}");
    assert!(!denied.to_string().contains("secret-y"), "{denied}");
    super::connections::step(&mut window.session);
    let notices = window.run("ui.notifications.list", json!({})).to_string();
    assert!(notices.contains("denied") && !notices.contains("secret-y"), "{notices}");

    let password = zen_secrets::database::id(None, c2, zen_secrets::database::Kind::Password).unwrap();
    memory.deny(&password);
    let asked =
        window.run("db.connection.setSecret", json!({ "connection": c2, "kind": "password", "value": "secret-w" }));
    assert_eq!(window.run("db.dataSource.get", json!({ "id": c2 }))["hasPassword"], true, "marked at once");
    assert_eq!(wait(&mut window, &asked["operation"])["status"], "failed");
    super::connections::step(&mut window.session);
    assert_eq!(window.run("db.dataSource.get", json!({ "id": c2 }))["hasPassword"], false, "the mark is taken back");

    let off = window.run("db.dataSource.update", json!({ "id": c1, "savePassword": false }));
    assert_eq!(wait(&mut window, &off["secretOperation"])["kind"], "deleteSecrets");
    assert!(memory.get(&id).unwrap().is_none(), "turning Save password off deletes the saved one");
    assert_eq!(window.run("db.dataSource.get", json!({ "id": c1 }))["hasPassword"], false);
}

#[test]
fn a_saved_password_connects_without_being_passed() {
    let mut window = Window::new();
    let driver = with_drivers(&mut window);
    driver.server.require_password("kv-pw");
    let added = window.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "kv-pw" }));
    let id = added["id"].as_str().unwrap().to_owned();
    assert_eq!(added["hasPassword"], true);
    assert_eq!(wait(&mut window, &added["secretOperation"])["status"], "done");
    let file = std::fs::read_to_string(window.dir.path().join("data/databases/data-sources.json")).unwrap();
    assert!(file.contains("\"passwordSaved\": true") && !file.contains("kv-pw"), "{file}");
    let reads = window.secrets.accesses();
    let listed = window.run("db.dataSource.list", json!({}));
    assert_eq!(listed["connections"][0]["hasPassword"], true);
    assert_eq!(window.secrets.accesses(), reads, "listing never reads the store");
    let started = window.run("db.connection.connect", json!({ "connection": id }));
    let done = wait(&mut window, &started["operation"]);
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(window.secrets.accesses(), reads + 1);
    let removed = window.run("db.dataSource.remove", json!({ "id": id }));
    wait(&mut window, &removed["secretOperation"]);
    assert!(window.secrets.ids().is_empty(), "{:?}", window.secrets.ids());
}

#[test]
fn testing_settings_saves_nothing_and_reports_the_server_or_the_cause() {
    let mut window = Window::new();
    let driver = with_drivers(&mut window);
    driver.server.require_password("t0p");
    let started = window.run(
        "db.dataSource.test",
        json!({ "driver": "memory-kv", "name": "Cache", "password": "t0p", "settings": { "mode": "cluster" } }),
    );
    let done = wait(&mut window, &started["operation"]);
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(done["test"]["serverVersion"], "Memory 1.0");
    assert!(done["test"]["roundTripMs"].is_u64() && done["test"]["connectMs"].is_u64(), "{done}");
    assert_eq!(window.run("db.dataSource.list", json!({}))["total"], 0, "testing saves nothing");
    assert!(window.secrets.ids().is_empty(), "nor a password");
    assert!(!done.to_string().contains("t0p"));

    driver.server.set_down(true);
    let started = window.run("db.dataSource.test", json!({ "driver": "memory-kv", "password": "t0p" }));
    let refused = wait(&mut window, &started["operation"]);
    assert_eq!((refused["status"].as_str(), refused["cause"].as_str()), (Some("failed"), Some("refused")));
    assert!(refused["message"].as_str().unwrap().contains("nothing listens"), "{refused}");
    driver.server.set_down(false);

    let added = window.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "t0p" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let wrong = window.run("db.dataSource.test", json!({ "connection": id, "password": "nope" }));
    let wrong = wait(&mut window, &wrong["operation"]);
    assert_eq!(wrong["cause"], "authentication", "{wrong}");
    let saved = window.run("db.dataSource.test", json!({ "connection": id, "readOnly": true }));
    assert_eq!(wait(&mut window, &saved["operation"])["status"], "done", "the saved password is used");
    assert_eq!(window.run("db.dataSource.get", json!({ "id": id }))["readOnly"], false, "fields change only the test");

    let drivers = window.run("db.driver.list", json!({}));
    let ids: Vec<_> = drivers["drivers"].as_array().unwrap().iter().map(|driver| driver["id"].clone()).collect();
    assert_eq!(ids, [json!("postgresql"), json!("memory-kv")]);
    assert_eq!(drivers["drivers"][0]["defaultPort"], 5432);
    let redis = window.run("db.dataSource.add", json!({ "name": "R", "url": "memory://u:secret@cache:7000/3" }));
    assert_eq!((redis["driver"].as_str(), redis["port"].as_u64()), (Some("memory-kv"), Some(7000)));
    assert_eq!(redis["hasPassword"], true);
}

fn kv_connection(window: &mut Window) -> String {
    with_drivers(window);
    let added = window.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "pw" }));
    added["id"].as_str().unwrap().to_owned()
}

fn console_with(window: &mut Window, text: &str) -> PathBuf {
    let console = window.run("db.console.new", json!({ "text": text }));
    PathBuf::from(console["path"].as_str().unwrap())
}

fn caret(window: &mut Window, offset: usize) {
    window.run("editor.selection.set", json!({ "selections": [{ "head": offset }] }));
}

fn finished(window: &mut Window, started: &Value) -> Value {
    window.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }))
}

#[test]
fn the_statement_under_the_caret_and_its_neighbours() {
    let mut window = Window::new().with_connection();
    let text = "SELECT 1;\nSELECT 2;\n\nSELECT 3\n";
    let path = console_with(&mut window, text);
    let listed = window.run("db.dataSource.list", json!({}));
    window.run("db.dataSource.remove", json!({ "id": listed["connections"][0]["id"] }));
    caret(&mut window, 12);
    let at = window.run("db.console.statement", json!({}));
    assert_eq!(at["statement"]["text"], "SELECT 2");
    assert_eq!((at["statement"]["start"].as_u64(), at["statement"]["line"].as_u64()), (Some(10), Some(2)));
    assert_eq!(at["selection"], false);
    let next = window.run("db.console.nextStatement", json!({}));
    assert_eq!(next["statement"]["text"], "SELECT 3");
    let selection = window.run("editor.selection.get", json!({}));
    assert_eq!(selection[0]["head"], 21, "the caret moved to it: {selection}");
    let back = window.run("db.console.previousStatement", json!({ "path": path }));
    assert_eq!(back["statement"]["text"], "SELECT 2");
    window.run("editor.selection.set", json!({ "selections": [{ "anchor": 0, "head": 6 }] }));
    let selected = window.run("db.console.statement", json!({}));
    assert_eq!((selected["statement"]["text"].as_str(), selected["selection"].as_bool()), (Some("SELECT"), Some(true)));
    let split = window
        .run("db.script.split", json!({ "text": "SELECT 'a;b'; SELECT $$x;$$", "dialect": "postgres", "limit": 1 }));
    assert_eq!(split["total"], 2);
    assert_eq!(split["truncated"], true);
    assert_eq!(split["statements"][0]["text"], "SELECT 'a;b'");
    let console = window.run("db.script.split", json!({ "path": path }));
    assert_eq!(console["total"], 3);
    assert_eq!(console["dialect"], "standard", "a console without a connection splits as standard SQL");
    let state = window.run("db.console.state", json!({}));
    assert!(state["statement"].is_null(), "a selection is run as it is: {state}");
}

#[test]
fn a_script_connects_first_and_its_results_output_and_log_stay_with_the_console() {
    let mut window = Window::new();
    let id = kv_connection(&mut window);
    let path = console_with(&mut window, "SET a 1\nNOTICE hello there\nROWS 250\nGET a\n");
    let state = window.run("db.console.state", json!({}));
    assert_eq!(state["connection"], id.as_str(), "the only connection is the console's");
    assert_eq!((state["state"].as_str(), state["dialect"].as_str()), (Some("disconnected"), Some("lines")));
    let started = window.run("db.console.executeScript", json!({}));
    assert_eq!((started["statements"].as_u64(), started["connecting"].as_bool()), (Some(4), Some(true)));
    let done = finished(&mut window, &started);
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(done["updated"], 1);
    assert_eq!(done["results"][0]["rows"], 250);
    assert_eq!(done["results"][0]["page"].as_array().unwrap().len(), 50);
    assert_eq!(done["results"][0]["truncated"], true);
    let state = window.run("db.console.state", json!({ "path": path }));
    let titles: Vec<&str> =
        state["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Result 1", "Result 2"]);
    assert_eq!(state["panel"]["shown"], true);
    assert_eq!(state["panel"]["active"]["kind"], "result");
    assert_eq!(state["last"]["statements"], 4);
    assert_eq!(state["last"]["executed"], 4);
    assert_eq!(state["last"]["rows"], 251);
    assert_eq!(state["last"]["messages"], 1);
    let hints = state["hints"].as_array().unwrap();
    assert_eq!(hints.len(), 4);
    assert!(hints[2]["label"].as_str().unwrap().ends_with("250 rows"), "{hints:?}");
    assert_eq!(hints[0]["offset"], 7, "after the statement");
    let output = window.run("db.console.output", json!({}));
    assert_eq!(output["messages"][0]["text"], "hello there");
    assert_eq!(output["log"].as_array().unwrap().len(), 4);
    let listed = window.run("db.results.list", json!({ "path": path }));
    assert_eq!(listed["total"], 2);
    assert_eq!(listed["results"][0]["title"], "Result 1");
    assert_eq!(listed["results"][0]["columns"][4]["type"], "document");
    let result = listed["results"][0]["id"].clone();
    let page = window.run("db.result.page", json!({ "id": result, "offset": 248, "limit": 10 }));
    assert_eq!(page["page"].as_array().unwrap().len(), 2);
    assert_eq!(page["page"][1][0], json!({ "kind": "integer", "value": 249 }));
    assert_eq!(page["page"][1][3], json!({ "kind": "null" }));
    assert_eq!(page["truncated"], false);
    let copied =
        window.run("db.result.copy", json!({ "id": result, "rows": [0, 1], "columns": [0, 2], "header": true }));
    assert_eq!(copied["text"], "id\tname\teven\n0\trow 0\ttrue\n1\trow 1\tfalse");
    let status = window.run("db.connection.status", json!({ "connection": id }));
    let contexts = status["connections"][0]["contexts"].as_array().unwrap();
    assert!(contexts.iter().any(|context| context["context"]["kind"] == "console"), "{contexts:?}");
}

#[test]
fn a_run_hint_goes_after_the_statements_semicolon() {
    let mut window = Window::new();
    sql_connection(&mut window, "dev", false);
    let text = "SELECT 1 ;\nSELECT 2\n";
    console_with(&mut window, text);
    let started = window.run("db.console.executeScript", json!({}));
    assert_eq!(finished(&mut window, &started)["status"], "done");
    let state = window.run("db.console.state", json!({}));
    let offsets: Vec<u64> =
        state["hints"].as_array().unwrap().iter().map(|hint| hint["offset"].as_u64().unwrap()).collect();
    assert_eq!(offsets, [10, 19], "{state}");
}

#[test]
fn execute_runs_the_statement_at_the_caret_and_replaces_unpinned_tabs() {
    let mut window = Window::new();
    kv_connection(&mut window);
    console_with(&mut window, "ROWS 3\nGET a\nROWS 2\n");
    caret(&mut window, 2);
    let started = window.run("db.console.execute", json!({}));
    assert_eq!(started["statements"], 1);
    finished(&mut window, &started);
    let state = window.run("db.console.state", json!({}));
    assert_eq!(state["tabs"].as_array().unwrap().len(), 1);
    let first = state["tabs"][0]["result"].clone();
    window.run("db.result.pin", json!({ "id": first }));
    caret(&mut window, 9);
    let again = window.run("db.console.execute", json!({}));
    finished(&mut window, &again);
    let state = window.run("db.console.state", json!({}));
    assert_eq!(state["tabs"].as_array().unwrap().len(), 2, "the pinned tab stays");
    let unpinned = state["tabs"][1]["result"].clone();
    caret(&mut window, 16);
    let third = window.run("db.console.execute", json!({}));
    finished(&mut window, &third);
    let state = window.run("db.console.state", json!({}));
    assert_eq!(state["tabs"].as_array().unwrap().len(), 2, "the unpinned one was replaced");
    assert!(window.refused("db.result.page", json!({ "id": unpinned })).contains("no result"));
    let fourth = window.run("db.console.executeInNewTab", json!({}));
    finished(&mut window, &fourth);
    let state = window.run("db.console.state", json!({}));
    assert_eq!(state["tabs"].as_array().unwrap().len(), 3);
    window.run("db.result.rename", json!({ "id": first, "title": "Mine" }));
    window.run("db.result.close", json!({ "id": state["tabs"][2]["result"] }));
    let state = window.run("db.console.state", json!({}));
    assert_eq!(state["tabs"][0]["title"], "Mine");
    assert_eq!(state["tabs"].as_array().unwrap().len(), 2);
}

#[test]
fn each_run_numbers_its_result_tabs_from_one_and_pinned_tabs_keep_their_names() {
    let mut window = Window::new();
    let (id, _driver) = sql_connection(&mut window, "dev", true);
    let path = console_with(&mut window, "SELECT * FROM customers;\nSELECT * FROM customers;\nSELECT 1;\n");
    window.run("db.console.chooseConnection", json!({ "path": path, "connection": id }));
    let titles = |window: &mut Window| -> Vec<String> {
        let state = window.run("db.console.state", json!({ "path": path }));
        state["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap().to_owned()).collect()
    };
    let started = window.run("db.console.executeScript", json!({}));
    finished(&mut window, &started);
    let first = titles(&mut window);
    assert_eq!(first.len(), 3, "{first:?}");
    let table = first[0].clone();
    assert_eq!(first, [table.clone(), format!("{table} 2"), "Result 1".to_owned()]);

    let started = window.run("db.console.executeScript", json!({}));
    finished(&mut window, &started);
    assert_eq!(titles(&mut window), first, "the second run starts from 1 again");

    let state = window.run("db.console.state", json!({ "path": path }));
    window.run("db.result.pin", json!({ "id": state["tabs"][1]["result"] }));
    let started = window.run("db.console.executeScript", json!({}));
    finished(&mut window, &started);
    assert_eq!(titles(&mut window), [format!("{table} 2"), table.clone(), format!("{table} 2"), "Result 1".to_owned()]);
}

#[test]
fn an_error_is_marked_in_the_console_where_the_server_found_it() {
    let mut window = Window::new();
    kv_connection(&mut window);
    let path = console_with(&mut window, "SET a 1\nFAIL here\nSET b 2\n");
    let started = window.run("db.console.executeScript", json!({}));
    let done = finished(&mut window, &started);
    assert_eq!(done["status"], "failed");
    assert!(done["message"].as_str().unwrap().contains("syntax error"), "{done}");
    assert_eq!(done["statements"][2]["status"], "cancelled", "the script stopped");
    let problems = super::consoles::problems(&window.session, &path);
    assert_eq!(problems.len(), 1);
    assert_eq!((problems[0].start, problems[0].line, problems[0].column), (13, 2, 6));
    assert_eq!(problems[0].end, 17, "the word the server points at");
    let state = window.run("db.console.state", json!({}));
    assert_eq!(state["last"]["errorLine"], 2);
    assert_eq!(state["panel"]["active"]["kind"], "log");
    assert_eq!(state["hints"][1]["error"], true);
    caret(&mut window, 0);
    window.run("editor.edit.insert", json!({ "text": "-- note\n" }));
    let moved = super::consoles::problems(&window.session, &path);
    assert_eq!(moved[0].start, 21, "marks move with the edits");
}

#[test]
fn fetch_all_asks_first_and_rows_past_the_cap_are_read_again() {
    let mut window = Window::new();
    kv_connection(&mut window);
    let settings = window.run("db.settings.update", json!({ "page": 10, "rowCap": 20, "blankLines": "smart" }));
    assert_eq!(settings["paging"]["rowCap"], 20);
    let file = window.dir.path().join("data/databases/settings.json");
    assert!(std::fs::read_to_string(&file).unwrap().contains("smart"));
    console_with(&mut window, "ROWS 45");
    let started = window.run("db.console.execute", json!({}));
    let done = finished(&mut window, &started);
    let id = done["results"][0]["id"].clone();
    assert_eq!((done["results"][0]["rows"].as_u64(), done["results"][0]["more"].as_bool()), (Some(20), Some(true)));
    assert_eq!(done["results"][0]["stoppedBecause"], "cap");
    let refused = window.registry.dispatch(&mut window.session, "db.result.fetchAll", json!({ "id": id })).unwrap_err();
    assert!(format!("{refused:?}").contains("confirmFetchAll"), "{refused:?}");
    let next = window.run("db.result.fetchNext", json!({ "id": id }));
    wait(&mut window, &next["operation"]);
    assert_eq!(window.run("db.result.page", json!({ "id": id }))["rows"], 30);
    let all = window.run("db.result.fetchAll", json!({ "id": id, "confirmed": true }));
    wait(&mut window, &all["operation"]);
    let page = window.run("db.result.page", json!({ "id": id, "offset": 40 }));
    assert_eq!((page["rows"].as_u64(), page["more"].as_bool()), (Some(45), Some(false)));
    window.run("db.result.close", json!({ "id": id }));
    assert_eq!(window.run("db.results.list", json!({}))["bytes"], 0, "closing gives the memory back");
}

#[test]
fn agents_run_sql_and_read_it_page_by_page() {
    let mut window = Window::new();
    let id = kv_connection(&mut window);
    let answer = window.run(
        "db.query.run",
        json!({ "connection": id, "sql": "SET k v\nROWS 5\nGET k", "waitMs": 10_000, "limit": 2 }),
    );
    assert_eq!(answer["status"], "done", "{answer}");
    assert_eq!(answer["statements"].as_array().unwrap().len(), 3);
    assert_eq!(answer["results"][0]["page"].as_array().unwrap().len(), 2);
    assert_eq!(answer["results"][0]["truncated"], true);
    assert_eq!(answer["results"][1]["page"][0][3], json!({ "kind": "text", "value": "v" }));
    let listed = window.run("db.results.list", json!({ "query": answer["id"] }));
    assert_eq!(listed["total"], 2);
    let started = window.run("db.query.run", json!({ "connection": id, "sql": "SLEEP 5000" }));
    assert_eq!(started["status"], "running");
    let cancelled = window.run("db.query.cancel", json!({ "query": started["id"] }));
    assert_eq!(cancelled["cancelled"], true);
    let stopped = window.run("db.query.state", json!({ "query": started["id"], "waitMs": 5000 }));
    assert_eq!(stopped["status"], "cancelled", "{stopped}");
    let refused = window
        .registry
        .dispatch(&mut window.session, "db.query.run", json!({ "connection": id, "sql": "  " }))
        .unwrap_err();
    assert!(format!("{refused:?}").contains("noStatement"));
}

#[test]
fn the_chosen_connection_and_schema_stay_with_the_console_tab() {
    let mut window = Window::new();
    let id = kv_connection(&mut window);
    window.run("db.dataSource.add", json!({ "name": "Other", "driver": "memory-kv" }));
    console_with(&mut window, "PING");
    let state = window.run("db.console.state", json!({}));
    assert!(state["connection"].is_null(), "two connections: none is chosen by itself");
    let refused = window.registry.dispatch(&mut window.session, "db.console.execute", json!({})).unwrap_err();
    assert!(format!("{refused:?}").contains("noConnection"));
    window.run("db.console.chooseConnection", json!({ "connection": id }));
    let bound = window.run("db.console.chooseSchema", json!({ "schema": "public" }));
    assert_eq!((bound["connection"].as_str(), bound["schema"].as_str()), (Some(id.as_str()), Some("public")));
    let saved = super::super::workspaces::saved(&mut window.session);
    let tab = saved[0].tabs.iter().find(|tab| tab.path.is_some()).unwrap();
    assert_eq!(tab.data["connection"], id.as_str());
    assert_eq!(tab.data["schema"], "public");
    let toggled = window.run("db.console.toggleResults", json!({}));
    assert_eq!(toggled["shown"], true);
    let maximized = window.run("db.console.maximizeResults", json!({}));
    assert_eq!(maximized["maximized"], true);
    let shown = window.run("db.console.showPanelTab", json!({ "tab": { "kind": "output" } }));
    assert_eq!(shown["active"]["kind"], "output");
    let cancel = window.run("db.operation.cancel", json!({}));
    assert!(cancel["query"].is_null());
}

fn shop_snapshot(connection: &str) -> zen_db::schema::Snapshot {
    use zen_db::schema::{Catalog, Column, Level, Object, ObjectKind, Schema, Snapshot};
    let mut orders = Object::new(ObjectKind::Table, "orders");
    orders.detailed = true;
    orders.size = Some(16384);
    orders.columns = vec![
        Column { name: "id".into(), position: 1, type_name: "bigint".into(), ..Column::default() },
        Column { name: "total".into(), position: 2, type_name: "numeric".into(), nullable: true, ..Column::default() },
    ];
    let mut customers = Object::new(ObjectKind::Table, "customers");
    customers.detailed = true;
    customers.size = Some(8192);
    let mut big = Object::new(ObjectKind::View, "big_orders");
    big.detailed = true;
    big.definition = Some(" SELECT id FROM orders;".into());
    Snapshot {
        server_version: Some("PostgreSQL 18.0".into()),
        taken: 1,
        catalogs: vec![Catalog {
            name: "shop".into(),
            current: true,
            schemas_read: true,
            schemas: vec![Schema {
                name: "public".into(),
                level: Level::Details,
                objects: vec![customers, orders, big],
                ..Schema::default()
            }],
            ..Catalog::default()
        }],
        ..Snapshot::new(connection, "postgresql")
    }
}

fn step(window: &mut Window) {
    super::connections::step(&mut window.session);
}

#[test]
fn a_snapshot_on_disk_answers_before_connecting_and_the_navigator_shows_it() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "url": "postgresql://ann@db.local:6543/shop", "folder": "Work", "type": "prod" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &shop_snapshot(&id)).unwrap();

    let tables = window.run("db.schema.children", json!({ "connection": id, "path": ["shop", "public", "tables"] }));
    let labels: Vec<&str> =
        tables["nodes"].as_array().unwrap().iter().map(|node| node["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["customers", "orders"]);
    assert_eq!((tables["loading"].as_bool(), tables["total"].as_u64()), (Some(false), Some(2)));
    let limited =
        window.run("db.schema.children", json!({ "connection": id, "path": ["shop", "public", "tables"], "limit": 1 }));
    assert_eq!(limited["truncated"], true);
    let status = window.run("db.connection.status", json!({ "connection": id }));
    assert_eq!(status["connections"][0]["state"], "disconnected", "reading the snapshot connects nowhere");

    let orders = json!(["shop", "public", "tables", "orders"]);
    let object = window.run("db.object.get", json!({ "connection": id, "path": orders, "limit": 1 }));
    assert_eq!(object["qualified"], "\"public\".\"orders\"");
    assert_eq!(object["object"]["columns"].as_array().unwrap().len(), 1);
    assert_eq!(object["truncated"], true);
    let ddl = window.run("db.object.ddl", json!({ "connection": id, "path": orders }));
    assert_eq!(
        ddl["ddl"],
        "CREATE TABLE \"public\".\"orders\" (\n    \"id\" bigint NOT NULL,\n    \"total\" numeric\n);\n"
    );
    let name = window.run(
        "db.object.copyName",
        json!({ "connection": id, "path": ["shop", "public", "tables", "orders", "columns", "id"], "qualified": true }),
    );
    assert_eq!(name["name"], "\"public\".\"orders\".\"id\"");
    let opened = window.run("db.object.open", json!({ "connection": id, "path": orders }));
    assert_eq!((opened["kind"].as_str(), opened["page"].as_str()), (Some("table"), Some("data")));
    assert!(opened.get("query").is_none(), "a disconnected connection is not read: {opened}");
    let state = window.run("db.object.state", json!({}));
    assert_eq!(state["data"]["statement"], "SELECT * FROM \"public\".\"orders\"");
    assert_eq!(state["state"], "disconnected");
    let unknown =
        window.refused("db.object.get", json!({ "connection": id, "path": ["shop", "public", "tables", "nope"] }));
    assert!(unknown.contains("nope"), "{unknown}");

    let state = window.run("db.navigator.state", json!({}));
    let rows: Vec<(&str, &str)> = state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["kind"].as_str().unwrap(), row["label"].as_str().unwrap()))
        .collect();
    assert_eq!(rows, [("connectionFolder", "Work"), ("connection", "Shop")]);
    assert_eq!(state["rows"][1]["detail"], "db.local:6543");
    assert_eq!(state["rows"][1]["color"], "red", "Production's colour by name");
    window.run("db.navigator.select", json!({ "connection": id, "path": ["shop", "public", "tables", "orders"] }));
    let state = window.run("db.navigator.state", json!({}));
    let rows: Vec<(usize, &str)> = state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["depth"].as_u64().unwrap() as usize, row["label"].as_str().unwrap()))
        .collect();
    assert_eq!(
        rows,
        [
            (0, "Work"),
            (1, "Shop"),
            (2, "shop"),
            (3, "public"),
            (4, "Tables"),
            (5, "customers"),
            (5, "orders"),
            (4, "Foreign Tables"),
            (4, "Views"),
            (4, "Materialized Views"),
            (4, "Indexes"),
            (4, "Functions"),
            (4, "Sequences"),
            (4, "Aggregate Functions"),
            (3, "Extensions"),
            (3, "Event Triggers"),
            (3, "Tablespaces"),
            (3, "Roles")
        ]
    );
    let orders_row = &state["rows"][6];
    assert_eq!((orders_row["selected"].as_bool(), orders_row["sizeRatio"].as_f64()), (Some(true), Some(1.0)));
    assert_eq!(state["rows"][5]["sizeRatio"], 0.5);
    window.run("db.navigator.expand", json!({ "connection": id, "path": ["shop", "public", "tables", "orders"] }));
    window.run(
        "db.navigator.expand",
        json!({ "connection": id, "path": ["shop", "public", "tables", "orders", "columns"] }),
    );
    let filtered = window.run("db.navigator.filter", json!({ "text": "TOT" }));
    let rows: Vec<&str> =
        filtered["rows"].as_array().unwrap().iter().map(|row| row["label"].as_str().unwrap()).collect();
    assert_eq!(rows, ["Work", "Shop", "shop", "public", "Tables", "orders", "Columns", "total"]);
    window.run("db.navigator.filter", json!({ "text": "" }));
    let options = window.run("db.navigator.options", json!({ "view": "simple", "showHost": false }));
    assert_eq!((options["view"].as_str(), options["showHost"].as_bool()), (Some("simple"), Some(false)));
    let collapsed = window.run("db.navigator.collapse", json!({ "all": true }));
    assert!(collapsed["collapsed"].as_u64().unwrap() >= 4);
    step(&mut window);
    let kept = std::fs::read_to_string(
        window
            .dir
            .path()
            .join("data/databases")
            .join(project_key(window.session.state::<Workbench>().unwrap().project().unwrap()))
            .join("navigator.json"),
    )
    .unwrap();
    assert!(kept.contains("\"simple\"") && kept.contains("orders"), "{kept}");
    assert!(!kept.contains("ann@"), "no connection details in the navigator's state: {kept}");

    let refreshed = window.run("db.navigator.refresh", json!({ "connection": id }));
    assert_eq!(refreshed["started"], false, "a disconnected connection is not read, and nothing connects");
}

#[test]
fn a_key_value_connection_supplies_its_own_tree_read_in_the_background_and_kept() {
    let mut window = Window::new();
    let driver = with_drivers(&mut window);
    let added = window.run("db.dataSource.add", json!({ "name": "Cache", "driver": "memory-kv", "auth": "none" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let expanded = window.run("db.navigator.expand", json!({ "connection": id }));
    assert_eq!(wait(&mut window, &expanded["connecting"])["status"], "done");
    let engine = window.session.state::<super::Database>().unwrap().started().unwrap().clone();
    for key in ["user:1", "user:2", "session:9"] {
        let op = engine
            .execute(
                &id,
                zen_db::driver::ContextKind::Main,
                &format!("SET {key} x"),
                zen_db::driver::ExecOptions::default(),
            )
            .unwrap();
        engine.wait(op, std::time::Duration::from_secs(5));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let labels = loop {
        step(&mut window);
        let state = window.run("db.navigator.state", json!({}));
        let rows: Vec<String> =
            state["rows"].as_array().unwrap().iter().map(|row| row["label"].as_str().unwrap().to_owned()).collect();
        if !rows.iter().any(|label| label == "Loading…") && rows.len() > 1 || std::time::Instant::now() > deadline {
            break rows;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(labels, ["Cache", "session", "user"]);
    window.run("db.navigator.expand", json!({ "connection": id, "path": ["user"] }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let users = loop {
        step(&mut window);
        let users = window.run("db.schema.children", json!({ "connection": id, "path": ["user"] }));
        if users["loading"] == false || std::time::Instant::now() > deadline {
            break users;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let keys: Vec<&str> =
        users["nodes"].as_array().unwrap().iter().map(|node| node["label"].as_str().unwrap()).collect();
    assert_eq!(keys, ["1", "2"]);
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    wait(&mut window, &closing["operation"]);
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while zen_db::schema::read_snapshot(&file).is_none_or(|snapshot| snapshot.cached(&["user".to_owned()]).is_none()) {
        assert!(std::time::Instant::now() < deadline, "the tree is kept on disk");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let offline = window.run("db.schema.children", json!({ "connection": id, "path": ["user"] }));
    assert_eq!(offline["total"], 2, "kept for when it is disconnected: {offline}");
    let _ = driver;
}

fn tree_of(keys: &[&str]) -> zen_db::schema::Snapshot {
    let mut snapshot = zen_db::schema::Snapshot::new("kv", "memory-kv");
    for key in keys {
        snapshot.keep(&[(*key).to_owned()], Vec::new());
    }
    snapshot
}

#[test]
fn an_earlier_save_of_a_tree_never_replaces_a_later_one() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("kv.snapshot");
    let saver = super::structure::Saver::default();
    let mut jobs = Vec::new();
    for keys in [&["session"][..], &["session", "user"][..]] {
        saver.save(tree_of(keys), &file, |job| {
            jobs.push(job);
            true
        });
    }
    for job in jobs.into_iter().rev() {
        job();
    }
    assert_eq!(zen_db::schema::read_snapshot(&file), Some(tree_of(&["session", "user"])));
}

#[test]
fn saves_of_a_tree_share_one_writer_until_it_ends_or_fails_to_start() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("kv.snapshot");
    let saver = super::structure::Saver::default();
    let mut started = 0;
    let mut waiting = Vec::new();
    for keys in [&["a"][..], &["a", "b"][..], &["a", "b", "c"][..]] {
        saver.save(tree_of(keys), &file, |job| {
            started += 1;
            waiting.push(job);
            true
        });
    }
    assert_eq!(started, 1, "a second writer would share the first one's temporary file");
    waiting.remove(0)();
    assert_eq!(zen_db::schema::read_snapshot(&file), Some(tree_of(&["a", "b", "c"])));
    saver.save(tree_of(&["d"]), &file, |_| false);
    saver.save(tree_of(&["e"]), &file, |job| {
        job();
        true
    });
    assert_eq!(
        zen_db::schema::read_snapshot(&file),
        Some(tree_of(&["e"])),
        "a writer that never started blocks nothing"
    );
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_postgresql_connection_is_read_on_connect_and_again_after_ddl() {
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
    let connected = window.run("db.navigator.expand", json!({ "connection": id }));
    assert_eq!(wait(&mut window, &connected["connecting"])["status"], "done");
    let engine = window.session.state::<super::Database>().unwrap().started().unwrap().clone();
    let run = |sql: &str| {
        let op = engine
            .execute(
                &id,
                zen_db::driver::ContextKind::Console("console".into()),
                sql,
                zen_db::driver::ExecOptions::default(),
            )
            .unwrap();
        let state = engine.wait(op, std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(state.status, zen_db::engine::OperationStatus::Done, "{state:?}");
    };
    run("DROP SCHEMA IF EXISTS nav_ws CASCADE");
    run("CREATE SCHEMA nav_ws");
    run("CREATE TABLE nav_ws.items (id integer PRIMARY KEY)");
    let until = |window: &mut Window, path: Value, wanted: &str| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            step(window);
            let answer = window.run("db.schema.children", json!({ "connection": id, "path": path }));
            let found = answer["nodes"].as_array().unwrap().iter().any(|node| node["label"] == wanted);
            if found {
                return answer;
            }
            assert!(std::time::Instant::now() < deadline, "{wanted} in {answer}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    until(&mut window, json!(["postgres", "nav_ws", "tables"]), "items");
    run("ALTER TABLE nav_ws.items ADD COLUMN label text");
    until(&mut window, json!(["postgres", "nav_ws", "tables", "items", "columns"]), "label");
    let ddl =
        window.run("db.object.ddl", json!({ "connection": id, "path": ["postgres", "nav_ws", "tables", "items"] }));
    assert!(ddl["ddl"].as_str().unwrap().contains("label text"), "{ddl}");
    let query = window.run(
        "db.query.run",
        json!({ "connection": id, "sql": "ALTER TABLE nav_ws.items ADD COLUMN note text", "waitMs": 10_000 }),
    );
    assert_eq!(query["status"], "done", "{query}");
    until(&mut window, json!(["postgres", "nav_ws", "tables", "items", "columns"]), "note");
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    let snapshot = zen_db::schema::read_snapshot(&file).unwrap();
    assert!(snapshot.schema("postgres", "nav_ws").is_some());
    assert!(!std::fs::read(&file).unwrap().windows(3).any(|bytes| bytes == b"zen"), "no password in the snapshot");
    run("DROP SCHEMA nav_ws CASCADE");
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    wait(&mut window, &closing["operation"]);
}

fn object_snapshot(connection: &str, driver: &str) -> zen_db::schema::Snapshot {
    use zen_db::schema::{
        Constraint, ConstraintKind, Index, Object, ObjectKind, Partitioning, Reference, Routine, SequenceInfo,
    };
    let mut snapshot = shop_snapshot(connection);
    snapshot.driver = driver.to_owned();
    let schema = &mut snapshot.catalogs[0].schemas[0];
    schema.owner = Some("ann".into());
    let customers = schema.objects.iter_mut().find(|object| object.name == "customers").unwrap();
    customers.owner = Some("ann".into());
    customers.comment = Some("Who buys".into());
    customers.tablespace = Some("fast".into());
    customers.rows = Some(25);
    customers.acl = vec!["ann=arwdDxtm/ann".into(), "bob=r*/ann".into()];
    customers.columns = vec![
        zen_db::schema::Column { name: "id".into(), position: 1, type_name: "bigint".into(), ..Default::default() },
        zen_db::schema::Column {
            name: "name".into(),
            position: 2,
            type_name: "text".into(),
            nullable: true,
            comment: Some("Full name".into()),
            ..Default::default()
        },
    ];
    customers.constraints = vec![Constraint {
        name: "customers_pkey".into(),
        kind: ConstraintKind::PrimaryKey,
        columns: vec!["id".into()],
        definition: "PRIMARY KEY (id)".into(),
        references: None,
        index: Some("customers_pkey".into()),
        comment: None,
    }];
    customers.indexes = vec![Index {
        name: "customers_pkey".into(),
        columns: vec!["id".into()],
        unique: true,
        primary: true,
        method: "btree".into(),
        size: Some(16384),
        definition: "CREATE UNIQUE INDEX customers_pkey ON public.customers USING btree (id)".into(),
        constraint: Some("customers_pkey".into()),
        comment: None,
    }];
    customers.partitioning = Some(Partitioning { key: Some("RANGE (id)".into()), ..Partitioning::default() });
    let orders = schema.objects.iter_mut().find(|object| object.name == "orders").unwrap();
    orders.constraints = vec![Constraint {
        name: "orders_customer_fkey".into(),
        kind: ConstraintKind::ForeignKey,
        columns: vec!["total".into()],
        definition: "FOREIGN KEY (total) REFERENCES customers(id)".into(),
        references: Some(Reference {
            schema: "public".into(),
            table: "customers".into(),
            columns: vec!["id".into()],
            on_update: "no action".into(),
            on_delete: "cascade".into(),
        }),
        index: None,
        comment: None,
    }];
    let mut part = Object::new(ObjectKind::Table, "customers_1");
    part.detailed = true;
    part.partitioning = Some(Partitioning {
        parent: Some("public.customers".into()),
        bound: Some("FOR VALUES FROM (1) TO (100)".into()),
        ..Partitioning::default()
    });
    let mut count = Object::new(ObjectKind::Function, "order_count");
    count.detailed = true;
    count.routine = Some(Routine {
        arguments: "c integer".into(),
        result: Some("bigint".into()),
        language: "sql".into(),
        kind: "function".into(),
        definition: Some(
            "CREATE FUNCTION public.order_count(c integer) RETURNS bigint LANGUAGE sql AS $$ SELECT 1 $$".into(),
        ),
    });
    let mut sequence = Object::new(ObjectKind::Sequence, "orders_id_seq");
    sequence.detailed = true;
    sequence.sequence = Some(SequenceInfo {
        data_type: "bigint".into(),
        start: "1".into(),
        increment: "1".into(),
        cycle: false,
        ..SequenceInfo::default()
    });
    schema.objects.extend([part, count, sequence]);
    snapshot
}

#[test]
fn an_object_opens_as_a_tab_with_its_pages_and_is_kept_without_data() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let added = window
        .run("db.dataSource.add", json!({ "name": "Shop", "url": "postgresql://ann@db.local/shop", "type": "prod" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &object_snapshot(&id, "postgresql")).unwrap();
    let customers = json!(["shop", "public", "tables", "customers"]);

    let opened = window.run("db.object.open", json!({ "connection": id, "path": customers }));
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    assert_eq!(tab, format!("object:{id}:shop/public/tables/customers"));
    assert_eq!((opened["tab"]["title"].as_str(), opened["page"].as_str()), (Some("customers"), Some("data")));
    let again = window.run("db.object.open", json!({ "connection": id, "path": customers }));
    assert_eq!(again["tab"]["id"], tab.as_str(), "the same object comes to front");
    assert_eq!(window.titles(), ["Overview", "customers"]);
    let state = window.run("db.object.state", json!({ "tab": tab }));
    assert_eq!(state["pages"], json!(["properties", "data", "ddl", "diagram"]));
    assert_eq!(state["data"]["statement"], "SELECT * FROM \"public\".\"customers\"", "in the database's order");
    assert_eq!((state["color"].as_str(), state["qualified"].as_str()), (Some("red"), Some("\"public\".\"customers\"")));

    let properties = window.run("db.object.properties", json!({}));
    let header: Vec<(String, String)> = properties["header"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| (field["label"].as_str().unwrap().to_owned(), field["value"].as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(
        header,
        [("Name", "customers"), ("Comment", "Who buys"), ("Owner", "ann"), ("Tablespace", "fast")]
            .map(|(label, value)| (label.to_owned(), value.to_owned()))
    );
    let sections: Vec<&str> =
        properties["sections"].as_array().unwrap().iter().map(|section| section["id"].as_str().unwrap()).collect();
    assert_eq!(
        sections,
        [
            "columns",
            "constraints",
            "foreignKeys",
            "indexes",
            "triggers",
            "references",
            "dependencies",
            "rules",
            "policies",
            "partitions",
            "statistics",
            "permissions"
        ]
    );
    assert_eq!(properties["section"], "columns");
    assert_eq!(properties["rows"][1], json!(["name", "text", "", "", "", "Full name"]), "the grid numbers the rows");
    assert_eq!(properties["rows"][0][2], "yes");
    let counted = properties["sections"].as_array().unwrap().iter().filter(|section| section["count"].is_u64()).count();
    assert_eq!(counted, 12, "every section is counted: {}", properties["sections"]);
    let snapshot = object_snapshot(&id, "sqlite");
    let path: Vec<String> = ["shop", "public", "tables", "customers"].map(str::to_owned).to_vec();
    let sqlite = super::objects::properties(&snapshot, &id, &path, None, 10).unwrap();
    let labels: Vec<&str> = sqlite.header.iter().map(|field| field.label.as_str()).collect();
    assert_eq!(labels, ["Name", "Comment"], "SQLite has no owners or tablespaces");
    let shown: Vec<&str> = sqlite.sections.iter().map(|section| section.id.as_str()).collect();
    assert_eq!(
        shown,
        ["columns", "constraints", "foreignKeys", "indexes", "triggers", "references", "statistics"],
        "SQLite has no policies, partitions or grants"
    );
    let references = window.run("db.object.properties", json!({ "section": "references" }));
    assert_eq!(references["rows"], json!([["public", "orders", "orders_customer_fkey"]]));
    let partitions = window.run("db.object.properties", json!({ "section": "partitions" }));
    assert_eq!(partitions["note"], "Partitioned by RANGE (id)");
    assert_eq!(partitions["rows"], json!([["public.customers_1", "FOR VALUES FROM (1) TO (100)"]]));
    let permissions = window.run("db.object.properties", json!({ "section": "permissions", "limit": 1 }));
    assert_eq!((permissions["total"].as_u64(), permissions["truncated"].as_bool()), (Some(2), Some(true)));
    let permissions = window.run("db.object.properties", json!({ "section": "permissions" }));
    assert_eq!(permissions["rows"][1], json!(["bob", "SELECT*", "ann"]));
    let statistics = window.run("db.object.properties", json!({ "section": "statistics" }));
    assert!(statistics["rows"].as_array().unwrap().contains(&json!(["Total size", "8 KB"])), "{statistics}");

    window.run("db.object.showSection", json!({ "section": "indexes" }));
    let refused = window.refused("db.object.showSection", json!({ "section": "source" }));
    assert!(refused.contains("columns, constraints"), "{refused}");
    let state = window.run("db.object.state", json!({}));
    assert_eq!((state["page"].as_str(), state["section"].as_str()), (Some("properties"), Some("indexes")));
    let options = window.run("db.object.ddlOptions", json!({ "comments": true }));
    assert_eq!(options["options"]["comments"], true);
    window.run("db.object.showPage", json!({ "page": "ddl" }));
    let ddl = window.run("db.object.ddl", json!({ "connection": id, "path": customers, "comments": true }));
    assert!(ddl["ddl"].as_str().unwrap().contains("COMMENT ON TABLE \"public\".\"customers\" IS 'Who buys';"), "{ddl}");

    let schema = window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public"] }));
    assert_eq!((schema["kind"].as_str(), schema["page"].as_str()), (Some("schema"), Some("properties")));
    let objects = window.run("db.object.properties", json!({}));
    assert_eq!(objects["section"], "objects");
    assert!(objects["rows"].as_array().unwrap().iter().any(|row| row[0] == "order_count(c integer)"), "{objects}");
    let schema_ddl = window.run("db.object.ddl", json!({ "connection": id, "path": ["shop", "public"] }));
    assert_eq!(schema_ddl["ddl"], "CREATE SCHEMA \"public\" AUTHORIZATION \"ann\";\n");
    let function = window.run(
        "db.object.open",
        json!({ "connection": id, "path": ["shop", "public", "functions", "order_count(c integer)"] }),
    );
    assert_eq!((function["kind"].as_str(), function["page"].as_str()), (Some("function"), Some("properties")));
    let source = window.run("db.object.properties", json!({}));
    assert!(source["text"].as_str().unwrap().starts_with("CREATE FUNCTION public.order_count"), "{source}");
    assert!(source["header"].as_array().unwrap().contains(&json!({ "label": "Returns", "value": "bigint" })));
    let refused = window.refused("db.object.showPage", json!({ "page": "data" }));
    assert!(refused.contains("data"), "{refused}");
    let sequence = window.run(
        "db.object.open",
        json!({ "connection": id, "path": ["shop", "public", "sequences", "orders_id_seq"], "page": "properties" }),
    );
    assert_eq!(sequence["kind"], "sequence");
    let settings = window.run("db.object.properties", json!({}));
    assert!(settings["rows"].as_array().unwrap().contains(&json!(["Data type", "bigint"])), "{settings}");

    let saved = super::super::workspaces::saved(&mut window.session);
    let kept = serde_json::to_string(&saved).unwrap();
    assert!(kept.contains("\"page\":\"properties\"") && kept.contains("\"section\":\"indexes\""), "{kept}");
    assert!(kept.contains("\"comments\":true"), "the DDL toggles are kept: {kept}");
    let status = window.run("db.connection.status", json!({ "connection": id }));
    assert_eq!(status["connections"][0]["state"], "disconnected", "opening objects connects nowhere");
}

pub(super) fn until_result(window: &mut Window, tab: &str) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let state = window.run("db.object.state", json!({ "tab": tab }));
        if state["data"]["result"].is_u64() && !state["data"]["reading"].as_bool().unwrap_or(true) {
            return state;
        }
        assert!(state["data"]["error"].is_null(), "{state}");
        assert!(std::time::Instant::now() < deadline, "the rows were read: {state}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn a_table_reads_its_rows_ordered_by_its_key_and_its_tab_frees_them() {
    let mut window = Window::new();
    let driver = with_drivers(&mut window);
    let added = window.run("db.dataSource.add", json!({ "name": "KV", "driver": "memory-kv", "password": "pw" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &object_snapshot(&id, "memory-kv")).unwrap();
    let opened =
        window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    let read = window.run("db.object.readData", json!({}));
    assert_eq!(
        (read["connecting"].as_bool(), read["statement"].as_str()),
        (Some(true), Some("SELECT * FROM \"public\".\"customers\""))
    );
    let state = until_result(&mut window, &tab);
    let result = state["data"]["result"].as_u64().unwrap();
    let page = window.run("db.result.page", json!({ "id": result, "limit": 2 }));
    assert_eq!(page["rows"], 25);
    assert_eq!(page["page"][0][1], json!({ "kind": "text", "value": "Customer 1" }));
    assert!(driver.server.executed().iter().any(|sql| sql == "SELECT * FROM \"public\".\"customers\""));
    assert!(
        driver.server.opened().iter().any(|opened| opened.context == zen_db::driver::ContextKind::Main),
        "read in the Main context"
    );

    let refreshed = window.run("db.result.refresh", json!({ "id": result }));
    assert_eq!(refreshed["tab"], tab.as_str());
    let state = until_result(&mut window, &tab);
    let again = state["data"]["result"].as_u64().unwrap();
    assert_ne!(again, result);
    let listed = window.run("db.results.list", json!({}));
    let ids: Vec<u64> =
        listed["results"].as_array().unwrap().iter().map(|result| result["id"].as_u64().unwrap()).collect();
    assert!(!ids.contains(&result) && ids.contains(&again), "the old rows are freed: {ids:?}");

    window.run("workspace.tab.close", json!({ "workspace": WORKSPACE, "tab": tab }));
    super::objects::step(&mut window.session);
    let listed = window.run("db.results.list", json!({}));
    assert_eq!(listed["total"], 0, "closing the tab frees its rows: {listed}");
    window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
}

#[test]
fn the_value_panel_reads_json_pictures_arrays_and_times_and_saves_them() {
    let mut window = Window::new();
    let id = kv_connection(&mut window);
    let query = window.run("db.query.run", json!({ "connection": id, "sql": "SELECT", "waitMs": 10_000 }));
    let result = query["results"][0]["id"].as_u64().unwrap();
    let value = |window: &mut Window, column: usize, extra: Value| {
        let mut params = json!({ "id": result, "row": 0, "column": column });
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        window.run("db.result.value", params)
    };
    let json = value(&mut window, 2, json!({}));
    assert_eq!(
        (json["format"].as_str(), json["kind"].as_str(), json["type"].as_str()),
        (Some("json"), Some("json"), Some("jsonb"))
    );
    assert!(
        json["text"].as_str().unwrap().starts_with("{\n  \"id\": 1,\n  \"tier\": \"gold\",\n  \"address\": {\n"),
        "{json}"
    );
    let picture = value(&mut window, 3, json!({}));
    assert_eq!(picture["formats"], json!(["image", "hex"]));
    assert_eq!(picture["picture"], json!({ "format": "PNG", "bytes": 73 }));
    let hex = value(&mut window, 3, json!({ "format": "hex", "limit": 80 }));
    assert!(hex["text"].as_str().unwrap().starts_with("00000000  89 50 4e 47"), "{hex}");
    assert_eq!(hex["truncated"], true);
    let tags = value(&mut window, 4, json!({}));
    assert_eq!((tags["format"].as_str(), tags["text"].as_str()), (Some("text"), Some(r#"{new,"vip client"}"#)));
    let created = value(&mut window, 5, json!({}));
    assert_eq!(created["text"], "2026-10-01 09:30:00+08", "as the server sent it, with its zone");
    let cut = value(&mut window, 2, json!({ "limit": 10 }));
    assert_eq!((cut["text"].as_str(), cut["truncated"].as_bool()), (Some("{\n  \"id\": "), Some(true)));
    let missing = window.refused("db.result.value", json!({ "id": result, "row": 99, "column": 0 }));
    assert!(missing.contains("25 rows"), "{missing}");

    let calc = window.run("db.result.calc", json!({ "id": result, "columns": [0, 0] }));
    assert_eq!(
        (calc["count"].as_u64(), calc["sum"].as_str(), calc["avg"].as_str()),
        (Some(25), Some("325"), Some("13"))
    );
    assert_eq!(
        (calc["min"].as_str(), calc["max"].as_str(), calc["distinct"].as_u64()),
        (Some("1"), Some("25"), Some(25))
    );
    let names = window.run("db.result.calc", json!({ "id": result, "rows": [0, 2], "columns": [1, 1] }));
    assert_eq!((names["count"].as_u64(), names.get("sum")), (Some(3), None));

    let file = window.dir.path().join("picture.png");
    let saved = window.run("db.result.saveValue", json!({ "id": result, "row": 0, "column": 3, "path": file }));
    assert_eq!(saved["bytes"], 73);
    assert_eq!(std::fs::read(&file).unwrap(), zen_db::memory::PICTURE);
    let exists = window.refused("db.result.saveValue", json!({ "id": result, "row": 0, "column": 2, "path": file }));
    assert!(exists.contains("overwrite"), "{exists}");
    assert_eq!(std::fs::read(&file).unwrap(), zen_db::memory::PICTURE, "nothing was replaced");
    window.run(
        "db.result.saveValue",
        json!({ "id": result, "row": 0, "column": 2, "path": file, "overwrite": true, "format": "json" }),
    );
    assert!(std::fs::read_to_string(&file).unwrap().contains("\n  \"tier\": \"gold\","));

    assert_eq!(window.run("db.result.toggleValuePanel", json!({}))["shown"], false, "open at first, so hidden");
    assert_eq!(window.run("db.result.toggleValuePanel", json!({}))["shown"], true);
    window.run("db.settings.update", json!({ "valueFormat": "text" }));
    assert_eq!(value(&mut window, 2, json!({}))["format"], "text", "the panel's format when the value offers it");
    assert_eq!(value(&mut window, 0, json!({}))["format"], "text");
    let settings = window.run("db.settings.update", json!({ "valueFormat": null }));
    assert_eq!((settings["valuePanel"].as_bool(), settings.get("valueFormat")), (Some(true), None));
    assert_eq!(window.run("db.result.toggleValuePanel", json!({ "shown": false }))["shown"], false);
}

#[test]
fn spatial_values_read_as_geometry_and_their_shapes_are_listed() {
    let mut window = Window::new();
    let id = kv_connection(&mut window);
    let query = window.run("db.query.run", json!({ "connection": id, "sql": "PLACES", "waitMs": 10_000 }));
    let result = query["results"][0]["id"].as_u64().unwrap();
    let area = window.run("db.result.value", json!({ "id": result, "row": 0, "column": 2 }));
    assert_eq!((area["format"].as_str(), area["formats"][0].as_str()), (Some("geometry"), Some("geometry")));
    assert_eq!(area["text"], "SRID=4326;POLYGON ((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))");
    assert_eq!(
        area["geometry"],
        json!({
            "kind": "POLYGON", "srid": 4326, "dimensions": "xy", "encoding": "ewkb", "points": 10, "parts": 2,
            "empty": false, "bounds": { "minX": 0.0, "minY": 0.0, "maxX": 4.0, "maxY": 4.0 }
        })
    );
    let hex = window.run("db.result.value", json!({ "id": result, "row": 0, "column": 2, "format": "text" }));
    assert!(hex["text"].as_str().unwrap().starts_with("\\x0103000020e6100000"), "{hex}");
    let name = window.run("db.result.value", json!({ "id": result, "row": 0, "column": 1 }));
    assert!(name.get("geometry").is_none(), "{name}");

    let shapes = window.run("db.result.geometry", json!({ "id": result }));
    let found: Vec<(u64, u64, &str)> = shapes["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|shape| {
            (shape["row"].as_u64().unwrap(), shape["column"].as_u64().unwrap(), shape["kind"].as_str().unwrap())
        })
        .collect();
    assert_eq!(found, [(0, 2, "POLYGON"), (0, 3, "LINESTRING"), (1, 2, "POINT")]);
    assert_eq!(shapes["bounds"], json!({ "minX": 0.0, "minY": 0.0, "maxX": 6.0, "maxY": 6.0 }));
    assert_eq!((shapes["cells"].as_u64(), shapes["truncated"].as_bool()), (Some(12), Some(false)));
    assert_eq!(shapes["shapes"][2]["wkt"], "SRID=4326;POINT (5.5 2)");
    let few = window.run("db.result.geometry", json!({ "id": result, "columns": [2, 2], "limit": 1, "chars": 12 }));
    assert_eq!((few["shapes"].as_array().unwrap().len(), few["truncated"].as_bool()), (1, Some(true)));
    assert_eq!(
        (few["shapes"][0]["wkt"].as_str(), few["shapes"][0]["truncated"].as_bool()),
        (Some("SRID=4326;PO"), Some(true))
    );
    let bare = window.run("db.result.geometry", json!({ "id": result, "rows": [1, 1], "chars": 0 }));
    assert!(bare["shapes"][0].get("wkt").is_none(), "{bare}");

    let file = window.dir.path().join("area.wkt");
    window
        .run("db.result.saveValue", json!({ "id": result, "row": 0, "column": 2, "path": file, "format": "geometry" }));
    assert!(std::fs::read_to_string(&file).unwrap().starts_with("SRID=4326;POLYGON (("));
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_postgresql_table_opens_on_its_rows_in_key_order_and_its_values_read_in_the_panel() {
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
    let setup = window.run_confirmed(
        "db.query.run",
        json!({
            "connection": id,
            "waitMs": 20_000,
            "sql": "DROP SCHEMA IF EXISTS obj_ws CASCADE;\nCREATE SCHEMA obj_ws;\n\
CREATE TABLE obj_ws.items (id integer PRIMARY KEY, doc jsonb, pic bytea, tags text[], at timestamptz);\n\
COMMENT ON TABLE obj_ws.items IS 'Things';\n\
INSERT INTO obj_ws.items VALUES (3, '{\"b\": [1, 2], \"a\": {\"c\": null}}', '\\x89504e470d0a1a0a', '{x,\"y z\"}', '2026-10-26 12:00:00+00'), (1, NULL, NULL, NULL, NULL), (2, '[]', '\\x00', '{}', NULL);",
        }),
    );
    assert_eq!(setup["status"], "done", "{setup}");
    window.run("db.schema.refresh", json!({ "connection": id, "path": ["postgres", "obj_ws"], "force": true }));
    let items = json!(["postgres", "obj_ws", "tables", "items"]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        step(&mut window);
        let children =
            window.run("db.schema.children", json!({ "connection": id, "path": ["postgres", "obj_ws", "tables"] }));
        if children["nodes"].as_array().unwrap().iter().any(|node| node["label"] == "items") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{children}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let opened = window.run("db.object.open", json!({ "connection": id, "path": items }));
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    assert!(opened["query"].is_u64(), "a connected connection reads at once: {opened}");
    let state = until_result(&mut window, &tab);
    assert_eq!(state["data"]["statement"], "SELECT * FROM obj_ws.items");
    let result = state["data"]["result"].as_u64().unwrap();
    let page = window.run("db.result.page", json!({ "id": result }));
    let ids: Vec<i64> = page["page"].as_array().unwrap().iter().map(|row| row[0]["value"].as_i64().unwrap()).collect();
    assert_eq!(ids, [1, 2, 3], "in key order");
    let doc = window.run("db.result.value", json!({ "id": result, "row": 2, "column": 1 }));
    assert_eq!(doc["format"], "json");
    assert!(doc["text"].as_str().unwrap().contains("\"a\": {\n    \"c\": null\n  }"), "{doc}");
    let pic = window.run("db.result.value", json!({ "id": result, "row": 2, "column": 2 }));
    assert_eq!((pic["picture"]["format"].as_str(), pic["formats"][0].as_str()), (Some("PNG"), Some("image")));
    let tags = window.run("db.result.value", json!({ "id": result, "row": 2, "column": 3 }));
    assert_eq!(tags["text"], r#"{x,"y z"}"#);
    let at = window.run("db.result.value", json!({ "id": result, "row": 2, "column": 4 }));
    assert!(at["text"].as_str().unwrap().contains("2026-10-26") && at["type"] == "timestamptz", "{at}");
    let calc = window.run("db.result.calc", json!({ "id": result, "columns": [0, 0] }));
    assert_eq!((calc["sum"].as_str(), calc["max"].as_str()), (Some("6"), Some("3")));
    let properties = window.run("db.object.properties", json!({ "tab": tab }));
    assert!(properties["header"].as_array().unwrap().contains(&json!({ "label": "Comment", "value": "Things" })));
    assert_eq!(properties["rows"][1][1], "doc");
    let status = window.run("db.connection.status", json!({ "connection": id }));
    assert!(
        status["connections"][0]["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|context| context["context"]["kind"] == "main"),
        "{status}"
    );
    window.run_confirmed(
        "db.query.run",
        json!({ "connection": id, "sql": "DROP SCHEMA obj_ws CASCADE", "waitMs": 10_000 }),
    );
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    wait(&mut window, &closing["operation"]);
}

pub(super) fn sql_snapshot(connection: &str, keyed: bool) -> zen_db::schema::Snapshot {
    use zen_db::schema::{Catalog, Column, Constraint, ConstraintKind, Level, Object, ObjectKind, Schema, Snapshot};
    let mut customers = Object::new(ObjectKind::Table, "customers");
    customers.oid = zen_db::memory::TABLE;
    customers.detailed = true;
    let types = ["bigint", "text", "jsonb", "bytea", "text[]", "timestamp with time zone"];
    customers.columns = ["id", "name", "payload", "picture", "tags", "created"]
        .iter()
        .zip(types)
        .enumerate()
        .map(|(at, (name, type_name))| Column {
            name: (*name).into(),
            position: i32::try_from(at + 1).unwrap(),
            type_name: type_name.into(),
            nullable: at > 0,
            ..Column::default()
        })
        .collect();
    if keyed {
        customers.constraints = vec![Constraint {
            name: "customers_pkey".into(),
            kind: ConstraintKind::PrimaryKey,
            columns: vec!["id".into()],
            definition: "PRIMARY KEY (id)".into(),
            references: None,
            index: None,
            comment: None,
        }];
    }
    Snapshot {
        taken: 1,
        catalogs: vec![Catalog {
            name: "shop".into(),
            current: true,
            schemas_read: true,
            schemas: vec![Schema {
                name: "public".into(),
                level: Level::Details,
                objects: vec![customers],
                ..Schema::default()
            }],
            ..Catalog::default()
        }],
        ..Snapshot::new(connection, zen_db::memory::SQL)
    }
}

pub(super) fn sql_connection(
    window: &mut Window,
    kind: &str,
    keyed: bool,
) -> (String, std::sync::Arc<zen_db::memory::MemoryDriver>) {
    let driver = std::sync::Arc::new(zen_db::memory::MemoryDriver::sql());
    let drivers: Vec<std::sync::Arc<dyn zen_db::driver::Driver>> = vec![std::sync::Arc::clone(&driver) as _];
    window.session.state_mut::<super::Database>().set_drivers(drivers);
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": format!("Shop {kind}"), "driver": zen_db::memory::SQL, "password": "pw", "type": kind }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &sql_snapshot(&id, keyed)).unwrap();
    (id, driver)
}

pub(super) fn open_customers(window: &mut Window, id: &str) -> (String, u64) {
    let opened =
        window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    window.run("db.object.readData", json!({ "tab": tab }));
    let state = until_result(window, &tab);
    (tab, state["data"]["result"].as_u64().unwrap())
}

fn until_done(window: &mut Window, tab: &str, old: u64) -> u64 {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let state = until_result(window, tab);
        let result = state["data"]["result"].as_u64().unwrap();
        if result != old {
            return result;
        }
        assert!(std::time::Instant::now() < deadline, "the rows were read again: {state}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn until_started(window: &mut Window, query: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let state = window.run("db.query.state", json!({ "query": query }));
        if state["statements"][0]["status"] == "running" {
            assert!(state["statements"][0]["result"].is_null(), "no result before its columns: {state}");
            return;
        }
        assert!(std::time::Instant::now() < deadline, "the statement started: {state}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_re_read_shows_the_old_rows_until_its_columns_come_and_nothing_is_staged_meanwhile() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let (tab, first) = open_customers(&mut window, &id);
    driver.server.hold(true);
    let read = window.run("db.object.readData", json!({ "tab": tab }));
    let query = read["query"].as_u64().unwrap();
    until_started(&mut window, query);

    let state = window.run("db.object.state", json!({ "tab": tab }));
    assert_eq!(state["data"]["result"].as_u64(), Some(first), "the old rows stay until the new columns come: {state}");
    assert_eq!(state["data"]["reading"], true);
    let filter = window.run("db.result.filterState", json!({}));
    assert_eq!((filter["id"].as_u64(), filter["query"].as_u64()), (Some(first), Some(query)), "{filter}");
    let refused = window.refused("db.result.setCell", json!({ "row": 0, "column": 1, "text": "Ann Smith" }));
    assert!(refused.contains("being read again"), "edits would be lost with the old rows: {refused}");
    let refused = window.refused("db.result.addRow", json!({}));
    assert!(refused.contains("being read again"), "{refused}");

    driver.server.hold(false);
    let second = until_done(&mut window, &tab, first);
    let filter = window.run("db.result.filterState", json!({}));
    assert!(filter["query"].is_null(), "{filter}");
    window.run("db.result.setCell", json!({ "row": 0, "column": 1, "text": "Ann Smith" }));
    let refused = window.refused("db.object.readData", json!({ "tab": tab }));
    assert!(refused.contains("not saved"), "staged edits are never dropped: {refused}");
    let refused = window.refused("db.result.refresh", json!({ "id": second }));
    assert!(refused.contains("not saved"), "{refused}");
    window.run("db.result.revert", json!({}));
    window.run("db.object.readData", json!({ "tab": tab }));
    until_done(&mut window, &tab, second);
}

fn staged_question(window: &mut Window, command: &str, params: Value) -> (Value, Value) {
    let error = window.registry.dispatch(&mut window.session, command, params).unwrap_err();
    let zen_core::DispatchError::Failed { error, .. } = error else { panic!("{error}") };
    assert_eq!(error.code.as_deref(), Some(super::edits::PENDING_CHANGES), "{}", error.message);
    let data = error.data.unwrap();
    (data["results"].clone(), data["retry"].clone())
}

#[test]
fn staged_edits_are_never_dropped_by_a_new_run_or_a_closed_tab() {
    let mut window = Window::new();
    let (id, _driver) = sql_connection(&mut window, "dev", true);
    let (opened_tab, shown) = open_customers(&mut window, &id);
    let path = console_with(&mut window, "SELECT * FROM customers\n");
    let console_tab = window.state()["active"].as_str().unwrap().to_owned();
    let started = window.run("db.console.execute", json!({}));
    finished(&mut window, &started);
    let first = window.run("db.console.state", json!({ "path": path }))["tabs"][0]["result"].as_u64().unwrap();
    window.run("db.result.setCell", json!({ "id": first, "row": 0, "column": 1, "text": "Ann" }));

    let (results, retry) = staged_question(&mut window, "db.console.execute", json!({}));
    assert_eq!(results, json!([first]));
    assert_eq!(retry, json!({ "command": "db.console.execute", "params": { "path": path } }));
    let (_, retry) = staged_question(&mut window, "db.console.executeScript", json!({}));
    assert_eq!(retry["command"], "db.console.executeScript");
    let (_, retry) = staged_question(&mut window, "db.result.refresh", json!({ "id": first }));
    assert_eq!(retry, json!({ "command": "db.result.refresh", "params": { "id": first } }));
    let (_, retry) = staged_question(&mut window, "db.result.close", json!({ "id": first }));
    assert_eq!(retry, json!({ "command": "db.result.close", "params": { "id": first } }));
    let close = json!({ "workspace": WORKSPACE, "tab": console_tab });
    let (_, retry) = staged_question(&mut window, "workspace.tab.close", close.clone());
    assert_eq!(retry, json!({ "command": "workspace.tab.close", "params": close }));
    let (_, retry) = staged_question(&mut window, "db.console.close", json!({ "path": path }));
    assert_eq!(retry["command"], "db.console.close");
    let state = window.run("db.result.editState", json!({ "id": first }));
    assert_eq!(state["dirty"], true, "everything refused kept the changes: {state}");
    assert_eq!(window.run("db.console.state", json!({ "path": path }))["tabs"][0]["result"].as_u64(), Some(first));

    window.run("db.result.pin", json!({ "id": first }));
    let started = window.run("db.console.execute", json!({}));
    finished(&mut window, &started);
    let tabs = window.run("db.console.state", json!({ "path": path }))["tabs"].clone();
    assert_eq!(tabs.as_array().unwrap().len(), 2, "a pinned tab is never replaced, so nothing asks: {tabs}");
    let second = tabs[1]["result"].as_u64().unwrap();
    window.run("db.result.close", json!({ "id": second }));

    window.run("db.result.setCell", json!({ "id": shown, "row": 0, "column": 1, "text": "Bob" }));
    let close = json!({ "workspace": WORKSPACE, "tab": opened_tab });
    let (results, _) = staged_question(&mut window, "workspace.tab.close", close.clone());
    assert_eq!(results, json!([shown]));
    window.run("db.result.revert", json!({ "id": shown }));
    window.run("workspace.tab.close", close);

    window.run("db.result.revert", json!({ "id": first }));
    window.run("db.result.close", json!({ "id": first }));
    window.run("workspace.tab.close", json!({ "workspace": WORKSPACE, "tab": console_tab }));
    assert_eq!(window.titles(), ["Overview"]);
}

#[test]
fn removing_a_connection_closes_its_tabs_and_leaves_its_consoles_unbound() {
    let mut window = Window::new();
    let (id, _driver) = sql_connection(&mut window, "dev", true);
    window.run("db.dataSource.add", json!({ "name": "Other", "driver": zen_db::memory::SQL, "password": "pw" }));
    let (_, shown) = open_customers(&mut window, &id);
    let console = window.run("db.console.new", json!({ "connection": id, "text": "SELECT * FROM customers\n" }));
    let path = console["path"].as_str().unwrap().to_owned();
    let started = window.run("db.console.execute", json!({}));
    finished(&mut window, &started);
    assert_eq!(window.titles().len(), 3, "{:?}", window.titles());

    let preview = window.run("db.dataSource.removal", json!({ "id": id }));
    assert_eq!((preview["tabs"].as_u64(), preview["consoles"].as_u64()), (Some(2), Some(1)), "{preview}");
    assert_eq!(
        preview["message"],
        "Remove the connection “Shop dev”? 2 tabs close. 1 console stays, without a connection."
    );

    window.run("db.result.setCell", json!({ "id": shown, "row": 0, "column": 1, "text": "Ann" }));
    let (results, retry) = staged_question(&mut window, "db.dataSource.remove", json!({ "id": id }));
    assert_eq!(results, json!([shown]));
    assert_eq!(retry, json!({ "command": "db.dataSource.remove", "params": { "id": id } }));
    assert_eq!(window.titles().len(), 3, "a refusal closes nothing");
    window.run("db.result.revert", json!({ "id": shown }));

    let removed = window.run("db.dataSource.remove", json!({ "id": id }));
    assert_eq!(removed["closedTabs"], 2);
    assert_eq!(removed["unboundConsoles"], json!([path]));
    assert_eq!(window.titles().len(), 2, "the object tab closed, the console stayed: {:?}", window.titles());
    let state = window.run("db.console.state", json!({ "path": path }));
    assert!(state["connection"].is_null(), "not bound to the other connection: {state}");
    assert_eq!(state["removedConnection"], "Shop dev");
    assert_eq!(state["tabs"], json!([]));
    let listed = window.run("db.results.list", json!({}));
    assert_eq!(listed["total"], 0, "the rows are freed: {listed}");
}

#[test]
fn a_table_is_filtered_and_sorted_and_keeps_its_filter_history() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let (tab, first) = open_customers(&mut window, &id);
    let state = window.run("db.result.filterState", json!({}));
    assert_eq!((state["id"].as_u64(), state["filterable"].as_bool()), (Some(first), Some(true)));

    let refused = window.refused("db.result.filter", json!({ "condition": "1 = 1; DROP TABLE customers" }));
    assert!(refused.contains("one condition"), "{refused}");
    let state = window.run("db.result.filter", json!({ "condition": "name LIKE 'Customer 1%'" }));
    assert_eq!(state["applied"], "SELECT * FROM \"public\".\"customers\" WHERE (name LIKE 'Customer 1%')");
    assert_eq!(state["composition"], "table");
    assert!(state["query"].is_u64(), "the tab's read is the filter's query while it runs: {state}");
    let second = until_done(&mut window, &tab, first);
    assert!(window.refused("db.result.page", json!({ "id": first })).contains("no result"), "the old rows are freed");
    let page = window.run("db.result.page", json!({ "id": second, "limit": 1 }));
    assert_eq!(page["rows"], 11, "Customer 1 and Customer 10 to 19: {page}");
    assert!(driver.server.executed().iter().any(|sql| sql.contains("WHERE (name LIKE 'Customer 1%')")));
    let state = window.run("db.result.filterState", json!({}));
    assert!(state["query"].is_null(), "the rows came: {state}");
    assert_eq!(state["history"], json!(["name LIKE 'Customer 1%'"]));
    assert_eq!(state["filteredColumns"], json!([1]));
    let kept = std::fs::read_to_string(window.dir.path().join("data/databases/filters.json")).unwrap();
    assert!(kept.contains("Customer 1%"), "history is kept per table in the data folder: {kept}");

    let sorted = window.run("db.result.sort", json!({ "column": 1, "direction": "desc" }));
    assert_eq!(sorted["clientSort"], true, "every row is loaded: sorted in memory");
    let page = window.run("db.result.page", json!({ "id": second, "limit": 2 }));
    assert_eq!(page["page"][0][1], json!({ "kind": "text", "value": "Customer 19" }));

    let by_value = window.run("db.result.filterByValue", json!({ "column": 0, "row": 0, "operator": "notEqual" }));
    assert_eq!(by_value["condition"], "name LIKE 'Customer 1%' AND \"id\" <> 19");
    assert!(by_value["applied"].as_str().unwrap().ends_with("ORDER BY \"name\" DESC"), "{by_value}");
    let third = until_done(&mut window, &tab, second);
    let reset = window.run("db.result.resetFilter", json!({ "column": 0 }));
    assert_eq!(reset["condition"], "name LIKE 'Customer 1%'");
    let fourth = until_done(&mut window, &tab, third);
    let back = window.run("db.result.filterBack", json!({}));
    assert_eq!(back["condition"], "name LIKE 'Customer 1%' AND \"id\" <> 19");
    assert_eq!(back["canGoForward"], true);
    let fifth = until_done(&mut window, &tab, fourth);
    let forward = window.run("db.result.filterForward", json!({}));
    assert_eq!(forward["condition"], "name LIKE 'Customer 1%'");
    until_done(&mut window, &tab, fifth);

    let saved = window.run("db.filter.save", json!({ "name": "Ones" }));
    assert_eq!(saved["saved"], json!([{ "name": "Ones", "condition": "name LIKE 'Customer 1%'" }]));
    let deleted = window.run("db.filter.delete", json!({ "name": "Ones" }));
    assert_eq!(deleted["saved"], json!([]));
    let completed = window.run("db.result.completeFilter", json!({ "text": "id > 1 AND na", "caret": 13 }));
    assert_eq!((completed["start"].as_u64(), completed["items"][0]["insert"].as_str()), (Some(11), Some("name")));
    let found = window.run("db.result.find", json!({ "text": "customer 1", "limit": 3 }));
    assert_eq!(found["cells"].as_array().unwrap().len(), 3);
    let values = window.run("db.result.distinctValues", json!({ "column": 1, "limit": 2 }));
    assert_eq!((values["truncated"].as_bool(), values["values"][0]["count"].as_u64()), (Some(true), Some(1)));
    let cleared = window.run("db.result.resetFilter", json!({}));
    assert_eq!(cleared["applied"], "SELECT * FROM \"public\".\"customers\"");
}

#[test]
fn resetting_a_filter_or_exporting_never_runs_a_change_again() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let changed = "UPDATE customers SET name = 'x' WHERE id > 0 RETURNING *";
    console_with(&mut window, &format!("{changed}\n"));
    let started = window.run("db.console.execute", json!({}));
    finished(&mut window, &started);
    let runs = || driver.server.executed().iter().filter(|sql| sql.starts_with("UPDATE customers")).count();
    assert_eq!(runs(), 1);

    let (code, _) = refusal(&mut window, "db.result.resetFilter", json!({}));
    assert_eq!(code.as_deref(), Some("invalidFilter"));
    let (code, _) =
        refusal(&mut window, "db.export.start", json!({ "source": { "kind": "result" }, "clipboard": true }));
    assert_eq!(code.as_deref(), Some("exportChanges"));
    let (code, _) =
        refusal(&mut window, "db.export.start", json!({ "source": { "kind": "console" }, "clipboard": true }));
    assert_eq!(code.as_deref(), Some("exportChanges"));
    let query =
        json!({ "source": { "kind": "query", "connection": id, "sql": "DELETE FROM orders" }, "clipboard": true });
    let refused = window.refused("db.export.start", query);
    assert!(refused.contains("change data"), "{refused}");
    assert_eq!(runs(), 1, "{:?}", driver.server.executed());
    assert!(!driver.server.executed().iter().any(|sql| sql.starts_with("DELETE")));

    let error = zen_db::driver::DbError::new(zen_db::driver::ErrorKind::Unsupported, "fetch")
        .with_code(zen_db::results::NOT_REPEATABLE);
    assert!(super::connections::describe(&error).contains("again"), "{}", super::connections::describe(&error));
}

#[test]
fn a_console_result_is_filtered_by_merging_into_its_select_and_its_tab_shows_the_new_rows() {
    let mut window = Window::new();
    let (_, driver) = sql_connection(&mut window, "dev", true);
    let path = console_with(&mut window, "SELECT * FROM customers WHERE id > 0\n");
    let started = window.run("db.console.execute", json!({}));
    finished(&mut window, &started);
    let state = window.run("db.console.state", json!({ "path": path }));
    let first = state["tabs"][0]["result"].as_u64().unwrap();
    driver.server.hold(true);
    let filtered = window.run("db.result.filter", json!({ "condition": "id < 5" }));
    assert_eq!(filtered["applied"], "SELECT * FROM customers WHERE (id > 0) AND (id < 5)");
    assert_eq!(filtered["composition"], "merged");
    until_started(&mut window, filtered["query"].as_u64().unwrap());
    let state = window.run("db.console.state", json!({ "path": path }));
    assert_eq!(state["tabs"][0]["result"].as_u64(), Some(first), "the old rows stay until the new columns come");
    let refused = window.refused("db.result.setCell", json!({ "id": first, "row": 0, "column": 1, "text": "Ann" }));
    assert!(refused.contains("being read again"), "{refused}");
    driver.server.hold(false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let second = loop {
        let state = window.run("db.console.state", json!({ "path": path }));
        let shown = state["tabs"][0]["result"].as_u64().unwrap();
        if shown != first {
            break shown;
        }
        assert!(std::time::Instant::now() < deadline, "{state}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let state = window.run("db.result.filterState", json!({ "id": second }));
    assert_eq!(state["condition"], "id < 5", "the filter moves with the tab to the new rows");
    assert!(driver.server.executed().iter().any(|sql| sql == "SELECT * FROM customers WHERE (id > 0) AND (id < 5)"));
    let sorted = window.run("db.result.sort", json!({ "id": second, "column": 0, "direction": "desc" }));
    assert_eq!(sorted["clientSort"], true);
}

#[test]
fn edits_are_staged_previewed_and_saved_in_one_transaction() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let (_, result) = open_customers(&mut window, &id);
    let state = window.run("db.result.editState", json!({}));
    assert_eq!(state["editable"], true, "{state}");
    assert_eq!(state["key"]["kind"], "primaryKey");
    assert_eq!(state["table"], "\"public\".\"customers\"");

    let invalid = window.refused("db.result.setCell", json!({ "row": 0, "column": 0, "text": "x" }));
    assert!(invalid.contains("whole number"), "{invalid}");
    let not_null = window.refused("db.result.setCell", json!({ "row": 0, "column": 0, "null": true }));
    assert!(not_null.contains("NULL"), "{not_null}");
    window.run("db.result.setCell", json!({ "row": 0, "column": 1, "text": "Ann" }));
    let added = window.run("db.result.addRow", json!({}));
    assert_eq!(added["row"], 25);
    window.run("db.result.setCell", json!({ "row": 25, "column": 1, "text": "New" }));
    window.run("db.result.duplicateRow", json!({ "row": 2 }));
    let state = window.run("db.result.deleteRows", json!({ "rows": [1, 26] }));
    assert_eq!(state["pending"], json!({ "cells": 2, "updated": 1, "inserted": 1, "deleted": 1 }));
    assert_eq!(state["dirty"], true);

    let preview = window.run("db.result.previewChanges", json!({}));
    let sql = preview["sql"].as_str().unwrap();
    assert!(
        sql.starts_with(
            "DELETE FROM \"public\".\"customers\" WHERE \"id\" = 2;\nINSERT INTO \"public\".\"customers\" (\"name\") VALUES ('New')"
        ),
        "{sql}"
    );
    assert!(
        sql.contains(
            "UPDATE \"public\".\"customers\" SET \"name\" = 'Ann' WHERE \"id\" = 1 AND \"name\" IS NOT DISTINCT FROM 'Customer 1'"
        ),
        "{sql}"
    );
    assert_eq!(preview["statements"][0]["sql"], "DELETE FROM \"public\".\"customers\" WHERE \"id\" = $1");
    assert!(preview["statements"][0].get("params").is_none(), "values of parameters are never answered");

    let generated = window.run("db.result.generateSql", json!({ "kind": "delete", "rows": [3, 4] }));
    assert_eq!(
        generated["sql"],
        "DELETE FROM \"public\".\"customers\" WHERE \"id\" = 4;\nDELETE FROM \"public\".\"customers\" WHERE \"id\" = 5;"
    );

    let started = window.run("db.result.save", json!({}));
    assert_eq!(started["statements"], 3);
    wait(&mut window, &started["operation"]);
    let state = window.run("db.result.editState", json!({}));
    assert_eq!(state["lastSave"]["status"], "done", "{state}");
    assert_eq!(state["dirty"], false);
    let page = window.run("db.result.page", json!({ "id": result, "limit": 1 }));
    assert_eq!(page["page"][0][1], json!({ "kind": "text", "value": "Ann" }));
    assert_eq!(page["rows"], 25, "a row deleted, a row inserted");
    let executed = driver.server.executed();
    let at = executed.iter().rposition(|sql| sql == "BEGIN").unwrap();
    assert_eq!(executed.last().map(String::as_str), Some("COMMIT"));
    assert_eq!(executed.len() - at, 5);
    let bound = driver.server.bound();
    assert_eq!(bound.last().unwrap().1, [Some("Ann".to_owned()), Some("1".to_owned()), Some("Customer 1".to_owned())]);

    driver.server.set_conflict(true);
    window.run("db.result.setCell", json!({ "row": 0, "column": 1, "text": "Bob" }));
    let started = window.run("db.result.save", json!({}));
    wait(&mut window, &started["operation"]);
    let state = window.run("db.result.editState", json!({}));
    assert_eq!(state["lastSave"]["status"], "failed");
    assert!(state["lastSave"]["message"].as_str().unwrap().contains("Row 1 was changed or deleted"), "{state}");
    assert_eq!(state["dirty"], true, "a failed save keeps the changes");
    assert_eq!(driver.server.executed().last().map(String::as_str), Some("ROLLBACK"));
    let state = window.run("db.result.revert", json!({}));
    assert_eq!(state["dirty"], false);
    driver.server.set_conflict(false);
}

#[test]
fn nothing_staged_is_reverted_or_restored_while_its_save_runs() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let (_, result) = open_customers(&mut window, &id);
    window.run("db.result.setCell", json!({ "row": 0, "column": 1, "text": "Ann" }));
    window.run("db.result.deleteRows", json!({ "rows": [1] }));
    driver.server.hold(true);
    let started = window.run("db.result.save", json!({}));
    for (command, params) in [
        ("db.result.revertCell", json!({ "id": result, "row": 0, "column": 1 })),
        ("db.result.deleteRows", json!({ "id": result, "rows": [1], "restore": true })),
        ("db.result.revert", json!({ "id": result })),
    ] {
        let (code, _) = refusal(&mut window, command, params);
        assert_eq!(code.as_deref(), Some("saving"), "{command}");
    }
    let state = window.run("db.result.editState", json!({ "id": result }));
    assert_eq!(state["pending"]["updated"], 1, "{state}");
    assert_eq!(state["pending"]["deleted"], 1, "{state}");
    driver.server.hold(false);
    wait(&mut window, &started["operation"]);
    let state = window.run("db.result.editState", json!({ "id": result }));
    assert_eq!(
        (state["lastSave"]["status"].as_str(), state["dirty"].as_bool()),
        (Some("done"), Some(false)),
        "{state}"
    );
}

#[test]
fn the_grids_edit_state_reads_its_files_again_only_when_they_change() {
    let mut window = Window::new();
    let (id, _) = sql_connection(&mut window, "dev", true);
    let (_, result) = open_customers(&mut window, &id);
    assert_eq!(window.run("db.result.editState", json!({ "id": result }))["editable"], true);
    let file = window.dir.path().join("data/databases/data-sources.json");
    let unchanged = |path: &std::path::Path| {
        let modified = std::fs::metadata(path).unwrap().modified().unwrap();
        let length = std::fs::metadata(path).unwrap().len() as usize;
        std::fs::write(path, " ".repeat(length)).unwrap();
        std::fs::File::options().write(true).open(path).unwrap().set_modified(modified).unwrap();
    };
    let text = std::fs::read_to_string(&file).unwrap();
    unchanged(&file);
    let state = window.run("db.result.editState", json!({ "id": result }));
    assert_eq!(state["editable"], true, "an unchanged file is not read again: {state}");
    std::fs::write(&file, text.replace("\"readOnly\": false", "\"readOnly\": true")).unwrap();
    let state = window.run("db.result.editState", json!({ "id": result }));
    assert_eq!((state["editable"].as_bool(), state["reason"].as_str()), (Some(false), Some("connection")), "{state}");
}

#[test]
fn a_result_edits_the_table_of_its_own_database_when_another_has_the_same_id() {
    let mut window = Window::new();
    let (id, _) = sql_connection(&mut window, "dev", true);
    let mut snapshot = sql_snapshot(&id, true);
    let mut other = sql_snapshot(&id, false).catalogs.remove(0);
    other.name = "archive".into();
    other.current = false;
    snapshot.catalogs.insert(0, other);
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &snapshot).unwrap();
    let (_, result) = open_customers(&mut window, &id);
    let state = window.run("db.result.editState", json!({ "id": result }));
    assert_eq!(
        (state["editable"].as_bool(), state["key"]["kind"].as_str()),
        (Some(true), Some("primaryKey")),
        "{state}"
    );
}

#[test]
fn production_confirms_after_the_preview_and_a_table_without_a_key_takes_a_chosen_one() {
    let mut window = Window::new();
    let (id, _) = sql_connection(&mut window, "prod", false);
    let (_, _) = open_customers(&mut window, &id);
    let state = window.run("db.result.editState", json!({}));
    assert_eq!((state["editable"].as_bool(), state["reason"].as_str()), (Some(false), Some("noKey")));
    assert!(state["message"].as_str().unwrap().contains("no key"), "{state}");
    let refused = window.refused("db.result.setCell", json!({ "row": 0, "column": 1, "text": "x" }));
    assert!(refused.contains("no key"), "{refused}");
    let bad = window.refused("db.result.setRowKey", json!({ "key": { "kind": "columns", "columns": ["nope"] } }));
    assert!(bad.contains("id, name"), "{bad}");
    let state =
        window.run("db.result.setRowKey", json!({ "key": { "kind": "columns", "columns": ["id"] }, "remember": true }));
    assert_eq!(state["key"]["kind"], "virtual");
    let kept = window.dir.path().join("project/.zen/databases/virtual-keys.json");
    assert!(std::fs::read_to_string(kept).unwrap().contains("public.customers"));
    assert_eq!(state["confirm"], true, "Production confirms data changes");
    window.run("db.result.setCell", json!({ "row": 0, "column": 1, "text": "x" }));
    let error = window.registry.dispatch(&mut window.session, "db.result.save", json!({})).unwrap_err();
    assert!(format!("{error:?}").contains("confirmDataChange"), "{error:?}");
    let started = window.run("db.result.save", json!({ "confirmed": true }));
    let done = wait(&mut window, &started["operation"]);
    assert_eq!(done["status"], "done", "{done}");
    let state = window.run("db.result.editState", json!({}));
    assert_eq!(
        state["lastSave"]["report"]["inTransaction"], true,
        "Production's manual commit keeps the transaction open"
    );
}

fn refusal(window: &mut Window, command: &str, params: Value) -> (Option<String>, Value) {
    match window.registry.dispatch(&mut window.session, command, params).unwrap_err() {
        zen_core::DispatchError::Failed { error, .. } => {
            (error.code, error.data.map(Value::Object).unwrap_or(Value::Null))
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_manual_console_counts_its_changes_and_asks_before_they_are_lost() {
    let mut window = Window::new();
    let id = kv_connection(&mut window);
    let path = console_with(&mut window, "SET a 1\nGET a\n");
    let state = window.run("db.transaction.mode", json!({ "mode": "manual" }));
    assert_eq!((state["mode"].as_str(), state["autoCommit"].as_bool()), (Some("manual"), Some(false)));
    let started = window.run("db.console.executeScript", json!({}));
    assert_eq!(finished(&mut window, &started)["status"], "done");
    let status = window.run("db.transaction.status", json!({ "path": path, "limit": 1 }));
    assert_eq!(status["inTransaction"], true, "{status}");
    assert_eq!((status["statements"].as_u64(), status["modifying"].as_u64()), (Some(2), Some(1)));
    assert_eq!((status["log"].as_array().unwrap().len(), status["truncated"].as_bool()), (1, Some(true)));
    let console = window.run("db.console.state", json!({}));
    assert_eq!(console["transaction"]["statements"], 2);
    let pending = window.run("db.transaction.pending", json!({}));
    assert_eq!(
        (pending["total"].as_u64(), pending["transactions"][0]["connection"].as_str()),
        (Some(1), Some(id.as_str()))
    );
    let tab =
        window.state()["tabs"].as_array().unwrap().iter().find(|tab| tab["path"] == json!(path)).unwrap()["id"].clone();
    let (code, data) = refusal(&mut window, "workspace.tab.close", json!({ "workspace": WORKSPACE, "tab": tab }));
    assert_eq!(code.as_deref(), Some("openTransactions"));
    assert_eq!(data["retry"]["command"], "db.console.close");
    assert_eq!(data["pending"][0]["modifying"], 1, "{data}");
    let (code, data) = refusal(&mut window, "db.connection.disconnect", json!({ "connection": id }));
    assert_eq!(
        (code.as_deref(), data["retry"]["command"].as_str()),
        (Some("openTransactions"), Some("db.connection.disconnect"))
    );
    let (code, _) = refusal(&mut window, "db.connection.invalidate", json!({ "connection": id }));
    assert_eq!(code.as_deref(), Some("openTransactions"));
    let (code, _) = refusal(&mut window, "db.transaction.mode", json!({ "mode": "auto" }));
    assert_eq!(code.as_deref(), Some("openTransactions"));
    let committed = window.run("db.transaction.commit", json!({ "path": path }));
    assert_eq!(wait(&mut window, &committed["operation"])["status"], "done");
    let status = window.run("db.transaction.status", json!({}));
    assert_eq!((status["inTransaction"].as_bool(), status["statements"].as_u64()), (Some(false), Some(0)));
    assert_eq!(window.run("db.transaction.pending", json!({}))["total"], 0);
    let started = window.run("db.console.executeScript", json!({}));
    finished(&mut window, &started);
    let closed = window.run("db.console.close", json!({ "path": path, "transactions": "rollback" }));
    assert_eq!(wait(&mut window, &closed["operation"])["status"], "done");
    assert!(!window.titles().iter().any(|title| title.starts_with("console")));
    assert_eq!(window.run("db.transaction.pending", json!({}))["total"], 0);
}

#[test]
fn switching_a_consoles_connection_asks_about_its_changes() {
    let mut window = Window::new();
    let id = kv_connection(&mut window);
    let other = window.run("db.dataSource.add", json!({ "name": "Other", "driver": "memory-kv", "password": "pw" }));
    let other = other["id"].as_str().unwrap().to_owned();
    let path = console_with(&mut window, "SET a 1\n");
    window.run("db.console.chooseConnection", json!({ "connection": id }));
    window.run("db.transaction.mode", json!({ "mode": "manual" }));
    let started = window.run("db.console.executeScript", json!({}));
    finished(&mut window, &started);
    let (code, data) = refusal(&mut window, "db.console.chooseConnection", json!({ "connection": other }));
    assert_eq!(
        (code.as_deref(), data["retry"]["params"]["connection"].as_str()),
        (Some("openTransactions"), Some(other.as_str()))
    );
    window.run("db.console.chooseConnection", json!({ "path": path, "connection": other, "transactions": "rollback" }));
    assert_eq!(window.run("db.console.state", json!({}))["connection"], other.as_str());
    let params = json!({ "connection": id, "context": { "kind": "main" }, "isolation": "chaotic" });
    let (code, _) = refusal(&mut window, "db.transaction.mode", params);
    assert_eq!(code.as_deref(), Some("invalid"), "a level the driver does not name");
}

#[cfg(unix)]
#[test]
fn ssh_asks_through_the_ide_and_a_saved_answer_goes_to_the_keychain() {
    use std::os::unix::fs::PermissionsExt;
    let mut window = Window::new();
    with_drivers(&mut window);
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Far", "driver": "memory-kv", "password": "pw", "host": "db.internal", "port": 6379, "ssh": { "host": "bastion", "user": "deploy" } }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let tools = window.dir.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    let args = tools.join("args");
    let ssh = format!(
        "#!/bin/sh\ncase \" $* \" in *\" -O \"*) exit 0;; *\" -G \"*) exit 0;; esac\nanswer=$(\"$SSH_ASKPASS\" \"deploy@bastion's password: \")\n[ \"$answer\" = \"s3cret\" ] || {{ echo 'Permission denied' >&2; exit 255; }}\necho \"$@\" > '{}'\nfor a in \"$@\"; do [ \"$p\" = -L ] && port=${{a#127.0.0.1:}} && port=${{port%%:*}}; p=$a; done\necho \"debug1: Local forwarding listening on 127.0.0.1 port $port.\" >&2\nexec sleep 600\n",
        args.display()
    );
    let askpass = "#!/bin/sh\nprintf '{\"token\":\"%s\",\"prompt\":\"%s\"}\\n' \"$ZEN_DB_ASKPASS_TOKEN\" \"$1\" | /usr/bin/nc -U \"$ZEN_DB_ASKPASS\" | sed -n 's/.*\"answer\":\"\\([^\"]*\\)\".*/\\1/p'\n";
    for (name, text) in [("ssh", ssh.as_str()), ("askpass", askpass)] {
        std::fs::write(tools.join(name), text).unwrap();
        std::fs::set_permissions(tools.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let engine = window.session.state_mut::<super::Database>().engine();
    engine.set_ssh(zen_db::ssh::SshConfig {
        program: tools.join("ssh"),
        askpass: Some(tools.join("askpass")),
        folder: tools.join("s"),
    });
    let started = window.run("db.connection.connect", json!({ "connection": id }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let prompt = loop {
        let listed = window.run("db.ssh.prompts", json!({}));
        if let Some(prompt) = listed["prompts"].get(0).cloned() {
            break prompt;
        }
        assert!(std::time::Instant::now() < deadline, "ssh never asked");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!((prompt["secret"].as_bool(), prompt["question"].as_bool()), (Some(true), Some(false)), "{prompt}");
    super::connections::step(&mut window.session);
    assert_eq!(window.session.state::<super::connections::SshPrompts>().unwrap().0.len(), 1);
    let answered = window.run("db.ssh.answer", json!({ "prompt": prompt["id"], "value": "s3cret", "save": true }));
    assert_eq!(answered["answered"], true);
    assert!(window.session.state::<super::connections::SshPrompts>().unwrap().0.is_empty());
    assert_eq!(wait(&mut window, &started["operation"])["status"], "done");
    assert!(std::fs::read_to_string(&args).is_ok_and(|text| text.contains("-L 127.0.0.1:")));
    let saved = zen_secrets::database::id(None, &id, zen_secrets::database::Kind::Ssh).unwrap();
    assert!(window.secrets.ids().contains(&saved), "the passphrase went to the Keychain as asked");
    assert!(!window.run("db.ssh.prompts", json!({})).to_string().contains("s3cret"));
    window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
}

#[test]
fn database_files_are_created_once_and_opened_through_their_connection() {
    let mut window = Window::new();
    let project = window.dir.path().join("project");
    let file = project.join("shop.db");
    let created = window.run("db.databaseFile.create", json!({ "path": "shop.db" }));
    assert_eq!(created["path"], file.display().to_string());
    assert_eq!(std::fs::metadata(&file).unwrap().len(), 0);
    std::fs::write(&file, b"SQLite format 3\0kept").unwrap();
    let refused = window.registry.dispatch(&mut window.session, "db.databaseFile.create", json!({ "path": file }));
    let Err(zen_core::DispatchError::Failed { error, .. }) = refused else { panic!("{refused:?}") };
    assert_eq!(error.code.as_deref(), Some("exists"));
    assert_eq!(std::fs::read(&file).unwrap(), b"SQLite format 3\0kept", "an existing file is never replaced");
    assert!(super::files::is_database_file(&file));
    assert!(!super::files::is_database_file(&project.join("main.go")));
    std::fs::write(project.join("notes.db"), "plain text").unwrap();
    assert!(!super::files::is_database_file(&project.join("notes.db")), "a .db that is not SQLite is opened as a file");

    let offered = window.run("db.databaseFile.open", json!({ "path": file }));
    assert_eq!(
        offered["offer"],
        json!({ "driver": "sqlite", "name": "shop.db", "database": file.display().to_string() })
    );
    assert!(offered.get("connection").is_none());
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "driver": "sqlite", "database": "shop.db", "scope": "project" }),
    );
    let opened = window.run("db.databaseFile.open", json!({ "path": "shop.db" }));
    assert_eq!(opened["connection"], added["id"]);
    assert!(opened.get("offer").is_none());
    assert_eq!(crate::workspaces::expanded(&window.session).as_deref(), Some(WORKSPACE));
    let navigator = window.run("db.navigator.state", json!({}));
    assert_eq!(navigator["selected"]["connection"], added["id"], "{navigator}");
    assert_eq!(super::files::resolve(&mut window.session, "sqlite", "shop.db".to_owned()), file.display().to_string());
    assert_eq!(super::files::resolve(&mut window.session, "postgresql", "shop".to_owned()), "shop");
    assert_eq!(super::files::resolve(&mut window.session, "sqlite", ":memory:".to_owned()), ":memory:");
    let missing = window.registry.dispatch(&mut window.session, "db.databaseFile.open", json!({ "path": "gone.db" }));
    let Err(zen_core::DispatchError::Failed { error, .. }) = missing else { panic!("{missing:?}") };
    assert_eq!(error.code.as_deref(), Some(zen_db::sqlite::NO_FILE));
}

#[test]
fn a_project_connection_opens_its_file_from_the_projects_folder() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return;
    }
    let mut window = Window::new();
    window.run("db.databaseFile.create", json!({ "path": "local.sqlite" }));
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Local", "driver": "sqlite", "database": "local.sqlite", "scope": "project" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let started = window.run("db.connection.connect", json!({ "connection": id }));
    let state = wait(&mut window, &started["operation"]);
    assert_eq!(state["status"], "done", "{state}");
    let missing = window.run(
        "db.dataSource.add",
        json!({ "name": "Gone", "driver": "sqlite", "database": "gone.sqlite", "scope": "project" }),
    );
    let started = window.run("db.connection.connect", json!({ "connection": missing["id"] }));
    let state = wait(&mut window, &started["operation"]);
    assert_eq!(state["status"], "failed", "{state}");
    assert!(state["message"].as_str().unwrap().contains("does not exist"), "{state}");
    assert!(!window.dir.path().join("project/gone.sqlite").exists());
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    wait(&mut window, &closing["operation"]);
}

#[test]
fn mysql_connections_take_their_uris_and_quote_names_with_backticks() {
    use zen_db::schema::{Catalog, Level, Object, ObjectKind, Schema, Snapshot};
    let mut window = Window::new();
    let drivers = zen_db::drivers().into_iter().filter(|driver| driver.id() != "postgresql").collect();
    window.session.state_mut::<super::Database>().set_drivers(drivers);
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "url": "mysql://ann:s3cret@db.local:3307/shop?ssl-mode=VERIFY_IDENTITY" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    assert_eq!((added["driver"].as_str(), added["port"].as_u64()), (Some("mysql"), Some(3307)));
    assert_eq!(added["tls"]["mode"], "verify-full");
    let maria = window.run("db.dataSource.add", json!({ "name": "M", "url": "mariadb://root@db.local/app" }));
    assert_eq!(maria["driver"], "mariadb");
    let mut orders = Object::new(ObjectKind::Table, "Order Lines");
    orders.detailed = true;
    let mut nightly = Object::new(ObjectKind::Event, "nightly");
    nightly.detailed = true;
    nightly.definition = Some("CREATE EVENT `nightly` ON SCHEDULE EVERY 1 DAY DO DELETE FROM t".into());
    let snapshot = Snapshot {
        catalogs: vec![Catalog {
            name: "def".into(),
            current: true,
            schemas_read: true,
            schemas: vec![Schema {
                name: "shop".into(),
                level: Level::Details,
                objects: vec![orders, nightly],
                ..Schema::default()
            }],
            ..Catalog::default()
        }],
        ..Snapshot::new(&id, "mysql")
    };
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &snapshot).unwrap();
    let folders = window.run("db.schema.children", json!({ "connection": id, "path": ["def", "shop"] }));
    let labels: Vec<&str> =
        folders["nodes"].as_array().unwrap().iter().map(|node| node["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Tables", "Views", "Functions", "Procedures", "Events"]);
    let name = window.run(
        "db.object.copyName",
        json!({ "connection": id, "path": ["def", "shop", "tables", "Order Lines"], "qualified": true }),
    );
    assert_eq!(name["name"], "`shop`.`Order Lines`");
    let object =
        window.run("db.object.get", json!({ "connection": id, "path": ["def", "shop", "tables", "Order Lines"] }));
    assert_eq!(
        (object["qualified"].as_str(), object["driver"].as_str()),
        (Some("`shop`.`Order Lines`"), Some("mysql"))
    );
    window.run("db.navigator.select", json!({ "connection": id, "path": ["def", "shop", "tables", "Order Lines"] }));
    let state = window.run("db.navigator.state", json!({}));
    let rows: Vec<(usize, &str, &str)> = state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["key"]["connection"] == json!(id))
        .map(|row| {
            (row["depth"].as_u64().unwrap() as usize, row["kind"].as_str().unwrap(), row["label"].as_str().unwrap())
        })
        .collect();
    assert_eq!(
        &rows[..4],
        [(0, "connection", "Shop"), (1, "schema", "shop"), (2, "folder", "Tables"), (3, "table", "Order Lines")],
        "the databases are right under the connection, without the catalogue `def`"
    );
    assert_eq!(state["rows"].as_array().unwrap().iter().filter(|row| row["label"] == "def").count(), 0);
    let ddl = window.run("db.object.ddl", json!({ "connection": id, "path": ["def", "shop", "events", "nightly"] }));
    assert_eq!(ddl["ddl"], "CREATE EVENT `nightly` ON SCHEDULE EVERY 1 DAY DO DELETE FROM t;\n");
}

#[test]
fn a_key_opens_in_its_tab_and_reads_with_its_time_to_live() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let added = window.run("db.dataSource.add", json!({ "name": "Cache", "driver": "memory-kv", "auth": "none" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let expanded = window.run("db.navigator.expand", json!({ "connection": id }));
    assert_eq!(wait(&mut window, &expanded["connecting"])["status"], "done");
    let engine = window.session.state::<super::Database>().unwrap().started().unwrap().clone();
    for command in ["SET user:1 tea", "HSET user:2 name ada"] {
        let op = engine
            .execute(&id, zen_db::driver::ContextKind::Main, command, zen_db::driver::ExecOptions::default())
            .unwrap();
        engine.wait(op, std::time::Duration::from_secs(5));
    }
    window.run("db.navigator.expand", json!({ "connection": id, "path": ["user"] }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        step(&mut window);
        let users = window.run("db.schema.children", json!({ "connection": id, "path": ["user"] }));
        if users["loading"] == false && users["total"] == 2 || std::time::Instant::now() > deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let opened = window.run("db.object.open", json!({ "connection": id, "path": ["user", "1"] }));
    assert_eq!((opened["tab"]["kind"].as_str(), opened["tab"]["title"].as_str()), (Some("key"), Some("1")), "{opened}");
    assert_eq!(opened["tab"]["data"]["path"], json!(["user", "1"]));
    let state = window.state();
    assert_eq!(state["active"], opened["tab"]["id"]);
    let read = window.run("db.key.get", json!({ "connection": id, "path": ["user", "1"], "waitMs": 5000 }));
    assert_eq!((read["status"].as_str(), read["key"]["type"].as_str()), (Some("done"), Some("string")), "{read}");
    assert_eq!(read["key"]["rows"], json!([[{ "kind": "text", "value": "tea" }]]));
    assert!(read["key"].get("ttlMs").is_none());
    let hash = window.run("db.key.get", json!({ "connection": id, "path": ["user", "2"], "waitMs": 5000 }));
    assert_eq!(hash["key"]["columns"][0]["name"], "field");
    let expiring =
        window.run("db.key.expire", json!({ "connection": id, "path": ["user", "1"], "seconds": 60, "waitMs": 5000 }));
    assert_eq!(expiring["key"]["ttlMs"], 60_000);
    let kept = window.run("db.key.expire", json!({ "connection": id, "path": ["user", "1"], "waitMs": 5000 }));
    assert!(kept["key"].get("ttlMs").is_none());
    let started = window.run("db.key.get", json!({ "connection": id, "path": ["user", "1"] }));
    let later = window.run("db.key.state", json!({ "operation": started["operation"], "waitMs": 5000 }));
    assert_eq!(later["status"], "done");
    let refused = window.refused("db.key.get", json!({ "connection": "nope", "path": ["user", "1"] }));
    assert!(refused.contains("nope"), "{refused}");
    let locked = window
        .run("db.dataSource.add", json!({ "name": "Locked", "driver": "memory-kv", "auth": "none", "readOnly": true }));
    let locked = locked["id"].as_str().unwrap().to_owned();
    let refused = window
        .run("db.key.expire", json!({ "connection": locked, "path": ["user", "1"], "seconds": 5, "waitMs": 5000 }));
    assert_eq!(
        (refused["status"].as_str(), refused["error"]["code"].as_str()),
        (Some("failed"), Some("readOnly")),
        "{refused}"
    );
    let relational = window.run("db.dataSource.add", json!({ "name": "Pg", "driver": "postgresql", "auth": "none" }));
    let refused = window.refused("db.key.get", json!({ "connection": relational["id"], "path": ["a", "b"] }));
    assert!(refused.contains("not a key-value database"), "{refused}");
}

#[test]
fn an_empty_sqlite_table_opens_on_its_columns_and_no_rows() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        return;
    }
    let mut window = Window::new();
    let file = window.dir.path().join("empty.db");
    std::fs::File::create(&file).unwrap();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Empty", "driver": "sqlite", "database": file.display().to_string(), "type": "dev" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let setup = window.run(
        "db.query.run",
        json!({ "connection": id, "waitMs": 20_000, "sql": "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)" }),
    );
    assert_eq!(setup["status"], "done", "{setup}");
    let query = window.run(
        "db.query.run",
        json!({ "connection": id, "waitMs": 20_000, "sql": "SELECT name, id FROM t WHERE id > 10" }),
    );
    let result = query["results"][0]["id"].as_u64().unwrap_or_else(|| panic!("an empty result is kept: {query}"));
    let page = window.run("db.result.page", json!({ "id": result }));
    assert_eq!((page["rows"].as_u64(), page["columns"].as_array().map(Vec::len)), (Some(0), Some(2)), "{page}");
    window.run("db.schema.refresh", json!({ "connection": id, "path": ["empty.db", "main"], "force": true }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        step(&mut window);
        let children =
            window.run("db.schema.children", json!({ "connection": id, "path": ["empty.db", "main", "tables"] }));
        if children["nodes"].as_array().unwrap().iter().any(|node| node["label"] == "t") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{children}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let opened = window.run("db.object.open", json!({ "connection": id, "path": ["empty.db", "main", "tables", "t"] }));
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    let state = until_result(&mut window, &tab);
    let first = state["data"]["result"].as_u64().unwrap();
    let page = window.run("db.result.page", json!({ "id": first }));
    assert_eq!(page["rows"], 0, "{page}");
    let names: Vec<&str> =
        page["columns"].as_array().unwrap().iter().filter_map(|column| column["name"].as_str()).collect();
    assert_eq!(names, ["id", "name"], "{page}");
    window.run("db.result.filterByValue", json!({ "column": 1, "operator": "contains", "value": "x" }));
    let filtered = until_done(&mut window, &tab, first);
    assert_eq!(window.run("db.result.page", json!({ "id": filtered }))["rows"], 0);
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    wait(&mut window, &closing["operation"]);
}

#[test]
fn a_sqlite_table_is_filtered_and_edited_in_its_own_dialect() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        return;
    }
    let mut window = Window::new();
    let file = window.dir.path().join("edit.db");
    std::fs::File::create(&file).unwrap();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Lite", "driver": "sqlite", "database": file.display().to_string(), "type": "dev" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let setup = window.run(
        "db.query.run",
        json!({
            "connection": id,
            "waitMs": 20_000,
            "sql": "CREATE TABLE \"Orders\" (id INTEGER PRIMARY KEY, \"Note\" TEXT, flag INTEGER NOT NULL DEFAULT 7);\n\
        INSERT INTO \"Orders\" VALUES (1, 'a', 0), (2, '50%', 1), (3, 'x_y', 0);",
        }),
    );
    assert_eq!(setup["status"], "done", "{setup}");
    window.run("db.schema.refresh", json!({ "connection": id, "path": ["edit.db", "main"], "force": true }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        step(&mut window);
        let children =
            window.run("db.schema.children", json!({ "connection": id, "path": ["edit.db", "main", "tables"] }));
        if children["nodes"].as_array().unwrap().iter().any(|node| node["label"] == "Orders") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{children}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let opened =
        window.run("db.object.open", json!({ "connection": id, "path": ["edit.db", "main", "tables", "Orders"] }));
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    let state = until_result(&mut window, &tab);
    assert_eq!(state["data"]["statement"], "SELECT * FROM \"main\".\"Orders\"");
    let first = state["data"]["result"].as_u64().unwrap();

    let filtered =
        window.run("db.result.filterByValue", json!({ "column": 1, "operator": "contains", "value": "50%" }));
    assert_eq!(
        filtered["applied"],
        "SELECT * FROM \"main\".\"Orders\" WHERE (CAST(\"Note\" AS TEXT) LIKE '%50!%%' ESCAPE '!')"
    );
    let second = until_done(&mut window, &tab, first);
    assert_eq!(window.run("db.result.page", json!({ "id": second }))["rows"], 1, "% is matched as itself");
    let reset = window.run("db.result.resetFilter", json!({}));
    let third = until_done(&mut window, &tab, second);
    assert_eq!(reset["condition"], "");

    let state = window.run("db.result.editState", json!({ "id": third }));
    assert_eq!(state["editable"], true, "{state}");
    assert_eq!(
        (state["table"].as_str(), state["key"]["kind"].as_str()),
        (Some("\"main\".\"Orders\""), Some("primaryKey"))
    );
    window.run("db.result.setCell", json!({ "id": third, "row": 0, "column": 1, "text": "it's" }));
    window.run("db.result.setCell", json!({ "id": third, "row": 1, "column": 2, "default": true }));
    let preview = window.run("db.result.previewChanges", json!({ "id": third }));
    assert_eq!(
        preview["sql"],
        "UPDATE \"main\".\"Orders\" SET \"Note\" = 'it''s' WHERE \"id\" = 1 AND \"Note\" IS 'a' RETURNING \"id\", \"Note\", \"flag\";\n\
UPDATE \"main\".\"Orders\" SET \"flag\" = (7) WHERE \"id\" = 2 AND \"flag\" IS 1 RETURNING \"id\", \"Note\", \"flag\";"
    );
    let started = window.run("db.result.save", json!({ "id": third }));
    let done = wait(&mut window, &started["operation"]);
    assert_eq!(done["status"], "done", "{done}");
    let page = window.run("db.result.page", json!({ "id": third, "limit": 2 }));
    assert_eq!(page["page"][0][1], json!({ "kind": "text", "value": "it's" }));
    assert_eq!(page["page"][1][2], json!({ "kind": "integer", "value": 7 }));

    window.run("db.result.filter", json!({ "id": third, "condition": "nope = 1" }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let failed = loop {
        let state = window.run("db.object.state", json!({ "tab": tab }));
        if state["data"]["error"].is_string() {
            break state;
        }
        assert!(std::time::Instant::now() < deadline, "the filter fails: {state}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(failed["data"]["result"], third, "the rows before stay kept: {failed}");
    let filter = window.run("db.result.filterState", json!({ "id": third }));
    assert!(filter["error"].as_str().is_some_and(|error| error.contains("nope")), "{filter}");
    assert_eq!(filter["condition"], "nope = 1");
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    wait(&mut window, &closing["operation"]);
}

pub(super) fn orders_connection(window: &mut Window, load: bool) -> String {
    use zen_db::schema::{Column, Constraint, ConstraintKind, Object, ObjectKind, Reference};
    let (id, _) = sql_connection(window, "test", true);
    let mut snapshot = sql_snapshot(&id, true);
    let mut orders = Object::new(ObjectKind::Table, "orders");
    orders.detailed = true;
    orders.comment = Some("What customers bought".into());
    orders.columns = [("id", "bigint"), ("customer_id", "bigint"), ("total", "numeric")]
        .iter()
        .enumerate()
        .map(|(at, (name, type_name))| Column {
            name: (*name).into(),
            position: i32::try_from(at + 1).unwrap(),
            type_name: (*type_name).into(),
            nullable: at > 0,
            ..Column::default()
        })
        .collect();
    orders.constraints = vec![Constraint {
        name: "orders_customer_fk".into(),
        kind: ConstraintKind::ForeignKey,
        columns: vec!["customer_id".into()],
        definition: "FOREIGN KEY (customer_id) REFERENCES customers(id)".into(),
        references: Some(Reference {
            schema: "public".into(),
            table: "customers".into(),
            columns: vec!["id".into()],
            on_update: "no action".into(),
            on_delete: "no action".into(),
        }),
        index: None,
        comment: None,
    }];
    snapshot.catalogs[0].schemas[0].objects.push(orders);
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &snapshot).unwrap();
    if load {
        window.run("db.object.get", json!({ "connection": id, "path": ["shop", "public", "tables", "orders"] }));
    }
    id
}

fn labels_of(list: &Value) -> Vec<String> {
    list["items"].as_array().unwrap().iter().map(|item| item["label"].as_str().unwrap().to_owned()).collect()
}

fn connection_state(window: &mut Window, id: &str) -> Value {
    window.run("db.connection.status", json!({ "connection": id }))["state"].clone()
}

#[test]
fn completion_in_a_console_comes_from_its_snapshot_without_connecting() {
    let mut window = Window::new();
    let id = orders_connection(&mut window, true);
    console_with(&mut window, "SELECT o. FROM orders o");
    caret(&mut window, "SELECT o.".len());
    let listed = window.run("editor.completion.start", json!({}));
    assert_eq!(labels_of(&listed), vec!["id", "customer_id", "total"], "{listed}");
    assert_eq!(listed["items"][2]["kind"], "column");
    assert_eq!(listed["items"][2]["detail"], "numeric");
    let index = listed["items"][2]["index"].clone();
    window.run("editor.completion.accept", json!({ "index": index }));
    let text = window.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(text, "SELECT o.total FROM orders o");
    assert_ne!(connection_state(&mut window, &id), "connected", "completion never connects");

    let answer = window.run("db.sql.complete", json!({ "offset": "SELECT o.tot".len() }));
    assert_eq!(answer["prefix"], "tot");
    assert_eq!(answer["from"], "SELECT o.".len());
    assert_eq!(answer["items"][0]["label"], "total");
    assert_eq!(answer["connection"], id.as_str());

    console_with(&mut window, "SELECT * FROM orders o JOIN ");
    let joins = window.run("db.sql.complete", json!({ "offset": "SELECT * FROM orders o JOIN ".len(), "limit": 3 }));
    assert_eq!(joins["items"][0]["label"], "customers c ON c.id = o.customer_id", "{joins}");
    assert_eq!(joins["items"][0]["kind"], "join");
    assert_eq!(joins["truncated"], true);
    assert_eq!(joins["items"].as_array().unwrap().len(), 3);

    console_with(&mut window, "sel");
    caret(&mut window, 3);
    let keywords = window.run("editor.completion.start", json!({}));
    assert_eq!(keywords["items"][0]["label"], "select", "the case typed: {keywords}");
    console_with(&mut window, "SELECT 'cus");
    caret(&mut window, "SELECT 'cus".len());
    let quoted = window.run("db.sql.complete", json!({}));
    assert!(quoted["items"].as_array().unwrap().is_empty(), "nothing inside a literal");
}

#[test]
fn names_in_a_console_open_their_objects_and_describe_their_columns() {
    let mut window = Window::new();
    let id = orders_connection(&mut window, true);
    let text = "SELECT o.total FROM orders o JOIN customers c ON c.id = o.customer_id";
    console_with(&mut window, text);
    let resolved = window.run("db.sql.resolve", json!({ "offset": text.find("total").unwrap() + 1, "limit": 2 }));
    assert_eq!(resolved["target"]["kind"], "column");
    assert_eq!(resolved["target"]["column"], "total");
    assert_eq!(resolved["target"]["path"], json!(["shop", "public", "tables", "orders"]));
    assert_eq!(resolved["range"], json!([9, 14]));
    assert_eq!(resolved["description"]["columns"].as_array().unwrap().len(), 2);
    assert_eq!(resolved["description"]["truncated"], true);

    let docs = window.run("editor.documentation.show", json!({ "offset": text.find("orders").unwrap() }));
    let markdown = docs["markdown"].as_str().unwrap();
    assert!(markdown.contains("**Table** `public.orders`"), "{markdown}");
    assert!(markdown.contains("What customers bought") && markdown.contains("`total` numeric"), "{markdown}");
    assert!(markdown.contains("`id` bigint, not null"), "{markdown}");
    let peek = window.run("editor.declarationPeek.show", json!({ "offset": text.find("customers").unwrap() }));
    assert_eq!(
        peek["declaration"],
        "TABLE public.customers (id bigint, name text, payload jsonb, picture bytea, tags text[], created timestamp with time zone)"
    );
    assert_eq!(peek["language"], "sql");

    let opened = window.run("navigate.declaration.goTo", json!({ "offset": text.find("orders").unwrap() + 2 }));
    assert_eq!(opened["navigated"], true, "{opened}");
    let tab = super::objects::tab_id(&id, &["shop".into(), "public".into(), "tables".into(), "orders".into()]);
    assert!(window.state()["tabs"].as_array().unwrap().iter().any(|known| known["id"] == tab.as_str()));
    let column = window.run("db.sql.navigate", json!({ "offset": text.find("customer_id").unwrap() }));
    assert_eq!(column["target"]["column"], "customer_id");
    let object = window.run("db.object.state", json!({ "tab": tab }));
    assert_eq!(object["section"], "columns", "{object}");
    let nothing = window.registry.dispatch(&mut window.session, "db.sql.navigate", json!({ "offset": 1 })).unwrap_err();
    assert!(format!("{nothing:?}").contains("unknownName"));
    assert_ne!(connection_state(&mut window, &id), "connected");
}

#[test]
fn unknown_tables_and_columns_of_a_console_are_warnings() {
    let mut window = Window::new();
    orders_connection(&mut window, true);
    let text = "SELECT nme FROM customers;\nSELECT o.totl FROM orders o;\nSELECT * FROM nothing;\nCREATE TABLE fresh (a int);\nINSERT INTO fresh VALUES (1);\n";
    let path = console_with(&mut window, text);
    window.run("db.console.state", json!({}));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let problems = loop {
        let answer = window.run("problems.file.list", json!({ "path": path }));
        let problems = crate::language::listed_problems(&answer);
        if !problems.is_empty() || std::time::Instant::now() > deadline {
            break problems;
        }
        window.run("db.sql.complete", json!({ "offset": 0 }));
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let messages: Vec<&str> = problems.iter().map(|problem| problem["message"].as_str().unwrap()).collect();
    assert_eq!(
        messages,
        vec!["Unknown column nme", "Unknown column totl in o", "Unknown table nothing"],
        "{problems:?}"
    );
    assert!(problems.iter().all(|problem| problem["severity"] == "warning" && problem["source"] == "database"));
    assert_eq!((problems[1]["line"].as_u64(), problems[1]["column"].as_u64()), (Some(2), Some(10)));
}

#[test]
fn completion_waits_for_the_snapshot_read_from_disk() {
    let mut window = Window::new();
    orders_connection(&mut window, false);
    console_with(&mut window, "SELECT * FROM orders o WHERE o.");
    caret(&mut window, "SELECT * FROM orders o WHERE o.".len());
    window.run("editor.completion.start", json!({}));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let listed = window.run("editor.completion.list", json!({}));
        if labels_of(&listed) == ["id", "customer_id", "total"] {
            assert_eq!(listed["pending"], false);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{listed}");
        assert_eq!(listed["pending"], true, "it waits while the snapshot is read: {listed}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn export_done(window: &mut Window, started: &Value) -> Value {
    let state = window.run("db.export.state", json!({ "id": started["id"], "waitMs": 30_000 }));
    state["exports"][0].clone()
}

#[test]
fn tables_a_result_and_a_console_query_export_to_files_and_the_clipboard_with_the_settings_kept() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        return;
    }
    let mut window = Window::new();
    let file = window.dir.path().join("shop.db");
    std::fs::File::create(&file).unwrap();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "driver": "sqlite", "database": file.display().to_string(), "type": "dev" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let setup = window.run(
        "db.query.run",
        json!({
            "connection": id,
            "waitMs": 20_000,
            "sql": "CREATE TABLE orders (id INTEGER PRIMARY KEY, note TEXT);\n\
                    INSERT INTO orders VALUES (1, 'secret note'), (2, '=1+1'), (3, NULL);\n\
                    CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT);\n\
                    INSERT INTO people VALUES (1, 'Ann');",
        }),
    );
    assert_eq!(setup["status"], "done", "{setup}");
    let out = window.dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    let started = window.run(
        "db.export.start",
        json!({
            "source": { "kind": "tables", "connection": id, "paths": [
                ["shop.db", "main", "tables", "orders"], ["shop.db", "main", "tables", "people"]
            ] },
            "settings": { "format": "csv" },
            "folder": out,
            "pattern": "${connection}-${table}",
        }),
    );
    assert_eq!(started["items"][0]["path"], json!(out.join("Shop-orders.csv")));
    assert_eq!(started["items"][1]["path"], json!(out.join("Shop-people.csv")));
    let state = export_done(&mut window, &started);
    assert_eq!(state["status"], "done", "{state}");
    assert_eq!((state["rows"].as_u64(), state["items"][0]["rows"].as_u64()), (Some(4), Some(3)));
    assert!(!state.to_string().contains("secret note"), "answers never hold rows: {state}");
    assert_eq!(std::fs::read_to_string(out.join("Shop-orders.csv")).unwrap(), "id,note\n1,secret note\n2,'=1+1\n3,\n");
    assert_eq!(std::fs::read_to_string(out.join("Shop-people.csv")).unwrap(), "id,name\n1,Ann\n");
    let kept = window.run("db.export.settings", json!({}));
    assert_eq!((kept["pattern"].as_str(), kept["folder"].as_str()), (Some("${connection}-${table}"), out.to_str()));
    assert!(window.dir.path().join("data/databases/export.json").exists());
    assert_eq!(kept["formats"].as_array().unwrap().len(), 8);

    let again = json!({
        "source": { "kind": "tables", "connection": id, "paths": [["shop.db", "main", "tables", "people"]] },
        "folder": out,
    });
    let (code, data) = refusal(&mut window, "db.export.start", again.clone());
    assert_eq!(code.as_deref(), Some("exists"));
    assert_eq!(data["paths"], json!([out.join("Shop-people.csv")]));
    let mut overwrite = again;
    overwrite["overwrite"] = json!(true);
    overwrite["settings"] = json!({ "format": "markdown", "nullText": "NULL" });
    let started = window.run("db.export.start", overwrite);
    export_done(&mut window, &started);
    assert_eq!(
        std::fs::read_to_string(out.join("Shop-people.md")).unwrap(),
        "| id | name |\n| --- | --- |\n| 1 | Ann |\n"
    );

    let ran = window.run(
        "db.query.run",
        json!({ "connection": id, "sql": "SELECT note FROM orders ORDER BY id DESC", "waitMs": 10_000 }),
    );
    let result = ran["results"][0]["id"].as_u64().unwrap();
    let json_file = out.join("notes.json");
    let started = window.run(
        "db.export.start",
        json!({ "source": { "kind": "result", "id": result }, "settings": { "format": "json" }, "path": json_file, "remember": false }),
    );
    assert_eq!(export_done(&mut window, &started)["status"], "done");
    let parsed: Value = serde_json::from_str(&std::fs::read_to_string(&json_file).unwrap()).unwrap();
    assert_eq!(parsed, json!([{ "note": null }, { "note": "=1+1" }, { "note": "secret note" }]));
    assert_eq!(
        window.run("db.export.settings", json!({}))["settings"]["format"],
        "markdown",
        "remember: false keeps the last"
    );

    console_with(&mut window, "SELECT name FROM people");
    window.run("db.console.chooseConnection", json!({ "connection": id }));
    let started = window.run(
        "db.export.start",
        json!({ "source": { "kind": "console" }, "clipboard": true, "settings": { "format": "sql", "table": "staff" } }),
    );
    assert_eq!(started["clipboard"], true);
    assert_eq!(export_done(&mut window, &started)["status"], "done");
    step(&mut window);
    let copied = window.session.state_mut::<super::exports::Exports>().take_clipboard();
    assert_eq!(copied.as_deref(), Some("INSERT INTO staff (\"name\") VALUES\n\t('Ann');\n"));
    let notes = window.session.state::<crate::notifications::Notifications>().unwrap();
    let bodies: Vec<String> = notes.newest_first().map(|note| note.body.clone()).collect();
    assert!(bodies.iter().any(|body| body == "One row was copied to the clipboard."), "{bodies:?}");
    assert!(bodies.iter().any(|body| body.contains("Shop-people.md")), "{bodies:?}");
    let reveal = notes.newest_first().find(|note| note.body.contains("notes.json")).unwrap();
    let commands: Vec<&str> = reveal.actions.iter().map(|action| action.command.as_str()).collect();
    assert_eq!(commands, ["file.document.open", "file.fs.reveal"]);

    let (code, _) = refusal(
        &mut window,
        "db.export.start",
        json!({ "source": { "kind": "tables", "connection": id, "paths": [["shop.db", "main"]] } }),
    );
    assert_eq!(code.as_deref(), Some("notATable"));
    let (code, _) = refusal(
        &mut window,
        "db.export.start",
        json!({ "source": { "kind": "tables", "connection": id, "paths": [["shop.db", "main", "tables", "people"]] }, "folder": out.join("missing"), "clipboard": false }),
    );
    assert_eq!(code.as_deref(), Some("noFolder"));
    assert_eq!(window.run("db.export.state", json!({}))["exports"].as_array().unwrap().len(), 4);
    assert_eq!(window.run("db.export.cancel", json!({ "id": started["id"] }))["cancelled"], false);
    let closing = window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    wait(&mut window, &closing["operation"]);
}

#[test]
fn reformat_code_reads_a_console_in_the_dialect_of_its_connection() {
    let mut window = Window::new();
    let added = window.run("db.dataSource.add", json!({ "name": "Shop", "driver": "mysql", "host": "localhost" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let text = "select `a  b`, \"x  y\" from t # note\n";
    let path = console_with(&mut window, text);
    window.run("db.console.chooseConnection", json!({ "connection": id }));
    window.run("editor.code.reformat", json!({ "path": path, "waitMs": 10000 }));
    let formatted = window.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(formatted, "select `a  b`,\n    \"x  y\"\nfrom t # note\n");
    let answer = window.run("db.sql.format", json!({ "text": "select 'it\\'s  a', b", "connection": id }));
    assert_eq!(answer["text"], "select 'it\\'s  a',\n    b");
    let state = window.run("db.connection.status", json!({ "connection": id }));
    assert_eq!(state["connections"][0]["state"], "disconnected", "nothing connects");
}

#[test]
fn profiles_are_listed_over_their_drivers_and_a_profile_or_its_url_fills_a_new_connection() {
    let mut window = Window::new();
    window.session.state_mut::<super::Database>().set_drivers(zen_db::drivers());
    let listed = window.run("db.driver.list", json!({}));
    let profiles = listed["profiles"].as_array().unwrap();
    assert_eq!(listed["totalProfiles"].as_u64(), Some(profiles.len() as u64));
    assert_eq!(listed["truncated"], false);
    assert!(profiles.len() >= 25, "{}", profiles.len());
    let cockroach = profiles.iter().find(|profile| profile["id"] == "cockroachdb").unwrap();
    assert_eq!((cockroach["driver"].as_str(), cockroach["defaultPort"].as_u64()), (Some("postgresql"), Some(26257)));
    assert_eq!(cockroach["capabilities"]["uriSchemes"][0], "cockroachdb");
    let mariadb = listed["drivers"].as_array().unwrap().iter().find(|driver| driver["id"] == "mariadb").unwrap();
    assert_eq!(mariadb["family"], "mysql");

    let narrow = window.run("db.driver.list", json!({ "family": "redis", "limit": 2, "capabilities": false }));
    assert_eq!(narrow["profiles"].as_array().unwrap().len(), 2);
    assert_eq!(narrow["truncated"], true);
    assert!(narrow["profiles"][0].get("capabilities").is_none());
    assert_eq!(narrow["drivers"].as_array().unwrap().len(), 1, "only Redis of the drivers");
    let found = window.run("db.driver.list", json!({ "query": "tidb" }));
    assert_eq!((found["profiles"][0]["id"].as_str(), found["drivers"].as_array().unwrap().len()), (Some("tidb"), 0));

    let added = window.run("db.dataSource.add", json!({ "name": "Roach", "profile": "cockroachdb", "host": "db" }));
    assert_eq!((added["driver"].as_str(), added["profile"].as_str()), (Some("postgresql"), Some("cockroachdb")));
    assert_eq!((added["port"].as_u64(), added["database"].as_str()), (Some(26257), Some("defaultdb")));
    let quest = window.run("db.dataSource.add", json!({ "name": "Quest", "profile": "questdb", "port": 9000 }));
    assert_eq!((quest["port"].as_u64(), quest["settings"]["pooler"].as_bool()), (Some(9000), Some(true)));

    let by_scheme =
        window.run("db.dataSource.add", json!({ "name": "C2", "url": "cockroachdb://root@crdb:26258/bank" }));
    assert_eq!(
        (by_scheme["driver"].as_str(), by_scheme["profile"].as_str()),
        (Some("postgresql"), Some("cockroachdb"))
    );
    assert_eq!((by_scheme["port"].as_u64(), by_scheme["database"].as_str()), (Some(26258), Some("bank")));
    let neon = window.run(
        "db.dataSource.add",
        json!({ "name": "Neon", "url": "postgresql://alex:pw@ep-cool-darkness-123456.us-east-2.aws.neon.tech/neondb" }),
    );
    assert_eq!((neon["profile"].as_str(), neon["tls"]["mode"].as_str()), (Some("neon"), Some("require")));
    let kept = window.run(
        "db.dataSource.add",
        json!({ "name": "Neon 2", "url": "postgresql://ep-x.aws.neon.tech/db?sslmode=verify-full" }),
    );
    assert_eq!(kept["tls"]["mode"], "verify-full", "the URI's own TLS mode wins");
    let valkey = window.run("db.dataSource.add", json!({ "name": "V", "url": "valkeys://cache:6380" }));
    assert_eq!((valkey["driver"].as_str(), valkey["profile"].as_str()), (Some("redis"), Some("valkey")));
    assert_eq!(valkey["tls"]["mode"], "require");
    let plain = window.run("db.dataSource.add", json!({ "name": "P", "url": "postgresql://db.example.com/shop" }));
    assert!(plain.get("profile").is_none(), "{plain}");

    let id = plain["id"].as_str().unwrap().to_owned();
    let changed = window.run("db.dataSource.update", json!({ "id": id, "profile": "cockroachdb" }));
    assert_eq!(changed["profile"], "cockroachdb");
    assert!(changed.get("port").is_none(), "a saved connection keeps its own port: {changed}");
    let cleared = window.run("db.dataSource.update", json!({ "id": id, "profile": "" }));
    assert!(cleared.get("profile").is_none());
    let refused = window.refused("db.dataSource.add", json!({ "name": "X", "profile": "nosuchdb" }));
    assert!(refused.contains("nosuchdb"), "{refused}");
}

#[test]
fn drivers_of_plugins_are_listed_after_the_compiled_ones_and_have_a_log() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let declaration: zen_db::external::DriverDeclaration = serde_json::from_value(json!({
        "id": "duckdb",
        "name": "DuckDB",
        "icon": "database",
        "process": "shared",
        "defaultPort": 0,
        "capabilities": { "models": ["relational"], "language": "sql:duckdb", "password": false },
    }))
    .unwrap();
    let declared = zen_db::external::Declared::default();
    declared.set(vec![
        zen_db::external::ExternalSpec {
            declaration: declaration.clone(),
            program: window.dir.path().join("no-such-program"),
            sha256: None,
            plugin: Some("duck".into()),
        },
        zen_db::external::ExternalSpec {
            declaration: zen_db::external::DriverDeclaration { id: "postgresql".into(), ..declaration },
            program: window.dir.path().join("shadowed"),
            sha256: None,
            plugin: Some("shadow".into()),
        },
    ]);
    let source = declared.clone();
    window.session.state_mut::<super::Database>().set_more_drivers(std::sync::Arc::new(move || source.as_drivers()));
    let drivers = window.run("db.driver.list", json!({}));
    let ids: Vec<_> = drivers["drivers"].as_array().unwrap().iter().map(|driver| driver["id"].clone()).collect();
    assert_eq!(ids, [json!("postgresql"), json!("memory-kv"), json!("duckdb")], "a compiled-in id is never replaced");
    let duck = &drivers["drivers"][2];
    assert_eq!(duck["external"]["plugin"], "duck");
    assert_eq!(duck["external"]["installed"], false);
    assert_eq!(duck["external"]["process"], "shared");
    assert_eq!(duck["external"]["running"], 0, "listing starts nothing");
    assert_eq!(duck["capabilities"]["language"], "sql:duckdb");
    assert!(drivers["drivers"][0].get("external").is_none());

    let added = window.run("db.dataSource.add", json!({ "name": "Duck", "driver": "duckdb" }));
    let connecting = window.run("db.connection.connect", json!({ "connection": added["id"] }));
    let failed = wait(&mut window, &connecting["operation"]);
    assert_eq!(failed["status"], "failed", "{failed}");
    assert!(failed["message"].as_str().unwrap().contains("not installed"), "{failed}");

    let log = window.run("db.driver.log", json!({ "id": "duckdb", "limit": 10 }));
    assert_eq!(log["truncated"], false);
    assert!(log["lines"].is_array());
    let refused = window.refused("db.driver.log", json!({ "id": "postgresql" }));
    assert!(refused.contains("external"), "{refused}");
}

#[test]
fn an_agent_cannot_read_or_write_files_hidden_from_it_through_database_commands() {
    let mut window = Window::new();
    let project = window.dir.path().join("project");
    std::fs::write(project.join(".env"), "name,secret\nkey,hunter2\n").unwrap();
    std::fs::create_dir_all(project.join("secrets")).unwrap();
    std::fs::write(project.join("secrets/run.sql"), "SELECT 'hunter2';\n").unwrap();
    window
        .session
        .state_mut::<crate::hidden_files::HiddenFromAgents>()
        .set(&[".env".to_owned(), "secrets/**".to_owned()]);
    let as_agent = |window: &mut Window, command: &str, params: Value| {
        zen_core::macros::as_agent(&mut window.session, |session| window.registry.dispatch(session, command, params))
    };
    for (command, params) in [
        ("db.csvImport.preview", json!({ "file": project.join(".env") })),
        ("db.databaseFile.open", json!({ "path": project.join(".env").display().to_string() })),
        ("db.console.statement", json!({ "path": project.join("secrets/run.sql"), "offset": 0 })),
    ] {
        let refused = as_agent(&mut window, command, params).unwrap_err();
        let text = format!("{refused:?}");
        assert!(text.contains("redacted") && !text.contains("hunter2"), "{command}: {text}");
    }
    let preview = window.run("db.csvImport.preview", json!({ "file": project.join(".env") }));
    assert!(preview.to_string().contains("hunter2"), "{preview}");
}

struct DialectPlugin(String);

impl zen_syntax::Outliner for DialectPlugin {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".sql")
    }

    fn outline(&self, text: &str) -> Vec<zen_syntax::OutlineItem> {
        let end = text.chars().count();
        vec![zen_syntax::OutlineItem {
            name: self.0.clone(),
            detail: None,
            kind: zen_syntax::SymbolKind::Key,
            start: 0,
            end,
            selection_start: 0,
            children: Vec::new(),
        }]
    }
}

impl zen_syntax::TextFeatures for DialectPlugin {
    fn supports(&self, feature: zen_syntax::Feature) -> bool {
        matches!(feature, zen_syntax::Feature::Outline | zen_syntax::Feature::Completion)
    }

    fn folds(&self, _: &str) -> Vec<zen_syntax::LineFold> {
        Vec::new()
    }

    fn complete(&self, _: &str, caret: usize) -> Option<zen_syntax::Suggestions> {
        let item = zen_syntax::Suggestion {
            label: format!("select_in_{}", self.0),
            kind: "keyword".to_owned(),
            insert: format!("select_in_{}", self.0),
            caret: None,
            detail: None,
        };
        Some(zen_syntax::Suggestions { from: caret.saturating_sub(3), items: vec![item] })
    }

    fn expand(&self, _: &str, _: usize) -> Option<zen_syntax::Expansion> {
        None
    }

    fn in_dialect(&self, dialect: &str) -> Option<std::sync::Arc<dyn zen_syntax::TextFeatures>> {
        Some(std::sync::Arc::new(DialectPlugin(dialect.to_owned())))
    }
}

#[test]
fn the_sql_plugin_reads_a_console_in_its_connections_dialect_and_a_file_without_one() {
    let mut window = Window::new().with_connection();
    window
        .session
        .state_mut::<crate::plugins::PluginFeatures>()
        .set(vec![std::sync::Arc::new(DialectPlugin("none".to_owned()))]);
    let console = console_with(&mut window, "sel");
    let structure = window.run("editor.structure.get", json!({ "path": console }));
    assert_eq!(structure["symbols"][0]["name"], "postgres", "{structure}");
    caret(&mut window, 3);
    let listed = window.run("editor.completion.start", json!({}));
    assert!(
        !labels_of(&listed).iter().any(|label| label.starts_with("select_in_")),
        "the connection completes: {listed}"
    );
    assert!(labels_of(&listed).contains(&"select".to_owned()), "{listed}");

    let file = window.dir.path().join("project/query.sql");
    std::fs::write(&file, "sel").unwrap();
    window.run("file.document.open", json!({ "path": file }));
    let structure = window.run("editor.structure.get", json!({ "path": file }));
    assert_eq!(structure["symbols"][0]["name"], "none", "no binding, no dialect: {structure}");
    caret(&mut window, 3);
    let listed = window.run("editor.completion.start", json!({}));
    assert_eq!(labels_of(&listed), ["select_in_none"], "{listed}");
}

#[test]
fn go_to_row_selects_a_read_row_and_the_value_panel_is_open_at_first() {
    let mut window = Window::new();
    assert_eq!(window.run("db.settings.get", json!({}))["valuePanel"], true);
    let (id, _) = sql_connection(&mut window, "dev", true);
    let (_, result) = open_customers(&mut window, &id);
    let selected = window.run("db.result.select", json!({ "id": result, "row": 3, "column": 1 }));
    assert_eq!((selected["row"].as_u64(), selected["rows"].as_u64()), (Some(3), Some(25)), "{selected}");
    assert_eq!(super::values::take_selection(&mut window.session, result), Some((2, 1)));
    assert_eq!(super::values::take_selection(&mut window.session, result), None, "taken once");
    let refused = window.refused("db.result.select", json!({ "id": result, "row": 26 }));
    assert!(refused.contains("25 rows are read"), "{refused}");
}

#[test]
fn a_duplicated_connection_keeps_its_settings_and_password_under_a_new_name() {
    use zen_secrets::SecretStore as _;
    use zen_secrets::database::Kind;
    let mut window = Window::new();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "url": "postgresql://ann:s3cret@db.local:6543/shop", "folder": "Work", "type": "test" }),
    );
    wait(&mut window, &added["secretOperation"]);
    let id = added["id"].as_str().unwrap().to_owned();
    let copy = window.run("db.dataSource.duplicate", json!({ "id": id }));
    assert_eq!(wait(&mut window, &copy["secretOperation"])["status"], "done");
    let copy_id = copy["id"].as_str().unwrap().to_owned();
    assert_ne!(copy_id, id);
    assert_eq!(
        (copy["name"].as_str(), copy["host"].as_str(), copy["port"].as_u64(), copy["folder"].as_str()),
        (Some("Shop (copy)"), Some("db.local"), Some(6543), Some("Work"))
    );
    assert_eq!((copy["type"].as_str(), copy["hasPassword"].as_bool()), (Some("test"), Some(true)));
    let key = zen_secrets::database::id(None, &copy_id, Kind::Password).unwrap();
    assert_eq!(window.secrets.get(&key).unwrap().unwrap().expose(), "s3cret");
    let again = window.run("db.dataSource.duplicate", json!({ "id": id }));
    wait(&mut window, &again["secretOperation"]);
    assert_eq!(again["name"], "Shop (copy) 2");
    assert_eq!(window.run("db.dataSource.list", json!({}))["total"], 3);
}

#[test]
fn postgresql_indexes_roles_extensions_rules_and_dependencies_have_properties() {
    use zen_db::schema::{
        Catalog, Dependency, Extension, Index, Level, Object, ObjectKind, Role, Rule, Schema, Snapshot,
    };
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut items = Object::new(ObjectKind::Table, "items");
    items.detailed = true;
    items.owner = Some("app".into());
    items.indexes = vec![Index {
        name: "items_price".into(),
        columns: vec!["price".into()],
        method: "btree".into(),
        definition: "CREATE INDEX items_price ON shop.items USING btree (price)".into(),
        ..Index::default()
    }];
    items.rules = vec![Rule {
        name: "log_items".into(),
        event: "insert".into(),
        instead: false,
        definition: "CREATE RULE log_items AS ON INSERT TO shop.items DO INSERT INTO shop.log VALUES ('x');".into(),
    }];
    items.dependencies = vec![Dependency {
        direction: "usedBy".into(),
        schema: "shop".into(),
        name: "cheap".into(),
        kind: "view".into(),
    }];
    let snapshot = Snapshot {
        catalogs: vec![Catalog {
            name: "db".into(),
            current: true,
            schemas_read: true,
            schemas: vec![Schema {
                name: "shop".into(),
                level: Level::Details,
                objects: vec![items],
                ..Schema::default()
            }],
            extensions: vec![Extension { name: "pgcrypto".into(), version: "1.3".into(), ..Extension::default() }],
            roles: vec![Role { name: "reader".into(), login: true, connection_limit: -1, ..Role::default() }],
            ..Catalog::default()
        }],
        ..Snapshot::new("pg", "postgresql")
    };
    let path = |parts: &[&str]| parts.iter().map(|part| (*part).to_owned()).collect::<Vec<_>>();
    let props = |parts: &[&str], section: Option<&str>| {
        super::objects::properties(&snapshot, "pg", &path(parts), section, 50).unwrap()
    };
    let table = props(&["db", "shop", "tables", "items"], Some("dependencies"));
    let sections: Vec<&str> = table.sections.iter().map(|section| section.id.as_str()).collect();
    assert!(sections.contains(&"dependencies") && sections.contains(&"rules"), "{sections:?}");
    assert_eq!(table.rows, [["used by", "shop", "cheap", "view"]]);
    let rules = props(&["db", "shop", "tables", "items"], Some("rules"));
    assert_eq!((rules.rows[0][0].as_str(), rules.rows[0][1].as_str()), ("log_items", "INSERT"));
    let index = props(&["db", "shop", "indexes", "items_price"], None);
    assert_eq!((index.kind.as_str(), index.section.as_str()), ("index", "settings"));
    assert!(index.rows.contains(&vec!["Table".to_owned(), "shop.items".to_owned()]), "{:?}", index.rows);
    assert!(index.rows.contains(&vec!["Method".to_owned(), "btree".to_owned()]), "{:?}", index.rows);
    let role = props(&["db", zen_db::schema::ROLES, "reader"], None);
    assert_eq!(role.kind, "role");
    assert!(role.rows.contains(&vec!["Can log in".to_owned(), "yes".to_owned()]), "{:?}", role.rows);
    assert!(role.rows.contains(&vec!["Connection limit".to_owned(), "no limit".to_owned()]), "{:?}", role.rows);
    let extension = props(&["db", zen_db::schema::EXTENSIONS, "pgcrypto"], None);
    assert_eq!(extension.header[0].value, "pgcrypto");
    assert!(extension.rows.contains(&vec!["Version".to_owned(), "1.3".to_owned()]));
    assert_eq!(super::objects::pages("role"), [super::objects::Page::Properties]);
    assert!(super::objects::properties(&snapshot, "pg", &path(&["db", "#roles", "nobody"]), None, 50).is_none());
}
#[test]
fn a_driver_that_asks_no_password_gets_connections_without_one() {
    let mut window = Window::new();
    let driver = zen_db::memory::MemoryDriver::sql().without_password();
    let drivers: Vec<std::sync::Arc<dyn zen_db::driver::Driver>> = vec![std::sync::Arc::new(driver)];
    window.session.state_mut::<super::Database>().set_drivers(drivers);
    let added = window.run("db.dataSource.add", json!({ "name": "Lake", "driver": zen_db::memory::SQL }));
    assert_eq!(added["auth"], "none", "{added}");
}

#[test]
fn a_program_named_by_a_project_connection_is_never_passed_to_its_driver() {
    let mut window = Window::new();
    let settings = json!({ "program": "/project/tools/sql", "javaHome": "/project/jdk", "role": "SYSDBA" });
    let shared = window
        .run("db.dataSource.add", json!({ "scope": "project", "name": "Team", "host": "h", "settings": settings }));
    let mine = window.run("db.dataSource.add", json!({ "name": "Mine", "host": "h", "settings": settings }));
    let request = |window: &mut Window, id: &Value| {
        let entry = super::sources::find(&window.session, id.as_str().unwrap()).unwrap();
        super::connections::request(&mut window.session, &entry, None).source.settings
    };
    let team = request(&mut window, &shared["id"]);
    assert!(!team.contains_key("program") && !team.contains_key("javaHome"), "{team:?}");
    assert_eq!(team["role"], "SYSDBA", "other settings stay");
    let own = request(&mut window, &mine["id"]);
    assert_eq!(own["program"], "/project/tools/sql", "the person's own connection keeps it");
}

#[test]
fn an_agent_runs_duckdb_sql_only_on_the_database_itself() {
    let mut window = Window::new();
    with_drivers(&mut window);
    let declaration: zen_db::external::DriverDeclaration = serde_json::from_value(json!({
        "id": "duckdb", "name": "DuckDB", "icon": "database", "process": "shared", "defaultPort": 0,
        "capabilities": { "models": ["relational"], "language": "sql:duckdb", "password": false },
    }))
    .unwrap();
    let declared = zen_db::external::Declared::default();
    declared.set(vec![zen_db::external::ExternalSpec {
        declaration,
        program: window.dir.path().join("no-such-program"),
        sha256: None,
        plugin: Some("duck".into()),
    }]);
    window.session.state_mut::<super::Database>().set_more_drivers(std::sync::Arc::new(move || declared.as_drivers()));
    let added = window.run("db.dataSource.add", json!({ "name": "Duck", "driver": "duckdb" }));
    let as_agent = |window: &mut Window, sql: &str| {
        let params = json!({ "connection": added["id"], "sql": sql, "waitMs": 0 });
        zen_core::macros::as_agent(&mut window.session, |session| {
            window.registry.dispatch(session, "db.query.run", params)
        })
    };
    for sql in
        ["SELECT * FROM read_csv('secrets.csv')", "COPY t TO 'out.csv'", "INSTALL httpfs", "SELECT 1;\nATTACH 'x.db'"]
    {
        match as_agent(&mut window, sql) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some("agentOutsideDatabase"), "{sql}: {error:?}");
                let hand_off = &error.data.as_ref().unwrap()["handOff"];
                assert_eq!(hand_off["command"], "db.console.new", "{sql}");
            }
            other => panic!("{sql}: not refused: {other:?}"),
        }
    }
    let inside = as_agent(&mut window, "SELECT * FROM orders");
    assert!(
        !matches!(&inside, Err(zen_core::DispatchError::Failed { error, .. }) if error.code.as_deref() == Some("agentOutsideDatabase")),
        "{inside:?}"
    );
    let person = window.registry.dispatch(
        &mut window.session,
        "db.query.run",
        json!({ "connection": added["id"], "sql": "SELECT * FROM read_csv('orders.csv')", "waitMs": 0 }),
    );
    assert!(
        !matches!(&person, Err(zen_core::DispatchError::Failed { error, .. }) if error.code.as_deref() == Some("agentOutsideDatabase")),
        "{person:?}"
    );
}

#[test]
fn an_agent_never_chooses_the_program_a_driver_runs() {
    let mut window = Window::new();
    let as_agent = |window: &mut Window, command: &str, params: Value| {
        zen_core::macros::as_agent(&mut window.session, |session| window.registry.dispatch(session, command, params))
    };
    let code = |answer: Result<Value, zen_core::DispatchError>| match answer {
        Err(zen_core::DispatchError::Failed { error, .. }) => error.code.unwrap_or_default(),
        other => panic!("not refused: {other:?}"),
    };
    let named = json!({ "name": "Lake", "host": "h", "settings": { "program": "/tmp/anything" } });
    assert_eq!(code(as_agent(&mut window, "db.dataSource.add", named.clone())), "agentNotAllowed");
    let java = json!({ "name": "Ora", "host": "h", "settings": { "javaHome": "/tmp/jdk" } });
    assert_eq!(code(as_agent(&mut window, "db.dataSource.add", java)), "agentNotAllowed");
    let plain = as_agent(&mut window, "db.dataSource.add", json!({ "name": "Plain", "host": "h" })).unwrap();
    let change = json!({ "id": plain["id"], "settings": { "program": "/tmp/anything" } });
    assert_eq!(code(as_agent(&mut window, "db.dataSource.update", change)), "agentNotAllowed");
    let mine = window.run("db.dataSource.add", named);
    let kept = json!({ "id": mine["id"], "settings": { "program": "/tmp/anything", "role": "x" } });
    as_agent(&mut window, "db.dataSource.update", kept).unwrap();
    let other = json!({ "id": mine["id"], "settings": { "role": "y" } });
    as_agent(&mut window, "db.dataSource.update", other).unwrap();
    let entry = super::sources::find(&window.session, mine["id"].as_str().unwrap()).unwrap();
    assert_eq!(entry.source.settings["program"], "/tmp/anything");
}

#[test]
fn a_sql_file_that_is_not_open_is_split_as_it_is_now_after_it_changes() {
    let mut window = Window::new();
    let path = window.dir.path().join("project/script.sql");
    let mut seed = 0x6c07_8965_5ad6_9f43_u64;
    for _ in 0..40 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let statements = 1 + (seed % 6) as usize;
        let text: String = (0..statements)
            .map(|at| format!("SELECT {};\n", "x".repeat(((seed >> (at * 5 + 3)) % 30) as usize)))
            .collect();
        std::fs::write(&path, &text).unwrap();
        let split = window.run("db.script.split", json!({ "path": path, "dialect": "postgres" }));
        let fresh = window.run("db.script.split", json!({ "text": text, "dialect": "postgres" }));
        assert_eq!(split["statements"], fresh["statements"], "{text:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_private_file_is_a_new_file_only_its_owner_can_read_never_an_existing_one_or_a_link() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("secret");
    super::write_private(&file, "hunter2").unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(super::write_private(&file, "other").is_err(), "never an existing file");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hunter2");
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(super::write_private(&link, "hunter2").is_err(), "never through a link");
    assert!(!target.exists());
}
