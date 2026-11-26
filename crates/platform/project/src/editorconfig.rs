use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::settings::{CodeStyle, FileCodeStyle};

pub const FILE_NAME: &str = ".editorconfig";

pub const UNSET: &str = "unset";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct File {
    pub root: bool,
    pub sections: Vec<Section>,
    pub problems: Vec<Problem>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub glob: String,
    pub properties: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub kind: ProblemKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    NotAProperty(String),
    UnclosedSection(String),
    OutsideSection(String),
    NoName,
    InvalidValue { name: String, value: String },
}

impl File {
    pub fn parse(text: &str) -> File {
        let mut file = File::default();
        let mut in_broken_section = false;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') {
                match line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
                    Some(glob) => file.sections.push(Section { glob: glob.to_owned(), properties: Vec::new() }),
                    None => file.problem(number, ProblemKind::UnclosedSection(line.to_owned())),
                }
                in_broken_section = !line.ends_with(']');
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                file.problem(number, ProblemKind::NotAProperty(line.to_owned()));
                continue;
            };
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim().to_ascii_lowercase());
            if name.is_empty() {
                file.problem(number, ProblemKind::NoName);
                continue;
            }
            if !is_valid(&name, &value) {
                file.problem(number, ProblemKind::InvalidValue { name, value });
                continue;
            }
            match file.sections.last_mut() {
                _ if in_broken_section => {}
                Some(_) if name == "root" => {}
                Some(section) => section.properties.push((name, value)),
                None if name == "root" => file.root = value == "true",
                None => file.problem(number, ProblemKind::OutsideSection(name)),
            }
        }
        file
    }

    fn problem(&mut self, line: usize, kind: ProblemKind) {
        self.problems.push(Problem { line, kind });
    }

    pub fn properties_for(&self, relative: &str) -> impl Iterator<Item = &(String, String)> {
        self.sections
            .iter()
            .filter(move |section| section_matches(&section.glob, relative))
            .flat_map(|section| section.properties.iter())
    }
}

fn is_number(value: &str, zero: bool) -> bool {
    value.parse::<u16>().is_ok_and(|number| zero || number > 0)
}

fn is_valid(name: &str, value: &str) -> bool {
    if value == UNSET {
        return name != "root";
    }
    let option = ij_option(name).unwrap_or_default();
    match name {
        "root" | "trim_trailing_whitespace" | "insert_final_newline" => matches!(value, "true" | "false"),
        "indent_style" => matches!(value, "tab" | "space"),
        "indent_size" => value == "tab" || value.parse::<u8>().is_ok_and(|size| size > 0),
        "tab_width" => value.parse::<u8>().is_ok_and(|size| size > 0),
        "end_of_line" => matches!(value, "lf" | "crlf" | "cr"),
        "charset" => matches!(value, "latin1" | "utf-8" | "utf-8-bom" | "utf-16be" | "utf-16le"),
        "max_line_length" => value == "off" || is_number(value, false),
        _ if option == CONTINUATION_INDENT => value.parse::<u8>().is_ok(),
        _ if option == VISUAL_GUIDES => visual_guides(value).is_some(),
        _ => true,
    }
}

pub const KNOWN: &[(&str, &[&str])] = &[
    ("root", &["true", "false"]),
    ("charset", &["utf-8", "utf-8-bom", "latin1", "utf-16be", "utf-16le"]),
    ("end_of_line", &["lf", "crlf", "cr"]),
    ("indent_size", &["2", "4", "8", "tab"]),
    ("indent_style", &["space", "tab"]),
    ("insert_final_newline", &["true", "false"]),
    ("max_line_length", &["80", "100", "120", "off"]),
    ("tab_width", &["2", "4", "8"]),
    ("trim_trailing_whitespace", &["true", "false"]),
    ("ij_continuation_indent_size", &["4", "8"]),
    ("ij_visual_guides", &["none"]),
];

