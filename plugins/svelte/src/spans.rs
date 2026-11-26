use crate::lexer::{Tok, Token, entity_len};
use crate::markup::{Markup, block_tag};
use crate::sfc::{Block, Content};
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

pub(crate) const RUNES: &[&str] = &["$state", "$derived", "$effect", "$props", "$bindable", "$inspect", "$host"];

pub(crate) const DIRECTIVES: &[&str] =
    &["on", "bind", "class", "style", "use", "transition", "in", "out", "animate", "let"];

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
    let markup = Markup::read(text);
    let dialect = expression_dialect(&markup.blocks);
    let mut spans = Spans(Vec::with_capacity(markup.tokens.len()));
    let mut blocks = markup.blocks.iter().peekable();
    let mut skip_to = 0;
    for token in &markup.tokens {
        if token.start < skip_to {
            continue;
        }
        if let Some(block) = blocks.peek()
            && token.start >= block.inner.0
            && block.inner.0 < block.inner.1
        {
            code(text, block, &mut spans);
            skip_to = block.inner.1;
            blocks.next();
            if token.start < skip_to {
                continue;
            }
        }
        markup_token(text, &markup, token, dialect, &mut spans);
    }
    spans.0
}

fn markup_token(text: &str, markup: &Markup, token: &Token, dialect: script::Dialect, spans: &mut Spans) {
    let (start, end) = (token.start, token.end);
    match token.kind {
        Tok::Lt | Tok::LtSlash | Tok::Gt | Tok::SlashGt | Tok::Eq => spans.push(start, end, Kind::Punctuation),
        Tok::TagName => {
            let name = &text[start..end];
            let component = name.starts_with(|c: char| c.is_ascii_uppercase()) || name.contains('.');
            spans.push(start, end, if component { Kind::Type } else { Kind::Keyword });
        }
        Tok::AttrName => {
            let name = &text[start..end];
            if name.starts_with('{') {
                regions(text, markup, start, end, dialect, None, spans);
            } else if let Some((prefix, _)) = name.split_once(':')
                && DIRECTIVES.contains(&prefix)
            {
                directive_name(text, start, end, prefix.len(), spans);
            } else {
                spans.push(start, end, Kind::Attribute);
            }
        }
        Tok::AttrValue => regions(text, markup, start, end, dialect, Some(Kind::String), spans),
        Tok::Text => regions(text, markup, start, end, dialect, None, spans),
        Tok::Entity => spans.push(start, end, Kind::Constant),
        Tok::Comment => spans.push(start, end, Kind::Comment),
        Tok::Doctype | Tok::Cdata | Tok::Pi => spans.push(start, end, Kind::Macro),
        Tok::RawText => {}
    }
}

fn directive_name(text: &str, start: usize, end: usize, prefix: usize, spans: &mut Spans) {
    spans.push(start, start + prefix, Kind::Keyword);
    spans.push(start + prefix, start + prefix + 1, Kind::Punctuation);
    let mut at = start + prefix + 1;
    while at < end {
        let next = text[at..end].find('|').map_or(end, |bar| at + bar);
        spans.push(at, next, Kind::Attribute);
        if next < end {
            spans.push(next, next + 1, Kind::Punctuation);
        }
        at = next + 1;
    }
}

fn regions(
    text: &str,
    markup: &Markup,
    start: usize,
    end: usize,
    dialect: script::Dialect,
    between: Option<Kind>,
    spans: &mut Spans,
) {
    let first = markup.regions.partition_point(|region| region.1 <= start);
    let mut at = start;
    for &(region_start, region_end) in markup.regions[first..].iter().take_while(|region| region.0 < end) {
        if region_start < at || region_end > end {
            continue;
        }
        plain(text, at, region_start, between, spans);
        expression_region(text, (region_start, region_end), dialect, spans);
        at = region_end;
    }
    plain(text, at, end, between, spans);
}

