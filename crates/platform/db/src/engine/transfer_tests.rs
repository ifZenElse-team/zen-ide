use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::csv_import::{LoadSettings, OnError};
use crate::dialect::SqlDialect;
use crate::history::{HistorySink, Provenance, Ran, Source};
use crate::memory::{MemoryDriver, SQL};
use crate::registry::AuthKind;
use crate::transfer::SqlType;
use crate::types::DEVELOPMENT;

const WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Kept(Mutex<Vec<Ran>>);

impl HistorySink for Kept {
    fn record(&self, ran: Ran) {
        self.0.lock().unwrap().push(ran);
    }
}

fn connect(engine: &Engine, id: &str, read_only: bool) {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: SQL.to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        auth: AuthKind::Password,
        read_only,
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    let request =
        ConnectRequest { source, kind, password: Some(Secret::new("pw".to_owned())), stored: None, ssh_secret: None };
    let op = engine.connect(request).unwrap();
    assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
}

fn open_engine(read_only: bool) -> (Engine, Arc<MemoryDriver>, Arc<Kept>) {
    let sql = Arc::new(MemoryDriver::sql());
    let engine = Engine::new(vec![Arc::clone(&sql) as Arc<dyn Driver>], "Zen Test");
    let kept = Arc::new(Kept::default());
    engine.set_history(Some(Arc::clone(&kept) as Arc<dyn HistorySink>));
    connect(&engine, "src", false);
    connect(&engine, "dst", read_only);
    (engine, sql, kept)
}

fn table(types: Vec<SqlType>, prelude: Vec<String>) -> TransferTable {
    TransferTable {
        name: "public.customers → public.copy".to_owned(),
        select: "SELECT id, name FROM customers".to_owned(),
        table: "\"public\".\"copy\"".to_owned(),
        columns: vec!["id".to_owned(), "name".to_owned()],
        types,
        prelude,
    }
}

fn request(tables: Vec<TransferTable>, load: LoadSettings) -> TransferRequest {
    TransferRequest { source: "src".to_owned(), target: "dst".to_owned(), dialect: SqlDialect::Postgres, tables, load }
}

fn finished(engine: &Engine, id: u64) -> TransferState {
    engine.wait(id, WAIT).unwrap();
    let state = engine.transfer_state(id).unwrap();
    assert_ne!(state.status, OperationStatus::Running, "{state:?}");
    state
}

fn verbs(executed: &[String]) -> Vec<String> {
    executed
        .iter()
        .filter(|sql| !sql.starts_with("SELECT"))
        .map(|sql| {
            let words: Vec<&str> = sql.split_whitespace().take(2).collect();
            if words[0] == "INSERT" { format!("INSERT {}", sql.matches("\n(").count()) } else { words.join(" ") }
        })
        .collect()
}

#[test]
fn rows_are_read_from_the_source_and_written_into_the_target_in_batches() {
    let (engine, driver, kept) = open_engine(false);
    let create = "CREATE TABLE \"public\".\"copy\" (id bigint, name text)".to_owned();
    let tables = vec![table(vec![SqlType::BigInt, SqlType::Text], vec![create])];
    let id =
        engine.transfer(request(tables, LoadSettings { batch: 10, ..LoadSettings::default() }), Provenance::default());
    let state = finished(&engine, id.unwrap());
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!((state.rows_read, state.rows_inserted, state.rows_committed, state.rows_skipped), (25, 25, 25, 0));
    assert_eq!(state.tables[0].status, ItemStatus::Done);
    assert_eq!(
        verbs(&driver.server.executed()),
        [
            "BEGIN",
            "CREATE TABLE",
            "SAVEPOINT zen_import",
            "INSERT 10",
            "RELEASE SAVEPOINT",
            "SAVEPOINT zen_import",
            "INSERT 10",
            "RELEASE SAVEPOINT",
            "SAVEPOINT zen_import",
            "INSERT 5",
            "RELEASE SAVEPOINT",
            "COMMIT",
        ]
    );
    let first = driver.server.executed().into_iter().find(|sql| sql.starts_with("INSERT")).unwrap();
    assert!(
        first.starts_with("INSERT INTO \"public\".\"copy\" (\"id\", \"name\") VALUES\n(1, 'Customer 1'),\n"),
        "{first}"
    );
    for id in ["src", "dst"] {
        assert!(
            engine
                .status(id)
                .contexts
                .iter()
                .all(|context| !matches!(&context.context, ContextKind::Console(name) if name.starts_with("transfer"))),
            "the transfer's own sessions close"
        );
    }
    let history = kept.0.lock().unwrap();
    let texts: Vec<(&str, &str)> = history.iter().map(|ran| (ran.connection.as_str(), ran.text.as_str())).collect();
    assert!(texts.contains(&("src", "SELECT id, name FROM customers")), "{texts:?}");
    assert!(texts.contains(&("dst", "CREATE TABLE \"public\".\"copy\" (id bigint, name text)")), "{texts:?}");
    assert!(history.iter().all(|ran| ran.provenance.source == Source::Transfer));
    let json = serde_json::to_string(&state).unwrap();
    assert!(!json.contains("Customer 1"), "answers never hold rows: {json}");
}

