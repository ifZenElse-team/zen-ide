use crate::data::VOID;
use crate::lexer::{Tok, Token, is_space, lex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Element {
    pub(crate) name: String,
    pub(crate) id: Option<String>,
    pub(crate) classes: Vec<String>,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) children: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tree {
    pub(crate) elements: Vec<Element>,
    pub(crate) roots: Vec<usize>,
    pub(crate) open: Vec<usize>,
}

const MAX_END_TAG_SEARCH: usize = 256;

const MAX_OUTLINE_DEPTH: usize = 32;

const P_CLOSERS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "dialog",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "search",
    "section",
    "table",
    "ul",
    "li",
    "dt",
    "dd",
];

fn implicitly_closed(open: &str, incoming: &str) -> bool {
    match open {
        "p" => P_CLOSERS.contains(&incoming),
        "li" => incoming == "li",
        "dt" | "dd" => matches!(incoming, "dt" | "dd"),
        "option" => matches!(incoming, "option" | "optgroup"),
        "optgroup" => incoming == "optgroup",
        "tr" => matches!(incoming, "tr" | "tbody" | "thead" | "tfoot"),
        "td" | "th" => matches!(incoming, "td" | "th" | "tr" | "tbody" | "thead" | "tfoot"),
        "thead" | "tbody" => matches!(incoming, "tbody" | "tfoot"),
        "rt" | "rp" => matches!(incoming, "rt" | "rp"),
        _ => false,
    }
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    match bytes.first() {
        Some(&quote @ (b'"' | b'\'')) => {
            let inner = &value[1..];
            inner.strip_suffix(quote as char).unwrap_or(inner)
        }
        _ => value,
    }
}

struct Builder<'a> {
    text: &'a str,
    tree: Tree,
    last: Vec<usize>,
}

impl Builder<'_> {
    fn note_content(&mut self, end: usize) {
        if let Some(last) = self.last.last_mut() {
            *last = (*last).max(end);
        }
    }

    fn close_top(&mut self, end: Option<usize>) {
        let Some(index) = self.tree.open.pop() else { return };
        let last = self.last.pop().unwrap_or(end.unwrap_or(0));
        let end = end.unwrap_or(last);
        self.tree.elements[index].end = end;
        self.note_content(end);
    }

    fn start_tag(&mut self, tokens: &[Token], at: usize) -> usize {
        let name_token = tokens[at + 1];
        let name = &self.text[name_token.start..name_token.end];
        let lower = name.to_ascii_lowercase();
        while let Some(&top) = self.tree.open.last() {
            if !implicitly_closed(&self.tree.elements[top].name.to_ascii_lowercase(), &lower) {
                break;
            }
            self.close_top(None);
        }
        let mut element = Element {
            name: name.to_owned(),
            id: None,
            classes: Vec::new(),
            start: tokens[at].start,
            end: name_token.end,
            children: Vec::new(),
        };
        let mut i = at + 2;
        let mut self_closing = false;
        let mut attribute: Option<String> = None;
        while let Some(token) = tokens.get(i) {
            match token.kind {
                Tok::AttrName => attribute = Some(self.text[token.start..token.end].to_ascii_lowercase()),
                Tok::Eq => {}
                Tok::AttrValue => {
                    let value = unquote(&self.text[token.start..token.end]);
                    match attribute.as_deref() {
                        Some("id") if !value.is_empty() => element.id = Some(value.to_owned()),
                        Some("class") => element.classes = value.split_ascii_whitespace().map(str::to_owned).collect(),
                        _ => {}
                    }
                    attribute = None;
                }
                Tok::Gt => {
                    element.end = token.end;
                    i += 1;
                    break;
                }
                Tok::SlashGt => {
                    element.end = token.end;
                    self_closing = true;
                    i += 1;
                    break;
                }
                _ => break,
            }
            element.end = token.end;
            i += 1;
        }
        let index = self.tree.elements.len();
        let end = element.end;
        self.tree.elements.push(element);
        match self.tree.open.last() {
            Some(&parent) => self.tree.elements[parent].children.push(index),
            None => self.tree.roots.push(index),
        }
        if self_closing || VOID.contains(&lower.as_str()) {
            self.note_content(end);
        } else {
            self.tree.open.push(index);
            self.last.push(end);
        }
        i
    }

    fn end_tag(&mut self, tokens: &[Token], at: usize) -> usize {
        let name_token = tokens[at + 1];
        let name = &self.text[name_token.start..name_token.end];
        let mut i = at + 2;
        let mut end = name_token.end;
        while let Some(token) = tokens.get(i) {
            if !matches!(token.kind, Tok::AttrName | Tok::Eq | Tok::AttrValue | Tok::Gt | Tok::SlashGt) {
                break;
            }
            end = token.end;
            i += 1;
            if matches!(token.kind, Tok::Gt | Tok::SlashGt) {
                break;
            }
        }
        let searched = self.tree.open.len().saturating_sub(MAX_END_TAG_SEARCH);
        if let Some(depth) = self.tree.open[searched..]
            .iter()
            .rposition(|&open| self.tree.elements[open].name.eq_ignore_ascii_case(name))
            .map(|depth| depth + searched)
        {
            while self.tree.open.len() > depth + 1 {
                self.close_top(None);
            }
            self.close_top(Some(end));
        }
        i
    }
}

