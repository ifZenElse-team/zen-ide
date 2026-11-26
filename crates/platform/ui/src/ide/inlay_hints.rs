use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, CornerRadius, FontId, Rect, Ui, pos2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::IdePalette;

const GAP: f32 = 2.0;
const PAD: f32 = 4.0;
const PADDING: f32 = 3.0;
const FONT_SCALE: f32 = 0.88;
const LIST_MARGIN: usize = 50;
const MEASURED_LABELS: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ShownHint {
    pub(super) id: u64,
    pub(super) offset: usize,
    pub(super) label: String,
    pub(super) padding_left: bool,
    pub(super) padding_right: bool,
    pub(super) tooltip: Option<String>,
    pub(super) resolvable: bool,
}

impl ShownHint {
    fn from_json(hint: &Value) -> Option<Self> {
        Some(Self {
            id: hint["id"].as_u64()?,
            offset: hint["offset"].as_u64()? as usize,
            label: hint["label"].as_str()?.to_owned(),
            padding_left: hint["paddingLeft"].as_bool().unwrap_or(false),
            padding_right: hint["paddingRight"].as_bool().unwrap_or(false),
            tooltip: hint["tooltip"].as_str().map(str::to_owned),
            resolvable: hint["resolvable"].as_bool().unwrap_or(false),
        })
    }
}

struct Listed {
    path: PathBuf,
    generation: u64,
    lines: (usize, usize),
    hints: Arc<Vec<ShownHint>>,
}

struct Measured {
    hints: Arc<Vec<ShownHint>>,
    font: FontId,
    widths: Arc<Vec<f32>>,
    labels: HashMap<String, f32>,
}

struct Widest {
    hints: Arc<Vec<ShownHint>>,
    widths: Arc<Vec<f32>>,
    version: u64,
    char_width: u32,
    width: f32,
}

#[derive(Default)]
pub(super) struct InlayHints {
    listed: Option<Listed>,
    measured: Option<Measured>,
    widest: Option<Widest>,
    hovered: Option<u64>,
    resolving: HashSet<u64>,
    drawn: Vec<(Rect, u64)>,
}

impl InlayHints {
    pub(super) fn follow(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        lines: (usize, usize),
    ) -> Arc<Vec<ShownHint>> {
        if let Some(id) = self.hovered.take()
            && self.resolving.insert(id)
        {
            let _ = registry.dispatch(session, "editor.inlayHints.resolve", json!({ "path": path, "id": id }));
        }
        let kept = self
            .listed
            .as_ref()
            .filter(|listed| listed.path == path && listed.lines.0 <= lines.0 && lines.1 <= listed.lines.1);
        let since = kept.map(|listed| listed.generation);
        let lines = kept
            .map(|listed| listed.lines)
            .unwrap_or((lines.0.saturating_sub(LIST_MARGIN), lines.1.saturating_add(LIST_MARGIN)));
        let params = json!({ "path": path, "fromLine": lines.0 + 1, "toLine": lines.1 + 1, "since": since });
        match registry.dispatch(session, "editor.inlayHints.list", params) {
            Ok(answer) => {
                if answer["pending"] == true {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                }
                let generation = answer["generation"].as_u64().unwrap_or(0);
                if answer["enabled"] == false {
                    self.listed = None;
                } else if let Some(hints) = answer["hints"].as_array() {
                    let hints: Vec<ShownHint> = hints.iter().filter_map(ShownHint::from_json).collect();
                    if self.listed.as_ref().is_none_or(|listed| listed.path != path) {
                        self.resolving.clear();
                    }
                    self.listed = Some(Listed { path: path.to_owned(), generation, lines, hints: Arc::new(hints) });
                }
            }
            Err(_) => self.listed = None,
        }
        self.listed
            .as_ref()
            .filter(|listed| listed.path == path)
            .map(|listed| Arc::clone(&listed.hints))
            .unwrap_or_default()
    }

