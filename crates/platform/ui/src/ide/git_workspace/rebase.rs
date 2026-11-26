use std::collections::HashMap;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::rebase::{RebaseAction, RebaseEditor, RebaseKind, RebasePlan, RebaseStepInfo};

use super::super::glyphs::{glyph_grip, glyph_rebase};
use super::super::vcs::format_date;
use super::look::{self, Tint};
use super::{
    HEADER, IdePalette, IdeView, SUMMARY, Side, TOOLBAR, commit_message, draw_avatar, framed_field, hint, hover,
    lane_color, message_fields,
};
use crate::theme;
use zen_workbench::vcs::workspace::{REBASE_TAB, WORKSPACE as GIT};
use zen_workbench::workspaces::Workspaces;

const ROW: f32 = 34.0;
const GRIP: f32 = 26.0;
const ACTION: f32 = 96.0;
const GRAPH: f32 = 40.0;
const AUTHOR: f32 = 140.0;
const DATE: f32 = 128.0;
const SHA: f32 = 74.0;
const MESSAGE: f32 = 240.0;

#[derive(Default)]
pub(in crate::ide) struct RebaseView {
    seen_raised: u64,
    seen_revision: u64,
    plan: Option<RebasePlan>,
    problem: Option<String>,
    changed: bool,
    selected: usize,
    dragged: Option<usize>,
    action_menu: Option<(usize, egui::Pos2)>,
    drafts: HashMap<String, (String, String)>,
    focus_summary: bool,
}

enum Ask {
    Edit(serde_json::Value),
    Run(&'static str),
}

impl RebaseView {
    fn rows(&self) -> usize {
        self.plan.as_ref().map_or(0, |plan| plan.steps.len())
    }

    fn step_of(&self, row: usize) -> usize {
        self.rows().saturating_sub(1).saturating_sub(row)
    }
}

impl IdeView {
    pub(in crate::ide) fn rebase_in_front(&self, session: &Session) -> bool {
        self.git_tab_in_front(session, REBASE_TAB) && session.state::<RebaseEditor>().is_some_and(RebaseEditor::is_open)
    }

    pub(in crate::ide) fn rebase_title(&self, session: &Session) -> String {
        match session.state::<RebaseEditor>().and_then(RebaseEditor::plan).map(|plan| plan.kind) {
            Some(RebaseKind::CherryPick) => t!("vcs-rebase-tab-cherry-pick"),
            _ => t!("vcs-rebase-tab"),
        }
    }

    pub(in crate::ide) fn rebase_frame(&mut self, session: &mut Session) {
        let Some(editor) = session.state::<RebaseEditor>() else { return };
        let (open, raised) = (editor.is_open(), editor.raised());
        if !open {
            session.state_mut::<Workspaces>().remove_tab(GIT, REBASE_TAB);
            self.rebase_view.plan = None;
            return;
        }
        Self::put_git_tab(session, REBASE_TAB, self.rebase_title(session));
        if raised != self.rebase_view.seen_raised {
            self.rebase_view.seen_raised = raised;
            self.rebase_view.selected = 0;
            self.rebase_view.drafts.clear();
            zen_workbench::workspaces::bring(session, GIT, REBASE_TAB);
            self.close_text_dialogs();
        }
    }

    fn read_rebase(&mut self, registry: &Registry, session: &mut Session) {
        let revision = session.state::<RebaseEditor>().map_or(0, RebaseEditor::revision);
        let view = &mut self.rebase_view;
        if view.plan.is_some() && revision == view.seen_revision {
            return;
        }
        view.seen_revision = revision;
        let state = registry
            .dispatch(session, "vcs.rebase.state", json!({}))
            .ok()
            .and_then(|state| serde_json::from_value::<zen_workbench::vcs::rebase::RebaseState>(state).ok())
            .unwrap_or_default();
        view.plan = state.plan;
        view.problem = state.problem;
        view.changed = state.changed;
        view.selected = view.selected.min(view.rows().saturating_sub(1));
        let Some(plan) = &view.plan else { return };
        view.drafts.retain(|id, _| plan.steps.iter().any(|step| step.id == *id && step.action == RebaseAction::Reword));
        for step in &plan.steps {
            if step.action == RebaseAction::Reword {
                let fields = message_fields(&step.new_message());
                let draft = view.drafts.entry(step.id.clone()).or_insert_with(|| fields.clone());
                if commit_message(&draft.0, &draft.1) != commit_message(&fields.0, &fields.1) {
                    *draft = fields;
                }
            }
        }
    }

