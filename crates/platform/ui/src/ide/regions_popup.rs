use egui::{Key, Modifiers, Pos2, Sense, Ui, pos2, vec2};
use serde_json::Value;
use zen_i18n::t;

use super::IdePalette;
use crate::theme;

const ROW: f32 = 22.0;
const VISIBLE: usize = 14;
const INDENT: f32 = 14.0;

#[derive(Clone, Debug, PartialEq)]
struct Region {
    start: usize,
    line: usize,
    depth: usize,
    name: String,
}

#[derive(Clone, Debug)]
struct Shown {
    regions: Vec<Region>,
    selected: usize,
    at: Option<Pos2>,
    reveal: bool,
}

#[derive(Default)]
pub(super) struct RegionsPopup {
    shown: Option<Shown>,
}

impl RegionsPopup {
    pub(super) fn open(&mut self, regions: &Value) -> bool {
        let regions: Vec<Region> = regions
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|region| {
                let line = region["line"].as_u64()? as usize;
                let name = region["description"].as_str().unwrap_or_default();
                Some(Region {
                    start: region["start"].as_u64()? as usize,
                    line,
                    depth: region["depth"].as_u64().unwrap_or_default() as usize,
                    name: if name.is_empty() {
                        t!("ide-custom-folding-unnamed", line = line + 1)
                    } else {
                        name.to_owned()
                    },
                })
            })
            .collect();
        self.shown = (!regions.is_empty()).then_some(Shown { regions, selected: 0, at: None, reveal: true });
        self.shown.is_some()
    }

    pub(super) fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Option<usize> {
        let shown = self.shown.as_mut()?;
        let count = shown.regions.len();
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
        }
        if down {
            shown.selected = (shown.selected + 1) % count;
        }
        shown.reveal |= up || down;
        if enter {
            let start = shown.regions[shown.selected].start;
            self.shown = None;
            return Some(start);
        }
        if escape {
            self.shown = None;
        }
        None
    }

    pub(super) fn show(&mut self, ui: &Ui, palette: &IdePalette, caret: Pos2) -> Option<usize> {
        let shown = self.shown.as_mut()?;
        let at = *shown.at.get_or_insert(caret);
        let width = 320.0_f32.min(ui.ctx().content_rect().width() - 40.0).max(200.0);
        let mut chosen = None;
        let area = egui::Area::new(ui.scope_id().with("custom-folding-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        let title = t!("ide-custom-folding-title");
                        ui.label(egui::RichText::new(title).font(theme::semibold(12.5)).color(palette.text));
                    });
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]).show(
                        ui,
                        |ui| {
                            for (index, region) in shown.regions.iter().enumerate() {
                                let (rect, response) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                                if index == shown.selected {
                                    ui.painter().rect_filled(rect, 4, palette.selection);
                                    if shown.reveal {
                                        response.scroll_to_me(None);
                                    }
                                } else if response.hovered() {
                                    ui.painter().rect_filled(rect, 4, palette.popup_hover);
                                }
                                crate::accessibility::name_with(&response, egui::Role::ListBoxOption, || {
                                    region.name.clone()
                                });
                                crate::accessibility::edit(&response, |node| {
                                    node.set_selected(index == shown.selected)
                                });
                                let painter = ui.painter();
                                let name =
                                    painter.layout_no_wrap(region.name.clone(), theme::regular(13.0), palette.text);
                                let left = rect.left() + 8.0 + region.depth as f32 * INDENT;
                                painter.galley(pos2(left, rect.center().y - name.size().y / 2.0), name, palette.text);
                                let line = painter.layout_no_wrap(
                                    (region.line + 1).to_string(),
                                    theme::regular(12.0),
                                    palette.text_secondary,
                                );
                                let place =
                                    pos2(rect.right() - 8.0 - line.size().x, rect.center().y - line.size().y / 2.0);
                                painter.galley(place, line, palette.text_secondary);
                                if response.clicked() {
                                    chosen = Some(index);
                                }
                            }
                        },
                    );
                });
            });
        shown.reveal = false;
        if let Some(index) = chosen {
            let start = shown.regions[index].start;
            self.shown = None;
            return Some(start);
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
    use serde_json::json;

    fn regions() -> Value {
        json!([
            { "description": "Helpers", "start": 14, "end": 90, "line": 2, "depth": 0, "style": "region", "folded": false },
            { "description": "", "start": 40, "end": 70, "line": 5, "depth": 1, "style": "editorFold", "folded": true },
        ])
    }

    #[test]
    fn the_list_names_the_regions_and_enter_goes_to_the_chosen_one() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let mut popup = RegionsPopup::default();
        assert!(!popup.open(&json!([])), "nothing to list");
        assert!(popup.open(&regions()));
        let shown = popup.shown.clone().unwrap();
        assert_eq!(shown.regions[0].name, "Helpers");
        assert_eq!(shown.regions[1].name, t!("ide-custom-folding-unnamed", line = 6));
        assert_eq!(shown.regions[1].depth, 1);
        let ctx = egui::Context::default();
        let key = |key: Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let frame = |events: Vec<egui::Event>, popup: &mut RegionsPopup| {
            let mut out = None;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                out = popup.keys(ui).or(popup.show(ui, &IdePalette::zen_dark(), Pos2::ZERO));
            });
            output.textures_delta.clear();
            out
        };
        assert_eq!(frame(vec![key(Key::ArrowDown)], &mut popup), None);
        assert_eq!(frame(vec![key(Key::Enter)], &mut popup), Some(40));
        assert!(!popup.is_open());
        popup.open(&regions());
        frame(vec![key(Key::Escape)], &mut popup);
        assert!(!popup.is_open());
    }
}
