use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use egui::FontId;
use zen_editor::soft_wrap::estimate_rows;
use zen_editor::{LineWrap, Movement, Selection, Selections};
use zen_workbench::Document;
use zen_workbench::soft_wrap::{ShownVisualRows, VisualRows};

const LAYOUT_CACHE: usize = 4096;
const RIGHT_MARGIN: f32 = 16.0;
const MIN_COLUMNS: f32 = 10.0;

#[derive(Clone, Debug, Default)]
pub(super) struct ShownRows {
    lines: Vec<usize>,
    tops: Option<Vec<usize>>,
}

impl Deref for ShownRows {
    type Target = [usize];

    fn deref(&self) -> &[usize] {
        &self.lines
    }
}

impl ShownRows {
    pub(super) fn new(lines: Vec<usize>) -> Self {
        Self { lines, tops: None }
    }

    #[cfg(test)]
    pub(super) fn wrapped(lines: Vec<usize>, tops: Vec<usize>) -> Self {
        Self { lines, tops: Some(tops) }
    }

    pub(super) fn top(&self, row: usize) -> usize {
        match &self.tops {
            None => row,
            Some(tops) => tops[row.min(tops.len() - 1)],
        }
    }

    pub(super) fn height(&self, row: usize) -> usize {
        self.top(row + 1) - self.top(row)
    }

    pub(super) fn total(&self) -> usize {
        self.top(self.lines.len())
    }

    pub(super) fn row_at(&self, visual: usize) -> usize {
        match &self.tops {
            None => visual,
            Some(tops) => tops.partition_point(|&top| top <= visual).saturating_sub(1),
        }
    }
}

#[derive(Debug)]
pub(super) struct Measured {
    pub(super) wrap: LineWrap,
    pub(super) widths: Vec<f32>,
}

pub(super) struct PointedRow {
    pub(super) line: usize,
    pub(super) columns: Vec<usize>,
    pub(super) segment: std::ops::Range<usize>,
    pub(super) galley: Arc<egui::Galley>,
    pub(super) left: f32,
    pub(super) top: f32,
    pub(super) wrapped: bool,
    pub(super) hint_columns: Vec<usize>,
}

impl PointedRow {
    pub(super) fn x_of(&self, column: usize) -> f32 {
        let column = column.clamp(self.segment.start, self.segment.end);
        let shown = self.columns[column] - self.columns[self.segment.start];
        self.left + self.galley.pos_from_cursor(egui::text::CCursor::new(shown)).min.x
    }

    pub(super) fn x_after(&self, column: usize) -> f32 {
        let column = column.clamp(self.segment.start, self.segment.end);
        let hints = self.hint_columns.iter().filter(|&&at| at == column).count();
        let shown = self.columns[column] + hints - self.columns[self.segment.start];
        self.left + self.galley.pos_from_cursor(egui::text::CCursor::new(shown)).min.x
    }

    pub(super) fn column_at(&self, x: f32) -> (usize, bool) {
        let from = self.columns[self.segment.start];
        let index = from + self.galley.cursor_from_pos(egui::vec2(x - self.left, 0.0)).index.0;
        let column = self
            .columns
            .iter()
            .enumerate()
            .take(self.segment.end + 1)
            .skip(self.segment.start)
            .min_by_key(|(_, display)| display.abs_diff(index))
            .map_or(self.segment.start, |(column, _)| column);
        (column, self.wrapped && column == self.segment.end)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Metrics<'a> {
    pub(super) ctx: &'a egui::Context,
    pub(super) font: &'a FontId,
    pub(super) tab: usize,
    pub(super) width: f32,
    pub(super) char_width: f32,
}

impl Metrics<'_> {
    pub(super) fn wrap_width(text_width: f32, char_width: f32) -> f32 {
        (text_width - RIGHT_MARGIN).max(char_width * MIN_COLUMNS)
    }
}

struct Frame {
    path: PathBuf,
    rows: ShownRows,
    folded: HashMap<usize, usize>,
    ctx: egui::Context,
    font: FontId,
    tab: usize,
    width: f32,
    char_width: f32,
}

impl Frame {
    fn metrics(&self) -> Metrics<'_> {
        Metrics { ctx: &self.ctx, font: &self.font, tab: self.tab, width: self.width, char_width: self.char_width }
    }
}

