use crate::token::{Tok, Token, scss, tokenize};
use crate::tree::{Node, NodeKind, at_name, is_grouping, parse};

const MAX_NAME: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

pub fn fold(text: &str) -> Vec<Fold> {
    let tokens = tokenize(text);
    let nodes = parse(text, &tokens);
    let lines: Vec<usize> =
        std::iter::once(0).chain(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1)).collect();
    let line = |offset: usize| (lines.partition_point(|&start| start <= offset) - 1) as u32;
    let mut folds = Vec::new();
    for token in tokens.iter().filter(|t| t.tok == Tok::Comment) {
        let (start, end) = (line(token.start), line(token.end - 1));
        if end > start {
            folds.push(Fold { start_line: start, end_line: end });
        }
    }
    fn blocks(nodes: &[Node], tokens: &[Token], line: &dyn Fn(usize) -> u32, folds: &mut Vec<Fold>) {
        for block in nodes.iter().filter_map(|node| node.block.as_ref()) {
            if let Some(close) = block.close {
                let (start, end) = (line(tokens[block.open].start), line(tokens[close].start));
                if end > start {
                    folds.push(Fold { start_line: start, end_line: end });
                }
            }
            blocks(&block.children, tokens, line, folds);
        }
    }
    blocks(&nodes, &tokens, &line, &mut folds);
    folds.sort_unstable();
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

pub fn outline(text: &str) -> Vec<Symbol> {
    let tokens = tokenize(text);
    let nodes = parse(text, &tokens);
    let mut budget = MAX_SYMBOLS;
    symbols(text, &tokens, &nodes, 0, &mut budget)
}

const MAX_SYMBOLS: usize = 20_000;

const MAX_OUTLINE_DEPTH: usize = 32;

fn symbols(text: &str, tokens: &[Token], nodes: &[Node], depth: usize, budget: &mut usize) -> Vec<Symbol> {
    let mut out = Vec::new();
    if depth >= MAX_OUTLINE_DEPTH {
        return out;
    }
    for node in nodes {
        if *budget == 0 {
            break;
        }
        if depth == 0 && node.kind == NodeKind::Declaration && tokens[node.first].tok == Tok::Variable {
            let name = &tokens[node.first];
            *budget -= 1;
            out.push(Symbol {
                name: text[name.start..name.end].to_owned(),
                kind: "variable",
                start: name.start,
                end: tokens[node.end - 1].end,
                children: Vec::new(),
            });
            continue;
        }
        let Some(block) = &node.block else {
            continue;
        };
        let prelude = collapse(text, &tokens[node.prelude.clone()]);
        let (name, kind) = match node.kind {
            NodeKind::Rule if prelude.is_empty() => continue,
            NodeKind::Rule => (prelude, "rule"),
            NodeKind::AtRule => {
                let keyword = &tokens[node.first];
                let mut name = text[keyword.start..keyword.end].to_owned();
                if !prelude.is_empty() {
                    name.push(' ');
                    name.push_str(&prelude);
                }
                let at = at_name(text, keyword);
                let kind = if is_grouping(&at) {
                    "module"
                } else if scss() && matches!(at.as_str(), "mixin" | "function") {
                    "function"
                } else {
                    "rule"
                };
                (name, kind)
            }
            NodeKind::Declaration => continue,
        };
        *budget -= 1;
        out.push(Symbol {
            name: truncate(name),
            kind,
            start: tokens[node.first].start,
            end: tokens[node.end - 1].end,
            children: symbols(text, tokens, &block.children, depth + 1, budget),
        });
    }
    out
}

fn collapse(text: &str, tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token.tok {
            Tok::Comment => {}
            Tok::Space => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            _ => out.push_str(&text[token.start..token.end]),
        }
    }
    out.trim_end().to_owned()
}

fn truncate(mut name: String) -> String {
    if name.len() > MAX_NAME {
        let mut cut = MAX_NAME;
        while !name.is_char_boundary(cut) {
            cut -= 1;
        }
        name.truncate(cut);
        name.push('…');
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outline_is_bounded() {
        assert_eq!(outline(&"a{}\n".repeat(30_000)).len(), MAX_SYMBOLS);
        let mut depth = 0;
        let mut level = outline(&"@media x{".repeat(100));
        while let Some(first) = level.first() {
            depth += 1;
            level = first.children.clone();
        }
        assert_eq!(depth, MAX_OUTLINE_DEPTH);
    }

    fn folds(text: &str) -> Vec<(u32, u32)> {
        fold(text).into_iter().map(|f| (f.start_line, f.end_line)).collect()
    }

    fn tree(symbols: &[Symbol]) -> String {
        symbols
            .iter()
            .map(|s| {
                let children = if s.children.is_empty() { String::new() } else { format!("[{}]", tree(&s.children)) };
                format!("{}:{}{children}", s.kind, s.name)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn folds_blocks_and_comments() {
        let text = "/* a\n b */\na,\nb {\n  x: 1;\n}\n@media print {\n  c { y: 2 }\n  d {\n  }\n}\ne { z: 3 }\nf {\n";
        assert_eq!(folds(text), [(0, 1), (3, 5), (6, 10), (8, 9)]);
        assert_eq!(folds("/* one line */ a { }"), []);
        assert_eq!(folds("a {\r\n}\r\n"), [(0, 1)]);
    }

    #[test]
    fn outlines_rules_inside_groups() {
        let text = "@import 'x';\n/* c */ a ,\n  b:hover /* d */ { x: 1 }\n@media (min-width: 1px) {\n  .c { y: 2; &:focus { } }\n  @supports (display: grid) { #d { } }\n}\n@layer base { e {} }\n@font-face { font-family: f }\n@keyframes k { from { } to { } }\n{ }";
        let symbols = outline(text);
        assert_eq!(
            tree(&symbols),
            "rule:a , b:hover, module:@media (min-width: 1px)[rule:.c[rule:&:focus], module:@supports (display: grid)[rule:#d]], module:@layer base[rule:e], rule:@font-face, rule:@keyframes k[rule:from, rule:to]"
        );
        assert_eq!(&text[symbols[0].start..symbols[0].end], "a ,\n  b:hover /* d */ { x: 1 }");
        assert!(
            text[symbols[1].start..symbols[1].end].starts_with("@media") && text[..symbols[1].end].ends_with("}\n}")
        );
    }

    #[test]
    fn outlines_scss() {
        if !scss() {
            return;
        }
        let text = "$gap: 4px;\n@mixin pad($x) { padding: $x; }\n.a { $local: 1; .b { } }\n@function twice($n) { @return $n * 2; }";
        assert_eq!(
            tree(&outline(text)),
            "variable:$gap, function:@mixin pad($x), rule:.a[rule:.b], function:@function twice($n)"
        );
        assert_eq!(folds("// a\n// b\n.a {\n  b: 1;\n}\n"), [(2, 4)]);
    }

    #[test]
    fn shortens_long_names() {
        let text = format!("{} {{}}", ".é".repeat(300));
        let name = &outline(&text)[0].name;
        assert!(name.len() <= MAX_NAME + 3 && name.ends_with('…'));
    }
}
