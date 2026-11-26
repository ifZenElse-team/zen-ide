use egui::Ui;
use zen_i18n::t;
use zen_workbench::notifications::Group;

use super::{AUTOSAVE_IDLE_SECONDS, OpenProjectIn, SettingsDialog, widgets};
use crate::ide::IdePalette;
use crate::ide::notifications::group_name;

impl SettingsDialog {
    pub(super) fn system_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::checkbox(ui, p, &mut draft.confirm_exit, &t!("settings-system-confirm-exit"), query);
        ui.add_space(8.0);
        ui.label(egui::RichText::new(t!("settings-system-confirm-exit-note")).color(p.text_secondary));
        ui.add_space(8.0);
        widgets::checkbox(ui, p, &mut draft.whats_new_on_update, &t!("settings-system-whats-new"), query);
        ui.add_space(4.0);
        widgets::checkbox(ui, p, &mut draft.show_tips_on_startup, &t!("settings-system-tips"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-open-project"), query);
        let mut opening = draft.project_opening();
        ui.radio_value(
            &mut opening,
            OpenProjectIn::NewWindow,
            widgets::label_job(ui, p, &t!("settings-open-window"), query),
        );
        ui.radio_value(
            &mut opening,
            OpenProjectIn::CurrentWindow,
            widgets::label_job(ui, p, &t!("settings-open-current"), query),
        );
        ui.radio_value(&mut opening, OpenProjectIn::Ask, widgets::label_job(ui, p, &t!("settings-open-ask"), query));
        if opening != draft.project_opening() {
            draft.open_project_in = Some(opening);
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new(t!("settings-open-note")).color(p.text_secondary));
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let label = t!("settings-default-project-directory");
            ui.label(widgets::label_job(ui, p, &label, query));
            let width = widgets::field_width(ui, 200.0, 420.0);
            let field = egui::TextEdit::singleline(&mut draft.default_project_directory).desired_width(width);
            let field = ui.add(field);
            crate::accessibility::edit(&field, |node| node.set_label(label.clone()));
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new(t!("settings-default-project-directory-note")).color(p.text_secondary));
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-generators"), query);
        widgets::checkbox(ui, p, &mut draft.generators_network, &t!("settings-generators-network"), query);
        ui.add_space(4.0);
        ui.label(egui::RichText::new(t!("settings-generators-network-note")).color(p.text_secondary));
        for (generator, setting) in &self.generator_settings {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let label = format!("{generator}: {}", setting.label);
                ui.label(widgets::label_job(ui, p, &label, query));
                let mut value = draft.generator_settings.get(&setting.key).cloned().unwrap_or_default();
                let width = widgets::field_width(ui, 200.0, 420.0);
                let field = egui::TextEdit::singleline(&mut value)
                    .hint_text(setting.default.as_str())
                    .desired_width(width)
                    .id_salt(("generator-setting", &setting.key));
                let field = ui.add(field);
                crate::accessibility::edit(&field, |node| node.set_label(label.clone()));
                if field.changed() {
                    if value.trim().is_empty() {
                        draft.generator_settings.remove(&setting.key);
                    } else {
                        draft.generator_settings.insert(setting.key.clone(), value);
                    }
                }
            });
            if let Some(help) = &setting.help {
                ui.label(egui::RichText::new(help).color(p.text_secondary));
            }
        }
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-system-files"), query);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 0.0),
            egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
            |ui| {
                let label = t!("settings-system-autosave-idle");
                let check = widgets::checkbox(ui, p, &mut draft.autosave_on_idle, &label, query);
                ui.add_enabled(
                    draft.autosave_on_idle,
                    egui::DragValue::new(&mut draft.autosave_idle_seconds).range(AUTOSAVE_IDLE_SECONDS),
                )
                .labelled_by(check.id);
                ui.label(widgets::label_job(ui, p, &t!("settings-system-autosave-seconds"), query));
            },
        );
        ui.add_space(4.0);
        widgets::checkbox(
            ui,
            p,
            &mut draft.autosave_on_frame_deactivation,
            &t!("settings-system-autosave-deactivation"),
            query,
        );
        ui.add_space(8.0);
        ui.label(egui::RichText::new(t!("settings-system-autosave-note")).color(p.text_secondary));
    }

    pub(super) fn debugger_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::group_title(ui, p, &t!("settings-debugger-remove"), query);
        let mut drag = draft.remove_breakpoint_by_drag;
        ui.radio_value(&mut drag, false, widgets::label_job(ui, p, &t!("settings-debugger-remove-click"), query));
        ui.radio_value(&mut drag, true, widgets::label_job(ui, p, &t!("settings-debugger-remove-drag"), query));
        draft.remove_breakpoint_by_drag = drag;
        ui.add_space(12.0);
        widgets::group_title(ui, p, &t!("settings-debugger-coroutines"), query);
        widgets::checkbox(
            ui,
            p,
            &mut draft.coroutine_debug_mode,
            &t!("settings-debugger-coroutines-debug-mode"),
            query,
        );
        ui.add(
            egui::Label::new(egui::RichText::new(t!("settings-debugger-coroutines-about")).color(p.text_secondary))
                .wrap(),
        );
    }

    pub(super) fn data_views_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::checkbox(ui, p, &mut draft.inline_values, &t!("settings-debugger-inline-values"), query);
        ui.add_space(6.0);
        widgets::checkbox(ui, p, &mut draft.collection_views, &t!("settings-debugger-collection-views"), query);
        ui.add_space(6.0);
        widgets::checkbox(ui, p, &mut draft.value_tooltip, &t!("settings-debugger-value-tooltip"), query);
        ui.add_space(6.0);
        ui.add_enabled_ui(draft.value_tooltip, |ui| {
            widgets::number_field(
                ui,
                p,
                &t!("settings-debugger-tooltip-delay"),
                query,
                &mut draft.value_tooltip_delay,
                0..=10_000,
            );
        });
    }

    pub(super) fn notifications_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::checkbox(ui, p, &mut draft.notification_balloons, &t!("settings-notifications-balloons"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-notifications-groups"), query);
        ui.add_enabled_ui(draft.notification_balloons, |ui| {
            for group in Group::ALL {
                let mut on = !draft.balloon_groups_off.contains(group.id());
                if widgets::checkbox(ui, p, &mut on, &group_name(group), query).changed() {
                    if on {
                        draft.balloon_groups_off.remove(group.id());
                    } else {
                        draft.balloon_groups_off.insert(group.id().to_owned());
                    }
                }
            }
        });
        ui.add_space(8.0);
        ui.label(egui::RichText::new(t!("settings-notifications-note")).color(p.text_secondary));
    }
}
