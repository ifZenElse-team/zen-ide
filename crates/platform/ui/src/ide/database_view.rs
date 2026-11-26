mod clipboard;
mod code;
mod compare;
mod data_edit;
mod diagram;
mod find;
mod geometry_canvas;
mod history;
mod key_view;
mod migrations;
mod object;
mod plan;
mod recovery;
mod relations;
mod results;
mod sessions;
mod structure_edit;
mod value_panel;
mod variables;

pub(super) use diagram::ThemeRenderer;

pub(crate) use value_panel::SelectionFigures;

pub(super) use results::{glyph_run_script, glyph_run_several};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Align, Align2, Color32, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::database::{NewConsole, OVERVIEW};
use zen_workbench::workspaces::InnerTab;

use super::database_navigator::{NavigatorPanel, type_color};
use super::glyphs::{Glyph, glyph_check, glyph_database, glyph_lock, glyph_plus, glyph_run, glyph_stop, glyph_undo};
use super::transaction_dialog::{self, Dialog};
use super::workspace_host::{TextExtras, WorkspaceView};
use super::{IdePalette, Side, hint, theme, vcs};
use crate::accessibility;

pub(super) fn glyph_history(painter: &egui::Painter, rect: Rect, color: Color32) {
    history::glyph_history(painter, rect, color);
}

const TOOLBAR: f32 = 32.0;

const OVERVIEW_WIDTH: f32 = 440.0;

const LIST_WIDTH: f32 = 640.0;

const ROW: f32 = 32.0;

const REFRESH: Duration = Duration::from_secs(1);

const BUTTON: f32 = 30.0;

const GAP: f32 = 8.0;

const MIN_TEXT: f32 = 140.0;

struct RowLayout {
    status: f32,
    edit: f32,
    toggle: f32,
}

impl RowLayout {
    fn of(ui: &Ui, p: &IdePalette) -> Self {
        let measure =
            |text: String, size: f32| ui.painter().layout_no_wrap(text, theme::regular(size), p.text).size().x;
        let button = |labels: &[String]| {
            labels
                .iter()
                .map(|label| measure(label.clone(), 13.0) + 2.0 * vcs::BUTTON_PADDING)
                .fold(vcs::BUTTON_WIDTH, f32::max)
        };
        let states = [
            ("connected", None),
            ("connecting", None),
            ("connecting", Some("keychain")),
            ("closed", None),
            ("failed", None),
            ("disconnected", None),
        ];
        Self {
            status: states
                .iter()
                .map(|(state, waiting)| measure(state_label(p, state, *waiting).0, 12.0))
                .fold(0.0, f32::max)
                .ceil(),
            edit: button(&[t!("ide-database-edit")]),
            toggle: button(&[
                t!("ide-database-connect"),
                t!("ide-database-disconnect"),
                t!("ide-database-cancel-connect"),
            ]),
        }
    }

    fn buttons(&self) -> f32 {
        self.edit + GAP + self.toggle
    }

    fn text_room(&self, width: f32) -> f32 {
        width - 18.0 - self.buttons() - 12.0 - self.status - 12.0
    }
}

fn button_slot(rect: Rect) -> UiBuilder {
    UiBuilder::new().max_rect(rect).layout(Layout::centered_and_justified(egui::Direction::TopDown))
}

fn paint_cut(ui: &Ui, (text, font, color): (&str, egui::FontId, Color32), at: egui::Pos2, width: f32) {
    if width <= 4.0 {
        return;
    }
    let job = vcs::truncated(text, font, color, width);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(pos2(at.x, at.y - galley.size().y / 2.0), galley, color);
}

#[derive(Default)]
struct Listed {
    read: Option<Instant>,
    connections: Vec<Value>,
    colors: Vec<(String, String)>,
    detections: Vec<Value>,
    scanning: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Connection,
    Schema,
    Mode,
}

#[derive(Default)]
pub(super) struct DatabaseView {
    navigator: NavigatorPanel,
    error: Option<String>,
    listed: Listed,
    pending: Option<u64>,
    console: Option<(PathBuf, u64, Value)>,
    colors: HashMap<PathBuf, String>,
    open_menu: Option<Menu>,
    results: results::Results,
    object: object::ObjectView,
    structure: structure_edit::StructureEdit,
    drivers: Option<Vec<Value>>,
    marks: HashMap<PathBuf, usize>,
    keys: key_view::KeyViews,
    history: history::HistoryView,
    plan: plan::PlanView,
    sessions: sessions::SessionsViews,
    migrations: migrations::MigrationsView,
    recovery: recovery::RecoveryView,
    find: find::SearchView,
    compare: compare::CompareView,
    diagram: diagram::DiagramView,
    toolbar_need: f32,
    want_console: bool,
    context: Option<(Option<String>, u64, Value)>,
    flashes: HashMap<PathBuf, (u64, f64)>,
}

const TOOL: f32 = 28.0;

fn message(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.to_string(),
        other => other.to_string(),
    }
}

fn open_dialog(session: &mut Session, params: Value) {
    session.state_mut::<super::connection_dialog::ConnectionDialogRequest>().0 = Some(params);
}

fn toolbar_button(
    ui: &mut Ui,
    p: &IdePalette,
    left: f32,
    (label, tip, key): (String, Option<String>, Option<String>),
    (salt, compact): (&str, bool),
) -> (bool, f32) {
    let bar = ui.max_rect();
    let font = theme::regular(13.0);
    let width = if compact { TOOL - 2.0 } else { labelled_width(ui, p, &label) };
    let size = vec2(width, TOOLBAR - 8.0);
    let button = Rect::from_min_size(pos2(left, bar.center().y - size.y / 2.0), size);
    let response = ui.interact(button, ui.scope_id().with(salt), Sense::click());
    hint(ui, &response, &label, tip.as_deref().or(key.as_deref()), Side::Below);
    if response.hovered() {
        ui.painter().rect_filled(button, 4, p.hover);
    }
    let center = if compact { button.center().x } else { button.left() + 15.0 };
    let plus = Rect::from_center_size(pos2(center, button.center().y), vec2(12.0, 12.0));
    glyph_plus(ui.painter(), plus, p.text_secondary);
    if !compact {
        ui.painter().text(pos2(button.left() + 27.0, button.center().y), Align2::LEFT_CENTER, label, font, p.text);
    }
    (response.clicked(), size.x)
}

