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

fn push(spans: &mut Vec<Span>, start: usize, end: usize, kind: Kind) {
    if end > start {
        spans.push(Span { start, end, kind });
    }
}

fn is_bare(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn run(bytes: &[u8], test: impl Fn(u8) -> bool) -> usize {
    bytes.iter().take_while(|byte| test(**byte)).count()
}

pub fn highlight(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut nesting: Vec<u8> = Vec::new();
    let mut expect_key = true;
    let mut line_start = true;
    let mut at = 0;
    while at < bytes.len() {
        let c = bytes[at];
        match c {
            b'\n' => {
                if nesting.is_empty() {
                    expect_key = true;
                }
                line_start = true;
                at += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                at += 1;
                continue;
            }
            b'#' => {
                let end = line_end(text, at);
                push(&mut spans, at, end, Kind::Comment);
                at = end;
                continue;
            }
            _ => {}
        }
        if line_start {
            line_start = false;
            if !nesting.is_empty() {
                let line = &text[at..line_end(text, at)];
                if looks_like_header(line) || (nesting.last() == Some(&b'[') && looks_like_key_value(line)) {
                    nesting.clear();
                    expect_key = true;
                }
            }
            if nesting.is_empty() && c == b'[' {
                at = header(text, at, &mut spans);
                expect_key = false;
                continue;
            }
        }
        if expect_key {
            match c {
                _ if is_bare(c) => {
                    let end = at + run(&bytes[at..], is_bare);
                    push(&mut spans, at, end, Kind::Constant);
                    at = end;
                    continue;
                }
                b'"' => {
                    at = basic_string(text, at, false, Kind::Constant, false, &mut spans);
                    continue;
                }
                b'\'' => {
                    at = literal_string(text, at, false, Kind::Constant, &mut spans);
                    continue;
                }
                b'.' => {
                    push(&mut spans, at, at + 1, Kind::Punctuation);
                    at += 1;
                    continue;
                }
                b'=' => {
                    push(&mut spans, at, at + 1, Kind::Punctuation);
                    expect_key = false;
                    at += 1;
                    continue;
                }
                _ => expect_key = false,
            }
        }
        let rest = &text[at..];
        match c {
            b'"' => at = basic_string(text, at, rest.starts_with("\"\"\""), Kind::String, true, &mut spans),
            b'\'' => at = literal_string(text, at, rest.starts_with("'''"), Kind::String, &mut spans),
            b'[' | b'{' => {
                push(&mut spans, at, at + 1, Kind::Punctuation);
                nesting.push(c);
                expect_key = c == b'{';
                at += 1;
            }
            b']' | b'}' => {
                push(&mut spans, at, at + 1, Kind::Punctuation);
                nesting.pop();
                at += 1;
            }
            b',' => {
                push(&mut spans, at, at + 1, Kind::Punctuation);
                expect_key = nesting.last() == Some(&b'{');
                at += 1;
            }
            b'=' | b'.' => {
                push(&mut spans, at, at + 1, Kind::Punctuation);
                at += 1;
            }
            b'+' | b'-' if rest[1..].starts_with("inf") || rest[1..].starts_with("nan") => {
                push(&mut spans, at, at + 4, Kind::Number);
                at += 4;
            }
            b'+' | b'-' if bytes.get(at + 1).is_some_and(u8::is_ascii_digit) => {
                let end = number_end(bytes, at);
                push(&mut spans, at, end, Kind::Number);
                at = end;
            }
            _ if c.is_ascii_digit() => {
                let end = number_end(bytes, at);
                push(&mut spans, at, end, Kind::Number);
                at = end;
            }
            _ if is_bare(c) => {
                let end = at + run(&bytes[at..], is_bare);
                match &text[at..end] {
                    "true" | "false" => push(&mut spans, at, end, Kind::Constant),
                    "inf" | "nan" => push(&mut spans, at, end, Kind::Number),
                    _ => {}
                }
                at = end;
            }
            _ => at += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    spans
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |newline| at + newline)
}

fn header(text: &str, at: usize, spans: &mut Vec<Span>) -> usize {
    let bytes = text.as_bytes();
    let double = text[at..].starts_with("[[");
    let open = if double { 2 } else { 1 };
    push(spans, at, at + open, Kind::Punctuation);
    let mut index = at + open;
    while let Some(&c) = bytes.get(index) {
        match c {
            b'\n' | b'#' => break,
            b' ' | b'\t' => index += 1,
            b'.' => {
                push(spans, index, index + 1, Kind::Punctuation);
                index += 1;
            }
            b'"' => index = basic_string(text, index, false, Kind::Keyword, false, spans),
            b'\'' => index = literal_string(text, index, false, Kind::Keyword, spans),
            b']' => {
                let close = if double && bytes.get(index + 1) == Some(&b']') { 2 } else { 1 };
                push(spans, index, index + close, Kind::Punctuation);
                return index + close;
            }
            _ if is_bare(c) => {
                let end = index + run(&bytes[index..], is_bare);
                push(spans, index, end, Kind::Keyword);
                index = end;
            }
            _ => index += text[index..].chars().next().map_or(1, char::len_utf8),
        }
    }
    index
}

fn basic_string(
    text: &str,
    at: usize,
    multiline: bool,
    kind: Kind,
    split_escapes: bool,
    spans: &mut Vec<Span>,
) -> usize {
    let bytes = text.as_bytes();
    let mut piece = at;
    let mut index = at + if multiline { 3 } else { 1 };
    let end = loop {
        match bytes.get(index) {
            None => break text.len(),
            Some(b'\\') => {
                let length = escape_length(&text[index..]);
                if split_escapes {
                    push(spans, piece, index, kind);
                    push(spans, index, index + length, Kind::Constant);
                    piece = index + length;
                }
                index += length;
            }
            Some(b'"') if multiline => {
                if text[index..].starts_with("\"\"\"") {
                    break index + 3 + run(&bytes[index + 3..], |byte| byte == b'"').min(2);
                }
                index += 1;
            }
            Some(b'"') => break index + 1,
            Some(b'\n') if !multiline => break index,
            Some(_) => index += 1,
        }
    };
    push(spans, piece, end, kind);
    end
}

fn escape_length(text: &str) -> usize {
    let hex = |digits: usize| 2 + run(&text.as_bytes()[2..], |byte| byte.is_ascii_hexdigit()).min(digits);
    match text[1..].chars().next() {
        None | Some(' ' | '\t' | '\r' | '\n') => 1,
        Some('u') => hex(4),
        Some('U') => hex(8),
        Some('x') => hex(2),
        Some(other) => 1 + other.len_utf8(),
    }
}

fn literal_string(text: &str, at: usize, multiline: bool, kind: Kind, spans: &mut Vec<Span>) -> usize {
    let end = if multiline {
        let body = at + 3;
        text[body..].find("'''").map_or(text.len(), |close| {
            let close = body + close;
            close + 3 + run(&text.as_bytes()[close + 3..], |byte| byte == b'\'').min(2)
        })
    } else {
        let line = line_end(text, at);
        text[at + 1..line].find('\'').map_or(line, |close| at + 1 + close + 1)
    };
    push(spans, at, end, kind);
    end
}

fn number_end(bytes: &[u8], at: usize) -> usize {
    let part = |from: usize| {
        from + run(&bytes[from..], |byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'+' | b'-')
        })
    };
    let end = part(at + 1);
    let date = &bytes[at..end];
    let is_date = date.len() == 10 && date[4] == b'-' && date[7] == b'-';
    let time_follows = bytes.get(end) == Some(&b' ')
        && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)
        && bytes.get(end + 2).is_some_and(u8::is_ascii_digit)
        && bytes.get(end + 3) == Some(&b':');
    if is_date && time_follows { part(end + 1) } else { end }
}

