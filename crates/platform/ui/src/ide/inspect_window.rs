use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::inspect_code::{
    ExportFormat, InspectCode, InspectRequest, InspectionResults, ItemFixList, ResultItem, ResultsTree, ScopeKind,
};
use zen_workbench::inspection_profiles::{ProfileId, ProfileList};
use zen_workbench::language::Severity;

use super::glyphs::{
    Glyph, glyph_collapse_all, glyph_expand_all, glyph_export, glyph_eye, glyph_folder, glyph_minus, glyph_rerun,
};
use super::scope_row::{ScopeChoices, scope_combo};
use super::{IdePalette, IdeView, Side, dialog_frame, form, hint, hover};
use crate::{accessibility, file_icons, theme};

const ROW: f32 = 24.0;
const TOOLBAR: f32 = 34.0;
const INDENT: f32 = 16.0;
const LABEL: f32 = 120.0;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct InspectOptions {
    pub(super) scope: ScopeKind,
    pub(super) directory: String,
    pub(super) custom: String,
    pub(super) include_tests: bool,
    pub(super) group_by_directory: bool,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            scope: ScopeKind::Project,
            directory: String::new(),
            custom: "projectFiles".to_owned(),
            include_tests: true,
            group_by_directory: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Severity(Severity),
    Inspection(Severity, String),
    Folder(Severity, String, String),
    File(Severity, String, String),
    Item(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    node: Node,
    depth: usize,
    label: String,
    count: usize,
    items: Vec<u64>,
    path: Option<PathBuf>,
}

pub(in crate::ide) fn severity_title(severity: Severity) -> String {
    match severity {
        Severity::Error => t!("ide-inspect-severity-error"),
        Severity::Warning => t!("ide-inspect-severity-warning"),
        Severity::WeakWarning => t!("ide-inspect-severity-weak-warning"),
        Severity::Typo => t!("ide-inspect-severity-typo"),
        Severity::Information => t!("ide-inspect-severity-information"),
        Severity::Hint => t!("ide-inspect-severity-hint"),
    }
}

fn severity_name(severity: Severity) -> String {
    serde_json::to_value(severity).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default()
}

fn rows_of(tree: &ResultsTree, results: &InspectionResults, collapsed: &BTreeSet<Node>) -> Vec<Row> {
    let mut rows = Vec::new();
    for severity in &tree.severities {
        let key = severity.severity;
        let all: Vec<u64> = severity
            .inspections
            .iter()
            .flat_map(|node| &node.folders)
            .flat_map(|folder| &folder.files)
            .flat_map(|file| file.items.iter().copied())
            .collect();
        let node = Node::Severity(key);
        let shut = collapsed.contains(&node);
        rows.push(Row { node, depth: 0, label: severity_title(key), count: severity.count, items: all, path: None });
        if shut {
            continue;
        }
        for inspection in &severity.inspections {
            let all: Vec<u64> = inspection
                .folders
                .iter()
                .flat_map(|folder| &folder.files)
                .flat_map(|file| file.items.iter().copied())
                .collect();
            let node = Node::Inspection(key, inspection.id.clone());
            let shut = collapsed.contains(&node);
            rows.push(Row {
                node,
                depth: 1,
                label: inspection.name.clone(),
                count: inspection.count,
                items: all,
                path: None,
            });
            if shut {
                continue;
            }
            for folder in &inspection.folders {
                let mut depth = 2;
                let mut hidden = false;
                if let Some(directory) = &folder.directory {
                    let node = Node::Folder(key, inspection.id.clone(), directory.clone());
                    hidden = collapsed.contains(&node);
                    let items: Vec<u64> = folder.files.iter().flat_map(|file| file.items.iter().copied()).collect();
                    let label = if directory.is_empty() { "/".to_owned() } else { directory.clone() };
                    rows.push(Row { node, depth, label, count: items.len(), items, path: None });
                    depth += 1;
                }
                if hidden {
                    continue;
                }
                for file in &folder.files {
                    let node = Node::File(key, inspection.id.clone(), file.relative.clone());
                    let shut = collapsed.contains(&node);
                    let name = Path::new(&file.relative).file_name().map(|name| name.to_string_lossy().into_owned());
                    rows.push(Row {
                        node,
                        depth,
                        label: name.unwrap_or_default(),
                        count: file.items.len(),
                        items: file.items.clone(),
                        path: Some(file.path.clone()),
                    });
                    if shut {
                        continue;
                    }
                    for id in &file.items {
                        let Some(item) = results.items.iter().find(|item| item.id == *id) else { continue };
                        rows.push(Row {
                            node: Node::Item(*id),
                            depth: depth + 1,
                            label: item.problem.message.lines().next().unwrap_or_default().to_owned(),
                            count: 0,
                            items: vec![*id],
                            path: Some(file.path.clone()),
                        });
                    }
                }
            }
        }
    }
    rows
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum InspectAction {
    GoTo { path: PathBuf, offset: usize, focus_editor: bool },
    Rerun(u64),
    Exclude { run: u64, items: Vec<u64>, excluded: bool },
    Fix { run: u64, items: Vec<u64>, fix: Option<String> },
    RunWindowAction(Value),
    Export(ExportFormat),
}

#[derive(Debug, Default)]
pub(super) struct InspectWindow {
    pub(super) options: InspectOptions,
    pub(super) tab: Option<u64>,
    pub(super) pending: Option<u64>,
    pub(super) show_excluded: bool,
    results: Option<InspectionResults>,
    tree: Option<ResultsTree>,
    stamp: Option<(u64, u64, bool, bool)>,
    rows: Vec<Row>,
    collapsed: BTreeSet<Node>,
    selected: Option<Node>,
    drawn: Vec<(Node, Rect)>,
    reveal: bool,
    fixes: Option<(u64, ItemFixList)>,
}

impl InspectWindow {
    fn refresh(&mut self, registry: &Registry, session: &mut Session) {
        let Some(run) = self.tab else { return };
        let revision = session.state::<InspectCode>().map_or(0, InspectCode::revision);
        let stamp = (run, revision, self.options.group_by_directory, self.show_excluded);
        if self.stamp == Some(stamp) {
            return;
        }
        self.stamp = Some(stamp);
        self.fixes = None;
        self.results = session
            .state::<InspectCode>()
            .and_then(|state| state.runs().iter().find(|results| results.run == run).cloned());
        let params = json!({
            "run": run,
            "groupByDirectory": self.options.group_by_directory,
            "showExcluded": self.show_excluded,
        });
        self.tree = registry
            .dispatch(session, "inspect.results.tree", params)
            .ok()
            .and_then(|tree| serde_json::from_value(tree).ok());
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.rows = match (&self.tree, &self.results) {
            (Some(tree), Some(results)) => rows_of(tree, results, &self.collapsed),
            _ => Vec::new(),
        };
        if self.selected.as_ref().is_none_or(|selected| !self.rows.iter().any(|row| row.node == *selected)) {
            self.selected = self.rows.first().map(|row| row.node.clone());
            self.fixes = None;
        }
    }

    fn item(&self, id: u64) -> Option<&ResultItem> {
        self.results.as_ref()?.items.iter().find(|item| item.id == id)
    }

    fn selected_row(&self) -> Option<&Row> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().find(|row| row.node == *selected)
    }

    fn selected_item(&self) -> Option<&ResultItem> {
        match self.selected.as_ref()? {
            Node::Item(id) => self.item(*id),
            _ => None,
        }
    }

    fn select(&mut self, node: Node) {
        if self.selected.as_ref() != Some(&node) {
            self.fixes = None;
        }
        self.selected = Some(node);
    }

    fn toggle(&mut self, node: &Node) {
        if !self.collapsed.remove(node) {
            self.collapsed.insert(node.clone());
        }
        self.rebuild();
    }

    fn expand_all(&mut self) {
        self.collapsed.clear();
        self.rebuild();
    }

    fn collapse_all(&mut self) {
        let all: Vec<Node> = self.rows.iter().map(|row| row.node.clone()).filter(|node| !is_item(node)).collect();
        self.collapsed.extend(all);
        self.rebuild();
    }

    fn jump(&self, focus_editor: bool) -> Option<InspectAction> {
        let item = self.selected_item().or_else(|| self.item(*self.selected_row()?.items.first()?))?;
        Some(InspectAction::GoTo { path: item.path.clone(), offset: item.problem.start, focus_editor })
    }

    fn excluded(&self, row: &Row) -> bool {
        row.items.iter().all(|id| self.item(*id).is_some_and(|item| item.excluded))
    }

    fn exclude_selected(&self) -> Option<InspectAction> {
        let row = self.selected_row()?;
        Some(InspectAction::Exclude { run: self.tab?, items: row.items.clone(), excluded: !self.excluded(row) })
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<InspectAction>) {
        let none = Modifiers::NONE;
        let (up, down, left, right, enter, f4, expand, collapse, delete) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Enter),
                input.consume_key(none, Key::F4),
                input.consume_key(Modifiers::COMMAND, Key::Plus) || input.consume_key(Modifiers::COMMAND, Key::Equals),
                input.consume_key(Modifiers::COMMAND, Key::Minus),
                input.consume_key(none, Key::Delete) || input.consume_key(none, Key::Backspace),
            )
        });
        if expand {
            self.expand_all();
        }
        if collapse {
            self.collapse_all();
        }
        let at = self.selected.as_ref().and_then(|selected| self.rows.iter().position(|row| row.node == *selected));
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(self.rows.len().saturating_sub(1)),
            };
            if let Some(row) = self.rows.get(next).map(|row| row.node.clone()) {
                self.select(row);
                self.reveal = true;
            }
        }
        if let Some(node) = self.selected.clone().filter(|node| !is_item(node)) {
            let shut = self.collapsed.contains(&node);
            if (left && !shut) || (right && shut) {
                self.toggle(&node);
            }
        }
        if enter || f4 {
            actions.extend(self.jump(true));
        }
        if delete {
            actions.extend(self.exclude_selected());
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Vec<InspectAction> {
        let mut actions = Vec::new();
        let body = ui.available_rect_before_wrap();
        let toolbar = Rect::from_min_size(body.min, vec2(TOOLBAR, body.height()));
        self.toolbar(ui, p, toolbar, &mut actions);
        let rest = Rect::from_min_max(pos2(toolbar.right(), body.top()), body.max);
        let split = rest.left() + rest.width() * 0.6;
        let tree = Rect::from_min_max(rest.min, pos2(split, rest.bottom()));
        let details = Rect::from_min_max(pos2(split + 1.0, rest.top()), rest.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tree).id_salt("inspect-tree"));
        self.tree(&mut child, p, &mut actions);
        ui.painter().vline(details.left() - 0.5, details.y_range(), egui::Stroke::new(1.0, p.tab_border));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(details.shrink(10.0)).id_salt("inspect-details"));
        self.details(&mut child, p, &mut actions);
        ui.allocate_rect(body, Sense::hover());
        actions
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect, actions: &mut Vec<InspectAction>) {
        let by_directory = self.options.group_by_directory;
        let excluded = self.show_excluded;
        let buttons: [(Glyph, String, Option<bool>); 8] = [
            (glyph_rerun, t!("ide-inspect-rerun"), None),
            (glyph_folder, t!("ide-inspect-group-by-directory"), Some(by_directory)),
            (glyph_expand_all, t!("ide-todo-expand-all"), None),
            (glyph_collapse_all, t!("ide-todo-collapse-all"), None),
            (glyph_minus, t!("ide-inspect-exclude"), None),
            (glyph_eye, t!("ide-inspect-show-excluded"), Some(excluded)),
            (glyph_export, t!("ide-inspect-export-html"), None),
            (glyph_export, t!("ide-inspect-export-xml"), None),
        ];
        for (index, (glyph, name, on)) in buttons.into_iter().enumerate() {
            let gap = 5.0 * [1, 4, 6].iter().filter(|start| index >= **start).count() as f32;
            let center = pos2(rect.center().x, rect.top() + 15.0 + 25.0 * index as f32 + gap);
            let response = ui.interact(
                Rect::from_center_size(center, Vec2::splat(24.0)),
                ui.scope_id().with(("inspect-tool", index)),
                Sense::click(),
            );
            if on == Some(true) {
                ui.painter().rect_filled(response.rect, 6, p.popup_hover);
            } else {
                hover(ui, &response, p);
            }
            glyph(ui.painter(), response.rect.shrink(6.0), if on == Some(true) { p.text } else { p.text_secondary });
            hint(ui, &response, &name, None, Side::Right);
            if let Some(on) = on {
                accessibility::pressed(&response, on);
            }
            if response.clicked() {
                match index {
                    0 => actions.extend(self.tab.map(InspectAction::Rerun)),
                    1 => self.options.group_by_directory = !by_directory,
                    2 => self.expand_all(),
                    3 => self.collapse_all(),
                    4 => actions.extend(self.exclude_selected()),
                    5 => self.show_excluded = !excluded,
                    6 => actions.push(InspectAction::Export(ExportFormat::Html)),
                    _ => actions.push(InspectAction::Export(ExportFormat::Xml)),
                }
            }
        }
    }

    fn summary(&self) -> Option<String> {
        let results = self.results.as_ref()?;
        if results.items.is_empty() {
            return Some(results.note.clone().unwrap_or_else(|| t!("ide-inspect-none")));
        }
        None
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<InspectAction>) {
        let notice = self.unchecked_notice();
        if let Some(text) = notice {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(text).color(p.text_secondary));
            });
        }
        if let Some(text) = self.summary() {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(text).color(p.text_secondary));
            });
            return;
        }
        let id = ui.make_persistent_id("inspect-tree");
        let area = ui.interact(ui.available_rect_before_wrap(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("ide-inspect-tree"));
        if area.clicked() {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        if ui.memory(|memory| memory.has_focus(id)) {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
            self.keys(ui, actions);
        }
        let reveal = std::mem::take(&mut self.reveal);
        let rows = self.rows.clone();
        let selected = self.selected.clone();
        let mut drawn = Vec::new();
        let mut clicked = None;
        egui::ScrollArea::vertical().id_salt("inspect-scroll").auto_shrink([false, false]).show_rows(
            ui,
            ROW,
            rows.len(),
            |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for index in range {
                    let row = &rows[index];
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    drawn.push((row.node.clone(), rect));
                    let is_selected = selected.as_ref() == Some(&row.node);
                    if is_selected {
                        ui.painter().rect_filled(rect, 4, p.selection);
                        if reveal {
                            ui.scroll_to_rect(rect, None);
                        }
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.hover);
                    }
                    self.draw_row(ui, p, rect, row);
                    accessibility::name_with(&response, egui::Role::TreeItem, || row.label.clone());
                    accessibility::edit(&response, |node| {
                        node.set_level(row.depth + 1);
                        node.set_selected(is_selected);
                    });
                    if response.clicked() || response.double_clicked() {
                        let chevron = rect.left() + row.depth as f32 * INDENT + 20.0;
                        let on_chevron = response.interact_pointer_pos().is_some_and(|at| at.x < chevron);
                        clicked = Some((row.node.clone(), response.double_clicked(), on_chevron));
                    }
                }
            },
        );
        self.drawn = drawn;
        if let Some((node, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            self.select(node.clone());
            if is_item(&node) {
                if double {
                    actions.extend(self.jump(true));
                }
            } else if double || on_chevron {
                self.toggle(&node);
            }
        }
    }

    fn unchecked_notice(&self) -> Option<String> {
        let results = self.results.as_ref()?;
        if results.unchecked.is_empty() {
            return None;
        }
        Some(t!("ide-inspect-unchecked", count = results.unchecked.len()))
    }

    fn draw_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, row: &Row) {
        let painter = ui.painter().with_clip_rect(rect);
        let x = rect.left() + 6.0 + row.depth as f32 * INDENT;
        let y = rect.center().y;
        let excluded = self.excluded(row);
        let color = if excluded { p.text_secondary } else { p.text };
        if !is_item(&row.node) {
            let shut = self.collapsed.contains(&row.node);
            painter.text(
                pos2(x + 4.0, y),
                Align2::CENTER_CENTER,
                if shut { "›" } else { "⌄" },
                theme::regular(12.0),
                p.text_secondary,
            );
        }
        let mut text_at = x + 16.0;
        match &row.node {
            Node::Severity(severity) => {
                painter.circle_filled(pos2(text_at + 4.0, y), 4.0, p.problem(&severity_name(*severity)));
                text_at += 14.0;
            }
            Node::Item(id) => {
                if let Some(item) = self.item(*id) {
                    painter.circle_filled(
                        pos2(text_at + 4.0, y),
                        4.0,
                        p.problem(&severity_name(item.problem.severity)),
                    );
                }
                text_at += 14.0;
            }
            Node::File(..) => {
                if let Some(path) = &row.path {
                    file_icons::file_icon_at(
                        &painter,
                        Rect::from_center_size(pos2(text_at + 7.0, y), Vec2::splat(15.0)),
                        path,
                    );
                }
                text_at += 20.0;
            }
            Node::Folder(..) => {
                glyph_folder(
                    &painter,
                    Rect::from_center_size(pos2(text_at + 7.0, y), Vec2::splat(14.0)),
                    p.text_secondary,
                );
                text_at += 20.0;
            }
            Node::Inspection(..) => {}
        }
        let font = if matches!(row.node, Node::Severity(_)) { theme::semibold(13.0) } else { theme::regular(13.0) };
        let label = painter.text(pos2(text_at, y), Align2::LEFT_CENTER, &row.label, font, color);
        let detail = match row.node {
            Node::Item(id) => self.item(id).map(|item| format!(":{}", item.problem.line)),
            _ => Some(t!("ide-problems-count", count = row.count)),
        };
        if let Some(detail) = detail {
            painter.text(
                pos2(label.right() + 8.0, y),
                Align2::LEFT_CENTER,
                detail,
                theme::regular(12.5),
                p.text_secondary,
            );
        }
    }

    fn description_of(&self, row: &Row) -> (String, String) {
        let Some(tree) = &self.tree else { return Default::default() };
        let wanted = match &row.node {
            Node::Inspection(_, id) | Node::Folder(_, id, _) | Node::File(_, id, _) => Some(id.clone()),
            Node::Item(id) => self.item(*id).map(|item| item.inspection.clone()),
            Node::Severity(_) => None,
        };
        let found = tree
            .severities
            .iter()
            .flat_map(|severity| &severity.inspections)
            .find(|node| Some(&node.id) == wanted.as_ref());
        found.map(|node| (node.name.clone(), node.description.clone())).unwrap_or_default()
    }

    fn details(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<InspectAction>) {
        let Some(row) = self.selected_row().cloned() else { return };
        let text = |text: &str, color: Color32| egui::RichText::new(text).font(theme::regular(13.0)).color(color);
        let (name, description) = self.description_of(&row);
        egui::ScrollArea::vertical().id_salt("inspect-details-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            if !name.is_empty() {
                ui.label(egui::RichText::new(&name).font(theme::semibold(13.0)).color(p.text));
            }
            if !description.is_empty() {
                ui.add(egui::Label::new(text(&description, p.text_secondary)).wrap());
            }
            let item = self.selected_item().cloned();
            if let Some(item) = &item {
                ui.add(egui::Label::new(text(&item.problem.message, p.text)).wrap());
                for extra in [&item.problem.note, &item.problem.details].into_iter().flatten() {
                    ui.add(egui::Label::new(text(extra, p.text_secondary)).wrap());
                }
                let place = format!("{}:{}:{}", item.relative, item.problem.line, item.problem.column);
                ui.label(text(&place, p.text_secondary));
            }
            ui.add_space(4.0);
            if let Some(note) = self.fixes_note() {
                ui.label(text(&note, p.text_secondary));
            }
            for (title, action) in self.buttons() {
                if ui.button(text(&title, p.text)).clicked() {
                    actions.push(action);
                }
            }
        });
    }

    fn fixes_note(&self) -> Option<String> {
        let item = self.selected_item()?;
        let (_, list) = self.fixes.as_ref().filter(|(id, _)| *id == item.id)?;
        if list.pending {
            Some(t!("ide-inspect-fixes-loading"))
        } else if list.fixes.is_empty() {
            Some(t!("ide-inspect-no-fixes"))
        } else {
            None
        }
    }

    fn buttons(&self) -> Vec<(String, InspectAction)> {
        let run = self.tab.unwrap_or_default();
        if let Some(item) = self.selected_item() {
            let Some((_, list)) = self.fixes.as_ref().filter(|(id, _)| *id == item.id) else { return Vec::new() };
            return list
                .fixes
                .iter()
                .map(|fix| {
                    let action = match &fix.action {
                        Some(action) => InspectAction::RunWindowAction(action.clone()),
                        None => InspectAction::Fix { run, items: vec![item.id], fix: Some(fix.id.clone()) },
                    };
                    (fix.title.clone(), action)
                })
                .collect();
        }
        let Some(row) = self.selected_row() else { return Vec::new() };
        let label =
            if matches!(row.node, Node::File(..)) { t!("ide-inspect-fix-all-file") } else { t!("ide-inspect-fix-all") };
        vec![(label, InspectAction::Fix { run, items: row.items.clone(), fix: None })]
    }

    fn follow_fixes(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let Some(run) = self.tab else { return false };
        let Some(id) = self.selected_item().map(|item| item.id) else { return false };
        if self.fixes.as_ref().is_some_and(|(seen, list)| *seen == id && !list.pending) {
            return false;
        }
        let list = registry
            .dispatch_unrecorded(session, "inspect.results.fixes", json!({ "run": run, "item": id }))
            .ok()
            .and_then(|answer| serde_json::from_value::<ItemFixList>(answer).ok())
            .unwrap_or_default();
        let pending = list.pending;
        self.fixes = Some((id, list));
        pending
    }
}

