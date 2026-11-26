use std::time::{Duration, Instant};

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::workspaces::InnerTab;

use super::super::glyphs::{Glyph, glyph_refresh, glyph_run, glyph_undo};
use super::super::transaction_dialog;
use super::super::{IdePalette, theme, vcs};
use super::{Listed, message};
use crate::accessibility;

const BAR: f32 = 36.0;
const HEAD: f32 = 24.0;
const ROW: f32 = 24.0;
const POLL: Duration = Duration::from_millis(250);
const LISTED: usize = 500;
const WIDTHS: [f32; 4] = [110.0, 170.0, 220.0, 220.0];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Refresh,
    Up,
    Down,
    Connect,
}

#[derive(Default)]
pub(super) struct MigrationsView {
    key: Option<(u64, String, String, String)>,
    read: Option<Instant>,
    sets: Value,
    status: Value,
    error: Option<String>,
    notice: Option<String>,
}

fn state_label(state: &str, file: bool) -> String {
    match (state, file) {
        (_, false) => t!("ide-dbmigrations-state-missing"),
        ("applied", _) => t!("ide-dbmigrations-state-applied"),
        ("pending", _) => t!("ide-dbmigrations-state-pending"),
        ("failed", _) => t!("ide-dbmigrations-state-failed"),
        _ => t!("ide-dbmigrations-state-unknown"),
    }
}

fn state_color(p: &IdePalette, state: &str, file: bool) -> Color32 {
    match (state, file) {
        (_, false) | ("failed", _) => p.error_text,
        ("applied", _) => p.run,
        ("pending", _) => p.warning_text,
        _ => p.text_secondary,
    }
}

fn set_label(set: &Value) -> String {
    format!("{} · {}", set["toolName"].as_str().unwrap_or_default(), set["folder"].as_str().unwrap_or_default())
}

fn columns(width: f32) -> [f32; 4] {
    let natural: f32 = WIDTHS.iter().sum();
    if width >= natural {
        let mut widths = WIDTHS;
        widths[3] = width - WIDTHS[..3].iter().sum::<f32>();
        return widths;
    }
    WIDTHS.map(|column| column * width.max(0.0) / natural)
}

fn busy(status: &Value, sets: &Value) -> bool {
    sets["scanning"] == true
        || status["loading"] == true
        || status["run"]["waiting"] == true
        || status["run"]["state"] == "running"
        || status["run"]["state"] == "starting"
}

impl MigrationsView {
    fn chosen_set(&self, tab: &InnerTab) -> Option<Value> {
        let sets = self.sets["sets"].as_array()?;
        let wanted = tab.data["set"].as_str();
        sets.iter().find(|set| Some(set["id"].as_str().unwrap_or_default()) == wanted).or(sets.first()).cloned()
    }

    fn read(&mut self, registry: &Registry, session: &mut Session, tab: &InnerTab, refresh: bool) {
        self.sets = registry
            .dispatch(session, "db.migration.list", json!({ "limit": LISTED, "refresh": refresh }))
            .unwrap_or_else(|error| {
                self.error = Some(message(error));
                Value::Null
            });
        self.status = Value::Null;
        if let Some(set) = self.chosen_set(tab) {
            let mut params = json!({ "set": set["id"], "limit": LISTED, "refresh": refresh });
            if let Some(connection) = tab.data["connection"].as_str() {
                params["connection"] = json!(connection);
            }
            match registry.dispatch(session, "db.migration.status", params) {
                Ok(status) => self.status = status,
                Err(error) => self.error = Some(message(error)),
            }
        }
        self.read = Some(Instant::now());
    }

