use crate::token::{Tok, Token};
use std::ops::Range;

const MAX_DEPTH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
    Rule,
    AtRule,
    Declaration,
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) first: usize,
    pub(crate) prelude: Range<usize>,
    pub(crate) colon: Option<usize>,
    pub(crate) semicolon: Option<usize>,
    pub(crate) block: Option<Block>,
    pub(crate) end: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Block {
    pub(crate) open: usize,
    pub(crate) close: Option<usize>,
    pub(crate) mixed: bool,
    pub(crate) children: Vec<Node>,
}

pub(crate) fn is_grouping(name: &str) -> bool {
    matches!(
        name,
        "media" | "supports" | "layer" | "container" | "scope" | "starting-style" | "document" | "-moz-document"
    )
}

pub(crate) fn at_name(text: &str, token: &Token) -> String {
    text[token.start + 1..token.end].to_ascii_lowercase()
}

pub(crate) fn parse(text: &str, tokens: &[Token]) -> Vec<Node> {
    let mut parser = Parser { text, tokens, at: 0 };
    parser.list(false, 0)
}

struct Parser<'a> {
    text: &'a str,
    tokens: &'a [Token],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Tok> {
        self.tokens.get(self.at).map(|t| t.tok)
    }

    fn list(&mut self, mixed: bool, depth: usize) -> Vec<Node> {
        let mut nodes = Vec::new();
        loop {
            match self.peek() {
                None => break,
                Some(Tok::Space | Tok::Comment | Tok::Cdo | Tok::Semicolon) => self.at += 1,
                Some(Tok::CloseBrace) if depth > 0 => break,
                Some(Tok::CloseBrace) => self.at += 1,
                Some(Tok::AtKeyword) => {
                    let node = self.at_rule(mixed, depth);
                    nodes.push(node);
                }
                Some(Tok::Variable) if self.variable_declaration() => {
                    let node = self.declaration();
                    nodes.push(node);
                }
                Some(_) if mixed && !self.block_first() => {
                    let node = self.declaration();
                    nodes.push(node);
                }
                Some(_) => {
                    let node = self.rule(depth);
                    nodes.push(node);
                }
            }
        }
        nodes
    }

    fn variable_declaration(&self) -> bool {
        self.tokens[self.at + 1..]
            .iter()
            .find(|t| !matches!(t.tok, Tok::Space | Tok::Comment))
            .is_some_and(|t| t.tok == Tok::Colon)
    }

    fn block_first(&self) -> bool {
        self.tokens[self.at..]
            .iter()
            .find(|t| matches!(t.tok, Tok::OpenBrace | Tok::Semicolon | Tok::CloseBrace))
            .is_some_and(|t| t.tok == Tok::OpenBrace)
    }

    fn prelude(&mut self) -> Range<usize> {
        let start = self.at;
        while self.peek().is_some_and(|tok| !matches!(tok, Tok::OpenBrace | Tok::Semicolon | Tok::CloseBrace)) {
            self.at += 1;
        }
        start..self.at
    }

    fn finish(&mut self, kind: NodeKind, first: usize, prelude: Range<usize>, mixed: bool, depth: usize) -> Node {
        let mut node = Node { kind, first, prelude, colon: None, semicolon: None, block: None, end: 0 };
        match self.peek() {
            Some(Tok::OpenBrace) => node.block = Some(self.block(mixed, depth + 1)),
            Some(Tok::Semicolon) => {
                node.semicolon = Some(self.at);
                self.at += 1;
            }
            _ => {}
        }
        node.end = self.at;
        node
    }

    fn rule(&mut self, depth: usize) -> Node {
        let first = self.at;
        let prelude = self.prelude();
        self.finish(NodeKind::Rule, first, prelude, true, depth)
    }

    fn at_rule(&mut self, mixed: bool, depth: usize) -> Node {
        let first = self.at;
        let name = at_name(self.text, &self.tokens[first]);
        self.at += 1;
        let prelude = self.prelude();
        let rules_only = name.ends_with("keyframes") || (!mixed && is_grouping(&name));
        self.finish(NodeKind::AtRule, first, prelude, !rules_only, depth)
    }

    fn declaration(&mut self) -> Node {
        let first = self.at;
        self.at += 1;
        let colon = self.tokens[self.at..]
            .iter()
            .position(|t| !matches!(t.tok, Tok::Space | Tok::Comment))
            .map(|offset| self.at + offset)
            .filter(|&i| self.tokens[i].tok == Tok::Colon);
        let value_start = colon.map_or(self.at, |c| c + 1);
        self.at = value_start;
        let value = self.prelude();
        let mut node =
            Node { kind: NodeKind::Declaration, first, prelude: value, colon, semicolon: None, block: None, end: 0 };
        if self.peek() == Some(Tok::Semicolon) {
            node.semicolon = Some(self.at);
            self.at += 1;
        }
        node.end = self.at;
        node
    }

    fn block(&mut self, mixed: bool, depth: usize) -> Block {
        let open = self.at;
        self.at += 1;
        let children = if depth > MAX_DEPTH {
            self.skip_nested();
            Vec::new()
        } else {
            self.list(mixed, depth)
        };
        let close = (self.peek() == Some(Tok::CloseBrace)).then_some(self.at);
        if close.is_some() {
            self.at += 1;
        }
        Block { open, close, mixed, children }
    }

    fn skip_nested(&mut self) {
        let mut open = 0usize;
        while let Some(tok) = self.peek() {
            match tok {
                Tok::OpenBrace => open += 1,
                Tok::CloseBrace if open == 0 => break,
                Tok::CloseBrace => open -= 1,
                _ => {}
            }
            self.at += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::tokenize;

    fn shape(text: &str) -> String {
        fn walk(text: &str, tokens: &[Token], nodes: &[Node], out: &mut String) {
            for node in nodes {
                let first = &tokens[node.first];
                let label = match node.kind {
                    NodeKind::Rule => "R",
                    NodeKind::AtRule => "A",
                    NodeKind::Declaration => "D",
                };
                out.push_str(label);
                out.push('(');
                out.push_str(&text[first.start..first.end]);
                if let Some(block) = &node.block {
                    out.push_str(if block.mixed { "{" } else { "[" });
                    walk(text, tokens, &block.children, out);
                    out.push_str(match (block.close.is_some(), block.mixed) {
                        (false, _) => "…",
                        (true, true) => "}",
                        (true, false) => "]",
                    });
                }
                out.push(')');
            }
        }
        let tokens = tokenize(text);
        let mut out = String::new();
        walk(text, &tokens, &parse(text, &tokens), &mut out);
        out
    }

    #[test]
    fn tells_rules_from_declarations() {
        assert_eq!(shape("a { color: red; b { x: 1 } &:hover { y: 2 } }"), "R(a{D(color)R(b{D(x)})R(&{D(y)})})");
        assert_eq!(shape("@media screen { a { x: 1 } }"), "A(@media[R(a{D(x)})])");
        assert_eq!(shape("a { @media print { x: 1 } }"), "R(a{A(@media{D(x)})})");
        assert_eq!(shape("@keyframes k { from { x: 0 } 50% { x: 1 } }"), "A(@keyframes[R(from{D(x)})R(50%{D(x)})])");
        assert_eq!(shape("@font-face { font-family: x; src: url(a) }"), "A(@font-face{D(font-family)D(src)})");
        assert_eq!(shape("@import 'a'; a:hover"), "A(@import)R(a)");
        assert_eq!(shape("} a { b"), "R(a{D(b)…)");
    }

    #[test]
    fn reads_scss_statements() {
        if crate::token::scss() {
            assert_eq!(
                shape("$a: 1;\n@mixin m($x: 2) { b: $x; }\n.c { $d: 3; @include m; &-e { f: #{$a} } }"),
                "D($a)A(@mixin{D(b)})R(.{D($d)A(@include)R(&{D(f)})})"
            );
        }
    }

    #[test]
    fn survives_deep_nesting() {
        let text = "{".repeat(10_000) + &"}".repeat(10_000);
        let tokens = tokenize(&text);
        assert_eq!(parse(&text, &tokens).len(), 1);
    }
}
