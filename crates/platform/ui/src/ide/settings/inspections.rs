use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use egui::{Rect, Sense, Ui, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_workbench::inspection_profiles::{
    InspectionOverride, ProfileId, ProfileInspection, ProfileLevel, ProfileList, ProfileView,
};
use zen_workbench::language::Severity;

use super::model::SpellingSettings;
use super::{Dispatch, SettingsDialog, widgets};
use crate::ide::IdePalette;
use crate::ide::glyphs::glyph_settings;
use crate::ide::inspect_window::severity_title;
use crate::{accessibility, theme};

const TREE_WIDTH: f32 = 300.0;
const TREE_HEIGHT: f32 = 380.0;
const ROW_INDENT: f32 = 22.0;
const SEVERITIES: [Severity; 5] =
    [Severity::Error, Severity::Warning, Severity::WeakWarning, Severity::Information, Severity::Hint];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Gear {
    CopyToOtherLevel,
    Duplicate,
    Rename,
    Delete,
    Export,
    Import,
    Restore,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Selected {
    #[default]
    Spelling,
    Inspection(String),
}

#[derive(Default)]
pub(super) struct InspectionsPage {
    loaded: bool,
    list: Option<ProfileList>,
    view: Option<ProfileView>,
    chosen: Option<ProfileId>,
    edits: BTreeMap<String, InspectionOverride>,
    filter: String,
    only_modified: bool,
    only_cleanup: bool,
    only_severity: Option<Severity>,
    collapsed: BTreeSet<String>,
    selected: Selected,
    naming: Option<(Gear, String)>,
    message: Option<String>,
}

fn other_level(level: ProfileLevel) -> ProfileLevel {
    match level {
        ProfileLevel::Project => ProfileLevel::Ide,
        ProfileLevel::Ide => ProfileLevel::Project,
    }
}

fn profile_label(id: &ProfileId) -> String {
    match id.level {
        ProfileLevel::Project => t!("settings-inspections-level-project", name = id.name.as_str()),
        ProfileLevel::Ide => t!("settings-inspections-level-ide", name = id.name.as_str()),
    }
}

fn is_builtin(id: &ProfileId) -> bool {
    *id == ProfileId::default()
}

fn modified(row: &ProfileInspection) -> bool {
    row.severity.is_some() || row.enabled != row.enabled_by_default
}

impl InspectionsPage {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch, None);
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>, wanted: Option<ProfileId>) {
        let list = dispatch("inspect.profile.list", json!({}))
            .and_then(|value| serde_json::from_value::<ProfileList>(value).map_err(|error| error.to_string()));
        let list = match list {
            Ok(list) => list,
            Err(error) => {
                self.message = Some(error);
                return;
            }
        };
        let chosen = wanted
            .or_else(|| self.chosen.take())
            .filter(|id| list.profiles.contains(id))
            .unwrap_or_else(|| list.active.clone());
        self.show_profile(dispatch, chosen);
        self.list = Some(list);
        self.loaded = true;
    }

    fn show_profile(&mut self, dispatch: &mut Dispatch<'_>, id: ProfileId) {
        match dispatch("inspect.profile.get", json!({ "profile": id }))
            .and_then(|value| serde_json::from_value::<ProfileView>(value).map_err(|error| error.to_string()))
        {
            Ok(view) => self.view = Some(view),
            Err(error) => self.message = Some(error),
        }
        self.chosen = Some(id);
        self.edits.clear();
    }

    fn save_edits(&mut self, dispatch: &mut Dispatch<'_>) -> bool {
        if self.edits.is_empty() {
            return true;
        }
        let params = json!({ "profile": self.chosen, "inspections": self.edits });
        match dispatch("inspect.profile.set", params) {
            Ok(_) => {
                self.edits.clear();
                true
            }
            Err(error) => {
                self.message = Some(error);
                false
            }
        }
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        if !self.loaded {
            return;
        }
        self.save_edits(dispatch);
        let active = self.list.as_ref().map(|list| &list.active);
        if let Some(chosen) = self.chosen.as_ref().filter(|chosen| Some(*chosen) != active) {
            if let Err(error) = dispatch("inspect.profile.select", json!({ "profile": chosen })) {
                self.message = Some(error);
                return;
            }
            if let Some(list) = &mut self.list {
                list.active = chosen.clone();
            }
        }
    }

    pub(super) fn gear(
        &mut self,
        dispatch: &mut Dispatch<'_>,
        action: Gear,
        name: Option<String>,
        file: Option<PathBuf>,
    ) {
        let Some(chosen) = self.chosen.clone() else { return };
        if !self.save_edits(dispatch) {
            return;
        }
        let name = name.unwrap_or_default();
        let (result, next) = match action {
            Gear::CopyToOtherLevel => {
                let to = ProfileId { name, level: other_level(chosen.level) };
                (dispatch("inspect.profile.copy", json!({ "profile": chosen, "to": to })), Some(to))
            }
            Gear::Duplicate => {
                let to = ProfileId { name, level: chosen.level };
                (dispatch("inspect.profile.copy", json!({ "profile": chosen, "to": to })), Some(to))
            }
            Gear::Rename => {
                let to = ProfileId { name: name.clone(), level: chosen.level };
                (dispatch("inspect.profile.rename", json!({ "profile": chosen, "name": name })), Some(to))
            }
            Gear::Delete => (dispatch("inspect.profile.delete", json!({ "profile": chosen })), None),
            Gear::Restore => (dispatch("inspect.profile.restore", json!({ "profile": chosen })), Some(chosen.clone())),
            Gear::Export => {
                let params = json!({ "profile": chosen, "path": file });
                (dispatch("inspect.profile.export", params), Some(chosen.clone()))
            }
            Gear::Import => {
                let params = json!({ "path": file, "level": chosen.level });
                let imported = dispatch("inspect.profile.import", params);
                let next = imported.as_ref().ok().and_then(|value| serde_json::from_value(value.clone()).ok());
                (imported, next)
            }
        };
        self.message = result.err();
        let reload = if self.message.is_some() { Some(chosen) } else { next };
        self.load(dispatch, reload);
    }

    fn rows(&self) -> Vec<&ProfileInspection> {
        let needle = self.filter.trim().to_lowercase();
        let Some(view) = &self.view else { return Vec::new() };
        view.inspections
            .iter()
            .filter(|row| {
                (needle.is_empty()
                    || row.title.to_lowercase().contains(&needle)
                    || row.group.to_lowercase().contains(&needle)
                    || row.description.to_lowercase().contains(&needle))
                    && (!self.only_modified || modified(row))
                    && (!self.only_cleanup || row.cleanup)
                    && self.only_severity.is_none_or(|severity| row.severity == Some(severity))
            })
            .collect()
    }

    fn change(&mut self, id: &str, enabled: Option<bool>, severity: Option<Option<Severity>>) {
        let Some(row) = self.view.as_mut().and_then(|view| view.inspections.iter_mut().find(|row| row.id == id)) else {
            return;
        };
        if let Some(enabled) = enabled {
            row.enabled = enabled;
        }
        if let Some(severity) = severity {
            row.severity = severity;
        }
        row.modified = modified(row);
        let enabled = (row.enabled != row.enabled_by_default).then_some(row.enabled);
        self.edits.insert(row.id.clone(), InspectionOverride { enabled, severity: row.severity });
    }

    fn top(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: Option<&mut Dispatch<'_>>) {
        let Some(dispatch) = dispatch else { return };
        let Some(list) = self.list.clone() else { return };
        let Some(chosen) = self.chosen.clone() else { return };
        ui.horizontal(|ui| {
            ui.label(t!("settings-inspections-profile"));
            let mut picked = None;
            let combo = egui::ComboBox::from_id_salt("settings-inspections-profile")
                .selected_text(profile_label(&chosen))
                .width(260.0)
                .show_ui(ui, |ui| {
                    for id in &list.profiles {
                        if ui.selectable_label(*id == chosen, profile_label(id)).clicked() {
                            picked = Some(id.clone());
                        }
                    }
                });
            accessibility::name(&combo.response, egui::Role::ComboBox, &t!("settings-inspections-profile"));
            if let Some(id) = picked.filter(|id| *id != chosen)
                && self.save_edits(dispatch)
            {
                self.show_profile(dispatch, id);
            }
            let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
            glyph_settings(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), p.text_secondary);
            accessibility::button_with(&response, || t!("settings-inspections-gear"));
            egui::Popup::menu(&response).show(|ui| self.gear_menu(ui, dispatch, &chosen));
        });
        if let Some((action, name)) = self.naming.clone() {
            let mut value = name;
            let done =
                super::spelling::field_row(ui, &mut value, &t!("settings-inspections-new-name"), "inspection-profile");
            self.naming = Some((action, value.clone()));
            match done {
                Some(true) => {
                    self.naming = None;
                    self.gear(dispatch, action, Some(value), None);
                }
                Some(false) => self.naming = None,
                None => {}
            }
        }
    }

    fn gear_menu(&mut self, ui: &mut Ui, dispatch: &mut Dispatch<'_>, chosen: &ProfileId) {
        let copy = match chosen.level {
            ProfileLevel::Project => t!("settings-inspections-copy-to-ide"),
            ProfileLevel::Ide => t!("settings-inspections-copy-to-project"),
        };
        let copied = format!("{} {}", chosen.name, t!("settings-inspections-copy-suffix"));
        if ui.button(copy).clicked() {
            self.naming = Some((Gear::CopyToOtherLevel, chosen.name.clone()));
        }
        if ui.button(t!("settings-inspections-duplicate")).clicked() {
            self.naming = Some((Gear::Duplicate, copied));
        }
        let builtin = is_builtin(chosen);
        if ui.add_enabled(!builtin, egui::Button::new(t!("settings-inspections-rename"))).clicked() {
            self.naming = Some((Gear::Rename, chosen.name.clone()));
        }
        if ui.add_enabled(!builtin, egui::Button::new(t!("settings-inspections-delete"))).clicked() {
            self.gear(dispatch, Gear::Delete, None, None);
        }
        ui.separator();
        if ui.button(t!("settings-inspections-export")).clicked()
            && let Some(file) = rfd::FileDialog::new()
                .set_file_name(format!("{}.json", chosen.name))
                .add_filter("JSON", &["json"])
                .save_file()
        {
            self.gear(dispatch, Gear::Export, None, Some(file));
        }
        if ui.button(t!("settings-inspections-import")).clicked()
            && let Some(file) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()
        {
            self.gear(dispatch, Gear::Import, None, Some(file));
        }
        ui.separator();
        if ui.button(t!("settings-inspections-restore")).clicked() {
            self.gear(dispatch, Gear::Restore, None, None);
        }
    }

    fn filters(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let hint = t!("settings-inspections-filter");
        let field = egui::TextEdit::singleline(&mut self.filter)
            .id_salt("settings-inspections-filter")
            .hint_text(hint.as_str())
            .desired_width(ui.available_width());
        accessibility::labelled(&ui.add(field), &hint);
        ui.horizontal_wrapped(|ui| {
            widgets::checkbox(ui, p, &mut self.only_modified, &t!("settings-inspections-only-modified"), query);
            widgets::checkbox(ui, p, &mut self.only_cleanup, &t!("settings-inspections-only-cleanup"), query);
            let current = self.only_severity.map_or_else(|| t!("settings-inspections-any-severity"), severity_title);
            let combo = egui::ComboBox::from_id_salt("settings-inspections-severity-filter")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.only_severity, None, t!("settings-inspections-any-severity"));
                    for severity in SEVERITIES {
                        ui.selectable_value(&mut self.only_severity, Some(severity), severity_title(severity));
                    }
                });
            accessibility::name(&combo.response, egui::Role::ComboBox, &t!("settings-inspections-severity-filter"));
        });
    }

    fn spelling_rows(&mut self, ui: &mut Ui, p: &IdePalette, spelling: &mut SpellingSettings, query: Option<&str>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("⌄").color(p.text_secondary));
            widgets::checkbox(ui, p, &mut spelling.enabled, &t!("settings-inspections-proofreading"), query);
        });
        ui.horizontal(|ui| {
            ui.add_space(ROW_INDENT);
            let fill = if self.selected == Selected::Spelling { p.selection } else { egui::Color32::TRANSPARENT };
            let row = egui::Frame::new().fill(fill).corner_radius(3).inner_margin(egui::Margin::symmetric(4, 1));
            row.show(ui, |ui| {
                let name = t!("settings-inspections-spelling");
                accessibility::labelled(&ui.checkbox(&mut spelling.enabled, ""), &name);
                let job = widgets::label_job(ui, p, &name, query);
                if ui.add(egui::Label::new(job).sense(Sense::click())).clicked() {
                    self.selected = Selected::Spelling;
                }
            });
        });
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, spelling: &mut SpellingSettings, query: Option<&str>) {
        self.spelling_rows(ui, p, spelling, query);
        let rows: Vec<ProfileInspection> = self.rows().into_iter().cloned().collect();
        let mut groups: BTreeMap<String, Vec<ProfileInspection>> = BTreeMap::new();
        for row in rows {
            groups.entry(row.group.clone()).or_default().push(row);
        }
        for (group, rows) in groups {
            let collapsed = self.collapsed.contains(&group);
            ui.horizontal(|ui| {
                let arrow = ui.add(
                    egui::Label::new(egui::RichText::new(if collapsed { "›" } else { "⌄" }).color(p.text_secondary))
                        .sense(Sense::click()),
                );
                if arrow.clicked() && !self.collapsed.remove(&group) {
                    self.collapsed.insert(group.clone());
                }
                accessibility::button_with(&arrow, || group.clone());
                let on = rows.iter().filter(|row| row.enabled).count();
                let mut all = on == rows.len();
                let checkbox = egui::Checkbox::new(&mut all, group.as_str()).indeterminate(on > 0 && on < rows.len());
                if ui.add(checkbox).changed() {
                    for row in &rows {
                        self.change(&row.id, Some(all), None);
                    }
                }
            });
            if collapsed {
                continue;
            }
            for row in rows {
                self.row(ui, p, &row);
            }
        }
    }

    fn row(&mut self, ui: &mut Ui, p: &IdePalette, row: &ProfileInspection) {
        ui.horizontal(|ui| {
            ui.add_space(ROW_INDENT);
            let selected = self.selected == Selected::Inspection(row.id.clone());
            let fill = if selected { p.selection } else { egui::Color32::TRANSPARENT };
            let frame = egui::Frame::new().fill(fill).corner_radius(3).inner_margin(egui::Margin::symmetric(4, 1));
            frame.show(ui, |ui| {
                let mut enabled = row.enabled;
                let checkbox = ui.checkbox(&mut enabled, "");
                accessibility::labelled(&checkbox, &row.title);
                if checkbox.changed() {
                    self.change(&row.id, Some(enabled), None);
                }
                let color = if row.modified { p.link } else { p.text };
                let title = egui::RichText::new(row.title.as_str()).color(color);
                if ui.add(egui::Label::new(title).sense(Sense::click())).clicked() {
                    self.selected = Selected::Inspection(row.id.clone());
                }
            });
        });
    }

    fn details(&mut self, ui: &mut Ui, p: &IdePalette, spelling: &mut SpellingSettings, query: Option<&str>) {
        let chosen = match &self.selected {
            Selected::Spelling => None,
            Selected::Inspection(id) => {
                self.view.as_ref().and_then(|view| view.inspections.iter().find(|row| row.id == *id)).cloned()
            }
        };
        let Some(row) = chosen else { return spelling_details(ui, p, spelling, query) };
        ui.label(egui::RichText::new(row.title.as_str()).font(theme::semibold(13.0)).color(p.text));
        ui.add_space(4.0);
        ui.add(egui::Label::new(egui::RichText::new(row.description.as_str()).color(p.text_secondary)).wrap());
        ui.add_space(12.0);
        let mut severity = row.severity;
        ui.add_enabled_ui(row.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label(t!("settings-inspections-severity"));
                let current = severity.map_or_else(|| t!("settings-inspections-as-reported"), severity_title);
                let combo = egui::ComboBox::from_id_salt("settings-inspections-severity")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut severity, None, t!("settings-inspections-as-reported"));
                        for option in SEVERITIES {
                            ui.selectable_value(&mut severity, Some(option), severity_title(option));
                        }
                    });
                accessibility::name(&combo.response, egui::Role::ComboBox, &t!("settings-inspections-severity"));
            });
        });
        if severity != row.severity {
            self.change(&row.id, None, Some(severity));
        }
        if row.cleanup {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t!("settings-inspections-is-cleanup")).color(p.text_secondary));
        }
    }
}

