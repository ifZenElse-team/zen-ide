use std::io::{self, Write};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::SqlDialect;
use crate::value::{Column, Value};

/// A format rows are written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    /// Comma-separated values.
    #[default]
    Csv,
    /// Tab-separated values: CSV with a tab between fields.
    Tsv,
    /// JSON: an array of objects, or one object a line.
    Json,
    /// SQL `INSERT` statements in the connection's dialect.
    Sql,
    /// A Markdown table.
    Markdown,
    /// An HTML page with one table.
    Html,
    /// An XML document of rows.
    Xml,
    /// Apache Parquet: typed columns in row groups, compressed.
    Parquet,
}

impl Format {
    pub const ALL: [Format; 8] = [
        Format::Csv,
        Format::Tsv,
        Format::Json,
        Format::Sql,
        Format::Markdown,
        Format::Html,
        Format::Xml,
        Format::Parquet,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Tsv => "tsv",
            Format::Json => "json",
            Format::Sql => "sql",
            Format::Markdown => "markdown",
            Format::Html => "html",
            Format::Xml => "xml",
            Format::Parquet => "parquet",
        }
    }

    pub fn binary(self) -> bool {
        self == Format::Parquet
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Csv => "CSV",
            Format::Tsv => "TSV",
            Format::Json => "JSON",
            Format::Sql => "SQL INSERT",
            Format::Markdown => "Markdown",
            Format::Html => "HTML",
            Format::Xml => "XML",
            Format::Parquet => "Parquet",
        }
    }

    pub fn extension(self, settings: &Settings) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Tsv => "tsv",
            Format::Json if settings.json == JsonStyle::Lines => "jsonl",
            Format::Json => "json",
            Format::Sql => "sql",
            Format::Markdown => "md",
            Format::Html => "html",
            Format::Xml => "xml",
            Format::Parquet => "parquet",
        }
    }
}

/// The codec Parquet pages are compressed with; all of them pure Rust.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Compression {
    /// Not compressed.
    None,
    /// Snappy, what most readers expect.
    #[default]
    Snappy,
    /// Gzip.
    Gzip,
    /// LZ4 (Parquet's `LZ4_RAW`).
    Lz4,
}

impl Compression {
    pub const ALL: [Compression; 4] = [Compression::Snappy, Compression::Gzip, Compression::Lz4, Compression::None];

    pub fn name(self) -> &'static str {
        match self {
            Compression::None => "None",
            Compression::Snappy => "Snappy",
            Compression::Gzip => "Gzip",
            Compression::Lz4 => "LZ4",
        }
    }
}

pub const ROW_GROUP: usize = 65_536;

/// How JSON is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum JsonStyle {
    /// One array of objects, an object a line.
    #[default]
    Array,
    /// One object a line and nothing else (JSON Lines).
    Lines,
}

/// What ends a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LineEnding {
    /// `\n`.
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
}

impl LineEnding {
    fn text(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        }
    }
}

