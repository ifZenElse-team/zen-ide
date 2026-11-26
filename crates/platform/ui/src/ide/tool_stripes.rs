use std::path::Path;

use egui::{CornerRadius, CursorIcon, LayerId, Order, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::agent::ActionError;
use super::glyphs::*;
use super::tool_windows::{Anchor, Direction, STRETCH_STEP, Side, ToolWindowLayout, ViewMode};
use super::{IdeView, customization, hint, hover, in_development, metrics};

pub(super) struct Kind {
    pub(super) id: &'static str,
    pub(super) glyph: Glyph,
    pub(super) title: &'static str,
    pub(super) action: &'static str,
}

pub(super) const KINDS: [Kind; 23] = [
    Kind { id: "project", glyph: glyph_folder, title: "ide-project-title", action: "view.toolWindow.project" },
    Kind { id: "commit", glyph: glyph_commit, title: "ide-tool-commit", action: "view.toolWindow.commit" },
    Kind { id: "bookmarks", glyph: glyph_bookmark, title: "ide-tool-bookmarks", action: "view.toolWindow.bookmarks" },
    Kind { id: "structure", glyph: glyph_structure, title: "ide-tool-structure", action: "view.toolWindow.structure" },
    Kind { id: "find", glyph: glyph_search, title: "ide-find-title", action: "view.toolWindow.find" },
    Kind { id: "debug", glyph: glyph_bug, title: "ide-tool-debug", action: "view.toolWindow.debug" },
    Kind { id: "run", glyph: glyph_run_window, title: "ide-tool-run", action: "view.toolWindow.run" },
    Kind { id: "terminal", glyph: glyph_terminal, title: "ide-terminal-title", action: "view.toolWindow.terminal" },
    Kind { id: "problems", glyph: glyph_problems, title: "ide-tool-problems", action: "view.toolWindow.problems" },
    Kind { id: "git", glyph: glyph_branch, title: "ide-tool-git", action: "view.toolWindow.git" },
    Kind { id: "services", glyph: glyph_services, title: "ide-tool-services", action: "view.toolWindow.services" },
    Kind { id: "hierarchy", glyph: glyph_hierarchy, title: "ide-hierarchy-title", action: "view.toolWindow.hierarchy" },
    Kind { id: "todo", glyph: glyph_todo, title: "ide-todo-title", action: "view.toolWindow.todo" },
    Kind { id: "caffeine", glyph: glyph_caffeine, title: "ide-caffeine-title", action: "view.toolWindow.caffeine" },
    Kind { id: "maven", glyph: glyph_maven, title: "ide-maven-title", action: "view.toolWindow.maven" },
    Kind { id: "gradle", glyph: glyph_gradle, title: "ide-gradle-title", action: "view.toolWindow.gradle" },
    Kind { id: "build", glyph: glyph_build_project, title: "ide-tool-build", action: "view.toolWindow.build" },
    Kind { id: "ant", glyph: glyph_ant, title: "ide-ant-title", action: "view.toolWindow.ant" },
    Kind { id: "npm", glyph: glyph_npm, title: "ide-npm-title", action: "view.toolWindow.npm" },
    Kind { id: "endpoints", glyph: glyph_endpoints, title: "ide-endpoints-title", action: "view.toolWindow.endpoints" },
    Kind { id: "coverage", glyph: glyph_coverage, title: "ide-coverage-title", action: "view.toolWindow.coverage" },
    Kind {
        id: "dependencies",
        glyph: glyph_dependencies,
        title: "ide-deps-title",
        action: "view.toolWindow.dependencies",
    },
    Kind {
        id: "notifications",
        glyph: glyph_bell,
        title: "ide-tool-notifications",
        action: "view.toolWindow.notifications",
    },
];

pub(super) fn kind(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|kind| kind.id == id).or_else(|| super::plugin_tool_window::kind(id))
}

pub(super) fn built_in(id: &str) -> bool {
    KINDS.iter().any(|kind| kind.id == id)
}

