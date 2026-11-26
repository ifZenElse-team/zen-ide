use std::time::{Duration, Instant};

use zen_db::driver::{ContextKind, ErrorKind, ExecOptions};
use zen_db::engine::{
    CommitMode, ConnectRequest, Engine, ModeChange, OperationState, OperationStatus, QueryOptions, QueryState,
    QueryStatement, StatementStatus, Timing, TransactionPolicy,
};
use zen_db::model::NodeKind;
use zen_db::registry::{DataSource, TlsMode};
use zen_db::results::{Paging, ReadState, StopReason};
use zen_db::schema::{
    ConstraintKind, DdlOptions, IntrospectRequest, LARGE_SCHEMA, Level, ObjectKind, Snapshot, TreeOptions, children,
};
use zen_db::types::{ConnectionType, DEVELOPMENT, PRODUCTION};
use zen_db::value::{Typed, Value};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Server {
    driver: &'static str,
    port: u16,
}

impl Server {
    fn mariadb(self) -> bool {
        self.driver == "mariadb"
    }
}

fn servers() -> Vec<Server> {
    std::env::var("ZEN_DB_MYSQL_SERVERS")
        .unwrap_or_else(|_| "mysql:33084,mariadb:33114".to_owned())
        .split(',')
        .filter_map(|pair| {
            let (driver, port) = pair.trim().split_once(':')?;
            let driver = if driver == "mariadb" { "mariadb" } else { "mysql" };
            Some(Server { driver, port: port.parse().ok()? })
        })
        .collect()
}

fn new_engine() -> Engine {
    Engine::with_timing(
        zen_db::drivers(),
        "Zen Test",
        Timing { backoff: [0, 100, 300].map(Duration::from_millis).to_vec(), ..Timing::default() },
    )
}

fn request(id: &str, server: Server, kind: &str) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: server.driver.to_owned(),
        connection_type: kind.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(server.port),
        user: Some("root".to_owned()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == kind).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None }
}

fn done(engine: &Engine, op: u64) -> OperationState {
    let state = engine.wait(op, WAIT).unwrap();
    assert_ne!(state.status, OperationStatus::Running);
    state
}

fn ok(engine: &Engine, op: u64) -> OperationState {
    let state = done(engine, op);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state
}

fn query(engine: &Engine, id: &str, context: ContextKind, sql: &str) -> zen_db::value::Execution {
    ok(engine, engine.execute(id, context, sql, ExecOptions::default()).unwrap()).execution.unwrap()
}