    pub(in crate::ide) fn rebase_editor(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        self.caret = None;
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        ui.painter().rect_filled(full, 0, p.editor_background);
        self.read_rebase(registry, session);
        let Some(plan) = self.rebase_view.plan.clone() else { return };
        let mut asks = Vec::new();
        let toolbar = Rect::from_min_size(full.min, vec2(full.width(), TOOLBAR));
        rebase_toolbar(ui, toolbar, &p, &plan, &self.rebase_view, &mut asks);
        ui.painter().hline(full.x_range(), toolbar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let body = Rect::from_min_max(pos2(full.left(), toolbar.bottom()), full.max);
        let right_width = (body.width() * 0.28).clamp(280.0, 400.0);
        let right = Rect::from_min_max(pos2(body.right() - right_width, body.top()), body.max);
        let list = Rect::from_min_max(body.min, pos2(right.left() - 1.0, body.bottom()));
        ui.painter().vline(right.left(), body.y_range(), Stroke::new(1.0, p.frame));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list).id_salt("rb-list"));
        rebase_list(&mut child, &p, &plan, &mut self.rebase_view, &mut asks);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(right).id_salt("rb-details"));
        rebase_details(&mut child, &p, &plan, &mut self.rebase_view, &mut asks);
        action_menu(ui, &plan, &mut self.rebase_view, &mut asks);
        rebase_keys(ui, &plan, &mut self.rebase_view, &mut asks);
        for ask in asks {
            match ask {
                Ask::Edit(params) => self.dispatch(ui, registry, session, "vcs.rebase.edit", params),
                Ask::Run(command) => self.dispatch(ui, registry, session, command, json!({})),
            }
        }
    }
}

fn action_color(p: &IdePalette, action: RebaseAction) -> Color32 {
    match action {
        RebaseAction::Pick => p.vcs_added,
        RebaseAction::Reword => p.vcs_modified,
        RebaseAction::Squash => lane_color(p, 3),
        RebaseAction::Fixup => lane_color(p, 4),
        RebaseAction::Drop => p.vcs_deleted,
    }
}

fn action_name(action: RebaseAction) -> (String, &'static str) {
    match action {
        RebaseAction::Pick => (t!("vcs-rebase-pick"), "P"),
        RebaseAction::Reword => (t!("vcs-rebase-reword"), "R"),
        RebaseAction::Squash => (t!("vcs-rebase-squash"), "S"),
        RebaseAction::Fixup => (t!("vcs-rebase-fixup"), "F"),
        RebaseAction::Drop => (t!("vcs-rebase-drop"), "D"),
    }
}

const ACTIONS: [RebaseAction; 5] =
    [RebaseAction::Pick, RebaseAction::Reword, RebaseAction::Squash, RebaseAction::Fixup, RebaseAction::Drop];

fn plan_caption(plan: &RebasePlan) -> String {
    let count = plan.steps.len();
    match plan.kind {
        RebaseKind::CherryPick => t!("vcs-rebase-caption-cherry-pick", count = count, onto = plan.onto_label.as_str()),
        RebaseKind::Rebase => {
            let branch = plan.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
            t!("vcs-rebase-caption", branch = branch.as_str(), onto = plan.onto_label.as_str(), count = count)
        }
    }
}

fn plan_summary(plan: &RebasePlan) -> String {
    let count = |action: RebaseAction| plan.steps.iter().filter(|step| step.action == action).count();
    let melded = count(RebaseAction::Squash) + count(RebaseAction::Fixup);
    t!(
        "vcs-rebase-summary",
        picked = count(RebaseAction::Pick),
        reworded = count(RebaseAction::Reword),
        melded = melded,
        dropped = count(RebaseAction::Drop)
    )
}

fn rebase_toolbar(ui: &mut Ui, rect: Rect, p: &IdePalette, plan: &RebasePlan, view: &RebaseView, asks: &mut Vec<Ask>) {
    let painter = ui.painter();
    let icon = Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), Vec2::splat(22.0));
    glyph_rebase(painter, icon, p.vcs_current_branch);
    let title = match plan.kind {
        RebaseKind::CherryPick => t!("vcs-rebase-title-cherry-pick"),
        RebaseKind::Rebase => t!("vcs-rebase-title"),
    };
    let x = icon.right() + 12.0;
    painter.text(pos2(x, rect.center().y - 9.0), Align2::LEFT_CENTER, title, theme::semibold(15.0), p.text);
    painter.text(
        pos2(x, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        plan_caption(plan),
        theme::regular(12.5),
        p.text_secondary,
    );
    let start_label = match plan.kind {
        RebaseKind::CherryPick => t!("vcs-rebase-start-cherry-pick"),
        RebaseKind::Rebase => t!("vcs-rebase-start"),
    };
    let can_start = view.problem.is_none();
    let mut right = rect.right() - 14.0;
    let mut button = |ui: &mut Ui, label: String, enabled: bool, primary: bool, why: Option<&str>| {
        let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(13.0), p.text);
        let width = galley.size().x + 28.0;
        let area = Rect::from_min_max(pos2(right - width, rect.center().y - 15.0), pos2(right, rect.center().y + 15.0));
        right = area.left() - 8.0;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let color = match (enabled, primary) {
            (false, _) => p.text_secondary,
            (true, true) => p.accent_text,
            (true, false) => p.text,
        };
        let fill = match (enabled, primary) {
            (true, true) => p.accent,
            (false, true) => p.hover,
            _ => Color32::TRANSPARENT,
        };
        let mut widget = egui::Button::new(egui::RichText::new(label).font(theme::regular(13.0)).color(color))
            .fill(fill)
            .corner_radius(6)
            .min_size(area.size());
        if !primary {
            widget = widget.stroke(Stroke::new(1.0, p.button_border));
        }
        let response = child.add_enabled(enabled, widget);
        let response = match why {
            Some(why) => response.on_disabled_hover_text(why),
            None => response,
        };
        response.clicked()
    };
    if button(ui, start_label, can_start, true, view.problem.as_deref()) {
        asks.push(Ask::Run("vcs.rebase.start"));
    }
    if button(ui, t!("vcs-rebase-cancel"), true, false, None) {
        asks.push(Ask::Run("vcs.rebase.cancel"));
    }
    if button(ui, t!("vcs-rebase-reset"), view.changed, false, None) {
        asks.push(Ask::Run("vcs.rebase.reset"));
    }
}

