use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zen_db::dialect::{SqlDialect, table_id};
use zen_db::driver::{ContextKind, ErrorKind, ExecOptions};
use zen_db::edit::{self, KeyKind, Staged};
use zen_db::engine::{
    CommitMode, ConnectRequest, Engine, ModeChange, OperationState, OperationStatus, QueryOptions, QueryStatement,
    Timing, TransactionPolicy,
};
use zen_db::filter::{self, Operator, Sort, Source};
use zen_db::model::NodeKind;
use zen_db::registry::DataSource;
use zen_db::results::{ReadState, ResultSet};
use zen_db::schema::{ConstraintKind, DdlOptions, IntrospectRequest, ObjectKind, Snapshot};
use zen_db::sqlite::{ID, NO_FILE, PROGRAM, process};
use zen_db::types::{ConnectionType, DEVELOPMENT, PRODUCTION};
use zen_db::value::{Column, Value};

const WAIT: Duration = Duration::from_secs(30);

fn installed() -> bool {
    let found = process::find_program(None).is_ok();
    let note =
        if found { format!("{}\n", version()) } else { "skipped: the sqlite3 program is not installed\n".to_owned() };
    let _ = std::io::Write::write_all(&mut std::io::stderr(), note.as_bytes());
    found
}

type Version = (String, Option<(u32, u32, u32)>);

fn program_version() -> &'static Version {
    static VERSION: std::sync::OnceLock<Version> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        let program = process::find_program(None).unwrap();
        let output = std::process::Command::new(&program).arg("-version").output().unwrap();
        let reported = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (format!("sqlite3 {reported} at {}", program.display()), process::parse_version(&reported))
    })
}

fn version() -> &'static str {
    &program_version().0
}

fn engine() -> Engine {
    Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default())
}

fn source(id: &str, file: &Path) -> DataSource {
    DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: ID.to_owned(),
        database: Some(file.display().to_string()),
        ..DataSource::default()
    }
}

fn request(source: DataSource, kind: &str) -> ConnectRequest {
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == kind).unwrap();
    ConnectRequest { source, kind, password: None, stored: None, ssh_secret: None }
}

fn new_file(dir: &Path, name: &str) -> PathBuf {
    let file = dir.join(name);
    std::fs::File::create(&file).unwrap();
    file
}

fn wait(engine: &Engine, op: u64) -> OperationState {
    let state = engine.wait(op, WAIT).unwrap();
    assert_ne!(state.status, OperationStatus::Running, "{}", version());
    state
}

fn ok(engine: &Engine, op: u64) -> OperationState {
    let state = wait(engine, op);
    assert_eq!(state.status, OperationStatus::Done, "{}: {state:?}", version());
    state
}

fn exec(engine: &Engine, id: &str, sql: &str) -> zen_db::value::Execution {
    ok(engine, engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap()).execution.unwrap()
}

fn failed(engine: &Engine, id: &str, sql: &str) -> zen_db::driver::DbError {
    let state = wait(engine, engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap());
    assert_eq!(state.status, OperationStatus::Failed, "{}: {state:?}", version());
    state.error.unwrap()
}

fn connected(engine: &Engine, file: &Path, id: &str) {
    ok(engine, engine.connect(request(source(id, file), DEVELOPMENT)).unwrap());
}

