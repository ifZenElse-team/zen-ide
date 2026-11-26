use serde_json::{Value, json};

use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::engine::OperationStatus;
use zen_db::schema::{IntrospectRequest, LARGE_SCHEMA, Snapshot};

use super::super::tests::{Window, sql_connection};

fn open(window: &mut Window, id: &str) -> String {
    let opened = window.run(
        "db.object.open",
        json!({ "connection": id, "path": ["shop", "public", "tables", "customers"], "page": "properties" }),
    );
    opened["tab"]["id"].as_str().unwrap().to_owned()
}

fn stage(window: &mut Window, tab: &str, change: Value) -> Value {
    window.run("db.structure.stage", json!({ "tab": tab, "change": change }))
}

fn code(window: &mut Window, command: &str, params: Value) -> (String, Value) {
    match window.registry.dispatch(&mut window.session, command, params) {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            (error.code.unwrap_or_default(), error.data.map(Value::Object).unwrap_or(Value::Null))
        }
        other => panic!("not refused: {other:?}"),
    }
}

fn until_ran(window: &mut Window, tab: &str) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        super::super::connections::step(&mut window.session);
        let state = window.run("db.structure.state", json!({ "tab": tab }));
        if state["running"].is_null() && !state["last"].is_null() {
            return state;
        }
        assert!(std::time::Instant::now() < deadline, "the script ended: {state}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn add_email() -> Value {
    json!({ "action": "addColumn", "schema": "public", "table": "customers",
            "column": { "name": "email", "type": "varchar(200)", "notNull": true, "default": "''" } })
}

#[test]
fn changes_accumulate_preview_in_the_dialect_and_run_in_one_transaction() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let tab = open(&mut window, &id);
    stage(&mut window, &tab, add_email());
    stage(
        &mut window,
        &tab,
        json!({ "action": "alterColumn", "schema": "public", "table": "customers", "column": "email",
                "patch": { "name": "mail" } }),
    );
    let state = stage(
        &mut window,
        &tab,
        json!({ "action": "dropColumn", "schema": "public", "table": "customers", "column": "tags" }),
    );
    assert_eq!(state["total"], 3);
    assert_eq!(state["dirty"], true);
    assert_eq!(state["changes"][0]["summary"], "Add column email varchar(200) to customers");
    assert_eq!(state["supports"]["sequences"], true);
    let preview = window.run("db.structure.preview", json!({ "tab": tab }));
    assert_eq!(preview["transaction"], true);
    assert_eq!(
        preview["statements"],
        json!([
            "BEGIN",
            "ALTER TABLE \"public\".\"customers\" ADD COLUMN \"email\" varchar(200) NOT NULL DEFAULT ''",
            "ALTER TABLE \"public\".\"customers\" RENAME COLUMN \"email\" TO \"mail\"",
            "ALTER TABLE \"public\".\"customers\" DROP COLUMN \"tags\"",
            "COMMIT",
        ])
    );
    assert!(preview["sql"].as_str().unwrap().contains("DROP COLUMN \"tags\";\n\nCOMMIT;\n"));
    let (refused, data) = code(&mut window, "db.structure.execute", json!({ "tab": tab }));
    assert_eq!(refused, "confirmExecute", "dropping a column asks on every connection type");
    assert_eq!(data["statements"][0]["index"], 3);
    assert!(driver.server.executed().iter().all(|statement| !statement.contains("ALTER")), "nothing ran yet");
    let token = data["token"].clone();
    let started = window.run("db.structure.execute", json!({ "tab": tab, "confirmed": token }));
    assert_eq!(started["statements"], 5);
    let state = until_ran(&mut window, &tab);
    assert_eq!(state["last"]["status"], "done", "{state}");
    assert_eq!(state["total"], 0, "the changes are cleared once they ran");
    let executed = driver.server.executed();
    let at = executed.iter().position(|statement| statement == "BEGIN").expect("began");
    assert_eq!(executed[at + 3], "ALTER TABLE \"public\".\"customers\" DROP COLUMN \"tags\"");
    assert_eq!(executed[at + 4], "COMMIT");
    let journal = super::super::history::journal(&window.session).expect("history");
    assert!(journal.flush(std::time::Duration::from_secs(10)));
    let history = window.run("db.history.list", json!({ "sources": ["structure"], "limit": 20 }));
    assert!(history["total"].as_u64().unwrap() >= 5, "{history}");
}

