use std::ops::Range;

use serde::{Deserialize, Serialize};
use zen_text::{Point, Text};

use crate::{Selection, Selections};

/// Where a caret moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Movement {
    /// One character left; a selection collapses to its start instead.
    Left,
    /// One character right; a selection collapses to its end instead.
    Right,
    /// To the start of the word on the left (⌥←).
    WordLeft,
    /// To the end of the word on the right (⌥→).
    WordRight,
    /// To the start of the word or camel hump on the left (⌥← with "CamelHumps" words on).
    HumpLeft,
    /// To the end of the word or camel hump on the right (⌥→ with "CamelHumps" words on).
    HumpRight,
    /// One line up, keeping the column.
    Up,
    /// One line down, keeping the column.
    Down,
    /// `lines` lines up (Page Up).
    PageUp {
        /// How many lines a page is.
        lines: usize,
    },
    /// `lines` lines down (Page Down).
    PageDown {
        /// How many lines a page is.
        lines: usize,
    },
    /// To the first non-blank character of the line, or to its very start when already there (⌘←).
    LineStart,
    /// To the end of the line (⌘→).
    LineEnd,
    /// To the start of the text (⌘↑).
    DocumentStart,
    /// To the end of the text (⌘↓).
    DocumentEnd,
}

impl Movement {
    pub fn by_camel_humps(self) -> Movement {
        match self {
            Movement::WordLeft => Movement::HumpLeft,
            Movement::WordRight => Movement::HumpRight,
            other => other,
        }
    }
}

impl Selections {
    pub fn moved(&self, text: &Text, movement: Movement, extend: bool) -> Selections {
        let items = self.iter().map(|selection| move_selection(text, *selection, movement, extend)).collect();
        Selections::from_vec(items, self.primary_index())
    }
}

pub(crate) fn move_selection(text: &Text, selection: Selection, movement: Movement, extend: bool) -> Selection {
    if !extend && !selection.is_empty() {
        match movement {
            Movement::Left => return Selection::caret(selection.range().start),
            Movement::Right => return Selection::caret(selection.range().end),
            _ => {}
        }
    }
    let vertical =
        matches!(movement, Movement::Up | Movement::Down | Movement::PageUp { .. } | Movement::PageDown { .. });
    let (head, goal_column) = match movement {
        Movement::Up => vertical_move(text, selection, -1),
        Movement::Down => vertical_move(text, selection, 1),
        Movement::PageUp { lines } => vertical_move(text, selection, -(lines.max(1) as isize)),
        Movement::PageDown { lines } => vertical_move(text, selection, lines.max(1) as isize),
        other => (horizontal_target(text, selection.head, other), None),
    };
    let anchor = if extend { selection.anchor } else { head };
    Selection { anchor, head, goal_column: if vertical { goal_column } else { None } }
}

fn point(text: &Text, offset: usize) -> Point {
    text.offset_to_point(offset.min(text.len_chars())).unwrap_or_default()
}

fn line_chars(text: &Text, line: usize) -> Vec<char> {
    text.line(line).map(|line| line.chars().collect()).unwrap_or_default()
}

fn offset(text: &Text, line: usize, column: usize) -> usize {
    let len = line_chars(text, line).len();
    text.point_to_offset(Point::new(line, column.min(len))).unwrap_or_else(|_| text.len_chars())
}

fn vertical_move(text: &Text, selection: Selection, lines: isize) -> (usize, Option<usize>) {
    let here = point(text, selection.head);
    let goal = selection.goal_column.unwrap_or(here.column);
    let last = text.len_lines().saturating_sub(1);
    let target = here.line.saturating_add_signed(lines).min(last);
    if target == here.line {
        return (selection.head, Some(goal));
    }
    (offset(text, target, goal), Some(goal))
}

fn horizontal_target(text: &Text, head: usize, movement: Movement) -> usize {
    let here = point(text, head);
    let chars = line_chars(text, here.line);
    let last_line = text.len_lines().saturating_sub(1);
    match movement {
        Movement::Left => {
            if here.column > 0 {
                head - 1
            } else if here.line > 0 {
                offset(text, here.line - 1, usize::MAX)
            } else {
                head
            }
        }
        Movement::Right => {
            if here.column < chars.len() {
                head + 1
            } else if here.line < last_line {
                offset(text, here.line + 1, 0)
            } else {
                head
            }
        }
        Movement::WordLeft | Movement::HumpLeft => {
            if here.column == 0 {
                return if here.line > 0 { offset(text, here.line - 1, usize::MAX) } else { head };
            }
            let humps = movement == Movement::HumpLeft;
            head - (here.column - word_start(&chars, here.column, humps))
        }
        Movement::WordRight | Movement::HumpRight => {
            if here.column == chars.len() {
                return if here.line < last_line { offset(text, here.line + 1, 0) } else { head };
            }
            let humps = movement == Movement::HumpRight;
            head + (word_end(&chars, here.column, humps) - here.column)
        }
        Movement::LineStart => {
            let first = chars.iter().position(|c| !c.is_whitespace()).unwrap_or(chars.len());
            let column = if here.column == first { 0 } else { first };
            head - here.column + column
        }
        Movement::LineEnd => head - here.column + chars.len(),
        Movement::DocumentStart => 0,
        Movement::DocumentEnd => text.len_chars(),
        Movement::Up | Movement::Down | Movement::PageUp { .. } | Movement::PageDown { .. } => head,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Blank,
    Word,
    Symbol,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Blank
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Symbol
    }
}

