use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::ClientFolders;
use crate::database::tests::{Window, sql_connection};
use crate::terminal::Terminals;

const PASSWORD: &str = "s3cr3t-Pa55";

fn fake_client(folder: &Path, name: &str) {
    let script = "#!/bin/sh\n\
        mode=$(stat -f %Lp \"$PGPASSFILE\" 2>/dev/null || stat -c %a \"$PGPASSFILE\" 2>/dev/null)\n\
        if [ -z \"$PGPASSWORD\" ] && [ \"$mode\" = 600 ] && grep -q ':s3cr3t-Pa55$' \"$PGPASSFILE\"; \
        then echo PASSWORD-OK; else echo PASSWORD-MISSING; fi\n\
        env | grep -c s3cr3t >/dev/null && echo PASSWORD-IN-ENV\n\
        echo \"FILE [$PGPASSFILE]\"\n\
        echo \"ARGS $*\"\n";
    let path = folder.join(name);
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn window_with_clients(client: bool) -> (Window, tempfile::TempDir) {
    let mut window = Window::new();
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::new(zen_db::postgres::Postgres)];
    window.session.state_mut::<super::Database>().set_drivers(drivers);
    let folder = tempfile::tempdir().unwrap();
    if client {
        fake_client(folder.path(), "psql");
    }
    window.session.set_state(ClientFolders(Some(vec![folder.path().to_owned()])));
    window
        .session
        .state_mut::<Terminals>()
        .set_launcher(Arc::new(|cwd: &Path| zen_terminal::Options::command("/bin/sh", &[], cwd)));
    (window, folder)
}

fn add(window: &mut Window, kind: &str, password: Option<&str>) -> String {
    let mut fields = json!({ "name": "Shop", "url": "postgresql://ann@localhost:5432/shop", "type": kind });
    if let Some(password) = password {
        fields["password"] = Value::from(password);
    }
    let added = window.run("db.dataSource.add", fields);
    if let Some(operation) = added["secretOperation"].as_u64() {
        wait(window, operation);
    }
    added["id"].as_str().unwrap().to_owned()
}

