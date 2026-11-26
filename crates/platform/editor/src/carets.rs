use zen_text::{Point, Text};

use crate::{Selection, Selections};

/// A direction for cloning carets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    /// To the line above.
    Up,
    /// To the line below.
    Down,
}

impl Selections {
    pub fn toggled_caret(&self, offset: usize) -> Selections {
        let existing = self.iter().position(|selection| selection.is_empty() && selection.head == offset);
        match existing {
            Some(index) if self.len() > 1 => {
                let items: Vec<Selection> =
                    self.iter().enumerate().filter(|(i, _)| *i != index).map(|(_, selection)| *selection).collect();
                let primary = items.len() - 1;
                Selections::from_vec(items, primary)
            }
            Some(_) => self.clone(),
            None => {
                let mut selections = self.clone();
                selections.add(Selection::caret(offset));
                selections
            }
        }
    }

    pub fn with_next_occurrence(&self, text: &Text) -> Selections {
        let primary = self.primary();
        if primary.is_empty() {
            let (start, end) = word_at(text, primary.head);
            if start == end {
                return self.clone();
            }
            let mut items: Vec<Selection> = self.iter().copied().collect();
            items[self.primary_index()] = Selection::new(start, end);
            return Selections::from_vec(items, self.primary_index());
        }
        let needle: Vec<char> = text.slice(primary.range()).map(|s| s.chars().collect()).unwrap_or_default();
        let haystack: Vec<char> = text.to_string().chars().collect();
        let after = self.iter().map(|selection| selection.range().end).max().unwrap_or(0);
        let taken = |start: usize| self.iter().any(|selection| selection.range().start == start);
        let found = find_from(&haystack, &needle, after)
            .filter(|start| !taken(*start))
            .or_else(|| find_from(&haystack, &needle, 0).filter(|start| !taken(*start)));
        match found {
            Some(start) => {
                let mut selections = self.clone();
                selections.add(Selection::new(start, start + needle.len()));
                selections
            }
            None => self.clone(),
        }
    }

    pub fn without_primary(&self) -> Selections {
        if self.len() == 1 {
            return self.clone();
        }
        let index = self.primary_index();
        let items: Vec<Selection> =
            self.iter().enumerate().filter(|(i, _)| *i != index).map(|(_, selection)| *selection).collect();
        Selections::from_vec(items, index.saturating_sub(1))
    }

    pub fn with_all_occurrences(&self, text: &Text) -> Selections {
        let primary = self.primary();
        let range = if primary.is_empty() {
            let (start, end) = word_at(text, primary.head);
            start..end
        } else {
            primary.range()
        };
        if range.is_empty() {
            return self.clone();
        }
        let needle: Vec<char> = text.slice(range.clone()).map(|s| s.chars().collect()).unwrap_or_default();
        let haystack: Vec<char> = text.to_string().chars().collect();
        let mut items = Vec::new();
        let mut from = 0;
        while let Some(start) = find_from(&haystack, &needle, from) {
            items.push(Selection::new(start, start + needle.len()));
            from = start + needle.len().max(1);
        }
        let primary = items.iter().position(|selection| selection.range() == range).unwrap_or(0);
        Selections::from_vec(items, primary)
    }

    pub fn with_cloned_caret(&self, text: &Text, direction: Direction) -> Selections {
        let edge = match direction {
            Direction::Up => self.iter().min_by_key(|selection| selection.head),
            Direction::Down => self.iter().max_by_key(|selection| selection.head),
        };
        let Some(edge) = edge else { return self.clone() };
        let point = text.offset_to_point(edge.head).unwrap_or_default();
        let column = edge.goal_column.unwrap_or(point.column);
        let line = match direction {
            Direction::Up if point.line > 0 => point.line - 1,
            Direction::Down if point.line + 1 < text.len_lines() => point.line + 1,
            _ => return self.clone(),
        };
        let mut selections = self.clone();
        let mut caret = Selection::caret(offset_at(text, line, column));
        caret.goal_column = Some(column);
        selections.add(caret);
        selections
    }
}

pub fn column_selection(text: &Text, anchor: Point, head: Point) -> Selections {
    let (top, bottom) = (anchor.line.min(head.line), anchor.line.max(head.line));
    let items: Vec<Selection> = (top..=bottom.min(text.len_lines().saturating_sub(1)))
        .map(|line| Selection::new(offset_at(text, line, anchor.column), offset_at(text, line, head.column)))
        .collect();
    let primary = if head.line >= anchor.line { items.len().saturating_sub(1) } else { 0 };
    Selections::from_vec(items, primary)
}

