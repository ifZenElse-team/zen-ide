use std::ops::Range;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A file position found in a console line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkSpan {
    /// The characters of the line the link covers, from the file to the last number.
    pub range: Range<usize>,
    /// The file as written: absolute, relative, or a bare name.
    pub file: PathBuf,
    /// The line, as written (from 1).
    pub line: u32,
    /// The column, as written (from 1), when there is one.
    pub column: Option<u32>,
}

impl LinkSpan {
    pub fn resolve(&self, base: &Path) -> PathBuf {
        if self.file.is_absolute() { self.file.clone() } else { base.join(&self.file) }
    }

    pub fn is_bare(&self) -> bool {
        self.file.components().count() == 1 && !self.file.is_absolute()
    }
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-' | '+' | '@' | '~')
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | '[' | '<' | '"' | '\'' | '=' | ',')
}

fn has_extension(path: &[char]) -> bool {
    let name = match path.iter().rposition(|&c| c == '/') {
        Some(slash) => &path[slash + 1..],
        None => path,
    };
    let Some(dot) = name.iter().rposition(|&c| c == '.') else { return false };
    let extension = &name[dot + 1..];
    dot > 0
        && extension.first().is_some_and(|c| c.is_ascii_alphabetic())
        && extension.iter().all(|c| c.is_ascii_alphanumeric())
}

fn number(chars: &[char], at: usize) -> Option<(u32, usize)> {
    let digits = chars[at..].iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let value: String = chars[at..at + digits].iter().collect();
    value.parse().ok().map(|parsed| (parsed, at + digits))
}

pub fn detect(line: &str) -> Vec<LinkSpan> {
    let chars: Vec<char> = line.chars().collect();
    let mut links = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        if !(is_path_char(chars[start]) && (start == 0 || is_boundary(chars[start - 1]))) {
            start += 1;
            continue;
        }
        let end = start + chars[start..].iter().take_while(|&&c| is_path_char(c)).count();
        let path = &chars[start..end];
        let found = (chars.get(end) == Some(&':') && has_extension(path))
            .then(|| number(&chars, end + 1))
            .flatten()
            .map(|(line, after)| match chars.get(after) {
                Some(':') => match number(&chars, after + 1) {
                    Some((column, after_column)) => (line, Some(column), after_column),
                    None => (line, None, after),
                },
                _ => (line, None, after),
            });
        match found {
            Some((line, column, after)) => {
                let file = PathBuf::from(path.iter().collect::<String>());
                links.push(LinkSpan { range: start..after, file, line, column });
                start = after;
            }
            None => start = end.max(start + 1),
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(start: usize, end: usize, file: &str, line: u32, column: Option<u32>) -> LinkSpan {
        LinkSpan { range: start..end, file: PathBuf::from(file), line, column }
    }

    #[test]
    fn file_positions_are_found() {
        let cases: Vec<(&str, Vec<LinkSpan>)> = vec![
            ("main.go:12:5: undefined: x", vec![link(0, 12, "main.go", 12, Some(5))]),
            ("./cmd/app/main.go:7", vec![link(0, 19, "./cmd/app/main.go", 7, None)]),
            ("../lib/a.go:3:9: bad", vec![link(0, 15, "../lib/a.go", 3, Some(9))]),
            ("\t/Users/me/app/main.go:123 +0x1d", vec![link(1, 26, "/Users/me/app/main.go", 123, None)]),
            ("    foo_test.go:42: expected 1, got 2", vec![link(4, 18, "foo_test.go", 42, None)]),
            (
                "pkg/a.go:1:2 and pkg/b.go:3",
                vec![link(0, 12, "pkg/a.go", 1, Some(2)), link(17, 27, "pkg/b.go", 3, None)],
            ),
            ("panic at (x.go:10)", vec![link(10, 17, "x.go", 10, None)]),
            ("see http://example.com:80/x", vec![]),
            ("go version go1.22.3:4", vec![]),
            ("no position main.go here", vec![]),
            ("time 12:30:00", vec![]),
            ("ok  \texample.com/app\t0.012s", vec![]),
            ("файл.go:5", vec![link(0, 9, "файл.go", 5, None)]),
        ];
        for (text, expected) in cases {
            assert_eq!(detect(text), expected, "{text:?}");
        }
    }

    #[test]
    fn links_resolve_against_the_run_folder() {
        let base = Path::new("/project/cmd");
        assert_eq!(detect("./main.go:1")[0].resolve(base), PathBuf::from("/project/cmd/./main.go"));
        assert_eq!(detect("/abs/x.go:1")[0].resolve(base), PathBuf::from("/abs/x.go"));
        assert!(detect("x.go:1")[0].is_bare());
        assert!(!detect("pkg/x.go:1")[0].is_bare());
    }
}
