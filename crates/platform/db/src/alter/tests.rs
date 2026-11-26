use super::*;
use crate::schema::{Index, Trigger};

fn column(name: &str, type_name: &str, nullable: bool) -> Column {
    Column { name: name.to_owned(), type_name: type_name.to_owned(), nullable, ..Column::default() }
}

fn catalog(schema: &str, definition: Option<&str>) -> Catalog {
    let mut orders = Object::new(ObjectKind::Table, "orders");
    orders.detailed = true;
    orders.columns =
        vec![column("id", "integer", false), column("total", "numeric", true), column("note", "text", true)];
    orders.constraints.push(Constraint {
        name: "orders_pkey".to_owned(),
        kind: ConstraintKind::PrimaryKey,
        columns: vec!["id".to_owned()],
        definition: "PRIMARY KEY (id)".to_owned(),
        references: None,
        index: None,
        comment: None,
    });
    orders.indexes.push(Index {
        name: "orders_total_idx".to_owned(),
        columns: vec!["total".to_owned()],
        definition: "CREATE INDEX orders_total_idx ON orders (total)".to_owned(),
        ..Index::default()
    });
    orders.definition = definition.map(str::to_owned);
    let mut view = Object::new(ObjectKind::View, "big_orders");
    view.definition = Some("SELECT * FROM orders WHERE total > 100".to_owned());
    Catalog {
        name: "shop".to_owned(),
        schemas: vec![Schema { name: schema.to_owned(), objects: vec![orders, view], ..Schema::default() }],
        ..Catalog::default()
    }
}

fn spec(name: &str, type_name: &str) -> ColumnSpec {
    ColumnSpec { name: name.to_owned(), type_name: type_name.to_owned(), ..ColumnSpec::default() }
}

fn run(dialect: SqlDialect, catalog: &Catalog, changes: &[Change]) -> Vec<String> {
    script(dialect, catalog, changes).expect("script").statements
}

#[test]
fn postgres_writes_one_statement_per_column_change_and_renames_last() {
    let catalog = catalog("public", None);
    let changes = [Change::AlterColumn {
        schema: "public".to_owned(),
        table: "orders".to_owned(),
        column: "total".to_owned(),
        patch: ColumnPatch {
            name: Some("amount".to_owned()),
            type_name: Some("numeric(12,2)".to_owned()),
            not_null: Some(true),
            default: Some("0".to_owned()),
            comment: Some("Sum, it's paid".to_owned()),
        },
    }];
    assert_eq!(
        run(SqlDialect::Postgres, &catalog, &changes),
        [
            "ALTER TABLE \"public\".\"orders\" ALTER COLUMN \"total\" TYPE numeric(12,2)",
            "ALTER TABLE \"public\".\"orders\" ALTER COLUMN \"total\" SET NOT NULL",
            "ALTER TABLE \"public\".\"orders\" ALTER COLUMN \"total\" SET DEFAULT 0",
            "COMMENT ON COLUMN \"public\".\"orders\".\"total\" IS 'Sum, it''s paid'",
            "ALTER TABLE \"public\".\"orders\" RENAME COLUMN \"total\" TO \"amount\"",
        ]
    );
}