pub(crate) fn build(text: &str, tokens: &[Token]) -> Tree {
    let mut builder = Builder { text, tree: Tree::default(), last: Vec::new() };
    let mut i = 0;
    while let Some(token) = tokens.get(i) {
        let has_name = tokens.get(i + 1).is_some_and(|next| next.kind == Tok::TagName && next.start == token.end);
        i = match token.kind {
            Tok::Lt if has_name => builder.start_tag(tokens, i),
            Tok::LtSlash if has_name => builder.end_tag(tokens, i),
            Tok::Text => {
                let trimmed =
                    text[token.start..token.end].trim_end_matches(|ch: char| ch.is_ascii() && is_space(ch as u8));
                if !trimmed.is_empty() {
                    builder.note_content(token.start + trimmed.len());
                }
                i + 1
            }
            _ => {
                builder.note_content(token.end);
                i + 1
            }
        };
    }
    let open = builder.tree.open.clone();
    while !builder.tree.open.is_empty() {
        builder.close_top(None);
    }
    builder.tree.open = open;
    builder.tree
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

fn line_of(line_starts: &[usize], offset: usize) -> u32 {
    (line_starts.partition_point(|&start| start <= offset) - 1) as u32
}

pub fn fold(text: &str) -> Vec<Fold> {
    let (tokens, _) = lex(text);
    let tree = build(text, &tokens);
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.bytes().enumerate().filter(|(_, byte)| *byte == b'\n').map(|(at, _)| at + 1))
        .collect();
    let ranges = tree
        .elements
        .iter()
        .map(|element| (element.start, element.end))
        .chain(tokens.iter().filter(|token| token.kind == Tok::Comment).map(|token| (token.start, token.end)));
    let mut folds: Vec<Fold> = ranges
        .filter(|(start, end)| end > start)
        .map(|(start, end)| Fold { start_line: line_of(&line_starts, start), end_line: line_of(&line_starts, end - 1) })
        .filter(|fold| fold.end_line > fold.start_line)
        .collect();
    folds.sort();
    folds.dedup();
    folds
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

fn symbol(tree: &Tree, index: usize, depth: usize) -> Symbol {
    let element = &tree.elements[index];
    let mut name = element.name.clone();
    if let Some(id) = &element.id {
        name.push('#');
        name.push_str(id);
    }
    for class in &element.classes {
        name.push('.');
        name.push_str(class);
    }
    Symbol {
        name,
        kind: "element",
        start: element.start,
        end: element.end,
        children: if depth + 1 < MAX_OUTLINE_DEPTH {
            element.children.iter().map(|&child| symbol(tree, child, depth + 1)).collect()
        } else {
            Vec::new()
        },
    }
}

pub fn outline(text: &str) -> Vec<Symbol> {
    let (tokens, _) = lex(text);
    let tree = build(text, &tokens);
    tree.roots.iter().map(|&root| symbol(&tree, root, 0)).collect()
}
