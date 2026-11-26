use egui::{Align2, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_workbench::templates::postfix::{PostfixInfo, PostfixList};
use zen_workbench::templates::{Context, OTHER, Template, TemplateInfo, Variable, expressions, variables_of};

use super::Dispatch;
use crate::accessibility;
use crate::ide::IdePalette;
use crate::theme;

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    template: Template,
    description: String,
    builtin: bool,
    default: Option<Template>,
    plugin: Option<String>,
    original: Option<usize>,
}

#[derive(Default)]
pub(super) struct TemplatesPage {
    original: Vec<TemplateInfo>,
    draft: Vec<Entry>,
    selected: Option<usize>,
    group: Option<String>,
    message: Option<String>,
}

fn entry(index: usize, info: &TemplateInfo) -> Entry {
    Entry {
        template: info.template.clone(),
        description: info.description_text.clone(),
        builtin: info.builtin,
        default: info.default.clone(),
        plugin: info.plugin.clone(),
        original: Some(index),
    }
}

fn group_name(language: &str) -> String {
    match language {
        OTHER => t!("settings-templates-group-other"),
        "go" => "Go".to_owned(),
        "rust" => "Rust".to_owned(),
        other => {
            let mut chars = other.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    }
}

fn saved(entry: &Entry, original: &[TemplateInfo]) -> Template {
    let mut template = entry.template.clone();
    let shown = entry
        .original
        .and_then(|index| original.get(index))
        .map(|info| (info.description_text.clone(), info.template.description.clone()));
    let default = entry.default.as_ref().map(|default| (default.description_text(), default.description.clone()));
    template.description = match (shown, default) {
        (_, Some((text, raw))) if entry.description == text => raw,
        (Some((text, raw)), _) if entry.description == text => raw,
        _ => entry.description.clone(),
    };
    template
}

fn same(a: &Template, b: &Template) -> bool {
    let plain = |template: &Template| Template { variables: Vec::new(), ..template.clone() };
    plain(a) == plain(b) && variables(&a.text, &a.variables) == variables(&b.text, &b.variables)
}

fn is_changed(entry: &Entry) -> bool {
    entry.default.as_ref().is_some_and(|default| !same(default, &entry.template))
}

fn requests(original: &[TemplateInfo], draft: &[Entry]) -> Vec<(&'static str, Value)> {
    let mut requests = Vec::new();
    for (index, info) in original.iter().enumerate() {
        if !info.builtin && !draft.iter().any(|entry| entry.original == Some(index)) {
            let key = json!({ "language": info.template.language, "abbreviation": info.template.abbreviation });
            requests.push(("editor.templates.remove", key));
        }
    }
    for entry in draft {
        let template = saved(entry, original);
        let before = entry.original.and_then(|index| original.get(index));
        if entry.builtin && entry.default.as_ref().is_some_and(|default| same(default, &template)) {
            if before.is_some_and(|info| info.modified) {
                let key = json!({ "language": template.language, "abbreviation": template.abbreviation });
                requests.push(("editor.templates.reset", key));
            }
        } else if before.is_none_or(|info| !same(&info.template, &template)) {
            let previous = before
                .filter(|info| !info.builtin && info.template.abbreviation != template.abbreviation)
                .map(|info| info.template.abbreviation.clone());
            requests.push(("editor.templates.set", json!({ "template": template, "previous": previous })));
        }
    }
    requests
}

fn variables(text: &str, known: &[Variable]) -> Vec<Variable> {
    let names = variables_of(text);
    let mut kept: Vec<Variable> = known.iter().filter(|variable| names.contains(&variable.name)).cloned().collect();
    for name in names {
        if !kept.iter().any(|variable| variable.name == name) {
            kept.push(Variable { name, ..Variable::default() });
        }
    }
    kept
}

impl TemplatesPage {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch);
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("editor.templates.list", json!({}))
            .and_then(|value| serde_json::from_value::<Vec<TemplateInfo>>(value).map_err(|error| error.to_string()))
        {
            Ok(listed) => {
                self.draft = listed.iter().enumerate().map(|(index, info)| entry(index, info)).collect();
                self.original = listed;
            }
            Err(error) => self.message = Some(error),
        }
        self.selected = self.selected.filter(|index| *index < self.draft.len());
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        let mut failed = None;
        for (command, params) in requests(&self.original, &self.draft) {
            if let Err(error) = dispatch(command, params) {
                failed = Some(error);
            }
        }
        let selected = self.selected.and_then(|index| self.draft.get(index)).map(|entry| entry.template.clone());
        self.load(dispatch);
        self.selected = selected.and_then(|template| {
            self.draft.iter().position(|entry| {
                entry.template.language == template.language && entry.template.abbreviation == template.abbreviation
            })
        });
        self.message = failed;
    }

    fn groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = Vec::new();
        for entry in &self.draft {
            if entry.template.language != OTHER && !groups.contains(&entry.template.language) {
                groups.push(entry.template.language.clone());
            }
        }
        groups.push(OTHER.to_owned());
        groups
    }

    fn add(&mut self) {
        let language = self
            .selected
            .and_then(|index| self.draft.get(index))
            .map(|entry| entry.template.language.clone())
            .or_else(|| self.group.clone())
            .unwrap_or_else(|| OTHER.to_owned());
        let abbreviation = self.free_abbreviation(&language, &t!("settings-templates-new-abbreviation"));
        self.draft.push(Entry {
            template: Template { language, abbreviation, text: "$END$".to_owned(), ..Template::default() },
            description: String::new(),
            builtin: false,
            default: None,
            plugin: None,
            original: None,
        });
        self.selected = Some(self.draft.len() - 1);
    }

    fn free_abbreviation(&self, language: &str, base: &str) -> String {
        let taken = |name: &str| {
            self.draft.iter().any(|entry| entry.template.language == language && entry.template.abbreviation == name)
        };
        let mut abbreviation = base.to_owned();
        let mut count = 1;
        while taken(&abbreviation) {
            count += 1;
            abbreviation = format!("{base}{count}");
        }
        abbreviation
    }

    fn duplicate(&mut self) {
        let Some(entry) = self.selected.and_then(|index| self.draft.get(index)).cloned() else { return };
        let original = entry.template.abbreviation.as_str();
        let stem = original.trim_end_matches(|c: char| c.is_ascii_digit());
        let base = if stem.is_empty() { original } else { stem };
        let abbreviation = self.free_abbreviation(&entry.template.language, base);
        self.draft.push(Entry {
            template: Template { abbreviation, enabled: true, ..entry.template },
            description: entry.description,
            builtin: false,
            default: None,
            plugin: None,
            original: None,
        });
        self.selected = Some(self.draft.len() - 1);
    }

    fn restore(&mut self) {
        let chosen = self.selected.and_then(|index| self.draft.get(index)).map(|entry| entry.template.language.clone());
        let group = chosen.or_else(|| self.group.clone());
        let only = self.selected;
        for (index, entry) in self.draft.iter_mut().enumerate() {
            let wanted = match only {
                Some(selected) => selected == index,
                None => group.as_deref().is_none_or(|group| entry.template.language == group),
            };
            if wanted && let Some(default) = &entry.default {
                entry.template = default.clone();
                entry.description = default.description_text();
            }
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.label(egui::RichText::new(t!("settings-templates-note")).color(p.text_secondary));
        ui.add_space(6.0);
        let selected_entry = self.selected.and_then(|index| self.draft.get(index));
        let can_duplicate = selected_entry.is_some();
        let can_remove = selected_entry.is_some_and(|entry| !entry.builtin);
        let can_restore = match selected_entry {
            Some(entry) => is_changed(entry),
            None => self
                .draft
                .iter()
                .filter(|entry| self.group.as_deref().is_none_or(|group| entry.template.language == group))
                .any(is_changed),
        };
        ui.horizontal_wrapped(|ui| {
            if ui.button(t!("settings-templates-add")).clicked() {
                self.add();
            }
            if ui.add_enabled(can_duplicate, egui::Button::new(t!("settings-templates-duplicate"))).clicked() {
                self.duplicate();
            }
            if ui.add_enabled(can_remove, egui::Button::new(t!("settings-templates-remove"))).clicked()
                && let Some(index) = self.selected.take()
            {
                self.draft.remove(index);
            }
            if ui.add_enabled(can_restore, egui::Button::new(t!("settings-templates-restore"))).clicked() {
                self.restore();
            }
        });
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.diagnostic_error));
        }
        ui.add_space(4.0);
        let list_height = (ui.available_height() * 0.42).max(120.0);
        egui::Frame::new().fill(p.island).corner_radius(4).inner_margin(4).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("settings-templates-list")
                .max_height(list_height)
                .auto_shrink([false, false])
                .show(ui, |ui| self.list(ui, p));
        });
        ui.add_space(8.0);
        self.editor(ui, p);
    }

    fn list(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        for group in self.groups() {
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            let group_selected = self.selected.is_none() && self.group.as_deref() == Some(group.as_str());
            accessibility::tree_item(&response, 1, Some(true), group_selected, &group_name(&group));
            if group_selected {
                ui.painter().rect_filled(rect, 4, p.selection);
            }
            ui.painter().text(
                pos2(rect.left() + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                group_name(&group),
                theme::semibold(13.0),
                p.text,
            );
            if response.clicked() {
                self.group = Some(group.clone());
                self.selected = None;
            }
            for index in 0..self.draft.len() {
                if self.draft[index].template.language != group {
                    continue;
                }
                let selected = self.selected == Some(index);
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    let entry = &mut self.draft[index];
                    let enabled = ui.checkbox(&mut entry.template.enabled, "");
                    accessibility::labelled(&enabled, &entry.template.abbreviation);
                    let color = if is_changed(entry) { p.link } else { p.text };
                    let mut text = egui::text::LayoutJob::default();
                    text.append(
                        &entry.template.abbreviation,
                        0.0,
                        egui::TextFormat::simple(theme::regular(13.0), color),
                    );
                    if !entry.description.is_empty() {
                        let format = egui::TextFormat::simple(theme::regular(13.0), p.text_secondary);
                        text.append(&format!(" ({})", entry.description), 0.0, format);
                    }
                    let response = ui.add(egui::Button::selectable(selected, text).frame_when_inactive(false));
                    if response.clicked() {
                        self.selected = Some(index);
                        self.group = Some(group.clone());
                    }
                });
            }
        }
    }

    fn editor(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(entry) = self.selected.and_then(|index| self.draft.get_mut(index)) else {
            ui.label(egui::RichText::new(t!("settings-templates-empty")).color(p.text_secondary));
            return;
        };
        let editable = entry.plugin.is_none();
        egui::Grid::new("settings-templates-fields").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(t!("settings-templates-abbreviation"));
            ui.add_enabled(
                !entry.builtin,
                egui::TextEdit::singleline(&mut entry.template.abbreviation).desired_width(160.0),
            );
            ui.end_row();
            ui.label(t!("settings-templates-description"));
            ui.add_enabled(
                editable,
                egui::TextEdit::singleline(&mut entry.description).desired_width(ui.available_width()),
            );
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.label(t!("settings-templates-text"));
        let edited = ui
            .add_enabled(
                editable,
                egui::TextEdit::multiline(&mut entry.template.text)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(5)
                    .desired_width(ui.available_width()),
            )
            .changed();
        if edited {
            entry.template.variables = variables(&entry.template.text, &entry.template.variables);
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(t!("settings-templates-contexts"));
            for (context, label) in [
                (Context::Code, "settings-templates-context-code"),
                (Context::Comment, "settings-templates-context-comment"),
                (Context::String, "settings-templates-context-string"),
            ] {
                let mut on = entry.template.expands_in(context);
                if ui.add_enabled(editable, egui::Checkbox::new(&mut on, t!(label))).changed() {
                    let mut contexts: Vec<Context> = [Context::Code, Context::Comment, Context::String]
                        .into_iter()
                        .filter(|known| if *known == context { on } else { entry.template.expands_in(*known) })
                        .collect();
                    if contexts == [Context::Code] {
                        contexts.clear();
                    }
                    entry.template.contexts = contexts;
                }
            }
        });
        ui.add_space(6.0);
        let mut current = variables(&entry.template.text, &entry.template.variables);
        if current.is_empty() {
            ui.label(egui::RichText::new(t!("settings-templates-no-variables")).color(p.text_secondary));
        } else {
            ui.label(egui::RichText::new(t!("settings-templates-variables")).font(theme::semibold(13.0)).color(p.text));
            let mut problem = None;
            egui::Grid::new("settings-templates-variables").num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
                for heading in [
                    "settings-templates-variable-name",
                    "settings-templates-variable-expression",
                    "settings-templates-variable-default",
                    "settings-templates-variable-skip",
                ] {
                    ui.label(egui::RichText::new(t!(heading)).color(p.text_secondary));
                }
                ui.end_row();
                let mut changed = false;
                for variable in &mut current {
                    ui.label(&variable.name);
                    let expression = egui::TextEdit::singleline(&mut variable.expression).desired_width(200.0);
                    changed |= ui.add_enabled(editable, expression).changed();
                    let default = egui::TextEdit::singleline(&mut variable.default).desired_width(140.0);
                    changed |= ui.add_enabled(editable, default).changed();
                    let skip = egui::Checkbox::without_text(&mut variable.skip_if_defined);
                    changed |= ui.add_enabled(editable, skip).changed();
                    ui.end_row();
                    if let Err(error) = expressions::check(&variable.expression) {
                        problem.get_or_insert(error);
                    }
                }
                if changed {
                    entry.template.variables = current;
                }
            });
            if let Some(problem) = problem {
                ui.label(egui::RichText::new(problem).color(p.diagnostic_error));
            }
            let functions: Vec<String> = expressions::FUNCTIONS.iter().map(|name| format!("{name}()")).collect();
            ui.label(
                egui::RichText::new(t!("settings-templates-functions", list = functions.join(", ")))
                    .color(p.text_secondary),
            );
        }
        if let Some(plugin) = &entry.plugin {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(t!("settings-templates-plugin", name = plugin.as_str())).color(p.text_secondary),
            );
        } else if entry.builtin {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(t!("settings-templates-builtin")).color(p.text_secondary));
        }
    }
}