fn is_item(node: &Node) -> bool {
    matches!(node, Node::Item(_))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Named {
    id: String,
    title: String,
    group: String,
    description: String,
}

#[derive(Debug, Default)]
struct ByName {
    list: Vec<Named>,
    query: String,
    chosen: Option<Named>,
}

impl ByName {
    fn shown(&self) -> Vec<&Named> {
        let query = self.query.trim().to_lowercase();
        self.list
            .iter()
            .filter(|named| {
                query.is_empty()
                    || named.title.to_lowercase().contains(&query)
                    || named.group.to_lowercase().contains(&query)
                    || named.description.to_lowercase().contains(&query)
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub(super) struct InspectDialog {
    options: InspectOptions,
    files: Vec<PathBuf>,
    file_mask: String,
    choices: ScopeChoices,
    by_name: Option<ByName>,
    cleanup: bool,
    profiles: Vec<ProfileId>,
    profile: ProfileId,
    error: Option<String>,
}

impl InspectDialog {
    fn request(&self) -> InspectRequest {
        let options = &self.options;
        InspectRequest {
            scope: options.scope,
            directory: options.directory.trim().to_owned(),
            files: if options.scope == ScopeKind::Files { self.files.clone() } else { Vec::new() },
            custom: options.custom.clone(),
            include_tests: Some(options.include_tests),
            inspection: self
                .by_name
                .as_ref()
                .and_then(|by_name| by_name.chosen.as_ref())
                .map(|chosen| chosen.id.clone()),
            file_mask: self.file_mask.trim().to_owned(),
            profile: Some(self.profile.clone()),
        }
    }

    fn ready(&self) -> bool {
        match self.options.scope {
            ScopeKind::Directory => !self.options.directory.trim().is_empty(),
            ScopeKind::Files => !self.files.is_empty(),
            ScopeKind::Project | ScopeKind::Uncommitted | ScopeKind::Custom => true,
        }
    }
}

enum Choice {
    Analyze,
    Configure,
    Cancel,
}

fn show_picker(ui: &mut Ui, p: &IdePalette, by_name: &mut ByName) -> bool {
    let modal = egui::Modal::new(ui.scope_id().with("inspect-by-name")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(egui::RichText::new(t!("ide-inspect-by-name-title")).font(theme::semibold(15.0)).color(p.text));
        let field = form::framed(ui, p, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut by_name.query)
                    .desired_width(ui.available_width())
                    .hint_text(t!("ide-inspect-by-name-hint"))
                    .id_salt("inspect-by-name-query"),
            )
        });
        field.request_focus();
        let shown: Vec<Named> = by_name.shown().into_iter().cloned().collect();
        let mut chosen = None;
        if shown.is_empty() {
            ui.label(egui::RichText::new(t!("ide-inspect-by-name-none")).color(p.text_secondary));
        }
        egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for named in &shown {
                let line = format!("{}  —  {}", named.title, named.group);
                let response = ui.selectable_label(false, egui::RichText::new(line).font(theme::regular(13.0)));
                if !named.description.is_empty() {
                    response.clone().on_hover_text(&named.description);
                }
                if response.clicked() {
                    chosen = Some(named.clone());
                }
            }
        });
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) && shown.len() == 1 {
            chosen = shown.first().cloned();
        }
        if chosen.is_some() {
            by_name.chosen = chosen;
        }
    });
    modal.should_close()
}

