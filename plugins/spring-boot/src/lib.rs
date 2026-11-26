mod initializr;
#[allow(unreachable_pub, dead_code, reason = "the shared reader is public in its own crate")]
#[path = "../../properties/src/properties.rs"]
mod properties;
#[allow(unreachable_pub, reason = "the shared YAML highlighter is public in its own crate")]
#[path = "../../yaml/src/yaml.rs"]
mod yaml;

use std::cell::RefCell;
use std::fmt::Write as _;

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Key>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub error: bool,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inlay {
    pub offset: usize,
    pub text: String,
}

struct Line<'a> {
    start: usize,
    end: usize,
    indent: usize,
    content: &'a str,
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut found = Vec::new();
    let mut start = 0;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let content = line.trim_start_matches(' ');
        found.push(Line { start, end: start + line.len(), indent: line.len() - content.len(), content });
        start += raw.len();
    }
    found
}

fn significant(line: &Line<'_>) -> bool {
    !line.content.trim().is_empty() && !line.content.starts_with('#')
}

pub fn is_yaml(text: &str) -> bool {
    for line in lines(text) {
        let content = line.content.trim_end();
        if content.is_empty() || content.starts_with(['#', '!']) {
            continue;
        }
        if content == "---" || content.starts_with("- ") || content.ends_with(':') || line.indent > 0 {
            return true;
        }
        let equals = content.find('=');
        let colon = content.find(": ");
        match (equals, colon) {
            (Some(equals), Some(colon)) if equals < colon => return false,
            (Some(_), None) => return false,
            _ => {}
        }
    }
    false
}

fn yaml_key(content: &str) -> Option<&str> {
    let content = content.trim_end();
    if content.starts_with(['#', '-', '[', '{', '|', '>', '&', '*', '!', '%']) || content == "..." {
        return None;
    }
    let end = if let Some(quote) = content.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let close = content[1..].find(quote)? + 2;
        content[close..].starts_with(':').then_some(close)?
    } else {
        content
            .match_indices(':')
            .map(|(at, _)| at)
            .find(|at| content[at + 1..].is_empty() || content[at + 1..].starts_with([' ', '\t']))?
    };
    let key = content[..end].trim_end();
    (!key.is_empty() && !key.contains(" #")).then_some(key)
}

pub fn yaml_keys(text: &str) -> Vec<Key> {
    let lines = lines(text);
    let mut stack: Vec<(usize, Key)> = Vec::new();
    let mut roots: Vec<Key> = Vec::new();
    let mut block: Option<usize> = None;
    fn close(stack: &mut Vec<(usize, Key)>, roots: &mut Vec<Key>, indent: Option<usize>) {
        while let Some((at, _)) = stack.last() {
            if indent.is_some_and(|indent| *at < indent) {
                break;
            }
            let Some((_, key)) = stack.pop() else { break };
            match stack.last_mut() {
                Some((_, parent)) => {
                    parent.end = parent.end.max(key.end);
                    parent.children.push(key);
                }
                None => roots.push(key),
            }
        }
    }
    for line in &lines {
        if !significant(line) {
            continue;
        }
        if let Some(column) = block {
            if line.indent > column {
                if let Some((_, key)) = stack.last_mut() {
                    key.end = key.end.max(line.end);
                }
                continue;
            }
            block = None;
        }
        if line.content.trim_end() == "---" {
            close(&mut stack, &mut roots, None);
            continue;
        }
        close(&mut stack, &mut roots, Some(line.indent));
        let Some(name) = yaml_key(line.content) else {
            if let Some((_, key)) = stack.last_mut() {
                key.end = key.end.max(line.end);
            }
            continue;
        };
        let rest = line.content[name.len()..].trim_start_matches([':', ' ']).trim_end();
        if rest.starts_with(['|', '>']) {
            block = Some(line.indent);
        }
        let start = line.start + line.indent;
        let key = Key { name: name.trim_matches(['"', '\'']).to_owned(), start, end: line.end, children: Vec::new() };
        stack.push((line.indent, key));
    }
    close(&mut stack, &mut roots, None);
    roots
}

pub fn highlight(text: &str) -> Vec<Span> {
    if is_yaml(text) {
        yaml::highlight(text)
            .into_iter()
            .map(|span| Span { start: span.start, end: span.end, kind: span.kind as u32 })
            .collect()
    } else {
        properties::highlight(text)
            .into_iter()
            .map(|span| Span { start: span.start, end: span.end, kind: span.kind as u32 })
            .collect()
    }
}

fn line_of(text: &str, offset: usize) -> u32 {
    text.as_bytes()[..offset.min(text.len())].iter().filter(|byte| **byte == b'\n').count() as u32
}