#[test]
fn what_cannot_be_done_is_refused_when_staged_and_changes_can_be_dropped() {
    let mut window = Window::new();
    let (id, _) = sql_connection(&mut window, "dev", true);
    let tab = open(&mut window, &id);
    let (refused, data) = code(
        &mut window,
        "db.structure.stage",
        json!({ "tab": tab, "change": { "action": "dropColumn", "schema": "public", "table": "customers", "column": "nope" } }),
    );
    assert_eq!((refused.as_str(), data["refusal"]["reason"].as_str()), ("unsupported", Some("unknownColumn")));
    stage(&mut window, &tab, add_email());
    let (refused, _) = code(&mut window, "db.structure.stage", json!({ "tab": tab, "change": add_email() }));
    assert_eq!(refused, "unsupported", "the second email column already exists after the first");
    stage(
        &mut window,
        &tab,
        json!({ "action": "createIndex", "schema": "public", "table": "customers", "index": { "columns": ["email"] } }),
    );
    let (refused, _) = code(&mut window, "db.structure.unstage", json!({ "tab": tab, "index": 0 }));
    assert_eq!(refused, "unsupported", "the index needs the column staged before it");
    let state = window.run("db.structure.unstage", json!({ "tab": tab, "index": 1 }));
    assert_eq!(state["total"], 1);
    let (refused, data) = code(&mut window, "workspace.tab.close", json!({ "workspace": "database", "tab": tab }));
    assert_eq!(refused, super::PENDING_STRUCTURE);
    assert_eq!(data["retry"]["command"], "workspace.tab.close");
    let state = window.run("db.structure.discard", json!({ "tab": tab }));
    assert_eq!(state["dirty"], false);
    window.run("workspace.tab.close", json!({ "workspace": "database", "tab": tab }));
}

#[test]
fn the_script_goes_to_a_console_or_a_new_migration_file_inside_the_project() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let tab = open(&mut window, &id);
    let (refused, _) = code(&mut window, "db.structure.saveMigration", json!({ "tab": tab }));
    assert_eq!(refused, "nothingStaged");
    stage(&mut window, &tab, add_email());
    let saved = window.run("db.structure.saveMigration", json!({ "tab": tab }));
    let path = std::path::PathBuf::from(saved["path"].as_str().unwrap());
    let project = window.dir.path().join("project/migrations");
    assert_eq!(path.parent(), Some(project.as_path()));
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with("_customers_add_column_email_varchar_200_to_customers.sql"), "{name}");
    assert_eq!(name.find('_'), Some(14), "a UTC time stamp first: {name}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("ADD COLUMN \"email\" varchar(200) NOT NULL DEFAULT '';\n"), "{text}");
    let again = window.run("db.structure.saveMigration", json!({ "tab": tab, "name": "add email" }));
    assert_ne!(again["path"], saved["path"]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "an existing file is never written over");
    for folder in ["../outside", "/tmp", ""] {
        let (refused, _) = code(&mut window, "db.structure.saveMigration", json!({ "tab": tab, "folder": folder }));
        assert_eq!(refused, "invalidFolder", "{folder}");
    }
    #[cfg(unix)]
    {
        let outside = window.dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, window.dir.path().join("project/linked")).unwrap();
        let (refused, _) = code(&mut window, "db.structure.saveMigration", json!({ "tab": tab, "folder": "linked" }));
        assert_eq!(refused, "invalidFolder", "a link out of the project");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }
    let console = window.run("db.structure.toConsole", json!({ "tab": tab }));
    let console_text = std::fs::read_to_string(console["path"].as_str().unwrap()).unwrap();
    assert!(console_text.starts_with("BEGIN;\n"), "{console_text}");
    let state = window.run("db.structure.state", json!({ "tab": tab }));
    assert_eq!(state["total"], 1, "copying the script keeps the changes staged");
    assert!(driver.server.executed().iter().all(|statement| !statement.contains("ALTER")), "nothing ran");
}