#[test]
fn values_come_back_by_storage_class_with_counts_and_errors() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "values.db");
    let engine = engine();
    connected(&engine, &file, "v");
    exec(&engine, "v", "CREATE TABLE t (i INTEGER, r REAL, s TEXT, b BLOB, n NUMERIC, a)");
    let inserted = exec(
        &engine,
        "v",
        "INSERT INTO t VALUES (1, 1.5, 'a''b' || char(10) || 'c', x'00ff', '12', NULL), (2, 2, 'é', x'', 3.0, 'x');",
    );
    assert_eq!(inserted.affected, Some(2));
    let read = exec(&engine, "v", "SELECT * FROM t ORDER BY i");
    assert_eq!(
        read.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(),
        ["i", "r", "s", "b", "n", "a"]
    );
    let v = version();
    assert_eq!((read.columns[0].name.as_str(), read.columns[0].type_name.as_str()), ("i", "INTEGER"), "{v}: {read:?}");
    assert_eq!(read.columns[0].origin.as_deref(), Some("i"), "{v}: a plain SELECT of one table is traced to it");
    assert_eq!(read.columns[0].source.map(|source| source.table), Some(table_id("main", "t")), "{v}");
    let computed = exec(&engine, "v", "SELECT i + 1 FROM t");
    assert_eq!(computed.columns[0], Column::new("i + 1", "INTEGER"), "{v}");
    assert_eq!(read.columns[5].type_name, "TEXT", "{v}: NULLs do not count; no declared type");
    assert_eq!(read.columns[4].type_name, "NUMERIC", "{v}: the declared type of a traced column");
    exec(&engine, "v", "CREATE TABLE typed (d DATE, ts TIMESTAMP, j JSON, f BOOLEAN)");
    exec(&engine, "v", "INSERT INTO typed VALUES ('2026-10-28', '2026-10-28 10:00', '{}', 1)");
    let typed = exec(&engine, "v", "SELECT * FROM typed");
    assert_eq!(
        typed.columns.iter().map(|column| column.type_name.as_str()).collect::<Vec<_>>(),
        ["DATE", "TIMESTAMP", "JSON", "BOOLEAN"],
        "{v}"
    );
    assert_eq!(
        read.rows[0],
        vec![
            Value::Integer(1),
            Value::Float(1.5),
            Value::Text("a'b\nc".into()),
            Value::Bytes(vec![0, 255]),
            Value::Integer(12),
            Value::Null
        ],
        "{v}: NUMERIC affinity stored '12' as an integer"
    );
    assert_eq!(read.rows[1][1], Value::Float(2.0), "{v}: REAL affinity stored 2 as a real");
    assert_eq!(read.rows[1][4], Value::Integer(3), "{v}: NUMERIC affinity stored 3.0 as an integer");
    assert_eq!(read.affected, None);
    let updated = exec(&engine, "v", "UPDATE t SET a = 'y' WHERE i > 100");
    assert_eq!(updated.affected, Some(0));
    let error = failed(&engine, "v", "SELECT i,\n  nope FROM t");
    assert_eq!((error.kind, error.detail.as_str()), (ErrorKind::Server, "no such column: nope"), "{v}");
    if program_version().1.is_some_and(|known| known >= (3, 38, 0)) {
        assert_eq!(error.position, Some(13), "{v}: the position of `nope`");
    }
    assert_eq!(failed(&engine, "v", ".shell ls").kind, ErrorKind::Unsupported);
    assert_eq!(failed(&engine, "v", "SELECT 1; SELECT 2").detail, "one statement at a time");
    let capped = engine
        .execute(
            "v",
            ContextKind::Main,
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c LIMIT 1000) SELECT x FROM c",
            ExecOptions { max_rows: 10, timeout: None },
        )
        .unwrap();
    let capped = ok(&engine, capped).execution.unwrap();
    assert_eq!((capped.rows.len(), capped.truncated), (10, true));
    let empty = exec(&engine, "v", "-- nothing\n");
    assert!(empty.columns.is_empty() && empty.rows.is_empty());
    ok(&engine, engine.disconnect("v", TransactionPolicy::Rollback).unwrap());
}

