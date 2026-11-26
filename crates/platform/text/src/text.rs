use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use ropey::Rope;

use crate::TextError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    pub line: usize,
    pub column: usize,
}

impl Point {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Clone, Default)]
pub struct Text {
    rope: Rope,
}

impl Text {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line(&self, line: usize) -> Result<Cow<'_, str>, TextError> {
        self.check_line(line)?;
        let start = self.rope.line_to_char(line);
        let end = start + self.line_len_chars(line);
        Ok(self.rope.slice(start..end).into())
    }

    pub fn slice(&self, range: Range<usize>) -> Result<Cow<'_, str>, TextError> {
        self.check_range(&range)?;
        Ok(self.rope.slice(range).into())
    }

    pub fn offset_to_point(&self, offset: usize) -> Result<Point, TextError> {
        self.check_offset(offset)?;
        let line = self.rope.char_to_line(offset);
        Ok(Point::new(line, offset - self.rope.line_to_char(line)))
    }

    pub fn point_to_offset(&self, point: Point) -> Result<usize, TextError> {
        self.check_line(point.line)?;
        let line_len = self.line_len_chars(point.line);
        if point.column > line_len {
            return Err(TextError::ColumnOutOfBounds { line: point.line, column: point.column, line_len });
        }
        Ok(self.rope.line_to_char(point.line) + point.column)
    }

    pub fn offset_to_utf16(&self, offset: usize) -> Result<(usize, usize), TextError> {
        let point = self.offset_to_point(offset)?;
        let line_start = self.rope.line_to_char(point.line);
        let column = self.rope.char_to_utf16_cu(offset) - self.rope.char_to_utf16_cu(line_start);
        Ok((point.line, column))
    }

    pub fn utf16_to_offset(&self, line: usize, column: usize) -> Result<usize, TextError> {
        self.check_line(line)?;
        let line_start = self.rope.line_to_char(line);
        let line_end = line_start + self.line_len_chars(line);
        let start_cu = self.rope.char_to_utf16_cu(line_start);
        let line_len = self.rope.char_to_utf16_cu(line_end) - start_cu;
        if column > line_len {
            return Err(TextError::ColumnOutOfBounds { line, column, line_len });
        }
        let offset = self.rope.utf16_cu_to_char(start_cu + column);
        if self.rope.char_to_utf16_cu(offset) != start_cu + column {
            return Err(TextError::ColumnInsideCharacter { line, column });
        }
        Ok(offset)
    }

    pub fn offset_to_utf8(&self, offset: usize) -> Result<(usize, usize), TextError> {
        let point = self.offset_to_point(offset)?;
        let line_start = self.rope.line_to_char(point.line);
        Ok((point.line, self.rope.char_to_byte(offset) - self.rope.char_to_byte(line_start)))
    }

    pub fn utf8_to_offset(&self, line: usize, column: usize) -> Result<usize, TextError> {
        self.check_line(line)?;
        let line_start = self.rope.line_to_char(line);
        let line_end = line_start + self.line_len_chars(line);
        let start_byte = self.rope.char_to_byte(line_start);
        let line_len = self.rope.char_to_byte(line_end) - start_byte;
        if column > line_len {
            return Err(TextError::ColumnOutOfBounds { line, column, line_len });
        }
        let offset = self.rope.byte_to_char(start_byte + column);
        if self.rope.char_to_byte(offset) != start_byte + column {
            return Err(TextError::ColumnInsideCharacter { line, column });
        }
        Ok(offset)
    }

    pub fn offset_to_byte(&self, offset: usize) -> Result<usize, TextError> {
        self.check_offset(offset)?;
        Ok(self.rope.char_to_byte(offset))
    }

    pub(crate) fn replaced(&self, range: Range<usize>, text: &str) -> Result<Text, TextError> {
        self.check_range(&range)?;
        let mut rope = self.rope.clone();
        rope.remove(range.clone());
        rope.insert(range.start, text);
        Ok(Text { rope })
    }

    fn line_len_chars(&self, line: usize) -> usize {
        let slice = self.rope.line(line);
        let mut len = slice.len_chars();
        if len > 0 && slice.char(len - 1) == '\n' {
            len -= 1;
        }
        if len > 0 && slice.char(len - 1) == '\r' {
            len -= 1;
        }
        len
    }

    fn check_offset(&self, offset: usize) -> Result<(), TextError> {
        let len = self.len_chars();
        if offset > len { Err(TextError::OffsetOutOfBounds { offset, len }) } else { Ok(()) }
    }

    pub(crate) fn check_range(&self, range: &Range<usize>) -> Result<(), TextError> {
        if range.start > range.end {
            return Err(TextError::InvertedRange { start: range.start, end: range.end });
        }
        self.check_offset(range.end)
    }

    fn check_line(&self, line: usize) -> Result<(), TextError> {
        let lines = self.len_lines();
        if line >= lines { Err(TextError::LineOutOfBounds { line, lines }) } else { Ok(()) }
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Self { rope: Rope::from_str(text) }
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.rope.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.rope == other.rope
    }
}

