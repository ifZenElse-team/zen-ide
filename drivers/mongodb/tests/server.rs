use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

const WAIT: Duration = Duration::from_secs(60);

fn port() -> u16 {
    std::env::var("ZEN_DB_MONGO_PORT").ok().and_then(|port| port.parse().ok()).unwrap_or(27037)
}

fn password() -> String {
    std::env::var("ZEN_DB_MONGO_PASSWORD").unwrap_or_else(|_| "Zen-Test-2026!".to_owned())
}

struct Driver {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Json>,
    next: u64,
}

impl Driver {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zen-db-mongodb"))
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
            json!({ "protocol": 1, "driver": "mongodb", "client": { "name": "Zen Test", "version": "1" } }),
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

    fn connect(&mut self, params: Json) -> Json {
        let id = self.send("connect", params);
        self.answer_to(id)
    }

    fn session_with(&mut self, params: Json) -> String {
        let deadline = Instant::now() + WAIT;
        loop {
            let answer = self.connect(params.clone());
            if let Some(session) = answer["result"]["session"].as_str() {
                return session.to_owned();
            }
            assert!(Instant::now() < deadline, "the server accepts a session: {answer}");
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    fn session(&mut self) -> String {
        self.session_with(connect_params("zen_test"))
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

fn connect_params(database: &str) -> Json {
    json!({
        "connection": "test", "context": "main", "host": "127.0.0.1", "port": port(), "database": database,
        "user": "zen", "password": password(), "tls": { "mode": "disable" }, "settings": {},
        "applicationName": "Zen Test", "state": {}, "timeoutMs": 15000
    })
}

fn kind(answer: &Json) -> String {
    answer["error"]["data"]["kind"].as_str().unwrap_or_default().to_owned()
}

fn column(result: &Json, name: &str) -> usize {
    result["columns"].as_array().unwrap().iter().position(|column| column["name"] == name).unwrap()
}

fn fill(driver: &mut Driver, session: &str, collection: &str, count: usize) {
    driver.execute(session, &format!("db.{collection}.drop()"));
    for start in (0..count).step_by(1000) {
        let documents: Vec<String> = (start..count.min(start + 1000)).map(|n| format!("{{ n: {} }}", n + 1)).collect();
        driver.execute(session, &format!("db.{collection}.insertMany([{}])", documents.join(",")));
    }
}

#[test]
#[ignore = "needs MongoDB in Docker (docs/development.md)"]
fn documents_come_back_as_rows_with_typed_values_and_nested_ones_as_json() {
    let mut driver = Driver::start();
    let session = driver.session();
    driver.execute(&session, "db.people.drop()");
    let inserted = driver.execute(
        &session,
        "db.people.insertMany([{ _id: 1, name: 'Ada', born: ISODate('1815-12-10T00:00:00Z'), score: NumberDecimal('1.50'), \
         big: NumberLong(5), ok: true, address: { city: 'London' }, tags: ['math', 'poetry'] }, { _id: 2, name: 'Grace', extra: null }])",
    );
    assert_eq!(inserted["affected"], 2);
    let found = driver.execute(&session, "db.people.find({}).sort({ _id: 1 })");
    let row = &found["rows"][0];
    assert_eq!(found["columns"][0]["name"], "_id");
    assert_eq!(row[column(&found, "_id")], json!({ "kind": "integer", "value": 1 }));
    assert_eq!(row[column(&found, "name")]["value"], "Ada");
    assert_eq!(row[column(&found, "born")]["value"]["text"], "1815-12-10T00:00:00Z");
    assert_eq!(row[column(&found, "score")], json!({ "kind": "decimal", "value": "1.50" }));
    assert_eq!(row[column(&found, "ok")]["value"], true);
    assert_eq!(row[column(&found, "address")], json!({ "kind": "json", "value": "{\"city\":\"London\"}" }));
    assert_eq!(row[column(&found, "tags")]["kind"], "json");
    assert_eq!(found["rows"][1][column(&found, "born")]["kind"], "null", "a field a document lacks is null");
    let counted = driver.execute(&session, "db.people.countDocuments({ name: /^a/i })");
    assert_eq!(counted["rows"][0][0]["value"], 1);
    let updated = driver.execute(&session, "db.people.updateOne({ _id: 2 }, { $set: { name: 'Grace H.' } })");
    assert_eq!(updated["affected"], 1);
    let names = driver.execute(&session, "db.people.distinct('name')");
    assert_eq!(names["rows"].as_array().unwrap().len(), 2);
    let deleted = driver.execute(&session, "db.people.deleteMany({})");
    assert_eq!(deleted["affected"], 2);
}

#[test]
#[ignore = "needs MongoDB in Docker (docs/development.md)"]
fn the_statements_that_save_grid_edits_find_their_document_by_id_and_old_values() {
    let mut driver = Driver::start();
    let session = driver.session();
    let target = "db.getSiblingDB(\"zen_edits\").getCollection(\"people.v1\")";
    driver.execute(&session, &format!("{target}.drop()"));
    let id = "ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\")";
    let inserted = driver.execute(
        &session,
        &format!(
            "{target}.insertOne({{ \"_id\": {id}, \"name\": \"Ada\", \"age\": 36, \"views\": NumberLong(\"7\"), \
             \"meta\": {{\"at\":{{\"$date\":\"2026-10-28T00:00:00Z\"}},\"ref\":{{\"$oid\":\"6ae0b2ad2ee72a7b6191f5aa\"}}}} }})"
        ),
    );
    assert_eq!(inserted["affected"], 1);
    let back = driver.execute(&session, &format!("{target}.find({{ \"_id\": {id} }}, {{ \"_id\": 1 }})"));
    assert_eq!(back["columns"].as_array().unwrap().len(), 1);
    assert_eq!(back["rows"][0][0]["value"]["text"], id);
    let typed = driver.execute(
        &session,
        &format!("{target}.find({{ \"meta.at\": {{ $type: \"date\" }}, \"meta.ref\": {{ $type: \"objectId\" }} }})"),
    );
    assert_eq!(typed["rows"].as_array().unwrap().len(), 1, "Extended JSON in a saved document keeps its types");
    let stale = driver.execute(
        &session,
        &format!("{target}.updateOne({{ \"_id\": {id}, \"age\": 35 }}, {{ \"$set\": {{ \"age\": 37 }} }})"),
    );
    assert_eq!(stale["affected"], 0, "a document changed by someone else is not found");
    let same = driver.execute(
        &session,
        &format!("{target}.updateOne({{ \"_id\": {id}, \"age\": 36 }}, {{ \"$set\": {{ \"age\": 36 }} }})"),
    );
    assert_eq!(same["affected"], 1, "a value set to what it was still finds its document");
    let updated = driver.execute(
        &session,
        &format!(
            "{target}.updateOne({{ \"_id\": {id}, \"age\": 36 }}, {{ \"$set\": {{ \"age\": 37 }}, \"$unset\": {{ \"views\": \"\" }} }})"
        ),
    );
    assert_eq!(updated["affected"], 1);
    let found = driver.execute(&session, &format!("{target}.find({{}})"));
    assert_eq!(found["rows"][0][column(&found, "age")]["value"], 37);
    assert!(found["columns"].as_array().unwrap().iter().all(|column| column["name"] != "views"));
    let deleted = driver.execute(&session, &format!("{target}.deleteOne({{ \"_id\": {id} }})"));
    assert_eq!(deleted["affected"], 1);
    driver.execute(&session, "db.getSiblingDB(\"zen_edits\").dropDatabase()");
}

#[test]
#[ignore = "needs MongoDB in Docker (docs/development.md)"]
fn rows_are_paged_through_a_server_cursor_and_skipped_rows_are_read_on() {
    let mut driver = Driver::start();
    let session = driver.session();
    fill(&mut driver, &session, "numbers", 5000);
    let statement = "db.numbers.find({}, { _id: 0 }).sort({ n: 1 })";
    let opened = driver.call("query", json!({ "session": session, "statement": statement, "skip": 0 }));
    assert_eq!(opened["columns"][0]["name"], "n");
    let cursor = opened["cursor"].as_str().unwrap().to_owned();
    let mut seen = 0;
    loop {
        let page = driver.call("fetch", json!({ "session": session, "cursor": cursor, "count": 700 }));
        let rows = page["rows"].as_array().unwrap();
        if let Some(first) = rows.first() {
            assert_eq!(first[0]["value"], seen + 1, "pages follow each other");
        }
        seen += rows.len();
        if page["done"] == true {
            break;
        }
    }
    assert_eq!(seen, 5000);
    let skipped = driver.call("query", json!({ "session": session, "statement": statement, "skip": 4990 }));
    let cursor = skipped["cursor"].as_str().unwrap().to_owned();
    let page = driver.call("fetch", json!({ "session": session, "cursor": cursor, "count": 100 }));
    assert_eq!(page["rows"][0][0]["value"], 4991);
    assert_eq!(page["done"], true);
    let again = driver.call("query", json!({ "session": session, "statement": statement, "skip": 0 }));
    let cursor = again["cursor"].as_str().unwrap().to_owned();
    driver.call("fetch", json!({ "session": session, "cursor": cursor, "count": 10 }));
    driver.call("closeCursor", json!({ "session": session, "cursor": cursor }));
    let id = driver.send("fetch", json!({ "session": session, "cursor": cursor, "count": 10 }));
    assert_eq!(driver.answer_to(id)["error"]["code"], -32012, "a closed cursor is gone");
    let truncated = driver.call("execute", json!({ "session": session, "statement": statement, "maxRows": 3 }));
    assert_eq!((truncated["rows"].as_array().unwrap().len(), &truncated["truncated"]), (3, &json!(true)));
    let one = driver.execute(&session, "db.numbers.find().sort({ n: -1 }).limit(2)");
    assert_eq!((one["rows"].as_array().unwrap().len(), &one["truncated"]), (2, &json!(false)));
}

#[test]
#[ignore = "needs MongoDB in Docker (docs/development.md)"]
fn cancel_and_a_timeout_stop_a_statement_and_the_session_goes_on() {
    let mut driver = Driver::start();
    let session = driver.session();
    fill(&mut driver, &session, "slow", 200);
    let started = Instant::now();
    let slow = "db.slow.find({ $where: 'sleep(100) || true' })";
    let running = driver.send("execute", json!({ "session": session, "statement": slow, "maxRows": 1000 }));
    std::thread::sleep(Duration::from_millis(700));
    driver.call("ping", json!({ "session": session }));
    driver.call("cancel", json!({ "session": session }));
    let answer = driver.answer_to(running);
    assert_eq!(kind(&answer), "cancelled", "{answer}");
    assert!(started.elapsed() < Duration::from_secs(15));
    let operations = driver.execute(
        &session,
        "db.getSiblingDB('admin').aggregate([{ $currentOp: { allUsers: true } }, { $match: { 'command.filter.$where': { $exists: true } } }])",
    );
    assert_eq!(operations["rows"].as_array().unwrap().len(), 0, "the server stopped the operation: {operations}");
    let id =
        driver.send("execute", json!({ "session": session, "statement": slow, "maxRows": 1000, "timeoutMs": 500 }));
    assert_eq!(kind(&driver.answer_to(id)), "timeout");
    assert_eq!(driver.execute(&session, "db.slow.countDocuments({})")["rows"][0][0]["value"], 200);
}

#[test]
#[ignore = "needs MongoDB in Docker (docs/development.md)"]
fn a_wrong_password_a_wrong_statement_and_a_server_error_say_what_failed() {
    let mut driver = Driver::start();
    let session = driver.session();
    let mut params = connect_params("zen_test");
    params["password"] = json!("not the password");
    let refused = driver.connect(params);
    assert_eq!(kind(&refused), "authentication", "{refused}");
    assert!(!refused.to_string().contains("not the password"), "the password is never echoed");
    let id =
        driver.send("execute", json!({ "session": session, "statement": "db.people.find({ age: })", "maxRows": 10 }));
    let invalid = driver.answer_to(id);
    assert_eq!((kind(&invalid), &invalid["error"]["data"]["position"]), ("invalid".to_owned(), &json!(23)));
    let id = driver
        .send("execute", json!({ "session": session, "statement": "db.people.find({ $bogus: 1 })", "maxRows": 10 }));
    let failed = driver.answer_to(id);
    assert_eq!(kind(&failed), "server");
    assert_eq!(failed["error"]["data"]["code"], "2", "BadValue: {failed}");
    driver.execute(&session, "db.unique.drop()");
    driver.execute(&session, "db.unique.insertOne({ _id: 1 })");
    let id = driver
        .send("execute", json!({ "session": session, "statement": "db.unique.insertOne({ _id: 1 })", "maxRows": 10 }));
    assert_eq!(driver.answer_to(id)["error"]["data"]["code"], "11000", "a duplicate key");
}

#[test]
#[ignore = "needs MongoDB in Docker (docs/development.md)"]
fn a_read_only_session_reads_and_refuses_changes() {
    let mut driver = Driver::start();
    let mut params = connect_params("zen_test");
    params["readOnly"] = json!(true);
    let session = driver.session_with(params);
    driver.execute(&session, "db.people.find()");
    for change in [
        "db.people.insertOne({ a: 1 })",
        "db.people.aggregate([{ $out: 'copy' }])",
        "db.runCommand({ drop: 'people' })",
    ] {
        let id = driver.send("execute", json!({ "session": session, "statement": change, "maxRows": 10 }));
        assert_eq!(kind(&driver.answer_to(id)), "refused", "{change}");
    }
    driver.call("apply", json!({ "session": session, "state": { "readOnly": false } }));
    driver.execute(&session, "db.people.insertOne({ a: 1 })");
}

#[test]
#[ignore = "needs MongoDB in Docker (docs/development.md)"]
fn the_navigator_lists_databases_collections_views_and_indexes_and_use_switches_the_database() {
    let mut driver = Driver::start();
    let session = driver.session();
    driver.execute(&session, "use zen_tree");
    driver.execute(&session, "db.dropDatabase()");
    driver.execute(&session, "db.orders.insertOne({ total: 5 })");
    driver.execute(&session, "db.orders.createIndex({ total: -1 }, { unique: true })");
    driver.execute(&session, "db.createView('big_orders', 'orders', [{ $match: { total: { $gt: 3 } } }])");
    let roots = driver.call("children", json!({ "session": session, "path": [] }));
    let tree = roots["nodes"].as_array().unwrap();
    assert!(tree.iter().any(|node| node["label"] == "zen_tree" && node["kind"] == "catalog"));
    assert!(tree.iter().any(|node| node["label"] == "admin" && node["system"] == true));
    let collections = driver.call("children", json!({ "session": session, "path": ["zen_tree"] }));
    let nodes = collections["nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|node| node["label"] == "orders" && node["kind"] == "collection"));
    assert!(nodes.iter().any(|node| node["label"] == "big_orders" && node["kind"] == "view"));
    let indexes = driver.call("children", json!({ "session": session, "path": ["zen_tree", "orders"] }));
    let index = indexes["nodes"].as_array().unwrap().iter().find(|node| node["label"] == "total_-1").unwrap().clone();
    assert_eq!((index["kind"].as_str(), index["detail"].as_str()), (Some("index"), Some("{\"total\":-1} unique")));
    let shown = driver.execute(&session, "show collections");
    let names: Vec<&str> =
        shown["rows"].as_array().unwrap().iter().filter_map(|row| row[0]["value"].as_str()).collect();
    assert!(names.contains(&"orders") && names.contains(&"big_orders"), "{names:?}");
    driver.call("apply", json!({ "session": session, "state": { "database": "zen_test" } }));
    let other = driver.execute(&session, "db.orders.find()");
    assert!(other["rows"].as_array().unwrap().is_empty(), "the session moved to another database");
}

#[test]
#[ignore = "needs MongoDB in Docker (docs/development.md)"]
fn scram_sha_1_users_log_in_through_a_mongodb_address() {
    let mut driver = Driver::start();
    let session = driver.session();
    driver.execute(&session, "use zen_auth");
    let _ = driver.send(
        "execute",
        json!({ "session": session, "statement": "db.runCommand({ dropUser: 'old' })", "maxRows": 1 }),
    );
    driver.execute(
        &session,
        "db.runCommand({ createUser: 'old', pwd: 'p@ss:word', roles: ['readWrite'], mechanisms: ['SCRAM-SHA-1'] })",
    );
    let url = format!("mongodb://old:p%40ss%3Aword@127.0.0.1:{}/zen_auth?authSource=zen_auth", port());
    let old =
        driver.session_with(json!({ "url": url, "tls": { "mode": "prefer" }, "settings": {}, "timeoutMs": 15000 }));
    driver.execute(&old, "db.things.drop()");
    driver.execute(&old, "db.things.insertOne({ a: 1 })");
    assert_eq!(driver.execute(&old, "db.things.countDocuments()")["rows"][0][0]["value"], 1);
    let mut sha256 = connect_params("zen_test");
    sha256["settings"] = json!({ "authMechanism": "SCRAM-SHA-256" });
    driver.session_with(sha256);
}

#[test]
#[ignore = "needs MongoDB with TLS required and its CA in ZEN_DB_MONGO_CA (docs/development.md)"]
fn tls_is_used_from_the_first_byte_and_checked_against_the_ca() {
    let (Ok(ca), Ok(tls_port)) = (std::env::var("ZEN_DB_MONGO_CA"), std::env::var("ZEN_DB_MONGO_TLS_PORT")) else {
        return;
    };
    let mut driver = Driver::start();
    let mut params = connect_params("zen_test");
    params["host"] = json!("localhost");
    params["port"] = json!(tls_port.parse::<u16>().unwrap());
    params["tls"] = json!({ "mode": "disable" });
    assert!(driver.connect(params.clone()).get("error").is_some(), "the server refuses plain connections");
    params["tls"] = json!({ "mode": "verify-full", "caFile": ca });
    let session = driver.session_with(params.clone());
    let status = driver.execute(&session, "db.runCommand({ connectionStatus: 1 })");
    assert!(status["rows"][0].as_array().is_some());
    params["tls"] = json!({ "mode": "verify-full" });
    assert_eq!(kind(&driver.connect(params.clone())), "tls", "an unknown issuer");
    params["tls"] = json!({ "mode": "prefer" });
    driver.session_with(params);
}

#[test]
#[ignore = "needs MongoDB with TLS required, its CA in ZEN_DB_MONGO_CA and a client certificate in ZEN_DB_MONGO_CLIENT_PEM (docs/development.md)"]
fn x509_logs_in_with_the_client_certificate() {
    let (Ok(ca), Ok(tls_port), Ok(client)) = (
        std::env::var("ZEN_DB_MONGO_CA"),
        std::env::var("ZEN_DB_MONGO_TLS_PORT"),
        std::env::var("ZEN_DB_MONGO_CLIENT_PEM"),
    ) else {
        return;
    };
    let mut driver = Driver::start();
    let mut params = connect_params("zen_test");
    params["host"] = json!("localhost");
    params["port"] = json!(tls_port.parse::<u16>().unwrap());
    params["tls"] = json!({ "mode": "verify-full", "caFile": ca });
    let admin = driver.session_with(params.clone());
    let subject =
        std::env::var("ZEN_DB_MONGO_CLIENT_SUBJECT").unwrap_or_else(|_| "CN=zen,OU=clients,O=Zen Client".to_owned());
    let dropped = driver.send(
        "execute",
        json!({ "session": admin, "statement": format!("db.getSiblingDB('$external').runCommand({{ dropUser: '{subject}' }})") }),
    );
    driver.answer_to(dropped);
    driver.execute(
        &admin,
        &format!("db.getSiblingDB('$external').runCommand({{ createUser: '{subject}', roles: [{{ role: 'read', db: 'zen_test' }}] }})"),
    );
    params["user"] = Json::Null;
    params["password"] = Json::Null;
    params["settings"] = json!({ "authMechanism": "MONGODB-X509" });
    params["tls"] = json!({ "mode": "verify-full", "caFile": ca, "clientCert": client, "clientKey": client });
    let session = driver.session_with(params.clone());
    let status = driver.execute(&session, "db.runCommand({ connectionStatus: 1 })");
    let users = status["rows"][0][column(&status, "authInfo")]["value"].as_str().unwrap_or_default().to_owned();
    assert!(users.contains(&subject), "{status}");
    params["user"] = json!("CN=someone,O=Else");
    assert_eq!(kind(&driver.connect(params.clone())), "authentication", "a user the certificate does not name");
    params["tls"] = json!({ "mode": "verify-full", "caFile": ca });
    params["user"] = Json::Null;
    assert!(driver.connect(params).get("error").is_some(), "no client certificate, no X.509 login");
}

#[test]
#[ignore = "needs a MongoDB replica set member in ZEN_DB_MONGO_RS_PORT (docs/development.md)"]
fn manual_commit_runs_statements_in_a_transaction_that_commits_or_rolls_back() {
    let Ok(port) = std::env::var("ZEN_DB_MONGO_RS_PORT") else { return };
    let mut driver = Driver::start();
    let params = json!({
        "connection": "test", "context": "main", "host": "127.0.0.1", "port": port.parse::<u16>().unwrap(),
        "database": "zen_tx", "tls": { "mode": "disable" }, "settings": { "directConnection": "true" },
        "state": {}, "timeoutMs": 15000
    });
    let session = driver.session_with(params.clone());
    let initiated = driver.send(
        "execute",
        json!({ "session": session, "statement": "db.adminCommand({ replSetInitiate: { _id: 'rs0', members: [{ _id: 0, host: 'localhost:27017' }] } })" }),
    );
    driver.answer_to(initiated);
    let deadline = Instant::now() + WAIT;
    loop {
        let hello = driver.execute(&session, "db.runCommand({ hello: 1 })");
        if hello["rows"][0][column(&hello, "isWritablePrimary")]["value"] == true {
            break;
        }
        assert!(Instant::now() < deadline, "the member becomes primary: {hello}");
        std::thread::sleep(Duration::from_millis(500));
    }
    driver.execute(&session, "db.items.drop()");
    driver.execute(&session, "db.createCollection('items')");
    let other = driver.session_with(params);
    driver.call("apply", json!({ "session": session, "state": { "autoCommit": false } }));
    let inserted = driver.execute(&session, "db.items.insertOne({ _id: 1 })");
    assert_eq!((inserted["affected"].as_i64(), inserted["inTransaction"].as_bool()), (Some(1), Some(true)));
    let inside = driver.execute(&session, "db.items.find({})");
    assert_eq!(inside["rows"].as_array().unwrap().len(), 1, "the transaction sees its own change");
    let outside = driver.execute(&other, "db.items.find({})");
    assert_eq!(outside["rows"].as_array().unwrap().len(), 0, "another session does not");
    assert_eq!(driver.call("rollback", json!({ "session": session }))["inTransaction"], false);
    assert_eq!(driver.execute(&other, "db.items.find({})")["rows"].as_array().unwrap().len(), 0);
    driver.execute(&session, "db.items.insertOne({ _id: 2 })");
    driver.call("commit", json!({ "session": session }));
    let committed = driver.execute(&other, "db.items.find({})");
    assert_eq!(committed["rows"][0][0]["value"], 2, "{committed}");
    driver.call("apply", json!({ "session": session, "state": { "autoCommit": true } }));
    let plain = driver.execute(&session, "db.items.insertOne({ _id: 3 })");
    assert_eq!(plain["inTransaction"], false);
    driver.execute(&session, "db.dropDatabase()");
}

#[test]
fn a_closed_port_ends_the_connect_with_an_error() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut driver = Driver::start();
    let mut params = connect_params("zen_test");
    params["port"] = json!(closed);
    params["timeoutMs"] = json!(5000);
    let answer = driver.connect(params);
    assert_eq!(answer["error"]["data"]["kind"], "connection", "{answer}");
    assert!(answer["error"]["message"].as_str().unwrap().contains(&closed.to_string()), "{answer}");
}
