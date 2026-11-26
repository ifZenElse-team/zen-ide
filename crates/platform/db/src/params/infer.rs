use std::collections::BTreeMap;

use super::{ParamType, code_mask, is_word, is_word_start, placeholders, word_end};
use crate::sql::Dialect;

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Number,
    Text,
    Placeholder(String),
    Operator(String),
    Punct(u8),
}

fn tokens(text: &str, dialect: Dialect) -> Vec<Token> {
    let mask = code_mask(text, dialect);
    let bytes = text.as_bytes();
    let found = placeholders(text, dialect);
    let mut next_placeholder = found.iter().peekable();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if next_placeholder.peek().is_some_and(|placeholder| placeholder.start == at) {
            if let Some(placeholder) = next_placeholder.next() {
                out.push(Token::Placeholder(placeholder.name.clone()));
                at = placeholder.end;
            }
            continue;
        }
        let byte = bytes[at];
        if !mask[at] {
            let end = mask[at..].iter().position(|code| *code).map_or(bytes.len(), |found| at + found);
            let closing = match byte {
                b'`' => Some('`'),
                b'[' => Some(']'),
                b'"' if dialect != Dialect::MySql => Some('"'),
                _ => None,
            };
            match (closing, text.get(at + 1..end)) {
                (Some(closing), Some(inner)) => {
                    out.push(Token::Word(inner.strip_suffix(closing).unwrap_or(inner).to_ascii_lowercase()));
                }
                _ if matches!(byte, b'\'' | b'"' | b'$') => out.push(Token::Text),
                _ => {}
            }
            at = end;
            continue;
        }
        if byte.is_ascii_whitespace() {
            at += 1;
        } else if byte.is_ascii_digit() || (byte == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)) {
            let mut end = at + 1;
            while end < bytes.len() {
                let next = bytes[end];
                let exponent_sign = matches!(next, b'+' | b'-') && matches!(bytes[end - 1], b'e' | b'E');
                if next.is_ascii_alphanumeric() || next == b'.' || exponent_sign {
                    end += 1;
                } else {
                    break;
                }
            }
            out.push(Token::Number);
            at = end;
        } else if is_word_start(byte) {
            let end = word_end(bytes, at);
            out.push(Token::Word(text[at..end].to_ascii_lowercase()));
            at = end;
        } else if b"<>=!+-*/%|&^~".contains(&byte) {
            let mut end = at + 1;
            while end < bytes.len() && b"<>=!|&".contains(&bytes[end]) && mask[end] {
                end += 1;
            }
            out.push(Token::Operator(text[at..end].to_owned()));
            at = end;
        } else {
            out.push(Token::Punct(byte));
            at += 1;
        }
    }
    out
}

const NUMBER_TYPES: [&str; 19] = [
    "integer",
    "smallint",
    "bigint",
    "tinyint",
    "mediumint",
    "serial",
    "smallserial",
    "bigserial",
    "numeric",
    "decimal",
    "dec",
    "real",
    "float",
    "double",
    "money",
    "number",
    "oid",
    "year",
    "smallmoney",
];

const NUMBER_PREFIXES: [&str; 6] = ["int", "uint", "float", "decimal", "serial", "numeric"];

const COMPARISONS: [&str; 8] = ["=", "<>", "!=", "<", ">", "<=", ">=", "=="];

const ARITHMETIC: [&str; 5] = ["+", "-", "*", "/", "%"];

const COUNTS: [&str; 4] = ["limit", "offset", "top", "first"];

pub fn column_type(type_name: &str) -> Option<ParamType> {
    let lower = type_name.trim().to_ascii_lowercase();
    for wrapper in ["nullable(", "lowcardinality("] {
        if let Some(inner) = lower.strip_prefix(wrapper) {
            return column_type(inner.trim_end_matches(')'));
        }
    }
    if lower.ends_with("[]") || lower.starts_with('_') || lower.starts_with("array") {
        return None;
    }
    let base = lower.split(['(', ' ', ')']).next().unwrap_or_default();
    let numbered = NUMBER_PREFIXES
        .iter()
        .any(|prefix| base.strip_prefix(prefix).is_some_and(|rest| rest.bytes().all(|byte| byte.is_ascii_digit())));
    if numbered || NUMBER_TYPES.contains(&base) {
        Some(ParamType::Number)
    } else if matches!(base, "bool" | "boolean") {
        Some(ParamType::Boolean)
    } else {
        None
    }
}

