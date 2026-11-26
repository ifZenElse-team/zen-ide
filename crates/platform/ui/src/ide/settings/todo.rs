use egui::Ui;
use zen_i18n::t;
use zen_workbench::todo_patterns::{TodoFilter, TodoIcon, TodoPattern, TodoSettings};

use super::SettingsDialog;
use super::external_tools::{dialog_button, glyph_edit, tool_button};
use super::widgets;
use crate::ide::glyphs::{glyph_minus, glyph_plus};
use crate::ide::{IdePalette, dialog_frame};

pub(super) const SEARCH: &[&str] = &[
    "settings-todo-multiline",
    "settings-todo-patterns",
    "settings-todo-pattern",
    "settings-todo-case-sensitive",
    "settings-todo-icon",
    "settings-todo-filters",
    "settings-todo-filter-name",
    "settings-todo-no-filters",
];

const DEFAULT_COLOR: [u8; 3] = [0xFF, 0xFF, 0xFF];

struct PatternEditor {
    index: Option<usize>,
    pattern: TodoPattern,
    own_color: bool,
    color: [u8; 3],
    error: Option<String>,
}

struct FilterEditor {
    index: Option<usize>,
    filter: TodoFilter,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct TodoPage {
    pattern: Option<usize>,
    filter: Option<usize>,
    pattern_editor: Option<PatternEditor>,
    filter_editor: Option<FilterEditor>,
}

fn icon_name(icon: TodoIcon) -> String {
    match icon {
        TodoIcon::Default => t!("settings-todo-icon-default"),
        TodoIcon::Question => t!("settings-todo-icon-question"),
        TodoIcon::Important => t!("settings-todo-icon-important"),
    }
}

fn parse_color(color: Option<&str>) -> Option<[u8; 3]> {
    let hex = color?.strip_prefix('#').filter(|hex| hex.len() == 6)?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    let [_, red, green, blue] = value.to_be_bytes();
    Some([red, green, blue])
}

fn color_text([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02X}{green:02X}{blue:02X}")
}

impl PatternEditor {
    fn new(index: Option<usize>, pattern: TodoPattern) -> Self {
        let color = parse_color(pattern.color.as_deref());
        Self { index, pattern, own_color: color.is_some(), color: color.unwrap_or(DEFAULT_COLOR), error: None }
    }

    fn finished(&mut self, others: &[TodoPattern]) -> Option<TodoPattern> {
        let text = self.pattern.pattern.trim().to_owned();
        let same = others.iter().enumerate().any(|(at, other)| Some(at) != self.index && other.pattern == text);
        self.error = if text.is_empty() {
            Some(t!("settings-todo-pattern-empty"))
        } else if !zen_workbench::todo_patterns::is_valid(&text) {
            Some(t!("settings-todo-pattern-invalid"))
        } else if same {
            Some(t!("settings-todo-pattern-same"))
        } else {
            None
        };
        self.error.is_none().then(|| TodoPattern {
            pattern: text,
            color: self.own_color.then(|| color_text(self.color)),
            ..self.pattern.clone()
        })
    }

    fn show(&mut self, ui: &mut Ui, p: &IdePalette, others: &[TodoPattern]) -> Option<Option<TodoPattern>> {
        let mut done = None;
        egui::Modal::new(ui.scope_id().with("todo-pattern")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            let title =
                if self.index.is_some() { t!("settings-todo-edit-pattern") } else { t!("settings-todo-add-pattern") };
            ui.heading(title);
            ui.add_space(10.0);
            widgets::text_field(ui, p, &t!("settings-todo-pattern"), None, &mut self.pattern.pattern);
            ui.checkbox(&mut self.pattern.case_sensitive, t!("settings-todo-case-sensitive"));
            ui.horizontal(|ui| {
                let label = ui.label(t!("settings-todo-icon"));
                let combo = egui::ComboBox::from_id_salt("todo-pattern-icon")
                    .selected_text(icon_name(self.pattern.icon))
                    .show_ui(ui, |ui| {
                        for icon in [TodoIcon::Default, TodoIcon::Question, TodoIcon::Important] {
                            ui.selectable_value(&mut self.pattern.icon, icon, icon_name(icon));
                        }
                    });
                combo.response.labelled_by(label.id);
            });
            let mut scheme = !self.own_color;
            ui.checkbox(&mut scheme, t!("settings-todo-scheme-colors"));
            self.own_color = !scheme;
            if self.own_color {
                ui.horizontal(|ui| {
                    ui.label(t!("settings-todo-color"));
                    ui.color_edit_button_srgb(&mut self.color);
                });
            }
            if let Some(error) = &self.error {
                ui.colored_label(p.error_text, error);
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if dialog_button(ui, p, t!("settings-ok"), true)
                    && let Some(pattern) = self.finished(others)
                {
                    done = Some(Some(pattern));
                }
                if dialog_button(ui, p, t!("settings-cancel"), false) {
                    done = Some(None);
                }
            });
        });
        done
    }
}

