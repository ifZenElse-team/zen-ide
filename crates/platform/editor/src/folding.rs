use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FoldKind {
    Block,
    Imports,
    Comment,
    Region,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    pub range: Range<usize>,
    pub kind: FoldKind,
    pub placeholder: Option<String>,
    pub option: Option<&'static str>,
}

impl Fold {
    pub fn new(range: Range<usize>, kind: FoldKind) -> Self {
        Self { range, kind, placeholder: None, option: None }
    }
}

pub fn folds(text: &str) -> Vec<Fold> {
    let chars: Vec<char> = text.chars().collect();
    let mut found: Vec<Fold> = fold_ranges(text)
        .into_iter()
        .map(|range| {
            let opening = range.start.checked_sub(1).map(|at| chars[at]);
            let line_start =
                chars[..range.start.saturating_sub(1)].iter().rposition(|c| *c == '\n').map_or(0, |at| at + 1);
            let before: String = chars[line_start..range.start.saturating_sub(1)].iter().collect();
            if chars[range.start..].starts_with(&['/', '*']) {
                Fold::new(range, FoldKind::Comment)
            } else if opening == Some('(') && before.trim() == "import" {
                Fold::new(range.start - 1..(range.end + 1).min(chars.len()), FoldKind::Imports)
            } else if opening.is_some_and(|c| matches!(c, '{' | '(' | '['))
                && chars.get(range.end).is_some_and(|c| matches!(c, '}' | ')' | ']'))
            {
                Fold::new(range, FoldKind::Block)
            } else {
                Fold::new(range, FoldKind::Comment)
            }
        })
        .collect();
    found.extend(use_runs(text).into_iter().map(|range| Fold::new(range, FoldKind::Imports)));
    found.sort_by_key(|fold| (fold.range.start, std::cmp::Reverse(fold.range.end)));
    found
}

pub fn fold_ranges(text: &str) -> Vec<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let mut ranges = Vec::new();
    let mut open: Vec<(char, usize, usize)> = Vec::new();
    let mut line = 0;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();
        match c {
            '\n' => line += 1,
            '/' if next == Some('/') => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
                continue;
            }
            '/' if next == Some('*') => {
                let (start, start_line) = (index, line);
                index += 2;
                while index < chars.len() && !(chars[index] == '*' && chars.get(index + 1) == Some(&'/')) {
                    if chars[index] == '\n' {
                        line += 1;
                    }
                    index += 1;
                }
                index += 2;
                if index <= chars.len() && line > start_line {
                    ranges.push(start..index);
                }
                continue;
            }
            '\'' if next != Some('\\') && chars.get(index + 2) != Some(&'\'') => {}
            '"' | '\'' | '`' => {
                let quote = c;
                index += 1;
                while index < chars.len() && chars[index] != quote {
                    if chars[index] == '\\' && quote != '`' {
                        index += 1;
                    } else if chars[index] == '\n' {
                        if quote != '`' {
                            break;
                        }
                        line += 1;
                    }
                    index += 1;
                }
            }
            '{' | '(' | '[' => open.push((c, index, line)),
            '}' | ')' | ']' => {
                let wanted = match c {
                    '}' => '{',
                    ')' => '(',
                    _ => '[',
                };
                if let Some(at) = open.iter().rposition(|(bracket, _, _)| *bracket == wanted) {
                    let (_, start, start_line) = open[at];
                    open.truncate(at);
                    if line > start_line {
                        ranges.push(start + 1..index);
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    ranges.extend(comment_runs(text));
    ranges.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
    ranges.dedup();
    ranges
}

fn use_runs(text: &str) -> Vec<Range<usize>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        starts.push(offset);
        offset += line.chars().count();
    }
    let body = |index: usize| lines[index].trim_end_matches(['\n', '\r']);
    let keyword = |index: usize| {
        ["use ", "pub use ", "pub(crate) use "]
            .into_iter()
            .find(|key| body(index).starts_with(key) && is_use_path(&body(index)[key.len()..]))
    };
    let mut runs = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(first) = keyword(index) else {
            index += 1;
            continue;
        };
        let start = starts[index] + first.chars().count();
        let (mut count, mut last) = (0, index);
        while index < lines.len() {
            if body(index).trim().is_empty() {
                index += 1;
                continue;
            }
            if keyword(index).is_none() {
                break;
            }
            while index < lines.len() && !body(index).trim_end().ends_with(';') {
                index += 1;
            }
            last = index.min(lines.len() - 1);
            count += 1;
            index += 1;
        }
        if count >= 2 {
            runs.push(start..starts[last] + body(last).chars().count());
        }
    }
    runs
}

