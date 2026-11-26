use std::path::Path;
use std::time::{Duration, Instant};

use zen_db::alter::{self, Change, ColumnPatch, ColumnSpec, ConstraintSpec, IndexSpec, ReferenceSpec, SequenceSpec};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::engine::{
    ConnectRequest, Engine, OperationStatus, QueryOptions, QueryState, QueryStatement, StatementStatus,
};
use zen_db::registry::DataSource;
use zen_db::schema::{Catalog, ConstraintKind, IntrospectRequest, LARGE_SCHEMA, Object, ObjectKind, Snapshot};
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(60);

fn engine() -> Engine {
    Engine::with_timing(zen_db::drivers(), "Zen Test", zen_db::engine::Timing::default())
}

fn request(source: DataSource, password: Option<&str>) -> ConnectRequest {
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    ConnectRequest {
        source,
        kind,
        password: password.map(|text| Secret::new(text.to_owned())),
        stored: None,
        ssh_secret: None,
    }
}

fn ok(engine: &Engine, op: u64) {
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
}

fn exec(engine: &Engine, id: &str, sql: &str) -> zen_db::value::Execution {
    let op = engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{sql}: {state:?}");
    state.execution.unwrap()
}

fn run(engine: &Engine, id: &str, statements: &[String]) -> QueryState {
    let mut at = 0;
    let statements = statements
        .iter()
        .map(|text| {
            let length = text.chars().count();
            let statement = QueryStatement {
                text: text.clone(),
                start: at,
                end: at + length,
                client: false,
                params: Vec::new(),
                kept: None,
            };
            at += length + 3;
            statement
        })
        .collect();
    let op = engine.run_query(id, ContextKind::Main, statements, QueryOptions::default()).unwrap();
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

fn ran(query: &QueryState) {
    assert!(
        query.statements.iter().all(|statement| statement.status == StatementStatus::Done),
        "every statement ran: {query:#?}"
    );
}

fn introspect(engine: &Engine, id: &str, driver: &str, schemas: &[&str]) -> Snapshot {
    let request = IntrospectRequest {
        schemas: schemas.iter().map(|schema| (*schema).to_owned()).collect(),
        force: true,
        large_schema: LARGE_SCHEMA,
        ..IntrospectRequest::default()
    };
    let op = engine.introspect(id, request, Snapshot::new(id, driver), None).unwrap();
    ok(engine, op);
    engine.take_snapshot(op).unwrap()
}

fn current(snapshot: &Snapshot) -> &Catalog {
    snapshot.catalogs.iter().find(|catalog| catalog.current).unwrap_or(&snapshot.catalogs[0])
}

fn object<'a>(catalog: &'a Catalog, schema: &str, kind: ObjectKind, name: &str) -> Option<&'a Object> {
    catalog
        .schemas
        .iter()
        .find(|known| known.name == schema)?
        .objects
        .iter()
        .find(|object| object.kind == kind && object.name == name)
}

fn spec(name: &str, type_name: &str) -> ColumnSpec {
    ColumnSpec { name: name.to_owned(), type_name: type_name.to_owned(), ..ColumnSpec::default() }
}

