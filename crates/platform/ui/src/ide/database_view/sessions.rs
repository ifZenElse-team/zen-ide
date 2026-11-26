use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Align, Align2, Color32, FontId, Layout, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_workbench::workspaces::InnerTab;

use super::super::glyphs::{Glyph, glyph_close, glyph_refresh, glyph_stop};
use super::super::{IdePalette, customization, theme, vcs};
use super::{Listed, message};
use crate::accessibility;

const BAR: f32 = 36.0;
const HEAD: f32 = 24.0;
const ROW: f32 = 24.0;
const DETAIL: f32 = 0.38;
const LISTED: usize = 1000;
const POLL: Duration = Duration::from_millis(100);
const INTERVALS: [u64; 5] = [0, 2, 5, 10, 30];

#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Cancel,
    Terminate,
}

impl End {
    fn command(self) -> &'static str {
        match self {
            End::Cancel => "db.session.cancelQuery",
            End::Terminate => "db.session.terminate",
        }
    }

    fn label(self, mysql: bool) -> String {
        match (self, mysql) {
            (End::Cancel, false) => t!("ide-dbsessions-cancel-query"),
            (End::Terminate, false) => t!("ide-dbsessions-terminate"),
            (End::Cancel, true) => t!("ide-dbsessions-kill-query"),
            (End::Terminate, true) => t!("ide-dbsessions-kill-session"),
        }
    }
}

struct Question {
    label: String,
    command: String,
    params: Value,
    message: String,
    statement: String,
    risks: Vec<String>,
    dangerous: bool,
    understood: bool,
}

#[derive(Default)]
struct SessionsTab {
    connection: String,
    listing: Option<u64>,
    ending: Option<u64>,
    answer: Option<Value>,
    read: Option<Instant>,
    selected: Option<String>,
    question: Option<Question>,
    error: Option<String>,
    notice: Option<String>,
}

#[derive(Default)]
pub(in super::super) struct SessionsViews {
    tabs: HashMap<String, SessionsTab>,
}

struct Column {
    title: String,
    width: f32,
    cell: fn(&Value) -> String,
}