fn wait(window: &mut Window, operation: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let state = window.run("db.operation.state", json!({ "operation": operation }));
        if state["status"] != "running" || Instant::now() > deadline {
            return state;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn screen_until(window: &mut Window, id: u64, wanted: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let read = window.run("terminal.session.read", json!({ "id": id }));
        let text = read["text"].as_str().unwrap_or_default().to_owned();
        if text.contains(wanted) || Instant::now() > deadline {
            return text;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn the_client_gets_the_saved_password_only_through_its_environment() {
    let (mut window, _folder) = window_with_clients(true);
    let id = add(&mut window, "dev", Some(PASSWORD));
    let opened = window.run("db.connection.openTerminal", json!({ "connection": id }));
    assert_eq!(opened["passwordFrom"], "saved");
    assert_eq!(opened["tool"], "psql");
    assert!(opened["session"].is_null(), "the client waits for the password read in the background: {opened}");
    assert!(!opened.to_string().contains(PASSWORD));
    wait(&mut window, opened["operation"].as_u64().unwrap());
    super::step(&mut window.session);
    let list = window.run("terminal.session.list", json!({}));
    let tab = list["tabs"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(tab["name"], "psql: Shop");
    let session = tab["sessions"][0]["id"].as_u64().unwrap();
    let text = screen_until(&mut window, session, "ARGS");
    assert!(text.contains("PASSWORD-OK"), "{text}");
    assert!(text.contains("ARGS --host=localhost --port=5432 --username=ann --dbname=shop"), "{text}");
    assert!(!text.contains(PASSWORD), "the password is never on the screen: {text}");
    assert!(!text.contains("PASSWORD-IN-ENV"), "nor in the client's environment: {text}");
    let file = text.split("FILE [").nth(1).and_then(|rest| rest.split(']').next()).unwrap().to_owned();
    window.run("terminal.session.send", json!({ "id": session, "text": "echo \"[pw:$PGPASSWORD$PGPASSFILE]\"\n" }));
    let text = screen_until(&mut window, session, "[pw:]");
    assert!(text.contains("[pw:]"), "the shell after the client has no password: {text}");
    assert!(!Path::new(&file).exists(), "the password file is removed when the client ends: {file}");
    assert!(
        window
            .session
            .state::<Terminals>()
            .unwrap()
            .save()
            .iter()
            .all(|saved| saved.name.as_deref() != Some("psql: Shop")),
        "a client's tab is not saved with the window"
    );
}

#[cfg(unix)]
#[test]
fn a_password_given_once_starts_the_client_at_once() {
    let (mut window, _folder) = window_with_clients(true);
    let id = add(&mut window, "dev", None);
    let opened = window.run("db.connection.openTerminal", json!({ "connection": id, "password": PASSWORD }));
    assert_eq!(opened["passwordFrom"], "given");
    let session = opened["session"].as_u64().unwrap();
    let text = screen_until(&mut window, session, "ARGS");
    assert!(text.contains("PASSWORD-OK"), "{text}");
    let asked = window.run("db.connection.openTerminal", json!({ "connection": id }));
    assert_eq!(asked["passwordFrom"], "asked");
    let text = screen_until(&mut window, asked["session"].as_u64().unwrap(), "ARGS");
    assert!(text.contains("PASSWORD-MISSING"), "{text}");
}

#[test]
fn a_missing_client_is_refused_and_the_command_line_has_no_password() {
    let (mut window, _folder) = window_with_clients(false);
    let id = add(&mut window, "dev", Some(PASSWORD));
    let refused =
        window.registry.dispatch(&mut window.session, "db.connection.openTerminal", json!({ "connection": id }));
    match refused {
        Err(zen_core::DispatchError::Failed { error, .. }) => assert_eq!(error.code.as_deref(), Some("clientNotFound")),
        other => panic!("not refused: {other:?}"),
    }
    let line = window.run("db.connection.clientCommand", json!({ "connection": id }));
    assert_eq!(line["passwordVia"], "PGPASSFILE");
    assert!(line["program"].is_null());
    assert_eq!(line["commandLine"], "PGSSLMODE=prefer psql --host=localhost --port=5432 --username=ann --dbname=shop");
    assert!(!line.to_string().contains(PASSWORD));
}

#[cfg(unix)]
#[test]
fn agents_open_clients_only_with_full_access_and_never_with_a_saved_password() {
    let (mut window, _folder) = window_with_clients(true);
    let dev = add(&mut window, "dev", Some(PASSWORD));
    let prod = add(&mut window, "prod", Some(PASSWORD));
    let registry = &window.registry;
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        registry.dispatch(session, "db.connection.openTerminal", json!({ "connection": prod }))
    });
    match refused {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some("agentNotAllowed"))
        }
        other => panic!("not refused: {other:?}"),
    }
    let opened = zen_core::macros::as_agent(&mut window.session, |session| {
        registry.dispatch(session, "db.connection.openTerminal", json!({ "connection": dev }))
    })
    .unwrap();
    assert_eq!(opened["passwordFrom"], "asked");
    let text = screen_until(&mut window, opened["session"].as_u64().unwrap(), "ARGS");
    assert!(text.contains("PASSWORD-MISSING"), "{text}");
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Ro", "url": "postgresql://ann@localhost:5432/shop", "type": "dev", "readOnly": true }),
    );
    let read_only = added["id"].as_str().unwrap().to_owned();
    let registry = &window.registry;
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        registry.dispatch(session, "db.connection.openTerminal", json!({ "connection": read_only }))
    });
    match refused {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some("agentNotAllowed"), "a client could undo read-only: {error:?}")
        }
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn a_type_is_generated_from_a_table_for_each_language() {
    let mut window = Window::new();
    let (id, _driver) = sql_connection(&mut window, "dev", true);
    let path = json!(["shop", "public", "tables", "customers"]);
    let go = window.run("db.object.generateCode", json!({ "connection": id, "path": path, "language": "go" }));
    assert_eq!(go["typeName"], "Customer");
    assert_eq!(go["fields"], 6);
    let code = go["code"].as_str().unwrap();
    assert!(code.starts_with("type Customer struct {\n"), "{code}");
    assert!(code.contains("\tCreated *time.Time"), "{code}");
    let rust = window.run("db.object.generateCode", json!({ "connection": id, "path": path, "language": "rust" }));
    assert!(rust["code"].as_str().unwrap().contains("pub tags: Option<Vec<String>>,"), "{rust}");
    let typescript =
        window.run("db.object.generateCode", json!({ "connection": id, "path": path, "language": "typescript" }));
    assert!(typescript["code"].as_str().unwrap().contains("picture: Uint8Array | null;"), "{typescript}");
    let schema = window.registry.dispatch(
        &mut window.session,
        "db.object.generateCode",
        json!({ "connection": id, "path": ["shop", "public", "tables", "missing"], "language": "go" }),
    );
    assert!(schema.is_err());
}

