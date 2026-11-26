use std::ops::Range;

use zen_modules::folding::METHOD_BODIES;
use zen_syntax::{FoldRegion, FoldRegionKind, HighlightKind};

use super::{GoLines, Mode, Piece};

pub const IF_ERRORS: &str = "go.folding.ifErrors";
pub const ONE_LINE_RETURNS: &str = "go.folding.oneLineReturns";
pub const ONE_LINE_CASES: &str = "go.folding.oneLineCases";
pub const EMPTY_FUNCTIONS: &str = "go.folding.emptyFunctions";
pub const EMPTY_TYPES: &str = "go.folding.emptyTypes";
pub const FORMATTED_STRINGS: &str = "go.folding.formattedStrings";

const FORMATTERS: &[&str] = &["Printf", "Sprintf", "Errorf", "Fprintf", "Logf", "Fatalf", "Panicf", "Skipf"];

enum Body {
    Function { declared: bool },
    Type,
    If,
    Other,
}

impl GoLines {
    pub(crate) fn mark_options(&self, text: &str, folds: &mut Vec<FoldRegion>) {
        let mut tagged = Vec::new();
        for (index, fold) in folds.iter().enumerate() {
            if let Some(tag) = self.block_option(text, fold, folds) {
                tagged.push((index, tag));
            }
        }
        for (index, (option, placeholder)) in tagged {
            folds[index].option = Some(option);
            folds[index].placeholder = placeholder;
        }
        folds.extend(self.case_clauses(text));
        folds.extend(self.formatted_strings(text));
    }