fn show_dialog(ui: &mut Ui, p: &IdePalette, dialog: &mut InspectDialog) -> Option<Choice> {
    let mut choice = None;
    let text = |text: String, color: Color32| egui::RichText::new(text).font(theme::regular(13.0)).color(color);
    let named = dialog.by_name.as_ref().and_then(|by_name| by_name.chosen.as_ref()).map(|chosen| chosen.title.clone());
    let modal = egui::Modal::new(ui.scope_id().with("inspect-code")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(500.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        let title = match &named {
            Some(name) => t!("ide-inspect-run-title", inspection = name.as_str()),
            None if dialog.cleanup => t!("ide-cleanup-title"),
            None => t!("ide-inspect-title"),
        };
        ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
        let options = &mut dialog.options;
        form::radio(ui, p, &mut options.scope, ScopeKind::Project, &t!("ide-inspect-whole-project"));
        ui.horizontal(|ui| {
            form::radio(ui, p, &mut options.scope, ScopeKind::Directory, &t!("ide-inspect-directory"));
            form::framed(ui, p, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut options.directory)
                        .desired_width(300.0)
                        .id_salt("inspect-directory"),
                )
            });
        });
        let selected = t!("ide-inspect-selected-files", count = dialog.files.len());
        form::radio(ui, p, &mut options.scope, ScopeKind::Files, &selected);
        form::radio(ui, p, &mut options.scope, ScopeKind::Uncommitted, &t!("ide-inspect-uncommitted"));
        ui.horizontal(|ui| {
            form::radio(ui, p, &mut options.scope, ScopeKind::Custom, &t!("ide-inspect-custom"));
            scope_combo(ui, "inspect-custom", &dialog.choices, &mut options.custom, 300.0);
        });
        form::check(ui, p, &mut options.include_tests, &t!("ide-inspect-include-tests"));
        if named.is_some() {
            ui.horizontal(|ui| {
                form::label(ui, p, &t!("ide-inspect-file-mask"), LABEL);
                form::framed(ui, p, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.file_mask)
                            .desired_width(300.0)
                            .hint_text("*.go,!*_test.go")
                            .id_salt("inspect-file-mask"),
                    )
                });
            });
        } else {
            ui.horizontal(|ui| {
                form::label(ui, p, &t!("ide-inspect-profile"), LABEL);
                egui::ComboBox::from_id_salt("inspect-profile")
                    .selected_text(dialog.profile.name.clone())
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for profile in &dialog.profiles {
                            let name = profile.name.clone();
                            ui.selectable_value(&mut dialog.profile, profile.clone(), name);
                        }
                    });
                if ui.button(text(t!("ide-inspect-configure"), p.text)).clicked() {
                    choice = Some(Choice::Configure);
                }
            });
        }
        if let Some(error) = &dialog.error {
            ui.add(egui::Label::new(text(error.clone(), p.error_text)).wrap());
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let ready = dialog.ready();
            let run = if dialog.cleanup { t!("ide-cleanup-run") } else { t!("ide-inspect-analyze") };
            let analyze = egui::Button::new(text(run, p.accent_text))
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(if ready { p.accent } else { p.accent.gamma_multiply(0.4) });
            if ui.add_enabled(ready, analyze).clicked() {
                choice = Some(Choice::Analyze);
            }
            let cancel = egui::Button::new(text(t!("settings-cancel"), p.text))
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(1.0, p.button_border));
            if ui.add(cancel).clicked() {
                choice = Some(Choice::Cancel);
            }
            if ready && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
                choice = Some(Choice::Analyze);
            }
        });
    });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

