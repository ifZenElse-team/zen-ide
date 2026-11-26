use std::io::{self, BufRead};
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::SqlDialect;

pub const SAMPLE_LIMIT: usize = 100;

/// How a CSV file is written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CsvFormat {
    /// What separates fields: one character, `\t` for a tab.
    pub delimiter: char,
    /// The character that quotes fields.
    pub quote: char,
    /// The character that escapes the next one inside quotes; absent when a quote is written doubled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escape: Option<char>,
    /// The first record holds the column names.
    pub header: bool,
    /// What an unquoted field holding `NULL` is written as; empty by default.
    pub null_text: String,
    /// How dates and timestamps are written, in `strftime` style (`%d.%m.%Y %H:%M`) or Java style (`dd.MM.yyyy HH:mm`); ISO 8601 when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_format: Option<String>,
}

impl Default for CsvFormat {
    fn default() -> Self {
        Self {
            delimiter: ',',
            quote: '"',
            escape: None,
            header: true,
            null_text: String::new(),
            timestamp_format: None,
        }
    }
}

impl CsvFormat {
    pub fn problem(&self) -> Option<&'static str> {
        let breaks = |c: char| matches!(c, '\n' | '\r');
        if breaks(self.delimiter) || breaks(self.quote) || self.escape.is_some_and(breaks) {
            return Some("lineBreak");
        }
        if self.delimiter == self.quote || self.escape == Some(self.delimiter) {
            return Some("delimiterQuote");
        }
        None
    }

    pub fn pattern(&self) -> Option<String> {
        let pattern = self.timestamp_format.as_deref().map(str::trim).filter(|pattern| !pattern.is_empty())?;
        Some(if pattern.contains('%') { pattern.to_owned() } else { java_pattern(pattern) })
    }
}

pub fn java_pattern(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c == '\'' {
            at += 1;
            while at < chars.len() && chars[at] != '\'' {
                out.push(chars[at]);
                at += 1;
            }
            at += 1;
            continue;
        }
        let run = chars[at..].iter().take_while(|next| **next == c).count();
        let piece = match (c, run) {
            ('y' | 'u', 2) => "%y",
            ('y' | 'u', _) => "%Y",
            ('M', 1 | 2) => "%m",
            ('M', 3) => "%b",
            ('M', _) => "%B",
            ('d', _) => "%d",
            ('H', _) => "%H",
            ('h', _) => "%I",
            ('m', _) => "%M",
            ('s', _) => "%S",
            ('S', _) => "%f",
            ('a', _) => "%p",
            ('X' | 'x' | 'Z', _) => "%:z",
            ('E', 1..=3) => "%a",
            ('E', _) => "%A",
            _ => {
                out.extend(std::iter::repeat_n(c, run));
                at += run;
                continue;
            }
        };
        if c == 'S' && out.ends_with('.') {
            out.pop();
            out.push_str("%.f");
        } else {
            out.push_str(piece);
        }
        at += run;
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub text: String,
    pub quoted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub fields: Vec<Field>,
    pub line: u64,
}

impl Record {
    pub fn value<'a>(&'a self, at: usize, format: &CsvFormat) -> Option<&'a str> {
        let field = self.fields.get(at)?;
        (field.quoted || field.text != format.null_text).then_some(field.text.as_str())
    }
}

pub struct CsvReader<R: BufRead> {
    input: R,
    format: CsvFormat,
    line: u64,
    bytes: u64,
    started: bool,
    buffer: Vec<u8>,
}