struct Columns {
    grip: (f32, f32),
    action: (f32, f32),
    graph: (f32, f32),
    message: (f32, f32),
    author: Option<(f32, f32)>,
    date: Option<(f32, f32)>,
    sha: Option<(f32, f32)>,
}

impl Columns {
    fn new(rect: Rect, action_width: f32) -> Self {
        let grip = (rect.left(), rect.left() + GRIP);
        let action = (grip.1, grip.1 + action_width);
        let graph = (action.1, action.1 + GRAPH);
        let mut end = rect.right();
        let take = |width: f32, end: &mut f32| {
            (*end - width - graph.1 >= MESSAGE).then(|| {
                let column = (*end - width, *end);
                *end -= width;
                column
            })
        };
        let sha = take(SHA, &mut end);
        let date = sha.and_then(|_| take(DATE, &mut end));
        let author = date.and_then(|_| take(AUTHOR, &mut end));
        Columns { grip, action, graph, message: (graph.1, end), author, date, sha }
    }

    fn graph_x(&self) -> f32 {
        (self.graph.0 + self.graph.1) / 2.0
    }
}

fn rebase_list(ui: &mut Ui, p: &IdePalette, plan: &RebasePlan, view: &mut RebaseView, asks: &mut Vec<Ask>) {
    let width = ui.available_width();
    let notice = match (&view.problem, plan.checkout) {
        (Some(problem), _) => Some((problem.clone(), p.warning_text)),
        (None, true) => {
            let branch = plan.branch.clone().unwrap_or_default();
            Some((t!("vcs-rebase-checkout-first", branch = branch.as_str()), p.text_secondary))
        }
        (None, false) => None,
    };
    if let Some((text, color)) = notice {
        let (band, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
        let fill = look::mix(p.editor_background, color, 0.1);
        ui.painter().rect_filled(band.shrink2(vec2(8.0, 3.0)), 6, fill);
        let painter = ui.painter().with_clip_rect(band.shrink2(vec2(16.0, 0.0)));
        painter.text(pos2(band.left() + 20.0, band.center().y), Align2::LEFT_CENTER, text, theme::regular(12.5), color);
    }
    let (header, _) = ui.allocate_exact_size(vec2(width, HEADER), Sense::hover());
    let action_width = ACTIONS
        .iter()
        .map(|action| {
            ui.painter().layout_no_wrap(action_name(*action).0, theme::semibold(12.0), p.text).size().x + 40.0
        })
        .fold(ACTION, f32::max);
    let columns = Columns::new(header, action_width);
    let title = |(x, right): (f32, f32), key: &str| {
        let span = Rect::from_x_y_ranges(x..=right.max(x), header.y_range());
        let response = ui.interact(span, ui.scope_id().with(("gw-rebase-title", key)), Sense::hover());
        super::columns::column_title(ui, &response, header, (x, right), &t!(key), p.text_secondary);
    };
    title((columns.action.0 - 6.0, columns.action.1), "vcs-rebase-column-action");
    title(columns.message, "vcs-workspace-column-message");
    if let Some(author) = columns.author {
        title(author, "vcs-workspace-column-author");
    }
    if let Some(date) = columns.date {
        title(date, "vcs-workspace-column-date");
    }
    if let Some(sha) = columns.sha {
        title(sha, "vcs-workspace-column-sha");
    }
    ui.painter().hline(header.x_range(), header.bottom() - 0.5, Stroke::new(1.0, p.frame));

    let rows = plan.steps.len();
    let footer = 30.0;
    let height = (ui.available_height() - footer).max(ROW);
    let lane = lane_color(p, 0);
    egui::ScrollArea::vertical().id_salt("rb-rows").max_height(height).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let top = ui.cursor().top();
        let mut drop_slot = None;
        for row in 0..rows {
            let index = rows - 1 - row;
            let step = &plan.steps[index];
            let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click_and_drag());
            let columns = Columns::new(rect, action_width);
            let selected = view.selected == row;
            let dragged = view.dragged == Some(row);
            let tint = if selected || dragged {
                Tint::Selected
            } else if response.hovered() {
                Tint::Hovered
            } else {
                Tint::Row
            };
            let painter = ui.painter().with_clip_rect(rect);
            let color = if step.action == RebaseAction::Drop { p.text_secondary } else { lane };
            painter.rect_filled(
                Rect::from_x_y_ranges(columns.graph_x()..=rect.right(), rect.y_range()),
                0,
                look::row_tint(p, color, tint),
            );
            if selected {
                painter.rect_filled(
                    Rect::from_x_y_ranges(columns.message.0..=columns.message.0 + 3.0, rect.y_range().shrink(4.0)),
                    1,
                    color,
                );
            }
            let grip = Rect::from_center_size(
                pos2((columns.grip.0 + columns.grip.1) / 2.0, rect.center().y),
                Vec2::splat(14.0),
            );
            let grip_color = if response.hovered() || dragged { p.text } else { p.text_secondary.gamma_multiply(0.6) };
            glyph_grip(&painter, grip, grip_color);
            if response.hovered() {
                ui.ctx().set_cursor_icon(if dragged { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
            }
            let pill = Rect::from_min_max(
                pos2(columns.action.0 + 2.0, rect.center().y - 11.0),
                pos2(columns.action.1 - 8.0, rect.center().y + 11.0),
            );
            let pill_response = ui.interact(pill, ui.scope_id().with(("rb-action", &step.id)), Sense::click());
            draw_action(&painter, p, pill, step.action, pill_response.hovered());
            hint(ui, &pill_response, &t!("vcs-rebase-action-hint"), None, Side::Below);
            if pill_response.clicked() {
                view.selected = row;
                view.action_menu = Some((row, pill.left_bottom() + vec2(0.0, 2.0)));
            }
            draw_graph(&painter, p, &columns, rect, step.action, row == 0, lane);
            let message = Rect::from_x_y_ranges(columns.message.0 + 10.0..=columns.message.1 - 10.0, rect.y_range());
            if step.action == RebaseAction::Reword {
                inline_summary(ui, p, step, message, view, selected, asks);
            } else {
                draw_subject(&painter, p, plan, index, step, message);
            }
            let cell = |painter: &egui::Painter, range: Option<(f32, f32)>, text: &str, font: FontId| {
                let Some((left, right)) = range else { return };
                let clip = painter.with_clip_rect(Rect::from_x_y_ranges(left..=right - 8.0, rect.y_range()));
                clip.text(pos2(left + 10.0, rect.center().y), Align2::LEFT_CENTER, text, font, p.text_secondary);
            };
            cell(&painter, columns.author, &step.author, theme::regular(12.5));
            cell(&painter, columns.date, &format_date(step.time + i64::from(step.offset)), theme::regular(12.5));
            cell(&painter, columns.sha, &step.short_id, FontId::monospace(12.0));
            if response.clicked() {
                view.selected = row;
            }
            if response.drag_started() {
                view.dragged = Some(row);
                view.selected = row;
            }
            if let Some(from) = view.dragged
                && let Some(pointer) = ui.ctx().pointer_interact_pos()
            {
                let slot = (((pointer.y - top) / ROW).round().max(0.0) as usize).min(rows);
                drop_slot = Some((from, slot));
            }
            if response.secondary_clicked() {
                view.selected = row;
                view.action_menu = Some((row, response.interact_pointer_pos().unwrap_or(pill.left_bottom())));
            }
        }
        let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        let columns = Columns::new(rect, action_width);
        let painter = ui.painter().with_clip_rect(rect);
        let x = columns.graph_x();
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.center().y - 6.0)], Stroke::new(look::LINE, lane));
        painter.circle_stroke(pos2(x, rect.center().y), 6.0, Stroke::new(look::LINE, lane));
        let onto = match plan.kind {
            RebaseKind::CherryPick => t!("vcs-rebase-onto-head", onto = plan.onto_label.as_str()),
            RebaseKind::Rebase => t!("vcs-rebase-onto", onto = plan.onto_label.as_str()),
        };
        painter.text(
            pos2(columns.message.0 + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            onto,
            theme::regular(13.0),
            p.text_secondary,
        );
        if let Some((left, _)) = columns.sha {
            let short: String = plan.onto.chars().take(7).collect();
            painter.text(
                pos2(left + 10.0, rect.center().y),
                Align2::LEFT_CENTER,
                short,
                FontId::monospace(12.0),
                p.text_secondary,
            );
        }
        if let Some((from, slot)) = drop_slot {
            let y = top + slot as f32 * ROW;
            let range = (columns.grip.0 + 6.0)..=(rect.right() - 6.0);
            ui.painter().hline(range, y, Stroke::new(2.0, p.accent));
            ui.painter().circle_filled(pos2(columns.grip.0 + 6.0, y), 3.5, p.accent);
            if ui.input(|input| input.pointer.any_released()) {
                view.dragged = None;
                let to = if slot > from { slot - 1 } else { slot };
                if to != from {
                    view.selected = to;
                    let index = rows - 1 - from;
                    asks.push(Ask::Edit(json!({ "index": index, "moveTo": rows - 1 - to })));
                }
            }
        }
        if !ui.input(|input| input.pointer.any_down()) {
            view.dragged = None;
        }
    });
    let (line, _) = ui.allocate_exact_size(vec2(width, footer), Sense::hover());
    ui.painter().hline(line.x_range(), line.top() + 0.5, Stroke::new(1.0, p.frame));
    let painter = ui.painter().with_clip_rect(line);
    let summary = painter.text(
        pos2(line.left() + 14.0, line.center().y),
        Align2::LEFT_CENTER,
        plan_summary(plan),
        theme::regular(12.0),
        p.text_secondary,
    );
    let keys = painter.layout_no_wrap(t!("vcs-rebase-keys"), theme::regular(12.0), p.text_secondary);
    if summary.right() + 32.0 + keys.size().x <= line.right() - 14.0 {
        let at = pos2(line.right() - 14.0 - keys.size().x, line.center().y - keys.size().y / 2.0);
        painter.galley(at, keys, p.text_secondary);
    }
}

