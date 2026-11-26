use std::collections::HashMap;
use std::ops::Range;

use egui::{Align2, FontId, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::results::CellText;
use zen_i18n::t;
use zen_workbench::workspaces::InnerTab;

use super::super::data_grid::{self, GridCell, GridColumn, GridSource, GridState};
use super::super::{IdePalette, theme};
use crate::accessibility;

const HEADER: f32 = 34.0;
const FOOTER: f32 = 24.0;
const CELL_CHARS: usize = 200;

#[derive(Default)]
struct KeyTab {
    operation: Option<u64>,
    answer: Option<Value>,
    error: Option<String>,
    columns: Vec<GridColumn>,
    rows: Vec<Vec<GridCell>>,
    text: Option<String>,
    revision: u64,
    grid: GridState,
    ttl: String,
}

struct KeySource<'a>(&'a KeyTab);

impl GridSource for KeySource<'_> {
    fn columns(&self) -> &[GridColumn] {
        &self.0.columns
    }

    fn row_count(&self) -> u64 {
        self.0.rows.len() as u64
    }

    fn revision(&self) -> u64 {
        self.0.revision
    }

    fn cells(&self, rows: Range<u64>, columns: Range<usize>) -> Vec<Vec<GridCell>> {
        let rows = usize::try_from(rows.start).unwrap_or(usize::MAX)..usize::try_from(rows.end).unwrap_or(usize::MAX);
        self.0
            .rows
            .get(rows.start.min(self.0.rows.len())..rows.end.min(self.0.rows.len()))
            .unwrap_or_default()
            .iter()
            .map(|row| row.get(columns.clone()).unwrap_or_default().to_vec())
            .collect()
    }
}

#[derive(Default)]
pub(in super::super) struct KeyViews {
    tabs: HashMap<String, KeyTab>,
}

fn grid_cell(value: &zen_db::value::Value) -> GridCell {
    let cell = CellText::of(value, CELL_CHARS);
    GridCell {
        text: cell.text,
        null: cell.null,
        number: cell.number,
        boolean: cell.boolean,
        cut: cell.cut,
        changed: false,
        default: false,
    }
}

fn pretty(value: &zen_db::value::Value) -> String {
    match value {
        zen_db::value::Value::Json(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|json| serde_json::to_string_pretty(&json).ok())
            .unwrap_or_else(|| text.clone()),
        other => other.display().unwrap_or_default(),
    }
}

pub(super) fn ttl_text(millis: i64) -> String {
    let seconds = millis.max(0) / 1000;
    let (days, hours, minutes, rest) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60, seconds % 60);
    if days > 0 {
        t!("ide-dbkey-ttl-days", days = days, hours = hours)
    } else if hours > 0 {
        t!("ide-dbkey-ttl-hours", hours = hours, minutes = minutes)
    } else if minutes > 0 {
        t!("ide-dbkey-ttl-minutes", minutes = minutes, seconds = rest)
    } else {
        t!("ide-dbkey-ttl-seconds", seconds = rest)
    }
}

pub(super) fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{size:.1} {}", UNITS[unit]) }
}

impl KeyTab {
    fn load(&mut self, registry: &Registry, session: &mut Session, tab: &InnerTab, seconds: Option<Option<u64>>) {
        let mut params = json!({ "connection": tab.data["connection"], "path": tab.data["path"] });
        let command = match seconds {
            Some(seconds) => {
                params["seconds"] = seconds.map_or(Value::Null, Value::from);
                "db.key.expire"
            }
            None => "db.key.get",
        };
        match registry.dispatch(session, command, params) {
            Ok(answer) => {
                self.operation = answer["operation"].as_u64();
                self.error = None;
                self.take(answer);
            }
            Err(error) => {
                self.operation = None;
                self.error = Some(match error {
                    zen_core::DispatchError::Failed { error, .. } => error.to_string(),
                    other => other.to_string(),
                });
            }
        }
    }

    fn poll(&mut self, registry: &Registry, session: &mut Session) {
        let Some(operation) = self.operation else { return };
        if let Ok(answer) = registry.dispatch(session, "db.key.state", json!({ "operation": operation })) {
            self.take(answer);
        }
    }

