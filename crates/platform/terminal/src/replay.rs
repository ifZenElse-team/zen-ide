use std::fmt::Write;

use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};

const STYLE: Flags = Flags::BOLD
    .union(Flags::DIM)
    .union(Flags::ITALIC)
    .union(Flags::ALL_UNDERLINES)
    .union(Flags::INVERSE)
    .union(Flags::HIDDEN)
    .union(Flags::STRIKEOUT);

pub(crate) fn snapshot<T: EventListener>(term: &mut Term<T>, title: Option<&str>) -> Vec<u8> {
    let mut out = String::new();
    let alt = term.mode().contains(TermMode::ALT_SCREEN);
    if alt {
        let mut alt_screen = String::new();
        screen(term.grid(), &mut alt_screen);
        term.swap_alt();
        lines(term.grid(), &mut out);
        term.swap_alt();
        let mut parser: Processor = Processor::new();
        parser.advance(term, alt_screen.as_bytes());
        out.push_str("\x1b[?1049h");
        out.push_str(&alt_screen);
    } else {
        lines(term.grid(), &mut out);
    }
    let cursor = term.grid().cursor.point;
    let _ = write!(out, "\x1b[0m\x1b[{};{}H", cursor.line.0 + 1, cursor.column.0 + 1);
    modes(*term.mode(), &mut out);
    if let Some(title) = title {
        let _ = write!(out, "\x1b]2;{title}\x07");
    }
    out.into_bytes()
}

fn lines(grid: &Grid<Cell>, out: &mut String) {
    let history = grid.history_size() as i32;
    let rows = grid.screen_lines() as i32;
    let mut pen = Pen::default();
    for line in -history..rows {
        let wrapped = row(grid, Line(line), &mut pen, out);
        if !wrapped && line + 1 < rows {
            out.push_str("\r\n");
        }
    }
}

fn screen(grid: &Grid<Cell>, out: &mut String) {
    for line in 0..grid.screen_lines() as i32 {
        let _ = write!(out, "\x1b[0m\x1b[{};1H\x1b[2K", line + 1);
        let mut pen = Pen::default();
        row(grid, Line(line), &mut pen, out);
    }
}

