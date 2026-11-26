use egui::{Align2, Color32, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_workbench::file_templates::{ChildTemplate, FileTemplate, Scheme, Source, TemplateInfo, TemplateKind};

use super::Dispatch;
use crate::accessibility;
use crate::ide::IdePalette;
use crate::ide::glyphs::{Glyph, glyph_copy, glyph_minus, glyph_plus, glyph_plus_menu, glyph_undo};
use crate::theme;

const TABS: [TemplateKind; 3] = [TemplateKind::File, TemplateKind::Include, TemplateKind::Code];

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    template: FileTemplate,
    source: Source,
    default: Option<FileTemplate>,
    original: Option<usize>,
}

impl Entry {
    fn builtin(&self) -> bool {
        self.default.is_some()
    }

    fn changed(&self) -> bool {
        self.default.as_ref().is_some_and(|default| *default != self.template)
    }
}

#[derive(Clone, Debug, Default)]
struct Draft {
    original: Vec<TemplateInfo>,
    entries: Vec<Entry>,
}

impl Draft {
    fn of(listed: Vec<TemplateInfo>) -> Self {
        let entries = listed
            .iter()
            .enumerate()
            .map(|(index, info)| Entry {
                template: info.template.clone(),
                source: info.source,
                default: info.default.clone(),
                original: Some(index),
            })
            .collect();
        Self { original: listed, entries }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Variable {
    name: String,
    description: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selected {
    entry: usize,
    child: Option<usize>,
}

#[derive(Default)]
pub(super) struct FileTemplatesPage {
    loaded: bool,
    project: bool,
    in_use: Option<Scheme>,
    scheme: Option<Scheme>,
    default: Option<Draft>,
    project_draft: Option<Draft>,
    tab: usize,
    selected: Option<Selected>,
    variables: Vec<Variable>,
    variables_language: String,
    message: Option<String>,
}

fn scheme_name(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Default => "default",
        Scheme::Project => "project",
    }
}

fn kind_name(kind: TemplateKind) -> &'static str {
    match kind {
        TemplateKind::File => "file",
        TemplateKind::Include => "include",
        TemplateKind::Code => "code",
    }
}

fn requests(scheme: Scheme, draft: &Draft) -> Vec<(&'static str, Value)> {
    let own = |source: Source| match scheme {
        Scheme::Default => source == Source::Default,
        Scheme::Project => source == Source::Project,
    };
    let mut requests = Vec::new();
    for (index, info) in draft.original.iter().enumerate() {
        if !info.builtin && own(info.source) && !draft.entries.iter().any(|entry| entry.original == Some(index)) {
            let key = json!({ "name": info.template.name, "kind": kind_name(info.template.kind), "scheme": scheme_name(scheme) });
            requests.push(("file.templates.remove", key));
        }
    }
    for entry in &draft.entries {
        let before = entry.original.and_then(|index| draft.original.get(index));
        if entry.builtin() && !entry.changed() {
            if before.is_some_and(|info| info.modified && own(info.source)) {
                let key = json!({ "name": entry.template.name, "scheme": scheme_name(scheme) });
                requests.push(("file.templates.reset", key));
            }
            continue;
        }
        if before.is_some_and(|info| info.template == entry.template) {
            continue;
        }
        let previous = before
            .filter(|info| !info.builtin && own(info.source) && info.template.name != entry.template.name)
            .map(|info| info.template.name.clone());
        let mut template = entry.template.clone();
        template.name = template.name.trim().to_owned();
        template.extension = template.extension.trim().trim_start_matches('.').to_owned();
        requests.push((
            "file.templates.set",
            json!({ "template": template, "scheme": scheme_name(scheme), "previous": previous }),
        ));
    }
    requests
}

fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_owned();
    }
    (2..).map(|count| format!("{base} {count}")).find(|name| !taken(name)).unwrap_or_default()
}

