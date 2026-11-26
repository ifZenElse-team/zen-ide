use std::ops::Range;

use crate::dialect::Dialect;
use crate::lexer::{Token, TokenKind, tokens};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    pub tokens: Range<usize>,
    pub delimiter: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub open: usize,
    pub close: usize,
}

#[derive(Debug)]
pub struct Script<'a> {
    pub text: &'a str,
    pub dialect: Dialect,
    pub tokens: Vec<Token>,
    pub statements: Vec<Statement>,
    pub blocks: Vec<Block>,
    pub unclosed: Vec<usize>,
    line_starts: Vec<usize>,
}

const TRANSACTION_WORDS: &[&str] =
    &["TRAN", "TRANSACTION", "WORK", "DEFERRED", "IMMEDIATE", "EXCLUSIVE", "ISOLATION", "READ", "DISTRIBUTED", "NOT"];

const LOOP_WORDS: &[&str] = &["IF", "LOOP", "WHILE", "REPEAT", "FOR"];

impl<'a> Script<'a> {
    pub fn read(text: &'a str, dialect: Dialect) -> Self {
        let tokens = tokens(text, dialect);
        let mut statements = Vec::new();
        let mut blocks = Vec::new();
        let mut unclosed = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        let mut start: Option<usize> = None;
        let mut index = 0;
        let next_word = |index: usize| {
            tokens[index + 1..].iter().find(|token| !token.is_comment()).filter(|token| token.kind == TokenKind::Word)
        };
        while index < tokens.len() {
            let token = tokens[index];
            let ends = match token.kind {
                TokenKind::Directive => true,
                TokenKind::Delimiter => open.is_empty() || token.text(text) != ";",
                _ => false,
            };
            if ends {
                if let Some(first) = start.take() {
                    let delimiter = (token.kind == TokenKind::Delimiter).then_some(index);
                    statements.push(Statement { tokens: first..index, delimiter });
                }
                unclosed.append(&mut open);
                index += 1;
                continue;
            }
            if token.is_comment() {
                index += 1;
                continue;
            }
            start.get_or_insert(index);
            if token.is(text, "CASE") {
                open.push(index);
            } else if token.is(text, "BEGIN") {
                let transaction = match next_word(index) {
                    Some(next) => TRANSACTION_WORDS.iter().any(|word| next.is(text, word)),
                    None => true,
                };
                if !transaction {
                    open.push(index);
                }
            } else if token.is(text, "END") {
                let after = next_word(index);
                let loop_end = after.is_some_and(|next| LOOP_WORDS.iter().any(|word| next.is(text, word)));
                if !loop_end && let Some(opened) = open.pop() {
                    blocks.push(Block { open: opened, close: index });
                }
                if loop_end || after.is_some_and(|next| next.is(text, "CASE")) {
                    let skip = tokens[index + 1..].iter().position(|token| !token.is_comment()).unwrap_or(0);
                    index += skip + 1;
                }
            }
            index += 1;
        }
        if let Some(first) = start {
            statements.push(Statement { tokens: first..tokens.len(), delimiter: None });
        }
        unclosed.append(&mut open);
        unclosed.sort_unstable();
        blocks.sort_by_key(|block| block.open);
        let line_starts = std::iter::once(0)
            .chain(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(at, _)| at + 1))
            .collect();
        Self { text, dialect, tokens, statements, blocks, unclosed, line_starts }
    }

    pub fn line(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= offset).saturating_sub(1)
    }

    pub fn significant(&self, statement: &Statement) -> impl Iterator<Item = (usize, Token)> + '_ {
        let range = statement.tokens.clone();
        range.map(|index| (index, self.tokens[index])).filter(|(_, token)| !token.is_comment())
    }

    pub fn span(&self, statement: &Statement) -> Range<usize> {
        let start = self.tokens[statement.tokens.start].start;
        let last = statement.delimiter.unwrap_or(statement.tokens.end.saturating_sub(1));
        start..self.tokens[last.max(statement.tokens.start)].end
    }
}