impl<R: BufRead> CsvReader<R> {
    pub fn new(input: R, format: &CsvFormat) -> Self {
        Self { input, format: format.clone(), line: 0, bytes: 0, started: false, buffer: Vec::new() }
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    fn read_line(&mut self) -> io::Result<Option<String>> {
        self.buffer.clear();
        let read = self.input.read_until(b'\n', &mut self.buffer)?;
        if read == 0 {
            return Ok(None);
        }
        self.bytes += read as u64;
        self.line += 1;
        let mut bytes = self.buffer.as_slice();
        if !std::mem::replace(&mut self.started, true) && bytes.starts_with(b"\xef\xbb\xbf") {
            bytes = &bytes[3..];
        }
        Ok(Some(String::from_utf8_lossy(bytes).into_owned()))
    }

    pub fn next_record(&mut self) -> io::Result<Option<Record>> {
        loop {
            let Some(text) = self.read_line()? else { return Ok(None) };
            if text.trim_end_matches(['\r', '\n']).is_empty() {
                continue;
            }
            let line = self.line;
            return self.parse(text, line).map(Some);
        }
    }

    fn parse(&mut self, first: String, line: u64) -> io::Result<Record> {
        let (delimiter, quote) = (self.format.delimiter, self.format.quote);
        let escape = self.format.escape.filter(|escape| *escape != quote);
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut in_quotes = false;
        let mut at_start = true;
        let mut text = first;
        loop {
            let mut chars = text.chars().peekable();
            while let Some(c) = chars.next() {
                if in_quotes {
                    if Some(c) == escape {
                        if let Some(next) = chars.next() {
                            field.push(next);
                        }
                    } else if c == quote {
                        if escape.is_none() && chars.peek() == Some(&quote) {
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
                if c == delimiter {
                    fields.push(Field { text: std::mem::take(&mut field), quoted });
                    quoted = false;
                    at_start = true;
                    continue;
                }
                if matches!(c, '\n' | '\r') && chars.clone().all(|rest| matches!(rest, '\n' | '\r')) {
                    break;
                }
                if at_start && c == quote {
                    in_quotes = true;
                    quoted = true;
                    at_start = false;
                    continue;
                }
                at_start = false;
                field.push(c);
            }
            if !in_quotes {
                break;
            }
            match self.read_line()? {
                Some(next) => text = next,
                None => break,
            }
        }
        fields.push(Field { text: field, quoted });
        Ok(Record { fields, line })
    }
}

/// What a column holds, as an import converts its fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Text, sent as it is.
    #[default]
    Text,
    /// Whole numbers.
    Integer,
    /// Numbers with a fraction.
    Decimal,
    /// `true` or `false` (also `t`, `f`, `yes`, `no`, `on`, `off`, `1`, `0`).
    Boolean,
    /// Dates.
    Date,
    /// Dates with a time.
    Timestamp,
}

impl Kind {
    pub fn of_type(type_name: &str) -> Kind {
        let lower = type_name.trim().to_ascii_lowercase();
        let base = lower.strip_prefix("nullable(").map_or(lower.as_str(), |inner| inner.trim_end_matches(')'));
        let base = base.split(['(', ' ']).next().unwrap_or_default();
        match base {
            "smallint" | "integer" | "int" | "int2" | "int4" | "int8" | "bigint" | "tinyint" | "mediumint"
            | "serial" | "bigserial" | "smallserial" | "year" => Kind::Integer,
            name if (name.starts_with("int") || name.starts_with("uint"))
                && name[name.find('t').unwrap_or(0) + 1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                Kind::Integer
            }
            "numeric" | "decimal" | "real" | "double" | "float" | "float4" | "float8" | "float32" | "float64"
            | "money" | "decimal32" | "decimal64" | "decimal128" | "decimal256" => Kind::Decimal,
            "bool" | "boolean" => Kind::Boolean,
            "date" | "date32" => Kind::Date,
            "timestamp" | "timestamptz" | "datetime" | "datetime64" | "smalldatetime" => Kind::Timestamp,
            _ => Kind::Text,
        }
    }

    pub fn sql_type(self, dialect: SqlDialect) -> &'static str {
        match (dialect, self) {
            (SqlDialect::Postgres, Kind::Text) => "text",
            (SqlDialect::Postgres, Kind::Integer) => "bigint",
            (SqlDialect::Postgres, Kind::Decimal) => "numeric",
            (SqlDialect::Postgres, Kind::Boolean) => "boolean",
            (SqlDialect::Postgres, Kind::Date) => "date",
            (SqlDialect::Postgres, Kind::Timestamp) => "timestamp",
            (SqlDialect::MySql | SqlDialect::MariaDb, Kind::Text) => "TEXT",
            (SqlDialect::MySql | SqlDialect::MariaDb, Kind::Integer) => "BIGINT",
            (SqlDialect::MySql | SqlDialect::MariaDb, Kind::Decimal) => "DOUBLE",
            (SqlDialect::MySql | SqlDialect::MariaDb, Kind::Boolean) => "BOOLEAN",
            (SqlDialect::MySql | SqlDialect::MariaDb, Kind::Date) => "DATE",
            (SqlDialect::MySql | SqlDialect::MariaDb, Kind::Timestamp) => "DATETIME(6)",
            (SqlDialect::Sqlite, Kind::Text) => "TEXT",
            (SqlDialect::Sqlite, Kind::Integer) => "INTEGER",
            (SqlDialect::Sqlite, Kind::Decimal) => "REAL",
            (SqlDialect::Sqlite, Kind::Boolean) => "BOOLEAN",
            (SqlDialect::Sqlite, Kind::Date) => "DATE",
            (SqlDialect::Sqlite, Kind::Timestamp) => "TIMESTAMP",
            (SqlDialect::ClickHouse, Kind::Text) => "Nullable(String)",
            (SqlDialect::ClickHouse, Kind::Integer) => "Nullable(Int64)",
            (SqlDialect::ClickHouse, Kind::Decimal) => "Nullable(Float64)",
            (SqlDialect::ClickHouse, Kind::Boolean) => "Nullable(Bool)",
            (SqlDialect::ClickHouse, Kind::Date) => "Nullable(Date32)",
            (SqlDialect::ClickHouse, Kind::Timestamp) => "Nullable(DateTime64(6))",
            (SqlDialect::SqlServer, Kind::Text) => "NVARCHAR(MAX)",
            (SqlDialect::SqlServer, Kind::Integer) => "BIGINT",
            (SqlDialect::SqlServer, Kind::Decimal) => "FLOAT",
            (SqlDialect::SqlServer, Kind::Boolean) => "BIT",
            (SqlDialect::SqlServer, Kind::Date) => "DATE",
            (SqlDialect::SqlServer, Kind::Timestamp) => "DATETIME2",
            (SqlDialect::Oracle, Kind::Text) => "VARCHAR2(4000 CHAR)",
            (SqlDialect::Oracle, Kind::Integer) => "NUMBER(19)",
            (SqlDialect::Oracle, Kind::Decimal) => "BINARY_DOUBLE",
            (SqlDialect::Oracle, Kind::Boolean) => "NUMBER(1)",
            (SqlDialect::Oracle, Kind::Date) => "DATE",
            (SqlDialect::Oracle, Kind::Timestamp) => "TIMESTAMP",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unconverted {
    pub code: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Null,
    Raw(String),
    Text(String),
}

fn integer(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

fn decimal(text: &str) -> bool {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    body.chars().any(|c| c.is_ascii_digit())
        && body.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        && text.parse::<f64>().is_ok_and(f64::is_finite)
}

fn boolean(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "on" | "1" => Some(true),
        "false" | "f" | "no" | "n" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn word_boolean(text: &str) -> bool {
    !integer(text) && boolean(text).is_some()
}

fn parsed_time(text: &str, pattern: &str, date_only: bool) -> Option<String> {
    let broken = jiff::fmt::strtime::parse(pattern, text).ok()?;
    if date_only {
        return broken.to_date().ok().map(|date| date.strftime("%Y-%m-%d").to_string());
    }
    let datetime = broken
        .to_datetime()
        .or_else(|_| broken.to_date().map(|date| date.to_datetime(jiff::civil::Time::midnight())))
        .ok()?;
    let mut text = datetime.strftime("%Y-%m-%d %H:%M:%S%.f").to_string();
    if let Some(offset) = broken.offset() {
        text.push_str(&offset.to_string());
    }
    Some(text)
}

fn iso_date(text: &str) -> bool {
    text.parse::<jiff::civil::Date>().is_ok() && text.len() <= 10
}

fn iso_timestamp(text: &str) -> bool {
    text.parse::<jiff::civil::DateTime>().is_ok() || text.parse::<jiff::Timestamp>().is_ok()
}

pub fn convert(kind: Kind, text: Option<&str>, pattern: Option<&str>) -> Result<Cell, Unconverted> {
    let Some(text) = text else { return Ok(Cell::Null) };
    if kind == Kind::Text {
        return Ok(Cell::Text(text.to_owned()));
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Cell::Null);
    }
    match kind {
        Kind::Text => Ok(Cell::Text(text.to_owned())),
        Kind::Integer if integer(trimmed) => Ok(Cell::Raw(trimmed.trim_start_matches('+').to_owned())),
        Kind::Integer => Err(Unconverted { code: "notANumber" }),
        Kind::Decimal if decimal(trimmed) => Ok(Cell::Raw(trimmed.trim_start_matches('+').to_owned())),
        Kind::Decimal => Err(Unconverted { code: "notANumber" }),
        Kind::Boolean => match boolean(trimmed) {
            Some(true) => Ok(Cell::Raw("TRUE".to_owned())),
            Some(false) => Ok(Cell::Raw("FALSE".to_owned())),
            None => Err(Unconverted { code: "notABoolean" }),
        },
        Kind::Date | Kind::Timestamp => match pattern {
            Some(pattern) => parsed_time(trimmed, pattern, kind == Kind::Date)
                .map(Cell::Text)
                .ok_or(Unconverted { code: "notADate" }),
            None => Ok(Cell::Text(trimmed.to_owned())),
        },
    }
}

pub fn guess<'a>(values: impl IntoIterator<Item = &'a str>, pattern: Option<&str>) -> Kind {
    let values: Vec<&str> = values.into_iter().map(str::trim).filter(|value| !value.is_empty()).collect();
    if values.is_empty() {
        return Kind::Text;
    }
    let all = |test: &dyn Fn(&str) -> bool| values.iter().all(|value| test(value));
    if all(&|value| integer(value) && value.trim_start_matches(['+', '-']).len() < 19) {
        return Kind::Integer;
    }
    if all(&decimal) {
        return Kind::Decimal;
    }
    if all(&word_boolean) {
        return Kind::Boolean;
    }
    match pattern {
        Some(pattern) => {
            let has_time = ["%H", "%I", "%M", "%S", "%T", "%R", "%p"].iter().any(|code| pattern.contains(code));
            if all(&|value| parsed_time(value, pattern, !has_time).is_some()) {
                return if has_time { Kind::Timestamp } else { Kind::Date };
            }
        }
        None => {
            if all(&iso_date) {
                return Kind::Date;
            }
            if all(&iso_timestamp) {
                return Kind::Timestamp;
            }
        }
    }
    Kind::Text
}

/// The first rows of a file, read with a format, and what its columns seem to hold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// The column names: the header's, else `column1`, `column2`…; empty and repeated names made unique.
    pub columns: Vec<String>,
    /// The kind each column seems to hold.
    pub kinds: Vec<Kind>,
    /// The rows read, each field `None` for `NULL`.
    pub rows: Vec<Vec<Option<String>>>,
    /// Whether the file has more rows than were read.
    pub more: bool,
    /// The file's size in bytes.
    pub file_bytes: u64,
}

pub fn column_names(header: &[String], width: usize) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(width);
    for at in 0..width {
        let base = header.get(at).map(|name| name.trim().to_owned()).filter(|name| !name.is_empty());
        let base = base.unwrap_or_else(|| format!("column{}", at + 1));
        let mut name = base.clone();
        let mut number = 2;
        while names.iter().any(|known| known.eq_ignore_ascii_case(&name)) {
            name = format!("{base}_{number}");
            number += 1;
        }
        names.push(name);
    }
    names
}

pub fn sample(path: &Path, format: &CsvFormat, rows: usize) -> io::Result<Sample> {
    let file = std::fs::File::open(path)?;
    let file_bytes = file.metadata()?.len();
    let mut reader = CsvReader::new(io::BufReader::with_capacity(64 * 1024, file), format);
    let header: Vec<String> = match format.header {
        true => reader.next_record()?.map(|record| record.fields.into_iter().map(|field| field.text).collect()),
        false => None,
    }
    .unwrap_or_default();
    let wanted = rows.clamp(1, SAMPLE_LIMIT);
    let mut records = Vec::new();
    let mut more = false;
    while let Some(record) = reader.next_record()? {
        if records.len() == wanted {
            more = true;
            break;
        }
        records.push(record);
    }
    let width = records.iter().map(|record| record.fields.len()).max().unwrap_or(0).max(header.len());
    let columns = column_names(&header, width);
    let pattern = format.pattern();
    let kinds = (0..width)
        .map(|at| guess(records.iter().filter_map(|record| record.value(at, format)), pattern.as_deref()))
        .collect();
    let rows = records
        .iter()
        .map(|record| (0..width).map(|at| record.value(at, format).map(str::to_owned)).collect())
        .collect();
    Ok(Sample { columns, kinds, rows, more, file_bytes })
}

/// What an import does with a row that fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OnError {
    /// Stop and roll back what is not committed.
    #[default]
    Stop,
    /// Leave the row out, note it and go on.
    Skip,
}

/// How rows are loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LoadSettings {
    /// Rows sent in one statement (one `INSERT` or one `COPY`), from 1 to 100 000.
    pub batch: usize,
    /// Commit after this many rows; 0 loads the whole file in one transaction.
    pub commit_every: u64,
    /// Empty the table first (`TRUNCATE TABLE`, `DELETE FROM` on SQLite), in the import's first transaction.
    pub truncate: bool,
    /// What to do with a row that fails.
    pub on_error: OnError,
    /// Send the rows with PostgreSQL's `COPY … FROM STDIN` where the driver can.
    pub server_copy: bool,
}

impl Default for LoadSettings {
    fn default() -> Self {
        Self { batch: 1000, commit_every: 0, truncate: false, on_error: OnError::Stop, server_copy: true }
    }
}

/// A column of the target table and the field of the file it takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMapping {
    /// The field's position in a record, from 0.
    pub source: usize,
    /// The target column's name.
    pub target: String,
    /// How the field is converted.
    #[serde(default)]
    pub kind: Kind,
    /// The column's type when the table is created; the kind's type in the connection's dialect when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql_type: Option<String>,
}

