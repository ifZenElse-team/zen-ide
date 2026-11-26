use std::ops::Range;

use zen_text::{Edit, Point, Text};

use crate::{Selection, Selections};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Block {
    first: usize,
    last: usize,
}

fn line_len(text: &Text, line: usize) -> usize {
    text.line(line).map(|line| line.chars().count()).unwrap_or_default()
}

fn line_start(text: &Text, line: usize) -> usize {
    text.point_to_offset(Point::new(line, 0)).unwrap_or_else(|_| text.len_chars())
}

fn line_end(text: &Text, line: usize) -> usize {
    line_start(text, line) + line_len(text, line)
}

fn next_line_start(text: &Text, line: usize) -> usize {
    if line + 1 < text.len_lines() { line_start(text, line + 1) } else { text.len_chars() }
}

fn point(text: &Text, offset: usize) -> Point {
    text.offset_to_point(offset.min(text.len_chars())).unwrap_or_default()
}

fn blocks(text: &Text, selections: &Selections) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    for selection in selections.iter() {
        let range = selection.range();
        let first = point(text, range.start).line;
        let end = point(text, range.end);
        let last = if !selection.is_empty() && end.column == 0 && end.line > first { end.line - 1 } else { end.line };
        match blocks.last_mut() {
            Some(block) if first <= block.last + 1 && first >= block.first => block.last = block.last.max(last),
            _ => blocks.push(Block { first, last }),
        }
    }
    blocks
}

fn sequential(mut replacements: Vec<(Range<usize>, String)>) -> Vec<Edit> {
    replacements.sort_by_key(|(range, _)| range.start);
    replacements.into_iter().rev().map(|(range, text)| Edit::replace(range, text)).collect()
}

type Tagged = Vec<(Range<usize>, String, usize, u8)>;

fn placed(mut tagged: Tagged) -> (Tagged, Vec<usize>) {
    tagged.sort_by_key(|(range, _, _, part)| (range.start, *part, range.end));
    let mut shift: isize = 0;
    let mut starts = Vec::with_capacity(tagged.len());
    for (range, text, _, _) in &tagged {
        starts.push(range.start.saturating_add_signed(shift));
        shift += text.chars().count() as isize - range.len() as isize;
    }
    (tagged, starts)
}

fn start_of(tagged: &Tagged, starts: &[usize], selection: usize, part: u8) -> usize {
    tagged.iter().position(|(_, _, s, p)| *s == selection && *p == part).map_or(0, |index| starts[index])
}

fn edits_of(tagged: &Tagged) -> Vec<Edit> {
    tagged.iter().rev().map(|(range, text, _, _)| Edit::replace(range.clone(), text.clone())).collect()
}

fn map_offset(offset: usize, replacements: &[(Range<usize>, String)]) -> usize {
    let mut shift: isize = 0;
    for (range, text) in replacements {
        if range.end <= offset && !(range.is_empty() && range.start == offset) {
            shift += text.chars().count() as isize - range.len() as isize;
        } else if range.start < offset {
            return range.start.saturating_add_signed(shift) + text.chars().count();
        }
    }
    offset.saturating_add_signed(shift)
}

