use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextError {
    OffsetOutOfBounds { offset: usize, len: usize },
    InvertedRange { start: usize, end: usize },
    LineOutOfBounds { line: usize, lines: usize },
    ColumnOutOfBounds { line: usize, column: usize, line_len: usize },
    ColumnInsideCharacter { line: usize, column: usize },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TextError::OffsetOutOfBounds { offset, len } => {
                write!(f, "offset {offset} is past the end of a text of {len} characters")
            }
            TextError::InvertedRange { start, end } => write!(f, "range {start}..{end} starts after it ends"),
            TextError::LineOutOfBounds { line, lines } => {
                write!(f, "line {line} is past the last line of a text with {lines} lines")
            }
            TextError::ColumnOutOfBounds { line, column, line_len } => {
                write!(f, "column {column} is past the end of line {line}, which has length {line_len}")
            }
            TextError::ColumnInsideCharacter { line, column } => {
                write!(f, "column {column} on line {line} points inside a character")
            }
        }
    }
}

impl std::error::Error for TextError {}
