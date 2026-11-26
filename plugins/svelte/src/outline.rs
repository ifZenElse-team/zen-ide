use crate::lexer::Tok;
use crate::markup::{Markup, Node, NodeKind};
use crate::sfc::{Block, Content};
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
    let markup = Markup::read(text);
    let lines = Lines::new(text);
    let mut folds = Vec::new();
    let add = |folds: &mut Vec<Fold>, start: usize, end: usize| {
        let (start_line, end_line) = (lines.of(start), lines.of(end));
        if end_line > start_line {
            folds.push(Fold { start_line, end_line });
        }
    };
    for token in markup.tokens.iter().filter(|token| token.kind == Tok::Comment) {
        add(&mut folds, token.start, token.end.saturating_sub(1));
    }
    for node in &markup.nodes {
        add(&mut folds, node.start, node.end.saturating_sub(1).max(node.start));
    }
    for block in &markup.blocks {
        add(&mut folds, block.start, block.end.saturating_sub(1));
        let inner = &text[block.inner.0..block.inner.1];
        let shift = lines.of(block.inner.0);
        let shifted =
            |fold_start: u32, fold_end: u32| Fold { start_line: fold_start + shift, end_line: fold_end + shift };
        match block.content {
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
            Content::Template | Content::Other => {}
        }
    }
    folds.sort_unstable();
    folds.dedup();
    folds
}

pub fn outline(text: &str) -> Vec<Symbol> {
    let markup = Markup::read(text);
    let mut budget = MAX_SYMBOLS;
    let mut entries: Vec<Symbol> = Vec::new();
    for block in &markup.blocks {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let inner = &text[block.inner.0..block.inner.1];
        let base = block.inner.0;
        let children = match block.content {
            Content::JavaScript | Content::TypeScript => {
                script_symbols(&script::outline(inner, script_dialect(block)), base, 0, &mut budget)
            }
            Content::Css | Content::Scss => {
                let found = with_style_dialect(block.content, || crate::structure::outline(inner));
                style_symbols(&found, base, 0, &mut budget)
            }
            Content::Template | Content::Other => Vec::new(),
        };
        entries.push(Symbol { name: label(text, block), kind: "module", start: block.start, end: block.end, children });
    }
    entries.extend(tree(&markup.nodes, &mut budget));
    entries.sort_by_key(|symbol| symbol.start);
    entries
}

pub(crate) fn label(text: &str, block: &Block) -> String {
    let mut label = block.name.clone();
    let open = &text[block.start..block.inner.0];
    if block.module || open.contains("context=\"module\"") || open.contains("context='module'") {
        label.push_str(" module");
    }
    if let Some(lang) = &block.lang {
        label.push_str(&format!(" lang=\"{lang}\""));
    }
    label
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

fn tree(nodes: &[Node], budget: &mut usize) -> Vec<Symbol> {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        match node.parent {
            Some(parent) => children[parent].push(index),
            None => roots.push(index),
        }
    }
    fn build(nodes: &[Node], children: &[Vec<usize>], list: &[usize], depth: usize, budget: &mut usize) -> Vec<Symbol> {
        let mut out = Vec::new();
        for &index in list {
            if *budget == 0 || depth >= MAX_DEPTH {
                break;
            }
            *budget -= 1;
            let node = &nodes[index];
            out.push(Symbol {
                name: node.label.clone(),
                kind: if node.kind == NodeKind::Element { "element" } else { "module" },
                start: node.start,
                end: node.end.max(node.open_end),
                children: build(nodes, children, &children[index], depth + 1, budget),
            });
        }
        out
    }
    build(nodes, &children, &roots, 0, budget)
}
