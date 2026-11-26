use std::path::PathBuf;

use egui::{Color32, Ui, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_workbench::external_tools::MacroInfo;
use zen_workbench::file_watchers::{
    EnvVar, FileWatcher, Level, ListedWatcher, ShowConsole, Template, WatchScope, WatcherList,
};

use super::Dispatch;
use super::external_tools::{dialog_button, glyph_copy, glyph_down, glyph_edit, glyph_up, tool_button};
use crate::accessibility;
use crate::ide::glyphs::{glyph_minus, glyph_plus};
use crate::ide::{IdePalette, dialog_frame};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Program,
    Arguments,
    OutputPaths,
    WorkingDirectory,
}

impl Field {
    const ALL: [Field; 4] = [Field::Program, Field::Arguments, Field::OutputPaths, Field::WorkingDirectory];

    fn label(self) -> String {
        match self {
            Field::Program => t!("settings-file-watchers-program"),
            Field::Arguments => t!("settings-file-watchers-arguments"),
            Field::OutputPaths => t!("settings-file-watchers-output-paths"),
            Field::WorkingDirectory => t!("settings-file-watchers-working-directory"),
        }
    }

    fn text(self, watcher: &mut FileWatcher) -> &mut String {
        match self {
            Field::Program => &mut watcher.program,
            Field::Arguments => &mut watcher.arguments,
            Field::OutputPaths => &mut watcher.output_paths,
            Field::WorkingDirectory => &mut watcher.working_directory,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Remove,
    Edit,
    Up,
    Down,
    Copy,
    Import,
    Export,
}

struct WatcherEditor {
    index: Option<usize>,
    level: Level,
    watcher: FileWatcher,
    environment: String,
    filters: String,
    error: Option<String>,
    focus: bool,
}

pub(super) fn environment_text(environment: &[EnvVar]) -> String {
    environment.iter().map(|variable| format!("{}={}", variable.name, variable.value)).collect::<Vec<_>>().join(";")
}

pub(super) fn parse_environment(text: &str) -> Vec<EnvVar> {
    text.split(';')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => EnvVar { name: name.trim().to_owned(), value: value.to_owned() },
            None => EnvVar { name: pair.to_owned(), value: String::new() },
        })
        .collect()
}

impl WatcherEditor {
    fn new(index: Option<usize>, listed: ListedWatcher) -> Self {
        let environment = environment_text(&listed.watcher.environment);
        let filters = listed.watcher.output_filters.join("\n");
        Self { index, level: listed.level, watcher: listed.watcher, environment, filters, error: None, focus: true }
    }

    fn finished(&self) -> ListedWatcher {
        let mut watcher = self.watcher.clone();
        watcher.name = watcher.name.trim().to_owned();
        watcher.environment = parse_environment(&self.environment);
        watcher.output_filters =
            self.filters.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect();
        ListedWatcher { level: self.level, watcher }
    }
}

#[derive(Default)]
pub(super) struct FileWatchersPage {
    original: Vec<ListedWatcher>,
    pub(super) draft: Vec<ListedWatcher>,
    selected: Option<usize>,
    templates: Vec<Template>,
    editor: Option<WatcherEditor>,
    macros: Vec<MacroInfo>,
    types: Vec<(String, String)>,
    has_project: bool,
    message: Option<String>,
    pub(super) actions_on_save: bool,
}

fn level_name(level: Level) -> String {
    match level {
        Level::Project => t!("settings-file-watchers-level-project"),
        Level::Global => t!("settings-file-watchers-level-global"),
    }
}

fn scope_name(scope: WatchScope) -> String {
    match scope {
        WatchScope::ProjectFiles => t!("settings-file-watchers-scope-project"),
        WatchScope::OpenFiles => t!("settings-file-watchers-scope-open"),
        WatchScope::ChangedFiles => t!("settings-file-watchers-scope-changed"),
    }
}

fn console_name(console: ShowConsole) -> String {
    match console {
        ShowConsole::Always => t!("settings-file-watchers-console-always"),
        ShowConsole::OnError => t!("settings-file-watchers-console-error"),
        ShowConsole::Never => t!("settings-file-watchers-console-never"),
    }
}

