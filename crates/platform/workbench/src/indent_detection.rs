use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MIN_INDENTED_LINES: usize = 3;

pub const MAX_LINES: usize = 5000;

pub const MAX_BYTES: usize = 512 * 1024;

pub const SPACE_WIDTHS: [u8; 4] = [2, 3, 4, 8];

/// The indentation a file's own lines use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum DetectedIndent {
    /// One tab character per level.
    Tabs,
    /// Spaces, `width` of them per level.
    Spaces {
        /// How many spaces one level takes: 2, 3, 4 or 8.
        width: u8,
    },
}

struct Syntax {
    line_comments: &'static [&'static str],
    block_comments: &'static [(&'static str, &'static str)],
    long_strings: &'static [(&'static str, &'static str)],
    quotes: &'static [char],
    char_literals: bool,
    operators: bool,
}

const C_BLOCK: &[(&str, &str)] = &[("/*", "*/")];
const MARKUP_BLOCK: &[(&str, &str)] = &[("<!--", "-->")];

fn syntax(language: &str) -> Syntax {
    let c_like = |long_strings, char_literals| Syntax {
        line_comments: &["//"],
        block_comments: C_BLOCK,
        long_strings,
        quotes: &['"', '\''],
        char_literals,
        operators: true,
    };
    let hash = |long_strings| Syntax {
        line_comments: &["#"],
        block_comments: &[],
        long_strings,
        quotes: &['"', '\''],
        char_literals: false,
        operators: true,
    };
    match language {
        "go" => c_like(&[("`", "`")], true),
        "rust" => Syntax {
            quotes: &[],
            ..c_like(&[("r###\"", "\"###"), ("r##\"", "\"##"), ("r#\"", "\"#"), ("\"", "\"")], true)
        },
        "javascript" | "javascriptreact" | "typescript" | "typescriptreact" => c_like(&[("`", "`")], false),
        "java" | "kotlin" => c_like(&[("\"\"\"", "\"\"\"")], true),
        "json" | "proto" => c_like(&[], false),
        "css" | "scss" => {
            Syntax { line_comments: if language == "scss" { &["//"] } else { &[] }, ..c_like(&[], false) }
        }
        "sql" => Syntax { line_comments: &["--"], ..c_like(&[], false) },
        "python" | "toml" => hash(&[("\"\"\"", "\"\"\""), ("'''", "'''")]),
        "yaml" | "shellscript" | "makefile" | "dockerfile" | "properties" | "go.mod" => hash(&[]),
        "html" | "xml" => Syntax {
            line_comments: &[],
            block_comments: MARKUP_BLOCK,
            long_strings: &[],
            quotes: &['"', '\''],
            char_literals: false,
            operators: false,
        },
        "markdown" => Syntax {
            line_comments: &[],
            block_comments: MARKUP_BLOCK,
            long_strings: &[],
            quotes: &[],
            char_literals: false,
            operators: false,
        },
        _ => Syntax {
            line_comments: &["//", "#"],
            block_comments: C_BLOCK,
            long_strings: &[],
            quotes: &['"', '\''],
            char_literals: false,
            operators: false,
        },
    }
}

