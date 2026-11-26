use egui::{Color32, Key, Stroke, Ui, vec2};
use zen_i18n::t;
use zen_workbench::maintenance::InvalidateParams;

use super::{IdePalette, IdeView, Request, RestartAsk, dialog_frame, form};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    InvalidateAndRestart,
    JustRestart,
    Cancel,
}

#[derive(Debug, Default)]
pub(super) struct InvalidateDialog {
    pub(super) chosen: InvalidateParams,
}

fn button(ui: &mut Ui, p: &IdePalette, label: String, primary: bool) -> bool {
    let color = if primary { p.accent_text } else { p.text };
    let text = egui::RichText::new(label).font(theme::regular(13.0)).color(color);
    let button = egui::Button::new(text).min_size(vec2(84.0, 30.0)).corner_radius(6);
    let button = if primary {
        button.fill(p.accent)
    } else {
        button.fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, p.button_border))
    };
    ui.add(button).clicked()
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut InvalidateDialog) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("invalidate-caches")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(460.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        let title = egui::RichText::new(t!("ide-invalidate-title")).font(theme::semibold(15.0)).color(p.text);
        ui.add(egui::Label::new(title));
        ui.add_space(12.0);
        let text = egui::RichText::new(t!("ide-invalidate-text")).font(theme::regular(13.0)).color(p.text);
        ui.add(egui::Label::new(text).wrap());
        ui.add_space(12.0);
        form::check(ui, p, &mut dialog.chosen.clear_local_history, &t!("ide-invalidate-local-history"));
        ui.add_space(4.0);
        form::check(ui, p, &mut dialog.chosen.clear_vcs_log, &t!("ide-invalidate-vcs-log"));
        ui.add_space(20.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if button(ui, p, t!("ide-invalidate-and-restart"), true) {
                choice = Some(Choice::InvalidateAndRestart);
            }
            if button(ui, p, t!("ide-invalidate-just-restart"), false) {
                choice = Some(Choice::JustRestart);
            }
            if button(ui, p, t!("settings-cancel"), false) {
                choice = Some(Choice::Cancel);
            }
        });
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            choice = Some(Choice::InvalidateAndRestart);
        }
    });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

impl IdeView {
    pub(super) fn open_invalidate_dialog(&mut self) {
        self.invalidate_dialog.get_or_insert_with(InvalidateDialog::default);
    }

    pub(super) fn invalidate_dialog(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let Some(dialog) = &mut self.invalidate_dialog else { return };
        let Some(choice) = show(ui, &p, dialog) else { return };
        let chosen = dialog.chosen;
        self.invalidate_dialog = None;
        let invalidate = match choice {
            Choice::InvalidateAndRestart => Some(chosen),
            Choice::JustRestart => None,
            Choice::Cancel => return,
        };
        self.requests.push(Request::RestartAsking(RestartAsk::IfWorkRuns, invalidate));
    }
}

#[cfg(test)]
pub(super) fn default_params() -> InvalidateParams {
    InvalidateDialog::default().chosen
}
