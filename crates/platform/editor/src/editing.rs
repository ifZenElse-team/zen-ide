use std::ops::Range;

use serde::{Deserialize, Serialize};
use zen_text::{Edit, Text};

use crate::movement::{word_end, word_start};
use crate::{Selection, Selections};

/// An editing operation, done at every caret at once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EditOp {
    /// Types or pastes `text`, replacing what is selected.
    Insert { text: String },
    /// Deletes the selection, or the character before the caret (⌫).
    DeleteBackward,
    /// Deletes the selection, or the character after the caret (⌦).
    DeleteForward,
    /// Deletes the selection, or to the start of the word before the caret (⌥⌫).
    DeleteWordBackward,
    /// Deletes the selection, or to the end of the word after the caret (⌥⌦).
    DeleteWordForward,
    /// Deletes the selection, or to the start of the word or camel hump before the caret (⌥⌫ with
    /// "CamelHumps" words on).
    DeleteHumpBackward,
    /// Deletes the selection, or to the end of the word or camel hump after the caret (⌥⌦ with
    /// "CamelHumps" words on).
    DeleteHumpForward,
    /// Pastes `text`: with as many lines as there are carets, each caret gets its own line;
    /// otherwise every caret gets the whole text.
    Paste { text: String },
}

fn pasted_lines(text: &str) -> Vec<&str> {
    let body = text.strip_suffix("\r\n").or_else(|| text.strip_suffix(['\n', '\r'])).unwrap_or(text);
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = body.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\r' if bytes.get(at + 1) == Some(&b'\n') => {
                lines.push(&body[start..at]);
                at += 2;
                start = at;
            }
            b'\n' | b'\r' => {
                lines.push(&body[start..at]);
                at += 1;
                start = at;
            }
            _ => at += 1,
        }
    }
    lines.push(&body[start..]);
    lines
}

impl EditOp {
    pub fn by_camel_humps(self) -> EditOp {
        match self {
            EditOp::DeleteWordBackward => EditOp::DeleteHumpBackward,
            EditOp::DeleteWordForward => EditOp::DeleteHumpForward,
            other => other,
        }
    }
}

pub fn edit(text: &Text, selections: &Selections, op: &EditOp) -> Option<(Vec<Edit>, Selections)> {
    let inserts: Vec<String> = match op {
        EditOp::Insert { text } => vec![text.clone(); selections.len()],
        EditOp::Paste { text } => {
            let lines = pasted_lines(text);
            if selections.len() > 1 && lines.len() == selections.len() {
                lines.into_iter().map(str::to_owned).collect()
            } else {
                vec![text.clone(); selections.len()]
            }
        }
        _ => vec![String::new(); selections.len()],
    };
    let mut targets: Vec<(Range<usize>, bool, &str)> = selections
        .iter()
        .enumerate()
        .map(|(index, selection)| {
            (target(text, selection, op), index == selections.primary_index(), inserts[index].as_str())
        })
        .collect();
    targets.sort_by_key(|(range, _, _)| (range.start, range.end));
    let mut ranges: Vec<(Range<usize>, bool, &str)> = Vec::with_capacity(targets.len());
    for (range, primary, insert) in targets {
        match ranges.last_mut() {
            Some((last, last_primary, _)) if range.start < last.end || range.start == last.start => {
                last.end = last.end.max(range.end);
                *last_primary |= primary;
            }
            _ => ranges.push((range, primary, insert)),
        }
    }
    if ranges.iter().all(|(range, _, insert)| range.is_empty() && insert.is_empty()) {
        return None;
    }
    let edits: Vec<Edit> =
        ranges.iter().rev().map(|(range, _, insert)| Edit::replace(range.clone(), *insert)).collect();
    let mut shift: isize = 0;
    let mut carets = Vec::with_capacity(ranges.len());
    let mut primary = 0;
    for (index, (range, is_primary, insert)) in ranges.iter().enumerate() {
        let inserted = insert.chars().count();
        let start = range.start.saturating_add_signed(shift);
        carets.push(Selection::caret(start + inserted));
        if *is_primary {
            primary = index;
        }
        shift += inserted as isize - range.len() as isize;
    }
    Some((edits, Selections::from_vec(carets, primary)))
}