    pub(super) fn widths(&mut self, ui: &Ui, hints: &Arc<Vec<ShownHint>>, font: &FontId) -> Arc<Vec<f32>> {
        if let Some(measured) = &self.measured
            && Arc::ptr_eq(&measured.hints, hints)
            && measured.font == *font
        {
            return Arc::clone(&measured.widths);
        }
        let mut labels = self
            .measured
            .take()
            .filter(|measured| measured.font == *font)
            .map(|measured| measured.labels)
            .unwrap_or_default();
        if labels.len() > MEASURED_LABELS {
            labels.clear();
        }
        let widths: Arc<Vec<f32>> = Arc::new(
            hints
                .iter()
                .map(|hint| match labels.get(&hint.label) {
                    Some(&width) => width,
                    None => {
                        let width = ui.fonts_mut(|fonts| {
                            fonts.layout_no_wrap(hint.label.clone(), font.clone(), Color32::WHITE).size().x
                        });
                        labels.insert(hint.label.clone(), width);
                        width
                    }
                })
                .collect(),
        );
        self.measured =
            Some(Measured { hints: Arc::clone(hints), font: font.clone(), widths: Arc::clone(&widths), labels });
        widths
    }

    #[allow(clippy::too_many_arguments, reason = "the inputs of one cached width")]
    pub(super) fn widest(
        &mut self,
        hints: &Arc<Vec<ShownHint>>,
        widths: &Arc<Vec<f32>>,
        version: u64,
        char_width: f32,
        line_of: impl Fn(usize) -> usize,
        columns_of: impl Fn(usize) -> usize,
    ) -> f32 {
        if let Some(widest) = &self.widest
            && Arc::ptr_eq(&widest.hints, hints)
            && Arc::ptr_eq(&widest.widths, widths)
            && widest.version == version
            && widest.char_width == char_width.to_bits()
        {
            return widest.width;
        }
        let mut width = 0.0f32;
        let mut current: Option<(usize, f32)> = None;
        for (hint, &label) in hints.iter().zip(widths.iter()) {
            let line = line_of(hint.offset);
            let room = label + room_around(hint);
            current = match current {
                Some((at, sum)) if at == line => Some((at, sum + room)),
                previous => {
                    if let Some((at, sum)) = previous {
                        width = width.max(columns_of(at) as f32 * char_width + sum);
                    }
                    Some((line, room))
                }
            };
        }
        if let Some((at, sum)) = current {
            width = width.max(columns_of(at) as f32 * char_width + sum);
        }
        self.widest = Some(Widest {
            hints: Arc::clone(hints),
            widths: Arc::clone(widths),
            version,
            char_width: char_width.to_bits(),
            width,
        });
        width
    }

    pub(super) fn set_drawn(&mut self, drawn: Vec<(Rect, u64)>) {
        self.drawn = drawn;
    }

    #[cfg(test)]
    pub(super) fn drawn(&self) -> &[(Rect, u64)] {
        &self.drawn
    }

    pub(super) fn hover(&mut self, hint: &ShownHint) {
        if hint.resolvable {
            self.hovered = Some(hint.id);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placed<'a> {
    pub(super) display: usize,
    pub(super) column: usize,
    pub(super) width: f32,
    pub(super) hint: &'a ShownHint,
}

pub(super) fn font(editor: &FontId) -> FontId {
    FontId::new(editor.size * FONT_SCALE, editor.family.clone())
}

pub(super) fn on_line(hints: &[ShownHint], line_start: usize, line_len: usize) -> std::ops::Range<usize> {
    let first = hints.partition_point(|hint| hint.offset < line_start);
    let last = hints.partition_point(|hint| hint.offset <= line_start + line_len);
    first..last.max(first)
}

fn room_around(hint: &ShownHint) -> f32 {
    2.0 * (GAP + PAD) + f32::from(u8::from(hint.padding_left) + u8::from(hint.padding_right)) * PADDING
}

pub(super) fn place<'a>(
    (display, columns): (String, Vec<usize>),
    hints: &'a [ShownHint],
    widths: &[f32],
    line_start: usize,
) -> (String, Vec<usize>, Vec<Placed<'a>>) {
    if hints.is_empty() {
        return (display, columns, Vec::new());
    }
    let len = columns.len() - 1;
    let at: Vec<usize> = hints.iter().map(|hint| hint.offset.saturating_sub(line_start).min(len)).collect();
    let mut placed = Vec::with_capacity(hints.len());
    for (index, hint) in hints.iter().enumerate() {
        placed.push(Placed {
            display: columns[at[index]] + index,
            column: at[index],
            width: widths.get(index).copied().unwrap_or_default() + room_around(hint),
            hint,
        });
    }
    let shifted: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(column, &shown)| shown + at.iter().filter(|&&hint| hint < column).count())
        .collect();
    let mut chars: Vec<char> = display.chars().collect();
    for place in &placed {
        chars.insert(place.display.min(chars.len()), ' ');
    }
    (chars.into_iter().collect(), shifted, placed)
}

