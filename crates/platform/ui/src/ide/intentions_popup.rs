use std::path::PathBuf;

use egui::{Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::Workbench;

use super::IdePalette;
use super::caret_popup::{CaretPlace, PopupAnchor};
use crate::theme;

const ROW: f32 = 24.0;
const VISIBLE: usize = 14;

#[derive(Clone, Debug, PartialEq)]
struct Action {
    id: String,
    title: String,
    quick_fix: bool,
    disabled: Option<String>,
}

#[derive(Clone, Debug)]
struct Shown {
    actions: Vec<Action>,
    selected: usize,
    at: PopupAnchor,
    reveal: bool,
}

type Place = (PathBuf, u64, u64);

#[derive(Clone, Debug, PartialEq)]
enum Bulb {
    Unasked,
    Asking,
    Answered(Option<bool>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum IntentionsOutcome {
    Apply(String),
    Notice(String),
}

#[derive(Default)]
pub(super) struct IntentionsPopup {
    waiting: Option<PopupAnchor>,
    shown: Option<Shown>,
    bulb: Option<(Place, Bulb)>,
}

fn actions(list: &Value) -> Vec<Action> {
    list["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|action| {
            Some(Action {
                id: action["id"].as_str()?.to_owned(),
                title: action["title"].as_str()?.to_owned(),
                quick_fix: action["quickFix"] == true,
                disabled: action["disabled"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

fn place(session: &Session) -> Option<Place> {
    let document = session.state::<Workbench>()?.active_document()?;
    (document.image().is_none()).then(|| (document.path().to_owned(), document.version(), document.selection_version()))
}

impl IntentionsPopup {
    pub(super) fn open(&mut self, at: PopupAnchor) {
        self.shown = None;
        self.waiting = Some(at);
    }

    pub(super) fn is_open(&self) -> bool {
        self.shown.is_some() || self.waiting.is_some()
    }

    pub(super) fn bulb(&self, session: &Session) -> Option<bool> {
        match &self.bulb {
            Some((at, Bulb::Answered(found))) if place(session).as_ref() == Some(at) => *found,
            _ => None,
        }
    }

    pub(super) fn follow(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        settled: bool,
    ) -> Option<IntentionsOutcome> {
        let current = place(session);
        if self.shown.is_some() && self.bulb.as_ref().map(|(at, _)| at) != current.as_ref() {
            self.shown = None;
        }
        if current.as_ref() != self.bulb.as_ref().map(|(at, _)| at) {
            self.bulb = current.clone().map(|at| (at, Bulb::Unasked));
        }
        let mut outcome = None;
        if let Some(at) = self.waiting {
            match registry.dispatch(session, "editor.intentions.list", json!({})) {
                Ok(list) if list["pending"] == true => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50))
                }
                Ok(list) => {
                    self.waiting = None;
                    let found = actions(&list);
                    self.answered(&found);
                    if !found.is_empty() {
                        self.shown = Some(Shown { actions: found, selected: 0, at, reveal: true });
                    } else if let Some(error) = list["error"].as_str() {
                        outcome = Some(IntentionsOutcome::Notice(error.to_owned()));
                    } else {
                        outcome = Some(IntentionsOutcome::Notice(zen_i18n::t!("ide-intentions-none")));
                    }
                }
                Err(error) => {
                    self.waiting = None;
                    self.answered(&[]);
                    outcome = Some(IntentionsOutcome::Notice(error.to_string()));
                }
            }
        }
        let asking = match &self.bulb {
            Some((_, Bulb::Unasked)) => settled,
            Some((_, Bulb::Asking)) => true,
            _ => false,
        };
        if asking && self.waiting.is_none() {
            match registry.dispatch(session, "editor.intentions.list", json!({})) {
                Ok(list) if list["pending"] == true => {
                    if let Some((_, bulb)) = &mut self.bulb {
                        *bulb = Bulb::Asking;
                    }
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                }
                Ok(list) => self.answered(&actions(&list)),
                Err(_) => self.answered(&[]),
            }
        }
        outcome
    }

    fn answered(&mut self, found: &[Action]) {
        if let Some((_, bulb)) = &mut self.bulb {
            let usable: Vec<&Action> = found.iter().filter(|action| action.disabled.is_none()).collect();
            *bulb = Bulb::Answered((!usable.is_empty()).then(|| usable.iter().any(|action| action.quick_fix)));
        }
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Option<IntentionsOutcome> {
        if self.waiting.is_some() && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.waiting = None;
        }
        let shown = self.shown.as_mut()?;
        let count = shown.actions.len();
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
            return self.choose(self.shown.as_ref().map_or(0, |shown| shown.selected));
        }
        if escape {
            self.shown = None;
        }
        None
    }

    fn choose(&mut self, index: usize) -> Option<IntentionsOutcome> {
        let action = self.shown.as_ref()?.actions.get(index)?.clone();
        self.shown = None;
        Some(match action.disabled {
            Some(reason) => IntentionsOutcome::Notice(reason),
            None => IntentionsOutcome::Apply(action.id),
        })
    }

    pub(super) fn show(&mut self, ui: &Ui, palette: &IdePalette, caret: CaretPlace) -> Option<IntentionsOutcome> {
        let shown = self.shown.as_mut()?;
        let area = egui::Area::new(ui.scope_id().with("intentions-popup")).order(egui::Order::Foreground);
        let area = shown.at.area(ui.ctx(), area, caret)?;
        let mut chosen = None;
        let area = area.show(ui.ctx(), |ui| {
            palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                let widest = shown
                    .actions
                    .iter()
                    .map(|action| {
                        ui.painter().layout_no_wrap(action.title.clone(), theme::regular(13.0), palette.text).size().x
                    })
                    .fold(0.0_f32, f32::max);
                ui.set_width((widest + 48.0).clamp(220.0, 560.0));
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]).show(
                    ui,
                    |ui| {
                        for (index, action) in shown.actions.iter().enumerate() {
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
                            crate::accessibility::item(
                                &response,
                                egui::Role::ListBoxOption,
                                index == shown.selected,
                                &action.title,
                            );
                            row(ui, rect, action, palette);
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

pub(super) fn bulb_color(palette: &IdePalette, quick_fix: bool) -> egui::Color32 {
    if quick_fix { palette.problem("error") } else { palette.problem("warning") }
}

fn row(ui: &Ui, rect: Rect, action: &Action, palette: &IdePalette) {
    let painter = ui.painter();
    let icon = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), Vec2::splat(14.0));
    let color = if action.disabled.is_some() { palette.text_secondary } else { palette.text };
    super::glyphs::glyph_bulb(painter, icon, bulb_color(palette, action.quick_fix));
    let text = painter.layout_no_wrap(action.title.clone(), theme::regular(13.0), color);
    let left = rect.left() + 28.0;
    let clip = Rect::from_min_max(pos2(left, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
    painter.with_clip_rect(clip).galley(pos2(left, rect.center().y - text.size().y / 2.0), text, color);
}

#[cfg(test)]
mod tests {
    use egui::Pos2;

    use super::*;

    fn list() -> Value {
        json!({
            "pending": false,
            "actions": [
                { "id": "1.0", "title": "Import fmt", "kind": "quickfix", "quickFix": true, "preferred": true, "disabled": null },
                { "id": "1.1", "title": "Inline", "kind": "refactor.inline", "quickFix": false, "preferred": false, "disabled": "nothing to inline" }
            ],
            "error": null
        })
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    #[test]
    fn arrows_choose_enter_applies_and_a_disabled_action_says_why() {
        let ctx = egui::Context::default();
        let mut popup = IntentionsPopup {
            shown: Some(Shown {
                actions: actions(&list()),
                selected: 0,
                at: PopupAnchor::At(Pos2::ZERO),
                reveal: true,
            }),
            ..IntentionsPopup::default()
        };
        let frame = |events: Vec<egui::Event>, popup: &mut IntentionsPopup| {
            let mut out = None;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                out = popup.keys(ui);
                let _ = popup.show(ui, &IdePalette::zen_dark(), CaretPlace::default());
            });
            output.textures_delta.clear();
            out
        };
        assert_eq!(frame(vec![key(Key::ArrowDown)], &mut popup), None);
        assert_eq!(
            frame(vec![key(Key::Enter)], &mut popup),
            Some(IntentionsOutcome::Notice("nothing to inline".to_owned()))
        );
        assert!(!popup.is_open());
        popup.shown =
            Some(Shown { actions: actions(&list()), selected: 0, at: PopupAnchor::At(Pos2::ZERO), reveal: true });
        assert_eq!(frame(vec![key(Key::Enter)], &mut popup), Some(IntentionsOutcome::Apply("1.0".to_owned())));
        popup.shown =
            Some(Shown { actions: actions(&list()), selected: 0, at: PopupAnchor::At(Pos2::ZERO), reveal: true });
        frame(vec![key(Key::Escape)], &mut popup);
        assert!(!popup.is_open());
    }

    #[test]
    fn the_bulb_shows_usable_actions_and_is_red_for_a_quick_fix() {
        let mut popup = IntentionsPopup { bulb: Some(((PathBuf::new(), 0, 0), Bulb::Asking)), ..Default::default() };
        popup.answered(&actions(&list()));
        assert_eq!(popup.bulb, Some(((PathBuf::new(), 0, 0), Bulb::Answered(Some(true)))));
        let only_disabled: Vec<Action> = actions(&list()).into_iter().skip(1).collect();
        popup.answered(&only_disabled);
        assert_eq!(popup.bulb, Some(((PathBuf::new(), 0, 0), Bulb::Answered(None))));
        let palette = IdePalette::zen_dark();
        assert_ne!(bulb_color(&palette, true), bulb_color(&palette, false));
    }
}
