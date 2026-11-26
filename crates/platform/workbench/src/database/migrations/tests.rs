use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_run::RunEnv;

use super::super::tests::Window;
use super::*;

const SECRET: &str = "hunter2-secret";

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn project(window: &Window) -> PathBuf {
    window.dir.path().join("project")
}

fn executable(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn fake_migrate(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("migrate"),
        "#!/bin/sh\necho \"args: $*\"\n\
         if [ -n \"$PGPASSFILE\" ] && /usr/bin/grep -q hunter2-secret \"$PGPASSFILE\"; then echo password given; echo \"file: $PGPASSFILE\"; fi\n\
         if [ -n \"$PGPASSWORD\" ]; then echo password in the environment; fi\n\
         if [ -n \"$DATABASE_URL\" ]; then echo address in the environment; fi\n",
    );
    bin
}

fn code(window: &mut Window, command: &str, params: Value) -> (String, Value) {
    match window.registry.dispatch(&mut window.session, command, params) {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            (error.code.unwrap_or_default(), error.data.map(Value::Object).unwrap_or(Value::Null))
        }
        other => panic!("not refused: {other:?}"),
    }
}

fn listed(window: &mut Window) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let answer = window.run("db.migration.list", json!({}));
        if answer["scanning"] == false {
            return answer;
        }
        assert!(Instant::now() < deadline, "the scan ended: {answer}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn settle(window: &mut Window, operation: &Value) {
    let Some(operation) = operation.as_u64() else { return };
    let deadline = Instant::now() + Duration::from_secs(20);
    while window.run("db.operation.state", json!({ "operation": operation }))["status"] == "running" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn add(window: &mut Window, name: &str, kind: &str, port: u16) -> String {
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": name, "url": format!("postgresql://ann@127.0.0.1:{port}/shop?sslmode=disable"), "password": SECRET, "type": kind }),
    );
    settle(window, &added["secretOperation"]);
    added["id"].as_str().unwrap().to_owned()
}

