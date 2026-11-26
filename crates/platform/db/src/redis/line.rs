use std::collections::BTreeMap;

use ::redis::Value as Reply;

use crate::value::{Column, Value};

pub fn split(line: &str) -> Option<Vec<Vec<u8>>> {
    let bytes = line.as_bytes();
    let mut args = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at == bytes.len() {
            break;
        }
        let mut arg = Vec::new();
        let mut quoted = false;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
            match bytes[at] {
                b'"' => {
                    quoted = true;
                    at += 1;
                    loop {
                        let byte = *bytes.get(at)?;
                        at += 1;
                        match byte {
                            b'"' => break,
                            b'\\' => {
                                let escaped = *bytes.get(at)?;
                                at += 1;
                                match escaped {
                                    b'n' => arg.push(b'\n'),
                                    b'r' => arg.push(b'\r'),
                                    b't' => arg.push(b'\t'),
                                    b'b' => arg.push(8),
                                    b'a' => arg.push(7),
                                    b'x' => match line.get(at..at + 2).and_then(|hex| u8::from_str_radix(hex, 16).ok())
                                    {
                                        Some(value) => {
                                            arg.push(value);
                                            at += 2;
                                        }
                                        None => arg.push(b'x'),
                                    },
                                    other => arg.push(other),
                                }
                            }
                            other => arg.push(other),
                        }
                    }
                }
                b'\'' => {
                    quoted = true;
                    at += 1;
                    loop {
                        let byte = *bytes.get(at)?;
                        at += 1;
                        match byte {
                            b'\'' => break,
                            b'\\' if bytes.get(at) == Some(&b'\'') => {
                                arg.push(b'\'');
                                at += 1;
                            }
                            other => arg.push(other),
                        }
                    }
                }
                other => {
                    arg.push(other);
                    at += 1;
                }
            }
        }
        if !arg.is_empty() || quoted {
            args.push(arg);
        }
    }
    Some(args)
}

pub fn quote(arg: &[u8]) -> String {
    let plain = !arg.is_empty()
        && arg.iter().all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\'' | b'\\'))
        && std::str::from_utf8(arg).is_ok();
    if plain {
        return String::from_utf8_lossy(arg).into_owned();
    }
    if let Ok(text) = std::str::from_utf8(arg) {
        let mut out = String::with_capacity(text.len() + 2);
        out.push('"');
        for c in text.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        return out;
    }
    let mut out = String::from("\"");
    for byte in arg {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            byte if byte.is_ascii_graphic() || *byte == b' ' => out.push(char::from(*byte)),
            byte => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push('"');
    out
}

pub fn value(reply: Reply) -> Value {
    match reply {
        Reply::Nil => Value::Null,
        Reply::Int(number) => Value::Integer(number),
        Reply::BulkString(bytes) => text(bytes),
        Reply::SimpleString(text) => Value::Text(text),
        Reply::Okay => Value::Text("OK".to_owned()),
        Reply::Array(items) | Reply::Set(items) => Value::Array(items.into_iter().map(value).collect()),
        Reply::Push { data, .. } => Value::Array(data.into_iter().map(value).collect()),
        Reply::Map(pairs) => Value::Map(pairs.into_iter().map(|(key, item)| (name(key), value(item))).collect()),
        Reply::Attribute { data, .. } => value(*data),
        Reply::Double(number) => Value::Float(number),
        Reply::Boolean(flag) => Value::Bool(flag),
        Reply::VerbatimString { text, .. } => Value::Text(text),
        Reply::BigNumber(digits) => Value::Decimal(String::from_utf8_lossy(&digits).into_owned()),
        Reply::ServerError(error) => Value::Other(crate::value::Typed {
            text: format!("{} {}", error.code(), error.details().unwrap_or_default()).trim().to_owned(),
            type_name: "error".to_owned(),
        }),
        other => Value::Other(crate::value::Typed { text: format!("{other:?}"), type_name: "reply".to_owned() }),
    }
}

