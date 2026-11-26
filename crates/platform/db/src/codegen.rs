use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::Column;

/// The language a type is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CodeLanguage {
    /// A Go struct with `db` and `json` tags.
    #[default]
    Go,
    /// A Rust struct.
    Rust,
    /// A TypeScript interface.
    TypeScript,
}

/// A type written from a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Generated {
    /// The type's name.
    pub type_name: String,
    /// The code.
    pub code: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bool,
    Int(u16),
    Unsigned(u16),
    Float32,
    Float64,
    Decimal,
    Text,
    Bytes,
    Uuid,
    Date,
    Time,
    Timestamp,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mapped {
    kind: Kind,
    array: bool,
    nullable: bool,
}

fn strip_wrapper<'a>(text: &'a str, wrapper: &str) -> Option<&'a str> {
    text.strip_prefix(wrapper).and_then(|rest| rest.strip_prefix('(')).and_then(|rest| rest.strip_suffix(')'))
}

fn map_type(driver: &str, type_name: &str, nullable: bool) -> Mapped {
    let mut text = type_name.trim().to_ascii_lowercase();
    let mut mapped = Mapped { kind: Kind::Text, array: false, nullable };
    loop {
        if let Some(inner) = strip_wrapper(&text, "nullable") {
            mapped.nullable = true;
            text = inner.to_owned();
        } else if let Some(inner) = strip_wrapper(&text, "lowcardinality") {
            text = inner.to_owned();
        } else if let Some(inner) = strip_wrapper(&text, "array") {
            mapped.array = true;
            text = inner.to_owned();
        } else {
            break;
        }
    }
    if let Some(inner) = text.strip_suffix("[]") {
        mapped.array = true;
        text = inner.trim_end_matches("[]").to_owned();
    } else if driver == "postgresql"
        && let Some(inner) = text.strip_prefix('_')
    {
        mapped.array = true;
        text = inner.to_owned();
    }
    let unsigned = text.contains("unsigned");
    let boolean_tinyint = text.starts_with("tinyint(1)") || text == "bit(1)";
    let base = text.split('(').next().unwrap_or_default().replace(" unsigned", "").replace(" zerofill", "");
    let base = base.trim();
    mapped.kind = if boolean_tinyint {
        Kind::Bool
    } else if driver == "sqlite" {
        sqlite_kind(base)
    } else if driver == "clickhouse" {
        clickhouse_kind(base)
    } else {
        sql_kind(base)
    };
    if unsigned && let Kind::Int(bits) = mapped.kind {
        mapped.kind = Kind::Unsigned(bits);
    }
    mapped
}

fn sql_kind(base: &str) -> Kind {
    match base {
        "bool" | "boolean" => Kind::Bool,
        "tinyint" => Kind::Int(8),
        "smallint" | "int2" | "smallserial" | "serial2" | "year" => Kind::Int(16),
        "mediumint" | "int" | "integer" | "int4" | "serial" | "serial4" | "oid" => Kind::Int(32),
        "bigint" | "int8" | "bigserial" | "serial8" => Kind::Int(64),
        "real" | "float4" | "float" => Kind::Float32,
        "double precision" | "double" | "float8" => Kind::Float64,
        "numeric" | "decimal" | "dec" | "money" | "fixed" => Kind::Decimal,
        "bytea" | "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" | "bit" | "varbit"
        | "bit varying" => Kind::Bytes,
        "uuid" => Kind::Uuid,
        "date" => Kind::Date,
        "time" | "timetz" | "time without time zone" | "time with time zone" => Kind::Time,
        "timestamp" | "timestamptz" | "timestamp without time zone" | "timestamp with time zone" | "datetime" => {
            Kind::Timestamp
        }
        "json" | "jsonb" => Kind::Json,
        _ => Kind::Text,
    }
}

fn sqlite_kind(base: &str) -> Kind {
    match base {
        "boolean" | "bool" => Kind::Bool,
        "date" => Kind::Date,
        "datetime" | "timestamp" => Kind::Timestamp,
        "json" => Kind::Json,
        _ if base.contains("int") => Kind::Int(64),
        _ if base.contains("char") || base.contains("clob") || base.contains("text") => Kind::Text,
        _ if base.contains("blob") => Kind::Bytes,
        _ if base.contains("real") || base.contains("floa") || base.contains("doub") => Kind::Float64,
        "" => Kind::Text,
        _ => Kind::Decimal,
    }
}