impl FileTemplatesPage {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch);
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        self.loaded = true;
        self.default = None;
        self.project_draft = None;
        match dispatch("file.templates.list", json!({})) {
            Ok(listed) => {
                let scheme = serde_json::from_value::<Scheme>(listed["scheme"].clone()).unwrap_or(Scheme::Default);
                self.in_use = Some(scheme);
                self.scheme = Some(scheme);
            }
            Err(error) => self.message = Some(error),
        }
        self.project = dispatch("file.new.list", json!({})).is_ok();
        self.load_variables(dispatch);
        self.ensure_draft(dispatch);
    }

    fn load_variables(&mut self, dispatch: &mut Dispatch<'_>) {
        self.variables_language = zen_i18n::language();
        self.variables = dispatch("file.templates.variables", json!({}))
            .ok()
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|variable| Variable {
                name: variable["name"].as_str().unwrap_or_default().to_owned(),
                description: variable["description"].as_str().unwrap_or_default().to_owned(),
            })
            .collect();
    }

    fn ensure_draft(&mut self, dispatch: &mut Dispatch<'_>) {
        let scheme = self.scheme.unwrap_or(Scheme::Default);
        let missing = match scheme {
            Scheme::Default => self.default.is_none(),
            Scheme::Project => self.project_draft.is_none(),
        };
        if !missing {
            return;
        }
        let listed = dispatch("file.templates.list", json!({ "scheme": scheme_name(scheme) })).and_then(|value| {
            serde_json::from_value::<Vec<TemplateInfo>>(value["templates"].clone()).map_err(|error| error.to_string())
        });
        let draft = match listed {
            Ok(listed) => Draft::of(listed),
            Err(error) => {
                self.message = Some(error);
                Draft::default()
            }
        };
        match scheme {
            Scheme::Default => self.default = Some(draft),
            Scheme::Project => self.project_draft = Some(draft),
        }
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        if !self.loaded {
            return;
        }
        let scheme = self.scheme.unwrap_or(Scheme::Default);
        let mut sent: Vec<(&'static str, Value)> = Vec::new();
        if let Some(draft) = &self.default {
            sent.extend(requests(Scheme::Default, draft));
        }
        let switch = self.in_use != Some(scheme);
        match scheme {
            Scheme::Project => {
                if switch {
                    sent.push(("file.templates.useScheme", json!({ "scheme": "project" })));
                }
                if let Some(draft) = &self.project_draft {
                    sent.extend(requests(Scheme::Project, draft));
                }
            }
            Scheme::Default if switch => {
                sent.push(("file.templates.useScheme", json!({ "scheme": "default" })));
            }
            Scheme::Default => {}
        }
        let mut failed = None;
        for (command, params) in sent {
            if let Err(error) = dispatch(command, params) {
                failed = Some(error);
            }
        }
        let selected = self.selected_template().map(|template| (template.kind, template.name.clone()));
        let child = self.selected.and_then(|selected| selected.child);
        let tab = self.tab;
        self.load(dispatch);
        self.tab = tab;
        self.selected = selected.and_then(|(kind, name)| {
            let entry =
                self.entries().iter().position(|entry| entry.template.kind == kind && entry.template.name == name)?;
            Some(Selected { entry, child })
        });
        self.message = failed;
    }

    fn draft(&self) -> Option<&Draft> {
        match self.scheme.unwrap_or(Scheme::Default) {
            Scheme::Default => self.default.as_ref(),
            Scheme::Project => self.project_draft.as_ref(),
        }
    }

    fn draft_mut(&mut self) -> Option<&mut Draft> {
        match self.scheme.unwrap_or(Scheme::Default) {
            Scheme::Default => self.default.as_mut(),
            Scheme::Project => self.project_draft.as_mut(),
        }
    }

    fn entries(&self) -> &[Entry] {
        self.draft().map_or(&[], |draft| draft.entries.as_slice())
    }

    fn kind(&self) -> TemplateKind {
        TABS[self.tab.min(TABS.len() - 1)]
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|selected| self.entries().get(selected.entry))
    }

    fn selected_template(&self) -> Option<&FileTemplate> {
        self.selected_entry().map(|entry| &entry.template)
    }

    fn own(&self, entry: &Entry) -> bool {
        match self.scheme.unwrap_or(Scheme::Default) {
            Scheme::Default => entry.source != Source::Project,
            Scheme::Project => entry.source == Source::Project || entry.original.is_none(),
        }
    }

    fn taken(&self, kind: TemplateKind, name: &str) -> bool {
        self.entries().iter().any(|entry| entry.template.kind == kind && entry.template.name == name)
    }

    fn create(&mut self) {
        let kind = self.kind();
        if kind == TemplateKind::Code {
            return;
        }
        let name = unique(&t!("settings-file-templates-unnamed"), |name| self.taken(kind, name));
        let template = FileTemplate {
            name,
            extension: String::new(),
            kind,
            file_name: String::new(),
            text: String::new(),
            reformat: true,
            live_templates: false,
            children: Vec::new(),
        };
        self.push(template);
    }

    fn copy(&mut self) {
        let Some(mut template) = self.selected_template().cloned() else { return };
        let base = t!("settings-file-templates-copy-name", name = template.name.as_str());
        template.name = unique(&base, |name| self.taken(template.kind, name));
        self.push(template);
    }

    fn push(&mut self, template: FileTemplate) {
        let Some(draft) = self.draft_mut() else { return };
        draft.entries.push(Entry { template, source: Source::Default, default: None, original: None });
        let entry = draft.entries.len() - 1;
        self.selected = Some(Selected { entry, child: None });
    }

    fn create_child(&mut self) {
        let Some(selected) = self.selected else { return };
        let Some(draft) = self.draft_mut() else { return };
        let Some(entry) = draft.entries.get_mut(selected.entry) else { return };
        let child = ChildTemplate { extension: entry.template.extension.clone(), ..ChildTemplate::default() };
        entry.template.children.push(child);
        let index = entry.template.children.len() - 1;
        self.selected = Some(Selected { entry: selected.entry, child: Some(index) });
    }

    fn remove(&mut self) {
        let Some(selected) = self.selected else { return };
        let Some(draft) = self.draft_mut() else { return };
        match selected.child {
            Some(child) => {
                if let Some(entry) = draft.entries.get_mut(selected.entry)
                    && child < entry.template.children.len()
                {
                    entry.template.children.remove(child);
                }
                self.selected = Some(Selected { entry: selected.entry, child: None });
            }
            None => {
                if selected.entry < draft.entries.len() {
                    draft.entries.remove(selected.entry);
                }
                self.selected = None;
            }
        }
    }

    fn reset_selected(&mut self) {
        let Some(selected) = self.selected else { return };
        let Some(draft) = self.draft_mut() else { return };
        if let Some(entry) = draft.entries.get_mut(selected.entry)
            && let Some(default) = entry.default.clone()
        {
            entry.template = default;
        }
        self.selected = Some(Selected { entry: selected.entry, child: None });
    }

    fn can_remove(&self) -> bool {
        let Some(selected) = self.selected else { return false };
        let Some(entry) = self.selected_entry() else { return false };
        selected.child.is_some() || (!entry.builtin() && self.own(entry))
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: Option<&mut Dispatch<'_>>) {
        if let Some(dispatch) = dispatch {
            if !self.loaded {
                self.load(dispatch);
            } else if self.variables_language != zen_i18n::language() {
                self.load_variables(dispatch);
            }
            self.scheme_row(ui, p, dispatch);
        }
        ui.add_space(8.0);
        self.tabs(ui, p);
        ui.add_space(6.0);
        self.toolbar(ui, p);
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.diagnostic_error));
        }
        ui.add_space(4.0);
        let height = 330.0;
        ui.horizontal_top(|ui| {
            let list_width = 190.0;
            ui.allocate_ui(vec2(list_width, height), |ui| {
                egui::Frame::new().fill(p.island).corner_radius(4).inner_margin(4).show(ui, |ui| {
                    ui.set_width(list_width - 8.0);
                    ui.set_min_height(height - 8.0);
                    egui::ScrollArea::vertical()
                        .id_salt(("settings-file-templates-list", self.tab))
                        .max_height(height - 8.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| ui.vertical(|ui| self.list(ui, p)));
                });
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                self.editor(ui, p);
            });
        });
    }

    fn scheme_row(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let mut scheme = self.scheme.unwrap_or(Scheme::Default);
        let before = scheme;
        ui.horizontal(|ui| {
            let caption = ui.label(egui::RichText::new(t!("settings-file-templates-scheme")).color(p.text));
            let shown = |scheme: Scheme| match scheme {
                Scheme::Default => t!("settings-file-templates-scheme-default"),
                Scheme::Project => t!("settings-file-templates-scheme-project"),
            };
            ui.add_enabled_ui(self.project, |ui| {
                egui::ComboBox::from_id_salt("settings-file-templates-scheme")
                    .width(160.0)
                    .selected_text(shown(scheme))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut scheme, Scheme::Default, shown(Scheme::Default));
                        ui.selectable_value(&mut scheme, Scheme::Project, shown(Scheme::Project));
                    })
                    .response
                    .labelled_by(caption.id);
            });
        });
        let note = match scheme {
            Scheme::Project => t!("settings-file-templates-scheme-project-note"),
            Scheme::Default => t!("settings-file-templates-scheme-default-note"),
        };
        ui.label(egui::RichText::new(note).font(theme::regular(12.0)).color(p.text_secondary));
        if scheme != before {
            self.scheme = Some(scheme);
            self.selected = None;
            self.ensure_draft(dispatch);
        }
    }

    fn tabs(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (index, key) in
                ["settings-file-templates-files", "settings-file-templates-includes", "settings-file-templates-code"]
                    .into_iter()
                    .enumerate()
            {
                let selected = self.tab == index;
                let text = egui::RichText::new(t!(key)).color(if selected { p.text } else { p.text_secondary });
                let response = ui.add(egui::Button::selectable(selected, text));
                accessibility::edit(&response, |node| {
                    node.set_role(egui::accesskit::Role::Tab);
                    node.set_selected(selected);
                });
                if response.clicked() && !selected {
                    self.tab = index;
                    self.selected = None;
                }
            }
        });
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette) {
        let kind = self.kind();
        let entry = self.selected_entry();
        let can_create = kind != TemplateKind::Code;
        let can_child = kind == TemplateKind::File && entry.is_some();
        let can_remove = self.can_remove();
        let can_copy = kind != TemplateKind::Code && self.selected.is_some_and(|selected| selected.child.is_none());
        let can_reset = entry.is_some_and(Entry::changed);
        let actions: [(Glyph, &str, bool); 5] = [
            (glyph_plus, "settings-file-templates-create", can_create),
            (glyph_plus_menu, "settings-file-templates-create-child", can_child),
            (glyph_minus, "settings-file-templates-remove", can_remove),
            (glyph_copy, "settings-file-templates-copy", can_copy),
            (glyph_undo, "settings-file-templates-reset", can_reset),
        ];
        let mut clicked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (glyph, key, enabled) in actions {
                if icon_button(ui, p, glyph, &t!(key), enabled) {
                    clicked = Some(key);
                }
            }
        });
        match clicked {
            Some("settings-file-templates-create") => self.create(),
            Some("settings-file-templates-create-child") => self.create_child(),
            Some("settings-file-templates-remove") => self.remove(),
            Some("settings-file-templates-copy") => self.copy(),
            Some("settings-file-templates-reset") => self.reset_selected(),
            _ => {}
        }
    }

    fn list(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let kind = self.kind();
        let mut clicked = None;
        for (index, entry) in self.entries().iter().enumerate() {
            if entry.template.kind != kind {
                continue;
            }
            let color = if entry.changed() { p.link } else { p.text };
            let label = if entry.template.extension.is_empty() {
                entry.template.name.clone()
            } else {
                format!("{}.{}", entry.template.name, entry.template.extension)
            };
            let selected = self.selected == Some(Selected { entry: index, child: None });
            if row(ui, p, &label, color, 0.0, selected) {
                clicked = Some(Selected { entry: index, child: None });
            }
            for (child_index, child) in entry.template.children.iter().enumerate() {
                let stem = if child.file_name.trim().is_empty() { "${NAME}" } else { child.file_name.trim() };
                let label =
                    if child.extension.is_empty() { stem.to_owned() } else { format!("{stem}.{}", child.extension) };
                let selected = self.selected == Some(Selected { entry: index, child: Some(child_index) });
                if row(ui, p, &label, color, 16.0, selected) {
                    clicked = Some(Selected { entry: index, child: Some(child_index) });
                }
            }
        }
        if clicked.is_some() {
            self.selected = clicked;
        }
    }

    fn editor(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(selected) = self.selected else {
            ui.label(egui::RichText::new(t!("settings-file-templates-empty")).color(p.text_secondary));
            self.variables_description(ui, p);
            return;
        };
        let builtin = self.selected_entry().is_some_and(Entry::builtin);
        let Some(entry) = self.draft_mut().and_then(|draft| draft.entries.get_mut(selected.entry)) else { return };
        let template = &mut entry.template;
        let field = |ui: &mut Ui, key: &str, value: &mut String, width: f32, enabled: bool| {
            let caption = ui.label(t!(key));
            ui.add_enabled(enabled, egui::TextEdit::singleline(value).desired_width(width)).labelled_by(caption.id);
        };
        match selected.child.and_then(|child| template.children.get_mut(child)) {
            Some(child) => {
                ui.horizontal(|ui| {
                    field(ui, "settings-file-templates-extension", &mut child.extension, 70.0, true);
                    field(
                        ui,
                        "settings-file-templates-file-name",
                        &mut child.file_name,
                        ui.available_width() - 70.0,
                        true,
                    );
                });
                text_editor(ui, &mut child.text);
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut child.reformat, t!("settings-file-templates-reformat"));
                    ui.checkbox(&mut child.live_templates, t!("settings-file-templates-live"));
                });
            }
            None => {
                ui.horizontal(|ui| {
                    field(ui, "settings-file-templates-name", &mut template.name, 150.0, !builtin);
                    field(ui, "settings-file-templates-extension", &mut template.extension, 60.0, !builtin);
                });
                if template.kind == TemplateKind::File {
                    ui.horizontal(|ui| {
                        let width = ui.available_width() - 70.0;
                        field(ui, "settings-file-templates-file-name", &mut template.file_name, width, true);
                    });
                }
                text_editor(ui, &mut template.text);
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut template.reformat, t!("settings-file-templates-reformat"));
                    ui.checkbox(&mut template.live_templates, t!("settings-file-templates-live"));
                });
            }
        }
        if builtin {
            ui.label(
                egui::RichText::new(t!("settings-file-templates-builtin"))
                    .font(theme::regular(12.0))
                    .color(p.text_secondary),
            );
        }
        self.variables_description(ui, p);
    }

    fn variables_description(&self, ui: &mut Ui, p: &IdePalette) {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(t!("settings-file-templates-description")).font(theme::semibold(13.0)).color(p.text),
        );
        egui::Frame::new().fill(p.island).corner_radius(4).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt("settings-file-templates-variables").max_height(150.0).show(
                ui,
                |ui| {
                    ui.label(
                        egui::RichText::new(t!("settings-file-templates-description-note"))
                            .font(theme::regular(12.0))
                            .color(p.text_secondary),
                    );
                    egui::Grid::new("settings-file-templates-variables-grid")
                        .num_columns(2)
                        .spacing([10.0, 2.0])
                        .max_col_width((ui.available_width() - 190.0).max(160.0))
                        .show(ui, |ui| {
                            for variable in &self.variables {
                                ui.label(
                                    egui::RichText::new(format!("${{{}}}", variable.name))
                                        .font(egui::FontId::monospace(12.0))
                                        .color(p.text),
                                );
                                ui.label(
                                    egui::RichText::new(&variable.description)
                                        .font(theme::regular(12.0))
                                        .color(p.text_secondary),
                                );
                                ui.end_row();
                            }
                        });
                },
            );
        });
    }
}