    fn body<'a>(&self, text: &'a str, index: usize) -> &'a str {
        text[self.lines.start(index)..self.lines.end(index)].trim_end_matches(['\n', '\r'])
    }

    fn block_option(
        &self,
        text: &str,
        fold: &FoldRegion,
        folds: &[FoldRegion],
    ) -> Option<(&'static str, Option<String>)> {
        let open = fold.range.start.checked_sub(1)?;
        if fold.kind != FoldRegionKind::Block || text.as_bytes()[open] != b'{' {
            return None;
        }
        let (open_line, close_line) = (self.lines.line_of(open), self.lines.line_of(fold.range.end));
        let line_start = self.lines.start(open_line);
        let head = text[line_start..open].trim();
        let opens_line = text[fold.range.start..].lines().next().is_none_or(|rest| rest.trim().is_empty());
        let closes_line = text[self.lines.start(close_line)..fold.range.end].trim().is_empty();
        if !opens_line || !closes_line {
            return None;
        }
        let inner = text[fold.range.clone()].trim();
        let single = (close_line == open_line + 2 && self.is_plain(open_line + 1))
            .then(|| self.body(text, open_line + 1).trim())
            .filter(|line| !line.is_empty());
        let shown = |line: &str| Some(format!(" {line} "));
        match self.body_of(text, head, open_line, folds) {
            Body::Type if inner.is_empty() => Some((EMPTY_TYPES, Some(String::new()))),
            Body::Function { .. } if inner.is_empty() => Some((EMPTY_FUNCTIONS, Some(String::new()))),
            Body::Function { .. } if single.is_some_and(|line| starts_with_word(line, "return")) => {
                Some((ONE_LINE_RETURNS, single.and_then(shown)))
            }
            Body::Function { declared: true } => Some((METHOD_BODIES, None)),
            Body::If if single.is_some() && handles_error(head) => Some((IF_ERRORS, single.and_then(shown))),
            _ => None,
        }
    }

    fn body_of(&self, text: &str, head: &str, line: usize, folds: &[FoldRegion]) -> Body {
        let body = self.body(text, line);
        let mut statement = body.trim_start();
        if statement.starts_with(')') {
            let close = self.lines.start(line) + (body.len() - statement.len());
            let parameters = folds.iter().find(|fold| {
                fold.range.end == close && fold.range.start > 0 && text.as_bytes()[fold.range.start - 1] == b'('
            });
            if let Some(parameters) = parameters {
                statement = self.body(text, self.lines.line_of(parameters.range.start - 1)).trim_start();
            }
        }
        if starts_with_word(statement, "func") {
            Body::Function { declared: true }
        } else if head.contains("func(") {
            Body::Function { declared: false }
        } else if ends_with_word(head, "struct") || ends_with_word(head, "interface") {
            Body::Type
        } else if starts_with_word(head, "if") {
            Body::If
        } else {
            Body::Other
        }
    }

    fn case_clauses(&self, text: &str) -> Vec<FoldRegion> {
        let starts_clause = |line: &str| starts_with_word(line, "case") || line.starts_with("default:");
        let mut found = Vec::new();
        for index in 0..self.lines.len().saturating_sub(2) {
            let line = self.body(text, index).trim_end();
            let keyword = self.lines.line(index).tokens.first().is_some_and(|token| {
                token.piece == Piece::Fixed(HighlightKind::Keyword)
                    && starts_clause(&text[self.lines.start(index) + token.start as usize..self.lines.end(index)])
            });
            if !keyword || !line.ends_with(':') {
                continue;
            }
            let statement = self.body(text, index + 1).trim();
            let next = self.body(text, index + 2).trim_start();
            if statement.is_empty()
                || starts_clause(statement)
                || statement.starts_with('}')
                || !self.is_plain(index + 1)
                || !(starts_clause(next) || next.starts_with('}'))
            {
                continue;
            }
            let start = self.lines.start(index) + line.len();
            let end = self.lines.start(index + 1) + self.body(text, index + 1).trim_end().len();
            found.push(FoldRegion {
                range: start..end,
                kind: FoldRegionKind::Block,
                placeholder: Some(format!(" {statement}")),
                option: Some(ONE_LINE_CASES),
            });
        }
        found
    }

    fn is_plain(&self, index: usize) -> bool {
        let code = |index: usize| index >= self.lines.len() || self.lines.state(index).mode == Mode::Code;
        code(index)
            && code(index + 1)
            && self.balanced(index)
            && !self.lines.line(index).tokens.iter().any(|token| token.piece == Piece::Fixed(HighlightKind::Comment))
    }

    fn balanced(&self, index: usize) -> bool {
        let mut depth = 0i32;
        for token in &self.lines.line(index).tokens {
            match token.piece {
                Piece::Bracket(b'{' | b'(' | b'[') => depth += 1,
                Piece::Bracket(b'}' | b')' | b']') => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return false;
            }
        }
        depth == 0
    }

    fn formatted_strings(&self, text: &str) -> Vec<FoldRegion> {
        let mut found = Vec::new();
        for index in 0..self.lines.len() {
            let start = self.lines.start(index);
            let body = self.body(text, index);
            let tokens = &self.lines.line(index).tokens;
            for (at, token) in tokens.iter().enumerate() {
                let word = &text[start + token.start as usize..start + token.end as usize];
                let called = matches!(token.piece, Piece::Name { .. })
                    && FORMATTERS.contains(&word)
                    && body[..token.start as usize].ends_with('.')
                    && tokens
                        .get(at + 1)
                        .is_some_and(|paren| paren.piece == Piece::Bracket(b'(') && paren.start == token.end);
                if !called {
                    continue;
                }
                let Some(arguments) = arguments(body, token.end as usize + 1) else { continue };
                let format = usize::from(word == "Fprintf");
                let Some(literal) = arguments.get(format) else { continue };
                let Some(placeholder) = substituted(&body[literal.clone()], &arguments[format + 1..], body) else {
                    continue;
                };
                let plain = |argument: &Range<usize>| {
                    let argument = &body[argument.clone()];
                    !argument.contains("/*") && !argument.ends_with("...")
                };
                if !arguments[format + 1..].iter().all(plain) {
                    continue;
                }
                let last = arguments.last().map_or(literal.end, |argument| argument.end);
                found.push(FoldRegion {
                    range: start + literal.start..start + last,
                    kind: FoldRegionKind::Block,
                    placeholder: Some(placeholder),
                    option: Some(FORMATTED_STRINGS),
                });
            }
        }
        found
    }
}