#[test]
fn edits_accumulate_and_later_ones_see_earlier_ones() {
    let catalog = catalog("public", None);
    let changes = [
        Change::AddColumn {
            schema: "public".to_owned(),
            table: "orders".to_owned(),
            column: ColumnSpec { not_null: true, default: Some("now()".to_owned()), ..spec("created", "timestamptz") },
        },
        Change::CreateIndex {
            schema: "public".to_owned(),
            table: "orders".to_owned(),
            index: IndexSpec { columns: vec!["created".to_owned()], ..IndexSpec::default() },
        },
        Change::Rename {
            schema: "public".to_owned(),
            kind: ObjectKind::Table,
            name: "orders".to_owned(),
            to: "Orders 2".to_owned(),
        },
        Change::DropColumn {
            schema: "public".to_owned(),
            table: "Orders 2".to_owned(),
            column: "note".to_owned(),
            cascade: true,
        },
    ];
    assert_eq!(
        run(SqlDialect::Postgres, &catalog, &changes),
        [
            "ALTER TABLE \"public\".\"orders\" ADD COLUMN \"created\" timestamptz NOT NULL DEFAULT now()",
            "CREATE INDEX \"orders_created_idx\" ON \"public\".\"orders\" (\"created\")",
            "ALTER TABLE \"public\".\"orders\" RENAME TO \"Orders 2\"",
            "ALTER TABLE \"public\".\"Orders 2\" DROP COLUMN \"note\" CASCADE",
        ]
    );
    let unknown = script(
        SqlDialect::Postgres,
        &catalog,
        &[
            changes[2].clone(),
            Change::DropColumn {
                schema: "public".to_owned(),
                table: "orders".to_owned(),
                column: "note".to_owned(),
                cascade: false,
            },
        ],
    );
    assert_eq!(
        unknown.unwrap_err(),
        Refusal { change: 1, reason: Reason::UnknownObject, subject: "orders".to_owned() }
    );
}

#[test]
fn tables_are_created_in_each_dialect() {
    let change = |schema: &str| Change::CreateTable {
        schema: schema.to_owned(),
        name: "customers".to_owned(),
        columns: vec![
            ColumnSpec { identity: true, not_null: true, ..spec("id", "integer") },
            ColumnSpec { not_null: true, comment: Some("Shown name".to_owned()), ..spec("name", "varchar(80)") },
        ],
        primary_key: vec!["id".to_owned()],
        comment: None,
    };
    assert_eq!(
        run(SqlDialect::Postgres, &catalog("public", None), &[change("public")]),
        [
            "CREATE TABLE \"public\".\"customers\" (\n    \"id\" integer GENERATED BY DEFAULT AS IDENTITY NOT NULL,\n    \"name\" varchar(80) NOT NULL,\n    PRIMARY KEY (\"id\")\n)",
            "COMMENT ON COLUMN \"public\".\"customers\".\"name\" IS 'Shown name'",
        ]
    );
    assert_eq!(
        run(SqlDialect::MySql, &catalog("shop", None), &[change("shop")]),
        [
            "CREATE TABLE `shop`.`customers` (\n    `id` integer NOT NULL AUTO_INCREMENT,\n    `name` varchar(80) NOT NULL COMMENT 'Shown name',\n    PRIMARY KEY (`id`)\n)",
        ]
    );
    assert_eq!(
        script(SqlDialect::Sqlite, &catalog("main", None), &[change("main")]).unwrap_err().reason,
        Reason::Comments,
        "SQLite keeps no comments"
    );
    let plain = Change::CreateTable {
        schema: "main".to_owned(),
        name: "customers".to_owned(),
        columns: vec![ColumnSpec { identity: true, ..spec("id", "INTEGER") }, spec("name", "TEXT")],
        primary_key: Vec::new(),
        comment: None,
    };
    assert_eq!(
        run(SqlDialect::Sqlite, &catalog("main", None), &[plain]),
        ["CREATE TABLE \"main\".\"customers\" (\n    \"id\" INTEGER PRIMARY KEY AUTOINCREMENT,\n    \"name\" TEXT\n)"]
    );
}

