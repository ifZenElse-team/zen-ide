use zen_syntax::{HighlightKind, Highlighter, Span};

const DIRECTIVES: &[&str] =
    &["module", "go", "toolchain", "require", "replace", "exclude", "retract", "godebug", "tool", "ignore", "use"];

fn base_name(file_name: &str) -> &str {
    file_name.rsplit(['/', '\\']).next().unwrap_or(file_name)
}

fn is_version(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next() == Some('v') && chars.next().is_some_and(|c| c.is_ascii_digit())
}

fn is_go_release(word: &str) -> bool {
    let number = word.strip_prefix("go").unwrap_or(word);
    number.starts_with(|c: char| c.is_ascii_digit()) || word == "default"
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Space,
    Newline,
    Comment,
    Quoted,
    Punct,
    Word,
}

pub(crate) fn tokenize(text: &str) -> Vec<(Token, usize, usize)> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        let start = i;
        let rest = &text[i..];
        let token = if c == '\n' {
            i += 1;
            Token::Newline
        } else if c.is_whitespace() {
            i += rest.find(|c: char| c == '\n' || !c.is_whitespace()).unwrap_or(rest.len());
            Token::Space
        } else if rest.starts_with("//") {
            i += rest.find('\n').unwrap_or(rest.len());
            Token::Comment
        } else if c == '"' || c == '`' {
            i += quoted_len(rest, c);
            Token::Quoted
        } else if rest.starts_with("=>") {
            i += 2;
            Token::Punct
        } else if matches!(c, '(' | ')' | '[' | ']' | ',') {
            i += 1;
            Token::Punct
        } else {
            let end = rest
                .char_indices()
                .find(|&(at, c)| {
                    at > 0
                        && (c.is_whitespace()
                            || matches!(c, '(' | ')' | '[' | ']' | ',' | '"' | '`')
                            || rest[at..].starts_with("//")
                            || rest[at..].starts_with("=>"))
                })
                .map_or(rest.len(), |(at, _)| at);
            i += end;
            Token::Word
        };
        tokens.push((token, start, i));
    }
    tokens
}

fn quoted_len(rest: &str, quote: char) -> usize {
    let mut chars = rest.char_indices().skip(1);
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' if quote == '"' => {
                chars.next();
            }
            '\n' if quote == '"' => return at,
            c if c == quote => return at + 1,
            _ => {}
        }
    }
    rest.len()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GoModHighlighter;

