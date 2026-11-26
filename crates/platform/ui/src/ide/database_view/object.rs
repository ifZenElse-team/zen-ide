use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::Duration;

use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::workspaces::InnerTab;

use super::super::data_grid::{self, GridCell, GridColumn, GridSource, GridState};
use super::super::database_navigator::type_color;
use super::super::{IdePalette, theme, vcs};
use super::DatabaseView;
use super::code::{self, Lang};
use crate::accessibility;

const PAGES: f32 = 32.0;
const FIELD: f32 = 22.0;
const SECTIONS: f32 = 190.0;
const TOGGLES: f32 = 34.0;

type DdlKey = (String, String, u64);

#[derive(Default)]
pub(super) struct ObjectView {
    properties: Option<((String, String, u64), Value)>,
    ddl: Option<(DdlKey, Result<Value, String>)>,
    grids: HashMap<String, GridState>,
    read: HashSet<String>,
    message: Option<(String, String)>,
}

struct TextSource {
    columns: Vec<GridColumn>,
    rows: Vec<Vec<String>>,
    revision: u64,
}

impl GridSource for TextSource {
    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn row_count(&self) -> u64 {
        self.rows.len() as u64
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn cells(&self, rows: Range<u64>, columns: Range<usize>) -> Vec<Vec<GridCell>> {
        let rows = usize::try_from(rows.start).unwrap_or(usize::MAX)..usize::try_from(rows.end).unwrap_or(usize::MAX);
        self.rows
            .get(rows.start.min(self.rows.len())..rows.end.min(self.rows.len()))
            .unwrap_or_default()
            .iter()
            .map(|row| {
                columns
                    .clone()
                    .map(|column| {
                        let text = row.get(column).cloned().unwrap_or_default();
                        let line = text.lines().next().unwrap_or_default().to_owned();
                        GridCell { cut: line.len() < text.len(), text: line, ..GridCell::default() }
                    })
                    .collect()
            })
            .collect()
    }

    fn full_text(&self, row: u64, column: usize) -> Option<String> {
        self.rows.get(usize::try_from(row).ok()?)?.get(column).cloned()
    }
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().map(|item| item.as_str().unwrap_or_default().to_owned()).collect()
}

fn page_label(page: &str) -> String {
    match page {
        "data" => t!("ide-database-object-page-data"),
        "properties" => t!("ide-database-object-page-properties"),
        "ddl" => t!("ide-database-object-page-ddl"),
        _ => t!("ide-database-object-page-diagram"),
    }
}

pub(super) fn centered(ui: &Ui, p: &IdePalette, area: Rect, text: &str) {
    ui.painter().text(area.center(), Align2::CENTER_CENTER, text, theme::regular(13.0), p.text_secondary);
}

fn structure_revision(session: &Session) -> u64 {
    session
        .state::<zen_workbench::database::structure::Structure>()
        .map_or(0, zen_workbench::database::structure::Structure::revision)
}

pub(super) fn code_view(ui: &mut Ui, p: &IdePalette, area: Rect, text: &str, lang: Lang, salt: &str, name: &str) {
    ui.painter().rect_filled(area, 0, p.editor_background);
    let mut child = ui.new_child(UiBuilder::new().id_salt(salt).max_rect(area.shrink2(vec2(6.0, 4.0))));
    let palette = *p;
    let font = FontId::monospace(12.5);
    egui::ScrollArea::both().auto_shrink([false, false]).show(&mut child, |ui| {
        let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, _width: f32| {
            let mut job = code::job(buffer.as_str(), lang, &palette, &font);
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let mut shown = text;
        let response = ui.add(
            egui::TextEdit::multiline(&mut shown)
                .id_salt(salt)
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter),
        );
        accessibility::labelled(&response, name);
    });
}

pub(super) fn button(ui: &mut Ui, p: &IdePalette, at: Rect, label: &str, salt: &str) -> egui::Response {
    let response = ui.interact(at, ui.scope_id().with(("database-object-button", salt)), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(at, 4, p.hover);
    }
    ui.painter().rect_stroke(at, 4, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
    ui.painter().text(at.center(), Align2::CENTER_CENTER, label, theme::regular(12.0), p.text);
    accessibility::button_with(&response, || label.to_owned());
    response
}

pub(super) fn width_of(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font.clone(), ui.visuals().text_color()).size().x)
}