#[cfg(unix)]
#[test]
#[ignore = "needs psql on PATH and PostgreSQL in Docker on the last port of ZEN_DB_PG_PORTS (docs/development.md)"]
fn the_real_psql_signs_in_with_the_password_file_and_leaves_nothing_behind() {
    let port: u16 = std::env::var("ZEN_DB_PG_PORTS")
        .ok()
        .and_then(|ports| ports.rsplit(',').next().and_then(|port| port.trim().parse().ok()))
        .unwrap_or(54318);
    let (mut window, _folder) = window_with_clients(false);
    window.session.set_state(ClientFolders(None));
    let url = format!("postgresql://postgres@127.0.0.1:{port}/postgres");
    let added = window.run("db.dataSource.add", json!({ "name": "Real", "url": url, "type": "dev" }));
    let id = added["id"].as_str().unwrap().to_owned();
    let opened = window.run("db.connection.openTerminal", json!({ "connection": id, "password": "zen" }));
    assert_eq!(opened["passwordFrom"], "given", "{opened}");
    let session = opened["session"].as_u64().unwrap();
    window.run(
        "terminal.session.send",
        json!({ "id": session, "text": "SELECT 'SIGNED' || '-IN';\n\\! echo \"[env:$PGPASSWORD]\"\n\\q\n" }),
    );
    let text = screen_until(&mut window, session, "[env:]");
    assert!(text.contains("SIGNED-IN"), "{text}");
    assert!(text.contains("[env:]"), "psql's shell has no password in its environment: {text}");
    window.run("terminal.session.send", json!({ "id": session, "text": "echo \"[file:$PGPASSFILE]\"\n" }));
    let text = screen_until(&mut window, session, "[file:]");
    assert!(text.contains("[file:]"), "{text}");
    let left: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with(&format!("zen-db-client-{}-", std::process::id()))
        })
        .collect();
    assert!(left.is_empty(), "the password file is gone once psql ends: {left:?}");
}

#[test]
fn a_relative_entry_of_path_is_never_searched_for_a_client() {
    let path = std::env::join_paths(["/usr/bin", "bin", "", "./tools"]).unwrap();
    assert_eq!(super::absolute_folders(Some(&path)), [std::path::PathBuf::from("/usr/bin")]);
    assert!(super::absolute_folders(None).is_empty());
}

#[cfg(unix)]
#[test]
fn the_option_file_is_the_clients_first_option() {
    let command = zen_db::client::ClientCommand {
        tool: zen_db::client::Tool::Mysql,
        args: vec!["--host=db".to_owned(), "shop".to_owned()],
        env: Vec::new(),
        password_via: None,
    };
    let launch =
        super::Launch { command, program: "/usr/bin/mysql".into(), cwd: "/tmp".into(), name: "Shop".to_owned() };
    let options = super::options(&launch, Path::new("/bin/zsh"), Some(Path::new("/tmp/secret")), false);
    let client = options.args.iter().position(|arg| arg == "/usr/bin/mysql").unwrap();
    assert_eq!(
        options.args[client + 1..],
        ["--defaults-extra-file=/tmp/secret", "--host=db", "shop"],
        "mysql reads its option file only as the first option"
    );
}
