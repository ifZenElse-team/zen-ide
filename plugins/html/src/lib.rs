#[path = "../../css/src/color.rs"]
mod color;
mod complete;
mod data;
mod emmet;
mod highlight;
mod json;
mod lexer;
mod styles;
mod tree;
mod typing;

use std::cell::RefCell;

pub use complete::{Completion, Item, complete};
pub use emmet::{Expansion, expand, expand_abbreviation};
pub use highlight::{Kind, Span, highlight};
pub use tree::{Fold, Symbol, fold, outline};
pub use typing::{TRIGGERS, on_type};

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

fn symbols_json(symbols: &[Symbol], out: &mut String) {
    out.push('[');
    for (index, symbol) in symbols.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        json::string(out, &symbol.name);
        out.push_str(",\"kind\":");
        json::string(out, symbol.kind);
        out.push_str(&format!(",\"start\":{},\"end\":{},\"children\":", symbol.start, symbol.end));
        symbols_json(&symbol.children, out);
        out.push('}');
    }
    out.push(']');
}

pub fn outline_json(symbols: &[Symbol]) -> String {
    let mut out = String::new();
    symbols_json(symbols, &mut out);
    out
}

pub fn completion_json(completion: &Completion) -> String {
    let mut out = format!("{{\"from\":{},\"items\":[", completion.from);
    for (index, item) in completion.items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        json::string(&mut out, &item.label);
        out.push_str(",\"kind\":");
        json::string(&mut out, item.kind);
        out.push_str(",\"insert\":");
        json::string(&mut out, &item.insert);
        out.push('}');
    }
    out.push_str("]}");
    out
}

pub fn expansion_json(expansion: &Expansion) -> String {
    let mut out = format!("{{\"from\":{},\"to\":{},\"text\":", expansion.from, expansion.to);
    json::string(&mut out, &expansion.text);
    out.push('}');
    out
}

pub fn colors_request(input: &[u8]) -> Vec<u8> {
    color::encode(&styles::colors(text(input)))
}

pub fn on_type_request(input: &[u8]) -> Vec<u8> {
    let Some((caret, rest)) = input.split_first_chunk::<4>() else { return Vec::new() };
    let Some((typed, page)) = rest.split_first_chunk::<4>() else { return Vec::new() };
    let (Some(typed), Ok(page)) = (char::from_u32(u32::from_le_bytes(*typed)), std::str::from_utf8(page)) else {
        return Vec::new();
    };
    on_type(page, u32::from_le_bytes(*caret) as usize, typed)
        .map_or_else(Vec::new, |added| expansion_json(&added).into_bytes())
}

pub fn split_caret_input(input: &[u8]) -> Option<(usize, &str)> {
    let caret = u32::from_le_bytes(input.get(..4)?.try_into().ok()?) as usize;
    Some((caret, std::str::from_utf8(&input[4..]).ok()?))
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
    call(ptr, len, |input| outline_json(&outline(text(input))).into_bytes())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_complete(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| {
        split_caret_input(input)
            .and_then(|(caret, text)| complete(text, caret))
            .map_or_else(Vec::new, |completion| completion_json(&completion).into_bytes())
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_expand(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| {
        split_caret_input(input)
            .and_then(|(caret, text)| expand(text, caret))
            .map_or_else(Vec::new, |expansion| expansion_json(&expansion).into_bytes())
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_colors(ptr: i32, len: i32) -> i64 {
    call(ptr, len, colors_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_on_type_triggers(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |_| TRIGGERS.as_bytes().to_vec())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_on_type(ptr: i32, len: i32) -> i64 {
    call(ptr, len, on_type_request)
}

#[cfg(test)]
mod tests;