    fn act(&mut self, registry: &Registry, session: &mut Session, tab: &InnerTab, action: Action) {
        self.notice = None;
        self.error = None;
        let connection = tab.data["connection"].as_str().unwrap_or_default().to_owned();
        let set = self.chosen_set(tab).map(|set| set["id"].clone()).unwrap_or_default();
        let direction = match action {
            Action::Refresh => {
                self.read(registry, session, tab, true);
                return;
            }
            Action::Connect => {
                if let Err(error) =
                    registry.dispatch(session, "db.connection.connect", json!({ "connection": connection }))
                {
                    self.error = Some(message(error));
                }
                return;
            }
            Action::Up => "up",
            Action::Down => "down",
        };
        let params = json!({ "set": set, "connection": connection, "direction": direction });
        match registry.dispatch(session, "db.migration.run", params) {
            Ok(started) => self.notice = started["command"].as_str().map(str::to_owned),
            Err(error) if transaction_dialog::ask(session, &error) => {}
            Err(error) => self.error = Some(message(error)),
        }
        self.key = None;
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (tab, listed): (&InnerTab, &Listed),
    ) {
        let connection = tab.data["connection"].as_str().unwrap_or_default().to_owned();
        let known = listed.connections.iter().find(|known| known["id"] == connection.as_str());
        let connection_state = known.and_then(|known| known["state"].as_str()).unwrap_or("disconnected").to_owned();
        let read_only = known.is_some_and(|known| known["readOnly"] == true);
        let revision =
            session.state::<zen_workbench::database::migrations::Migrations>().map_or(0, |state| state.revision());
        let set = tab.data["set"].as_str().unwrap_or_default().to_owned();
        let key = (revision, connection.clone(), set, format!("{connection_state}/{read_only}"));
        let stale = self.read.is_none_or(|read| busy(&self.status, &self.sets) && read.elapsed() >= POLL);
        if self.key.as_ref() != Some(&key) || stale {
            self.key = Some(key);
            self.read(registry, session, tab, false);
        }
        if busy(&self.status, &self.sets) {
            ui.ctx().request_repaint_after(POLL);
        }
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.editor_background);
        let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR));
        let mut actions = Vec::new();
        let mut bar_ui = ui.new_child(UiBuilder::new().id_salt(("database-migrations-bar", &tab.id)).max_rect(bar));
        self.bar(&mut bar_ui, p, (registry, session), (tab, listed, bar), &mut actions);
        let mut top = bar.bottom();
        let summary = self.summary();
        let run_error = self.status["error"].as_str().map(str::to_owned);
        for (text, color) in [
            (self.error.clone().or(run_error), p.error_text),
            (self.notice.clone(), p.text_secondary),
            (summary, p.text_secondary),
        ] {
            let Some(text) = text else { continue };
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), 24.0));
            let job = vcs::truncated(&text, theme::regular(12.0), color, strip.width() - 24.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(strip.left() + 12.0, strip.center().y - galley.size().y / 2.0), galley, color);
            top = strip.bottom();
        }
        let body = Rect::from_min_max(pos2(area.left(), top), area.max);
        if connection_state != "connected" && !connection.is_empty() && self.status["migrations"].is_array() {
            let strip = Rect::from_min_size(body.min, vec2(body.width(), 32.0));
            let mut child = ui.new_child(
                UiBuilder::new().id_salt("database-migrations-connect").max_rect(strip.shrink2(vec2(12.0, 2.0))),
            );
            child.horizontal_centered(|ui| {
                let connect = (connection_state != "connecting").then(|| t!("ide-dbmigrations-connect"));
                let reserved = connect.as_ref().map_or(0.0, |label| {
                    ui.painter().layout_no_wrap(label.clone(), theme::regular(13.0), p.text).size().x + 48.0
                });
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - reserved).max(0.0));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(t!("ide-dbmigrations-offline"))
                                .font(theme::regular(12.5))
                                .color(p.text_secondary),
                        )
                        .truncate(),
                    );
                });
                if let Some(connect) = connect
                    && vcs::secondary_button(ui, p, &connect).clicked()
                {
                    actions.push(Action::Connect);
                }
            });
            self.table(ui, p, Rect::from_min_max(pos2(body.left(), strip.bottom()), body.max));
        } else {
            self.table(ui, p, body);
        }
        for action in actions {
            self.act(registry, session, tab, action);
        }
        ui.advance_cursor_after_rect(area);
    }

    fn summary(&self) -> Option<String> {
        let status = &self.status;
        if !status["counts"].is_object() {
            return None;
        }
        let counts = &status["counts"];
        let count = |field: &str| counts[field].as_u64().unwrap_or(0);
        let mut text = if status["connected"] == true {
            t!(
                "ide-dbmigrations-counts",
                applied = count("applied"),
                pending = count("pending"),
                failed = count("failed") + count("missing")
            )
        } else {
            t!("ide-dbmigrations-files", count = count("unknown"))
        };
        if status["table"] == false {
            text = format!(
                "{text} · {}",
                t!("ide-dbmigrations-no-table", table = status["set"]["table"].as_str().unwrap_or_default())
            );
        }
        let run = &status["run"];
        if run.is_object() {
            let line = if run["waiting"] == true {
                t!("ide-dbmigrations-run-waiting")
            } else if run["state"] == "running" || run["state"] == "starting" {
                t!("ide-dbmigrations-run-running")
            } else {
                t!(
                    "ide-dbmigrations-run-ended",
                    code = run["exitCode"].as_i64().map_or_else(|| "-".to_owned(), |code| code.to_string())
                )
            };
            text = format!("{text} · {line}");
        }
        if status["connection"].is_null() {
            text = format!("{text} · {}", t!("ide-dbmigrations-choose-connection"));
        }
        Some(text)
    }

    fn bar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        (registry, session): (&Registry, &mut Session),
        (tab, listed, bar): (&InnerTab, &Listed, Rect),
        actions: &mut Vec<Action>,
    ) {
        ui.painter().rect_filled(bar, 0, p.island);
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let sets: Vec<Value> = self.sets["sets"].as_array().cloned().unwrap_or_default();
        let chosen = self.chosen_set(tab);
        let set_text = match (&chosen, self.sets["scanning"] == true) {
            (Some(set), _) => set_label(set),
            (None, true) => t!("ide-dbmigrations-scanning"),
            (None, false) => t!("ide-dbmigrations-no-sets"),
        };
        let mut open = serde_json::Map::new();
        let (set_button, at) =
            super::DatabaseView::selector(ui, p, bar.left() + 8.0, (&set_text, chosen.is_none()), "migrations-set");
        egui::Popup::menu(&set_button).show(|ui| {
            ui.set_min_width(240.0);
            for set in &sets {
                let selected = chosen.as_ref().is_some_and(|chosen| chosen["id"] == set["id"]);
                if ui.selectable_label(selected, set_label(set)).clicked() {
                    open.insert("set".to_owned(), set["id"].clone());
                    ui.close();
                }
            }
        });
        let connection = tab.data["connection"].as_str().unwrap_or_default();
        let name = listed
            .connections
            .iter()
            .find(|known| known["id"] == connection)
            .and_then(|known| known["name"].as_str())
            .map_or_else(|| t!("ide-dbmigrations-no-connection"), str::to_owned);
        let (connection_button, at) =
            super::DatabaseView::selector(ui, p, at, (&name, connection.is_empty()), "migrations-connection");
        egui::Popup::menu(&connection_button).show(|ui| {
            ui.set_min_width(200.0);
            for known in &listed.connections {
                let id = known["id"].as_str().unwrap_or_default();
                if ui.selectable_label(id == connection, known["name"].as_str().unwrap_or(id)).clicked() {
                    open.insert("connection".to_owned(), json!(id));
                    ui.close();
                }
            }
        });
        if !open.is_empty()
            && let Err(error) = registry.dispatch(session, "db.migration.open", Value::Object(open))
        {
            self.error = Some(message(error));
        }
        let has_set = chosen.is_some();
        let read_only = self.status["readOnly"] == true
            || listed.connections.iter().any(|known| known["id"] == connection && known["readOnly"] == true);
        let runnable = has_set && !connection.is_empty() && !read_only && !busy(&self.status, &Value::Null);
        let reverts = chosen.as_ref().is_some_and(|set| set["reverts"] == true);
        let refused = read_only.then(|| t!("ide-dbmigrations-read-only", name = name.as_str()));
        let tools: [(Action, Glyph, Color32, String, bool); 3] = [
            (Action::Refresh, glyph_refresh, p.text_secondary, t!("ide-dbmigrations-refresh"), true),
            (Action::Up, glyph_run, p.run, t!("ide-dbmigrations-up"), runnable),
            (Action::Down, glyph_undo, p.warning_text, t!("ide-dbmigrations-down"), runnable && reverts),
        ];
        let mut at = at + 8.0;
        let font = theme::regular(12.5);
        let full: f32 = tools
            .iter()
            .map(|(.., label, _)| ui.painter().layout_no_wrap(label.clone(), font.clone(), p.text).size().x + 38.0)
            .sum();
        let compact = at + full > bar.right();
        for (action, glyph, color, label, enabled) in tools {
            let reason = refused.as_deref().filter(|_| action != Action::Refresh);
            let (clicked, next) = tool(ui, p, (at, compact), (glyph, color), (enabled, reason), &label);
            at = next;
            if clicked {
                actions.push(action);
            }
        }
    }

    fn table(&self, ui: &mut Ui, p: &IdePalette, rect: Rect) {
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEAD));
        ui.painter().rect_filled(head, 0, p.island);
        ui.painter().hline(head.x_range(), head.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let widths = columns(rect.width() - 16.0);
        let titles = [
            t!("ide-dbmigrations-col-state"),
            t!("ide-dbmigrations-col-version"),
            t!("ide-dbmigrations-col-name"),
            t!("ide-dbmigrations-col-file"),
        ];
        let mut x = head.left() + 16.0;
        for (title, width) in titles.iter().zip(widths) {
            let job = vcs::truncated(title, theme::semibold(12.0), p.text_secondary, width - 8.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(x + 4.0, head.center().y - galley.size().y / 2.0), galley, p.text_secondary);
            x += width;
        }
        let list = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
        let migrations: Vec<Value> = self.status["migrations"].as_array().cloned().unwrap_or_default();
        if migrations.is_empty() {
            let text = if self.sets["scanning"] == true {
                t!("ide-dbmigrations-scanning")
            } else if self.sets["total"] == 0 {
                t!("ide-dbmigrations-empty")
            } else {
                t!("ide-dbmigrations-no-migrations")
            };
            ui.painter().text(
                pos2(list.center().x, list.top() + 30.0),
                Align2::CENTER_CENTER,
                text,
                theme::regular(13.0),
                p.text_secondary,
            );
            return;
        }
        let mut child = ui.new_child(UiBuilder::new().id_salt("database-migrations-list").max_rect(list));
        child.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            &mut child,
            ROW,
            migrations.len(),
            |ui, range| {
                for migration in &migrations[range] {
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    if response.hovered() {
                        ui.painter().rect_filled(row, 0, p.hover);
                    }
                    let state = migration["state"].as_str().unwrap_or_default();
                    let file = migration["file"].as_str();
                    let color = state_color(p, state, file.is_some());
                    let label = state_label(state, file.is_some());
                    ui.painter().circle_filled(pos2(row.left() + 8.0, row.center().y), 3.5, color);
                    let cells = [
                        (label.clone(), theme::regular(12.5), color),
                        (migration["version"].as_str().unwrap_or_default().to_owned(), FontId::monospace(12.0), p.text),
                        (migration["name"].as_str().unwrap_or_default().to_owned(), theme::regular(12.5), p.text),
                        (file.unwrap_or_default().to_owned(), theme::regular(12.5), p.text_secondary),
                    ];
                    let mut x = row.left() + 16.0;
                    for (index, ((text, font, text_color), width)) in cells.into_iter().zip(widths).enumerate() {
                        let job = vcs::truncated(&text, font, text_color, width - 8.0);
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                        let elided = galley.elided;
                        ui.painter().galley(pos2(x + 4.0, row.center().y - galley.size().y / 2.0), galley, text_color);
                        let cell = Rect::from_min_size(pos2(x, row.top()), vec2(width, ROW));
                        if elided || index == 3 && !text.is_empty() {
                            ui.interact(
                                cell,
                                ui.scope_id().with(("database-migrations-cell", index, &text)),
                                Sense::hover(),
                            )
                            .on_hover_text(text);
                        }
                        x += width;
                    }
                    accessibility::name_with(&response, egui::Role::ListItem, || {
                        format!(
                            "{label} {} {}",
                            migration["version"].as_str().unwrap_or_default(),
                            migration["name"].as_str().unwrap_or_default()
                        )
                    });
                }
            },
        );
    }
}

