use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::Bindings;
use crate::Workbench;
use crate::workspaces::Workspaces;

const FUNCTION: &str = "CREATE FUNCTION f() RETURNS int AS $$ SELECT 1; $$ LANGUAGE sql;\n\n-- the second\nSELECT 2;\n";

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
        std::fs::create_dir_all(project.join("queries")).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::write(project.join("queries/report.sql"), "SET a 1\nGET a\n").unwrap();
        std::fs::write(project.join("select.sql"), "SELECT 1;\n").unwrap();
        std::fs::write(project.join("schema.sql"), FUNCTION).unwrap();
        let mut session = Session::new();
        super::super::install(&mut session);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
        session.set_state(super::super::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
        let drivers: Vec<Arc<dyn zen_db::driver::Driver>> =
            vec![Arc::new(zen_db::memory::MemoryDriver::new(false)), Arc::new(zen_db::memory::MemoryDriver::sql())];
        session.state_mut::<super::super::Database>().set_drivers(drivers);
        Self { registry, session, dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn refused(&mut self, command: &str, params: Value) -> zen_core::DispatchError {
        self.registry.dispatch(&mut self.session, command, params).unwrap_err()
    }

    fn project(&self) -> PathBuf {
        self.dir.path().join("project")
    }

    fn add(&mut self, name: &str, driver: &str) -> String {
        let added = self.run("db.dataSource.add", json!({ "name": name, "driver": driver, "password": "pw" }));
        added["id"].as_str().unwrap().to_owned()
    }

    fn open(&mut self, relative: &str) -> PathBuf {
        let path = self.project().join(relative);
        self.run("file.document.open", json!({ "path": path }));
        path
    }
}

fn code(error: &zen_core::DispatchError) -> (Option<String>, Value) {
    match error {
        zen_core::DispatchError::Failed { error, .. } => {
            (error.code.clone(), Value::Object(error.data.clone().unwrap_or_default()))
        }
        other => panic!("{other}"),
    }
}

fn bindings_file(window: &Window) -> PathBuf {
    let key = super::super::project_key(&window.project());
    window.dir.path().join("data/databases/scripts").join(format!("{key}.json"))
}

fn wait(window: &mut Window, started: &Value) -> Value {
    window.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }))
}

fn active(window: &Window) -> PathBuf {
    window.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned()
}

#[test]
fn a_project_script_is_bound_per_project_outside_the_file_and_comes_back() {
    let mut window = Window::new();
    let kv = window.add("Cache", zen_db::memory::ID);
    let shop = window.add("Shop", zen_db::memory::SQL);
    let path = window.open("queries/report.sql");
    let state = window.run("db.script.state", json!({}));
    assert_eq!((state["bound"].as_bool(), state["console"].as_bool()), (Some(false), Some(false)), "{state}");
    assert_eq!(state["dialect"], "standard");

    let bound = window.run("db.script.bind", json!({ "connection": shop, "schema": "sales" }));
    assert_eq!((bound["connection"].as_str(), bound["schema"].as_str()), (Some(shop.as_str()), Some("sales")));
    let state = window.run("db.script.state", json!({ "path": "queries/report.sql" }));
    assert_eq!(state["connectionName"], "Shop");
    assert_eq!(state["dialect"], "postgres", "the dialect follows the connection");
    assert_eq!(state["inWorkspace"], false);
    let moved = window.run("db.script.bind", json!({ "connection": kv }));
    assert!(moved.get("schema").is_none(), "another connection forgets the schema: {moved}");
    assert_eq!(window.run("db.script.state", json!({}))["dialect"], "lines");
    window.run("db.script.bind", json!({ "schema": "0" }));

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "SET a 1\nGET a\n", "never written into the file");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(bindings_file(&window)).unwrap()).unwrap();
    assert_eq!(kept, json!({ "scripts": { "queries/report.sql": { "connection": kv, "schema": "0" } } }));
    assert!(!window.project().join(".zen").exists(), "nothing in the repository");

    window.session.set_state(Bindings::default());
    let state = window.run("db.script.state", json!({ "path": path }));
    assert_eq!((state["connection"].as_str(), state["schema"].as_str()), (Some(kv.as_str()), Some("0")), "restored");

    let unbound = window.run("db.script.bind", json!({ "connection": null }));
    assert!(unbound.get("connection").is_none(), "{unbound}");
    assert_eq!(window.run("db.script.state", json!({}))["bound"], false);
    let refused = window.refused("db.script.bind", json!({ "schema": "public" }));
    assert_eq!(code(&refused).0.as_deref(), Some("noBinding"));
    let refused = window.refused("db.script.bind", json!({ "connection": "nope" }));
    assert_eq!(code(&refused).0.as_deref(), Some("unknownConnection"));
}

