use crate::{Edit, Text, TextError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Other,
}

#[derive(Clone, Debug)]
struct Step {
    before: Text,
    after: Text,
    edits: Vec<Edit>,
    inverse: Vec<Edit>,
    kind: EditKind,
}

#[derive(Clone, Debug, Default)]
pub struct Buffer {
    text: Text,
    undo: Vec<Step>,
    redo: Vec<Step>,
    group_open: bool,
}

impl Buffer {
    pub fn new(text: impl Into<Text>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }

    pub fn text(&self) -> &Text {
        &self.text
    }

    pub fn edit(&mut self, edits: Vec<Edit>, kind: EditKind) -> Result<&Text, TextError> {
        if edits.iter().all(Edit::is_noop) {
            return Ok(&self.text);
        }
        let (after, inverse) = self.text.apply(&edits)?;
        self.redo.clear();

        let merge = kind == EditKind::Typing
            && self.group_open
            && self.undo.last().is_some_and(|last| continues_typing(last, &edits));
        if merge && let Some(last) = self.undo.last_mut() {
            last.after = after.clone();
            last.edits.extend(edits);
            let mut combined = inverse;
            combined.append(&mut last.inverse);
            last.inverse = combined;
        } else {
            self.undo.push(Step { before: self.text.clone(), after: after.clone(), edits, inverse, kind });
        }
        self.group_open = kind == EditKind::Typing;
        self.text = after;
        Ok(&self.text)
    }

    pub fn close_group(&mut self) {
        self.group_open = false;
    }

