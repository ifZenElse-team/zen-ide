use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::csv_import::Cell;
use crate::dialect::SqlDialect;
use crate::value::Value;

/// A column type as every SQL dialect has it, read from a type name of any driver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SqlType {
    /// `true` or `false`.
    Boolean,
    /// A 16-bit integer.
    SmallInt,
    /// A 32-bit integer.
    Integer,
    /// A 64-bit integer.
    BigInt,
    /// A 32-bit floating-point number.
    Real,
    /// A 64-bit floating-point number.
    Double,
    /// An exact decimal number.
    Decimal {
        /// Its digits in all, when the type says.
        precision: Option<u32>,
        /// Its digits after the point, when the type says.
        scale: Option<u32>,
    },
    /// Text of a fixed length.
    Char {
        /// The length, when the type says.
        length: Option<u32>,
    },
    /// Text of at most a length.
    Varchar {
        /// The most characters, when the type says.
        length: Option<u32>,
    },
    /// Text of any length.
    Text,
    /// Binary data.
    Bytes,
    /// A date.
    Date,
    /// A time of day.
    Time,
    /// A date and time without a time zone.
    Timestamp,
    /// A moment in time, with a time zone.
    TimestampTz,
    /// A span of time.
    Interval,
    /// A UUID.
    Uuid,
    /// JSON.
    Json,
    /// A type no other dialect has (arrays, enums, geometry), carried as text.
    #[default]
    Other,
}

fn number(text: &str) -> Option<u32> {
    text.trim().parse().ok()
}

fn unwrap_type(name: &str) -> &str {
    let mut name = name.trim();
    loop {
        let lower = name.to_ascii_lowercase();
        let inner = ["nullable(", "lowcardinality("]
            .iter()
            .find(|wrapper| lower.starts_with(**wrapper) && name.ends_with(')'))
            .map(|wrapper| &name[wrapper.len()..name.len() - 1]);
        match inner {
            Some(inner) => name = inner.trim(),
            None => return name,
        }
    }
}

fn clickhouse_type(base: &str, args: &[&str]) -> Option<SqlType> {
    Some(match base {
        "Bool" => SqlType::Boolean,
        "Int8" | "UInt8" | "Int16" => SqlType::SmallInt,
        "UInt16" | "Int32" => SqlType::Integer,
        "UInt32" | "Int64" => SqlType::BigInt,
        "UInt64" => SqlType::Decimal { precision: Some(20), scale: Some(0) },
        "Int128" | "UInt128" => SqlType::Decimal { precision: Some(39), scale: Some(0) },
        "Int256" | "UInt256" => SqlType::Decimal { precision: Some(77), scale: Some(0) },
        "Float32" => SqlType::Real,
        "Float64" => SqlType::Double,
        "String" => SqlType::Text,
        "FixedString" => SqlType::Char { length: args.first().and_then(|arg| number(arg)) },
        "Date" | "Date32" => SqlType::Date,
        "DateTime" | "DateTime64" if args.iter().any(|arg| arg.contains('\'')) => SqlType::TimestampTz,
        "DateTime" | "DateTime64" => SqlType::Timestamp,
        "UUID" => SqlType::Uuid,
        "JSON" => SqlType::Json,
        "Decimal32" => SqlType::Decimal { precision: Some(9), scale: args.first().and_then(|arg| number(arg)) },
        "Decimal64" => SqlType::Decimal { precision: Some(18), scale: args.first().and_then(|arg| number(arg)) },
        "Decimal128" => SqlType::Decimal { precision: Some(38), scale: args.first().and_then(|arg| number(arg)) },
        "Decimal256" => SqlType::Decimal { precision: Some(76), scale: args.first().and_then(|arg| number(arg)) },
        "Enum8" | "Enum16" => SqlType::Text,
        _ => return None,
    })
}

