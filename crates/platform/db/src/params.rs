use std::collections::BTreeMap;
use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::{Placeholders, SqlDialect};
use crate::sql::Dialect;
use crate::value::Value;

mod infer;

pub use infer::{column_type, infer_types, mentions};

const MASK: &str = "***";

/// How a placeholder is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PlaceholderKind {
    /// `:name`.
    Named,
    /// `$1` (PostgreSQL, SQLite and other standard SQL) or `?1` (outside PostgreSQL).
    Numbered,
    /// `?` (MySQL, MariaDB, SQLite and others; not PostgreSQL, where `?` is a JSON operator).
    Question,
}

/// A placeholder of a statement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Placeholder {
    /// How it is written.
    pub kind: PlaceholderKind,
    /// Its name: `id` for `:id`, `$1` for `$1`, `?1` for `?1` and for the first `?` of the statement.
    pub name: String,
    /// Where it starts in the statement, in bytes.
    pub start: usize,
    /// The byte after it.
    pub end: usize,
}

/// The type a value of a parameter is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ParamType {
    /// Text; the server converts it to the parameter's type.
    #[default]
    Text,
    /// A number.
    Number,
    /// `true` or `false`.
    Boolean,
    /// SQL `NULL`.
    Null,
}

/// A value of a parameter as a person or an agent gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParamValue {
    /// Its type.
    #[serde(rename = "type", default)]
    pub kind: ParamType,
    /// Its text; ignored for `null`.
    #[serde(default)]
    pub value: String,
}

/// A value bound to a statement, with the name of its placeholder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NamedValue {
    /// The placeholder's name.
    pub name: String,
    /// The value.
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bound {
    pub text: String,
    pub params: Vec<NamedValue>,
    pub inlined: Vec<NamedValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptCommand {
    Set { name: String, value: String },
    Include { file: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamError {
    NotANumber(String),
    NotABoolean(String),
    NotAValue,
    IncludeNeedsFile,
    SetNeedsValue,
    NotAVariableName(String),
}

impl ParamValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self { kind: ParamType::Text, value: value.into() }
    }

    pub fn null() -> Self {
        Self { kind: ParamType::Null, value: String::new() }
    }

    pub fn of_json(json: &serde_json::Value) -> Result<Self, ParamError> {
        match json {
            serde_json::Value::Null => Ok(Self::null()),
            serde_json::Value::Bool(value) => Ok(Self { kind: ParamType::Boolean, value: value.to_string() }),
            serde_json::Value::Number(number) => Ok(Self { kind: ParamType::Number, value: number.to_string() }),
            serde_json::Value::String(text) => Ok(Self::text(text.clone())),
            serde_json::Value::Object(_) => serde_json::from_value(json.clone()).map_err(|_| ParamError::NotAValue),
            serde_json::Value::Array(_) => Err(ParamError::NotAValue),
        }
    }

    pub fn to_value(&self) -> Result<Value, ParamError> {
        let text = self.value.trim();
        match self.kind {
            ParamType::Null => Ok(Value::Null),
            ParamType::Text => Ok(Value::Text(self.value.clone())),
            ParamType::Number => {
                if let Ok(integer) = text.parse::<i64>() {
                    Ok(Value::Integer(integer))
                } else if !text.is_empty()
                    && text.parse::<f64>().is_ok_and(f64::is_finite)
                    && text
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+' | b'e' | b'E'))
                {
                    Ok(Value::Decimal(text.to_owned()))
                } else {
                    Err(ParamError::NotANumber(text.to_owned()))
                }
            }
            ParamType::Boolean => match text.to_ascii_lowercase().as_str() {
                "true" | "t" | "yes" | "y" | "1" | "on" => Ok(Value::Bool(true)),
                "false" | "f" | "no" | "n" | "0" | "off" => Ok(Value::Bool(false)),
                _ => Err(ParamError::NotABoolean(text.to_owned())),
            },
        }
    }
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn word_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && is_word(bytes[at]) {
        at += 1;
    }
    at
}

fn closed(bytes: &[u8], mut at: usize, quote: u8, backslash: bool) -> usize {
    while at < bytes.len() {
        if backslash && bytes[at] == b'\\' {
            at += 2;
            continue;
        }
        if bytes[at] == quote {
            if bytes.get(at + 1) == Some(&quote) {
                at += 2;
                continue;
            }
            return at + 1;
        }
        at += 1;
    }
    bytes.len()
}

fn line_end(bytes: &[u8], at: usize) -> usize {
    bytes[at..].iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |found| at + found)
}

