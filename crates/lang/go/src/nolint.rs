use zen_syntax::SuppressedLines;

use crate::{Token, tokenize};

const WHOLE_FILE: std::ops::RangeInclusive<u32> = 1..=u32::MAX;

struct Code {
    first_line: u32,
    last_line: u32,
    depth: i32,
    package: bool,
}

fn ids_of(comment: &str) -> Option<Vec<String>> {
    let rest = comment.strip_prefix("//")?.trim_start().strip_prefix("nolint")?;
    let list = match rest.chars().next() {
        None => return Some(Vec::new()),
        Some(c) if c.is_whitespace() => return Some(Vec::new()),
        Some(':') => rest[1..].split("//").next().unwrap_or_default(),
        Some(_) => return None,
    };
    let ids: Vec<String> = list.split(',').filter_map(|id| id.split_whitespace().next()).map(str::to_owned).collect();
    Some(if ids.iter().any(|id| id == "all") { Vec::new() } else { ids })
}

fn read(text: &str) -> (Vec<Code>, Vec<(u32, Vec<String>)>) {
    let mut code = Vec::new();
    let mut directives = Vec::new();
    let mut line = 1;
    for (token, start, end) in tokenize(text) {
        let piece = &text[start..end];
        let last_line = line + piece.matches('\n').count() as u32;
        match token {
            Token::Whitespace => {}
            Token::Comment => {
                if let Some(ids) = ids_of(piece) {
                    directives.push((line, ids));
                }
            }
            token => {
                let depth = match token {
                    Token::Punct('(' | '[' | '{') => 1,
                    Token::Punct(')' | ']' | '}') => -1,
                    _ => 0,
                };
                let package = matches!(token, Token::Ident) && piece == "package";
                code.push(Code { first_line: line, last_line, depth, package });
            }
        }
        line = last_line;
    }
    (code, directives)
}

fn last_line_of(code: &[Code]) -> Option<u32> {
    let mut depth = 0;
    let mut last = code.first()?.last_line;
    for token in code {
        if token.first_line > last && depth <= 0 {
            break;
        }
        depth += token.depth;
        last = last.max(token.last_line);
    }
    Some(last)
}

fn reach(code: &[Code], line: u32) -> std::ops::RangeInclusive<u32> {
    let at = code.partition_point(|token| token.last_line < line);
    let target = match code.get(at) {
        Some(token) if token.first_line <= line => line,
        Some(token) if token.first_line == line + 1 => line + 1,
        _ => return line..=line,
    };
    let from = code[at..].iter().position(|token| token.last_line >= target).map_or(at, |skip| at + skip);
    if code[from].package {
        return WHOLE_FILE;
    }
    line.min(code[from].first_line)..=last_line_of(&code[from..]).unwrap_or(line)
}

pub(crate) fn suppressions(text: &str) -> Vec<SuppressedLines> {
    if !text.contains("nolint") {
        return Vec::new();
    }
    let (code, directives) = read(text);
    directives.into_iter().map(|(line, ids)| SuppressedLines { lines: reach(&code, line), ids }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(u32, u32, Vec<String>)> {
        suppressions(text).into_iter().map(|one| (*one.lines.start(), *one.lines.end(), one.ids)).collect()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn a_directive_at_the_end_of_a_line_covers_that_line() {
        let text = "package a\n\nvar x = 1 //nolint:unused,shadow // kept for a test\nvar y = 2\n";
        assert_eq!(found(text), [(3, 3, ids(&["unused", "shadow"]))]);
    }

    #[test]
    fn a_directive_above_a_function_covers_the_whole_function() {
        let text = "package a\n\n//nolint:unusedfunc\nfunc f() {\n\tg(\n\t\t1,\n\t)\n}\n\nfunc h() {}\n";
        assert_eq!(found(text), [(3, 8, ids(&["unusedfunc"]))]);
    }

    #[test]
    fn a_directive_on_the_line_of_a_block_covers_the_block() {
        let text = "package a\n\nfunc f() {\n\tif x { //nolint:bools\n\t\ty()\n\t} else {\n\t\tz()\n\t}\n\tw()\n}\n";
        assert_eq!(found(text), [(4, 8, ids(&["bools"]))]);
    }

    #[test]
    fn a_directive_above_the_package_clause_covers_the_file() {
        let text = "//nolint:unusedfunc\npackage a\n\nfunc f() {}\n";
        assert_eq!(found(text), [(1, u32::MAX, ids(&["unusedfunc"]))]);
    }

    #[test]
    fn a_directive_with_a_blank_line_after_it_covers_only_its_own_line() {
        let text = "package a\n\n//nolint:unused\n\nvar x = 1\n";
        assert_eq!(found(text), [(3, 3, ids(&["unused"]))]);
    }

    #[test]
    fn a_bare_directive_and_all_name_every_inspection() {
        let text = "package a\n\nvar x = 1 //nolint\nvar y = 2 //nolint:all\nvar z = 3 // nolint because\n";
        assert_eq!(found(text), [(3, 3, ids(&[])), (4, 4, ids(&[])), (5, 5, ids(&[]))]);
    }

    #[test]
    fn a_directive_inside_a_string_or_a_longer_word_is_not_one() {
        let text = "package a\n\nvar x = \"//nolint:unused\"\n//nolintlint is another tool\nvar y = 1\n";
        assert!(found(text).is_empty());
    }

    #[test]
    fn spaces_after_a_comma_and_a_reason_after_the_list_are_read() {
        let text = "package a\n\nvar x = 1 //nolint:unused, shadow // why\n";
        assert_eq!(found(text), [(3, 3, ids(&["unused", "shadow"]))]);
    }

    #[test]
    fn a_raw_string_over_several_lines_counts_its_lines() {
        let text = "package a\n\nvar x = `one\ntwo` //nolint:unused\nvar y = 1\n";
        assert_eq!(found(text), [(3, 4, ids(&["unused"]))]);
    }
}