#[test]
fn the_structure_is_read_with_fingerprints_and_ddl() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "shop.db");
    let engine = engine();
    connected(&engine, &file, "s");
    for sql in [
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE, created TEXT DEFAULT CURRENT_TIMESTAMP)",
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer INTEGER REFERENCES customers(id) ON DELETE CASCADE, total REAL, doubled REAL AS (total * 2))",
        "CREATE INDEX orders_customer ON orders (customer, total DESC)",
        "CREATE VIEW big AS SELECT * FROM orders WHERE total > 100",
        "CREATE TRIGGER orders_touch AFTER INSERT ON orders BEGIN UPDATE customers SET created = created WHERE id = new.customer; END",
        "CREATE TEMP TABLE old (x)",
    ] {
        exec(&engine, "s", sql);
    }
    let request = IntrospectRequest { force: false, ..IntrospectRequest::default() };
    let op = engine.introspect("s", request.clone(), Snapshot::new("s", ID), None).unwrap();
    let state = ok(&engine, op);
    assert_eq!(state.introspected.unwrap().read, ["shop.db.main", "shop.db.temp"]);
    let snapshot = engine.take_snapshot(op).unwrap();
    assert!(snapshot.server_version.as_deref().is_some_and(|version| version.starts_with("SQLite 3.")));
    let main = snapshot.schema("shop.db", "main").unwrap();
    assert!(main.fingerprint.is_some());
    let orders = main.objects.iter().find(|object| object.name == "orders").unwrap();
    assert_eq!(orders.kind, ObjectKind::Table);
    assert_eq!(
        orders.columns.iter().map(|column| (column.name.as_str(), column.type_name.as_str())).collect::<Vec<_>>(),
        [("id", "INTEGER"), ("customer", "INTEGER"), ("total", "REAL"), ("doubled", "REAL")]
    );
    assert_eq!(orders.columns[3].generated.as_deref(), Some("virtual"));
    let key = orders.constraints.iter().find(|constraint| constraint.kind == ConstraintKind::ForeignKey).unwrap();
    let reference = key.references.as_ref().unwrap();
    assert_eq!((reference.table.as_str(), reference.on_delete.as_str()), ("customers", "cascade"));
    assert_eq!(orders.indexes[0].columns, ["customer", "total"]);
    assert_eq!(orders.triggers[0].name, "orders_touch");
    let customers = main.objects.iter().find(|object| object.name == "customers").unwrap();
    assert!(customers.constraints.iter().any(|constraint| constraint.kind == ConstraintKind::Unique));
    assert!(customers.constraints.iter().any(|constraint| constraint.kind == ConstraintKind::PrimaryKey));
    assert!(!customers.columns[1].nullable);
    assert!(main.objects.iter().any(|object| object.name == "big" && object.kind == ObjectKind::View));
    assert!(snapshot.schema("shop.db", "temp").unwrap().objects.iter().any(|object| object.name == "old"));

    let again = engine.introspect("s", request.clone(), snapshot.clone(), None).unwrap();
    let state = ok(&engine, again);
    assert_eq!(state.introspected.as_ref().map(|read| (read.read.len(), read.unchanged)), Some((0, 2)));
    exec(&engine, "s", "CREATE TEMP TABLE newer (y)");
    let changed = engine.introspect("s", request, snapshot.clone(), None).unwrap();
    assert_eq!(ok(&engine, changed).introspected.unwrap().read, ["shop.db.temp"]);

    let driver = engine.driver(ID).unwrap();
    let path: Vec<String> = ["shop.db", "main", "tables", "orders"].map(str::to_owned).to_vec();
    let ddl = driver.ddl(&snapshot, &path, &DdlOptions { nested: true, ..DdlOptions::default() }).unwrap();
    let v = version();
    assert!(ddl.starts_with("CREATE TABLE orders ("), "{v}: {ddl}");
    assert!(ddl.contains("CREATE INDEX orders_customer") && ddl.contains("CREATE TRIGGER orders_touch"), "{v}: {ddl}");
    let tree = ok(&engine, engine.children("s", ["shop.db".to_owned(), "main".to_owned()].to_vec()).unwrap());
    let folders: Vec<String> = tree.nodes.unwrap().into_iter().map(|node| node.label).collect();
    assert_eq!(folders, ["Tables", "Views"], "SQLite has no functions or sequences");
    let schemas = ok(&engine, engine.children("s", vec![file.display().to_string()]).unwrap());
    let schemas: Vec<String> = schemas.nodes.unwrap().into_iter().map(|node| node.label).collect();
    assert_eq!(schemas, ["main", "temp"], "the one catalogue answers whatever names it, as consoles name it by path");
    let columns = ok(&engine, engine.children("s", [path.clone(), vec!["columns".to_owned()]].concat()).unwrap());
    assert!(columns.nodes.unwrap().iter().all(|node| node.kind == NodeKind::Column));
    ok(&engine, engine.disconnect("s", TransactionPolicy::Rollback).unwrap());
}