fn looks_like_header(line: &str) -> bool {
    let Some(inner) = line.strip_prefix("[[").or_else(|| line.strip_prefix('[')) else {
        return false;
    };
    let Some(close) = inner.find(']') else {
        return false;
    };
    let names = inner[..close].trim();
    let after = inner[close..].trim_start_matches(']').trim();
    names.bytes().next().is_some_and(is_bare)
        && names.bytes().all(|byte| is_bare(byte) || matches!(byte, b'.' | b' ' | b'\t'))
        && (after.is_empty() || after.starts_with('#'))
}

fn looks_like_key_value(line: &str) -> bool {
    let bytes = line.as_bytes();
    let key = run(bytes, |byte| is_bare(byte) || matches!(byte, b'.' | b' ' | b'\t'));
    key > 0 && is_bare(bytes[0]) && bytes.get(key) == Some(&b'=') && bytes.get(key + 1) != Some(&b'=')
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

    use Kind::{Comment, Constant, Keyword, Number, Punctuation, String};

    #[test]
    fn highlights_tables_keys_and_values() {
        let text = "# config\n[server.\"main\"]\nhost = 'local' # here\nport = 8_080\n\n[[bin]]\nname.first = \"a\\tb\\u00E9\"\nok = true\n";
        assert_eq!(
            kinds(text),
            [
                ("# config", Comment),
                ("[", Punctuation),
                ("server", Keyword),
                (".", Punctuation),
                ("\"main\"", Keyword),
                ("]", Punctuation),
                ("host", Constant),
                ("=", Punctuation),
                ("'local'", String),
                ("# here", Comment),
                ("port", Constant),
                ("=", Punctuation),
                ("8_080", Number),
                ("[[", Punctuation),
                ("bin", Keyword),
                ("]]", Punctuation),
                ("name", Constant),
                (".", Punctuation),
                ("first", Constant),
                ("=", Punctuation),
                ("\"a", String),
                ("\\t", Constant),
                ("b", String),
                ("\\u00E9", Constant),
                ("\"", String),
                ("ok", Constant),
                ("=", Punctuation),
                ("true", Constant),
            ]
        );
    }

    #[test]
    fn highlights_numbers_and_dates() {
        let text = "a = [0xDEAD_beef, 0o755, 0b1101, -3.14e+2, +inf, -nan, nan, 1e6]\n\
                    b = 1979-05-27T07:32:00.999999-07:00\nc = 1979-05-27 07:32:00Z\nd = 1979-05-27\ne = 07:32:00\n";
        let numbers: Vec<&str> = kinds(text).into_iter().filter(|(_, kind)| *kind == Number).map(|(s, _)| s).collect();
        assert_eq!(
            numbers,
            [
                "0xDEAD_beef",
                "0o755",
                "0b1101",
                "-3.14e+2",
                "+inf",
                "-nan",
                "nan",
                "1e6",
                "1979-05-27T07:32:00.999999-07:00",
                "1979-05-27 07:32:00Z",
                "1979-05-27",
                "07:32:00",
            ]
        );
    }

    #[test]
    fn highlights_multi_line_strings_and_inline_tables() {
        let text = "s = \"\"\"\nline \\\n  \"q\" \"\"\"\"\nt = '''\nraw \\n '''\ndep = { version = \"1\", features = [\"a\", 'b'], optional = false }\nnext = 1\n";
        let found = kinds(text);
        for expected in [
            ("\"\"\"\nline ", String),
            ("\\", Constant),
            ("\n  \"q\" \"\"\"\"", String),
            ("'''\nraw \\n '''", String),
            ("{", Punctuation),
            ("version", Constant),
            ("\"1\"", String),
            (",", Punctuation),
            ("features", Constant),
            ("\"a\"", String),
            ("'b'", String),
            ("optional", Constant),
            ("false", Constant),
            ("}", Punctuation),
            ("next", Constant),
            ("1", Number),
        ] {
            assert!(found.contains(&expected), "{expected:?} in {found:?}");
        }
        assert!(!found.contains(&("raw", Constant)), "a multi-line string hides keys: {found:?}");
    }

    #[test]
    fn multi_line_arrays_keep_values_as_values() {
        let text = "members = [\n  \"a\",\n  # note\n  [1, 2],\n]\n[x]\n";
        let found = kinds(text);
        assert!(found.contains(&("\"a\"", String)));
        assert!(found.contains(&("# note", Comment)));
        assert!(found.contains(&("1", Number)));
        assert!(found.contains(&("x", Keyword)), "the array is closed: {found:?}");
        assert_eq!(found.iter().filter(|(_, kind)| *kind == Keyword).count(), 1, "`[1, 2]` is no header");
    }

    #[test]
    fn recovers_from_an_unclosed_array() {
        let found = kinds("a = [1,\n[table]\nkey = 2\n");
        assert!(found.contains(&("table", Keyword)), "{found:?}");
        assert!(found.contains(&("key", Constant)), "{found:?}");
        let found = kinds("a = [1,\nkey = 2\n");
        assert!(found.contains(&("key", Constant)), "{found:?}");
    }

    #[test]
    fn highlights_the_workspace_manifest() {
        let text = include_str!("../../../Cargo.toml");
        assert_well_formed(text);
        let found = kinds(text);
        for expected in [
            ("workspace", Keyword),
            ("package", Keyword),
            ("lints", Keyword),
            ("resolver", Constant),
            ("\"3\"", String),
            ("members", Constant),
            ("\"crates/platform/*\"", String),
            ("zen-core", Constant),
            ("default-features", Constant),
            ("false", Constant),
            ("priority", Constant),
            ("-1", Number),
            ("opt-level", Constant),
            ("3", Number),
        ] {
            assert!(found.contains(&expected), "{expected:?}");
        }
        assert!(!found.iter().any(|(word, kind)| *kind == Constant && word.contains('/')), "no value is a key");
    }

    #[test]
    fn highlights_a_lock_file() {
        let text = "# This file is automatically @generated by Cargo.\nversion = 4\n\n[[package]]\nname = \"ab_glyph\"\nversion = \"0.2.32\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"01c0457472c38ea5bd1c3b5ada5e368271cb550be7a4ca4a0b4634e9913f6cc2\"\ndependencies = [\n \"ab_glyph_rasterizer\",\n \"owned_ttf_parser\",\n]\n\n[[package]]\nname = \"zen-plugin-toml\"\n";
        assert_well_formed(text);
        let found = kinds(text);
        for expected in [
            ("# This file is automatically @generated by Cargo.", Comment),
            ("version", Constant),
            ("4", Number),
            ("[[", Punctuation),
            ("package", Keyword),
            ("]]", Punctuation),
            ("\"0.2.32\"", String),
            ("checksum", Constant),
            ("dependencies", Constant),
            ("\"ab_glyph_rasterizer\"", String),
            ("\"owned_ttf_parser\"", String),
            ("\"zen-plugin-toml\"", String),
        ] {
            assert!(found.contains(&expected), "{expected:?}");
        }
        assert_eq!(found.iter().filter(|(word, _)| *word == "package").count(), 2);
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "'",
            "\"",
            "'''",
            "\"\"\"",
            "\\",
            "\\u",
            "[",
            "[[",
            "]",
            "{",
            "}",
            "=",
            ",",
            ".",
            "#",
            "\n",
            " ",
            "1",
            "-",
            "+",
            "1979-05-27",
            "a",
            "true",
            "é",
            "日",
            "\u{1F600}",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    assert_well_formed(&format!("{a}{b}{c}"));
                    assert_well_formed(&format!("{a}{b}\n{c}"));
                }
            }
        }
    }

    #[test]
    fn encodes_records() {
        assert_eq!(encode(&[Span { start: 1, end: 3, kind: Punctuation }]), [1, 0, 0, 0, 3, 0, 0, 0, 11, 0, 0, 0]);
    }
}
