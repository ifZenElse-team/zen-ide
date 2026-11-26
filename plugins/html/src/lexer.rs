#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    Lt,
    LtSlash,
    Gt,
    SlashGt,
    TagName,
    AttrName,
    Eq,
    AttrValue,
    Text,
    Entity,
    Comment,
    Doctype,
    Cdata,
    Pi,
    RawText,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: Tok,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    Data,
    Opaque,
    TagName { lt: usize, closing: bool },
    Tag { lt: usize, name: (usize, usize) },
    AttrName { lt: usize, name: (usize, usize), attr: usize },
    Value { lt: usize, name: (usize, usize), attr: (usize, usize), start: usize, quote: Option<u8> },
}

const RAW_TEXT: &[(&str, bool)] = &[
    ("script", false),
    ("style", false),
    ("textarea", true),
    ("title", true),
    ("xmp", false),
    ("iframe", false),
    ("noembed", false),
    ("noframes", false),
];

pub(crate) fn is_raw_text(name: &str) -> bool {
    RAW_TEXT.iter().any(|(element, _)| name.eq_ignore_ascii_case(element))
}

pub(crate) fn lex(text: &str) -> (Vec<Token>, End) {
    let mut lexer = Lexer { text, bytes: text.as_bytes(), i: 0, tokens: Vec::new(), end: End::Data, xml: false };
    lexer.run();
    (lexer.tokens, lexer.end)
}

#[allow(dead_code, reason = "only the XML plug-in reads XML")]
pub(crate) fn lex_xml(text: &str) -> (Vec<Token>, End) {
    let mut lexer = Lexer { text, bytes: text.as_bytes(), i: 0, tokens: Vec::new(), end: End::Data, xml: true };
    lexer.run();
    (lexer.tokens, lexer.end)
}

pub(crate) fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

pub(crate) fn entity_len(rest: &[u8]) -> Option<usize> {
    if rest.first() != Some(&b'&') {
        return None;
    }
    let (digits, from): (fn(&u8) -> bool, usize) = match (rest.get(1), rest.get(2)) {
        (Some(b'#'), Some(b'x' | b'X')) => (u8::is_ascii_hexdigit, 3),
        (Some(b'#'), _) => (u8::is_ascii_digit, 2),
        _ => (u8::is_ascii_alphanumeric, 1),
    };
    let count = rest[from..].iter().take_while(|byte| digits(byte)).count();
    (count > 0 && rest.get(from + count) == Some(&b';')).then_some(from + count + 1)
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    i: usize,
    tokens: Vec<Token>,
    end: End,
    xml: bool,
}

