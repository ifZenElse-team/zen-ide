use std::ops::Range;

const MAX_INDENT_SHARE: f32 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct LineWrap {
    breaks: Vec<usize>,
    len: usize,
    indent: f32,
}

impl LineWrap {
    pub const fn single(len: usize) -> Self {
        Self { breaks: Vec::new(), len, indent: 0.0 }
    }

    pub fn new(chars: &[char], widths: &[f32], width: f32) -> Self {
        let len = chars.len().min(widths.len());
        let leading = chars.iter().take(len).take_while(|c| c.is_whitespace()).count();
        let mut indent: f32 = widths[..leading].iter().sum();
        if indent > width * MAX_INDENT_SHARE {
            indent = 0.0;
        }
        let mut breaks = Vec::new();
        let mut row_start = 0;
        let mut x = 0.0;
        let mut last_break = None;
        for at in 0..len {
            let c = chars[at];
            if at > row_start && at > leading && may_break(chars[at - 1], c) {
                last_break = Some(at);
            }
            if !c.is_whitespace() {
                while at > row_start && x + widths[at] > width {
                    let next = last_break.filter(|&point| point > row_start).unwrap_or(at);
                    breaks.push(next);
                    row_start = next;
                    last_break = None;
                    x = indent + widths[next..at].iter().sum::<f32>();
                }
            }
            x += widths[at];
        }
        Self { breaks, len, indent }
    }

