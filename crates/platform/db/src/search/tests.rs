use super::*;

#[test]
fn classifies_types_of_every_driver() {
    for text in
        ["text", "character varying(20)", "VARCHAR(255)", "bpchar", "citext", "name", "uuid", "enum", "set", "longtext"]
    {
        assert_eq!(classify(text), ColumnClass::Text, "{text}");
    }
    for text in
        ["String", "Nullable(String)", "LowCardinality(Nullable(String))", "FixedString(8)", "Enum8('a' = 1)", ""]
    {
        assert_eq!(classify(text), ColumnClass::Text, "{text}");
    }
    for number in [
        "integer",
        "bigint",
        "int unsigned",
        "numeric(10,2)",
        "double precision",
        "real",
        "UInt64",
        "Int8",
        "Float32",
        "Decimal(10, 2)",
        "INTEGER",
    ] {
        assert_eq!(classify(number), ColumnClass::Number, "{number}");
    }
    for temporal in [
        "date",
        "timestamp with time zone",
        "timestamptz",
        "time",
        "interval",
        "DateTime64(3)",
        "Date32",
        "year",
        "datetime",
    ] {
        assert_eq!(classify(temporal), ColumnClass::Temporal, "{temporal}");
    }
    for other in [
        "jsonb",
        "json",
        "bytea",
        "blob",
        "boolean",
        "point",
        "text[]",
        "_int4",
        "Array(String)",
        "Map(String, String)",
        "xml",
    ] {
        assert_eq!(classify(other), ColumnClass::Other, "{other}");
    }
}

#[test]
fn catalogue_queries_read_the_schema_as_a_literal() {
    let postgres = catalog_query(SqlDialect::Postgres, "it's");
    assert!(postgres.contains("n.nspname = 'it''s'"), "{postgres}");
    assert!(postgres.contains("NOT c.relispartition"));
    let mysql = catalog_query(SqlDialect::MySql, "a\\b");
    assert!(mysql.contains("c.TABLE_SCHEMA = _utf8mb4 X'615c62'"), "{mysql}");
    let sqlite = catalog_query(SqlDialect::Sqlite, "main");
    assert!(sqlite.contains("FROM \"main\".sqlite_master m JOIN pragma_table_info(m.name, 'main') p"), "{sqlite}");
    let clickhouse = catalog_query(SqlDialect::ClickHouse, "it's");
    assert!(clickhouse.contains("c.database = 'it\\'s'"), "{clickhouse}");
}

fn row(values: [&str; 4]) -> Vec<Value> {
    values.iter().map(|value| Value::Text((*value).to_owned())).collect()
}

#[test]
fn catalogue_rows_group_into_tables_and_leave_out_foreign_ones() {
    let rows = vec![
        row(["orders", "id", "integer", "r"]),
        row(["orders", "note", "text", "r"]),
        row(["remote", "id", "integer", "f"]),
        row(["summary", "total", "numeric", "m"]),
        row(["v_orders", "note", "text", "v"]),
    ];
    let tables = catalog_tables(SqlDialect::Postgres, &rows);
    let names: Vec<(&str, ObjectKind, usize)> =
        tables.iter().map(|table| (table.name.as_str(), table.kind, table.columns.len())).collect();
    assert_eq!(
        names,
        [
            ("orders", ObjectKind::Table, 2),
            ("summary", ObjectKind::MaterializedView, 1),
            ("v_orders", ObjectKind::View, 1)
        ]
    );
    let clickhouse = vec![
        row(["events", "body", "String", "MergeTree"]),
        row(["queue", "body", "String", "Kafka"]),
        row(["daily", "n", "UInt64", "MaterializedView"]),
    ];
    let kinds: Vec<(String, ObjectKind)> =
        catalog_tables(SqlDialect::ClickHouse, &clickhouse).into_iter().map(|table| (table.name, table.kind)).collect();
    assert_eq!(kinds, [("events".to_owned(), ObjectKind::Table), ("daily".to_owned(), ObjectKind::MaterializedView)]);
    let mysql = vec![row(["t", "a", "varchar", "BASE TABLE"]), row(["w", "a", "varchar", "VIEW"])];
    let kinds: Vec<ObjectKind> =
        catalog_tables(SqlDialect::MySql, &mysql).into_iter().map(|table| table.kind).collect();
    assert_eq!(kinds, [ObjectKind::Table, ObjectKind::View]);
}

