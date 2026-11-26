use super::*;

const SOURCE: &str = r#"@file:JvmName("Shop")
package demo.shop

import kotlin.math.max
import kotlinx.coroutines.launch

/** A shop with its items. */
data class Shop(val name: String) {
    private val items = mutableListOf<Item>()
    companion object {
        const val LIMIT = 0x1F_FFL
    }

    override fun toString(): String = "Shop $name with ${items.size} items"

    fun <T : Comparable<T>> List<T>.biggest(): T? {
        val local = 1.5e3f
        loop@ for (item in this) {
            if (item == null) break@loop
        }
        return this@biggest.maxOrNull()
    }
}

/* outer /* nested */ still a comment */
interface Priced { fun price(): Int }

enum class Size { SMALL, LARGE }

typealias Items = List<Item>

fun main() {
    val raw = """
        |line ${'$'}x
    """.trimMargin()
    println(Shop("corner").`odd name`)
}
"#;

fn kinds(text: &str) -> Vec<(&str, Kind)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

#[test]
fn kotlin_is_highlighted() {
    let found = kinds(SOURCE);
    let has = |text: &str, kind: Kind| found.iter().any(|(t, k)| *t == text && *k == kind);
    for (text, kind) in [
        ("@file:JvmName", Kind::Attribute),
        ("package", Kind::Keyword),
        ("import", Kind::Keyword),
        ("/** A shop with its items. */", Kind::DocComment),
        ("data", Kind::Keyword),
        ("class", Kind::Keyword),
        ("Shop", Kind::Type),
        ("private", Kind::Keyword),
        ("companion", Kind::Keyword),
        ("const", Kind::Keyword),
        ("0x1F_FFL", Kind::Number),
        ("override", Kind::Keyword),
        ("toString", Kind::Function),
        ("$name", Kind::Constant),
        ("${", Kind::Constant),
        ("biggest", Kind::Function),
        ("1.5e3f", Kind::Number),
        ("loop@", Kind::Label),
        ("@loop", Kind::Label),
        ("@biggest", Kind::Label),
        ("/* outer /* nested */ still a comment */", Kind::Comment),
        ("interface", Kind::Keyword),
        ("enum", Kind::Keyword),
        ("typealias", Kind::Keyword),
        ("println", Kind::Function),
        ("'$'", Kind::String),
    ] {
        assert!(has(text, kind), "{text:?} as {kind:?} in {found:?}");
    }
    assert!(!has("size", Kind::Keyword), "a name after a dot is not a keyword");
    let spans = highlight(SOURCE);
    assert!(spans.windows(2).all(|pair| pair[0].end <= pair[1].start), "ordered and not overlapping");
    assert!(spans.iter().all(|span| SOURCE.is_char_boundary(span.start) && SOURCE.is_char_boundary(span.end)));
}

