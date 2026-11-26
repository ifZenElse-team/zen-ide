use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{Align2, Key, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::ChangeInfo;
use zen_workbench::vcs::commit::DiffView;
use zen_workbench::vcs::shelf::ShelfInfo;

use super::{CommitTab, checkbox, status_named};
use crate::ide::vcs::{primary_button, secondary_button, status_color};
use crate::ide::{IdePalette, IdeView, dialog_frame};
use crate::theme;

#[derive(Clone, Debug)]
pub(in crate::ide) struct DialogFile {
    pub(super) path: PathBuf,
    pub(super) relative: String,
    pub(super) status: String,
}

impl From<&ChangeInfo> for DialogFile {
    fn from(change: &ChangeInfo) -> Self {
        DialogFile { path: change.path.clone(), relative: change.relative.clone(), status: change.status.clone() }
    }
}

#[derive(Debug, Default)]
pub(in crate::ide) struct FilePicker {
    files: Vec<DialogFile>,
    checked: BTreeSet<PathBuf>,
    selected: Option<PathBuf>,
    preview: Option<DiffView>,
    stale: bool,
}

impl FilePicker {
    pub(super) fn new(files: Vec<DialogFile>, checked: BTreeSet<PathBuf>) -> Self {
        let selected = files.iter().find(|file| checked.contains(&file.path)).or(files.first()).map(|f| f.path.clone());
        FilePicker { files, checked, selected, preview: None, stale: true }
    }

    pub(super) fn checked(&self) -> Vec<PathBuf> {
        self.files.iter().filter(|file| self.checked.contains(&file.path)).map(|file| file.path.clone()).collect()
    }
}

#[derive(Debug)]
pub(in crate::ide) enum ChangesDialog {
    NewChangelist {
        name: String,
        comment: String,
        active: bool,
        paths: Vec<PathBuf>,
        focused: bool,
        error: Option<String>,
    },
    EditChangelist {
        old: String,
        name: String,
        comment: String,
        active: bool,
        focused: bool,
        error: Option<String>,
    },
    Move {
        paths: Vec<PathBuf>,
        lists: Vec<String>,
        target: Option<String>,
        new_name: String,
        error: Option<String>,
    },
    Shelve {
        name: String,
        picker: FilePicker,
        focused: bool,
        error: Option<String>,
    },
    Unshelve {
        shelf: ShelfInfo,
        picker: FilePicker,
        lists: Vec<String>,
        target: String,
        remove: bool,
        error: Option<String>,
    },
    RenameShelf {
        id: u64,
        name: String,
        focused: bool,
        error: Option<String>,
    },
    DeleteShelf {
        id: u64,
        name: String,
        error: Option<String>,
    },
}

impl ChangesDialog {
    pub(in crate::ide) fn name(&self) -> &'static str {
        match self {
            ChangesDialog::NewChangelist { .. } => "newChangelist",
            ChangesDialog::EditChangelist { .. } => "editChangelist",
            ChangesDialog::Move { .. } => "moveToChangelist",
            ChangesDialog::Shelve { .. } => "shelve",
            ChangesDialog::Unshelve { .. } => "unshelve",
            ChangesDialog::RenameShelf { .. } => "renameShelf",
            ChangesDialog::DeleteShelf { .. } => "deleteShelf",
        }
    }
}

enum Outcome {
    Stay,
    Close,
    Run(&'static str, serde_json::Value),
}

impl IdeView {
    pub(in crate::ide) fn open_new_changelist(&mut self, paths: Vec<PathBuf>) {
        self.changes_dialog = Some(ChangesDialog::NewChangelist {
            name: String::new(),
            comment: String::new(),
            active: false,
            paths,
            focused: false,
            error: None,
        });
    }

    pub(in crate::ide) fn open_edit_changelist(&mut self, name: &str) {
        let Some(list) = self.commit_window.lists.iter().find(|list| list.name == name) else { return };
        self.changes_dialog = Some(ChangesDialog::EditChangelist {
            old: list.name.clone(),
            name: list.name.clone(),
            comment: list.comment.clone(),
            active: list.active,
            focused: false,
            error: None,
        });
    }

    pub(in crate::ide) fn open_move_to_changelist(&mut self, paths: Vec<PathBuf>) {
        let window = &self.commit_window;
        let from: BTreeSet<String> =
            paths.iter().filter_map(|path| window.list_of(path)).map(|list| list.name.clone()).collect();
        let lists: Vec<String> = window.list_names().into_iter().filter(|name| !from.contains(name)).collect();
        let target = lists.first().cloned();
        self.changes_dialog = Some(ChangesDialog::Move { paths, lists, target, new_name: String::new(), error: None });
    }

