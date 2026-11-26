#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
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

const CONSTANTS: &[&str] = &["true", "false", "yes", "no", "on", "off", "null"];

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    spans: Vec<Span>,
    line_start: usize,
    flow: usize,
    key_allowed: bool,
    node_column: Option<usize>,
    block: Option<usize>,
}

pub fn highlight(text: &str) -> Vec<Span> {
    let mut scanner = Scanner {
        text,
        bytes: text.as_bytes(),
        spans: Vec::new(),
        line_start: 0,
        flow: 0,
        key_allowed: true,
        node_column: None,
        block: None,
    };
    scanner.run();
    scanner.spans
}

impl Scanner<'_> {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if end > start {
            self.spans.push(Span { start, end, kind });
        }
    }

    fn line_end(&self, at: usize) -> usize {
        let end = self.text[at..].find('\n').map_or(self.text.len(), |newline| at + newline);
        if end > at && self.bytes[end - 1] == b'\r' { end - 1 } else { end }
    }

    fn ends_token(&self, at: usize) -> bool {
        match self.bytes.get(at) {
            None | Some(b' ' | b'\t' | b'\r' | b'\n') => true,
            Some(b',' | b'[' | b']' | b'{' | b'}') => self.flow > 0,
            Some(_) => false,
        }
    }

    fn run(&mut self) {
        let mut at = 0;
        while at < self.bytes.len() {
            let c = self.bytes[at];
            if c == b'\n' {
                at += 1;
                self.line_start = at;
                if let Some(parent) = self.block.take() {
                    at = self.block_body(at, parent);
                    self.line_start = at;
                }
                if self.flow == 0 {
                    self.key_allowed = true;
                }
                self.node_column = None;
                continue;
            }
            if at == self.line_start {
                let rest = &self.text[at..];
                if (rest.starts_with("---") || rest.starts_with("..."))
                    && matches!(self.bytes.get(at + 3), None | Some(b' ' | b'\t' | b'\r' | b'\n'))
                {
                    self.push(at, at + 3, Kind::Punctuation);
                    self.flow = 0;
                    self.key_allowed = true;
                    at += 3;
                    continue;
                }
                if c == b'%' && self.flow == 0 {
                    let end = self.line_end(at);
                    self.push(at, end, Kind::Attribute);
                    at = end;
                    continue;
                }
            }
            if matches!(c, b' ' | b'\t' | b'\r') {
                at += 1;
                continue;
            }
            at = self.token(at);
        }
    }

    fn token(&mut self, at: usize) -> usize {
        let c = self.bytes[at];
        let column = at - self.line_start;
        if c == b'#' && (at == 0 || matches!(self.bytes[at - 1], b' ' | b'\t' | b'\r' | b'\n')) {
            let end = self.line_end(at);
            self.push(at, end, Kind::Comment);
            return end;
        }
        if (c == b'-' || c == b'?') && self.flow == 0 && self.ends_token(at + 1) {
            self.push(at, at + 1, Kind::Punctuation);
            self.key_allowed = true;
            self.node_column = Some(column);
            return at + 1;
        }
        if c == b':' && self.ends_token(at + 1) {
            self.push(at, at + 1, Kind::Punctuation);
            self.key_allowed = false;
            return at + 1;
        }
        match c {
            b'[' | b'{' => {
                self.push(at, at + 1, Kind::Punctuation);
                self.flow += 1;
                self.key_allowed = true;
                return at + 1;
            }
            b']' | b'}' if self.flow > 0 => {
                self.push(at, at + 1, Kind::Punctuation);
                self.flow -= 1;
                self.key_allowed = false;
                return at + 1;
            }
            b',' if self.flow > 0 => {
                self.push(at, at + 1, Kind::Punctuation);
                self.key_allowed = true;
                return at + 1;
            }
            b'&' | b'*' | b'!' => {
                let mut end = at + 1;
                while !self.ends_token(end) {
                    end += 1;
                }
                self.push(at, end, Kind::Attribute);
                return end;
            }
            b'|' | b'>' if self.flow == 0 => {
                let indicators =
                    self.bytes[at + 1..].iter().take(2).take_while(|b| matches!(b, b'-' | b'+' | b'1'..=b'9')).count();
                let end = at + 1 + indicators;
                if self.ends_token(end) {
                    self.push(at, end, Kind::Punctuation);
                    let indent = self.bytes[self.line_start..].iter().take_while(|b| **b == b' ').count();
                    self.block = Some(self.node_column.unwrap_or(indent));
                    self.key_allowed = false;
                    return end;
                }
            }
            b'"' | b'\'' => return self.quoted(at),
            _ => {}
        }
        self.plain(at)
    }

    fn quoted(&mut self, at: usize) -> usize {
        let quote = self.bytes[at];
        let mut index = at + 1;
        let end = loop {
            match self.bytes.get(index) {
                None => break self.text.len(),
                Some(b'\\') if quote == b'"' => index += 2,
                Some(b'\'') if quote == b'\'' && self.bytes.get(index + 1) == Some(&b'\'') => index += 2,
                Some(byte) if *byte == quote => break index + 1,
                Some(_) => index += 1,
            }
        };
        let end = end.min(self.text.len());
        if let Some(newline) = self.text[at..end].rfind('\n') {
            self.line_start = at + newline + 1;
        }
        let mut colon = end;
        while matches!(self.bytes.get(colon), Some(b' ' | b'\t')) {
            colon += 1;
        }
        if (self.key_allowed || self.flow > 0) && self.bytes.get(colon) == Some(&b':') && self.ends_token(colon + 1) {
            self.push(at, end, Kind::Keyword);
            self.push(colon, colon + 1, Kind::Punctuation);
            self.key_allowed = false;
            self.node_column = Some(at - self.line_start.min(at));
            return colon + 1;
        }
        self.interpolated(at, end, Some(Kind::String));
        self.key_allowed = false;
        end
    }

    fn plain(&mut self, at: usize) -> usize {
        let mut index = at;
        let mut key = false;
        loop {
            match self.bytes.get(index) {
                None | Some(b'\n') => break,
                Some(b'#') if index > at && matches!(self.bytes[index - 1], b' ' | b'\t') => break,
                Some(b':') if (self.key_allowed || self.flow > 0) && self.ends_token(index + 1) => {
                    key = true;
                    break;
                }
                Some(b',' | b'[' | b']' | b'{' | b'}') if self.flow > 0 => break,
                Some(_) => index += 1,
            }
        }
        let mut end = index;
        while end > at && matches!(self.bytes[end - 1], b' ' | b'\t' | b'\r') {
            end -= 1;
        }
        if end == at {
            return at + self.text[at..].chars().next().map_or(1, char::len_utf8);
        }
        let word = &self.text[at..end];
        if key {
            let kind = if word == "<<" { Kind::Attribute } else { Kind::Keyword };
            self.push(at, end, kind);
            self.push(index, index + 1, Kind::Punctuation);
            self.key_allowed = false;
            self.node_column = Some(at - self.line_start);
            return index + 1;
        }
        self.key_allowed = false;
        if is_number(word) {
            self.push(at, end, Kind::Number);
        } else if word == "~" || CONSTANTS.iter().any(|constant| word.eq_ignore_ascii_case(constant)) {
            self.push(at, end, Kind::Constant);
        } else {
            self.interpolated(at, end, None);
        }
        index
    }

    fn block_body(&mut self, mut at: usize, parent: usize) -> usize {
        while at < self.bytes.len() {
            let end = self.line_end(at);
            let next = self.text[at..].find('\n').map_or(self.text.len(), |newline| at + newline + 1);
            let line = &self.bytes[at..end];
            let indent = line.iter().take_while(|b| **b == b' ').count();
            if line.iter().all(|b| matches!(b, b' ' | b'\t')) {
                at = next;
                continue;
            }
            if indent <= parent {
                break;
            }
            self.interpolated(at + indent, end, Some(Kind::String));
            at = next;
        }
        at
    }

    fn interpolated(&mut self, start: usize, end: usize, kind: Option<Kind>) {
        let mut piece = start;
        let mut at = start;
        while at < end {
            if self.bytes[at] != b'$' {
                at += 1;
                continue;
            }
            let length = match self.bytes.get(at + 1) {
                Some(b'$') => {
                    at += 2;
                    continue;
                }
                Some(b'{') => self.text[at..end].find('}').map(|close| close + 1),
                Some(b) if b.is_ascii_alphabetic() || *b == b'_' => Some(
                    1 + self.bytes[at + 1..end].iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count(),
                ),
                _ => None,
            };
            match length {
                Some(length) if at + length <= end => {
                    if let Some(kind) = kind {
                        self.push(piece, at, kind);
                    }
                    self.push(at, at + length, Kind::Constant);
                    at += length;
                    piece = at;
                }
                _ => at += 1,
            }
        }
        if let Some(kind) = kind {
            self.push(piece, end, kind);
        }
    }
}

fn is_number(word: &str) -> bool {
    let unsigned = word.strip_prefix(['-', '+']).unwrap_or(word);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(word, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    if let Some(hex) = word.strip_prefix("0x") {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit() || b == b'_');
    }
    if let Some(octal) = word.strip_prefix("0o") {
        return !octal.is_empty() && octal.bytes().all(|b| (b'0'..=b'7').contains(&b));
    }
    let bytes = unsigned.as_bytes();
    let digits = |from: usize| bytes[from..].iter().take_while(|b| b.is_ascii_digit() || **b == b'_').count();
    let mut at = digits(0);
    let mut mantissa = at > 0 && bytes[0] != b'_';
    if bytes.get(at) == Some(&b'.') {
        let fraction = digits(at + 1);
        mantissa |= fraction > 0;
        at += 1 + fraction;
    }
    if !mantissa {
        return false;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(at + 1), Some(b'+' | b'-')));
        let exponent = bytes[(at + 1 + sign).min(bytes.len())..].iter().take_while(|b| b.is_ascii_digit()).count();
        if exponent == 0 {
            return false;
        }
        at += 1 + sign + exponent;
    }
    at == bytes.len()
}
