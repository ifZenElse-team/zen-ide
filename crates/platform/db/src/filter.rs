use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::{Lexing, SqlDialect};
use crate::value::Value;

/// Why a condition cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FilterError {
    /// It has a `;`: one condition, never another statement.
    Semicolon,
    /// Its parentheses do not match.
    Parentheses,
    /// A quote or a comment is not closed.
    Unterminated,
    /// The statement cannot take a condition: it is not a query that reads rows.
    NotAQuery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Word,
    Quoted,
    Literal,
    Symbol,
    Comment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: Kind,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) depth: usize,
}

pub(crate) struct Scan {
    pub(crate) tokens: Vec<Token>,
    pub(crate) unterminated: bool,
    unbalanced: bool,
    semicolon: bool,
}

fn dollar_tag(bytes: &[u8], at: usize) -> Option<usize> {
    let mut end = at + 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    (end < bytes.len() && bytes[end] == b'$' && !bytes.get(at + 1).is_some_and(u8::is_ascii_digit)).then_some(end + 1)
}

fn quoted(bytes: &[u8], at: &mut usize, close: u8, doubled: bool, backslashes: bool) -> bool {
    *at += 1;
    while *at < bytes.len() {
        if backslashes && bytes[*at] == b'\\' {
            *at += 2;
            continue;
        }
        if bytes[*at] == close {
            if doubled && bytes.get(*at + 1) == Some(&close) {
                *at += 2;
                continue;
            }
            *at += 1;
            return true;
        }
        *at += 1;
    }
    *at = bytes.len();
    false
}

pub(crate) fn scan(text: &str, lexing: Lexing) -> Scan {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut depth: usize = 0;
    let mut at = 0;
    let (mut unterminated, mut unbalanced, mut semicolon) = (false, false, false);
    while at < bytes.len() {
        let byte = bytes[at];
        let start = at;
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let kind = if (byte == b'-' && bytes.get(at + 1) == Some(&b'-')) || (byte == b'#' && lexing.hash_comments) {
            at = text[at..].find('\n').map_or(bytes.len(), |end| at + end);
            Kind::Comment
        } else if byte == b'/' && bytes.get(at + 1) == Some(&b'*') {
            let mut nested = 1;
            at += 2;
            while at < bytes.len() && nested > 0 {
                if lexing.dollar_quotes && bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'*') {
                    nested += 1;
                    at += 2;
                } else if bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/') {
                    nested -= 1;
                    at += 2;
                } else {
                    at += 1;
                }
            }
            unterminated |= nested > 0;
            Kind::Comment
        } else if byte == b'\'' {
            unterminated |= !quoted(bytes, &mut at, b'\'', true, lexing.backslashes);
            Kind::Literal
        } else if lexing.dollar_quotes && (byte == b'E' || byte == b'e') && bytes.get(at + 1) == Some(&b'\'') {
            at += 1;
            unterminated |= !quoted(bytes, &mut at, b'\'', true, true);
            Kind::Literal
        } else if byte == b'"' && lexing.double_quoted_strings {
            unterminated |= !quoted(bytes, &mut at, b'"', true, lexing.backslashes);
            Kind::Literal
        } else if byte == b'"' || (byte == b'`' && lexing.backticks) {
            unterminated |= !quoted(bytes, &mut at, byte, true, false);
            Kind::Quoted
        } else if byte == b'[' && lexing.brackets {
            unterminated |= !quoted(bytes, &mut at, b']', false, false);
            Kind::Quoted
        } else if byte == b'$'
            && lexing.dollar_quotes
            && let Some(tag_end) = dollar_tag(bytes, at)
        {
            let tag = &text[at..tag_end];
            match text[tag_end..].find(tag) {
                Some(found) => at = tag_end + found + tag.len(),
                None => {
                    at = bytes.len();
                    unterminated = true;
                }
            }
            Kind::Literal
        } else if byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80 {
            while at < bytes.len()
                && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_' || bytes[at] == b'$' || bytes[at] >= 0x80)
            {
                at += 1;
            }
            Kind::Word
        } else {
            at += 1;
            match byte {
                b'(' => {
                    tokens.push(Token { kind: Kind::Symbol, start, end: at, depth });
                    depth += 1;
                    continue;
                }
                b')' => {
                    if depth == 0 {
                        unbalanced = true;
                    } else {
                        depth -= 1;
                    }
                }
                b';' => semicolon = true,
                _ => {}
            }
            Kind::Symbol
        };
        tokens.push(Token { kind, start, end: at, depth });
    }
    Scan { tokens, unterminated, unbalanced: unbalanced || depth != 0, semicolon }
}