pub fn known_names() -> Vec<String> {
    let mut names: Vec<String> = KNOWN.iter().map(|(name, _)| (*name).to_owned()).collect();
    let mut prefixes: Vec<&str> =
        std::iter::once("any").chain(LANGUAGE_PREFIXES.iter().map(|(_, prefix)| *prefix)).collect();
    prefixes.dedup();
    for option in [CONTINUATION_INDENT, VISUAL_GUIDES] {
        names.extend(prefixes.iter().map(|prefix| format!("ij_{prefix}_{option}")));
    }
    names
}

pub fn known_values(name: &str) -> Option<&'static [&'static str]> {
    let name = name.to_ascii_lowercase();
    let generic = ij_option(&name).map(|option| format!("ij_{option}"));
    let wanted = generic.as_deref().unwrap_or(&name);
    KNOWN.iter().find(|(known, _)| *known == wanted).map(|(_, values)| *values)
}

const CONTINUATION_INDENT: &str = "continuation_indent_size";
const VISUAL_GUIDES: &str = "visual_guides";

fn ij_option(name: &str) -> Option<&'static str> {
    let rest = name.strip_prefix("ij_")?;
    [CONTINUATION_INDENT, VISUAL_GUIDES].into_iter().find(|option| {
        rest == *option || rest.strip_suffix(option).is_some_and(|prefix| prefix.ends_with('_') && prefix.len() > 1)
    })
}

fn visual_guides(value: &str) -> Option<Vec<u16>> {
    if value.is_empty() || value == "none" {
        return Some(Vec::new());
    }
    value.split(',').map(|column| column.trim().parse::<u16>().ok().filter(|column| *column > 0)).collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub properties: BTreeMap<String, String>,
    pub files: Vec<PathBuf>,
}

pub fn lookup(root: &Path, path: &Path, mut load: impl FnMut(&Path) -> Option<Arc<File>>) -> Found {
    let mut files = Vec::new();
    let mut folder = path.parent();
    while let Some(current) = folder {
        if !current.starts_with(root) {
            break;
        }
        if let Some(file) = load(current) {
            let Ok(relative) = path.strip_prefix(current) else { break };
            let relative: Vec<String> =
                relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
            let is_root = file.root;
            files.push((current.join(FILE_NAME), file, relative.join("/")));
            if is_root {
                break;
            }
        }
        if current == root {
            break;
        }
        folder = current.parent();
    }
    let mut found = Found::default();
    for (path, file, relative) in files.iter().rev() {
        let mut matched = false;
        for (name, value) in file.properties_for(relative) {
            matched = true;
            if value == UNSET {
                found.properties.remove(name);
            } else {
                found.properties.insert(name.clone(), value.clone());
            }
        }
        if matched {
            found.files.insert(0, path.clone());
        }
    }
    found
}

pub fn properties(root: &Path, path: &Path) -> BTreeMap<String, String> {
    lookup(root, path, |folder| {
        std::fs::read_to_string(folder.join(FILE_NAME)).ok().map(|text| Arc::new(File::parse(&text)))
    })
    .properties
}

const LANGUAGE_PREFIXES: &[(&str, &str)] = &[
    ("go", "go"),
    ("rust", "rust"),
    ("json", "json"),
    ("yaml", "yaml"),
    ("toml", "toml"),
    ("markdown", "markdown"),
    ("java", "java"),
    ("kotlin", "kotlin"),
    ("javascript", "javascript"),
    ("javascriptreact", "javascript"),
    ("typescript", "typescript"),
    ("typescriptreact", "typescript"),
    ("html", "html"),
    ("css", "css"),
    ("scss", "scss"),
    ("xml", "xml"),
    ("properties", "properties"),
    ("python", "python"),
    ("sql", "sql"),
];

/// How many columns one level of indentation takes, as EditorConfig sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Indent {
    /// A number of columns.
    Columns(u8),
    /// As many as the tab size (`indent_size = tab`).
    TabSize,
}

/// Where the right margin is, as EditorConfig sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RightMargin {
    /// At this column.
    Column(u16),
    /// Nowhere (`max_line_length = off`).
    Off,
}

