use std::cell::RefCell;
use std::fmt::Write as _;

pub mod convert;
mod kotlin;
#[path = "../../java/src/new_project.rs"]
pub mod new_project;

pub use kotlin::{Fold, JvmFile, Kind, Span, Symbol, fold, highlight, jvm, outline};

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
                    ",\"kind\":\"{}\",\"start\":{},\"end\":{},\"children\":",
                    symbol.kind, symbol.start, symbol.end
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

pub fn jvm_json(file: &JvmFile) -> String {
    let mut out = String::from("{\"package\":");
    json_string(&mut out, &file.package);
    out.push_str(",\"fileClass\":");
    let file_class = match (file.jvm_name.is_empty(), file.package.is_empty()) {
        (true, _) => String::new(),
        (false, true) => file.jvm_name.clone(),
        (false, false) => format!("{}.{}", file.package, file.jvm_name),
    };
    json_string(&mut out, &file_class);
    out.push_str(",\"classes\":[");
    for (index, class) in file.classes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        json_string(&mut out, &class.name);
        let _ = write!(out, ",\"start\":{},\"end\":{}}}", class.start, class.end);
    }
    out.push_str("],\"mains\":[");
    for (index, main) in file.mains.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"class\":");
        json_string(&mut out, &main.class);
        if let Some(owner) = main.owner {
            let _ = write!(out, ",\"owner\":{owner}");
        }
        let _ = write!(out, ",\"start\":{},\"end\":{}}}", main.start, main.end);
    }
    out.push_str("],\"tests\":[");
    for (index, test) in file.tests.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        json_string(&mut out, &test.name);
        let _ = write!(out, ",\"start\":{},\"testng\":{},\"methods\":[", test.start, test.testng);
        for (at, (name, start)) in test.methods.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            json_string(&mut out, name);
            let _ = write!(out, ",\"start\":{start}}}");
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}

pub fn conversion_json(conversion: &convert::Conversion) -> String {
    let mut out = String::from("{\"text\":");
    json_string(&mut out, &conversion.text);
    out.push_str(",\"problems\":[");
    for (index, problem) in conversion.problems.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"start\":{},\"end\":{},\"message\":", problem.start, problem.end);
        json_string(&mut out, &problem.message);
        out.push('}');
    }
    out.push_str("]}");
    out
}

