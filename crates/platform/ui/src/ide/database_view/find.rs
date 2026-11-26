use std::time::{Duration, Instant};

use egui::{Align, Align2, Layout, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::workspaces::InnerTab;

use super::super::glyphs::glyph_search;
use super::super::{IdePalette, customization, theme, vcs};
use super::{Listed, message};
use crate::accessibility;

const BAR: f32 = 36.0;

const ROW: f32 = 26.0;

const LISTED: usize = 2000;

const POLL: Duration = Duration::from_millis(150);

const TARGETS: Duration = Duration::from_secs(2);

const KINDS: [&str; 4] = ["text", "number", "temporal", "other"];

#[derive(Clone)]
struct Known {
    key: Value,
    read: Instant,
    answer: Value,
}

pub(super) struct SearchView {
    text: String,
    connection: Option<String>,
    schema: Option<String>,
    data: bool,
    metadata: bool,
    case_sensitive: bool,
    views: bool,
    kinds: [bool; 4],
    columns: String,
    limit: u32,
    seconds: u32,
    preset: Value,
    targets: Option<Known>,
    id: Option<u64>,
    state: Option<Value>,
    read: Option<Instant>,
    selected: Option<String>,
    error: Option<String>,
    focus: bool,
}

impl Default for SearchView {
    fn default() -> Self {
        Self {
            text: String::new(),
            connection: None,
            schema: None,
            data: true,
            metadata: false,
            case_sensitive: false,
            views: false,
            kinds: [true, false, false, false],
            columns: String::new(),
            limit: 100,
            seconds: 10,
            preset: Value::Null,
            targets: None,
            id: None,
            state: None,
            read: None,
            selected: None,
            error: None,
            focus: true,
        }
    }
}

fn kind_label(kind: &str) -> String {
    match kind {
        "text" => t!("ide-dbsearch-kind-text"),
        "number" => t!("ide-dbsearch-kind-number"),
        "temporal" => t!("ide-dbsearch-kind-temporal"),
        _ => t!("ide-dbsearch-kind-other"),
    }
}

fn seconds(ms: u64) -> String {
    let tenths = ms / 100;
    format!("{}.{}", tenths / 10, tenths % 10)
}

impl SearchView {
    fn take_preset(&mut self, session: &mut Session, tab: &InnerTab) {
        if let Some((text, _)) = session.state_mut::<zen_workbench::database::find::Preset>().take() {
            self.text = text;
            self.focus = true;
        }
        if tab.data == self.preset {
            return;
        }
        self.preset = tab.data.clone();
        let data = &tab.data;
        if let Some(connection) = data["connection"].as_str() {
            self.connection = Some(connection.to_owned());
            self.schema = data["schema"].as_str().map(str::to_owned);
        }
        for (flag, key) in [
            (&mut self.data, "data"),
            (&mut self.metadata, "metadata"),
            (&mut self.case_sensitive, "caseSensitive"),
            (&mut self.views, "views"),
        ] {
            if let Some(value) = data[key].as_bool() {
                *flag = value;
            }
        }
        if let Some(kinds) = data["columnKinds"].as_array() {
            self.kinds = KINDS.map(|kind| kinds.iter().any(|known| known == kind));
        }
        if let Some(columns) = data["columns"].as_array() {
            self.columns = columns.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
        }
        if let Some(limit) = data["limitPerTable"].as_u64() {
            self.limit = u32::try_from(limit).unwrap_or(100);
        }
        if let Some(ms) = data["tableTimeoutMs"].as_u64() {
            self.seconds = u32::try_from(ms / 1000).unwrap_or(10).max(1);
        }
    }

    fn connection_or_first(&mut self, listed: &Listed) -> Option<String> {
        let known = |id: &str| listed.connections.iter().any(|connection| connection["id"] == id);
        if self.connection.as_deref().is_some_and(|id| !known(id)) && !listed.connections.is_empty() {
            self.connection = None;
        }
        if self.connection.is_none() {
            self.connection = listed
                .connections
                .iter()
                .find(|connection| connection["driver"] != "redis")
                .and_then(|connection| connection["id"].as_str())
                .map(str::to_owned);
        }
        self.connection.clone()
    }

    fn refresh_targets(&mut self, registry: &Registry, session: &mut Session, connection: &str) {
        let key = json!({ "connection": connection, "schema": self.schema, "views": self.views });
        let fresh = self.targets.as_ref().is_some_and(|known| known.key == key && known.read.elapsed() < TARGETS);
        if fresh {
            return;
        }
        let answer = registry.dispatch(session, "db.search.targets", key.clone()).unwrap_or_default();
        self.targets = Some(Known { key, read: Instant::now(), answer });
    }

    fn targets(&self) -> Value {
        self.targets.as_ref().map(|known| known.answer.clone()).unwrap_or_default()
    }

    fn running(&self) -> bool {
        self.state.as_ref().is_some_and(|state| state["search"]["status"] == "running")
    }

    fn poll(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let current = session
            .state::<zen_workbench::database::find::Searches>()
            .and_then(zen_workbench::database::find::Searches::current);
        if current.is_some() && current != self.id {
            self.id = current;
            self.state = None;
            self.read = None;
        }
        let Some(id) = self.id else { return };
        let due = self.read.is_none_or(|read| read.elapsed() >= POLL);
        if !due || (self.state.is_some() && !self.running()) {
            return;
        }
        match registry.dispatch(session, "db.search.state", json!({ "id": id, "limit": LISTED })) {
            Ok(answer) => self.state = Some(answer),
            Err(error) => {
                self.error = Some(message(error));
                self.id = None;
            }
        }
        self.read = Some(Instant::now());
        if self.running() {
            ctx.request_repaint_after(POLL);
        }
    }

    fn start(&mut self, registry: &Registry, session: &mut Session) {
        let Some(connection) = self.connection.clone() else { return };
        let kinds: Vec<&str> = KINDS.iter().zip(self.kinds).filter(|(_, on)| *on).map(|(kind, _)| *kind).collect();
        let columns: Vec<String> =
            self.columns.split(',').map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned).collect();
        let params = json!({
            "connection": connection,
            "schema": self.schema,
            "text": self.text,
            "data": self.data,
            "metadata": self.metadata,
            "caseSensitive": self.case_sensitive,
            "columnKinds": kinds,
            "columns": columns,
            "views": self.views,
            "limitPerTable": self.limit,
            "tableTimeoutMs": u64::from(self.seconds) * 1000,
        });
        match registry.dispatch(session, "db.search.start", params) {
            Ok(started) => {
                self.error = None;
                self.id = started["id"].as_u64();
                self.state = None;
                self.read = None;
                self.selected = None;
                if self.schema.is_none() {
                    self.schema = started["schema"].as_str().map(str::to_owned);
                }
            }
            Err(error) => self.error = Some(message(error)),
        }
    }

    fn cancel(&mut self, registry: &Registry, session: &mut Session) {
        if let Some(id) = self.id {
            self.error = registry.dispatch(session, "db.search.cancel", json!({ "id": id })).err().map(message);
            self.read = None;
        }
    }

    fn open(&mut self, registry: &Registry, session: &mut Session, table: &str, columns: &[String]) {
        let Some(id) = self.id else { return };
        let params = json!({ "id": id, "table": table, "columns": columns });
        self.error = registry.dispatch(session, "db.search.openResult", params).err().map(message);
    }

    fn tables(&self) -> Vec<Value> {
        self.state
            .as_ref()
            .and_then(|state| state["search"]["tables"].as_array())
            .map(|tables| tables.iter().filter(|table| found(table) || failed(table)).cloned().collect())
            .unwrap_or_default()
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (tab, listed): (&InnerTab, &Listed),
    ) {
        self.take_preset(session, tab);
        let connection = self.connection_or_first(listed);
        if let Some(connection) = &connection {
            self.refresh_targets(registry, session, connection);
        }
        self.poll(ui.ctx(), registry, session);
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.editor_background);
        let form = Rect::from_min_size(area.min, vec2(area.width(), BAR * 3.0));
        ui.painter().rect_filled(form, 0, p.island);
        ui.painter().hline(form.x_range(), form.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let first = Rect::from_min_size(form.min, vec2(form.width(), BAR));
        let mut first_ui = ui.new_child(UiBuilder::new().id_salt("database-find-first").max_rect(first));
        let mut go = self.first_row(&mut first_ui, p, (first, listed), connection.as_deref());
        let second = Rect::from_min_size(pos2(form.left(), first.bottom()), vec2(form.width(), BAR));
        let entered = self.option_rows(ui, p, second);
        if go.is_none() {
            go = entered;
        }
        if go == Some(true) {
            self.start(registry, session);
        } else if go == Some(false) {
            self.cancel(registry, session);
        }
        let mut top = form.bottom();
        let mut strips: Vec<(String, egui::Color32)> = Vec::new();
        if let Some(error) = &self.error {
            strips.push((error.clone(), p.error_text));
        }
        for warning in self.targets()["warnings"].as_array().into_iter().flatten() {
            if let Some(text) = warning["message"].as_str() {
                strips.push((text.to_owned(), p.warning_text));
            }
        }
        if self.targets()["sql"] == false {
            strips.push((t!("ide-dbsearch-not-sql"), p.warning_text));
        }
        for (text, color) in strips {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), 24.0));
            ui.painter().rect_filled(strip, 0, p.banner);
            let job = vcs::truncated(&text, theme::regular(12.0), color, strip.width() - 24.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(strip.left() + 12.0, strip.center().y - galley.size().y / 2.0), galley, color);
            top = strip.bottom();
        }
        let status = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), 28.0));
        self.status(ui, p, status);
        let list = Rect::from_min_max(pos2(area.left(), status.bottom()), area.max);
        let mut opened = None;
        self.list(ui, p, list, &mut opened);
        if let Some((table, columns)) = opened {
            self.open(registry, session, &table, &columns);
        }
        ui.advance_cursor_after_rect(area);
    }

    fn first_row(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        (bar, listed): (Rect, &Listed),
        connection: Option<&str>,
    ) -> Option<bool> {
        let field = Rect::from_min_size(
            pos2(bar.left() + 8.0, bar.top() + 5.0),
            vec2(320.0_f32.min(bar.width() * 0.4), BAR - 10.0),
        );
        ui.painter().rect_filled(field, 5, p.text_field);
        ui.painter().rect_stroke(field, 5, Stroke::new(1.0, p.text_field_border), StrokeKind::Inside);
        glyph_search(
            ui.painter(),
            Rect::from_center_size(pos2(field.left() + 12.0, field.center().y), vec2(12.0, 12.0)),
            p.text_secondary,
        );
        let text =
            Rect::from_min_max(pos2(field.left() + 24.0, field.top()), pos2(field.right() - 6.0, field.bottom()));
        let hint_text = t!("ide-dbsearch-text");
        let response = ui.put(
            text,
            egui::TextEdit::singleline(&mut self.text)
                .id_salt("database-find-text")
                .frame(egui::Frame::NONE)
                .font(theme::regular(13.0))
                .align(Align2::LEFT_CENTER)
                .hint_text(hint_text.clone())
                .desired_width(text.width()),
        );
        accessibility::labelled(&response, &hint_text);
        if std::mem::take(&mut self.focus) {
            response.request_focus();
        }
        let mut go = None;
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            go = Some(true);
        }
        let mut at = field.right() + 8.0;
        let name = connection
            .and_then(|id| {
                listed.connections.iter().find(|known| known["id"] == id).and_then(|known| known["name"].as_str())
            })
            .map_or_else(|| t!("ide-dbsearch-no-connection"), str::to_owned);
        let (chooser, next) =
            super::DatabaseView::selector(ui, p, at, (&name, connection.is_none()), "search-connection");
        at = next;
        egui::Popup::menu(&chooser).show(|ui| {
            ui.set_min_width(200.0);
            for known in &listed.connections {
                let Some(id) = known["id"].as_str() else { continue };
                let label = known["name"].as_str().unwrap_or(id);
                if ui.selectable_label(connection == Some(id), label).clicked() {
                    self.connection = Some(id.to_owned());
                    self.schema = None;
                    ui.close();
                }
            }
        });
        let targets = self.targets();
        let schema = self.schema.clone().or_else(|| targets["schema"].as_str().map(str::to_owned));
        let label = schema.clone().unwrap_or_else(|| t!("ide-dbsearch-no-schema"));
        let (schemas, next) = super::DatabaseView::selector(ui, p, at, (&label, schema.is_none()), "search-schema");
        at = next + 4.0;
        egui::Popup::menu(&schemas).show(|ui| {
            ui.set_min_width(180.0);
            let listed = targets["schemas"].as_array().cloned().unwrap_or_default();
            if listed.is_empty() {
                ui.add_enabled(false, egui::Button::new(t!("ide-dbsearch-schemas-unknown")));
            }
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for name in listed.iter().filter_map(Value::as_str) {
                    if ui.selectable_label(schema.as_deref() == Some(name), name).clicked() {
                        self.schema = Some(name.to_owned());
                        ui.close();
                    }
                }
            });
        });
        let running = self.running();
        let label = if running { t!("ide-dbsearch-stop") } else { t!("ide-dbsearch-find") };
        let button = Rect::from_min_size(pos2(at, bar.center().y - 13.0), vec2(90.0, 26.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(button).layout(Layout::left_to_right(Align::Center)));
        let can_start = !self.text.trim().is_empty() && connection.is_some();
        let clicked = if running {
            vcs::secondary_button(&mut child, p, &label).clicked()
        } else {
            child.add_enabled_ui(can_start, |ui| vcs::primary_button(ui, p, &label)).inner.clicked()
        };
        if clicked {
            go = Some(!running);
        }
        if go == Some(true) && !can_start {
            go = None;
        }
        go
    }

    fn option_rows(&mut self, ui: &mut Ui, p: &IdePalette, rows: Rect) -> Option<bool> {
        let font = theme::regular(12.5);
        let label = |text: String| egui::RichText::new(text).font(font.clone()).color(p.text);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("database-find-options")
                .max_rect(rows.shrink2(vec2(10.0, 4.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 10.0;
        child.checkbox(&mut self.data, label(t!("ide-dbsearch-data")));
        child.checkbox(&mut self.metadata, label(t!("ide-dbsearch-names")));
        child.checkbox(&mut self.case_sensitive, label(t!("ide-dbsearch-match-case")));
        child.checkbox(&mut self.views, label(t!("ide-dbsearch-views")));
        child.separator();
        let columns_of = t!("ide-dbsearch-columns-of");
        let width_of = |text: &str| child.painter().layout_no_wrap(text.to_owned(), font.clone(), p.text).size().x;
        let kinds_width: f32 =
            width_of(&columns_of) + KINDS.iter().map(|kind| width_of(&kind_label(kind)) + 34.0).sum::<f32>();
        if child.cursor().left() + kinds_width > rows.right() - 10.0 {
            let chosen: Vec<String> =
                self.kinds.iter().zip(KINDS).filter(|(on, _)| **on).map(|(_, kind)| kind_label(kind)).collect();
            let shown = format!("{columns_of} {}", chosen.join(", "));
            let at = child.cursor().left();
            let (chooser, _) =
                super::DatabaseView::selector(&mut child, p, at, (&shown, chosen.is_empty()), "search-kinds");
            let kinds = &mut self.kinds;
            egui::Popup::menu(&chooser).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(160.0);
                for (on, kind) in kinds.iter_mut().zip(KINDS) {
                    ui.checkbox(on, kind_label(kind));
                }
            });
        } else {
            child.label(egui::RichText::new(columns_of).font(font.clone()).color(p.text_secondary));
            for (on, kind) in self.kinds.iter_mut().zip(KINDS) {
                child.checkbox(on, label(kind_label(kind)));
            }
        }
        let third = Rect::from_min_size(pos2(rows.left(), rows.bottom()), vec2(rows.width(), BAR));
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("database-find-limits")
                .max_rect(third.shrink2(vec2(10.0, 4.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 8.0;
        let measure = |text: String| child.painter().layout_no_wrap(text, font.clone(), p.text).size().x;
        let others = measure(t!("ide-dbsearch-only-columns"))
            + measure(t!("ide-dbsearch-rows-per-table"))
            + measure(t!("ide-dbsearch-seconds-per-table"))
            + 2.0 * 60.0
            + 16.0
            + 6.0 * 8.0;
        let field_width = (child.max_rect().width() - others).clamp(80.0, 200.0);
        child.label(egui::RichText::new(t!("ide-dbsearch-only-columns")).font(font.clone()).color(p.text_secondary));
        let hint = t!("ide-dbsearch-only-columns-hint");
        let response = child.add(
            egui::TextEdit::singleline(&mut self.columns)
                .id_salt("database-find-columns")
                .font(font.clone())
                .hint_text(hint.clone())
                .desired_width(field_width),
        );
        accessibility::labelled(&response, &t!("ide-dbsearch-only-columns"));
        let outline = |child: &Ui, rect: Rect| {
            child.painter().rect_stroke(rect.expand(3.0), 4, Stroke::new(1.0, p.text_field_border), StrokeKind::Inside);
        };
        outline(&child, response.rect);
        let go = (response.lost_focus() && child.input(|input| input.key_pressed(egui::Key::Enter))).then_some(true);
        child.add_space(8.0);
        child.label(egui::RichText::new(t!("ide-dbsearch-rows-per-table")).font(font.clone()).color(p.text_secondary));
        let limit = child.add(egui::DragValue::new(&mut self.limit).range(1..=10_000));
        outline(&child, limit.rect);
        accessibility::labelled(&limit, &t!("ide-dbsearch-rows-per-table"));
        child.add_space(8.0);
        child.label(egui::RichText::new(t!("ide-dbsearch-seconds-per-table")).font(font).color(p.text_secondary));
        let seconds = child.add(egui::DragValue::new(&mut self.seconds).range(1..=600));
        outline(&child, seconds.rect);
        accessibility::labelled(&seconds, &t!("ide-dbsearch-seconds-per-table"));
        go
    }

    fn status(&self, ui: &Ui, p: &IdePalette, rect: Rect) {
        let Some(state) = self.state.as_ref().map(|state| &state["search"]) else {
            let text = if self.id.is_some() { t!("ide-dbsearch-starting") } else { t!("ide-dbsearch-intro") };
            draw_line(ui, rect, &text, p.text_secondary);
            return;
        };
        let total = self.state.as_ref().and_then(|state| state["tables"].as_u64()).unwrap_or(0);
        let ended = state["ended"].as_u64().unwrap_or(0);
        let found = state["found"].as_u64().unwrap_or(0);
        let rows = state["rows"].as_u64().unwrap_or(0);
        let elapsed = seconds(state["elapsedMs"].as_u64().unwrap_or(0));
        let (text, color) = match state["status"].as_str().unwrap_or_default() {
            "running" if state["listed"] != true => {
                (t!("ide-dbsearch-listing", schema = state["schema"].as_str().unwrap_or_default()), p.text_secondary)
            }
            "running" => (
                t!(
                    "ide-dbsearch-running",
                    table = state["current"].as_str().unwrap_or_default(),
                    ended = ended,
                    total = total,
                    found = found
                ),
                p.text_secondary,
            ),
            "failed" => (
                t!("ide-dbsearch-failed", reason = state["error"]["detail"].as_str().unwrap_or_default()),
                p.error_text,
            ),
            "cancelled" => (t!("ide-dbsearch-cancelled", ended = ended, total = total, found = found), p.warning_text),
            _ if found == 0 => (t!("ide-dbsearch-nothing", total = total, seconds = elapsed), p.text_secondary),
            _ => (t!("ide-dbsearch-done", rows = rows, found = found, total = total, seconds = elapsed), p.text),
        };
        draw_line(ui, rect, &text, color);
        if state["status"] == "running" && total > 0 {
            let track = Rect::from_min_size(pos2(rect.left(), rect.bottom() - 2.0), vec2(rect.width(), 2.0));
            ui.painter().rect_filled(track, 0, p.frame);
            let share = (ended as f32 / total as f32).clamp(0.0, 1.0);
            ui.painter().rect_filled(Rect::from_min_size(track.min, vec2(track.width() * share, 2.0)), 0, p.accent);
        }
    }

    fn list(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect, opened: &mut Option<(String, Vec<String>)>) {
        let tables = self.tables();
        if tables.is_empty() {
            return;
        }
        let mut child = ui.new_child(UiBuilder::new().id_salt("database-find-list").max_rect(rect));
        child.spacing_mut().item_spacing.y = 0.0;
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            &mut child,
            ROW,
            tables.len(),
            |ui, range| {
                for index in range {
                    let table = &tables[index];
                    let name = table["table"].as_str().unwrap_or_default().to_owned();
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    if self.selected.as_deref() == Some(name.as_str()) {
                        ui.painter().rect_filled(row, 0, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(row, 0, p.hover);
                    }
                    let summary = draw_row(ui, p, row, table);
                    accessibility::name_with(&response, egui::Role::ListItem, || format!("{name} {summary}"));
                    if response.clicked() {
                        chosen = Some(name.clone());
                    }
                    if response.double_clicked() {
                        *opened = Some((name.clone(), Vec::new()));
                    }
                    let columns: Vec<String> = table["columns"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|column| column["name"].as_str().map(str::to_owned))
                        .collect();
                    response.context_menu(|ui| {
                        ui.set_min_width(220.0);
                        let shown = |slot: &str| slot != customization::SEARCH_COLUMNS || columns.len() > 1;
                        customization::popup(ui, customization::DATABASE_SEARCH_POPUP, shown, |ui, slot| {
                            if slot != customization::SEARCH_COLUMNS {
                                if ui.button(t!("ide-dbsearch-open-rows")).clicked() {
                                    *opened = Some((name.clone(), Vec::new()));
                                    ui.close();
                                }
                                return;
                            }
                            for column in &columns {
                                if ui.button(t!("ide-dbsearch-open-column", column = column.as_str())).clicked() {
                                    *opened = Some((name.clone(), vec![column.clone()]));
                                    ui.close();
                                }
                            }
                        });
                    });
                }
            },
        );
        if let Some(name) = chosen {
            self.selected = Some(name);
        }
        let enter =
            ui.input(|input| input.key_pressed(egui::Key::Enter)) && ui.memory(|memory| memory.focused().is_none());
        if enter && let Some(name) = self.selected.clone() {
            *opened = Some((name, Vec::new()));
        }
    }
}

fn found(table: &Value) -> bool {
    table["rows"].as_u64().unwrap_or(0) > 0
        || table["nameMatches"] == true
        || table["namedColumns"].as_array().is_some_and(|names| !names.is_empty())
}

fn failed(table: &Value) -> bool {
    matches!(table["status"].as_str(), Some("timeout" | "failed"))
}

fn draw_line(ui: &Ui, rect: Rect, text: &str, color: egui::Color32) {
    let job = vcs::truncated(text, theme::regular(12.5), color, rect.width() - 24.0);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0), galley, color);
}

fn kind_name(kind: &str) -> String {
    match kind {
        "view" => t!("ide-dbsearch-view"),
        "materializedView" => t!("ide-dbsearch-materialized-view"),
        _ => t!("ide-dbsearch-table"),
    }
}

fn draw_row(ui: &Ui, p: &IdePalette, row: Rect, table: &Value) -> String {
    let painter = ui.painter();
    let font = theme::regular(12.5);
    let status = table["status"].as_str().unwrap_or_default();
    let mark = match status {
        "timeout" => p.warning_text,
        "failed" => p.error_text,
        _ => p.vcs_added,
    };
    painter.circle_filled(pos2(row.left() + 14.0, row.center().y), 3.5, mark);
    let name = table["table"].as_str().unwrap_or_default();
    let job = vcs::truncated(name, theme::regular(13.0), p.text, 220.0);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let name_width = galley.size().x;
    painter.galley(pos2(row.left() + 26.0, row.center().y - galley.size().y / 2.0), galley, p.text);
    let kind = kind_name(table["kind"].as_str().unwrap_or_default());
    painter.text(
        pos2(row.left() + 32.0 + name_width, row.center().y),
        Align2::LEFT_CENTER,
        &kind,
        font.clone(),
        p.text_secondary,
    );
    let rows = table["rows"].as_u64().unwrap_or(0);
    let mut figures = if rows == 0 {
        String::new()
    } else if table["truncated"] == true {
        t!("ide-dbsearch-rows-more", rows = rows)
    } else {
        t!("ide-dbsearch-rows", rows = rows)
    };
    let (detail, detail_color) = match status {
        "timeout" => {
            (t!("ide-dbsearch-timeout", seconds = seconds(table["elapsedMs"].as_u64().unwrap_or(0))), p.warning_text)
        }
        "failed" => (
            t!("ide-dbsearch-table-failed", reason = table["error"]["detail"].as_str().unwrap_or_default()),
            p.error_text,
        ),
        _ => {
            let mut parts: Vec<String> = table["columns"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|column| {
                    format!(
                        "{} ({})",
                        column["name"].as_str().unwrap_or_default(),
                        column["rows"].as_u64().unwrap_or(0)
                    )
                })
                .collect();
            if table["nameMatches"] == true {
                parts.push(t!("ide-dbsearch-name-matches"));
            }
            let named: Vec<&str> =
                table["namedColumns"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !named.is_empty() {
                parts.push(t!("ide-dbsearch-named-columns", columns = named.join(", ")));
            }
            (parts.join(" · "), p.text_secondary)
        }
    };
    if status == "timeout" && rows > 0 {
        figures = t!("ide-dbsearch-rows-more", rows = rows);
    }
    let figures_right = row.right() - 12.0;
    let figures_width = painter.layout_no_wrap(figures.clone(), font.clone(), p.text).size().x;
    painter.text(pos2(figures_right, row.center().y), Align2::RIGHT_CENTER, &figures, font.clone(), p.text);
    let detail_left = row.left() + 300.0;
    let width = (figures_right - figures_width - 16.0 - detail_left).max(40.0);
    let job = vcs::truncated(&detail, font, detail_color, width);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    painter.galley(pos2(detail_left, row.center().y - galley.size().y / 2.0), galley, detail_color);
    format!("{kind} {figures} {detail}")
}

#[cfg(test)]
mod tests;
