use std::sync::Arc;
use std::time::{Duration, Instant};

use zen_db::dialect::{SqlDialect, table_id};
use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::edit::{self, KeyKind, Staged};
use zen_db::engine::{ConnectRequest, Engine, OperationStatus, QueryOptions, QueryStatement, Timing};
use zen_db::filter::{self, Composition, Operator, Sort, Source};
use zen_db::registry::DataSource;
use zen_db::results::{ReadState, ResultSet};
use zen_db::schema::{IntrospectRequest, Snapshot};
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(60);
const DATABASE: &str = "zen_dialect";

#[derive(Clone, Copy, Debug)]
struct Server {
    driver: &'static str,
    port: u16,
}

impl Server {
    fn dialect(self) -> SqlDialect {
        if self.driver == "mariadb" { SqlDialect::MariaDb } else { SqlDialect::MySql }
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

fn connect(engine: &Engine, id: &str, server: Server) {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: server.driver.to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(server.port),
        user: Some("root".to_owned()),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let request =
        ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None };
    let state = engine.wait(engine.connect(request).unwrap(), WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn exec(engine: &Engine, id: &str, sql: &str) -> zen_db::value::Execution {
    let op =
        engine.execute(id, ContextKind::Console("setup".into()), sql, ExecOptions { max_rows: 1000, timeout: None });
    let state = engine.wait(op.unwrap(), WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{sql}: {state:?}");
    state.execution.unwrap()
}

fn ids(engine: &Engine, id: &str, sql: &str) -> Vec<Value> {
    exec(engine, id, sql).rows.into_iter().map(|row| row[0].clone()).collect()
}

fn read_set(engine: &Engine, id: &str, sql: &str) -> Arc<ResultSet> {
    let statement = QueryStatement {
        text: sql.to_owned(),
        start: 0,
        end: sql.len(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let op = engine.run_query(id, ContextKind::Main, vec![statement], QueryOptions::default()).unwrap();
    let deadline = Instant::now() + WAIT;
    let query = loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            break query;
        }
        assert!(Instant::now() < deadline, "{query:?}");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(query.status, OperationStatus::Done, "{query:?}");
    let set = engine.results().get(query.statements[0].result.unwrap()).unwrap();
    while set.info().state == ReadState::Reading {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    set
}

fn create(engine: &Engine, id: &str) {
    exec(engine, id, &format!("CREATE DATABASE IF NOT EXISTS {DATABASE}"));
    exec(engine, id, &format!("DROP TABLE IF EXISTS {DATABASE}.Orders"));
    exec(
        engine,
        id,
        &format!(
            "CREATE TABLE {DATABASE}.Orders (id INT AUTO_INCREMENT PRIMARY KEY, Note VARCHAR(60), data VARBINARY(16), flag BOOLEAN NOT NULL DEFAULT TRUE, price DOUBLE, `order` INT)"
        ),
    );
    exec(
        engine,
        id,
        &format!(
            "INSERT INTO {DATABASE}.Orders (id, Note, data, flag, price, `order`) VALUES (1, 'a\\\\b', x'01', FALSE, 1.5, 10), (2, '50%', NULL, TRUE, NULL, 20), (3, 'it''s', x'ff', FALSE, 2.5, 30), (4, NULL, NULL, TRUE, NULL, 40), (5, 'n_5', NULL, FALSE, 3.0, 50)"
        ),
    );
}

fn int(values: &[i64]) -> Vec<Value> {
    values.iter().map(|value| Value::Integer(*value)).collect()
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn filters_sorts_and_quick_filters_are_written_in_the_mysql_dialect() {
    for server in servers() {
        let dialect = server.dialect();
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        connect(&engine, "f", server);
        create(&engine, "f");
        let names: Vec<String> = ["id", "Note", "data", "flag", "price", "order"].map(str::to_owned).to_vec();
        let table = dialect.qualified(DATABASE, "Orders");
        let source = Source::Table { name: &table, order: Some("id") };
        let down = [Sort { column: 4, name: "price".into(), descending: true }];
        let (sql, _) =
            filter::compose(source, Some("price IS NOT NULL OR Note IS NULL"), &down, &names, dialect).unwrap();
        assert_eq!(ids(&engine, "f", &sql), int(&[4, 5, 3, 1]), "{server:?}: NULLs first going down: {sql}");
        let up = [Sort { column: 4, name: "price".into(), descending: false }];
        let (sql, _) = filter::compose(source, None, &up, &names, dialect).unwrap();
        assert_eq!(ids(&engine, "f", &sql), int(&[1, 3, 5, 2, 4]), "{server:?}: and last going up");
        let reserved = [Sort { column: 5, name: "order".into(), descending: true }];
        let (sql, _) = filter::compose(source, Some("`order` > 15"), &reserved, &names, dialect).unwrap();
        assert_eq!(ids(&engine, "f", &sql), int(&[5, 4, 3, 2]), "{server:?}: a reserved name in backticks");
        let quick = |column: &str, operator, values: &[Value]| {
            let condition = filter::quick(column, operator, values, dialect);
            let (sql, _) = filter::compose(source, Some(&condition), &[], &names, dialect).unwrap();
            ids(&engine, "f", &sql)
        };
        assert_eq!(quick("Note", Operator::Contains, &[Value::Text("50%".into())]), int(&[2]), "{server:?}");
        assert_eq!(quick("Note", Operator::Contains, &[Value::Text("_".into())]), int(&[5]), "{server:?}");
        assert_eq!(quick("Note", Operator::Contains, &[Value::Text("\\".into())]), int(&[1]), "{server:?}");
        assert_eq!(quick("Note", Operator::Equal, &[Value::Text("a\\b".into())]), int(&[1]), "{server:?}");
        assert_eq!(quick("Note", Operator::Equal, &[Value::Text("it's".into())]), int(&[3]), "{server:?}");
        assert_eq!(quick("Note", Operator::In, &[Value::Text("it's".into()), Value::Null]), int(&[3, 4]));
        assert_eq!(quick("data", Operator::Equal, &[Value::Bytes(vec![0xff])]), int(&[3]), "{server:?}");
        assert_eq!(quick("flag", Operator::Equal, &[Value::Bool(true)]), int(&[2, 4]), "{server:?}");
        assert_eq!(quick("price", Operator::Contains, &[Value::Text(".5".into())]), int(&[1, 3]), "{server:?}");

        let console = format!("SELECT id, Note FROM {DATABASE}.Orders WHERE id > 1 ORDER BY id");
        let (sql, how) =
            filter::compose(Source::Query(&console), Some("Note LIKE 'n%'"), &[], &names[..2], dialect).unwrap();
        assert_eq!(how, Composition::Merged);
        assert_eq!(ids(&engine, "f", &sql), int(&[5]), "{server:?}");
        let grouped = format!("SELECT flag, count(*) AS n FROM {DATABASE}.Orders GROUP BY flag");
        let sorts = [Sort { column: 1, name: "n".into(), descending: true }];
        let names = ["flag".to_owned(), "n".to_owned()];
        let (sql, how) = filter::compose(Source::Query(&grouped), Some("n > 1"), &sorts, &names, dialect).unwrap();
        assert_eq!(how, Composition::Wrapped);
        assert_eq!(exec(&engine, "f", &sql).rows.len(), 2, "{server:?}");
        let hostile = filter::compose(source, Some("1 = 1); DROP TABLE Orders; --"), &[], &names, dialect);
        assert!(hostile.is_err());
        exec(&engine, "f", &format!("DROP DATABASE {DATABASE}"));
    }
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn edits_are_saved_with_prepared_statements_read_back_and_conflicts_found() {
    for server in servers() {
        let dialect = server.dialect();
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        connect(&engine, "e", server);
        create(&engine, "e");
        let request =
            IntrospectRequest { schemas: vec![DATABASE.to_owned()], force: true, ..IntrospectRequest::default() };
        let op = engine.introspect("e", request, Snapshot::new("e", server.driver), None).unwrap();
        let state = engine.wait(op, WAIT).unwrap();
        assert_eq!(state.status, OperationStatus::Done, "{state:?}");
        let snapshot = engine.take_snapshot(op).unwrap();
        let mut object = snapshot
            .schema(zen_db::mysql::CATALOG, DATABASE)
            .and_then(|schema| schema.objects.iter().find(|object| object.name == "Orders"))
            .cloned()
            .unwrap();
        object.oid = table_id(DATABASE, "Orders");
        let table = dialect.qualified(DATABASE, "Orders");
        let set = read_set(&engine, "e", &format!("SELECT *, upper(Note) AS shout FROM {table} ORDER BY id"));
        let columns = set.columns();
        assert_eq!(columns[1].origin.as_deref(), Some("Note"), "{server:?}: {columns:?}");
        assert!(columns[6].source.is_none(), "a computed column reads no table");
        let editing = edit::identify(&columns, DATABASE, &object, None, dialect);
        assert!(editing.editable && editing.key.as_ref().unwrap().kind == KeyKind::PrimaryKey, "{editing:?}");
        assert!(!editing.columns[6].editable);
        set.stage(0, 1, Staged::Value(Value::Text("it's \\'; DROP TABLE Orders; --".into())));
        set.stage(1, 2, Staged::Value(Value::Bytes(vec![0, 0xfe])));
        set.stage(1, 3, Staged::Value(Value::Bool(false)));
        set.stage(2, 4, Staged::Value(Value::Decimal("2.50".into())));
        set.delete_rows(&[4]);
        set.add_row(vec![(1, Staged::Value(Value::Text("new".into()))), (3, Staged::Default)]);
        let planned = edit::plan(&editing, &set.changes());
        assert!(planned.iter().all(|statement| !statement.sql.contains("it's")), "values are bound");
        let state = engine.wait(engine.save_edits(set.id(), planned).unwrap(), WAIT).unwrap();
        assert_eq!(state.status, OperationStatus::Done, "{server:?}: {state:?}");
        let report = state.saved.unwrap();
        assert_eq!((report.deleted, report.inserted, report.updated), (1, 1, 3), "an unchanged value is no conflict");
        let saved = exec(&engine, "e", &format!("SELECT id, Note, data, flag, price FROM {table} ORDER BY id")).rows;
        assert_eq!(saved.len(), 5);
        assert_eq!(saved[0][1], Value::Text("it's \\'; DROP TABLE Orders; --".into()), "{server:?}");
        assert_eq!(saved[1][2], Value::Bytes(vec![0, 0xfe]));
        assert_eq!(saved[1][3], Value::Integer(0), "BOOLEAN is TINYINT(1)");
        assert_eq!(saved[4][..2], [Value::Integer(6), Value::Text("new".into())]);
        assert_eq!(saved[4][3], Value::Integer(1), "the column's default");
        assert_eq!(set.len(), 5);
        assert_eq!(set.get(4, 0), Value::Integer(6), "{server:?}: the new key came back by LAST_INSERT_ID()");
        assert_eq!(set.get(4, 3), Value::Integer(1));
        assert_eq!(set.get(4, 6), Value::Null, "a computed column is not read back");

        let set = read_set(&engine, "e", &format!("SELECT * FROM {table} ORDER BY id"));
        let editing = edit::identify(&set.columns(), DATABASE, &object, None, dialect);
        set.delete_rows(&[1]);
        set.stage(2, 1, Staged::Value(Value::Text("mine".into())));
        exec(&engine, "e", &format!("UPDATE {table} SET Note = 'theirs' WHERE id = 3"));
        let state =
            engine.wait(engine.save_edits(set.id(), edit::plan(&editing, &set.changes())).unwrap(), WAIT).unwrap();
        assert_eq!(state.status, OperationStatus::Failed, "{server:?}");
        assert_eq!(state.error.unwrap().code.as_deref(), Some("conflict"));
        let report = state.saved.unwrap();
        assert_eq!((report.failed_row, report.conflict), (Some(2), true));
        assert_eq!(
            exec(&engine, "e", &format!("SELECT count(*) FROM {table}")).rows[0][0],
            Value::Integer(5),
            "rolled back"
        );
        assert!(engine.open_transactions("e").is_empty());
        exec(&engine, "e", &format!("DROP DATABASE {DATABASE}"));
    }
}