fn run(engine: &Engine, id: &str, context: &ContextKind, sql: &str) -> OperationStatus {
    let statement = QueryStatement {
        text: sql.to_owned(),
        start: 0,
        end: sql.len(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let options = QueryOptions { rollback_on_error: false, ..QueryOptions::default() };
    let op = engine.run_query(id, context.clone(), vec![statement], options).unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            return query.status;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn outside_count(file: &Path) -> String {
    let program = process::find_program(None).unwrap();
    let output = std::process::Command::new(program)
        .arg("-batch")
        .arg("-readonly")
        .arg(file)
        .arg("SELECT count(*) FROM t")
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn manual_commit_holds_changes_until_commit_or_rollback() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "tx.db");
    let engine = engine();
    let prod = request(source("tx", &file), PRODUCTION);
    ok(&engine, engine.connect(prod.clone()).unwrap());
    let console = ContextKind::Console("tx/console".to_owned());
    let auto = ModeChange { mode: Some(CommitMode::Auto), isolation: None };
    if let Some(op) = engine.set_mode("tx", console.clone(), auto, TransactionPolicy::Ask).unwrap() {
        ok(&engine, op);
    }
    assert_eq!(run(&engine, "tx", &console, "CREATE TABLE t (a)"), OperationStatus::Done);
    let manual = ModeChange { mode: Some(CommitMode::Manual), isolation: Some(Some("immediate".to_owned())) };
    ok(&engine, engine.set_mode("tx", console.clone(), manual, TransactionPolicy::Ask).unwrap().unwrap());
    assert_eq!(run(&engine, "tx", &console, "INSERT INTO t VALUES (1)"), OperationStatus::Done);
    let state = engine.transaction(&prod, &console, 10);
    assert!(state.in_transaction && !state.auto_commit, "{state:?}");
    assert_eq!(outside_count(&file), "0", "another program does not see the uncommitted row");
    ok(&engine, engine.end_transaction("tx", console.clone(), true).unwrap());
    assert_eq!(outside_count(&file), "1");
    assert_eq!(run(&engine, "tx", &console, "DELETE FROM t"), OperationStatus::Done);
    ok(&engine, engine.end_transaction("tx", console.clone(), false).unwrap());
    assert_eq!(outside_count(&file), "1", "the delete was rolled back");
    ok(&engine, engine.disconnect("tx", TransactionPolicy::Rollback).unwrap());
}

#[test]
fn a_savepoint_never_commits_a_manual_transaction_and_its_release_ends_one_it_opened() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "savepoint.db");
    let engine = engine();
    let prod = request(source("sp", &file), PRODUCTION);
    ok(&engine, engine.connect(prod.clone()).unwrap());
    let main = ContextKind::Main;
    let mode = |mode| {
        let change = ModeChange { mode: Some(mode), isolation: None };
        if let Some(op) = engine.set_mode("sp", main.clone(), change, TransactionPolicy::Commit).unwrap() {
            ok(&engine, op);
        }
    };
    mode(CommitMode::Auto);
    assert_eq!(run(&engine, "sp", &main, "CREATE TABLE t (id INTEGER PRIMARY KEY, a)"), OperationStatus::Done);
    mode(CommitMode::Manual);
    for sql in ["SAVEPOINT s", "INSERT INTO t (a) VALUES (1)", "RELEASE SAVEPOINT s"] {
        assert_eq!(run(&engine, "sp", &main, sql), OperationStatus::Done, "{sql}");
    }
    let state = engine.transaction(&prod, &main, 10);
    assert!(state.in_transaction, "{state:?}");
    assert_eq!(outside_count(&file), "0", "releasing the savepoint left the manual transaction open");
    ok(&engine, engine.end_transaction("sp", main.clone(), false).unwrap());
    assert_eq!(outside_count(&file), "0");

    let op = engine.introspect("sp", IntrospectRequest::default(), Snapshot::new("sp", ID), None).unwrap();
    ok(&engine, op);
    let mut object = engine.take_snapshot(op).unwrap().schema("savepoint.db", "main").unwrap().objects[0].clone();
    object.oid = table_id("main", "t");
    let set = read_set(&engine, "sp", "SELECT * FROM main.t ORDER BY id");
    let editing = edit::identify(&set.columns(), "main", &object, None, SqlDialect::Sqlite);
    set.add_row(vec![(1, Staged::Value(Value::Integer(2)))]);
    let state = wait(&engine, engine.save_edits(set.id(), edit::plan(&editing, &set.changes())).unwrap());
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!(outside_count(&file), "0", "a save in manual mode waits for Commit");
    let pending = engine.transaction(&prod, &main, 10);
    assert!(pending.in_transaction && pending.modifying == 1, "{pending:?}");
    ok(&engine, engine.end_transaction("sp", main.clone(), true).unwrap());
    assert_eq!(outside_count(&file), "1");

    mode(CommitMode::Auto);
    for sql in ["SAVEPOINT outer", "SAVEPOINT inner", "INSERT INTO t (a) VALUES (3)", "RELEASE inner"] {
        assert_eq!(run(&engine, "sp", &main, sql), OperationStatus::Done, "{sql}");
    }
    assert!(engine.transaction(&prod, &main, 0).in_transaction, "the outer savepoint holds the transaction");
    assert_eq!(run(&engine, "sp", &main, "RELEASE outer"), OperationStatus::Done);
    assert!(!engine.transaction(&prod, &main, 0).in_transaction, "releasing the outer savepoint committed");
    assert_eq!(outside_count(&file), "2");
    ok(&engine, engine.disconnect("sp", TransactionPolicy::Rollback).unwrap());
}

#[test]
fn a_trigger_cut_short_after_a_case_is_refused_instead_of_waiting_for_its_end() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "trigger.db");
    let engine = engine();
    connected(&engine, &file, "tr");
    exec(&engine, "tr", "CREATE TABLE a (x)");
    let cut = "CREATE TRIGGER t AFTER INSERT ON a BEGIN SELECT CASE WHEN 1 THEN 2 END;";
    let op = engine.execute("tr", ContextKind::Main, cut, ExecOptions::default()).unwrap();
    let state = engine.wait(op, Duration::from_secs(10)).unwrap();
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    let whole = "CREATE TRIGGER t AFTER INSERT ON a BEGIN SELECT CASE WHEN new.x THEN 2 ELSE 3 END; END";
    exec(&engine, "tr", whole);
    exec(&engine, "tr", "INSERT INTO a VALUES (1)");
    assert_eq!(
        exec(&engine, "tr", "SELECT count(*) FROM sqlite_master WHERE type = 'trigger'").rows,
        [[Value::Integer(1)]]
    );
    ok(&engine, engine.disconnect("tr", TransactionPolicy::Rollback).unwrap());
}

