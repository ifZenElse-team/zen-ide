use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::backup::{Origin, Probe};

use crate::workspaces::Workspaces;

const PG_DUMP: &str = r#"#!/bin/sh
here=$(dirname "$0")
if [ "$1" = "--version" ]; then echo "pg_dump (PostgreSQL) 17.2"; exit 0; fi
printf '%s\n' "$@" > "$here/pg_dump.args"
if [ -n "$PGPASSFILE" ]; then sed 's/^[*]:[*]:[*]:[*]://' "$PGPASSFILE" | tr -d '\n' > "$here/pg_dump.password"; printf '%s' "$PGPASSFILE" > "$here/pg_dump.passfile"; fi
printf '%s' "$PGPASSWORD" > "$here/pg_dump.variable"
case "$*" in *"dbname='slow'"*) sleep 30 & wait; exit 1;; esac
case "$*" in *"dbname='broken'"*) echo "pg_dump: error: connection failed" >&2; exit 1;; esac
for arg in "$@"; do case $arg in --file=*) file=${arg#--file=};; esac; done
echo "pg_dump: dumping contents of table public.orders" >&2
printf 'PGDMP fake backup' > "$file"
"#;

const PG_RESTORE: &str = r#"#!/bin/sh
here=$(dirname "$0")
if [ "$1" = "--version" ]; then echo "pg_restore (PostgreSQL) 17.2"; exit 0; fi
printf '%s\n' "$@" > "$here/pg_restore.args"
if [ -n "$PGPASSFILE" ]; then sed 's/^[*]:[*]:[*]:[*]://' "$PGPASSFILE" | tr -d '\n' > "$here/pg_restore.password"; printf '%s' "$PGPASSFILE" > "$here/pg_restore.passfile"; fi
printf '%s' "$PGPASSWORD" > "$here/pg_restore.variable"
"#;

const PSQL: &str = r#"#!/bin/sh
here=$(dirname "$0")
if [ "$1" = "--version" ]; then echo "psql (PostgreSQL) 17.2"; exit 0; fi
printf '%s\n' "$@" > "$here/psql.args"
"#;

const MYSQLDUMP: &str = r#"#!/bin/sh
here=$(dirname "$0")
if [ "$1" = "--version" ]; then echo "mysqldump  Ver 8.4.3 for macos15.0 on arm64 (Homebrew)"; exit 0; fi
printf '%s\n' "$@" > "$here/mysqldump.args"
case "$1" in --defaults-extra-file=*) f=${1#--defaults-extra-file=}; sed -n 's/^password="\(.*\)"$/\1/p' "$f" | tr -d '\n' > "$here/mysqldump.password"; printf '%s' "$f" > "$here/mysqldump.passfile";; esac
printf '%s' "$MYSQL_PWD" > "$here/mysqldump.variable"
for arg in "$@"; do case $arg in --result-file=*) file=${arg#--result-file=};; esac; done
printf 'CREATE TABLE t (id int);\n' > "$file"
"#;

const MYSQL: &str = r#"#!/bin/sh
here=$(dirname "$0")
if [ "$1" = "--version" ]; then echo "mysql  Ver 8.4.3 for macos15.0 on arm64 (Homebrew)"; exit 0; fi
printf '%s\n' "$@" > "$here/mysql.args"
cat > "$here/mysql.stdin"
case "$1" in --defaults-extra-file=*) f=${1#--defaults-extra-file=}; sed -n 's/^password="\(.*\)"$/\1/p' "$f" | tr -d '\n' > "$here/mysql.password"; printf '%s' "$f" > "$here/mysql.passfile";; esac
printf '%s' "$MYSQL_PWD" > "$here/mysql.variable"
"#;

struct Window {
    registry: Registry,
    session: Session,
    tools: PathBuf,
    dir: tempfile::TempDir,
}

fn tool(folder: &Path, name: &str, script: &str) {
    let path = folder.join(name);
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
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
        let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![
            Arc::new(zen_db::postgres::Postgres),
            Arc::new(zen_db::mysql::MySql::for_mysql()),
            Arc::new(zen_db::mysql::MySql::for_mariadb()),
            Arc::new(zen_db::memory::MemoryDriver::sql()),
        ];
        session.state_mut::<super::super::Database>().set_drivers(drivers);
        let tools = dir.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::create_dir(dir.path().join("backups")).unwrap();
        super::set_probe(
            &mut session,
            Probe { folders: vec![(tools.clone(), Origin::Path)], docker: None, timeout: Duration::from_secs(60) },
        );
        Self { registry, session, tools, dir }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn refusal(&mut self, command: &str, params: Value) -> (String, String, Value) {
        match self.registry.dispatch(&mut self.session, command, params) {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                (error.code.unwrap_or_default(), error.message, error.data.map(Value::Object).unwrap_or(Value::Null))
            }
            other => panic!("not refused: {other:?}"),
        }
    }

    fn connection(&mut self, driver: &str, database: &str, extra: Value) -> String {
        let mut params = json!({ "name": database, "driver": driver, "host": "localhost", "port": 5432, "database": database, "user": "app" });
        if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
            params.extend(extra.clone());
        }
        let added = self.run("db.dataSource.add", params);
        if let Some(operation) = added["secretOperation"].as_u64() {
            self.run("db.operation.state", json!({ "operation": operation, "waitMs": 10_000 }));
        }
        added["id"].as_str().unwrap().to_owned()
    }

    fn finished(&mut self, run_id: u64) -> Value {
        let state = self.run("db.backup.state", json!({ "runId": run_id, "waitMs": 60_000 }));
        state["jobs"][0].clone()
    }

    fn console(&mut self, run_id: u64) -> String {
        let output = self.run("run.output.get", json!({ "runId": run_id }));
        output["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn backups(&self) -> PathBuf {
        self.dir.path().join("backups")
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.tools.join(name)).unwrap_or_default()
    }
}

