#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    String = 1,
    Comment = 3,
    Constant = 10,
    Punctuation = 11,
    Property = 17,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub key_start: usize,
    pub key_end: usize,
    pub value: String,
    pub value_start: usize,
    pub end: usize,
    pub first_line: u32,
    pub last_line: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inlay {
    pub offset: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completions {
    pub from: usize,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub entries: Vec<Entry>,
    pub spans: Vec<Span>,
    pub comments: Vec<(u32, u32)>,
    pub malformed: Vec<(usize, usize)>,
}

const MAX_ITEMS: usize = 2_000;
const INLAY_CHARS: usize = 80;

fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\x0c')
}

fn is_newline(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r')
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    line: u32,
    doc: Document,
}

impl Reader<'_> {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if end > start {
            self.doc.spans.push(Span { start, end, kind });
        }
    }

    fn skip_newline(&mut self) {
        if self.bytes.get(self.at) == Some(&b'\r') {
            self.at += 1;
            if self.bytes.get(self.at) == Some(&b'\n') {
                self.at += 1;
            }
        } else if self.bytes.get(self.at) == Some(&b'\n') {
            self.at += 1;
        }
        self.line += 1;
    }

    fn skip_blank(&mut self) {
        while self.bytes.get(self.at).is_some_and(|&byte| is_blank(byte)) {
            self.at += 1;
        }
    }

    fn at_end_of_line(&self) -> bool {
        self.bytes.get(self.at).is_none_or(|&byte| is_newline(byte))
    }

    fn continues(&self) -> bool {
        self.bytes.get(self.at) == Some(&b'\\') && self.bytes.get(self.at + 1).is_none_or(|&byte| is_newline(byte))
    }

    fn take_continuation(&mut self, run_start: &mut usize, kind: Kind) {
        self.push(*run_start, self.at, kind);
        self.push(self.at, self.at + 1, Kind::Punctuation);
        self.at += 1;
        if self.at < self.bytes.len() {
            self.skip_newline();
        }
        self.skip_blank();
        *run_start = self.at;
    }

    fn escape(&mut self, out: &mut String, run_start: &mut usize, kind: Kind) {
        self.push(*run_start, self.at, kind);
        let start = self.at;
        self.at += 1;
        let Some(c) = self.text[self.at..].chars().next() else { return };
        if c == 'u' {
            let digits = self.bytes.get(self.at + 1..self.at + 5).filter(|hex| hex.iter().all(u8::is_ascii_hexdigit));
            match digits.and_then(|hex| u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()) {
                Some(unit) => {
                    self.at += 5;
                    out.push(char::from_u32(unit).unwrap_or('\u{fffd}'));
                    self.push(start, self.at, Kind::Constant);
                }
                None => {
                    self.at += 1;
                    let mut end = self.at;
                    while end < self.at + 4 && self.bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
                        end += 1;
                    }
                    self.at = end;
                    self.doc.malformed.push((start, end));
                    self.push(start, end, kind);
                }
            }
        } else {
            self.at += c.len_utf8();
            out.push(match c {
                't' => '\t',
                'n' => '\n',
                'r' => '\r',
                'f' => '\x0c',
                other => other,
            });
            self.push(start, self.at, Kind::Constant);
        }
        *run_start = self.at;
    }

    fn comment(&mut self) {
        let start = self.at;
        while !self.at_end_of_line() {
            self.at += 1;
        }
        self.push(start, self.at, Kind::Comment);
        match self.doc.comments.last_mut() {
            Some((_, last)) if *last + 1 == self.line => *last = self.line,
            _ => self.doc.comments.push((self.line, self.line)),
        }
    }

    fn entry(&mut self) {
        let first_line = self.line;
        let key_start = self.at;
        let mut key = String::new();
        let mut run = self.at;
        let mut key_end = self.at;
        let mut separated = false;
        loop {
            if self.continues() {
                self.take_continuation(&mut run, Kind::Property);
                continue;
            }
            let Some(&byte) = self.bytes.get(self.at) else { break };
            match byte {
                b'\n' | b'\r' => break,
                b'\\' => self.escape(&mut key, &mut run, Kind::Property),
                b'=' | b':' => {
                    self.push(run, self.at, Kind::Property);
                    key_end = self.at;
                    self.push(self.at, self.at + 1, Kind::Punctuation);
                    self.at += 1;
                    separated = true;
                    break;
                }
                _ if is_blank(byte) => break,
                _ => {
                    let c = self.text[self.at..].chars().next().unwrap_or(' ');
                    key.push(c);
                    self.at += c.len_utf8();
                }
            }
        }
        if !separated {
            self.push(run, self.at, Kind::Property);
            key_end = self.at;
        }
        let mut value = String::new();
        let mut value_start = None;
        loop {
            if self.continues() {
                let mut ignored = self.at;
                self.take_continuation(&mut ignored, Kind::String);
                continue;
            }
            match self.bytes.get(self.at) {
                Some(&byte) if is_blank(byte) => self.at += 1,
                Some(b'=' | b':') if !separated && value_start.is_none() => {
                    self.push(self.at, self.at + 1, Kind::Punctuation);
                    self.at += 1;
                    separated = true;
                }
                _ => break,
            }
        }
        let mut run = self.at;
        loop {
            if self.continues() {
                value_start.get_or_insert(self.at);
                self.take_continuation(&mut run, Kind::String);
                continue;
            }
            let Some(&byte) = self.bytes.get(self.at) else { break };
            if is_newline(byte) {
                break;
            }
            value_start.get_or_insert(self.at);
            if byte == b'\\' {
                self.escape(&mut value, &mut run, Kind::String);
            } else {
                let c = self.text[self.at..].chars().next().unwrap_or(' ');
                value.push(c);
                self.at += c.len_utf8();
            }
        }
        self.push(run, self.at, Kind::String);
        let end = self.at;
        self.doc.entries.push(Entry {
            key,
            key_start,
            key_end,
            value,
            value_start: value_start.unwrap_or(end),
            end,
            first_line,
            last_line: self.line,
        });
    }
}

