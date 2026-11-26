use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, UiBuilder, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::groups::GroupId;
use zen_workbench::workspaces::{InnerTab, SavedWorkspace, Workspaces};

use crate::accessibility;

use super::editor_groups::{self, TabRow};
use super::glyphs::{Glyph, glyph_branch, glyph_database, glyph_merge_conflict, glyph_rebase, glyph_square};
use super::{IdePalette, IdeView, customization, metrics, tab_strip, theme};

const GIT: &str = zen_workbench::vcs::workspace::WORKSPACE;

const SLIDE_SECONDS: f64 = 0.15;

pub trait WorkspaceView: Send {
    fn toolbar_height(&self) -> f32 {
        0.0
    }

    fn toolbar(&mut self, _ui: &mut Ui, _palette: &IdePalette, _registry: &Registry, _session: &mut Session) {}

    fn left_panel_width(&self) -> f32 {
        0.0
    }

    fn left_panel(&mut self, _ui: &mut Ui, _palette: &IdePalette, _registry: &Registry, _session: &mut Session) {}

    fn tab(&mut self, ui: &mut Ui, palette: &IdePalette, registry: &Registry, session: &mut Session, tab: &InnerTab);

    fn tab_color(&self, _tab: &InnerTab, _palette: &IdePalette) -> Option<Color32> {
        None
    }

    fn tab_dirty(&self, _tab: &InnerTab, _session: &Session) -> bool {
        false
    }

    fn tab_mark(&self, _tab: &InnerTab, _palette: &IdePalette) -> Option<(Color32, String)> {
        None
    }

    fn panel_sizes(&self) -> BTreeMap<String, f32> {
        BTreeMap::new()
    }

    fn restore_panel_sizes(&mut self, _sizes: &BTreeMap<String, f32>) {}

    fn popups(&self) -> Vec<String> {
        Vec::new()
    }

    fn text_panel_height(
        &mut self,
        _tab: &InnerTab,
        _registry: &Registry,
        _session: &mut Session,
        _height: f32,
    ) -> f32 {
        0.0
    }

    fn text_panel(
        &mut self,
        _ui: &mut Ui,
        _palette: &IdePalette,
        _registry: &Registry,
        _session: &mut Session,
        _tab: &InnerTab,
    ) {
    }

    fn text_extras(&mut self, _tab: &InnerTab, _registry: &Registry, _session: &mut Session) -> Option<TextExtras> {
        None
    }

    fn action(&mut self, _action: &str) -> bool {
        false
    }

    fn takes_key(&self, _focused: Option<egui::Id>, _key: egui::Key, _modifiers: egui::Modifiers) -> bool {
        false
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextExtras {
    pub hints: Vec<(usize, String, Option<String>, bool)>,
    pub band: Option<(usize, usize)>,
    pub frame: Option<Color32>,
    pub flash: Option<(usize, usize, f32)>,
}

struct Pane<'a> {
    workspace: (&'a str, &'a str),
    group: GroupId,
    split: bool,
    focused: bool,
    active: Option<String>,
}

#[derive(Default)]
struct PaneOutcome {
    text: bool,
    dragged: Option<(InnerTab, egui::Pos2, bool)>,
}

struct Slide {
    id: String,
    expanding: bool,
    start: f64,
}

#[derive(Default)]
pub(super) struct WorkspaceHost {
    views: HashMap<String, Box<dyn WorkspaceView>>,
    rows: HashMap<String, TabRow>,
    panes: Vec<(GroupId, Rect)>,
    strip: Option<Rect>,
    shown: Option<String>,
    slide: Option<Slide>,
    covered: bool,
    last_front: (Option<usize>, u64),
    started: bool,
    tool_keyboard: bool,
    last: Option<String>,
    first_lines: HashMap<PathBuf, usize>,
    scroll_to: HashMap<PathBuf, usize>,
    panels: HashMap<String, BTreeMap<String, f32>>,
}

impl WorkspaceHost {
    pub(super) fn with_features() -> Self {
        let mut host = Self::default();
        let database: Box<dyn WorkspaceView> = Box::new(super::database_view::DatabaseView::default());
        host.views.insert(zen_workbench::database::WORKSPACE.to_owned(), database);
        host
    }

