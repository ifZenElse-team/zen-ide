use super::*;
use crate::registry::SshSettings;

fn source(driver: &str) -> DataSource {
    DataSource {
        id: "c1".to_owned(),
        name: "Main".to_owned(),
        driver: driver.to_owned(),
        host: Some("db.example".to_owned()),
        port: Some(5433),
        user: Some("ann".to_owned()),
        database: Some("shop".to_owned()),
        ..DataSource::default()
    }
}

#[test]
fn psql_gets_its_settings_as_options_and_tls_in_the_environment() {
    let mut source = source("postgresql");
    source.tls.mode = TlsMode::VerifyFull;
    source.tls.ca_file = Some("/certs/ca.pem".into());
    source.read_only = true;
    let command = build(&source, Tool::Psql, None).unwrap();
    assert_eq!(command.args, ["--host=db.example", "--port=5433", "--username=ann", "--dbname=shop"]);
    assert_eq!(command.password_via.as_deref(), Some("PGPASSFILE"));
    assert!(command.env.contains(&("PGSSLMODE".to_owned(), "verify-full".to_owned())));
    assert!(command.env.contains(&("PGSSLROOTCERT".to_owned(), "/certs/ca.pem".to_owned())));
    assert!(command.env.iter().any(|(name, value)| name == "PGOPTIONS" && value.contains("read_only=on")));
    assert_eq!(
        command.command_line("psql"),
        "PGSSLMODE=verify-full PGSSLROOTCERT=/certs/ca.pem PGOPTIONS='-c default_transaction_read_only=on' \
         psql --host=db.example --port=5433 --username=ann --dbname=shop"
    );
}

#[test]
fn mysql_and_mariadb_name_tls_their_own_way() {
    let mut source = source("mysql");
    source.tls.mode = TlsMode::Require;
    let mysql = build(&source, Tool::Mysql, None).unwrap();
    assert_eq!(
        mysql.args,
        ["--protocol=TCP", "--host=db.example", "--port=5433", "--user=ann", "--database=shop", "--ssl-mode=REQUIRED"]
    );
    assert_eq!(mysql.password_via.as_deref(), Some("--defaults-extra-file"));
    source.read_only = true;
    let read_only = build(&source, Tool::Mysql, None).unwrap();
    assert_eq!(read_only.args.last().map(String::as_str), Some("--init-command=SET SESSION TRANSACTION READ ONLY"));
    source.read_only = false;
    let mariadb = build(&source, Tool::Mariadb, None).unwrap();
    assert_eq!(mariadb.args.last().map(String::as_str), Some("--ssl"));
}

#[test]
fn a_tunnelled_connection_goes_through_the_open_tunnel_only() {
    let mut source = source("postgresql");
    source.ssh = Some(SshSettings { host: "bastion".to_owned(), ..SshSettings::default() });
    assert_eq!(build(&source, Tool::Psql, None), Err(ClientError::TunnelClosed));
    let via: SocketAddr = "127.0.0.1:61000".parse().unwrap();
    let command = build(&source, Tool::Psql, Some(via)).unwrap();
    assert_eq!(command.args[..2], ["--host=127.0.0.1", "--port=61000"]);
    let mut clickhouse = source.clone();
    clickhouse.driver = "clickhouse".to_owned();
    assert_eq!(build(&clickhouse, Tool::ClickHouse, Some(via)), Err(ClientError::TunnelUnsupported));
}

#[test]
fn redis_cli_gets_the_node_database_and_tls() {
    let mut source = source("redis");
    source.port = Some(6380);
    source.database = Some("3".to_owned());
    source.user = None;
    source.tls.mode = TlsMode::Require;
    let command = build(&source, Tool::RedisCli, None).unwrap();
    assert_eq!(command.args, ["-h", "db.example", "-p", "6380", "-n", "3", "--tls", "--insecure"]);
    assert_eq!(command.password_via.as_deref(), Some("--askpass"));
    source.settings.insert("mode".to_owned(), "sentinel".into());
    assert_eq!(build(&source, Tool::RedisCli, None), Err(ClientError::Sentinel));
}

#[test]
fn sqlite_opens_its_file_and_clickhouse_takes_client_first() {
    let mut sqlite = source("sqlite");
    sqlite.database = Some("/data/app.db".to_owned());
    sqlite.read_only = true;
    let command = build(&sqlite, Tool::Sqlite3, None).unwrap();
    assert_eq!(command.args, ["-readonly", "/data/app.db"]);
    assert_eq!(command.password_via, None);
    sqlite.database = None;
    assert_eq!(build(&sqlite, Tool::Sqlite3, None), Err(ClientError::NoFile));
    let clickhouse = source("clickhouse");
    let command = build(&clickhouse, Tool::ClickHouse, None).unwrap();
    assert_eq!(command.args, ["--host=db.example", "--user=ann", "--database=shop"]);
    assert_eq!(
        command.command_line("/opt/homebrew/bin/clickhouse"),
        "/opt/homebrew/bin/clickhouse client --host=db.example --user=ann --database=shop"
    );
}

#[test]
fn shell_quoting_keeps_odd_values_one_word() {
    assert_eq!(shell_quote("plain-1.0"), "plain-1.0");
    assert_eq!(shell_quote("it's"), "'it'\\''s'");
    assert_eq!(shell_quote(""), "''");
    assert_eq!(tools_for("mariadb"), [Tool::Mariadb, Tool::Mysql]);
    assert!(tools_for("mongodb").is_empty());
}

#[test]
fn a_password_file_holds_the_password_in_each_clients_format_and_nothing_else() {
    let tricky = r#"a:b\c"d'e<f>&g#h"#;
    assert_eq!(Tool::Psql.password_file(tricky).unwrap(), "*:*:*:*:a\\:b\\\\c\"d'e<f>&g#h\n");
    assert_eq!(Tool::Psql.password_file("two\nlines"), None, "a .pgpass line cannot hold a line break");
    assert_eq!(Tool::Mysql.password_file(tricky).unwrap(), "[client]\npassword=\"a:b\\\\c\"d'e<f>&g#h\"\n");
    assert_eq!(Tool::Mariadb.password_file("x\ny").unwrap(), "[client]\npassword=\"x\\ny\"\n");
    assert_eq!(
        Tool::ClickHouse.password_file(tricky).unwrap(),
        "<config>\n    <password>a:b\\c&quot;d&apos;e&lt;f&gt;&amp;g#h</password>\n</config>\n"
    );
    assert_eq!(Tool::RedisCli.password_file(tricky), None, "redis-cli asks for it");
    assert_eq!(Tool::Sqlite3.password_way(), None);
}
