use std::sync::Arc;
use std::time::{Duration, Instant};

use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::edit::{self, KeyKind, Staged};
use zen_db::engine::{ConnectRequest, Engine, OperationStatus, QueryOptions, QueryState, QueryStatement, Timing};
use zen_db::filter::{self, Composition, Sort, Source};
use zen_db::registry::DataSource;
use zen_db::results::ResultSet;
use zen_db::schema::{Column, Constraint, ConstraintKind, Object, ObjectKind};
use zen_db::types::{ConnectionType, DEVELOPMENT, PRODUCTION};
use zen_db::value::Value;
use zen_secrets::Secret;

const PG: SqlDialect = SqlDialect::Postgres;
const WAIT: Duration = Duration::from_secs(60);

fn ports() -> Vec<u16> {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
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

fn connect(engine: &Engine, request: ConnectRequest) {
    let op = engine.connect(request).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn finished(engine: &Engine, op: u64) -> QueryState {
    let deadline = Instant::now() + WAIT;
    loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            return query;
        }
        assert!(Instant::now() < deadline, "{query:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn run(engine: &Engine, id: &str, context: &ContextKind, sql: &str) -> QueryState {
    let statement = QueryStatement {
        text: sql.to_owned(),
        start: 0,
        end: sql.len(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let op = engine.run_query(id, context.clone(), vec![statement], QueryOptions::default()).unwrap();
    let query = finished(engine, op);
    assert_eq!(query.status, OperationStatus::Done, "{query:?}");
    query
}

fn read(engine: &Engine, id: &str, context: &ContextKind, sql: &str) -> Arc<ResultSet> {
    let query = run(engine, id, context, sql);
    let result = query.statements[0].result.unwrap();
    let set = engine.results().get(result).unwrap();
    let deadline = Instant::now() + WAIT;
    while set.info().state == zen_db::results::ReadState::Reading {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    set
}

fn rows(engine: &Engine, id: &str, sql: &str) -> Vec<Vec<Value>> {
    let set = read(engine, id, &ContextKind::Console("check".into()), sql);
    let rows = set.page(0, 1000);
    engine.results().remove(set.id());
    rows
}

fn table(engine: &Engine, id: &str, name: &str) -> Object {
    let found = rows(engine, id, &format!("SELECT '{name}'::regclass::oid::int8"));
    let Value::Integer(oid) = found[0][0] else { panic!("{found:?}") };
    let mut object = Object::new(ObjectKind::Table, name);
    object.oid = u32::try_from(oid).unwrap();
    object.detailed = true;
    let column = |position: i32, name: &str, type_name: &str, nullable: bool| Column {
        name: name.into(),
        position,
        type_name: type_name.into(),
        nullable,
        ..Column::default()
    };
    object.columns = vec![
        column(1, "id", "integer", false),
        column(2, "name", "text", true),
        column(3, "doc", "jsonb", true),
        column(4, "seen", "timestamp with time zone", true),
    ];
    object.constraints = vec![Constraint {
        name: format!("{name}_pkey"),
        kind: ConstraintKind::PrimaryKey,
        columns: vec!["id".into()],
        definition: "PRIMARY KEY (id)".into(),
        references: None,
        index: None,
        comment: None,
    }];
    object
}

fn create(engine: &Engine, id: &str, name: &str) {
    let setup = ContextKind::Console("setup".into());
    run(engine, id, &setup, &format!("DROP TABLE IF EXISTS {name}"));
    run(
        engine,
        id,
        &setup,
        &format!("CREATE TABLE {name} (id int PRIMARY KEY, name text, doc jsonb, seen timestamptz)"),
    );
    run(
        engine,
        id,
        &setup,
        &format!(
            "INSERT INTO {name} SELECT g, 'n' || g, jsonb_build_object('g', g), '2026-10-27 10:00+00' FROM generate_series(1, 5) g"
        ),
    );
}

fn save(engine: &Engine, set: &ResultSet, editing: &edit::Editing) -> zen_db::engine::OperationState {
    let planned = edit::plan(editing, &set.changes());
    let op = engine.save_edits(set.id(), planned).unwrap();
    engine.wait(op, WAIT).unwrap()
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn staged_edits_are_saved_with_bound_parameters_and_read_back() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        connect(&engine, request("e", port, DEVELOPMENT));
        create(&engine, "e", "zen_edit");
        let object = table(&engine, "e", "zen_edit");
        let set = read(&engine, "e", &ContextKind::Main, "SELECT *, upper(name) AS shout FROM zen_edit ORDER BY id");
        assert!(set.columns()[0].source.is_some() && set.columns()[4].source.is_none(), "{:?}", set.columns());
        let editing = edit::identify(&set.columns(), "public", &object, None, PG);
        assert!(editing.editable && editing.key.as_ref().unwrap().kind == KeyKind::PrimaryKey, "{editing:?}");
        assert!(!editing.columns[4].editable, "a computed column is read-only");
        set.stage(0, 1, Staged::Value(Value::Text("it's; DROP TABLE zen_edit; --".into())));
        set.stage(0, 2, Staged::Value(edit::parse_input("{\"a\": [1, 2]}", "jsonb").unwrap()));
        set.stage(1, 3, Staged::Value(edit::parse_input("2026-01-02 03:04:05+00", "timestamptz").unwrap()));
        set.stage(2, 1, Staged::Value(Value::Null));
        set.delete_rows(&[4]);
        let added =
            set.add_row(vec![(0, Staged::Value(Value::Integer(10))), (1, Staged::Value(Value::Text("ten".into())))]);
        assert_eq!(added, 5);
        let state = save(&engine, &set, &editing);
        assert_eq!(state.status, OperationStatus::Done, "{state:?}");
        let report = state.saved.unwrap();
        assert_eq!((report.deleted, report.inserted, report.updated), (1, 1, 3));
        let saved = rows(&engine, "e", "SELECT id, name, doc::text, seen::text FROM zen_edit ORDER BY id");
        assert_eq!(saved.len(), 5);
        assert_eq!(saved[0][1], Value::Text("it's; DROP TABLE zen_edit; --".into()), "values are bound, never spliced");
        assert_eq!(saved[0][2], Value::Text("{\"a\": [1, 2]}".into()));
        assert!(matches!(&saved[1][3], Value::Text(text) if text.starts_with("2026-01-02 03:04:05")), "{saved:?}");
        assert_eq!(saved[2][1], Value::Null);
        assert_eq!(saved[4][..2], [Value::Integer(10), Value::Text("ten".into())]);
        assert!(!set.pending().any());
        assert_eq!(set.len(), 5, "the deleted row went, the new row came back from the server");
        assert_eq!(set.get(4, 4), Value::Null, "a column the server does not return for the new row stays empty");
        assert!(engine.open_transactions("e").is_empty(), "auto-commit: the save committed");
        run(&engine, "e", &ContextKind::Main, "DROP TABLE zen_edit");
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_row_changed_by_someone_else_is_a_conflict_and_nothing_is_saved() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        connect(&engine, request("c", port, DEVELOPMENT));
        create(&engine, "c", "zen_conflict");
        let object = table(&engine, "c", "zen_conflict");
        let set = read(&engine, "c", &ContextKind::Main, "SELECT * FROM zen_conflict ORDER BY id");
        let editing = edit::identify(&set.columns(), "public", &object, None, PG);
        set.delete_rows(&[0]);
        set.stage(1, 1, Staged::Value(Value::Text("mine".into())));
        run(
            &engine,
            "c",
            &ContextKind::Console("other".into()),
            "UPDATE zen_conflict SET name = 'theirs' WHERE id = 2",
        );
        let state = save(&engine, &set, &editing);
        assert_eq!(state.status, OperationStatus::Failed);
        assert_eq!(state.error.unwrap().code.as_deref(), Some("conflict"));
        let report = state.saved.unwrap();
        assert_eq!((report.failed_row, report.conflict), (Some(1), true));
        let after = rows(&engine, "c", "SELECT id, name FROM zen_conflict ORDER BY id");
        assert_eq!(after.len(), 5, "the delete before the conflict was rolled back");
        assert_eq!(after[1][1], Value::Text("theirs".into()));
        assert!(set.pending().any(), "the changes stay staged");
        set.unstage(1, 1);
        run(&engine, "c", &ContextKind::Console("other".into()), "DELETE FROM zen_conflict WHERE id = 1");
        let gone = save(&engine, &set, &editing);
        assert_eq!(gone.error.unwrap().code.as_deref(), Some("conflict"), "a row deleted by someone else");
        run(&engine, "c", &ContextKind::Main, "DROP TABLE zen_conflict");
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_failing_statement_rolls_back_the_save_and_manual_mode_keeps_the_transaction() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        connect(&engine, request("r", port, DEVELOPMENT));
        create(&engine, "r", "zen_rollback");
        let object = table(&engine, "r", "zen_rollback");
        let set = read(&engine, "r", &ContextKind::Main, "SELECT * FROM zen_rollback ORDER BY id");
        let editing = edit::identify(&set.columns(), "public", &object, None, PG);
        set.delete_rows(&[4]);
        set.add_row(vec![(0, Staged::Value(Value::Integer(1)))]);
        let state = save(&engine, &set, &editing);
        assert_eq!(state.status, OperationStatus::Failed);
        assert_eq!(state.error.unwrap().code.as_deref(), Some("23505"));
        assert_eq!(state.saved.unwrap().failed_row, Some(5));
        assert_eq!(rows(&engine, "r", "SELECT count(*) FROM zen_rollback")[0][0], Value::Integer(5));
        assert!(engine.open_transactions("r").is_empty());

        let manual = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        connect(&manual, request("m", port, PRODUCTION));
        let set = read(&manual, "m", &ContextKind::Main, "SELECT * FROM zen_rollback ORDER BY id");
        let editing = edit::identify(&set.columns(), "public", &object, None, PG);
        set.stage(0, 1, Staged::Value(Value::Text("pending".into())));
        let state = save(&manual, &set, &editing);
        assert_eq!(state.status, OperationStatus::Done, "{state:?}");
        assert!(state.saved.unwrap().in_transaction);
        assert_eq!(manual.open_transactions("m"), [ContextKind::Main], "left for the person to commit");
        assert_eq!(rows(&engine, "r", "SELECT name FROM zen_rollback WHERE id = 1")[0][0], Value::Text("n1".into()));
        let op = manual.execute("m", ContextKind::Main, "ROLLBACK", ExecOptions::default()).unwrap();
        manual.wait(op, WAIT).unwrap();
        run(&engine, "r", &ContextKind::Main, "DROP TABLE zen_rollback");
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn filters_and_sorts_compose_into_statements_the_server_runs() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        connect(&engine, request("f", port, DEVELOPMENT));
        create(&engine, "f", "zen_filter");
        let names: Vec<String> = ["id", "name", "doc", "seen"].iter().map(|name| (*name).to_owned()).collect();
        let sort = |column: usize, descending: bool| Sort { column, name: names[column].clone(), descending };
        let table = Source::Table { name: "public.zen_filter", order: Some("id") };
        let (sql, how) =
            filter::compose(table, Some("id BETWEEN 2 AND 4 AND name <> 'n3'"), &[sort(0, true)], &names, PG).unwrap();
        assert_eq!(how, Composition::Table);
        let ids: Vec<Value> = rows(&engine, "f", &sql).into_iter().map(|row| row[0].clone()).collect();
        assert_eq!(ids, [Value::Integer(4), Value::Integer(2)]);

        let plain = Source::Query("SELECT id, name FROM zen_filter WHERE id > 1 ORDER BY id;");
        let condition =
            filter::quick("name", filter::Operator::In, &[Value::Text("n2".into()), Value::Text("n5".into())], PG);
        let (sql, how) = filter::compose(plain, Some(&condition), &[sort(1, true)], &names[..2], PG).unwrap();
        assert_eq!(how, Composition::Merged);
        let found = rows(&engine, "f", &sql);
        assert_eq!(
            found.iter().map(|row| row[1].clone()).collect::<Vec<_>>(),
            [Value::Text("n5".into()), Value::Text("n2".into())]
        );

        let grouped = Source::Query("SELECT name, count(*) AS n FROM zen_filter GROUP BY name");
        let (sql, how) = filter::compose(
            grouped,
            Some("name LIKE 'n%' AND n = 1"),
            &[sort(0, false)],
            &["name".into(), "n".into()],
            PG,
        )
        .unwrap();
        assert_eq!(how, Composition::Wrapped);
        assert_eq!(rows(&engine, "f", &sql).len(), 5);

        let wrapped = read(
            &engine,
            "f",
            &ContextKind::Main,
            "SELECT * FROM (\nSELECT * FROM zen_filter\n) AS zen_filtered WHERE (id = 3)",
        );
        assert!(wrapped.columns()[1].source.is_some(), "a wrapped query still tells the table its columns come from");
        engine.results().remove(wrapped.id());

        let set = read(&engine, "f", &ContextKind::Main, "SELECT * FROM zen_filter ORDER BY id");
        assert!(set.sort_rows(&[(1, true)]));
        assert_eq!(set.get(0, 1), Value::Text("n5".into()));
        let like = |text: &str| {
            let condition = filter::quick("name", filter::Operator::Contains, &[Value::Text(text.into())], PG);
            rows(&engine, "f", &filter::compose(table, Some(&condition), &[], &names, PG).unwrap().0).len()
        };
        assert_eq!((like("n_"), like("n"), like("3")), (0, 5, 1), "LIKE escapes _ and %");
        let hostile = filter::compose(table, Some("1 = 1); DROP TABLE zen_filter; --"), &[], &names, PG);
        assert!(hostile.is_err());
        assert_eq!(rows(&engine, "f", "SELECT count(*) FROM zen_filter")[0][0], Value::Integer(5));
        run(&engine, "f", &ContextKind::Main, "DROP TABLE zen_filter");
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn a_key_of_every_column_never_changes_several_equal_rows() {
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        connect(&engine, request("d", port, DEVELOPMENT));
        let setup = ContextKind::Console("setup".into());
        run(&engine, "d", &setup, "DROP TABLE IF EXISTS zen_dupes");
        run(&engine, "d", &setup, "CREATE TABLE zen_dupes (body text, n int, doc jsonb)");
        run(
            &engine,
            "d",
            &setup,
            "INSERT INTO zen_dupes VALUES ('same', 1, '{}'), ('same', 1, '{}'), ('other', 2, '{}')",
        );
        let found = rows(&engine, "d", "SELECT 'zen_dupes'::regclass::oid::int8");
        let Value::Integer(oid) = found[0][0] else { panic!("{found:?}") };
        let mut object = Object::new(ObjectKind::Table, "zen_dupes");
        object.oid = u32::try_from(oid).unwrap();
        object.detailed = true;
        object.columns = ["body", "n", "doc"]
            .iter()
            .zip(["text", "integer", "jsonb"])
            .enumerate()
            .map(|(at, (name, type_name))| Column {
                name: (*name).into(),
                position: i32::try_from(at).unwrap() + 1,
                type_name: type_name.into(),
                nullable: true,
                ..Column::default()
            })
            .collect();
        let count = |engine: &Engine| rows(engine, "d", "SELECT count(*) FROM zen_dupes WHERE body = 'same'");
        let read_all = |engine: &Engine| read(engine, "d", &ContextKind::Main, "SELECT * FROM zen_dupes ORDER BY ctid");
        let set = read_all(&engine);
        let editing = edit::identify(&set.columns(), "public", &object, Some(&edit::KeyChoice::AllColumns), PG);
        assert!(editing.editable && editing.key.as_ref().unwrap().kind == KeyKind::AllColumns, "{editing:?}");

        set.delete_rows(&[0]);
        let state = save(&engine, &set, &editing);
        assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
        assert_eq!(state.error.and_then(|error| error.code).as_deref(), Some("severalRows"));
        assert_eq!(count(&engine), [[Value::Integer(2)]], "deleting one of two equal rows deleted none");

        let set = read_all(&engine);
        set.stage(1, 0, Staged::Value(Value::Text("changed".into())));
        let state = save(&engine, &set, &editing);
        assert_eq!(state.error.and_then(|error| error.code).as_deref(), Some("severalRows"));
        assert_eq!(count(&engine), [[Value::Integer(2)]], "updating one of two equal rows changed none");
        assert!(engine.open_transactions("d").is_empty(), "rolled back, nothing left open");

        let set = read_all(&engine);
        set.stage(2, 0, Staged::Value(Value::Text("unique".into())));
        assert_eq!(save(&engine, &set, &editing).status, OperationStatus::Done, "a row unlike the others saves");
        run(&engine, "d", &setup, "DROP TABLE zen_dupes");
    }
}
