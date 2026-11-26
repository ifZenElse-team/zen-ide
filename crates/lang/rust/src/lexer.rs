#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Whitespace,
    LineComment { doc: bool },
    BlockComment { doc: bool },
    Ident,
    Lifetime,
    Char,
    String,
    Number,
    Punct(char),
}

pub(crate) fn tokenize(text: &str) -> Vec<(Token, usize, usize)> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let c = text[i..].chars().next().unwrap_or('\0');
        let token = if c.is_whitespace() {
            i += c.len_utf8();
            while let Some(next) = text[i..].chars().next().filter(|c| c.is_whitespace()) {
                i += next.len_utf8();
            }
            Token::Whitespace
        } else if text[i..].starts_with("//") {
            let doc = (text[i..].starts_with("///") && !text[i..].starts_with("////")) || text[i..].starts_with("//!");
            i = text[i..].find('\n').map_or(bytes.len(), |end| i + end);
            Token::LineComment { doc }
        } else if text[i..].starts_with("/*") {
            let doc = (text[i..].starts_with("/**") && !text[i..].starts_with("/**/")) || text[i..].starts_with("/*!");
            i = block_comment_end(text, i);
            Token::BlockComment { doc }
        } else if let Some(end) = raw_string_end(text, i) {
            i = end;
            Token::String
        } else if c == '"' || ((c == 'b' || c == 'c') && text[i + 1..].starts_with('"')) {
            i += if c == '"' { 0 } else { 1 };
            i = quoted_end(text, i + 1, '"');
            Token::String
        } else if c == 'b' && text[i + 1..].starts_with('\'') {
            i = quoted_end(text, i + 2, '\'');
            Token::Char
        } else if c == '\'' {
            lifetime_or_char(text, &mut i)
        } else if c.is_ascii_digit() {
            i += 1;
            while let Some(next) = text[i..].chars().next() {
                let exponent_sign = (next == '+' || next == '-')
                    && matches!(bytes[i - 1], b'e' | b'E')
                    && !text[start..i].starts_with("0x");
                let fraction = next == '.' && text[i + 1..].chars().next().is_some_and(|after| after.is_ascii_digit());
                if next.is_ascii_alphanumeric() || next == '_' || exponent_sign || fraction {
                    i += next.len_utf8();
                } else {
                    break;
                }
            }
            Token::Number
        } else if c == '_' || c.is_alphabetic() {
            if text[i..].starts_with("r#") {
                i += 2;
            }
            while let Some(next) = text[i..].chars().next().filter(|c| *c == '_' || c.is_alphanumeric()) {
                i += next.len_utf8();
            }
            Token::Ident
        } else {
            i += c.len_utf8();
            Token::Punct(c)
        };
        tokens.push((token, start, i));
    }
    tokens
}

fn block_comment_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let (mut i, mut depth) = (start + 2, 1);
    while i < bytes.len() {
        if text[i..].starts_with("/*") {
            depth += 1;
            i += 2;
        } else if text[i..].starts_with("*/") {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn raw_string_end(text: &str, start: usize) -> Option<usize> {
    let rest = &text[start..];
    let after_prefix =
        rest.strip_prefix("br").or_else(|| rest.strip_prefix("cr")).or_else(|| rest.strip_prefix('r'))?;
    let hashes = after_prefix.chars().take_while(|c| *c == '#').count();
    if !after_prefix[hashes..].starts_with('"') {
        return None;
    }
    let body_start = start + (rest.len() - after_prefix.len()) + hashes + 1;
    let closing = format!("\"{}", "#".repeat(hashes));
    Some(text[body_start..].find(&closing).map_or(text.len(), |end| body_start + end + closing.len()))
}

fn quoted_end(text: &str, from: usize, quote: char) -> usize {
    let mut chars = text[from..].char_indices();
    while let Some((offset, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            c if c == quote => return from + offset + 1,
            '\n' if quote == '\'' => return from + offset,
            _ => {}
        }
    }
    text.len()
}

fn lifetime_or_char(text: &str, i: &mut usize) -> Token {
    let start = *i;
    let mut chars = text[start + 1..].chars();
    let first = chars.next();
    let second = chars.next();
    let is_char = matches!((first, second), (Some('\\'), _) | (Some(_), Some('\'')));
    if is_char {
        *i = quoted_end(text, start + 1, '\'');
        Token::Char
    } else {
        *i = start + 1;
        while let Some(next) = text[*i..].chars().next().filter(|c| *c == '_' || c.is_alphanumeric()) {
            *i += next.len_utf8();
        }
        Token::Lifetime
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<(Token, &str)> {
        tokenize(text)
            .into_iter()
            .filter(|(token, _, _)| *token != Token::Whitespace)
            .map(|(token, start, end)| (token, &text[start..end]))
            .collect()
    }

    #[test]
    fn covers_the_whole_input() {
        let text = "fn main() { let s = r#\"a \"b\" c\"#; /* x /* y */ z */ 'a' 'b; 1.5e-3_f64 }";
        let ranges = tokenize(text);
        assert_eq!(ranges.first().map(|t| t.1), Some(0));
        assert_eq!(ranges.last().map(|t| t.2), Some(text.len()));
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].2, pair[1].1);
        }
    }

    #[test]
    fn literals_comments_and_lifetimes() {
        let found = tokens("r#\"a \"b\" c\"# /* x /* y */ z */ 'a' 'b '\\n' b'x' b\"bytes\" 1.5e-3_f64 0xff");
        assert_eq!(found[0], (Token::String, "r#\"a \"b\" c\"#"));
        assert_eq!(found[1], (Token::BlockComment { doc: false }, "/* x /* y */ z */"));
        assert_eq!(found[2], (Token::Char, "'a'"));
        assert_eq!(found[3], (Token::Lifetime, "'b"));
        assert_eq!(found[4], (Token::Char, "'\\n'"));
        assert_eq!(found[5], (Token::Char, "b'x'"));
        assert_eq!(found[6], (Token::String, "b\"bytes\""));
        assert_eq!(found[7], (Token::Number, "1.5e-3_f64"));
        assert_eq!(found[8], (Token::Number, "0xff"));
    }

    #[test]
    fn unterminated_input_never_panics() {
        for text in ["\"abc", "/* open", "'", "r#\"x", "b'", "'\\", "é'"] {
            let _ = tokenize(text);
        }
    }
}