#[test]
fn values_that_do_not_convert_are_noted_and_skipped_or_stop_the_table() {
    let (engine, _, _) = open_engine(false);
    let tables = vec![table(vec![SqlType::Boolean, SqlType::Text], Vec::new())];
    let load = LoadSettings { batch: 100, on_error: OnError::Skip, ..LoadSettings::default() };
    let state = finished(&engine, engine.transfer(request(tables.clone(), load), Provenance::default()).unwrap());
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!((state.rows_read, state.rows_inserted, state.rows_skipped), (25, 1, 24), "only id 1 is a boolean");
    assert_eq!(state.tables[0].errors[0].code, "notABoolean");
    assert_eq!(state.tables[0].errors[0].row, 2);
    assert_eq!(state.tables[0].errors[0].column.as_deref(), Some("id"));
    assert_eq!(state.tables[0].errors_total, 24);

    let state =
        finished(&engine, engine.transfer(request(tables, LoadSettings::default()), Provenance::default()).unwrap());
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    assert_eq!(state.tables[0].error.as_ref().and_then(|error| error.code.as_deref()), Some("rowFailed"));
    assert_eq!(state.rows_committed, 0, "everything is rolled back");
}

#[test]
fn a_read_only_target_refuses_and_cancel_stops_the_transfer() {
    let (engine, _, _) = open_engine(true);
    let tables = vec![table(vec![SqlType::BigInt, SqlType::Text], Vec::new())];
    let refused = engine.transfer(request(tables.clone(), LoadSettings::default()), Provenance::default()).unwrap_err();
    assert_eq!((refused.kind, refused.code.as_deref()), (ErrorKind::Refused, Some("readOnly")));
    let empty = engine.transfer(request(Vec::new(), LoadSettings::default()), Provenance::default()).unwrap_err();
    assert_eq!(empty.code.as_deref(), Some("noTables"));

    let (engine, driver, _) = open_engine(false);
    driver.server.hold(true);
    let twice = vec![tables[0].clone(), tables[0].clone()];
    let id = engine.transfer(request(twice, LoadSettings::default()), Provenance::default()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(engine.cancel(id));
    driver.server.hold(false);
    let state = finished(&engine, id);
    assert_eq!(state.status, OperationStatus::Cancelled, "{state:?}");
    assert_eq!(state.rows_committed, 0);
    assert_eq!(state.tables.iter().map(|table| table.status).collect::<Vec<_>>(), [ItemStatus::Cancelled; 2]);
    assert_eq!(engine.transfers(), vec![id]);
}

#[test]
fn the_source_of_a_transfer_is_read_in_a_read_only_session() {
    let (engine, driver, _) = open_engine(false);
    let create = "CREATE TABLE \"public\".\"copy\" (id bigint, name text)".to_owned();
    let tables = vec![table(vec![SqlType::BigInt, SqlType::Text], vec![create])];
    let id = engine.transfer(request(tables, LoadSettings::default()), Provenance::default());
    assert_eq!(finished(&engine, id.unwrap()).status, OperationStatus::Done);
    let own = |read_only: bool| {
        driver.server.opened().iter().any(|open| {
            open.state.read_only == read_only
                && matches!(&open.context, ContextKind::Console(name) if name.starts_with("transfer"))
        })
    };
    assert!(own(true), "the session that reads");
    assert!(own(false), "the session that writes");
}
