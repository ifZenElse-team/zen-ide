use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::csv_import::{ColumnMapping, CsvFormat, Kind, LoadSettings, OnError};
use crate::dialect::SqlDialect;
use crate::history::{HistorySink, Provenance, Ran, Source};
use crate::memory::{MemoryDriver, SQL};
use crate::registry::AuthKind;
use crate::types::DEVELOPMENT;

const WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Kept(Mutex<Vec<Ran>>);

impl HistorySink for Kept {
    fn record(&self, ran: Ran) {
        self.0.lock().unwrap().push(ran);
    }
}

fn open_engine(read_only: bool) -> (Engine, Arc<MemoryDriver>, Arc<Kept>) {
    let sql = Arc::new(MemoryDriver::sql());
    let engine = Engine::new(vec![Arc::clone(&sql) as Arc<dyn Driver>], "Zen Test");
    let kept = Arc::new(Kept::default());
    engine.set_history(Some(Arc::clone(&kept) as Arc<dyn HistorySink>));
    let source = DataSource {
        id: "db".to_owned(),
        name: "Shop".to_owned(),
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
    (engine, sql, kept)
}

fn file(dir: &Path, rows: &[&str]) -> PathBuf {
    let path = dir.join("people.csv");
    std::fs::write(&path, format!("id,name\n{}\n", rows.join("\n"))).unwrap();
    path
}

fn request(path: &Path, load: LoadSettings) -> ImportRequest {
    ImportRequest {
        connection: "db".to_owned(),
        file: path.to_path_buf(),
        format: CsvFormat::default(),
        dialect: SqlDialect::Postgres,
        table: "\"public\".\"people\"".to_owned(),
        name: "people".to_owned(),
        columns: vec![
            ColumnMapping { source: 0, target: "id".to_owned(), kind: Kind::Integer, sql_type: None },
            ColumnMapping { source: 1, target: "name".to_owned(), kind: Kind::Text, sql_type: None },
        ],
        prelude: Vec::new(),
        load,
    }
}

fn finished(engine: &Engine, id: u64) -> ImportState {
    engine.wait(id, WAIT).unwrap();
    let state = engine.import_state(id).unwrap();
    assert_ne!(state.status, OperationStatus::Running, "{state:?}");
    state
}

fn numbered(count: usize) -> Vec<String> {
    (1..=count).map(|at| format!("{at},name {at}")).collect()
}

fn verbs(executed: &[String]) -> Vec<String> {
    executed
        .iter()
        .map(|sql| {
            let words: Vec<&str> = sql.split_whitespace().take(2).collect();
            if words[0] == "INSERT" { format!("INSERT {}", sql.matches("\n(").count()) } else { words.join(" ") }
        })
        .collect()
}

#[test]
fn rows_go_in_batches_inside_savepoints_in_one_transaction_and_the_session_closes() {
    let (engine, driver, kept) = open_engine(false);
    let dir = tempfile::tempdir().unwrap();
    let rows = numbered(25);
    let path = file(dir.path(), &rows.iter().map(String::as_str).collect::<Vec<_>>());
    let mut wanted = request(&path, LoadSettings { batch: 10, truncate: true, ..LoadSettings::default() });
    wanted.prelude = vec!["TRUNCATE TABLE \"public\".\"people\"".to_owned()];
    let id = engine.import_csv(wanted, Provenance::of(Source::Import)).unwrap();
    let state = finished(&engine, id);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!((state.rows_read, state.rows_inserted, state.rows_committed, state.rows_skipped), (25, 25, 25, 0));
    assert_eq!(state.bytes_read, state.file_bytes);
    assert!(!state.server_copy, "the memory server has no COPY, so INSERTs are used");
    let executed = verbs(&driver.server.executed());
    assert_eq!(
        executed,
        [
            "BEGIN",
            "TRUNCATE TABLE",
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
        ],
    );
    let first = driver.server.executed().into_iter().find(|sql| sql.starts_with("INSERT")).unwrap();
    assert!(
        first.starts_with("INSERT INTO \"public\".\"people\" (\"id\", \"name\") VALUES\n(1, 'name 1'),\n"),
        "{first}"
    );
    assert!(
        engine
            .status("db")
            .contexts
            .iter()
            .all(|context| !matches!(&context.context, ContextKind::Console(name) if name.starts_with("import"))),
        "the import's own session closes"
    );
    let history = kept.0.lock().unwrap();
    let texts: Vec<&str> = history.iter().map(|ran| ran.text.as_str()).collect();
    assert_eq!(
        texts,
        ["TRUNCATE TABLE \"public\".\"people\"", "INSERT INTO \"public\".\"people\" (\"id\", \"name\") VALUES (…)"]
    );
    assert!(history.iter().all(|ran| ran.provenance.source == Source::Import));
    assert_eq!(history[1].affected, Some(25));
    assert_eq!(history[1].provenance.place.as_deref(), Some("people.csv"));
    let json = serde_json::to_string(&state).unwrap();
    assert!(!json.contains("name 1"), "answers never hold rows: {json}");
}

#[test]
fn on_mysql_the_truncate_commits_by_itself_before_the_transaction_of_the_savepoints_begins() {
    for dialect in [SqlDialect::MySql, SqlDialect::MariaDb] {
        let (engine, driver, _) = open_engine(false);
        let dir = tempfile::tempdir().unwrap();
        let rows = numbered(15);
        let path = file(dir.path(), &rows.iter().map(String::as_str).collect::<Vec<_>>());
        let mut wanted = request(&path, LoadSettings { batch: 10, truncate: true, ..LoadSettings::default() });
        wanted.dialect = dialect;
        wanted.table = "`shop`.`people`".to_owned();
        wanted.prelude = vec!["TRUNCATE TABLE `shop`.`people`".to_owned()];
        let state = finished(&engine, engine.import_csv(wanted, Provenance::default()).unwrap());
        assert_eq!(state.status, OperationStatus::Done, "{state:?}");
        assert_eq!((state.rows_inserted, state.rows_committed), (15, 15));
        assert_eq!(
            verbs(&driver.server.executed()),
            [
                "TRUNCATE TABLE",
                "BEGIN",
                "SAVEPOINT zen_import",
                "INSERT 10",
                "RELEASE SAVEPOINT",
                "SAVEPOINT zen_import",
                "INSERT 5",
                "RELEASE SAVEPOINT",
                "COMMIT",
            ],
            "{dialect:?}"
        );
    }
}

#[test]
fn skipped_rows_are_noted_with_their_numbers_and_the_rest_go_in() {
    let (engine, driver, _) = open_engine(false);
    driver.server.fail_on(Some("'dup'"));
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), &["1,a", "x,b", "3,dup", "4,d,extra", "\"5\",\"multi\nline\"", "6,f"]);
    let load = LoadSettings { batch: 100, on_error: OnError::Skip, ..LoadSettings::default() };
    let state = finished(&engine, engine.import_csv(request(&path, load), Provenance::default()).unwrap());
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!((state.rows_read, state.rows_inserted, state.rows_skipped, state.rows_committed), (6, 3, 3, 3));
    let noted: Vec<(u64, u64, &str, Option<&str>)> = state
        .errors
        .iter()
        .map(|error| (error.row, error.line, error.code.as_str(), error.column.as_deref()))
        .collect();
    assert_eq!(noted, [(2, 3, "notANumber", Some("id")), (4, 5, "columns", None), (3, 4, "23505", None)]);
    assert_eq!(state.errors_total, 3);
    let executed = verbs(&driver.server.executed());
    assert_eq!(
        executed,
        [
            "BEGIN",
            "SAVEPOINT zen_import",
            "INSERT 4",
            "ROLLBACK TO",
            "RELEASE SAVEPOINT",
            "SAVEPOINT zen_import",
            "INSERT 1",
            "RELEASE SAVEPOINT",
            "SAVEPOINT zen_import",
            "INSERT 1",
            "ROLLBACK TO",
            "RELEASE SAVEPOINT",
            "SAVEPOINT zen_import",
            "INSERT 1",
            "RELEASE SAVEPOINT",
            "SAVEPOINT zen_import",
            "INSERT 1",
            "RELEASE SAVEPOINT",
            "COMMIT",
        ],
        "the failed batch is retried row by row"
    );
}