    pub(in crate::ide) fn open_shelve(&mut self, registry: &Registry, session: &mut Session, paths: Vec<PathBuf>) {
        self.refresh_changes(registry, session);
        let window = &self.commit_window;
        let files: Vec<DialogFile> = window.changes.iter().map(DialogFile::from).collect();
        let checked: BTreeSet<PathBuf> =
            if paths.is_empty() { window.checked_paths() } else { paths }.into_iter().collect();
        let name = checked
            .iter()
            .find_map(|path| window.list_of(path))
            .map_or_else(|| window.active_list(), |list| list.name.clone());
        self.changes_dialog =
            Some(ChangesDialog::Shelve { name, picker: FilePicker::new(files, checked), focused: false, error: None });
    }

    pub(in crate::ide) fn open_unshelve(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        id: u64,
        paths: Vec<PathBuf>,
    ) {
        self.refresh_shelf(registry, session, true);
        self.refresh_changes(registry, session);
        let Some(shelf) = self.commit_window.shelf.shelves.iter().find(|shelf| shelf.id == id).cloned() else { return };
        let files: Vec<DialogFile> = shelf
            .files
            .iter()
            .map(|file| DialogFile {
                path: file.path.clone(),
                relative: file.relative.clone(),
                status: file.status.clone(),
            })
            .collect();
        let checked: BTreeSet<PathBuf> = if paths.is_empty() {
            files.iter().map(|file| file.path.clone()).collect()
        } else {
            paths.into_iter().collect()
        };
        let lists = self.commit_window.list_names();
        let target =
            lists.iter().find(|list| **list == shelf.name).cloned().unwrap_or_else(|| self.commit_window.active_list());
        self.changes_dialog = Some(ChangesDialog::Unshelve {
            shelf,
            picker: FilePicker::new(files, checked),
            lists,
            target,
            remove: true,
            error: None,
        });
    }

