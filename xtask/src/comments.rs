use std::path::{Path, PathBuf};
use std::process::Command;

const DOC_DERIVES: [&str; 5] = ["JsonSchema", "Parser", "Args", "Subcommand", "ValueEnum"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Code,
    Str,
    Raw(usize),
    Block(usize),
}

struct Line {
    start: State,
    comment: Option<usize>,
    block: bool,
    depth: i32,
}

pub(crate) fn run() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut problems = Vec::new();
    for file in sources(&root)? {
        let text = std::fs::read_to_string(root.join(&file)).map_err(|error| format!("{}: {error}", file.display()))?;
        for (line, problem) in check(&text) {
            problems.push(format!("{}:{line}: {problem}", file.display()));
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    for problem in &problems {
        eprintln!("{problem}");
    }
    Err(format!("{} comment(s) break the rule in docs/adr/0194-comments.md", problems.len()))
}

fn sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard", "*.rs"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err("git ls-files failed".to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().map(PathBuf::from).collect())
}

fn check(text: &str) -> Vec<(usize, &'static str)> {
    let lines: Vec<&str> = text.lines().collect();
    let info = lex(&lines);
    let verbatim = verbatim(&lines, &info);
    let mut problems = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let (line, at) = (lines[index], &info[index]);
        let trimmed = line.trim_start();
        let whole = at.start == State::Code && at.comment == Some(line.len() - trimmed.len());
        if at.block {
            problems.push((index + 1, "a block comment"));
        }
        if whole && trimmed.starts_with("///") && !trimmed.starts_with("////") {
            let mut end = index;
            while end < lines.len() && info[end].start == State::Code && is_doc(lines[end]) {
                end += 1;
            }
            if !(index..end).all(|line| verbatim[line]) {
                problems.push((index + 1, "a doc comment outside a command schema or a clap type"));
            }
            index = end;
            continue;
        }
        if whole && trimmed.starts_with("// WHY: ") {
            let comment_at = |line: usize| {
                lines.get(line).is_some_and(|text| {
                    let text_trimmed = text.trim_start();
                    info[line].start == State::Code && info[line].comment == Some(text.len() - text_trimmed.len())
                })
            };
            if (index > 0 && comment_at(index - 1)) || comment_at(index + 1) {
                problems.push((index + 1, "a `// WHY:` next to another comment: it is one line on its own"));
            }
            index += 1;
            continue;
        }
        if let Some(column) = at.comment
            && !line[column..].starts_with("// SAFETY:")
        {
            problems.push((index + 1, "a comment"));
        }
        index += 1;
    }
    problems
}

fn is_doc(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("///") && !trimmed.starts_with("////")
}

fn lex(lines: &[&str]) -> Vec<Line> {
    let mut out = Vec::with_capacity(lines.len());
    let (mut state, mut depth) = (State::Code, 0);
    for line in lines {
        let chars: Vec<char> = line.chars().collect();
        let mut info = Line { start: state, comment: None, block: matches!(state, State::Block(_)), depth };
        let mut i = 0;
        let mut byte = 0;
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            match state {
                State::Code => {
                    if c == '/' && next == Some('/') {
                        info.comment = Some(byte);
                        break;
                    }
                    if c == '/' && next == Some('*') {
                        state = State::Block(1);
                        info.block = true;
                        i += 2;
                        byte += 2;
                        continue;
                    }
                    if let Some(skip) = raw_start(&chars, i) {
                        state = State::Raw(skip.1);
                        byte += chars[i..i + skip.0].iter().map(|c| c.len_utf8()).sum::<usize>();
                        i += skip.0;
                        continue;
                    }
                    if c == '"' {
                        state = State::Str;
                    } else if c == '\'' {
                        if let Some(length) = char_literal(&chars, i) {
                            byte += chars[i..i + length].iter().map(|c| c.len_utf8()).sum::<usize>();
                            i += length;
                            continue;
                        }
                    } else if c == '{' {
                        depth += 1;
                    } else if c == '}' {
                        depth -= 1;
                    }
                }
                State::Str => {
                    if c == '\\' {
                        byte += c.len_utf8() + next.map_or(0, char::len_utf8);
                        i += 2;
                        continue;
                    }
                    if c == '"' {
                        state = State::Code;
                    }
                }
                State::Raw(hashes) => {
                    if c == '"' && chars[i + 1..].iter().take(hashes).filter(|&&h| h == '#').count() == hashes {
                        state = State::Code;
                        byte += 1 + hashes;
                        i += 1 + hashes;
                        continue;
                    }
                }
                State::Block(level) => {
                    if c == '/' && next == Some('*') {
                        state = State::Block(level + 1);
                        i += 2;
                        byte += 2;
                        continue;
                    }
                    if c == '*' && next == Some('/') {
                        state = if level == 1 { State::Code } else { State::Block(level - 1) };
                        i += 2;
                        byte += 2;
                        continue;
                    }
                }
            }
            byte += c.len_utf8();
            i += 1;
        }
        out.push(info);
    }
    out
}

