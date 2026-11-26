use super::*;
use crate::value::Typed;

fn columns() -> Vec<Column> {
    vec![
        Column::new("id", "int8"),
        Column::new("name", "text"),
        Column::new("data", "bytea"),
        Column::new("at", "timestamptz"),
        Column::new("ok", "bool"),
    ]
}

fn rows() -> Vec<Vec<Value>> {
    vec![
        vec![
            Value::Integer(1),
            Value::Text("Ann, \"the\" first\nline".into()),
            Value::Bytes(vec![0, 1, 0xff]),
            Value::Temporal(Typed { text: "2026-10-27 09:30:00+00".into(), type_name: "timestamptz".into() }),
            Value::Bool(true),
        ],
        vec![Value::Integer(2), Value::Text("=SUM(A1)".into()), Value::Null, Value::Null, Value::Bool(false)],
        vec![
            Value::Integer(3),
            Value::Text("Ünïcødé ✓ 日本".into()),
            Value::Bytes(Vec::new()),
            Value::Null,
            Value::Null,
        ],
        vec![Value::Integer(4), Value::Text(String::new()), Value::Null, Value::Null, Value::Null],
    ]
}

fn target() -> Target {
    Target { dialect: SqlDialect::Postgres, table: "public.people".into(), title: "people".into() }
}

fn of(settings: Settings) -> String {
    render(&settings, columns(), &rows(), target())
}

#[test]
fn csv_quotes_what_needs_it_escapes_formulas_and_tells_null_from_empty() {
    let text = of(Settings::default());
    assert_eq!(
        text,
        "id,name,data,at,ok\n\
         1,\"Ann, \"\"the\"\" first\nline\",\\x0001ff,2026-10-27 09:30:00+00,true\n\
         2,'=SUM(A1),,,false\n\
         3,Ünïcødé ✓ 日本,\\x,,\n\
         4,\"\",,,\n"
    );
    let plain = of(Settings { escape_formulas: false, null_text: "NULL".into(), header: false, ..Settings::default() });
    assert!(plain.starts_with("1,"), "{plain}");
    assert!(plain.contains("2,=SUM(A1),NULL,NULL,false\n"), "{plain}");
    assert!(plain.contains("4,,NULL,NULL,NULL\n"), "an empty text needs no quotes once NULL has its own text: {plain}");
    let quoted = render(
        &Settings { quote_all: true, null_text: "NULL".into(), ..Settings::default() },
        vec![Column::new("a", "text")],
        &[vec![Value::Text("NULL".into())], vec![Value::Null]],
        target(),
    );
    assert_eq!(quoted, "\"a\"\n\"NULL\"\nNULL\n", "the text NULL stays apart from a NULL");
}

#[test]
fn tsv_and_semicolons_and_crlf() {
    let text = of(Settings { format: Format::Tsv, line_ending: LineEnding::CrLf, ..Settings::default() });
    assert!(text.starts_with("id\tname\tdata\tat\tok\r\n"), "{text}");
    assert!(text.contains("3\tÜnïcødé ✓ 日本\t\\x\t\t\r\n"), "{text}");
    let semi = of(Settings { delimiter: ";".into(), quote: '\'', escape_formulas: false, ..Settings::default() });
    assert!(semi.contains("1;'Ann, \"the\" first\nline';"), "{semi}");
    assert_eq!(Settings { delimiter: "\"".into(), ..Settings::default() }.problem(), Some("delimiterQuote"));
    assert_eq!(Settings { delimiter: String::new(), ..Settings::default() }.problem(), Some("emptyDelimiter"));
}

