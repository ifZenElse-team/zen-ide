use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use egui::{Align2, Color32, CursorIcon, FontId, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::results::ResultSet;
use zen_i18n::t;

use super::super::data_grid::{
    self, GridAction, GridCell, GridColumn, GridFeatures, GridRowMark, GridSource, GridState,
};
use super::super::glyphs::{glyph_check, glyph_close, glyph_pin, glyph_stop};
use super::super::{IdePalette, customization, theme, vcs};
use super::DatabaseView;
use super::value_panel::ValuePanel;
use crate::accessibility;

const SPLITTER: f32 = 5.0;
const HEADER: f32 = 24.0;
const TABS: f32 = 26.0;
const BOTTOM: f32 = 26.0;
const MIN_HEIGHT: f32 = 120.0;
const CELL_CHARS: usize = 300;
const RECORD_CHARS: usize = 2000;

pub(in crate::ide) fn glyph_run_script(painter: &egui::Painter, rect: Rect, color: Color32) {
    let height = rect.height() * 0.8;
    let width = rect.width() * 0.45;
    let top = rect.center().y - height / 2.0;
    for left in [rect.left() + rect.width() * 0.05, rect.left() + rect.width() * 0.5] {
        let points = vec![pos2(left, top), pos2(left + width, top + height / 2.0), pos2(left, top + height)];
        painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    }
}

pub(in crate::ide) fn glyph_run_several(painter: &egui::Painter, rect: Rect, color: Color32) {
    let height = rect.height() * 0.7;
    let top = rect.center().y - height / 2.0;
    let left = rect.left() + rect.width() * 0.02;
    let points = vec![pos2(left, top), pos2(left + rect.width() * 0.42, top + height / 2.0), pos2(left, top + height)];
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    let stroke = Stroke::new((rect.width() / 12.0).max(1.0), color);
    let width = rect.width() * 0.4;
    let box_height = rect.height() * 0.3;
    for (index, y) in [rect.top() + rect.height() * 0.12, rect.top() + rect.height() * 0.56].into_iter().enumerate() {
        let x = rect.right() - width - rect.width() * 0.02 * index as f32;
        let cell = Rect::from_min_size(pos2(x, y), vec2(width, box_height));
        painter.rect_stroke(cell, 2.0, stroke, egui::StrokeKind::Middle);
    }
}

#[derive(Default)]
pub(super) struct Results {
    pub(super) ctx: Option<egui::Context>,
    height: f32,
    grids: HashMap<u64, GridState>,
    pub(super) grid_ids: Vec<egui::Id>,
    renaming: Option<(u64, String)>,
    confirm_all: Option<u64>,
    filter: String,
    fetch_asked: HashMap<u64, u64>,
    go_to: Option<(u64, String, Option<String>)>,
    fetch_size: Option<String>,
    view: Option<String>,
    value: ValuePanel,
    pub(super) data: super::data_edit::DataEdit,
    pub(super) clip: super::clipboard::Clip,
    pub(super) links: super::relations::Links,
    pub(super) variables: super::variables::VariablesEdit,
}

impl Results {
    pub(super) fn panel_sizes(&self) -> BTreeMap<String, f32> {
        let mut sizes = BTreeMap::new();
        if self.height > 0.0 {
            sizes.insert("results".to_owned(), self.height);
        }
        if let Some(width) = self.value.kept_width() {
            sizes.insert("valuePanel".to_owned(), width);
        }
        sizes
    }

    pub(super) fn restore_panel_sizes(&mut self, sizes: &BTreeMap<String, f32>) {
        if let Some(height) = sizes.get("results") {
            self.height = *height;
        }
        if let Some(width) = sizes.get("valuePanel") {
            self.value.restore_width(*width);
        }
    }

    pub(super) fn popups(&self) -> Vec<String> {
        let mut open = Vec::new();
        if self.renaming.is_some() {
            open.push("databaseRenameResult".to_owned());
        }
        if self.confirm_all.is_some() {
            open.push("databaseFetchAll".to_owned());
        }
        if self.go_to.is_some() {
            open.push("databaseGoToRow".to_owned());
        }
        open.extend(self.data.popups());
        open.extend(self.clip.popups());
        open.extend(self.links.popups());
        open
    }

    pub(super) fn height_for(&mut self, state: &Value, height: f32) -> f32 {
        if state["panel"]["shown"] != true {
            return 0.0;
        }
        if state["panel"]["maximized"] == true {
            return height;
        }
        if self.height <= 0.0 {
            self.height = (height * 0.45).max(MIN_HEIGHT);
        }
        self.height.clamp(MIN_HEIGHT.min(height), (height - 60.0).max(MIN_HEIGHT.min(height)))
    }

    pub(super) fn takes_key(&self, focused: Option<egui::Id>, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        let Some(focused) = focused else { return false };
        if !self.grid_ids.contains(&focused) {
            return false;
        }
        use egui::Key;
        (modifiers.command
            && matches!(
                key,
                Key::C | Key::V | Key::A | Key::N | Key::D | Key::S | Key::B | Key::Backspace | Key::Delete
            ))
            || (!modifiers.any() && matches!(key, Key::Enter | Key::Escape | Key::F2 | Key::Tab))
            || (modifiers.ctrl && !modifiers.command && key == Key::G)
            || (modifiers.shift_only() && key == Key::Enter)
            || matches!(
                key,
                Key::ArrowUp
                    | Key::ArrowDown
                    | Key::ArrowLeft
                    | Key::ArrowRight
                    | Key::PageUp
                    | Key::PageDown
                    | Key::Home
                    | Key::End
            )
    }
}

struct ResultSource {
    set: Arc<ResultSet>,
    columns: Vec<GridColumn>,
    rows: u64,
    revision: u64,
    links: Vec<Option<String>>,
    references: Vec<String>,
    refusal: Option<String>,
}

impl ResultSource {
    fn with_states(set: Arc<ResultSet>, filter: &Value, edit: &Value, relations: &Value) -> Self {
        let info = set.info();
        let sorts = filter["sorts"].as_array().cloned().unwrap_or_default();
        let filtered: Vec<u64> =
            filter["filteredColumns"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        let editable = edit["editable"] == true;
        let columns = info
            .columns
            .iter()
            .enumerate()
            .map(|(at, column)| GridColumn {
                name: column.name.clone(),
                type_name: column.type_name.clone(),
                sort: sorts
                    .iter()
                    .position(|sort| sort["column"].as_u64() == Some(at as u64))
                    .map(|order| (sorts[order]["descending"] == true, order + 1)),
                editable: editable && edit["columns"][at]["editable"] == true,
                filtered: filtered.contains(&(at as u64)),
                not_null: edit["columns"][at]["nullable"] == false,
                primary: edit["key"]["kind"] == "primaryKey"
                    && edit["key"]["columns"].as_array().is_some_and(|key| key.contains(&Value::from(at))),
            })
            .collect();
        let state = info.state as u64;
        let rows = set.len() as u64;
        let (links, references) = super::relations::labels(relations, info.columns.len());
        let refusal = if edit["saving"].is_u64() {
            Some(t!("ide-database-edit-saving"))
        } else {
            edit["message"].as_str().filter(|_| !editable).map(str::to_owned)
        };
        Self {
            links,
            references,
            refusal,
            columns,
            rows,
            revision: rows.wrapping_mul(8)
                ^ state
                ^ (info.bytes as u64).rotate_left(40)
                ^ set.revision().rotate_left(20),
            set,
        }
    }
}

impl GridSource for ResultSource {
    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn row_count(&self) -> u64 {
        self.rows
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn cells(&self, rows: Range<u64>, columns: Range<usize>) -> Vec<Vec<GridCell>> {
        let rows = usize::try_from(rows.start).unwrap_or(usize::MAX)..usize::try_from(rows.end).unwrap_or(usize::MAX);
        self.set
            .cells(rows, columns, CELL_CHARS)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| GridCell {
                        text: cell.text,
                        null: cell.null,
                        number: cell.number,
                        boolean: cell.boolean,
                        cut: cell.cut,
                        changed: cell.changed,
                        default: cell.default,
                    })
                    .collect()
            })
            .collect()
    }

    fn marks(&self, rows: Range<u64>) -> Vec<GridRowMark> {
        let rows = usize::try_from(rows.start).unwrap_or(usize::MAX)..usize::try_from(rows.end).unwrap_or(usize::MAX);
        self.set
            .marks(rows)
            .into_iter()
            .map(|mark| match mark {
                zen_db::results::RowMark::Clean => GridRowMark::Clean,
                zen_db::results::RowMark::Changed => GridRowMark::Changed,
                zen_db::results::RowMark::Inserted => GridRowMark::Inserted,
                zen_db::results::RowMark::Deleted => GridRowMark::Deleted,
            })
            .collect()
    }

    fn link(&self, column: usize) -> Option<&str> {
        self.links.get(column)?.as_deref()
    }

    fn references(&self) -> &[String] {
        &self.references
    }

    fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    fn full_text(&self, row: u64, column: usize) -> Option<String> {
        let row = usize::try_from(row).ok()?;
        Some(match self.set.get(row, column) {
            zen_db::value::Value::Json(text) => text,
            value => value.display().unwrap_or_default(),
        })
    }
}