#[test]
fn cancel_interrupts_the_statement_and_the_next_one_runs() {
    if !installed() || !cfg!(unix) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "cancel.db");
    let engine = engine();
    connected(&engine, &file, "c");
    let endless = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c";
    let op = engine.execute("c", ContextKind::Main, endless, ExecOptions::default()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    assert!(engine.cancel(op));
    let state = wait(&engine, op);
    assert_eq!(state.status, OperationStatus::Cancelled, "{state:?}");
    if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }
    let after = exec(&engine, "c", "SELECT 41 + 1");
    assert_eq!(after.rows, [[Value::Integer(42)]]);
    let timed = engine
        .execute(
            "c",
            ContextKind::Main,
            endless,
            ExecOptions { max_rows: 10, timeout: Some(Duration::from_millis(300)) },
        )
        .unwrap();
    assert_eq!(wait(&engine, timed).status, OperationStatus::Failed);
    assert_eq!(exec(&engine, "c", "SELECT 1").rows, [[Value::Integer(1)]]);
    ok(&engine, engine.disconnect("c", TransactionPolicy::Rollback).unwrap());
}

#[test]
fn files_open_only_as_asked() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let engine = engine();
    let missing = dir.path().join("missing.db");
    let state = wait(&engine, engine.connect(request(source("m", &missing), DEVELOPMENT)).unwrap());
    assert_eq!(state.error.as_ref().and_then(|error| error.code.as_deref()), Some(NO_FILE), "{state:?}");
    assert!(!missing.exists(), "a missing file is never created by connecting");

    let file = new_file(dir.path(), "ro.db");
    connected(&engine, &file, "rw");
    exec(&engine, "rw", "CREATE TABLE t (a)");
    ok(&engine, engine.disconnect("rw", TransactionPolicy::Rollback).unwrap());
    let read_only = DataSource { read_only: true, ..source("ro", &file) };
    ok(&engine, engine.connect(request(read_only, DEVELOPMENT)).unwrap());
    assert!(exec(&engine, "ro", "SELECT count(*) FROM t").rows[0][0] == Value::Integer(0));
    let refused = failed(&engine, "ro", "INSERT INTO t VALUES (1)");
    assert!(refused.detail.contains("readonly"), "{refused:?}");

    let memory = DataSource { database: Some(":memory:".to_owned()), ..source("mem", &file) };
    ok(&engine, engine.connect(request(memory, DEVELOPMENT)).unwrap());
    exec(&engine, "mem", "CREATE TABLE m (a)");
    assert_eq!(exec(&engine, "mem", "SELECT count(*) FROM m").rows[0][0], Value::Integer(0));

    let mut nowhere = source("np", &file);
    nowhere.settings.insert(PROGRAM.to_owned(), serde_json::json!("/nonexistent/sqlite3"));
    let state = wait(&engine, engine.connect(request(nowhere, DEVELOPMENT)).unwrap());
    assert_eq!(state.error.and_then(|error| error.code), Some(process::MISSING.to_owned()));
    let not_a_database = dir.path().join("text.db");
    std::fs::write(&not_a_database, "hello, this is not a database at all, but it is long enough").unwrap();
    let state = wait(&engine, engine.connect(request(source("txt", &not_a_database), DEVELOPMENT)).unwrap());
    assert_eq!(state.status, OperationStatus::Failed);
    assert!(state.error.unwrap().detail.contains("not a database"));
}

#[test]
fn nothing_sent_runs_a_shell_command_or_touches_files_outside_the_database() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "safe.db");
    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "hidden from agents").unwrap();
    let shelled = dir.path().join("shelled");
    let written = dir.path().join("written.txt");
    let engine = engine();
    connected(&engine, &file, "rw");
    exec(&engine, "rw", "CREATE TABLE t (a)");
    ok(&engine, engine.disconnect("rw", TransactionPolicy::Rollback).unwrap());
    let read_only = DataSource { read_only: true, ..source("ro", &file) };
    ok(&engine, engine.connect(request(read_only, DEVELOPMENT)).unwrap());
    connected(&engine, &file, "rw");
    let touch = format!("touch '{}'", shelled.display());
    for id in ["ro", "rw"] {
        for sql in [
            format!("SELECT 1\n/\n.shell {touch}"),
            format!("SELECT 1\nGO\n.system {touch}"),
            format!("SELECT 1\n go -- done\n.shell {touch}"),
            format!("SELECT writefile('{}', 'x')", written.display()),
            format!("SELECT readfile('{}')", secret.display()),
            format!("ATTACH '{}' AS other", secret.display()),
            format!("VACUUM INTO '{}'", written.display()),
        ] {
            let state = wait(&engine, engine.execute(id, ContextKind::Main, &sql, ExecOptions::default()).unwrap());
            assert_eq!(state.status, OperationStatus::Failed, "{}: {id}: {sql}: {state:?}", version());
            assert!(!format!("{state:?}").contains("hidden from agents"), "{id}: {sql}: {state:?}");
            assert!(!shelled.exists(), "{}: {id}: {sql} ran a shell command", version());
            assert!(!written.exists(), "{}: {id}: {sql} wrote a file", version());
            assert_eq!(exec(&engine, id, "SELECT count(*) FROM t").rows, [[Value::Integer(0)]], "{id}: {sql}");
        }
    }
}