fn draw_action(painter: &egui::Painter, p: &IdePalette, rect: Rect, action: RebaseAction, hovered: bool) {
    let color = action_color(p, action);
    let fill = look::mix(p.editor_background, color, if hovered { 0.34 } else { 0.22 });
    painter.rect_filled(rect, 5, fill);
    painter.rect_stroke(rect, 5, Stroke::new(1.0, look::mix(p.editor_background, color, 0.7)), StrokeKind::Inside);
    let (name, _) = action_name(action);
    painter.text(pos2(rect.left() + 9.0, rect.center().y), Align2::LEFT_CENTER, name, theme::semibold(12.0), p.text);
    crate::icons::chevron_down(
        painter,
        Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), Vec2::splat(9.0)),
        p.text_secondary,
    );
}

fn draw_graph(
    painter: &egui::Painter,
    p: &IdePalette,
    columns: &Columns,
    rect: Rect,
    action: RebaseAction,
    first: bool,
    lane: Color32,
) {
    let x = columns.graph_x();
    let line = Stroke::new(look::LINE, lane);
    let top = if first { rect.center().y } else { rect.top() };
    painter.line_segment([pos2(x, top), pos2(x, rect.bottom())], line);
    let center = pos2(x, rect.center().y);
    match action {
        RebaseAction::Pick | RebaseAction::Reword => {
            painter.circle_filled(center, 7.0, lane);
            painter.circle_stroke(center, 7.0, Stroke::new(1.5, p.editor_background));
        }
        RebaseAction::Squash | RebaseAction::Fixup => {
            let color = action_color(p, action);
            painter.line_segment([center, pos2(x, rect.bottom())], Stroke::new(look::LINE + 1.0, color));
            painter.circle_filled(center, 4.5, color);
            painter.circle_stroke(center, 4.5, Stroke::new(1.5, p.editor_background));
        }
        RebaseAction::Drop => {
            painter.circle_filled(center, 6.0, p.editor_background);
            painter.circle_stroke(center, 6.0, Stroke::new(1.5, p.text_secondary));
        }
    }
    if action == RebaseAction::Drop {
        let d = 3.5;
        painter.line_segment([center + vec2(-d, -d), center + vec2(d, d)], Stroke::new(1.5, p.vcs_deleted));
        painter.line_segment([center + vec2(d, -d), center + vec2(-d, d)], Stroke::new(1.5, p.vcs_deleted));
    }
}

