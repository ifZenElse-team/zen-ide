use std::ops::Range;

pub const SHORTEST_WORD: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Context {
    Code,
    Literal,
    Comment,
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub range: Range<usize>,
    pub context: Context,
}

pub fn words(text: &str, region: &Region) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let range = region.range.start.min(text.len())..region.range.end.min(text.len());
    if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
        return found;
    }
    match region.context {
        Context::Code => run_words(text, range, true, &mut found),
        Context::Literal => prose(text, range, true, &mut found),
        Context::Comment | Context::Text => prose(text, range, false, &mut found),
    }
    found
}

fn prose(text: &str, range: Range<usize>, literal: bool, found: &mut Vec<Range<usize>>) {
    let mut at = range.start;
    while at < range.end {
        let rest = &text[at..range.end];
        let Some(start) = rest.find(|c: char| !c.is_whitespace()) else { break };
        let start = at + start;
        let end = text[start..range.end].find(char::is_whitespace).map_or(range.end, |end| start + end);
        if !skipped(&text[start..end]) {
            chunk_words(text, start..end, literal, found);
        }
        at = end;
    }
}

fn skipped(chunk: &str) -> bool {
    let trimmed = chunk.trim_matches(|c: char| !c.is_alphanumeric());
    if chunk.contains("://") || trimmed.starts_with("www.") || trimmed.starts_with("mailto:") {
        return true;
    }
    if path_or_dotted(chunk, trimmed) {
        return true;
    }
    if let Some((user, host)) = trimmed.split_once('@')
        && !user.is_empty()
        && host.split('.').filter(|part| !part.is_empty()).count() >= 2
    {
        return true;
    }
    hexadecimal(trimmed)
}

fn path_or_dotted(chunk: &str, trimmed: &str) -> bool {
    let near = |byte: Option<&u8>| byte.is_some_and(|byte| byte.is_ascii_alphanumeric() || b".~".contains(byte));
    let raw = chunk.as_bytes();
    let bytes = trimmed.as_bytes();
    trimmed.contains("::")
        || (0..raw.len())
            .any(|at| raw[at] == b'/' && (near(at.checked_sub(1).and_then(|at| raw.get(at))) || near(raw.get(at + 1))))
        || (1..bytes.len().saturating_sub(1)).any(|at| {
            bytes[at] == b'.' && bytes[at - 1].is_ascii_alphanumeric() && bytes[at + 1].is_ascii_alphanumeric()
        })
}

fn hexadecimal(word: &str) -> bool {
    if let Some(colour) = word.strip_prefix('#') {
        return matches!(colour.len(), 3 | 4 | 6 | 8) && colour.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    if let Some(digits) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        return !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'_');
    }
    !word.is_empty()
        && word.bytes().all(|byte| byte.is_ascii_hexdigit())
        && word.bytes().any(|byte| byte.is_ascii_digit())
        && word.bytes().any(|byte| byte.is_ascii_alphabetic())
}

fn chunk_words(text: &str, range: Range<usize>, literal: bool, found: &mut Vec<Range<usize>>) {
    let mut run: Option<usize> = None;
    let mut at = range.start;
    while at < range.end {
        let Some(c) = text[at..range.end].chars().next() else { break };
        let width = c.len_utf8();
        if literal && (c == '\\' || c == '%') {
            if let Some(start) = run.take() {
                run_words(text, start..at, false, found);
            }
            at += width;
            if c == '\\' {
                at += text[at..range.end].chars().next().map_or(0, char::len_utf8);
            } else {
                at += format_verb(&text[at..range.end]);
            }
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '\'' || c == '\u{2019}' {
            run.get_or_insert(at);
        } else if let Some(start) = run.take() {
            run_words(text, start..at, false, found);
        }
        at += width;
    }
    if let Some(start) = run {
        run_words(text, start..range.end, false, found);
    }
}

fn format_verb(rest: &str) -> usize {
    let flags = rest.bytes().take_while(|byte| b"+-# 0123456789.*".contains(byte)).count();
    match rest.as_bytes().get(flags) {
        Some(byte) if byte.is_ascii_alphabetic() => flags + 1,
        _ => 0,
    }
}

fn run_words(text: &str, range: Range<usize>, code: bool, found: &mut Vec<Range<usize>>) {
    let run = &text[range.clone()];
    if run.chars().any(|c| c.is_alphabetic() && !c.is_ascii()) {
        return;
    }
    let bytes = run.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !is_letter(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_letter(bytes[at]) {
            at += 1;
        }
        let digit = |index: Option<usize>| index.and_then(|index| bytes.get(index)).is_some_and(u8::is_ascii_digit);
        if !code && (digit(start.checked_sub(1)) || digit(Some(at))) {
            continue;
        }
        split_case(text, range.start + start..range.start + at, found);
    }
}

fn is_letter(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'\'' || byte >= 0x80
}

fn split_case(text: &str, range: Range<usize>, found: &mut Vec<Range<usize>>) {
    let bytes = text[range.clone()].as_bytes();
    let mut start = 0;
    for at in 1..bytes.len() {
        let (previous, current) = (bytes[at - 1], bytes[at]);
        let next_lower = bytes.get(at + 1).is_some_and(u8::is_ascii_lowercase);
        if (previous.is_ascii_lowercase() && current.is_ascii_uppercase())
            || (previous.is_ascii_uppercase() && current.is_ascii_uppercase() && next_lower)
        {
            word(text, range.start + start..range.start + at, found);
            start = at;
        }
    }
    word(text, range.start + start..range.end, found);
}

fn word(text: &str, range: Range<usize>, found: &mut Vec<Range<usize>>) {
    let piece = &text[range.clone()];
    let apostrophe = |c: char| c == '\'' || c == '\u{2019}';
    let leading = piece.len() - piece.trim_start_matches(apostrophe).len();
    let mut trimmed = piece.trim_matches(apostrophe);
    for possessive in ["'s", "\u{2019}s"] {
        if let Some(stem) = trimmed.strip_suffix(possessive) {
            trimmed = stem;
        }
    }
    let letters = trimmed.chars().filter(char::is_ascii_alphabetic).count();
    if letters < SHORTEST_WORD || trimmed.chars().all(|c| !c.is_ascii_lowercase()) {
        return;
    }
    let mut lower = trimmed.bytes().map(|byte| byte.to_ascii_lowercase());
    let first = lower.next();
    if lower.all(|byte| Some(byte) == first) {
        return;
    }
    let start = range.start + leading;
    found.push(start..start + trimmed.len());
}