pub(super) fn after(columns: &[usize], placed: &[Placed], column: usize) -> usize {
    columns[column] + placed.iter().filter(|place| place.column == column).count()
}

pub(super) fn on_row<'a>(placed: &[Placed<'a>], display: std::ops::Range<usize>) -> Vec<Placed<'a>> {
    placed
        .iter()
        .filter(|place| display.contains(&place.display))
        .map(|&place| Placed { display: place.display - display.start, ..place })
        .collect()
}

pub(super) fn make_room(job: &mut LayoutJob, display: &str, placed: &[Placed], advance: impl FnOnce() -> f32) {
    if placed.is_empty() {
        return;
    }
    let advance = advance();
    if job.text.len() < display.len() && display.starts_with(job.text.as_str()) {
        let format = job.sections.last().map(|section| section.format.clone()).unwrap_or_default();
        let tail = display[job.text.len()..].to_owned();
        job.append(&tail, 0.0, format);
    }
    let bytes: Vec<usize> = job.text.char_indices().map(|(byte, _)| byte).collect();
    for place in placed {
        let Some(&byte) = bytes.get(place.display + 1) else { continue };
        split_at(job, byte);
        if let Some(section) = job.sections.iter_mut().find(|section| section.byte_range.start.0 == byte) {
            section.leading_space += (place.width - advance).max(0.0);
        }
    }
}

fn split_at(job: &mut LayoutJob, byte: usize) {
    let Some(index) =
        job.sections.iter().position(|section| section.byte_range.start.0 < byte && byte < section.byte_range.end.0)
    else {
        return;
    };
    let section = job.sections[index].clone();
    job.sections[index].byte_range = section.byte_range.start..ByteIndex(byte);
    job.sections.insert(
        index + 1,
        LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(byte)..section.byte_range.end,
            format: section.format,
        },
    );
}

