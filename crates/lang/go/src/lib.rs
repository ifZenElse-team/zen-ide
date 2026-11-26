pub mod arrange;
pub mod deps;
mod embed;
mod file_templates;
mod generate;
pub mod gomod;
mod imports;
mod incremental;
mod interfaces;
pub mod modcache;
pub mod module;
mod modules;
pub mod new_project;
mod nolint;
mod outline;
pub mod project_env;
mod provider;
mod run;
pub mod tags;
mod template;
pub mod test_support;
pub mod toolchain;
pub mod vet;

pub use arrange::GoArranger;
pub use deps::GoModInspector;
pub use embed::GoEmbedFiles;
pub use file_templates::FILE_TEMPLATES;
pub use generate::GoGenerators;
pub use imports::add_import;
pub use module::{MODULE, register};
pub use modules::{GoModHighlighter, GoSumHighlighter};
pub use outline::GoOutliner;
pub use provider::{GoModules, compare_versions};
pub use run::{BUILD, GoRun, TEST, Test2JsonParser, VerboseParser, explain_adapter_error, quote_meta, rerun_patterns};
pub use template::GoTemplateHighlighter;
pub use test_support::GoTests;
pub use vet::{GoVetInspector, InspectionsStatus, InstallStaticcheck, RunInspections};
use zen_syntax::{HighlightKind, Highlighter, Span};

pub const LANGUAGE_SERVERS: &str = include_str!("../language-servers.json");

pub const CODE_STYLE: &str = include_str!("../code-style.json");

pub const TEMPLATES: &str = include_str!("../templates.json");

pub const SURROUNDERS: &str = include_str!("../surrounders.json");

pub const POSTFIX: &str = include_str!("../postfix.json");

pub const DOC_COMMENTS: &str = include_str!("../doc-comments.json");

pub const STATEMENTS: &str = include_str!("../statements.json");

pub const LOCALES: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en/commands.ftl")),
    ("en", include_str!("../locales/en/go.ftl")),
    ("en", include_str!("../locales/en/settings.ftl")),
    ("en", include_str!("../locales/en/toolchain.ftl")),
    ("ru", include_str!("../locales/ru/commands.ftl")),
    ("ru", include_str!("../locales/ru/go.ftl")),
    ("ru", include_str!("../locales/ru/settings.ftl")),
    ("ru", include_str!("../locales/ru/toolchain.ftl")),
];

const KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

const CONSTANTS: &[&str] = &["true", "false", "nil", "iota"];

const TYPES: &[&str] = &[
    "any",
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Whitespace,
    Comment,
    String,
    Rune,
    Number,
    Ident,
    Punct(char),
}

fn tokenize(text: &str) -> Vec<(Token, usize, usize)> {
    let mut tokens = Vec::with_capacity(text.len() / 3);
    let mut i = 0;
    while i < text.len() {
        let start = i;
        let rest = &text[i..];
        let c = rest.chars().next().unwrap_or('\0');
        let token = if c.is_whitespace() {
            i += rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
            Token::Whitespace
        } else if rest.starts_with("//") {
            i += rest.find('\n').unwrap_or(rest.len());
            Token::Comment
        } else if let Some(body) = rest.strip_prefix("/*") {
            i += body.find("*/").map_or(rest.len(), |end| end + 4);
            Token::Comment
        } else if c == '`' {
            i += rest[1..].find('`').map_or(rest.len(), |end| end + 2);
            Token::String
        } else if c == '"' || c == '\'' {
            i = quoted_end(text, i + 1, c);
            if c == '"' { Token::String } else { Token::Rune }
        } else if c.is_ascii_digit() || (c == '.' && rest[1..].starts_with(|c: char| c.is_ascii_digit())) {
            i += 1;
            let hex = rest.starts_with("0x") || rest.starts_with("0X");
            while let Some(next) = text[i..].chars().next() {
                let exponent = matches!(next, '+' | '-')
                    && matches!(text.as_bytes()[i - 1], b'e' | b'E' | b'p' | b'P')
                    && (!hex || matches!(text.as_bytes()[i - 1], b'p' | b'P'));
                if next.is_ascii_alphanumeric() || next == '_' || next == '.' || exponent {
                    i += next.len_utf8();
                } else {
                    break;
                }
            }
            Token::Number
        } else if c == '_' || c.is_alphabetic() {
            i += rest.find(|c: char| !(c == '_' || c.is_alphanumeric())).unwrap_or(rest.len());
            Token::Ident
        } else {
            i += c.len_utf8();
            Token::Punct(c)
        };
        tokens.push((token, start, i));
    }
    tokens
}

fn declared_constants<'a>(text: &'a str, tokens: &[(Token, usize, usize)]) -> std::collections::HashSet<&'a str> {
    let mut names = std::collections::HashSet::new();
    let significant: Vec<(Token, &str, bool)> = {
        let mut out = Vec::new();
        let mut line_start = true;
        for &(token, start, end) in tokens {
            match token {
                Token::Whitespace => line_start |= text[start..end].contains('\n'),
                Token::Comment => line_start |= text[start..end].ends_with('\n') || text[start..end].starts_with("//"),
                _ => {
                    out.push((token, &text[start..end], line_start));
                    line_start = false;
                }
            }
        }
        out
    };
    let mut index = 0;
    while index < significant.len() {
        if significant[index].1 == "const" {
            match significant.get(index + 1) {
                Some((Token::Ident, name, _)) => names.insert(*name),
                Some((Token::Punct('('), ..)) => {
                    index += 2;
                    while let Some(&(token, word, first)) = significant.get(index) {
                        if token == Token::Punct(')') && first {
                            break;
                        }
                        if token == Token::Ident && first {
                            names.insert(word);
                        }
                        index += 1;
                    }
                    true
                }
                _ => false,
            };
        }
        index += 1;
    }
    names
}

