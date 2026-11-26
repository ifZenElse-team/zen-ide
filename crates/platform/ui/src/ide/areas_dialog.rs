use std::collections::BTreeSet;

use egui::{Key, Ui};
use zen_i18n::t;
use zen_modules::{AreaKind, AreaList};

use super::IdePalette;
use super::vcs::{primary_button, secondary_button};
use crate::theme;

const WIDTH: f32 = 440.0;

pub(super) struct AreasDialog {
    list: AreaList,
    checked: BTreeSet<String>,
    pub(super) generation: u64,
    pub(super) error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Choose(Vec<String>),
    Skip,
}

impl AreasDialog {
    pub(super) fn new(list: AreaList, generation: u64) -> Self {
        let checked = list.areas.iter().filter(|area| area.enabled).map(|area| area.id.clone()).collect();
        Self { list, checked, generation, error: None }
    }

    pub(super) fn chosen(&self) -> Vec<String> {
        self.list.areas.iter().filter(|area| self.checked.contains(&area.id)).map(|area| area.id.clone()).collect()
    }

    pub(super) fn restarts(&self) -> Vec<&str> {
        self.list
            .areas
            .iter()
            .filter(|area| area.kind == AreaKind::Module && area.enabled != self.checked.contains(&area.id))
            .map(|area| area.name.as_str())
            .collect()
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Option<Outcome> {
        let mut outcome = None;
        let product = self.list.product.clone();
        let modal =
            egui::Modal::new(ui.scope_id().with("plugins-areas")).frame(super::dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing.y = 0.0;
                let title = t!("areas-title");
                ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t!("areas-intro", product = product.as_str()))
                            .font(theme::regular(13.0))
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(14.0);
                let indent = ui.spacing().icon_width + ui.spacing().icon_spacing;
                for area in &self.list.areas {
                    let mut on = self.checked.contains(&area.id);
                    let name = egui::RichText::new(&area.name).font(theme::semibold(13.0)).color(p.text);
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            self.checked.insert(area.id.clone());
                        } else {
                            self.checked.remove(&area.id);
                        }
                    }
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(indent);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&area.description)
                                    .font(theme::regular(12.0))
                                    .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    });
                    ui.add_space(10.0);
                }
                ui.add_space(2.0);
                let restarts = self.restarts();
                let note = if restarts.is_empty() {
                    t!("areas-later")
                } else {
                    t!("areas-restart", product = product.as_str(), names = restarts.join(", "))
                };
                ui.add(
                    egui::Label::new(egui::RichText::new(note).font(theme::regular(12.0)).color(p.text_secondary))
                        .wrap(),
                );
                if let Some(error) = &self.error {
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(error).font(theme::regular(12.0)).color(p.error_text))
                            .wrap(),
                    );
                }
                ui.add_space(18.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if primary_button(ui, p, &t!("areas-continue")).clicked() {
                        outcome = Some(Outcome::Choose(self.chosen()));
                    }
                    if secondary_button(ui, p, &t!("areas-skip")).clicked() {
                        outcome = Some(Outcome::Skip);
                    }
                });
                if ui.input(|input| input.key_pressed(Key::Enter)) {
                    outcome = Some(Outcome::Choose(self.chosen()));
                }
            });
        if outcome.is_none() && modal.should_close() {
            outcome = Some(Outcome::Skip);
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use zen_modules::AreaEntry;

    use super::*;

    fn area(id: &str, kind: AreaKind, enabled: bool) -> AreaEntry {
        AreaEntry {
            id: id.to_owned(),
            kind,
            name: id.to_uppercase(),
            description: format!("What {id} gives."),
            enabled,
            restart_required: false,
        }
    }

    fn list() -> AreaList {
        AreaList {
            product: "TestTotami".to_owned(),
            offered: false,
            dialog_open: false,
            areas: vec![
                area("e2e-tests", AreaKind::Plugin, false),
                area("load-tests", AreaKind::Module, true),
                area("api-tests", AreaKind::Plugin, true),
            ],
            restart_required: Vec::new(),
        }
    }

    #[test]
    fn the_dialog_starts_from_what_is_on_and_names_the_restarts() {
        let mut dialog = AreasDialog::new(list(), 3);
        assert_eq!(dialog.chosen(), ["load-tests", "api-tests"], "in the product's order");
        assert!(dialog.restarts().is_empty());
        dialog.checked.remove("load-tests");
        dialog.checked.insert("e2e-tests".to_owned());
        assert_eq!(dialog.chosen(), ["e2e-tests", "api-tests"]);
        assert_eq!(dialog.restarts(), ["LOAD-TESTS"], "only a built-in module needs a restart");
    }

    #[test]
    fn the_dialog_draws_and_enter_continues() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        let p = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let mut dialog = AreasDialog::new(list(), 0);
        let mut run = |events: Vec<egui::Event>| {
            let mut outcome = None;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| outcome = dialog.show(ui, &p));
            output.textures_delta.clear();
            outcome
        };
        assert_eq!(run(Vec::new()), None);
        let enter = egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(run(vec![enter]), Some(Outcome::Choose(vec!["load-tests".to_owned(), "api-tests".to_owned()])));
        let escape = egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(run(vec![escape]), Some(Outcome::Skip), "closing the dialog skips");
    }
}