#[test]
fn conditions_follow_each_dialect_and_case() {
    assert_eq!(
        matches(SqlDialect::Postgres, "\"note\"", "50%_off", false),
        "\"note\"::text ILIKE '%50!%!_off%' ESCAPE '!'"
    );
    assert_eq!(matches(SqlDialect::Postgres, "\"note\"", "A", true), "\"note\"::text LIKE '%A%' ESCAPE '!'");
    assert_eq!(
        matches(SqlDialect::MySql, "note", "Ab", false),
        "LOWER(CAST(note AS CHAR CHARACTER SET utf8mb4)) LIKE LOWER('%Ab%') ESCAPE '!'"
    );
    assert_eq!(
        matches(SqlDialect::MariaDb, "note", "Ab", true),
        "CAST(note AS CHAR CHARACTER SET utf8mb4) COLLATE utf8mb4_bin LIKE '%Ab%' ESCAPE '!'"
    );
    assert_eq!(
        matches(SqlDialect::Sqlite, "\"note\"", "it's", false),
        "CAST(\"note\" AS TEXT) LIKE '%it''s%' ESCAPE '!'"
    );
    assert_eq!(matches(SqlDialect::Sqlite, "\"note\"", "50%", true), "instr(CAST(\"note\" AS TEXT), '50%') > 0");
    assert_eq!(matches(SqlDialect::ClickHouse, "note", "a_b", false), "toString(note) ILIKE '%a\\\\_b%'");
    assert_eq!(matches(SqlDialect::ClickHouse, "note", "a", true), "toString(note) LIKE '%a%'");
}

#[test]
fn data_queries_read_flags_never_values() {
    let columns = vec!["name".to_owned(), "Note".to_owned()];
    let sql = data_query(SqlDialect::Postgres, ("public", "orders"), &columns, "x", false, 101);
    assert_eq!(
        sql,
        "SELECT CASE WHEN \"name\"::text ILIKE '%x%' ESCAPE '!' THEN 1 ELSE 0 END, \
         CASE WHEN \"Note\"::text ILIKE '%x%' ESCAPE '!' THEN 1 ELSE 0 END \
         FROM \"public\".\"orders\" WHERE \"name\"::text ILIKE '%x%' ESCAPE '!' OR \"Note\"::text ILIKE '%x%' ESCAPE '!' LIMIT 101"
    );
    let mysql = data_query(SqlDialect::MySql, ("shop", "order"), &columns[..1], "x", true, 10);
    assert!(mysql.contains("FROM `shop`.`order` WHERE"), "{mysql}");
    assert!(mysql.ends_with("LIMIT 10"));
}

#[test]
fn filter_conditions_pass_the_filter_box() {
    for dialect in
        [SqlDialect::Postgres, SqlDialect::MySql, SqlDialect::MariaDb, SqlDialect::Sqlite, SqlDialect::ClickHouse]
    {
        for case_sensitive in [false, true] {
            let columns = vec!["a".to_owned(), "b c".to_owned()];
            let condition = filter_condition(dialect, &columns, "it's; 100% \\ \"x\"", case_sensitive);
            let composed = crate::filter::compose(
                crate::filter::Source::Table { name: "t", order: None },
                Some(&condition),
                &[],
                &columns,
                dialect,
            );
            assert!(composed.is_ok(), "{dialect:?} {condition}: {composed:?}");
        }
    }
}

#[test]
fn names_match_with_or_without_case() {
    assert!(name_matches("CustomerEmail", "email", false));
    assert!(!name_matches("CustomerEmail", "email", true));
    assert!(name_matches("ÄPFEL_LIST", "äpfel", false));
}
