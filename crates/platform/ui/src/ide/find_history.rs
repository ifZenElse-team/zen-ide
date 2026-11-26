use egui::{Align2, Color32, Key, Rect, Sense, Ui, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::IdePalette;
use super::glyphs::stroke;
use crate::{accessibility, theme};

const SHOWN: usize = 20;
const ROW: f32 = 24.0;

#[derive(Debug, Default)]
pub(super) struct HistoryPopup {
    entries: Option<Vec<String>>,
    selected: usize,
}

impl HistoryPopup {
    pub(super) fn is_open(&self) -> bool {
        self.entries.is_some()
    }

    pub(super) fn open(&mut self, registry: &Registry, session: &mut Session, replacements: bool) {
        let data = registry.dispatch(session, "search.findHistory.list", json!({})).unwrap_or_default();
        let key = if replacements { "replacements" } else { "queries" };
        let entries = data[key]
            .as_array()
            .map(|list| list.iter().filter_map(|entry| entry.as_str().map(str::to_owned)).take(SHOWN).collect())
            .unwrap_or_default();
        self.entries = Some(entries);
        self.selected = 0;
    }

    pub(super) fn close(&mut self) {
        self.entries = None;
    }

    pub(super) fn keys(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        replacements: bool,
    ) -> Option<String> {
        if !self.is_open() {
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::ALT, Key::ArrowDown)) {
                self.open(registry, session, replacements);
            }
            return None;
        }
        let count = self.entries.as_ref().map_or(0, Vec::len);
        let (mut chosen, mut close) = (None, false);
        ui.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                self.selected = (self.selected + 1).min(count.saturating_sub(1));
            }
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                self.selected = self.selected.saturating_sub(1);
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Enter) {
                chosen = self.entries.as_ref().and_then(|entries| entries.get(self.selected).cloned());
                close = true;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Escape) {
                close = true;
            }
        });
        if close {
            self.close();
        }
        chosen
    }

    pub(super) fn show(&mut self, ui: &Ui, field: Rect, button: Rect, palette: &IdePalette) -> Option<String> {
        let entries = self.entries.clone()?;
        let rows = entries.len().max(1);
        let rect = Rect::from_min_size(
            pos2(field.left(), field.bottom() + 2.0),
            vec2(field.width().max(240.0), 8.0 + ROW * rows as f32),
        );
        let mut chosen = None;
        let area = egui::Area::new(ui.scope_id().with("find-history"))
            .order(egui::Order::Tooltip)
            .fixed_pos(rect.min)
            .show(ui.ctx(), |ui| {
                ui.painter().rect_filled(rect, 6, palette.popup);
                ui.painter().rect_stroke(rect, 6, egui::Stroke::new(1.0, palette.tab_border), egui::StrokeKind::Inside);
                if entries.is_empty() {
                    ui.painter().text(
                        pos2(rect.left() + 12.0, rect.top() + 4.0 + ROW / 2.0),
                        Align2::LEFT_CENTER,
                        t!("ide-find-history-empty"),
                        theme::regular(13.0),
                        palette.text_secondary,
                    );
                    ui.allocate_rect(rect, Sense::hover());
                    return;
                }
                for (index, entry) in entries.iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(rect.left() + 4.0, rect.top() + 4.0 + index as f32 * ROW),
                        vec2(rect.width() - 8.0, ROW),
                    );
                    let response = ui.interact(row, ui.scope_id().with(("find-history-row", index)), Sense::click());
                    let shown = entry.replace('\n', " ⏎ ");
                    accessibility::item(&response, egui::Role::ListBoxOption, index == self.selected, &shown);
                    if response.hovered() {
                        self.selected = index;
                    }
                    if index == self.selected {
                        ui.painter().rect_filled(row, 4, palette.selection);
                    }
                    let galley = ui.painter().layout(shown, theme::regular(13.0), palette.text, row.width() - 16.0);
                    let galley = if galley.rows.len() > 1 {
                        ui.painter().layout_no_wrap(
                            entry.lines().next().unwrap_or_default().to_owned() + " …",
                            theme::regular(13.0),
                            palette.text,
                        )
                    } else {
                        galley
                    };
                    ui.painter().galley(
                        pos2(row.left() + 8.0, row.center().y - galley.size().y / 2.0),
                        galley,
                        palette.text,
                    );
                    if response.clicked() {
                        chosen = Some(entry.clone());
                    }
                }
            });
        let outside = ui.input(|input| input.pointer.any_pressed())
            && ui
                .input(|input| input.pointer.interact_pos())
                .is_some_and(|at| !area.response.rect.contains(at) && !field.contains(at) && !button.contains(at));
        if chosen.is_some() || outside {
            self.close();
        }
        chosen
    }
}

pub(super) fn glyph_history(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let lens = Rect::from_min_size(rect.min, rect.size() * 0.78);
    painter.circle_stroke(lens.center() - vec2(lens.width() * 0.08, lens.height() * 0.08), lens.width() * 0.33, s);
    painter.line_segment([lens.center() + vec2(lens.width() * 0.16, lens.height() * 0.16), lens.right_bottom()], s);
    let (x, y) = (rect.right() - rect.width() * 0.12, rect.bottom() - rect.height() * 0.12);
    let w = rect.width() * 0.14;
    painter.line_segment([pos2(x - w, y - w * 0.6), pos2(x, y)], s);
    painter.line_segment([pos2(x, y), pos2(x + w, y - w * 0.6)], s);
}

pub(super) fn glyph_new_line(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (right, top, bottom) =
        (rect.right() - rect.width() * 0.15, rect.top() + rect.height() * 0.2, rect.center().y + rect.height() * 0.12);
    let left = rect.left() + rect.width() * 0.15;
    painter.line_segment([pos2(right, top), pos2(right, bottom)], s);
    painter.line_segment([pos2(right, bottom), pos2(left, bottom)], s);
    let w = rect.width() * 0.18;
    painter.line_segment([pos2(left, bottom), pos2(left + w, bottom - w)], s);
    painter.line_segment([pos2(left, bottom), pos2(left + w, bottom + w)], s);
}