impl SqlType {
    pub fn parse(type_name: &str) -> SqlType {
        let name = unwrap_type(type_name);
        let (base, rest) = name.split_once('(').unwrap_or((name, ""));
        let (args, suffix) = rest.rsplit_once(')').unwrap_or((rest, ""));
        let args: Vec<&str> =
            if args.trim().is_empty() { Vec::new() } else { args.split(',').map(str::trim).collect() };
        if let Some(found) = clickhouse_type(base.trim(), &args) {
            return found;
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with("[]") || lower.starts_with('_') || lower.starts_with("array(") {
            return SqlType::Other;
        }
        let words = format!("{} {}", base.trim().to_ascii_lowercase(), suffix.trim().to_ascii_lowercase());
        let unsigned = words.contains("unsigned");
        let zoned = words.contains("with time zone") && !words.contains("without time zone");
        let base = base.trim().to_ascii_lowercase();
        let base = base
            .trim_end_matches(" without time zone")
            .trim_end_matches(" with time zone")
            .trim_end_matches(" unsigned")
            .trim_end_matches(" zerofill")
            .trim_end_matches(" signed")
            .trim();
        let length = args.first().and_then(|arg| number(arg));
        match base {
            "bool" | "boolean" => SqlType::Boolean,
            "bit" if length.is_none_or(|bits| bits == 1) => SqlType::Boolean,
            "tinyint" if length == Some(1) && !unsigned => SqlType::Boolean,
            "tinyint" => SqlType::SmallInt,
            "smallint" | "int2" | "smallserial" | "serial2" if unsigned => SqlType::Integer,
            "smallint" | "int2" | "smallserial" | "serial2" | "year" => SqlType::SmallInt,
            "mediumint" => SqlType::Integer,
            "integer" | "int" | "int4" | "serial" | "serial4" if unsigned => SqlType::BigInt,
            "integer" | "int" | "int4" | "serial" | "serial4" => SqlType::Integer,
            "bigint" | "int8" | "bigserial" | "serial8" if unsigned => {
                SqlType::Decimal { precision: Some(20), scale: Some(0) }
            }
            "bigint" | "int8" | "bigserial" | "serial8" => SqlType::BigInt,
            "real" | "float4" => SqlType::Real,
            "float" if length.is_some_and(|bits| bits <= 24) && args.len() == 1 => SqlType::Real,
            "double" | "double precision" | "float8" | "float" => SqlType::Double,
            "numeric" | "decimal" | "dec" | "fixed" | "number" => SqlType::Decimal {
                precision: length,
                scale: args.get(1).and_then(|arg| number(arg)).or(length.map(|_| 0)),
            },
            "char" | "character" | "bpchar" | "nchar" => SqlType::Char { length },
            "varchar" | "character varying" | "nvarchar" | "varchar2" => SqlType::Varchar { length },
            "text" | "tinytext" | "mediumtext" | "longtext" | "citext" | "name" | "clob" | "string" | "enum"
            | "set" => SqlType::Text,
            "bytea" | "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" => SqlType::Bytes,
            "date" => SqlType::Date,
            "time" | "timetz" => SqlType::Time,
            "timestamp" if zoned => SqlType::TimestampTz,
            "timestamptz" | "datetimeoffset" => SqlType::TimestampTz,
            "timestamp" | "datetime" | "datetime2" | "smalldatetime" => SqlType::Timestamp,
            "interval" => SqlType::Interval,
            "uuid" | "uniqueidentifier" => SqlType::Uuid,
            "json" | "jsonb" => SqlType::Json,
            other if other.contains("int") => SqlType::BigInt,
            other if other.contains("char") || other.contains("clob") || other.contains("text") => SqlType::Text,
            other if other.contains("blob") => SqlType::Bytes,
            other if other.contains("real") || other.contains("floa") || other.contains("doub") => SqlType::Double,
            _ => SqlType::Other,
        }
    }

    pub fn sql(self, dialect: SqlDialect, nullable: bool) -> String {
        match dialect {
            SqlDialect::Postgres => self.postgres(),
            SqlDialect::MySql | SqlDialect::MariaDb => self.mysql(),
            SqlDialect::Sqlite => self.sqlite().to_owned(),
            SqlDialect::ClickHouse if nullable => format!("Nullable({})", self.clickhouse()),
            SqlDialect::ClickHouse => self.clickhouse(),
            SqlDialect::SqlServer => self.sqlserver(),
            SqlDialect::Oracle => self.oracle(),
        }
    }

    fn oracle(self) -> String {
        match self {
            SqlType::Boolean => "NUMBER(1)".to_owned(),
            SqlType::SmallInt => "NUMBER(5)".to_owned(),
            SqlType::Integer => "NUMBER(10)".to_owned(),
            SqlType::BigInt => "NUMBER(19)".to_owned(),
            SqlType::Real => "BINARY_FLOAT".to_owned(),
            SqlType::Double => "BINARY_DOUBLE".to_owned(),
            SqlType::Decimal { precision: Some(precision), scale } if precision <= 38 => {
                format!("NUMBER({precision},{})", scale.unwrap_or(0).min(precision))
            }
            SqlType::Decimal { .. } => "NUMBER".to_owned(),
            SqlType::Char { length: Some(length) } if length <= 2000 => format!("CHAR({length} CHAR)"),
            SqlType::Varchar { length: Some(length) } if length <= 4000 => format!("VARCHAR2({length} CHAR)"),
            SqlType::Char { .. } | SqlType::Varchar { .. } | SqlType::Text | SqlType::Other | SqlType::Json => {
                "CLOB".to_owned()
            }
            SqlType::Bytes => "BLOB".to_owned(),
            SqlType::Date | SqlType::Timestamp => "TIMESTAMP".to_owned(),
            SqlType::TimestampTz => "TIMESTAMP WITH TIME ZONE".to_owned(),
            SqlType::Time | SqlType::Interval => "VARCHAR2(64 CHAR)".to_owned(),
            SqlType::Uuid => "VARCHAR2(36 CHAR)".to_owned(),
        }
    }

    fn sqlserver(self) -> String {
        match self {
            SqlType::Boolean => "BIT".to_owned(),
            SqlType::SmallInt => "SMALLINT".to_owned(),
            SqlType::Integer => "INT".to_owned(),
            SqlType::BigInt => "BIGINT".to_owned(),
            SqlType::Real => "REAL".to_owned(),
            SqlType::Double => "FLOAT".to_owned(),
            SqlType::Decimal { precision: Some(precision), scale } if precision <= 38 => {
                format!("DECIMAL({precision},{})", scale.unwrap_or(0).min(precision))
            }
            SqlType::Decimal { .. } => "DECIMAL(38,10)".to_owned(),
            SqlType::Char { length: Some(length) } if length <= 4000 => format!("NCHAR({length})"),
            SqlType::Varchar { length: Some(length) } if length <= 4000 => format!("NVARCHAR({length})"),
            SqlType::Char { .. }
            | SqlType::Varchar { .. }
            | SqlType::Text
            | SqlType::Other
            | SqlType::Interval
            | SqlType::Json => "NVARCHAR(MAX)".to_owned(),
            SqlType::Bytes => "VARBINARY(MAX)".to_owned(),
            SqlType::Date => "DATE".to_owned(),
            SqlType::Time => "TIME(7)".to_owned(),
            SqlType::Timestamp => "DATETIME2(7)".to_owned(),
            SqlType::TimestampTz => "DATETIMEOFFSET(7)".to_owned(),
            SqlType::Uuid => "UNIQUEIDENTIFIER".to_owned(),
        }
    }

    fn postgres(self) -> String {
        match self {
            SqlType::Boolean => "boolean".to_owned(),
            SqlType::SmallInt => "smallint".to_owned(),
            SqlType::Integer => "integer".to_owned(),
            SqlType::BigInt => "bigint".to_owned(),
            SqlType::Real => "real".to_owned(),
            SqlType::Double => "double precision".to_owned(),
            SqlType::Decimal { precision: Some(precision), scale } if precision <= 1000 => {
                format!("numeric({precision},{})", scale.unwrap_or(0).min(precision))
            }
            SqlType::Decimal { .. } => "numeric".to_owned(),
            SqlType::Char { length: Some(length) } => format!("character({length})"),
            SqlType::Varchar { length: Some(length) } => format!("varchar({length})"),
            SqlType::Char { length: None } | SqlType::Varchar { length: None } | SqlType::Text | SqlType::Other => {
                "text".to_owned()
            }
            SqlType::Bytes => "bytea".to_owned(),
            SqlType::Date => "date".to_owned(),
            SqlType::Time => "time".to_owned(),
            SqlType::Timestamp => "timestamp".to_owned(),
            SqlType::TimestampTz => "timestamptz".to_owned(),
            SqlType::Interval => "interval".to_owned(),
            SqlType::Uuid => "uuid".to_owned(),
            SqlType::Json => "jsonb".to_owned(),
        }
    }

    fn mysql(self) -> String {
        match self {
            SqlType::Boolean => "BOOLEAN".to_owned(),
            SqlType::SmallInt => "SMALLINT".to_owned(),
            SqlType::Integer => "INT".to_owned(),
            SqlType::BigInt => "BIGINT".to_owned(),
            SqlType::Real => "FLOAT".to_owned(),
            SqlType::Double => "DOUBLE".to_owned(),
            SqlType::Decimal { precision: Some(precision), scale } if precision <= 65 => {
                format!("DECIMAL({precision},{})", scale.unwrap_or(0).min(30).min(precision))
            }
            SqlType::Decimal { .. } => "DECIMAL(65,30)".to_owned(),
            SqlType::Char { length: Some(length) } if length <= 255 => format!("CHAR({length})"),
            SqlType::Varchar { length: Some(length) } if length <= 2000 => format!("VARCHAR({length})"),
            SqlType::Char { .. } | SqlType::Varchar { .. } | SqlType::Text | SqlType::Other => "LONGTEXT".to_owned(),
            SqlType::Bytes => "LONGBLOB".to_owned(),
            SqlType::Date => "DATE".to_owned(),
            SqlType::Time => "TIME(6)".to_owned(),
            SqlType::Timestamp | SqlType::TimestampTz => "DATETIME(6)".to_owned(),
            SqlType::Interval => "VARCHAR(255)".to_owned(),
            SqlType::Uuid => "CHAR(36)".to_owned(),
            SqlType::Json => "JSON".to_owned(),
        }
    }

    fn sqlite(self) -> &'static str {
        match self {
            SqlType::Boolean => "BOOLEAN",
            SqlType::SmallInt | SqlType::Integer | SqlType::BigInt => "INTEGER",
            SqlType::Real | SqlType::Double => "REAL",
            SqlType::Decimal { .. } => "NUMERIC",
            SqlType::Bytes => "BLOB",
            SqlType::Date => "DATE",
            SqlType::Time => "TIME",
            SqlType::Timestamp | SqlType::TimestampTz => "TIMESTAMP",
            SqlType::Char { .. }
            | SqlType::Varchar { .. }
            | SqlType::Text
            | SqlType::Interval
            | SqlType::Uuid
            | SqlType::Json
            | SqlType::Other => "TEXT",
        }
    }

