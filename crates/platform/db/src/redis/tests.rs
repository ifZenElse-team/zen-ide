use super::*;
use crate::registry::DataSource;

fn source(
    host: &str,
    port: Option<u16>,
    database: Option<&str>,
    settings: serde_json::Value,
    url: Option<&str>,
) -> DataSource {
    DataSource {
        id: "cache".to_owned(),
        name: "Cache".to_owned(),
        driver: ID.to_owned(),
        host: Some(host.to_owned()),
        port,
        database: database.map(str::to_owned),
        url: url.map(str::to_owned),
        settings: settings.as_object().cloned().unwrap_or_default(),
        ..DataSource::default()
    }
}

#[test]
fn targets_read_hosts_databases_and_modes() {
    let plain = target(&source("localhost", Some(6380), Some("3"), serde_json::json!({}), None)).unwrap();
    assert_eq!(
        (plain.mode, plain.nodes.clone(), plain.db, plain.tls),
        (Mode::Standalone, vec![("localhost".to_owned(), 6380)], 3, false)
    );
    assert_eq!(plain.separator, ":");
    let tls =
        target(&source("cache.example.com", None, None, serde_json::json!({}), Some("rediss://cache.example.com")))
            .unwrap();
    assert!(tls.tls);
    assert_eq!(tls.nodes, vec![("cache.example.com".to_owned(), 6379)]);
    let sentinel = target(&source(
        "s1:26379,s2",
        None,
        Some("primary/2"),
        serde_json::json!({}),
        Some("redis+sentinel://s1:26379,s2/primary/2"),
    ))
    .unwrap();
    assert_eq!(sentinel.mode, Mode::Sentinel);
    assert_eq!(sentinel.nodes, vec![("s1".to_owned(), 26379), ("s2".to_owned(), 26379)]);
    assert_eq!((sentinel.master.as_str(), sentinel.db), ("primary", 2));
    let named =
        target(&source("s1", None, Some("1"), serde_json::json!({"mode": "sentinel", "master": "cache"}), None))
            .unwrap();
    assert_eq!((named.master.as_str(), named.db), ("cache", 1));
    let cluster = target(&source(
        "n1:7000,n2:7001",
        None,
        Some("5"),
        serde_json::json!({"mode": "cluster", "separator": "/"}),
        None,
    ))
    .unwrap();
    assert_eq!((cluster.mode, cluster.db, cluster.separator.as_str()), (Mode::Cluster, 0, "/"));
    assert_eq!(cluster.nodes.len(), 2);
    assert!(target(&source("localhost", None, Some("x"), serde_json::json!({}), None)).is_err());
}

#[test]
fn commands_are_classified_and_opened() {
    assert_eq!(classify("GET user:1"), StatementKind::Read);
    assert_eq!(classify("hgetall user:1"), StatementKind::Read);
    assert_eq!(classify("SET user:1 x"), StatementKind::Write);
    assert_eq!(classify("FLUSHALL"), StatementKind::Write);
    let driver = Redis;
    assert!(driver.changes_structure("DEL user:1"));
    assert!(!driver.changes_structure("GET user:1"));
    let path = ["db0".to_owned(), "user:".to_owned(), "1 2".to_owned()];
    assert_eq!(driver.open_text(&path, &NodeKind::Key).as_deref(), Some("TYPE \"user:1 2\"\n"));
    assert_eq!(driver.open_text(&path[..2], &NodeKind::KeyPrefix).as_deref(), Some("SCAN 0 MATCH user:* COUNT 100\n"));
}

#[test]
fn versions_name_the_server() {
    assert_eq!(
        version_of("# Server\r\nredis_version:8.0.2\r\nredis_mode:standalone\r\n").as_deref(),
        Some("Redis 8.0.2")
    );
    assert_eq!(
        version_of("# Server\r\nredis_version:7.2.4\r\nserver_name:valkey\r\nvalkey_version:8.1.1\r\n").as_deref(),
        Some("Valkey 8.1.1")
    );
    for (info, shown, profile) in [
        ("# Server\r\nredis_version:7.2.4\r\nserver_name:valkey\r\nvalkey_version:8.1.1\r\n", "Valkey 8.1.1", "valkey"),
        ("# Server\r\nredis_version:7.4.0\r\ndragonfly_version:df-v1.25.0\r\n", "Dragonfly df-v1.25.0", "dragonfly"),
        ("# Server\r\nredis_version:7.2.5\r\ngarnet_version:1.0.60\r\n", "Garnet 1.0.60", "garnet"),
        ("# Server\r\nredis_version:6.3.4\r\nexecutable:/usr/local/bin/keydb-server\r\n", "KeyDB 6.3.4", "keydb"),
    ] {
        assert_eq!(version_of(info).as_deref(), Some(shown));
        assert_eq!(crate::profile::detect(ID, shown, None).map(|found| found.id), Some(profile));
    }
    assert_eq!(crate::profile::detect(ID, "Redis 8.0.2", None), None);
}

#[test]
fn a_tunnel_keeps_checking_the_host_name_of_the_server() {
    let (via, hello) = crate::ssh::tests::client_hello();
    let mut source = source("cache.example.com", Some(6380), None, serde_json::json!({}), None);
    source.tls.mode = TlsMode::VerifyFull;
    let spec = ConnectSpec {
        source,
        password: None,
        context: crate::driver::ContextKind::Main,
        guards: crate::driver::Guards::default(),
        state: TrackedState::default(),
        timeout: Duration::from_secs(10),
        via: Some(via),
    };
    let failed = crate::engine::runtime().block_on(Redis.connect(&spec)).err().unwrap();
    let hello = hello.join().unwrap();
    assert!(crate::ssh::tests::names(&hello, "cache.example.com"), "{failed:?}");
    let target = target(&spec.source).unwrap();
    let addr = address(&spec.source, &target, "cache.example.com", 6380);
    assert!(!format!("{addr:?}").contains("danger_accept_invalid_hostnames: true"), "{addr:?}");
}
