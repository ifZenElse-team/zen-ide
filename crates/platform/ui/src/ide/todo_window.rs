use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, FontId, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::todo::{TodoList, TodoState, TodoTab};

use super::glyphs::{
    Glyph, glyph_arrow_down, glyph_arrow_up, glyph_collapse_all, glyph_expand_all, glyph_filter, glyph_minus,
    glyph_more, glyph_pointer, glyph_split_right,
};
use super::tool_stripes::{MenuRequest, layout_menu};
use super::tool_windows::ToolWindowLayout;
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const TOOLBAR: f32 = 34.0;
const DEBOUNCE: f64 = 0.3;
const PREVIEW_LINES: usize = 61;
const PROJECT_FILES: &str = "projectFiles";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct TodoOptions {
    pub(super) tab: TodoTab,
    pub(super) scope: String,
    pub(super) preview_source: bool,
    pub(super) navigate_with_single_click: bool,
    pub(super) filter: Option<String>,
}

impl Default for TodoOptions {
    fn default() -> Self {
        TodoOptions {
            tab: TodoTab::Project,
            scope: PROJECT_FILES.to_owned(),
            preview_source: false,
            navigate_with_single_click: false,
            filter: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    File(usize),
    Item(usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Selected {
    File(PathBuf),
    Item(PathBuf, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TodoAction {
    Hide,
    GoTo { path: PathBuf, offset: usize, focus_editor: bool },
    FocusEditor,
    Layout(MenuRequest),
    EditFilters,
}

impl TodoAction {
    pub(super) fn command(&self) -> Option<(&'static str, Value)> {
        match self {
            Self::GoTo { path, offset, .. } => Some(("navigate.place.goTo", json!({ "path": path, "offset": offset }))),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct Preview {
    first: usize,
    lines: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct ScopeEntry {
    id: String,
    name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct ScopeList {
    scopes: Vec<ScopeEntry>,
}

#[derive(Debug, Default)]
pub(super) struct TodoWindow {
    pub(super) options: TodoOptions,
    list: TodoList,
    git: bool,
    scopes: Vec<(String, String)>,
    collapsed: BTreeSet<PathBuf>,
    selected: Option<Selected>,
    reveal: bool,
    rows: Vec<(Row, Rect)>,
    stamp: Option<u64>,
    changed: Option<(u64, f64)>,
    scan: Option<u64>,
    disk: u64,
    preview: Option<((PathBuf, usize, u64), Preview)>,
}

impl TodoWindow {
    pub(super) fn disk_changed(&mut self) {
        self.disk = self.disk.wrapping_add(1);
    }

    fn tabs(&self) -> Vec<TodoTab> {
        TodoTab::ALL.into_iter().filter(|tab| self.git || *tab != TodoTab::CurrentChangelist).collect()
    }

    fn params(&self) -> Value {
        let mut params = match self.options.tab {
            TodoTab::ScopeBased => json!({ "tab": self.options.tab, "scope": self.options.scope }),
            tab => json!({ "tab": tab }),
        };
        if let Some(filter) = &self.options.filter {
            params["filter"] = json!(filter);
        }
        params
    }

    fn stamp_of(&self, session: &Session, git_revision: u64) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (self.options.tab, &self.options.scope, &self.options.filter, self.disk, self.git).hash(&mut hasher);
        session
            .settings()
            .get::<Value>(zen_workbench::todo_patterns::SETTINGS)
            .map(|value| value.to_string())
            .hash(&mut hasher);
        if let Some(workbench) = session.state::<Workbench>() {
            workbench.revision().hash(&mut hasher);
            for document in workbench.documents() {
                (document.path(), document.version()).hash(&mut hasher);
            }
            if self.options.tab == TodoTab::CurrentFile {
                workbench.active_document().map(|document| document.path().to_owned()).hash(&mut hasher);
            }
        }
        if self.options.tab == TodoTab::CurrentChangelist {
            git_revision.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub(super) fn select_tab(&mut self, tab: TodoTab, scope: Option<String>) -> Result<(), String> {
        if tab == TodoTab::CurrentChangelist && !self.git {
            return Err("Current Changelist is only there for a project in Git".to_owned());
        }
        self.options.tab = tab;
        if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
            self.options.scope = scope;
        }
        Ok(())
    }

    pub(super) fn follow(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        now: f64,
        git: (bool, u64),
    ) -> Option<Duration> {
        self.git = git.0;
        if !self.git && self.options.tab == TodoTab::CurrentChangelist {
            self.options.tab = TodoTab::Project;
        }
        let stamp = self.stamp_of(session, git.1);
        let mut again = None;
        if self.stamp != Some(stamp) {
            let since = match self.changed {
                Some((seen, at)) if seen == stamp => at,
                _ => {
                    self.changed = Some((stamp, now));
                    now
                }
            };
            let first = self.stamp.is_none();
            if first || now - since >= DEBOUNCE {
                if let Ok(started) = registry.dispatch(session, "todo.scan.start", self.params()) {
                    self.scan = started["scan"].as_u64();
                }
                self.stamp = Some(stamp);
                self.changed = None;
            } else {
                again = Some(Duration::from_secs_f64(DEBOUNCE - (now - since)));
            }
        }
        let finished =
            |scan: u64| session.state::<TodoState>().is_none_or(|state| state.scan_id() != scan || state.is_done());
        if self.scan.is_some_and(|scan| !finished(scan)) {
            return Some(Duration::from_millis(50));
        }
        if let Some(scan) = self.scan {
            let results = registry
                .dispatch(session, "todo.scan.results", json!({}))
                .ok()
                .and_then(|value| serde_json::from_value::<TodoList>(value).ok());
            match results {
                Some(list) if list.scan == scan && list.done => {
                    self.update(list);
                    self.scan = None;
                }
                Some(list) if list.scan == scan => again = Some(Duration::from_millis(50)),
                _ => self.scan = None,
            }
        }
        again
    }

    fn update(&mut self, list: TodoList) {
        self.list = list;
        let still = match &self.selected {
            Some(Selected::Item(path, line)) => self
                .list
                .files
                .iter()
                .any(|file| file.path == *path && file.items.iter().any(|item| item.line == *line)),
            Some(Selected::File(path)) => self.list.files.iter().any(|file| file.path == *path),
            None => false,
        };
        if !still {
            self.selected = self
                .list
                .files
                .first()
                .and_then(|file| file.items.first().map(|item| Selected::Item(file.path.clone(), item.line)));
        }
    }

    fn row_of(&self, selected: &Selected) -> Option<Row> {
        match selected {
            Selected::File(path) => self.list.files.iter().position(|file| file.path == *path).map(Row::File),
            Selected::Item(path, line) => {
                let file = self.list.files.iter().position(|file| file.path == *path)?;
                let item = self.list.files[file].items.iter().position(|item| item.line == *line)?;
                Some(Row::Item(file, item))
            }
        }
    }

    fn selected_row(&self) -> Option<Row> {
        self.selected.as_ref().and_then(|selected| self.row_of(selected))
    }

    fn select(&mut self, row: Row) {
        self.selected = match row {
            Row::File(file) => self.list.files.get(file).map(|file| Selected::File(file.path.clone())),
            Row::Item(file, item) => self
                .list
                .files
                .get(file)
                .and_then(|file| file.items.get(item).map(|item| Selected::Item(file.path.clone(), item.line))),
        };
    }

    fn visible(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (index, file) in self.list.files.iter().enumerate() {
            rows.push(Row::File(index));
            if !self.collapsed.contains(&file.path) {
                rows.extend((0..file.items.len()).map(|item| Row::Item(index, item)));
            }
        }
        rows
    }

    fn go_to(&self, row: Row, focus_editor: bool) -> Option<TodoAction> {
        let Row::Item(file, item) = row else { return None };
        let file = self.list.files.get(file)?;
        let item = file.items.get(item)?;
        Some(TodoAction::GoTo { path: file.path.clone(), offset: item.offset, focus_editor })
    }

    fn toggle(&mut self, file: usize) {
        let Some(path) = self.list.files.get(file).map(|file| file.path.clone()) else { return };
        if !self.collapsed.remove(&path) {
            self.collapsed.insert(path);
        }
    }

    pub(super) fn expand_all(&mut self) {
        self.collapsed.clear();
    }

    pub(super) fn collapse_all(&mut self) {
        self.collapsed = self.list.files.iter().map(|file| file.path.clone()).collect();
        if let Some(Selected::Item(path, _)) = &self.selected {
            self.selected = Some(Selected::File(path.clone()));
        }
    }

    pub(super) fn occurrence(&mut self, forward: bool) -> Option<TodoAction> {
        let items: Vec<Row> = self
            .list
            .files
            .iter()
            .enumerate()
            .flat_map(|(file, entry)| (0..entry.items.len()).map(move |item| Row::Item(file, item)))
            .collect();
        let next = match self.selected_row() {
            None => {
                if forward {
                    items.first().copied()
                } else {
                    items.last().copied()
                }
            }
            Some(Row::Item(file, item)) => {
                let at = items.iter().position(|row| *row == Row::Item(file, item))?;
                if forward {
                    items.get(at + 1).copied()
                } else {
                    at.checked_sub(1).and_then(|at| items.get(at).copied())
                }
            }
            Some(Row::File(file)) => {
                if forward {
                    items.iter().find(|row| matches!(row, Row::Item(at, _) if *at >= file)).copied()
                } else {
                    items.iter().rev().find(|row| matches!(row, Row::Item(at, _) if *at < file)).copied()
                }
            }
        }?;
        if let Row::Item(file, _) = next
            && let Some(path) = self.list.files.get(file).map(|file| file.path.clone())
        {
            self.collapsed.remove(&path);
        }
        self.select(next);
        self.reveal = true;
        self.go_to(next, false)
    }

    pub(super) fn state(&self) -> Value {
        let selected = match self.selected_row() {
            Some(Row::Item(file, item)) => {
                let (file, item) = (&self.list.files[file], &self.list.files[file].items[item]);
                json!({ "path": file.path, "line": item.line, "text": item.text })
            }
            Some(Row::File(file)) => json!({ "path": self.list.files[file].path }),
            None => Value::Null,
        };
        json!({
            "tab": self.options.tab,
            "tabs": self.tabs(),
            "scope": (self.options.tab == TodoTab::ScopeBased).then_some(&self.options.scope),
            "items": self.list.total,
            "files": self.list.files.len(),
            "scanning": self.scan.is_some(),
            "selected": selected,
            "collapsed": self.collapsed,
            "filter": self.options.filter,
            "previewSource": self.options.preview_source,
            "navigateWithSingleClick": self.options.navigate_with_single_click,
        })
    }

    fn summary(&self) -> String {
        t!(
            "ide-todo-found",
            items = t!("ide-todo-items", count = self.list.total),
            files = t!("ide-todo-files", count = self.list.files.len())
        )
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        layout: &ToolWindowLayout,
        filters: &[String],
    ) -> Vec<TodoAction> {
        let mut actions = Vec::new();
        self.header(ui, p, layout, &mut actions);
        if self.options.tab == TodoTab::ScopeBased {
            self.scope_row(ui, p);
        }
        let body = ui.available_rect_before_wrap();
        let toolbar = Rect::from_min_size(body.min, vec2(TOOLBAR, body.height()));
        self.toolbar(ui, p, toolbar, &mut actions);
        self.filter_button(ui, p, toolbar, filters, &mut actions);
        let rest = Rect::from_min_max(pos2(toolbar.right(), body.top()), body.max);
        let (tree, preview) = if self.options.preview_source {
            let split = rest.left() + rest.width() * 0.5;
            (
                Rect::from_min_max(rest.min, pos2(split, rest.bottom())),
                Some(Rect::from_min_max(pos2(split + 1.0, rest.top()), rest.max)),
            )
        } else {
            (rest, None)
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tree).id_salt("todo-tree"));
        self.tree(&mut child, p, &mut actions);
        if let Some(preview) = preview {
            ui.painter().vline(preview.left() - 0.5, preview.y_range(), egui::Stroke::new(1.0, p.tab_border));
            self.draw_preview(ui, p, preview);
        }
        ui.allocate_rect(body, Sense::hover());
        actions
    }

    fn header(&mut self, ui: &mut Ui, p: &IdePalette, layout: &ToolWindowLayout, actions: &mut Vec<TodoAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-todo-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("todo-title"), title, || t!("ide-todo-title"));
        let mut x = title.right() + 14.0;
        for tab in self.tabs() {
            let galley = ui.painter().layout_no_wrap(tab_title(tab), theme::regular(13.0), p.text);
            let rect = Rect::from_min_size(pos2(x, header.center().y - 13.0), vec2(galley.size().x + 20.0, 26.0));
            let response = ui.interact(rect, ui.scope_id().with(("todo-tab", tab.id())), Sense::click());
            let active = self.options.tab == tab;
            accessibility::item(&response, egui::Role::Tab, active, galley.text());
            if active {
                ui.painter().rect_filled(rect, 6, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 6, p.hover);
            }
            ui.painter().galley(rect.center() - galley.size() / 2.0, galley, p.text);
            if response.clicked() {
                self.options.tab = tab;
            }
            x = rect.right() + 6.0;
        }
        let button = |index: usize| {
            Rect::from_center_size(
                pos2(header.right() - 20.0 - 26.0 * index as f32, header.center().y),
                Vec2::splat(24.0),
            )
        };
        let hide = ui.interact(button(0), ui.scope_id().with("todo-hide"), Sense::click());
        hover(ui, &hide, p);
        glyph_minus(ui.painter(), hide.rect.shrink(6.0), p.text_secondary);
        hint(ui, &hide, &t!("a11y-hide"), None, Side::Below);
        if hide.clicked() {
            actions.push(TodoAction::Hide);
        }
        let more = ui.interact(button(1), ui.scope_id().with("todo-more"), Sense::click());
        hover(ui, &more, p);
        glyph_more(ui.painter(), more.rect.shrink(6.0), p.text_secondary);
        hint(ui, &more, &t!("ide-todo-options"), None, Side::Below);
        let options = &mut self.options;
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(220.0);
            ui.checkbox(&mut options.navigate_with_single_click, t!("ide-todo-single-click"));
            ui.checkbox(&mut options.preview_source, t!("ide-todo-preview-source"));
            ui.separator();
            if let Some(request) = layout_menu(ui, layout, "todo", false) {
                actions.push(TodoAction::Layout(request));
            }
        });
    }

    fn scope_row(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(egui::RichText::new(t!("ide-todo-scope")).color(p.text_secondary));
            let current = self
                .scopes
                .iter()
                .find(|(id, _)| *id == self.options.scope)
                .map_or_else(|| self.options.scope.clone(), |(_, name)| name.clone());
            egui::ComboBox::from_id_salt("todo-scope").selected_text(current).width(240.0).show_ui(ui, |ui| {
                for (id, name) in &self.scopes {
                    ui.selectable_value(&mut self.options.scope, id.clone(), name);
                }
            });
        });
        ui.add_space(4.0);
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect, actions: &mut Vec<TodoAction>) {
        let buttons: [(Glyph, String, Option<String>, Option<bool>); 6] = [
            (
                glyph_arrow_up,
                super::keymap_page::action_title("view.occurrence.previous"),
                crate::keymap::shortcut("view.occurrence.previous"),
                None,
            ),
            (
                glyph_arrow_down,
                super::keymap_page::action_title("view.occurrence.next"),
                crate::keymap::shortcut("view.occurrence.next"),
                None,
            ),
            (glyph_pointer, t!("ide-todo-single-click"), None, Some(self.options.navigate_with_single_click)),
            (glyph_expand_all, t!("ide-todo-expand-all"), Some("⌘+".to_owned()), None),
            (glyph_collapse_all, t!("ide-todo-collapse-all"), Some("⌘−".to_owned()), None),
            (glyph_split_right, t!("ide-todo-preview-source"), None, Some(self.options.preview_source)),
        ];
        for (index, (glyph, name, shortcut, on)) in buttons.into_iter().enumerate() {
            let gap = 8.0 * [2, 3, 5].iter().filter(|start| index >= **start).count() as f32;
            let center = pos2(rect.center().x, rect.top() + 16.0 + 28.0 * index as f32 + gap);
            let response = ui.interact(
                Rect::from_center_size(center, Vec2::splat(24.0)),
                ui.scope_id().with(("todo-tool", index)),
                Sense::click(),
            );
            if on == Some(true) {
                ui.painter().rect_filled(response.rect, 6, p.popup_hover);
            } else {
                hover(ui, &response, p);
            }
            glyph(ui.painter(), response.rect.shrink(6.0), if on == Some(true) { p.text } else { p.text_secondary });
            hint(ui, &response, &name, shortcut.as_deref(), Side::Right);
            if let Some(on) = on {
                accessibility::pressed(&response, on);
            }
            if response.clicked() {
                match index {
                    0 => actions.extend(self.occurrence(false)),
                    1 => actions.extend(self.occurrence(true)),
                    2 => self.options.navigate_with_single_click = !self.options.navigate_with_single_click,
                    3 => self.expand_all(),
                    4 => self.collapse_all(),
                    _ => self.options.preview_source = !self.options.preview_source,
                }
            }
        }
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<TodoAction>) {
        if self.list.files.is_empty() {
            let text = if self.scan.is_some() && self.list.scan == 0 {
                t!("ide-todo-searching")
            } else {
                self.list.error.clone().unwrap_or_else(|| t!("ide-todo-none"))
            };
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(text).color(p.text_secondary));
            });
            return;
        }
        let id = ui.make_persistent_id("todo-tree");
        let area = ui.interact(ui.available_rect_before_wrap(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("ide-todo-title"));
        if area.clicked() {
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
        let rows = self.visible();
        let selected = self.selected_row();
        let line_width = self
            .list
            .files
            .iter()
            .flat_map(|file| &file.items)
            .map(|item| item.line + 1)
            .max()
            .map_or(1, |line| line.to_string().len()) as f32
            * 7.5;
        let mut clicked: Option<(Row, bool, bool)> = None;
        let mut drawn = Vec::new();
        let summary = self.summary();
        let reveal =
            std::mem::take(&mut self.reveal).then(|| selected.and_then(|at| rows.iter().position(|row| *row == at)));
        ui.spacing_mut().item_spacing.y = 0.0;
        let area = egui::ScrollArea::vertical().id_salt("todo-scroll").auto_shrink([false, false]);
        area.show_rows(ui, ROW, rows.len() + 1, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let top = ui.max_rect().top() - range.start as f32 * ROW;
            if let Some(Some(at)) = reveal {
                let y = top + (at + 1) as f32 * ROW;
                ui.scroll_to_rect(Rect::from_x_y_ranges(ui.max_rect().x_range(), y..=y + ROW), None);
            }
            for index in range {
                let sense = if index == 0 { Sense::hover() } else { Sense::click() };
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), sense);
                let Some(row) = index.checked_sub(1).and_then(|at| rows.get(at)).copied() else {
                    ui.painter().with_clip_rect(rect).text(
                        pos2(rect.left() + 12.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        &summary,
                        theme::regular(13.0),
                        p.text_secondary,
                    );
                    accessibility::label(ui, ui.scope_id().with("todo-summary"), rect, || summary.clone());
                    continue;
                };
                drawn.push((row, rect));
                let is_selected = selected == Some(row);
                if is_selected {
                    ui.painter().rect_filled(
                        rect.shrink2(vec2(4.0, 0.0)),
                        4,
                        if focused { p.selection } else { p.hover },
                    );
                } else if response.hovered() {
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 4, p.hover);
                }
                let chevron = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(12.0));
                match row {
                    Row::File(index) => {
                        let file = &self.list.files[index];
                        let expanded = !self.collapsed.contains(&file.path);
                        accessibility::tree_item(&response, 1, Some(expanded), is_selected, &file.relative);
                        self.file_row(ui, p, rect, chevron, index);
                    }
                    Row::Item(file, item) => {
                        let found = &self.list.files[file].items[item];
                        accessibility::name_with(&response, egui::Role::TreeItem, || {
                            format!("{}: {}", found.line + 1, found.line_text.trim())
                        });
                        accessibility::edit(&response, |node| {
                            node.set_level(2);
                            node.set_selected(is_selected);
                        });
                        self.item_row(ui, p, rect, line_width, file, item);
                    }
                }
                if response.clicked() || response.double_clicked() {
                    let on_chevron = response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    clicked = Some((row, response.double_clicked(), on_chevron));
                }
            }
        });
        self.rows = drawn;
        if let Some((row, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            self.select(row);
            match row {
                Row::File(file) if double || on_chevron => self.toggle(file),
                Row::File(_) => {}
                Row::Item(..) if double => actions.extend(self.go_to(row, true)),
                Row::Item(..) if self.options.navigate_with_single_click => actions.extend(self.go_to(row, false)),
                Row::Item(..) => {}
            }
        }
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<TodoAction>) {
        let none = Modifiers::NONE;
        let (up, down, left, right, enter, f4, escape, expand, collapse) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Enter),
                input.consume_key(none, Key::F4),
                input.consume_key(none, Key::Escape),
                input.consume_key(Modifiers::COMMAND, Key::Plus) || input.consume_key(Modifiers::COMMAND, Key::Equals),
                input.consume_key(Modifiers::COMMAND, Key::Minus),
            )
        });
        if expand {
            self.expand_all();
        }
        if collapse {
            self.collapse_all();
        }
        let rows = self.visible();
        let at = self.selected_row().and_then(|selected| rows.iter().position(|row| *row == selected));
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(rows.len().saturating_sub(1)),
            };
            if let Some(row) = rows.get(next).copied() {
                self.select(row);
                if self.options.navigate_with_single_click {
                    actions.extend(self.go_to(row, false));
                }
            }
            self.reveal = true;
        }
        match self.selected_row() {
            Some(Row::File(file)) if left && !self.collapsed.contains(&self.list.files[file].path) => self.toggle(file),
            Some(Row::File(file)) if right && self.collapsed.contains(&self.list.files[file].path) => self.toggle(file),
            Some(Row::File(file)) if enter => self.toggle(file),
            Some(Row::Item(file, _)) if left => {
                self.select(Row::File(file));
                self.reveal = true;
            }
            Some(row @ Row::Item(..)) if enter || f4 => actions.extend(self.go_to(row, true)),
            _ => {}
        }
        if escape {
            actions.push(TodoAction::FocusEditor);
        }
    }

    fn file_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, chevron: Rect, index: usize) {
        let file = &self.list.files[index];
        if self.collapsed.contains(&file.path) {
            crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
        } else {
            crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
        }
        let name = file.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        crate::file_icons::file_icon(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 38.0, rect.center().y), Vec2::splat(15.0)),
            &name,
        );
        let painter = ui.painter().with_clip_rect(rect);
        let label = painter.text(
            pos2(rect.left() + 52.0, rect.center().y),
            Align2::LEFT_CENTER,
            &name,
            theme::regular(13.0),
            p.text,
        );
        let folder = Path::new(&file.relative).parent().map(|folder| folder.display().to_string()).unwrap_or_default();
        let mut x = label.right() + 8.0;
        if !folder.is_empty() {
            x = painter
                .text(pos2(x, rect.center().y), Align2::LEFT_CENTER, folder, theme::regular(12.5), p.text_secondary)
                .right()
                + 8.0;
        }
        painter.text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            t!("ide-todo-items", count = file.items.len()),
            theme::regular(12.5),
            p.text_secondary,
        );
    }

    fn filter_button(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        rect: Rect,
        filters: &[String],
        actions: &mut Vec<TodoAction>,
    ) {
        let center = pos2(rect.center().x, rect.top() + 16.0 + 28.0 * 6.0 + 24.0);
        let area = Rect::from_center_size(center, Vec2::splat(24.0));
        let response = ui.interact(area, ui.scope_id().with("todo-filter"), Sense::click());
        let on = self.options.filter.is_some();
        if on {
            ui.painter().rect_filled(area, 6, p.popup_hover);
        } else {
            hover(ui, &response, p);
        }
        glyph_filter(ui.painter(), area.shrink(6.0), if on { p.text } else { p.text_secondary });
        let name = t!("ide-todo-filter");
        hint(ui, &response, &name, None, Side::Right);
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(180.0);
            if ui.selectable_label(!on, t!("ide-todo-filter-all")).clicked() {
                self.options.filter = None;
                ui.close();
            }
            for filter in filters {
                if ui.selectable_label(self.options.filter.as_ref() == Some(filter), filter).clicked() {
                    self.options.filter = Some(filter.clone());
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(t!("ide-todo-filter-edit")).clicked() {
                actions.push(TodoAction::EditFilters);
                ui.close();
            }
        });
    }

    fn item_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, line_width: f32, file: usize, item: usize) {
        let item = &self.list.files[file].items[item];
        let painter = ui.painter().with_clip_rect(rect);
        let left = rect.left() + 52.0;
        painter.text(
            pos2(left + line_width, rect.center().y),
            Align2::RIGHT_CENTER,
            (item.line + 1).to_string(),
            theme::regular(12.5),
            p.text_secondary,
        );
        let mut line = item.line_text.clone();
        for more in &item.continued {
            line.push(' ');
            line.push_str(more);
        }
        let galley = painter.layout_job(item_job(&line, item.column, item.length, p));
        painter.galley(pos2(left + line_width + 12.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    }

    fn draw_preview(&self, ui: &Ui, p: &IdePalette, rect: Rect) {
        let painter = ui.painter().with_clip_rect(rect);
        let Some(((_, line, _), preview)) = &self.preview else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                t!("ide-todo-preview-none"),
                theme::regular(13.0),
                p.text_secondary,
            );
            return;
        };
        let family = metrics::editor_family(ui.ctx());
        let height = 20.0;
        let at = line.saturating_sub(preview.first) as f32;
        let offset = (at * height - rect.height() / 2.0 + height / 2.0).max(0.0);
        for (row, text) in preview.lines.iter().enumerate() {
            let y = rect.top() + 8.0 + row as f32 * height - offset + height / 2.0;
            if y < rect.top() - height || y > rect.bottom() + height {
                continue;
            }
            let index = preview.first + row;
            if index == *line {
                painter.rect_filled(
                    Rect::from_min_max(pos2(rect.left(), y - height / 2.0), pos2(rect.right(), y + height / 2.0)),
                    0,
                    p.current_line,
                );
            }
            painter.text(
                pos2(rect.left() + 44.0, y),
                Align2::RIGHT_CENTER,
                (index + 1).to_string(),
                FontId::new(12.0, family.clone()),
                p.gutter_text,
            );
            painter.text(
                pos2(rect.left() + 56.0, y),
                Align2::LEFT_CENTER,
                text.replace('\t', "    "),
                FontId::new(12.5, family.clone()),
                p.code,
            );
        }
    }

    fn preview_key(&self) -> Option<(PathBuf, usize, u64)> {
        if !self.options.preview_source {
            return None;
        }
        match self.selected_row()? {
            Row::Item(file, item) => {
                let file = &self.list.files[file];
                Some((file.path.clone(), file.items[item].line, self.stamp.unwrap_or_default()))
            }
            Row::File(_) => None,
        }
    }
}

