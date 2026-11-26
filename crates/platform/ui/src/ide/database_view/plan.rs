use egui::{Align, Align2, Color32, FontId, Layout, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_db::explain::{ExplainOption, ExplainOptions, Measure, Plan, PlanNode};
use zen_i18n::t;
use zen_workbench::database::plans::{PlanStatus, PlanTab, PlanView as Shown, Plans};
use zen_workbench::workspaces::InnerTab;

use super::super::{IdePalette, theme, vcs};
use super::message;
use crate::accessibility;

const BAR: f32 = 36.0;

const STRIP: f32 = 28.0;

const ROW: f32 = 24.0;

const INDENT: f32 = 16.0;

const DETAILS: f32 = 320.0;

const DETAILS_SHOWN_FROM: f32 = 820.0;

#[derive(Default)]
pub(super) struct PlanView {
    edited: std::collections::HashMap<String, ExplainOptions>,
    error: Option<String>,
}

pub(super) fn glyph_plan(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new((rect.width() / 10.0).max(1.2), color);
    let (left, top) = (rect.left() + rect.width() * 0.1, rect.top() + rect.height() * 0.18);
    let step = rect.height() * 0.32;
    for level in 0..3 {
        let x = left + level as f32 * rect.width() * 0.25;
        let y = top + level as f32 * step;
        painter.line_segment([pos2(x, y), pos2(x + rect.width() * 0.45, y)], stroke);
        if level > 0 {
            let parent = x - rect.width() * 0.25 + rect.width() * 0.08;
            painter.line_segment([pos2(parent, y - step), pos2(parent, y)], stroke);
            painter.line_segment([pos2(parent, y), pos2(x, y)], stroke);
        }
    }
}

fn option_label(option: ExplainOption) -> String {
    match option {
        ExplainOption::Analyze => t!("ide-dbplan-analyze"),
        ExplainOption::Verbose => t!("ide-dbplan-verbose"),
        ExplainOption::Costs => t!("ide-dbplan-costs"),
        ExplainOption::Buffers => t!("ide-dbplan-buffers"),
        ExplainOption::Settings => t!("ide-dbplan-settings"),
    }
}

fn option_mut(options: &mut ExplainOptions, option: ExplainOption) -> &mut bool {
    match option {
        ExplainOption::Analyze => &mut options.analyze,
        ExplainOption::Verbose => &mut options.verbose,
        ExplainOption::Costs => &mut options.costs,
        ExplainOption::Buffers => &mut options.buffers,
        ExplainOption::Settings => &mut options.settings,
    }
}

fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else if value.abs() >= 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn millis(value: f64) -> String {
    t!("ide-dbplan-ms", ms = if value >= 100.0 { format!("{value:.0}") } else { format!("{value:.3}") })
}

pub(super) fn heat(p: &IdePalette, share: f64) -> Option<Color32> {
    if share < 0.05 {
        return None;
    }
    let base = if share >= zen_db::explain::HOT { p.error_text } else { p.warning_text };
    Some(base.gamma_multiply((0.10 + share as f32 * 0.32).min(0.42)))
}

fn rows_text(node: &PlanNode) -> String {
    match (node.rows, node.actual_rows) {
        (Some(estimated), Some(actual)) => format!("{} / {}", number(estimated), number(actual)),
        (Some(estimated), None) => number(estimated),
        (None, Some(actual)) => number(actual),
        (None, None) => String::new(),
    }
}

fn restored(tab: &InnerTab) -> PlanTab {
    PlanTab {
        connection: tab.data["connection"].as_str().unwrap_or_default().to_owned(),
        statement: tab.data["statement"].as_str().unwrap_or_default().to_owned(),
        options: serde_json::from_value(tab.data["options"].clone()).unwrap_or_default(),
        ..PlanTab::default()
    }
}

enum Act {
    Explain(ExplainOptions),
    Cancel(u64),
    Select(usize),
    View(Shown),
}

impl PlanView {
    pub(super) fn options(&self, tab: &str, state: &PlanTab) -> ExplainOptions {
        self.edited.get(tab).cloned().unwrap_or_else(|| state.options.clone())
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        tab: &InnerTab,
    ) {
        zen_workbench::database::plans::step(session);
        let state =
            session.state::<Plans>().and_then(|plans| plans.get(&tab.id)).cloned().unwrap_or_else(|| restored(tab));
        if state.status == PlanStatus::Running {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.editor_background);
        let mut acts = Vec::new();
        let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR));
        let mut top = self.bar(ui, p, bar, (&tab.id, &state), &mut acts);
        let options = self.options(&tab.id, &state);
        if options.analyze && (state.supported.is_empty() || state.supported.contains(&ExplainOption::Analyze)) {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), STRIP));
            self.strip(ui, p, strip, &t!("ide-dbplan-analyze-warning"), p.warning_text);
            top = strip.bottom();
        }
        let note = match state.status {
            PlanStatus::Failed | PlanStatus::Cancelled => Some((
                state
                    .error
                    .as_ref()
                    .map_or_else(|| t!("ide-dbplan-failed"), zen_workbench::database::connections::describe),
                p.error_text,
            )),
            PlanStatus::NotRun => Some((t!("ide-dbplan-not-run"), p.text_secondary)),
            PlanStatus::Running => Some((t!("ide-dbplan-running"), p.text_secondary)),
            PlanStatus::Done if !state.inlined.is_empty() => {
                Some((t!("ide-dbplan-inlined", values = state.inlined.join(", ")), p.text_secondary))
            }
            PlanStatus::Done if !state.ignored.is_empty() => {
                let names: Vec<String> = state.ignored.iter().map(|option| option_label(*option)).collect();
                Some((t!("ide-dbplan-ignored", options = names.join(", ")), p.text_secondary))
            }
            PlanStatus::Done => None,
        };
        let note = self.error.clone().map(|error| (error, p.error_text)).or(note);
        if let Some((text, color)) = note {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), STRIP));
            self.strip(ui, p, strip, &text, color);
            top = strip.bottom();
        }
        let body = Rect::from_min_max(pos2(area.left(), top), area.max);
        if let Some(plan) = state.plan.clone() {
            match state.view {
                Shown::Raw => raw(ui, p, body, &plan),
                Shown::Tree => {
                    let details = if body.width() >= DETAILS_SHOWN_FROM { DETAILS } else { 0.0 };
                    let tree = Rect::from_min_max(body.min, pos2(body.right() - details, body.bottom()));
                    tree_view(ui, p, tree, (&plan, state.selected), &mut acts);
                    if details > 0.0 {
                        let pane = Rect::from_min_max(pos2(tree.right(), body.top()), body.max);
                        details_pane(ui, p, pane, (&plan, state.selected));
                    }
                }
            }
        }
        for act in acts {
            let (command, params) = match act {
                Act::Explain(options) => {
                    self.edited.remove(&tab.id);
                    (
                        "db.console.explain",
                        json!({
                            "tab": tab.id,
                            "analyze": options.analyze,
                            "verbose": options.verbose,
                            "costs": options.costs,
                            "buffers": options.buffers,
                            "settings": options.settings,
                        }),
                    )
                }
                Act::Cancel(operation) => ("db.connection.cancel", json!({ "operation": operation })),
                Act::Select(node) => ("db.plan.select", json!({ "tab": tab.id, "node": node })),
                Act::View(view) => ("db.plan.view", json!({ "tab": tab.id, "view": view })),
            };
            self.error = match registry.dispatch(session, command, params) {
                Ok(_) => None,
                Err(error) if super::super::transaction_dialog::ask(session, &error) => None,
                Err(error) => Some(message(error)),
            };
        }
        ui.advance_cursor_after_rect(area);
    }

    fn strip(&self, ui: &mut Ui, p: &IdePalette, strip: Rect, text: &str, color: Color32) {
        ui.painter().rect_filled(strip, 0, p.banner);
        let job = vcs::truncated(text, theme::regular(12.5), color, strip.width() - 24.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(strip.left() + 12.0, strip.center().y - galley.size().y / 2.0), galley, color);
    }

    fn bar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        bar: Rect,
        (id, state): (&str, &PlanTab),
        acts: &mut Vec<Act>,
    ) -> f32 {
        let background = ui.painter().add(egui::Shape::Noop);
        let mut options = self.options(id, state);
        let supported = if state.supported.is_empty() && state.status == PlanStatus::NotRun {
            zen_db::explain::postgres_options()
        } else {
            state.supported.clone()
        };
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("database-plan-bar", id))
                .max_rect(bar.shrink2(vec2(10.0, 6.0)))
                .layout(Layout::left_to_right(Align::Center).with_main_wrap(true)),
        );
        child.spacing_mut().item_spacing = vec2(10.0, 6.0);
        let running = state.status == PlanStatus::Running;
        let label = t!("ide-dbplan-explain");
        let explain = child.add_enabled_ui(!running, |ui| vcs::secondary_button(ui, p, &label)).inner;
        if explain.clicked() {
            acts.push(Act::Explain(options.clone()));
        }
        if running
            && let Some(operation) = state.operation
            && vcs::secondary_button(&mut child, p, &t!("ide-dbplan-cancel")).clicked()
        {
            acts.push(Act::Cancel(operation));
        }
        let mut changed = false;
        for option in supported {
            let flag = option_mut(&mut options, option);
            let text = egui::RichText::new(option_label(option)).font(theme::regular(12.5)).color(p.text);
            changed |= child.checkbox(flag, text).changed();
        }
        if changed {
            self.edited.insert(id.to_owned(), options);
        }
        child.separator();
        for (view, label) in [(Shown::Tree, t!("ide-dbplan-tree")), (Shown::Raw, t!("ide-dbplan-raw"))] {
            let chosen = state.view == view;
            let text = egui::RichText::new(label).font(theme::regular(12.5)).color(if chosen {
                p.text
            } else {
                p.text_secondary
            });
            if child.selectable_label(chosen, text).clicked() && !chosen {
                acts.push(Act::View(view));
            }
        }
        if let Some(plan) = &state.plan {
            let mut facts = Vec::new();
            if let Some(cost) = plan.total_cost {
                facts.push(t!("ide-dbplan-total-cost", cost = number(cost)));
            }
            if let Some(planning) = plan.planning_ms {
                facts.push(t!("ide-dbplan-planning", time = millis(planning)));
            }
            if let Some(execution) = plan.execution_ms.or(plan.total_time_ms) {
                facts.push(t!("ide-dbplan-execution", time = millis(execution)));
            }
            if state.rolled_back {
                facts.push(t!("ide-dbplan-rolled-back"));
            }
            child.separator();
            child.label(egui::RichText::new(facts.join(" · ")).font(theme::regular(12.0)).color(p.text_secondary));
        }
        let bottom = (child.min_rect().bottom() + 6.0).max(bar.bottom());
        let drawn = Rect::from_min_max(bar.min, pos2(bar.right(), bottom));
        ui.painter().set(background, egui::Shape::rect_filled(drawn, 0, p.island));
        ui.painter().hline(drawn.x_range(), drawn.bottom() - 0.5, Stroke::new(1.0, p.frame));
        bottom
    }
}

