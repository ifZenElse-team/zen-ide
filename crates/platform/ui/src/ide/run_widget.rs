use egui::{Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_run::Configuration;
use zen_workbench::run::Runs;

use super::glyphs::{glyph_coverage, glyph_more, glyph_run};
use super::{IdePalette, IdeView, Side, hint, hover};
use crate::accessibility;
use crate::theme;

const POPUP_ROW: f32 = 26.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum WidgetAction {
    Dispatch(&'static str, Value),
    Rerun(u64, String),
    EditConfigurations,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Seen {
    runs: Vec<u64>,
    running: Vec<u64>,
    selected: Option<String>,
    temporary: Vec<String>,
}

impl Seen {
    fn of(runs: &Runs) -> Self {
        Self {
            runs: runs.runs().iter().map(|run| run.id).collect(),
            running: runs
                .runs()
                .iter()
                .filter(|run| run.output.read(zen_run::RunOutput::is_running))
                .map(|run| run.id)
                .collect(),
            selected: runs.selected().map(str::to_owned),
            temporary: runs.temporary().iter().map(|configuration| configuration.id.clone()).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PopupRow {
    Configuration(String),
    Edit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WidgetState {
    Empty,
    Selected(String),
    Running(String, u64),
}

struct MoreItem {
    label: String,
    configuration_id: Option<String>,
    supported: bool,
    opened: bool,
}

#[derive(Default)]
pub(super) struct RunWidget {
    configurations: Option<Vec<Configuration>>,
    selected: Option<String>,
    running: Vec<(u64, String)>,
    stale: bool,
    focused: bool,
    seen: Option<Seen>,
    popup: bool,
    popup_rows: Vec<(PopupRow, Rect)>,
    buttons: Vec<(&'static str, Rect)>,
    more: Option<bool>,
    more_rows: Vec<Rect>,
}

impl RunWidget {
    pub(super) fn invalidate(&mut self) {
        self.stale = true;
    }

    pub(super) fn selected_configuration(&self) -> Option<&Configuration> {
        let configurations = self.configurations.as_deref()?;
        configurations
            .iter()
            .find(|configuration| Some(&configuration.id) == self.selected.as_ref())
            .or(configurations.first())
    }

    pub(super) fn state(&self) -> WidgetState {
        let Some(selected) = self.selected_configuration() else { return WidgetState::Empty };
        match self.running.iter().rev().find(|(_, id)| *id == selected.id) {
            Some((run, _)) => WidgetState::Running(selected.id.clone(), *run),
            None => WidgetState::Selected(selected.id.clone()),
        }
    }

    pub(super) fn is_running(&self) -> bool {
        !self.running.is_empty()
    }

    pub(super) fn close_popup(&mut self) {
        self.popup = false;
    }

    #[cfg(test)]
    pub(super) fn popup_open(&self) -> bool {
        self.popup
    }

    #[cfg(test)]
    pub(super) fn popup_row(&self, row: &PopupRow) -> Option<Rect> {
        self.popup_rows.iter().find(|(shown, _)| shown == row).map(|(_, rect)| *rect)
    }

    #[cfg(test)]
    pub(super) fn more_row(&self) -> Option<Rect> {
        self.more_rows.first().copied()
    }

    #[cfg(test)]
    pub(super) fn button(&self, name: &str) -> Option<Rect> {
        self.buttons.iter().find(|(shown, _)| *shown == name).map(|(_, rect)| *rect)
    }

    fn refresh(&mut self, focused: bool, registry: &Registry, session: &mut Session) {
        let seen = session.state::<Runs>().map(Seen::of);
        let regained = focused && !self.focused;
        self.focused = focused;
        if self.configurations.is_some() && !self.stale && !regained && self.seen == seen {
            return;
        }
        self.stale = false;
        let list = registry.dispatch(session, "run.configuration.list", json!({})).unwrap_or(Value::Null);
        self.configurations = Some(serde_json::from_value(list["configurations"].clone()).unwrap_or_default());
        self.selected = list["selected"].as_str().map(str::to_owned);
        let runs = registry.dispatch(session, "run.process.list", json!({})).unwrap_or(Value::Null);
        self.running = runs["runs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|run| run["state"] == "running")
            .flat_map(|run| {
                let id = run["runId"].as_u64();
                let own = run["configurationId"].as_str().map(str::to_owned);
                let compound = run["compound"].as_str().map(str::to_owned);
                [own, compound].into_iter().flatten().filter_map(move |configuration| Some((id?, configuration)))
            })
            .collect();
        self.seen = session.state::<Runs>().map(Seen::of);
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        right: f32,
        y: f32,
    ) -> (f32, Option<WidgetAction>) {
        let focused = ui.input(|input| input.viewport().focused.unwrap_or(true));
        self.refresh(focused, registry, session);
        self.buttons.clear();
        let mut action = None;
        let state = self.state();
        let name = self.selected_configuration().map(|configuration| configuration.name.clone());
        let compound = self.selected_configuration().is_some_and(zen_workbench::compound::is_compound);
        let mut x = right;
        if self.is_running() {
            let stop = Rect::from_center_size(pos2(x - 14.0, y), Vec2::splat(28.0));
            let response = ui.interact(stop, ui.scope_id().with("run-widget-stop"), Sense::click());
            hover(ui, &response, p);
            let (label, params) = match (&state, &name) {
                (WidgetState::Running(id, _), Some(name)) if compound => {
                    (t!("ide-run-widget-stop", name = name.as_str()), json!({ "configurationId": id }))
                }
                (WidgetState::Running(_, run), Some(name)) => {
                    (t!("ide-run-widget-stop", name = name.as_str()), json!({ "runId": run }))
                }
                _ => (t!("ide-run-stop"), json!({})),
            };
            hint(ui, &response, &label, crate::keymap::shortcut("run.process.stop").as_deref(), Side::Below);
            if response.clicked() {
                action = Some(WidgetAction::Dispatch("run.process.stop", params));
            }
            glyph_stop(ui.painter(), stop.shrink(8.0), p.problem("error"));
            self.buttons.push(("stop", stop));
            x -= 30.0;
        }
        let enabled = name.is_some();
        let more = Rect::from_center_size(pos2(x - 14.0, y), Vec2::splat(28.0));
        let response = ui.interact(more, ui.scope_id().with("run-widget-more"), Sense::click());
        if enabled {
            hover(ui, &response, p);
        }
        hint(ui, &response, &t!("ide-run-widget-more"), None, Side::Below);
        glyph_more(ui.painter(), more.shrink(8.0), if enabled { p.text_secondary } else { p.tab_border });
        self.buttons.push(("more", more));
        let opened = enabled && response.clicked();
        if opened {
            self.more = match self.more {
                Some(_) => None,
                None => {
                    let params = match &state {
                        WidgetState::Selected(id) | WidgetState::Running(id, _) => json!({ "configurationId": id }),
                        WidgetState::Empty => json!({}),
                    };
                    let supported = registry
                        .dispatch(session, "coverage.run.supports", params)
                        .is_ok_and(|answer| answer["supported"] == true);
                    Some(supported)
                }
            };
        }
        if let Some(supported) = self.more {
            let item = MoreItem {
                label: t!("ide-run-widget-coverage", name = name.as_deref().unwrap_or_default()),
                configuration_id: match &state {
                    WidgetState::Selected(id) | WidgetState::Running(id, _) => Some(id.clone()),
                    WidgetState::Empty => None,
                },
                supported,
                opened,
            };
            if let Some(chosen) = self.more_menu(ui, p, more, item) {
                action = Some(chosen);
            }
        } else {
            self.more_rows.clear();
        }
        x -= 30.0;
        let debug = Rect::from_center_size(pos2(x - 14.0, y), Vec2::splat(28.0));
        let response = ui.interact(debug, ui.scope_id().with("run-widget-debug"), Sense::click());
        if enabled {
            hover(ui, &response, p);
        }
        let label = match &name {
            Some(name) => t!("ide-run-widget-debug", name = name.as_str()),
            None => t!("ide-action-debug"),
        };
        hint(ui, &response, &label, crate::keymap::shortcut("debug.session.start").as_deref(), Side::Below);
        if response.clicked()
            && let WidgetState::Selected(id) | WidgetState::Running(id, _) = &state
        {
            action = Some(WidgetAction::Dispatch("debug.session.start", json!({ "configurationId": id })));
        }
        super::glyphs::glyph_bug(ui.painter(), debug.shrink(7.0), if enabled { p.run } else { p.text_secondary });
        self.buttons.push(("debug", debug));
        x -= 30.0;
        let run = Rect::from_center_size(pos2(x - 14.0, y), Vec2::splat(28.0));
        let response = ui.interact(run, ui.scope_id().with("run-widget-run"), Sense::click());
        if enabled {
            hover(ui, &response, p);
        }
        let multiple = self.selected_configuration().is_some_and(|chosen| chosen.common.allow_multiple_instances);
        let rerun = matches!(state, WidgetState::Running(..)) && !multiple;
        let label = match (&state, &name) {
            (WidgetState::Running(..), Some(name)) if rerun => t!("ide-run-widget-rerun", name = name.as_str()),
            (_, Some(name)) => t!("ide-run-widget-run", name = name.as_str()),
            _ => t!("ide-action-run"),
        };
        hint(ui, &response, &label, crate::keymap::shortcut("run.process.start").as_deref(), Side::Below);
        if response.clicked() {
            action = match &state {
                WidgetState::Selected(id) => {
                    Some(WidgetAction::Dispatch("run.process.start", json!({ "configurationId": id })))
                }
                WidgetState::Running(id, _) if multiple || compound => {
                    Some(WidgetAction::Dispatch("run.process.start", json!({ "configurationId": id })))
                }
                WidgetState::Running(id, run) => Some(WidgetAction::Rerun(*run, id.clone())),
                WidgetState::Empty => action,
            };
        }
        let color = if enabled { p.run } else { p.text_secondary };
        if rerun {
            glyph_rerun(ui.painter(), run.shrink(7.0), color);
        } else {
            glyph_run(ui.painter(), run.shrink(7.0), color);
        }
        self.buttons.push(("run", run));
        let label = name.clone().unwrap_or_else(|| t!("ide-run-add-configuration"));
        let galley = ui.painter().layout_no_wrap(label, theme::regular(13.0), p.text);
        let chevron = if enabled { 16.0 } else { 0.0 };
        let width = galley.size().x + 20.0 + chevron;
        let selector = Rect::from_min_size(pos2(run.left() - 4.0 - width, y - 14.0), vec2(width, 28.0));
        let response = ui.interact(selector, ui.scope_id().with("run-widget-configuration"), Sense::click());
        let shown = std::sync::Arc::clone(&galley);
        accessibility::name(&response, if enabled { egui::Role::ComboBox } else { egui::Role::Button }, shown.text());
        if enabled {
            accessibility::edit(&response, |node| node.set_expanded(self.popup));
        }
        hover(ui, &response, p);
        ui.painter().galley(pos2(selector.left() + 10.0, y - galley.size().y / 2.0), galley, p.text);
        if enabled {
            let arrow = Rect::from_center_size(pos2(selector.right() - 14.0, y), vec2(8.0, 5.0));
            glyph_chevron(ui.painter(), arrow, p.text_secondary);
        }
        self.buttons.push(("selector", selector));
        if response.clicked() {
            if enabled {
                self.popup = !self.popup;
            } else {
                action = Some(WidgetAction::EditConfigurations);
            }
        }
        if self.popup {
            if let Some(chosen) = self.popup(ui, p, selector) {
                action = Some(chosen);
            }
        } else {
            self.popup_rows.clear();
        }
        (selector.left(), action)
    }

    fn more_menu(&mut self, ui: &Ui, p: &IdePalette, button: Rect, item: MoreItem) -> Option<WidgetAction> {
        let MoreItem { label, configuration_id, supported, opened } = item;
        let mut clicked = false;
        let mut rows = Vec::new();
        let area = egui::Area::new(ui.scope_id().with("run-widget-more-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(button.left(), button.bottom() + 4.0))
            .show(ui.ctx(), |ui| {
                super::doc_popups::popup_frame(p).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(13.0), p.text);
                    let width = (galley.size().x + 44.0).max(200.0);
                    let (rect, response) = ui.allocate_exact_size(vec2(width, POPUP_ROW), Sense::click());
                    accessibility::name(&response, egui::Role::MenuItem, &label);
                    let color = if supported { p.text } else { p.text_secondary };
                    if supported && response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    }
                    let icon = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(11.0));
                    glyph_coverage(ui.painter(), icon, if supported { p.run } else { p.text_secondary });
                    ui.painter().text(
                        pos2(rect.left() + 28.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &label,
                        theme::regular(13.0),
                        color,
                    );
                    clicked = supported && response.clicked();
                    rows.push(rect);
                });
            });
        self.more_rows = rows;
        let closing = !opened
            && ui.input(|input| {
                input.key_pressed(Key::Escape)
                    || (input.pointer.any_pressed()
                        && input.pointer.interact_pos().is_some_and(|pos| !area.response.rect.contains(pos)))
            });
        if closing || clicked {
            self.more = None;
        }
        let id = configuration_id.filter(|_| clicked)?;
        Some(WidgetAction::Dispatch("coverage.run.start", json!({ "configurationId": id })))
    }

    fn popup(&mut self, ui: &Ui, p: &IdePalette, selector: Rect) -> Option<WidgetAction> {
        let configurations = self.configurations.clone().unwrap_or_default();
        let selected = self.selected_configuration().map(|configuration| configuration.id.clone());
        let mut rows = Vec::new();
        let mut chosen = None;
        let area = egui::Area::new(ui.scope_id().with("run-widget-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(selector.left(), selector.bottom() + 4.0))
            .show(ui.ctx(), |ui| {
                super::doc_popups::popup_frame(p).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.set_min_width(selector.width().max(240.0));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut row = |ui: &mut Ui, text: String, color, current: bool, entry: PopupRow| {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(ui.available_width(), POPUP_ROW), Sense::click());
                        accessibility::item(&response, egui::Role::ListBoxOption, current, &text);
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 4, p.selection);
                        } else if current {
                            ui.painter().rect_filled(rect, 4, p.popup_hover);
                        }
                        let icon = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(10.0));
                        if matches!(entry, PopupRow::Configuration(_)) {
                            glyph_run(ui.painter(), icon, if color == p.text { p.run } else { color });
                        }
                        let galley = ui.painter().layout_no_wrap(text, theme::regular(13.0), color);
                        ui.painter().galley(
                            pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0),
                            galley,
                            color,
                        );
                        rows.push((entry.clone(), rect));
                        if response.clicked() {
                            chosen = Some(entry);
                        }
                    };
                    for configuration in &configurations {
                        let color = if configuration.temporary { p.text_secondary } else { p.text };
                        let current = selected.as_ref() == Some(&configuration.id);
                        row(
                            ui,
                            configuration.name.clone(),
                            color,
                            current,
                            PopupRow::Configuration(configuration.id.clone()),
                        );
                    }
                    ui.add_space(4.0);
                    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, p.tab_border));
                    ui.add_space(4.0);
                    row(ui, t!("ide-run-widget-edit"), p.text, false, PopupRow::Edit);
                });
            });
        self.popup_rows = rows;
        let closing = ui.input(|input| {
            input.key_pressed(Key::Escape)
                || (input.pointer.any_pressed()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|pos| !area.response.rect.contains(pos) && !selector.contains(pos)))
        });
        if closing {
            self.popup = false;
        }
        let chosen = chosen?;
        self.popup = false;
        Some(match chosen {
            PopupRow::Configuration(id) => WidgetAction::Dispatch("run.configuration.select", json!({ "id": id })),
            PopupRow::Edit => WidgetAction::EditConfigurations,
        })
    }
}

