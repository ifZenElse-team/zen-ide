use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Comment = 3,
    Constant = 10,
    Punctuation = 11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub fn highlight(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut at = 0;
    while at < text.len() {
        at = line(text.as_bytes(), at, &mut spans);
    }
    spans
}

fn line(bytes: &[u8], start: usize, spans: &mut Vec<Span>) -> usize {
    let (end, next) = line_end(bytes, start);
    let mut at = skip_blank(bytes, start, end);
    if at == end {
        return next;
    }
    if bytes[at] == b'#' {
        push(spans, at, end, Kind::Comment);
        return next;
    }

    let exported = bytes[at..end].starts_with(b"export") && bytes.get(at + 6).is_some_and(|&b| is_blank(b));
    if exported {
        push(spans, at, at + 6, Kind::Keyword);
        at = skip_blank(bytes, at + 6, end);
    }

    let key_start = at;
    while at < end && !matches!(bytes[at], b'=' | b':' | b'#') && !is_blank(bytes[at]) {
        at += 1;
    }
    let key_end = at;
    at = skip_blank(bytes, at, end);
    let separator = match bytes.get(at) {
        Some(b'=') if at < end => true,
        Some(b':') if at < end => at + 1 == end || is_blank(bytes[at + 1]),
        _ => false,
    };
    if !separator {
        if exported {
            push(spans, key_start, key_end, Kind::Constant);
            trailing_comment(bytes, key_end, end, spans);
        }
        return next;
    }
    push(spans, key_start, key_end, Kind::Constant);
    push(spans, at, at + 1, Kind::Punctuation);
    at += 1;
    let spaced = at < end && is_blank(bytes[at]);
    at = skip_blank(bytes, at, end);
    value(bytes, at, end, next, spaced, spans)
}

fn value(bytes: &[u8], at: usize, end: usize, next: usize, spaced: bool, spans: &mut Vec<Span>) -> usize {
    match bytes.get(at) {
        _ if at >= end => next,
        Some(b'#') if spaced => {
            push(spans, at, end, Kind::Comment);
            next
        }
        Some(&quote @ (b'\'' | b'`')) => {
            let close = bytes[at + 1..end].iter().position(|&b| b == quote).map_or(end, |offset| at + 2 + offset);
            push(spans, at, close, Kind::String);
            trailing_comment(bytes, close, end, spans);
            next
        }
        Some(b'"') => {
            let mut i = at + 1;
            let mut close = None;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' => i += 2,
                    b'"' => {
                        close = Some(i + 1);
                        break;
                    }
                    _ => i += 1,
                }
            }
            match close {
                Some(close) => {
                    string_with_references(bytes, at, close, spans);
                    let line_start = bytes[..close].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
                    let (close_end, close_next) = line_end(bytes, line_start);
                    trailing_comment(bytes, close, close_end, spans);
                    close_next
                }
                None => {
                    string_with_references(bytes, at, end, spans);
                    next
                }
            }
        }
        _ => {
            let comment = (at + 1..end).find(|&i| bytes[i] == b'#' && is_blank(bytes[i - 1]));
            let mut value_end = comment.unwrap_or(end);
            while value_end > at && is_blank(bytes[value_end - 1]) {
                value_end -= 1;
            }
            string_with_references(bytes, at, value_end, spans);
            if let Some(comment) = comment {
                push(spans, comment, end, Kind::Comment);
            }
            next
        }
    }
}