#[test]
fn mysql_restates_a_column_and_keeps_renames_and_defaults_simple() {
    let definition = "CREATE TABLE `orders` (\n  `id` int NOT NULL,\n  `total` decimal(10,2) DEFAULT NULL,\n  `note` text,\n  `changed` timestamp NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP\n)";
    let mut catalog = catalog("shop", Some(definition));
    catalog.schemas[0].objects[0].columns.push(column("changed", "timestamp", true));
    let alter = |column: &str, patch: ColumnPatch| Change::AlterColumn {
        schema: "shop".to_owned(),
        table: "orders".to_owned(),
        column: column.to_owned(),
        patch,
    };
    assert_eq!(
        run(
            SqlDialect::MySql,
            &catalog,
            &[alter("total", ColumnPatch { name: Some("amount".to_owned()), ..ColumnPatch::default() })]
        ),
        ["ALTER TABLE `shop`.`orders` RENAME COLUMN `total` TO `amount`"]
    );
    assert_eq!(
        run(
            SqlDialect::MariaDb,
            &catalog,
            &[alter("total", ColumnPatch { default: Some("0".to_owned()), ..ColumnPatch::default() })]
        ),
        ["ALTER TABLE `shop`.`orders` ALTER COLUMN `total` SET DEFAULT 0"]
    );
    assert_eq!(
        run(
            SqlDialect::MySql,
            &catalog,
            &[alter(
                "total",
                ColumnPatch { not_null: Some(true), comment: Some("Paid".to_owned()), ..ColumnPatch::default() }
            )]
        ),
        ["ALTER TABLE `shop`.`orders` CHANGE COLUMN `total` `total` numeric NOT NULL COMMENT 'Paid'"]
    );
    let lost = script(
        SqlDialect::MySql,
        &catalog,
        &[alter("changed", ColumnPatch { not_null: Some(true), ..ColumnPatch::default() })],
    );
    assert_eq!(lost.unwrap_err(), Refusal { change: 0, reason: Reason::Restate, subject: "ON UPDATE".to_owned() });
    let mut doubled = column("doubled", "decimal(10,2)", true);
    doubled.generated = Some("(`total` * 2)".to_owned());
    catalog.schemas[0].objects[0].columns.push(doubled);
    let comment = ColumnPatch { comment: Some("Twice".to_owned()), ..ColumnPatch::default() };
    let restated = script(SqlDialect::MySql, &catalog, &[alter("doubled", comment)]);
    assert_eq!(
        restated.unwrap_err(),
        Refusal { change: 0, reason: Reason::Generated, subject: "doubled".to_owned() },
        "CHANGE COLUMN would drop GENERATED ALWAYS AS"
    );
    assert_eq!(
        run(
            SqlDialect::MySql,
            &catalog,
            &[alter("doubled", ColumnPatch { name: Some("twice".to_owned()), ..ColumnPatch::default() })]
        ),
        ["ALTER TABLE `shop`.`orders` RENAME COLUMN `doubled` TO `twice`"]
    );
}

#[test]
fn constraints_and_indexes_are_dropped_as_each_server_names_them() {
    let mut catalog = catalog("shop", None);
    let orders = &mut catalog.schemas[0].objects[0];
    for (name, kind) in
        [("orders_customer_fk", ConstraintKind::ForeignKey), ("orders_total_check", ConstraintKind::Check)]
    {
        orders.constraints.push(Constraint {
            name: name.to_owned(),
            kind,
            columns: Vec::new(),
            definition: String::new(),
            references: None,
            index: None,
            comment: None,
        });
    }
    let drop = |name: &str| Change::DropConstraint {
        schema: "shop".to_owned(),
        table: "orders".to_owned(),
        name: name.to_owned(),
        cascade: false,
    };
    let index = Change::DropIndex {
        schema: "shop".to_owned(),
        table: "orders".to_owned(),
        name: "orders_total_idx".to_owned(),
    };
    let changes = [drop("orders_customer_fk"), drop("orders_total_check"), drop("orders_pkey"), index.clone()];
    assert_eq!(
        run(SqlDialect::MySql, &catalog, &changes),
        [
            "ALTER TABLE `shop`.`orders` DROP FOREIGN KEY `orders_customer_fk`",
            "ALTER TABLE `shop`.`orders` DROP CHECK `orders_total_check`",
            "ALTER TABLE `shop`.`orders` DROP PRIMARY KEY",
            "DROP INDEX `orders_total_idx` ON `shop`.`orders`",
        ]
    );
    assert_eq!(
        run(SqlDialect::MariaDb, &catalog, &changes)[1],
        "ALTER TABLE `shop`.`orders` DROP CONSTRAINT `orders_total_check`"
    );
    assert_eq!(script(SqlDialect::MySql, &catalog, &changes).unwrap().notes, [Note::ImplicitCommit]);
    let mut postgres = catalog.clone();
    postgres.schemas[0].name = "public".to_owned();
    let cascade = Change::DropConstraint {
        schema: "public".to_owned(),
        table: "orders".to_owned(),
        name: "orders_pkey".to_owned(),
        cascade: true,
    };
    assert_eq!(
        run(SqlDialect::Postgres, &postgres, &[cascade]),
        ["ALTER TABLE \"public\".\"orders\" DROP CONSTRAINT \"orders_pkey\" CASCADE"]
    );
    let refused = script(
        SqlDialect::MySql,
        &catalog,
        &[Change::DropConstraint {
            schema: "shop".to_owned(),
            table: "orders".to_owned(),
            name: "orders_pkey".to_owned(),
            cascade: true,
        }],
    );
    assert_eq!(refused.unwrap_err().reason, Reason::Cascade);
}