fn neighbour(draft: &[ListedWatcher], index: usize, up: bool) -> Option<usize> {
    let level = draft.get(index)?.level;
    if up {
        draft[..index].iter().rposition(|listed| listed.level == level)
    } else {
        draft[index + 1..].iter().position(|listed| listed.level == level).map(|at| index + 1 + at)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_import() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(t!("settings-file-watchers-import"))
        .add_filter(t!("settings-file-watchers-files"), &["json"])
        .pick_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_import() -> Option<PathBuf> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_export() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(t!("settings-file-watchers-export"))
        .set_file_name("watchers.json")
        .add_filter(t!("settings-file-watchers-files"), &["json"])
        .save_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_export() -> Option<PathBuf> {
    None
}

fn glyph_import(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let s = crate::ide::glyphs::stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.12);
    painter.line_segment([r.center_top(), egui::pos2(r.center().x, r.bottom() - r.height() * 0.3)], s);
    let tip = egui::pos2(r.center().x, r.bottom() - r.height() * 0.3);
    painter.line_segment([tip, tip + vec2(-r.width() * 0.25, -r.height() * 0.25)], s);
    painter.line_segment([tip, tip + vec2(r.width() * 0.25, -r.height() * 0.25)], s);
    painter.line_segment([r.left_bottom(), r.right_bottom()], s);
}

fn glyph_export(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let s = crate::ide::glyphs::stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.12);
    let top = r.center_top();
    painter.line_segment([top, egui::pos2(r.center().x, r.bottom() - r.height() * 0.3)], s);
    painter.line_segment([top, top + vec2(-r.width() * 0.25, r.height() * 0.25)], s);
    painter.line_segment([top, top + vec2(r.width() * 0.25, r.height() * 0.25)], s);
    painter.line_segment([r.left_bottom(), r.right_bottom()], s);
}

