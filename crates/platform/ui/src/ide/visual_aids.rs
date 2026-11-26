use egui::{Color32, Painter, Stroke, pos2};
use zen_workbench::visual_aids::{TabRendering, WhitespaceKinds};

const BLANK_REACH: usize = 200;

const BLOCK_REACH: usize = 5000;

const SKIP_REACH: usize = 256;

pub(super) const GUIDE_WIDTH: f32 = 1.0;

pub(super) fn indent_of(line: &str, tab: usize) -> Option<usize> {
    let mut column = 0;
    for c in line.chars() {
        match c {
            ' ' => column += 1,
            '\t' => column += tab - column % tab,
            '\r' | '\n' => return None,
            _ => return Some(column),
        }
    }
    None
}

fn closes_block(line: &str) -> bool {
    matches!(line.trim_start().chars().next(), Some('}' | ')' | ']'))
}

pub(super) fn guide_limit<'a>(line: usize, lines: usize, text_of: impl Fn(usize) -> &'a str, tab: usize) -> usize {
    if let Some(indent) = indent_of(text_of(line), tab) {
        return indent;
    }
    let above = (line.saturating_sub(BLANK_REACH)..line).rev().find_map(|other| indent_of(text_of(other), tab));
    let below = (line + 1..lines.min(line + 1 + BLANK_REACH))
        .find_map(|other| indent_of(text_of(other), tab).map(|indent| (indent, closes_block(text_of(other)))));
    match (above, below) {
        (Some(above), Some((below, closes))) => above.min(if closes { below + 1 } else { below }),
        _ => 0,
    }
}

fn enclosing<'a>(line: usize, text_of: impl Fn(usize) -> &'a str, tab: usize) -> Vec<usize> {
    let mut found = Vec::new();
    let mut below = usize::MAX;
    for other in (line.saturating_sub(BLOCK_REACH)..line).rev() {
        if let Some(indent) = indent_of(text_of(other), tab).filter(|&indent| indent < below) {
            found.push(indent);
            below = indent;
            if indent == 0 {
                break;
            }
        }
    }
    found.reverse();
    found
}

fn push(open: &mut Vec<usize>, indent: Option<usize>) {
    if let Some(indent) = indent {
        while open.last().is_some_and(|&last| last >= indent) {
            open.pop();
        }
        open.push(indent);
    }
}

pub(super) fn guides_of<'a>(
    drawn: impl IntoIterator<Item = usize>,
    lines: usize,
    text_of: impl Fn(usize) -> &'a str + Copy,
    tab: usize,
) -> std::collections::HashMap<usize, Vec<usize>> {
    let mut guides = std::collections::HashMap::new();
    let mut open: Vec<usize> = Vec::new();
    let mut next: Option<usize> = None;
    for line in drawn {
        match next {
            Some(from) if line >= from && line - from <= SKIP_REACH => {
                for other in from..line {
                    push(&mut open, indent_of(text_of(other), tab));
                }
            }
            Some(from) if line < from => continue,
            _ => open = enclosing(line, text_of, tab),
        }
        let limit = guide_limit(line, lines, text_of, tab);
        guides.insert(line, open.iter().copied().take_while(|&column| column < limit).collect());
        push(&mut open, indent_of(text_of(line), tab));
        next = Some(line + 1);
    }
    guides
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectedGuide {
    pub(super) column: usize,
    pub(super) first: usize,
    pub(super) last: usize,
}

impl SelectedGuide {
    pub(super) fn is(&self, line: usize, column: usize) -> bool {
        column == self.column && (self.first..=self.last).contains(&line)
    }
}

#[derive(Default)]
pub(super) struct CaretBlock {
    found_for: Option<(u64, usize, usize)>,
    guide: Option<SelectedGuide>,
}

impl CaretBlock {
    pub(super) fn find<'a>(
        &mut self,
        (version, caret_line, lines): (u64, usize, usize),
        tab: usize,
        text_of: impl Fn(usize) -> &'a str + Copy,
    ) -> Option<SelectedGuide> {
        let key = (version, caret_line, tab);
        if self.found_for != Some(key) {
            self.found_for = Some(key);
            self.guide = selected_guide(caret_line, lines, tab, text_of);
        }
        self.guide
    }
}