fn draw_subject(
    painter: &egui::Painter,
    p: &IdePalette,
    plan: &RebasePlan,
    index: usize,
    step: &RebaseStepInfo,
    rect: Rect,
) {
    let clip = painter.with_clip_rect(rect);
    let (color, font) = match step.action {
        RebaseAction::Drop => (p.text_secondary, theme::regular(13.0)),
        _ => (p.text, theme::regular(13.0)),
    };
    let galley = clip.layout_no_wrap(step.subject.clone(), font, color);
    let at = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    let width = galley.size().x;
    clip.galley(at, galley, color);
    if step.action == RebaseAction::Drop {
        clip.hline(rect.left()..=rect.left() + width.min(rect.width()), rect.center().y, Stroke::new(1.0, color));
    }
    if step.action.melds() {
        let into = plan.steps[..index]
            .iter()
            .rev()
            .find(|step| step.action != RebaseAction::Drop)
            .map(|step| step.short_id.clone());
        if let Some(into) = into {
            let note = t!("vcs-rebase-into", commit = into.as_str());
            clip.text(
                pos2(rect.left() + width + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                note,
                theme::regular(12.0),
                p.text_secondary,
            );
        }
    }
}

fn inline_summary(
    ui: &mut Ui,
    p: &IdePalette,
    step: &RebaseStepInfo,
    rect: Rect,
    view: &mut RebaseView,
    selected: bool,
    asks: &mut Vec<Ask>,
) {
    let id = ui.scope_id().with(("rb-summary", &step.id));
    let draft = view.drafts.entry(step.id.clone()).or_insert_with(|| message_fields(&step.new_message()));
    let field =
        Rect::from_min_max(pos2(rect.left() - 4.0, rect.center().y - 12.0), pos2(rect.right(), rect.center().y + 12.0));
    let focused = ui.memory(|memory| memory.has_focus(id));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field));
    let frame = egui::Frame::new()
        .fill(p.text_field)
        .stroke(Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { p.accent } else { p.text_field_border }))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(6, 3));
    let response = frame
        .show(&mut child, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut draft.0)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .font(theme::regular(13.0))
                    .hint_text(t!("vcs-workspace-summary"))
                    .desired_width(f32::INFINITY),
            )
        })
        .inner;
    if selected && std::mem::take(&mut view.focus_summary) {
        response.request_focus();
    }
    if response.changed() {
        asks.push(Ask::Edit(json!({ "id": step.id, "message": commit_message(&draft.0, &draft.1) })));
    }
}

