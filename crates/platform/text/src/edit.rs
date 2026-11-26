use std::ops::Range;

use crate::{Text, TextError};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
}

impl Edit {
    pub fn replace(range: Range<usize>, text: impl Into<String>) -> Self {
        Self { range, text: text.into() }
    }

    pub fn insert(offset: usize, text: impl Into<String>) -> Self {
        Self::replace(offset..offset, text)
    }

    pub fn delete(range: Range<usize>) -> Self {
        Self::replace(range, String::new())
    }

    pub fn is_noop(&self) -> bool {
        self.range.is_empty() && self.text.is_empty()
    }

    pub fn inserted_range(&self) -> Range<usize> {
        self.range.start..self.range.start + self.text.chars().count()
    }
}

impl Text {
    pub fn apply(&self, edits: &[Edit]) -> Result<(Text, Vec<Edit>), TextError> {
        let mut text = self.clone();
        let mut inverse = Vec::with_capacity(edits.len());
        for edit in edits {
            let removed = text.slice(edit.range.clone())?.into_owned();
            text = text.replaced(edit.range.clone(), &edit.text)?;
            inverse.push(Edit::replace(edit.inserted_range(), removed));
        }
        inverse.reverse();
        Ok((text, inverse))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_and_inverse() {
        let original = Text::from("let x = 1;");
        let edits = [Edit::replace(4..5, "value"), Edit::insert(13, "0"), Edit::delete(0..4)];
        let (changed, inverse) = original.apply(&edits).unwrap();
        assert_eq!(changed, "value = 10;");
        let (restored, _) = changed.apply(&inverse).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn a_failing_edit_changes_nothing() {
        let original = Text::from("abc");
        let result = original.apply(&[Edit::insert(1, "x"), Edit::delete(2..10)]);
        assert_eq!(result, Err(TextError::OffsetOutOfBounds { offset: 10, len: 4 }));
        assert_eq!(original, "abc");
    }

    #[test]
    fn inserted_range_counts_characters() {
        assert_eq!(Edit::insert(3, "π😀").inserted_range(), 3..5);
    }
}
