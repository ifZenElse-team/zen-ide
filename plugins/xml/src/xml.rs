use std::ops::Range;

use crate::lexer::{End, Tok, Token, entity_len, lex_xml};

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
    pub kind: Kind,
}

pub fn highlight(text: &str) -> Vec<Span> {
    let (tokens, _) = lex_xml(text);
    let mut spans = Vec::with_capacity(tokens.len());
    for token in tokens {
        let kind = match token.kind {
            Tok::Lt | Tok::LtSlash | Tok::Gt | Tok::SlashGt | Tok::Eq => Kind::Punctuation,
            Tok::TagName => {
                prefixed(text, token, Kind::Keyword, &mut spans);
                continue;
            }
            Tok::AttrName => {
                prefixed(text, token, Kind::Attribute, &mut spans);
                continue;
            }
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

fn prefixed(text: &str, token: Token, kind: Kind, spans: &mut Vec<Span>) {
    let name = &text[token.start..token.end];
    match name.find(':').filter(|colon| *colon > 0 && colon + 1 < name.len()) {
        Some(colon) => {
            let colon = token.start + colon;
            spans.push(Span { start: token.start, end: colon, kind: Kind::Type });
            spans.push(Span { start: colon, end: colon + 1, kind: Kind::Punctuation });
            spans.push(Span { start: colon + 1, end: token.end, kind });
        }
        None => spans.push(Span { start: token.start, end: token.end, kind }),
    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Element {
    pub(crate) name: Range<usize>,
    pub(crate) range: Range<usize>,
    pub(crate) id: Option<String>,
    pub(crate) parent: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub range: Range<usize>,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tree {
    pub(crate) elements: Vec<Element>,
    pub(crate) open: Vec<usize>,
    pub(crate) problems: Vec<Problem>,
}

fn tag_end(tokens: &[Token], at: usize) -> Option<usize> {
    let offset = tokens[at + 1..].iter().position(|token| {
        matches!(token.kind, Tok::Gt | Tok::SlashGt | Tok::Lt | Tok::LtSlash | Tok::Text | Tok::Comment)
    })?;
    let end = at + 1 + offset;
    matches!(tokens[end].kind, Tok::Gt | Tok::SlashGt).then_some(end)
}

fn unquoted(text: &str, value: Token) -> String {
    let value = &text[value.start..value.end];
    for quote in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(quote) {
            return inner.strip_suffix(quote).unwrap_or(inner).to_owned();
        }
    }
    value.to_owned()
}

pub(crate) fn tree(text: &str) -> Tree {
    let (tokens, end) = lex_xml(text);
    let mut tree = Tree::default();
    let problem = |range: Range<usize>, message: String| Problem { range, message };
    let mut at = 0;
    while at < tokens.len() {
        let token = tokens[at];
        match token.kind {
            Tok::Lt => {
                let name = tokens.get(at + 1).filter(|next| next.kind == Tok::TagName).copied();
                let Some(name) = name else {
                    at += 1;
                    continue;
                };
                let Some(close) = tag_end(&tokens, at) else {
                    let label = &text[name.start..name.end];
                    tree.problems.push(problem(token.start..name.end, format!("Start tag <{label}> is not finished")));
                    at += 1;
                    continue;
                };
                let mut id = None;
                for index in at + 2..close {
                    if tokens[index].kind == Tok::AttrName
                        && matches!(&text[tokens[index].start..tokens[index].end], "id" | "xml:id")
                        && let Some(value) = tokens.get(index + 2).filter(|value| value.kind == Tok::AttrValue)
                    {
                        id = Some(unquoted(text, *value));
                    }
                }
                let parent = tree.open.last().copied();
                let self_closing = tokens[close].kind == Tok::SlashGt;
                let range_end = if self_closing { tokens[close].end } else { text.len() };
                tree.elements.push(Element { name: name.start..name.end, range: token.start..range_end, id, parent });
                if !self_closing {
                    tree.open.push(tree.elements.len() - 1);
                }
                at = close + 1;
            }
            Tok::LtSlash => {
                let name = tokens.get(at + 1).filter(|next| next.kind == Tok::TagName).copied();
                let Some(name) = name else {
                    at += 1;
                    continue;
                };
                let label = &text[name.start..name.end];
                let close = tag_end(&tokens, at);
                let tag_range = token.start..close.map_or(name.end, |close| tokens[close].end);
                if close.is_none() {
                    tree.problems.push(problem(tag_range.clone(), format!("End tag </{label}> is not finished")));
                }
                let matching = tree.open.iter().rposition(|&open| &text[tree.elements[open].name.clone()] == label);
                match matching {
                    Some(position) => {
                        for &inner in &tree.open[position + 1..] {
                            let inner_name = &text[tree.elements[inner].name.clone()];
                            tree.problems.push(problem(
                                tree.elements[inner].name.clone(),
                                format!("Element <{inner_name}> is not closed before </{label}>"),
                            ));
                            tree.elements[inner].range.end = token.start;
                        }
                        let element = tree.open[position];
                        tree.elements[element].range.end = tag_range.end;
                        tree.open.truncate(position);
                    }
                    None => {
                        let message = match tree.open.last() {
                            Some(&open) => format!(
                                "End tag </{label}> does not match the open element <{}>",
                                &text[tree.elements[open].name.clone()]
                            ),
                            None => format!("End tag </{label}> has no start tag"),
                        };
                        tree.problems.push(problem(tag_range.clone(), message));
                    }
                }
                at = close.map_or(at + 2, |close| close + 1);
            }
            Tok::Comment | Tok::Cdata | Tok::Pi if end == End::Opaque && token.end == text.len() => {
                let (opening, what) = match token.kind {
                    Tok::Comment => ("<!--", "Comment"),
                    Tok::Cdata => ("<![CDATA[", "CDATA section"),
                    _ => ("<?", "Processing instruction"),
                };
                let start = token.start;
                let shown = (start + opening.len()).min(text.len());
                tree.problems.push(problem(start..shown, format!("{what} is not closed")));
                at += 1;
            }
            _ => at += 1,
        }
    }
    for &open in &tree.open {
        let name = &text[tree.elements[open].name.clone()];
        tree.problems.push(problem(tree.elements[open].name.clone(), format!("Element <{name}> is not closed")));
    }
    tree.problems.sort_by_key(|problem| problem.range.start);
    tree
}

pub fn problems(text: &str) -> Vec<Problem> {
    tree(text).problems
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

pub fn fold(text: &str) -> Vec<Fold> {
    let line_of = |at: usize| text[..at.min(text.len())].matches('\n').count() as u32;
    let mut folds: Vec<Fold> = Vec::new();
    let mut add = |range: Range<usize>| {
        let last = range.end.saturating_sub(1).max(range.start);
        let (start_line, end_line) = (line_of(range.start), line_of(last));
        if end_line > start_line {
            folds.push(Fold { start_line, end_line });
        }
    };
    for element in tree(text).elements {
        add(element.range);
    }
    for token in lex_xml(text).0 {
        if matches!(token.kind, Tok::Comment | Tok::Cdata | Tok::Doctype) {
            add(token.start..token.end);
        }
    }
    folds.sort();
    folds.dedup();
    folds
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

const MAX_DEPTH: usize = 32;
const MAX_ENTRIES: usize = 20_000;

pub fn outline(text: &str) -> Vec<Symbol> {
    let tree = tree(text);
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); tree.elements.len()];
    let mut roots = Vec::new();
    for (index, element) in tree.elements.iter().enumerate() {
        match element.parent {
            Some(parent) => children[parent].push(index),
            None => roots.push(index),
        }
    }
    let mut count = 0;
    fn build(
        text: &str,
        tree: &Tree,
        children: &[Vec<usize>],
        index: usize,
        depth: usize,
        count: &mut usize,
    ) -> Option<Symbol> {
        if *count >= MAX_ENTRIES {
            return None;
        }
        *count += 1;
        let element = &tree.elements[index];
        let name = &text[element.name.clone()];
        let name = match &element.id {
            Some(id) if !id.is_empty() => format!("{name}#{id}"),
            _ => name.to_owned(),
        };
        let nested = if depth + 1 < MAX_DEPTH {
            children[index].iter().filter_map(|&child| build(text, tree, children, child, depth + 1, count)).collect()
        } else {
            Vec::new()
        };
        Some(Symbol { name, start: element.range.start, end: element.range.end, children: nested })
    }
    roots.into_iter().filter_map(|root| build(text, &tree, &children, root, 0, &mut count)).collect()
}

pub const TRIGGERS: &str = ">/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insertion {
    pub at: usize,
    pub text: String,
}

pub fn on_type(text: &str, caret: usize, typed: char) -> Option<Insertion> {
    if caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let prefix = &text[..caret];
    match typed {
        '>' if prefix.ends_with('>') && !prefix.ends_with("/>") => end_tag(text, caret),
        '/' if prefix.ends_with("</") => open_name(text, caret),
        _ => None,
    }
}

fn end_tag(text: &str, caret: usize) -> Option<Insertion> {
    let (tokens, _) = lex_xml(&text[..caret]);
    let last = tokens.last().filter(|token| token.kind == Tok::Gt && token.end == caret)?;
    let lt =
        tokens.iter().rposition(|token| matches!(token.kind, Tok::Lt | Tok::LtSlash) && token.start < last.start)?;
    if tokens[lt].kind != Tok::Lt {
        return None;
    }
    let name = tokens.get(lt + 1).filter(|token| token.kind == Tok::TagName)?;
    let name = &text[name.start..name.end];
    let (all, _) = lex_xml(text);
    let (before, after): (Vec<Token>, Vec<Token>) = all.into_iter().partition(|token| token.end <= caret);
    let open = balance(text, &before, name);
    let closed_after = -balance(text, &after, name);
    (closed_after < open).then(|| Insertion { at: caret, text: format!("$0</{name}>") })
}

fn balance(text: &str, tokens: &[Token], name: &str) -> i64 {
    let mut balance = 0i64;
    for (at, token) in tokens.iter().enumerate() {
        let opening = match token.kind {
            Tok::Lt => true,
            Tok::LtSlash => false,
            _ => continue,
        };
        let Some(tag) = tokens.get(at + 1).filter(|next| next.kind == Tok::TagName) else { continue };
        if &text[tag.start..tag.end] != name {
            continue;
        }
        if !opening {
            balance -= 1;
            continue;
        }
        let ends =
            tokens[at + 2..].iter().find(|next| matches!(next.kind, Tok::Gt | Tok::SlashGt | Tok::Lt | Tok::LtSlash));
        if ends.is_none_or(|end| end.kind != Tok::SlashGt) {
            balance += 1;
        }
    }
    balance
}

fn open_name(text: &str, caret: usize) -> Option<Insertion> {
    if text[caret..].starts_with(|c: char| c.is_alphanumeric() || matches!(c, '>' | '-' | '_' | ':' | '.')) {
        return None;
    }
    let lt = caret - 2;
    let before = tree(&text[..lt]);
    let open = *before.open.last()?;
    let name = &text[before.elements[open].name.clone()];
    Some(Insertion { at: caret, text: format!("{name}>") })
}