impl FileWatchersPage {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch);
        self.templates = dispatch("run.fileWatchers.templates", json!({}))
            .ok()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        self.types = zen_workbench::file_types::list().types.into_iter().map(|info| (info.id, info.name)).collect();
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("run.fileWatchers.list", json!({}))
            .and_then(|value| serde_json::from_value::<WatcherList>(value).map_err(|error| error.to_string()))
        {
            Ok(listed) => {
                self.has_project = listed.project_file.is_some();
                self.draft.clone_from(&listed.watchers);
                self.original = listed.watchers;
            }
            Err(error) => self.message = Some(error),
        }
        self.selected = self.selected.filter(|index| *index < self.draft.len());
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.draft == self.original {
            return;
        }
        match dispatch("run.fileWatchers.replace", json!({ "watchers": self.draft })) {
            Ok(_) => self.message = None,
            Err(error) => {
                self.message = Some(error);
                return;
            }
        }
        self.load(dispatch);
    }

    fn default_level(&self) -> Level {
        if self.has_project { Level::Project } else { Level::Global }
    }

    fn add(&mut self, watcher: FileWatcher) {
        let level = self.default_level();
        self.editor = Some(WatcherEditor::new(None, ListedWatcher { level, watcher }));
    }

    fn act(&mut self, tool: Tool, dispatch: &mut Dispatch<'_>) {
        let selected = self.selected.filter(|index| *index < self.draft.len());
        match (tool, selected) {
            (Tool::Remove, Some(index)) => {
                self.draft.remove(index);
                self.selected = (!self.draft.is_empty()).then(|| index.min(self.draft.len() - 1));
            }
            (Tool::Edit, Some(index)) => self.editor = Some(WatcherEditor::new(Some(index), self.draft[index].clone())),
            (Tool::Up | Tool::Down, Some(index)) => {
                if let Some(other) = neighbour(&self.draft, index, tool == Tool::Up) {
                    self.draft.swap(index, other);
                    self.selected = Some(other);
                }
            }
            (Tool::Copy, Some(index)) => {
                let original = &self.draft[index];
                let mut copy = original.clone();
                copy.watcher.id = String::new();
                copy.watcher.name = t!("settings-file-watchers-copy-name", name = original.watcher.name.as_str());
                self.draft.insert(index + 1, copy);
                self.selected = Some(index + 1);
            }
            (Tool::Import, _) => {
                if let Some(path) = pick_import() {
                    self.commit(dispatch);
                    let level = self.default_level();
                    if let Err(error) = dispatch("run.fileWatchers.import", json!({ "path": path, "level": level })) {
                        self.message = Some(error);
                    }
                    self.load(dispatch);
                }
            }
            (Tool::Export, _) => {
                if let Some(path) = pick_export() {
                    self.commit(dispatch);
                    if let Err(error) = dispatch("run.fileWatchers.export", json!({ "path": path })) {
                        self.message = Some(error);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, mut dispatch: Option<&mut Dispatch<'_>>) {
        ui.label(egui::RichText::new(t!("settings-file-watchers-about")).color(p.text_secondary));
        if ui.link(t!("settings-file-watchers-actions-on-save")).clicked() {
            self.actions_on_save = true;
        }
        ui.add_space(6.0);
        let selected = self.selected.is_some_and(|index| index < self.draft.len());
        let movable = |up: bool| self.selected.and_then(|index| neighbour(&self.draft, index, up)).is_some();
        let buttons: [(Tool, crate::ide::glyphs::Glyph, String, bool); 7] = [
            (Tool::Remove, glyph_minus, t!("settings-file-watchers-remove"), selected),
            (Tool::Edit, glyph_edit, t!("settings-file-watchers-edit"), selected),
            (Tool::Up, glyph_up, t!("settings-file-watchers-up"), movable(true)),
            (Tool::Down, glyph_down, t!("settings-file-watchers-down"), movable(false)),
            (Tool::Copy, glyph_copy, t!("settings-file-watchers-copy"), selected),
            (Tool::Import, glyph_import, t!("settings-file-watchers-import"), dispatch.is_some()),
            (
                Tool::Export,
                glyph_export,
                t!("settings-file-watchers-export"),
                dispatch.is_some() && !self.draft.is_empty(),
            ),
        ];
        let mut chosen = None;
        let mut added = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let plus = egui::Button::new("").min_size(vec2(24.0, 24.0)).frame(false);
            let response = ui.add(plus);
            glyph_plus(ui.painter(), response.rect.shrink(5.0), p.text);
            accessibility::button(&response, &t!("settings-file-watchers-add"));
            egui::Popup::menu(&response).show(|ui| {
                if ui.button(t!("settings-file-watchers-custom")).clicked() {
                    added = Some(FileWatcher::default());
                }
                for template in &self.templates {
                    if ui.button(&template.watcher.name).clicked() {
                        added = Some(template.watcher.clone());
                    }
                }
            });
            for (tool, glyph, name, enabled) in buttons {
                if tool_button(ui, p, glyph, &name, enabled) {
                    chosen = Some(tool);
                }
            }
        });
        if let Some(watcher) = added {
            self.add(watcher);
        }
        if let (Some(tool), Some(dispatch)) = (chosen, dispatch.as_deref_mut()) {
            self.act(tool, dispatch);
        }
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.diagnostic_error));
        }
        ui.add_space(4.0);
        egui::Frame::new().fill(p.island).corner_radius(4).inner_margin(4).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(220.0);
            if self.draft.is_empty() {
                ui.add_space(90.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(t!("settings-file-watchers-empty")).color(p.text_secondary));
                });
                return;
            }
            self.table(ui, p);
        });
        if let Some(dispatch) = dispatch {
            self.editor_dialog(ui, p, dispatch);
        }
    }

    fn table(&mut self, ui: &mut Ui, p: &IdePalette) {
        egui::Grid::new("file-watchers-table").num_columns(3).spacing([16.0, 4.0]).striped(false).show(ui, |ui| {
            for title in [
                t!("settings-file-watchers-enabled"),
                t!("settings-file-watchers-name"),
                t!("settings-file-watchers-level"),
            ] {
                ui.label(egui::RichText::new(title).font(theme::semibold(13.0)).color(p.text_secondary));
            }
            ui.end_row();
            for index in 0..self.draft.len() {
                let selected = self.selected == Some(index);
                let listed = &mut self.draft[index];
                let enabled = ui.checkbox(&mut listed.watcher.enabled, "");
                accessibility::labelled(&enabled, &listed.watcher.name);
                let text = egui::RichText::new(&listed.watcher.name).font(theme::regular(13.0)).color(p.text);
                let response = ui.add(egui::Button::selectable(selected, text).frame_when_inactive(false));
                ui.label(egui::RichText::new(level_name(listed.level)).color(p.text_secondary));
                ui.end_row();
                if response.clicked() {
                    self.selected = Some(index);
                }
                if response.double_clicked() {
                    self.selected = Some(index);
                    self.editor = Some(WatcherEditor::new(Some(index), self.draft[index].clone()));
                }
            }
        });
    }

    fn editor_dialog(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        if self.editor.is_none() {
            return;
        }
        if self.macros.is_empty() {
            self.macros = dispatch("run.externalTools.macros", json!({}))
                .ok()
                .and_then(|value| serde_json::from_value::<Vec<MacroInfo>>(value).ok())
                .unwrap_or_default();
        }
        let has_project = self.has_project;
        let types = &self.types;
        let macros = &self.macros;
        let Some(editor) = self.editor.as_mut() else { return };
        let title = if editor.index.is_some() {
            t!("settings-file-watchers-edit-title")
        } else {
            t!("settings-file-watchers-new-title")
        };
        let mut done = None;
        let modal =
            egui::Modal::new(ui.scope_id().with("file-watcher-editor")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(600.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(4.0);
                let focus = std::mem::take(&mut editor.focus);
                egui::Grid::new("file-watcher-fields").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(t!("settings-file-watchers-name"));
                    let name = ui.add(egui::TextEdit::singleline(&mut editor.watcher.name).desired_width(320.0));
                    if focus {
                        name.request_focus();
                    }
                    ui.end_row();
                    ui.label(t!("settings-file-watchers-level"));
                    egui::ComboBox::from_id_salt("file-watcher-level").selected_text(level_name(editor.level)).show_ui(
                        ui,
                        |ui| {
                            if has_project {
                                ui.selectable_value(&mut editor.level, Level::Project, level_name(Level::Project));
                            }
                            ui.selectable_value(&mut editor.level, Level::Global, level_name(Level::Global));
                        },
                    );
                    ui.end_row();
                    ui.label(t!("settings-file-watchers-file-type"));
                    let shown = types
                        .iter()
                        .find(|(id, _)| *id == editor.watcher.file_type)
                        .map_or_else(|| any_or(&editor.watcher.file_type), |(_, name)| name.clone());
                    egui::ComboBox::from_id_salt("file-watcher-type").selected_text(shown).show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut editor.watcher.file_type,
                            String::new(),
                            t!("settings-file-watchers-any-type"),
                        );
                        for (id, name) in types {
                            ui.selectable_value(&mut editor.watcher.file_type, id.clone(), name);
                        }
                    });
                    ui.end_row();
                    ui.label(t!("settings-file-watchers-scope"));
                    egui::ComboBox::from_id_salt("file-watcher-scope")
                        .selected_text(scope_name(editor.watcher.scope))
                        .show_ui(ui, |ui| {
                            for scope in [WatchScope::ProjectFiles, WatchScope::OpenFiles, WatchScope::ChangedFiles] {
                                ui.selectable_value(&mut editor.watcher.scope, scope, scope_name(scope));
                            }
                        });
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t!("settings-file-watchers-tool-to-run"))
                        .font(theme::semibold(13.0))
                        .color(p.text),
                );
                egui::Grid::new("file-watcher-tool").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                    for field in Field::ALL {
                        ui.label(field.label());
                        ui.add(egui::TextEdit::singleline(field.text(&mut editor.watcher)).desired_width(360.0));
                        let insert = ui.button("+").on_hover_text(t!("settings-file-watchers-insert-macro"));
                        egui::Popup::menu(&insert).show(|ui| {
                            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                                for info in macros {
                                    let label = format!("{} - {}", info.token.trim_matches('$'), info.description);
                                    if ui.button(label).clicked() {
                                        field.text(&mut editor.watcher).push_str(&info.token);
                                    }
                                }
                            });
                        });
                        ui.end_row();
                    }
                    ui.label(t!("settings-file-watchers-environment"));
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.environment)
                            .desired_width(360.0)
                            .hint_text("NAME=value;OTHER=value"),
                    );
                    ui.end_row();
                });
                ui.add_space(4.0);
                egui::CollapsingHeader::new(
                    egui::RichText::new(t!("settings-file-watchers-advanced"))
                        .font(theme::semibold(13.0))
                        .color(p.text),
                )
                .id_salt("file-watcher-advanced")
                .default_open(true)
                .show(ui, |ui| {
                    ui.checkbox(&mut editor.watcher.auto_save, t!("settings-file-watchers-auto-save"));
                    ui.checkbox(&mut editor.watcher.trigger_on_external, t!("settings-file-watchers-external"));
                    ui.checkbox(
                        &mut editor.watcher.trigger_on_syntax_errors,
                        t!("settings-file-watchers-syntax-errors"),
                    );
                    ui.checkbox(&mut editor.watcher.create_output_from_stdout, t!("settings-file-watchers-stdout"));
                    ui.horizontal(|ui| {
                        ui.label(t!("settings-file-watchers-show-console"));
                        egui::ComboBox::from_id_salt("file-watcher-console")
                            .selected_text(console_name(editor.watcher.show_console))
                            .show_ui(ui, |ui| {
                                for console in [ShowConsole::Always, ShowConsole::OnError, ShowConsole::Never] {
                                    ui.selectable_value(
                                        &mut editor.watcher.show_console,
                                        console,
                                        console_name(console),
                                    );
                                }
                            });
                    });
                    ui.label(t!("settings-file-watchers-filters"));
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.filters)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(2)
                            .desired_width(ui.available_width())
                            .hint_text("$FILE_PATH$:$LINE$:$COLUMN$: $MESSAGE$"),
                    );
                    ui.label(
                        egui::RichText::new(t!("settings-file-watchers-filters-note"))
                            .font(theme::regular(12.0))
                            .color(p.text_secondary),
                    );
                });
                if let Some(error) = &editor.error {
                    ui.label(egui::RichText::new(error).color(p.diagnostic_error));
                }
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if dialog_button(ui, p, t!("settings-ok"), true) {
                        done = Some(true);
                    }
                    if dialog_button(ui, p, t!("settings-cancel"), false) {
                        done = Some(false);
                    }
                });
            });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                let listed = editor.finished();
                match listed.watcher.validate() {
                    Err(error) => editor.error = Some(error),
                    Ok(()) => {
                        let index = editor.index;
                        self.editor = None;
                        match index.filter(|index| *index < self.draft.len()) {
                            Some(index) => self.draft[index] = listed,
                            None => {
                                self.draft.push(listed);
                                self.selected = Some(self.draft.len() - 1);
                            }
                        }
                    }
                }
            }
            Some(false) => self.editor = None,
            None => {}
        }
    }

    pub(super) fn enabled_names(&self) -> Vec<String> {
        self.draft.iter().filter(|listed| listed.watcher.enabled).map(|listed| listed.watcher.name.clone()).collect()
    }
}

