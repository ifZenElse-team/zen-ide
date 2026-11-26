use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use egui::Ui;
use serde_json::{Value, json};
use zen_i18n::t;
use zen_project::code_style::{CodeStyleDeclaration, Label, OptionKind, StyleOption, StyleTab, Values};
use zen_workbench::code_style_options::{ARRANGEMENT, LanguageStyle};

use super::model::{CodeStyleScheme, IdeSettings};
use super::{Dispatch, widgets};
use crate::accessibility;
use crate::ide::IdePalette;

#[derive(Default)]
pub(in crate::ide) struct CodeStylePages {
    listed: Vec<LanguageStyle>,
    project: BTreeMap<String, Values>,
    tabs: BTreeMap<String, StyleTab>,
    message: Option<String>,
}

pub(in crate::ide) fn text(label: &Label) -> String {
    label.resolve(&zen_i18n::language(), |id| t!(id))
}

pub(in crate::ide) fn intern(text: String) -> &'static str {
    static INTERNED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let Ok(mut known) = INTERNED.lock() else { return Box::leak(text.into_boxed_str()) };
    if let Some(found) = known.get(text.as_str()) {
        return found;
    }
    let leaked: &'static str = Box::leak(text.into_boxed_str());
    known.insert(leaked);
    leaked
}

pub(in crate::ide) fn plugin_pages(
    session: &zen_core::Session,
    known: &[zen_modules::CodeStylePage],
) -> Vec<zen_modules::CodeStylePage> {
    let declared = session
        .state::<zen_workbench::code_style_options::CodeStyles>()
        .map(zen_workbench::code_style_options::CodeStyles::declarations)
        .unwrap_or_default();
    declared
        .into_iter()
        .filter(|declaration| !known.iter().any(|page| page.language == declaration.language))
        .map(|declaration| zen_modules::CodeStylePage {
            language: intern(declaration.language.clone()),
            title: intern(text(&declaration.title)),
        })
        .collect()
}

pub(in crate::ide) fn tab_key(tab: StyleTab) -> &'static str {
    match tab {
        StyleTab::TabsAndIndents => "settings-code-style-tabs",
        StyleTab::Spaces => "settings-code-style-tab-spaces",
        StyleTab::WrappingAndBraces => "settings-code-style-tab-wrapping",
        StyleTab::BlankLines => "settings-code-style-tab-blank-lines",
        StyleTab::Imports => "settings-code-style-tab-imports",
        StyleTab::Arrangement => "settings-code-style-tab-arrangement",
    }
}

