use super::*;

fn records(text: &str, format: &CsvFormat) -> Vec<Record> {
    let mut reader = CsvReader::new(text.as_bytes(), format);
    let mut all = Vec::new();
    while let Some(record) = reader.next_record().unwrap() {
        all.push(record);
    }
    all
}

fn texts(record: &Record) -> Vec<&str> {
    record.fields.iter().map(|field| field.text.as_str()).collect()
}

#[test]
fn records_keep_quotes_line_breaks_and_their_starting_lines() {
    let text = "\u{feff}id,note\r\n1,\"a, \"\"quoted\"\" note\"\r\n\r\n2,\"two\nlines\"\n3,\n";
    let read = records(text, &CsvFormat::default());
    assert_eq!(read.len(), 4);
    assert_eq!(texts(&read[0]), ["id", "note"]);
    assert_eq!(texts(&read[1]), ["1", "a, \"quoted\" note"]);
    assert_eq!((texts(&read[2]), read[2].line), (vec!["2", "two\nlines"], 4));
    assert_eq!((texts(&read[3]), read[3].line), (vec!["3", ""], 6));
    let format = CsvFormat::default();
    assert_eq!(read[3].value(1, &format), None, "an unquoted empty field is NULL");
    let quoted = records("1,\"\"\n", &format);
    assert_eq!(quoted[0].value(1, &format), Some(""), "a quoted empty field is empty text");
}

#[test]
fn a_backslash_escape_tabs_and_null_text_are_read() {
    let format = CsvFormat {
        delimiter: '\t',
        quote: '\'',
        escape: Some('\\'),
        null_text: "\\N".to_owned(),
        ..CsvFormat::default()
    };
    let read = records("1\t'it\\'s'\t\\N\n", &format);
    assert_eq!(texts(&read[0]), ["1", "it's", "\\N"]);
    assert_eq!(read[0].value(2, &format), None);
    assert_eq!(CsvFormat { delimiter: '"', ..CsvFormat::default() }.problem(), Some("delimiterQuote"));
    assert_eq!(CsvFormat { quote: '\n', ..CsvFormat::default() }.problem(), Some("lineBreak"));
}

#[test]
fn fields_convert_for_their_column_and_bad_ones_say_why() {
    assert_eq!(convert(Kind::Integer, Some(" +42 "), None), Ok(Cell::Raw("42".to_owned())));
    assert_eq!(convert(Kind::Integer, Some("4.2"), None), Err(Unconverted { code: "notANumber" }));
    assert_eq!(convert(Kind::Integer, Some(""), None), Ok(Cell::Null));
    assert_eq!(convert(Kind::Decimal, Some("-1.5e3"), None), Ok(Cell::Raw("-1.5e3".to_owned())));
    assert_eq!(convert(Kind::Decimal, Some("inf"), None), Err(Unconverted { code: "notANumber" }));
    assert_eq!(convert(Kind::Boolean, Some("Yes"), None), Ok(Cell::Raw("TRUE".to_owned())));
    assert_eq!(convert(Kind::Boolean, Some("maybe"), None), Err(Unconverted { code: "notABoolean" }));
    assert_eq!(convert(Kind::Text, Some(""), None), Ok(Cell::Text(String::new())));
    assert_eq!(convert(Kind::Text, None, None), Ok(Cell::Null));
    let pattern = java_pattern("dd.MM.yyyy HH:mm");
    assert_eq!(pattern, "%d.%m.%Y %H:%M");
    assert_eq!(
        convert(Kind::Timestamp, Some("27.10.2026 09:30"), Some(&pattern)),
        Ok(Cell::Text("2026-10-27 09:30:00".to_owned()))
    );
    assert_eq!(convert(Kind::Date, Some("27.10.2026"), Some("%d.%m.%Y")), Ok(Cell::Text("2026-10-27".to_owned())));
    assert_eq!(convert(Kind::Date, Some("2026-13-01"), Some("%Y-%m-%d")), Err(Unconverted { code: "notADate" }));
    assert_eq!(convert(Kind::Timestamp, Some("whenever"), None), Ok(Cell::Text("whenever".to_owned())));
    assert_eq!(java_pattern("yyyy-MM-dd'T'HH:mm:ss.SSS"), "%Y-%m-%dT%H:%M:%S%.f");
    assert_eq!(
        convert(Kind::Timestamp, Some("2026-10-27T09:30:00.250"), Some(&java_pattern("yyyy-MM-dd'T'HH:mm:ss.SSS"))),
        Ok(Cell::Text("2026-10-27 09:30:00.25".to_owned()))
    );
}

