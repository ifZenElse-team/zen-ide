use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{Align2, Event, Key, Modifiers, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::bookmarks::Bookmarks;
use zen_workbench::debug::Breakpoints;

use super::editor_view::bookmark_icon;
use super::glyphs::{glyph_bookmark, glyph_minus, glyph_more};
use super::tool_stripes::{MenuRequest, layout_menu};
use super::tool_windows::ToolWindowLayout;
use super::{IdePalette, IdeView, Side, customization, hint, hover, metrics};
use crate::{accessibility, theme};

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Line {
    pub(super) path: PathBuf,
    pub(super) line: Option<u32>,
    pub(super) folder: bool,
    pub(super) mnemonic: Option<char>,
    pub(super) description: String,
    pub(super) condition: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Group {
    pub(super) name: String,
    pub(super) is_default: bool,
    pub(super) bookmarks: Vec<Line>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Listed {
    pub(super) groups: Vec<Group>,
    pub(super) group_by_file: bool,
    pub(super) replace_without_asking: bool,
    #[serde(skip)]
    pub(super) breakpoints: Vec<Line>,
}

impl Listed {
    pub(super) fn read(registry: &Registry, session: &mut Session) -> Self {
        let answer = registry.dispatch(session, "navigate.bookmark.list", json!({})).unwrap_or_default();
        let mut listed: Listed = serde_json::from_value(answer).unwrap_or_default();
        for bookmark in listed.groups.iter_mut().flat_map(|group| group.bookmarks.iter_mut()) {
            bookmark.line = bookmark.line.and_then(|line| line.checked_sub(1));
        }
        let breakpoints = registry.dispatch(session, "debug.breakpoint.list", json!({})).unwrap_or_default();
        listed.breakpoints = serde_json::from_value(breakpoints["breakpoints"].clone()).unwrap_or_default();
        listed
    }

    fn items_of(group: &Group) -> Vec<Item<'_>> {
        let mut items: Vec<Item<'_>> = Vec::new();
        for found in &group.bookmarks {
            let item = if found.line.is_some() { Item::File(&found.path) } else { Item::Whole(found) };
            if !items.contains(&item) {
                items.push(item);
            }
        }
        items
    }

    fn leaves(&self) -> Vec<RowKey> {
        let mut leaves = Vec::new();
        for group in &self.groups {
            if self.group_by_file {
                for item in Self::items_of(group) {
                    match item {
                        Item::Whole(found) => leaves.push(RowKey::Bookmark(found.path.clone(), None)),
                        Item::File(file) => {
                            let lines = group.bookmarks.iter().filter(|line| line.path == file && line.line.is_some());
                            leaves.extend(lines.map(|line| RowKey::Bookmark(line.path.clone(), line.line)));
                        }
                    }
                }
            } else {
                leaves.extend(group.bookmarks.iter().map(|line| RowKey::Bookmark(line.path.clone(), line.line)));
            }
        }
        let breakpoints = self.breakpoints.iter();
        leaves.extend(breakpoints.map(|line| RowKey::Breakpoint(line.path.clone(), line.line.unwrap_or_default())));
        leaves
    }

    fn bookmark(&self, path: &Path, line: Option<u32>) -> Option<(&Group, usize, &Line)> {
        self.groups.iter().find_map(|group| {
            let at = group.bookmarks.iter().position(|found| found.path == path && found.line == line)?;
            Some((group, at, &group.bookmarks[at]))
        })
    }

    fn breakpoint(&self, path: &Path, line: u32) -> Option<&Line> {
        self.breakpoints.iter().find(|found| found.path == path && found.line == Some(line))
    }

    fn whole(&self, path: &Path) -> Option<&Line> {
        self.bookmark(path, None).map(|(_, _, found)| found)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Item<'a> {
    Whole(&'a Line),
    File(&'a Path),
}

pub(super) fn place_params(path: &Path, line: Option<u32>) -> Value {
    match line {
        Some(line) => json!({ "path": path, "line": line + 1 }),
        None => json!({ "path": path, "whole": true }),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum RowKey {
    List(String),
    Breakpoints,
    File(String, PathBuf),
    Bookmark(PathBuf, Option<u32>),
    Breakpoint(PathBuf, u32),
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    key: RowKey,
    depth: usize,
    parent: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum BookmarksAction {
    Hide,
    FocusEditor,
    Layout(MenuRequest),
    Open { path: PathBuf, line: Option<u32>, focus_editor: bool },
    Command(&'static str, Value),
    CreateList,
    RenameList(String),
    DeleteList(String),
    RenameBookmark(PathBuf, Option<u32>),
    MoveToList(PathBuf, Option<u32>),
}

#[derive(Clone, Debug, PartialEq)]
struct Drop {
    list: String,
    index: usize,
    y: f32,
}

#[derive(Default)]
pub(super) struct BookmarksWindow {
    listed: Listed,
    project: Vec<PathBuf>,
    seen: Option<(u64, u64)>,
    collapsed: HashSet<RowKey>,
    selected: Option<RowKey>,
    reveal: bool,
    dragged: Option<(PathBuf, Option<u32>)>,
    drop: Option<Drop>,
    rows: Vec<(RowKey, Rect)>,
    tree: Option<egui::Id>,
}

pub(super) fn is_step(key: Key, modifiers: Modifiers) -> bool {
    matches!(key, Key::ArrowDown | Key::ArrowUp)
        && (modifiers.matches_exact(Modifiers::CTRL | Modifiers::ALT)
            || modifiers.matches_exact(Modifiers::ALT | Modifiers::COMMAND))
}

impl BookmarksWindow {
    pub(super) fn refresh(&mut self, registry: &Registry, session: &mut Session) {
        let key = (
            session.state::<Bookmarks>().map_or(0, Bookmarks::revision),
            session.state::<Breakpoints>().map_or(0, Breakpoints::revision),
        );
        if self.seen != Some(key) {
            self.listed = Listed::read(registry, session);
            let project = session.state::<zen_workbench::Workbench>().and_then(zen_workbench::Workbench::project);
            self.project = project
                .into_iter()
                .flat_map(|project| [project.to_owned(), project.canonicalize().unwrap_or_default()])
                .collect();
            self.seen = Some((
                session.state::<Bookmarks>().map_or(0, Bookmarks::revision),
                session.state::<Breakpoints>().map_or(0, Breakpoints::revision),
            ));
            if self.selected.as_ref().is_some_and(|selected| !self.exists(selected)) {
                self.selected = None;
            }
        }
    }

    pub(super) fn has_focus(&self, ctx: &egui::Context) -> bool {
        self.tree.is_some_and(|tree| ctx.memory(|memory| memory.has_focus(tree)))
    }

    fn select(&mut self, key: RowKey) {
        self.selected = Some(key);
        self.reveal = true;
    }

    fn exists(&self, key: &RowKey) -> bool {
        match key {
            RowKey::List(name) => self.listed.groups.iter().any(|group| &group.name == name),
            RowKey::Breakpoints => !self.listed.breakpoints.is_empty(),
            RowKey::File(list, path) => self.listed.groups.iter().any(|group| {
                &group.name == list && group.bookmarks.iter().any(|b| &b.path == path && b.line.is_some())
            }),
            RowKey::Bookmark(path, line) => self.listed.bookmark(path, *line).is_some(),
            RowKey::Breakpoint(path, line) => self.listed.breakpoint(path, *line).is_some(),
        }
    }

    fn visible(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for group in &self.listed.groups {
            let key = RowKey::List(group.name.clone());
            let open = !self.collapsed.contains(&key);
            rows.push(Row { key, depth: 0, parent: !group.bookmarks.is_empty() });
            if !open {
                continue;
            }
            if self.listed.group_by_file {
                for item in Listed::items_of(group) {
                    let file = match item {
                        Item::Whole(found) => {
                            rows.push(Row { key: RowKey::Bookmark(found.path.clone(), None), depth: 1, parent: false });
                            continue;
                        }
                        Item::File(file) => file,
                    };
                    let key = RowKey::File(group.name.clone(), file.to_owned());
                    let open = !self.collapsed.contains(&key);
                    rows.push(Row { key, depth: 1, parent: true });
                    if open {
                        for line in group.bookmarks.iter().filter(|line| line.path == file && line.line.is_some()) {
                            let key = RowKey::Bookmark(line.path.clone(), line.line);
                            rows.push(Row { key, depth: 2, parent: false });
                        }
                    }
                }
            } else {
                for line in &group.bookmarks {
                    rows.push(Row { key: RowKey::Bookmark(line.path.clone(), line.line), depth: 1, parent: false });
                }
            }
        }
        if !self.listed.breakpoints.is_empty() {
            let open = !self.collapsed.contains(&RowKey::Breakpoints);
            rows.push(Row { key: RowKey::Breakpoints, depth: 0, parent: true });
            if open {
                for line in &self.listed.breakpoints {
                    let key = RowKey::Breakpoint(line.path.clone(), line.line.unwrap_or_default());
                    rows.push(Row { key, depth: 1, parent: false });
                }
            }
        }
        rows
    }

    fn expand_to(&mut self, key: &RowKey) {
        match key {
            RowKey::Bookmark(path, line) => {
                if let Some((group, ..)) = self.listed.bookmark(path, *line) {
                    let (list, file) =
                        (RowKey::List(group.name.clone()), RowKey::File(group.name.clone(), path.clone()));
                    self.collapsed.remove(&list);
                    if line.is_some() {
                        self.collapsed.remove(&file);
                    }
                }
            }
            RowKey::Breakpoint(..) => {
                self.collapsed.remove(&RowKey::Breakpoints);
            }
            RowKey::File(list, _) => {
                self.collapsed.remove(&RowKey::List(list.clone()));
            }
            RowKey::List(_) | RowKey::Breakpoints => {}
        }
    }

    pub(super) fn step(&mut self, forward: bool) -> Option<BookmarksAction> {
        let leaves = self.listed.leaves();
        if leaves.is_empty() {
            return None;
        }
        let at = self.selected.as_ref().and_then(|selected| leaves.iter().position(|leaf| leaf == selected));
        let next = match (at, forward) {
            (Some(at), true) => (at + 1) % leaves.len(),
            (Some(at), false) => (at + leaves.len() - 1) % leaves.len(),
            (None, true) => 0,
            (None, false) => leaves.len() - 1,
        };
        let key = leaves[next].clone();
        self.expand_to(&key);
        self.select(key.clone());
        match open_action(&key) {
            Some(BookmarksAction::Open { path, line, .. }) => {
                Some(BookmarksAction::Open { path, line, focus_editor: false })
            }
            other => other,
        }
    }

    fn delete(&self, key: &RowKey) -> Option<BookmarksAction> {
        match key {
            RowKey::Bookmark(path, line) => {
                Some(BookmarksAction::Command("navigate.bookmark.remove", place_params(path, *line)))
            }
            RowKey::Breakpoint(path, line) => {
                Some(BookmarksAction::Command("debug.breakpoint.remove", json!({ "path": path, "line": line })))
            }
            RowKey::List(name) => Some(BookmarksAction::DeleteList(name.clone())),
            RowKey::Breakpoints | RowKey::File(..) => None,
        }
    }

    fn rename(key: &RowKey) -> Option<BookmarksAction> {
        match key {
            RowKey::List(name) => Some(BookmarksAction::RenameList(name.clone())),
            RowKey::Bookmark(path, line) => Some(BookmarksAction::RenameBookmark(path.clone(), *line)),
            _ => None,
        }
    }

    pub(super) fn delete_selected(&mut self) -> Option<BookmarksAction> {
        let selected = self.selected.clone()?;
        let action = self.delete(&selected)?;
        let rows = self.visible();
        if let Some(at) = rows.iter().position(|row| row.key == selected) {
            let after =
                rows[at + 1..].iter().find(|row| matches!(row.key, RowKey::Bookmark(..) | RowKey::Breakpoint(..)));
            let before = rows[..at].iter().rev().find(|row| row.depth == rows[at].depth);
            self.selected =
                after.or(before).map(|row| row.key.clone()).filter(|_| !matches!(selected, RowKey::List(_)));
        }
        Some(action)
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, layout: &ToolWindowLayout) -> Vec<BookmarksAction> {
        let mut actions = Vec::new();
        self.header(ui, p, layout, &mut actions);
        self.tree(ui, p, &mut actions);
        actions
    }

    fn header(&mut self, ui: &mut Ui, p: &IdePalette, layout: &ToolWindowLayout, actions: &mut Vec<BookmarksAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-tool-bookmarks"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("bookmarks-title"), title, || t!("ide-tool-bookmarks"));
        let button = |index: usize| {
            Rect::from_center_size(
                pos2(header.right() - 20.0 - 26.0 * index as f32, header.center().y),
                Vec2::splat(24.0),
            )
        };
        let hide = ui.interact(button(0), ui.scope_id().with("bookmarks-hide"), Sense::click());
        hover(ui, &hide, p);
        glyph_minus(ui.painter(), hide.rect.shrink(6.0), p.text_secondary);
        hint(ui, &hide, &t!("ide-structure-hide"), None, Side::Below);
        accessibility::button_with(&hide, || t!("ide-structure-hide"));
        if hide.clicked() {
            actions.push(BookmarksAction::Hide);
        }
        let more = ui.interact(button(1), ui.scope_id().with("bookmarks-more"), Sense::click());
        hover(ui, &more, p);
        glyph_more(ui.painter(), more.rect.shrink(6.0), p.text_secondary);
        if !egui::Popup::is_any_open(ui.ctx()) {
            hint(ui, &more, &t!("ide-structure-options"), None, Side::Below);
        }
        accessibility::button_with(&more, || t!("ide-structure-options"));
        let listed = &self.listed;
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(240.0);
            options_menu(ui, listed, actions);
            ui.separator();
            if let Some(request) = layout_menu(ui, layout, "bookmarks", false) {
                actions.push(BookmarksAction::Layout(request));
            }
        });
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<BookmarksAction>) {
        let id = ui.make_persistent_id("bookmarks-tree");
        self.tree = Some(id);
        let area = ui.interact(ui.available_rect_before_wrap(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("ide-tool-bookmarks"));
        if area.clicked() {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        area.context_menu(|ui| {
            customization::popup(
                ui,
                customization::BOOKMARKS_AREA_POPUP,
                |_| true,
                |ui, slot| {
                    if ui.button(t!(slot)).clicked() {
                        actions.push(BookmarksAction::CreateList);
                        ui.close();
                    }
                },
            );
        });
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
        let rows = self.visible();
        let mut clicked: Option<(RowKey, bool, bool)> = None;
        let mut drawn = Vec::with_capacity(rows.len());
        let mut drag_started = None;
        let mut released = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let height = metrics::TREE_ROW;
            let top = ui.cursor().top();
            let clip = ui.clip_rect();
            let first = (((clip.top() - top) / height).floor().max(0.0) as usize).min(rows.len());
            let last = ((((clip.bottom() - top) / height).ceil().max(0.0) as usize) + 1).clamp(first, rows.len());
            let selected_at =
                self.selected.as_ref().and_then(|selected| rows.iter().position(|row| &row.key == selected));
            if self.reveal
                && let Some(at) = selected_at.filter(|at| *at < first || *at >= last)
            {
                let rect = Rect::from_min_size(
                    pos2(ui.max_rect().left(), top + at as f32 * height),
                    vec2(ui.available_width(), height),
                );
                ui.scroll_to_rect(rect, None);
            }
            ui.add_space(first as f32 * height);
            for row in &rows[first..last] {
                let sense =
                    if matches!(row.key, RowKey::Bookmark(..)) { Sense::click_and_drag() } else { Sense::click() };
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), sense);
                drawn.push((row.key.clone(), rect));
                let label = self.label(&row.key);
                let expanded = row.parent.then(|| !self.collapsed.contains(&row.key));
                let selected = self.selected.as_ref() == Some(&row.key);
                accessibility::tree_item(&response, row.depth + 1, expanded, selected, &label.0);
                if !label.1.is_empty() {
                    accessibility::describe(&response, &label.1);
                }
                let highlight =
                    Rect::from_min_max(pos2(rect.left() + 12.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                if selected {
                    ui.painter().rect_filled(highlight, 5, if focused { p.selection } else { p.hover });
                    if std::mem::take(&mut self.reveal) {
                        response.scroll_to_me(None);
                    }
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 5, p.hover);
                }
                let x = highlight.left() + row.depth as f32 * metrics::TREE_INDENT;
                let chevron = Rect::from_center_size(pos2(x + metrics::TREE_ARROW, rect.center().y), Vec2::splat(11.0));
                if row.parent && self.collapsed.contains(&row.key) {
                    crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
                } else if row.parent {
                    crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
                }
                self.paint_row(ui, p, rect, x, &row.key, &label);
                if response.drag_started()
                    && let RowKey::Bookmark(path, line) = &row.key
                {
                    drag_started = Some((path.clone(), *line));
                }
                if response.drag_stopped() {
                    released = true;
                }
                if response.clicked() || response.double_clicked() {
                    let on_chevron =
                        row.parent && response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    clicked = Some((row.key.clone(), response.double_clicked(), on_chevron));
                }
                let key = row.key.clone();
                response.context_menu(|ui| {
                    customization::popup(
                        ui,
                        customization::BOOKMARKS_ROW_POPUP,
                        |_| true,
                        |ui, _| {
                            row_menu(ui, &self.listed, &key, actions);
                        },
                    );
                });
            }
            ui.add_space((rows.len() - last) as f32 * height);
        });
        self.rows = drawn;
        self.drag(ui, p, drag_started, released, actions);
        if let Some((key, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            if on_chevron {
                self.toggle(&key);
            } else if double {
                match open_action(&key) {
                    Some(open) => actions.push(open),
                    None => self.toggle(&key),
                }
            }
            self.selected = Some(key);
        }
        if self.listed.groups.iter().all(|group| group.bookmarks.is_empty()) && self.listed.breakpoints.is_empty() {
            let below = self.rows.last().map_or(ui.min_rect().top(), |(_, rect)| rect.bottom()) + 16.0;
            let width = ui.max_rect().width() - 32.0;
            let galley = ui.painter().layout(t!("ide-bookmarks-empty"), theme::regular(12.5), p.text_secondary, width);
            ui.painter().galley(pos2(ui.max_rect().left() + 16.0, below), galley, p.text_secondary);
        }
    }

    fn drag(
        &mut self,
        ui: &Ui,
        p: &IdePalette,
        started: Option<(PathBuf, Option<u32>)>,
        released: bool,
        actions: &mut Vec<BookmarksAction>,
    ) {
        if let Some(started) = started {
            self.dragged = Some(started);
        }
        let Some((path, line)) = self.dragged.clone() else { return };
        let pointer = ui.input(|input| input.pointer.interact_pos().or(input.pointer.hover_pos()));
        self.drop = pointer.and_then(|at| self.target(at, &path, line));
        if let Some(drop) = &self.drop {
            let left = self.rows.first().map_or(0.0, |(_, rect)| rect.left() + 12.0);
            let right = self.rows.first().map_or(0.0, |(_, rect)| rect.right() - 6.0);
            ui.painter().line_segment([pos2(left, drop.y), pos2(right, drop.y)], Stroke::new(2.0, p.accent));
        }
        if released || !ui.input(|input| input.pointer.any_down()) {
            if let Some(drop) = self.drop.take() {
                let mut params = place_params(&path, line);
                params["list"] = json!(drop.list);
                params["index"] = json!(drop.index);
                actions.push(BookmarksAction::Command("navigate.bookmark.moveToList", params));
            }
            self.dragged = None;
        }
    }

    fn target(&self, at: egui::Pos2, path: &Path, line: Option<u32>) -> Option<Drop> {
        let (key, rect) = self.rows.iter().find(|(_, rect)| rect.y_range().contains(at.y))?;
        let (from_list, from_at) = self.listed.bookmark(path, line).map(|(group, at, _)| (group.name.clone(), at))?;
        let (list, index, y) = match key {
            RowKey::List(name) => {
                let count = self.listed.groups.iter().find(|group| &group.name == name)?.bookmarks.len();
                (name.clone(), count, rect.bottom())
            }
            RowKey::File(name, file) => {
                let group = self.listed.groups.iter().find(|group| &group.name == name)?;
                let last = group.bookmarks.iter().rposition(|found| &found.path == file && found.line.is_some())?;
                (name.clone(), last + 1, rect.bottom())
            }
            RowKey::Bookmark(other, other_line) => {
                let (group, index, _) = self.listed.bookmark(other, *other_line)?;
                let below = at.y > rect.center().y;
                (group.name.clone(), index + usize::from(below), if below { rect.bottom() } else { rect.top() })
            }
            RowKey::Breakpoints | RowKey::Breakpoint(..) => return None,
        };
        let index = if list == from_list && from_at < index { index - 1 } else { index };
        if list == from_list && index == from_at {
            return None;
        }
        Some(Drop { list, index, y })
    }

    fn toggle(&mut self, key: &RowKey) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.clone());
        }
    }

    fn label(&self, key: &RowKey) -> (String, String) {
        let place = |path: &Path, line: u32| {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            format!("{name}:{}", line + 1)
        };
        let in_project = |path: &Path| {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let parent = path.parent().unwrap_or(Path::new(""));
            let folder = match self.project.iter().find_map(|project| parent.strip_prefix(project).ok()) {
                Some(inside) => inside.display().to_string(),
                None => parent.display().to_string(),
            };
            (name, folder)
        };
        match key {
            RowKey::List(name) => (name.clone(), String::new()),
            RowKey::Breakpoints => (t!("ide-bookmarks-breakpoints"), String::new()),
            RowKey::File(_, path) => in_project(path),
            RowKey::Bookmark(path, None) => {
                let Some(found) = self.listed.whole(path) else { return Default::default() };
                let (name, folder) = in_project(path);
                if found.description.is_empty() {
                    (name, folder)
                } else {
                    (found.description.clone(), format!("{name} {folder}").trim().to_owned())
                }
            }
            RowKey::Bookmark(path, Some(line)) => {
                let Some((_, _, found)) = self.listed.bookmark(path, Some(*line)) else { return Default::default() };
                let at = if self.listed.group_by_file {
                    t!("ide-bookmarks-line", line = line + 1)
                } else {
                    place(path, *line)
                };
                if found.description.is_empty() { (at, String::new()) } else { (found.description.clone(), at) }
            }
            RowKey::Breakpoint(path, line) => {
                let condition = self.listed.breakpoint(path, *line).and_then(|found| found.condition.clone());
                (place(path, *line), condition.unwrap_or_default())
            }
        }
    }

    fn paint_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, x: f32, key: &RowKey, label: &(String, String)) {
        let icon = Rect::from_center_size(pos2(x + metrics::TREE_ICON, rect.center().y), Vec2::splat(14.0));
        match key {
            RowKey::List(_) => glyph_bookmark(ui.painter(), icon, p.text_secondary),
            RowKey::Breakpoints | RowKey::Breakpoint(..) => {
                ui.painter().circle_filled(icon.center(), 5.0, p.breakpoint);
            }
            RowKey::File(_, path) => {
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                crate::file_icons::file_icon(ui.painter(), icon, &name);
            }
            RowKey::Bookmark(path, None) => {
                let found = self.listed.whole(path);
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                if found.is_some_and(|found| found.folder) {
                    crate::file_icons::folder_icon(ui.painter(), icon, &name, false, false);
                } else {
                    crate::file_icons::file_icon(ui.painter(), icon, &name);
                }
            }
            RowKey::Bookmark(path, line) => {
                let mnemonic = self.listed.bookmark(path, *line).and_then(|(_, _, found)| found.mnemonic);
                bookmark_icon(ui.painter(), icon.shrink(1.0), mnemonic, p);
            }
        }
        let painter = ui.painter().with_clip_rect(rect);
        let font = if matches!(key, RowKey::List(_) | RowKey::Breakpoints) {
            theme::semibold(13.0)
        } else {
            theme::regular(13.0)
        };
        let name =
            painter.text(pos2(x + metrics::TREE_TEXT, rect.center().y), Align2::LEFT_CENTER, &label.0, font, p.text);
        let mut after = name.right() + 6.0;
        if let RowKey::Bookmark(path, None) = key {
            let mnemonic = self.listed.whole(path).and_then(|found| found.mnemonic);
            let mark = Rect::from_center_size(pos2(after + 6.0, rect.center().y), Vec2::splat(13.0));
            bookmark_icon(&painter, mark, mnemonic, p);
            after = mark.right() + 6.0;
        }
        if let RowKey::List(list) = key
            && self.listed.groups.iter().any(|group| &group.name == list && group.is_default)
        {
            let galley = painter.layout_no_wrap(t!("ide-bookmarks-default"), theme::regular(11.0), p.text_secondary);
            let tag = Rect::from_min_size(
                pos2(after, rect.center().y - galley.size().y / 2.0 - 1.5),
                galley.size() + vec2(10.0, 3.0),
            );
            painter.rect_stroke(tag, 4, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
            painter.galley(tag.min + vec2(5.0, 1.5), galley, p.text_secondary);
            after = tag.right() + 6.0;
        }
        if !label.1.is_empty() {
            painter.text(
                pos2(after, rect.center().y),
                Align2::LEFT_CENTER,
                &label.1,
                theme::regular(13.0),
                p.text_secondary,
            );
        }
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<BookmarksAction>) {
        let none = Modifiers::NONE;
        let (mut next, mut previous) = (false, false);
        let (up, down, left, right, enter, escape, delete, f2) = ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Key { key, pressed: true, modifiers, .. } if is_step(*key, *modifiers) => {
                    if *key == Key::ArrowDown {
                        next = true;
                    } else {
                        previous = true;
                    }
                    false
                }
                _ => true,
            });
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Enter),
                input.consume_key(none, Key::Escape),
                input.consume_key(none, Key::Delete) | input.consume_key(none, Key::Backspace),
                input.consume_key(none, Key::F2),
            )
        });
        if next || previous {
            actions.extend(self.step(next));
            return;
        }
        let rows = self.visible();
        let at = self.selected.as_ref().and_then(|selected| rows.iter().position(|row| &row.key == selected));
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(rows.len().saturating_sub(1)),
            };
            self.selected = rows.get(next).map(|row| row.key.clone());
            self.reveal = true;
        }
        let row = at.and_then(|at| rows.get(at)).cloned();
        match row {
            Some(row) if left && row.parent && !self.collapsed.contains(&row.key) => {
                self.collapsed.insert(row.key);
            }
            Some(row) if left && row.depth > 0 => {
                let parent = rows[..at.unwrap_or(0)].iter().rev().find(|other| other.depth < row.depth);
                self.selected = parent.map(|parent| parent.key.clone());
                self.reveal = true;
            }
            Some(row) if right && row.parent && self.collapsed.contains(&row.key) => {
                self.collapsed.remove(&row.key);
            }
            Some(row) if enter => match open_action(&row.key) {
                Some(open) => actions.push(open),
                None => self.toggle(&row.key),
            },
            Some(row) if f2 => actions.extend(Self::rename(&row.key)),
            Some(_) if delete => actions.extend(self.delete_selected()),
            _ => {}
        }
        if escape {
            actions.push(BookmarksAction::FocusEditor);
        }
    }
}