#[test]
fn foreign_keys_checks_and_their_actions_are_checked() {
    let catalog = catalog("public", None);
    let add = |constraint: ConstraintSpec| Change::AddConstraint {
        schema: "public".to_owned(),
        table: "orders".to_owned(),
        constraint,
    };
    let foreign = ConstraintSpec {
        name: Some("orders_customer_fk".to_owned()),
        kind: ConstraintKind::ForeignKey,
        columns: vec!["note".to_owned()],
        check: None,
        references: Some(ReferenceSpec {
            schema: Some("crm".to_owned()),
            table: "customers".to_owned(),
            columns: vec!["code".to_owned()],
            on_delete: Some("SET_NULL".to_owned()),
            on_update: Some("cascade".to_owned()),
        }),
    };
    let check = ConstraintSpec {
        name: None,
        kind: ConstraintKind::Check,
        columns: Vec::new(),
        check: Some("total >= 0".to_owned()),
        references: None,
    };
    assert_eq!(
        run(SqlDialect::Postgres, &catalog, &[add(foreign.clone()), add(check)]),
        [
            "ALTER TABLE \"public\".\"orders\" ADD CONSTRAINT \"orders_customer_fk\" FOREIGN KEY (\"note\") REFERENCES \"crm\".\"customers\" (\"code\") ON DELETE SET NULL ON UPDATE CASCADE",
            "ALTER TABLE \"public\".\"orders\" ADD CHECK (total >= 0)",
        ]
    );
    let mut bad = foreign;
    bad.references.as_mut().unwrap().on_delete = Some("DROP TABLE x".to_owned());
    assert_eq!(script(SqlDialect::Postgres, &catalog, &[add(bad)]).unwrap_err().reason, Reason::Action);
    let injected = ConstraintSpec {
        name: None,
        kind: ConstraintKind::Check,
        columns: Vec::new(),
        check: Some("true); DROP TABLE orders; --".to_owned()),
        references: None,
    };
    assert_eq!(script(SqlDialect::Postgres, &catalog, &[add(injected)]).unwrap_err().reason, Reason::NotOneStatement);
}

