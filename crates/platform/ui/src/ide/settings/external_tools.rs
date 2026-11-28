use egui::{Align2, Color32, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_workbench::external_tools::{DEFAULT_GROUP, ExternalTool, MacroInfo, Preview, ToolList};

use super::Dispatch;
use crate::accessibility;
use crate::ide::glyphs::{Glyph, glyph_minus, glyph_plus, stroke};
use crate::ide::{IdePalette, Side, dialog_frame, hint};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Program,
    Arguments,
    WorkingDirectory,
}

impl Field {
    const ALL: [Field; 3] = [Field::Program, Field::Arguments, Field::WorkingDirectory];

    fn label(self) -> String {
        match self {
            Field::Program => t!("settings-external-tools-program"),
            Field::Arguments => t!("settings-external-tools-arguments"),
            Field::WorkingDirectory => t!("settings-external-tools-working-directory"),
        }
    }

    fn text(self, tool: &mut ExternalTool) -> &mut String {
        match self {
            Field::Program => &mut tool.program,
            Field::Arguments => &mut tool.arguments,
            Field::WorkingDirectory => &mut tool.working_directory,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Add,
    Remove,
    Edit,
    Up,
    Down,
    Copy,
}

struct MacroPicker {
    field: Field,
    macros: Vec<MacroInfo>,
    selected: Option<usize>,
}

struct ToolEditor {
    index: Option<usize>,
    tool: ExternalTool,
    filters: String,
    preview: Option<(String, Preview)>,
    carets: [Option<usize>; 3],
    picker: Option<MacroPicker>,
    error: Option<String>,
    focus: bool,
}

impl ToolEditor {
    fn new(index: Option<usize>, tool: ExternalTool) -> Self {
        let filters = tool.output_filters.join("\n");
        Self { index, tool, filters, preview: None, carets: [None; 3], picker: None, error: None, focus: true }
    }

    fn finished(&self) -> ExternalTool {
        let mut tool = self.tool.clone();
        tool.name = tool.name.trim().to_owned();
        tool.group = match tool.group.trim() {
            "" => DEFAULT_GROUP.to_owned(),
            group => group.to_owned(),
        };
        tool.output_filters =
            self.filters.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect();
        tool
    }

    fn insert(&mut self, field: Field, token: &str) {
        let slot = Field::ALL.iter().position(|known| *known == field).unwrap_or_default();
        let text = field.text(&mut self.tool);
        let at = self.carets[slot].unwrap_or(usize::MAX).min(text.chars().count());
        let byte = text.char_indices().nth(at).map_or(text.len(), |(byte, _)| byte);
        text.insert_str(byte, token);
        self.carets[slot] = Some(at + token.chars().count());
    }
}

#[derive(Default)]
pub(super) struct ExternalToolsPage {
    original: Vec<ExternalTool>,
    draft: Vec<ExternalTool>,
    selected: Option<usize>,
    editor: Option<ToolEditor>,
    message: Option<String>,
}

fn group_of(tool: &ExternalTool) -> String {
    match tool.group.trim() {
        "" => DEFAULT_GROUP.to_owned(),
        group => group.to_owned(),
    }
}

fn groups(tools: &[ExternalTool]) -> Vec<String> {
    let mut groups: Vec<String> = Vec::new();
    for tool in tools {
        let group = group_of(tool);
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    groups
}

fn neighbour(tools: &[ExternalTool], index: usize, up: bool) -> Option<usize> {
    let group = group_of(tools.get(index)?);
    if up {
        tools[..index].iter().rposition(|tool| group_of(tool) == group)
    } else {
        tools[index + 1..].iter().position(|tool| group_of(tool) == group).map(|at| index + 1 + at)
    }
}

pub(super) fn glyph_up(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.12);
    painter.line_segment([r.center_bottom(), r.center_top()], s);
    painter.line_segment([r.center_top(), pos2(r.left() + r.width() * 0.2, r.top() + r.height() * 0.35)], s);
    painter.line_segment([r.center_top(), pos2(r.right() - r.width() * 0.2, r.top() + r.height() * 0.35)], s);
}

pub(super) fn glyph_down(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.12);
    painter.line_segment([r.center_top(), r.center_bottom()], s);
    painter.line_segment([r.center_bottom(), pos2(r.left() + r.width() * 0.2, r.bottom() - r.height() * 0.35)], s);
    painter.line_segment([r.center_bottom(), pos2(r.right() - r.width() * 0.2, r.bottom() - r.height() * 0.35)], s);
}

pub(super) fn glyph_edit(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.1);
    let tip = r.left_bottom();
    let end = r.right_top();
    let side = vec2(r.width() * 0.14, r.height() * 0.14);
    painter.line_segment([tip + vec2(side.x, 0.0), end + vec2(0.0, side.y)], s);
    painter.line_segment([tip - vec2(0.0, side.y), end - vec2(side.x, 0.0)], s);
    painter.line_segment([end - vec2(side.x, 0.0), end + vec2(0.0, side.y)], s);
    painter.line_segment([tip, tip + vec2(side.x, 0.0)], s);
    painter.line_segment([tip, tip - vec2(0.0, side.y)], s);
}