impl DatabaseView {
    fn object_dispatch(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        tab: &str,
        command: &str,
        params: Value,
    ) -> Option<Value> {
        match registry.dispatch(session, command, params) {
            Ok(answer) => {
                self.object.message = None;
                Some(answer)
            }
            Err(error) => {
                self.object.message = Some((tab.to_owned(), super::message(error)));
                None
            }
        }
    }

    pub(super) fn object_tab(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        tab: &InnerTab,
    ) {
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.island);
        let state = match registry.dispatch(session, "db.object.state", json!({ "tab": tab.id })) {
            Ok(state) => state,
            Err(error) => {
                centered(ui, p, area, &super::message(error));
                return;
            }
        };
        let row = Rect::from_min_size(area.min, vec2(area.width(), PAGES));
        let page = self.page_row(ui, p, registry, session, &state, row);
        let body = Rect::from_min_max(pos2(area.left(), row.bottom()), area.max);
        let mut child =
            ui.new_child(UiBuilder::new().id_salt(("database-object", tab.id.as_str(), page.as_str())).max_rect(body));
        if state["kind"].is_null() {
            centered(&child, p, body, &t!("ide-database-object-gone"));
            return;
        }
        match page.as_str() {
            "data" => self.data_page(&mut child, p, registry, session, &state),
            "properties" => self.properties_page(&mut child, p, registry, session, &state),
            "ddl" => self.ddl_page(&mut child, p, registry, session, &state),
            _ => {
                let tab = tab.id.clone();
                self.diagram_body(&mut child, p, registry, session, &tab);
            }
        }
        self.structure_dialogs(ui, p, registry, session);
        if let Some((known, message)) = &self.object.message
            && *known == tab.id
        {
            ui.painter().text(
                pos2(area.right() - 10.0, row.center().y),
                Align2::RIGHT_CENTER,
                message,
                theme::regular(12.0),
                p.error_text,
            );
        }
    }

    fn page_row(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        state: &Value,
        row: Rect,
    ) -> String {
        let tab = state["tab"].as_str().unwrap_or_default().to_owned();
        let current = state["page"].as_str().unwrap_or("properties").to_owned();
        let font = theme::regular(12.5);
        let mut x = row.left() + 8.0;
        let mut chosen = None;
        for page in strings(&state["pages"]) {
            let label = page_label(&page);
            let width = width_of(ui, &label, &font) + 22.0;
            let rect = Rect::from_min_size(pos2(x, row.top() + 4.0), vec2(width, row.height() - 6.0));
            x = rect.right() + 2.0;
            let selected = page == current;
            let response =
                ui.interact(rect, ui.scope_id().with(("database-object-page", page.as_str())), Sense::click());
            if selected {
                ui.painter().rect_filled(rect, 4, p.active_tab);
                ui.painter().hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(2.0, p.active_tab_border));
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                &label,
                font.clone(),
                if selected { p.text } else { p.text_secondary },
            );
            accessibility::item(&response, egui::Role::Tab, selected, &label);
            if response.clicked() && !selected {
                chosen = Some(page);
            }
        }
        ui.painter().hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let connection = state["connectionName"].as_str().unwrap_or_default();
        let qualified = state["qualified"].as_str().unwrap_or_default();
        let detail = format!("{qualified} · {connection}");
        let job = vcs::truncated(&detail, theme::regular(12.0), p.text_secondary, (row.right() - x - 40.0).max(40.0));
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let left = row.right() - 12.0 - galley.size().x;
        if self.object.message.is_none() {
            ui.painter().galley(pos2(left, row.center().y - galley.size().y / 2.0), galley, p.text_secondary);
            if let Some(color) = state["color"].as_str().and_then(|color| type_color(p, color)) {
                ui.painter().circle_filled(pos2(left - 9.0, row.center().y), 4.0, color);
            }
        }
        match chosen {
            Some(page) => {
                self.object_dispatch(
                    registry,
                    session,
                    &tab,
                    "db.object.showPage",
                    json!({ "tab": tab, "page": page }),
                );
                page
            }
            None => current,
        }
    }

    fn data_page(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, state: &Value) {
        let area = ui.max_rect();
        let tab = state["tab"].as_str().unwrap_or_default().to_owned();
        let data = &state["data"];
        if let Some(result) = data["result"].as_u64() {
            super::results::result_body(self, ui, p, registry, session, result);
            return;
        }
        if data["reading"] == true {
            centered(ui, p, area, &t!("ide-database-object-reading"));
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        }
        let connected = matches!(state["state"].as_str(), Some("connected" | "closed" | "connecting"));
        if let Some(error) = data["error"].as_str() {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(Rect::from_center_size(area.center(), vec2(area.width().min(560.0), 120.0)))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            child.add(
                egui::Label::new(egui::RichText::new(error).font(theme::regular(12.5)).color(p.error_text)).wrap(),
            );
            child.add_space(10.0);
            if vcs::secondary_button(&mut child, p, &t!("ide-database-object-retry")).clicked() {
                self.object_dispatch(registry, session, &tab, "db.object.readData", json!({ "tab": tab }));
            }
            return;
        }
        if connected && self.object.read.insert(tab.clone()) {
            self.object_dispatch(registry, session, &tab, "db.object.readData", json!({ "tab": tab }));
            ui.ctx().request_repaint();
            return;
        }
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_center_size(area.center(), vec2(area.width().min(520.0), 110.0)))
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        let statement = data["statement"].as_str().unwrap_or_default();
        child.label(egui::RichText::new(t!("ide-database-object-not-read")).font(theme::semibold(13.5)).color(p.text));
        child.add_space(4.0);
        child.label(egui::RichText::new(statement).font(FontId::monospace(12.0)).color(p.text_secondary));
        child.add_space(10.0);
        if vcs::primary_button(&mut child, p, &t!("ide-database-object-read-data")).clicked() {
            self.object.read.insert(tab.clone());
            self.object_dispatch(registry, session, &tab, "db.object.readData", json!({ "tab": tab }));
        }
    }

    fn properties_page(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        state: &Value,
    ) {
        let area = ui.max_rect();
        let tab = state["tab"].as_str().unwrap_or_default().to_owned();
        let section = state["section"].as_str().unwrap_or_default().to_owned();
        let key = (tab.clone(), section.clone(), structure_revision(session));
        if self.object.properties.as_ref().is_none_or(|(known, _)| *known != key) {
            let answer = registry.dispatch(session, "db.object.properties", json!({ "tab": tab })).unwrap_or_default();
            let key = (tab.clone(), section, structure_revision(session));
            self.object.properties = Some((key, answer));
        }
        let Some((_, properties)) = self.object.properties.clone() else { return };
        let alter = self.structure_state(ui.ctx(), registry, session, &tab);
        let top = self.structure_height(&tab, &alter);
        if top > 0.0 {
            let shown = properties["section"].as_str().unwrap_or_default();
            let selected = self
                .object
                .grids
                .get(&format!("{tab}/{shown}"))
                .and_then(GridState::selection)
                .and_then(|selection| properties["rows"].get(usize::try_from(selection.head.0).ok()?).cloned());
            let bar = Rect::from_min_size(area.min, vec2(area.width(), top));
            self.structure_bar(ui, p, registry, session, (state, &properties, &alter, selected), bar);
        }
        let area = Rect::from_min_max(pos2(area.left(), area.top() + top), area.max);
        let fields: Vec<(String, String)> = properties["header"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|field| {
                (
                    field["label"].as_str().unwrap_or_default().to_owned(),
                    field["value"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let rows = fields.len().div_ceil(2);
        let header = Rect::from_min_size(area.min, vec2(area.width(), rows as f32 * FIELD + 16.0));
        let half = (area.width() - 24.0) / 2.0;
        let label_width = 110.0;
        for (index, (label, value)) in fields.iter().enumerate() {
            let x = area.left() + 12.0 + (index % 2) as f32 * half;
            let y = header.top() + 8.0 + (index / 2) as f32 * FIELD + FIELD / 2.0;
            ui.painter().text(pos2(x, y), Align2::LEFT_CENTER, label, theme::regular(12.5), p.text_secondary);
            let shown = if value.is_empty() { "—" } else { value.as_str() };
            let job = vcs::truncated(shown, theme::regular(12.5), p.text, half - label_width - 12.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(x + label_width, y - galley.size().y / 2.0), galley, p.text);
            let field = Rect::from_min_size(pos2(x, y - FIELD / 2.0), vec2(half, FIELD));
            accessibility::label(ui, ui.scope_id().with(("database-object-field", index)), field, || {
                format!("{label}: {value}")
            });
        }
        ui.painter().hline(area.x_range(), header.bottom(), Stroke::new(1.0, p.frame));
        let lower = Rect::from_min_max(pos2(area.left(), header.bottom()), area.max);
        let list = Rect::from_min_max(lower.min, pos2(lower.left() + SECTIONS, lower.bottom()));
        ui.painter().vline(list.right(), list.y_range(), Stroke::new(1.0, p.frame));
        let current = properties["section"].as_str().unwrap_or_default().to_owned();
        let mut chosen = None;
        for (index, section) in properties["sections"].as_array().into_iter().flatten().enumerate() {
            let id = section["id"].as_str().unwrap_or_default();
            let title = section["title"].as_str().unwrap_or_default();
            let rect = Rect::from_min_size(
                pos2(list.left() + 4.0, list.top() + 6.0 + index as f32 * 26.0),
                vec2(SECTIONS - 8.0, 24.0),
            );
            let selected = id == current;
            let response = ui.interact(rect, ui.scope_id().with(("database-object-section", id)), Sense::click());
            if selected {
                ui.painter().rect_filled(rect, 4, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            ui.painter().text(
                pos2(rect.left() + 10.0, rect.center().y),
                Align2::LEFT_CENTER,
                title,
                theme::regular(12.5),
                p.text,
            );
            if let Some(count) = section["count"].as_u64() {
                ui.painter().text(
                    pos2(rect.right() - 8.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    count.to_string(),
                    theme::regular(12.0),
                    p.text_secondary,
                );
            }
            accessibility::item(&response, egui::Role::Tab, selected, title);
            if response.clicked() && !selected {
                chosen = Some(id.to_owned());
            }
        }
        let content = Rect::from_min_max(pos2(list.right() + 1.0, lower.top()), lower.max);
        if properties["loading"] == true {
            let connected = matches!(state["state"].as_str(), Some("connected" | "closed"));
            let text = if connected {
                t!("ide-database-object-details-reading")
            } else {
                t!("ide-database-object-details-connect")
            };
            centered(ui, p, content, &text);
            if connected {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
        } else if let Some(text) = properties["text"].as_str() {
            code_view(
                ui,
                p,
                content,
                text,
                Lang::Sql,
                &format!("database-object-source-{tab}"),
                &t!("ide-database-object-page-properties"),
            );
        } else {
            let mut table = content;
            if let Some(note) = properties["note"].as_str() {
                ui.painter().text(
                    pos2(content.left() + 10.0, content.top() + 14.0),
                    Align2::LEFT_CENTER,
                    note,
                    theme::regular(12.5),
                    p.text,
                );
                table.min.y += 28.0;
            }
            let rows: Vec<Vec<String>> = properties["rows"].as_array().into_iter().flatten().map(strings).collect();
            if rows.is_empty() {
                centered(ui, p, table, &t!("ide-database-object-section-empty"));
            } else {
                let columns = strings(&properties["columns"])
                    .into_iter()
                    .map(|name| GridColumn { name, ..GridColumn::default() })
                    .collect();
                let revision = key_hash(&(&tab, &current, &rows));
                let source = TextSource { columns, rows, revision };
                let grid = self.object.grids.entry(format!("{tab}/{current}")).or_default();
                let grid_id = ui.make_persistent_id(("database-object-grid", tab.as_str(), current.as_str()));
                let mut child = ui.new_child(
                    UiBuilder::new().id_salt(("database-object-grid", tab.as_str(), current.as_str())).max_rect(table),
                );
                grid.fit_last();
                let outcome = data_grid::show(&mut child, p, &source, grid, grid_id);
                self.results.grid_ids.push(grid_id);
                if let Some((rows, columns, header)) = outcome.copy {
                    ui.ctx().copy_text(copied(&source, rows, columns, header));
                }
            }
        }
        if let Some(section) = chosen {
            self.object_dispatch(
                registry,
                session,
                &tab,
                "db.object.showSection",
                json!({ "tab": tab, "section": section }),
            );
        }
    }

    fn ddl_page(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, state: &Value) {
        let area = ui.max_rect();
        let tab = state["tab"].as_str().unwrap_or_default().to_owned();
        let options = state["ddl"].clone();
        let bar = Rect::from_min_size(area.min, vec2(area.width(), TOGGLES));
        let toggles = [
            ("nested", t!("ide-database-ddl-nested")),
            ("permissions", t!("ide-database-ddl-permissions")),
            ("separateForeignKeys", t!("ide-database-ddl-separate-foreign-keys")),
            ("separateConstraintIndexes", t!("ide-database-ddl-separate-constraint-indexes")),
            ("comments", t!("ide-database-ddl-comments")),
            ("partitions", t!("ide-database-ddl-partitions")),
        ];
        let copy_label = t!("ide-database-ddl-copy");
        let console_label = t!("ide-database-ddl-open-console");
        let font = theme::regular(12.0);
        let console_width = width_of(ui, &console_label, &font) + 20.0;
        let copy_width = width_of(ui, &copy_label, &font) + 20.0;
        let console = Rect::from_min_size(
            pos2(bar.right() - 8.0 - console_width, bar.top() + 5.0),
            vec2(console_width, bar.height() - 10.0),
        );
        let copy = Rect::from_min_size(
            pos2(console.left() - 6.0 - copy_width, console.top()),
            vec2(copy_width, console.height()),
        );
        let mut changed = None;
        let mut x = bar.left() + 10.0;
        for (field, label) in &toggles {
            let on = options[*field] == true;
            let width = 22.0 + width_of(ui, label, &font) + 14.0;
            if x + width > copy.left() - 8.0 {
                break;
            }
            let rect = Rect::from_min_size(pos2(x, bar.top() + 6.0), vec2(width, bar.height() - 12.0));
            x = rect.right() + 4.0;
            let response = ui.interact(rect, ui.scope_id().with(("database-ddl-toggle", *field)), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            let mark = Rect::from_center_size(pos2(rect.left() + 11.0, rect.center().y), vec2(14.0, 14.0));
            super::super::commit_window::checkbox(ui.painter(), mark, Some(on), p);
            ui.painter().text(
                pos2(rect.left() + 24.0, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                font.clone(),
                p.text,
            );
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, label.as_str()));
            if response.clicked() {
                changed = Some((*field, !on));
            }
        }
        let key: DdlKey = (tab.clone(), options.to_string(), structure_revision(session));
        if self.object.ddl.as_ref().is_none_or(|(known, _)| *known != key) {
            let mut params = options.clone();
            params["connection"] = state["connection"].clone();
            params["path"] = state["path"].clone();
            let answer = registry.dispatch(session, "db.object.ddl", params).map_err(super::message);
            self.object.ddl = Some((key, answer));
        }
        let text = match self.object.ddl.as_ref().map(|(_, answer)| answer) {
            Some(Ok(answer)) => answer["ddl"].as_str().map(str::to_owned),
            _ => None,
        };
        if button(ui, p, copy, &copy_label, "copy").clicked()
            && let Some(text) = &text
        {
            ui.ctx().copy_text(text.clone());
        }
        if button(ui, p, console, &console_label, "console").clicked()
            && let Some(text) = &text
        {
            let params = json!({ "connection": state["connection"], "text": text });
            self.object_dispatch(registry, session, &tab, "db.console.new", params);
        }
        ui.painter().hline(area.x_range(), bar.bottom(), Stroke::new(1.0, p.frame));
        let content = Rect::from_min_max(pos2(area.left(), bar.bottom() + 1.0), area.max);
        match (self.object.ddl.as_ref().map(|(_, answer)| answer), text) {
            (_, Some(text)) => code_view(
                ui,
                p,
                content,
                &text,
                Lang::Sql,
                &format!("database-object-ddl-{tab}"),
                &t!("ide-database-object-page-ddl"),
            ),
            (Some(Err(error)), None) => centered(ui, p, content, error),
            _ => {
                let connected = matches!(state["state"].as_str(), Some("connected" | "closed"));
                let text = if connected {
                    t!("ide-database-object-details-reading")
                } else {
                    t!("ide-database-object-details-connect")
                };
                centered(ui, p, content, &text);
                if connected {
                    ui.ctx().request_repaint_after(Duration::from_millis(200));
                }
            }
        }
        if let Some((field, on)) = changed {
            let mut params = json!({ "tab": tab });
            params[field] = json!(on);
            self.object_dispatch(registry, session, &tab, "db.object.ddlOptions", params);
        }
    }
}

fn key_hash(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn copied(source: &TextSource, rows: Range<u64>, columns: Range<usize>, header: bool) -> String {
    let mut lines = Vec::new();
    let end = columns.end.min(source.columns.len());
    let columns = columns.start.min(end)..end;
    if header {
        lines.push(
            source.columns[columns.clone()].iter().map(|column| column.name.clone()).collect::<Vec<_>>().join("\t"),
        );
    }
    let end = usize::try_from(rows.end).unwrap_or(usize::MAX).min(source.rows.len());
    let rows = usize::try_from(rows.start).unwrap_or(usize::MAX).min(end)..end;
    for row in &source.rows[rows] {
        lines.push(row.get(columns.clone()).unwrap_or_default().join("\t"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