#[cfg(unix)]
#[test]
fn tools_are_found_with_their_versions_and_other_drivers_are_refused() {
    let mut window = Window::new();
    tool(&window.tools, "pg_dump", PG_DUMP);
    tool(&window.tools, "psql", PSQL);
    let id = window.connection("postgresql", "shop", json!({ "auth": "none" }));
    let found = window.run("db.backup.tools", json!({ "connection": id, "waitMs": 60_000 }));
    assert_eq!(found["status"], "done", "{found}");
    assert_eq!(found["family"], "postgres");
    assert_eq!(found["docker"]["state"], "notFound", "no docker in the probe");
    let chosen: Vec<&str> =
        found["chosen"].as_array().unwrap().iter().map(|tool| tool["tool"].as_str().unwrap()).collect();
    assert_eq!(chosen, ["pgDump", "psql"]);
    assert_eq!(found["chosen"][0]["version"], "pg_dump (PostgreSQL) 17.2");
    assert_eq!(found["chosen"][0]["major"], 17);
    assert_eq!(found["chosen"][0]["origin"], "path");

    let memory =
        window.run("db.dataSource.add", json!({ "name": "kv", "driver": zen_db::memory::SQL, "auth": "none" }));
    let (code, message, _) = window.refusal("db.backup.tools", json!({ "connection": memory["id"] }));
    assert_eq!(code, "unsupported", "{message}");
    let tunnel = window.connection("postgresql", "far", json!({ "auth": "none", "ssh": { "host": "bastion" } }));
    let (code, _, _) = window.refusal("db.backup.start", json!({ "connection": tunnel }));
    assert_eq!(code, "sshTunnel");
}

