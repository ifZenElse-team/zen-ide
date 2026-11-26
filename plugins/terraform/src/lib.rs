use std::cell::RefCell;
use std::fmt::Write as _;

mod hcl;

pub use hcl::{Entry, Fold, Kind, Span, fold, highlight, outline};

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn encode(spans: &[Span]) -> Vec<u8> {
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

fn entries_json(out: &mut String, entries: &[Entry]) {
    out.push('[');
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        json_string(out, &entry.name);
        let _ =
            write!(out, ",\"kind\":\"{}\",\"start\":{},\"end\":{},\"children\":", entry.kind, entry.start, entry.end);
        entries_json(out, &entry.children);
        out.push('}');
    }
    out.push(']');
}

pub fn outline_json(entries: &[Entry]) -> String {
    let mut out = String::new();
    entries_json(&mut out, entries);
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
    call(ptr, len, |input| encode(&highlight(text(input))))
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

const COMMANDS: [&str; 4] = ["init", "validate", "plan", "apply"];

pub fn run_targets_request(input: &[u8]) -> Vec<u8> {
    let Some(split) = input.iter().position(|byte| *byte == 0) else { return Vec::new() };
    let path = std::str::from_utf8(&input[..split]).unwrap_or("");
    let text = std::str::from_utf8(&input[split + 1..]).unwrap_or("");
    if !path.ends_with(".tf") {
        return Vec::new();
    }
    let block = outline(text).into_iter().find(|entry| entry.name == "terraform");
    let main = path.rsplit('/').next() == Some("main.tf");
    if block.is_none() && !main {
        return Vec::new();
    }
    let line = block.map_or(0, |entry| text[..entry.start].matches('\n').count());
    let mut out = String::from("[");
    for (index, command) in COMMANDS.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"line\":{line},\"name\":\"terraform {command}\",\"id\":\"{command}\",\"program\":\"terraform\",\
             \"cwd\":\"${{fileDir}}\",\"kind\":\"main\",\"args\":[\"{command}\"]}}"
        );
    }
    out.push(']');
    out.into_bytes()
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_run_targets(ptr: i32, len: i32) -> i64 {
    call(ptr, len, run_targets_request)
}

#[cfg(test)]
mod tests;
