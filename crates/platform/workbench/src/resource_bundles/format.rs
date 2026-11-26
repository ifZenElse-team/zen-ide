use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    pub key_range: Range<usize>,
    pub value_range: Range<usize>,
    pub line_range: Range<usize>,
}

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
}

impl Reader<'_> {
    fn skip_newline(&mut self) {
        match self.bytes.get(self.at) {
            Some(b'\r') => {
                self.at += 1;
                if self.bytes.get(self.at) == Some(&b'\n') {
                    self.at += 1;
                }
            }
            Some(b'\n') => self.at += 1,
            _ => {}
        }
    }

    fn skip_blank(&mut self) {
        while self.bytes.get(self.at).is_some_and(|&byte| is_blank(byte)) {
            self.at += 1;
        }
    }

    fn continues(&self) -> bool {
        self.bytes.get(self.at) == Some(&b'\\') && self.bytes.get(self.at + 1).is_none_or(|&byte| is_newline(byte))
    }

    fn continuation(&mut self) {
        self.at += 1;
        self.skip_newline();
        self.skip_blank();
    }

    fn next_char(&mut self) -> char {
        let c = self.text[self.at..].chars().next().unwrap_or(' ');
        self.at += c.len_utf8();
        c
    }

    fn escape(&mut self, out: &mut String) {
        self.at += 1;
        if self.at >= self.bytes.len() {
            return;
        }
        match self.next_char() {
            'u' => {
                let hex = self.text.get(self.at..self.at + 4).filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()));
                match hex.and_then(|hex| u32::from_str_radix(hex, 16).ok()) {
                    Some(unit) => {
                        self.at += 4;
                        out.push(char::from_u32(unit).unwrap_or('\u{fffd}'));
                    }
                    None => out.push('u'),
                }
            }
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            'f' => out.push('\x0c'),
            other => out.push(other),
        }
    }

    fn entry(&mut self, line_start: usize) -> Entry {
        let key_start = self.at;
        let mut key = String::new();
        let mut key_end = None;
        let mut separated = false;
        loop {
            if self.continues() {
                self.continuation();
                continue;
            }
            let Some(&byte) = self.bytes.get(self.at) else { break };
            match byte {
                b'\n' | b'\r' => break,
                b'\\' => self.escape(&mut key),
                b'=' | b':' => {
                    key_end = Some(self.at);
                    self.at += 1;
                    separated = true;
                    break;
                }
                _ if is_blank(byte) => break,
                _ => key.push(self.next_char()),
            }
        }
        let key_end = key_end.unwrap_or(self.at);
        loop {
            if self.continues() {
                self.continuation();
                continue;
            }
            match self.bytes.get(self.at) {
                Some(&byte) if is_blank(byte) => self.at += 1,
                Some(b'=' | b':') if !separated => {
                    self.at += 1;
                    separated = true;
                }
                _ => break,
            }
        }
        let value_start = self.at;
        let mut value = String::new();
        loop {
            if self.continues() {
                self.continuation();
                continue;
            }
            match self.bytes.get(self.at) {
                None | Some(b'\n' | b'\r') => break,
                Some(b'\\') => self.escape(&mut value),
                Some(_) => value.push(self.next_char()),
            }
        }
        let value_end = self.at;
        self.skip_newline();
        Entry {
            key,
            value,
            key_range: key_start..key_end,
            value_range: value_start..value_end,
            line_range: line_start..self.at,
        }
    }
}

pub fn read(text: &str) -> Vec<Entry> {
    let mut reader = Reader { text, bytes: text.as_bytes(), at: 0 };
    if text.starts_with('\u{feff}') {
        reader.at = 3;
    }
    let mut entries = Vec::new();
    while reader.at < reader.bytes.len() {
        let line_start = reader.at;
        reader.skip_blank();
        match reader.bytes.get(reader.at) {
            None => break,
            Some(b'\n' | b'\r') => reader.skip_newline(),
            Some(b'#' | b'!') => {
                while reader.bytes.get(reader.at).is_some_and(|&byte| !is_newline(byte)) {
                    reader.at += 1;
                }
                reader.skip_newline();
            }
            Some(_) => entries.push(reader.entry(line_start)),
        }
    }
    entries
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
            ' ' | '\t' if index == 0 => {
                out.push('\\');
                escape_char(&mut out, c);
            }
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

fn separator_of(text: &str) -> &'static str {
    let entries = read(text);
    let spaced = entries.iter().filter(|entry| text[entry.key_range.end..].starts_with(" = ")).count();
    if spaced * 2 > entries.len() { " = " } else { "=" }
}

fn newline_of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

pub fn with_value(text: &str, key: &str, value: &str) -> String {
    let entries = read(text);
    match entries.iter().rfind(|entry| entry.key == key) {
        Some(entry) => {
            let mut out = text.to_owned();
            let range = entry.value_range.clone();
            if range.start == range.end && entry.key_range.end == range.start {
                out.replace_range(range, &format!("{}{}", separator_of(text), escape_value(value)));
            } else {
                out.replace_range(range, &escape_value(value));
            }
            out
        }
        None => appended(text, key, value),
    }
}

pub fn appended(text: &str, key: &str, value: &str) -> String {
    let newline = newline_of(text);
    let mut out = text.to_owned();
    if !out.is_empty() && !out.ends_with('\n') && !out.ends_with('\r') {
        out.push_str(newline);
    }
    out.push_str(&escape_key(key));
    out.push_str(separator_of(text));
    out.push_str(&escape_value(value));
    out.push_str(newline);
    out
}

pub fn renamed(text: &str, key: &str, new_key: &str) -> String {
    let mut out = text.to_owned();
    for entry in read(text).iter().rev().filter(|entry| entry.key == key) {
        out.replace_range(entry.key_range.clone(), &escape_key(new_key));
    }
    out
}

pub fn without(text: &str, key: &str) -> String {
    let mut out = text.to_owned();
    for entry in read(text).iter().rev().filter(|entry| entry.key == key) {
        out.replace_range(entry.line_range.clone(), "");
    }
    out
}