#[derive(Default)]
struct Carets {
    heads: Vec<usize>,
    before_wrap: HashSet<usize>,
    goals: Vec<(usize, f32)>,
}

#[derive(Default)]
struct Shown {
    frame: Option<Frame>,
    carets: Carets,
}

#[derive(Default)]
struct SharedRows(Mutex<Shown>);

impl SharedRows {
    fn lock(&self) -> MutexGuard<'_, Shown> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Anchor {
    path: PathBuf,
    line: usize,
    offset: f32,
    begins: f32,
}

#[derive(Default)]
pub(super) struct SoftWrap {
    key: Option<(PathBuf, u32, FontId, usize)>,
    version: u64,
    lengths: Vec<usize>,
    counts: Vec<usize>,
    char_width: f32,
    width: f32,
    layouts: HashMap<u64, Arc<Measured>>,
    laid_out: usize,
    resized: bool,
    anchor: Option<Anchor>,
    kept: bool,
    shared: Arc<SharedRows>,
}

impl SoftWrap {
    pub(super) fn follow(&mut self, path: &Path, version: u64, line_bytes: &[usize], metrics: Metrics<'_>) {
        let same_path = self.key.as_ref().is_some_and(|(shown, ..)| shown == path);
        let same = same_path
            && self.key.as_ref().is_some_and(|(_, width, font, tab)| {
                *width == metrics.width.to_bits() && font == metrics.font && *tab == metrics.tab
            });
        self.resized = same_path && !same;
        if same && self.version == version {
            return;
        }
        let lengths: Vec<usize> = line_bytes.windows(2).map(|pair| pair[1] - pair[0]).collect();
        self.counts = if same { shifted(&self.lengths, &lengths, &self.counts) } else { vec![0; lengths.len()] };
        self.key = Some((path.to_owned(), metrics.width.to_bits(), metrics.font.clone(), metrics.tab));
        self.version = version;
        self.lengths = lengths;
        self.char_width = metrics.char_width;
        self.width = metrics.width;
    }

    pub(super) fn anchored<'t>(
        &mut self,
        metrics: Metrics<'_>,
        path: &Path,
        rows: &mut ShownRows,
        view_rows: usize,
        (folded, text_of): (impl Fn(usize) -> bool, impl Fn(usize) -> &'t str),
    ) -> Option<f32> {
        let anchor = self.anchor.as_ref().filter(|anchor| anchor.path == path)?;
        let (line, offset, begins) = (anchor.line, anchor.offset, anchor.begins);
        let row = rows.partition_point(|&at| at <= line).checked_sub(1).filter(|&row| rows[row] == line)?;
        if self.resized {
            let visual = rows.top(row);
            self.fit(metrics, rows, visual..visual + view_rows, &folded, &text_of);
        }
        let wanted = rows.top(row) as f32 + offset.clamp(0.0, rows.height(row) as f32);
        ((wanted - begins).abs() > 0.01).then_some(wanted)
    }

    pub(super) fn settle<'t>(
        &mut self,
        metrics: Metrics<'_>,
        path: &Path,
        rows: &mut ShownRows,
        (from, to): (f32, f32),
        view_rows: usize,
        (folded, text_of): (impl Fn(usize) -> bool, impl Fn(usize) -> &'t str),
    ) {
        if rows.is_empty() {
            return;
        }
        let row_at = |rows: &ShownRows, at: f32| rows.row_at(at.max(0.0) as usize).min(rows.len() - 1);
        let seen = row_at(rows, from);
        let before = rows.top(seen) as f32;
        let mut top = to;
        if (to - from).abs() > 0.01 {
            let first = to.max(0.0) as usize;
            self.fit(metrics, rows, first..first + view_rows, &folded, &text_of);
            if to < from && from - to < view_rows as f32 {
                top = to + rows.top(seen) as f32 - before;
            }
        }
        let row = row_at(rows, top);
        let anchor = Anchor { path: path.to_owned(), line: rows[row], offset: top - rows.top(row) as f32, begins: to };
        self.anchor = Some(anchor);
    }

    pub(super) fn rows(&self, lines: Vec<usize>, folded: impl Fn(usize) -> bool) -> ShownRows {
        let mut rows = ShownRows { lines, tops: Some(Vec::new()) };
        self.retop(&mut rows, folded);
        rows
    }