fn is_keyword(token: Option<&Token>, keyword: &str) -> bool {
    matches!(token, Some(Token::Word(word)) if word == keyword)
}

fn operand_type(tokens: &[Token], at: usize, columns: &dyn Fn(&str) -> Option<ParamType>) -> Option<ParamType> {
    match tokens.get(at)? {
        Token::Number => Some(ParamType::Number),
        Token::Word(word) if word == "true" || word == "false" => Some(ParamType::Boolean),
        Token::Word(word) => match (tokens.get(at + 1), tokens.get(at + 2)) {
            (Some(Token::Punct(b'.')), Some(Token::Word(column))) => columns(column),
            _ => columns(word),
        },
        _ => None,
    }
}

fn cast_type(tokens: &[Token], at: usize) -> Option<ParamType> {
    match (tokens.get(at + 1), tokens.get(at + 2), tokens.get(at + 3)) {
        (Some(Token::Punct(b':')), Some(Token::Punct(b':')), Some(Token::Word(name))) => column_type(name),
        (Some(Token::Word(word)), Some(Token::Word(name)), _) if word == "as" => {
            let cast =
                at >= 2 && tokens.get(at - 1) == Some(&Token::Punct(b'(')) && is_keyword(tokens.get(at - 2), "cast");
            if cast { column_type(name) } else { None }
        }
        _ => None,
    }
}

fn listed_type(tokens: &[Token], at: usize, columns: &dyn Fn(&str) -> Option<ParamType>) -> Option<ParamType> {
    let mut open = at;
    while open > 0 {
        open -= 1;
        match tokens.get(open)? {
            Token::Punct(b'(') => break,
            Token::Placeholder(_) | Token::Number | Token::Text | Token::Punct(b',') => {}
            _ => return None,
        }
    }
    if open < 2 || tokens.get(open) != Some(&Token::Punct(b'(')) || !is_keyword(tokens.get(open - 1), "in") {
        return None;
    }
    operand_type(tokens, open - 2, columns)
}

fn type_at(tokens: &[Token], at: usize, columns: &dyn Fn(&str) -> Option<ParamType>) -> Option<ParamType> {
    if let Some(cast) = cast_type(tokens, at) {
        return Some(cast);
    }
    let before = at.checked_sub(1).and_then(|before| tokens.get(before));
    let after = tokens.get(at + 1);
    if COUNTS.iter().any(|count| is_keyword(before, count)) {
        return Some(ParamType::Number);
    }
    for (operator, other) in [(before, at.checked_sub(2)), (after, Some(at + 2))] {
        let Some(Token::Operator(operator)) = operator else { continue };
        if ARITHMETIC.contains(&operator.as_str()) {
            return Some(ParamType::Number);
        }
        if COMPARISONS.contains(&operator.as_str())
            && let Some(found) = other.and_then(|other| operand_type(tokens, other, columns))
        {
            return Some(found);
        }
    }
    if is_keyword(before, "between") {
        return at.checked_sub(2).and_then(|left| operand_type(tokens, left, columns));
    }
    if is_keyword(before, "and") && at >= 4 && is_keyword(tokens.get(at - 3), "between") {
        return operand_type(tokens, at - 4, columns);
    }
    listed_type(tokens, at, columns)
}

pub fn infer_types(
    text: &str,
    dialect: Dialect,
    columns: &dyn Fn(&str) -> Option<ParamType>,
) -> BTreeMap<String, ParamType> {
    let mut found: BTreeMap<String, ParamType> = BTreeMap::new();
    if dialect == Dialect::Lines {
        return found;
    }
    let tokens = tokens(text, dialect);
    for (at, token) in tokens.iter().enumerate() {
        let Token::Placeholder(name) = token else { continue };
        if found.contains_key(name) {
            continue;
        }
        if let Some(kind) = type_at(&tokens, at, columns) {
            found.insert(name.clone(), kind);
        }
    }
    found
}

pub fn mentions(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let (text, word) = (text.to_ascii_lowercase(), word.to_ascii_lowercase());
    let bytes = text.as_bytes();
    text.match_indices(&word).any(|(start, _)| {
        let end = start + word.len();
        (start == 0 || !is_word(bytes[start - 1])) && (end >= bytes.len() || !is_word(bytes[end]))
    })
}
