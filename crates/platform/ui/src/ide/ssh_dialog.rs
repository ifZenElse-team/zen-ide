use std::time::Duration;

use egui::{Align, Key, Layout, RichText, Role, Ui};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::database::connections::SshPrompts;

use super::{IdeView, dialog_frame, vcs};
use crate::{accessibility, theme};

#[derive(Default)]
pub(super) struct SshDialog {
    shown: Option<u64>,
    value: String,
    save: bool,
}

impl SshDialog {
    pub(super) fn is_open(&self) -> bool {
        self.shown.is_some()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Send,
    Yes,
    No,
    Cancel,
}

impl IdeView {
    pub(super) fn ssh_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if session.state::<SshPrompts>().is_some_and(|prompts| !prompts.0.is_empty()) {
            let live = registry.dispatch(session, "db.ssh.prompts", json!({ "limit": 200 })).unwrap_or_default();
            let ids: Vec<u64> =
                live["prompts"].as_array().into_iter().flatten().filter_map(|prompt| prompt["id"].as_u64()).collect();
            session.state_mut::<SshPrompts>().0.retain(|prompt| ids.contains(&prompt.id));
        }
        let Some(prompt) = session.state::<SshPrompts>().and_then(|prompts| prompts.0.first().cloned()) else {
            self.ssh_dialog = SshDialog::default();
            return;
        };
        if self.ssh_dialog.shown != Some(prompt.id) {
            self.ssh_dialog = SshDialog { shown: Some(prompt.id), value: String::new(), save: prompt.secret };
        }
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        let p = self.palette;
        let dialog = &mut self.ssh_dialog;
        let mut choice = None;
        let modal = egui::Modal::new(ui.scope_id().with("ssh-prompt")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let title = t!("ide-ssh-title", name = prompt.name.as_str());
            ui.label(RichText::new(title).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(12.0);
            ui.add(
                egui::Label::new(
                    RichText::new(prompt.prompt.as_str()).font(egui::FontId::monospace(12.5)).color(p.text),
                )
                .wrap(),
            );
            if prompt.question {
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(t!("ide-ssh-host-key")).font(theme::regular(12.0)).color(p.text_secondary),
                    )
                    .wrap(),
                );
            } else {
                ui.add_space(10.0);
                let field =
                    egui::TextEdit::singleline(&mut dialog.value).password(prompt.secret).desired_width(f32::INFINITY);
                let response = ui.add(field);
                let role = if prompt.secret { Role::PasswordInput } else { Role::TextInput };
                accessibility::name(&response, role, &t!("ide-ssh-answer"));
                response.request_focus();
                if prompt.secret {
                    ui.add_space(8.0);
                    ui.checkbox(&mut dialog.save, t!("ide-ssh-save"));
                }
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                    choice = Some(Choice::Send);
                }
            }
            ui.add_space(20.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if prompt.question {
                    if vcs::secondary_button(ui, &p, &t!("ide-ssh-no")).clicked() {
                        choice = Some(Choice::No);
                    }
                    if vcs::secondary_button(ui, &p, &t!("ide-ssh-yes")).clicked() {
                        choice = Some(Choice::Yes);
                    }
                } else {
                    if vcs::primary_button(ui, &p, &t!("ide-ssh-ok")).clicked() {
                        choice = Some(Choice::Send);
                    }
                    if vcs::secondary_button(ui, &p, &t!("settings-cancel")).clicked() {
                        choice = Some(Choice::Cancel);
                    }
                }
            });
        });
        let choice = choice.or_else(|| modal.should_close().then_some(Choice::Cancel));
        let Some(choice) = choice else { return };
        let value = match choice {
            Choice::Send => Some(std::mem::take(&mut dialog.value)),
            Choice::Yes => Some("yes".to_owned()),
            Choice::No => Some("no".to_owned()),
            Choice::Cancel => None,
        };
        let save = dialog.save && prompt.secret;
        dialog.value.clear();
        let params = json!({ "prompt": prompt.id, "value": value, "save": save });
        if let Err(error) = registry.dispatch(session, "db.ssh.answer", params) {
            self.report_at(ui.ctx(), error);
        }
        self.ssh_dialog = SshDialog::default();
    }
}
