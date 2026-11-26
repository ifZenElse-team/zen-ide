use std::sync::Arc;
use std::time::{Duration, Instant};

use zen_db::driver::{ContextKind, Driver, ErrorKind, ExecOptions};
use zen_db::engine::{ConnectRequest, Engine, OperationState, OperationStatus, Timing, TransactionPolicy};
use zen_db::model::NodeKind;
use zen_db::registry::{DataSource, POOLER, TlsMode};
use zen_db::types::{ConnectionType, DEVELOPMENT, PRODUCTION};
use zen_db::value::{Typed, Value};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(20);

fn ports() -> Vec<u16> {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
}

fn new_engine() -> Engine {
    Engine::with_timing(
        zen_db::drivers(),
        "Zen Test",
        Timing { backoff: [0, 100, 300].map(Duration::from_millis).to_vec(), ..Timing::default() },
    )
}

fn request(id: &str, port: u16, kind: &str) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: "postgresql".to_owned(),
        connection_type: kind.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        database: Some("postgres".to_owned()),
        user: Some("postgres".to_owned()),
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

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn values_come_back_exactly() {
    for port in ports() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("v", port, DEVELOPMENT)).unwrap());
        query(&engine, "v", ContextKind::Main, "SET TimeZone = 'UTC'");
        let got = query(
            &engine,
            "v",
            ContextKind::Main,
            "SELECT 1::int2, 2::int4, 9007199254740993::int8, 12.340::numeric(10,3), -0.000001::numeric, 1.5::float8, 'x'::text, \
             'a'::varchar, E'\\\\xdead'::bytea, true, date '2024-02-29', time '01:02:03.5', timetz '10:00:00+03', \
             timestamp '2024-01-01 10:00:00.25', timestamptz '2024-01-01 10:00:00+00', interval '1 year 2 mons 3 days 04:05:06', \
             'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::uuid, '{\"a\": 1}'::jsonb, '[1, 2]'::json, ARRAY[[1,2],[3,4]], \
             ARRAY['x', NULL]::text[], NULL::int, '192.168.0.1'::inet, 'NaN'::numeric, date 'infinity'",
        );
        let expected = vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(9_007_199_254_740_993),
            Value::Decimal("12.340".into()),
            Value::Decimal("-0.000001".into()),
            Value::Float(1.5),
            text("x"),
            text("a"),
            Value::Bytes(vec![0xde, 0xad]),
            Value::Bool(true),
            temporal("2024-02-29", "date"),
            temporal("01:02:03.5", "time"),
            temporal("10:00:00+03", "timetz"),
            temporal("2024-01-01 10:00:00.25", "timestamp"),
            temporal("2024-01-01 10:00:00+00", "timestamptz"),
            Value::Interval("1 year 2 mons 3 days 04:05:06".into()),
            Value::Uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into()),
            Value::Json("{\"a\": 1}".into()),
            Value::Json("[1, 2]".into()),
            Value::Array(vec![
                Value::Array(vec![Value::Integer(1), Value::Integer(2)]),
                Value::Array(vec![Value::Integer(3), Value::Integer(4)]),
            ]),
            Value::Array(vec![text("x"), Value::Null]),
            Value::Null,
            Value::Other(Typed { text: "192.168.0.1".into(), type_name: "inet".into() }),
            Value::Decimal("NaN".into()),
            temporal("infinity", "date"),
        ];
        assert_eq!(got.rows[0], expected, "port {port}");
        assert_eq!(got.columns[3].type_name, "numeric");
        let empty = query(&engine, "v", ContextKind::Main, "SELECT 1 AS one WHERE false");
        assert_eq!(empty.columns[0].name, "one", "columns come back without rows");
        let many = ok(
            &engine,
            engine
                .execute(
                    "v",
                    ContextKind::Main,
                    "SELECT generate_series(1, 1000)",
                    ExecOptions { max_rows: 10, timeout: None },
                )
                .unwrap(),
        );
        let many = many.execution.unwrap();
        assert_eq!((many.rows.len(), many.truncated), (10, true));
        let changed = query(&engine, "v", ContextKind::Main, "CREATE TEMP TABLE t AS SELECT 1 AS a UNION SELECT 2");
        assert_eq!(changed.affected, Some(2));
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn notices_application_name_and_guards() {
    for port in ports() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("g", port, PRODUCTION)).unwrap());
        let noticed = query(&engine, "g", ContextKind::Main, "DO $$ BEGIN RAISE NOTICE 'hello %', 42; END $$");
        assert_eq!(noticed.notices[0].message, "hello 42");
        assert_eq!(noticed.notices[0].severity, "NOTICE");
        assert_eq!(scalar(&engine, "g", ContextKind::Main, "SHOW application_name"), text("Zen Test"));
        assert_eq!(scalar(&engine, "g", ContextKind::Main, "SHOW statement_timeout"), text("5min"));
        assert_eq!(scalar(&engine, "g", ContextKind::Main, "SHOW lock_timeout"), text("0"));
        assert_eq!(scalar(&engine, "g", ContextKind::Metadata, "SHOW lock_timeout"), text("10s"));
        assert_eq!(
            scalar(&engine, "g", ContextKind::Metadata, "SHOW idle_in_transaction_session_timeout"),
            text("1min")
        );
        assert_eq!(scalar(&engine, "g", ContextKind::Metadata, "SHOW default_transaction_read_only"), text("on"));
        assert_eq!(engine.open_transactions("g"), vec![ContextKind::Main], "Production runs in a transaction");
        assert_eq!(engine.disconnect("g", TransactionPolicy::Ask).unwrap_err().kind, ErrorKind::OpenTransactions);
        ok(&engine, engine.disconnect("g", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn cancel_stops_the_server_within_a_second() {
    for port in ports() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("c", port, DEVELOPMENT)).unwrap());
        let op = engine.execute("c", ContextKind::Main, "SELECT pg_sleep(30)", ExecOptions::default()).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        assert!(engine.cancel(op));
        let state = done(&engine, op);
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert_eq!(state.status, OperationStatus::Cancelled);
        assert_eq!(state.error.unwrap().code, None, "the server confirmed it");
        assert_eq!(scalar(&engine, "c", ContextKind::Main, "SELECT 1"), Value::Integer(1));
        let timed = engine
            .execute(
                "c",
                ContextKind::Main,
                "SELECT pg_sleep(30)",
                ExecOptions { max_rows: 1, timeout: Some(Duration::from_millis(200)) },
            )
            .unwrap();
        assert_eq!(done(&engine, timed).error.unwrap().kind, ErrorKind::Timeout);
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_killed_session_reconnects_and_restores_its_schema() {
    for port in ports() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("k", port, DEVELOPMENT)).unwrap());
        query(&engine, "k", ContextKind::Main, "CREATE SCHEMA IF NOT EXISTS zen_kept");
        let tracked =
            zen_db::driver::TrackedState { schema: Some("zen_kept".into()), auto_commit: true, ..Default::default() };
        ok(&engine, engine.apply("k", ContextKind::Main, tracked).unwrap());
        let main = scalar(&engine, "k", ContextKind::Main, "SELECT pg_backend_pid()");
        let Value::Integer(pid) = main else { panic!("{main:?}") };
        let killer = new_engine();
        ok(&killer, killer.connect(request("killer", port, DEVELOPMENT)).unwrap());
        assert_eq!(
            scalar(&killer, "killer", ContextKind::Main, &format!("SELECT pg_terminate_backend({pid})")),
            Value::Bool(true)
        );
        std::thread::sleep(Duration::from_millis(200));
        let read = ok(
            &engine,
            engine.execute("k", ContextKind::Main, "SELECT current_schema()", ExecOptions::default()).unwrap(),
        );
        assert!(read.reconnected);
        assert_eq!(read.execution.unwrap().rows[0][0], text("zen_kept"));
        let Value::Integer(pid) = scalar(&engine, "k", ContextKind::Main, "SELECT pg_backend_pid()") else { panic!() };
        scalar(&killer, "killer", ContextKind::Main, &format!("SELECT pg_terminate_backend({pid})"));
        std::thread::sleep(Duration::from_millis(200));
        let before = scalar(
            &killer,
            "killer",
            ContextKind::Main,
            "SELECT count(*) FROM pg_namespace WHERE nspname = 'zen_written'",
        );
        let write = done(
            &engine,
            engine.execute("k", ContextKind::Main, "CREATE SCHEMA zen_written", ExecOptions::default()).unwrap(),
        );
        assert_eq!(
            write.status,
            OperationStatus::Done,
            "a closed session is noticed before the write is sent: {write:?}"
        );
        query(&engine, "k", ContextKind::Main, "DROP SCHEMA zen_written");
        assert_eq!(before, Value::Integer(0));
        query(&engine, "k", ContextKind::Main, "DROP SCHEMA zen_kept");
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn md5_scram_tls_and_the_tree() {
    for port in ports() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("a", port, DEVELOPMENT)).unwrap());
        query(&engine, "a", ContextKind::Main, "SET password_encryption = 'md5'");
        query(&engine, "a", ContextKind::Main, "DROP ROLE IF EXISTS zen_md5");
        query(&engine, "a", ContextKind::Main, "CREATE ROLE zen_md5 LOGIN PASSWORD 'old'");
        query(&engine, "a", ContextKind::Main, "CREATE TABLE IF NOT EXISTS public.zen_tree (id int, name text)");
        let mut md5 = request("md5", port, DEVELOPMENT);
        md5.source.user = Some("zen_md5".into());
        md5.password = Some(Secret::new("old".to_owned()));
        ok(&engine, engine.test(md5.clone()).unwrap());
        md5.password = Some(Secret::new("wrong".to_owned()));
        assert_eq!(done(&engine, engine.test(md5).unwrap()).error.unwrap().kind, ErrorKind::Authentication);

        let mut tls = request("tls", port, DEVELOPMENT);
        tls.source.tls.mode = TlsMode::Require;
        ok(&engine, engine.connect(tls.clone()).unwrap());
        assert_eq!(
            scalar(&engine, "tls", ContextKind::Main, "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()"),
            Value::Bool(true)
        );
        tls.source.tls.mode = TlsMode::VerifyFull;
        tls.source.id = "verify".into();
        assert_eq!(
            done(&engine, engine.test(tls.clone()).unwrap()).error.unwrap().kind,
            ErrorKind::Tls,
            "a self-signed certificate does not verify"
        );
        let mut direct = request("direct", port, DEVELOPMENT);
        direct.source.tls.mode = TlsMode::Require;
        direct.source.tls.direct = true;
        let direct = done(&engine, engine.test(direct).unwrap());
        if port == 54318 {
            assert_eq!(direct.status, OperationStatus::Done, "PostgreSQL 17+ takes direct TLS: {direct:?}");
        }

        let roots = ok(&engine, engine.children("a", Vec::new()).unwrap()).nodes.unwrap();
        assert!(roots.iter().any(|node| node.label == "postgres" && node.kind == NodeKind::Catalog));
        let schemas = ok(&engine, engine.children("a", vec!["postgres".into()]).unwrap()).nodes.unwrap();
        assert!(schemas.iter().any(|node| node.label == "public"));
        assert!(!schemas.iter().any(|node| node.label == "pg_catalog"));
        let path: Vec<String> = ["postgres", "public", "tables"].map(str::to_owned).to_vec();
        let tables = ok(&engine, engine.children("a", path.clone()).unwrap()).nodes.unwrap();
        assert!(tables.iter().any(|node| node.label == "zen_tree"));
        let mut table = path;
        table.push("zen_tree".into());
        let folders = ok(&engine, engine.children("a", table.clone()).unwrap()).nodes.unwrap();
        assert_eq!(folders[0].label, "Columns");
        table.push("columns".into());
        let columns = ok(&engine, engine.children("a", table).unwrap()).nodes.unwrap();
        let columns: Vec<_> = columns.iter().map(|node| (node.label.as_str(), node.detail.as_deref())).collect();
        assert_eq!(columns, [("id", Some("integer")), ("name", Some("text"))]);
        query(&engine, "a", ContextKind::Main, "DROP TABLE public.zen_tree");
        query(&engine, "a", ContextKind::Main, "DROP ROLE zen_md5");
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn behind_a_pooler_nothing_is_held() {
    for port in ports() {
        let engine = new_engine();
        let mut pooled = request("p", port, PRODUCTION);
        pooled.source.settings.insert(POOLER.to_owned(), true.into());
        ok(&engine, engine.connect(pooled).unwrap());
        assert!(engine.open_transactions("p").is_empty());
        assert_eq!(scalar(&engine, "p", ContextKind::Main, "SELECT 1"), Value::Integer(1));
        assert!(engine.open_transactions("p").is_empty(), "auto-commit behind a pooler");
        let begin = done(&engine, engine.execute("p", ContextKind::Main, "BEGIN", ExecOptions::default()).unwrap());
        assert_eq!(begin.error.unwrap().kind, ErrorKind::Unsupported);
        assert!(engine.open_transactions("p").is_empty());
        let empty = query(&engine, "p", ContextKind::Main, "SELECT 1 AS one");
        assert_eq!(empty.columns[0].name, "one");
    }
}