    fn follow(&mut self, expanded: Option<String>, now: f64, reduce_motion: bool) -> Option<(String, bool, f32)> {
        if !std::mem::replace(&mut self.started, true) {
            self.shown = expanded.clone();
        }
        if expanded != self.shown {
            if let Some(id) = &expanded {
                self.tool_keyboard = false;
                self.last = Some(id.clone());
            }
            self.slide = match (&self.shown, &expanded) {
                (None, Some(id)) => Some(Slide { id: id.clone(), expanding: true, start: now }),
                (Some(id), None) => Some(Slide { id: id.clone(), expanding: false, start: now }),
                _ => None,
            }
            .filter(|_| !reduce_motion);
            self.shown = expanded;
        }
        let slide = self.slide.as_ref()?;
        let progress = ((now - slide.start) / SLIDE_SECONDS) as f32;
        if progress >= 1.0 {
            self.slide = None;
            return None;
        }
        Some((slide.id.clone(), slide.expanding, progress.max(0.0)))
    }
}

pub(super) fn workspace_glyph(icon: &str) -> Glyph {
    match icon {
        "database" => glyph_database,
        "branch" => glyph_branch,
        _ => square,
    }
}

pub(super) fn workspace_glyph_color(icon: &str, p: &IdePalette) -> Color32 {
    if icon == "branch" { p.vcs_current_branch } else { p.text_secondary }
}

fn square(painter: &egui::Painter, rect: Rect, color: Color32) {
    glyph_square(painter, rect.shrink(2.0), color);
}

fn ease(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

impl IdeView {
    pub fn register_workspace_view(&mut self, workspace: &str, mut view: Box<dyn WorkspaceView>) {
        if let Some(sizes) = self.workspace_host.panels.get(workspace) {
            view.restore_panel_sizes(sizes);
        }
        self.workspace_host.views.insert(workspace.to_owned(), view);
    }

    pub(super) fn workspace_has_keyboard(&self, session: &Session) -> bool {
        !self.workspace_host.tool_keyboard && zen_workbench::workspaces::expanded(session).is_some()
    }

    pub(super) fn tool_window_took_keyboard(&mut self) {
        self.workspace_host.tool_keyboard = true;
    }

    pub(super) fn workspace_window_state(&self, session: &Session) -> (serde_json::Value, Vec<String>) {
        let expanded = zen_workbench::workspaces::expanded(session);
        let Some(workspaces) = session.state::<Workspaces>() else { return (serde_json::Value::Null, Vec::new()) };
        let id =
            expanded.clone().or_else(|| self.workspace_host.last.clone().filter(|id| workspaces.info(id).is_some()));
        let Some(id) = id else { return (serde_json::Value::Null, Vec::new()) };
        let active = workspaces.active_tab(&id).map(|tab| {
            let mut shown = json!({ "id": tab.id, "kind": tab.kind, "title": tab.title });
            if let Some(path) = &tab.path {
                shown["path"] = json!(path);
            }
            shown
        });
        let open = expanded.as_deref() == Some(id.as_str());
        let popups = match self.workspace_host.views.get(&id) {
            Some(view) if open => view.popups(),
            _ => Vec::new(),
        };
        (json!({ "id": id, "expanded": open, "activeTab": active }), popups)
    }

    pub(super) fn saved_workspaces(&self, session: &mut Session) -> Vec<SavedWorkspace> {
        let mut saved = zen_workbench::workspaces::saved(session);
        for workspace in &mut saved {
            for tab in &mut workspace.tabs {
                let Some(path) = &tab.path else { continue };
                let line =
                    self.workspace_host.first_lines.get(path).or_else(|| self.workspace_host.scroll_to.get(path));
                if let Some(line) = line {
                    tab.first_line = Some(*line);
                }
            }
            let sizes = match self.workspace_host.views.get(&workspace.id) {
                Some(view) => view.panel_sizes(),
                None => self.workspace_host.panels.get(&workspace.id).cloned().unwrap_or_default(),
            };
            if !sizes.is_empty() {
                workspace.panels = sizes;
            }
        }
        saved
    }

    pub(super) fn restore_workspaces(&mut self, session: &mut Session, saved: &[SavedWorkspace]) {
        for workspace in saved {
            for tab in &workspace.tabs {
                if let (Some(path), Some(line)) = (&tab.path, tab.first_line) {
                    self.workspace_host.scroll_to.insert(path.clone(), line);
                }
            }
            if !workspace.panels.is_empty() {
                if let Some(view) = self.workspace_host.views.get_mut(&workspace.id) {
                    view.restore_panel_sizes(&workspace.panels);
                }
                self.workspace_host.panels.insert(workspace.id.clone(), workspace.panels.clone());
            }
            if workspace.expanded {
                self.workspace_host.last = Some(workspace.id.clone());
            }
        }
        zen_workbench::workspaces::restore(session, saved.to_vec());
    }

    pub(super) fn editor_covered(&mut self, session: &Session) -> bool {
        let covered = zen_workbench::workspaces::expanded(session).is_some()
            || self.workspace_host.shown.is_some()
            || self.workspace_host.slide.is_some();
        if covered {
            self.workspace_host.covered = true;
            return true;
        }
        let front = Self::front_document(session);
        let before = self.workspace_host.last_front;
        if std::mem::take(&mut self.workspace_host.covered) && front.0 == before.0 {
            self.move_front(before, front);
        }
        self.workspace_host.last_front = front;
        false
    }

    pub(super) fn workspace_area(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) -> Option<bool> {
        zen_workbench::workspaces::sync(session);
        let expanded = zen_workbench::workspaces::expanded(session);
        let now = ui.input(|input| input.time);
        let Some((id, expanding, progress)) =
            self.workspace_host.follow(expanded.clone(), now, accessibility::reduce_motion())
        else {
            let id = expanded?;
            let area = ui.available_rect_before_wrap();
            if ui.input(|input| {
                input.pointer.any_pressed() && input.pointer.interact_pos().is_some_and(|pos| area.contains(pos))
            }) {
                self.workspace_host.tool_keyboard = false;
            }
            self.follow_groups(ui.ctx(), ui.scope_id(), session);
            self.groups_ui.workspace_tabs.clear();
            let area = ui.available_rect_before_wrap();
            let body = self.workspace_tab_strip(ui, registry, session, &id, area);
            let mut child = ui.new_child(UiBuilder::new().id_salt(("workspace-host", &id)).max_rect(body));
            let text = self.workspace_host_ui(&mut child, registry, session, &id, true);
            ui.advance_cursor_after_rect(area);
            let panes = std::mem::take(&mut self.workspace_host.panes);
            self.dragged_tab(ui, registry, session, &panes, None);
            return Some(text);
        };
        let area = ui.available_rect_before_wrap();
        self.groups_ui.inert = true;
        let text = self.split_editors(ui, registry, session);
        self.groups_ui.inert = false;
        let _ = ui.interact(area, ui.scope_id().with("workspace-slide"), Sense::click_and_drag());
        let shift = if expanding { 1.0 - ease(progress) } else { ease(progress) };
        let mut area = area;
        area.min.y += self.workspace_strip_height();
        let rect = area.translate(vec2(shift * area.width(), 0.0));
        let mut child = ui.new_child(UiBuilder::new().id_salt(("workspace-host", &id)).max_rect(rect));
        child.set_clip_rect(area.intersect(ui.clip_rect()));
        let host_text = self.workspace_host_ui(&mut child, registry, session, &id, expanding);
        child.painter().vline(rect.left(), rect.y_range(), Stroke::new(1.0, self.palette.frame));
        ui.ctx().request_repaint();
        Some(if expanding { host_text } else { text })
    }

    fn workspace_strip_height(&self) -> f32 {
        if self.appearance.hides_chrome() { 0.0 } else { metrics::TABS }
    }

    fn workspace_tab_strip(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        id: &str,
        area: Rect,
    ) -> Rect {
        let height = self.workspace_strip_height();
        self.workspace_host.strip = None;
        if height <= 0.0 {
            return area;
        }
        let strip = Rect::from_min_size(area.min, vec2(area.width(), height));
        self.workspace_host.strip = Some(strip);
        let main = session.state::<Workbench>().map_or(1, |workbench| workbench.groups().main_focused());
        let mut child = ui.new_child(UiBuilder::new().id_salt(("workspace-strip", id)).max_rect(strip));
        self.workspace_strip(&mut child, registry, session, main, id);
        Rect::from_min_max(egui::pos2(area.left(), strip.bottom()), area.max)
    }

    fn workspace_host_ui(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        id: &str,
        focused: bool,
    ) -> bool {
        let p = self.palette;
        let area = ui.max_rect();
        ui.painter().rect_filled(area, 0, p.island);
        let Some(workspaces) = session.state::<Workspaces>() else { return false };
        let Some(info) = workspaces.info(id).cloned() else { return false };
        let tabs = workspaces.tabs(id).to_vec();
        let region = ui.interact(area, ui.scope_id().with("workspace-region"), Sense::hover());
        accessibility::name_with(&region, egui::Role::Pane, || t!("a11y-workspace", title = info.title.as_str()));
        let mut view = self.workspace_host.views.remove(id);
        let toolbar = view.as_ref().map_or(0.0, |view| view.toolbar_height());
        let left = view.as_ref().map_or(0.0, |view| view.left_panel_width()).min(area.width() / 2.0);
        let mut body = area;
        if toolbar > 0.0 {
            let bar = Rect::from_min_size(area.min, vec2(area.width(), toolbar));
            body.min.y = bar.bottom();
            let mut child = ui.new_child(UiBuilder::new().id_salt("workspace-toolbar").max_rect(bar));
            if let Some(view) = view.as_mut() {
                view.toolbar(&mut child, &p, registry, session);
            }
            ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        }
        if left > 0.0 {
            let panel = Rect::from_min_size(body.min, vec2(left, body.height()));
            body.min.x = panel.right() + 1.0;
            let mut child = ui.new_child(UiBuilder::new().id_salt("workspace-left").max_rect(panel));
            if let Some(view) = view.as_mut() {
                view.left_panel(&mut child, &p, registry, session);
            }
            ui.painter().vline(panel.right() + 0.5, panel.y_range(), Stroke::new(1.0, p.frame));
        }
        let mut right = ui.new_child(UiBuilder::new().id_salt("workspace-tabs").max_rect(body));
        let text =
            self.workspace_tabs(&mut right, registry, session, (&info.id, &info.icon), &tabs, focused, view.as_mut());
        if let Some(view) = view {
            self.workspace_host.views.insert(id.to_owned(), view);
        }
        text
    }

    #[allow(clippy::too_many_arguments, reason = "the host's parts, passed apart so that the view can be borrowed")]
    fn workspace_tabs(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        (id, icon): (&str, &str),
        tabs: &[InnerTab],
        focused: bool,
        mut view: Option<&mut Box<dyn WorkspaceView>>,
    ) -> bool {
        let area = ui.available_rect_before_wrap();
        let (owned, keyboard) = match session.state::<Workbench>() {
            Some(workbench) => (workbench.groups().owned_of(id).cloned(), Some(workbench.groups().focused())),
            None => (None, None),
        };
        let (mut placed, mut dividers) = (Vec::new(), Vec::new());
        match owned.as_ref().filter(|owned| owned.is_split()) {
            Some(owned) => editor_groups::layout(&owned.root, area, &mut placed, &mut dividers),
            None => placed.push((owned.as_ref().map_or(0, |owned| owned.group().id), area)),
        }
        let split = placed.len() > 1;
        self.workspace_host.panes = placed.iter().filter(|(group, _)| *group != 0).copied().collect();
        let pressed = ui
            .input(|input| input.pointer.any_pressed().then(|| input.pointer.interact_pos()).flatten())
            .filter(|_| split && focused)
            .filter(|at| ui.ctx().layer_id_at(*at) == Some(ui.layer_id()))
            .filter(|at| !dividers.iter().any(|divider| editor_groups::grip(divider).0.contains(*at)));
        let mut text = false;
        let mut promote = None;
        let mut dragged = None;
        for (group, rect) in &placed {
            let (shown, active) = {
                let Some(workspaces) = session.state::<Workspaces>() else { return false };
                let shown: Vec<InnerTab> = if split {
                    tabs.iter().filter(|tab| tab.group == *group).cloned().collect()
                } else {
                    tabs.to_vec()
                };
                let active = if split { workspaces.shown_in(id, *group) } else { workspaces.active_tab(id) };
                (shown, active.map(|tab| tab.id.clone()))
            };
            let pane_focused = focused && (!split || keyboard == Some(*group));
            let mut child = ui.new_child(UiBuilder::new().id_salt(("workspace-pane", *group)).max_rect(*rect));
            let pane = Pane { workspace: (id, icon), group: *group, split, focused: pane_focused, active };
            let outcome = self.workspace_pane(&mut child, registry, session, &pane, &shown, view.as_deref_mut());
            text |= outcome.text;
            if let Some((tab, at, done)) = outcome.dragged {
                dragged = Some((tab, *group, at, done));
            }
            if !pane_focused && pressed.is_some_and(|at| rect.contains(at)) {
                promote = Some(*group);
            }
        }
        ui.advance_cursor_after_rect(area);
        if split {
            self.dividers(ui, registry, session, &dividers);
        }
        if let Some((tab, from, at, done)) = dragged {
            self.inner_tab_dragged(ui, registry, session, id, (&tab, from), (at, done), &placed);
        }
        if let Some(group) = promote {
            let _ = registry.dispatch(session, "editor.split.focus", json!({ "group": group }));
        }
        text
    }

    #[allow(clippy::too_many_arguments, reason = "the drag's parts")]
    fn inner_tab_dragged(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        id: &str,
        (tab, from): (&InnerTab, GroupId),
        (at, done): (egui::Pos2, bool),
        placed: &[(GroupId, Rect)],
    ) {
        let pane = placed.iter().find(|(group, rect)| *group != from && *group != 0 && rect.contains(at)).copied();
        let main =
            self.workspace_host.strip.filter(|strip| strip.contains(at) && tab.path.is_some()).and_then(|strip| {
                session.state::<Workbench>().map(|workbench| (workbench.groups().main_focused(), strip))
            });
        let Some((target, rect)) = pane.or(main) else { return };
        if done {
            let params = json!({ "workspace": id, "tab": tab.id, "group": target });
            self.dispatch(ui, registry, session, "workspace.tab.move", params);
            return;
        }
        let painter =
            ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("inner-drop")));
        painter.rect_stroke(
            rect.shrink(1.0),
            4,
            Stroke::new(2.0, self.palette.active_tab_border),
            egui::StrokeKind::Inside,
        );
    }