pub fn duplicate(text: &Text, selections: &Selections) -> (Vec<Edit>, Selections) {
    if selections.iter().any(|selection| !selection.is_empty()) {
        let mut replacements = Vec::new();
        let mut copies = Vec::new();
        for selection in selections.iter() {
            let range = selection.range();
            let piece = text.slice(range.clone()).map(|slice| slice.into_owned()).unwrap_or_default();
            replacements.push((range.end..range.end, piece.clone()));
            copies.push((range.end, piece.chars().count()));
        }
        replacements.sort_by_key(|(range, _)| range.start);
        let items = copies
            .iter()
            .map(|&(at, len)| {
                let start = map_offset(at, &replacements);
                Selection::new(start, start + len)
            })
            .collect();
        return (sequential(replacements), Selections::from_vec(items, selections.primary_index()));
    }
    let mut replacements = Vec::new();
    for block in blocks(text, selections) {
        let start = line_start(text, block.first);
        let end = line_end(text, block.last);
        let body = text.slice(start..end).map(|slice| slice.into_owned()).unwrap_or_default();
        replacements.push((end..end, format!("\n{body}")));
    }
    let lines_added = |line: usize| {
        blocks(text, selections)
            .iter()
            .take_while(|block| block.first <= line)
            .map(|block| block.last - block.first + 1)
            .sum::<usize>()
    };
    let items = selections
        .iter()
        .map(|selection| {
            let here = point(text, selection.head);
            let line = here.line + lines_added(here.line);
            Selection::caret(here.column + line_start_after(text, line, &replacements))
        })
        .collect();
    (sequential(replacements), Selections::from_vec(items, selections.primary_index()))
}

fn line_start_after(text: &Text, line: usize, replacements: &[(Range<usize>, String)]) -> usize {
    let (after, _) = text.apply(&sequential(replacements.to_vec())).unwrap_or((text.clone(), Vec::new()));
    line_start(&after, line.min(after.len_lines().saturating_sub(1)))
}

pub fn delete_lines(text: &Text, selections: &Selections) -> (Vec<Edit>, Selections) {
    let mut replacements = Vec::new();
    for block in blocks(text, selections) {
        let start = line_start(text, block.first);
        let mut end = next_line_start(text, block.last);
        let start = if end == text.len_chars() && block.last + 1 >= text.len_lines() && block.first > 0 {
            end = text.len_chars();
            line_end(text, block.first - 1)
        } else {
            start
        };
        replacements.push((start..end, String::new()));
    }
    let edits = sequential(replacements.clone());
    let (after, _) = text.apply(&edits).unwrap_or((text.clone(), Vec::new()));
    let items = selections
        .iter()
        .map(|selection| {
            let column = point(text, selection.head).column;
            let at = point(&after, map_offset(selection.range().start, &replacements));
            Selection::caret(line_start(&after, at.line) + column.min(line_len(&after, at.line)))
        })
        .collect();
    (edits, Selections::from_vec(items, selections.primary_index()))
}

pub fn move_lines(text: &Text, selections: &Selections, down: bool) -> Option<(Vec<Edit>, Selections)> {
    let blocks = blocks(text, selections);
    let last_line = text.len_lines().saturating_sub(1);
    if blocks.iter().any(|block| if down { block.last >= last_line } else { block.first == 0 }) {
        return None;
    }
    let mut replacements = Vec::new();
    for block in &blocks {
        let (first, last) = if down { (block.first, block.last + 1) } else { (block.first - 1, block.last) };
        let lines: Vec<String> =
            (first..=last).map(|line| text.line(line).map(|l| l.into_owned()).unwrap_or_default()).collect();
        let rotated: Vec<&String> = if down {
            std::iter::once(&lines[lines.len() - 1]).chain(&lines[..lines.len() - 1]).collect()
        } else {
            lines[1..].iter().chain(std::iter::once(&lines[0])).collect()
        };
        let joined = rotated.iter().map(|line| line.as_str()).collect::<Vec<_>>().join("\n");
        replacements.push((line_start(text, first)..line_end(text, last), joined));
    }
    let edits = sequential(replacements);
    let (after, _) = text.apply(&edits).ok()?;
    let shift_point = |offset: usize| {
        let here = point(text, offset);
        let line = if down { here.line + 1 } else { here.line - 1 };
        line_start(&after, line) + here.column.min(line_len(&after, line))
    };
    let items = selections
        .iter()
        .map(|selection| Selection::new(shift_point(selection.anchor), shift_point(selection.head)))
        .collect();
    Some((edits, Selections::from_vec(items, selections.primary_index())))
}

