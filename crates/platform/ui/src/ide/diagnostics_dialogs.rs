use egui::{Color32, Stroke, Ui, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::diagnostics::{Categories, SpecialFolder};

use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

#[derive(Debug, Default)]
pub(super) struct DiagnosticsDialogs {
    pub(super) debug_log: Option<String>,
    pub(super) special: Option<Vec<SpecialFolder>>,
    pub(super) collect: bool,
    pub(super) licences: Option<crate::licences::LicencesView>,
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

enum DebugChoice {
    Ok,
    Cancel,
}

fn debug_log(ui: &mut Ui, p: &IdePalette, text: &mut String) -> Option<DebugChoice> {
    let mut choice = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("debug-log-settings")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(480.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("ide-debug-log-title")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t!("ide-debug-log-hint")).font(theme::regular(12.0)).color(p.text_secondary),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            let edit = egui::TextEdit::multiline(text).desired_rows(10).desired_width(f32::INFINITY).code_editor();
            ui.add(edit);
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if button(ui, p, t!("settings-ok"), true) {
                    choice = Some(DebugChoice::Ok);
                }
                if button(ui, p, t!("settings-cancel"), false) {
                    choice = Some(DebugChoice::Cancel);
                }
            });
        });
    choice.or_else(|| modal.should_close().then_some(DebugChoice::Cancel))
}

fn collect(ui: &mut Ui, p: &IdePalette) -> Option<bool> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("collect-logs")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(480.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(egui::RichText::new(t!("ide-collect-logs-title")).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(8.0);
        let text = egui::RichText::new(t!("ide-collect-logs-text")).font(theme::regular(13.0)).color(p.text);
        ui.add(egui::Label::new(text).wrap());
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if button(ui, p, t!("ide-collect-logs-go"), true) {
                choice = Some(true);
            }
            if button(ui, p, t!("settings-cancel"), false) {
                choice = Some(false);
            }
        });
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            choice = Some(true);
        }
    });
    choice.or_else(|| modal.should_close().then_some(false))
}

fn special(ui: &mut Ui, p: &IdePalette, folders: &[SpecialFolder]) -> (bool, Option<std::path::PathBuf>) {
    let (mut close, mut opened) = (false, None);
    let modal = egui::Modal::new(ui.scope_id().with("special-files")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(640.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(egui::RichText::new(t!("ide-special-title")).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(6.0);
        egui::Grid::new("special-files-rows").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
            for folder in folders {
                ui.label(egui::RichText::new(&folder.name).font(theme::regular(13.0)).color(p.text));
                let path =
                    egui::RichText::new(folder.path.display().to_string()).font(theme::regular(13.0)).color(p.link);
                if ui.add(egui::Label::new(path).sense(egui::Sense::click())).clicked() {
                    opened = Some(folder.path.clone());
                }
                ui.end_row();
            }
        });
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, p, t!("settings-ok"), true) {
                close = true;
            }
        });
    });
    (close || modal.should_close(), opened)
}

impl IdeView {
    pub(super) fn open_debug_log_settings(&mut self, registry: &Registry, session: &mut Session) {
        let read = registry.dispatch(session, "diagnostics.logCategories.get", json!({}));
        let categories = read.ok().and_then(|value| serde_json::from_value::<Categories>(value).ok());
        self.diagnostics.debug_log = Some(categories.unwrap_or_default().categories.join("\n"));
    }

    pub(super) fn open_special_files(&mut self, registry: &Registry, session: &mut Session) {
        let read = registry.dispatch(session, "diagnostics.folders.list", json!({}));
        let folders = read.ok().and_then(|value| serde_json::from_value::<Vec<SpecialFolder>>(value).ok());
        self.diagnostics.special = Some(folders.unwrap_or_default());
    }

    pub(super) fn open_licences(&mut self) -> serde_json::Value {
        let view = crate::licences::LicencesView::bundled();
        let listed =
            json!({ "folder": view.folder().map(|folder| folder.display().to_string()), "files": view.files() });
        self.diagnostics.licences = Some(view);
        listed
    }

    pub(super) fn diagnostics_dialogs(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        if let Some(view) = &mut self.diagnostics.licences
            && view.show(ui, dialog_frame(&p), self.product.name)
        {
            self.diagnostics.licences = None;
        }
        if let Some(text) = &mut self.diagnostics.debug_log
            && let Some(choice) = debug_log(ui, &p, text)
        {
            let lines: Vec<&str> = text.lines().collect();
            if matches!(choice, DebugChoice::Ok) {
                let params = json!({ "categories": lines });
                self.dispatch(ui, registry, session, "diagnostics.logCategories.set", params);
            }
            self.diagnostics.debug_log = None;
        }
        if self.diagnostics.collect
            && let Some(go) = collect(ui, &p)
        {
            self.diagnostics.collect = false;
            if go {
                self.dispatch(ui, registry, session, "diagnostics.logs.collect", json!({}));
            }
        }
        if let Some(folders) = &self.diagnostics.special {
            let (close, opened) = special(ui, &p, folders);
            if let Some(path) = opened {
                self.dispatch(ui, registry, session, "file.fs.reveal", json!({ "path": path }));
            }
            if close {
                self.diagnostics.special = None;
            }
        }
    }
}