fn string_with_references(bytes: &[u8], start: usize, end: usize, spans: &mut Vec<Span>) {
    let mut segment = start;
    let mut i = start;
    while i < end {
        match bytes[i] {
            b'\\' => i += 2,
            b'$' => match reference(bytes, i, end) {
                Some(len) => {
                    push(spans, segment, i, Kind::String);
                    push(spans, i, i + len, Kind::Keyword);
                    i += len;
                    segment = i;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    push(spans, segment, end, Kind::String);
}

fn reference(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
    let first = *bytes.get(at + 1).filter(|_| at + 1 < end)?;
    if first == b'{' {
        let mut depth = 0usize;
        for (i, &byte) in bytes.iter().enumerate().take(end).skip(at + 1) {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return (i > at + 2).then_some(i + 1 - at);
                    }
                }
                _ => {}
            }
        }
        return None;
    }
    if first == b'_' || first.is_ascii_alphabetic() {
        let len = bytes[at + 1..end].iter().take_while(|&&b| b == b'_' || b.is_ascii_alphanumeric()).count();
        return Some(1 + len);
    }
    None
}

fn trailing_comment(bytes: &[u8], at: usize, end: usize, spans: &mut Vec<Span>) {
    let at = skip_blank(bytes, at, end);
    if at < end && bytes[at] == b'#' {
        push(spans, at, end, Kind::Comment);
    }
}

fn line_end(bytes: &[u8], start: usize) -> (usize, usize) {
    let newline = bytes[start..].iter().position(|&b| b == b'\n').map(|offset| start + offset);
    let next = newline.map_or(bytes.len(), |newline| newline + 1);
    let mut end = newline.unwrap_or(bytes.len());
    if end > start && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    (end, next)
}

fn skip_blank(bytes: &[u8], mut at: usize, end: usize) -> usize {
    while at < end && is_blank(bytes[at]) {
        at += 1;
    }
    at
}

fn is_blank(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn push(spans: &mut Vec<Span>, start: usize, end: usize, kind: Kind) {
    if end > start {
        spans.push(Span { start, end, kind });
    }
}

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

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_abi() -> i32 {
    0
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
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        let text = if valid { std::str::from_utf8(&input[..len as usize]).unwrap_or("") } else { "" };
        encode(&highlight(text))
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    #[test]
    fn highlights_comments_export_names_and_values() {
        let text = "# Database\nexport DB_HOST=localhost\n  PORT = 5432\nDEBUG=true\r\nEMPTY=\nexport PATH\n";
        assert_eq!(
            kinds(text),
            [
                ("# Database", Kind::Comment),
                ("export", Kind::Keyword),
                ("DB_HOST", Kind::Constant),
                ("=", Kind::Punctuation),
                ("localhost", Kind::String),
                ("PORT", Kind::Constant),
                ("=", Kind::Punctuation),
                ("5432", Kind::String),
                ("DEBUG", Kind::Constant),
                ("=", Kind::Punctuation),
                ("true", Kind::String),
                ("EMPTY", Kind::Constant),
                ("=", Kind::Punctuation),
                ("export", Kind::Keyword),
                ("PATH", Kind::Constant),
            ]
        );
    }

    #[test]
    fn highlights_colon_separators() {
        let text = "KEY: value\nURL:http://x\n";
        assert_eq!(kinds(text), [("KEY", Kind::Constant), (":", Kind::Punctuation), ("value", Kind::String)]);
    }

    #[test]
    fn highlights_inline_comments() {
        let text = "A=1 # one\nB=#notacomment\nC= # empty\nD='x # y' # z\nE=a#b\n";
        assert_eq!(
            kinds(text),
            [
                ("A", Kind::Constant),
                ("=", Kind::Punctuation),
                ("1", Kind::String),
                ("# one", Kind::Comment),
                ("B", Kind::Constant),
                ("=", Kind::Punctuation),
                ("#notacomment", Kind::String),
                ("C", Kind::Constant),
                ("=", Kind::Punctuation),
                ("# empty", Kind::Comment),
                ("D", Kind::Constant),
                ("=", Kind::Punctuation),
                ("'x # y'", Kind::String),
                ("# z", Kind::Comment),
                ("E", Kind::Constant),
                ("=", Kind::Punctuation),
                ("a#b", Kind::String),
            ]
        );
    }

    #[test]
    fn highlights_quoted_values() {
        let text = "S='$HOME'\nB=`${X}`\nD=\"a \\\" $HOME\"\nM=\"line one\nline two\" # end\nN=1\n";
        assert_eq!(
            kinds(text),
            [
                ("S", Kind::Constant),
                ("=", Kind::Punctuation),
                ("'$HOME'", Kind::String),
                ("B", Kind::Constant),
                ("=", Kind::Punctuation),
                ("`${X}`", Kind::String),
                ("D", Kind::Constant),
                ("=", Kind::Punctuation),
                ("\"a \\\" ", Kind::String),
                ("$HOME", Kind::Keyword),
                ("\"", Kind::String),
                ("M", Kind::Constant),
                ("=", Kind::Punctuation),
                ("\"line one\nline two\"", Kind::String),
                ("# end", Kind::Comment),
                ("N", Kind::Constant),
                ("=", Kind::Punctuation),
                ("1", Kind::String),
            ]
        );
    }

    #[test]
    fn highlights_references() {
        let text = "U=${USER}@$HOST:${PORT:-${DEFAULT}}/x\nP=\\$NOT $ ${open\n";
        assert_eq!(
            kinds(text),
            [
                ("U", Kind::Constant),
                ("=", Kind::Punctuation),
                ("${USER}", Kind::Keyword),
                ("@", Kind::String),
                ("$HOST", Kind::Keyword),
                (":", Kind::String),
                ("${PORT:-${DEFAULT}}", Kind::Keyword),
                ("/x", Kind::String),
                ("P", Kind::Constant),
                ("=", Kind::Punctuation),
                ("\\$NOT $ ${open", Kind::String),
            ]
        );
    }

    #[test]
    fn unclosed_quotes_end_with_their_line() {
        let text = "A=\"open\nB=2\nC='open\n";
        assert_eq!(
            kinds(text),
            [
                ("A", Kind::Constant),
                ("=", Kind::Punctuation),
                ("\"open", Kind::String),
                ("B", Kind::Constant),
                ("=", Kind::Punctuation),
                ("2", Kind::String),
                ("C", Kind::Constant),
                ("=", Kind::Punctuation),
                ("'open", Kind::String),
            ]
        );
    }

    #[test]
    fn leaves_other_lines_alone() {
        assert!(kinds("just text\nexport\nexportFOO\na:b\n\n   \n").is_empty());
    }

    #[test]
    fn survives_any_input() {
        let pieces =
            ["", "#", " ", "=", ":", "\"", "'", "`", "\\", "$", "{", "}", "é", "日", "\n", "\r\n", "export ", "A"];
        let mut texts = Vec::new();
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    for d in pieces {
                        texts.push(format!("{a}{b}{c}{d}"));
                    }
                }
            }
        }
        for text in &texts {
            let mut previous = 0;
            for span in highlight(text) {
                assert!(
                    span.start >= previous
                        && span.start < span.end
                        && span.end <= text.len()
                        && text.is_char_boundary(span.start)
                        && text.is_char_boundary(span.end),
                    "{text:?}"
                );
                previous = span.end;
            }
        }
    }

    #[test]
    fn encodes_records() {
        let bytes = encode(&[Span { start: 1, end: 3, kind: Kind::Constant }]);
        assert_eq!(bytes, [1, 0, 0, 0, 3, 0, 0, 0, 10, 0, 0, 0]);
    }
}