pub fn toggle_line_comment(text: &Text, selections: &Selections, prefix: &str) -> (Vec<Edit>, Selections) {
    let mut replacements = Vec::new();
    for block in blocks(text, selections) {
        let lines: Vec<(usize, String)> = (block.first..=block.last)
            .map(|line| (line, text.line(line).map(|l| l.into_owned()).unwrap_or_default()))
            .collect();
        let commented =
            lines.iter().filter(|(_, l)| !l.trim().is_empty()).all(|(_, l)| l.trim_start().starts_with(prefix))
                && lines.iter().any(|(_, l)| !l.trim().is_empty());
        for (line, content) in lines {
            let start = line_start(text, line);
            if commented {
                let indent = content.chars().take_while(|c| c.is_whitespace()).count();
                if content.trim().is_empty() {
                    continue;
                }
                let mut len = prefix.chars().count();
                if content.chars().nth(indent + len) == Some(' ') {
                    len += 1;
                }
                replacements.push((start + indent..start + indent + len, String::new()));
            } else {
                replacements.push((start..start, format!("{prefix} ")));
            }
        }
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let items = selections
        .iter()
        .map(|selection| {
            Selection::new(map_offset(selection.anchor, &replacements), map_offset(selection.head, &replacements))
        })
        .collect();
    (sequential(replacements), Selections::from_vec(items, selections.primary_index()))
}

pub fn toggle_block_comment(text: &Text, selections: &Selections, open: &str, close: &str) -> (Vec<Edit>, Selections) {
    let (open_len, close_len) = (open.chars().count(), close.chars().count());
    let mut tagged: Tagged = Vec::new();
    let mut unwrap = Vec::new();
    for (index, selection) in selections.iter().enumerate() {
        let range = selection.range();
        let inside = text.slice(range.clone()).map(|slice| slice.into_owned()).unwrap_or_default();
        let wrapped =
            inside.starts_with(open) && inside.ends_with(close) && inside.chars().count() >= open_len + close_len;
        unwrap.push(wrapped);
        if wrapped {
            tagged.push((range.start..range.start + open_len, String::new(), index, 0));
            tagged.push((range.end - close_len..range.end, String::new(), index, 1));
        } else {
            tagged.push((range.start..range.start, open.to_owned(), index, 0));
            tagged.push((range.end..range.end, close.to_owned(), index, 1));
        }
    }
    let (tagged, starts) = placed(tagged);
    let items = selections
        .iter()
        .enumerate()
        .map(|(index, selection)| {
            let (start, end) = if unwrap[index] {
                (start_of(&tagged, &starts, index, 0), start_of(&tagged, &starts, index, 1))
            } else {
                (start_of(&tagged, &starts, index, 0) + open_len, start_of(&tagged, &starts, index, 1))
            };
            if selection.head >= selection.anchor { Selection::new(start, end) } else { Selection::new(end, start) }
        })
        .collect();
    (edits_of(&tagged), Selections::from_vec(items, selections.primary_index()))
}

pub fn indent(text: &Text, selections: &Selections, unit: &str, outdent: bool) -> (Vec<Edit>, Selections) {
    let mut replacements = Vec::new();
    let unit_columns = if unit == "\t" { 4 } else { unit.chars().count().max(1) };
    for block in blocks(text, selections) {
        for line in block.first..=block.last {
            let content = text.line(line).map(|l| l.into_owned()).unwrap_or_default();
            let start = line_start(text, line);
            if outdent {
                let mut take = 0;
                let mut columns = 0;
                for c in content.chars() {
                    match c {
                        '\t' if columns == 0 => {
                            take += 1;
                            break;
                        }
                        ' ' if columns < unit_columns => {
                            take += 1;
                            columns += 1;
                        }
                        _ => break,
                    }
                }
                if take > 0 {
                    replacements.push((start..start + take, String::new()));
                }
            } else if !content.trim().is_empty() {
                replacements.push((start..start, unit.to_owned()));
            }
        }
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let items = selections
        .iter()
        .map(|selection| {
            Selection::new(map_offset(selection.anchor, &replacements), map_offset(selection.head, &replacements))
        })
        .collect();
    (sequential(replacements), Selections::from_vec(items, selections.primary_index()))
}

pub fn smart_newline(text: &Text, selections: &Selections, unit: &str) -> (Vec<Edit>, Selections) {
    smart_newline_continuing(text, selections, unit, unit)
}

pub fn smart_newline_continuing(
    text: &Text,
    selections: &Selections,
    unit: &str,
    continuation: &str,
) -> (Vec<Edit>, Selections) {
    let mut tagged: Tagged = Vec::new();
    let mut offsets = Vec::new();
    for (index, selection) in selections.iter().enumerate() {
        let range = selection.range();
        let here = point(text, range.start);
        let line = text.line(here.line).map(|l| l.into_owned()).unwrap_or_default();
        let chars: Vec<char> = line.chars().collect();
        let indent: String = chars.iter().take_while(|c| **c == ' ' || **c == '\t').collect();
        let before = chars[..here.column.min(chars.len())].iter().rev().find(|c| !c.is_whitespace()).copied();
        let after_point = point(text, range.end);
        let after = if after_point.line == here.line { chars.get(after_point.column).copied() } else { None };
        let closes =
            matches!((before, after), (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']')));
        let inner = match before {
            Some('(' | '[') => format!("{indent}{continuation}"),
            Some('{') => format!("{indent}{unit}"),
            _ => indent.clone(),
        };
        let inserted = if closes { format!("\n{inner}\n{indent}") } else { format!("\n{inner}") };
        offsets.push(1 + inner.chars().count());
        tagged.push((range, inserted, index, 0));
    }
    let (tagged, starts) = placed(tagged);
    let items = (0..selections.len())
        .map(|index| Selection::caret(start_of(&tagged, &starts, index, 0) + offsets[index]))
        .collect();
    (edits_of(&tagged), Selections::from_vec(items, selections.primary_index()))
}

fn closing(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

pub fn type_paired(text: &Text, selections: &Selections, typed: char) -> Option<(Vec<Edit>, Selections)> {
    enum Plan {
        StepOver(usize),
        Wrap,
        Pair,
    }
    let char_at =
        |offset: usize| text.slice(offset..(offset + 1).min(text.len_chars())).ok().and_then(|s| s.chars().next());
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let quote = matches!(typed, '"' | '\'' | '`');
    let mut tagged: Tagged = Vec::new();
    let mut plans = Vec::new();
    for (index, selection) in selections.iter().enumerate() {
        let range = selection.range();
        let next = char_at(range.end);
        let previous = range.start.checked_sub(1).and_then(char_at);
        if selection.is_empty() && next == Some(typed) && (matches!(typed, ')' | ']' | '}') || quote) {
            plans.push(Plan::StepOver(range.end + 1));
            continue;
        }
        let close = closing(typed)?;
        if !selection.is_empty() {
            tagged.push((range.start..range.start, typed.to_string(), index, 0));
            tagged.push((range.end..range.end, close.to_string(), index, 1));
            plans.push(Plan::Wrap);
            continue;
        }
        if is_word(next) || (quote && is_word(previous)) {
            return None;
        }
        tagged.push((range, format!("{typed}{close}"), index, 0));
        plans.push(Plan::Pair);
    }
    let replacements: Vec<(Range<usize>, String)> =
        tagged.iter().map(|(range, text, _, _)| (range.clone(), text.clone())).collect();
    let mut sorted = replacements.clone();
    sorted.sort_by_key(|(range, _)| range.start);
    let (tagged, starts) = placed(tagged);
    let items = plans
        .iter()
        .enumerate()
        .map(|(index, plan)| match plan {
            Plan::StepOver(at) => Selection::caret(map_offset(*at, &sorted)),
            Plan::Wrap => {
                Selection::new(start_of(&tagged, &starts, index, 0) + 1, start_of(&tagged, &starts, index, 1))
            }
            Plan::Pair => Selection::caret(start_of(&tagged, &starts, index, 0) + 1),
        })
        .collect();
    Some((edits_of(&tagged), Selections::from_vec(items, selections.primary_index())))
}

pub fn delete_pair(text: &Text, selections: &Selections) -> Option<(Vec<Edit>, Selections)> {
    let char_at =
        |offset: usize| text.slice(offset..(offset + 1).min(text.len_chars())).ok().and_then(|s| s.chars().next());
    let mut replacements = Vec::new();
    for selection in selections.iter() {
        let head = selection.head;
        if !selection.is_empty() || head == 0 {
            return None;
        }
        let (Some(open), Some(next)) = (char_at(head - 1), char_at(head)) else { return None };
        if closing(open) != Some(next) {
            return None;
        }
        replacements.push((head - 1..head + 1, String::new()));
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let items = replacements.iter().enumerate().map(|(i, (range, _))| Selection::caret(range.start - 2 * i)).collect();
    Some((sequential(replacements), Selections::from_vec(items, selections.primary_index())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(
        text: &str,
        selections: Vec<Selection>,
        action: impl Fn(&Text, &Selections) -> (Vec<Edit>, Selections),
    ) -> (String, Vec<(usize, usize)>) {
        let text = Text::from(text);
        let (edits, after) = action(&text, &Selections::from_vec(selections, 0));
        let (result, _) = text.apply(&edits).unwrap();
        (result.to_string(), after.iter().map(|selection| (selection.range().start, selection.range().end)).collect())
    }

    #[test]
    fn duplicates_lines_and_selections() {
        let (text, carets) = apply("a\nbc\nd", vec![Selection::caret(3)], duplicate);
        assert_eq!(text, "a\nbc\nbc\nd");
        assert_eq!(carets, [(6, 6)], "the caret goes down with the copy, same column");
        let (text, carets) = apply("let x", vec![Selection::new(4, 5)], duplicate);
        assert_eq!(text, "let xx");
        assert_eq!(carets, [(5, 6)], "the copy is selected");
    }

    #[test]
    fn deletes_lines() {
        assert_eq!(apply("a\nbc\nd", vec![Selection::caret(3)], delete_lines).0, "a\nd");
        assert_eq!(
            apply("a\nbc\nd", vec![Selection::caret(5)], delete_lines).0,
            "a\nbc",
            "the last line takes its break"
        );
        assert_eq!(apply("a\nbc\nd", vec![Selection::new(0, 4)], delete_lines).0, "d");
        assert_eq!(
            apply("a\nbc\nd\n", vec![Selection::new(2, 5)], delete_lines).0,
            "a\nd\n",
            "a selection ending at a line start keeps that line"
        );
    }

    #[test]
    fn moves_lines() {
        let text = Text::from("one\ntwo\nthree");
        let caret = Selections::single(Selection::caret(5));
        let (edits, after) = move_lines(&text, &caret, true).unwrap();
        assert_eq!(text.apply(&edits).unwrap().0.to_string(), "one\nthree\ntwo");
        assert_eq!(after.primary().head, 11, "the caret goes with its line");
        let (edits, _) = move_lines(&text, &caret, false).unwrap();
        assert_eq!(text.apply(&edits).unwrap().0.to_string(), "two\none\nthree");
        assert!(move_lines(&text, &Selections::single(Selection::caret(0)), false).is_none());
    }

    #[test]
    fn toggles_line_comments() {
        let comment = |text: &Text, s: &Selections| toggle_line_comment(text, s, "//");
        let (text, _) = apply("\tx := 1\n\ty := 2", vec![Selection::new(0, 15)], comment);
        assert_eq!(text, "// \tx := 1\n// \ty := 2");
        let (back, _) = apply(&text, vec![Selection::new(0, 21)], comment);
        assert_eq!(back, "\tx := 1\n\ty := 2");
        let (text, carets) = apply("a\nb", vec![Selection::caret(1)], comment);
        assert_eq!(text, "// a\nb");
        assert_eq!(carets, [(4, 4)], "the caret stays after `a`");
    }

    #[test]
    fn toggles_block_comments() {
        let block = |text: &Text, s: &Selections| toggle_block_comment(text, s, "/*", "*/");
        let (text, carets) = apply("a b c", vec![Selection::new(2, 3)], block);
        assert_eq!(text, "a /*b*/ c");
        assert_eq!(carets, [(4, 5)]);
        assert_eq!(apply("a /*b*/ c", vec![Selection::new(2, 7)], block).0, "a b c");
    }

    #[test]
    fn a_continuation_indent_applies_after_a_bracket_or_parenthesis_only() {
        let text = Text::from("foo(\nbar{");
        let enter = |offset| {
            let carets = Selections::from_vec(vec![Selection::caret(offset)], 0);
            smart_newline_continuing(&text, &carets, "  ", "    ")
        };
        let inserted = |(edits, _): (Vec<Edit>, Selections)| edits[0].text.clone();
        assert_eq!(inserted(enter(4)), "\n    ");
        assert_eq!(inserted(enter(9)), "\n  ");
    }

    #[test]
    fn indents_and_outdents() {
        let (text, _) = apply("a\n\nb", vec![Selection::new(0, 4)], |t, s| indent(t, s, "\t", false));
        assert_eq!(text, "\ta\n\n\tb", "blank lines stay empty");
        let (text, _) = apply("\ta\n    b\n  c", vec![Selection::new(0, 12)], |t, s| indent(t, s, "\t", true));
        assert_eq!(text, "a\nb\nc");
    }

    #[test]
    fn smart_enter() {
        let enter = |text: &Text, s: &Selections| smart_newline(text, s, "\t");
        let (text, carets) = apply("\tif x {}", vec![Selection::caret(7)], enter);
        assert_eq!(text, "\tif x {\n\t\t\n\t}");
        assert_eq!(carets, [(10, 10)], "the caret on the indented middle line");
        let (text, carets) = apply("\tx := 1", vec![Selection::caret(7)], enter);
        assert_eq!(text, "\tx := 1\n\t");
        assert_eq!(carets, [(9, 9)]);
    }

    #[test]
    fn paired_brackets_and_quotes() {
        let typed = |c: char| move |text: &Text, s: &Selections| type_paired(text, s, c).unwrap();
        assert_eq!(apply("f", vec![Selection::caret(1)], typed('(')), ("f()".to_owned(), vec![(2, 2)]));
        assert_eq!(apply("f()", vec![Selection::caret(2)], typed(')')), ("f()".to_owned(), vec![(3, 3)]), "steps over");
        assert_eq!(
            apply("a b", vec![Selection::new(2, 3)], typed('"')),
            ("a \"b\"".to_owned(), vec![(3, 4)]),
            "wraps a selection"
        );
        let text = Text::from("x");
        assert!(type_paired(&text, &Selections::single(Selection::caret(0)), '(').is_none(), "not before a word");
        assert!(
            type_paired(&text, &Selections::single(Selection::caret(1)), '"').is_none(),
            "no quote pair after a word"
        );
        assert!(type_paired(&text, &Selections::single(Selection::caret(1)), 'a').is_none());
        let pair = Text::from("f()");
        let (edits, carets) = delete_pair(&pair, &Selections::single(Selection::caret(2))).unwrap();
        assert_eq!(pair.apply(&edits).unwrap().0.to_string(), "f");
        assert_eq!(carets.primary().head, 1);
        assert!(delete_pair(&Text::from("f(x)"), &Selections::single(Selection::caret(3))).is_none());
    }
}
