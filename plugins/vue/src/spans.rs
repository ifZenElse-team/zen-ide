use crate::lexer::{Tok, Token, entity_len, lex};
use crate::sfc::{Block, Content, blocks};
use crate::{script, with_style_dialect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Comment = 3,
    Macro = 5,
    Type = 6,
    Attribute = 9,
    Constant = 10,
    Punctuation = 11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: u32,
}

struct Spans(Vec<Span>);

impl Spans {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        self.raw(start, end, kind as u32);
    }

    fn raw(&mut self, start: usize, end: usize, kind: u32) {
        if end > start && self.0.last().is_none_or(|last| last.end <= start) {
            self.0.push(Span { start, end, kind });
        }
    }
}

pub(crate) fn expression_dialect(blocks: &[Block]) -> script::Dialect {
    if blocks.iter().any(|block| block.content == Content::TypeScript) {
        script::Dialect::TypeScript
    } else {
        script::Dialect::JavaScript
    }
}

pub fn highlight(text: &str) -> Vec<Span> {
    let (tokens, _) = lex(text);
    let blocks = blocks(text, &tokens);
    let dialect = expression_dialect(&blocks);
    let mut spans = Spans(Vec::with_capacity(tokens.len()));
    let mut index = 0;
    for block in &blocks {
        markup(text, &tokens[index.min(block.tokens.0)..block.tokens.0], None, &mut spans);
        let inner = &text[block.inner.0..block.inner.1];
        match block.content {
            Content::Template => markup(text, &tokens[block.tokens.0..block.tokens.1], Some(dialect), &mut spans),
            Content::JavaScript | Content::TypeScript => {
                let dialect = if block.content == Content::TypeScript {
                    script::Dialect::TypeScript
                } else {
                    script::Dialect::JavaScript
                };
                for span in script::highlight(inner, dialect) {
                    spans.raw(block.inner.0 + span.start, block.inner.0 + span.end, span.kind as u32);
                }
            }
            Content::Css | Content::Scss => {
                for span in with_style_dialect(block.content, || crate::highlight::highlight(inner)) {
                    spans.raw(block.inner.0 + span.start, block.inner.0 + span.end, span.kind as u32);
                }
            }
            Content::Other => {}
        }
        index = block.tokens.1;
    }
    markup(text, &tokens[index.min(tokens.len())..], None, &mut spans);
    spans.0
}

fn markup(text: &str, tokens: &[Token], dialect: Option<script::Dialect>, spans: &mut Spans) {
    let mut directive = false;
    for token in tokens {
        let (start, end) = (token.start, token.end);
        match token.kind {
            Tok::Lt | Tok::LtSlash | Tok::Gt | Tok::SlashGt | Tok::Eq => spans.push(start, end, Kind::Punctuation),
            Tok::TagName if dialect.is_some() && is_component(&text[start..end]) => spans.push(start, end, Kind::Type),
            Tok::TagName => spans.push(start, end, Kind::Keyword),
            Tok::AttrName => {
                directive = dialect.is_some() && is_directive(&text[start..end]);
                if directive {
                    directive_name(text, start, end, dialect.unwrap_or(script::Dialect::JavaScript), spans);
                } else {
                    spans.push(start, end, Kind::Attribute);
                }
            }
            Tok::AttrValue => match dialect {
                Some(dialect) if directive => expression_value(text, start, end, dialect, spans),
                _ => value(text.as_bytes(), start, end, spans),
            },
            Tok::Entity => spans.push(start, end, Kind::Constant),
            Tok::Comment => spans.push(start, end, Kind::Comment),
            Tok::Doctype | Tok::Cdata | Tok::Pi => spans.push(start, end, Kind::Macro),
            Tok::Text => {
                if let Some(dialect) = dialect {
                    interpolations(text, start, end, dialect, spans);
                }
            }
            Tok::RawText => {}
        }
    }
}

const BUILT_IN_COMPONENTS: &[&str] =
    &["component", "transition", "transition-group", "keep-alive", "teleport", "suspense"];

pub(crate) fn is_component(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase()) || name.contains('.') || BUILT_IN_COMPONENTS.contains(&name)
}

pub(crate) fn is_directive(name: &str) -> bool {
    name.starts_with("v-") || name.starts_with([':', '@', '#'])
}

fn directive_name(text: &str, start: usize, end: usize, dialect: script::Dialect, spans: &mut Spans) {
    let name = &text[start..end];
    let head = if name.starts_with("v-") { name.find([':', '.']).unwrap_or(name.len()) } else { 1 };
    spans.push(start, start + head, Kind::Keyword);
    let mut at = start + head;
    if name.starts_with("v-") && text[at..end].starts_with(':') {
        spans.push(at, at + 1, Kind::Punctuation);
        at += 1;
    }
    if text[at..end].starts_with('[')
        && let Some(close) = text[at..end].find(']')
    {
        spans.push(at, at + 1, Kind::Punctuation);
        expression(text, at + 1, at + close, dialect, spans);
        spans.push(at + close, at + close + 1, Kind::Punctuation);
        at += close + 1;
    }
    while at < end {
        let next = text[at..end].char_indices().skip(1).find(|&(_, c)| c == '.').map_or(end, |(dot, _)| at + dot);
        if text[at..].starts_with('.') {
            spans.push(at, at + 1, Kind::Punctuation);
            spans.push(at + 1, next, Kind::Attribute);
        } else {
            spans.push(at, next, Kind::Attribute);
        }
        at = next;
    }
}

fn expression_value(text: &str, start: usize, end: usize, dialect: script::Dialect, spans: &mut Spans) {
    let value = &text[start..end];
    if !value.starts_with(['"', '\'']) {
        expression(text, start, end, dialect, spans);
        return;
    }
    let closed = value.len() > 1 && value.ends_with(&value[..1]);
    let inner_end = if closed { end - 1 } else { end };
    spans.push(start, start + 1, Kind::String);
    expression(text, start + 1, inner_end, dialect, spans);
    if closed {
        spans.push(end - 1, end, Kind::String);
    }
}

fn expression(text: &str, start: usize, end: usize, dialect: script::Dialect, spans: &mut Spans) {
    for span in script::highlight(&text[start..end], dialect) {
        spans.raw(start + span.start, start + span.end, span.kind as u32);
    }
}

fn interpolations(text: &str, start: usize, end: usize, dialect: script::Dialect, spans: &mut Spans) {
    let mut at = start;
    while let Some(open) = text[at..end].find("{{").map(|offset| at + offset) {
        let close = text[open + 2..end].find("}}").map(|offset| open + 2 + offset);
        spans.push(open, open + 2, Kind::Macro);
        let inner_end = close.unwrap_or(end);
        expression(text, open + 2, inner_end, dialect, spans);
        let Some(close) = close else { return };
        spans.push(close, close + 2, Kind::Macro);
        at = close + 2;
    }
}

fn value(bytes: &[u8], start: usize, end: usize, spans: &mut Spans) {
    let mut from = start;
    let mut i = start;
    while i < end {
        if let Some(len) = entity_len(&bytes[i..end]) {
            spans.push(from, i, Kind::String);
            spans.push(i, i + len, Kind::Constant);
            i += len;
            from = i;
        } else {
            i += 1;
        }
    }
    spans.push(from, end, Kind::String);
}