#[cfg(unix)]
#[test]
fn an_sqlite3_without_safe_mode_is_refused_with_its_version() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("sqlite3");
    std::fs::write(&program, "#!/bin/sh\necho '3.36.0 2021-06-18 18:36:39 old'\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let file = new_file(dir.path(), "old.db");
    let mut old = source("old", &file);
    old.settings.insert(PROGRAM.to_owned(), serde_json::json!(program.display().to_string()));
    let engine = engine();
    let state = wait(&engine, engine.connect(request(old, DEVELOPMENT)).unwrap());
    let error = state.error.unwrap();
    assert_eq!(error.code.as_deref(), Some(process::MISSING), "{error:?}");
    assert!(error.detail.contains("3.36.0") && error.detail.contains("3.37.0"), "{error:?}");
}

fn read_set(engine: &Engine, id: &str, sql: &str) -> std::sync::Arc<ResultSet> {
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
        assert!(Instant::now() < deadline);
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

fn ids(engine: &Engine, id: &str, sql: &str) -> Vec<Value> {
    exec(engine, id, sql).rows.into_iter().map(|row| row[0].clone()).collect()
}

#[test]
fn filters_sorts_quick_filters_and_edits_are_written_in_sqlite() {
    if !installed() {
        return;
    }
    const SQLITE: SqlDialect = SqlDialect::Sqlite;
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "edit.db");
    let engine = engine();
    connected(&engine, &file, "e");
    exec(
        &engine,
        "e",
        "CREATE TABLE \"Orders\" (id INTEGER PRIMARY KEY, \"Note\" TEXT, data BLOB, flag INTEGER NOT NULL DEFAULT 7, price REAL)",
    );
    exec(
        &engine,
        "e",
        "INSERT INTO \"Orders\" VALUES (1, 'a\\b', x'01', 0, 1.5), (2, '50%', NULL, 1, NULL), (3, 'it''s', x'ff', 0, 2.5), (4, NULL, NULL, 1, NULL), (5, 'n_5', NULL, 0, 3.0)",
    );
    let names: Vec<String> = ["id", "Note", "data", "flag", "price"].map(str::to_owned).to_vec();
    let table = SQLITE.qualified("main", "Orders");
    let source = Source::Table { name: &table, order: Some("id") };
    let by_price = [Sort { column: 4, name: "price".into(), descending: true }];
    let (sql, _) =
        filter::compose(source, Some("price IS NOT NULL OR \"Note\" IS NULL"), &by_price, &names, SQLITE).unwrap();
    let int = |values: &[i64]| values.iter().map(|value| Value::Integer(*value)).collect::<Vec<_>>();
    assert_eq!(ids(&engine, "e", &sql), int(&[4, 5, 3, 1]), "NULLs first going down, as in memory: {sql}");
    let up = [Sort { column: 4, name: "price".into(), descending: false }];
    let (sql, _) = filter::compose(source, None, &up, &names, SQLITE).unwrap();
    assert_eq!(ids(&engine, "e", &sql), int(&[1, 3, 5, 2, 4]), "and last going up");
    let quick = |operator, values: &[Value]| {
        let condition = filter::quick("Note", operator, values, SQLITE);
        let (sql, _) = filter::compose(source, Some(&condition), &[], &names, SQLITE).unwrap();
        ids(&engine, "e", &sql)
    };
    assert_eq!(quick(Operator::Contains, &[Value::Text("50%".into())]), int(&[2]), "% is matched as itself");
    assert_eq!(quick(Operator::Contains, &[Value::Text("_".into())]), int(&[5]), "and _ too");
    assert_eq!(quick(Operator::Equal, &[Value::Text("a\\b".into())]), int(&[1]));
    assert_eq!(quick(Operator::Equal, &[Value::Text("it's".into())]), int(&[3]));
    assert_eq!(quick(Operator::In, &[Value::Text("it's".into()), Value::Null]), int(&[3, 4]));
    let data = filter::quick("data", Operator::Equal, &[Value::Bytes(vec![0xff])], SQLITE);
    assert_eq!(ids(&engine, "e", &filter::compose(source, Some(&data), &[], &names, SQLITE).unwrap().0), int(&[3]));

    let op = engine.introspect("e", IntrospectRequest::default(), Snapshot::new("e", ID), None).unwrap();
    ok(&engine, op);
    let snapshot = engine.take_snapshot(op).unwrap();
    let mut object = snapshot.schema("edit.db", "main").unwrap().objects[0].clone();
    object.oid = table_id("main", "Orders");
    let set = read_set(&engine, "e", &format!("SELECT * FROM {table} ORDER BY id"));
    let editing = edit::identify(&set.columns(), "main", &object, None, SQLITE);
    assert!(editing.editable && editing.key.as_ref().unwrap().kind == KeyKind::PrimaryKey, "{editing:?}");
    set.stage(0, 1, Staged::Value(Value::Text("it's; DROP TABLE \"Orders\"; --".into())));
    set.stage(1, 2, Staged::Value(Value::Bytes(vec![0, 1])));
    set.stage(2, 3, Staged::Default);
    set.delete_rows(&[4]);
    set.add_row(vec![(1, Staged::Value(Value::Text("new".into())))]);
    let planned = edit::plan(&editing, &set.changes());
    let state = wait(&engine, engine.save_edits(set.id(), planned).unwrap());
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    let report = state.saved.unwrap();
    assert_eq!((report.deleted, report.inserted, report.updated), (1, 1, 3));
    let saved = exec(&engine, "e", "SELECT id, \"Note\", data, flag FROM \"Orders\" ORDER BY id").rows;
    assert_eq!(saved.len(), 5);
    assert_eq!(saved[0][1], Value::Text("it's; DROP TABLE \"Orders\"; --".into()), "quoted, never spliced");
    assert_eq!(saved[1][2], Value::Bytes(vec![0, 1]));
    assert_eq!(saved[2][3], Value::Integer(7), "the column's default");
    assert_eq!(saved[4][..2], [Value::Integer(5), Value::Text("new".into())], "the deleted key is free again");
    assert_eq!(set.get(4, 0), Value::Integer(5), "the new row's key came back with RETURNING");
    assert_eq!(set.get(4, 3), Value::Integer(7));

    let set = read_set(&engine, "e", &format!("SELECT * FROM {table} ORDER BY id"));
    set.stage(0, 1, Staged::Value(Value::Text("mine".into())));
    exec(&engine, "e", "UPDATE \"Orders\" SET \"Note\" = 'theirs' WHERE id = 1");
    let state = wait(&engine, engine.save_edits(set.id(), edit::plan(&editing, &set.changes())).unwrap());
    assert_eq!(state.status, OperationStatus::Failed);
    assert_eq!(state.error.unwrap().code.as_deref(), Some("conflict"));
    assert_eq!(
        exec(&engine, "e", "SELECT \"Note\" FROM \"Orders\" WHERE id = 1").rows[0][0],
        Value::Text("theirs".into())
    );
    ok(&engine, engine.disconnect("e", TransactionPolicy::Rollback).unwrap());
}

