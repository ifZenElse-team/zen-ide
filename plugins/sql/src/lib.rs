use std::cell::{Cell, RefCell};
use std::fmt::Write as _;

pub mod complete;
pub mod dialect;
pub mod fold;
pub mod highlight;
pub mod lexer;
pub mod outline;
pub mod problems;
pub mod script;
pub mod words;

pub use complete::{Completion, Suggestion, complete};
pub use dialect::Dialect;
pub use fold::{Fold, fold};
pub use highlight::{Kind, Span, highlight};
pub use outline::{Symbol, outline};
pub use problems::{Problem, problems};

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static DIALECT: Cell<Dialect> = const { Cell::new(Dialect::Generic) };
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

pub fn outline_json(symbols: &[Symbol]) -> String {
    fn write(out: &mut String, symbols: &[Symbol]) {
        out.push('[');
        for (index, symbol) in symbols.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            json_string(out, &symbol.name);
            let _ = write!(
                out,
                ",\"kind\":\"{}\",\"start\":{},\"end\":{},\"children\":",
                symbol.kind, symbol.start, symbol.end
            );
            write(out, &symbol.children);
            out.push('}');
        }
        out.push(']');
    }
    let mut out = String::new();
    write(&mut out, symbols);
    out
}

pub fn completion_json(completion: &Completion) -> String {
    let mut out = format!("{{\"from\":{},\"items\":[", completion.from);
    for (index, item) in completion.items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        json_string(&mut out, &item.label);
        let _ = write!(out, ",\"kind\":\"{}\",\"insert\":", item.kind);
        json_string(&mut out, &item.insert);
        out.push('}');
    }
    out.push_str("]}");
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

pub fn current_dialect() -> Dialect {
    DIALECT.get()
}

pub fn dialect_request(input: &[u8]) -> Vec<u8> {
    DIALECT.set(Dialect::from_name(std::str::from_utf8(input).unwrap_or("")));
    Vec::new()
}

pub fn fold_request(input: &[u8]) -> Vec<u8> {
    encode_folds(&fold(text(input), current_dialect()))
}

pub fn outline_request(input: &[u8]) -> Vec<u8> {
    outline_json(&outline(text(input), current_dialect())).into_bytes()
}

pub fn complete_request(input: &[u8]) -> Vec<u8> {
    let Some((caret, rest)) = input.split_first_chunk::<4>() else { return Vec::new() };
    let Ok(text) = std::str::from_utf8(rest) else { return Vec::new() };
    complete(text, u32::from_le_bytes(*caret) as usize, current_dialect())
        .map_or_else(Vec::new, |found| completion_json(&found).into_bytes())
}

pub fn problems_request(input: &[u8]) -> Vec<u8> {
    problems_json(&problems(text(input), current_dialect())).into_bytes()
}

fn text(input: &[u8]) -> &str {
    std::str::from_utf8(input).unwrap_or("")
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
pub extern "C" fn zen_dialect(ptr: i32, len: i32) -> i64 {
    call(ptr, len, dialect_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_fold(ptr: i32, len: i32) -> i64 {
    call(ptr, len, fold_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_outline(ptr: i32, len: i32) -> i64 {
    call(ptr, len, outline_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_complete(ptr: i32, len: i32) -> i64 {
    call(ptr, len, complete_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_problems(ptr: i32, len: i32) -> i64 {
    call(ptr, len, problems_request)
}

#[cfg(test)]
mod tests;