fn word<'a>(text: &'a str, token: &Token) -> Option<&'a str> {
    (token.kind == Kind::Word).then(|| &text[token.start..token.end])
}

fn is_word(text: &str, token: &Token, wanted: &str) -> bool {
    word(text, token).is_some_and(|found| found.eq_ignore_ascii_case(wanted))
}

pub fn check(condition: &str, dialect: SqlDialect) -> Result<(), FilterError> {
    let scanned = scan(condition, dialect.lexing());
    if scanned.unterminated {
        Err(FilterError::Unterminated)
    } else if scanned.semicolon {
        Err(FilterError::Semicolon)
    } else if scanned.unbalanced {
        Err(FilterError::Parentheses)
    } else {
        Ok(())
    }
}

fn wrapped(condition: &str, lexing: Lexing) -> String {
    let comment = scan(condition, lexing).tokens.iter().any(|token| token.kind == Kind::Comment);
    if comment { format!("({}\n)", condition.trim()) } else { format!("({})", condition.trim()) }
}

/// A sort of a data view: a column and its direction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    /// The result column, from 0.
    pub column: usize,
    /// Its name in the result.
    pub name: String,
    /// Largest first.
    #[serde(default)]
    pub descending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source<'a> {
    Table { name: &'a str, order: Option<&'a str> },
    Query(&'a str),
}

/// How a condition was put into a statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Composition {
    /// Into a table's generated `SELECT`.
    Table,
    /// Merged into a plain `SELECT`'s `WHERE` and `ORDER BY`.
    Merged,
    /// Around the statement as a subselect.
    Wrapped,
    /// The statement as it was: no condition and no sort.
    Unchanged,
}