fn spelling_details(ui: &mut Ui, p: &IdePalette, spelling: &mut SpellingSettings, query: Option<&str>) {
    let name = t!("settings-inspections-spelling");
    ui.label(egui::RichText::new(name).font(theme::semibold(13.0)).color(p.text));
    ui.add_space(4.0);
    let description = egui::RichText::new(t!("settings-inspections-description")).color(p.text_secondary);
    ui.add(egui::Label::new(description).wrap());
    ui.add_space(12.0);
    ui.add_enabled_ui(spelling.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label(widgets::label_job(ui, p, &t!("settings-inspections-severity"), query));
            let (mark, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            ui.painter().circle_filled(mark.center(), 4.5, p.diagnostic_typo);
            ui.label(widgets::label_job(ui, p, &t!("settings-inspections-typo"), query));
        });
        ui.add_space(12.0);
        widgets::group_title(ui, p, &t!("settings-inspections-options"), query);
        widgets::checkbox(ui, p, &mut spelling.process_code, &t!("settings-inspections-process-code"), query);
        ui.add_space(4.0);
        widgets::checkbox(ui, p, &mut spelling.process_literals, &t!("settings-inspections-process-literals"), query);
        ui.add_space(4.0);
        widgets::checkbox(ui, p, &mut spelling.process_comments, &t!("settings-inspections-process-comments"), query);
    });
}

