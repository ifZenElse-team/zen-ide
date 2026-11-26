#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    pub fn xterm_rgb(index: u8) -> Option<(u8, u8, u8)> {
        match index {
            0..=15 => None,
            16..=231 => {
                let level = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
                let cube = index - 16;
                Some((level(cube / 36), level(cube / 6 % 6), level(cube % 6)))
            }
            _ => {
                let grey = 8 + (index - 232) * 10;
                Some((grey, grey, grey))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Attributes {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub hidden: bool,
    pub strikethrough: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub combining: Vec<char>,
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attributes,
    pub wide: bool,
    pub spacer: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            combining: Vec::new(),
            fg: Color::Default,
            bg: Color::Default,
            attrs: Attributes::default(),
            wide: false,
            spacer: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionKind {
    #[default]
    Simple,
    Word,
    Line,
    Block,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridPoint {
    pub row: usize,
    pub col: usize,
    pub right: bool,
}

pub fn tidy_copied(text: &str) -> String {
    text.split('\n').map(|line| line.trim_end_matches([' ', '\t'])).collect::<Vec<_>>().join("\n")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    pub lines: Vec<Vec<Cell>>,
    pub cols: usize,
    pub cursor: Cursor,
    pub history: usize,
    pub scroll_offset: usize,
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub alt_screen: bool,
    pub alternate_scroll: bool,
    pub wrapped: Vec<bool>,
    pub selection: Vec<(usize, std::ops::Range<usize>)>,
}

impl Screen {
    pub fn rows(&self) -> usize {
        self.lines.len()
    }

    pub fn row_text(&self, row: usize) -> String {
        let Some(line) = self.lines.get(row) else { return String::new() };
        let mut text = String::new();
        for cell in line.iter().filter(|cell| !cell.spacer) {
            text.push(cell.ch);
            text.extend(&cell.combining);
        }
        text.truncate(text.trim_end().len());
        text
    }

    pub fn text(&self) -> String {
        let rows: Vec<String> = (0..self.rows()).map(|row| self.row_text(row)).collect();
        rows.join("\n").trim_end().to_owned()
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.lines.get(row).and_then(|line| line.get(col))
    }

    pub fn selected(&self, row: usize, col: usize) -> bool {
        self.selection.iter().any(|(selected, cols)| *selected == row && cols.contains(&col))
    }

    pub fn find(&self, needle: &str) -> Option<(usize, usize)> {
        (0..self.rows()).find_map(|row| self.row_text(row).find(needle).map(|col| (row, col)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_palette_cube_and_ramp() {
        assert_eq!(Color::xterm_rgb(7), None);
        assert_eq!(Color::xterm_rgb(16), Some((0, 0, 0)));
        assert_eq!(Color::xterm_rgb(196), Some((255, 0, 0)));
        assert_eq!(Color::xterm_rgb(231), Some((255, 255, 255)));
        assert_eq!(Color::xterm_rgb(232), Some((8, 8, 8)));
        assert_eq!(Color::xterm_rgb(255), Some((238, 238, 238)));
    }

    #[test]
    fn copied_text_loses_trailing_blanks_and_keeps_its_line_breaks() {
        assert_eq!(tidy_copied("a  \n\tb \t\n\nc"), "a\n\tb\n\nc");
        assert_eq!(tidy_copied("one long wrapped line  "), "one long wrapped line");
    }

    #[test]
    fn text_trims_blanks() {
        let line = |text: &str| {
            let mut cells: Vec<Cell> = text.chars().map(|ch| Cell { ch, ..Cell::default() }).collect();
            cells.resize(10, Cell::default());
            cells
        };
        let screen = Screen { lines: vec![line("ab"), line(" c"), line("")], cols: 10, ..Screen::default() };
        assert_eq!(screen.row_text(1), " c");
        assert_eq!(screen.text(), "ab\n c");
        assert_eq!(screen.find("c"), Some((1, 1)));
        assert_eq!(screen.row_text(9), "");
    }
}
