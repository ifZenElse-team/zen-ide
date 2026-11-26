use std::cell::RefCell;
use std::fmt::Write as _;

mod properties;

pub use properties::{
    Completions, Document, Entry, Fold, Inlay, Item, Kind, Problem, Severity, Span, complete_keys, entries, escape_key,
    escape_value, fold, highlight, inlays, key_prefix_at, parse, problems, shown_value,
};

pub const KEY_REFERENCES: &str = r#"[{"files":["*.java","*.kt","*.kts","*.groovy","*.scala"],"keys":["getString","getObject","getStringArray","containsKey","getMessage"],"bundles":["getBundle"]}]"#;

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn encode_spans(spans: &[Span]) -> Vec<u8> {
    let mut out = Vec::with_capacity(spans.len() * 12);
    for span in spans {
        for value in [span.start as u32, span.end as u32, span.kind as u32] {
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

pub fn outline_json(entries: &[Entry]) -> String {
    let mut out = String::from("[");
    for (index, entry) in entries.iter().take(20_000).enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        json_string(&mut out, if entry.key.is_empty() { " " } else { &entry.key });
        let _ =
            write!(out, ",\"kind\":\"property\",\"start\":{},\"end\":{},\"children\":[]}}", entry.key_start, entry.end);
    }
    out.push(']');
    out
}

pub fn problems_json(problems: &[Problem]) -> String {
    let mut out = String::from("[");
    for (index, problem) in problems.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let severity = match problem.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
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
    call(ptr, len, |input| outline_json(&entries(text(input))).into_bytes())
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

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_key_references(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |_| KEY_REFERENCES.as_bytes().to_vec())
}

#[cfg(test)]
mod tests;