fn scalar(engine: &Engine, id: &str, context: ContextKind, sql: &str) -> Value {
    query(engine, id, context, sql).rows.remove(0).remove(0)
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn temporal(text: &str, type_name: &str) -> Value {
    Value::Temporal(Typed { text: text.to_owned(), type_name: type_name.to_owned() })
}

fn console(name: &str) -> ContextKind {
    ContextKind::Console(name.to_owned())
}

fn finished(engine: &Engine, op: u64) -> QueryState {
    let deadline = Instant::now() + WAIT;
    loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            return query;
        }
        assert!(Instant::now() < deadline, "{query:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn run(engine: &Engine, id: &str, context: &ContextKind, texts: &[&str]) -> QueryState {
    let mut at = 0;
    let statements = texts
        .iter()
        .map(|text| {
            let statement = QueryStatement {
                text: (*text).to_owned(),
                start: at,
                end: at + text.len(),
                client: false,
                params: Vec::new(),
                kept: None,
            };
            at += text.len() + 2;
            statement
        })
        .collect();
    let options = QueryOptions { rollback_on_error: false, ..QueryOptions::default() };
    finished(engine, engine.run_query(id, context.clone(), statements, options).unwrap())
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn values_come_back_exactly() {
    for server in servers() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("v", server, DEVELOPMENT)).unwrap());
        let main = || ContextKind::Main;
        query(&engine, "v", main(), "DROP DATABASE IF EXISTS zen_values");
        query(&engine, "v", main(), "CREATE DATABASE zen_values");
        query(&engine, "v", main(), "SET SESSION sql_mode = ''");
        query(
            &engine,
            "v",
            main(),
            "CREATE TABLE zen_values.t (i int, u bigint unsigned, d decimal(10,3), f double, c varchar(10), \
             dt date, zero datetime, ts datetime(3), tm time, y year, b1 bit(1), b10 bit(10), j json, \
             e enum('new','paid'), s set('a','b'), bin varbinary(4), bl blob, tx text)",
        );
        query(
            &engine,
            "v",
            main(),
            "INSERT INTO zen_values.t VALUES (-7, 18446744073709551615, 12.340, 1.5, 'x''y', '2024-02-29', \
             '0000-00-00 00:00:00', '2024-01-01 10:00:00.250', '-01:02:03', 2024, b'1', b'1000000001', \
             '{\"a\": 1}', 'paid', 'a,b', X'dead', X'00ff', 'long text')",
        );
        let got = query(&engine, "v", main(), "SELECT * FROM zen_values.t");
        let json = if server.mariadb() { text("{\"a\": 1}") } else { Value::Json("{\"a\": 1}".into()) };
        assert_eq!(
            got.rows[0],
            vec![
                Value::Integer(-7),
                Value::Decimal("18446744073709551615".into()),
                Value::Decimal("12.340".into()),
                Value::Float(1.5),
                text("x'y"),
                temporal("2024-02-29", "date"),
                temporal("0000-00-00 00:00:00", "datetime"),
                temporal("2024-01-01 10:00:00.250", "datetime"),
                temporal("-01:02:03", "time"),
                temporal("2024", "year"),
                Value::Bool(true),
                Value::Integer(513),
                json,
                text("paid"),
                text("a,b"),
                Value::Bytes(vec![0xde, 0xad]),
                Value::Bytes(vec![0x00, 0xff]),
                text("long text"),
            ],
            "{server:?}"
        );
        let types: Vec<&str> = got.columns.iter().map(|column| column.type_name.as_str()).collect();
        assert_eq!(&types[..4], ["int", "bigint unsigned", "decimal", "double"], "{server:?}");
        assert_eq!(types[13], "enum");
        let empty = query(&engine, "v", main(), "SELECT 1 AS one FROM DUAL WHERE 1 = 0");
        assert_eq!((empty.columns[0].name.as_str(), empty.rows.len()), ("one", 0));
        let many = ok(
            &engine,
            engine
                .execute(
                    "v",
                    main(),
                    "SELECT * FROM information_schema.COLUMNS",
                    ExecOptions { max_rows: 10, timeout: None },
                )
                .unwrap(),
        )
        .execution
        .unwrap();
        assert_eq!((many.rows.len(), many.truncated), (10, true));
        let changed = query(&engine, "v", main(), "UPDATE zen_values.t SET i = 1");
        assert_eq!(changed.affected, Some(1));
        let warned = query(&engine, "v", main(), "SELECT CAST('abc' AS SIGNED)");
        assert!(warned.notices.iter().any(|notice| notice.code.as_deref() == Some("1292")), "{warned:?}");
        query(&engine, "v", main(), "DROP DATABASE zen_values");
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn production_sessions_have_their_guards() {
    for server in servers() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("g", server, PRODUCTION)).unwrap());
        let timeout = if server.mariadb() {
            scalar(&engine, "g", ContextKind::Main, "SELECT @@max_statement_time")
        } else {
            scalar(&engine, "g", ContextKind::Main, "SELECT @@max_execution_time")
        };
        let expected = if server.mariadb() { Value::Float(300.0) } else { Value::Integer(300_000) };
        assert_eq!(timeout, expected, "{server:?}");
        assert_ne!(
            scalar(&engine, "g", ContextKind::Main, "SELECT @@lock_wait_timeout"),
            Value::Integer(10),
            "the person's SQL keeps the server's lock wait"
        );
        assert_eq!(scalar(&engine, "g", ContextKind::Metadata, "SELECT @@lock_wait_timeout"), Value::Integer(10));
        assert_eq!(
            scalar(&engine, "g", ContextKind::Metadata, "SELECT @@innodb_lock_wait_timeout"),
            Value::Integer(10)
        );
        assert_eq!(scalar(&engine, "g", ContextKind::Metadata, "SELECT @@transaction_read_only"), Value::Integer(1));
        assert_eq!(scalar(&engine, "g", ContextKind::Main, "SELECT @@autocommit"), Value::Integer(0));
        if !server.mariadb() {
            let mut plain = request("plain", server, DEVELOPMENT);
            plain.source.tls.mode = TlsMode::Disable;
            ok(&engine, engine.connect(plain).unwrap());
            let program = scalar(
                &engine,
                "plain",
                ContextKind::Main,
                "SELECT ATTR_VALUE FROM performance_schema.session_connect_attrs \
                 WHERE PROCESSLIST_ID = CONNECTION_ID() AND ATTR_NAME = 'program_name'",
            );
            assert_eq!(program, text("Zen Test"));
        }
        query(&engine, "g", ContextKind::Main, "SELECT COUNT(*) FROM mysql.db");
        assert_eq!(engine.open_transactions("g"), vec![ContextKind::Main], "Production runs in a transaction");
        query(&engine, "g", ContextKind::Main, "CREATE DATABASE IF NOT EXISTS zen_guard");
        query(&engine, "g", ContextKind::Main, "CREATE TEMPORARY TABLE zen_guard.t (a int) ENGINE = InnoDB");
        query(&engine, "g", ContextKind::Main, "INSERT INTO zen_guard.t VALUES (1)");
        assert_eq!(engine.disconnect("g", TransactionPolicy::Ask).unwrap_err().kind, ErrorKind::OpenTransactions);
        ok(&engine, engine.disconnect("g", TransactionPolicy::Rollback).unwrap());
        ok(&engine, engine.connect(request("cleanup", server, DEVELOPMENT)).unwrap());
        query(&engine, "cleanup", ContextKind::Main, "DROP DATABASE zen_guard");
    }
}