    fn workspace_pane(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        pane: &Pane<'_>,
        tabs: &[InnerTab],
        mut view: Option<&mut Box<dyn WorkspaceView>>,
    ) -> PaneOutcome {
        let p = self.palette;
        let ((id, icon), group, focused) = (pane.workspace, pane.group, pane.focused);
        let mut done = PaneOutcome::default();
        let active_index = pane.active.as_ref().and_then(|active| tabs.iter().position(|tab| tab.id == *active));
        let dirty = |tab: &InnerTab| {
            tab.path.as_ref().is_some_and(|path| {
                session.state::<Workbench>().is_some_and(|workbench| {
                    workbench.documents().iter().any(|document| document.path() == path && document.is_dirty())
                })
            })
        };
        let shown: Vec<tab_strip::Tab<'_>> = tabs
            .iter()
            .map(|tab| {
                let dirty = dirty(tab) || view.as_ref().is_some_and(|view| view.tab_dirty(tab, session));
                let icon: tab_strip::TabIcon<'_> = match &tab.path {
                    Some(path) => Box::new(move |painter, rect| crate::file_icons::file_icon_at(painter, rect, path)),
                    None if id == GIT => git_tab_icon(&tab.kind, &p),
                    None => {
                        let glyph = workspace_glyph(icon);
                        let color = workspace_glyph_color(icon, &p);
                        Box::new(move |painter, rect| glyph(painter, rect, color))
                    }
                };
                tab_strip::Tab {
                    title: tab.title.clone(),
                    color: if dirty { p.modified_tab } else { p.text },
                    icon,
                    dirty,
                    closable: tab.closable,
                    draggable: true,
                    strip: view.as_ref().and_then(|view| view.tab_color(tab, &p)),
                    mark: view.as_ref().and_then(|view| view.tab_mark(tab, &p)),
                    pinned: false,
                    preview: false,
                }
            })
            .collect();
        let key = format!("{id}/{group}");
        let mut row = self.workspace_host.rows.remove(&key).unwrap_or_default();
        let look = tab_strip::Look::default();
        let options = tab_strip::Options { height: metrics::TABS, focused, reorder: true, look };
        let outcome = tab_strip::show(ui, &p, &mut row, &shown, active_index, options);
        drop(shown);
        self.workspace_host.rows.insert(key, row);
        if let Some((index, at)) = outcome.dragging {
            done.dragged = Some((tabs[index].clone(), at, false));
        }
        if let Some((index, at)) = outcome.dropped {
            done.dragged = Some((tabs[index].clone(), at, true));
        }
        let mut chosen = None;
        for (index, response) in outcome.responses.iter().enumerate() {
            let Some(response) = response else { continue };
            let tab = &tabs[index];
            let movable = tab.path.as_ref().is_some_and(|path| {
                session.state::<Workspaces>().is_some_and(|workspaces| workspaces.accepts(id, session, path))
            });
            response.context_menu(|ui| {
                customization::popup(
                    ui,
                    customization::WORKSPACE_TAB_POPUP,
                    |_| true,
                    |ui, _| {
                        if let Some(action) = inner_tab_menu(ui, id, tab, pane.split, movable) {
                            chosen = Some(action);
                        }
                    },
                );
            });
        }
        if let Some((command, mut params)) = chosen {
            if command.starts_with("editor.split.unsplit") {
                let _ = registry.dispatch(session, "editor.split.focus", json!({ "group": group }));
            }
            if command == "workspace.tab.move" {
                let main = session.state::<Workbench>().map_or(1, |workbench| workbench.groups().main_focused());
                params["group"] = json!(main);
            }
            if command.starts_with("view.") {
                self.run_action(ui, registry, session, command, params);
            } else {
                self.dispatch(ui, registry, session, command, params);
            }
            return done;
        }
        if let Some(index) = outcome.close {
            let params = json!({ "workspace": id, "tab": tabs[index].id });
            self.dispatch(ui, registry, session, "workspace.tab.close", params);
            return done;
        }
        if let Some((from, to)) = outcome.reorder {
            let params = json!({ "workspace": id, "tab": tabs[from].id, "to": to });
            self.dispatch(ui, registry, session, "workspace.tab.move", params);
        }
        if let Some(index) = outcome.select {
            let params = json!({ "workspace": id, "tab": tabs[index].id });
            self.dispatch(ui, registry, session, "workspace.tab.select", params);
            return done;
        }
        let area = ui.available_rect_before_wrap();
        let Some(tab) = active_index.map(|index| &tabs[index]) else {
            if focused {
                self.caret = None;
                self.close_text_dialogs();
            }
            let title = session
                .state::<Workspaces>()
                .and_then(|workspaces| workspaces.info(id))
                .map(|info| info.title.clone())
                .unwrap_or_default();
            empty(ui, area, &p, &t!("ide-workspace-empty", title = title.as_str()));
            return done;
        };
        if id == GIT && tab.path.is_none() {
            if focused {
                self.caret = None;
                self.close_text_dialogs();
            }
            self.git_inner_tab(ui, registry, session, tab);
            return done;
        }
        match (&tab.path, group) {
            (Some(_), group) if group != 0 => {
                done.text = self.workspace_file(ui, registry, session, (group, focused), view, tab);
            }
            _ => {
                if focused {
                    self.caret = None;
                    self.close_text_dialogs();
                }
                match view.as_mut() {
                    Some(view) => view.tab(ui, &p, registry, session, tab),
                    None => empty(ui, area, &p, &t!("ide-workspace-no-view", kind = tab.kind.as_str())),
                }
            }
        }
        done
    }

    fn workspace_file(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        (group, focused): (GroupId, bool),
        mut view: Option<&mut Box<dyn WorkspaceView>>,
        tab: &InnerTab,
    ) -> bool {
        if !focused {
            let area = ui.available_rect_before_wrap();
            let mut child = ui.new_child(UiBuilder::new().id_salt("workspace-text").max_rect(area));
            self.waiting_group(&mut child, registry, session, group, true);
            ui.advance_cursor_after_rect(area);
            return false;
        }
        let area = ui.available_rect_before_wrap();
        let panel = view
            .as_mut()
            .map_or(0.0, |view| view.text_panel_height(tab, registry, session, area.height()))
            .clamp(0.0, area.height());
        let extras = view.as_mut().and_then(|view| view.text_extras(tab, registry, session));
        let frame = extras.as_ref().and_then(|extras| extras.frame);
        let path = session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
        let requested = path.as_ref().and_then(|path| self.workspace_host.scroll_to.remove(path));
        if let Some(line) = requested {
            self.editor_view.scroll_to_line(line);
        }
        let editor = Rect::from_min_max(area.min, egui::pos2(area.right(), area.bottom() - panel));
        let mut text = false;
        if editor.height() >= 1.0 {
            self.editor_view.set_extras(path.clone().zip(extras));
            let mut child = ui.new_child(UiBuilder::new().id_salt("workspace-text").max_rect(editor));
            text = self.group_text(&mut child, registry, session, group, true);
            self.editor_view.set_extras(None);
        }
        if panel > 0.0 {
            let below = Rect::from_min_max(egui::pos2(area.left(), area.bottom() - panel), area.max);
            let mut child = ui.new_child(UiBuilder::new().id_salt("workspace-text-panel").max_rect(below));
            if let Some(view) = view.as_mut() {
                let p = self.palette;
                view.text_panel(&mut child, &p, registry, session, tab);
            }
        }
        if let Some(color) = frame {
            ui.painter().rect_stroke(area, 0, Stroke::new(2.0, color), egui::StrokeKind::Inside);
        }
        ui.advance_cursor_after_rect(area);
        if let Some(path) = path {
            let line = requested.unwrap_or_else(|| self.editor_view.first_shown_line());
            self.workspace_host.first_lines.insert(path, line);
        }
        text
    }

    pub(super) fn workspace_view_action(&mut self, session: &Session, action: &str) -> bool {
        let Some(id) = zen_workbench::workspaces::expanded(session) else { return false };
        self.workspace_host.views.get_mut(&id).is_some_and(|view| view.action(action))
    }

    pub(super) fn workspace_takes_key(
        &self,
        focused: Option<egui::Id>,
        expanded: Option<&str>,
        key: egui::Key,
        modifiers: egui::Modifiers,
    ) -> bool {
        let Some(id) = expanded else { return false };
        self.workspace_host.views.get(id).is_some_and(|view| view.takes_key(focused, key, modifiers))
    }
}