fn millis(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0)
}

fn seconds(ms: u64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

fn small_button(
    ui: &mut Ui,
    p: &IdePalette,
    rect: Rect,
    label: &str,
    enabled: bool,
    salt: (&str, impl std::hash::Hash + std::fmt::Debug),
) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, ui.scope_id().with(salt), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let color = if enabled { p.text } else { p.text_secondary.gamma_multiply(0.6) };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::regular(12.0), color);
    accessibility::button_with(&response, || label.to_owned());
    if !enabled {
        accessibility::disabled(&response);
    }
    response
}

fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font.clone(), ui.visuals().text_color()).size().x)
}

pub(super) fn panel(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (path, state): (&Path, &Value),
) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.island);
    let region = ui.interact(area, ui.scope_id().with("database-results"), Sense::hover());
    accessibility::name_with(&region, egui::Role::Pane, || t!("ide-database-results"));
    view.results.grid_ids.clear();
    let splitter = Rect::from_min_max(area.min, pos2(area.right(), area.top() + SPLITTER));
    let drag = ui.interact(splitter, ui.scope_id().with("database-results-splitter"), Sense::drag());
    if drag.hovered() || drag.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if drag.dragged() {
        view.results.height = (view.results.height - drag.drag_delta().y).max(MIN_HEIGHT);
    }
    ui.painter().hline(area.x_range(), area.top() + 0.5, Stroke::new(1.0, p.frame));
    let header = Rect::from_min_size(pos2(area.left(), area.top() + SPLITTER - 2.0), vec2(area.width(), HEADER));
    header_line(view, ui, p, registry, session, (path, state), header);
    let tabs = Rect::from_min_size(pos2(area.left(), header.bottom()), vec2(area.width(), TABS));
    let active = tab_row(view, ui, p, registry, session, (path, state), tabs);
    let body = Rect::from_min_max(pos2(area.left(), tabs.bottom()), area.max);
    let mut child = ui.new_child(UiBuilder::new().id_salt("database-results-body").max_rect(body));
    match active {
        Some(Active::Result(id)) => result_body(view, &mut child, p, registry, session, id),
        Some(Active::Output) => output(&mut child, p, registry, session, path),
        Some(Active::Log) => log(&mut child, p, registry, session, path),
        Some(Active::Variables) => super::variables::body(view, &mut child, p, registry, session, path),
        Some(Active::Connections) => connections(view, &mut child, p, registry, session, (path, state)),
        None => {
            child.painter().text(
                body.center(),
                Align2::CENTER_CENTER,
                t!("ide-database-no-results"),
                theme::regular(13.0),
                p.text_secondary,
            );
        }
    }
}