struct Columns {
    node: f32,
    entity: f32,
    cost: f32,
    rows: f32,
    time: f32,
    condition: f32,
}

impl Columns {
    fn of(width: f32) -> Self {
        let node = width * 0.26;
        let entity = width * 0.18;
        let (cost, rows, time) = (width * 0.10, width * 0.11, width * 0.11);
        Self { node, entity, cost, rows, time, condition: width - node - entity - cost - rows - time }
    }

    fn lefts(&self, left: f32) -> [f32; 6] {
        let mut at = left;
        let mut lefts = [0.0; 6];
        for (index, width) in
            [self.node, self.entity, self.cost, self.rows, self.time, self.condition].iter().enumerate()
        {
            lefts[index] = at;
            at += width;
        }
        lefts
    }
}

fn cell(ui: &Ui, text: &str, (left, width, center_y): (f32, f32, f32), font: FontId, color: Color32, right: bool) {
    if text.is_empty() || width < 12.0 {
        return;
    }
    let job = vcs::truncated(text, font, color, width - 12.0);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let x = if right { left + width - 8.0 - galley.size().x } else { left + 6.0 };
    ui.painter().galley(pos2(x, center_y - galley.size().y / 2.0), galley, color);
}

fn header(ui: &Ui, p: &IdePalette, rect: Rect, columns: &Columns, measure: Measure) {
    ui.painter().rect_filled(rect, 0, p.island);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.frame));
    let lefts = columns.lefts(rect.left());
    let widths = [columns.node, columns.entity, columns.cost, columns.rows, columns.time, columns.condition];
    let names = [
        t!("ide-dbplan-column-node"),
        t!("ide-dbplan-column-entity"),
        t!("ide-dbplan-column-cost"),
        t!("ide-dbplan-column-rows"),
        t!("ide-dbplan-column-time"),
        t!("ide-dbplan-column-condition"),
    ];
    for (index, name) in names.iter().enumerate() {
        let right = (2..=4).contains(&index);
        let marked = (index == 2 && measure == Measure::Cost) || (index == 4 && measure == Measure::Time);
        let color = if marked { p.text } else { p.text_secondary };
        let font = if marked { theme::semibold(12.0) } else { theme::regular(12.0) };
        cell(ui, name, (lefts[index], widths[index], rect.center().y), font, color, right);
    }
}