    pub(in crate::ide) fn changes_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(mut dialog) = self.changes_dialog.take() else { return };
        let p = self.palette;
        let root = self.vcs.info.root.clone().unwrap_or_default();
        match &mut dialog {
            ChangesDialog::Shelve { picker, .. } if picker.stale => {
                picker.stale = false;
                picker.preview = picker.selected.as_ref().and_then(|path| {
                    let view = registry.dispatch(session, "vcs.diff.file", json!({ "path": path })).ok()?;
                    serde_json::from_value(view).ok()
                });
            }
            ChangesDialog::Unshelve { shelf, picker, .. } if picker.stale => {
                picker.stale = false;
                picker.preview = picker.selected.as_ref().and_then(|path| {
                    let params = json!({ "id": shelf.id, "path": path });
                    serde_json::from_value(registry.dispatch(session, "vcs.shelf.diff", params).ok()?).ok()
                });
            }
            _ => {}
        }
        let id = ui.scope_id().with(("vcs-changes-dialog", dialog.name()));
        let mut outcome = Outcome::Stay;
        let modal = egui::Modal::new(id).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            outcome = match &mut dialog {
                ChangesDialog::NewChangelist { name, comment, active, paths, focused, error } => {
                    let title = t!("vcs-changelist-new-title");
                    let fields = changelist_fields(ui, &p, &title, name, comment, active, focused, error.as_deref());
                    let params = json!({ "name": name.trim(), "comment": comment, "active": active, "paths": paths });
                    buttons(ui, &p, &t!("vcs-changelist-create"), fields).then_run("vcs.changelist.create", params)
                }
                ChangesDialog::EditChangelist { old, name, comment, active, focused, error } => {
                    let title = t!("vcs-changelist-edit-title");
                    let fields = changelist_fields(ui, &p, &title, name, comment, active, focused, error.as_deref());
                    let params = json!({ "name": old, "newName": name.trim(), "comment": comment, "active": active });
                    buttons(ui, &p, &t!("vcs-ok"), fields).then_run("vcs.changelist.edit", params)
                }
                ChangesDialog::Move { paths, lists, target, new_name, error } => {
                    move_body(ui, &p, paths.len(), lists, target, new_name, error.as_deref());
                    let params = match target {
                        Some(to) => ("vcs.changelist.move", json!({ "paths": paths, "to": to })),
                        None => ("vcs.changelist.create", json!({ "name": new_name.trim(), "paths": paths })),
                    };
                    buttons(ui, &p, &t!("vcs-changelist-move"), false).then_run(params.0, params.1)
                }
                ChangesDialog::Shelve { name, picker, focused, error } => {
                    ui.set_width(640.0);
                    title_label(ui, &p, &t!("vcs-shelve-title"));
                    file_picker(ui, &p, picker, &root);
                    ui.add_space(12.0);
                    preview(ui, &p, picker.preview.as_ref(), &[]);
                    ui.add_space(12.0);
                    label(ui, &p, &t!("vcs-shelve-name"));
                    let enter = name_field(ui, name, focused);
                    error_label(ui, &p, error.as_deref());
                    let params = json!({ "paths": picker.checked(), "name": name.trim() });
                    buttons(ui, &p, &t!("vcs-shelve-confirm"), enter).then_run("vcs.shelf.create", params)
                }
                ChangesDialog::Unshelve { shelf, picker, lists, target, remove, error } => {
                    ui.set_width(640.0);
                    title_label(ui, &p, &t!("vcs-unshelve-title", name = shelf.name.as_str()));
                    file_picker(ui, &p, picker, &root);
                    ui.add_space(12.0);
                    preview(ui, &p, picker.preview.as_ref(), &[]);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(t!("vcs-unshelve-changelist")).font(theme::regular(13.0)).color(p.text),
                        );
                        egui::ComboBox::from_id_salt("vcs-unshelve-target").selected_text(target.as_str()).show_ui(
                            ui,
                            |ui| {
                                for list in lists.iter() {
                                    ui.selectable_value(target, list.clone(), list);
                                }
                            },
                        );
                    });
                    ui.add_space(8.0);
                    ui.checkbox(remove, t!("vcs-unshelve-remove"));
                    error_label(ui, &p, error.as_deref());
                    let params = json!({
                        "id": shelf.id,
                        "paths": picker.checked(),
                        "changelist": target,
                        "remove": remove,
                    });
                    buttons(ui, &p, &t!("vcs-unshelve-confirm"), false).then_run("vcs.shelf.unshelve", params)
                }
                ChangesDialog::RenameShelf { id, name, focused, error } => {
                    ui.set_width(420.0);
                    title_label(ui, &p, &t!("vcs-shelf-rename-title"));
                    label(ui, &p, &t!("vcs-shelve-name"));
                    let enter = name_field(ui, name, focused);
                    error_label(ui, &p, error.as_deref());
                    let params = json!({ "id": id, "name": name.trim() });
                    buttons(ui, &p, &t!("vcs-ok"), enter).then_run("vcs.shelf.rename", params)
                }
                ChangesDialog::DeleteShelf { id, name, error } => {
                    ui.set_width(420.0);
                    title_label(ui, &p, &t!("vcs-shelf-delete-title"));
                    ui.label(
                        egui::RichText::new(t!("vcs-shelf-delete-text", name = name.as_str()))
                            .font(theme::regular(13.0))
                            .color(p.text),
                    );
                    error_label(ui, &p, error.as_deref());
                    let enter = ui.input(|input| input.key_pressed(Key::Enter));
                    buttons(ui, &p, &t!("vcs-shelf-delete"), enter).then_run("vcs.shelf.delete", json!({ "id": id }))
                }
            };
        });
        match outcome {
            Outcome::Close => return,
            Outcome::Stay if modal.should_close() => return,
            Outcome::Stay => {}
            Outcome::Run(command, params) => match registry.dispatch(session, command, params) {
                Ok(result) => {
                    self.after_changes_dialog(ui, &dialog, result);
                    return;
                }
                Err(zen_core::DispatchError::Failed { error, .. })
                    if error.code.as_deref() == Some(zen_workbench::vcs::CONFLICTS) =>
                {
                    self.commit_window.shelf.stale = true;
                    return;
                }
                Err(zen_core::DispatchError::Failed { error, .. }) => set_error(&mut dialog, error.message),
                Err(other) => set_error(&mut dialog, other.to_string()),
            },
        }
        self.changes_dialog = Some(dialog);
    }

    fn after_changes_dialog(&mut self, ui: &Ui, dialog: &ChangesDialog, result: serde_json::Value) {
        match dialog {
            ChangesDialog::NewChangelist { .. } | ChangesDialog::EditChangelist { .. } | ChangesDialog::Move { .. } => {
                self.changes_from(result);
            }
            ChangesDialog::Shelve { .. } => {
                self.commit_window.revision = None;
                self.commit_window.shelf.stale = true;
                let count = result["files"].as_array().map_or(0, Vec::len);
                let name = result["name"].as_str().unwrap_or_default();
                self.post(ui, t!("vcs-shelve-done", count = count, name = name));
            }
            ChangesDialog::Unshelve { .. } => {
                self.commit_window.revision = None;
                self.commit_window.shelf.stale = true;
                self.commit_window.tab = CommitTab::Changes;
            }
            ChangesDialog::RenameShelf { .. } | ChangesDialog::DeleteShelf { .. } => {
                self.commit_window.shelf.stale = true;
            }
        }
    }
}