    fn clickhouse(self) -> String {
        match self {
            SqlType::Boolean => "Bool".to_owned(),
            SqlType::SmallInt => "Int16".to_owned(),
            SqlType::Integer => "Int32".to_owned(),
            SqlType::BigInt => "Int64".to_owned(),
            SqlType::Real => "Float32".to_owned(),
            SqlType::Double => "Float64".to_owned(),
            SqlType::Decimal { precision: Some(precision), scale } if (1..=76).contains(&precision) => {
                format!("Decimal({precision}, {})", scale.unwrap_or(0).min(precision))
            }
            SqlType::Decimal { .. } => "Decimal(38, 10)".to_owned(),
            SqlType::Date => "Date32".to_owned(),
            SqlType::Timestamp => "DateTime64(6)".to_owned(),
            SqlType::TimestampTz => "DateTime64(6, 'UTC')".to_owned(),
            SqlType::Uuid => "UUID".to_owned(),
            SqlType::Char { .. }
            | SqlType::Varchar { .. }
            | SqlType::Text
            | SqlType::Bytes
            | SqlType::Time
            | SqlType::Interval
            | SqlType::Json
            | SqlType::Other => "String".to_owned(),
        }
    }

    fn numeric(self) -> bool {
        matches!(
            self,
            SqlType::SmallInt
                | SqlType::Integer
                | SqlType::BigInt
                | SqlType::Real
                | SqlType::Double
                | SqlType::Decimal { .. }
        )
    }