impl IdeView {
    fn scope_files(&self, session: &Session) -> Vec<PathBuf> {
        let visible: Vec<PathBuf> = self.selection.lead().cloned().into_iter().collect();
        let mut files: Vec<PathBuf> =
            self.selection.items(&visible).into_iter().filter(|path| path.is_file()).collect();
        if files.is_empty() {
            files.extend(
                session
                    .state::<Workbench>()
                    .and_then(Workbench::active_document)
                    .map(|document| document.path().to_owned()),
            );
        }
        files
    }

    pub(super) fn open_inspect_dialog(&mut self, registry: &Registry, session: &mut Session, by_name: bool) {
        let by_name = by_name.then(|| {
            let list = registry.dispatch(session, "inspect.inspections.list", json!({})).unwrap_or_default();
            let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
            let list = list
                .as_array()
                .into_iter()
                .flatten()
                .map(|info| Named {
                    id: text(&info["id"]),
                    title: text(&info["title"]),
                    group: text(&info["group"]),
                    description: text(&info["description"]),
                })
                .collect();
            ByName { list, ..ByName::default() }
        });
        let list = registry
            .dispatch(session, "inspect.profile.list", json!({}))
            .ok()
            .and_then(|value| serde_json::from_value::<ProfileList>(value).ok())
            .unwrap_or_else(|| ProfileList { active: ProfileId::default(), profiles: vec![ProfileId::default()] });
        self.inspect_dialog = Some(InspectDialog {
            options: self.inspect_window.options.clone(),
            files: self.scope_files(session),
            file_mask: String::new(),
            choices: ScopeChoices::load(registry, session, false),
            by_name,
            cleanup: false,
            profile: list.active,
            profiles: list.profiles,
            error: None,
        });
    }