fn run_of(window: &mut Window, set: &str, connection: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        super::super::connections::step(&mut window.session);
        let status = window.run("db.migration.status", json!({ "set": set, "connection": connection }));
        let run = &status["run"];
        if run["runId"].is_u64() && run["state"] != "running" && !run["state"].is_null() {
            super::super::connections::step(&mut window.session);
            return window.run("db.migration.status", json!({ "set": set, "connection": connection }));
        }
        assert!(Instant::now() < deadline, "the run ended: {status}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn output(window: &mut Window, run: &Value) -> String {
    let answer = window.run("run.output.get", json!({ "runId": run["runId"] }));
    answer["lines"]
        .as_array()
        .map(|lines| lines.iter().filter_map(|line| line["text"].as_str()).collect::<Vec<_>>().join("\n"))
        .unwrap_or_else(|| answer.to_string())
}

fn golang_project(window: &Window) {
    let root = project(window);
    crate::trust::trust_in_tests(&crate::build_tools::BuildEnv::global(), &root);
    write(&root, "go.mod", "module shop\nrequire github.com/golang-migrate/migrate/v4 v4.18.1\n");
    write(&root, "db/migrations/000001_users.up.sql", "CREATE TABLE users (id int PRIMARY KEY);\n");
    write(&root, "db/migrations/000001_users.down.sql", "DROP TABLE users;\n");
    write(&root, "db/migrations/000002_orders.up.sql", "CREATE TABLE orders (id int PRIMARY KEY);\n");
    write(&root, "db/migrations/000002_orders.down.sql", "DROP TABLE orders;\n");
    write(&root, "node_modules/x/migrations/V1__ignored.sql", "");
}

fn with_runs(window: &mut Window, folders: Vec<PathBuf>) {
    let data = window.dir.path().join("data");
    let provider = MigrationRun::in_folders(Some(data), folders);
    window.session.state_mut::<Runs>().set_providers(vec![Arc::new(provider)], RunEnv::default());
    window.session.state_mut::<Database>().set_drivers(zen_db::drivers());
}

#[test]
fn migrations_are_found_offline_and_run_in_the_run_window_with_the_password_kept_out_of_sight() {
    let mut window = Window::new();
    golang_project(&window);
    let bin = fake_migrate(window.dir.path());
    with_runs(&mut window, vec![bin]);
    let list = listed(&mut window);
    assert_eq!(list["total"], 1, "{list}");
    let set = list["sets"][0].clone();
    assert_eq!(
        (set["tool"].as_str(), set["folder"].as_str(), set["count"].as_u64()),
        (Some("golangMigrate"), Some("db/migrations"), Some(2))
    );
    assert_eq!(set["table"], "schema_migrations");
    let set = set["id"].as_str().unwrap().to_owned();

    let id = add(&mut window, "Shop", "dev", 1);
    let offline = window.run("db.migration.status", json!({ "set": set, "connection": id }));
    assert_eq!(offline["connected"], false);
    assert_eq!(offline["counts"]["unknown"], 2, "{offline}");
    assert_eq!(offline["migrations"][0]["file"], "db/migrations/000001_users.up.sql");
    let one = window.run("db.migration.status", json!({ "set": set, "limit": 1 }));
    assert_eq!((one["total"].as_u64(), one["truncated"].as_bool()), (Some(2), Some(true)));

    let (refused, data) = code(&mut window, "db.migration.run", json!({ "set": set, "connection": id }));
    assert_eq!(refused, "notConnected", "what runs is read from the applied state");
    assert_eq!(data["handOff"]["command"], "db.connection.connect");

    let root = project(&window);
    remember(&credential_key(&root, &id), Secret::new(SECRET.to_owned()));
    let mut options =
        json!({ "tool": "golangMigrate", "folder": "db/migrations", "connection": id, "direction": "up" });
    options["approval"] = json!(approve());
    let configuration = json!({ "name": "migrate", "kind": KIND, "options": options });
    let started =
        crate::run::start(&mut window.session, serde_json::from_value(configuration.clone()).unwrap()).unwrap();
    let ran = json!({ "runId": started.run_id });
    let deadline = Instant::now() + Duration::from_secs(20);
    while window.run("run.output.get", ran.clone())["state"] == "running" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(window.run("run.output.get", ran.clone())["exitCode"], 0);
    let text = output(&mut window, &ran);
    assert!(text.contains("password given") && !text.contains("password in the environment"), "{text}");
    assert!(
        text.contains("args: -path db/migrations -database postgres://ann@127.0.0.1:1/shop?sslmode=disable up"),
        "{text}"
    );
    let file = text.lines().find_map(|line| line.strip_prefix("file: ")).unwrap();
    assert!(!Path::new(file).exists(), "the password file goes when the tool ends");
    let again = crate::run::start(&mut window.session, serde_json::from_value(configuration).unwrap());
    assert!(format!("{again:?}").contains("Migrations tab"), "an approval runs once: {again:?}");
    let unapproved = window.refused(
        "run.process.start",
        json!({ "configuration": { "name": "migrate", "kind": KIND, "options": { "tool": "golangMigrate", "folder": "db/migrations", "connection": id, "direction": "down" } } }),
    );
    assert!(unapproved.contains("Migrations tab"), "never past db.migration.run's checks: {unapproved}");

    let prod = add(&mut window, "Shop prod", "prod", 2);
    let (refused, _) = code(&mut window, "db.migration.run", json!({ "set": set, "connection": prod }));
    assert_eq!(refused, "notConnected", "closed: nothing to show of what would run");
    let registry = &window.registry;
    let (refused, data) = zen_core::macros::as_agent(&mut window.session, |session| {
        match registry.dispatch(session, "db.migration.run", json!({ "set": set, "connection": prod })) {
            Err(zen_core::DispatchError::Failed { error, .. }) => (error.code.unwrap_or_default(), error.data),
            other => panic!("{other:?}"),
        }
    });
    assert_eq!(refused, AGENT_READ_ONLY);
    assert_eq!(data.unwrap()["handOff"]["command"], "db.migration.open");
    assert_eq!(window.run("db.migration.status", json!({ "set": set, "connection": id }))["readOnly"], false);
    window.run("db.connection.readOnly", json!({ "connection": id, "readOnly": true }));
    assert_eq!(window.run("db.migration.status", json!({ "set": set, "connection": id }))["readOnly"], true);
    let (refused, _) = code(&mut window, "db.migration.run", json!({ "set": set, "connection": id }));
    assert_eq!(refused, READ_ONLY);
    let (refused, _) =
        code(&mut window, "db.migration.run", json!({ "set": set, "connection": id, "direction": "down" }));
    assert_eq!(refused, READ_ONLY);

    let (refused, _) = code(&mut window, "db.migration.run", json!({ "set": "nope", "connection": id }));
    assert_eq!(refused, UNKNOWN_SET);
    let tab = window.run("db.migration.open", json!({ "connection": prod, "set": set }));
    assert_eq!((tab["kind"].as_str(), tab["data"]["set"].as_str()), (Some(TAB), Some(set.as_str())));
    let status = window.run("db.connection.status", json!({}));
    assert!(
        status["connections"].as_array().unwrap().iter().all(|connection| connection["state"] == "disconnected"),
        "nothing connected: {status}"
    );
}

#[test]
fn an_agent_runs_no_migration_in_a_project_nobody_trusted() {
    let mut window = Window::new();
    golang_project(&window);
    let bin = fake_migrate(window.dir.path());
    with_runs(&mut window, vec![bin]);
    let set = listed(&mut window)["sets"][0]["id"].as_str().unwrap().to_owned();
    let id = add(&mut window, "Shop", "dev", 1);
    window.run("project.trust.revoke", json!({}));
    let registry = &window.registry;
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        registry.dispatch(session, "db.migration.run", json!({ "set": set, "connection": id }))
    });
    assert!(format!("{refused:?}").contains(crate::trust::NOT_TRUSTED), "{refused:?}");
    let (person, _) = code(&mut window, "db.migration.run", json!({ "set": set, "connection": id }));
    assert_eq!(person, "notConnected", "the person's run goes on to its own checks");
}

