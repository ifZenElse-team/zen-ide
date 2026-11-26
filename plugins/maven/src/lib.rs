use std::cell::RefCell;
use std::fmt::Write as _;

#[allow(dead_code, reason = "the HTML tokenizer is shared; this plug-in uses part of it")]
#[path = "../../html/src/lexer.rs"]
mod lexer;
pub mod new_project;
mod pom;
#[allow(dead_code, reason = "the XML plug-in's code is shared; this plug-in uses part of it")]
#[path = "../../xml/src/xml.rs"]
mod xml;

pub use pom::{Completions, Item, complete};
pub use xml::{Fold, Problem, Span, Symbol, TRIGGERS, fold, highlight, on_type, outline, problems};

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
    fn write(out: &mut String, symbols: &[Symbol]) {
        out.push('[');
        for (index, symbol) in symbols.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            json_string(out, &symbol.name);
            let _ =
                write!(out, ",\"kind\":\"element\",\"start\":{},\"end\":{},\"children\":", symbol.start, symbol.end);
            write(out, &symbol.children);
            out.push('}');
        }
        out.push(']');
    }
    let mut out = String::new();
    write(&mut out, symbols);
    out
}

pub fn problems_json(problems: &[Problem]) -> String {
    let mut out = String::from("[");
    for (index, problem) in problems.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"start\":{},\"end\":{},\"severity\":\"error\",\"message\":",
            problem.range.start, problem.range.end
        );
        json_string(&mut out, &problem.message);
        out.push('}');
    }
    out.push(']');
    out
}

pub fn completions_json(completions: &Completions) -> String {
    let mut out = format!("{{\"from\":{},\"items\":[", completions.from);
    for (index, item) in completions.items.iter().take(2000).enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        json_string(&mut out, &item.label);
        let _ = write!(out, ",\"kind\":\"{}\",\"insert\":", item.kind);
        json_string(&mut out, &item.insert);
        out.push_str(",\"detail\":");
        json_string(&mut out, &item.detail);
        out.push('}');
    }
    out.push_str("]}");
    out
}

pub fn complete_request(input: &[u8]) -> Vec<u8> {
    let Some((caret, text)) = input.split_first_chunk::<4>() else { return Vec::new() };
    let Ok(text) = std::str::from_utf8(text) else { return Vec::new() };
    complete(text, u32::from_le_bytes(*caret) as usize)
        .filter(|found| !found.items.is_empty())
        .map_or_else(Vec::new, |found| completions_json(&found).into_bytes())
}

pub fn on_type_request(input: &[u8]) -> Vec<u8> {
    let Some((caret, rest)) = input.split_first_chunk::<4>() else { return Vec::new() };
    let Some((typed, text)) = rest.split_first_chunk::<4>() else { return Vec::new() };
    let (Some(typed), Ok(text)) = (char::from_u32(u32::from_le_bytes(*typed)), std::str::from_utf8(text)) else {
        return Vec::new();
    };
    on_type(text, u32::from_le_bytes(*caret) as usize, typed).map_or_else(Vec::new, |added| {
        let mut out = format!("{{\"from\":{},\"to\":{},\"text\":", added.at, added.at);
        json_string(&mut out, &added.text);
        out.push('}');
        out.into_bytes()
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_project(ptr: i32, len: i32) -> i64 {
    call(ptr, len, new_project::answer)
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
pub extern "C" fn zen_on_type_triggers(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |_| TRIGGERS.as_bytes().to_vec())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_on_type(ptr: i32, len: i32) -> i64 {
    call(ptr, len, on_type_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_problems(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| problems_json(&problems(text(input))).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pom_keeps_what_xml_gives_it_and_completes() {
        let text = "<project>\n  <dependencies>\n    <dependency><scope></scope></dependency>\n  </dependencies>\n</project>\n";
        assert!(!highlight(text).is_empty());
        assert_eq!(encode_folds(&fold(text)).len(), 16, "the project and the dependencies fold");
        assert_eq!(outline(text)[0].name, "project");
        assert!(problems("<project><a></project>").len() == 1);
        let caret = text.find("<scope>").unwrap() + "<scope>".len();
        let mut input = (caret as u32).to_le_bytes().to_vec();
        input.extend_from_slice(text.as_bytes());
        let json: serde_json::Value = serde_json::from_slice(&complete_request(&input)).unwrap();
        assert_eq!(json["from"], caret);
        assert_eq!(json["items"][0]["label"], "compile");
        assert_eq!(json["items"][0]["kind"], "value");
        let typed = text.find("<scope>").unwrap() + "<scope>".len();
        let mut on = (typed as u32).to_le_bytes().to_vec();
        on.extend_from_slice(&('>' as u32).to_le_bytes());
        on.extend_from_slice("<project><scope>".as_bytes());
        let _ = on_type_request(&on);
    }

    #[test]
    fn the_manifest_takes_pom_xml_by_name_for_maven_projects() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
        assert_eq!(manifest["names"], serde_json::json!(["pom.xml"]));
        assert_eq!(manifest["project"]["markers"]["files"], serde_json::json!(["pom.xml"]));
    }
}