impl FilterEditor {
    fn finished(&mut self, others: &[TodoFilter]) -> Option<TodoFilter> {
        let name = self.filter.name.trim().to_owned();
        let same = others.iter().enumerate().any(|(at, other)| Some(at) != self.index && other.name == name);
        self.error = if name.is_empty() {
            Some(t!("settings-todo-filter-name-empty"))
        } else if same {
            Some(t!("settings-todo-filter-name-same"))
        } else {
            None
        };
        self.error.is_none().then(|| TodoFilter { name, ..self.filter.clone() })
    }

    fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        patterns: &[TodoPattern],
        others: &[TodoFilter],
    ) -> Option<Option<TodoFilter>> {
        let mut done = None;
        egui::Modal::new(ui.scope_id().with("todo-filter")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            let title =
                if self.index.is_some() { t!("settings-todo-edit-filter") } else { t!("settings-todo-add-filter") };
            ui.heading(title);
            ui.add_space(10.0);
            widgets::text_field(ui, p, &t!("settings-todo-filter-name"), None, &mut self.filter.name);
            ui.label(t!("settings-todo-patterns"));
            for pattern in patterns {
                let mut on = self.filter.patterns.contains(&pattern.pattern);
                if ui.checkbox(&mut on, &pattern.pattern).changed() {
                    self.filter.patterns.retain(|other| *other != pattern.pattern);
                    if on {
                        self.filter.patterns.push(pattern.pattern.clone());
                    }
                }
            }
            if let Some(error) = &self.error {
                ui.colored_label(p.error_text, error);
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if dialog_button(ui, p, t!("settings-ok"), true)
                    && let Some(filter) = self.finished(others)
                {
                    done = Some(Some(filter));
                }
                if dialog_button(ui, p, t!("settings-cancel"), false) {
                    done = Some(None);
                }
            });
        });
        done
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Add,
    Remove,
    Edit,
}

fn tools(ui: &mut Ui, p: &IdePalette, selected: bool) -> Option<Tool> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if tool_button(ui, p, glyph_plus, &t!("settings-todo-add"), true) {
            chosen = Some(Tool::Add);
        }
        if tool_button(ui, p, glyph_minus, &t!("settings-todo-remove"), selected) {
            chosen = Some(Tool::Remove);
        }
        if tool_button(ui, p, glyph_edit, &t!("settings-todo-edit"), selected) {
            chosen = Some(Tool::Edit);
        }
    });
    chosen
}

fn row(ui: &mut Ui, selected: bool, cells: &[String], widths: &[f32]) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        for (cell, width) in cells.iter().zip(widths) {
            let button = egui::Button::selectable(selected, cell.as_str()).truncate();
            clicked |= ui.add_sized([*width, 20.0], button).clicked();
        }
    });
    clicked
}

fn renamed(settings: &mut TodoSettings, old: &str, new: &str) {
    for filter in &mut settings.filters {
        for pattern in filter.patterns.iter_mut().filter(|pattern| *pattern == old) {
            new.clone_into(pattern);
        }
    }
}

