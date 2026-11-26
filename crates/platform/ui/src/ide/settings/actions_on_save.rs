use egui::Ui;
use serde_json::json;
use zen_i18n::t;
use zen_workbench::actions_on_save::{ActionsOnSave, ReformatScope};

use super::model::IdeSettings;
use super::page::PageId;
use super::{Dispatch, SettingsDialog, widgets};
use crate::ide::IdePalette;

const ESLINT: &str = "eslint";
const STYLELINT: &str = "stylelint";

#[derive(Default)]
pub(super) struct OnSavePage {
    pub(super) for_new_projects: bool,
    project: Option<ActionsOnSave>,
    loaded: Option<ActionsOnSave>,
    pub(super) version_control: bool,
    pub(super) eslint: Option<String>,
    pub(super) stylelint: Option<String>,
}

fn setting_of(tool: &serde_json::Value, field: &str) -> Option<String> {
    tool[field][0].as_str().map(str::to_owned)
}

fn tool_key(list: &serde_json::Value, group: &str, id: &str, field: &str) -> Option<String> {
    list[group].as_array()?.iter().find(|tool| tool["id"] == id).and_then(|tool| setting_of(tool, field))
}

fn glob_of(language: &str) -> String {
    zen_workbench::code_style::globs_of(language).first().cloned().unwrap_or_else(|| language.to_owned())
}

fn files_summary(languages: &[String]) -> String {
    if languages.is_empty() {
        return t!("settings-on-save-all-files");
    }
    languages.iter().map(|language| glob_of(language)).collect::<Vec<_>>().join(", ")
}

fn files_link(ui: &mut Ui, languages: &mut Vec<String>) -> bool {
    let name = t!("settings-on-save-files", files = files_summary(languages));
    let link = ui.link(name);
    let mut changed = false;
    crate::components::popup_menu(&link, |ui| {
        if crate::components::menu_item(ui, t!("settings-on-save-all-files")) {
            changed = !languages.is_empty();
            languages.clear();
            ui.close();
        }
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for language in zen_workbench::code_style::languages() {
                let mut on = languages.iter().any(|known| known == language);
                if ui.checkbox(&mut on, glob_of(language)).changed() {
                    if on {
                        languages.push(language.to_owned());
                        languages.sort_unstable();
                    } else {
                        languages.retain(|known| known != language);
                    }
                    changed = true;
                }
            }
        });
    });
    changed
}

fn scope_name(scope: ReformatScope) -> String {
    match scope {
        ReformatScope::WholeFile => t!("settings-on-save-whole-file"),
        ReformatScope::ChangedLines => t!("settings-on-save-changed-lines"),
    }
}

fn scope_link(ui: &mut Ui, scope: &mut ReformatScope) -> bool {
    let link = ui.link(scope_name(*scope));
    let mut changed = false;
    crate::components::popup_menu(&link, |ui| {
        for choice in [ReformatScope::WholeFile, ReformatScope::ChangedLines] {
            if crate::components::menu_item(ui, scope_name(choice)) {
                changed = *scope != choice;
                *scope = choice;
                ui.close();
            }
        }
    });
    changed
}

fn hint(ui: &mut Ui, p: &IdePalette, key: &str) {
    ui.add(egui::Label::new(egui::RichText::new(t!(key)).color(p.text_secondary)).wrap());
}

fn action_cell(ui: &mut Ui, p: &IdePalette, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
            ui.set_width(width);
            ui.vertical(add);
        });
        ui.label(egui::RichText::new(t!("settings-on-save-any-save")).color(p.text_secondary));
    });
    ui.add_space(10.0);
}