    fn take(&mut self, answer: Value) {
        if answer["status"] == "running" {
            return;
        }
        self.operation = None;
        if answer["status"] != "done" {
            self.error = answer["message"].as_str().map(str::to_owned);
            return;
        }
        let key: zen_db::value::KeyValue = serde_json::from_value(answer["key"].clone()).unwrap_or_default();
        self.columns = key
            .columns
            .iter()
            .map(|column| GridColumn {
                name: column.name.clone(),
                type_name: column.type_name.clone(),
                sort: None,
                editable: false,
                filtered: false,
                not_null: false,
                primary: false,
            })
            .collect();
        self.rows = key.rows.iter().map(|row| row.iter().map(grid_cell).collect()).collect();
        self.text = matches!(key.type_name.as_str(), "string" | "json")
            .then(|| key.rows.first().and_then(|row| row.first()).map(pretty).unwrap_or_default());
        self.revision += 1;
        self.answer = Some(answer["key"].clone());
    }
}

fn button(ui: &mut Ui, p: &IdePalette, rect: Rect, label: &str, salt: &str, enabled: bool) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(salt), if enabled { Sense::click() } else { Sense::hover() });
    accessibility::name_with(&response, egui::Role::Button, || label.to_owned());
    let fill = if response.hovered() && enabled { p.hover } else { p.text_field };
    ui.painter().rect_filled(rect, 4, fill);
    ui.painter().rect_stroke(rect, 4, egui::Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
    let color = if enabled { p.text } else { p.text_secondary };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::regular(12.5), color);
    enabled && response.clicked()
}