/// The code style the `.editorconfig` properties of a file set; what they leave out is `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Style {
    /// `indent_style`: whether indentation is written with tab characters.
    pub use_tabs: Option<bool>,
    /// `indent_size`: how many columns one level of indentation takes.
    pub indent: Option<Indent>,
    /// `tab_width`: how many columns a tab character takes.
    pub tab_size: Option<u8>,
    /// `ij_<language>_`, `ij_any_` or `ij_continuation_indent_size`.
    pub continuation_indent: Option<u8>,
    /// `max_line_length`.
    pub right_margin: Option<RightMargin>,
    /// `ij_<language>_`, `ij_any_` or `ij_visual_guides`: the columns of the extra guides.
    pub visual_guides: Option<Vec<u16>>,
}

impl Style {
    pub fn from_properties(properties: &BTreeMap<String, String>, language: &str) -> Style {
        let get = |name: &str| properties.get(name).map(String::as_str);
        let use_tabs = match get("indent_style") {
            Some("tab") => Some(true),
            Some("space") => Some(false),
            _ => None,
        };
        let indent = match get("indent_size") {
            Some("tab") => Some(Indent::TabSize),
            Some(size) => size.parse().ok().filter(|size| *size > 0).map(Indent::Columns),
            None if use_tabs == Some(true) => Some(Indent::TabSize),
            None => None,
        };
        let tab_size = get("tab_width").and_then(|size| size.parse().ok()).filter(|size| *size > 0).or(match indent {
            Some(Indent::Columns(size)) => Some(size),
            _ => None,
        });
        let right_margin = match get("max_line_length") {
            Some("off") => Some(RightMargin::Off),
            Some(column) => column.parse().ok().filter(|column| *column > 0).map(RightMargin::Column),
            None => None,
        };
        let prefix = LANGUAGE_PREFIXES.iter().find(|(id, _)| *id == language).map(|(_, prefix)| *prefix);
        let option = |option: &str| {
            prefix
                .and_then(|prefix| get(&format!("ij_{prefix}_{option}")))
                .or_else(|| get(&format!("ij_any_{option}")))
                .or_else(|| get(&format!("ij_{option}")))
        };
        Style {
            use_tabs,
            indent,
            tab_size,
            continuation_indent: option(CONTINUATION_INDENT).and_then(|size| size.parse().ok()),
            right_margin,
            visual_guides: option(VISUAL_GUIDES).and_then(visual_guides),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }

    pub fn sets_indent(&self) -> bool {
        self.use_tabs.is_some() || self.indent.is_some()
    }

    pub fn apply_to_file(&self, base: FileCodeStyle) -> FileCodeStyle {
        let tab_size = self.tab_size.unwrap_or(base.tab_size);
        let indent = match self.indent {
            Some(Indent::Columns(size)) => size,
            Some(Indent::TabSize) => tab_size,
            None => base.indent,
        };
        FileCodeStyle {
            use_tabs: self.use_tabs.unwrap_or(base.use_tabs),
            tab_size,
            indent,
            continuation_indent: self.continuation_indent.or(base.continuation_indent),
            right_margin: match self.right_margin {
                Some(RightMargin::Column(column)) => Some(column),
                Some(RightMargin::Off) => None,
                None => base.right_margin,
            },
            visual_guides: self.visual_guides.clone().unwrap_or(base.visual_guides),
        }
    }

    pub fn apply_to(&self, base: CodeStyle) -> CodeStyle {
        self.apply_to_file(FileCodeStyle::from(base)).code_style()
    }
}

pub fn section_matches(glob: &str, relative: &str) -> bool {
    let glob =
        if has_separator(glob) { glob.strip_prefix('/').unwrap_or(glob).to_owned() } else { format!("**/{glob}") };
    let tokens = tokens(&glob.chars().collect::<Vec<_>>(), true);
    let name: Vec<char> = relative.chars().collect();
    let mut start = vec![false; name.len() + 1];
    start[0] = true;
    reach(&tokens, &name, start)[name.len()]
}

fn has_separator(glob: &str) -> bool {
    let mut in_class = false;
    let mut escaped = false;
    for c in glob.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '[' => in_class = true,
            ']' => in_class = false,
            '/' if !in_class => return true,
            _ => {}
        }
    }
    false
}

#[derive(Clone, Debug)]
enum Token {
    Char(char),
    One,
    Star,
    Stars,
    Folders,
    Class { negated: bool, ranges: Vec<(char, char)> },
    Numbers(i64, i64),
    Either(Vec<Vec<Token>>),
}

