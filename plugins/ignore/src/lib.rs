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
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let start = offset;
        offset += line.len();

        if content.starts_with('#') {
            push(&mut spans, start, start + content.len(), Kind::Comment);
            continue;
        }
        if let Some(value) = content.strip_prefix("syntax:") {
            let keyword_end = start + "syntax:".len();
            push(&mut spans, start, keyword_end, Kind::Keyword);
            let value = value.trim();
            if !value.is_empty() {
                let value_start = keyword_end + content["syntax:".len()..].find(value).unwrap_or(0);
                push(&mut spans, value_start, value_start + value.len(), Kind::String);
            }
            continue;
        }
        pattern(content, start, &mut spans);
    }
    spans
}

fn pattern(text: &str, base: usize, spans: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    let mut at = 0;
    if bytes.first() == Some(&b'!') {
        push(spans, base, base + 1, Kind::Keyword);
        at = 1;
    }
    while at < bytes.len() {
        let rest = &text[at..];
        match bytes[at] {
            b'\\' => {
                if let Some(escaped) = rest[1..].chars().next() {
                    let end = at + 1 + escaped.len_utf8();
                    push(spans, base + at, base + end, Kind::Constant);
                    at = end;
                    continue;
                }
            }
            b'*' => {
                let run = rest.bytes().take_while(|&b| b == b'*').count();
                push(spans, base + at, base + at + run, Kind::Keyword);
                at += run;
                continue;
            }
            b'?' => push(spans, base + at, base + at + 1, Kind::Keyword),
            b'/' => push(spans, base + at, base + at + 1, Kind::Punctuation),
            b'[' => {
                if let Some(end) = class_end(rest) {
                    push(spans, base + at, base + at + end, Kind::String);
                    at += end;
                    continue;
                }
            }
            _ => {}
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
}

fn class_end(text: &str) -> Option<usize> {
    let mut at = 1;
    if text[at..].starts_with(['!', '^']) {
        at += 1;
    }
    at += text[at..].chars().next()?.len_utf8();
    let bytes = text.as_bytes();
    while at < bytes.len() {
        match bytes[at] {
            b']' => return Some(at + 1),
            b'\\' if at + 1 < bytes.len() => at += 1 + text[at + 1..].chars().next().map_or(1, char::len_utf8),
            _ => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    None
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
    fn highlights_comments_and_negations() {
        let text = "# Build output\ntarget\n!keep.txt\n\\#literal\n\\!bang\r\n";
        assert_eq!(
            kinds(text),
            [("# Build output", Kind::Comment), ("!", Kind::Keyword), ("\\#", Kind::Constant), ("\\!", Kind::Constant),]
        );
    }

    #[test]
    fn highlights_globs_classes_and_separators() {
        let text = "/build/\n**/logs/*.log\nfile?.[ch]\n[!a-z]x\n[]]y\nname\\ with\\*\n";
        assert_eq!(
            kinds(text),
            [
                ("/", Kind::Punctuation),
                ("/", Kind::Punctuation),
                ("**", Kind::Keyword),
                ("/", Kind::Punctuation),
                ("/", Kind::Punctuation),
                ("*", Kind::Keyword),
                ("?", Kind::Keyword),
                ("[ch]", Kind::String),
                ("[!a-z]", Kind::String),
                ("[]]", Kind::String),
                ("\\ ", Kind::Constant),
                ("\\*", Kind::Constant),
            ]
        );
    }

    #[test]
    fn highlights_mercurial_syntax_lines() {
        let text = "syntax: glob\n*.orig\nsyntax:regexp\n";
        assert_eq!(
            kinds(text),
            [
                ("syntax:", Kind::Keyword),
                ("glob", Kind::String),
                ("*", Kind::Keyword),
                ("syntax:", Kind::Keyword),
                ("regexp", Kind::String),
            ]
        );
    }

    #[test]
    fn leaves_plain_paths_and_open_classes_alone() {
        assert!(kinds("node_modules\n.env.local\n [unclosed\n  # not a comment\n").is_empty());
    }

    #[test]
    fn survives_any_input() {
        let pieces = ["", "#", "!", "\\", "*", "?", "[", "]", "/", "é", "日", " ", "\n", "\r\n", "syntax:", "^"];
        let mut texts = Vec::new();
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    texts.push(format!("{a}{b}{c}"));
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
