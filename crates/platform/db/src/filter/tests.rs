use super::*;

const PG: SqlDialect = SqlDialect::Postgres;
const MYSQL: SqlDialect = SqlDialect::MySql;
const MARIADB: SqlDialect = SqlDialect::MariaDb;
const SQLITE: SqlDialect = SqlDialect::Sqlite;
const CLICKHOUSE: SqlDialect = SqlDialect::ClickHouse;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn sort(column: usize, name: &str, descending: bool) -> Sort {
    Sort { column, name: name.to_owned(), descending }
}

#[test]
fn a_condition_is_one_balanced_expression() {
    assert_eq!(check("id > 3 AND name LIKE 'a;b'", PG), Ok(()));
    assert_eq!(check("note = 'it''s (open'", PG), Ok(()));
    assert_eq!(check("x = $$a;)$$", PG), Ok(()));
    assert_eq!(check("\"we;ird\" = 1", PG), Ok(()));
    assert_eq!(check("id = 1; DROP TABLE t", PG), Err(FilterError::Semicolon));
    assert_eq!(check("id = 1) OR (1 = 1", PG), Err(FilterError::Parentheses));
    assert_eq!(check("(id = 1", PG), Err(FilterError::Parentheses));
    assert_eq!(check("name = 'open", PG), Err(FilterError::Unterminated));
    assert_eq!(check("id = 1 /* open", PG), Err(FilterError::Unterminated));
    assert_eq!(check("id = E'\\' ) --'", PG), Ok(()));
}

#[test]
fn each_dialect_reads_its_own_quotes_and_comments() {
    assert_eq!(check("name = 'a\\' ;'", MYSQL), Ok(()), "a backslash escapes a quote in MySQL");
    assert_eq!(check("name = 'a\\' OR 1 = 1; --'", PG), Err(FilterError::Semicolon), "but not in PostgreSQL");
    assert_eq!(check("`we;ird` = 1", MARIADB), Ok(()));
    assert_eq!(check("`we;ird` = 1", PG), Err(FilterError::Semicolon));
    assert_eq!(check("name = \"a;b\"", MYSQL), Ok(()), "a double-quoted string");
    assert_eq!(check("id = 1 # ; note", MYSQL), Ok(()));
    assert_eq!(check("id = 1 # ; note", SQLITE), Err(FilterError::Semicolon));
    assert_eq!(check("[we;ird] = 1", SQLITE), Ok(()));
    assert_eq!(check("x = $$a;$$", SQLITE), Err(FilterError::Semicolon), "dollar quotes are PostgreSQL's");
    assert_eq!(check("x = 'a\\'; DROP'", CLICKHOUSE), Ok(()));
    let condition = "`Total` > 1 AND \"Total\" = 'x' AND id = 2";
    assert_eq!(columns_named(condition, &names(&["Total", "id"]), MYSQL), [0, 1]);
    assert_eq!(
        without_column(condition, "Total", MYSQL),
        "\"Total\" = 'x' AND id = 2",
        "a MySQL string names no column"
    );
    assert_eq!(without_column(condition, "Total", PG), "id = 2", "a PostgreSQL identifier in double quotes");
    assert_eq!(and("a = 'x OR y'", "b = 1", MYSQL), "a = 'x OR y' AND b = 1");
}

#[test]
fn a_table_gets_the_condition_in_its_select_and_its_key_order_without_a_sort() {
    let table = Source::Table { name: "public.orders", order: Some("id") };
    let columns = names(&["id", "total", "Note"]);
    assert_eq!(
        compose(table, Some("total > 10"), &[], &columns, PG).unwrap(),
        ("SELECT * FROM public.orders WHERE (total > 10) ORDER BY id".to_owned(), Composition::Table)
    );
    let sorted = compose(table, None, &[sort(2, "Note", true), sort(1, "total", false)], &columns, PG).unwrap();
    assert_eq!(sorted.0, "SELECT * FROM public.orders ORDER BY \"Note\" DESC, \"total\"");
    let commented = compose(table, Some("id = 1 -- one"), &[], &columns, PG).unwrap();
    assert_eq!(commented.0, "SELECT * FROM public.orders WHERE (id = 1 -- one\n) ORDER BY id");
}

