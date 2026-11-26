use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    JavaScript,
    TypeScript,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
    DocComment = 4,
    Type = 6,
    Function = 7,
    Attribute = 9,
    Constant = 10,
    Punctuation = 11,
    Property = 17,
    GlobalVariable = 18,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Method,
    Variable,
    Constant,
    Interface,
    Type,
    Module,
    Field,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Class => "class",
            Self::Method => "method",
            Self::Variable => "variable",
            Self::Constant => "constant",
            Self::Interface => "interface",
            Self::Type => "type",
            Self::Module => "module",
            Self::Field => "field",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: &'static str,
    pub insert: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub from: usize,
    pub items: Vec<CompletionItem>,
}

impl Completion {
    pub fn to_json(&self) -> String {
        let mut out = format!("{{\"from\":{},\"items\":[", self.from);
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"label\":");
            json_string(&mut out, &item.label);
            let _ = write!(out, ",\"kind\":\"{}\",\"insert\":", item.kind);
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
}

pub fn outline_json(symbols: &[Symbol]) -> String {
    let mut out = String::new();
    write_symbols(&mut out, symbols);
    out
}

fn write_symbols(out: &mut String, symbols: &[Symbol]) {
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
            symbol.kind.as_str(),
            symbol.start,
            symbol.end
        );
        write_symbols(out, &symbol.children);
        out.push('}');
    }
    out.push(']');
}

fn json_string(out: &mut String, value: &str) {
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

pub fn highlight(text: &str, dialect: Dialect) -> Vec<Span> {
    let mut analysis = Analysis::new(text, dialect);
    analysis.walk();
    analysis.spans()
}

pub fn fold(text: &str, dialect: Dialect) -> Vec<Fold> {
    Analysis::new(text, dialect).folds()
}

pub fn outline(text: &str, dialect: Dialect) -> Vec<Symbol> {
    let analysis = Analysis::new(text, dialect);
    analysis.items(0, analysis.sig.len())
}

pub fn complete(text: &str, caret: usize, dialect: Dialect) -> Completion {
    let mut caret = caret.min(text.len());
    while !text.is_char_boundary(caret) {
        caret -= 1;
    }
    let mut analysis = Analysis::new(text, dialect);
    analysis.walk();
    analysis.complete(caret)
}

pub const JSX_TRIGGERS: &str = ">";

const TYPE_NAMES: &[&str] = &[
    "any",
    "bigint",
    "boolean",
    "never",
    "null",
    "number",
    "object",
    "string",
    "symbol",
    "undefined",
    "unknown",
    "void",
];

pub fn jsx_on_type(text: &str, caret: usize, typed: char, dialect: Dialect) -> Option<(usize, usize, String)> {
    if typed != '>' || caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let prefix = &text[..caret];
    if !prefix.ends_with('>') || prefix.ends_with("/>") || prefix.ends_with("=>") || prefix.ends_with("->") {
        return None;
    }
    let bytes = prefix.as_bytes();
    let mut lowest = caret.saturating_sub(4096);
    while !prefix.is_char_boundary(lowest) {
        lowest += 1;
    }
    let mut lt = caret - 1;
    let (start, name) = loop {
        lt = prefix[lowest..lt].rfind('<').map(|at| at + lowest)?;
        if let Some(name) = start_tag(prefix, lt).filter(|end| *end == caret).and_then(|_| tag_name(prefix, lt)) {
            break (lt, name);
        }
    };
    if bytes.get(start + 1) == Some(&b'/') {
        return None;
    }
    let name_end = start + 1 + name.len();
    let attributes = prefix[name_end..caret - 1].trim();
    let generic = attributes.starts_with("extends ")
        || attributes.chars().next().is_some_and(|c| !(c.is_alphabetic() || matches!(c, '_' | '$' | '{')));
    if generic {
        return None;
    }
    if attributes.is_empty() {
        let single_capital = name.len() == 1 && name.as_bytes()[0].is_ascii_uppercase();
        if single_capital || (dialect == Dialect::TypeScript && TYPE_NAMES.contains(&name)) {
            return None;
        }
    }
    if !expression_before(prefix, start) {
        return None;
    }
    let quiet = highlight(text, dialect).into_iter().any(|span| {
        span.start <= start && start < span.end && matches!(span.kind, Kind::String | Kind::Comment | Kind::DocComment)
    });
    if quiet {
        return None;
    }
    let open = open_count(text, 0, caret, name);
    let closed_after = -open_count(text, caret, text.len(), name);
    if closed_after >= open {
        return None;
    }
    Some((caret, caret, format!("$0</{name}>")))
}

fn tag_name(text: &str, lt: usize) -> Option<&str> {
    let rest = &text[lt + 1..];
    if rest.starts_with('>') {
        return Some("");
    }
    let first = rest.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return None;
    }
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.' | '-' | ':')))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

fn start_tag(text: &str, lt: usize) -> Option<usize> {
    let name = tag_name(text, lt)?;
    let bytes = text.as_bytes();
    let mut at = lt + 1 + name.len();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    while at < bytes.len() {
        let b = bytes[at];
        match (quote, b) {
            (Some(q), _) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'' | b'`') => quote = Some(b),
            (None, b'{') => depth += 1,
            (None, b'}') => depth = depth.checked_sub(1)?,
            (None, b'<') if depth == 0 => return None,
            (None, b'/') if depth == 0 && bytes.get(at + 1) == Some(&b'>') => return None,
            (None, b'>') if depth == 0 => return Some(at + 1),
            _ => {}
        }
        at += 1;
    }
    None
}

fn expression_before(text: &str, lt: usize) -> bool {
    let before = text[..lt].trim_end();
    let Some(last) = before.chars().last() else { return true };
    if "(,=:?[{};&|!>}".contains(last) {
        return true;
    }
    let word_start =
        before.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')).map_or(0, |at| at + 1);
    matches!(&before[word_start..], "return" | "yield" | "default" | "case" | "await")
}

fn open_count(text: &str, from: usize, to: usize, name: &str) -> i64 {
    let mut count = 0i64;
    let mut at = from;
    while let Some(found) = text[at..to].find('<') {
        let lt = at + found;
        at = lt + 1;
        if text[lt + 1..to].starts_with('/') {
            let close = &text[lt + 2..to];
            if close.starts_with(name) && close[name.len()..].trim_start().starts_with('>') {
                count -= 1;
            }
            continue;
        }
        if tag_name(text, lt) == Some(name) && start_tag(&text[..to], lt).is_some() {
            count += 1;
        }
    }
    count
}

const RESERVED: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

const MAX_OUTLINE_DEPTH: usize = 32;

const MAX_COMPLETIONS: usize = 2000;

const CONSTANTS: &[&str] = &["true", "false", "null", "undefined", "NaN", "Infinity"];

const LITERAL_KEYWORDS: &[&str] = &["true", "false", "null"];

const GLOBAL_CLASSES: &[&str] = &[
    "Object",
    "Array",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "Promise",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Date",
    "RegExp",
    "Number",
    "String",
    "Boolean",
    "Symbol",
    "BigInt",
    "Function",
    "Proxy",
    "URL",
    "Buffer",
];

const GLOBALS: &[&str] = &[
    "console",
    "window",
    "document",
    "globalThis",
    "self",
    "navigator",
    "location",
    "history",
    "localStorage",
    "sessionStorage",
    "performance",
    "crypto",
    "process",
    "module",
    "exports",
    "__dirname",
    "__filename",
    "Math",
    "JSON",
    "Reflect",
    "Intl",
    "Atomics",
];

const REGEX_AFTER: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
];

const TYPE_KEYWORDS: &[&str] = &[
    "any",
    "bigint",
    "boolean",
    "never",
    "number",
    "object",
    "string",
    "symbol",
    "unknown",
    "void",
    "this",
    "null",
    "undefined",
];

const TYPE_PREFIXES: &[&str] = &["keyof", "typeof", "infer", "readonly", "unique", "asserts", "new", "abstract"];

const MODIFIERS: &[&str] = &[
    "static",
    "public",
    "private",
    "protected",
    "readonly",
    "abstract",
    "declare",
    "override",
    "accessor",
    "async",
    "get",
    "set",
];

const PARAM_MODIFIERS: &[&str] = &["public", "private", "protected", "readonly", "override"];

const STATEMENT_KEYWORDS: &[&str] = &[
    "async", "break", "class", "const", "continue", "debugger", "do", "export", "for", "function", "if", "import",
    "let", "return", "switch", "throw", "try", "var", "while",
];

const TS_STATEMENT_KEYWORDS: &[&str] = &["abstract", "declare", "enum", "interface", "namespace", "type"];

const EXPRESSION_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "class",
    "delete",
    "false",
    "function",
    "new",
    "null",
    "super",
    "this",
    "true",
    "typeof",
    "undefined",
    "void",
    "yield",
];

const CLASS_KEYWORDS: &[&str] = &["async", "constructor", "get", "set", "static"];

const TS_CLASS_KEYWORDS: &[&str] = &["abstract", "declare", "override", "private", "protected", "public", "readonly"];

