use std::collections::BTreeSet;
use std::sync::Arc;

use egui::{Align2, Event, Key, Sense, Ui, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::clipboard_history::{ClipboardHistory, preview};

use super::{IdePalette, dialog_frame};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const ROWS: usize = 10;
const PREVIEW_LINES: usize = 12;
const ROW_CHARS: usize = 90;
const PREVIEW_LINE_CHARS: usize = 200;

#[derive(Default)]
pub(super) struct PasteHistory {
    open: bool,
    filter: String,
    selected: usize,
    marked: BTreeSet<usize>,
    anchor: usize,
    rows: Vec<Row>,
    built_for: Option<(u64, String)>,
}

struct Row {
    index: usize,
    text: Arc<str>,
    preview: String,
    lines: usize,
}

impl PasteHistory {
    #[cfg(test)]
    pub(super) fn typed(&self) -> &str {
        &self.filter
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn open(&mut self) {
        *self = Self { open: true, ..Self::default() };
    }

    pub(super) fn close(&mut self) {
        self.open = false;
    }

    pub(super) fn show(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, palette: &IdePalette) {
        if !self.open {
            return;
        }
        let history = session.state::<ClipboardHistory>().cloned().unwrap_or_default();
        self.build_rows(&history);
        let rows: Vec<usize> = self.rows.iter().map(|row| row.index).collect();
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        self.marked.retain(|index| rows.contains(index));

        let (mut paste, mut remove, mut close) = (false, false, false);
        let (mut clicked, mut double_clicked) = (None, None);
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Key { key, pressed, modifiers, .. } => {
                    if *pressed {
                        match key {
                            Key::ArrowDown | Key::ArrowUp => {
                                let down = *key == Key::ArrowDown;
                                let next = if down {
                                    (self.selected + 1).min(rows.len().saturating_sub(1))
                                } else {
                                    self.selected.saturating_sub(1)
                                };
                                self.move_to(next, modifiers.shift, &rows);
                            }
                            Key::Enter => paste = true,
                            Key::Escape => close = true,
                            Key::Delete => remove = true,
                            Key::Backspace => {
                                self.filter.pop();
                            }
                            _ => {}
                        }
                    }
                    false
                }
                Event::Text(text) => {
                    self.filter.push_str(text);
                    self.selected = 0;
                    self.marked.clear();
                    false
                }
                Event::Paste(_) | Event::Copy | Event::Cut | Event::Ime(_) => false,
                _ => true,
            });
        });

        let screen = ui.ctx().content_rect();
        let size = vec2(620.0, 0.0);
        let area = egui::Area::new(ui.scope_id().with("paste-history"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.center().x - size.x / 2.0, screen.top() + screen.height() * 0.18));
        let response = area.show(ui.ctx(), |ui| {
            dialog_frame(palette).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(size.x - 20.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let title = if self.filter.is_empty() {
                    t!("ide-paste-history-title")
                } else {
                    format!("{}: {}", t!("ide-paste-history-title"), self.filter)
                };
                ui.label(egui::RichText::new(title).font(theme::semibold(13.0)).color(palette.text));
                ui.add_space(8.0);
                if rows.is_empty() {
                    let empty = if history.is_empty() { "ide-paste-history-empty" } else { "ide-goto-file-nothing" };
                    ui.label(egui::RichText::new(t!(empty)).font(theme::regular(13.0)).color(palette.text_secondary));
                    return;
                }
                super::filter_popups::list(ui, "paste-history-rows", ROW * ROWS as f32, |ui| {
                    for (row, shown) in self.rows.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        let chosen = row == self.selected || self.marked.contains(&shown.index);
                        accessibility::item(&response, egui::Role::ListBoxOption, chosen, &shown.preview);
                        if chosen {
                            ui.painter().rect_filled(rect, 5, palette.selection);
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 5, palette.popup_hover);
                        }
                        ui.painter().text(
                            pos2(rect.left() + 8.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            &shown.preview,
                            theme::regular(13.0),
                            palette.text,
                        );
                        let lines = shown.lines;
                        if lines > 1 {
                            ui.painter().text(
                                pos2(rect.right() - 8.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                t!("ide-paste-history-lines", count = lines),
                                theme::regular(12.0),
                                palette.text_secondary,
                            );
                        }
                        if response.clicked() {
                            let modifiers = ui.input(|input| input.modifiers);
                            clicked = Some((row, modifiers));
                        }
                        if response.double_clicked() {
                            double_clicked = Some(row);
                        }
                    }
                });
                ui.add_space(8.0);
                let text = self.rows.get(self.selected).map(full_preview).unwrap_or_default();
                egui::Frame::new().fill(palette.editor_background).corner_radius(6).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::horizontal().id_salt("paste-history-preview").show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(text)
                                    .font(egui::FontId::new(12.5, theme::family::editor()))
                                    .color(palette.text),
                            )
                            .extend(),
                        );
                    });
                });
            });
        });
        if let Some((row, modifiers)) = clicked {
            if modifiers.command {
                self.toggle(row, &rows);
            } else {
                self.move_to(row, modifiers.shift, &rows);
            }
        }
        if let Some(row) = double_clicked {
            self.move_to(row, false, &rows);
            paste = true;
        }
        if ui.input(|input| input.pointer.any_pressed())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !response.response.rect.contains(pointer)
        {
            close = true;
        }
        let chosen: Vec<usize> = if self.marked.is_empty() {
            rows.get(self.selected).copied().into_iter().collect()
        } else {
            self.marked.iter().copied().collect()
        };
        if paste && !chosen.is_empty() {
            if registry.dispatch(session, "editor.clipboardHistory.paste", json!({ "indices": chosen })).is_ok()
                && let Some(text) = session.state::<ClipboardHistory>().and_then(|history| history.get(0))
            {
                ui.ctx().copy_text(text.to_string());
            }
            close = true;
        }
        if remove {
            for index in chosen.iter().rev() {
                let _ = registry.dispatch(session, "editor.clipboardHistory.remove", json!({ "index": index }));
            }
            self.marked.clear();
        }
        if close {
            self.open = false;
        }
    }

    fn build_rows(&mut self, history: &ClipboardHistory) {
        let key = (history.generation(), self.filter.to_lowercase());
        if self.built_for.as_ref() == Some(&key) {
            return;
        }
        self.rows = history
            .entries()
            .into_iter()
            .enumerate()
            .filter(|(_, text)| key.1.is_empty() || text.to_lowercase().contains(&key.1))
            .map(|(index, text)| Row { index, preview: preview(&text, ROW_CHARS), lines: text.lines().count(), text })
            .collect();
        self.built_for = Some(key);
    }

    fn move_to(&mut self, row: usize, extend: bool, rows: &[usize]) {
        if extend {
            if self.marked.is_empty() {
                self.anchor = self.selected;
            }
            let (from, to) = (self.anchor.min(row), self.anchor.max(row));
            self.marked = rows.iter().skip(from).take(to - from + 1).copied().collect();
        } else {
            self.marked.clear();
            self.anchor = row;
        }
        self.selected = row;
    }

    fn toggle(&mut self, row: usize, rows: &[usize]) {
        if self.marked.is_empty()
            && let Some(index) = rows.get(self.selected)
        {
            self.marked.insert(*index);
        }
        if let Some(index) = rows.get(row)
            && !self.marked.remove(index)
        {
            self.marked.insert(*index);
        }
        self.selected = row;
        self.anchor = row;
    }
}