pub fn fold(text: &str) -> Vec<Fold> {
    if !is_yaml(text) {
        return properties::fold(text)
            .into_iter()
            .map(|fold| Fold { start_line: fold.start_line, end_line: fold.end_line })
            .collect();
    }
    fn walk(text: &str, keys: &[Key], folds: &mut Vec<Fold>) {
        for key in keys {
            let (start_line, end_line) = (line_of(text, key.start), line_of(text, key.end));
            if end_line > start_line {
                folds.push(Fold { start_line, end_line });
            }
            walk(text, &key.children, folds);
        }
    }
    let mut folds = Vec::new();
    walk(text, &yaml_keys(text), &mut folds);
    folds.sort_by_key(|fold| fold.start_line);
    folds
}

pub fn outline(text: &str) -> Vec<Key> {
    if is_yaml(text) {
        return yaml_keys(text);
    }
    properties::entries(text)
        .into_iter()
        .map(|entry| Key { name: entry.key, start: entry.key_start, end: entry.end, children: Vec::new() })
        .collect()
}

pub fn problems(text: &str) -> Vec<Problem> {
    if is_yaml(text) {
        return Vec::new();
    }
    properties::problems(text)
        .into_iter()
        .map(|problem| Problem {
            start: problem.start,
            end: problem.end,
            error: problem.severity == properties::Severity::Error,
            message: problem.message,
        })
        .collect()
}

pub fn inlays(text: &str) -> Vec<Inlay> {
    if is_yaml(text) {
        return Vec::new();
    }
    properties::inlays(text).into_iter().map(|inlay| Inlay { offset: inlay.offset, text: inlay.text }).collect()
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn outline_json(keys: &[Key]) -> String {
    fn write_keys(out: &mut String, keys: &[Key], depth: usize, count: &mut usize) {
        out.push('[');
        for (index, key) in keys.iter().enumerate() {
            if *count >= 20_000 {
                break;
            }
            *count += 1;
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            json_string(out, if key.name.is_empty() { " " } else { &key.name });
            let _ = write!(out, ",\"kind\":\"property\",\"start\":{},\"end\":{},\"children\":", key.start, key.end);
            if depth < 32 {
                write_keys(out, &key.children, depth + 1, count);
            } else {
                out.push_str("[]");
            }
            out.push('}');
        }
        out.push(']');
    }
    let mut out = String::new();
    write_keys(&mut out, keys, 0, &mut 0);
    out
}

pub fn problems_json(problems: &[Problem]) -> String {
    let mut out = String::from("[");
    for (index, problem) in problems.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let severity = if problem.error { "error" } else { "warning" };
        let _ = write!(
            out,
            "{{\"start\":{},\"end\":{},\"severity\":\"{severity}\",\"message\":",
            problem.start, problem.end
        );
        json_string(&mut out, &problem.message);
        out.push('}');
    }
    out.push(']');
    out
}

pub fn inlays_json(inlays: &[Inlay]) -> String {
    let mut out = String::from("[");
    for (index, inlay) in inlays.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"offset\":{},\"text\":", inlay.offset);
        json_string(&mut out, &inlay.text);
        out.push('}');
    }
    out.push(']');
    out
}

pub fn encode_spans(spans: &[Span]) -> Vec<u8> {
    let mut out = Vec::with_capacity(spans.len() * 12);
    for span in spans {
        for value in [span.start as u32, span.end as u32, span.kind] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

pub fn encode_folds(folds: &[Fold]) -> Vec<u8> {
    let mut out = Vec::with_capacity(folds.len() * 8);
    for fold in folds {
        out.extend_from_slice(&fold.start_line.to_le_bytes());
        out.extend_from_slice(&fold.end_line.to_le_bytes());
    }
    out
}

fn call(ptr: i32, len: i32, run: impl FnOnce(&[u8]) -> Vec<u8>) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        run(if valid { &input[..len as usize] } else { &[] })
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

fn text(input: &[u8]) -> &str {
    std::str::from_utf8(input).unwrap_or("")
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_abi() -> i32 {
    1
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_alloc(len: i32) -> i32 {
    INPUT.with_borrow_mut(|input| {
        input.clear();
        input.resize(usize::try_from(len).unwrap_or(0), 0);
        input.as_mut_ptr() as usize as i32
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_project(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| initializr::answer(text(input)).into_bytes())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_highlight(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| encode_spans(&highlight(text(input))))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_fold(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| encode_folds(&fold(text(input))))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_outline(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| outline_json(&outline(text(input))).into_bytes())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_problems(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| problems_json(&problems(text(input))).into_bytes())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_inlays(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| inlays_json(&inlays(text(input))).into_bytes())
}

#[cfg(test)]
mod tests;
