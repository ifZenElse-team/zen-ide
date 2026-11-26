use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::driver::{ContextKind, Guards};
use crate::registry::DataSource;
use crate::value::{Typed, Value};
use zen_secrets::Secret;

#[derive(Clone, Debug, Default)]
struct Seen {
    target: String,
    headers: Vec<(String, String)>,
    body: String,
}

struct Server {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn serve(reply: impl Fn(&str) -> (u16, String) + Send + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let kept = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = Seen::default();
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            request.target = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
            let mut length = 0;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    break;
                }
                let (name, value) = trimmed.split_once(':').unwrap();
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse().unwrap();
                }
                request.headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            request.body = String::from_utf8(body).unwrap();
            let (status, text) = reply(&request.body);
            kept.lock().unwrap().push(request);
            let answer =
                format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    Server { port, seen }
}

fn spec(port: u16, read_only: bool, timeout: Option<Duration>) -> ConnectSpec {
    ConnectSpec {
        source: DataSource {
            id: "ch".to_owned(),
            name: "ch".to_owned(),
            driver: ID.to_owned(),
            host: Some("127.0.0.1".to_owned()),
            port: Some(port),
            database: Some("analytics".to_owned()),
            user: Some("reader".to_owned()),
            ..DataSource::default()
        },
        password: Some(Secret::new("s3cret".to_owned())),
        context: ContextKind::Main,
        guards: Guards { statement_timeout: timeout, ..Guards::default() },
        state: TrackedState { read_only, auto_commit: true, ..TrackedState::default() },
        timeout: Duration::from_secs(5),
        via: None,
    }
}

