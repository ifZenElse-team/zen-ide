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
    Label = 8,
    Attribute = 9,
    Constant = 10,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

const HARD: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

const SOFT: &[&str] = &[
    "by",
    "catch",
    "constructor",
    "delegate",
    "dynamic",
    "field",
    "file",
    "finally",
    "get",
    "import",
    "init",
    "param",
    "property",
    "receiver",
    "set",
    "setparam",
    "value",
    "where",
    "abstract",
    "actual",
    "annotation",
    "companion",
    "const",
    "crossinline",
    "data",
    "enum",
    "expect",
    "external",
    "final",
    "infix",
    "inline",
    "inner",
    "internal",
    "lateinit",
    "noinline",
    "open",
    "operator",
    "out",
    "override",
    "private",
    "protected",
    "public",
    "reified",
    "sealed",
    "suspend",
    "tailrec",
    "vararg",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Word,
    Open,
    Close,
    Paren,
    CloseParen,
    Assign,
    End,
    Other,
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    spans: Vec<Span>,
    tokens: Vec<(Tok, usize, usize)>,
    brackets: usize,
}

fn is_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_part(byte: u8) -> bool {
    is_start(byte) || byte.is_ascii_digit()
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, bytes: text.as_bytes(), at: 0, spans: Vec::new(), tokens: Vec::new(), brackets: 0 }
    }

    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if end > start {
            match self.spans.last_mut() {
                Some(last) if last.end == start && last.kind == kind => last.end = end,
                _ => self.spans.push(Span { start, end, kind }),
            }
        }
    }

    fn boundary(&self, mut at: usize) -> usize {
        at = at.min(self.bytes.len());
        while !self.text.is_char_boundary(at) {
            at += 1;
        }
        at
    }

    fn peek(&self, offset: usize) -> u8 {
        self.bytes.get(self.at + offset).copied().unwrap_or(0)
    }

    fn starts_with(&self, text: &str) -> bool {
        self.bytes[self.at..].starts_with(text.as_bytes())
    }

    fn comment(&mut self) {
        let start = self.at;
        if self.starts_with("//") {
            while self.at < self.bytes.len() && self.bytes[self.at] != b'\n' {
                self.at += 1;
            }
            self.push(start, self.at, Kind::Comment);
            return;
        }
        let doc = self.starts_with("/**") && !self.starts_with("/**/");
        let mut depth = 0;
        while self.at < self.bytes.len() {
            if self.starts_with("/*") {
                depth += 1;
                self.at += 2;
            } else if self.starts_with("*/") {
                depth -= 1;
                self.at += 2;
                if depth == 0 {
                    break;
                }
            } else {
                self.at += 1;
            }
        }
        self.push(start, self.at, if doc { Kind::DocComment } else { Kind::Comment });
    }

    fn string(&mut self) {
        let raw = self.starts_with("\"\"\"");
        let close = if raw { "\"\"\"" } else { "\"" };
        let mut start = self.at;
        self.at += close.len();
        while self.at < self.bytes.len() {
            if self.starts_with(close) {
                self.at += close.len();
                while raw && self.peek(0) == b'"' {
                    self.at += 1;
                }
                break;
            }
            let c = self.bytes[self.at];
            if c == b'\n' && !raw {
                break;
            }
            if c == b'\\' && !raw {
                self.push(start, self.at, Kind::String);
                let end = self.boundary(if self.peek(1) == b'u' { self.at + 6 } else { self.at + 2 });
                self.push(self.at, end, Kind::Constant);
                self.at = end;
                start = end;
                continue;
            }
            if c == b'$' && self.peek(1) == b'{' {
                self.push(start, self.at, Kind::String);
                self.template();
                start = self.at;
                continue;
            }
            if c == b'$' && (is_start(self.peek(1)) || self.peek(1) == b'`') {
                self.push(start, self.at, Kind::String);
                let from = self.at;
                self.at += 1;
                if self.peek(0) == b'`' {
                    self.at += 1;
                    while self.at < self.bytes.len() && !matches!(self.bytes[self.at], b'`' | b'\n') {
                        self.at += 1;
                    }
                    self.at = (self.at + 1).min(self.bytes.len());
                } else {
                    while self.at < self.bytes.len() && is_part(self.bytes[self.at]) {
                        self.at += 1;
                    }
                }
                self.push(from, self.at, Kind::Constant);
                start = self.at;
                continue;
            }
            self.at += 1;
        }
        self.push(start, self.at, Kind::String);
    }

    fn template(&mut self) {
        let from = self.at;
        self.at += 2;
        self.push(from, self.at, Kind::Constant);
        let mut depth = 1;
        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b'{' => {
                    depth += 1;
                    self.at += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        self.push(self.at, self.at + 1, Kind::Constant);
                        self.at += 1;
                        return;
                    }
                    self.at += 1;
                }
                b'"' => self.string(),
                b'\'' => self.character(),
                b'/' if matches!(self.peek(1), b'/' | b'*') => self.comment(),
                b'0'..=b'9' => self.number(),
                c if is_start(c) => self.word(true),
                b'\n' => return,
                _ => self.at += 1,
            }
        }
    }

    fn character(&mut self) {
        let start = self.at;
        self.at += 1;
        if self.peek(0) == b'\\' {
            self.at = self.boundary(self.at + 2);
        }
        while self.at < self.bytes.len() && !matches!(self.bytes[self.at], b'\'' | b'\n') {
            self.at += 1;
        }
        if self.peek(0) == b'\'' {
            self.at += 1;
        }
        self.push(start, self.at, Kind::String);
    }

    fn number(&mut self) {
        let start = self.at;
        let hex = self.starts_with("0x") || self.starts_with("0X");
        let binary = self.starts_with("0b") || self.starts_with("0B");
        if hex || binary {
            self.at += 2;
        }
        loop {
            let c = self.peek(0);
            let exponent = !hex
                && matches!(c, b'e' | b'E')
                && (self.peek(1).is_ascii_digit() || matches!(self.peek(1), b'+' | b'-'));
            if exponent {
                self.at += 2;
            } else if c.is_ascii_digit()
                || c == b'_'
                || (hex && c.is_ascii_hexdigit())
                || (c == b'.' && self.peek(1).is_ascii_digit() && !hex && !binary)
            {
                self.at += 1;
            } else {
                break;
            }
        }
        while matches!(self.peek(0), b'L' | b'f' | b'F' | b'u' | b'U') {
            self.at += 1;
        }
        self.push(start, self.at, Kind::Number);
        self.tokens.push((Tok::Other, start, self.at));
    }

    fn word(&mut self, template: bool) {
        let start = self.at;
        while self.at < self.bytes.len() && is_part(self.bytes[self.at]) {
            self.at += 1;
        }
        let word = &self.text[start..self.at];
        let jumps = matches!(word, "return" | "break" | "continue" | "this" | "super");
        if self.peek(0) == b'@' && !jumps {
            self.at += 1;
            self.push(start, self.at, Kind::Label);
            if !template {
                self.tokens.push((Tok::Other, start, self.at));
            }
            return;
        }
        let before = self.text[..start].trim_end();
        let after_dot = before.ends_with('.') && !before.ends_with("..");
        let previous = self.tokens.last().map(|(_, from, to)| &self.text[*from..*to]);
        let mut next = self.at;
        while next < self.bytes.len() && matches!(self.bytes[next], b' ' | b'\t') {
            next += 1;
        }
        let following = self.bytes.get(next).copied().unwrap_or(0);
        let keyword =
            !after_dot && (HARD.contains(&word) || (SOFT.contains(&word) && soft_keyword(word, following, previous)));
        let kind = if keyword {
            Some(Kind::Keyword)
        } else if previous == Some("fun") {
            Some(Kind::Function)
        } else if word_is_type(word) {
            Some(Kind::Type)
        } else if following == b'(' || following == b'{' {
            Some(Kind::Function)
        } else {
            None
        };
        if let Some(kind) = kind {
            self.push(start, self.at, kind);
        }
        if !template {
            self.tokens.push((Tok::Word, start, self.at));
        }
    }

    fn run(&mut self) {
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            match c {
                b'\n' => {
                    if self.brackets == 0 {
                        self.tokens.push((Tok::End, self.at, self.at + 1));
                    }
                    self.at += 1;
                }
                b' ' | b'\t' | b'\r' => self.at += 1,
                b'/' if matches!(self.peek(1), b'/' | b'*') => self.comment(),
                b'"' => {
                    let start = self.at;
                    self.string();
                    self.tokens.push((Tok::Other, start, self.at));
                }
                b'\'' => {
                    let start = self.at;
                    self.character();
                    self.tokens.push((Tok::Other, start, self.at));
                }
                b'`' => {
                    let start = self.at;
                    self.at += 1;
                    while self.at < self.bytes.len() && !matches!(self.bytes[self.at], b'`' | b'\n') {
                        self.at += 1;
                    }
                    self.at = (self.at + 1).min(self.bytes.len());
                    if self.tokens.last().is_some_and(|(_, from, to)| &self.text[*from..*to] == "fun") {
                        self.push(start, self.at, Kind::Function);
                    }
                    self.tokens.push((Tok::Word, start, self.at));
                }
                b'@' => {
                    let start = self.at;
                    self.at += 1;
                    let label = self.tokens.last().is_some_and(|(_, from, to)| {
                        matches!(&self.text[*from..*to], "return" | "break" | "continue" | "this" | "super")
                            && *to == start
                    });
                    if label {
                        while self.at < self.bytes.len() && is_part(self.bytes[self.at]) {
                            self.at += 1;
                        }
                        self.push(start, self.at, Kind::Label);
                        self.tokens.push((Tok::Other, start, self.at));
                        continue;
                    }
                    while self.at < self.bytes.len()
                        && (is_part(self.bytes[self.at]) || matches!(self.bytes[self.at], b'.' | b':'))
                    {
                        if self.bytes[self.at] == b':'
                            && !matches!(
                                &self.text[start + 1..self.at],
                                "file"
                                    | "field"
                                    | "get"
                                    | "set"
                                    | "param"
                                    | "property"
                                    | "receiver"
                                    | "setparam"
                                    | "delegate"
                            )
                        {
                            break;
                        }
                        self.at += 1;
                    }
                    self.push(start, self.at, Kind::Attribute);
                    self.tokens.push((Tok::Other, start, self.at));
                }
                b'0'..=b'9' => self.number(),
                c if is_start(c) => self.word(false),
                _ => {
                    let token = match c {
                        b'{' => Tok::Open,
                        b'}' => Tok::Close,
                        b'(' | b'[' => {
                            self.brackets += 1;
                            Tok::Paren
                        }
                        b')' | b']' => {
                            self.brackets = self.brackets.saturating_sub(1);
                            Tok::CloseParen
                        }
                        b'=' if !matches!(self.peek(1), b'=' | b'>')
                            && !matches!(
                                self.bytes.get(self.at.wrapping_sub(1)),
                                Some(b'!' | b'<' | b'>' | b'=' | b'+' | b'-' | b'*' | b'/' | b'%')
                            ) =>
                        {
                            Tok::Assign
                        }
                        b';' => Tok::End,
                        _ => Tok::Other,
                    };
                    self.tokens.push((token, self.at, self.at + 1));
                    self.at += 1;
                }
            }
        }
    }
}

