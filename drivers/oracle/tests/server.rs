use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

const WAIT: Duration = Duration::from_secs(120);

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

struct Driver {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Json>,
    next: u64,
}

impl Driver {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zen-db-oracle"))
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
            json!({ "protocol": 1, "driver": "oracle", "client": { "name": "Zen Test", "version": "1" } }),
        );
        driver
    }

    fn send(&mut self, method: &str, params: Json) -> u64 {
        let id = self.next;
        self.next += 1;
        writeln!(self.input, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
        self.input.flush().unwrap();
        id
    }

    fn answer_to(&mut self, id: u64) -> Json {
        let deadline = Instant::now() + WAIT;
        loop {
            let message =
                self.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("an answer");
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

    fn connect(&mut self, database: &str, settings: Json) -> Json {
        let mut settings = settings;
        settings["program"] = json!(env("ZEN_DB_ORACLE_SQLCL", "sql"));
        self.answer(
            "connect",
            json!({
                "connection": "c1", "context": "main", "host": env("ZEN_DB_ORACLE_HOST", "localhost"),
                "port": env("ZEN_DB_ORACLE_PORT", "1521").parse::<u64>().unwrap(), "database": database,
                "user": env("ZEN_DB_ORACLE_USER", "zen"), "password": env("ZEN_DB_ORACLE_PASSWORD", "zen_secret"),
                "settings": settings, "state": { "autoCommit": true }, "timeoutMs": 60_000,
            }),
        )
    }

    fn session(&mut self) -> String {
        let connected = self.connect("FREEPDB1", json!({}));
        assert!(connected.get("error").is_none(), "{connected}");
        connected["result"]["session"].as_str().unwrap().to_owned()
    }

    fn run(&mut self, session: &str, statement: &str) -> Json {
        self.call("execute", json!({ "session": session, "statement": statement, "maxRows": 100 }))
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "needs Oracle in Docker and SQLcl"]
fn queries_come_back_typed_and_errors_say_where() {
    let mut driver = Driver::start();
    let session = driver.session();
    let result = driver.run(
        &session,
        "SELECT 42 AS n, 12345678901234567890.5 AS big, 'Ada' AS name, DATE '2026-10-31' AS day, \
         HEXTORAW('00FF') AS raw_value, CAST(NULL AS VARCHAR2(10)) AS nothing FROM dual",
    );
    let row = &result["rows"][0];
    assert_eq!(row[0], json!({ "kind": "integer", "value": 42 }));
    assert_eq!(row[1], json!({ "kind": "decimal", "value": "12345678901234567890.5" }));
    assert_eq!(row[2], json!({ "kind": "text", "value": "Ada" }));
    assert_eq!(row[3]["kind"], "temporal");
    assert_eq!(row[5], json!({ "kind": "null" }));
    let failed =
        driver.answer("execute", json!({ "session": session, "statement": "SELECT *\n  FROM nope", "maxRows": 1 }));
    assert_eq!(failed["error"]["data"]["code"], "ORA-00942", "{failed}");
    assert_eq!(failed["error"]["data"]["position"], 17, "{failed}");
}

#[test]
#[ignore = "needs Oracle in Docker and SQLcl"]
fn a_sid_and_a_connect_string_connect_too() {
    let mut driver = Driver::start();
    let sid = driver.connect("FREE", json!({ "connectionType": "sid" }));
    assert!(sid["error"]["data"]["kind"].is_string() || sid["result"]["session"].is_string(), "{sid}");
    let host = env("ZEN_DB_ORACLE_HOST", "localhost");
    let port = env("ZEN_DB_ORACLE_PORT", "1521");
    let custom =
        driver.connect("", json!({ "connectionType": "custom", "connectString": format!("{host}:{port}/FREEPDB1") }));
    assert!(custom.get("error").is_none(), "{custom}");
}

#[test]
#[ignore = "needs Oracle in Docker and SQLcl"]
fn plsql_units_run_and_the_navigator_lists_them() {
    let mut driver = Driver::start();
    let session = driver.session();
    let _ = driver.answer("execute", json!({ "session": session, "statement": "DROP TABLE zen_people", "maxRows": 1 }));
    driver.run(&session, "CREATE TABLE zen_people (id NUMBER PRIMARY KEY, name VARCHAR2(40 CHAR) NOT NULL)");
    driver.run(&session, "CREATE OR REPLACE PACKAGE zen_pay AS\n  PROCEDURE pay(n NUMBER);\nEND zen_pay;");
    driver.run(&session, "CREATE OR REPLACE PROCEDURE zen_hello IS\n  n NUMBER;\nBEGIN\n  n := 1;\nEND;");
    driver.run(&session, "CREATE SEQUENCE zen_seq");
    driver.run(&session, "BEGIN\n  INSERT INTO zen_people VALUES (1, 'Ada');\n\n  zen_hello;\nEND;");
    let read = driver.call(
        "introspect",
        json!({ "session": session, "schemas": ["ZEN"], "objects": [], "force": true, "system": false, "largeSchema": 2000, "outline": false, "fingerprints": {} }),
    );
    let objects = read["catalogs"][0]["schemas"][0]["objects"].as_array().unwrap().clone();
    let kind = |name: &str| objects.iter().find(|o| o["name"] == name).map(|o| o["kind"].as_str().unwrap().to_owned());
    assert_eq!(kind("ZEN_PEOPLE").as_deref(), Some("table"));
    assert_eq!(kind("ZEN_PAY").as_deref(), Some("package"));
    assert_eq!(kind("ZEN_HELLO").as_deref(), Some("procedure"));
    assert_eq!(kind("ZEN_SEQ").as_deref(), Some("sequence"));
    let selected = driver.run(&session, "SELECT * FROM zen_people");
    assert_eq!(selected["columns"][0]["origin"], "ID");
    let broken = driver.answer(
        "execute",
        json!({ "session": session, "statement": "CREATE OR REPLACE PROCEDURE zen_broken IS\nBEGIN\n  nope;\nEND;", "maxRows": 1 }),
    );
    assert!(broken["error"]["data"]["position"].is_u64(), "{broken}");
}

#[test]
#[ignore = "needs Oracle in Docker and SQLcl"]
fn manual_commit_and_rollback_are_seen_by_another_session() {
    let mut driver = Driver::start();
    let one = driver.session();
    let two = driver.session();
    let _ = driver.answer("execute", json!({ "session": one, "statement": "DROP TABLE zen_tx", "maxRows": 1 }));
    driver.run(&one, "CREATE TABLE zen_tx (a NUMBER)");
    driver.call("apply", json!({ "session": one, "state": { "autoCommit": false } }));
    assert_eq!(driver.run(&one, "INSERT INTO zen_tx VALUES (1)")["inTransaction"], true);
    assert_eq!(driver.run(&two, "SELECT COUNT(*) FROM zen_tx")["rows"][0][0]["value"], 0);
    driver.call("commit", json!({ "session": one }));
    assert_eq!(driver.run(&two, "SELECT COUNT(*) FROM zen_tx")["rows"][0][0]["value"], 1);
    driver.run(&one, "INSERT INTO zen_tx VALUES (2)");
    driver.call("rollback", json!({ "session": one }));
    assert_eq!(driver.run(&two, "SELECT COUNT(*) FROM zen_tx")["rows"][0][0]["value"], 1);
}

#[test]
#[ignore = "needs Oracle in Docker and SQLcl"]
fn cancel_stops_a_sleep_and_the_session_goes_on() {
    let mut driver = Driver::start();
    let session = driver.session();
    let started = Instant::now();
    let running = driver.send(
        "execute",
        json!({ "session": session, "statement": "BEGIN DBMS_SESSION.SLEEP(60); END;", "maxRows": 1 }),
    );
    std::thread::sleep(Duration::from_secs(2));
    driver.call("cancel", json!({ "session": session }));
    let answer = driver.answer_to(running);
    assert_eq!(answer["error"]["data"]["kind"], "cancelled", "{answer}");
    assert!(started.elapsed() < Duration::from_secs(30));
    driver.run(&session, "SELECT 1 FROM dual");
}
