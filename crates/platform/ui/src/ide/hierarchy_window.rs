use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use egui::{Align2, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::language::hierarchy::{
    Hierarchies, HierarchyDirection, HierarchyKind, HierarchyNode, HierarchyScope, HierarchyState, HierarchyTab,
    NodeLocation,
};

use super::glyphs::{
    Glyph, glyph_callees, glyph_callers, glyph_close, glyph_collapse_all, glyph_expand_all, glyph_minus, glyph_pin,
    glyph_recursive, glyph_refresh, glyph_single_click, glyph_sort_alpha, glyph_subtypes, glyph_supertypes,
    symbol_badge,
};
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const TOOLBAR: f32 = 30.0;

const BUTTON: f32 = 24.0;

const STEP: f32 = 26.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum HierarchyAction {
    Hide,
    FocusEditor,
    GoTo { path: PathBuf, offset: usize, focus_editor: bool },
    Select(usize),
    Expand(usize),
    Collapse(usize),
    Direction(HierarchyDirection),
    Sort(bool),
    Scope(HierarchyScope),
    SingleClick(bool),
    Refresh,
    ExpandAll,
    CollapseAll,
    Pin(bool),
    Close,
}

impl HierarchyAction {
    pub(super) fn command(&self, tab: Option<usize>) -> Option<(&'static str, Value)> {
        Some(match self {
            Self::Hide | Self::FocusEditor => return None,
            Self::GoTo { path, offset, .. } => ("navigate.place.goTo", json!({ "path": path, "offset": offset })),
            Self::Select(tab) => ("navigate.hierarchy.select", json!({ "tab": tab })),
            Self::Expand(node) => ("navigate.hierarchy.expand", json!({ "node": node })),
            Self::Collapse(node) => ("navigate.hierarchy.collapse", json!({ "node": node })),
            Self::Direction(direction) => {
                ("navigate.hierarchy.setOptions", json!({ "tab": tab, "direction": direction }))
            }
            Self::Sort(sort) => ("navigate.hierarchy.setOptions", json!({ "tab": tab, "sort": sort })),
            Self::Scope(scope) => ("navigate.hierarchy.setOptions", json!({ "tab": tab, "scope": scope })),
            Self::SingleClick(on) => ("navigate.hierarchy.setOptions", json!({ "singleClick": on })),
            Self::Refresh => ("navigate.hierarchy.refresh", json!({ "tab": tab })),
            Self::ExpandAll => ("navigate.hierarchy.expandAll", json!({ "tab": tab })),
            Self::CollapseAll => ("navigate.hierarchy.collapse", json!({ "tab": tab })),
            Self::Pin(pinned) => ("navigate.hierarchy.pin", json!({ "tab": tab, "pinned": pinned })),
            Self::Close => ("navigate.hierarchy.close", json!({ "tab": tab })),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) node: usize,
    pub(super) depth: usize,
}

#[derive(Debug, Default)]
pub(super) struct HierarchyWindow {
    state: HierarchyState,
    seen: Option<u64>,
    started: u64,
    selected: Option<usize>,
    reveal: bool,
    focus_next: bool,
    index: HashMap<usize, usize>,
    tree: Vec<Row>,
    viewport: Option<(f32, f32)>,
    rows: Vec<(Row, Rect)>,
    tabs: Vec<(usize, Rect)>,
    buttons: Vec<(&'static str, Rect)>,
}

pub(super) fn tab_title(tab: &HierarchyTab) -> String {
    let symbol = tab.symbol.clone().unwrap_or_default();
    match (tab.kind, tab.direction) {
        (_, Some(HierarchyDirection::Callers)) => t!("ide-hierarchy-tab-callers", symbol = symbol),
        (_, Some(HierarchyDirection::Callees)) => t!("ide-hierarchy-tab-callees", symbol = symbol),
        (_, Some(HierarchyDirection::Supertypes)) => t!("ide-hierarchy-tab-supertypes", symbol = symbol),
        (_, Some(HierarchyDirection::Subtypes)) => t!("ide-hierarchy-tab-subtypes", symbol = symbol),
        (HierarchyKind::Call, None) => t!("ide-hierarchy-tab-call", symbol = symbol),
        (HierarchyKind::Type, None) => t!("ide-hierarchy-tab-type", symbol = symbol),
    }
}

fn scope_name(scope: HierarchyScope) -> String {
    match scope {
        HierarchyScope::Project => t!("ide-hierarchy-scope-project"),
        HierarchyScope::Test => t!("ide-hierarchy-scope-test"),
        HierarchyScope::All => t!("ide-hierarchy-scope-all"),
    }
}

impl HierarchyWindow {
    pub(super) fn should_read(&self, hierarchies: &Hierarchies) -> bool {
        hierarchies.pending() || self.seen != Some(hierarchies.revision())
    }

    pub(super) fn update(&mut self, state: HierarchyState, revision: u64, started: u64) -> bool {
        let fresh = started != self.started;
        self.started = started;
        self.seen = Some(revision);
        let tab_changed = self.state.tab != state.tab;
        self.state = state;
        self.index = self.state.nodes.iter().enumerate().map(|(at, node)| (node.id, at)).collect();
        self.tree = self.layout();
        let known = self.selected.is_some_and(|selected| self.node(selected).is_some());
        if fresh || tab_changed || !known {
            self.selected = self.state.roots.first().copied().filter(|root| self.node(*root).is_some());
            self.reveal = true;
        }
        if fresh {
            self.focus_next = true;
        }
        fresh
    }

    pub(super) fn state(&self) -> &HierarchyState {
        &self.state
    }

    #[cfg(test)]
    pub(super) fn row_rect(&self, node: usize) -> Option<Rect> {
        self.rows.iter().find(|(row, _)| row.node == node).map(|(_, rect)| *rect)
    }

    #[cfg(test)]
    pub(super) fn tab_rect(&self, tab: usize) -> Option<Rect> {
        self.tabs.iter().find(|(id, _)| *id == tab).map(|(_, rect)| *rect)
    }

    fn node(&self, id: usize) -> Option<&HierarchyNode> {
        self.index.get(&id).and_then(|at| self.state.nodes.get(*at))
    }

    fn layout(&self) -> Vec<Row> {
        let mut depths: HashMap<usize, usize> = HashMap::with_capacity(self.state.nodes.len());
        self.state
            .nodes
            .iter()
            .map(|node| {
                let depth = node.parent.and_then(|parent| depths.get(&parent).map(|depth| depth + 1)).unwrap_or(0);
                depths.insert(node.id, depth);
                Row { node: node.id, depth }
            })
            .collect()
    }

    fn expandable(node: &HierarchyNode) -> bool {
        !node.recursive && node.children.as_ref().is_none_or(|children| !children.is_empty())
    }

    fn expanded(node: &HierarchyNode) -> bool {
        node.children.as_ref().is_some_and(|children| !children.is_empty()) && !node.collapsed
    }

    fn go_to(&self, id: usize, focus_editor: bool) -> Option<HierarchyAction> {
        let node = self.node(id)?;
        Some(HierarchyAction::GoTo { path: node.selection.path.clone(), offset: node.selection.start, focus_editor })
    }

    fn toggle(&self, id: usize) -> Option<HierarchyAction> {
        let node = self.node(id)?;
        if Self::expanded(node) {
            Some(HierarchyAction::Collapse(id))
        } else if Self::expandable(node) {
            Some(HierarchyAction::Expand(id))
        } else {
            None
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Vec<HierarchyAction> {
        let mut actions = Vec::new();
        self.header(ui, p, &mut actions);
        if self.state.open {
            self.toolbar(ui, p, &mut actions);
        }
        match self.message() {
            Some(text) => {
                let body = ui.available_rect_before_wrap();
                let galley = ui.painter().layout(text, theme::regular(13.0), p.text_secondary, body.width() - 32.0);
                let at = pos2(body.center().x - galley.size().x / 2.0, body.top() + body.height() * 0.4);
                ui.painter().galley(at, galley, p.text_secondary);
            }
            None => self.tree(ui, p, &mut actions),
        }
        actions
    }

    fn message(&self) -> Option<String> {
        if !self.state.open {
            return Some(t!("ide-hierarchy-empty"));
        }
        if !self.state.nodes.is_empty() {
            return None;
        }
        Some(match &self.state.error {
            Some(error) => error.clone(),
            None if self.state.pending => t!("ide-hierarchy-building"),
            None => t!("ide-hierarchy-nothing"),
        })
    }

    fn header(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<HierarchyAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-hierarchy-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("hierarchy-title"), title, || t!("ide-hierarchy-title"));
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(BUTTON));
        let mut x = title.right() + 14.0;
        let mut drawn = Vec::new();
        let clip =
            ui.painter().with_clip_rect(Rect::from_x_y_ranges(header.left()..=hide.left() - 4.0, header.y_range()));
        let count = self.state.tabs.len().max(1) as f32;
        let room = ((hide.left() - 4.0 - x) / count - 6.0).max(40.0);
        for tab in &self.state.tabs {
            let title = tab_title(tab);
            let pin = if tab.pinned { 16.0 } else { 0.0 };
            let mut job = egui::text::LayoutJob::simple_singleline(title.clone(), theme::regular(13.0), p.text);
            job.wrap = egui::text::TextWrapping {
                max_width: (room - 20.0 - pin).max(12.0),
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('…'),
            };
            let galley = ui.painter().layout_job(job);
            let rect = Rect::from_min_size(pos2(x, header.center().y - 13.0), vec2(galley.size().x + 20.0 + pin, 26.0));
            let response = ui.interact(rect, ui.scope_id().with(("hierarchy-tab", tab.tab)), Sense::click());
            let active = self.state.active == Some(tab.tab);
            accessibility::item(&response, egui::Role::Tab, active, &title);
            if tab.pinned {
                accessibility::describe(&response, &t!("ide-hierarchy-pinned"));
            }
            if active {
                clip.rect_filled(rect, 6, p.selection);
            } else if response.hovered() {
                clip.rect_filled(rect, 6, p.hover);
            }
            if tab.pinned {
                let at = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(12.0));
                glyph_pin(&clip, at, p.text_secondary);
            }
            clip.galley(pos2(rect.left() + 10.0 + pin, rect.center().y - galley.size().y / 2.0), galley, p.text);
            if response.clicked() && !active {
                actions.push(HierarchyAction::Select(tab.tab));
            }
            drawn.push((tab.tab, rect));
            x = rect.right() + 6.0;
        }
        self.tabs = drawn;
        let response = ui.interact(hide, ui.scope_id().with("hierarchy-hide"), Sense::click());
        accessibility::button_with(&response, || t!("a11y-hide"));
        hover(ui, &response, p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-hierarchy-hide"), None, Side::Below);
        if response.clicked() {
            actions.push(HierarchyAction::Hide);
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<HierarchyAction>) {
        let state = &self.state;
        let (directions, glyphs): ([HierarchyDirection; 2], [Glyph; 2]) = match state.kind {
            Some(HierarchyKind::Type) => {
                ([HierarchyDirection::Supertypes, HierarchyDirection::Subtypes], [glyph_supertypes, glyph_subtypes])
            }
            _ => ([HierarchyDirection::Callers, HierarchyDirection::Callees], [glyph_callers, glyph_callees]),
        };
        let direction_name = |direction: HierarchyDirection| match direction {
            HierarchyDirection::Callers => "ide-hierarchy-callers",
            HierarchyDirection::Callees => "ide-hierarchy-callees",
            HierarchyDirection::Supertypes => "ide-hierarchy-supertypes",
            HierarchyDirection::Subtypes => "ide-hierarchy-subtypes",
        };
        let scope_text = t!("ide-hierarchy-scope-button", scope = scope_name(state.scope));
        let scope_width = ui.painter().layout_no_wrap(scope_text.clone(), theme::regular(12.5), p.text).size().x + 26.0;
        let width = ui.available_width();
        let widths: Vec<f32> = (0..10).map(|index| if index == 3 { scope_width } else { BUTTON }).collect();
        let places = |left: f32, top: f32| {
            let mut places = Vec::with_capacity(widths.len());
            let (mut x, mut y) = (left + 8.0, top + TOOLBAR / 2.0);
            for (index, w) in widths.iter().enumerate() {
                if x + w > left + width - 8.0 && index > 0 {
                    x = left + 8.0;
                    y += TOOLBAR;
                }
                places.push(Rect::from_min_size(pos2(x, y - BUTTON / 2.0), vec2(*w, BUTTON)));
                x += w + if matches!(index, 1 | 3) { 10.0 } else { STEP - BUTTON };
            }
            places
        };
        let rows = places(0.0, 0.0).last().map_or(1.0, |last| ((last.center().y / TOOLBAR) + 0.5).floor().max(1.0));
        let (bar, _) = ui.allocate_exact_size(vec2(width, TOOLBAR * rows), Sense::hover());
        let places = places(bar.left(), bar.top());
        let mut drawn = Vec::new();
        for (index, direction) in directions.into_iter().enumerate() {
            let on = state.direction == Some(direction);
            let id = if index == 0 { "direction-first" } else { "direction-second" };
            drawn.push((id, places[index]));
            if tool_button(ui, p, places[index], id, glyphs[index], &t!(direction_name(direction)), Some(on)) && !on {
                actions.push(HierarchyAction::Direction(direction));
            }
        }
        drawn.push(("sort", places[2]));
        if tool_button(ui, p, places[2], "sort", glyph_sort_alpha, &t!("ide-hierarchy-sort"), Some(state.sort)) {
            actions.push(HierarchyAction::Sort(!state.sort));
        }
        let scope_rect = places[3];
        let scope = ui.interact(scope_rect, ui.scope_id().with("hierarchy-scope"), Sense::click());
        hover(ui, &scope, p);
        accessibility::name(&scope, egui::Role::ComboBox, &scope_text);
        ui.painter().text(
            pos2(scope_rect.left() + 6.0, scope_rect.center().y),
            Align2::LEFT_CENTER,
            &scope_text,
            theme::regular(12.5),
            p.text,
        );
        crate::icons::chevron_down(
            ui.painter(),
            Rect::from_center_size(pos2(scope_rect.right() - 10.0, scope_rect.center().y), Vec2::splat(9.0)),
            p.text_secondary,
        );
        hint(ui, &scope, &t!("ide-hierarchy-scope"), None, Side::Below);
        drawn.push(("scope", scope_rect));
        egui::Popup::menu(&scope).show(|ui| {
            ui.set_min_width(140.0);
            for choice in [HierarchyScope::Project, HierarchyScope::Test, HierarchyScope::All] {
                if ui.radio(state.scope == choice, scope_name(choice)).clicked() && state.scope != choice {
                    actions.push(HierarchyAction::Scope(choice));
                    ui.close();
                }
            }
        });
        let (single, pinned) = (state.single_click, state.pinned);
        let buttons: [(&'static str, Glyph, &str, Option<bool>, HierarchyAction); 6] = [
            ("refresh", glyph_refresh, "ide-hierarchy-refresh", None, HierarchyAction::Refresh),
            (
                "single-click",
                glyph_single_click,
                "ide-hierarchy-single-click",
                Some(single),
                HierarchyAction::SingleClick(!single),
            ),
            ("expand-all", glyph_expand_all, "ide-hierarchy-expand-all", None, HierarchyAction::ExpandAll),
            ("collapse-all", glyph_collapse_all, "ide-hierarchy-collapse-all", None, HierarchyAction::CollapseAll),
            ("pin", glyph_pin, "ide-hierarchy-pin", Some(pinned), HierarchyAction::Pin(!pinned)),
            ("close", glyph_close, "ide-hierarchy-close", None, HierarchyAction::Close),
        ];
        for (index, (id, glyph, name, on, action)) in buttons.into_iter().enumerate() {
            let rect = places[index + 4];
            drawn.push((id, rect));
            if tool_button(ui, p, rect, id, glyph, &t!(name), on) {
                actions.push(action);
            }
        }
        self.buttons = drawn;
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<HierarchyAction>) {
        let id = ui.make_persistent_id("hierarchy-tree");
        let area = ui.interact(ui.available_rect_before_wrap(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("ide-hierarchy-title"));
        if std::mem::take(&mut self.focus_next) || area.clicked() {
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
        let rows = std::mem::take(&mut self.tree);
        let row_height = metrics::TREE_ROW;
        let mut area =
            egui::ScrollArea::vertical().id_salt(("hierarchy-rows", self.state.tab)).auto_shrink([false, false]);
        if std::mem::take(&mut self.reveal)
            && let Some(at) = self.selected.and_then(|selected| rows.iter().position(|row| row.node == selected))
        {
            let top = at as f32 * row_height;
            let (offset, height) = self.viewport.unwrap_or((0.0, 0.0));
            if top < offset || top + row_height > offset + height {
                area = area.vertical_scroll_offset((top - height / 2.0).max(0.0));
            }
        }
        let mut clicked: Option<(usize, bool, bool)> = None;
        let mut drawn = Vec::new();
        ui.spacing_mut().item_spacing.y = 0.0;
        let output = area.show_rows(ui, row_height, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows[range] {
                let Some(node) = self.node(row.node) else { continue };
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row_height), Sense::click());
                drawn.push((*row, rect));
                let selected = self.selected == Some(row.node);
                let expandable = Self::expandable(node);
                accessibility::tree_item(
                    &response,
                    row.depth + 1,
                    expandable.then(|| Self::expanded(node)),
                    selected,
                    &node.name,
                );
                let mut description = node.detail.clone().unwrap_or_default();
                if node.location == NodeLocation::Test {
                    description = format!("{description} {}", t!("ide-hierarchy-test")).trim().to_owned();
                }
                if node.recursive {
                    description = format!("{description} {}", t!("ide-hierarchy-recursive")).trim().to_owned();
                }
                accessibility::describe(&response, &description);
                if node.location == NodeLocation::Test {
                    ui.painter().rect_filled(rect, 0, p.test_row);
                }
                let highlight =
                    Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                if selected {
                    ui.painter().rect_filled(highlight, 5, if focused { p.selection } else { p.hover });
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 5, p.hover);
                }
                let x = highlight.left() + row.depth as f32 * metrics::TREE_INDENT;
                let chevron = Rect::from_center_size(pos2(x + metrics::TREE_ARROW, rect.center().y), Vec2::splat(11.0));
                if Self::expanded(node) {
                    crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
                } else if expandable {
                    crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
                }
                node_row(ui, p, rect, x, node);
                if response.clicked() || response.double_clicked() {
                    let on_chevron =
                        expandable && response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    clicked = Some((row.node, response.double_clicked(), on_chevron));
                }
            }
        });
        self.viewport = Some((output.state.clamped_offset().y, output.inner_rect.height()));
        self.tree = rows;
        self.rows = drawn;
        if let Some((node, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            if on_chevron {
                actions.extend(self.toggle(node));
            } else if double {
                actions.extend(self.go_to(node, true));
            } else if self.state.single_click {
                actions.extend(self.go_to(node, false));
            }
            self.selected = Some(node);
        }
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<HierarchyAction>) {
        let none = Modifiers::NONE;
        let (up, down, left, right, enter, f4, escape) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Enter),
                input.consume_key(none, Key::F4),
                input.consume_key(none, Key::Escape),
            )
        });
        let rows = self.tree.clone();
        let at = self.selected.and_then(|selected| rows.iter().position(|row| row.node == selected));
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(rows.len().saturating_sub(1)),
            };
            self.selected = rows.get(next).map(|row| row.node);
            self.reveal = true;
        }
        let Some(node) = self.selected.and_then(|selected| self.node(selected)).cloned() else {
            if escape {
                actions.push(HierarchyAction::FocusEditor);
            }
            return;
        };
        if left && Self::expanded(&node) {
            actions.push(HierarchyAction::Collapse(node.id));
        } else if left && node.parent.is_some() {
            self.selected = node.parent;
            self.reveal = true;
        } else if right && Self::expandable(&node) && !Self::expanded(&node) {
            actions.push(HierarchyAction::Expand(node.id));
        } else if right && Self::expanded(&node) {
            self.selected = node.children.as_ref().and_then(|children| children.first().copied());
            self.reveal = true;
        } else if enter || f4 {
            actions.extend(self.go_to(node.id, true));
        }
        if escape {
            actions.push(HierarchyAction::FocusEditor);
        }
    }
}

