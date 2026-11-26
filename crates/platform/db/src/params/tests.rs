use std::collections::BTreeMap;

use super::*;

fn names_of(text: &str, dialect: Dialect) -> Vec<String> {
    placeholders(text, dialect).into_iter().map(|placeholder| placeholder.name).collect()
}

#[test]
fn placeholders_are_found_only_in_code() {
    let text = "SELECT :id, ':no', \":no\", $1 -- :no ?\n/* :no $2 */ FROM t WHERE a = :name AND b::int = $$ :no $$";
    assert_eq!(names_of(text, Dialect::Postgres), ["id", "$1", "name"]);
    let mysql = "SELECT ? , `:no`, '\\' ?', :id # ?\n FROM t WHERE x = ? AND @v := 1";
    assert_eq!(names_of(mysql, Dialect::MySql), ["?1", "id", "?2"]);
    assert_eq!(names_of("SELECT data ? 'key' FROM t", Dialect::Postgres), Vec::<String>::new());
    assert_eq!(names_of("SELECT $tag$ :no $tag$, $3", Dialect::Postgres), ["$3"]);
    assert_eq!(names_of("SELECT E'it\\'s :no', :yes", Dialect::Postgres), ["yes"]);
    assert_eq!(names_of("SELECT arr[1:n], ts::date, a$1 FROM t", Dialect::Postgres), Vec::<String>::new());
    assert_eq!(names_of("GET :key", Dialect::Lines), Vec::<String>::new());
    assert_eq!(names("SELECT :a, :b, :a", Dialect::Standard), ["a", "b"]);
}

#[test]
fn sqlite_takes_numbered_question_and_named_parameters() {
    assert_eq!(names_of("SELECT $1, ?1, ?, :name, a$1 FROM t", Dialect::Standard), ["$1", "?1", "?2", "name"]);
    let values: BTreeMap<String, Value> = [
        ("$1".to_owned(), Value::Integer(7)),
        ("?1".to_owned(), Value::Integer(8)),
        ("name".to_owned(), Value::Integer(9)),
    ]
    .into();
    for (text, sent) in [
        ("SELECT * FROM orders WHERE customer_id = $1", "SELECT * FROM orders WHERE customer_id = 7"),
        ("SELECT * FROM orders WHERE customer_id = ?1", "SELECT * FROM orders WHERE customer_id = 8"),
        ("SELECT * FROM orders WHERE customer_id = ?", "SELECT * FROM orders WHERE customer_id = 8"),
        ("SELECT * FROM orders WHERE customer_id = :name", "SELECT * FROM orders WHERE customer_id = 9"),
    ] {
        let bound = bind(text, Dialect::Standard, (SqlDialect::Sqlite, false), &values).unwrap();
        assert_eq!(bound.text, sent, "{text}");
    }
    assert_eq!(names_of("SELECT $1 FROM t WHERE x = ?1", Dialect::MySql), ["?1"]);
}

#[test]
fn values_are_bound_as_each_driver_takes_them() {
    let values: BTreeMap<String, Value> = [
        ("id".to_owned(), Value::Integer(7)),
        ("name".to_owned(), Value::Text("O'Brien".to_owned())),
        ("$1".to_owned(), Value::Bool(true)),
    ]
    .into();
    let text = "SELECT * FROM t WHERE id = :id AND name = :name OR id = :id AND ok = $1";
    let postgres = bind(text, Dialect::Postgres, (SqlDialect::Postgres, true), &values).unwrap();
    assert_eq!(postgres.text, "SELECT * FROM t WHERE id = $1 AND name = $2 OR id = $1 AND ok = $3");
    assert_eq!(postgres.params.iter().map(|bound| bound.name.as_str()).collect::<Vec<_>>(), ["id", "name", "$1"]);
    assert!(postgres.inlined.is_empty());
    let mysql_text = "SELECT * FROM t WHERE id = :id AND name = :name OR id = :id";
    let mysql = bind(mysql_text, Dialect::MySql, (SqlDialect::MySql, true), &values).unwrap();
    assert_eq!(mysql.text, "SELECT * FROM t WHERE id = ? AND name = ? OR id = ?");
    assert_eq!(mysql.params.iter().map(|bound| bound.name.as_str()).collect::<Vec<_>>(), ["id", "name", "id"]);
    let sqlite = bind(mysql_text, Dialect::Standard, (SqlDialect::Sqlite, false), &values).unwrap();
    assert_eq!(sqlite.text, "SELECT * FROM t WHERE id = 7 AND name = 'O''Brien' OR id = 7");
    assert!(sqlite.params.is_empty());
    assert_eq!(sqlite.inlined.len(), 2);
    let missing = bind("SELECT :a, :b, :a", Dialect::Standard, (SqlDialect::Sqlite, false), &values).unwrap_err();
    assert_eq!(missing, ["a", "b"]);
}

