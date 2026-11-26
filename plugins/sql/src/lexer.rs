use crate::dialect::Dialect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    Quoted,
    String,
    Number,
    LineComment,
    BlockComment,
    Parameter,
    Delimiter,
    Directive,
    Symbol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
    pub open: usize,
    pub closed: bool,
}

impl Token {
    pub fn text<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }

    pub fn is(&self, text: &str, word: &str) -> bool {
        self.kind == TokenKind::Word && self.text(text).eq_ignore_ascii_case(word)
    }

    pub fn is_symbol(&self, text: &str, symbol: char) -> bool {
        self.kind == TokenKind::Symbol && self.text(text).starts_with(symbol)
    }

    pub fn is_comment(&self) -> bool {
        matches!(self.kind, TokenKind::LineComment | TokenKind::BlockComment)
    }
}

pub fn tokens(text: &str, dialect: Dialect) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut delimiter: Option<String> = None;
    let mut line_start = true;
    let mut at = 0;
    while at < bytes.len() {
        let c = bytes[at];
        if c == b'\n' {
            line_start = true;
            at += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let rest = &text[at..];
        let line_end = rest.find('\n').map_or(text.len(), |newline| at + newline);
        if line_start {
            line_start = false;
            if let Some(found) = directive(rest, dialect) {
                if let Some(set) = found {
                    delimiter = (set != ";").then(|| set.to_owned());
                }
                tokens.push(token(TokenKind::Directive, at, line_end, 0, true));
                at = line_end;
                continue;
            }
        }
        let (kind, end, open, closed) = if let Some(custom) = delimiter.as_deref().filter(|d| rest.starts_with(d)) {
            (TokenKind::Delimiter, at + custom.len(), 0, true)
        } else if c == b';' {
            (if delimiter.is_some() { TokenKind::Symbol } else { TokenKind::Delimiter }, at + 1, 0, true)
        } else if rest.starts_with("--") || (c == b'#' && dialect.hash_comments()) {
            (TokenKind::LineComment, line_end, 0, true)
        } else if rest.starts_with("/*") {
            let (end, closed) = block_comment(text, at, dialect.nested_comments());
            (TokenKind::BlockComment, end, 2, closed)
        } else if c == b'\'' {
            let (end, closed) = quoted(bytes, at + 1, b'\'', dialect.backslashes());
            (TokenKind::String, end, 1, closed)
        } else if matches!(c, b'E' | b'e')
            && bytes.get(at + 1) == Some(&b'\'')
            && dialect.postgres_strings()
            && !after_word(bytes, at)
        {
            let (end, closed) = quoted(bytes, at + 2, b'\'', true);
            (TokenKind::String, end, 2, closed)
        } else if c == b'$' && dialect.postgres_strings() && dollar_tag(rest).is_some() {
            let tag = dollar_tag(rest).unwrap_or(2);
            match rest[tag..].find(&rest[..tag]) {
                Some(close) => (TokenKind::String, at + tag + close + tag, tag, true),
                None => (TokenKind::String, text.len(), tag, false),
            }
        } else if c == b'$' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit) {
            let digits = rest[1..].bytes().take_while(u8::is_ascii_digit).count();
            (TokenKind::Parameter, at + 1 + digits, 0, true)
        } else if c == b'"' {
            let (end, closed) = quoted(bytes, at + 1, b'"', dialect.backslashes());
            (TokenKind::Quoted, end, 1, closed)
        } else if c == b'`' && dialect.backticks() {
            let (end, closed) = quoted(bytes, at + 1, b'`', false);
            (TokenKind::Quoted, end, 1, closed)
        } else if c == b'[' && dialect.brackets() {
            let (end, closed) = quoted(bytes, at + 1, b']', false);
            (TokenKind::Quoted, end, 1, closed)
        } else if (c == b':' && bytes.get(at + 1).is_some_and(|b| word_start(*b)) && (at == 0 || bytes[at - 1] != b':'))
            || c == b'@'
        {
            let length = 1 + rest[1..].bytes().take_while(|b| word_byte(*b) || *b == b'@').count();
            (TokenKind::Parameter, at + length, 0, true)
        } else if c == b'?' {
            (TokenKind::Parameter, at + 1, 0, true)
        } else if (c.is_ascii_digit() || (c == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)))
            && !after_word(bytes, at)
        {
            (TokenKind::Number, at + number_length(rest), 0, true)
        } else if word_start(c) {
            let length = rest.bytes().take_while(|b| word_byte(*b) || *b == b'$').count();
            let length = delimiter
                .as_deref()
                .and_then(|custom| rest[..length].find(custom))
                .filter(|cut| *cut > 0)
                .unwrap_or(length);
            (TokenKind::Word, at + length, 0, true)
        } else {
            (TokenKind::Symbol, at + rest.chars().next().map_or(1, char::len_utf8), 0, true)
        };
        tokens.push(token(kind, at, end.min(text.len()), open, closed));
        at = end.max(at + 1).min(text.len());
        while at < text.len() && !text.is_char_boundary(at) {
            at += 1;
        }
    }
    tokens
}

