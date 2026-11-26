use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use crate::Screen;

pub const MAX_LINE_CHARS: usize = 4096;

pub const MAX_LINKS: usize = 64;

pub const MAX_WRAPPED_ROWS: usize = 32;

const SCHEMES: [&str; 9] = ["https", "http", "file", "ftp", "ftps", "ws", "wss", "ssh", "git"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkKind {
    Url,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Link {
    pub kind: LinkKind,
    pub span: Range<usize>,
    pub text: String,
    pub target: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

pub fn find(text: &str) -> Vec<Link> {
    let chars: Vec<char> = text.chars().take(MAX_LINE_CHARS).collect();
    find_chars(&chars)
}

pub fn parse(text: &str) -> Option<Link> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let count = trimmed.chars().count();
    if let Some(link) = find(trimmed).into_iter().find(|link| link.span == (0..count)) {
        return Some(link);
    }
    let (target, line, column) = split_position(trimmed);
    Some(Link {
        kind: LinkKind::File,
        span: 0..count,
        text: trimmed.to_owned(),
        target: target.to_owned(),
        line,
        column,
    })
}

pub fn resolve(target: &str, bases: &[PathBuf]) -> Option<PathBuf> {
    let exists = |path: &Path| std::fs::metadata(path).is_ok();
    let path = match target.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME")?).join(rest),
        None => PathBuf::from(target),
    };
    if path.is_absolute() {
        return exists(&path).then(|| clean(&path));
    }
    let undiffed = target.strip_prefix("a/").or_else(|| target.strip_prefix("b/"));
    let relative = std::iter::once(path).chain(undiffed.map(PathBuf::from));
    let candidates: Vec<PathBuf> = relative.collect();
    candidates
        .iter()
        .flat_map(|relative| bases.iter().map(move |base| base.join(relative)))
        .find(|path| exists(path))
        .map(|path| clean(&path))
}

fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenLink {
    pub link: Link,
    pub cells: Vec<(usize, Range<usize>)>,
}

impl ScreenLink {
    pub fn covers(&self, row: usize, col: usize) -> bool {
        self.cells.iter().any(|(at, cols)| *at == row && cols.contains(&col))
    }
}

struct LogicalLine {
    chars: Vec<char>,
    cells: Vec<(usize, usize, usize)>,
}

impl Screen {
    fn logical_rows(&self, row: usize) -> Range<usize> {
        let wraps = |row: usize| self.wrapped.get(row).copied().unwrap_or(false);
        let mut start = row;
        while start > 0 && row - start < MAX_WRAPPED_ROWS && wraps(start - 1) {
            start -= 1;
        }
        let mut end = row + 1;
        while end < self.rows() && end - start < MAX_WRAPPED_ROWS && wraps(end - 1) {
            end += 1;
        }
        start..end
    }

    fn logical_line(&self, rows: Range<usize>) -> LogicalLine {
        let mut line = LogicalLine { chars: Vec::new(), cells: Vec::new() };
        'rows: for row in rows {
            let Some(cells) = self.lines.get(row) else { continue };
            for (col, cell) in cells.iter().enumerate() {
                if cell.spacer {
                    continue;
                }
                if line.chars.len() >= MAX_LINE_CHARS {
                    break 'rows;
                }
                line.chars.push(cell.ch);
                line.cells.push((row, col, if cell.wide { 2 } else { 1 }));
            }
        }
        line
    }

    fn place(line: &LogicalLine, link: Link) -> ScreenLink {
        let mut cells: Vec<(usize, Range<usize>)> = Vec::new();
        for &(row, col, width) in &line.cells[link.span.clone()] {
            match cells.last_mut() {
                Some((at, cols)) if *at == row => cols.end = col + width,
                _ => cells.push((row, col..col + width)),
            }
        }
        ScreenLink { link, cells }
    }

    pub fn links(&self) -> Vec<ScreenLink> {
        let mut links = Vec::new();
        let mut row = 0;
        while row < self.rows() {
            let rows = self.logical_rows(row);
            row = rows.end.max(row + 1);
            let line = self.logical_line(rows);
            links.extend(find_chars(&line.chars).into_iter().map(|link| Self::place(&line, link)));
        }
        links
    }

    pub fn link_at(&self, row: usize, col: usize) -> Option<ScreenLink> {
        if row >= self.rows() {
            return None;
        }
        let line = self.logical_line(self.logical_rows(row));
        find_chars(&line.chars).into_iter().map(|link| Self::place(&line, link)).find(|link| link.covers(row, col))
    }
}