fn header_line(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (path, state): (&Path, &Value),
    rect: Rect,
) {
    let last = &state["last"];
    if last.is_null() {
        return;
    }
    let font = theme::regular(12.0);
    let left = rect.left() + 10.0;
    let y = rect.center().y;
    let status = last["status"].as_str().unwrap_or_default();
    let (text, color, glyph): (String, Color32, Option<super::super::glyphs::Glyph>) = match status {
        "running" => {
            let seconds = millis(&last["elapsedMs"]) as f64 / 1000.0;
            (t!("ide-database-running", seconds = format!("{seconds:.1}")), p.text, None)
        }
        "failed" => {
            let error = last["error"].as_str().unwrap_or_default();
            let text = match last["errorLine"].as_u64() {
                Some(line) => t!("ide-database-header-failed", line = line, error = error),
                None => error.to_owned(),
            };
            (text, p.error_text, None)
        }
        "cancelled" => {
            (t!("ide-database-header-cancelled"), p.warning_text, Some(glyph_stop as super::super::glyphs::Glyph))
        }
        _ => {
            let mut parts = vec![t!("ide-database-header-statements", count = millis(&last["executed"]))];
            let updated = millis(&last["updated"]);
            if updated > 0 {
                parts.push(t!("ide-database-header-updated", count = updated));
            }
            let rows = millis(&last["rows"]);
            if rows > 0 {
                parts.push(t!("ide-database-header-rows", count = rows));
            }
            parts.push(t!(
                "ide-database-header-times",
                execution = seconds(millis(&last["executionMs"])),
                fetch = seconds(millis(&last["fetchMs"]))
            ));
            (parts.join(" · "), p.text_secondary, Some(glyph_check as super::super::glyphs::Glyph))
        }
    };
    let mut x = left;
    if let Some(glyph) = glyph {
        let mark = if status == "done" { p.vcs_added } else { color };
        glyph(ui.painter(), Rect::from_center_size(pos2(x + 6.0, y), vec2(12.0, 12.0)), mark);
        x += 18.0;
    }
    let cancel_width = if status == "running" { 70.0 } else { 0.0 };
    let job = vcs::truncated(&text, font.clone(), color, (rect.right() - x - 16.0 - cancel_width).max(40.0));
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let width = galley.size().x;
    ui.painter().galley(pos2(x, y - galley.size().y / 2.0), galley, color);
    let label = ui.interact(
        Rect::from_min_size(pos2(x, rect.top()), vec2(width, rect.height())),
        ui.scope_id().with("database-header"),
        Sense::hover(),
    );
    accessibility::name_with(&label, egui::Role::Label, || text.clone());
    if status == "running" {
        let button = Rect::from_min_size(pos2(x + width + 10.0, rect.top() + 2.0), vec2(60.0, rect.height() - 4.0));
        if small_button(ui, p, button, &t!("ide-database-cancel-button"), true, ("database-header-cancel", 0)).clicked()
        {
            view.dispatch(registry, session, "db.operation.cancel", json!({ "path": path }));
        }
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Active {
    Result(u64),
    Output,
    Log,
    Variables,
    Connections,
}

fn tab_row(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (path, state): (&Path, &Value),
    rect: Rect,
) -> Option<Active> {
    let tabs = state["tabs"].as_array().cloned().unwrap_or_default();
    let active = match state["panel"]["active"]["kind"].as_str() {
        Some("result") => state["panel"]["active"]["result"].as_u64().map(Active::Result),
        Some("output") => Some(Active::Output),
        Some("log") => Some(Active::Log),
        Some("variables") => Some(Active::Variables),
        Some("connections") if state["connections"].is_object() => Some(Active::Connections),
        _ => None,
    }
    .or_else(|| tabs.first().and_then(|tab| tab["result"].as_u64()).map(Active::Result))
    .or(state["last"].is_object().then_some(Active::Log));
    let font = theme::regular(12.5);
    let mut x = rect.left() + 6.0;
    let mut chosen = None;
    let mut entries: Vec<(Active, String, bool)> = tabs
        .iter()
        .filter_map(|tab| {
            Some((Active::Result(tab["result"].as_u64()?), tab["title"].as_str()?.to_owned(), tab["pinned"] == true))
        })
        .collect();
    let messages = millis(&state["last"]["messages"]);
    let output =
        if messages > 0 { format!("{} ({messages})", t!("ide-database-output")) } else { t!("ide-database-output") };
    if state["connections"].is_object() {
        entries.insert(0, (Active::Connections, t!("ide-database-connections-tab"), false));
    }
    entries.push((Active::Output, output, false));
    entries.push((Active::Log, t!("ide-database-log"), false));
    entries.push((Active::Variables, t!("ide-database-variables"), false));
    for (entry, title, pinned) in entries {
        let result = match entry {
            Active::Result(id) => Some(id),
            _ => None,
        };
        let renaming = result.is_some() && view.results.renaming.as_ref().is_some_and(|(id, _)| Some(*id) == result);
        let title_width = text_width(ui, &title, &font).min(220.0);
        let width = title_width + 20.0 + if pinned { 14.0 } else { 0.0 } + if result.is_some() { 16.0 } else { 0.0 };
        let width = if renaming { 180.0 } else { width };
        let tab = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(width, rect.height() - 2.0));
        x = tab.right() + 2.0;
        let selected = active == Some(entry);
        let response = ui.interact(
            tab,
            ui.scope_id().with(("database-result-tab", title.as_str(), result.unwrap_or(u64::MAX))),
            Sense::click(),
        );
        if selected {
            ui.painter().rect_filled(tab, 4, p.active_tab);
            ui.painter().hline(tab.x_range(), tab.bottom() - 1.0, Stroke::new(2.0, p.active_tab_border));
        } else if response.hovered() {
            ui.painter().rect_filled(tab, 4, p.hover);
        }
        accessibility::item(&response, egui::Role::Tab, selected, &title);
        let mut text_x = tab.left() + 10.0;
        if pinned {
            glyph_pin(
                ui.painter(),
                Rect::from_center_size(pos2(text_x + 5.0, tab.center().y), vec2(10.0, 10.0)),
                p.text_secondary,
            );
            text_x += 14.0;
        }
        if renaming {
            if let Some((id, name)) = view.results.renaming.as_mut() {
                let id = *id;
                let field = Rect::from_min_size(
                    pos2(text_x - 4.0, tab.top() + 2.0),
                    vec2(tab.right() - text_x, tab.height() - 4.0),
                );
                let mut child = ui.new_child(UiBuilder::new().max_rect(field));
                let edit = child.add(
                    egui::TextEdit::singleline(name)
                        .font(font.clone())
                        .background_color(p.text_field)
                        .text_color(p.text)
                        .desired_width(field.width()),
                );
                edit.request_focus();
                let (enter, escape) =
                    ui.input(|input| (input.key_pressed(egui::Key::Enter), input.key_pressed(egui::Key::Escape)));
                if enter {
                    let title = name.clone();
                    view.results.renaming = None;
                    view.dispatch(registry, session, "db.result.rename", json!({ "id": id, "title": title }));
                } else if escape || edit.lost_focus() {
                    view.results.renaming = None;
                }
            }
            continue;
        }
        let job = vcs::truncated(&title, font.clone(), if selected { p.text } else { p.text_secondary }, 220.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(text_x, tab.center().y - galley.size().y / 2.0), galley, p.text);
        if let Some(id) = result {
            let close = Rect::from_center_size(pos2(tab.right() - 10.0, tab.center().y), vec2(12.0, 12.0));
            let closing = ui.interact(close, ui.scope_id().with(("database-result-close", id)), Sense::click());
            if response.hovered() || closing.hovered() || selected {
                glyph_close(ui.painter(), close.shrink(2.0), if closing.hovered() { p.text } else { p.text_secondary });
            }
            accessibility::button_with(&closing, || t!("ide-database-close-tab"));
            if closing.clicked() || response.middle_clicked() {
                view.dispatch(registry, session, "db.result.close", json!({ "id": id }));
                continue;
            }
            response.context_menu(|ui| {
                customization::popup(
                    ui,
                    customization::DATABASE_RESULT_POPUP,
                    |_| true,
                    |ui, slot| match slot {
                        "ide-database-pin" => {
                            let pin = if pinned { t!("ide-database-unpin") } else { t!("ide-database-pin") };
                            if ui.button(pin).clicked() {
                                view.dispatch(
                                    registry,
                                    session,
                                    "db.result.pin",
                                    json!({ "id": id, "pinned": !pinned }),
                                );
                                ui.close();
                            }
                        }
                        "ide-database-rename" => {
                            if ui.button(t!("ide-database-rename")).clicked() {
                                view.results.renaming = Some((id, title.clone()));
                                ui.close();
                            }
                        }
                        _ => {
                            if ui.button(t!("ide-database-close-tab")).clicked() {
                                view.dispatch(registry, session, "db.result.close", json!({ "id": id }));
                                ui.close();
                            }
                        }
                    },
                );
            });
            if response.double_clicked() {
                view.results.renaming = Some((id, title.clone()));
            }
        }
        if response.clicked() {
            chosen = Some(entry);
        }
    }
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.frame));
    if let Some(entry) = chosen {
        let tab = match entry {
            Active::Result(id) => json!({ "kind": "result", "result": id }),
            Active::Output => json!({ "kind": "output" }),
            Active::Log => json!({ "kind": "log" }),
            Active::Variables => json!({ "kind": "variables" }),
            Active::Connections => json!({ "kind": "connections" }),
        };
        view.dispatch(registry, session, "db.console.showPanelTab", json!({ "path": path, "tab": tab }));
        return Some(entry);
    }
    active
}

fn result_of(session: &Session, id: u64) -> Option<Arc<ResultSet>> {
    session.state::<zen_workbench::database::Database>()?.started()?.results().get(id)
}