impl Eq for Text {}

impl PartialEq<str> for Text {
    fn eq(&self, other: &str) -> bool {
        self.rope == other
    }
}

impl PartialEq<&str> for Text {
    fn eq(&self, other: &&str) -> bool {
        self.rope == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_follow_the_language_server_protocol() {
        let text = Text::from("a\nb\r\nc\rd\u{2028}e");
        assert_eq!(text.len_lines(), 4);
        assert_eq!(text.line(0).unwrap(), "a");
        assert_eq!(text.line(1).unwrap(), "b");
        assert_eq!(text.line(2).unwrap(), "c");
        assert_eq!(text.line(3).unwrap(), "d\u{2028}e", "U+2028 is not a line break");
    }

    #[test]
    fn a_trailing_line_break_starts_an_empty_last_line() {
        let text = Text::from("a\n");
        assert_eq!(text.len_lines(), 2);
        assert_eq!(text.line(1).unwrap(), "");
        assert_eq!(text.offset_to_point(2).unwrap(), Point::new(1, 0));
    }

    #[test]
    fn points_and_offsets_round_trip() {
        let text = Text::from("fn main() {\r\n    println!(\"π\");\n}");
        for offset in 0..=text.len_chars() {
            let point = text.offset_to_point(offset).unwrap();
            if text.slice(offset.saturating_sub(1)..offset).unwrap() == "\r" && point.column > 0 {
                continue;
            }
            assert_eq!(text.point_to_offset(point).unwrap(), offset, "offset {offset}, point {point:?}");
        }
    }

    #[test]
    fn utf16_and_utf8_columns() {
        let text = Text::from("x\né😀z");
        let z = text.point_to_offset(Point::new(1, 2)).unwrap();
        assert_eq!(text.offset_to_utf16(z).unwrap(), (1, 3));
        assert_eq!(text.utf16_to_offset(1, 3).unwrap(), z);
        assert_eq!(text.offset_to_utf8(z).unwrap(), (1, 6));
        assert_eq!(text.utf8_to_offset(1, 6).unwrap(), z);
        assert_eq!(text.utf16_to_offset(1, 2), Err(TextError::ColumnInsideCharacter { line: 1, column: 2 }));
        assert_eq!(text.utf8_to_offset(1, 1), Err(TextError::ColumnInsideCharacter { line: 1, column: 1 }));
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges, reason = "the test checks that an inverted range is rejected")]
    fn out_of_bounds_is_an_error_not_a_panic() {
        let text = Text::from("ab\ncd");
        assert_eq!(text.offset_to_point(6), Err(TextError::OffsetOutOfBounds { offset: 6, len: 5 }));
        assert_eq!(text.point_to_offset(Point::new(2, 0)), Err(TextError::LineOutOfBounds { line: 2, lines: 2 }));
        assert_eq!(
            text.point_to_offset(Point::new(0, 3)),
            Err(TextError::ColumnOutOfBounds { line: 0, column: 3, line_len: 2 })
        );
        assert_eq!(text.slice(3..2), Err(TextError::InvertedRange { start: 3, end: 2 }));
        assert!(text.utf16_to_offset(0, 3).is_err());
        assert!(text.utf8_to_offset(5, 0).is_err());
    }

    #[test]
    fn snapshots_are_independent() {
        let original = Text::from("hello");
        let changed = original.replaced(0..5, "bye").unwrap();
        assert_eq!(original, "hello");
        assert_eq!(changed, "bye");
    }
}