pub(super) fn glyph_copy(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let size = rect.size() * 0.7;
    painter.rect_stroke(Rect::from_min_size(rect.min, size), 2.0, s, egui::StrokeKind::Middle);
    painter.rect_stroke(Rect::from_min_size(rect.max - size, size), 2.0, s, egui::StrokeKind::Middle);
}

pub(super) fn tool_button(ui: &mut Ui, p: &IdePalette, glyph: Glyph, name: &str, enabled: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4, p.popup_hover);
    }
    glyph(ui.painter(), rect.shrink(5.0), if enabled { p.text } else { p.gutter_text });
    hint(ui, &response, name, None, Side::Below);
    accessibility::button(&response, name);
    if !enabled {
        accessibility::disabled(&response);
    }
    enabled && response.clicked()
}

pub(super) fn dialog_button(ui: &mut Ui, p: &IdePalette, label: String, primary: bool) -> bool {
    let response = if primary {
        crate::ide::vcs::primary_button(ui, p, &label)
    } else {
        crate::ide::vcs::secondary_button(ui, p, &label)
    };
    response.clicked()
}

impl ExternalToolsPage {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch);
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("run.externalTools.list", json!({}))
            .and_then(|value| serde_json::from_value::<ToolList>(value).map_err(|error| error.to_string()))
        {
            Ok(listed) => {
                self.draft.clone_from(&listed.tools);
                self.original = listed.tools;
            }
            Err(error) => self.message = Some(error),
        }
        self.selected = self.selected.filter(|index| *index < self.draft.len());
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.draft == self.original {
            return;
        }
        let selected = self.selected.and_then(|index| self.draft.get(index)).map(|tool| tool.name.clone());
        match dispatch("run.externalTools.replace", json!({ "tools": self.draft })) {
            Ok(_) => self.message = None,
            Err(error) => {
                self.message = Some(error);
                return;
            }
        }
        self.load(dispatch);
        self.selected = selected.and_then(|name| self.draft.iter().position(|tool| tool.name == name));
    }

    fn act(&mut self, tool: Tool) {
        let selected = self.selected.filter(|index| *index < self.draft.len());
        match (tool, selected) {
            (Tool::Add, _) => {
                let group = selected.map_or_else(|| DEFAULT_GROUP.to_owned(), |index| group_of(&self.draft[index]));
                self.editor = Some(ToolEditor::new(None, ExternalTool { group, ..ExternalTool::default() }));
            }
            (Tool::Remove, Some(index)) => {
                self.draft.remove(index);
                self.selected = (!self.draft.is_empty()).then(|| index.min(self.draft.len() - 1));
            }
            (Tool::Edit, Some(index)) => self.editor = Some(ToolEditor::new(Some(index), self.draft[index].clone())),
            (Tool::Up | Tool::Down, Some(index)) => {
                if let Some(other) = neighbour(&self.draft, index, tool == Tool::Up) {
                    self.draft.swap(index, other);
                    self.selected = Some(other);
                }
            }
            (Tool::Copy, Some(index)) => {
                let original = &self.draft[index];
                let copy = ExternalTool {
                    id: String::new(),
                    name: t!("settings-external-tools-copy-name", name = original.name.as_str()),
                    ..original.clone()
                };
                self.draft.insert(index + 1, copy);
                self.selected = Some(index + 1);
            }
            _ => {}
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: Option<&mut Dispatch<'_>>) {
        let selected = self.selected.is_some_and(|index| index < self.draft.len());
        let movable = |up: bool| self.selected.and_then(|index| neighbour(&self.draft, index, up)).is_some();
        let buttons: [(Tool, Glyph, String, bool); 6] = [
            (Tool::Add, glyph_plus, t!("settings-external-tools-add"), true),
            (Tool::Remove, glyph_minus, t!("settings-external-tools-remove"), selected),
            (Tool::Edit, glyph_edit, t!("settings-external-tools-edit"), selected),
            (Tool::Up, glyph_up, t!("settings-external-tools-up"), movable(true)),
            (Tool::Down, glyph_down, t!("settings-external-tools-down"), movable(false)),
            (Tool::Copy, glyph_copy, t!("settings-external-tools-copy"), selected),
        ];
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (tool, glyph, name, enabled) in buttons {
                if tool_button(ui, p, glyph, &name, enabled) {
                    chosen = Some(tool);
                }
            }
        });
        if let Some(tool) = chosen {
            self.act(tool);
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
                    ui.label(egui::RichText::new(t!("settings-external-tools-empty")).color(p.text_secondary));
                });
                return;
            }
            self.list(ui, p);
        });
        if let Some(dispatch) = dispatch {
            self.editor_dialog(ui, p, dispatch);
        }
    }

    fn list(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        for group in groups(&self.draft) {
            let members: Vec<usize> =
                (0..self.draft.len()).filter(|index| group_of(&self.draft[*index]) == group).collect();
            ui.horizontal(|ui| {
                let on = members.iter().filter(|index| self.draft[**index].enabled).count();
                let mut all = on == members.len();
                let response = ui.add(egui::Checkbox::new(&mut all, "").indeterminate(on > 0 && on < members.len()));
                accessibility::labelled(&response, &group);
                if response.changed() {
                    for index in &members {
                        self.draft[*index].enabled = all;
                    }
                }
                ui.label(egui::RichText::new(&group).font(theme::semibold(13.0)).color(p.text));
            });
            for index in members {
                let selected = self.selected == Some(index);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let tool = &mut self.draft[index];
                    let enabled = ui.checkbox(&mut tool.enabled, "");
                    accessibility::labelled(&enabled, &tool.name);
                    let mut text = egui::text::LayoutJob::default();
                    text.append(&tool.name, 0.0, egui::TextFormat::simple(theme::regular(13.0), p.text));
                    if !tool.description.trim().is_empty() {
                        let format = egui::TextFormat::simple(theme::regular(13.0), p.text_secondary);
                        text.append(&format!("  {}", tool.description.trim()), 0.0, format);
                    }
                    let response = ui.add(egui::Button::selectable(selected, text).frame_when_inactive(false));
                    if response.clicked() {
                        self.selected = Some(index);
                    }
                    if response.double_clicked() {
                        self.selected = Some(index);
                        self.editor = Some(ToolEditor::new(Some(index), self.draft[index].clone()));
                    }
                });
            }
        }
    }

    fn editor_dialog(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let Some(editor) = self.editor.as_mut() else { return };
        let key =
            format!("{}\u{1f}{}\u{1f}{}", editor.tool.program, editor.tool.arguments, editor.tool.working_directory);
        if editor.preview.as_ref().is_none_or(|(known, _)| *known != key) {
            let params = json!({
                "program": editor.tool.program,
                "arguments": editor.tool.arguments,
                "workingDirectory": editor.tool.working_directory,
            });
            let preview = dispatch("run.externalTools.preview", params)
                .ok()
                .and_then(|value| serde_json::from_value::<Preview>(value).ok())
                .unwrap_or_default();
            editor.preview = Some((key, preview));
        }
        let title = if editor.index.is_some() {
            t!("settings-external-tools-edit-title")
        } else {
            t!("settings-external-tools-create-title")
        };
        let mut done = None;
        let mut pick = None;
        let modal =
            egui::Modal::new(ui.scope_id().with("external-tool-editor")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(560.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(4.0);
                let focus = std::mem::take(&mut editor.focus);
                egui::Grid::new("external-tool-fields").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(t!("settings-external-tools-name"));
                    let name = ui.add(egui::TextEdit::singleline(&mut editor.tool.name).desired_width(300.0));
                    if focus {
                        name.request_focus();
                    }
                    ui.end_row();
                    ui.label(t!("settings-external-tools-group"));
                    ui.add(egui::TextEdit::singleline(&mut editor.tool.group).desired_width(300.0));
                    ui.end_row();
                    ui.label(t!("settings-external-tools-description"));
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.tool.description).desired_width(ui.available_width()),
                    );
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t!("settings-external-tools-tool-settings"))
                        .font(theme::semibold(13.0))
                        .color(p.text),
                );
                let preview = editor.preview.as_ref().map(|(_, preview)| preview.clone()).unwrap_or_default();
                egui::Grid::new("external-tool-settings").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (slot, field) in Field::ALL.into_iter().enumerate() {
                        ui.label(field.label());
                        let output =
                            egui::TextEdit::singleline(field.text(&mut editor.tool)).desired_width(330.0).show(ui);
                        if let Some(range) = output.cursor_range {
                            editor.carets[slot] = Some(range.primary.index.0);
                        }
                        if ui.button("+").on_hover_text(t!("settings-external-tools-insert-macro")).clicked() {
                            pick = Some(field);
                        }
                        ui.end_row();
                        ui.label("");
                        let shown = match field {
                            Field::Program => preview.program.clone(),
                            Field::Arguments => preview.arguments.join(" "),
                            Field::WorkingDirectory => preview.working_directory.clone(),
                        };
                        ui.label(egui::RichText::new(shown).font(theme::regular(12.0)).color(p.text_secondary));
                        ui.end_row();
                    }
                });
                if !preview.missing.is_empty() {
                    let missing = preview.missing.join(", ");
                    ui.label(
                        egui::RichText::new(t!("settings-external-tools-preview-missing", macros = missing))
                            .font(theme::regular(12.0))
                            .color(p.text_secondary),
                    );
                }
                ui.add_space(4.0);
                egui::CollapsingHeader::new(
                    egui::RichText::new(t!("settings-external-tools-advanced"))
                        .font(theme::semibold(13.0))
                        .color(p.text),
                )
                .id_salt("external-tool-advanced")
                .default_open(false)
                .show(ui, |ui| {
                    ui.checkbox(&mut editor.tool.synchronize, t!("settings-external-tools-synchronize"));
                    ui.checkbox(&mut editor.tool.open_console, t!("settings-external-tools-open-console"));
                    ui.add_enabled_ui(editor.tool.open_console, |ui| {
                        ui.indent("external-tool-activate", |ui| {
                            ui.checkbox(&mut editor.tool.activate_on_stdout, t!("settings-external-tools-stdout"));
                            ui.checkbox(&mut editor.tool.activate_on_stderr, t!("settings-external-tools-stderr"));
                        });
                    });
                    ui.label(t!("settings-external-tools-filters"));
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.filters)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(2)
                            .desired_width(ui.available_width())
                            .hint_text("$FILE_PATH$:$LINE$:$COLUMN$"),
                    );
                    ui.label(
                        egui::RichText::new(t!("settings-external-tools-filters-note"))
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
        if let Some(field) = pick {
            let macros = dispatch("run.externalTools.macros", json!({}))
                .ok()
                .and_then(|value| serde_json::from_value::<Vec<MacroInfo>>(value).ok())
                .unwrap_or_default();
            editor.picker = Some(MacroPicker { field, macros, selected: None });
        }
        if editor.picker.is_some() {
            macro_picker(ui, p, editor);
            return;
        }
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                let tool = editor.finished();
                if tool.name.is_empty() {
                    editor.error = Some(t!("settings-external-tools-no-name"));
                } else if tool.program.trim().is_empty() {
                    editor.error = Some(t!("settings-external-tools-no-program"));
                } else {
                    let index = editor.index;
                    self.editor = None;
                    match index.filter(|index| *index < self.draft.len()) {
                        Some(index) => self.draft[index] = tool,
                        None => {
                            let at = self
                                .draft
                                .iter()
                                .rposition(|known| group_of(known) == group_of(&tool))
                                .map_or(self.draft.len(), |at| at + 1);
                            self.draft.insert(at, tool);
                            self.selected = Some(at);
                        }
                    }
                }
            }
            Some(false) => self.editor = None,
            None => {}
        }
    }
}