pub(super) fn result_body(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    id: u64,
) {
    let area = ui.max_rect();
    let Some(set) = result_of(session, id) else {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            t!("ide-database-no-results"),
            theme::regular(13.0),
            p.text_secondary,
        );
        return;
    };
    let settings = registry.dispatch(session, "db.settings.get", json!({})).unwrap_or_default();
    view.results.view = Some(settings["view"].as_str().unwrap_or("grid").to_owned());
    let panel_shown = settings["valuePanel"] == true;
    let wanted: Option<zen_db::present::ValueFormat> = serde_json::from_value(settings["valueFormat"].clone()).ok();
    let info = set.info();
    let (filter, edit) = view.results.data.states(ui.ctx(), registry, session, id);
    let filter_bar = Rect::from_min_size(area.min, vec2(area.width(), super::data_edit::FILTER_BAR));
    super::data_edit::filter_bar(view, ui, p, registry, session, (id, &filter), filter_bar);
    let edit_height = if super::data_edit::wants_edit_bar(view, id, &edit) { super::data_edit::EDIT_BAR } else { 0.0 };
    let mut grid_area = Rect::from_min_max(
        pos2(area.left(), filter_bar.bottom()),
        pos2(area.right(), area.bottom() - BOTTOM - edit_height),
    );
    let panel_width = if panel_shown { view.results.value.width(grid_area.width()) } else { 0.0 };
    let panel_area = Rect::from_min_max(pos2(grid_area.right() - panel_width, grid_area.top()), grid_area.max);
    grid_area.max.x -= panel_width;
    let relations = super::relations::relations(view, registry, session, id);
    let source = ResultSource::with_states(Arc::clone(&set), &filter, &edit, &relations);
    let grid_id = ui.make_persistent_id(("database-grid", id));
    let state = view.results.grids.entry(id).or_default();
    let mut child = ui.new_child(UiBuilder::new().id_salt(("database-result", id)).max_rect(grid_area));
    let mode = view.results.view.clone().unwrap_or_else(|| "grid".to_owned());
    let mut near_end = false;
    let documents = edit["documents"] == true;
    let failed = filter["error"].as_str().filter(|_| filter["query"].is_null());
    match (failed, mode.as_str()) {
        (Some(error), _) => {
            failed_read(&mut child, p, error, filter["condition"].as_str().is_some_and(|c| !c.is_empty()))
        }
        (None, "text") => near_end = text_view(&mut child, p, &source, state),
        (None, "json") if documents => near_end = json_view(&mut child, p, &source, state),
        (None, "record") => {
            record_view(&mut child, p, &source, state, &mut view.results.filter);
            let tab = ui.memory(|memory| memory.focused().is_none())
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
            if tab {
                view.results.view = Some("grid".to_owned());
                view.dispatch(registry, session, "db.result.view", json!({ "view": "grid" }));
            }
        }
        (None, _) => {
            let features = GridFeatures {
                editable: edit["editable"] == true && edit["saving"].is_null(),
                filterable: filter["filterable"] == true,
                sortable: true,
                generate: edit["table"].is_string(),
                clipboard: true,
                result: true,
                value_panel: panel_shown,
            };
            if let Some((row, column)) = zen_workbench::database::values::take_selection(session, id) {
                state.go_to(row as u64, source.rows);
                if column != usize::MAX && column < source.columns.len() {
                    state.select(row as u64, column);
                }
            }
            let outcome = data_grid::show_with(&mut child, p, &source, state, grid_id, features);
            grid_actions(view, registry, session, id, &outcome);
            view.results.grid_ids.push(grid_id);
            near_end = outcome.near_end;
            if let Some(reason) = &outcome.refused {
                if source.refusal.is_some() {
                    view.results.data.refuse(id);
                } else {
                    view.results.data.tell(id, reason.clone(), true);
                }
            }
            let names: Vec<String> = source.columns.iter().map(|column| column.name.clone()).collect();
            if let Some(row) =
                super::data_edit::act(view, ui, registry, session, (id, &filter, &edit), &names, &outcome)
                && let Some(state) = view.results.grids.get_mut(&id)
            {
                state.select(row, 0);
                state.scroll_to(row);
            }
            super::relations::act(view, registry, session, (id, &relations), &outcome);
            let cells = view
                .results
                .grids
                .get(&id)
                .and_then(GridState::selection)
                .map(|selection| selection.bounds(source.rows, source.columns.len()));
            super::clipboard::act(view, registry, session, id, &outcome, cells);
        }
    }
    let selection = view.results.grids.get(&id).and_then(GridState::selection);
    let (rows, columns) = (source.rows, source.columns.len());
    let bounds = selection.map(|selection| {
        let (rows, columns) = selection.bounds(rows, columns);
        let to = |range: Range<u64>| {
            usize::try_from(range.start).unwrap_or(usize::MAX)..usize::try_from(range.end).unwrap_or(usize::MAX)
        };
        (to(rows), columns)
    });
    let cell = selection.map(|selection| (usize::try_from(selection.head.0).unwrap_or(usize::MAX), selection.head.1));
    let bounds = bounds.filter(|(rows, columns)| !rows.is_empty() && !columns.is_empty());
    let calc = view.results.value.status(ui, session, &set, bounds.clone());
    if panel_shown {
        view.results.value.show(ui, p, registry, session, panel_area, (&set, cell, wanted), bounds, calc);
    }
    let reading = matches!(info.state, zen_db::results::ReadState::Reading);
    let open = matches!(info.state, zen_db::results::ReadState::Open);
    let capped = info.stopped_because == Some(zen_db::results::StopReason::Cap);
    if near_end && (open || capped) && info.more && view.results.fetch_asked.get(&id) != Some(&info.rows) {
        view.results.fetch_asked.insert(id, info.rows);
        view.dispatch(registry, session, "db.result.fetchNext", json!({ "id": id }));
    }
    if reading || open {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
    if edit_height > 0.0 {
        let bar = Rect::from_min_max(
            pos2(area.left(), area.bottom() - BOTTOM - edit_height),
            pos2(area.right(), area.bottom() - BOTTOM),
        );
        super::data_edit::edit_bar(view, ui, p, registry, session, (id, &edit), bar);
    }
    bottom_bar(
        view,
        ui,
        p,
        registry,
        session,
        (&set, &info, panel_shown, settings["paging"]["page"].as_u64().unwrap_or(200), documents),
        Rect::from_min_max(pos2(area.left(), area.bottom() - BOTTOM), area.max),
    );
    super::data_edit::dialogs(view, ui, p, registry, session);
    go_to_dialog(view, ui, p, registry, session);
    super::relations::lookup_popup(view, ui, p, registry, session);
    super::clipboard::poll(view, ui.ctx(), registry, session);
    super::clipboard::dialogs(view, ui, p, registry, session);
}

const FETCH_BOX: f32 = 46.0;
const PAGE_BUTTONS: f32 = 100.0;

fn page_glyph(painter: &egui::Painter, rect: Rect, (back, edge): (bool, bool), color: Color32) {
    let c = rect.center();
    let dir = if back { -1.0 } else { 1.0 };
    let tip = c.x + dir * 3.0;
    let base = c.x - dir * 3.0;
    painter.add(egui::Shape::convex_polygon(
        vec![pos2(base, c.y - 4.0), pos2(tip, c.y), pos2(base, c.y + 4.0)],
        color,
        Stroke::NONE,
    ));
    if edge {
        let bar = tip + dir * 1.5;
        painter.vline(bar, (c.y - 4.0)..=(c.y + 4.0), Stroke::new(1.5, color));
    }
}

fn page_buttons(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (id, info): (u64, &zen_db::results::ResultInfo),
    at: Rect,
) -> f32 {
    let rows = info.rows;
    let state = view.results.grids.entry(id).or_default();
    let current = state.current_row().unwrap_or(state.top());
    let page = state.page_rows();
    let buttons = [
        ("first", t!("ide-database-page-first"), (true, true), current > 0),
        ("previous", t!("ide-database-page-previous"), (true, false), current > 0),
        ("next", t!("ide-database-page-next"), (false, false), current + 1 < rows || info.more),
        ("last", t!("ide-database-page-last"), (false, true), current + 1 < rows),
    ];
    let mut x = at.left();
    let mut wanted = None;
    for (kind, tip, glyph, enabled) in buttons {
        let rect = Rect::from_min_size(pos2(x, at.top()), vec2(22.0, at.height()));
        x = rect.right() + 1.0;
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let response = ui.interact(rect, ui.scope_id().with(("database-page", kind, id)), sense);
        if enabled && response.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        let color = if enabled { p.text } else { p.text_secondary.gamma_multiply(0.6) };
        page_glyph(ui.painter(), rect, glyph, color);
        accessibility::button_with(&response, || tip.clone());
        if !enabled {
            accessibility::disabled(&response);
        }
        if response.on_hover_text(&tip).clicked() {
            wanted = Some(kind);
        }
    }
    let state = view.results.grids.entry(id).or_default();
    match wanted {
        Some("first") => state.go_to(0, rows),
        Some("previous") => state.go_to(current.saturating_sub(page), rows),
        Some("next") => {
            state.go_to(current + page, rows);
            if current + 2 * page >= rows && info.more {
                view.dispatch(registry, session, "db.result.fetchNext", json!({ "id": id }));
            }
        }
        Some("last") => state.go_to(rows.saturating_sub(1), rows),
        _ => {}
    }
    x + 2.0
}