fn set_error(dialog: &mut ChangesDialog, message: String) {
    match dialog {
        ChangesDialog::NewChangelist { error, .. }
        | ChangesDialog::EditChangelist { error, .. }
        | ChangesDialog::Move { error, .. }
        | ChangesDialog::Shelve { error, .. }
        | ChangesDialog::Unshelve { error, .. }
        | ChangesDialog::RenameShelf { error, .. }
        | ChangesDialog::DeleteShelf { error, .. } => *error = Some(message),
    }
}

pub(super) enum Pressed {
    Confirm,
    Cancel,
    None,
}

impl Pressed {
    fn then_run(self, command: &'static str, params: serde_json::Value) -> Outcome {
        match self {
            Pressed::Confirm => Outcome::Run(command, params),
            Pressed::Cancel => Outcome::Close,
            Pressed::None => Outcome::Stay,
        }
    }
}

pub(super) fn buttons(ui: &mut Ui, p: &IdePalette, confirm: &str, enter: bool) -> Pressed {
    ui.add_space(20.0);
    let mut pressed = if enter { Pressed::Confirm } else { Pressed::None };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if primary_button(ui, p, confirm).clicked() {
            pressed = Pressed::Confirm;
        }
        if secondary_button(ui, p, &t!("settings-cancel")).clicked() {
            pressed = Pressed::Cancel;
        }
    });
    pressed
}

pub(super) fn title_label(ui: &mut Ui, p: &IdePalette, title: &str) {
    ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
    ui.add_space(14.0);
}

fn label(ui: &mut Ui, p: &IdePalette, text: &str) {
    ui.label(egui::RichText::new(text).font(theme::regular(13.0)).color(p.text));
    ui.add_space(6.0);
}

pub(super) fn error_label(ui: &mut Ui, p: &IdePalette, error: Option<&str>) {
    if let Some(error) = error {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(error).font(theme::regular(12.0)).color(p.vcs_conflict));
    }
}

fn name_field(ui: &mut Ui, name: &mut String, focused: &mut bool) -> bool {
    let field =
        ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
    if !std::mem::replace(focused, true) {
        field.request_focus();
    }
    field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))
}

#[allow(clippy::too_many_arguments)]
fn changelist_fields(
    ui: &mut Ui,
    p: &IdePalette,
    title: &str,
    name: &mut String,
    comment: &mut String,
    active: &mut bool,
    focused: &mut bool,
    error: Option<&str>,
) -> bool {
    ui.set_width(440.0);
    title_label(ui, p, title);
    label(ui, p, &t!("vcs-changelist-name"));
    let enter = name_field(ui, name, focused);
    ui.add_space(12.0);
    label(ui, p, &t!("vcs-changelist-comment"));
    ui.add(
        egui::TextEdit::multiline(comment)
            .desired_width(f32::INFINITY)
            .desired_rows(3)
            .margin(egui::Margin::symmetric(8, 6)),
    );
    ui.add_space(10.0);
    ui.checkbox(active, t!("vcs-changelist-make-active"));
    error_label(ui, p, error);
    enter
}

fn move_body(
    ui: &mut Ui,
    p: &IdePalette,
    count: usize,
    lists: &[String],
    target: &mut Option<String>,
    new_name: &mut String,
    error: Option<&str>,
) {
    ui.set_width(420.0);
    title_label(ui, p, &t!("vcs-changelist-move-title", count = count));
    for list in lists {
        ui.radio_value(target, Some(list.clone()), list.as_str());
        ui.add_space(6.0);
    }
    ui.horizontal(|ui| {
        ui.radio_value(target, None, t!("vcs-changelist-move-new"));
        let field = ui.add(
            egui::TextEdit::singleline(new_name).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 4)),
        );
        if field.gained_focus() || field.changed() {
            *target = None;
        }
    });
    error_label(ui, p, error);
}