const HEAVY: &str =
    "SELECT COUNT(*) FROM information_schema.COLUMNS a, information_schema.COLUMNS b, information_schema.COLUMNS c";

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn kill_query_stops_the_server_within_a_second() {
    for server in servers() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("c", server, DEVELOPMENT)).unwrap());
        let op = engine
            .execute("c", ContextKind::Main, "SELECT SLEEP(30) FROM DUAL WHERE 1 = 1", ExecOptions::default())
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        assert!(engine.cancel(op));
        let state = done(&engine, op);
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert_eq!(state.status, OperationStatus::Cancelled, "{server:?} {state:?}");
        assert_eq!(state.error.unwrap().code, None, "the server confirmed it");
        assert_eq!(scalar(&engine, "c", ContextKind::Main, "SELECT 1"), Value::Integer(1));
        let timed = engine
            .execute(
                "c",
                ContextKind::Main,
                "SELECT SLEEP(30)",
                ExecOptions { max_rows: 1, timeout: Some(Duration::from_millis(200)) },
            )
            .unwrap();
        assert_eq!(done(&engine, timed).error.unwrap().kind, ErrorKind::Timeout);
        let context = console("c");
        let op = engine
            .run_query(
                "c",
                context.clone(),
                vec![QueryStatement {
                    text: HEAVY.into(),
                    start: 0,
                    end: HEAVY.len(),
                    client: false,
                    params: Vec::new(),
                    kept: None,
                }],
                QueryOptions::default(),
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(engine.cancel_query(op));
        assert_eq!(finished(&engine, op).status, OperationStatus::Cancelled);
        assert_eq!(run(&engine, "c", &context, &["SELECT 1"]).status, OperationStatus::Done, "the console works on");
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn passwords_plugins_and_tls() {
    for server in servers() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("a", server, DEVELOPMENT)).unwrap());
        let main = || ContextKind::Main;
        query(&engine, "a", main(), "DROP USER IF EXISTS 'zen_native'@'%'");
        query(
            &engine,
            "a",
            main(),
            if server.mariadb() {
                "CREATE USER 'zen_native'@'%' IDENTIFIED VIA mysql_native_password USING PASSWORD('old')"
            } else {
                "CREATE USER 'zen_native'@'%' IDENTIFIED WITH mysql_native_password BY 'old'"
            },
        );
        let mut users = vec!["zen_native"];
        if !server.mariadb() {
            query(&engine, "a", main(), "DROP USER IF EXISTS 'zen_sha2'@'%'");
            query(&engine, "a", main(), "CREATE USER 'zen_sha2'@'%' IDENTIFIED WITH caching_sha2_password BY 'old'");
            users.push("zen_sha2");
        }
        for user in users {
            let mut plain = request(&format!("plain-{user}"), server, DEVELOPMENT);
            plain.source.user = Some(user.into());
            plain.password = Some(Secret::new("old".to_owned()));
            plain.source.tls.mode = TlsMode::Disable;
            ok(&engine, engine.test(plain.clone()).unwrap());
            ok(&engine, engine.test(plain.clone()).unwrap());
            plain.password = Some(Secret::new("wrong".to_owned()));
            assert_eq!(done(&engine, engine.test(plain).unwrap()).error.unwrap().kind, ErrorKind::Authentication);
        }
        let mut tls = request("tls", server, DEVELOPMENT);
        tls.source.tls.mode = TlsMode::Require;
        ok(&engine, engine.connect(tls.clone()).unwrap());
        let cipher = query(&engine, "tls", main(), "SHOW SESSION STATUS LIKE 'Ssl_cipher'").rows.remove(0).remove(1);
        assert!(matches!(&cipher, Value::Text(name) if !name.is_empty()), "{cipher:?}");
        tls.source.tls.mode = TlsMode::VerifyFull;
        tls.source.id = "verify".into();
        assert_eq!(
            done(&engine, engine.test(tls.clone()).unwrap()).error.unwrap().kind,
            ErrorKind::Tls,
            "a self-signed certificate does not verify"
        );
        if let Ok(ca) = std::env::var(if server.mariadb() { "ZEN_DB_MARIADB_CA" } else { "ZEN_DB_MYSQL_CA" }) {
            tls.source.tls.mode = TlsMode::VerifyCa;
            tls.source.tls.ca_file = Some(ca.into());
            let checked = done(&engine, engine.test(tls).unwrap());
            assert!(
                checked.status == OperationStatus::Done
                    || checked.error.is_some_and(|error| error.kind == ErrorKind::Tls),
                "verify-ca checks the chain and never accepts less"
            );
        }
        query(&engine, "a", main(), "DROP USER IF EXISTS 'zen_native'@'%', 'zen_sha2'@'%'");
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn a_killed_session_reconnects_and_restores_its_database() {
    for server in servers() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("k", server, DEVELOPMENT)).unwrap());
        query(&engine, "k", ContextKind::Main, "CREATE DATABASE IF NOT EXISTS zen_kept");
        let tracked =
            zen_db::driver::TrackedState { schema: Some("zen_kept".into()), auto_commit: true, ..Default::default() };
        ok(&engine, engine.apply("k", ContextKind::Main, tracked).unwrap());
        let Value::Integer(id) = scalar(&engine, "k", ContextKind::Main, "SELECT CONNECTION_ID()") else { panic!() };
        let killer = new_engine();
        ok(&killer, killer.connect(request("killer", server, DEVELOPMENT)).unwrap());
        query(&killer, "killer", ContextKind::Main, &format!("KILL {id}"));
        std::thread::sleep(Duration::from_millis(200));
        let read =
            ok(&engine, engine.execute("k", ContextKind::Main, "SELECT DATABASE()", ExecOptions::default()).unwrap());
        assert!(read.reconnected, "{server:?}");
        assert_eq!(read.execution.unwrap().rows[0][0], text("zen_kept"));
        query(&engine, "k", ContextKind::Main, "DROP DATABASE zen_kept");
    }
}