#[test]
fn sqlite_alters_in_place_where_it_can_and_rebuilds_where_it_cannot() {
    let definition = "CREATE TABLE orders (id integer PRIMARY KEY, total numeric, note text)";
    let mut catalog = catalog("main", Some(definition));
    catalog.schemas[0].objects[0].triggers.push(Trigger {
        name: "orders_touch".to_owned(),
        definition: "CREATE TRIGGER orders_touch AFTER UPDATE ON orders BEGIN SELECT 1; END".to_owned(),
        enabled: true,
        function: String::new(),
    });
    let rename = Change::AlterColumn {
        schema: "main".to_owned(),
        table: "orders".to_owned(),
        column: "note".to_owned(),
        patch: ColumnPatch { name: Some("remark".to_owned()), ..ColumnPatch::default() },
    };
    assert_eq!(
        run(SqlDialect::Sqlite, &catalog, &[rename]),
        ["ALTER TABLE \"main\".\"orders\" RENAME COLUMN \"note\" TO \"remark\""]
    );
    let retype = Change::AlterColumn {
        schema: "main".to_owned(),
        table: "orders".to_owned(),
        column: "total".to_owned(),
        patch: ColumnPatch { type_name: Some("real".to_owned()), not_null: Some(true), ..ColumnPatch::default() },
    };
    let rebuilt = script(SqlDialect::Sqlite, &catalog, &[retype]).unwrap();
    assert_eq!(rebuilt.before, ["PRAGMA foreign_keys = OFF"]);
    assert_eq!(
        rebuilt.statements,
        [
            "PRAGMA legacy_alter_table = ON",
            "CREATE TABLE \"main\".\"_zen_new_orders\" (\n    \"id\" integer,\n    \"total\" real NOT NULL,\n    \"note\" text,\n    PRIMARY KEY (\"id\")\n)",
            "INSERT INTO \"main\".\"_zen_new_orders\" (\"id\", \"total\", \"note\")\nSELECT \"id\", \"total\", \"note\" FROM \"main\".\"orders\"",
            "DROP TABLE \"main\".\"orders\"",
            "ALTER TABLE \"main\".\"_zen_new_orders\" RENAME TO \"orders\"",
            "CREATE INDEX orders_total_idx ON orders (total)",
            "CREATE TRIGGER orders_touch AFTER UPDATE ON orders BEGIN SELECT 1; END",
            "PRAGMA legacy_alter_table = OFF",
        ]
    );
    assert_eq!(rebuilt.notes, [Note::Rebuild { table: "orders".to_owned() }]);
    assert_eq!(
        rebuilt.all(true)[..3],
        ["PRAGMA foreign_keys = OFF".to_owned(), "BEGIN".to_owned(), "PRAGMA legacy_alter_table = ON".to_owned()]
    );
}

#[test]
fn sqlite_refuses_a_rebuild_that_would_lose_part_of_the_table() {
    let checked = catalog(
        "main",
        Some("CREATE TABLE orders (id integer PRIMARY KEY, total numeric CHECK (total > 0), note text)"),
    );
    let drop_key = Change::DropConstraint {
        schema: "main".to_owned(),
        table: "orders".to_owned(),
        name: "orders_pkey".to_owned(),
        cascade: false,
    };
    assert_eq!(
        script(SqlDialect::Sqlite, &checked, std::slice::from_ref(&drop_key)).unwrap_err(),
        Refusal { change: 0, reason: Reason::RebuildLoses, subject: "CHECK".to_owned() }
    );
    let quoted = catalog(
        "main",
        Some("CREATE TABLE orders (id integer PRIMARY KEY, \"check\" text, note text DEFAULT 'STRICT')"),
    );
    assert!(script(SqlDialect::Sqlite, &quoted, &[drop_key]).is_ok(), "names and strings are not words");
}

#[test]
fn what_a_server_lacks_is_refused_cleanly() {
    let refused = |dialect: SqlDialect, schema: &str, change: Change| {
        script(dialect, &catalog(schema, None), &[change]).unwrap_err().reason
    };
    assert_eq!(refused(SqlDialect::Sqlite, "main", Change::CreateSchema { name: "aux".to_owned() }), Reason::Schemas);
    assert_eq!(
        refused(SqlDialect::MySql, "shop", Change::RenameSchema { name: "shop".to_owned(), to: "store".to_owned() }),
        Reason::RenameSchema
    );
    assert_eq!(
        refused(
            SqlDialect::MySql,
            "shop",
            Change::CreateSequence {
                schema: "shop".to_owned(),
                name: "numbers".to_owned(),
                settings: SequenceSpec::default()
            }
        ),
        Reason::Sequences
    );
    assert_eq!(
        refused(
            SqlDialect::Sqlite,
            "main",
            Change::Rename {
                schema: "main".to_owned(),
                kind: ObjectKind::View,
                name: "big_orders".to_owned(),
                to: "large".to_owned()
            }
        ),
        Reason::Rename
    );
    assert_eq!(
        refused(SqlDialect::MySql, "shop", Change::DropSchema { name: "shop".to_owned(), cascade: false }),
        Reason::NotEmpty
    );
    assert_eq!(
        refused(SqlDialect::ClickHouse, "default", Change::CreateSchema { name: "x".to_owned() }),
        Reason::Driver
    );
    assert_eq!(
        refused(
            SqlDialect::Postgres,
            "public",
            Change::CreateRoutine {
                schema: "public".to_owned(),
                name: "f".to_owned(),
                definition: "DROP TABLE orders".to_owned()
            }
        ),
        Reason::NotOneStatement
    );
}

