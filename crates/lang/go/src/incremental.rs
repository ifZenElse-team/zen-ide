use std::collections::HashMap;
use std::ops::Range;

use zen_syntax::{FoldRegion, FoldRegionKind, HighlightKind, Incremental, LineLexer, Lines, Span, TextChange};

use crate::{is_builtin_type, is_keyword};

pub(crate) mod fold_options;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Code,
    BlockComment,
    RawString,
    Quoted(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Previous {
    #[default]
    Other,
    Func,
    Type,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Const {
    #[default]
    Idle,
    After,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State {
    mode: Mode,
    previous: Previous,
    first: bool,
    konst: Const,
}

impl Default for State {
    fn default() -> Self {
        Self { mode: Mode::Code, previous: Previous::Other, first: true, konst: Const::Idle }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    Paren,
    Other,
    Later,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    Fixed(HighlightKind),
    Name { builtin: bool, previous: Previous, next: Next },
    Bracket(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token {
    start: u32,
    end: u32,
    piece: Piece,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Line {
    tokens: Vec<Token>,
    constants: Vec<Box<str>>,
    first_paren: Option<bool>,
    comment: bool,
    block_opens: Option<u32>,
    block_closes: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GoLexer;

fn quoted_end(line: &str, from: usize, quote: char) -> (usize, bool) {
    let mut chars = line[from..].char_indices();
    while let Some((offset, c)) = chars.next() {
        match c {
            '\\' => {
                if chars.next().is_some_and(|(_, escaped)| escaped == '\n') {
                    return (line.len(), true);
                }
            }
            '\n' => return (from + offset, false),
            c if c == quote => return (from + offset + 1, false),
            _ => {}
        }
    }
    (line.len(), false)
}

impl LineLexer for GoLexer {
    type State = State;
    type Line = Line;

    fn lex(&self, line: &str, state: &mut State) -> Line {
        let body = line.trim_end_matches(['\n', '\r']);
        let mut out = Line { comment: body.trim_start().starts_with("//"), ..Line::default() };
        let push = |out: &mut Line, start: usize, end: usize, piece: Piece| {
            if end > start {
                out.tokens.push(Token { start: start as u32, end: end as u32, piece });
            }
        };
        let mut i = match state.mode {
            Mode::Code => 0,
            Mode::BlockComment => {
                let closed = line.find("*/");
                let end = closed.map_or(line.len(), |at| at + 2);
                if closed.is_some() {
                    state.mode = Mode::Code;
                    out.block_closes = Some(end as u32);
                }
                push(&mut out, 0, end, Piece::Fixed(HighlightKind::Comment));
                end
            }
            Mode::RawString => {
                let closed = line.find('`');
                let end = closed.map_or(line.len(), |at| at + 1);
                if closed.is_some() {
                    state.mode = Mode::Code;
                }
                push(&mut out, 0, end, Piece::Fixed(HighlightKind::String));
                end
            }
            Mode::Quoted(quote) => {
                let (end, continues) = quoted_end(line, 0, char::from(quote));
                if !continues {
                    state.mode = Mode::Code;
                }
                push(&mut out, 0, end, Piece::Fixed(HighlightKind::String));
                end
            }
        };
        let mut waiting: Option<usize> = None;
        let bytes = line.as_bytes();
        while i < line.len() {
            let start = i;
            let rest = &line[i..];
            let c = rest.chars().next().unwrap_or('\0');
            if c.is_whitespace() {
                i += rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
                state.first |= line[start..i].contains('\n');
                continue;
            }
            if rest.starts_with("//") {
                i += rest.find('\n').unwrap_or(rest.len());
                push(&mut out, start, i, Piece::Fixed(HighlightKind::Comment));
                state.first = true;
                continue;
            }
            if let Some(comment) = rest.strip_prefix("/*") {
                match comment.find("*/") {
                    Some(end) => i += end + 4,
                    None => {
                        i = line.len();
                        state.mode = Mode::BlockComment;
                        out.block_opens = Some(start as u32);
                    }
                }
                push(&mut out, start, i, Piece::Fixed(HighlightKind::Comment));
                continue;
            }
            let piece = if c == '`' {
                match rest[1..].find('`') {
                    Some(end) => i += end + 2,
                    None => {
                        i = line.len();
                        state.mode = Mode::RawString;
                    }
                }
                Piece::Fixed(HighlightKind::String)
            } else if c == '"' || c == '\'' {
                let (end, continues) = quoted_end(line, i + 1, c);
                i = end;
                if continues {
                    state.mode = Mode::Quoted(c as u8);
                }
                Piece::Fixed(HighlightKind::String)
            } else if c.is_ascii_digit() || (c == '.' && rest[1..].starts_with(|c: char| c.is_ascii_digit())) {
                i += 1;
                let hex = rest.starts_with("0x") || rest.starts_with("0X");
                while let Some(next) = line[i..].chars().next() {
                    let exponent = matches!(next, '+' | '-')
                        && matches!(bytes[i - 1], b'e' | b'E' | b'p' | b'P')
                        && (!hex || matches!(bytes[i - 1], b'p' | b'P'));
                    if next.is_ascii_alphanumeric() || next == '_' || next == '.' || exponent {
                        i += next.len_utf8();
                    } else {
                        break;
                    }
                }
                Piece::Fixed(HighlightKind::Number)
            } else if c == '_' || c.is_alphabetic() {
                i += rest.find(|c: char| !(c == '_' || c.is_alphanumeric())).unwrap_or(rest.len());
                let word = &line[start..i];
                if is_keyword(word) {
                    Piece::Fixed(HighlightKind::Keyword)
                } else {
                    Piece::Name { builtin: is_builtin_type(word), previous: state.previous, next: Next::Later }
                }
            } else {
                i += c.len_utf8();
                Piece::Bracket(if c.is_ascii() { c as u8 } else { 0 })
            };
            let word = &line[start..i];
            let paren = word == "(";
            let ident = c == '_' || c.is_alphabetic();
            if let Some(at) = waiting.take()
                && let Piece::Name { next, .. } = &mut out.tokens[at].piece
            {
                *next = if paren { Next::Paren } else { Next::Other };
            }
            if out.first_paren.is_none() {
                out.first_paren = Some(paren);
            }
            state.konst = match state.konst {
                Const::Idle if word == "const" => Const::After,
                Const::Idle => Const::Idle,
                Const::After if ident => {
                    out.constants.push(word.into());
                    if word == "const" { Const::After } else { Const::Idle }
                }
                Const::After if paren => Const::Block,
                Const::After => Const::Idle,
                Const::Block if word == ")" && state.first => Const::Idle,
                Const::Block => {
                    if ident && state.first {
                        out.constants.push(word.into());
                    }
                    Const::Block
                }
            };
            state.previous = match word {
                "func" => Previous::Func,
                "type" => Previous::Type,
                _ => Previous::Other,
            };
            state.first = false;
            match piece {
                Piece::Bracket(b'{' | b'}' | b'(' | b')' | b'[' | b']') => push(&mut out, start, i, piece),
                Piece::Bracket(_) => {}
                Piece::Name { .. } => {
                    waiting = Some(out.tokens.len());
                    push(&mut out, start, i, piece);
                }
                Piece::Fixed(_) => push(&mut out, start, i, piece),
            }
        }
        out
    }
}

#[derive(Debug)]
pub(crate) struct GoLines {
    lines: Lines<GoLexer>,
    constants: HashMap<Box<str>, usize>,
}

impl GoLines {
    pub(crate) fn new(text: &str) -> Self {
        let lines = Lines::new(&GoLexer, text);
        let mut constants = HashMap::new();
        for line in lines.lines() {
            for name in &line.constants {
                *constants.entry(name.clone()).or_default() += 1;
            }
        }
        Self { lines, constants }
    }

    fn paren_after(&self, index: usize) -> bool {
        self.lines.lines()[index + 1..].iter().find_map(|line| line.first_paren).unwrap_or(false)
    }

    fn kind(&self, text: &str, index: usize, start: usize, token: &Token) -> Option<HighlightKind> {
        match token.piece {
            Piece::Fixed(kind) => Some(kind),
            Piece::Bracket(_) => None,
            Piece::Name { builtin, previous, next } => {
                let word = &text[start + token.start as usize..start + token.end as usize];
                let call = || match next {
                    Next::Paren => true,
                    Next::Other => false,
                    Next::Later => self.paren_after(index),
                };
                if self.constants.contains_key(word) {
                    Some(HighlightKind::Constant)
                } else if builtin {
                    Some(HighlightKind::Type)
                } else if previous == Previous::Func || call() {
                    Some(HighlightKind::Function)
                } else if previous == Previous::Type {
                    Some(HighlightKind::Type)
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn fold_regions(&self, text: &str) -> Vec<FoldRegion> {
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut open: Vec<(u8, usize, usize)> = Vec::new();
        let mut comment: Option<usize> = None;
        for (index, line) in self.lines.lines().iter().enumerate() {
            let start = self.lines.start(index);
            if let Some(end) = line.block_closes {
                ranges.extend(comment.take().map(|opened| opened..start + end as usize));
            }
            if let Some(at) = line.block_opens {
                comment = Some(start + at as usize);
            }
            for token in &line.tokens {
                let Piece::Bracket(bracket) = token.piece else { continue };
                let at = start + token.start as usize;
                let wanted = match bracket {
                    b'{' | b'(' | b'[' => {
                        open.push((bracket, at, index));
                        continue;
                    }
                    b'}' => b'{',
                    b')' => b'(',
                    _ => b'[',
                };
                if let Some(found) = open.iter().rposition(|(bracket, ..)| *bracket == wanted) {
                    let (_, opened, opened_line) = open[found];
                    open.truncate(found);
                    if index > opened_line {
                        ranges.push(opened + 1..at);
                    }
                }
            }
        }
        ranges.extend(self.comment_runs(text));
        ranges.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
        ranges.dedup();
        let bytes = text.as_bytes();
        let mut folds: Vec<FoldRegion> = ranges
            .into_iter()
            .map(|range| {
                let opening = range.start.checked_sub(1).map(|at| bytes[at]);
                let line_start = self.lines.start(self.lines.line_of(range.start.saturating_sub(1)));
                let before = text.get(line_start..range.start.saturating_sub(1)).unwrap_or_default();
                if text[range.start..].starts_with("/*") {
                    FoldRegion::new(range, FoldRegionKind::Comment)
                } else if opening == Some(b'(') && before.trim() == "import" {
                    FoldRegion::new(range.start - 1..(range.end + 1).min(text.len()), FoldRegionKind::Imports)
                } else if opening.is_some_and(|c| matches!(c, b'{' | b'(' | b'['))
                    && bytes.get(range.end).is_some_and(|c| matches!(c, b'}' | b')' | b']'))
                {
                    FoldRegion::new(range, FoldRegionKind::Block)
                } else {
                    FoldRegion::new(range, FoldRegionKind::Comment)
                }
            })
            .collect();
        self.mark_options(text, &mut folds);
        folds.sort_by_key(|fold| (fold.range.start, std::cmp::Reverse(fold.range.end)));
        folds
    }

    fn comment_runs(&self, text: &str) -> Vec<Range<usize>> {
        let lines = self.lines.lines();
        let body = |index: usize| text[self.lines.start(index)..self.lines.end(index)].trim_end_matches(['\n', '\r']);
        let mut runs = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            if !lines[index].comment {
                index += 1;
                continue;
            }
            let first = index;
            while index < lines.len() && lines[index].comment {
                index += 1;
            }
            if index - first < 2 {
                continue;
            }
            let first_line = body(first);
            let mut start = self.lines.start(first) + first_line.len() - first_line.trim_start().len();
            let after_slashes = first_line.trim_start().trim_start_matches('/');
            let name_len = after_slashes
                .trim_start()
                .char_indices()
                .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
                .map_or(after_slashes.trim_start().len(), |(at, _)| at);
            let name = &after_slashes.trim_start()[..name_len];
            if !name.is_empty() && index < lines.len() && declares(body(index), name) {
                let lead = first_line.len() - after_slashes.trim_start().len();
                start = self.lines.start(first) + lead + name.len();
            }
            let last = index - 1;
            runs.push(start..self.lines.start(last) + body(last).len());
        }
        runs
    }
}

fn declares(line: &str, name: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(name).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + name.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

impl Incremental for GoLines {
    fn edit(&mut self, text: &str, change: TextChange) -> usize {
        let constants = &mut self.constants;
        let lexed = self.lines.edit(&GoLexer, text, change, |line| {
            for name in line.constants {
                if let Some(count) = constants.get_mut(&name) {
                    *count -= 1;
                    if *count == 0 {
                        constants.remove(&name);
                    }
                }
            }
        });
        for line in &self.lines.lines()[lexed.clone()] {
            for name in &line.constants {
                *self.constants.entry(name.clone()).or_default() += 1;
            }
        }
        lexed.len()
    }

    fn spans(&self, text: &str, range: Range<usize>) -> Vec<Span> {
        let first = self.lines.line_of(range.start);
        let last = self.lines.line_of(range.end.saturating_sub(1).max(range.start));
        let mut spans = Vec::new();
        for index in first..=last.min(self.lines.len() - 1) {
            let start = self.lines.start(index);
            for token in &self.lines.line(index).tokens {
                let (from, to) = (start + token.start as usize, start + token.end as usize);
                if to <= range.start || (from >= range.end && range.end > range.start) {
                    continue;
                }
                if let Some(kind) = self.kind(text, index, start, token) {
                    spans.push(Span { range: from..to, kind });
                }
            }
        }
        spans
    }

    fn folds(&self, text: &str) -> Option<Vec<FoldRegion>> {
        Some(self.fold_regions(text))
    }

    fn lines(&self) -> usize {
        self.lines.len()
    }
}

#[cfg(test)]
mod tests {
    use zen_syntax::Highlighter;

    use super::*;
    use crate::GoHighlighter;

    fn by_lines(text: &str) -> Vec<Span> {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        let mut out = Vec::new();
        for span in GoHighlighter.highlight(text) {
            let mut from = span.range.start;
            while from < span.range.end {
                let line_end = starts.iter().copied().find(|start| *start > from).unwrap_or(text.len());
                let to = span.range.end.min(line_end);
                out.push(Span { range: from..to, kind: span.kind });
                from = to;
            }
        }
        out
    }

    fn assert_same(lines: &GoLines, text: &str) {
        assert_eq!(lines.spans(text, 0..text.len()), by_lines(text), "{text:?}");
        let fresh = GoLines::new(text);
        assert_eq!(lines.lines.lines(), fresh.lines.lines(), "{text:?}");
        assert_eq!(lines.constants, fresh.constants, "{text:?}");
        assert_eq!(lines.fold_regions(text), fresh.fold_regions(text), "{text:?}");
    }

    const SAMPLE: &str = "package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n\nconst Max = 3\n\nconst (\n\tA = iota\n\tB\n)\n\n// Greet says hello\n// to a name.\nfunc Greet(name string) error {\n\tfmt.Println(`raw\nstring`, 'x', 0x1F, 1.5e-3, len(name), nil, \"a\\\nb\")\n\t/* a block\n\tcomment */ call\n\t(1)\n\treturn nil\n}\n\ntype Point struct{\n\tX int\n}\n\nvar p = Point{\n\tX: Max + B,\n}\n";

    #[test]
    fn the_lines_give_the_spans_of_the_whole_text() {
        assert_same(&GoLines::new(SAMPLE), SAMPLE);
        for text in
            ["", "\n", "\"open", "`open\n", "/* open\n\n", "'", "0x", "func (", "\"a\\", "const", "const (\nX\n"]
        {
            assert_same(&GoLines::new(text), text);
        }
    }

    fn apply(text: &mut String, start: usize, end: usize, insert: &str) -> TextChange {
        text.replace_range(start..end, insert);
        TextChange { start, old_end: end, new_end: start + insert.len() }
    }

    #[test]
    fn random_edits_keep_the_highlighting_and_the_folds_of_the_whole_text() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move |below: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % below.max(1) as u64) as usize
        };
        let pieces = [
            "\n",
            "/*",
            "*/",
            "`",
            "\"",
            "'",
            "\\",
            "\\\n",
            "//",
            "const ",
            "const (\n",
            ")",
            "(",
            "{",
            "}",
            "\n)\n",
            "func ",
            "type ",
            "X",
            "Max",
            "B",
            "int",
            " ",
            "\t",
            "1e+5",
            "0x1p-2",
            ".5",
            "import (",
            "// Doc\n",
            "é",
        ];
        for round in 0..40 {
            let mut text = SAMPLE.to_owned();
            let mut lines = GoLines::new(&text);
            for _ in 0..150 {
                let mut start = next(text.len() + 1);
                while !text.is_char_boundary(start) {
                    start -= 1;
                }
                let mut end = (start + next(8)).min(text.len());
                while !text.is_char_boundary(end) {
                    end += 1;
                }
                let mut changes = vec![apply(&mut text, start, end, pieces[next(pieces.len())])];
                if round % 3 == 0 {
                    let mut at = next(text.len() + 1);
                    while !text.is_char_boundary(at) {
                        at -= 1;
                    }
                    changes.push(apply(&mut text, at, at, pieces[next(pieces.len())]));
                }
                lines.edit(&text, TextChange::merged(&changes).unwrap());
                assert_same(&lines, &text);
            }
        }
    }

    fn editor_folds(lines: &GoLines, text: &str) -> Vec<zen_editor::Fold> {
        let chars = |byte: usize| text[..byte].chars().count();
        let only_options = [fold_options::ONE_LINE_CASES, fold_options::FORMATTED_STRINGS];
        lines
            .fold_regions(text)
            .into_iter()
            .filter(|fold| !fold.option.is_some_and(|option| only_options.contains(&option)))
            .map(|fold| {
                zen_editor::Fold::new(
                    chars(fold.range.start)..chars(fold.range.end),
                    match fold.kind {
                        FoldRegionKind::Block => zen_editor::FoldKind::Block,
                        FoldRegionKind::Imports => zen_editor::FoldKind::Imports,
                        FoldRegionKind::Comment => zen_editor::FoldKind::Comment,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn the_folds_are_those_the_editor_finds_in_brackets_and_comments() {
        for text in [
            SAMPLE.to_owned(),
            large(30),
            "func f() {\n\ts := \"{\"\n\tr := `\n}`\n\tg(func() {\n\t\th()\n\t})\n}\n".to_owned(),
        ] {
            let plain: Vec<zen_editor::Fold> =
                zen_editor::folds(&text).into_iter().map(|fold| zen_editor::Fold::new(fold.range, fold.kind)).collect();
            let tagged: Vec<zen_editor::Fold> = editor_folds(&GoLines::new(&text), &text);
            assert_eq!(tagged, plain, "{text}");
        }
        let folds = GoLines::new(SAMPLE).fold_regions(SAMPLE);
        let shown: Vec<&str> = folds.iter().map(|fold| &SAMPLE[fold.range.clone()]).collect();
        assert!(shown.contains(&"(\n\t\"fmt\"\n\t\"os\"\n)"), "the imports with their parentheses: {shown:?}");
        assert!(shown.contains(&" says hello\n// to a name."), "a doc comment keeps its name: {shown:?}");
        assert!(shown.contains(&"\n\tX: Max + B,\n"), "a composite literal: {shown:?}");
        assert!(shown.contains(&"\n\tA = iota\n\tB\n"), "a group of constants: {shown:?}");
        assert!(shown.contains(&"/* a block\n\tcomment */"), "a block comment: {shown:?}");
    }

    #[test]
    fn block_comments_of_several_lines_fold_whole() {
        let text = "/*\nLicense text\nline two\n*/\nvar x = 1\n/* one line */\n/* a\n b */ /* c\n d */\n/* open\n";
        let mut lines = GoLines::new(text);
        let shown = |lines: &GoLines, text: &str| -> Vec<(String, FoldRegionKind)> {
            lines.fold_regions(text).into_iter().map(|fold| (text[fold.range].to_owned(), fold.kind)).collect()
        };
        let comment = |body: &str| (body.to_owned(), FoldRegionKind::Comment);
        assert_eq!(
            shown(&lines, text),
            [comment("/*\nLicense text\nline two\n*/"), comment("/* a\n b */"), comment("/* c\n d */")]
        );
        assert_eq!(editor_folds(&lines, text), zen_editor::folds(text));
        let mut text = text.to_owned();
        let at = text.len();
        let change = apply(&mut text, at, at, "*/\n");
        lines.edit(&text, change);
        assert_same(&lines, &text);
        assert_eq!(shown(&lines, &text).last(), Some(&comment("/* open\n*/")));
    }

    fn large(count: usize) -> String {
        let mut text = String::from("package main\n\nimport \"fmt\"\n\n");
        for n in 0..count {
            text.push_str(&format!(
                "// Sum{n} adds the positive numbers of a slice.\nfunc Sum{n}(values []int) int {{\n\ttotal := 0\n\tfor _, v := range values {{\n\t\tif v > 0 {{\n\t\t\ttotal += v // positive only\n\t\t}}\n\t}}\n\tfmt.Println(\"sum\", {n})\n\treturn total + {n}\n}}\n\n"
            ));
        }
        text
    }

    #[test]
    fn typing_in_a_large_file_lexes_a_line_or_two() {
        let mut text = large(834);
        assert!(text.lines().count() >= 10_000);
        let mut lines = GoLines::new(&text);
        let middle = text.find("func Sum400(").unwrap() + "func Sum400(values []int) int {\n\t".len();
        let mut lexed = Vec::new();
        for (at, typed) in "x := 1".char_indices() {
            let change = apply(&mut text, middle + at, middle + at, &typed.to_string());
            lexed.push(lines.edit(&text, change));
        }
        assert!(lexed.iter().all(|count| *count == 1), "{lexed:?}");
        let change = apply(&mut text, middle, middle, "/*");
        let all = lines.edit(&text, change);
        assert!(all > 5_000, "{all}");
        let line_end = middle + text[middle..].find('\n').unwrap();
        let change = apply(&mut text, line_end, line_end, "*/");
        assert_eq!(lines.edit(&text, change), all);
        let change = apply(&mut text, middle + 2, middle + 2, "y");
        assert_eq!(lines.edit(&text, change), 1);
        assert_same(&lines, &text);
    }

    #[test]
    #[ignore = "a benchmark"]
    #[allow(clippy::print_stdout, reason = "a measurement, read on the terminal")]
    fn benchmark_typing() {
        for count in [834, 8_340] {
            let mut text = large(count);
            let lines_count = text.lines().count();
            let mut lines = GoLines::new(&text);
            let middle = text.len() / 2;
            let middle = middle + text[middle..].find('\n').unwrap() + 1;
            let rounds = 200;
            let started = std::time::Instant::now();
            for _ in 0..rounds {
                let change = apply(&mut text, middle, middle, "x");
                lines.edit(&text, change);
                std::hint::black_box(lines.spans(&text, middle..middle + 400));
                let change = apply(&mut text, middle, middle + 1, "");
                lines.edit(&text, change);
            }
            let incremental = started.elapsed().as_secs_f64() * 1000.0 / (2 * rounds) as f64;
            let started = std::time::Instant::now();
            for _ in 0..20 {
                std::hint::black_box(GoHighlighter.highlight(&text));
            }
            let whole = started.elapsed().as_secs_f64() * 1000.0 / 20.0;
            println!(
                "{lines_count} lines: a keystroke {incremental:.4} ms incrementally, {whole:.2} ms for the whole text"
            );
        }
    }
}
