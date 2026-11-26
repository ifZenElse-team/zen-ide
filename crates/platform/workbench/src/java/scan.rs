#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestClass {
    pub name: String,
    pub line: u32,
    pub methods: Vec<TestMethod>,
    pub testng: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestMethod {
    pub name: String,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestFile {
    pub package: String,
    pub classes: Vec<TestClass>,
}

const TEST_ANNOTATIONS: &[&str] = &["Test", "ParameterizedTest", "RepeatedTest", "TestFactory", "TestTemplate"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token<'a> {
    pub(crate) text: &'a str,
    pub(crate) line: u32,
}

pub(crate) fn tokens(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut line = 0u32;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        match byte {
            b'\n' => {
                line += 1;
                at += 1;
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at += 2;
                while at < bytes.len() && !(bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/')) {
                    line += u32::from(bytes[at] == b'\n');
                    at += 1;
                }
                at += 2;
            }
            b'"' if text[at..].starts_with("\"\"\"") => {
                at += 3;
                while at < bytes.len() && !text[at..].starts_with("\"\"\"") {
                    line += u32::from(bytes[at] == b'\n');
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
                at += 3;
            }
            b'"' | b'\'' => {
                at += 1;
                while at < bytes.len() && bytes[at] != byte && bytes[at] != b'\n' {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
                at += 1;
            }
            _ if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' || byte >= 0x80 => {
                let start = at;
                while at < bytes.len()
                    && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'_' | b'$') || bytes[at] >= 0x80)
                {
                    at += 1;
                }
                out.push(Token { text: &text[start..at], line });
            }
            _ if byte.is_ascii_whitespace() => at += 1,
            _ => {
                let width = text[at..].chars().next().map_or(1, char::len_utf8);
                out.push(Token { text: &text[at..at + width], line });
                at += width;
            }
        }
    }
    out
}

fn is_word(token: &Token<'_>) -> bool {
    token.text.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
}

struct Open {
    name: String,
    depth: usize,
    class: usize,
}