fn fetch_size_box(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    page: u64,
    at: Rect,
) -> f32 {
    let mut text = view.results.fetch_size.clone().unwrap_or_else(|| page.to_string());
    let field_id = ui.scope_id().with("database-fetch-size");
    ui.painter().rect_filled(at.shrink2(vec2(0.0, 2.0)), 4, p.text_field);
    ui.painter().rect_stroke(
        at.shrink2(vec2(0.0, 2.0)),
        4,
        Stroke::new(1.0, p.text_field_border),
        egui::StrokeKind::Inside,
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(at.shrink2(vec2(5.0, 4.0))));
    let response = child.add(
        egui::TextEdit::singleline(&mut text)
            .id(field_id)
            .frame(egui::Frame::NONE)
            .font(theme::regular(12.0))
            .desired_width(at.width() - 10.0),
    );
    accessibility::labelled(&response, &t!("ide-database-fetch-size"));
    let response = response.on_hover_text(t!("ide-database-fetch-size"));
    if response.has_focus() {
        view.results.fetch_size = Some(text.chars().filter(char::is_ascii_digit).collect());
    } else if response.lost_focus() {
        view.results.fetch_size = None;
        if let Ok(wanted) = text.trim().parse::<u64>()
            && wanted != page
            && wanted > 0
        {
            view.dispatch(registry, session, "db.settings.update", json!({ "page": wanted }));
        }
    }
    at.right() + 6.0
}

fn grid_actions(
    view: &mut DatabaseView,
    registry: &Registry,
    session: &mut Session,
    id: u64,
    outcome: &data_grid::GridOutcome,
) {
    for action in &outcome.actions {
        match action {
            GridAction::GoToRow => view.results.go_to = Some((id, String::new(), None)),
            GridAction::FetchNext => {
                view.dispatch(registry, session, "db.result.fetchNext", json!({ "id": id }));
            }
            GridAction::FetchAll => view.results.confirm_all = Some(id),
            GridAction::ToggleValuePanel => {
                view.dispatch(registry, session, "db.result.toggleValuePanel", json!({}));
            }
            GridAction::View { mode } => {
                let current = view.results.view.clone().unwrap_or_else(|| "grid".to_owned());
                let mode = if *mode == "record" && current == "record" { "grid" } else { *mode };
                view.results.view = Some(mode.to_owned());
                view.dispatch(registry, session, "db.result.view", json!({ "view": mode }));
            }
            GridAction::Export => {
                super::super::export_dialog::request(session, json!({ "source": { "kind": "result", "id": id } }))
            }
            _ => {}
        }
    }
}

fn go_to_dialog(view: &mut DatabaseView, ui: &Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
    let Some((id, mut text, mut error)) = view.results.go_to.clone() else { return };
    let mut close = false;
    let modal = egui::Modal::new(ui.scope_id().with("database-go-to-row")).frame(super::super::dialog_frame(p)).show(
        ui.ctx(),
        |ui| {
            ui.set_width(320.0);
            ui.label(egui::RichText::new(t!("ide-database-grid-go-to-row")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(8.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut text).hint_text(t!("ide-database-go-to-row-hint")).desired_width(300.0),
            );
            accessibility::labelled(&field, &t!("ide-database-go-to-row-hint"));
            field.request_focus();
            if let Some(message) = &error {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(message).font(theme::regular(12.0)).color(p.error_text))
                        .wrap(),
                );
            }
            ui.add_space(12.0);
            let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                let go = vcs::primary_button(ui, p, &t!("ide-database-go-to-row-go")).clicked();
                if go || enter {
                    match text.trim().parse::<usize>() {
                        Ok(row) => {
                            match registry.dispatch(session, "db.result.select", json!({ "id": id, "row": row })) {
                                Ok(_) => close = true,
                                Err(failed) => error = Some(super::message(failed)),
                            }
                        }
                        Err(_) => error = Some(t!("ide-database-go-to-row-number")),
                    }
                }
                if vcs::secondary_button(ui, p, &t!("ide-database-preview-close")).clicked() {
                    close = true;
                }
            });
        },
    );
    view.results.go_to = if close || modal.should_close() { None } else { Some((id, text, error)) };
}

fn failed_read(ui: &mut Ui, p: &IdePalette, error: &str, filtered: bool) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.editor_background);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_center_size(area.center(), vec2(area.width().min(560.0) - 24.0, 120.0)))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let title = if filtered { t!("ide-database-filter-failed") } else { t!("ide-database-read-failed") };
    child.label(egui::RichText::new(title).font(theme::semibold(13.0)).color(p.text));
    child.add_space(6.0);
    child.add(egui::Label::new(egui::RichText::new(error).font(theme::regular(12.5)).color(p.error_text)).wrap());
}