fn tree_view(ui: &mut Ui, p: &IdePalette, rect: Rect, (plan, selected): (&Plan, Option<usize>), acts: &mut Vec<Act>) {
    let columns = Columns::of(rect.width());
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), ROW));
    header(ui, p, head, &columns, plan.measure);
    let list = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
    if plan.nodes.is_empty() {
        let text = t!("ide-dbplan-empty");
        ui.painter().text(
            pos2(list.center().x, list.top() + 32.0),
            Align2::CENTER_CENTER,
            text,
            theme::regular(13.0),
            p.text_secondary,
        );
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().id_salt("database-plan-tree").max_rect(list));
    child.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
        &mut child,
        ROW,
        plan.nodes.len(),
        |ui, range| {
            for index in range {
                let node = &plan.nodes[index];
                let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                if selected == Some(node.id) {
                    ui.painter().rect_filled(row, 0, p.selection);
                } else if let Some(tint) = heat(p, node.share) {
                    ui.painter().rect_filled(row, 0, tint);
                } else if response.hovered() {
                    ui.painter().rect_filled(row, 0, p.hover);
                }
                draw_node(ui, p, row, &columns, (node, plan.measure));
                accessibility::name_with(&response, egui::Role::TreeItem, || {
                    let entity = node.entity.clone().unwrap_or_default();
                    format!("{} {} {}%", node.node, entity, (node.share * 100.0).round())
                });
                if response.clicked() {
                    acts.push(Act::Select(node.id));
                }
            }
        },
    );
}

