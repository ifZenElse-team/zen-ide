use super::*;

const ALL: [SqlDialect; 5] =
    [SqlDialect::Postgres, SqlDialect::MySql, SqlDialect::MariaDb, SqlDialect::Sqlite, SqlDialect::ClickHouse];

#[test]
fn dialects_come_from_query_languages() {
    assert_eq!(SqlDialect::of_language("sql:postgres"), Some(SqlDialect::Postgres));
    assert_eq!(SqlDialect::of_language("sql:mariadb"), Some(SqlDialect::MariaDb));
    assert_eq!(SqlDialect::of_language("sql:sqlite"), Some(SqlDialect::Sqlite));
    assert_eq!(SqlDialect::of_language("sql:clickhouse"), Some(SqlDialect::ClickHouse));
    assert_eq!(SqlDialect::of_language("redis"), None);
    assert!(ALL.iter().all(|dialect| dialect.edits() == (*dialect != SqlDialect::ClickHouse)));
}

#[test]
fn identifiers_and_strings_are_quoted_for_each_server() {
    assert_eq!(SqlDialect::Postgres.quote("Order"), "\"Order\"");
    assert_eq!(SqlDialect::MySql.quote("Order"), "`Order`");
    assert_eq!(SqlDialect::MySql.quote("Total"), "`Total`");
    assert_eq!(SqlDialect::Sqlite.qualified("main", "my table"), "\"main\".\"my table\"");
    assert_eq!(SqlDialect::Postgres.string("it's \\n"), "'it''s \\n'");
    assert_eq!(SqlDialect::MySql.string("it's"), "'it''s'");
    assert_eq!(SqlDialect::MySql.string("a\\'b"), "_utf8mb4 X'615c2762'", "whatever NO_BACKSLASH_ESCAPES says");
    assert_eq!(SqlDialect::MariaDb.string("nul\0"), "_utf8mb4 X'6e756c00'");
    assert_eq!(SqlDialect::Sqlite.string("a\\b"), "'a\\b'");
    assert_eq!(SqlDialect::ClickHouse.string("a\\'b"), "'a\\\\\\'b'");
}

#[test]
fn generated_sql_quotes_every_identifier_so_keywords_never_read_as_sql() {
    for name in ["to", "true", "unique", "current_date", "key", "index", "range", "rank", "plain"] {
        assert_eq!(SqlDialect::Postgres.quote(name), format!("\"{name}\""));
        assert_eq!(SqlDialect::Sqlite.quote(name), format!("\"{name}\""));
        assert_eq!(SqlDialect::ClickHouse.quote(name), format!("\"{name}\""));
        assert_eq!(SqlDialect::MySql.quote(name), format!("`{name}`"));
        assert_eq!(SqlDialect::MariaDb.quote(name), format!("`{name}`"));
        assert_eq!(SqlDialect::SqlServer.quote(name), format!("[{name}]"));
        assert_eq!(SqlDialect::Oracle.quote(&name.to_uppercase()), format!("\"{}\"", name.to_uppercase()));
    }
    assert_eq!(crate::schema::qualified_for("postgresql", "public", "to"), "\"public\".\"to\"");
}

#[test]
fn literals_and_parameters_are_written_for_each_server() {
    let bytes = Value::Bytes(vec![0, 255]);
    assert_eq!(SqlDialect::Postgres.literal(&bytes), "'\\x00ff'");
    assert_eq!(SqlDialect::MySql.literal(&bytes), "X'00ff'");
    assert_eq!(SqlDialect::Sqlite.literal(&bytes), "X'00ff'");
    assert_eq!(SqlDialect::ClickHouse.literal(&bytes), "unhex('00ff')");
    assert!(ALL.iter().all(|dialect| dialect.literal(&Value::Bool(true)) == "TRUE"));
    assert!(ALL.iter().all(|dialect| dialect.literal(&Value::Null) == "NULL"));
    assert_eq!(SqlDialect::Postgres.param(&Value::Bool(false)), Some("f".to_owned()));
    assert_eq!(SqlDialect::MySql.param(&Value::Bool(false)), Some("0".to_owned()));
    assert_eq!(SqlDialect::MySql.param(&bytes), Some("00ff".to_owned()));
    assert_eq!(SqlDialect::MySql.placeholder(3, &bytes), "UNHEX(?)");
    assert_eq!(SqlDialect::Postgres.placeholder(3, &bytes), "$3");
    assert_eq!(SqlDialect::Sqlite.placeholder(1, &Value::Text("x'".into())), "'x'''");
    assert!(SqlDialect::MariaDb.inlined(&Value::Bool(true)) && !SqlDialect::Postgres.inlined(&Value::Bool(true)));
    let temporal =
        Value::Temporal(crate::value::Typed { text: "2026-10-27 10:00:00".into(), type_name: "datetime".into() });
    assert!(ALL.iter().all(|dialect| dialect.literal(&temporal) == "'2026-10-27 10:00:00'"));
}