#[cfg(unix)]
#[test]
fn a_backup_runs_in_the_run_window_with_the_password_only_in_a_private_file() {
    let mut window = Window::new();
    tool(&window.tools, "pg_dump", PG_DUMP);
    let id = window.connection("postgresql", "shop", json!({ "password": "s3cret!" }));
    let file = window.backups().join("shop.dump");
    let params = json!({
        "connection": id,
        "file": file,
        "pgDump": { "format": "custom", "schemas": ["public"], "noOwner": true },
    });
    let started = window.run("db.backup.start", params.clone());
    let run_id = started["runId"].as_u64().unwrap();
    assert_eq!(started["database"], "shop");
    let job = window.finished(run_id);
    assert_eq!(job["status"], "finished", "{job}");
    assert_eq!(job["tool"]["tool"], "pgDump");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "PGDMP fake backup");
    assert_eq!(job["bytes"], 17);
    let args = window.read("pg_dump.args");
    assert!(
        args.contains("--dbname=dbname='shop'\n")
            && args.contains("--schema=public\n")
            && args.contains("--no-owner\n"),
        "{args}"
    );
    assert!(args.contains("--no-password\n") && args.contains("--format=c\n"), "{args}");
    assert!(!args.contains("s3cret"), "the password is never an argument: {args}");
    assert_eq!(window.read("pg_dump.password"), "s3cret!", "the saved password reaches the tool in PGPASSFILE");
    assert_eq!(window.read("pg_dump.variable"), "", "never in PGPASSWORD");
    assert!(!window.read("pg_dump.args").contains("s3cret!"), "nor in the arguments");
    let passfile = PathBuf::from(window.read("pg_dump.passfile"));
    assert!(passfile.is_absolute() && !passfile.exists(), "the password file goes when the run ends: {passfile:?}");
    let leftovers: Vec<String> = std::fs::read_dir(window.backups())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["shop.dump"], "the hidden file was moved into place");

    let runs = window.run("run.process.list", json!({}));
    let run = runs["runs"].as_array().unwrap().iter().find(|run| run["runId"] == run_id).cloned().unwrap();
    assert_eq!(
        (run["kind"].as_str(), run["name"].as_str(), run["debug"].as_bool()),
        (Some("databaseBackup"), Some("Backup shop"), Some(false))
    );
    let console = window.console(run_id);
    assert!(console.contains("Using pg_dump (PostgreSQL) 17.2") && console.contains("dumping contents"), "{console}");
    assert!(console.contains("Backup written to") && !console.contains("s3cret"), "{console}");
    let state = window.run("db.backup.state", json!({}));
    assert!(!state.to_string().contains("s3cret"), "{state}");

    let (code, _, data) = window.refusal("db.backup.start", params.clone());
    assert_eq!(code, "exists");
    assert_eq!(data["paths"][0], file.to_string_lossy().as_ref());
    let mut again = params;
    again["overwrite"] = json!(true);
    let rerun = window.run("db.backup.start", again)["runId"].as_u64().unwrap();
    assert_eq!(window.finished(rerun)["status"], "finished");

    let settings = window.run("db.backup.settings", json!({}));
    assert_eq!(settings["folder"], window.backups().to_string_lossy().as_ref());
    assert_eq!(settings["pgDump"]["noOwner"], true, "{settings}");
    let named = window.run("db.backup.start", json!({ "connection": id }));
    let named_file = PathBuf::from(named["file"].as_str().unwrap());
    assert_eq!(named_file.parent(), Some(window.backups().as_path()));
    assert!(named_file.file_name().unwrap().to_string_lossy().starts_with("shop_"), "{named_file:?}");
    assert_eq!(window.finished(named["runId"].as_u64().unwrap())["status"], "finished");
}

