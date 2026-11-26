use egui::{Key, Rect, Sense, Ui, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::internal_errors::{ErrorReport, InternalError, InternalErrors};

use super::vcs::{primary_button, secondary_button};
use super::{IdeView, accessibility, dialog_frame};
use crate::theme;

const WIDTH: f32 = 640.0;
const STACK_HEIGHT: f32 = 220.0;

#[derive(Debug, Default)]
pub(super) struct ErrorsDialog {
    shown: usize,
    description: Option<(u64, String)>,
}

impl IdeView {
    pub(super) fn errors_indicator(&mut self, ui: &mut Ui, session: &Session, right: f32, y: f32) -> f32 {
        let count = session.state::<InternalErrors>().map_or(0, InternalErrors::count);
        if count == 0 {
            return right;
        }
        let p = self.palette;
        let label = t!("ide-internal-errors-count", count = count);
        let galley = ui.painter().layout_no_wrap(count.to_string(), theme::semibold(11.5), p.accent_text);
        let width = (galley.size().x + 12.0).max(18.0);
        let pill = Rect::from_min_size(pos2(right - width, y - 9.0), vec2(width, 18.0));
        ui.painter().rect_filled(pill, 9, p.diagnostic_error);
        ui.painter().galley(pill.center() - galley.size() / 2.0, galley, p.accent_text);
        let response =
            ui.interact(pill, ui.scope_id().with("internal-errors"), Sense::click()).on_hover_text(label.as_str());
        accessibility::name_with(&response, egui::Role::Button, || label.clone());
        if response.clicked() {
            self.errors_dialog = Some(ErrorsDialog::default());
        }
        pill.left() - 8.0
    }

    pub(super) fn open_errors_dialog(&mut self) {
        self.errors_dialog = Some(ErrorsDialog::default());
    }

    pub(super) fn internal_errors_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &mut self.errors_dialog else { return };
        let errors = session.state::<InternalErrors>().map(InternalErrors::list).unwrap_or_default();
        if errors.is_empty() {
            self.errors_dialog = None;
            return;
        }
        dialog.shown = dialog.shown.min(errors.len() - 1);
        let p = self.palette;
        let error = &errors[dialog.shown];
        let total = errors.len();
        let mut close = false;
        let mut clear = false;
        let mut copy = false;
        let mut open_issue = false;
        if dialog.description.as_ref().is_none_or(|(id, _)| *id != error.id) {
            dialog.description = Some((error.id, error.description.clone()));
        }
        let modal = egui::Modal::new(ui.scope_id().with("internal-errors-dialog")).frame(dialog_frame(&p)).show(
            ui.ctx(),
            |ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    let title = egui::RichText::new(t!("ide-internal-errors-title")).font(theme::semibold(15.0));
                    ui.label(title.color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(dialog.shown + 1 < total, egui::Button::new("›")).clicked() {
                            dialog.shown += 1;
                        }
                        let place = t!("ide-internal-errors-place", number = dialog.shown + 1, total = total);
                        ui.label(egui::RichText::new(place).font(theme::regular(12.5)).color(p.text_secondary));
                        if ui.add_enabled(dialog.shown > 0, egui::Button::new("‹")).clicked() {
                            dialog.shown -= 1;
                        }
                    });
                });
                ui.add_space(12.0);
                message(ui, &p, error);
                ui.add_space(8.0);
                egui::ScrollArea::both().max_height(STACK_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(&error.stack).font(egui::FontId::monospace(12.0)).color(p.text_secondary),
                    );
                });
                ui.add_space(12.0);
                let hint = t!("ide-internal-errors-description");
                ui.label(egui::RichText::new(hint.as_str()).font(theme::regular(12.5)).color(p.text_secondary));
                ui.add_space(4.0);
                if let Some((id, text)) = &mut dialog.description {
                    let field = egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY);
                    let field = ui.add(field);
                    accessibility::edit(&field, |node| node.set_label(hint.as_str()));
                    if field.changed() {
                        let params = json!({ "id": *id, "description": text });
                        let _ = registry.dispatch(session, "ide.errors.describe", params);
                    }
                }
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    clear = secondary_button(ui, &p, &t!("ide-internal-errors-clear")).clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = primary_button(ui, &p, &t!("ide-internal-errors-close")).clicked();
                        open_issue = secondary_button(ui, &p, &t!("ide-internal-errors-open-issue")).clicked();
                        copy = secondary_button(ui, &p, &t!("ide-internal-errors-copy")).clicked();
                    });
                });
            },
        );
        let escape = modal.should_close() || ui.input(|input| input.key_pressed(Key::Escape));
        if copy || open_issue {
            let id = error.id;
            let report = registry
                .dispatch(session, "ide.errors.report", json!({ "id": id }))
                .ok()
                .and_then(|report| serde_json::from_value::<ErrorReport>(report).ok());
            if let Some(report) = report {
                if copy {
                    ui.ctx().copy_text(report.text);
                } else {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(report.url));
                }
            }
        }
        if clear {
            let _ = registry.dispatch(session, "ide.errors.clear", json!({}));
        }
        if close || clear || escape {
            self.errors_dialog = None;
        }
    }
}

fn message(ui: &mut Ui, p: &super::IdePalette, error: &InternalError) {
    let text = egui::RichText::new(&error.message).font(theme::regular(13.5)).color(p.text);
    ui.add(egui::Label::new(text).wrap());
}