    pub fn merge_last(&mut self, count: usize) -> bool {
        if count < 2 || count > self.undo.len() {
            return false;
        }
        let steps = self.undo.split_off(self.undo.len() - count);
        let edits = steps.iter().flat_map(|step| step.edits.iter().cloned()).collect();
        let inverse = steps.iter().rev().flat_map(|step| step.inverse.iter().cloned()).collect();
        let (before, after) = (steps[0].before.clone(), steps[count - 1].after.clone());
        self.undo.push(Step { before, after, edits, inverse, kind: EditKind::Other });
        self.group_open = false;
        true
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> Option<Vec<Edit>> {
        let step = self.undo.pop()?;
        self.text = step.before.clone();
        let applied = step.inverse.clone();
        self.redo.push(step);
        self.group_open = false;
        Some(applied)
    }

    pub fn redo(&mut self) -> Option<Vec<Edit>> {
        let step = self.redo.pop()?;
        self.text = step.after.clone();
        let applied = step.edits.clone();
        self.undo.push(step);
        self.group_open = false;
        Some(applied)
    }
}

fn continues_typing(last: &Step, edits: &[Edit]) -> bool {
    let Some(first) = last.edits.len().checked_sub(edits.len()).filter(|_| !edits.is_empty()) else { return false };
    let previous = &last.edits[first..];
    if last.kind != EditKind::Typing || !bottom_up(previous) || !bottom_up(edits) {
        return false;
    }
    let mut shift = 0_isize;
    previous.iter().rev().zip(edits.iter().rev()).all(|(previous, edit)| {
        let end = previous.inserted_range().end.cast_signed() + shift;
        shift = end - previous.range.end.cast_signed();
        edit.range.is_empty()
            && !edit.text.contains(['\n', '\r'])
            && !previous.text.ends_with(['\n', '\r'])
            && edit.range.start.cast_signed() == end
    })
}

fn bottom_up(edits: &[Edit]) -> bool {
    edits.windows(2).all(|pair| pair[1].range.end <= pair[0].range.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(buffer: &mut Buffer, at: usize, text: &str) {
        for (index, ch) in text.chars().enumerate() {
            buffer.edit(vec![Edit::insert(at + index, ch.to_string())], EditKind::Typing).unwrap();
        }
    }

    #[test]
    fn consecutive_typing_is_one_step() {
        let mut buffer = Buffer::new("");
        type_str(&mut buffer, 0, "hello");
        assert_eq!(buffer.text(), "hello");
        let applied = buffer.undo().unwrap();
        assert_eq!(buffer.text(), "");
        assert_eq!(
            applied,
            vec![Edit::delete(4..5), Edit::delete(3..4), Edit::delete(2..3), Edit::delete(1..2), Edit::delete(0..1)]
        );
        assert!(!buffer.can_undo());
    }

    #[test]
    fn closing_the_group_starts_a_new_step() {
        let mut buffer = Buffer::new("");
        type_str(&mut buffer, 0, "ab");
        buffer.close_group();
        type_str(&mut buffer, 2, "cd");
        buffer.undo();
        assert_eq!(buffer.text(), "ab");
        buffer.undo();
        assert_eq!(buffer.text(), "");
    }

    fn type_at_carets(buffer: &mut Buffer, carets: &[usize], ch: &str) {
        let edits = carets.iter().rev().map(|caret| Edit::insert(*caret, ch)).collect();
        buffer.edit(edits, EditKind::Typing).unwrap();
    }

    #[test]
    fn typing_at_several_carets_is_one_step() {
        let mut buffer = Buffer::new("(), ()");
        type_at_carets(&mut buffer, &[1, 5], "a");
        assert_eq!(buffer.text(), "(a), (a)");
        type_at_carets(&mut buffer, &[2, 7], "b");
        type_at_carets(&mut buffer, &[3, 9], "c");
        assert_eq!(buffer.text(), "(abc), (abc)");
        assert_eq!(buffer.undo_depth(), 1);
        buffer.undo();
        assert_eq!(buffer.text(), "(), ()");
        assert!(!buffer.can_undo());
        buffer.redo();
        assert_eq!(buffer.text(), "(abc), (abc)");
    }

    #[test]
    fn typing_at_carets_that_moved_starts_a_new_step() {
        let mut buffer = Buffer::new("(), ()");
        type_at_carets(&mut buffer, &[1, 5], "a");
        type_at_carets(&mut buffer, &[1, 7], "b");
        assert_eq!(buffer.undo_depth(), 2);
        type_at_carets(&mut buffer, &[0, 2, 9], "c");
        assert_eq!(buffer.undo_depth(), 3);
    }

    #[test]
    fn typing_elsewhere_or_a_line_break_starts_a_new_step() {
        let mut buffer = Buffer::new("");
        type_str(&mut buffer, 0, "ab");
        type_str(&mut buffer, 0, "x");
        assert_eq!(buffer.text(), "xab");
        buffer.undo();
        assert_eq!(buffer.text(), "ab");

        type_str(&mut buffer, 2, "\n");
        type_str(&mut buffer, 3, "c");
        buffer.undo();
        assert_eq!(buffer.text(), "ab\n");
        buffer.undo();
        assert_eq!(buffer.text(), "ab");
    }

    #[test]
    fn other_edits_are_always_separate_steps() {
        let mut buffer = Buffer::new("abc");
        buffer.edit(vec![Edit::insert(3, "d")], EditKind::Other).unwrap();
        buffer.edit(vec![Edit::insert(4, "e")], EditKind::Other).unwrap();
        buffer.undo();
        assert_eq!(buffer.text(), "abcd");
    }

    #[test]
    fn redo_reapplies_and_a_new_edit_clears_it() {
        let mut buffer = Buffer::new("abc");
        buffer.edit(vec![Edit::delete(0..1)], EditKind::Other).unwrap();
        buffer.undo();
        assert_eq!(buffer.redo(), Some(vec![Edit::delete(0..1)]));
        assert_eq!(buffer.text(), "bc");
        buffer.undo();
        buffer.edit(vec![Edit::insert(0, "z")], EditKind::Other).unwrap();
        assert!(!buffer.can_redo());
        assert_eq!(buffer.redo(), None);
    }

    #[test]
    fn a_failed_edit_leaves_the_buffer_unchanged() {
        let mut buffer = Buffer::new("abc");
        assert!(buffer.edit(vec![Edit::delete(2..9)], EditKind::Other).is_err());
        assert_eq!(buffer.text(), "abc");
        assert!(!buffer.can_undo());
    }

    #[test]
    fn merged_steps_undo_and_redo_as_one() {
        let mut buffer = Buffer::new("abc");
        buffer.edit(vec![Edit::insert(0, "x")], EditKind::Other).unwrap();
        buffer.edit(vec![Edit::insert(4, "yz"), Edit::delete(1..2)], EditKind::Other).unwrap();
        buffer.edit(vec![Edit::replace(0..1, "q")], EditKind::Other).unwrap();
        assert!(!buffer.merge_last(4));
        assert!(!buffer.merge_last(1));
        assert!(buffer.merge_last(2));
        assert_eq!(buffer.text(), "qbcyz");
        buffer.undo();
        assert_eq!(buffer.text(), "xabc");
        buffer.undo();
        assert_eq!(buffer.text(), "abc");
        assert!(!buffer.can_undo());
        buffer.redo();
        assert_eq!(buffer.redo(), Some(vec![Edit::insert(4, "yz"), Edit::delete(1..2), Edit::replace(0..1, "q")]));
        assert_eq!(buffer.text(), "qbcyz");
    }

    #[test]
    fn noop_changes_are_not_recorded() {
        let mut buffer = Buffer::new("abc");
        buffer.edit(vec![Edit::insert(1, "")], EditKind::Other).unwrap();
        assert!(!buffer.can_undo());
    }
}