#[cfg(unix)]
#[test]
fn a_cancelled_or_failing_backup_leaves_no_file_and_a_missing_tool_says_how_to_get_it() {
    let mut window = Window::new();
    let id = window.connection("postgresql", "slow", json!({ "auth": "none" }));
    let file = window.backups().join("slow.dump");
    let missing = window.run("db.backup.start", json!({ "connection": id, "file": file }))["runId"].as_u64().unwrap();
    let job = window.finished(missing);
    assert_eq!(job["status"], "failed", "{job}");
    let error = job["error"].as_str().unwrap();
    assert!(error.contains("pg_dump was not found") && error.contains("brew install libpq"), "{error}");

    tool(&window.tools, "pg_dump", PG_DUMP);
    window.run("db.backup.tools", json!({ "connection": id, "refresh": true, "waitMs": 60_000 }));
    let run_id = window.run("db.backup.start", json!({ "connection": id, "file": file }))["runId"].as_u64().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while window.read("pg_dump.args").is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(window.run("db.backup.cancel", json!({ "runId": run_id }))["cancelled"], true);
    let job = window.finished(run_id);
    assert_eq!(job["status"], "stopped", "{job}");
    assert!(std::fs::read_dir(window.backups()).unwrap().next().is_none(), "nothing left behind");
    assert_eq!(window.run("db.backup.cancel", json!({ "runId": run_id }))["cancelled"], false);

    let broken = window.connection("postgresql", "broken", json!({ "auth": "none" }));
    let run_id = window
        .run("db.backup.start", json!({ "connection": broken, "file": window.backups().join("b.dump") }))["runId"]
        .as_u64()
        .unwrap();
    let job = window.finished(run_id);
    assert_eq!((job["status"].as_str(), job["exitCode"].as_i64()), (Some("failed"), Some(1)), "{job}");
    assert!(window.console(run_id).contains("connection failed"));
    assert!(std::fs::read_dir(window.backups()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn a_restore_asks_first_and_picks_pg_restore_or_psql_by_the_backups_format() {
    let mut window = Window::new();
    tool(&window.tools, "pg_restore", PG_RESTORE);
    tool(&window.tools, "psql", PSQL);
    let id = window.connection("postgresql", "shop", json!({ "password": "pw" }));
    let archive = window.backups().join("shop.dump");
    std::fs::write(&archive, b"PGDMP archive").unwrap();
    let params = json!({ "connection": id, "file": archive, "pgRestore": { "clean": true, "ifExists": true } });
    let (code, message, data) = window.refusal("db.backup.restore", params.clone());
    assert_eq!(code, "confirmExecute", "{message}");
    assert!(message.contains("into database shop"), "{message}");
    assert_eq!(data["retry"]["command"], "db.backup.restore");
    let token = data["token"].as_str().unwrap().to_owned();
    let mut confirmed = params.clone();
    confirmed["confirmed"] = json!(token);
    let run_id = window.run("db.backup.restore", confirmed)["runId"].as_u64().unwrap();
    let job = window.finished(run_id);
    assert_eq!((job["status"].as_str(), job["tool"]["tool"].as_str()), (Some("finished"), Some("pgRestore")), "{job}");
    let args = window.read("pg_restore.args");
    assert!(args.contains("--clean\n--if-exists\n") && args.ends_with(&format!("{}\n", archive.display())), "{args}");
    assert_eq!(window.read("pg_restore.password"), "pw");
    assert!(window.console(run_id).contains("Restore into shop finished"));

    let plain = window.backups().join("shop.sql");
    std::fs::write(&plain, "CREATE TABLE t (id int);\n").unwrap();
    let params = json!({ "connection": id, "file": plain });
    let (_, _, data) = window.refusal("db.backup.restore", params.clone());
    let mut confirmed = params;
    confirmed["confirmed"] = data["token"].clone();
    let run_id = window.run("db.backup.restore", confirmed)["runId"].as_u64().unwrap();
    assert_eq!(window.finished(run_id)["tool"]["tool"], "psql");
    assert!(window.read("psql.args").contains(&format!("--file={}", plain.display())));

    let (code, _, _) =
        window.refusal("db.backup.restore", json!({ "connection": id, "file": window.backups().join("none.sql") }));
    assert_eq!(code, "noFile");
    window.run("db.connection.readOnly", json!({ "connection": id, "readOnly": true }));
    let (code, _, _) = window.refusal("db.backup.restore", json!({ "connection": id, "file": plain }));
    assert_eq!(code, "readOnly");
}

#[cfg(unix)]
#[test]
fn agents_restore_only_where_their_access_is_full() {
    let mut window = Window::new();
    let id = window.connection("postgresql", "shop", json!({ "auth": "none", "type": "prod" }));
    let file = window.backups().join("shop.sql");
    std::fs::write(&file, "SELECT 1;\n").unwrap();
    let registry = &window.registry;
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        registry.dispatch(session, "db.backup.restore", json!({ "connection": id, "file": file }))
    });
    match refused {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert!(matches!(error.code.as_deref(), Some("agentReadOnly" | "agentNotAllowed")), "{error:?}");
        }
        other => panic!("not refused: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn agents_pass_no_tool_arguments_and_never_write_a_hidden_file() {
    let mut window = Window::new();
    let id = window.connection("postgresql", "shop", json!({ "auth": "none", "type": "dev" }));
    window.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
    let registry = &window.registry;
    let backups = window.backups();
    let code = |session: &mut Session, params: Value| match zen_core::macros::as_agent(session, |session| {
        registry.dispatch(session, "db.backup.start", params)
    }) {
        Err(zen_core::DispatchError::Failed { error, .. }) => error.code.unwrap_or_default(),
        other => panic!("not refused: {other:?}"),
    };
    let extra = json!({ "connection": id, "folder": backups, "pgDump": { "extraArgs": ["--file=/tmp/elsewhere"] } });
    assert_eq!(code(&mut window.session, extra), "agentExtraArgs");
    let hidden = json!({ "connection": id, "file": backups.join(".env") });
    assert_eq!(code(&mut window.session, hidden), "redacted", "a file hidden from agents");
}

#[cfg(unix)]
#[test]
fn an_agent_never_has_a_hidden_file_read_into_the_database() {
    let mut window = Window::new();
    let id = window.connection("postgresql", "shop", json!({ "auth": "none", "type": "dev" }));
    window.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
    let file = window.backups().join(".env");
    std::fs::write(&file, "TOKEN=x\n").unwrap();
    let registry = &window.registry;
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        registry.dispatch(session, "db.backup.restore", json!({ "connection": id, "file": file }))
    });
    assert!(
        matches!(&refused, Err(zen_core::DispatchError::Failed { error, .. }) if error.code.as_deref() == Some("redacted")),
        "{refused:?}"
    );
}

