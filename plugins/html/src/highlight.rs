use crate::lexer::{Tok, entity_len, lex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Comment = 3,
    Macro = 5,
    Attribute = 9,
    Constant = 10,
    Punctuation = 11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub fn highlight(text: &str) -> Vec<Span> {
    let (tokens, _) = lex(text);
    let mut spans = Vec::with_capacity(tokens.len());
    for token in tokens {
        let kind = match token.kind {
            Tok::Lt | Tok::LtSlash | Tok::Gt | Tok::SlashGt | Tok::Eq => Kind::Punctuation,
            Tok::TagName => Kind::Keyword,
            Tok::AttrName => Kind::Attribute,
            Tok::Entity => Kind::Constant,
            Tok::Comment => Kind::Comment,
            Tok::Doctype | Tok::Cdata | Tok::Pi => Kind::Macro,
            Tok::AttrValue => {
                value_spans(text.as_bytes(), token.start, token.end, &mut spans);
                continue;
            }
            Tok::Text | Tok::RawText => continue,
        };
        spans.push(Span { start: token.start, end: token.end, kind });
    }
    spans
}

fn value_spans(bytes: &[u8], start: usize, end: usize, spans: &mut Vec<Span>) {
    let mut from = start;
    let mut i = start;
    while i < end {
        if let Some(len) = entity_len(&bytes[i..end]) {
            if i > from {
                spans.push(Span { start: from, end: i, kind: Kind::String });
            }
            spans.push(Span { start: i, end: i + len, kind: Kind::Constant });
            i += len;
            from = i;
        } else {
            i += 1;
        }
    }
    if end > from {
        spans.push(Span { start: from, end, kind: Kind::String });
    }
}