fn clickhouse_kind(base: &str) -> Kind {
    let bits = |rest: &str| rest.parse::<u16>().ok();
    if let Some(bits) = base.strip_prefix("uint").and_then(bits) {
        return Kind::Unsigned(bits);
    }
    if let Some(bits) = base.strip_prefix("int").and_then(bits) {
        return Kind::Int(bits);
    }
    match base {
        "bool" | "boolean" => Kind::Bool,
        "float32" => Kind::Float32,
        "float64" => Kind::Float64,
        "uuid" => Kind::Uuid,
        "date" | "date32" => Kind::Date,
        "datetime" | "datetime64" => Kind::Timestamp,
        "json" | "object" => Kind::Json,
        _ if base.starts_with("decimal") => Kind::Decimal,
        _ => Kind::Text,
    }
}

fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous: Option<char> = None;
    for c in name.chars() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            previous = None;
            continue;
        }
        if c.is_uppercase() && previous.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.extend(c.to_lowercase());
        previous = Some(c);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

const GO_INITIALISMS: [&str; 20] = [
    "id", "url", "uri", "api", "http", "https", "json", "xml", "sql", "uuid", "ip", "html", "css", "cpu", "db", "tls",
    "ssh", "utc", "ttl", "acl",
];

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn pascal(name: &str, initialisms: bool) -> String {
    let out: String = words(name)
        .iter()
        .map(|word| {
            if initialisms && GO_INITIALISMS.contains(&word.as_str()) { word.to_uppercase() } else { capitalised(word) }
        })
        .collect();
    if out.is_empty() {
        "Row".to_owned()
    } else if out.starts_with(|c: char| c.is_ascii_digit()) {
        format!("T{out}")
    } else {
        out
    }
}

fn singular(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if let Some(stem) = name.strip_suffix("ies").filter(|_| lower.len() > 3) {
        format!("{stem}y")
    } else if lower.ends_with("sses") || lower.ends_with("xes") || lower.ends_with("ches") || lower.ends_with("shes") {
        name[..name.len() - 2].to_owned()
    } else if lower.ends_with('s')
        && !lower.ends_with("ss")
        && !lower.ends_with("us")
        && !lower.ends_with("is")
        && lower.len() > 1
    {
        name[..name.len() - 1].to_owned()
    } else {
        name.to_owned()
    }
}

const RUST_KEYWORDS: [&str; 39] = [
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
    "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn", "gen", "try",
];

fn snake(name: &str) -> String {
    let out = words(name).join("_");
    let out = if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) { format!("_{out}") } else { out };
    if matches!(out.as_str(), "self" | "super" | "crate") {
        format!("{out}_")
    } else if RUST_KEYWORDS.contains(&out.as_str()) {
        format!("r#{out}")
    } else {
        out
    }
}

fn unique(name: String, taken: &mut BTreeSet<String>) -> String {
    let mut candidate = name.clone();
    let mut number = 2;
    while !taken.insert(candidate.clone()) {
        candidate = format!("{name}{number}");
        number += 1;
    }
    candidate
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn go_type(mapped: Mapped) -> String {
    let base = match mapped.kind {
        Kind::Bool => "bool".to_owned(),
        Kind::Int(bits) if bits <= 64 => format!("int{}", bits.max(8).next_power_of_two()),
        Kind::Unsigned(bits) if bits <= 64 => format!("uint{}", bits.max(8).next_power_of_two()),
        Kind::Float32 => "float32".to_owned(),
        Kind::Float64 => "float64".to_owned(),
        Kind::Bytes => "[]byte".to_owned(),
        Kind::Date | Kind::Timestamp => "time.Time".to_owned(),
        Kind::Json => "json.RawMessage".to_owned(),
        _ => "string".to_owned(),
    };
    let reference = matches!(mapped.kind, Kind::Bytes | Kind::Json);
    if mapped.array {
        format!("[]{base}")
    } else if mapped.nullable && !reference {
        format!("*{base}")
    } else {
        base
    }
}

fn rust_type(mapped: Mapped) -> String {
    let base = match mapped.kind {
        Kind::Bool => "bool".to_owned(),
        Kind::Int(bits) if bits <= 128 => format!("i{}", bits.max(8).next_power_of_two()),
        Kind::Unsigned(bits) if bits <= 128 => format!("u{}", bits.max(8).next_power_of_two()),
        Kind::Float32 => "f32".to_owned(),
        Kind::Float64 => "f64".to_owned(),
        Kind::Bytes => "Vec<u8>".to_owned(),
        _ => "String".to_owned(),
    };
    let base = if mapped.array { format!("Vec<{base}>") } else { base };
    if mapped.nullable { format!("Option<{base}>") } else { base }
}

fn typescript_type(mapped: Mapped) -> String {
    let base = match mapped.kind {
        Kind::Bool => "boolean",
        Kind::Int(bits) | Kind::Unsigned(bits) if bits <= 32 => "number",
        Kind::Float32 | Kind::Float64 => "number",
        Kind::Bytes => "Uint8Array",
        Kind::Date | Kind::Timestamp => "Date",
        Kind::Json => "unknown",
        _ => "string",
    };
    let base = if mapped.array { format!("{base}[]") } else { base.to_owned() };
    if mapped.nullable { format!("{base} | null") } else { base }
}

fn typescript_key(name: &str) -> String {
    let mut chars = name.chars();
    let plain = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if plain { name.to_owned() } else { format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\"")) }
}

