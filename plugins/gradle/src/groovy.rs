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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "continue",
    "def",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "int",
    "interface",
    "long",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "throws",
    "trait",
    "true",
    "try",
    "var",
    "void",
    "while",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Word,
    Open,
    Close,
    OpenParen,
    Other,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
    spans: Vec<Span>,
    regex_allowed: bool,
    tokens: Vec<(Token, usize, usize)>,
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_ident(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

impl Lexer<'_> {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if end > start {
            match self.spans.last_mut() {
                Some(last) if last.end == start && last.kind == kind => last.end = end,
                _ => self.spans.push(Span { start, end, kind }),
            }
        }
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
        self.at += 2;
        while self.at < self.bytes.len() && !self.starts_with("*/") {
            self.at += 1;
        }
        self.at = (self.at + 2).min(self.bytes.len());
        self.push(start, self.at, if doc { Kind::DocComment } else { Kind::Comment });
    }

    fn string(&mut self, close: &str, interpolates: bool, slashy: bool) {
        let mut start = self.at;
        self.at += close.len();
        while self.at < self.bytes.len() {
            if self.starts_with(close) {
                self.at += close.len();
                break;
            }
            let c = self.bytes[self.at];
            if c == b'\n' && close.len() == 1 && !slashy {
                break;
            }
            if c == b'\\' && !slashy {
                self.push(start, self.at, Kind::String);
                let end = (self.at + 2).min(self.bytes.len());
                self.push(self.at, end, Kind::Constant);
                self.at = end;
                start = end;
                continue;
            }
            if interpolates && c == b'$' && self.peek(1) == b'{' {
                self.push(start, self.at, Kind::String);
                let from = self.at;
                let mut depth = 0;
                while self.at < self.bytes.len() {
                    match self.bytes[self.at] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                self.at += 1;
                                break;
                            }
                        }
                        b'\n' if close.len() == 1 => break,
                        _ => {}
                    }
                    self.at += 1;
                }
                self.push(from, self.at, Kind::Constant);
                start = self.at;
                continue;
            }
            if interpolates && c == b'$' && is_ident_start(self.peek(1)) {
                self.push(start, self.at, Kind::String);
                let from = self.at;
                self.at += 1;
                while self.at < self.bytes.len()
                    && (is_ident(self.bytes[self.at]) || (self.bytes[self.at] == b'.' && is_ident_start(self.peek(1))))
                {
                    self.at += 1;
                }
                self.push(from, self.at, Kind::Constant);
                start = self.at;
                continue;
            }
            self.at += 1;
        }
        self.push(start, self.at, Kind::String);
    }

    fn number(&mut self) {
        let start = self.at;
        while self.at < self.bytes.len()
            && (self.bytes[self.at].is_ascii_alphanumeric()
                || self.bytes[self.at] == b'_'
                || (self.bytes[self.at] == b'.' && self.peek(1).is_ascii_digit()))
        {
            self.at += 1;
        }
        self.push(start, self.at, Kind::Number);
    }

    fn word(&mut self) {
        let start = self.at;
        while self.at < self.bytes.len() && is_ident(self.bytes[self.at]) {
            self.at += 1;
        }
        let word = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or_default();
        let previous_dot =
            start > 0 && self.bytes[..start].iter().rev().find(|b| !b.is_ascii_whitespace()) == Some(&b'.');
        let mut next = self.at;
        while next < self.bytes.len() && matches!(self.bytes[next], b' ' | b'\t') {
            next += 1;
        }
        let following = self.bytes.get(next).copied().unwrap_or(0);
        let kind = if KEYWORDS.contains(&word) && !previous_dot {
            Some(Kind::Keyword)
        } else if following == b'(' || following == b'{' {
            Some(Kind::Function)
        } else if word.as_bytes()[0].is_ascii_uppercase() && word.bytes().any(|b| b.is_ascii_lowercase()) {
            Some(Kind::Type)
        } else {
            None
        };
        if let Some(kind) = kind {
            self.push(start, self.at, kind);
        }
        self.tokens.push((Token::Word, start, self.at));
        self.regex_allowed = matches!(word, "return" | "in" | "case" | "assert");
    }

    fn run(&mut self) {
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            match c {
                b' ' | b'\t' | b'\r' | b'\n' => self.at += 1,
                b'/' if self.peek(1) == b'/' || self.peek(1) == b'*' => self.comment(),
                b'/' if self.regex_allowed && self.peek(1) != b' ' && self.peek(1) != b'=' => {
                    let start = self.at;
                    self.string("/", false, true);
                    self.tokens.push((Token::Other, start, self.at));
                    self.regex_allowed = false;
                }
                b'\'' | b'"' => {
                    let start = self.at;
                    let triple = if c == b'\'' { "'''" } else { "\"\"\"" };
                    if self.starts_with(triple) {
                        self.string(triple, c == b'"', false);
                    } else {
                        self.string(if c == b'\'' { "'" } else { "\"" }, c == b'"', false);
                    }
                    self.tokens.push((Token::Other, start, self.at));
                    self.regex_allowed = false;
                }
                b'@' if is_ident_start(self.peek(1)) => {
                    let start = self.at;
                    self.at += 1;
                    while self.at < self.bytes.len() && (is_ident(self.bytes[self.at]) || self.bytes[self.at] == b'.') {
                        self.at += 1;
                    }
                    self.push(start, self.at, Kind::Attribute);
                    self.regex_allowed = false;
                }
                b'0'..=b'9' => {
                    let start = self.at;
                    self.number();
                    self.tokens.push((Token::Other, start, self.at));
                    self.regex_allowed = false;
                }
                c if is_ident_start(c) => self.word(),
                _ => {
                    let token = match c {
                        b'{' => Token::Open,
                        b'}' => Token::Close,
                        b'(' => Token::OpenParen,
                        _ => Token::Other,
                    };
                    self.tokens.push((token, self.at, self.at + 1));
                    self.regex_allowed = !matches!(c, b')' | b']' | b'}');
                    self.at += 1;
                }
            }
        }
    }
}

