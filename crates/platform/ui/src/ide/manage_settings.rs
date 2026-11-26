use std::path::{Path, PathBuf};

use egui::{Color32, Key, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_workbench::ide_settings::{
    Component, ComponentEntries, ComponentsReport, ImportReport, Inspection, RestoreReport, SourceKind,
};
use zen_workbench::notifications::{Group, Kind, NewNotification};

use super::settings::Dispatch;
use super::{IdePalette, IdeView, Request, dialog_frame};
use crate::theme;

#[derive(Debug, Default)]
pub(super) struct ManageSettings {
    export: Option<ExportDialog>,
    import: Option<ImportDialog>,
    restore: Option<RestoreDialog>,
}

#[derive(Debug)]
struct ExportDialog {
    components: Vec<(ComponentEntries, bool)>,
    path: String,
    error: Option<String>,
}

#[derive(Debug)]
enum ImportStep {
    Source { path: String },
    Components { inspection: Inspection, chosen: Vec<bool> },
    Done { report: ImportReport },
}

#[derive(Debug)]
pub(super) struct ImportDialog {
    step: ImportStep,
    backups: Option<PathBuf>,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImportEnd {
    Closed,
    Restart,
}

impl ImportDialog {
    pub(super) fn open(dispatch: &mut Dispatch<'_>, params: &Value) -> Result<Self, String> {
        let value = dispatch("settings.ide.components", json!({}))?;
        let report: ComponentsReport = serde_json::from_value(value).map_err(|error| error.to_string())?;
        let mut dialog =
            Self { step: ImportStep::Source { path: String::new() }, backups: Some(report.backups), error: None };
        if let Some(path) = params.get("path").and_then(Value::as_str) {
            dialog.step = components_step(inspect(dispatch, &expand(path))?);
        }
        Ok(dialog)
    }

    pub(super) fn name(&self) -> &'static str {
        match self.step {
            ImportStep::Source { .. } => "importSettings",
            ImportStep::Components { .. } => "selectComponentsToImport",
            ImportStep::Done { .. } => "settingsImported",
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        product: &str,
        dispatch: &mut Dispatch<'_>,
    ) -> Option<ImportEnd> {
        match show_import(ui, p, product, self)? {
            ImportChoice::Cancel => return Some(ImportEnd::Closed),
            ImportChoice::Restart => return Some(ImportEnd::Restart),
            ImportChoice::Inspect(path) => match inspect(dispatch, &path) {
                Ok(inspection) => {
                    self.step = components_step(inspection);
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            },
            ImportChoice::Import(path, components) => {
                let params = json!({ "path": path, "components": components });
                let answer = dispatch("settings.ide.import", params)
                    .and_then(|value| serde_json::from_value::<ImportReport>(value).map_err(|error| error.to_string()));
                match answer {
                    Ok(report) => {
                        self.step = ImportStep::Done { report };
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        None
    }
}

#[derive(Debug, Default)]
struct RestoreDialog {
    error: Option<String>,
}

impl ManageSettings {
    pub(super) fn person_answers(&self) -> bool {
        self.restore.is_some() || self.import.is_some()
    }

    pub(super) fn open_dialogs(&self) -> Vec<&'static str> {
        let mut open = Vec::new();
        if self.export.is_some() {
            open.push("exportSettings");
        }
        if let Some(dialog) = &self.import {
            open.push(dialog.name());
        }
        if self.restore.is_some() {
            open.push("restoreDefaultSettings");
        }
        open
    }
}

fn component_name(id: Component) -> String {
    match id {
        Component::Settings => t!("ide-settings-component-settings"),
        Component::Keymaps => t!("ide-settings-component-keymaps"),
        Component::Themes => t!("ide-settings-component-themes"),
        Component::Templates => t!("ide-settings-component-templates"),
        Component::Plugins => t!("ide-settings-component-plugins"),
    }
}

fn counted(entries: &ComponentEntries) -> String {
    t!("ide-settings-component-count", name = component_name(entries.id), count = entries.entries.len())
}

fn failure(error: DispatchError) -> String {
    match error {
        DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

fn expand(path: &str) -> PathBuf {
    let path = path.trim();
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

fn default_export_path() -> String {
    std::env::var_os("HOME").map_or_else(
        || "settings.zip".to_owned(),
        |home| PathBuf::from(home).join("settings.zip").display().to_string(),
    )
}

fn parse_components(params: &Value) -> Result<Option<Vec<Component>>, String> {
    match params.get("components") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone()).map(Some).map_err(|error| error.to_string()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_export_file(current: &str) -> Option<PathBuf> {
    let current = expand(current);
    let mut dialog = rfd::FileDialog::new().set_title(t!("ide-settings-export-title")).add_filter("ZIP", &["zip"]);
    if let Some(folder) = current.parent().filter(|folder| folder.is_dir()) {
        dialog = dialog.set_directory(folder);
    }
    if let Some(name) = current.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    dialog.save_file()
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_import_source(folder: bool, backups: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(t!("ide-settings-import-title"));
    if folder {
        if let Some(backups) = backups.filter(|backups| backups.is_dir()) {
            dialog = dialog.set_directory(backups);
        }
        dialog.pick_folder()
    } else {
        dialog.add_filter(t!("ide-settings-import-files"), &["zip", "xml", "icls", "json"]).pick_file()
    }
}

#[cfg(target_arch = "wasm32")]
fn pick_export_file(_: &str) -> Option<PathBuf> {
    None
}

#[cfg(target_arch = "wasm32")]
fn pick_import_source(_: bool, _: Option<&Path>) -> Option<PathBuf> {
    None
}

impl IdeView {
    fn settings_components(&self, registry: &Registry, session: &mut Session) -> Result<ComponentsReport, String> {
        let value = registry.dispatch(session, "settings.ide.components", json!({})).map_err(failure)?;
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    pub(super) fn open_export_settings(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
    ) -> Result<(), String> {
        let report = self.settings_components(registry, session)?;
        let chosen = parse_components(params)?;
        let components = report
            .components
            .into_iter()
            .map(|entries| {
                let on = chosen.as_ref().is_none_or(|chosen| chosen.contains(&entries.id));
                (entries, on)
            })
            .collect();
        let path = params.get("path").and_then(Value::as_str).map_or_else(default_export_path, str::to_owned);
        self.manage_settings.export = Some(ExportDialog { components, path, error: None });
        Ok(())
    }

    pub(super) fn open_import_settings(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
    ) -> Result<(), String> {
        let mut dispatch = |id: &str, params| registry.dispatch(session, id, params).map_err(failure);
        self.manage_settings.import = Some(ImportDialog::open(&mut dispatch, params)?);
        Ok(())
    }

    pub(super) fn open_restore_settings(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
        agent: bool,
    ) -> Result<Value, String> {
        if !agent && params.get("confirmed").and_then(Value::as_bool).unwrap_or(false) {
            return self.restore_settings(registry, session);
        }
        self.settings_components(registry, session)?;
        self.manage_settings.restore = Some(RestoreDialog::default());
        Ok(Value::Null)
    }

    fn restore_settings(&mut self, registry: &Registry, session: &mut Session) -> Result<Value, String> {
        let value = registry.dispatch(session, "settings.ide.restoreDefaults", json!({})).map_err(failure)?;
        if let Ok(report) = serde_json::from_value::<RestoreReport>(value.clone()) {
            let body = t!("ide-settings-restore-done", path = report.backup.display().to_string());
            self.notify(
                NewNotification::message(Group::General, Kind::Info, body)
                    .titled_by(zen_i18n::Message::new("ide-settings-restore-title")),
            );
        }
        self.requests.push(Request::Restart);
        Ok(value)
    }

    pub(super) fn manage_settings_dialogs(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        if let Some(dialog) = &mut self.manage_settings.export {
            match show_export(ui, &p, dialog) {
                Some(true) => {
                    let components: Vec<Component> =
                        dialog.components.iter().filter(|(_, on)| *on).map(|(entries, _)| entries.id).collect();
                    let params = json!({ "path": expand(&dialog.path), "components": components });
                    match registry.dispatch(session, "settings.ide.export", params) {
                        Ok(value) => {
                            self.manage_settings.export = None;
                            let path = value["path"].as_str().unwrap_or_default().to_owned();
                            let body = t!("ide-settings-export-done", path = path);
                            self.notify(
                                NewNotification::message(Group::General, Kind::Info, body)
                                    .titled_by(zen_i18n::Message::new("ide-settings-export-title")),
                            );
                        }
                        Err(error) => dialog.error = Some(failure(error)),
                    }
                }
                Some(false) => self.manage_settings.export = None,
                None => {}
            }
        }
        if let Some(dialog) = &mut self.manage_settings.import {
            let mut dispatch = |id: &str, params| registry.dispatch(session, id, params).map_err(failure);
            match dialog.show(ui, &p, self.product.name, &mut dispatch) {
                Some(ImportEnd::Closed) => self.manage_settings.import = None,
                Some(ImportEnd::Restart) => {
                    self.manage_settings.import = None;
                    self.requests.push(Request::Restart);
                }
                None => {}
            }
        }
        if let Some(dialog) = &mut self.manage_settings.restore {
            match show_restore(ui, &p, self.product.name, dialog) {
                Some(true) => match self.restore_settings(registry, session) {
                    Ok(_) => self.manage_settings.restore = None,
                    Err(error) => {
                        if let Some(dialog) = &mut self.manage_settings.restore {
                            dialog.error = Some(error);
                        }
                    }
                },
                Some(false) => self.manage_settings.restore = None,
                None => {}
            }
        }
    }
}

fn inspect(dispatch: &mut Dispatch<'_>, path: &Path) -> Result<Inspection, String> {
    let value = dispatch("settings.ide.inspect", json!({ "path": path }))?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn components_step(inspection: Inspection) -> ImportStep {
    let chosen = vec![true; inspection.components.len()];
    ImportStep::Components { inspection, chosen }
}

enum ImportChoice {
    Cancel,
    Inspect(PathBuf),
    Import(PathBuf, Vec<Component>),
    Restart,
}

fn text(text: String, color: Color32) -> egui::RichText {
    egui::RichText::new(text).font(theme::regular(13.0)).color(color)
}

fn title(ui: &mut Ui, p: &IdePalette, title: String) {
    ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
    ui.add_space(4.0);
}

fn primary(ui: &mut Ui, p: &IdePalette, label: String) -> bool {
    let button = egui::Button::new(egui::RichText::new(label).font(theme::regular(13.0)).color(p.accent_text))
        .min_size(vec2(84.0, 30.0))
        .corner_radius(6)
        .fill(p.accent);
    ui.add(button).clicked()
}

fn secondary(ui: &mut Ui, p: &IdePalette, label: String) -> bool {
    let button = egui::Button::new(egui::RichText::new(label).font(theme::regular(13.0)).color(p.text))
        .min_size(vec2(84.0, 30.0))
        .corner_radius(6)
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, p.button_border));
    ui.add(button).clicked()
}

fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(14.0);
    let layout = egui::Layout::right_to_left(egui::Align::Center);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), layout, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        add(ui);
    });
}

fn error_line(ui: &mut Ui, p: &IdePalette, error: Option<&String>) {
    if let Some(error) = error {
        ui.add(egui::Label::new(text(error.clone(), p.error_text)).wrap());
    }
}

fn enter(ui: &mut Ui) -> bool {
    ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter))
}

fn skipped_list(ui: &mut Ui, p: &IdePalette, skipped: &[zen_workbench::ide_settings::Skipped]) {
    if skipped.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(text(t!("ide-settings-import-skipped", count = skipped.len()), p.text));
    egui::Frame::new().fill(p.frame).corner_radius(6).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt("settings-import-skipped").max_height(140.0).show(ui, |ui| {
            for skipped in skipped {
                ui.add(
                    egui::Label::new(text(
                        t!(
                            "ide-settings-import-skipped-entry",
                            entry = skipped.entry.as_str(),
                            reason = skipped.reason.as_str()
                        ),
                        p.text_secondary,
                    ))
                    .wrap(),
                );
            }
        });
    });
}

fn show_export(ui: &mut Ui, p: &IdePalette, dialog: &mut ExportDialog) -> Option<bool> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("export-settings")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(480.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        title(ui, p, t!("ide-settings-export-title"));
        let mut all = dialog.components.iter().all(|(_, on)| *on);
        if ui.checkbox(&mut all, text(t!("ide-settings-select-all"), p.text)).changed() {
            for (_, on) in &mut dialog.components {
                *on = all;
            }
        }
        egui::Frame::new().fill(p.frame).corner_radius(6).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (entries, on) in &mut dialog.components {
                ui.horizontal(|ui| {
                    ui.checkbox(on, text(component_name(entries.id), p.text));
                    let files = t!("ide-settings-component-files", count = entries.entries.len());
                    ui.label(text(files, p.text_secondary));
                });
            }
        });
        ui.add_space(6.0);
        ui.label(text(t!("ide-settings-export-path"), p.text));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut dialog.path).desired_width(400.0));
            if ui.button("…").clicked()
                && let Some(path) = pick_export_file(&dialog.path)
            {
                dialog.path = path.display().to_string();
            }
        });
        error_line(ui, p, dialog.error.as_ref());
        let any = dialog.components.iter().any(|(_, on)| *on);
        buttons(ui, |ui| {
            if ui.add_enabled_ui(any, |ui| primary(ui, p, t!("settings-ok"))).inner {
                choice = Some(true);
            }
            if secondary(ui, p, t!("settings-cancel")) {
                choice = Some(false);
            }
        });
        if any && enter(ui) {
            choice = Some(true);
        }
    });
    choice.or_else(|| modal.should_close().then_some(false))
}

