use egui::{Key, Modifiers, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_run::TestStatus;

use super::run_window::status_look;
use super::{IdePalette, IdeView};
use crate::theme;

const ROW: f32 = 24.0;
const VISIBLE: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) status: TestStatus,
}

#[derive(Debug)]
struct Shown {
    entries: Vec<Entry>,
    selected: usize,
    at: Pos2,
}

#[derive(Debug, Default)]
pub(super) struct TestHistoryPopup {
    shown: Option<Shown>,
}

impl TestHistoryPopup {
    pub(super) fn open(&mut self, list: &Value, at: Pos2, offset: i64) {
        let entries = list["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let when = super::local_history::when(entry["finishedAt"].as_u64()?, offset);
                let name = entry["name"].as_str()?;
                Some(Entry {
                    id: entry["id"].as_str()?.to_owned(),
                    label: t!("ide-test-history-entry", name = name, when = when.as_str()),
                    status: serde_json::from_value(entry["status"].clone()).ok()?,
                })
            })
            .collect();
        self.shown = Some(Shown { entries, selected: 0, at });
    }

    pub(super) fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    #[cfg(test)]
    pub(super) fn entries(&self) -> Vec<Entry> {
        self.shown.as_ref().map(|shown| shown.entries.clone()).unwrap_or_default()
    }

    pub(super) fn show(&mut self, ui: &Ui, p: &IdePalette) -> Option<String> {
        let shown = self.shown.as_mut()?;
        let count = shown.entries.len();
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if count > 0 {
            if up {
                shown.selected = shown.selected.checked_sub(1).unwrap_or(count - 1);
            }
            if down {
                shown.selected = (shown.selected + 1) % count;
            }
        }
        if escape {
            self.shown = None;
            return None;
        }
        if enter && count > 0 {
            let id = shown.entries[shown.selected].id.clone();
            self.shown = None;
            return Some(id);
        }
        let mut chosen = None;
        let area = egui::Area::new(ui.scope_id().with("test-history-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(shown.at)
            .show(ui.ctx(), |ui| {
                p.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.set_width(420.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(t!("ide-test-history")).font(theme::semibold(12.5)).color(p.text));
                    });
                    ui.add_space(4.0);
                    if shown.entries.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(t!("ide-test-history-empty"))
                                    .font(theme::regular(13.0))
                                    .color(p.text_secondary),
                            );
                        });
                        return;
                    }
                    egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]).show(
                        ui,
                        |ui| {
                            for (index, entry) in shown.entries.iter().enumerate() {
                                let (rect, response) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                                if index == shown.selected {
                                    ui.painter().rect_filled(rect, 4, p.selection);
                                } else if response.hovered() {
                                    ui.painter().rect_filled(rect, 4, p.popup_hover);
                                }
                                crate::accessibility::item(
                                    &response,
                                    egui::Role::ListBoxOption,
                                    index == shown.selected,
                                    &entry.label,
                                );
                                let (glyph, color) = status_look(entry.status, p);
                                glyph(
                                    ui.painter(),
                                    Rect::from_center_size(
                                        pos2(rect.left() + 14.0, rect.center().y),
                                        Vec2::splat(14.0),
                                    ),
                                    color,
                                );
                                ui.painter().with_clip_rect(rect).text(
                                    pos2(rect.left() + 30.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    &entry.label,
                                    theme::regular(13.0),
                                    p.text,
                                );
                                if response.clicked() {
                                    chosen = Some(entry.id.clone());
                                }
                            }
                        },
                    );
                });
            });
        let pressed_outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| !area.response.rect.contains(pos))
        });
        if chosen.is_some() || pressed_outside {
            self.shown = None;
        }
        chosen
    }
}

impl IdeView {
    pub(super) fn open_test_history(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        at: Pos2,
    ) -> Result<Value, DispatchError> {
        let list = registry.dispatch(session, "run.history.list", json!({}))?;
        self.test_history.open(&list, at, i64::from(zen_vcs::local_offset()));
        Ok(list)
    }

    pub(super) fn test_history_popup(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        let Some(id) = self.test_history.show(ui, &palette) else { return };
        match registry.dispatch(session, "run.history.open", json!({ "id": id })) {
            Ok(_) => self.show_tool("run"),
            Err(error) => self.report(ui, error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_lists_entries_with_their_status_and_enter_reopens_the_selected_one() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let mut popup = TestHistoryPopup::default();
        let list = json!({ "entries": [
            { "id": "2", "name": "shop", "finishedAt": 1_000_000, "status": "failed", "counts": {} },
            { "id": "1", "name": "shop", "finishedAt": 900_000, "status": "passed", "counts": {} }
        ] });
        popup.open(&list, Pos2::ZERO, 0);
        let entries = popup.entries();
        assert_eq!(
            entries.iter().map(|entry| (entry.id.as_str(), entry.status)).collect::<Vec<_>>(),
            [("2", TestStatus::Failed), ("1", TestStatus::Passed)]
        );
        assert!(entries[0].label.starts_with("shop — "), "{}", entries[0].label);
        let ctx = egui::Context::default();
        let key = |key: Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let frame = |events: Vec<egui::Event>, popup: &mut TestHistoryPopup| {
            let mut out = None;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                out = popup.show(ui, &IdePalette::zen_dark());
            });
            output.textures_delta.clear();
            out
        };
        assert_eq!(frame(vec![key(Key::ArrowDown)], &mut popup), None);
        assert_eq!(frame(vec![key(Key::Enter)], &mut popup).as_deref(), Some("1"));
        assert!(!popup.is_open());
        popup.open(&json!({ "entries": [] }), Pos2::ZERO, 0);
        assert_eq!(frame(vec![key(Key::Enter)], &mut popup), None);
        frame(vec![key(Key::Escape)], &mut popup);
        assert!(!popup.is_open());
    }
}
