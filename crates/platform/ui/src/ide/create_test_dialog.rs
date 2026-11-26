use std::path::{Path, PathBuf};

use egui::{Color32, Key, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::{IdePalette, IdeView, dialog_frame, form};
use crate::theme;

const LABEL: f32 = 170.0;

#[derive(Debug)]
pub(super) struct CreateTestDialog {
    pub(super) subject: PathBuf,
    pub(super) frameworks: Vec<(String, String)>,
    pub(super) framework: usize,
    pub(super) name: String,
    pub(super) destinations: Vec<String>,
    pub(super) destination: String,
    pub(super) members: Vec<(String, bool)>,
    pub(super) error: Option<String>,
}

fn shown_path(path: &str, root: Option<&Path>) -> String {
    let path = Path::new(path);
    root.and_then(|root| path.strip_prefix(root).ok())
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

impl CreateTestDialog {
    pub(super) fn from_options(options: &Value, root: Option<&Path>) -> Self {
        let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        let list = |key: &str| options[key].as_array().cloned().unwrap_or_default();
        let destinations: Vec<String> =
            list("destinations").iter().map(|destination| shown_path(&text(destination), root)).collect();
        Self {
            subject: PathBuf::from(text(&options["subject"])),
            frameworks: list("frameworks")
                .iter()
                .map(|framework| (text(&framework["id"]), text(&framework["name"])))
                .collect(),
            framework: 0,
            name: text(&options["name"]),
            destination: destinations.first().cloned().unwrap_or_default(),
            destinations,
            members: list("members")
                .iter()
                .map(|member| (text(&member["label"]), member["selected"].as_bool().unwrap_or(false)))
                .collect(),
            error: None,
        }
    }

    pub(super) fn params(&self) -> Value {
        let members: Vec<&str> =
            self.members.iter().filter(|(_, checked)| *checked).map(|(label, _)| label.as_str()).collect();
        json!({
            "path": self.subject,
            "framework": self.frameworks.get(self.framework).map(|(id, _)| id.as_str()),
            "name": self.name.trim(),
            "destination": self.destination.trim(),
            "members": members,
        })
    }

    fn ready(&self) -> bool {
        !self.name.trim().is_empty() && !self.destination.trim().is_empty() && !self.frameworks.is_empty()
    }
}

enum Choice {
    Ok,
    Cancel,
}

impl IdeView {
    pub(super) fn open_create_test(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
    ) -> Result<Value, DispatchError> {
        let options = registry.dispatch(session, "navigate.test.options", params.clone())?;
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        self.create_test = Some(CreateTestDialog::from_options(&options, root.as_deref()));
        Ok(options)
    }

    pub(super) fn create_test_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(dialog) = &mut self.create_test else { return };
        match show(ui, &p, dialog) {
            Some(Choice::Cancel) => self.create_test = None,
            Some(Choice::Ok) => match registry.dispatch(session, "navigate.test.create", dialog.params()) {
                Ok(_) => {
                    self.create_test = None;
                    self.editor_view.focus();
                }
                Err(DispatchError::Failed { error, .. }) => dialog.error = Some(error.message),
                Err(other) => dialog.error = Some(other.to_string()),
            },
            None => {}
        }
    }
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut CreateTestDialog) -> Option<Choice> {
    let mut choice = None;
    let text = |text: String, color: Color32| egui::RichText::new(text).font(theme::regular(13.0)).color(color);
    let modal = egui::Modal::new(ui.scope_id().with("create-test")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(egui::RichText::new(t!("ide-create-test-title")).font(theme::semibold(15.0)).color(p.text));
        ui.horizontal(|ui| {
            let name = t!("ide-create-test-framework");
            form::label(ui, p, &name, LABEL);
            let shown = dialog.frameworks.get(dialog.framework).map(|(_, name)| name.clone()).unwrap_or_default();
            form::combo(ui, p, "create-test-framework", &name, &shown, 320.0, |ui| {
                for (index, (_, framework)) in dialog.frameworks.iter().enumerate() {
                    ui.selectable_value(&mut dialog.framework, index, text(framework.clone(), p.text));
                }
            });
        });
        ui.horizontal(|ui| {
            form::label(ui, p, &t!("ide-create-test-name"), LABEL);
            form::framed(ui, p, |ui| {
                ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(320.0).id_salt("create-test-name"))
            });
        });
        ui.horizontal(|ui| {
            let name = t!("ide-create-test-destination");
            form::label(ui, p, &name, LABEL);
            form::framed(ui, p, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.destination)
                        .desired_width(286.0)
                        .id_salt("create-test-destination"),
                )
            });
            if dialog.destinations.len() > 1 {
                form::combo(ui, p, "create-test-destinations", &name, "", 10.0, |ui| {
                    for destination in &dialog.destinations {
                        ui.selectable_value(
                            &mut dialog.destination,
                            destination.clone(),
                            text(destination.clone(), p.text),
                        );
                    }
                });
            }
        });
        ui.label(text(t!("ide-create-test-members"), p.text));
        egui::Frame::new().stroke(Stroke::new(1.0, p.button_border)).corner_radius(4).inner_margin(6).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if dialog.members.is_empty() {
                    ui.label(text(t!("ide-create-test-no-members"), p.text_secondary));
                }
                for (label, checked) in &mut dialog.members {
                    form::check(ui, p, checked, label);
                }
            });
        });
        if let Some(error) = &dialog.error {
            ui.add(egui::Label::new(text(error.clone(), p.error_text)).wrap());
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let ready = dialog.ready();
            let ok = egui::Button::new(text(t!("ide-create-test-ok"), p.accent_text))
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(if ready { p.accent } else { p.accent.gamma_multiply(0.4) });
            if ui.add_enabled(ready, ok).clicked() {
                choice = Some(Choice::Ok);
            }
            let cancel = egui::Button::new(text(t!("settings-cancel"), p.text))
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
            if ui.add(cancel).clicked() {
                choice = Some(Choice::Cancel);
            }
            if ready && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                choice = Some(Choice::Ok);
            }
        });
    });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_starts_from_the_options_and_ok_names_what_was_chosen() {
        let options = json!({
            "subject": "/p/src/cart.rs",
            "frameworks": [{ "id": "rust", "name": "Built-in" }, { "id": "tokio", "name": "Tokio" }],
            "name": "cart",
            "destinations": ["/p/src", "/p/tests"],
            "members": [{ "label": "Cart.total", "selected": false }, { "label": "price", "selected": true }]
        });
        let mut dialog = CreateTestDialog::from_options(&options, Some(Path::new("/p")));
        assert_eq!((dialog.destination.as_str(), dialog.destinations.len()), ("src", 2));
        assert_eq!(
            dialog.params(),
            json!({ "path": "/p/src/cart.rs", "framework": "rust", "name": "cart", "destination": "src", "members": ["price"] })
        );
        dialog.framework = 1;
        dialog.destination = "tests".to_owned();
        dialog.members[0].1 = true;
        let params = dialog.params();
        assert_eq!((&params["framework"], &params["members"]), (&json!("tokio"), &json!(["Cart.total", "price"])));
        dialog.name = " ".to_owned();
        assert!(!dialog.ready());
    }
}