#[test]
fn a_key_of_every_column_never_changes_several_equal_rows() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "dupes.db");
    let engine = engine();
    connected(&engine, &file, "d");
    exec(&engine, "d", "CREATE TABLE notes (body TEXT, n INTEGER)");
    exec(&engine, "d", "INSERT INTO notes VALUES ('same', 1), ('same', 1), ('other', 2)");
    let op = engine.introspect("d", IntrospectRequest::default(), Snapshot::new("d", ID), None).unwrap();
    ok(&engine, op);
    let snapshot = engine.take_snapshot(op).unwrap();
    let mut object = snapshot.schema("dupes.db", "main").unwrap().objects[0].clone();
    object.oid = table_id("main", "notes");
    let set = read_set(&engine, "d", "SELECT * FROM main.notes ORDER BY rowid");
    let editing =
        edit::identify(&set.columns(), "main", &object, Some(&edit::KeyChoice::AllColumns), SqlDialect::Sqlite);
    assert!(editing.editable && editing.key.as_ref().unwrap().kind == KeyKind::AllColumns, "{editing:?}");
    let count = |engine: &Engine| exec(engine, "d", "SELECT count(*) FROM notes WHERE body = 'same'").rows;

    set.delete_rows(&[0]);
    let state = wait(&engine, engine.save_edits(set.id(), edit::plan(&editing, &set.changes())).unwrap());
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    assert_eq!(state.error.and_then(|error| error.code).as_deref(), Some("severalRows"));
    assert_eq!(count(&engine), [[Value::Integer(2)]], "deleting one of two equal rows deleted none");

    let set = read_set(&engine, "d", "SELECT * FROM main.notes ORDER BY rowid");
    set.stage(1, 0, Staged::Value(Value::Text("changed".into())));
    let state = wait(&engine, engine.save_edits(set.id(), edit::plan(&editing, &set.changes())).unwrap());
    assert_eq!(state.error.and_then(|error| error.code).as_deref(), Some("severalRows"));
    assert_eq!(count(&engine), [[Value::Integer(2)]], "updating one of two equal rows changed none");

    let set = read_set(&engine, "d", "SELECT * FROM main.notes ORDER BY rowid");
    set.stage(2, 0, Staged::Value(Value::Text("unique".into())));
    let state = wait(&engine, engine.save_edits(set.id(), edit::plan(&editing, &set.changes())).unwrap());
    assert_eq!(state.status, OperationStatus::Done, "a row unlike the others still saves: {state:?}");
    ok(&engine, engine.disconnect("d", TransactionPolicy::Rollback).unwrap());
}