fn rows(lines: &[&str]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

fn answer(body: &str) -> (u16, String) {
    if body.contains("version()") {
        (200, rows(&[r#"["version()"]"#, r#"["String"]"#, r#"["25.8.1.1"]"#]))
    } else if body.starts_with("SELECT broken") {
        (
            500,
            "Code: 62. DB::Exception: Syntax error: failed at position 8. (SYNTAX_ERROR) (version 25.8.1.1)".to_owned(),
        )
    } else if body.contains("system.databases") {
        (200, rows(&[r#"["name","comment"]"#, r#"["String","String"]"#, r#"["analytics",""]"#, r#"["system",""]"#]))
    } else if body.contains("system.tables") {
        (
            200,
            rows(&[
                r#"["name","engine","total_rows","total_bytes","comment","is_temporary"]"#,
                r#"["String","String","Nullable(UInt64)","Nullable(UInt64)","String","UInt8"]"#,
                r#"["events","MergeTree","1000","4096","clicks",0]"#,
            ]),
        )
    } else if body.starts_with("SELECT * FROM numbers") {
        let mut lines = vec![r#"["number"]"#.to_owned(), r#"["UInt64"]"#.to_owned()];
        lines.extend((0..1000).map(|number| format!(r#"["{number}"]"#)));
        (200, lines.join("\n"))
    } else {
        (
            200,
            rows(&[
                r#"["id","price","at","tags","ok","name","attrs"]"#,
                r#"["UInt64","Decimal(10, 2)","DateTime64(3)","Array(LowCardinality(String))","Bool","Nullable(String)","Map(String, Int32)"]"#,
                r#"["18446744073709551615","12.30","2026-10-26 10:00:00.250",["a","b"],true,null,{"k":1}]"#,
            ]),
        )
    }
}

fn block<T>(future: impl Future<Output = T>) -> T {
    crate::engine::runtime().block_on(future)
}

#[test]
fn values_keep_their_clickhouse_types() {
    assert_eq!(
        value(Json::String("18446744073709551615".into()), "UInt64"),
        Value::Decimal("18446744073709551615".into())
    );
    assert_eq!(value(Json::String("42".into()), "Int64"), Value::Integer(42));
    assert_eq!(value(serde_json::json!(7), "Nullable(UInt8)"), Value::Integer(7));
    assert_eq!(value(Json::Null, "Nullable(UInt8)"), Value::Null);
    assert_eq!(value(Json::String("12.30".into()), "Decimal(10, 2)"), Value::Decimal("12.30".into()));
    assert_eq!(value(Json::String("inf".into()), "Float64"), Value::Float(f64::INFINITY));
    assert_eq!(
        value(Json::String("2026-10-26".into()), "Date"),
        Value::Temporal(Typed { text: "2026-10-26".into(), type_name: "Date".into() })
    );
    assert_eq!(
        value(serde_json::json!([["a", 1]]), "Array(Tuple(String, UInt8))"),
        Value::Array(vec![Value::Array(vec![Value::Text("a".into()), Value::Integer(1)])])
    );
    assert_eq!(
        value(serde_json::json!({"k": "1"}), "Map(String, UInt64)"),
        Value::Map(vec![("k".into(), Value::Integer(1))])
    );
    assert_eq!(
        value(Json::String("10.0.0.1".into()), "IPv4"),
        Value::Other(Typed { text: "10.0.0.1".into(), type_name: "IPv4".into() })
    );
    assert_eq!(value(Json::String("x".into()), "LowCardinality(Nullable(String))"), Value::Text("x".into()));
}

#[test]
fn statements_are_classified_quoted_and_opened() {
    assert_eq!(classify("-- note\nSELECT 1"), StatementKind::Read);
    assert_eq!(classify("WITH x AS (SELECT 1) SELECT * FROM x"), StatementKind::Read);
    assert_eq!(classify("INSERT INTO t VALUES (1)"), StatementKind::Write);
    assert!(ClickHouse.changes_structure("ALTER TABLE t ADD COLUMN c UInt8"));
    assert!(!ClickHouse.changes_structure("SELECT 1"));
    assert_eq!(quote("we`ird"), "`we\\`ird`");
    let path = ["db".to_owned(), "tables".to_owned(), "events".to_owned()];
    assert_eq!(
        ClickHouse.open_text(&path, &NodeKind::Table).as_deref(),
        Some("SELECT * FROM `db`.`events` LIMIT 200;\n")
    );
}

#[test]
fn queries_go_over_http_with_settings_and_come_back_typed() {
    let server = serve(answer);
    let driver = ClickHouse;
    let mut session = block(driver.connect(&spec(server.port, true, Some(Duration::from_secs(300))))).unwrap();
    assert_eq!(block(session.server_version()).unwrap().as_deref(), Some("ClickHouse 25.8.1.1"));
    let got = block(session.execute("SELECT * FROM t;", &ExecOptions::default())).unwrap();
    assert_eq!(got.columns[1], Column::new("price", "Decimal(10, 2)"));
    assert_eq!(
        got.rows[0],
        vec![
            Value::Decimal("18446744073709551615".into()),
            Value::Decimal("12.30".into()),
            Value::Temporal(Typed { text: "2026-10-26 10:00:00.250".into(), type_name: "DateTime64(3)".into() }),
            Value::Array(vec![Value::Text("a".into()), Value::Text("b".into())]),
            Value::Bool(true),
            Value::Null,
            Value::Map(vec![("k".into(), Value::Integer(1))]),
        ]
    );
    let failed = block(session.execute("SELECT broken", &ExecOptions::default())).unwrap_err();
    assert_eq!((failed.kind, failed.code.as_deref()), (ErrorKind::Server, Some("62")));
    assert!(failed.detail.contains("SYNTAX_ERROR"));
    let seen = server.seen.lock().unwrap().clone();
    let query = &seen[1];
    assert_eq!(query.body, "SELECT * FROM t");
    for part in [
        "default_format=JSONCompactEachRowWithNamesAndTypes",
        "readonly=2",
        "max_execution_time=300",
        "database=analytics",
        "session_id=zen-",
        "query_id=zen-",
    ] {
        assert!(query.target.contains(part), "{part} in {}", query.target);
    }
    assert!(!query.target.contains("s3cret"), "the password never goes in the URL");
    assert!(query.headers.contains(&("x-clickhouse-user".into(), "reader".into())));
    assert!(query.headers.contains(&("x-clickhouse-key".into(), "s3cret".into())));
    let session_ids: Vec<&str> =
        seen.iter().filter_map(|seen| seen.target.split("session_id=").nth(1)?.split('&').next()).collect();
    assert!(session_ids.windows(2).all(|pair| pair[0] == pair[1]), "one HTTP session per driver session");
}

#[test]
fn the_tree_lists_databases_folders_and_tables() {
    let server = serve(answer);
    let mut session = block(ClickHouse.connect(&spec(server.port, false, None))).unwrap();
    let roots = block(session.children(&[])).unwrap();
    assert_eq!(
        roots.iter().map(|node| (node.label.as_str(), node.system)).collect::<Vec<_>>(),
        [("analytics", false), ("system", true)]
    );
    let folders = block(session.children(&["analytics".to_owned()])).unwrap();
    assert_eq!(folders.iter().map(|node| node.label.as_str()).collect::<Vec<_>>(), ["Tables", "Views", "Dictionaries"]);
    let tables = block(session.children(&["analytics".to_owned(), "tables".to_owned()])).unwrap();
    assert_eq!(
        (tables[0].label.as_str(), tables[0].detail.as_deref(), tables[0].rows, tables[0].size),
        ("events", Some("MergeTree"), Some(1000), Some(4096))
    );
    let seen = server.seen.lock().unwrap().clone();
    let listed = seen.iter().find(|seen| seen.body.contains("system.tables")).unwrap();
    assert!(listed.target.contains("param_database=analytics"), "names go as query parameters, never into the text");
    assert!(!listed.target.contains("readonly"));
}

struct Sink {
    columns: Vec<Column>,
    rows: Vec<Vec<Value>>,
    cap: usize,
}

impl RowSink for Sink {
    fn columns(&mut self, columns: Vec<Column>) {
        self.columns = columns;
    }

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> BoxFuture<'_, usize> {
        let left = self.cap.saturating_sub(self.rows.len());
        Box::pin(async move { left })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        self.rows.extend(rows);
        self.rows.len() < self.cap
    }

    fn cap(&self) -> usize {
        self.cap
    }
}

#[test]
fn results_stream_page_by_page_and_stop_at_the_cap() {
    let server = serve(answer);
    let mut session = block(ClickHouse.connect(&spec(server.port, false, None))).unwrap();
    let mut sink = Sink { columns: Vec::new(), rows: Vec::new(), cap: 250 };
    let end = block(session.stream("SELECT * FROM numbers(1000)", &StreamOptions { page: 100, skip: 10 }, &mut sink))
        .unwrap();
    assert_eq!(sink.columns[0].type_name, "UInt64");
    assert_eq!(sink.rows.len(), 250);
    assert_eq!(sink.rows[0], vec![Value::Integer(10)]);
    assert!(!end.complete);
}

#[test]
fn a_tunnel_keeps_checking_the_certificate_by_the_servers_own_name() {
    let (via, hello) = crate::ssh::tests::client_hello();
    let mut through = spec(8443, false, None);
    through.source.host = Some("ch.example.com".to_owned());
    through.source.tls.mode = TlsMode::VerifyFull;
    through.via = Some(via);
    assert!(!endpoint(&through).unwrap().agent.config().tls_config().disable_verification());
    let failed = block(ClickHouse.connect(&through)).err().unwrap();
    let hello = hello.join().unwrap();
    assert!(crate::ssh::tests::names(&hello, "ch.example.com"), "{failed:?}");

    let server = serve(answer);
    let mut plain = spec(8123, false, None);
    plain.source.host = Some("ch.example.com".to_owned());
    plain.via = Some(std::net::SocketAddr::from(([127, 0, 0, 1], server.port)));
    let mut session = block(ClickHouse.connect(&plain)).unwrap();
    assert_eq!(block(session.server_version()).unwrap().as_deref(), Some("ClickHouse 25.8.1.1"));
    let seen = server.seen.lock().unwrap().clone();
    assert!(seen[0].headers.contains(&("host".to_owned(), "ch.example.com:8123".to_owned())), "{seen:?}");
}
