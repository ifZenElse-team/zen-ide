use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_i18n::t;

use super::IdePalette;
use super::find_window::FindAction;
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const BAR: f32 = 44.0;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct PreviewChange {
    pub path: PathBuf,
    pub start: usize,
    pub line: u32,
    pub preview: Option<String>,
    pub highlight: Option<[usize; 2]>,
    pub new_text: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct PreviewFile {
    pub path: PathBuf,
    pub relative: String,
    pub changes: Vec<PreviewChange>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct PreviewOperation {
    pub kind: String,
    pub path: PathBuf,
    pub relative: String,
    pub new_relative: Option<String>,
}

impl PreviewOperation {
    pub(super) fn label(&self) -> String {
        match (self.kind.as_str(), &self.new_relative) {
            ("rename", Some(to)) => t!("ide-refactoring-file-rename", from = self.relative.as_str(), to = to.as_str()),
            ("delete", _) => t!("ide-refactoring-file-delete", path = self.relative.as_str()),
            _ => t!("ide-refactoring-file-create", path = self.relative.as_str()),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct RenamePreview {
    pub pending: bool,
    pub files: Vec<PreviewFile>,
    pub operations: Vec<PreviewOperation>,
    pub usage_count: usize,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewRow {
    Usages,
    File(usize),
    Change(usize, usize),
    Operations,
    Operation(usize),
}

#[derive(Debug, Default)]
pub(super) struct PreviewTab {
    pub apply: (String, Value),
    pub cancel: (String, Value),
    pub heading: Option<String>,
    pub conflicts: bool,
    pub preview: RenamePreview,
    usages_collapsed: bool,
    operations_collapsed: bool,
    collapsed: BTreeSet<usize>,
    selected: Option<PreviewRow>,
    reveal: bool,
    rows: Vec<(PreviewRow, Rect)>,
    buttons: Vec<Rect>,
}

impl PreviewTab {
    pub(super) fn new(new_name: String, path: PathBuf, offset: usize, preview: RenamePreview) -> Self {
        let apply =
            ("refactor.rename.apply".to_owned(), json!({ "newName": new_name, "path": path, "offset": offset }));
        Self::with_commands(preview, apply, ("refactor.rename.cancel".to_owned(), json!({})))
    }

    pub(super) fn with_commands(preview: RenamePreview, apply: (String, Value), cancel: (String, Value)) -> Self {
        let selected = preview.files.first().filter(|file| !file.changes.is_empty()).map(|_| PreviewRow::Change(0, 0));
        Self { apply, cancel, preview, selected, reveal: true, ..Self::default() }
    }

    pub(super) fn title(&self) -> String {
        t!("ide-refactoring-preview-title")
    }

    pub(super) fn summary(&self) -> String {
        match &self.heading {
            Some(heading) => t!(
                "ide-refactoring-heading",
                heading = heading.as_str(),
                count = self.preview.usage_count,
                files = self.preview.files.len()
            ),
            None => t!("ide-refactoring-usages", count = self.preview.usage_count, files = self.preview.files.len()),
        }
    }

    #[cfg(test)]
    pub(super) fn drawn(&self) -> (Option<PreviewRow>, &[(PreviewRow, Rect)], &[Rect]) {
        (self.selected, &self.rows, &self.buttons)
    }

    fn visible(&self) -> Vec<PreviewRow> {
        let mut rows = vec![PreviewRow::Usages];
        if !self.usages_collapsed {
            for (index, file) in self.preview.files.iter().enumerate() {
                rows.push(PreviewRow::File(index));
                if !self.collapsed.contains(&index) {
                    rows.extend((0..file.changes.len()).map(|change| PreviewRow::Change(index, change)));
                }
            }
        }
        if !self.preview.operations.is_empty() {
            rows.push(PreviewRow::Operations);
            if !self.operations_collapsed {
                rows.extend((0..self.preview.operations.len()).map(PreviewRow::Operation));
            }
        }
        rows
    }

    fn operations_summary(&self) -> String {
        t!("ide-refactoring-file-operations", count = self.preview.operations.len())
    }

    fn go_to(&self, row: PreviewRow, focus_editor: bool) -> Option<FindAction> {
        let PreviewRow::Change(file, change) = row else { return None };
        let change = self.preview.files.get(file)?.changes.get(change)?;
        Some(FindAction::GoTo { path: change.path.clone(), offset: change.start, focus_editor })
    }

    fn toggle(&mut self, row: PreviewRow) {
        match row {
            PreviewRow::Usages => self.usages_collapsed = !self.usages_collapsed,
            PreviewRow::Operations => self.operations_collapsed = !self.operations_collapsed,
            PreviewRow::File(file) => {
                if !self.collapsed.remove(&file) {
                    self.collapsed.insert(file);
                }
            }
            PreviewRow::Change(..) | PreviewRow::Operation(_) => {}
        }
    }

    fn refactor(&self) -> FindAction {
        FindAction::Refactor { command: self.apply.0.clone(), params: self.apply.1.clone() }
    }

    fn cancel_action(&self) -> FindAction {
        FindAction::CancelRefactoring { command: self.cancel.0.clone(), params: self.cancel.1.clone() }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, focus: &mut bool, actions: &mut Vec<FindAction>) {
        let full = ui.available_rect_before_wrap();
        let tree = Rect::from_min_max(full.min, pos2(full.right(), (full.bottom() - BAR).max(full.top())));
        let bar = Rect::from_min_max(pos2(full.left(), tree.bottom()), full.max);
        let id = ui.make_persistent_id("refactoring-preview-tree");
        let area = ui.interact(tree, id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || self.summary());
        if std::mem::take(focus) || area.clicked() {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        let focused = ui.memory(|memory| memory.has_focus(id));
        if focused {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
            self.keys(ui, actions);
        }
        let mut tree_ui = ui.new_child(egui::UiBuilder::new().max_rect(tree));
        self.tree(&mut tree_ui, p, focused, id, actions);
        let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(12.0, 7.0))));
        self.bar(&mut bar_ui, p, actions);
        ui.advance_cursor_after_rect(full);
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, focused: bool, id: egui::Id, actions: &mut Vec<FindAction>) {
        let rows = self.visible();
        let line_width = self
            .preview
            .files
            .iter()
            .flat_map(|file| &file.changes)
            .map(|change| change.line)
            .max()
            .map_or(1, |line| line.to_string().len()) as f32
            * 7.5;
        let mut clicked: Option<(PreviewRow, bool, bool)> = None;
        let mut drawn = Vec::with_capacity(rows.len());
        egui::ScrollArea::vertical().id_salt("refactoring-preview-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in rows {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                drawn.push((row, rect));
                if self.selected == Some(row) {
                    let fill = if focused { p.selection } else { p.hover };
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 4, fill);
                    if std::mem::take(&mut self.reveal) {
                        response.scroll_to_me(None);
                    }
                } else if response.hovered() {
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 4, p.hover);
                }
                let indent = match row {
                    PreviewRow::Usages | PreviewRow::Operations => 0.0,
                    PreviewRow::File(_) | PreviewRow::Operation(_) => 20.0,
                    PreviewRow::Change(..) => 40.0,
                };
                let chevron =
                    Rect::from_center_size(pos2(rect.left() + 18.0 + indent, rect.center().y), Vec2::splat(12.0));
                let selected = self.selected == Some(row);
                match row {
                    PreviewRow::Usages => {
                        accessibility::name_with(&response, egui::Role::TreeItem, || self.summary());
                        accessibility::edit(&response, |node| {
                            node.set_level(1);
                            node.set_expanded(!self.usages_collapsed);
                            node.set_selected(selected);
                        });
                    }
                    PreviewRow::File(index) => {
                        let expanded = Some(!self.collapsed.contains(&index));
                        accessibility::tree_item(&response, 2, expanded, selected, &self.preview.files[index].relative);
                    }
                    PreviewRow::Operations => {
                        accessibility::name_with(&response, egui::Role::TreeItem, || self.operations_summary());
                        accessibility::edit(&response, |node| {
                            node.set_level(1);
                            node.set_expanded(!self.operations_collapsed);
                            node.set_selected(selected);
                        });
                    }
                    PreviewRow::Operation(index) => {
                        let label = self.preview.operations[index].label();
                        accessibility::tree_item(&response, 2, None, selected, &label);
                    }
                    PreviewRow::Change(file, change) => {
                        let found = &self.preview.files[file].changes[change];
                        accessibility::name_with(&response, egui::Role::TreeItem, || match found.preview.as_deref() {
                            Some(text) if !text.trim().is_empty() => format!("{}: {}", found.line, text.trim()),
                            _ => found.line.to_string(),
                        });
                        accessibility::edit(&response, |node| {
                            node.set_level(3);
                            node.set_selected(selected);
                        });
                    }
                }
                match row {
                    PreviewRow::Usages => self.usages_row(ui, p, rect, chevron),
                    PreviewRow::Operations => self.operations_row(ui, p, rect, chevron),
                    PreviewRow::Operation(index) => self.operation_row(ui, p, rect, chevron, index),
                    PreviewRow::File(index) => self.file_row(ui, p, rect, chevron, index),
                    PreviewRow::Change(file, change) => {
                        self.change_row(ui, p, rect, indent, line_width, file, change);
                    }
                }
                if response.clicked() || response.double_clicked() {
                    let at = response.interact_pointer_pos();
                    let on_chevron = at.is_some_and(|at| at.x < chevron.right() + 4.0);
                    clicked = Some((row, response.double_clicked(), on_chevron));
                }
            }
        });
        self.rows = drawn;
        if let Some((row, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            self.selected = Some(row);
            match row {
                PreviewRow::Change(..) => actions.extend(self.go_to(row, double)),
                _ if double || on_chevron => self.toggle(row),
                _ => {}
            }
        }
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<FindAction>) {
        let none = Modifiers::NONE;
        let (up, down, left, right, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Enter),
                input.consume_key(none, Key::Escape),
            )
        });
        let rows = self.visible();
        let at = self.selected.and_then(|selected| rows.iter().position(|row| *row == selected));
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(rows.len().saturating_sub(1)),
            };
            self.selected = rows.get(next).copied();
            self.reveal = true;
        }
        let collapsed = |tab: &Self, row: PreviewRow| match row {
            PreviewRow::Usages => tab.usages_collapsed,
            PreviewRow::Operations => tab.operations_collapsed,
            PreviewRow::File(file) => tab.collapsed.contains(&file),
            PreviewRow::Change(..) | PreviewRow::Operation(_) => false,
        };
        match self.selected {
            Some(row @ PreviewRow::Change(file, _)) => {
                if left {
                    self.selected = Some(PreviewRow::File(file));
                    self.reveal = true;
                } else if enter {
                    actions.extend(self.go_to(row, true));
                }
            }
            Some(row) if (left && !collapsed(self, row)) || (right && collapsed(self, row)) || enter => {
                self.toggle(row);
            }
            _ => {}
        }
        if escape {
            actions.push(FindAction::FocusEditor);
        }
    }

    fn chevron(&self, ui: &Ui, p: &IdePalette, chevron: Rect, collapsed: bool) {
        if collapsed {
            crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
        } else {
            crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
        }
    }

    fn usages_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, chevron: Rect) {
        self.chevron(ui, p, chevron, self.usages_collapsed);
        ui.painter().with_clip_rect(rect).text(
            pos2(chevron.right() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            self.summary(),
            theme::regular(13.0),
            p.text,
        );
    }

    fn operations_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, chevron: Rect) {
        self.chevron(ui, p, chevron, self.operations_collapsed);
        ui.painter().with_clip_rect(rect).text(
            pos2(chevron.right() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            self.operations_summary(),
            theme::regular(13.0),
            p.text,
        );
    }

    fn operation_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, chevron: Rect, index: usize) {
        let operation = &self.preview.operations[index];
        crate::file_icons::file_icon_at(
            ui.painter(),
            Rect::from_center_size(pos2(chevron.right() + 14.0, rect.center().y), Vec2::splat(15.0)),
            &operation.path,
        );
        ui.painter().with_clip_rect(rect).text(
            pos2(chevron.right() + 28.0, rect.center().y),
            Align2::LEFT_CENTER,
            operation.label(),
            theme::regular(13.0),
            p.text,
        );
    }

    fn file_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, chevron: Rect, index: usize) {
        let file = &self.preview.files[index];
        self.chevron(ui, p, chevron, self.collapsed.contains(&index));
        crate::file_icons::file_icon_at(
            ui.painter(),
            Rect::from_center_size(pos2(chevron.right() + 14.0, rect.center().y), Vec2::splat(15.0)),
            &file.path,
        );
        let painter = ui.painter().with_clip_rect(rect);
        let label = painter.text(
            pos2(chevron.right() + 28.0, rect.center().y),
            Align2::LEFT_CENTER,
            &file.relative,
            theme::regular(13.0),
            p.text,
        );
        painter.text(
            pos2(label.right() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            t!("ide-find-usages-count", count = file.changes.len()),
            theme::regular(12.5),
            p.text_secondary,
        );
    }

    #[expect(clippy::too_many_arguments, reason = "a row is drawn from the tab, its place and its indices")]
    fn change_row(
        &self,
        ui: &Ui,
        p: &IdePalette,
        rect: Rect,
        indent: f32,
        line_width: f32,
        file: usize,
        change: usize,
    ) {
        let change = &self.preview.files[file].changes[change];
        let painter = ui.painter().with_clip_rect(rect);
        let left = rect.left() + 32.0 + indent;
        painter.text(
            pos2(left + line_width, rect.center().y),
            Align2::RIGHT_CENTER,
            change.line.to_string(),
            theme::regular(12.5),
            p.text_secondary,
        );
        let preview = change.preview.clone().unwrap_or_default();
        let galley = painter.layout_job(change_job(&preview, change.highlight, &change.new_text, !self.conflicts, p));
        painter.galley(pos2(left + line_width + 12.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    }

    fn bar(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<FindAction>) {
        self.buttons.clear();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let refactor = egui::Button::new(
                egui::RichText::new(t!("ide-refactoring-do")).font(theme::regular(13.0)).color(p.accent_text),
            )
            .min_size(vec2(96.0, 28.0))
            .corner_radius(6)
            .fill(p.accent);
            let response = ui.add(refactor);
            self.buttons.push(response.rect);
            if response.clicked() {
                actions.push(self.refactor());
            }
            let cancel = egui::Button::new(
                egui::RichText::new(t!("ide-refactoring-cancel")).font(theme::regular(13.0)).color(p.text),
            )
            .min_size(vec2(84.0, 28.0))
            .corner_radius(6)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, p.button_border));
            let response = ui.add(cancel);
            self.buttons.push(response.rect);
            if response.clicked() {
                actions.push(self.cancel_action());
            }
        });
    }
}