fn tab_title(tab: TodoTab) -> String {
    match tab {
        TodoTab::Project => t!("ide-todo-tab-project"),
        TodoTab::CurrentFile => t!("ide-todo-tab-current-file"),
        TodoTab::ScopeBased => t!("ide-todo-tab-scope-based"),
        TodoTab::CurrentChangelist => t!("ide-todo-tab-current-changelist"),
    }
}

fn item_job(line: &str, column: usize, length: usize, p: &IdePalette) -> LayoutJob {
    let lead = line.chars().take_while(|c| c.is_whitespace()).count().min(column);
    let chars: Vec<char> = line.chars().skip(lead).collect();
    let start = column - lead;
    let end = (start + length).min(chars.len());
    let start = start.min(end);
    let plain = TextFormat { font_id: theme::regular(13.0), color: p.text, ..TextFormat::default() };
    let todo = TextFormat { font_id: theme::regular(13.0), color: p.link, italics: true, ..TextFormat::default() };
    let mut job = LayoutJob::default();
    job.append(&chars[..start].iter().collect::<String>(), 0.0, plain.clone());
    job.append(&chars[start..end].iter().collect::<String>(), 0.0, todo);
    job.append(chars[end..].iter().collect::<String>().trim_end(), 0.0, plain);
    job
}