#[derive(Default)]
pub(super) struct PostfixPage {
    original: PostfixList,
    enabled: bool,
    draft: Vec<bool>,
    selected: Option<usize>,
    message: Option<String>,
}

fn postfix_requests(original: &PostfixList, enabled: bool, draft: &[bool]) -> Vec<Value> {
    let mut requests = Vec::new();
    if enabled != original.enabled {
        requests.push(json!({ "enabled": enabled }));
    }
    for (info, on) in original.templates.iter().zip(draft) {
        if info.enabled != *on {
            requests.push(json!({ "language": info.template.language, "key": info.template.key, "enabled": on }));
        }
    }
    requests
}

impl PostfixPage {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch);
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("editor.postfix.list", json!({}))
            .and_then(|value| serde_json::from_value::<PostfixList>(value).map_err(|error| error.to_string()))
        {
            Ok(listed) => {
                self.enabled = listed.enabled;
                self.draft = listed.templates.iter().map(|info| info.enabled).collect();
                self.original = listed;
            }
            Err(error) => self.message = Some(error),
        }
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        let mut failed = None;
        for params in postfix_requests(&self.original, self.enabled, &self.draft) {
            if let Err(error) = dispatch("editor.postfix.set", params) {
                failed = Some(error);
            }
        }
        self.load(dispatch);
        self.message = failed;
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.checkbox(&mut self.enabled, t!("settings-postfix-enable"));
        ui.label(egui::RichText::new(t!("settings-postfix-note")).color(p.text_secondary));
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.diagnostic_error));
        }
        ui.add_space(6.0);
        let mut languages: Vec<String> = Vec::new();
        for info in &self.original.templates {
            if !languages.contains(&info.template.language) {
                languages.push(info.template.language.clone());
            }
        }
        let list_height = (ui.available_height() * 0.55).max(120.0);
        let enabled = self.enabled;
        egui::Frame::new().fill(p.island).corner_radius(4).inner_margin(4).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("settings-postfix-list")
                .max_height(list_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        for language in &languages {
                            ui.label(
                                egui::RichText::new(group_name(language)).font(theme::semibold(13.0)).color(p.text),
                            );
                            for (index, info) in self.original.templates.iter().enumerate() {
                                if info.template.language != *language {
                                    continue;
                                }
                                ui.horizontal(|ui| {
                                    ui.add_space(18.0);
                                    let on = ui.checkbox(&mut self.draft[index], "");
                                    accessibility::labelled(&on, &info.template.key);
                                    let label = postfix_label(info);
                                    let row = egui::Button::selectable(self.selected == Some(index), label);
                                    if ui.add(row.frame_when_inactive(false)).clicked() {
                                        self.selected = Some(index);
                                    }
                                });
                            }
                        }
                    });
                });
        });
        ui.add_space(8.0);
        match self.selected.and_then(|index| self.original.templates.get(index)) {
            Some(info) => {
                ui.label(egui::RichText::new(t!("settings-postfix-text")).color(p.text_secondary));
                ui.label(egui::RichText::new(&info.template.text).monospace().color(p.text));
                if let Some(plugin) = &info.plugin {
                    ui.label(
                        egui::RichText::new(t!("settings-postfix-plugin", name = plugin.as_str()))
                            .color(p.text_secondary),
                    );
                }
            }
            None => {
                ui.label(egui::RichText::new(t!("settings-postfix-empty")).color(p.text_secondary));
            }
        }
    }
}