pub(super) fn file_picker(ui: &mut Ui, p: &IdePalette, picker: &mut FilePicker, root: &std::path::Path) {
    egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(6).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt("vcs-dialog-files").max_height(170.0).show(ui, |ui| {
            for file in &picker.files {
                let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                let on = picker.checked.contains(&file.path);
                let selected = picker.selected.as_ref() == Some(&file.path);
                if selected {
                    ui.painter().rect_filled(row, 4, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(row, 4, p.hover);
                }
                let check = Rect::from_center_size(pos2(row.left() + 12.0, row.center().y), Vec2::splat(14.0));
                let check_response =
                    ui.interact(check.expand(3.0), ui.scope_id().with(("check", &file.path)), Sense::click());
                checkbox(ui.painter(), check, Some(on), p);
                check_response
                    .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &file.relative));
                crate::accessibility::item(&response, egui::Role::ListItem, selected, &file.relative);
                let name = file.relative.rsplit('/').next().unwrap_or_default();
                crate::file_icons::file_icon_at(
                    ui.painter(),
                    Rect::from_center_size(pos2(row.left() + 34.0, row.center().y), Vec2::splat(15.0)),
                    &file.path,
                );
                let painter = ui.painter().with_clip_rect(row);
                let label = painter.text(
                    pos2(row.left() + 48.0, row.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    theme::regular(13.0),
                    status_color(p, status_named(&file.status)),
                );
                let folder = file.path.parent().and_then(|parent| parent.strip_prefix(root).ok());
                if let Some(folder) = folder.filter(|folder| !folder.as_os_str().is_empty()) {
                    painter.text(
                        pos2(label.right() + 7.0, row.center().y),
                        Align2::LEFT_CENTER,
                        folder.display().to_string(),
                        theme::regular(12.5),
                        p.text_secondary,
                    );
                }
                if check_response.clicked() {
                    if on {
                        picker.checked.remove(&file.path);
                    } else {
                        picker.checked.insert(file.path.clone());
                    }
                } else if response.clicked() && !selected {
                    picker.selected = Some(file.path.clone());
                    picker.stale = true;
                }
            }
        });
    });
}

const CONTEXT: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreviewLine {
    pub(super) kind: char,
    pub(super) number: usize,
    pub(super) text: String,
    pub(super) modified: bool,
}

pub(super) fn preview_lines(view: &DiffView) -> Vec<PreviewLine> {
    let before = zen_vcs::diff::lines(&view.before);
    let after = zen_vcs::diff::lines(&view.after);
    let side = |lines: &[&str], from: usize, to: usize, kind: char, modified: bool| -> Vec<PreviewLine> {
        let to = to.min(lines.len());
        let from = from.min(to);
        lines[from..to]
            .iter()
            .enumerate()
            .map(|(offset, line)| PreviewLine {
                kind,
                number: from + offset + 1,
                text: line.trim_end_matches(['\n', '\r']).to_owned(),
                modified,
            })
            .collect()
    };
    let mut lines = Vec::new();
    let mut shown = 0usize;
    for (index, hunk) in view.hunks.iter().enumerate() {
        let start = hunk.after_start.saturating_sub(CONTEXT).max(shown);
        if index > 0 && start > shown {
            lines.push(PreviewLine { kind: '…', number: 0, text: String::new(), modified: false });
        }
        lines.extend(side(&after, start, hunk.after_start, ' ', false));
        let modified = hunk.kind == "modified";
        lines.extend(side(&before, hunk.before_start, hunk.before_end, '-', modified));
        lines.extend(side(&after, hunk.after_start, hunk.after_end, '+', modified));
        let next = view.hunks.get(index + 1).map_or(usize::MAX, |next| next.after_start.saturating_sub(CONTEXT));
        let end = (hunk.after_end + CONTEXT).min(after.len()).min(next.max(hunk.after_end));
        lines.extend(side(&after, hunk.after_end, end, ' ', false));
        shown = end.max(hunk.after_end);
    }
    lines
}

