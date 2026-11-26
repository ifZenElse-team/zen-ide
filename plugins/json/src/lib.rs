use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
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
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let c = bytes[at];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => at += 1,
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                push(&mut spans, at, at + 1, Kind::Punctuation);
                at += 1;
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                let end = text[at..].find('\n').map_or(text.len(), |newline| at + newline);
                push(&mut spans, at, end, Kind::Comment);
                at = end;
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                let end = text[at + 2..].find("*/").map_or(text.len(), |close| at + 2 + close + 2);
                push(&mut spans, at, end, Kind::Comment);
                at = end;
            }
            b'"' | b'\'' => at = string(text, at, &mut spans),
            b'0'..=b'9' | b'-' | b'+' | b'.' => {
                let length = number_length(&bytes[at..]);
                if length > 0 {
                    push(&mut spans, at, at + length, Kind::Number);
                    at += length;
                } else {
                    at += 1;
                }
            }
            _ if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => {
                let length =
                    bytes[at..].iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_' || **b == b'$').count();
                let word = &text[at..at + length];
                let kind = match word {
                    "true" | "false" | "null" => Some(Kind::Constant),
                    "Infinity" | "NaN" => Some(Kind::Number),
                    _ if followed_by_colon(bytes, at + length) => Some(Kind::Keyword),
                    _ => None,
                };
                if let Some(kind) = kind {
                    push(&mut spans, at, at + length, kind);
                }
                at += length;
            }
            _ => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    spans
}

fn push(spans: &mut Vec<Span>, start: usize, end: usize, kind: Kind) {
    if end > start {
        spans.push(Span { start, end, kind });
    }
}

fn followed_by_colon(bytes: &[u8], at: usize) -> bool {
    bytes[at..].iter().find(|b| !matches!(b, b' ' | b'\t')) == Some(&b':')
}

fn string(text: &str, start: usize, spans: &mut Vec<Span>) -> usize {
    let bytes = text.as_bytes();
    let quote = bytes[start];
    let mut escapes = Vec::new();
    let mut at = start + 1;
    let end = loop {
        match bytes.get(at) {
            None => break text.len(),
            Some(b'\n') => break at,
            Some(&b) if b == quote => break at + 1,
            Some(b'\\') => {
                let Some(next) = text[at + 1..].chars().next() else {
                    escapes.push((at, at + 1));
                    break text.len();
                };
                let mut escape_end = at + 1 + next.len_utf8();
                let digits = match next {
                    'u' => 4,
                    'x' => 2,
                    _ => 0,
                };
                escape_end += bytes[escape_end..].iter().take(digits).take_while(|b| b.is_ascii_hexdigit()).count();
                escapes.push((at, escape_end));
                at = escape_end;
            }
            Some(_) => at += 1,
        }
    };
    let kind = if followed_by_colon(bytes, end) { Kind::Keyword } else { Kind::String };
    let mut piece = start;
    for (escape_start, escape_end) in escapes {
        push(spans, piece, escape_start, kind);
        push(spans, escape_start, escape_end, Kind::Constant);
        piece = escape_end;
    }
    push(spans, piece, end, kind);
    end
}