#[test]
fn strings_and_templates_end_where_they_should() {
    let found = kinds(r#"val s = "a${map["k"]}b\n" + 'c'"#);
    assert!(found.contains(&("\"a", Kind::String)), "{found:?}");
    assert!(found.contains(&("\"k\"", Kind::String)), "a string inside a template: {found:?}");
    assert!(found.contains(&("\\n", Kind::Constant)));
    assert!(found.contains(&("'c'", Kind::String)));
    let raw = kinds("val r = \"\"\"a \\n \"quoted\" \"\"\"\"\nval x = 1");
    assert_eq!(raw[1], ("\"\"\"a \\n \"quoted\" \"\"\"\"", Kind::String), "no escapes, extra quotes kept: {raw:?}");
    assert!(raw.contains(&("1", Kind::Number)));
    assert_eq!(kinds("val s = \"unclosed\nval y = 2")[1].0, "\"unclosed");
}

#[test]
fn soft_keywords_are_names_where_they_are_names() {
    let found = kinds("val value = data.open + field\nset(x)\nvalue class Id(val v: Int)");
    assert!(!found.contains(&("data", Kind::Keyword)) && !found.contains(&("open", Kind::Keyword)), "{found:?}");
    assert!(found.contains(&("value", Kind::Keyword)), "value class: {found:?}");
}

#[test]
fn blocks_comments_and_imports_fold() {
    let folds = fold(SOURCE);
    assert!(folds.contains(&Fold { start_line: 3, end_line: 4 }), "the imports: {folds:?}");
    assert!(folds.contains(&Fold { start_line: 7, end_line: 22 }), "the class: {folds:?}");
    assert!(folds.contains(&Fold { start_line: 9, end_line: 11 }), "the companion object");
    assert!(fold("fun a() {}\n").is_empty());
}

#[test]
fn declarations_make_the_structure() {
    let outline = outline(SOURCE);
    let top: Vec<(&str, &str)> = outline.iter().map(|symbol| (symbol.name.as_str(), symbol.kind)).collect();
    assert_eq!(
        top,
        [("Shop", "class"), ("Priced", "interface"), ("Size", "class"), ("Items", "type"), ("main()", "function")]
    );
    let shop: Vec<(&str, &str)> =
        outline[0].children.iter().map(|symbol| (symbol.name.as_str(), symbol.kind)).collect();
    assert_eq!(
        shop,
        [("items", "field"), ("companion object", "class"), ("toString()", "method"), ("biggest()", "method")]
    );
    assert_eq!(outline[0].children[1].children[0].name, "LIMIT");
    assert_eq!(outline[0].children[1].children[0].kind, "constant");
    assert_eq!(outline[1].children[0].name, "price()", "an interface's function");
    assert!(SOURCE[outline[0].start..outline[0].end].starts_with("data class Shop"));
    assert!(SOURCE[outline[0].start..outline[0].end].ends_with('}'));
}

#[test]
fn a_gradle_kotlin_script_lists_its_blocks() {
    let script = "plugins {\n    kotlin(\"jvm\") version \"2.1.0\"\n}\n\nval ktorVersion = \"3.0.0\"\n\ndependencies {\n    implementation(\"io.ktor:ktor-server-core:$ktorVersion\")\n}\n\ntasks.register(\"hello\") {\n    doLast { println(\"Hello\") }\n}\n";
    let outline = outline(script);
    let names: Vec<(&str, &str)> = outline.iter().map(|symbol| (symbol.name.as_str(), symbol.kind)).collect();
    assert_eq!(
        names,
        [
            ("plugins", "module"),
            ("ktorVersion", "property"),
            ("dependencies", "module"),
            ("tasks.register(\"hello\")", "module")
        ]
    );
    let found = kinds(script);
    assert!(found.contains(&("implementation", Kind::Function)) && found.contains(&("$ktorVersion", Kind::Constant)));
}

#[test]
fn broken_and_unusual_text_never_breaks_the_lexer() {
    for text in [
        "\"unclosed ${ a + \"b",
        "val c = '\\é'",
        "\"\\u00é\"",
        "/* never closed /* nested",
        "`unclosed name",
        "@",
        "x@",
        "fun",
        "class",
        "}}}{{{",
        "val 😀 = \"😀$😀${😀}\"",
        "\"\"\"raw ${ never",
        "0x",
        "1e",
        "'",
    ] {
        let spans = highlight(text);
        assert!(spans.iter().all(|span| span.end <= text.len() && span.start < span.end), "{text:?}");
        assert!(
            spans.iter().all(|span| text.is_char_boundary(span.start) && text.is_char_boundary(span.end)),
            "{text:?}"
        );
        assert!(spans.windows(2).all(|pair| pair[0].end <= pair[1].start), "{text:?}");
        let _ = fold(text);
        let _ = outline(text);
        let _ = jvm(text);
    }
}

const APP: &str = r#"package com.acme.shop

import kotlin.math.max

fun helper(x: Int) = x + 1

fun main(args: Array<String>) {
    val list = listOf(1, 2).map { it * 2 }
    println(list)
}

class Shop(val name: String) {
    fun main(args: Array<String>) {}

    companion object {
        @JvmStatic
        fun main(args: Array<String>) {
            println(Foo::class.java)
        }
    }
}

object Tool {
    @JvmStatic fun main(vararg args: String) = Unit
}

object NotRunnable {
    fun main(args: Array<String>) {}
}

private fun Int.main() {}
"#;

fn at(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count()
}

#[test]
fn mains_are_top_level_functions_and_jvm_static_mains_of_objects() {
    let file = jvm(APP);
    assert_eq!(file.package, "com.acme.shop");
    assert_eq!(file.jvm_name, "");
    let mains: Vec<(&str, Option<usize>, usize, usize)> = file
        .mains
        .iter()
        .map(|main| {
            (main.class.as_str(), main.owner.map(|owner| at(APP, owner)), at(APP, main.start), at(APP, main.end))
        })
        .collect();
    assert_eq!(
        mains,
        [("", None, 6, 9), ("com.acme.shop.Shop", Some(11), 16, 18), ("com.acme.shop.Tool", Some(22), 23, 23)]
    );
    let classes: Vec<(&str, usize, usize)> =
        file.classes.iter().map(|class| (class.name.as_str(), at(APP, class.start), at(APP, class.end))).collect();
    assert_eq!(
        classes,
        [
            ("com.acme.shop.Shop", 11, 20),
            ("com.acme.shop.Shop$Companion", 14, 19),
            ("com.acme.shop.Tool", 22, 24),
            ("com.acme.shop.NotRunnable", 26, 28)
        ]
    );
    assert!(file.tests.is_empty());
}

#[test]
fn a_main_without_arguments_and_the_file_s_jvm_name() {
    let text = "@file:JvmName(\"Launcher\")\npackage demo\n\nsuspend fun main() {\n    println(1)\n}\n";
    let file = jvm(text);
    assert_eq!((file.package.as_str(), file.jvm_name.as_str()), ("demo", "Launcher"));
    assert_eq!(file.mains.len(), 1);
    assert_eq!(at(text, file.mains[0].start), 3);
    let none = jvm("fun main(x: Int) {}\nclass A { fun main() {} }\n");
    assert!(none.mains.is_empty(), "{none:?}");
}

const TESTS: &str = r#"package com.acme

import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import kotlin.test.assertEquals

class CalcTest {
    @Test
    fun adds() {
        assertEquals(3, 1 + 2)
    }

    @ParameterizedTest
    @ValueSource(ints = [1, 2])
    fun positive(value: Int) = Unit

    @Test fun `two words`() {}

    private fun helper() {}

    @Nested
    inner class Inside {
        @org.junit.jupiter.api.Test
        fun deep() {}
    }
}

abstract class Base {
    @Test fun inherited() {}
}
"#;

#[test]
fn test_classes_and_their_test_functions_are_found() {
    let file = jvm(TESTS);
    type Found<'a> = (&'a str, usize, bool, Vec<(&'a str, usize)>);
    let found: Vec<Found<'_>> = file
        .tests
        .iter()
        .map(|test| {
            let methods = test.methods.iter().map(|(name, start)| (name.as_str(), at(TESTS, *start))).collect();
            (test.name.as_str(), at(TESTS, test.start), test.testng, methods)
        })
        .collect();
    assert_eq!(
        found,
        [
            ("com.acme.CalcTest", 8, false, vec![("adds", 10), ("positive", 16), ("two words", 18)]),
            ("com.acme.CalcTest$Inside", 23, false, vec![("deep", 25)]),
        ]
    );
    let testng = jvm("import org.testng.annotations.Test\n\n@Test\nclass OrderTest {\n    fun ships() {}\n}\n");
    assert_eq!(testng.tests.len(), 1, "{testng:?}");
    assert!(testng.tests[0].testng);
    assert_eq!(testng.tests[0].methods, [("ships".to_owned(), testng.tests[0].methods[0].1)]);
    let kotlin_test = jvm("import kotlin.test.Test\n\nclass SumTest {\n    @Test\n    fun sums() {}\n}\n");
    assert_eq!(kotlin_test.tests[0].name, "SumTest");
}

fn manifest_list(key: &str) -> Vec<serde_json::Value> {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../../plugin.json")).unwrap();
    manifest["liveTemplates"][key].as_array().cloned().unwrap_or_default()
}

fn both_languages(text: &serde_json::Value) -> bool {
    ["en", "ru"].iter().all(|code| text[*code].as_str().is_some_and(|text| !text.trim().is_empty()))
}

#[test]
fn the_manifest_declares_the_kotlin_live_templates_in_both_languages() {
    let mut seen = std::collections::BTreeSet::new();
    for template in &manifest_list("templates") {
        let abbreviation = template["abbreviation"].as_str().unwrap();
        assert!(seen.insert(abbreviation.to_owned()), "{abbreviation} is declared once");
        assert_eq!(template["language"], "kotlin");
        assert!(both_languages(&template["description"]), "{abbreviation}");
    }
    for abbreviation in ["main", "maino", "sout", "serr", "fun0", "fun1", "fun2", "void", "ifn", "inn", "iter"] {
        assert!(seen.contains(abbreviation), "{abbreviation}");
    }
    let mut keys = std::collections::BTreeSet::new();
    for template in &manifest_list("postfix") {
        let key = template["key"].as_str().unwrap();
        assert!(keys.insert(key.to_owned()), "{key} is declared once");
        assert!(template["text"].as_str().unwrap().contains("$EXPR$"), "{key}");
        assert!(both_languages(&template["description"]), "{key}");
    }
    for key in ["val", "var", "if", "else", "not", "nn", "null", "for", "return", "sout", "try", "par", "when"] {
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
fn the_kdoc_stub_and_statement_rules_use_kotlin_keywords() {
    let stub = &manifest_list("docComments")[0];
    assert_eq!((stub["opening"].as_str(), stub["throwsAnnotation"].as_str()), (Some("/**"), Some("@Throws(")));
    let rules = &manifest_list("statements")[0];
    assert!(rules.get("terminator").is_none(), "Kotlin statements end with the line");
    let words = stub["keywords"].as_array().unwrap().iter().chain(rules["blocks"].as_array().unwrap());
    for word in words.filter_map(serde_json::Value::as_str) {
        assert!(HARD.contains(&word) || SOFT.contains(&word), "{word} is a Kotlin keyword");
    }
}