fn soft_keyword(word: &str, following: u8, previous: Option<&str>) -> bool {
    match word {
        "get" | "set" => following == b'(' || following == b'=' || following == b'{' || previous == Some("@"),
        "field" | "value" | "file" | "param" | "property" | "receiver" | "setparam" | "delegate" | "dynamic" => {
            word == "value" && following.is_ascii_alphabetic()
        }
        "by" | "where" => true,
        "catch" | "finally" | "init" | "constructor" => true,
        "out" => following.is_ascii_alphabetic() || following == b'*',
        _ => is_start(following) || following == b'@' || following == b'`',
    }
}

fn word_is_type(word: &str) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else { return false };
    first.is_ascii_uppercase() && (word.len() == 1 || word.chars().any(|c| c.is_ascii_lowercase()))
}

pub fn highlight(text: &str) -> Vec<Span> {
    let mut lexer = Lexer::new(text);
    lexer.run();
    lexer.spans
}

fn line_of(text: &str, offset: usize) -> u32 {
    u32::try_from(text.as_bytes()[..offset.min(text.len())].iter().filter(|b| **b == b'\n').count()).unwrap_or(u32::MAX)
}

pub fn fold(text: &str) -> Vec<Fold> {
    let mut lexer = Lexer::new(text);
    lexer.run();
    let mut folds = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for (token, start, _) in &lexer.tokens {
        match token {
            Tok::Open => open.push(*start),
            Tok::Close => {
                if let Some(from) = open.pop() {
                    let (first, last) = (line_of(text, from), line_of(text, *start));
                    if last > first {
                        folds.push(Fold { start_line: first, end_line: last });
                    }
                }
            }
            _ => {}
        }
    }
    for span in lexer.spans.iter().filter(|span| matches!(span.kind, Kind::Comment | Kind::DocComment)) {
        if text[span.start..].starts_with("/*") {
            let (first, last) = (line_of(text, span.start), line_of(text, span.end.saturating_sub(1)));
            if last > first {
                folds.push(Fold { start_line: first, end_line: last });
            }
        }
    }
    let mut run: Option<(u32, u32)> = None;
    for (index, line) in text.lines().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let trimmed = line.trim();
        if trimmed.starts_with("import ") {
            run = Some(run.map_or((index, index), |(first, _)| (first, index)));
        } else if !trimmed.is_empty()
            && let Some((first, last)) = run.take().filter(|(first, last)| last > first)
        {
            folds.push(Fold { start_line: first, end_line: last });
        }
    }
    if let Some((first, last)) = run.filter(|(first, last)| last > first) {
        folds.push(Fold { start_line: first, end_line: last });
    }
    folds.sort();
    folds.dedup();
    folds
}

