use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Comment = 3,
    Punctuation = 11,
    Heading = 12,
    Strong = 13,
    Emphasis = 14,
    Link = 15,
    Code = 16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub fn highlight(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let indent = content.len() - content.trim_start_matches(' ').len();
        let trimmed = &content[indent..];
        let start = offset + indent;
        let end = offset + content.len();
        offset += line.len();

        if let Some((marker, length)) = fence {
            if indent < 4
                && is_fence(trimmed).is_some_and(|(m, l)| m == marker && l >= length && trimmed[l..].trim().is_empty())
            {
                fence = None;
            }
            push(&mut spans, offset - line.len(), end, Kind::Code);
            continue;
        }
        if indent < 4
            && let Some(opening) = is_fence(trimmed)
        {
            fence = Some(opening);
            push(&mut spans, start, end, Kind::Code);
            continue;
        }
        if indent >= 4 {
            push(&mut spans, start, end, Kind::Code);
            continue;
        }
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && (trimmed.len() == hashes || trimmed[hashes..].starts_with([' ', '\t'])) {
            push(&mut spans, start, start + hashes, Kind::Keyword);
            let title = trimmed[hashes..].trim_start_matches([' ', '\t']);
            push(&mut spans, end - title.len(), end, Kind::Heading);
            continue;
        }
        if is_rule(trimmed) {
            push(&mut spans, start, end, Kind::Punctuation);
            continue;
        }

        let mut at = 0;
        let bytes = trimmed.as_bytes();
        while at < bytes.len() && bytes[at] == b'>' {
            push(&mut spans, start + at, start + at + 1, Kind::Punctuation);
            at += 1;
            while at < bytes.len() && bytes[at] == b' ' {
                at += 1;
            }
        }
        if let Some(marker) = list_marker(&trimmed[at..]) {
            push(&mut spans, start + at, start + at + marker, Kind::Keyword);
            at += marker;
            let rest = &trimmed[at..];
            if rest.starts_with(" [ ]") || rest.starts_with(" [x]") || rest.starts_with(" [X]") {
                push(&mut spans, start + at + 1, start + at + 4, Kind::Keyword);
                at += 4;
            }
        }
        inline(&trimmed[at..], start + at, &mut spans);
    }
    spans
}

fn is_fence(line: &str) -> Option<(u8, usize)> {
    let marker = *line.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let length = line.bytes().take_while(|&b| b == marker).count();
    (length >= 3 && !(marker == b'`' && line[length..].contains('`'))).then_some((marker, length))
}

fn is_rule(line: &str) -> bool {
    let marks: Vec<u8> = line.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    marks.len() >= 3 && marks.iter().all(|&b| b == marks[0]) && matches!(marks[0], b'-' | b'*' | b'_')
}

fn list_marker(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'-' | b'*' | b'+') && bytes[1] == b' ' {
        return Some(1);
    }
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if (1..=9).contains(&digits)
        && bytes.len() > digits + 1
        && matches!(bytes[digits], b'.' | b')')
        && bytes[digits + 1] == b' '
    {
        return Some(digits + 1);
    }
    None
}

