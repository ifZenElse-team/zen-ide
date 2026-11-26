use egui::{Color32, Key, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::language::usages_settings::{UsagesSettingsState, dialog_generation};

use super::{IdePalette, IdeView, dialog_frame};
use crate::{accessibility, theme};

#[derive(Debug, Default)]
pub(super) struct UsagesSettings {
    state: Option<UsagesSettingsState>,
    seen: Option<(bool, u64, String)>,
}

#[derive(Clone, Debug, PartialEq)]
enum Choice {
    Set(Value),
    Find,
    Cancel,
}

impl UsagesSettings {
    pub(super) fn is_open(&self) -> bool {
        self.state.as_ref().is_some_and(|state| state.open)
    }

    fn read(&mut self, registry: &Registry, session: &mut Session) {
        let now = dialog_generation(session).map(|(open, generation)| (open, generation, zen_i18n::language()));
        if now == self.seen {
            return;
        }
        self.seen = now;
        self.state = registry
            .dispatch(session, "navigate.usagesSettings.get", json!({}))
            .ok()
            .and_then(|value| serde_json::from_value(value).ok());
    }
}

fn show(ui: &mut Ui, p: &IdePalette, state: &UsagesSettingsState) -> Option<Choice> {
    let mut choice = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("find-usages-settings")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let target = state.target_name.clone().unwrap_or_default();
            let title = t!("ide-usages-settings-title", target = target);
            ui.add(egui::Label::new(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text)).wrap());
            ui.add_space(14.0);
            ui.label(egui::RichText::new(t!("ide-usages-settings-options")).font(theme::semibold(13.0)).color(p.text));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(egui::RichText::new(t!("ide-usages-settings-scope")).font(theme::regular(13.0)).color(p.text));
                let current = state
                    .scopes
                    .iter()
                    .find(|scope| scope.scope == state.options.scope)
                    .map(|scope| scope.name.clone())
                    .unwrap_or_default();
                let combo = ui.add(
                    egui::Button::new(
                        egui::RichText::new(format!("{current}  ⌄")).font(theme::regular(13.0)).color(p.text),
                    )
                    .min_size(vec2(200.0, 26.0))
                    .corner_radius(6)
                    .stroke(Stroke::new(1.0, p.button_border)),
                );
                accessibility::name_with(&combo, egui::Role::ComboBox, || current.clone());
                egui::Popup::menu(&combo).show(|ui| {
                    ui.set_min_width(200.0);
                    for scope in &state.scopes {
                        if ui.selectable_label(scope.scope == state.options.scope, &scope.name).clicked() {
                            choice = Some(Choice::Set(json!({ "scope": scope.scope })));
                        }
                    }
                });
            });
            ui.add_space(10.0);
            let mut new_tab = state.options.new_tab;
            if ui.checkbox(&mut new_tab, t!("ide-usages-settings-new-tab")).changed() {
                choice = Some(Choice::Set(json!({ "newTab": new_tab })));
            }
            ui.add_space(4.0);
            let mut skip = state.options.skip_single;
            if ui.checkbox(&mut skip, t!("ide-usages-settings-skip-single")).changed() {
                choice = Some(Choice::Set(json!({ "skipSingle": skip })));
            }
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let find = egui::Button::new(
                    egui::RichText::new(t!("ide-usages-settings-find")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add(find).clicked() {
                    choice = Some(Choice::Find);
                }
                let cancel = egui::Button::new(
                    egui::RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(cancel).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
            if !egui::Popup::is_any_open(ui.ctx())
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter))
            {
                choice = Some(Choice::Find);
            }
        });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

impl IdeView {
    pub(super) fn usages_settings_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.usages_settings.read(registry, session);
        let Some(state) = self.usages_settings.state.clone().filter(|state| state.open) else { return };
        let palette = self.palette;
        let Some(choice) = show(ui, &palette, &state) else { return };
        let (command, params) = match choice {
            Choice::Set(params) => ("navigate.usagesSettings.set", params),
            Choice::Find => ("navigate.usagesSettings.find", json!({})),
            Choice::Cancel => ("navigate.usagesSettings.cancel", json!({})),
        };
        if let Err(error) = registry.dispatch(session, command, params) {
            self.report(ui, error);
        }
        if command != "navigate.usagesSettings.set" {
            self.editor_view.focus();
        }
    }
}