pub fn scan(text: &str) -> TestFile {
    let tokens = tokens(text);
    let word = |index: usize| tokens.get(index).map_or("", |token| token.text);
    let testng = text.contains("org.testng");
    let mut file = TestFile::default();
    let mut found: Vec<(TestClass, bool, bool)> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    let mut depth = 0usize;
    let mut annotations: Vec<&str> = Vec::new();
    let mut modifiers_abstract = false;
    let mut paren = 0usize;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        match token.text {
            "package" if depth == 0 && file.package.is_empty() => {
                let mut parts = Vec::new();
                index += 1;
                while index < tokens.len() && word(index) != ";" {
                    if word(index) != "." {
                        parts.push(word(index));
                    }
                    index += 1;
                }
                file.package = parts.join(".");
            }
            "@" if word(index + 1) != "interface" => {
                let mut name = word(index + 1);
                index += 1;
                while word(index + 1) == "." && tokens.get(index + 2).is_some_and(is_word) {
                    index += 2;
                    name = word(index);
                }
                annotations.push(name);
                if word(index + 1) == "(" {
                    let mut nested = 0;
                    index += 1;
                    while index < tokens.len() {
                        match word(index) {
                            "(" => nested += 1,
                            ")" => {
                                nested -= 1;
                                if nested == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        index += 1;
                    }
                }
            }
            "abstract" => modifiers_abstract = true,
            "class" | "interface" | "enum" | "record"
                if paren == 0
                    && (index == 0 || word(index - 1) != ".")
                    && tokens.get(index + 1).is_some_and(is_word) =>
            {
                let simple = word(index + 1);
                let name = match open.last() {
                    Some(outer) => format!("{}${simple}", outer.name),
                    None if file.package.is_empty() => simple.to_owned(),
                    None => format!("{}.{simple}", file.package),
                };
                let class_level_test = annotations.contains(&"Test");
                let concrete = token.text == "class" && !modifiers_abstract;
                found.push((
                    TestClass { name: name.clone(), line: tokens[index + 1].line, methods: Vec::new(), testng },
                    concrete,
                    class_level_test,
                ));
                index += 2;
                while index < tokens.len() && word(index) != "{" {
                    index += 1;
                }
                depth += 1;
                open.push(Open { name, depth, class: found.len() - 1 });
                annotations.clear();
                modifiers_abstract = false;
            }
            "(" => {
                if paren == 0
                    && let Some(class) = open.last().filter(|class| class.depth == depth)
                    && index > 0
                    && tokens.get(index - 1).is_some_and(is_word)
                    && annotations.iter().any(|annotation| TEST_ANNOTATIONS.contains(annotation))
                {
                    let name = tokens[index - 1].text;
                    let line = tokens[index - 1].line;
                    found[class.class].0.methods.push(TestMethod { name: name.to_owned(), line });
                    annotations.clear();
                }
                paren += 1;
            }
            ")" => paren = paren.saturating_sub(1),
            "{" => {
                depth += 1;
                annotations.clear();
                modifiers_abstract = false;
            }
            "}" => {
                if open.last().is_some_and(|class| class.depth == depth) {
                    open.pop();
                }
                depth = depth.saturating_sub(1);
                annotations.clear();
                modifiers_abstract = false;
            }
            ";" if paren == 0 => {
                annotations.clear();
                modifiers_abstract = false;
            }
            _ => {}
        }
        index += 1;
    }
    file.classes = found
        .into_iter()
        .filter(|(class, concrete, class_level)| *concrete && (!class.methods.is_empty() || *class_level))
        .map(|(class, _, _)| class)
        .collect();
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junit_5_and_4_tests_nested_classes_and_parameterized_ones() {
        let text = r#"package com.acme.shop;

import org.junit.jupiter.api.*;

/** Not a test: "class Fake { @Test void no() {} }" */
class CartTest {
    private final String text = """
        @Test void notAMethod() {}
        """;

    @BeforeEach
    void setUp() {}

    @Test
    void adds() { new Object() { void inner() {} }; }

    @org.junit.jupiter.api.Test
    @DisplayName("removes (all)")
    void removes() {}

    @ParameterizedTest
    @ValueSource(ints = {1, 2})
    void positive(int value) {}

    void helper() {}

    @Nested
    class Empty {
        @Test void isEmpty() {}
    }
}

abstract class BaseTest {
    @Test void inherited() {}
}
"#;
        let file = scan(text);
        assert_eq!(file.package, "com.acme.shop");
        type Shown<'a> = (&'a str, u32, Vec<(&'a str, u32)>);
        let names: Vec<Shown<'_>> = file
            .classes
            .iter()
            .map(|class| {
                (class.name.as_str(), class.line, class.methods.iter().map(|m| (m.name.as_str(), m.line)).collect())
            })
            .collect();
        assert_eq!(
            names,
            [
                ("com.acme.shop.CartTest", 5, vec![("adds", 14), ("removes", 18), ("positive", 22)]),
                ("com.acme.shop.CartTest$Empty", 27, vec![("isEmpty", 28)]),
            ]
        );
        assert!(!file.classes[0].testng);
    }

    #[test]
    fn testng_classes_annotated_as_a_whole_count() {
        let text =
            "import org.testng.annotations.Test;\n\n@Test\npublic class OrderTest {\n    public void ships() {}\n}\n";
        let file = scan(text);
        assert_eq!(file.package, "");
        assert_eq!(file.classes.len(), 1);
        assert_eq!(
            (file.classes[0].name.as_str(), file.classes[0].line, file.classes[0].testng),
            ("OrderTest", 3, true)
        );
        assert!(scan("class Plain { void main() {} }").classes.is_empty());
        assert!(scan("@interface Test {} interface Spec { @Test void a(); }").classes.is_empty());
    }
}