#[test]
fn a_script_never_runs_without_a_binding_and_runs_into_the_workspace_with_one() {
    let mut window = Window::new();
    let kv = window.add("Cache", zen_db::memory::ID);
    let path = window.open("queries/report.sql");
    let refused = window.refused("db.script.run", json!({}));
    let (code_of, data) = code(&refused);
    assert_eq!(code_of.as_deref(), Some("noBinding"), "the only connection is not taken by itself");
    assert_eq!(data["connections"], json!([{ "id": kv, "name": "Cache" }]));
    assert_eq!(code(&window.refused("db.console.execute", json!({}))).0.as_deref(), Some("noBinding"));
    let engine = window.session.state::<super::super::Database>().and_then(super::super::Database::started);
    assert!(
        engine.is_none_or(|engine| engine.status(&kv).state == zen_db::engine::ConnectionState::Disconnected),
        "a refused run connects nowhere"
    );

    window.run("db.script.bind", json!({ "connection": kv }));
    let targets = window.run("db.script.targets", json!({}));
    assert_eq!(
        targets["targets"],
        json!([
            { "line": 1, "offset": 0, "label": "SET a 1" },
            { "line": 2, "offset": 8, "label": "GET a" },
        ])
    );
    let started = window.run("db.script.run", json!({ "offset": 8 }));
    assert_eq!(started["connection"], kv.as_str());
    assert_eq!(started["statements"], 1);
    assert_eq!(started["tab"]["path"], json!(path));
    let host = window.run("workspace.host.state", json!({ "id": super::WORKSPACE }));
    assert_eq!(host["expanded"], true, "the results show in the Database workspace");
    assert_eq!(host["active"], started["tab"]["id"]);
    assert_eq!(active(&window), path);
    assert_eq!(wait(&mut window, &started)["status"], "done");
    let state = window.run("db.console.state", json!({ "path": path }));
    assert_eq!(state["last"]["statements"], 1, "only the statement at the offset: {state}");
    assert_eq!(state["panel"]["shown"], true);
    assert_eq!(window.run("db.script.state", json!({}))["inWorkspace"], true);

    let started = window.run("db.script.run", json!({ "path": "queries/report.sql", "scope": "script" }));
    assert_eq!(started["statements"], 2);
    assert_eq!(wait(&mut window, &started)["status"], "done");
    let consoles = window.run("db.console.state", json!({ "path": path }));
    assert_eq!(consoles["connection"], kv.as_str());
}

#[test]
fn statements_split_in_the_bound_connections_dialect() {
    let mut window = Window::new();
    let shop = window.add("Shop", zen_db::memory::SQL);
    window.open("schema.sql");
    let standard = window.run("db.script.targets", json!({}));
    assert_eq!(standard["total"], 3, "unbound, standard SQL ends a statement inside $$: {standard}");
    window.run("db.script.bind", json!({ "connection": shop }));
    let targets = window.run("db.script.targets", json!({ "limit": 1 }));
    assert_eq!(targets["total"], 2, "{targets}");
    assert_eq!(targets["truncated"], true);
    let label = "CREATE FUNCTION f() RETURNS int AS $$ SELECT 1; $$ LANGUAGE …";
    assert_eq!(targets["targets"], json!([{ "line": 1, "offset": 0, "label": label }]));
    let all = window.run("db.script.targets", json!({}));
    assert_eq!(all["targets"][1]["line"], 3);
    assert_eq!(all["targets"][1]["label"], "-- the second");
}