#[test]
fn sorting_puts_nulls_where_the_in_memory_sort_does_in_every_dialect() {
    let mysql = Source::Table { name: "shop.orders", order: Some("id") };
    let columns = names(&["id", "Note"]);
    let sorts = [sort(1, "Note", true), sort(0, "id", false)];
    assert_eq!(
        compose(mysql, Some("`Note` <> 'x'"), &sorts, &columns, MYSQL).unwrap().0,
        "SELECT * FROM shop.orders WHERE (`Note` <> 'x') ORDER BY `Note` IS NULL DESC, `Note` DESC, `id` IS NULL, `id`"
    );
    let sqlite = Source::Table { name: "main.orders", order: None };
    assert_eq!(
        compose(sqlite, None, &sorts, &columns, SQLITE).unwrap().0,
        "SELECT * FROM main.orders ORDER BY \"Note\" DESC NULLS FIRST, \"id\" NULLS LAST"
    );
    assert_eq!(
        compose(Source::Table { name: "db.t", order: None }, None, &sorts, &columns, CLICKHOUSE).unwrap().0,
        "SELECT * FROM db.t ORDER BY \"Note\" DESC NULLS FIRST, \"id\" NULLS LAST"
    );
    let duplicate = compose(
        Source::Query("SELECT 1 a, 2 a UNION SELECT 3, 4"),
        None,
        &[sort(1, "a", true)],
        &names(&["a", "a"]),
        MYSQL,
    );
    assert!(duplicate.unwrap().0.ends_with("AS zen_filtered ORDER BY 2 DESC"), "MySQL sorts a position as it is");
    let merged = compose(Source::Query("SELECT id FROM t LIMIT 5"), Some("id > 1"), &[], &names(&["id"]), MARIADB);
    assert_eq!(merged.unwrap().1, Composition::Wrapped, "LIMIT stays inside the subselect");
}

#[test]
fn a_plain_select_gets_the_condition_merged_and_anything_else_is_wrapped() {
    let columns = names(&["id", "name"]);
    let plain = Source::Query("SELECT id, name FROM t WHERE id > 1 OR id < 0 ORDER BY name; -- done");
    assert_eq!(
        compose(plain, Some("name LIKE 'a%'"), &[], &columns, PG).unwrap(),
        (
            "SELECT id, name FROM t WHERE (id > 1 OR id < 0) AND (name LIKE 'a%') ORDER BY name".to_owned(),
            Composition::Merged
        )
    );
    assert_eq!(
        compose(plain, None, &[sort(0, "id", true)], &columns, PG).unwrap().0,
        "SELECT id, name FROM t WHERE id > 1 OR id < 0 ORDER BY \"id\" DESC"
    );
    let no_where = Source::Query("select * from t");
    assert_eq!(compose(no_where, Some("id = 2"), &[], &columns, PG).unwrap().0, "select * from t WHERE (id = 2)");
    let grouped = Source::Query("SELECT name, count(*) FROM t GROUP BY name");
    assert_eq!(
        compose(grouped, Some("count > 1"), &[sort(1, "count", false)], &names(&["name", "count"]), PG).unwrap(),
        (
            "SELECT * FROM (\nSELECT name, count(*) FROM t GROUP BY name\n) AS zen_filtered WHERE (count > 1) ORDER BY \"count\""
                .to_owned(),
            Composition::Wrapped
        )
    );
    let duplicate = compose(
        Source::Query("SELECT 1 a, 2 a UNION SELECT 3, 4"),
        None,
        &[sort(1, "a", false)],
        &names(&["a", "a"]),
        PG,
    );
    assert!(duplicate.unwrap().0.ends_with("AS zen_filtered ORDER BY 2"), "a name two columns share sorts by position");
    let nested = Source::Query("SELECT * FROM (SELECT id FROM t WHERE x = 1) s");
    assert_eq!(compose(nested, Some("id = 3"), &[], &columns, PG).unwrap().1, Composition::Merged);
    let cte = Source::Query("WITH a AS (SELECT 1 AS id) SELECT * FROM a");
    assert_eq!(compose(cte, Some("id = 1"), &[], &columns, PG).unwrap().1, Composition::Wrapped);
    let update = Source::Query("UPDATE t SET a = 1 RETURNING *");
    assert_eq!(compose(update, Some("a = 1"), &[], &columns, PG), Err(FilterError::NotAQuery));
    assert_eq!(compose(update, None, &[], &columns, PG).unwrap().1, Composition::Unchanged);
    assert_eq!(compose(plain, Some("1 = 1); DELETE FROM t; --"), &[], &columns, PG), Err(FilterError::Semicolon));
    let hashed = Source::Query("SELECT id FROM t # all of them");
    assert_eq!(compose(hashed, Some("id = 1"), &[], &columns, MYSQL).unwrap().0, "SELECT id FROM t WHERE (id = 1)");
}