#[test]
fn a_saved_configuration_without_the_password_read_asks_for_the_tab_and_ssh_is_refused() {
    let mut window = Window::new();
    golang_project(&window);
    let bin = fake_migrate(window.dir.path());
    with_runs(&mut window, vec![bin]);
    let set = listed(&mut window)["sets"][0]["id"].as_str().unwrap().to_owned();
    let id = add(&mut window, "Unread", "dev", 3);
    let configuration = json!({
        "name": "migrate",
        "kind": KIND,
        "options": { "tool": "golangMigrate", "folder": "db/migrations", "connection": id, "direction": "up" }
    });
    let refused = window.refused("run.process.start", json!({ "configuration": configuration }));
    assert!(refused.contains("Migrations tab"), "{refused}");
    let tunnel = window.run(
        "db.dataSource.add",
        json!({ "name": "Tunnel", "url": "postgresql://ann@10.0.0.5/shop", "ssh": { "host": "bastion" } }),
    );
    let (refused, _) =
        code(&mut window, "db.migration.run", json!({ "set": set, "connection": tunnel["id"], "password": "x" }));
    assert_eq!(refused, UNSUPPORTED);
}

#[test]
fn changed_migration_files_scan_again() {
    let mut window = Window::new();
    golang_project(&window);
    let first = listed(&mut window);
    assert_eq!(first["sets"][0]["count"], 2);
    let root = project(&window);
    write(&root, "db/migrations/000003_items.up.sql", "");
    files_changed(&mut window.session, &[root.join("db/migrations/000003_items.up.sql")]);
    assert_eq!(listed(&mut window)["sets"][0]["count"], 3);
    files_changed(&mut window.session, &[root.join("main.go")]);
    assert!(!window.session.state::<Migrations>().unwrap().stale, "an unrelated file does not scan again");
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md) and psql"]
fn migrations_run_against_postgresql_and_the_structure_and_state_are_read_again() {
    let port: u16 = std::env::var("ZEN_DB_PG_PORTS")
        .ok()
        .and_then(|ports| ports.split(',').filter_map(|port| port.trim().parse().ok()).next_back())
        .unwrap_or(54318);
    let mut window = Window::new();
    golang_project(&window);
    let bin = window.dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("migrate"), PSQL_MIGRATE);
    let mut folders = vec![bin];
    folders.extend(zen_lsp::servers::search_folders());
    with_runs(&mut window, folders);
    let set = listed(&mut window)["sets"][0]["id"].as_str().unwrap().to_owned();
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "PG", "url": format!("postgresql://postgres@127.0.0.1:{port}/postgres?sslmode=disable"), "password": "zen" }),
    );
    settle(&mut window, &added["secretOperation"]);
    let id = added["id"].as_str().unwrap().to_owned();
    let connecting = window.run("db.connection.connect", json!({ "connection": id }));
    settle(&mut window, &connecting["operation"]);
    let engine = window.session.state::<Database>().unwrap().started().unwrap().clone();
    let clean = || {
        let op = engine
            .execute(
                &id,
                ContextKind::Console("c".into()),
                "DROP TABLE IF EXISTS users, orders, schema_migrations",
                ExecOptions::default(),
            )
            .unwrap();
        engine.wait(op, Duration::from_secs(10)).unwrap();
    };
    clean();
    let read = |window: &mut Window| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            super::super::connections::step(&mut window.session);
            let status = window.run("db.migration.status", json!({ "set": set, "connection": id }));
            if status["loading"] == false && !status["table"].is_null() {
                return status;
            }
            assert!(Instant::now() < deadline, "{status}");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let before = read(&mut window);
    assert_eq!((before["table"].as_bool(), before["counts"]["pending"].as_u64()), (Some(false), Some(2)), "{before}");

    window.run("db.migration.run", json!({ "set": set, "connection": id, "direction": "up" }));
    let ran = run_of(&mut window, &set, &id);
    assert_eq!(ran["run"]["exitCode"], 0, "{}", output(&mut window, &ran["run"]));
    assert!(ran["run"]["refresh"].is_u64(), "the structure is read again: {ran}");
    let after = read(&mut window);
    assert_eq!(after["counts"]["applied"], 2, "{after}");
    let until_tables = |window: &mut Window, wanted: bool| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            super::super::connections::step(&mut window.session);
            let answer =
                window.run("db.schema.children", json!({ "connection": id, "path": ["postgres", "public", "tables"] }));
            let has =
                answer["nodes"].as_array().is_some_and(|nodes| nodes.iter().any(|node| node["label"] == "orders"));
            if has == wanted {
                return;
            }
            assert!(Instant::now() < deadline, "{answer}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    until_tables(&mut window, true);

    window.run_confirmed("db.migration.run", json!({ "set": set, "connection": id, "direction": "down" }));
    let reverted = run_of(&mut window, &set, &id);
    assert_eq!(reverted["run"]["exitCode"], 0, "{}", output(&mut window, &reverted["run"]));
    let state = read(&mut window);
    let states: Vec<&str> =
        state["migrations"].as_array().unwrap().iter().map(|status| status["state"].as_str().unwrap()).collect();
    assert_eq!(states, ["applied", "pending"], "{state}");
    until_tables(&mut window, false);
    clean();
    window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
}