pub fn insert_statement(dialect: SqlDialect, table: &str, columns: &[String], rows: &[Vec<Cell>]) -> String {
    let names: Vec<String> = columns.iter().map(|name| dialect.quote(name)).collect();
    let mut sql = format!("INSERT INTO {table} ({}) VALUES", names.join(", "));
    for (at, row) in rows.iter().enumerate() {
        let values: Vec<String> = row
            .iter()
            .map(|cell| match cell {
                Cell::Null => "NULL".to_owned(),
                Cell::Raw(text) => text.clone(),
                Cell::Text(text) => dialect.string(text),
            })
            .collect();
        sql.push_str(if at == 0 { "\n(" } else { ",\n(" });
        sql.push_str(&values.join(", "));
        sql.push(')');
    }
    sql
}

pub fn insert_head(dialect: SqlDialect, table: &str, columns: &[String]) -> String {
    let names: Vec<String> = columns.iter().map(|name| dialect.quote(name)).collect();
    format!("INSERT INTO {table} ({}) VALUES (…)", names.join(", "))
}

pub fn copy_statement(table: &str, columns: &[String]) -> String {
    let names: Vec<String> = columns.iter().map(|name| SqlDialect::Postgres.quote(name)).collect();
    format!("COPY {table} ({}) FROM STDIN WITH (FORMAT csv, ENCODING 'UTF8')", names.join(", "))
}

