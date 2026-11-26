#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tk {
    Ident,
    Int,
    Float,
    Char,
    Str,
    TextBlock,
    Punct,
    At,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: Tk,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Comment {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) line_end: bool,
}

const PUNCTS: &[&str] = &[
    ">>>=", "<<=", ">>=", "...", "->", "::", "++", "--", "&&", "||", "==", "!=", "<=", ">=", "+=", "-=", "*=", "/=",
    "%=", "&=", "|=", "^=", "<<", "(", ")", "{", "}", "[", "]", ";", ",", ".", "=", ">", "<", "!", "~", "?", ":", "+",
    "-", "*", "/", "&", "|", "^", "%",
];

pub(crate) fn lex(text: &str) -> (Vec<Token>, Vec<Comment>) {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut comments = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let c = bytes[at];
        if c.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if text[at..].starts_with("//") {
            let end = text[at..].find('\n').map_or(text.len(), |n| at + n);
            comments.push(Comment { start: at, end, line_end: true });
            at = end;
            continue;
        }
        if text[at..].starts_with("/*") {
            let end = text[at + 2..].find("*/").map_or(text.len(), |n| at + 2 + n + 2);
            comments.push(Comment { start: at, end, line_end: false });
            at = end;
            continue;
        }
        let start = at;
        if text[at..].starts_with("\"\"\"") {
            let mut end = at + 3;
            while end < bytes.len() && !text[end..].starts_with("\"\"\"") {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            let end = (end + 3).min(text.len());
            tokens.push(Token { kind: Tk::TextBlock, start, end });
            at = end;
            continue;
        }
        if c == b'"' || c == b'\'' {
            let mut end = at + 1;
            while end < bytes.len() && bytes[end] != c && bytes[end] != b'\n' {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            let end = (end + 1).min(text.len());
            tokens.push(Token { kind: if c == b'"' { Tk::Str } else { Tk::Char }, start, end });
            at = end;
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)) {
            let (end, float) = number(bytes, at);
            tokens.push(Token { kind: if float { Tk::Float } else { Tk::Int }, start, end });
            at = end;
            continue;
        }
        let ch = text[at..].chars().next().unwrap_or(' ');
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let mut end = at;
            for (offset, next) in text[at..].char_indices() {
                if next.is_alphanumeric() || next == '_' || next == '$' {
                    end = at + offset + next.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(Token { kind: Tk::Ident, start, end });
            at = end;
            continue;
        }
        if c == b'@' {
            tokens.push(Token { kind: Tk::At, start, end: at + 1 });
            at += 1;
            continue;
        }
        let len = PUNCTS.iter().find(|punct| text[at..].starts_with(**punct)).map_or(ch.len_utf8(), |p| p.len());
        tokens.push(Token { kind: Tk::Punct, start, end: at + len });
        at += len;
    }
    tokens.push(Token { kind: Tk::Eof, start: text.len(), end: text.len() });
    (tokens, comments)
}

fn number(bytes: &[u8], start: usize) -> (usize, bool) {
    let mut at = start;
    let mut float = false;
    if bytes[at] == b'0' && matches!(bytes.get(at + 1), Some(b'x' | b'X' | b'b' | b'B')) {
        at += 2;
        while at < bytes.len() && (bytes[at].is_ascii_hexdigit() || bytes[at] == b'_') {
            at += 1;
        }
    } else {
        while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'_') {
            at += 1;
        }
        if at < bytes.len()
            && bytes[at] == b'.'
            && bytes.get(at + 1).is_none_or(|next| {
                !next.is_ascii_alphabetic() || matches!(next, b'e' | b'E' | b'f' | b'F' | b'd' | b'D')
            })
        {
            float = true;
            at += 1;
            while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'_') {
                at += 1;
            }
        }
        if at < bytes.len() && matches!(bytes[at], b'e' | b'E') {
            float = true;
            at += 1;
            if at < bytes.len() && matches!(bytes[at], b'+' | b'-') {
                at += 1;
            }
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
        }
    }
    if at < bytes.len() && matches!(bytes[at], b'f' | b'F' | b'd' | b'D') {
        float = true;
        at += 1;
    } else if at < bytes.len() && matches!(bytes[at], b'l' | b'L') {
        at += 1;
    }
    (at, float)
}
