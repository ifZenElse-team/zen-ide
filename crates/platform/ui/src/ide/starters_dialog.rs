use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{Align, Color32, Layout, RichText, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_project::generators::{Field, FieldKind, Values};
use zen_workbench::spring_boot::starters::StartersState;

use super::settings::Dispatch;
use super::{IdePalette, dialog_frame, new_project_dialog};
use crate::theme;

pub(super) const OPEN: &str = "view.startersDialog.open";
pub(super) const SET: &str = "view.startersDialog.set";
pub(super) const APPLY: &str = "view.startersDialog.apply";
pub(super) const CLOSE: &str = "view.startersDialog.close";

const WIDTH: f32 = 620.0;
const HEIGHT: f32 = 520.0;
const POLL: std::time::Duration = std::time::Duration::from_millis(100);
const GENERATOR: &str = "spring-boot.initializr";

#[derive(Debug, Default)]
pub(super) struct StartersDialog {
    module: Option<String>,
    file: Option<PathBuf>,
    loaded: Option<StartersState>,
    ticked: BTreeSet<String>,
    search: String,
    error: Option<String>,
    retry: bool,
    address: Option<Option<String>>,
    connected: Option<String>,
    connect_pressed: bool,
    pub(super) by_agent: bool,
}

impl StartersDialog {
    pub(super) fn new(module: Option<String>, file: Option<PathBuf>) -> Self {
        Self { module, file, ..Self::default() }
    }

    fn target(&self) -> Value {
        json!({ "module": self.module, "file": self.file })
    }

    fn find_address(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.address.is_some() {
            return;
        }
        let list = dispatch("project.new.generators", json!({})).unwrap_or_default();
        let allowed = list["networkAllowed"].as_bool().unwrap_or(true);
        let found = list["generators"].as_array().into_iter().flatten().find(|info| info["id"] == GENERATOR);
        let service = found.and_then(|info| info["service"].as_str()).filter(|_| allowed && !self.by_agent);
        self.address = Some(service.map(str::to_owned));
    }

    fn waits_to_connect(&self) -> Option<&str> {
        let address = self.address.as_ref()?.as_deref()?;
        (self.connected.as_deref() != Some(address)).then_some(address)
    }

    pub(super) fn connect(&mut self, dispatch: &mut Dispatch<'_>) {
        self.connected = self.waits_to_connect().map(str::to_owned);
        self.load(dispatch);
    }

    pub(super) fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        self.find_address(dispatch);
        if self.waits_to_connect().is_some() {
            return;
        }
        if self.loaded.as_ref().is_some_and(|loaded| loaded.state == "ready" || loaded.state == "failed") {
            return;
        }
        let mut params = self.target();
        params["retry"] = json!(std::mem::take(&mut self.retry));
        match dispatch("springBoot.starters.get", params).map(serde_json::from_value::<StartersState>) {
            Ok(Ok(answer)) => {
                self.module = Some(answer.module.clone());
                self.ticked = answer.ticked.iter().cloned().collect();
                self.loaded = Some(answer);
                self.error = None;
            }
            Ok(Err(error)) => self.fail(error.to_string()),
            Err(error) => self.fail(error),
        }
    }

    fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.loaded = Some(StartersState { state: "failed".to_owned(), ..StartersState::default() });
    }

    fn delta(&self) -> (Vec<String>, Vec<String>) {
        let had: BTreeSet<String> = self.loaded.iter().flat_map(|loaded| loaded.ticked.iter().cloned()).collect();
        (self.ticked.difference(&had).cloned().collect(), had.difference(&self.ticked).cloned().collect())
    }

    pub(super) fn set(&mut self, ticked: BTreeSet<String>) {
        self.ticked = ticked;
    }

    pub(super) fn apply(&mut self, dispatch: &mut Dispatch<'_>) -> bool {
        let (add, remove) = self.delta();
        let mut params = self.target();
        params["add"] = json!(add);
        params["remove"] = json!(remove);
        match dispatch("springBoot.starters.apply", params) {
            Ok(_) => true,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    pub(super) fn state(&self) -> Value {
        let (add, remove) = self.delta();
        json!({
            "loaded": self.loaded.as_ref().map(|loaded| loaded.state.clone()),
            "connects": self.waits_to_connect(),
            "module": self.module,
            "bootVersion": self.loaded.as_ref().map(|loaded| loaded.boot_version.clone()),
            "ticked": self.ticked,
            "add": add,
            "remove": remove,
            "notice": self.loaded.as_ref().and_then(|loaded| loaded.notice.clone()),
            "error": self.error.clone().or_else(|| self.loaded.as_ref().and_then(|loaded| loaded.error.clone())),
        })
    }

    fn chooser(&mut self, ui: &mut Ui, p: &IdePalette, field: &Field) {
        let FieldKind::Select { groups, .. } = &field.kind else { return };
        let boot = self.loaded.as_ref().map(|loaded| loaded.boot_version.clone()).unwrap_or_default();
        let all = Values::from([("bootVersion".to_owned(), json!(boot))]);
        new_project_dialog::option_list(ui, p, field, groups, &mut self.ticked, &all, &mut self.search);
    }

    fn body(&mut self, ui: &mut Ui, p: &IdePalette) {
        if let Some(address) = self.waits_to_connect().map(str::to_owned) {
            ui.horizontal(|ui| {
                let text = t!("ide-new-project-connects", service = address.as_str());
                ui.label(RichText::new(text).font(theme::regular(13.0)).color(p.text));
                self.connect_pressed |= ui.button(t!("ide-new-project-connect")).clicked();
            });
            return;
        }
        let Some(loaded) = self.loaded.clone() else {
            ui.add(egui::Spinner::new().size(14.0).color(p.text_secondary));
            return;
        };
        if !loaded.boot_version.is_empty() {
            let line =
                t!("ide-starters-version", version = loaded.boot_version.as_str(), module = loaded.name.as_str());
            ui.label(RichText::new(line).font(theme::regular(13.0)).color(p.text_secondary));
        }
        if let Some(notice) = &loaded.notice {
            ui.add(egui::Label::new(RichText::new(notice).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
        }
        match (loaded.state.as_str(), &loaded.field) {
            ("ready", Some(field)) => self.chooser(ui, p, field),
            ("loading", _) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(p.text_secondary));
                    let text = t!("ide-starters-asking");
                    ui.label(RichText::new(text).font(theme::regular(13.0)).color(p.text_secondary));
                });
            }
            _ => {}
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) -> Option<bool> {
        if std::mem::take(&mut self.connect_pressed) {
            self.connect(dispatch);
        }
        self.load(dispatch);
        let waiting = self.waits_to_connect().is_some();
        let loading = !waiting && self.loaded.as_ref().is_none_or(|loaded| loaded.state == "loading");
        if loading {
            ui.ctx().request_repaint_after(POLL);
        }
        let (add, remove) = self.delta();
        let ready = !add.is_empty() || !remove.is_empty();
        let failed = self.loaded.as_ref().is_some_and(|loaded| loaded.state == "failed");
        let mut outcome = None;
        let size = ui.ctx().content_rect().size();
        let (width, height) = ((size.x - 80.0).clamp(420.0, WIDTH), (size.y - 80.0).clamp(320.0, HEIGHT));
        let modal = egui::Modal::new(ui.scope_id().with("edit-starters")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.set_height(height);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new(t!("ide-starters-title")).font(theme::semibold(15.0)).color(p.text));
            self.body(ui, p);
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let label =
                            RichText::new(t!("ide-starters-apply")).font(theme::regular(13.0)).color(p.accent_text);
                        let primary = egui::Button::new(label)
                            .min_size(vec2(84.0, 30.0))
                            .corner_radius(6)
                            .fill(if ready { p.accent } else { p.accent.gamma_multiply(0.4) });
                        if ui.add_enabled(ready, primary).clicked() && self.apply(dispatch) {
                            outcome = Some(true);
                        }
                        let cancel = RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text);
                        let cancel = egui::Button::new(cancel)
                            .min_size(vec2(84.0, 30.0))
                            .corner_radius(6)
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, p.button_border));
                        if ui.add(cancel).clicked() {
                            outcome = Some(false);
                        }
                        if failed {
                            let again =
                                RichText::new(t!("ide-starters-retry")).font(theme::regular(13.0)).color(p.text);
                            let again = egui::Button::new(again)
                                .min_size(vec2(84.0, 30.0))
                                .corner_radius(6)
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::new(1.0, p.button_border));
                            if ui.add(again).clicked() {
                                self.retry = true;
                                self.loaded = None;
                                self.error = None;
                            }
                        }
                    });
                });
                let error = self.error.clone().or_else(|| self.loaded.as_ref().and_then(|loaded| loaded.error.clone()));
                if let Some(line) = error {
                    ui.add(egui::Label::new(RichText::new(line).font(theme::regular(13.0)).color(p.error_text)).wrap());
                }
            });
        });
        if modal.should_close() && outcome.is_none() {
            outcome = Some(false);
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn answering(
        calls: &RefCell<Vec<String>>,
        id: &str,
        service: Option<&str>,
        allowed: bool,
    ) -> Result<Value, String> {
        calls.borrow_mut().push(id.to_owned());
        Ok(match id {
            "project.new.generators" => json!({
                "networkAllowed": allowed,
                "generators": [{ "id": GENERATOR, "service": service }],
            }),
            _ => json!({ "state": "loading" }),
        })
    }

    fn asked(calls: &RefCell<Vec<String>>) -> usize {
        calls.borrow().iter().filter(|id| *id == "springBoot.starters.get").count()
    }

    #[test]
    fn a_person_sees_the_address_first_and_it_asks_only_after_connect() {
        let calls = RefCell::new(Vec::new());
        let mut dispatch = |id: &str, _: Value| answering(&calls, id, Some("https://start.example"), true);
        let mut dialog = StartersDialog::default();
        dialog.load(&mut dispatch);
        dialog.load(&mut dispatch);
        assert_eq!(dialog.state()["connects"], "https://start.example");
        assert_eq!(asked(&calls), 0, "nothing was asked before the address was seen");
        dialog.connect(&mut dispatch);
        assert_eq!(asked(&calls), 1);
        assert_eq!(dialog.state()["connects"], Value::Null, "once per address in a dialog");
        dialog.load(&mut dispatch);
        assert_eq!(asked(&calls), 2, "it follows the answer without asking the person again");
    }

    #[test]
    fn an_agent_s_dialog_and_a_network_that_is_off_show_no_button() {
        let calls = RefCell::new(Vec::new());
        let mut dispatch = |id: &str, _: Value| answering(&calls, id, Some("https://start.example"), true);
        let mut dialog = StartersDialog { by_agent: true, ..StartersDialog::default() };
        dialog.load(&mut dispatch);
        assert_eq!(asked(&calls), 1, "an agent's explicit use stays, the owner's decision");
        let calls = RefCell::new(Vec::new());
        let mut dispatch = |id: &str, _: Value| answering(&calls, id, Some("https://start.example"), false);
        let mut dialog = StartersDialog::default();
        dialog.load(&mut dispatch);
        assert_eq!(dialog.state()["connects"], Value::Null, "off: the host refuses, saved answers may show");
        assert_eq!(asked(&calls), 1);
    }
}