fn text_of(session: &Value, field: &str) -> String {
    session[field].as_str().unwrap_or_default().to_owned()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn columns(mysql: bool) -> Vec<Column> {
    let id = |session: &Value| {
        let id = text_of(session, "id");
        if session["own"] == true { format!("{id} · {}", t!("ide-dbsessions-own")) } else { id }
    };
    let query = |session: &Value| one_line(&text_of(session, "query"));
    if mysql {
        vec![
            Column { title: t!("ide-dbsessions-col-id"), width: 86.0, cell: id },
            Column { title: t!("ide-dbsessions-col-user"), width: 100.0, cell: |s| text_of(s, "user") },
            Column { title: t!("ide-dbsessions-col-host"), width: 150.0, cell: |s| text_of(s, "host") },
            Column { title: t!("ide-dbsessions-col-database"), width: 110.0, cell: |s| text_of(s, "database") },
            Column { title: t!("ide-dbsessions-col-command"), width: 84.0, cell: |s| text_of(s, "command") },
            Column {
                title: t!("ide-dbsessions-col-time"),
                width: 60.0,
                cell: |s| s["seconds"].as_u64().map(|seconds| seconds.to_string()).unwrap_or_default(),
            },
            Column { title: t!("ide-dbsessions-col-state"), width: 150.0, cell: |s| text_of(s, "state") },
            Column { title: t!("ide-dbsessions-col-query"), width: 0.0, cell: query },
        ]
    } else {
        vec![
            Column { title: t!("ide-dbsessions-col-pid"), width: 86.0, cell: id },
            Column { title: t!("ide-dbsessions-col-user"), width: 100.0, cell: |s| text_of(s, "user") },
            Column { title: t!("ide-dbsessions-col-database"), width: 100.0, cell: |s| text_of(s, "database") },
            Column { title: t!("ide-dbsessions-col-application"), width: 150.0, cell: |s| text_of(s, "application") },
            Column { title: t!("ide-dbsessions-col-query-start"), width: 140.0, cell: |s| text_of(s, "queryStart") },
            Column { title: t!("ide-dbsessions-col-state"), width: 130.0, cell: |s| text_of(s, "state") },
            Column { title: t!("ide-dbsessions-col-query"), width: 0.0, cell: query },
        ]
    }
}

fn interval_label(seconds: u64) -> String {
    if seconds == 0 { t!("ide-dbsessions-auto-off") } else { t!("ide-dbsessions-auto-every", seconds = seconds) }
}

fn failure(error: DispatchError) -> (Option<String>, String, Option<Value>) {
    match error {
        DispatchError::Failed { error, .. } => {
            let data = error.data.clone().map(Value::Object);
            (error.code.clone(), error.message, data)
        }
        other => (None, other.to_string(), None),
    }
}

fn mark_color(p: &IdePalette, session: &Value) -> Option<Color32> {
    let waits = session["blockedBy"].as_array().is_some_and(|ids| !ids.is_empty());
    let blocks = session["blocking"].as_array().is_some_and(|ids| !ids.is_empty());
    match (waits, blocks) {
        (_, true) => Some(p.error_text),
        (true, false) => Some(p.warning_text),
        _ => None,
    }
}

fn ids(value: &Value) -> String {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
}

impl SessionsTab {
    fn sessions(&self) -> &[Value] {
        self.answer.as_ref().and_then(|answer| answer["sessions"]["sessions"].as_array()).map_or(&[], Vec::as_slice)
    }

    fn list(&mut self, registry: &Registry, session: &mut Session, tab: &InnerTab) {
        let params = json!({
            "connection": self.connection,
            "showIdle": tab.data["showIdle"].as_bool().unwrap_or(true),
            "limit": LISTED,
        });
        match registry.dispatch(session, "db.session.list", params) {
            Ok(answer) => {
                self.listing = answer["operation"].as_u64();
                self.error = None;
                self.take(answer);
            }
            Err(error) => {
                self.listing = None;
                self.read = Some(Instant::now());
                self.error = Some(message(error));
            }
        }
    }

    fn take(&mut self, answer: Value) {
        if answer["status"] == "running" {
            return;
        }
        self.listing = None;
        self.read = Some(Instant::now());
        if answer["status"] == "done" {
            let listed = answer["sessions"]["sessions"].as_array().cloned().unwrap_or_default();
            if self.selected.as_ref().is_some_and(|id| !listed.iter().any(|session| session["id"] == id.as_str())) {
                self.selected = None;
            }
            self.answer = Some(answer);
        } else {
            self.error = answer["message"].as_str().map(str::to_owned);
        }
    }

    fn poll(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session, tab: &InnerTab) {
        if let Some(operation) = self.listing {
            match registry.dispatch(session, "db.session.state", json!({ "operation": operation })) {
                Ok(answer) => self.take(answer),
                Err(error) => {
                    self.listing = None;
                    self.error = Some(message(error));
                }
            }
            ctx.request_repaint_after(POLL);
        }
        if let Some(operation) = self.ending {
            match registry.dispatch(session, "db.session.state", json!({ "operation": operation })) {
                Ok(answer) if answer["status"] == "running" => {}
                Ok(answer) => {
                    self.ending = None;
                    let id = answer["ended"]["statement"].as_str().unwrap_or_default().to_owned();
                    if answer["status"] == "done" {
                        self.notice = Some(if answer["ended"]["ended"] == true {
                            t!("ide-dbsessions-ended", statement = id.as_str())
                        } else {
                            t!("ide-dbsessions-not-found", statement = id.as_str())
                        });
                    } else {
                        self.error = answer["message"].as_str().map(str::to_owned);
                    }
                    self.list(registry, session, tab);
                }
                Err(error) => {
                    self.ending = None;
                    self.error = Some(message(error));
                }
            }
            ctx.request_repaint_after(POLL);
        }
    }

    fn ask(&mut self, registry: &Registry, session: &mut Session, (end, id, own): (End, String, bool), mysql: bool) {
        let params = json!({ "connection": self.connection, "session": id, "own": own });
        self.notice = None;
        match registry.dispatch(session, end.command(), params) {
            Ok(_) => self.error = None,
            Err(error) => {
                let (code, text, data) = failure(error);
                let data = data.unwrap_or_default();
                if code.as_deref() == Some(zen_workbench::database::safety::CONFIRM_EXECUTE) {
                    let statement = &data["statements"][0];
                    self.error = None;
                    self.question = Some(Question {
                        label: end.label(mysql),
                        command: data["retry"]["command"].as_str().unwrap_or(end.command()).to_owned(),
                        params: data["retry"]["params"].clone(),
                        message: text,
                        statement: statement["text"].as_str().unwrap_or_default().to_owned(),
                        risks: statement["risks"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                        dangerous: data["dangerous"] == true,
                        understood: false,
                    });
                } else {
                    self.error = Some(text);
                }
            }
        }
    }

    fn confirm(&mut self, registry: &Registry, session: &mut Session) {
        let Some(question) = self.question.take() else { return };
        match registry.dispatch(session, &question.command, question.params) {
            Ok(answer) => {
                self.error = None;
                self.ending = answer["operation"].as_u64();
            }
            Err(error) => self.error = Some(message(error)),
        }
    }
}

impl SessionsViews {
    pub(in super::super) fn retain(&mut self, open: &[String]) {
        self.tabs.retain(|id, _| open.contains(id));
    }

    pub(in super::super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (tab, listed): (&InnerTab, &Listed),
    ) {
        let state = self.tabs.entry(tab.id.clone()).or_default();
        state.connection = tab.data["connection"].as_str().unwrap_or_default().to_owned();
        let known = listed.connections.iter().find(|known| known["id"] == state.connection.as_str());
        let connection_state = known.and_then(|known| known["state"].as_str()).unwrap_or("disconnected").to_owned();
        let driver = known.and_then(|known| known["driver"].as_str()).unwrap_or_default();
        let mysql = matches!(driver, "mysql" | "mariadb");
        let name = known.and_then(|known| known["name"].as_str()).unwrap_or(&state.connection).to_owned();
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.editor_background);
        let connected = connection_state == "connected";
        state.poll(ui.ctx(), registry, session, tab);
        let every = tab.data["autoRefresh"].as_u64().unwrap_or(0);
        if connected && state.listing.is_none() && state.ending.is_none() && state.question.is_none() {
            match state.read {
                None => state.list(registry, session, tab),
                Some(read) if every > 0 => {
                    let interval = Duration::from_secs(every);
                    if read.elapsed() >= interval {
                        state.list(registry, session, tab);
                    } else {
                        ui.ctx().request_repaint_after(interval - read.elapsed());
                    }
                }
                Some(_) => {}
            }
        }
        if !connected && state.listing.is_none() {
            state.read = None;
        }
        let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR));
        let mut bar_ui = ui.new_child(UiBuilder::new().id_salt(("database-sessions-bar", &tab.id)).max_rect(bar));
        let mut actions: Vec<(End, String, bool)> = Vec::new();
        bar_actions(state, &mut bar_ui, p, (registry, session), (tab, bar, mysql, connected), &mut actions);
        let mut top = bar.bottom();
        if state.question.is_some() {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), 88.0));
            question_strip(state, ui, p, registry, session, strip);
            top = strip.bottom();
        }
        for (text, color) in [(state.error.clone(), p.error_text), (state.notice.clone(), p.text_secondary)] {
            let Some(text) = text else { continue };
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), 24.0));
            let job = vcs::truncated(&text, theme::regular(12.0), color, strip.width() - 24.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(strip.left() + 12.0, strip.center().y - galley.size().y / 2.0), galley, color);
            top = strip.bottom();
        }
        let body = Rect::from_min_max(pos2(area.left(), top), area.max);
        if !connected && state.answer.is_none() {
            not_connected(ui, p, registry, session, (body, &name, &connection_state, &state.connection));
            ui.advance_cursor_after_rect(area);
            return;
        }
        let detail_height = (body.height() * DETAIL).max(140.0).min(body.height() * 0.6);
        let table = Rect::from_min_max(body.min, pos2(body.right(), body.bottom() - detail_height));
        draw_table(state, ui, p, (table, mysql), &mut actions);
        let detail = Rect::from_min_max(pos2(body.left(), table.bottom()), body.max);
        draw_detail(state, ui, p, detail);
        for action in actions {
            state.ask(registry, session, action, mysql);
        }
        ui.advance_cursor_after_rect(area);
    }
}