#[test]
fn schemas_views_sequences_and_routines_are_created_and_dropped() {
    let catalog = catalog("public", None);
    let template = routine_template(SqlDialect::Postgres, "public", "answer", false);
    let changes = [
        Change::CreateSchema { name: "Reports".to_owned() },
        Change::CreateView {
            schema: "Reports".to_owned(),
            name: "daily".to_owned(),
            query: "SELECT 1 AS one;".to_owned(),
        },
        Change::CreateSequence {
            schema: "public".to_owned(),
            name: "numbers".to_owned(),
            settings: SequenceSpec { start: Some(10), increment: Some(5), cycle: true, ..SequenceSpec::default() },
        },
        Change::CreateRoutine { schema: "public".to_owned(), name: "answer".to_owned(), definition: template.clone() },
        Change::Comment {
            schema: "public".to_owned(),
            kind: ObjectKind::Table,
            name: "orders".to_owned(),
            comment: String::new(),
        },
        Change::Drop {
            schema: "public".to_owned(),
            kind: ObjectKind::View,
            name: "big_orders".to_owned(),
            cascade: true,
        },
        Change::DropSchema { name: "Reports".to_owned(), cascade: true },
    ];
    assert_eq!(
        run(SqlDialect::Postgres, &catalog, &changes),
        [
            "CREATE SCHEMA \"Reports\"".to_owned(),
            "CREATE VIEW \"Reports\".\"daily\" AS\nSELECT 1 AS one".to_owned(),
            "CREATE SEQUENCE \"public\".\"numbers\" INCREMENT BY 5 START WITH 10 CYCLE".to_owned(),
            template,
            "COMMENT ON TABLE \"public\".\"orders\" IS NULL".to_owned(),
            "DROP VIEW \"public\".\"big_orders\" CASCADE".to_owned(),
            "DROP SCHEMA \"Reports\" CASCADE".to_owned(),
        ]
    );
    assert_eq!(
        text(&["CREATE SCHEMA a".to_owned(), "DROP SCHEMA a".to_owned()]),
        "CREATE SCHEMA a;\n\nDROP SCHEMA a;\n"
    );
}

#[test]
fn routines_keep_their_arguments_on_postgres() {
    let mut catalog = catalog("public", None);
    let mut routine = Object::new(ObjectKind::Function, "total");
    routine.routine =
        Some(crate::schema::Routine { arguments: "integer".to_owned(), ..crate::schema::Routine::default() });
    catalog.schemas[0].objects.push(routine);
    let changes = [
        Change::Rename {
            schema: "public".to_owned(),
            kind: ObjectKind::Function,
            name: "total(integer)".to_owned(),
            to: "sum".to_owned(),
        },
        Change::Drop {
            schema: "public".to_owned(),
            kind: ObjectKind::Function,
            name: "sum(integer)".to_owned(),
            cascade: false,
        },
    ];
    assert_eq!(
        run(SqlDialect::Postgres, &catalog, &changes),
        ["ALTER FUNCTION \"public\".\"total\"(integer) RENAME TO \"sum\"", "DROP FUNCTION \"public\".\"sum\"(integer)"]
    );
}

#[test]
fn changes_serialise_with_their_action() {
    let change: Change = serde_json::from_value(serde_json::json!({
        "action": "addColumn", "schema": "public", "table": "orders",
        "column": { "name": "code", "type": "text", "notNull": true }
    }))
    .unwrap();
    assert_eq!(
        change,
        Change::AddColumn {
            schema: "public".to_owned(),
            table: "orders".to_owned(),
            column: ColumnSpec { not_null: true, ..spec("code", "text") }
        }
    );
    assert_eq!(change.inside(), Some((ObjectKind::Table, "orders")));
}
