use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::SqlDialect;
use crate::export::{self, cell_text, json_value};
use crate::value::Value;

/// How values are written as text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ValueStyle {
    /// As the grid shows them; arrays as their SQL literal, bytes as `\x` and hexadecimal.
    #[default]
    Display,
    /// As SQL literals of the connection's dialect: `NULL`, `42`, `'text'`.
    Sql,
    /// As JSON values: `null`, `42`, `"text"`.
    Json,
}

/// When values are put in quotes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Quoting {
    /// Only values that hold a delimiter, the quote or a line break, and empty text when `NULL` is written as nothing.
    #[default]
    Needed,
    /// Every value that is not `NULL`.
    Always,
    /// Never: values are written as they are.
    Never,
}

/// The options of Advanced Copy; the defaults are plain ⌘C: values tab-separated, one row a line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CopySettings {
    /// Write the columns' names first.
    pub header: bool,
    /// Write each row's number first, in a column named `#`.
    pub row_numbers: bool,
    /// What separates values in a row.
    pub column_delimiter: String,
    /// What separates rows.
    pub row_delimiter: String,
    /// The character values are quoted with.
    pub quote: char,
    /// When values are quoted.
    pub quoting: Quoting,
    /// What a `NULL` is written as (display style).
    pub null_text: String,
    /// How values are written.
    pub values: ValueStyle,
}

impl Default for CopySettings {
    fn default() -> Self {
        Self {
            header: false,
            row_numbers: false,
            column_delimiter: "\t".to_owned(),
            row_delimiter: "\n".to_owned(),
            quote: '"',
            quoting: Quoting::Needed,
            null_text: String::new(),
            values: ValueStyle::Display,
        }
    }
}

impl CopySettings {
    pub fn problem(&self) -> Option<&'static str> {
        if self.column_delimiter.is_empty() || self.row_delimiter.is_empty() {
            return Some("emptyDelimiter");
        }
        if self.quoting != Quoting::Never
            && (self.column_delimiter.contains(self.quote) || self.row_delimiter.contains(self.quote))
        {
            return Some("delimiterQuote");
        }
        None
    }

    fn field(&self, text: &str) -> String {
        let quote = self.quote;
        let needs = match self.quoting {
            Quoting::Never => false,
            Quoting::Always => true,
            Quoting::Needed => {
                text.contains(self.column_delimiter.as_str())
                    || text.contains(self.row_delimiter.as_str())
                    || text.contains(quote)
                    || text.contains(['\n', '\r'])
                    || (text.is_empty() && self.null_text.is_empty())
                    || (!self.null_text.is_empty() && text == self.null_text)
            }
        };
        if needs {
            format!("{quote}{}{quote}", text.replace(quote, &format!("{quote}{quote}")))
        } else {
            text.to_owned()
        }
    }

    pub fn value(&self, value: &Value, dialect: SqlDialect) -> String {
        match self.values {
            ValueStyle::Display => match cell_text(value) {
                None => self.null_text.clone(),
                Some(text) => self.field(&text),
            },
            ValueStyle::Sql => dialect.literal(value),
            ValueStyle::Json => json_value(value),
        }
    }

    pub fn header_line(&self, names: &[String]) -> String {
        let numbers = self.row_numbers.then(|| self.field("#"));
        numbers
            .into_iter()
            .chain(names.iter().map(|name| self.field(name)))
            .collect::<Vec<_>>()
            .join(&self.column_delimiter)
    }

    pub fn row_line(&self, number: usize, values: &[Value], dialect: SqlDialect) -> String {
        let numbers = self.row_numbers.then(|| number.to_string());
        numbers
            .into_iter()
            .chain(values.iter().map(|value| self.value(value, dialect)))
            .collect::<Vec<_>>()
            .join(&self.column_delimiter)
    }
}

/// A format of Copy As.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CopyFormat {
    /// Comma-separated values with a header.
    #[default]
    Csv,
    /// Tab-separated values with a header.
    Tsv,
    /// A JSON array of objects.
    Json,
    /// A Markdown table.
    Markdown,
    /// An HTML table.
    Html,
    /// An XML document of rows.
    Xml,
    /// SQL `INSERT` statements in the connection's dialect.
    SqlInsert,
    /// SQL `UPDATE` statements of each row by its key.
    SqlUpdate,
    /// An SQL condition `column IN (…)` of the values, or `(a, b) IN ((…), …)` for several columns.
    SqlIn,
    /// A Go slice of maps.
    Go,
    /// A Rust struct and an array of its values.
    Rust,
    /// A TypeScript array of objects.
    TypeScript,
    /// A Python list of dictionaries.
    Python,
    /// A Kotlin list of maps.
    Kotlin,
    /// The columns' names, separated by commas.
    ColumnNames,
}