fn tool(
    ui: &mut Ui,
    p: &IdePalette,
    at: f32,
    (glyph, color): (Glyph, Color32),
    (enabled, compact): (bool, bool),
    label: &str,
) -> (bool, f32) {
    let bar = ui.max_rect();
    let font = theme::regular(12.5);
    let width = if compact { 26.0 } else { tool_width(ui, p, label) };
    let button = Rect::from_min_size(pos2(at, bar.center().y - 12.0), vec2(width, 24.0));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let mut response = ui.interact(button, ui.scope_id().with(("database-sessions-tool", label)), sense);
    if compact {
        response = response.on_hover_text(label);
    }
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
    (response.clicked(), button.right() + 4.0)
}

fn tool_width(ui: &Ui, p: &IdePalette, label: &str) -> f32 {
    ui.painter().layout_no_wrap(label.to_owned(), theme::regular(12.5), p.text).size().x + 34.0
}

fn bar_actions(
    state: &mut SessionsTab,
    ui: &mut Ui,
    p: &IdePalette,
    (registry, session): (&Registry, &mut Session),
    (tab, bar, mysql, connected): (&InnerTab, Rect, bool, bool),
    actions: &mut Vec<(End, String, bool)>,
) {
    ui.painter().rect_filled(bar, 0, p.island);
    ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));
    let busy = state.listing.is_some();
    let every = tab.data["autoRefresh"].as_u64().unwrap_or(0);
    let refresh_label = t!("ide-dbsessions-refresh");
    let idle_text = t!("ide-dbsessions-show-idle");
    let idle_width = ui.painter().layout_no_wrap(idle_text.clone(), theme::regular(12.5), p.text).size().x + 30.0;
    let interval_width =
        ui.painter().layout_no_wrap(interval_label(every), theme::regular(12.5), p.text).size().x.min(220.0) + 34.0;
    let labelled: f32 = [refresh_label.clone(), End::Cancel.label(mysql), End::Terminate.label(mysql)]
        .iter()
        .map(|label| tool_width(ui, p, label) + 4.0)
        .sum();
    let compact = 6.0 + labelled + 6.0 + idle_width + 4.0 + interval_width + 8.0 + 90.0 > bar.width();
    let (refresh, at) =
        tool(ui, p, bar.left() + 6.0, (glyph_refresh, p.text_secondary), (!busy, compact), &refresh_label);
    if refresh {
        state.notice = None;
        state.list(registry, session, tab);
    }
    let mut show_idle = tab.data["showIdle"].as_bool().unwrap_or(true);
    let check = Rect::from_min_size(pos2(at + 6.0, bar.top()), vec2(idle_width, BAR));
    let mut check_ui = ui.new_child(UiBuilder::new().max_rect(check).layout(Layout::left_to_right(Align::Center)));
    let label = egui::RichText::new(idle_text).font(theme::regular(12.5)).color(p.text);
    let toggled = check_ui.checkbox(&mut show_idle, label).changed();
    let at = check.right() + 4.0;
    let (chooser, at) = super::DatabaseView::selector(ui, p, at, (&interval_label(every), every == 0), "sessions-auto");
    let mut chosen_interval = None;
    egui::Popup::menu(&chooser).show(|ui| {
        ui.set_min_width(140.0);
        for seconds in INTERVALS {
            let label =
                if seconds == 0 { t!("ide-dbsessions-off") } else { t!("ide-dbsessions-every", seconds = seconds) };
            if ui.selectable_label(every == seconds, label).clicked() {
                chosen_interval = Some(seconds);
                ui.close();
            }
        }
    });
    let mut settings = serde_json::Map::new();
    if toggled {
        settings.insert("showIdle".to_owned(), json!(show_idle));
    }
    if let Some(seconds) = chosen_interval {
        settings.insert("autoRefresh".to_owned(), json!(seconds));
    }
    if !settings.is_empty() {
        settings.insert("connection".to_owned(), json!(state.connection));
        match registry.dispatch(session, "db.session.open", Value::Object(settings)) {
            Ok(_) if toggled && connected => state.read = None,
            Ok(_) => {}
            Err(error) => state.error = Some(message(error)),
        }
    }
    let selected = state
        .selected
        .as_ref()
        .and_then(|id| state.sessions().iter().find(|session| session["id"] == id.as_str()))
        .cloned();
    let can_end = connected && selected.is_some() && state.ending.is_none();
    let mut at = at + 8.0;
    for (end, glyph, color) in
        [(End::Cancel, glyph_stop as Glyph, p.warning_text), (End::Terminate, glyph_close, p.error_text)]
    {
        let (clicked, next) = tool(ui, p, at, (glyph, color), (can_end, compact), &end.label(mysql));
        at = next;
        if clicked && let Some(chosen) = &selected {
            actions.push((end, text_of(chosen, "id"), chosen["own"] == true));
        }
    }
    let listed = state.answer.as_ref().map(|answer| &answer["sessions"]);
    let total = listed.and_then(|listed| listed["total"].as_u64()).unwrap_or(0);
    let shown = state.sessions().len();
    let waiting = state
        .sessions()
        .iter()
        .filter(|session| session["blockedBy"].as_array().is_some_and(|ids| !ids.is_empty()))
        .count();
    let mut count = t!("ide-dbsessions-count", shown = shown, total = total);
    if waiting > 0 {
        count = format!("{count} · {}", t!("ide-dbsessions-waiting", count = waiting));
    }
    let color = if waiting > 0 { p.warning_text } else { p.text_secondary };
    let right = bar.right() - 12.0;
    if right - at > 80.0 {
        let job = vcs::truncated(&count, theme::regular(12.0), color, right - at - 8.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(right - galley.size().x, bar.center().y - galley.size().y / 2.0), galley, color);
    }
}