fn changes_of_a_table(schema: &str, numeric: &str) -> Vec<Change> {
    let s = || schema.to_owned();
    vec![
        Change::AddColumn {
            schema: s(),
            table: "orders".to_owned(),
            column: ColumnSpec {
                not_null: true,
                default: Some("''".to_owned()),
                comment: Some("Where to write".to_owned()),
                ..spec("email", "varchar(200)")
            },
        },
        Change::AlterColumn {
            schema: s(),
            table: "orders".to_owned(),
            column: "total".to_owned(),
            patch: ColumnPatch {
                name: Some("amount".to_owned()),
                type_name: Some(numeric.to_owned()),
                not_null: Some(true),
                default: Some("0".to_owned()),
                comment: Some("Paid".to_owned()),
            },
        },
        Change::AddConstraint {
            schema: s(),
            table: "orders".to_owned(),
            constraint: ConstraintSpec {
                name: Some("orders_customer_fk".to_owned()),
                kind: ConstraintKind::ForeignKey,
                columns: vec!["note".to_owned()],
                check: None,
                references: Some(ReferenceSpec {
                    schema: None,
                    table: "customers".to_owned(),
                    columns: vec!["code".to_owned()],
                    on_delete: Some("set null".to_owned()),
                    on_update: None,
                }),
            },
        },
        Change::AddConstraint {
            schema: s(),
            table: "orders".to_owned(),
            constraint: ConstraintSpec {
                name: Some("orders_amount_check".to_owned()),
                kind: ConstraintKind::Check,
                columns: Vec::new(),
                check: Some("amount >= 0".to_owned()),
                references: None,
            },
        },
        Change::CreateIndex {
            schema: s(),
            table: "orders".to_owned(),
            index: IndexSpec {
                name: Some("orders_email_key".to_owned()),
                columns: vec!["email".to_owned()],
                unique: true,
            },
        },
        Change::Comment {
            schema: s(),
            kind: ObjectKind::Table,
            name: "orders".to_owned(),
            comment: "What was bought".to_owned(),
        },
        Change::CreateView {
            schema: s(),
            name: "big".to_owned(),
            query: format!("SELECT id, amount FROM {schema}.orders WHERE amount > 100"),
        },
        Change::Rename { schema: s(), kind: ObjectKind::Table, name: "orders".to_owned(), to: "purchases".to_owned() },
    ]
}

fn check_the_table(catalog: &Catalog, schema: &str) {
    let purchases = object(catalog, schema, ObjectKind::Table, "purchases").expect("renamed");
    assert!(object(catalog, schema, ObjectKind::Table, "orders").is_none());
    let columns: Vec<(&str, bool)> =
        purchases.columns.iter().map(|column| (column.name.as_str(), column.nullable)).collect();
    assert_eq!(columns, [("id", false), ("amount", false), ("note", true), ("email", false)]);
    assert_eq!(purchases.columns[1].comment.as_deref(), Some("Paid"));
    assert_eq!(purchases.columns[3].comment.as_deref(), Some("Where to write"));
    assert_eq!(purchases.comment.as_deref(), Some("What was bought"));
    let kinds: Vec<(ConstraintKind, &str)> =
        purchases.constraints.iter().map(|constraint| (constraint.kind, constraint.name.as_str())).collect();
    assert!(kinds.contains(&(ConstraintKind::ForeignKey, "orders_customer_fk")), "{kinds:?}");
    assert!(kinds.contains(&(ConstraintKind::Check, "orders_amount_check")), "{kinds:?}");
    assert!(
        purchases.indexes.iter().any(|index| index.name == "orders_email_key" && index.unique),
        "{:?}",
        purchases.indexes
    );
    assert!(object(catalog, schema, ObjectKind::View, "big").is_some());
}