impl Lexer<'_> {
    fn push(&mut self, kind: Tok, start: usize, end: usize) {
        if end > start {
            self.tokens.push(Token { kind, start, end });
        }
    }

    fn starts_with_ci(&self, at: usize, word: &str) -> bool {
        self.bytes.get(at..at + word.len()).is_some_and(|slice| slice.eq_ignore_ascii_case(word.as_bytes()))
    }

    fn find(&self, from: usize, needle: &str) -> Option<usize> {
        self.text.get(from..)?.find(needle).map(|at| from + at)
    }

    fn run(&mut self) {
        let mut text_start = self.i;
        while self.i < self.bytes.len() {
            let byte = self.bytes[self.i];
            if byte == b'&'
                && let Some(len) = entity_len(&self.bytes[self.i..])
            {
                self.push(Tok::Text, text_start, self.i);
                self.push(Tok::Entity, self.i, self.i + len);
                self.i += len;
                text_start = self.i;
                continue;
            }
            if byte != b'<' {
                self.i += 1;
                continue;
            }
            let at = self.i;
            let next = self.bytes.get(at + 1).copied();
            let markup = match next {
                Some(letter) if letter.is_ascii_alphabetic() || (self.xml && letter == b'_') => true,
                Some(b'/') => {
                    matches!(self.bytes.get(at + 2), Some(letter) if letter.is_ascii_alphabetic() || (self.xml && *letter == b'_'))
                        || at + 2 == self.bytes.len()
                }
                Some(b'!' | b'?') => true,
                None => true,
                _ => false,
            };
            if !markup {
                self.i += 1;
                continue;
            }
            self.push(Tok::Text, text_start, at);
            match next {
                None => {
                    self.end = End::TagName { lt: at, closing: false };
                    self.i = self.bytes.len();
                }
                Some(b'!') => self.bang(at),
                Some(b'?') => self.opaque(at, Tok::Pi, if self.xml { "?>" } else { ">" }, 2),
                Some(b'/') => self.tag(at, true),
                _ => self.tag(at, false),
            }
            text_start = self.i;
        }
        self.push(Tok::Text, text_start, self.i);
    }

    fn opaque(&mut self, at: usize, kind: Tok, terminator: &str, skip: usize) {
        match self.find(at + skip, terminator) {
            Some(close) => {
                self.i = close + terminator.len();
            }
            None => {
                self.i = self.bytes.len();
                self.end = End::Opaque;
            }
        }
        self.push(kind, at, self.i);
    }

    fn bang(&mut self, at: usize) {
        if self.starts_with_ci(at, "<!--") {
            self.opaque(at, Tok::Comment, "-->", 2);
        } else if self.starts_with_ci(at, "<!doctype") {
            let subset =
                self.xml && self.find(at, "[").is_some_and(|open| self.find(at, ">").is_none_or(|close| open < close));
            self.opaque(at, Tok::Doctype, if subset { "]>" } else { ">" }, 2);
        } else if self.starts_with_ci(at, "<![CDATA[") {
            self.opaque(at, Tok::Cdata, "]]>", 9);
        } else {
            self.opaque(at, Tok::Comment, ">", 2);
        }
    }

    fn skip_spaces(&mut self) {
        while self.i < self.bytes.len() && is_space(self.bytes[self.i]) {
            self.i += 1;
        }
    }

    fn tag(&mut self, lt: usize, closing: bool) {
        let name_start = lt + if closing { 2 } else { 1 };
        self.push(if closing { Tok::LtSlash } else { Tok::Lt }, lt, name_start);
        self.i = name_start;
        while self.i < self.bytes.len()
            && !matches!(self.bytes[self.i], b'/' | b'>' | b'<')
            && !is_space(self.bytes[self.i])
        {
            self.i += 1;
        }
        let name = (name_start, self.i);
        self.push(Tok::TagName, name.0, name.1);
        if self.i == self.bytes.len() {
            self.end = End::TagName { lt, closing };
            return;
        }
        loop {
            self.skip_spaces();
            let Some(&byte) = self.bytes.get(self.i) else {
                self.end = End::Tag { lt, name };
                return;
            };
            match byte {
                b'>' => {
                    self.push(Tok::Gt, self.i, self.i + 1);
                    self.i += 1;
                    if !closing {
                        self.raw_text(name);
                    }
                    return;
                }
                b'/' if self.bytes.get(self.i + 1) == Some(&b'>') => {
                    self.push(Tok::SlashGt, self.i, self.i + 2);
                    self.i += 2;
                    return;
                }
                b'/' => self.i += 1,
                b'<' => return,
                _ => {
                    if !self.attribute(lt, name) {
                        return;
                    }
                }
            }
        }
    }

    fn attribute(&mut self, lt: usize, name: (usize, usize)) -> bool {
        let attr_start = self.i;
        self.i += 1;
        while self.i < self.bytes.len()
            && !matches!(self.bytes[self.i], b'/' | b'>' | b'=' | b'<')
            && !is_space(self.bytes[self.i])
        {
            self.i += 1;
        }
        let attr = (attr_start, self.i);
        self.push(Tok::AttrName, attr.0, attr.1);
        if self.i == self.bytes.len() {
            self.end = End::AttrName { lt, name, attr: attr_start };
            return false;
        }
        let after_name = self.i;
        self.skip_spaces();
        if self.bytes.get(self.i) != Some(&b'=') {
            self.i = after_name;
            return true;
        }
        self.push(Tok::Eq, self.i, self.i + 1);
        self.i += 1;
        self.skip_spaces();
        let value_start = self.i;
        match self.bytes.get(self.i).copied() {
            None => {
                self.end = End::Value { lt, name, attr, start: value_start, quote: None };
                false
            }
            Some(quote @ (b'"' | b'\'')) => match self.bytes[self.i + 1..].iter().position(|&byte| byte == quote) {
                Some(offset) => {
                    self.i += offset + 2;
                    self.push(Tok::AttrValue, value_start, self.i);
                    true
                }
                None => {
                    self.i = self.bytes.len();
                    self.push(Tok::AttrValue, value_start, self.i);
                    self.end = End::Value { lt, name, attr, start: value_start + 1, quote: Some(quote) };
                    false
                }
            },
            Some(_) => {
                while self.i < self.bytes.len() && self.bytes[self.i] != b'>' && !is_space(self.bytes[self.i]) {
                    self.i += 1;
                }
                self.push(Tok::AttrValue, value_start, self.i);
                if self.i == self.bytes.len() {
                    self.end = End::Value { lt, name, attr, start: value_start, quote: None };
                    return false;
                }
                true
            }
        }
    }

    fn raw_text(&mut self, name: (usize, usize)) {
        if self.xml {
            return;
        }
        let tag = &self.bytes[name.0..name.1];
        let Some(&(element, entities)) =
            RAW_TEXT.iter().find(|(element, _)| tag.eq_ignore_ascii_case(element.as_bytes()))
        else {
            return;
        };
        let start = self.i;
        let mut close = None;
        let mut at = start;
        while let Some(found) = self.find(at, "</") {
            let after = found + 2 + element.len();
            if self.starts_with_ci(found + 2, element)
                && self.bytes.get(after).is_none_or(|&byte| is_space(byte) || byte == b'>' || byte == b'/')
            {
                close = Some(found);
                break;
            }
            at = found + 2;
        }
        let end = close.unwrap_or(self.bytes.len());
        if entities {
            let mut text_start = start;
            let mut i = start;
            while i < end {
                if let Some(len) = entity_len(&self.bytes[i..end]) {
                    self.push(Tok::Text, text_start, i);
                    self.push(Tok::Entity, i, i + len);
                    i += len;
                    text_start = i;
                } else {
                    i += 1;
                }
            }
            self.push(Tok::Text, text_start, end);
        } else {
            self.push(Tok::RawText, start, end);
        }
        self.i = end;
        if close.is_none() {
            self.end = End::Opaque;
        } else {
            self.tag(end, true);
        }
    }
}
