use std::path::{Path, PathBuf};

use egui::{Color32, Key, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::file_templates::{FileTemplate, TemplateInfo, TemplateKind};

use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

#[derive(Debug)]
pub(super) struct NewFromTemplate {
    folder: PathBuf,
    templates: Vec<TemplateInfo>,
    chosen: usize,
    pub(super) name: String,
    pub(super) values: Vec<(String, String)>,
    error: Option<String>,
    focus: bool,
    fixed: bool,
    kinds: bool,
}

impl NewFromTemplate {
    fn template(&self) -> Option<&TemplateInfo> {
        self.templates.get(self.chosen)
    }

    #[cfg(test)]
    pub(super) fn template_names(&self) -> Vec<String> {
        self.templates.iter().map(|info| info.template.name.clone()).collect()
    }

    pub(super) fn asks_name(&self) -> bool {
        self.template().is_some_and(|info| info.asked.iter().any(|name| name == "NAME"))
    }

    fn choose(&mut self, index: usize) {
        self.chosen = index.min(self.templates.len().saturating_sub(1));
        let kept = std::mem::take(&mut self.values);
        self.values = self
            .template()
            .map(|info| {
                info.asked
                    .iter()
                    .filter(|name| *name != "NAME")
                    .map(|name| {
                        let value = kept.iter().find(|(known, _)| known == name).map(|(_, value)| value.clone());
                        (name.clone(), value.unwrap_or_default())
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.error = None;
        self.focus = true;
    }

    pub(super) fn create_params(&self) -> Option<Value> {
        let info = self.template()?;
        let variables: serde_json::Map<String, Value> =
            self.values.iter().map(|(name, value)| (name.clone(), Value::String(value.clone()))).collect();
        Some(json!({
            "template": info.template.name,
            "directory": self.folder,
            "name": self.name.trim(),
            "variables": variables,
        }))
    }
}

#[derive(Debug)]
pub(super) struct SaveAsTemplate {
    pub(super) template: FileTemplate,
    project_scheme: bool,
    error: Option<String>,
    focus: bool,
}

enum Choice {
    Ok,
    Cancel,
}

fn dispatch_error(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

impl IdeView {
    fn template_folder(&self, session: &Session) -> Option<PathBuf> {
        let workbench = session.state::<Workbench>()?;
        let project = workbench.project()?.to_owned();
        let from_tree = self.selection.lead().map(|lead| {
            if lead.is_dir() { lead.clone() } else { lead.parent().map_or_else(|| project.clone(), Path::to_owned) }
        });
        let from_editor = workbench.active_document().and_then(|document| document.path().parent()).map(Path::to_owned);
        Some(
            from_tree
                .or(from_editor)
                .filter(|folder| zen_workbench::inside_project(&project, folder) && folder.is_dir())
                .unwrap_or(project),
        )
    }

    pub(super) fn open_new_from_template(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        template: Option<&str>,
        folder: Option<PathBuf>,
        fixed: bool,
    ) -> Result<(), String> {
        self.open_new_dialog(registry, session, template, None, folder, fixed)
    }

    pub(super) fn open_new_in_group(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        group: &str,
        folder: PathBuf,
    ) -> Result<(), String> {
        self.open_new_dialog(registry, session, None, Some(group), Some(folder), false)
    }

    fn open_new_dialog(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        template: Option<&str>,
        group: Option<&str>,
        folder: Option<PathBuf>,
        fixed: bool,
    ) -> Result<(), String> {
        let folder = match folder {
            Some(folder) => session.state::<Workbench>().map_or(folder.clone(), |workbench| workbench.resolve(&folder)),
            None => self.template_folder(session).ok_or_else(|| t!("ide-file-template-no-project"))?,
        };
        let listed =
            registry.dispatch(session, "file.templates.list", json!({ "kind": "file" })).map_err(dispatch_error)?;
        let templates: Vec<TemplateInfo> = listed
            .get("templates")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|value| serde_json::from_value::<TemplateInfo>(value.clone()).ok())
            .filter(|info| group.is_none_or(|group| info.group.as_deref() == Some(group)))
            .collect();
        if templates.is_empty() {
            return Err(t!("ide-file-template-none"));
        }
        let chosen = match template {
            Some(name) => templates
                .iter()
                .position(|info| info.template.name == name)
                .ok_or_else(|| t!("ide-file-template-unknown", name = name))?,
            None => 0,
        };
        let mut templates = templates;
        let mut chosen = chosen;
        let kinds = fixed
            && template.is_some_and(|name| {
                templates.retain(|info| info.template.name == name || info.kind_of.as_deref() == Some(name));
                chosen = templates.iter().position(|info| info.template.name == name).unwrap_or(0);
                templates.len() > 1
            });
        let mut dialog = NewFromTemplate {
            folder,
            templates,
            chosen: 0,
            name: String::new(),
            values: Vec::new(),
            error: None,
            focus: true,
            fixed: fixed && template.is_some() && !kinds,
            kinds,
        };
        dialog.choose(chosen);
        self.new_from_template = Some(dialog);
        Ok(())
    }

    pub(super) fn open_save_as_template(&mut self, session: &Session) -> Result<(), String> {
        let workbench = session.state::<Workbench>().ok_or_else(|| t!("ide-file-template-no-file"))?;
        let document = workbench
            .active_document()
            .filter(|document| document.image().is_none())
            .ok_or_else(|| t!("ide-file-template-no-file"))?;
        let template =
            zen_workbench::file_templates::commands::template_of_file(document.path(), document.text(), None);
        let project_scheme = session.state::<zen_workbench::file_templates::FileTemplates>().is_some_and(|templates| {
            templates.scheme_of(workbench.project()) == zen_workbench::file_templates::Scheme::Project
        });
        self.save_as_template = Some(SaveAsTemplate { template, project_scheme, error: None, focus: true });
        Ok(())
    }

    pub(super) fn new_from_template_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &mut self.new_from_template else { return };
        let p = self.palette;
        match show_new(ui, &p, dialog) {
            Some(Choice::Cancel) => self.new_from_template = None,
            Some(Choice::Ok) => {
                let Some(params) = dialog.create_params() else { return };
                match registry.dispatch(session, "file.templates.create", params) {
                    Ok(_) => self.new_from_template = None,
                    Err(error) => dialog.error = Some(dispatch_error(error)),
                }
            }
            None => {}
        }
    }

    pub(super) fn save_as_template_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &mut self.save_as_template else { return };
        let p = self.palette;
        match show_save(ui, &p, dialog) {
            Some(Choice::Cancel) => self.save_as_template = None,
            Some(Choice::Ok) => {
                let listed = registry.dispatch(session, "file.templates.list", json!({ "kind": "file" }));
                let taken = listed.ok().and_then(|listed| {
                    listed["templates"].as_array().map(|all| {
                        all.iter().any(|info| info["template"]["name"].as_str() == Some(dialog.template.name.trim()))
                    })
                });
                if taken == Some(true) {
                    dialog.error = Some(t!("ide-file-template-exists", name = dialog.template.name.trim()));
                    return;
                }
                let mut template = dialog.template.clone();
                template.name = template.name.trim().to_owned();
                template.extension = template.extension.trim().trim_start_matches('.').to_owned();
                match registry.dispatch(session, "file.templates.set", json!({ "template": template })) {
                    Ok(_) => self.save_as_template = None,
                    Err(error) => dialog.error = Some(dispatch_error(error)),
                }
            }
            None => {}
        }
    }
}

fn label(text: String, color: Color32) -> egui::RichText {
    egui::RichText::new(text).font(theme::regular(13.0)).color(color)
}

fn buttons(ui: &mut Ui, p: &IdePalette, choice: &mut Option<Choice>, ok_enabled: bool) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let ok = egui::Button::new(label(t!("ide-file-template-ok"), p.accent_text))
            .min_size(vec2(84.0, 30.0))
            .corner_radius(6)
            .fill(p.accent);
        if ui.add_enabled(ok_enabled, ok).clicked() {
            *choice = Some(Choice::Ok);
        }
        let cancel = egui::Button::new(label(t!("settings-cancel"), p.text))
            .min_size(vec2(84.0, 30.0))
            .corner_radius(6)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, p.button_border));
        if ui.add(cancel).clicked() {
            *choice = Some(Choice::Cancel);
        }
    });
}