fn any_or(id: &str) -> String {
    if id.is_empty() { t!("settings-file-watchers-any-type") } else { id.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(id: &str, level: Level) -> ListedWatcher {
        ListedWatcher {
            level,
            watcher: FileWatcher {
                id: id.to_owned(),
                name: id.to_uppercase(),
                program: "/bin/true".to_owned(),
                ..FileWatcher::default()
            },
        }
    }

    #[test]
    fn up_and_down_stay_within_a_level_and_copies_get_new_ids() {
        let mut page = FileWatchersPage {
            draft: vec![listed("a", Level::Project), listed("b", Level::Project), listed("c", Level::Global)],
            ..FileWatchersPage::default()
        };
        assert_eq!(neighbour(&page.draft, 1, false), None, "the global one is not below b");
        assert_eq!(neighbour(&page.draft, 1, true), Some(0));
        let mut dispatch = |_: &str, _: serde_json::Value| -> Result<serde_json::Value, String> { Ok(json!({})) };
        page.selected = Some(1);
        page.act(Tool::Up, &mut dispatch);
        let ids: Vec<&str> = page.draft.iter().map(|listed| listed.watcher.id.as_str()).collect();
        assert_eq!(ids, ["b", "a", "c"]);
        page.act(Tool::Copy, &mut dispatch);
        assert_eq!(page.draft[1].watcher.id, "");
        assert_eq!(page.draft[1].level, Level::Project);
        page.act(Tool::Remove, &mut dispatch);
        assert_eq!(page.draft.len(), 3);
        page.draft[2].watcher.enabled = false;
        assert_eq!(page.enabled_names(), ["B", "A"]);
    }

    #[test]
    fn the_environment_is_typed_as_pairs_separated_by_semicolons() {
        let parsed = parse_environment(" A=1 ; B=x=y;;C ");
        assert_eq!(
            parsed,
            [
                EnvVar { name: "A".to_owned(), value: "1".to_owned() },
                EnvVar { name: "B".to_owned(), value: "x=y".to_owned() },
                EnvVar { name: "C".to_owned(), value: String::new() },
            ]
        );
        assert_eq!(environment_text(&parsed), "A=1;B=x=y;C=");
    }
}