fn draw_node(ui: &Ui, p: &IdePalette, row: Rect, columns: &Columns, (node, measure): (&PlanNode, Measure)) {
    let lefts = columns.lefts(row.left());
    let y = row.center().y;
    let indent = 10.0 + node.depth as f32 * INDENT;
    if node.depth > 0 {
        let x = lefts[0] + indent - INDENT / 2.0;
        let stroke = Stroke::new(1.0, p.indent_guide);
        ui.painter().line_segment([pos2(x, row.top()), pos2(x, y)], stroke);
        ui.painter().line_segment([pos2(x, y), pos2(x + INDENT / 2.0 - 2.0, y)], stroke);
    }
    let font = if node.hot { theme::semibold(12.5) } else { theme::regular(12.5) };
    let node_color = if node.hot { p.error_text } else { p.text };
    cell(ui, &node.node, (lefts[0] + indent - 6.0, columns.node - indent + 6.0, y), font, node_color, false);
    let mono = FontId::monospace(12.0);
    cell(
        ui,
        node.entity.as_deref().unwrap_or_default(),
        (lefts[1], columns.entity, y),
        theme::regular(12.5),
        p.text,
        false,
    );
    let cost = node.cost.map(number).unwrap_or_default();
    cell(ui, &cost, (lefts[2], columns.cost, y), mono.clone(), p.text, true);
    cell(ui, &rows_text(node), (lefts[3], columns.rows, y), mono.clone(), p.text, true);
    let time = node.time_ms.map(millis).unwrap_or_default();
    cell(ui, &time, (lefts[4], columns.time, y), mono.clone(), p.text, true);
    let condition = node.condition.as_deref().unwrap_or_default();
    cell(ui, condition, (lefts[5], columns.condition, y), mono, p.text_secondary, false);
    let measured = match measure {
        Measure::Time => Some((lefts[4], columns.time)),
        Measure::Cost => Some((lefts[2], columns.cost)),
        Measure::None => None,
    };
    if let Some((left, width)) = measured.filter(|_| node.share > 0.0) {
        let full = (width - 12.0).max(0.0);
        let bar = Rect::from_min_size(pos2(left + 6.0, row.bottom() - 4.0), vec2(full * node.share as f32, 2.0));
        let color = if node.hot { p.error_text } else { p.warning_text };
        ui.painter().rect_filled(bar, 1, color);
    }
}