fn tool_row(
    ui: &mut Ui,
    p: &IdePalette,
    width: f32,
    draft: &mut IdeSettings,
    key: Option<&String>,
    (title, off_hint): (&str, &str),
    query: Option<&str>,
) {
    action_cell(ui, p, width, |ui| match key {
        Some(key) => {
            let mut on = draft.declared_settings.get(key).copied().unwrap_or(false);
            if widgets::checkbox(ui, p, &mut on, &t!(title), query).changed() {
                draft.declared_settings.insert(key.clone(), on);
            }
        }
        None => {
            ui.label(widgets::label_job(ui, p, &t!(title), query));
            hint(ui, p, off_hint);
        }
    });
}

impl OnSavePage {
    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        *self = Self { for_new_projects: self.for_new_projects, ..Self::default() };
        if let Ok(answer) = dispatch("project.actionsOnSave.get", json!({})) {
            self.version_control = answer["versionControl"] == true;
            if answer["project"] == true {
                self.project = serde_json::from_value(answer["actions"].clone()).ok();
                self.loaded = self.project.clone();
            }
        }
        if let Ok(tools) = dispatch("codeQuality.tools.list", json!({})) {
            self.eslint = tool_key(&tools, "linters", ESLINT, "fixOnSave");
            self.stylelint = tool_key(&tools, "formatters", STYLELINT, "onSave");
        }
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.project == self.loaded {
            return;
        }
        let Some(actions) = &self.project else { return };
        if dispatch("project.actionsOnSave.set", json!({ "actions": actions })).is_ok() {
            self.loaded = self.project.clone();
        }
    }

    fn edits_project(&self) -> bool {
        self.project.is_some() && !self.for_new_projects
    }

    pub(super) fn actions(&self, draft: &IdeSettings) -> ActionsOnSave {
        match self.project.as_ref().filter(|_| self.edits_project()) {
            Some(project) => project.clone(),
            None => draft.actions_on_save(),
        }
    }

    pub(super) fn set_actions(&mut self, draft: &mut IdeSettings, actions: ActionsOnSave) {
        if self.edits_project() {
            self.project = Some(actions);
        } else {
            draft.set_actions_on_save(actions);
        }
    }
}

