mod script;

use std::cell::RefCell;

pub use script::{
    Completion, CompletionItem, Dialect, Fold, JSX_TRIGGERS, Kind, Span, Symbol, SymbolKind, complete, fold, highlight,
    jsx_on_type, outline, outline_json,
};

const DIALECT: Dialect = Dialect::JavaScript;

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

pub fn complete_request(input: &[u8]) -> Vec<u8> {
    let Some((caret, text)) = input.split_first_chunk::<4>() else { return Vec::new() };
    let text = std::str::from_utf8(text).unwrap_or("");
    complete(text, u32::from_le_bytes(*caret) as usize, DIALECT).to_json().into_bytes()
}

pub fn on_type_request(input: &[u8]) -> Vec<u8> {
    let Some((caret, rest)) = input.split_first_chunk::<4>() else { return Vec::new() };
    let Some((typed, text)) = rest.split_first_chunk::<4>() else { return Vec::new() };
    let (Some(typed), Ok(text)) = (char::from_u32(u32::from_le_bytes(*typed)), std::str::from_utf8(text)) else {
        return Vec::new();
    };
    jsx_on_type(text, u32::from_le_bytes(*caret) as usize, typed, DIALECT).map_or_else(Vec::new, |(from, to, added)| {
        let added = added.replace('\\', "\\\\").replace('"', "\\\"");
        format!(r#"{{"from":{from},"to":{to},"text":"{added}"}}"#).into_bytes()
    })
}

fn call(ptr: i32, len: i32, answer: impl FnOnce(&[u8]) -> Vec<u8>) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        answer(if valid { &input[..len as usize] } else { &[] })
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
    call(ptr, len, |input| encode_spans(&highlight(text(input), DIALECT)))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_fold(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| encode_folds(&fold(text(input), DIALECT)))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_outline(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| outline_json(&outline(text(input), DIALECT)).into_bytes())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_complete(ptr: i32, len: i32) -> i64 {
    call(ptr, len, complete_request)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_on_type_triggers(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |_| JSX_TRIGGERS.as_bytes().to_vec())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_on_type(ptr: i32, len: i32) -> i64 {
    call(ptr, len, on_type_request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end_tag(marked: &str) -> Option<String> {
        let caret = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        jsx_on_type(&text, caret, '>', DIALECT).map(|(from, to, added)| {
            assert_eq!((from, to), (caret, caret));
            added
        })
    }

    #[test]
    fn typing_the_end_of_a_jsx_start_tag_adds_its_end_tag() {
        let cases: &[(&str, Option<&str>)] = &[
            ("const a = (<div className=\"x\">|", Some("$0</div>")),
            ("function A() {\n  return <>|\n}", Some("$0</>")),
            ("render(<Card.Body title={a > b ? 'x' : 'y'}>|)", Some("$0</Card.Body>")),
            ("const list = items.map((item) => <li key={item}>|)", Some("$0</li>")),
            ("(<div><span>|</div>)", Some("$0</span>")),
            ("(<div><div>|</div>)", Some("$0</div>")),
            ("(<div>|</div>)", None),
            ("const less = a <b>|", None),
            ("const f = () =>|", None),
            ("(<br />|", None),
            ("// (<div>|", None),
            ("const s = \"(<div>|\"", None),
            ("const t = `<p>|`", None),
            ("(<input value={a >|", None),
            ("if (a > b>|", None),
        ];
        for (marked, expected) in cases {
            assert_eq!(end_tag(marked).as_deref(), *expected, "{marked}");
        }
    }

    #[test]
    fn the_on_type_request_answers_json() {
        let text = "x = <p>";
        let mut input = Vec::new();
        input.extend_from_slice(&(text.len() as u32).to_le_bytes());
        input.extend_from_slice(&('>' as u32).to_le_bytes());
        input.extend_from_slice(text.as_bytes());
        assert_eq!(String::from_utf8(on_type_request(&input)).unwrap(), r#"{"from":7,"to":7,"text":"$0</p>"}"#);
        assert!(on_type_request(&input[..6]).is_empty());
    }

    #[test]
    fn hostile_input_stays_bounded() {
        for text in ["class A {'é", "class A {\n 'é\n}", "enum E { 'é", "'\\é"] {
            outline(text, DIALECT);
        }
        assert_eq!(outline("class A { 'ab", DIALECT)[0].children.first().map(|m| m.name.as_str()), Some("ab"));
        let brackets = format!("{}{}", "(".repeat(20_000), "]".repeat(20_000));
        assert!(fold(&brackets, DIALECT).is_empty());
        outline(&"function;".repeat(20_000), DIALECT);
        let names: String = (0..5000).map(|n| format!("x=<a{n}>;\n")).collect();
        highlight(&names, DIALECT);
        let vars: String = (0..20_000).map(|n| format!("var a{n};\n")).collect();
        assert_eq!(complete(&vars, vars.len(), DIALECT).items.len(), 2000);
    }

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text, DIALECT).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    fn significant(text: &str) -> Vec<(&str, Kind)> {
        kinds(text).into_iter().filter(|&(_, kind)| kind != Kind::Punctuation).collect()
    }

    fn assert_contains(text: &str, expected: &[(&str, Kind)]) {
        let found = kinds(text);
        for item in expected {
            assert!(found.contains(item), "{item:?} in {found:?}");
        }
    }

    fn assert_plain(text: &str, words: &[&str]) {
        let found = kinds(text);
        for word in words {
            assert!(!found.iter().any(|(text, _)| text == word), "{word} stays plain in {found:?}");
        }
    }

    fn names(symbols: &[Symbol]) -> Vec<(String, SymbolKind)> {
        symbols.iter().map(|s| (s.name.clone(), s.kind)).collect()
    }

    fn labels(text: &str) -> Vec<(String, &'static str)> {
        let caret = text.find('|').expect("a caret");
        let source = text.replacen('|', "", 1);
        complete(&source, caret, DIALECT).items.into_iter().map(|item| (item.label, item.kind)).collect()
    }

    fn has(items: &[(String, &'static str)], label: &str, kind: &str) -> bool {
        items.iter().any(|(l, k)| l == label && *k == kind)
    }

    #[test]
    fn highlights_a_module() {
        let text = "\
import { readFile } from 'node:fs';
/** Reads a file. @param {string} path */
export async function load(path, ...rest) {
  const data = await readFile(path, \"utf8\");
  return data ?? null;
}
";
        assert_eq!(
            significant(text),
            [
                ("import", Kind::Keyword),
                ("from", Kind::Keyword),
                ("'node:fs'", Kind::String),
                ("/** Reads a file. ", Kind::DocComment),
                ("@param", Kind::Attribute),
                (" {string} path */", Kind::DocComment),
                ("export", Kind::Keyword),
                ("async", Kind::Keyword),
                ("function", Kind::Keyword),
                ("load", Kind::Function),
                ("const", Kind::Keyword),
                ("await", Kind::Keyword),
                ("readFile", Kind::Function),
                ("\"utf8\"", Kind::String),
                ("return", Kind::Keyword),
                ("null", Kind::Keyword),
            ]
        );
    }

    #[test]
    fn highlights_template_literals_with_nesting() {
        let text = "const s = `a ${b ? `x${c}y` : 'z'} d`;";
        assert_eq!(
            kinds(text),
            [
                ("const", Kind::Keyword),
                ("s", Kind::GlobalVariable),
                ("=", Kind::Punctuation),
                ("`a ", Kind::String),
                ("${", Kind::Punctuation),
                ("?", Kind::Punctuation),
                ("`x", Kind::String),
                ("${", Kind::Punctuation),
                ("}", Kind::Punctuation),
                ("y`", Kind::String),
                (":", Kind::Punctuation),
                ("'z'", Kind::String),
                ("}", Kind::Punctuation),
                (" d`", Kind::String),
                (";", Kind::Punctuation),
            ]
        );
        assert_contains("`${ {a: 1}.a }`", &[("`", Kind::String), ("1", Kind::Number), ("}", Kind::Punctuation)]);
        assert_contains("tag`one\ntwo ${x}`", &[("`one\ntwo ", Kind::String)]);
    }

    #[test]
    fn calls_properties_and_globals_are_coloured_after_indexing() {
        let text = "const d = \"d\"; function test() { console.warn(d); } test();";
        assert_eq!(
            significant(text),
            [
                ("const", Kind::Keyword),
                ("d", Kind::GlobalVariable),
                ("\"d\"", Kind::String),
                ("function", Kind::Keyword),
                ("test", Kind::Function),
                ("console", Kind::GlobalVariable),
                ("warn", Kind::Function),
                ("d", Kind::GlobalVariable),
                ("test", Kind::Function),
            ]
        );
        let richer = "\
let count = 0;
class Greeter {
  size = 1;
  greet(times) { const local = this.name; return local.repeat(times); }
}
function main(args) {
  const g = new Greeter(); count += args.length; window.setTimeout(() => g.greet(2), 10);
  return Object.assign(new Error(\"x\"), { statusCode: args.code ?? null, ok: undefined });
}
";
        assert_contains(
            richer,
            &[
                ("count", Kind::GlobalVariable),
                ("size", Kind::Property),
                ("greet", Kind::Function),
                ("repeat", Kind::Function),
                ("window", Kind::GlobalVariable),
                ("setTimeout", Kind::Function),
                ("Greeter", Kind::Function),
                ("Object", Kind::Function),
                ("assign", Kind::Function),
                ("Error", Kind::Function),
                ("statusCode", Kind::Property),
                ("ok", Kind::Property),
                ("null", Kind::Keyword),
                ("undefined", Kind::GlobalVariable),
            ],
        );
        assert_plain(richer, &["local", "times", "args", "g", "name", "length", "code"]);
        let shadowed = "const x = 1; function f() { const x = 2; return x; }";
        assert_plain(shadowed, &["x"]);
    }

    #[test]
    fn highlights_numbers() {
        for number in
            ["0", "42", "3.14", ".5", "1e3", "6.02E+23", "0x1F", "0o17", "0b1010", "1_000_000", "10n", "0xFFn"]
        {
            assert_eq!(kinds(&format!("x = {number};"))[1], (number, Kind::Number), "{number}");
        }
        assert_contains("a?.5:1", &[(".5", Kind::Number)]);
        assert_contains("a?.b", &[("?.", Kind::Punctuation)]);
    }

    #[test]
    fn tells_regular_expressions_from_division() {
        assert_contains("const re = /ab+[/]c/gi;", &[("/ab+[/]c/gi", Kind::String)]);
        assert_contains("if (/^\\d+$/.test(s)) {}", &[("/^\\d+$/", Kind::String)]);
        assert_contains("return /x/;", &[("/x/", Kind::String)]);
        let division = kinds("const half = total / 2 / count;");
        assert!(division.contains(&("/", Kind::Punctuation)), "{division:?}");
        assert!(!division.iter().any(|&(_, kind)| kind == Kind::String), "{division:?}");
        let call = kinds("f(a) / g(b) / 2");
        assert!(!call.iter().any(|&(_, kind)| kind == Kind::String), "{call:?}");
        assert_contains("x = a[0] /2/ 1", &[("/", Kind::Punctuation)]);
    }

    #[test]
    fn highlights_comments() {
        assert_eq!(
            kinds("#!/usr/bin/env node\n// line\n/* block\n */ x /**/"),
            [
                ("#!/usr/bin/env node", Kind::Comment),
                ("// line", Kind::Comment),
                ("/* block\n */", Kind::Comment),
                ("/**/", Kind::Comment),
            ]
        );
        assert_eq!(
            kinds("/**\n * Adds.\n * @returns {number}\n */"),
            [
                ("/**\n * Adds.\n * ", Kind::DocComment),
                ("@returns", Kind::Attribute),
                (" {number}\n */", Kind::DocComment),
            ]
        );
        assert_eq!(kinds("/** mail me@example.com */"), [("/** mail me@example.com */", Kind::DocComment)]);
    }

    #[test]
    fn highlights_declarations() {
        let text = "\
function* gen() {}
class Shape extends Base.Model {
  #size = 0;
  static count = 1;
  constructor(size) { super(); this.#size = size; }
  get area() { return this.#size; }
  static async create() {}
  render = () => {};
  *items() {}
}
const add = (a, b) => a + b, mul = async x => x;
let handler = function () {};
const obj = { method() {}, get value() { return 1; }, key: 1, default: 2 };
";
        assert_contains(
            text,
            &[
                ("gen", Kind::Function),
                ("Shape", Kind::Type),
                ("extends", Kind::Keyword),
                ("Base", Kind::Type),
                ("Model", Kind::Type),
                ("static", Kind::Keyword),
                ("constructor", Kind::Function),
                ("super", Kind::Keyword),
                ("this", Kind::Keyword),
                ("get", Kind::Keyword),
                ("area", Kind::Function),
                ("async", Kind::Keyword),
                ("create", Kind::Function),
                ("render", Kind::Function),
                ("items", Kind::Function),
                ("add", Kind::Function),
                ("handler", Kind::Function),
                ("method", Kind::Function),
                ("value", Kind::Function),
            ],
        );
        assert_contains(
            text,
            &[
                ("#size", Kind::Property),
                ("count", Kind::Property),
                ("key", Kind::Property),
                ("default", Kind::Property),
            ],
        );
        assert_eq!(kinds(text).iter().filter(|(word, _)| *word == "#size").count(), 1, "this.#size stays plain");
        assert_plain(text, &["size", "a", "b"]);
        assert_contains(text, &[("obj", Kind::GlobalVariable)]);
    }

    #[test]
    fn keywords_after_a_dot_are_properties() {
        assert_contains(
            "promise.catch(e).finally(f); map.delete(k); x?.new",
            &[("catch", Kind::Function), ("finally", Kind::Function), ("delete", Kind::Function)],
        );
        assert_plain("x?.new", &["new"]);
        assert_contains("for (const item of items) {}", &[("of", Kind::Keyword)]);
        let names = "const of = 1, async = 2, from = 3, get = 4;";
        for word in ["of", "async", "from", "get"] {
            assert!(kinds(names).contains(&(word, Kind::GlobalVariable)), "{word} in {:?}", kinds(names));
        }
    }

    #[test]
    fn highlights_jsx() {
        let text = "\
const App = () => (
  <div className=\"app\" onClick={() => go(1)}>
    Hello, {user.name}! a < b
    <Button.Primary disabled {...props} />
    <>
      <span data-id='x'>{count > 1 ? 'many' : 'one'}</span>
    </>
  </div>
);
const n = 1 < 2;
";
        assert_contains(
            text,
            &[
                ("App", Kind::Function),
                ("div", Kind::Keyword),
                ("className", Kind::Attribute),
                ("\"app\"", Kind::String),
                ("onClick", Kind::Attribute),
                ("1", Kind::Number),
                ("Button.Primary", Kind::Type),
                ("disabled", Kind::Attribute),
                ("/>", Kind::Punctuation),
                ("span", Kind::Keyword),
                ("data-id", Kind::Attribute),
                ("'x'", Kind::String),
                ("'many'", Kind::String),
                ("</", Kind::Punctuation),
            ],
        );
        assert_plain(text, &["Hello", "user", "b"]);
        assert_plain(text, &["name"]);
        let after = kinds(text);
        assert!(after.ends_with(&[
            ("1", Kind::Number),
            ("<", Kind::Punctuation),
            ("2", Kind::Number),
            (";", Kind::Punctuation)
        ]));
    }

    #[test]
    fn decorators_are_attributes() {
        assert_contains(
            "@Component({ tag: 'x' })\nclass View { @observable.ref value = 1; }",
            &[
                ("@", Kind::Attribute),
                ("Component", Kind::Attribute),
                ("observable", Kind::Attribute),
                ("ref", Kind::Attribute),
            ],
        );
    }

    #[test]
    fn folds_blocks_comments_jsx_and_imports() {
        let text = "\
import a from 'a';
import {
  b,
} from 'b';
/*
 * Note.
 */
function f() {
  const list = [
    1,
  ];
  return (
    <ul>
      <li />
    </ul>
  );
}
const one = { a: 1 };
";
        assert_eq!(
            fold(text, DIALECT),
            [
                Fold { start_line: 0, end_line: 3 },
                Fold { start_line: 1, end_line: 3 },
                Fold { start_line: 4, end_line: 6 },
                Fold { start_line: 7, end_line: 16 },
                Fold { start_line: 8, end_line: 10 },
                Fold { start_line: 12, end_line: 14 },
            ]
        );
        assert!(
            fold("import a from 'a';\nimport b from 'b';\n\nx();\nimport c from 'c';\n", DIALECT)
                .contains(&Fold { start_line: 0, end_line: 1 })
        );
        assert_eq!(
            fold("a\r\n{\r\n}\r{\r}", DIALECT),
            [Fold { start_line: 1, end_line: 2 }, Fold { start_line: 3, end_line: 4 }]
        );
    }

    #[test]
    fn outlines_a_module() {
        let text = "\
import x from 'x';
export const API = 'https://example.com', retries = 3;
let counter = 0;
var legacy;
const handler = async (event) => { return event; };
export default function () {}
function helper(a) {
  function inner() {}
  const local = 1;
}
export class Store extends Base {
  static instance = null;
  #items = [];
  constructor() { super(); }
  get size() { return this.#items.length; }
  add = (item) => { this.#items.push(item); };
  async *[Symbol.asyncIterator]() {}
}
if (x) { function hidden() {} }
const { a, b } = obj;
";
        let symbols = outline(text, DIALECT);
        assert_eq!(
            names(&symbols),
            [
                ("API".to_owned(), SymbolKind::Constant),
                ("retries".to_owned(), SymbolKind::Constant),
                ("counter".to_owned(), SymbolKind::Variable),
                ("legacy".to_owned(), SymbolKind::Variable),
                ("handler".to_owned(), SymbolKind::Function),
                ("default".to_owned(), SymbolKind::Function),
                ("helper".to_owned(), SymbolKind::Function),
                ("Store".to_owned(), SymbolKind::Class),
            ]
        );
        let store = &symbols[7];
        assert_eq!(
            names(&store.children),
            [
                ("instance".to_owned(), SymbolKind::Field),
                ("#items".to_owned(), SymbolKind::Field),
                ("constructor".to_owned(), SymbolKind::Method),
                ("size".to_owned(), SymbolKind::Method),
                ("add".to_owned(), SymbolKind::Method),
                ("[Symbol.asyncIterator]".to_owned(), SymbolKind::Method),
            ]
        );
        let range = |s: &Symbol| &text[s.start..s.end];
        assert_eq!(range(&symbols[0]), "export const API = 'https://example.com'");
        assert_eq!(range(&symbols[1]), "retries = 3");
        assert!(range(&symbols[6]).starts_with("function helper(a) {") && range(&symbols[6]).ends_with('}'));
        assert!(range(store).starts_with("export class Store") && range(store).ends_with("}"));
        assert_eq!(range(&store.children[0]), "static instance = null");
        assert_eq!(range(&store.children[4]), "add = (item) => { this.#items.push(item); }");
    }

    #[test]
    fn outlines_statements_without_semicolons() {
        let text = "const a = 1\nconst b = () => {\n  return 2\n}\nclass C {\n  x = 1\n  y\n  m() {}\n}\n";
        let symbols = outline(text, DIALECT);
        assert_eq!(
            names(&symbols)[..2],
            [("a".to_owned(), SymbolKind::Constant), ("b".to_owned(), SymbolKind::Function)]
        );
        assert_eq!(&text[symbols[0].start..symbols[0].end], "const a = 1");
        assert_eq!(&text[symbols[1].start..symbols[1].end], "const b = () => {\n  return 2\n}");
        assert_eq!(
            names(&symbols[2].children),
            [
                ("x".to_owned(), SymbolKind::Field),
                ("y".to_owned(), SymbolKind::Field),
                ("m".to_owned(), SymbolKind::Method)
            ]
        );
    }

    #[test]
    fn encodes_outline_json() {
        let json = outline_json(&outline("class A { \"q\\\"\"() {} }", DIALECT));
        assert_eq!(
            json,
            "[{\"name\":\"A\",\"kind\":\"class\",\"start\":0,\"end\":22,\"children\":[{\"name\":\"q\\\\\\\"\",\"kind\":\"method\",\"start\":10,\"end\":20,\"children\":[]}]}]"
        );
    }

    #[test]
    fn completes_keywords_names_and_snippets() {
        let text = "\
import React, { useState as useLocal } from 'react';
import * as path from 'path';
function render(props, { title }) {}
class Widget {}
const LIMIT = 10;
let total = 0;
const make = () => {};
|";
        let items = labels(text);
        for (label, kind) in [
            ("React", "variable"),
            ("useLocal", "variable"),
            ("path", "variable"),
            ("render", "function"),
            ("props", "variable"),
            ("title", "variable"),
            ("Widget", "class"),
            ("LIMIT", "constant"),
            ("total", "variable"),
            ("make", "function"),
            ("const", "keyword"),
            ("function", "keyword"),
            ("for", "snippet"),
            ("if", "snippet"),
            ("class", "snippet"),
            ("function", "snippet"),
        ] {
            assert!(has(&items, label, kind), "{label} {kind} in {items:?}");
        }
        assert!(!has(&items, "useState", "variable"), "{items:?}");
        assert!(!has(&items, "interface", "keyword"), "{items:?}");
        assert!(!has(&items, "arrow", "snippet"), "{items:?}");

        let items = labels("const total = 1;\nconst x = to|");
        assert_eq!(items, [("total".to_owned(), "constant")]);
        let items = labels("const x = |");
        assert!(
            has(&items, "arrow", "snippet") && has(&items, "new", "keyword") && !has(&items, "const", "keyword"),
            "{items:?}"
        );
        let items = labels("for (const k i|");
        assert!(has(&items, "in", "keyword") && !has(&items, "if", "keyword"), "{items:?}");
        let items = labels("if (a) {} el|");
        assert_eq!(items, [("else".to_owned(), "keyword")]);
    }

    #[test]
    fn completion_replaces_the_word_before_the_caret() {
        let source = "let value = 1;\nva";
        let completion = complete(source, source.len(), DIALECT);
        assert_eq!(completion.from, source.len() - 2);
        assert!(completion.items.iter().any(|item| item.label == "value"));
        assert!(completion.items.iter().any(|item| item.label == "var" && item.kind == "keyword"));
        assert!(!completion.items.iter().any(|item| item.label == "va"));
    }

    #[test]
    fn completes_members_of_this() {
        let text = "\
class Counter {
  count = 0;
  #step = 1;
  constructor() { this.label = 'c'; }
  increment() { this.| }
  static reset() {}
}
class Other { foreign() {} }
";
        let items = labels(text);
        assert_eq!(
            items,
            [
                ("count".to_owned(), "field"),
                ("#step".to_owned(), "field"),
                ("increment".to_owned(), "method"),
                ("reset".to_owned(), "method"),
                ("label".to_owned(), "field"),
            ]
        );
        assert_eq!(labels("class A { #x = 1; m() { this.#| } }"), [("#x".to_owned(), "field")]);
        assert!(labels("obj.|").is_empty());
    }

    #[test]
    fn completes_nothing_in_strings_comments_and_jsx_text() {
        for text in
            ["'abc|'", "\"ab|", "// note |", "/* | */", "`a|`", "`${'x'}|`", "/re|/", "const a = <p>Hello wo|</p>;"]
        {
            assert!(labels(text).is_empty(), "{text}");
        }
        assert!(!labels("const a = <p>{c|}</p>;").is_empty());
        assert!(!labels("const s = `${|}`;").is_empty());
    }

    #[test]
    fn class_bodies_offer_modifiers() {
        let items = labels("class A {\n  |\n}");
        assert!(has(&items, "static", "keyword") && has(&items, "constructor", "keyword"), "{items:?}");
        assert!(!has(&items, "private", "keyword") && !has(&items, "for", "snippet"), "{items:?}");
    }

    #[test]
    fn snippets_follow_the_indentation() {
        let source = "function f() {\n    if (x) {}\n    fo";
        let completion = complete(source, source.len(), DIALECT);
        let snippet = completion.items.iter().find(|item| item.kind == "snippet" && item.label == "for").unwrap();
        assert_eq!(snippet.insert, "for (let i = 0; i < $0; i++) {\n        \n    }");
        assert_eq!(snippet.insert.matches("$0").count(), 1);
        let json = completion.to_json();
        assert!(json.starts_with(&format!("{{\"from\":{},\"items\":[", source.len() - 2)), "{json}");
        assert!(
            json.contains("\"insert\":\"for (let i = 0; i < $0; i++) {\\n        \\n    }\",\"detail\":"),
            "{json}"
        );
    }

    #[test]
    fn encodes_requests_and_records() {
        assert_eq!(
            encode_spans(&[Span { start: 1, end: 3, kind: Kind::Attribute }]),
            [1, 0, 0, 0, 3, 0, 0, 0, 9, 0, 0, 0]
        );
        assert_eq!(encode_folds(&[Fold { start_line: 2, end_line: 5 }]), [2, 0, 0, 0, 5, 0, 0, 0]);
        let mut input = 3u32.to_le_bytes().to_vec();
        input.extend_from_slice(b"let");
        let json = String::from_utf8(complete_request(&input)).unwrap();
        assert!(
            json.starts_with("{\"from\":0,\"items\":[{\"label\":\"let\",\"kind\":\"keyword\",\"insert\":\"let\"}"),
            "{json}"
        );
        assert!(complete_request(&[1, 0]).is_empty());
        assert_eq!(zen_abi(), 1);
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "'", "\"", "`", "${", "}", "{", "(", ")", "[", "]", "<", ">", "/", "/*", "*/", "//", "\n", "a", "1", ".",
            "=", "=>", "#", "@", "\\", "é", "日", "\r", "class ", "<a>", "</", "/>", ":", "?",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    let text = format!("{a}{b}{c}");
                    let mut previous = 0;
                    for span in highlight(&text, DIALECT) {
                        assert!(
                            span.start >= previous
                                && span.end > span.start
                                && span.end <= text.len()
                                && text.is_char_boundary(span.start)
                                && text.is_char_boundary(span.end),
                            "{text:?}"
                        );
                        previous = span.end;
                    }
                    for f in fold(&text, DIALECT) {
                        assert!(f.end_line > f.start_line, "{text:?}");
                    }
                    for symbol in outline(&text, DIALECT) {
                        assert!(symbol.start <= symbol.end && symbol.end <= text.len(), "{text:?}");
                    }
                    for caret in 0..=text.len() {
                        let completion = complete(&text, caret, DIALECT);
                        assert!(
                            completion.from <= caret.max(completion.from) && text.is_char_boundary(completion.from),
                            "{text:?}"
                        );
                    }
                }
            }
        }
    }
}
