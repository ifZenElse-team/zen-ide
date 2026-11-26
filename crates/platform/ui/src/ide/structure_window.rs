use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{Align2, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::language::structure::{StructureStatus, Symbol};

use super::glyphs::{Glyph, glyph_fields, glyph_minus, glyph_more, glyph_sort_alpha, symbol_badge};
use super::tool_stripes::{MenuRequest, layout_menu};
use super::tool_windows::ToolWindowLayout;
use super::workspace::StructureOptions;
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

const PENDING_POLL: f64 = 0.1;
const UNAVAILABLE_POLL: f64 = 1.0;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Structure {
    pub path: PathBuf,
    pub status: StructureStatus,
    pub message: Option<String>,
    pub symbols: Vec<Symbol>,
    pub current: Vec<usize>,
}

impl Default for Structure {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            status: StructureStatus::Unavailable,
            message: None,
            symbols: Vec::new(),
            current: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReadKey {
    path: PathBuf,
    version: u64,
    caret: Option<usize>,
    sort_alphabetically: bool,
    show_fields: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum StructureAction {
    Hide,
    GoTo { path: PathBuf, offset: usize, focus_editor: bool },
    FocusEditor,
    Layout(MenuRequest),
}

impl StructureAction {
    pub(super) fn command(&self) -> Option<(&'static str, Value)> {
        match self {
            Self::GoTo { path, offset, .. } => Some(("navigate.place.goTo", json!({ "path": path, "offset": offset }))),
            Self::Hide | Self::FocusEditor | Self::Layout(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) index: Vec<usize>,
    pub(super) depth: usize,
    pub(super) parent: bool,
}

#[derive(Debug, Default)]
pub(super) struct StructureWindow {
    pub(super) options: StructureOptions,
    structure: Option<Structure>,
    seen: Option<ReadKey>,
    read_at: f64,
    collapsed: HashMap<PathBuf, HashSet<Vec<String>>>,
    selected: Option<Vec<usize>>,
    reveal: bool,
    rows: Vec<(Vec<usize>, Rect)>,
    select_current: bool,
}

impl StructureWindow {
    pub(super) fn key(&self, path: PathBuf, version: u64, caret: usize) -> ReadKey {
        ReadKey {
            path,
            version,
            caret: self.options.follow_caret.then_some(caret),
            sort_alphabetically: self.options.sort_alphabetically,
            show_fields: self.options.show_fields,
        }
    }

    pub(super) fn params(&self, path: &Path) -> Value {
        let sort = if self.options.sort_alphabetically { "name" } else { "position" };
        json!({ "path": path, "sort": sort, "fields": self.options.show_fields })
    }

    pub(super) fn should_read(&self, key: &ReadKey, now: f64) -> bool {
        if self.seen.as_ref() != Some(key) {
            return true;
        }
        let wait = match self.structure.as_ref().map(|structure| structure.status) {
            Some(StructureStatus::Pending) => PENDING_POLL,
            Some(StructureStatus::Unavailable) => UNAVAILABLE_POLL,
            _ => return false,
        };
        now - self.read_at >= wait
    }

    pub(super) fn update(&mut self, structure: Structure, key: ReadKey, now: f64) {
        let moved = self.seen.as_ref().is_none_or(|seen| seen.path != key.path || seen.caret != key.caret);
        let same_file = self.structure.as_ref().is_some_and(|previous| previous.path == structure.path);
        let selected = self.selected.take().filter(|_| same_file).map(|selected| self.names(&selected));
        let current = self.structure.as_ref().filter(|_| same_file).map(|previous| self.names(&previous.current));
        self.structure = Some(structure);
        self.seen = Some(key);
        self.read_at = now;
        self.selected = selected.and_then(|names| self.find(&names));
        let current_changed = current.is_none_or(|names| {
            names != self.structure.as_ref().map(|structure| self.names(&structure.current)).unwrap_or_default()
        });
        let asked = self.select_current && self.status() != Some(StructureStatus::Pending);
        if asked || (self.options.follow_caret && (moved || current_changed)) {
            self.select_current &= !asked;
            self.follow();
        }
    }

    pub(super) fn select_at_caret(&mut self) {
        self.seen = None;
        self.select_current = true;
    }

    pub(super) fn clear(&mut self) {
        self.structure = None;
        self.seen = None;
        self.selected = None;
    }

    pub(super) fn status(&self) -> Option<StructureStatus> {
        self.structure.as_ref().map(|structure| structure.status)
    }

    fn follow(&mut self) {
        let Some(current) = self.structure.as_ref().map(|structure| structure.current.clone()) else { return };
        if current.is_empty() || self.symbol(&current).is_none() {
            return;
        }
        for depth in 1..current.len() {
            self.set_collapsed(&current[..depth], false);
        }
        self.selected = Some(current);
        self.reveal = true;
    }

    fn symbol(&self, index: &[usize]) -> Option<&Symbol> {
        let (first, rest) = index.split_first()?;
        let mut symbol = self.structure.as_ref()?.symbols.get(*first)?;
        for at in rest {
            symbol = symbol.children.get(*at)?;
        }
        Some(symbol)
    }

    fn names(&self, index: &[usize]) -> Vec<String> {
        (1..=index.len()).filter_map(|depth| self.symbol(&index[..depth]).map(|symbol| symbol.name.clone())).collect()
    }

    fn find(&self, names: &[String]) -> Option<Vec<usize>> {
        let mut symbols = &self.structure.as_ref()?.symbols;
        let mut index = Vec::with_capacity(names.len());
        for name in names {
            let at = symbols.iter().position(|symbol| &symbol.name == name)?;
            index.push(at);
            symbols = &symbols[at].children;
        }
        (!index.is_empty()).then_some(index)
    }

    fn is_collapsed(&self, index: &[usize]) -> bool {
        let Some(structure) = &self.structure else { return false };
        self.collapsed.get(&structure.path).is_some_and(|collapsed| collapsed.contains(&self.names(index)))
    }

    fn set_collapsed(&mut self, index: &[usize], collapse: bool) {
        let Some(path) = self.structure.as_ref().map(|structure| structure.path.clone()) else { return };
        let names = self.names(index);
        let collapsed = self.collapsed.entry(path).or_default();
        if collapse {
            collapsed.insert(names);
        } else {
            collapsed.remove(&names);
        }
    }

    fn toggle(&mut self, index: &[usize]) {
        self.set_collapsed(index, !self.is_collapsed(index));
    }

    fn visible(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Some(structure) = &self.structure {
            let collapsed = self.collapsed.get(&structure.path);
            walk(&structure.symbols, &mut Vec::new(), &mut Vec::new(), collapsed, &mut rows);
        }
        rows
    }

    fn go_to(&self, index: &[usize], focus_editor: bool) -> Option<StructureAction> {
        let symbol = self.symbol(index)?;
        let path = self.structure.as_ref()?.path.clone();
        Some(StructureAction::GoTo { path, offset: symbol.selection_start, focus_editor })
    }

    fn message(&self) -> Option<String> {
        let Some(structure) = &self.structure else { return Some(t!("ide-structure-empty")) };
        if !structure.symbols.is_empty() {
            return None;
        }
        Some(match structure.status {
            StructureStatus::Pending => t!("ide-structure-loading"),
            StructureStatus::Unavailable => structure.message.clone().unwrap_or_else(|| t!("ide-structure-empty")),
            StructureStatus::Ready => t!("ide-structure-empty"),
        })
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, layout: &ToolWindowLayout) -> Vec<StructureAction> {
        let mut actions = Vec::new();
        self.header(ui, p, layout, &mut actions);
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

    fn header(&mut self, ui: &mut Ui, p: &IdePalette, layout: &ToolWindowLayout, actions: &mut Vec<StructureAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-structure-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("structure-title"), title, || t!("ide-structure-title"));
        let button = |index: usize| {
            Rect::from_center_size(
                pos2(header.right() - 20.0 - 26.0 * index as f32, header.center().y),
                Vec2::splat(24.0),
            )
        };
        let hide = ui.interact(button(0), ui.scope_id().with("structure-hide"), Sense::click());
        hover(ui, &hide, p);
        glyph_minus(ui.painter(), hide.rect.shrink(6.0), p.text_secondary);
        hint(ui, &hide, &t!("ide-structure-hide"), None, Side::Below);
        if hide.clicked() {
            actions.push(StructureAction::Hide);
        }
        let more = ui.interact(button(1), ui.scope_id().with("structure-more"), Sense::click());
        hover(ui, &more, p);
        glyph_more(ui.painter(), more.rect.shrink(6.0), p.text_secondary);
        hint(ui, &more, &t!("ide-structure-options"), None, Side::Below);
        let options = &mut self.options;
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(220.0);
            ui.checkbox(&mut options.single_click, t!("ide-structure-single-click"));
            ui.checkbox(&mut options.follow_caret, t!("ide-structure-follow"));
            ui.separator();
            if let Some(request) = layout_menu(ui, layout, "structure", false) {
                actions.push(StructureAction::Layout(request));
            }
        });
        let toggles: [(Glyph, &str); 2] =
            [(glyph_fields, "ide-structure-fields"), (glyph_sort_alpha, "ide-structure-sort")];
        for (index, (glyph, name)) in toggles.into_iter().enumerate() {
            let on = if index == 0 { self.options.show_fields } else { self.options.sort_alphabetically };
            let response =
                ui.interact(button(index + 2), ui.scope_id().with(("structure-toggle", index)), Sense::click());
            if on {
                ui.painter().rect_filled(response.rect, 6, p.popup_hover);
            } else {
                hover(ui, &response, p);
            }
            glyph(ui.painter(), response.rect.shrink(6.0), if on { p.text } else { p.text_secondary });
            hint(ui, &response, &t!(name), None, Side::Below);
            accessibility::pressed(&response, on);
            if response.clicked() {
                if index == 0 {
                    self.options.show_fields = !on;
                } else {
                    self.options.sort_alphabetically = !on;
                }
            }
        }
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<StructureAction>) {
        let id = ui.make_persistent_id("structure-tree");
        let area = ui.interact(ui.available_rect_before_wrap(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("ide-structure-title"));
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
        let mut clicked: Option<(Vec<usize>, bool, bool)> = None;
        let mut drawn = Vec::with_capacity(rows.len());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                drawn.push((row.index.clone(), rect));
                if let Some(symbol) = self.symbol(&row.index) {
                    let expanded = row.parent.then(|| !self.is_collapsed(&row.index));
                    let selected = self.selected.as_ref() == Some(&row.index);
                    accessibility::tree_item(&response, row.depth + 1, expanded, selected, &symbol.name);
                    accessibility::describe(&response, symbol.detail.as_deref().unwrap_or_default());
                }
                let highlight =
                    Rect::from_min_max(pos2(rect.left() + 12.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                if self.selected.as_ref() == Some(&row.index) {
                    ui.painter().rect_filled(highlight, 5, if focused { p.selection } else { p.hover });
                    if std::mem::take(&mut self.reveal) {
                        response.scroll_to_me(None);
                    }
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 5, p.hover);
                }
                let x = highlight.left() + row.depth as f32 * metrics::TREE_INDENT;
                let chevron = Rect::from_center_size(pos2(x + metrics::TREE_ARROW, rect.center().y), Vec2::splat(11.0));
                if row.parent && self.is_collapsed(&row.index) {
                    crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
                } else if row.parent {
                    crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
                }
                self.symbol_row(ui, p, rect, x, &row.index);
                if response.clicked() || response.double_clicked() {
                    let on_chevron =
                        row.parent && response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    clicked = Some((row.index.clone(), response.double_clicked(), on_chevron));
                }
            }
        });
        self.rows = drawn;
        if let Some((index, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            if on_chevron {
                self.toggle(&index);
            } else if double {
                actions.extend(self.go_to(&index, true));
            } else if self.options.single_click {
                actions.extend(self.go_to(&index, false));
            }
            self.selected = Some(index);
        }
    }

    fn symbol_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, x: f32, index: &[usize]) {
        let Some(symbol) = self.symbol(index) else { return };
        let badge = Rect::from_center_size(pos2(x + metrics::TREE_ICON, rect.center().y), Vec2::splat(14.0));
        symbol_badge(ui.painter(), badge, symbol.kind, p);
        let painter = ui.painter().with_clip_rect(rect);
        let name = painter.text(
            pos2(x + metrics::TREE_TEXT, rect.center().y),
            Align2::LEFT_CENTER,
            &symbol.name,
            theme::regular(13.0),
            p.text,
        );
        if let Some(detail) = symbol.detail.as_deref().filter(|detail| !detail.is_empty()) {
            painter.text(
                pos2(name.right() + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                detail,
                theme::regular(13.0),
                p.text_secondary,
            );
        }
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<StructureAction>) {
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
        let at = self.selected.as_ref().and_then(|selected| rows.iter().position(|row| &row.index == selected));
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(rows.len().saturating_sub(1)),
            };
            self.selected = rows.get(next).map(|row| row.index.clone());
            self.reveal = true;
        }
        let row = self.selected.as_ref().and_then(|selected| rows.iter().find(|row| &row.index == selected)).cloned();
        match row {
            Some(row) if left && row.parent && !self.is_collapsed(&row.index) => self.set_collapsed(&row.index, true),
            Some(row) if left && row.depth > 0 => {
                self.selected = Some(row.index[..row.depth].to_vec());
                self.reveal = true;
            }
            Some(row) if right && row.parent && self.is_collapsed(&row.index) => self.set_collapsed(&row.index, false),
            Some(row) if right && row.parent => {
                let mut child = row.index;
                child.push(0);
                self.selected = Some(child);
                self.reveal = true;
            }
            Some(row) if enter => actions.extend(self.go_to(&row.index, true)),
            _ => {}
        }
        if escape {
            actions.push(StructureAction::FocusEditor);
        }
    }
}

pub(super) fn walk(
    symbols: &[Symbol],
    index: &mut Vec<usize>,
    names: &mut Vec<String>,
    collapsed: Option<&HashSet<Vec<String>>>,
    rows: &mut Vec<Row>,
) {
    for (at, symbol) in symbols.iter().enumerate() {
        index.push(at);
        names.push(symbol.name.clone());
        rows.push(Row { index: index.clone(), depth: index.len() - 1, parent: !symbol.children.is_empty() });
        if !collapsed.is_some_and(|collapsed| collapsed.contains(names)) {
            walk(&symbol.children, index, names, collapsed, rows);
        }
        index.pop();
        names.pop();
    }
}

pub(super) fn read(registry: &Registry, session: &mut Session, path: &Path, params: Value) -> Structure {
    let unavailable =
        |reason: String| Structure { path: path.to_owned(), message: Some(reason), ..Structure::default() };
    match registry.dispatch(session, "editor.structure.get", params) {
        Ok(value) => serde_json::from_value(value).unwrap_or_else(|error| unavailable(error.to_string())),
        Err(zen_core::DispatchError::Failed { error, .. }) => unavailable(error.message),
        Err(error) => unavailable(error.to_string()),
    }
}

impl IdeView {
    pub(super) fn structure_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let now = ui.input(|input| input.time);
        let document = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .filter(|document| document.image().is_none())
            .map(|document| (document.path().to_owned(), document.version(), document.selections().primary().head));
        match document {
            None => self.structure_window.clear(),
            Some((path, version, caret)) => {
                let key = self.structure_window.key(path, version, caret);
                if self.structure_window.should_read(&key, now) {
                    let structure = read(registry, session, &key.path, self.structure_window.params(&key.path));
                    self.structure_window.update(structure, key, now);
                }
            }
        }
        match self.structure_window.status() {
            Some(StructureStatus::Pending) => ui.ctx().request_repaint_after(Duration::from_secs_f64(PENDING_POLL)),
            Some(StructureStatus::Unavailable) => {
                ui.ctx().request_repaint_after(Duration::from_secs_f64(UNAVAILABLE_POLL));
            }
            _ => {}
        }
        let palette = self.palette;
        for action in self.structure_window.show(ui, &palette, &self.tool_windows) {
            if let Some((command, params)) = action.command() {
                self.dispatch(ui, registry, session, command, params);
            }
            match action {
                StructureAction::Hide => self.hide_tool("structure"),
                StructureAction::Layout(request) => self.apply_menu_request(ui.ctx(), "structure", request),
                StructureAction::GoTo { focus_editor: true, .. } | StructureAction::FocusEditor => {
                    self.editor_view.focus();
                }
                StructureAction::GoTo { .. } => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, PointerButton, Pos2};
    use zen_syntax::SymbolKind;

    use super::*;

    struct Harness {
        ctx: egui::Context,
        window: StructureWindow,
        actions: Vec<StructureAction>,
        version: u64,
    }

    impl Harness {
        fn new(structure: Structure) -> Self {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut harness = Self { ctx, window: StructureWindow::default(), actions: Vec::new(), version: 1 };
            harness.take(structure, 0);
            harness.frame(Vec::new());
            harness
        }

        fn take(&mut self, structure: Structure, caret: usize) {
            let key = self.window.key(structure.path.clone(), self.version, caret);
            self.window.update(structure, key, 0.0);
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                self.actions.extend(self.window.show(ui, &IdePalette::zen_dark(), &ToolWindowLayout::default()));
            });
            output.textures_delta.clear();
        }

        fn click(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }

        fn row(&self, index: &[usize]) -> Rect {
            self.window.rows.iter().find(|(drawn, _)| drawn == index).map(|(_, rect)| *rect).unwrap()
        }

        fn drawn(&self) -> Vec<Vec<usize>> {
            self.window.rows.iter().map(|(index, _)| index.clone()).collect()
        }
    }

    fn symbol(name: &str, kind: SymbolKind, selection_start: usize, children: Vec<Symbol>) -> Symbol {
        Symbol {
            name: name.to_owned(),
            detail: Some("detail".to_owned()),
            kind,
            start: selection_start.saturating_sub(5),
            end: selection_start + 20,
            selection_start,
            line: 1,
            column: 1,
            children,
        }
    }

    fn structure(path: &Path) -> Structure {
        Structure {
            path: path.to_owned(),
            status: StructureStatus::Ready,
            message: None,
            symbols: vec![
                symbol(
                    "Server",
                    SymbolKind::Struct,
                    20,
                    vec![
                        symbol("addr", SymbolKind::Field, 30, Vec::new()),
                        symbol("port", SymbolKind::Field, 45, Vec::new()),
                    ],
                ),
                symbol("Start", SymbolKind::Method, 80, Vec::new()),
                symbol("main", SymbolKind::Function, 120, Vec::new()),
            ],
            current: Vec::new(),
        }
    }

    #[test]
    fn rows_follow_the_symbols_in_order_and_nesting_and_remember_collapsed_ones_per_file() {
        let (a, b) = (PathBuf::from("/p/a.go"), PathBuf::from("/p/b.go"));
        let mut structure_window = Harness::new(structure(&a));
        assert_eq!(structure_window.drawn(), [vec![0], vec![0, 0], vec![0, 1], vec![1], vec![2]], "expanded at first");
        let depths: Vec<usize> = structure_window.window.visible().iter().map(|row| row.depth).collect();
        assert_eq!(depths, [0, 1, 1, 0, 0]);
        let chevron = structure_window.row(&[0]).left_center() + vec2(12.0 + metrics::TREE_ARROW, 0.0);
        structure_window.click(chevron);
        structure_window.frame(Vec::new());
        assert_eq!(structure_window.drawn(), [vec![0], vec![1], vec![2]]);
        assert!(structure_window.actions.is_empty(), "the chevron only collapses");
        structure_window.take(structure(&b), 0);
        structure_window.frame(Vec::new());
        assert_eq!(structure_window.drawn().len(), 5, "another file has its own rows");
        structure_window.take(structure(&a), 0);
        structure_window.frame(Vec::new());
        assert_eq!(structure_window.drawn(), [vec![0], vec![1], vec![2]], "the collapsed row is remembered");
    }

    #[test]
    fn the_toggles_are_the_parameters_and_a_change_reads_again() {
        let path = PathBuf::from("/p/a.go");
        let mut structure_window = Harness::new(structure(&path));
        assert_eq!(structure_window.window.params(&path), json!({ "path": path, "sort": "position", "fields": true }));
        let key = structure_window.window.key(path.clone(), 1, 0);
        assert!(!structure_window.window.should_read(&key, 0.0));
        let header = structure_window.row(&[0]).top() - metrics::TOOL_HEADER / 2.0;
        let right = structure_window.row(&[0]).right();
        structure_window.click(pos2(right - 20.0 - 26.0 * 2.0, header));
        structure_window.click(pos2(right - 20.0 - 26.0 * 3.0, header));
        assert!(!structure_window.window.options.show_fields && structure_window.window.options.sort_alphabetically);
        assert_eq!(structure_window.window.params(&path), json!({ "path": path, "sort": "name", "fields": false }));
        let key = structure_window.window.key(path, 1, 0);
        assert!(structure_window.window.should_read(&key, 0.0), "a toggle changes what is shown");
    }

    #[test]
    fn a_click_goes_to_the_symbol_through_navigate_place_go_to() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "package main\n\ntype Server struct {\n\taddr string\n}\n\nfunc main() {}\n").unwrap();
        let mut structure_window = Harness::new(structure(&file));
        structure_window.click(structure_window.row(&[0, 1]).center());
        let go = StructureAction::GoTo { path: file.clone(), offset: 45, focus_editor: false };
        assert_eq!(structure_window.actions, std::slice::from_ref(&go));
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[0, 1][..]));

        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let go = StructureAction::GoTo { path: file.clone(), offset: 20, focus_editor: true };
        let (command, params) = go.command().unwrap();
        assert_eq!(command, "navigate.place.goTo");
        registry.dispatch(&mut session, command, params).unwrap();
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!((document.path(), document.selections().primary().head), (file.as_path(), 20));
    }

    #[test]
    fn without_single_click_navigation_a_click_only_selects_and_a_double_click_goes() {
        let path = PathBuf::from("/p/a.go");
        let mut structure_window = Harness::new(structure(&path));
        structure_window.window.options.single_click = false;
        let at = structure_window.row(&[2]).center();
        structure_window.click(at);
        assert!(structure_window.actions.is_empty());
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[2][..]));
        structure_window.click(at);
        assert_eq!(structure_window.actions, [StructureAction::GoTo { path, offset: 120, focus_editor: true }]);
    }

    #[test]
    fn keys_move_collapse_expand_and_go_to_the_selected_symbol() {
        let path = PathBuf::from("/p/a.go");
        let mut structure_window = Harness::new(structure(&path));
        structure_window.click(structure_window.row(&[0]).center());
        structure_window.actions.clear();
        structure_window.key(Key::ArrowDown);
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[0, 0][..]));
        structure_window.key(Key::ArrowLeft);
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[0][..]), "← goes to the parent");
        structure_window.key(Key::ArrowLeft);
        assert!(structure_window.window.is_collapsed(&[0]));
        structure_window.key(Key::ArrowDown);
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[1][..]), "a collapsed row hides its children");
        structure_window.key(Key::ArrowUp);
        structure_window.key(Key::ArrowRight);
        assert!(!structure_window.window.is_collapsed(&[0]));
        structure_window.key(Key::ArrowRight);
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[0, 0][..]), "→ goes to the first child");
        structure_window.key(Key::Enter);
        assert_eq!(structure_window.actions, [StructureAction::GoTo { path, offset: 30, focus_editor: true }]);
        structure_window.key(Key::Escape);
        assert_eq!(structure_window.actions.last(), Some(&StructureAction::FocusEditor));
    }

    #[test]
    fn follow_caret_selects_and_reveals_the_symbol_at_the_caret() {
        let path = PathBuf::from("/p/a.go");
        let mut structure_window = Harness::new(structure(&path));
        structure_window.window.set_collapsed(&[0], true);
        let mut at_caret = structure(&path);
        at_caret.current = vec![0, 1];
        structure_window.take(at_caret.clone(), 46);
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[0, 1][..]));
        assert!(!structure_window.window.is_collapsed(&[0]), "its parent is expanded");
        assert!(structure_window.window.reveal);
        structure_window.frame(Vec::new());
        assert!(!structure_window.window.reveal, "scrolled into view");

        structure_window.window.options.follow_caret = false;
        structure_window.window.selected = Some(vec![2]);
        at_caret.current = vec![1];
        structure_window.take(at_caret.clone(), 81);
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[2][..]), "not followed when off");

        structure_window.window.select_at_caret();
        let key = structure_window.window.key(path.clone(), structure_window.version, 81);
        assert!(structure_window.window.should_read(&key, 0.0), "Select In reads the structure again");
        structure_window.take(at_caret, 81);
        assert_eq!(structure_window.window.selected.as_deref(), Some(&[1][..]), "Select In → Structure");
        assert!(!structure_window.window.select_current, "once");
    }

    #[test]
    fn a_new_order_of_the_same_symbols_keeps_the_selection_and_does_not_scroll() {
        let path = PathBuf::from("/p/a.go");
        let mut window = StructureWindow::default();
        let mut server = structure(&path);
        server.current = vec![0, 1];
        let key = window.key(path.clone(), 1, 46);
        window.update(server.clone(), key.clone(), 0.0);
        window.set_collapsed(&[0], true);
        window.reveal = false;
        let mut syntax = server.clone();
        syntax.symbols.reverse();
        syntax.current = vec![2, 1];
        window.update(syntax, key.clone(), 0.1);
        assert_eq!(window.selected.as_deref(), Some(&[2, 1][..]), "the same symbol stays selected");
        assert!(window.is_collapsed(&[2]), "collapsed rows are kept by name");
        assert!(!window.reveal, "the symbol at the caret did not change, so nothing scrolls");
        window.update(server, key, 0.2);
        assert_eq!(window.selected.as_deref(), Some(&[0, 1][..]));
        assert!(!window.reveal);
    }

    #[test]
    fn the_structure_is_read_only_after_a_change_or_while_it_is_pending() {
        let path = PathBuf::from("/p/a.go");
        let mut window = StructureWindow::default();
        let key = window.key(path.clone(), 1, 10);
        assert!(window.should_read(&key, 0.0), "nothing read yet");
        window.update(structure(&path), key.clone(), 0.0);
        assert!(!window.should_read(&key, 0.0));
        assert!(!window.should_read(&key, 60.0), "a ready structure is not read again");
        assert!(window.should_read(&window.key(path.clone(), 2, 10), 0.0), "the text changed");
        assert!(window.should_read(&window.key(path.clone(), 1, 11), 0.0), "the caret moved");
        assert!(window.should_read(&window.key(PathBuf::from("/p/b.go"), 1, 10), 0.0), "another file");
        window.options.follow_caret = false;
        let unfollowed = window.key(path.clone(), 1, 10);
        assert!(window.should_read(&unfollowed, 0.0), "the toggle changed");
        window.update(structure(&path), unfollowed, 0.0);
        assert!(!window.should_read(&window.key(path.clone(), 1, 99), 0.0), "the caret is not followed");

        let key = window.key(path.clone(), 3, 0);
        window.update(Structure { status: StructureStatus::Pending, ..structure(&path) }, key.clone(), 1.0);
        assert!(!window.should_read(&key, 1.05));
        assert!(window.should_read(&key, 1.1), "pending is read again every 100 ms");
        window.update(Structure { path: path.clone(), ..Structure::default() }, key.clone(), 2.0);
        assert!(!window.should_read(&key, 2.5));
        assert!(window.should_read(&key, 3.0), "unavailable is read again every second");
    }

    #[test]
    fn the_states_without_symbols_say_why() {
        let path = PathBuf::from("/p/a.go");
        let mut window = StructureWindow::default();
        assert_eq!(window.message(), Some(t!("ide-structure-empty")), "no file");
        let key = window.key(path.clone(), 1, 0);
        let pending = Structure { path: path.clone(), status: StructureStatus::Pending, ..Structure::default() };
        window.update(pending, key.clone(), 0.0);
        assert_eq!(window.message(), Some(t!("ide-structure-loading")));
        let reason = Structure { path: path.clone(), message: Some("no server".to_owned()), ..Structure::default() };
        window.update(reason, key.clone(), 0.0);
        assert_eq!(window.message().as_deref(), Some("no server"));
        let ready = Structure { path: path.clone(), status: StructureStatus::Ready, ..Structure::default() };
        window.update(ready, key.clone(), 0.0);
        assert_eq!(window.message(), Some(t!("ide-structure-empty")));
        window.update(Structure { status: StructureStatus::Pending, ..structure(&path) }, key, 0.0);
        assert_eq!(window.message(), None, "the last known symbols are shown while pending");
    }

    #[test]
    fn the_structure_of_the_command_is_read_into_the_window() {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let path = PathBuf::from("/p/main.go");
        let window = StructureWindow::default();
        let structure = read(&registry, &mut session, &path, window.params(&path));
        assert_eq!(structure.path, path);
        assert_eq!(structure.status, StructureStatus::Unavailable);
        assert!(structure.message.is_some());
    }

    #[test]
    fn the_symbols_are_a_tree_for_screen_readers() {
        let mut structure_window = Harness::new(structure(&PathBuf::from("/p/a.go")));
        let tree = accessibility::testing::tree_of(&structure_window.ctx, vec2(400.0, 600.0), |ui| {
            let _ = structure_window.window.show(ui, &IdePalette::zen_dark(), &ToolWindowLayout::default());
        });
        assert_eq!(accessibility::unnamed(&tree), Vec::<String>::new());
        let server = accessibility::testing::find(&tree, egui::Role::TreeItem, "Server").unwrap();
        assert_eq!((server.level(), server.is_expanded(), server.description()), (Some(1), Some(true), Some("detail")));
        let field = accessibility::testing::find(&tree, egui::Role::TreeItem, "addr").unwrap();
        assert_eq!((field.level(), field.is_expanded()), (Some(2), None));
        assert!(accessibility::testing::find(&tree, egui::Role::Heading, &t!("ide-structure-title")).is_some());
    }
}
