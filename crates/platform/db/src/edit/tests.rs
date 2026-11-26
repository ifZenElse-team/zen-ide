use super::*;
use crate::schema::{Column as TableColumn, Constraint, ObjectKind};
use crate::value::ColumnSource;

const PG: SqlDialect = SqlDialect::Postgres;

const OID: u32 = 16384;

fn table() -> Object {
    let mut object = Object::new(ObjectKind::Table, "orders");
    object.oid = OID;
    let column = |position: i32, name: &str, type_name: &str, nullable: bool| TableColumn {
        name: name.into(),
        position,
        type_name: type_name.into(),
        nullable,
        ..TableColumn::default()
    };
    object.columns = vec![
        TableColumn { identity: Some("byDefault".into()), ..column(1, "id", "bigint", false) },
        column(2, "Note", "text", true),
        column(3, "payload", "jsonb", true),
        TableColumn { generated: Some("id * 2".into()), ..column(4, "twice", "bigint", true) },
        column(5, "code", "text", false),
    ];
    object
}

fn key(kind: ConstraintKind, columns: &[&str]) -> Constraint {
    Constraint {
        name: "k".into(),
        kind,
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        definition: String::new(),
        references: None,
        index: None,
        comment: None,
    }
}

fn result_columns(positions: &[(&str, &str, Option<i16>)]) -> Vec<Column> {
    positions
        .iter()
        .map(|(name, type_name, position)| Column {
            name: (*name).to_owned(),
            type_name: (*type_name).to_owned(),
            source: position.map(|position| ColumnSource { table: OID, position }),
            origin: None,
        })
        .collect()
}

fn all_columns() -> Vec<Column> {
    result_columns(&[
        ("id", "int8", Some(1)),
        ("Note", "text", Some(2)),
        ("payload", "jsonb", Some(3)),
        ("twice", "int8", Some(4)),
        ("code", "text", Some(5)),
        ("upper", "text", None),
    ])
}

#[test]
fn rows_are_identified_by_the_primary_key_then_a_unique_key_then_the_persons_choice() {
    let mut object = table();
    object.constraints = vec![key(ConstraintKind::Unique, &["code"]), key(ConstraintKind::PrimaryKey, &["id"])];
    let editing = identify(&all_columns(), "public", &object, None, PG);
    assert!(editing.editable);
    assert_eq!(editing.table.as_deref(), Some("\"public\".\"orders\""));
    let found = editing.key.clone().unwrap();
    assert_eq!((found.kind, found.columns, found.names), (KeyKind::PrimaryKey, vec![0], vec!["id".to_owned()]));
    let editable: Vec<bool> = editing.columns.iter().map(|column| column.editable).collect();
    assert_eq!(editable, [true, true, true, false, true, false], "generated and computed columns are read-only");

    let without_id = result_columns(&[("Note", "text", Some(2)), ("code", "text", Some(5))]);
    let unique = identify(&without_id, "public", &object, None, PG);
    assert_eq!(unique.key.unwrap().kind, KeyKind::UniqueKey, "the key must be in the result");

    object.constraints.clear();
    let none = identify(&without_id, "public", &object, None, PG);
    assert_eq!((none.editable, none.reason), (false, Some(ReadOnly::NoKey)));
    let chosen = identify(&without_id, "public", &object, Some(&KeyChoice::Columns(vec!["code".into()])), PG);
    assert_eq!(chosen.key.unwrap().kind, KeyKind::Virtual);
    let all = identify(&without_id, "public", &object, Some(&KeyChoice::AllColumns), PG);
    assert_eq!(all.key.unwrap().columns, [0, 1]);
    let computed = result_columns(&[("one", "int4", None)]);
    assert_eq!(identify(&computed, "public", &object, None, PG).reason, Some(ReadOnly::NoTable));
    let mut joined = all_columns();
    joined[1].source = Some(ColumnSource { table: OID + 1, position: 1 });
    assert_eq!(source_table(&joined), Err(ReadOnly::SeveralTables));
    assert_eq!(source_table(&computed), Err(ReadOnly::NoTable));
    assert_eq!(source_table(&all_columns()), Ok(OID));
}