fn comment_end(bytes: &[u8], mut at: usize, nests: bool) -> usize {
    let mut depth = 1_u32;
    while at + 1 < bytes.len() {
        if bytes[at] == b'*' && bytes[at + 1] == b'/' {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return at;
            }
            continue;
        }
        if nests && bytes[at] == b'/' && bytes[at + 1] == b'*' {
            depth += 1;
            at += 2;
            continue;
        }
        at += 1;
    }
    bytes.len()
}

fn dollar_tag(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut end = at + 1;
    if end < bytes.len() && is_word_start(bytes[end]) {
        end = word_end(bytes, end);
    }
    (end < bytes.len() && bytes[end] == b'$').then_some(end + 1)
}

fn code_mask(text: &str, dialect: Dialect) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut mask = vec![true; bytes.len()];
    let mysql = dialect == Dialect::MySql;
    let postgres = dialect == Dialect::Postgres;
    let nests = matches!(dialect, Dialect::Postgres | Dialect::SqlServer);
    let mut at = 0;
    while at < bytes.len() {
        let before = at.checked_sub(1).map(|before| bytes[before]);
        let end = match bytes[at] {
            b'\'' => {
                let escape = postgres
                    && matches!(before, Some(b'e' | b'E'))
                    && at.checked_sub(2).is_none_or(|word| !is_word(bytes[word]));
                Some(closed(bytes, at + 1, b'\'', mysql || escape))
            }
            b'"' => Some(closed(bytes, at + 1, b'"', mysql)),
            b'`' if mysql => Some(closed(bytes, at + 1, b'`', false)),
            b'[' if dialect == Dialect::SqlServer => Some(closed(bytes, at + 1, b']', false)),
            b'-' if bytes.get(at + 1) == Some(&b'-')
                && (!mysql || bytes.get(at + 2).is_none_or(u8::is_ascii_whitespace)) =>
            {
                Some(line_end(bytes, at))
            }
            b'#' if mysql => Some(line_end(bytes, at)),
            b'/' if bytes.get(at + 1) == Some(&b'*') => Some(comment_end(bytes, at + 2, nests)),
            b'$' if postgres && before.is_none_or(|before| !is_word(before)) => dollar_tag(text, at).map(|tag_end| {
                let tag = &text[at..tag_end];
                text[tag_end..].find(tag).map_or(bytes.len(), |found| tag_end + found + tag.len())
            }),
            _ => None,
        };
        match end {
            Some(end) => {
                let end = end.clamp(at + 1, bytes.len());
                mask[at..end].iter_mut().for_each(|code| *code = false);
                at = end;
            }
            None => at += 1,
        }
    }
    mask
}

