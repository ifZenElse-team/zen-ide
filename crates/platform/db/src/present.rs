use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::value::Value;

/// A way to show one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ValueFormat {
    /// The value as text; an array as its literal (`{1,2,"a b"}`).
    Text,
    /// JSON, pretty-printed with folding.
    Json,
    /// XML, indented with folding.
    Xml,
    /// Bytes as a hex dump with their characters.
    Hex,
    /// Bytes that hold a picture (PNG, JPEG, GIF, WebP, BMP, ICO).
    Image,
    /// A spatial value drawn as shapes, its text as WKT.
    Geometry,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub length: usize,
    pub truncated: bool,
    pub folds: Vec<(usize, usize)>,
}

pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("PNG")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("JPEG")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("GIF")
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("WebP")
    } else if bytes.starts_with(b"BM") && bytes.len() > 26 {
        Some("BMP")
    } else if bytes.starts_with(&[0, 0, 1, 0]) && bytes.len() > 6 {
        Some("ICO")
    } else {
        None
    }
}

fn is_json(text: &str) -> bool {
    let trimmed = text.trim_start();
    (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok()
}

fn is_xml(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with('<') && trimmed.ends_with('>') && xml_tokens(trimmed).is_some()
}

pub fn formats(value: &Value) -> Vec<ValueFormat> {
    let mut formats = own_formats(value);
    if crate::geometry::from_value(value).is_some() {
        let at = usize::from(matches!(value, Value::Json(_)) || formats[0] == ValueFormat::Json);
        formats.insert(at.min(formats.len()), ValueFormat::Geometry);
    }
    formats
}

pub fn text_or_json(value: &Value) -> ValueFormat {
    if own_formats(value).first() == Some(&ValueFormat::Json) { ValueFormat::Json } else { ValueFormat::Text }
}

fn own_formats(value: &Value) -> Vec<ValueFormat> {
    match value {
        Value::Json(_) | Value::Map(_) | Value::Document(_) => vec![ValueFormat::Json, ValueFormat::Text],
        Value::Array(_) => vec![ValueFormat::Text, ValueFormat::Json],
        Value::Bytes(bytes) => {
            let mut formats = Vec::new();
            if image_kind(bytes).is_some() {
                formats.push(ValueFormat::Image);
            }
            formats.push(ValueFormat::Hex);
            if let Ok(text) = std::str::from_utf8(bytes) {
                if is_json(text) {
                    formats.push(ValueFormat::Json);
                }
                if is_xml(text) {
                    formats.push(ValueFormat::Xml);
                }
                formats.push(ValueFormat::Text);
            }
            formats
        }
        Value::Text(text) | Value::Other(crate::value::Typed { text, .. }) => {
            let mut formats = Vec::new();
            if is_json(text) {
                formats.push(ValueFormat::Json);
            }
            if is_xml(text) {
                formats.push(ValueFormat::Xml);
            }
            formats.push(ValueFormat::Text);
            formats.push(ValueFormat::Hex);
            formats
        }
        _ => vec![ValueFormat::Text],
    }
}

pub fn chosen(value: &Value, wanted: Option<ValueFormat>) -> ValueFormat {
    let offered = formats(value);
    wanted.filter(|format| offered.contains(format)).unwrap_or(offered[0])
}

fn quote_element(text: &str) -> String {
    let plain = !text.is_empty()
        && !text.eq_ignore_ascii_case("null")
        && !text.chars().any(|c| matches!(c, '{' | '}' | ',' | '"' | '\\') || c.is_whitespace());
    if plain { text.to_owned() } else { format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"")) }
}