/// How rows are written; every format reads the settings it has and ignores the others.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The format.
    pub format: Format,
    /// What separates CSV fields; TSV always uses a tab.
    pub delimiter: String,
    /// The character that quotes CSV and TSV fields.
    pub quote: char,
    /// Quote every CSV and TSV field, not only those that need it.
    pub quote_all: bool,
    /// Write the column names first (CSV, TSV); Markdown and HTML always have them.
    pub header: bool,
    /// What a `NULL` is written as in CSV, TSV, Markdown and HTML.
    pub null_text: String,
    /// What ends a line.
    pub line_ending: LineEnding,
    /// Put `'` before text starting with `=`, `+`, `-`, `@`, a tab or a carriage return, so spreadsheets do not run it as a formula (CSV, TSV).
    pub escape_formulas: bool,
    /// Start the file with a UTF-8 byte order mark.
    pub bom: bool,
    /// How JSON is laid out.
    pub json: JsonStyle,
    /// The table `INSERT`s write into, as SQL text written as given; the source table when absent.
    pub table: Option<String>,
    /// Rows per `INSERT` statement, from 1.
    pub batch: usize,
    /// Let PostgreSQL write CSV and TSV itself (`COPY (query) TO STDOUT`) when the settings allow.
    pub server_copy: bool,
    /// The codec of Parquet pages.
    pub compression: Compression,
    /// Rows in one Parquet row group, from 1 000 to 1 000 000; the most rows held in memory while writing.
    pub row_group: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format: Format::Csv,
            delimiter: ",".to_owned(),
            quote: '"',
            quote_all: false,
            header: true,
            null_text: String::new(),
            line_ending: LineEnding::Lf,
            escape_formulas: true,
            bom: false,
            json: JsonStyle::Array,
            table: None,
            batch: 100,
            server_copy: true,
            compression: Compression::Snappy,
            row_group: ROW_GROUP,
        }
    }
}

impl Settings {
    pub fn separator(&self) -> &str {
        if self.format == Format::Tsv { "\t" } else { &self.delimiter }
    }