fn is_use_path(rest: &str) -> bool {
    let rest = rest.trim_end();
    let Some(head) = rest.strip_suffix(';').or_else(|| rest.strip_suffix('{').map(|_| rest)) else { return false };
    let path = head.split('{').next().unwrap_or_default();
    let path = path.split_once(" as ").map_or(path, |(path, _)| path).trim();
    let qualified = path.contains("::") || path.contains('\\') || head.contains('{');
    qualified && !path.is_empty() && path.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '\\' | '*'))
}

fn comment_runs(text: &str) -> Vec<Range<usize>> {
    comment_runs_and_names(text).into_iter().filter_map(|comment| comment.fold).collect()
}

pub fn doc_comment_names(text: &str) -> Vec<Range<usize>> {
    comment_runs_and_names(text).into_iter().filter_map(|comment| comment.name).collect()
}

pub fn doc_comment_names_in(text: &zen_text::Text, lines: Range<usize>) -> Vec<Range<usize>> {
    const REACH: usize = 400;
    let count = text.len_lines();
    let is_comment = |line: usize| text.line(line).is_ok_and(|body| body.trim_start().starts_with("//"));
    let mut first = lines.start.min(count);
    while first > 0 && lines.start - first < REACH && is_comment(first - 1) {
        first -= 1;
    }
    let mut last = lines.end.min(count);
    while last < count && last - lines.end < REACH && is_comment(last) {
        last += 1;
    }
    let last = (last + 1).min(count);
    let Ok(start) = text.point_to_offset(zen_text::Point::new(first, 0)) else { return Vec::new() };
    let end = text.point_to_offset(zen_text::Point::new(last, 0)).unwrap_or(text.len_chars());
    let Ok(slice) = text.slice(start..end) else { return Vec::new() };
    let mut names = doc_comment_names(&slice);
    if first > 0 && is_comment(first - 1) {
        names.retain(|name| text.offset_to_point(start + name.start).is_ok_and(|point| point.line > first));
    }
    names.into_iter().map(|name| start + name.start..start + name.end).collect()
}

struct LineComment {
    fold: Option<Range<usize>>,
    name: Option<Range<usize>>,
}

fn comment_runs_and_names(text: &str) -> Vec<LineComment> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        starts.push(offset);
        offset += line.chars().count();
    }
    let body = |index: usize| lines[index].trim_end_matches(['\n', '\r']);
    let is_comment = |index: usize| body(index).trim_start().starts_with("//");
    let mut runs = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if !is_comment(index) {
            index += 1;
            continue;
        }
        let first = index;
        while index < lines.len() && is_comment(index) {
            index += 1;
        }
        let first_line = body(first);
        let indent = first_line.chars().count() - first_line.trim_start().chars().count();
        let mut start = starts[first] + indent;
        let mut named = None;
        let after_slashes = first_line.trim_start().trim_start_matches('/');
        let name: String =
            after_slashes.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if !name.is_empty() && index < lines.len() && declares(body(index), &name) {
            let lead = first_line.chars().count() - after_slashes.trim_start().chars().count();
            start = starts[first] + lead + name.chars().count();
            named = Some(starts[first] + lead..start);
        }
        let last = index - 1;
        let run = (index - first >= 2).then(|| start..starts[last] + body(last).chars().count());
        runs.push(LineComment { fold: run, name: named });
    }
    runs
}