pub(super) fn more_button(
    ui: &mut Ui,
    p: &IdePalette,
    rect: Rect,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> egui::Response {
    let name = t!("ide-database-more-actions");
    let response = ui.interact(rect, ui.scope_id().with(("database-more", salt)), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let glyph = Rect::from_center_size(rect.center(), vec2(14.0, 14.0));
    super::glyphs::glyph_more_horizontal(ui.painter(), glyph, p.text_secondary);
    accessibility::name_with(&response, egui::Role::Button, || name.clone());
    response.on_hover_text(name)
}

fn labelled_width(ui: &Ui, p: &IdePalette, label: &str) -> f32 {
    ui.painter().layout_no_wrap(label.to_owned(), theme::regular(13.0), p.text).size().x + 36.0
}

fn is_console(tab: &InnerTab) -> bool {
    tab.path.as_ref().is_some_and(|path| {
        path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
            || zen_workbench::database::injected::hosts(path)
    })
}

fn active_console(session: &Session) -> Option<PathBuf> {
    let workspaces = session.state::<zen_workbench::workspaces::Workspaces>()?;
    let tab = workspaces.active_tab(zen_workbench::database::WORKSPACE)?;
    is_console(tab).then(|| tab.path.clone()).flatten()
}

impl DatabaseView {
    fn new_console(&mut self, registry: &Registry, session: &mut Session) {
        match registry.dispatch(session, <NewConsole as zen_core::Command>::ID, json!({})) {
            Ok(_) => self.error = None,
            Err(zen_core::DispatchError::Failed { error, .. })
                if error.code.as_deref() == Some(zen_workbench::database::NO_CONNECTION) =>
            {
                self.error = None;
                open_dialog(session, json!({}));
            }
            Err(error) => self.error = Some(message(error)),
        }
    }

    fn refresh(&mut self, registry: &Registry, session: &mut Session) {
        if self.listed.read.is_some_and(|read| read.elapsed() < REFRESH) {
            return;
        }
        let listed = registry.dispatch(session, "db.dataSource.list", json!({ "limit": 200 })).unwrap_or_default();
        let types = registry.dispatch(session, "db.connectionType.list", json!({})).unwrap_or_default();
        let detected = registry.dispatch(session, "db.detect.list", json!({ "limit": 20 })).unwrap_or_default();
        self.listed = Listed {
            detections: detected["detections"].as_array().cloned().unwrap_or_default(),
            scanning: detected["scanning"] == true,
            read: Some(Instant::now()),
            connections: listed["connections"].as_array().cloned().unwrap_or_default(),
            colors: types["types"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|kind| Some((kind["id"].as_str()?.to_owned(), kind["color"].as_str()?.to_owned())))
                .collect(),
        };
        if let Some(operation) = self.pending
            && let Ok(state) = registry.dispatch(session, "db.operation.state", json!({ "operation": operation }))
            && state["status"] != "running"
        {
            self.pending = None;
            self.error = state["message"].as_str().map(str::to_owned);
        }
    }

    fn console_state(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session, path: &Path) -> Value {
        let pass = ctx.cumulative_pass_nr();
        if let Some((known, at, state)) = &self.console
            && known == path
            && *at == pass
        {
            return state.clone();
        }
        let state = registry.dispatch(session, "db.console.state", json!({ "path": path })).unwrap_or_default();
        if let Some(color) = state["color"].as_str() {
            self.colors.insert(path.to_owned(), color.to_owned());
        } else {
            self.colors.remove(path);
        }
        let open = state["transaction"]["inTransaction"] == true;
        match state["transaction"]["statements"].as_u64() {
            Some(count) if open => {
                self.marks.insert(path.to_owned(), usize::try_from(count).unwrap_or(usize::MAX));
            }
            _ => {
                self.marks.remove(path);
            }
        }
        if open {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if state["running"] == true || state["last"]["status"] == "running" {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.console = Some((path.to_owned(), pass, state.clone()));
        state
    }

    fn dispatch(&mut self, registry: &Registry, session: &mut Session, command: &str, params: Value) -> Option<Value> {
        match registry.dispatch(session, command, params) {
            Ok(answer) => {
                self.error = None;
                Some(answer)
            }
            Err(error) => {
                if !transaction_dialog::ask(session, &error) {
                    self.error = Some(message(error));
                }
                None
            }
        }
    }

    fn toggle(&mut self, registry: &Registry, session: &mut Session, id: &str, connected: bool) {
        let command = if connected { "db.connection.disconnect" } else { "db.connection.connect" };
        match registry.dispatch(session, command, json!({ "connection": id })) {
            Ok(started) => {
                self.pending = started["operation"].as_u64();
                self.error = None;
            }
            Err(error) if transaction_dialog::ask(session, &error) => {}
            Err(error) => self.error = Some(message(error)),
        }
        self.listed.read = None;
    }

    fn connections(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.island);
        let width = LIST_WIDTH.min(area.width() - 32.0).max(160.0);
        let block = Rect::from_min_size(
            pos2(area.center().x - width / 2.0, area.top() + 24.0),
            vec2(width, area.height() - 24.0),
        );
        let mut child = ui.new_child(
            UiBuilder::new().id_salt("database-connections").max_rect(block).layout(Layout::top_down(Align::Min)),
        );
        let title = t!("ide-database-connections");
        let (new_label, import_label) = (t!("ide-database-new-connection"), t!("ide-database-import"));
        let measure = |text: &str, font| child.painter().layout_no_wrap(text.to_owned(), font, p.text).size().x;
        let buttons = [&new_label, &import_label]
            .map(|label| (measure(label, theme::regular(13.0)) + 2.0 * vcs::BUTTON_PADDING).max(vcs::BUTTON_WIDTH))
            .iter()
            .sum::<f32>()
            + 3.0 * GAP;
        let one_line = measure(&title, theme::semibold(15.0)) + buttons <= width;
        let mut new_connection = false;
        let mut import = false;
        let mut header_buttons = |ui: &mut Ui| {
            new_connection = vcs::secondary_button(ui, p, &new_label).clicked();
            import = vcs::secondary_button(ui, p, &import_label).clicked();
        };
        child.horizontal(|ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(&title).font(theme::semibold(15.0)).color(p.text));
            if one_line {
                ui.with_layout(Layout::right_to_left(Align::Center), &mut header_buttons);
            }
        });
        if !one_line {
            child.add_space(6.0);
            child.horizontal_wrapped(|ui| {
                ui.set_max_width(width);
                header_buttons(ui);
            });
        }
        if new_connection {
            open_dialog(session, json!({}));
        }
        if import {
            super::import_dialog::request(session, None);
        }
        child.add_space(10.0);
        if self.listed.connections.is_empty() {
            child.label(
                egui::RichText::new(t!("ide-database-no-connections"))
                    .font(theme::regular(13.0))
                    .color(p.text_secondary),
            );
        }
        let connections = self.listed.connections.clone();
        let mut toggled = None;
        let mut edited = None;
        let mut cancelled = None;
        let layout = RowLayout::of(&child, p);
        for connection in &connections {
            let id = connection["id"].as_str().unwrap_or_default();
            let name = connection["name"].as_str().unwrap_or_default();
            let address = [connection["host"].as_str(), connection["database"].as_str()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" / ");
            let detail = format!("{} {}", connection["driver"].as_str().unwrap_or_default(), address);
            let state = connection["state"].as_str().unwrap_or("disconnected");
            let (label, state_color) = state_label(p, state, connection["waitingFor"].as_str());
            let two_lines = layout.text_room(width) < MIN_TEXT;
            let height = if two_lines { 2.0 * ROW } else { ROW + 6.0 };
            let (row, _) = child.allocate_exact_size(vec2(width, height), Sense::hover());
            let kind = connection["type"].as_str().unwrap_or_default();
            let color =
                self.listed.colors.iter().find(|(known, _)| known == kind).and_then(|(_, color)| type_color(p, color));
            let first_line = if two_lines { row.top() + ROW / 2.0 } else { row.center().y };
            if let Some(color) = color {
                child.painter().circle_filled(pos2(row.left() + 6.0, first_line), 4.0, color);
            }
            let text_left = row.left() + 18.0;
            let buttons_left = row.right() - layout.buttons();
            let name_room = if two_lines { buttons_left - 12.0 - text_left } else { layout.text_room(width) };
            let name_font = theme::regular(13.0);
            let name_width = child
                .painter()
                .layout_no_wrap(name.to_owned(), name_font.clone(), p.text)
                .size()
                .x
                .min(name_room.max(20.0));
            paint_cut(&child, (name, name_font, p.text), pos2(text_left, first_line), name_width + 1.0);
            let small = theme::regular(12.0);
            if two_lines {
                let second = row.top() + ROW * 1.5;
                paint_cut(&child, (&label, small.clone(), state_color), pos2(text_left, second), layout.status);
                let detail_left = text_left + layout.status + 12.0;
                paint_cut(
                    &child,
                    (&detail, small, p.text_secondary),
                    pos2(detail_left, second),
                    buttons_left - 12.0 - detail_left,
                );
            } else {
                let detail_left = text_left + name_width + 12.0;
                let status_left = buttons_left - 12.0 - layout.status;
                paint_cut(
                    &child,
                    (&detail, small.clone(), p.text_secondary),
                    pos2(detail_left, first_line),
                    status_left - 12.0 - detail_left,
                );
                paint_cut(&child, (&label, small, state_color), pos2(status_left, first_line), layout.status);
            }
            let connected = matches!(state, "connected" | "closed");
            let action = if connected { t!("ide-database-disconnect") } else { t!("ide-database-connect") };
            let toggle = Rect::from_min_size(
                pos2(row.right() - layout.toggle, row.center().y - BUTTON / 2.0),
                vec2(layout.toggle, BUTTON),
            );
            let mut slot = child.new_child(button_slot(toggle));
            if state == "connecting" {
                if vcs::secondary_button_sized(&mut slot, p, &t!("ide-database-cancel-connect"), layout.toggle)
                    .clicked()
                {
                    cancelled = Some(id.to_owned());
                }
            } else if vcs::secondary_button_sized(&mut slot, p, &action, layout.toggle).clicked() {
                toggled = Some((id.to_owned(), connected));
            }
            let edit_rect =
                Rect::from_min_size(pos2(toggle.left() - GAP - layout.edit, toggle.top()), vec2(layout.edit, BUTTON));
            let edit = vcs::secondary_button_sized(
                &mut child.new_child(button_slot(edit_rect)),
                p,
                &t!("ide-database-edit"),
                layout.edit,
            );
            accessibility::name_with(&edit, egui::Role::Button, || format!("{} {name}", t!("ide-database-edit")));
            if edit.clicked() {
                edited = Some(id.to_owned());
            }
        }
        if let Some(id) = edited {
            open_dialog(session, json!({ "id": id }));
        }
        if let Some((id, connected)) = toggled {
            self.toggle(registry, session, &id, connected);
        }
        if let Some(id) = cancelled {
            self.dispatch(registry, session, "db.connection.cancel", json!({ "connection": id }));
            self.listed.read = None;
        }
        self.detected(&mut child, p, registry, session, width);
        if let Some(error) = &self.error {
            child.add_space(8.0);
            child.add(
                egui::Label::new(egui::RichText::new(error).font(theme::regular(12.0)).color(p.error_text)).wrap(),
            );
        }
        ui.advance_cursor_after_rect(area);
    }

    fn detected(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, width: f32) {
        if self.listed.detections.is_empty() {
            return;
        }
        ui.add_space(18.0);
        ui.label(egui::RichText::new(t!("ide-database-detected")).font(theme::semibold(13.0)).color(p.text));
        ui.add_space(4.0);
        let detections = self.listed.detections.clone();
        let mut added = None;
        let mut dismissed = None;
        for offer in &detections {
            let id = offer["id"].as_str().unwrap_or_default();
            let (row, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
            let painter = ui.painter();
            let product = offer["product"].as_str().unwrap_or_default();
            let address = match (offer["host"].as_str(), offer["port"].as_u64()) {
                (Some(host), Some(port)) => format!("{host}:{port}"),
                (Some(host), None) => host.to_owned(),
                _ => String::new(),
            };
            let place = [Some(address.as_str()).filter(|address| !address.is_empty()), offer["database"].as_str()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" / ");
            let file = offer["file"].as_str().unwrap_or_default();
            let line = t!("ide-database-detected-in", file = file, product = product, place = place.as_str());
            let line = if offer["published"] == false {
                format!("{line} · {}", t!("ide-database-detected-not-published"))
            } else {
                line
            };
            let name = offer["name"].as_str().unwrap_or(file);
            let font = theme::regular(13.0);
            let name_width = painter.layout_no_wrap(name.to_owned(), font.clone(), p.text).size().x.min(width * 0.3);
            let job = vcs::truncated(name, font, p.text, name_width + 1.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(row.left() + 18.0, row.center().y - galley.size().y / 2.0), galley, p.text);
            let detail_left = row.left() + 30.0 + name_width;
            let detail_width = (row.right() - 210.0 - detail_left).max(20.0);
            let job = vcs::truncated(&line, theme::regular(12.0), p.text_secondary, detail_width);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(detail_left, row.center().y - galley.size().y / 2.0), galley, p.text_secondary);
            let button = Rect::from_min_size(pos2(row.right() - 196.0, row.top() + 4.0), vec2(196.0, ROW - 8.0));
            let mut inner =
                ui.new_child(UiBuilder::new().max_rect(button).layout(Layout::right_to_left(Align::Center)));
            let dismiss = vcs::secondary_button(&mut inner, p, &t!("ide-database-dismiss"));
            accessibility::name_with(&dismiss, egui::Role::Button, || format!("{} {name}", t!("ide-database-dismiss")));
            if dismiss.clicked() {
                dismissed = Some(id.to_owned());
            }
            let supported = offer["supported"] == true;
            let label = if offer["connection"].is_string() {
                t!("ide-database-detected-update")
            } else {
                t!("ide-database-add")
            };
            let add = inner.add_enabled_ui(supported, |ui| vcs::secondary_button(ui, p, &label)).inner;
            accessibility::name_with(&add, egui::Role::Button, || format!("{label} {name}"));
            if !supported {
                add.on_disabled_hover_text(t!("ide-database-detected-no-driver", product = product));
            } else if add.clicked() {
                added = Some(id.to_owned());
            }
        }
        if let Some(id) = added {
            open_dialog(session, json!({ "detection": id }));
        }
        if let Some(id) = dismissed {
            self.dispatch(registry, session, "db.detect.dismiss", json!({ "id": id }));
            self.listed.read = None;
        }
    }

    fn overview(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        self.refresh(registry, session);
        if self.pending.is_some()
            || self.listed.connections.iter().any(|connection| connection["state"] == "connecting")
        {
            ui.ctx().request_repaint_after(REFRESH);
        }
        if self.listed.scanning {
            ui.ctx().request_repaint_after(REFRESH);
        }
        if !self.listed.connections.is_empty() || !self.listed.detections.is_empty() {
            self.connections(ui, p, registry, session);
            return;
        }
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.island);
        let width = OVERVIEW_WIDTH.min(area.width() - 32.0).max(120.0);
        let block = Rect::from_center_size(pos2(area.center().x, area.center().y - 40.0), vec2(width, 220.0));
        let mut child = ui.new_child(
            UiBuilder::new().id_salt("database-overview").max_rect(block).layout(Layout::top_down(Align::Center)),
        );
        let (icon, _) = child.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
        glyph_database(child.painter(), icon.shrink(2.0), p.text_secondary);
        child.add_space(14.0);
        child.label(egui::RichText::new(t!("ide-database-no-connections")).font(theme::semibold(15.0)).color(p.text));
        child.add_space(6.0);
        child.add(
            egui::Label::new(
                egui::RichText::new(t!("ide-database-overview-hint"))
                    .font(theme::regular(13.0))
                    .color(p.text_secondary),
            )
            .halign(Align::Center)
            .wrap(),
        );
        child.add_space(16.0);
        if vcs::primary_button(&mut child, p, &t!("ide-database-new-connection")).clicked() {
            open_dialog(session, json!({}));
        }
        child.add_space(4.0);
        if vcs::secondary_button(&mut child, p, &t!("ide-database-import")).clicked() {
            super::import_dialog::request(session, None);
        }
        if let Some(error) = &self.error {
            child.add_space(8.0);
            child.add(
                egui::Label::new(egui::RichText::new(error).font(theme::regular(12.0)).color(p.error_text))
                    .halign(Align::Center)
                    .wrap(),
            );
        }
        ui.advance_cursor_after_rect(area);
    }

    pub(super) fn tool_button(
        ui: &mut Ui,
        p: &IdePalette,
        (at, glyph, color): (f32, Glyph, Color32),
        enabled: bool,
        (name, detail): (&str, Option<&str>),
    ) -> (egui::Response, f32) {
        let bar = ui.max_rect();
        let button = Rect::from_min_size(pos2(at, bar.center().y - 12.0), vec2(26.0, 24.0));
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let response = ui.interact(button, ui.scope_id().with(("database-tool", name)), sense);
        if enabled && response.hovered() {
            ui.painter().rect_filled(button, 4, p.hover);
        }
        let shown = if enabled { color } else { p.text_secondary.gamma_multiply(0.5) };
        glyph(ui.painter(), Rect::from_center_size(button.center(), vec2(14.0, 14.0)), shown);
        hint(ui, &response, name, detail, Side::Below);
        if !enabled {
            accessibility::disabled(&response);
        }
        (response, button.right() + 2.0)
    }

    pub(super) fn selector(
        ui: &mut Ui,
        p: &IdePalette,
        at: f32,
        (label, empty): (&str, bool),
        salt: &str,
    ) -> (egui::Response, f32) {
        let bar = ui.max_rect();
        let font = theme::regular(12.5);
        let width = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), p.text).size().x.min(220.0) + 30.0;
        let button = Rect::from_min_size(pos2(at, bar.center().y - 12.0), vec2(width, 24.0));
        let response = ui.interact(button, ui.scope_id().with(("database-selector", salt)), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(button, 4, p.hover);
        }
        let color = if empty { p.text_secondary } else { p.text };
        let job = vcs::truncated(label, font, color, width - 26.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(button.left() + 8.0, button.center().y - galley.size().y / 2.0), galley, color);
        super::data_grid::chevron(ui.painter(), pos2(button.right() - 10.0, button.center().y), p.text_secondary);
        accessibility::button_with(&response, || label.to_owned());
        (response, button.right() + 4.0)
    }

    fn console_toolbar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        (registry, session): (&Registry, &mut Session),
        path: &Path,
        (at, collapsed): (f32, bool),
    ) -> f32 {
        let state = self.console_state(ui.ctx(), registry, session, path);
        let running = state["running"] == true;
        let bar = ui.max_rect();
        ui.painter().vline(at, (bar.top() + 7.0)..=(bar.bottom() - 7.0), egui::Stroke::new(1.0, p.frame));
        let at = at + 6.0;
        let shortcut = |action: &str| crate::keymap::shortcut(action);
        let execute = t!("ide-database-execute");
        let execute_key = shortcut("db.console.execute");
        let (run, at) = Self::tool_button(ui, p, (at, glyph_run, p.run), true, (&execute, execute_key.as_deref()));
        let script = t!("ide-database-execute-script");
        let script_key = shortcut("db.console.executeScript");
        let (all, at) = Self::tool_button(ui, p, (at, glyph_run_script, p.run), true, (&script, script_key.as_deref()));
        let explain = t!("ide-dbplan-explain-plan");
        let explain_key = shortcut("db.console.explain");
        let several = t!("ide-database-execute-several");
        let export = t!("ide-database-export-query");
        let (mut plan_clicked, mut several_clicked, mut export_clicked) = (false, false, false);
        let mut at = at;
        if !collapsed {
            let (plan_button, next) = Self::tool_button(
                ui,
                p,
                (at, plan::glyph_plan, p.text_secondary),
                true,
                (&explain, explain_key.as_deref()),
            );
            let (on_several, next) = Self::tool_button(ui, p, (next, glyph_run_several, p.run), true, (&several, None));
            (plan_clicked, several_clicked, at) = (plan_button.clicked(), on_several.clicked(), next);
        }
        let cancel = t!("ide-database-cancel");
        let cancel_key = shortcut("db.operation.cancel");
        let (stop, next) =
            Self::tool_button(ui, p, (at, glyph_stop, p.error_text), running, (&cancel, cancel_key.as_deref()));
        at = next;
        if collapsed {
            let more = t!("ide-database-more-actions");
            let (button, next) = Self::tool_button(
                ui,
                p,
                (at, super::glyphs::glyph_more_horizontal, p.text_secondary),
                true,
                (&more, None),
            );
            at = next;
            egui::Popup::menu(&button).show(|ui| {
                ui.set_min_width(240.0);
                let item = |ui: &mut Ui, label: &str, key: Option<&str>| {
                    let button = egui::Button::new(label).shortcut_text(key.unwrap_or_default());
                    ui.add(button).clicked()
                };
                plan_clicked |= item(ui, &explain, explain_key.as_deref());
                several_clicked |= item(ui, &several, None);
                export_clicked |= item(ui, &export, None);
            });
        } else {
            let (export_query, next) =
                Self::tool_button(ui, p, (at, super::glyphs::glyph_push, p.text_secondary), true, (&export, None));
            export_clicked = export_query.clicked();
            at = next;
        }
        if run.clicked() {
            self.dispatch(registry, session, "db.console.execute", json!({ "path": path }));
        }
        if all.clicked() {
            self.dispatch(registry, session, "db.console.executeScript", json!({ "path": path }));
        }
        if plan_clicked {
            self.dispatch(registry, session, "db.console.explain", json!({ "path": path }));
        }
        if several_clicked {
            super::several_dialog::open(session, path, state["connection"].as_str().map(str::to_owned));
        }
        if stop.clicked() {
            self.dispatch(registry, session, "db.operation.cancel", json!({ "path": path }));
        }
        if export_clicked {
            super::export_dialog::request(session, json!({ "source": { "kind": "console", "path": path } }));
        }
        ui.painter().vline(at + 4.0, (bar.top() + 7.0)..=(bar.bottom() - 7.0), egui::Stroke::new(1.0, p.frame));
        self.bound_controls(ui, p, (registry, session), (&state, Some(path)), at + 10.0)
    }

    fn context_toolbar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        (registry, session): (&Registry, &mut Session),
        at: f32,
    ) -> f32 {
        let bar = ui.max_rect();
        ui.painter().vline(at, (bar.top() + 7.0)..=(bar.bottom() - 7.0), egui::Stroke::new(1.0, p.frame));
        let from_tab = zen_workbench::database::tab_connection(session);
        let connection = from_tab.clone().or_else(|| zen_workbench::database::selected_connection(session));
        let pass = ui.ctx().cumulative_pass_nr();
        let state = match &self.context {
            Some((known, at, state)) if *known == connection && *at == pass => state.clone(),
            _ => {
                let mut state = connection
                    .as_deref()
                    .and_then(|id| zen_workbench::database::toolbar_state(session, id))
                    .unwrap_or_default();
                if from_tab.is_some()
                    && let Some(schema) = session
                        .state::<zen_workbench::workspaces::Workspaces>()
                        .and_then(|workspaces| workspaces.active_tab(zen_workbench::database::WORKSPACE))
                        .and_then(|tab| tab.data["schema"].as_str().map(str::to_owned))
                {
                    state["schema"] = json!(schema);
                }
                if state["transaction"]["inTransaction"] == true {
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                }
                self.context = Some((connection, pass, state.clone()));
                state
            }
        };
        self.bound_controls(ui, p, (registry, session), (&state, None), at + 6.0)
    }

    fn bound_controls(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        (registry, session): (&Registry, &mut Session),
        (state, path): (&Value, Option<&Path>),
        at: f32,
    ) -> f32 {
        let bar = ui.max_rect();
        let shortcut = |action: &str| crate::keymap::shortcut(action);
        let connection = state["connectionName"].as_str().map(str::to_owned);
        let no_connection = match state["removedConnection"].as_str() {
            Some(name) => t!("ide-database-connection-removed", name = name),
            None => t!("ide-database-no-connection"),
        };
        let fixed = path.is_none();
        let shown = (connection.as_deref().unwrap_or(&no_connection), connection.is_none() || fixed);
        let (chooser, at) = Self::selector(ui, p, at, shown, "connection");
        if let Some(color) = state["color"].as_str().and_then(|color| type_color(p, color)) {
            ui.painter().circle_filled(pos2(chooser.rect.left() + 3.0, chooser.rect.center().y), 2.5, color);
        }
        let schema = state["schema"].as_str().map(str::to_owned);
        let default_schema = t!("ide-database-default-schema");
        let shown = (schema.as_deref().unwrap_or(&default_schema), schema.is_none() || fixed);
        let (schemas, at) = Self::selector(ui, p, at, shown, "schema");
        if fixed {
            let reason = t!("ide-database-selector-fixed");
            hint(ui, &chooser, &reason, None, Side::Below);
            hint(ui, &schemas, &reason, None, Side::Below);
            if matches!(self.open_menu, Some(Menu::Connection | Menu::Schema)) {
                self.open_menu = None;
            }
        }
        if let Some(path) = path {
            let connection_key = shortcut("view.database.chooseConnection");
            hint(ui, &chooser, &t!("ide-database-choose-connection"), connection_key.as_deref(), Side::Below);
            let schema_key = shortcut("view.database.chooseSchema");
            hint(ui, &schemas, &t!("ide-database-choose-schema"), schema_key.as_deref(), Side::Below);
            self.choosers(ui, p, (registry, session), path, (&chooser, &schemas));
        }
        let at = self.safety_toolbar(ui, p, registry, session, (state, at));
        let target = match path {
            Some(path) => json!({ "path": path }),
            None => json!({ "connection": state["connection"] }),
        };
        let end = self.transaction_toolbar(ui, p, registry, session, (&target, state), at);
        if let Some(error) = &self.error {
            let right = bar.right() - 8.0;
            let job = vcs::truncated(error, theme::regular(12.0), p.error_text, (right - end - 16.0).max(40.0));
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                pos2(right - galley.size().x, bar.center().y - galley.size().y / 2.0),
                galley,
                p.error_text,
            );
        }
        end
    }

    fn choosers(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        (registry, session): (&Registry, &mut Session),
        path: &Path,
        (chooser, schemas): (&egui::Response, &egui::Response),
    ) {
        let connection_popup = egui::Popup::default_response_id(chooser);
        let schema_popup = egui::Popup::default_response_id(schemas);
        match self.open_menu {
            Some(Menu::Connection) => {
                self.open_menu = None;
                egui::Popup::open_id(ui.ctx(), connection_popup);
            }
            Some(Menu::Schema) => {
                self.open_menu = None;
                egui::Popup::open_id(ui.ctx(), schema_popup);
            }
            Some(Menu::Mode) | None => {}
        }
        self.refresh(registry, session);
        let connections = self.listed.connections.clone();
        let mut chosen = None;
        egui::Popup::menu(chooser).show(|ui| {
            ui.set_min_width(220.0);
            if connections.is_empty() {
                ui.label(egui::RichText::new(t!("ide-database-no-connections")).color(p.text_secondary));
            }
            for connection in &connections {
                let name = connection["name"].as_str().unwrap_or_default();
                if ui.button(name).clicked() {
                    chosen = connection["id"].as_str().map(str::to_owned);
                    ui.close();
                }
            }
        });
        if let Some(id) = chosen {
            self.dispatch(registry, session, "db.console.chooseConnection", json!({ "path": path, "connection": id }));
        }
        let mut picked: Option<Option<String>> = None;
        let open = egui::Popup::is_id_open(ui.ctx(), schema_popup);
        let listed = if open {
            registry.dispatch(session, "db.console.schemas", json!({ "path": path })).unwrap_or_default()
        } else {
            Value::Null
        };
        egui::Popup::menu(schemas).show(|ui| {
            ui.set_min_width(200.0);
            if ui.button(t!("ide-database-default-schema")).clicked() {
                picked = Some(None);
                ui.close();
            }
            ui.separator();
            match listed["schemas"].as_array() {
                Some(names) => {
                    for name in names.iter().filter_map(Value::as_str) {
                        if ui.button(name).clicked() {
                            picked = Some(Some(name.to_owned()));
                            ui.close();
                        }
                    }
                }
                None if listed["reading"] == true => {
                    ui.label(egui::RichText::new(t!("ide-database-reading")).color(p.text_secondary));
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
                None => {
                    ui.label(egui::RichText::new(t!("ide-database-connect-for-schemas")).color(p.text_secondary));
                }
            }
        });
        if let Some(schema) = picked {
            self.dispatch(registry, session, "db.console.chooseSchema", json!({ "path": path, "schema": schema }));
        }
    }
}