fn bottom_bar(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (set, info, panel_shown, page, documents): (&ResultSet, &zen_db::results::ResultInfo, bool, u64, bool),
    rect: Rect,
) {
    let id = set.id();
    ui.painter().rect_filled(rect, 0, p.island);
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.frame));
    let y = rect.top() + 2.0;
    let height = rect.height() - 4.0;
    let mut x = rect.left() + 6.0;
    let button = |ui: &mut Ui, x: &mut f32, label: String, enabled: bool, salt: &str| {
        let width = text_width(ui, &label, &theme::regular(12.0)) + 16.0;
        let at = Rect::from_min_size(pos2(*x, y), vec2(width, height));
        *x = at.right() + 2.0;
        small_button(ui, p, at, &label, enabled, (salt, id))
    };
    if view.results.confirm_all == Some(id) {
        let question = t!("ide-database-confirm-fetch-all");
        ui.painter().text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            &question,
            theme::regular(12.0),
            p.warning_text,
        );
        x += text_width(ui, &question, &theme::regular(12.0)) + 10.0;
        if button(ui, &mut x, t!("ide-database-fetch-all"), true, "database-confirm-all").clicked() {
            view.results.confirm_all = None;
            view.dispatch(registry, session, "db.result.fetchAll", json!({ "id": id, "confirmed": true }));
        }
        if button(ui, &mut x, t!("ide-database-cancel-button"), true, "database-confirm-cancel").clicked() {
            view.results.confirm_all = None;
        }
        return;
    }
    x += super::relations::navigation_buttons(view, ui, p, registry, session, (id, x, y, height));
    ui.painter().vline(x + 2.0, (rect.top() + 6.0)..=(rect.bottom() - 6.0), Stroke::new(1.0, p.frame));
    x += 6.0;
    let rows = match (info.total, info.more) {
        (Some(total), _) => t!("ide-database-rows-of", rows = info.rows, total = total),
        (None, true) => t!("ide-database-rows-more", rows = info.rows),
        (None, false) => info.rows.to_string(),
    };
    let labels = [
        t!("ide-database-refresh"),
        t!("ide-database-fetch-next"),
        t!("ide-database-fetch-all"),
        t!("ide-database-export"),
    ];
    let small = theme::regular(12.0);
    let total_label = t!("ide-database-calculate-total");
    let counts = info.more && info.total.is_none();
    let buttons: f32 = labels.iter().map(|label| text_width(ui, label, &small) + 18.0).sum::<f32>()
        + FETCH_BOX
        + 8.0
        + if counts { text_width(ui, &total_label, &small) + 18.0 } else { 0.0 };
    let right_side: f32 = [
        t!("ide-database-value-panel"),
        t!("ide-database-view-grid"),
        t!("ide-database-view-text"),
        t!("ide-database-view-record"),
    ]
    .iter()
    .map(|label| text_width(ui, label, &small) + 18.0)
    .sum();
    let rows_width = text_width(ui, &rows, &theme::semibold(12.0)) + 14.0;
    let least = text_width(ui, &labels[0], &small) + 18.0 + 28.0 + 8.0 + rows_width + 16.0 + right_side + 14.0;
    if x + PAGE_BUTTONS + least <= rect.right() {
        x = page_buttons(
            view,
            ui,
            p,
            registry,
            session,
            (id, info),
            Rect::from_min_size(pos2(x, y), vec2(0.0, height)),
        );
        ui.painter().vline(x + 2.0, (rect.top() + 6.0)..=(rect.bottom() - 6.0), Stroke::new(1.0, p.frame));
        x += 6.0;
    }
    let collapsed = x + buttons + 8.0 + rows_width + 16.0 + right_side + 14.0 > rect.right();
    let [refresh_label, next_label, all_label, export_label] = labels;
    let refresh = button(ui, &mut x, refresh_label, true, "database-refresh");
    let (mut next, mut all, mut export) = (false, false, false);
    if collapsed {
        let at = Rect::from_min_size(pos2(x, y), vec2(26.0, height));
        x = at.right() + 2.0;
        let more = super::more_button(ui, p, at, ("result", id));
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(180.0);
            next = ui.add_enabled(info.more, egui::Button::new(&next_label)).clicked();
            all = ui.add_enabled(info.more, egui::Button::new(&all_label)).clicked();
            if counts && ui.button(&total_label).clicked() {
                view.dispatch(registry, session, "db.result.count", json!({ "id": id }));
            }
            export = ui.button(&export_label).clicked();
        });
    } else {
        next = button(ui, &mut x, next_label, info.more, "database-fetch-next").clicked();
        all = button(ui, &mut x, all_label, info.more, "database-fetch-all").clicked();
        export = button(ui, &mut x, export_label, true, "database-export").clicked();
        x = fetch_size_box(
            view,
            ui,
            p,
            registry,
            session,
            page,
            Rect::from_min_size(pos2(x + 4.0, y), vec2(FETCH_BOX, height)),
        );
        if counts && button(ui, &mut x, total_label.clone(), true, "database-calculate-total").clicked() {
            view.dispatch(registry, session, "db.result.count", json!({ "id": id }));
        }
    }
    if export {
        let mut params = json!({ "source": { "kind": "result", "id": id } });
        if let Some(selection) = view.results.grids.get(&id).and_then(GridState::selection) {
            let (rows, columns) = selection.bounds(info.rows, info.columns.len());
            if !rows.is_empty() && !columns.is_empty() && (rows.end - rows.start > 1 || columns.len() > 1) {
                params["selection"] =
                    json!({ "rows": [rows.start, rows.end - 1], "columns": [columns.start, columns.end - 1] });
            }
        }
        super::super::export_dialog::request(session, params);
    }
    if refresh.clicked() {
        view.dispatch(registry, session, "db.result.refresh", json!({ "id": id }));
    }
    if next {
        view.dispatch(registry, session, "db.result.fetchNext", json!({ "id": id }));
    }
    if all {
        view.results.confirm_all = Some(id);
    }
    ui.painter().vline(x + 3.0, (rect.top() + 6.0)..=(rect.bottom() - 6.0), Stroke::new(1.0, p.frame));
    x += 8.0;
    let width = text_width(ui, &rows, &theme::semibold(12.0)) + 14.0;
    let count = Rect::from_min_size(pos2(x, y), vec2(width, height));
    let counting = ui.interact(count, ui.scope_id().with(("database-count", id)), Sense::click());
    if info.more && counting.hovered() {
        ui.painter().rect_filled(count, 4, p.hover);
    }
    ui.painter().text(count.center(), Align2::CENTER_CENTER, &rows, theme::semibold(12.0), p.text);
    if info.more {
        counting.clone().on_hover_text(t!("ide-database-count-hint"));
        accessibility::button_with(&counting, || t!("ide-database-count-hint"));
        if counting.clicked() {
            view.dispatch(registry, session, "db.result.count", json!({ "id": id }));
        }
    }
    x = count.right() + 8.0;
    let mut stats = t!(
        "ide-database-result-stats",
        rows = info.rows,
        execution = seconds(info.execution_ms),
        fetch = seconds(info.fetch_ms)
    );
    let note = match (info.state, info.stopped_because) {
        (zen_db::results::ReadState::Reading, _) => Some(t!("ide-database-reading-rows")),
        (zen_db::results::ReadState::Open, _) => Some(t!("ide-database-portal-open")),
        (_, Some(zen_db::results::StopReason::Budget)) => Some(t!("ide-database-stopped-budget")),
        (_, Some(zen_db::results::StopReason::Cap)) => Some(t!("ide-database-stopped-cap")),
        (_, Some(zen_db::results::StopReason::Cancelled)) => Some(t!("ide-database-stopped-cancelled")),
        _ => None,
    };
    if let Some(note) = note {
        stats = format!("{stats} · {note}");
    }
    let mut modes = vec![
        ("grid", t!("ide-database-view-grid")),
        ("text", t!("ide-database-view-text")),
        ("record", t!("ide-database-view-record")),
    ];
    if documents {
        modes.push(("json", t!("ide-database-view-json")));
    }
    let panel_label = t!("ide-database-value-panel");
    let panel_width = text_width(ui, &panel_label, &theme::regular(12.0)) + 18.0;
    let modes_width: f32 =
        modes.iter().map(|(_, label)| text_width(ui, label, &theme::regular(12.0)) + 18.0).sum::<f32>()
            + panel_width
            + 8.0;
    let room = rect.right() - modes_width - x - 12.0;
    if room >= 40.0 {
        let job = vcs::truncated(&stats, theme::regular(12.0), p.text_secondary, room);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, p.text_secondary);
    }
    let mut right = rect.right() - 6.0 - modes_width;
    let toggle = Rect::from_min_size(pos2(right, y), vec2(panel_width - 2.0, height));
    if panel_shown {
        ui.painter().rect_filled(toggle, 4, p.active_tab);
    }
    let response = small_button(ui, p, toggle, &panel_label, true, ("database-value-panel", id));
    accessibility::pressed(&response, panel_shown);
    let key = crate::keymap::shortcut("db.result.toggleValuePanel");
    super::super::hint(ui, &response, &panel_label, key.as_deref(), super::super::Side::Left);
    if response.clicked() {
        view.dispatch(registry, session, "db.result.toggleValuePanel", json!({}));
    }
    right = toggle.right() + 8.0;
    let current =
        view.results.view.clone().filter(|mode| documents || mode != "json").unwrap_or_else(|| "grid".to_owned());
    for (mode, label) in modes {
        let width = text_width(ui, &label, &theme::regular(12.0)) + 16.0;
        let at = Rect::from_min_size(pos2(right, y), vec2(width, height));
        right = at.right() + 2.0;
        let selected = current == mode;
        if selected {
            ui.painter().rect_filled(at, 4, p.active_tab);
        }
        let response = small_button(ui, p, at, &label, true, ("database-view", mode));
        accessibility::pressed(&response, selected);
        if response.clicked() && !selected {
            view.results.view = Some(mode.to_owned());
            view.dispatch(registry, session, "db.result.view", json!({ "view": mode }));
        }
    }
}

fn scroll_rows(ui: &Ui, state: &mut GridState, rows: u64, visible: u64, area: Rect) {
    let hovered = ui.input(|input| input.pointer.hover_pos()).is_some_and(|at| area.contains(at));
    if !hovered {
        return;
    }
    let delta = ui.input(|input| input.smooth_scroll_delta.y);
    if delta != 0.0 {
        let step = (-delta / 18.0).round() as i64;
        let top = i64::try_from(state.top())
            .unwrap_or(i64::MAX)
            .saturating_add(step)
            .clamp(0, rows.saturating_sub(visible) as i64);
        state.scroll_to(top as u64);
    }
}