fn open_action(key: &RowKey) -> Option<BookmarksAction> {
    match key {
        RowKey::Bookmark(path, line) => {
            Some(BookmarksAction::Open { path: path.clone(), line: *line, focus_editor: true })
        }
        RowKey::Breakpoint(path, line) => {
            Some(BookmarksAction::Open { path: path.clone(), line: Some(*line), focus_editor: true })
        }
        _ => None,
    }
}

fn options_menu(ui: &mut Ui, listed: &Listed, actions: &mut Vec<BookmarksAction>) {
    let mut group = listed.group_by_file;
    if ui.checkbox(&mut group, t!("ide-bookmarks-group-by-file")).changed() {
        actions.push(BookmarksAction::Command("navigate.bookmark.options", json!({ "groupByFile": group })));
    }
    let mut ask = !listed.replace_without_asking;
    if ui.checkbox(&mut ask, t!("ide-bookmarks-ask-mnemonic")).changed() {
        actions.push(BookmarksAction::Command("navigate.bookmark.options", json!({ "replaceWithoutAsking": !ask })));
    }
}

fn row_menu(ui: &mut Ui, listed: &Listed, key: &RowKey, actions: &mut Vec<BookmarksAction>) {
    let mut item = |ui: &mut Ui, enabled: bool, label: String, action: BookmarksAction| {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            actions.push(action);
            ui.close();
        }
    };
    match key {
        RowKey::List(name) => {
            let default = listed.groups.iter().any(|group| &group.name == name && group.is_default);
            item(ui, true, t!("command-navigate-bookmarkList-create"), BookmarksAction::CreateList);
            item(ui, true, t!("ide-bookmarks-rename-list"), BookmarksAction::RenameList(name.clone()));
            let set_default = BookmarksAction::Command("navigate.bookmarkList.setDefault", json!({ "name": name }));
            item(ui, !default, t!("command-navigate-bookmarkList-setDefault"), set_default);
            ui.separator();
            item(ui, true, t!("command-navigate-bookmarkList-delete"), BookmarksAction::DeleteList(name.clone()));
        }
        RowKey::Bookmark(path, line) => {
            let (group, at) = listed.bookmark(path, *line).map(|(group, at, _)| (group, at)).unzip();
            let count = group.map_or(0, |group| group.bookmarks.len());
            let place = place_params(path, *line);
            item(
                ui,
                true,
                t!("command-navigate-bookmark-open"),
                BookmarksAction::Open { path: path.clone(), line: *line, focus_editor: true },
            );
            item(ui, true, t!("keymap-view-bookmark-rename"), BookmarksAction::RenameBookmark(path.clone(), *line));
            item(ui, true, t!("ide-bookmark-another-list"), BookmarksAction::MoveToList(path.clone(), *line));
            ui.separator();
            let up = BookmarksAction::Command("navigate.bookmark.moveUp", place.clone());
            item(ui, at.is_some_and(|at| at > 0), t!("command-navigate-bookmark-moveUp"), up);
            let down = BookmarksAction::Command("navigate.bookmark.moveDown", place.clone());
            item(ui, at.is_some_and(|at| at + 1 < count), t!("command-navigate-bookmark-moveDown"), down);
            ui.separator();
            item(ui, true, t!("ide-bookmarks-delete"), BookmarksAction::Command("navigate.bookmark.remove", place));
        }
        RowKey::Breakpoint(path, line) => {
            item(
                ui,
                true,
                t!("command-navigate-bookmark-open"),
                BookmarksAction::Open { path: path.clone(), line: Some(*line), focus_editor: true },
            );
            let remove = BookmarksAction::Command("debug.breakpoint.remove", json!({ "path": path, "line": line }));
            item(ui, true, t!("ide-bookmarks-delete"), remove);
        }
        RowKey::Breakpoints | RowKey::File(..) => {
            item(ui, true, t!("command-navigate-bookmarkList-create"), BookmarksAction::CreateList);
        }
    }
}