fn selected_guide<'a>(
    caret_line: usize,
    lines: usize,
    tab: usize,
    text_of: impl Fn(usize) -> &'a str + Copy,
) -> Option<SelectedGuide> {
    let limit = |line| guide_limit(line, lines, text_of, tab);
    let caret_limit = limit(caret_line);
    let column = enclosing(caret_line, text_of, tab).into_iter().take_while(|&column| column < caret_limit).last()?;
    let first = (caret_line.saturating_sub(BLOCK_REACH)..caret_line)
        .rev()
        .take_while(|&line| limit(line) > column)
        .last()
        .unwrap_or(caret_line);
    let last = (caret_line + 1..lines.min(caret_line + 1 + BLOCK_REACH))
        .take_while(|&line| limit(line) > column)
        .last()
        .unwrap_or(caret_line);
    Some(SelectedGuide { column, first, last })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LineShape {
    pub(super) text_start: usize,
    pub(super) text_end: usize,
}

impl LineShape {
    pub(super) fn of(line: &str) -> Self {
        let mut text_start = None;
        let mut text_end = 0;
        let mut count = 0;
        for (index, c) in line.chars().enumerate() {
            count = index + 1;
            if !c.is_whitespace() {
                text_start.get_or_insert(index);
                text_end = index + 1;
            }
        }
        Self { text_start: text_start.unwrap_or(count), text_end }
    }

    pub(super) fn shows(&self, index: usize, kinds: WhitespaceKinds, selected: bool) -> bool {
        let leading = index < self.text_start;
        let trailing = index >= self.text_end;
        (kinds.selection && selected)
            || (kinds.leading && leading)
            || (kinds.trailing && trailing)
            || (kinds.inner && !leading && !trailing)
    }
}

pub(super) fn space(painter: &Painter, (left, right): (f32, f32), y: f32, char_width: f32, color: Color32) {
    painter.circle_filled(pos2((left + right) / 2.0, y), (char_width * 0.12).clamp(0.8, 1.6), color);
}

pub(super) fn tab(
    painter: &Painter,
    (left, right): (f32, f32),
    y: f32,
    char_width: f32,
    rendering: TabRendering,
    color: Color32,
) {
    let inset = char_width * 0.25;
    let (from, to) = (left + inset, (right - inset).max(left + inset + 1.0));
    let stroke = Stroke::new(1.0, color);
    painter.hline(from..=to, y, stroke);
    if rendering == TabRendering::Arrow {
        let head = (char_width * 0.35).min(to - from);
        painter.line_segment([pos2(to - head, y - head), pos2(to, y)], stroke);
        painter.line_segment([pos2(to - head, y + head), pos2(to, y)], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(text: &str, tab: usize) -> Vec<usize> {
        let lines: Vec<&str> = text.split('\n').collect();
        (0..lines.len()).map(|line| guide_limit(line, lines.len(), |other| lines[other], tab)).collect()
    }

    #[test]
    fn indents_count_tabs_to_their_stops() {
        assert_eq!(indent_of("\tx", 4), Some(4));
        assert_eq!(indent_of("  \tx", 4), Some(4));
        assert_eq!(indent_of("      x", 4), Some(6));
        assert_eq!(indent_of("x", 4), Some(0));
        assert_eq!(indent_of(" \t ", 4), None);
        assert_eq!(indent_of("", 4), None);
    }

    fn guides(text: &str, tab: usize) -> Vec<Vec<usize>> {
        let lines: Vec<&str> = text.split('\n').collect();
        let found = guides_of(0..lines.len(), lines.len(), |other| lines[other], tab);
        (0..lines.len()).map(|line| found[&line].clone()).collect()
    }

    #[test]
    fn a_block_has_a_guide_over_its_body_and_its_blank_lines() {
        let go = "func main() {\n\tif x {\n\t\ta()\n\n\t}\n\n\tb()\n}\n\nfunc other() {}";
        assert_eq!(limits(go, 4), vec![0, 4, 8, 5, 4, 4, 4, 0, 0, 0]);
        let none = Vec::<usize>::new();
        assert_eq!(
            guides(go, 4),
            vec![
                none.clone(),
                vec![0],
                vec![0, 4],
                vec![0, 4],
                vec![0],
                vec![0],
                vec![0],
                none.clone(),
                none.clone(),
                none
            ],
            "the blank line before the if's end keeps the if's guide, nothing between two functions"
        );
    }

    #[test]
    fn guides_stand_at_the_indents_the_text_uses() {
        let js = "f() {\n  if (a) {\n    b();\n  }\n}";
        assert_eq!(guides(js, 4), vec![vec![], vec![0], vec![0, 2], vec![0], vec![]], "two spaces, whatever the step");
        let python = "def f(a,\n      b):\n    if a:\n        return b";
        assert_eq!(guides(python, 4), vec![vec![], vec![0], vec![0], vec![0, 4]], "no guide inside a continued line");
        let mixed = "f {\n\tif {\n\t    b\n        c\n  \t\td\n\t}\n}";
        assert_eq!(guides(mixed, 4)[2..5], [vec![0, 4], vec![0, 4], vec![0, 4]], "tabs and spaces to the same column");
        assert_eq!(guides("a:\n        b:\n                c", 8), vec![vec![], vec![0], vec![0, 8]], "wide tabs");
    }

    #[test]
    fn lines_after_a_fold_take_the_lines_above_them() {
        let mut text = String::from("f {\n\tg {\n");
        text.push_str(&"\t\tx\n".repeat(1000));
        text.push_str("\t\ty\n\t}\n}");
        let lines: Vec<&str> = text.split('\n').collect();
        let drawn = [0, 1, 1002, 1003, 1004];
        let found = guides_of(drawn, lines.len(), |other| lines[other], 4);
        assert_eq!(found[&1002], vec![0, 4]);
        assert_eq!(found[&1003], vec![0]);
        assert!(found[&1004].is_empty());
    }

    #[test]
    fn a_blank_line_takes_the_shallower_side_unless_a_block_closes_below() {
        assert_eq!(limits("a:\n    b\n\nc", 4), vec![0, 4, 0, 0]);
        assert_eq!(limits("    a\n\n    b", 4), vec![4, 4, 4]);
        assert_eq!(limits("\n\nx", 4), vec![0, 0, 0], "nothing above");
    }

    #[test]
    fn the_caret_selects_the_innermost_guide_of_its_line() {
        let go = "func main() {\n\tif x {\n\t\ta()\n\n\t}\n\tb()\n}";
        let lines: Vec<&str> = go.split('\n').collect();
        let find = |line| selected_guide(line, lines.len(), 4, |other| lines[other]);
        assert_eq!(find(2), Some(SelectedGuide { column: 4, first: 2, last: 3 }));
        assert_eq!(find(5), Some(SelectedGuide { column: 0, first: 1, last: 5 }));
        assert_eq!(find(1), Some(SelectedGuide { column: 0, first: 1, last: 5 }), "the if's line is in main's block");
        assert_eq!(find(0), None, "a top-level line has no guide");
        let js = "f() {\n  if (a) {\n    b();\n  }\n}";
        let lines: Vec<&str> = js.split('\n').collect();
        assert_eq!(selected_guide(2, lines.len(), 4, |other| lines[other]).map(|guide| guide.column), Some(2));
        let lines: Vec<&str> = go.split('\n').collect();
        let mut block = CaretBlock::default();
        let first = block.find((1, 2, lines.len()), 4, |other| lines[other]);
        assert_eq!(first, find(2));
        assert_eq!(block.find((1, 2, lines.len()), 4, |_| "x"), first, "kept while nothing changed");
    }

    #[test]
    fn whitespace_kinds_pick_their_places() {
        let shape = LineShape::of("  a b  ");
        assert_eq!(shape, LineShape { text_start: 2, text_end: 5 });
        let only = |leading, inner, trailing, selection| WhitespaceKinds { leading, inner, trailing, selection };
        assert!(shape.shows(0, only(true, false, false, false), false));
        assert!(!shape.shows(3, only(true, false, false, false), false));
        assert!(shape.shows(3, only(false, true, false, false), false));
        assert!(shape.shows(6, only(false, false, true, false), false));
        assert!(!shape.shows(6, only(false, false, false, true), false));
        assert!(shape.shows(6, only(false, false, false, true), true), "a selection shows its own");
        let blank = LineShape::of("   ");
        assert_eq!(blank, LineShape { text_start: 3, text_end: 0 });
        assert!(blank.shows(1, only(false, false, true, false), false), "a blank line is trailing too");
    }
}