const SHOP: &[&str] = &[
    "DROP DATABASE IF EXISTS nav_shop",
    "DROP DATABASE IF EXISTS nav_other",
    "CREATE DATABASE nav_shop",
    "CREATE TABLE nav_shop.customers (id int AUTO_INCREMENT PRIMARY KEY, name varchar(40) NOT NULL COMMENT 'Full name', \
     email varchar(80) COLLATE utf8mb4_bin, UNIQUE KEY customers_email (email)) COMMENT = 'People'",
    "CREATE TABLE nav_shop.orders (id bigint AUTO_INCREMENT PRIMARY KEY, customer int NOT NULL, \
     status enum('new','paid') NOT NULL DEFAULT 'new', total decimal(10,2), doubled decimal(12,2) AS (total * 2) STORED, \
     placed datetime DEFAULT CURRENT_TIMESTAMP, KEY orders_customer (customer), KEY orders_status (status, total), \
     CONSTRAINT orders_customer_fk FOREIGN KEY (customer) REFERENCES nav_shop.customers (id) ON DELETE CASCADE, \
     CONSTRAINT orders_total CHECK (total < 1000000))",
    "CREATE VIEW nav_shop.big_orders AS SELECT id, total FROM nav_shop.orders WHERE total > 100",
    "CREATE FUNCTION nav_shop.order_count(c int) RETURNS int READS SQL DATA \
     RETURN (SELECT COUNT(*) FROM nav_shop.orders WHERE customer = c)",
    "CREATE PROCEDURE nav_shop.close_orders(IN c int, OUT closed int) BEGIN \
     UPDATE nav_shop.orders SET status = 'paid' WHERE customer = c; SELECT ROW_COUNT() INTO closed; END",
    "CREATE TRIGGER nav_shop.orders_touch BEFORE UPDATE ON nav_shop.orders FOR EACH ROW SET NEW.placed = NOW()",
    "CREATE EVENT nav_shop.nightly ON SCHEDULE EVERY 1 DAY DISABLE COMMENT 'Clean up' DO DELETE FROM nav_shop.orders WHERE total = 0",
    "CREATE DATABASE nav_other",
    "CREATE TABLE nav_other.notes (id int PRIMARY KEY)",
];

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn introspection_reads_databases_and_a_refresh_reads_only_what_changed() {
    for server in servers() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("nav", server, DEVELOPMENT)).unwrap());
        for statement in SHOP {
            query(&engine, "nav", ContextKind::Main, statement);
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("caches/databases/nav.snapshot");
        let request = IntrospectRequest { large_schema: LARGE_SCHEMA, ..IntrospectRequest::default() };
        let first = ok(
            &engine,
            engine.introspect("nav", request.clone(), Snapshot::new("nav", server.driver), Some(file.clone())).unwrap(),
        );
        let summary = first.introspected.clone().unwrap();
        assert!(summary.saved && summary.read.contains(&"def.nav_shop".to_owned()), "{summary:?}");
        let snapshot = engine.take_snapshot(first.id).unwrap();
        let version = snapshot.server_version.clone().unwrap_or_default();
        assert!(version.starts_with(if server.mariadb() { "MariaDB 11" } else { "MySQL 8" }), "{version}");
        let shop = snapshot.schema("def", "nav_shop").unwrap();
        assert_eq!(shop.level, Level::Details);
        assert!(snapshot.schema("def", "mysql").is_some_and(|schema| schema.system && schema.level == Level::NotRead));
        let object = |kind: ObjectKind, name: &str| {
            shop.objects
                .iter()
                .find(|object| object.kind == kind && object.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        let orders = object(ObjectKind::Table, "orders");
        let columns: Vec<_> = orders.columns.iter().map(|column| (column.name.as_str(), column.nullable)).collect();
        assert_eq!(
            columns,
            [
                ("id", false),
                ("customer", false),
                ("status", false),
                ("total", true),
                ("doubled", true),
                ("placed", true)
            ]
        );
        assert_eq!(orders.columns[0].identity.as_deref(), Some("byDefault"));
        assert_eq!(orders.columns[2].type_name, "enum('new','paid')");
        assert_eq!(orders.columns[2].default.as_deref(), Some("'new'"));
        assert!(orders.columns[4].generated.as_deref().is_some_and(|expression| expression.contains("STORED")));
        let customers = object(ObjectKind::Table, "customers");
        assert_eq!(customers.comment.as_deref(), Some("People"));
        assert_eq!(customers.columns[1].comment.as_deref(), Some("Full name"));
        assert_eq!(customers.columns[2].collation.as_deref(), Some("utf8mb4_bin"));
        let fk = orders.constraints.iter().find(|constraint| constraint.kind == ConstraintKind::ForeignKey).unwrap();
        let reference = fk.references.as_ref().unwrap();
        assert_eq!(
            (
                reference.schema.as_str(),
                reference.table.as_str(),
                reference.columns.as_slice(),
                reference.on_delete.as_str()
            ),
            ("nav_shop", "customers", &["id".to_owned()][..], "cascade")
        );
        assert!(orders.constraints.iter().any(|constraint| constraint.kind == ConstraintKind::Check
            && constraint.name == "orders_total"
            && constraint.definition.contains("1000000")));
        assert!(
            orders.indexes.iter().any(|index| index.name == "orders_status" && index.columns == ["status", "total"])
        );
        assert!(orders.indexes.iter().any(|index| index.primary && index.constraint.as_deref() == Some("PRIMARY")));
        assert!(customers.indexes.iter().any(|index| index.unique && index.name == "customers_email"));
        assert_eq!(orders.triggers[0].name, "orders_touch");
        assert_eq!(orders.triggers[0].function, "BEFORE UPDATE");
        assert!(orders.definition.as_deref().is_some_and(|ddl| ddl.starts_with("CREATE TABLE `orders`")));
        let view = object(ObjectKind::View, "big_orders");
        assert!(view.detailed && view.columns.len() == 2);

        assert!(view.definition.as_deref().is_some_and(|ddl| ddl.contains("`big_orders` AS select")));
        let count = object(ObjectKind::Function, "order_count");
        assert_eq!(
            count.routine.as_ref().map(|routine| (routine.arguments.as_str(), routine.result.as_deref())),
            Some(if server.mariadb() { ("c int(11)", Some("int(11)")) } else { ("c int", Some("int")) })
        );
        let close = object(ObjectKind::Procedure, "close_orders");
        let arguments = if server.mariadb() { "IN c int(11), OUT closed int(11)" } else { "IN c int, OUT closed int" };
        assert_eq!(close.routine.as_ref().unwrap().arguments, arguments);
        let nightly = object(ObjectKind::Event, "nightly");
        assert_eq!(nightly.comment.as_deref(), Some("Clean up"));
        assert!(nightly.definition.as_deref().is_some_and(|ddl| ddl.contains("EVENT `nightly`")));

        let tree = |snapshot: &Snapshot, parts: &[&str]| {
            let path: Vec<String> = parts.iter().map(|part| (*part).to_owned()).collect();
            children(snapshot, &path, &TreeOptions::default())
                .unwrap()
                .into_iter()
                .map(|node| node.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(tree(&snapshot, &["def", "nav_shop"]), ["Tables", "Views", "Functions", "Procedures", "Events"]);
        assert_eq!(
            tree(&snapshot, &["def", "nav_shop", "tables", "customers", "references"]),
            ["orders (orders_customer_fk)"]
        );
        let driver = engine.driver(server.driver).unwrap();
        let path: Vec<String> = ["def", "nav_shop", "tables", "orders"].map(str::to_owned).to_vec();
        let ddl = driver.ddl(&snapshot, &path, &DdlOptions { nested: true, ..DdlOptions::default() }).unwrap();
        assert!(ddl.starts_with("CREATE TABLE `orders` ("), "{ddl}");
        assert!(ddl.contains("CREATE TRIGGER `orders_touch` BEFORE UPDATE ON `orders`"), "{ddl}");
        let procedure: Vec<String> =
            ["def", "nav_shop", "procedures", &format!("close_orders({arguments})")].map(str::to_owned).to_vec();
        let ddl = driver.ddl(&snapshot, &procedure, &DdlOptions::default()).unwrap();
        assert!(ddl.starts_with("DELIMITER $$\nCREATE DEFINER="), "{ddl}");
        assert!(ddl.ends_with("END$$\nDELIMITER ;\n"), "{ddl}");
        let script = zen_db::sql::split(&ddl, zen_db::sql::Dialect::MySql);
        let runnable: Vec<&str> = script
            .iter()
            .filter(|statement| !statement.client)
            .map(|statement| &ddl[statement.start..statement.end])
            .collect();
        assert_eq!(runnable.len(), 1, "{script:?}");

        let unchanged = ok(&engine, engine.introspect("nav", request.clone(), snapshot.clone(), None).unwrap());
        assert!(unchanged.introspected.unwrap().read.is_empty(), "nothing changed, nothing is read");
        let altered = done(
            &engine,
            engine
                .execute(
                    "nav",
                    ContextKind::Main,
                    "ALTER TABLE nav_shop.customers ADD COLUMN phone varchar(20)",
                    ExecOptions::default(),
                )
                .unwrap(),
        );
        assert_eq!(altered.status, OperationStatus::Done);
        assert!(engine.take_events().iter().any(
            |event| matches!(event, zen_db::engine::Event::StructureChanged { connection } if connection == "nav")
        ));
        let refreshed =
            ok(&engine, engine.introspect("nav", request.clone(), snapshot.clone(), Some(file.clone())).unwrap());
        assert_eq!(refreshed.introspected.clone().unwrap().read, ["def.nav_shop"], "only the changed database is read");
        let after = engine.take_snapshot(refreshed.id).unwrap();
        let customers =
            after.schema("def", "nav_shop").unwrap().objects.iter().find(|object| object.name == "customers").unwrap();
        assert_eq!(customers.columns.last().map(|column| column.name.as_str()), Some("phone"));
        assert_eq!(after.schema("def", "nav_other"), snapshot.schema("def", "nav_other"), "kept as it was");

        let small = IntrospectRequest { large_schema: 3, force: true, ..request.clone() };
        let names = ok(&engine, engine.introspect("nav", small, Snapshot::new("nav", server.driver), None).unwrap());
        let names = engine.take_snapshot(names.id).unwrap();
        let shop = names.schema("def", "nav_shop").unwrap();
        assert_eq!(shop.level, Level::Names, "a large database is read by names only");
        assert!(shop.objects.iter().all(|object| !object.detailed));
        let on_demand =
            IntrospectRequest { objects: vec![("nav_shop".into(), "orders".into())], ..IntrospectRequest::default() };
        let detailed = ok(&engine, engine.introspect("nav", on_demand, names.clone(), None).unwrap());
        let detailed = engine.take_snapshot(detailed.id).unwrap();
        let orders =
            detailed.schema("def", "nav_shop").unwrap().objects.iter().find(|object| object.name == "orders").unwrap();
        assert!(orders.detailed && orders.columns.len() == 6 && orders.definition.is_some(), "details on demand");

        let roots = ok(&engine, engine.children("nav", Vec::new()).unwrap()).nodes.unwrap();
        assert!(roots.iter().any(|node| node.label == "def" && node.kind == NodeKind::Catalog));
        let databases = ok(&engine, engine.children("nav", vec!["whatever".into()]).unwrap()).nodes.unwrap();
        assert!(databases.iter().any(|node| node.label == "nav_shop" && node.kind == NodeKind::Schema));
        assert!(!databases.iter().any(|node| node.label == "mysql"), "system databases are hidden");
        let path: Vec<String> = ["def", "nav_shop", "tables", "orders", "columns"].map(str::to_owned).to_vec();
        let columns = ok(&engine, engine.children("nav", path).unwrap()).nodes.unwrap();
        let first = columns.first().map(|node| (node.label.as_str(), node.detail.clone().unwrap_or_default()));
        assert!(first.is_some_and(|(label, detail)| label == "id" && detail.starts_with("bigint")), "{columns:?}");

        query(&engine, "nav", ContextKind::Main, "DROP DATABASE nav_shop");
        query(&engine, "nav", ContextKind::Main, "DROP DATABASE nav_other");
        ok(&engine, engine.disconnect("nav", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn a_hundred_thousand_rows_stream_within_the_cap_and_errors_say_where() {
    for server in servers() {
        let engine = new_engine();
        engine.results().set_paging(Paging { page: 200, row_cap: 10_000, keep_open: false });
        ok(&engine, engine.connect(request("p", server, DEVELOPMENT)).unwrap());
        let context = console("p");
        let setup = run(
            &engine,
            "p",
            &context,
            &[
                "DROP DATABASE IF EXISTS zen_big",
                "CREATE DATABASE zen_big",
                "USE zen_big",
                if server.mariadb() {
                    "SET SESSION max_recursive_iterations = 200000"
                } else {
                    "SET SESSION cte_max_recursion_depth = 200000"
                },
                "CREATE TABLE big (id int PRIMARY KEY, hash char(32))",
                "INSERT INTO big WITH RECURSIVE g (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM g WHERE n < 100000) \
                 SELECT n, MD5(n) FROM g",
            ],
        );
        assert!(setup.statements.iter().all(|statement| statement.status == StatementStatus::Done), "{setup:?}");
        let query = run(&engine, "p", &context, &["SELECT * FROM big ORDER BY id"]);
        let id = query.statements[0].result.unwrap();
        let result = engine.results().get(id).unwrap();
        let info = result.info();
        assert_eq!((info.rows, info.stopped_because, info.more), (10_000, Some(StopReason::Cap), true), "{info:?}");
        let op = engine.fetch_more(id, Some(200)).unwrap().expect("read again past the cap");
        assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
        assert_eq!(result.len(), 10_200);
        assert_eq!(result.get(10_199, 0), Value::Integer(10_200));
        let op = engine.fetch_more(id, None).unwrap().expect("Fetch All reads the rest");
        assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
        let info = result.info();
        assert_eq!((info.rows, info.more, info.state), (100_000, false, ReadState::Done));
        assert_eq!(result.get(99_999, 0), Value::Integer(100_000));
        engine.results().remove(id);
        let exact = run(&engine, "p", &context, &["SELECT id FROM big WHERE id <= 10000"]);
        let exact = engine.results().get(exact.statements[0].result.unwrap()).unwrap();
        assert_eq!((exact.len(), exact.info().more, exact.info().state), (10_000, false, ReadState::Done));
        let limited = run(&engine, "p", &context, &["SELECT COUNT(*) FROM big"]);
        let limited = engine.results().get(limited.statements[0].result.unwrap()).unwrap();
        assert_eq!(limited.get(0, 0), Value::Integer(100_000), "the row cap never limits what the server counts");

        let errors = run(&engine, "p", &context, &["SELEC 1", "SELECT idd FROM big"]);
        let syntax = errors.statements[0].error.clone().unwrap();
        assert_eq!((syntax.kind, syntax.code.as_deref(), syntax.position), (ErrorKind::Server, Some("1064"), Some(1)));
        let unknown = errors.statements[1].error.clone();
        assert!(unknown.is_none_or(|error| error.code.as_deref() == Some("1054")));
        run(&engine, "p", &context, &["DROP DATABASE zen_big"]);
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn manual_and_smart_commit_follow_the_servers_transaction() {
    for server in servers() {
        let engine = new_engine();
        let prod = request("tx", server, PRODUCTION);
        ok(&engine, engine.connect(prod.clone()).unwrap());
        let context = console("tx/console");
        let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
        assert_eq!(engine.set_mode("tx", context.clone(), auto.clone(), TransactionPolicy::Ask).unwrap(), None);
        run(
            &engine,
            "tx",
            &context,
            &["DROP DATABASE IF EXISTS zen_tx", "CREATE DATABASE zen_tx", "CREATE TABLE zen_tx.t (a int)"],
        );
        let manual = ModeChange { mode: Some(CommitMode::Manual), isolation: Some(Some("serializable".to_owned())) };
        ok(&engine, engine.set_mode("tx", context.clone(), manual, TransactionPolicy::Ask).unwrap().unwrap());
        run(&engine, "tx", &context, &["INSERT INTO zen_tx.t VALUES (1)"]);
        let state = engine.transaction(&prod, &context, 10);
        assert!(state.in_transaction && !state.auto_commit, "{state:?}");
        let isolation = run(&engine, "tx", &context, &["SELECT @@transaction_isolation"]);
        let isolation = engine.results().get(isolation.statements[0].result.unwrap()).unwrap();
        assert_eq!(isolation.get(0, 0), text("SERIALIZABLE"));
        let count = |engine: &Engine| scalar(engine, "tx", ContextKind::Metadata, "SELECT COUNT(*) FROM zen_tx.t");
        assert_eq!(count(&engine), Value::Integer(0), "another session does not see uncommitted rows");
        ok(&engine, engine.end_transaction("tx", context.clone(), true).unwrap());
        assert!(!engine.transaction(&prod, &context, 10).in_transaction);
        assert_eq!(count(&engine), Value::Integer(1));
        run(&engine, "tx", &context, &["DELETE FROM zen_tx.t"]);
        ok(&engine, engine.end_transaction("tx", context.clone(), false).unwrap());
        assert_eq!(count(&engine), Value::Integer(1), "the delete was rolled back");
        assert!(engine.set_mode("tx", context.clone(), auto, TransactionPolicy::Ask).is_ok());
        ok(&engine, engine.disconnect("tx", TransactionPolicy::Rollback).unwrap());

        let mut dev = request("smart", server, DEVELOPMENT);
        dev.kind.smart_commit = true;
        ok(&engine, engine.connect(dev.clone()).unwrap());
        let context = console("smart/console");
        run(&engine, "smart", &context, &["SELECT 1"]);
        assert!(engine.transaction(&dev, &context, 0).auto_commit, "a read does not switch");
        run(&engine, "smart", &context, &["INSERT INTO zen_tx.t VALUES (2)"]);
        let switched = engine.transaction(&dev, &context, 0);
        assert!(!switched.auto_commit && switched.in_transaction, "{switched:?}");
        ok(&engine, engine.end_transaction("smart", context.clone(), true).unwrap());
        assert!(engine.transaction(&dev, &context, 0).auto_commit, "Development returns to auto-commit");
        run(&engine, "smart", &context, &["DROP DATABASE zen_tx"]);
        ok(&engine, engine.disconnect("smart", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn a_failed_ddl_after_its_implicit_commit_shows_the_transaction_ended() {
    for server in servers() {
        let engine = new_engine();
        let prod = request("ddl", server, PRODUCTION);
        ok(&engine, engine.connect(prod.clone()).unwrap());
        let context = console("ddl/console");
        let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
        assert_eq!(engine.set_mode("ddl", context.clone(), auto.clone(), TransactionPolicy::Ask).unwrap(), None);
        run(
            &engine,
            "ddl",
            &context,
            &["DROP DATABASE IF EXISTS zen_ddl", "CREATE DATABASE zen_ddl", "CREATE TABLE zen_ddl.t (a int)"],
        );
        let manual = ModeChange { mode: Some(CommitMode::Manual), isolation: None };
        ok(&engine, engine.set_mode("ddl", context.clone(), manual, TransactionPolicy::Ask).unwrap().unwrap());
        run(&engine, "ddl", &context, &["INSERT INTO zen_ddl.t VALUES (1)"]);
        assert_eq!(run(&engine, "ddl", &context, &["SELECT * FROM zen_ddl.missing"]).status, OperationStatus::Failed);
        assert!(engine.transaction(&prod, &context, 0).in_transaction, "a failed read keeps the transaction");
        let failed = run(&engine, "ddl", &context, &["CREATE TABLE zen_ddl.t (a int)"]);
        assert_eq!(failed.status, OperationStatus::Failed, "{failed:?}");
        let state = engine.transaction(&prod, &context, 0);
        assert!(!state.in_transaction, "the DDL committed the insert before it failed: {state:?}");
        let count = scalar(&engine, "ddl", ContextKind::Metadata, "SELECT COUNT(*) FROM zen_ddl.t");
        assert_eq!(count, Value::Integer(1));
        ok(&engine, engine.set_mode("ddl", context.clone(), auto, TransactionPolicy::Rollback).unwrap().unwrap());
        run(&engine, "ddl", &context, &["DROP DATABASE zen_ddl"]);
        ok(&engine, engine.disconnect("ddl", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
fn the_drivers_are_relational_and_say_so() {
    let drivers = zen_db::drivers();
    for id in ["mysql", "mariadb"] {
        let driver = drivers.iter().find(|driver| driver.id() == id).unwrap();
        let capabilities = driver.capabilities();
        assert_eq!(capabilities.language, format!("sql:{id}"));
        assert_eq!(capabilities.uri_schemes, [id]);
        assert!(capabilities.server_cancel && capabilities.transactions && capabilities.schemas);
        assert_eq!(driver.default_port(), Some(3306));
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn spatial_values_read_as_geometry() {
    for server in servers() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("g", server, DEVELOPMENT)).unwrap());
        let polygon = scalar(
            &engine,
            "g",
            ContextKind::Main,
            "SELECT ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 1))', 4326)",
        );
        let found = zen_db::geometry::from_value(&polygon).unwrap_or_else(|| panic!("{server:?}: {polygon:?}"));
        let summary = found.summary();
        assert_eq!((summary.kind.as_str(), summary.srid, summary.points), ("POLYGON", Some(4326), 9), "{server:?}");
        let collection = scalar(
            &engine,
            "g",
            ContextKind::Main,
            "SELECT ST_GeomFromText('GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,3 3))')",
        );
        let found = zen_db::geometry::from_value(&collection).unwrap_or_else(|| panic!("{server:?}: {collection:?}"));
        assert_eq!(found.summary().points, 3, "{server:?}");
        let outlines = found.geometry.outlines();
        assert_eq!((outlines.points.len(), outlines.lines.len()), (1, 1), "{server:?}");
    }
}