fn ends_path(ch: char) -> bool {
    ch.is_whitespace() || ch.is_control() || "\"'`<>|;,=()[]{}".contains(ch)
}

fn ends_url(ch: char) -> bool {
    ch.is_whitespace() || ch.is_control() || "\"'`<>".contains(ch)
}

fn find_chars(chars: &[char]) -> Vec<Link> {
    let chars = &chars[..chars.len().min(MAX_LINE_CHARS)];
    let mut links = Vec::new();
    let mut at = 0;
    while at < chars.len() && links.len() < MAX_LINKS {
        if ends_path(chars[at]) {
            at += 1;
            continue;
        }
        if let Some(link) = url_at(chars, at) {
            at = link.span.end;
            links.push(link);
            continue;
        }
        let mut end = at;
        while end < chars.len() && !ends_path(chars[end]) {
            end += 1;
        }
        if let Some(link) = path_at(chars, at, end) {
            at = link.span.end.max(end);
            links.push(link);
        } else {
            at = end;
        }
    }
    links
}

fn url_at(chars: &[char], at: usize) -> Option<Link> {
    if at > 0 && chars[at - 1].is_ascii_alphanumeric() {
        return None;
    }
    let mut colon = at;
    while colon < chars.len() && colon - at <= 5 && chars[colon].is_ascii_alphabetic() {
        colon += 1;
    }
    let scheme: String = chars[at..colon].iter().collect::<String>().to_ascii_lowercase();
    if !SCHEMES.contains(&scheme.as_str()) || chars.get(colon..colon + 3) != Some(&[':', '/', '/']) {
        return None;
    }
    let body = colon + 3;
    let mut end = body;
    let mut depth: i32 = 0;
    while end < chars.len() && !ends_url(chars[end]) {
        match chars[end] {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => break,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    while end > body && ".,;:!?".contains(chars[end - 1]) {
        end -= 1;
    }
    if end == body {
        return None;
    }
    let text: String = chars[at..end].iter().collect();
    if scheme == "file" {
        let rest: String = chars[body..end].iter().collect();
        let rest = rest.strip_prefix("localhost").unwrap_or(&rest);
        if !rest.starts_with('/') {
            return None;
        }
        let (path, line, column) = split_position(rest);
        return Some(Link { kind: LinkKind::File, span: at..end, text, target: percent_decode(path), line, column });
    }
    Some(Link { kind: LinkKind::Url, span: at..end, target: text.clone(), text, line: None, column: None })
}

fn path_at(chars: &[char], at: usize, end: usize) -> Option<Link> {
    let mut stop = end;
    while stop > at && ".:".contains(chars[stop - 1]) {
        stop -= 1;
    }
    let word: String = chars[at..stop].iter().collect();
    let (path, mut line, mut column) = split_position(&word);
    if !looks_like_path(path) {
        return None;
    }
    let mut span_end = stop;
    if line.is_none() {
        if let Some((found, after)) = parenthesised(chars, end) {
            (line, column) = (Some(found.0), found.1);
            span_end = after;
        } else if let Some((found, after)) = python_line(chars, at, end) {
            line = Some(found);
            span_end = after;
        }
    }
    Some(Link {
        kind: LinkKind::File,
        span: at..span_end,
        text: chars[at..span_end].iter().collect(),
        target: path.to_owned(),
        line,
        column,
    })
}

fn split_position(word: &str) -> (&str, Option<u32>, Option<u32>) {
    let number = |part: &str| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())).then(|| part.parse().ok()).flatten()
    };
    if let Some((rest, last)) = word.rsplit_once(':')
        && let Some(last) = number(last)
    {
        if let Some((path, line)) = rest.rsplit_once(':')
            && let Some(line) = number(line)
            && !path.is_empty()
        {
            return (path, Some(line), Some(last));
        }
        if !rest.is_empty() {
            return (rest, Some(last), None);
        }
    }
    (word, None, None)
}