fn question_strip(
    state: &mut SessionsTab,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    strip: Rect,
) {
    ui.painter().rect_filled(strip, 0, p.banner);
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("database-sessions-question")
            .max_rect(strip.shrink2(vec2(12.0, 6.0)))
            .layout(Layout::top_down(Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 3.0;
    let mut answer = None;
    if let Some(question) = state.question.as_mut() {
        child.label(egui::RichText::new(&question.message).font(theme::semibold(13.0)).color(p.text));
        let mut line = question.statement.clone();
        let detail = if question.dangerous && question.risks.len() < 2 { None } else { question.risks.last() };
        if let Some(risk) = detail {
            line = format!("{line}  —  {risk}");
        }
        let job = vcs::truncated(&line, FontId::monospace(12.0), p.text_secondary, strip.width() - 24.0);
        child.label(job);
        child.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if question.dangerous {
                let risk = question.risks.first().cloned().unwrap_or_default();
                let understood = t!("ide-dbsessions-understood", risk = risk.as_str());
                ui.checkbox(
                    &mut question.understood,
                    egui::RichText::new(understood).font(theme::regular(12.5)).color(p.text),
                );
            }
            let enabled = !question.dangerous || question.understood;
            let button =
                egui::Button::new(egui::RichText::new(&question.label).font(theme::regular(13.0)).color(p.accent_text))
                    .min_size(vec2(84.0, 26.0))
                    .corner_radius(6)
                    .fill(if question.dangerous { p.error_text } else { p.accent });
            if ui.add_enabled(enabled, button).clicked() {
                answer = Some(true);
            }
            if vcs::secondary_button(ui, p, &t!("ide-dbsessions-cancel")).clicked() {
                answer = Some(false);
            }
        });
    }
    match answer {
        Some(true) => state.confirm(registry, session),
        Some(false) => state.question = None,
        None => {}
    }
}