#[test]
fn the_driver_is_relational_and_says_so() {
    let driver: Arc<dyn Driver> = zen_db::drivers().remove(0);
    assert_eq!(driver.id(), "postgresql");
    assert_eq!(driver.capabilities().language, "sql:postgres");
    assert!(driver.capabilities().server_cancel);
}

const SHOP: &str = "
CREATE SCHEMA nav_shop;
COMMENT ON SCHEMA nav_shop IS 'The shop';
CREATE TYPE nav_shop.status AS ENUM ('new', 'paid');
CREATE DOMAIN nav_shop.price AS numeric(10, 2) NOT NULL CHECK (VALUE >= 0);
CREATE TABLE nav_shop.customers (id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY, name text NOT NULL COLLATE \"C\", email text UNIQUE);
COMMENT ON COLUMN nav_shop.customers.name IS 'Full name';
CREATE TABLE nav_shop.orders (
    id bigserial PRIMARY KEY,
    customer integer NOT NULL REFERENCES nav_shop.customers (id) ON DELETE CASCADE,
    status nav_shop.status NOT NULL DEFAULT 'new',
    total nav_shop.price,
    doubled numeric GENERATED ALWAYS AS (total * 2) STORED,
    CHECK (total < 1000000)
);
CREATE INDEX orders_customer ON nav_shop.orders (customer);
CREATE VIEW nav_shop.big_orders AS SELECT id, total FROM nav_shop.orders WHERE total > 100;
CREATE MATERIALIZED VIEW nav_shop.order_totals AS SELECT customer, sum(total) AS total FROM nav_shop.orders GROUP BY customer;
CREATE FUNCTION nav_shop.order_count(c integer) RETURNS bigint LANGUAGE sql STABLE AS 'SELECT count(*) FROM nav_shop.orders WHERE customer = c';
CREATE FUNCTION nav_shop.touch() RETURNS trigger LANGUAGE plpgsql AS 'BEGIN RETURN NEW; END';
CREATE TRIGGER orders_touch BEFORE UPDATE ON nav_shop.orders FOR EACH ROW EXECUTE FUNCTION nav_shop.touch();
ALTER TABLE nav_shop.orders ENABLE ROW LEVEL SECURITY;
CREATE POLICY own_orders ON nav_shop.orders FOR SELECT USING (customer > 0);
CREATE TABLE nav_shop.events (at date NOT NULL, what text) PARTITION BY RANGE (at);
CREATE TABLE nav_shop.events_2026 PARTITION OF nav_shop.events FOR VALUES FROM ('2026-01-01') TO ('2027-01-01');
CREATE SCHEMA nav_other;
CREATE TABLE nav_other.notes (id integer PRIMARY KEY);
";

const MORE: &str = "
CREATE SCHEMA nav_more;
CREATE EXTENSION IF NOT EXISTS postgres_fdw;
CREATE TABLE nav_more.items (id integer PRIMARY KEY, price numeric);
CREATE INDEX items_price ON nav_more.items (price);
CREATE VIEW nav_more.cheap AS SELECT id FROM nav_more.items WHERE price < 10;
CREATE MATERIALIZED VIEW nav_more.totals AS SELECT count(*) AS n FROM nav_more.items;
CREATE TABLE nav_more.log (what text);
CREATE RULE log_items AS ON INSERT TO nav_more.items DO ALSO INSERT INTO nav_more.log VALUES ('insert');
CREATE FUNCTION nav_more.add(numeric, numeric) RETURNS numeric LANGUAGE sql IMMUTABLE AS 'SELECT $1 + $2';
CREATE AGGREGATE nav_more.total(numeric) (SFUNC = nav_more.add, STYPE = numeric, INITCOND = '0');
CREATE SERVER nav_more_server FOREIGN DATA WRAPPER postgres_fdw OPTIONS (dbname 'postgres');
CREATE FOREIGN TABLE nav_more.remote (id integer) SERVER nav_more_server OPTIONS (table_name 'nothing');
CREATE FUNCTION nav_more.on_ddl() RETURNS event_trigger LANGUAGE plpgsql AS 'BEGIN END';
CREATE EVENT TRIGGER nav_more_ddl ON ddl_command_end EXECUTE FUNCTION nav_more.on_ddl();
CREATE ROLE nav_more_reader LOGIN CONNECTION LIMIT 3;
COMMENT ON ROLE nav_more_reader IS 'Reads reports';
";

const LESS: &str = "DROP EVENT TRIGGER IF EXISTS nav_more_ddl;
DROP SCHEMA IF EXISTS nav_more CASCADE;
DROP SERVER IF EXISTS nav_more_server CASCADE;
DROP EXTENSION IF EXISTS postgres_fdw;
DROP ROLE IF EXISTS nav_more_reader";

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn the_tree_lists_the_postgresql_objects_people_look_for() {
    use zen_db::schema::{
        AGGREGATES, EVENT_TRIGGERS, EXTENSIONS, IntrospectRequest, LARGE_SCHEMA, ObjectKind, ROLES, SCHEMA_INDEXES,
        Snapshot, TABLESPACES, TreeOptions, children, find_object,
    };
    for port in ports() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("more", port, DEVELOPMENT)).unwrap());
        for statement in LESS.split(";\n").chain(MORE.split(";\n")).map(str::trim).filter(|text| !text.is_empty()) {
            query(&engine, "more", ContextKind::Main, statement);
        }
        let request = IntrospectRequest { large_schema: LARGE_SCHEMA, ..IntrospectRequest::default() };
        let read = ok(&engine, engine.introspect("more", request, Snapshot::new("more", "postgresql"), None).unwrap());
        let snapshot = engine.take_snapshot(read.id).unwrap();
        let tree = |parts: &[&str]| {
            let path: Vec<String> = parts.iter().map(|part| (*part).to_owned()).collect();
            children(&snapshot, &path, &TreeOptions::default()).unwrap()
        };
        let labels = |parts: &[&str]| tree(parts).into_iter().map(|node| node.label).collect::<Vec<_>>();
        let database = labels(&["postgres"]);
        for wanted in ["nav_more", "Extensions", "Event Triggers", "Tablespaces", "Roles"] {
            assert!(database.iter().any(|label| label == wanted), "{wanted} in {database:?}");
        }
        assert!(labels(&["postgres", EXTENSIONS]).contains(&"postgres_fdw".to_owned()));
        assert_eq!(labels(&["postgres", EVENT_TRIGGERS]), ["nav_more_ddl"]);
        assert!(labels(&["postgres", TABLESPACES]).contains(&"pg_default".to_owned()));
        assert!(labels(&["postgres", ROLES]).contains(&"nav_more_reader".to_owned()));
        let catalog = snapshot.catalog("postgres").unwrap();
        let reader = catalog.roles.iter().find(|role| role.name == "nav_more_reader").unwrap();
        assert_eq!(
            (reader.login, reader.connection_limit, reader.comment.as_deref()),
            (true, 3, Some("Reads reports"))
        );
        let trigger = &catalog.event_triggers[0];
        assert_eq!((trigger.event.as_str(), trigger.function.as_str()), ("ddl_command_end", "nav_more.on_ddl"));
        assert!(catalog.tablespaces.iter().any(|tablespace| tablespace.size.is_some()), "superuser reads sizes");

        assert_eq!(labels(&["postgres", "nav_more", "foreignTables"]), ["remote"]);
        assert_eq!(labels(&["postgres", "nav_more", "materializedViews"]), ["totals"]);
        let indexes = labels(&["postgres", "nav_more", SCHEMA_INDEXES]);
        assert_eq!(indexes, ["items_pkey", "items_price"]);
        assert_eq!(labels(&["postgres", "nav_more", AGGREGATES]), ["total(numeric)"]);
        assert_eq!(labels(&["postgres", "nav_more", "functions"]), ["add(numeric, numeric)", "on_ddl()"]);
        let aggregate: Vec<String> = ["postgres", "nav_more", AGGREGATES, "total(numeric)"].map(str::to_owned).to_vec();
        assert_eq!(find_object(&snapshot, &aggregate).unwrap().object.kind, ObjectKind::Function);
        let items_tree = labels(&["postgres", "nav_more", "tables", "items"]);
        assert!(items_tree.contains(&"Policies".to_owned()) && items_tree.contains(&"Partitions".to_owned()));

        let schema = snapshot.schema("postgres", "nav_more").unwrap();
        let object = |name: &str| schema.objects.iter().find(|object| object.name == name).unwrap();
        let rule = &object("items").rules[0];
        assert_eq!((rule.name.as_str(), rule.event.as_str(), rule.instead), ("log_items", "insert", false));
        assert!(rule.definition.starts_with("CREATE RULE log_items AS"), "{}", rule.definition);
        let uses = |name: &str| {
            object(name)
                .dependencies
                .iter()
                .map(|dependency| {
                    format!("{} {}.{} {}", dependency.direction, dependency.schema, dependency.name, dependency.kind)
                })
                .collect::<Vec<_>>()
        };
        assert!(uses("cheap").contains(&"uses nav_more.items table".to_owned()), "{:?}", uses("cheap"));
        let items = uses("items");
        for wanted in ["usedBy nav_more.cheap view", "usedBy nav_more.totals materializedView"] {
            assert!(items.contains(&wanted.to_owned()), "{wanted} in {items:?}");
        }

        for statement in LESS.split(";\n") {
            query(&engine, "more", ContextKind::Main, statement.trim());
        }
        ok(&engine, engine.disconnect("more", TransactionPolicy::Rollback).unwrap());
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn introspection_reads_the_structure_into_a_snapshot_and_a_refresh_reads_only_what_changed() {
    use zen_db::schema::{
        ConstraintKind, DdlOptions, IntrospectRequest, LARGE_SCHEMA, Level, ObjectKind, Snapshot, TreeOptions,
        children, find_object, read_snapshot,
    };
    for port in ports() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("nav", port, DEVELOPMENT)).unwrap());
        query(&engine, "nav", ContextKind::Main, "DROP SCHEMA IF EXISTS nav_shop, nav_other CASCADE");
        for statement in SHOP.split(";\n").map(str::trim).filter(|statement| !statement.is_empty()) {
            query(&engine, "nav", ContextKind::Main, statement);
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("caches/databases/nav.snapshot");
        let request = IntrospectRequest { large_schema: LARGE_SCHEMA, ..IntrospectRequest::default() };
        let first = ok(
            &engine,
            engine.introspect("nav", request.clone(), Snapshot::new("nav", "postgresql"), Some(file.clone())).unwrap(),
        );
        let summary = first.introspected.clone().unwrap();
        assert!(summary.saved && summary.read.contains(&"postgres.nav_shop".to_owned()), "{summary:?}");
        let snapshot = engine.take_snapshot(first.id).unwrap();
        assert_eq!(read_snapshot(&file).as_ref(), Some(&snapshot), "the file holds what was read");
        assert!(snapshot.server_version.as_deref().is_some_and(|version| version.starts_with("PostgreSQL")));
        let shop = snapshot.schema("postgres", "nav_shop").unwrap();
        assert_eq!((shop.level, shop.comment.as_deref()), (Level::Details, Some("The shop")));
        assert!(
            snapshot
                .schema("postgres", "pg_catalog")
                .is_some_and(|schema| schema.system && schema.level == Level::NotRead)
        );
        let object = |kind: ObjectKind, name: &str| {
            shop.objects
                .iter()
                .find(|object| object.kind == kind && object.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        let orders = object(ObjectKind::Table, "orders");
        let columns: Vec<_> = orders
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_name.as_str(), column.nullable))
            .collect();
        assert_eq!(
            columns,
            [
                ("id", "bigint", false),
                ("customer", "integer", false),
                ("status", "nav_shop.status", false),
                ("total", "nav_shop.price", true),
                ("doubled", "numeric", true)
            ]
        );
        assert!(orders.columns[0].default.as_deref().is_some_and(|default| default.starts_with("nextval(")));
        assert!(
            orders.columns[4]
                .generated
                .as_deref()
                .is_some_and(|expression| expression.contains("total") && expression.contains("2"))
        );
        let customers = object(ObjectKind::Table, "customers");
        assert_eq!(customers.columns[0].identity.as_deref(), Some("always"));
        assert_eq!(customers.columns[1].collation.as_deref(), Some("pg_catalog.\"C\""));
        assert_eq!(customers.columns[1].comment.as_deref(), Some("Full name"));
        let fk = orders.constraints.iter().find(|constraint| constraint.kind == ConstraintKind::ForeignKey).unwrap();
        let reference = fk.references.as_ref().unwrap();
        assert_eq!(
            (reference.table.as_str(), reference.columns.as_slice(), reference.on_delete.as_str()),
            ("customers", &["id".to_owned()][..], "cascade")
        );
        assert!(orders.constraints.iter().any(|constraint| constraint.kind == ConstraintKind::Check));
        assert!(orders.indexes.iter().any(|index| index.name == "orders_customer" && index.columns == ["customer"]));
        assert!(orders.indexes.iter().any(|index| index.primary && index.constraint.as_deref() == Some("orders_pkey")));
        assert_eq!(orders.triggers[0].name, "orders_touch");
        assert!(
            orders.row_security && orders.policies[0].name == "own_orders" && orders.policies[0].command == "select"
        );
        assert!(orders.size.is_some_and(|size| size > 0));
        assert!(
            object(ObjectKind::View, "big_orders").definition.as_deref().is_some_and(|query| query.contains("> 100"))
        );
        assert!(object(ObjectKind::MaterializedView, "order_totals").detailed);
        let count = object(ObjectKind::Function, "order_count");
        assert_eq!(
            count.routine.as_ref().map(|routine| (routine.arguments.as_str(), routine.result.as_deref())),
            Some(("c integer", Some("bigint")))
        );
        assert_eq!(
            object(ObjectKind::Sequence, "orders_id_seq").sequence.as_ref().unwrap().owned_by.as_deref(),
            Some("nav_shop.orders.id")
        );
        assert_eq!(object(ObjectKind::Type, "status").type_info.as_ref().unwrap().labels, ["new", "paid"]);
        assert_eq!(
            object(ObjectKind::Type, "price").type_info.as_ref().unwrap().checks,
            ["CHECK (VALUE >= 0::numeric)"]
        );
        let partition = object(ObjectKind::Table, "events_2026").partitioning.clone().unwrap();
        assert_eq!(partition.parent.as_deref(), Some("nav_shop.events"));
        assert!(
            object(ObjectKind::Table, "events").partitioning.as_ref().unwrap().key.as_deref() == Some("RANGE (at)")
        );

        let tree = |snapshot: &Snapshot, parts: &[&str]| {
            let path: Vec<String> = parts.iter().map(|part| (*part).to_owned()).collect();
            children(snapshot, &path, &TreeOptions::default())
                .unwrap()
                .into_iter()
                .map(|node| node.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            tree(&snapshot, &["postgres", "nav_shop"]),
            [
                "Tables",
                "Foreign Tables",
                "Views",
                "Materialized Views",
                "Indexes",
                "Functions",
                "Sequences",
                "Types",
                "Aggregate Functions"
            ]
        );
        assert_eq!(
            tree(&snapshot, &["postgres", "nav_shop", "tables", "customers", "references"]),
            ["orders (orders_customer_fkey)"]
        );
        assert_eq!(tree(&snapshot, &["postgres", "nav_shop", "tables", "events", "partitions"]), ["events_2026"]);
        let driver = engine.driver("postgresql").unwrap();
        let path: Vec<String> = ["postgres", "nav_shop", "tables", "orders"].map(str::to_owned).to_vec();
        let ddl = driver.ddl(&snapshot, &path, &DdlOptions { nested: true, ..DdlOptions::default() }).unwrap();
        assert!(ddl.starts_with("CREATE TABLE \"nav_shop\".\"orders\" (\n    \"id\" bigint DEFAULT nextval("), "{ddl}");
        assert!(ddl.contains("CREATE TRIGGER orders_touch"), "{ddl}");
        let function = ["postgres", "nav_shop", "functions", "order_count(c integer)"].map(str::to_owned).to_vec();
        assert!(find_object(&snapshot, &function).is_some());
        assert!(
            driver
                .ddl(&snapshot, &function, &DdlOptions::default())
                .unwrap()
                .contains("CREATE OR REPLACE FUNCTION nav_shop.order_count")
        );

        let unchanged = ok(&engine, engine.introspect("nav", request.clone(), snapshot.clone(), None).unwrap());
        assert!(unchanged.introspected.unwrap().read.is_empty(), "nothing changed, nothing is read");
        let ddl_run = done(
            &engine,
            engine
                .execute(
                    "nav",
                    ContextKind::Main,
                    "ALTER TABLE nav_shop.customers ADD COLUMN phone text",
                    ExecOptions::default(),
                )
                .unwrap(),
        );
        assert_eq!(ddl_run.status, OperationStatus::Done);
        assert!(engine.take_events().iter().any(
            |event| matches!(event, zen_db::engine::Event::StructureChanged { connection } if connection == "nav")
        ));
        let refreshed =
            ok(&engine, engine.introspect("nav", request.clone(), snapshot.clone(), Some(file.clone())).unwrap());
        assert_eq!(
            refreshed.introspected.clone().unwrap().read,
            ["postgres.nav_shop"],
            "only the changed schema is read again"
        );
        let after = engine.take_snapshot(refreshed.id).unwrap();
        let customers = after
            .schema("postgres", "nav_shop")
            .unwrap()
            .objects
            .iter()
            .find(|object| object.name == "customers")
            .unwrap();
        assert_eq!(customers.columns.last().map(|column| column.name.as_str()), Some("phone"));
        assert_eq!(after.schema("postgres", "nav_other"), snapshot.schema("postgres", "nav_other"), "kept as it was");
        assert_eq!(read_snapshot(&file).as_ref(), Some(&after));

        let small = IntrospectRequest { large_schema: 3, force: true, ..request.clone() };
        let names = ok(&engine, engine.introspect("nav", small, Snapshot::new("nav", "postgresql"), None).unwrap());
        let names = engine.take_snapshot(names.id).unwrap();
        let shop = names.schema("postgres", "nav_shop").unwrap();
        assert_eq!(shop.level, Level::Names, "a large schema is read by names only");
        assert!(
            shop.objects.iter().all(|object| !object.detailed
                || object.kind == ObjectKind::Sequence
                || object.kind == ObjectKind::Type)
        );
        let on_demand =
            IntrospectRequest { objects: vec![("nav_shop".into(), "orders".into())], ..IntrospectRequest::default() };
        let detailed = ok(&engine, engine.introspect("nav", on_demand, names.clone(), None).unwrap());
        let detailed = engine.take_snapshot(detailed.id).unwrap();
        let orders = detailed
            .schema("postgres", "nav_shop")
            .unwrap()
            .objects
            .iter()
            .find(|object| object.name == "orders")
            .unwrap();
        assert!(orders.detailed && orders.columns.len() == 5, "details on demand");

        query(&engine, "nav", ContextKind::Main, "DROP SCHEMA nav_shop, nav_other CASCADE");
        ok(&engine, engine.disconnect("nav", TransactionPolicy::Rollback).unwrap());
    }
}