fn text_view(ui: &mut Ui, p: &IdePalette, source: &ResultSource, state: &mut GridState) -> bool {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.editor_background);
    let font = FontId::monospace(12.5);
    let line = 18.0;
    let visible = ((area.height() - 26.0) / line).floor().max(1.0) as u64;
    scroll_rows(ui, state, source.rows, visible, area);
    let top = state.top().min(source.rows.saturating_sub(visible));
    let columns = source.columns.len();
    let cells = source.cells(top..(top + visible).min(source.rows), 0..columns);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            let header = source.columns[column].name.chars().count();
            cells
                .iter()
                .map(|row| if row[column].null { 4 } else { row[column].text.chars().count() })
                .fold(header, usize::max)
                .min(40)
        })
        .collect();
    let pad = |text: &str, width: usize, right: bool| {
        let shown: String = text.chars().take(width).collect();
        let fill = " ".repeat(width.saturating_sub(shown.chars().count()));
        if right { format!("{fill}{shown}") } else { format!("{shown}{fill}") }
    };
    let header: Vec<String> =
        (0..columns).map(|column| pad(&source.columns[column].name, widths[column], false)).collect();
    let mut lines = vec![header.join("|"), widths.iter().map(|width| "-".repeat(*width)).collect::<Vec<_>>().join("+")];
    for row in &cells {
        let shown: Vec<String> = (0..columns)
            .map(|column| {
                let cell = &row[column];
                let text = if cell.null { "NULL" } else { cell.text.as_str() };
                pad(text, widths[column], cell.number)
            })
            .collect();
        lines.push(shown.join("|"));
    }
    let painter = ui.painter_at(area);
    for (index, text) in lines.iter().enumerate() {
        let color = if index < 2 { p.text_secondary } else { p.text };
        painter.text(
            pos2(area.left() + 8.0, area.top() + 4.0 + index as f32 * line),
            Align2::LEFT_TOP,
            text,
            font.clone(),
            color,
        );
    }
    let region = ui.interact(area, ui.scope_id().with("database-text-view"), Sense::hover());
    accessibility::name_with(&region, egui::Role::Document, || lines.join("\n"));
    top + visible >= source.rows
}

fn json_view(ui: &mut Ui, p: &IdePalette, source: &ResultSource, state: &mut GridState) -> bool {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.editor_background);
    if source.rows == 0 {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            t!("ide-database-no-rows"),
            theme::regular(13.0),
            p.text_secondary,
        );
        return true;
    }
    let font = FontId::monospace(12.5);
    let line = 18.0;
    let fits = ((area.height() - 8.0) / line).floor().max(1.0) as usize;
    scroll_rows(ui, state, source.rows, 1, area);
    let top = state.top().min(source.rows - 1);
    let columns = source.set.columns();
    let first = usize::try_from(top).unwrap_or(usize::MAX);
    let selected = state.selection().map(|selection| selection.head.0);
    let painter = ui.painter_at(area);
    let mut y = area.top() + 4.0;
    let mut used = 0;
    let mut shown = 0u64;
    let mut spoken = Vec::new();
    let mut clicked = None;
    for (offset, row) in source.set.page(first, fits).iter().enumerate() {
        if used >= fits {
            break;
        }
        let index = top + offset as u64;
        let text = zen_db::edit::document::shell_document(&columns, row);
        let lines: Vec<&str> = text.lines().collect();
        let height = lines.len().min(fits - used) as f32 * line;
        let block = Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), height));
        if selected == Some(index) {
            painter.rect_filled(block, 0, p.selection);
        }
        for (at, text) in lines.iter().take(fits - used).enumerate() {
            painter.text(pos2(area.left() + 8.0, y + at as f32 * line), Align2::LEFT_TOP, *text, font.clone(), p.text);
        }
        let response = ui.interact(block, ui.scope_id().with(("database-json-document", index)), Sense::click());
        if response.clicked() {
            clicked = Some(index);
        }
        used += lines.len().min(fits - used);
        y += height;
        shown += 1;
        spoken.push(text);
    }
    if let Some(index) = clicked {
        state.select(index, 0);
    }
    let region = ui.interact(area, ui.scope_id().with("database-json-view"), Sense::hover());
    accessibility::name_with(&region, egui::Role::Document, || spoken.join("\n"));
    top + shown >= source.rows
}

fn record_text(value: &zen_db::value::Value) -> String {
    use zen_db::present;
    let format = present::text_or_json(value);
    let rendered = present::render(value, format, RECORD_CHARS).unwrap_or_default();
    if rendered.truncated { format!("{}…", rendered.text) } else { rendered.text }
}

fn record_view(ui: &mut Ui, p: &IdePalette, source: &ResultSource, state: &mut GridState, filter: &mut String) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.editor_background);
    if source.rows == 0 {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            t!("ide-database-no-rows"),
            theme::regular(13.0),
            p.text_secondary,
        );
        return;
    }
    let row = state.selection().map_or(state.top(), |selection| selection.head.0).min(source.rows - 1);
    let selected_column = state.selection().map(|selection| selection.head.1);
    let bar = Rect::from_min_size(area.min, vec2(area.width(), 28.0));
    let previous = small_button(
        ui,
        p,
        Rect::from_min_size(pos2(bar.left() + 6.0, bar.top() + 3.0), vec2(26.0, 22.0)),
        "<",
        row > 0,
        ("database-record-previous", row),
    );
    let label = t!("ide-database-record-row", row = row + 1, rows = source.rows);
    ui.painter().text(
        pos2(bar.left() + 40.0, bar.center().y),
        Align2::LEFT_CENTER,
        &label,
        theme::regular(12.0),
        p.text,
    );
    let label_width = text_width(ui, &label, &theme::regular(12.0));
    let next_rect = Rect::from_min_size(pos2(bar.left() + 46.0 + label_width, bar.top() + 3.0), vec2(26.0, 22.0));
    let next = small_button(ui, p, next_rect, ">", row + 1 < source.rows, ("database-record-next", row));
    let column = selected_column.unwrap_or(0);
    if previous.clicked() {
        state.select(row - 1, column);
        state.scroll_to(row - 1);
    }
    if next.clicked() {
        state.select(row + 1, column);
        state.scroll_to(row + 1);
    }
    let field = Rect::from_min_size(pos2(next_rect.right() + 16.0, bar.top() + 3.0), vec2(220.0, 22.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(field));
    let edit = child.add(
        egui::TextEdit::singleline(filter)
            .hint_text(t!("ide-database-record-filter"))
            .background_color(p.text_field)
            .text_color(p.text)
            .desired_width(field.width()),
    );
    accessibility::labelled(&edit, &t!("ide-database-record-filter"));
    let index = usize::try_from(row).unwrap_or(usize::MAX);
    let wanted = filter.to_lowercase();
    let list = Rect::from_min_max(pos2(area.left(), bar.bottom() + 2.0), area.max);
    let mut child = ui.new_child(UiBuilder::new().id_salt("database-record").max_rect(list));
    let name_width = source
        .columns
        .iter()
        .map(|column| text_width(ui, &column.name, &theme::semibold(12.5)))
        .fold(80.0, f32::max)
        .min(260.0)
        + 16.0;
    let mut chosen = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
        for (at, column) in source.columns.iter().enumerate() {
            if !wanted.is_empty() && !column.name.to_lowercase().contains(&wanted) {
                continue;
            }
            let value = source.set.get(index, at);
            let null = value == zen_db::value::Value::Null;
            let number = matches!(
                value,
                zen_db::value::Value::Integer(_) | zen_db::value::Value::Float(_) | zen_db::value::Value::Decimal(_)
            );
            let (text, color) = if null {
                ("[NULL]".to_owned(), p.text_secondary)
            } else {
                (record_text(&value), if number { p.number } else { p.text })
            };
            let mono = matches!(
                value,
                zen_db::value::Value::Json(_) | zen_db::value::Value::Map(_) | zen_db::value::Value::Document(_)
            );
            let font = if mono { FontId::monospace(12.0) } else { theme::regular(12.5) };
            let background = ui.painter().add(egui::Shape::Noop);
            let line = ui.horizontal_top(|ui| {
                ui.add_space(8.0);
                ui.add_sized(
                    vec2(name_width, 20.0),
                    egui::Label::new(egui::RichText::new(&column.name).font(theme::semibold(12.5)).color(p.text))
                        .truncate(),
                );
                ui.add_sized(
                    vec2(90.0, 20.0),
                    egui::Label::new(
                        egui::RichText::new(&column.type_name).font(theme::regular(11.5)).color(p.text_secondary),
                    )
                    .truncate(),
                );
                ui.add(egui::Label::new(egui::RichText::new(text.clone()).font(font).color(color)).wrap());
            });
            let rect = line.response.rect;
            if selected_column == Some(at) {
                ui.painter().set(background, egui::Shape::rect_filled(rect.expand2(vec2(0.0, 1.0)), 3, p.selection));
            }
            let response = ui.interact(rect, ui.scope_id().with(("database-record-field", at)), Sense::click());
            accessibility::item(
                &response,
                egui::Role::Row,
                selected_column == Some(at),
                &format!("{} {text}", column.name),
            );
            if response.clicked() {
                chosen = Some(at);
            }
        }
    });
    if let Some(at) = chosen {
        state.select(row, at);
    }
}