pub(super) fn preview(ui: &mut Ui, p: &IdePalette, view: Option<&DiffView>, conflicts: &[(usize, usize)]) {
    let height = 200.0;
    egui::Frame::new().fill(p.editor_background).stroke(egui::Stroke::new(1.0, p.tab_border)).corner_radius(6).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(height);
            let message = match view {
                None => Some(t!("vcs-shelve-no-preview")),
                Some(view) if view.binary => Some(t!("vcs-diff-binary")),
                Some(view) if view.hunks.is_empty() => Some(t!("vcs-diff-count", count = 0)),
                Some(_) => None,
            };
            if let Some(message) = message {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(message).color(p.text_secondary));
                });
                return;
            }
            let Some(view) = view else { return };
            let font = egui::FontId::monospace(12.0);
            let row = 18.0;
            egui::ScrollArea::both().id_salt("vcs-dialog-preview").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let lines = preview_lines(view);
                let width = ui
                    .available_width()
                    .max(lines.iter().map(|line| line.text.chars().count()).max().unwrap_or(0) as f32 * 7.4 + 70.0);
                for line in lines {
                    let (rect, _) = ui.allocate_exact_size(vec2(width, row), Sense::hover());
                    let conflicted = line.kind != '-'
                        && line.kind != '…'
                        && conflicts
                            .iter()
                            .any(|(start, end)| (*start..*end).contains(&(line.number.saturating_sub(1))));
                    let fill = match (line.kind, line.modified) {
                        _ if conflicted => Some(p.vcs_conflict.gamma_multiply(0.3)),
                        ('-' | '+', true) => Some(p.diff_modified),
                        ('-', false) => Some(p.diff_deleted),
                        ('+', false) => Some(p.diff_inserted),
                        _ => None,
                    };
                    if let Some(fill) = fill {
                        ui.painter().rect_filled(rect, 0, fill);
                    }
                    let y = rect.center().y;
                    if line.kind == '…' {
                        ui.painter().text(
                            pos2(rect.left() + 8.0, y),
                            Align2::LEFT_CENTER,
                            "⋯",
                            font.clone(),
                            p.text_secondary,
                        );
                        continue;
                    }
                    ui.painter().text(
                        pos2(rect.left() + 36.0, y),
                        Align2::RIGHT_CENTER,
                        line.number.to_string(),
                        font.clone(),
                        p.text_secondary,
                    );
                    ui.painter().text(
                        pos2(rect.left() + 44.0, y),
                        Align2::LEFT_CENTER,
                        line.kind.to_string(),
                        font.clone(),
                        p.text_secondary,
                    );
                    ui.painter().text(
                        pos2(rect.left() + 58.0, y),
                        Align2::LEFT_CENTER,
                        &line.text,
                        font.clone(),
                        p.text,
                    );
                }
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use zen_workbench::vcs::commit::DiffHunk;

    use super::*;

    fn hunk(kind: &str, before: (usize, usize), after: (usize, usize)) -> DiffHunk {
        DiffHunk {
            kind: kind.to_owned(),
            before_start: before.0,
            before_end: before.1,
            after_start: after.0,
            after_end: after.1,
            ..DiffHunk::default()
        }
    }

    #[test]
    fn the_preview_shows_each_change_with_two_lines_around_it() {
        let view = DiffView {
            before: "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n".to_owned(),
            after: "1\nTWO\n3\n4\n5\n6\n7\n8\n9\n10\n11\n".to_owned(),
            hunks: vec![hunk("modified", (1, 2), (1, 2)), hunk("added", (10, 10), (10, 11))],
            ..DiffView::default()
        };
        let lines: Vec<String> =
            preview_lines(&view).iter().map(|line| format!("{}{} {}", line.kind, line.number, line.text)).collect();
        assert_eq!(lines, [" 1 1", "-2 2", "+2 TWO", " 3 3", " 4 4", "…0 ", " 9 9", " 10 10", "+11 11"]);
        assert!(preview_lines(&view)[1].modified);
        assert!(!preview_lines(&view)[8].modified);
    }

    #[test]
    fn close_changes_share_their_context() {
        let view = DiffView {
            before: "a\nb\nc\nd\n".to_owned(),
            after: "A\nb\nc\nD\n".to_owned(),
            hunks: vec![hunk("modified", (0, 1), (0, 1)), hunk("modified", (3, 4), (3, 4))],
            ..DiffView::default()
        };
        let kinds: String = preview_lines(&view).iter().map(|line| line.kind).collect();
        assert_eq!(kinds, "-+  -+");
    }
}
