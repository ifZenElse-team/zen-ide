use std::sync::Arc;

use zen_syntax::inject::{Injector, Literal};
use zen_syntax::{HighlightKind, Highlighter, Span};

use crate::dialect::{Lexing, SqlDialect};
use crate::filter::{Kind, scan};

const HOSTS: &[&str] =
    &["go", "rs", "py", "pyi", "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "java", "kt", "kts"];

const LANGUAGES: &[&str] =
    &["sql", "postgresql", "postgres", "pgsql", "mysql", "mariadb", "sqlite", "clickhouse", "plsql", "tsql"];

const STARTS: &[&str] = &[
    "select", "insert", "update", "delete", "with", "create", "alter", "drop", "truncate", "merge", "replace", "grant",
    "revoke", "explain",
];

const OBJECTS: &[&str] = &[
    "table",
    "view",
    "index",
    "schema",
    "database",
    "sequence",
    "function",
    "procedure",
    "trigger",
    "type",
    "extension",
    "materialized",
    "role",
    "user",
    "domain",
    "policy",
    "unique",
    "temporary",
    "temp",
    "or",
    "if",
];

const SCANNED_WORDS: usize = 64;

pub fn host_extensions() -> &'static [&'static str] {
    HOSTS
}

pub fn hosts(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.rsplit_once('.').is_some_and(|(_, extension)| HOSTS.contains(&extension))
}

pub struct SqlStrings {
    highlighter: Option<Arc<dyn Highlighter>>,
}

impl SqlStrings {
    pub fn new(highlighter: Option<Arc<dyn Highlighter>>) -> Self {
        Self { highlighter }
    }
}

impl Injector for SqlStrings {
    fn language(&self) -> &str {
        "sql"
    }

    fn may_hold(&self, text: &str) -> bool {
        let bytes = text.as_bytes();
        bytes.windows(4).any(|window| {
            let lower = [
                window[0].to_ascii_lowercase(),
                window[1].to_ascii_lowercase(),
                window[2].to_ascii_lowercase(),
                window[3].to_ascii_lowercase(),
            ];
            STARTS.iter().any(|start| start.as_bytes()[..4] == lower) || &lower == b"lang" || &lower[..3] == b"sql"
        })
    }

    fn accepts(&self, text: &str, literal: &Literal, code: &str) -> bool {
        if let Some(language) = directive(text, literal.range.start) {
            return LANGUAGES.contains(&language.as_str());
        }
        if literal.quote == b'`' && tagged(&text[..literal.range.start]) {
            return true;
        }
        looks_like_sql(code)
    }

    fn highlight(&self, code: &str) -> Vec<Span> {
        match &self.highlighter {
            Some(highlighter) => highlighter.highlight(code),
            None => highlight(code, SqlDialect::Postgres),
        }
    }
}

fn directive(text: &str, before: usize) -> Option<String> {
    let line = text[..before].rfind('\n').map_or(0, |at| at + 1);
    let previous = text[..line.saturating_sub(1)].rfind('\n').map_or(0, |at| at + 1);
    let near = &text[previous..before];
    let lower = near.to_ascii_lowercase();
    let at = lower.rfind("language")?;
    let rest = lower[at + "language".len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let word: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    (!word.is_empty()).then_some(word)
}

fn tagged(before: &str) -> bool {
    let head = match before.strip_suffix('>').and_then(|inner| inner.rfind('<')) {
        Some(open) => &before[..open],
        None => before,
    };
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
        .last()
        .map_or(head.len(), |(at, _)| at);
    head[start..].eq_ignore_ascii_case("sql")
}

fn words(code: &str) -> Vec<&str> {
    code.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .take(SCANNED_WORDS)
        .collect()
}

pub fn looks_like_sql(code: &str) -> bool {
    let trimmed = code.trim_start().trim_start_matches('(').trim_start();
    let words = words(trimmed);
    let Some(first) = words.first() else { return false };
    if !trimmed.starts_with(first) {
        return false;
    }
    let upper = first.bytes().all(|byte| byte.is_ascii_uppercase());
    let lower = first.bytes().all(|byte| byte.is_ascii_lowercase());
    if !upper && !lower {
        return false;
    }
    let keyword = first.to_ascii_lowercase();
    let rest: Vec<String> = words[1..].iter().map(|word| word.to_ascii_lowercase()).collect();
    let next = rest.first().map(String::as_str);
    let has = |wanted: &str, within: usize| rest.iter().take(within).any(|word| word == wanted);
    match keyword.as_str() {
        "select" if upper => next.is_some() || trimmed[first.len()..].trim_start().starts_with('*'),
        "select" => has("from", SCANNED_WORDS),
        "insert" => matches!(next, Some("into" | "ignore" | "or")),
        "replace" | "merge" => next == Some("into"),
        "update" => next.is_some() && has("set", 12),
        "delete" => has("from", 3),
        "with" => {
            has("as", SCANNED_WORDS)
                && rest.iter().any(|word| matches!(word.as_str(), "select" | "insert" | "update" | "delete"))
        }
        "create" | "alter" | "drop" => next.is_some_and(|word| OBJECTS.contains(&word)),
        "truncate" => next.is_some() && (upper || next == Some("table")),
        "grant" | "revoke" => has("on", 12) && (has("to", SCANNED_WORDS) || has("from", SCANNED_WORDS)),
        "explain" => {
            let after = trimmed[first.len()..].trim_start();
            matches!(next, Some("analyze" | "verbose" | "query")) || looks_like_sql(after)
        }
        _ => false,
    }
}

fn lenient(dialect: SqlDialect) -> Lexing {
    Lexing { backticks: true, ..dialect.lexing() }
}

pub fn highlight(code: &str, dialect: SqlDialect) -> Vec<Span> {
    let tokens = scan(code, lenient(dialect)).tokens;
    let bytes = code.as_bytes();
    let mut spans = Vec::with_capacity(tokens.len());
    let marks =
        |at: usize| at > 0 && matches!(bytes[at - 1], b':' | b'$' | b'@') && !(at >= 2 && bytes[at - 2] == b':');
    for (index, token) in tokens.iter().enumerate() {
        let text = &code[token.start..token.end];
        let next = tokens.get(index + 1).filter(|after| after.start == token.end);
        let kind = match token.kind {
            Kind::Comment => Some(HighlightKind::Comment),
            Kind::Literal => Some(HighlightKind::String),
            Kind::Word if marks(token.start) => Some(HighlightKind::Constant),
            Kind::Word if text.as_bytes()[0].is_ascii_digit() => Some(HighlightKind::Number),
            Kind::Word if crate::assist::is_sql_keyword(text) => Some(HighlightKind::Keyword),
            Kind::Word => {
                next.is_some_and(|after| &code[after.start..after.end] == "(").then_some(HighlightKind::Function)
            }
            Kind::Symbol if text == "?" => Some(HighlightKind::Constant),
            Kind::Symbol if matches!(text, "$" | ":" | "@") => next
                .is_some_and(|after| after.kind == Kind::Word && marks(after.start))
                .then_some(HighlightKind::Constant),
            Kind::Quoted | Kind::Symbol => None,
        };
        if let Some(kind) = kind {
            spans.push(Span { range: token.start..token.end, kind });
        }
    }
    spans
}

#[cfg(test)]
mod tests;
