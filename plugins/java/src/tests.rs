use super::*;
use crate::lexer::{KEYWORDS, Raw, State, lex_line};

fn kinds(text: &str) -> Vec<(&str, Kind)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

fn assert_contains(text: &str, expected: &[(&str, Kind)]) {
    let found = kinds(text);
    for item in expected {
        assert!(found.contains(item), "{item:?} in {found:?}");
    }
}

fn assert_plain(text: &str, words: &[&str]) {
    let found = kinds(text);
    for word in words {
        assert!(!found.iter().any(|(text, _)| text == word), "{word} stays plain in {found:?}");
    }
}

#[test]
fn keywords_types_methods_constants_and_annotations() {
    let text = "\
package com.acme.shop;

import java.util.List;
import static java.util.Objects.requireNonNull;

@SuppressWarnings(\"unchecked\")
public final class Cart<T extends Item> implements Iterable<T> {
    private static final int MAX_ITEMS = 0x1F_FF;
    private final List<T> items = new ArrayList<>();
    @java.lang.Override
    public Iterator<T> iterator() { return items.iterator(); }
    long total(double rate) { return Math.round(rate * 1.5e3d) + 10L; }
}
";
    assert_contains(
        text,
        &[
            ("package", Kind::Keyword),
            ("import", Kind::Keyword),
            ("static", Kind::Keyword),
            ("@", Kind::Attribute),
            ("SuppressWarnings", Kind::Attribute),
            ("\"unchecked\"", Kind::String),
            ("public", Kind::Keyword),
            ("final", Kind::Keyword),
            ("class", Kind::Keyword),
            ("Cart", Kind::Type),
            ("T", Kind::Type),
            ("extends", Kind::Keyword),
            ("Item", Kind::Type),
            ("Iterable", Kind::Type),
            ("int", Kind::Keyword),
            ("MAX_ITEMS", Kind::Constant),
            ("0x1F_FF", Kind::Number),
            ("ArrayList", Kind::Type),
            ("Override", Kind::Attribute),
            ("lang", Kind::Attribute),
            ("iterator", Kind::Function),
            ("total", Kind::Function),
            ("round", Kind::Function),
            ("1.5e3d", Kind::Number),
            ("10L", Kind::Number),
        ],
    );
    assert_plain(text, &["items", "rate", "acme", "shop"]);
}

#[test]
fn records_sealed_types_switch_patterns_and_var() {
    let text = "\
public sealed interface Shape permits Circle, Square {}
public record Circle(double radius) implements Shape {}
public non-sealed class Square implements Shape {}
class Area {
    static double of(Shape shape) {
        var label = \"area\";
        return switch (shape) {
            case Circle c when c.radius() > 0 -> Math.PI * c.radius();
            case Square s -> { yield 1; }
            default -> 0;
        };
    }
    int record = 1;
    int when = 2;
    int yield(int var) { return var; }
}
";
    assert_contains(
        text,
        &[
            ("sealed", Kind::Keyword),
            ("permits", Kind::Keyword),
            ("record", Kind::Keyword),
            ("non", Kind::Keyword),
            ("Circle", Kind::Type),
            ("var", Kind::Keyword),
            ("when", Kind::Keyword),
            ("yield", Kind::Keyword),
            ("PI", Kind::Constant),
        ],
    );
    let lines: Vec<&str> = text.lines().collect();
    for line in [lines[12], lines[13]] {
        let found = kinds(line);
        assert!(
            !found.iter().any(|(word, kind)| matches!(*word, "record" | "when") && *kind == Kind::Keyword),
            "{line}: {found:?}"
        );
    }
    let found = kinds(lines[14]);
    assert!(found.contains(&("yield", Kind::Function)), "{found:?}");
}

#[test]
fn text_blocks_comments_and_characters_span_lines() {
    let text = "\
/**
 * A doc comment.
 */
String query = \"\"\"
    SELECT * FROM t WHERE a = \"x\" /* not a comment */
    \"\"\";
char c = '\\'';
/* block
   comment */ int after = 1;
";
    let found = kinds(text);
    assert!(found.contains(&("/**", Kind::Doc)) || found.iter().any(|(t, k)| t.starts_with("/**") && *k == Kind::Doc));
    assert!(found.iter().any(|(t, k)| t.contains("SELECT") && *k == Kind::String), "{found:?}");
    assert!(found.iter().any(|(t, k)| t.contains("/* not a comment */") && *k == Kind::String), "{found:?}");
    assert!(found.contains(&("'\\''", Kind::String)), "{found:?}");
    assert!(found.contains(&("int", Kind::Keyword)), "code after a closed block comment: {found:?}");
    let (_, state) = lex_line("String q = \"\"\"", State::Code);
    assert_eq!(state, State::TextBlock);
    let (tokens, state) = lex_line("  \"\"\"; int x;", state);
    assert_eq!(state, State::Code);
    assert_eq!(tokens[0].raw, Raw::String);
    let (_, state) = lex_line("/** open", State::Code);
    assert_eq!(state, State::Comment(true));
}

#[test]
fn module_info_words_are_keywords_only_there() {
    let module = "\
open module com.acme.shop {
    requires transitive java.sql;
    exports com.acme.shop.api to com.acme.web;
    uses com.acme.spi.Plugin;
    provides com.acme.spi.Plugin with com.acme.shop.Impl;
}
";
    assert_contains(
        module,
        &[
            ("open", Kind::Keyword),
            ("module", Kind::Keyword),
            ("requires", Kind::Keyword),
            ("transitive", Kind::Keyword),
            ("exports", Kind::Keyword),
            ("to", Kind::Keyword),
            ("uses", Kind::Keyword),
            ("provides", Kind::Keyword),
            ("with", Kind::Keyword),
        ],
    );
    assert_plain("class A { int module; void to() { requires(); } }", &["module"]);
    assert_eq!(outline(module)[0].name, "com.acme.shop");
    assert_eq!(outline(module)[0].kind, SymbolKind::Module);
}

