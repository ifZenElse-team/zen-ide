use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{CursorIcon, Id, LayerId, Order, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::groups::{GroupId, SplitDirection, SplitNode};
use zen_workbench::tabs::EditorTabSettings;

use crate::accessibility;

use super::breadcrumbs::Breadcrumbs;
use super::editor_view::EditorView;
use super::{IdeView, customization, theme};

const DIVIDER_GRIP: f32 = 6.0;

pub(super) struct TabRow {
    pub(super) scroll: f32,
    pub(super) last_scroll: f64,
    pub(super) revealed: Option<usize>,
    pub(super) revealed_in: (f32, usize, f32),
    pub(super) drag: Option<(usize, f32)>,
    pub(super) place: Option<usize>,
}

impl Default for TabRow {
    fn default() -> Self {
        Self {
            scroll: 0.0,
            last_scroll: f64::NEG_INFINITY,
            revealed: None,
            drag: None,
            place: None,
            revealed_in: (0.0, 0, 0.0),
        }
    }
}

struct DraggedTab {
    group: GroupId,
    index: usize,
    name: String,
}

pub(super) struct GroupsUi {
    views: HashMap<GroupId, EditorView>,
    crumbs: HashMap<GroupId, Breadcrumbs>,
    pub(super) view_group: GroupId,
    rows: HashMap<GroupId, TabRow>,
    pub(super) extras_group: GroupId,
    extras_shown: bool,
    dragged: Option<DraggedTab>,
    count: usize,
    pub(super) inert: bool,
    pub(super) workspace_tabs: Vec<(String, Rect)>,
    pub(super) tab_rows: HashMap<GroupId, PlacedRow>,
    pub(super) opened_windows: Vec<(GroupId, Option<zen_workbench::groups::WindowBounds>)>,
    pub(super) focused_window: Option<GroupId>,
}

impl Default for GroupsUi {
    fn default() -> Self {
        Self {
            views: HashMap::new(),
            crumbs: HashMap::new(),
            view_group: 1,
            rows: HashMap::new(),
            extras_group: 1,
            extras_shown: false,
            dragged: None,
            count: 1,
            inert: false,
            workspace_tabs: Vec::new(),
            tab_rows: HashMap::new(),
            opened_windows: Vec::new(),
            focused_window: None,
        }
    }
}

impl GroupsUi {
    pub(super) fn take_row(&mut self, group: GroupId) -> TabRow {
        self.rows.remove(&group).unwrap_or_default()
    }

    pub(super) fn put_row(&mut self, group: GroupId, row: TabRow) {
        self.rows.insert(group, row);
    }

    pub(super) fn drag_tab(&mut self, group: GroupId, index: usize, name: String) {
        self.dragged = Some(DraggedTab { group, index, name });
    }

    pub(super) fn image_slots(&self) -> usize {
        self.count.max(1)
    }
}

pub(super) type PlacedRow = (Vec<(usize, Rect)>, bool, Option<usize>);

pub(super) fn place_in_row(row: &[(usize, Rect)], dragged: usize, at: Pos2, vertical: bool) -> Option<usize> {
    let whole = row.iter().map(|(_, rect)| *rect).reduce(|a, b| a.union(b))?;
    if !whole.expand(8.0).contains(at) {
        return None;
    }
    let along = |point: Pos2| if vertical { point.y } else { point.x };
    Some(row.iter().filter(|(index, rect)| *index != dragged && along(rect.center()) < along(at)).count())
}

pub(super) fn tab_titles(tabs: &[(String, bool, PathBuf)], settings: &EditorTabSettings) -> Vec<String> {
    tabs.iter()
        .map(|(name, dirty, path)| {
            let same = tabs.iter().filter(|(other, _, _)| other == name).count();
            let shown = if settings.show_extension {
                name.clone()
            } else {
                path.file_stem().map_or_else(|| name.clone(), |stem| stem.to_string_lossy().into_owned())
            };
            let folder = path.parent().and_then(Path::file_name).filter(|_| settings.show_directory && same > 1);
            let shown = folder.map_or(shown.clone(), |folder| format!("{}/{shown}", folder.to_string_lossy()));
            if *dirty && settings.mark_modified { format!("*{shown}") } else { shown }
        })
        .collect()
}

pub(super) struct TabMenu<'a> {
    pub(super) group: GroupId,
    pub(super) index: usize,
    pub(super) path: &'a Path,
    pub(super) pins: &'a [bool],
    pub(super) previews: &'a [bool],
    pub(super) split: bool,
    pub(super) opposite: bool,
    pub(super) bookmarked: bool,
    pub(super) reopen: bool,
}