fn glyph_stop(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    painter.rect_filled(rect, rect.width() * 0.15, color);
}

fn glyph_rerun(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let stroke = Stroke::new((rect.width() / 8.0).max(1.4), color);
    let center = rect.center();
    let radius = rect.width() * 0.42;
    let points: Vec<egui::Pos2> = (0..=20u8)
        .map(|step| {
            let angle = -0.6 + f32::from(step) * (std::f32::consts::TAU * 0.8) / 20.0;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    let end = *points.last().unwrap_or(&center);
    painter.add(egui::Shape::line(points, stroke));
    let size = rect.width() * 0.3;
    painter.add(egui::Shape::convex_polygon(
        vec![end + vec2(-size, -size * 0.2), end + vec2(size * 0.4, -size), end + vec2(size * 0.5, size * 0.3)],
        color,
        Stroke::NONE,
    ));
}

fn glyph_chevron(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.line_segment([rect.left_top(), rect.center_bottom()], stroke);
    painter.line_segment([rect.center_bottom(), rect.right_top()], stroke);
}

pub(super) fn rerun_commands(configuration_id: &str) -> [(&'static str, Value); 1] {
    [("run.process.start", json!({ "configurationId": configuration_id }))]
}

impl IdeView {
    pub(super) fn run_widget(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        right: f32,
        y: f32,
    ) -> f32 {
        let p = self.palette;
        let (left, action) = self.run_widget.show(ui, &p, registry, session, right, y);
        match action {
            Some(WidgetAction::Dispatch(command @ ("debug.session.start" | "coverage.run.start"), params)) => {
                self.run_action(ui, registry, session, command, params);
                self.run_widget.invalidate();
            }
            Some(WidgetAction::Dispatch(command, params)) => {
                self.run_action(ui, registry, session, command, params);
                self.run_widget.invalidate();
            }
            Some(WidgetAction::Rerun(_, id)) => {
                for (command, params) in rerun_commands(&id) {
                    self.run_action(ui, registry, session, command, params);
                }
                self.run_widget.invalidate();
            }
            Some(WidgetAction::EditConfigurations) => {
                self.run_action(ui, registry, session, "view.runConfigurations.open", json!({}));
            }
            None => {}
        }
        left
    }
}

#[cfg(test)]
mod tests;