fn tool(
    ui: &mut Ui,
    p: &IdePalette,
    (at, compact): (f32, bool),
    (glyph, color): (Glyph, Color32),
    (enabled, reason): (bool, Option<&str>),
    label: &str,
) -> (bool, f32) {
    let bar = ui.max_rect();
    let font = theme::regular(12.5);
    let width = if compact {
        26.0
    } else {
        ui.painter().layout_no_wrap(label.to_owned(), font.clone(), p.text).size().x + 34.0
    };
    let button = Rect::from_min_size(pos2(at, bar.center().y - 12.0), vec2(width, 24.0));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let mut response = ui.interact(button, ui.scope_id().with(("database-migrations-tool", label)), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(button, 4, p.hover);
    }
    let (glyph_color, text_color) =
        if enabled { (color, p.text) } else { (p.text_secondary.gamma_multiply(0.5), p.text_secondary) };
    glyph(
        ui.painter(),
        Rect::from_center_size(pos2(button.left() + 13.0, button.center().y), vec2(12.0, 12.0)),
        glyph_color,
    );
    if !compact {
        ui.painter().text(pos2(button.left() + 26.0, button.center().y), Align2::LEFT_CENTER, label, font, text_color);
    }
    accessibility::button_with(&response, || label.to_owned());
    if !enabled {
        accessibility::disabled(&response);
    }
    match (reason, compact) {
        (Some(reason), true) => response = response.on_hover_text(format!("{label}: {reason}")),
        (Some(reason), false) => response = response.on_hover_text(reason),
        (None, true) => response = response.on_hover_text(label),
        (None, false) => {}
    }
    (response.clicked(), button.right() + 4.0)
}

#[cfg(test)]
mod tests;
