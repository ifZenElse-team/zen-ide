use std::sync::Arc;

use egui::{Event, Key, Modifiers, Pos2, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::Highlighter;

use super::IdePalette;
use crate::theme;

const ROW: f32 = 24.0;
const VISIBLE: usize = 16;

#[derive(Clone, Debug, PartialEq)]
struct Item {
    mnemonic: String,
    name: String,
    template: bool,
    title: String,
    description: String,
    preview: Option<String>,
}

#[derive(Clone, Debug)]
struct Shown {
    items: Vec<Item>,
    templates_only: bool,
    selected: usize,
    hovered: Option<usize>,
    at: Pos2,
    reveal: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SurroundOutcome {
    Apply { name: String, template: bool },
    Configure,
    Notice(String),
}

#[derive(Default)]
pub(super) struct SurroundPopup {
    shown: Option<Shown>,
}

fn items(list: &Value) -> Vec<Item> {
    list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(Item {
                mnemonic: item["mnemonic"].as_str().unwrap_or_default().to_owned(),
                name: item["name"].as_str()?.to_owned(),
                template: item["template"] == true,
                title: item["title"].as_str()?.to_owned(),
                description: item["description"].as_str().unwrap_or_default().to_owned(),
                preview: item["preview"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

impl SurroundPopup {
    pub(super) fn open(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        at: Pos2,
        templates_only: bool,
    ) -> Option<SurroundOutcome> {
        self.shown = None;
        let params = json!({ "templatesOnly": templates_only, "preview": true });
        match registry.dispatch(session, "editor.surround.list", params) {
            Ok(list) => {
                let found = items(&list);
                if found.is_empty() {
                    let none = if templates_only { "ide-surround-templates-none" } else { "ide-surround-none" };
                    return Some(SurroundOutcome::Notice(t!(none)));
                }
                self.shown = Some(Shown { items: found, templates_only, selected: 0, hovered: None, at, reveal: true });
                None
            }
            Err(error) => Some(SurroundOutcome::Notice(error.to_string())),
        }
    }

    pub(super) fn open_name(&self) -> Option<&'static str> {
        self.shown.as_ref().map(|shown| if shown.templates_only { "surroundWithTemplate" } else { "surround" })
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Option<SurroundOutcome> {
        let shown = self.shown.as_mut()?;
        let count = shown.items.len();
        let mut picked = None;
        super::filter_popups::take_typing(ui);
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Key { key: Key::Tab, .. } => false,
                Event::Key { key, physical_key, pressed, modifiers, .. }
                    if !modifiers.command && !modifiers.ctrl && !modifiers.alt =>
                {
                    let name = physical_key.unwrap_or(*key).name();
                    let mnemonic = name.chars().count() == 1 && name.chars().all(|c| c.is_ascii_alphanumeric());
                    if mnemonic && *pressed && picked.is_none() {
                        picked = shown.items.iter().position(|item| item.mnemonic.eq_ignore_ascii_case(name));
                    }
                    !mnemonic
                }
                _ => true,
            });
        });
        if let Some(index) = picked {
            return self.choose(index);
        }
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
        if up || down {
            shown.reveal = true;
            shown.hovered = None;
        }
        if enter {
            let selected = shown.selected;
            return self.choose(selected);
        }
        if escape {
            self.shown = None;
        }
        None
    }

    fn choose(&mut self, index: usize) -> Option<SurroundOutcome> {
        let item = self.shown.take()?.items.get(index)?.clone();
        Some(SurroundOutcome::Apply { name: item.name, template: item.template })
    }

    pub(super) fn show(
        &mut self,
        ui: &Ui,
        palette: &IdePalette,
        highlighter: Option<&Arc<dyn Highlighter>>,
    ) -> Option<SurroundOutcome> {
        let shown = self.shown.as_mut()?;
        let mut chosen = None;
        let mut configure = false;
        let mut hovered = None;
        let title = t!(if shown.templates_only { "ide-surround-templates-title" } else { "ide-surround-title" });
        let area = egui::Area::new(ui.scope_id().with("surround-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(shown.at)
            .show(ui.ctx(), |ui| {
                palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    let widest = shown
                        .items
                        .iter()
                        .map(|item| {
                            let text = format!("{}. {}  {}", item.mnemonic, item.title, item.description);
                            ui.painter().layout_no_wrap(text, theme::regular(13.0), palette.text).size().x
                        })
                        .fold(0.0_f32, f32::max);
                    ui.set_width((widest + 32.0).clamp(220.0, 520.0));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    label(ui, rect, &title, palette.text_secondary, true);
                    egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]).show(
                        ui,
                        |ui| {
                            let first_template = shown.items.iter().position(|item| item.template);
                            for (index, item) in shown.items.iter().enumerate() {
                                if !shown.templates_only && first_template == Some(index) {
                                    ui.separator();
                                    let (rect, _) =
                                        ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                                    label(ui, rect, &t!("ide-surround-templates"), palette.text_secondary, false);
                                }
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
                                if response.hovered() {
                                    hovered = Some(index);
                                }
                                let name = format!("{}. {}", item.mnemonic, item.title);
                                crate::accessibility::item(
                                    &response,
                                    egui::Role::ListBoxOption,
                                    index == shown.selected,
                                    &name,
                                );
                                row(ui, rect, item, palette);
                                if response.clicked() {
                                    chosen = Some(index);
                                }
                            }
                        },
                    );
                    ui.separator();
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 4, palette.popup_hover);
                    }
                    let text = t!("ide-surround-configure");
                    crate::accessibility::button_with(&response, || text.clone());
                    label(ui, rect, &text, palette.link, false);
                    configure = response.clicked();
                });
            });
        shown.reveal = false;
        if hovered.is_some() || ui.input(|input| input.pointer.delta() != egui::Vec2::ZERO) {
            shown.hovered = hovered;
        }
        let previewed = shown.hovered.unwrap_or(shown.selected);
        if let Some(preview) = shown.items.get(previewed).and_then(|item| item.preview.clone()) {
            egui::Area::new(ui.scope_id().with("surround-preview"))
                .order(egui::Order::Foreground)
                .fixed_pos(area.response.rect.right_top() + vec2(4.0, 0.0))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::symmetric(10, 6)).show(
                        ui,
                        |ui| {
                            let job = super::doc_popups::code_job(&preview, "", palette, highlighter, 560.0);
                            ui.add(egui::Label::new(job).extend());
                        },
                    );
                });
        }
        if let Some(index) = chosen {
            return self.choose(index);
        }
        if configure {
            self.shown = None;
            return Some(SurroundOutcome::Configure);
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

fn label(ui: &Ui, rect: Rect, text: &str, color: egui::Color32, strong: bool) {
    let font = if strong { theme::semibold(12.0) } else { theme::regular(12.0) };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    ui.painter().galley(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley, color);
}

fn row(ui: &Ui, rect: Rect, item: &Item, palette: &IdePalette) {
    let painter = ui.painter();
    let left = rect.left() + 8.0;
    let clip = Rect::from_min_max(pos2(left, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
    let painter = painter.with_clip_rect(clip);
    let mnemonic = painter.layout_no_wrap(format!("{}.", item.mnemonic), theme::regular(13.0), palette.text_secondary);
    let mnemonic_width = mnemonic.size().x.max(16.0);
    painter.galley(pos2(left, rect.center().y - mnemonic.size().y / 2.0), mnemonic, palette.text_secondary);
    let title = painter.layout_no_wrap(item.title.clone(), theme::regular(13.0), palette.text);
    let title_left = left + mnemonic_width + 6.0;
    let title_width = title.size().x;
    painter.galley(pos2(title_left, rect.center().y - title.size().y / 2.0), title, palette.text);
    if !item.description.is_empty() {
        let description =
            painter.layout_no_wrap(item.description.clone(), theme::regular(12.0), palette.text_secondary);
        let at = pos2(title_left + title_width + 12.0, rect.center().y - description.size().y / 2.0);
        painter.galley(at, description, palette.text_secondary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zen_workbench::templates::surround::mnemonic;

    fn list() -> Value {
        json!({
            "path": "/p/main.go",
            "items": [
                { "index": 0, "mnemonic": "1", "name": "if", "template": false, "title": "if", "range": [0, 1], "preview": "if  {\n\tx\n}" },
                { "index": 1, "mnemonic": "2", "name": "region", "template": false, "title": "region…endregion Comments", "range": [0, 1] },
                { "index": 2, "mnemonic": "3", "name": "defer", "template": true, "title": "defer", "description": "Run later", "range": [0, 1] }
            ]
        })
    }

    fn shown() -> SurroundPopup {
        SurroundPopup {
            shown: Some(Shown {
                items: items(&list()),
                templates_only: false,
                selected: 0,
                hovered: None,
                at: Pos2::ZERO,
                reveal: true,
            }),
        }
    }

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    fn frame(ctx: &egui::Context, events: Vec<Event>, popup: &mut SurroundPopup) -> (Option<SurroundOutcome>, usize) {
        let mut out = None;
        let mut left = 0;
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            out = popup.keys(ui);
            left = ui.input(|input| input.events.len());
            let _ = popup.show(ui, &IdePalette::zen_dark(), None);
        });
        output.textures_delta.clear();
        (out, left)
    }

    #[test]
    fn a_digit_picks_its_item_and_its_text_goes_nowhere() {
        let ctx = egui::Context::default();
        let mut popup = shown();
        assert_eq!(popup.open_name(), Some("surround"));
        let (out, left) = frame(&ctx, vec![key(Key::Num3), Event::Text("3".to_owned())], &mut popup);
        assert_eq!(out, Some(SurroundOutcome::Apply { name: "defer".to_owned(), template: true }));
        assert_eq!(left, 0, "the editor does not type the digit");
        assert_eq!(popup.open_name(), None);
    }

    #[test]
    fn a_letter_picks_by_its_place_on_the_keyboard_whatever_the_layout() {
        let ctx = egui::Context::default();
        let mut popup = shown();
        if let Some(shown) = popup.shown.as_mut() {
            let more = (3..12).map(|index| Item {
                mnemonic: mnemonic(index),
                name: format!("item{index}"),
                template: true,
                title: format!("item{index}"),
                description: String::new(),
                preview: None,
            });
            shown.items.extend(more);
        }
        let russian = Event::Key {
            key: Key::A,
            physical_key: Some(Key::A),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let (out, left) = frame(&ctx, vec![russian, Event::Text("ф".to_owned())], &mut popup);
        assert_eq!(out, Some(SurroundOutcome::Apply { name: "item10".to_owned(), template: true }));
        assert_eq!(left, 0, "the editor types neither the letter nor `ф`");
    }

    #[test]
    fn typing_pasting_and_deleting_go_nowhere_while_it_is_open() {
        let ctx = egui::Context::default();
        let mut popup = shown();
        let events = vec![
            key(Key::Backspace),
            key(Key::Delete),
            key(Key::Tab),
            Event::Paste("pasted".to_owned()),
            Event::Cut,
            Event::Ime(egui::ImeEvent::Commit("ж".to_owned())),
            Event::Text("ж".to_owned()),
        ];
        let (out, left) = frame(&ctx, events, &mut popup);
        assert_eq!((out, left), (None, 0));
        assert_eq!(popup.open_name(), Some("surround"), "it stays open");
    }

    #[test]
    fn arrows_choose_enter_applies_and_escape_closes() {
        let ctx = egui::Context::default();
        let mut popup = shown();
        assert_eq!(frame(&ctx, vec![key(Key::ArrowDown)], &mut popup).0, None);
        assert_eq!(
            frame(&ctx, vec![key(Key::Enter)], &mut popup).0,
            Some(SurroundOutcome::Apply { name: "region".to_owned(), template: false })
        );
        let mut popup = shown();
        assert_eq!(frame(&ctx, vec![key(Key::ArrowUp)], &mut popup).0, None);
        assert_eq!(popup.shown.as_ref().map(|shown| shown.selected), Some(2), "up from the first wraps to the last");
        frame(&ctx, vec![key(Key::Escape)], &mut popup);
        assert_eq!(popup.open_name(), None);
        let mut popup = shown();
        assert_eq!(frame(&ctx, vec![key(Key::Z)], &mut popup).0, None, "a key no item has picks nothing");
        assert!(popup.open_name().is_some());
    }

    #[test]
    fn the_items_keep_what_the_command_gave() {
        let found = items(&list());
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].preview.as_deref(), Some("if  {\n\tx\n}"));
        assert_eq!((found[2].template, found[2].description.as_str()), (true, "Run later"));
    }
}