fn number_length(bytes: &[u8]) -> usize {
    let sign = usize::from(matches!(bytes.first(), Some(b'-' | b'+')));
    let rest = &bytes[sign..];
    for word in [&b"Infinity"[..], b"NaN"] {
        if rest.starts_with(word) && !rest.get(word.len()).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') {
            return sign + word.len();
        }
    }
    if rest.len() > 2 && matches!(&rest[..2], b"0x" | b"0X") && rest[2].is_ascii_hexdigit() {
        return sign + 2 + rest[2..].iter().take_while(|b| b.is_ascii_hexdigit()).count();
    }
    let mut at = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    let integer = at;
    if rest.get(at) == Some(&b'.') {
        let fraction = rest[at + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
        if integer == 0 && fraction == 0 {
            return 0;
        }
        at += 1 + fraction;
    }
    if at == 0 {
        return 0;
    }
    if matches!(rest.get(at), Some(b'e' | b'E')) {
        let exponent_sign = usize::from(matches!(rest.get(at + 1), Some(b'+' | b'-')));
        let digits = rest[(at + 1 + exponent_sign).min(rest.len())..].iter().take_while(|b| b.is_ascii_digit()).count();
        if digits > 0 {
            at += 1 + exponent_sign + digits;
        }
    }
    sign + at
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

    fn assert_well_formed(text: &str) {
        let mut previous = 0;
        for span in highlight(text) {
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
    }

    #[test]
    fn highlights_a_document() {
        let text = r#"{"name": "zen", "n": -1.5e+3, "ok": [true, false, null]}"#;
        assert_eq!(
            kinds(text),
            [
                ("{", Kind::Punctuation),
                ("\"name\"", Kind::Keyword),
                (":", Kind::Punctuation),
                ("\"zen\"", Kind::String),
                (",", Kind::Punctuation),
                ("\"n\"", Kind::Keyword),
                (":", Kind::Punctuation),
                ("-1.5e+3", Kind::Number),
                (",", Kind::Punctuation),
                ("\"ok\"", Kind::Keyword),
                (":", Kind::Punctuation),
                ("[", Kind::Punctuation),
                ("true", Kind::Constant),
                (",", Kind::Punctuation),
                ("false", Kind::Constant),
                (",", Kind::Punctuation),
                ("null", Kind::Constant),
                ("]", Kind::Punctuation),
                ("}", Kind::Punctuation),
            ]
        );
    }

    #[test]
    fn splits_strings_at_escapes() {
        let text = r#"{"a\tb": "x\n\u00e9y\"", "\u12": "\é"}"#;
        let found = kinds(text);
        for expected in [
            ("\"a", Kind::Keyword),
            ("\\t", Kind::Constant),
            ("b\"", Kind::Keyword),
            ("\"x", Kind::String),
            ("\\n", Kind::Constant),
            ("\\u00e9", Kind::Constant),
            ("y", Kind::String),
            ("\\\"", Kind::Constant),
            ("\"", Kind::String),
            ("\\u12", Kind::Constant),
            ("\\é", Kind::Constant),
        ] {
            assert!(found.contains(&expected), "{expected:?} in {found:?}");
        }
        assert_well_formed(text);
    }

    #[test]
    fn highlights_jsonc_and_json5() {
        let text =
            "// top\n{ /* block\n comment */ unquoted: 'single', $id : 0x1F, b: .5, c: +Infinity, d: NaN, e: 5., }";
        let found = kinds(text);
        for expected in [
            ("// top", Kind::Comment),
            ("/* block\n comment */", Kind::Comment),
            ("unquoted", Kind::Keyword),
            ("'single'", Kind::String),
            ("$id", Kind::Keyword),
            ("0x1F", Kind::Number),
            (".5", Kind::Number),
            ("+Infinity", Kind::Number),
            ("NaN", Kind::Number),
            ("5.", Kind::Number),
        ] {
            assert!(found.contains(&expected), "{expected:?} in {found:?}");
        }
    }

    #[test]
    fn survives_invalid_json() {
        assert_eq!(
            kinds("{\"a\": \"open\n\"b\": nul}"),
            [
                ("{", Kind::Punctuation),
                ("\"a\"", Kind::Keyword),
                (":", Kind::Punctuation),
                ("\"open", Kind::String),
                ("\"b\"", Kind::Keyword),
                (":", Kind::Punctuation),
                ("}", Kind::Punctuation),
            ]
        );
        assert_eq!(kinds("- . +x /*"), [("/*", Kind::Comment)]);
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "\"", "'", "\\", "\\u", "/", "/*", "*/", "//", "\n", ":", ",", "{", "-", "+", ".", "0x", "1", "e", "N",
            "Infinity", "true", "é", "日", "😀",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    assert_well_formed(&format!("{a}{b}{c}"));
                }
            }
        }
    }

    #[test]
    fn is_linear_on_large_input() {
        let line = "{\"key\": \"value \\n é\", \"n\": [1, -2.5e3, true, null], \"o\": {\"k\": 'x'}} // c\n";
        let text = line.repeat(1 << 20 >> 6);
        assert!(text.len() > 1 << 20);
        let started = std::time::Instant::now();
        let spans = highlight(&text);
        assert!(spans.len() > 100_000);
        assert!(started.elapsed().as_secs() < 60, "took {:?}", started.elapsed());
        let quote = format!("\"{}", "a".repeat(1 << 20));
        assert_eq!(highlight(&quote).len(), 1);
        let blanks = format!("\"k\"{}:", " ".repeat(1 << 20));
        assert_eq!(highlight(&blanks)[0].kind, Kind::Keyword);
    }

    #[test]
    fn encodes_records() {
        assert_eq!(
            encode(&[Span { start: 1, end: 3, kind: Kind::Punctuation }]),
            [1, 0, 0, 0, 3, 0, 0, 0, 11, 0, 0, 0]
        );
    }
}
