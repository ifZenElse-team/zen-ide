use crate::value::Value;

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl Reader<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn skip_spaces(&mut self) {
        let trimmed = self.rest().trim_start_matches(' ');
        self.at = self.text.len() - trimmed.len();
    }

    fn eat(&mut self, token: char) -> bool {
        self.skip_spaces();
        if self.rest().starts_with(token) {
            self.at += token.len_utf8();
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        self.skip_spaces();
        let rest = self.rest();
        let matches = rest.len() >= word.len()
            && rest.is_char_boundary(word.len())
            && rest[..word.len()].eq_ignore_ascii_case(word)
            && !rest[word.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        if matches {
            self.at += word.len();
        }
        matches
    }

    fn string(&mut self) -> Option<String> {
        self.skip_spaces();
        let rest = self.rest().strip_prefix('\'')?;
        let mut out = String::new();
        let mut chars = rest.char_indices();
        while let Some((index, c)) = chars.next() {
            if c == '\'' {
                if rest[index + 1..].starts_with('\'') {
                    out.push('\'');
                    chars.next();
                } else {
                    self.at += 1 + index + 1;
                    return Some(out);
                }
            } else {
                out.push(c);
            }
        }
        None
    }

    fn integer(&mut self) -> Option<i64> {
        self.skip_spaces();
        let rest = self.rest();
        let end = rest
            .char_indices()
            .find(|(index, c)| !(c.is_ascii_digit() || (*index == 0 && (*c == '-' || *c == '+'))))
            .map_or(rest.len(), |(index, _)| index);
        let value = rest[..end].parse().ok()?;
        self.at += end;
        Some(value)
    }

    fn expression(&mut self) -> Option<Value> {
        self.skip_spaces();
        let rest = self.rest();
        if rest.starts_with('\'') {
            return self.string().map(Value::Text);
        }
        if rest.starts_with("X'") || rest.starts_with("x'") {
            self.at += 1;
            let hex = self.string()?;
            return unhex(&hex).map(Value::Bytes);
        }
        if self.eat_word("NULL") {
            return Some(Value::Null);
        }
        if self.eat_word("unistr") {
            if !self.eat('(') {
                return None;
            }
            let text = unescape(&self.string()?)?;
            return self.eat(')').then_some(Value::Text(text));
        }
        if self.eat_word("replace") {
            if !self.eat('(') {
                return None;
            }
            let Value::Text(text) = self.expression()? else { return None };
            if !self.eat(',') {
                return None;
            }
            let from = self.string()?;
            if !self.eat(',') {
                return None;
            }
            let Value::Text(to) = self.expression()? else { return None };
            return self.eat(')').then(|| Value::Text(text.replace(&from, &to)));
        }
        if self.eat_word("char") {
            if !self.eat('(') {
                return None;
            }
            let mut text = String::new();
            loop {
                let code = u32::try_from(self.integer()?).ok().and_then(char::from_u32)?;
                text.push(code);
                if self.eat(')') {
                    return Some(Value::Text(text));
                }
                if !self.eat(',') {
                    return None;
                }
            }
        }
        self.number()
    }

    fn number(&mut self) -> Option<Value> {
        let rest = self.rest();
        let end = rest.find([',', ')']).unwrap_or(rest.len());
        let token = rest[..end].trim();
        let value = match token {
            "Inf" | "+Inf" => Value::Float(f64::INFINITY),
            "-Inf" => Value::Float(f64::NEG_INFINITY),
            _ if !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit() || b == b'-' || b == b'+') => {
                match token.parse::<i64>() {
                    Ok(integer) => Value::Integer(integer),
                    Err(_) => Value::Float(token.parse().ok()?),
                }
            }
            _ => Value::Float(token.parse().ok()?),
        };
        self.at += end;
        Some(value)
    }
}

fn unhex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len()).step_by(2).map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok()).collect()
}

fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let digits = match chars.next()? {
            '\\' => {
                out.push('\\');
                continue;
            }
            'u' => 4,
            'U' => 8,
            '+' => 6,
            other => {
                out.push('\\');
                out.push(other);
                continue;
            }
        };
        let code: String = chars.by_ref().take(digits).collect();
        out.push(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32)?);
    }
    Some(out)
}

pub(super) fn row(line: &str) -> Option<Vec<Value>> {
    let mut reader = Reader { text: line, at: 0 };
    let mut values = Vec::new();
    loop {
        values.push(reader.expression()?);
        reader.skip_spaces();
        if reader.at == line.len() {
            return Some(values);
        }
        if !reader.eat(',') {
            return None;
        }
    }
}

pub(super) fn storage_class(value: &Value) -> Option<&'static str> {
    match value {
        Value::Integer(_) => Some("INTEGER"),
        Value::Float(_) => Some("REAL"),
        Value::Text(_) => Some("TEXT"),
        Value::Bytes(_) => Some("BLOB"),
        _ => None,
    }
}

pub(super) fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

pub(super) fn identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_of_every_storage_class_are_read() {
        assert_eq!(
            row("1,1.0,'x''y',x'00ff',NULL,Inf,-Inf,-9223372036854775808,1.0e-07,'é'").unwrap(),
            vec![
                Value::Integer(1),
                Value::Float(1.0),
                Value::Text("x'y".into()),
                Value::Bytes(vec![0, 255]),
                Value::Null,
                Value::Float(f64::INFINITY),
                Value::Float(f64::NEG_INFINITY),
                Value::Integer(i64::MIN),
                Value::Float(1.0e-7),
                Value::Text("é".into()),
            ]
        );
        assert_eq!(row("x'',''").unwrap(), vec![Value::Bytes(Vec::new()), Value::Text(String::new())]);
    }

    #[test]
    fn line_breaks_come_back_from_both_forms() {
        assert_eq!(
            row(r"unistr('x''y\u000az\\u'),2").unwrap(),
            vec![Value::Text("x'y\nz\\u".into()), Value::Integer(2)]
        );
        assert_eq!(
            row(r"replace(replace('a\nb\r','\n',char(10)),'\r',char(13))").unwrap(),
            vec![Value::Text("a\nb\r".into())]
        );
        assert_eq!(row("'a,b','c)'").unwrap(), vec![Value::Text("a,b".into()), Value::Text("c)".into())]);
    }

    #[test]
    fn a_line_that_is_not_literals_is_refused() {
        assert!(row("QUERY PLAN").is_none());
        assert!(row("`--SCAN t").is_none());
        assert!(row("'open").is_none());
        assert_eq!(identifier("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(literal("it's"), "'it''s'");
    }
}