    pub(super) fn open_cleanup_dialog(&mut self, registry: &Registry, session: &mut Session) {
        self.open_inspect_dialog(registry, session, false);
        if let Some(dialog) = &mut self.inspect_dialog {
            dialog.cleanup = true;
        }
    }

    pub(super) fn silent_cleanup(&mut self, registry: &Registry, session: &mut Session) -> Result<(), String> {
        let files = self.scope_files(session);
        if files.is_empty() {
            return Err(t!("ide-cleanup-nothing"));
        }
        let params = json!({ "scope": ScopeKind::Files, "files": files });
        registry.dispatch(session, "inspect.cleanup.run", params).map(|_| ()).map_err(|error| error.to_string())
    }

    pub(super) fn inspect_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(dialog) = &mut self.inspect_dialog else { return };
        if let Some(by_name) = dialog.by_name.as_mut().filter(|by_name| by_name.chosen.is_none()) {
            if show_picker(ui, &p, by_name) {
                self.inspect_dialog = None;
            }
            return;
        }
        match show_dialog(ui, &p, dialog) {
            None => {}
            Some(Choice::Cancel) => self.inspect_dialog = None,
            Some(Choice::Configure) => {
                self.inspect_dialog = None;
                self.dispatch(ui, registry, session, "view.settings.open", json!({ "page": "inspections" }));
            }
            Some(Choice::Analyze) => {
                let params = serde_json::to_value(dialog.request()).unwrap_or_default();
                let command = if dialog.cleanup { "inspect.cleanup.run" } else { "inspect.code.run" };
                match registry.dispatch(session, command, params) {
                    Ok(started) => {
                        let group = self.inspect_window.options.group_by_directory;
                        self.inspect_window.options =
                            InspectOptions { group_by_directory: group, ..dialog.options.clone() };
                        self.inspect_window.pending = started["run"].as_u64().filter(|_| !dialog.cleanup);
                        self.inspect_dialog = None;
                    }
                    Err(error) => dialog.error = Some(error.to_string()),
                }
            }
        }
    }

    pub(super) fn follow_inspections(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        if session.state::<InspectCode>().is_some_and(InspectCode::running) {
            let _ = registry.dispatch_unrecorded(session, "inspect.code.status", json!({}));
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let finished = self.inspect_window.pending.filter(|run| {
            session.state::<InspectCode>().is_some_and(|state| state.runs().iter().any(|results| results.run == *run))
        });
        if let Some(run) = finished {
            self.inspect_window.pending = None;
            self.inspect_window.tab = Some(run);
            self.show_tool("problems");
        }
        if let Some(run) = self.inspect_window.tab
            && session.state::<InspectCode>().is_none_or(|state| state.runs().iter().all(|results| results.run != run))
        {
            self.inspect_window.tab = None;
        }
    }

    pub(super) fn inspect_tabs(&self, session: &Session) -> Vec<(u64, String)> {
        session
            .state::<InspectCode>()
            .map(|state| {
                state
                    .runs()
                    .iter()
                    .map(|results| (results.run, t!("ide-inspect-tab", scope = results.title.as_str())))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn export_results(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, format: ExportFormat) {
        let (name, extension) = match format {
            ExportFormat::Html => ("inspection-results.html", "html"),
            ExportFormat::Xml => ("inspection-results.xml", "xml"),
        };
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let mut picker = rfd::FileDialog::new()
            .set_title(t!("ide-inspect-export-title"))
            .set_file_name(name)
            .add_filter(extension, &[extension]);
        if let Some(root) = root {
            picker = picker.set_directory(root);
        }
        let Some(path) = picker.save_file() else { return };
        let params = json!({ "run": self.inspect_window.tab, "format": format, "path": path });
        self.dispatch(ui, registry, session, "inspect.results.export", params);
    }

    pub(super) fn inspect_results(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.inspect_window.refresh(registry, session);
        if self.inspect_window.follow_fixes(registry, session) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        let palette = self.palette;
        for action in self.inspect_window.show(ui, &palette) {
            match action {
                InspectAction::GoTo { path, offset, focus_editor } => {
                    self.dispatch(
                        ui,
                        registry,
                        session,
                        "navigate.place.goTo",
                        json!({ "path": path, "offset": offset }),
                    );
                    if focus_editor {
                        self.editor_view.focus();
                    }
                }
                InspectAction::Rerun(run) => {
                    match registry.dispatch(session, "inspect.results.rerun", json!({ "run": run })) {
                        Ok(started) => self.inspect_window.pending = started["run"].as_u64(),
                        Err(error) => self.report(ui, error),
                    }
                }
                InspectAction::Exclude { run, items, excluded } => {
                    let params = json!({ "run": run, "items": items, "excluded": excluded });
                    self.dispatch(ui, registry, session, "inspect.results.exclude", params);
                }
                InspectAction::Fix { run, items, fix } => {
                    let params = json!({ "run": run, "items": items, "fix": fix });
                    self.dispatch(ui, registry, session, "inspect.results.fix", params);
                }
                InspectAction::RunWindowAction(action) => {
                    let name = action["action"].as_str().unwrap_or_default().to_owned();
                    let params = action.get("params").cloned().unwrap_or_else(|| json!({}));
                    self.dispatch(ui, registry, session, &name, params);
                }
                InspectAction::Export(format) => self.export_results(ui, registry, session, format),
            }
        }
    }
}

#[cfg(test)]
mod tests;
