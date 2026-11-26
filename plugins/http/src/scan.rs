const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "PATCH", "OPTIONS", "TRACE"];

const MAX_NAME: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
    DocComment = 4,
    Macro = 5,
    Attribute = 9,
    Constant = 10,
    Punctuation = 11,
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
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Before,
    Url,
    Headers,
    Body,
    Handler,
}

struct Line<'a> {
    start: usize,
    text: &'a str,
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut found = Vec::new();
    let mut start = 0;
    for piece in text.split('\n') {
        found.push(Line { start, text: piece.strip_suffix('\r').unwrap_or(piece) });
        start += piece.len() + 1;
    }
    found
}

fn is_comment(trimmed: &str) -> bool {
    trimmed.starts_with('#') || trimmed.starts_with("//")
}

fn variable_name(trimmed: &str) -> Option<usize> {
    let rest = trimmed.strip_prefix('@')?;
    let equals = rest.find('=')?;
    let name = rest[..equals].trim();
    let valid = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'));
    valid.then_some(equals + 1)
}

pub fn highlight(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut state = State::Before;
    for line in lines(text) {
        let indent = line.text.len() - line.text.trim_start().len();
        let at = line.start + indent;
        let trimmed = line.text.trim();
        if trimmed.starts_with("###") {
            push(&mut spans, at, at + trimmed.len(), Kind::Comment);
            state = State::Before;
            continue;
        }
        if state == State::Handler {
            push(&mut spans, at, at + trimmed.len(), Kind::Macro);
            if trimmed.contains("%}") {
                state = State::Body;
            }
            continue;
        }
        if state == State::Url && !(indent > 0 && !trimmed.is_empty()) {
            state = State::Headers;
        }
        match state {
            State::Before => {
                if trimmed.is_empty() {
                    continue;
                }
                if is_comment(trimmed) {
                    let body = trimmed.trim_start_matches(['#', '/']).trim_start();
                    let kind = if body.starts_with("@name") || body.starts_with("@no-redirect") {
                        Kind::DocComment
                    } else {
                        Kind::Comment
                    };
                    push(&mut spans, at, at + trimmed.len(), kind);
                } else if let Some(equals) = variable_name(trimmed) {
                    push(&mut spans, at, at + trimmed[..equals].trim_end().len(), Kind::Attribute);
                    push(&mut spans, at + equals, at + equals + 1, Kind::Punctuation);
                    values(&mut spans, &trimmed[equals + 1..], at + equals + 1, Some(Kind::String));
                } else {
                    request_line(&mut spans, trimmed, at);
                    state = State::Url;
                }
            }
            State::Url => values(&mut spans, trimmed, at, None),
            State::Headers => {
                if trimmed.is_empty() {
                    state = State::Body;
                } else if is_comment(trimmed) {
                    push(&mut spans, at, at + trimmed.len(), Kind::Comment);
                } else if let Some(colon) = header_colon(trimmed) {
                    push(&mut spans, at, at + colon, Kind::Attribute);
                    push(&mut spans, at + colon, at + colon + 1, Kind::Punctuation);
                    values(&mut spans, &trimmed[colon + 1..], at + colon + 1, Some(Kind::String));
                } else {
                    state = body_line(&mut spans, trimmed, at);
                }
            }
            State::Body => state = body_line(&mut spans, trimmed, at),
            State::Handler => {}
        }
    }
    spans
}

fn request_line(spans: &mut Vec<Span>, trimmed: &str, at: usize) {
    let mut rest = trimmed;
    let mut offset = at;
    if let Some(word) = trimmed.split_whitespace().next()
        && METHODS.contains(&word)
    {
        push(spans, at, at + word.len(), Kind::Keyword);
        rest = &trimmed[word.len()..];
        offset = at + word.len();
    }
    let version = rest.rfind(" HTTP/").map(|space| space + 1);
    let url_end = version.unwrap_or(rest.len());
    values(spans, &rest[..url_end], offset, None);
    if let Some(version) = version {
        push(spans, offset + version, offset + rest.len(), Kind::Keyword);
    }
}

fn header_colon(trimmed: &str) -> Option<usize> {
    let colon = trimmed.find(':')?;
    let name = trimmed[..colon].trim_end();
    let valid =
        !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte));
    valid.then_some(colon)
}