fn kind_text(inspection: &Inspection) -> String {
    let path = inspection.path.display().to_string();
    match inspection.kind {
        SourceKind::Zen => t!(
            "ide-settings-import-kind-zen",
            path = path,
            product = inspection.product.clone().unwrap_or_else(|| "Zen IDE".to_owned())
        ),
        SourceKind::Backup => t!("ide-settings-import-kind-backup", path = path),
        SourceKind::JetBrains => t!("ide-settings-import-kind-jetbrains", path = path),
        SourceKind::File => t!("ide-settings-import-kind-file", path = path),
    }
}

fn show_import(ui: &mut Ui, p: &IdePalette, product: &str, dialog: &mut ImportDialog) -> Option<ImportChoice> {
    let mut choice = None;
    let step = match dialog.step {
        ImportStep::Source { .. } => "import-settings",
        ImportStep::Components { .. } => "select-components-to-import",
        ImportStep::Done { .. } => "settings-imported",
    };
    let modal = egui::Modal::new(ui.scope_id().with(step)).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        match &mut dialog.step {
            ImportStep::Source { path } => {
                title(ui, p, t!("ide-settings-import-title"));
                ui.add(egui::Label::new(text(t!("ide-settings-import-intro"), p.text_secondary)).wrap());
                ui.label(text(t!("ide-settings-import-path"), p.text));
                ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if ui.button(t!("ide-settings-import-choose-file")).clicked()
                        && let Some(chosen) = pick_import_source(false, None)
                    {
                        *path = chosen.display().to_string();
                    }
                    if ui.button(t!("ide-settings-import-choose-folder")).clicked()
                        && let Some(chosen) = pick_import_source(true, dialog.backups.as_deref())
                    {
                        *path = chosen.display().to_string();
                    }
                });
                error_line(ui, p, dialog.error.as_ref());
                let ready = !path.trim().is_empty();
                buttons(ui, |ui| {
                    if ui.add_enabled_ui(ready, |ui| primary(ui, p, t!("ide-settings-import-next"))).inner {
                        choice = Some(ImportChoice::Inspect(expand(path)));
                    }
                    if secondary(ui, p, t!("settings-cancel")) {
                        choice = Some(ImportChoice::Cancel);
                    }
                });
                if ready && enter(ui) {
                    choice = Some(ImportChoice::Inspect(expand(path)));
                }
            }
            ImportStep::Components { inspection, chosen } => {
                title(ui, p, t!("ide-settings-import-components-title"));
                ui.add(egui::Label::new(text(kind_text(inspection), p.text_secondary)).wrap());
                let mut all = chosen.iter().all(|on| *on);
                if ui.checkbox(&mut all, text(t!("ide-settings-select-all"), p.text)).changed() {
                    chosen.iter_mut().for_each(|on| *on = all);
                }
                egui::Frame::new().fill(p.frame).corner_radius(6).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (entries, on) in inspection.components.iter().zip(chosen.iter_mut()) {
                        ui.horizontal(|ui| {
                            ui.checkbox(on, text(component_name(entries.id), p.text));
                            let files = t!("ide-settings-component-files", count = entries.entries.len());
                            ui.label(text(files, p.text_secondary));
                        });
                    }
                });
                skipped_list(ui, p, &inspection.skipped);
                error_line(ui, p, dialog.error.as_ref());
                let picked: Vec<Component> = inspection
                    .components
                    .iter()
                    .zip(chosen.iter())
                    .filter(|(_, on)| **on)
                    .map(|(entries, _)| entries.id)
                    .collect();
                let ready = !picked.is_empty();
                buttons(ui, |ui| {
                    if ui.add_enabled_ui(ready, |ui| primary(ui, p, t!("settings-ok"))).inner {
                        choice = Some(ImportChoice::Import(inspection.path.clone(), picked.clone()));
                    }
                    if secondary(ui, p, t!("settings-cancel")) {
                        choice = Some(ImportChoice::Cancel);
                    }
                });
                if ready && enter(ui) {
                    choice = Some(ImportChoice::Import(inspection.path.clone(), picked));
                }
            }
            ImportStep::Done { report } => {
                title(ui, p, t!("ide-settings-import-done-title"));
                let imported: Vec<String> = report.imported.iter().map(counted).collect();
                let summary = if imported.is_empty() {
                    t!("ide-settings-import-nothing")
                } else {
                    t!("ide-settings-import-imported", what = imported.join(", "))
                };
                ui.add(egui::Label::new(text(summary, p.text)).wrap());
                skipped_list(ui, p, &report.skipped);
                if report.restart_required {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(text(t!("ide-settings-import-restart-text", product = product), p.text))
                            .wrap(),
                    );
                    buttons(ui, |ui| {
                        if primary(ui, p, t!("ide-settings-import-restart")) {
                            choice = Some(ImportChoice::Restart);
                        }
                        if secondary(ui, p, t!("ide-settings-import-later")) {
                            choice = Some(ImportChoice::Cancel);
                        }
                    });
                    if enter(ui) {
                        choice = Some(ImportChoice::Restart);
                    }
                } else {
                    buttons(ui, |ui| {
                        if primary(ui, p, t!("settings-ok")) {
                            choice = Some(ImportChoice::Cancel);
                        }
                    });
                    if enter(ui) {
                        choice = Some(ImportChoice::Cancel);
                    }
                }
            }
        }
    });
    choice.or_else(|| modal.should_close().then_some(ImportChoice::Cancel))
}

fn show_restore(ui: &mut Ui, p: &IdePalette, product: &str, dialog: &mut RestoreDialog) -> Option<bool> {
    let mut choice = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("restore-default-settings")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            title(ui, p, t!("ide-settings-restore-title"));
            ui.add(egui::Label::new(text(t!("ide-settings-restore-text", product = product), p.text)).wrap());
            error_line(ui, p, dialog.error.as_ref());
            buttons(ui, |ui| {
                if primary(ui, p, t!("ide-settings-restore-ok")) {
                    choice = Some(true);
                }
                if secondary(ui, p, t!("settings-cancel")) {
                    choice = Some(false);
                }
            });
            if enter(ui) {
                choice = Some(true);
            }
        });
    choice.or_else(|| modal.should_close().then_some(false))
}

#[cfg(test)]
mod tests;
