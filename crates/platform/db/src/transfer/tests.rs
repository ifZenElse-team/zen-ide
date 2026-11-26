use super::*;
use crate::value::Typed;

#[test]
fn type_names_of_every_driver_read_as_portable_types() {
    let cases = [
        ("integer", SqlType::Integer),
        ("int4", SqlType::Integer),
        ("int8", SqlType::BigInt),
        ("Int8", SqlType::SmallInt),
        ("int unsigned", SqlType::BigInt),
        ("int(11) unsigned", SqlType::BigInt),
        ("bigint unsigned", SqlType::Decimal { precision: Some(20), scale: Some(0) }),
        ("tinyint(1)", SqlType::Boolean),
        ("tinyint", SqlType::SmallInt),
        ("bool", SqlType::Boolean),
        ("double precision", SqlType::Double),
        ("float(10)", SqlType::Real),
        ("numeric(10,2)", SqlType::Decimal { precision: Some(10), scale: Some(2) }),
        ("numeric(10)", SqlType::Decimal { precision: Some(10), scale: Some(0) }),
        ("numeric", SqlType::Decimal { precision: None, scale: None }),
        ("character varying(40)", SqlType::Varchar { length: Some(40) }),
        ("varchar(255)", SqlType::Varchar { length: Some(255) }),
        ("bpchar", SqlType::Char { length: None }),
        ("longtext", SqlType::Text),
        ("enum('a','b')", SqlType::Text),
        ("bytea", SqlType::Bytes),
        ("varbinary(16)", SqlType::Bytes),
        ("timestamp(3) without time zone", SqlType::Timestamp),
        ("timestamp with time zone", SqlType::TimestampTz),
        ("timestamptz", SqlType::TimestampTz),
        ("datetime(6)", SqlType::Timestamp),
        ("time without time zone", SqlType::Time),
        ("jsonb", SqlType::Json),
        ("uuid", SqlType::Uuid),
        ("interval", SqlType::Interval),
        ("_int4", SqlType::Other),
        ("integer[]", SqlType::Other),
        ("Nullable(DateTime64(3, 'UTC'))", SqlType::TimestampTz),
        ("LowCardinality(Nullable(String))", SqlType::Text),
        ("Decimal(12, 4)", SqlType::Decimal { precision: Some(12), scale: Some(4) }),
        ("UInt64", SqlType::Decimal { precision: Some(20), scale: Some(0) }),
        ("VARCHAR(20)", SqlType::Varchar { length: Some(20) }),
        ("UNSIGNED BIG INT", SqlType::BigInt),
        ("geometry", SqlType::Other),
    ];
    for (name, expected) in cases {
        assert_eq!(SqlType::parse(name), expected, "{name}");
    }
}

#[test]
fn portable_types_are_written_in_each_dialect() {
    let decimal = SqlType::Decimal { precision: Some(12), scale: Some(2) };
    assert_eq!(decimal.sql(SqlDialect::Postgres, true), "numeric(12,2)");
    assert_eq!(decimal.sql(SqlDialect::MySql, true), "DECIMAL(12,2)");
    assert_eq!(decimal.sql(SqlDialect::Sqlite, true), "NUMERIC");
    assert_eq!(decimal.sql(SqlDialect::ClickHouse, true), "Nullable(Decimal(12, 2))");
    assert_eq!(decimal.sql(SqlDialect::ClickHouse, false), "Decimal(12, 2)");
    assert_eq!(SqlType::Decimal { precision: None, scale: None }.sql(SqlDialect::MySql, true), "DECIMAL(65,30)");
    assert_eq!(SqlType::Varchar { length: Some(40) }.sql(SqlDialect::MySql, true), "VARCHAR(40)");
    assert_eq!(SqlType::Varchar { length: Some(40_000) }.sql(SqlDialect::MySql, true), "LONGTEXT");
    assert_eq!(SqlType::Text.sql(SqlDialect::Postgres, true), "text");
    assert_eq!(SqlType::TimestampTz.sql(SqlDialect::MySql, true), "DATETIME(6)");
    assert_eq!(SqlType::Json.sql(SqlDialect::Postgres, true), "jsonb");
    assert_eq!(SqlType::Uuid.sql(SqlDialect::MariaDb, true), "CHAR(36)");
    assert_eq!(SqlType::Bytes.sql(SqlDialect::MySql, true), "LONGBLOB");
    assert_eq!(SqlType::Other.sql(SqlDialect::MySql, true), "LONGTEXT");
}

#[test]
fn types_are_copied_between_dialects_that_share_them_and_mapped_otherwise() {
    let (pg, my, maria) = (SqlDialect::Postgres, SqlDialect::MySql, SqlDialect::MariaDb);
    assert_eq!(target_type("character varying(40)", pg, pg, true), "character varying(40)");
    assert_eq!(target_type("integer[]", pg, pg, true), "integer[]");
    assert_eq!(target_type("int unsigned", my, maria, true), "int unsigned");
    assert_eq!(target_type("character varying(40)", pg, my, true), "VARCHAR(40)");
    assert_eq!(target_type("timestamp with time zone", pg, my, true), "DATETIME(6)");
    assert_eq!(target_type("tinyint(1)", my, pg, true), "boolean");
    assert_eq!(target_type("datetime(6)", my, pg, true), "timestamp");
    assert_eq!(target_type("integer[]", pg, my, true), "LONGTEXT");
}

