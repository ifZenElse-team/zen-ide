use super::*;
use crate::export::{Target, render};
use crate::value::{Column, Typed};

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn rows() -> Vec<Vec<Value>> {
    vec![
        vec![Value::Integer(1), Value::Text("Ann".into()), Value::Bool(true), Value::Null],
        vec![
            Value::Integer(2),
            Value::Text("tab\there \"quoted\"\nline".into()),
            Value::Bool(false),
            Value::Float(1.5),
        ],
    ]
}

fn texts(table: &Table) -> Vec<Vec<String>> {
    table.rows.iter().map(|row| row.iter().map(|field| field.text.clone()).collect()).collect()
}

#[test]
fn plain_copy_is_tab_separated_with_quotes_only_where_needed() {
    let settings = CopySettings::default();
    let lines: Vec<String> =
        rows().iter().enumerate().map(|(at, row)| settings.row_line(at + 1, row, SqlDialect::Postgres)).collect();
    assert_eq!(lines[0], "1\tAnn\ttrue\t");
    assert_eq!(lines[1], "2\t\"tab\there \"\"quoted\"\"\nline\"\tfalse\t1.5");
    assert_eq!(settings.value(&Value::Text(String::new()), SqlDialect::Postgres), "\"\"", "empty text is not NULL");
    let parsed = parse(&lines.join("\n"), None, '"', usize::MAX);
    assert_eq!(parsed.delimiter, '\t');
    assert_eq!(texts(&parsed)[1][1], "tab\there \"quoted\"\nline", "copied text pastes back whole");
    assert_eq!(parsed.rows[0][3].value(""), None, "an empty field is NULL");
}

#[test]
fn advanced_copy_writes_its_options() {
    let settings = CopySettings {
        header: true,
        row_numbers: true,
        column_delimiter: ";".into(),
        row_delimiter: "\r\n".into(),
        quote: '\'',
        quoting: Quoting::Always,
        null_text: "NULL".into(),
        values: ValueStyle::Display,
    };
    assert_eq!(settings.header_line(&names(&["id", "name"])), "'#';'id';'name'");
    let row = vec![Value::Integer(7), Value::Text("O'Brien".into()), Value::Null];
    assert_eq!(settings.row_line(3, &row, SqlDialect::Postgres), "3;'7';'O''Brien';NULL");
    let sql = CopySettings { values: ValueStyle::Sql, ..CopySettings::default() };
    assert_eq!(sql.row_line(1, &row, SqlDialect::MySql), "7\t'O''Brien'\tNULL");
    let json = CopySettings { values: ValueStyle::Json, ..CopySettings::default() };
    assert_eq!(json.row_line(1, &row, SqlDialect::Postgres), "7\t\"O'Brien\"\tnull");
    assert_eq!(
        CopySettings { column_delimiter: String::new(), ..CopySettings::default() }.problem(),
        Some("emptyDelimiter")
    );
    assert_eq!(
        CopySettings { column_delimiter: "\"".into(), ..CopySettings::default() }.problem(),
        Some("delimiterQuote")
    );
    assert_eq!(
        CopySettings { column_delimiter: "\"".into(), quoting: Quoting::Never, ..CopySettings::default() }.problem(),
        None
    );
}

fn columns(list: &[&str]) -> Vec<Column> {
    list.iter().map(|name| Column::new(*name, "text")).collect()
}

#[test]
fn copy_as_reuses_the_export_writers() {
    let target = Target { dialect: SqlDialect::MySql, table: "`shop`.`people`".into(), title: "people".into() };
    let cols = columns(&["id", "name", "ok", "score"]);
    let csv = render(&export_settings(export::Format::Csv), cols.clone(), &rows(), target.clone());
    assert_eq!(csv, "id,name,ok,score\n1,Ann,true,\n2,\"tab\there \"\"quoted\"\"\nline\",false,1.5\n");
    let tsv = render(&export_settings(export::Format::Tsv), cols.clone(), &rows()[..1], target.clone());
    assert_eq!(tsv, "id\tname\tok\tscore\n1\tAnn\ttrue\t\n");
    let json = render(&export_settings(export::Format::Json), cols.clone(), &rows()[..1], target.clone());
    assert_eq!(json, "[\n  {\"id\":1,\"name\":\"Ann\",\"ok\":true,\"score\":null}\n]\n");
    let markdown = render(&export_settings(export::Format::Markdown), cols.clone(), &rows()[..1], target.clone());
    assert_eq!(markdown, "| id | name | ok | score |\n| --- | --- | --- | --- |\n| 1 | Ann | true |  |\n");
    let html = render(&export_settings(export::Format::Html), cols.clone(), &rows()[..1], target.clone());
    assert!(html.contains("<td>Ann</td>"), "{html}");
    let xml = render(&export_settings(export::Format::Xml), cols.clone(), &rows()[..1], target.clone());
    assert!(xml.contains("<value column=\"score\" null=\"true\"/>"), "{xml}");
    let insert = render(&export_settings(export::Format::Sql), cols, &rows(), target);
    assert_eq!(
        insert,
        "INSERT INTO `shop`.`people` (`id`, `name`, `ok`, `score`) VALUES\n\t(1, 'Ann', TRUE, NULL),\n\t(2, 'tab\there \"quoted\"\nline', FALSE, 1.5);\n"
    );
    assert_eq!(CopyFormat::SqlInsert.export(), Some(export::Format::Sql));
    assert_eq!(CopyFormat::Go.export(), None);
}