impl SettingsDialog {
    pub(super) fn actions_on_save_page(&mut self, ui: &mut Ui, p: &IdePalette) {
        let query = self.search.query();
        let watchers = self.file_watchers.enabled_names();
        let mut open = None;
        let Some(draft) = self.draft.as_mut() else { return };
        let page = &mut self.on_save;
        if page.for_new_projects {
            hint(ui, p, "settings-on-save-new-projects");
            ui.add_space(8.0);
        }
        let mut actions = page.actions(draft);
        let mut changed = false;
        let width = (ui.available_width() - 140.0).max(160.0);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                    ui.set_width(width);
                    hint(ui, p, "settings-on-save-action");
                });
                hint(ui, p, "settings-on-save-activated-on");
            });
            ui.add_space(8.0);
            action_cell(ui, p, width, |ui| {
                changed |=
                    widgets::checkbox(ui, p, &mut actions.reformat, &t!("settings-on-save-reformat"), query).changed();
                hint(ui, p, "settings-on-save-reformat-hint");
                ui.horizontal(|ui| {
                    changed |= files_link(ui, &mut actions.reformat_files);
                    if page.version_control {
                        changed |= scope_link(ui, &mut actions.reformat_scope);
                    }
                });
            });
            action_cell(ui, p, width, |ui| {
                changed |= widgets::checkbox(
                    ui,
                    p,
                    &mut actions.optimize_imports,
                    &t!("settings-on-save-optimize-imports"),
                    query,
                )
                .changed();
                hint(ui, p, "settings-on-save-optimize-hint");
                changed |= files_link(ui, &mut actions.optimize_files);
            });
            ui.add_enabled_ui(false, |ui| {
                let mut off = false;
                action_cell(ui, p, width, |ui| {
                    widgets::checkbox(ui, p, &mut off, &t!("settings-on-save-rearrange"), query);
                    hint(ui, p, "settings-on-save-rearrange-hint");
                });
            });
            ui.add_enabled_ui(false, |ui| {
                let mut off = false;
                action_cell(ui, p, width, |ui| {
                    widgets::checkbox(ui, p, &mut off, &t!("settings-on-save-cleanup"), query);
                    hint(ui, p, "settings-on-save-cleanup-profile");
                    hint(ui, p, "settings-on-save-cleanup-hint");
                });
            });
            action_cell(ui, p, width, |ui| {
                ui.label(widgets::label_job(ui, p, &t!("settings-on-save-copyright"), query));
                hint(ui, p, "settings-on-save-copyright-hint");
            });
            tool_row(
                ui,
                p,
                width,
                draft,
                page.stylelint.as_ref(),
                ("settings-on-save-stylelint", "settings-on-save-stylelint-hint"),
                query,
            );
            tool_row(
                ui,
                p,
                width,
                draft,
                page.eslint.as_ref(),
                ("settings-on-save-eslint", "settings-on-save-eslint-hint"),
                query,
            );
            action_cell(ui, p, width, |ui| {
                ui.label(widgets::label_job(ui, p, &t!("settings-on-save-file-watcher"), query));
                ui.horizontal_wrapped(|ui| {
                    let state = if watchers.is_empty() {
                        t!("settings-on-save-file-watcher-none")
                    } else {
                        watchers.join(", ")
                    };
                    ui.label(egui::RichText::new(state).color(p.text_secondary));
                    if ui.link(t!("settings-on-save-file-watcher-configure")).clicked() {
                        open = Some(PageId::FileWatchers);
                    }
                });
            });
        });
        if changed {
            page.set_actions(draft, actions);
        }
        ui.add_space(14.0);
        if ui.link(t!("settings-on-save-code-quality")).clicked() {
            open = Some(PageId::Module(zen_workbench::code_quality::PAGE));
        }
        ui.add_space(4.0);
        if ui.link(t!("settings-on-save-autosave")).clicked() {
            open = Some(PageId::SystemSettings);
        }
        if let Some(page) = open {
            self.section = page;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_go_to_the_project_unless_the_dialog_is_for_new_projects() {
        let mut page = OnSavePage { project: Some(ActionsOnSave::default()), ..OnSavePage::default() };
        let mut draft = IdeSettings::default();
        let actions = ActionsOnSave { reformat: true, ..ActionsOnSave::default() };
        page.set_actions(&mut draft, actions.clone());
        assert!(!draft.reformat_on_save);
        assert_eq!(page.actions(&draft), actions);
        page.for_new_projects = true;
        page.set_actions(&mut draft, ActionsOnSave { optimize_imports: true, ..ActionsOnSave::default() });
        assert!(draft.optimize_imports_on_save);
        assert_eq!(page.project, Some(actions), "the project's stays");
    }

    #[test]
    fn without_a_project_the_defaults_are_edited() {
        let mut page = OnSavePage::default();
        let mut draft = IdeSettings::default();
        page.set_actions(&mut draft, ActionsOnSave { reformat: true, ..ActionsOnSave::default() });
        assert!(draft.reformat_on_save);
    }

    #[test]
    fn the_tools_come_from_the_settings_they_declare() {
        let list = json!({
            "linters": [{ "id": "eslint", "fixOnSave": ["eslint.fix", false] }, { "id": "other", "fixOnSave": null }],
            "formatters": [{ "id": "stylelint", "onSave": ["stylelint.fix", true] }],
        });
        assert_eq!(tool_key(&list, "linters", ESLINT, "fixOnSave").as_deref(), Some("eslint.fix"));
        assert_eq!(tool_key(&list, "formatters", STYLELINT, "onSave").as_deref(), Some("stylelint.fix"));
        assert_eq!(tool_key(&list, "linters", "other", "fixOnSave"), None);
    }

    #[test]
    fn a_language_shows_as_its_first_glob() {
        assert_eq!(files_summary(&[]), t!("settings-on-save-all-files"));
        assert_eq!(files_summary(&["go".to_owned()]), "*.go");
    }
}