fn text(cell: Result<Cell, &'static str>) -> String {
    match cell.unwrap() {
        Cell::Null => "NULL".to_owned(),
        Cell::Raw(text) => format!("raw {text}"),
        Cell::Text(text) => format!("text {text}"),
    }
}

#[test]
fn values_are_converted_for_the_target_column() {
    let (pg, my) = (SqlDialect::Postgres, SqlDialect::MySql);
    let any_decimal = SqlType::Decimal { precision: None, scale: None };
    assert_eq!(text(cell(&Value::Null, SqlType::Integer, pg)), "NULL");
    assert_eq!(text(cell(&Value::Integer(1), SqlType::Boolean, pg)), "raw TRUE");
    assert_eq!(cell(&Value::Integer(7), SqlType::Boolean, pg), Err("notABoolean"));
    assert_eq!(text(cell(&Value::Bool(true), SqlType::SmallInt, my)), "raw 1");
    assert_eq!(text(cell(&Value::Bool(false), SqlType::Boolean, my)), "raw FALSE");
    assert_eq!(text(cell(&Value::Bool(true), SqlType::Text, my)), "text true");
    assert_eq!(text(cell(&Value::Integer(42), SqlType::Varchar { length: None }, my)), "text 42");
    assert_eq!(text(cell(&Value::Decimal("12.50".to_owned()), any_decimal, my)), "raw 12.50");
    assert_eq!(text(cell(&Value::Float(f64::NAN), SqlType::Double, pg)), "text NaN");
    assert_eq!(cell(&Value::Float(f64::INFINITY), SqlType::Double, my), Err("notANumber"));
    assert_eq!(text(cell(&Value::Bytes(vec![1, 255]), SqlType::Bytes, pg)), "text \\x01ff");
    assert_eq!(text(cell(&Value::Bytes(vec![1, 255]), SqlType::Bytes, my)), "raw X'01ff'");
    assert_eq!(text(cell(&Value::Text("yes".to_owned()), SqlType::Boolean, my)), "raw TRUE");
    assert_eq!(cell(&Value::Text("x".to_owned()), SqlType::Integer, my), Err("notANumber"));
    let zoned =
        Value::Temporal(Typed { text: "2026-10-27 09:30:00+03".to_owned(), type_name: "timestamptz".to_owned() });
    assert_eq!(text(cell(&zoned, SqlType::Timestamp, my)), "text 2026-10-27 06:30:00");
    assert_eq!(text(cell(&zoned, SqlType::TimestampTz, pg)), "text 2026-10-27 09:30:00+03");
    let fraction =
        Value::Temporal(Typed { text: "2026-10-27 09:30:00.25+05:30".to_owned(), type_name: "timestamptz".to_owned() });
    assert_eq!(text(cell(&fraction, SqlType::Timestamp, my)), "text 2026-10-27 04:00:00.25");
    let naive = Value::Temporal(Typed { text: "2026-10-27 09:30:00".to_owned(), type_name: "datetime".to_owned() });
    assert_eq!(text(cell(&naive, SqlType::TimestampTz, my)), "text 2026-10-27 09:30:00");
    let infinite = Value::Temporal(Typed { text: "infinity".to_owned(), type_name: "timestamp".to_owned() });
    assert_eq!(cell(&infinite, SqlType::Timestamp, my), Err("notADate"));
    let array = Value::Array(vec![Value::Integer(1), Value::Integer(2)]);
    assert_eq!(text(cell(&array, SqlType::Other, pg)), "text {1,2}");
    assert_eq!(text(cell(&array, SqlType::Text, my)), "text [1,2]");
    let json = Value::Json("{\"a\":1}".to_owned());
    assert_eq!(text(cell(&json, SqlType::Json, my)), "text {\"a\":1}");
}

#[test]
fn the_select_and_create_table_are_written_in_their_dialects() {
    assert_eq!(
        select_statement(SqlDialect::MySql, "`shop`.`orders`", &["id".to_owned(), "total".to_owned()]),
        "SELECT `id`, `total` FROM `shop`.`orders`"
    );
    let columns = vec![
        NewColumn { name: "id".to_owned(), sql_type: "bigint".to_owned(), nullable: false },
        NewColumn { name: "note".to_owned(), sql_type: "text".to_owned(), nullable: true },
    ];
    assert_eq!(
        create_table(SqlDialect::Postgres, "\"public\".\"t\"", &columns, &["id".to_owned()]),
        "CREATE TABLE \"public\".\"t\" (\n    \"id\" bigint NOT NULL,\n    \"note\" text,\n    PRIMARY KEY (\"id\")\n)"
    );
    assert_eq!(
        create_table(SqlDialect::ClickHouse, "`t`", &columns, &[]),
        "CREATE TABLE `t` (\n    \"id\" bigint,\n    \"note\" text\n)\nENGINE = MergeTree ORDER BY tuple()"
    );
}