#[test]
fn json_as_an_array_or_lines_keeps_types() {
    let text = of(Settings { format: Format::Json, ..Settings::default() });
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 4);
    assert_eq!(parsed[0]["id"], 1);
    assert_eq!(parsed[0]["name"], "Ann, \"the\" first\nline");
    assert_eq!(parsed[0]["data"], "\\x0001ff");
    assert_eq!(parsed[0]["at"], "2026-10-27 09:30:00+00");
    assert_eq!(parsed[0]["ok"], true);
    assert_eq!(parsed[1]["data"], serde_json::Value::Null);
    assert_eq!(parsed[2]["name"], "Ünïcødé ✓ 日本");
    let lines = of(Settings { format: Format::Json, json: JsonStyle::Lines, ..Settings::default() });
    assert_eq!(lines.lines().count(), 4);
    for line in lines.lines() {
        serde_json::from_str::<serde_json::Value>(line).unwrap();
    }
    let empty = render(&Settings { format: Format::Json, ..Settings::default() }, columns(), &[], target());
    assert_eq!(serde_json::from_str::<serde_json::Value>(&empty).unwrap(), serde_json::json!([]));
    assert_eq!(json_value(&Value::Json("{\"a\": [1, 2]}".into())), "{\"a\": [1, 2]}");
    assert_eq!(json_value(&Value::Json("not json".into())), "\"not json\"");
    assert_eq!(json_value(&Value::Float(2.0)), "2.0");
    assert_eq!(json_value(&Value::Float(f64::NAN)), "\"NaN\"");
    assert_eq!(json_value(&Value::Decimal("12345678901234567890.123".into())), "12345678901234567890.123");
    assert_eq!(
        json_value(&Value::Document(vec![
            ("n".into(), Value::Integer(1)),
            ("t".into(), Value::Array(vec![Value::Null]))
        ])),
        "{\"n\":1,\"t\":[null]}"
    );
}

#[test]
fn sql_inserts_in_batches_in_the_dialect() {
    let text = of(Settings { format: Format::Sql, batch: 3, ..Settings::default() });
    assert_eq!(
        text,
        "INSERT INTO public.people (\"id\", \"name\", \"data\", \"at\", \"ok\") VALUES\n\
         \t(1, 'Ann, \"the\" first\nline', '\\x0001ff', '2026-10-27 09:30:00+00', TRUE),\n\
         \t(2, '=SUM(A1)', NULL, NULL, FALSE),\n\
         \t(3, 'Ünïcødé ✓ 日本', '\\x', NULL, NULL);\n\
         INSERT INTO public.people (\"id\", \"name\", \"data\", \"at\", \"ok\") VALUES\n\
         \t(4, '', NULL, NULL, NULL);\n"
    );
    let mysql = render(
        &Settings { format: Format::Sql, table: Some("`copy`".into()), ..Settings::default() },
        vec![Column::new("select", "text"), Column::new("b", "blob")],
        &[vec![Value::Text("it's \\ here".into()), Value::Bytes(vec![1])]],
        Target { dialect: SqlDialect::MySql, ..target() },
    );
    assert_eq!(mysql, "INSERT INTO `copy` (`select`, `b`) VALUES\n\t(_utf8mb4 X'69742773205c2068657265', X'01');\n");
}

#[test]
fn markdown_html_and_xml_escape_their_markup() {
    let markdown = render(
        &Settings { format: Format::Markdown, null_text: "NULL".into(), ..Settings::default() },
        vec![Column::new("a|b", "text"), Column::new("c", "text")],
        &[vec![Value::Text("x | y\nz".into()), Value::Null]],
        target(),
    );
    assert_eq!(markdown, "| a\\|b | c |\n| --- | --- |\n| x \\| y<br>z | NULL |\n");
    let html = render(
        &Settings { format: Format::Html, ..Settings::default() },
        vec![Column::new("<a>", "text")],
        &[vec![Value::Text("Tom & \"Jerry\" <b>".into())], vec![Value::Null]],
        Target { title: "t<1>".into(), ..target() },
    );
    assert!(html.contains("<title>t&lt;1&gt;</title>"), "{html}");
    assert!(html.contains("<th>&lt;a&gt;</th>"), "{html}");
    assert!(html.contains("<td>Tom &amp; &quot;Jerry&quot; &lt;b&gt;</td>"), "{html}");
    assert!(html.contains("<tr><td></td></tr>") && html.ends_with("</html>\n"), "{html}");
    let xml = render(
        &Settings { format: Format::Xml, ..Settings::default() },
        vec![Column::new("n\"ame", "text"), Column::new("v", "text")],
        &[vec![Value::Text("a<b>&\u{1}ü".into()), Value::Null]],
        target(),
    );
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<export name=\"people\">\n"), "{xml}");
    assert!(xml.contains("<value column=\"n&quot;ame\">a&lt;b&gt;&amp;\u{fffd}ü</value>"), "{xml}");
    assert!(xml.contains("<value column=\"v\" null=\"true\"/>") && xml.ends_with("</export>\n"), "{xml}");
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(parsed.root_element().children().filter(roxmltree::Node::is_element).count(), 1);
}