fn rebase_details(ui: &mut Ui, p: &IdePalette, plan: &RebasePlan, view: &mut RebaseView, asks: &mut Vec<Ask>) {
    let Some(step) = plan.steps.get(view.step_of(view.selected)).cloned() else { return };
    egui::ScrollArea::vertical().id_salt("rb-details-scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
        let label = ui.painter().layout_no_wrap(
            format!("{}:", t!("vcs-workspace-commit")),
            theme::regular(13.0),
            p.text_secondary,
        );
        let value = ui.painter().layout_no_wrap(step.short_id.clone(), FontId::monospace(13.0), p.text);
        let start = row.center().x - (label.size().x + 6.0 + value.size().x) / 2.0;
        let label_width = label.size().x;
        ui.painter().galley(pos2(start, row.center().y - label.size().y / 2.0), label, p.text_secondary);
        ui.painter().galley(pos2(start + label_width + 6.0, row.center().y - value.size().y / 2.0), value, p.text);
        ui.painter().hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, p.frame));
        egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 12 }).show(ui, |ui| {
            egui::Frame::new()
                .stroke(Stroke::new(1.0, p.frame))
                .corner_radius(6)
                .inner_margin(egui::Margin { left: 14, right: 14, top: 10, bottom: 12 })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(egui::RichText::new(&step.subject).font(theme::regular(17.0)).color(p.text))
                            .wrap(),
                    );
                    if !step.body.is_empty() {
                        ui.add_space(8.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(step.body.trim_end()).font(theme::regular(13.0)).color(p.text),
                            )
                            .wrap(),
                        );
                    }
                });
            ui.add_space(14.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (badge, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                draw_avatar(ui.painter(), p, badge.center(), 20.0, &step.author, 15.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(egui::RichText::new(&step.author).font(theme::regular(14.5)).color(p.text));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        ui.label(
                            egui::RichText::new(t!("vcs-workspace-authored"))
                                .font(theme::regular(12.5))
                                .color(p.text_secondary),
                        );
                        ui.label(
                            egui::RichText::new(format_date(step.time + i64::from(step.offset)))
                                .font(theme::regular(12.5))
                                .color(p.text),
                        );
                    });
                });
            });
            ui.add_space(18.0);
            let galley =
                ui.painter().layout_job(look::small_caps(&t!("vcs-rebase-column-action"), 10.5, p.text_secondary));
            let (title, _) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y), Sense::hover());
            ui.painter().galley(title.min, galley, p.text_secondary);
            ui.add_space(8.0);
            for action in ACTIONS {
                let (name, key) = action_name(action);
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                let chosen = step.action == action;
                if chosen {
                    let color = action_color(p, action);
                    ui.painter().rect_filled(rect, 6, look::mix(p.editor_background, color, 0.22));
                    ui.painter().rect_stroke(
                        rect,
                        6,
                        Stroke::new(1.0, look::mix(p.editor_background, color, 0.7)),
                        StrokeKind::Inside,
                    );
                } else {
                    hover(ui, &response, p);
                }
                ui.painter().circle_filled(pos2(rect.left() + 14.0, rect.center().y), 4.5, action_color(p, action));
                ui.painter().text(
                    pos2(rect.left() + 28.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    theme::regular(13.0),
                    p.text,
                );
                ui.painter().text(
                    pos2(rect.right() - 12.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    key,
                    theme::regular(12.0),
                    p.text_secondary,
                );
                let what = action_help(action);
                hint(ui, &response, &what, None, Side::Below);
                if response.clicked() && !chosen {
                    asks.push(Ask::Edit(json!({ "id": step.id, "action": action })));
                    if action == RebaseAction::Reword {
                        view.focus_summary = true;
                    }
                }
                ui.add_space(4.0);
            }
            if step.action == RebaseAction::Reword {
                ui.add_space(14.0);
                let galley =
                    ui.painter().layout_job(look::small_caps(&t!("vcs-rebase-new-message"), 10.5, p.text_secondary));
                let (title, _) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y), Sense::hover());
                ui.painter().galley(title.min, galley, p.text_secondary);
                ui.add_space(8.0);
                let draft = view.drafts.entry(step.id.clone()).or_insert_with(|| message_fields(&step.new_message()));
                let count = draft.0.trim().chars().count();
                let counter = t!("vcs-workspace-summary-count", count = count, limit = SUMMARY);
                let counter_color = if count > SUMMARY { p.warning_text } else { p.text_secondary };
                let mut changed = false;
                let summary_id = ui.scope_id().with(("rb-panel-summary", &step.id));
                framed_field(ui, p, summary_id, |ui| {
                    ui.horizontal(|ui| {
                        let galley = ui.painter().layout_no_wrap(counter.clone(), theme::regular(11.5), counter_color);
                        let room = (ui.available_width() - galley.size().x - 8.0).max(40.0);
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut draft.0)
                                .id(summary_id)
                                .frame(egui::Frame::NONE)
                                .hint_text(t!("vcs-workspace-summary"))
                                .desired_width(room),
                        );
                        changed |= field.changed();
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(galley.size().x, field.rect.height()), Sense::hover());
                        ui.painter().galley(
                            pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
                            galley,
                            counter_color,
                        );
                        field
                    })
                    .inner
                });
                ui.add_space(8.0);
                let description_id = ui.scope_id().with(("rb-panel-description", &step.id));
                let description = framed_field(ui, p, description_id, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut draft.1)
                            .id(description_id)
                            .frame(egui::Frame::NONE)
                            .hint_text(t!("vcs-workspace-description"))
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    )
                });
                changed |= description.changed();
                if changed {
                    asks.push(Ask::Edit(json!({ "id": step.id, "message": commit_message(&draft.0, &draft.1) })));
                }
            }
        });
    });
}