fn output(ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, path: &Path) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.editor_background);
    let answer =
        registry.dispatch(session, "db.console.output", json!({ "path": path, "limit": 500 })).unwrap_or_default();
    let messages = answer["messages"].as_array().cloned().unwrap_or_default();
    if messages.is_empty() {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            t!("ide-database-no-messages"),
            theme::regular(13.0),
            p.text_secondary,
        );
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().id_salt("database-output").max_rect(area.shrink2(vec2(8.0, 4.0))));
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(&mut child, |ui| {
        for message in &messages {
            let severity = message["severity"].as_str().unwrap_or_default();
            let color = match severity {
                "WARNING" => p.warning_text,
                "ERROR" | "FATAL" | "PANIC" => p.error_text,
                "INFO" => p.text_secondary,
                _ => p.text,
            };
            let text = format!("{severity}: {}", message["text"].as_str().unwrap_or_default());
            ui.add(egui::Label::new(egui::RichText::new(text).font(FontId::monospace(12.5)).color(color)).wrap());
        }
    });
}

fn log(ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, path: &Path) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.editor_background);
    let answer =
        registry.dispatch(session, "db.console.output", json!({ "path": path, "limit": 500 })).unwrap_or_default();
    let entries = answer["log"].as_array().cloned().unwrap_or_default();
    if entries.is_empty() {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            t!("ide-database-no-log"),
            theme::regular(13.0),
            p.text_secondary,
        );
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().id_salt("database-log").max_rect(area.shrink2(vec2(8.0, 4.0))));
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(&mut child, |ui| {
        for entry in &entries {
            let status = entry["status"].as_str().unwrap_or_default();
            let (mark, color) = match status {
                "done" => ("✓", p.vcs_added),
                "failed" => ("✗", p.error_text),
                "cancelled" => ("■", p.warning_text),
                "skipped" => ("–", p.text_secondary),
                _ => ("…", p.text_secondary),
            };
            let time = log_time(entry);
            let amount = match (entry["rows"].as_u64(), entry["affected"].as_u64()) {
                (Some(rows), _) => t!("ide-database-log-rows", count = rows),
                (None, Some(rows)) => t!("ide-database-log-updated", count = rows),
                _ => String::new(),
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(mark).color(color).font(theme::semibold(12.5)));
                ui.add_sized(
                    vec2(70.0, 18.0),
                    egui::Label::new(egui::RichText::new(time).color(p.text_secondary).font(theme::regular(12.0))),
                );
                ui.add_sized(
                    vec2(90.0, 18.0),
                    egui::Label::new(egui::RichText::new(amount).color(p.text_secondary).font(theme::regular(12.0))),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(entry["summary"].as_str().unwrap_or_default())
                            .font(FontId::monospace(12.5))
                            .color(p.text),
                    )
                    .truncate(),
                );
            });
            if let Some(error) = entry["error"].as_str() {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(error).color(p.error_text).font(theme::regular(12.0)))
                            .wrap(),
                    );
                });
            }
        }
    });
}

fn log_time(entry: &Value) -> String {
    let ms = millis(&entry["executionMs"]) + millis(&entry["fetchMs"]);
    match entry["status"].as_str() {
        Some("done" | "failed") => t!("ide-database-log-ms", ms = ms),
        Some("cancelled") if ms > 0 => t!("ide-database-log-ms", ms = ms),
        _ => String::new(),
    }
}

fn cell(ui: &mut Ui, width: f32, text: egui::RichText) {
    ui.allocate_ui_with_layout(vec2(width, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(width);
        ui.add(egui::Label::new(text).truncate());
    });
}

fn connections(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (path, state): (&Path, &Value),
) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.editor_background);
    let run = &state["connections"];
    if run["running"] == true {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
    let targets = run["targets"].as_array().cloned().unwrap_or_default();
    let mut shown = None;
    let mut child =
        ui.new_child(UiBuilder::new().id_salt("database-connections").max_rect(area.shrink2(vec2(8.0, 4.0))));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
        for target in &targets {
            let status = target["status"].as_str().unwrap_or_default();
            let (mark, mark_color, words) = match status {
                "done" => ("✓", p.vcs_added, t!("ide-several-status-done")),
                "failed" => ("✗", p.error_text, t!("ide-several-status-failed")),
                "cancelled" => ("■", p.warning_text, t!("ide-several-status-cancelled")),
                "skipped" => ("–", p.text_secondary, t!("ide-several-status-skipped")),
                "running" => ("…", p.text, t!("ide-several-status-running")),
                _ => ("…", p.text_secondary, t!("ide-several-status-waiting")),
            };
            let color =
                target["color"].as_str().and_then(|color| super::super::database_navigator::type_color(p, color));
            let name = target["name"].as_str().unwrap_or_default().to_owned();
            let row = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(egui::RichText::new(mark).color(mark_color).font(theme::semibold(12.5)));
                let (dot, _) = ui.allocate_exact_size(vec2(8.0, 16.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, color.unwrap_or(p.text_secondary));
                cell(ui, 160.0, egui::RichText::new(&name).color(p.text).font(theme::semibold(12.5)));
                let kind = target["typeName"].as_str().unwrap_or_default();
                cell(
                    ui,
                    96.0,
                    egui::RichText::new(kind).color(color.unwrap_or(p.text_secondary)).font(theme::regular(12.0)),
                );
                cell(ui, 84.0, egui::RichText::new(&words).color(mark_color).font(theme::regular(12.0)));
                let mut parts = vec![t!(
                    "ide-several-statements",
                    executed = millis(&target["executed"]),
                    count = millis(&target["statements"])
                )];
                let rows = millis(&target["rows"]);
                if rows > 0 {
                    parts.push(t!("ide-database-header-rows", count = rows));
                }
                let updated = millis(&target["updated"]);
                if updated > 0 {
                    parts.push(t!("ide-database-header-updated", count = updated));
                }
                if status != "waiting" && status != "skipped" {
                    parts.push(t!("ide-several-time", seconds = seconds(millis(&target["elapsedMs"]))));
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(parts.join(" · ")).color(p.text_secondary).font(theme::regular(12.0)),
                    )
                    .truncate(),
                );
            });
            let first = target["results"].as_array().and_then(|results| results.first()).and_then(Value::as_u64);
            let response = ui.interact(row.response.rect, ui.scope_id().with(("several-row", &name)), Sense::click());
            if response.hovered() && first.is_some() {
                ui.painter().rect_filled(row.response.rect, 3, p.hover.gamma_multiply(0.5));
            }
            accessibility::button_with(&response, || format!("{name}: {words}"));
            if response.clicked()
                && let Some(first) = first
            {
                shown = Some(first);
            }
            if let Some(error) = target["error"].as_str() {
                ui.horizontal(|ui| {
                    ui.add_space(30.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(error).color(p.error_text).font(theme::regular(12.0)))
                            .wrap(),
                    );
                });
            }
            ui.add_space(4.0);
        }
    });
    if let Some(result) = shown {
        let tab = json!({ "kind": "result", "result": result });
        view.dispatch(registry, session, "db.console.showPanelTab", json!({ "path": path, "tab": tab }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_gives_a_time_only_to_statements_that_ran() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let entry = |status: &str| json!({ "status": status, "executionMs": 12, "fetchMs": 3 });
        assert_eq!(log_time(&entry("done")), "15 ms");
        assert_eq!(log_time(&entry("failed")), "15 ms");
        for status in ["skipped", "cancelled", "running", "pending"] {
            assert_eq!(log_time(&json!({ "status": status, "executionMs": 0, "fetchMs": 0 })), "", "{status}");
        }
    }
}