pub(super) fn plain_galley(ui: &Ui, display: String, placed: &[Placed], font: &FontId) -> Arc<egui::Galley> {
    if placed.is_empty() {
        return ui.fonts_mut(|fonts| fonts.layout_no_wrap(display, font.clone(), Color32::WHITE));
    }
    let mut job = LayoutJob::single_section(
        display,
        TextFormat { font_id: font.clone(), color: Color32::WHITE, ..TextFormat::default() },
    );
    make_room(&mut job, "", placed, || ui.fonts_mut(|fonts| fonts.glyph_width(font, ' ')));
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

pub(super) fn boxes(galley: &egui::Galley, origin_x: f32, top: f32, line_height: f32, placed: &[Placed]) -> Vec<Rect> {
    placed
        .iter()
        .map(|place| {
            let left = origin_x + galley.pos_from_cursor(egui::text::CCursor::new(place.display)).min.x;
            let before = GAP + if place.hint.padding_left { PADDING } else { 0.0 };
            let after = GAP + if place.hint.padding_right { PADDING } else { 0.0 };
            Rect::from_min_max(
                pos2(left + before, top + 3.0),
                pos2(left + place.width - after, top + line_height - 3.0),
            )
        })
        .collect()
}

pub(super) fn paint(painter: &egui::Painter, boxes: &[Rect], placed: &[Placed], font: &FontId, palette: &IdePalette) {
    for (rect, place) in boxes.iter().zip(placed) {
        painter.rect_filled(*rect, CornerRadius::same(4), palette.inlay_hint_background);
        let label = painter.layout_no_wrap(place.hint.label.clone(), font.clone(), palette.inlay_hint_foreground);
        let at = pos2(rect.left() + PAD, rect.center().y - label.size().y / 2.0);
        painter.galley(at, label, palette.inlay_hint_foreground);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(offset: usize, label: &str) -> ShownHint {
        ShownHint {
            id: offset as u64,
            offset,
            label: label.to_owned(),
            padding_left: false,
            padding_right: false,
            tooltip: None,
            resolvable: false,
        }
    }

    #[test]
    fn a_hint_takes_a_space_and_the_columns_step_over_it() {
        let line = ("f(a, b)".to_owned(), (0..=7).collect::<Vec<usize>>());
        let hints = [hint(12, "x:"), hint(15, "y:")];
        let (display, columns, placed) = place(line, &hints, &[2.0, 2.0], 10);
        assert_eq!(display, "f( a,  b)");
        assert_eq!(columns, [0, 1, 2, 4, 5, 6, 8, 9]);
        assert_eq!(placed.iter().map(|place| (place.display, place.column)).collect::<Vec<_>>(), [(2, 2), (6, 5)]);
        assert_eq!(placed[0].width, 2.0 + 2.0 * (GAP + PAD));
        assert_eq!((after(&columns, &placed, 2), after(&columns, &placed, 3)), (3, 4));
        assert_eq!(on_line(&hints, 10, 7), 0..2);
        assert!(on_line(&hints, 0, 9).is_empty());
        let twice = [hint(2, "t"), hint(2, "u")];
        let (display, columns, placed) = place(("ab".to_owned(), vec![0, 1, 2]), &twice, &[1.0, 1.0], 0);
        assert_eq!((display.as_str(), columns.clone(), placed[1].display), ("ab  ", vec![0, 1, 2], 3));
        assert_eq!(after(&columns, &placed, 2), 4);
    }

    #[test]
    fn a_line_without_hints_keeps_its_text_and_columns_untouched() {
        let display = "f(a, b)".to_owned();
        let columns: Vec<usize> = (0..=7).collect();
        let (text_at, columns_at) = (display.as_ptr(), columns.as_ptr());
        let (display, columns, placed) = place((display, columns), &[], &[], 0);
        assert_eq!((display.as_ptr(), columns.as_ptr()), (text_at, columns_at), "nothing is copied");
        assert_eq!(placed.capacity(), 0, "nothing is allocated");
        assert_eq!(after(&columns, &placed, 3), 3);
        let mut job = LayoutJob::single_section(display.clone(), TextFormat::default());
        make_room(&mut job, &display, &placed, || unreachable!("no width is measured"));
        assert_eq!(job.sections.len(), 1);
    }

    #[test]
    fn the_widest_line_counts_its_hints() {
        let mut shown = InlayHints::default();
        let hints = Arc::new(vec![hint(1, "a"), hint(2, "b"), hint(20, "c")]);
        let widths = Arc::new(vec![10.0, 10.0, 10.0]);
        let line_of = |offset: usize| offset / 10;
        let columns_of = |line: usize| if line == 0 { 8 } else { 30 };
        let widest = shown.widest(&hints, &widths, 1, 2.0, line_of, columns_of);
        let room = 10.0 + 2.0 * (GAP + PAD);
        assert_eq!(widest, (16.0 + 2.0 * room).max(60.0 + room));
        let cached = shown.widest(&hints, &widths, 1, 2.0, |_| unreachable!(), |_| unreachable!());
        assert_eq!(cached, widest);
    }

    #[test]
    fn the_character_after_a_hint_is_pushed_right_by_its_width() {
        let shown = hint(2, "x:");
        let mut job = LayoutJob::single_section("f( a)".to_owned(), TextFormat::default());
        let placed = vec![Placed { display: 2, column: 2, width: 30.0, hint: &shown }];
        make_room(&mut job, "f( a)", &placed, || 8.0);
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[1].byte_range.start.0, 3);
        assert_eq!(job.sections[1].leading_space, 22.0);
        let mut short = LayoutJob::single_section("ab".to_owned(), TextFormat::default());
        let at_end = vec![Placed { display: 2, column: 2, width: 30.0, hint: &shown }];
        make_room(&mut short, "ab ", &at_end, || 8.0);
        assert_eq!(short.text, "ab ", "a hint at the end of the line keeps its space");
    }
}