fn declares(line: &str, name: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(name).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + name.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folded(text: &str, range: &Range<usize>) -> String {
        text.chars().skip(range.start).take(range.len()).collect()
    }

    #[test]
    fn blocks_and_comment_runs() {
        let text = "// one\n// two\nvar (\n\ta = 1\n)\n\nfunc f() {\n\tif x {\n\t\ty()\n\t}\n}\n";
        let ranges = fold_ranges(text);
        let shown: Vec<String> = ranges.iter().map(|range| folded(text, range)).collect();
        assert_eq!(shown, ["// one\n// two", "\n\ta = 1\n", "\n\tif x {\n\t\ty()\n\t}\n", "\n\t\ty()\n\t"]);
    }

    #[test]
    fn block_comments_of_several_lines_fold_whole() {
        let text = "/*\nLicense text\nline two\n*/\nvar x = 1\n/* one line */\nf(/* a\n b */)\n/* open\n";
        let found = folds(text);
        let shown: Vec<(String, FoldKind)> = found.iter().map(|fold| (folded(text, &fold.range), fold.kind)).collect();
        assert_eq!(
            shown,
            [
                ("/*\nLicense text\nline two\n*/".to_owned(), FoldKind::Comment),
                ("/* a\n b */".to_owned(), FoldKind::Comment),
            ]
        );
    }

    #[test]
    fn brackets_in_strings_and_comments_do_not_count() {
        let text = "s := \"{\"\n// }\nr := `a\n{`\nf(func() {\n\tg()\n})\n";
        let ranges = fold_ranges(text);
        let shown: Vec<String> = ranges.iter().map(|range| folded(text, range)).collect();
        assert_eq!(shown, ["func() {\n\tg()\n}", "\n\tg()\n"]);
    }

    #[test]
    fn imports_fold_with_their_parentheses() {
        let text = "package main\n\nimport (\n\t\"fmt\"\n)\n\nfunc f() {\n\tx()\n}\n";
        let found = folds(text);
        assert_eq!(found[0].kind, FoldKind::Imports);
        assert_eq!(folded(text, &found[0].range), "(\n\t\"fmt\"\n)");
        assert_eq!(found[1].kind, FoldKind::Block);
        assert_eq!(folded(text, &found[1].range), "\n\tx()\n");
    }

    #[test]
    fn a_lifetime_is_no_quote() {
        let text = "fn fmt(&self, f: &mut Formatter<'_>) -> Result {\n    write!(f, \"{}\", 'x')\n}\nfn g<'a>(x: &'a str) {\n    h('\\'')\n}\n";
        let shown: Vec<String> = fold_ranges(text).iter().map(|range| folded(text, range)).collect();
        assert_eq!(shown, ["\n    write!(f, \"{}\", 'x')\n", "\n    h('\\'')\n"]);
    }

    #[test]
    fn runs_of_use_declarations_fold_as_imports() {
        let text = "//! Crate.\n\nuse std::fmt;\nuse std::{\n    io,\n    path,\n};\n\npub use crate::a::B;\n\nfn f() {}\nuse x;\n";
        let imports: Vec<String> = folds(text)
            .iter()
            .filter(|fold| fold.kind == FoldKind::Imports)
            .map(|fold| folded(text, &fold.range))
            .collect();
        assert_eq!(imports, ["std::fmt;\nuse std::{\n    io,\n    path,\n};\n\npub use crate::a::B;"]);
        assert!(
            folds("use std::fmt;\n\nfn f() {}\n").iter().all(|fold| fold.kind != FoldKind::Imports),
            "one is no run"
        );
        for prose in ["use the tool;\nuse it again;\n", "Then\nuse this\nand that.\nuse more;\n"] {
            assert!(folds(prose).iter().all(|fold| fold.kind != FoldKind::Imports), "{prose:?}");
        }
        let php = "use App\\Models\\User;\nuse Foo\\Bar as Baz;\n";
        assert_eq!(folds(php).iter().filter(|fold| fold.kind == FoldKind::Imports).count(), 1);
    }

    #[test]
    fn one_line_blocks_do_not_fold_and_offsets_count_characters() {
        assert!(fold_ranges("f(a, b)\n").is_empty());
        let text = "// привет\n// мир\nx\n";
        let ranges = fold_ranges(text);
        assert_eq!(folded(text, &ranges[0]), "// привет\n// мир");
    }

    #[test]
    fn a_doc_comment_keeps_the_name_it_describes() {
        let text = "\t// TestLimitStops checks\n\t// the limit.\nfunc TestLimitStops(t *testing.T) {}\n";
        let found = folds(text);
        assert_eq!(found[0].kind, FoldKind::Comment);
        assert_eq!(folded(text, &found[0].range), " checks\n\t// the limit.");
        let names = doc_comment_names(text);
        assert_eq!(names.iter().map(|name| folded(text, name)).collect::<Vec<_>>(), ["TestLimitStops"]);
        let one = "// Run starts.\nfunc Run() {}\n";
        assert!(fold_ranges(one).is_empty());
        assert_eq!(doc_comment_names(one), vec![Range { start: 3, end: 6 }]);
        let long = format!("x := 1\n{}// Run starts\n// it.\nfunc Run() {{}}\n", "y := 2\n".repeat(50));
        let rope = zen_text::Text::from(long.as_str());
        let all = doc_comment_names(&long);
        assert_eq!(all.len(), 1);
        assert_eq!(doc_comment_names_in(&rope, 52..53), all, "the comment's second line");
        assert_eq!(doc_comment_names_in(&rope, 53..54), all, "the declaring line");
        assert!(doc_comment_names_in(&rope, 0..10).is_empty());
        let text = "// TestLimit checks\n// more.\nfunc TestLimitStops() {}\n";
        assert_eq!(folded(text, &folds(text)[0].range), "// TestLimit checks\n// more.");
    }
}