#[test]
fn an_empty_table_and_an_empty_result_keep_their_columns() {
    if !installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "empty.db");
    let engine = engine();
    connected(&engine, &file, "z");
    exec(&engine, "z", "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL, n)");
    let v = version();
    let table = exec(&engine, "z", "SELECT * FROM t");
    assert!(table.rows.is_empty(), "{v}");
    assert_eq!(
        table.columns.iter().map(|column| (column.name.as_str(), column.type_name.as_str())).collect::<Vec<_>>(),
        [("id", "INTEGER"), ("name", "TEXT"), ("n", "")],
        "{v}: an empty table has its columns with their declared types (!521); none for a column declared without one"
    );
    assert_eq!(table.columns[1].source.map(|source| source.table), Some(table_id("main", "t")), "{v}");
    let set = read_set(&engine, "z", "SELECT * FROM t LIMIT 200");
    assert_eq!(set.columns().iter().map(|column| column.name.as_str()).collect::<Vec<_>>(), ["id", "name", "n"]);
    assert_eq!(set.info().rows, 0);
    let query = read_set(&engine, "z", "SELECT id, id, name || '!' AS shout FROM t WHERE id > 0 ORDER BY id");
    assert_eq!(query.columns().iter().map(|column| column.name.as_str()).collect::<Vec<_>>(), ["id", "id", "shout"]);
    assert_eq!(query.info().rows, 0);
    let values = exec(&engine, "z", "WITH w(a, b) AS (VALUES (1, 2)) SELECT b, a FROM w WHERE a > 5");
    assert_eq!(values.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    assert!(exec(&engine, "z", "UPDATE t SET n = 1").columns.is_empty());
    assert!(exec(&engine, "z", "WITH x AS (SELECT 1) DELETE FROM t").columns.is_empty());
    assert_eq!(exec(&engine, "z", "SELECT count(*) FROM temp.sqlite_master").rows, [[Value::Integer(0)]]);
    ok(&engine, engine.disconnect("z", TransactionPolicy::Rollback).unwrap());
    let read_only = DataSource { read_only: true, ..source("zr", &file) };
    ok(&engine, engine.connect(request(read_only, DEVELOPMENT)).unwrap());
    assert_eq!(exec(&engine, "zr", "SELECT name FROM t").columns[0].name, "name", "{v}");
    assert_eq!(exec(&engine, "zr", "PRAGMA query_only").rows, [[Value::Integer(1)]], "{v}: still read-only");
    assert!(failed(&engine, "zr", "CREATE TEMP TABLE scratch (a)").detail.contains("readonly"));
    ok(&engine, engine.disconnect("zr", TransactionPolicy::Rollback).unwrap());
}

fn start_query(engine: &Engine, id: &str, sql: &str) -> u64 {
    let statement = QueryStatement {
        text: sql.to_owned(),
        start: 0,
        end: sql.len(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    engine.run_query(id, ContextKind::Main, vec![statement], QueryOptions::default()).unwrap()
}

fn finished(engine: &Engine, op: u64) -> zen_db::engine::QueryState {
    let deadline = Instant::now() + WAIT;
    loop {
        let query = engine.query(op).unwrap();
        if query.status != OperationStatus::Running {
            return query;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn overlapping_reads_and_cancels_on_one_connection_never_end_the_next_read() {
    if !installed() || !cfg!(unix) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = new_file(dir.path(), "race.db");
    let engine = engine();
    connected(&engine, &file, "r");
    exec(&engine, "r", "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
    exec(
        &engine,
        "r",
        "INSERT INTO t WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 5000) SELECT x, 'v' || x FROM c",
    );
    let slow = "SELECT t.id, count(*) FROM t JOIN t AS u ON u.id <= t.id / 50 GROUP BY t.id ORDER BY t.id LIMIT 20";
    let quick = "SELECT * FROM t ORDER BY id LIMIT 200";
    for step in 0..60u64 {
        let first = start_query(&engine, "r", if step % 2 == 0 { slow } else { quick });
        std::thread::sleep(Duration::from_micros(step * 300));
        let second = start_query(&engine, "r", quick);
        engine.cancel_query(first);
        let third = start_query(&engine, "r", quick);
        if step % 3 == 0 {
            engine.cancel_query(third);
        }
        let first = finished(&engine, first);
        assert!(
            matches!(first.status, OperationStatus::Done | OperationStatus::Cancelled),
            "{v}, step {step}: {first:?}",
            v = version()
        );
        for (op, cancelled) in [(second, false), (third, step % 3 == 0)] {
            let query = finished(&engine, op);
            if cancelled && query.status == OperationStatus::Cancelled {
                continue;
            }
            assert_eq!(query.status, OperationStatus::Done, "{v}, step {step}: {query:?}", v = version());
            let set = engine.results().get(query.statements[0].result.unwrap()).unwrap();
            while set.info().state == ReadState::Reading {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(set.info().rows, 200, "step {step}");
        }
    }
    assert_eq!(exec(&engine, "r", "SELECT count(*) FROM t").rows, [[Value::Integer(5000)]]);
    ok(&engine, engine.disconnect("r", TransactionPolicy::Rollback).unwrap());
}
