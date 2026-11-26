use egui::Ui;
use zen_i18n::t;
use zen_workbench::tabs::{
    ActivateOnClose, CloseButton, EditorTabSettings, MAX_TAB_LIMIT, OverLimit, TabPlacement, TabRows,
};

use super::{SettingsDialog, widgets};
use crate::ide::IdePalette;

pub(super) const SEARCH: &[&str] = &[
    "settings-tabs-appearance",
    "settings-tabs-placement",
    "settings-tabs-one-row",
    "settings-tabs-scroll",
    "settings-tabs-squeeze",
    "settings-tabs-multiple-rows",
    "settings-tabs-pinned-row",
    "settings-tabs-file-icon",
    "settings-tabs-file-extension",
    "settings-tabs-directory",
    "settings-tabs-mark-modified",
    "settings-tabs-full-path",
    "settings-tabs-close-button",
    "settings-tabs-order",
    "settings-tabs-sort",
    "settings-tabs-open-at-end",
    "settings-tabs-opening-policy",
    "settings-tabs-preview",
    "settings-tabs-closing-policy",
    "settings-tabs-limit",
    "settings-tabs-over-limit",
    "settings-tabs-close-unchanged",
    "settings-tabs-close-unused",
    "settings-tabs-activate",
    "settings-tabs-activate-left",
    "settings-tabs-activate-right",
    "settings-tabs-activate-recent",
];

fn placement_name(placement: TabPlacement) -> String {
    match placement {
        TabPlacement::Top => t!("settings-tabs-placement-top"),
        TabPlacement::Bottom => t!("settings-tabs-placement-bottom"),
        TabPlacement::Left => t!("settings-tabs-placement-left"),
        TabPlacement::Right => t!("settings-tabs-placement-right"),
        TabPlacement::None => t!("settings-tabs-placement-none"),
    }
}

fn close_button_name(button: CloseButton) -> String {
    match button {
        CloseButton::Left => t!("settings-tabs-placement-left"),
        CloseButton::Right => t!("settings-tabs-placement-right"),
        CloseButton::None => t!("settings-tabs-placement-none"),
    }
}

fn choice<T: Copy + PartialEq>(
    ui: &mut Ui,
    p: &IdePalette,
    query: Option<&str>,
    (label, id): (&str, &str),
    value: &mut T,
    choices: &[T],
    name: impl Fn(T) -> String,
) {
    ui.horizontal_wrapped(|ui| {
        let title = ui.label(widgets::label_job(ui, p, label, query));
        let combo = egui::ComboBox::from_id_salt(id)
            .selected_text(name(*value))
            .width(widgets::field_width(ui, 120.0, 160.0))
            .show_ui(ui, |ui| {
                for option in choices {
                    ui.selectable_value(value, *option, name(*option));
                }
            });
        combo.response.labelled_by(title.id);
    });
}

fn radio<T: Copy + PartialEq>(ui: &mut Ui, p: &IdePalette, query: Option<&str>, value: &mut T, option: T, key: &str) {
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.radio_value(value, option, widgets::label_job(ui, p, &t!(key), query));
    });
}

fn appearance(ui: &mut Ui, p: &IdePalette, query: Option<&str>, tabs: &mut EditorTabSettings) {
    widgets::group_title(ui, p, &t!("settings-tabs-appearance"), query);
    let placements =
        [TabPlacement::Top, TabPlacement::Bottom, TabPlacement::Left, TabPlacement::Right, TabPlacement::None];
    let label = t!("settings-tabs-placement");
    choice(ui, p, query, (&label, "settings-tabs-placement"), &mut tabs.placement, &placements, placement_name);
    let mut one_row = tabs.rows != TabRows::Multiple;
    let label = t!("settings-tabs-one-row");
    if ui.radio(one_row, widgets::label_job(ui, p, &label, query)).clicked() && !one_row {
        one_row = true;
        tabs.rows = TabRows::Scroll;
    }
    ui.add_enabled_ui(one_row, |ui| {
        radio(ui, p, query, &mut tabs.rows, TabRows::Scroll, "settings-tabs-scroll");
        radio(ui, p, query, &mut tabs.rows, TabRows::Squeeze, "settings-tabs-squeeze");
    });
    let label = t!("settings-tabs-multiple-rows");
    if ui.radio(!one_row, widgets::label_job(ui, p, &label, query)).clicked() {
        tabs.rows = TabRows::Multiple;
    }
    widgets::checkbox(ui, p, &mut tabs.pinned_row, &t!("settings-tabs-pinned-row"), query);
    widgets::checkbox(ui, p, &mut tabs.show_icon, &t!("settings-tabs-file-icon"), query);
    widgets::checkbox(ui, p, &mut tabs.show_extension, &t!("settings-tabs-file-extension"), query);
    widgets::checkbox(ui, p, &mut tabs.show_directory, &t!("settings-tabs-directory"), query);
    widgets::checkbox(ui, p, &mut tabs.mark_modified, &t!("settings-tabs-mark-modified"), query);
    widgets::checkbox(ui, p, &mut tabs.full_path_on_hover, &t!("settings-tabs-full-path"), query);
    let buttons = [CloseButton::Right, CloseButton::Left, CloseButton::None];
    let label = t!("settings-tabs-close-button");
    choice(ui, p, query, (&label, "settings-tabs-close-button"), &mut tabs.close_button, &buttons, close_button_name);
}

fn closing_policy(ui: &mut Ui, p: &IdePalette, query: Option<&str>, tabs: &mut EditorTabSettings) {
    widgets::group_title(ui, p, &t!("settings-tabs-closing-policy"), query);
    widgets::number_field(ui, p, &t!("settings-tabs-limit"), query, &mut tabs.limit, 1..=MAX_TAB_LIMIT);
    ui.label(widgets::label_job(ui, p, &t!("settings-tabs-over-limit"), query));
    radio(ui, p, query, &mut tabs.over_limit, OverLimit::CloseUnchanged, "settings-tabs-close-unchanged");
    radio(ui, p, query, &mut tabs.over_limit, OverLimit::CloseUnused, "settings-tabs-close-unused");
    ui.label(widgets::label_job(ui, p, &t!("settings-tabs-activate"), query));
    radio(ui, p, query, &mut tabs.activate_on_close, ActivateOnClose::Left, "settings-tabs-activate-left");
    radio(ui, p, query, &mut tabs.activate_on_close, ActivateOnClose::Right, "settings-tabs-activate-right");
    radio(ui, p, query, &mut tabs.activate_on_close, ActivateOnClose::Recent, "settings-tabs-activate-recent");
}

impl SettingsDialog {
    pub(super) fn editor_tabs_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let tabs = &mut draft.editor_tabs;
        appearance(ui, p, query, tabs);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-tabs-order"), query);
        widgets::checkbox(ui, p, &mut tabs.sort_alphabetically, &t!("settings-tabs-sort"), query);
        widgets::checkbox(ui, p, &mut tabs.open_at_end, &t!("settings-tabs-open-at-end"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-tabs-opening-policy"), query);
        widgets::checkbox(ui, p, &mut tabs.preview, &t!("settings-tabs-preview"), query);
        ui.label(egui::RichText::new(t!("settings-tabs-preview-note")).small().color(p.text_secondary));
        ui.add_space(14.0);
        closing_policy(ui, p, query, tabs);
    }
}