#[test]
fn quick_filters_write_literals_and_handle_null() {
    assert_eq!(quick("name", Operator::Equal, &[Value::Text("O'Hara".into())], PG), "\"name\" = 'O''Hara'");
    assert_eq!(quick("Total", Operator::Greater, &[Value::Decimal("10.50".into())], PG), "\"Total\" > 10.50");
    assert_eq!(quick("id", Operator::NotEqual, &[Value::Null], PG), "\"id\" IS NOT NULL");
    assert_eq!(quick("id", Operator::Equal, &[Value::Null], PG), "\"id\" IS NULL");
    assert_eq!(
        quick("note", Operator::Contains, &[Value::Text("50%_off!".into())], PG),
        "\"note\"::text LIKE '%50!%!_off!!%' ESCAPE '!'"
    );
    assert_eq!(
        quick("id", Operator::In, &[Value::Integer(1), Value::Integer(2), Value::Integer(1), Value::Null], PG),
        "(\"id\" IN (1, 2) OR \"id\" IS NULL)"
    );
    assert_eq!(quick("flag", Operator::Equal, &[Value::Bool(true)], PG), "\"flag\" = TRUE");
    assert_eq!(typed_value("42", true), Value::Decimal("42".into()));
    assert_eq!(typed_value("42", false), Value::Text("42".into()));
}

#[test]
fn quick_filters_are_written_in_each_dialect() {
    let text = |text: &str| Value::Text(text.to_owned());
    assert_eq!(quick("Order", Operator::Equal, &[text("O'Hara")], MYSQL), "`Order` = 'O''Hara'");
    assert_eq!(quick("Name", Operator::Equal, &[text("a\\b")], MARIADB), "`Name` = _utf8mb4 X'615c62'");
    assert_eq!(
        quick("Name", Operator::Contains, &[text("50%")], MYSQL),
        "CAST(`Name` AS CHAR) LIKE '%50!%%' ESCAPE '!'"
    );
    assert_eq!(
        quick("Name", Operator::Contains, &[text("a_b")], SQLITE),
        "CAST(\"Name\" AS TEXT) LIKE '%a!_b%' ESCAPE '!'"
    );
    assert_eq!(
        quick("Name", Operator::Contains, &[text("5%\\")], CLICKHOUSE),
        "toString(\"Name\") LIKE '%5\\\\%\\\\\\\\%'"
    );
    assert_eq!(quick("data", Operator::Equal, &[Value::Bytes(vec![0xde, 0xad])], SQLITE), "\"data\" = X'dead'");
    assert_eq!(quick("data", Operator::Equal, &[Value::Bytes(vec![0xde, 0xad])], PG), "\"data\" = '\\xdead'");
    assert_eq!(quick("flag", Operator::Equal, &[Value::Bool(false)], SQLITE), "\"flag\" = FALSE");
    assert_eq!(quick("t", Operator::Less, &[text("it's")], CLICKHOUSE), "\"t\" < 'it\\'s'");
}