fn closing(glob: &[char], open: usize, close: char) -> Option<usize> {
    let mut depth = 0;
    let mut index = open;
    while index < glob.len() {
        match glob[index] {
            '\\' => index += 1,
            '{' if close == '}' => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 || close == ']' {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn split_top(inner: &[char]) -> Vec<&[char]> {
    let mut parts = Vec::new();
    let (mut depth, mut start, mut index) = (0, 0, 0);
    while index < inner.len() {
        match inner[index] {
            '\\' => index += 1,
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&inner[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(&inner[start..]);
    parts
}

fn numbers(inner: &[char]) -> Option<(i64, i64)> {
    let inner: String = inner.iter().collect();
    let (low, high) = inner.split_once("..")?;
    Some((low.parse().ok()?, high.parse().ok()?))
}

fn class(inner: &[char]) -> Token {
    let negated = matches!(inner.first(), Some('!' | '^'));
    let mut chars = Vec::new();
    let mut index = usize::from(negated);
    while index < inner.len() {
        let escaped = inner[index] == '\\' && index + 1 < inner.len();
        index += usize::from(escaped);
        chars.push((inner[index], escaped));
        index += 1;
    }
    let mut ranges = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if at + 2 < chars.len() && chars[at + 1] == ('-', false) {
            ranges.push((chars[at].0, chars[at + 2].0));
            at += 3;
        } else {
            ranges.push((chars[at].0, chars[at].0));
            at += 1;
        }
    }
    Token::Class { negated, ranges }
}

fn tokens(glob: &[char], after_separator: bool) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut index = 0;
    while index < glob.len() {
        let at_separator = if tokens.is_empty() {
            after_separator
        } else {
            matches!(tokens.last(), Some(Token::Char('/') | Token::Folders))
        };
        match glob[index] {
            '\\' if index + 1 < glob.len() => {
                index += 1;
                tokens.push(Token::Char(glob[index]));
            }
            '?' => tokens.push(Token::One),
            '*' if glob.get(index + 1) == Some(&'*') => {
                index += 1;
                if at_separator && glob.get(index + 1) == Some(&'/') {
                    index += 1;
                    tokens.push(Token::Folders);
                } else {
                    tokens.push(Token::Stars);
                }
            }
            '*' => tokens.push(Token::Star),
            '[' => match closing(glob, index + 1, ']') {
                Some(end) if end > index + 1 && !glob[index + 1..end].contains(&'/') => {
                    tokens.push(class(&glob[index + 1..end]));
                    index = end;
                }
                _ => tokens.push(Token::Char('[')),
            },
            '{' => match closing(glob, index, '}') {
                Some(end) => {
                    let inner = &glob[index + 1..end];
                    if let Some((low, high)) = numbers(inner) {
                        tokens.push(Token::Numbers(low.min(high), low.max(high)));
                    } else {
                        let parts = split_top(inner);
                        if parts.len() > 1 {
                            let choices = parts.into_iter().map(|part| self::tokens(part, at_separator)).collect();
                            tokens.push(Token::Either(choices));
                        } else {
                            tokens.push(Token::Char('{'));
                            tokens.extend(self::tokens(inner, false));
                            tokens.push(Token::Char('}'));
                        }
                    }
                    index = end;
                }
                None => tokens.push(Token::Char('{')),
            },
            c => tokens.push(Token::Char(c)),
        }
        index += 1;
    }
    tokens
}

fn number_ends(name: &[char], at: usize, low: i64, high: i64) -> impl Iterator<Item = usize> + '_ {
    let sign = usize::from(name.get(at) == Some(&'-'));
    let digits = name[(at + sign).min(name.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
    let leading_zero = digits > 1 && name[at + sign] == '0';
    (1..=digits).filter(move |length| !leading_zero || *length == 1).filter_map(move |length| {
        let end = at + sign + length;
        let number: String = name[at..end].iter().collect();
        number.parse::<i64>().is_ok_and(|number| (low..=high).contains(&number)).then_some(end)
    })
}

fn reach(tokens: &[Token], name: &[char], mut current: Vec<bool>) -> Vec<bool> {
    for token in tokens {
        let mut next = vec![false; name.len() + 1];
        if let Token::Either(choices) = token {
            for choice in choices {
                for (end, reached) in reach(choice, name, current.clone()).into_iter().enumerate() {
                    next[end] |= reached;
                }
            }
        }
        let starts: Vec<usize> = (0..=name.len()).filter(|at| current[*at]).collect();
        for at in starts {
            let here = name.get(at).copied();
            match token {
                Token::Char(c) if here == Some(*c) => next[at + 1] = true,
                Token::One if here.is_some_and(|c| c != '/') => next[at + 1] = true,
                Token::Class { negated, ranges }
                    if here.is_some_and(|c| {
                        c != '/' && ranges.iter().any(|(low, high)| (*low..=*high).contains(&c)) != *negated
                    }) =>
                {
                    next[at + 1] = true;
                }
                Token::Star => {
                    let reach = name[at..].iter().position(|c| *c == '/').map_or(name.len(), |slash| at + slash);
                    next[at..=reach].iter_mut().for_each(|reached| *reached = true);
                }
                Token::Stars => {
                    next[at..].iter_mut().for_each(|reached| *reached = true);
                    break;
                }
                Token::Folders => {
                    next[at] = true;
                    for end in at + 1..=name.len() {
                        next[end] |= name[end - 1] == '/';
                    }
                }
                Token::Numbers(low, high) => number_ends(name, at, *low, *high).for_each(|end| next[end] = true),
                _ => {}
            }
        }
        if !next.contains(&true) {
            return next;
        }
        current = next;
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    mod conformance;

    fn parse_for(text: &str, relative: &str) -> Vec<(String, String)> {
        File::parse(text).properties_for(relative).cloned().collect()
    }

    fn style(text: &str, relative: &str, language: &str) -> Style {
        let file = File::parse(text);
        let mut properties = BTreeMap::new();
        for (name, value) in file.properties_for(relative) {
            if value == UNSET {
                properties.remove(name);
            } else {
                properties.insert(name.clone(), value.clone());
            }
        }
        Style::from_properties(&properties, language)
    }

    #[test]
    fn globs_match_as_the_specification_says() {
        assert!(section_matches("*", "main.go"));
        assert!(section_matches("*", "pkg/deep/main.go"), "a glob without a slash matches in any folder");
        assert!(section_matches("*.{go,mod}", "go.mod"));
        assert!(!section_matches("*.{go,mod}", "go.sum"));
        assert!(section_matches("*.[ch]", "a/b.h"));
        assert!(!section_matches("*.[!ch]", "b.h"));
        assert!(section_matches("*.[a-c]", "x.b"));
        assert!(section_matches("file{1..3}.txt", "file2.txt"));
        assert!(!section_matches("file{1..3}.txt", "file4.txt"));
        assert!(section_matches("v{-2..12}", "v-1"));
        assert!(section_matches("/src/*.go", "src/main.go"));
        assert!(!section_matches("/src/*.go", "src/sub/main.go"), "* stops at a slash");
        assert!(section_matches("src/**.go", "src/sub/main.go"));
        assert!(section_matches("**/test/*.go", "a/b/test/x.go"));
        assert!(section_matches("**.go", "a/b/x.go"));
        assert!(!section_matches("/src/*.go", "other/src/main.go"));
        assert!(section_matches("Makefile", "build/Makefile"));
        assert!(section_matches("?.txt", "a.txt"));
        assert!(!section_matches("?.txt", "ab.txt"));
        assert!(section_matches("{*.json,.prettierrc}", ".prettierrc"));
        assert!(section_matches("{a,{b,c}}.go", "c.go"), "braces nest");
        assert!(section_matches("a\\*.go", "a*.go") && !section_matches("a\\*.go", "ab.go"), "escapes");
        assert!(!has_separator("*.[/]") && has_separator("a/*.go") && !has_separator("a\\/b"), "a slash in brackets");
    }

    #[test]
    fn later_sections_win_within_a_file() {
        let text = "root = true\n\n[*]\nend_of_line = LF\nindent_style = space\n\n# a comment\n; another\n[*.bat]\nend_of_line = crlf\n";
        let file = File::parse(text);
        assert!(file.root);
        assert!(file.problems.is_empty(), "{:?}", file.problems);
        assert_eq!(
            parse_for(text, "run.bat"),
            [("end_of_line", "lf"), ("indent_style", "space"), ("end_of_line", "crlf")]
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
        assert!(!File::parse("[*]\nroot = true\n").root, "root counts before the first section only");
        let broken = "[*.txt]\nindent_size = 2\n[*.md\nindent_size = 4\n[*.go]\nindent_size = 8\n";
        assert_eq!(
            parse_for(broken, "a.txt"),
            vec![("indent_size".to_owned(), "2".to_owned())],
            "a broken section matches nothing"
        );
        assert_eq!(parse_for(broken, "a.go"), vec![("indent_size".to_owned(), "8".to_owned())]);
    }

    #[test]
    fn lines_it_cannot_read_are_problems() {
        let text = "root = maybe\nindent_size = 2\n[*\n[*.go]\njust words\n = 3\nindent_style = tabs\nindent_size = 0\n\
                    tab_width = x\nmax_line_length = off\nmax_line_length = -1\nij_go_continuation_indent_size = big\n\
                    ij_visual_guides = 80,x\nij_go_unknown_option = whatever\nindent_size = unset\n";
        let file = File::parse(text);
        let kinds: Vec<(usize, ProblemKind)> =
            file.problems.iter().map(|problem| (problem.line, problem.kind.clone())).collect();
        let invalid =
            |name: &str, value: &str| ProblemKind::InvalidValue { name: name.to_owned(), value: value.to_owned() };
        assert_eq!(
            kinds,
            vec![
                (1, invalid("root", "maybe")),
                (2, ProblemKind::OutsideSection("indent_size".to_owned())),
                (3, ProblemKind::UnclosedSection("[*".to_owned())),
                (5, ProblemKind::NotAProperty("just words".to_owned())),
                (6, ProblemKind::NoName),
                (7, invalid("indent_style", "tabs")),
                (8, invalid("indent_size", "0")),
                (9, invalid("tab_width", "x")),
                (11, invalid("max_line_length", "-1")),
                (12, invalid("ij_go_continuation_indent_size", "big")),
                (13, invalid("ij_visual_guides", "80,x")),
            ]
        );
        assert_eq!(
            parse_for(text, "a.go"),
            [("max_line_length", "off"), ("ij_go_unknown_option", "whatever"), ("indent_size", "unset")]
                .map(|(name, value)| (name.to_owned(), value.to_owned())),
            "valid lines are still read"
        );
    }

    #[test]
    fn the_nearest_file_wins_up_to_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("win/sub")).unwrap();
        std::fs::write(root.join(".editorconfig"), "root = true\n[*]\nend_of_line = lf\ncharset = utf-8\n").unwrap();
        std::fs::write(root.join("win/.editorconfig"), "[*.txt]\nend_of_line = crlf\ncharset = unset\n").unwrap();
        let file = root.join("win/sub/a.txt");
        let found = properties(root, &file);
        assert_eq!(found.get("end_of_line").map(String::as_str), Some("crlf"));
        assert_eq!(found.get("charset"), None, "unset drops it");
        assert_eq!(properties(root, &root.join("win/a.go")).get("end_of_line").map(String::as_str), Some("lf"));
        std::fs::write(root.join("win/.editorconfig"), "root = true\n[*.md]\nend_of_line = cr\n").unwrap();
        assert_eq!(properties(root, &file).get("end_of_line"), None, "nothing above a root file is read");
        assert!(properties(&root.join("win"), &root.join("a.txt")).is_empty(), "outside the root");
    }

    #[test]
    fn lookup_names_the_files_that_matched_the_nearest_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join(".editorconfig"), "[*.go]\nindent_style = tab\n").unwrap();
        std::fs::write(root.join("a/.editorconfig"), "[*.md]\nindent_size = 2\n").unwrap();
        std::fs::write(root.join("a/b/.editorconfig"), "[*]\nindent_size = 3\n").unwrap();
        let mut reads = 0;
        let found = lookup(root, &root.join("a/b/x.go"), |folder| {
            reads += 1;
            std::fs::read_to_string(folder.join(FILE_NAME)).ok().map(|text| Arc::new(File::parse(&text)))
        });
        assert_eq!(reads, 3);
        assert_eq!(found.files, vec![root.join("a/b/.editorconfig"), root.join(".editorconfig")]);
        assert_eq!(found.properties.get("indent_size").map(String::as_str), Some("3"));
        assert_eq!(found.properties.get("indent_style").map(String::as_str), Some("tab"));
    }

    #[test]
    fn standard_properties_map_onto_the_code_style() {
        let go = style("[*]\nindent_style = space\nindent_size = 2\n", "a.go", "go");
        assert_eq!(go.use_tabs, Some(false));
        assert_eq!(go.indent, Some(Indent::Columns(2)));
        assert_eq!(go.tab_size, Some(2), "tab_width defaults to a numeric indent_size");
        assert!(go.sets_indent());
        let base = CodeStyle { use_tabs: true, tab_size: 4, indent: 4 };
        assert_eq!(go.apply_to(base), CodeStyle { use_tabs: false, tab_size: 2, indent: 2 });

        let tabs = style("[*]\nindent_style = tab\ntab_width = 8\n", "a.go", "go");
        assert_eq!(tabs.indent, Some(Indent::TabSize), "indent_size defaults to tab under indent_style = tab");
        assert_eq!(tabs.apply_to(CodeStyle::default()), CodeStyle { use_tabs: true, tab_size: 8, indent: 8 });

        let tab_size = style("[*]\nindent_size = tab\n", "a.rs", "rust");
        assert_eq!(tab_size.apply_to(CodeStyle { use_tabs: false, tab_size: 3, indent: 4 }).indent, 3);

        let width_only = style("[*]\ntab_width = 6\n", "a.txt", "plaintext");
        assert!(!width_only.sets_indent());
        assert_eq!(width_only.apply_to(CodeStyle::default()), CodeStyle { use_tabs: false, tab_size: 6, indent: 4 });

        let margin = style("[*]\nmax_line_length = 100\n", "a.go", "go");
        let file = margin.apply_to_file(FileCodeStyle { right_margin: Some(120), ..FileCodeStyle::default() });
        assert_eq!(file.right_margin, Some(100));
        let off = style("[*]\nmax_line_length = off\n", "a.go", "go");
        assert_eq!(off.right_margin, Some(RightMargin::Off));
        assert_eq!(
            off.apply_to_file(FileCodeStyle { right_margin: Some(120), ..FileCodeStyle::default() }).right_margin,
            None
        );

        let unset =
            style("[*]\nindent_style = tab\nmax_line_length = 90\n[*.go]\nindent_style = unset\n", "a.go", "go");
        assert_eq!(unset.use_tabs, None, "unset leaves it to the project and the IDE");
        assert_eq!(unset.indent, None);
        assert!(style("", "a.go", "go").is_empty());
    }

    #[test]
    fn ij_options_go_from_the_language_to_any_to_general() {
        let text = "[*]\nij_continuation_indent_size = 4\nij_visual_guides = 80, 100\nij_any_continuation_indent_size = 6\n\
                    ij_go_continuation_indent_size = 8\nij_go_visual_guides = none\nij_unknown_thing = 1\n";
        let go = style(text, "a.go", "go");
        assert_eq!(go.continuation_indent, Some(8));
        assert_eq!(go.visual_guides, Some(Vec::new()));
        let json = style(text, "a.json", "json");
        assert_eq!(json.continuation_indent, Some(6), "ij_any_ beats ij_");
        assert_eq!(json.visual_guides, Some(vec![80, 100]));
        let general =
            style("[*]\nij_continuation_indent_size = 4\nij_json_continuation_indent_size = 2\n", "a.go", "go");
        assert_eq!(general.continuation_indent, Some(4), "another language's option is not read");
        let file = json.apply_to_file(FileCodeStyle::default());
        assert_eq!((file.continuation_indent, file.visual_guides), (Some(6), vec![80, 100]));
    }
}