fn pg_ports() -> Vec<u16> {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn postgresql_runs_the_previewed_script_all_or_nothing() {
    for port in pg_ports() {
        let engine = engine();
        let source = DataSource {
            id: "alter".to_owned(),
            name: "alter".to_owned(),
            driver: "postgresql".to_owned(),
            host: Some("127.0.0.1".to_owned()),
            port: Some(port),
            database: Some("postgres".to_owned()),
            user: Some("postgres".to_owned()),
            ..DataSource::default()
        };
        ok(&engine, engine.connect(request(source, Some("zen"))).unwrap());
        exec(&engine, "alter", "DROP SCHEMA IF EXISTS zen_alter CASCADE");
        exec(&engine, "alter", "CREATE SCHEMA zen_alter");
        exec(&engine, "alter", "CREATE TABLE zen_alter.customers (code text PRIMARY KEY)");
        exec(&engine, "alter", "CREATE TABLE zen_alter.orders (id integer PRIMARY KEY, total numeric, note text)");
        exec(&engine, "alter", "INSERT INTO zen_alter.orders VALUES (1, 5, NULL)");
        let snapshot = introspect(&engine, "alter", "postgresql", &["zen_alter"]);
        let mut changes = changes_of_a_table("zen_alter", "numeric(12,2)");
        changes.push(Change::CreateSequence {
            schema: "zen_alter".to_owned(),
            name: "numbers".to_owned(),
            settings: SequenceSpec { start: Some(10), increment: Some(5), ..SequenceSpec::default() },
        });
        let script = alter::script(SqlDialect::Postgres, current(&snapshot), &changes).unwrap();
        ran(&run(&engine, "alter", &script.all(true)));
        let after = introspect(&engine, "alter", "postgresql", &["zen_alter"]);
        check_the_table(current(&after), "zen_alter");
        assert!(object(current(&after), "zen_alter", ObjectKind::Sequence, "numbers").is_some());

        let failing = [
            Change::AddColumn {
                schema: "zen_alter".to_owned(),
                table: "purchases".to_owned(),
                column: spec("later", "text"),
            },
            Change::AlterColumn {
                schema: "zen_alter".to_owned(),
                table: "purchases".to_owned(),
                column: "note".to_owned(),
                patch: ColumnPatch { not_null: Some(true), ..ColumnPatch::default() },
            },
        ];
        let script = alter::script(SqlDialect::Postgres, current(&after), &failing).unwrap();
        let query = run(&engine, "alter", &script.all(true));
        assert!(query.statements.iter().any(|statement| statement.status == StatementStatus::Failed), "{query:?}");
        let rolled_back = introspect(&engine, "alter", "postgresql", &["zen_alter"]);
        let purchases = object(current(&rolled_back), "zen_alter", ObjectKind::Table, "purchases").unwrap();
        assert!(purchases.columns.iter().all(|column| column.name != "later"), "the whole script was rolled back");

        let drop = [Change::DropSchema { name: "zen_alter".to_owned(), cascade: true }];
        let script = alter::script(SqlDialect::Postgres, current(&rolled_back), &drop).unwrap();
        ran(&run(&engine, "alter", &script.all(true)));
        let gone = introspect(&engine, "alter", "postgresql", &[]);
        assert!(current(&gone).schemas.iter().all(|schema| schema.name != "zen_alter"));
        ok(&engine, engine.disconnect("alter", zen_db::engine::TransactionPolicy::Rollback).unwrap());
    }
}

fn mysql_servers() -> Vec<(&'static str, u16)> {
    std::env::var("ZEN_DB_MYSQL_SERVERS")
        .unwrap_or_else(|_| "mysql:33084,mariadb:33114".to_owned())
        .split(',')
        .filter_map(|pair| {
            let (driver, port) = pair.trim().split_once(':')?;
            Some((if driver == "mariadb" { "mariadb" } else { "mysql" }, port.parse().ok()?))
        })
        .collect()
}

#[test]
#[ignore = "needs MySQL and MariaDB in Docker (docs/development.md)"]
fn mysql_and_mariadb_run_the_previewed_script() {
    for (driver, port) in mysql_servers() {
        let engine = engine();
        let source = DataSource {
            id: "alter".to_owned(),
            name: "alter".to_owned(),
            driver: driver.to_owned(),
            host: Some("127.0.0.1".to_owned()),
            port: Some(port),
            user: Some("root".to_owned()),
            ..DataSource::default()
        };
        ok(&engine, engine.connect(request(source, Some("zen"))).unwrap());
        let dialect = if driver == "mariadb" { SqlDialect::MariaDb } else { SqlDialect::MySql };
        exec(&engine, "alter", "DROP DATABASE IF EXISTS zen_alter");
        let snapshot = introspect(&engine, "alter", driver, &[]);
        let create = [Change::CreateSchema { name: "zen_alter".to_owned() }];
        let script = alter::script(dialect, current(&snapshot), &create).unwrap();
        ran(&run(&engine, "alter", &script.all(true)));
        exec(&engine, "alter", "CREATE TABLE zen_alter.customers (code varchar(20) PRIMARY KEY)");
        exec(
            &engine,
            "alter",
            "CREATE TABLE zen_alter.orders (id int PRIMARY KEY, total decimal(10,2), note varchar(20))",
        );
        exec(&engine, "alter", "INSERT INTO zen_alter.orders VALUES (1, 5, NULL)");
        let snapshot = introspect(&engine, "alter", driver, &["zen_alter"]);
        let mut changes = changes_of_a_table("zen_alter", "decimal(12,2)");
        changes.push(Change::AlterColumn {
            schema: "zen_alter".to_owned(),
            table: "purchases".to_owned(),
            column: "note".to_owned(),
            patch: ColumnPatch { default: Some("'none'".to_owned()), ..ColumnPatch::default() },
        });
        if dialect == SqlDialect::MariaDb {
            changes.push(Change::CreateSequence {
                schema: "zen_alter".to_owned(),
                name: "numbers".to_owned(),
                settings: SequenceSpec { start: Some(10), ..SequenceSpec::default() },
            });
        }
        let script = alter::script(dialect, current(&snapshot), &changes).unwrap();
        assert!(!script.transactional);
        ran(&run(&engine, "alter", &script.all(true)));
        let after = introspect(&engine, "alter", driver, &["zen_alter"]);
        check_the_table(current(&after), "zen_alter");
        let purchases = object(current(&after), "zen_alter", ObjectKind::Table, "purchases").unwrap();
        assert_eq!(purchases.columns[2].default.as_deref(), Some("'none'"), "{driver}");
        assert_eq!(purchases.columns[1].type_name, "decimal(12,2)", "{driver}");

        let more = [
            Change::DropConstraint {
                schema: "zen_alter".to_owned(),
                table: "purchases".to_owned(),
                name: "orders_customer_fk".to_owned(),
                cascade: false,
            },
            Change::DropIndex {
                schema: "zen_alter".to_owned(),
                table: "purchases".to_owned(),
                name: "orders_email_key".to_owned(),
            },
            Change::DropColumn {
                schema: "zen_alter".to_owned(),
                table: "purchases".to_owned(),
                column: "email".to_owned(),
                cascade: false,
            },
            Change::Drop {
                schema: "zen_alter".to_owned(),
                kind: ObjectKind::View,
                name: "big".to_owned(),
                cascade: false,
            },
        ];
        let script = alter::script(dialect, current(&after), &more).unwrap();
        ran(&run(&engine, "alter", &script.all(true)));
        let dropped = introspect(&engine, "alter", driver, &["zen_alter"]);
        let purchases = object(current(&dropped), "zen_alter", ObjectKind::Table, "purchases").unwrap();
        assert!(
            purchases.constraints.iter().all(|constraint| constraint.kind != ConstraintKind::ForeignKey),
            "{driver}"
        );
        assert!(purchases.columns.iter().all(|column| column.name != "email"), "{driver}");
        assert!(object(current(&dropped), "zen_alter", ObjectKind::View, "big").is_none());

        let drop = [Change::DropSchema { name: "zen_alter".to_owned(), cascade: true }];
        let script = alter::script(dialect, current(&dropped), &drop).unwrap();
        ran(&run(&engine, "alter", &script.all(true)));
        ok(&engine, engine.disconnect("alter", zen_db::engine::TransactionPolicy::Rollback).unwrap());
    }
}

fn sqlite_installed() -> bool {
    let found = zen_db::sqlite::process::find_program(None).is_ok();
    if !found {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
    }
    found
}

fn sqlite(file: &Path) -> Engine {
    let engine = engine();
    let source = DataSource {
        id: "lite".to_owned(),
        name: "lite".to_owned(),
        driver: zen_db::sqlite::ID.to_owned(),
        database: Some(file.display().to_string()),
        ..DataSource::default()
    };
    ok(&engine, engine.connect(request(source, None)).unwrap());
    engine
}

#[test]
fn sqlite_rebuilds_a_table_keeping_its_rows_indexes_and_triggers() {
    if !sqlite_installed() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("shop.db");
    std::fs::File::create(&file).unwrap();
    let engine = sqlite(&file);
    exec(&engine, "lite", "CREATE TABLE customers (code TEXT PRIMARY KEY)");
    exec(&engine, "lite", "CREATE TABLE orders (id INTEGER PRIMARY KEY, total NUMERIC, note TEXT)");
    exec(&engine, "lite", "CREATE INDEX orders_total ON orders (total)");
    exec(&engine, "lite", "CREATE TABLE log (what TEXT)");
    exec(
        &engine,
        "lite",
        "CREATE TRIGGER orders_log AFTER INSERT ON orders BEGIN INSERT INTO log VALUES ('added'); END",
    );
    exec(&engine, "lite", "CREATE VIEW big AS SELECT id FROM orders WHERE total > 100");
    exec(&engine, "lite", "INSERT INTO orders VALUES (1, 5, 'a'), (2, 500, NULL)");
    let snapshot = introspect(&engine, "lite", zen_db::sqlite::ID, &[]);
    let changes = [
        Change::AddConstraint {
            schema: "main".to_owned(),
            table: "orders".to_owned(),
            constraint: ConstraintSpec {
                name: None,
                kind: ConstraintKind::ForeignKey,
                columns: vec!["note".to_owned()],
                check: None,
                references: Some(ReferenceSpec {
                    table: "customers".to_owned(),
                    columns: vec!["code".to_owned()],
                    ..ReferenceSpec::default()
                }),
            },
        },
        Change::AddColumn {
            schema: "main".to_owned(),
            table: "orders".to_owned(),
            column: ColumnSpec { not_null: true, default: Some("'x'".to_owned()), ..spec("code", "TEXT") },
        },
        Change::AlterColumn {
            schema: "main".to_owned(),
            table: "orders".to_owned(),
            column: "total".to_owned(),
            patch: ColumnPatch {
                name: Some("amount".to_owned()),
                type_name: Some("REAL".to_owned()),
                not_null: Some(true),
                default: Some("0".to_owned()),
                comment: None,
            },
        },
    ];
    let refused = alter::script(SqlDialect::Sqlite, current(&snapshot), &[changes[2].clone(), changes[0].clone()]);
    assert_eq!(refused.unwrap_err().reason, alter::Reason::RunFirst, "the trigger's text was rewritten by the rename");
    let script = alter::script(SqlDialect::Sqlite, current(&snapshot), &changes).unwrap();
    assert!(script.notes.iter().any(|note| matches!(note, alter::Note::Rebuild { .. })));
    ran(&run(&engine, "lite", &script.all(true)));
    let rows = exec(&engine, "lite", "SELECT id, amount, note, code FROM orders ORDER BY id");
    assert_eq!(
        rows.rows[0],
        vec![Value::Integer(1), Value::Float(5.0), Value::Text("a".into()), Value::Text("x".into())]
    );
    assert_eq!(rows.rows[1][2], Value::Null);
    let after = introspect(&engine, "lite", zen_db::sqlite::ID, &[]);
    let orders = object(current(&after), "main", ObjectKind::Table, "orders").unwrap();
    let amount = orders.columns.iter().find(|column| column.name == "amount").unwrap();
    assert_eq!((amount.type_name.as_str(), amount.nullable), ("REAL", false));
    assert!(orders.indexes.iter().any(|index| index.name == "orders_total"), "{:?}", orders.indexes);
    assert!(orders.triggers.iter().any(|trigger| trigger.name == "orders_log"));
    assert!(orders.constraints.iter().any(|constraint| constraint.kind == ConstraintKind::ForeignKey));
    assert!(object(current(&after), "main", ObjectKind::Table, "_zen_new_orders").is_none());
    exec(&engine, "lite", "INSERT INTO orders (id, amount) VALUES (3, 1)");
    assert_eq!(
        exec(&engine, "lite", "SELECT count(*) FROM log").rows[0][0],
        Value::Integer(3),
        "the trigger fires again"
    );
    assert_eq!(
        exec(&engine, "lite", "SELECT count(*) FROM big").rows[0][0],
        Value::Integer(1),
        "the view still reads it"
    );

    exec(&engine, "lite", "UPDATE orders SET note = NULL");
    let failing = [Change::AlterColumn {
        schema: "main".to_owned(),
        table: "orders".to_owned(),
        column: "note".to_owned(),
        patch: ColumnPatch { not_null: Some(true), ..ColumnPatch::default() },
    }];
    let script = alter::script(SqlDialect::Sqlite, current(&after), &failing).unwrap();
    let query = run(&engine, "lite", &script.all(true));
    assert!(query.statements.iter().any(|statement| statement.status == StatementStatus::Failed), "{query:?}");
    let kept = introspect(&engine, "lite", zen_db::sqlite::ID, &[]);
    assert!(object(current(&kept), "main", ObjectKind::Table, "_zen_new_orders").is_none(), "rolled back");
    assert_eq!(exec(&engine, "lite", "SELECT count(*) FROM orders").rows[0][0], Value::Integer(3));
}