fn tool_button(ui: &mut Ui, p: &IdePalette, rect: Rect, id: &str, glyph: Glyph, name: &str, on: Option<bool>) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(("hierarchy-button", id)), Sense::click());
    if on == Some(true) {
        ui.painter().rect_filled(rect, 6, p.popup_hover);
    } else {
        hover(ui, &response, p);
    }
    glyph(ui.painter(), rect.shrink(6.0), if on == Some(true) { p.text } else { p.text_secondary });
    hint(ui, &response, name, None, Side::Below);
    accessibility::button(&response, name);
    if let Some(on) = on {
        accessibility::pressed(&response, on);
    }
    response.clicked()
}

fn node_row(ui: &Ui, p: &IdePalette, rect: Rect, x: f32, node: &HierarchyNode) {
    let badge = Rect::from_center_size(pos2(x + metrics::TREE_ICON, rect.center().y), Vec2::splat(14.0));
    symbol_badge(ui.painter(), badge, node.kind, p);
    let painter = ui.painter().with_clip_rect(rect);
    let color = if node.location == NodeLocation::Library { p.text_secondary } else { p.text };
    let name = painter.text(
        pos2(x + metrics::TREE_TEXT, rect.center().y),
        Align2::LEFT_CENTER,
        &node.name,
        theme::regular(13.0),
        color,
    );
    let mut right = name.right();
    if node.recursive {
        let at = Rect::from_center_size(pos2(right + 11.0, rect.center().y), Vec2::splat(12.0));
        glyph_recursive(&painter, at, p.text_secondary);
        right = at.right();
    }
    if let Some(detail) = node.detail.as_deref() {
        right = painter
            .text(
                pos2(right + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                detail,
                theme::regular(13.0),
                p.text_secondary,
            )
            .right();
    }
    if node.calls.len() > 1 {
        right = painter
            .text(
                pos2(right + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                t!("ide-hierarchy-calls", count = node.calls.len()),
                theme::regular(13.0),
                p.text_secondary,
            )
            .right();
    }
    if node.pending {
        painter.text(
            pos2(right + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            t!("ide-hierarchy-loading"),
            theme::regular(13.0),
            p.text_secondary,
        );
    } else if let Some(error) = &node.error {
        painter.text(
            pos2(right + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            error,
            theme::regular(13.0),
            p.error_text,
        );
    }
}

impl IdeView {
    pub(super) fn follow_hierarchy(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let Some(hierarchies) = session.state::<Hierarchies>() else { return };
        if !self.hierarchy_window.should_read(hierarchies) {
            return;
        }
        let (revision, started) = (hierarchies.revision(), hierarchies.started());
        let Ok(value) = registry.dispatch(session, "navigate.hierarchy.get", json!({})) else { return };
        let Ok(state) = serde_json::from_value::<HierarchyState>(value) else { return };
        let pending = state.pending;
        if self.hierarchy_window.update(state, revision, started) {
            self.show_tool("hierarchy");
        }
        if pending {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    pub(super) fn hierarchy_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        let tab = self.hierarchy_window.state().tab;
        for action in self.hierarchy_window.show(ui, &palette) {
            if let Some((command, params)) = action.command(tab) {
                self.dispatch(ui, registry, session, command, params);
            }
            match action {
                HierarchyAction::Hide => self.hide_tool("hierarchy"),
                HierarchyAction::GoTo { focus_editor: true, .. } | HierarchyAction::FocusEditor => {
                    self.editor_view.focus();
                }
                _ => {}
            }
        }
        self.follow_hierarchy(ui.ctx(), registry, session);
    }
}

#[cfg(test)]
mod tests;