impl Highlighter for GoModHighlighter {
    fn handles(&self, file_name: &str) -> bool {
        matches!(base_name(file_name), "go.mod" | "go.work")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut block: Option<&str> = None;
        let mut directive: Option<&str> = None;
        let mut line_start = true;
        let mut previous_word: Option<&str> = None;
        for (token, start, end) in tokenize(text) {
            let word = &text[start..end];
            let kind = match token {
                Token::Space => None,
                Token::Newline => {
                    line_start = true;
                    directive = block;
                    previous_word = None;
                    None
                }
                Token::Comment => Some(HighlightKind::Comment),
                Token::Quoted => Some(HighlightKind::String),
                Token::Punct => {
                    if word == "(" && block.is_none() && directive.is_some() {
                        block = directive;
                    } else if word == ")" {
                        block = None;
                    }
                    line_start = false;
                    Some(HighlightKind::Punctuation)
                }
                Token::Word => {
                    let first = line_start;
                    line_start = false;
                    let kind = if first && block.is_none() && DIRECTIVES.contains(&word) {
                        directive = Some(word);
                        Some(HighlightKind::Keyword)
                    } else if is_version(word)
                        || (matches!(directive, Some("go" | "toolchain"))
                            && previous_word == directive
                            && is_go_release(word))
                    {
                        Some(HighlightKind::Number)
                    } else if word == "=>" {
                        None
                    } else {
                        Some(HighlightKind::String)
                    };
                    previous_word = Some(word);
                    kind
                }
            };
            if let Some(kind) = kind {
                spans.push(Span { range: start..end, kind });
            }
        }
        spans
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GoSumHighlighter;

impl Highlighter for GoSumHighlighter {
    fn handles(&self, file_name: &str) -> bool {
        matches!(base_name(file_name), "go.sum" | "go.work.sum")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let mut field = 0;
            let mut rest = line;
            let mut at = offset;
            loop {
                let skipped = rest.len() - rest.trim_start().len();
                rest = &rest[skipped..];
                at += skipped;
                if rest.is_empty() {
                    break;
                }
                let length = rest.find(char::is_whitespace).unwrap_or(rest.len());
                let kind = match field {
                    0 => Some(HighlightKind::String),
                    1 => Some(HighlightKind::Number),
                    _ => Some(HighlightKind::Comment),
                };
                if let Some(kind) = kind {
                    spans.push(Span { range: at..at + length, kind });
                }
                field += 1;
                rest = &rest[length..];
                at += length;
            }
            offset += line.len();
        }
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(highlighter: &dyn Highlighter, text: &str) -> Vec<(String, HighlightKind)> {
        highlighter.highlight(text).into_iter().map(|span| (text[span.range].to_owned(), span.kind)).collect()
    }

    fn expect(found: Vec<(String, HighlightKind)>, expected: &[(&str, HighlightKind)]) {
        let expected: Vec<(String, HighlightKind)> =
            expected.iter().map(|&(text, kind)| (text.to_owned(), kind)).collect();
        assert_eq!(found, expected);
    }

    use HighlightKind::{Comment, Keyword, Number as Num, Punctuation, String as Str};

    #[test]
    fn highlights_go_mod() {
        let text = "// Example module.\nmodule example.com/hello\n\ngo 1.22.0\n\ntoolchain go1.22.1\n\nrequire (\n\tgolang.org/x/text v0.14.0 // indirect\n\trsc.io/quote/v3 v3.1.0\n\texample.com/old v0.0.0-20240101000000-abcdef123456\n)\n\nrequire example.com/one v1.2.3+incompatible\n\nreplace example.com/old v1.0.0 => ../old\n\nexclude example.com/bad v1.0.1\n\nretract [v1.0.0, v1.0.5]\n\ngodebug default=go1.21\n\ntool golang.org/x/tools/cmd/stringer\n\nignore ./node_modules\n";
        expect(
            kinds(&GoModHighlighter, text),
            &[
                ("// Example module.", Comment),
                ("module", Keyword),
                ("example.com/hello", Str),
                ("go", Keyword),
                ("1.22.0", Num),
                ("toolchain", Keyword),
                ("go1.22.1", Num),
                ("require", Keyword),
                ("(", Punctuation),
                ("golang.org/x/text", Str),
                ("v0.14.0", Num),
                ("// indirect", Comment),
                ("rsc.io/quote/v3", Str),
                ("v3.1.0", Num),
                ("example.com/old", Str),
                ("v0.0.0-20240101000000-abcdef123456", Num),
                (")", Punctuation),
                ("require", Keyword),
                ("example.com/one", Str),
                ("v1.2.3+incompatible", Num),
                ("replace", Keyword),
                ("example.com/old", Str),
                ("v1.0.0", Num),
                ("=>", Punctuation),
                ("../old", Str),
                ("exclude", Keyword),
                ("example.com/bad", Str),
                ("v1.0.1", Num),
                ("retract", Keyword),
                ("[", Punctuation),
                ("v1.0.0", Num),
                (",", Punctuation),
                ("v1.0.5", Num),
                ("]", Punctuation),
                ("godebug", Keyword),
                ("default=go1.21", Str),
                ("tool", Keyword),
                ("golang.org/x/tools/cmd/stringer", Str),
                ("ignore", Keyword),
                ("./node_modules", Str),
            ],
        );
    }

    #[test]
    fn highlights_go_work_and_quoted_paths() {
        let text = "go 1.23\n\nuse (\n\t./api\n\t\"./my tool\"\n)\nreplace \"example.com/x\" v1.0.0 => \"../x\"\n";
        expect(
            kinds(&GoModHighlighter, text),
            &[
                ("go", Keyword),
                ("1.23", Num),
                ("use", Keyword),
                ("(", Punctuation),
                ("./api", Str),
                ("\"./my tool\"", Str),
                (")", Punctuation),
                ("replace", Keyword),
                ("\"example.com/x\"", Str),
                ("v1.0.0", Num),
                ("=>", Punctuation),
                ("\"../x\"", Str),
            ],
        );
    }

    #[test]
    fn directive_names_inside_a_block_are_module_paths() {
        let text = "require (\n\ttool v1.0.0\n)\n";
        expect(
            kinds(&GoModHighlighter, text),
            &[("require", Keyword), ("(", Punctuation), ("tool", Str), ("v1.0.0", Num), (")", Punctuation)],
        );
    }

    #[test]
    fn highlights_go_sum() {
        let text = "golang.org/x/text v0.14.0 h1:ScX5w1eTa3QqT8oi6+ziP7dTV1S2+ALU0bI+0zXKWiQ=\ngolang.org/x/text v0.14.0/go.mod h1:18ZOQIKpY8NJVqYksKHtTdi31H5itFRjB5/qKTNYzSU=\n";
        expect(
            kinds(&GoSumHighlighter, text),
            &[
                ("golang.org/x/text", Str),
                ("v0.14.0", Num),
                ("h1:ScX5w1eTa3QqT8oi6+ziP7dTV1S2+ALU0bI+0zXKWiQ=", Comment),
                ("golang.org/x/text", Str),
                ("v0.14.0/go.mod", Num),
                ("h1:18ZOQIKpY8NJVqYksKHtTdi31H5itFRjB5/qKTNYzSU=", Comment),
            ],
        );
    }

    #[test]
    fn handles_module_files_only() {
        for name in ["go.mod", "go.work", "a/b/go.mod"] {
            assert!(GoModHighlighter.handles(name), "{name}");
        }
        for name in ["go.sum", "go.work.sum", "main.go", "xgo.mod"] {
            assert!(!GoModHighlighter.handles(name), "{name}");
        }
        for name in ["go.sum", "go.work.sum", "dir\\go.sum"] {
            assert!(GoSumHighlighter.handles(name), "{name}");
        }
        for name in ["go.mod", "main.go", "go.summary"] {
            assert!(!GoSumHighlighter.handles(name), "{name}");
        }
    }

    #[test]
    fn survives_any_input() {
        let samples = [
            "",
            "(",
            ")",
            "\"open",
            "`open",
            "//",
            "=>",
            "=",
            "v",
            "go",
            "require (\n",
            "é v1 ü\r\n",
            "module 例え.com\u{00A0}v1.0.0 // コメント",
            "  \t\n\n",
            "a\"b\"c",
            "x=>y//z",
        ];
        for text in samples {
            for highlighter in [&GoModHighlighter as &dyn Highlighter, &GoSumHighlighter] {
                let mut previous = 0;
                for span in highlighter.highlight(text) {
                    assert!(span.range.start >= previous, "{text:?}: unsorted");
                    assert!(span.range.start < span.range.end && span.range.end <= text.len(), "{text:?}");
                    assert!(
                        text.is_char_boundary(span.range.start) && text.is_char_boundary(span.range.end),
                        "{text:?}"
                    );
                    previous = span.range.end;
                }
            }
            let tokens = tokenize(text);
            assert_eq!(tokens.last().map_or(0, |token| token.2), text.len(), "{text:?}");
        }
    }
}