fn row(grid: &Grid<Cell>, line: Line, pen: &mut Pen, out: &mut String) -> bool {
    let cols = grid.columns();
    let cells = &grid[line];
    let wrapped = cells[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
    let end =
        if wrapped { cols } else { (0..cols).rev().find(|col| !blank(&cells[Column(*col)])).map_or(0, |col| col + 1) };
    for col in 0..end {
        let cell = &cells[Column(col)];
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let wanted = Pen { fg: cell.fg, bg: cell.bg, flags: cell.flags & STYLE };
        if wanted != *pen {
            wanted.write(out);
            *pen = wanted;
        }
        out.push(cell.c);
        if let Some(zerowidth) = cell.zerowidth() {
            out.extend(zerowidth);
        }
    }
    if *pen != Pen::default() {
        out.push_str("\x1b[0m");
        *pen = Pen::default();
    }
    wrapped
}

fn blank(cell: &Cell) -> bool {
    cell.c == ' '
        && cell.bg == Color::Named(NamedColor::Background)
        && !cell.flags.intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
        && cell.zerowidth().is_none()
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Pen {
    fg: Color,
    bg: Color,
    flags: Flags,
}

impl Default for Pen {
    fn default() -> Self {
        Pen {
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            flags: Flags::empty(),
        }
    }
}

impl Pen {
    fn write(&self, out: &mut String) {
        out.push_str("\x1b[0");
        for (flag, code) in [
            (Flags::BOLD, "1"),
            (Flags::DIM, "2"),
            (Flags::ITALIC, "3"),
            (Flags::UNDERLINE, "4"),
            (Flags::DOUBLE_UNDERLINE, "4:2"),
            (Flags::UNDERCURL, "4:3"),
            (Flags::DOTTED_UNDERLINE, "4:4"),
            (Flags::DASHED_UNDERLINE, "4:5"),
            (Flags::INVERSE, "7"),
            (Flags::HIDDEN, "8"),
            (Flags::STRIKEOUT, "9"),
        ] {
            if self.flags.contains(flag) {
                out.push(';');
                out.push_str(code);
            }
        }
        color(self.fg, 30, out);
        color(self.bg, 40, out);
        out.push('m');
    }
}

fn color(color: Color, base: u8, out: &mut String) {
    let _ = match color {
        Color::Spec(rgb) => write!(out, ";{};2;{};{};{}", base + 8, rgb.r, rgb.g, rgb.b),
        Color::Indexed(index) => write!(out, ";{};5;{index}", base + 8),
        Color::Named(named) => match named as usize {
            index @ 0..=7 => write!(out, ";{}", usize::from(base) + index),
            index @ 8..=15 => write!(out, ";{}", usize::from(base) + 60 + index - 8),
            _ => Ok(()),
        },
    };
}

fn modes(mode: TermMode, out: &mut String) {
    for (flag, on, off) in [
        (TermMode::APP_CURSOR, "\x1b[?1h", ""),
        (TermMode::APP_KEYPAD, "\x1b=", ""),
        (TermMode::BRACKETED_PASTE, "\x1b[?2004h", ""),
        (TermMode::ALTERNATE_SCROLL, "", "\x1b[?1007l"),
        (TermMode::MOUSE_REPORT_CLICK, "\x1b[?1000h", ""),
        (TermMode::MOUSE_DRAG, "\x1b[?1002h", ""),
        (TermMode::MOUSE_MOTION, "\x1b[?1003h", ""),
        (TermMode::SGR_MOUSE, "\x1b[?1006h", ""),
        (TermMode::FOCUS_IN_OUT, "\x1b[?1004h", ""),
        (TermMode::LINE_WRAP, "", "\x1b[?7l"),
        (TermMode::INSERT, "\x1b[4h", ""),
        (TermMode::LINE_FEED_NEW_LINE, "\x1b[20h", ""),
        (TermMode::SHOW_CURSOR, "", "\x1b[?25l"),
    ] {
        out.push_str(if mode.contains(flag) { on } else { off });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;

    use crate::Size;
    use crate::session::Dims;

    fn term(bytes: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &Dims(Size::new(5, 10)), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes);
        term
    }

    fn cells(grid: &Grid<Cell>) -> Vec<(char, Color, Color, Flags)> {
        let history = grid.history_size() as i32;
        (-history..grid.screen_lines() as i32)
            .flat_map(|line| {
                (0..grid.columns()).map(move |col| {
                    let cell = &grid[Line(line)][Column(col)];
                    (cell.c, cell.fg, cell.bg, cell.flags & (STYLE | Flags::WRAPLINE | Flags::WIDE_CHAR))
                })
            })
            .collect()
    }

    fn same(original: &mut Term<VoidListener>) -> Term<VoidListener> {
        let copy = term(&snapshot(original, None));
        assert_eq!(cells(copy.grid()), cells(original.grid()), "the screen shown");
        assert_eq!(copy.grid().cursor.point, original.grid().cursor.point, "the cursor");
        assert_eq!(*copy.mode() & !TermMode::URGENCY_HINTS, *original.mode() & !TermMode::URGENCY_HINTS, "the modes");
        copy
    }

    #[test]
    fn a_shell_screen_comes_back_with_history_colours_and_wrapped_lines() {
        let mut original = term(
            b"one\r\n\x1b[1;31mred\x1b[0m \x1b[48;5;22mbg\x1b[0m\r\n\x1b[38;2;1;2;3mrgb\x1b[0m\r\n0123456789wrapped\r\nfour\r\nfive\r\n\xe4\xb8\xad\xe6\x96\x87 $ \x1b[?2004h\x1b[?1h",
        );
        assert!(original.grid().history_size() > 0, "some lines are in the history");
        same(&mut original);
    }

    #[test]
    fn a_full_screen_program_comes_back_over_the_shell_it_hides() {
        let mut original = term(
            b"shell line\r\n$ vim\r\n\x1b[?1049h\x1b[2J\x1b[H\x1b[7m~ editor ~\x1b[0m\x1b[3;4Hx\x1b[?25l\x1b[?1000h",
        );
        let mut copy = same(&mut original);
        same(&mut original);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut original, b"\x1b[?1049l");
        parser.advance(&mut copy, b"\x1b[?1049l");
        assert_eq!(cells(copy.grid()), cells(original.grid()), "the shell under it, after the program ends");
    }
}