#[test]
fn read_only_connections_and_agents_on_production_never_change_structure() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "prod", true);
    let tab = open(&mut window, &id);
    stage(&mut window, &tab, add_email());
    let preview = window.run("db.structure.preview", json!({ "tab": tab }));
    assert_eq!(preview["confirm"], true);
    assert_eq!(preview["transaction"], false, "production commits by hand");
    assert!(preview["notes"][0].as_str().unwrap().contains("manual commit"), "{preview}");
    let registry = &window.registry;
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        registry.dispatch(session, "db.structure.execute", json!({ "tab": tab }))
    });
    let Err(zen_core::DispatchError::Failed { error, .. }) = refused else { panic!("not refused: {refused:?}") };
    assert_eq!(error.code.as_deref(), Some("agentReadOnly"));
    assert_eq!(error.data.unwrap()["handOff"]["command"], "db.console.new");
    window.run("db.connection.readOnly", json!({ "connection": id, "readOnly": true }));
    let (refused, _) = code(&mut window, "db.structure.execute", json!({ "tab": tab }));
    assert_eq!(refused, "readOnly");
    let (refused, _) = code(&mut window, "db.structure.stage", json!({ "tab": tab, "change": add_email() }));
    assert_eq!(refused, "readOnly");
    assert!(driver.server.executed().iter().all(|statement| !statement.contains("ALTER")), "nothing ran");
}

fn sqlite_shop(window: &mut Window) -> Option<(String, String)> {
    zen_db::sqlite::process::find_program(None).ok()?;
    let drivers = zen_db::drivers();
    window.session.state_mut::<super::super::Database>().set_drivers(drivers);
    let file = window.dir.path().join("shop.db");
    std::fs::File::create(&file).unwrap();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Lite", "driver": zen_db::sqlite::ID, "database": file.display().to_string(), "type": "dev" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let started = window.run("db.connection.connect", json!({ "connection": id }));
    assert_eq!(super::super::tests::wait(window, &started["operation"])["status"], "done");
    for sql in [
        "CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT)",
        "CREATE TABLE child (id INTEGER PRIMARY KEY, parent INTEGER REFERENCES parent (id) ON DELETE CASCADE)",
        "INSERT INTO parent VALUES (1, 'a'), (2, 'b')",
        "INSERT INTO child VALUES (10, 1), (20, 2)",
        "PRAGMA foreign_keys = ON",
    ] {
        assert_eq!(sql_of(window, &id, sql), Vec::<Vec<zen_db::value::Value>>::new(), "{sql}");
    }
    let engine = super::super::consoles::engine(&mut window.session);
    let request = IntrospectRequest { force: true, large_schema: LARGE_SCHEMA, ..IntrospectRequest::default() };
    let op = engine.introspect(&id, request, Snapshot::new(&id, zen_db::sqlite::ID), None).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while engine.operation(op).is_some_and(|state| state.status == OperationStatus::Running) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let snapshot = engine.take_snapshot(op).unwrap();
    let catalog = snapshot.catalogs[0].name.clone();
    let path = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&path, &snapshot).unwrap();
    Some((id, catalog))
}