#[test]
fn typed_input_is_checked_by_the_columns_type() {
    assert_eq!(parse_input(" 42 ", "bigint"), Ok(Value::Integer(42)));
    assert_eq!(parse_input("4x", "integer"), Err(InputError::Integer));
    assert_eq!(parse_input("10.50", "numeric(10,2)"), Ok(Value::Decimal("10.50".into())));
    assert_eq!(parse_input("yes", "boolean"), Ok(Value::Bool(true)));
    assert_eq!(parse_input("maybe", "bool"), Err(InputError::Boolean));
    assert_eq!(parse_input("{\"a\": 1}", "jsonb"), Ok(Value::Json("{\"a\": 1}".into())));
    assert_eq!(parse_input("{a: 1}", "json"), Err(InputError::Json));
    assert_eq!(parse_input("\\x0aff", "bytea"), Ok(Value::Bytes(vec![10, 255])));
    assert_eq!(parse_input("0aff", "bytea"), Err(InputError::Bytes));
    assert_eq!(
        parse_input("2026-10-27 10:00", "timestamp with time zone"),
        Ok(Value::Temporal(Typed { text: "2026-10-27 10:00".into(), type_name: "timestamp with time zone".into() }))
    );
    assert_eq!(parse_input("not-a-uuid", "uuid"), Err(InputError::Uuid));
    assert!(parse_input("0e9b1c2a-1111-4c4c-8888-000000000001", "uuid").is_ok());
    assert_eq!(parse_input(" padded ", "character varying(40)"), Ok(Value::Text(" padded ".into())));
}

#[test]
fn saving_deletes_then_inserts_then_updates_with_bound_parameters() {
    let mut object = table();
    object.constraints = vec![key(ConstraintKind::PrimaryKey, &["id"])];
    let editing = identify(&all_columns(), "public", &object, None, PG);
    let original = vec![
        Value::Integer(7),
        Value::Text("old".into()),
        Value::Json("{\"a\": 1}".into()),
        Value::Integer(14),
        Value::Text("c7".into()),
        Value::Text("OLD".into()),
    ];
    let changes = Changes {
        updated: vec![Updated {
            row: 0,
            original: original.clone(),
            cells: vec![(1, Staged::Value(Value::Text("it's new".into()))), (2, Staged::Value(Value::Null))],
        }],
        inserted: vec![Inserted {
            row: 3,
            cells: vec![(1, Staged::Value(Value::Text("n".into()))), (4, Staged::Default)],
        }],
        deleted: vec![Deleted { row: 1, original: vec![Value::Integer(8)] }],
    };
    let planned = plan(&editing, &changes);
    let kinds: Vec<ChangeKind> = planned.iter().map(|statement| statement.kind).collect();
    assert_eq!(kinds, [ChangeKind::Delete, ChangeKind::Insert, ChangeKind::Update]);
    assert_eq!(planned[0].sql, "DELETE FROM \"public\".\"orders\" WHERE \"id\" = $1");
    assert_eq!(planned[0].params, [Some("8".to_owned())]);
    assert_eq!(
        planned[1].sql,
        "INSERT INTO \"public\".\"orders\" (\"Note\", \"code\") VALUES ($1, DEFAULT) RETURNING \"id\", \"Note\", \"payload\", \"twice\", \"code\""
    );
    assert_eq!(planned[1].returning, [0, 1, 2, 3, 4]);
    assert_eq!(
        planned[2].sql,
        "UPDATE \"public\".\"orders\" SET \"Note\" = $1, \"payload\" = $2 WHERE \"id\" = $3 AND \"Note\" IS NOT DISTINCT FROM $4 AND \"payload\"::text = $5 RETURNING \"id\", \"Note\", \"payload\", \"twice\", \"code\""
    );
    assert_eq!(
        planned[2].params,
        [
            Some("it's new".to_owned()),
            None,
            Some("7".to_owned()),
            Some("old".to_owned()),
            Some("{\"a\": 1}".to_owned())
        ]
    );
    assert_eq!(
        planned[2].preview,
        "UPDATE \"public\".\"orders\" SET \"Note\" = 'it''s new', \"payload\" = NULL WHERE \"id\" = 7 AND \"Note\" IS NOT DISTINCT FROM 'old' AND \"payload\"::text = '{\"a\": 1}' RETURNING \"id\", \"Note\", \"payload\", \"twice\", \"code\""
    );
    assert!(planned.iter().all(|statement| statement.one_row));
    assert!(preview(&planned).starts_with("DELETE FROM \"public\".\"orders\" WHERE \"id\" = 8;\nINSERT INTO"));
}