pub fn column_head(text: &Text, head: Point, movement: crate::Movement) -> Point {
    use crate::Movement;
    let last_line = text.len_lines().saturating_sub(1);
    let line_len = |line: usize| {
        text.line(line).map(|line| line.trim_end_matches(['\n', '\r']).chars().count()).unwrap_or_default()
    };
    match movement {
        Movement::Left => Point::new(head.line, head.column.saturating_sub(1)),
        Movement::Right => Point::new(head.line, head.column + 1),
        Movement::Up => Point::new(head.line.saturating_sub(1), head.column),
        Movement::Down => Point::new((head.line + 1).min(last_line), head.column),
        Movement::PageUp { lines } => Point::new(head.line.saturating_sub(lines.max(1)), head.column),
        Movement::PageDown { lines } => Point::new((head.line + lines.max(1)).min(last_line), head.column),
        Movement::LineEnd => Point::new(head.line, line_len(head.line)),
        other => {
            let caret = Selection::caret(offset_at(text, head.line, head.column));
            let moved = crate::movement::move_selection(text, caret, other, false);
            text.offset_to_point(moved.head).unwrap_or(head)
        }
    }
}

fn offset_at(text: &Text, line: usize, column: usize) -> usize {
    let len = text.line(line).map(|line| line.chars().count()).unwrap_or_default();
    text.point_to_offset(Point::new(line, column.min(len))).unwrap_or_default()
}

pub(crate) fn word_at(text: &Text, offset: usize) -> (usize, usize) {
    let point = text.offset_to_point(offset).unwrap_or_default();
    let chars: Vec<char> = text.line(point.line).map(|line| line.chars().collect()).unwrap_or_default();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let column = point.column.min(chars.len());
    let line_start = offset - point.column;
    let mut start = column;
    while start > 0 && word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = column;
    while end < chars.len() && word(chars[end]) {
        end += 1;
    }
    (line_start + start, line_start + end)
}

fn find_from(haystack: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&start| haystack[start..start + needle.len()] == *needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(selections: &Selections) -> Vec<std::ops::Range<usize>> {
        selections.iter().map(Selection::range).collect()
    }

    #[test]
    fn next_occurrence_selects_the_word_then_its_occurrences() {
        let text = Text::from("item := items[i]\nuse(item)\nitem = 1");
        let start = Selections::single(Selection::caret(1));
        let first = start.with_next_occurrence(&text);
        assert_eq!(first.primary().range(), 0..4);
        assert_eq!(first.len(), 1);
        let second = first.with_next_occurrence(&text);
        assert_eq!(ranges(&second), [0..4, 8..12], "\"items\" contains it too");
        let third = second.with_next_occurrence(&text).with_next_occurrence(&text);
        assert_eq!(ranges(&third), [0..4, 8..12, 21..25, 27..31]);
        assert_eq!(third.with_next_occurrence(&text), third, "every occurrence is taken");
        assert_eq!(ranges(&third.without_primary()), [0..4, 8..12, 21..25]);
    }

    #[test]
    fn all_occurrences() {
        let text = Text::from("a.b(a, a)");
        let all = Selections::single(Selection::caret(0)).with_all_occurrences(&text);
        assert_eq!(ranges(&all), [0..1, 4..5, 7..8]);
    }

    #[test]
    fn toggling_carets() {
        let selections = Selections::single(Selection::caret(2)).toggled_caret(5);
        assert_eq!(selections.len(), 2);
        assert_eq!(selections.toggled_caret(5).len(), 1, "a second ⌥⇧-click removes it");
        assert_eq!(Selections::single(Selection::caret(2)).toggled_caret(2).len(), 1, "the last caret stays");
    }

    #[test]
    fn column_selection_cuts_at_line_ends() {
        let text = Text::from("abcdef\nab\nabcdef");
        let column = column_selection(&text, Point::new(0, 1), Point::new(2, 4));
        assert_eq!(ranges(&column), [1..4, 8..9, 11..14]);
    }

    #[test]
    fn cloning_carets_keeps_the_column() {
        let text = Text::from("abcdef\nab\nabcdef");
        let one = Selections::single(Selection::caret(4));
        let two = one.with_cloned_caret(&text, Direction::Down);
        let three = two.with_cloned_caret(&text, Direction::Down);
        let heads: Vec<usize> = three.iter().map(|selection| selection.head).collect();
        assert_eq!(heads, [4, 9, 14], "column 4, cut on the short line, back to 4");
        assert_eq!(three.with_cloned_caret(&text, Direction::Down), three, "no line below");
    }
}
