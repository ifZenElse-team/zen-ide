use std::time::Duration;

use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::engine::{ConnectRequest, Engine, OperationState, OperationStatus, Timing};
use zen_db::registry::DataSource;
use zen_db::schema::{IntrospectRequest, LARGE_SCHEMA, Level, ObjectKind, Snapshot};
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(20);

fn port(variable: &str, default: u16) -> u16 {
    std::env::var(variable)
        .ok()
        .and_then(|ports| ports.split(',').filter_map(|port| port.trim().parse().ok()).next_back())
        .unwrap_or(default)
}

fn engine() -> Engine {
    Engine::with_timing(
        zen_db::drivers(),
        "Zen Test",
        Timing { backoff: [0, 100, 300].map(Duration::from_millis).to_vec(), ..Timing::default() },
    )
}

fn request(id: &str, driver: &str, port: u16, profile: Option<&str>, user: &str, database: &str) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: driver.to_owned(),
        profile: profile.map(str::to_owned),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        database: (!database.is_empty()).then(|| database.to_owned()),
        user: Some(user.to_owned()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None }
}

fn ok(engine: &Engine, op: u64) -> OperationState {
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state
}

fn run(engine: &Engine, id: &str, sql: &str) {
    ok(engine, engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap());
}

fn snapshot(engine: &Engine, id: &str, driver: &str) -> Snapshot {
    let request = IntrospectRequest { large_schema: LARGE_SCHEMA, ..IntrospectRequest::default() };
    let read = ok(engine, engine.introspect(id, request, Snapshot::new(id, driver), None).unwrap());
    engine.take_snapshot(read.id).unwrap()
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn postgresql_read_through_the_information_schema_as_cockroachdb_and_redshift_profiles_are() {
    let port = port("ZEN_DB_PG_PORTS", 54318);
    let engine = engine();
    ok(&engine, engine.connect(request("plain", "postgresql", port, None, "postgres", "postgres")).unwrap());
    run(&engine, "plain", "DROP SCHEMA IF EXISTS prof_shop CASCADE");
    run(&engine, "plain", "CREATE SCHEMA prof_shop");
    run(
        &engine,
        "plain",
        "CREATE TABLE prof_shop.orders (id bigint PRIMARY KEY, code text NOT NULL, total numeric, UNIQUE (code, total))",
    );
    run(&engine, "plain", "CREATE VIEW prof_shop.big AS SELECT id FROM prof_shop.orders WHERE total > 100");
    run(&engine, "plain", "CREATE SEQUENCE prof_shop.numbers");
    let plain = snapshot(&engine, "plain", "postgresql");
    assert_eq!(plain.profile, None, "a stock PostgreSQL is no profile");
    assert!(plain.server_version.as_deref().is_some_and(|version| version.starts_with("PostgreSQL 1")), "{plain:?}");
    assert_eq!(engine.status("plain").detected_profile, None);

    for profile in ["cockroachdb", "redshift"] {
        ok(
            &engine,
            engine.connect(request(profile, "postgresql", port, Some(profile), "postgres", "postgres")).unwrap(),
        );
        let read = snapshot(&engine, profile, "postgresql");
        assert_eq!(read.profile.as_deref(), Some(profile));
        assert_eq!(read.catalogs.len(), 1, "only the connection's database");
        let shop = read.schema("postgres", "prof_shop").unwrap();
        assert_eq!(shop.level, Level::Details);
        let kinds: Vec<(ObjectKind, &str)> =
            shop.objects.iter().map(|object| (object.kind, object.name.as_str())).collect();
        assert_eq!(kinds, [(ObjectKind::Table, "orders"), (ObjectKind::View, "big")], "{profile}");
        let orders = &shop.objects[0];
        let columns: Vec<(&str, &str, bool)> = orders
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column.type_name.as_str(), column.nullable))
            .collect();
        assert_eq!(columns, [("id", "bigint", false), ("code", "text", false), ("total", "numeric", true)]);
        let keys: Vec<&str> = orders.constraints.iter().map(|constraint| constraint.definition.as_str()).collect();
        assert!(keys.contains(&"PRIMARY KEY (id)") && keys.contains(&"UNIQUE (code, total)"), "{keys:?}");
        assert!(
            read.schema("postgres", "pg_catalog").is_some_and(|schema| schema.system && schema.level == Level::NotRead)
        );
        let folders = zen_db::schema::children(
            &read,
            &["postgres".to_owned(), "prof_shop".to_owned()],
            &zen_db::schema::TreeOptions::default(),
        )
        .unwrap();
        let labels: Vec<&str> = folders.iter().map(|node| node.label.as_str()).collect();
        assert_eq!(labels, ["Tables", "Views"], "{profile}: no sequences or functions folders");
        run(&engine, profile, "SELECT 1");
        assert_eq!(engine.status(profile).detected_profile, None, "its own profile is not news");
    }
    run(&engine, "plain", "DROP SCHEMA prof_shop CASCADE");
}

#[test]
#[ignore = "needs MySQL in Docker (docs/development.md)"]
fn mysql_read_without_fingerprints_as_starrocks_and_doris_profiles_are() {
    let port = port("ZEN_DB_MYSQL_PORT", 33084);
    let engine = engine();
    ok(&engine, engine.connect(request("rocks", "mysql", port, Some("starrocks"), "root", "")).unwrap());
    run(&engine, "rocks", "DROP DATABASE IF EXISTS prof_rocks");
    run(&engine, "rocks", "CREATE DATABASE prof_rocks");
    run(&engine, "rocks", "CREATE TABLE prof_rocks.events (id INT PRIMARY KEY, name VARCHAR(20))");
    let first = snapshot(&engine, "rocks", "mysql");
    assert_eq!(first.profile.as_deref(), Some("starrocks"));
    let schema = first.schema("def", "prof_rocks").unwrap();
    assert_eq!((schema.level, schema.fingerprint.as_deref()), (Level::Details, None));
    assert!(schema.objects.iter().any(|object| object.name == "events"));
    let folders = zen_db::schema::children(
        &first,
        &["def".to_owned(), "prof_rocks".to_owned()],
        &zen_db::schema::TreeOptions::default(),
    )
    .unwrap();
    let labels: Vec<&str> = folders.iter().map(|node| node.label.as_str()).collect();
    assert_eq!(labels, ["Tables", "Views"], "routines and events are hidden");
    assert!(first.server_version.as_deref().is_some_and(|version| version.starts_with("MySQL 8.4")), "{first:?}");
    run(&engine, "rocks", "DROP DATABASE prof_rocks");
}

#[test]
#[ignore = "needs Redis in Docker (docs/development.md)"]
fn a_redis_server_tested_as_a_valkey_profile_says_it_is_redis() {
    let port = port("ZEN_DB_REDIS_PORTS", 63808);
    let engine = engine();
    let mut tested = request("vk", "redis", port, Some("valkey"), "default", "");
    tested.source.user = None;
    let state = ok(&engine, engine.test(tested).unwrap());
    let report = state.test.unwrap();
    assert!(report.server_version.as_deref().is_some_and(|version| version.starts_with("Redis ")), "{report:?}");
    assert_eq!(report.profile, None, "the server is Redis itself, whatever the connection says");
}
