use std::sync::Arc;

use egui::{Align2, FontId, Rect, Sense, Ui, pos2};
use zen_workbench::sticky::{Scope, sticky_scopes};

use super::IdePalette;
use super::metrics::gutter;

const SHADOW: f32 = 4.0;

#[derive(Default)]
pub(super) struct StickyLines {
    scroll_to: Option<(usize, usize)>,
    #[cfg(test)]
    shown: Vec<usize>,
}

pub(super) struct StickyFrame<'a> {
    pub area: Rect,
    pub gutter: Rect,
    pub text_x: f32,
    pub number_right: f32,
    pub numbers: bool,
    pub numbering: super::line_numbers::Numbering<'a>,
    pub line_height: f32,
    pub number_font: FontId,
    pub caret_line: usize,
    pub lines: &'a [(usize, Arc<egui::Galley>)],
}

impl StickyLines {
    pub(super) fn pick(scopes: &[Scope], rows: &[usize], top_row: usize, max: usize) -> Vec<usize> {
        let Some(&top_line) = rows.get(top_row) else { return Vec::new() };
        let next_shown = |line: usize| {
            let row = rows.partition_point(|&shown| shown <= line);
            rows.get(row).copied().unwrap_or(line + 1)
        };
        sticky_scopes(scopes, top_line, max, next_shown).iter().map(|scope| scope.start_line).collect()
    }

    pub(super) fn take_scroll(&mut self, row_of: impl Fn(usize) -> usize, line_height: f32) -> Option<f32> {
        let (line, slot) = self.scroll_to.take()?;
        Some(row_of(line).saturating_sub(slot) as f32 * line_height)
    }

    #[cfg(test)]
    pub(super) fn shown(&self) -> &[usize] {
        &self.shown
    }

    pub(super) fn paint_text(ui: &Ui, frame: &StickyFrame, palette: &IdePalette) {
        let text_area = Rect::from_min_max(pos2(frame.gutter.right(), frame.area.top()), frame.area.max);
        let painter = ui.painter().with_clip_rect(text_area);
        for (slot, (_, galley)) in frame.lines.iter().enumerate() {
            let row = row_rect(frame, slot, text_area.left(), text_area.right());
            let background = if ui.rect_contains_pointer(row_rect(frame, slot, frame.area.left(), frame.area.right())) {
                palette.hover
            } else {
                palette.sticky_background
            };
            painter.rect_filled(row, 0, background);
            painter.galley(pos2(frame.text_x, row.top()), Arc::clone(galley), palette.code);
        }
    }

    pub(super) fn paint_gutter(
        &mut self,
        ui: &Ui,
        id: egui::Id,
        frame: &StickyFrame,
        palette: &IdePalette,
    ) -> Option<usize> {
        #[cfg(test)]
        {
            self.shown = frame.lines.iter().map(|(line, _)| *line).collect();
        }
        if frame.lines.is_empty() {
            return None;
        }
        let painter = ui.painter().with_clip_rect(frame.area);
        let right = frame.gutter.right() - gutter::SEPARATOR;
        for (slot, (line, _)) in frame.lines.iter().enumerate() {
            let row = row_rect(frame, slot, frame.gutter.left(), right);
            let hovered = ui.rect_contains_pointer(row_rect(frame, slot, frame.area.left(), frame.area.right()));
            painter.rect_filled(row, 0, if hovered { palette.hover } else { palette.sticky_background });
            if !frame.numbers {
                continue;
            }
            let color = if *line == frame.caret_line { palette.text_secondary } else { palette.gutter_text };
            painter.text(
                pos2(frame.number_right, row.center().y),
                Align2::RIGHT_CENTER,
                frame.numbering.label(*line),
                frame.number_font.clone(),
                color,
            );
        }
        let bottom = frame.area.top() + frame.lines.len() as f32 * frame.line_height;
        let shadow = palette.sticky_border.gamma_multiply(0.5);
        for step in 0..SHADOW as usize {
            let fade = 1.0 - step as f32 / SHADOW;
            let band = Rect::from_x_y_ranges(frame.area.x_range(), bottom + step as f32..=bottom + step as f32 + 1.0);
            painter.rect_filled(band, 0, shadow.gamma_multiply(fade));
        }
        painter.hline(frame.area.x_range(), bottom - 0.5, egui::Stroke::new(1.0, palette.sticky_border));
        let all = Rect::from_x_y_ranges(frame.area.x_range(), frame.area.top()..=bottom);
        let response = ui.interact(all, id.with("sticky-lines"), Sense::click_and_drag());
        crate::accessibility::name_with(&response, egui::Role::Button, || zen_i18n::t!("a11y-sticky-lines"));
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let pos = response.interact_pointer_pos().filter(|_| response.clicked())?;
        let slot =
            (((pos.y - frame.area.top()) / frame.line_height).floor().max(0.0) as usize).min(frame.lines.len() - 1);
        let line = frame.lines[slot].0;
        self.scroll_to = Some((line, slot));
        Some(line)
    }
}

fn row_rect(frame: &StickyFrame, slot: usize, left: f32, right: f32) -> Rect {
    let top = frame.area.top() + slot as f32 * frame.line_height;
    Rect::from_min_max(pos2(left, top), pos2(right, top + frame.line_height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_shown_decide_what_is_under_each_slot() {
        let scopes = [Scope { start_line: 0, end_line: 20 }, Scope { start_line: 4, end_line: 8 }];
        let rows: Vec<usize> = (0..30).collect();
        assert_eq!(StickyLines::pick(&scopes, &rows, 3, 5), [0]);
        assert_eq!(StickyLines::pick(&scopes, &rows, 4, 5), [0, 4]);
        let folded: Vec<usize> = [0, 1].into_iter().chain(5..30).collect();
        assert_eq!(StickyLines::pick(&scopes, &folded, 1, 5), [0, 4], "under line 1 is line 5, inside the inner scope");
        assert_eq!(StickyLines::pick(&scopes, &rows, 40, 5), Vec::<usize>::new());
        let mut sticky = StickyLines { scroll_to: Some((10, 2)), shown: Vec::new() };
        assert_eq!(sticky.take_scroll(|line| line, 20.0), Some(160.0));
        assert_eq!(sticky.take_scroll(|line| line, 20.0), None);
    }
}
