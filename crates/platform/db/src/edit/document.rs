use std::hash::{BuildHasher, Hasher};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{
    ChangeKind, Changes, EditColumn, Editing, Fetch, Generate, InputError, KeyKind, Planned, ReadOnly, RowKey, Staged,
};
use crate::dialect::SqlDialect;
use crate::value::{Column, Typed, Value};

const ID: &str = "_id";
const CURSOR_METHODS: &[&str] =
    &["sort", "limit", "skip", "projection", "batchSize", "hint", "comment", "maxTimeMS", "collation", "allowDiskUse"];

struct Scanner {
    chars: Vec<char>,
    at: usize,
}

impl Scanner {
    fn space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn eat(&mut self, wanted: &str) -> bool {
        self.space();
        let found = wanted.chars().enumerate().all(|(offset, c)| self.chars.get(self.at + offset) == Some(&c));
        if found {
            self.at += wanted.chars().count();
        }
        found
    }

    fn name(&mut self) -> Option<String> {
        self.space();
        let start = self.at;
        while self.chars.get(self.at).is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$')) {
            self.at += 1;
        }
        (self.at > start).then(|| self.chars[start..self.at].iter().collect())
    }

    fn string(&mut self) -> Option<String> {
        self.space();
        let quote = *self.chars.get(self.at).filter(|c| matches!(c, '"' | '\''))?;
        self.at += 1;
        let mut text = String::new();
        loop {
            let c = *self.chars.get(self.at)?;
            self.at += 1;
            match c {
                c if c == quote => return Some(text),
                '\\' => {
                    text.push(*self.chars.get(self.at)?);
                    self.at += 1;
                }
                c => text.push(c),
            }
        }
    }

    fn arguments(&mut self) -> bool {
        if !self.eat("(") {
            return false;
        }
        let mut depth = 1usize;
        while let Some(&c) = self.chars.get(self.at) {
            match c {
                '"' | '\'' => {
                    if self.string().is_none() {
                        return false;
                    }
                    continue;
                }
                '(' | '{' | '[' => depth += 1,
                ')' | '}' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        self.at += 1;
                        return c == ')';
                    }
                }
                _ => {}
            }
            self.at += 1;
        }
        false
    }

    fn done(&mut self) -> bool {
        self.space();
        while self.eat(";") {}
        self.space();
        self.at >= self.chars.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub database: Option<String>,
    pub collection: String,
}

impl Target {
    pub fn expression(&self) -> String {
        let collection = format!("getCollection({})", json_string(&self.collection));
        match &self.database {
            Some(database) => format!("db.getSiblingDB({}).{collection}", json_string(database)),
            None => format!("db.{collection}"),
        }
    }

    pub fn name(&self) -> String {
        match &self.database {
            Some(database) => format!("{database}.{}", self.collection),
            None => self.collection.clone(),
        }
    }
}

pub fn find_text(database: &str, collection: &str) -> String {
    let target = Target { database: Some(database.to_owned()), collection: collection.to_owned() };
    format!("{}.find({{}})", target.expression())
}

pub fn collection_of(statement: &str) -> Option<Target> {
    let mut scanner = Scanner { chars: statement.trim().chars().collect(), at: 0 };
    if !scanner.eat("db.") {
        return None;
    }
    let mut database = None;
    let mut name = scanner.name()?;
    if name == "getSiblingDB" {
        if !scanner.eat("(") {
            return None;
        }
        database = Some(scanner.string()?);
        if !scanner.eat(")") || !scanner.eat(".") {
            return None;
        }
        name = scanner.name()?;
    }
    let collection = if name == "getCollection" {
        if !scanner.eat("(") {
            return None;
        }
        let name = scanner.string()?;
        if !scanner.eat(")") {
            return None;
        }
        name
    } else {
        name
    };
    if !scanner.eat(".") {
        return None;
    }
    let method = scanner.name()?;
    if !matches!(method.as_str(), "find" | "findOne") || !scanner.arguments() {
        return None;
    }
    loop {
        if scanner.done() {
            return Some(Target { database, collection }).filter(|target| !target.collection.is_empty());
        }
        if !scanner.eat(".") {
            return None;
        }
        let modifier = scanner.name()?;
        if !CURSOR_METHODS.contains(&modifier.as_str()) || !scanner.arguments() {
            return None;
        }
    }
}