pub(super) enum TabChoice {
    Command(&'static str, Value),
    Tree(super::TreeAction),
}

pub(super) fn tab_menu(ui: &mut Ui, menu: &TabMenu<'_>) -> Option<TabChoice> {
    let mut chosen = None;
    let (group, index, path) = (menu.group, menu.index, menu.path);
    let tab = json!({ "group": group, "index": index });
    let whole = json!({ "group": group });
    let unpinned = |range: std::ops::Range<usize>| menu.pins.get(range).is_some_and(|pins| pins.contains(&false));
    let left = unpinned(0..index);
    let right = unpinned(index + 1..menu.pins.len());
    let pinned = menu.pins.get(index).copied().unwrap_or(false);
    let any_unpinned = unpinned(0..menu.pins.len());
    let mut item = |ui: &mut Ui, label: String, action: &str, enabled: bool, choice: TabChoice| {
        let keys = crate::keymap::shortcut(action).unwrap_or_default();
        let button = egui::Button::new(label.as_str());
        let button = if keys.is_empty() { button } else { button.shortcut_text(keys) };
        let response = ui.add_enabled(enabled, button);
        accessibility::name_with(&response, egui::Role::Button, || label.clone());
        if response.clicked() {
            chosen = Some(choice);
            ui.close();
        }
    };
    let command = |id: &'static str, params: Value| TabChoice::Command(id, params);
    let closing: [(&'static str, Value, bool); 8] = [
        ("file.document.close", tab.clone(), true),
        ("file.document.closeOthers", tab.clone(), menu.pins.len() > 1),
        ("file.document.closeAll", whole.clone(), true),
        ("file.document.closeUnmodified", whole.clone(), true),
        ("file.document.closeAllButPinned", whole.clone(), any_unpinned),
        ("file.document.closeLeft", tab.clone(), left),
        ("file.document.closeRight", tab.clone(), right),
        ("file.document.closeReadOnly", whole, true),
    ];
    let splits: [(&str, &'static str, Value); 4] = [
        ("editor.split.right", "editor.split.right", tab.clone()),
        ("editor.split.right/move", "editor.split.right", json!({ "group": group, "index": index, "move": true })),
        ("editor.split.down", "editor.split.down", tab.clone()),
        ("editor.split.down/move", "editor.split.down", json!({ "group": group, "index": index, "move": true })),
    ];
    let file = json!({ "path": path, "whole": true });
    let shown = |slot: &str| !matches!(slot, "editor.split.unsplit" | "editor.split.unsplitAll") || menu.split;
    customization::popup(ui, customization::TAB_POPUP, shown, |ui, slot| {
        if let Some((id, params, enabled)) = closing.iter().find(|(id, ..)| *id == slot) {
            let id = *id;
            item(ui, label(id), id, *enabled, command(id, params.clone()));
            return;
        }
        if let Some((action, id, params)) = splits.iter().find(|(action, ..)| *action == slot) {
            item(ui, label(action), action, true, command(id, params.clone()));
            return;
        }
        match slot {
            "ide-tab-copy-reference" => {
                ui.menu_button(t!("ide-tab-copy-reference"), |ui| {
                    let copy = command("project.file.copyPath", json!({ "paths": [path] }));
                    item(ui, t!("ide-tab-copy-absolute-path"), "project.file.copyPath", true, copy);
                    for (key, form) in [
                        ("ide-tab-copy-file-name", "fileName"),
                        ("ide-tab-copy-from-project", "fromProject"),
                        ("ide-tab-copy-from-repository", "fromRepository"),
                    ] {
                        let copy = command("project.file.copyPath", json!({ "paths": [path], "form": form }));
                        item(ui, t!(key), "", true, copy);
                    }
                });
            }
            "editor.split.moveToOpposite" => {
                let opposite = "editor.split.moveToOpposite";
                item(ui, label(opposite), opposite, menu.opposite, command(opposite, tab.clone()));
            }
            "editor.window.openTab" => {
                let window = "editor.window.openTab";
                item(ui, label(window), window, true, command(window, tab.clone()));
            }
            "editor.split.unsplit" | "editor.split.unsplitAll" => {
                let id =
                    if slot == "editor.split.unsplit" { "editor.split.unsplit" } else { "editor.split.unsplitAll" };
                item(ui, label(id), id, true, command(id, json!({})));
            }
            "ide-tab-pin-active" => {
                let pin_label = if pinned { t!("ide-tab-unpin-active") } else { t!("ide-tab-pin-active") };
                let pin = json!({ "group": group, "index": index, "pinned": !pinned });
                item(ui, pin_label, "editor.tab.pin", true, command("editor.tab.pin", pin));
            }
            "editor.tab.keepOpen" => {
                let keep = "editor.tab.keepOpen";
                let preview = menu.previews.get(index).copied().unwrap_or(false);
                item(ui, label(keep), keep, preview, command(keep, tab.clone()));
            }
            "view.settings.open/editorTabs" => {
                let configure = "view.settings.open/editorTabs";
                let page = command("view.settings.open", json!({ "page": "editorTabs" }));
                item(ui, label(configure), configure, true, page);
            }
            "file.document.reopenClosed" => {
                let reopen = "file.document.reopenClosed";
                item(ui, label(reopen), reopen, menu.reopen, command(reopen, json!({})));
            }
            "ide-tool-bookmarks" => {
                ui.menu_button(t!("ide-tool-bookmarks"), |ui| {
                    ui.set_min_width(240.0);
                    let toggle = command("navigate.bookmark.toggle", file.clone());
                    item(ui, t!("keymap-navigate-bookmark-toggle"), "navigate.bookmark.toggle", true, toggle);
                    let mnemonic = command("view.bookmark.mnemonic", file.clone());
                    item(ui, t!("ide-bookmark-toggle-mnemonic"), "view.bookmark.mnemonic", true, mnemonic);
                    let remove = command("navigate.bookmark.remove", file.clone());
                    item(ui, t!("ide-bookmarks-delete"), "", menu.bookmarked, remove);
                    ui.separator();
                    item(
                        ui,
                        t!("keymap-view-bookmark-openTabs"),
                        "",
                        true,
                        command("view.bookmark.openTabs", json!({})),
                    );
                });
            }
            "ide-tree-open-in" => {
                ui.menu_button(t!("ide-tree-open-in"), |ui| {
                    let reveal = TabChoice::Tree(super::TreeAction::Reveal(path.to_owned()));
                    item(ui, super::file_manager_label(), "", true, reveal);
                    let terminal = TabChoice::Tree(super::TreeAction::OpenInTerminal(path.to_owned()));
                    item(ui, t!("ide-tree-open-in-terminal"), "", true, terminal);
                });
            }
            "menu-file-local-history" => {
                ui.menu_button(t!("menu-file-local-history"), |ui| {
                    let show = TabChoice::Tree(super::TreeAction::ShowHistory(path.to_owned()));
                    item(ui, t!("keymap-view-localHistory-show"), "", true, show);
                    let put = TabChoice::Tree(super::TreeAction::PutLabel);
                    item(ui, t!("keymap-view-localHistory-putLabel"), "", true, put);
                });
            }
            _ => {}
        }
    });
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

fn label(action: &str) -> String {
    t!(&format!("keymap-{}", action.replace(['.', '/'], "-")))
}

pub(super) struct Divider {
    before: GroupId,
    direction: SplitDirection,
    line: Rect,
    parent: Rect,
}

pub(super) fn layout(node: &SplitNode, rect: Rect, groups: &mut Vec<(GroupId, Rect)>, dividers: &mut Vec<Divider>) {
    match node {
        SplitNode::Group(group) => groups.push((group.id, rect)),
        SplitNode::Split { direction, ratio, first, second } => {
            let (a, b, line) = match direction {
                SplitDirection::Right => {
                    let x = (rect.left() + rect.width() * ratio).round();
                    (
                        Rect::from_min_max(rect.min, pos2(x - 0.5, rect.bottom())),
                        Rect::from_min_max(pos2(x + 0.5, rect.top()), rect.max),
                        Rect::from_min_max(pos2(x - 0.5, rect.top()), pos2(x + 0.5, rect.bottom())),
                    )
                }
                SplitDirection::Down => {
                    let y = (rect.top() + rect.height() * ratio).round();
                    (
                        Rect::from_min_max(rect.min, pos2(rect.right(), y - 0.5)),
                        Rect::from_min_max(pos2(rect.left(), y + 0.5), rect.max),
                        Rect::from_min_max(pos2(rect.left(), y - 0.5), pos2(rect.right(), y + 0.5)),
                    )
                }
            };
            dividers.push(Divider { before: first_group(second), direction: *direction, line, parent: rect });
            layout(first, a, groups, dividers);
            layout(second, b, groups, dividers);
        }
    }
}

pub(super) fn grip(divider: &Divider) -> (Rect, CursorIcon) {
    match divider.direction {
        SplitDirection::Right => (divider.line.expand2(vec2(DIVIDER_GRIP / 2.0, 0.0)), CursorIcon::ResizeHorizontal),
        SplitDirection::Down => (divider.line.expand2(vec2(0.0, DIVIDER_GRIP / 2.0)), CursorIcon::ResizeVertical),
    }
}

fn first_group(node: &SplitNode) -> GroupId {
    match node {
        SplitNode::Group(group) => group.id,
        SplitNode::Split { first, .. } => first_group(first),
    }
}

fn group_scope(area: Id, group: GroupId) -> Id {
    area.with(("editor-group", group))
}

fn editor_scroll_id(scope: Id, path: &Path) -> Id {
    scope
        .with(egui::IdSalt::new("editor-area"))
        .with(egui::IdSalt::new("child"))
        .with(egui::IdSalt::new(("editor", path.to_path_buf())))
}

impl IdeView {
    pub(super) fn follow_groups(&mut self, ctx: &egui::Context, area: Id, session: &Session) {
        let Some(workbench) = session.state::<Workbench>() else { return };
        let groups = workbench.groups();
        let focused = groups.focused();
        if focused != self.groups_ui.view_group {
            let previous = self.groups_ui.view_group;
            let view = match self.groups_ui.views.remove(&focused) {
                Some(view) => view,
                None => {
                    let shown = |id| groups.group(id).and_then(|group| group.active_path()).map(Path::to_owned);
                    if let Some(path) = shown(focused).filter(|path| shown(previous).as_ref() == Some(path)) {
                        let from = editor_scroll_id(group_scope(area, previous), &path);
                        if let Some(state) = egui::scroll_area::State::load(ctx, from) {
                            state.store(ctx, editor_scroll_id(group_scope(area, focused), &path));
                        }
                    }
                    EditorView::default()
                }
            };
            let old = std::mem::replace(&mut self.editor_view, view);
            self.groups_ui.views.insert(previous, old);
            let crumbs = self.groups_ui.crumbs.remove(&focused).unwrap_or_default();
            let old = std::mem::replace(&mut self.breadcrumbs, crumbs);
            self.groups_ui.crumbs.insert(previous, old);
            self.groups_ui.view_group = focused;
            self.editor_view.focus();
        }
        self.groups_ui.count = if groups.is_split() { groups.groups().len() } else { 1 };
        self.groups_ui.views.retain(|id, _| groups.group(*id).is_some());
        self.groups_ui.crumbs.retain(|id, _| groups.group(*id).is_some());
        self.groups_ui.rows.retain(|id, _| groups.group(*id).is_some());
        let extras = self.diff.is_some() && !self.diff_from_workspace;
        let main = groups.main_focused();
        if (extras && !self.groups_ui.extras_shown)
            || groups.groups().iter().all(|group| group.id != self.groups_ui.extras_group)
        {
            self.groups_ui.extras_group = main;
        }
        self.groups_ui.extras_shown = extras;
    }

    pub(super) fn waiting_group(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        id: GroupId,
        body_only: bool,
    ) -> bool {
        session.state_mut::<Workbench>().present(id);
        let mut view = self.groups_ui.views.remove(&id).unwrap_or_default();
        let mut crumbs = self.groups_ui.crumbs.remove(&id).unwrap_or_default();
        std::mem::swap(&mut self.editor_view, &mut view);
        std::mem::swap(&mut self.breadcrumbs, &mut crumbs);
        if body_only {
            self.group_text(ui, registry, session, id, false);
        } else {
            self.editor_group(ui, registry, session, id, false);
        }
        std::mem::swap(&mut self.editor_view, &mut view);
        std::mem::swap(&mut self.breadcrumbs, &mut crumbs);
        self.groups_ui.crumbs.insert(id, crumbs);
        let stolen = view.has_focus_in(ui.ctx());
        self.groups_ui.views.insert(id, view);
        let workbench = session.state_mut::<Workbench>();
        workbench.present(workbench.groups().focused());
        stolen
    }

    pub(super) fn split_editors(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) -> bool {
        let area_id = ui.scope_id();
        self.groups_ui.workspace_tabs.clear();
        self.follow_groups(ui.ctx(), area_id, session);
        let groups = session.state::<Workbench>().map(|workbench| workbench.groups().clone()).unwrap_or_default();
        let area = ui.available_rect_before_wrap();
        if !groups.is_split() {
            let group = first_group(groups.root());
            let mut child = ui.new_child(UiBuilder::new().scope_id(group_scope(area_id, group)).max_rect(area));
            let text = if group == self.groups_ui.view_group {
                self.editor_group(&mut child, registry, session, group, true)
            } else {
                self.waiting_group(&mut child, registry, session, group, false);
                let pressed = ui
                    .input(|input| input.pointer.any_pressed().then(|| input.pointer.interact_pos()).flatten())
                    .filter(|at| area.contains(*at) && ui.ctx().layer_id_at(*at) == Some(ui.layer_id()));
                if pressed.is_some() && !self.groups_ui.inert {
                    let _ = registry.dispatch(session, "editor.split.focus", json!({ "group": group }));
                }
                false
            };
            ui.advance_cursor_after_rect(area);
            if !self.groups_ui.inert {
                self.dragged_tab(ui, registry, session, &[], None);
            }
            return text;
        }
        let (mut placed, mut dividers) = (Vec::new(), Vec::new());
        layout(groups.root(), area, &mut placed, &mut dividers);
        let pressed = ui
            .input(|input| input.pointer.any_pressed().then(|| input.pointer.interact_pos()).flatten())
            .filter(|_| !self.groups_ui.inert)
            .filter(|at| ui.ctx().layer_id_at(*at) == Some(ui.layer_id()))
            .filter(|at| !dividers.iter().any(|divider| grip(divider).0.contains(*at)));
        let mut text = false;
        let mut promote = None;
        let mut stolen = false;
        for (id, rect) in &placed {
            if session.state::<Workbench>().is_none_or(|workbench| workbench.groups().group(*id).is_none()) {
                continue;
            }
            let mut child = ui.new_child(UiBuilder::new().scope_id(group_scope(area_id, *id)).max_rect(*rect));
            if *id == self.groups_ui.view_group {
                session.state_mut::<Workbench>().present(*id);
                text = self.editor_group(&mut child, registry, session, *id, true);
                let workbench = session.state_mut::<Workbench>();
                workbench.present(workbench.groups().focused());
            } else {
                let took = self.waiting_group(&mut child, registry, session, *id, false);
                if pressed.is_some_and(|at| rect.contains(at)) {
                    promote = Some(*id);
                } else if took {
                    stolen = true;
                }
            }
        }
        ui.advance_cursor_after_rect(area);
        if !self.groups_ui.inert {
            self.dividers(ui, registry, session, &dividers);
            self.dragged_tab(ui, registry, session, &placed, None);
        }
        if let Some(group) = promote {
            let _ = registry.dispatch(session, "editor.split.focus", json!({ "group": group }));
        } else if stolen && self.groups_ui.view_group == groups.focused() && groups.expanded().is_none() {
            self.editor_view.focus();
        }
        text
    }

    pub(super) fn dividers(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, dividers: &[Divider]) {
        let p = self.palette;
        for divider in dividers {
            ui.painter().rect_filled(divider.line, 0, p.frame);
            let (grip, cursor) = grip(divider);
            let response = ui.interact(grip, ui.scope_id().with(("split-divider", divider.before)), Sense::drag());
            accessibility::name_with(&response, egui::Role::Splitter, || t!("a11y-editor-divider"));
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(cursor);
            }
            if response.dragged()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let ratio = match divider.direction {
                    SplitDirection::Right => (pointer.x - divider.parent.left()) / divider.parent.width(),
                    SplitDirection::Down => (pointer.y - divider.parent.top()) / divider.parent.height(),
                };
                let params = json!({ "before": divider.before, "ratio": ratio });
                let _ = registry.dispatch(session, "editor.split.resize", params);
            }
        }
    }

    pub(super) fn dragged_tab(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        placed: &[(GroupId, Rect)],
        window: Option<GroupId>,
    ) {
        let Some(dragged) = &self.groups_ui.dragged else { return };
        let from_window =
            session.state::<Workbench>().is_some_and(|workbench| workbench.groups().window(dragged.group).is_some());
        if window != from_window.then_some(dragged.group) {
            return;
        }
        let (pointer, down, released) = ui.input(|input| {
            (input.pointer.interact_pos(), input.pointer.primary_down(), input.pointer.primary_released())
        });
        let outside = pointer.is_none_or(|at| !ui.ctx().content_rect().contains(at));
        let in_row = pointer.and_then(|at| {
            let (row, vertical, place) = self.groups_ui.tab_rows.get(&dragged.group)?;
            let to = place_in_row(row, dragged.index, at, *vertical)?;
            let to = place.filter(|_| !*vertical).unwrap_or(to);
            Some((to, row.clone(), *vertical))
        });
        let workspace = pointer
            .and_then(|at| self.groups_ui.workspace_tabs.iter().find(|(_, rect)| rect.contains(at)))
            .map(|(id, rect)| (id.clone(), *rect));
        let target = pointer
            .and_then(|at| placed.iter().find(|(_, rect)| rect.contains(at)))
            .map(|(id, rect)| (*id, *rect))
            .filter(|(id, _)| *id != dragged.group && workspace.is_none());
        if released {
            if let Some((to, _, _)) = &in_row {
                if *to != dragged.index {
                    let params = json!({ "group": dragged.group, "index": dragged.index, "to": to });
                    self.dispatch(ui, registry, session, "editor.tab.move", params);
                }
            } else if let Some((id, _)) = &workspace {
                let params = json!({ "from": dragged.group, "index": dragged.index, "workspace": id });
                self.dispatch(ui, registry, session, "editor.split.moveTab", params);
            } else if let Some((to, _)) = target {
                let params = json!({ "from": dragged.group, "index": dragged.index, "to": to });
                self.dispatch(ui, registry, session, "editor.split.moveTab", params);
            } else if outside {
                let tab = (dragged.group, dragged.index);
                self.drop_outside(ui, registry, session, tab, pointer);
            }
            self.groups_ui.dragged = None;
            return;
        }
        if !down {
            self.groups_ui.dragged = None;
            return;
        }
        let p = self.palette;
        let painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, ui.scope_id().with("dragged-tab")));
        if let Some((_, rect)) = &workspace {
            painter.rect_stroke(*rect, 7, Stroke::new(2.0, p.active_tab_border), StrokeKind::Inside);
        }
        if let Some((_, rect)) = target.filter(|_| in_row.is_none()) {
            painter.rect_stroke(rect.shrink(1.0), 4, Stroke::new(2.0, p.active_tab_border), StrokeKind::Inside);
        }
        if let Some((to, row, vertical)) = in_row.as_ref().filter(|(to, _, vertical)| *to != dragged.index && *vertical)
        {
            let others: Vec<Rect> =
                row.iter().filter(|(index, _)| *index != dragged.index).map(|(_, rect)| *rect).collect();
            let stroke = Stroke::new(2.0, p.active_tab_border);
            let edge = |rect: &Rect, after: bool| match (vertical, after) {
                (false, false) => (rect.left() - 2.0, true),
                (false, true) => (rect.right() + 2.0, true),
                (true, false) => (rect.top() - 2.0, false),
                (true, true) => (rect.bottom() + 2.0, false),
            };
            let marked = others.get(*to).map(|rect| (rect, false)).or_else(|| others.last().map(|rect| (rect, true)));
            if let Some((rect, after)) = marked {
                match edge(rect, after) {
                    (x, true) => painter.vline(x, rect.y_range(), stroke),
                    (y, false) => painter.hline(rect.x_range(), y, stroke),
                };
            }
        }
        let follows = in_row.as_ref().is_some_and(|(_, _, vertical)| !vertical);
        if let Some(at) = pointer.filter(|_| !follows) {
            let galley = painter.layout_no_wrap(dragged.name.clone(), theme::regular(13.0), p.text);
            let label = Rect::from_min_size(at + vec2(12.0, 8.0), galley.size() + vec2(20.0, 10.0));
            painter.rect_filled(label, 6, p.active_tab);
            painter.rect_stroke(label, 6, Stroke::new(1.0, p.active_tab_border), StrokeKind::Inside);
            painter.galley(label.min + vec2(10.0, 5.0), galley, p.text);
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