#[test]
fn a_key_of_every_column_compares_nulls_and_must_change_one_row() {
    let object = table();
    let columns = result_columns(&[("Note", "text", Some(2)), ("code", "text", Some(5))]);
    let editing = identify(&columns, "s", &object, Some(&KeyChoice::AllColumns), PG);
    let changes = Changes {
        deleted: vec![Deleted { row: 0, original: vec![Value::Null, Value::Text("c".into())] }],
        ..Changes::default()
    };
    let planned = plan(&editing, &changes);
    assert_eq!(
        planned[0].sql,
        "DELETE FROM \"s\".\"orders\" WHERE \"Note\" IS NULL AND \"code\" IS NOT DISTINCT FROM $1"
    );
    let mysql = identify(&columns, "s", &object, Some(&KeyChoice::AllColumns), SqlDialect::MySql);
    assert_eq!(plan(&mysql, &changes)[0].sql, "DELETE FROM `s`.`orders` WHERE `Note` IS NULL AND `code` <=> ?");
    let sqlite = identify(&columns, "s", &object, Some(&KeyChoice::AllColumns), SqlDialect::Sqlite);
    assert_eq!(
        plan(&sqlite, &changes)[0].sql,
        "DELETE FROM \"s\".\"orders\" WHERE \"Note\" IS NULL AND \"code\" IS 'c'"
    );
    assert!(planned[0].one_row, "equal rows are never changed together: several rows roll the save back");
}

#[test]
fn literals_and_parameters_keep_every_value() {
    assert_eq!(literal(&Value::Bytes(vec![0, 255])), "'\\x00ff'");
    assert_eq!(literal(&Value::Float(f64::INFINITY)), "'Infinity'");
    assert_eq!(literal(&Value::Decimal("1e400".into())), "'1e400'");
    assert_eq!(param_text(&Value::Float(0.1)), Some("0.1".to_owned()));
    assert_eq!(param_text(&Value::Bool(false)), Some("f".to_owned()));
    assert_eq!(
        param_text(&Value::Array(vec![Value::Text("a b".into()), Value::Null])),
        Some("{\"a b\",NULL}".to_owned())
    );
}

#[test]
fn sql_is_generated_from_rows() {
    let mut object = table();
    object.constraints = vec![key(ConstraintKind::PrimaryKey, &["id"])];
    let columns = result_columns(&[("id", "int8", Some(1)), ("Note", "text", Some(2))]);
    let editing = identify(&columns, "public", &object, None, PG);
    let rows = vec![vec![Value::Integer(1), Value::Text("a".into())], vec![Value::Integer(2), Value::Null]];
    assert_eq!(
        generate(&editing, Generate::Insert, &rows).unwrap(),
        "INSERT INTO \"public\".\"orders\" (\"id\", \"Note\") VALUES (1, 'a');\nINSERT INTO \"public\".\"orders\" (\"id\", \"Note\") VALUES (2, NULL);"
    );
    assert_eq!(
        generate(&editing, Generate::Update, &rows[..1]).unwrap(),
        "UPDATE \"public\".\"orders\" SET \"Note\" = 'a' WHERE \"id\" = 1;"
    );
    assert_eq!(
        generate(&editing, Generate::Delete, &rows[1..]).unwrap(),
        "DELETE FROM \"public\".\"orders\" WHERE \"id\" = 2;"
    );
    assert_eq!(
        generate(&editing, Generate::Select, &rows[..1]).unwrap(),
        "SELECT \"id\", \"Note\" FROM \"public\".\"orders\" WHERE \"id\" = 1;"
    );
}