#[test]
fn scripts_are_listed_per_connection_and_new_ones_go_in_a_folder_of_the_project() {
    let mut window = Window::new();
    let kv = window.add("Cache", zen_db::memory::ID);
    let shop = window.add("Shop", zen_db::memory::SQL);
    window.open("queries/report.sql");
    window.run("db.script.bind", json!({ "connection": kv }));
    let created = window.run(
        "db.script.new",
        json!({ "connection": shop, "folder": "queries", "schema": "public", "text": "SELECT 1;" }),
    );
    let path = PathBuf::from(created["path"].as_str().unwrap());
    assert_eq!(path, window.project().join("queries/script.sql"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "SELECT 1;");
    assert_eq!(created["binding"]["schema"], "public");
    assert_eq!(active(&window), path, "it opens in the Database workspace");
    let second = window.run("db.script.new", json!({ "connection": shop, "folder": "queries" }));
    assert!(second["path"].as_str().unwrap().ends_with("queries/script_1.sql"), "{second}");
    let named =
        window.run("db.script.new", json!({ "connection": shop, "folder": "reports/monthly", "name": "sales" }));
    assert_eq!(PathBuf::from(named["path"].as_str().unwrap()), window.project().join("reports/monthly/sales.sql"));
    let console = window.run("db.console.new", json!({ "connection": kv }));
    let console = PathBuf::from(console["path"].as_str().unwrap());

    let listed = window.run("db.script.list", json!({ "connection": shop }));
    let places: Vec<&str> =
        listed["scripts"].as_array().unwrap().iter().map(|entry| entry["place"].as_str().unwrap()).collect();
    assert_eq!(places, ["queries/script.sql", "queries/script_1.sql", "reports/monthly/sales.sql"]);
    let cache = window.run("db.script.list", json!({ "connection": kv }));
    assert_eq!(cache["total"], 2);
    assert_eq!(cache["scripts"][0]["place"], "queries/report.sql");
    assert_eq!(cache["scripts"][1]["console"], true);
    assert_eq!(PathBuf::from(cache["scripts"][1]["path"].as_str().unwrap()), console);
    let all = window.run("db.script.list", json!({ "limit": 2 }));
    assert_eq!((all["total"].as_u64(), all["truncated"].as_bool()), (Some(5), Some(true)));

    for (folder, name) in
        [("../outside", None), (".git", None), ("queries", Some("../x")), ("queries", Some(".hidden"))]
    {
        let refused = window.refused("db.script.new", json!({ "connection": shop, "folder": folder, "name": name }));
        assert_eq!(code(&refused).0.as_deref(), Some("invalidPath"), "{folder} {name:?}");
    }
    assert!(!window.dir.path().join("outside").exists());
    assert_eq!(std::fs::read_dir(window.project().join(".git")).unwrap().count(), 0, ".git is never written");
    let exists = window.refused("db.script.new", json!({ "connection": shop, "folder": "queries", "name": "report" }));
    assert_eq!(code(&exists).0.as_deref(), Some("exists"));
    assert_eq!(std::fs::read_to_string(window.project().join("queries/report.sql")).unwrap(), "SET a 1\nGET a\n");
}

#[test]
fn a_console_is_bound_through_the_same_command_and_cannot_be_unbound() {
    let mut window = Window::new();
    let kv = window.add("Cache", zen_db::memory::ID);
    let shop = window.add("Shop", zen_db::memory::SQL);
    let console = window.run("db.console.new", json!({ "text": "GET a" }));
    let path = Path::new(console["path"].as_str().unwrap()).to_owned();
    let state = window.run("db.script.state", json!({}));
    assert_eq!(state["console"], true);
    assert_eq!(state["inWorkspace"], true);
    window.run("db.script.bind", json!({ "connection": shop, "schema": "public" }));
    let kept = window.run("db.console.state", json!({ "path": path }));
    assert_eq!((kept["connection"].as_str(), kept["schema"].as_str()), (Some(shop.as_str()), Some("public")));
    assert!(!bindings_file(&window).exists(), "a console keeps its binding with its tab");
    let refused = window.refused("db.script.bind", json!({ "connection": null }));
    assert_eq!(code(&refused).0.as_deref(), Some("console"));
    window.run("db.script.bind", json!({ "connection": kv }));
    assert_eq!(window.run("db.script.state", json!({}))["dialect"], "lines");
}

#[test]
fn completion_works_in_a_bound_project_script_only() {
    let mut window = Window::new();
    let shop = window.add("Shop", zen_db::memory::SQL);
    let path = window.open("select.sql");
    assert!(!super::super::assist::is_bound(&window.session, &path), "an unbound script gets no database completion");
    window.run("db.script.bind", json!({ "connection": shop }));
    assert!(super::super::assist::is_bound(&window.session, &path));
    window.run("editor.selection.set", json!({ "selections": [{ "head": 3 }] }));
    let answer = window.run("db.sql.complete", json!({ "path": path, "offset": 3 }));
    assert_eq!(answer["connection"], shop.as_str());
    assert_eq!(answer["items"][0]["label"], "SELECT", "{answer}");
}

#[test]
fn an_agent_creates_a_script_in_a_project_that_holds_a_hidden_file() {
    let mut window = Window::new();
    let shop = window.add("Shop", zen_db::memory::SQL);
    std::fs::write(window.project().join(".env"), "SECRET=hunter2\n").unwrap();
    window
        .session
        .state_mut::<crate::hidden_files::HiddenFromAgents>()
        .set(&[".env".to_owned(), "*.secret.sql".to_owned()]);
    let mut as_agent = |command: &str, params: Value| {
        zen_core::macros::as_agent(&mut window.session, |session| window.registry.dispatch(session, command, params))
    };
    let created = as_agent("db.script.new", json!({ "connection": shop })).unwrap();
    assert!(created["path"].as_str().unwrap().ends_with("script.sql"), "{created}");
    let refused = as_agent("db.script.new", json!({ "connection": shop, "name": "keys.secret" })).unwrap_err();
    assert!(format!("{refused:?}").contains("redacted"), "{refused:?}");
}

#[test]
fn a_target_offset_runs_its_own_statement_when_no_space_follows_the_delimiter() {
    let mut window = Window::new();
    let shop = window.add("Shop", zen_db::memory::SQL);
    std::fs::write(window.project().join("tight.sql"), "SELECT 1;SELECT 2;\n").unwrap();
    let path = window.open("tight.sql");
    window.run("db.script.bind", json!({ "connection": shop }));
    let targets = window.run("db.script.targets", json!({}));
    assert_eq!(targets["targets"][1]["offset"], 9, "{targets}");
    let started = window.run("db.script.run", json!({ "offset": 9 }));
    assert_eq!(wait(&mut window, &started)["status"], "done");
    let state = window.run("db.console.state", json!({ "path": path }));
    assert_eq!(state["ran"], json!([9, 17]), "the second statement, not the one before it: {state}");
}
