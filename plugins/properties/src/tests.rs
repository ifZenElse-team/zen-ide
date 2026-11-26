use super::*;
use Kind::{Comment, Constant, Property, Punctuation, String};

fn kinds(text: &str) -> Vec<(&str, Kind)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

fn assert_well_formed(text: &str) {
    let mut last = 0;
    for span in highlight(text) {
        assert!(span.start >= last && span.end > span.start && span.end <= text.len(), "{text:?}: {span:?}");
        assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end), "{text:?}: {span:?}");
        last = span.end;
    }
    for problem in problems(text) {
        assert!(problem.start < problem.end && problem.end <= text.len(), "{text:?}: {problem:?}");
    }
    for inlay in inlays(text) {
        assert!(inlay.offset <= text.len() && text.is_char_boundary(inlay.offset), "{text:?}: {inlay:?}");
    }
    let _ = fold(text);
    let _ = key_prefix_at(text, text.len());
}

#[test]
fn reads_every_separator_as_java_util_properties_does() {
    let text = "a=1\nb : 2\nc 3\nd\te\nf\n  g = spaced value \n";
    let found: Vec<(std::string::String, std::string::String)> =
        entries(text).into_iter().map(|entry| (entry.key, entry.value)).collect();
    let pairs: Vec<(&str, &str)> = found.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    assert_eq!(pairs, [("a", "1"), ("b", "2"), ("c", "3"), ("d", "e"), ("f", ""), ("g", "spaced value ")]);
}

#[test]
fn a_continued_line_joins_without_its_leading_white_space() {
    let text = "fruits = apple, \\\n         banana, \\\n   pear\nnext = 1\n";
    let found = entries(text);
    assert_eq!(found[0].value, "apple, banana, pear");
    assert_eq!((found[0].first_line, found[0].last_line), (0, 2));
    assert_eq!((found[1].key.as_str(), found[1].first_line), ("next", 3));
    assert_eq!(fold(text), [Fold { start_line: 0, end_line: 2 }]);
}

#[test]
fn an_even_number_of_backslashes_does_not_continue_the_line() {
    let found = entries("path = c:\\\\\nnext = 1\n");
    assert_eq!(found[0].value, "c:\\");
    assert_eq!(found[1].key, "next");
}

#[test]
fn comment_lines_never_continue_and_fold_as_a_block() {
    let text = "# one \\\n! two\n#three\nkey = v\n\n# alone\n";
    assert_eq!(entries(text).len(), 1);
    assert_eq!(fold(text), [Fold { start_line: 0, end_line: 2 }]);
    assert!(kinds(text).contains(&("! two", Comment)));
}

#[test]
fn escapes_decode_in_keys_and_values() {
    let text = "my\\ key\\=x = \\u041F\\u0440\\u0438\\u0432\\u0435\\u0442\\t\\n\\q\n";
    let found = entries(text);
    assert_eq!(found[0].key, "my key=x");
    assert_eq!(found[0].value, "Привет\t\nq");
    let spans = kinds(text);
    assert!(spans.contains(&("my", Property)), "{spans:?}");
    assert!(spans.contains(&("\\ ", Constant)), "{spans:?}");
    assert!(spans.contains(&("=", Punctuation)), "{spans:?}");
    assert!(spans.contains(&("\\u041F\\u0440\\u0438\\u0432\\u0435\\u0442\\t\\n\\q", Constant)), "{spans:?}");
}

#[test]
fn highlights_keys_separators_values_and_comments() {
    let text = "# Messages\ngreeting=Hello, {0}!\nempty:\n";
    assert_eq!(
        kinds(text),
        [
            ("# Messages", Comment),
            ("greeting", Property),
            ("=", Punctuation),
            ("Hello, {0}!", String),
            ("empty", Property),
            (":", Punctuation),
        ]
    );
}