impl CodeStylePages {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        let tabs = std::mem::take(&mut self.tabs);
        self.reset();
        self.tabs = tabs;
        match dispatch("editor.codeStyle.list", json!({}))
            .and_then(|value| serde_json::from_value::<Vec<LanguageStyle>>(value).map_err(|error| error.to_string()))
        {
            Ok(listed) => {
                self.project = listed
                    .iter()
                    .filter_map(|style| Some((style.declaration.language.clone(), style.project.clone()?)))
                    .collect();
                self.listed = listed;
            }
            Err(error) => self.message = Some(error),
        }
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>, scheme: CodeStyleScheme) {
        if scheme != CodeStyleScheme::Project {
            return;
        }
        let mut wrote = false;
        for style in &self.listed {
            let language = &style.declaration.language;
            let (Some(before), Some(after)) = (&style.project, self.project.get(language)) else { continue };
            let changed: Values = after
                .iter()
                .filter(|(id, value)| before.get(*id) != Some(*value))
                .map(|(id, value)| (id.clone(), value.clone()))
                .collect();
            if changed.is_empty() {
                continue;
            }
            match dispatch("editor.codeStyle.writeProject", json!({ "language": language, "values": changed })) {
                Ok(_) => wrote = true,
                Err(error) => self.message = Some(error),
            }
        }
        if wrote {
            self.begin(dispatch);
        }
    }

    pub(in crate::ide) fn declared(&self, language: &str) -> Option<&LanguageStyle> {
        self.listed.iter().find(|style| style.declaration.language == language)
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        query: Option<&str>,
        draft: &mut IdeSettings,
        language: &str,
        indents: impl FnOnce(&mut Ui, &mut IdeSettings),
    ) {
        let Some(style) = self.declared(language).cloned() else { return };
        let declaration = &style.declaration;
        let chosen = self.tabs.get(language).copied().unwrap_or(StyleTab::TabsAndIndents);
        ui.horizontal_wrapped(|ui| {
            for tab in StyleTab::ALL {
                let selected = chosen == tab;
                let name = t!(tab_key(tab));
                let text = egui::RichText::new(&name).color(if selected { p.text } else { p.text_secondary });
                let response = ui.add(egui::Button::selectable(selected, text));
                accessibility::item(&response, egui::Role::Tab, selected, &name);
                if response.clicked() && !selected {
                    self.tabs.insert(language.to_owned(), tab);
                }
            }
        });
        ui.add_space(8.0);
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.error_text));
            ui.add_space(6.0);
        }
        let project = draft.code_style_scheme == CodeStyleScheme::Project && style.project_file.is_some();
        if project && let Some(file) = style.project_file.as_ref().and_then(|file| file.file_name()) {
            let note = t!("settings-code-style-stored-in", file = file.to_string_lossy().into_owned());
            ui.label(egui::RichText::new(note).color(p.text_secondary));
            ui.add_space(6.0);
        }
        if chosen == StyleTab::TabsAndIndents {
            indents(ui, draft);
            ui.add_space(10.0);
        }
        let ide = declaration.complete(draft.code_style_options.get(language).unwrap_or(&Values::new()));
        let file = self.project.get(language).cloned().unwrap_or_else(|| declaration.defaults());
        let mut values = if project { declaration.with_config_values(&ide, &file) } else { ide.clone() };
        let before = values.clone();
        let mut group: Option<String> = None;
        for option in declaration.options.iter().filter(|option| option.tab == chosen) {
            let title = option.group.as_ref().map(text);
            if title.is_some() && title != group {
                ui.add_space(4.0);
                widgets::group_title(ui, p, title.as_deref().unwrap_or_default(), query);
                group = title;
            }
            if let Some(value) = values.get_mut(&option.id) {
                option_field(ui, p, query, &declaration.language, option, value);
            }
            ui.add_space(4.0);
        }
        if values != before {
            if project {
                self.project.insert(language.to_owned(), declaration.with_config_values(&file, &values));
                store_ide(draft, declaration, declaration.with_config_values(&values, &ide));
            } else {
                store_ide(draft, declaration, values);
            }
        }
        if chosen == StyleTab::Arrangement && !declaration.arrangement.is_empty() {
            arrangement(ui, p, query, draft, declaration, &style.arrangement);
        }
        if let Some(note) = declaration.notes.get(&chosen) {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(text(note)).color(p.text_secondary));
        }
        if !declaration.preview.is_empty() {
            ui.add_space(14.0);
            widgets::group_title(ui, p, &t!("settings-code-style-preview"), query);
            egui::Frame::new().fill(p.editor_background).corner_radius(6).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::both().id_salt(("settings-code-style-preview", language)).max_height(260.0).show(
                    ui,
                    |ui| {
                        let preview = egui::RichText::new(&declaration.preview).monospace().color(p.text);
                        ui.add(egui::Label::new(preview).extend());
                    },
                );
            });
        }
    }
}

fn store_ide(draft: &mut IdeSettings, declaration: &CodeStyleDeclaration, values: Values) {
    let entry = draft.code_style_options.entry(declaration.language.clone()).or_default();
    for option in &declaration.options {
        match values.get(&option.id) {
            Some(value) if *value != option.default => {
                entry.insert(option.id.clone(), value.clone());
            }
            _ => {
                entry.remove(&option.id);
            }
        }
    }
    if entry.is_empty() {
        draft.code_style_options.remove(&declaration.language);
    }
}