pub fn array_literal(items: &[Value]) -> String {
    let items: Vec<String> = items
        .iter()
        .map(|item| match item {
            Value::Null => "NULL".to_owned(),
            Value::Array(inner) => array_literal(inner),
            other => quote_element(&other.display().unwrap_or_default()),
        })
        .collect();
    format!("{{{}}}", items.join(","))
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

pub fn json_text(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Integer(value) => value.to_string(),
        Value::Float(value) if value.is_finite() => crate::value::float_text(*value),
        Value::Float(value) => json_string(&value.to_string()),
        Value::Decimal(text) if text.parse::<f64>().is_ok_and(f64::is_finite) => text.clone(),
        Value::Json(text) => text.clone(),
        Value::Array(items) => format!("[{}]", items.iter().map(json_text).collect::<Vec<_>>().join(",")),
        Value::Map(fields) | Value::Document(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, value)| format!("{}:{}", json_string(key), json_text(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Bytes(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) if is_json(text) => text.to_owned(),
            _ => json_string(&value.display().unwrap_or_default()),
        },
        Value::Text(text) if is_json(text) => text.clone(),
        other => json_string(&other.display().unwrap_or_default()),
    }
}

pub fn pretty_json(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut out = String::with_capacity(text.len() * 2);
    let mut folds = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut line = 0usize;
    let mut depth = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    let newline = |out: &mut String, line: &mut usize, depth: usize| {
        out.push('\n');
        *line += 1;
        out.extend(std::iter::repeat_n(' ', depth * 2));
    };
    while at < chars.len() {
        let c = chars[at];
        match c {
            '"' => {
                out.push(c);
                at += 1;
                while at < chars.len() {
                    let inner = chars[at];
                    out.push(inner);
                    at += 1;
                    if inner == '\\' && at < chars.len() {
                        out.push(chars[at]);
                        at += 1;
                    } else if inner == '"' {
                        break;
                    }
                }
                continue;
            }
            '{' | '[' => {
                let close = if c == '{' { '}' } else { ']' };
                let next = chars[at + 1..].iter().position(|c| !c.is_whitespace()).map(|skip| chars[at + 1 + skip]);
                out.push(c);
                if next == Some(close) {
                    out.push(close);
                    let skip = chars[at + 1..].iter().position(|c| *c == close).unwrap_or(0);
                    at += skip + 2;
                    continue;
                }
                open.push(line);
                depth += 1;
                newline(&mut out, &mut line, depth);
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, &mut line, depth);
                out.push(c);
                if let Some(start) = open.pop()
                    && line > start
                {
                    folds.push((start, line));
                }
            }
            ',' => {
                out.push(c);
                newline(&mut out, &mut line, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
        at += 1;
    }
    folds.sort_unstable();
    (out, folds)
}

#[derive(Debug, PartialEq, Eq)]
enum XmlToken<'a> {
    Open(&'a str),
    Close(&'a str),
    Single(&'a str),
    Text(&'a str),
}

fn tag_name(tag: &str) -> &str {
    tag.trim_start_matches(['<', '/']).split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or("")
}

fn xml_tokens(text: &str) -> Option<Vec<XmlToken<'_>>> {
    let mut tokens = Vec::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(body) = rest.strip_prefix("<!--") {
            let end = body.find("-->")? + 4 + 3;
            tokens.push(XmlToken::Single(&rest[..end]));
            rest = &rest[end..];
        } else if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>")? + 3;
            tokens.push(XmlToken::Single(&rest[..end]));
            rest = &rest[end..];
        } else if rest.starts_with('<') {
            let end = rest.find('>')? + 1;
            let tag = &rest[..end];
            if tag.starts_with("</") {
                if stack.pop() != Some(tag_name(tag)) {
                    return None;
                }
                tokens.push(XmlToken::Close(tag));
            } else if tag.starts_with("<?") || tag.starts_with("<!") || tag.ends_with("/>") {
                tokens.push(XmlToken::Single(tag));
            } else {
                stack.push(tag_name(tag));
                tokens.push(XmlToken::Open(tag));
            }
            rest = &rest[end..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let text = rest[..end].trim();
            if !text.is_empty() {
                tokens.push(XmlToken::Text(text));
            }
            rest = &rest[end..];
        }
    }
    (stack.is_empty() && tokens.iter().any(|token| matches!(token, XmlToken::Open(_) | XmlToken::Single(_))))
        .then_some(tokens)
}

pub fn pretty_xml(text: &str) -> Option<(String, Vec<(usize, usize)>)> {
    let tokens = xml_tokens(text.trim())?;
    let mut lines: Vec<String> = Vec::new();
    let mut folds = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut depth = 0usize;
    let mut at = 0;
    while at < tokens.len() {
        let indent = "  ".repeat(depth);
        match tokens[at] {
            XmlToken::Open(tag) => {
                if let (Some(XmlToken::Text(inner)), Some(XmlToken::Close(close))) =
                    (tokens.get(at + 1), tokens.get(at + 2))
                {
                    lines.push(format!("{indent}{tag}{inner}{close}"));
                    at += 3;
                    continue;
                }
                if let Some(XmlToken::Close(close)) = tokens.get(at + 1) {
                    lines.push(format!("{indent}{tag}{close}"));
                    at += 2;
                    continue;
                }
                open.push(lines.len());
                lines.push(format!("{indent}{tag}"));
                depth += 1;
            }
            XmlToken::Close(tag) => {
                depth = depth.saturating_sub(1);
                lines.push(format!("{}{tag}", "  ".repeat(depth)));
                if let Some(start) = open.pop() {
                    folds.push((start, lines.len() - 1));
                }
            }
            XmlToken::Single(tag) => lines.push(format!("{indent}{tag}")),
            XmlToken::Text(inner) => lines.push(format!("{indent}{inner}")),
        }
        at += 1;
    }
    folds.sort_unstable();
    Some((lines.join("\n"), folds))
}

pub fn hex_dump(bytes: &[u8], lines: usize) -> String {
    let mut out = String::new();
    for (index, chunk) in bytes.chunks(16).take(lines).enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!("{:08x} ", index * 16));
        for column in 0..16 {
            if column == 8 {
                out.push(' ');
            }
            match chunk.get(column) {
                Some(byte) => out.push_str(&format!(" {byte:02x}")),
                None => out.push_str("   "),
            }
        }
        out.push_str("  |");
        out.extend(chunk.iter().map(|byte| if byte.is_ascii_graphic() || *byte == b' ' { *byte as char } else { '.' }));
        out.push('|');
    }
    out
}

fn cut(text: String, chars: usize) -> (String, usize, bool) {
    let length = text.chars().count();
    if length <= chars {
        return (text, length, false);
    }
    (text.chars().take(chars).collect(), length, true)
}

fn bytes_of(value: &Value) -> Vec<u8> {
    match value {
        Value::Bytes(bytes) => bytes.clone(),
        other => other.display().unwrap_or_default().into_bytes(),
    }
}

pub fn plain_text(value: &Value) -> String {
    match value {
        Value::Array(items) => array_literal(items),
        Value::Bytes(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(_) => value.display().unwrap_or_default(),
        },
        other => other.display().unwrap_or_default(),
    }
}

pub fn render(value: &Value, format: ValueFormat, chars: usize) -> Option<Rendered> {
    if !formats(value).contains(&format) {
        return None;
    }
    let (whole, folds) = match format {
        ValueFormat::Text => (plain_text(value), Vec::new()),
        ValueFormat::Json => pretty_json(&json_text(value)),
        ValueFormat::Xml => pretty_xml(&plain_text(value))?,
        ValueFormat::Hex => {
            let bytes = bytes_of(value);
            let lines = chars.div_ceil(78).max(1);
            let mut dump = hex_dump(&bytes, lines);
            let length = bytes.len().div_ceil(16) * 78;
            let truncated = bytes.len() > lines * 16;
            if truncated {
                dump.push('\n');
            }
            return Some(Rendered { text: dump, length, truncated, folds: Vec::new() });
        }
        ValueFormat::Image => {
            let bytes = bytes_of(value);
            let kind = image_kind(&bytes).unwrap_or("?");
            (format!("{kind}, {} bytes", bytes.len()), Vec::new())
        }
        ValueFormat::Geometry => {
            let (text, length, truncated) = crate::geometry::from_value(value)?.ewkt(chars);
            return Some(Rendered { text, length, truncated, folds: Vec::new() });
        }
    };
    let (text, length, truncated) = cut(whole, chars);
    let shown_lines = text.lines().count();
    let folds = folds.into_iter().filter(|(_, end)| *end < shown_lines).collect();
    Some(Rendered { text, length, truncated, folds })
}

/// The figures of a selection of cells, as the calc panel and the status bar show them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Calc {
    /// The cells looked at.
    pub cells: u64,
    /// The cells that are not `NULL`.
    pub count: u64,
    /// The numeric cells among them.
    pub numeric: u64,
    /// The sum of the numeric cells.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sum: Option<String>,
    /// Their average.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg: Option<String>,
    /// The smallest value: numerically when there are numbers, else as text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    /// The largest value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    /// How many different values there are, `NULL` not counted.
    pub distinct: u64,
    /// Whether cells were left out: at most the cap is looked at.
    pub truncated: bool,
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(value) => Some(*value as f64),
        Value::Float(value) if value.is_finite() => Some(*value),
        Value::Decimal(text) => text.parse::<f64>().ok().filter(|value| value.is_finite()),
        _ => None,
    }
}

