use std::fmt;

use serde::de::{Deserializer, MapAccess, Visitor};
use serde_json::{Value as Json, json};
use zen_db_driver_kit::value;

struct Fields(Vec<(String, Json)>);

impl<'de> serde::Deserialize<'de> for Fields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Ordered;

        impl<'de> Visitor<'de> for Ordered {
            type Value = Fields;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Fields, A::Error> {
                let mut fields = Vec::new();
                while let Some(entry) = map.next_entry::<String, Json>()? {
                    fields.push(entry);
                }
                Ok(Fields(fields))
            }
        }

        deserializer.deserialize_map(Ordered)
    }
}

fn quote_bare_numbers(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            rest = &rest[c.len_utf8()..];
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            rest = &rest[1..];
            continue;
        }
        if let Some(word) = ["-Infinity", "Infinity", "-NaN", "NaN"].iter().find(|word| rest.starts_with(**word)) {
            out.push('"');
            out.push_str(if word.ends_with("NaN") { "NaN" } else { word });
            out.push('"');
            rest = &rest[word.len()..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

pub(crate) fn row(line: &str) -> Option<Vec<(String, Json)>> {
    if let Ok(Fields(fields)) = serde_json::from_str::<Fields>(line) {
        return Some(fields);
    }
    serde_json::from_str::<Fields>(&quote_bare_numbers(line)).ok().map(|Fields(fields)| fields)
}

fn base(type_name: &str) -> String {
    let upper = type_name.trim().to_ascii_uppercase();
    upper.split('(').next().unwrap_or_default().trim().to_owned()
}

fn nested(type_name: &str) -> bool {
    let upper = type_name.trim().to_ascii_uppercase();
    upper.ends_with(']') || ["STRUCT", "MAP", "UNION", "LIST"].iter().any(|kind| upper.starts_with(kind))
}

fn text_of(json: &Json) -> String {
    match json {
        Json::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn blob(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\\'
            && bytes.get(at + 1) == Some(&b'x')
            && let Some(byte) = text.get(at + 2..at + 4).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            at += 4;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    out
}

fn float(json: &Json) -> Json {
    match json {
        Json::Number(number) => number.as_f64().map_or_else(|| value::decimal(number.to_string()), value::float),
        Json::String(text) => match text.as_str() {
            "NaN" => value::float(f64::NAN),
            "Infinity" => value::float(f64::INFINITY),
            "-Infinity" => value::float(f64::NEG_INFINITY),
            other => other.parse::<f64>().map_or_else(|_| value::text(other), value::float),
        },
        other => value::text(other.to_string()),
    }
}

fn inferred(json: &Json) -> Json {
    match json {
        Json::Null => value::null(),
        Json::Bool(flag) => value::boolean(*flag),
        Json::Number(number) => match number.as_i64() {
            Some(integer) => value::integer(integer),
            None => float(json),
        },
        Json::String(text) => value::text(text.clone()),
        Json::Array(_) | Json::Object(_) => value::json_text(json.to_string()),
    }
}

pub(crate) fn cell(json: &Json, type_name: Option<&str>) -> Json {
    let Some(type_name) = type_name else { return inferred(json) };
    if json.is_null() {
        return value::null();
    }
    if nested(type_name) {
        return value::json_text(json.to_string());
    }
    match base(type_name).as_str() {
        "BOOLEAN" => json.as_bool().map_or_else(|| inferred(json), value::boolean),
        "TINYINT" | "SMALLINT" | "INTEGER" | "BIGINT" | "UTINYINT" | "USMALLINT" | "UINTEGER" | "UBIGINT" => {
            match json.as_i64() {
                Some(integer) => value::integer(integer),
                None => value::decimal(text_of(json)),
            }
        }
        "HUGEINT" | "UHUGEINT" | "DECIMAL" | "VARINT" | "BIGNUM" => value::decimal(text_of(json)),
        "FLOAT" | "DOUBLE" | "REAL" => float(json),
        "VARCHAR" | "ENUM" | "BIT" => value::text(text_of(json)),
        "DATE"
        | "TIME"
        | "TIMESTAMP"
        | "TIMESTAMP WITH TIME ZONE"
        | "TIMESTAMP_S"
        | "TIMESTAMP_MS"
        | "TIMESTAMP_NS"
        | "TIME WITH TIME ZONE" => value::temporal(text_of(json), &type_name.to_ascii_lowercase()),
        "INTERVAL" => json!({ "kind": "interval", "value": text_of(json) }),
        "UUID" => value::uuid(text_of(json)),
        "BLOB" => value::bytes(&blob(&text_of(json))),
        "JSON" => match json {
            Json::String(text) => value::json_text(text.clone()),
            other => value::json_text(other.to_string()),
        },
        _ => value::other(&text_of(json), type_name),
    }
}

#[cfg(test)]
mod tests;
