use super::*;

#[test]
fn rows_keep_their_order_and_repeated_names_and_read_bare_numbers() {
    let fields = row(r#"{"b":1,"a":"x","b":NaN,"c":-Infinity,"s":"NaN in text"}"#).unwrap();
    let names: Vec<&str> = fields.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["b", "a", "b", "c", "s"]);
    assert_eq!(fields[2].1, "NaN");
    assert_eq!(fields[3].1, "-Infinity");
    assert_eq!(fields[4].1, "NaN in text");
    assert!(row("").is_none() && row("[1]").is_none());
}

#[test]
fn values_follow_the_column_types() {
    assert_eq!(cell(&json!(42), Some("INTEGER")), value::integer(42));
    assert_eq!(cell(&json!("92233720368547758070"), Some("HUGEINT")), value::decimal("92233720368547758070"));
    assert_eq!(cell(&json!("1.50"), Some("DECIMAL(10,2)")), value::decimal("1.50"));
    assert_eq!(cell(&json!("NaN"), Some("DOUBLE")), value::float(f64::NAN));
    assert_eq!(cell(&json!(2.5), Some("DOUBLE")), value::float(2.5));
    assert_eq!(cell(&json!("2026-10-28"), Some("DATE")), value::temporal("2026-10-28", "date"));
    assert_eq!(cell(&json!("\\x01a"), Some("BLOB")), value::bytes(&[1, b'a']));
    assert_eq!(cell(&json!([1, 2]), Some("INTEGER[]")), value::json_text("[1,2]"));
    assert_eq!(cell(&json!({"k": 1}), Some("STRUCT(k INTEGER)")), value::json_text("{\"k\":1}"));
    assert_eq!(cell(&json!(null), Some("VARCHAR")), value::null());
    assert_eq!(cell(&json!("{\"a\":1}"), Some("JSON")), value::json_text("{\"a\":1}"));
    assert_eq!(cell(&json!(1.5), None), value::float(1.5));
    assert_eq!(cell(&json!("x"), Some("GEOMETRY")), value::other("x", "GEOMETRY"));
}