pub fn identify(columns: &[Column], target: &Target) -> Editing {
    let edit_columns: Vec<EditColumn> = columns
        .iter()
        .enumerate()
        .map(|(index, column)| EditColumn {
            index,
            name: column.name.clone(),
            type_name: column.type_name.clone(),
            editable: column.name != ID,
            nullable: column.name != ID,
            default: None,
            sourced: true,
        })
        .collect();
    let key = columns.iter().position(|column| column.name == ID).map(|index| RowKey {
        kind: KeyKind::PrimaryKey,
        columns: vec![index],
        names: vec![ID.to_owned()],
    });
    let reason = key.is_none().then_some(ReadOnly::NoKey);
    Editing {
        editable: reason.is_none(),
        reason,
        table: Some(target.name()),
        key,
        columns: edit_columns,
        dialect: SqlDialect::default(),
        documents: true,
        target: Some(target.expression()),
    }
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

fn is_object_id(text: &str) -> bool {
    text.len() == 24 && text.chars().all(|c| c.is_ascii_hexdigit())
}

fn number(text: &str) -> Option<Value> {
    if let Ok(integer) = text.parse::<i64>() {
        return Some(Value::Integer(integer));
    }
    text.parse::<f64>().ok().filter(|value| value.is_finite()).map(Value::Float)
}

fn inferred(text: &str) -> Value {
    let trimmed = text.trim();
    if let Some(value) = number(trimmed) {
        return value;
    }
    match trimmed {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return Value::Json(trimmed.to_owned());
    }
    if trimmed.starts_with('"')
        && let Ok(serde_json::Value::String(inner)) = serde_json::from_str::<serde_json::Value>(trimmed)
    {
        return Value::Text(inner);
    }
    let constructor = ["ObjectId(", "ISODate(", "NumberLong(", "NumberDecimal(", "UUID(", "BinData(", "Timestamp("];
    if constructor.iter().any(|prefix| trimmed.starts_with(prefix)) && trimmed.ends_with(')') {
        return Value::Other(Typed { text: trimmed.to_owned(), type_name: "mixed".to_owned() });
    }
    Value::Text(text.to_owned())
}

pub fn parse_input(text: &str, type_name: &str) -> Result<Value, InputError> {
    let trimmed = text.trim();
    match type_name {
        "string" | "symbol" => Ok(Value::Text(text.to_owned())),
        "int" | "long" => trimmed.parse::<i64>().map(Value::Integer).map_err(|_| InputError::Integer),
        "double" => trimmed.parse::<f64>().map(Value::Float).map_err(|_| InputError::Number),
        "decimal" => {
            if trimmed.parse::<f64>().is_ok() {
                Ok(Value::Decimal(trimmed.to_owned()))
            } else {
                Err(InputError::Number)
            }
        }
        "bool" => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(Value::Bool(true)),
            "false" | "0" | "no" => Ok(Value::Bool(false)),
            _ => Err(InputError::Boolean),
        },
        "date" => Ok(Value::Temporal(Typed { text: trimmed.to_owned(), type_name: "date".to_owned() })),
        "object" | "array" => serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .filter(|json| json.is_object() == (type_name == "object") && (json.is_object() || json.is_array()))
            .map(|_| Value::Json(trimmed.to_owned()))
            .ok_or(InputError::Json),
        "objectId" => {
            let hex = trimmed
                .strip_prefix("ObjectId(")
                .and_then(|rest| rest.strip_suffix(')'))
                .map_or(trimmed, |inner| inner.trim().trim_matches(['"', '\'']));
            if is_object_id(hex) {
                Ok(Value::Other(Typed {
                    text: format!("ObjectId(\"{}\")", hex.to_ascii_lowercase()),
                    type_name: "objectId".to_owned(),
                }))
            } else {
                Err(InputError::ObjectId)
            }
        }
        "binData" => {
            if let Ok(Value::Uuid(uuid)) = super::parse_input(trimmed, "uuid") {
                return Ok(Value::Uuid(uuid));
            }
            super::parse_input(trimmed, "bytea")
        }
        "null" | "mixed" => Ok(inferred(text)),
        other => Ok(Value::Other(Typed { text: trimmed.to_owned(), type_name: other.to_owned() })),
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |acc, (at, byte)| acc | (u32::from(*byte) << (16 - 8 * at)));
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(char::from(ALPHABET[((triple >> (18 - 6 * at)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn float_literal(value: f64) -> String {
    if value.is_nan() {
        "{\"$numberDouble\": \"NaN\"}".to_owned()
    } else if value.is_infinite() {
        let sign = if value > 0.0 { "" } else { "-" };
        format!("{{\"$numberDouble\": \"{sign}Infinity\"}}")
    } else {
        format!("{value:?}")
    }
}

pub fn literal(value: &Value, type_name: &str) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Integer(number) if type_name == "long" => format!("NumberLong(\"{number}\")"),
        Value::Integer(number) if type_name == "double" => format!("{:?}", *number as f64),
        Value::Integer(number) => number.to_string(),
        Value::Float(number) => float_literal(*number),
        Value::Decimal(text) => format!("NumberDecimal({})", json_string(text)),
        Value::Text(text) | Value::Interval(text) => json_string(text),
        Value::Temporal(typed) => format!("ISODate({})", json_string(&typed.text)),
        Value::Uuid(text) => format!("UUID({})", json_string(text)),
        Value::Bytes(bytes) => format!("BinData(0, \"{}\")", base64(bytes)),
        Value::Json(text) => text.clone(),
        Value::Other(typed) => typed.text.clone(),
        Value::Array(_) | Value::Map(_) | Value::Document(_) => crate::present::json_text(value),
    }
}

pub fn shell_document(columns: &[Column], row: &[Value]) -> String {
    let mut lines = vec!["{".to_owned()];
    let fields: Vec<(&Column, &Value)> =
        columns.iter().zip(row).filter(|(_, value)| !matches!(value, Value::Null)).collect();
    for (at, (column, value)) in fields.iter().enumerate() {
        let text = match value {
            Value::Json(text) => serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(|json| serde_json::to_string_pretty(&json).ok())
                .unwrap_or_else(|| text.clone()),
            other => literal(other, &column.type_name),
        };
        let comma = if at + 1 < fields.len() { "," } else { "" };
        let text = text.replace('\n', "\n  ");
        lines.push(format!("  {}: {text}{comma}", json_string(&column.name)));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn process_bytes() -> u64 {
    static RANDOM: OnceLock<u64> = OnceLock::new();
    *RANDOM.get_or_init(|| {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u32(std::process::id());
        hasher.finish()
    })
}

pub fn new_object_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| u32::try_from(elapsed.as_secs()).unwrap_or(u32::MAX));
    let random = process_bytes();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_add((random >> 40) as u32) & 0x00FF_FFFF;
    format!("{seconds:08x}{:010x}{counter:06x}", random & 0xFF_FFFF_FFFF)
}

fn type_of(editing: &Editing, index: usize) -> &str {
    editing.columns.get(index).map_or("mixed", |column| column.type_name.as_str())
}

fn name_of(editing: &Editing, index: usize) -> String {
    editing.columns.get(index).map(|column| column.name.clone()).unwrap_or_default()
}

fn comparable(value: &Value) -> bool {
    !matches!(value, Value::Json(_) | Value::Array(_) | Value::Map(_) | Value::Document(_) | Value::Float(_))
}

fn id_filter(editing: &Editing, key: &RowKey, row: &[Value]) -> String {
    let index = key.columns.first().copied().unwrap_or(0);
    let value = row.get(index).unwrap_or(&Value::Null);
    format!("{{ \"_id\": {} }}", literal(value, type_of(editing, index)))
}

fn fields(editing: &Editing, cells: &[(usize, String)]) -> String {
    let parts: Vec<String> =
        cells.iter().map(|(index, text)| format!("{}: {text}", json_string(&name_of(editing, *index)))).collect();
    if parts.is_empty() { "{}".to_owned() } else { format!("{{ {} }}", parts.join(", ")) }
}

fn planned(kind: ChangeKind, row: usize, text: String, returning: Vec<usize>, fetch: Option<Fetch>) -> Planned {
    Planned { kind, row, sql: text.clone(), params: Vec::new(), preview: text, returning, one_row: true, fetch }
}

pub fn plan(editing: &Editing, changes: &Changes) -> Vec<Planned> {
    let (Some(target), Some(key)) = (&editing.target, &editing.key) else { return Vec::new() };
    let id_column = key.columns.first().copied().unwrap_or(0);
    let mut out = Vec::with_capacity(changes.rows());
    for deleted in &changes.deleted {
        let text = format!("{target}.deleteOne({})", id_filter(editing, key, &deleted.original));
        out.push(planned(ChangeKind::Delete, deleted.row, text, Vec::new(), None));
    }
    for inserted in &changes.inserted {
        let mut cells: Vec<(usize, String)> = Vec::new();
        let given_id = inserted.cells.iter().find_map(|(index, staged)| match staged {
            Staged::Value(value) if *index == id_column && *value != Value::Null => Some(value.clone()),
            _ => None,
        });
        let id = given_id.unwrap_or_else(|| {
            Value::Other(Typed { text: format!("ObjectId(\"{}\")", new_object_id()), type_name: "objectId".to_owned() })
        });
        cells.push((id_column, literal(&id, type_of(editing, id_column))));
        for (index, staged) in &inserted.cells {
            let Staged::Value(value) = staged else { continue };
            if *index == id_column || editing.columns.get(*index).is_none() {
                continue;
            }
            cells.push((*index, literal(value, type_of(editing, *index))));
        }
        let text = format!("{target}.insertOne({})", fields(editing, &cells));
        let mut row = vec![Value::Null; editing.columns.len()];
        if let Some(slot) = row.get_mut(id_column) {
            *slot = id;
        }
        let fetch_text = format!("{target}.find({}, {{ \"_id\": 1 }})", id_filter(editing, key, &row));
        let fetch = Fetch { sql: fetch_text, params: Vec::new() };
        out.push(planned(ChangeKind::Insert, inserted.row, text, vec![id_column], Some(fetch)));
    }
    for updated in &changes.updated {
        let mut set = Vec::new();
        let mut unset = Vec::new();
        let mut checks = Vec::new();
        for (index, staged) in &updated.cells {
            if !editing.columns.get(*index).is_some_and(|column| column.editable) {
                continue;
            }
            match staged {
                Staged::Value(value) => set.push((*index, literal(value, type_of(editing, *index)))),
                Staged::Default => unset.push((*index, "\"\"".to_owned())),
            }
            let original = updated.original.get(*index).unwrap_or(&Value::Null);
            if comparable(original) {
                checks.push((*index, literal(original, type_of(editing, *index))));
            }
        }
        if set.is_empty() && unset.is_empty() {
            continue;
        }
        let id_index = key.columns.first().copied().unwrap_or(0);
        let id = updated.original.get(id_index).unwrap_or(&Value::Null);
        let mut filter = vec![(id_index, literal(id, type_of(editing, id_index)))];
        filter.extend(checks);
        let mut changes = Vec::new();
        if !set.is_empty() {
            changes.push(format!("\"$set\": {}", fields(editing, &set)));
        }
        if !unset.is_empty() {
            changes.push(format!("\"$unset\": {}", fields(editing, &unset)));
        }
        let text = format!("{target}.updateOne({}, {{ {} }})", fields(editing, &filter), changes.join(", "));
        out.push(planned(ChangeKind::Update, updated.row, text, Vec::new(), None));
    }
    out
}

pub fn generate(editing: &Editing, kind: Generate, rows: &[Vec<Value>]) -> Option<String> {
    let target = editing.target.as_ref()?;
    let key = editing.key.as_ref()?;
    let id_index = key.columns.first().copied().unwrap_or(0);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let present: Vec<(usize, String)> = editing
            .columns
            .iter()
            .filter(|column| row.get(column.index).is_some_and(|value| *value != Value::Null))
            .map(|column| (column.index, literal(&row[column.index], &column.type_name)))
            .collect();
        let filter = id_filter(editing, key, row);
        let text = match kind {
            Generate::Select => format!("{target}.find({filter})"),
            Generate::Insert => format!("{target}.insertOne({})", fields(editing, &present)),
            Generate::Update => {
                let set: Vec<(usize, String)> = present.into_iter().filter(|(index, _)| *index != id_index).collect();
                format!("{target}.updateOne({filter}, {{ \"$set\": {} }})", fields(editing, &set))
            }
            Generate::Delete => format!("{target}.deleteOne({filter})"),
        };
        out.push(format!("{text};"));
    }
    Some(out.join("\n"))
}

#[cfg(test)]
mod tests;