impl SettingsDialog {
    pub(super) fn inspections_page(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        query: Option<&str>,
        mut dispatch: Option<&mut Dispatch<'_>>,
    ) {
        let Some(draft) = self.draft.as_mut() else { return };
        let page = &mut self.inspections;
        let spelling = &mut draft.spelling;
        if !page.loaded
            && page.message.is_none()
            && let Some(dispatch) = dispatch.as_deref_mut()
        {
            page.load(dispatch, None);
        }
        page.top(ui, p, dispatch);
        if let Some(message) = &page.message {
            ui.add(egui::Label::new(egui::RichText::new(message.as_str()).color(p.error_text)).wrap());
        }
        ui.add_space(6.0);
        let side_by_side = ui.available_width() >= TREE_WIDTH * 2.0;
        let tree = |ui: &mut Ui, page: &mut InspectionsPage, spelling: &mut SpellingSettings| {
            let frame = egui::Frame::new()
                .stroke(egui::Stroke::new(1.0, p.text_field_border))
                .inner_margin(6.0)
                .corner_radius(4);
            frame.show(ui, |ui| {
                ui.vertical(|ui| {
                    if side_by_side {
                        ui.set_width(TREE_WIDTH);
                    }
                    page.filters(ui, p, query);
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().id_salt("settings-inspections-tree").max_height(TREE_HEIGHT).show(
                        ui,
                        |ui| {
                            ui.set_min_height(TREE_HEIGHT);
                            page.tree(ui, p, spelling, query);
                        },
                    );
                });
            });
        };
        if side_by_side {
            ui.horizontal_top(|ui| {
                tree(ui, page, spelling);
                ui.add_space(12.0);
                ui.vertical(|ui| page.details(ui, p, spelling, query));
            });
        } else {
            tree(ui, page, spelling);
            ui.add_space(12.0);
            page.details(ui, p, spelling, query);
        }
    }
}