fn inline(text: &str, base: usize, spans: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let rest = &text[at..];
        match bytes[at] {
            b'\\' if at + 1 < bytes.len() => {
                at += 1 + rest[1..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            b'`' => {
                let ticks = rest.bytes().take_while(|&b| b == b'`').count();
                let fence = &rest[..ticks];
                if let Some(close) = rest[ticks..].find(fence) {
                    let end = at + ticks + close + ticks;
                    push(spans, base + at, base + end, Kind::Code);
                    at = end;
                    continue;
                }
                at += ticks;
                continue;
            }
            b'<' if rest.starts_with("<!--") => {
                let end = rest.find("-->").map_or(bytes.len(), |close| at + close + 3);
                push(spans, base + at, base + end, Kind::Comment);
                at = end;
                continue;
            }
            b'<' if rest.starts_with("<http://") || rest.starts_with("<https://") => {
                if let Some(close) = rest.find('>') {
                    push(spans, base + at, base + at + close + 1, Kind::Link);
                    at += close + 1;
                    continue;
                }
            }
            b'!' | b'[' => {
                let open = if bytes[at] == b'!' { at + 1 } else { at };
                if bytes.get(open) == Some(&b'[')
                    && let Some(close) = text[open..].find("](")
                    && let Some(paren) = text[open + close + 2..].find(')')
                {
                    let label_end = open + close + 1;
                    let target_end = open + close + 2 + paren + 1;
                    push(spans, base + at, base + label_end, Kind::Link);
                    push(spans, base + label_end, base + target_end, Kind::String);
                    at = target_end;
                    continue;
                }
            }
            b'*' | b'_' => {
                let marker = bytes[at];
                let run = rest.bytes().take_while(|&b| b == marker).count().min(2);
                let delimiter = &rest[..run];
                let flanking = rest[run..].chars().next().is_some_and(|c| !c.is_whitespace());
                let word_inside = marker == b'_' && at > 0 && bytes[at - 1].is_ascii_alphanumeric();
                if flanking
                    && !word_inside
                    && let Some(close) = rest[run..].find(delimiter).filter(|&close| close > 0)
                {
                    let end = at + run + close + run;
                    let kind = if run == 2 { Kind::Strong } else { Kind::Emphasis };
                    push(spans, base + at, base + end, kind);
                    at = end;
                    continue;
                }
                at += run;
                continue;
            }
            b'|' => push(spans, base + at, base + at + 1, Kind::Punctuation),
            _ => {}
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

pub fn headings(text: &str) -> Vec<Heading> {
    let mut found = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut offset = 0;
    let mut previous: Option<(usize, usize, &str)> = None;
    for (number, line) in text.split_inclusive('\n').enumerate() {
        let start = offset;
        offset += line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        let trimmed = content.trim_start();
        let indent = content.len() - trimmed.len();
        if let Some((mark, count)) = fence {
            let run = trimmed.bytes().take_while(|&byte| byte == mark).count();
            if indent < 4 && run >= count && trimmed[run..].trim().is_empty() {
                fence = None;
            }
            previous = None;
            continue;
        }
        let mark = trimmed.bytes().next();
        if indent < 4 && matches!(mark, Some(b'`' | b'~')) {
            let mark = mark.unwrap_or(b'`');
            let run = trimmed.bytes().take_while(|&byte| byte == mark).count();
            if run >= 3 {
                fence = Some((mark, run));
                previous = None;
                continue;
            }
        }
        let hashes = trimmed.bytes().take_while(|&byte| byte == b'#').count();
        if indent < 4 && (1..=6).contains(&hashes) && trimmed[hashes..].chars().next().is_none_or(char::is_whitespace) {
            let title = trimmed[hashes..].trim().trim_end_matches('#').trim_end().to_owned();
            found.push(Heading { level: hashes as u8, text: title, start, end: start + content.len(), line: number });
            previous = None;
            continue;
        }
        let underline = indent < 4
            && !trimmed.is_empty()
            && (trimmed.trim_end().bytes().all(|byte| byte == b'=')
                || trimmed.trim_end().bytes().all(|byte| byte == b'-'));
        if underline
            && let Some((from, line_number, title)) = previous
            && !title.trim().is_empty()
        {
            let level = if trimmed.starts_with('=') { 1 } else { 2 };
            found.push(Heading {
                level,
                text: title.trim().to_owned(),
                start: from,
                end: start + content.len(),
                line: line_number,
            });
            previous = None;
            continue;
        }
        previous = (!trimmed.is_empty() && indent < 4 && !trimmed.starts_with(['>', '-', '*', '+', '|']))
            .then_some((start, number, content));
    }
    found
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Symbol>,
}

const MAX_ENTRIES: usize = 20_000;

pub fn outline(text: &str) -> Vec<Symbol> {
    let headings: Vec<Heading> = headings(text).into_iter().take(MAX_ENTRIES).collect();
    fn build(text: &str, headings: &[Heading], from: usize, until_level: u8) -> (Vec<Symbol>, usize) {
        let mut symbols = Vec::new();
        let mut at = from;
        while at < headings.len() && headings[at].level > until_level {
            let heading = &headings[at];
            let end = headings[at + 1..]
                .iter()
                .find(|next| next.level <= heading.level)
                .map_or(text.len(), |next| next.start)
                .max(heading.end);
            let (children, next) = build(text, headings, at + 1, heading.level);
            symbols.push(Symbol { name: heading.text.clone(), start: heading.start, end, children });
            at = next;
        }
        (symbols, at)
    }
    build(text, &headings, 0, 0).0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

pub fn fold(text: &str) -> Vec<Fold> {
    let line_of = |at: usize| text[..at.min(text.len())].matches('\n').count() as u32;
    let mut folds = Vec::new();
    fn sections(symbols: &[Symbol], line_of: &dyn Fn(usize) -> u32, text: &str, folds: &mut Vec<Fold>) {
        for symbol in symbols {
            let body = text[symbol.start..symbol.end].trim_end();
            let (start_line, end_line) = (line_of(symbol.start), line_of(symbol.start + body.len()));
            if end_line > start_line {
                folds.push(Fold { start_line, end_line });
            }
            sections(&symbol.children, line_of, text, folds);
        }
    }
    sections(&outline(text), &line_of, text, &mut folds);
    let mut open: Option<(u32, u8, usize)> = None;
    for (number, line) in text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        let mark = trimmed.bytes().next();
        let run = mark.map_or(0, |mark| trimmed.bytes().take_while(|&byte| byte == mark).count());
        match open {
            Some((start, fence, count)) if mark == Some(fence) && run >= count && trimmed[run..].trim().is_empty() => {
                if number as u32 > start {
                    folds.push(Fold { start_line: start, end_line: number as u32 });
                }
                open = None;
            }
            None if matches!(mark, Some(b'`' | b'~')) && run >= 3 => {
                open = Some((number as u32, mark.unwrap_or(b'`'), run))
            }
            _ => {}
        }
    }
    folds.sort();
    folds.dedup();
    folds
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn outline_json(symbols: &[Symbol]) -> String {
    fn write(out: &mut String, symbols: &[Symbol], depth: usize) {
        out.push('[');
        for (index, symbol) in symbols.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            json_string(out, &symbol.name);
            out.push_str(&format!(
                ",\"kind\":\"module\",\"start\":{},\"end\":{},\"children\":",
                symbol.start, symbol.end
            ));
            if depth < 30 {
                write(out, &symbol.children, depth + 1)
            } else {
                out.push_str("[]")
            }
            out.push('}');
        }
        out.push(']');
    }
    let mut out = String::new();
    write(&mut out, symbols, 0);
    out
}

pub fn encode_folds(folds: &[Fold]) -> Vec<u8> {
    let mut out = Vec::with_capacity(folds.len() * 8);
    for fold in folds {
        out.extend_from_slice(&fold.start_line.to_le_bytes());
        out.extend_from_slice(&fold.end_line.to_le_bytes());
    }
    out
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
    1
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

fn call(ptr: i32, len: i32, run: impl FnOnce(&str) -> Vec<u8>) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        run(if valid { std::str::from_utf8(&input[..len as usize]).unwrap_or("") } else { "" })
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_outline(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |text| outline_json(&outline(text)).into_bytes())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_fold(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |text| encode_folds(&fold(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_headings_are_the_structure_and_their_sections_fold() {
        let text = "# Title\n\nIntro.\n\n## One\n\n```sh\n# not a heading\n```\n\nSub\n---\n\n### Deep\n\ntext\n\n## Two\n\nend\n";
        let names: Vec<(u8, String)> =
            headings(text).into_iter().map(|heading| (heading.level, heading.text)).collect();
        assert_eq!(
            names,
            [
                (1, "Title".to_owned()),
                (2, "One".to_owned()),
                (2, "Sub".to_owned()),
                (3, "Deep".to_owned()),
                (2, "Two".to_owned())
            ]
        );
        let symbols = outline(text);
        assert_eq!(symbols.len(), 1);
        let children: Vec<&str> = symbols[0].children.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(children, ["One", "Sub", "Two"]);
        assert_eq!(symbols[0].children[1].children[0].name, "Deep");
        assert_eq!(symbols[0].end, text.len());
        assert_eq!(
            &text[symbols[0].children[0].start..symbols[0].children[0].end],
            "## One\n\n```sh\n# not a heading\n```\n\n"
        );
        let folds: Vec<(u32, u32)> = fold(text).into_iter().map(|fold| (fold.start_line, fold.end_line)).collect();
        assert!(
            folds.contains(&(0, 19)) && folds.contains(&(6, 8)) && folds.contains(&(4, 8)) && folds.contains(&(10, 15)),
            "{folds:?}"
        );
        let json = outline_json(&outline("# A \"q\"\n"));
        assert_eq!(json, r#"[{"name":"A \"q\"","kind":"module","start":0,"end":8,"children":[]}]"#);
        assert!(headings("#hashtag\n    # indented code\n").is_empty());
    }

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    #[test]
    fn highlights_blocks() {
        let text = "# Title\n## Sub\n###\n\nSome text.\n\n- item\n1. first\n> quote\n---\n```rust\nlet x = 1;\n```\n    indented\n";
        assert_eq!(
            kinds(text),
            [
                ("#", Kind::Keyword),
                ("Title", Kind::Heading),
                ("##", Kind::Keyword),
                ("Sub", Kind::Heading),
                ("###", Kind::Keyword),
                ("-", Kind::Keyword),
                ("1.", Kind::Keyword),
                (">", Kind::Punctuation),
                ("---", Kind::Punctuation),
                ("```rust", Kind::Code),
                ("let x = 1;", Kind::Code),
                ("```", Kind::Code),
                ("indented", Kind::Code),
            ]
        );
    }

    #[test]
    fn highlights_inline_markup() {
        let text = "Use `cargo` with **care**, *style* and [docs](https://example.org) <!-- note --> | a\n";
        assert_eq!(
            kinds(text),
            [
                ("`cargo`", Kind::Code),
                ("**care**", Kind::Strong),
                ("*style*", Kind::Emphasis),
                ("[docs]", Kind::Link),
                ("(https://example.org)", Kind::String),
                ("<!-- note -->", Kind::Comment),
                ("|", Kind::Punctuation),
            ]
        );
    }

    #[test]
    fn leaves_snake_case_and_lone_markers_alone() {
        assert!(kinds("a snake_case_name and 2 * 3 and a ` tick\n").is_empty());
    }

    #[test]
    fn survives_any_input() {
        for text in
            ["", "#", "```", "**", "[a](", "<!--", "\\", "- [ ]", "é*è*", "\r\n\r\n", "`` `a` ``", "####### seven"]
        {
            let spans = highlight(text);
            let mut previous = 0;
            for span in spans {
                assert!(
                    span.start >= previous
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
        let bytes = encode(&[Span { start: 1, end: 3, kind: Kind::Code }]);
        assert_eq!(bytes, [1, 0, 0, 0, 3, 0, 0, 0, 16, 0, 0, 0]);
    }
}