#[test]
fn a_bom_comes_first_and_extensions_follow_the_format() {
    let text = of(Settings { bom: true, ..Settings::default() });
    assert!(text.starts_with('\u{feff}'));
    let lines = Settings { json: JsonStyle::Lines, ..Settings::default() };
    assert_eq!(Format::Json.extension(&lines), "jsonl");
    assert_eq!(Format::Markdown.extension(&Settings::default()), "md");
    assert_eq!(Format::ALL.len(), 8);
    assert_eq!(Format::Parquet.extension(&Settings::default()), "parquet");
    assert!(Format::Parquet.binary() && !Format::Csv.binary());
}

#[test]
fn copy_options_follow_the_settings_and_refuse_what_copy_cannot_write() {
    let options = Settings::default().copy().unwrap();
    assert_eq!(
        options.statement("SELECT 1 -- one\n;"),
        "COPY (\nSELECT 1 -- one\n) TO STDOUT WITH (FORMAT csv, DELIMITER ',', QUOTE '\"', ESCAPE '\"', HEADER true, NULL '', ENCODING 'UTF8')"
    );
    let tsv = Settings { format: Format::Tsv, quote_all: true, ..Settings::default() }.copy().unwrap();
    assert_eq!(tsv.delimiter, '\t');
    assert!(tsv.statement("SELECT 1").ends_with(", FORCE_QUOTE *)"));
    assert!(Settings { delimiter: "::".into(), ..Settings::default() }.copy().is_none());
    assert!(Settings { null_text: "a,b".into(), ..Settings::default() }.copy().is_none());
    assert!(Settings { server_copy: false, ..Settings::default() }.copy().is_none());
    assert!(Settings { format: Format::Json, ..Settings::default() }.copy().is_none());
}

#[test]
fn the_copy_rewriter_escapes_formulas_of_text_columns_across_chunks_and_ends_lines() {
    let options = Settings::default().copy().unwrap();
    let input = "n,t\n-1,=A1\n2,\"+x,\"\"y\"\"\nz\"\n3,\"\"\n4,@me\n".as_bytes();
    let mut whole = Vec::new();
    CopyRewriter::new(&options, vec![false, true], true).feed(input, &mut whole);
    let expected = "n,t\r\n-1,'=A1\r\n2,\"'+x,\"\"y\"\"\r\nz\"\r\n3,\"\"\r\n4,'@me\r\n";
    assert_eq!(String::from_utf8(whole.clone()).unwrap(), expected.replace("\"\"y\"\"\r\nz", "\"\"y\"\"\nz"));
    for size in 1..input.len() {
        let mut rewriter = CopyRewriter::new(&options, vec![false, true], true);
        let mut out = Vec::new();
        for chunk in input.chunks(size) {
            rewriter.feed(chunk, &mut out);
        }
        assert_eq!(out, whole, "chunks of {size}");
    }
    let mut counted = CopyRewriter::new(&options, vec![false, true], false);
    counted.feed(input, &mut Vec::new());
    assert_eq!(counted.records(), 4, "the header and a line break inside quotes are not records");
    assert!(text_type("varchar") && !text_type("_text") && !text_type("int4"));
}

#[test]
fn file_names_come_from_the_pattern_and_never_leave_the_folder() {
    let values = [("table", "public.orders"), ("timestamp", "20261027093000"), ("connection", "Shop/DB")];
    assert_eq!(file_name(PATTERN, &values, "csv"), "public.orders_20261027093000.csv");
    assert_eq!(file_name("${connection}-${table}", &values, "json"), "Shop_DB-public.orders.json");
    assert_eq!(file_name("../../etc/passwd", &values, "csv"), "_.._etc_passwd.csv");
    assert_eq!(file_name("", &values, "md"), "public.orders_20261027093000.md");
    assert_eq!(file_name("report.CSV", &values, "csv"), "report.CSV");
    assert_eq!(file_name("...", &values, "xml"), "export.xml");
}
