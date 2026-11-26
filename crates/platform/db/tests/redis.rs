use std::time::{Duration, Instant};

use zen_db::driver::{ContextKind, ErrorKind, ExecOptions, KeyChange, KeyRequest};
use zen_db::engine::{ConnectRequest, Engine, OperationState, OperationStatus, Timing};
use zen_db::model::{Node, NodeKind};
use zen_db::registry::DataSource;
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::{Execution, KeyValue, Value};
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(20);

fn ports() -> Vec<u16> {
    std::env::var("ZEN_DB_REDIS_PORTS")
        .unwrap_or_else(|_| "63808,63809".to_owned())
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

fn request(id: &str, port: u16, password: &str, read_only: bool) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: "redis".to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        database: Some("0".to_owned()),
        read_only,
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new(password.to_owned())), stored: None, ssh_secret: None }
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

fn run(engine: &Engine, id: &str, command: &str) -> Execution {
    ok(engine, engine.execute(id, ContextKind::Main, command, ExecOptions::default()).unwrap()).execution.unwrap()
}

fn children(engine: &Engine, id: &str, path: &[&str]) -> Vec<Node> {
    let path = path.iter().map(|part| (*part).to_owned()).collect();
    ok(engine, engine.children(id, path).unwrap()).nodes.unwrap()
}