#[test]
fn programs_on_path_inside_the_project_or_relative_are_never_searched() {
    let project = tempfile::tempdir().unwrap();
    let bin = project.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    assert!(!zen_db::backup::outside(Path::new("bin"), None), "a relative folder is wherever the IDE runs");
    assert!(!zen_db::backup::outside(&bin, Some(project.path())), "the project's own bin");
    assert!(zen_db::backup::outside(Path::new("/usr/bin"), Some(project.path())));
}

#[cfg(unix)]
#[test]
fn mysqldump_and_mysql_get_the_password_in_an_option_file_and_the_file_on_standard_input() {
    let mut window = Window::new();
    tool(&window.tools, "mysqldump", MYSQLDUMP);
    tool(&window.tools, "mysql", MYSQL);
    let id = window.connection("mysql", "shop", json!({ "port": 3306, "password": "my pw" }));
    let file = window.backups().join("shop.sql");
    let params = json!({ "connection": id, "file": file, "mysqldump": { "tables": ["orders"], "routines": false } });
    let run_id = window.run("db.backup.start", params)["runId"].as_u64().unwrap();
    assert_eq!(window.finished(run_id)["status"], "finished");
    let args = window.read("mysqldump.args");
    assert!(
        args.contains("--protocol=TCP\n") && args.ends_with("shop\norders\n") && !args.contains("--routines"),
        "{args}"
    );
    assert!(!args.contains("my pw"), "{args}");
    assert_eq!(window.read("mysqldump.password"), "my pw", "from --defaults-extra-file, the first option");
    assert_eq!(window.read("mysqldump.variable"), "", "never in MYSQL_PWD");
    assert!(!PathBuf::from(window.read("mysqldump.passfile")).exists(), "removed after the run");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "CREATE TABLE t (id int);\n");

    let params = json!({ "connection": id, "file": file, "mysql": { "force": true } });
    let (_, _, data) = window.refusal("db.backup.restore", params.clone());
    let mut confirmed = params;
    confirmed["confirmed"] = data["token"].clone();
    let run_id = window.run("db.backup.restore", confirmed)["runId"].as_u64().unwrap();
    let job = window.finished(run_id);
    assert_eq!((job["status"].as_str(), job["tool"]["tool"].as_str()), (Some("finished"), Some("mysql")), "{job}");
    assert_eq!(window.read("mysql.stdin"), "CREATE TABLE t (id int);\n", "the backup on standard input");
    assert!(window.read("mysql.args").contains("--force\n"));
    assert_eq!(window.read("mysql.password"), "my pw");
    assert!(window.console(run_id).contains("< "), "the console shows the redirection");
}

fn live_port(variable: &str, driver: Option<&str>, default: u16) -> u16 {
    let value = std::env::var(variable).unwrap_or_default();
    let port = match driver {
        None => value.split(',').filter_map(|port| port.trim().parse().ok()).next_back(),
        Some(driver) => value.split(',').find_map(|pair| {
            let (name, port) = pair.trim().split_once(':')?;
            (name == driver).then(|| port.parse().ok()).flatten()
        }),
    };
    port.unwrap_or(default)
}

fn restore_confirmed(window: &mut Window, params: Value) -> Value {
    let job = restore_ended(window, params);
    assert_eq!(job["status"], "finished", "{job}");
    job
}