#[derive(Clone, Copy)]
enum Within {
    Code,
    Comment(&'static str),
    Text { end: &'static str, escapes: bool },
}

#[derive(Clone, Copy)]
struct Open {
    bracket: u8,
    at: usize,
    line: usize,
    hanging: bool,
}

struct Scanner {
    syntax: Syntax,
    within: Within,
    open: Vec<Open>,
    continues: bool,
}

impl Scanner {
    fn new(language: &str) -> Self {
        Self { syntax: syntax(language), within: Within::Code, open: Vec::new(), continues: false }
    }

    fn starts_comment(&self, text: &str) -> bool {
        self.syntax.line_comments.iter().any(|marker| text.starts_with(marker))
            || self.syntax.block_comments.iter().any(|(start, _)| text.starts_with(start))
    }

    fn scan(&mut self, line: &str, number: usize) {
        let bytes = line.as_bytes();
        let mut at = 0;
        let mut last: Option<(usize, u8)> = None;
        while at < bytes.len() {
            match self.within {
                Within::Comment(end) => match line[at..].find(end) {
                    Some(found) => {
                        at += found + end.len();
                        self.within = Within::Code;
                    }
                    None => at = bytes.len(),
                },
                Within::Text { end, escapes } => {
                    let rest = &line[at..];
                    if escapes && rest.starts_with('\\') {
                        at += 1 + rest[1..].chars().next().map_or(0, char::len_utf8);
                    } else if rest.starts_with(end) {
                        at += end.len();
                        self.within = Within::Code;
                        last = Some((at - 1, b'"'));
                    } else {
                        at += rest.chars().next().map_or(1, char::len_utf8);
                    }
                }
                Within::Code => {
                    let rest = &line[at..];
                    if self.syntax.line_comments.iter().any(|marker| rest.starts_with(marker)) {
                        break;
                    }
                    if let Some((start, end)) =
                        self.syntax.block_comments.iter().find(|(start, _)| rest.starts_with(start))
                    {
                        at += start.len();
                        self.within = Within::Comment(end);
                        continue;
                    }
                    if let Some((start, end)) =
                        self.syntax.long_strings.iter().find(|(start, _)| rest.starts_with(start))
                    {
                        at += start.len();
                        let raw = start.starts_with('r') || *start == "`";
                        self.within = Within::Text { end, escapes: !raw };
                        continue;
                    }
                    let c = rest.chars().next().unwrap_or(' ');
                    if c == '\'' && self.syntax.char_literals {
                        at += char_literal(rest).unwrap_or(1);
                        last = Some((at - 1, b'\''));
                        continue;
                    }
                    if self.syntax.quotes.contains(&c) {
                        at += quoted(rest, c);
                        last = Some((at - 1, b'"'));
                        continue;
                    }
                    match c {
                        '(' | '[' | '{' => self.open.push(Open { bracket: c as u8, at, line: number, hanging: false }),
                        ')' | ']' | '}' => {
                            self.open.pop();
                        }
                        _ => {}
                    }
                    if !c.is_whitespace() {
                        last = Some((at, c as u8));
                    }
                    at += c.len_utf8();
                }
            }
        }
        if let Some(top) = self.open.last_mut()
            && top.line == number
        {
            top.hanging = last.is_some_and(|(position, _)| position == top.at);
        }
        let ends_with_operator = matches!(last, Some((_, b'+' | b'=' | b'&' | b'|' | b'\\')));
        self.continues = self.syntax.operators && ends_with_operator && matches!(self.within, Within::Code);
    }

    fn inside_open_bracket(&self) -> bool {
        self.open.last().is_some_and(|open| matches!(open.bracket, b'(' | b'[') && !open.hanging)
    }
}

fn char_literal(rest: &str) -> Option<usize> {
    let mut chars = rest.char_indices().skip(1);
    let (_, first) = chars.next()?;
    if first == '\\' {
        let close = rest[2..].find('\'')?;
        return (close <= 10).then_some(close + 3);
    }
    let (at, second) = chars.next()?;
    (second == '\'' && first != '\'').then_some(at + 1)
}

fn quoted(rest: &str, quote: char) -> usize {
    let mut escaped = false;
    for (at, c) in rest.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return at + c.len_utf8();
        }
    }
    rest.len()
}