fn token(kind: TokenKind, start: usize, end: usize, open: usize, closed: bool) -> Token {
    Token { kind, start, end, open, closed }
}

fn directive(line: &str, dialect: Dialect) -> Option<Option<&str>> {
    let line = &line[..line.find('\n').unwrap_or(line.len())];
    let word_end = line.bytes().take_while(|b| b.is_ascii_alphabetic()).count();
    let (word, after) = line.split_at(word_end);
    if dialect.delimiter_lines() && word.eq_ignore_ascii_case("DELIMITER") && after.starts_with([' ', '\t']) {
        return Some(after.split_whitespace().next());
    }
    if dialect.go_lines()
        && word.eq_ignore_ascii_case("GO")
        && after.trim().bytes().all(|b| b.is_ascii_digit())
        && (after.is_empty() || after.starts_with([' ', '\t', '\r']))
    {
        return Some(None);
    }
    (dialect.backslash_lines() && line.starts_with('\\')).then_some(None)
}

fn block_comment(text: &str, at: usize, nested: bool) -> (usize, bool) {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut index = at;
    while index + 1 < bytes.len() {
        if bytes[index] == b'/' && bytes[index + 1] == b'*' && (depth == 0 || nested) {
            depth += 1;
            index += 2;
        } else if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return (index, true);
            }
        } else {
            index += 1;
        }
    }
    (text.len(), false)
}

fn quoted(bytes: &[u8], from: usize, close: u8, backslashes: bool) -> (usize, bool) {
    let mut index = from;
    loop {
        match bytes.get(index) {
            None => return (bytes.len(), false),
            Some(b'\\') if backslashes => index += 2,
            Some(b) if *b == close && bytes.get(index + 1) == Some(&close) => index += 2,
            Some(b) if *b == close => return (index + 1, true),
            Some(_) => index += 1,
        }
    }
}

pub(crate) fn dollar_tag(text: &str) -> Option<usize> {
    let first = *text.as_bytes().get(1)?;
    if first.is_ascii_digit() {
        return None;
    }
    let inner = text[1..].bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
    (text.as_bytes().get(1 + inner) == Some(&b'$')).then_some(inner + 2)
}

pub(crate) fn number_length(text: &str) -> usize {
    let bytes = text.as_bytes();
    if text.len() > 2 && (text.starts_with("0x") || text.starts_with("0X")) {
        return 2 + bytes[2..].iter().take_while(|b| b.is_ascii_hexdigit()).count();
    }
    let mut at = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if bytes.get(at) == Some(&b'.') {
        at += 1 + bytes[at + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(at + 1), Some(b'+' | b'-')));
        let digits = bytes[(at + 1 + sign).min(bytes.len())..].iter().take_while(|b| b.is_ascii_digit()).count();
        if digits > 0 {
            at += 1 + sign + digits;
        }
    }
    at.max(1)
}

pub fn word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

pub fn word_byte(byte: u8) -> bool {
    word_start(byte) || byte.is_ascii_digit()
}

fn after_word(bytes: &[u8], at: usize) -> bool {
    at > 0 && word_byte(bytes[at - 1])
}