#[test]
fn in_lists_quote_names_and_values_in_the_dialect_and_drop_repeats() {
    let mut list = InList::new(SqlDialect::Postgres, &names(&["Name"]));
    for value in ["Ann", "O'Brien", "Ann"] {
        list.row(&[Value::Text(value.into())]);
    }
    assert_eq!(list.finish(), "\"Name\" IN ('Ann', 'O''Brien')");
    let mut pairs = InList::new(SqlDialect::MySql, &names(&["id", "kind"]));
    pairs.row(&[Value::Integer(1), Value::Text("a".into())]);
    pairs.row(&[Value::Integer(2), Value::Null]);
    assert_eq!(pairs.finish(), "(`id`, `kind`) IN ((1, 'a'), (2, NULL))");
}

fn code(format: CopyFormat) -> String {
    let list = names(&["id", "full name", "ok", "score"]);
    let mut known = CodeColumns::new(list.len());
    for row in rows() {
        known.learn(&row);
    }
    let writer = CodeWriter::new(format, &list, known);
    let mut out = writer.start();
    for row in rows() {
        out.push_str(&writer.row(&row));
    }
    out.push_str(&writer.finish());
    out
}

#[test]
fn code_literals_for_each_language() {
    assert_eq!(
        code(CopyFormat::Go),
        "[]map[string]any{\n\t{\"id\": 1, \"full name\": \"Ann\", \"ok\": true, \"score\": nil},\n\t{\"id\": 2, \"full name\": \"tab\\there \\\"quoted\\\"\\nline\", \"ok\": false, \"score\": 1.5},\n}\n"
    );
    assert_eq!(
        code(CopyFormat::Rust),
        "struct Row {\n    id: i64,\n    full_name: &'static str,\n    ok: bool,\n    score: Option<f64>,\n}\n\nlet rows = [\n    Row { id: 1, full_name: \"Ann\", ok: true, score: None },\n    Row { id: 2, full_name: \"tab\\there \\\"quoted\\\"\\nline\", ok: false, score: Some(1.5) },\n];\n"
    );
    assert_eq!(
        code(CopyFormat::TypeScript),
        "const rows = [\n  { id: 1, \"full name\": \"Ann\", ok: true, score: null },\n  { id: 2, \"full name\": \"tab\\there \\\"quoted\\\"\\nline\", ok: false, score: 1.5 },\n];\n"
    );
    assert_eq!(
        code(CopyFormat::Python),
        "rows = [\n    {\"id\": 1, \"full name\": \"Ann\", \"ok\": True, \"score\": None},\n    {\"id\": 2, \"full name\": \"tab\\there \\\"quoted\\\"\\nline\", \"ok\": False, \"score\": 1.5},\n]\n"
    );
    assert_eq!(
        code(CopyFormat::Kotlin),
        "val rows = listOf(\n    mapOf(\"id\" to 1, \"full name\" to \"Ann\", \"ok\" to true, \"score\" to null),\n    mapOf(\"id\" to 2, \"full name\" to \"tab\\there \\\"quoted\\\"\\nline\", \"ok\" to false, \"score\" to 1.5),\n)\n"
    );
}

