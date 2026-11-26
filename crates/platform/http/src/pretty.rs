pub fn pretty_body(content_type: Option<&str>, body: &str) -> String {
    let says_json = content_type.is_some_and(|kind| kind.to_ascii_lowercase().contains("json"));
    let looks_json = matches!(body.trim_start().chars().next(), Some('{' | '['));
    if says_json || looks_json { pretty_json(body).unwrap_or_else(|| body.to_owned()) } else { body.to_owned() }
}

pub fn pretty_json(text: &str) -> Option<String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text).ok()?;
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0usize;
    let mut chars = text.trim().chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                while let Some(c) = chars.next() {
                    out.push(c);
                    match c {
                        '\\' => out.extend(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '{' | '[' => {
                out.push(c);
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if matches!(chars.peek(), Some('}' | ']')) {
                    out.extend(chars.next());
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_keeping_order_and_numbers() {
        let text = r#"{"z":1,"a":[1.50,{"k":"v, \"q\" {x}"}],"e":{},"l":[ ]}"#;
        assert_eq!(
            pretty_json(text).unwrap(),
            "{\n  \"z\": 1,\n  \"a\": [\n    1.50,\n    {\n      \"k\": \"v, \\\"q\\\" {x}\"\n    }\n  ],\n  \"e\": {},\n  \"l\": []\n}"
        );
        assert_eq!(pretty_json("not json"), None);
        assert_eq!(pretty_json("42").as_deref(), Some("42"));
    }

    #[test]
    fn bodies_by_content_type() {
        assert_eq!(pretty_body(Some("application/json; charset=utf-8"), "{\"a\":1}"), "{\n  \"a\": 1\n}");
        assert_eq!(pretty_body(None, "[1,2]"), "[\n  1,\n  2\n]");
        assert_eq!(pretty_body(Some("text/plain"), "a:b,c"), "a:b,c");
        assert_eq!(pretty_body(Some("application/json"), "{broken"), "{broken");
    }
}