impl TodoPage {
    fn patterns(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>, settings: &mut TodoSettings) {
        widgets::group_title(ui, p, &t!("settings-todo-patterns"), query);
        match tools(ui, p, self.pattern.is_some()) {
            Some(Tool::Add) => self.pattern_editor = Some(PatternEditor::new(None, TodoPattern::default())),
            Some(Tool::Remove) => {
                if let Some(at) = self.pattern.take().filter(|at| *at < settings.patterns.len()) {
                    let gone = settings.patterns.remove(at);
                    for filter in &mut settings.filters {
                        filter.patterns.retain(|pattern| *pattern != gone.pattern);
                    }
                }
            }
            Some(Tool::Edit) => {
                if let Some(at) = self.pattern.filter(|at| *at < settings.patterns.len()) {
                    self.pattern_editor = Some(PatternEditor::new(Some(at), settings.patterns[at].clone()));
                }
            }
            None => {}
        }
        let room = ui.available_width() - 16.0;
        let widths = [room * 0.18, room * 0.26, room * 0.56];
        let head = [t!("settings-todo-icon"), t!("settings-todo-case-sensitive"), t!("settings-todo-pattern")];
        ui.horizontal(|ui| {
            for (title, width) in head.iter().zip(widths) {
                let title = egui::Label::new(widgets::label_job(ui, p, title, query)).truncate();
                ui.add_sized([width, 18.0], title);
            }
        });
        for (at, pattern) in settings.patterns.iter().enumerate() {
            let case = if pattern.case_sensitive { t!("settings-todo-yes") } else { t!("settings-todo-no") };
            let cells = [icon_name(pattern.icon), case, pattern.pattern.clone()];
            if row(ui, self.pattern == Some(at), &cells, &widths) {
                self.pattern = Some(at);
            }
        }
    }

    fn filters(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>, settings: &mut TodoSettings) {
        widgets::group_title(ui, p, &t!("settings-todo-filters"), query);
        match tools(ui, p, self.filter.is_some()) {
            Some(Tool::Add) => {
                self.filter_editor = Some(FilterEditor { index: None, filter: TodoFilter::default(), error: None });
            }
            Some(Tool::Remove) => {
                if let Some(at) = self.filter.take().filter(|at| *at < settings.filters.len()) {
                    let gone = settings.filters.remove(at);
                    if settings.commit_filter.as_deref() == Some(gone.name.as_str()) {
                        settings.commit_filter = None;
                    }
                }
            }
            Some(Tool::Edit) => {
                if let Some(at) = self.filter.filter(|at| *at < settings.filters.len()) {
                    let filter = settings.filters[at].clone();
                    self.filter_editor = Some(FilterEditor { index: Some(at), filter, error: None });
                }
            }
            None => {}
        }
        if settings.filters.is_empty() {
            ui.label(widgets::label_job(ui, p, &t!("settings-todo-no-filters"), query));
            return;
        }
        let room = ui.available_width() - 8.0;
        let widths = [room * 0.32, room * 0.68];
        for (at, filter) in settings.filters.iter().enumerate() {
            let cells = [filter.name.clone(), filter.patterns.join(", ")];
            if row(ui, self.filter == Some(at), &cells, &widths) {
                self.filter = Some(at);
            }
        }
    }

    fn dialogs(&mut self, ui: &mut Ui, p: &IdePalette, settings: &mut TodoSettings) {
        if let Some(editor) = &mut self.pattern_editor
            && let Some(outcome) = editor.show(ui, p, &settings.patterns)
        {
            match (outcome, editor.index) {
                (Some(pattern), Some(at)) => {
                    let old = std::mem::replace(&mut settings.patterns[at], pattern);
                    let new = settings.patterns[at].pattern.clone();
                    renamed(settings, &old.pattern, &new);
                }
                (Some(pattern), None) => settings.patterns.push(pattern),
                (None, _) => {}
            }
            self.pattern_editor = None;
        }
        if let Some(editor) = &mut self.filter_editor
            && let Some(outcome) = editor.show(ui, p, &settings.patterns, &settings.filters)
        {
            match (outcome, editor.index) {
                (Some(filter), Some(at)) => {
                    let old = std::mem::replace(&mut settings.filters[at], filter);
                    if settings.commit_filter.as_deref() == Some(old.name.as_str()) {
                        settings.commit_filter = Some(settings.filters[at].name.clone());
                    }
                }
                (Some(filter), None) => settings.filters.push(filter),
                (None, _) => {}
            }
            self.filter_editor = None;
        }
    }
}

impl SettingsDialog {
    pub(super) fn todo_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let settings = &mut draft.todo;
        let label = t!("settings-todo-multiline");
        ui.scope(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            widgets::checkbox(ui, p, &mut settings.multiline, &label, query);
        });
        ui.add_space(14.0);
        self.todo.patterns(ui, p, query, settings);
        ui.add_space(14.0);
        self.todo.filters(ui, p, query, settings);
        self.todo.dialogs(ui, p, settings);
    }
}