pub fn parse(text: &str) -> Document {
    let mut reader = Reader { text, bytes: text.as_bytes(), at: 0, line: 0, doc: Document::default() };
    if text.starts_with('\u{feff}') {
        reader.at = 3;
    }
    while reader.at < reader.bytes.len() {
        reader.skip_blank();
        match reader.bytes.get(reader.at) {
            None => break,
            Some(b'\n' | b'\r') => {}
            Some(b'#' | b'!') => reader.comment(),
            Some(_) => reader.entry(),
        }
        if reader.at < reader.bytes.len() {
            reader.skip_newline();
        }
    }
    reader.doc
}

pub fn highlight(text: &str) -> Vec<Span> {
    let mut spans = parse(text).spans;
    spans.sort_by_key(|span| span.start);
    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if last.end > span.start => {}
            Some(last) if last.end == span.start && last.kind == span.kind => last.end = span.end,
            _ => merged.push(span),
        }
    }
    merged
}

pub fn fold(text: &str) -> Vec<Fold> {
    let doc = parse(text);
    let mut folds: Vec<Fold> = doc
        .comments
        .iter()
        .filter(|(first, last)| last > first)
        .map(|&(start_line, end_line)| Fold { start_line, end_line })
        .chain(
            doc.entries
                .iter()
                .filter(|entry| entry.last_line > entry.first_line)
                .map(|entry| Fold { start_line: entry.first_line, end_line: entry.last_line }),
        )
        .collect();
    folds.sort_by_key(|fold| fold.start_line);
    folds
}

pub fn entries(text: &str) -> Vec<Entry> {
    parse(text).entries
}