fn option_field(
    ui: &mut Ui,
    p: &IdePalette,
    query: Option<&str>,
    language: &str,
    option: &StyleOption,
    value: &mut Value,
) {
    let label = text(&option.label);
    match &option.kind {
        OptionKind::Boolean => {
            let mut on = value.as_bool().unwrap_or_default();
            if widgets::checkbox(ui, p, &mut on, &label, query).changed() {
                *value = Value::Bool(on);
            }
        }
        OptionKind::Integer { min, max } => {
            let mut number = value.as_i64().unwrap_or(*min);
            if widgets::number_field(ui, p, &label, query, &mut number, *min..=*max).changed() {
                *value = Value::from(number.clamp(*min, *max));
            }
        }
        OptionKind::Choice { choices } => {
            let current = value.as_str().unwrap_or_default().to_owned();
            let shown = choices.iter().find(|choice| choice.value == current).map(|choice| text(&choice.label));
            ui.horizontal_wrapped(|ui| {
                let title = ui.label(widgets::label_job(ui, p, &label, query));
                let combo = egui::ComboBox::from_id_salt(("settings-code-style-option", language, &option.id))
                    .selected_text(shown.unwrap_or(current.clone()))
                    .width(widgets::field_width(ui, 120.0, 220.0))
                    .show_ui(ui, |ui| {
                        for choice in choices {
                            if ui.selectable_label(choice.value == current, text(&choice.label)).clicked() {
                                *value = Value::String(choice.value.clone());
                            }
                        }
                    });
                accessibility::name(&combo.response.labelled_by(title.id), egui::Role::ComboBox, &label);
            });
        }
        OptionKind::Text => {
            let mut line = value.as_str().unwrap_or_default().to_owned();
            if widgets::text_field(ui, p, &label, query, &mut line).changed() {
                *value = Value::String(line.replace(['\n', '\r'], ""));
            }
        }
    }
}

fn arrangement(
    ui: &mut Ui,
    p: &IdePalette,
    query: Option<&str>,
    draft: &mut IdeSettings,
    declaration: &CodeStyleDeclaration,
    listed: &[String],
) {
    widgets::group_title(ui, p, &t!("settings-code-style-arrangement-order"), query);
    let language = &declaration.language;
    let stored = draft
        .code_style_options
        .get(language)
        .and_then(|values| values.get(ARRANGEMENT))
        .and_then(|value| serde_json::from_value::<Vec<String>>(value.clone()).ok());
    let mut order: Vec<String> = stored.unwrap_or_else(|| listed.to_vec());
    for entry in &declaration.arrangement {
        if !order.contains(&entry.id) {
            order.push(entry.id.clone());
        }
    }
    order.retain(|id| declaration.arrangement.iter().any(|entry| entry.id == *id));
    let mut moved = None;
    let last = order.len().saturating_sub(1);
    for (position, id) in order.iter().enumerate() {
        let Some(entry) = declaration.arrangement.iter().find(|entry| entry.id == *id) else { continue };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}.", position + 1)).color(p.text_secondary));
            ui.label(egui::RichText::new(text(&entry.label)).color(p.text));
            let up = t!("settings-code-style-arrangement-up");
            let down = t!("settings-code-style-arrangement-down");
            if ui.add_enabled(position > 0, egui::Button::new(up)).clicked() {
                moved = Some((position, position - 1));
            }
            if ui.add_enabled(position < last, egui::Button::new(down)).clicked() {
                moved = Some((position, position + 1));
            }
        });
    }
    if let Some((from, to)) = moved {
        order.swap(from, to);
        let declared: Vec<&str> = declaration.arrangement.iter().map(|entry| entry.id.as_str()).collect();
        let values = draft.code_style_options.entry(language.clone()).or_default();
        if order.iter().map(String::as_str).eq(declared) {
            values.remove(ARRANGEMENT);
        } else {
            values.insert(ARRANGEMENT.to_owned(), json!(order));
        }
        if values.is_empty() {
            draft.code_style_options.remove(language);
        }
    }
    ui.add_space(8.0);
    widgets::checkbox(ui, p, &mut draft.rearrange_on_reformat, &t!("settings-code-style-rearrange-on-reformat"), query);
}