fn action_help(action: RebaseAction) -> String {
    match action {
        RebaseAction::Pick => t!("vcs-rebase-pick-help"),
        RebaseAction::Reword => t!("vcs-rebase-reword-help"),
        RebaseAction::Squash => t!("vcs-rebase-squash-help"),
        RebaseAction::Fixup => t!("vcs-rebase-fixup-help"),
        RebaseAction::Drop => t!("vcs-rebase-drop-help"),
    }
}

fn action_menu(ui: &Ui, plan: &RebasePlan, view: &mut RebaseView, asks: &mut Vec<Ask>) {
    let Some((row, at)) = view.action_menu else { return };
    let Some(step) = plan.steps.get(view.step_of(row)).cloned() else {
        view.action_menu = None;
        return;
    };
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("rb-action-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let mut focus = false;
    popup.show(|ui| {
        ui.set_min_width(180.0);
        for action in ACTIONS {
            let (name, key) = action_name(action);
            let response = ui.add(egui::Button::new(name).shortcut_text(key).selected(step.action == action));
            if response.clicked() {
                if step.action != action {
                    asks.push(Ask::Edit(json!({ "id": step.id, "action": action })));
                    focus = action == RebaseAction::Reword;
                }
                ui.close();
            }
        }
    });
    if focus {
        view.focus_summary = true;
    }
    if !open {
        view.action_menu = None;
    }
}

fn rebase_keys(ui: &Ui, plan: &RebasePlan, view: &mut RebaseView, asks: &mut Vec<Ask>) {
    if view.action_menu.is_some() || ui.memory(|memory| memory.focused().is_some()) {
        return;
    }
    let rows = plan.steps.len();
    if rows == 0 {
        return;
    }
    let row = view.selected.min(rows - 1);
    let index = view.step_of(row);
    let keys: Vec<(egui::Key, egui::Modifiers)> = ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    for (key, modifiers) in keys {
        if modifiers.command || modifiers.ctrl {
            continue;
        }
        match key {
            egui::Key::ArrowUp | egui::Key::ArrowDown => {
                let up = key == egui::Key::ArrowUp;
                let to = if up { row.checked_sub(1) } else { (row + 1 < rows).then_some(row + 1) };
                let Some(to) = to else { continue };
                if modifiers.alt {
                    asks.push(Ask::Edit(json!({ "index": index, "moveTo": rows - 1 - to })));
                }
                view.selected = to;
            }
            egui::Key::Enter if plan.steps[index].action == RebaseAction::Reword => view.focus_summary = true,
            _ => {
                let letter = match key {
                    egui::Key::P => 'p',
                    egui::Key::R => 'r',
                    egui::Key::S => 's',
                    egui::Key::F => 'f',
                    egui::Key::D => 'd',
                    _ => continue,
                };
                if modifiers.alt || modifiers.shift {
                    continue;
                }
                if let Some(action) = RebaseAction::from_key(letter)
                    && plan.steps[index].action != action
                {
                    asks.push(Ask::Edit(json!({ "index": index, "action": action })));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(actions: &[RebaseAction]) -> RebasePlan {
        RebasePlan {
            kind: RebaseKind::Rebase,
            branch: Some("main".to_owned()),
            onto: "0123456789".to_owned(),
            onto_label: "0123456".to_owned(),
            steps: actions
                .iter()
                .enumerate()
                .map(|(index, action)| RebaseStepInfo {
                    id: format!("{index}abcdef0"),
                    short_id: format!("{index}abcdef"),
                    subject: format!("Commit {index}"),
                    action: *action,
                    ..RebaseStepInfo::default()
                })
                .collect(),
            ..RebasePlan::default()
        }
    }

    #[test]
    fn the_newest_commit_is_on_top() {
        let view = RebaseView { plan: Some(plan(&[RebaseAction::Pick; 3])), ..RebaseView::default() };
        assert_eq!([view.step_of(0), view.step_of(1), view.step_of(2)], [2, 1, 0]);
    }

    #[test]
    fn the_caption_and_the_summary_say_what_happens() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let plan = plan(&[RebaseAction::Pick, RebaseAction::Squash, RebaseAction::Drop, RebaseAction::Reword]);
        assert_eq!(plan_caption(&plan), "main onto 0123456 · 4 commits");
        assert_eq!(plan_summary(&plan), "1 picked · 1 reworded · 1 melded · 1 dropped");
        let cherry = RebasePlan { kind: RebaseKind::CherryPick, onto_label: "side".into(), ..plan.clone() };
        assert_eq!(plan_caption(&cherry), "4 commits onto side");
    }

    fn keys_frame(
        view: &mut RebaseView,
        plan: &RebasePlan,
        keys: &[(egui::Key, egui::Modifiers)],
    ) -> Vec<serde_json::Value> {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        for (key, modifiers) in keys {
            input.events.push(egui::Event::Key {
                key: *key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: *modifiers,
            });
        }
        let mut edits = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            let mut asks = Vec::new();
            rebase_keys(ui, plan, view, &mut asks);
            edits.extend(asks.into_iter().filter_map(|ask| match ask {
                Ask::Edit(params) => Some(params),
                Ask::Run(_) => None,
            }));
        });
        output.textures_delta.clear();
        edits
    }

    #[test]
    fn keys_choose_actions_and_move_commits() {
        let plan = plan(&[RebaseAction::Pick; 3]);
        let mut view = RebaseView { plan: Some(plan.clone()), ..RebaseView::default() };
        let none = egui::Modifiers::NONE;
        assert_eq!(keys_frame(&mut view, &plan, &[(egui::Key::S, none)]), [json!({ "index": 2, "action": "squash" })]);
        assert_eq!(keys_frame(&mut view, &plan, &[(egui::Key::ArrowDown, none)]), Vec::<serde_json::Value>::new());
        assert_eq!(view.selected, 1);
        assert_eq!(keys_frame(&mut view, &plan, &[(egui::Key::D, none)]), [json!({ "index": 1, "action": "drop" })]);
        let moved = keys_frame(&mut view, &plan, &[(egui::Key::ArrowDown, egui::Modifiers::ALT)]);
        assert_eq!(moved, [json!({ "index": 1, "moveTo": 0 })]);
        assert_eq!(view.selected, 2);
        let moved = keys_frame(&mut view, &plan, &[(egui::Key::ArrowUp, egui::Modifiers::ALT)]);
        assert_eq!(moved, [json!({ "index": 0, "moveTo": 1 })]);
        assert!(keys_frame(&mut view, &plan, &[(egui::Key::P, none)]).is_empty());
        assert!(keys_frame(&mut view, &plan, &[(egui::Key::D, egui::Modifiers::COMMAND)]).is_empty());
        view.selected = 0;
        assert!(keys_frame(&mut view, &plan, &[(egui::Key::ArrowUp, egui::Modifiers::ALT)]).is_empty());
    }
}