pub fn problems(text: &str) -> Vec<Problem> {
    let doc = parse(text);
    let mut problems: Vec<Problem> = doc
        .malformed
        .iter()
        .map(|&(start, end)| Problem {
            start,
            end,
            severity: Severity::Error,
            message: "Malformed \\uXXXX escape: four hexadecimal digits must follow \\u".to_owned(),
        })
        .collect();
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for entry in &doc.entries {
        *counts.entry(entry.key.as_str()).or_default() += 1;
    }
    for entry in doc.entries.iter().filter(|entry| counts[entry.key.as_str()] > 1) {
        problems.push(Problem {
            start: entry.key_start,
            end: entry.key_end.max(entry.key_start + 1).min(text.len()),
            severity: Severity::Warning,
            message: format!("Duplicate key '{}'", entry.key),
        });
    }
    problems.retain(|problem| problem.end > problem.start);
    problems.sort_by_key(|problem| problem.start);
    problems
}

pub fn shown_value(value: &str) -> String {
    let mut shown = String::new();
    for c in value.chars() {
        if shown.chars().count() >= INLAY_CHARS {
            shown.push('…');
            break;
        }
        match c {
            '\n' => shown.push('⏎'),
            '\t' => shown.push('⇥'),
            '\r' | '\x0c' => shown.push(' '),
            c => shown.push(c),
        }
    }
    shown
}

pub fn inlays(text: &str) -> Vec<Inlay> {
    parse(text)
        .entries
        .into_iter()
        .filter(|entry| entry.value_start < entry.end && text[entry.value_start..entry.end] != entry.value)
        .map(|entry| Inlay { offset: entry.end, text: format!("= {}", shown_value(&entry.value)) })
        .collect()
}

pub fn key_prefix_at(text: &str, caret: usize) -> Option<usize> {
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let line_start = text[..caret].rfind(['\n', '\r']).map_or(0, |at| at + 1);
    if line_start > 0 && text[..line_start - 1].ends_with('\\') {
        let doc = parse(text);
        if doc.entries.iter().any(|entry| entry.key_end < line_start && line_start <= entry.end) {
            return None;
        }
    }
    let line = &text[line_start..caret];
    let typed = line.trim_start_matches([' ', '\t', '\x0c']);
    if typed.starts_with(['#', '!']) || typed.contains(['=', ':', ' ', '\t']) {
        return None;
    }
    Some(caret - typed.len())
}

pub fn complete_keys(text: &str, caret: usize, keys: &[(&str, &str)]) -> Option<Completions> {
    let from = key_prefix_at(text, caret)?;
    let typed = &text[from..caret.min(text.len())];
    let present: std::collections::HashSet<String> = entries(text).into_iter().map(|entry| entry.key).collect();
    let items: Vec<Item> = keys
        .iter()
        .filter(|(key, _)| key.starts_with(typed) && !present.contains(*key))
        .take(MAX_ITEMS)
        .map(|(key, detail)| Item { label: (*key).to_owned(), detail: (*detail).to_owned() })
        .collect();
    (!items.is_empty()).then_some(Completions { from, items })
}

pub fn escape_key(key: &str) -> String {
    let mut out = String::new();
    for (index, c) in key.chars().enumerate() {
        match c {
            ' ' | '=' | ':' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '#' | '!' if index == 0 => {
                out.push('\\');
                out.push(c);
            }
            _ => escape_char(&mut out, c),
        }
    }
    out
}

pub fn escape_value(value: &str) -> String {
    let mut out = String::new();
    for (index, c) in value.chars().enumerate() {
        match c {
            ' ' if index == 0 => out.push_str("\\ "),
            '\\' => out.push_str("\\\\"),
            _ => escape_char(&mut out, c),
        }
    }
    out
}

fn escape_char(out: &mut String, c: char) {
    match c {
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\x0c' => out.push_str("\\f"),
        c => out.push(c),
    }
}