    pub fn problem(&self) -> Option<&'static str> {
        if matches!(self.format, Format::Csv | Format::Tsv) {
            let separator = self.separator();
            if separator.is_empty() {
                return Some("emptyDelimiter");
            }
            if separator.contains(self.quote) || matches!(self.quote, '\n' | '\r') || separator.contains(['\n', '\r']) {
                return Some("delimiterQuote");
            }
        }
        None
    }

    pub fn copy(&self) -> Option<CopyOptions> {
        if !self.server_copy || !matches!(self.format, Format::Csv | Format::Tsv) || self.problem().is_some() {
            return None;
        }
        let mut separator = self.separator().chars();
        let delimiter = separator.next().filter(|c| c.is_ascii() && separator.next().is_none())?;
        let quote = Some(self.quote).filter(char::is_ascii)?;
        let null_safe = !self.null_text.contains([delimiter, quote, '\n', '\r', '\\']);
        null_safe.then(|| CopyOptions {
            delimiter,
            quote,
            header: self.header,
            null_text: self.null_text.clone(),
            quote_all: self.quote_all,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyOptions {
    pub delimiter: char,
    pub quote: char,
    pub header: bool,
    pub null_text: String,
    pub quote_all: bool,
}

impl CopyOptions {
    pub fn statement(&self, query: &str) -> String {
        let query = query.trim().trim_end_matches(';').trim_end();
        let literal = |text: &str| format!("'{}'", text.replace('\'', "''"));
        let mut options = format!(
            "FORMAT csv, DELIMITER {}, QUOTE {}, ESCAPE {}, HEADER {}, NULL {}, ENCODING 'UTF8'",
            literal(&self.delimiter.to_string()),
            literal(&self.quote.to_string()),
            literal(&self.quote.to_string()),
            if self.header { "true" } else { "false" },
            literal(&self.null_text),
        );
        if self.quote_all {
            options.push_str(", FORCE_QUOTE *");
        }
        format!("COPY (\n{query}\n) TO STDOUT WITH ({options})")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub dialect: SqlDialect,
    pub table: String,
    pub title: String,
}

fn formula(text: &str) -> bool {
    text.starts_with(['=', '+', '-', '@', '\t', '\r'])
}

pub fn cell_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Array(items) => Some(crate::present::array_literal(items)),
        other => other.display(),
    }
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

pub fn json_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Integer(value) => value.to_string(),
        Value::Float(value) if value.is_finite() => {
            let text = value.to_string();
            if text.contains(['.', 'e', 'E']) { text } else { format!("{text}.0") }
        }
        Value::Decimal(text) if serde_json::from_str::<serde_json::Number>(text).is_ok() => text.clone(),
        Value::Json(text) if serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok() => text.trim().to_owned(),
        Value::Array(items) => format!("[{}]", items.iter().map(json_value).collect::<Vec<_>>().join(",")),
        Value::Map(fields) | Value::Document(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, value)| format!("{}:{}", json_string(key), json_value(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        other => json_string(&other.display().unwrap_or_default()),
    }
}

fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if u32::from(c) < 0x20 || matches!(c, '\u{fffe}' | '\u{ffff}') => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

fn markdown_cell(text: &str) -> String {
    text.replace('\\', "\\\\").replace('|', "\\|").replace("\r\n", "<br>").replace(['\n', '\r'], "<br>")
}

pub struct Writer<W: Write> {
    out: W,
    settings: Settings,
    columns: Vec<Column>,
    target: Target,
    rows: u64,
    in_statement: usize,
    eol: &'static str,
}

impl<W: Write> Writer<W> {
    pub fn new(out: W, settings: &Settings, columns: Vec<Column>, target: Target) -> io::Result<Self> {
        if settings.format.binary() {
            return Err(io::Error::other("binary"));
        }
        let mut writer = Self {
            out,
            settings: settings.clone(),
            columns,
            target,
            rows: 0,
            in_statement: 0,
            eol: settings.line_ending.text(),
        };
        writer.settings.batch = writer.settings.batch.max(1);
        writer.start()?;
        Ok(writer)
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }

    fn start(&mut self) -> io::Result<()> {
        if self.settings.bom {
            self.out.write_all("\u{feff}".as_bytes())?;
        }
        let eol = self.eol;
        match self.settings.format {
            Format::Csv | Format::Tsv if self.settings.header => {
                let names: Vec<String> =
                    self.columns.iter().map(|column| self.csv_field(&column.name, false)).collect();
                write!(self.out, "{}{eol}", names.join(self.settings.separator()))
            }
            Format::Csv | Format::Tsv | Format::Sql | Format::Parquet => Ok(()),
            Format::Json => match self.settings.json {
                JsonStyle::Array => write!(self.out, "["),
                JsonStyle::Lines => Ok(()),
            },
            Format::Markdown => {
                let names: Vec<String> = self.columns.iter().map(|column| markdown_cell(&column.name)).collect();
                let rule: Vec<&str> = self.columns.iter().map(|_| "---").collect();
                write!(self.out, "| {} |{eol}| {} |{eol}", names.join(" | "), rule.join(" | "))
            }
            Format::Html => {
                let title = html_escape(&self.target.title);
                write!(
                    self.out,
                    "<!DOCTYPE html>{eol}<html>{eol}<head>{eol}<meta charset=\"utf-8\">{eol}<title>{title}</title>{eol}</head>{eol}<body>{eol}<table border=\"1\">{eol}<thead>{eol}<tr>"
                )?;
                for column in &self.columns {
                    write!(self.out, "<th>{}</th>", html_escape(&column.name))?;
                }
                write!(self.out, "</tr>{eol}</thead>{eol}<tbody>{eol}")
            }
            Format::Xml => {
                write!(
                    self.out,
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>{eol}<export name=\"{}\">{eol}",
                    xml_escape(&self.target.title)
                )
            }
        }
    }

    fn csv_field(&self, text: &str, escape: bool) -> String {
        let quote = self.settings.quote;
        let separator = self.settings.separator();
        let escaped =
            if escape && self.settings.escape_formulas && formula(text) { format!("'{text}") } else { text.to_owned() };
        let needs = self.settings.quote_all
            || escaped.contains(separator)
            || escaped.contains(quote)
            || escaped.contains(['\n', '\r'])
            || (escaped.is_empty() && self.settings.null_text.is_empty())
            || escaped == self.settings.null_text;
        if needs {
            let doubled = escaped.replace(quote, &format!("{quote}{quote}"));
            format!("{quote}{doubled}{quote}")
        } else {
            escaped
        }
    }

    fn csv_cell(&self, value: &Value) -> String {
        match cell_text(value) {
            None => self.settings.null_text.clone(),
            Some(text) => self.csv_field(&text, matches!(value, Value::Text(_) | Value::Other(_))),
        }
    }

    pub fn row(&mut self, row: &[Value]) -> io::Result<()> {
        let eol = self.eol;
        let width = self.columns.len();
        let cell = |at: usize| row.get(at).unwrap_or(&Value::Null);
        match self.settings.format {
            Format::Csv | Format::Tsv => {
                let fields: Vec<String> = (0..width).map(|at| self.csv_cell(cell(at))).collect();
                write!(self.out, "{}{eol}", fields.join(self.settings.separator()))?;
            }
            Format::Json => {
                let fields: Vec<String> = self
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(at, column)| format!("{}:{}", json_string(&column.name), json_value(cell(at))))
                    .collect();
                let object = format!("{{{}}}", fields.join(","));
                match self.settings.json {
                    JsonStyle::Array => {
                        let comma = if self.rows == 0 { "" } else { "," };
                        write!(self.out, "{comma}{eol}  {object}")?;
                    }
                    JsonStyle::Lines => write!(self.out, "{object}{eol}")?,
                }
            }
            Format::Sql => {
                let dialect = self.target.dialect;
                let values: Vec<String> = (0..width).map(|at| dialect.literal(cell(at))).collect();
                if self.in_statement == 0 {
                    let table = self.settings.table.clone().filter(|table| !table.trim().is_empty());
                    let table = table.unwrap_or_else(|| self.target.table.clone());
                    let names: Vec<String> = self.columns.iter().map(|column| dialect.quote(&column.name)).collect();
                    write!(self.out, "INSERT INTO {table} ({}) VALUES{eol}", names.join(", "))?;
                } else {
                    write!(self.out, ",{eol}")?;
                }
                write!(self.out, "\t({})", values.join(", "))?;
                self.in_statement += 1;
                if self.in_statement >= self.settings.batch {
                    write!(self.out, ";{eol}")?;
                    self.in_statement = 0;
                }
            }
            Format::Markdown => {
                let fields: Vec<String> = (0..width)
                    .map(|at| markdown_cell(&cell_text(cell(at)).unwrap_or_else(|| self.settings.null_text.clone())))
                    .collect();
                write!(self.out, "| {} |{eol}", fields.join(" | "))?;
            }
            Format::Html => {
                write!(self.out, "<tr>")?;
                for at in 0..width {
                    let text = cell_text(cell(at)).unwrap_or_else(|| self.settings.null_text.clone());
                    write!(self.out, "<td>{}</td>", html_escape(&text))?;
                }
                write!(self.out, "</tr>{eol}")?;
            }
            Format::Xml => {
                write!(self.out, "  <row>{eol}")?;
                for (at, column) in self.columns.iter().enumerate() {
                    let name = xml_escape(&column.name);
                    match cell_text(cell(at)) {
                        None => write!(self.out, "    <value column=\"{name}\" null=\"true\"/>{eol}")?,
                        Some(text) => {
                            write!(self.out, "    <value column=\"{name}\">{}</value>{eol}", xml_escape(&text))?
                        }
                    }
                }
                write!(self.out, "  </row>{eol}")?;
            }
            Format::Parquet => {}
        }
        self.rows += 1;
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<W> {
        let eol = self.eol;
        match self.settings.format {
            Format::Json if self.settings.json == JsonStyle::Array => {
                if self.rows > 0 {
                    write!(self.out, "{eol}")?;
                }
                write!(self.out, "]{eol}")?;
            }
            Format::Sql if self.in_statement > 0 => write!(self.out, ";{eol}")?,
            Format::Html => write!(self.out, "</tbody>{eol}</table>{eol}</body>{eol}</html>{eol}")?,
            Format::Xml => write!(self.out, "</export>{eol}")?,
            _ => {}
        }
        self.out.flush()?;
        Ok(self.out)
    }
}

pub fn render(settings: &Settings, columns: Vec<Column>, rows: &[Vec<Value>], target: Target) -> String {
    let written = Writer::new(Vec::new(), settings, columns, target).and_then(|mut writer| {
        for row in rows {
            writer.row(row)?;
        }
        writer.finish()
    });
    String::from_utf8(written.unwrap_or_default()).unwrap_or_default()
}

pub const PATTERN: &str = "${table}_${timestamp}";

pub fn file_name(pattern: &str, values: &[(&str, &str)], extension: &str) -> String {
    let mut name = if pattern.trim().is_empty() { PATTERN.to_owned() } else { pattern.trim().to_owned() };
    for (key, value) in values {
        name = name.replace(&format!("${{{key}}}"), value);
    }
    let mut clean: String =
        name.chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect();
    clean = clean.trim_start_matches('.').trim().to_owned();
    if clean.is_empty() {
        clean = "export".to_owned();
    }
    let suffix = format!(".{extension}");
    if clean.to_ascii_lowercase().ends_with(&suffix) { clean } else { format!("{clean}{suffix}") }
}

pub fn text_type(type_name: &str) -> bool {
    matches!(
        type_name.to_ascii_lowercase().as_str(),
        "text"
            | "varchar"
            | "bpchar"
            | "char"
            | "name"
            | "citext"
            | "xml"
            | "unknown"
            | "character varying"
            | "character"
    )
}

pub struct CopyRewriter {
    delimiter: u8,
    quote: u8,
    crlf: bool,
    escape: Vec<bool>,
    header: bool,
    column: usize,
    field_start: bool,
    in_quotes: bool,
    after_quote: bool,
    first_inside: bool,
    records: u64,
}

impl CopyRewriter {
    pub fn new(options: &CopyOptions, escape: Vec<bool>, crlf: bool) -> Self {
        Self {
            delimiter: u8::try_from(u32::from(options.delimiter)).unwrap_or(b','),
            quote: u8::try_from(u32::from(options.quote)).unwrap_or(b'"'),
            crlf,
            escape,
            header: options.header,
            column: 0,
            field_start: true,
            in_quotes: false,
            after_quote: false,
            first_inside: false,
            records: 0,
        }
    }

    pub fn records(&self) -> u64 {
        self.records
    }

    fn escapes(&self, byte: u8) -> bool {
        !self.header
            && self.escape.get(self.column).copied().unwrap_or(false)
            && matches!(byte, b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r')
    }

    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        out.reserve(input.len() + input.len() / 16);
        for &byte in input {
            if self.in_quotes {
                if self.after_quote {
                    self.after_quote = false;
                    if byte == self.quote {
                        out.push(byte);
                        continue;
                    }
                    self.in_quotes = false;
                } else {
                    if std::mem::take(&mut self.first_inside) && byte != self.quote && self.escapes(byte) {
                        out.push(b'\'');
                    }
                    if byte == self.quote {
                        self.after_quote = true;
                    }
                    out.push(byte);
                    continue;
                }
            }
            if self.field_start {
                self.field_start = false;
                if byte == self.quote {
                    self.in_quotes = true;
                    self.first_inside = true;
                    out.push(byte);
                    continue;
                }
                if self.escapes(byte) && byte != b'\r' {
                    out.push(b'\'');
                }
            }
            if byte == self.delimiter {
                self.column += 1;
                self.field_start = true;
                out.push(byte);
            } else if byte == b'\n' {
                if self.crlf {
                    out.push(b'\r');
                }
                out.push(byte);
                self.column = 0;
                self.field_start = true;
                if !std::mem::replace(&mut self.header, false) {
                    self.records += 1;
                }
            } else {
                out.push(byte);
            }
        }
    }
}

#[cfg(test)]
mod tests;