const STEP: f32 = metrics::STRIPE_BUTTON + 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Part {
    First,
    Second,
    Bottom,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Stripe {
    pub(super) rect: Rect,
    pub(super) start: f32,
    pub(super) first: Vec<f32>,
    pub(super) separator: Option<f32>,
    pub(super) second: Vec<f32>,
    pub(super) top_end: f32,
    pub(super) bottom: Vec<f32>,
}

impl Stripe {
    pub(super) fn new(rect: Rect, fixed: usize, first: usize, second: usize, bottom: usize) -> Self {
        let start = Self::first_centre(rect, fixed);
        let row = |from: f32, count: usize| (0..count).map(|index| from + index as f32 * STEP).collect::<Vec<_>>();
        let first = row(start, first);
        let mut y = start + first.len() as f32 * STEP;
        let separator = (second > 0).then(|| {
            let line = y - 13.0;
            y += 10.0;
            line
        });
        let second = row(y, second);
        y += second.len() as f32 * STEP;
        let bottom_start = rect.bottom() - 20.0 - bottom.saturating_sub(1) as f32 * STEP;
        Self { rect, start, first, separator, second, top_end: y, bottom: row(bottom_start, bottom) }
    }

    pub(super) fn first_centre(rect: Rect, fixed: usize) -> f32 {
        rect.top() + metrics::GAP + 20.0 + fixed as f32 * STEP
    }

    fn centres(&self, part: Part) -> &[f32] {
        match part {
            Part::First => &self.first,
            Part::Second => &self.second,
            Part::Bottom => &self.bottom,
        }
    }

    pub(super) fn target(&self, pos: Pos2) -> Option<(Part, usize)> {
        if !self.rect.expand2(vec2(8.0, 0.0)).contains(pos) {
            return None;
        }
        let before = |centres: &[f32]| centres.iter().filter(|centre| **centre < pos.y).count();
        if pos.y > self.rect.center().y.max(self.top_end) {
            return Some((Part::Bottom, before(&self.bottom)));
        }
        let boundary = match self.separator {
            Some(line) => line,
            None => self.first.last().copied().unwrap_or(self.start) + STEP / 2.0,
        };
        Some(if pos.y < boundary { (Part::First, before(&self.first)) } else { (Part::Second, before(&self.second)) })
    }

    pub(super) fn indicator(&self, part: Part, index: usize) -> f32 {
        let centres = self.centres(part);
        if let Some(centre) = centres.get(index) {
            return centre - STEP / 2.0;
        }
        if let Some(last) = centres.last() {
            return last + STEP / 2.0;
        }
        match part {
            Part::First => self.start - STEP / 2.0,
            Part::Second => self.separator.unwrap_or(self.first.last().copied().unwrap_or(self.start) + STEP / 2.0),
            Part::Bottom => self.rect.bottom() - 2.0,
        }
    }
}

#[derive(Default)]
pub(super) struct Stripes {
    left: Option<Stripe>,
    right: Option<Stripe>,
    dragged: Option<&'static str>,
    ids: [Vec<&'static str>; 6],
    drawn: [Option<(&'static str, Rect)>; 6],
    hidden_all: Option<ToolWindowLayout>,
    workspaces: Vec<(String, Rect)>,
}

fn side_index(side: Side) -> usize {
    Side::ALL.iter().position(|other| *other == side).unwrap_or(0)
}

impl Stripes {
    pub(super) fn on_stripe(&self, pos: Pos2) -> bool {
        [&self.left, &self.right].into_iter().flatten().any(|stripe| stripe.rect.contains(pos))
    }

    pub(super) fn drawn_at(&self, pos: Pos2) -> Option<&'static str> {
        self.drawn.iter().flatten().find(|(_, rect)| rect.contains(pos)).map(|(id, _)| *id)
    }

    pub(super) fn set_drawn(&mut self, side: Side, id: &'static str, rect: Rect) {
        self.drawn[side_index(side)] = Some((id, rect));
    }

    pub(super) fn target(&self, pos: Pos2) -> Option<(Side, usize, f32, &Stripe)> {
        let sides = |left: bool, part: Part| match (left, part) {
            (true, Part::First) => Side::LeftTop,
            (true, Part::Second) => Side::LeftBottom,
            (true, Part::Bottom) => Side::BottomLeft,
            (false, Part::First) => Side::RightTop,
            (false, Part::Second) => Side::RightBottom,
            (false, Part::Bottom) => Side::BottomRight,
        };
        [(true, self.left.as_ref()), (false, self.right.as_ref())].into_iter().find_map(|(left, stripe)| {
            let stripe = stripe?;
            let (part, index) = stripe.target(pos)?;
            Some((sides(left, part), index, stripe.indicator(part, index), stripe))
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuRequest {
    Hide,
    Move(Side),
    Stretch(Direction),
    Mode(ViewMode),
}

const MOVE_TO: [Side; 6] =
    [Side::LeftTop, Side::LeftBottom, Side::BottomLeft, Side::BottomRight, Side::RightBottom, Side::RightTop];

const STRETCHES: [(Direction, &str); 4] = [
    (Direction::Left, "view.toolWindow.resize/left"),
    (Direction::Right, "view.toolWindow.resize/right"),
    (Direction::Up, "view.toolWindow.resize/up"),
    (Direction::Down, "view.toolWindow.resize/down"),
];

pub(super) fn layout_menu(ui: &mut Ui, layout: &ToolWindowLayout, id: &str, stripe: bool) -> Option<MenuRequest> {
    let visible = layout.is_visible(id);
    let current = layout.side_of(id)?;
    let mode = layout.mode_of(id).unwrap_or_default();
    let mut request = None;
    let item = |ui: &mut Ui, text: String, action: &str| {
        let shortcut = crate::keymap::shortcut(action).unwrap_or_default();
        ui.add(egui::Button::new(text).shortcut_text(shortcut)).clicked()
    };
    if stripe && visible && item(ui, t!("ide-tool-hide"), "view.toolWindow.hide") {
        request = Some(MenuRequest::Hide);
        ui.close();
    }
    if !stripe {
        ui.menu_button(t!("ide-tool-view-mode"), |ui| {
            for choice in ViewMode::ALL {
                let title = super::keymap_page::action_title(choice.action());
                if check_item(ui, &title, choice == mode).clicked() {
                    request = Some(MenuRequest::Mode(choice));
                    ui.close();
                }
            }
        });
    }
    ui.menu_button(t!("ide-tool-move-to"), |ui| {
        for side in MOVE_TO {
            if ui.add_enabled(side != current, egui::Button::new(t!(side.message()))).clicked() {
                request = Some(MenuRequest::Move(side));
                ui.close();
            }
        }
    });
    if !stripe && visible && mode.on_side() {
        ui.menu_button(t!("ide-tool-resize"), |ui| {
            for (direction, action) in STRETCHES {
                if item(ui, super::keymap_page::action_title(action), action) {
                    request = Some(MenuRequest::Stretch(direction));
                    ui.close();
                }
            }
        });
    }
    request
}

fn check_item(ui: &mut Ui, text: &str, checked: bool) -> egui::Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let color = ui.visuals().text_color();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let padding = ui.spacing().button_padding;
    let height = (galley.size().y + 2.0 * padding.y).max(ui.spacing().interact_size.y);
    let width = (galley.size().x + 26.0 + 2.0 * padding.x).max(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if response.hovered() {
        let visuals = ui.visuals().widgets.hovered;
        ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    if checked {
        let tick = Rect::from_center_size(pos2(rect.left() + padding.x + 6.0, rect.center().y), Vec2::splat(11.0));
        glyph_check(ui.painter(), tick, color);
    }
    ui.painter().galley(pos2(rect.left() + padding.x + 20.0, rect.center().y - galley.size().y / 2.0), galley, color);
    crate::accessibility::item(&response, egui::Role::MenuItemRadio, checked, text);
    response
}

fn edge_length(ctx: &egui::Context, anchor: Anchor) -> f32 {
    let screen = ctx.content_rect();
    match anchor {
        Anchor::Left | Anchor::Right => screen.height(),
        Anchor::Bottom => screen.width(),
    }
}

impl IdeView {
    pub(super) fn show_tool(&mut self, id: &str) {
        self.tool_windows.show(id);
        self.sync_tool_flags();
    }

    pub(super) fn hide_tool(&mut self, id: &str) {
        self.tool_windows.hide(id);
        self.sync_tool_flags();
    }

    pub(super) fn toggle_tool(&mut self, id: &str) {
        if !self.tool_windows.is_visible(id) {
            self.tool_window_took_keyboard();
        }
        self.tool_windows.toggle(id);
        self.sync_tool_flags();
    }

    pub(super) fn tool_visible(&self, id: &str) -> bool {
        self.tool_windows.is_visible(id)
    }

    pub(super) fn move_tool(&mut self, id: &str, side: Side, index: Option<usize>) {
        self.tool_windows.move_to(id, side, index);
        self.sync_tool_flags();
    }

    pub(super) fn restore_default_layout(&mut self) {
        self.tool_windows = ToolWindowLayout::default();
        self.sync_tool_flags();
        self.default_layout_current();
    }

    pub(super) fn hide_all_tools(&mut self) -> ToolWindowLayout {
        let kept = self.tool_windows.hide_all();
        self.sync_tool_flags();
        kept
    }

    pub(super) fn restore_tools(&mut self, kept: ToolWindowLayout) {
        self.tool_windows.restore(kept);
        self.sync_tool_flags();
    }

    pub(super) fn toggle_all_tools(&mut self) {
        if self.tool_windows.windows.iter().any(|window| window.visible) {
            self.stripes.hidden_all = Some(self.hide_all_tools());
        } else if let Some(kept) = self.stripes.hidden_all.take() {
            self.restore_tools(kept);
        }
    }

    pub(super) fn sync_tool_flags(&mut self) {
        self.project_open = self.tool_windows.is_visible("project");
        self.structure_open = self.tool_windows.is_visible("structure");
        self.commit_open = self.tool_windows.is_visible("commit");
    }

    pub(super) fn absorb_tool_flags(&mut self) {
        for (id, open) in
            [("project", self.project_open), ("structure", self.structure_open), ("commit", self.commit_open)]
        {
            if open != self.tool_windows.is_visible(id) {
                if open {
                    self.tool_windows.show(id);
                } else {
                    self.tool_windows.hide(id);
                }
            }
        }
        self.sync_tool_flags();
    }

    pub(super) fn apply_menu_request(&mut self, ctx: &egui::Context, id: &str, request: MenuRequest) {
        match request {
            MenuRequest::Hide => self.hide_tool(id),
            MenuRequest::Move(side) => self.move_tool(id, side, None),
            MenuRequest::Mode(mode) => self.set_view_mode(id, mode),
            MenuRequest::Stretch(direction) => {
                let Some(side) = self.tool_windows.side_of(id) else { return };
                let length = edge_length(ctx, side.anchor());
                self.tool_windows.stretch(id, direction, STRETCH_STEP, length);
            }
        }
    }

    pub(super) fn tool_window_menu(&mut self, ui: &mut Ui, id: &str, stripe: bool) {
        if let Some(request) = layout_menu(ui, &self.tool_windows, id, stripe) {
            self.apply_menu_request(ui.ctx(), id, request);
        }
    }

    fn refresh_stripe_ids(&mut self, side: Side) -> usize {
        let index = side_index(side);
        let layout = &self.tool_windows;
        let tools = &self.build_tools;
        let current = || {
            layout
                .at(side)
                .filter(|window| tools.available(&window.id))
                .filter_map(|window| kind(&window.id))
                .map(|kind| kind.id)
        };
        if !current().eq(self.stripes.ids[index].iter().copied()) {
            self.stripes.ids[index] = current().collect();
        }
        self.stripes.ids[index].len()
    }

    fn laid_out_stripe(&mut self, rect: Rect, fixed: usize, sides: [Side; 3], left: bool) -> Stripe {
        let [first, second, bottom] = sides.map(|side| self.refresh_stripe_ids(side));
        let kept = if left { self.stripes.left.take() } else { self.stripes.right.take() };
        match kept {
            Some(stripe)
                if stripe.rect == rect
                    && stripe.start == Stripe::first_centre(rect, fixed)
                    && (stripe.first.len(), stripe.second.len(), stripe.bottom.len()) == (first, second, bottom) =>
            {
                stripe
            }
            _ => Stripe::new(rect, fixed, first, second, bottom),
        }
    }

    fn stripe_button(&mut self, ui: &mut Ui, id: &'static str, y: f32, hint_side: super::Side) {
        let Some(kind) = kind(id) else { return };
        let p = self.palette;
        let rect = ui.max_rect();
        let button = Rect::from_center_size(pos2(rect.center().x, y), Vec2::splat(metrics::STRIPE_BUTTON));
        let response = ui.interact(button, ui.scope_id().with(("ide-tool-stripe", id)), Sense::click_and_drag());
        let active = self.tool_windows.is_visible(id);
        crate::accessibility::button(&response, &t!(kind.title));
        crate::accessibility::pressed(&response, active);
        if active {
            ui.painter().rect_filled(button, 7, p.accent);
        } else {
            hover(ui, &response, &p);
        }
        let busy = match id {
            "run" => self.run_window.any_running(),
            "debug" => self.debug_window.any_live(),
            "services" => self.services_window.any_running(),
            _ => false,
        };
        if busy {
            ui.painter().circle_filled(button.right_top() + vec2(-6.0, 6.0), 3.0, p.run);
        }
        if self.stripes.dragged != Some(id) && response.hovered() {
            hint(ui, &response, &t!(kind.title), crate::keymap::shortcut(kind.action).as_deref(), hint_side);
        }
        let color = if active { p.stripe_active_icon } else { p.text_secondary };
        let color = if self.stripes.dragged == Some(id) { color.gamma_multiply(0.4) } else { color };
        match self.caffeine_ui.look.filter(|_| id == "caffeine") {
            Some(look) => super::caffeine::draw_stripe_cup(ui.painter(), button.shrink(7.5), look, color),
            None => super::plugin_tool_window::paint_glyph(kind, ui.painter(), button.shrink(7.5), color),
        }
        if let Some(error) = self.notifications.badge.filter(|_| id == "notifications") {
            let color = if error { p.diagnostic_error } else { p.accent };
            let dot = pos2(button.right() - 8.0, button.top() + 8.0);
            ui.painter().circle_filled(dot, 4.0, p.frame);
            ui.painter().circle_filled(dot, 3.0, color);
        }
        if response.drag_started() {
            self.stripes.dragged = Some(id);
        }
        if response.clicked() {
            if id == "notifications" {
                self.toggle_notifications();
            } else {
                self.toggle_tool(id);
            }
        }
        response.context_menu(|ui| {
            customization::popup(
                ui,
                customization::TOOL_WINDOW_POPUP,
                |_| true,
                |ui, _| {
                    self.tool_window_menu(ui, id, true);
                },
            );
        });
    }

    pub(super) fn left_stripe(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let sides = [Side::LeftTop, Side::LeftBottom, Side::BottomLeft];
        let stripe = self.laid_out_stripe(ui.max_rect(), 0, sides, true);
        self.draw_stripe(ui, &stripe, sides, super::Side::Right);
        let more =
            Rect::from_center_size(pos2(stripe.rect.center().x, stripe.top_end), Vec2::splat(metrics::STRIPE_BUTTON));
        let response = ui.interact(more, ui.scope_id().with("ide-tool-stripe-more"), Sense::click());
        hover(ui, &response, &p);
        glyph_more_horizontal(ui.painter(), more.shrink(7.5), p.text_secondary);
        if in_development(ui, &response, &t!("ide-tool-more"), super::Side::Right) {
            self.post(ui, t!("ide-in-development-notice"));
        }
        self.stripes.left = Some(stripe);
    }

    fn draw_stripe(&mut self, ui: &mut Ui, stripe: &Stripe, sides: [Side; 3], hint_side: super::Side) {
        if let Some(line) = stripe.separator {
            let half = metrics::STRIPE_BUTTON / 2.0 - 6.0;
            let x = stripe.rect.center().x;
            ui.painter().hline(x - half..=x + half, line, Stroke::new(1.0, self.palette.hover));
        }
        for (side, centres) in sides.into_iter().zip([&stripe.first, &stripe.second, &stripe.bottom]) {
            let ids = std::mem::take(&mut self.stripes.ids[side_index(side)]);
            for (id, y) in ids.iter().zip(centres.iter()) {
                self.stripe_button(ui, id, *y, hint_side);
            }
            self.stripes.ids[side_index(side)] = ids;
        }
    }

    pub(super) fn right_stripe(&mut self, ui: &mut Ui, session: &Session) -> Option<(&'static str, Value)> {
        let p = self.palette;
        let rect = ui.max_rect();
        let mut action = None;
        let git = zen_workbench::vcs::workspace::WORKSPACE;
        let expanded = zen_workbench::workspaces::expanded(session);
        let active = expanded.as_deref() == Some(git);
        let button = Rect::from_center_size(
            pos2(rect.center().x, Stripe::first_centre(rect, 0)),
            Vec2::splat(metrics::STRIPE_BUTTON),
        );
        let response = ui.interact(button, ui.scope_id().with("stripe-right-git-workspace"), Sense::click());
        hint(
            ui,
            &response,
            &t!("keymap-vcs-workspace-open"),
            crate::keymap::shortcut("vcs.workspace.open").as_deref(),
            super::Side::Left,
        );
        crate::accessibility::pressed(&response, active);
        if active {
            ui.painter().rect_filled(button, 7, p.accent);
        } else {
            hover(ui, &response, &p);
        }
        glyph_git_workspace(
            ui.painter(),
            button.shrink(8.0),
            if active { p.stripe_active_icon } else { p.text_secondary },
            p.run.gamma_multiply(0.55),
        );
        if response.clicked() {
            action = Some(("workspace.host.toggle", json!({ "id": git })));
        }
        let workspaces: Vec<zen_workbench::workspaces::WorkspaceInfo> = session
            .state::<zen_workbench::workspaces::Workspaces>()
            .map(|workspaces| workspaces.registered().iter().filter(|info| info.id != git).cloned().collect())
            .unwrap_or_default();
        self.stripes.workspaces.clear();
        for (index, info) in workspaces.iter().enumerate() {
            let centre = pos2(rect.center().x, Stripe::first_centre(rect, index + 1));
            let button = Rect::from_center_size(centre, Vec2::splat(metrics::STRIPE_BUTTON));
            let id = ui.scope_id().with(("stripe-right-workspace", info.id.as_str()));
            let response = ui.interact(button, id, Sense::click());
            self.stripes.workspaces.push((info.id.clone(), button));
            let lit = expanded.as_deref() == Some(info.id.as_str());
            hint(ui, &response, &info.title, None, super::Side::Left);
            crate::accessibility::pressed(&response, lit);
            if lit {
                ui.painter().rect_filled(button, 7, p.accent);
            } else {
                hover(ui, &response, &p);
            }
            let glyph = super::workspace_host::workspace_glyph(&info.icon);
            glyph(ui.painter(), button.shrink(7.5), if lit { p.stripe_active_icon } else { p.text_secondary });
            if response.clicked() {
                action = Some(("workspace.host.toggle", json!({ "id": info.id })));
            }
        }
        let sides = [Side::RightTop, Side::RightBottom, Side::BottomRight];
        let stripe = self.laid_out_stripe(rect, 1 + workspaces.len(), sides, false);
        self.draw_stripe(ui, &stripe, sides, super::Side::Left);
        self.stripes.right = Some(stripe);
        action
    }

    #[cfg(test)]
    pub(super) fn workspace_button(&self, id: &str) -> Option<Rect> {
        self.stripes.workspaces.iter().find(|(drawn, _)| drawn == id).map(|(_, rect)| *rect)
    }

    #[cfg(test)]
    pub(super) fn first_right_tool_button(&self) -> Option<f32> {
        self.stripes.right.as_ref().map(|stripe| stripe.start)
    }

    pub(super) fn stripe_drag(&mut self, ui: &Ui) {
        let Some(id) = self.stripes.dragged else { return };
        let (pos, down) = ui.input(|input| (input.pointer.latest_pos(), input.pointer.primary_down()));
        let target = pos
            .and_then(|pos| self.stripes.target(pos))
            .map(|(side, index, line, stripe)| (side, index, line, stripe.rect));
        if !down {
            self.stripes.dragged = None;
            if let Some((side, index, _, _)) = target {
                let mut index = index;
                if self.tool_windows.side_of(id) == Some(side)
                    && let Some(at) = self.tool_windows.at(side).position(|window| window.id == id)
                    && at < index
                {
                    index -= 1;
                }
                self.move_tool(id, side, Some(index));
            }
            return;
        }
        let p = self.palette;
        let painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, ui.scope_id().with("ide-tool-stripe-drag")));
        if let Some((_, _, line, rect)) = target {
            let half = metrics::STRIPE_BUTTON / 2.0;
            painter.hline(rect.center().x - half..=rect.center().x + half, line, Stroke::new(2.0, p.accent));
        }
        if let (Some(pos), Some(kind)) = (pos, kind(id)) {
            let button = Rect::from_center_size(pos, Vec2::splat(metrics::STRIPE_BUTTON));
            painter.rect_filled(button, 7, p.popup);
            super::plugin_tool_window::paint_glyph(kind, &painter, button.shrink(7.5), p.text);
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        ui.ctx().request_repaint();
    }

    pub(super) fn tool_window_panels(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        project: &Path,
    ) {
        self.absorb_tool_flags();
        self.follow_tool_focus(ui);
        let room = ui.available_rect_before_wrap();
        let [left, bottom, right] = self.tool_windows.fitted_sizes(room.width(), room.height());
        for (anchor, size) in [(Anchor::Bottom, bottom), (Anchor::Left, left), (Anchor::Right, right)] {
            self.edge_panel(ui, anchor, size, registry, session, project);
        }
        let editor = ui.available_rect_before_wrap();
        self.undocked_tool_windows(ui, editor, registry, session, project);
    }

    fn edge_panel(
        &mut self,
        ui: &mut Ui,
        anchor: Anchor,
        size: f32,
        registry: &Registry,
        session: &mut Session,
        project: &Path,
    ) {
        let [first_side, second_side] = anchor.sides();
        self.stripes.drawn[side_index(first_side)] = None;
        self.stripes.drawn[side_index(second_side)] = None;
        let visible = |layout: &ToolWindowLayout, side| {
            layout.docked_at(side).and_then(|window| kind(&window.id)).map(|kind| kind.id)
        };
        let (first, second) = (visible(&self.tool_windows, first_side), visible(&self.tool_windows, second_side));
        if first.is_none() && second.is_none() {
            return;
        }
        let p = self.palette;
        let panel = match anchor {
            Anchor::Left => egui::Panel::left("ide-tool-left"),
            Anchor::Bottom => egui::Panel::bottom("ide-tool-bottom"),
            Anchor::Right => egui::Panel::right("ide-tool-right"),
        };
        let shown = panel
            .exact_size(size)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.frame))
            .show(ui, |ui| {
                let outer = ui.max_rect();
                let inner = inner_rect(outer, anchor);
                let parts = match (first, second) {
                    (Some(first), Some(second)) => {
                        let share = self.tool_windows.split(anchor);
                        let (a, b) = split_rect(inner, anchor, share);
                        self.split_handle(ui, anchor, inner, a, b);
                        [Some((first_side, first, a)), Some((second_side, second, b))]
                    }
                    (Some(only), None) => [Some((first_side, only, inner)), None],
                    (None, Some(only)) => [Some((second_side, only, inner)), None],
                    (None, None) => [None, None],
                };
                for (side, id, rect) in parts.into_iter().flatten() {
                    self.stripes.drawn[side_index(side)] = Some((id, rect));
                    self.tool_window_part(ui, id, rect, registry, session, project);
                }
                outer
            });
        self.edge_handle(ui, anchor, shown.inner);
    }

    pub(super) fn tool_window_part(
        &mut self,
        ui: &mut Ui,
        id: &'static str,
        rect: Rect,
        registry: &Registry,
        session: &mut Session,
        project: &Path,
    ) {
        let p = self.palette;
        if ui.input(|input| {
            input.pointer.any_pressed() && input.pointer.interact_pos().is_some_and(|pos| rect.contains(pos))
        }) {
            self.tool_windows.activate(id);
            self.tool_window_took_keyboard();
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("ide-tool-window", id)));
        child.shrink_clip_rect(rect);
        egui::Frame::new().fill(p.island).corner_radius(CornerRadius::same(metrics::ISLAND_RADIUS)).show(
            &mut child,
            |ui| {
                ui.set_min_size(ui.available_size());
                self.bottom = Some(id);
                self.draw_tool_window(ui, id, registry, session, project);
            },
        );
        let hid_itself = self.bottom.take().is_none();
        self.absorb_tool_flags();
        if hid_itself {
            self.hide_tool(id);
        }
    }

    pub(super) fn draw_tool_window(
        &mut self,
        ui: &mut Ui,
        id: &str,
        registry: &Registry,
        session: &mut Session,
        project: &Path,
    ) {
        match id {
            "project" => self.project_tool_window(ui, registry, session, project),
            "commit" => self.as_git(|view| view.commit_tool_window(ui, registry, session)),
            "structure" => self.structure_tool_window(ui, registry, session),
            "bookmarks" => self.bookmarks_tool_window(ui, registry, session),
            "find" => self.find_tool_window(ui, registry, session),
            "debug" => self.debug_tool_window(ui, registry, session),
            "run" => self.run_tool_window(ui, registry, session),
            "problems" => self.problems_tool_window(ui, registry, session),
            "git" => self.as_git(|view| view.git_tool_window(ui, registry, session)),
            "services" => self.services_tool_window(ui, registry, session),
            "hierarchy" => self.hierarchy_tool_window(ui, registry, session),
            "coverage" => self.coverage_tool_window(ui, registry, session),
            "todo" => self.todo_tool_window(ui, registry, session),
            "notifications" => self.notifications_tool_window(ui, registry, session),
            "caffeine" => self.caffeine_tool_window(ui, registry, session),
            "maven" => self.build_tool_window(ui, super::build_tool_window::Tool::Maven, registry, session),
            "gradle" => self.build_tool_window(ui, super::build_tool_window::Tool::Gradle, registry, session),
            "build" => self.build_sync_tool_window(ui, registry, session),
            "ant" => self.ant_tool_window(ui, registry, session),
            "npm" => self.npm_tool_window(ui, registry, session),
            "endpoints" => self.endpoints_tool_window(ui, registry, session),
            "dependencies" => self.dependencies_tool_window(ui, registry, session),
            "terminal" => {
                #[cfg(not(target_arch = "wasm32"))]
                self.terminal_tool_window(ui, registry, session);
            }
            other => self.plugin_tool_window(ui, other, registry, session),
        }
    }

    pub(super) fn edge_handle(&mut self, ui: &Ui, anchor: Anchor, outer: Rect) {
        let gap = metrics::GAP;
        let handle = match anchor {
            Anchor::Left => Rect::from_x_y_ranges(outer.right() - 2.0..=outer.right() + gap + 2.0, outer.y_range()),
            Anchor::Right => Rect::from_x_y_ranges(outer.left() - gap - 2.0..=outer.left() + 2.0, outer.y_range()),
            Anchor::Bottom => Rect::from_x_y_ranges(outer.x_range(), outer.top() - gap - 2.0..=outer.top() + 2.0),
        };
        let response = ui.interact(handle, ui.scope_id().with(("ide-tool-edge", anchor_name(anchor))), Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(match anchor {
                Anchor::Bottom => CursorIcon::ResizeVertical,
                Anchor::Left | Anchor::Right => CursorIcon::ResizeHorizontal,
            });
        }
        if response.dragged()
            && let Some(pos) = ui.input(|input| input.pointer.interact_pos())
        {
            let size = match anchor {
                Anchor::Left => pos.x - outer.left(),
                Anchor::Right => outer.right() - pos.x,
                Anchor::Bottom => outer.bottom() - pos.y,
            };
            self.tool_windows.set_size_at(anchor, size);
        }
    }

    fn split_handle(&mut self, ui: &Ui, anchor: Anchor, inner: Rect, first: Rect, second: Rect) {
        let handle = match anchor {
            Anchor::Bottom => Rect::from_x_y_ranges(first.right() - 2.0..=second.left() + 2.0, inner.y_range()),
            Anchor::Left | Anchor::Right => {
                Rect::from_x_y_ranges(inner.x_range(), first.bottom() - 2.0..=second.top() + 2.0)
            }
        };
        let response = ui.interact(handle, ui.scope_id().with(("ide-tool-split", anchor_name(anchor))), Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(match anchor {
                Anchor::Bottom => CursorIcon::ResizeHorizontal,
                Anchor::Left | Anchor::Right => CursorIcon::ResizeVertical,
            });
        }
        if response.dragged()
            && let Some(pos) = ui.input(|input| input.pointer.interact_pos())
        {
            let gap = metrics::GAP;
            let share = match anchor {
                Anchor::Bottom => (pos.x - inner.left() - gap / 2.0) / (inner.width() - gap),
                Anchor::Left | Anchor::Right => (pos.y - inner.top() - gap / 2.0) / (inner.height() - gap),
            };
            self.tool_windows.set_split(anchor, share);
        }
    }

    pub(super) fn layout_action(
        &mut self,
        ctx: &egui::Context,
        action: &str,
        params: &Value,
    ) -> Result<Value, ActionError> {
        use ActionError::Refused;
        if action == "view.layout.restoreDefault" {
            self.restore_default_layout();
            return Ok(Value::Null);
        }
        if action == "view.toolWindow.hideAll" {
            self.toggle_all_tools();
            return Ok(Value::Null);
        }
        if action.starts_with("view.layout.") {
            return self.saved_layout_action(action, params);
        }
        let named = params.get("id").and_then(Value::as_str);
        if let Some(id) = named
            && self.tool_windows.get(id).is_none_or(|_| kind(id).is_none())
        {
            let known: Vec<&str> = KINDS.iter().map(|kind| kind.id).collect();
            return Err(Refused(format!("no tool window `{id}`; the tool windows are {}", known.join(", "))));
        }
        let id = named.map(str::to_owned).or_else(|| self.tool_windows.active().map(str::to_owned));
        match action {
            "view.toolWindow.hide" => {
                if let Some(id) = id {
                    self.hide_tool(&id);
                }
                Ok(Value::Null)
            }
            "view.toolWindow.move" => {
                let sides: Vec<&str> = Side::ALL.iter().map(|side| side.name()).collect();
                let Some(side) = params.get("side").and_then(Value::as_str) else {
                    return Err(Refused(format!("give `side`: one of {}", sides.join(", "))));
                };
                let Some(side) = Side::from_name(side) else {
                    return Err(Refused(format!("`{side}` is not a side; give one of {}", sides.join(", "))));
                };
                let Some(id) = id else {
                    return Err(Refused("no tool window is active; name one with `id`".to_owned()));
                };
                let index = params.get("index").and_then(Value::as_u64).map(|index| index as usize);
                self.move_tool(&id, side, index);
                Ok(json!({ "id": id, "side": side.name() }))
            }
            "view.toolWindow.viewMode" => {
                let modes: Vec<&str> = ViewMode::ALL.iter().map(|mode| mode.name()).collect();
                let Some(mode) = params.get("mode").and_then(Value::as_str) else {
                    return Err(Refused(format!("give `mode`: one of {}", modes.join(", "))));
                };
                let Some(mode) = ViewMode::from_name(mode) else {
                    return Err(Refused(format!("`{mode}` is not a view mode; give one of {}", modes.join(", "))));
                };
                let Some(id) = id else {
                    return Err(Refused("no tool window is active; name one with `id`".to_owned()));
                };
                self.set_view_mode(&id, mode);
                Ok(json!({ "id": id, "mode": mode.name(), "visible": self.tool_visible(&id) }))
            }
            "view.toolWindow.resize" => {
                let direction =
                    params.get("direction").cloned().and_then(|value| serde_json::from_value::<Direction>(value).ok());
                let Some(direction) = direction else {
                    return Err(Refused("give `direction`: left, right, up or down".to_owned()));
                };
                let Some(id) = id else { return Ok(Value::Null) };
                let Some(side) = self.tool_windows.side_of(&id) else { return Ok(Value::Null) };
                let by = params.get("by").and_then(Value::as_f64).map_or(STRETCH_STEP, |by| by as f32);
                self.tool_windows.stretch(&id, direction, by, edge_length(ctx, side.anchor()));
                Ok(Value::Null)
            }
            _ => Err(Refused(format!("no layout action `{action}`"))),
        }
    }

    pub(super) fn layout_state(&self) -> Value {
        let windows: Vec<Value> = Side::ALL
            .into_iter()
            .flat_map(|side| {
                self.tool_windows
                    .at(side)
                    .filter(|window| kind(&window.id).is_some() && self.build_tools.available(&window.id))
                    .enumerate()
                    .map(move |(order, window)| {
                        json!({
                            "id": window.id,
                            "side": side.name(),
                            "order": order,
                            "size": window.size,
                            "visible": window.visible,
                            "mode": window.mode.name(),
                            "bounds": window.bounds,
                        })
                    })
            })
            .collect();
        json!({
            "windows": windows,
            "active": self.tool_windows.active(),
            "layouts": self.layouts_state(),
        })
    }

    pub(super) fn shown_at(&self, anchor: Anchor) -> Option<&str> {
        anchor
            .sides()
            .into_iter()
            .find_map(|side| {
                self.tool_windows
                    .visible_at(side)
                    .filter(|window| kind(&window.id).is_some() && self.build_tools.available(&window.id))
            })
            .map(|window| window.id.as_str())
    }
}

fn anchor_name(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::Left => "left",
        Anchor::Bottom => "bottom",
        Anchor::Right => "right",
    }
}

pub(super) fn inner_rect(outer: Rect, anchor: Anchor) -> Rect {
    let gap = metrics::GAP;
    match anchor {
        Anchor::Left => Rect::from_min_max(outer.min + vec2(gap, gap), outer.max - vec2(0.0, gap)),
        Anchor::Right => Rect::from_min_max(outer.min + vec2(0.0, gap), outer.max - vec2(gap, gap)),
        Anchor::Bottom => Rect::from_min_max(outer.min + vec2(gap, 0.0), outer.max - vec2(gap, gap)),
    }
}

fn split_rect(inner: Rect, anchor: Anchor, share: f32) -> (Rect, Rect) {
    let gap = metrics::GAP;
    match anchor {
        Anchor::Bottom => {
            let split = inner.left() + (inner.width() - gap) * share;
            (
                Rect::from_min_max(inner.min, pos2(split, inner.bottom())),
                Rect::from_min_max(pos2(split + gap, inner.top()), inner.max),
            )
        }
        Anchor::Left | Anchor::Right => {
            let split = inner.top() + (inner.height() - gap) * share;
            (
                Rect::from_min_max(inner.min, pos2(inner.right(), split)),
                Rect::from_min_max(pos2(inner.left(), split + gap), inner.max),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{Event, Key, Modifiers, PointerButton};

    use super::super::tool_windows::EDITOR_ROOM;
    use super::super::workspace;
    use super::*;

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        view: IdeView,
        time: f64,
        project: tempfile::TempDir,
        size: Vec2,
        texts: Vec<(String, Rect)>,
    }

    fn texts(shape: &egui::Shape, found: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Text(text) => found.push((text.galley.text().to_owned(), text.visual_bounding_rect())),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, found)),
            _ => {}
        }
    }

    fn product() -> super::super::IdeProduct {
        super::super::IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: Vec::new(),
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            folding_options: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        }
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            zen_i18n::register(zen_workbench::LOCALES);
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let project = tempfile::tempdir().unwrap();
            std::fs::write(project.path().join("main.go"), "package main\n").unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
            let view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
            let mut harness =
                Self { ctx, registry, session, view, time: 0.0, project, size: vec2(1200.0, 800.0), texts: Vec::new() };
            harness.frame(Vec::new());
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| self.view.show(ui, &self.registry, &mut self.session));
            output.textures_delta.clear();
            self.texts.clear();
            output.shapes.iter().for_each(|clipped| texts(&clipped.shape, &mut self.texts));
        }

        fn text(&self, text: &str) -> Option<Pos2> {
            self.texts.iter().find(|(drawn, _)| drawn == text).map(|(_, rect)| rect.center())
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![button(from, true)]);
            for step in 1..=5 {
                self.frame(vec![Event::PointerMoved(from + (to - from) * step as f32 / 5.0)]);
            }
            self.frame(vec![button(to, false)]);
            self.frame(Vec::new());
        }

        fn click(&mut self, pos: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.frame(Vec::new());
        }

        fn action(&mut self, action: &str) {
            let ctx = self.ctx.clone();
            let (command, params) = crate::keymap::current()
                .bindings()
                .into_iter()
                .find(|binding| binding.action == action)
                .map_or_else(|| (action.to_owned(), json!({})), |binding| (binding.command, binding.params));
            let _ = self.view.perform_action(&ctx, &self.registry, &mut self.session, &command, params, false);
            self.frame(Vec::new());
        }

        fn drawn(&self, id: &str) -> Option<Rect> {
            self.view.stripes.drawn.iter().flatten().find(|(drawn, _)| *drawn == id).map(|(_, rect)| *rect)
        }

        fn hide_button(&self, id: &str) -> Pos2 {
            let rect = self.drawn(id).unwrap();
            pos2(rect.right() - 18.0, rect.top() + metrics::TOOL_HEADER / 2.0)
        }

        fn press(&mut self, key: Key, modifiers: Modifiers) {
            self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
            self.frame(Vec::new());
        }

        fn state(&self) -> Value {
            self.view.window_state(&self.session)
        }
    }

    #[test]
    fn always_select_opened_file_is_in_the_project_options_behavior_submenu() {
        let mut harness = Harness::new();
        let header = harness.drawn("project").unwrap();
        harness.click(pos2(header.right() - 46.0, header.top() + metrics::TOOL_HEADER / 2.0));
        let always = t!("ide-project-always-select-opened");
        assert!(harness.text(&t!("ide-tool-move-to")).is_some(), "⋮ is open");
        assert!(harness.text(&always).is_none(), "Always Select Opened File is not at the top of ⋮");
        let behavior = harness.text(&t!("ide-project-behavior")).unwrap();
        harness.click(behavior);
        let item = harness.text(&always).unwrap();
        harness.click(item);
        assert_eq!(harness.state()["projectView"]["alwaysSelectOpened"], true);
    }

    #[test]
    fn dragging_a_stripe_button_to_the_other_stripe_moves_its_tool_window() {
        let mut harness = Harness::new();
        assert_eq!(harness.state()["left"], "project");
        let left = harness.view.stripes.left.clone().unwrap();
        let right = harness.view.stripes.right.clone().unwrap();
        let project = pos2(left.rect.center().x, left.first[0]);
        let top = right.separator.map_or(right.start, |line| line - 2.0);
        harness.drag(project, pos2(right.rect.center().x, top));
        assert_eq!(harness.view.tool_windows.side_of("project"), Some(Side::RightTop));
        let state = harness.state();
        assert_eq!(state["left"], Value::Null, "{state}");
        assert_eq!(state["right"], "project", "a shown tool window shows where it goes");
        assert_eq!(
            harness.view.stripes.right.as_ref().unwrap().first.len(),
            4,
            "its button is on the right stripe, after Caffeine, Hierarchy and Coverage"
        );

        let left = harness.view.stripes.left.clone().unwrap();
        let git = pos2(left.rect.center().x, left.bottom[left.bottom.len() - 3]);
        harness.drag(git, pos2(left.rect.center().x, left.start - 10.0));
        let ids: Vec<&str> = harness.view.tool_windows.at(Side::LeftTop).map(|window| window.id.as_str()).collect();
        assert_eq!(ids, ["git", "commit", "structure"], "dropped above the first button, it goes first");
        assert!(!harness.view.tool_visible("git"), "a hidden tool window stays hidden");

        let left = harness.view.stripes.left.clone().unwrap();
        let commit = pos2(left.rect.center().x, left.first[1]);
        harness.drag(commit, pos2(left.rect.center().x, left.second[0] - 4.0));
        assert_eq!(harness.view.tool_windows.side_of("commit"), Some(Side::LeftBottom));
        let under: Vec<&str> =
            harness.view.tool_windows.at(Side::LeftBottom).map(|window| window.id.as_str()).collect();
        assert_eq!(under, ["commit", "bookmarks"], "dropped on the first left bottom button, it goes before it");
        assert!(harness.view.stripes.left.as_ref().unwrap().separator.is_some(), "a line before the left bottom ones");

        harness.press(Key::F12, Modifiers::SHIFT);
        assert_eq!(harness.view.tool_windows, ToolWindowLayout::default(), "⇧F12 restores the default layout");
        assert_eq!(harness.state()["left"], "project");
    }

    #[test]
    fn a_resized_edge_and_the_shown_tool_windows_come_back_with_the_project() {
        let mut harness = Harness::new();
        harness.view.show_tool("terminal");
        harness.view.move_tool("structure", Side::RightTop, None);
        harness.view.show_tool("structure");
        harness.frame(Vec::new());
        let stripe = harness.view.stripes.left.clone().unwrap().rect;
        let edge = stripe.right() + 330.0 + metrics::GAP / 2.0;
        harness.drag(pos2(edge, 300.0), pos2(edge + 80.0, 300.0));
        let width = harness.view.tool_windows.size_at(Anchor::Left);
        assert!((width - 412.5).abs() < 0.5, "the left edge is {width} wide");
        assert_eq!(harness.state()["right"], "structure");

        let layout = workspace::capture(&harness.view, &mut harness.session, harness.project.path());
        let json = serde_json::to_value(&layout).unwrap();
        assert!(json["toolWindows"].get("active").is_none());
        let read: workspace::ProjectLayout = serde_json::from_value(json).unwrap();
        let mut reopened = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        workspace::restore(&mut reopened, &harness.registry, &mut harness.session, &read);
        assert!((reopened.tool_windows.size_at(Anchor::Left) - width).abs() < 0.01);
        assert!(
            reopened.tool_visible("project") && reopened.tool_visible("terminal") && reopened.tool_visible("structure")
        );
        assert_eq!(reopened.tool_windows.side_of("structure"), Some(Side::RightTop));
        assert!(reopened.project_open && reopened.structure_open);
    }

    #[test]
    fn older_workspace_files_open_with_their_panels() {
        let harness = Harness::new();
        let mut session = Session::new();
        let read = |text: &str| serde_json::from_str::<workspace::ProjectLayout>(text).unwrap();
        let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        workspace::restore(
            &mut view,
            &harness.registry,
            &mut session,
            &read(r#"{ "structurePanel": true, "terminalOpen": true }"#),
        );
        assert!(view.tool_visible("structure") && view.tool_visible("terminal") && !view.tool_visible("project"));
        assert!(view.structure_open && !view.project_open);
        let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        workspace::restore(
            &mut view,
            &harness.registry,
            &mut session,
            &read(r#"{ "projectPanel": true, "toolWindows": { "windows": 3 } }"#),
        );
        assert!(view.tool_visible("project"), "a layout the file spells wrongly is left out, the rest is read");
        let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        workspace::restore(&mut view, &harness.registry, &mut session, &read("{}"));
        assert!(view.tool_windows.windows.iter().all(|window| !window.visible), "nothing was shown");
    }

    #[test]
    fn shift_escape_hides_the_active_tool_window_and_the_stretch_keys_resize_it() {
        let mut harness = Harness::new();
        harness.view.show_tool("problems");
        harness.frame(Vec::new());
        if cfg!(target_os = "macos") {
            harness.view.tool_windows.activate("project");
            let left = harness.view.tool_windows.size_at(Anchor::Left);
            harness.press(Key::ArrowRight, Modifiers { ctrl: true, alt: true, ..Modifiers::NONE });
            assert_eq!(harness.view.tool_windows.size_at(Anchor::Left), left + STRETCH_STEP, "⌃⌥→");
            harness.view.tool_windows.activate("problems");
        }
        harness.press(Key::Escape, Modifiers::SHIFT);
        assert!(!harness.view.tool_visible("problems"), "⇧⎋ hides the tool window shown last");
        assert!(harness.view.tool_visible("project"));
    }

    #[test]
    fn git_and_commit_open_hide_and_move_through_the_layout() {
        let mut harness = Harness::new();
        harness.view.move_tool("git", Side::RightTop, None);
        harness.action("view.toolWindow.git");
        assert_eq!(harness.state()["right"], "git");
        let git = harness.drawn("git").unwrap();
        assert!(git.left() > 600.0, "the Git tool window is drawn on the right: {git:?}");
        harness.click(harness.hide_button("git"));
        assert!(!harness.view.tool_visible("git"), "its own Hide button hides it where it is");
        assert_eq!(harness.state()["right"], Value::Null);
        assert_eq!(harness.view.tool_windows.side_of("git"), Some(Side::RightTop));

        harness.view.move_tool("commit", Side::BottomRight, None);
        harness.action("view.vcs.commit");
        assert!(harness.view.tool_visible("commit") && harness.view.commit_open);
        assert!(harness.view.tool_visible("project"), "on another side, Commit leaves Project shown");
        assert!(harness.view.project_open);
        harness.click(harness.hide_button("commit"));
        assert!(!harness.view.tool_visible("commit") && !harness.view.commit_open);
        assert!(harness.view.tool_visible("project"));

        harness.view.restore_default_layout();
        harness.action("view.vcs.commit");
        assert!(
            harness.view.tool_visible("commit") && !harness.view.tool_visible("project"),
            "on the left top it does"
        );
        harness.view.commit_open = false;
        harness.frame(Vec::new());
        assert!(!harness.view.tool_visible("commit"), "a flag the Commit code clears between frames is taken");
    }

    #[test]
    fn shift_escape_is_left_to_popups_and_text_fields() {
        let mut harness = Harness::new();
        let path = harness.project.path().join("main.go");
        harness.registry.dispatch(&mut harness.session, "file.document.open", json!({ "path": path })).unwrap();
        harness.view.show_tool("problems");
        harness.frame(Vec::new());
        harness.view.search_everywhere.toggle(super::super::search_everywhere::Tab::ALL);
        harness.frame(Vec::new());
        harness.press(Key::Escape, Modifiers::SHIFT);
        assert!(harness.view.tool_visible("problems"), "Search Everywhere keeps ⇧⎋");
        assert!(harness.view.search_everywhere.open_tab().is_none(), "and closes with it");

        harness.action("view.find.open");
        harness.frame(Vec::new());
        let focused = harness.ctx.memory(egui::Memory::focused).unwrap();
        assert!(
            egui::text_edit::TextEditState::load(&harness.ctx, focused).is_some(),
            "the find field has the keyboard"
        );
        harness.press(Key::Escape, Modifiers::SHIFT);
        assert!(harness.view.tool_visible("problems"), "a text field keeps ⇧⎋");

        harness.view.editor_view.focus();
        harness.frame(Vec::new());
        let left = harness.view.stripes.left.clone().unwrap();
        let project = pos2(left.rect.center().x, left.first[0]);
        let secondary = |pressed| Event::PointerButton {
            pos: project,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        harness.frame(vec![Event::PointerMoved(project)]);
        harness.frame(vec![secondary(true)]);
        harness.frame(vec![secondary(false)]);
        harness.frame(Vec::new());
        assert!(egui::Popup::is_any_open(&harness.ctx), "the stripe button's menu is open");
        harness.press(Key::Escape, Modifiers::SHIFT);
        assert!(harness.view.tool_visible("problems"), "a menu keeps ⇧⎋");
        egui::Popup::close_all(&harness.ctx);
        harness.view.editor_view.focus();
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        harness.press(Key::Escape, Modifiers::SHIFT);
        assert!(!harness.view.tool_visible("problems"), "from the editor ⇧⎋ hides the active tool window");
    }

    #[test]
    fn hide_all_tool_windows_hides_them_and_shows_them_again() {
        let mut harness = Harness::new();
        harness.view.show_tool("terminal");
        harness.action("view.toolWindow.hideAll");
        assert!(harness.view.tool_windows.windows.iter().all(|window| !window.visible));
        assert!(!harness.view.project_open);
        assert_eq!((harness.state()["left"].clone(), harness.state()["bottom"].clone()), (Value::Null, Value::Null));
        harness.action("view.toolWindow.hideAll");
        assert!(harness.view.tool_visible("project") && harness.view.tool_visible("terminal"));
        assert!(harness.view.project_open);
        harness.action("view.toolWindow.hideAll");
        harness.view.show_tool("problems");
        harness.action("view.toolWindow.hideAll");
        assert!(!harness.view.tool_visible("problems"), "with one shown again, it hides that one");
    }

    #[test]
    fn a_smaller_window_fits_the_tool_windows_and_keeps_their_sizes() {
        let mut harness = Harness::new();
        harness.view.tool_windows.set_size_at(Anchor::Bottom, 900.0);
        harness.view.tool_windows.set_size_at(Anchor::Left, 640.0);
        harness.view.move_tool("problems", Side::RightTop, None);
        harness.view.show_tool("problems");
        harness.view.show_tool("find");
        harness.size = vec2(900.0, 600.0);
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        let (left, right, bottom) =
            (harness.drawn("project").unwrap(), harness.drawn("problems").unwrap(), harness.drawn("find").unwrap());
        assert!(bottom.height() < 600.0 - EDITOR_ROOM, "{bottom:?}");
        assert!(right.left() - left.right() >= EDITOR_ROOM - 2.0 * metrics::GAP, "{left:?} {right:?}");
        assert_eq!(harness.view.tool_windows.size_at(Anchor::Bottom), 900.0, "the size kept is the larger one");
        assert_eq!(harness.view.tool_windows.size_at(Anchor::Left), 640.0);
    }

    #[test]
    fn the_layout_actions_answer_agents() {
        let mut harness = Harness::new();
        let ctx = harness.ctx.clone();
        let run = |harness: &mut Harness, action: &str, params: Value| {
            harness.view.layout_action(&ctx, action, &params).map_err(|error| match error {
                ActionError::Refused(message) => message,
                ActionError::Dispatch(error) => error.to_string(),
            })
        };
        let moved = run(&mut harness, "view.toolWindow.move", json!({ "id": "terminal", "side": "right" })).unwrap();
        assert_eq!(moved, json!({ "id": "terminal", "side": "rightTop" }));
        run(&mut harness, "view.toolWindow.move", json!({ "id": "git", "side": "rightTop", "index": 0 })).unwrap();
        let state = harness.state();
        let windows = state["toolWindows"]["windows"].as_array().unwrap();
        let terminal = windows.iter().find(|window| window["id"] == "terminal").unwrap();
        assert_eq!(terminal["side"], "rightTop");
        assert_eq!(terminal["order"], 4, "after Git, Caffeine, Hierarchy and Coverage");
        assert_eq!(terminal["visible"], false);
        assert!(
            run(&mut harness, "view.toolWindow.move", json!({ "id": "terminal" })).unwrap_err().contains("leftTop")
        );
        assert!(run(&mut harness, "view.toolWindow.move", json!({ "id": "terminal", "side": "top" })).is_err());
        assert!(
            run(&mut harness, "view.toolWindow.move", json!({ "id": "nothing", "side": "left" }))
                .unwrap_err()
                .contains("terminal")
        );
        assert!(run(&mut harness, "view.toolWindow.move", json!({ "side": "left" })).unwrap_err().contains("active"));

        harness.view.show_tool("terminal");
        assert_eq!(harness.state()["toolWindows"]["active"], "terminal");
        run(&mut harness, "view.toolWindow.resize", json!({ "direction": "left", "by": 50 })).unwrap();
        assert_eq!(harness.view.tool_windows.size_at(Anchor::Right), 380.0);
        assert!(run(&mut harness, "view.toolWindow.resize", json!({ "direction": "sideways" })).is_err());
        run(&mut harness, "view.toolWindow.hide", json!({})).unwrap();
        assert_eq!(harness.state()["right"], Value::Null);
        run(&mut harness, "view.toolWindow.hide", json!({})).unwrap();
        run(&mut harness, "view.layout.restoreDefault", json!({})).unwrap();
        assert_eq!(harness.view.tool_windows.side_of("terminal"), Some(Side::BottomLeft));
    }

    fn laid_out(first: usize, second: usize, bottom: usize) -> Stripe {
        Stripe::new(Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 600.0)), 0, first, second, bottom)
    }

    #[test]
    fn every_declared_tool_window_has_a_stripe_button() {
        for declared in super::super::tool_windows::DECLARED {
            assert!(kind(declared.id).is_some(), "{}", declared.id);
        }
        assert_eq!(KINDS.len(), super::super::tool_windows::DECLARED.len());
    }

    #[test]
    fn a_stripe_lays_out_its_parts() {
        let stripe = laid_out(3, 0, 2);
        assert_eq!(stripe.first, [25.0, 61.0, 97.0]);
        assert_eq!(stripe.separator, None);
        assert_eq!(stripe.top_end, 133.0);
        assert_eq!(stripe.bottom, [544.0, 580.0]);
        let split = laid_out(1, 2, 0);
        assert_eq!(split.separator, Some(48.0));
        assert_eq!(split.second, [71.0, 107.0]);
        assert_eq!(split.top_end, 143.0);
        let fixed = Stripe::new(Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 600.0)), 1, 1, 0, 0);
        assert_eq!(fixed.first, [61.0]);
    }

    #[test]
    fn a_button_dropped_on_a_stripe_goes_between_the_buttons_under_it() {
        let stripe = laid_out(3, 0, 2);
        assert_eq!(stripe.target(pos2(20.0, 10.0)), Some((Part::First, 0)));
        assert_eq!(stripe.target(pos2(20.0, 70.0)), Some((Part::First, 2)));
        assert_eq!(stripe.target(pos2(20.0, 120.0)), Some((Part::Second, 0)), "under the last: the second half");
        assert_eq!(stripe.target(pos2(20.0, 560.0)), Some((Part::Bottom, 1)));
        assert_eq!(stripe.target(pos2(20.0, 599.0)), Some((Part::Bottom, 2)));
        assert_eq!(stripe.target(pos2(200.0, 70.0)), None, "off the stripe");
        let split = laid_out(1, 2, 0);
        assert_eq!(split.target(pos2(20.0, 40.0)), Some((Part::First, 1)));
        assert_eq!(split.target(pos2(20.0, 80.0)), Some((Part::Second, 1)));
        let empty = laid_out(0, 0, 0);
        assert_eq!(empty.target(pos2(20.0, 20.0)), Some((Part::First, 0)));
        assert_eq!(empty.target(pos2(20.0, 400.0)), Some((Part::Bottom, 0)));
        assert_eq!(stripe.indicator(Part::First, 1), 43.0);
        assert_eq!(stripe.indicator(Part::First, 3), 115.0);
        assert_eq!(empty.indicator(Part::Bottom, 0), 598.0);
    }

    #[test]
    fn the_stripes_name_the_side_a_button_goes_to() {
        let rect = |x: f32| Rect::from_min_size(pos2(x, 0.0), vec2(40.0, 600.0));
        let stripes = Stripes {
            left: Some(Stripe::new(rect(0.0), 0, 3, 0, 6)),
            right: Some(Stripe::new(rect(960.0), 1, 0, 0, 0)),
            ..Stripes::default()
        };
        let side = |x, y| stripes.target(pos2(x, y)).map(|(side, index, _, _)| (side, index));
        assert_eq!(side(20.0, 30.0), Some((Side::LeftTop, 1)));
        assert_eq!(side(20.0, 115.0), Some((Side::LeftBottom, 0)));
        assert_eq!(side(20.0, 590.0), Some((Side::BottomLeft, 6)));
        assert_eq!(side(980.0, 20.0), Some((Side::RightTop, 0)));
        assert_eq!(side(980.0, 100.0), Some((Side::RightBottom, 0)));
        assert_eq!(side(980.0, 500.0), Some((Side::BottomRight, 0)));
        assert_eq!(side(500.0, 300.0), None);
    }

    #[test]
    fn the_halves_of_an_edge_share_it_with_a_gap() {
        let inner = Rect::from_min_size(pos2(0.0, 0.0), vec2(105.0, 205.0));
        let (top, bottom) = split_rect(inner, Anchor::Left, 0.4);
        assert_eq!(top.height(), 80.0);
        assert_eq!(bottom.top(), 85.0);
        let (left, right) = split_rect(inner, Anchor::Bottom, 0.5);
        assert_eq!(left.width(), 50.0);
        assert_eq!(right.left(), 55.0);
    }

    #[cfg(unix)]
    #[test]
    fn the_terminal_header_keeps_its_buttons_and_shows_the_tab_shown() {
        use zen_workbench::terminal::Terminals;
        let mut harness = Harness::new();
        let home = format!("HOME={}", harness.project.path().display());
        harness.session.state_mut::<Terminals>().set_launcher(Arc::new(move |cwd: &Path| {
            zen_terminal::Options::command("/usr/bin/env", &["-i", &home, "PATH=/usr/bin:/bin", "/bin/sh"], cwd)
        }));
        let long = "someone@MacBook-Pro-of-someone:~/.claude/jobs/031edaed/tmp/reviewF1/proj";
        for tab in 0..3 {
            harness.registry.dispatch(&mut harness.session, "terminal.session.new", json!({})).unwrap();
            let name = json!({ "tab": tab, "name": format!("{long}/{tab}") });
            harness.registry.dispatch(&mut harness.session, "terminal.tab.rename", name).unwrap();
        }
        harness.view.show_tool("terminal");
        for (width, height, shown) in [(1280.0, 820.0, 2), (900.0, 700.0, 0), (640.0, 480.0, 2), (640.0, 480.0, 1)] {
            harness.registry.dispatch(&mut harness.session, "terminal.tab.select", json!({ "tab": shown })).unwrap();
            harness.size = vec2(width, height);
            harness.frame(Vec::new());
            harness.frame(Vec::new());
            let header = harness.view.terminal_header.clone();
            let names: Vec<&str> = header.buttons.iter().map(|(name, _)| *name).collect();
            assert_eq!(names, ["new", "tabs", "more", "hide"], "at {width}");
            let button = |name: &str| header.buttons.iter().find(|(drawn, _)| *drawn == name).unwrap().1;
            let (plus, list, more, hide) = (button("new"), button("tabs"), button("more"), button("hide"));
            assert!(hide.right() <= width && more.right() <= hide.left(), "at {width}: {more:?} {hide:?}");
            assert!(list.right() <= more.left(), "at {width}: ⌄ {list:?} runs under ⋮ {more:?}");
            assert!(plus.left() >= header.strip.left() && plus.right() <= list.left(), "at {width}");
            for (index, tab) in &header.tabs {
                assert!(tab.right() <= plus.left(), "at {width}: tab {index} runs under +");
                assert!(
                    header.strip.contains_rect(*tab),
                    "at {width}: tab {index} is drawn in part: {tab:?} in {:?}",
                    header.strip
                );
            }
            assert!(header.tabs.iter().any(|(index, _)| *index == shown), "at {width}: the tab shown is not drawn");
        }

        harness.size = vec2(900.0, 700.0);
        for tab in [0, 1] {
            harness.registry.dispatch(&mut harness.session, "terminal.tab.select", json!({ "tab": tab })).unwrap();
            harness.frame(Vec::new());
            harness.frame(Vec::new());
        }
        let drawn = |harness: &Harness| harness.view.terminal_header.tabs.iter().map(|(i, _)| *i).collect::<Vec<_>>();
        assert_eq!(drawn(&harness), [0, 1], "two tabs fit at 900");
        let over = harness.view.terminal_header.strip.center();
        let wheel = |delta: f32| Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, delta),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        };
        harness.frame(vec![Event::PointerMoved(over), wheel(-400.0)]);
        for _ in 0..10 {
            harness.frame(Vec::new());
        }
        assert_eq!(drawn(&harness), [1, 2], "scrolled down a tab, and no further: tab 1 is shown");
        harness.frame(vec![Event::PointerMoved(over), wheel(400.0)]);
        for _ in 0..10 {
            harness.frame(Vec::new());
        }
        assert_eq!(drawn(&harness), [0, 1], "and back");
    }

    fn with_caffeine() -> (Harness, zen_workbench::caffeine::Caffeine) {
        let mut harness = Harness::new();
        let caffeine = zen_workbench::caffeine::Caffeine::with(
            Box::new(zen_workbench::caffeine::keeper::FakeKeeper::default()),
            Arc::new(zen_workbench::caffeine::power::FakePower::new("Now drawing from 'AC Power'\n")),
        );
        harness.session.set_state(caffeine.clone());
        harness.frame(Vec::new());
        (harness, caffeine)
    }

    fn output(harness: &mut Harness) -> egui::FullOutput {
        harness.time += 0.1;
        let input = egui::RawInput {
            time: Some(harness.time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, harness.size)),
            ..Default::default()
        };
        let mut output = harness.ctx.run_ui(input, |ui| harness.view.show(ui, &harness.registry, &mut harness.session));
        output.textures_delta.clear();
        output
    }

    fn buttons_named(harness: &mut Harness, name: &str) -> Vec<Rect> {
        harness.ctx.enable_accesskit();
        let tree = output(harness).platform_output.accesskit_update.unwrap();
        tree.nodes
            .iter()
            .filter(|(_, node)| {
                node.role() == egui::accesskit::Role::Button
                    && crate::accessibility::accessible_name(node, &tree.nodes).as_deref() == Some(name)
            })
            .filter_map(|(_, node)| node.bounds())
            .map(|b| Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32)))
            .collect()
    }

    #[test]
    fn caffeine_is_a_tool_window_under_git_and_the_status_bar_has_no_widget() {
        let (mut harness, caffeine) = with_caffeine();
        caffeine.start(&zen_workbench::caffeine::StartParams::default(), std::time::Instant::now()).unwrap();
        let title = t!("ide-caffeine-title");
        let buttons = buttons_named(&mut harness, &title);
        let status_bar = harness.size.y - metrics::STATUS_BAR;
        assert!(
            buttons.iter().all(|rect| rect.center().y < status_bar),
            "no Caffeine widget in the status bar: {buttons:?}"
        );
        let right = harness.view.stripes.right.clone().unwrap();
        assert_eq!(buttons.len(), 1, "one button: the stripe's");
        assert!(buttons[0].left() >= right.rect.left(), "on the right stripe: {buttons:?}");
        assert_eq!(right.first.first().copied(), Some(buttons[0].center().y), "first under the Git workspace");
        assert_eq!(harness.state()["caffeine"], "steady", "the stripe cup steams");

        harness.action("view.toolWindow.caffeine");
        assert_eq!(harness.state()["right"], "caffeine");
        let turn_off = buttons_named(&mut harness, &t!("ide-caffeine-turn-off"));
        assert_eq!(turn_off.len(), 1, "the tool window has the controls");
        harness.click(turn_off[0].center());
        assert!(!caffeine.is_on(), "Turn Off turns Caffeine off");
        assert_eq!(harness.state()["caffeine"], "asleep");
        harness.action("view.toolWindow.caffeine");
        assert_eq!(harness.state()["right"], Value::Null, "the action hides it again");
    }

    #[test]
    fn the_coffee_drinker_asks_for_frames_only_while_his_tool_window_shows() {
        let (mut harness, caffeine) = with_caffeine();
        caffeine.start(&zen_workbench::caffeine::StartParams::default(), std::time::Instant::now()).unwrap();
        let delay = |harness: &mut Harness| output(harness).viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        let mut hidden = delay(&mut harness);
        for _ in 0..20 {
            if hidden >= std::time::Duration::from_secs(1) {
                break;
            }
            hidden = delay(&mut harness);
        }
        assert!(hidden >= std::time::Duration::from_secs(1), "hidden: no frames of his, {hidden:?}");
        harness.view.show_tool("caffeine");
        delay(&mut harness);
        let shown = delay(&mut harness);
        assert!(shown <= std::time::Duration::from_millis(400), "shown: his next frame, {shown:?}");
    }

    #[test]
    fn the_caffeine_controls_stay_put_whatever_the_state_the_time_the_frame_or_the_language() {
        use zen_workbench::caffeine::{Activity, Assertion, StartParams};
        for language in ["en", "ru"] {
            zen_i18n::with_language(language, || {
                let (mut harness, caffeine) = with_caffeine();
                harness.view.show_tool("caffeine");
                harness.frame(Vec::new());
                let names = [
                    t!("ide-caffeine-maximum"),
                    t!("ide-caffeine-for-ever-item"),
                    t!("ide-caffeine-while-item"),
                    t!("ide-caffeine-custom-start"),
                ];
                let turn = [t!("ide-caffeine-turn-on"), t!("ide-caffeine-turn-off")];
                let place = |harness: &mut Harness| -> Vec<(String, f32)> {
                    let mut places: Vec<(String, f32)> = names
                        .iter()
                        .map(|name| {
                            let found = buttons_named(harness, name);
                            assert!(!found.is_empty(), "{language}: {name}");
                            (name.clone(), found[0].top())
                        })
                        .collect();
                    let switch = turn
                        .iter()
                        .flat_map(|name| buttons_named(harness, name))
                        .map(|rect| rect.top())
                        .min_by(f32::total_cmp);
                    places.push(("Turn On/Off".to_owned(), switch.expect("Turn On or Turn Off")));
                    places
                };
                let first = place(&mut harness);
                let now = std::time::Instant::now();
                let states: Vec<(&str, StartParams)> = vec![
                    ("steady, until turned off", StartParams { minutes: Some(0), ..StartParams::default() }),
                    ("for 90 min: the time left shows", StartParams { minutes: Some(90), ..StartParams::default() }),
                    (
                        "the display for 15 min",
                        StartParams {
                            what: Some(vec![Assertion::Idle, Assertion::Display]),
                            minutes: Some(15),
                            ..StartParams::default()
                        },
                    ),
                    (
                        "every option: the longest description",
                        StartParams { what: Some(Assertion::ALL.to_vec()), minutes: Some(0), ..StartParams::default() },
                    ),
                    (
                        "waiting while something runs",
                        StartParams {
                            what: Some(Assertion::ALL.to_vec()),
                            during: Some(Activity::ALL.to_vec()),
                            ..StartParams::default()
                        },
                    ),
                ];
                for (state, params) in states {
                    caffeine.start(&params, now).unwrap();
                    for frame in 0..12 {
                        assert_eq!(place(&mut harness), first, "{language}: {state}, frame {frame}");
                    }
                }
                caffeine.stop(now);
                assert_eq!(place(&mut harness), first, "{language}: off again");
                let maximum = buttons_named(&mut harness, &t!("ide-caffeine-maximum"))[0];
                harness.click(maximum.center());
                let status = caffeine.status(std::time::Instant::now());
                assert!(status.on && status.maximum, "{language}: {status:?}");
                assert_eq!(harness.state()["caffeine"], "wired");
                assert_eq!(place(&mut harness), first, "{language}: at maximum");
                harness.click(maximum.center());
                assert!(!caffeine.is_on(), "{language}: the second click turns it off");
            });
        }
    }

    fn radio_items(harness: &mut Harness) -> Vec<(String, bool, Rect)> {
        harness.ctx.enable_accesskit();
        let tree = output(harness).platform_output.accesskit_update.unwrap();
        let mut items: Vec<(String, bool, Rect)> = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == egui::accesskit::Role::MenuItemRadio)
            .filter_map(|(_, node)| {
                let b = node.bounds()?;
                let rect = Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32));
                let name = crate::accessibility::accessible_name(node, &tree.nodes).unwrap_or_default();
                Some((name, node.is_selected().unwrap_or(false), rect))
            })
            .collect();
        items.sort_by(|a, b| a.2.top().total_cmp(&b.2.top()));
        items
    }

    fn view_mode_items(harness: &mut Harness) -> Vec<(String, bool, Rect)> {
        let view_mode = buttons_named(harness, &t!("ide-tool-view-mode"));
        assert_eq!(view_mode.len(), 1, "View Mode is in the menu");
        harness.click(view_mode[0].center());
        radio_items(harness)
    }

    fn mode_names() -> Vec<String> {
        ViewMode::ALL.iter().map(|mode| super::super::keymap_page::action_title(mode.action())).collect()
    }

    #[test]
    fn the_header_menu_offers_the_five_view_modes_and_the_stripe_menu_hide_and_move_to() {
        let mut harness = Harness::new();
        let options = buttons_named(&mut harness, &t!("ide-project-options"));
        harness.click(options[0].center());
        let items = view_mode_items(&mut harness);
        let names: Vec<String> = items.iter().map(|(name, ..)| name.clone()).collect();
        assert_eq!(names, mode_names(), "⋮ → View Mode: the five modes in order");
        assert_eq!(names, ["Dock Pinned", "Dock Unpinned", "Undock", "Float", "Window"]);
        let checked: Vec<&str> = items.iter().filter(|(_, on, _)| *on).map(|(name, ..)| name.as_str()).collect();
        assert_eq!(checked, ["Dock Pinned"], "the current one is checked");
        harness.click(items[3].2.center());
        assert_eq!(harness.view.tool_windows.mode_of("project"), Some(ViewMode::Float), "Float chosen");
        assert!(harness.view.tool_visible("project"), "and still shown, in its own window");
        assert_eq!(harness.state()["left"], Value::Null, "no longer docked at the left");
        assert!(!egui::Popup::is_any_open(&harness.ctx), "the menu closed");

        let left = harness.view.stripes.left.clone().unwrap();
        let project = pos2(left.rect.center().x, left.first[0]);
        let secondary = |pressed| Event::PointerButton {
            pos: project,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        harness.frame(vec![Event::PointerMoved(project)]);
        harness.frame(vec![secondary(true)]);
        harness.frame(vec![secondary(false)]);
        harness.frame(Vec::new());
        assert!(egui::Popup::is_any_open(&harness.ctx), "the stripe button's menu is open");
        assert_eq!(buttons_named(&mut harness, &t!("ide-tool-move-to")).len(), 1, "Move to");
        assert!(buttons_named(&mut harness, &t!("ide-tool-view-mode")).is_empty(), "no View Mode here");
        assert!(buttons_named(&mut harness, &t!("ide-tool-resize")).is_empty(), "no Resize here");
        egui::Popup::close_all(&harness.ctx);
        harness.action("view.toolWindow.viewMode/dockPinned");
        assert_eq!(harness.view.tool_windows.mode_of("project"), Some(ViewMode::DockPinned));
        assert_eq!(harness.state()["left"], "project", "docked again");
    }

    #[test]
    fn the_window_menu_checks_the_active_tool_windows_mode() {
        let mut harness = Harness::new();
        harness.view.show_tool("terminal");
        for mode in ViewMode::ALL {
            harness.action(mode.action());
            assert_eq!(harness.view.tool_windows.mode_of("terminal"), Some(mode), "{}", mode.name());
            let checked: Vec<ViewMode> = ViewMode::ALL
                .into_iter()
                .filter(|other| harness.view.action_checked(other.action(), &harness.session))
                .collect();
            assert_eq!(checked, [mode], "Window → Active Tool Window → View Mode checks {}", mode.name());
        }
        assert_eq!(harness.state()["toolWindows"]["active"], "terminal");
    }

    #[test]
    fn an_unpinned_tool_window_hides_when_something_else_is_clicked() {
        let mut harness = Harness::new();
        harness.view.show_tool("project");
        harness.action("view.toolWindow.viewMode/dockUnpinned");
        assert_eq!(harness.view.tool_windows.mode_of("project"), Some(ViewMode::DockUnpinned), "the active one");
        let project = harness.drawn("project").unwrap();
        harness.click(pos2(project.center().x, project.bottom() - 20.0));
        assert!(harness.view.tool_visible("project"), "a click in it keeps it");
        let left = harness.view.stripes.left.clone().unwrap();
        harness.click(pos2(left.rect.center().x, left.rect.center().y));
        assert!(harness.view.tool_visible("project"), "a click on the stripe is not a click elsewhere");
        harness.click(pos2(harness.size.x - 300.0, harness.size.y / 2.0));
        assert!(!harness.view.tool_visible("project"), "a click in the editor hides it");
        assert_eq!(harness.state()["left"], Value::Null);
        harness.click(pos2(left.rect.center().x, left.first[0]));
        assert!(harness.view.tool_visible("project"), "its stripe button shows it again");
        harness.action("view.toolWindow.terminal");
        assert!(!harness.view.tool_visible("project"), "another tool window taking the focus hides it");
        assert!(harness.view.tool_visible("terminal"));
    }

    #[test]
    fn an_undocked_tool_window_overlays_the_editor_and_hides_when_another_opens() {
        let mut harness = Harness::new();
        harness.view.show_tool("terminal");
        harness.frame(Vec::new());
        let docked = harness.drawn("terminal").unwrap();
        let layer = |harness: &Harness, rect: Rect| harness.ctx.layer_id_at(rect.center()).map(|layer| layer.order);
        assert_eq!(layer(&harness, docked), Some(Order::Background), "docked, it is beside the editor");
        harness.action("view.toolWindow.viewMode/undock");
        harness.frame(Vec::new());
        let undocked = harness.drawn("terminal").unwrap();
        assert_eq!(layer(&harness, undocked), Some(Order::Middle), "undocked, it is over the editor");
        assert!((undocked.height() - docked.height()).abs() < 1.0 && undocked.bottom() <= harness.size.y);
        assert_eq!(harness.state()["bottom"], "terminal");
        assert_eq!(harness.view.tool_windows.fitted_sizes(1000.0, 800.0)[1], 0.0, "the editor keeps the bottom");
        harness.click(pos2(harness.size.x / 2.0, 200.0));
        assert!(harness.view.tool_visible("terminal"), "a click in the editor leaves it");
        harness.action("view.toolWindow.problems");
        assert!(!harness.view.tool_visible("terminal"), "another tool window opening hides it");
        assert!(harness.view.tool_visible("problems"));
        harness.action("view.toolWindow.terminal");
        harness.frame(Vec::new());
        assert!(harness.view.tool_visible("terminal") && !harness.view.tool_visible("problems"), "one a side");
    }

    #[test]
    fn the_view_mode_and_layout_actions_answer_agents() {
        let mut harness = Harness::new();
        let ctx = harness.ctx.clone();
        let run = |harness: &mut Harness, action: &str, params: Value| {
            harness.view.layout_action(&ctx, action, &params).map_err(|error| match error {
                ActionError::Refused(message) => message,
                ActionError::Dispatch(error) => error.to_string(),
            })
        };
        let done = run(&mut harness, "view.toolWindow.viewMode", json!({ "id": "git", "mode": "window" })).unwrap();
        assert_eq!(done, json!({ "id": "git", "mode": "window", "visible": false }));
        assert!(
            run(&mut harness, "view.toolWindow.viewMode", json!({ "id": "git" })).unwrap_err().contains("dockPinned")
        );
        assert!(
            run(&mut harness, "view.toolWindow.viewMode", json!({ "mode": "sideways" })).unwrap_err().contains("float")
        );
        harness.view.hide_tool("project");
        assert!(
            run(&mut harness, "view.toolWindow.viewMode", json!({ "mode": "float" })).unwrap_err().contains("active")
        );
        let state = harness.state();
        let git = state["toolWindows"]["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|window| window["id"] == "git")
            .unwrap()
            .clone();
        assert_eq!(git["mode"], "window");
        assert_eq!(state["toolWindows"]["layouts"], json!({ "saved": [], "current": "default" }));

        assert!(run(&mut harness, "view.layout.apply", json!({})).unwrap_err().contains("default"));
        assert!(run(&mut harness, "view.layout.apply", json!({ "name": "Nope" })).unwrap_err().contains("Nope"));
        assert_eq!(
            run(&mut harness, "view.layout.saveAsNew", json!({ "name": "Git" })).unwrap(),
            json!({ "name": "Git" })
        );
        assert!(
            run(&mut harness, "view.layout.saveAsNew", json!({ "name": "git" })).unwrap_err().contains("saved already")
        );
        assert!(
            run(&mut harness, "view.layout.saveAsNew", json!({ "name": "Default" }))
                .unwrap_err()
                .contains("default layout")
        );
        assert_eq!(harness.state()["toolWindows"]["layouts"], json!({ "saved": ["Git"], "current": "Git" }));
        let kept = harness.view.requests.iter().rev().find_map(|request| match request {
            super::super::app::Request::Layouts(saved) => Some(saved.clone()),
            _ => None,
        });
        assert_eq!(kept.unwrap().layouts.len(), 1, "the application is asked to keep it for the IDE");

        run(&mut harness, "view.layout.restoreDefault", json!({})).unwrap();
        assert_eq!(
            harness.view.tool_windows.mode_of("git"),
            Some(ViewMode::DockPinned),
            "the default layout resets modes"
        );
        assert_eq!(harness.state()["toolWindows"]["layouts"]["current"], "default");
        run(&mut harness, "view.layout.apply", json!({ "name": "git" })).unwrap();
        assert_eq!(harness.view.tool_windows.mode_of("git"), Some(ViewMode::Window), "the saved layout has its modes");
        harness.view.set_view_mode("terminal", ViewMode::Undock);
        run(&mut harness, "view.layout.restore", json!({})).unwrap();
        assert_eq!(harness.view.tool_windows.mode_of("terminal"), Some(ViewMode::DockPinned), "Restore: as saved");
        harness.view.set_view_mode("terminal", ViewMode::Float);
        assert_eq!(run(&mut harness, "view.layout.saveChanges", json!({})).unwrap(), json!({ "saved": "Git" }));
        harness.view.set_view_mode("terminal", ViewMode::DockPinned);
        run(&mut harness, "view.layout.restore", json!({ "name": "Git" })).unwrap();
        assert_eq!(harness.view.tool_windows.mode_of("terminal"), Some(ViewMode::Float), "the changes were saved");
        assert!(
            run(&mut harness, "view.layout.delete", json!({ "name": "git" })).unwrap_err().contains("current layout")
        );
        assert_eq!(
            run(&mut harness, "view.layout.rename", json!({})).unwrap(),
            json!({ "dialog": "renameLayout" }),
            "without a new name it asks for one"
        );
        assert!(harness.state()["popups"].as_array().unwrap().contains(&json!("renameLayout")));
        harness.view.layout_form = None;
        assert_eq!(
            run(&mut harness, "view.layout.rename", json!({ "newName": "Review" })).unwrap(),
            json!({ "name": "Review" })
        );
        assert_eq!(harness.state()["toolWindows"]["layouts"], json!({ "saved": ["Review"], "current": "Review" }));
        assert!(
            run(&mut harness, "view.layout.rename", json!({ "newName": "default" })).unwrap_err().contains("default")
        );

        run(&mut harness, "view.layout.apply", json!({ "name": "default" })).unwrap();
        assert_eq!(harness.view.tool_windows.mode_of("git"), Some(ViewMode::DockPinned));
        assert!(run(&mut harness, "view.layout.saveChanges", json!({})).unwrap_err().contains("default layout"));
        run(&mut harness, "view.layout.restore", json!({})).unwrap();
        assert_eq!(harness.state()["toolWindows"]["layouts"]["current"], "default", "Restore keeps the default");
        assert!(run(&mut harness, "view.layout.delete", json!({})).unwrap_err().contains("give `name`"));
        assert_eq!(
            run(&mut harness, "view.layout.delete", json!({ "name": "review", "ask": true })).unwrap(),
            json!({ "dialog": "deleteLayout" })
        );
        harness.frame(Vec::new());
        harness.press(Key::Enter, Modifiers::NONE);
        assert!(!harness.view.layout_dialog_open(), "Delete… confirmed");
        assert_eq!(harness.state()["toolWindows"]["layouts"], json!({ "saved": [], "current": "default" }));
        run(&mut harness, "view.layout.saveAsNew", json!({ "name": "Git" })).unwrap();
        run(&mut harness, "view.layout.apply", json!({ "name": "default" })).unwrap();
        assert_eq!(
            run(&mut harness, "view.layout.delete", json!({ "name": "git" })).unwrap(),
            json!({ "deleted": "Git" })
        );
        assert_eq!(harness.state()["toolWindows"]["layouts"], json!({ "saved": [], "current": "default" }));
    }

    #[test]
    fn save_current_layout_as_new_asks_for_a_name() {
        let mut harness = Harness::new();
        harness.view.set_view_mode("project", ViewMode::Undock);
        harness.action("view.layout.saveAsNew");
        assert!(harness.state()["popups"].as_array().unwrap().contains(&json!("saveLayout")));
        harness.frame(Vec::new());
        harness.press(Key::Enter, Modifiers::NONE);
        assert!(harness.view.layout_dialog_open(), "no name, nothing saved");
        harness.frame(vec![Event::Text("Default".to_owned())]);
        harness.press(Key::Enter, Modifiers::NONE);
        assert!(harness.view.layout_dialog_open(), "the default layout's name is refused");
        for _ in 0.."Default".len() {
            harness.press(Key::Backspace, Modifiers::NONE);
        }
        harness.frame(vec![Event::Text("Overlay".to_owned())]);
        harness.press(Key::Enter, Modifiers::NONE);
        assert!(!harness.view.layout_dialog_open());
        assert_eq!(harness.state()["toolWindows"]["layouts"], json!({ "saved": ["Overlay"], "current": "Overlay" }));
        let items = harness.view.layout_items();
        let shown: Vec<(&str, bool)> = items.iter().map(|(title, _, on)| (title.as_str(), *on)).collect();
        assert_eq!(shown, [("Default", false), ("Overlay", true)], "the current layout is checked");
        assert_eq!(
            harness.view.overview.layouts.get("overlay").unwrap().layout.mode_of("project"),
            Some(ViewMode::Undock)
        );
    }

    fn maven_plugin() -> zen_workbench::plugin_projects::PluginProject {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/maven/plugin.json");
        let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        zen_workbench::plugin_projects::PluginProject {
            id: "maven".to_owned(),
            name: "Maven".to_owned(),
            support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
        }
    }

    #[derive(Default)]
    struct Recorded;

    impl zen_workbench::dependencies::sources::Transport for Recorded {
        fn get(
            &self,
            url: &str,
            _: &zen_workbench::dependencies::sources::Validators,
            _: Option<&str>,
        ) -> Result<zen_workbench::dependencies::sources::Answer, String> {
            use zen_workbench::dependencies::sources::{Answer, Validators};
            let body = if url.contains("/select?") {
                r#"{"response":{"docs":[{"g":"com.squareup","a":"okio","latestVersion":"1.0"}]}}"#
            } else if url.contains("okio") {
                "<metadata><versioning><versions><version>3.9.0</version></versions></versioning></metadata>"
            } else if url.contains("slf4j") {
                "<metadata><versioning><versions><version>2.0.0</version><version>2.1.0</version></versions></versioning></metadata>"
            } else {
                return Ok(Answer::Missing);
            };
            Ok(Answer::Changed { body: body.as_bytes().to_vec(), validators: Validators::default() })
        }

        fn post(&self, _: &str, _: &[u8]) -> Result<Vec<u8>, String> {
            Ok(br#"{"vulns":[]}"#.to_vec())
        }
    }

    #[test]
    fn the_dependencies_tool_window_lists_a_module_searches_adds_and_removes() {
        let mut harness = Harness::new();
        zen_i18n::set_language(Some("en"));
        let listed =
            |harness: &Harness| harness.view.stripes.ids.iter().flatten().any(|shown| *shown == "dependencies");
        let wait = |harness: &mut Harness| {
            for _ in 0..12 {
                harness.frame(Vec::new());
            }
        };
        let pom = harness.project.path().join("pom.xml");
        std::fs::write(
            &pom,
            "<project>\n  <groupId>demo</groupId>\n  <artifactId>shop</artifactId>\n  <version>1</version>\n  <dependencies>\n    <dependency>\n      <groupId>org.slf4j</groupId>\n      <artifactId>slf4j-api</artifactId>\n      <version>2.0.0</version>\n    </dependency>\n  </dependencies>\n</project>\n",
        )
        .unwrap();
        wait(&mut harness);
        harness.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(Vec::new());
        wait(&mut harness);
        assert!(!listed(&harness), "not without the Maven plug-in");
        harness.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![maven_plugin()]);
        wait(&mut harness);
        assert!(listed(&harness), "on the bottom stripe with the plug-in");
        let service = harness.session.state_mut::<zen_workbench::dependencies::Dependencies>().clone();
        service.set_transport(Arc::new(Recorded));
        for key in [zen_workbench::dependencies::SEARCH_SETTING, zen_workbench::dependencies::CHECK_UPDATES] {
            harness.session.settings_mut().set(key, json!(true)).unwrap();
        }
        harness.action("view.toolWindow.dependencies");
        let ready = |harness: &mut Harness, service: &zen_workbench::dependencies::Dependencies| {
            for _ in 0..40 {
                service.wait_idle(std::time::Duration::from_secs(5));
                harness.view.dependencies.force_read();
                harness.frame(Vec::new());
            }
        };
        ready(&mut harness, &service);
        assert_eq!(harness.state()["bottom"], "dependencies");
        let state = harness.state()["buildTools"]["dependencies"].clone();
        assert_eq!(state["module"], "pom.xml");
        assert_eq!(state["rows"][0]["id"], "org.slf4j:slf4j-api:compile");
        assert_eq!(state["rows"][0]["note"], "2.1.0 is available");
        assert!(harness.text("slf4j-api").is_some() && harness.text("Modules").is_none());
        let context = harness.ctx.clone();
        harness
            .view
            .perform_action(
                &context,
                &harness.registry,
                &mut harness.session,
                "view.dependencies.select",
                json!({ "row": "org.slf4j:slf4j-api:compile" }),
                false,
            )
            .unwrap();
        harness.frame(Vec::new());
        assert!(harness.text("Upgrade").is_some() && harness.text("Remove").is_some());
        harness
            .view
            .perform_action(
                &context,
                &harness.registry,
                &mut harness.session,
                "view.dependencies.search",
                json!({ "query": "okio" }),
                false,
            )
            .unwrap();
        ready(&mut harness, &service);
        let state = harness.state()["buildTools"]["dependencies"].clone();
        assert_eq!(state["results"][0]["latest"], "3.9.0", "from the metadata, never the index");
        let result = harness.text("com.squareup:okio").expect("the result is shown");
        harness.click(result);
        harness.frame(Vec::new());
        let add = harness.text("Add").expect("the add form is shown");
        harness.click(add);
        ready(&mut harness, &service);
        let text = std::fs::read_to_string(&pom).unwrap();
        assert!(text.contains("<artifactId>okio</artifactId>\n      <version>3.9.0</version>"), "{text}");
        assert_eq!(harness.view.dependencies.selected_row(), Some("org.slf4j:slf4j-api:compile"));
        assert_eq!(harness.state()["buildTools"]["dependencies"]["query"], "", "back to the module's list");
        let upgrade = harness.text("Upgrade").expect("the row's upgrade");
        harness.click(upgrade);
        ready(&mut harness, &service);
        assert!(std::fs::read_to_string(&pom).unwrap().contains("<version>2.1.0</version>"));
        let remove = harness.text("Remove").expect("the row's remove");
        harness.click(remove);
        ready(&mut harness, &service);
        assert!(!std::fs::read_to_string(&pom).unwrap().contains("slf4j-api"));
    }

    #[test]
    fn the_maven_tool_window_is_listed_only_where_the_plugin_supports_the_project() {
        let mut harness = Harness::new();
        zen_i18n::set_language(Some("en"));
        let listed = |harness: &Harness, id: &str| harness.view.stripes.ids.iter().flatten().any(|shown| *shown == id);
        let wait = |harness: &mut Harness| {
            for _ in 0..12 {
                harness.frame(Vec::new());
            }
        };
        assert!(!listed(&harness, "maven"), "not a Maven project");
        assert!(
            harness
                .view
                .perform_action(
                    &harness.ctx.clone(),
                    &harness.registry,
                    &mut harness.session,
                    "view.toolWindow.maven",
                    json!({}),
                    false
                )
                .is_err()
        );
        std::fs::write(
            harness.project.path().join("pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><groupId>demo</groupId><artifactId>shop</artifactId><version>1</version></project>",
        )
        .unwrap();
        wait(&mut harness);
        assert!(!listed(&harness, "maven"), "a pom.xml without the Maven plug-in");
        harness.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![maven_plugin()]);
        wait(&mut harness);
        assert!(harness.view.build_tools.maven, "the Maven tool window applies");
        assert!(listed(&harness, "maven"), "on the right stripe with the plug-in");
        assert!(!listed(&harness, "gradle"), "not a Gradle build");
        harness.action("view.toolWindow.maven");
        harness.frame(Vec::new());
        assert_eq!(harness.state()["right"], "maven");
        assert!(harness.text("shop").is_some() && harness.text("Lifecycle").is_some(), "the project and its lifecycle");
        harness.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(Vec::new());
        wait(&mut harness);
        assert!(!listed(&harness, "maven"), "the plug-in switched off");
        assert_ne!(harness.state()["right"], "maven", "and its tool window hidden");
    }

    #[cfg(unix)]
    #[test]
    fn one_banner_says_why_a_maven_project_does_not_load_and_goes_when_the_reason_does() {
        use std::os::unix::fs::PermissionsExt;
        let mut harness = Harness::new();
        zen_i18n::set_language(Some("en"));
        let wait = |harness: &mut Harness| {
            for _ in 0..12 {
                harness.frame(Vec::new());
            }
        };
        let tools = tempfile::tempdir().unwrap();
        let env = zen_workbench::build_tools::BuildEnv::default();
        env.set_folders(Some(vec![tools.path().to_owned()]));
        env.set_home(Some(tools.path().join("home")));
        harness.session.set_state(env);
        std::fs::write(
            harness.project.path().join("pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><groupId>demo</groupId><artifactId>shop</artifactId><version>1</version></project>",
        )
        .unwrap();
        harness.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![maven_plugin()]);
        wait(&mut harness);
        harness.action("view.toolWindow.maven");
        wait(&mut harness);
        assert!(harness.text("Download Maven").is_some(), "the tool is the first reason");
        assert!(harness.text("Trust Project").is_none(), "one banner at a time");

        let mvn = tools.path().join("mvn");
        std::fs::write(&mvn, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&mvn, std::fs::Permissions::from_mode(0o755)).unwrap();
        harness.view.build_tools.read_now(
            super::super::build_tool_window::Tool::Maven,
            &harness.registry,
            &mut harness.session,
        );
        wait(&mut harness);
        assert!(harness.text("Download Maven").is_none(), "gone without a restart");
        let trust = harness.text("Trust Project").expect("the next reason");
        harness.click(trust);
        wait(&mut harness);
        assert!(harness.text("Trust Project").is_none(), "the person trusted it");
        let project = harness.project.path().to_owned();
        assert!(zen_workbench::trust::trusted(&harness.session, &project));
    }

    #[test]
    fn an_edited_pom_shows_load_changes_and_a_failed_sync_opens_the_build_window() {
        let mut harness = Harness::new();
        zen_i18n::set_language(Some("en"));
        let wait = |harness: &mut Harness| {
            for _ in 0..12 {
                harness.frame(Vec::new());
            }
        };
        let env = zen_workbench::build_tools::BuildEnv::default();
        env.set_folders(Some(Vec::new()));
        harness.session.set_state(env);
        let pom = harness.project.path().join("pom.xml");
        std::fs::write(&pom, "<project><modelVersion>4.0.0</modelVersion><artifactId>shop</artifactId></project>\n")
            .unwrap();
        harness.registry.dispatch(&mut harness.session, "project.trust.grant", json!({})).unwrap();
        harness.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![maven_plugin()]);
        wait(&mut harness);
        assert_eq!(harness.state()["buildSync"]["pending"], json!([]), "loaded as it opened");
        assert!(listed(&harness, "build"), "the Build tool window applies to a Maven project");
        harness.registry.dispatch(&mut harness.session, "file.document.open", json!({ "path": pom })).unwrap();
        let document = harness.session.state_mut::<zen_workbench::Workbench>().active_document_mut().unwrap();
        assert!(document.replace_ranges(vec![(0..0, "<!-- x -->".to_owned())]));
        wait(&mut harness);
        let pending = harness.state()["buildSync"]["pending"].clone();
        assert_eq!((pending[0]["id"].clone(), pending[0]["autoReload"].clone()), (json!("maven"), json!(false)));
        assert!(harness.view.build_sync_button_for(&pom), "the button shows in the pom.xml editor");
        harness.press(Key::I, Modifiers::COMMAND | Modifiers::SHIFT);
        wait(&mut harness);
        assert!(std::fs::read_to_string(&pom).unwrap().starts_with("<!-- x -->"), "saved before loading");
        assert_eq!(harness.state()["buildSync"]["pending"], json!([]), "loaded, the button hides");
        assert!(harness.view.tool_visible("build"), "the failed sync shows the Build tool window");
        let rows = harness.state()["buildSync"]["rows"].clone();
        assert_eq!(rows[0]["state"], "failed", "{rows}");
        let fix = rows.as_array().unwrap().iter().find_map(|row| row.get("fix")).cloned().unwrap();
        assert_eq!(fix["command"], "maven.distribution.download");
        assert!(harness.text("Sync").is_some() && harness.text("Maven: failed").is_some(), "{rows}");
    }

    #[test]
    fn a_person_only_command_an_agent_put_in_a_menu_stays_refused_when_the_person_clicks_it() {
        let mut harness = Harness::new();
        let root = harness.project.path().to_owned();
        let env = zen_workbench::build_tools::BuildEnv::default();
        harness.session.set_state(env.clone());
        let click = |harness: &mut Harness, from_agent: bool| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, harness.size)),
                ..Default::default()
            };
            let (ctx, view, registry, session) =
                (harness.ctx.clone(), &mut harness.view, &harness.registry, &mut harness.session);
            let _ = ctx.run_ui(input, |ui| {
                view.run_customized_as(ui, registry, session, "project.trust.grant", from_agent);
            });
        };
        click(&mut harness, true);
        assert!(!zen_workbench::trust::is_trusted(&env, &root), "the agent's item runs as the agent");
        click(&mut harness, false);
        assert!(zen_workbench::trust::is_trusted(&env, &root), "the person's own item runs as the person");
    }

    #[test]
    fn analyze_dependencies_shows_a_conflict_and_exclude_edits_the_pom() {
        let mut harness = Harness::new();
        zen_i18n::set_language(Some("en"));
        let wait = |harness: &mut Harness| {
            for _ in 0..12 {
                harness.frame(Vec::new());
            }
        };
        let env = zen_workbench::build_tools::BuildEnv::default();
        env.set_folders(Some(Vec::new()));
        harness.session.set_state(env.clone());
        let pom = harness.project.path().join("pom.xml");
        std::fs::write(
            &pom,
            "<project>\n  <groupId>demo</groupId>\n  <artifactId>app</artifactId>\n  <dependencies>\n    <dependency>\n      <groupId>h</groupId>\n      <artifactId>client</artifactId>\n      <version>4</version>\n    </dependency>\n  </dependencies>\n</project>\n",
        )
        .unwrap();
        harness.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![maven_plugin()]);
        wait(&mut harness);
        use zen_workbench::dependency_analysis::DependencyAnalyzer;
        let root = harness.session.state::<zen_workbench::Workbench>().unwrap().project().unwrap().to_owned();
        let key = zen_workbench::maven::analysis::MavenAnalyzer.key(&root);
        let output = "[INFO] demo:app:jar:1.0\n[INFO] +- h:client:jar:4:compile\n[INFO] |  \\- (c:codec:jar:1.11:compile - omitted for conflict with 1.10)\n[INFO] \\- c:codec:jar:1.10:compile\n";
        env.set_load(
            &key,
            zen_workbench::build_tools::Load {
                state: zen_workbench::build_tools::LoadState::Loaded,
                output: output.to_owned(),
                error: None,
                task: None,
                started: std::time::Instant::now(),
                took: None,
            },
        );
        harness.action("view.dependencyAnalyzer.open");
        wait(&mut harness);
        assert!(harness.view.tool_visible("build"));
        assert!(harness.text("Dependency Analyzer").is_some() && harness.text("c:codec").is_some());
        let state = harness.state()["dependencyAnalyzer"].clone();
        assert_eq!(state["module"], "demo:app", "{state}");
        let rows = state["rows"].as_array().unwrap();
        assert_eq!(rows[0]["mark"], "conflict");
        assert_eq!(rows[0]["excludable"], true);

        let set = |harness: &mut Harness, action: &str, params: Value| {
            let (ctx, registry) = (harness.ctx.clone(), &harness.registry);
            harness.view.perform_action(&ctx, registry, &mut harness.session, action, params, true).unwrap()
        };
        let conflicts =
            set(&mut harness, "view.dependencyAnalyzer.set", json!({ "conflictsOnly": true, "tree": true }));
        let rows = conflicts["rows"].as_array().unwrap().clone();
        let labels: Vec<&str> = rows.iter().map(|row| row["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["h:client", "c:codec"], "the conflict with what brings it: {conflicts}");
        assert_eq!(rows[1]["via"], "h:client");
        let row = rows[1]["id"].clone();
        let selected = set(&mut harness, "view.dependencyAnalyzer.select", json!({ "row": row }));
        assert!(selected["details"][1].as_str().unwrap().contains("1.10"), "{selected}");
        wait(&mut harness);
        assert!(harness.text("Exclude").is_some() && harness.text("Go to Declaration").is_some());
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 700.0), 2.0);
            crate::theme::install(screen.context());
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                let (view, registry, session) = (&mut harness.view, &harness.registry, &mut harness.session);
                for _ in 0..3 {
                    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
                }
                let image = screen.frame(Vec::new(), |ui| view.show(ui, registry, session)).into_image();
                let file = Path::new(&dir).join(format!("dependency-analyzer-{language}.png"));
                crate::offscreen::write_png(&file, &image).unwrap();
            }
            zen_i18n::set_language(Some("en"));
        }

        let exclude = json!({ "groupId": "c", "artifactId": "codec", "via": "h:client" });
        harness.registry.dispatch(&mut harness.session, "build.dependencies.exclude", exclude).unwrap();
        let text = std::fs::read_to_string(&pom).unwrap();
        assert!(
            text.contains(
                "<version>4</version>\n      <exclusions>\n        <exclusion>\n          <groupId>c</groupId>"
            ),
            "{text}"
        );
        let place = json!({ "groupId": "c", "artifactId": "codec" });
        harness.registry.dispatch(&mut harness.session, "build.dependencies.declaration", place).unwrap();
        wait(&mut harness);
        let document = harness.session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.path().file_name().unwrap(), "pom.xml");

        set(&mut harness, "view.dependencyAnalyzer.set", json!({ "shown": false }));
        wait(&mut harness);
        assert!(harness.text("Dependency Analyzer").is_some() && harness.text("Sync").is_some());

        harness.action("view.toolWindow.maven");
        wait(&mut harness);
        let rows = harness.state()["buildTools"]["maven"]["rows"].clone();
        let head = rows.as_array().unwrap().iter().find(|row| row["label"] == "app").unwrap();
        let menu = set(&mut harness, "view.buildTool.menu", json!({ "tool": "maven", "row": head["id"] }));
        let items = menu["items"].as_array().unwrap();
        let analyze = items.iter().find(|item| item["label"] == "Analyze Dependencies").unwrap();
        let command = analyze["command"].as_str().unwrap();
        set(&mut harness, command, analyze["params"].clone());
        wait(&mut harness);
        let state = harness.state()["dependencyAnalyzer"].clone();
        assert_eq!((state["tab"].clone(), state["module"].clone()), (json!(true), json!("demo:app")), "{state}");
        assert!(harness.view.tool_visible("build"), "the project's menu shows the analyzer");
    }

    struct Initializr;

    impl zen_project::generators::ProjectGenerator for Initializr {
        fn info(&self) -> zen_project::generators::GeneratorInfo {
            zen_project::generators::GeneratorInfo {
                id: "spring-boot.initializr".to_owned(),
                ..zen_project::generators::GeneratorInfo::default()
            }
        }

        fn form(&self) -> Result<zen_project::generators::GeneratorForm, String> {
            Ok(zen_project::generators::GeneratorForm::default())
        }

        fn files(
            &self,
            _request: &zen_project::generators::GeneratorRequest,
        ) -> Result<Vec<zen_project::generators::GeneratedFile>, String> {
            Ok(Vec::new())
        }

        fn step(
            &self,
            _step: &str,
            _values: zen_project::generators::Values,
        ) -> Result<zen_project::generators::StepAnswer, String> {
            let answer = json!({ "starters": {
                "field": { "id": "dependencies", "label": "Dependencies", "type": "select", "groups": [{
                    "name": "Web",
                    "options": [
                        { "value": "web", "label": "Spring Web" },
                        { "value": "ws", "label": "WebSocket" },
                        { "value": "old", "label": "Old Thing", "disabledNote": "Requires Spring Boot >=9",
                          "enabledWhen": { "field": "bootVersion", "anyOf": [] } }
                    ],
                }] },
                "ticked": ["web"],
                "dependencies": {
                    "web": { "groupId": "o.s", "artifactId": "starter-web", "scope": "compile" },
                    "ws": { "groupId": "o.s", "artifactId": "starter-ws", "scope": "compile" },
                },
            } });
            Ok(zen_project::generators::StepAnswer { answer, from_saved: true })
        }
    }

    #[test]
    fn edit_starters_shows_what_the_module_has_and_edits_its_pom() {
        let mut harness = Harness::new();
        zen_i18n::set_language(Some("en"));
        let env = zen_workbench::build_tools::BuildEnv::default();
        env.set_folders(Some(Vec::new()));
        harness.session.set_state(env);
        let pom = harness.project.path().join("pom.xml");
        std::fs::write(
            &pom,
            "<project>\n  <parent>\n    <artifactId>spring-boot-starter-parent</artifactId>\n    <version>4.1.2</version>\n  </parent>\n  <artifactId>app</artifactId>\n  <dependencies>\n    <dependency>\n      <groupId>o.s</groupId>\n      <artifactId>starter-web</artifactId>\n    </dependency>\n  </dependencies>\n</project>\n",
        )
        .unwrap();
        let spring = zen_workbench::plugin_projects::PluginProject {
            id: zen_workbench::spring_boot::PLUGIN.to_owned(),
            name: "Spring Boot".to_owned(),
            support: zen_project::support::ProjectSupport::default(),
        };
        harness.session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![maven_plugin(), spring]);
        zen_workbench::dependencies::service(&mut harness.session)
            .set_providers(vec![Arc::new(zen_workbench::maven::dependencies::MavenDependencies)]);
        let projects = zen_workbench::new_project::NewProjects::default();
        projects.declare(Arc::new(Initializr));
        harness.session.set_state(projects);
        let (ctx, registry) = (harness.ctx.clone(), &harness.registry);
        let opened = harness.view.perform_action(
            &ctx,
            registry,
            &mut harness.session,
            "view.startersDialog.open",
            json!({ "file": std::fs::canonicalize(&pom).unwrap() }),
            true,
        );
        opened.unwrap();
        for _ in 0..30 {
            harness.frame(Vec::new());
            if harness.state()["starters"]["loaded"] == "ready" {
                break;
            }
            std::thread::yield_now();
            harness
                .registry
                .dispatch(&mut harness.session, "springBoot.starters.get", json!({ "waitMs": 1000 }))
                .unwrap();
        }
        harness.frame(Vec::new());
        let state = harness.state()["starters"].clone();
        assert_eq!(state["bootVersion"], "4.1.2", "{state}");
        assert_eq!(state["ticked"], json!(["web"]), "{state}");
        assert!(state["notice"].is_string(), "a list from a saved answer says so: {state}");
        assert!(harness.text("Edit Starters").is_some() && harness.text("WebSocket").is_some());
        assert!(harness.text("Requires Spring Boot >=9").is_some(), "a starter the version cannot take is disabled");

        let (ctx, registry) = (harness.ctx.clone(), &harness.registry);
        let session = &mut harness.session;
        let params = json!({ "ticked": ["ws"] });
        let state = harness.view.perform_action(&ctx, registry, session, "view.startersDialog.set", params, true);
        let state = state.unwrap();
        assert_eq!((state["add"].clone(), state["remove"].clone()), (json!(["ws"]), json!(["web"])), "{state}");
        harness.view.perform_action(&ctx, registry, session, "view.startersDialog.apply", json!({}), true).unwrap();
        harness.frame(Vec::new());
        assert!(harness.state()["starters"].is_null(), "the dialog closes");
        let text = std::fs::read_to_string(&pom).unwrap();
        assert!(text.contains("<artifactId>starter-ws</artifactId>") && !text.contains("starter-web"), "{text}");
    }

    fn listed(harness: &Harness, id: &str) -> bool {
        harness.view.stripes.ids.iter().flatten().any(|shown| *shown == id)
    }
}