fn continues_with_operator(text: &str) -> bool {
    let bytes = text.as_bytes();
    let member = bytes.first() == Some(&b'.') && bytes.get(1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_');
    member || text.starts_with("&&") || text.starts_with("||")
}

pub fn detect(text: &str, language: &str) -> Option<DetectedIndent> {
    let mut end = text.len().min(MAX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let detected = count(&text[..end], language)?;
    let allowed = match detected {
        DetectedIndent::Tabs => language != "yaml",
        DetectedIndent::Spaces { .. } => language != "makefile",
    };
    allowed.then_some(detected)
}

fn count(text: &str, language: &str) -> Option<DetectedIndent> {
    let mut scanner = Scanner::new(language);
    let (mut tab_lines, mut space_lines) = (0_usize, 0_usize);
    let mut steps: BTreeMap<usize, usize> = BTreeMap::new();
    let mut previous: Option<usize> = Some(0);
    for (number, line) in text.lines().take(MAX_LINES).enumerate() {
        let inside = !matches!(scanner.within, Within::Code);
        let continued = scanner.continues || scanner.inside_open_bracket();
        let content = line.trim_start_matches([' ', '\t']);
        let skip = inside
            || content.trim().is_empty()
            || scanner.starts_comment(content)
            || continued
            || (scanner.syntax.operators && continues_with_operator(content));
        scanner.scan(line, number);
        if skip {
            continue;
        }
        let lead = &line[..line.len() - content.len()];
        if lead.starts_with('\t') {
            tab_lines += 1;
            previous = None;
            continue;
        }
        if lead.contains('\t') {
            previous = None;
            continue;
        }
        let width = lead.len();
        if width > 0 {
            space_lines += 1;
        }
        if let Some(before) = previous
            && width > before
        {
            *steps.entry(width - before).or_default() += 1;
        }
        previous = Some(width);
    }
    if tab_lines + space_lines < MIN_INDENTED_LINES || tab_lines == space_lines {
        return None;
    }
    if tab_lines > space_lines {
        return Some(DetectedIndent::Tabs);
    }
    let (step, _) = steps.iter().max_by(|(a, x), (b, y)| x.cmp(y).then(b.cmp(a)))?;
    let width = u8::try_from(*step).ok().filter(|width| SPACE_WIDTHS.contains(width))?;
    Some(DetectedIndent::Spaces { width })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACES_2: Option<DetectedIndent> = Some(DetectedIndent::Spaces { width: 2 });
    const SPACES_4: Option<DetectedIndent> = Some(DetectedIndent::Spaces { width: 4 });

    #[test]
    fn tabs_and_each_space_width_are_found() {
        let tabs = "func a() {\n\tif x {\n\t\ty()\n\t}\n\tz()\n}\n";
        assert_eq!(detect(tabs, "go"), Some(DetectedIndent::Tabs));
        for width in SPACE_WIDTHS {
            let one = " ".repeat(usize::from(width));
            let text = format!("a {{\n{one}b {{\n{one}{one}c\n{one}}}\n{one}d\n}}\n");
            assert_eq!(detect(&text, "javascript"), Some(DetectedIndent::Spaces { width }), "{width}");
        }
    }

    #[test]
    fn too_few_indented_lines_say_nothing() {
        assert_eq!(detect("a:\n  b: 1\n", "yaml"), None);
        assert_eq!(detect("a\n\tb\n", "plaintext"), None);
        assert_eq!(detect("", "go"), None);
        assert_eq!(detect("a {\n  b\n  c\n}\n", "css"), None, "two lines are too few");
        assert_eq!(detect("a {\n  b\n  c\n  d\n}\n", "css"), SPACES_2);
    }

    #[test]
    fn as_many_tabs_as_spaces_or_an_odd_width_say_nothing() {
        assert_eq!(detect("a\n\tb\n\tc\nd\n  e\n  f\n", "plaintext"), None);
        assert_eq!(detect("a\n      b\n      c\n      d\n", "plaintext"), None, "six is not a width");
        assert_eq!(detect("a\n b\n c\n d\n", "plaintext"), None, "nor is one");
    }

    #[test]
    fn comments_are_skipped() {
        let text = "/**\n * Docs.\n * More.\n * Even more.\n */\nfn a() {\n    b();\n    c();\n    d();\n}\n";
        assert_eq!(detect(text, "rust"), SPACES_4);
        let text = "a:\n    # a comment\n    # another\n  b: 1\n  c: 2\n  d: 3\n";
        assert_eq!(detect(text, "yaml"), SPACES_2);
        let text = "<div>\n<!--\n     note\n     note\n     note\n-->\n  <p>\n  <p>\n  <p>\n</div>\n";
        assert_eq!(detect(text, "html"), SPACES_2);
    }

    #[test]
    fn strings_over_several_lines_are_skipped() {
        let text = "var s = `\n   one\n   two\n   three\n`\n\nfunc a() {\n\tb()\n\tc()\n\td()\n}\n";
        assert_eq!(detect(text, "go"), Some(DetectedIndent::Tabs));
        let text = "def a():\n    \"\"\"\n       Docs.\n       More.\n    \"\"\"\n    b()\n    c()\n";
        assert_eq!(detect(text, "python"), SPACES_4);
        let text = "fn a() {\n    let s = \"\n           x\n           y\n           z\";\n    b();\n    c();\n}\n";
        assert_eq!(detect(text, "rust"), SPACES_4);
        let text = "let a = \"{\";\nfn b<'a>(x: &'a str) {\n    c();\n    d();\n    e();\n}\n";
        assert_eq!(detect(text, "rust"), SPACES_4, "a lifetime is no quote, a brace in a string no brace");
    }

    #[test]
    fn continuation_lines_are_skipped() {
        let text = "class A {\n  void a() {\n    call(first,\n            second,\n            third);\n    \
                    total = x +\n            y;\n    b();\n  }\n}\n";
        assert_eq!(detect(text, "java"), SPACES_2);
        let text =
            "promise\n        .then(a)\n        .then(b)\n        .then(c)\nif (x) {\n  y();\n  z();\n  w();\n}\n";
        assert_eq!(detect(text, "javascript"), SPACES_2);
        let text = "import (\n\t\"fmt\"\n\t\"os\"\n)\n\nvar x = []int{\n\t1,\n}\n";
        assert_eq!(detect(text, "go"), Some(DetectedIndent::Tabs), "a bracket at the end of a line opens a block");
    }

    #[test]
    fn the_most_common_step_wins() {
        let text = "a:\n  b:\n    c: 1\n    d: 2\n  e:\n    f: 1\n  g: 1\n";
        assert_eq!(detect(text, "yaml"), SPACES_2);
    }

    #[test]
    fn yaml_never_takes_tabs_and_a_makefile_never_takes_spaces() {
        let tabbed = "a:\n\tb: 1\n\tc: 2\n\td: 3\n";
        assert_eq!(detect(tabbed, "yaml"), None);
        assert_eq!(detect(tabbed, "toml"), Some(DetectedIndent::Tabs));
        let make = "define A\n    one\n    two\n    three\nendef\nall:\n\tgo build\n";
        assert_eq!(detect(make, "makefile"), None);
        assert_eq!(detect(make, "plaintext"), SPACES_4);
        let make = "all:\n\tgo build\n\tgo test\n\tgo vet\n";
        assert_eq!(detect(make, "makefile"), Some(DetectedIndent::Tabs));
    }

    #[test]
    fn only_the_start_of_a_long_file_is_read() {
        let head = "a {\n  b\n  c\n  d\n}\n";
        let text = format!("{head}{}\n{}", "x".repeat(MAX_BYTES), "\tz\n".repeat(10));
        assert_eq!(detect(&text, "plaintext"), SPACES_2);
        let text = format!("{}ё{head}", "x".repeat(MAX_BYTES - 1));
        assert_eq!(detect(&text, "plaintext"), None, "a cut inside a character is moved back");
    }

    #[test]
    fn it_is_written_with_its_kind() {
        assert_eq!(serde_json::to_value(DetectedIndent::Tabs).unwrap(), serde_json::json!({ "kind": "tabs" }));
        assert_eq!(
            serde_json::to_value(DetectedIndent::Spaces { width: 3 }).unwrap(),
            serde_json::json!({ "kind": "spaces", "width": 3 })
        );
    }
}