#[test]
fn typed_values_are_checked() {
    assert_eq!(ParamValue { kind: ParamType::Number, value: " 42 ".into() }.to_value(), Ok(Value::Integer(42)));
    assert_eq!(
        ParamValue { kind: ParamType::Number, value: "1.50".into() }.to_value(),
        Ok(Value::Decimal("1.50".into()))
    );
    assert!(ParamValue { kind: ParamType::Number, value: "1; DROP".into() }.to_value().is_err());
    assert!(ParamValue { kind: ParamType::Number, value: "inf".into() }.to_value().is_err());
    assert_eq!(ParamValue { kind: ParamType::Boolean, value: "Yes".into() }.to_value(), Ok(Value::Bool(true)));
    assert!(ParamValue { kind: ParamType::Boolean, value: "maybe".into() }.to_value().is_err());
    assert_eq!(ParamValue::null().to_value(), Ok(Value::Null));
    assert_eq!(ParamValue::text(" a ").to_value(), Ok(Value::Text(" a ".into())));
    assert_eq!(ParamValue::of_json(&serde_json::json!(5)).unwrap().kind, ParamType::Number);
    assert_eq!(ParamValue::of_json(&serde_json::json!(null)).unwrap(), ParamValue::null());
    assert_eq!(
        ParamValue::of_json(&serde_json::json!({ "type": "boolean", "value": "false" })).unwrap().to_value(),
        Ok(Value::Bool(false))
    );
    assert!(ParamValue::of_json(&serde_json::json!([1])).is_err());
}

#[test]
fn variables_expand_and_script_commands_parse() {
    let values: BTreeMap<String, String> = [("table".to_owned(), "orders".to_owned())].into();
    assert_eq!(expand("SELECT * FROM ${table} -- ${table}", &values).unwrap(), "SELECT * FROM orders -- orders");
    assert_eq!(expand("SELECT ${a}, ${b}, ${a}, ${ x}", &values).unwrap_err(), ["a", "b"]);
    assert_eq!(
        script_command("@set table = 'orders';"),
        Some(Ok(ScriptCommand::Set { name: "table".into(), value: "orders".into() }))
    );
    assert_eq!(
        script_command("@SET limit=10"),
        Some(Ok(ScriptCommand::Set { name: "limit".into(), value: "10".into() }))
    );
    assert_eq!(
        script_command("@include \"lib/setup.sql\""),
        Some(Ok(ScriptCommand::Include { file: "lib/setup.sql".into() }))
    );
    assert!(matches!(script_command("@set nothing"), Some(Err(_))));
    assert!(matches!(script_command("@set 1x = 2"), Some(Err(_))));
    assert_eq!(script_command("@settle"), None);
    assert_eq!(script_command("SELECT 1"), None);
}

#[test]
fn secret_values_are_masked_when_shown() {
    let values = vec![
        NamedValue { name: "id".into(), value: Value::Integer(1) },
        NamedValue { name: "user_password".into(), value: Value::Text("hunter2".into()) },
        NamedValue { name: "id".into(), value: Value::Integer(1) },
        NamedValue { name: "?1".into(), value: Value::Null },
    ];
    assert_eq!(shown(&values, SqlDialect::Postgres), [":id = 1", ":user_password = ***", "?1 = NULL"]);
    assert!(secret_name("API_KEY") && secret_name("authToken") && !secret_name("author"));
}

fn inferred(text: &str, dialect: Dialect) -> Vec<(String, ParamType)> {
    let columns = |name: &str| match name {
        "id" | "amount" => Some(ParamType::Number),
        "active" => Some(ParamType::Boolean),
        _ => None,
    };
    infer_types(text, dialect, &columns).into_iter().collect()
}

