use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

const WAIT: Duration = Duration::from_secs(60);

fn program() -> String {
    std::env::var("ZEN_DB_DUCKDB_PROGRAM").unwrap_or_default()
}

fn folder() -> PathBuf {
    let folder = std::env::var_os("ZEN_DB_DUCKDB_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let made = folder.join(format!("zen-duckdb-{}", std::process::id()));
    std::fs::create_dir_all(&made).unwrap();
    made
}

struct Driver {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Json>,
    next: u64,
}

impl Driver {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zen-db-duckdb"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the driver program starts");
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if sender.send(serde_json::from_str(&line).unwrap_or(Json::Null)).is_err() {
                    break;
                }
            }
        });
        let mut driver = Self { child, input, lines, next: 1 };
        driver.call(
            "initialize",
            json!({ "protocol": 1, "driver": "duckdb", "client": { "name": "Zen Test", "version": "1" } }),
        );
        driver
    }

    fn send(&mut self, method: &str, params: Json) -> u64 {
        let id = self.next;
        self.next += 1;
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.input, "{line}").unwrap();
        self.input.flush().unwrap();
        id
    }

    fn answer_to(&mut self, id: u64) -> Json {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let message = self.lines.recv_timeout(left).expect("an answer in time");
            if message["id"] == id {
                return message;
            }
        }
    }

    fn call(&mut self, method: &str, params: Json) -> Json {
        let id = self.send(method, params);
        let answer = self.answer_to(id);
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        answer["result"].clone()
    }

    fn session(&mut self, file: &str, read_only: bool) -> String {
        let params = json!({
            "connection": "test", "context": "main", "readOnly": read_only,
            "settings": { "file": file, "program": program() }, "state": {}, "timeoutMs": 60000
        });
        self.call("connect", params)["session"].as_str().unwrap().to_owned()
    }

    fn execute(&mut self, session: &str, statement: &str) -> Json {
        let id = self.send("execute", json!({ "session": session, "statement": statement, "maxRows": 1000 }));
        self.answer_to(id)
    }

    fn ok(&mut self, session: &str, statement: &str) -> Json {
        let answer = self.execute(session, statement);
        assert!(answer.get("error").is_none(), "{statement}: {answer}");
        answer["result"].clone()
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn kind(answer: &Json) -> String {
    answer["error"]["data"]["kind"].as_str().unwrap_or_default().to_owned()
}

#[test]
#[ignore = "needs the duckdb program (docs/development.md)"]
fn queries_come_back_typed_and_errors_say_where() {
    let mut driver = Driver::start();
    let session = driver.session("", false);
    driver.ok(&session, "CREATE TABLE people (id INTEGER PRIMARY KEY, name VARCHAR, born DATE, score DECIMAL(5,2), tags VARCHAR[], doc STRUCT(k INTEGER), raw BLOB, big HUGEINT)");
    driver.ok(&session, "INSERT INTO people VALUES (1, 'Ada', DATE '1815-12-10', 1.5, ['math'], {'k': 1}, '\\x01a'::BLOB, 170141183460469231731687303715884105727)");
    let found = driver.ok(&session, "SELECT *, 1 AS id FROM people");
    let names: Vec<&str> =
        found["columns"].as_array().unwrap().iter().filter_map(|column| column["name"].as_str()).collect();
    assert_eq!(names, ["id", "name", "born", "score", "tags", "doc", "raw", "big", "id"], "repeated names stay");
    assert_eq!(found["columns"][2]["type"], "date");
    let row = &found["rows"][0];
    assert_eq!(row[0], json!({ "kind": "integer", "value": 1 }));
    assert_eq!(row[2]["value"]["text"], "1815-12-10");
    assert_eq!(row[3], json!({ "kind": "decimal", "value": "1.50" }));
    assert_eq!(row[4], json!({ "kind": "json", "value": "[\"math\"]" }));
    assert_eq!(row[6], json!({ "kind": "bytes", "value": "0161" }));
    assert_eq!(row[7]["value"], "170141183460469231731687303715884105727");
    let empty = driver.ok(&session, "SELECT id, name FROM people WHERE false");
    assert_eq!(empty["columns"].as_array().unwrap().len(), 2, "an empty result still has its columns");
    let failed = driver.execute(&session, "SELECT *\nFROM nope");
    assert_eq!(kind(&failed), "server");
    assert_eq!(failed["error"]["data"]["position"], 15, "{failed}");
    assert_eq!(kind(&driver.execute(&session, ".shell ls")), "invalid");
    assert_eq!(kind(&driver.execute(&session, "SELECT 1; SELECT 2")), "invalid");
    let after = driver.ok(&session, "SELECT 2 AS two");
    assert_eq!(after["rows"][0][0]["value"], 2, "the session goes on after errors");
}

#[test]
#[ignore = "needs the duckdb program (docs/development.md)"]
fn files_parquet_and_csv_are_read_and_a_read_only_file_refuses_changes() {
    let folder = folder();
    let file = folder.join("shop.duckdb").display().to_string();
    let parquet = folder.join("orders.parquet").display().to_string();
    let csv = folder.join("orders.csv").display().to_string();
    let mut driver = Driver::start();
    let session = driver.session(&file, false);
    driver.ok(&session, "CREATE TABLE orders AS SELECT range AS id, range * 1.5 AS total FROM range(5)");
    driver.ok(&session, &format!("COPY orders TO '{parquet}' (FORMAT parquet)"));
    driver.ok(&session, &format!("COPY orders TO '{csv}' (HEADER)"));
    let from_parquet = driver.ok(&session, &format!("SELECT count(*) AS n, sum(total) AS s FROM '{parquet}'"));
    assert_eq!(from_parquet["rows"][0][0]["value"], 5);
    let from_csv = driver.ok(&session, &format!("SELECT * FROM read_csv('{csv}') ORDER BY id"));
    assert_eq!(from_csv["rows"].as_array().unwrap().len(), 5);
    driver.call("close", json!({ "session": session }));
    let reader = driver.session(&file, true);
    let read = driver.ok(&reader, "SELECT count(*) FROM orders");
    assert_eq!(read["rows"][0][0]["value"], 5);
    let refused = driver.execute(&reader, "DELETE FROM orders");
    assert_eq!(kind(&refused), "server", "{refused}");
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
#[ignore = "needs the duckdb program (docs/development.md)"]
fn no_line_of_a_statement_reaches_the_shell_of_the_duckdb_program() {
    let folder = folder();
    let mark = folder.join("shell-ran");
    let touch = format!("touch {}", mark.display());
    let mut driver = Driver::start();
    let session = driver.session("", false);
    for attempt in [
        format!(".shell {touch}"),
        format!("SELECT 1\n.shell {touch}"),
        format!("SELECT 1\n  .system {touch}"),
        format!("SELECT 1\nGO\n.shell {touch}"),
        format!("SELECT 1\n/\n.shell {touch}"),
        format!("SELECT 1;\n.shell {touch}"),
        format!("SELECT 1\r\n.shell {touch}"),
    ] {
        let answer = driver.execute(&session, &attempt);
        assert_eq!(kind(&answer), "invalid", "{attempt:?}: {answer}");
    }
    let quoted = driver.ok(&session, &format!("SELECT $$\n;\n.shell {touch}\n$$ AS text"));
    assert!(quoted["rows"][0][0]["value"].as_str().unwrap().contains(".shell"));
    assert!(!mark.exists(), "no shell command ran");
    let setting = driver.ok(&session, "SELECT current_setting('autoinstall_known_extensions') AS on");
    assert_eq!(setting["rows"][0][0]["value"], false, "extensions are never downloaded unasked");
    let _ = std::fs::remove_dir_all(folder);
}

#[cfg(unix)]
fn fake_program(folder: &std::path::Path, script: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let path = folder.join("duckdb");
    std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[cfg(unix)]
fn connect_with(program: &str) -> Json {
    let mut driver = Driver::start();
    let params = json!({
        "connection": "test", "context": "main", "readOnly": false,
        "settings": { "file": "", "program": program }, "state": {}, "timeoutMs": 10000
    });
    let id = driver.send("connect", params);
    driver.answer_to(id)
}

#[cfg(unix)]
#[test]
fn a_duckdb_older_than_1_0_is_refused_before_it_reads_anything() {
    let folder = folder().join("old");
    std::fs::create_dir_all(&folder).unwrap();
    let calls = folder.join("calls");
    let program = fake_program(
        &folder,
        &format!(
            "echo \"$*\" >> '{}'\ncase \"$1\" in -version) echo 'v0.10.0 20b1486d11'; exit 0;; esac\nexit 1\n",
            calls.display()
        ),
    );
    let answer = connect_with(&program);
    assert_eq!(kind(&answer), "unsupported", "{answer}");
    let message = answer["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("v0.10.0") && message.contains("1.0"), "{message}");
    let calls = std::fs::read_to_string(calls).unwrap();
    assert_eq!(calls.lines().collect::<Vec<_>>(), ["-version"], "only the version is asked");
    let _ = std::fs::remove_dir_all(folder);
}

#[cfg(unix)]
#[test]
fn the_duckdb_program_runs_without_an_init_file_or_agent_output() {
    let folder = folder().join("safe");
    std::fs::create_dir_all(&folder).unwrap();
    let calls = folder.join("calls");
    let program = fake_program(
        &folder,
        &format!(
            "echo \"$DUCKDB_AGENT_MODE $*\" >> '{}'\ncase \"$*\" in *-version) echo 'v2.0.0 d54cbbe3df'; exit 0;; esac\nexit 1\n",
            calls.display()
        ),
    );
    let answer = connect_with(&program);
    assert!(answer.get("error").is_some(), "{answer}");
    let calls = std::fs::read_to_string(calls).unwrap();
    let started = calls.lines().last().unwrap_or_default();
    assert_eq!(started, "0 -batch -init /dev/null :memory:", "{calls}");
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
#[ignore = "needs the duckdb program (docs/development.md)"]
fn transactions_commit_and_roll_back_and_manual_commit_opens_one() {
    let mut driver = Driver::start();
    let session = driver.session("", false);
    driver.ok(&session, "CREATE TABLE t (id INTEGER)");
    let begun = driver.ok(&session, "BEGIN");
    assert_eq!(begun["inTransaction"], true);
    driver.ok(&session, "INSERT INTO t VALUES (1)");
    assert_eq!(driver.call("commit", json!({ "session": session }))["inTransaction"], false);
    driver.call("apply", json!({ "session": session, "state": { "autoCommit": false } }));
    let inserted = driver.ok(&session, "INSERT INTO t VALUES (2)");
    assert_eq!(inserted["inTransaction"], true, "the first change opens a transaction");
    driver.call("rollback", json!({ "session": session }));
    let rows = driver.ok(&session, "SELECT id FROM t");
    assert_eq!(rows["rows"], json!([[{ "kind": "integer", "value": 1 }]]));
}

#[test]
#[ignore = "needs the duckdb program (docs/development.md)"]
fn cancel_stops_a_statement_and_the_next_one_runs_in_a_new_process() {
    let folder = folder();
    let file = folder.join("cancel.duckdb").display().to_string();
    let mut driver = Driver::start();
    let session = driver.session(&file, false);
    driver.ok(&session, "CREATE TABLE kept AS SELECT 1 AS id");
    let started = Instant::now();
    let id = driver.send(
        "execute",
        json!({ "session": session, "statement": "SELECT sum(i * random()) FROM range(1000000000000) t(i)", "maxRows": 10 }),
    );
    std::thread::sleep(Duration::from_secs(2));
    driver.call("cancel", json!({ "session": session }));
    let answer = driver.answer_to(id);
    assert_eq!(kind(&answer), "cancelled", "{answer}");
    assert!(started.elapsed() < Duration::from_secs(30));
    let timed = driver.send(
        "execute",
        json!({ "session": session, "statement": "SELECT sum(i * random()) FROM range(1000000000000) t(i)", "timeoutMs": 1500 }),
    );
    assert_eq!(kind(&driver.answer_to(timed)), "timeout");
    let after = driver.ok(&session, "SELECT count(*) FROM kept");
    assert_eq!(after["rows"][0][0]["value"], 1, "a database file keeps its data");
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
#[ignore = "needs the duckdb program (docs/development.md)"]
fn the_navigator_lists_databases_schemas_tables_views_and_columns() {
    let mut driver = Driver::start();
    let session = driver.session("", false);
    driver.ok(&session, "CREATE SCHEMA shop");
    driver.ok(&session, "CREATE TABLE shop.orders (id INTEGER NOT NULL, total DOUBLE)");
    driver.ok(&session, "CREATE VIEW shop.big AS SELECT * FROM shop.orders WHERE total > 10");
    let roots = driver.call("children", json!({ "session": session, "path": [] }))["nodes"].clone();
    let memory = roots.as_array().unwrap().iter().find(|node| node["label"] == "memory").unwrap();
    assert_eq!(memory["kind"], "catalog");
    let schemas = driver.call("children", json!({ "session": session, "path": ["memory"] }))["nodes"].clone();
    assert!(schemas.as_array().unwrap().iter().any(|node| node["label"] == "shop"));
    let objects = driver.call("children", json!({ "session": session, "path": ["memory", "shop"] }))["nodes"].clone();
    let kinds: Vec<(&str, &str)> = objects
        .as_array()
        .unwrap()
        .iter()
        .map(|node| (node["label"].as_str().unwrap(), node["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(kinds, [("big", "view"), ("orders", "table")]);
    let columns =
        driver.call("children", json!({ "session": session, "path": ["memory", "shop", "orders"] }))["nodes"].clone();
    assert_eq!(columns[0]["detail"], "INTEGER NOT NULL");
    assert_eq!(columns[1]["label"], "total");
    driver.call("apply", json!({ "session": session, "state": { "schema": "shop" } }));
    let used = driver.ok(&session, "SELECT count(*) FROM orders");
    assert_eq!(used["rows"][0][0]["value"], 0, "the schema applied is the session's");
}