fn tree(symbols: &[Symbol]) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(symbols: &[Symbol], depth: usize, out: &mut Vec<String>) {
        for symbol in symbols {
            out.push(format!("{}{} {:?}", "  ".repeat(depth), symbol.name, symbol.kind));
            walk(&symbol.children, depth + 1, out);
        }
    }
    walk(symbols, 0, &mut out);
    out
}

#[test]
fn the_structure_has_types_members_and_nested_types() {
    let text = "\
package com.acme;

import java.util.*;

/** Orders. */
@Entity
public class Order extends Base implements Comparable<Order> {
    public static final String TABLE = \"orders\", ALIAS = \"o\";
    private final Map<String, List<Line>> lines = new HashMap<>();
    private int count, size = 3;
    private final Runnable task = () -> { run(); };

    public Order() { this(0); }
    Order(int count) { this.count = count; }

    @Override
    public int compareTo(Order other) { return 0; }

    abstract void close();

    static {
        init();
    }

    enum Status { NEW, PAID(1) { int code() { return 2; } }, SHIPPED; Status() {} Status(int c) {} int c; }

    record Line(String sku, int quantity) {
        Line { if (quantity < 0) throw new IllegalArgumentException(); }
    }

    @interface Audited { String value() default \"\"; }

    interface Listener { void changed(Order order); }
}
";
    assert_eq!(
        tree(&outline(text)),
        [
            "Order Class",
            "  TABLE Field",
            "  ALIAS Field",
            "  lines Field",
            "  count Field",
            "  size Field",
            "  task Field",
            "  Order Method",
            "  Order Method",
            "  compareTo Method",
            "  close Method",
            "  Status Enum",
            "    NEW Constant",
            "    PAID Constant",
            "    SHIPPED Constant",
            "    Status Method",
            "    Status Method",
            "    c Field",
            "  Line Class",
            "    sku Field",
            "    quantity Field",
            "  Audited Interface",
            "    value Method",
            "  Listener Interface",
            "    changed Method",
        ]
    );
    let order = &outline(text)[0];
    assert!(text[order.start..order.end].starts_with("@Entity"), "the declaration with its annotations");
    assert!(text[..order.end].ends_with('}'));
}

#[test]
fn deep_nesting_stops_at_32_levels() {
    let text = "class A {".repeat(2000);
    let mut depth = 0;
    let mut level = outline(&text);
    while let Some(first) = level.first() {
        depth += 1;
        level = first.children.clone();
    }
    assert_eq!(depth, 33);
}

#[test]
fn folds_bodies_imports_and_comments() {
    let text = "\
import a.B;
import a.C;
import a.D;

/*
 * Licence.
 */
class E {
    void f() {
        g();
    }
    String s = \"\"\"
        x
        \"\"\";
}
";
    let folds = fold(text);
    for expected in [(0, 2), (4, 6), (7, 14), (8, 10), (11, 13)] {
        assert!(folds.contains(&Fold { start_line: expected.0, end_line: expected.1 }), "{expected:?} in {folds:?}");
    }
}

#[test]
fn completes_keywords_and_names_and_leaves_templates_to_the_manifest() {
    let complete_at = |marked: &str| {
        let caret = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        let (from, items) = complete(&text, caret);
        (from, items.into_iter().map(|item| (item.label, item.kind)).collect::<Vec<_>>())
    };
    let (from, items) = complete_at("class Shop { void ship() { Shipment s; sh| } }");
    assert_eq!(from, 39);
    assert!(items.contains(&("ship".to_owned(), "method")), "{items:?}");
    assert!(items.contains(&("Shipment".to_owned(), "class")), "{items:?}");
    assert!(items.contains(&("short".to_owned(), "keyword")), "{items:?}");
    let (_, items) = complete_at("class A { void m() { so| } }");
    assert!(!items.iter().any(|(_, kind)| *kind == "snippet"), "live templates come from plugin.json: {items:?}");
    let (_, items) = complete_at("class A { void m() { rec| } }");
    assert!(items.contains(&("record".to_owned(), "keyword")), "{items:?}");
    let (_, items) = complete_at("class A { void m() { list.si| } }");
    assert!(!items.iter().any(|(_, kind)| *kind == "keyword"), "no keywords after a dot: {items:?}");
    let json = complete_json(3, &[Item { label: "a\"b".into(), kind: "keyword", insert: "a".into(), detail: None }]);
    assert_eq!(json, r#"{"from":3,"items":[{"label":"a\"b","kind":"keyword","insert":"a"}]}"#);
}

#[test]
fn every_offset_is_on_a_character_boundary() {
    let text = "class Тест { String ш = \"ж\"; /* é */ void 方法() {} }\n// ü\n";
    for span in highlight(text) {
        assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
    }
    for symbol in outline(text) {
        assert!(text.is_char_boundary(symbol.start) && text.is_char_boundary(symbol.end));
    }
    assert_eq!(outline(text)[0].name, "Тест");
}

#[test]
fn the_abi_encodes_records() {
    let spans = highlight("int a;");
    assert_eq!(encode_spans(&spans)[..12], [0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(encode_folds(&[Fold { start_line: 1, end_line: 4 }]), [1, 0, 0, 0, 4, 0, 0, 0]);
    let mut request = 23u32.to_le_bytes().to_vec();
    request.extend_from_slice(b"class A { void m() { re } }");
    assert!(String::from_utf8(complete_request(&request)).unwrap().contains("\"return\""));
}

fn manifest_list(key: &str) -> Vec<serde_json::Value> {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    manifest["liveTemplates"][key].as_array().cloned().unwrap_or_default()
}

fn both_languages(text: &serde_json::Value) -> bool {
    ["en", "ru"].iter().all(|code| text[*code].as_str().is_some_and(|text| !text.trim().is_empty()))
}

#[test]
fn the_manifest_declares_the_java_live_templates_in_both_languages() {
    let templates = manifest_list("templates");
    let mut seen = std::collections::BTreeSet::new();
    for template in &templates {
        let abbreviation = template["abbreviation"].as_str().unwrap();
        assert!(seen.insert(abbreviation), "{abbreviation} is declared once");
        assert_eq!(template["language"], "java");
        assert!(both_languages(&template["description"]), "{abbreviation}");
        assert!(!template["text"].as_str().unwrap().contains("$0"), "{abbreviation}: live template syntax");
    }
    let wanted = ["psvm", "main", "sout", "souf", "soutv", "serr", "fori", "iter", "itar", "itli", "ifn", "inn"];
    for abbreviation in wanted.iter().chain(&["psf", "psfs", "psfi", "prsf", "thr"]) {
        assert!(seen.contains(abbreviation), "{abbreviation}");
    }
    let postfix = manifest_list("postfix");
    let mut keys = std::collections::BTreeSet::new();
    for template in &postfix {
        let key = template["key"].as_str().unwrap();
        assert!(keys.insert(key), "{key} is declared once");
        assert!(template["text"].as_str().unwrap().contains("$EXPR$"), "{key}");
        assert!(both_languages(&template["description"]), "{key}");
    }
    let wanted = ["var", "val", "if", "else", "not", "nn", "null", "for", "fori", "forr", "return", "sout", "try"];
    for key in wanted.iter().chain(&["cast", "new", "par", "lambda"]) {
        assert!(keys.contains(key), "{key}");
    }
    let surrounders = manifest_list("surrounders");
    for surrounder in &surrounders {
        assert!(surrounder["text"].as_str().unwrap().contains("$SELECTION$"), "{surrounder}");
        assert!(both_languages(&surrounder["title"]), "{surrounder}");
    }
    let ids: Vec<&str> = surrounders.iter().filter_map(|surrounder| surrounder["id"].as_str()).collect();
    for id in ["if", "ifElse", "while", "for", "tryCatch", "tryFinally", "synchronized", "runnable"] {
        assert!(ids.contains(&id), "{id}");
    }
}

#[test]
fn the_javadoc_stub_and_statement_rules_use_java_keywords() {
    let stub = &manifest_list("docComments")[0];
    assert_eq!((stub["opening"].as_str(), stub["onEnter"].as_bool()), (Some("/**"), Some(true)));
    for line in ["@param $PARAMETER$", "@return", "@throws $EXCEPTION$"] {
        let declared = [&stub["parameter"], &stub["result"], &stub["thrown"]];
        assert!(declared.iter().any(|text| text.as_str().unwrap_or_default().ends_with(line)), "{line}");
    }
    let rules = &manifest_list("statements")[0];
    assert_eq!(rules["terminator"], ";");
    let contextual = ["record", "throws"];
    let words = stub["keywords"].as_array().unwrap().iter().chain(rules["blocks"].as_array().unwrap());
    for word in words.chain(std::iter::once(&stub["throws"])).filter_map(serde_json::Value::as_str) {
        assert!(KEYWORDS.contains(&word) || contextual.contains(&word), "{word} is a Java keyword");
    }
}

#[test]
fn the_manifest_declares_test_naming_rules_and_junit_and_testng_templates() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    let tests = manifest["tests"].as_array().unwrap();
    assert_eq!(tests.len(), 1);
    let declared = &tests[0];
    let pairs: Vec<(&str, &str)> = declared["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| (pair["subject"].as_str().unwrap(), pair["test"].as_str().unwrap()))
        .collect();
    for (main, test) in [("src/main/java", "src/test/java")] {
        for name in ["{name}Test", "{name}Tests", "{name}IT", "Test{name}"] {
            let subject = format!("{main}/{{path}}{{name}}.java");
            let paired = format!("{test}/{{path}}{name}.java");
            assert!(pairs.contains(&(subject.as_str(), paired.as_str())), "{paired}");
        }
    }
    assert!(declared["members"].as_str().unwrap().contains("(?P<name>"));
    let frameworks = declared["frameworks"].as_array().unwrap();
    let ids: Vec<&str> = frameworks.iter().map(|framework| framework["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["junit5", "junit4", "testng"]);
    for framework in frameworks {
        let file = framework["file"].as_str().unwrap();
        for part in ["${PACKAGE_LINE}", "class ${NAME}", "setUp()", "tearDown()", "${METHODS}"] {
            assert!(file.contains(part), "{}: {part}", framework["id"]);
        }
        assert!(framework["method"].as_str().unwrap().contains("@Test"), "{}", framework["id"]);
        assert!(!framework["detect"].as_array().unwrap().is_empty(), "{}", framework["id"]);
    }
}

#[test]
fn a_caret_inside_a_character_does_not_panic() {
    let text = "class Żółw { }";
    for caret in 0..=text.len() + 2 {
        let _ = complete(text, caret);
    }
}