    fn retop(&self, rows: &mut ShownRows, folded: impl Fn(usize) -> bool) {
        let mut tops = Vec::with_capacity(rows.lines.len() + 1);
        let mut top = 0;
        for &line in &rows.lines {
            tops.push(top);
            top += if folded(line) { 1 } else { self.count(line) };
        }
        tops.push(top);
        rows.tops = Some(tops);
    }

    fn count(&self, line: usize) -> usize {
        match self.counts.get(line) {
            Some(&count) if count > 0 => count,
            _ => estimate_rows(self.lengths.get(line).copied().unwrap_or(0), self.char_width, self.width),
        }
    }

    pub(super) fn fit<'t>(
        &mut self,
        metrics: Metrics<'_>,
        rows: &mut ShownRows,
        visuals: std::ops::Range<usize>,
        folded: impl Fn(usize) -> bool,
        text_of: impl Fn(usize) -> &'t str,
    ) {
        for _ in 0..4 {
            let first = rows.row_at(visuals.start).min(rows.len());
            let last = (rows.row_at(visuals.end) + 1).min(rows.len());
            let mut changed = false;
            for row in first..last {
                let line = rows[row];
                if folded(line) {
                    continue;
                }
                let count = self.measure(metrics, text_of(line)).wrap.rows();
                if let Some(known) = self.counts.get_mut(line) {
                    *known = count;
                }
                changed |= rows.height(row) != count;
            }
            if !changed {
                return;
            }
            self.retop(rows, &folded);
        }
    }

    pub(super) fn measure(&mut self, metrics: Metrics<'_>, line: &str) -> Arc<Measured> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (line, metrics.width.to_bits(), metrics.font, metrics.tab).hash(&mut hasher);
        let key = hasher.finish();
        if let Some(measured) = self.layouts.get(&key) {
            return Arc::clone(measured);
        }
        if self.layouts.len() > LAYOUT_CACHE {
            self.layouts.clear();
        }
        let measured = Arc::new(lay_out(metrics, line));
        self.laid_out += 1;
        self.layouts.insert(key, Arc::clone(&measured));
        measured
    }

    #[cfg(test)]
    pub(super) fn laid_out(&self) -> usize {
        self.laid_out
    }

    #[cfg(test)]
    pub(super) fn width(&self) -> f32 {
        self.width
    }

    #[cfg(test)]
    pub(super) fn kept(&self) -> Option<ShownRows> {
        self.shared.lock().frame.as_ref().map(|frame| frame.rows.clone())
    }

    #[cfg(test)]
    pub(super) fn before_wrap(&self, head: usize) -> bool {
        self.shared.lock().carets.before_wrap.contains(&head)
    }

    pub(super) fn drawn_before_wrap(&self, heads: &[usize]) -> HashSet<usize> {
        let shown = self.shared.lock();
        if shown.carets.heads == heads { shown.carets.before_wrap.clone() } else { HashSet::new() }
    }

    pub(super) fn clicked(&mut self, head: usize, before_wrap: bool) {
        let before_wrap = if before_wrap { HashSet::from([head]) } else { HashSet::new() };
        self.shared.lock().carets = Carets { heads: vec![head], before_wrap, goals: Vec::new() };
    }

    pub(super) fn forget_carets(&mut self) {
        self.shared.lock().carets = Carets::default();
    }

    pub(super) fn keep(
        &mut self,
        session: &mut zen_core::Session,
        path: &Path,
        rows: ShownRows,
        folded: HashMap<usize, usize>,
        metrics: Metrics<'_>,
    ) {
        self.shared.lock().frame = Some(Frame {
            path: path.to_owned(),
            rows,
            folded,
            ctx: metrics.ctx.clone(),
            font: metrics.font.clone(),
            tab: metrics.tab,
            width: metrics.width,
            char_width: metrics.char_width,
        });
        self.kept = true;
        let published = session
            .state::<ShownVisualRows>()
            .is_some_and(|shown| std::ptr::addr_eq(Arc::as_ptr(&shown.0), Arc::as_ptr(&self.shared)));
        if !published {
            session.set_state(ShownVisualRows(Arc::clone(&self.shared) as Arc<dyn VisualRows>));
        }
    }

    pub(super) fn drop_frame(&mut self) {
        if std::mem::take(&mut self.kept) {
            self.shared.lock().frame = None;
        }
    }
}