fn named_columns(names: &[(&str, &str)]) -> Vec<Column> {
    names
        .iter()
        .map(|(name, type_name)| Column {
            name: (*name).to_owned(),
            type_name: (*type_name).to_owned(),
            source: Some(ColumnSource { table: OID, position: 0 }),
            origin: Some((*name).to_owned()),
        })
        .collect()
}

fn keyed_table() -> Object {
    let mut object = table();
    object.constraints = vec![key(ConstraintKind::PrimaryKey, &["id"])];
    object.columns.push(TableColumn {
        name: "flag".into(),
        position: 6,
        type_name: "tinyint(1)".into(),
        nullable: true,
        default: Some("0".into()),
        ..TableColumn::default()
    });
    object.columns.push(TableColumn {
        name: "data".into(),
        position: 7,
        type_name: "blob".into(),
        nullable: true,
        ..TableColumn::default()
    });
    object
}

fn mysql_changes() -> Changes {
    let original = vec![
        Value::Integer(7),
        Value::Text("a\\b".into()),
        Value::Float(1.5),
        Value::Bool(false),
        Value::Bytes(vec![1]),
    ];
    Changes {
        updated: vec![Updated {
            row: 0,
            original,
            cells: vec![
                (1, Staged::Value(Value::Text("it's".into()))),
                (2, Staged::Value(Value::Float(2.5))),
                (3, Staged::Value(Value::Bool(true))),
                (4, Staged::Value(Value::Bytes(vec![0xff]))),
            ],
        }],
        inserted: vec![Inserted {
            row: 1,
            cells: vec![(1, Staged::Value(Value::Text("n".into()))), (3, Staged::Default)],
        }],
        deleted: Vec::new(),
    }
}

#[test]
fn columns_named_by_the_server_are_found_by_name_and_ctid_is_postgresql_only() {
    let object = keyed_table();
    let mut columns = named_columns(&[("id", "int"), ("Note", "varchar"), ("flag", "tinyint")]);
    columns.push(Column { name: "shout".into(), type_name: "varchar".into(), source: None, origin: None });
    let editing = identify(&columns, "shop", &object, None, SqlDialect::MySql);
    assert!(editing.editable, "{editing:?}");
    assert_eq!(editing.table.as_deref(), Some("`shop`.`orders`"));
    assert_eq!(editing.key.unwrap().columns, [0]);
    let sourced: Vec<bool> = editing.columns.iter().map(|column| column.sourced).collect();
    assert_eq!(sourced, [true, true, true, false]);

    let mut unkeyed = table();
    unkeyed.constraints.clear();
    let mut with_ctid = named_columns(&[("Note", "text")]);
    with_ctid.push(Column::new("ctid", "tid"));
    assert_eq!(identify(&with_ctid, "main", &unkeyed, None, SqlDialect::Sqlite).reason, Some(ReadOnly::NoKey));
}