fn decimal_parts(text: &str) -> Option<(i128, u32)> {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole.bytes().chain(fraction.bytes()).all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let scale = u32::try_from(fraction.len()).ok()?;
    let mut mantissa: i128 = 0;
    for byte in whole.bytes().chain(fraction.bytes()) {
        mantissa = mantissa.checked_mul(10)?.checked_add(i128::from(byte - b'0'))?;
    }
    Some((if negative { -mantissa } else { mantissa }, scale))
}

fn add_exact((total, scale): (i128, u32), value: &Value) -> Option<(i128, u32)> {
    let (mantissa, digits) = match value {
        Value::Integer(value) => (i128::from(*value), 0),
        Value::Decimal(text) => decimal_parts(text)?,
        _ => return None,
    };
    let scale_to = scale.max(digits);
    let widen = |value: i128, from: u32| value.checked_mul(10i128.checked_pow(scale_to - from)?);
    Some((widen(total, scale)?.checked_add(widen(mantissa, digits)?)?, scale_to))
}

fn exact_text((total, scale): (i128, u32)) -> String {
    if scale == 0 {
        return total.to_string();
    }
    let digits = total.unsigned_abs().to_string();
    let width = scale as usize + 1;
    let digits = if digits.len() < width { format!("{digits:0>width$}") } else { digits };
    let (whole, fraction) = digits.split_at(digits.len() - scale as usize);
    format!("{}{whole}.{fraction}", if total < 0 { "-" } else { "" })
}