#[test]
fn a_bad_row_stops_the_import_and_rolls_back_what_was_not_committed() {
    let (engine, driver, _) = open_engine(false);
    let dir = tempfile::tempdir().unwrap();
    let mut rows = numbered(12);
    rows[10] = "11,dup".to_owned();
    driver.server.fail_on(Some("'dup'"));
    let path = file(dir.path(), &rows.iter().map(String::as_str).collect::<Vec<_>>());
    let load = LoadSettings { batch: 2, commit_every: 4, ..LoadSettings::default() };
    let state = finished(&engine, engine.import_csv(request(&path, load), Provenance::default()).unwrap());
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    assert_eq!(state.error.as_ref().and_then(|error| error.code.as_deref()), Some("rowFailed"));
    assert_eq!((state.rows_committed, state.errors[0].row), (8, 11));
    let executed = driver.server.executed();
    assert_eq!(executed.iter().filter(|sql| *sql == "COMMIT").count(), 2, "every 4 rows");
    assert_eq!(executed.last().map(String::as_str), Some("DISCARD"), "the open transaction is rolled back");

    let path = file(dir.path(), &["1,a", "two,b", "3,c"]);
    let state =
        finished(&engine, engine.import_csv(request(&path, LoadSettings::default()), Provenance::default()).unwrap());
    assert_eq!(state.status, OperationStatus::Failed);
    assert_eq!((state.rows_read, state.rows_inserted, state.rows_committed), (2, 1, 0));
    assert_eq!(state.errors[0].code, "notANumber");
}

