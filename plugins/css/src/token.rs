#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "each plug-in that includes this file reads one dialect")]
pub(crate) enum Dialect {
    Css,
    Scss,
}

pub(crate) fn scss() -> bool {
    crate::dialect() == Dialect::Scss
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    Space,
    Comment,
    String,
    Url,
    Number,
    Percentage,
    Dimension,
    Hash,
    AtKeyword,
    Ident,
    Function,
    Delim,
    Colon,
    Semicolon,
    Comma,
    OpenBrace,
    CloseBrace,
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    Cdo,
    Variable,
    Interpolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) tok: Tok,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let c = bytes[at];
        let tok = match c {
            b' ' | b'\t' | b'\n' | b'\r' | 0x0C => {
                at += bytes[at..].iter().take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)).count();
                Tok::Space
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at = text[at + 2..].find("*/").map_or(bytes.len(), |close| at + 2 + close + 2);
                Tok::Comment
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') && scss() => {
                at += bytes[at..].iter().take_while(|b| **b != b'\n').count();
                Tok::Comment
            }
            b'$' if scss() && starts_ident(bytes, at + 1) => {
                at = name_end(bytes, at + 1);
                Tok::Variable
            }
            b'#' if scss() && bytes.get(at + 1) == Some(&b'{') => {
                at = interpolation_end(bytes, at + 1);
                Tok::Interpolation
            }
            b'"' | b'\'' => {
                at = string_end(bytes, at);
                Tok::String
            }
            b'#' if at + 1 < bytes.len() && (is_name_byte(bytes[at + 1]) || is_escape(bytes, at + 1)) => {
                at = name_end(bytes, at + 1);
                Tok::Hash
            }
            b'@' if starts_ident(bytes, at + 1) => {
                at = name_end(bytes, at + 1);
                Tok::AtKeyword
            }
            b'<' if bytes[at..].starts_with(b"<!--") => {
                at += 4;
                Tok::Cdo
            }
            b'-' if bytes[at..].starts_with(b"-->") => {
                at += 3;
                Tok::Cdo
            }
            _ if starts_number(bytes, at) => {
                at = number_end(bytes, at);
                if starts_ident(bytes, at) {
                    at = name_end(bytes, at);
                    Tok::Dimension
                } else if bytes.get(at) == Some(&b'%') {
                    at += 1;
                    Tok::Percentage
                } else {
                    Tok::Number
                }
            }
            _ if starts_ident(bytes, at) => {
                at = name_end(bytes, at);
                if bytes.get(at) != Some(&b'(') {
                    Tok::Ident
                } else if text[start..at].eq_ignore_ascii_case("url") && !quoted_after(bytes, at + 1) {
                    at = url_end(bytes, at + 1);
                    Tok::Url
                } else {
                    Tok::Function
                }
            }
            _ => {
                at += char_len(c).min(bytes.len() - at);
                match c {
                    b':' => Tok::Colon,
                    b';' => Tok::Semicolon,
                    b',' => Tok::Comma,
                    b'{' => Tok::OpenBrace,
                    b'}' => Tok::CloseBrace,
                    b'(' => Tok::OpenParen,
                    b')' => Tok::CloseParen,
                    b'[' => Tok::OpenBracket,
                    b']' => Tok::CloseBracket,
                    _ => Tok::Delim,
                }
            }
        };
        tokens.push(Token { tok, start, end: at });
    }
    tokens
}

pub(crate) fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

fn char_len(lead: u8) -> usize {
    match lead {
        0..0x80 => 1,
        0xC0..0xE0 => 2,
        0xE0..0xF0 => 3,
        0xF0.. => 4,
        _ => 1,
    }
}

fn is_escape(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&b'\\') && bytes.get(at + 1).is_some_and(|b| *b != b'\n' && *b != b'\r' && *b != 0x0C)
}

fn escape_end(bytes: &[u8], at: usize) -> usize {
    let first = at + 1;
    let hex = bytes[first..].iter().take(6).take_while(|b| b.is_ascii_hexdigit()).count();
    if hex == 0 {
        return (first + char_len(bytes[first])).min(bytes.len());
    }
    let mut end = first + hex;
    if bytes[end..].starts_with(b"\r\n") {
        end += 2;
    } else if bytes.get(end).is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)) {
        end += 1;
    }
    end
}

fn starts_ident(bytes: &[u8], at: usize) -> bool {
    match bytes.get(at) {
        Some(b'-') => bytes.get(at + 1).is_some_and(|b| *b == b'-' || is_name_start(*b)) || is_escape(bytes, at + 1),
        Some(b'\\') => is_escape(bytes, at),
        Some(&b) => is_name_start(b),
        None => false,
    }
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn name_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() {
        if is_name_byte(bytes[at]) {
            at += 1;
        } else if is_escape(bytes, at) {
            at = escape_end(bytes, at);
        } else {
            break;
        }
    }
    at
}

fn starts_number(bytes: &[u8], at: usize) -> bool {
    let digit_at = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    match bytes.get(at) {
        Some(b'0'..=b'9') => true,
        Some(b'.') => digit_at(at + 1),
        Some(b'+' | b'-') => digit_at(at + 1) || (bytes.get(at + 1) == Some(&b'.') && digit_at(at + 2)),
        _ => false,
    }
}

fn number_end(bytes: &[u8], mut at: usize) -> usize {
    let digits = |from: usize| bytes[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    if matches!(bytes[at], b'+' | b'-') {
        at += 1;
    }
    at += digits(at);
    if bytes.get(at) == Some(&b'.') && bytes.get(at + 1).is_some_and(u8::is_ascii_digit) {
        at += 1 + digits(at + 1);
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(at + 1), Some(b'+' | b'-')));
        if bytes.get(at + 1 + sign).is_some_and(u8::is_ascii_digit) {
            at += 1 + sign + digits(at + 1 + sign);
        }
    }
    at
}