    fn textual(self) -> bool {
        matches!(self, SqlType::Char { .. } | SqlType::Varchar { .. } | SqlType::Text | SqlType::Other)
    }
}

pub fn same_types(from: SqlDialect, to: SqlDialect) -> bool {
    from == to
        || matches!((from, to), (SqlDialect::MySql, SqlDialect::MariaDb) | (SqlDialect::MariaDb, SqlDialect::MySql))
}

pub fn target_type(type_name: &str, from: SqlDialect, to: SqlDialect, nullable: bool) -> String {
    if same_types(from, to) && !type_name.trim().is_empty() {
        return type_name.trim().to_owned();
    }
    SqlType::parse(type_name).sql(to, nullable)
}

fn hex(bytes: &[u8]) -> String {
    crate::value::hex(bytes)
}

pub fn instant(text: &str) -> Option<jiff::Timestamp> {
    let trimmed = text.trim().replacen(' ', "T", 1);
    let offset = trimmed.rfind(['+', '-']).filter(|at| *at > 10).map(|at| &trimmed[at..]);
    let padded = match offset {
        Some(offset) if offset.len() == 3 => format!("{trimmed}:00"),
        _ => trimmed,
    };
    padded.parse::<jiff::Timestamp>().ok()
}

fn utc_text(text: &str) -> Option<String> {
    Some(instant(text)?.to_zoned(jiff::tz::TimeZone::UTC).strftime("%Y-%m-%d %H:%M:%S%.f").to_string())
}

