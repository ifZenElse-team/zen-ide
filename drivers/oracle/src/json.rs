#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Raw {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    Array(Vec<Raw>),
    Object(Vec<(String, Raw)>),
}

impl Raw {
    pub(crate) fn get(&self, name: &str) -> Option<&Raw> {
        match self {
            Raw::Object(fields) => fields.iter().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
    }

    pub(crate) fn items(&self) -> &[Raw] {
        match self {
            Raw::Array(items) => items,
            _ => &[],
        }
    }

    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Raw::Text(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn to_json(&self) -> String {
        match self {
            Raw::Null => "null".to_owned(),
            Raw::Bool(value) => value.to_string(),
            Raw::Number(text) => text.clone(),
            Raw::Text(text) => serde_json::Value::from(text.as_str()).to_string(),
            Raw::Array(items) => format!("[{}]", items.iter().map(Raw::to_json).collect::<Vec<_>>().join(",")),
            Raw::Object(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(key, value)| format!("{}:{}", serde_json::Value::from(key.as_str()), value.to_json()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }

    pub(crate) fn scalar(&self) -> Option<String> {
        match self {
            Raw::Text(text) if text.is_empty() => None,
            Raw::Text(text) | Raw::Number(text) => Some(text.clone()),
            Raw::Bool(value) => Some(value.to_string()),
            Raw::Null | Raw::Array(_) | Raw::Object(_) => None,
        }
    }
}

struct Reader<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    text: &'a str,
}

impl Reader<'_> {
    fn blank(&mut self) {
        while self.chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            self.chars.next();
        }
    }

    fn expect(&mut self, wanted: char) -> Option<()> {
        self.blank();
        (self.chars.next()?.1 == wanted).then_some(())
    }

    fn word(&mut self, word: &str, value: Raw) -> Option<Raw> {
        for wanted in word.chars() {
            if self.chars.next()?.1 != wanted {
                return None;
            }
        }
        Some(value)
    }

    fn string(&mut self) -> Option<String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let (_, c) = self.chars.next()?;
            match c {
                '"' => return Some(out),
                '\\' => {
                    let (_, escaped) = self.chars.next()?;
                    match escaped {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'u' => {
                            let unit = self.hex4()?;
                            if (0xd800..0xdc00).contains(&unit) {
                                let low = if self.chars.next()?.1 == '\\' && self.chars.next()?.1 == 'u' {
                                    self.hex4()?
                                } else {
                                    return None;
                                };
                                let joined = 0x10000 + ((unit - 0xd800) << 10) + (low.checked_sub(0xdc00)? & 0x3ff);
                                out.push(char::from_u32(joined).unwrap_or('\u{fffd}'));
                            } else {
                                out.push(char::from_u32(unit).unwrap_or('\u{fffd}'));
                            }
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut unit = 0;
        for _ in 0..4 {
            unit = unit * 16 + self.chars.next()?.1.to_digit(16)?;
        }
        Some(unit)
    }

    fn number(&mut self) -> Option<Raw> {
        let (start, _) = *self.chars.peek()?;
        let mut end = start;
        while let Some(&(at, c)) = self.chars.peek() {
            if c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E') {
                end = at + c.len_utf8();
                self.chars.next();
            } else {
                break;
            }
        }
        (end > start).then(|| Raw::Number(self.text[start..end].to_owned()))
    }

    fn value(&mut self) -> Option<Raw> {
        self.blank();
        match self.chars.peek()?.1 {
            '{' => {
                self.chars.next();
                let mut fields = Vec::new();
                self.blank();
                if self.chars.peek()?.1 == '}' {
                    self.chars.next();
                    return Some(Raw::Object(fields));
                }
                loop {
                    let key = self.string()?;
                    self.expect(':')?;
                    fields.push((key, self.value()?));
                    self.blank();
                    match self.chars.next()?.1 {
                        ',' => self.blank(),
                        '}' => return Some(Raw::Object(fields)),
                        _ => return None,
                    }
                }
            }
            '[' => {
                self.chars.next();
                let mut items = Vec::new();
                self.blank();
                if self.chars.peek()?.1 == ']' {
                    self.chars.next();
                    return Some(Raw::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    self.blank();
                    match self.chars.next()?.1 {
                        ',' => {}
                        ']' => return Some(Raw::Array(items)),
                        _ => return None,
                    }
                }
            }
            '"' => self.string().map(Raw::Text),
            't' => self.word("true", Raw::Bool(true)),
            'f' => self.word("false", Raw::Bool(false)),
            'n' => self.word("null", Raw::Null),
            _ => self.number(),
        }
    }
}

pub(crate) fn read(text: &str, start: usize) -> Option<(Raw, usize)> {
    let rest = text.get(start..)?;
    let mut reader = Reader { chars: rest.char_indices().peekable(), text: rest };
    let value = reader.value()?;
    let end = reader.chars.peek().map_or(rest.len(), |(at, _)| *at);
    Some((value, start + end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_keep_their_text_and_fields_their_order() {
        let text = r#"x {"b": 12345678901234567890.25, "a": [1, -2e3, "q\"é"], "b": null, "c": true} tail"#;
        let (value, end) = read(text, 2).unwrap();
        assert_eq!(&text[end..], " tail");
        let Raw::Object(fields) = &value else { panic!("an object") };
        let names: Vec<&str> = fields.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["b", "a", "b", "c"]);
        assert_eq!(value.get("b"), Some(&Raw::Number("12345678901234567890.25".into())));
        assert_eq!(value.get("a").unwrap().items()[1], Raw::Number("-2e3".into()));
        assert_eq!(value.get("a").unwrap().items()[2].text(), Some("q\"é"));
        assert_eq!(value.get("c"), Some(&Raw::Bool(true)));
        assert_eq!(value.get("a").unwrap().to_json(), r#"[1,-2e3,"q\"é"]"#);
    }

    #[test]
    fn broken_json_is_not_read() {
        assert!(read(r#"{"a": "#, 0).is_none());
        assert!(read(r#"{"a" 1}"#, 0).is_none());
        assert_eq!(Raw::Text(String::new()).scalar(), None);
    }
}