fn parenthesised(chars: &[char], at: usize) -> Option<((u32, Option<u32>), usize)> {
    if chars.get(at) != Some(&'(') {
        return None;
    }
    let close = (at + 1..chars.len().min(at + 24)).find(|index| chars[*index] == ')')?;
    let inside: String = chars[at + 1..close].iter().collect();
    let mut parts = inside.split(',').map(str::trim);
    let line = parts.next()?.parse().ok()?;
    let column = match parts.next() {
        Some(column) => Some(column.parse().ok()?),
        None => None,
    };
    parts.next().is_none().then_some(((line, column), close + 1))
}

fn python_line(chars: &[char], at: usize, end: usize) -> Option<(u32, usize)> {
    if at == 0 || chars[at - 1] != '"' {
        return None;
    }
    let tail: String = chars[end..chars.len().min(end + 24)].iter().collect();
    let rest = tail.strip_prefix("\", line ")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let line = digits.parse().ok()?;
    Some((line, end + "\", line ".len() + digits.len()))
}

fn looks_like_path(word: &str) -> bool {
    if word.is_empty() || word.starts_with('-') || word.starts_with("//") || word.contains("://") {
        return false;
    }
    let has_letter = word.chars().any(char::is_alphabetic);
    if word.starts_with('/') || word.starts_with("./") || word.starts_with("../") || word.starts_with("~/") {
        return word.len() > 1 && has_letter;
    }
    if word.contains('/') {
        return has_letter;
    }
    let Some((name, extension)) = word.rsplit_once('.') else { return false };
    !name.is_empty()
        && (1..=12).contains(&extension.len())
        && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
        && extension.chars().any(|ch| ch.is_ascii_alphabetic())
        && name.chars().any(char::is_alphanumeric)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = text.get(index + 1..index + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cell;

    fn one(text: &str) -> Link {
        let links = find(text);
        assert_eq!(links.len(), 1, "{text}: {links:?}");
        links.into_iter().next().unwrap()
    }

    fn file(text: &str) -> (String, Option<u32>, Option<u32>, String) {
        let link = one(text);
        assert_eq!(link.kind, LinkKind::File, "{text}");
        (link.target, link.line, link.column, link.text)
    }

    #[test]
    fn urls_are_found_without_trailing_punctuation() {
        let link = one("see https://example.com/a?b=1#c.");
        assert_eq!(
            (link.kind, link.target.as_str(), link.span.clone()),
            (LinkKind::Url, "https://example.com/a?b=1#c", 4..31)
        );
        assert_eq!(one("(http://host:8080/x)").target, "http://host:8080/x");
        assert_eq!(
            one("https://en.wikipedia.org/wiki/Rust_(language)").target,
            "https://en.wikipedia.org/wiki/Rust_(language)"
        );
        assert_eq!(one("<wss://h/s>,").target, "wss://h/s");
        assert!(find("xhttps://a.b").iter().all(|link| link.kind != LinkKind::Url), "a scheme inside a word");
        assert!(find("https:// alone").is_empty());
        assert!(find("javascript://x").is_empty(), "only the known schemes");
    }

    #[test]
    fn compiler_and_test_runner_paths_carry_their_line_and_column() {
        assert_eq!(
            file("  --> src/main.rs:12:5"),
            ("src/main.rs".into(), Some(12), Some(5), "src/main.rs:12:5".into())
        );
        assert_eq!(file("./main.go:7:2: undefined: x"), ("./main.go".into(), Some(7), Some(2), "./main.go:7:2".into()));
        assert_eq!(
            file("src/app.ts(3,14): error TS2304"),
            ("src/app.ts".into(), Some(3), Some(14), "src/app.ts(3,14)".into())
        );
        assert_eq!(file("at /home/me/x.js:40"), ("/home/me/x.js".into(), Some(40), None, "/home/me/x.js:40".into()));
        assert_eq!(
            file("  File \"tool.py\", line 9, in main"),
            ("tool.py".into(), Some(9), None, "tool.py\", line 9".into())
        );
        assert_eq!(file("open ~/notes/todo.md now."), ("~/notes/todo.md".into(), None, None, "~/notes/todo.md".into()));
        assert_eq!(file("[Cargo.toml]"), ("Cargo.toml".into(), None, None, "Cargo.toml".into()));
        assert_eq!(
            file("file:///tmp/a%20b.txt:3"),
            ("/tmp/a b.txt".into(), Some(3), None, "file:///tmp/a%20b.txt:3".into())
        );
    }

    #[test]
    fn words_that_are_not_paths_are_left_alone() {
        for text in ["version 1.2.3", "127.0.0.1:8080", "ok", "--flag", "50%", "3/4", "e", "...", "a:1"] {
            assert!(find(text).is_empty(), "{text}: {:?}", find(text));
        }
        let both = find("error: a.rs:1 and https://x.y/z");
        assert_eq!(both.iter().map(|link| link.kind).collect::<Vec<_>>(), [LinkKind::File, LinkKind::Url]);
    }

    #[test]
    fn a_long_line_is_searched_only_so_far() {
        let line = "a/b.rs ".repeat(2000);
        let links = find(&line);
        assert_eq!(links.len(), MAX_LINKS);
        let far = format!("{}https://x.y", " ".repeat(MAX_LINE_CHARS));
        assert!(find(&far).is_empty());
    }

    #[test]
    fn an_agents_target_is_read_whole() {
        let link = parse(" src/lib.rs:3:1 ").unwrap();
        assert_eq!((link.target.as_str(), link.line, link.column), ("src/lib.rs", Some(3), Some(1)));
        assert_eq!(parse("https://a.b/c").unwrap().kind, LinkKind::Url);
        let bare = parse("Makefile:4").unwrap();
        assert_eq!((bare.kind, bare.target.as_str(), bare.line), (LinkKind::File, "Makefile", Some(4)));
        assert!(parse("  ").is_none());
    }

    #[test]
    fn paths_resolve_in_the_first_folder_that_has_them() {
        let dir = tempfile::tempdir().unwrap();
        let (cwd, project) = (dir.path().join("cwd"), dir.path().join("project"));
        std::fs::create_dir_all(cwd.join("src")).unwrap();
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("src/lib.rs"), "").unwrap();
        std::fs::write(cwd.join("src/main.rs"), "").unwrap();
        let bases = [cwd.clone(), project.clone()];
        assert_eq!(resolve("src/main.rs", &bases), Some(cwd.join("src/main.rs")));
        assert_eq!(resolve("./src/lib.rs", &bases), Some(project.join("src/lib.rs")));
        assert_eq!(resolve("b/src/lib.rs", &bases), Some(project.join("src/lib.rs")), "Git's diff prefix");
        assert_eq!(resolve("src/../src/lib.rs", &bases), Some(project.join("src/lib.rs")));
        assert_eq!(resolve(&cwd.join("src/main.rs").display().to_string(), &[]), Some(cwd.join("src/main.rs")));
        assert_eq!(resolve("src/none.rs", &bases), None);
    }

    fn screen(rows: &[&str], wrapped: &[bool], cols: usize) -> Screen {
        let lines = rows
            .iter()
            .map(|text| {
                let mut cells: Vec<Cell> = text.chars().map(|ch| Cell { ch, ..Cell::default() }).collect();
                cells.resize(cols, Cell::default());
                cells
            })
            .collect();
        Screen { lines, cols, wrapped: wrapped.to_vec(), ..Screen::default() }
    }

    #[test]
    fn links_on_the_screen_follow_wrapped_rows() {
        let screen = screen(&["see https://exa", "mple.com/x ok", "src/a.rs:2"], &[true, false, false], 15);
        let links = screen.links();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].link.target, "https://example.com/x");
        assert_eq!(links[0].cells, vec![(0, 4..15), (1, 0..10)]);
        assert_eq!(links[1].cells, vec![(2, 0..10)]);
        assert_eq!(screen.link_at(1, 3).map(|link| link.link.target), Some("https://example.com/x".to_owned()));
        assert!(screen.link_at(1, 12).is_none(), "\"ok\" is no link");
        assert!(screen.link_at(0, 1).is_none());
        assert_eq!(screen.link_at(2, 9).unwrap().link.line, Some(2));
        assert!(screen.link_at(9, 0).is_none());
    }

    #[test]
    fn wide_characters_take_two_cells() {
        let mut screen = screen(&["", ""], &[false, false], 20);
        let cells = &mut screen.lines[0];
        cells[0] = Cell { ch: '界', wide: true, ..Cell::default() };
        cells[1] = Cell { spacer: true, ..Cell::default() };
        for (index, ch) in " a/b.rs".chars().enumerate() {
            cells[2 + index] = Cell { ch, ..Cell::default() };
        }
        let link = screen.link_at(0, 4).unwrap();
        assert_eq!(link.cells, vec![(0, 3..9)]);
    }
}