pub fn text(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(text) => Value::Text(text),
        Err(error) => Value::Bytes(error.into_bytes()),
    }
}

pub fn name(reply: Reply) -> String {
    match reply {
        Reply::BulkString(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Reply::SimpleString(text) | Reply::VerbatimString { text, .. } => text,
        other => value(other).display().unwrap_or_default(),
    }
}

pub fn integer(reply: &Reply) -> Option<i64> {
    match reply {
        Reply::Int(number) => Some(*number),
        Reply::BulkString(bytes) => std::str::from_utf8(bytes).ok()?.trim().parse().ok(),
        Reply::SimpleString(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn column(name: &str, type_name: &str) -> Column {
    Column::new(name.to_owned(), type_name.to_owned())
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "nil",
        Value::Integer(_) => "integer",
        Value::Float(_) => "double",
        Value::Bool(_) => "boolean",
        Value::Decimal(_) => "bignumber",
        Value::Bytes(_) => "bytes",
        Value::Array(_) => "array",
        Value::Map(_) | Value::Document(_) => "map",
        _ => "string",
    }
}

pub fn rows(reply: Reply, max_rows: usize) -> (Vec<Column>, Vec<Vec<Value>>, bool) {
    match reply {
        Reply::Map(pairs) => {
            let truncated = pairs.len() > max_rows;
            let rows = pairs.into_iter().take(max_rows).map(|(key, item)| vec![Value::Text(name(key)), value(item)]);
            (vec![column("field", "string"), column("value", "value")], rows.collect(), truncated)
        }
        Reply::Array(items) | Reply::Set(items) => {
            let truncated = items.len() > max_rows;
            let rows =
                items.into_iter().take(max_rows).enumerate().map(|(index, item)| {
                    vec![Value::Integer(i64::try_from(index + 1).unwrap_or(i64::MAX)), value(item)]
                });
            (vec![column("#", "integer"), column("value", "value")], rows.collect(), truncated)
        }
        Reply::Attribute { data, .. } => rows(*data, max_rows),
        other => {
            let value = value(other);
            let kind = type_of(&value);
            (vec![column("reply", kind)], vec![vec![value]], false)
        }
    }
}

pub fn pairs(reply: Reply) -> Vec<(Reply, Reply)> {
    match reply {
        Reply::Map(pairs) => pairs,
        Reply::Array(items) => {
            if items.iter().all(|item| matches!(item, Reply::Array(pair) if pair.len() == 2)) {
                items
                    .into_iter()
                    .filter_map(|item| match item {
                        Reply::Array(mut pair) => {
                            let second = pair.pop()?;
                            let first = pair.pop()?;
                            Some((first, second))
                        }
                        _ => None,
                    })
                    .collect()
            } else {
                let mut out = Vec::with_capacity(items.len() / 2);
                let mut items = items.into_iter();
                while let (Some(first), Some(second)) = (items.next(), items.next()) {
                    out.push((first, second));
                }
                out
            }
        }
        _ => Vec::new(),
    }
}

pub fn glob_escape(prefix: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 1);
    for byte in prefix {
        if matches!(byte, b'*' | b'?' | b'[' | b']' | b'\\') {
            out.push(b'\\');
        }
        out.push(*byte);
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grouped {
    pub prefixes: BTreeMap<String, usize>,
    pub keys: Vec<String>,
}

pub fn group<'a>(prefix: &str, keys: impl IntoIterator<Item = &'a str>, separator: &str) -> Grouped {
    let mut grouped = Grouped::default();
    for key in keys {
        let Some(rest) = key.strip_prefix(prefix) else { continue };
        match (!separator.is_empty()).then(|| rest.find(separator)).flatten() {
            Some(at) => *grouped.prefixes.entry(rest[..at + separator.len()].to_owned()).or_default() += 1,
            None => grouped.keys.push(rest.to_owned()),
        }
    }
    grouped.keys.sort();
    grouped.keys.dedup();
    grouped
}

#[cfg(test)]
mod tests {
    use super::rows as rows_of;
    use super::*;

    fn words(line: &str) -> Vec<String> {
        split(line).unwrap().into_iter().map(|arg| String::from_utf8_lossy(&arg).into_owned()).collect()
    }

    #[test]
    fn lines_split_as_redis_cli_reads_them() {
        assert_eq!(words("  SET  user:1   hello "), ["SET", "user:1", "hello"]);
        assert_eq!(words(r#"SET "a b" "line\nnext" 'it\'s'"#), ["SET", "a b", "line\nnext", "it's"]);
        assert_eq!(split(r#"SET k "\x00\xff""#).unwrap()[2], vec![0, 255]);
        assert_eq!(words(r#"SET k """#), ["SET", "k", ""]);
        assert!(split(r#"SET k "open"#).is_none());
        assert!(split("").unwrap().is_empty());
    }

    #[test]
    fn quoting_reads_back() {
        for arg in [&b"plain"[..], b"with space", b"q\"uote", b"back\\slash", b"", b"\x00\xff", "ключ".as_bytes()] {
            let line = format!("GET {}", quote(arg));
            assert_eq!(split(&line).unwrap()[1], arg, "{line}");
        }
        assert_eq!(quote(b"user:1"), "user:1");
    }

    #[test]
    fn replies_become_values_and_rows() {
        let map = Reply::Map(vec![(Reply::BulkString(b"name".to_vec()), Reply::BulkString(b"tea".to_vec()))]);
        let (columns, rows, truncated) = rows_of(map, 10);
        assert_eq!(columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(), ["field", "value"]);
        assert_eq!(rows, vec![vec![Value::Text("name".into()), Value::Text("tea".into())]]);
        assert!(!truncated);
        let array = Reply::Array(vec![Reply::Int(1), Reply::Nil, Reply::BulkString(vec![0xff])]);
        let (_, rows, truncated) = rows_of(array, 2);
        assert_eq!(rows, vec![vec![Value::Integer(1), Value::Integer(1)], vec![Value::Integer(2), Value::Null]]);
        assert!(truncated);
        let (columns, rows, _) = rows_of(Reply::Okay, 10);
        assert_eq!((columns[0].name.as_str(), &rows[0][0]), ("reply", &Value::Text("OK".into())));
        assert_eq!(value(Reply::Double(1.5)), Value::Float(1.5));
        assert_eq!(value(Reply::BulkString(vec![0xff, 0])), Value::Bytes(vec![0xff, 0]));
    }

    #[test]
    fn flat_and_paired_replies_pair_up() {
        let flat = Reply::Array(vec![Reply::Int(1), Reply::Int(2), Reply::Int(3), Reply::Int(4)]);
        assert_eq!(pairs(flat).len(), 2);
        let paired = Reply::Array(vec![Reply::Array(vec![Reply::Int(1), Reply::Double(2.0)])]);
        assert_eq!(pairs(paired), vec![(Reply::Int(1), Reply::Double(2.0))]);
    }

    #[test]
    fn keys_group_by_the_separator() {
        let keys = ["user:1", "user:2", "user:2:cart", "session:x", "plain", "user:"];
        let grouped = group("", keys.iter().copied(), ":");
        assert_eq!(grouped.prefixes.get("user:"), Some(&4));
        assert_eq!(grouped.prefixes.get("session:"), Some(&1));
        assert_eq!(grouped.keys, ["plain"]);
        let inner = group("user:", keys.iter().copied(), ":");
        assert_eq!(inner.keys, ["", "1", "2"]);
        assert_eq!(inner.prefixes.get("2:"), Some(&1));
        assert_eq!(glob_escape(b"a*b?[c]\\"), b"a\\*b\\?\\[c\\]\\\\".to_vec());
    }
}