impl CopyFormat {
    pub const ALL: [CopyFormat; 14] = [
        CopyFormat::Csv,
        CopyFormat::Tsv,
        CopyFormat::Json,
        CopyFormat::Markdown,
        CopyFormat::Html,
        CopyFormat::Xml,
        CopyFormat::SqlInsert,
        CopyFormat::SqlUpdate,
        CopyFormat::SqlIn,
        CopyFormat::Go,
        CopyFormat::Rust,
        CopyFormat::TypeScript,
        CopyFormat::Python,
        CopyFormat::Kotlin,
    ];

    pub fn id(self) -> &'static str {
        match self {
            CopyFormat::Csv => "csv",
            CopyFormat::Tsv => "tsv",
            CopyFormat::Json => "json",
            CopyFormat::Markdown => "markdown",
            CopyFormat::Html => "html",
            CopyFormat::Xml => "xml",
            CopyFormat::SqlInsert => "sqlInsert",
            CopyFormat::SqlUpdate => "sqlUpdate",
            CopyFormat::SqlIn => "sqlIn",
            CopyFormat::Go => "go",
            CopyFormat::Rust => "rust",
            CopyFormat::TypeScript => "typeScript",
            CopyFormat::Python => "python",
            CopyFormat::Kotlin => "kotlin",
            CopyFormat::ColumnNames => "columnNames",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CopyFormat::Csv => "CSV",
            CopyFormat::Tsv => "TSV",
            CopyFormat::Json => "JSON",
            CopyFormat::Markdown => "Markdown",
            CopyFormat::Html => "HTML",
            CopyFormat::Xml => "XML",
            CopyFormat::SqlInsert => "SQL INSERT",
            CopyFormat::SqlUpdate => "SQL UPDATE",
            CopyFormat::SqlIn => "SQL IN (…)",
            CopyFormat::Go => "Go",
            CopyFormat::Rust => "Rust",
            CopyFormat::TypeScript => "TypeScript",
            CopyFormat::Python => "Python",
            CopyFormat::Kotlin => "Kotlin",
            CopyFormat::ColumnNames => "Column Names",
        }
    }

    pub fn export(self) -> Option<export::Format> {
        Some(match self {
            CopyFormat::Csv => export::Format::Csv,
            CopyFormat::Tsv => export::Format::Tsv,
            CopyFormat::Json => export::Format::Json,
            CopyFormat::Markdown => export::Format::Markdown,
            CopyFormat::Html => export::Format::Html,
            CopyFormat::Xml => export::Format::Xml,
            CopyFormat::SqlInsert => export::Format::Sql,
            _ => return None,
        })
    }

    pub fn code(self) -> bool {
        matches!(
            self,
            CopyFormat::Go | CopyFormat::Rust | CopyFormat::TypeScript | CopyFormat::Python | CopyFormat::Kotlin
        )
    }
}

pub fn export_settings(format: export::Format) -> export::Settings {
    export::Settings { format, escape_formulas: false, server_copy: false, ..export::Settings::default() }
}

pub struct InList {
    dialect: SqlDialect,
    names: Vec<String>,
    seen: BTreeSet<String>,
    items: Vec<String>,
}

impl InList {
    pub fn new(dialect: SqlDialect, names: &[String]) -> Self {
        Self { dialect, names: names.to_vec(), seen: BTreeSet::new(), items: Vec::new() }
    }

    pub fn row(&mut self, values: &[Value]) {
        let literals: Vec<String> = values.iter().map(|value| self.dialect.literal(value)).collect();
        let item = if literals.len() == 1 { literals[0].clone() } else { format!("({})", literals.join(", ")) };
        if self.seen.insert(item.clone()) {
            self.items.push(item);
        }
    }

