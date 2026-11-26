use crate::dialect::Dialect;
use crate::lexer::{Token, TokenKind};
use crate::script::Script;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub error: bool,
    pub message: String,
}

pub const MAX_PROBLEMS: usize = 1_000;

const CLAUSES: &[&str] = &["FROM", "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "UNION", "EXCEPT", "INTERSECT"];

pub fn problems(text: &str, dialect: Dialect) -> Vec<Problem> {
    let mut found = if dialect == Dialect::Generic {
        let mysql: std::collections::HashSet<Problem> = check(text, Dialect::MySql).into_iter().collect();
        let mut both: Vec<Problem> =
            check(text, Dialect::Postgres).into_iter().filter(|problem| mysql.contains(problem)).collect();
        for problem in &mut both {
            if problem.message.starts_with("Unexpected ','") {
                problem.error = false;
            }
        }
        both
    } else {
        check(text, dialect)
    };
    found.sort_by_key(|problem| (problem.start, problem.end));
    found.truncate(MAX_PROBLEMS);
    found
}

fn check(text: &str, dialect: Dialect) -> Vec<Problem> {
    let script = Script::read(text, dialect);
    let mut found = Vec::new();
    let mut open: Vec<Token> = Vec::new();
    let error = |token: &Token, length: usize, message: String| Problem {
        start: token.start,
        end: (token.start + length.max(1)).min(token.end.max(token.start + 1)).min(text.len()),
        error: true,
        message,
    };
    let unclosed = |open: &mut Vec<Token>, found: &mut Vec<Problem>| {
        for token in open.drain(..) {
            found.push(error(&token, 1, format!("'{}' is not closed", token.text(text))));
        }
    };
    let significant: Vec<(usize, Token)> =
        script.tokens.iter().copied().enumerate().filter(|(_, token)| !token.is_comment()).collect();
    for (at, (_, token)) in significant.iter().enumerate() {
        match token.kind {
            TokenKind::String | TokenKind::Quoted if !token.closed => {
                let message = match (token.kind, text[token.start..].starts_with('$')) {
                    (TokenKind::String, true) => "Unclosed dollar-quoted string",
                    (TokenKind::String, false) => "Unclosed string literal",
                    _ => "Unclosed quoted name",
                };
                found.push(error(token, token.open, message.to_owned()));
            }
            TokenKind::Symbol if token.is_symbol(text, '(') || token.is_symbol(text, '[') => open.push(*token),
            TokenKind::Symbol if token.is_symbol(text, ')') || token.is_symbol(text, ']') => {
                let opener = if token.is_symbol(text, ')') { '(' } else { '[' };
                if open.last().is_some_and(|last| last.is_symbol(text, opener)) {
                    open.pop();
                } else {
                    found.push(error(token, 1, format!("Unmatched '{}'", token.text(text))));
                }
            }
            TokenKind::Symbol if token.is_symbol(text, ',') && dialect.refuses_trailing_commas() => {
                let next = significant.get(at + 1).map(|(_, next)| next);
                let before = match next {
                    None => Some("the end of the statement".to_owned()),
                    Some(next) if matches!(next.kind, TokenKind::Delimiter | TokenKind::Directive) => {
                        Some("the end of the statement".to_owned())
                    }
                    Some(next) if next.is_symbol(text, ')') => Some("')'".to_owned()),
                    Some(next) if CLAUSES.iter().any(|word| next.is(text, word)) => {
                        Some(next.text(text).to_ascii_uppercase())
                    }
                    Some(_) => None,
                };
                if let Some(before) = before {
                    found.push(error(token, 1, format!("Unexpected ',' before {before}")));
                }
            }
            TokenKind::Delimiter | TokenKind::Directive => unclosed(&mut open, &mut found),
            _ => {}
        }
    }
    unclosed(&mut open, &mut found);
    for token in script.tokens.iter().filter(|token| token.kind == TokenKind::BlockComment && !token.closed) {
        found.push(error(token, token.open, "Unclosed comment".to_owned()));
    }
    for index in &script.unclosed {
        let token = script.tokens[*index];
        if token.is(text, "CASE") {
            found.push(error(&token, token.end - token.start, "CASE without END".to_owned()));
        }
    }
    found
}