impl DatabaseView {
    fn capabilities(&mut self, registry: &Registry, session: &mut Session, connection: Option<&str>) -> Value {
        if self.drivers.is_none() {
            let listed = registry.dispatch(session, "db.driver.list", json!({})).unwrap_or_default();
            self.drivers = Some(listed["drivers"].as_array().cloned().unwrap_or_default());
        }
        let driver = connection.and_then(|id| {
            self.listed.connections.iter().find(|known| known["id"] == id).and_then(|known| known["driver"].as_str())
        });
        let Some(driver) = driver else { return Value::Null };
        self.drivers
            .iter()
            .flatten()
            .find(|known| known["id"] == driver)
            .map(|known| known["capabilities"].clone())
            .unwrap_or_default()
    }

    fn isolation_levels(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        connection: Option<&str>,
    ) -> Vec<String> {
        self.capabilities(registry, session, connection)["isolationLevels"]
            .as_array()
            .map(|levels| levels.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn safety_toolbar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (state, at): (&Value, f32),
    ) -> f32 {
        let safety = &state["safety"];
        let Some(connection) = safety["connection"].as_str().map(str::to_owned) else { return at };
        let bar = ui.max_rect();
        let mut at = at;
        if let Some(color) = safety["color"].as_str().and_then(|color| type_color(p, color)) {
            let name = safety["typeName"].as_str().unwrap_or_default().to_owned();
            let font = theme::semibold(11.5);
            let width = ui.painter().layout_no_wrap(name.clone(), font.clone(), color).size().x + 16.0;
            let badge = Rect::from_min_size(pos2(at + 2.0, bar.center().y - 9.0), vec2(width, 18.0));
            ui.painter().rect_filled(badge, 9, color.gamma_multiply(0.16));
            ui.painter().rect_stroke(badge, 9, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
            ui.painter().text(badge.center(), Align2::CENTER_CENTER, &name, font, color);
            let response = ui.interact(badge, ui.scope_id().with("connection-type-badge"), Sense::hover());
            let detail =
                if safety["confirmExecute"] == true { t!("ide-dbsafety-type-confirms") } else { String::new() };
            hint(
                ui,
                &response,
                &t!("ide-dbsafety-type", kind = name.as_str()),
                Some(&detail).filter(|detail| !detail.is_empty()).map(String::as_str),
                Side::Below,
            );
            at = badge.right() + 4.0;
        }
        let read_only = safety["readOnly"] == true;
        let (name, color) = if read_only {
            (t!("ide-dbsafety-read-only-on"), p.warning_text)
        } else {
            (t!("ide-dbsafety-read-only-off"), p.text_secondary)
        };
        let (lock, next) = Self::tool_button(ui, p, (at, glyph_lock, color), true, (&name, None));
        if read_only {
            ui.painter().rect_stroke(lock.rect, 4, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
        }
        if lock.clicked() {
            self.dispatch(registry, session, "db.connection.readOnly", json!({ "connection": connection }));
            self.console = None;
            self.listed.read = None;
        }
        next
    }

    fn transaction_toolbar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (target, state): (&Value, &Value),
        at: f32,
    ) -> f32 {
        let transaction = &state["transaction"];
        let unsupported = self.capabilities(registry, session, state["connection"].as_str())["transactions"] == false;
        if unsupported && !transaction.is_null() {
            return at;
        }
        let bar = ui.max_rect();
        ui.painter().vline(at + 2.0, (bar.top() + 7.0)..=(bar.bottom() - 7.0), egui::Stroke::new(1.0, p.frame));
        let mut at = at + 8.0;
        let known = !transaction.is_null();
        let mode = transaction["mode"].as_str().unwrap_or("auto").to_owned();
        if known {
            at = self.mode_control(ui, p, registry, session, (target, state), at);
        }
        let auto = transaction["autoCommit"] == true;
        let open = transaction["inTransaction"] == true;
        let enabled = known && (!auto || open);
        let shortcut = |action: &str| crate::keymap::shortcut(action);
        let commit_name = t!("ide-dbtx-commit");
        let commit_key = shortcut("db.transaction.commit");
        let (commit, at) =
            Self::tool_button(ui, p, (at, glyph_check, p.vcs_added), enabled, (&commit_name, commit_key.as_deref()));
        let rollback_name = t!("ide-dbtx-rollback");
        let rollback_key = shortcut("db.transaction.rollback");
        let (rollback, at) = Self::tool_button(
            ui,
            p,
            (at, glyph_undo, p.vcs_deleted),
            enabled,
            (&rollback_name, rollback_key.as_deref()),
        );
        if commit.clicked() {
            self.dispatch(registry, session, "db.transaction.commit", target.clone());
            self.context = None;
        }
        if rollback.clicked() {
            self.dispatch(registry, session, "db.transaction.rollback", target.clone());
            self.context = None;
        }
        let statements = transaction["statements"].as_u64().unwrap_or(0);
        let (text, color) = counter(p, open, statements);
        let font = theme::semibold(12.0);
        let width = ui.painter().layout_no_wrap(text.clone(), font.clone(), color).size().x.max(14.0) + 16.0;
        let rect = Rect::from_min_size(pos2(at + 2.0, bar.center().y - 11.0), vec2(width, 22.0));
        let sense = if known { Sense::click() } else { Sense::hover() };
        let response = ui.interact(rect, ui.scope_id().with("transaction-counter"), sense);
        if known && response.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        ui.painter().rect_stroke(rect, 4, egui::Stroke::new(1.0, color.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, font, color);
        let detail = if known { counter_detail(transaction, &mode) } else { t!("ide-dbtx-counter-no-connection") };
        accessibility::name_with(&response, egui::Role::Button, || t!("ide-dbtx-log-open"));
        response.clone().on_hover_ui(|ui| {
            ui.set_max_width(420.0);
            ui.label(egui::RichText::new(detail).font(theme::regular(12.0)).color(p.text));
        });
        if known && response.clicked() {
            transaction_dialog::request(session, Dialog::Log(target.clone()));
        }
        rect.right() + 4.0
    }

    fn mode_control(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (target, state): (&Value, &Value),
        at: f32,
    ) -> f32 {
        let transaction = &state["transaction"];
        let mode = transaction["mode"].as_str().unwrap_or("auto").to_owned();
        let label = transaction_dialog::mode_label(&mode);
        let (chooser, at) = Self::selector(ui, p, at, (&label, false), "transaction-mode");
        hint(ui, &chooser, &t!("ide-dbtx-mode-hint"), Some(&mode_hint(&mode)), Side::Below);
        let popup = egui::Popup::default_response_id(&chooser);
        if self.open_menu == Some(Menu::Mode) {
            self.open_menu = None;
            egui::Popup::open_id(ui.ctx(), popup);
        }
        let levels = if egui::Popup::is_id_open(ui.ctx(), popup) {
            self.isolation_levels(registry, session, state["connection"].as_str())
        } else {
            Vec::new()
        };
        let current_isolation = transaction["isolation"].as_str().map(str::to_owned);
        let mut change: Option<Value> = None;
        let mut pending = false;
        let with = |extra: Value| {
            let mut params = target.clone();
            if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
                params.extend(extra.clone());
            }
            params
        };
        egui::Popup::menu(&chooser).show(|ui| {
            ui.set_min_width(220.0);
            for (id, name) in [
                ("auto", t!("ide-dbtx-mode-auto")),
                ("manual", t!("ide-dbtx-mode-manual")),
                ("smart", t!("ide-dbtx-mode-smart")),
            ] {
                if ui.selectable_label(mode == id, name).on_hover_text(mode_hint(id)).clicked() {
                    change = Some(with(json!({ "mode": id })));
                    ui.close();
                }
            }
            ui.separator();
            ui.label(egui::RichText::new(t!("ide-dbtx-isolation")).color(p.text_secondary));
            if ui.selectable_label(current_isolation.is_none(), t!("ide-dbtx-isolation-default")).clicked() {
                change = Some(with(json!({ "isolation": "" })));
                ui.close();
            }
            for level in &levels {
                let chosen = current_isolation.as_deref().is_some_and(|known| known.eq_ignore_ascii_case(level));
                if ui.selectable_label(chosen, level).clicked() {
                    change = Some(with(json!({ "isolation": level })));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(t!("ide-dbtx-pending-open")).clicked() {
                pending = true;
                ui.close();
            }
        });
        if let Some(params) = change {
            self.dispatch(registry, session, "db.transaction.mode", params);
            self.context = None;
        }
        if pending {
            transaction_dialog::request(session, Dialog::Pending);
        }
        at
    }
}

fn mode_hint(mode: &str) -> String {
    match mode {
        "manual" => t!("ide-dbtx-mode-manual-hint"),
        "smart" => t!("ide-dbtx-mode-smart-hint"),
        _ => t!("ide-dbtx-mode-auto-hint"),
    }
}

fn counter(p: &IdePalette, open: bool, statements: u64) -> (String, Color32) {
    match (open, statements) {
        (false, _) => ("0".to_owned(), p.text_secondary),
        (true, 0..5) => (statements.to_string(), p.vcs_added),
        (true, 5..20) => (statements.to_string(), p.warning_text),
        (true, _) => (statements.to_string(), p.error_text),
    }
}

fn counter_detail(transaction: &Value, mode: &str) -> String {
    if transaction["inTransaction"] != true {
        return format!("{}\n{}", t!("ide-dbtx-counter-idle"), mode_hint(mode));
    }
    let mut lines = vec![t!(
        "ide-dbtx-counter-open",
        statements = transaction["statements"].as_u64().unwrap_or(0),
        modifying = transaction["modifying"].as_u64().unwrap_or(0),
        age = transaction_dialog::age(transaction["ageSeconds"].as_u64().unwrap_or(0))
    )];
    let modifying: Vec<&str> = transaction["log"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|statement| statement["modifying"] == true)
        .filter_map(|statement| statement["summary"].as_str())
        .collect();
    lines.extend(modifying.iter().map(|summary| format!("• {summary}")));
    lines.push(t!("ide-dbtx-counter-click"));
    lines.join("\n")
}

fn state_label(p: &IdePalette, state: &str, waiting: Option<&str>) -> (String, Color32) {
    match (state, waiting) {
        ("connected", _) => (t!("ide-database-state-connected"), p.vcs_added),
        ("connecting", Some("keychain")) => (t!("ide-database-state-waiting-keychain"), p.warning_text),
        ("connecting", Some("tunnel")) => (t!("ide-database-state-waiting-tunnel"), p.text_secondary),
        ("connecting", Some("network")) => (t!("ide-database-state-waiting-network"), p.text_secondary),
        ("connecting", _) => (t!("ide-database-state-connecting"), p.text_secondary),
        ("closed", _) => (t!("ide-database-state-closed"), p.text_secondary),
        ("failed", _) => (t!("ide-database-state-failed"), p.error_text),
        _ => (t!("ide-database-state-disconnected"), p.text_secondary),
    }
}

const FLASH: f64 = 0.8;

impl DatabaseView {
    fn flash(&mut self, ctx: &egui::Context, path: &Path, state: &Value) -> Option<(usize, usize, f32)> {
        let now = ctx.input(|input| input.time);
        let query = state["last"]["query"].as_u64()?;
        let finished = state["last"]["status"] != "running";
        let seen = self.flashes.entry(path.to_owned()).or_insert((query, f64::NEG_INFINITY));
        if finished && seen.0 != query {
            *seen = (query, now);
        }
        let elapsed = now - seen.1;
        if !(0.0..FLASH).contains(&elapsed) || accessibility::reduce_motion() {
            return None;
        }
        ctx.request_repaint();
        let range = state["ran"].as_array()?;
        let (start, end) = (range.first()?.as_u64()? as usize, range.get(1)?.as_u64()? as usize);
        Some((start, end, (1.0 - elapsed / FLASH) as f32))
    }
}

impl WorkspaceView for DatabaseView {
    fn toolbar_height(&self) -> f32 {
        TOOLBAR
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        self.results.ctx = Some(ui.ctx().clone());
        let open_tabs = |kind: &str| -> Vec<String> {
            session
                .state::<zen_workbench::workspaces::Workspaces>()
                .map(|workspaces| {
                    workspaces
                        .tabs(zen_workbench::database::WORKSPACE)
                        .iter()
                        .filter(|tab| tab.kind == kind)
                        .map(|tab| tab.id.clone())
                        .collect()
                })
                .unwrap_or_default()
        };
        let (open_keys, open_sessions) =
            (open_tabs(zen_workbench::database::keys::KEY_TAB), open_tabs(zen_workbench::database::sessions::KIND));
        self.keys.retain(&open_keys);
        self.sessions.retain(&open_sessions);
        self.results.grid_ids.clear();
        zen_workbench::database::objects::step(session);
        zen_workbench::database::plans::step(session);
        self.refresh(registry, session);
        let left = ui.max_rect().left() + 8.0;
        let room = ui.max_rect().right() - 8.0 - left;
        let console_path = active_console(session);
        let labels = [t!("ide-database-new-connection"), t!("ide-database-new-console")];
        let label_saving: f32 = labels.iter().map(|label| labelled_width(ui, p, label) - (TOOL - 2.0)).sum();
        let overflow_saving = if console_path.is_some() { 2.0 * TOOL } else { 0.0 };
        let need = self.toolbar_need;
        let collapsed = need > room && overflow_saving > 0.0;
        let compact = need - overflow_saving > room;
        let [connection_label, console_label] = labels;
        let connection_key = crate::keymap::shortcut("view.database.newConnection");
        let (connection, width) =
            toolbar_button(ui, p, left, (connection_label, None, connection_key), ("database-new-connection", compact));
        if connection {
            open_dialog(session, json!({}));
        }
        let console_left = left + width + 4.0;
        let tip = self.listed.connections.is_empty().then(|| t!("ide-database-new-console-first"));
        let console_key = crate::keymap::shortcut("view.database.newConsole");
        let (console, console_width) =
            toolbar_button(ui, p, console_left, (console_label, tip, console_key), ("database-new-console", compact));
        if console || std::mem::take(&mut self.want_console) {
            self.new_console(registry, session);
        }
        let history_name = t!("ide-dbhistory-open");
        let at = console_left + console_width + 2.0;
        let (history, at) =
            Self::tool_button(ui, p, (at, history::glyph_history, p.text_secondary), true, (&history_name, None));
        if history.clicked() {
            self.dispatch(registry, session, "db.history.open", json!({}));
        }
        let search_name = t!("ide-dbsearch-open");
        let (search, at) =
            Self::tool_button(ui, p, (at, super::glyphs::glyph_search, p.text_secondary), true, (&search_name, None));
        if search.clicked() {
            self.dispatch(registry, session, "db.search.open", json!({}));
        }
        let compare_name = t!("ide-dbcompare-open");
        let (compare, at) =
            Self::tool_button(ui, p, (at, compare::glyph_compare, p.text_secondary), true, (&compare_name, None));
        if compare.clicked() {
            self.dispatch(registry, session, "db.compare.open", json!({}));
        }
        let end = match console_path {
            Some(path) => self.console_toolbar(ui, p, (registry, session), &path, (at + 4.0, collapsed)),
            None => self.context_toolbar(ui, p, (registry, session), at + 4.0),
        };
        let saved = if collapsed { overflow_saving } else { 0.0 } + if compact { label_saving } else { 0.0 };
        self.toolbar_need = end - left + saved;
    }

    fn left_panel_width(&self) -> f32 {
        self.navigator.width()
    }

    fn left_panel(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        self.navigator.show(ui, p, registry, session);
    }

    fn tab(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, tab: &InnerTab) {
        if tab.kind == OVERVIEW {
            self.overview(ui, p, registry, session);
        } else if tab.kind == zen_workbench::database::keys::KEY_TAB {
            self.keys.show(ui, p, registry, session, tab);
        } else if tab.kind == zen_workbench::database::objects::KIND {
            self.object_tab(ui, p, registry, session, tab);
        } else if tab.kind == zen_workbench::database::history::TAB {
            self.refresh(registry, session);
            self.history.show(ui, p, registry, session, (tab, &self.listed));
        } else if tab.kind == zen_workbench::database::plans::KIND {
            self.plan.show(ui, p, registry, session, tab);
        } else if tab.kind == zen_workbench::database::sessions::KIND {
            self.refresh(registry, session);
            self.sessions.show(ui, p, registry, session, (tab, &self.listed));
        } else if tab.kind == zen_workbench::database::migrations::TAB {
            self.refresh(registry, session);
            self.migrations.show(ui, p, registry, session, (tab, &self.listed));
        } else if tab.kind == zen_workbench::database::recovery::TAB {
            self.recovery.show(ui, p, registry, session);
        } else if tab.kind == zen_workbench::database::find::TAB {
            self.refresh(registry, session);
            self.find.show(ui, p, registry, session, (tab, &self.listed));
        } else if tab.kind == zen_workbench::database::compare::TAB {
            self.refresh(registry, session);
            self.compare.show(ui, p, registry, session, (tab, &self.listed));
        } else if tab.kind == zen_workbench::database::diagrams::KIND {
            self.refresh(registry, session);
            self.diagram_tab(ui, p, registry, session, tab);
        }
    }

    fn tab_color(&self, tab: &InnerTab, palette: &IdePalette) -> Option<Color32> {
        if [
            zen_workbench::database::objects::KIND,
            zen_workbench::database::keys::KEY_TAB,
            zen_workbench::database::plans::KIND,
            zen_workbench::database::sessions::KIND,
            zen_workbench::database::diagrams::KIND,
        ]
        .contains(&tab.kind.as_str())
        {
            let connection = tab.data["connection"].as_str()?;
            let listed = self.listed.connections.iter().find(|known| known["id"] == connection)?;
            let kind = listed["type"].as_str()?;
            let color = self.listed.colors.iter().find(|(known, _)| known == kind).map(|(_, color)| color)?;
            return type_color(palette, color);
        }
        let path = tab.path.as_ref()?;
        self.colors.get(path).and_then(|color| type_color(palette, color))
    }

    fn tab_mark(&self, tab: &InnerTab, palette: &IdePalette) -> Option<(Color32, String)> {
        let path = tab.path.as_ref()?;
        let count = *self.marks.get(path)?;
        let color = if count >= 20 { palette.error_text } else { palette.warning_text };
        Some((color, t!("ide-dbtx-tab-mark", count = count)))
    }

    fn panel_sizes(&self) -> std::collections::BTreeMap<String, f32> {
        let mut sizes = self.results.panel_sizes();
        sizes.extend(self.navigator.sizes());
        sizes
    }

    fn restore_panel_sizes(&mut self, sizes: &std::collections::BTreeMap<String, f32>) {
        self.navigator.restore(sizes);
        self.results.restore_panel_sizes(sizes);
    }

    fn popups(&self) -> Vec<String> {
        let mut open = self.results.popups();
        open.extend(self.structure.popups());
        open
    }

    fn text_panel_height(&mut self, tab: &InnerTab, registry: &Registry, session: &mut Session, height: f32) -> f32 {
        let Some(path) = tab.path.as_ref().filter(|_| is_console(tab)) else { return 0.0 };
        let Some(ctx) = self.results.ctx.clone() else { return 0.0 };
        let state = self.console_state(&ctx, registry, session, path);
        self.results.height_for(&state, height)
    }

    fn text_panel(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, tab: &InnerTab) {
        let Some(path) = tab.path.clone() else { return };
        let state = self.console_state(ui.ctx(), registry, session, &path);
        results::panel(self, ui, p, registry, session, (&path, &state));
    }

    fn text_extras(&mut self, tab: &InnerTab, registry: &Registry, session: &mut Session) -> Option<TextExtras> {
        let path = tab.path.as_ref().filter(|_| is_console(tab))?;
        let ctx = self.results.ctx.clone()?;
        let state = self.console_state(&ctx, registry, session, path);
        let hints = state["hints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|hint| {
                Some((
                    hint["offset"].as_u64()? as usize,
                    hint["label"].as_str()?.to_owned(),
                    hint["tooltip"].as_str().map(str::to_owned),
                    hint["error"] == true,
                ))
            })
            .collect();
        let band = state["statement"]
            .as_array()
            .and_then(|range| Some((range.first()?.as_u64()? as usize, range.get(1)?.as_u64()? as usize)));
        let frame = (state["safety"]["confirmExecute"] == true)
            .then(|| state["safety"]["color"].as_str().and_then(|color| type_color(&IdePalette::of(&ctx), color)))
            .flatten();
        let flash = self.flash(&ctx, path, &state);
        Some(TextExtras { hints, band, frame, flash })
    }

    fn action(&mut self, action: &str) -> bool {
        match action {
            "view.database.chooseConnection" => self.open_menu = Some(Menu::Connection),
            "view.database.chooseSchema" => self.open_menu = Some(Menu::Schema),
            "view.database.focusFilter" => self.results.data.focus_filter(),
            "view.database.copyAdvanced" => self.results.clip.want_copy(),
            "view.database.pasteAdvanced" => self.results.clip.want_paste(),
            "view.database.transactionMode" => self.open_menu = Some(Menu::Mode),
            "view.database.newConsole" => self.want_console = true,
            _ => return false,
        }
        true
    }

    fn takes_key(&self, focused: Option<egui::Id>, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        self.results.takes_key(focused, key, modifiers)
    }

    fn tab_dirty(&self, tab: &InnerTab, session: &Session) -> bool {
        if tab.kind == zen_workbench::database::objects::KIND
            && session
                .state::<zen_workbench::database::alter::Alterations>()
                .is_some_and(|alterations| alterations.staged(&tab.id))
        {
            return true;
        }
        let Some(engine) = session.state::<zen_workbench::database::Database>().and_then(|database| database.started())
        else {
            return false;
        };
        let pending = |id: u64| engine.results().get(id).is_some_and(|result| result.pending().any());
        if tab.kind == zen_workbench::database::objects::KIND {
            return session
                .state::<zen_workbench::database::objects::Objects>()
                .and_then(|objects| objects.shown(&tab.id))
                .is_some_and(pending);
        }
        tab.path.as_ref().is_some_and(|path| {
            session
                .state::<zen_workbench::database::consoles::Consoles>()
                .is_some_and(|consoles| consoles.tabs(path).iter().any(|result| pending(result.result)))
        })
    }
}
