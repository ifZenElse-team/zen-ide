use std::path::PathBuf;
use std::time::Duration;

use egui::{Align2, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_modules::search::{SearchItem, SearchTarget};
use zen_workbench::language::usages::UsagesState;
use zen_workbench::search_everywhere::FoundEverywhere;

use super::find_results::ResultsTab;
use super::glyphs::glyph_minus;
use super::refactoring_preview::PreviewTab;
use super::{IdePalette, IdeView, hover, metrics};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;

#[derive(Debug)]
pub(super) enum FindContent {
    RefactoringPreview(Box<PreviewTab>),
    Everywhere(Box<EverywhereTab>),
    Results(Box<ResultsTab>),
}

#[derive(Debug, Default)]
pub(super) struct EverywhereTab {
    found: FoundEverywhere,
    selected: Option<usize>,
    reveal: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum FindAction {
    Hide,
    GoTo { path: PathBuf, offset: usize, focus_editor: bool },
    Open { path: PathBuf, line: Option<u32>, column: Option<u32>, focus_editor: bool },
    SelectFolder(PathBuf),
    FocusEditor,
    Refactor { command: String, params: serde_json::Value },
    CancelRefactoring { command: String, params: serde_json::Value },
    Command(&'static str, serde_json::Value),
    Run(String, serde_json::Value),
    Settings(u64),
    ToggleNewTab,
}

impl FindAction {
    pub(super) fn command(&self) -> Option<(&str, serde_json::Value)> {
        match self {
            Self::GoTo { path, offset, .. } => Some(("navigate.place.goTo", json!({ "path": path, "offset": offset }))),
            Self::Refactor { command, params } | Self::CancelRefactoring { command, params } => {
                Some((command.as_str(), params.clone()))
            }
            Self::Command(command, params) => Some((command, params.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct FindWindow {
    tabs: Vec<FindContent>,
    active: usize,
    focus_next: bool,
    everywhere_seen: u64,
    pub(super) results_seen: Option<u64>,
    pub(super) asking: Option<u64>,
    pub(super) new_tab: bool,
    pub(super) usages_new_tab: bool,
}

impl FindWindow {
    pub(super) fn sync_results(&mut self, tabs: &[(u64, Option<u64>)], active: Option<u64>) -> bool {
        let known = |id: u64, list: &[FindContent]| {
            list.iter().any(|content| matches!(content, FindContent::Results(tab) if tab.id == id))
        };
        let mut fresh = None;
        let mut kept = Vec::with_capacity(self.tabs.len());
        for content in std::mem::take(&mut self.tabs) {
            match content {
                FindContent::Results(mut tab) if !tabs.iter().any(|(id, _)| *id == tab.id) => {
                    let Some((id, _)) = tabs.iter().find(|(_, replaces)| *replaces == Some(tab.id)) else { continue };
                    *tab = ResultsTab::new(*id);
                    fresh = fresh.or(Some(*id));
                    kept.push(FindContent::Results(tab));
                }
                FindContent::Results(mut tab) => {
                    tab.stale = true;
                    kept.push(FindContent::Results(tab));
                }
                other => kept.push(other),
            }
        }
        for (id, _) in tabs {
            if !known(*id, &kept) {
                kept.push(FindContent::Results(Box::new(ResultsTab::new(*id))));
                fresh = Some(*id);
            }
        }
        self.tabs = kept;
        let position = |id: u64, list: &[FindContent]| {
            list.iter().position(|content| matches!(content, FindContent::Results(tab) if tab.id == id))
        };
        let shown = fresh.filter(|id| Some(*id) == active).and_then(|id| position(id, &self.tabs));
        if let Some(index) = shown {
            self.active = index;
            self.focus_next = true;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        shown.is_some()
    }

    pub(super) fn results_tabs(&mut self) -> impl Iterator<Item = &mut ResultsTab> {
        self.tabs.iter_mut().filter_map(|content| match content {
            FindContent::Results(tab) => Some(tab.as_mut()),
            _ => None,
        })
    }

    pub(super) fn active_results(&mut self) -> Option<&mut ResultsTab> {
        match self.tabs.get_mut(self.active) {
            Some(FindContent::Results(tab)) => Some(tab.as_mut()),
            _ => None,
        }
    }

    pub(super) fn results_focused(&self, ctx: &egui::Context) -> bool {
        matches!(self.tabs.get(self.active), Some(FindContent::Results(tab)) if tab.has_focus(ctx))
    }

    pub(super) fn update_everywhere(&mut self, found: Option<&FoundEverywhere>) -> bool {
        let Some(found) = found.filter(|found| found.search > self.everywhere_seen) else { return false };
        self.everywhere_seen = found.search;
        let selected = found.groups.iter().any(|group| !group.items.is_empty()).then_some(0);
        self.tabs.push(FindContent::Everywhere(Box::new(EverywhereTab {
            found: found.clone(),
            selected,
            reveal: true,
        })));
        self.active = self.tabs.len() - 1;
        self.focus_next = true;
        true
    }

    pub(super) fn show_refactoring(&mut self, tab: PreviewTab) {
        let index = self.tabs.iter().position(|tab| matches!(tab, FindContent::RefactoringPreview(_)));
        match index {
            Some(index) => {
                self.tabs[index] = FindContent::RefactoringPreview(Box::new(tab));
                self.active = index;
            }
            None => {
                self.tabs.push(FindContent::RefactoringPreview(Box::new(tab)));
                self.active = self.tabs.len() - 1;
            }
        }
        self.focus_next = true;
    }

    pub(super) fn close_refactoring(&mut self) -> bool {
        self.tabs.retain(|tab| !matches!(tab, FindContent::RefactoringPreview(_)));
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.tabs.is_empty()
    }

    #[cfg(test)]
    pub(super) fn refactoring(&self) -> Option<&PreviewTab> {
        self.tabs.iter().find_map(|tab| match tab {
            FindContent::RefactoringPreview(tab) => Some(tab.as_ref()),
            FindContent::Everywhere(_) | FindContent::Results(_) => None,
        })
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Vec<FindAction> {
        let mut actions = Vec::new();
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-find-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("find-title"), title, || t!("ide-find-title"));
        let mut x = title.right() + 14.0;
        for (index, content) in self.tabs.iter().enumerate() {
            let galley = ui.painter().layout_no_wrap(content.title(), theme::regular(13.0), p.text);
            let tab = Rect::from_min_size(pos2(x, header.center().y - 13.0), vec2(galley.size().x + 20.0, 26.0));
            let response = ui.interact(tab, ui.scope_id().with(("find-tab", index)), Sense::click());
            accessibility::item(&response, egui::Role::Tab, self.active == index, galley.text());
            if self.active == index {
                ui.painter().rect_filled(tab, 6, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(tab, 6, p.hover);
            }
            ui.painter().galley(tab.center() - galley.size() / 2.0, galley, p.text);
            if response.clicked() {
                self.active = index;
                if let FindContent::Results(tab) = content {
                    actions.push(FindAction::Command("search.results.select", json!({ "tab": tab.id })));
                }
            }
            x = tab.right() + 6.0;
        }
        if let Some(summary) = self.tabs.get(self.active).and_then(FindContent::summary) {
            ui.painter().with_clip_rect(header.shrink2(vec2(40.0, 0.0))).text(
                pos2(x + 8.0, header.center().y),
                Align2::LEFT_CENTER,
                summary,
                theme::regular(12.5),
                p.text_secondary,
            );
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("find-hide"), Sense::click());
        accessibility::button_with(&response, || t!("a11y-hide"));
        hover(ui, &response, p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        if response.clicked() {
            actions.push(FindAction::Hide);
        }
        match self.tabs.get_mut(self.active) {
            Some(FindContent::RefactoringPreview(tab)) => tab.show(ui, p, &mut self.focus_next, &mut actions),
            Some(FindContent::Results(tab)) => {
                tab.show(ui, p, &mut self.focus_next, (self.new_tab, self.usages_new_tab), &mut actions)
            }
            Some(FindContent::Everywhere(tab)) => tab.show(ui, p, &mut self.focus_next, &mut actions),
            None => message(ui, p, t!("ide-find-empty")),
        }
        actions
    }
}

impl FindContent {
    fn title(&self) -> String {
        match self {
            Self::RefactoringPreview(tab) => tab.title(),
            Self::Everywhere(tab) => {
                t!("ide-find-se-title", tab = t!(&tab.found.title), query = tab.found.query.clone())
            }
            Self::Results(tab) => tab.title(),
        }
    }

    fn summary(&self) -> Option<String> {
        match self {
            Self::RefactoringPreview(tab) => Some(tab.summary()),
            Self::Results(tab) => tab.summary(),
            Self::Everywhere(tab) => Some(t!("ide-find-se-count", count = tab.items().count())),
        }
    }
}

impl EverywhereTab {
    fn items(&self) -> impl Iterator<Item = &SearchItem> {
        self.found.groups.iter().flat_map(|group| &group.items)
    }

    fn open(&self, index: usize, focus_editor: bool) -> Option<FindAction> {
        match &self.items().nth(index)?.target {
            SearchTarget::File { path, line, column } => {
                Some(FindAction::Open { path: path.clone(), line: *line, column: *column, focus_editor })
            }
            SearchTarget::Place { path, offset, .. } => {
                Some(FindAction::GoTo { path: path.clone(), offset: *offset, focus_editor })
            }
            SearchTarget::Directory { path } => Some(FindAction::SelectFolder(path.clone())),
            _ => None,
        }
    }

    fn show(&mut self, ui: &mut Ui, p: &IdePalette, focus: &mut bool, actions: &mut Vec<FindAction>) {
        let count = self.items().count();
        if count == 0 {
            message(ui, p, t!("ide-find-none"));
            return;
        }
        let id = ui.make_persistent_id("find-everywhere-list");
        let area = ui.interact(ui.available_rect_before_wrap(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::List, || t!("ide-find-title"));
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
            let none = Modifiers::NONE;
            let (up, down, enter, escape) = ui.input_mut(|input| {
                (
                    input.consume_key(none, Key::ArrowUp),
                    input.consume_key(none, Key::ArrowDown),
                    input.consume_key(none, Key::Enter),
                    input.consume_key(none, Key::Escape),
                )
            });
            let at = self.selected.unwrap_or(0);
            if up || down {
                self.selected = Some(if up { at.saturating_sub(1) } else { (at + 1).min(count - 1) });
                self.reveal = true;
            }
            if enter {
                actions.extend(self.open(at, true));
            }
            if escape {
                actions.push(FindAction::FocusEditor);
            }
        }
        let headers = self.found.tab == zen_workbench::search_everywhere::ALL;
        let mut clicked = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut index = 0;
            for group in &self.found.groups {
                if headers && !group.items.is_empty() {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    ui.painter().text(
                        pos2(rect.left() + 12.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        t!(&group.title),
                        theme::semibold(12.5),
                        p.text_secondary,
                    );
                }
                for item in &group.items {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let selected = self.selected == Some(index);
                    if selected {
                        ui.painter().rect_filled(
                            rect.shrink2(vec2(4.0, 0.0)),
                            4,
                            if focused { p.selection } else { p.hover },
                        );
                        if std::mem::take(&mut self.reveal) {
                            response.scroll_to_me(None);
                        }
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 4, p.hover);
                    }
                    accessibility::item(&response, egui::Role::ListItem, selected, &item.name);
                    super::search_everywhere::item_row(ui, rect.shrink2(vec2(8.0, 0.0)), item, p);
                    if response.clicked() || response.double_clicked() {
                        clicked = Some((index, response.double_clicked()));
                    }
                    index += 1;
                }
            }
        });
        if let Some((index, double)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(id));
            self.selected = Some(index);
            actions.extend(self.open(index, double));
        }
    }
}

fn message(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(egui::RichText::new(text).color(p.text_secondary));
    });
}

impl IdeView {
    pub(super) fn follow_usages(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let pending = session.state::<UsagesState>().is_some_and(|state| state.pending);
        let classifying =
            session.state::<zen_workbench::find_results::FindResults>().is_some_and(|results| results.classifying());
        if !pending && !classifying {
            return;
        }
        let _ = registry.dispatch(session, "navigate.usages.list", json!({}));
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    pub(super) fn follow_everywhere(&mut self, session: &Session) {
        if self.find_window.update_everywhere(session.state::<FoundEverywhere>()) {
            self.show_tool("find");
        }
    }

    pub(super) fn find_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        self.find_window.new_tab = self.find_in_files.new_tab();
        self.find_window.usages_new_tab = zen_workbench::find_results::FindUsagesOptions::of(session).new_tab;
        for action in self.find_window.show(ui, &palette) {
            if let Some((command, params)) = action.command() {
                self.dispatch(ui, registry, session, command, params);
            }
            match action {
                FindAction::Hide => self.hide_tool("find"),
                FindAction::GoTo { focus_editor: true, .. } | FindAction::FocusEditor => self.editor_view.focus(),
                FindAction::GoTo { .. } | FindAction::Command(..) => {}
                FindAction::Run(command, params) => self.dispatch(ui, registry, session, &command, params),
                FindAction::Open { path, line, column, focus_editor } => {
                    self.dispatch(ui, registry, session, "file.document.open", json!({ "path": path }));
                    if let Some(line) = line {
                        self.dispatch(
                            ui,
                            registry,
                            session,
                            "editor.caret.goTo",
                            json!({ "line": line, "column": column }),
                        );
                    }
                    if focus_editor {
                        self.editor_view.focus();
                    }
                }
                FindAction::SelectFolder(path) => {
                    self.run_action(ui, registry, session, "view.project.select", json!({ "paths": [path] }));
                }
                FindAction::Settings(tab) => self.find_settings(ui, registry, session, tab),
                FindAction::ToggleNewTab => {
                    let on = !self.find_in_files.new_tab();
                    self.find_in_files.set_new_tab(on);
                }
                FindAction::Refactor { .. } | FindAction::CancelRefactoring { .. } => {
                    if self.find_window.close_refactoring() {
                        self.hide_tool("find");
                    }
                    self.editor_view.focus();
                }
            }
        }
    }
}