fn postfix_label(info: &PostfixInfo) -> String {
    if info.description_text.is_empty() {
        format!(".{}", info.template.key)
    } else {
        format!(".{} ({})", info.template.key, info.description_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(abbreviation: &str, builtin: bool) -> TemplateInfo {
        let template = Template {
            language: "go".to_owned(),
            abbreviation: abbreviation.to_owned(),
            description: format!("go-{abbreviation}"),
            text: format!("{abbreviation} $X$"),
            variables: Vec::new(),
            imports: Vec::new(),
            contexts: Vec::new(),
            enabled: true,
        };
        TemplateInfo {
            description_text: format!("The {abbreviation}"),
            default: builtin.then(|| template.clone()),
            template,
            builtin,
            modified: false,
            plugin: None,
        }
    }

    #[test]
    fn the_draft_becomes_commands() {
        let original = vec![info("for", true), info("mine", false), info("gone", false)];
        let mut page = TemplatesPage {
            draft: original.iter().enumerate().map(|(index, info)| entry(index, info)).collect(),
            original: original.clone(),
            ..TemplatesPage::default()
        };
        assert!(requests(&page.original, &page.draft).is_empty(), "nothing changed");
        page.draft[0].template.enabled = false;
        page.draft[1].template.abbreviation = "renamed".to_owned();
        page.draft[1].description = "Mine now".to_owned();
        page.draft.remove(2);
        page.selected = Some(0);
        page.add();
        let sent = requests(&page.original, &page.draft);
        let ids: Vec<&str> = sent.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            ["editor.templates.remove", "editor.templates.set", "editor.templates.set", "editor.templates.set"]
        );
        assert_eq!(sent[1].1["template"]["description"], "go-for", "an untouched description keeps its message");
        assert_eq!(sent[1].1["template"]["enabled"], false);
        assert_eq!(sent[2].1["previous"], "mine");
        assert_eq!(sent[2].1["template"]["description"], "Mine now");
        assert_eq!(sent[3].1["template"]["language"], "go", "a new template joins the selected one's group");

        let mut changed = info("for", true);
        changed.modified = true;
        changed.template.enabled = false;
        let mut page =
            TemplatesPage { draft: vec![entry(0, &changed)], original: vec![changed], ..TemplatesPage::default() };
        page.selected = Some(0);
        page.restore();
        let reset = ("editor.templates.reset", json!({ "language": "go", "abbreviation": "for" }));
        assert_eq!(requests(&page.original, &page.draft), [reset]);
    }

    #[test]
    fn the_variables_follow_the_text() {
        let known = vec![Variable { name: "B".to_owned(), default: "b".to_owned(), ..Variable::default() }];
        let found = variables("$A$ $B$ $END$ $A$", &known);
        let names: Vec<&str> = found.iter().map(|variable| variable.name.as_str()).collect();
        assert_eq!(names, ["B", "A"]);
        assert_eq!(found[0].default, "b");
        assert!(variables("plain $END$", &known).is_empty());
        assert_eq!(group_name("go"), "Go");
    }

    #[test]
    fn a_duplicate_is_the_users_own_and_a_plugin_template_only_switches() {
        let mut plugin = info("sout", true);
        plugin.plugin = Some("java".to_owned());
        let original = vec![plugin];
        let mut page = TemplatesPage {
            draft: original.iter().enumerate().map(|(index, info)| entry(index, info)).collect(),
            original: original.clone(),
            ..TemplatesPage::default()
        };
        page.selected = Some(0);
        page.duplicate();
        page.duplicate();
        let names: Vec<&str> = page.draft.iter().map(|entry| entry.template.abbreviation.as_str()).collect();
        assert_eq!(names, ["sout", "sout2", "sout3"]);
        assert!(page.draft[1].plugin.is_none() && !page.draft[1].builtin);
        page.draft[0].template.enabled = false;
        let sent = requests(&page.original, &page.draft);
        assert_eq!(sent.len(), 3);
        assert_eq!(
            (sent[0].1["template"]["abbreviation"].as_str(), sent[0].1["template"]["enabled"].as_bool()),
            (Some("sout"), Some(false))
        );
    }

    #[test]
    fn the_postfix_page_sends_only_what_changed() {
        let template = zen_workbench::templates::PostfixTemplate {
            language: "go".to_owned(),
            key: "if".to_owned(),
            description: String::new(),
            text: "if $EXPR$ {}".to_owned(),
            variables: Vec::new(),
            imports: Vec::new(),
        };
        let listed = PostfixList {
            enabled: true,
            templates: vec![PostfixInfo { template, description_text: String::new(), enabled: true, plugin: None }],
        };
        assert!(postfix_requests(&listed, true, &[true]).is_empty());
        assert_eq!(
            postfix_requests(&listed, false, &[false]),
            [json!({ "enabled": false }), json!({ "language": "go", "key": "if", "enabled": false })]
        );
    }
}