#[test]
fn kinds_are_guessed_from_the_sample_and_from_type_names() {
    assert_eq!(guess(["1", "-20", ""], None), Kind::Integer);
    assert_eq!(guess(["1", "2.5"], None), Kind::Decimal);
    assert_eq!(guess(["true", "F"], None), Kind::Boolean);
    assert_eq!(guess(["1", "0"], None), Kind::Integer, "ones and zeros stay numbers");
    assert_eq!(guess(["2026-10-27"], None), Kind::Date);
    assert_eq!(guess(["2026-10-27 09:30:00", "2026-10-27T10:00"], None), Kind::Timestamp);
    assert_eq!(guess(["27.10.2026 09:30"], Some("%d.%m.%Y %H:%M")), Kind::Timestamp);
    assert_eq!(guess(["a", "1"], None), Kind::Text);
    assert_eq!(guess(Vec::<&str>::new(), None), Kind::Text);
    assert_eq!(Kind::of_type("bigint"), Kind::Integer);
    assert_eq!(Kind::of_type("Nullable(UInt32)"), Kind::Integer);
    assert_eq!(Kind::of_type("numeric(10,2)"), Kind::Decimal);
    assert_eq!(Kind::of_type("timestamp with time zone"), Kind::Timestamp);
    assert_eq!(Kind::of_type("tinyint(1)"), Kind::Integer);
    assert_eq!(Kind::of_type("interval"), Kind::Text);
    assert_eq!(Kind::of_type("jsonb"), Kind::Text);
}

#[test]
fn a_sample_reads_at_most_its_rows_and_names_every_column() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("people.csv");
    let mut text = String::from("id,,id\n");
    for at in 0..150 {
        text.push_str(&format!("{at},name {at},\n"));
    }
    std::fs::write(&path, &text).unwrap();
    let read = sample(&path, &CsvFormat::default(), 500).unwrap();
    assert_eq!(read.columns, ["id", "column2", "id_2"]);
    assert_eq!(read.kinds, [Kind::Integer, Kind::Text, Kind::Text]);
    assert_eq!(read.rows.len(), SAMPLE_LIMIT);
    assert!(read.more);
    assert_eq!(read.rows[3], [Some("3".to_owned()), Some("name 3".to_owned()), None]);
    assert_eq!(read.file_bytes, text.len() as u64);
    let headless = sample(&path, &CsvFormat { header: false, ..CsvFormat::default() }, 2).unwrap();
    assert_eq!(headless.columns, ["column1", "column2", "column3"]);
    assert_eq!(headless.rows[0][0].as_deref(), Some("id"));
}

#[test]
fn statements_are_written_in_each_dialect() {
    let columns = vec!["id".to_owned(), "note".to_owned()];
    let rows = vec![
        vec![Cell::Raw("1".to_owned()), Cell::Text("it's".to_owned())],
        vec![Cell::Raw("2".to_owned()), Cell::Null],
    ];
    assert_eq!(
        insert_statement(SqlDialect::Postgres, "\"public\".\"t\"", &columns, &rows),
        "INSERT INTO \"public\".\"t\" (\"id\", \"note\") VALUES\n(1, 'it''s'),\n(2, NULL)"
    );
    assert_eq!(
        insert_statement(SqlDialect::MySql, "`t`", &columns, &rows[..1]),
        "INSERT INTO `t` (`id`, `note`) VALUES\n(1, 'it''s')"
    );
    assert_eq!(
        copy_statement("\"t\"", &columns),
        "COPY \"t\" (\"id\", \"note\") FROM STDIN WITH (FORMAT csv, ENCODING 'UTF8')"
    );
    let text_rows = vec![vec![Cell::Text(String::new()), Cell::Null, Cell::Text("a \"b\"\nc".to_owned())]];
    assert_eq!(copy_rows(&text_rows), b"\"\",,\"a \"\"b\"\"\nc\"\n");
    let defined = vec![("id".to_owned(), "bigint".to_owned()), ("note".to_owned(), "text".to_owned())];
    assert_eq!(
        create_statement(SqlDialect::Postgres, "\"t\"", &defined),
        "CREATE TABLE \"t\" (\n    \"id\" bigint,\n    \"note\" text\n)"
    );
    assert!(create_statement(SqlDialect::ClickHouse, "`t`", &defined).ends_with("ENGINE = MergeTree ORDER BY tuple()"));
    assert_eq!(truncate_statement(SqlDialect::Sqlite, "\"t\""), "DELETE FROM \"t\"");
    assert_eq!(truncate_statement(SqlDialect::MariaDb, "`t`"), "TRUNCATE TABLE `t`");
    assert_eq!(Kind::Timestamp.sql_type(SqlDialect::MySql), "DATETIME(6)");
}

#[test]
fn a_column_type_is_a_type_and_nothing_more() {
    for good in
        ["text", "varchar(80)", "numeric(10, 2)", "timestamp with time zone", "Nullable(String)", "pg_catalog.int8"]
    {
        assert!(plain_sql_type(good), "{good}");
    }
    for bad in ["int); DROP TABLE users; --", "int -- x", "text /* */", "varchar('x')", "int)", "(int", "", "a;b"] {
        assert!(!plain_sql_type(bad), "{bad}");
    }
}