fn sql_of(window: &mut Window, id: &str, sql: &str) -> Vec<Vec<zen_db::value::Value>> {
    let engine = super::super::consoles::engine(&mut window.session);
    let op = engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap();
    let state = engine.wait(op, std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{sql}: {state:?}");
    state.execution.map(|execution| execution.rows).unwrap_or_default()
}

#[test]
fn a_sqlite_rebuild_keeps_the_rows_that_refer_to_the_table_and_runs_whole() {
    let mut window = Window::new();
    let Some((id, catalog)) = sqlite_shop(&mut window) else { return };
    let opened = window.run(
        "db.object.open",
        json!({ "connection": id, "path": [catalog, "main", "tables", "parent"], "page": "properties" }),
    );
    let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
    let not_null = json!({ "action": "alterColumn", "schema": "main", "table": "parent", "column": "name",
                           "patch": { "notNull": true, "default": "''" } });
    stage(&mut window, &tab, not_null);
    let preview = window.run("db.structure.preview", json!({ "tab": tab }));
    assert!(preview["sql"].as_str().unwrap().contains("PRAGMA foreign_keys = OFF"), "{preview}");
    assert!(preview["notes"].to_string().contains("switches them on again"), "{preview}");

    let saved = window.run("db.structure.saveMigration", json!({ "tab": tab }));
    let text = std::fs::read_to_string(saved["path"].as_str().unwrap()).unwrap();
    assert!(text.ends_with("COMMIT;\n-- PRAGMA foreign_keys = ON;\n"), "{text}");
    let (skipped, _) = code(&mut window, "db.structure.execute", json!({ "tab": tab, "skip": [0] }));
    assert_eq!(skipped, "allOrNothing");
    window.run("db.transaction.mode", json!({ "connection": id, "context": { "kind": "main" }, "mode": "manual" }));
    let (manual, _) = code(&mut window, "db.structure.execute", json!({ "tab": tab }));
    assert_eq!(manual, "rebuildInTransaction", "SQLite cannot switch foreign keys off in a transaction");
    window.run("db.transaction.mode", json!({ "connection": id, "context": { "kind": "main" }, "mode": "auto" }));

    let (asked, data) = code(&mut window, "db.structure.execute", json!({ "tab": tab }));
    assert_eq!(asked, "confirmExecute", "the rebuild drops the old table: {data}");
    assert_eq!(data["skippable"], false, "the script runs whole");
    window.run("db.structure.execute", json!({ "tab": tab, "confirmed": data["token"] }));
    let state = until_ran(&mut window, &tab);
    assert_eq!(state["last"]["status"], "done", "{state}");
    use zen_db::value::Value::Integer;
    assert_eq!(sql_of(&mut window, &id, "SELECT count(*) FROM child"), vec![vec![Integer(2)]], "no row cascaded");
    assert_eq!(sql_of(&mut window, &id, "PRAGMA foreign_keys"), vec![vec![Integer(1)]], "switched on again");
}

fn until_outcome(window: &mut Window, id: &str) -> super::Outcome {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let key = super::navigator_key(id);
    loop {
        super::super::connections::step(&mut window.session);
        super::step(&mut window.session);
        if let Some(outcome) = window.session.state::<super::Alterations>().and_then(|known| known.outcome(&key)) {
            return outcome.clone();
        }
        assert!(std::time::Instant::now() < deadline, "the change ended");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn customers_path() -> Value {
    json!(["shop", "public", "tables", "customers"])
}

#[test]
fn the_navigator_drops_an_object_at_once_after_naming_what_is_lost() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let tab = open(&mut window, &id);
    let (refused, _) =
        code(&mut window, "db.object.drop", json!({ "connection": id, "path": ["shop", "public", "tables"] }));
    assert_eq!(refused, "notDroppable");
    let (asked, data) = code(&mut window, "db.object.drop", json!({ "connection": id, "path": customers_path() }));
    assert_eq!(asked, "confirmExecute", "a drop always asks: {data}");
    let loss = data["loss"].as_str().unwrap();
    assert!(loss.starts_with("This deletes table public.customers."), "{loss}");
    assert!(loss.contains("columns (6)"), "{loss}");
    assert_eq!(data["retry"]["command"], "db.object.drop");
    assert!(driver.server.executed().iter().all(|statement| !statement.contains("DROP")), "nothing ran yet");
    let started =
        window.run("db.object.drop", json!({ "connection": id, "path": customers_path(), "confirmed": data["token"] }));
    assert!(started["query"].is_u64(), "{started}");
    let outcome = until_outcome(&mut window, &id);
    assert_eq!(outcome.status, OperationStatus::Done, "{outcome:?}");
    assert!(driver.server.executed().iter().any(|statement| statement == "DROP TABLE \"public\".\"customers\""));
    let tabs = window.run("workspace.host.state", json!({ "id": "database" }));
    assert!(!tabs.to_string().contains(&tab), "the dropped table's tab closed: {tabs}");
}

#[test]
fn the_navigator_renames_and_truncates_and_refuses_what_it_cannot() {
    let mut window = Window::new();
    let (id, driver) = sql_connection(&mut window, "dev", true);
    let (refused, _) =
        code(&mut window, "db.object.rename", json!({ "connection": id, "path": customers_path(), "to": " " }));
    assert_eq!(refused, "invalidName");
    let index = json!(["shop", "public", "tables", "customers", "indexes", "customers_pkey"]);
    let (refused, _) = code(&mut window, "db.object.rename", json!({ "connection": id, "path": index, "to": "x" }));
    assert_eq!(refused, "notRenamable");
    let column = json!(["shop", "public", "tables", "customers", "columns", "name"]);
    window.run("db.object.rename", json!({ "connection": id, "path": column, "to": "full_name" }));
    assert_eq!(until_outcome(&mut window, &id).status, OperationStatus::Done);
    let executed = driver.server.executed();
    assert!(
        executed.iter().any(
            |statement| statement == "ALTER TABLE \"public\".\"customers\" RENAME COLUMN \"name\" TO \"full_name\""
        )
    );

    let view = json!(["shop", "public", "views", "customers"]);
    let (refused, _) = code(&mut window, "db.object.truncate", json!({ "connection": id, "path": view }));
    assert_eq!(refused, "notTruncatable");
    let (asked, data) = code(&mut window, "db.object.truncate", json!({ "connection": id, "path": customers_path() }));
    assert_eq!(asked, "confirmExecute");
    assert!(data["loss"].as_str().unwrap().contains("every row of table public.customers"), "{data}");
    window.run("db.object.truncate", json!({ "connection": id, "path": customers_path(), "confirmed": data["token"] }));
    assert_eq!(until_outcome(&mut window, &id).status, OperationStatus::Done);
    assert!(driver.server.executed().iter().any(|statement| statement == "TRUNCATE TABLE \"public\".\"customers\""));

    window.run("db.dataSource.update", json!({ "id": id, "readOnly": true }));
    let (refused, _) = code(&mut window, "db.object.drop", json!({ "connection": id, "path": customers_path() }));
    assert_eq!(refused, "readOnly");
}

#[test]
fn create_opens_the_parent_tab_and_asks_the_window_for_its_form() {
    let mut window = Window::new();
    let (id, _) = sql_connection(&mut window, "dev", true);
    let asked = window
        .run("db.navigator.create", json!({ "connection": id, "path": ["shop", "public", "tables"], "kind": "table" }));
    let tab = asked["tab"].as_str().unwrap().to_owned();
    assert_eq!(tab, format!("object:{id}:shop/public"));
    let state = window.run("db.object.state", json!({ "tab": tab }));
    assert_eq!(state["page"], "properties");
    let request = window.session.state_mut::<super::CreateRequest>().0.take().unwrap();
    assert_eq!((request.tab.as_str(), request.kind), (tab.as_str(), super::CreateKind::Table));
    let asked =
        window.run("db.navigator.create", json!({ "connection": id, "path": customers_path(), "kind": "column" }));
    let state = window.run("db.object.state", json!({ "tab": asked["tab"] }));
    assert_eq!(state["section"], "columns", "{state}");
    let (refused, _) =
        code(&mut window, "db.navigator.create", json!({ "connection": id, "path": ["shop"], "kind": "view" }));
    assert_eq!(refused, "notCreatable");
}

#[test]
fn a_sqlite_table_is_emptied_and_dropped_from_the_navigator() {
    let mut window = Window::new();
    let Some((id, catalog)) = sqlite_shop(&mut window) else { return };
    let child = json!([catalog, "main", "tables", "child"]);
    let (asked, data) = code(&mut window, "db.object.truncate", json!({ "connection": id, "path": child }));
    assert_eq!(asked, "confirmExecute", "{data}");
    assert_eq!(data["statements"][0]["text"], "DELETE FROM \"main\".\"child\"");
    window.run("db.object.truncate", json!({ "connection": id, "path": child, "confirmed": data["token"] }));
    assert_eq!(until_outcome(&mut window, &id).status, OperationStatus::Done);
    use zen_db::value::Value::Integer;
    assert_eq!(sql_of(&mut window, &id, "SELECT count(*) FROM child"), vec![vec![Integer(0)]]);
    assert_eq!(sql_of(&mut window, &id, "SELECT count(*) FROM parent"), vec![vec![Integer(2)]], "only child");
    let (_, data) = code(&mut window, "db.object.drop", json!({ "connection": id, "path": child }));
    assert_eq!(data["cascade"], false, "SQLite cannot cascade");
    window.run("db.object.drop", json!({ "connection": id, "path": child, "confirmed": data["token"] }));
    assert_eq!(until_outcome(&mut window, &id).status, OperationStatus::Done);
    let left = sql_of(&mut window, &id, "SELECT count(*) FROM sqlite_master WHERE name = 'child'");
    assert_eq!(left, vec![vec![Integer(0)]]);
    assert_eq!(sql_of(&mut window, &id, "SELECT count(*) FROM parent"), vec![vec![Integer(2)]], "parent kept");
}
