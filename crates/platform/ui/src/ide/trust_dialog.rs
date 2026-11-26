use std::path::{Path, PathBuf};

use egui::{Color32, Stroke, Ui, vec2};
use zen_i18n::t;

use super::settings::widgets;
use super::{IdePalette, dialog_frame};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Answer {
    Trust,
    Preview,
    Leave,
}

#[derive(Clone, Debug)]
pub(super) struct Question {
    pub(super) folder: PathBuf,
    pub(super) then_open: PathBuf,
    pub(super) own_window: bool,
    pub(super) already_open: bool,
    pub(super) all_in_parent: bool,
    pub(super) error: Option<String>,
}

impl Question {
    pub(super) fn opening(folder: PathBuf, then_open: PathBuf, own_window: bool) -> Self {
        Self { folder, then_open, own_window, already_open: false, all_in_parent: false, error: None }
    }

    pub(super) fn of_open_project(folder: PathBuf) -> Self {
        Self { then_open: folder.clone(), already_open: true, ..Self::opening(folder, PathBuf::new(), false) }
    }

    pub(super) fn parent(&self) -> Option<&Path> {
        self.folder.parent().filter(|parent| parent.parent().is_some())
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn button(ui: &mut Ui, p: &IdePalette, label: String, primary: bool) -> bool {
    let text =
        egui::RichText::new(label).font(theme::regular(13.0)).color(if primary { p.accent_text } else { p.text });
    let button = egui::Button::new(text).min_size(vec2(84.0, 30.0)).corner_radius(6);
    let button = if primary {
        button.fill(p.accent)
    } else {
        button.fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, p.button_border))
    };
    ui.add(button).clicked()
}

pub(super) fn show(ui: &mut Ui, p: &IdePalette, question: &mut Question) -> Option<Answer> {
    let mut answer = None;
    let project = name_of(&question.folder);
    let title = if question.already_open {
        t!("ide-trust-title-open", project = project.as_str())
    } else {
        t!("ide-trust-title", project = project.as_str())
    };
    let modal = egui::Modal::new(ui.scope_id().with("trust-project")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(egui::Label::new(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text)).wrap());
        ui.add_space(8.0);
        ui.add(
            egui::Label::new(egui::RichText::new(t!("ide-trust-body")).font(theme::regular(13.0)).color(p.text)).wrap(),
        );
        ui.add_space(8.0);
        let path = question.folder.display().to_string();
        ui.add(egui::Label::new(egui::RichText::new(path).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
        if let Some(parent) = question.parent().map(name_of) {
            ui.add_space(12.0);
            let label = t!("ide-trust-all-in", parent = parent.as_str());
            widgets::checkbox(ui, p, &mut question.all_in_parent, &label, None);
            if question.all_in_parent && question.parent().is_some_and(zen_workbench::trust::covers_home) {
                ui.add_space(6.0);
                let warning = egui::RichText::new(t!("settings-trusted-locations-home"))
                    .font(theme::regular(12.0))
                    .color(p.diagnostic_warning);
                ui.add(egui::Label::new(warning).wrap());
            }
        }
        if let Some(error) = &question.error {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(error).font(theme::regular(12.0)).color(p.diagnostic_error));
        }
        ui.add_space(20.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if question.already_open {
                if button(ui, p, t!("ide-trust-project"), true) {
                    answer = Some(Answer::Trust);
                }
                if button(ui, p, t!("settings-cancel"), false) {
                    answer = Some(Answer::Leave);
                }
                return;
            }
            if button(ui, p, t!("ide-trust-dont-open"), false) {
                answer = Some(Answer::Leave);
            }
            if button(ui, p, t!("ide-trust-preview"), false) {
                answer = Some(Answer::Preview);
            }
            if button(ui, p, t!("ide-trust-project"), true) {
                answer = Some(Answer::Trust);
            }
        });
    });
    answer.or_else(|| modal.should_close().then_some(Answer::Leave))
}