pub(crate) fn word_start(chars: &[char], column: usize, humps: bool) -> usize {
    let mut i = column.min(chars.len());
    while i > 0 && class(chars[i - 1]) == Class::Blank {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    let kind = class(chars[i - 1]);
    let from = i;
    while i > 0 && class(chars[i - 1]) == kind {
        if humps && kind == Class::Word && i < from && hump_boundary(chars, i) {
            return i;
        }
        i -= 1;
    }
    i
}

pub(crate) fn word_end(chars: &[char], column: usize, humps: bool) -> usize {
    let mut i = column.min(chars.len());
    while i < chars.len() && class(chars[i]) == Class::Blank {
        i += 1;
    }
    if i == chars.len() {
        return i;
    }
    let kind = class(chars[i]);
    let from = i;
    while i < chars.len() && class(chars[i]) == kind {
        if humps && kind == Class::Word && i > from && hump_boundary(chars, i) {
            return i;
        }
        i += 1;
    }
    i
}

fn hump_boundary(chars: &[char], at: usize) -> bool {
    if at == 0 || at >= chars.len() {
        return false;
    }
    let (before, here) = (chars[at - 1], chars[at]);
    if class(before) != Class::Word || class(here) != Class::Word {
        return false;
    }
    if before == '_' && here != '_' {
        return true;
    }
    if (before.is_lowercase() || before.is_numeric()) && here.is_uppercase() {
        return true;
    }
    before.is_uppercase() && here.is_uppercase() && chars.get(at + 1).is_some_and(|next| next.is_lowercase())
}

pub fn hump_at(text: &Text, offset: usize) -> Range<usize> {
    let offset = offset.min(text.len_chars());
    let (start, end) = crate::carets::word_at(text, offset);
    if start == end {
        return offset..offset;
    }
    let chars: Vec<char> = text.slice(start..end).map(|word| word.chars().collect()).unwrap_or_default();
    let column = (offset - start).min(chars.len());
    let mut hump_start = column.min(chars.len() - 1);
    while hump_start > 0 && !hump_boundary(&chars, hump_start) {
        hump_start -= 1;
    }
    let mut hump_end = hump_start + 1;
    while hump_end < chars.len() && !hump_boundary(&chars, hump_end) {
        hump_end += 1;
    }
    while hump_end > hump_start && chars[hump_end - 1] == '_' {
        hump_end -= 1;
    }
    if hump_end == hump_start || column > hump_end {
        return start..end;
    }
    start + hump_start..start + hump_end
}

pub fn double_click_word(text: &Text, offset: usize, humps: bool) -> Range<usize> {
    let offset = offset.min(text.len_chars());
    if humps {
        return hump_at(text, offset);
    }
    let (start, end) = crate::carets::word_at(text, offset);
    if start == end { offset..offset } else { start..end }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caret(text: &Text, offset: usize, movement: Movement) -> usize {
        Selections::single(Selection::caret(offset)).moved(text, movement, false).primary().head
    }

    #[test]
    fn characters_and_line_breaks() {
        let text = Text::from("ab\r\ncd");
        assert_eq!(caret(&text, 2, Movement::Right), 4);
        assert_eq!(caret(&text, 4, Movement::Left), 2);
        assert_eq!(caret(&text, 0, Movement::Left), 0);
        assert_eq!(caret(&text, 6, Movement::Right), 6);
    }

    #[test]
    fn a_selection_collapses_before_moving() {
        let text = Text::from("hello");
        let selection = Selections::single(Selection::new(1, 4));
        assert_eq!(selection.moved(&text, Movement::Left, false).primary(), Selection::caret(1));
        assert_eq!(selection.moved(&text, Movement::Right, false).primary(), Selection::caret(4));
        assert_eq!(selection.moved(&text, Movement::Right, true).primary(), Selection::new(1, 5));
    }

    #[test]
    fn words() {
        let text = Text::from("let foo_bar = x.y(1);");
        assert_eq!(caret(&text, 0, Movement::WordRight), 3);
        assert_eq!(caret(&text, 3, Movement::WordRight), 11);
        assert_eq!(caret(&text, 11, Movement::WordRight), 13);
        assert_eq!(caret(&text, 11, Movement::WordLeft), 4);
        assert_eq!(caret(&text, 16, Movement::WordLeft), 15);
        assert_eq!(caret(&text, 21, Movement::WordLeft), 19);
    }

    #[test]
    fn camel_humps() {
        let text = Text::from("x fooBarBaz HTTPServer foo_bar SOME_CONST utf8Decode");
        let right = |from| caret(&text, from, Movement::WordRight.by_camel_humps());
        let left = |from| caret(&text, from, Movement::WordLeft.by_camel_humps());
        assert_eq!([right(1), right(5), right(8)], [5, 8, 11]);
        assert_eq!([left(11), left(8), left(5)], [8, 5, 2]);
        assert_eq!(caret(&text, 1, Movement::WordRight), 11, "off, the identifier is one word");
        assert_eq!([right(11), right(16)], [16, 22]);
        assert_eq!([right(22), right(27)], [27, 30]);
        assert_eq!([left(30), left(27)], [27, 23]);
        assert_eq!([right(30), right(36)], [36, 41]);
        assert_eq!([right(41), right(46)], [46, 52]);
        assert_eq!(caret(&text, 12, Movement::HumpLeft), 8, "blanks are skipped as without humps");
        let extended = Selections::single(Selection::caret(5)).moved(&text, Movement::HumpRight, true);
        assert_eq!(extended.primary(), Selection::new(5, 8));
        let other = Text::from("приветМир foo😀Bar URLs");
        let right = |from| caret(&other, from, Movement::HumpRight);
        assert_eq!([right(0), right(6), right(10), right(13), right(14), right(18)], [6, 9, 13, 14, 17, 20]);
        assert_eq!(hump_at(&other, 7), 6..9);
    }

    #[test]
    fn the_hump_a_double_click_selects() {
        let text = Text::from("fooBarBaz foo_bar __init x");
        assert_eq!(hump_at(&text, 4), 3..6);
        assert_eq!(hump_at(&text, 3), 3..6);
        assert_eq!(hump_at(&text, 0), 0..3);
        assert_eq!(hump_at(&text, 9), 6..9, "at the end of the identifier, its last hump");
        assert_eq!(hump_at(&text, 11), 10..13, "the hump without its `_`");
        assert_eq!(hump_at(&text, 13), 10..13, "right before the `_`, the hump it ends");
        assert_eq!(hump_at(&text, 15), 14..17);
        assert_eq!(hump_at(&text, 18), 18..24, "a leading run of `_` alone, the whole identifier");
        assert_eq!(hump_at(&text, 19), 18..24);
        assert_eq!(hump_at(&text, 21), 20..24);
        assert_eq!(hump_at(&text, 25), 25..26);
        assert_eq!(hump_at(&text, 24), 20..24, "right after a word, its hump");
        assert_eq!(double_click_word(&text, 4, false), 0..9);
        assert_eq!(double_click_word(&text, 4, true), 3..6);
        let blank = Text::from("a  b");
        assert_eq!(double_click_word(&blank, 2, false), 2..2);
        assert_eq!(double_click_word(&blank, 2, true), 2..2);
    }

    #[test]
    fn vertical_movement_keeps_the_goal_column() {
        let text = Text::from("long line\nab\nanother line");
        let start = Selections::single(Selection::caret(7));
        let down = start.moved(&text, Movement::Down, false);
        assert_eq!(down.primary().head, 12, "column 2 of the short line");
        let down = down.moved(&text, Movement::Down, false);
        assert_eq!(down.primary().head, 13 + 7, "back to column 7");
        assert_eq!(caret(&text, 3, Movement::Up), 3, "the first line stays");
        let page = Selections::single(Selection::caret(0)).moved(&text, Movement::PageDown { lines: 10 }, false);
        assert_eq!(page.primary().head, 13, "the last line, column 0");
    }

    #[test]
    fn smart_line_start_and_end() {
        let text = Text::from("    func main() {");
        assert_eq!(caret(&text, 10, Movement::LineStart), 4);
        assert_eq!(caret(&text, 4, Movement::LineStart), 0);
        assert_eq!(caret(&text, 0, Movement::LineStart), 4);
        assert_eq!(caret(&text, 0, Movement::LineEnd), 17);
        assert_eq!(caret(&text, 5, Movement::DocumentEnd), 17);
        assert_eq!(caret(&text, 5, Movement::DocumentStart), 0);
    }

    #[test]
    fn every_caret_moves() {
        let text = Text::from("abc\nabc\nabc");
        let carets = Selections::from_vec(vec![Selection::caret(1), Selection::caret(5), Selection::caret(9)], 0);
        let moved = carets.moved(&text, Movement::LineEnd, false);
        let heads: Vec<_> = moved.iter().map(|selection| selection.head).collect();
        assert_eq!(heads, [3, 7, 11]);
        assert_eq!(carets.moved(&text, Movement::DocumentStart, false).len(), 1);
    }
}
