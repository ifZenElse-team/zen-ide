use std::path::{Path, PathBuf};

use zen_modules::file_references::FileReferences;

use crate::{Token, tokenize};

const DIRECTIVE: &str = "//go:embed";

const MAX_FILES: usize = 2_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct GoEmbedFiles;

struct Pattern<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn patterns(line: &str, base: usize) -> Vec<Pattern<'_>> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < line.len() {
        let rest = &line[at..];
        let Some(first) = rest.chars().next() else { break };
        if first.is_whitespace() {
            at += first.len_utf8();
            continue;
        }
        let quoted = matches!(first, '"' | '`');
        let (from, to) = if quoted {
            (at + 1, rest[1..].find(first).map_or(line.len(), |close| at + 1 + close))
        } else {
            (at, rest.find(char::is_whitespace).map_or(line.len(), |space| at + space))
        };
        found.push(Pattern { start: base + from, end: base + to, text: &line[from..to] });
        at = to + usize::from(quoted);
    }
    found
}

fn class(pattern: &[char], c: char) -> Option<(bool, usize)> {
    let mut at = 1;
    let negated = pattern.get(at) == Some(&'^');
    at += usize::from(negated);
    let mut hit = false;
    let first = at;
    loop {
        let low = *pattern.get(at)?;
        if low == ']' && at > first {
            return Some((hit != negated, at + 1));
        }
        let low = if low == '\\' {
            at += 1;
            *pattern.get(at)?
        } else {
            low
        };
        at += 1;
        let mut high = low;
        if pattern.get(at) == Some(&'-') && pattern.get(at + 1).is_some_and(|next| *next != ']') {
            high = pattern[at + 1];
            at += 2;
        }
        hit |= (low..=high).contains(&c);
    }
}

fn matches(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => (0..=name.len()).any(|skip| matches(&pattern[1..], &name[skip..])),
        Some('?') => !name.is_empty() && matches(&pattern[1..], &name[1..]),
        Some('[') => name.first().is_some_and(|c| {
            class(pattern, *c).is_some_and(|(hit, used)| hit && matches(&pattern[used..], &name[1..]))
        }),
        Some('\\') => pattern.get(1).is_some() && name.first() == pattern.get(1) && matches(&pattern[2..], &name[1..]),
        Some(c) => name.first() == Some(c) && matches(&pattern[1..], &name[1..]),
    }
}

fn is_ignored(name: &str) -> bool {
    name.starts_with(['.', '_'])
}

fn sorted_entries(folder: &Path) -> Vec<std::fs::DirEntry> {
    let mut entries: Vec<std::fs::DirEntry> =
        std::fs::read_dir(folder).map(|entries| entries.flatten().collect()).unwrap_or_default();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    entries
}

fn whole_folder(folder: &Path, all: bool, found: &mut Vec<PathBuf>) {
    if found.len() >= MAX_FILES || folder.join("go.mod").is_file() {
        return;
    }
    for entry in sorted_entries(folder) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if !all && is_ignored(&name) {
            continue;
        }
        if kind.is_dir() {
            whole_folder(&entry.path(), all, found);
        } else if kind.is_file() && found.len() < MAX_FILES {
            found.push(entry.path());
        }
    }
}

fn expand(folder: &Path, parts: &[&str], all: bool, found: &mut Vec<PathBuf>) {
    let Some((part, rest)) = parts.split_first() else {
        if folder.is_dir() {
            whole_folder(folder, all, found);
        } else if folder.is_file() && found.len() < MAX_FILES {
            found.push(folder.to_owned());
        }
        return;
    };
    if !part.contains(['*', '?', '[', '\\']) {
        expand(&folder.join(part), rest, all, found);
        return;
    }
    let pattern: Vec<char> = part.chars().collect();
    for entry in sorted_entries(folder) {
        let name: Vec<char> = entry.file_name().to_string_lossy().chars().collect();
        let plain = entry.file_type().is_ok_and(|kind| kind.is_dir() || kind.is_file());
        if plain && matches(&pattern, &name) {
            expand(&entry.path(), rest, all, found);
        }
    }
}

fn files_of(folder: &Path, pattern: &str) -> Vec<PathBuf> {
    let (all, pattern) = pattern.strip_prefix("all:").map_or((false, pattern), |rest| (true, rest));
    let parts: Vec<&str> = pattern.split('/').collect();
    if pattern.is_empty() || pattern.starts_with('/') || parts.iter().any(|part| matches!(*part, "" | "." | "..")) {
        return Vec::new();
    }
    let mut found = Vec::new();
    expand(folder, &parts, all, &mut found);
    found
}