fn arguments(line: &str, from: usize) -> Option<Vec<Range<usize>>> {
    let bytes = line.as_bytes();
    let (mut depth, mut at, mut piece) = (0usize, from, from);
    let mut found = Vec::new();
    let mut push = |range: Range<usize>| {
        let text = &line[range.clone()];
        let lead = text.len() - text.trim_start().len();
        let trimmed = range.start + lead..range.start + lead + text.trim().len();
        if !trimmed.is_empty() {
            found.push(trimmed);
        }
    };
    while at < bytes.len() {
        match bytes[at] {
            quote @ (b'"' | b'\'' | b'`') => {
                at += 1;
                while at < bytes.len() && bytes[at] != quote {
                    at += if bytes[at] == b'\\' && quote != b'`' { 2 } else { 1 };
                }
                if at >= bytes.len() {
                    return None;
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth > 0 => depth -= 1,
            b')' => {
                push(piece..at);
                return Some(found);
            }
            b',' if depth == 0 => {
                push(piece..at);
                piece = at + 1;
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => return None,
            _ => {}
        }
        at += 1;
    }
    None
}

fn substituted(literal: &str, arguments: &[Range<usize>], line: &str) -> Option<String> {
    let quote = literal.chars().next().filter(|quote| matches!(quote, '"' | '`'))?;
    let content = literal.strip_prefix(quote)?.strip_suffix(quote)?;
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::from(quote);
    let (mut used, mut at) = (0, 0);
    while at < chars.len() {
        if chars[at] != '%' {
            out.push(chars[at]);
            at += 1;
            continue;
        }
        if chars.get(at + 1) == Some(&'%') {
            out.push('%');
            at += 2;
            continue;
        }
        let mut end = at + 1;
        while chars.get(end).is_some_and(|c| matches!(c, '+' | '-' | '#' | ' ' | '0'..='9' | '.')) {
            end += 1;
        }
        chars.get(end).filter(|c| c.is_ascii_alphabetic())?;
        let argument = arguments.get(used)?;
        out.push('{');
        out.push_str(&line[argument.clone()]);
        out.push('}');
        used += 1;
        at = end + 1;
    }
    out.push(quote);
    (used > 0 && used == arguments.len()).then_some(out)
}

fn handles_error(head: &str) -> bool {
    head.contains("!= nil")
        && head
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|word| word == "err" || word.ends_with("Err") || (word.ends_with("err") && word.len() <= 5))
}

fn starts_with_word(text: &str, word: &str) -> bool {
    text.strip_prefix(word).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

fn ends_with_word(text: &str, word: &str) -> bool {
    text.strip_suffix(word).is_some_and(|rest| !rest.ends_with(|c: char| c.is_alphanumeric() || c == '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(text: &str) -> Vec<(&'static str, String, Option<String>)> {
        GoLines::new(text)
            .fold_regions(text)
            .into_iter()
            .filter_map(|fold| fold.option.map(|option| (option, text[fold.range].to_owned(), fold.placeholder)))
            .collect()
    }

    fn with(text: &str, option: &str) -> Vec<(String, Option<String>)> {
        tagged(text).into_iter().filter(|(tag, ..)| *tag == option).map(|(_, hidden, shown)| (hidden, shown)).collect()
    }

    const SAMPLE: &str = "package main

import \"fmt\"

func run() error {
\tif err := work(); err != nil {
\t\treturn fmt.Errorf(\"run %s: %w\", name, err)
\t}
\tif ok {
\t\tdone()
\t}
\tswitch n {
\tcase 1:
\t\treturn nil
\tcase 2, 3:
\t\tx := 1
\t\ty := 2
\tdefault:
\t\tpanic(n)
\t}
\tfmt.Printf(\"%d of %v%%\\n\", count, len(all))
\treturn nil
}

func one() int {
\treturn 1
}

func empty() {
}

func (s *Server) Serve(
\tport int,
) error {
\tlisten(port)
\treturn nil
}

type Empty struct {
}

type None interface {
}

var handler = func(w Writer) {
}
";

    #[test]
    fn if_blocks_that_handle_an_error_show_their_line() {
        assert_eq!(
            with(SAMPLE, IF_ERRORS),
            [(
                "\n\t\treturn fmt.Errorf(\"run %s: %w\", name, err)\n\t".to_owned(),
                Some(" return fmt.Errorf(\"run %s: %w\", name, err) ".to_owned())
            )],
            "`if ok` handles no error"
        );
    }

    #[test]
    fn functions_of_one_return_show_it_and_empty_ones_nothing() {
        assert_eq!(with(SAMPLE, ONE_LINE_RETURNS), [("\n\treturn 1\n".to_owned(), Some(" return 1 ".to_owned()))]);
        assert_eq!(
            with(SAMPLE, EMPTY_FUNCTIONS),
            [("\n".to_owned(), Some(String::new())), ("\n".to_owned(), Some(String::new()))],
            "a function and a function literal"
        );
        assert_eq!(with(SAMPLE, EMPTY_TYPES), vec![("\n".to_owned(), Some(String::new())); 2]);
    }

    #[test]
    fn method_bodies_are_the_other_declared_functions() {
        let bodies = with(SAMPLE, METHOD_BODIES);
        assert_eq!(bodies.len(), 2, "run and Serve, its signature over three lines: {bodies:?}");
        assert!(bodies[0].0.starts_with("\n\tif err := work()"));
        assert_eq!(bodies[1], ("\n\tlisten(port)\n\treturn nil\n".to_owned(), None));
    }

    #[test]
    fn case_clauses_of_one_statement_show_it_after_the_colon() {
        assert_eq!(
            with(SAMPLE, ONE_LINE_CASES),
            [
                ("\n\t\treturn nil".to_owned(), Some(" return nil".to_owned())),
                ("\n\t\tpanic(n)".to_owned(), Some(" panic(n)".to_owned())),
            ],
            "case 2, 3 has two statements"
        );
    }

    #[test]
    fn format_strings_show_their_arguments_in_place_of_the_verbs() {
        assert_eq!(
            with(SAMPLE, FORMATTED_STRINGS),
            [
                ("\"run %s: %w\", name, err".to_owned(), Some("\"run {name}: {err}\"".to_owned())),
                ("\"%d of %v%%\\n\", count, len(all)".to_owned(), Some("\"{count} of {len(all)}%\\n\"".to_owned())),
            ]
        );
        for text in [
            "package p\nvar s = fmt.Sprintf(\"%d %d\", 1)\n",
            "package p\nvar s = fmt.Sprintf(\"plain\")\n",
            "package p\nvar s = fmt.Sprintf(\"%[1]d\", 1)\n",
            "package p\nvar s = fmt.Sprintf(\"%d\",\n\t1)\n",
            "package p\n// fmt.Sprintf(\"%d\", 1)\n",
            "package p\nvar s = Sprintf(\"%d\", 1)\n",
        ] {
            assert!(with(text, FORMATTED_STRINGS).is_empty(), "{text:?}");
        }
        let writer = "package p\nfunc f() { fmt.Fprintf(os.Stderr, \"%s\\n\", msg) }\n";
        assert_eq!(with(writer, FORMATTED_STRINGS), [("\"%s\\n\", msg".to_owned(), Some("\"{msg}\\n\"".to_owned()))]);
    }

    #[test]
    fn tricky_lines_get_no_placeholder() {
        let text = "package p

func a() error {
\treturn err // wrapped later
}

func b() string {
\treturn `
`
}

func c() {
\tif err != nil {
\t\treturn /* why */ err
\t}
\tif err != nil {
\t\treturn `x
\t}`
\t}
\tswitch n {
\tcase 1:
\t\ts := `
\tcase 2:
\t`
\tcase 3:
\t\tf() // note
\t}
\tfmt.Printf(\"%v\", args...)
\tfmt.Printf(\"%d\", n /* count */)
\tfmt.Printf(`%d
%d`, a, b)
\tfmt.Printf(\"100%% %[1]d %*d\", n, w, m)
\ts := `fmt.Printf(\"%d\", n)`
}
";
        let tagged = tagged(text);
        let options: Vec<&str> = tagged.iter().map(|(option, ..)| *option).collect();
        for option in [IF_ERRORS, ONE_LINE_RETURNS, ONE_LINE_CASES, FORMATTED_STRINGS] {
            assert!(!options.contains(&option), "{option}: {tagged:?}");
        }
        let tricky = "package p\nfunc f() {\n\tfmt.Printf(\"%5.2f%% of %q: \\\"%s\\\" %v\", a, `b`, c, map[string]int{\"x,\": 1})\n}\n";
        assert_eq!(
            with(tricky, FORMATTED_STRINGS),
            [(
                "\"%5.2f%% of %q: \\\"%s\\\" %v\", a, `b`, c, map[string]int{\"x,\": 1}".to_owned(),
                Some("\"{a}% of {`b`}: \\\"{c}\\\" {map[string]int{\"x,\": 1}}\"".to_owned())
            )]
        );
    }

    #[test]
    fn a_fold_in_characters_keeps_its_option_and_placeholder() {
        let text = "package p\n\n// Привет.\nfunc f() int {\n\treturn 1\n}\n";
        let regions = GoLines::new(text).fold_regions(text);
        assert_eq!(regions.len(), 1);
        assert_eq!(
            (regions[0].option, regions[0].placeholder.as_deref()),
            (Some(ONE_LINE_RETURNS), Some(" return 1 "))
        );
    }
}
