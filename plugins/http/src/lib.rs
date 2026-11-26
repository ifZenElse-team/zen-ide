use std::cell::RefCell;
use std::fmt::Write;

mod scan;

pub use scan::{Fold, Kind, Span, Symbol, fold, highlight, outline};

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

pub fn outline_json(symbols: &[Symbol]) -> String {
    let mut out = String::from("[");
    for (index, symbol) in symbols.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        string(&mut out, &symbol.name);
        out.push_str(",\"kind\":");
        string(&mut out, symbol.kind);
        let _ = write!(out, ",\"start\":{},\"end\":{},\"children\":[]}}", symbol.start, symbol.end);
    }
    out.push(']');
    out
}

fn string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
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

fn with_input(ptr: i32, len: i32, run: impl FnOnce(&str) -> Vec<u8>) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        let bytes = if valid { &input[..len as usize] } else { &[] };
        run(std::str::from_utf8(bytes).unwrap_or(""))
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
    with_input(ptr, len, |text| encode(&highlight(text)))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_fold(ptr: i32, len: i32) -> i64 {
    with_input(ptr, len, |text| encode_folds(&fold(text)))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_outline(ptr: i32, len: i32) -> i64 {
    with_input(ptr, len, |text| outline_json(&outline(text)).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exports_read_what_was_written() {
        let text = b"GET http://h/\nAccept: */*\n";
        let ptr = zen_alloc(text.len() as i32);
        INPUT.with_borrow_mut(|input| input.copy_from_slice(text));
        assert_eq!(zen_abi(), 1);
        assert_eq!(zen_highlight(ptr, text.len() as i32) & 0xFFFF_FFFF, 4 * 12);
        assert_eq!(zen_fold(ptr, text.len() as i32) & 0xFFFF_FFFF, 8);
        let result = zen_outline(ptr, text.len() as i32);
        let json = OUTPUT.with_borrow(|output| String::from_utf8(output.clone()).unwrap());
        assert_eq!(result & 0xFFFF_FFFF, json.len() as i64);
        assert_eq!(json, r#"[{"name":"GET http://h/","kind":"method","start":0,"end":25,"children":[]}]"#);
        assert_eq!(zen_outline(ptr + 1, text.len() as i32) & 0xFFFF_FFFF, 2, "a wrong pointer reads nothing");
    }

    #[test]
    fn names_are_escaped() {
        let symbols = [Symbol { name: "a \"b\"\\\u{1}".into(), kind: "method", start: 0, end: 1 }];
        assert_eq!(
            outline_json(&symbols),
            r#"[{"name":"a \"b\"\\\u0001","kind":"method","start":0,"end":1,"children":[]}]"#
        );
    }
}