pub fn copy_rows(rows: &[Vec<Cell>]) -> Vec<u8> {
    let mut out = String::new();
    for row in rows {
        for (at, cell) in row.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            match cell {
                Cell::Null => {}
                Cell::Raw(text) => out.push_str(text),
                Cell::Text(text) => {
                    out.push('"');
                    out.push_str(&text.replace('"', "\"\""));
                    out.push('"');
                }
            }
        }
        out.push('\n');
    }
    out.into_bytes()
}

pub fn plain_sql_type(text: &str) -> bool {
    let mut depth = 0usize;
    let text = text.trim();
    !text.is_empty()
        && text.len() <= 200
        && text.chars().all(|c| match c {
            '(' => {
                depth += 1;
                true
            }
            ')' => depth.checked_sub(1).map(|left| depth = left).is_some(),
            c => c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | ','),
        })
        && depth == 0
}

pub fn create_statement(dialect: SqlDialect, table: &str, columns: &[(String, String)]) -> String {
    let lines: Vec<String> =
        columns.iter().map(|(name, sql_type)| format!("    {} {}", dialect.quote(name), sql_type)).collect();
    let engine = if dialect == SqlDialect::ClickHouse { "\nENGINE = MergeTree ORDER BY tuple()" } else { "" };
    format!("CREATE TABLE {table} (\n{}\n){engine}", lines.join(",\n"))
}

pub fn truncate_statement(dialect: SqlDialect, table: &str) -> String {
    match dialect {
        SqlDialect::Sqlite => format!("DELETE FROM {table}"),
        _ => format!("TRUNCATE TABLE {table}"),
    }
}

#[cfg(test)]
mod tests;
