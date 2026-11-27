#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

const WAIT: Duration = Duration::from_secs(30);

fn fake(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake").join(name).to_string_lossy().into_owned()
}

struct Driver {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Json>,
    next: u64,
}

impl Driver {
    fn start(env: &[(&str, &str)]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zen-db-oracle"));
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("the driver program starts");
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
        let init = driver.answer(
            "initialize",
            json!({ "protocol": 1, "driver": "oracle", "client": { "name": "Zen Test", "version": "1" } }),
        );
        assert_eq!(init["result"]["driver"]["id"], "oracle");
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

    fn answer(&mut self, method: &str, params: Json) -> Json {
        let id = self.send(method, params);
        self.answer_to(id)
    }

    fn call(&mut self, method: &str, params: Json) -> Json {
        let answer = self.answer(method, params);
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        answer["result"].clone()
    }

    fn connect(&mut self, password: &str, settings: Json) -> Json {
        let mut settings = settings;
        if settings.get("program").is_none() {
            settings["program"] = json!(fake("sql"));
        }
        self.answer(
            "connect",
            json!({
                "connection": "c1", "context": "main", "host": "localhost", "port": 1521, "database": "FREEPDB1",
                "user": "zen", "password": password, "settings": settings, "state": { "autoCommit": true },
                "timeoutMs": 20_000,
            }),
        )
    }

    fn session(&mut self) -> String {
        let connected = self.connect("secret", json!({}));
        assert!(connected.get("error").is_none(), "{connected}");
        assert_eq!(connected["result"]["serverVersion"], "Oracle AI Database 26ai Free 23.26.0.0.0");
        connected["result"]["session"].as_str().unwrap().to_owned()
    }

    fn execute(&mut self, session: &str, statement: &str, max_rows: u64) -> Json {
        self.answer("execute", json!({ "session": session, "statement": statement, "maxRows": max_rows }))
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn data(answer: &Json) -> Json {
    answer["error"]["data"].clone()
}

#[test]
fn a_query_comes_back_typed_with_its_table_and_a_cap_on_rows() {
    let mut driver = Driver::start(&[]);
    let session = driver.session();
    let result = driver.execute(&session, "SELECT * FROM people;", 2)["result"].clone();
    let names: Vec<&str> = result["columns"].as_array().unwrap().iter().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(names, ["ID", "NAME", "BORN", "SALARY"]);
    assert_eq!(result["columns"][1]["type"], "varchar2");
    assert_eq!(result["columns"][0]["origin"], "ID");
    assert_eq!(result["columns"][3]["source"], json!({ "table": 73001, "position": 3 }));
    assert_eq!(result["truncated"], true);
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], json!({ "kind": "integer", "value": 1 }));
    assert_eq!(rows[0][2], json!({ "kind": "temporal", "value": { "text": "1815-12-10 00:00:00", "type": "date" } }));
    assert_eq!(rows[0][3], json!({ "kind": "decimal", "value": "1234567890123456789012.5" }));
    assert_eq!(rows[1][2], json!({ "kind": "null" }));
    assert_eq!(rows[1][3], json!({ "kind": "decimal", "value": "0.5" }));
    let all = driver.execute(&session, "SELECT * FROM people", 100)["result"].clone();
    assert_eq!((all["rows"].as_array().unwrap().len(), all["truncated"].as_bool()), (3, Some(false)));
}

#[test]
fn errors_say_where_and_commands_of_sqlcl_are_refused() {
    let mut driver = Driver::start(&[]);
    let session = driver.session();
    let failed = driver.execute(&session, "SELECT *\n  FROM nope", 10);
    assert_eq!(failed["error"]["message"], "ORA-00942: table or view \"ZEN\".\"NOPE\" does not exist");
    assert_eq!(data(&failed), json!({ "kind": "server", "code": "ORA-00942", "position": 17 }));
    let host = driver.execute(&session, "host rm -rf /", 10);
    assert_eq!(data(&host)["kind"], "invalid");
    driver.call("ping", json!({ "session": session }));
}

#[test]
fn manual_commit_opens_a_transaction_that_commit_ends() {
    let mut driver = Driver::start(&[]);
    let session = driver.session();
    let applied = driver
        .call("apply", json!({ "session": session, "state": { "autoCommit": false, "isolation": "SERIALIZABLE" } }));
    assert_eq!(applied["inTransaction"], false);
    let inserted = driver.execute(&session, "INSERT INTO people (name) VALUES ('Cy')", 10)["result"].clone();
    assert_eq!((inserted["affected"].as_u64(), inserted["inTransaction"].as_bool()), (Some(1), Some(true)));
    let read = driver.execute(&session, "SELECT * FROM people", 10)["result"].clone();
    assert_eq!(read["inTransaction"], true, "a query keeps the transaction open");
    assert_eq!(driver.call("commit", json!({ "session": session }))["inTransaction"], false);
    driver.execute(&session, "UPDATE people SET name = 'X'", 10);
    assert_eq!(driver.call("rollback", json!({ "session": session }))["inTransaction"], false);
    let created = driver.execute(&session, "CREATE TABLE t (a NUMBER)", 10)["result"].clone();
    assert_eq!(created["notices"][0]["message"], "Table T created.");
    driver.call("apply", json!({ "session": session, "state": { "autoCommit": true } }));
    assert_eq!(
        driver.execute(&session, "BEGIN", 1)["result"]["inTransaction"],
        true,
        "saving edits opens its own transaction"
    );
    driver.execute(&session, "SAVEPOINT zen_save", 1);
    assert_eq!(driver.execute(&session, "UPDATE people SET name = 'Y'", 1)["result"]["inTransaction"], true);
    assert!(driver.execute(&session, "RELEASE SAVEPOINT zen_save", 1).get("error").is_none());
    assert_eq!(driver.execute(&session, "COMMIT", 1)["result"]["inTransaction"], false);
    assert_eq!(
        driver.execute(&session, "UPDATE people SET name = 'Z'", 1)["result"]["inTransaction"],
        false,
        "autocommit is back"
    );
    let refused = driver.answer("apply", json!({ "session": session, "state": { "isolation": "READ UNCOMMITTED" } }));
    assert_eq!(data(&refused)["kind"], "unsupported");
}

#[test]
fn cancel_interrupts_the_statement_and_the_session_goes_on() {
    let mut driver = Driver::start(&[]);
    let session = driver.session();
    let started = Instant::now();
    let running = driver.send(
        "execute",
        json!({ "session": session, "statement": "BEGIN DBMS_SESSION.SLEEP(30); END;", "maxRows": 1 }),
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(driver.answer("cancel", json!({ "session": session })).get("error").is_none());
    let answer = driver.answer_to(running);
    assert_eq!(data(&answer)["kind"], "cancelled", "{answer}");
    assert!(started.elapsed() < Duration::from_secs(10));
    let read = driver.execute(&session, "SELECT * FROM people", 10);
    assert_eq!(read["result"]["rows"].as_array().map(Vec::len), Some(3), "{read}");
}

#[test]
fn the_structure_has_schemas_tables_keys_packages_and_sequences() {
    let mut driver = Driver::start(&[]);
    let session = driver.session();
    let read = driver.call(
        "introspect",
        json!({ "session": session, "schemas": [], "objects": [], "force": true, "system": false, "largeSchema": 2000, "outline": false, "fingerprints": {} }),
    );
    let catalog = &read["catalogs"][0];
    assert_eq!(catalog["name"], "FREEPDB1");
    let schemas = catalog["schemas"].as_array().unwrap();
    assert_eq!(schemas.len(), 1, "Oracle's own schemas are left out: {schemas:?}");
    let objects = schemas[0]["objects"].as_array().unwrap();
    let people = &objects[0];
    assert_eq!((people["kind"].as_str(), people["oid"].as_u64()), (Some("table"), Some(73001)));
    assert_eq!(people["columns"][0]["identity"], "byDefault");
    assert_eq!(people["columns"][1]["typeName"], "VARCHAR2(100 CHAR)");
    assert_eq!(people["constraints"][0]["kind"], "primaryKey");
    assert_eq!(people["constraints"].as_array().unwrap().len(), 1, "NOT NULL checks are columns' business");
    assert_eq!(objects[1]["kind"], "package");
    assert!(objects[1]["definition"].as_str().unwrap().starts_with("CREATE OR REPLACE PACKAGE payroll AS"));
    assert_eq!(objects[2]["sequence"]["cache"], "20");
    let again = driver.call(
        "introspect",
        json!({ "session": session, "schemas": [], "objects": [], "force": false, "system": false, "largeSchema": 2000, "outline": false,
                "fingerprints": { "FREEPDB1.ZEN": "3-20261031120000" } }),
    );
    assert_eq!(again["unchanged"], json!(["FREEPDB1.ZEN"]));
    let tree = driver.call("children", json!({ "session": session, "path": [] }));
    assert_eq!(tree["nodes"][0]["system"], true);
}

#[test]
fn a_wrong_password_is_an_authentication_error_that_never_shows_it() {
    let mut driver = Driver::start(&[]);
    let failed = driver.connect("not-it-at-all", json!({}));
    assert_eq!(data(&failed)["kind"], "authentication", "{failed}");
    assert!(!failed.to_string().contains("not-it-at-all"));
}

#[test]
fn the_password_never_reaches_the_command_line() {
    let file = std::env::temp_dir().join(format!("zen-fake-sqlcl-args-{}", std::process::id()));
    let mut driver = Driver::start(&[
        ("ZEN_FAKE_SQLCL_ARGS", file.to_str().unwrap()),
        ("SQLPATH", "/planted"),
        ("ORACLE_PATH", "/planted"),
    ]);
    let session = driver.session();
    let written = std::fs::read_to_string(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    let mut lines = written.lines();
    assert_eq!(lines.next(), Some("-S -R 3 -NOLOGINTIME /NOLOG"));
    let said: std::collections::HashMap<&str, &str> = lines.filter_map(|line| line.split_once('=')).collect();
    assert_eq!((said["sqlpath"], said["oracle_path"], said["listed"]), ("", "", ""), "{written}");
    let folder = std::path::Path::new(said["cwd"]);
    assert!(folder.file_name().unwrap().to_string_lossy().starts_with("zen-sqlcl-"), "{written}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(folder).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "only the user's");
    }
    let closed = driver.answer("close", json!({ "session": session }));
    assert!(closed.get("error").is_none(), "{closed}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while folder.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!folder.exists(), "removed when SQLcl ends");
}

#[test]
fn a_missing_sqlcl_or_java_says_so() {
    let mut driver = Driver::start(&[]);
    let missing = driver.connect("secret", json!({ "program": "/nowhere/sqlcl/bin/sql" }));
    assert_eq!(data(&missing)["code"], "sqlclMissing", "{missing}");
    assert!(missing["error"]["message"].as_str().unwrap().starts_with("SQLcl is missing"));
    let no_java = driver.connect("secret", json!({ "program": fake("no-java") }));
    assert_eq!(data(&no_java)["code"], "javaMissing", "{no_java}");
    assert!(no_java["error"]["message"].as_str().unwrap().starts_with("Java is missing"));
}

#[cfg(unix)]
#[test]
fn what_sqlcl_printed_before_it_ended_is_read_on_a_machine_with_java() {
    let home = std::env::temp_dir().join(format!("zen-java-home-{}", std::process::id()));
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::write(home.join("bin/java"), "#!/bin/sh\n").unwrap();
    for _ in 0..20 {
        let mut driver = Driver::start(&[("JAVA_HOME", home.to_str().unwrap())]);
        let no_java = driver.connect("secret", json!({ "program": fake("no-java") }));
        assert_eq!(data(&no_java)["code"], "javaMissing", "{no_java}");
    }
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_result_larger_than_a_statement_may_print_comes_back_cut() {
    let mut driver = Driver::start(&[("ZEN_DB_ORACLE_MOST_BYTES", "330")]);
    let session = driver.session();
    let read = driver.execute(&session, "SELECT * FROM people", 1000);
    assert!(read.get("error").is_none(), "{read}");
    let rows = read["result"]["rows"].as_array().unwrap();
    assert!(!rows.is_empty() && rows.len() < 3, "fewer rows, within the budget: {read}");
    assert_eq!(read["result"]["truncated"], true, "and said to be cut");
}
