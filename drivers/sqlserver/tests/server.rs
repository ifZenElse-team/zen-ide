use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

const WAIT: Duration = Duration::from_secs(60);

fn port() -> u16 {
    std::env::var("ZEN_DB_MSSQL_PORT").ok().and_then(|port| port.parse().ok()).unwrap_or(14333)
}

fn password() -> String {
    std::env::var("ZEN_DB_MSSQL_PASSWORD").unwrap_or_else(|_| "Zen-Test-2026!".to_owned())
}

struct Driver {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Json>,
    next: u64,
}

impl Driver {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zen-db-sqlserver"))
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
            json!({ "protocol": 1, "driver": "sqlserver", "client": { "name": "Zen Test", "version": "1" } }),
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

    fn connect_with(&mut self, tls: Json, settings: Json) -> Json {
        let id = self.send("connect", connect_params(tls, settings));
        self.answer_to(id)
    }

    fn session(&mut self) -> String {
        let deadline = Instant::now() + WAIT;
        loop {
            let answer = self.connect_with(json!({ "mode": "disable" }), json!({}));
            if let Some(session) = answer["result"]["session"].as_str() {
                return session.to_owned();
            }
            assert!(Instant::now() < deadline, "the server accepts a session: {answer}");
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    fn execute(&mut self, session: &str, statement: &str) -> Json {
        self.call("execute", json!({ "session": session, "statement": statement, "maxRows": 1000 }))
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn connect_params(tls: Json, settings: Json) -> Json {
    json!({
        "connection": "test", "context": "main", "host": "localhost", "port": port(), "database": "master",
        "user": "sa", "password": password(), "tls": tls, "settings": settings, "applicationName": "Zen Test",
        "state": { "autoCommit": true, "readOnly": false }, "timeoutMs": 15000
    })
}

fn kind(answer: &Json) -> String {
    answer["error"]["data"]["kind"].as_str().unwrap_or_default().to_owned()
}

fn scratch(driver: &mut Driver, session: &str, name: &str) {
    driver.execute(session, &format!("IF DB_ID('{name}') IS NULL CREATE DATABASE [{name}]"));
    driver.call("apply", json!({ "session": session, "state": { "database": name } }));
}

#[test]
#[ignore = "needs SQL Server in Docker (docs/development.md)"]
fn values_come_back_typed() {
    let mut driver = Driver::start();
    let session = driver.session();
    let ran = driver.execute(
        &session,
        "SELECT CAST(42 AS int) AS i, CAST(12.50 AS decimal(10,2)) AS d, N'Ada' AS t, CAST(1 AS bit) AS b, \
         CAST('2026-10-27T10:11:12.1234567' AS datetime2) AS dt, \
         CAST('2026-10-27T10:00:00+02:00' AS datetimeoffset(0)) AS dto, \
         CAST('6F9619FF-8B86-D011-B42D-00C04FC964FF' AS uniqueidentifier) AS g, 0x00FF AS bin, NULL AS n",
    );
    let row = &ran["rows"][0];
    assert_eq!(row[0], json!({ "kind": "integer", "value": 42 }));
    assert_eq!(row[1], json!({ "kind": "decimal", "value": "12.50" }));
    assert_eq!(row[2]["value"], "Ada");
    assert_eq!(row[3]["value"], true);
    assert_eq!(row[4]["value"]["text"], "2026-10-27 10:11:12.1234567");
    assert_eq!(row[5]["value"]["text"], "2026-10-27 10:00:00 +02:00");
    assert_eq!(
        row[6]["value"].as_str().map(str::to_ascii_uppercase).as_deref(),
        Some("6F9619FF-8B86-D011-B42D-00C04FC964FF")
    );
    assert_eq!(row[7]["value"], "00ff");
    assert_eq!(row[8]["kind"], "null");
    assert_eq!(ran["columns"][1]["type"], "decimal(10,2)", "declared types come from the server");
}

#[test]
#[ignore = "needs SQL Server in Docker (docs/development.md)"]
fn tls_negotiated_in_the_prelogin_works_and_a_self_signed_certificate_fails_verification() {
    let mut driver = Driver::start();
    driver.session();
    let required = driver.connect_with(json!({ "mode": "require" }), json!({}));
    let session = required["result"]["session"].as_str().expect("TLS without checking the certificate").to_owned();
    let ran = driver.execute(&session, "SELECT encrypt_option FROM sys.dm_exec_connections WHERE session_id = @@SPID");
    assert_eq!(ran["rows"][0][0]["value"], "TRUE", "the connection is encrypted");
    let verified = driver.connect_with(json!({ "mode": "verify-full" }), json!({}));
    assert_eq!(kind(&verified), "tls", "{verified}");
}

#[test]
#[ignore = "needs SQL Server forcing encryption with a certificate signed by the CA in ZEN_DB_MSSQL_CA (docs/development.md)"]
fn a_server_that_forces_encryption_refuses_plain_sessions_and_verifies_against_its_ca() {
    let Ok(ca) = std::env::var("ZEN_DB_MSSQL_CA") else { return };
    let mut driver = Driver::start();
    let plain = driver.connect_with(json!({ "mode": "disable" }), json!({}));
    assert_eq!(kind(&plain), "tls", "{plain}");
    let verified = driver.connect_with(json!({ "mode": "verify-full", "caFile": ca }), json!({}));
    let session =
        verified["result"]["session"].as_str().unwrap_or_else(|| panic!("verify-full: {verified}")).to_owned();
    let ran = driver.execute(&session, "SELECT encrypt_option FROM sys.dm_exec_connections WHERE session_id = @@SPID");
    assert_eq!(ran["rows"][0][0]["value"], "TRUE");
    let other = driver.connect_with(json!({ "mode": "verify-full" }), json!({}));
    assert_eq!(kind(&other), "tls", "an unknown issuer: {other}");
}

#[test]
#[ignore = "needs SQL Server 2022 or newer with a certificate for TDS 8.0 and ZEN_DB_MSSQL_STRICT=1"]
fn strict_tls_comes_before_the_prelogin() {
    if std::env::var("ZEN_DB_MSSQL_STRICT").as_deref() != Ok("1") {
        return;
    }
    let mut driver = Driver::start();
    driver.session();
    let strict = driver.connect_with(json!({ "mode": "require", "direct": true }), json!({}));
    let session = strict["result"]["session"].as_str().unwrap_or_else(|| panic!("TDS 8.0: {strict}")).to_owned();
    let ran =
        driver.execute(&session, "SELECT protocol_version FROM sys.dm_exec_connections WHERE session_id = @@SPID");
    assert!(!ran["rows"].as_array().unwrap().is_empty());
}

#[test]
#[ignore = "needs SQL Server in Docker (docs/development.md)"]
fn a_wrong_password_and_a_wrong_statement_say_what_failed() {
    let mut driver = Driver::start();
    let session = driver.session();
    let mut params = connect_params(json!({ "mode": "disable" }), json!({}));
    params["password"] = json!("not the password");
    let id = driver.send("connect", params);
    let refused = driver.answer_to(id);
    assert_eq!(kind(&refused), "authentication", "{refused}");
    assert!(!refused.to_string().contains("not the password"), "the password is never echoed");
    let id = driver.send(
        "execute",
        json!({ "session": session, "statement": "SELECT 1;\nSELECT 2\nFROM no_such_table", "maxRows": 10 }),
    );
    let failed = driver.answer_to(id);
    assert_eq!(failed["error"]["data"]["code"], "208", "{failed}");
    assert_eq!(failed["error"]["data"]["position"], 11, "the start of the statement the server names by its line");
}

#[test]
#[ignore = "needs SQL Server in Docker (docs/development.md)"]
fn rows_are_paged_through_a_cursor_and_a_cursor_closed_early_leaves_the_session_usable() {
    let mut driver = Driver::start();
    let session = driver.session();
    let statement = "SELECT TOP 5000 ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) AS n FROM sys.all_objects a CROSS JOIN sys.all_objects b";
    let opened = driver.call("query", json!({ "session": session, "statement": statement, "skip": 0 }));
    let cursor = opened["cursor"].as_str().unwrap().to_owned();
    let mut seen = 0;
    loop {
        let page = driver.call("fetch", json!({ "session": session, "cursor": cursor, "count": 1000 }));
        seen += page["rows"].as_array().unwrap().len();
        if page["done"] == true {
            break;
        }
    }
    assert_eq!(seen, 5000);
    let skipped = driver.call("query", json!({ "session": session, "statement": statement, "skip": 4990 }));
    let cursor = skipped["cursor"].as_str().unwrap().to_owned();
    let page = driver.call("fetch", json!({ "session": session, "cursor": cursor, "count": 100 }));
    assert_eq!(page["rows"][0][0]["value"], 4991, "skipped rows are read and dropped");
    let again = driver.call("query", json!({ "session": session, "statement": statement, "skip": 0 }));
    let cursor = again["cursor"].as_str().unwrap().to_owned();
    driver.call("fetch", json!({ "session": session, "cursor": cursor, "count": 10 }));
    driver.call("closeCursor", json!({ "session": session, "cursor": cursor }));
    let after = driver.execute(&session, "SELECT 7");
    assert_eq!(after["rows"][0][0]["value"], 7, "the rest of the result was cancelled and drained");
    let truncated = driver.call("execute", json!({ "session": session, "statement": statement, "maxRows": 3 }));
    assert_eq!(truncated["truncated"], true);
    assert_eq!(driver.execute(&session, "SELECT 8")["rows"][0][0]["value"], 8);
}

#[test]
#[ignore = "needs SQL Server in Docker (docs/development.md)"]
fn cancel_and_a_timeout_stop_a_statement_and_the_session_goes_on() {
    let mut driver = Driver::start();
    let session = driver.session();
    let started = Instant::now();
    let running =
        driver.send("execute", json!({ "session": session, "statement": "WAITFOR DELAY '00:00:20'", "maxRows": 1 }));
    std::thread::sleep(Duration::from_millis(500));
    driver.call("ping", json!({ "session": session }));
    driver.call("cancel", json!({ "session": session }));
    let answer = driver.answer_to(running);
    assert_eq!(kind(&answer), "cancelled", "{answer}");
    assert!(started.elapsed() < Duration::from_secs(10));
    let id = driver.send(
        "execute",
        json!({ "session": session, "statement": "WAITFOR DELAY '00:00:20'", "maxRows": 1, "timeoutMs": 500 }),
    );
    assert_eq!(kind(&driver.answer_to(id)), "timeout");
    assert_eq!(driver.execute(&session, "SELECT 1")["rows"][0][0]["value"], 1);
}

#[test]
#[ignore = "needs SQL Server in Docker (docs/development.md)"]
fn manual_commit_keeps_changes_until_commit_or_rollback() {
    let mut driver = Driver::start();
    let session = driver.session();
    scratch(&mut driver, &session, "zen_tx");
    driver.execute(&session, "DROP TABLE IF EXISTS t; CREATE TABLE t (id int PRIMARY KEY, name nvarchar(20))");
    let applied = driver
        .call("apply", json!({ "session": session, "state": { "autoCommit": false, "isolation": "SERIALIZABLE" } }));
    assert_eq!(applied["inTransaction"], false);
    let inserted = driver.execute(&session, "INSERT INTO t VALUES (1, N'Ada')");
    assert_eq!(inserted["affected"], 1);
    assert_eq!(inserted["inTransaction"], true, "the first change opens a transaction");
    assert_eq!(driver.call("rollback", json!({ "session": session }))["inTransaction"], false);
    assert_eq!(driver.execute(&session, "SELECT COUNT(*) FROM t")["rows"][0][0]["value"], 0);
    driver.execute(&session, "INSERT INTO t VALUES (2, N'Grace')");
    driver.call("commit", json!({ "session": session }));
    driver.call("apply", json!({ "session": session, "state": { "autoCommit": true } }));
    assert_eq!(driver.execute(&session, "SELECT name FROM t")["rows"][0][0]["value"], "Grace");
}

#[test]
#[ignore = "needs SQL Server in Docker (docs/development.md)"]
fn bound_values_save_edits_and_results_name_the_columns_they_read() {
    let mut driver = Driver::start();
    let session = driver.session();
    scratch(&mut driver, &session, "zen_edit");
    driver.execute(&session, "DROP TABLE IF EXISTS people; CREATE TABLE people (id int PRIMARY KEY, name nvarchar(20), photo varbinary(10), born date)");
    driver.execute(&session, "INSERT INTO people VALUES (1, N'Ada', NULL, '1815-12-10')");
    let read =
        driver.call("query", json!({ "session": session, "statement": "SELECT id, name FROM people", "skip": 0 }));
    let table = read["columns"][1]["source"]["table"].as_u64().expect("the column names its table");
    assert_eq!(read["columns"][1]["origin"], "name");
    let oid = driver.execute(&session, "SELECT OBJECT_ID('dbo.people')")["rows"][0][0]["value"].as_u64().unwrap();
    assert_eq!(table, oid, "the table is named by its object_id, as introspection names it");
    driver.call("closeCursor", json!({ "session": session, "cursor": read["cursor"] }));
    let saved = driver.call(
        "execute",
        json!({
            "session": session, "maxRows": 10,
            "statement": "UPDATE [dbo].[people] SET [name] = @P1, [photo] = CONVERT(VARBINARY(MAX), @P2, 2), [born] = @P3 WHERE [id] = @P4",
            "params": ["Ada L.", "00ff", "1815-12-11", "1"]
        }),
    );
    assert_eq!(saved["affected"], 1);
    let row = &driver.execute(&session, "SELECT name, photo, born FROM people")["rows"][0];
    assert_eq!((row[0]["value"].as_str(), row[1]["value"].as_str()), (Some("Ada L."), Some("00ff")));
    assert_eq!(row[2]["value"]["text"], "1815-12-11");
}

#[test]
#[ignore = "needs SQL Server in Docker (docs/development.md)"]
fn introspection_reads_tables_keys_and_indexes_and_keeps_unchanged_schemas() {
    let mut driver = Driver::start();
    let session = driver.session();
    scratch(&mut driver, &session, "zen_shape");
    driver.execute(&session, "DROP VIEW IF EXISTS v; DROP TABLE IF EXISTS orders; DROP TABLE IF EXISTS customers");
    driver.execute(
        &session,
        "CREATE TABLE customers (id int IDENTITY PRIMARY KEY, email nvarchar(100) NOT NULL UNIQUE, total AS (id * 2))",
    );
    driver.execute(&session, "CREATE TABLE orders (id bigint PRIMARY KEY, customer int REFERENCES customers(id) ON DELETE CASCADE, amount decimal(10,2) CHECK (amount > 0))");
    driver.execute(&session, "CREATE INDEX orders_customer ON orders (customer)");
    driver.execute(&session, "CREATE VIEW v AS SELECT id FROM customers");
    let read = driver.call(
        "introspect",
        json!({ "session": session, "schemas": [], "objects": [], "force": true, "system": false, "largeSchema": 2000, "outline": false, "fingerprints": {} }),
    );
    let catalog = read["catalogs"].as_array().unwrap().iter().find(|c| c["current"] == true).unwrap().clone();
    assert_eq!(catalog["name"], "zen_shape");
    let dbo = catalog["schemas"].as_array().unwrap().iter().find(|s| s["name"] == "dbo").unwrap().clone();
    assert_eq!(dbo["level"], "details");
    let object = |name: &str| dbo["objects"].as_array().unwrap().iter().find(|o| o["name"] == name).unwrap().clone();
    let customers = object("customers");
    assert_eq!(customers["columns"][0]["identity"], "always");
    assert_eq!(customers["columns"][1]["typeName"], "nvarchar(100)");
    assert!(customers["columns"][2]["generated"].is_string(), "a computed column");
    let orders = object("orders");
    let kinds: Vec<&str> =
        orders["constraints"].as_array().unwrap().iter().filter_map(|c| c["kind"].as_str()).collect();
    assert!(kinds.contains(&"primaryKey") && kinds.contains(&"foreignKey") && kinds.contains(&"check"), "{kinds:?}");
    let foreign = orders["constraints"].as_array().unwrap().iter().find(|c| c["kind"] == "foreignKey").unwrap().clone();
    assert_eq!(foreign["references"]["table"], "customers");
    assert_eq!(foreign["references"]["onDelete"], "CASCADE");
    assert!(orders["indexes"].as_array().unwrap().iter().any(|index| index["name"] == "orders_customer"));
    assert_eq!(object("v")["kind"], "view");
    assert!(object("v")["definition"].as_str().unwrap().contains("CREATE VIEW"));
    let fingerprint = dbo["fingerprint"].as_str().unwrap().to_owned();
    let again = driver.call(
        "introspect",
        json!({ "session": session, "schemas": [], "objects": [], "force": false, "system": false, "largeSchema": 2000, "outline": false, "fingerprints": { "zen_shape.dbo": fingerprint } }),
    );
    assert_eq!(again["unchanged"], json!(["zen_shape.dbo"]));
    let tree = driver.call("children", json!({ "session": session, "path": ["zen_shape", "dbo"] }));
    assert!(tree["nodes"].as_array().unwrap().iter().any(|node| node["label"] == "orders" && node["kind"] == "table"));
    let columns = driver.call("children", json!({ "session": session, "path": ["zen_shape", "dbo", "orders"] }));
    assert_eq!(columns["nodes"][2]["detail"], "decimal(10,2)");
}