#[test]
fn code_literals_escape_what_each_language_needs() {
    let list = names(&["type", "1st", "price"]);
    let row = vec![Value::Text("$x\u{1}é".into()), Value::Integer(9_007_199_254_740_993), Value::Decimal("10".into())];
    let mut known = CodeColumns::new(3);
    known.learn(&row);
    let rust = CodeWriter::new(CopyFormat::Rust, &list, known.clone());
    assert!(rust.start().contains("r#type: &'static str,\n    _1st: i64,\n    price: f64,"), "{}", rust.start());
    assert_eq!(rust.row(&row), "    Row { r#type: \"$x\\u{1}é\", _1st: 9007199254740993, price: 10.0 },\n");
    let kotlin = CodeWriter::new(CopyFormat::Kotlin, &list, known.clone());
    assert!(kotlin.row(&row).contains("\"\\$x\\u0001é\""), "{}", kotlin.row(&row));
    assert!(kotlin.row(&row).contains("9007199254740993L"), "{}", kotlin.row(&row));
    let typescript = CodeWriter::new(CopyFormat::TypeScript, &list, known.clone());
    assert!(typescript.row(&row).contains("\"1st\": 9007199254740993n"), "{}", typescript.row(&row));
    let python = CodeWriter::new(CopyFormat::Python, &list, known);
    assert!(python.row(&row).contains("\"$x\\x01é\""), "{}", python.row(&row));
    let json = vec![
        Value::Json("{\"a\": [1, 2]}".into()),
        Value::Bytes(vec![0, 255]),
        Value::Temporal(Typed { text: "2026-10-27".into(), type_name: "date".into() }),
    ];
    let mut known = CodeColumns::new(3);
    known.learn(&json);
    let typescript = CodeWriter::new(CopyFormat::TypeScript, &names(&["doc", "raw", "day"]), known.clone());
    assert_eq!(
        typescript.row(&json),
        "  { doc: {\"a\": [1, 2]}, raw: new Uint8Array([0, 255]), day: \"2026-10-27\" },\n"
    );
    let go = CodeWriter::new(CopyFormat::Go, &names(&["doc", "raw", "day"]), known.clone());
    assert!(go.row(&json).contains("[]byte{0x00, 0xff}"), "{}", go.row(&json));
    let kotlin = CodeWriter::new(CopyFormat::Kotlin, &names(&["doc", "raw", "day"]), known);
    assert!(kotlin.row(&json).contains("byteArrayOf(0, -1)"), "{}", kotlin.row(&json));
}

#[test]
fn excel_tsv_with_quotes_embedded_newlines_crlf_and_a_trailing_newline() {
    let text = "id\tnote\r\n1\t\"two\r\nlines\"\r\n2\t\"say \"\"hi\"\"\"\r\n";
    let table = parse(text, None, '"', usize::MAX);
    assert_eq!(table.delimiter, '\t');
    assert_eq!(texts(&table), [["id", "note"], ["1", "two\r\nlines"], ["2", "say \"hi\""]]);
    assert!(table.rows[1][1].quoted);
    assert!(names_columns(&table.rows[0], &names(&["ID", "Note", "other"])));
    assert!(!names_columns(&table.rows[1], &names(&["id", "note"])));
}

#[test]
fn csv_from_numbers_and_sheets_with_semicolons_cr_and_unicode() {
    let comma = parse("a,b\n\"x, y\",😀 ünï\n", None, '"', usize::MAX);
    assert_eq!(comma.delimiter, ',');
    assert_eq!(texts(&comma), [["a", "b"], ["x, y", "😀 ünï"]]);
    let semicolon = parse("\u{feff}a;b;c\r1,5;2;\"\"\r", None, '"', usize::MAX);
    assert_eq!(semicolon.delimiter, ';', "the same number on every line wins over commas inside values");
    assert_eq!(texts(&semicolon), [["a", "b", "c"], ["1,5", "2", ""]]);
    assert!(semicolon.rows[1][2].quoted, "a quoted empty field is empty text");
    assert_eq!(semicolon.rows[1][2].value(""), Some(""));
    let single = parse("one\ntwo", None, '"', usize::MAX);
    assert_eq!(texts(&single), [["one"], ["two"]], "no delimiter: one column");
    let blank = parse("a\t\n\tb\n", None, '"', usize::MAX);
    assert_eq!(texts(&blank), [["a", ""], ["", "b"]], "empty cells stay in place");
    assert_eq!(parse("", None, '"', usize::MAX).rows.len(), 0);
    assert_eq!(parse("1\n2\n3\n", None, '"', 2).rows.len(), 2, "at most the rows asked for");
    let nulls = parse("NULL\t\"NULL\"\n", Some('\t'), '"', usize::MAX);
    assert_eq!(nulls.rows[0][0].value("NULL"), None);
    assert_eq!(nulls.rows[0][1].value("NULL"), Some("NULL"), "a quoted NULL text is text");
    assert_eq!(nulls.width(), 2);
}