pub(super) fn moves_by_rows((id, params): &(String, serde_json::Value)) -> bool {
    id == "editor.caret.move"
        && matches!(
            params["movement"]["kind"].as_str(),
            Some("up" | "down" | "pageUp" | "pageDown" | "lineStart" | "lineEnd")
        )
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Up,
    Down,
    RowStart,
    RowEnd,
}

impl VisualRows for SharedRows {
    fn moved(&self, document: &Document, movement: Movement, extend: bool) -> Option<Selections> {
        let (step, times) = match movement {
            Movement::Up => (Step::Up, 1),
            Movement::Down => (Step::Down, 1),
            Movement::PageUp { lines } => (Step::Up, lines.max(1)),
            Movement::PageDown { lines } => (Step::Down, lines.max(1)),
            Movement::LineStart => (Step::RowStart, 1),
            Movement::LineEnd => (Step::RowEnd, 1),
            _ => return None,
        };
        let mut shown = self.lock();
        let Shown { frame, carets } = &mut *shown;
        let frame = frame.as_ref().filter(|frame| frame.path == document.path())?;
        let selections = document.selections();
        let heads: Vec<usize> = selections.iter().map(|selection| selection.head).collect();
        let known = carets.heads == heads;
        let mut mover = Mover { frame, text: document.snapshot(), lines: HashMap::new() };
        let mut moved = Vec::new();
        let mut before = HashSet::new();
        let mut goals = Vec::new();
        for selection in selections.iter() {
            let mut head = selection.head;
            let mut before_wrap = known && carets.before_wrap.contains(&head);
            let mut goal = known.then(|| carets.goals.iter().find(|(at, _)| *at == head).map(|(_, x)| *x)).flatten();
            for _ in 0..times {
                (head, before_wrap, goal) = mover.step(head, before_wrap, goal, step);
            }
            moved.push(Selection { anchor: if extend { selection.anchor } else { head }, head, goal_column: None });
            if before_wrap {
                before.insert(head);
            }
            if let Some(goal) = goal {
                goals.push((head, goal));
            }
        }
        let moved = Selections::from_vec(moved, selections.primary_index());
        *carets = Carets { heads: moved.iter().map(|selection| selection.head).collect(), before_wrap: before, goals };
        Some(moved)
    }
}

struct Mover<'a> {
    frame: &'a Frame,
    text: &'a zen_text::Text,
    lines: HashMap<usize, Arc<Measured>>,
}

impl Mover<'_> {
    fn line(&mut self, line: usize) -> Arc<Measured> {
        if let Some(measured) = self.lines.get(&line) {
            return Arc::clone(measured);
        }
        let content = self.text.line(line).map(|line| line.into_owned()).unwrap_or_default();
        let metrics = self.frame.metrics();
        let width = if self.frame.folded.contains_key(&line) { f32::INFINITY } else { metrics.width };
        let measured = Arc::new(lay_out(Metrics { width, ..metrics }, &content));
        self.lines.insert(line, Arc::clone(&measured));
        measured
    }

    fn step(&mut self, head: usize, before_wrap: bool, goal: Option<f32>, step: Step) -> (usize, bool, Option<f32>) {
        let text = self.text;
        let point = text.offset_to_point(head).unwrap_or_default();
        let here = self.line(point.line);
        let wrap = &here.wrap;
        let column = point.column.min(wrap.len());
        let row = wrap.row_of(column, before_wrap && wrap.is_wrap(column));
        let offset =
            |line: usize, column: usize| text.point_to_offset(zen_text::Point::new(line, column)).unwrap_or(head);
        match step {
            Step::RowEnd => {
                let end = wrap.row_range(row).end;
                if row + 1 < wrap.rows() && !(column == end && before_wrap) {
                    (offset(point.line, end), true, None)
                } else {
                    (offset(point.line, wrap.len()), false, None)
                }
            }
            Step::RowStart => {
                let start = wrap.row_range(row).start;
                if row > 0 && column != start {
                    (offset(point.line, start), false, None)
                } else {
                    let caret = Selections::from_vec(vec![Selection::caret(head)], 0);
                    (caret.moved(text, Movement::LineStart, false).primary().head, false, None)
                }
            }
            Step::Up | Step::Down => {
                let x = goal.unwrap_or_else(|| wrap.x_of(&here.widths, row, column));
                let down = step == Step::Down;
                let rows = &self.frame.rows;
                let shown = rows.partition_point(|&line| line <= point.line).saturating_sub(1);
                let target = if down && row + 1 < wrap.rows() {
                    Some((point.line, Arc::clone(&here), row + 1))
                } else if !down && row > 0 {
                    Some((point.line, Arc::clone(&here), row - 1))
                } else {
                    let next =
                        if down { rows.get(shown + 1) } else { shown.checked_sub(1).and_then(|row| rows.get(row)) };
                    next.copied().map(|line| {
                        let measured = self.line(line);
                        let row = if down { 0 } else { measured.wrap.rows() - 1 };
                        (line, measured, row)
                    })
                };
                let Some((line, measured, row)) = target else { return (head, before_wrap, Some(x)) };
                let (mut column, before_wrap) = measured.wrap.column_at(&measured.widths, row, x);
                if let Some(&fold) = self.frame.folded.get(&line) {
                    column = column.min(fold);
                }
                (offset(line, column), before_wrap, Some(x))
            }
        }
    }
}