fn macro_picker(ui: &mut Ui, p: &IdePalette, editor: &mut ToolEditor) {
    let Some(picker) = editor.picker.as_mut() else { return };
    let mut chosen = None;
    let mut close = false;
    let modal =
        egui::Modal::new(ui.scope_id().with("external-tool-macros")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            ui.label(
                egui::RichText::new(t!("settings-external-tools-macros-title"))
                    .font(theme::semibold(15.0))
                    .color(p.text),
            );
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("external-tool-macros-list").max_height(300.0).show(ui, |ui| {
                for (index, info) in picker.macros.iter().enumerate() {
                    let text = egui::RichText::new(format!("{} - {}", info.token.trim_matches('$'), info.description))
                        .font(theme::regular(13.0))
                        .color(p.text);
                    let response = ui
                        .add(egui::Button::selectable(picker.selected == Some(index), text).frame_when_inactive(false));
                    if response.clicked() {
                        picker.selected = Some(index);
                    }
                    if response.double_clicked() {
                        chosen = Some(info.token.clone());
                    }
                }
            });
            ui.add_space(4.0);
            let preview = picker
                .selected
                .and_then(|index| picker.macros.get(index))
                .map(|info| info.value.clone().or_else(|| info.missing.clone()).unwrap_or_default());
            ui.label(t!("settings-external-tools-macro-preview"));
            let rect = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover()).0;
            ui.painter().rect_filled(rect, 4, p.island);
            ui.painter().text(
                pos2(rect.left() + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                preview.unwrap_or_default(),
                theme::regular(12.0),
                p.text_secondary,
            );
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if dialog_button(ui, p, t!("settings-external-tools-insert"), true) {
                    chosen = picker.selected.and_then(|index| picker.macros.get(index)).map(|info| info.token.clone());
                    close = true;
                }
                if dialog_button(ui, p, t!("settings-cancel"), false) {
                    close = true;
                }
            });
        });
    let field = picker.field;
    if let Some(token) = chosen {
        editor.insert(field, &token);
        editor.picker = None;
    } else if close || modal.should_close() {
        editor.picker = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: &str, group: &str) -> ExternalTool {
        ExternalTool {
            id: id.to_owned(),
            name: id.to_uppercase(),
            group: group.to_owned(),
            program: "/usr/bin/true".to_owned(),
            ..ExternalTool::default()
        }
    }

    #[test]
    fn the_toolbar_moves_copies_and_removes_within_a_group() {
        let mut page = ExternalToolsPage {
            draft: vec![tool("a", "Lint"), tool("b", ""), tool("c", "Lint"), tool("d", DEFAULT_GROUP)],
            ..ExternalToolsPage::default()
        };
        assert_eq!(groups(&page.draft), ["Lint", DEFAULT_GROUP]);
        page.selected = Some(2);
        page.act(Tool::Up);
        let ids: Vec<&str> = page.draft.iter().map(|tool| tool.id.as_str()).collect();
        assert_eq!(ids, ["c", "b", "a", "d"], "c swaps with a, the tool above it in Lint");
        assert_eq!(page.selected, Some(0));
        assert_eq!(neighbour(&page.draft, 0, true), None);
        page.selected = Some(1);
        page.act(Tool::Down);
        assert_eq!(page.draft[3].id, "b", "the default group holds both the empty group and its name");
        page.act(Tool::Copy);
        assert_eq!(page.draft.len(), 5);
        assert_eq!(page.draft[4].id, "", "a copy gets its id when it is saved");
        assert!(page.draft[4].name.contains("B"));
        page.act(Tool::Remove);
        assert_eq!(page.draft.len(), 4);
        page.act(Tool::Add);
        assert_eq!(page.editor.as_ref().map(|editor| editor.tool.group.as_str()), Some(DEFAULT_GROUP));
    }

    #[test]
    fn the_editor_inserts_macros_at_the_caret_and_tidies_what_it_saves() {
        let mut editor =
            ToolEditor::new(None, ExternalTool { arguments: "-v  -x".to_owned(), ..ExternalTool::default() });
        editor.carets[1] = Some(3);
        editor.insert(Field::Arguments, "$FilePath$");
        assert_eq!(editor.tool.arguments, "-v $FilePath$ -x");
        editor.insert(Field::Program, "$GOROOT$");
        assert_eq!(editor.tool.program, "$GOROOT$");
        editor.tool.name = "  Vet  ".to_owned();
        editor.tool.group = " ".to_owned();
        editor.filters = "\n$FILE_PATH$:$LINE$\n  \n".to_owned();
        let tool = editor.finished();
        assert_eq!((tool.name.as_str(), tool.group.as_str()), ("Vet", DEFAULT_GROUP));
        assert_eq!(tool.output_filters, ["$FILE_PATH$:$LINE$"]);
    }
}