fn key(engine: &Engine, id: &str, path: &[&str], change: Option<KeyChange>) -> KeyValue {
    let path = path.iter().map(|part| (*part).to_owned()).collect();
    ok(engine, engine.key(id, path, KeyRequest { limit: 100, change }).unwrap()).key.unwrap()
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn labels(nodes: &[Node]) -> Vec<(String, NodeKind, Option<String>)> {
    nodes.iter().map(|node| (node.label.clone(), node.kind.clone(), node.detail.clone())).collect()
}

#[test]
#[ignore = "needs Redis and Valkey in Docker (docs/development.md)"]
fn keys_values_and_the_console() {
    for port in ports() {
        let engine = new_engine();
        let id = "cache";
        ok(&engine, engine.connect(request(id, port, "zen", false)).unwrap());
        let version = ok(&engine, engine.test(request("probe", port, "zen", false)).unwrap()).test.unwrap();
        let version = version.server_version.unwrap();
        assert!(version.starts_with("Redis ") || version.starts_with("Valkey "), "{version}");
        run(&engine, id, "FLUSHALL");
        run(&engine, id, "SET user:1 \"Ada Lovelace\"");
        run(&engine, id, "EXPIRE user:1 1000");
        run(&engine, id, "HSET user:2:profile name tea age 3");
        run(&engine, id, "RPUSH queue a b c");
        run(&engine, id, "SADD tags x y");
        run(&engine, id, "ZADD board 1 low 2.5 high");
        run(&engine, id, "XADD events 1-1 kind click");
        run(&engine, id, "SET doc '{\"a\": [1, 2]}'");
        run(&engine, id, "SET \"spaced key\" v");
        run(&engine, id, "SELECT 2");
        run(&engine, id, "SET other x");
        run(&engine, id, "SELECT 0");
        let bulk: Vec<String> = (0..2500).map(|index| format!("bulk:{index} {index}")).collect();
        run(&engine, id, &format!("MSET {}", bulk.join(" ")));
        run(&engine, id, "CONFIG RESETSTAT");

        let roots = children(&engine, id, &[]);
        assert_eq!(
            labels(&roots),
            vec![
                ("db0".to_owned(), NodeKind::Catalog, Some("2508".to_owned())),
                ("db2".to_owned(), NodeKind::Catalog, Some("1".to_owned()))
            ]
        );
        let level = children(&engine, id, &["db0"]);
        let found = labels(&level);
        assert!(found.contains(&("user".to_owned(), NodeKind::KeyPrefix, Some("2".to_owned()))), "{found:?}");
        assert!(found.contains(&("bulk".to_owned(), NodeKind::KeyPrefix, Some("2500".to_owned()))));
        for (name, kind) in
            [("queue", "list"), ("tags", "set"), ("board", "zset"), ("events", "stream"), ("doc", "string")]
        {
            assert!(found.contains(&(name.to_owned(), NodeKind::Key, Some(kind.to_owned()))), "{name}: {found:?}");
        }
        let users = children(&engine, id, &["db0", "user:"]);
        assert_eq!(users[0].path, ["db0", "user:", "2:"]);
        assert_eq!(labels(&users)[1], ("1".to_owned(), NodeKind::Key, Some("string".to_owned())));
        assert_eq!(children(&engine, id, &["db0", "bulk:"]).len(), 2500);
        let stats = run(&engine, id, "INFO commandstats");
        let stats = stats.rows[0][0].display().unwrap();
        assert!(stats.contains("cmdstat_scan"), "{stats}");
        assert!(!stats.contains("cmdstat_keys:"), "the tree never runs KEYS");

        let string = key(&engine, id, &["db0", "user:", "1"], None);
        assert_eq!((string.type_name.as_str(), string.length), ("string", Some(12)));
        assert_eq!(string.rows, vec![vec![text("Ada Lovelace")]]);
        assert!(string.ttl_ms.is_some_and(|ttl| ttl > 900_000 && ttl <= 1_000_000), "{:?}", string.ttl_ms);
        assert!(string.size.is_some() && string.encoding.is_some());
        let kept = key(&engine, id, &["db0", "user:", "1"], Some(KeyChange::Persist));
        assert_eq!(kept.ttl_ms, None);
        let expiring = key(&engine, id, &["db0", "user:", "1"], Some(KeyChange::Expire(60)));
        assert!(expiring.ttl_ms.is_some_and(|ttl| ttl <= 60_000));
        let hash = key(&engine, id, &["db0", "user:", "2:", "profile"], None);
        assert_eq!(hash.type_name, "hash");
        assert!(hash.rows.contains(&vec![text("name"), text("tea")]));
        let list = key(&engine, id, &["db0", "queue"], None);
        assert_eq!(list.rows[2], vec![Value::Integer(2), text("c")]);
        let set = key(&engine, id, &["db0", "tags"], None);
        assert_eq!((set.type_name.as_str(), set.length, set.rows.len()), ("set", Some(2), 2));
        let zset = key(&engine, id, &["db0", "board"], None);
        assert_eq!(zset.rows, vec![vec![text("low"), Value::Float(1.0)], vec![text("high"), Value::Float(2.5)]]);
        let stream = key(&engine, id, &["db0", "events"], None);
        assert_eq!(stream.rows, vec![vec![text("1-1"), Value::Map(vec![("kind".to_owned(), text("click"))])]]);
        let doc = key(&engine, id, &["db0", "doc"], None);
        assert_eq!(doc.rows, vec![vec![Value::Json("{\"a\": [1, 2]}".to_owned())]]);
        let spaced = key(&engine, id, &["db0", "spaced key"], None);
        assert_eq!(spaced.rows, vec![vec![text("v")]]);
        let bulk = key(&engine, id, &["db0", "bulk:", "7"], None);
        assert_eq!(bulk.rows, vec![vec![text("7")]]);
        let other = key(&engine, id, &["db2", "other"], None);
        assert_eq!(other.rows, vec![vec![text("x")]]);
        let missing = key(&engine, id, &["db0", "nothing"], None);
        assert_eq!((missing.type_name.as_str(), missing.rows.len()), ("none", 0));
        if run(&engine, id, "MODULE LIST").rows.iter().any(|row| format!("{row:?}").contains("ReJSON")) {
            run(&engine, id, "JSON.SET json:1 $ '{\"name\":\"tea\"}'");
            let json = key(&engine, id, &["db0", "json:", "1"], None);
            assert_eq!(json.type_name, "json");
            assert_eq!(json.rows, vec![vec![Value::Json("{\"name\":\"tea\"}".to_owned())]]);
        }

        let map = run(&engine, id, "HGETALL user:2:profile");
        assert_eq!(map.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(), ["field", "value"]);
        assert_eq!(map.rows.len(), 2);
        let items = run(&engine, id, "LRANGE queue 0 -1");
        assert_eq!(items.rows[0], vec![Value::Integer(1), text("a")]);
        assert_eq!(run(&engine, id, "GET nothing").rows, vec![vec![Value::Null]]);
        let failed =
            done(&engine, engine.execute(id, ContextKind::Main, "NOSUCHCOMMAND x", ExecOptions::default()).unwrap());
        let error = failed.error.unwrap();
        assert_eq!((error.kind, error.code.as_deref()), (ErrorKind::Server, Some("ERR")));
        let unbalanced =
            done(&engine, engine.execute(id, ContextKind::Main, "GET \"open", ExecOptions::default()).unwrap());
        assert_eq!(unbalanced.error.unwrap().kind, ErrorKind::Invalid);

        let console = ContextKind::Console("c".to_owned());
        let blocked = engine.execute(id, console.clone(), "BLPOP never 30", ExecOptions::default()).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let asked = Instant::now();
        assert!(engine.cancel(blocked));
        let cancelled = done(&engine, blocked);
        assert_eq!(cancelled.status, OperationStatus::Cancelled, "{cancelled:?}");
        assert!(asked.elapsed() < Duration::from_secs(2));
        let after = ok(&engine, engine.execute(id, console, "PING", ExecOptions::default()).unwrap());
        assert_eq!(after.execution.unwrap().rows, vec![vec![text("PONG")]]);

        let locked = "locked";
        ok(&engine, engine.connect(request(locked, port, "zen", true)).unwrap());
        assert_eq!(run(&engine, locked, "GET bulk:1").rows, vec![vec![text("1")]]);
        for writing in ["SET bulk:1 changed", "DEL bulk:1", "FLUSHALL", "CONFIG SET maxmemory 1", "EVAL \"return 1\" 0"]
        {
            let refused =
                done(&engine, engine.execute(locked, ContextKind::Main, writing, ExecOptions::default()).unwrap());
            let error = refused.error.clone().unwrap_or_else(|| panic!("{writing} ran: {refused:?}"));
            assert_eq!(error.code.as_deref(), Some("readOnly"), "{writing}: {error:?}");
        }
        assert_eq!(run(&engine, id, "GET bulk:1").rows, vec![vec![text("1")]]);
        let path = ["db0".to_owned(), "user:".to_owned(), "1".to_owned()].to_vec();
        let refused = done(
            &engine,
            engine.key(locked, path, KeyRequest { limit: 10, change: Some(KeyChange::Persist) }).unwrap(),
        );
        assert_eq!(refused.error.unwrap().code.as_deref(), Some("readOnly"));

        let wrong = done(&engine, engine.test(request("wrong", port, "nope", false)).unwrap());
        assert_eq!(wrong.error.unwrap().kind, ErrorKind::Authentication);
        run(&engine, id, "FLUSHALL");
    }
}

#[test]
#[ignore = "needs a Redis cluster in Docker (docs/development.md)"]
fn a_cluster_lists_its_primaries_and_their_keys() {
    let Ok(seeds) = std::env::var("ZEN_DB_REDIS_CLUSTER") else { return };
    let engine = new_engine();
    let mut connection = request("cluster", 0, "", false);
    connection.source.host = Some(seeds);
    connection.source.port = None;
    connection.source.settings.insert("mode".to_owned(), serde_json::json!("cluster"));
    connection.password = None;
    connection.source.auth = zen_db::registry::AuthKind::None;
    ok(&engine, engine.connect(connection).unwrap());
    for index in 0..300 {
        run(&engine, "cluster", &format!("SET item:{index} {index}"));
    }
    let primaries = children(&engine, "cluster", &[]);
    assert_eq!(primaries.len(), 3, "{primaries:?}");
    assert!(primaries.iter().all(|node| node.kind == NodeKind::Other("node".to_owned())));
    let mut total = 0;
    for primary in &primaries {
        let level = children(&engine, "cluster", &[primary.label.as_str()]);
        let items = level.iter().find(|node| node.label == "item").expect("an item prefix on every primary");
        total += items.detail.as_deref().unwrap().parse::<usize>().unwrap();
        let keys = children(&engine, "cluster", &[primary.label.as_str(), "item:"]);
        let first = keys[0].label.clone();
        let value = key(&engine, "cluster", &[primary.label.as_str(), "item:", first.as_str()], None);
        assert_eq!(value.rows, vec![vec![text(&first)]]);
    }
    assert_eq!(total, 300);
    run(&engine, "cluster", "FLUSHALL");
}

#[test]
#[ignore = "needs Redis with a Sentinel in Docker (docs/development.md)"]
fn sentinel_finds_the_primary() {
    let Ok(sentinels) = std::env::var("ZEN_DB_REDIS_SENTINEL") else { return };
    let engine = new_engine();
    let mut connection = request("sentinel", 0, "", false);
    connection.source.host = Some(sentinels.clone());
    connection.source.port = None;
    connection.source.url = Some(format!("redis+sentinel://{sentinels}/primary/1"));
    connection.source.database = Some("primary/1".to_owned());
    connection.password = None;
    connection.source.auth = zen_db::registry::AuthKind::None;
    ok(&engine, engine.connect(connection).unwrap());
    run(&engine, "sentinel", "SET found yes");
    let roots = children(&engine, "sentinel", &[]);
    assert!(roots.iter().any(|node| node.label == "db1"), "{roots:?}");
    assert_eq!(key(&engine, "sentinel", &["db1", "found"], None).rows, vec![vec![text("yes")]]);
    run(&engine, "sentinel", "FLUSHDB");
}

#[test]
#[ignore = "needs Redis and Valkey in Docker (docs/development.md)"]
fn a_database_chosen_with_select_stays_chosen_after_a_reconnect() {
    for port in ports() {
        let engine = new_engine();
        ok(&engine, engine.connect(request("sel", port, "zen", false)).unwrap());
        run(&engine, "sel", "DEL zen:selected");
        run(&engine, "sel", "SELECT 3");
        run(&engine, "sel", "SET zen:selected three");
        ok(&engine, engine.invalidate("sel", zen_db::engine::TransactionPolicy::Ask).unwrap());
        let read = run(&engine, "sel", "GET zen:selected");
        assert_eq!(read.rows, [[text("three")]], "the console is still in database 3");
        run(&engine, "sel", "DEL zen:selected");
        run(&engine, "sel", "SELECT 0");
        ok(&engine, engine.invalidate("sel", zen_db::engine::TransactionPolicy::Ask).unwrap());
        assert_eq!(run(&engine, "sel", "GET zen:selected").rows, [[Value::Null]]);
        ok(&engine, engine.disconnect("sel", zen_db::engine::TransactionPolicy::Rollback).unwrap());
    }
}