fn target(text: &Text, selection: &Selection, op: &EditOp) -> Range<usize> {
    if !selection.is_empty() || matches!(op, EditOp::Insert { .. } | EditOp::Paste { .. }) {
        return selection.range();
    }
    let head = selection.head.min(text.len_chars());
    let mut here = text.offset_to_point(head).unwrap_or_default();
    let chars: Vec<char> = text.line(here.line).map(|line| line.chars().collect()).unwrap_or_default();
    let line_start = head - here.column;
    here.column = here.column.min(chars.len());
    let head = line_start + here.column;
    let previous_line_end = || {
        here.line
            .checked_sub(1)
            .and_then(|line| {
                let len = text.line(line).ok()?.chars().count();
                text.point_to_offset(zen_text::Point::new(line, len)).ok()
            })
            .unwrap_or(head)
    };
    let next_line_start = || text.point_to_offset(zen_text::Point::new(here.line + 1, 0)).unwrap_or(head);
    match op {
        EditOp::DeleteBackward if here.column == 0 => previous_line_end()..head,
        EditOp::DeleteBackward => head - 1..head,
        EditOp::DeleteForward if here.column == chars.len() => head..next_line_start(),
        EditOp::DeleteForward => head..head + 1,
        EditOp::DeleteWordBackward | EditOp::DeleteHumpBackward if here.column == 0 => previous_line_end()..head,
        EditOp::DeleteWordBackward | EditOp::DeleteHumpBackward => {
            line_start + word_start(&chars, here.column, *op == EditOp::DeleteHumpBackward)..head
        }
        EditOp::DeleteWordForward | EditOp::DeleteHumpForward if here.column == chars.len() => head..next_line_start(),
        EditOp::DeleteWordForward | EditOp::DeleteHumpForward => {
            head..line_start + word_end(&chars, here.column, *op == EditOp::DeleteHumpForward)
        }
        EditOp::Insert { .. } | EditOp::Paste { .. } => selection.range(),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use zen_text::Buffer;

    use super::*;

    fn run(text: &str, selections: Vec<Selection>, op: EditOp) -> (String, Vec<Range<usize>>) {
        let text = Text::from(text);
        let selections = Selections::from_vec(selections, 0);
        let Some((edits, after)) = edit(&text, &selections, &op) else {
            return (text.to_string(), selections.iter().map(Selection::range).collect());
        };
        let (result, _) = text.apply(&edits).unwrap();
        (result.to_string(), after.iter().map(Selection::range).collect())
    }

    fn insert(text: &str) -> EditOp {
        EditOp::Insert { text: text.to_owned() }
    }

    #[test]
    fn typing_at_every_caret() {
        let (text, carets) =
            run("ab\nab\nab", vec![Selection::caret(1), Selection::caret(4), Selection::caret(7)], insert("X"));
        assert_eq!(text, "aXb\naXb\naXb");
        assert_eq!(carets, [2..2, 6..6, 10..10]);
    }

    #[test]
    fn typing_replaces_selections() {
        let (text, carets) = run("hello world", vec![Selection::new(0, 5), Selection::new(11, 6)], insert("hi"));
        assert_eq!(text, "hi hi");
        assert_eq!(carets, [2..2, 5..5]);
    }

    #[test]
    fn deleting_characters_and_line_breaks() {
        assert_eq!(run("ab\r\ncd", vec![Selection::caret(4)], EditOp::DeleteBackward).0, "abcd");
        assert_eq!(run("ab\r\ncd", vec![Selection::caret(2)], EditOp::DeleteForward).0, "abcd");
        let (text, carets) = run("abc", vec![Selection::caret(0)], EditOp::DeleteBackward);
        assert_eq!((text.as_str(), carets[0].clone()), ("abc", 0..0));
        let (text, carets) = run("abc", vec![Selection::caret(1)], EditOp::DeleteForward);
        assert_eq!((text.as_str(), carets[0].clone()), ("ac", 1..1));
    }

    #[test]
    fn deleting_words() {
        assert_eq!(run("let foo_bar = 1", vec![Selection::caret(11)], EditOp::DeleteWordBackward).0, "let  = 1");
        assert_eq!(run("let foo_bar = 1", vec![Selection::caret(3)], EditOp::DeleteWordForward).0, "let = 1");
    }

    #[test]
    fn deleting_camel_humps() {
        let back = EditOp::DeleteWordBackward.by_camel_humps();
        let forward = EditOp::DeleteWordForward.by_camel_humps();
        assert_eq!(back, EditOp::DeleteHumpBackward);
        assert_eq!(run("x fooBarBaz", vec![Selection::caret(11)], back.clone()).0, "x fooBar");
        assert_eq!(run("x fooBarBaz", vec![Selection::caret(8)], back.clone()).0, "x fooBaz");
        assert_eq!(run("x fooBarBaz", vec![Selection::caret(5)], forward.clone()).0, "x fooBaz");
        assert_eq!(run("x fooBarBaz", vec![Selection::caret(1)], forward).0, "xBarBaz");
        assert_eq!(run("let foo_bar", vec![Selection::caret(11)], back.clone()).0, "let foo_");
        assert_eq!(run("a\nb", vec![Selection::caret(2)], back).0, "ab");
        assert_eq!(EditOp::DeleteBackward.by_camel_humps(), EditOp::DeleteBackward);
    }

    #[test]
    fn carets_deleting_into_each_other_merge() {
        let (text, carets) = run("abcd", vec![Selection::caret(2), Selection::caret(3)], EditOp::DeleteBackward);
        assert_eq!(text, "ad");
        assert_eq!(carets.len(), 1);
        assert_eq!(carets[0], 1..1);
    }

    #[test]
    fn paste_gives_each_caret_its_line() {
        let carets = vec![Selection::caret(0), Selection::caret(2), Selection::caret(4)];
        let paste = |text: &str| EditOp::Paste { text: text.to_owned() };
        assert_eq!(run("a\nb\nc", carets.clone(), paste("1\n2\n3\n")).0, "1a\n2b\n3c");
        assert_eq!(run("a\nb\nc", carets.clone(), paste("xy")).0, "xya\nxyb\nxyc", "not as many lines: whole text");
        assert_eq!(run("a", vec![Selection::caret(0)], paste("1\n2")).0, "1\n2a", "one caret: as is");
        let carets = vec![Selection::caret(0), Selection::caret(2), Selection::caret(4)];
        assert_eq!(run("a\rb\rc", carets.clone(), paste("1\r2\r3\r")).0, "1a\r2b\r3c", "CR lines");
        let carets = vec![Selection::caret(0), Selection::caret(3), Selection::caret(6)];
        assert_eq!(run("a\r\nb\r\nc", carets, paste("1\r\n2\r\n3")).0, "1a\r\n2b\r\n3c", "CRLF lines");
    }

    #[test]
    fn edits_go_through_the_undo_history() {
        let mut buffer = Buffer::new("ab\nab");
        let selections = Selections::from_vec(vec![Selection::caret(0), Selection::caret(3)], 0);
        let (edits, _) = edit(buffer.text(), &selections, &insert("//")).unwrap();
        buffer.edit(edits, zen_text::EditKind::Other).unwrap();
        assert_eq!(buffer.text().to_string(), "//ab\n//ab");
        buffer.undo();
        assert_eq!(buffer.text().to_string(), "ab\nab");
    }

    proptest! {
        #[test]
        fn insertion_at_many_carets(text in "[a-c\n]{0,40}", offsets in prop::collection::vec(0usize..41, 1..6), word in "[x-z]{1,3}") {
            let base = Text::from(text.as_str());
            let len = base.len_chars();
            let mut unique: Vec<usize> = offsets.into_iter().map(|offset| offset.min(len)).collect();
            unique.sort_unstable();
            unique.dedup();
            let selections = Selections::from_vec(unique.iter().map(|&offset| Selection::caret(offset)).collect(), 0);
            let (edits, after) = edit(&base, &selections, &insert(&word)).unwrap();
            let (result, _) = base.apply(&edits).unwrap();
            let mut expected: Vec<char> = text.chars().collect();
            for &offset in unique.iter().rev() {
                for (i, c) in word.chars().enumerate() {
                    expected.insert(offset + i, c);
                }
            }
            prop_assert_eq!(result.to_string(), expected.into_iter().collect::<String>());
            let size = word.chars().count();
            let heads: Vec<usize> = after.iter().map(|selection| selection.head).collect();
            let wanted: Vec<usize> = unique.iter().enumerate().map(|(i, &offset)| offset + size * (i + 1)).collect();
            prop_assert_eq!(heads, wanted);
        }

        #[test]
        fn deletion_at_many_carets(text in "[a-c \r\n_.]{0,40}", offsets in prop::collection::vec(0usize..41, 1..6), op in 0..4u8) {
            let base = Text::from(text.as_str());
            let len = base.len_chars();
            let selections = Selections::from_vec(offsets.into_iter().map(|offset| Selection::caret(offset.min(len))).collect(), 0);
            let op = [EditOp::DeleteBackward, EditOp::DeleteForward, EditOp::DeleteWordBackward, EditOp::DeleteWordForward][op as usize].clone();
            if let Some((edits, after)) = edit(&base, &selections, &op) {
                let (result, _) = base.apply(&edits).unwrap();
                prop_assert!(result.len_chars() < len);
                prop_assert!(after.iter().all(|selection| selection.head <= result.len_chars()));
            }
        }
    }
}