impl IdeView {
    pub(super) fn follow_todo(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session, now: f64) {
        if !self.tool_visible("todo") {
            return;
        }
        let git = (self.vcs.info.root.is_some(), self.vcs.info.revision);
        if let Some(after) = self.todo_window.follow(registry, session, now, git) {
            ctx.request_repaint_after(after);
        }
    }

    pub(super) fn todo_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if self.todo_window.options.tab != TodoTab::ScopeBased {
            self.todo_window.scopes.clear();
        } else if self.todo_window.scopes.is_empty() {
            let list = registry
                .dispatch(session, "search.scope.list", json!({}))
                .ok()
                .and_then(|value| serde_json::from_value::<ScopeList>(value).ok())
                .unwrap_or_default();
            self.todo_window.scopes = list.scopes.into_iter().map(|scope| (scope.id, scope.name)).collect();
        }
        let key = self.todo_window.preview_key();
        if key.is_none() {
            self.todo_window.preview = None;
        } else if self.todo_window.preview.as_ref().map(|(seen, _)| seen) != key.as_ref()
            && let Some((path, line, stamp)) = key
        {
            let preview = registry
                .dispatch(session, "search.text.preview", json!({ "path": path, "line": line, "lines": PREVIEW_LINES }))
                .ok()
                .and_then(|value| serde_json::from_value::<Preview>(value).ok())
                .unwrap_or_default();
            self.todo_window.preview = Some(((path, line, stamp), preview));
        }
        let palette = self.palette;
        let filters: Vec<String> =
            self.overview.settings.todo.filters.iter().map(|filter| filter.name.clone()).collect();
        for action in self.todo_window.show(ui, &palette, &self.tool_windows, &filters) {
            if let Some((command, params)) = action.command() {
                self.dispatch(ui, registry, session, command, params);
            }
            match action {
                TodoAction::Hide => self.hide_tool("todo"),
                TodoAction::Layout(request) => self.apply_menu_request(ui.ctx(), "todo", request),
                TodoAction::EditFilters => self.settings.open_at(super::settings::PageId::Todo),
                TodoAction::GoTo { focus_editor: true, .. } | TodoAction::FocusEditor => self.editor_view.focus(),
                TodoAction::GoTo { .. } => {}
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
