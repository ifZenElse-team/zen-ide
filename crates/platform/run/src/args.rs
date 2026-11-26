pub fn split(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                started = true;
                for quoted in chars.by_ref() {
                    if quoted == '\'' {
                        break;
                    }
                    word.push(quoted);
                }
            }
            '"' => {
                started = true;
                while let Some(quoted) = chars.next() {
                    match quoted {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(escaped @ ('"' | '\\' | '$' | '`')) => word.push(escaped),
                            Some(other) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => word.push('\\'),
                        },
                        other => word.push(other),
                    }
                }
            }
            '\\' => {
                started = true;
                if let Some(escaped) = chars.next() {
                    word.push(escaped);
                }
            }
            space if space.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            other => {
                started = true;
                word.push(other);
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn arguments_split_like_a_shell() {
        let cases: &[(&str, &[&str])] = &[
            ("", &[]),
            ("  -v   -count=1 ", &["-v", "-count=1"]),
            ("--name 'two words'", &["--name", "two words"]),
            (r#"--msg "say \"hi\"" x"#, &["--msg", r#"say "hi""#, "x"]),
            (r"a\ b c", &["a b", "c"]),
            ("''", &[""]),
            (r#""C:\dir""#, &[r"C:\dir"]),
        ];
        for (text, expected) in cases {
            assert_eq!(split(text), expected.iter().map(|word| word.to_string()).collect::<Vec<_>>(), "{text}");
        }
    }
}