fn show_new(ui: &mut Ui, p: &IdePalette, dialog: &mut NewFromTemplate) -> Option<Choice> {
    let mut choice = None;
    let mut chosen = dialog.chosen;
    let modal = egui::Modal::new(ui.scope_id().with("new-from-template")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        let named = if dialog.kinds { dialog.templates.first() } else { dialog.template() };
        let title = named.map_or_else(
            || t!("ide-file-template-new-title-plain"),
            |info| t!("ide-file-template-new-title", name = info.template.name.as_str()),
        );
        ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(6.0);
        let asks_name = dialog.asks_name();
        let focus = std::mem::take(&mut dialog.focus);
        egui::Grid::new("new-from-template-fields").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            if !dialog.fixed {
                let caption =
                    if dialog.kinds { t!("ide-file-template-kind") } else { t!("ide-file-template-template") };
                let caption = ui.label(label(caption, p.text));
                let kinds = dialog.kinds;
                let shown = move |info: &TemplateInfo| {
                    if kinds { info.template.name.clone() } else { info.template.full_name() }
                };
                let current = dialog.template().map(shown).unwrap_or_default();
                egui::ComboBox::from_id_salt("new-from-template-choice")
                    .width(280.0)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (index, info) in dialog.templates.iter().enumerate() {
                            ui.selectable_value(&mut chosen, index, shown(info));
                        }
                    })
                    .response
                    .labelled_by(caption.id);
                ui.end_row();
            }
            let mut first = true;
            if asks_name {
                let caption = ui.label(label(t!("ide-file-template-file-name"), p.text));
                let response =
                    ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(280.0)).labelled_by(caption.id);
                if focus {
                    response.request_focus();
                }
                first = false;
                ui.end_row();
            }
            for (name, value) in &mut dialog.values {
                let caption = ui.label(label(format!("{name}:"), p.text));
                let response = ui.add(egui::TextEdit::singleline(value).desired_width(280.0)).labelled_by(caption.id);
                if focus && first {
                    response.request_focus();
                    first = false;
                }
                ui.end_row();
            }
        });
        if let Some(info) = dialog.template()
            && !info.problems.is_empty()
        {
            ui.add_space(4.0);
            ui.add(egui::Label::new(label(t!("ide-file-template-problems"), p.warning_text)).wrap());
            for problem in info.problems.iter().take(3) {
                let place = format!("{}:{}", problem.line, problem.column);
                ui.add(egui::Label::new(label(format!("{place} {}", problem.message), p.warning_text)).wrap());
            }
        }
        if let Some(error) = &dialog.error {
            ui.add(egui::Label::new(label(error.clone(), p.error_text)).wrap());
        }
        ui.add_space(14.0);
        let ready = !asks_name || !dialog.name.trim().is_empty();
        buttons(ui, p, &mut choice, ready);
        if ready && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            choice = Some(Choice::Ok);
        }
    });
    if chosen != dialog.chosen {
        dialog.choose(chosen);
    }
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