fn lex(text: &str) -> Lexer<'_> {
    let mut lexer = Lexer { bytes: text.as_bytes(), at: 0, spans: Vec::new(), regex_allowed: true, tokens: Vec::new() };
    lexer.run();
    lexer
}

pub fn highlight(text: &str) -> Vec<Span> {
    lex(text).spans
}

fn line_of(text: &str, offset: usize) -> u32 {
    u32::try_from(text.as_bytes()[..offset.min(text.len())].iter().filter(|b| **b == b'\n').count()).unwrap_or(u32::MAX)
}

pub fn fold(text: &str) -> Vec<Fold> {
    let lexer = lex(text);
    let mut folds = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for (token, start, _) in &lexer.tokens {
        match token {
            Token::Open => open.push(*start),
            Token::Close => {
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
        let (first, last) = (line_of(text, span.start), line_of(text, span.end.saturating_sub(1)));
        if last > first && text[span.start..].starts_with("/*") {
            folds.push(Fold { start_line: first, end_line: last });
        }
    }
    folds.sort_by_key(|fold| (fold.start_line, std::cmp::Reverse(fold.end_line)));
    folds.dedup();
    folds
}

pub fn outline(text: &str) -> Vec<Symbol> {
    let lexer = lex(text);
    let tokens = &lexer.tokens;
    let mut roots: Vec<Symbol> = Vec::new();
    let mut stack: Vec<Option<Symbol>> = Vec::new();
    for (index, (token, _, end)) in tokens.iter().enumerate() {
        match token {
            Token::Open => {
                let symbol = block_name(text, tokens, index).map(|(name, from)| Symbol {
                    name,
                    start: from,
                    end: *end,
                    children: Vec::new(),
                });
                stack.push(symbol);
            }
            Token::Close => {
                let Some(open) = stack.pop() else { continue };
                let Some(mut symbol) = open else { continue };
                symbol.end = *end;
                match stack.iter_mut().rev().find_map(Option::as_mut) {
                    Some(parent) => parent.children.push(symbol),
                    None => roots.push(symbol),
                }
            }
            _ => {}
        }
    }
    roots
}

fn block_name(text: &str, tokens: &[(Token, usize, usize)], index: usize) -> Option<(String, usize)> {
    let open = tokens[index].1;
    let line_start = text[..open].rfind('\n').map_or(0, |at| at + 1);
    let head = text[line_start..open].trim();
    if head.is_empty() || head.ends_with("->") || head.ends_with('=') || head.ends_with(',') {
        return None;
    }
    let first = tokens[..index].iter().rev().take_while(|(_, start, _)| *start >= line_start).last()?;
    if first.0 != Token::Word || KEYWORDS.contains(&&text[first.1..first.2]) {
        return None;
    }
    let mut name: String = head.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().count() > 80 {
        name = name.chars().take(80).collect::<String>() + "…";
    }
    Some((name, first.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = r#"plugins {
    id 'application'
}

/** The application. */
application {
    mainClass = "demo.${name}.App"
}

dependencies {
    implementation("org.slf4j:slf4j-api:2.0.13") // logging
    testImplementation 'junit:junit:4.13.2'
}

tasks.register('hello') {
    group = 'custom'
    doLast {
        def pattern = /a\d+/
        println "Hello $project.name ${1 + 2}"
        if (true) { return 42 / 2 }
    }
}
"#;

    fn kinds(text: &str) -> Vec<(String, Kind)> {
        highlight(text).iter().map(|span| (text[span.start..span.end].to_owned(), span.kind)).collect()
    }

    #[test]
    fn a_build_script_is_highlighted() {
        let found = kinds(SCRIPT);
        let has = |text: &str, kind: Kind| found.iter().any(|(t, k)| t == text && *k == kind);
        assert!(has("plugins", Kind::Function));
        assert!(has("'application'", Kind::String));
        assert!(has("/** The application. */", Kind::DocComment));
        assert!(has("${name}", Kind::Constant));
        assert!(has("implementation", Kind::Function));
        assert!(has("// logging", Kind::Comment));
        assert!(has("def", Kind::Keyword));
        assert!(has("/a\\d+/", Kind::String), "a slashy string: {found:?}");
        assert!(has("$project.name", Kind::Constant));
        assert!(has("42", Kind::Number));
        assert!(has("return", Kind::Keyword));
        assert!(!found.iter().any(|(t, _)| t.starts_with("/ 2")), "a division is not a string");
        let spans = highlight(SCRIPT);
        assert!(spans.windows(2).all(|pair| pair[0].end <= pair[1].start), "ordered and not overlapping");
    }

    #[test]
    fn strings_end_where_they_should() {
        assert_eq!(
            kinds("x = '''a\n'b'\n''' + 1"),
            [("'''a\n'b'\n'''".to_owned(), Kind::String), ("1".to_owned(), Kind::Number)]
        );
        assert_eq!(kinds("x = 'unclosed\ny = 2")[0].0, "'unclosed");
        assert_eq!(
            kinds(r#"x = "a\"b""#),
            [("\"a".to_owned(), Kind::String), ("\\\"".to_owned(), Kind::Constant), ("b\"".to_owned(), Kind::String),]
        );
        assert!(kinds("@CompileStatic class A extends B {}").contains(&("@CompileStatic".to_owned(), Kind::Attribute)));
    }

    #[test]
    fn blocks_fold_and_make_the_structure() {
        let folds = fold(SCRIPT);
        assert!(folds.contains(&Fold { start_line: 0, end_line: 2 }));
        assert!(folds.contains(&Fold { start_line: 14, end_line: 21 }));
        let outline = outline(SCRIPT);
        let names: Vec<&str> = outline.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["plugins", "application", "dependencies", "tasks.register('hello')"]);
        assert_eq!(outline[3].children[0].name, "doLast");
        assert!(outline[3].children[0].children.is_empty(), "an if's block is not a block of the build");
        assert_eq!(&SCRIPT[outline[0].start..outline[0].end], "plugins {\n    id 'application'\n}");
    }
}