const PSQL_MIGRATE: &str = r#"#!/bin/sh
set -e
dir="$2"; url="$4"; action="$5"
psql -X -q -v ON_ERROR_STOP=1 "$url" -c "CREATE TABLE IF NOT EXISTS schema_migrations (version bigint PRIMARY KEY, dirty boolean NOT NULL)"
current=$(psql -X -At "$url" -c "SELECT coalesce(max(version), 0) FROM schema_migrations")
if [ "$action" = up ]; then
  for file in "$dir"/*.up.sql; do
    version=$(basename "$file" | sed 's/_.*//' | sed 's/^0*//')
    if [ "$version" -gt "$current" ]; then
      psql -X -q -v ON_ERROR_STOP=1 "$url" -f "$file"
      psql -X -q "$url" -c "DELETE FROM schema_migrations; INSERT INTO schema_migrations VALUES ($version, false)"
      echo "applied $version"
    fi
  done
else
  file=$(ls "$dir"/*.down.sql | awk -v want="$current" '{ n=$0; sub(/.*\//, "", n); sub(/_.*/, "", n); if (n+0 == want) print $0 }')
  psql -X -q -v ON_ERROR_STOP=1 "$url" -f "$file"
  previous=$((current - 1))
  psql -X -q "$url" -c "DELETE FROM schema_migrations; INSERT INTO schema_migrations SELECT $previous, false WHERE $previous > 0"
  echo "reverted $current"
fi
"#;

fn a_run(window: &mut Window, set_env: Vec<(String, String)>) -> Value {
    let id = add(window, "Shop", "dev", 1);
    let root = project(window);
    remember(&credential_key(&root, &id), Secret::new(SECRET.to_owned()));
    let mut options =
        json!({ "tool": "golangMigrate", "folder": "db/migrations", "connection": id, "direction": "up" });
    options["approval"] = json!(approve());
    json!({ "name": "migrate", "kind": KIND, "common": { "env": set_env }, "options": options })
}

#[test]
fn a_program_the_project_brings_never_runs_as_a_migration_tool() {
    let mut window = Window::new();
    golang_project(&window);
    let root = project(&window);
    let bin = fake_migrate(&root);
    with_runs(&mut window, vec![bin]);
    let configuration = a_run(&mut window, Vec::new());
    let started = crate::run::start(&mut window.session, serde_json::from_value(configuration).unwrap());
    assert!(format!("{started:?}").contains("migrate"), "the project's own program ran: {started:?}");
    assert!(started.is_err());
}

#[test]
fn a_password_in_the_configurations_environment_never_reaches_the_tool() {
    let mut window = Window::new();
    golang_project(&window);
    let bin = fake_migrate(window.dir.path());
    with_runs(&mut window, vec![bin]);
    let env = vec![("DATABASE_URL".to_owned(), format!("postgres://ann:{SECRET}@db/shop"))];
    let configuration = a_run(&mut window, env);
    let started = crate::run::start(&mut window.session, serde_json::from_value(configuration).unwrap()).unwrap();
    let ran = json!({ "runId": started.run_id });
    let deadline = Instant::now() + Duration::from_secs(20);
    while window.run("run.output.get", ran.clone())["state"] == "running" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let text = output(&mut window, &ran);
    assert!(text.contains("password given") && !text.contains("address in the environment"), "{text}");
}