#[test]
fn conditions_are_joined_and_one_columns_parts_reset() {
    assert_eq!(and("", "id = 1", PG), "id = 1");
    assert_eq!(and("a = 1 OR b = 2", "id = 1", PG), "(a = 1 OR b = 2) AND id = 1");
    let condition = "id BETWEEN 1 AND 5 AND \"Name\" = 'x' AND (total > 1 OR id = 2)";
    assert_eq!(without_column(condition, "Name", PG), "id BETWEEN 1 AND 5 AND (total > 1 OR id = 2)");
    assert_eq!(without_column(condition, "id", PG), "\"Name\" = 'x'");
    assert_eq!(columns_named(condition, &names(&["id", "Name", "other"]), PG), [0, 1]);
}

#[test]
fn completion_offers_columns_then_keywords() {
    let columns = vec![("name".to_owned(), "text".to_owned()), ("Note".to_owned(), "text".to_owned())];
    let (start, end, found) = complete("id = 1 an", 9, &columns, PG);
    assert_eq!((start, end), (7, 9));
    assert_eq!(found.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["AND"]);
    let (_, _, found) = complete("n", 1, &columns, PG);
    assert_eq!(found.iter().map(|item| item.insert.as_str()).collect::<Vec<_>>(), ["name", "\"Note\"", "NOT", "NULL"]);
    let (_, _, all) = complete("", 0, &columns, PG);
    assert_eq!(all.len(), 2, "an empty word lists the columns only");
    let (_, _, found) = complete("`no", 3, &columns, MYSQL);
    assert_eq!(found.iter().map(|item| item.insert.as_str()).collect::<Vec<_>>(), ["Note", "NOT"]);
    let (_, _, found) = complete("il", 2, &columns, SQLITE);
    assert!(found.is_empty(), "ILIKE is PostgreSQL's");
}

#[test]
fn a_plain_select_of_one_table_is_traced_to_it() {
    let read = |sql: &str| table_read(sql, SQLITE);
    assert_eq!(
        read("SELECT * FROM \"main\".\"Orders\" WHERE (id > 1) ORDER BY id NULLS LAST LIMIT 10"),
        Some(TableRead { schema: Some("main".into()), table: "Orders".into(), columns: None })
    );
    assert_eq!(
        read("select id, \"Note\", o.total from orders as o -- note"),
        Some(TableRead { schema: None, table: "orders".into(), columns: Some(names(&["id", "Note", "total"])) })
    );
    assert_eq!(read("SELECT id + 1 FROM t"), None);
    assert_eq!(read("SELECT id AS x FROM t"), None);
    assert_eq!(read("SELECT DISTINCT id FROM t"), None);
    assert_eq!(read("SELECT * FROM a JOIN b ON a.id = b.id"), None);
    assert_eq!(read("SELECT * FROM a, b"), None);
    assert_eq!(read("SELECT * FROM t GROUP BY id"), None);
    assert_eq!(read("SELECT * FROM (SELECT 1) AS s"), None);
    assert_eq!(read("SELECT * FROM (\nSELECT * FROM t\n) AS zen_filtered WHERE (id = 1)"), None);
    assert_eq!(read("SELECT * FROM t UNION SELECT * FROM u"), None);
    assert_eq!(read("SELECT * FROM t WHERE id IN (SELECT id FROM u)").map(|read| read.table), Some("t".into()));
    assert_eq!(table_read("SELECT `Id` FROM `shop`.`t`", MYSQL).unwrap().columns, Some(names(&["Id"])));
}