fn pairs(expected: &[(&str, ParamType)]) -> Vec<(String, ParamType)> {
    let mut pairs: Vec<(String, ParamType)> = expected.iter().map(|(name, kind)| ((*name).to_owned(), *kind)).collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

#[test]
fn a_placeholder_takes_its_type_from_where_it_is_used() {
    use ParamType::{Boolean, Number};
    let text = "SELECT * FROM t WHERE n > 5 AND :a = 10 AND x = :b + 1 AND y = :c * 2 LIMIT :d OFFSET :e";
    assert_eq!(
        inferred(text, Dialect::Postgres),
        pairs(&[("a", Number), ("b", Number), ("c", Number), ("d", Number), ("e", Number)])
    );
    let columns = "UPDATE t SET amount = :amount, active = :on, name = :name WHERE t.id = :id AND :flag = true";
    assert_eq!(
        inferred(columns, Dialect::Postgres),
        pairs(&[("amount", Number), ("on", Boolean), ("id", Number), ("flag", Boolean)])
    );
    let mysql = "SELECT * FROM t WHERE `id` IN (?, ?) AND amount BETWEEN ? AND ? AND name = ? LIMIT ?";
    assert_eq!(
        inferred(mysql, Dialect::MySql),
        pairs(&[("?1", Number), ("?2", Number), ("?3", Number), ("?4", Number), ("?6", Number)])
    );
    let casts = "SELECT $1::bigint, CAST($2 AS boolean), $3::text, $4 WHERE note = ':x = 1' -- :y = 2";
    assert_eq!(inferred(casts, Dialect::Postgres), pairs(&[("$1", Number), ("$2", Boolean)]));
    assert_eq!(inferred("SELECT :a || 'x', :b WHERE name = :c", Dialect::Postgres), pairs(&[]));
    assert_eq!(inferred("SELECT * FROM t WHERE id = :id OR id = '5'", Dialect::Standard), pairs(&[("id", Number)]));
}

#[test]
fn column_types_read_as_parameter_types() {
    for number in [
        "integer",
        "int4",
        "BIGINT",
        "numeric(10,2)",
        "double precision",
        "Nullable(UInt64)",
        "Float64",
        "tinyint(1)",
        "decimal(8,2) unsigned",
    ] {
        assert_eq!(column_type(number), Some(ParamType::Number), "{number}");
    }
    for boolean in ["boolean", "bool", "Nullable(Bool)"] {
        assert_eq!(column_type(boolean), Some(ParamType::Boolean), "{boolean}");
    }
    for other in [
        "text",
        "character varying(40)",
        "date",
        "timestamp with time zone",
        "integer[]",
        "_int4",
        "interval",
        "uuid",
        "json",
    ] {
        assert_eq!(column_type(other), None, "{other}");
    }
    assert!(mentions("SELECT * FROM Orders o", "orders"));
    assert!(!mentions("SELECT * FROM orders_archive", "orders"));
}

#[test]
fn inference_survives_unclosed_quotes_and_other_scripts() {
    assert_eq!(inferred("SELECT \"имя = :a", Dialect::Postgres), pairs(&[]));
    assert_eq!(
        inferred("SELECT * FROM t WHERE \"Id\" = :a AND `x", Dialect::Postgres),
        pairs(&[("a", ParamType::Number)])
    );
    assert_eq!(inferred("SELECT * FROM t WHERE \"id\" = :a", Dialect::Postgres), pairs(&[("a", ParamType::Number)]));
    assert_eq!(
        inferred("SELECT * FROM t WHERE [id] = ? AND ´ = 1", Dialect::SqlServer),
        pairs(&[("?1", ParamType::Number)])
    );
    assert_eq!(inferred("GET :key", Dialect::Lines), pairs(&[]));
}

#[test]
fn a_negative_value_inlined_after_a_minus_stays_a_number_not_a_comment() {
    let values: BTreeMap<String, Value> =
        [("n".to_owned(), Value::Integer(-3)), ("f".to_owned(), Value::Float(-1.5))].into();
    let bound = bind("SELECT 5-:n, 2 -:f, 1+:n", Dialect::Standard, (SqlDialect::Sqlite, false), &values).unwrap();
    assert_eq!(bound.text, "SELECT 5- -3, 2 - -1.5, 1+-3");
}