fn go_tag_value(name: &str) -> String {
    name.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "")
}

pub fn generate(
    language: CodeLanguage,
    driver: &str,
    table: &str,
    comment: Option<&str>,
    columns: &[Column],
) -> Generated {
    let type_name = pascal(&singular(table), language == CodeLanguage::Go);
    let comment = comment.map(one_line).filter(|comment| !comment.is_empty());
    let mut taken = BTreeSet::new();
    let mut code = String::new();
    match language {
        CodeLanguage::Go => {
            let fields: Vec<(String, String, String, Option<String>)> = columns
                .iter()
                .map(|column| {
                    let name = unique(pascal(&column.name, true), &mut taken);
                    let kind = go_type(map_type(driver, &column.type_name, column.nullable));
                    let tag = go_tag_value(&column.name);
                    let note = column.comment.as_deref().map(one_line).filter(|note| !note.is_empty());
                    (name, kind, format!("`db:\"{tag}\" json:\"{tag}\"`"), note)
                })
                .collect();
            let name_width = fields.iter().map(|field| field.0.chars().count()).max().unwrap_or(0);
            let type_width = fields.iter().map(|field| field.1.chars().count()).max().unwrap_or(0);
            if let Some(comment) = &comment {
                code.push_str(&format!("// {type_name} {comment}\n"));
            }
            code.push_str(&format!("type {type_name} struct {{\n"));
            for (name, kind, tag, note) in fields {
                if let Some(note) = note {
                    code.push_str(&format!("\t// {note}\n"));
                }
                code.push_str(&format!("\t{name:name_width$} {kind:type_width$} {tag}\n"));
            }
            code.push_str("}\n");
        }
        CodeLanguage::Rust => {
            if let Some(comment) = &comment {
                code.push_str(&format!("/// {comment}\n"));
            }
            code.push_str(&format!("#[derive(Debug, Clone, PartialEq)]\npub struct {type_name} {{\n"));
            for column in columns {
                let name = unique(snake(&column.name), &mut taken);
                if let Some(note) = column.comment.as_deref().map(one_line).filter(|note| !note.is_empty()) {
                    code.push_str(&format!("    /// {note}\n"));
                }
                let kind = rust_type(map_type(driver, &column.type_name, column.nullable));
                code.push_str(&format!("    pub {name}: {kind},\n"));
            }
            code.push_str("}\n");
        }
        CodeLanguage::TypeScript => {
            if let Some(comment) = &comment {
                code.push_str(&format!("/** {} */\n", comment.replace("*/", "* /")));
            }
            code.push_str(&format!("export interface {type_name} {{\n"));
            for column in columns {
                let name = unique(typescript_key(&column.name), &mut taken);
                if let Some(note) = column.comment.as_deref().map(one_line).filter(|note| !note.is_empty()) {
                    code.push_str(&format!("  /** {} */\n", note.replace("*/", "* /")));
                }
                let kind = typescript_type(map_type(driver, &column.type_name, column.nullable));
                code.push_str(&format!("  {name}: {kind};\n"));
            }
            code.push_str("}\n");
        }
    }
    Generated { type_name, code }
}

#[cfg(test)]
mod tests;