#[test]
fn casts_patterns_orders_and_comparisons_follow_the_server() {
    assert_eq!(SqlDialect::Postgres.text("c"), "c::text");
    assert_eq!(SqlDialect::MySql.text("c"), "CAST(c AS CHAR)");
    assert_eq!(SqlDialect::Sqlite.text("c"), "CAST(c AS TEXT)");
    assert_eq!(SqlDialect::ClickHouse.text("c"), "toString(c)");
    assert_eq!(SqlDialect::Sqlite.contains("c", "a!b"), "CAST(c AS TEXT) LIKE '%a!!b%' ESCAPE '!'");
    assert_eq!(SqlDialect::MySql.order("c", false), "c IS NULL, c");
    assert_eq!(SqlDialect::MySql.order("2", true), "2 DESC");
    assert_eq!(SqlDialect::Postgres.order("c", true), "c DESC");
    assert_eq!(SqlDialect::Sqlite.order("c", false), "c NULLS LAST");
    assert_eq!(SqlDialect::Postgres.not_distinct("c", "$1"), "c IS NOT DISTINCT FROM $1");
    assert_eq!(SqlDialect::MySql.not_distinct("c", "?"), "c <=> ?");
    assert_eq!(SqlDialect::Sqlite.not_distinct("c", "1"), "c IS 1");
    assert_eq!(SqlDialect::MySql.insert_defaults("t"), "INSERT INTO t () VALUES ()");
    assert_eq!(SqlDialect::Sqlite.insert_defaults("t"), "INSERT INTO t DEFAULT VALUES");
    assert!(SqlDialect::Postgres.row_id() && !SqlDialect::Sqlite.row_id());
    assert_eq!(SqlDialect::MariaDb.last_insert_id(), Some("LAST_INSERT_ID()"));
}

#[test]
fn sql_server_gets_t_sql() {
    let sql = SqlDialect::SqlServer;
    assert_eq!(SqlDialect::of_language("sql:sqlserver"), Some(sql));
    assert!(sql.edits());
    assert_eq!(sql.quote("Order"), "[Order]");
    assert_eq!(sql.qualified("dbo", "a]b"), "[dbo].[a]]b]");
    assert_eq!(sql.string("it's"), "N'it''s'");
    assert_eq!(sql.literal(&Value::Bool(true)), "1");
    assert_eq!(sql.literal(&Value::Bytes(vec![0, 255])), "0x00ff");
    assert_eq!(sql.param(&Value::Bool(false)), Some("0".to_owned()));
    assert_eq!(sql.param(&Value::Bytes(vec![1])), Some("01".to_owned()));
    assert_eq!(sql.placeholders(), Placeholders::Named);
    assert_eq!(sql.placeholder(2, &Value::Integer(5)), "@P2");
    assert_eq!(sql.placeholder(1, &Value::Bytes(vec![1])), "CONVERT(VARBINARY(MAX), @P1, 2)");
    assert_eq!(sql.text("c"), "CAST(c AS NVARCHAR(MAX))");
    assert_eq!(sql.contains("c", "a[_"), "CAST(c AS NVARCHAR(MAX)) LIKE N'%a![!_%' ESCAPE '!'");
    assert_eq!(sql.order("c", false), "CASE WHEN c IS NULL THEN 1 ELSE 0 END, c");
    assert_eq!(sql.order("c", true), "CASE WHEN c IS NULL THEN 0 ELSE 1 END, c DESC");
    assert_eq!(sql.order("2", true), "2 DESC");
    assert_eq!(sql.not_distinct("c", "@P1"), "(c = @P1 OR (c IS NULL AND @P1 IS NULL))");
    assert_eq!(sql.insert_defaults("t"), "INSERT INTO t DEFAULT VALUES");
    assert!(!sql.returning() && !sql.row_id() && sql.last_insert_id().is_none());
    let lexing = sql.lexing();
    assert!(lexing.brackets && !lexing.backticks && !lexing.double_quoted_strings && !lexing.backslashes);
}

#[test]
fn oracle_writes_its_own_literals_and_quotes_every_name() {
    let sql = SqlDialect::Oracle;
    assert_eq!(SqlDialect::of_language("sql:oracle"), Some(sql));
    assert!(sql.edits());
    assert_eq!(sql.quote("ORDERS"), "\"ORDERS\"");
    assert_eq!(sql.quote("Orders"), "\"Orders\"");
    assert_eq!(sql.quote("SELECT"), "\"SELECT\"");
    assert_eq!(sql.qualified("HR", "A\"B"), "\"HR\".\"A\"\"B\"");
    assert_eq!(sql.string("it's"), "'it''s'");
    assert_eq!(sql.literal(&Value::Bool(true)), "1");
    assert_eq!(sql.literal(&Value::Bytes(vec![0, 255])), "HEXTORAW('00ff')");
    assert_eq!(sql.placeholders(), Placeholders::Inline);
    assert_eq!(sql.placeholder(1, &Value::Integer(5)), "5");
    assert_eq!(sql.text("c"), "TO_CHAR(c)");
    assert_eq!(sql.order("c", false), "c NULLS LAST");
    assert_eq!(sql.not_distinct("c", "1"), "DECODE(c, 1, 1, 0) = 1");
    assert!(!sql.returning() && !sql.row_id() && sql.last_insert_id().is_none());
    assert_eq!(sql.limit(5), " FETCH FIRST 5 ROWS ONLY");
    assert_eq!(sql.alias("f"), " f");
    assert_eq!(SqlDialect::Postgres.alias("f"), " AS f");
    let lexing = sql.lexing();
    assert!(!lexing.brackets && !lexing.backticks && !lexing.double_quoted_strings && !lexing.backslashes);
}

#[test]
fn tables_without_server_ids_get_stable_ones() {
    assert_eq!(table_id("shop", "orders"), table_id("shop", "orders"));
    assert_ne!(table_id("shop", "orders"), table_id("shopo", "rders"));
    assert_ne!(table_id("", ""), 0);
}
