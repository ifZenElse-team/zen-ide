use egui::{Color32, Key, Stroke, Ui, vec2};
use zen_i18n::t;
use zen_workbench::exit::{RunningWork, WorkKind};

use super::{IdePalette, dialog_frame};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Exit,
    Cancel,
}

pub(super) fn work_line(work: &RunningWork) -> String {
    let title = work.title.as_str();
    match work.kind {
        WorkKind::DependencyUpdate => t!("ide-exit-work-dependency-update", title = title),
        WorkKind::Git => t!("ide-exit-work-git", title = title),
        WorkKind::Task => t!("ide-exit-work-task", title = title),
        WorkKind::Run => t!("ide-exit-work-run", title = title),
        WorkKind::Debug => t!("ide-exit-work-debug", title = title),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ending {
    Exit,
    Restart,
}

impl Ending {
    fn question(self, product: &str) -> String {
        match self {
            Self::Exit => t!("ide-exit-question", product = product),
            Self::Restart => t!("ide-restart-question", product = product),
        }
    }

    fn stops(self) -> String {
        match self {
            Self::Exit => t!("ide-exit-stops"),
            Self::Restart => t!("ide-restart-stops"),
        }
    }

    fn confirm(self) -> String {
        match self {
            Self::Exit => t!("ide-exit-confirm"),
            Self::Restart => t!("ide-restart-confirm"),
        }
    }
}

pub(super) fn show(
    ui: &mut Ui,
    p: &IdePalette,
    ending: Ending,
    product: &str,
    running: &[RunningWork],
) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("confirm-exit")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(400.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        let question = ending.question(product);
        ui.add(egui::Label::new(egui::RichText::new(question).font(theme::semibold(15.0)).color(p.text)).wrap());
        if !running.is_empty() {
            ui.add_space(12.0);
            for work in running {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(egui::RichText::new("•").font(theme::regular(13.0)).color(p.text_secondary));
                    ui.add(
                        egui::Label::new(egui::RichText::new(work_line(work)).font(theme::regular(13.0)).color(p.text))
                            .wrap(),
                    );
                });
                ui.add_space(4.0);
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(ending.stops()).font(theme::regular(12.0)).color(p.text_secondary));
        }
        ui.add_space(20.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let exit = egui::Button::new(
                egui::RichText::new(ending.confirm()).font(theme::regular(13.0)).color(p.accent_text),
            )
            .min_size(vec2(84.0, 30.0))
            .corner_radius(6)
            .fill(p.accent);
            if ui.add(exit).clicked() {
                choice = Some(Choice::Exit);
            }
            let cancel =
                egui::Button::new(egui::RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text))
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, p.button_border));
            if ui.add(cancel).clicked() {
                choice = Some(Choice::Cancel);
            }
        });
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            choice = Some(Choice::Exit);
        }
    });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_work_is_named_by_its_kind() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let line = |kind| work_line(&RunningWork { kind, title: "main".to_owned() });
        assert_eq!(line(WorkKind::DependencyUpdate), "A dependency update is running: main");
        assert_eq!(line(WorkKind::Git), "A Git operation is running: main");
        assert_eq!(line(WorkKind::Task), "A background task is running: main");
        assert_eq!(line(WorkKind::Run), "A run configuration is running: main");
        assert_eq!(line(WorkKind::Debug), "A debug session is running: main");
    }
}