impl KeyViews {
    pub(in super::super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        tab: &InnerTab,
    ) {
        let state = self.tabs.entry(tab.id.clone()).or_default();
        if state.answer.is_none() && state.operation.is_none() && state.error.is_none() {
            state.load(registry, session, tab, None);
        }
        state.poll(registry, session);
        if state.operation.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        let area = ui.max_rect();
        ui.painter().rect_filled(area, 0, p.editor_background);
        let header = Rect::from_min_size(area.min, vec2(area.width(), HEADER));
        ui.painter().rect_filled(header, 0, p.island);
        ui.painter().hline(header.x_range(), header.bottom(), egui::Stroke::new(1.0, p.frame));
        let key = tab.data["key"].as_str().unwrap_or(&tab.title).to_owned();
        let name_font = theme::semibold(13.0);
        let galley = ui.painter().layout_no_wrap(key.clone(), name_font, p.text);
        let mut left = header.left() + 12.0;
        let name_width = galley.size().x.min(area.width() * 0.4);
        ui.painter().with_clip_rect(Rect::from_min_size(pos2(left, header.top()), vec2(name_width, HEADER))).galley(
            pos2(left, header.center().y - galley.size().y / 2.0),
            galley,
            p.text,
        );
        left += name_width + 14.0;
        let answer = state.answer.clone().unwrap_or(Value::Null);
        let mut facts = Vec::new();
        if let Some(kind) = answer["type"].as_str() {
            facts.push(if kind == "none" { t!("ide-dbkey-missing") } else { kind.to_owned() });
        }
        if answer.is_object() && answer["type"] != "none" {
            facts.push(match answer["ttlMs"].as_i64() {
                Some(ttl) => t!("ide-dbkey-ttl", ttl = ttl_text(ttl)),
                None => t!("ide-dbkey-no-ttl"),
            });
        }
        if let Some(length) = answer["length"].as_u64() {
            facts.push(if answer["type"] == "string" {
                t!("ide-dbkey-length-bytes", size = size_text(length))
            } else {
                t!("ide-dbkey-length-items", count = length)
            });
        }
        if let Some(size) = answer["size"].as_u64() {
            facts.push(t!("ide-dbkey-memory", size = size_text(size)));
        }
        if let Some(encoding) = answer["encoding"].as_str() {
            facts.push(encoding.to_owned());
        }
        if state.operation.is_some() {
            facts.push(t!("ide-dbkey-reading"));
        }
        let mut right = header.right() - 8.0;
        let height = HEADER - 10.0;
        let has_ttl = answer["ttlMs"].is_i64();
        let exists = answer.is_object() && answer["type"] != "none";
        let mut place = |width: f32| {
            right -= width;
            let rect = Rect::from_min_size(pos2(right, header.top() + 5.0), vec2(width, height));
            right -= 6.0;
            rect
        };
        let refresh = place(74.0);
        if button(ui, p, refresh, &t!("ide-dbkey-refresh"), "key-refresh", state.operation.is_none()) {
            state.load(registry, session, tab, None);
        }
        let persist = place(84.0);
        if button(ui, p, persist, &t!("ide-dbkey-persist"), "key-persist", has_ttl && state.operation.is_none()) {
            state.load(registry, session, tab, Some(None));
        }
        let set = place(70.0);
        let field = place(76.0);
        let seconds: Option<u64> = state.ttl.trim().parse().ok().filter(|seconds| *seconds > 0);
        let enabled = exists && seconds.is_some() && state.operation.is_none();
        if button(ui, p, set, &t!("ide-dbkey-set-ttl"), "key-set-ttl", enabled) {
            state.load(registry, session, tab, Some(seconds));
            state.ttl.clear();
        }
        ui.painter().rect_filled(field, 4, p.text_field);
        ui.painter().rect_stroke(field, 4, egui::Stroke::new(1.0, p.text_field_border), egui::StrokeKind::Inside);
        let edit = egui::TextEdit::singleline(&mut state.ttl)
            .hint_text(t!("ide-dbkey-seconds"))
            .font(theme::regular(12.5))
            .frame(egui::Frame::NONE)
            .align(Align2::LEFT_CENTER)
            .min_size(vec2(field.width() - 12.0, field.height()))
            .desired_width(field.width() - 12.0);
        ui.put(field.shrink2(vec2(6.0, 0.0)), edit);
        let facts_end = field.left() - 12.0;
        if facts_end > left {
            let facts_rect = Rect::from_min_max(pos2(left, header.top()), pos2(facts_end, header.bottom()));
            ui.painter().with_clip_rect(facts_rect).text(
                pos2(left, header.center().y),
                Align2::LEFT_CENTER,
                facts.join("  ·  "),
                theme::regular(12.5),
                p.text_secondary,
            );
        }

        let footer_text = match (&state.error, answer["length"].as_u64()) {
            (Some(error), _) => Some((error.clone(), p.error_text)),
            (None, Some(length)) if answer["truncated"] == true && answer["type"] != "string" => {
                Some((t!("ide-dbkey-first-items", shown = state.rows.len(), total = length), p.text_secondary))
            }
            (None, _) if answer["truncated"] == true => Some((t!("ide-dbkey-string-cut"), p.text_secondary)),
            _ => None,
        };
        let footer = footer_text.as_ref().map_or(0.0, |_| FOOTER);
        let body =
            Rect::from_min_max(pos2(area.left(), header.bottom() + 1.0), pos2(area.right(), area.bottom() - footer));
        if let Some((text, color)) = footer_text {
            let line = Rect::from_min_max(pos2(area.left(), body.bottom()), area.max);
            ui.painter().rect_filled(line, 0, p.island);
            ui.painter().text(
                pos2(line.left() + 12.0, line.center().y),
                Align2::LEFT_CENTER,
                text,
                theme::regular(12.0),
                color,
            );
        }
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        if let Some(text) = state.text.clone() {
            egui::ScrollArea::both().id_salt(("key-text", &tab.id)).auto_shrink([false, false]).show(
                &mut child,
                |ui| {
                    let mut shown = text;
                    ui.add(
                        egui::TextEdit::multiline(&mut shown)
                            .font(FontId::monospace(12.5))
                            .interactive(true)
                            .desired_width(f32::INFINITY)
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::same(12)),
                    );
                },
            );
        } else if !state.columns.is_empty() {
            let id = child.scope_id().with(("key-grid", &tab.id));
            let mut grid = std::mem::take(&mut state.grid);
            data_grid::show(&mut child, p, &KeySource(&*state), &mut grid, id);
            state.grid = grid;
        } else if answer["type"] == "none" {
            child.painter().text(
                body.center(),
                Align2::CENTER_CENTER,
                t!("ide-dbkey-missing-long"),
                theme::regular(13.0),
                p.text_secondary,
            );
        }
    }

    pub(in super::super) fn retain(&mut self, open: &[String]) {
        self.tabs.retain(|id, _| open.contains(id));
    }
}

#[cfg(test)]
mod tests;