#[test]
fn mysql_saves_bind_question_marks_inline_booleans_and_read_rows_back() {
    let object = keyed_table();
    let columns = named_columns(&[
        ("id", "int"),
        ("Note", "varchar"),
        ("twice", "double"),
        ("flag", "tinyint"),
        ("data", "blob"),
    ]);
    let mut editing = identify(&columns, "shop", &object, None, SqlDialect::MariaDb);
    editing.columns[2].editable = true;
    let planned = plan(&editing, &mysql_changes());
    let insert = &planned[0];
    assert_eq!(insert.sql, "INSERT INTO `shop`.`orders` (`Note`, `flag`) VALUES (?, DEFAULT)");
    let fetch = insert.fetch.as_ref().unwrap();
    assert_eq!(
        fetch.sql, "SELECT `id`, `Note`, `twice`, `flag`, `data` FROM `shop`.`orders` WHERE `id` = LAST_INSERT_ID()",
        "an auto-increment key is read back by LAST_INSERT_ID()"
    );
    assert_eq!(insert.returning, [0, 1, 2, 3, 4]);
    let update = &planned[1];
    assert_eq!(
        update.sql,
        "UPDATE `shop`.`orders` SET `Note` = ?, `twice` = ?, `flag` = TRUE, `data` = UNHEX(?) WHERE `id` = ? AND `Note` <=> ? AND `flag` <=> FALSE AND `data` <=> UNHEX(?)"
    );
    assert_eq!(
        update.params,
        [
            Some("it's".to_owned()),
            Some("2.5".to_owned()),
            Some("ff".to_owned()),
            Some("7".to_owned()),
            Some("a\\b".to_owned()),
            Some("01".to_owned())
        ]
    );
    assert_eq!(
        update.preview,
        "UPDATE `shop`.`orders` SET `Note` = 'it''s', `twice` = 2.5, `flag` = TRUE, `data` = X'ff' WHERE `id` = 7 AND `Note` <=> _utf8mb4 X'615c62' AND `flag` <=> FALSE AND `data` <=> X'01'",
        "floats are not compared on MySQL and backslashes never reach a string literal"
    );
    assert_eq!(
        update.fetch.as_ref().unwrap().sql,
        "SELECT `id`, `Note`, `twice`, `flag`, `data` FROM `shop`.`orders` WHERE `id` = ?"
    );
    assert_eq!(update.fetch.as_ref().unwrap().params, [Some("7".to_owned())]);
    let empty = Changes { inserted: vec![Inserted { row: 0, cells: Vec::new() }], ..Changes::default() };
    assert_eq!(plan(&editing, &empty)[0].sql, "INSERT INTO `shop`.`orders` () VALUES ()");
}

#[test]
fn sqlite_saves_write_safe_literals_and_defaults_without_the_keyword() {
    let object = keyed_table();
    let columns =
        named_columns(&[("id", "INTEGER"), ("Note", "TEXT"), ("twice", "REAL"), ("flag", "INTEGER"), ("data", "BLOB")]);
    let editing = identify(&columns, "main", &object, None, SqlDialect::Sqlite);
    let mut changes = mysql_changes();
    changes.updated[0].cells.push((3, Staged::Default));
    changes.updated[0].cells.remove(2);
    let planned = plan(&editing, &changes);
    assert!(planned.iter().all(|statement| statement.params.is_empty() && statement.fetch.is_none()));
    assert_eq!(
        planned[0].sql,
        "INSERT INTO \"main\".\"orders\" (\"Note\") VALUES ('n') RETURNING \"id\", \"Note\", \"twice\", \"flag\", \"data\"",
        "a default is left out"
    );
    assert_eq!(
        planned[1].sql,
        "UPDATE \"main\".\"orders\" SET \"Note\" = 'it''s', \"data\" = X'ff', \"flag\" = (0) WHERE \"id\" = 7 AND \"Note\" IS 'a\\b' AND \"data\" IS X'01' AND \"flag\" IS FALSE RETURNING \"id\", \"Note\", \"twice\", \"flag\", \"data\""
    );
    assert_eq!(planned[1].sql, planned[1].preview);
    let empty =
        Changes { inserted: vec![Inserted { row: 0, cells: vec![(1, Staged::Default)] }], ..Changes::default() };
    assert!(plan(&editing, &empty)[0].sql.starts_with("INSERT INTO \"main\".\"orders\" DEFAULT VALUES RETURNING"));
}