fn body_line(spans: &mut Vec<Span>, trimmed: &str, at: usize) -> State {
    if trimmed.starts_with("> {%") || trimmed.starts_with(">{%") {
        push(spans, at, at + trimmed.len(), Kind::Macro);
        let opened = trimmed.find("{%").map_or(0, |open| open + 2);
        return if trimmed[opened..].contains("%}") { State::Body } else { State::Handler };
    }
    for marker in [">>!", ">>", "<>", ">", "<"] {
        if let Some(rest) = trimmed.strip_prefix(marker)
            && rest.starts_with(' ')
        {
            let kind = if marker == "<" { Kind::Keyword } else { Kind::Macro };
            push(spans, at, at + marker.len(), kind);
            values(spans, rest, at + marker.len(), Some(Kind::String));
            return State::Body;
        }
    }
    json(spans, trimmed, at);
    State::Body
}

fn values(spans: &mut Vec<Span>, text: &str, at: usize, kind: Option<Kind>) {
    let mut rest = 0;
    while let Some(open) = text[rest..].find("{{").map(|open| rest + open) {
        let Some(close) = text[open + 2..].find("}}").map(|close| open + 2 + close + 2) else { break };
        if let Some(kind) = kind {
            trimmed_push(spans, text, rest, open, at, kind);
        }
        push(spans, at + open, at + close, Kind::Macro);
        rest = close;
    }
    if let Some(kind) = kind {
        trimmed_push(spans, text, rest, text.len(), at, kind);
    }
}

fn trimmed_push(spans: &mut Vec<Span>, text: &str, from: usize, to: usize, at: usize, kind: Kind) {
    let piece = &text[from..to];
    let lead = piece.len() - piece.trim_start().len();
    let kept = piece.trim();
    if !kept.is_empty() {
        push(spans, at + from + lead, at + from + lead + kept.len(), kind);
    }
}

fn json(spans: &mut Vec<Span>, text: &str, at: usize) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if text[index..].starts_with("{{")
            && let Some(close) = text[index + 2..].find("}}")
        {
            let end = index + 2 + close + 2;
            push(spans, at + index, at + end, Kind::Macro);
            index = end;
        } else if byte == b'"' {
            let mut end = index + 1;
            let mut from = index;
            while end < bytes.len() && bytes[end] != b'"' {
                if bytes[end] == b'\\' {
                    end += 1;
                } else if text[end..].starts_with("{{")
                    && let Some(close) = text[end + 2..].find("}}")
                {
                    push(spans, at + from, at + end, Kind::String);
                    let stop = end + 2 + close + 2;
                    push(spans, at + end, at + stop.min(bytes.len()), Kind::Macro);
                    end = stop;
                    from = stop;
                    continue;
                }
                end += 1;
            }
            let end = (end + 1).min(bytes.len());
            push(spans, at + from, at + end, Kind::String);
            index = end;
        } else if byte.is_ascii_digit() || (byte == b'-' && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)) {
            let end = index
                + 1
                + bytes[index + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
                    .count();
            push(spans, at + index, at + end, Kind::Number);
            index = end;
        } else if byte.is_ascii_alphabetic() {
            let end = index + bytes[index..].iter().take_while(|b| b.is_ascii_alphanumeric()).count();
            if matches!(&text[index..end], "true" | "false" | "null") {
                push(spans, at + index, at + end, Kind::Constant);
            }
            index = end;
        } else {
            if matches!(byte, b'{' | b'}' | b'[' | b']' | b':' | b',') {
                push(spans, at + index, at + index + 1, Kind::Punctuation);
            }
            index += text[index..].chars().next().map_or(1, char::len_utf8);
        }
    }
}

fn push(spans: &mut Vec<Span>, start: usize, end: usize, kind: Kind) {
    if end > start && spans.last().is_none_or(|last| last.end <= start) {
        spans.push(Span { start, end, kind });
    }
}

struct Block {
    request: Option<usize>,
    last: usize,
    name: Option<String>,
}

fn blocks(lines: &[Line<'_>]) -> Vec<Block> {
    let mut found = Vec::new();
    let mut current = Block { request: None, last: 0, name: None };
    for (number, line) in lines.iter().enumerate() {
        let trimmed = line.text.trim();
        if let Some(title) = trimmed.strip_prefix("###") {
            found.push(std::mem::replace(
                &mut current,
                Block { request: None, last: number, name: Some(title.trim().to_owned()).filter(|t| !t.is_empty()) },
            ));
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if current.request.is_none() {
            if is_comment(trimmed) {
                let body = trimmed.trim_start_matches(['#', '/']).trim_start();
                if let Some(name) =
                    body.strip_prefix("@name").filter(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '=']))
                {
                    let name = name.trim_start_matches([' ', '\t', '=']).trim();
                    if !name.is_empty() {
                        current.name = Some(name.to_owned());
                    }
                }
                continue;
            }
            if variable_name(trimmed).is_some() {
                continue;
            }
            current.request = Some(number);
        }
        current.last = number;
    }
    found.push(current);
    found.retain(|block| block.request.is_some());
    found
}