fn change_job(
    preview: &str,
    highlight: Option<[usize; 2]>,
    new_text: &str,
    replaced: bool,
    p: &IdePalette,
) -> LayoutJob {
    let chars = preview.chars().count();
    let [from, to] =
        highlight.map_or([chars, chars], |[from, to]| [from.min(chars), to.min(chars).max(from.min(chars))]);
    let byte = |index: usize| preview.char_indices().nth(index).map_or(preview.len(), |(byte, _)| byte);
    let plain = TextFormat { font_id: theme::regular(13.0), color: p.text, ..TextFormat::default() };
    let old = if replaced {
        TextFormat {
            font_id: theme::regular(13.0),
            color: p.text_secondary,
            background: p.search,
            strikethrough: Stroke::new(1.0, p.text_secondary),
            ..TextFormat::default()
        }
    } else {
        TextFormat { font_id: theme::regular(13.0), color: p.text, background: p.search, ..TextFormat::default() }
    };
    let new = TextFormat { font_id: theme::bold(13.0), color: p.text, ..TextFormat::default() };
    let mut job = LayoutJob::default();
    job.append(&preview[..byte(from)], 0.0, plain.clone());
    job.append(&preview[byte(from)..byte(to)], 0.0, old);
    job.append(new_text, 0.0, new);
    job.append(&preview[byte(to)..], 0.0, plain);
    job
}
