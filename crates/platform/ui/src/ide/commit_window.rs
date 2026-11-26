mod dialogs;
mod patch_dialogs;
mod shelf_tab;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Align2, Color32, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_vcs::FileStatus;
use zen_workbench::todo::TodoTab;
use zen_workbench::vcs::ChangeInfo;
use zen_workbench::vcs::changelists::{ChangelistInfo, ChangelistReport};

pub(super) use dialogs::ChangesDialog;
pub(super) use patch_dialogs::{PatchDialog, is_patch_file};

use super::glyphs::{Glyph, glyph_minus, glyph_stash};
use super::vcs::{primary_button, secondary_button, status_color};
use super::{IdePalette, IdeView, Side, customization, dialog_frame, hint, hover, metrics};
use crate::theme;

const TODO_LISTED: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CommitTab {
    #[default]
    Changes,
    Shelf,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct CommitTodo {
    path: PathBuf,
    line: usize,
    text: String,
}

#[derive(Clone, Copy)]
enum TodoChoice {
    Commit,
    Review,
    Cancel,
}

#[derive(Default)]
pub(super) struct CommitWindow {
    pub(super) committing: bool,
    todo_question: Option<(bool, Vec<CommitTodo>)>,
    changes: Vec<ChangeInfo>,
    lists: Vec<ChangelistInfo>,
    unversioned: Vec<ChangeInfo>,
    revision: Option<u64>,
    unsaved: u64,
    excluded: BTreeSet<PathBuf>,
    included_new: BTreeSet<PathBuf>,
    selected: Option<PathBuf>,
    collapsed: BTreeSet<String>,
    unversioned_collapsed: bool,
    conflicts_collapsed: bool,
    pub(super) message: String,
    pub(super) author: Option<String>,
    draft_generation: u64,
    amend: bool,
    pub(super) focus_message: bool,
    pub(super) tab: CommitTab,
    shelf: shelf_tab::ShelfView,
    tree: Option<egui::Id>,
    hovered: bool,
    pub(super) focus_tree: bool,
    pub(super) message_height: Option<f32>,
    form_rest: Option<f32>,
}

const MESSAGE_HEIGHT: f32 = 120.0;

const LEAST_MESSAGE: f32 = 40.0;

const LEAST_LIST: f32 = 40.0;

const SPLITTER: f32 = 7.0;

const FORM_REST: f32 = 150.0;

pub(super) fn commit_heights(total: f32, rest: f32, wanted: f32) -> (f32, f32) {
    let room = (total - rest - SPLITTER).max(0.0);
    let list_least = LEAST_LIST.min(room / 2.0);
    let message = wanted.max(LEAST_MESSAGE).min(room - list_least).max(0.0);
    (room - message, message)
}

pub(super) struct RollbackDialog {
    changes: Vec<ChangeInfo>,
    checked: BTreeSet<PathBuf>,
    delete_added: bool,
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().is_some_and(|a| b.canonicalize().ok().is_some_and(|b| a == b))
}

fn status_of(change: &ChangeInfo) -> FileStatus {
    status_named(&change.status)
}

pub(super) fn status_named(status: &str) -> FileStatus {
    match status {
        "added" => FileStatus::Added,
        "deleted" => FileStatus::Deleted,
        "renamed" => FileStatus::Renamed,
        "conflicted" => FileStatus::Conflicted,
        "unversioned" => FileStatus::Unversioned,
        _ => FileStatus::Modified,
    }
}

impl CommitWindow {
    pub(super) fn committed(&mut self) {
        self.message.clear();
        self.author = None;
        self.amend = false;
        self.revision = None;
    }

    fn update(&mut self, report: ChangelistReport) {
        let changes: Vec<ChangeInfo> =
            report.lists.iter().flat_map(|list| list.files.iter().cloned()).chain(report.unversioned.clone()).collect();
        let paths: BTreeSet<PathBuf> = changes.iter().map(|change| change.path.clone()).collect();
        self.excluded.retain(|path| paths.contains(path));
        self.included_new.retain(|path| paths.contains(path));
        if self.selected.as_ref().is_some_and(|selected| !paths.contains(selected)) {
            self.selected = None;
        }
        self.changes = changes;
        self.lists = report.lists;
        self.unversioned = report.unversioned;
        self.revision = Some(report.revision);
    }

    fn is_checked(&self, change: &ChangeInfo) -> bool {
        if change.status == "unversioned" {
            self.included_new.contains(&change.path)
        } else {
            !self.excluded.contains(&change.path)
        }
    }

    fn set_checked(&mut self, change: &ChangeInfo, checked: bool) {
        let (set, add) = if change.status == "unversioned" {
            (&mut self.included_new, checked)
        } else {
            (&mut self.excluded, !checked)
        };
        if add {
            set.insert(change.path.clone());
        } else {
            set.remove(&change.path);
        }
    }

    pub(super) fn check_only(&mut self, paths: &[PathBuf]) {
        let wanted = |path: &PathBuf| paths.iter().any(|given| path.starts_with(given));
        let changes = self.changes.clone();
        for change in &changes {
            self.set_checked(change, wanted(&change.path));
        }
        if let Some(first) = changes.iter().find(|change| wanted(&change.path)) {
            self.selected = Some(first.path.clone());
        }
    }

    pub(super) fn select(&mut self, path: &Path) -> bool {
        let Some(change) = self.changes.iter().find(|change| same_file(&change.path, path)).cloned() else {
            return false;
        };
        if change.status == "unversioned" {
            self.unversioned_collapsed = false;
        } else if let Some(name) = self.list_of(&change.path).map(|list| list.name.clone()) {
            self.collapsed.remove(&name);
        }
        self.tab = CommitTab::Changes;
        self.selected = Some(change.path);
        true
    }

    pub(super) fn is_changed(&self, path: &Path) -> bool {
        self.changes.iter().any(|change| same_file(&change.path, path))
    }

    pub(super) fn changes(&self) -> &[ChangeInfo] {
        &self.changes
    }

    pub(super) fn selected(&self) -> Option<PathBuf> {
        self.selected.clone()
    }

    pub(super) fn list_has_keyboard(&self, ctx: &egui::Context) -> bool {
        let focused = ctx.memory(egui::Memory::focused);
        match (self.tree, focused) {
            (Some(tree), Some(focused)) => tree == focused,
            (Some(_), None) => self.hovered,
            (None, _) => false,
        }
    }

    pub(super) fn jump_targets(&self) -> Vec<PathBuf> {
        self.selected
            .iter()
            .filter(|path| {
                self.changes.iter().any(|change| same_file(&change.path, path) && change.status != "deleted")
            })
            .cloned()
            .collect()
    }

    pub(super) fn checked_paths(&self) -> Vec<PathBuf> {
        self.changes.iter().filter(|change| self.is_checked(change)).map(|change| change.path.clone()).collect()
    }

    pub(super) fn list_names(&self) -> Vec<String> {
        self.lists.iter().map(|list| list.name.clone()).collect()
    }

    fn list_of(&self, path: &Path) -> Option<&ChangelistInfo> {
        self.lists.iter().find(|list| list.files.iter().any(|file| file.path == path))
    }

    pub(super) fn active_list(&self) -> String {
        self.lists.iter().find(|list| list.active).map(|list| list.name.clone()).unwrap_or_default()
    }

    pub(super) fn tracked_context(&self) -> Vec<PathBuf> {
        let tracked = |path: &PathBuf| self.list_of(path).is_some();
        match &self.selected {
            Some(selected) if tracked(selected) => vec![selected.clone()],
            _ => self
                .changes
                .iter()
                .filter(|change| change.status != "unversioned" && self.is_checked(change))
                .map(|change| change.path.clone())
                .collect(),
        }
    }
}

enum CommitAction {
    Hide,
    Refresh,
    Diff(PathBuf),
    Open(PathBuf),
    Rollback(Vec<PathBuf>),
    Commit { push: bool },
    CheckTodo(bool),
    CheckTodoFilter(Option<String>),
    Amend(bool),
    Run(&'static str, serde_json::Value),
    Shelve(Vec<PathBuf>),
    CreatePatch(Vec<PathBuf>),
    CopyPatch(Vec<PathBuf>),
    ClearAuthor,
    NewChangelist(Vec<PathBuf>),
    EditChangelist(String),
    Tab(CommitTab),
    Conflict(&'static str, serde_json::Value),
}

impl IdeView {
    pub(super) fn refresh_changes(&mut self, registry: &Registry, session: &mut Session) {
        let report: ChangelistReport = registry
            .dispatch(session, "vcs.changelist.list", json!({ "wait": false }))
            .ok()
            .and_then(|report| serde_json::from_value(report).ok())
            .unwrap_or_default();
        self.commit_window.update(report);
    }

    fn changes_from(&mut self, report: serde_json::Value) {
        if let Ok(report) = serde_json::from_value::<ChangelistReport>(report) {
            self.commit_window.update(report);
        }
    }

    pub(super) fn given_paths(&self, params: &serde_json::Value) -> Vec<PathBuf> {
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let paths: Vec<PathBuf> =
            params.get("paths").and_then(|paths| serde_json::from_value(paths.clone()).ok()).unwrap_or_default();
        paths
            .into_iter()
            .map(|path| {
                let path = if path.is_absolute() { path } else { root.join(path) };
                path.canonicalize().unwrap_or(path)
            })
            .collect()
    }

    pub(super) fn show_commit_tab(&mut self, tab: CommitTab) {
        self.commit_open = true;
        self.project_open = false;
        self.structure_open = false;
        self.commit_window.tab = tab;
        if tab == CommitTab::Shelf {
            self.commit_window.shelf.stale = true;
        }
        self.show_tool("commit");
    }

    pub(super) fn commit_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if self.commit_window.revision != Some(self.vcs.info.revision) || self.commit_window.unsaved != self.vcs.unsaved
        {
            self.commit_window.unsaved = self.vcs.unsaved;
            self.refresh_changes(registry, session);
        }
        self.refresh_shelf(registry, session, false);
        let p = self.palette;
        let mut actions = Vec::new();
        self.commit_header(ui, &p, &mut actions);
        if self.vcs.info.root.is_none() {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                if self.safe_mode {
                    ui.label(egui::RichText::new(t!("ide-vcs-safe-mode")).color(p.text_secondary));
                    if ui.link(t!("ide-safe-mode-trust")).clicked() {
                        self.ask_trust(session);
                    }
                } else {
                    ui.label(egui::RichText::new(t!("vcs-commit-no-repository")).color(p.text_secondary));
                }
            });
        } else if self.commit_window.tab == CommitTab::Shelf {
            self.shelf_tab(ui, registry, session, &p);
        } else {
            let total = ui.available_height();
            let rest = self.commit_window.form_rest.unwrap_or(FORM_REST);
            let wanted = self.commit_window.message_height.unwrap_or(MESSAGE_HEIGHT);
            let (list_height, message_height) = commit_heights(total, rest, wanted);
            let top = ui.cursor().top();
            egui::ScrollArea::vertical()
                .id_salt("commit-changes")
                .max_height(list_height)
                .min_scrolled_height(list_height)
                .auto_shrink([false, false])
                .show(ui, |ui| self.commit_tree(ui, &p, &mut actions));
            self.commit_splitter(ui, &p, message_height, total - rest);
            self.commit_form(ui, &p, &mut actions, message_height);
            let measured = ui.cursor().top() - top - list_height - message_height - SPLITTER;
            if self.commit_window.form_rest.is_none_or(|known| (known - measured).abs() > 0.5) {
                self.commit_window.form_rest = Some(measured);
                ui.ctx().request_repaint();
            }
        }
        self.todo_question(ui, registry, session);
        for action in actions {
            match action {
                CommitAction::Hide => self.commit_open = false,
                CommitAction::Refresh => {
                    let _ = registry.dispatch(session, "vcs.status.refresh", json!({}));
                }
                CommitAction::Diff(path) => self.open_diff(ui.ctx(), registry, session, &path),
                CommitAction::Open(path) => {
                    self.dispatch(ui, registry, session, "file.document.open", json!({ "path": path }));
                    self.editor_view.focus();
                }
                CommitAction::Rollback(paths) => self.open_rollback(paths),
                CommitAction::Amend(on) => {
                    self.commit_window.amend = on;
                    if on && self.commit_window.message.trim().is_empty() {
                        let last = registry.dispatch(session, "vcs.commit.lastMessage", json!({})).ok();
                        if let Some(message) = last.as_ref().and_then(|last| last["message"].as_str()) {
                            self.commit_window.message = message.to_owned();
                        }
                    }
                }
                CommitAction::Commit { push } => self.commit_checked(ui, registry, session, push),
                CommitAction::CheckTodo(on) => self.set_check_todo(|todo| todo.commit_check = on),
                CommitAction::CheckTodoFilter(filter) => self.set_check_todo(|todo| todo.commit_filter = filter),
                CommitAction::Run(command, params) => match registry.dispatch(session, command, params) {
                    Ok(report) => self.changes_from(report),
                    Err(error) => self.report(ui, error),
                },
                CommitAction::Shelve(paths) => self.open_shelve(registry, session, paths),
                CommitAction::CreatePatch(paths) => self.open_create_patch(registry, session, paths),
                CommitAction::CopyPatch(paths) => {
                    if let Err(error) = self.copy_patch(ui.ctx(), registry, session, paths, None) {
                        self.report(ui, error);
                    }
                }
                CommitAction::ClearAuthor => self.commit_window.author = None,
                CommitAction::NewChangelist(paths) => self.open_new_changelist(paths),
                CommitAction::EditChangelist(name) => self.open_edit_changelist(&name),
                CommitAction::Tab(tab) => self.show_commit_tab(tab),
                CommitAction::Conflict(command, params) => {
                    self.dispatch(ui, registry, session, command, params);
                    let _ = registry.dispatch(session, "vcs.status.refresh", json!({}));
                    self.git_workspace.conflicts_changed();
                }
            }
        }
    }

    fn commit_header(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<CommitAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let y = header.center().y;
        let title = ui.painter().text(
            pos2(header.left() + 12.0, y),
            Align2::LEFT_CENTER,
            t!("ide-tool-commit"),
            theme::semibold(13.0),
            p.text,
        );
        let window = &mut self.commit_window;
        if !window.shelf.shelves.is_empty() || window.tab == CommitTab::Shelf {
            let mut x = title.right() + 14.0;
            for (tab, name) in
                [(CommitTab::Changes, t!("vcs-commit-tab-changes")), (CommitTab::Shelf, t!("vcs-shelf-tab"))]
            {
                let galley = ui.painter().layout_no_wrap(name.clone(), theme::regular(13.0), p.text);
                let rect = Rect::from_min_size(pos2(x, y - 13.0), vec2(galley.size().x + 20.0, 26.0));
                let response = ui.interact(rect, ui.scope_id().with(("commit-tab", tab as u8)), Sense::click());
                crate::accessibility::item(&response, egui::Role::Tab, window.tab == tab, &name);
                if window.tab == tab {
                    ui.painter().rect_filled(rect, 6, p.selection);
                } else {
                    hover(ui, &response, p);
                }
                ui.painter().galley(pos2(rect.left() + 10.0, y - galley.size().y / 2.0), galley, p.text);
                if response.clicked() && window.tab != tab {
                    actions.push(CommitAction::Tab(tab));
                }
                x = rect.right() + 4.0;
            }
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("commit-hide"), Sense::click());
        hover(ui, &response, p);
        hint(ui, &response, &t!("ide-project-hide"), None, Side::Below);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        if response.clicked() {
            actions.push(CommitAction::Hide);
        }
        if window.tab == CommitTab::Shelf {
            return;
        }
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let mut x = bar.left() + 10.0;
        let selected = window.selected.clone();
        let any = !window.changes.is_empty();
        let buttons: [(Glyph, String, Option<&str>, bool); 6] = [
            (glyph_refresh, t!("vcs-commit-refresh"), None, true),
            (glyph_rollback, t!("keymap-view-vcs-rollback"), Some("⌥⌘Z"), true),
            (glyph_diff, t!("keymap-view-vcs-showDiff"), Some("⌘D"), selected.is_some()),
            (glyph_stash, t!("keymap-view-vcs-shelve"), None, any),
            (glyph_expand_all, t!("ide-project-expand-all"), Some("⌘="), true),
            (glyph_collapse_all, t!("ide-project-collapse-all"), Some("⌘−"), true),
        ];
        for (index, (glyph, name, shortcut, enabled)) in buttons.into_iter().enumerate() {
            let button = Rect::from_min_size(pos2(x, bar.center().y - 12.0), Vec2::splat(24.0));
            let response = ui.interact(
                button,
                ui.scope_id().with(("commit-tool", index)),
                if enabled { Sense::click() } else { Sense::hover() },
            );
            if enabled {
                hover(ui, &response, p);
            }
            hint(ui, &response, &name, shortcut, Side::Below);
            glyph(
                ui.painter(),
                button.shrink(6.0),
                if enabled { p.text_secondary } else { p.text_secondary.gamma_multiply(0.4) },
            );
            if response.clicked() {
                match index {
                    0 => actions.push(CommitAction::Refresh),
                    1 => {
                        let paths = match &selected {
                            Some(path) => vec![path.clone()],
                            None => window.checked_paths(),
                        };
                        actions.push(CommitAction::Rollback(paths));
                    }
                    2 => actions.extend(selected.clone().map(CommitAction::Diff)),
                    3 => actions.push(CommitAction::Shelve(Vec::new())),
                    4 => {
                        window.collapsed.clear();
                        window.unversioned_collapsed = false;
                    }
                    _ => {
                        window.collapsed = window.lists.iter().map(|list| list.name.clone()).collect();
                        window.unversioned_collapsed = true;
                    }
                }
            }
            x = button.right() + 4.0;
            if index == 3 {
                ui.painter().vline(x + 2.0, bar.y_range().shrink(7.0), Stroke::new(1.0, p.tab_border));
                x += 8.0;
            }
        }
    }

    fn commit_tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<CommitAction>) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let window = &mut self.commit_window;
        let tree = ui.make_persistent_id("commit-changes-tree");
        let area = ui.interact(ui.clip_rect(), tree, Sense::click() | Sense::focusable_noninteractive());
        window.tree = Some(tree);
        window.hovered = ui.ui_contains_pointer();
        if area.clicked()
            || std::mem::take(&mut window.focus_tree)
            || (window.hovered && ui.input(|input| input.pointer.any_pressed()))
        {
            ui.memory_mut(|memory| memory.request_focus(tree));
        }
        let conflicts: Vec<ChangeInfo> =
            window.changes.iter().filter(|change| change.status == "conflicted").cloned().collect();
        if !conflicts.is_empty() {
            conflicts_group(ui, p, window, &conflicts, actions);
        }
        let several = window.lists.len() > 1;
        let unconflicted = |files: &[ChangeInfo]| -> Vec<ChangeInfo> {
            files.iter().filter(|change| change.status != "conflicted").cloned().collect()
        };
        let groups: Vec<(Option<ChangelistInfo>, Vec<ChangeInfo>)> = window
            .lists
            .iter()
            .map(|list| (list, unconflicted(&list.files)))
            .filter(|(_, files)| several || !files.is_empty())
            .map(|(list, files)| (Some(list.clone()), files))
            .chain((!window.unversioned.is_empty()).then(|| (None, window.unversioned.clone())))
            .collect();
        if groups.is_empty() && !conflicts.is_empty() {
            return;
        }
        if groups.is_empty() {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(egui::RichText::new(t!("vcs-commit-no-changes")).color(p.text_secondary));
            });
            return;
        }
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let names = window.list_names();
        for (list, files) in groups {
            let key = list.as_ref().map(|list| list.name.clone());
            let collapsed = match &key {
                Some(name) => window.collapsed.contains(name),
                None => window.unversioned_collapsed,
            };
            let checked = files.iter().filter(|change| window.is_checked(change)).count();
            let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.hover);
            }
            let chevron = Rect::from_center_size(pos2(row.left() + 20.0, row.center().y), Vec2::splat(11.0));
            if collapsed {
                crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
            } else {
                crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
            }
            let state = if checked == 0 {
                Some(false)
            } else if checked == files.len() {
                Some(true)
            } else {
                None
            };
            let check = Rect::from_center_size(pos2(row.left() + 38.0, row.center().y), Vec2::splat(14.0));
            let check_response =
                ui.interact(check.expand(3.0), ui.scope_id().with(("commit-group-check", &key)), Sense::click());
            if !files.is_empty() {
                checkbox(ui.painter(), check, state, p);
            }
            let title = match &list {
                Some(list) => list.name.clone(),
                None => t!("vcs-commit-unversioned"),
            };
            let active = list.as_ref().is_some_and(|list| list.active);
            crate::accessibility::tree_item(&response, 1, Some(!collapsed), false, &title);
            check_response
                .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, state == Some(true), &title));
            let font = if active && several { theme::semibold(13.0) } else { theme::regular(13.0) };
            let label =
                ui.painter().text(pos2(row.left() + 52.0, row.center().y), Align2::LEFT_CENTER, &title, font, p.text);
            ui.painter().text(
                pos2(label.right() + 8.0, row.center().y),
                Align2::LEFT_CENTER,
                t!("vcs-commit-files", count = files.len()),
                theme::regular(12.5),
                p.text_secondary,
            );
            if check_response.clicked() && !files.is_empty() {
                let on = state != Some(true);
                for change in &files {
                    window.set_checked(change, on);
                }
            } else if response.clicked() || response.double_clicked() {
                match &key {
                    Some(name) => {
                        if !window.collapsed.remove(name) {
                            window.collapsed.insert(name.clone());
                        }
                    }
                    None => window.unversioned_collapsed = !window.unversioned_collapsed,
                }
            }
            if let Some(list) = &list {
                response.context_menu(|ui| list_menu(ui, list, &files, actions));
            }
            if collapsed {
                continue;
            }
            for change in &files {
                let (row, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let selected = window.selected.as_ref() == Some(&change.path);
                if selected {
                    ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.hover);
                }
                let check = Rect::from_center_size(pos2(row.left() + 58.0, row.center().y), Vec2::splat(14.0));
                let check_response =
                    ui.interact(check.expand(3.0), ui.scope_id().with(("commit-check", &change.path)), Sense::click());
                checkbox(ui.painter(), check, Some(window.is_checked(change)), p);
                crate::accessibility::tree_item(&response, 2, None, selected, &change.relative);
                check_response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::CheckBox, true, window.is_checked(change), &change.relative)
                });
                file_label(ui, row, 80.0, change, &root, p);
                if check_response.clicked() {
                    let on = !window.is_checked(change);
                    window.set_checked(change, on);
                } else if response.double_clicked() {
                    if change.status == "deleted" || change.status == "unversioned" {
                        actions.push(CommitAction::Open(change.path.clone()));
                    } else {
                        actions.push(CommitAction::Diff(change.path.clone()));
                    }
                } else if response.clicked() || response.secondary_clicked() {
                    window.selected = Some(change.path.clone());
                }
                response.context_menu(|ui| file_menu(ui, change, list.as_ref(), &names, actions));
            }
        }
        let keyboard = ui.memory(egui::Memory::focused).map_or(ui.ui_contains_pointer(), |focused| focused == tree);
        if keyboard && let Some(selected) = window.selected.clone() {
            let (diff, open) = ui.input_mut(|input| {
                (
                    input.consume_key(egui::Modifiers::COMMAND, Key::D),
                    input.consume_key(egui::Modifiers::NONE, Key::Enter),
                )
            });
            if diff {
                actions.push(CommitAction::Diff(selected.clone()));
            }
            if open {
                actions.push(CommitAction::Open(selected));
            }
        }
    }

    fn set_check_todo(&mut self, change: impl FnOnce(&mut zen_workbench::todo_patterns::TodoSettings)) {
        let mut settings = self.overview.settings.clone();
        change(&mut settings.todo);
        self.overview.settings = settings.clone();
        self.requests.push(super::Request::Settings(Box::new(settings)));
    }

    fn check_todo_row(&self, ui: &mut Ui, actions: &mut Vec<CommitAction>) {
        let todo = &self.overview.settings.todo;
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let shown = todo.commit_filter.clone().unwrap_or_else(|| t!("ide-todo-filter-all"));
            let mut on = todo.commit_check;
            if ui.checkbox(&mut on, t!("vcs-commit-check-todo", filter = shown.as_str())).changed() {
                actions.push(CommitAction::CheckTodo(on));
            }
            ui.menu_button(t!("vcs-commit-check-todo-configure"), |ui| {
                if ui.selectable_label(todo.commit_filter.is_none(), t!("ide-todo-filter-all")).clicked() {
                    actions.push(CommitAction::CheckTodoFilter(None));
                    ui.close();
                }
                for filter in &todo.filters {
                    let selected = todo.commit_filter.as_ref() == Some(&filter.name);
                    if ui.selectable_label(selected, &filter.name).clicked() {
                        actions.push(CommitAction::CheckTodoFilter(Some(filter.name.clone())));
                        ui.close();
                    }
                }
            });
        });
    }

    fn commit_checked(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, push: bool) {
        if self.overview.settings.todo.commit_check {
            let params = json!({ "paths": self.commit_window.checked_paths() });
            let found = registry.dispatch(session, "todo.commit.check", params).ok();
            let found: Vec<CommitTodo> = found.and_then(|found| serde_json::from_value(found).ok()).unwrap_or_default();
            if !found.is_empty() {
                self.commit_window.todo_question = Some((push, found));
                return;
            }
        }
        self.commit(ui, registry, session, push);
    }

    fn todo_question(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some((push, found)) = self.commit_window.todo_question.clone() else { return };
        let p = self.palette;
        let mut choice = None;
        egui::Modal::new(ui.scope_id().with("commit-check-todo")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            let title = t!("vcs-commit-todo-found", count = found.len());
            ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(10.0);
            for item in found.iter().take(TODO_LISTED) {
                let name = item.path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
                ui.label(format!("{name}:{}  {}", item.line + 1, item.text));
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui.button(t!("vcs-commit-todo-commit-anyway")).clicked() {
                    choice = Some(TodoChoice::Commit);
                }
                if ui.button(t!("vcs-commit-todo-review")).clicked() {
                    choice = Some(TodoChoice::Review);
                }
                if ui.button(t!("settings-cancel")).clicked() {
                    choice = Some(TodoChoice::Cancel);
                }
            });
        });
        match choice {
            None => return,
            Some(TodoChoice::Commit) => self.commit(ui, registry, session, push),
            Some(TodoChoice::Review) => {
                let tab = if self.vcs.info.root.is_some() { TodoTab::CurrentChangelist } else { TodoTab::Project };
                let _ = self.todo_window.select_tab(tab, None);
                self.show_tool("todo");
            }
            Some(TodoChoice::Cancel) => {}
        }
        self.commit_window.todo_question = None;
    }

    fn commit_splitter(&mut self, ui: &mut Ui, p: &IdePalette, message: f32, room: f32) {
        let (bar, response) = ui.allocate_exact_size(vec2(ui.available_width(), SPLITTER), Sense::drag());
        let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        crate::accessibility::name_with(&response, egui::Role::Splitter, || t!("vcs-commit-splitter"));
        let color = if response.hovered() || response.dragged() { p.active_tab_border } else { p.frame };
        ui.painter().hline(ui.max_rect().x_range(), bar.center().y, Stroke::new(1.0, color));
        if response.dragged() {
            let most = (room - LEAST_LIST).max(LEAST_MESSAGE);
            let wanted = (message - response.drag_delta().y).clamp(LEAST_MESSAGE, most);
            self.commit_window.message_height = Some(wanted);
        }
    }

    fn commit_form(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<CommitAction>, message: f32) {
        ui.add_space(3.0);
        self.check_todo_row(ui, actions);
        let window = &mut self.commit_window;
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let mut amend = window.amend;
            if ui.checkbox(&mut amend, t!("vcs-commit-amend")).changed() {
                actions.push(CommitAction::Amend(amend));
            }
        });
        ui.add_space(6.0);
        if let Some(author) = window.author.clone() {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(t!("vcs-commit-author", author = author.as_str()))
                        .font(theme::regular(12.5))
                        .color(p.text_secondary),
                );
                if ui.small_button("×").on_hover_text(t!("vcs-commit-author-clear")).clicked() {
                    actions.push(CommitAction::ClearAuthor);
                }
            });
            ui.add_space(4.0);
        }
        let field = egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 0)).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("commit-message")
                .max_height(message)
                .min_scrolled_height(message)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut window.message)
                            .hint_text(t!("vcs-commit-message"))
                            .font(egui::TextStyle::Body)
                            .margin(egui::Margin::same(8))
                            .desired_width(f32::INFINITY)
                            .min_size(vec2(0.0, message)),
                    )
                })
                .inner
        });
        let field = field.inner;
        crate::accessibility::edit(&field, |node| node.set_label(t!("vcs-commit-message")));
        if std::mem::take(&mut window.focus_message) {
            field.request_focus();
        }
        if field.has_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, Key::Enter)) {
            actions.push(CommitAction::Commit { push: false });
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            let commit = if window.amend { t!("vcs-commit-amend-commit") } else { t!("vcs-commit-commit") };
            let free = !window.committing;
            if primary_button(ui, p, &commit).clicked() && free {
                actions.push(CommitAction::Commit { push: false });
            }
            if secondary_button(ui, p, &t!("vcs-commit-and-push")).clicked() && free {
                actions.push(CommitAction::Commit { push: true });
            }
        });
    }

    fn commit(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, push: bool) {
        let window = &self.commit_window;
        let params = json!({
            "message": window.message,
            "paths": window.checked_paths(),
            "amend": window.amend,
            "push": push,
            "author": window.author,
        });
        let create = |session: &mut Session| registry.dispatch(session, "vcs.commit.create", params);
        match zen_workbench::vcs::off_the_frame(session, create) {
            Ok(_) => self.commit_window.committing = true,
            Err(zen_core::DispatchError::Failed { error, .. }) => self.post(ui, error.message),
            Err(other) => self.post(ui, other.to_string()),
        }
    }

    pub(super) fn jump_source(&self, ctx: &egui::Context, session: &Session) -> Option<JumpSource> {
        if self.tool_visible("commit") && self.commit_window.list_has_keyboard(ctx) {
            Some(JumpSource::Commit)
        } else if self.diff_in_front(session) && self.diff.as_ref().is_some_and(|diff| diff.has_keyboard(ctx)) {
            Some(JumpSource::Diff)
        } else {
            None
        }
    }

    pub(super) fn jump_to_source(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, super::agent::ActionError> {
        use super::agent::ActionError::{Dispatch, Refused};
        let line = params.get("line").and_then(serde_json::Value::as_u64).map(|line| line as usize);
        let given: Vec<PathBuf> = params
            .get("paths")
            .and_then(serde_json::Value::as_array)
            .map(|paths| paths.iter().filter_map(serde_json::Value::as_str).map(PathBuf::from).collect())
            .unwrap_or_default();
        let source = self.jump_source(ctx, session).or_else(|| {
            if self.diff_in_front(session) {
                Some(JumpSource::Diff)
            } else {
                self.tool_visible("commit").then_some(JumpSource::Commit)
            }
        });
        let targets: Vec<(PathBuf, Option<usize>)> = if !given.is_empty() {
            given.into_iter().map(|path| (path, line)).collect()
        } else {
            match source {
                Some(JumpSource::Diff) => {
                    let diff = self.diff.as_mut().ok_or_else(|| Refused("no diff is shown".to_owned()))?;
                    if let Some(line) = line {
                        let after = params.get("side").and_then(serde_json::Value::as_str) != Some("before");
                        diff.set_caret(after, line);
                    }
                    diff.jump_target().map(|(path, line)| (path, Some(line))).into_iter().collect()
                }
                Some(JumpSource::Commit) => {
                    self.commit_window.jump_targets().into_iter().map(|path| (path, None)).collect()
                }
                None => Vec::new(),
            }
        };
        if targets.is_empty() {
            return Err(Refused(
                "Jump to Source needs a file: select a change that was not deleted in the Commit window, show a \
                 two-sided diff, or give `paths`"
                    .to_owned(),
            ));
        }
        let mut opened = Vec::new();
        for (path, line) in targets {
            registry.dispatch(session, "file.document.open", json!({ "path": path })).map_err(Dispatch)?;
            if let Some(line) = line {
                let offset = session
                    .state::<zen_workbench::Workbench>()
                    .and_then(zen_workbench::Workbench::active_document)
                    .map(|document| {
                        let text = document.snapshot();
                        let line = line.min(text.len_lines().saturating_sub(1));
                        text.point_to_offset(zen_text::Point::new(line, 0)).unwrap_or_default()
                    })
                    .unwrap_or_default();
                registry
                    .dispatch(session, "navigate.place.goTo", json!({ "path": path, "offset": offset }))
                    .map_err(Dispatch)?;
            }
            opened.push(json!({ "path": path, "line": line }));
        }
        self.editor_view.focus();
        Ok(json!({ "opened": opened }))
    }

    pub(super) fn open_commit(&mut self) {
        self.commit_open = true;
        self.project_open = false;
        self.structure_open = false;
        self.commit_window.tab = CommitTab::Changes;
        self.commit_window.focus_message = true;
    }

    pub(super) fn open_rollback(&mut self, paths: Vec<PathBuf>) {
        let changes: Vec<ChangeInfo> =
            self.commit_window.changes.iter().filter(|change| change.status != "unversioned").cloned().collect();
        if changes.is_empty() {
            return;
        }
        let checked: BTreeSet<PathBuf> = if paths.is_empty() {
            changes.iter().map(|change| change.path.clone()).collect()
        } else {
            paths.into_iter().filter(|path| changes.iter().any(|change| &change.path == path)).collect()
        };
        self.rollback = Some(RollbackDialog { changes, checked, delete_added: false });
    }

    pub(super) fn rollback_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &mut self.rollback else { return };
        let p = self.palette;
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let (mut confirm, mut cancel) = (false, false);
        let modal = egui::Modal::new(ui.scope_id().with("vcs-rollback")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("vcs-rollback-title")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(12.0);
            egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(6).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for change in &dialog.changes {
                        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                        let on = dialog.checked.contains(&change.path);
                        checkbox(
                            ui.painter(),
                            Rect::from_center_size(pos2(row.left() + 12.0, row.center().y), Vec2::splat(14.0)),
                            Some(on),
                            &p,
                        );
                        let relative = change.path.strip_prefix(&root).unwrap_or(&change.path).display().to_string();
                        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &relative));
                        ui.painter().with_clip_rect(row).text(
                            pos2(row.left() + 28.0, row.center().y),
                            Align2::LEFT_CENTER,
                            relative,
                            theme::regular(13.0),
                            status_color(&p, status_of(change)),
                        );
                        if response.clicked() {
                            if on {
                                dialog.checked.remove(&change.path);
                            } else {
                                dialog.checked.insert(change.path.clone());
                            }
                        }
                    }
                });
            });
            ui.add_space(10.0);
            if dialog.changes.iter().any(|change| change.status == "added") {
                ui.checkbox(&mut dialog.delete_added, t!("vcs-rollback-delete-added"));
                ui.add_space(6.0);
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if primary_button(ui, &p, &t!("vcs-rollback-confirm")).clicked() && !dialog.checked.is_empty() {
                    confirm = true;
                }
                if secondary_button(ui, &p, &t!("settings-cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if confirm {
            let params = json!({ "paths": dialog.checked, "deleteAdded": dialog.delete_added });
            self.rollback = None;
            self.dispatch(ui, registry, session, "vcs.changes.rollback", params);
        } else if cancel || modal.should_close() {
            self.rollback = None;
        }
    }
}