fn quoted_end(text: &str, from: usize, quote: char) -> usize {
    let mut chars = text[from..].char_indices();
    while let Some((offset, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '\n' => return from + offset,
            c if c == quote => return from + offset + 1,
            _ => {}
        }
    }
    text.len()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GoHighlighter;

fn is_keyword(word: &str) -> bool {
    static SET: std::sync::LazyLock<std::collections::HashSet<&'static str>> =
        std::sync::LazyLock::new(|| KEYWORDS.iter().chain(CONSTANTS).copied().collect());
    word.len() <= 11 && word.as_bytes().first().is_some_and(u8::is_ascii_lowercase) && SET.contains(word)
}

fn is_builtin_type(word: &str) -> bool {
    static SET: std::sync::LazyLock<std::collections::HashSet<&'static str>> =
        std::sync::LazyLock::new(|| TYPES.iter().copied().collect());
    word.as_bytes().first().is_some_and(u8::is_ascii_lowercase) && SET.contains(word)
}

impl Highlighter for GoHighlighter {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn incremental(&self, text: &str) -> Option<Box<dyn zen_syntax::Incremental>> {
        Some(Box::new(incremental::GoLines::new(text)))
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let tokens = tokenize(text);
        let significant = |range: &mut dyn DoubleEndedIterator<Item = &(Token, usize, usize)>| {
            let mut found = None;
            for token in range {
                if !matches!(token.0, Token::Whitespace | Token::Comment) {
                    found = Some(*token);
                    break;
                }
            }
            found
        };
        let constants = declared_constants(text, &tokens);
        let mut spans: Vec<Span> = Vec::with_capacity(tokens.len() / 2);
        for (index, &(token, start, end)) in tokens.iter().enumerate() {
            let word = &text[start..end];
            let kind = match token {
                Token::Comment => Some(HighlightKind::Comment),
                Token::String | Token::Rune => Some(HighlightKind::String),
                Token::Number => Some(HighlightKind::Number),
                Token::Ident => {
                    let next = significant(&mut tokens[index + 1..].iter()).map(|token| token.0);
                    let previous = significant(&mut tokens[..index].iter().rev()).map(|(_, s, e)| &text[s..e]);
                    if is_keyword(word) {
                        Some(HighlightKind::Keyword)
                    } else if constants.contains(word) {
                        Some(HighlightKind::Constant)
                    } else if is_builtin_type(word) {
                        Some(HighlightKind::Type)
                    } else if previous == Some("func") || next == Some(Token::Punct('(')) {
                        Some(HighlightKind::Function)
                    } else if previous == Some("type") {
                        Some(HighlightKind::Type)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(kind) = kind {
                spans.push(Span { range: start..end, kind });
            }
        }
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, HighlightKind)> {
        GoHighlighter.highlight(text).into_iter().map(|span| (&text[span.range], span.kind)).collect()
    }

    #[test]
    fn highlights_a_small_program() {
        let text = "package main\n\nimport \"fmt\"\n\n// Greet says hello.\nfunc Greet(name string) error {\n\tfmt.Println(`raw\nstring`, 'x', 0x1F, 1.5e-3, len(name), nil)\n\treturn nil\n}\n\ntype Point struct{ X int }\n";
        let found = kinds(text);
        for expected in [
            ("package", HighlightKind::Keyword),
            ("\"fmt\"", HighlightKind::String),
            ("// Greet says hello.", HighlightKind::Comment),
            ("func", HighlightKind::Keyword),
            ("Greet", HighlightKind::Function),
            ("string", HighlightKind::Type),
            ("error", HighlightKind::Type),
            ("Println", HighlightKind::Function),
            ("`raw\nstring`", HighlightKind::String),
            ("'x'", HighlightKind::String),
            ("0x1F", HighlightKind::Number),
            ("1.5e-3", HighlightKind::Number),
            ("len", HighlightKind::Function),
            ("nil", HighlightKind::Keyword),
            ("Point", HighlightKind::Type),
        ] {
            assert!(found.contains(&expected), "missing {expected:?} in {found:?}");
        }
    }

    #[test]
    fn highlights_declared_constants_where_they_are_used() {
        let found = kinds("const Max = 3\nconst (\n\tA = iota\n\tB\n)\nfunc f() int { return Max + B }\n");
        let constants: Vec<_> =
            found.iter().filter(|(_, kind)| *kind == HighlightKind::Constant).map(|(word, _)| *word).collect();
        assert_eq!(constants, ["Max", "A", "B", "Max", "B"]);
    }

    #[test]
    fn covers_everything_and_survives_unterminated_input() {
        for text in ["\"open", "`open", "/* open", "'", "0x", "func ("] {
            let tokens = tokenize(text);
            assert_eq!(tokens.last().map(|token| token.2), Some(text.len()));
        }
        assert!(GoHighlighter.handles("main.go"));
        assert!(!GoHighlighter.handles("go.mod"));
    }
}