impl IdeView {
    pub(super) fn bookmarks_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.bookmarks_window.refresh(registry, session);
        let palette = self.palette;
        let actions = self.bookmarks_window.show(ui, &palette, &self.tool_windows);
        for action in actions {
            self.bookmarks_action(ui, registry, session, action);
        }
    }

    pub(super) fn bookmarks_action(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        action: BookmarksAction,
    ) {
        match action {
            BookmarksAction::Hide => self.hide_tool("bookmarks"),
            BookmarksAction::FocusEditor => self.editor_view.focus(),
            BookmarksAction::Layout(request) => self.apply_menu_request(ui.ctx(), "bookmarks", request),
            BookmarksAction::Open { path, line, focus_editor } => {
                let opened = registry.dispatch(
                    session,
                    "navigate.bookmark.open",
                    json!({ "path": path, "line": line.map(|line| line + 1) }),
                );
                let folder = matches!(&opened, Err(zen_core::DispatchError::Failed { error, .. })
                    if error.code.as_deref() == Some(zen_workbench::bookmarks::FOLDER_BOOKMARK));
                if let Err(error) = opened {
                    self.report(ui, error);
                }
                if focus_editor && !folder {
                    self.editor_view.focus();
                }
            }
            BookmarksAction::Command(command, params) => self.dispatch(ui, registry, session, command, params),
            BookmarksAction::CreateList => self.bookmarks.open_list_name(registry, session, None),
            BookmarksAction::RenameList(name) => self.bookmarks.open_list_name(registry, session, Some(name)),
            BookmarksAction::DeleteList(name) => {
                let count = self
                    .bookmarks_window
                    .listed
                    .groups
                    .iter()
                    .find(|group| group.name == name)
                    .map_or(0, |group| group.bookmarks.len());
                self.bookmarks.open_delete_list(name, count);
            }
            BookmarksAction::RenameBookmark(path, line) => {
                self.bookmarks.open_rename(registry, session, path, line);
            }
            BookmarksAction::MoveToList(path, line) => {
                self.bookmarks.open_move_to_list(registry, session, path, line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::{PointerButton, Pos2};

    use super::*;

    struct Harness {
        ctx: egui::Context,
        window: BookmarksWindow,
        actions: Vec<BookmarksAction>,
        time: f64,
    }

    fn line(path: &str, line: u32, mnemonic: Option<char>, description: &str) -> Line {
        Line {
            path: PathBuf::from(path),
            line: Some(line),
            folder: false,
            mnemonic,
            description: description.to_owned(),
            condition: None,
        }
    }

    fn listed() -> Listed {
        Listed {
            groups: vec![
                Group {
                    name: "shop".to_owned(),
                    is_default: true,
                    bookmarks: vec![
                        line("/p/a.go", 3, Some('K'), "entry"),
                        line("/p/b.go", 1, None, ""),
                        line("/p/a.go", 9, None, ""),
                    ],
                },
                Group { name: "Later".to_owned(), is_default: false, bookmarks: vec![line("/p/c.go", 4, None, "")] },
            ],
            group_by_file: false,
            replace_without_asking: false,
            breakpoints: vec![Line { condition: Some("x > 1".to_owned()), ..line("/p/a.go", 20, None, "") }],
        }
    }

    fn bookmark(path: &str, line: u32) -> RowKey {
        RowKey::Bookmark(PathBuf::from(path), Some(line))
    }

    impl Harness {
        fn new(listed: Listed) -> Self {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut harness = Self { ctx, window: BookmarksWindow::default(), actions: Vec::new(), time: 0.0 };
            harness.window.listed = listed;
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.05;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(420.0, 600.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                self.actions.extend(self.window.show(ui, &IdePalette::zen_dark(), &ToolWindowLayout::default()));
            });
            output.textures_delta.clear();
        }

        fn button(pos: Pos2, pressed: bool) -> Event {
            Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
        }

        fn click(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![Self::button(at, true)]);
            self.frame(vec![Self::button(at, false)]);
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![Self::button(from, true)]);
            for step in 1..=4 {
                self.frame(vec![Event::PointerMoved(from + (to - from) * step as f32 / 4.0)]);
            }
            self.frame(vec![Self::button(to, false)]);
            self.frame(Vec::new());
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        }

        fn row(&self, key: &RowKey) -> Rect {
            self.window.rows.iter().find(|(drawn, _)| drawn == key).map(|(_, rect)| *rect).unwrap()
        }

        fn drawn(&self) -> Vec<RowKey> {
            self.window.rows.iter().map(|(key, _)| key.clone()).collect()
        }
    }

    #[test]
    fn the_lists_show_their_bookmarks_in_order_and_the_breakpoints_are_a_list_of_their_own() {
        let mut harness = Harness::new(listed());
        assert_eq!(
            harness.drawn(),
            [
                RowKey::List("shop".to_owned()),
                bookmark("/p/a.go", 3),
                bookmark("/p/b.go", 1),
                bookmark("/p/a.go", 9),
                RowKey::List("Later".to_owned()),
                bookmark("/p/c.go", 4),
                RowKey::Breakpoints,
                RowKey::Breakpoint(PathBuf::from("/p/a.go"), 20),
            ]
        );
        assert_eq!(harness.window.label(&bookmark("/p/a.go", 3)), ("entry".to_owned(), "a.go:4".to_owned()));
        assert_eq!(harness.window.label(&bookmark("/p/b.go", 1)), ("b.go:2".to_owned(), String::new()));
        let breakpoint = harness.window.label(&RowKey::Breakpoint(PathBuf::from("/p/a.go"), 20));
        assert_eq!(breakpoint, ("a.go:21".to_owned(), "x > 1".to_owned()));

        harness.window.listed.group_by_file = true;
        harness.frame(Vec::new());
        assert_eq!(
            harness.drawn()[..6],
            [
                RowKey::List("shop".to_owned()),
                RowKey::File("shop".to_owned(), PathBuf::from("/p/a.go")),
                bookmark("/p/a.go", 3),
                bookmark("/p/a.go", 9),
                RowKey::File("shop".to_owned(), PathBuf::from("/p/b.go")),
                bookmark("/p/b.go", 1),
            ],
            "Group Line Bookmarks by File puts them under their files"
        );
        assert_eq!(harness.window.label(&bookmark("/p/a.go", 9)).0, t!("ide-bookmarks-line", line = 10));

        let list = harness.row(&RowKey::List("shop".to_owned()));
        harness.click(pos2(list.left() + 12.0 + metrics::TREE_ARROW, list.center().y));
        harness.frame(Vec::new());
        assert_eq!(harness.drawn()[1], RowKey::List("Later".to_owned()), "the chevron collapses the list");
    }

    #[test]
    fn files_and_folders_are_rows_of_their_lists_beside_the_lines_and_open_or_go_as_theirs() {
        let whole = |path: &str, folder: bool, mnemonic: Option<char>, description: &str| Line {
            line: None,
            folder,
            ..line(path, 0, mnemonic, description)
        };
        let mut listed = listed();
        listed.groups[0].bookmarks.insert(1, whole("/p/pkg", true, Some('P'), ""));
        listed.groups[0].bookmarks.push(whole("/p/a.go", false, None, "entry file"));
        let mut harness = Harness::new(listed);
        let file = |path: &str| RowKey::Bookmark(PathBuf::from(path), None);
        assert_eq!(
            harness.drawn()[..6],
            [
                RowKey::List("shop".to_owned()),
                bookmark("/p/a.go", 3),
                file("/p/pkg"),
                bookmark("/p/b.go", 1),
                bookmark("/p/a.go", 9),
                file("/p/a.go"),
            ],
            "in their list's order, among the lines"
        );
        assert_eq!(harness.window.label(&file("/p/pkg")), ("pkg".to_owned(), "/p".to_owned()));
        assert_eq!(harness.window.label(&file("/p/a.go")), ("entry file".to_owned(), "a.go /p".to_owned()));

        harness.window.listed.group_by_file = true;
        harness.frame(Vec::new());
        assert_eq!(
            harness.drawn()[..7],
            [
                RowKey::List("shop".to_owned()),
                RowKey::File("shop".to_owned(), PathBuf::from("/p/a.go")),
                bookmark("/p/a.go", 3),
                bookmark("/p/a.go", 9),
                file("/p/pkg"),
                RowKey::File("shop".to_owned(), PathBuf::from("/p/b.go")),
                bookmark("/p/b.go", 1),
            ],
            "by file, a file's or folder's own bookmark is a row of the list, not under the file's lines"
        );
        assert!(harness.drawn().contains(&file("/p/a.go")));

        harness.click(harness.row(&file("/p/pkg")).center());
        harness.key(Key::Enter, Modifiers::NONE);
        harness.key(Key::F2, Modifiers::NONE);
        harness.key(Key::Delete, Modifiers::NONE);
        assert_eq!(
            harness.actions,
            [
                BookmarksAction::Open { path: PathBuf::from("/p/pkg"), line: None, focus_editor: true },
                BookmarksAction::RenameBookmark(PathBuf::from("/p/pkg"), None),
                BookmarksAction::Command("navigate.bookmark.remove", json!({ "path": "/p/pkg", "whole": true })),
            ]
        );
        harness.actions.clear();
        harness.window.listed.group_by_file = false;
        harness.frame(Vec::new());
        let row = harness.row(&file("/p/a.go")).center();
        harness.click(row);
        harness.click(row);
        assert_eq!(
            harness.actions,
            [BookmarksAction::Open { path: PathBuf::from("/p/a.go"), line: None, focus_editor: true }],
            "a double-click opens a file's bookmark"
        );
        harness.actions.clear();
        let from = harness.row(&file("/p/a.go")).center();
        let onto = harness.row(&RowKey::List("Later".to_owned())).center();
        harness.drag(from, onto);
        assert_eq!(
            harness.actions,
            [BookmarksAction::Command(
                "navigate.bookmark.moveToList",
                json!({ "path": "/p/a.go", "whole": true, "list": "Later", "index": 1 })
            )],
            "a file's bookmark drags to another list too"
        );
        assert_eq!(place_params(Path::new("/p/a.go"), Some(2)), json!({ "path": "/p/a.go", "line": 3 }));
    }

    #[test]
    fn enter_and_a_double_click_open_and_ctrl_alt_arrows_go_round_every_bookmark() {
        let mut harness = Harness::new(listed());
        harness.click(harness.row(&bookmark("/p/a.go", 3)).center());
        assert!(harness.actions.is_empty(), "a click selects");
        harness.key(Key::ArrowDown, Modifiers::NONE);
        harness.key(Key::Enter, Modifiers::NONE);
        assert_eq!(
            harness.actions,
            [BookmarksAction::Open { path: PathBuf::from("/p/b.go"), line: Some(1), focus_editor: true }]
        );
        harness.actions.clear();
        let jump = Modifiers::CTRL | Modifiers::ALT;
        harness.key(Key::ArrowDown, jump);
        assert_eq!(
            harness.actions,
            [BookmarksAction::Open { path: PathBuf::from("/p/a.go"), line: Some(9), focus_editor: false }]
        );
        harness.key(Key::ArrowDown, jump);
        harness.key(Key::ArrowDown, jump);
        assert_eq!(
            harness.actions.last(),
            Some(&BookmarksAction::Open { path: PathBuf::from("/p/a.go"), line: Some(20), focus_editor: false })
        );
        harness.key(Key::ArrowDown, jump);
        assert_eq!(
            harness.actions.last(),
            Some(&BookmarksAction::Open { path: PathBuf::from("/p/a.go"), line: Some(3), focus_editor: false }),
            "round to the first"
        );
        harness.key(Key::ArrowUp, jump);
        assert_eq!(
            harness.actions.last(),
            Some(&BookmarksAction::Open { path: PathBuf::from("/p/a.go"), line: Some(20), focus_editor: false })
        );
        assert_eq!(harness.window.selected, Some(RowKey::Breakpoint(PathBuf::from("/p/a.go"), 20)));

        harness.actions.clear();
        let row = harness.row(&bookmark("/p/c.go", 4)).center();
        harness.click(row);
        harness.click(row);
        assert_eq!(
            harness.actions,
            [BookmarksAction::Open { path: PathBuf::from("/p/c.go"), line: Some(4), focus_editor: true }]
        );
        harness.frame(Vec::new());
        harness.key(Key::Escape, Modifiers::NONE);
        assert_eq!(harness.actions.last(), Some(&BookmarksAction::FocusEditor));
    }

    #[test]
    fn the_step_keys_are_ctrl_alt_and_alt_cmd_arrows_only() {
        for modifiers in [Modifiers::CTRL | Modifiers::ALT, Modifiers::ALT | Modifiers::MAC_CMD | Modifiers::COMMAND] {
            assert!(is_step(Key::ArrowDown, modifiers) && is_step(Key::ArrowUp, modifiers));
        }
        assert!(!is_step(Key::ArrowDown, Modifiers::ALT));
        assert!(!is_step(Key::ArrowDown, Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT));
        assert!(!is_step(Key::ArrowLeft, Modifiers::CTRL | Modifiers::ALT));
        let mut harness = Harness::new(listed());
        assert!(
            !harness.window.has_focus(&harness.ctx),
            "the window's shortcuts keep the keys until the tree has the keyboard"
        );
        harness.click(harness.row(&bookmark("/p/a.go", 3)).center());
        assert!(harness.window.has_focus(&harness.ctx));
    }

    #[test]
    fn a_thousand_bookmarks_draw_only_the_rows_in_view() {
        let lines: Vec<Line> = (0..1000).map(|n| line(&format!("/p/f{}.go", n % 40), n, None, "")).collect();
        let listed = Listed {
            groups: vec![
                Group { name: "shop".to_owned(), is_default: true, bookmarks: lines[..600].to_vec() },
                Group { name: "Later".to_owned(), is_default: false, bookmarks: lines[600..].to_vec() },
            ],
            ..Listed::default()
        };
        let mut harness = Harness::new(listed);
        harness.frame(Vec::new());
        let in_view = (600.0 / metrics::TREE_ROW) as usize + 2;
        assert!(harness.window.rows.len() <= in_view, "{} rows drawn", harness.window.rows.len());
        let started = std::time::Instant::now();
        for _ in 0..20 {
            harness.frame(Vec::new());
        }
        let frame = started.elapsed() / 20;
        assert!(frame < std::time::Duration::from_millis(50), "a frame of 1000 bookmarks took {frame:?}");
        harness.click(harness.row(&bookmark("/p/f1.go", 1)).center());
        harness.key(Key::ArrowUp, Modifiers::CTRL | Modifiers::ALT);
        for _ in 0..3 {
            harness.frame(Vec::new());
        }
        assert_eq!(harness.window.selected, Some(bookmark("/p/f0.go", 0)));
        harness.key(Key::ArrowUp, Modifiers::CTRL | Modifiers::ALT);
        for _ in 0..12 {
            harness.frame(Vec::new());
        }
        assert_eq!(harness.window.selected, Some(bookmark("/p/f39.go", 999)), "round to the last one");
        assert!(harness.window.rows.iter().any(|(key, _)| key == &bookmark("/p/f39.go", 999)), "scrolled to it");
        assert!(harness.window.rows.len() <= in_view + 1);
        harness.window.listed.group_by_file = true;
        harness.frame(Vec::new());
        assert!(harness.window.rows.len() <= in_view + 1, "by file too");
    }

    #[test]
    fn delete_removes_the_selected_bookmark_breakpoint_or_list_and_f2_renames() {
        let mut harness = Harness::new(listed());
        harness.click(harness.row(&bookmark("/p/b.go", 1)).center());
        harness.key(Key::Delete, Modifiers::NONE);
        assert_eq!(
            harness.actions,
            [BookmarksAction::Command("navigate.bookmark.remove", json!({ "path": "/p/b.go", "line": 2 }))]
        );
        assert_eq!(harness.window.selected, Some(bookmark("/p/a.go", 9)), "the next one is selected");
        harness.actions.clear();
        harness.click(harness.row(&RowKey::Breakpoint(PathBuf::from("/p/a.go"), 20)).center());
        harness.key(Key::Backspace, Modifiers::NONE);
        assert_eq!(
            harness.actions,
            [BookmarksAction::Command("debug.breakpoint.remove", json!({ "path": "/p/a.go", "line": 20 }))]
        );
        harness.actions.clear();
        harness.click(harness.row(&RowKey::List("Later".to_owned())).center());
        harness.key(Key::F2, Modifiers::NONE);
        harness.key(Key::Delete, Modifiers::NONE);
        assert_eq!(
            harness.actions,
            [BookmarksAction::RenameList("Later".to_owned()), BookmarksAction::DeleteList("Later".to_owned())]
        );
        harness.actions.clear();
        harness.click(harness.row(&bookmark("/p/a.go", 3)).center());
        harness.key(Key::F2, Modifiers::NONE);
        assert_eq!(harness.actions, [BookmarksAction::RenameBookmark(PathBuf::from("/p/a.go"), Some(3))]);
    }

    #[test]
    fn a_bookmark_dragged_onto_another_list_or_bookmark_moves_there() {
        let mut harness = Harness::new(listed());
        let from = harness.row(&bookmark("/p/c.go", 4)).center();
        let onto = harness.row(&bookmark("/p/b.go", 1));
        harness.drag(from, pos2(onto.center().x, onto.top() + 3.0));
        assert_eq!(
            harness.actions,
            [BookmarksAction::Command(
                "navigate.bookmark.moveToList",
                json!({ "path": "/p/c.go", "line": 5, "list": "shop", "index": 1 })
            )],
            "dropped on the upper half of a bookmark, it goes before it"
        );
        harness.actions.clear();
        let from = harness.row(&bookmark("/p/a.go", 3)).center();
        let onto = harness.row(&bookmark("/p/a.go", 9));
        harness.drag(from, pos2(onto.center().x, onto.bottom() - 3.0));
        assert_eq!(
            harness.actions,
            [BookmarksAction::Command(
                "navigate.bookmark.moveToList",
                json!({ "path": "/p/a.go", "line": 4, "list": "shop", "index": 2 })
            )],
            "moved down its own list, after the last one"
        );
        harness.actions.clear();
        let from = harness.row(&bookmark("/p/a.go", 9)).center();
        let onto = harness.row(&RowKey::List("Later".to_owned())).center();
        harness.drag(from, onto);
        assert_eq!(
            harness.actions,
            [BookmarksAction::Command(
                "navigate.bookmark.moveToList",
                json!({ "path": "/p/a.go", "line": 10, "list": "Later", "index": 1 })
            )],
            "dropped on a list, it goes last in it"
        );
        harness.actions.clear();
        let from = harness.row(&bookmark("/p/b.go", 1)).center();
        harness.drag(from, from + vec2(0.0, 2.0));
        assert!(harness.actions.is_empty(), "dropped where it was, nothing moves");
        assert!(harness.window.dragged.is_none());
    }

    #[test]
    fn the_options_menu_has_its_two_options_and_they_are_commands() {
        let mut harness = Harness::new(listed());
        let header = harness.row(&RowKey::List("shop".to_owned())).top() - metrics::TOOL_HEADER / 2.0;
        let right = harness.row(&RowKey::List("shop".to_owned())).right();
        harness.click(pos2(right - 20.0 - 26.0, header));
        let tree = accessibility::testing::tree_of(&harness.ctx, vec2(420.0, 600.0), |ui| {
            let _ = harness.window.show(ui, &IdePalette::zen_dark(), &ToolWindowLayout::default());
        });
        for option in ["ide-bookmarks-group-by-file", "ide-bookmarks-ask-mnemonic"] {
            assert!(
                accessibility::testing::find(&tree, egui::Role::CheckBox, &t!(option)).is_some(),
                "{option} in the ⋮ menu"
            );
        }
    }

    #[test]
    fn the_tree_is_named_for_screen_readers() {
        let mut harness = Harness::new(listed());
        let tree = accessibility::testing::tree_of(&harness.ctx, vec2(420.0, 600.0), |ui| {
            let _ = harness.window.show(ui, &IdePalette::zen_dark(), &ToolWindowLayout::default());
        });
        assert_eq!(accessibility::unnamed(&tree), Vec::<String>::new());
        let entry = accessibility::testing::find(&tree, egui::Role::TreeItem, "entry").unwrap();
        assert_eq!((entry.level(), entry.description()), (Some(2), Some("a.go:4")));
        let list = accessibility::testing::find(&tree, egui::Role::TreeItem, "shop").unwrap();
        assert_eq!((list.level(), list.is_expanded()), (Some(1), Some(true)));
        assert!(accessibility::testing::find(&tree, egui::Role::Heading, &t!("ide-tool-bookmarks")).is_some());
    }

    #[test]
    fn the_lists_and_breakpoints_are_read_from_their_commands() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.go");
        std::fs::write(&main, "package main\n\nfunc main() {}\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let run = |session: &mut Session, command: &str, params: Value| {
            registry.dispatch(session, command, params).unwrap();
        };
        run(&mut session, "file.project.open", json!({ "path": dir.path() }));
        run(&mut session, "navigate.bookmark.setMnemonic", json!({ "path": main, "line": 3, "mnemonic": "k" }));
        run(&mut session, "navigate.bookmarkList.create", json!({ "name": "Later" }));
        run(&mut session, "debug.breakpoint.toggle", json!({ "path": main, "line": 2 }));
        let mut window = BookmarksWindow::default();
        window.refresh(&registry, &mut session);
        let project = dir.path().file_name().unwrap().to_string_lossy().into_owned();
        let names: Vec<(&str, bool)> =
            window.listed.groups.iter().map(|group| (group.name.as_str(), group.is_default)).collect();
        assert_eq!(names, [(project.as_str(), true), ("Later", false)]);
        assert_eq!(window.listed.groups[0].bookmarks, [line(main.to_str().unwrap(), 2, Some('K'), "")]);
        assert_eq!(window.listed.breakpoints.len(), 1);
        window.selected = Some(RowKey::List("Later".to_owned()));
        registry.dispatch(&mut session, "navigate.bookmarkList.delete", json!({ "name": "Later" })).unwrap();
        window.refresh(&registry, &mut session);
        assert_eq!(window.listed.groups.len(), 1, "read again after a change");
        assert_eq!(window.selected, None, "a row that is gone is not selected");
    }
}