fn raw_start(chars: &[char], at: usize) -> Option<(usize, usize)> {
    if at > 0 && (chars[at - 1].is_alphanumeric() || chars[at - 1] == '_') {
        return None;
    }
    let mut i = at;
    if matches!(chars.get(i), Some('b' | 'c')) {
        i += 1;
    }
    if chars.get(i) != Some(&'r') {
        return None;
    }
    i += 1;
    let hashes = chars[i..].iter().take_while(|&&c| c == '#').count();
    i += hashes;
    (chars.get(i) == Some(&'"')).then_some((i + 1 - at, hashes))
}

fn char_literal(chars: &[char], at: usize) -> Option<usize> {
    match chars.get(at + 1)? {
        '\\' => chars[at + 2..].iter().position(|&c| c == '\'').map(|end| end + 3),
        '\'' => None,
        _ => (chars.get(at + 2) == Some(&'\'')).then_some(3),
    }
}

fn verbatim(lines: &[&str], info: &[Line]) -> Vec<bool> {
    let mut keep = vec![false; lines.len()];
    let mut bodies: Vec<i32> = Vec::new();
    let mut pending = false;
    let mut index = 0;
    while index < lines.len() {
        let at = &info[index];
        while bodies.last().is_some_and(|&depth| at.depth <= depth) {
            bodies.pop();
        }
        if !bodies.is_empty() {
            keep[index] = true;
        }
        let trimmed = lines[index].trim();
        if at.start == State::Code && trimmed.starts_with("#[derive(") {
            let mut end = index;
            let mut text = trimmed.to_owned();
            while text.matches('(').count() > text.matches(')').count() && end + 1 < lines.len() {
                end += 1;
                text.push_str(lines[end].trim());
            }
            if DOC_DERIVES.iter().any(|derive| has_word(&text, derive)) {
                pending = true;
                let mut above = index;
                while above > 0 {
                    let previous = lines[above - 1].trim();
                    if !(is_doc(previous) || previous.starts_with("#[") || previous.ends_with(")]")) {
                        break;
                    }
                    above -= 1;
                    keep[above] = true;
                }
            }
            let inside = !bodies.is_empty();
            for kept in &mut keep[index..=end] {
                *kept |= inside;
            }
            index = end + 1;
            continue;
        }
        if pending && at.start == State::Code && is_type(trimmed) {
            pending = false;
            if trimmed.contains('{') {
                bodies.push(at.depth);
            }
        }
        index += 1;
    }
    keep
}

fn is_type(line: &str) -> bool {
    let rest = line.strip_prefix("pub").map_or(line, |rest| {
        let rest = rest.trim_start();
        rest.strip_prefix('(').and_then(|inner| inner.split_once(')')).map_or(rest, |(_, after)| after.trim_start())
    });
    rest.starts_with("struct ") || rest.starts_with("enum ")
}

fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::check;

    fn problems(text: &str) -> Vec<usize> {
        check(text).into_iter().map(|(line, _)| line).collect()
    }

    #[test]
    fn code_without_comments_and_safety_pass() {
        let text = "pub fn thing() {\n    // SAFETY: nothing unsafe.\n    let _ = 1;\n}\n";
        assert!(problems(text).is_empty());
    }

    #[test]
    fn plain_trailing_block_and_doc_comments_fail() {
        let text = "//! The crate.\n/// A thing.\npub fn a() {\n    // why\n    let x = 1; // what\n    /* old */\n}\n";
        assert_eq!(problems(text), vec![1, 2, 4, 5, 6]);
    }

    #[test]
    fn one_line_why_on_its_own_passes() {
        let ok = "fn a() {\n    // WHY: the handle is not Send, so each call opens its own.\n    let _ = 1;\n}\n";
        assert!(problems(ok).is_empty());
        let two = "fn a() {\n    // WHY: one.\n    // and more.\n    let _ = 1;\n}\n";
        assert_eq!(problems(two), vec![2, 3]);
        let trailing = "fn a() {\n    let _ = 1; // WHY: no.\n}\n";
        assert_eq!(problems(trailing), vec![2]);
    }

    #[test]
    fn docs_of_schema_types_pass() {
        let schema = "/// What an agent reads.\n/// More of it.\n#[derive(Debug, JsonSchema)]\npub struct P {\n    /// A field,\n    /// described at length.\n    pub a: u32,\n}\n";
        assert!(problems(schema).is_empty());
        let after =
            "#[derive(Debug, JsonSchema)]\npub struct P {\n    pub a: u32,\n}\n/// Not a schema.\npub fn f() {}\n";
        assert_eq!(problems(after), vec![5]);
    }

    #[test]
    fn comments_inside_strings_are_text() {
        let text = "const A: &str = \"// not a comment\";\nconst B: &str = r#\"\n// region\n/* text */\n\"#;\nconst C: char = '\"';\n";
        assert!(problems(text).is_empty());
    }
}