fn git_tab_icon<'a>(kind: &str, p: &'a IdePalette) -> tab_strip::TabIcon<'a> {
    match kind {
        zen_workbench::vcs::workspace::DIFF_TAB => {
            Box::new(move |painter, rect| super::diff_view::diff_icon(painter, rect, p))
        }
        zen_workbench::vcs::workspace::REBASE_TAB => {
            Box::new(move |painter, rect| glyph_rebase(painter, rect.shrink(0.5), p.vcs_current_branch))
        }
        zen_workbench::vcs::workspace::CONFLICT_TAB => {
            Box::new(move |painter, rect| glyph_merge_conflict(painter, rect.shrink(0.5), p.vcs_conflict))
        }
        _ => Box::new(move |painter, rect| glyph_branch(painter, rect.shrink(0.5), p.vcs_current_branch)),
    }
}

fn inner_tab_menu(
    ui: &mut Ui,
    workspace: &str,
    tab: &InnerTab,
    split: bool,
    movable: bool,
) -> Option<(&'static str, serde_json::Value)> {
    let mut chosen = None;
    let close = json!({ "workspace": workspace, "tab": tab.id });
    if tab.closable && ui.button(t!("keymap-file-document-close")).clicked() {
        chosen = Some(("workspace.tab.close", close));
    }
    ui.separator();
    let items = [
        ("keymap-editor-split-right", "right", false),
        ("keymap-editor-split-down", "down", false),
        ("keymap-editor-split-right-move", "right", true),
        ("keymap-editor-split-down-move", "down", true),
    ];
    for (label, direction, moved) in items {
        if moved && tab.path.is_none() {
            continue;
        }
        if ui.button(t!(label)).clicked() {
            let params = json!({ "workspace": workspace, "tab": tab.id, "direction": direction, "move": moved });
            chosen = Some(("workspace.tab.split", params));
        }
    }
    if split {
        ui.separator();
        if ui.button(t!("keymap-editor-split-unsplit")).clicked() {
            chosen = Some(("editor.split.unsplit", json!({})));
        }
        if ui.button(t!("keymap-editor-split-unsplitAll")).clicked() {
            chosen = Some(("editor.split.unsplitAll", json!({})));
        }
    }
    if movable {
        ui.separator();
        if ui.button(t!("ide-workspace-move-to-editor")).clicked() {
            chosen = Some(("workspace.tab.move", json!({ "workspace": workspace, "tab": tab.id })));
        }
    }
    if let Some(path) = &tab.path {
        ui.separator();
        if ui.button(t!("keymap-view-localHistory-show")).clicked() {
            chosen = Some(("view.localHistory.show", json!({ "path": path })));
        }
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

fn empty(ui: &Ui, area: Rect, p: &IdePalette, text: &str) {
    ui.painter().text(area.center(), Align2::CENTER_CENTER, text, theme::regular(13.0), p.text_secondary);
}

#[cfg(test)]
mod tests;