#[test]
fn duplicate_keys_are_warnings_on_every_occurrence() {
    let text = "a=1\nb=2\na=3\n";
    let found = problems(text);
    assert_eq!(found.len(), 2);
    assert!(
        found.iter().all(|problem| problem.severity == Severity::Warning && &text[problem.start..problem.end] == "a")
    );
    assert_eq!(found[0].message, "Duplicate key 'a'");
}

#[test]
fn a_malformed_unicode_escape_is_an_error() {
    let text = "a = \\u12G4 ok\n";
    let found = problems(text);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(&text[found[0].start..found[0].end], "\\u12G4");
}

#[test]
fn the_unescaped_value_shows_after_a_value_with_escapes_only() {
    let text = "plain = Hello\nru = \\u041C\\u0438\\u0440\nmulti = a \\\n  b\n";
    let shown = inlays(text);
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert_eq!(shown[0].text, "= Мир");
    assert_eq!(&text[..shown[0].offset], "plain = Hello\nru = \\u041C\\u0438\\u0440");
    assert_eq!(shown[1].text, "= a b");
    assert_eq!(inlays_json(&shown[..1]), format!("[{{\"offset\":{},\"text\":\"= Мир\"}}]", shown[0].offset));
}

#[test]
fn the_outline_lists_keys_in_order() {
    let json = outline_json(&entries("b=1\na.b=2\n"));
    assert_eq!(
        json,
        "[{\"name\":\"b\",\"kind\":\"property\",\"start\":0,\"end\":3,\"children\":[]},{\"name\":\"a.b\",\"kind\":\"property\",\"start\":4,\"end\":9,\"children\":[]}]"
    );
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 2);
}

#[test]
fn another_plug_in_completes_keys_it_knows_where_a_key_is_typed() {
    let keys = [("server.port", "int"), ("server.address", "address"), ("spring.application.name", "string")];
    let text = "server.port=8080\nser";
    let found = complete_keys(text, text.len(), &keys).unwrap();
    assert_eq!(found.from, 17);
    assert_eq!(
        found.items,
        [Item { label: "server.address".into(), detail: "address".into() }],
        "present keys are left out"
    );
    assert_eq!(complete_keys("a = ser", 7, &keys), None, "not in a value");
    assert_eq!(complete_keys("# ser", 5, &keys), None, "not in a comment");
}

#[test]
fn keys_and_values_escape_so_that_they_read_back() {
    for (key, value) in [("a b", " lead"), ("#x", "back\\slash"), ("k=:", "tab\tnew\nline"), ("Ключ", "Значение")]
    {
        let line = format!("{} = {}\n", escape_key(key), escape_value(value));
        let found = entries(&line);
        assert_eq!((found[0].key.as_str(), found[0].value.as_str()), (key, value), "{line:?}");
    }
}

#[test]
fn survives_any_input() {
    let pieces = ["\\", "\\u", "\\u00", "=", ":", " ", "#", "!", "\n", "\r", "\r\n", "a", "é", "日", "\u{1F600}", "\t"];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                assert_well_formed(&format!("{a}{b}{c}"));
                assert_well_formed(&format!("{a}{b}\n{c}x"));
            }
        }
    }
}

#[test]
fn encodes_records() {
    assert_eq!(encode_spans(&[Span { start: 1, end: 3, kind: Punctuation }]), [1, 0, 0, 0, 3, 0, 0, 0, 11, 0, 0, 0]);
    assert_eq!(encode_folds(&[Fold { start_line: 1, end_line: 2 }]), [1, 0, 0, 0, 2, 0, 0, 0]);
    let json = problems_json(&problems("a=1\na=2\n"));
    assert!(json.starts_with("[{\"start\":0,\"end\":1,\"severity\":\"warning\""), "{json}");
}

#[test]
fn declares_the_calls_of_java_and_kotlin_that_name_keys_and_bundles() {
    let declared: serde_json::Value = serde_json::from_str(KEY_REFERENCES).unwrap();
    assert_eq!(declared[0]["files"][0], "*.java");
    assert!(declared[0]["keys"].as_array().unwrap().contains(&"getString".into()));
    assert_eq!(declared[0]["bundles"][0], "getBundle");
}
