use std::ops::Range;

use serde::{Deserialize, Serialize};
use zen_text::Edit;

/// A selection: the text between the anchor, where it started, and the head, where the caret is.
/// A caret is a selection whose anchor and head are equal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Selection {
    /// Where the selection started, as a character offset.
    pub anchor: usize,
    /// Where the caret is, as a character offset.
    pub head: usize,
    /// The column vertical movement aims for, kept across lines shorter than it.
    #[serde(skip)]
    #[schemars(skip)]
    pub goal_column: Option<usize>,
}

impl Selection {
    pub const fn caret(offset: usize) -> Self {
        Self { anchor: offset, head: offset, goal_column: None }
    }

    pub const fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head, goal_column: None }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn mapped(self, edits: &[Edit]) -> Self {
        let map = |mut offset: usize| {
            for edit in edits {
                let inserted = edit.text.chars().count();
                if offset >= edit.range.end {
                    offset = offset - edit.range.len() + inserted;
                } else if offset > edit.range.start {
                    offset = edit.range.start + inserted;
                }
            }
            offset
        };
        Self { anchor: map(self.anchor), head: map(self.head), goal_column: None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selections {
    items: Vec<Selection>,
    primary: usize,
}

impl Default for Selections {
    fn default() -> Self {
        Self::single(Selection::caret(0))
    }
}

impl Selections {
    pub fn single(selection: Selection) -> Self {
        Self { items: vec![selection], primary: 0 }
    }

    pub fn from_vec(items: Vec<Selection>, primary: usize) -> Self {
        if items.is_empty() {
            return Self::default();
        }
        let primary = primary.min(items.len() - 1);
        let mut selections = Self { items, primary };
        selections.normalize();
        selections
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Selection> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn primary(&self) -> Selection {
        self.items[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    pub fn add(&mut self, selection: Selection) {
        self.items.push(selection);
        self.primary = self.items.len() - 1;
        self.normalize();
    }

    pub fn collapse_to_primary(&mut self) {
        self.items = vec![self.primary()];
        self.primary = 0;
    }

    pub fn mapped(&self, edits: &[Edit]) -> Self {
        Self::from_vec(self.items.iter().map(|selection| selection.mapped(edits)).collect(), self.primary)
    }

    pub fn clamped(&self, len: usize) -> Self {
        let clamp = |selection: &Selection| Selection {
            anchor: selection.anchor.min(len),
            head: selection.head.min(len),
            goal_column: selection.goal_column,
        };
        Self::from_vec(self.items.iter().map(clamp).collect(), self.primary)
    }

    fn normalize(&mut self) {
        let primary = self.items[self.primary];
        let mut items = std::mem::take(&mut self.items);
        items.sort_by_key(|selection| (selection.range().start, selection.range().end));
        let mut merged: Vec<Selection> = Vec::with_capacity(items.len());
        let mut primary_index = 0;
        for selection in items {
            let is_primary = selection == primary;
            match merged.last_mut() {
                Some(last) if overlaps(last, &selection) => {
                    let start = last.range().start.min(selection.range().start);
                    let end = last.range().end.max(selection.range().end);
                    let forward = if selection.range().end >= last.range().end {
                        selection.head >= selection.anchor
                    } else {
                        last.head >= last.anchor
                    };
                    *last = if forward { Selection::new(start, end) } else { Selection::new(end, start) };
                    if is_primary {
                        primary_index = merged.len() - 1;
                    }
                }
                _ => {
                    if is_primary {
                        primary_index = merged.len();
                    }
                    merged.push(selection);
                }
            }
        }
        self.items = merged;
        self.primary = primary_index;
    }
}

fn overlaps(first: &Selection, second: &Selection) -> bool {
    let (a, b) = (first.range(), second.range());
    if a.is_empty() || b.is_empty() { b.start <= a.end && b.start >= a.start } else { b.start < a.end }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_overlapping_selections_and_keeps_the_primary() {
        let selections =
            Selections::from_vec(vec![Selection::new(5, 9), Selection::caret(1), Selection::new(7, 12)], 2);
        let items: Vec<_> = selections.iter().map(|selection| selection.range()).collect();
        assert_eq!(items, [1..1, 5..12]);
        assert_eq!(selections.primary().range(), 5..12);
        assert_eq!(selections.primary_index(), 1);
    }

    #[test]
    fn carets_at_the_same_place_become_one() {
        let selections = Selections::from_vec(vec![Selection::caret(3), Selection::caret(3)], 0);
        assert_eq!(selections.len(), 1);
    }

    #[test]
    fn adjacent_selections_stay_apart() {
        let selections = Selections::from_vec(vec![Selection::new(0, 3), Selection::new(3, 6)], 0);
        assert_eq!(selections.len(), 2);
    }

    #[test]
    fn offsets_follow_edits() {
        let selection = Selection::new(2, 8);
        let edits = [Edit::insert(0, "ab"), Edit::delete(5..7)];
        assert_eq!(selection.mapped(&edits).range(), 4..8);
        assert_eq!(Selection::caret(6).mapped(&[Edit::replace(4..8, "xyz")]), Selection::caret(7));
    }
}
