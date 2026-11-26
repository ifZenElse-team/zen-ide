#[allow(unreachable_pub, reason = "the shared analyser is public in the JavaScript crate")]
#[path = "../../javascript/src/script.rs"]
mod script;

use std::cell::RefCell;

pub use script::{
    Completion, CompletionItem, Dialect, Fold, JSX_TRIGGERS, Kind, Span, Symbol, SymbolKind, complete, fold, highlight,
    jsx_on_type, outline, outline_json,
};

const DIALECT: Dialect = Dialect::TypeScript;

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
        jsx_on_type(&text, caret, '>', DIALECT).map(|(_, _, added)| added)
    }

    #[test]
    fn tsx_start_tags_get_end_tags_but_generics_and_casts_do_not() {
        let cases: &[(&str, Option<&str>)] = &[
            ("export function App() {\n  return <Layout title=\"a\">|\n}", Some("$0</Layout>")),
            ("const el = (<main>|", Some("$0</main>")),
            ("const n = <number>|", None),
            ("const s = <string>|", None),
            ("function id<T>|", None),
            ("const g = <T,>|", None),
            ("const h = <T extends object>|", None),
            ("const m = useState<string>|", None),
            ("const x: Array<Item>|", None),
        ];
        for (marked, expected) in cases {
            assert_eq!(end_tag(marked).as_deref(), *expected, "{marked}");
        }
    }

    #[test]
    fn deep_namespaces_stop_at_32_levels() {
        let text = "namespace a {".repeat(2000);
        let mut depth = 0;
        let mut level = outline(&text, DIALECT);
        while let Some(first) = level.first() {
            depth += 1;
            level = first.children.clone();
        }
        assert_eq!(depth, 33);
        for text in ["interface I { 'é", "declare module 'é"] {
            outline(text, DIALECT);
        }
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

    fn names(symbols: &[Symbol]) -> Vec<(&str, SymbolKind)> {
        symbols.iter().map(|s| (s.name.as_str(), s.kind)).collect()
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
    fn highlights_annotations() {
        assert_eq!(
            significant("function parse(input: string, opts?: Options<Mode>): Result<Tree, Error[]> {}"),
            [
                ("function", Kind::Keyword),
                ("parse", Kind::Function),
                ("string", Kind::Keyword),
                ("Options", Kind::Type),
                ("Mode", Kind::Type),
                ("Result", Kind::Type),
                ("Tree", Kind::Type),
                ("Error", Kind::Type),
            ]
        );
        assert_eq!(
            significant("let count: number = cond ? a : b;"),
            [("let", Kind::Keyword), ("count", Kind::GlobalVariable), ("number", Kind::Keyword)]
        );
        assert_eq!(
            significant("const f = async <T,>(x: T): Promise<T> => x;"),
            [
                ("const", Kind::Keyword),
                ("f", Kind::Function),
                ("async", Kind::Keyword),
                ("T", Kind::Type),
                ("T", Kind::Type),
                ("Promise", Kind::Type),
                ("T", Kind::Type),
            ]
        );
        let point = "const point = { x: 1, y: cond ? a : b };";
        assert_contains(point, &[("x", Kind::Property), ("y", Kind::Property)]);
        assert_plain(point, &["cond", "a", "b"]);
        assert_plain("switch (k) { case A: break; }", &["A"]);
        assert_contains(
            "const cb = (e: MouseEvent | null, [a, b]: [number, string?]) => e;",
            &[
                ("MouseEvent", Kind::Type),
                ("null", Kind::Keyword),
                ("number", Kind::Keyword),
                ("string", Kind::Keyword),
            ],
        );
    }

    #[test]
    fn highlights_interfaces_and_aliases() {
        let text = "\
export interface Repo<T extends Entity = Entity> extends Base, Other<T> {
  readonly id: string;
  items?: Map<string, T>;
  find(id: string): T | undefined;
  [key: string]: unknown;
  new (raw: Raw): Repo<T>;
}
type Handler = (event: Event, ...rest: any[]) => void;
type Keys = keyof typeof config;
type Partial2<T> = { [K in keyof T]?: T[K] };
type Pick2<T, K extends keyof T> = T extends Array<infer U> ? U : never;
type Lit = 'a' | \"b\" | 1 | `x-${string}`;
";
        assert_contains(
            text,
            &[
                ("interface", Kind::Keyword),
                ("Repo", Kind::Type),
                ("Entity", Kind::Type),
                ("Base", Kind::Type),
                ("Other", Kind::Type),
                ("readonly", Kind::Keyword),
                ("Map", Kind::Type),
                ("find", Kind::Function),
                ("unknown", Kind::Keyword),
                ("Raw", Kind::Type),
                ("type", Kind::Keyword),
                ("Handler", Kind::Type),
                ("Event", Kind::Type),
                ("any", Kind::Keyword),
                ("void", Kind::Keyword),
                ("keyof", Kind::Keyword),
                ("typeof", Kind::Keyword),
                ("K", Kind::Type),
                ("infer", Kind::Keyword),
                ("U", Kind::Type),
                ("never", Kind::Keyword),
                ("'a'", Kind::String),
                ("\"b\"", Kind::String),
            ],
        );
        assert_contains(text, &[("id", Kind::Property), ("items", Kind::Property)]);
        assert_eq!(kinds(text).iter().filter(|(word, _)| *word == "id").count(), 1, "find(id) stays plain");
        assert_plain(text, &["key", "raw", "event", "rest"]);
    }

    #[test]
    fn highlights_classes_enums_and_namespaces() {
        let text = "\
@Injectable()
export abstract class Service<T> extends Base<T> implements Disposable {
  private readonly cache = new Map<string, T>();
  protected static count: number;
  declare value?: string;
  constructor(private http: Client, public name = 'x') { super(); }
  abstract load(id: string): Promise<T>;
  override get size(): number { return 0; }
  @Input() label!: string;
}
const enum Color { Red, Green = 'g' }
declare namespace App.Util { const x: number; }
declare module 'virtual' {}
let v = value as unknown as Color;
let w = cfg satisfies Config;
";
        assert_contains(
            text,
            &[
                ("@", Kind::Attribute),
                ("Injectable", Kind::Attribute),
                ("abstract", Kind::Keyword),
                ("Service", Kind::Type),
                ("Base", Kind::Type),
                ("implements", Kind::Keyword),
                ("Disposable", Kind::Type),
                ("private", Kind::Keyword),
                ("readonly", Kind::Keyword),
                ("string", Kind::Keyword),
                ("protected", Kind::Keyword),
                ("static", Kind::Keyword),
                ("declare", Kind::Keyword),
                ("constructor", Kind::Function),
                ("Client", Kind::Type),
                ("public", Kind::Keyword),
                ("load", Kind::Function),
                ("Promise", Kind::Type),
                ("override", Kind::Keyword),
                ("get", Kind::Keyword),
                ("size", Kind::Function),
                ("Input", Kind::Attribute),
                ("enum", Kind::Keyword),
                ("Color", Kind::Type),
                ("Red", Kind::Constant),
                ("Green", Kind::Constant),
                ("namespace", Kind::Keyword),
                ("App", Kind::Type),
                ("Util", Kind::Type),
                ("module", Kind::Keyword),
                ("as", Kind::Keyword),
                ("unknown", Kind::Keyword),
                ("satisfies", Kind::Keyword),
                ("Config", Kind::Type),
            ],
        );
        assert_contains(
            text,
            &[
                ("cache", Kind::Property),
                ("count", Kind::Property),
                ("value", Kind::Property),
                ("label", Kind::Property),
            ],
        );
        assert_plain(text, &["http", "name", "id", "x", "cfg"]);
        let found = kinds(text);
        assert!(
            found.contains(&("v", Kind::GlobalVariable)) && found.contains(&("w", Kind::GlobalVariable)),
            "{found:?}"
        );
    }

    #[test]
    fn tells_generics_from_comparisons() {
        assert_contains("const m = new Map<string, number>();", &[("Map", Kind::Function), ("number", Kind::Keyword)]);
        assert_contains("const r = useState<User | null>(null);", &[("User", Kind::Type)]);
        assert_plain("if (a < b && c > d) {}", &["a", "b", "c", "d"]);
        assert_plain("for (let i = 0; i < n; i++) {}", &["i", "n"]);
    }

    #[test]
    fn highlights_tsx_and_casts() {
        let text = "\
const view = <Panel title={name as string}>{items.map((i: Item) => <Row key={i.id} />)}</Panel>;
const n = <number>value;
const id = <T,>(x: T) => x;
";
        assert_contains(
            text,
            &[
                ("Panel", Kind::Type),
                ("title", Kind::Attribute),
                ("as", Kind::Keyword),
                ("string", Kind::Keyword),
                ("Item", Kind::Type),
                ("Row", Kind::Type),
                ("key", Kind::Attribute),
                ("number", Kind::Keyword),
                ("id", Kind::Function),
            ],
        );
        let found = kinds(text);
        assert!(found.contains(&("T", Kind::Type)), "{found:?}");
        assert!(!found.contains(&("value", Kind::Attribute)), "{found:?}");
    }

    #[test]
    fn folds_type_bodies() {
        let text = "interface A {\n  a: string;\n}\ntype B = {\n  b: number;\n};\nenum C {\n  X,\n}\n";
        assert_eq!(
            fold(text, DIALECT),
            [
                Fold { start_line: 0, end_line: 2 },
                Fold { start_line: 3, end_line: 5 },
                Fold { start_line: 6, end_line: 8 },
            ]
        );
    }

    #[test]
    fn outlines_types_and_namespaces() {
        let text = "\
import type { A } from './a';
export interface Shape {
  readonly kind: 'circle' | 'square';
  area(): number;
  scale?(by: number): void,
}
export type Id = string | number;
type Map2<K, V> = {
  [key: string]: V;
};
export enum Color { Red = 'r', Green = 'g' }
declare const enum Flag { On }
export namespace Geometry.Shapes {
  export function make(): Shape { return null!; }
  export const unit: number = 1;
  class Hidden {}
}
declare module 'virtual' {
  export const value: number;
}
export abstract class Base<T> implements Shape {
  kind: 'circle' = 'circle';
  private readonly store: Map<string, T> = new Map();
  abstract area(): number;
  constructor(private name: string) {}
}
export default class {}
export const handler = <T,>(value: T): T => value;
function overload(a: string): void;
function overload(a: any) {}
";
        let symbols = outline(text, DIALECT);
        assert_eq!(
            names(&symbols),
            [
                ("Shape", SymbolKind::Interface),
                ("Id", SymbolKind::Type),
                ("Map2", SymbolKind::Type),
                ("Color", SymbolKind::Type),
                ("Flag", SymbolKind::Type),
                ("Geometry.Shapes", SymbolKind::Module),
                ("virtual", SymbolKind::Module),
                ("Base", SymbolKind::Class),
                ("default", SymbolKind::Class),
                ("handler", SymbolKind::Function),
                ("overload", SymbolKind::Function),
                ("overload", SymbolKind::Function),
            ]
        );
        assert_eq!(
            names(&symbols[0].children),
            [("kind", SymbolKind::Field), ("area", SymbolKind::Method), ("scale", SymbolKind::Method)]
        );
        assert_eq!(names(&symbols[3].children), [("Red", SymbolKind::Constant), ("Green", SymbolKind::Constant)]);
        assert_eq!(
            names(&symbols[5].children),
            [("make", SymbolKind::Function), ("unit", SymbolKind::Constant), ("Hidden", SymbolKind::Class)]
        );
        assert_eq!(names(&symbols[6].children), [("value", SymbolKind::Constant)]);
        assert_eq!(
            names(&symbols[7].children),
            [
                ("kind", SymbolKind::Field),
                ("store", SymbolKind::Field),
                ("area", SymbolKind::Method),
                ("constructor", SymbolKind::Method),
            ]
        );
        let range = |s: &Symbol| &text[s.start..s.end];
        assert_eq!(range(&symbols[1]), "export type Id = string | number");
        assert_eq!(range(&symbols[0].children[1]), "area(): number;");
        assert_eq!(range(&symbols[7].children[2]), "abstract area(): number;");
        assert_eq!(range(&symbols[10]), "function overload(a: string): void");
        let json = outline_json(&symbols[3..4]);
        assert!(json.starts_with("[{\"name\":\"Color\",\"kind\":\"type\""), "{json}");
    }

    #[test]
    fn completes_types_in_type_positions() {
        let text = "\
interface User { name: string }
type Id = string;
enum Role { Admin }
class Account {}
const LIMIT = 1;
function f(u: |) {}";
        let items = labels(text);
        for (label, kind) in [
            ("User", "interface"),
            ("Id", "type"),
            ("Role", "type"),
            ("Account", "class"),
            ("string", "keyword"),
            ("keyof", "keyword"),
        ] {
            assert!(has(&items, label, kind), "{label} {kind} in {items:?}");
        }
        assert!(!has(&items, "LIMIT", "constant") && !has(&items, "for", "snippet"), "{items:?}");
        let items = labels("let x: Ma|");
        assert!(items.is_empty(), "{items:?}");
        let items = labels("interface User {}\nconst u = |");
        assert!(!has(&items, "User", "interface"), "{items:?}");
        let items = labels("|");
        assert!(has(&items, "interface", "keyword") && has(&items, "enum", "keyword"), "{items:?}");
        let items = labels("class A {\n  pr|\n}");
        assert_eq!(items, [("private".to_owned(), "keyword"), ("protected".to_owned(), "keyword")]);
        let items = labels("const v = x a|");
        assert_eq!(items, [("as".to_owned(), "keyword")]);
    }

    #[test]
    fn completes_members_of_this_with_parameter_properties() {
        let items = labels("class A {\n  private count: number = 0;\n  run(): void { this.c| }\n}");
        assert_eq!(items, [("count".to_owned(), "field")]);
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "'",
            "`",
            "${",
            "}",
            "{",
            "(",
            ")",
            "[",
            "]",
            "<",
            ">",
            "/",
            "/*",
            "\n",
            "a",
            "1",
            ".",
            "=",
            "=>",
            ":",
            "?",
            "|",
            "&",
            "@",
            "é",
            "type ",
            "interface ",
            "enum ",
            "as ",
            "extends ",
            "keyof ",
            "<T>",
            "</",
            "/>",
            ",",
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
                    let _ = fold(&text, DIALECT);
                    for symbol in outline(&text, DIALECT) {
                        assert!(symbol.start <= symbol.end && symbol.end <= text.len(), "{text:?}");
                    }
                    for caret in 0..=text.len() {
                        let completion = complete(&text, caret, DIALECT);
                        assert!(text.is_char_boundary(completion.from), "{text:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        let text = format!("let x: {}T{};", "(".repeat(5000), ")".repeat(5000));
        assert!(!highlight(&text, DIALECT).is_empty());
        let text = format!("type A = {}T{};", "{a: ".repeat(3000), "}".repeat(3000));
        assert!(!highlight(&text, DIALECT).is_empty());
        let _ = outline(&text, DIALECT);
    }

    #[test]
    fn is_abi_one() {
        assert_eq!(zen_abi(), 1);
    }
}
