use crate::lexer::{Tok, Token, lex};
use crate::sfc::{Block, Content, blocks};
use crate::{script, with_style_dialect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

const MAX_SYMBOLS: usize = 20_000;
const MAX_DEPTH: usize = 30;

struct Lines(Vec<usize>);

impl Lines {
    fn new(text: &str) -> Self {
        Self(std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect())
    }

    fn of(&self, offset: usize) -> u32 {
        (self.0.partition_point(|&start| start <= offset) - 1) as u32
    }
}

pub fn fold(text: &str) -> Vec<Fold> {
    let (tokens, _) = lex(text);
    let blocks = blocks(text, &tokens);
    let lines = Lines::new(text);
    let mut folds = Vec::new();
    let add = |folds: &mut Vec<Fold>, start: usize, end: usize| {
        let (start_line, end_line) = (lines.of(start), lines.of(end));
        if end_line > start_line {
            folds.push(Fold { start_line, end_line });
        }
    };
    for token in tokens.iter().filter(|token| token.kind == Tok::Comment) {
        add(&mut folds, token.start, token.end.saturating_sub(1));
    }
    for block in &blocks {
        add(&mut folds, block.start, block.end.saturating_sub(1));
        let inner = &text[block.inner.0..block.inner.1];
        let shift = lines.of(block.inner.0);
        let shifted =
            |start_line: u32, end_line: u32| Fold { start_line: start_line + shift, end_line: end_line + shift };
        match block.content {
            Content::Template => {
                for element in elements(text, &tokens[block.tokens.0..block.tokens.1]) {
                    add(&mut folds, element.start, element.end_tag.saturating_sub(1).max(element.start));
                }
            }
            Content::JavaScript | Content::TypeScript => {
                for fold in script::fold(inner, script_dialect(block)) {
                    folds.push(shifted(fold.start_line, fold.end_line));
                }
            }
            Content::Css | Content::Scss => {
                for fold in with_style_dialect(block.content, || crate::structure::fold(inner)) {
                    folds.push(shifted(fold.start_line, fold.end_line));
                }
            }
            Content::Other => {}
        }
    }
    folds.sort_unstable();
    folds.dedup();
    folds
}

pub fn outline(text: &str) -> Vec<Symbol> {
    let (tokens, _) = lex(text);
    let mut budget = MAX_SYMBOLS;
    let mut symbols = Vec::new();
    for block in blocks(text, &tokens) {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let inner = &text[block.inner.0..block.inner.1];
        let base = block.inner.0;
        let children = match block.content {
            Content::Template => tree(&elements(text, &tokens[block.tokens.0..block.tokens.1]), 0, &mut budget),
            Content::JavaScript | Content::TypeScript => {
                let found = script::outline(inner, script_dialect(&block));
                script_symbols(&found, base, 0, &mut budget)
            }
            Content::Css | Content::Scss => {
                let found = with_style_dialect(block.content, || crate::structure::outline(inner));
                style_symbols(&found, base, 0, &mut budget)
            }
            Content::Other => Vec::new(),
        };
        symbols.push(Symbol { name: block.label(), kind: "module", start: block.start, end: block.end, children });
    }
    symbols
}

fn script_dialect(block: &Block) -> script::Dialect {
    if block.content == Content::TypeScript { script::Dialect::TypeScript } else { script::Dialect::JavaScript }
}

fn script_symbols(found: &[script::Symbol], base: usize, depth: usize, budget: &mut usize) -> Vec<Symbol> {
    let mut out = Vec::new();
    for symbol in found {
        if *budget == 0 || depth >= MAX_DEPTH {
            break;
        }
        *budget -= 1;
        let children = script_symbols(&symbol.children, base, depth + 1, budget);
        out.push(Symbol {
            name: symbol.name.clone(),
            kind: symbol.kind.as_str(),
            start: base + symbol.start,
            end: base + symbol.end,
            children,
        });
    }
    out
}

fn style_symbols(found: &[crate::structure::Symbol], base: usize, depth: usize, budget: &mut usize) -> Vec<Symbol> {
    let mut out = Vec::new();
    for symbol in found {
        if *budget == 0 || depth >= MAX_DEPTH {
            break;
        }
        *budget -= 1;
        let children = style_symbols(&symbol.children, base, depth + 1, budget);
        out.push(Symbol {
            name: symbol.name.clone(),
            kind: symbol.kind,
            start: base + symbol.start,
            end: base + symbol.end,
            children,
        });
    }
    out
}

#[derive(Clone, Debug)]
struct Element {
    name: String,
    start: usize,
    end_tag: usize,
    end: usize,
    parent: Option<usize>,
}

fn elements(text: &str, tokens: &[Token]) -> Vec<Element> {
    let mut elements: Vec<Element> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        let named = tokens.get(index + 1).filter(|next| next.kind == Tok::TagName);
        match (token.kind, named) {
            (Tok::Lt, Some(name_token)) => {
                let tag = &text[name_token.start..name_token.end];
                let mut name = tag.to_owned();
                let mut close = index + 2;
                let mut attribute = None;
                while close < tokens.len() && matches!(tokens[close].kind, Tok::AttrName | Tok::Eq | Tok::AttrValue) {
                    let current = tokens[close];
                    match current.kind {
                        Tok::AttrName => attribute = Some(&text[current.start..current.end]),
                        Tok::AttrValue if attribute == Some("id") => {
                            name.push('#');
                            name.push_str(crate::sfc::unquote(&text[current.start..current.end]));
                        }
                        _ => {}
                    }
                    close += 1;
                }
                let end_of_tag = tokens.get(close).filter(|token| matches!(token.kind, Tok::Gt | Tok::SlashGt));
                let end = end_of_tag.map_or(name_token.end, |token| token.end);
                let leaf = end_of_tag.is_none_or(|token| token.kind == Tok::SlashGt)
                    || VOID.iter().any(|void| tag.eq_ignore_ascii_case(void));
                elements.push(Element { name, start: token.start, end_tag: end, end, parent: open.last().copied() });
                if !leaf {
                    open.push(elements.len() - 1);
                }
                index = close;
            }
            (Tok::LtSlash, Some(name_token)) => {
                let tag = &text[name_token.start..name_token.end];
                let gt = tokens.get(index + 2).filter(|token| token.kind == Tok::Gt);
                let end = gt.map_or(name_token.end, |token| token.end);
                if let Some(depth) = open.iter().rposition(|&element| {
                    let name = &elements[element].name;
                    name.split('#').next().is_some_and(|own| own.eq_ignore_ascii_case(tag))
                }) {
                    for &unclosed in &open[depth + 1..] {
                        elements[unclosed].end_tag = token.start;
                        elements[unclosed].end = token.start;
                    }
                    let element = open[depth];
                    elements[element].end_tag = token.start;
                    elements[element].end = end;
                    open.truncate(depth);
                }
                index += 2;
            }
            _ => index += 1,
        }
    }
    let last = tokens.last().map_or(0, |token| token.end);
    for unclosed in open {
        elements[unclosed].end_tag = last;
        elements[unclosed].end = last;
    }
    elements
}

fn tree(elements: &[Element], depth: usize, budget: &mut usize) -> Vec<Symbol> {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); elements.len()];
    let mut roots = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        match element.parent {
            Some(parent) => children[parent].push(index),
            None => roots.push(index),
        }
    }
    fn build(
        elements: &[Element],
        children: &[Vec<usize>],
        list: &[usize],
        depth: usize,
        budget: &mut usize,
    ) -> Vec<Symbol> {
        let mut out = Vec::new();
        for &index in list {
            if *budget == 0 || depth >= MAX_DEPTH {
                break;
            }
            *budget -= 1;
            let element = &elements[index];
            out.push(Symbol {
                name: element.name.clone(),
                kind: "element",
                start: element.start,
                end: element.end,
                children: build(elements, children, &children[index], depth + 1, budget),
            });
        }
        out
    }
    build(elements, &children, &roots, depth, budget)
}
