use std::ops::Range;

use egui::{Key, Modifiers, Pos2, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};

use super::IdePalette;
use crate::theme;

const ROW: f32 = 22.0;

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    title: String,
    remove: bool,
    kept: Vec<Range<usize>>,
}

#[derive(Clone, Debug)]
struct Shown {
    entries: Vec<Entry>,
    selected: usize,
    hovered: Option<usize>,
    at: Pos2,
    offset: usize,
    version: u64,
}

#[derive(Default)]
pub(super) struct UnwrapPopup {
    shown: Option<Shown>,
}

fn entries(listed: &Value) -> Vec<Entry> {
    listed
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let kept = entry["kept"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|range| Some(range[0].as_u64()? as usize..range[1].as_u64()? as usize))
                .collect();
            Some(Entry { title: entry["title"].as_str()?.to_owned(), remove: entry["kind"] == "remove", kept })
        })
        .collect()
}

impl UnwrapPopup {
    pub(super) fn open(&mut self, listed: &Value, at: Pos2, offset: usize, version: u64) {
        let entries = entries(listed);
        self.shown =
            (!entries.is_empty()).then_some(Shown { entries, selected: 0, hovered: None, at, offset, version });
    }

    pub(super) fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    pub(super) fn follow(&mut self, version: u64) {
        if self.shown.as_ref().is_some_and(|shown| shown.version != version) {
            self.shown = None;
        }
    }

    pub(super) fn kept(&self) -> &[Range<usize>] {
        self.shown.as_ref().map_or(&[], |shown| shown.entries[shown.hovered.unwrap_or(shown.selected)].kept.as_slice())
    }

    fn choose(&mut self, index: usize) -> Option<Value> {
        let shown = self.shown.take()?;
        Some(json!({ "index": index, "offset": shown.offset }))
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Option<Value> {
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
        if up {
            shown.selected = shown.selected.checked_sub(1).unwrap_or(count - 1);
            shown.hovered = None;
        }
        if down {
            shown.selected = (shown.selected + 1) % count;
            shown.hovered = None;
        }
        if enter {
            let index = shown.selected;
            return self.choose(index);
        }
        if escape {
            self.shown = None;
        }
        None
    }

    pub(super) fn show(&mut self, ui: &Ui, palette: &IdePalette) -> Option<Value> {
        let shown = self.shown.as_mut()?;
        let mut chosen = None;
        let mut hovered = None;
        let area = egui::Area::new(ui.scope_id().with("unwrap-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(shown.at)
            .show(ui.ctx(), |ui| {
                palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let font = theme::regular(13.0);
                    let widest = shown
                        .entries
                        .iter()
                        .map(|entry| {
                            ui.painter().layout_no_wrap(entry.title.clone(), font.clone(), palette.text).size().x
                        })
                        .fold(0.0_f32, f32::max);
                    let width = (widest + 24.0).max(180.0);
                    for (index, entry) in shown.entries.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                        if entry.remove {
                            ui.painter().rect_filled(rect, 4, palette.diff_deleted);
                        }
                        if index == shown.selected {
                            ui.painter().rect_filled(rect, 4, palette.selection);
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 4, palette.popup_hover);
                        }
                        if response.hovered() {
                            hovered = Some(index);
                        }
                        crate::accessibility::name_with(&response, egui::Role::ListBoxOption, || entry.title.clone());
                        crate::accessibility::edit(&response, |node| node.set_selected(index == shown.selected));
                        let galley = ui.painter().layout_no_wrap(entry.title.clone(), font.clone(), palette.text);
                        let top = rect.center().y - galley.size().y / 2.0;
                        ui.painter().galley(pos2(rect.left() + 10.0, top), galley, palette.text);
                        if response.clicked() {
                            chosen = Some(index);
                        }
                    }
                });
            });
        shown.hovered = hovered;
        if let Some(index) = chosen {
            return self.choose(index);
        }
        let pressed_outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| !area.response.rect.contains(pos))
        });
        if pressed_outside {
            self.shown = None;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed() -> Value {
        json!([
            { "index": 0, "kind": "unwrap", "title": "Unwrap 'else…'", "kept": [[40, 44]] },
            { "index": 1, "kind": "remove", "title": "Remove 'else…'", "kept": [[10, 30], [50, 52]] },
        ])
    }

    #[test]
    fn arrows_move_the_highlight_and_enter_applies() {
        let ctx = egui::Context::default();
        let mut popup = UnwrapPopup::default();
        popup.open(&listed(), Pos2::ZERO, 41, 7);
        assert_eq!(popup.kept(), std::slice::from_ref(&(40..44)));
        let key = |key: Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let frame = |events: Vec<egui::Event>, popup: &mut UnwrapPopup| {
            let mut out = None;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                out = popup.keys(ui);
                let _ = popup.show(ui, &IdePalette::zen_dark());
            });
            output.textures_delta.clear();
            out
        };
        assert_eq!(frame(vec![key(Key::ArrowDown)], &mut popup), None);
        assert_eq!(popup.kept(), [10..30, 50..52], "the selected entry shows what stays");
        assert_eq!(frame(vec![key(Key::Enter)], &mut popup), Some(json!({ "index": 1, "offset": 41 })));
        assert!(!popup.is_open());
        popup.open(&listed(), Pos2::ZERO, 41, 7);
        popup.follow(8);
        assert!(!popup.is_open(), "an edit closes it");
        popup.open(&listed(), Pos2::ZERO, 41, 7);
        frame(vec![key(Key::Escape)], &mut popup);
        assert!(!popup.is_open());
        popup.open(&json!([]), Pos2::ZERO, 0, 0);
        assert!(!popup.is_open());
    }
}