const TYPE_COMPLETIONS: &[&str] = &[
    "any",
    "bigint",
    "boolean",
    "keyof",
    "never",
    "null",
    "number",
    "object",
    "string",
    "symbol",
    "typeof",
    "undefined",
    "unknown",
    "void",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Ident,
    Private,
    Number,
    String,
    Template,
    TemplateOpen,
    TemplateClose,
    Regex,
    Comment,
    DocComment,
    Punct,
    JsxTag,
    JsxAttr,
    JsxString,
    JsxPunct,
    JsxOpen,
    JsxClose,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    start: usize,
    end: usize,
    tok: Tok,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctx {
    Brace,
    TemplateExpr,
    JsxExpr,
    JsxTag { closing: bool, named: bool },
    JsxChildren,
}

fn is_ident_start(c: char) -> bool {
    c == '$' || c == '_' || c.is_ascii_alphabetic() || (!c.is_ascii() && !is_space(c))
}

fn is_ident_char(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

fn is_jsx_name_char(c: char) -> bool {
    is_ident_char(c) || matches!(c, '-' | ':' | '.')
}

fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    ts: bool,
    tokens: Vec<Token>,
    stack: Vec<Ctx>,
    elements: Vec<(usize, usize)>,
    starts: Vec<usize>,
    found: std::collections::HashMap<String, Option<usize>>,
    closings: Option<std::collections::HashMap<String, Vec<usize>>>,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str, ts: bool) -> Self {
        Lexer {
            text,
            bytes: text.as_bytes(),
            pos: 0,
            ts,
            tokens: Vec::new(),
            stack: Vec::new(),
            elements: Vec::new(),
            starts: Vec::new(),
            found: std::collections::HashMap::new(),
            closings: None,
        }
    }

    fn run(mut self) -> (Vec<Token>, Vec<(usize, usize)>) {
        if self.text.starts_with("#!") {
            let end = self.line_end(0);
            self.push(0, end, Tok::Comment);
            self.pos = end;
        }
        while self.pos < self.text.len() {
            match self.stack.last() {
                Some(Ctx::JsxTag { .. }) => self.jsx_tag(),
                Some(Ctx::JsxChildren) => self.jsx_children(),
                _ => self.code(),
            }
        }
        self.elements.sort_unstable();
        (self.tokens, self.elements)
    }

    fn char_at(&self, pos: usize) -> Option<char> {
        self.text.get(pos..).and_then(|rest| rest.chars().next())
    }

    fn push(&mut self, start: usize, end: usize, tok: Tok) {
        if end > start {
            self.tokens.push(Token { start, end, tok });
        }
    }

    fn line_end(&self, from: usize) -> usize {
        self.bytes[from..].iter().position(|&b| b == b'\n' || b == b'\r').map_or(self.bytes.len(), |p| from + p)
    }

    fn scan_while(&self, from: usize, keep: impl Fn(char) -> bool) -> usize {
        self.text[from..].char_indices().find(|&(_, c)| !keep(c)).map_or(self.text.len(), |(offset, _)| from + offset)
    }

    fn code(&mut self) {
        let start = self.pos;
        let Some(c) = self.char_at(start) else {
            self.pos = self.text.len();
            return;
        };
        let next = self.bytes.get(start + 1).copied();
        if is_space(c) {
            self.pos += c.len_utf8();
            return;
        }
        let end = match c {
            '/' if next == Some(b'/') => {
                let end = self.line_end(start);
                self.push(start, end, Tok::Comment);
                end
            }
            '/' if next == Some(b'*') => {
                let end = self.text[start + 2..].find("*/").map_or(self.text.len(), |p| start + 2 + p + 2);
                let body = &self.text[start..end];
                let doc = body.starts_with("/**") && !body.starts_with("/**/");
                self.push(start, end, if doc { Tok::DocComment } else { Tok::Comment });
                end
            }
            '/' if self.regex_allowed() => match self.regex(start) {
                Some(end) => {
                    self.push(start, end, Tok::Regex);
                    end
                }
                None => {
                    self.push(start, start + 1, Tok::Punct);
                    start + 1
                }
            },
            '\'' | '"' => {
                let end = self.string(start);
                self.push(start, end, Tok::String);
                end
            }
            '`' => {
                self.pos = start + 1;
                self.template(start);
                return;
            }
            '0'..='9' => {
                let end = self.number(start);
                self.push(start, end, Tok::Number);
                end
            }
            '.' if next.is_some_and(|b| b.is_ascii_digit()) => {
                let end = self.number(start);
                self.push(start, end, Tok::Number);
                end
            }
            '#' if self.char_at(start + 1).is_some_and(is_ident_start) => {
                let end = self.scan_while(start + 1, is_ident_char);
                self.push(start, end, Tok::Private);
                end
            }
            c if is_ident_start(c) => {
                let end = self.scan_while(start, is_ident_char);
                self.push(start, end, Tok::Ident);
                end
            }
            '{' => {
                self.stack.push(Ctx::Brace);
                self.push(start, start + 1, Tok::Punct);
                start + 1
            }
            '}' => {
                match self.stack.last() {
                    Some(Ctx::TemplateExpr) => {
                        self.stack.pop();
                        self.push(start, start + 1, Tok::TemplateClose);
                        self.pos = start + 1;
                        self.template(start + 1);
                        return;
                    }
                    Some(Ctx::JsxExpr) => {
                        self.stack.pop();
                        self.push(start, start + 1, Tok::JsxClose);
                    }
                    Some(Ctx::Brace) => {
                        self.stack.pop();
                        self.push(start, start + 1, Tok::Punct);
                    }
                    _ => self.push(start, start + 1, Tok::Punct),
                }
                start + 1
            }
            '<' if self.regex_allowed() && self.jsx_ahead(start) => {
                self.starts.push(start);
                self.push(start, start + 1, Tok::JsxPunct);
                self.stack.push(Ctx::JsxTag { closing: false, named: false });
                start + 1
            }
            _ => {
                let rest = &self.text[start..];
                let len = if rest.starts_with("=>") {
                    2
                } else if rest.starts_with("...") {
                    3
                } else if rest.starts_with("?.") && !self.bytes.get(start + 2).is_some_and(u8::is_ascii_digit) {
                    2
                } else {
                    c.len_utf8()
                };
                self.push(start, start + len, Tok::Punct);
                start + len
            }
        };
        self.pos = end;
    }

    fn regex_allowed(&self) -> bool {
        let Some(last) = self.tokens.iter().rev().find(|t| !matches!(t.tok, Tok::Comment | Tok::DocComment)) else {
            return true;
        };
        let text = &self.text[last.start..last.end];
        match last.tok {
            Tok::Ident => REGEX_AFTER.contains(&text),
            Tok::Punct => !matches!(text, ")" | "]"),
            Tok::TemplateOpen | Tok::JsxOpen => true,
            _ => false,
        }
    }

    fn regex(&self, start: usize) -> Option<usize> {
        let mut p = start + 1;
        let mut class = false;
        loop {
            let b = *self.bytes.get(p)?;
            match b {
                b'\\' => {
                    p += 1;
                    if matches!(self.bytes.get(p), None | Some(b'\n' | b'\r')) {
                        return None;
                    }
                }
                b'[' => class = true,
                b']' => class = false,
                b'/' if !class => break,
                b'\n' | b'\r' => return None,
                _ => {}
            }
            p += 1;
        }
        p += 1;
        while !self.text.is_char_boundary(p) {
            p += 1;
        }
        Some(self.scan_while(p, is_ident_char))
    }

    fn string(&self, start: usize) -> usize {
        let quote = self.bytes[start];
        let mut p = start + 1;
        while p < self.bytes.len() {
            match self.bytes[p] {
                b'\\' => {
                    p += 1;
                    if self.bytes.get(p) == Some(&b'\r') && self.bytes.get(p + 1) == Some(&b'\n') {
                        p += 1;
                    }
                }
                b'\n' | b'\r' => return p,
                b if b == quote => return p + 1,
                _ => {}
            }
            p += 1;
        }
        self.bytes.len()
    }

    fn template(&mut self, chunk_start: usize) {
        let mut p = self.pos;
        while p < self.bytes.len() {
            match self.bytes[p] {
                b'\\' => p += 2,
                b'`' => {
                    self.push(chunk_start, p + 1, Tok::Template);
                    self.pos = p + 1;
                    return;
                }
                b'$' if self.bytes.get(p + 1) == Some(&b'{') => {
                    self.push(chunk_start, p, Tok::Template);
                    self.push(p, p + 2, Tok::TemplateOpen);
                    self.stack.push(Ctx::TemplateExpr);
                    self.pos = p + 2;
                    return;
                }
                _ => p += 1,
            }
        }
        self.push(chunk_start, self.bytes.len(), Tok::Template);
        self.pos = self.bytes.len();
    }

    fn number(&self, start: usize) -> usize {
        let b = self.bytes;
        let len = b.len();
        let mut p = start;
        let digit = |x: u8| x.is_ascii_digit() || x == b'_';
        if b[p] == b'0' && matches!(b.get(p + 1), Some(b'x' | b'X' | b'o' | b'O' | b'b' | b'B')) {
            p += 2;
            while p < len && (b[p].is_ascii_hexdigit() || b[p] == b'_') {
                p += 1;
            }
        } else {
            while p < len && digit(b[p]) {
                p += 1;
            }
            if p < len && b[p] == b'.' {
                p += 1;
                while p < len && digit(b[p]) {
                    p += 1;
                }
            }
            if p < len && matches!(b[p], b'e' | b'E') {
                let mut q = p + 1;
                if matches!(b.get(q), Some(b'+' | b'-')) {
                    q += 1;
                }
                if b.get(q).is_some_and(u8::is_ascii_digit) {
                    p = q;
                    while p < len && digit(b[p]) {
                        p += 1;
                    }
                }
            }
        }
        if p < len && b[p] == b'n' {
            p += 1;
        }
        p
    }

    fn skip_space(&self, from: usize) -> usize {
        self.scan_while(from, is_space)
    }

    fn occurs(&mut self, needle: String, from: usize) -> bool {
        match self.found.get(&needle) {
            Some(None) => return false,
            Some(Some(at)) if *at >= from => return true,
            _ => {}
        }
        let at = self.text[from..].find(&needle).map(|p| from + p);
        self.found.insert(needle, at);
        at.is_some()
    }

    fn closes_after(&mut self, name: &str, from: usize) -> bool {
        if self.closings.is_none() {
            let mut closings: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
            let mut at = 0;
            while let Some(found) = self.text[at..].find("</") {
                let start = at + found + 2;
                let end = self.scan_while(start, is_jsx_name_char);
                closings.entry(self.text[start..end].to_owned()).or_default().push(start - 2);
                at = start;
            }
            self.closings = Some(closings);
        }
        let positions = self.closings.as_ref().and_then(|closings| closings.get(name));
        positions.is_some_and(|positions| positions.last().is_some_and(|&last| last >= from))
    }

    fn jsx_ahead(&mut self, start: usize) -> bool {
        let p = start + 1;
        if self.bytes.get(p) == Some(&b'>') {
            return true;
        }
        if !self.char_at(p).is_some_and(is_ident_start) {
            return false;
        }
        let name_end = self.scan_while(p, is_jsx_name_char);
        let name = &self.text[p..name_end];
        let q = self.skip_space(name_end);
        match self.char_at(q) {
            Some('/') => self.bytes.get(q + 1) == Some(&b'>'),
            Some('>') => {
                if self.ts {
                    if TYPE_KEYWORDS.contains(&name) {
                        return false;
                    }
                    if self.char_at(self.skip_space(q + 1)) == Some('(') {
                        return false;
                    }
                }
                let name = name.to_owned();
                self.closes_after(&name, q)
            }
            Some(c) if c == '{' || is_ident_start(c) => {
                if self.ts {
                    let word_end = self.scan_while(q, is_ident_char);
                    if &self.text[q..word_end] == "extends" {
                        return false;
                    }
                }
                let name = name.to_owned();
                self.occurs("/>".to_owned(), q) || self.closes_after(&name, q)
            }
            _ => false,
        }
    }

    fn finish_element(&mut self, end: usize) {
        if let Some(start) = self.starts.pop() {
            self.elements.push((start, end));
        }
    }

    fn jsx_tag(&mut self) {
        let start = self.pos;
        let Some(c) = self.char_at(start) else {
            self.pos = self.text.len();
            return;
        };
        if is_space(c) {
            self.pos += c.len_utf8();
            return;
        }
        let Some(&Ctx::JsxTag { closing, named }) = self.stack.last() else {
            return;
        };
        match c {
            '/' if self.bytes.get(start + 1) == Some(&b'>') => {
                self.push(start, start + 2, Tok::JsxPunct);
                self.pos = start + 2;
                self.stack.pop();
                if closing && self.stack.last() == Some(&Ctx::JsxChildren) {
                    self.stack.pop();
                }
                self.finish_element(start + 2);
            }
            '>' => {
                self.push(start, start + 1, Tok::JsxPunct);
                self.pos = start + 1;
                self.stack.pop();
                if closing {
                    if self.stack.last() == Some(&Ctx::JsxChildren) {
                        self.stack.pop();
                    }
                    self.finish_element(start + 1);
                } else {
                    self.stack.push(Ctx::JsxChildren);
                }
            }
            '{' => {
                self.push(start, start + 1, Tok::JsxOpen);
                self.stack.push(Ctx::JsxExpr);
                self.pos = start + 1;
            }
            '"' | '\'' => {
                let end = self.text[start + 1..].find(c).map_or(self.text.len(), |p| start + 1 + p + 1);
                self.push(start, end, Tok::JsxString);
                self.pos = end;
            }
            c if is_ident_start(c) => {
                let end = self.scan_while(start, is_jsx_name_char);
                self.push(start, end, if named { Tok::JsxAttr } else { Tok::JsxTag });
                if let Some(Ctx::JsxTag { named, .. }) = self.stack.last_mut() {
                    *named = true;
                }
                self.pos = end;
            }
            c => {
                self.push(start, start + c.len_utf8(), Tok::JsxPunct);
                self.pos = start + c.len_utf8();
            }
        }
    }

    fn jsx_children(&mut self) {
        let start = self.pos;
        match self.bytes[start] {
            b'<' if self.bytes.get(start + 1) == Some(&b'>')
                || self.char_at(start + 1).is_some_and(|c| c == '/' || is_ident_start(c)) =>
            {
                if self.bytes.get(start + 1) == Some(&b'/') {
                    self.push(start, start + 2, Tok::JsxPunct);
                    self.pos = start + 2;
                    self.stack.push(Ctx::JsxTag { closing: true, named: false });
                } else {
                    self.starts.push(start);
                    self.push(start, start + 1, Tok::JsxPunct);
                    self.pos = start + 1;
                    self.stack.push(Ctx::JsxTag { closing: false, named: false });
                }
            }
            b'{' => {
                self.push(start, start + 1, Tok::JsxOpen);
                self.stack.push(Ctx::JsxExpr);
                self.pos = start + 1;
            }
            _ => {
                self.pos = self.bytes[start + 1..]
                    .iter()
                    .position(|&b| b == b'<' || b == b'{')
                    .map_or(self.bytes.len(), |p| start + 1 + p);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Block,
    Object,
    Class,
    Enum,
    Module,
    Interface,
    Params,
    ForHead,
    Paren,
    Bracket,
    Expr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Statement,
    Expression,
    AfterOperand,
    Member,
    Type,
    Nothing,
}

struct Declarator {
    name: Option<usize>,
    pattern: Option<usize>,
    value: Option<usize>,
    end: usize,
}

struct Analysis<'a> {
    text: &'a str,
    ts: bool,
    all: Vec<Token>,
    sig: Vec<Token>,
    elements: Vec<(usize, usize)>,
    matching: Vec<Option<usize>>,
    marks: Vec<Option<Kind>>,
    log: Vec<(usize, Option<Kind>)>,
    scopes: Vec<Option<Scope>>,
    next_brace: Option<Scope>,
    depth: usize,
    nesting: std::cell::Cell<usize>,
    globals: std::collections::HashSet<&'a str>,
}

impl<'a> Analysis<'a> {
    fn new(text: &'a str, dialect: Dialect) -> Self {
        let ts = dialect == Dialect::TypeScript;
        let (all, elements) = Lexer::new(text, ts).run();
        let sig: Vec<Token> =
            all.iter().copied().filter(|t| !matches!(t.tok, Tok::Comment | Tok::DocComment)).collect();
        let n = sig.len();
        let mut analysis = Analysis {
            text,
            ts,
            all,
            sig,
            elements,
            matching: vec![None; n],
            marks: vec![None; n],
            log: Vec::new(),
            scopes: vec![None; n],
            next_brace: None,
            nesting: std::cell::Cell::new(0),
            depth: 0,
            globals: std::collections::HashSet::new(),
        };
        analysis.match_brackets();
        analysis.globals = analysis.module_variables();
        analysis
    }

    fn match_brackets(&mut self) {
        let mut stack: Vec<(usize, u8)> = Vec::new();
        let mut open_of_kind = [0usize; 256];
        for i in 0..self.sig.len() {
            let Some((open, kind)) = self.bracket(i) else { continue };
            if open {
                stack.push((i, kind));
                open_of_kind[usize::from(kind)] += 1;
            } else if open_of_kind[usize::from(kind)] > 0
                && let Some(pos) = stack.iter().rposition(|&(_, k)| k == kind)
            {
                let opener = stack[pos].0;
                for &(_, closed) in &stack[pos..] {
                    open_of_kind[usize::from(closed)] -= 1;
                }
                stack.truncate(pos);
                self.matching[opener] = Some(i);
                self.matching[i] = Some(opener);
            }
        }
    }

    fn bracket(&self, i: usize) -> Option<(bool, u8)> {
        match self.sig[i].tok {
            Tok::Punct => match self.text(i) {
                "(" => Some((true, b'(')),
                ")" => Some((false, b'(')),
                "[" => Some((true, b'[')),
                "]" => Some((false, b'[')),
                "{" => Some((true, b'{')),
                "}" => Some((false, b'{')),
                _ => None,
            },
            Tok::TemplateOpen => Some((true, b't')),
            Tok::TemplateClose => Some((false, b't')),
            Tok::JsxOpen => Some((true, b'j')),
            Tok::JsxClose => Some((false, b'j')),
            _ => None,
        }
    }

    fn n(&self) -> usize {
        self.sig.len()
    }

    fn text(&self, i: usize) -> &'a str {
        self.sig.get(i).map_or("", |t| &self.text[t.start..t.end])
    }

    fn tok(&self, i: usize) -> Option<Tok> {
        self.sig.get(i).map(|t| t.tok)
    }

    fn is_ident(&self, i: usize) -> bool {
        self.tok(i) == Some(Tok::Ident)
    }

    fn is_word(&self, i: usize, word: &str) -> bool {
        self.is_ident(i) && self.text(i) == word
    }

    fn is_punct(&self, i: usize, punct: &str) -> bool {
        self.tok(i) == Some(Tok::Punct) && self.text(i) == punct
    }

    fn prev_is(&self, i: usize, punct: &str) -> bool {
        i > 0 && self.is_punct(i - 1, punct)
    }

    fn is_reserved(&self, i: usize) -> bool {
        self.is_ident(i) && RESERVED.contains(&self.text(i))
    }

    fn is_opener(&self, i: usize) -> bool {
        i < self.n() && self.bracket(i).is_some_and(|(open, _)| open)
    }

    fn is_closer(&self, i: usize) -> bool {
        i < self.n() && self.bracket(i).is_some_and(|(open, _)| !open)
    }

    fn close(&self, i: usize) -> usize {
        self.matching.get(i).copied().flatten().unwrap_or(self.n())
    }

    fn after_dot(&self, i: usize) -> bool {
        self.prev_is(i, ".") || self.prev_is(i, "?.")
    }

    fn same_line(&self, a: usize, b: usize) -> bool {
        match (self.sig.get(a), self.sig.get(b)) {
            (Some(x), Some(y)) if x.end <= y.start => !self.text[x.end..y.start].contains(['\n', '\r']),
            _ => true,
        }
    }

    fn operand(&self, i: usize) -> bool {
        let Some(t) = self.sig.get(i) else { return false };
        let text = self.text(i);
        match t.tok {
            Tok::Ident => !RESERVED.contains(&text) || matches!(text, "this" | "super"),
            Tok::Private | Tok::Number | Tok::String | Tok::Template | Tok::Regex => true,
            Tok::Punct => matches!(text, ")" | "]" | "}"),
            Tok::JsxPunct => text.ends_with('>'),
            _ => false,
        }
    }

    fn continues(&self, i: usize) -> bool {
        match self.tok(i) {
            Some(Tok::Punct) => !matches!(self.text(i), "{" | "}" | ";" | "!" | "~" | "@" | "#"),
            Some(Tok::Ident) => {
                matches!(self.text(i), "in" | "instanceof" | "of" | "as" | "satisfies" | "extends")
            }
            Some(Tok::Template | Tok::TemplateOpen) => true,
            _ => false,
        }
    }

    fn statement_start(&self, i: usize) -> bool {
        if i == 0 {
            return true;
        }
        let p = i - 1;
        self.is_punct(p, ";")
            || self.is_punct(p, "{")
            || self.is_punct(p, "}")
            || (!self.same_line(p, i) && self.operand(p) && !self.continues(i))
    }

    fn mark(&mut self, i: usize, kind: Kind) {
        if i < self.n() {
            self.log.push((i, self.marks[i]));
            self.marks[i] = Some(kind);
        }
    }

    fn rollback(&mut self, len: usize) {
        while self.log.len() > len {
            if let Some((i, old)) = self.log.pop() {
                self.marks[i] = old;
            }
        }
    }

    fn module_variables(&self) -> std::collections::HashSet<&'a str> {
        let (mut top, mut inner) = (std::collections::HashSet::new(), std::collections::HashSet::new());
        let mut depth = 0usize;
        let mut declaring: Option<usize> = None;
        for i in 0..self.n() {
            let mut declares = |name: usize, depth: usize| {
                let name: &'a str = &self.text[self.sig[name].start..self.sig[name].end];
                if depth == 0 {
                    top.insert(name)
                } else {
                    inner.insert(name)
                };
            };
            match self.sig[i].tok {
                Tok::Punct => match self.text(i) {
                    "{" | "(" | "[" => depth += 1,
                    "}" | ")" | "]" => depth = depth.saturating_sub(1),
                    ";" if declaring == Some(depth) => declaring = None,
                    "," if declaring == Some(depth) && self.is_ident(i + 1) && self.is_punct(i + 2, "=") => {
                        declares(i + 1, depth);
                    }
                    _ => {}
                },
                Tok::TemplateOpen | Tok::JsxOpen => depth += 1,
                Tok::TemplateClose | Tok::JsxClose => depth = depth.saturating_sub(1),
                Tok::Ident
                    if ["const", "let", "var"].iter().any(|word| self.is_word(i, word)) && self.is_ident(i + 1) =>
                {
                    declares(i + 1, depth);
                    declaring = Some(depth);
                }
                _ => {}
            }
        }
        top.retain(|name| !inner.contains(name) && !RESERVED.contains(name));
        top
    }

    fn declares_method(&self, i: usize) -> bool {
        let close = self.close(i + 1);
        self.is_punct(close + 1, "{") || (self.ts && self.is_punct(close + 1, ":"))
    }

    fn base_kind(&self, i: usize) -> Option<Kind> {
        let text = self.text(i);
        match self.sig[i].tok {
            Tok::Ident => {
                let key = (self.prev_is(i, "{") || self.prev_is(i, ",")) && self.is_punct(i + 1, ":");
                let called = self.is_punct(i + 1, "(") || self.is_punct(i + 1, "?.") && self.is_punct(i + 2, "(");
                if key {
                    Some(Kind::Property)
                } else if self.after_dot(i) {
                    called.then_some(Kind::Function)
                } else if LITERAL_KEYWORDS.contains(&text) {
                    Some(Kind::Keyword)
                } else if CONSTANTS.contains(&text) {
                    Some(Kind::GlobalVariable)
                } else if RESERVED.contains(&text) {
                    Some(Kind::Keyword)
                } else if (called && !self.declares_method(i))
                    || (GLOBAL_CLASSES.contains(&text) && self.is_punct(i + 1, "."))
                {
                    Some(Kind::Function)
                } else if GLOBALS.contains(&text) || self.globals.contains(text) {
                    Some(Kind::GlobalVariable)
                } else {
                    None
                }
            }
            Tok::Number => Some(Kind::Number),
            Tok::String | Tok::Template | Tok::Regex | Tok::JsxString => Some(Kind::String),
            Tok::Punct | Tok::JsxPunct | Tok::TemplateOpen | Tok::TemplateClose | Tok::JsxOpen | Tok::JsxClose => {
                Some(Kind::Punctuation)
            }
            Tok::JsxTag => {
                if text.starts_with(|c: char| c.is_ascii_uppercase()) {
                    Some(Kind::Type)
                } else {
                    Some(Kind::Keyword)
                }
            }
            Tok::JsxAttr => Some(Kind::Attribute),
            Tok::Private | Tok::Comment | Tok::DocComment => None,
        }
    }

    fn spans(&self) -> Vec<Span> {
        let mut spans = Vec::with_capacity(self.all.len());
        let mut index = 0;
        for t in &self.all {
            match t.tok {
                Tok::Comment => spans.push(Span { start: t.start, end: t.end, kind: Kind::Comment }),
                Tok::DocComment => self.doc_spans(*t, &mut spans),
                _ => {
                    if let Some(kind) = self.marks[index].or_else(|| self.base_kind(index)) {
                        spans.push(Span { start: t.start, end: t.end, kind });
                    }
                    index += 1;
                }
            }
        }
        spans
    }

    fn doc_spans(&self, t: Token, spans: &mut Vec<Span>) {
        let bytes = self.text.as_bytes();
        let mut piece = t.start;
        let mut p = t.start + 3;
        while p < t.end {
            let tag_start = bytes[p] == b'@'
                && matches!(bytes[p - 1], b' ' | b'\t' | b'\n' | b'\r' | b'*')
                && bytes.get(p + 1).is_some_and(u8::is_ascii_alphabetic);
            if tag_start {
                let mut end = p + 1;
                while end < t.end && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-') {
                    end += 1;
                }
                if p > piece {
                    spans.push(Span { start: piece, end: p, kind: Kind::DocComment });
                }
                spans.push(Span { start: p, end, kind: Kind::Attribute });
                piece = end;
                p = end;
            } else {
                p += 1;
            }
        }
        if t.end > piece {
            spans.push(Span { start: piece, end: t.end, kind: Kind::DocComment });
        }
    }

    fn mark_decorators(&mut self) {
        let mut i = 0;
        while i < self.n() {
            if self.is_punct(i, "@") && self.is_ident(i + 1) {
                self.marks[i] = Some(Kind::Attribute);
                self.marks[i + 1] = Some(Kind::Attribute);
                let mut j = i + 2;
                while self.is_punct(j, ".") && self.is_ident(j + 1) {
                    self.marks[j] = Some(Kind::Attribute);
                    self.marks[j + 1] = Some(Kind::Attribute);
                    j += 2;
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }

    fn skip_decorator(&self, i: usize) -> usize {
        let mut j = i + 1;
        if self.is_ident(j) {
            j += 1;
        }
        while self.is_punct(j, ".") && self.is_ident(j + 1) {
            j += 2;
        }
        if self.is_punct(j, "(") {
            j = self.close(j) + 1;
        }
        j
    }

    fn walk(&mut self) {
        self.mark_decorators();
        let mut stack: Vec<(usize, Scope)> = Vec::new();
        let mut i = 0;
        while i < self.n() {
            if let Some(&(open, Scope::Class)) = stack.last()
                && self.member_start(i, open)
            {
                i = self.class_member(i).max(i + 1);
                continue;
            }
            match self.sig[i].tok {
                Tok::Ident => i = self.ident(i, stack.last().map(|s| s.1)),
                Tok::Punct => match self.text(i) {
                    "(" => {
                        let params = self.is_params(i);
                        if params && self.ts {
                            self.scopes[i] = Some(Scope::Params);
                            i = self.params(i);
                            if self.is_punct(i, ":") {
                                i = self.type_at(i + 1);
                            }
                            continue;
                        }
                        let head = i > 0
                            && (self.is_word(i - 1, "for")
                                || (i > 1 && self.is_word(i - 1, "await") && self.is_word(i - 2, "for")));
                        let scope = if params {
                            Scope::Params
                        } else if head {
                            Scope::ForHead
                        } else {
                            Scope::Paren
                        };
                        self.scopes[i] = Some(scope);
                        stack.push((i, scope));
                        i += 1;
                    }
                    "[" => {
                        self.scopes[i] = Some(Scope::Bracket);
                        stack.push((i, Scope::Bracket));
                        i += 1;
                    }
                    "{" => {
                        let scope = self
                            .next_brace
                            .take()
                            .or(self.scopes[i])
                            .unwrap_or_else(|| if self.expression_start(i) { Scope::Object } else { Scope::Block });
                        self.scopes[i] = Some(scope);
                        stack.push((i, scope));
                        i += 1;
                    }
                    ")" | "]" | "}" => {
                        Self::pop_to(&mut stack, self.matching[i]);
                        i += 1;
                    }
                    "<" if self.ts => i = self.angle(i),
                    _ => i += 1,
                },
                Tok::TemplateOpen | Tok::JsxOpen => {
                    self.scopes[i] = Some(Scope::Expr);
                    stack.push((i, Scope::Expr));
                    i += 1;
                }
                Tok::TemplateClose | Tok::JsxClose => {
                    Self::pop_to(&mut stack, self.matching[i]);
                    i += 1;
                }
                _ => i += 1,
            }
        }
    }

    fn pop_to(stack: &mut Vec<(usize, Scope)>, opener: Option<usize>) {
        if let Some(opener) = opener
            && let Some(pos) = stack.iter().rposition(|&(x, _)| x == opener)
        {
            stack.truncate(pos);
        }
    }

    fn expression_start(&self, i: usize) -> bool {
        if i == 0 {
            return false;
        }
        let p = i - 1;
        let text = self.text(p);
        match self.sig[p].tok {
            Tok::Punct => matches!(
                text,
                "=" | "("
                    | ","
                    | ":"
                    | "["
                    | "?"
                    | "..."
                    | "|"
                    | "&"
                    | "!"
                    | "+"
                    | "-"
                    | "*"
                    | "/"
                    | "%"
                    | "<"
                    | ">"
                    | "^"
                    | "~"
            ),
            Tok::TemplateOpen | Tok::JsxOpen => true,
            Tok::Ident => matches!(
                text,
                "return" | "typeof" | "in" | "of" | "yield" | "await" | "new" | "delete" | "void" | "throw" | "default"
            ),
            _ => false,
        }
    }

    fn member_start(&self, i: usize, open: usize) -> bool {
        if i <= open {
            return false;
        }
        let name_like = match self.sig[i].tok {
            Tok::Ident | Tok::Private | Tok::String | Tok::Number => true,
            Tok::Punct => matches!(self.text(i), "[" | "*" | "@"),
            _ => false,
        };
        if !name_like {
            return false;
        }
        let p = i - 1;
        p == open
            || self.is_punct(p, ";")
            || self.is_punct(p, "}")
            || (!self.same_line(p, i) && self.operand(p) && !self.continues(i))
    }

    fn member_name_after(&self, j: usize) -> bool {
        match self.tok(j) {
            Some(Tok::Ident | Tok::Private | Tok::String | Tok::Number) => true,
            Some(Tok::Punct) => matches!(self.text(j), "[" | "*"),
            _ => false,
        }
    }

    fn class_member(&mut self, i: usize) -> usize {
        let mut j = i;
        while self.is_punct(j, "@") {
            j = self.skip_decorator(j);
        }
        if self.is_word(j, "static") && self.is_punct(j + 1, "{") {
            return j + 1;
        }
        while self.is_ident(j) && MODIFIERS.contains(&self.text(j)) && self.member_name_after(j + 1) {
            self.mark(j, Kind::Keyword);
            j += 1;
        }
        if self.is_punct(j, "*") {
            j += 1;
        }
        let name = j;
        match self.tok(j) {
            Some(Tok::Ident | Tok::Private | Tok::String | Tok::Number) => j += 1,
            _ if self.is_punct(j, "[") => j = self.close(j) + 1,
            _ => return j,
        }
        let named = matches!(self.tok(name), Some(Tok::Ident | Tok::Private));
        if self.is_punct(j, "?") || self.is_punct(j, "!") {
            j += 1;
        }
        if self.is_punct(j, "(") || self.is_punct(j, "<") {
            if named {
                self.mark(name, Kind::Function);
            }
            if self.ts {
                if self.is_punct(j, "<") {
                    j = self.type_params(j);
                }
                if self.is_punct(j, "(") {
                    self.scopes[j] = Some(Scope::Params);
                    j = self.params(j);
                    if self.is_punct(j, ":") {
                        j = self.type_at(j + 1);
                    }
                }
            }
            return j;
        }
        if self.ts && self.is_punct(j, ":") {
            j = self.type_at(j + 1);
        }
        if named && self.is_punct(j, "=") && self.is_function_value(j + 1) {
            self.mark(name, Kind::Function);
        } else if named {
            self.mark(name, Kind::Property);
        }
        j
    }

    fn mark_module_brace(&mut self, i: usize) {
        let mut j = i + 1;
        while j < self.n() {
            if self.is_punct(j, "{") {
                self.scopes[j] = Some(Scope::Module);
                return;
            }
            let word = self.text(j);
            let plain = self.is_ident(j)
                && (word == "type"
                    || !(RESERVED.contains(&word)
                        || matches!(word, "namespace" | "module" | "declare" | "abstract" | "async" | "global")));
            if plain || self.is_punct(j, ",") || self.is_punct(j, "*") {
                j += 1;
            } else {
                return;
            }
        }
    }

    fn ident(&mut self, i: usize, top: Option<Scope>) -> usize {
        let word = self.text(i);
        if self.after_dot(i) {
            return i + 1;
        }
        if self.is_punct(i + 1, ":") && matches!(top, Some(Scope::Object | Scope::Module)) {
            if self.prev_is(i, "{") || self.prev_is(i, ",") {
                self.mark(i, Kind::Property);
            }
            return i + 1;
        }
        let after_separator = self.prev_is(i, "{") || self.prev_is(i, ",");
        if top == Some(Scope::Enum) && after_separator {
            self.mark(i, Kind::Constant);
            return i + 1;
        }
        if top == Some(Scope::Object) {
            let lead = after_separator
                || self.prev_is(i, "*")
                || (i > 0 && matches!(self.text(i - 1), "async" | "get" | "set") && self.is_ident(i - 1));
            if lead && self.is_punct(i + 1, "(") && !RESERVED.contains(&word) {
                self.mark(i, Kind::Function);
                return i + 1;
            }
            if after_separator && matches!(word, "get" | "set" | "async") && self.member_name_after(i + 1) {
                self.mark(i, Kind::Keyword);
                return i + 1;
            }
        }
        let operand_before = i > 0
            && self.operand(i - 1)
            && self.same_line(i - 1, i)
            && !matches!(self.text(i - 1), "declare" | "abstract" | "async");
        match word {
            "function" => {
                let mut j = i + 1;
                if self.is_punct(j, "*") {
                    j += 1;
                }
                if self.is_ident(j) && !self.is_reserved(j) {
                    self.mark(j, Kind::Function);
                    j += 1;
                }
                if self.ts {
                    if self.is_punct(j, "<") {
                        j = self.type_params(j);
                    }
                    if self.is_punct(j, "(") {
                        self.scopes[j] = Some(Scope::Params);
                        j = self.params(j);
                        if self.is_punct(j, ":") {
                            j = self.type_at(j + 1);
                        }
                    }
                }
                j.max(i + 1)
            }
            "class" => {
                let mut j = i + 1;
                if self.is_ident(j) && !matches!(self.text(j), "extends" | "implements") {
                    self.mark(j, Kind::Type);
                    j += 1;
                }
                if self.ts && self.is_punct(j, "<") {
                    j = self.type_params(j);
                }
                if self.is_word(j, "extends") {
                    j += 1;
                    if self.is_ident(j) && !self.is_reserved(j) {
                        self.mark(j, Kind::Type);
                        j += 1;
                        while self.is_punct(j, ".") && self.is_ident(j + 1) {
                            self.mark(j + 1, Kind::Type);
                            j += 2;
                        }
                    }
                    if self.ts && self.is_punct(j, "<") {
                        j = self.type_args(j).0;
                    }
                }
                if self.ts && self.is_word(j, "implements") {
                    j = self.type_list(j + 1);
                }
                self.next_brace = Some(Scope::Class);
                j
            }
            "interface" if self.ts && self.is_ident(i + 1) && !self.is_reserved(i + 1) => {
                self.mark(i + 1, Kind::Type);
                let mut j = i + 2;
                if self.is_punct(j, "<") {
                    j = self.type_params(j);
                }
                if self.is_word(j, "extends") {
                    j = self.type_list(j + 1);
                }
                if self.is_punct(j, "{") {
                    self.scopes[j] = Some(Scope::Interface);
                    j = self.type_members(j);
                }
                j
            }
            "type"
                if self.ts
                    && self.is_ident(i + 1)
                    && (self.is_punct(i + 2, "=") || self.is_punct(i + 2, "<"))
                    && !operand_before =>
            {
                self.mark(i, Kind::Keyword);
                self.mark(i + 1, Kind::Type);
                let mut j = i + 2;
                if self.is_punct(j, "<") {
                    j = self.type_params(j);
                }
                if self.is_punct(j, "=") {
                    j = self.type_at(j + 1);
                }
                j
            }
            "type" if self.ts && i > 0 && (self.is_word(i - 1, "import") || self.is_word(i - 1, "export")) => {
                self.mark(i, Kind::Keyword);
                i + 1
            }
            "enum" if self.is_ident(i + 1) && self.is_punct(i + 2, "{") => {
                self.mark(i + 1, Kind::Type);
                self.next_brace = Some(Scope::Enum);
                i + 2
            }
            "namespace" | "module"
                if self.ts
                    && (self.is_ident(i + 1) || self.tok(i + 1) == Some(Tok::String))
                    && self.same_line(i, i + 1)
                    && !operand_before =>
            {
                self.mark(i, Kind::Keyword);
                let mut j = i + 1;
                if self.is_ident(j) {
                    self.mark(j, Kind::Type);
                    j += 1;
                    while self.is_punct(j, ".") && self.is_ident(j + 1) {
                        self.mark(j + 1, Kind::Type);
                        j += 2;
                    }
                } else {
                    j += 1;
                }
                j
            }
            "global" if self.ts && self.is_punct(i + 1, "{") && i > 0 && self.is_word(i - 1, "declare") => {
                self.mark(i, Kind::Keyword);
                i + 1
            }
            "declare" | "abstract" | "readonly"
                if self.ts && self.is_ident(i + 1) && self.same_line(i, i + 1) && !operand_before =>
            {
                self.mark(i, Kind::Keyword);
                i + 1
            }
            "let" | "const" | "var" => self.binding(i),
            "as" | "satisfies" => {
                if word == "as" && (self.prev_is(i, "*") || top == Some(Scope::Module)) {
                    self.mark(i, Kind::Keyword);
                    return i + 1;
                }
                if self.ts && operand_before {
                    self.mark(i, Kind::Keyword);
                    if self.is_word(i + 1, "const") {
                        return i + 2;
                    }
                    return self.type_at(i + 1);
                }
                i + 1
            }
            "async" => {
                let next_line = self.same_line(i, i + 1);
                if next_line
                    && (self.is_word(i + 1, "function")
                        || self.is_punct(i + 1, "(")
                        || (self.is_ident(i + 1) && self.is_punct(i + 2, "=>"))
                        || (self.ts && self.is_punct(i + 1, "<")))
                {
                    self.mark(i, Kind::Keyword);
                }
                i + 1
            }
            "of" if top == Some(Scope::ForHead) => {
                self.mark(i, Kind::Keyword);
                i + 1
            }
            "from"
                if self.tok(i + 1) == Some(Tok::String)
                    && i > 0
                    && (self.prev_is(i, "}") || self.prev_is(i, "*") || self.is_ident(i - 1)) =>
            {
                self.mark(i, Kind::Keyword);
                i + 1
            }
            "import" | "export" => {
                self.mark_module_brace(i);
                i + 1
            }
            "new" if self.is_ident(i + 1) && !self.is_reserved(i + 1) => {
                let mut j = i + 1;
                while self.is_punct(j + 1, ".") && self.is_ident(j + 2) {
                    self.mark(j, Kind::Type);
                    j += 2;
                }
                self.mark(j, Kind::Function);
                j + 1
            }
            _ => i + 1,
        }
    }

    fn binding(&mut self, i: usize) -> usize {
        let mut j = i + 1;
        if self.text(i) == "const" && self.is_word(j, "enum") {
            return j;
        }
        let name = if self.is_ident(j) && !self.is_reserved(j) {
            j += 1;
            Some(j - 1)
        } else if self.is_punct(j, "{") || self.is_punct(j, "[") {
            j = self.close(j) + 1;
            None
        } else {
            return i + 1;
        };
        if self.ts && self.is_punct(j, "!") {
            j += 1;
        }
        if self.ts && self.is_punct(j, ":") {
            j = self.type_at(j + 1);
        }
        if let Some(name) = name
            && self.is_punct(j, "=")
            && self.is_function_value(j + 1)
        {
            self.mark(name, Kind::Function);
        }
        j
    }

    fn angle(&mut self, i: usize) -> usize {
        let save = self.log.len();
        if i > 0 && self.is_ident(i - 1) && !self.is_reserved(i - 1) {
            let (end, ok) = self.type_args(i);
            if ok && (self.is_punct(end, "(") || self.tok(end) == Some(Tok::Template)) {
                return end;
            }
            self.rollback(save);
            return i + 1;
        }
        if self.expression_start(i) && self.is_ident(i + 1) {
            let j = self.type_params(i);
            if j > i + 1 && self.is_punct(j - 1, ">") && self.is_punct(j, "(") {
                self.scopes[j] = Some(Scope::Params);
                let mut k = self.params(j);
                if self.is_punct(k, ":") {
                    k = self.type_at(k + 1);
                }
                return k;
            }
            self.rollback(save);
            let (end, ok) = self.type_args(i);
            let operand = matches!(self.tok(end), Some(Tok::Ident | Tok::Number | Tok::String | Tok::Template))
                || self.is_punct(end, "(")
                || self.is_punct(end, "[");
            if ok && operand {
                return end;
            }
            self.rollback(save);
        }
        i + 1
    }

    fn is_params(&self, i: usize) -> bool {
        if i == 0 {
            return false;
        }
        let p = i - 1;
        if self.marks[p] == Some(Kind::Function) {
            return true;
        }
        if self.is_word(p, "function")
            || (p > 0 && self.is_punct(p, "*") && self.is_word(p - 1, "function"))
            || self.is_word(p, "catch")
        {
            return true;
        }
        let m = self.close(i);
        if self.is_punct(m + 1, "=>") {
            return true;
        }
        self.ts
            && self.is_punct(m + 1, ":")
            && !self.prev_is(i, "?")
            && !self.prev_is(i, ":")
            && (self.expression_start(i) || self.is_word(p, "async") || self.is_punct(p, "=>"))
    }

    fn is_function_value(&self, j: usize) -> bool {
        let mut k = j;
        if self.is_word(k, "async") {
            k += 1;
        }
        if self.is_word(k, "function") {
            return true;
        }
        if self.ts && self.is_punct(k, "<") {
            let mut depth = 0usize;
            let limit = (k + 256).min(self.n());
            while k < limit {
                if self.is_punct(k, "<") {
                    depth += 1;
                } else if self.is_punct(k, ">") {
                    depth -= 1;
                    if depth == 0 {
                        k += 1;
                        break;
                    }
                } else if self.is_opener(k) {
                    k = self.close(k);
                }
                k += 1;
            }
        }
        if self.is_punct(k, "(") {
            let m = self.close(k);
            return self.is_punct(m + 1, "=>") || (self.ts && self.is_punct(m + 1, ":"));
        }
        self.is_ident(k) && self.is_punct(k + 1, "=>")
    }

    fn params(&mut self, i: usize) -> usize {
        let m = self.close(i);
        let mut j = i + 1;
        while j < m {
            let start = j;
            while self.is_punct(j, "@") {
                j = self.skip_decorator(j);
            }
            while self.is_ident(j)
                && PARAM_MODIFIERS.contains(&self.text(j))
                && (self.is_ident(j + 1) || self.is_punct(j + 1, "{") || self.is_punct(j + 1, "["))
            {
                self.mark(j, Kind::Keyword);
                j += 1;
            }
            if self.is_punct(j, "...") {
                j += 1;
            }
            if self.is_ident(j) {
                j += 1;
            } else if self.is_punct(j, "{") || self.is_punct(j, "[") {
                j = self.close(j) + 1;
            }
            if self.is_punct(j, "?") {
                j += 1;
            }
            if self.ts && self.is_punct(j, ":") {
                j = self.type_at(j + 1);
            }
            if self.is_punct(j, "=") {
                j += 1;
                while j < m && !self.is_punct(j, ",") {
                    j = if self.is_opener(j) { self.close(j) + 1 } else { j + 1 };
                }
            }
            if self.is_punct(j, ",") || j == start {
                j += 1;
            }
        }
        m + 1
    }

    fn type_at(&mut self, i: usize) -> usize {
        if self.depth > 48 {
            return i;
        }
        self.depth += 1;
        let end = self.type_inner(i);
        self.depth -= 1;
        end
    }

    fn type_operand_after(&self, i: usize) -> bool {
        match self.tok(i) {
            Some(Tok::Ident | Tok::String | Tok::Number | Tok::Template) => true,
            Some(Tok::Punct) => matches!(self.text(i), "(" | "[" | "{" | "<"),
            _ => false,
        }
    }

    fn type_inner(&mut self, mut i: usize) -> usize {
        let mut operand = false;
        let mut conditional = 0usize;
        while i < self.n() {
            let tok = self.sig[i].tok;
            let text = self.text(i);
            if !operand {
                match tok {
                    Tok::Ident => {
                        if TYPE_PREFIXES.contains(&text) && self.type_operand_after(i + 1) {
                            self.mark(i, Kind::Keyword);
                            i += 1;
                            continue;
                        }
                        if self.is_word(i + 1, "is") {
                            self.mark(i + 1, Kind::Keyword);
                            i += 2;
                            continue;
                        }
                        if TYPE_KEYWORDS.contains(&text) {
                            self.mark(i, Kind::Keyword);
                        } else if !CONSTANTS.contains(&text) {
                            self.mark(i, Kind::Type);
                        }
                        i += 1;
                        while self.is_punct(i, ".") && self.is_ident(i + 1) {
                            self.mark(i + 1, Kind::Type);
                            i += 2;
                        }
                        operand = true;
                    }
                    Tok::String | Tok::Number | Tok::Template => {
                        i += 1;
                        operand = true;
                    }
                    Tok::Punct => match text {
                        "(" => {
                            let m = self.close(i);
                            if self.is_punct(m + 1, "=>") {
                                i = self.params(i);
                                i = self.type_at(i + 1);
                            } else {
                                let j = self.type_at(i + 1);
                                i = if self.is_punct(j, ")") { j + 1 } else { m + 1 };
                            }
                            operand = true;
                        }
                        "[" => {
                            i = self.tuple(i);
                            operand = true;
                        }
                        "{" => {
                            i = self.type_members(i);
                            operand = true;
                        }
                        "|" | "&" | "-" | "..." => i += 1,
                        "<" => i = self.type_params(i),
                        _ => break,
                    },
                    _ => break,
                }
            } else {
                match tok {
                    Tok::Punct => match text {
                        "<" => i = self.type_args(i).0,
                        "[" if self.same_line(i - 1, i) => {
                            if self.is_punct(i + 1, "]") {
                                i += 2;
                            } else {
                                let j = self.type_at(i + 1);
                                i = if self.is_punct(j, "]") { j + 1 } else { self.close(i) + 1 };
                            }
                        }
                        "|" | "&" => {
                            i += 1;
                            operand = false;
                        }
                        "?" if conditional > 0 => {
                            i += 1;
                            operand = false;
                        }
                        ":" if conditional > 0 => {
                            conditional -= 1;
                            i += 1;
                            operand = false;
                        }
                        _ => break,
                    },
                    Tok::Ident if text == "extends" => {
                        self.mark(i, Kind::Keyword);
                        conditional += 1;
                        i += 1;
                        operand = false;
                    }
                    Tok::TemplateOpen => {
                        let j = self.type_at(i + 1);
                        i = if self.tok(j) == Some(Tok::TemplateClose) { j + 1 } else { self.close(i) + 1 };
                    }
                    Tok::Template if self.tok(i - 1) == Some(Tok::TemplateClose) => i += 1,
                    _ => break,
                }
            }
        }
        i
    }

    fn tuple(&mut self, i: usize) -> usize {
        let m = self.close(i);
        let mut j = i + 1;
        while j < m {
            if self.is_punct(j, "...") {
                j += 1;
            }
            if self.is_ident(j) && self.is_punct(j + 1, ":") {
                j += 2;
            } else if self.is_ident(j) && self.is_punct(j + 1, "?") && self.is_punct(j + 2, ":") {
                j += 3;
            }
            let k = self.type_at(j);
            let k = if self.is_punct(k, "?") { k + 1 } else { k };
            if self.is_punct(k, ",") {
                j = k + 1;
            } else {
                break;
            }
        }
        m + 1
    }

    fn type_args(&mut self, i: usize) -> (usize, bool) {
        let mut j = i + 1;
        loop {
            if self.is_punct(j, ">") {
                return (j + 1, true);
            }
            let k = self.type_at(j);
            if k == j {
                return (j, false);
            }
            if self.is_punct(k, ",") {
                j = k + 1;
            } else if self.is_punct(k, ">") {
                return (k + 1, true);
            } else {
                return (k, false);
            }
        }
    }

    fn type_params(&mut self, i: usize) -> usize {
        let mut j = i + 1;
        loop {
            if self.is_punct(j, ">") {
                return j + 1;
            }
            if self.is_punct(j, ",") {
                j += 1;
                continue;
            }
            if self.is_ident(j) && matches!(self.text(j), "in" | "out" | "const") && self.is_ident(j + 1) {
                self.mark(j, Kind::Keyword);
                j += 1;
                continue;
            }
            if !self.is_ident(j) {
                return j;
            }
            self.mark(j, Kind::Type);
            j += 1;
            if self.is_word(j, "extends") {
                self.mark(j, Kind::Keyword);
                j = self.type_at(j + 1);
            }
            if self.is_punct(j, "=") {
                j = self.type_at(j + 1);
            }
        }
    }

    fn type_list(&mut self, mut i: usize) -> usize {
        loop {
            let k = self.type_at(i);
            if k == i {
                return i;
            }
            if self.is_punct(k, ",") {
                i = k + 1;
            } else {
                return k;
            }
        }
    }

    fn type_members(&mut self, i: usize) -> usize {
        let m = self.close(i);
        let mut j = i + 1;
        while j < m {
            if self.is_punct(j, ";") || self.is_punct(j, ",") || self.is_punct(j, "+") || self.is_punct(j, "-") {
                j += 1;
                continue;
            }
            if self.is_ident(j)
                && matches!(self.text(j), "readonly" | "get" | "set")
                && (self.is_ident(j + 1)
                    || self.is_punct(j + 1, "[")
                    || matches!(self.tok(j + 1), Some(Tok::String | Tok::Number)))
            {
                self.mark(j, Kind::Keyword);
                j += 1;
                continue;
            }
            let start = j;
            if self.is_punct(j, "[") {
                let c = self.close(j);
                let k = j + 1;
                if self.is_ident(k) && self.is_punct(k + 1, ":") {
                    self.type_at(k + 2);
                } else if self.is_ident(k) && self.is_word(k + 1, "in") {
                    self.mark(k, Kind::Type);
                    let e = self.type_at(k + 2);
                    if self.is_word(e, "as") {
                        self.mark(e, Kind::Keyword);
                        self.type_at(e + 1);
                    }
                } else {
                    self.type_at(k);
                }
                j = c + 1;
                if self.is_punct(j, "-") || self.is_punct(j, "+") {
                    j += 1;
                }
            } else if matches!(self.tok(j), Some(Tok::Ident | Tok::String | Tok::Number | Tok::Private)) {
                let call = self.is_punct(j + 1, "(")
                    || self.is_punct(j + 1, "<")
                    || (self.is_punct(j + 1, "?") && (self.is_punct(j + 2, "(") || self.is_punct(j + 2, "<")));
                if self.is_ident(j) && !self.is_reserved(j) && call {
                    self.mark(j, Kind::Function);
                } else if !call && matches!(self.tok(j), Some(Tok::Ident | Tok::Private)) {
                    self.mark(j, Kind::Property);
                }
                j += 1;
            } else if !(self.is_punct(j, "(") || self.is_punct(j, "<")) {
                j += 1;
                continue;
            }
            if self.is_punct(j, "?") {
                j += 1;
            }
            if self.is_punct(j, "<") {
                j = self.type_params(j);
            }
            if self.is_punct(j, "(") {
                j = self.params(j);
            }
            if self.is_punct(j, ":") {
                j = self.type_at(j + 1);
            }
            if j == start {
                j += 1;
            }
        }
        m + 1
    }

    fn line_starts(&self) -> Vec<usize> {
        let bytes = self.text.as_bytes();
        let mut starts = vec![0];
        let mut p = 0;
        while p < bytes.len() {
            match bytes[p] {
                b'\n' => starts.push(p + 1),
                b'\r' if bytes.get(p + 1) != Some(&b'\n') => starts.push(p + 1),
                _ => {}
            }
            p += 1;
        }
        starts
    }

    fn folds(&self) -> Vec<Fold> {
        let starts = self.line_starts();
        let line = |byte: usize| (starts.partition_point(|&s| s <= byte) - 1) as u32;
        let mut folds = Vec::new();
        let mut add = |first: usize, last: usize| {
            let (start_line, end_line) = (line(first), line(last));
            if end_line > start_line {
                folds.push(Fold { start_line, end_line });
            }
        };
        for i in 0..self.n() {
            let fold = self.is_punct(i, "{") || self.is_punct(i, "[") || self.tok(i) == Some(Tok::JsxOpen);
            if fold && let Some(c) = self.matching[i] {
                add(self.sig[i].start, self.sig[c].start);
            }
        }
        for t in &self.all {
            if matches!(t.tok, Tok::Comment | Tok::DocComment) && self.text[t.start..].starts_with("/*") {
                add(t.start, t.end - 1);
            }
        }
        for &(start, end) in &self.elements {
            add(start, end - 1);
        }
        let mut group: Option<(usize, usize)> = None;
        let mut i = 0;
        while i < self.n() {
            let import = self.is_word(i, "import")
                && !self.is_punct(i + 1, "(")
                && !self.is_punct(i + 1, ".")
                && self.statement_start(i);
            if !import {
                i += 1;
                continue;
            }
            let end = self.import_end(i);
            group = match group {
                Some((first, last)) if last + 1 == i => Some((first, end)),
                Some((first, last)) => {
                    add(self.sig[first].start, self.sig[last].end - 1);
                    Some((i, end))
                }
                None => Some((i, end)),
            };
            i = end + 1;
        }
        if let Some((first, last)) = group {
            add(self.sig[first].start, self.sig[last].end - 1);
        }
        folds.sort_unstable();
        folds.dedup();
        folds
    }

    fn import_end(&self, i: usize) -> usize {
        let last = self.n() - 1;
        let mut j = i + 1;
        loop {
            if j > last {
                return last;
            }
            if self.is_opener(j) {
                j = self.close(j);
                if j > last {
                    return last;
                }
            } else if self.tok(j) == Some(Tok::String) {
                return if self.is_punct(j + 1, ";") { j + 1 } else { j };
            } else if self.is_punct(j, ";") {
                return j;
            } else if !self.same_line(j - 1, j) && self.statement_start(j) {
                return j - 1;
            }
            j += 1;
        }
    }

    fn declarator_end(&self, first: usize, to: usize, comma: bool) -> usize {
        let to = to.min(self.n());
        let mut k = first;
        let mut angles = 0usize;
        loop {
            if k >= to {
                return to.saturating_sub(1).max(first - 1);
            }
            if self.ts && self.is_punct(k, "<") {
                angles += 1;
            } else if self.ts && self.is_punct(k, ">") {
                angles = angles.saturating_sub(1);
            }
            if (comma && angles == 0 && self.is_punct(k, ",")) || self.is_punct(k, ";") || self.is_closer(k) {
                return k - 1;
            }
            if !self.same_line(k - 1, k) && self.operand(k - 1) && !self.continues(k) {
                return k - 1;
            }
            k = if self.is_opener(k) { self.close(k) + 1 } else { k + 1 };
        }
    }

    fn declarators(&self, j: usize, to: usize) -> Vec<Declarator> {
        let mut out = Vec::new();
        let mut k = j + 1;
        loop {
            let (name, pattern) = if self.is_ident(k) && !self.is_reserved(k) {
                k += 1;
                (Some(k - 1), None)
            } else if self.is_punct(k, "{") || self.is_punct(k, "[") {
                let open = k;
                k = self.close(k) + 1;
                (None, Some(open))
            } else {
                break;
            };
            let end = self.declarator_end(k, to, true);
            let mut value = None;
            let mut x = k;
            while x <= end {
                if self.is_punct(x, "=") {
                    value = Some(x + 1);
                    break;
                }
                x = if self.is_opener(x) { self.close(x) + 1 } else { x + 1 };
            }
            out.push(Declarator { name, pattern, value, end });
            if end + 1 < to && self.is_punct(end + 1, ",") {
                k = end + 2;
            } else {
                break;
            }
        }
        out
    }

    fn skip(&self, i: usize) -> usize {
        if self.is_opener(i) { self.close(i) + 1 } else { i + 1 }
    }

    fn symbol(&self, name: String, kind: SymbolKind, first: usize, last: usize, children: Vec<Symbol>) -> Symbol {
        let last = last.min(self.n() - 1).max(first);
        Symbol { name, kind, start: self.sig[first].start, end: self.sig[last].end, children }
    }

    fn name_text(&self, i: usize) -> String {
        let text = self.text(i);
        if self.tok(i) == Some(Tok::String) && text.len() >= 2 {
            let quote = &text[..1];
            let inner = &text[1..];
            inner.strip_suffix(quote).unwrap_or(inner).to_owned()
        } else {
            text.to_owned()
        }
    }

    fn find_body(&self, mut k: usize, to: usize) -> Option<usize> {
        while k < to {
            if self.is_punct(k, "{") {
                return Some(k);
            }
            if self.is_punct(k, ";") || self.is_closer(k) {
                return None;
            }
            k = if self.is_punct(k, "(") || self.is_punct(k, "[") { self.close(k) + 1 } else { k + 1 };
        }
        None
    }

    fn items(&self, from: usize, to: usize) -> Vec<Symbol> {
        let mut out = Vec::new();
        let mut i = from;
        while i < to {
            if !(i == from || self.statement_start(i)) {
                i = self.skip(i);
                continue;
            }
            let first = i;
            let mut j = i;
            while self.is_punct(j, "@") {
                j = self.skip_decorator(j);
            }
            while j < to && matches!(self.text(j), "export" | "default" | "declare" | "abstract" | "async") {
                j += 1;
            }
            if self.is_word(j, "const") && self.is_word(j + 1, "enum") {
                j += 1;
            }
            let found = match self.text(j) {
                "function" if self.is_ident(j) => self.function_symbol(first, j, to),
                "class" if self.is_ident(j) => self.class_symbol(first, j, to),
                "interface" if self.ts && self.is_ident(j + 1) => self.interface_symbol(first, j, to),
                "type" if self.ts && self.is_ident(j + 1) => {
                    let end = self.declarator_end(j + 1, to, false);
                    let name = self.text(j + 1).to_owned();
                    Some((self.symbol(name, SymbolKind::Type, first, end, Vec::new()), end + 1))
                }
                "enum" if self.is_ident(j + 1) => self.enum_symbol(first, j, to),
                "namespace" | "module" | "global"
                    if self.ts
                        && (self.is_ident(j + 1)
                            || self.tok(j + 1) == Some(Tok::String)
                            || self.is_punct(j + 1, "{")) =>
                {
                    self.module_symbol(first, j, to)
                }
                "const" | "let" | "var" if self.is_ident(j) => {
                    let constant = self.text(j) == "const";
                    let mut last = j;
                    for (index, d) in self.declarators(j, to).into_iter().enumerate() {
                        last = d.end;
                        let Some(name) = d.name else { continue };
                        let kind = if d.value.is_some_and(|v| self.is_function_value(v)) {
                            SymbolKind::Function
                        } else if constant {
                            SymbolKind::Constant
                        } else {
                            SymbolKind::Variable
                        };
                        let start = if index == 0 { first } else { name };
                        out.push(self.symbol(self.text(name).to_owned(), kind, start, d.end, Vec::new()));
                    }
                    i = last.max(j) + 1;
                    continue;
                }
                _ => None,
            };
            match found {
                Some((symbol, next)) => {
                    out.push(symbol);
                    i = next.max(i + 1);
                }
                None => i = self.skip(j).max(i + 1),
            }
        }
        out
    }

    fn function_symbol(&self, first: usize, j: usize, to: usize) -> Option<(Symbol, usize)> {
        let mut k = j + 1;
        if self.is_punct(k, "*") {
            k += 1;
        }
        let name = if self.is_ident(k) {
            k += 1;
            self.text(k - 1).to_owned()
        } else {
            "default".to_owned()
        };
        while k < to && !self.is_punct(k, "(") {
            if self.is_punct(k, ";") || self.is_punct(k, "{") || self.is_punct(k, "}") {
                return None;
            }
            k += 1;
        }
        if k >= to {
            return None;
        }
        k = self.close(k) + 1;
        let end = match self.find_body(k, to) {
            Some(body) => self.close(body),
            None => self.declarator_end(k, to, false),
        }
        .min(to.saturating_sub(1));
        Some((self.symbol(name, SymbolKind::Function, first, end, Vec::new()), end + 1))
    }

    fn class_symbol(&self, first: usize, j: usize, to: usize) -> Option<(Symbol, usize)> {
        let name = if self.is_ident(j + 1) && !matches!(self.text(j + 1), "extends" | "implements") {
            self.text(j + 1).to_owned()
        } else {
            "default".to_owned()
        };
        let body = self.find_body(j + 1, to)?;
        let close = self.close(body);
        let children = self.class_members(body, close);
        Some((self.symbol(name, SymbolKind::Class, first, close, children), close + 1))
    }

    fn class_members(&self, open: usize, close: usize) -> Vec<Symbol> {
        let mut out = Vec::new();
        let mut k = open + 1;
        while k < close {
            if self.is_punct(k, ";") {
                k += 1;
                continue;
            }
            let first = k;
            while self.is_punct(k, "@") {
                k = self.skip_decorator(k);
            }
            if self.is_word(k, "static") && self.is_punct(k + 1, "{") {
                k = self.close(k + 1) + 1;
                continue;
            }
            while self.is_ident(k) && MODIFIERS.contains(&self.text(k)) && self.member_name_after(k + 1) {
                k += 1;
            }
            if self.is_punct(k, "*") {
                k += 1;
            }
            let name = match self.tok(k) {
                Some(Tok::Ident | Tok::Private | Tok::Number | Tok::String) => {
                    k += 1;
                    self.name_text(k - 1)
                }
                Some(Tok::Punct) if self.is_punct(k, "[") => {
                    let c = self.close(k).min(close - 1);
                    let name = self.text[self.sig[k].start..self.sig[c].end].to_owned();
                    k = c + 1;
                    name
                }
                _ => {
                    k = self.skip(k);
                    continue;
                }
            };
            if self.is_punct(k, "?") || self.is_punct(k, "!") {
                k += 1;
            }
            let (kind, end) = if self.is_punct(k, "(") || self.is_punct(k, "<") {
                (SymbolKind::Method, self.method_end(k, close))
            } else {
                let end = self.declarator_end(k, close, false);
                let mut value = k;
                while value <= end && !self.is_punct(value, "=") {
                    value = self.skip(value);
                }
                let method = value <= end && self.is_function_value(value + 1);
                (if method { SymbolKind::Method } else { SymbolKind::Field }, end)
            };
            out.push(self.symbol(name, kind, first, end, Vec::new()));
            k = end.max(first) + 1;
        }
        out
    }

    fn method_end(&self, mut k: usize, limit: usize) -> usize {
        while k < limit && !self.is_punct(k, "(") {
            k += 1;
        }
        if k >= limit {
            return limit - 1;
        }
        k = self.close(k) + 1;
        while k < limit {
            if self.is_punct(k, "{") {
                return self.close(k).min(limit - 1);
            }
            if self.is_punct(k, ";") || self.is_punct(k, ",") {
                return k;
            }
            if !self.same_line(k - 1, k) && self.operand(k - 1) && !self.continues(k) {
                return k - 1;
            }
            k = self.skip(k);
        }
        limit - 1
    }

    fn interface_symbol(&self, first: usize, j: usize, to: usize) -> Option<(Symbol, usize)> {
        let name = self.text(j + 1).to_owned();
        let body = self.find_body(j + 2, to)?;
        let close = self.close(body);
        let mut children = Vec::new();
        let mut k = body + 1;
        while k < close {
            if self.is_punct(k, ";") || self.is_punct(k, ",") {
                k += 1;
                continue;
            }
            let start = k;
            if self.is_word(k, "readonly") && self.member_name_after(k + 1) {
                k += 1;
            }
            let member = match self.tok(k) {
                Some(Tok::Ident | Tok::String | Tok::Number) => Some(self.name_text(k)),
                _ => None,
            };
            let Some(member) = member else {
                k = self.skip(k);
                continue;
            };
            k += 1;
            if self.is_punct(k, "?") {
                k += 1;
            }
            let (kind, end) = if self.is_punct(k, "(") || self.is_punct(k, "<") {
                (SymbolKind::Method, self.method_end(k, close))
            } else {
                (SymbolKind::Field, self.declarator_end(k, close, true))
            };
            children.push(self.symbol(member, kind, start, end, Vec::new()));
            k = end.max(start) + 1;
        }
        Some((self.symbol(name, SymbolKind::Interface, first, close, children), close + 1))
    }

    fn enum_symbol(&self, first: usize, j: usize, to: usize) -> Option<(Symbol, usize)> {
        let name = self.text(j + 1).to_owned();
        let body = self.find_body(j + 2, to)?;
        let close = self.close(body);
        let mut children = Vec::new();
        let mut k = body + 1;
        while k < close {
            if matches!(self.tok(k), Some(Tok::Ident | Tok::String)) && (k == body + 1 || self.is_punct(k - 1, ",")) {
                let end = self.declarator_end(k + 1, close, true);
                children.push(self.symbol(self.name_text(k), SymbolKind::Constant, k, end, Vec::new()));
                k = end + 1;
            } else {
                k = self.skip(k);
            }
        }
        Some((self.symbol(name, SymbolKind::Type, first, close, children), close + 1))
    }

    fn module_symbol(&self, first: usize, j: usize, to: usize) -> Option<(Symbol, usize)> {
        let mut k = j + 1;
        let name = if self.is_ident(k) {
            let mut name = self.text(k).to_owned();
            k += 1;
            while self.is_punct(k, ".") && self.is_ident(k + 1) {
                name.push('.');
                name.push_str(self.text(k + 1));
                k += 2;
            }
            name
        } else if self.tok(k) == Some(Tok::String) {
            k += 1;
            self.name_text(k - 1)
        } else {
            self.text(j).to_owned()
        };
        let Some(body) = self.find_body(k, to) else {
            let end = self.declarator_end(k, to, false);
            return Some((self.symbol(name, SymbolKind::Module, first, end, Vec::new()), end + 1));
        };
        let close = self.close(body);
        let children = if self.nesting.get() < MAX_OUTLINE_DEPTH {
            self.nesting.set(self.nesting.get() + 1);
            let children = self.items(body + 1, close.min(to));
            self.nesting.set(self.nesting.get() - 1);
            children
        } else {
            Vec::new()
        };
        Some((self.symbol(name, SymbolKind::Module, first, close, children), close + 1))
    }

    fn enclosing(&self, pos: usize) -> Option<usize> {
        let mut best = None;
        for i in 0..self.n() {
            if self.sig[i].start >= pos {
                break;
            }
            if self.is_opener(i) && self.sig[i].end <= pos {
                let closed = self.matching[i].is_some_and(|c| self.sig[c].start < pos);
                if !closed {
                    best = Some(i);
                }
            }
        }
        best
    }

    fn inside_literal(&self, caret: usize) -> bool {
        for (index, t) in self.all.iter().enumerate() {
            let after_close = index > 0 && self.all[index - 1].tok == Tok::TemplateClose;
            if t.start > caret || (t.start == caret && !(t.tok == Tok::Template && after_close)) {
                break;
            }
            if caret > t.end {
                continue;
            }
            let body = &self.text[t.start..t.end];
            let inside = match t.tok {
                Tok::Comment if body.starts_with("//") || body.starts_with("#!") => true,
                Tok::Comment | Tok::DocComment => caret < t.end || body.len() < 4 || !body.ends_with("*/"),
                Tok::String | Tok::JsxString => {
                    caret < t.end || body.len() < 2 || body.as_bytes()[body.len() - 1] != body.as_bytes()[0]
                }
                Tok::Template => {
                    let closed = body.ends_with('`') && (body.len() >= 2 || after_close);
                    caret < t.end || !closed
                }
                Tok::Regex => caret < t.end,
                _ => false,
            };
            if inside {
                return true;
            }
        }
        false
    }

    fn in_jsx_text(&self, from: usize) -> bool {
        let inside = self.elements.iter().any(|&(start, end)| start < from && from < end);
        if !inside {
            return false;
        }
        match self.enclosing(from) {
            Some(open) => {
                let within =
                    self.elements.iter().filter(|&&(start, end)| start < from && from < end).map(|e| e.0).max();
                !(self.tok(open) == Some(Tok::JsxOpen) || within.is_some_and(|s| self.sig[open].start > s))
            }
            None => true,
        }
    }

    fn place(&self, p: Option<usize>, from: usize) -> Place {
        let Some(p) = p else { return Place::Statement };
        if matches!(self.tok(p), Some(Tok::JsxTag | Tok::JsxAttr | Tok::JsxString | Tok::JsxPunct)) {
            return Place::Nothing;
        }
        let open = self.enclosing(from);
        let scope = open.and_then(|o| self.scopes[o]);
        let new_line = self.sig[p].end <= from && self.text[self.sig[p].end..from].contains(['\n', '\r']);
        if scope == Some(Scope::Class) {
            let start = Some(p) == open
                || self.is_punct(p, ";")
                || self.is_punct(p, "}")
                || (new_line && self.operand(p))
                || (self.is_ident(p) && MODIFIERS.contains(&self.text(p)));
            if start {
                return Place::Member;
            }
        }
        if scope == Some(Scope::Interface) {
            return Place::Nothing;
        }
        if self.ts {
            let text = self.text(p);
            let after_word = self.is_ident(p)
                && matches!(text, "extends" | "implements" | "as" | "satisfies" | "keyof" | "is")
                && !(text == "extends" && !self.is_ident(p.wrapping_sub(1)));
            let annotation = self.is_punct(p, ":")
                && (matches!(scope, Some(Scope::Params | Scope::Class))
                    || (p > 0 && self.is_punct(p - 1, ")") && scope != Some(Scope::Object))
                    || (p > 1
                        && self.is_ident(p - 1)
                        && matches!(self.text(p - 2), "let" | "const" | "var")
                        && self.is_ident(p - 2)));
            let generic = self.is_punct(p, "<") && p > 0 && self.is_ident(p - 1) && self.same_line(p - 1, p);
            if after_word || annotation || generic {
                return Place::Type;
            }
        }
        if self.is_punct(p, ";")
            || self.is_punct(p, "{") && scope != Some(Scope::Object)
            || self.is_punct(p, "}")
            || (new_line && self.operand(p))
            || matches!(self.text(p), "else" | "do") && self.is_ident(p)
        {
            return Place::Statement;
        }
        if self.operand(p) {
            return Place::AfterOperand;
        }
        Place::Expression
    }

    fn indent_unit(&self) -> String {
        let mut smallest = usize::MAX;
        for line in self.text.lines() {
            if line.starts_with('\t') {
                return "\t".to_owned();
            }
            let spaces = line.len() - line.trim_start_matches(' ').len();
            if spaces > 0 && spaces < line.len() {
                smallest = smallest.min(spaces);
            }
        }
        " ".repeat(if smallest == usize::MAX { 2 } else { smallest.clamp(2, 8) })
    }

    fn line_indent(&self, caret: usize) -> &'a str {
        let start = self.text[..caret].rfind(['\n', '\r']).map_or(0, |p| p + 1);
        let line = &self.text[start..];
        &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
    }

    fn declarations(&self) -> Vec<(usize, &'static str)> {
        let mut out: Vec<(usize, &'static str)> = Vec::new();
        let n = self.n();
        for i in 0..n {
            if !self.is_ident(i) || self.after_dot(i) {
                if self.tok(i) == Some(Tok::Punct) && self.scopes[i] == Some(Scope::Params) {
                    self.param_names(i, &mut out);
                }
                continue;
            }
            match self.text(i) {
                "function" => {
                    let k = if self.is_punct(i + 1, "*") { i + 2 } else { i + 1 };
                    if self.is_ident(k) && !self.is_reserved(k) {
                        out.push((k, "function"));
                    }
                }
                "class" if self.is_ident(i + 1) && !self.is_reserved(i + 1) => out.push((i + 1, "class")),
                "interface" if self.ts && self.is_ident(i + 1) => out.push((i + 1, "interface")),
                "type"
                    if self.ts && self.is_ident(i + 1) && (self.is_punct(i + 2, "=") || self.is_punct(i + 2, "<")) =>
                {
                    out.push((i + 1, "type"));
                }
                "enum" if self.is_ident(i + 1) => out.push((i + 1, "type")),
                "namespace" if self.ts && self.is_ident(i + 1) => out.push((i + 1, "module")),
                "let" | "const" | "var" => {
                    let kind = if self.text(i) == "const" { "constant" } else { "variable" };
                    for d in self.declarators(i, n) {
                        if let Some(name) = d.name {
                            let function = d.value.is_some_and(|v| self.is_function_value(v));
                            out.push((name, if function { "function" } else { kind }));
                        }
                        if let Some(open) = d.pattern {
                            self.pattern_names(open, kind, &mut out);
                        }
                    }
                }
                "import" if !self.is_punct(i + 1, "(") && !self.is_punct(i + 1, ".") => self.import_names(i, &mut out),
                _ => {
                    if self.is_punct(i + 1, "=>") && !self.is_reserved(i) {
                        out.push((i, "variable"));
                    }
                }
            }
        }
        out
    }

    fn param_names(&self, open: usize, out: &mut Vec<(usize, &'static str)>) {
        let close = self.close(open).min(self.n());
        let mut k = open + 1;
        let mut start = true;
        while k < close {
            if self.is_punct(k, ",") {
                start = true;
                k += 1;
                continue;
            }
            if start {
                if self.is_ident(k)
                    && !self.is_reserved(k)
                    && !(PARAM_MODIFIERS.contains(&self.text(k)) && self.is_ident(k + 1))
                {
                    out.push((k, "variable"));
                    start = false;
                } else if self.is_punct(k, "{") || self.is_punct(k, "[") {
                    self.pattern_names(k, "variable", out);
                    start = false;
                } else if !(self.is_punct(k, "...") || self.is_punct(k, "@") || self.is_ident(k)) {
                    start = false;
                }
            }
            k = if start { k + 1 } else { self.skip(k) };
        }
    }

    fn pattern_names(&self, open: usize, kind: &'static str, out: &mut Vec<(usize, &'static str)>) {
        let close = self.close(open).min(self.n());
        let mut k = open + 1;
        while k < close {
            if self.is_punct(k, "=") {
                k += 1;
                while k < close && !self.is_punct(k, ",") && !self.is_closer(k) {
                    k = self.skip(k);
                }
                continue;
            }
            if self.is_ident(k) && !self.is_reserved(k) && !self.is_punct(k + 1, ":") {
                let next = self.text(k + 1);
                if matches!(next, "," | "}" | "]" | "=") && !self.after_dot(k) {
                    out.push((k, kind));
                }
            }
            k += 1;
        }
    }

    fn import_names(&self, i: usize, out: &mut Vec<(usize, &'static str)>) {
        let mut k = i + 1;
        if self.is_word(k, "type") && !self.is_word(k + 1, "from") {
            k += 1;
        }
        while k < self.n() {
            if self.is_word(k, "from") || self.tok(k) == Some(Tok::String) || self.is_punct(k, ";") {
                return;
            }
            if self.is_punct(k, "*") && self.is_word(k + 1, "as") && self.is_ident(k + 2) {
                out.push((k + 2, "variable"));
                k += 3;
                continue;
            }
            if self.is_punct(k, "{") {
                let close = self.close(k).min(self.n());
                let mut m = k + 1;
                while m < close {
                    if self.is_ident(m) && (self.is_punct(m + 1, ",") || m + 1 == close) && !self.is_word(m, "type") {
                        out.push((m, "variable"));
                    }
                    m += 1;
                }
                k = close + 1;
                continue;
            }
            if self.is_ident(k) {
                out.push((k, "variable"));
            }
            k += 1;
        }
    }

    fn this_members(&self, from: usize) -> Vec<(String, &'static str)> {
        let mut class = None;
        for i in 0..self.n() {
            if self.sig[i].start >= from {
                break;
            }
            if self.scopes[i] == Some(Scope::Class) && self.matching[i].is_none_or(|c| self.sig[c].start >= from) {
                class = Some(i);
            }
        }
        let Some(open) = class else { return Vec::new() };
        let close = self.close(open);
        let mut out: Vec<(String, &'static str)> = Vec::new();
        for symbol in self.class_members(open, close) {
            if symbol.name != "constructor" {
                let kind = if symbol.kind == SymbolKind::Method { "method" } else { "field" };
                out.push((symbol.name, kind));
            }
        }
        for k in open..close.min(self.n()) {
            if self.is_word(k, "this")
                && self.is_punct(k + 1, ".")
                && matches!(self.tok(k + 2), Some(Tok::Ident | Tok::Private))
                && self.is_punct(k + 3, "=")
                && !self.is_punct(k + 4, "=")
                && self.sig[k + 2].end != from
            {
                out.push((self.text(k + 2).to_owned(), "field"));
            }
        }
        out
    }

    fn complete(&self, caret: usize) -> Completion {
        let bytes = self.text.as_bytes();
        let mut from = caret;
        while let Some(c) = self.text[..from].chars().next_back() {
            if !is_ident_char(c) {
                break;
            }
            from -= c.len_utf8();
        }
        if from > 0 && bytes[from - 1] == b'#' {
            from -= 1;
        }
        let mut result = Completion { from, items: Vec::new() };
        if self.inside_literal(caret) || self.in_jsx_text(from) {
            return result;
        }
        let prefix = self.text[from..caret].to_ascii_lowercase();
        let p = self.sig.iter().rposition(|t| t.end <= from);
        let mut items: Vec<CompletionItem> = Vec::new();
        let mut added = std::collections::HashSet::new();
        let mut add = |label: &str, kind: &'static str, insert: String, detail: Option<String>| {
            let matches = label.to_ascii_lowercase().starts_with(&prefix);
            if matches && items.len() < MAX_COMPLETIONS && added.insert((label.to_owned(), kind)) {
                items.push(CompletionItem { label: label.to_owned(), kind, insert, detail });
            }
        };
        if let Some(p) = p
            && (self.is_punct(p, ".") || self.is_punct(p, "?."))
        {
            if p > 0 && self.is_word(p - 1, "this") {
                for (name, kind) in self.this_members(from) {
                    add(&name, kind, name.clone(), None);
                }
            }
            result.items = items;
            return result;
        }
        let place = self.place(p, from);
        let keywords: Vec<&str> = match place {
            Place::Statement => {
                let mut words: Vec<&str> = STATEMENT_KEYWORDS.to_vec();
                if self.ts {
                    words.extend_from_slice(TS_STATEMENT_KEYWORDS);
                }
                if p.is_some_and(|p| self.is_punct(p, "}")) {
                    words.extend_from_slice(&["else", "catch", "finally"]);
                }
                words.extend_from_slice(EXPRESSION_KEYWORDS);
                words
            }
            Place::Expression => EXPRESSION_KEYWORDS.to_vec(),
            Place::AfterOperand => {
                let mut words = vec!["in", "instanceof"];
                if self.enclosing(from).and_then(|o| self.scopes[o]) == Some(Scope::ForHead) {
                    words.push("of");
                }
                if self.ts {
                    words.extend_from_slice(&["as", "satisfies"]);
                }
                words
            }
            Place::Member => {
                let mut words = CLASS_KEYWORDS.to_vec();
                if self.ts {
                    words.extend_from_slice(TS_CLASS_KEYWORDS);
                }
                words
            }
            Place::Type => TYPE_COMPLETIONS.to_vec(),
            Place::Nothing => Vec::new(),
        };
        let mut sorted = keywords;
        sorted.sort_unstable();
        sorted.dedup();
        if matches!(place, Place::Statement | Place::Expression | Place::Type) {
            let mut seen = std::collections::HashSet::new();
            for (index, kind) in self.declarations() {
                let name = self.text(index);
                let typeish = matches!(kind, "class" | "interface" | "type" | "module");
                let wanted = if place == Place::Type { typeish } else { !matches!(kind, "interface" | "type") };
                if !wanted || self.sig[index].start == from || !seen.insert(name) {
                    continue;
                }
                add(name, kind, name.to_owned(), None);
            }
        }
        for word in sorted {
            add(word, "keyword", word.to_owned(), None);
        }
        if matches!(place, Place::Statement | Place::Expression) {
            let indent = self.line_indent(from);
            let unit = self.indent_unit();
            let body = format!("{{\n{indent}{unit}\n{indent}}}");
            let mut snippets: Vec<(&str, String, &str)> = Vec::new();
            if place == Place::Statement {
                snippets.push(("for", format!("for (let i = 0; i < $0; i++) {body}"), "for (let i = 0; i < …; i++)"));
                snippets.push(("if", format!("if ($0) {body}"), "if (…)"));
                snippets.push(("class", format!("class $0 {body}"), "class … {}"));
            }
            snippets.push(("function", format!("function $0() {body}"), "function …() {}"));
            if place == Place::Expression {
                snippets.push(("arrow", format!("($0) => {body}"), "() => {}"));
            }
            for (label, insert, detail) in snippets {
                add(label, "snippet", insert, Some(detail.to_owned()));
            }
        }
        result.items = items;
        result
    }
}