pub fn placeholders(text: &str, dialect: Dialect) -> Vec<Placeholder> {
    if dialect == Dialect::Lines {
        return Vec::new();
    }
    let mask = code_mask(text, dialect);
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut questions = 0;
    let mut at = 0;
    while at < bytes.len() {
        if !mask[at] {
            at += 1;
            continue;
        }
        let before = at.checked_sub(1).map(|before| bytes[before]);
        let free = before.is_none_or(|before| !is_word(before) && !matches!(before, b':' | b'$' | b'@'));
        match bytes[at] {
            b':' if free && bytes.get(at + 1).is_some_and(|next| is_word_start(*next)) => {
                let end = word_end(bytes, at + 1);
                found.push(Placeholder {
                    kind: PlaceholderKind::Named,
                    name: text[at + 1..end].to_owned(),
                    start: at,
                    end,
                });
                at = end;
            }
            b'$' if matches!(dialect, Dialect::Postgres | Dialect::Standard)
                && free
                && bytes.get(at + 1).is_some_and(u8::is_ascii_digit) =>
            {
                let mut end = at + 1;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                if bytes.get(end).is_some_and(|next| is_word(*next)) {
                    at = end;
                    continue;
                }
                found.push(Placeholder {
                    kind: PlaceholderKind::Numbered,
                    name: text[at..end].to_owned(),
                    start: at,
                    end,
                });
                at = end;
            }
            b'?' if dialect != Dialect::Postgres && bytes.get(at + 1).is_some_and(u8::is_ascii_digit) => {
                let mut end = at + 1;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                questions = questions.max(text[at + 1..end].parse().unwrap_or(0));
                found.push(Placeholder {
                    kind: PlaceholderKind::Numbered,
                    name: text[at..end].to_owned(),
                    start: at,
                    end,
                });
                at = end;
            }
            b'?' if dialect != Dialect::Postgres => {
                questions += 1;
                found.push(Placeholder {
                    kind: PlaceholderKind::Question,
                    name: format!("?{questions}"),
                    start: at,
                    end: at + 1,
                });
                at += 1;
            }
            _ => at += 1,
        }
    }
    found
}

pub fn names(text: &str, dialect: Dialect) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for placeholder in placeholders(text, dialect) {
        if !names.contains(&placeholder.name) {
            names.push(placeholder.name);
        }
    }
    names
}

pub fn bind(
    text: &str,
    dialect: Dialect,
    (sql, binds): (SqlDialect, bool),
    values: &BTreeMap<String, Value>,
) -> Result<Bound, Vec<String>> {
    let found = placeholders(text, dialect);
    let mut missing: Vec<String> = Vec::new();
    for placeholder in &found {
        if !values.contains_key(&placeholder.name) && !missing.contains(&placeholder.name) {
            missing.push(placeholder.name.clone());
        }
    }
    if !missing.is_empty() {
        return Err(missing);
    }
    let style = if binds { sql.placeholders() } else { Placeholders::Inline };
    let mut out = String::with_capacity(text.len());
    let mut params: Vec<NamedValue> = Vec::new();
    let mut inlined: Vec<NamedValue> = Vec::new();
    let mut copied = 0;
    for placeholder in &found {
        let value = values.get(&placeholder.name).cloned().unwrap_or(Value::Null);
        out.push_str(&text[copied..placeholder.start]);
        copied = placeholder.end;
        match style {
            Placeholders::Numbered => {
                let number = match params.iter().position(|bound| bound.name == placeholder.name) {
                    Some(index) => index + 1,
                    None => {
                        params.push(NamedValue { name: placeholder.name.clone(), value });
                        params.len()
                    }
                };
                out.push_str(&format!("${number}"));
            }
            Placeholders::Named => {
                let number = match params.iter().position(|bound| bound.name == placeholder.name) {
                    Some(index) => index + 1,
                    None => {
                        params.push(NamedValue { name: placeholder.name.clone(), value });
                        params.len()
                    }
                };
                out.push_str(&format!("@P{number}"));
            }
            Placeholders::Question => {
                out.push('?');
                params.push(NamedValue { name: placeholder.name.clone(), value });
            }
            Placeholders::Inline => {
                let literal = sql.literal(&value);
                if literal.starts_with('-') && out.ends_with('-') {
                    out.push(' ');
                }
                out.push_str(&literal);
                if !inlined.iter().any(|known| known.name == placeholder.name) {
                    inlined.push(NamedValue { name: placeholder.name.clone(), value });
                }
            }
        }
    }
    out.push_str(&text[copied..]);
    Ok(Bound { text: out, params, inlined })
}