fn not_connected(
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (body, name, state, id): (Rect, &str, &str, &str),
) {
    let block = Rect::from_center_size(pos2(body.center().x, body.top() + 70.0), vec2(body.width().min(520.0), 80.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(block).layout(Layout::top_down(Align::Center)));
    let text = if state == "connecting" {
        t!("ide-dbsessions-connecting", name = name)
    } else {
        t!("ide-dbsessions-not-connected", name = name)
    };
    child.label(egui::RichText::new(text).font(theme::regular(13.0)).color(p.text_secondary));
    child.add_space(8.0);
    if state != "connecting" && vcs::secondary_button(&mut child, p, &t!("ide-dbsessions-connect")).clicked() {
        let _ = registry.dispatch(session, "db.connection.connect", json!({ "connection": id }));
    }
}

fn draw_table(
    state: &mut SessionsTab,
    ui: &mut Ui,
    p: &IdePalette,
    (rect, mysql): (Rect, bool),
    actions: &mut Vec<(End, String, bool)>,
) {
    let columns = columns(mysql);
    let fixed: f32 = columns.iter().map(|column| column.width).sum();
    let flexible = (rect.width() - 16.0 - fixed - 8.0).max(120.0);
    let widths: Vec<f32> =
        columns.iter().map(|column| if column.width == 0.0 { flexible } else { column.width }).collect();
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEAD));
    ui.painter().rect_filled(head, 0, p.island);
    ui.painter().hline(head.x_range(), head.bottom() - 0.5, Stroke::new(1.0, p.frame));
    let font = theme::semibold(12.0);
    let mut x = head.left() + 16.0;
    for (column, width) in columns.iter().zip(&widths) {
        let job = vcs::truncated(&column.title, font.clone(), p.text_secondary, width - 8.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(x + 4.0, head.center().y - galley.size().y / 2.0), galley, p.text_secondary);
        x += width;
    }
    let list = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
    let sessions = state.sessions().to_vec();
    if sessions.is_empty() {
        let text = if state.listing.is_some() || state.answer.is_none() {
            t!("ide-dbsessions-loading")
        } else {
            t!("ide-dbsessions-empty")
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
    let mut child = ui.new_child(UiBuilder::new().id_salt("database-sessions-list").max_rect(list));
    child.spacing_mut().item_spacing.y = 0.0;
    let mut chosen = None;
    let cell_font = theme::regular(12.5);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(&mut child, ROW, sessions.len(), |ui, range| {
        for index in range {
            let session = &sessions[index];
            let id = text_of(session, "id");
            let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            if state.selected.as_deref() == Some(id.as_str()) {
                ui.painter().rect_filled(row, 0, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(row, 0, p.hover);
            }
            if let Some(color) = mark_color(p, session) {
                ui.painter().circle_filled(pos2(row.left() + 8.0, row.center().y), 3.5, color);
            }
            let mut x = row.left() + 16.0;
            for (at, (column, width)) in columns.iter().zip(&widths).enumerate() {
                let text = (column.cell)(session);
                let monospace = column.width == 0.0;
                let color = if at == 0 && session["own"] == true { p.link } else { p.text };
                let font = if monospace { FontId::monospace(12.0) } else { cell_font.clone() };
                let job = vcs::truncated(&text, font, color, width - 8.0);
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                ui.painter().galley(pos2(x + 4.0, row.center().y - galley.size().y / 2.0), galley, color);
                x += width;
            }
            accessibility::name_with(&response, egui::Role::ListItem, || {
                format!(
                    "{id} {} {} {}",
                    text_of(session, "user"),
                    text_of(session, "state"),
                    one_line(&text_of(session, "query"))
                )
            });
            if response.clicked() || response.secondary_clicked() {
                chosen = Some(id.clone());
            }
            response.context_menu(|ui| {
                customization::popup(
                    ui,
                    customization::DATABASE_SESSIONS_POPUP,
                    |_| true,
                    |ui, slot| {
                        if slot == customization::SESSION_ENDS {
                            for end in [End::Cancel, End::Terminate] {
                                if ui.button(format!("{}…", end.label(mysql))).clicked() {
                                    actions.push((end, id.clone(), session["own"] == true));
                                    ui.close();
                                }
                            }
                        } else if ui.button(t!("ide-dbsessions-copy-query")).clicked() {
                            ui.ctx().copy_text(text_of(session, "query"));
                            ui.close();
                        }
                    },
                );
            });
        }
    });
    if let Some(id) = chosen {
        state.selected = Some(id);
    }
}

fn section_title(ui: &Ui, p: &IdePalette, at: egui::Pos2, text: &str) {
    ui.painter().text(at, Align2::LEFT_TOP, text, theme::semibold(12.5), p.text);
}

fn draw_detail(state: &SessionsTab, ui: &mut Ui, p: &IdePalette, rect: Rect) {
    ui.painter().rect_filled(rect, 0, p.island);
    ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.frame));
    let half = rect.left() + rect.width() * 0.5;
    ui.painter().vline(half, rect.y_range(), Stroke::new(1.0, p.frame));
    let left = Rect::from_min_max(rect.min + vec2(12.0, 8.0), pos2(half - 12.0, rect.bottom() - 6.0));
    let right = Rect::from_min_max(pos2(half + 12.0, rect.top() + 8.0), rect.max - vec2(12.0, 6.0));
    let selected =
        state.selected.as_ref().and_then(|id| state.sessions().iter().find(|session| session["id"] == id.as_str()));
    let mut child = ui.new_child(
        UiBuilder::new().id_salt("database-sessions-detail").max_rect(left).layout(Layout::top_down(Align::Min)),
    );
    match selected {
        None => {
            child.label(
                egui::RichText::new(t!("ide-dbsessions-select")).font(theme::regular(12.5)).color(p.text_secondary),
            );
        }
        Some(session) => {
            let mut facts = vec![text_of(session, "id")];
            for field in ["user", "host", "database", "application", "state", "wait"] {
                let value = text_of(session, field);
                if !value.is_empty() {
                    facts.push(value);
                }
            }
            if let Some(seconds) = session["seconds"].as_u64() {
                facts.push(t!("ide-dbsessions-seconds", seconds = seconds));
            }
            let job = vcs::truncated(&facts.join(" · "), theme::regular(12.0), p.text_secondary, left.width());
            child.label(job);
            let waits = ids(&session["blockedBy"]);
            if !waits.is_empty() {
                child.label(
                    egui::RichText::new(t!("ide-dbsessions-blocked-by", ids = waits.as_str()))
                        .font(theme::regular(12.0))
                        .color(p.warning_text),
                );
            }
            let blocks = ids(&session["blocking"]);
            if !blocks.is_empty() {
                child.label(
                    egui::RichText::new(t!("ide-dbsessions-blocking", ids = blocks.as_str()))
                        .font(theme::regular(12.0))
                        .color(p.error_text),
                );
            }
            child.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("database-sessions-query").auto_shrink([false, false]).show(
                &mut child,
                |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text_of(session, "query")).font(FontId::monospace(12.5)).color(p.text),
                        )
                        .wrap()
                        .selectable(true),
                    );
                },
            );
        }
    }
    draw_locks(state, ui, p, right);
}