fn full_preview(row: &Row) -> String {
    let mut text = row
        .text
        .lines()
        .take(PREVIEW_LINES)
        .map(|line| {
            if line.chars().count() <= PREVIEW_LINE_CHARS {
                line.to_owned()
            } else {
                let mut cut: String = line.chars().take(PREVIEW_LINE_CHARS).collect();
                cut.push('…');
                cut
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if row.lines > PREVIEW_LINES {
        text.push_str("\n…");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_extends_and_command_toggles_the_chosen_rows() {
        let rows = [0, 1, 2, 3];
        let mut popup = PasteHistory::default();
        popup.open();
        popup.move_to(1, true, &rows);
        popup.move_to(2, true, &rows);
        assert_eq!(popup.marked.iter().copied().collect::<Vec<_>>(), [0, 1, 2]);
        popup.move_to(3, false, &rows);
        assert!(popup.marked.is_empty());
        popup.toggle(1, &rows);
        assert_eq!(popup.marked.iter().copied().collect::<Vec<_>>(), [1, 3]);
        popup.toggle(3, &rows);
        assert_eq!(popup.marked.iter().copied().collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn rows_are_built_again_only_when_the_history_or_the_filter_changes() {
        let history = ClipboardHistory::default();
        history.record(&format!("{}\nend", "a".repeat(500)), 20);
        history.record("Beta", 20);
        let mut popup = PasteHistory::default();
        popup.open();
        popup.build_rows(&history);
        let built = popup.rows[0].text.as_ptr();
        popup.build_rows(&history);
        assert_eq!(popup.rows[0].text.as_ptr(), built, "the same frame's rows are reused");
        assert_eq!(popup.rows.len(), 2);
        popup.filter.push_str("BET");
        popup.build_rows(&history);
        assert_eq!(popup.rows.iter().map(|row| row.index).collect::<Vec<_>>(), [0]);
        popup.filter.clear();
        history.remove(0);
        popup.build_rows(&history);
        assert_eq!(popup.rows.len(), 1);
        let shown = full_preview(&popup.rows[0]);
        assert_eq!(shown.lines().next().unwrap().chars().count(), PREVIEW_LINE_CHARS + 1, "{shown}");
        assert!(shown.ends_with("\nend"));
    }
}