fn boolean_text(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "on" | "1" => Some(true),
        "false" | "f" | "no" | "n" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn keyword(value: bool) -> Cell {
    Cell::Raw(if value { "TRUE" } else { "FALSE" }.to_owned())
}

pub fn cell(value: &Value, target: SqlType, dialect: SqlDialect) -> Result<Cell, &'static str> {
    let text_target = target.textual();
    Ok(match value {
        Value::Null => Cell::Null,
        Value::Bool(value) if text_target => Cell::Text(value.to_string()),
        Value::Bool(value) if target.numeric() => Cell::Raw(if *value { "1" } else { "0" }.to_owned()),
        Value::Bool(value) => keyword(*value),
        Value::Integer(value) if target == SqlType::Boolean => match value {
            0 => keyword(false),
            1 => keyword(true),
            _ => return Err("notABoolean"),
        },
        Value::Integer(value) if text_target => Cell::Text(value.to_string()),
        Value::Integer(value) => Cell::Raw(value.to_string()),
        Value::Float(value) if value.is_finite() && text_target => Cell::Text(value.to_string()),
        Value::Float(value) if value.is_finite() => Cell::Raw(dialect.literal(&Value::Float(*value))),
        Value::Float(_) if matches!(dialect, SqlDialect::Postgres | SqlDialect::ClickHouse) || text_target => {
            Cell::Text(value.display().unwrap_or_default())
        }
        Value::Float(_) => return Err("notANumber"),
        Value::Decimal(text) if text_target => Cell::Text(text.clone()),
        Value::Decimal(text) if target == SqlType::Boolean => match boolean_text(text) {
            Some(value) => keyword(value),
            None => return Err("notABoolean"),
        },
        Value::Decimal(text) if text.parse::<f64>().is_ok_and(f64::is_finite) => Cell::Raw(text.clone()),
        Value::Decimal(text) if dialect == SqlDialect::Postgres => Cell::Text(text.clone()),
        Value::Decimal(_) => return Err("notANumber"),
        Value::Text(text) if target == SqlType::Boolean => match boolean_text(text) {
            Some(value) => keyword(value),
            None => return Err("notABoolean"),
        },
        Value::Text(text) if target.numeric() && !text.trim().is_empty() => {
            if text.trim().parse::<f64>().is_ok_and(f64::is_finite) {
                Cell::Raw(text.trim().to_owned())
            } else {
                return Err("notANumber");
            }
        }
        Value::Bytes(bytes) if target == SqlType::Bytes => match dialect {
            SqlDialect::Postgres => Cell::Text(format!("\\x{}", hex(bytes))),
            _ => Cell::Raw(dialect.literal(value)),
        },
        Value::Bytes(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => Cell::Text(text.to_owned()),
            Err(_) => Cell::Text(format!("\\x{}", hex(bytes))),
        },
        Value::Temporal(typed) if matches!(target, SqlType::Timestamp | SqlType::TimestampTz | SqlType::Date) => {
            match dialect {
                SqlDialect::Postgres => Cell::Text(typed.text.clone()),
                _ if target == SqlType::Date => Cell::Text(typed.text.trim().chars().take(10).collect()),
                _ => match utc_text(&typed.text) {
                    Some(text) => Cell::Text(text),
                    None if typed.text.trim().parse::<jiff::civil::DateTime>().is_ok()
                        || typed.text.trim().parse::<jiff::civil::Date>().is_ok() =>
                    {
                        Cell::Text(typed.text.trim().to_owned())
                    }
                    None => return Err("notADate"),
                },
            }
        }
        Value::Array(items) if dialect == SqlDialect::Postgres && target == SqlType::Other => {
            Cell::Text(crate::present::array_literal(items))
        }
        Value::Array(_) | Value::Map(_) | Value::Document(_) => Cell::Text(crate::export::json_value(value)),
        Value::Text(text) | Value::Interval(text) | Value::Uuid(text) | Value::Json(text) => Cell::Text(text.clone()),
        Value::Temporal(typed) | Value::Other(typed) => Cell::Text(typed.text.clone()),
    })
}

