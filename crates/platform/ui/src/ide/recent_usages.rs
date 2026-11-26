use egui::{Align2, Event, Key, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::{IdePalette, IdeView, dialog_frame};
use crate::{accessibility, theme};

const ROW: f32 = 26.0;
const ROWS: usize = 12;

#[derive(Clone, Debug, PartialEq)]
struct Search {
    title: String,
    place: String,
}

#[derive(Debug, Default)]
pub(super) struct RecentUsages {
    open: bool,
    searches: Vec<Search>,
    selected: usize,
}

impl RecentUsages {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn open(&mut self, recent: &Value) {
        let searches = recent["searches"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|search| Search {
                title: search["title"].as_str().unwrap_or_default().to_owned(),
                place: t!(
                    "ide-recent-usages-place",
                    file = search["relative"].as_str().unwrap_or_default(),
                    scope = search["scopeName"].as_str().unwrap_or_default()
                ),
            })
            .collect();
        *self = Self { open: true, searches, selected: 0 };
    }

    fn show(&mut self, ui: &mut Ui, palette: &IdePalette) -> Option<usize> {
        if !self.open {
            return None;
        }
        let count = self.searches.len();
        let (mut chosen, mut close) = (None, false);
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Key { key, pressed: true, .. } => {
                    match key {
                        Key::ArrowDown => self.selected = (self.selected + 1).min(count.saturating_sub(1)),
                        Key::ArrowUp => self.selected = self.selected.saturating_sub(1),
                        Key::Enter if count > 0 => chosen = Some(self.selected),
                        Key::Escape => close = true,
                        _ => {}
                    }
                    false
                }
                Event::Key { .. } | Event::Text(_) => false,
                _ => true,
            });
        });
        let screen = ui.ctx().content_rect();
        let width = 520.0;
        let area = egui::Area::new(ui.scope_id().with("recent-find-usages"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.center().x - width / 2.0, screen.top() + screen.height() * 0.18));
        let response = area.show(ui.ctx(), |ui| {
            dialog_frame(palette).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    egui::RichText::new(t!("ide-recent-usages-title")).font(theme::semibold(13.0)).color(palette.text),
                );
                ui.add_space(8.0);
                if self.searches.is_empty() {
                    let empty = t!("ide-recent-usages-empty");
                    ui.label(egui::RichText::new(empty).font(theme::regular(13.0)).color(palette.text_secondary));
                    return;
                }
                super::filter_popups::list(ui, "recent-find-usages-rows", ROW * ROWS as f32, |ui| {
                    for (index, search) in self.searches.iter().enumerate() {
                        let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        let selected = index == self.selected;
                        accessibility::item(&row, egui::Role::ListBoxOption, selected, &search.title);
                        if selected {
                            ui.painter().rect_filled(rect, 5, palette.selection);
                        } else if row.hovered() {
                            ui.painter().rect_filled(rect, 5, palette.popup_hover);
                        }
                        let title = ui.painter().text(
                            pos2(rect.left() + 8.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            &search.title,
                            theme::regular(13.0),
                            palette.text,
                        );
                        ui.painter().with_clip_rect(rect).text(
                            pos2(title.right() + 10.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            &search.place,
                            theme::regular(12.5),
                            palette.text_secondary,
                        );
                        if row.clicked() {
                            chosen = Some(index);
                        }
                    }
                });
            });
        });
        if ui.input(|input| input.pointer.any_pressed())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !response.response.rect.contains(pointer)
        {
            close = true;
        }
        if close || chosen.is_some() {
            self.open = false;
        }
        chosen
    }
}

impl IdeView {
    pub(super) fn recent_usages_popup(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        let Some(index) = self.recent_usages.show(ui, &palette) else { return };
        if let Err(error) = registry.dispatch(session, "navigate.usages.openRecent", json!({ "index": index })) {
            self.report(ui, error);
        }
    }
}
