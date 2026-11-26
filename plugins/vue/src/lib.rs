use std::cell::{Cell, RefCell};

#[allow(dead_code, reason = "the colour scanner is shared; this plug-in uses part of it")]
#[path = "../../css/src/color.rs"]
mod color;
#[allow(dead_code, unreachable_pub, reason = "the CSS completion is shared; this plug-in uses part of it")]
#[path = "../../css/src/complete.rs"]
mod complete;
#[allow(dead_code, reason = "the CSS data is shared; this plug-in uses part of it")]
#[path = "../../css/src/data.rs"]
mod data;
#[allow(dead_code, unreachable_pub, reason = "the CSS highlighter is shared; this plug-in uses part of it")]
#[path = "../../css/src/highlight.rs"]
mod highlight;
#[allow(dead_code, reason = "the HTML tokenizer is shared; this plug-in uses part of it")]
#[path = "../../html/src/lexer.rs"]
mod lexer;
mod outline;
#[allow(dead_code, unreachable_pub, reason = "the JavaScript analyser is shared; this plug-in uses part of it")]
#[path = "../../javascript/src/script.rs"]
mod script;
mod sfc;
mod spans;
#[allow(dead_code, unreachable_pub, reason = "the CSS structure is shared; this plug-in uses part of it")]
#[path = "../../css/src/structure.rs"]
mod structure;
mod suggest;
#[allow(dead_code, reason = "the CSS tokenizer is shared; this plug-in uses part of it")]
#[path = "../../css/src/token.rs"]
mod token;
#[allow(dead_code, reason = "the CSS parser is shared; this plug-in uses part of it")]
#[path = "../../css/src/tree.rs"]
mod tree;

pub use outline::{Fold, Symbol, fold, outline};
pub use spans::{Kind, Span, highlight};
pub use suggest::{Completion, Item, complete};

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static STYLE: Cell<token::Dialect> = const { Cell::new(token::Dialect::Css) };
}

pub(crate) fn dialect() -> token::Dialect {
    STYLE.get()
}

pub(crate) fn with_style_dialect<T>(content: sfc::Content, read: impl FnOnce() -> T) -> T {
    let dialect = if content == sfc::Content::Scss { token::Dialect::Scss } else { token::Dialect::Css };
    let before = STYLE.replace(dialect);
    let answer = read();
    STYLE.set(before);
    answer
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

fn json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn symbols_json(symbols: &[Symbol], out: &mut String) {
    out.push('[');
    for (index, symbol) in symbols.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        json_string(out, &symbol.name);
        out.push_str(",\"kind\":");
        json_string(out, symbol.kind);
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
        json_string(&mut out, &item.label);
        out.push_str(",\"kind\":");
        json_string(&mut out, item.kind);
        out.push_str(",\"insert\":");
        json_string(&mut out, &item.insert);
        if let Some(detail) = &item.detail {
            out.push_str(",\"detail\":");
            json_string(&mut out, detail);
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

pub fn complete_request(input: &[u8]) -> Vec<u8> {
    let Some((caret, rest)) = input.split_first_chunk::<4>() else { return Vec::new() };
    let Ok(text) = std::str::from_utf8(rest) else { return Vec::new() };
    complete(text, u32::from_le_bytes(*caret) as usize)
        .map_or_else(Vec::new, |completion| completion_json(&completion).into_bytes())
}

pub fn colors_request(input: &[u8]) -> Vec<u8> {
    color::encode(&suggest::colors(text(input)))
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
    call(ptr, len, complete_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_colors(ptr: i32, len: i32) -> i64 {
    call(ptr, len, colors_request)
}

#[cfg(test)]
mod tests;
