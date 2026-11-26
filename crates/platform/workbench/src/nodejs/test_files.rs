use std::path::Path;

const SCRIPTS: [&str; 8] = ["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub line: u32,
    pub path: Vec<String>,
    pub suite: bool,
}

pub fn is_test_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return false };
    let Some((stem, extension)) = name.rsplit_once('.') else { return false };
    SCRIPTS.contains(&extension)
        && (stem.ends_with(".test")
            || stem.ends_with(".spec")
            || path.components().any(|part| part.as_os_str() == "__tests__"))
}

fn call_title(line: &str) -> Option<(&'static str, String)> {
    let line = line.trim_start();
    let (kind, rest) = [("describe", "describe"), ("it", "it"), ("test", "test")]
        .into_iter()
        .find_map(|(word, kind)| line.strip_prefix(word).map(|rest| (kind, rest)))?;
    let rest = match rest.strip_prefix('.') {
        Some(modifier) => {
            let (word, rest) = modifier.split_once('(')?;
            if !["only", "skip", "todo", "concurrent", "fails"].contains(&word) {
                return None;
            }
            rest
        }
        None => rest.trim_start().strip_prefix('(')?,
    };
    let rest = rest.trim_start();
    let quote = rest.chars().next().filter(|quote| ['\'', '"', '`'].contains(quote))?;
    let mut title = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => title.extend(chars.next()),
            '$' if quote == '`' => return None,
            c if c == quote => return Some((kind, title)),
            c => title.push(c),
        }
    }
    None
}

fn depth_change(line: &str) -> i32 {
    let mut change = 0;
    let mut quote = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"' | '`') => quote = Some(c),
            (None, '/') if chars.peek() == Some(&'/') => break,
            (None, '{') => change += 1,
            (None, '}') => change -= 1,
            _ => {}
        }
    }
    change
}

pub fn find_tests(text: &str) -> Vec<Found> {
    let mut found = Vec::new();
    let mut suites: Vec<(String, i32)> = Vec::new();
    let mut depth = 0;
    for (index, line) in text.lines().enumerate() {
        while suites.last().is_some_and(|(_, at)| depth <= *at) {
            suites.pop();
        }
        if let Some((kind, title)) = call_title(line) {
            let mut path: Vec<String> = suites.iter().map(|(name, _)| name.clone()).collect();
            path.push(title.clone());
            let suite = kind == "describe";
            found.push(Found { line: index as u32, path, suite });
            if suite {
                suites.push((title, depth));
            }
        }
        depth += depth_change(line);
    }
    found
}

pub fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\^$.*+?()[]{}|/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"import { add } from './add';

describe('math', () => {
  it('adds', () => {
    expect({ a: add(1, 2) }).toEqual({ a: 3 });
  });

  describe("deep }", () => {
    test.only('it\'s fine', async () => {});
    it.each([1, 2])('case %d', () => {});
  });
});

test(`top`, () => {});
it(`computed ${name}`, () => {});
"#;

    #[test]
    fn suites_and_tests_are_found_with_their_nesting() {
        let found: Vec<(u32, String, bool)> =
            find_tests(FILE).into_iter().map(|test| (test.line, test.path.join(" > "), test.suite)).collect();
        assert_eq!(
            found,
            [
                (2, "math".to_owned(), true),
                (3, "math > adds".to_owned(), false),
                (7, "math > deep }".to_owned(), true),
                (8, "math > deep } > it's fine".to_owned(), false),
                (13, "top".to_owned(), false),
            ]
        );
    }

    #[test]
    fn test_files_go_by_their_name_or_folder() {
        for path in ["src/a.test.ts", "b.spec.jsx", "src/__tests__/c.js", "d.test.mjs"] {
            assert!(is_test_file(Path::new(path)), "{path}");
        }
        for path in ["src/a.ts", "test.ts", "a.test.json", "tests/c.js"] {
            assert!(!is_test_file(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn a_title_is_matched_literally() {
        assert_eq!(escaped("a (b) c.d"), r"a \(b\) c\.d");
    }
}