fn order_list(sorts: &[Sort], names: &[String], dialect: SqlDialect) -> String {
    sorts
        .iter()
        .map(|sort| {
            let unique = names.iter().filter(|name| **name == sort.name).count() == 1;
            let column = if unique { dialect.quote(&sort.name) } else { (sort.column + 1).to_string() };
            dialect.order(&column, sort.descending)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn code_end(text: &str, lexing: Lexing) -> usize {
    let scanned = scan(text, lexing);
    scanned
        .tokens
        .iter()
        .rev()
        .find(|token| {
            token.kind != Kind::Comment && !(token.kind == Kind::Symbol && &text[token.start..token.end] == ";")
        })
        .map_or(0, |token| token.end)
}

pub fn filterable(statement: &str) -> bool {
    crate::sql::pageable(statement)
}

const NOT_MERGED: &[&str] =
    &["union", "intersect", "except", "group", "having", "window", "limit", "offset", "fetch", "for", "into"];

fn merged(text: &str, condition: Option<&str>, order: Option<&str>, lexing: Lexing) -> Option<String> {
    let end = code_end(text, lexing);
    let body = &text[..end];
    let scanned = scan(body, lexing);
    let top: Vec<&Token> = scanned.tokens.iter().filter(|token| token.depth == 0 && token.kind == Kind::Word).collect();
    if !top.first().is_some_and(|token| is_word(body, token, "select")) {
        return None;
    }
    if top.iter().any(|token| NOT_MERGED.iter().any(|wanted| is_word(body, token, wanted))) {
        return None;
    }
    if top.iter().filter(|token| is_word(body, token, "from")).count() != 1 {
        return None;
    }
    let where_at = top.iter().find(|token| is_word(body, token, "where"));
    let order_at = top
        .windows(2)
        .find(|pair| is_word(body, pair[0], "order") && is_word(body, pair[1], "by"))
        .map(|pair| pair[0].start);
    let tail_start = order_at.unwrap_or(body.len());
    let mut out = String::with_capacity(body.len() + 64);
    match (condition, where_at) {
        (Some(condition), Some(token)) => {
            out.push_str(&body[..token.end]);
            out.push_str(" (");
            out.push_str(body[token.end..tail_start].trim());
            out.push_str(") AND ");
            out.push_str(&wrapped(condition, lexing));
        }
        (Some(condition), None) => {
            out.push_str(body[..tail_start].trim_end());
            out.push_str(" WHERE ");
            out.push_str(&wrapped(condition, lexing));
        }
        (None, _) => out.push_str(body[..tail_start].trim_end()),
    }
    match (order, order_at) {
        (Some(order), _) => {
            out.push_str(" ORDER BY ");
            out.push_str(order);
        }
        (None, Some(at)) => {
            out.push(' ');
            out.push_str(body[at..].trim());
        }
        (None, None) => {}
    }
    Some(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRead {
    pub schema: Option<String>,
    pub table: String,
    pub columns: Option<Vec<String>>,
}

const READ_TAIL: &[&str] = &["where", "order", "limit", "offset"];

fn name_of(text: &str, token: &Token) -> Option<String> {
    match token.kind {
        Kind::Word => Some(text[token.start..token.end].to_owned()),
        Kind::Quoted => Some(unquoted(&text[token.start..token.end])),
        _ => None,
    }
}

fn symbol(text: &str, token: &Token, wanted: &str) -> bool {
    token.kind == Kind::Symbol && &text[token.start..token.end] == wanted
}

pub fn table_read(statement: &str, dialect: SqlDialect) -> Option<TableRead> {
    let lexing = dialect.lexing();
    let body = &statement[..code_end(statement, lexing)];
    let scanned = scan(body, lexing);
    if scanned.unterminated || scanned.unbalanced || scanned.semicolon {
        return None;
    }
    let tokens: Vec<&Token> = scanned.tokens.iter().filter(|token| token.kind != Kind::Comment).collect();
    let mut at = 0;
    if !tokens.first().is_some_and(|token| is_word(body, token, "select")) {
        return None;
    }
    at += 1;
    if tokens.get(at).is_some_and(|token| is_word(body, token, "all")) {
        at += 1;
    }
    let from = tokens.iter().position(|token| token.depth == 0 && is_word(body, token, "from"))?;
    let items = &tokens[at..from];
    let columns = if items.len() == 1 && symbol(body, items[0], "*") {
        None
    } else {
        let mut names = Vec::new();
        for item in items.split(|token| symbol(body, token, ",")) {
            let name = match item {
                [name] => name_of(body, name)?,
                [_, dot, name] if symbol(body, dot, ".") => name_of(body, name)?,
                _ => return None,
            };
            names.push(name);
        }
        Some(names)
    };
    let rest = &tokens[from + 1..];
    let tail = rest
        .iter()
        .position(|token| token.depth == 0 && READ_TAIL.iter().any(|word| is_word(body, token, word)))
        .unwrap_or(rest.len());
    if rest[tail..].iter().any(|token| {
        token.depth == 0
            && NOT_MERGED.iter().any(|word| *word != "limit" && *word != "offset" && is_word(body, token, word))
    }) {
        return None;
    }
    let source = &rest[..tail];
    let (schema, table, alias) = match source {
        [schema, dot, table, alias @ ..] if symbol(body, dot, ".") => {
            (Some(name_of(body, schema)?), name_of(body, table)?, alias)
        }
        [table, alias @ ..] => (None, name_of(body, table)?, alias),
        _ => return None,
    };
    let plain_alias = match alias {
        [] => true,
        [name] => name.kind != Kind::Symbol,
        [word, name] => is_word(body, word, "as") && name.kind != Kind::Symbol,
        _ => false,
    };
    if !plain_alias || alias.iter().any(|token| is_word(body, token, "join")) {
        return None;
    }
    Some(TableRead { schema, table, columns })
}

pub fn compose(
    source: Source<'_>,
    condition: Option<&str>,
    sorts: &[Sort],
    names: &[String],
    dialect: SqlDialect,
) -> Result<(String, Composition), FilterError> {
    let lexing = dialect.lexing();
    let condition = condition.map(str::trim).filter(|condition| !condition.is_empty());
    if let Some(condition) = condition {
        check(condition, dialect)?;
    }
    match source {
        Source::Table { name, order } => {
            let mut sql = format!("SELECT * FROM {name}");
            if let Some(condition) = condition {
                sql.push_str(" WHERE ");
                sql.push_str(&wrapped(condition, lexing));
            }
            let sorted = (!sorts.is_empty()).then(|| order_list(sorts, names, dialect));
            if let Some(order) = sorted.as_deref().or(order) {
                sql.push_str(" ORDER BY ");
                sql.push_str(order);
            }
            Ok((sql, Composition::Table))
        }
        Source::Query(text) => {
            let base = &text[..code_end(text, lexing)];
            if condition.is_none() && sorts.is_empty() {
                return Ok((base.trim().to_owned(), Composition::Unchanged));
            }
            if !filterable(base) {
                return Err(FilterError::NotAQuery);
            }
            let order = (!sorts.is_empty()).then(|| order_list(sorts, names, dialect));
            if let Some(sql) = merged(base, condition, order.as_deref(), lexing) {
                return Ok((sql, Composition::Merged));
            }
            let mut sql = format!("SELECT * FROM (\n{}\n){}", base.trim(), dialect.alias("zen_filtered"));
            if let Some(condition) = condition {
                sql.push_str(" WHERE ");
                sql.push_str(&wrapped(condition, lexing));
            }
            if let Some(order) = order {
                sql.push_str(" ORDER BY ");
                sql.push_str(&order);
            }
            Ok((sql, Composition::Wrapped))
        }
    }
}

/// A quick filter from a cell's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Operator {
    /// `= value`.
    Equal,
    /// `<> value`.
    NotEqual,
    /// `> value`.
    Greater,
    /// `< value`.
    Less,
    /// `LIKE '%value%'`.
    Contains,
    /// `IS NULL`.
    IsNull,
    /// `IS NOT NULL`.
    IsNotNull,
    /// `IN (values)`.
    In,
}

pub fn quick(name: &str, operator: Operator, values: &[Value], dialect: SqlDialect) -> String {
    let column = dialect.quote(name);
    let first = values.first().unwrap_or(&Value::Null);
    match operator {
        Operator::IsNull => format!("{column} IS NULL"),
        Operator::IsNotNull => format!("{column} IS NOT NULL"),
        _ if *first == Value::Null && operator != Operator::In => {
            if operator == Operator::NotEqual {
                format!("{column} IS NOT NULL")
            } else {
                format!("{column} IS NULL")
            }
        }
        Operator::Equal => format!("{column} = {}", dialect.literal(first)),
        Operator::NotEqual => format!("{column} <> {}", dialect.literal(first)),
        Operator::Greater => format!("{column} > {}", dialect.literal(first)),
        Operator::Less => format!("{column} < {}", dialect.literal(first)),
        Operator::Contains => dialect.contains(&column, &crate::edit::param_text(first).unwrap_or_default()),
        Operator::In => {
            let mut seen: Vec<String> = Vec::new();
            for value in values.iter().filter(|value| **value != Value::Null) {
                let literal = dialect.literal(value);
                if !seen.contains(&literal) {
                    seen.push(literal);
                }
            }
            let null = values.contains(&Value::Null);
            match (seen.is_empty(), null) {
                (true, _) => format!("{column} IS NULL"),
                (false, false) => format!("{column} IN ({})", seen.join(", ")),
                (false, true) => format!("({column} IN ({}) OR {column} IS NULL)", seen.join(", ")),
            }
        }
    }
}

pub fn typed_value(text: &str, numeric: bool) -> Value {
    let trimmed = text.trim();
    if numeric && trimmed.parse::<f64>().is_ok_and(f64::is_finite) {
        Value::Decimal(trimmed.to_owned())
    } else {
        Value::Text(text.to_owned())
    }
}

fn top_level_or(condition: &str, lexing: Lexing) -> bool {
    let scanned = scan(condition, lexing);
    scanned.tokens.iter().any(|token| token.depth == 0 && is_word(condition, token, "or"))
}

pub fn and(current: &str, added: &str, dialect: SqlDialect) -> String {
    let current = current.trim();
    if current.is_empty() {
        return added.to_owned();
    }
    if top_level_or(current, dialect.lexing()) {
        format!("({current}) AND {added}")
    } else {
        format!("{current} AND {added}")
    }
}

fn parts(condition: &str, lexing: Lexing) -> Vec<&str> {
    let scanned = scan(condition, lexing);
    let mut cuts = Vec::new();
    let tokens = &scanned.tokens;
    for (at, token) in tokens.iter().enumerate() {
        if token.depth != 0 || !is_word(condition, token, "and") {
            continue;
        }
        let between = tokens[..at]
            .iter()
            .rev()
            .take_while(|before| !is_word(condition, before, "and"))
            .any(|before| before.depth == 0 && is_word(condition, before, "between"));
        if !between {
            cuts.push((token.start, token.end));
        }
    }
    let mut out = Vec::new();
    let mut from = 0;
    for (start, end) in cuts {
        out.push(condition[from..start].trim());
        from = end;
    }
    out.push(condition[from..].trim());
    out.into_iter().filter(|part| !part.is_empty()).collect()
}

fn unquoted(token: &str) -> String {
    let inner = &token[1..token.len().saturating_sub(1).max(1)];
    match token.as_bytes().first() {
        Some(b'"') => inner.replace("\"\"", "\""),
        Some(b'`') => inner.replace("``", "`"),
        _ => inner.to_owned(),
    }
}

fn mentions(part: &str, name: &str, lexing: Lexing) -> bool {
    scan(part, lexing).tokens.iter().any(|token| match token.kind {
        Kind::Word => part[token.start..token.end].eq_ignore_ascii_case(name),
        Kind::Quoted => unquoted(&part[token.start..token.end]) == name,
        _ => false,
    })
}

pub fn without_column(condition: &str, name: &str, dialect: SqlDialect) -> String {
    let lexing = dialect.lexing();
    parts(condition, lexing).into_iter().filter(|part| !mentions(part, name, lexing)).collect::<Vec<_>>().join(" AND ")
}

pub fn columns_named(condition: &str, names: &[String], dialect: SqlDialect) -> Vec<usize> {
    let lexing = dialect.lexing();
    names.iter().enumerate().filter(|(_, name)| mentions(condition, name, lexing)).map(|(index, _)| index).collect()
}

/// A completion of the word at the caret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    /// What is shown.
    pub label: String,
    /// What replaces the word.
    pub insert: String,
    /// `column` or `keyword`.
    pub kind: String,
    /// A column's type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub const KEYWORDS: &[&str] =
    &["AND", "OR", "NOT", "IS", "NULL", "BETWEEN", "LIKE", "ILIKE", "IN", "TRUE", "FALSE", "EXISTS"];

pub fn complete(
    text: &str,
    caret: usize,
    columns: &[(String, String)],
    dialect: SqlDialect,
) -> (usize, usize, Vec<Completion>) {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let mut start = caret;
    let quote = |c: char| c == '"' || c == '`';
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_' || quote(chars[start - 1])) {
        start -= 1;
    }
    let mut end = caret;
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_' || quote(chars[end])) {
        end += 1;
    }
    let prefix: String = chars[start..caret].iter().filter(|c| !quote(**c)).collect::<String>().to_lowercase();
    let mut found: Vec<Completion> = columns
        .iter()
        .filter(|(name, _)| name.to_lowercase().starts_with(&prefix))
        .map(|(name, type_name)| Completion {
            label: name.clone(),
            insert: crate::schema::quote_when_needed(dialect.driver(), name),
            kind: "column".to_owned(),
            detail: Some(type_name.clone()),
        })
        .collect();
    if !prefix.is_empty() {
        let known = |keyword: &&&str| **keyword != "ILIKE" || dialect == SqlDialect::Postgres;
        found.extend(KEYWORDS.iter().filter(known).filter(|keyword| keyword.to_lowercase().starts_with(&prefix)).map(
            |keyword| Completion {
                label: (*keyword).to_owned(),
                insert: (*keyword).to_owned(),
                kind: "keyword".to_owned(),
                detail: None,
            },
        ));
    }
    (start, end, found)
}

#[cfg(test)]
mod tests;