fn detail_rows(node: &PlanNode, measure: Measure) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if measure != Measure::None {
        rows.push((t!("ide-dbplan-share"), format!("{:.1}%", node.share * 100.0)));
    }
    let mut push = |label: String, value: Option<String>| {
        if let Some(value) = value {
            rows.push((label, value));
        }
    };
    push(t!("ide-dbplan-self-time"), node.self_time_ms.map(millis));
    push(t!("ide-dbplan-self-cost"), node.self_cost.map(number));
    push(t!("ide-dbplan-startup-cost"), node.startup_cost.map(number));
    push(t!("ide-dbplan-loops"), node.loops.map(number));
    rows.extend(node.details.iter().map(|detail| (detail.key.clone(), detail.value.clone())));
    rows
}

fn details_pane(ui: &mut Ui, p: &IdePalette, rect: Rect, (plan, selected): (&Plan, Option<usize>)) {
    ui.painter().rect_filled(rect, 0, p.island);
    ui.painter().vline(rect.left() + 0.5, rect.y_range(), Stroke::new(1.0, p.frame));
    let inner = rect.shrink2(vec2(12.0, 8.0));
    let mut child = ui.new_child(
        UiBuilder::new().id_salt("database-plan-details").max_rect(inner).layout(Layout::top_down(Align::Min)),
    );
    let node = selected.and_then(|id| plan.nodes.get(id));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
        let (title, rows) = match node {
            Some(node) => (node.node.clone(), detail_rows(node, plan.measure)),
            None => (
                t!("ide-dbplan-whole"),
                plan.details.iter().map(|detail| (detail.key.clone(), detail.value.clone())).collect(),
            ),
        };
        ui.label(egui::RichText::new(title).font(theme::semibold(13.0)).color(p.text));
        if let Some(entity) = node.and_then(|node| node.entity.as_deref()) {
            ui.label(egui::RichText::new(entity).font(theme::regular(12.5)).color(p.text_secondary));
        }
        ui.add_space(6.0);
        if let Some(condition) = node.and_then(|node| node.condition.as_deref()) {
            ui.add(
                egui::Label::new(egui::RichText::new(condition).font(FontId::monospace(12.0)).color(p.text))
                    .wrap()
                    .selectable(true),
            );
            ui.add_space(6.0);
        }
        for (key, value) in rows {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("{key}:")).font(theme::regular(12.0)).color(p.text_secondary));
                ui.add(
                    egui::Label::new(egui::RichText::new(value).font(theme::regular(12.0)).color(p.text))
                        .wrap()
                        .selectable(true),
                );
            });
        }
    });
}

fn raw(ui: &mut Ui, p: &IdePalette, rect: Rect, plan: &Plan) {
    let inner = rect.shrink2(vec2(12.0, 8.0));
    let mut child = ui
        .new_child(UiBuilder::new().id_salt("database-plan-raw").max_rect(inner).layout(Layout::top_down(Align::Min)));
    egui::ScrollArea::both().auto_shrink([false, false]).show(&mut child, |ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(&plan.raw).font(FontId::monospace(12.5)).color(p.text))
                .extend()
                .selectable(true),
        );
    });
}

#[cfg(test)]
mod tests;
