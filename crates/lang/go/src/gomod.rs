use std::path::{Path, PathBuf};

use serde::Serialize;
use zen_project::model::ModelProblem;

use crate::modules::{Token, tokenize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleDirective {
    pub path: String,
    pub deprecated: Option<String>,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    pub path: String,
    pub version: String,
    pub indirect: bool,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replace {
    pub path: String,
    pub version: Option<String>,
    pub new_path: String,
    pub new_version: Option<String>,
    pub line: u32,
}

impl Replace {
    pub fn is_local(&self) -> bool {
        self.new_version.is_none()
    }

    pub fn matches(&self, path: &str, version: &str) -> bool {
        self.path == path && self.version.as_deref().is_none_or(|own| own == version)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exclude {
    pub path: String,
    pub version: String,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Retract {
    pub low: String,
    pub high: String,
    pub rationale: Option<String>,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Godebug {
    pub key: String,
    pub value: String,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDirective {
    pub path: String,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoMod {
    pub module: Option<ModuleDirective>,
    pub go: Option<String>,
    pub toolchain: Option<String>,
    pub go_line: Option<u32>,
    pub toolchain_line: Option<u32>,
    pub requires: Vec<Requirement>,
    pub replaces: Vec<Replace>,
    pub excludes: Vec<Exclude>,
    pub retracts: Vec<Retract>,
    pub godebugs: Vec<Godebug>,
    pub tools: Vec<PathDirective>,
    pub ignores: Vec<PathDirective>,
    pub problems: Vec<ModelProblem>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoWork {
    pub go: Option<String>,
    pub toolchain: Option<String>,
    pub go_line: Option<u32>,
    pub toolchain_line: Option<u32>,
    pub uses: Vec<PathDirective>,
    pub replaces: Vec<Replace>,
    pub godebugs: Vec<Godebug>,
    pub problems: Vec<ModelProblem>,
}

const GO_MOD_DIRECTIVES: &[&str] =
    &["module", "go", "toolchain", "require", "replace", "exclude", "retract", "godebug", "tool", "ignore"];

const GO_WORK_DIRECTIVES: &[&str] = &["go", "toolchain", "use", "replace", "godebug"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArgKind {
    Word,
    Quoted,
    Punct,
}

#[derive(Clone, Debug)]
struct Arg {
    text: String,
    kind: ArgKind,
}

#[derive(Debug, Default)]
struct Line {
    number: u32,
    args: Vec<Arg>,
    comment: Option<String>,
}

#[derive(Debug)]
struct Entry {
    directive: String,
    args: Vec<Arg>,
    line: u32,
    comments: Vec<String>,
    suffix: Option<String>,
}

struct Problems<'a> {
    file: &'a Path,
    list: Vec<ModelProblem>,
}

impl Problems<'_> {
    fn add(&mut self, line: u32, message: impl Into<String>) {
        self.list.push(ModelProblem { file: self.file.to_owned(), line, message: message.into(), warning: false });
    }
}

pub fn parse_go_mod(file: &Path, text: &str) -> GoMod {
    let mut problems = Problems { file, list: Vec::new() };
    let mut parsed = GoMod::default();
    for entry in entries(text, GO_MOD_DIRECTIVES, &mut problems) {
        let line = entry.line;
        match entry.directive.as_str() {
            "module" => match single(&entry.args) {
                Some(path) if parsed.module.is_none() => {
                    let deprecated = deprecation(&entry.comments);
                    parsed.module = Some(ModuleDirective { path: path.to_owned(), deprecated, line });
                }
                Some(_) => problems.add(line, "repeated module statement"),
                None => problems.add(line, "usage: module module/path"),
            },
            "go" => {
                go_version(&entry, &mut parsed.go, &mut problems);
                parsed.go_line = parsed.go_line.or(parsed.go.as_ref().map(|_| entry.line));
            }
            "toolchain" => {
                toolchain(&entry, &mut parsed.toolchain, &mut problems);
                parsed.toolchain_line = parsed.toolchain_line.or(parsed.toolchain.as_ref().map(|_| entry.line));
            }
            "require" => match pair(&entry.args) {
                Some((path, version)) => {
                    check_version(version, line, &mut problems);
                    let indirect = entry
                        .suffix
                        .as_deref()
                        .is_some_and(|suffix| suffix == "indirect" || suffix.starts_with("indirect;"));
                    parsed.requires.push(Requirement {
                        path: path.to_owned(),
                        version: version.to_owned(),
                        indirect,
                        line,
                    });
                }
                None => problems.add(line, "usage: require module/path v1.2.3"),
            },
            "exclude" => match pair(&entry.args) {
                Some((path, version)) => {
                    check_version(version, line, &mut problems);
                    parsed.excludes.push(Exclude { path: path.to_owned(), version: version.to_owned(), line });
                }
                None => problems.add(line, "usage: exclude module/path v1.2.3"),
            },
            "replace" => replace(&entry, &mut parsed.replaces, &mut problems),
            "retract" => match retract_range(&entry.args) {
                Some((low, high)) => {
                    check_version(&low, line, &mut problems);
                    check_version(&high, line, &mut problems);
                    let rationale = (!entry.comments.is_empty()).then(|| entry.comments.join("\n"));
                    parsed.retracts.push(Retract { low, high, rationale, line });
                }
                None => problems.add(line, "usage: retract version or retract [low, high]"),
            },
            "godebug" => godebug(&entry, &mut parsed.godebugs, &mut problems),
            "tool" => path_directive(&entry, "usage: tool module/path/to/tool", &mut parsed.tools, &mut problems),
            "ignore" => {
                path_directive(&entry, "usage: ignore ./relative/directory", &mut parsed.ignores, &mut problems)
            }
            _ => {}
        }
    }
    if parsed.module.is_none() {
        problems.add(1, "no module directive");
    }
    problems.list.sort_by_key(|problem| problem.line);
    parsed.problems = problems.list;
    parsed
}

pub fn parse_go_work(file: &Path, text: &str) -> GoWork {
    let mut problems = Problems { file, list: Vec::new() };
    let mut parsed = GoWork::default();
    for entry in entries(text, GO_WORK_DIRECTIVES, &mut problems) {
        match entry.directive.as_str() {
            "go" => {
                go_version(&entry, &mut parsed.go, &mut problems);
                parsed.go_line = parsed.go_line.or(parsed.go.as_ref().map(|_| entry.line));
            }
            "toolchain" => {
                toolchain(&entry, &mut parsed.toolchain, &mut problems);
                parsed.toolchain_line = parsed.toolchain_line.or(parsed.toolchain.as_ref().map(|_| entry.line));
            }
            "use" => path_directive(&entry, "usage: use local/dir", &mut parsed.uses, &mut problems),
            "replace" => replace(&entry, &mut parsed.replaces, &mut problems),
            "godebug" => godebug(&entry, &mut parsed.godebugs, &mut problems),
            _ => {}
        }
    }
    problems.list.sort_by_key(|problem| problem.line);
    parsed.problems = problems.list;
    parsed
}

pub fn is_directory_path(path: &str) -> bool {
    path == "."
        || path == ".."
        || ["./", "../", "/", ".\\", "..\\", "\\"].iter().any(|prefix| path.starts_with(prefix))
        || PathBuf::from(path).is_absolute()
        || path.as_bytes().get(1..3).is_some_and(|rest| rest[0] == b':' && matches!(rest[1], b'\\' | b'/'))
}

fn lines(text: &str, problems: &mut Problems<'_>) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut current = Line { number: 1, ..Line::default() };
    let mut number = 1;
    for (token, start, end) in tokenize(text) {
        let word = &text[start..end];
        match token {
            Token::Space => {}
            Token::Newline => {
                number += 1;
                lines.push(std::mem::replace(&mut current, Line { number, ..Line::default() }));
            }
            Token::Comment => current.comment = Some(word[2..].trim().to_owned()),
            Token::Quoted => {
                let text = unquote(word).unwrap_or_else(|message| {
                    problems.add(number, message);
                    String::new()
                });
                number += u32::try_from(word.matches('\n').count()).unwrap_or(0);
                current.args.push(Arg { text, kind: ArgKind::Quoted });
            }
            Token::Punct => current.args.push(Arg { text: word.to_owned(), kind: ArgKind::Punct }),
            Token::Word => current.args.push(Arg { text: word.to_owned(), kind: ArgKind::Word }),
        }
    }
    lines.push(current);
    lines
}

fn unquote(quoted: &str) -> Result<String, String> {
    if let Some(body) = quoted.strip_prefix('`') {
        return body.strip_suffix('`').map(str::to_owned).ok_or_else(|| "unterminated raw string".to_owned());
    }
    let body = quoted
        .strip_prefix('"')
        .and_then(|body| body.strip_suffix('"'))
        .filter(|_| quoted.len() >= 2)
        .ok_or_else(|| "unterminated string".to_owned())?;
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let escaped = match chars.next() {
            Some('n') => '\n',
            Some('t') => '\t',
            Some('r') => '\r',
            Some('a') => '\u{7}',
            Some('b') => '\u{8}',
            Some('f') => '\u{c}',
            Some('v') => '\u{b}',
            Some(c @ ('\\' | '"' | '\'')) => c,
            Some(c) => return Err(format!("invalid escape \\{c} in string")),
            None => return Err("unterminated string".to_owned()),
        };
        out.push(escaped);
    }
    Ok(out)
}

fn entries(text: &str, allowed: &[&str], problems: &mut Problems<'_>) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut before: Vec<String> = Vec::new();
    let mut block: Option<(String, u32, Vec<String>)> = None;
    for line in lines(text, problems) {
        if line.args.is_empty() {
            match line.comment {
                Some(comment) => before.push(comment),
                None => before.clear(),
            }
            continue;
        }
        let mut comments = std::mem::take(&mut before);
        comments.extend(line.comment.iter().cloned());
        if let Some((directive, _, block_comments)) = &block {
            if line.args.len() == 1 && is_punct(&line.args[0], ")") {
                block = None;
                continue;
            }
            let comments = if comments.is_empty() { block_comments.clone() } else { comments };
            entries.push(Entry {
                directive: directive.clone(),
                args: line.args,
                line: line.number,
                comments,
                suffix: line.comment,
            });
            continue;
        }
        let first = &line.args[0];
        if first.kind != ArgKind::Word || !allowed.contains(&first.text.as_str()) {
            let what = if first.kind == ArgKind::Punct { "unexpected" } else { "unknown directive:" };
            problems.add(line.number, format!("{what} {}", first.text));
            continue;
        }
        let directive = first.text.clone();
        let rest = &line.args[1..];
        if rest.first().is_some_and(|arg| is_punct(arg, "(")) {
            if rest.len() == 2 && is_punct(&rest[1], ")") {
                continue;
            }
            if matches!(directive.as_str(), "go" | "toolchain") {
                problems.add(line.number, format!("{directive} does not take a block"));
            } else if rest.len() > 1 {
                problems.add(line.number, format!("unexpected text after ( in {directive} block"));
            }
            block = Some((directive, line.number, comments));
            continue;
        }
        entries.push(Entry { directive, args: rest.to_vec(), line: line.number, comments, suffix: line.comment });
    }
    if let Some((directive, number, _)) = block {
        problems.add(number, format!("unterminated {directive} block: missing )"));
    }
    entries
}

fn is_punct(arg: &Arg, text: &str) -> bool {
    arg.kind == ArgKind::Punct && arg.text == text
}

fn plain(args: &[Arg]) -> Option<Vec<&str>> {
    args.iter().map(|arg| (arg.kind != ArgKind::Punct).then_some(arg.text.as_str())).collect()
}

fn single(args: &[Arg]) -> Option<&str> {
    match plain(args)?.as_slice() {
        [one] => Some(one),
        _ => None,
    }
}

fn pair(args: &[Arg]) -> Option<(&str, &str)> {
    match plain(args)?.as_slice() {
        [first, second] => Some((first, second)),
        _ => None,
    }
}

fn check_version(version: &str, line: u32, problems: &mut Problems<'_>) {
    if !version.starts_with('v') || !version[1..].starts_with(|c: char| c.is_ascii_digit()) {
        problems.add(line, format!("invalid version {version}: must be of the form v1.2.3"));
    }
}

fn go_version(entry: &Entry, slot: &mut Option<String>, problems: &mut Problems<'_>) {
    let Some(version) = single(&entry.args) else {
        problems.add(entry.line, format!("usage: {} 1.23", entry.directive));
        return;
    };
    let valid = version.starts_with(|c: char| c.is_ascii_digit())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
    if !valid {
        problems.add(entry.line, format!("invalid go version {version}: must match format 1.23.0"));
    } else if slot.is_some() {
        problems.add(entry.line, "repeated go statement");
    } else {
        *slot = Some(version.to_owned());
    }
}

fn toolchain(entry: &Entry, slot: &mut Option<String>, problems: &mut Problems<'_>) {
    let Some(name) = single(&entry.args) else {
        problems.add(entry.line, "usage: toolchain go1.23.0");
        return;
    };
    let valid =
        name == "default" || name.strip_prefix("go").is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    if !valid {
        problems.add(entry.line, format!("invalid toolchain name {name}: must be of the form go1.23.0"));
    } else if slot.is_some() {
        problems.add(entry.line, "repeated toolchain statement");
    } else {
        *slot = Some(name.to_owned());
    }
}

fn replace(entry: &Entry, list: &mut Vec<Replace>, problems: &mut Problems<'_>) {
    let usage = "usage: replace module/path [v1.2.3] => other/module v1.4 or replace module/path [v1.2.3] => ../local/directory";
    let Some(arrow) = entry.args.iter().position(|arg| is_punct(arg, "=>")) else {
        problems.add(entry.line, usage);
        return;
    };
    let (Some(old), Some(new)) = (plain(&entry.args[..arrow]), plain(&entry.args[arrow + 1..])) else {
        problems.add(entry.line, usage);
        return;
    };
    if !(1..=2).contains(&old.len()) || !(1..=2).contains(&new.len()) {
        problems.add(entry.line, usage);
        return;
    }
    if let Some(version) = old.get(1) {
        check_version(version, entry.line, problems);
    }
    match new.get(1) {
        None if !is_directory_path(new[0]) => {
            problems.add(
                entry.line,
                "replacement module without version must be directory path (rooted or starting with ./ or ../)",
            );
            return;
        }
        Some(_) if is_directory_path(new[0]) => {
            problems.add(entry.line, format!("replacement module directory path {} cannot have version", new[0]));
            return;
        }
        Some(version) => check_version(version, entry.line, problems),
        None => {}
    }
    list.push(Replace {
        path: old[0].to_owned(),
        version: old.get(1).map(|version| (*version).to_owned()),
        new_path: new[0].to_owned(),
        new_version: new.get(1).map(|version| (*version).to_owned()),
        line: entry.line,
    });
}

fn retract_range(args: &[Arg]) -> Option<(String, String)> {
    if let Some(version) = single(args) {
        return Some((version.to_owned(), version.to_owned()));
    }
    match args {
        [open, low, comma, high, close]
            if is_punct(open, "[")
                && is_punct(comma, ",")
                && is_punct(close, "]")
                && low.kind != ArgKind::Punct
                && high.kind != ArgKind::Punct =>
        {
            Some((low.text.clone(), high.text.clone()))
        }
        _ => None,
    }
}

fn godebug(entry: &Entry, list: &mut Vec<Godebug>, problems: &mut Problems<'_>) {
    let setting = single(&entry.args).and_then(|word| word.split_once('='));
    match setting {
        Some((key, value)) if !key.is_empty() && !value.is_empty() => {
            list.push(Godebug { key: key.to_owned(), value: value.to_owned(), line: entry.line });
        }
        _ => problems.add(entry.line, "usage: godebug key=value"),
    }
}

fn path_directive(entry: &Entry, usage: &str, list: &mut Vec<PathDirective>, problems: &mut Problems<'_>) {
    match single(&entry.args) {
        Some(path) if !path.is_empty() => list.push(PathDirective { path: path.to_owned(), line: entry.line }),
        _ => problems.add(entry.line, usage),
    }
}

fn deprecation(comments: &[String]) -> Option<String> {
    let start = comments.iter().position(|comment| comment.starts_with("Deprecated:"))?;
    let mut paragraph = vec![comments[start].trim_start_matches("Deprecated:").trim().to_owned()];
    paragraph.extend(comments[start + 1..].iter().take_while(|comment| !comment.is_empty()).cloned());
    Some(paragraph.join("\n").trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_strings_are_unquoted() {
        assert_eq!(unquote("\"a\\\"b\\\\c\\n\"").unwrap(), "a\"b\\c\n");
        assert_eq!(unquote("`raw\\n`").unwrap(), "raw\\n");
        assert!(unquote("\"open").is_err());
        assert!(unquote("\"").is_err());
        assert!(unquote("`open").is_err());
        assert!(unquote("\"\\q\"").is_err());
    }

    #[test]
    fn directory_paths() {
        for path in ["./x", "../x", "/abs", ".", "..", "C:\\x", ".\\x"] {
            assert!(is_directory_path(path), "{path}");
        }
        for path in ["example.com/x", "x", ".x"] {
            assert!(!is_directory_path(path), "{path}");
        }
    }
}