fn restore_ended(window: &mut Window, params: Value) -> Value {
    let (_, _, data) = window.refusal("db.backup.restore", params.clone());
    let mut confirmed = params;
    confirmed["confirmed"] = data["token"].clone();
    let run_id = window.run("db.backup.restore", confirmed)["runId"].as_u64().unwrap();
    let mut job = window.finished(run_id);
    job["console"] = json!(window.console(run_id));
    job
}

fn server_major(window: &mut Window, id: &str) -> u64 {
    let text =
        window.run("db.query.run", json!({ "connection": id, "sql": "SHOW server_version_num", "waitMs": 30_000 }));
    let value = text["results"][0]["page"][0][0]["value"].as_str().unwrap_or_default().to_owned();
    value.parse::<u64>().unwrap_or(0) / 10_000
}

fn backup_finished(window: &mut Window, params: Value) -> Value {
    let run_id = window.run("db.backup.start", params)["runId"].as_u64().unwrap();
    let job = window.finished(run_id);
    assert_eq!(job["status"], "finished", "{job}\n{}", window.console(run_id));
    job
}

#[cfg(unix)]
#[test]
#[ignore = "needs PostgreSQL, MySQL and MariaDB in Docker (docs/development.md)"]
fn real_servers_are_backed_up_and_restored_with_the_tools_found() {
    let mut window = Window::new();
    super::set_probe(&mut window.session, Probe::system());
    let pg = live_port("ZEN_DB_PG_PORTS", None, 54318);
    let id = window.connection(
        "postgresql",
        "postgres",
        json!({ "port": pg, "user": "postgres", "password": "zen", "tls": { "mode": "disable" } }),
    );
    let found = window.run("db.backup.tools", json!({ "connection": id, "waitMs": 30_000 }));
    assert_eq!(found["status"], "done", "{found}");
    let script = window.backups().join("items.sql");
    std::fs::write(
        &script,
        "DROP TABLE IF EXISTS backup_items;\nCREATE TABLE backup_items (id int PRIMARY KEY, label text);\nINSERT INTO backup_items VALUES (1, 'one'), (2, 'two');\n",
    )
    .unwrap();
    restore_confirmed(&mut window, json!({ "connection": id, "file": script }));
    let custom = window.backups().join("pg.dump");
    let job = backup_finished(
        &mut window,
        json!({ "connection": id, "file": custom, "pgDump": { "format": "custom", "tables": ["public.backup_items"] } }),
    );
    assert_eq!(zen_db::backup::pg_format(&custom).unwrap(), zen_db::backup::PgFormat::Custom, "{job}");
    let restored = restore_ended(
        &mut window,
        json!({ "connection": id, "file": custom, "pgRestore": { "clean": true, "ifExists": true } }),
    );
    let server = server_major(&mut window, &id);
    let newer = |found: &Value| found["major"].as_u64() > Some(server);
    let tools = window.run("db.backup.tools", json!({ "connection": id, "waitMs": 30_000 }));
    let only_newer = tools["tools"].as_array().unwrap().iter().filter(|found| found["tool"] == "pgRestore").all(newer);
    if only_newer {
        assert_eq!(restored["status"], "failed", "{restored}");
        let error = restored["error"].as_str().unwrap_or_default();
        assert!(error.contains("is newer than the server") && error.contains("brew install postgresql@"), "{error}");
        assert!(!restored["console"].as_str().unwrap_or_default().contains("pg_restore: "), "nothing ran: {restored}");
    } else {
        assert_eq!(restored["status"], "finished", "{restored}");
    }
    let plain = window.backups().join("pg.sql");
    backup_finished(
        &mut window,
        json!({ "connection": id, "file": plain, "pgDump": { "format": "plain", "tables": ["public.backup_items"], "inserts": true } }),
    );
    let text = std::fs::read_to_string(&plain).unwrap();
    assert!(text.contains("INSERT INTO public.backup_items VALUES (2, 'two');"), "{text}");

    for (driver, port) in [
        ("mysql", live_port("ZEN_DB_MYSQL_SERVERS", Some("mysql"), 33084)),
        ("mariadb", live_port("ZEN_DB_MYSQL_SERVERS", Some("mariadb"), 33114)),
    ] {
        let id = window.connection(
            driver,
            "mysql",
            json!({ "name": driver, "port": port, "user": "root", "password": "zen", "tls": { "mode": "disable" } }),
        );
        let script = window.backups().join(format!("{driver}-items.sql"));
        std::fs::write(
            &script,
            "DROP TABLE IF EXISTS backup_items;\nCREATE TABLE backup_items (id int PRIMARY KEY, label varchar(10));\nINSERT INTO backup_items VALUES (1, 'one'), (2, 'two');\n",
        )
        .unwrap();
        restore_confirmed(&mut window, json!({ "connection": id, "file": script }));
        let dump = window.backups().join(format!("{driver}.sql"));
        let job = backup_finished(
            &mut window,
            json!({ "connection": id, "file": dump, "mysqldump": { "tables": ["backup_items"], "routines": false } }),
        );
        let text = std::fs::read_to_string(&dump).unwrap();
        assert!(text.contains("INSERT INTO `backup_items` VALUES") && text.contains("(2,'two')"), "{job}\n{text}");
        restore_confirmed(&mut window, json!({ "connection": id, "file": dump }));
    }
}