    pub fn rows(&self) -> usize {
        self.breaks.len() + 1
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn row_range(&self, row: usize) -> Range<usize> {
        let row = row.min(self.breaks.len());
        let start = if row == 0 { 0 } else { self.breaks[row - 1] };
        start..self.breaks.get(row).copied().unwrap_or(self.len)
    }

    pub fn indent_of(&self, row: usize) -> f32 {
        if row == 0 { 0.0 } else { self.indent }
    }

    pub fn is_wrap(&self, column: usize) -> bool {
        self.breaks.contains(&column)
    }

    pub fn row_of(&self, column: usize, before_wrap: bool) -> usize {
        let row = self.breaks.partition_point(|&start| start <= column);
        if before_wrap && row > 0 && self.breaks[row - 1] == column { row - 1 } else { row }
    }

    pub fn x_of(&self, widths: &[f32], row: usize, column: usize) -> f32 {
        let range = self.row_range(row);
        let column = column.clamp(range.start, range.end);
        self.indent_of(row) + widths[range.start..column].iter().sum::<f32>()
    }

    pub fn column_at(&self, widths: &[f32], row: usize, x: f32) -> (usize, bool) {
        let range = self.row_range(row);
        let mut left = self.indent_of(row);
        for column in range.clone() {
            let width = widths[column];
            if x < left + width / 2.0 {
                return (column, false);
            }
            left += width;
        }
        (range.end, row + 1 < self.rows())
    }
}

pub fn estimate_rows(chars: usize, char_width: f32, width: f32) -> usize {
    if width <= 0.0 || char_width <= 0.0 {
        return 1;
    }
    ((chars as f32 * char_width / width).ceil() as usize).max(1)
}

fn may_break(previous: char, next: char) -> bool {
    !next.is_whitespace() && (previous.is_whitespace() || is_wide(previous) || is_wide(next))
}

pub fn is_wide(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x2FFFD
            | 0x30000..=0x3FFFD
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(text: &str, width: f32) -> (Vec<String>, LineWrap) {
        let chars: Vec<char> = text.chars().collect();
        let widths: Vec<f32> = chars.iter().map(|&c| if is_wide(c) { 2.0 } else { 1.0 }).collect();
        let wrap = LineWrap::new(&chars, &widths, width);
        let rows = (0..wrap.rows()).map(|row| chars[wrap.row_range(row)].iter().collect()).collect();
        (rows, wrap)
    }

    #[test]
    fn a_line_that_fits_is_one_row() {
        let (rows, wrap) = layout("short line", 20.0);
        assert_eq!(rows, ["short line"]);
        assert_eq!(wrap, LineWrap::single(10));
        assert_eq!(layout("", 5.0).1.rows(), 1);
    }

    #[test]
    fn rows_break_after_spaces_which_stay_at_the_row_end() {
        let (rows, _) = layout("the quick brown fox jumps", 10.0);
        assert_eq!(rows, ["the quick ", "brown fox ", "jumps"]);
        let (rows, _) = layout("one  two", 4.0);
        assert_eq!(rows, ["one  ", "two"], "spaces hang past the edge instead of starting a row");
    }

    #[test]
    fn a_word_longer_than_the_row_breaks_where_it_overflows() {
        let (rows, _) = layout("abcdefghijkl xy", 5.0);
        assert_eq!(rows, ["abcde", "fghij", "kl xy"]);
    }

    #[test]
    fn wide_characters_take_their_width_and_break_between_any_two() {
        let (rows, _) = layout("日本語のテキスト", 6.0);
        assert_eq!(rows, ["日本語", "のテキ", "スト"]);
        let (rows, _) = layout("ab日本", 3.0);
        assert_eq!(rows, ["ab", "日", "本"]);
    }

    #[test]
    fn continuation_rows_keep_the_lines_indent() {
        let (rows, wrap) = layout("    alpha beta gamma", 14.0);
        assert_eq!(rows, ["    alpha beta ", "gamma"]);
        assert_eq!((wrap.indent_of(0), wrap.indent_of(1)), (0.0, 4.0));
        let (rows, _) = layout("  ab cd ef", 6.0);
        assert_eq!(rows, ["  ab ", "cd ", "ef"], "the indent counts against the continuation's room");
        let (_, deep) = layout("          a b c d e f g", 12.0);
        assert_eq!(deep.indent_of(1), 0.0, "an indent of more than half the width is dropped");
    }

    #[test]
    fn the_indent_is_never_a_row_of_its_own() {
        let (rows, _) = layout("    abcdefgh", 6.0);
        assert_eq!(rows, ["    ab", "cdefgh"]);
    }

    #[test]
    fn tabs_are_as_wide_as_the_caller_says() {
        let chars: Vec<char> = "\tab cd".chars().collect();
        let widths = [4.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let wrap = LineWrap::new(&chars, &widths, 8.0);
        assert_eq!((wrap.row_range(0), wrap.row_range(1)), (0..4, 4..6));
        assert_eq!(wrap.indent_of(1), 4.0, "a tab indent counts as its width");
        assert_eq!(wrap.x_of(&widths, 0, 2), 5.0);
        assert_eq!(wrap.x_of(&widths, 1, 5), 5.0);
    }

    #[test]
    fn a_position_at_a_wrap_is_on_either_row() {
        let (_, wrap) = layout("aaaa bbbb cccc", 6.0);
        assert_eq!((wrap.row_range(0), wrap.row_range(1), wrap.row_range(2)), (0..5, 5..10, 10..14));
        assert!(wrap.is_wrap(5) && !wrap.is_wrap(0) && !wrap.is_wrap(14));
        assert_eq!((wrap.row_of(5, false), wrap.row_of(5, true)), (1, 0));
        assert_eq!((wrap.row_of(3, true), wrap.row_of(14, false)), (0, 2));
    }

    #[test]
    fn a_click_maps_to_the_nearest_column_of_its_row() {
        let (_, wrap) = layout("aaaa bbbb cccc", 6.0);
        let widths = [1.0; 14];
        assert_eq!(wrap.column_at(&widths, 1, 0.4), (5, false));
        assert_eq!(wrap.column_at(&widths, 1, 2.6), (8, false));
        assert_eq!(wrap.column_at(&widths, 0, 40.0), (5, true), "past a wrapped row's end: before the wrap");
        assert_eq!(wrap.column_at(&widths, 2, 40.0), (14, false), "past the last row: the line's end");
    }

    #[test]
    fn the_estimate_needs_only_the_length() {
        assert_eq!(estimate_rows(0, 7.0, 700.0), 1);
        assert_eq!(estimate_rows(100, 7.0, 700.0), 1);
        assert_eq!(estimate_rows(101, 7.0, 700.0), 2);
        assert_eq!(estimate_rows(1000, 7.0, 0.0), 1);
    }
}
