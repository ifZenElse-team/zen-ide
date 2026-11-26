use egui::{Stroke, Ui, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::activity_monitor::{ActivityReport, ActivityRow, Subsystem};

use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

#[derive(Debug, Default)]
pub(super) struct ActivityMonitorDialog {
    report: ActivityReport,
}

impl IdeView {
    pub(super) fn open_activity_monitor(&mut self) {
        if self.activity_monitor.is_none() {
            self.activity_monitor = Some(ActivityMonitorDialog::default());
        }
    }

    pub(super) fn activity_monitor_open(&self) -> bool {
        self.activity_monitor.is_some()
    }

    pub(super) fn activity_monitor(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if self.activity_monitor.is_none() {
            return;
        }
        if let Ok(value) = registry.dispatch(session, "diagnostics.activity.get", serde_json::json!({}))
            && let Ok(report) = serde_json::from_value::<ActivityReport>(value)
            && let Some(dialog) = &mut self.activity_monitor
        {
            dialog.report = report;
        }
        ui.ctx().request_repaint_after(zen_workbench::activity_monitor::INTERVAL);
        let p = self.palette;
        let Some(dialog) = &self.activity_monitor else { return };
        if show(ui, &p, &dialog.report) {
            self.activity_monitor = None;
        }
    }
}

pub(super) fn row_name(row: &ActivityRow) -> String {
    let name = match row.subsystem {
        Subsystem::Ide => t!("ide-activity-ide"),
        Subsystem::Index => t!("ide-activity-index"),
        Subsystem::LanguageServer => t!("ide-activity-server", name = row.name.as_str()),
        Subsystem::Vcs => t!("ide-activity-vcs"),
        Subsystem::Process => t!("ide-activity-process", name = row.name.as_str()),
    };
    format!("<{name}>")
}

fn cpu_text(row: &ActivityRow) -> String {
    row.cpu_percent.map_or_else(|| "…".to_owned(), |percent| format!("{percent:.1}"))
}

pub(super) fn report_text(report: &ActivityReport) -> String {
    let mut rows: Vec<&ActivityRow> = report.rows.iter().collect();
    rows.sort_by(|a, b| b.cpu_percent.unwrap_or(-1.0).total_cmp(&a.cpu_percent.unwrap_or(-1.0)));
    let mut text = format!("{}  {}\n", t!("ide-activity-cpu"), t!("ide-activity-subsystem"));
    for row in rows {
        text.push_str(&format!("\n{:>4}  {}", cpu_text(row), row_name(row)));
    }
    text
}

fn show(ui: &mut Ui, p: &IdePalette, report: &ActivityReport) -> bool {
    let mut close = false;
    let modal = egui::Modal::new(ui.scope_id().with("activity-monitor")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(800.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(egui::RichText::new(t!("ide-activity-title")).font(theme::semibold(13.0)).color(p.text));
        let text = report_text(report);
        egui::Frame::new().stroke(Stroke::new(1.0, p.button_border)).inner_margin(egui::Margin::symmetric(8, 6)).show(
            ui,
            |ui| {
                ui.set_min_size(vec2(784.0, 186.0));
                egui::ScrollArea::vertical().max_height(186.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.add(egui::Label::new(
                        egui::RichText::new(text).font(egui::FontId::monospace(13.0)).color(p.text),
                    ));
                });
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if super::vcs::primary_button(ui, p, &t!("ide-activity-ok")).clicked() {
                close = true;
            }
        });
    });
    close || modal.should_close()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_lists_the_busiest_part_first() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let row = |subsystem, name: &str, cpu_percent| ActivityRow {
            subsystem,
            name: name.to_owned(),
            pids: vec![1],
            cpu_percent,
            memory_bytes: 0,
        };
        assert_eq!(row_name(&row(Subsystem::Ide, "gogarden", None)), "<Process total CPU usage>");
        assert_eq!(row_name(&row(Subsystem::LanguageServer, "gopls", None)), "<Language server: gopls>");
        assert_eq!(row_name(&row(Subsystem::Vcs, "git", None)), "<Git>");
        let report = ActivityReport {
            rows: vec![
                row(Subsystem::Index, "index", Some(0.7)),
                row(Subsystem::Ide, "gogarden", Some(5.34)),
                row(Subsystem::Process, "go", None),
                row(Subsystem::LanguageServer, "gopls", Some(2.2)),
            ],
            ..ActivityReport::default()
        };
        assert_eq!(
            report_text(&report),
            "%CPU  Subsystem\n\n 5.3  <Process total CPU usage>\n 2.2  <Language server: gopls>\n 0.7  <Index>\n   …  <go>"
        );
    }
}