fn figure(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    if value.abs() >= 1e15 || (value != 0.0 && value.abs() < 1e-5) {
        return crate::value::float_text(value);
    }
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub fn calc(values: impl IntoIterator<Item = Value>, cap: usize) -> Calc {
    let mut calc = Calc::default();
    let mut exact: Option<(i128, u32)> = Some((0, 0));
    let mut sum = 0.0f64;
    let mut low: Option<(f64, String)> = None;
    let mut high: Option<(f64, String)> = None;
    let mut low_text: Option<String> = None;
    let mut high_text: Option<String> = None;
    let mut seen: HashSet<String> = HashSet::new();
    for value in values {
        if calc.cells as usize >= cap {
            calc.truncated = true;
            break;
        }
        calc.cells += 1;
        let Some(text) = value.display() else { continue };
        calc.count += 1;
        if let Some(number) = number(&value) {
            calc.numeric += 1;
            sum += number;
            exact = exact.and_then(|total| add_exact(total, &value));
            if low.as_ref().is_none_or(|(known, _)| number < *known) {
                low = Some((number, text.clone()));
            }
            if high.as_ref().is_none_or(|(known, _)| number > *known) {
                high = Some((number, text.clone()));
            }
        } else {
            if low_text.as_ref().is_none_or(|known| text < *known) {
                low_text = Some(text.clone());
            }
            if high_text.as_ref().is_none_or(|known| text > *known) {
                high_text = Some(text.clone());
            }
        }
        seen.insert(text);
    }
    calc.distinct = seen.len() as u64;
    if calc.numeric > 0 {
        calc.sum = Some(match exact {
            Some(total) => exact_text(total),
            None => figure(sum),
        });
        calc.avg = Some(figure(sum / calc.numeric as f64));
        calc.min = low.map(|(_, text)| text);
        calc.max = high.map(|(_, text)| text);
    } else {
        calc.min = low_text;
        calc.max = high_text;
    }
    calc
}

#[cfg(test)]
mod tests;
