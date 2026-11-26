use std::cell::RefCell;

use crate::complete::complete;
use crate::highlight::{Span, highlight};
use crate::json::{completions_json, outline_json};
use crate::structure::{Fold, fold, outline};
use crate::{color, token};

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

pub fn complete_request(input: &[u8]) -> Vec<u8> {
    let Some((caret, text)) = input.split_first_chunk::<4>() else {
        return Vec::new();
    };
    let text = std::str::from_utf8(text).unwrap_or("");
    complete(text, u32::from_le_bytes(*caret) as usize)
        .map_or_else(Vec::new, |completions| completions_json(&completions).into_bytes())
}

pub fn colors_request(input: &[u8]) -> Vec<u8> {
    let text = as_text(input);
    let mut found = Vec::new();
    color::colors(text, 0..text.len(), color::Syntax { scss: token::scss(), in_block: false }, &mut found);
    color::encode(&found)
}

fn with_input(ptr: i32, len: i32, run: impl FnOnce(&[u8]) -> Vec<u8>) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        run(if valid { &input[..len as usize] } else { &[] })
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

fn as_text(input: &[u8]) -> &str {
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
    with_input(ptr, len, |input| encode(&highlight(as_text(input))))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_fold(ptr: i32, len: i32) -> i64 {
    with_input(ptr, len, |input| encode_folds(&fold(as_text(input))))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_outline(ptr: i32, len: i32) -> i64 {
    with_input(ptr, len, |input| outline_json(&outline(as_text(input))).into_bytes())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_colors(ptr: i32, len: i32) -> i64 {
    with_input(ptr, len, colors_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_complete(ptr: i32, len: i32) -> i64 {
    with_input(ptr, len, complete_request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use crate::highlight::Kind;

    #[test]
    fn encodes_records() {
        assert_eq!(encode(&[Span { start: 1, end: 3, kind: Kind::Attribute }]), [1, 0, 0, 0, 3, 0, 0, 0, 9, 0, 0, 0]);
        assert_eq!(encode_folds(&[Fold { start_line: 2, end_line: 258 }]), [2, 0, 0, 0, 2, 1, 0, 0]);
    }

    #[test]
    fn answers_completion_requests() {
        let mut input = 5u32.to_le_bytes().to_vec();
        input.extend_from_slice(b"a { d }");
        let json = String::from_utf8(complete_request(&input)).unwrap_or_default();
        assert!(json.starts_with("{\"from\":4,\"items\":[") && json.contains("\"insert\":\"display: $0;\""), "{json}");
        assert!(complete_request(&[1, 0]).is_empty());
        let mut comment = 4u32.to_le_bytes().to_vec();
        comment.extend_from_slice(b"/* d */");
        assert!(complete_request(&comment).is_empty());
    }

    #[test]
    fn lists_colours() {
        let text = "a { color: #f00; background: rgb(0 0 255) }";
        assert_eq!(
            colors_request(text.as_bytes()),
            [[11u32, 15, 0xff0000ff], [29, 41, 0x0000ffff]]
                .concat()
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<u8>>()
        );
    }

    #[test]
    fn every_named_colour_has_a_value() {
        for name in data::COLOURS.iter().filter(|name| **name != "currentcolor") {
            assert!(color::named(name).is_some(), "{name}");
        }
    }

    #[test]
    fn runs_through_the_exports() {
        let text = b"a {\n  color: red;\n}\n";
        let ptr = zen_alloc(text.len() as i32);
        INPUT.with_borrow_mut(|input| input.copy_from_slice(text));
        let result = zen_fold(ptr, text.len() as i32);
        assert_eq!(result & 0xFFFF_FFFF, 8);
        assert_eq!(zen_abi(), 1);
        let result = zen_outline(ptr + 1, text.len() as i32);
        assert_eq!(result & 0xFFFF_FFFF, 2);
    }
}