pub(super) fn file_label(ui: &Ui, row: Rect, x: f32, change: &ChangeInfo, root: &Path, p: &IdePalette) {
    let name = change.relative.rsplit('/').next().unwrap_or_default().to_owned();
    crate::file_icons::file_icon_at(
        ui.painter(),
        Rect::from_center_size(pos2(row.left() + x, row.center().y), Vec2::splat(15.0)),
        &change.path,
    );
    let painter = ui.painter().with_clip_rect(row);
    let label = painter.text(
        pos2(row.left() + x + 14.0, row.center().y),
        Align2::LEFT_CENTER,
        &name,
        theme::regular(13.0),
        status_color(p, status_of(change)),
    );
    let folder = change.path.parent().and_then(|parent| parent.strip_prefix(root).ok()).map(Path::to_owned);
    if let Some(folder) = folder.filter(|folder| !folder.as_os_str().is_empty()) {
        painter.text(
            pos2(label.right() + 7.0, row.center().y),
            Align2::LEFT_CENTER,
            folder.display().to_string(),
            theme::regular(12.5),
            p.text_secondary,
        );
    }
}

fn list_menu(ui: &mut Ui, list: &ChangelistInfo, files: &[ChangeInfo], actions: &mut Vec<CommitAction>) {
    let paths: Vec<PathBuf> = files.iter().map(|file| file.path.clone()).collect();
    customization::popup(
        ui,
        customization::CHANGELIST_POPUP,
        |_| true,
        |ui, slot| {
            let (enabled, action) = match slot {
                "vcs-changelist-set-active" => {
                    (!list.active, CommitAction::Run("vcs.changelist.setActive", json!({ "name": list.name })))
                }
                "vcs-changelist-edit" => (true, CommitAction::EditChangelist(list.name.clone())),
                "vcs-changelist-delete" => {
                    (!list.active, CommitAction::Run("vcs.changelist.delete", json!({ "name": list.name })))
                }
                "keymap-view-vcs-newChangelist" => (true, CommitAction::NewChangelist(Vec::new())),
                "keymap-view-vcs-shelve" => (!files.is_empty(), CommitAction::Shelve(paths.clone())),
                "vcs-patch-create-local" => (!paths.is_empty(), CommitAction::CreatePatch(paths.clone())),
                _ => (!paths.is_empty(), CommitAction::CopyPatch(paths.clone())),
            };
            if ui.add_enabled(enabled, egui::Button::new(t!(slot))).clicked() {
                actions.push(action);
                ui.close();
            }
        },
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JumpSource {
    Commit,
    Diff,
}

pub(super) const JUMP_TO_SOURCE: &str = "view.vcs.jumpToSource";

fn file_menu(
    ui: &mut Ui,
    change: &ChangeInfo,
    list: Option<&ChangelistInfo>,
    names: &[String],
    actions: &mut Vec<CommitAction>,
) {
    let keys: Vec<String> =
        crate::keymap::current().keystrokes(JUMP_TO_SOURCE).iter().map(ToString::to_string).collect();
    let shown = |slot: &str| match slot {
        "keymap-view-vcs-jumpToSource" => change.status != "deleted",
        "keymap-view-vcs-moveToChangelist" => list.is_some(),
        _ => true,
    };
    customization::popup(ui, customization::COMMIT_FILE_POPUP, shown, |ui, slot| {
        let paths = std::slice::from_ref(&change.path);
        let (shortcut, action) = match slot {
            "keymap-view-vcs-showDiff" => ("⌘D".to_owned(), CommitAction::Diff(change.path.clone())),
            "keymap-view-vcs-rollback" => ("⌥⌘Z".to_owned(), CommitAction::Rollback(paths.to_vec())),
            "keymap-view-vcs-jumpToSource" => (keys.join(", "), CommitAction::Open(change.path.clone())),
            "keymap-view-vcs-moveToChangelist" => {
                let Some(list) = list else { return };
                ui.menu_button(t!("keymap-view-vcs-moveToChangelist"), |ui| {
                    for name in names.iter().filter(|name| **name != list.name) {
                        if ui.button(name).clicked() {
                            let params = json!({ "paths": [change.path], "to": name });
                            actions.push(CommitAction::Run("vcs.changelist.move", params));
                            ui.close();
                        }
                    }
                    if names.len() > 1 {
                        ui.separator();
                    }
                    if ui.button(t!("keymap-view-vcs-newChangelist")).clicked() {
                        actions.push(CommitAction::NewChangelist(vec![change.path.clone()]));
                        ui.close();
                    }
                });
                return;
            }
            "keymap-view-vcs-shelve" => (String::new(), CommitAction::Shelve(paths.to_vec())),
            "vcs-patch-create-local" => (String::new(), CommitAction::CreatePatch(paths.to_vec())),
            _ => (String::new(), CommitAction::CopyPatch(paths.to_vec())),
        };
        if ui.add(egui::Button::new(t!(slot)).shortcut_text(shortcut)).clicked() {
            actions.push(action);
            ui.close();
        }
    });
}

pub(super) fn checkbox(painter: &egui::Painter, rect: Rect, state: Option<bool>, p: &IdePalette) {
    match state {
        Some(false) => {
            painter.rect_stroke(rect, 3, Stroke::new(1.0, p.tab_border), egui::StrokeKind::Inside);
        }
        Some(true) => {
            painter.rect_filled(rect, 3, p.accent);
            let stroke = Stroke::new(1.6, p.accent_text);
            let (a, b, c) = (
                pos2(rect.left() + rect.width() * 0.25, rect.center().y),
                pos2(rect.left() + rect.width() * 0.43, rect.bottom() - rect.height() * 0.28),
                pos2(rect.right() - rect.width() * 0.22, rect.top() + rect.height() * 0.3),
            );
            painter.line_segment([a, b], stroke);
            painter.line_segment([b, c], stroke);
        }
        None => {
            painter.rect_filled(rect, 3, p.accent);
            painter.line_segment(
                [pos2(rect.left() + 3.5, rect.center().y), pos2(rect.right() - 3.5, rect.center().y)],
                Stroke::new(1.6, p.accent_text),
            );
        }
    }
}

fn glyph_refresh(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let center = rect.center();
    let radius = rect.width() * 0.4;
    let points: Vec<egui::Pos2> = (0..=20)
        .map(|step| {
            let angle = -0.6 + step as f32 / 20.0 * 5.0;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    let end = *points.last().unwrap_or(&center);
    painter.add(egui::Shape::line(points, stroke));
    painter.line_segment([end, end + vec2(radius * 0.55, -radius * 0.05)], stroke);
    painter.line_segment([end, end + vec2(0.05 * radius, -radius * 0.55)], stroke);
}

fn glyph_rollback(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let (left, right) = (rect.left() + rect.width() * 0.15, rect.right() - rect.width() * 0.1);
    let top = rect.top() + rect.height() * 0.3;
    let center = pos2((left + right) / 2.0 + rect.width() * 0.1, rect.center().y + rect.height() * 0.08);
    let radius = rect.height() * 0.32;
    let arc: Vec<egui::Pos2> = (0..=12)
        .map(|step| {
            let angle = -std::f32::consts::FRAC_PI_2 + step as f32 / 12.0 * std::f32::consts::PI;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(arc, stroke));
    painter.line_segment([pos2(left, top + radius * 0.0), pos2(center.x, center.y - radius)], stroke);
    painter.line_segment([pos2(left, top), pos2(left + radius * 0.6, top - radius * 0.6)], stroke);
    painter.line_segment([pos2(left, top), pos2(left + radius * 0.6, top + radius * 0.6)], stroke);
    painter
        .line_segment([pos2(center.x, center.y + radius), pos2(left + rect.width() * 0.1, center.y + radius)], stroke);
}

pub(super) fn glyph_diff(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let (y1, y2) = (rect.top() + rect.height() * 0.32, rect.bottom() - rect.height() * 0.32);
    painter.line_segment([pos2(rect.left(), y1), pos2(rect.right(), y1)], stroke);
    painter.line_segment([pos2(rect.right(), y1), pos2(rect.right() - 3.5, y1 - 3.5)], stroke);
    painter.line_segment([pos2(rect.left(), y2), pos2(rect.right(), y2)], stroke);
    painter.line_segment([pos2(rect.left(), y2), pos2(rect.left() + 3.5, y2 + 3.5)], stroke);
}

fn glyph_expand_all(painter: &egui::Painter, rect: Rect, color: Color32) {
    super::glyphs::glyph_expand(painter, rect, color);
}

fn glyph_collapse_all(painter: &egui::Painter, rect: Rect, color: Color32) {
    super::glyphs::glyph_collapse(painter, rect, color);
}

fn conflicts_group(
    ui: &mut Ui,
    p: &IdePalette,
    window: &mut CommitWindow,
    conflicts: &[ChangeInfo],
    actions: &mut Vec<CommitAction>,
) {
    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.hover);
    }
    let chevron = Rect::from_center_size(pos2(row.left() + 20.0, row.center().y), Vec2::splat(11.0));
    if window.conflicts_collapsed {
        crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
    } else {
        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
    }
    let title = t!("vcs-commit-conflicts");
    crate::accessibility::tree_item(&response, 1, Some(!window.conflicts_collapsed), false, &title);
    let label = ui.painter().text(
        pos2(row.left() + 34.0, row.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::regular(13.0),
        p.vcs_conflict,
    );
    let count = ui.painter().text(
        pos2(label.right() + 8.0, row.center().y),
        Align2::LEFT_CENTER,
        t!("vcs-commit-files", count = conflicts.len()),
        theme::regular(12.5),
        p.text_secondary,
    );
    let resolve = t!("vcs-commit-resolve");
    let galley = ui.painter().layout_no_wrap(resolve.clone(), theme::regular(12.5), p.link);
    let link = Rect::from_min_size(pos2(count.right() + 12.0, row.center().y - galley.size().y / 2.0), galley.size());
    let link_response = ui
        .interact(link, ui.scope_id().with("commit-conflicts-resolve"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    crate::accessibility::name(&link_response, egui::Role::Link, &resolve);
    if link_response.hovered() {
        ui.painter().line_segment([link.left_bottom(), link.right_bottom()], Stroke::new(1.0, p.link));
    }
    ui.painter().galley(link.min, galley, p.link);
    if link_response.clicked() {
        actions.push(CommitAction::Conflict("vcs.conflict.open", json!({})));
    } else if response.clicked() {
        window.conflicts_collapsed = !window.conflicts_collapsed;
    }
    if window.conflicts_collapsed {
        return;
    }
    for change in conflicts {
        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
        let selected = window.selected.as_ref() == Some(&change.path);
        if selected {
            ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.hover);
        }
        crate::accessibility::tree_item(&response, 2, None, selected, &change.relative);
        let name = change.relative.rsplit('/').next().unwrap_or_default().to_owned();
        crate::file_icons::file_icon_at(
            ui.painter(),
            Rect::from_center_size(pos2(row.left() + 58.0, row.center().y), Vec2::splat(15.0)),
            &change.path,
        );
        let painter = ui.painter().with_clip_rect(row);
        let label = painter.text(
            pos2(row.left() + 72.0, row.center().y),
            Align2::LEFT_CENTER,
            &name,
            theme::regular(13.0),
            status_color(p, FileStatus::Conflicted),
        );
        if let Some((folder, _)) = change.relative.rsplit_once('/') {
            painter.text(
                pos2(label.right() + 7.0, row.center().y),
                Align2::LEFT_CENTER,
                folder,
                theme::regular(12.5),
                p.text_secondary,
            );
        }
        let path = json!({ "path": change.path });
        if response.double_clicked() {
            actions.push(CommitAction::Conflict("vcs.conflict.open", path.clone()));
        } else if response.clicked() || response.secondary_clicked() {
            window.selected = Some(change.path.clone());
        }
        response.context_menu(|ui| {
            customization::popup(
                ui,
                customization::CONFLICT_POPUP,
                |_| true,
                |ui, slot| {
                    if ui.button(t!(slot)).clicked() {
                        actions.push(match slot {
                            "vcs-commit-resolve" => CommitAction::Conflict("vcs.conflict.open", path.clone()),
                            _ => {
                                let take = if slot == "vcs-commit-accept-ours" { "ours" } else { "theirs" };
                                let params = json!({ "path": change.path, "take": take });
                                CommitAction::Conflict("vcs.conflict.resolve", params)
                            }
                        });
                        ui.close();
                    }
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, status: &str) -> ChangeInfo {
        ChangeInfo {
            path: PathBuf::from(path),
            relative: path.trim_start_matches('/').to_owned(),
            status: status.to_owned(),
            old_path: None,
            staged: false,
            staged_status: None,
            unstaged_status: Some(status.to_owned()),
        }
    }

    fn list(name: &str, active: bool, files: Vec<ChangeInfo>) -> ChangelistInfo {
        ChangelistInfo { name: name.to_owned(), comment: String::new(), active, files }
    }

    fn git(root: &Path, arguments: &[&str]) -> String {
        let output = std::process::Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap();
        assert!(output.status.success(), "{arguments:?}: {output:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn the_list_and_the_message_give_way_before_the_buttons() {
        assert_eq!(commit_heights(600.0, 150.0, 120.0), (600.0 - 150.0 - SPLITTER - 120.0, 120.0));
        let (list, message) = commit_heights(260.0, 150.0, 120.0);
        assert!(list >= LEAST_LIST.min((260.0 - 150.0 - SPLITTER) / 2.0) && list + message + 150.0 + SPLITTER <= 260.0);
        assert_eq!(commit_heights(100.0, 150.0, 120.0), (0.0, 0.0), "a tiny window keeps only the form's rows");
        assert_eq!(commit_heights(600.0, 150.0, 10.0).1, LEAST_MESSAGE);
    }

    fn commit_view(root: &Path, message: &str) -> (IdeView, Registry, Session) {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        let product = std::sync::Arc::new(super::super::editor_groups::tests::product());
        let mut view = IdeView::new(product, crate::plugins::Plugins::new(None));
        view.vcs.info.root = Some(root.to_owned());
        let files =
            (0..40).map(|at| change(&root.join(format!("f{at}.go")).display().to_string(), "modified")).collect();
        view.commit_window.update(ChangelistReport {
            lists: vec![list("Changes", true, files)],
            unversioned: Vec::new(),
            revision: 1,
        });
        view.commit_window.message = message.to_owned();
        (view, registry, session)
    }

    #[test]
    fn the_commit_buttons_stay_inside_a_short_window() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let long = (1..=60).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n");
        for (height, message) in [(260.0, "fix"), (320.0, long.as_str()), (700.0, long.as_str())] {
            let (mut view, registry, mut session) = commit_view(&root, message);
            let ctx = egui::Context::default();
            let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(360.0, height));
            let mut used = Rect::NOTHING;
            for _ in 0..3 {
                let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut output = ctx.run_ui(input, |ui| {
                    view.commit_tool_window(ui, &registry, &mut session);
                    used = ui.min_rect();
                });
                output.textures_delta.clear();
            }
            assert!(used.bottom() <= height + 0.5, "{height}: the form ends at {}", used.bottom());
        }
    }

    #[test]
    fn check_todo_stops_a_commit_with_a_todo_in_the_changed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("a.go"), "package a\n").unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        std::fs::write(root.join("a.go"), "package a\n\n// TODO later\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        super::super::todo_window::tests::comments_by_slashes(&mut session);
        let product = std::sync::Arc::new(super::super::editor_groups::tests::product());
        let mut view = IdeView::new(product, crate::plugins::Plugins::new(None));
        view.overview.settings.todo.commit_check = true;
        view.overview.settings.apply_to_session(session.settings_mut());
        view.commit_window.update(ChangelistReport {
            lists: vec![list("Changes", true, vec![change(&root.join("a.go").display().to_string(), "modified")])],
            unversioned: Vec::new(),
            revision: 1,
        });
        view.commit_window.message = "second".to_owned();
        let ctx = egui::Context::default();
        let pass = |view: &mut IdeView, session: &mut Session| {
            let mut output =
                ctx.run_ui(egui::RawInput::default(), |ui| view.commit_checked(ui, &registry, session, false));
            output.textures_delta.clear();
        };
        pass(&mut view, &mut session);
        let question = view.commit_window.todo_question.clone().expect("the commit stops and asks");
        assert_eq!(question.1[0].text, "TODO later");
        assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]), "1", "nothing is committed");

        view.commit_window.todo_question = None;
        view.overview.settings.todo.commit_check = false;
        view.overview.settings.apply_to_session(session.settings_mut());
        pass(&mut view, &mut session);
        assert!(view.commit_window.todo_question.is_none());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while git(&root, &["rev-list", "--count", "HEAD"]) != "2" && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]), "2", "with the check off it just commits");
    }

    #[test]
    fn tracked_changes_go_in_and_new_files_stay_out_until_checked() {
        let mut window = CommitWindow::default();
        window.update(ChangelistReport {
            lists: vec![list("Changes", true, vec![change("/r/a.go", "modified"), change("/r/b.go", "added")])],
            unversioned: vec![change("/r/new.go", "unversioned")],
            revision: 1,
        });
        assert_eq!(window.checked_paths(), [PathBuf::from("/r/a.go"), PathBuf::from("/r/b.go")]);
        let new = window.changes[2].clone();
        window.set_checked(&new, true);
        let a = window.changes[0].clone();
        window.set_checked(&a, false);
        assert_eq!(window.checked_paths(), [PathBuf::from("/r/b.go"), PathBuf::from("/r/new.go")]);
        window.update(ChangelistReport {
            lists: vec![list("Changes", true, vec![change("/r/b.go", "added")])],
            unversioned: vec![change("/r/new.go", "unversioned")],
            revision: 2,
        });
        assert_eq!(window.checked_paths(), [PathBuf::from("/r/b.go"), PathBuf::from("/r/new.go")]);
        assert!(window.excluded.is_empty());
    }

    #[test]
    fn changelist_actions_take_the_selected_file_or_the_checked_tracked_ones() {
        let mut window = CommitWindow::default();
        window.update(ChangelistReport {
            lists: vec![
                list("Changes", true, vec![change("/r/a.go", "modified")]),
                list("Feature", false, vec![change("/r/b.go", "modified"), change("/r/c.go", "added")]),
            ],
            unversioned: vec![change("/r/new.go", "unversioned")],
            revision: 1,
        });
        assert_eq!(window.active_list(), "Changes");
        assert_eq!(window.list_names(), ["Changes", "Feature"]);
        assert_eq!(window.list_of(Path::new("/r/c.go")).map(|list| list.name.as_str()), Some("Feature"));
        let c = window.changes[2].clone();
        window.set_checked(&c, false);
        let new = window.changes[3].clone();
        window.set_checked(&new, true);
        assert_eq!(
            window.tracked_context(),
            [PathBuf::from("/r/a.go"), PathBuf::from("/r/b.go")],
            "new files are in no list"
        );
        window.selected = Some(PathBuf::from("/r/c.go"));
        assert_eq!(window.tracked_context(), [PathBuf::from("/r/c.go")]);
        window.selected = Some(PathBuf::from("/r/new.go"));
        assert_eq!(window.tracked_context().len(), 2);
    }

    #[test]
    fn selecting_a_changed_file_unfolds_its_group() {
        let mut window = CommitWindow::default();
        window.update(ChangelistReport {
            lists: vec![list("Feature", false, vec![change("/r/b.go", "modified")])],
            unversioned: vec![change("/r/new.go", "unversioned")],
            revision: 1,
        });
        window.collapsed.insert("Feature".to_owned());
        window.unversioned_collapsed = true;
        window.tab = CommitTab::Shelf;
        assert!(window.is_changed(Path::new("/r/b.go")));
        assert!(window.select(Path::new("/r/b.go")));
        assert_eq!(window.selected(), Some(PathBuf::from("/r/b.go")));
        assert!(window.collapsed.is_empty());
        assert_eq!(window.tab, CommitTab::Changes);
        assert!(window.select(Path::new("/r/new.go")));
        assert!(!window.unversioned_collapsed);
        assert!(!window.is_changed(Path::new("/r/clean.go")));
        assert!(!window.select(Path::new("/r/clean.go")));
        assert_eq!(window.selected(), Some(PathBuf::from("/r/new.go")));
    }
}
