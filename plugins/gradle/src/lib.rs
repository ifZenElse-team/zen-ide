use std::cell::RefCell;
use std::fmt::Write as _;

mod groovy;
pub mod modules;

pub use groovy::{Fold, Kind, Span, Symbol, fold, highlight, outline};

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

pub fn outline_json(symbols: &[Symbol]) -> String {
    fn write(out: &mut String, symbols: &[Symbol], depth: usize) {
        out.push('[');
        if depth < 32 {
            for (index, symbol) in symbols.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str("{\"name\":");
                json_string(out, &symbol.name);
                let _ = write!(
                    out,
                    ",\"kind\":\"function\",\"start\":{},\"end\":{},\"children\":",
                    symbol.start, symbol.end
                );
                write(out, &symbol.children, depth + 1);
                out.push('}');
            }
        }
        out.push(']');
    }
    let mut out = String::new();
    write(&mut out, symbols, 0);
    out
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_project(ptr: i32, len: i32) -> i64 {
    call(ptr, len, modules::answer)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exports_encode_what_the_lexer_finds() {
        let text = "plugins {\n    id 'java'\n}\n";
        let spans = encode_spans(&highlight(text));
        assert_eq!(spans.len() % 12, 0);
        assert_eq!(&spans[..12], [0, 0, 0, 0, 7, 0, 0, 0, 7, 0, 0, 0].as_slice(), "plugins, a function");
        assert_eq!(encode_folds(&fold(text)), [0, 0, 0, 0, 2, 0, 0, 0]);
        let json: serde_json::Value = serde_json::from_str(&outline_json(&outline(text))).unwrap();
        assert_eq!(json[0]["name"], "plugins");
        assert_eq!(json[0]["kind"], "function");
    }

    #[test]
    fn the_manifest_offers_it_for_gradle_builds() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
        assert_eq!(manifest["extensions"], serde_json::json!(["gradle"]));
        assert!(
            manifest["project"]["markers"]["files"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("build.gradle.kts"))
        );
    }
}