pub fn outline(text: &str) -> Vec<Symbol> {
    let mut lexer = Lexer::new(text);
    lexer.run();
    let tokens = &lexer.tokens;
    let word = |index: usize| -> &str {
        tokens.get(index).filter(|(token, _, _)| *token == Tok::Word).map_or("", |(_, from, to)| &text[*from..*to])
    };
    let mut stack: Vec<Option<Symbol>> = Vec::new();
    let mut roots: Vec<Symbol> = Vec::new();
    let mut pending: Option<Symbol> = None;
    let mut statement_start: Option<usize> = None;
    let mut index = 0;
    while index < tokens.len() {
        let (token, start, end) = tokens[index];
        let in_class = stack
            .iter()
            .rev()
            .find_map(|symbol| symbol.as_ref().map(|s| matches!(s.kind, "class" | "interface")))
            .unwrap_or(false);
        match token {
            Tok::Word => {
                statement_start.get_or_insert(start);
                let keyword = word(index);
                let declares =
                    matches!(keyword, "class" | "interface" | "object" | "typealias" | "fun" | "val" | "var");
                if declares && pending.is_none() {
                    let modifiers_start = statement_start.unwrap_or(start);
                    let modifiers = &text[modifiers_start..start];
                    let (name, kind, skip) = match keyword {
                        "class" => {
                            let kind = if modifiers.contains("annotation") { "type" } else { "class" };
                            (word(index + 1).to_owned(), kind, 1)
                        }
                        "interface" => (word(index + 1).to_owned(), "interface", 1),
                        "object" => {
                            let named = word(index + 1);
                            let name = if named.is_empty() || SOFT.contains(&named) || HARD.contains(&named) {
                                "companion object".to_owned()
                            } else {
                                named.to_owned()
                            };
                            (name, "class", usize::from(!named.is_empty()))
                        }
                        "typealias" => (word(index + 1).to_owned(), "type", 1),
                        "fun" => {
                            let mut at = index + 1;
                            let mut name = String::new();
                            while let Some((token, from, to)) = tokens.get(at).copied() {
                                match token {
                                    Tok::Word => name = text[from..to].trim_matches('`').to_owned(),
                                    Tok::Paren if text[from..].starts_with('(') => break,
                                    Tok::Open | Tok::End | Tok::Assign | Tok::Close => break,
                                    _ => {}
                                }
                                at += 1;
                            }
                            (format!("{name}()"), if in_class { "method" } else { "function" }, at - index - 1)
                        }
                        _ => {
                            let name = word(index + 1).trim_matches('`').to_owned();
                            let kind = if modifiers.contains("const") {
                                "constant"
                            } else if in_class {
                                "field"
                            } else if stack.iter().any(Option::is_some) {
                                "variable"
                            } else {
                                "property"
                            };
                            (name, kind, 1)
                        }
                    };
                    let inside_function = stack
                        .iter()
                        .rev()
                        .find_map(|symbol| symbol.as_ref().map(|s| matches!(s.kind, "function" | "method")))
                        .unwrap_or(false);
                    let inside_code = stack.last().is_some_and(Option::is_none);
                    if !name.is_empty() && name != "()" && !inside_function && !inside_code {
                        pending = Some(Symbol { name, kind, start: modifiers_start, end, children: Vec::new() });
                        index += skip;
                    }
                }
                index += 1;
                continue;
            }
            Tok::Open => {
                let symbol = match pending.take() {
                    Some(symbol) => Some(symbol),
                    None => {
                        let top = stack.is_empty();
                        let previous = index.checked_sub(1).map(|at| tokens[at]);
                        match previous {
                            Some((Tok::Word, from, to)) if top && statement_start == Some(from) => Some(Symbol {
                                name: text[from..to].to_owned(),
                                kind: "module",
                                start: from,
                                end: to,
                                children: Vec::new(),
                            }),
                            Some((Tok::CloseParen, _, _)) if top => statement_start.and_then(|from| {
                                let head = text[from..start].trim();
                                (!head.is_empty() && head.len() <= 80 && !head.contains('\n')).then(|| Symbol {
                                    name: head.to_owned(),
                                    kind: "module",
                                    start: from,
                                    end: start,
                                    children: Vec::new(),
                                })
                            }),
                            _ => None,
                        }
                    }
                };
                stack.push(symbol);
                statement_start = None;
            }
            Tok::Close => {
                if let Some(mut open) = pending.take() {
                    open.end = start;
                    if let Some(parent) = stack.iter_mut().rev().find_map(Option::as_mut) {
                        parent.children.push(open);
                    }
                }
                if let Some(Some(mut symbol)) = stack.pop() {
                    symbol.end = end;
                    match stack.iter_mut().rev().find_map(Option::as_mut) {
                        Some(parent) => parent.children.push(symbol),
                        None => roots.push(symbol),
                    }
                }
                statement_start = None;
            }
            Tok::End => {
                if let Some(mut symbol) = pending.take() {
                    symbol.end = start;
                    match stack.iter_mut().rev().find_map(Option::as_mut) {
                        Some(parent) => parent.children.push(symbol),
                        None => roots.push(symbol),
                    }
                }
                statement_start = None;
            }
            Tok::Assign => {
                if let Some(symbol) = &mut pending {
                    symbol.end = end;
                }
                statement_start.get_or_insert(start);
            }
            Tok::Paren | Tok::CloseParen | Tok::Other => {
                statement_start.get_or_insert(start);
            }
        }
        index += 1;
    }
    if let Some(symbol) = pending.take() {
        roots.push(symbol);
    }
    roots
}

mod jvm;

pub use jvm::{JvmFile, jvm};

#[cfg(test)]
mod tests;