#[cfg(unix)]
#[test]
fn a_rerun_backs_up_again_with_the_same_tool_and_a_restore_rerun_asks_first() {
    let mut window = Window::new();
    tool(&window.tools, "pg_dump", PG_DUMP);
    tool(&window.tools, "pg_restore", PG_RESTORE);
    let id = window.connection("postgresql", "shop", json!({ "password": "pw" }));
    let file = window.backups().join("shop.dump");
    let params = json!({ "connection": id, "file": file, "pgDump": { "noOwner": true } });
    let first = window.run("db.backup.start", params)["runId"].as_u64().unwrap();
    let used = window.finished(first)["tool"].clone();
    std::fs::remove_file(window.tools.join("pg_dump.args")).unwrap();
    let again = window.run("run.process.rerun", json!({ "runId": first }));
    let second = again["runId"].as_u64().unwrap();
    assert_ne!(second, first);
    let job = window.finished(second);
    assert_eq!(job["status"], "finished", "{job}");
    assert_eq!(job["tool"], used, "the same tool");
    assert_eq!(job["file"], file.to_string_lossy().as_ref(), "the same file, replaced");
    assert!(window.read("pg_dump.args").contains("--no-owner\n"), "the same options");
    assert!(!super::has_asked(&window.session));

    let params = json!({ "connection": id, "file": file, "pgRestore": { "clean": true } });
    let (_, _, data) = window.refusal("db.backup.restore", params.clone());
    let mut confirmed = params;
    confirmed["confirmed"] = data["token"].clone();
    let restored = window.run("db.backup.restore", confirmed)["runId"].as_u64().unwrap();
    assert_eq!(window.finished(restored)["status"], "finished");
    let (code, _, data) = window.refusal("run.process.rerun", json!({ "runId": restored }));
    assert_eq!(code, "confirmExecute", "a restore always asks again");
    assert_eq!(data["retry"]["command"], "db.backup.restore");
    let asked = super::take_asked(&mut window.session).expect("left for the dialog");
    assert!(asked.restore && asked.token.is_some());
    assert_eq!(asked.params["pgRestore"]["clean"], true);
    assert_eq!(asked.params["tool"]["origin"], "path");
    assert!(asked.params.get("password").is_none() && asked.params.get("confirmed").is_none());
    let mut confirmed = asked.params.clone();
    confirmed["confirmed"] = json!(asked.token);
    let third = window.run("db.backup.restore", confirmed)["runId"].as_u64().unwrap();
    assert_eq!(window.finished(third)["status"], "finished");

    let once = window.connection("postgresql", "once", json!({}));
    let file = window.backups().join("once.dump");
    let started = window.run("db.backup.start", json!({ "connection": once, "file": file, "password": "typed" }));
    let run_id = started["runId"].as_u64().unwrap();
    assert_eq!(window.finished(run_id)["status"], "finished");
    assert_eq!(window.read("pg_dump.password"), "typed");
    let (code, _, _) = window.refusal("run.process.rerun", json!({ "runId": run_id }));
    assert_eq!(code, "passwordNeeded", "a password typed for one run is never kept");
    let asked = super::take_asked(&mut window.session).expect("the dialog asks for it");
    assert!(!asked.restore && asked.code == "passwordNeeded" && !asked.params.to_string().contains("typed"));
}