fn embed_line(text: &str, byte: usize) -> Option<(usize, usize)> {
    tokenize(text).into_iter().find_map(|(token, start, end)| {
        let line = &text[start..end];
        let directive = token == Token::Comment
            && (start == 0 || text[..start].ends_with('\n'))
            && line.strip_prefix(DIRECTIVE).is_some_and(|rest| rest.starts_with([' ', '\t']));
        (directive && (start..=end).contains(&byte)).then_some((start, end))
    })
}

impl FileReferences for GoEmbedFiles {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn files_at(&self, path: &Path, text: &str, offset: usize) -> Option<Vec<PathBuf>> {
        let byte = text.char_indices().nth(offset).map_or(text.len(), |(at, _)| at);
        let (start, end) = embed_line(text, byte)?;
        let base = start + DIRECTIVE.len();
        let all = patterns(&text[base..end], base);
        let folder = path.parent()?;
        let under: Vec<&Pattern<'_>> =
            all.iter().filter(|pattern| (pattern.start..=pattern.end).contains(&byte)).collect();
        let chosen = if under.is_empty() { all.iter().collect() } else { under };
        let mut files: Vec<PathBuf> = chosen.iter().flat_map(|pattern| files_of(folder, pattern.text)).collect();
        files.sort();
        files.dedup();
        Some(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in [
            "assets/a.txt",
            "assets/b.txt",
            "assets/c.png",
            "assets/.hidden",
            "assets/_skip/x.txt",
            "assets/deep/d.txt",
            "with space.txt",
            "nested/go.mod",
            "nested/n.txt",
        ] {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        dir
    }

    fn at(dir: &tempfile::TempDir, source: &str, needle: &str) -> Option<Vec<String>> {
        let offset = source[..source.find(needle).unwrap()].chars().count();
        let files = GoEmbedFiles.files_at(&dir.path().join("main.go"), source, offset)?;
        let name = |file: &PathBuf| file.strip_prefix(dir.path()).unwrap().to_string_lossy().into_owned();
        Some(files.iter().map(name).collect())
    }

    #[test]
    fn a_pattern_lists_the_files_it_embeds() {
        let dir = fixture();
        let source = "package p\n\n// Ярлык.\n//go:embed assets/*.txt\nvar files embed.FS\n";
        assert_eq!(at(&dir, source, "assets/").unwrap(), ["assets/a.txt", "assets/b.txt"]);
    }

    #[test]
    fn a_folder_embeds_what_go_embeds_and_all_takes_the_hidden_files_too() {
        let dir = fixture();
        assert_eq!(
            at(&dir, "//go:embed assets\n", "assets").unwrap(),
            ["assets/a.txt", "assets/b.txt", "assets/c.png", "assets/deep/d.txt"]
        );
        let all = at(&dir, "//go:embed all:assets\n", "all:").unwrap();
        assert!(all.contains(&"assets/.hidden".to_owned()) && all.contains(&"assets/_skip/x.txt".to_owned()));
        assert_eq!(at(&dir, "//go:embed nested\n", "nested").unwrap(), Vec::<String>::new(), "another module");
    }

    #[test]
    fn the_pattern_at_the_caret_counts_and_the_directive_itself_lists_them_all() {
        let dir = fixture();
        let source = "//go:embed assets/a.txt \"with space.txt\" assets/[bc].*\n";
        assert_eq!(at(&dir, source, "with").unwrap(), ["with space.txt"]);
        assert_eq!(at(&dir, source, "[bc]").unwrap(), ["assets/b.txt", "assets/c.png"]);
        assert_eq!(at(&dir, source, "go:embed").unwrap().len(), 4);
    }

    #[test]
    fn patterns_cannot_leave_the_folder_and_other_lines_are_left_to_the_language_server() {
        let dir = fixture();
        assert_eq!(at(&dir, "//go:embed ../x\n", "../").unwrap(), Vec::<String>::new());
        assert_eq!(at(&dir, "//go:embed /etc/hosts\n", "/etc").unwrap(), Vec::<String>::new());
        assert_eq!(at(&dir, "package p // go:embed x\n", "package"), None);
        assert_eq!(at(&dir, "  //go:embed assets\n", "assets"), None);
        assert_eq!(at(&dir, "var s = `\n//go:embed assets\n`\n", "assets"), None);
    }
}