fn draw_locks(state: &SessionsTab, ui: &mut Ui, p: &IdePalette, rect: Rect) {
    let listed = state.answer.as_ref().map(|answer| answer["sessions"].clone()).unwrap_or_default();
    let locks = listed["locks"].as_array().cloned().unwrap_or_default();
    let title = format!("{} ({})", t!("ide-dbsessions-locks"), locks.len());
    section_title(ui, p, rect.min, &title);
    let mut y = rect.top() + 22.0;
    if let Some(error) = listed["locksError"].as_str() {
        let text = t!("ide-dbsessions-locks-error", error = error);
        let job = vcs::truncated(&text, theme::regular(12.0), p.warning_text, rect.width());
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(rect.left(), y), galley, p.warning_text);
        return;
    }
    for chain in listed["chains"].as_array().into_iter().flatten() {
        let line = chain.as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(" ← ");
        let text = format!("{}: {line}", t!("ide-dbsessions-chain"));
        let job = vcs::truncated(&text, theme::regular(12.0), p.warning_text, rect.width());
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(rect.left(), y), galley, p.warning_text);
        y += 18.0;
    }
    if locks.is_empty() {
        ui.painter().text(
            pos2(rect.left(), y),
            Align2::LEFT_TOP,
            t!("ide-dbsessions-no-locks"),
            theme::regular(12.0),
            p.text_secondary,
        );
        return;
    }
    let widths = [0.14, 0.18, 0.24, 0.12, 0.32];
    let headers = [
        t!("ide-dbsessions-col-pid"),
        t!("ide-dbsessions-lock-kind"),
        t!("ide-dbsessions-lock-mode"),
        t!("ide-dbsessions-lock-granted"),
        t!("ide-dbsessions-lock-object"),
    ];
    let row = |ui: &Ui, y: f32, cells: &[String], color: Color32, font: FontId| {
        let mut x = rect.left();
        for (cell, share) in cells.iter().zip(widths) {
            let width = rect.width() * share;
            let job = vcs::truncated(cell, font.clone(), color, width - 6.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(x, y), galley, color);
            x += width;
        }
    };
    row(ui, y + 4.0, &headers, p.text_secondary, theme::semibold(12.0));
    y += 22.0;
    for lock in locks {
        if y + 18.0 > rect.bottom() {
            break;
        }
        let granted = lock["granted"] == true;
        let mut object = text_of(&lock, "object");
        let waits = ids(&lock["blockedBy"]);
        if !waits.is_empty() {
            object = format!("{object} · {}", t!("ide-dbsessions-blocked-by", ids = waits.as_str()));
        }
        let cells = [
            text_of(&lock, "session"),
            text_of(&lock, "kind"),
            text_of(&lock, "mode"),
            if granted { t!("ide-dbsessions-lock-held") } else { t!("ide-dbsessions-lock-waiting") },
            object,
        ];
        let color = if granted { p.text } else { p.warning_text };
        row(ui, y, &cells, color, theme::regular(12.0));
        y += 18.0;
    }
}

#[cfg(test)]
mod tests;