pub fn convert_input(input: &[u8]) -> Vec<u8> {
    let Some(length) =
        input.get(..4).map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
    else {
        return Vec::new();
    };
    let Some(from) = input.get(4..4 + length).and_then(|bytes| std::str::from_utf8(bytes).ok()) else {
        return Vec::new();
    };
    let Ok(text) = std::str::from_utf8(&input[4 + length..]) else { return Vec::new() };
    convert::convert(from, text).map(|conversion| conversion_json(&conversion).into_bytes()).unwrap_or_default()
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
pub extern "C" fn zen_jvm(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| jvm_json(&jvm(text(input))).into_bytes())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_project(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| new_project::answer(new_project::Language::Kotlin, input))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_convert(ptr: i32, len: i32) -> i64 {
    call(ptr, len, convert_input)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_convert_from(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |_| convert::SOURCES.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exports_encode_what_the_lexer_finds() {
        let text = "class A {\n    fun b() = 1\n}\n";
        let spans = encode_spans(&highlight(text));
        assert_eq!(spans.len() % 12, 0);
        assert_eq!(&spans[..12], [0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0].as_slice(), "class, a keyword");
        assert_eq!(encode_folds(&fold(text)), [0, 0, 0, 0, 2, 0, 0, 0]);
        let json: serde_json::Value = serde_json::from_str(&outline_json(&outline(text))).unwrap();
        assert_eq!(json[0]["name"], "A");
        assert_eq!(json[0]["kind"], "class");
        assert_eq!(json[0]["children"][0]["kind"], "method");
    }

    #[test]
    fn the_convert_export_answers_kotlin_and_its_problems_as_json() {
        let mut input = 4u32.to_le_bytes().to_vec();
        input.extend_from_slice(b"java");
        input.extend_from_slice(b"public class A { int x = ; }");
        let json: serde_json::Value = serde_json::from_slice(&convert_input(&input)).unwrap();
        assert!(json["text"].as_str().unwrap().starts_with("class A"), "{json}");
        assert_eq!(json["problems"].as_array().unwrap().len(), 1, "{json}");
        let mut other = 5u32.to_le_bytes().to_vec();
        other.extend_from_slice(b"scala");
        other.extend_from_slice(b"class A");
        assert!(convert_input(&other).is_empty(), "nothing for a language it does not read");
        assert!(convert_input(&[1, 0]).is_empty());
    }

    #[test]
    fn the_jvm_export_answers_json() {
        let text = "package a.b\n\nfun main() {}\n\nclass T {\n    @Test fun t() {}\n}\n";
        let json: serde_json::Value = serde_json::from_str(&jvm_json(&jvm(text))).unwrap();
        assert_eq!(json["package"], "a.b");
        assert_eq!(json["fileClass"], "");
        assert_eq!(json["mains"][0]["class"], "");
        assert_eq!(json["classes"][0]["name"], "a.b.T");
        assert_eq!(json["tests"][0]["methods"][0]["name"], "t");
        let named: serde_json::Value =
            serde_json::from_str(&jvm_json(&jvm("@file:JvmName(\"Run\")\npackage a\n"))).unwrap();
        assert_eq!(named["fileClass"], "a.Run");
    }

    #[test]
    fn the_manifest_declares_the_files_both_servers_and_the_templates() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
        assert_eq!(manifest["extensions"], serde_json::json!(["kt", "kts"]));
        let servers = manifest["languageServers"].as_array().unwrap();
        let ids: Vec<&str> = servers.iter().map(|server| server["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["kotlin-lsp", "kotlin-language-server"], "the vendor's first: the default");
        let vendor = &servers[0];
        assert_eq!(vendor["install"]["kind"], "archive");
        for platform in ["macos-aarch64", "macos-x86_64", "linux-x86_64", "linux-aarch64"] {
            let archive = &vendor["install"]["platforms"][platform];
            assert_eq!(archive["sha256"].as_str().map(str::len), Some(64), "{platform}");
            assert!(archive["url"].as_str().unwrap().starts_with("https://download.jetbrains.com/"), "{platform}");
        }
        assert!(vendor["notice"]["text"]["en"].as_str().unwrap().contains("JetBrains"));
        assert!(vendor["notice"]["text"]["ru"].as_str().unwrap().contains("JetBrains"));
        assert_eq!(vendor["notice"]["url"], "https://github.com/Kotlin/kotlin-lsp");
        let open = &servers[1];
        assert!(
            open["install"]["url"].as_str().unwrap().starts_with("https://github.com/fwcd/kotlin-language-server/")
        );
        assert_eq!(open["install"]["sha256"].as_str().map(str::len), Some(64));
        assert!(manifest["fileTemplates"]["templates"].as_array().unwrap().len() >= 5);
    }

    #[test]
    fn the_manifest_declares_test_naming_rules_and_a_template_per_framework() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
        let tests = manifest["tests"].as_array().unwrap();
        assert_eq!(tests.len(), 1);
        let declared = &tests[0];
        let pairs: Vec<(&str, &str)> = declared["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| (pair["subject"].as_str().unwrap(), pair["test"].as_str().unwrap()))
            .collect();
        for (main, test) in [("src/main/kotlin", "src/test/kotlin"), ("src/main/java", "src/test/java")] {
            for name in ["{name}Test", "{name}Tests", "{name}IT", "Test{name}"] {
                let subject = format!("{main}/{{path}}{{name}}.kt");
                let paired = format!("{test}/{{path}}{name}.kt");
                assert!(pairs.contains(&(subject.as_str(), paired.as_str())), "{paired}");
            }
        }
        assert!(declared["members"].as_str().unwrap().contains("(?P<name>"));
        let frameworks = declared["frameworks"].as_array().unwrap();
        let ids: Vec<&str> = frameworks.iter().map(|framework| framework["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["junit5", "junit4", "testng", "kotlin.test"]);
        for framework in frameworks {
            let file = framework["file"].as_str().unwrap();
            for part in ["${PACKAGE_LINE}", "class ${NAME}", "setUp()", "tearDown()", "${METHODS}"] {
                assert!(file.contains(part), "{}: {part}", framework["id"]);
            }
            assert!(framework["method"].as_str().unwrap().contains("@Test"), "{}", framework["id"]);
            assert!(!framework["detect"].as_array().unwrap().is_empty(), "{}", framework["id"]);
        }
    }
}