#[test]
fn oracle_saves_write_literals_it_reads_and_compare_nulls_with_decode() {
    let object = keyed_table();
    let columns = named_columns(&[
        ("id", "NUMBER"),
        ("Note", "VARCHAR2"),
        ("twice", "BINARY_DOUBLE"),
        ("flag", "NUMBER"),
        ("data", "RAW"),
    ]);
    let editing = identify(&columns, "SHOP", &object, None, SqlDialect::Oracle);
    let planned = plan(&editing, &mysql_changes());
    assert!(planned.iter().all(|statement| statement.params.is_empty()));
    let update = planned.iter().find(|statement| statement.sql.starts_with("UPDATE")).unwrap();
    assert_eq!(
        update.sql,
        "UPDATE \"SHOP\".\"orders\" SET \"Note\" = 'it''s', \"flag\" = 1, \"data\" = HEXTORAW('ff') WHERE \"id\" = 7 AND DECODE(\"Note\", 'a\\b', 1, 0) = 1 AND DECODE(\"flag\", 0, 1, 0) = 1 AND DECODE(\"data\", HEXTORAW('01'), 1, 0) = 1"
    );
    assert!(!update.sql.contains("RETURNING"));
}

#[test]
fn generated_sql_is_written_in_the_editings_dialect() {
    let object = keyed_table();
    let columns = named_columns(&[("id", "int"), ("Note", "varchar")]);
    let editing = identify(&columns, "shop", &object, None, SqlDialect::MySql);
    let rows = vec![vec![Value::Integer(1), Value::Text("a\\b".into())]];
    assert_eq!(
        generate(&editing, Generate::Insert, &rows).unwrap(),
        "INSERT INTO `shop`.`orders` (`id`, `Note`) VALUES (1, _utf8mb4 X'615c62');"
    );
    assert_eq!(
        generate(&editing, Generate::Select, &rows).unwrap(),
        "SELECT `id`, `Note` FROM `shop`.`orders` WHERE `id` = 1;"
    );
}

#[test]
fn each_driver_says_whether_its_results_can_be_edited() {
    use crate::driver::Driver;
    let dialect = |driver: &dyn Driver| edit_dialect(driver.capabilities());
    assert_eq!(dialect(&crate::postgres::Postgres), Ok(SqlDialect::Postgres));
    assert_eq!(dialect(&crate::mysql::MySql::for_mysql()), Ok(SqlDialect::MySql));
    assert_eq!(dialect(&crate::mysql::MySql::for_mariadb()), Ok(SqlDialect::MariaDb));
    assert_eq!(dialect(&crate::sqlite::Sqlite), Ok(SqlDialect::Sqlite));
    assert_eq!(dialect(&crate::clickhouse::ClickHouse), Err(ReadOnly::NoRowIdentity), "no row identity");
    assert_eq!(dialect(&crate::redis::Redis), Err(ReadOnly::Driver), "not SQL");
}

#[test]
fn binary_input_with_any_characters_is_refused_not_a_panic() {
    let alphabet = ['0', 'a', 'F', 'é', 'ж', '€', '😀', 'x', ' '];
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    for _ in 0..2000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let length = (seed % 9) as usize;
        let digits: String = (0..length).map(|at| alphabet[((seed >> (at * 4 + 8)) % 9) as usize]).collect();
        let parsed = parse_input(&format!("\\x{digits}"), "bytea");
        let digits = digits.trim_end();
        let hex = digits.len().is_multiple_of(2) && digits.chars().all(|c| c.is_ascii_hexdigit());
        assert_eq!(parsed.is_ok(), hex, "{digits:?}");
    }
    assert_eq!(parse_input("\\x0é0", "bytea"), Err(InputError::Bytes));
    assert_eq!(parse_input("\\x0aFF", "bytea"), Ok(Value::Bytes(vec![0x0a, 0xff])));
}