#[test]
fn read_only_connections_refuse_and_cancel_stops_the_import() {
    let (engine, _, _) = open_engine(true);
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), &["1,a"]);
    let refused = engine.import_csv(request(&path, LoadSettings::default()), Provenance::default()).unwrap_err();
    assert_eq!((refused.kind, refused.code.as_deref()), (ErrorKind::Refused, Some("readOnly")));

    let (engine, driver, _) = open_engine(false);
    let missing =
        engine.import_csv(request(&dir.path().join("missing.csv"), LoadSettings::default()), Provenance::default());
    assert_eq!(missing.unwrap_err().code.as_deref(), Some("io"));
    let bad = ImportRequest {
        format: CsvFormat { delimiter: '"', ..CsvFormat::default() },
        ..request(&path, LoadSettings::default())
    };
    assert_eq!(engine.import_csv(bad, Provenance::default()).unwrap_err().code.as_deref(), Some("delimiterQuote"));
    driver.server.hold(true);
    let id = engine.import_csv(request(&path, LoadSettings::default()), Provenance::default()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(engine.cancel(id));
    driver.server.hold(false);
    let state = finished(&engine, id);
    assert_eq!(state.status, OperationStatus::Cancelled, "{state:?}");
    assert_eq!(state.rows_committed, 0);
    assert_eq!(engine.imports(), vec![id]);
}

#[test]
fn an_import_cancelled_while_the_server_answers_never_commits() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), &numbered(40).iter().map(String::as_str).collect::<Vec<_>>());
    for round in 0..60 {
        let (engine, driver, _) = open_engine(false);
        driver.server.hold(true);
        let id = engine.import_csv(request(&path, LoadSettings::default()), Provenance::default()).unwrap();
        std::thread::sleep(Duration::from_micros(round * 50));
        assert!(engine.cancel(id));
        driver.server.hold(false);
        let state = finished(&engine, id);
        assert_eq!(state.status, OperationStatus::Cancelled, "{state:?}");
        assert!(!driver.server.executed().iter().any(|sql| sql == "COMMIT"), "{:?}", driver.server.executed());
    }
}
