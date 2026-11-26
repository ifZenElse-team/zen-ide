pub mod analyzer;
pub mod cargo;
mod lexer;
pub mod module;
mod provider;
pub mod run;
pub mod test_support;
pub mod toolchain;

use lexer::{Token, tokenize};
pub use module::{MODULE, register};
pub use provider::{CargoModels, from_metadata, load as load_model};
pub use run::{CargoOptions, CargoRun, LibtestParser};
pub use test_support::RustTests;
use zen_syntax::{HighlightKind, Highlighter, Span};

pub const LANGUAGE_SERVERS: &str = include_str!("../language-servers.json");

pub const POSTFIX: &str = include_str!("../postfix.json");

pub const DOC_COMMENTS: &str = include_str!("../doc-comments.json");

pub const STATEMENTS: &str = include_str!("../statements.json");

pub const CODE_STYLE: &str = include_str!("../code-style.json");

pub const LOCALES: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en/commands.ftl")),
    ("en", include_str!("../locales/en/run.ftl")),
    ("en", include_str!("../locales/en/rust.ftl")),
    ("en", include_str!("../locales/en/settings.ftl")),
    ("ru", include_str!("../locales/ru/commands.ftl")),
    ("ru", include_str!("../locales/ru/run.ftl")),
    ("ru", include_str!("../locales/ru/rust.ftl")),
    ("ru", include_str!("../locales/ru/settings.ftl")),
];

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
    "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "abstract",
    "become", "box", "do", "final", "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
    "union",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct RustHighlighter;

impl Highlighter for RustHighlighter {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".rs")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let tokens = tokenize(text);
        let next = |index: usize| significant(tokens[index + 1..].iter());
        let previous_word =
            |index: usize| significant(&mut tokens[..index].iter().rev()).map(|(_, start, end)| &text[start..end]);

        let mut spans: Vec<Span> = Vec::new();
        let mut attribute_depth: Option<usize> = None;
        for (index, &(token, start, end)) in tokens.iter().enumerate() {
            let word = &text[start..end];
            if let Some(depth) = attribute_depth.as_mut() {
                match token {
                    Token::Punct('[') => *depth += 1,
                    Token::Punct(']') => *depth = depth.saturating_sub(1),
                    _ => {}
                }
                if token != Token::Whitespace {
                    push(&mut spans, start, end, HighlightKind::Attribute);
                }
                if *depth == 0 && token == Token::Punct(']') {
                    attribute_depth = None;
                }
                continue;
            }
            let highlight = match token {
                Token::Punct('#') if matches!(next(index), Some((Token::Punct('[' | '!'), _, _))) => {
                    attribute_depth = Some(0);
                    Some(HighlightKind::Attribute)
                }
                Token::LineComment { doc: true } | Token::BlockComment { doc: true } => Some(HighlightKind::DocComment),
                Token::LineComment { .. } | Token::BlockComment { .. } => Some(HighlightKind::Comment),
                Token::String | Token::Char => Some(HighlightKind::String),
                Token::Number => Some(HighlightKind::Number),
                Token::Lifetime => Some(HighlightKind::Lifetime),
                Token::Ident => {
                    let following = next(index).map(|(token, _, _)| token);
                    if KEYWORDS.contains(&word) {
                        Some(HighlightKind::Keyword)
                    } else if following == Some(Token::Punct('!')) && previous_word(index) != Some("macro_rules") {
                        Some(HighlightKind::Macro)
                    } else if previous_word(index) == Some("fn") || following == Some(Token::Punct('(')) {
                        Some(HighlightKind::Function)
                    } else if word.len() > 1
                        && word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    {
                        Some(HighlightKind::Constant)
                    } else if word.chars().next().is_some_and(char::is_uppercase) {
                        Some(HighlightKind::Type)
                    } else {
                        None
                    }
                }
                Token::Punct('!')
                    if spans
                        .last()
                        .is_some_and(|span| span.kind == HighlightKind::Macro && span.range.end == start) =>
                {
                    Some(HighlightKind::Macro)
                }
                _ => None,
            };
            if let Some(highlight) = highlight {
                push(&mut spans, start, end, highlight);
            }
        }
        spans
    }
}

fn significant<'a>(mut tokens: impl Iterator<Item = &'a (Token, usize, usize)>) -> Option<(Token, usize, usize)> {
    tokens
        .find(|(token, _, _)| {
            !matches!(token, Token::Whitespace | Token::LineComment { .. } | Token::BlockComment { .. })
        })
        .copied()
}

fn push(spans: &mut Vec<Span>, start: usize, end: usize, kind: HighlightKind) {
    if let Some(last) = spans.last_mut()
        && last.kind == kind
        && last.range.end == start
    {
        last.range.end = end;
        return;
    }
    spans.push(Span { range: start..end, kind });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, HighlightKind)> {
        RustHighlighter.highlight(text).into_iter().map(|span| (&text[span.range], span.kind)).collect()
    }

    #[test]
    fn highlights_a_small_program() {
        let text =
            "/// Doc\n#[derive(Debug)]\nfn main() { let x: Vec<u8> = vec![1, 2]; println!(\"hi {x:?}\"); // done\n}";
        let found = kinds(text);
        assert!(found.contains(&("/// Doc", HighlightKind::DocComment)));
        assert!(found.contains(&("#[derive(Debug)]", HighlightKind::Attribute)));
        assert!(found.contains(&("fn", HighlightKind::Keyword)));
        assert!(found.contains(&("main", HighlightKind::Function)));
        assert!(found.contains(&("Vec", HighlightKind::Type)));
        assert!(kinds("const MAX_SIZE: usize = 3;").contains(&("MAX_SIZE", HighlightKind::Constant)));
        assert!(found.contains(&("vec!", HighlightKind::Macro)));
        assert!(found.contains(&("println!", HighlightKind::Macro)));
        assert!(found.contains(&("1", HighlightKind::Number)));
        assert!(found.contains(&("\"hi {x:?}\"", HighlightKind::String)));
        assert!(found.contains(&("// done", HighlightKind::Comment)));
    }

    #[test]
    fn lifetimes_and_handled_files() {
        assert!(kinds("fn f<'a>(x: &'a str) {}").contains(&("'a", HighlightKind::Lifetime)));
        assert!(RustHighlighter.handles("main.rs"));
        assert!(!RustHighlighter.handles("Cargo.toml"));
    }

    #[test]
    fn spans_are_ordered_and_do_not_overlap() {
        let text = "fn a() -> Result<(), Box<dyn std::error::Error>> { Ok(()) }";
        let spans = RustHighlighter.highlight(text);
        for pair in spans.windows(2) {
            assert!(pair[0].range.end <= pair[1].range.start);
        }
    }
}
