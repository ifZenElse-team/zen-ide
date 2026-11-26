use std::collections::BTreeMap;
use std::time::Duration;

use zen_db::dialect::SqlDialect;
use zen_db::driver::ContextKind;
use zen_db::engine::{ConnectRequest, Engine, OperationStatus, QueryOptions, QueryStatement, Timing};
use zen_db::params::bind;
use zen_db::registry::DataSource;
use zen_db::sql::Dialect;
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(60);

fn request(driver: &str, port: u16, database: Option<&str>, user: &str) -> ConnectRequest {
    let source = DataSource {
        id: format!("{driver}-{port}"),
        name: driver.to_owned(),
        driver: driver.to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        database: database.map(str::to_owned),
        user: Some(user.to_owned()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None }
}

fn rows_of(engine: &Engine, id: &str, text: &str, dialect: (Dialect, SqlDialect)) -> Vec<Vec<Value>> {
    let values: BTreeMap<String, Value> = [
        ("id".to_owned(), Value::Integer(2)),
        ("name".to_owned(), Value::Text("it's; DROP TABLE x --".to_owned())),
        ("limit".to_owned(), Value::Integer(5)),
        ("none".to_owned(), Value::Null),
        ("flag".to_owned(), Value::Bool(true)),
    ]
    .into();
    let bound = bind(text, dialect.0, (dialect.1, true), &values).unwrap();
    assert!(bound.inlined.is_empty(), "values are never written into the text");
    let statement =
        QueryStatement { text: bound.text, start: 0, end: text.len(), client: false, params: bound.params, kept: None };
    let op =
        engine.run_query(id, ContextKind::Console("bind".into()), vec![statement], QueryOptions::default()).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    let query = engine.query(op).unwrap();
    assert!(query.statements[0].error.is_none(), "{query:?}");
    let result = engine.results().get(query.statements[0].result.unwrap()).unwrap();
    result.page(0, 100)
}

fn connected(engine: &Engine, request: ConnectRequest) -> String {
    let id = request.source.id.clone();
    let op = engine.connect(request).unwrap();
    assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
    id
}

#[test]
#[ignore = "needs PostgreSQL in Docker"]
fn postgresql_binds_named_values_on_the_server() {
    let port = std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54318".to_owned())
        .split(',')
        .next_back()
        .and_then(|port| port.trim().parse().ok())
        .unwrap();
    let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
    let id = connected(&engine, request("postgresql", port, Some("postgres"), "postgres"));
    let text = "SELECT n, :name AS name, :none::int IS NULL AS is_empty, :flag::boolean AS flag \
                FROM generate_series(1, 10) AS n WHERE n > :id AND ':id' = $$:id$$ ORDER BY n LIMIT :limit";
    let rows = rows_of(&engine, &id, text, (Dialect::Postgres, SqlDialect::Postgres));
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0][0], Value::Integer(3));
    assert_eq!(rows[0][1], Value::Text("it's; DROP TABLE x --".to_owned()));
    assert_eq!((&rows[0][2], &rows[0][3]), (&Value::Bool(true), &Value::Bool(true)));
}

#[test]
#[ignore = "needs MySQL in Docker"]
fn mysql_binds_typed_values_with_prepared_statements() {
    let port = std::env::var("ZEN_DB_MYSQL_SERVERS")
        .unwrap_or_else(|_| "mysql:33084".to_owned())
        .split(',')
        .find_map(|pair| pair.trim().split_once(':').and_then(|(_, port)| port.parse().ok()))
        .unwrap();
    let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
    let id = connected(&engine, request("mysql", port, None, "root"));
    let text = "SELECT n, :name AS name, :none IS NULL AS is_empty, :flag AS flag FROM \
                (WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < 10) SELECT n FROM s) AS t \
                WHERE n > :id AND '?' = '?' ORDER BY n LIMIT :limit";
    let rows = rows_of(&engine, &id, text, (Dialect::MySql, SqlDialect::MySql));
    assert_eq!(rows.len(), 5, "LIMIT takes a bound integer");
    assert_eq!(rows[0][0], Value::Integer(3));
    assert_eq!(rows[0][1], Value::Text("it's; DROP TABLE x --".to_owned()));
    assert_eq!(rows[0][2], Value::Integer(1));
}
