use std::path::PathBuf;

use egui::Ui;
use serde_json::json;
use zen_i18n::t;
use zen_project::settings::{FileEncoding, NewFileBom};
use zen_workbench::file_encodings::EncodingSettings;

use super::{Dispatch, widgets};
use crate::accessibility;
use crate::ide::IdePalette;

#[derive(Default)]
pub(in crate::ide) struct FileEncodingsPage {
    pub(in crate::ide) has_project: bool,
    original: Option<EncodingSettings>,
    pub(in crate::ide) draft: EncodingSettings,
    charsets: Vec<String>,
    root: PathBuf,
    new_path: String,
    message: Option<String>,
}

fn bom_name(bom: NewFileBom) -> String {
    match bom {
        NewFileBom::With => t!("settings-file-encodings-with-bom"),
        NewFileBom::Without => t!("settings-file-encodings-no-bom"),
        NewFileBom::OnWindows => t!("settings-file-encodings-windows-bom"),
    }
}

fn charset_combo(ui: &mut Ui, id: &str, name: &str, value: &mut String, charsets: &[String]) {
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(value.clone())
        .width(widgets::field_width(ui, 140.0, 220.0))
        .show_ui(ui, |ui| {
            for charset in charsets {
                ui.selectable_value(value, charset.clone(), charset);
            }
        });
    accessibility::name(&combo.response, egui::Role::ComboBox, name);
}

fn optional_combo(ui: &mut Ui, id: &str, name: &str, value: &mut Option<String>, default: &str, charsets: &[String]) {
    let shown = value.clone().unwrap_or_else(|| default.to_owned());
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(widgets::field_width(ui, 180.0, 260.0))
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, default);
            for charset in charsets {
                ui.selectable_value(value, Some(charset.clone()), charset);
            }
        });
    accessibility::name(&combo.response, egui::Role::ComboBox, name);
}

impl FileEncodingsPage {
    pub(in crate::ide) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        *self = Self::default();
        self.charsets = dispatch("file.encoding.list", json!({}))
            .ok()
            .and_then(|answer| serde_json::from_value::<Vec<String>>(answer["charsets"].clone()).ok())
            .unwrap_or_default();
        self.root = dispatch("project.structure.get", json!({}))
            .ok()
            .and_then(|answer| answer["root"].as_str().map(PathBuf::from))
            .unwrap_or_default();
        self.has_project = !self.root.as_os_str().is_empty();
        match dispatch("project.encodings.get", json!({}))
            .and_then(|answer| serde_json::from_value::<EncodingSettings>(answer).map_err(|error| error.to_string()))
        {
            Ok(settings) => {
                self.draft = settings.clone();
                self.original = Some(settings);
            }
            Err(error) => self.message = Some(error),
        }
    }

    pub(in crate::ide) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        let Some(original) = &self.original else { return };
        let changed = original.project != self.draft.project
            || original.files != self.draft.files
            || original.bom_for_new_files != self.draft.bom_for_new_files;
        if !changed || !self.has_project {
            return;
        }
        let params = json!({
            "project": self.draft.project,
            "files": self.draft.files,
            "bomForNewFiles": self.draft.bom_for_new_files,
        });
        if let Err(error) = dispatch("project.encodings.set", params) {
            self.message = Some(error);
        }
    }

    pub(in crate::ide) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(in crate::ide) fn show(&mut self, ui: &mut Ui, p: &IdePalette, global: &mut Option<String>) {
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.error_text));
            ui.add_space(6.0);
        }
        let global_name = t!("settings-file-encodings-global");
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&global_name).color(p.text));
            optional_combo(ui, "settings-global-encoding", &global_name, global, "UTF-8", &self.charsets);
        });
        ui.add_space(6.0);
        if !self.has_project {
            ui.label(egui::RichText::new(t!("settings-file-encodings-no-project")).color(p.text_secondary));
            return;
        }
        let project_name = t!("settings-file-encodings-project");
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&project_name).color(p.text));
            let inherited = t!("settings-file-encodings-inherited", charset = global.as_deref().unwrap_or("UTF-8"));
            let value = &mut self.draft.project;
            optional_combo(ui, "settings-project-encoding", &project_name, value, &inherited, &self.charsets);
        });
        ui.add_space(12.0);
        widgets::group_title(ui, p, &t!("settings-file-encodings-table"), None);
        self.table(ui, p);
        ui.add_space(4.0);
        ui.label(egui::RichText::new(t!("settings-file-encodings-note")).color(p.text_secondary));
        ui.add_space(12.0);
        let bom_title = t!("settings-file-encodings-create");
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&bom_title).color(p.text));
            let current = self.draft.bom_for_new_files.unwrap_or_default();
            let mut chosen = current;
            let combo = egui::ComboBox::from_id_salt("settings-new-file-bom")
                .selected_text(bom_name(current))
                .width(widgets::field_width(ui, 200.0, 320.0))
                .show_ui(ui, |ui| {
                    for choice in [NewFileBom::With, NewFileBom::Without, NewFileBom::OnWindows] {
                        ui.selectable_value(&mut chosen, choice, bom_name(choice));
                    }
                });
            accessibility::name(&combo.response, egui::Role::ComboBox, &bom_title);
            if chosen != current {
                self.draft.bom_for_new_files = Some(chosen);
            }
        });
    }

    fn table(&mut self, ui: &mut Ui, p: &IdePalette) {
        if self.draft.files.is_empty() {
            ui.label(egui::RichText::new(t!("settings-file-encodings-empty")).color(p.text_secondary));
        }
        let root = self.root.clone();
        let mut removed = None;
        for (index, file) in self.draft.files.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let shown = file.path.strip_prefix(&root).unwrap_or(&file.path).display().to_string();
                ui.label(egui::RichText::new(&shown).color(p.text));
                let id = format!("settings-encoding-row-{index}");
                charset_combo(ui, &id, &shown, &mut file.charset, &self.charsets);
                if ui.button(t!("settings-file-encodings-remove")).clicked() {
                    removed = Some(index);
                }
            });
        }
        if let Some(index) = removed {
            self.draft.files.remove(index);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.new_path)
                    .desired_width(widgets::field_width(ui, 180.0, 320.0))
                    .hint_text(t!("settings-file-encodings-path")),
            );
            accessibility::name(&field, egui::Role::TextInput, &t!("settings-file-encodings-path"));
            let name = self.new_path.trim().to_owned();
            if ui.add_enabled(!name.is_empty(), egui::Button::new(t!("settings-file-encodings-add"))).clicked() {
                let path = root.join(&name);
                self.draft.files.retain(|file| file.path != path);
                self.draft.files.push(FileEncoding { path, charset: "UTF-8".to_owned() });
                self.draft.files.sort_by(|left, right| left.path.cmp(&right.path));
                self.new_path.clear();
            }
        });
    }
}