fn text_editor(ui: &mut Ui, text: &mut String) {
    let caption = t!("settings-file-templates-text");
    egui::ScrollArea::vertical().id_salt("settings-file-templates-text").max_height(170.0).show(ui, |ui| {
        let editor = ui.add(
            egui::TextEdit::multiline(text)
                .font(egui::TextStyle::Monospace)
                .code_editor()
                .desired_rows(8)
                .desired_width(f32::INFINITY),
        );
        accessibility::name(&editor, egui::Role::MultilineTextInput, &caption);
    });
}

fn row(ui: &mut Ui, p: &IdePalette, label: &str, color: Color32, indent: f32, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 4, p.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    accessibility::name_with(&response, egui::Role::ListBoxOption, || label.to_owned());
    accessibility::edit(&response, |node| node.set_selected(selected));
    let clip = rect.shrink2(vec2(4.0, 0.0)).intersect(ui.clip_rect());
    ui.painter().with_clip_rect(clip).text(
        pos2(rect.left() + 6.0 + indent, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(13.0),
        color,
    );
    response.clicked()
}

fn icon_button(ui: &mut Ui, p: &IdePalette, glyph: Glyph, name: &str, enabled: bool) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(26.0, 24.0), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let color = if enabled { p.text } else { p.text_secondary.gamma_multiply(0.5) };
    glyph(ui.painter(), Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), color);
    accessibility::button_with(&response, || name.to_owned());
    accessibility::edit(&response, |node| {
        if !enabled {
            node.set_disabled();
        }
    });
    let response = response.on_hover_text(name);
    enabled && response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(name: &str, text: &str) -> FileTemplate {
        FileTemplate {
            name: name.to_owned(),
            extension: "go".to_owned(),
            kind: TemplateKind::File,
            file_name: String::new(),
            text: text.to_owned(),
            reformat: true,
            live_templates: false,
            children: Vec::new(),
        }
    }

    fn info(template: FileTemplate, source: Source, default: Option<FileTemplate>) -> TemplateInfo {
        TemplateInfo {
            modified: default.as_ref().is_some_and(|default| *default != template),
            builtin: default.is_some(),
            origin: None,
            default,
            template,
            source,
            asked: Vec::new(),
            problems: Vec::new(),
            markers: Vec::new(),
            source_folder: false,
            group: None,
            kind_of: None,
        }
    }

    #[test]
    fn the_draft_becomes_commands_of_its_scheme() {
        let builtin = template("Go File", "package x\n");
        let mut changed = builtin.clone();
        changed.text = "package y\n".to_owned();
        let original = vec![
            info(builtin.clone(), Source::Builtin, Some(builtin.clone())),
            info(changed.clone(), Source::Default, Some(builtin.clone())),
            info(template("Mine", "mine\n"), Source::Default, None),
            info(template("Gone", "gone\n"), Source::Default, None),
        ];
        let mut draft = Draft::of(original);
        assert!(requests(Scheme::Default, &draft).is_empty(), "nothing changed");
        draft.entries[0].template.text = "package z\n".to_owned();
        draft.entries[1].template = builtin.clone();
        draft.entries[2].template.name = "Renamed".to_owned();
        draft.entries[2].template.children.push(ChildTemplate::default());
        draft.entries.remove(3);
        let sent = requests(Scheme::Default, &draft);
        let ids: Vec<&str> = sent.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, ["file.templates.remove", "file.templates.set", "file.templates.reset", "file.templates.set"]);
        assert_eq!(sent[0].1, json!({ "name": "Gone", "kind": "file", "scheme": "default" }));
        assert_eq!(sent[1].1["template"]["text"], "package z\n");
        assert_eq!(sent[2].1, json!({ "name": "Go File", "scheme": "default" }));
        assert_eq!(sent[3].1["previous"], "Mine");
        assert_eq!(sent[3].1["template"]["children"].as_array().map(Vec::len), Some(1));

        let project = Draft::of(vec![info(template("Mine", "ide\n"), Source::Default, None)]);
        assert!(requests(Scheme::Project, &project).is_empty(), "the IDE's templates shown in Project are not sent");
        let mut project = project;
        project.entries[0].template.text = "project\n".to_owned();
        let sent = requests(Scheme::Project, &project);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].1["scheme"], "project");
        assert_eq!(sent[0].1["previous"], Value::Null, "an override is not a rename");
    }

    #[test]
    fn built_in_templates_are_not_removed_and_copies_get_a_free_name() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::with_language("en", built_in_templates_in_english);
    }

    fn built_in_templates_in_english() {
        let builtin = template("Go File", "package x\n");
        let mut page = FileTemplatesPage {
            loaded: true,
            scheme: Some(Scheme::Default),
            default: Some(Draft::of(vec![
                info(builtin.clone(), Source::Builtin, Some(builtin.clone())),
                info(template("Mine", "mine\n"), Source::Default, None),
            ])),
            ..FileTemplatesPage::default()
        };
        page.selected = Some(Selected { entry: 0, child: None });
        assert!(!page.can_remove(), "a built-in template cannot be removed");
        page.copy();
        page.selected = Some(Selected { entry: 0, child: None });
        page.copy();
        let names: Vec<&str> = page.entries().iter().map(|entry| entry.template.name.as_str()).collect();
        assert_eq!(names[2..], ["Go File copy", "Go File copy 2"]);
        assert!(page.can_remove(), "a copy is the user's");
        page.selected = Some(Selected { entry: 0, child: None });
        page.create_child();
        assert_eq!(page.selected, Some(Selected { entry: 0, child: Some(0) }));
        assert!(page.can_remove(), "a child of a built-in one can be removed");
        assert!(page.entries()[0].changed(), "a built-in template with a child is changed");
        page.reset_selected();
        assert!(!page.entries()[0].changed(), "Revert to Original Template takes it back");
        page.tab = 2;
        page.create();
        assert_eq!(page.entries().len(), 4, "nothing is created on the Code tab: Create is off there");
    }
}