    pub fn len(&self) -> usize {
        self.items.iter().map(|item| item.len() + 2).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn finish(self) -> String {
        let names: Vec<String> = self.names.iter().map(|name| self.dialect.quote(name)).collect();
        let left = if names.len() == 1 { names[0].clone() } else { format!("({})", names.join(", ")) };
        format!("{left} IN ({})", self.items.join(", "))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Unknown,
    Integer,
    Float,
    Bool,
    Bytes,
    Text,
}

#[derive(Clone, Debug)]
pub struct CodeColumns {
    kinds: Vec<Kind>,
    nullable: Vec<bool>,
}

impl CodeColumns {
    pub fn new(width: usize) -> Self {
        Self { kinds: vec![Kind::Unknown; width], nullable: vec![false; width] }
    }

    pub fn learn(&mut self, values: &[Value]) {
        for (at, value) in values.iter().enumerate().take(self.kinds.len()) {
            let kind = match value {
                Value::Null => {
                    self.nullable[at] = true;
                    continue;
                }
                Value::Integer(_) => Kind::Integer,
                Value::Float(_) => Kind::Float,
                Value::Decimal(text) if text.parse::<f64>().is_ok_and(f64::is_finite) => Kind::Float,
                Value::Bool(_) => Kind::Bool,
                Value::Bytes(_) => Kind::Bytes,
                _ => Kind::Text,
            };
            self.kinds[at] = match (self.kinds[at], kind) {
                (Kind::Unknown, kind) => kind,
                (known, kind) if known == kind => known,
                (Kind::Integer, Kind::Float) | (Kind::Float, Kind::Integer) => Kind::Float,
                _ => Kind::Text,
            };
        }
    }
}

fn escaped(text: &str, dollar: bool, python: bool) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' if dollar => out.push_str("\\$"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                if python {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                } else if dollar {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                } else {
                    out.push_str(&format!("\\u{{{:x}}}", c as u32));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn go_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn float_literal(value: f64, nan: &str, infinity: &str, minus_infinity: &str) -> String {
    if value.is_nan() {
        nan.to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { infinity.to_owned() } else { minus_infinity.to_owned() }
    } else {
        let text = value.to_string();
        if text.contains(['.', 'e', 'E']) { text } else { format!("{text}.0") }
    }
}

fn identifier(name: &str, taken: &mut BTreeSet<String>) -> String {
    let mut out: String =
        name.chars().map(|c| if c.is_alphanumeric() || c == '_' { c.to_ascii_lowercase() } else { '_' }).collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    let keywords = [
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl",
        "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct",
        "super", "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn",
    ];
    if keywords.contains(&out.as_str()) {
        out = format!("r#{out}");
    }
    let mut unique = out.clone();
    let mut number = 2;
    while !taken.insert(unique.clone()) {
        unique = format!("{out}_{number}");
        number += 1;
    }
    unique
}

fn plain_key(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

pub struct CodeWriter {
    format: CopyFormat,
    names: Vec<String>,
    fields: Vec<String>,
    columns: CodeColumns,
}

impl CodeWriter {
    pub fn new(format: CopyFormat, names: &[String], columns: CodeColumns) -> Self {
        let mut taken = BTreeSet::new();
        let fields = names.iter().map(|name| identifier(name, &mut taken)).collect();
        Self { format, names: names.to_vec(), fields, columns }
    }

    pub fn start(&self) -> String {
        match self.format {
            CopyFormat::Go => "[]map[string]any{\n".to_owned(),
            CopyFormat::Rust => {
                let mut out = "struct Row {\n".to_owned();
                for (at, field) in self.fields.iter().enumerate() {
                    out.push_str(&format!("    {field}: {},\n", self.rust_type(at)));
                }
                out.push_str("}\n\nlet rows = [\n");
                out
            }
            CopyFormat::TypeScript => "const rows = [\n".to_owned(),
            CopyFormat::Python => "rows = [\n".to_owned(),
            CopyFormat::Kotlin => "val rows = listOf(\n".to_owned(),
            _ => String::new(),
        }
    }

    fn rust_type(&self, at: usize) -> String {
        let base = match self.columns.kinds[at] {
            Kind::Integer => "i64",
            Kind::Float => "f64",
            Kind::Bool => "bool",
            Kind::Bytes => "&'static [u8]",
            Kind::Text | Kind::Unknown => "&'static str",
        };
        if self.columns.nullable[at] { format!("Option<{base}>") } else { base.to_owned() }
    }

    fn rust_value(&self, at: usize, value: &Value) -> String {
        if matches!(value, Value::Null) {
            return "None".to_owned();
        }
        let text = match (self.columns.kinds[at], value) {
            (Kind::Integer, Value::Integer(number)) => number.to_string(),
            (Kind::Float, Value::Integer(number)) => format!("{number}.0"),
            (Kind::Float, Value::Float(number)) => {
                float_literal(*number, "f64::NAN", "f64::INFINITY", "f64::NEG_INFINITY")
            }
            (Kind::Float, Value::Decimal(text)) if text.contains(['.', 'e', 'E']) => text.clone(),
            (Kind::Float, Value::Decimal(text)) => format!("{text}.0"),
            (Kind::Bool, Value::Bool(flag)) => flag.to_string(),
            (Kind::Bytes, Value::Bytes(bytes)) => {
                format!("&[{}]", bytes.iter().map(|byte| format!("0x{byte:02x}")).collect::<Vec<_>>().join(", "))
            }
            (_, value) => escaped(&cell_text(value).unwrap_or_default(), false, false),
        };
        if self.columns.nullable[at] { format!("Some({text})") } else { text }
    }

    fn value(&self, value: &Value) -> String {
        let text = || cell_text(value).unwrap_or_default();
        match self.format {
            CopyFormat::Go => match value {
                Value::Null => "nil".to_owned(),
                Value::Bool(flag) => flag.to_string(),
                Value::Integer(number) => number.to_string(),
                Value::Float(number) => float_literal(*number, "math.NaN()", "math.Inf(1)", "math.Inf(-1)"),
                Value::Decimal(text) if text.parse::<f64>().is_ok_and(f64::is_finite) => text.clone(),
                Value::Bytes(bytes) => format!(
                    "[]byte{{{}}}",
                    bytes.iter().map(|byte| format!("0x{byte:02x}")).collect::<Vec<_>>().join(", ")
                ),
                _ => go_string(&text()),
            },
            CopyFormat::TypeScript => match value {
                Value::Null => "null".to_owned(),
                Value::Bool(flag) => flag.to_string(),
                Value::Integer(number) if number.unsigned_abs() > (1u64 << 53) => format!("{number}n"),
                Value::Integer(number) => number.to_string(),
                Value::Float(number) => float_literal(*number, "NaN", "Infinity", "-Infinity"),
                Value::Decimal(text) if text.parse::<f64>().is_ok_and(f64::is_finite) => text.clone(),
                Value::Json(_) | Value::Array(_) | Value::Map(_) | Value::Document(_) => {
                    let json = json_value(value);
                    if serde_json::from_str::<serde::de::IgnoredAny>(&json).is_ok() {
                        json
                    } else {
                        escaped(&text(), false, false)
                    }
                }
                Value::Bytes(bytes) => {
                    format!("new Uint8Array([{}])", bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(", "))
                }
                _ => serde_json::to_string(&text()).unwrap_or_default(),
            },
            CopyFormat::Python => match value {
                Value::Null => "None".to_owned(),
                Value::Bool(true) => "True".to_owned(),
                Value::Bool(false) => "False".to_owned(),
                Value::Integer(number) => number.to_string(),
                Value::Float(number) => float_literal(*number, "float(\"nan\")", "float(\"inf\")", "float(\"-inf\")"),
                Value::Decimal(text) if text.parse::<f64>().is_ok_and(f64::is_finite) => text.clone(),
                Value::Bytes(bytes) => {
                    format!("b\"{}\"", bytes.iter().map(|byte| format!("\\x{byte:02x}")).collect::<String>())
                }
                _ => escaped(&text(), false, true),
            },
            CopyFormat::Kotlin => match value {
                Value::Null => "null".to_owned(),
                Value::Bool(flag) => flag.to_string(),
                Value::Integer(number) if i32::try_from(*number).is_ok() => number.to_string(),
                Value::Integer(number) => format!("{number}L"),
                Value::Float(number) => {
                    float_literal(*number, "Double.NaN", "Double.POSITIVE_INFINITY", "Double.NEGATIVE_INFINITY")
                }
                Value::Decimal(text) if text.parse::<f64>().is_ok_and(f64::is_finite) => {
                    if text.contains(['.', 'e', 'E']) { text.clone() } else { format!("{text}.0") }
                }
                Value::Bytes(bytes) => format!(
                    "byteArrayOf({})",
                    bytes.iter().map(|byte| i8::from_ne_bytes([*byte]).to_string()).collect::<Vec<_>>().join(", ")
                ),
                _ => escaped(&text(), true, false),
            },
            _ => text(),
        }
    }

    pub fn row(&self, values: &[Value]) -> String {
        let null = Value::Null;
        let cell = |at: usize| values.get(at).unwrap_or(&null);
        match self.format {
            CopyFormat::Rust => {
                let fields: Vec<String> = self
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(at, field)| format!("{field}: {}", self.rust_value(at, cell(at))))
                    .collect();
                format!("    Row {{ {} }},\n", fields.join(", "))
            }
            CopyFormat::Go | CopyFormat::Python => {
                let fields: Vec<String> = self
                    .names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| {
                        let key =
                            if self.format == CopyFormat::Go { go_string(name) } else { escaped(name, false, true) };
                        format!("{key}: {}", self.value(cell(at)))
                    })
                    .collect();
                let indent = if self.format == CopyFormat::Go { "\t" } else { "    " };
                format!("{indent}{{{}}},\n", fields.join(", "))
            }
            CopyFormat::TypeScript => {
                let fields: Vec<String> = self
                    .names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| {
                        let key = if plain_key(name) {
                            name.clone()
                        } else {
                            serde_json::to_string(name).unwrap_or_default()
                        };
                        format!("{key}: {}", self.value(cell(at)))
                    })
                    .collect();
                format!("  {{ {} }},\n", fields.join(", "))
            }
            CopyFormat::Kotlin => {
                let fields: Vec<String> = self
                    .names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| format!("{} to {}", escaped(name, true, false), self.value(cell(at))))
                    .collect();
                format!("    mapOf({}),\n", fields.join(", "))
            }
            _ => String::new(),
        }
    }

    pub fn finish(&self) -> String {
        match self.format {
            CopyFormat::Go => "}\n".to_owned(),
            CopyFormat::Rust => "];\n".to_owned(),
            CopyFormat::TypeScript => "];\n".to_owned(),
            CopyFormat::Python => "]\n".to_owned(),
            CopyFormat::Kotlin => ")\n".to_owned(),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub text: String,
    pub quoted: bool,
}

impl Field {
    pub fn value(&self, null_text: &str) -> Option<&str> {
        if !self.quoted && self.text == null_text { None } else { Some(&self.text) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<Vec<Field>>,
    pub delimiter: char,
}

impl Table {
    pub fn width(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }
}

fn records(text: &str, delimiter: Option<char>, quote: char, most: usize) -> Vec<Vec<Field>> {
    let mut rows = Vec::new();
    let mut row: Vec<Field> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut at_start = true;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == quote {
                if chars.peek() == Some(&quote) {
                    chars.next();
                    field.push(quote);
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        if at_start && c == quote {
            at_start = false;
            quoted = true;
            in_quotes = true;
            continue;
        }
        at_start = false;
        if Some(c) == delimiter {
            row.push(Field { text: std::mem::take(&mut field), quoted: std::mem::take(&mut quoted) });
            at_start = true;
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(Field { text: std::mem::take(&mut field), quoted: std::mem::take(&mut quoted) });
            rows.push(std::mem::take(&mut row));
            at_start = true;
            if rows.len() >= most {
                return rows;
            }
            if chars.peek().is_none() {
                return rows;
            }
        } else {
            field.push(c);
        }
    }
    if !at_start || !field.is_empty() || quoted || !row.is_empty() {
        row.push(Field { text: field, quoted });
        rows.push(row);
    }
    rows
}

pub fn detect_delimiter(text: &str, quote: char) -> char {
    let mut counts: Vec<[usize; 4]> = Vec::new();
    let mut current = [0usize; 4];
    let mut in_quotes = false;
    let mut at_start = true;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if counts.len() >= 20 {
            break;
        }
        if in_quotes {
            if c == quote {
                if chars.peek() == Some(&quote) {
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            continue;
        }
        if c == quote && at_start {
            in_quotes = true;
            at_start = false;
            continue;
        }
        at_start = false;
        match c {
            '\t' => current[0] += 1,
            ',' => current[1] += 1,
            ';' => current[2] += 1,
            '|' => current[3] += 1,
            '\n' | '\r' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                counts.push(std::mem::take(&mut current));
                at_start = true;
                continue;
            }
            _ => continue,
        }
        at_start = true;
    }
    if current.iter().any(|count| *count > 0) || counts.is_empty() {
        counts.push(current);
    }
    if counts.iter().any(|count| count[0] > 0) {
        return '\t';
    }
    let candidates = [(1, ','), (2, ';'), (3, '|')];
    for (at, delimiter) in candidates {
        let first = counts[0][at];
        if first > 0 && counts.iter().all(|count| count[at] == first) {
            return delimiter;
        }
    }
    candidates
        .iter()
        .map(|(at, delimiter)| (counts.iter().map(|count| count[*at]).sum::<usize>(), *delimiter))
        .filter(|(total, _)| *total > 0)
        .max_by_key(|(total, _)| *total)
        .map_or('\t', |(_, delimiter)| delimiter)
}

pub fn parse(text: &str, delimiter: Option<char>, quote: char, most: usize) -> Table {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let delimiter = delimiter.unwrap_or_else(|| detect_delimiter(text, quote));
    Table { rows: records(text, Some(delimiter), quote, most), delimiter }
}

pub fn names_columns(row: &[Field], names: &[String]) -> bool {
    !row.is_empty()
        && row.iter().any(|field| !field.text.trim().is_empty())
        && row.iter().all(|field| {
            let text = field.text.trim();
            names.iter().any(|name| name.eq_ignore_ascii_case(text))
        })
}

#[cfg(test)]
mod tests;