fn plain(text: &str, start: usize, end: usize, kind: Option<Kind>, spans: &mut Spans) {
    let Some(kind) = kind else { return };
    let bytes = text.as_bytes();
    let mut from = start;
    let mut i = start;
    while i < end {
        if let Some(len) = entity_len(&bytes[i..end]) {
            spans.push(from, i, kind);
            spans.push(i, i + len, Kind::Constant);
            i += len;
            from = i;
        } else {
            i += 1;
        }
    }
    spans.push(from, end, kind);
}

fn expression_region(text: &str, region: (usize, usize), dialect: script::Dialect, spans: &mut Spans) {
    let (start, end) = region;
    spans.push(start, start + 1, Kind::Macro);
    let mut body = start + 1;
    if let Some(tag) = block_tag(text, region) {
        spans.push(tag.word_range.0, tag.word_range.1, Kind::Keyword);
        body = tag.word_range.1;
    }
    let inner_end = end - 1;
    let each = block_tag(text, region).is_some_and(|tag| tag.sigil == b'#' && tag.word == "each");
    let split = each.then(|| text[body..inner_end].find(" as ").map(|at| body + at)).flatten();
    match split {
        Some(at) => {
            script_part(text, body, at, dialect, spans);
            spans.push(at + 1, at + 3, Kind::Keyword);
            script_part(text, at + 3, inner_end, dialect, spans);
        }
        None => script_part(text, body, inner_end, dialect, spans),
    }
    spans.push(inner_end, end, Kind::Macro);
}

fn script_part(text: &str, start: usize, end: usize, dialect: script::Dialect, spans: &mut Spans) {
    for span in script::highlight(&text[start..end], dialect) {
        spans.raw(start + span.start, start + span.end, span.kind as u32);
    }
}

fn code(text: &str, block: &Block, spans: &mut Spans) {
    let inner = &text[block.inner.0..block.inner.1];
    let base = block.inner.0;
    match block.content {
        Content::JavaScript | Content::TypeScript => {
            let dialect = if block.content == Content::TypeScript {
                script::Dialect::TypeScript
            } else {
                script::Dialect::JavaScript
            };
            for span in script_spans(inner, dialect) {
                spans.raw(base + span.start, base + span.end, span.kind);
            }
        }
        Content::Css | Content::Scss => {
            for span in with_style_dialect(block.content, || crate::highlight::highlight(inner)) {
                spans.raw(base + span.start, base + span.end, span.kind as u32);
            }
        }
        Content::Template | Content::Other => {}
    }
}

pub(crate) fn script_spans(source: &str, dialect: script::Dialect) -> Vec<Span> {
    let runes = rune_ranges(source);
    let mut spans: Vec<Span> = script::highlight(source, dialect)
        .into_iter()
        .filter(|span| !runes.iter().any(|rune| span.start < rune.1 && rune.0 < span.end))
        .map(|span| Span { start: span.start, end: span.end, kind: span.kind as u32 })
        .collect();
    spans.extend(runes.into_iter().map(|(start, end)| Span { start, end, kind: Kind::Macro as u32 }));
    spans.sort_by_key(|span| span.start);
    spans
}

fn rune_ranges(source: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = source[at..].find('$') {
        let start = at + offset;
        at = start + 1;
        if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || matches!(bytes[start - 1], b'_' | b'$' | b'.')) {
            continue;
        }
        let Some(rune) = RUNES.iter().find(|rune| source[start..].starts_with(**rune)) else { continue };
        let mut end = start + rune.len();
        if bytes.get(end) == Some(&b'.') {
            let member = source[end + 1..].bytes().take_while(u8::is_ascii_alphabetic).count();
            end += 1 + member;
        }
        let after = source[end..].trim_start();
        if after.starts_with('(')
            && !bytes.get(start + rune.len()).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            found.push((start, end));
            at = end;
        }
    }
    found
}