fn show_save(ui: &mut Ui, p: &IdePalette, dialog: &mut SaveAsTemplate) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("save-as-template")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(560.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(egui::RichText::new(t!("ide-file-template-save-title")).font(theme::semibold(15.0)).color(p.text));
        let scheme = if dialog.project_scheme {
            t!("ide-file-template-scheme-project")
        } else {
            t!("ide-file-template-scheme-default")
        };
        ui.label(label(scheme, p.text_secondary));
        ui.add_space(6.0);
        let focus = std::mem::take(&mut dialog.focus);
        ui.horizontal(|ui| {
            let caption = ui.label(label(t!("ide-file-template-name"), p.text));
            let name = ui
                .add(
                    egui::TextEdit::singleline(&mut dialog.template.name)
                        .min_size(vec2(250.0, 0.0))
                        .desired_width(250.0),
                )
                .labelled_by(caption.id);
            if focus {
                name.request_focus();
            }
            let caption = ui.label(label(t!("ide-file-template-extension"), p.text));
            ui.add(
                egui::TextEdit::singleline(&mut dialog.template.extension)
                    .min_size(vec2(90.0, 0.0))
                    .desired_width(90.0),
            )
            .labelled_by(caption.id);
        });
        ui.horizontal(|ui| {
            let caption = ui.label(label(t!("ide-file-template-file-name"), p.text));
            ui.add(
                egui::TextEdit::singleline(&mut dialog.template.file_name)
                    .hint_text(t!("ide-file-template-file-name-hint"))
                    .desired_width(ui.available_width()),
            )
            .labelled_by(caption.id);
        });
        let text_caption = t!("ide-file-template-text");
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            let editor = ui.add(
                egui::TextEdit::multiline(&mut dialog.template.text)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            );
            crate::accessibility::name(&editor, egui::Role::MultilineTextInput, &text_caption);
        });
        ui.checkbox(&mut dialog.template.reformat, label(t!("ide-file-template-reformat"), p.text));
        ui.checkbox(&mut dialog.template.live_templates, label(t!("ide-file-template-live"), p.text));
        let problems = zen_workbench::file_templates::velocity::parse(&dialog.template.text).problems;
        for problem in problems.iter().take(2) {
            let place = format!("{}:{}", problem.line, problem.column);
            ui.add(egui::Label::new(label(format!("{place} {}", problem.message), p.warning_text)).wrap());
        }
        if let Some(error) = &dialog.error {
            ui.add(egui::Label::new(label(error.clone(), p.error_text)).wrap());
        }
        ui.add_space(10.0);
        let ready = !dialog.template.name.trim().is_empty() && dialog.template.kind == TemplateKind::File;
        buttons(ui, p, &mut choice, ready);
    });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}
