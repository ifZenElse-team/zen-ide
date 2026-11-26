pub fn strip(text: &str) -> String {
    let without_comments = strip_comments(text.as_bytes());
    let plain = strip_trailing_commas(without_comments);
    String::from_utf8(plain).unwrap_or_else(|_| text.to_owned())
}

fn blank(byte: u8) -> u8 {
    if byte == b'\n' || byte == b'\r' { byte } else { b' ' }
}

fn strip_comments(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        match (text[index], text.get(index + 1)) {
            (b'"', _) => {
                let end = string_end(text, index);
                out.extend_from_slice(&text[index..end]);
                index = end;
            }
            (b'/', Some(b'/')) => {
                while index < text.len() && text[index] != b'\n' && text[index] != b'\r' {
                    out.push(b' ');
                    index += 1;
                }
            }
            (b'/', Some(b'*')) => {
                let end = text[index + 2..]
                    .windows(2)
                    .position(|pair| pair == b"*/")
                    .map_or(text.len(), |position| index + 2 + position + 2);
                out.extend(text[index..end].iter().map(|&byte| blank(byte)));
                index = end;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    out
}

fn strip_trailing_commas(mut text: Vec<u8>) -> Vec<u8> {
    let mut index = 0;
    while index < text.len() {
        match text[index] {
            b'"' => index = string_end(&text, index),
            b',' => {
                let next = text[index + 1..].iter().find(|byte| !byte.is_ascii_whitespace());
                if matches!(next, Some(b'}' | b']')) {
                    text[index] = b' ';
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    text
}

fn string_end(text: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    while index < text.len() {
        match text[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> serde_json::Value {
        serde_json::from_str(&strip(text)).unwrap_or_else(|error| panic!("{error} in {:?}", strip(text)))
    }

    #[test]
    fn comments_become_spaces() {
        let text = "{\n  // a line comment\n  \"a\": 1, /* a block\n comment */ \"b\": 2\n}";
        let stripped = strip(text);
        assert_eq!(stripped.len(), text.len());
        assert_eq!(stripped.lines().count(), text.lines().count());
        assert_eq!(value(text), serde_json::json!({"a": 1, "b": 2}));
        assert_eq!(stripped.find("\"b\""), text.find("\"b\""));
    }

    #[test]
    fn trailing_commas_are_dropped() {
        assert_eq!(value("[1, 2, ]"), serde_json::json!([1, 2]));
        assert_eq!(value("{\"a\": [1,],\n}"), serde_json::json!({"a": [1]}));
        assert_eq!(value("{\"a\": 1, // last\n}"), serde_json::json!({"a": 1}));
        assert_eq!(value("{\"a\": 1, /* last */ }"), serde_json::json!({"a": 1}));
    }

    #[test]
    fn strings_keep_comment_markers_and_commas() {
        let text =
            r#"{"url": "https://example.com/a//b", "glob": "/* not a comment */", "list": ",]", "q": "a\"//b",}"#;
        assert_eq!(
            value(text),
            serde_json::json!({"url": "https://example.com/a//b", "glob": "/* not a comment */", "list": ",]", "q": "a\"//b"})
        );
    }

    #[test]
    fn plain_json_is_unchanged() {
        let text = "{\"a\": [1, 2], \"b\": {\"c\": \"d\"}}";
        assert_eq!(strip(text), text);
    }

    #[test]
    fn non_ascii_comments_keep_offsets() {
        let text = "{ /* clé — ü */ \"a\": \"π\" // naïve\n}";
        let stripped = strip(text);
        assert_eq!(stripped.len(), text.len());
        assert_eq!(value(text), serde_json::json!({"a": "π"}));
    }

    #[test]
    fn an_unclosed_comment_runs_to_the_end() {
        assert_eq!(strip("1 /* open"), "1        ");
        assert!(serde_json::from_str::<serde_json::Value>(&strip("[1, /* open")).is_err());
    }
}