fn string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut at = start + 1;
    loop {
        match bytes.get(at) {
            None => return bytes.len(),
            Some(b'\n') => return at,
            Some(&b) if b == quote => return at + 1,
            Some(b'\\') => {
                at += if bytes[at + 1..].starts_with(b"\r\n") {
                    3
                } else {
                    1 + bytes.get(at + 1).map_or(0, |b| char_len(*b))
                };
                at = at.min(bytes.len());
            }
            Some(_) => at += 1,
        }
    }
}

fn interpolation_end(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    let mut at = open;
    while at < bytes.len() {
        match bytes[at] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return at + 1;
                }
            }
            b'"' | b'\'' => {
                at = string_end(bytes, at);
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    at
}

fn quoted_after(bytes: &[u8], at: usize) -> bool {
    bytes[at.min(bytes.len())..]
        .iter()
        .find(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C))
        .is_some_and(|b| matches!(b, b'"' | b'\''))
}

fn url_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() {
        match bytes[at] {
            b')' => return at + 1,
            b'\n' => return at,
            b'\\' if is_escape(bytes, at) => at = escape_end(bytes, at),
            _ => at += 1,
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(text: &str) -> Vec<(&str, Tok)> {
        tokenize(text).into_iter().map(|t| (&text[t.start..t.end], t.tok)).collect()
    }

    #[test]
    fn reads_the_dialect() {
        if scss() {
            assert_eq!(
                toks("$a: 1; // x\n.b-#{$a + \"}\"} { }"),
                [
                    ("$a", Tok::Variable),
                    (":", Tok::Colon),
                    (" ", Tok::Space),
                    ("1", Tok::Number),
                    (";", Tok::Semicolon),
                    (" ", Tok::Space),
                    ("// x", Tok::Comment),
                    ("\n", Tok::Space),
                    (".", Tok::Delim),
                    ("b-", Tok::Ident),
                    ("#{$a + \"}\"}", Tok::Interpolation),
                    (" ", Tok::Space),
                    ("{", Tok::OpenBrace),
                    (" ", Tok::Space),
                    ("}", Tok::CloseBrace),
                ]
            );
        } else {
            assert_eq!(
                toks("$a //"),
                [("$", Tok::Delim), ("a", Tok::Ident), (" ", Tok::Space), ("/", Tok::Delim), ("/", Tok::Delim)]
            );
        }
    }

    #[test]
    fn splits_numbers_names_and_punctuation() {
        assert_eq!(
            toks("a{w:-1.5e3px 50% .5 +2}"),
            [
                ("a", Tok::Ident),
                ("{", Tok::OpenBrace),
                ("w", Tok::Ident),
                (":", Tok::Colon),
                ("-1.5e3px", Tok::Dimension),
                (" ", Tok::Space),
                ("50%", Tok::Percentage),
                (" ", Tok::Space),
                (".5", Tok::Number),
                (" ", Tok::Space),
                ("+2", Tok::Number),
                ("}", Tok::CloseBrace),
            ]
        );
        assert_eq!(toks("1em"), [("1em", Tok::Dimension)]);
        assert_eq!(
            toks("--x -y -"),
            [("--x", Tok::Ident), (" ", Tok::Space), ("-y", Tok::Ident), (" ", Tok::Space), ("-", Tok::Delim)]
        );
        assert_eq!(
            toks("#fff #-a #"),
            [("#fff", Tok::Hash), (" ", Tok::Space), ("#-a", Tok::Hash), (" ", Tok::Space), ("#", Tok::Delim)]
        );
        assert_eq!(toks("@media @"), [("@media", Tok::AtKeyword), (" ", Tok::Space), ("@", Tok::Delim)]);
    }

    #[test]
    fn reads_strings_urls_comments_and_escapes() {
        assert_eq!(toks(r#""a\"b" 'c"#), [(r#""a\"b""#, Tok::String), (" ", Tok::Space), ("'c", Tok::String)]);
        assert_eq!(toks("'a\nb"), [("'a", Tok::String), ("\n", Tok::Space), ("b", Tok::Ident)]);
        assert_eq!(
            toks("url(a.png) URL( 'b' )"),
            [
                ("url(a.png)", Tok::Url),
                (" ", Tok::Space),
                ("URL", Tok::Function),
                ("(", Tok::OpenParen),
                (" ", Tok::Space),
                ("'b'", Tok::String),
                (" ", Tok::Space),
                (")", Tok::CloseParen),
            ]
        );
        assert_eq!(toks("/* x */a/* y"), [("/* x */", Tok::Comment), ("a", Tok::Ident), ("/* y", Tok::Comment)]);
        assert_eq!(
            toks(r".a\:b \31 x"),
            [(".", Tok::Delim), (r"a\:b", Tok::Ident), (" ", Tok::Space), (r"\31 x", Tok::Ident)]
        );
        assert_eq!(toks("<!-- -->"), [("<!--", Tok::Cdo), (" ", Tok::Space), ("-->", Tok::Cdo)]);
    }

    #[test]
    fn covers_any_input() {
        let pieces = [
            "\\", "\"", "'", "/*", "*/", "url(", "(", ")", "#", "@", "-", "--", ".", "1", "e", "é", "日", "\r\n", "%",
            "{", "$", "#{", "//", "}",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    let text = format!("{a}{b}{c}");
                    let mut previous = 0;
                    for token in tokenize(&text) {
                        assert!(token.start == previous && token.end > token.start, "{text:?}");
                        assert!(text.is_char_boundary(token.end), "{text:?}");
                        previous = token.end;
                    }
                    assert_eq!(previous, text.len(), "{text:?}");
                }
            }
        }
    }
}