pub fn select_statement(dialect: SqlDialect, table: &str, columns: &[String]) -> String {
    let names: Vec<String> = columns.iter().map(|name| dialect.quote(name)).collect();
    format!("SELECT {} FROM {table}", names.join(", "))
}

/// A column of a table a transfer creates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewColumn {
    /// Its name.
    pub name: String,
    /// Its type as the target writes it.
    #[serde(rename = "type")]
    pub sql_type: String,
    /// Whether it takes `NULL`s.
    pub nullable: bool,
}

pub fn create_table(dialect: SqlDialect, table: &str, columns: &[NewColumn], key: &[String]) -> String {
    let mut lines: Vec<String> = columns
        .iter()
        .map(|column| {
            let not_null = !column.nullable && dialect != SqlDialect::ClickHouse;
            format!(
                "    {} {}{}",
                dialect.quote(&column.name),
                column.sql_type,
                if not_null { " NOT NULL" } else { "" }
            )
        })
        .collect();
    let key: Vec<String> = key.iter().map(|name| dialect.quote(name)).collect();
    if dialect == SqlDialect::ClickHouse {
        let order = if key.is_empty() { "tuple()".to_owned() } else { format!("({})", key.join(", ")) };
        return format!("CREATE TABLE {table} (\n{}\n)\nENGINE = MergeTree ORDER BY {order}", lines.join(",\n"));
    }
    if !key.is_empty() {
        lines.push(format!("    PRIMARY KEY ({})", key.join(", ")));
    }
    format!("CREATE TABLE {table} (\n{}\n)", lines.join(",\n"))
}

#[cfg(test)]
mod tests;