pub fn variables(text: &str) -> Vec<(Range<usize>, String)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = text[at..].find("${") {
        let start = at + offset;
        let name_start = start + 2;
        let name_end = word_end(bytes, name_start);
        if name_end > name_start && bytes.get(name_end) == Some(&b'}') {
            found.push((start..name_end + 1, text[name_start..name_end].to_owned()));
            at = name_end + 1;
        } else {
            at = name_start;
        }
    }
    found
}

pub fn expand(text: &str, values: &BTreeMap<String, String>) -> Result<String, Vec<String>> {
    let found = variables(text);
    let mut missing: Vec<String> = Vec::new();
    for (_, name) in &found {
        if !values.contains_key(name) && !missing.contains(name) {
            missing.push(name.clone());
        }
    }
    if !missing.is_empty() {
        return Err(missing);
    }
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (range, name) in found {
        out.push_str(&text[copied..range.start]);
        out.push_str(values.get(&name).map_or("", String::as_str));
        copied = range.end;
    }
    out.push_str(&text[copied..]);
    Ok(out)
}

pub fn is_script_command(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix('@') else { return false };
    let word_len = rest.bytes().take_while(|byte| byte.is_ascii_alphabetic()).count();
    let (word, after) = rest.split_at(word_len);
    (word.eq_ignore_ascii_case("set") || word.eq_ignore_ascii_case("include"))
        && after.chars().next().is_none_or(char::is_whitespace)
}

pub fn script_command(statement: &str) -> Option<Result<ScriptCommand, ParamError>> {
    let trimmed = statement.trim();
    if !is_script_command(trimmed) {
        return None;
    }
    let rest = &trimmed[1..];
    let word_len = rest.bytes().take_while(|byte| byte.is_ascii_alphabetic()).count();
    let (word, after) = rest.split_at(word_len);
    let after = after.trim().trim_end_matches(';').trim_end();
    if word.eq_ignore_ascii_case("include") {
        let file = unquote(after);
        if file.is_empty() {
            return Some(Err(ParamError::IncludeNeedsFile));
        }
        return Some(Ok(ScriptCommand::Include { file: file.to_owned() }));
    }
    let Some((name, value)) = after.split_once('=') else {
        return Some(Err(ParamError::SetNeedsValue));
    };
    let name = name.trim();
    let bytes = name.as_bytes();
    if bytes.is_empty() || !is_word_start(bytes[0]) || word_end(bytes, 0) != bytes.len() {
        return Some(Err(ParamError::NotAVariableName(name.to_owned())));
    }
    Some(Ok(ScriptCommand::Set { name: name.to_owned(), value: unquote(value.trim()).to_owned() }))
}

fn unquote(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'"' | b'\'') && bytes[bytes.len() - 1] == bytes[0] {
        &text[1..text.len() - 1]
    } else {
        text
    }
}

pub fn secret_name(name: &str) -> bool {
    const WORDS: [&str; 10] = [
        "password",
        "passwd",
        "pwd",
        "secret",
        "token",
        "apikey",
        "api_key",
        "credential",
        "private_key",
        "privatekey",
    ];
    let lower = name.to_ascii_lowercase();
    WORDS.iter().any(|word| lower.contains(word))
}

pub fn shown(values: &[NamedValue], sql: SqlDialect) -> Vec<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut lines = Vec::new();
    for bound in values {
        if seen.contains(&bound.name.as_str()) {
            continue;
        }
        seen.push(&bound.name);
        let literal = if secret_name(&bound.name) && bound.value != Value::Null {
            MASK.to_owned()
        } else {
            let literal = sql.literal(&bound.value);
            match literal.char_indices().nth(200) {
                Some((end, _)) => format!("{}…", &literal[..end]),
                None => literal,
            }
        };
        let name = if bound.name.starts_with(['$', '?']) { bound.name.clone() } else { format!(":{}", bound.name) };
        lines.push(format!("{name} = {literal}"));
    }
    lines
}

#[cfg(test)]
mod tests;
