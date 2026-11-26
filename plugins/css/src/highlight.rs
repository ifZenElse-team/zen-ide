use crate::data;
use crate::token::{Tok, Token, scss, tokenize};
use crate::tree::{Node, NodeKind, parse};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
    Macro = 5,
    Type = 6,
    Function = 7,
    Label = 8,
    Attribute = 9,
    Constant = 10,
    Punctuation = 11,
    Link = 15,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub fn highlight(text: &str) -> Vec<Span> {
    let tokens = tokenize(text);
    let nodes = parse(text, &tokens);
    let mut highlighter = Highlighter { text, tokens: &tokens, spans: Vec::new() };
    for token in &tokens {
        if token.tok == Tok::Comment {
            highlighter.push(token.start, token.end, Kind::Comment);
        }
    }
    highlighter.nodes(&nodes);
    highlighter.spans.sort_by_key(|span| span.start);
    highlighter.spans
}

struct Highlighter<'a> {
    text: &'a str,
    tokens: &'a [Token],
    spans: Vec<Span>,
}

impl Highlighter<'_> {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if end > start {
            self.spans.push(Span { start, end, kind });
        }
    }

    fn token(&mut self, index: usize, kind: Kind) {
        let token = self.tokens[index];
        self.push(token.start, token.end, kind);
    }

    fn word(&self, index: usize) -> &str {
        let token = &self.tokens[index];
        &self.text[token.start..token.end]
    }

    fn next_significant(&self, from: usize, end: usize) -> Option<usize> {
        (from..end).find(|&i| !matches!(self.tokens[i].tok, Tok::Space | Tok::Comment))
    }

    fn nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            match node.kind {
                NodeKind::Rule => self.selector(node.prelude.clone()),
                NodeKind::AtRule => {
                    self.token(node.first, Kind::Keyword);
                    let keyword = self.word(node.first).to_ascii_lowercase();
                    let mut prelude = node.prelude.clone();
                    if scss()
                        && matches!(keyword.as_str(), "@mixin" | "@include" | "@function")
                        && let Some(name) = self.next_significant(prelude.start, prelude.end)
                        && matches!(self.tokens[name].tok, Tok::Ident | Tok::Function)
                    {
                        self.token(name, Kind::Function);
                        prelude.start = name + 1;
                    }
                    self.at_prelude(prelude);
                }
                NodeKind::Declaration => {
                    let name = self.tokens[node.first];
                    if name.tok == Tok::Variable {
                        self.push(name.start, name.end, Kind::Label);
                    } else if name.tok == Tok::Ident {
                        let kind = if self.word(node.first).starts_with("--") { Kind::Label } else { Kind::Attribute };
                        self.push(name.start, name.end, kind);
                    }
                    if let Some(colon) = node.colon {
                        self.token(colon, Kind::Punctuation);
                        self.value(node.prelude.clone());
                    }
                }
            }
            if let Some(semicolon) = node.semicolon {
                self.token(semicolon, Kind::Punctuation);
            }
            if let Some(block) = &node.block {
                self.token(block.open, Kind::Punctuation);
                self.nodes(&block.children);
                if let Some(close) = block.close {
                    self.token(close, Kind::Punctuation);
                }
            }
        }
    }

    fn selector(&mut self, range: Range<usize>) {
        let mut attribute = None::<u8>;
        let mut i = range.start;
        while i < range.end {
            let token = self.tokens[i];
            let text = &self.text[token.start..token.end];
            match token.tok {
                Tok::Ident => {
                    let kind = match attribute {
                        None => Kind::Keyword,
                        Some(0) => Kind::Attribute,
                        Some(2) => Kind::String,
                        Some(_) => Kind::Keyword,
                    };
                    self.push(token.start, token.end, kind);
                    attribute = attribute.map(|state| if state == 2 { 3 } else { state.max(1) });
                }
                Tok::String => {
                    self.push(token.start, token.end, Kind::String);
                    attribute = attribute.map(|_| 3);
                }
                Tok::Delim
                    if text == "."
                        && self.tokens.get(i + 1).is_some_and(|next| next.tok == Tok::Ident)
                        && i + 1 < range.end =>
                {
                    self.push(token.start, self.tokens[i + 1].end, Kind::Type);
                    i += 1;
                }
                Tok::Hash => self.push(token.start, token.end, Kind::Constant),
                Tok::Colon => {
                    let mut j = i + 1;
                    if j < range.end && self.tokens[j].tok == Tok::Colon {
                        j += 1;
                    }
                    if j < range.end && matches!(self.tokens[j].tok, Tok::Ident | Tok::Function) {
                        self.push(token.start, self.tokens[j].end, Kind::Macro);
                        i = j;
                    } else {
                        self.push(token.start, self.tokens[j - 1].end, Kind::Punctuation);
                        i = j - 1;
                    }
                }
                Tok::Function => self.push(token.start, token.end, Kind::Function),
                Tok::Number | Tok::Percentage | Tok::Dimension => self.push(token.start, token.end, Kind::Number),
                Tok::OpenBracket => {
                    self.push(token.start, token.end, Kind::Punctuation);
                    attribute = Some(0);
                }
                Tok::CloseBracket => {
                    self.push(token.start, token.end, Kind::Punctuation);
                    attribute = None;
                }
                Tok::Delim if attribute.is_some() => {
                    self.push(token.start, token.end, Kind::Punctuation);
                    if text == "=" {
                        attribute = Some(2);
                    }
                }
                Tok::Delim
                    if text == "%"
                        && scss()
                        && self.tokens.get(i + 1).is_some_and(|next| next.tok == Tok::Ident)
                        && i + 1 < range.end =>
                {
                    self.push(token.start, self.tokens[i + 1].end, Kind::Type);
                    i += 1;
                }
                Tok::Interpolation | Tok::Variable => self.common_value(i),
                Tok::Delim if text == "*" || text == "&" => self.push(token.start, token.end, Kind::Keyword),
                Tok::Delim if matches!(text, ">" | "+" | "~" | "|") => {
                    self.push(token.start, token.end, Kind::Punctuation)
                }
                Tok::Comma | Tok::OpenParen | Tok::CloseParen => self.push(token.start, token.end, Kind::Punctuation),
                _ => {}
            }
            i += 1;
        }
    }

    fn at_prelude(&mut self, range: Range<usize>) {
        let mut depth = 0usize;
        for i in range.clone() {
            let token = self.tokens[i];
            let text = &self.text[token.start..token.end];
            match token.tok {
                Tok::Ident if text.starts_with("--") => self.push(token.start, token.end, Kind::Label),
                Tok::Ident
                    if depth > 0
                        && self
                            .next_significant(i + 1, range.end)
                            .is_some_and(|n| self.tokens[n].tok == Tok::Colon) =>
                {
                    self.push(token.start, token.end, Kind::Attribute);
                }
                Tok::Ident
                    if matches!(text.to_ascii_lowercase().as_str(), "and" | "or" | "not" | "only")
                        || (scss() && matches!(text, "in" | "from" | "through" | "to" | "if")) =>
                {
                    self.push(token.start, token.end, Kind::Keyword);
                }
                Tok::OpenParen => {
                    depth += 1;
                    self.push(token.start, token.end, Kind::Punctuation);
                }
                Tok::CloseParen => {
                    depth = depth.saturating_sub(1);
                    self.push(token.start, token.end, Kind::Punctuation);
                }
                Tok::Delim if matches!(text, "<" | ">" | "=" | "/") => {
                    self.push(token.start, token.end, Kind::Punctuation)
                }
                Tok::Colon | Tok::Comma => self.push(token.start, token.end, Kind::Punctuation),
                _ => self.common_value(i),
            }
        }
    }

    fn value(&mut self, range: Range<usize>) {
        let mut i = range.start;
        while i < range.end {
            let token = self.tokens[i];
            let text = &self.text[token.start..token.end];
            match token.tok {
                Tok::Ident if text.starts_with("--") => self.push(token.start, token.end, Kind::Label),
                Tok::Ident if data::is_colour_name(text) => self.push(token.start, token.end, Kind::Constant),
                Tok::Delim if text == "!" => {
                    if let Some(next) = self.next_significant(i + 1, range.end)
                        && self.tokens[next].tok == Tok::Ident
                        && (self.word(next).eq_ignore_ascii_case("important")
                            || (scss() && matches!(self.word(next), "default" | "global")))
                    {
                        self.push(token.start, token.end, Kind::Keyword);
                        self.token(next, Kind::Keyword);
                        i = next;
                    }
                }
                Tok::Delim if text == "/" => self.push(token.start, token.end, Kind::Punctuation),
                Tok::Comma | Tok::Colon | Tok::OpenParen | Tok::CloseParen | Tok::OpenBracket | Tok::CloseBracket => {
                    self.push(token.start, token.end, Kind::Punctuation);
                }
                _ => self.common_value(i),
            }
            i += 1;
        }
    }

    fn common_value(&mut self, index: usize) {
        let token = self.tokens[index];
        match token.tok {
            Tok::Number | Tok::Percentage | Tok::Dimension => self.push(token.start, token.end, Kind::Number),
            Tok::Hash => self.push(token.start, token.end, Kind::Constant),
            Tok::String => self.push(token.start, token.end, Kind::String),
            Tok::Function => self.push(token.start, token.end, Kind::Function),
            Tok::Url => self.url(token),
            Tok::Variable => self.push(token.start, token.end, Kind::Label),
            Tok::Interpolation => {
                let closed = self.text.as_bytes()[token.end - 1] == b'}' && token.end > token.start + 2;
                let inner_end = if closed { token.end - 1 } else { token.end };
                self.push(token.start, token.start + 2, Kind::Punctuation);
                self.push(token.start + 2, inner_end, Kind::Label);
                if closed {
                    self.push(inner_end, token.end, Kind::Punctuation);
                }
            }
            _ => {}
        }
    }

    fn url(&mut self, token: Token) {
        let bytes = self.text.as_bytes();
        let name_end = token.start + 3;
        self.push(token.start, name_end, Kind::Function);
        self.push(name_end, name_end + 1, Kind::Punctuation);
        let closed = token.end > name_end + 1 && bytes[token.end - 1] == b')';
        let inner_end = if closed { token.end - 1 } else { token.end };
        let inner = &self.text[name_end + 1..inner_end];
        let leading = inner.len() - inner.trim_start().len();
        let trailing = inner.len() - inner.trim_end().len();
        if leading < inner.len() {
            self.push(name_end + 1 + leading, inner_end - trailing, Kind::Link);
        }
        if closed {
            self.push(token.end - 1, token.end, Kind::Punctuation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    fn assert_contains(text: &str, expected: &[(&str, Kind)]) {
        let actual = kinds(text);
        for item in expected {
            assert!(actual.contains(item), "{item:?} not in {actual:?}");
        }
    }

    fn assert_well_formed(text: &str) {
        let mut previous = 0;
        for span in highlight(text) {
            assert!(
                span.start >= previous
                    && span.end > span.start
                    && span.end <= text.len()
                    && text.is_char_boundary(span.start)
                    && text.is_char_boundary(span.end),
                "{text:?}"
            );
            previous = span.end;
        }
    }

    #[test]
    fn highlights_a_rule() {
        assert_eq!(
            kinds("a.btn > #main:hover { color: #fff; }"),
            [
                ("a", Kind::Keyword),
                (".btn", Kind::Type),
                (">", Kind::Punctuation),
                ("#main", Kind::Constant),
                (":hover", Kind::Macro),
                ("{", Kind::Punctuation),
                ("color", Kind::Attribute),
                (":", Kind::Punctuation),
                ("#fff", Kind::Constant),
                (";", Kind::Punctuation),
                ("}", Kind::Punctuation),
            ]
        );
    }

    #[test]
    fn highlights_selectors() {
        assert_contains(
            "*, ul li::before, input[type=\"text\" i], a[href^=http], p:not(.x):nth-child(2n+1), & + b ~ c {}",
            &[
                ("*", Kind::Keyword),
                (",", Kind::Punctuation),
                ("ul", Kind::Keyword),
                ("li", Kind::Keyword),
                ("::before", Kind::Macro),
                ("[", Kind::Punctuation),
                ("type", Kind::Attribute),
                ("=", Kind::Punctuation),
                ("\"text\"", Kind::String),
                ("i", Kind::Keyword),
                ("]", Kind::Punctuation),
                ("href", Kind::Attribute),
                ("^", Kind::Punctuation),
                ("http", Kind::String),
                (":not", Kind::Macro),
                ("(", Kind::Punctuation),
                (".x", Kind::Type),
                (":nth-child", Kind::Macro),
                ("2n", Kind::Number),
                ("+1", Kind::Number),
                ("&", Kind::Keyword),
                ("+", Kind::Punctuation),
                ("~", Kind::Punctuation),
            ],
        );
    }

    #[test]
    fn highlights_values() {
        assert_contains(
            "a { margin: -1.5rem 0 10% auto !important; color: RebeccaPurple; background: url( img/a.png ) no-repeat, url(\"b.svg\"); width: calc(100% - var(--gap, 4px)); --gap: 8px; font: 12px/1.5 'Inter', sans-serif }",
            &[
                ("margin", Kind::Attribute),
                ("-1.5rem", Kind::Number),
                ("0", Kind::Number),
                ("10%", Kind::Number),
                ("!", Kind::Keyword),
                ("important", Kind::Keyword),
                ("RebeccaPurple", Kind::Constant),
                ("url", Kind::Function),
                ("img/a.png", Kind::Link),
                (")", Kind::Punctuation),
                ("\"b.svg\"", Kind::String),
                ("calc", Kind::Function),
                ("var", Kind::Function),
                ("--gap", Kind::Label),
                ("4px", Kind::Number),
                ("/", Kind::Punctuation),
                ("'Inter'", Kind::String),
            ],
        );
        assert!(!kinds("a { display: block }").iter().any(|(text, _)| *text == "block"));
    }

    #[test]
    fn highlights_at_rules_and_comments() {
        assert_contains(
            "/* top\n */ @import url(x.css) screen;\n@media screen and (min-width: 600px) { .a { x: 1 } }\n@supports not (display: grid) {}\n@layer base, theme;\n@font-face { font-family: \"X\"; }",
            &[
                ("/* top\n */", Kind::Comment),
                ("@import", Kind::Keyword),
                ("x.css", Kind::Link),
                ("@media", Kind::Keyword),
                ("and", Kind::Keyword),
                ("min-width", Kind::Attribute),
                ("600px", Kind::Number),
                (".a", Kind::Type),
                ("@supports", Kind::Keyword),
                ("not", Kind::Keyword),
                ("display", Kind::Attribute),
                ("@layer", Kind::Keyword),
                ("@font-face", Kind::Keyword),
                ("font-family", Kind::Attribute),
                ("\"X\"", Kind::String),
            ],
        );
        assert_contains(
            "@keyframes spin { from { a: 0 } 50% { b: 1turn } }",
            &[("from", Kind::Keyword), ("50%", Kind::Number), ("1turn", Kind::Number)],
        );
    }

    #[test]
    fn highlights_nesting_and_custom_properties() {
        assert_contains(
            ":root { --main-bg: #0a0b0c; }\n.card { color: var(--fg); &:hover { color: red } .icon { width: 1em } }",
            &[
                (":root", Kind::Macro),
                ("--main-bg", Kind::Label),
                ("#0a0b0c", Kind::Constant),
                ("--fg", Kind::Label),
                ("&", Kind::Keyword),
                (":hover", Kind::Macro),
                ("red", Kind::Constant),
                (".icon", Kind::Type),
                ("width", Kind::Attribute),
            ],
        );
    }

    #[test]
    fn highlights_scss() {
        if !scss() {
            return;
        }
        assert_contains(
            "// note\n$gap: 4px !default;\n@mixin pad($x) { padding: $x; }\n%base { a: 1 }\n.a-#{$n} { @include pad($gap); @extend %base; }",
            &[
                ("// note", Kind::Comment),
                ("$gap", Kind::Label),
                ("4px", Kind::Number),
                ("default", Kind::Keyword),
                ("@mixin", Kind::Keyword),
                ("pad", Kind::Function),
                ("$x", Kind::Label),
                ("%base", Kind::Type),
                ("#{", Kind::Punctuation),
                ("$n", Kind::Label),
                ("@include", Kind::Keyword),
            ],
        );
    }

    #[test]
    fn handles_unicode_crlf_and_broken_input() {
        let text = ".ключ::after {\r\n  content: \"日本\";\r\n  color: red\r\n";
        assert_contains(
            text,
            &[(".ключ", Kind::Type), ("::after", Kind::Macro), ("\"日本\"", Kind::String), ("red", Kind::Constant)],
        );
        assert_well_formed(text);
        assert_well_formed("a { b: c } } } { { : ; ! ( ) [ ] url(");
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "a", ".", "#", ":", "::", "{", "}", ";", "(", ")", "[", "]", "=", "!", "@m", "/*", "*/", "'", "\"", "\\",
            "url(", "1", "--x", "é", "日", "\n", "$a", "#{", "//", "%",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    assert_well_formed(&format!("{a}{b}{c}"));
                }
            }
        }
    }
}