pub fn fold(text: &str) -> Vec<Fold> {
    blocks(&lines(text))
        .iter()
        .filter_map(|block| {
            let start = block.request?;
            (block.last > start).then_some(Fold { start_line: start as u32, end_line: block.last as u32 })
        })
        .collect()
}

pub fn outline(text: &str) -> Vec<Symbol> {
    let lines = lines(text);
    blocks(&lines)
        .into_iter()
        .filter_map(|block| {
            let request = &lines[block.request?];
            let last = &lines[block.last];
            let name = block.name.unwrap_or_else(|| request.text.trim().to_owned());
            let name = match name.char_indices().nth(MAX_NAME) {
                Some((cut, _)) => name[..cut].to_owned(),
                None => name,
            };
            let start = request.start + (request.text.len() - request.text.trim_start().len());
            Some(Symbol { name, kind: "method", start, end: last.start + last.text.len() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text).iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    #[test]
    fn highlights_a_request() {
        let text = "### Items\n# @name list\n@host = {{base}}:8080\nGET http://{{host}}/items HTTP/1.1\nAccept: application/json\n// note\n\n{\"a\": [1, -2.5e3], \"b\": \"x{{id}}y\", \"c\": true}\n< ./payload.json\n> {%\n  client.log(1);\n%}\n>> out.json\n";
        assert_eq!(
            kinds(text),
            [
                ("### Items", Kind::Comment),
                ("# @name list", Kind::DocComment),
                ("@host", Kind::Attribute),
                ("=", Kind::Punctuation),
                ("{{base}}", Kind::Macro),
                (":8080", Kind::String),
                ("GET", Kind::Keyword),
                ("{{host}}", Kind::Macro),
                ("HTTP/1.1", Kind::Keyword),
                ("Accept", Kind::Attribute),
                (":", Kind::Punctuation),
                ("application/json", Kind::String),
                ("// note", Kind::Comment),
                ("{", Kind::Punctuation),
                ("\"a\"", Kind::String),
                (":", Kind::Punctuation),
                ("[", Kind::Punctuation),
                ("1", Kind::Number),
                (",", Kind::Punctuation),
                ("-2.5e3", Kind::Number),
                ("]", Kind::Punctuation),
                (",", Kind::Punctuation),
                ("\"b\"", Kind::String),
                (":", Kind::Punctuation),
                ("\"x", Kind::String),
                ("{{id}}", Kind::Macro),
                ("y\"", Kind::String),
                (",", Kind::Punctuation),
                ("\"c\"", Kind::String),
                (":", Kind::Punctuation),
                ("true", Kind::Constant),
                ("}", Kind::Punctuation),
                ("<", Kind::Keyword),
                ("./payload.json", Kind::String),
                ("> {%", Kind::Macro),
                ("client.log(1);", Kind::Macro),
                ("%}", Kind::Macro),
                (">>", Kind::Macro),
                ("out.json", Kind::String),
            ]
        );
    }

    #[test]
    fn variable_definitions_are_split_at_the_equals_sign() {
        let spans = highlight("@token = abc\n");
        assert_eq!(spans[0], Span { start: 0, end: 6, kind: Kind::Attribute });
        assert_eq!(spans[1], Span { start: 7, end: 8, kind: Kind::Punctuation });
        assert_eq!(spans[2], Span { start: 9, end: 12, kind: Kind::String });
    }

    #[test]
    fn spans_are_ordered_on_any_text() {
        for text in ["", "###", "GET", "{{", "\"{{a}", "POST x\n\n\"\\", "é {{é}} \"é", "> {% %}\n> {%\n", "@=\n@a=\n"]
        {
            let spans = highlight(text);
            let mut end = 0;
            for span in &spans {
                assert!(span.start >= end && span.end > span.start && span.end <= text.len(), "{text:?}: {spans:?}");
                assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
                end = span.end;
            }
        }
    }

    #[test]
    fn folds_and_outlines_each_request() {
        let text = "### First\nGET http://h/a\nAccept: */*\n\n###\n# @name second\nPOST http://h/b\n\n{}\n\n### only a comment\n# nothing\n###\nhttp://h/c\n";
        assert_eq!(
            fold(text),
            [Fold { start_line: 1, end_line: 2 }, Fold { start_line: 6, end_line: 8 }],
            "a one-line request does not fold"
        );
        let symbols = outline(text);
        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["First", "second", "http://h/c"]);
        assert_eq!(&text[symbols[1].start..symbols[1].end], "POST http://h/b\n\n{}");
        assert!(outline("").is_empty());
    }
}