fn lay_out(metrics: Metrics<'_>, line: &str) -> Measured {
    let chars: Vec<char> = line.chars().collect();
    let tab = metrics.tab.max(1);
    let widths: Vec<f32> = metrics.ctx.fonts_mut(|fonts| {
        let space = fonts.glyph_width(metrics.font, ' ');
        let mut column = 0;
        chars
            .iter()
            .map(|&c| {
                if c == '\t' {
                    let spaces = tab - column % tab;
                    column += spaces;
                    spaces as f32 * space
                } else {
                    column += 1;
                    fonts.glyph_width(metrics.font, c)
                }
            })
            .collect()
    });
    Measured { wrap: LineWrap::new(&chars, &widths, metrics.width), widths }
}

fn shifted(old_lengths: &[usize], new_lengths: &[usize], counts: &[usize]) -> Vec<usize> {
    let same = |a: &[usize], b: &[usize]| a.iter().zip(b).take_while(|(a, b)| a == b).count();
    let prefix = same(old_lengths, new_lengths);
    let room = old_lengths.len().min(new_lengths.len()) - prefix;
    let suffix = old_lengths.iter().rev().zip(new_lengths.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let mut shifted = Vec::with_capacity(new_lengths.len());
    shifted.extend_from_slice(&counts[..prefix.min(counts.len())]);
    shifted.resize(new_lengths.len() - suffix, 0);
    shifted.extend_from_slice(&counts[counts.len().saturating_sub(suffix)..]);
    shifted.resize(new_lengths.len(), 0);
    shifted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_count_their_visual_rows() {
        let rows = ShownRows { lines: vec![0, 1, 4], tops: Some(vec![0, 2, 3, 6]) };
        assert_eq!((rows.top(1), rows.height(0), rows.height(2), rows.total()), (2, 2, 3, 6));
        assert_eq!([0, 1, 2, 3, 5, 6, 99].map(|visual| rows.row_at(visual)), [0, 0, 1, 2, 2, 3, 3]);
        let plain = ShownRows::new(vec![0, 1, 2]);
        assert_eq!((plain.top(2), plain.total(), plain.row_at(1), plain.height(1)), (2, 3, 1, 1));
    }

    #[test]
    fn an_edit_keeps_the_counts_of_the_lines_it_did_not_touch() {
        assert_eq!(shifted(&[5, 9, 7, 3], &[5, 12, 7, 3], &[1, 2, 3, 4]), [1, 0, 3, 4]);
        assert_eq!(shifted(&[5, 9, 7, 3], &[5, 4, 6, 7, 3], &[1, 2, 3, 4]), [1, 0, 0, 3, 4], "a line split in two");
        assert_eq!(shifted(&[5, 9, 7, 3], &[5, 3], &[1, 2, 3, 4]), [1, 4], "lines deleted");
        assert_eq!(shifted(&[2, 2], &[2, 2, 2], &[1, 1]), [1, 1, 0]);
        assert_eq!(shifted(&[], &[4], &[]), [0]);
    }
}
