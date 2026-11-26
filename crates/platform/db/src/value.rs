use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A value read from any database: a cell of a table, a field of a document or the value of a key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Value {
    /// SQL `NULL`, a missing field, a key that does not exist.
    Null,
    /// A boolean.
    Bool(bool),
    /// An integer that fits in 64 bits.
    Integer(i64),
    /// An exact decimal number, kept as its text so that no digit is lost.
    Decimal(String),
    /// A floating-point number.
    Float(f64),
    /// Text.
    Text(String),
    /// Binary data, written as lowercase hexadecimal in answers.
    Bytes(
        #[serde(with = "hex_bytes")]
        #[schemars(with = "String")]
        Vec<u8>,
    ),
    /// A date, a time or a timestamp, as the server writes it.
    Temporal(Typed),
    /// An interval, as the server writes it.
    Interval(String),
    /// A UUID in its usual text form.
    Uuid(String),
    /// JSON, as the server sent it.
    Json(String),
    /// An array of values, possibly of arrays.
    Array(Vec<Value>),
    /// A map from keys to values, such as a Redis hash.
    Map(Vec<(String, Value)>),
    /// A document with its fields in order, such as a MongoDB document.
    Document(Vec<(String, Value)>),
    /// A value of a type the driver does not model, as text with its type's name.
    Other(Typed),
}

/// A value written as text together with the name of its type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Typed {
    /// The value as text.
    pub text: String,
    /// The type's name, as the database names it (`timestamptz`, `inet`).
    #[serde(rename = "type")]
    pub type_name: String,
}

pub fn float_text(value: f64) -> String {
    let size = value.abs();
    if value.is_finite() && size != 0.0 && !(1e-5..1e16).contains(&size) {
        format!("{value:e}")
    } else {
        value.to_string()
    }
}

impl Value {
    pub fn display(&self) -> Option<String> {
        Some(match self {
            Value::Null => return None,
            Value::Bool(value) => value.to_string(),
            Value::Integer(value) => value.to_string(),
            Value::Float(value) => float_text(*value),
            Value::Decimal(text)
            | Value::Text(text)
            | Value::Interval(text)
            | Value::Uuid(text)
            | Value::Json(text) => text.clone(),
            Value::Bytes(bytes) => format!("\\x{}", hex(bytes)),
            Value::Temporal(typed) | Value::Other(typed) => typed.text.clone(),
            Value::Array(items) => {
                let items: Vec<String> =
                    items.iter().map(|item| item.display().unwrap_or_else(|| "NULL".into())).collect();
                format!("{{{}}}", items.join(","))
            }
            Value::Map(fields) | Value::Document(fields) => {
                let fields: Vec<String> = fields
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", value.display().unwrap_or_else(|| "null".into())))
                    .collect();
                format!("{{{}}}", fields.join(", "))
            }
        })
    }
}

/// A column of a result: its name and its type as the database names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    /// The column's name or label.
    pub name: String,
    /// Its type's name (`int4`, `text`, `hash`).
    #[serde(rename = "type")]
    pub type_name: String,
    /// The table column it reads, when the server says (PostgreSQL's row description does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ColumnSource>,
    /// The name of the table column it reads, when the source names it instead of numbering it (MySQL, SQLite).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl Column {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self { name: name.into(), type_name: type_name.into(), source: None, origin: None }
    }
}

/// The table column a result column reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSource {
    /// The table's id on the server (PostgreSQL's oid), or `dialect::table_id` of its schema and name where the server has none.
    pub table: u32,
    /// The column's position in the table, from 1; negative for system columns; 0 when the column is known by its name (`Column::origin`).
    pub position: i16,
}

/// A message the server sent while a statement ran, such as a PostgreSQL `NOTICE`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// Its severity as the server names it (`NOTICE`, `WARNING`, `INFO`).
    pub severity: String,
    /// The message.
    pub message: String,
    /// The SQLSTATE or similar code, when the server sends one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// What running one statement gave.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    /// The columns of the rows; empty when the statement returned none.
    pub columns: Vec<Column>,
    /// The rows read, at most the limit asked for.
    pub rows: Vec<Vec<Value>>,
    /// Whether the statement had more rows than were kept.
    pub truncated: bool,
    /// How many rows the statement changed, when it reports that.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// The messages the server sent while it ran.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub notices: Vec<Notice>,
}

/// The value of one key of a key-value database, as its value view shows it: its type, time to live and size, and its items as rows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    /// The key.
    pub key: String,
    /// The logical database or the node it was read from (`db0`, `10.0.0.1:7000`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// Its type: `string`, `hash`, `list`, `set`, `zset`, `stream`, `json`, `none` for a key that does not exist, or a module's type name.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Milliseconds until it expires; `None` when it never does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<i64>,
    /// The bytes it takes in the server's memory, when the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// How many items it has, or the bytes of a string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// How the server stores it (`listpack`, `hashtable`, `embstr`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    /// The columns of its rows (`value`; `field`, `value`; `index`, `value`; `member`, `score`; `id`, `fields`).
    pub columns: Vec<Column>,
    /// Its items, at most the limit asked for.
    pub rows: Vec<Vec<Value>>,
    /// Whether items were left out.
    pub truncated: bool,
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::hex(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        (0..text.len())
            .step_by(2)
            .map(|at| {
                text.get(at..at + 2)
                    .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                    .ok_or_else(|| serde::de::Error::custom("not hexadecimal"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_read_as_plain_digits_or_a_short_exponent_and_keep_every_digit() {
        assert_eq!(Value::Float(1e300).display().as_deref(), Some("1e300"));
        assert_eq!(Value::Float(-2.5e-12).display().as_deref(), Some("-2.5e-12"));
        assert_eq!(Value::Float(11.25).display().as_deref(), Some("11.25"));
        assert_eq!(Value::Float(0.0).display().as_deref(), Some("0"));
        assert_eq!(Value::Float(1234567890123.5).display().as_deref(), Some("1234567890123.5"));
        assert_eq!(Value::Float(f64::INFINITY).display().as_deref(), Some("inf"));
        let text = float_text(std::f64::consts::PI * 1e20);
        assert_eq!(text.parse::<f64>().unwrap(), std::f64::consts::PI * 1e20);
    }
}
