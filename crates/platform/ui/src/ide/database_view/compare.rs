use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::database::compare::Compares;
use zen_workbench::vcs::commit::DiffView;
use zen_workbench::workspaces::InnerTab;

use super::super::diff_view::DiffTab;
use super::super::{IdePalette, theme, vcs};
use super::{Listed, message};
use crate::accessibility;

const BAR: f32 = 36.0;

const ROW: f32 = 24.0;

const TREE: f32 = 0.38;

const DATA: f32 = 0.45;

const OBJECTS: usize = 2000;

const ROWS: usize = 200;

const DDL: usize = 200_000;

#[derive(Clone, Default, PartialEq)]
struct Side {
    connection: Option<String>,
    snapshot: Option<(PathBuf, String)>,
    schema: String,
}

impl Side {
    fn params(&self) -> Option<Value> {
        if let Some((path, _)) = &self.snapshot {
            return Some(json!({ "kind": "snapshot", "path": path }));
        }
        let connection = self.connection.as_ref()?;
        (!self.schema.trim().is_empty())
            .then(|| json!({ "kind": "connection", "connection": connection, "schema": self.schema.trim() }))
    }

    fn of_request(side: &Value) -> Self {
        let text = |field: &str| side[field].as_str().unwrap_or_default().to_owned();
        if let Some(path) = side["path"].as_str() {
            let file = PathBuf::from(path);
            let label = file.file_name().map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned());
            return Side { connection: None, snapshot: Some((file, label)), schema: String::new() };
        }
        Side { connection: side["connection"].as_str().map(str::to_owned), snapshot: None, schema: text("schema") }
    }

    fn label(&self, listed: &Listed) -> Option<String> {
        if let Some((_, label)) = &self.snapshot {
            return Some(label.clone());
        }
        let id = self.connection.as_ref()?;
        let name =
            listed.connections.iter().find(|known| known["id"] == id.as_str()).and_then(|known| known["name"].as_str());
        Some(name.unwrap_or(id).to_owned())
    }
}

#[derive(Default)]
pub(super) struct CompareView {
    preset: Value,
    source: Side,
    target: Side,
    snapshots: Option<Vec<Value>>,
    id: Option<u64>,
    state: Value,
    expanded: BTreeSet<String>,
    selected: Option<(String, String)>,
    ddl: Option<((u64, String, String), DiffTab)>,
    drop: bool,
    data: Option<(u64, Value)>,
    error: Option<String>,
    notice: Option<String>,
    seen: u64,
}

pub(super) fn glyph_compare(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new((rect.width() / 12.0).max(1.2), color);
    let w = rect.width();
    let left = Rect::from_min_size(rect.min, vec2(w * 0.4, rect.height()));
    let right = Rect::from_min_size(pos2(rect.right() - w * 0.4, rect.top()), vec2(w * 0.4, rect.height()));
    painter.rect_stroke(left, 1.5, stroke, StrokeKind::Inside);
    painter.rect_stroke(right, 1.5, stroke, StrokeKind::Inside);
    let y = rect.center().y;
    painter.line_segment([pos2(left.right() + 1.0, y), pos2(right.left() - 1.0, y)], stroke);
}

fn chevron(painter: &egui::Painter, center: egui::Pos2, open: bool, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let (a, b, c) = if open {
        (center + vec2(-4.0, -2.0), center + vec2(0.0, 2.0), center + vec2(4.0, -2.0))
    } else {
        (center + vec2(-2.0, -4.0), center + vec2(2.0, 0.0), center + vec2(-2.0, 4.0))
    };
    painter.line_segment([a, b], stroke);
    painter.line_segment([b, c], stroke);
}

fn glyph_swap(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let (top, bottom) = (rect.top() + rect.height() * 0.32, rect.top() + rect.height() * 0.68);
    painter.line_segment([pos2(rect.left(), top), pos2(rect.right(), top)], stroke);
    painter.line_segment([pos2(rect.right() - 3.0, top - 3.0), pos2(rect.right(), top)], stroke);
    painter.line_segment([pos2(rect.left(), bottom), pos2(rect.right(), bottom)], stroke);
    painter.line_segment([pos2(rect.left() + 3.0, bottom + 3.0), pos2(rect.left(), bottom)], stroke);
}

fn change_color(p: &IdePalette, change: &str) -> Color32 {
    match change {
        "added" => p.vcs_added,
        "removed" => p.vcs_deleted,
        _ => p.vcs_modified,
    }
}

fn change_mark(change: &str) -> &'static str {
    match change {
        "added" => "+",
        "removed" => "−",
        _ => "~",
    }
}

fn kind_label(kind: &str) -> String {
    match kind {
        "table" => t!("ide-dbcompare-kind-table"),
        "view" => t!("ide-dbcompare-kind-view"),
        "materializedView" => t!("ide-dbcompare-kind-materialized-view"),
        "function" => t!("ide-dbcompare-kind-function"),
        "procedure" => t!("ide-dbcompare-kind-procedure"),
        "sequence" => t!("ide-dbcompare-kind-sequence"),
        "type" => t!("ide-dbcompare-kind-type"),
        "event" => t!("ide-dbcompare-kind-event"),
        "package" => t!("ide-dbcompare-kind-package"),
        _ => t!("ide-dbcompare-kind-foreign-table"),
    }
}

fn part_label(part: &str) -> String {
    match part {
        "column" => t!("ide-dbcompare-part-column"),
        "constraint" => t!("ide-dbcompare-part-constraint"),
        "foreignKey" => t!("ide-dbcompare-part-foreign-key"),
        "index" => t!("ide-dbcompare-part-index"),
        "trigger" => t!("ide-dbcompare-part-trigger"),
        "policy" => t!("ide-dbcompare-part-policy"),
        "definition" => t!("ide-dbcompare-part-definition"),
        _ => t!("ide-dbcompare-part-property"),
    }
}

fn value_text(value: &Value) -> String {
    match value["kind"].as_str() {
        Some("null") | None => "NULL".to_owned(),
        _ => {
            let inner = &value["value"];
            let text = inner.as_str().map(str::to_owned).unwrap_or_else(|| match inner {
                Value::Object(typed) => typed.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
                other => other.to_string(),
            });
            text.chars().take(80).collect()
        }
    }
}

fn text_button(
    ui: &mut Ui,
    p: &IdePalette,
    (left, y): (f32, f32),
    label: &str,
    enabled: bool,
) -> (egui::Response, f32) {
    let font = theme::regular(12.5);
    let width = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), p.text).size().x + 20.0;
    let button = Rect::from_min_size(pos2(left, y - 12.0), vec2(width, 24.0));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(button, ui.scope_id().with(("database-compare-button", label)), sense);
    ui.painter().rect_stroke(button, 4, Stroke::new(1.0, p.button_border), StrokeKind::Inside);
    if enabled && response.hovered() {
        ui.painter().rect_filled(button, 4, p.hover);
    }
    let color = if enabled { p.text } else { p.text_secondary };
    ui.painter().text(button.center(), Align2::CENTER_CENTER, label, font, color);
    accessibility::button(&response, label);
    if !enabled {
        accessibility::disabled(&response);
    }
    (response, button.right() + 6.0)
}

fn label_at(ui: &Ui, (left, y): (f32, f32), text: &str, color: Color32) -> f32 {
    let rect = ui.painter().text(pos2(left, y), Align2::LEFT_CENTER, text, theme::regular(12.5), color);
    rect.right() + 6.0
}

impl CompareView {
    fn take_preset(&mut self, tab: &InnerTab) -> bool {
        if tab.data == self.preset {
            return false;
        }
        self.preset = tab.data.clone();
        let text = |field: &str| tab.data[field].as_str().map(str::to_owned);
        if let Some(connection) = text("source") {
            self.source =
                Side { connection: Some(connection), schema: text("sourceSchema").unwrap_or_default(), snapshot: None };
        }
        if let Some(connection) = text("target") {
            self.target =
                Side { connection: Some(connection), schema: text("targetSchema").unwrap_or_default(), snapshot: None };
        }
        let Some(id) = tab.data["id"].as_u64() else { return false };
        self.show_compare(id);
        true
    }

    fn follow_latest(&mut self, registry: &Registry, session: &mut Session) {
        let revision = session.state::<Compares>().map_or(0, Compares::revision);
        if revision == self.seen {
            return;
        }
        self.seen = revision;
        let Ok(answer) = registry.dispatch(session, "db.compare.state", json!({ "limit": 1 })) else { return };
        let latest = &answer["compares"][0];
        let Some(id) = latest["id"].as_u64() else { return };
        if self.id == Some(id) || self.data.as_ref().is_some_and(|(shown, _)| *shown == id) {
            return;
        }
        let (source, target) =
            (Side::of_request(&latest["request"]["source"]), Side::of_request(&latest["request"]["target"]));
        self.error = None;
        self.notice = None;
        if latest["kind"] == "data" {
            if (&source, &target) != (&self.source, &self.target) {
                self.source = source;
                self.target = target;
                self.id = None;
                self.state = Value::Null;
                self.selected = None;
                self.ddl = None;
            }
            self.data = Some((id, Value::Null));
        } else {
            self.source = source;
            self.target = target;
            self.data = None;
            self.show_compare(id);
        }
    }

    fn show_compare(&mut self, id: u64) {
        self.id = Some(id);
        self.state = Value::Null;
        self.selected = None;
        self.ddl = None;
        self.expanded.clear();
    }

    fn running(state: &Value) -> bool {
        state["status"] == "running"
    }

    fn refresh(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        if let Some(id) = self.id
            && (self.state.is_null() || Self::running(&self.state))
        {
            let answer = registry.dispatch(session, "db.compare.state", json!({ "id": id, "limit": OBJECTS }));
            match answer {
                Ok(answer) => self.state = answer["compares"][0].clone(),
                Err(error) => {
                    self.error = Some(message(error));
                    self.id = None;
                }
            }
            if Self::running(&self.state) {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if let Some((id, state)) = &mut self.data
            && (state.is_null() || Self::running(state))
        {
            if let Ok(answer) = registry.dispatch(session, "db.compare.state", json!({ "id": *id, "limit": ROWS })) {
                *state = answer["compares"][0].clone();
            }
            if Self::running(state) {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }

    fn first_schema(registry: &Registry, session: &mut Session, connection: &str) -> String {
        let children = |session: &mut Session, path: Value| {
            registry
                .dispatch(session, "db.schema.children", json!({ "connection": connection, "path": path }))
                .ok()
                .and_then(|answer| answer["nodes"].as_array().cloned())
                .unwrap_or_default()
        };
        let catalogs = children(session, json!([]));
        let Some(catalog) = catalogs.first().and_then(|node| node["label"].as_str()).map(str::to_owned) else {
            return String::new();
        };
        let schemas = children(session, json!([catalog]));
        let names: Vec<&str> = schemas.iter().filter_map(|node| node["label"].as_str()).collect();
        names.iter().find(|name| **name == "public").or(names.first()).map(|name| (*name).to_owned()).unwrap_or(catalog)
    }

    fn compare(&mut self, registry: &Registry, session: &mut Session) {
        let (Some(source), Some(target)) = (self.source.params(), self.target.params()) else { return };
        match registry.dispatch(session, "db.compare.schemas", json!({ "source": source, "target": target })) {
            Ok(answer) => {
                self.error = None;
                self.notice = None;
                self.data = None;
                if let Some(id) = answer["id"].as_u64() {
                    self.show_compare(id);
                }
            }
            Err(error) => self.error = Some(message(error)),
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (tab, listed): (&InnerTab, &Listed),
    ) {
        if self.take_preset(tab) {
            self.seen = session.state::<Compares>().map_or(0, Compares::revision);
        }
        self.follow_latest(registry, session);
        self.refresh(ui.ctx(), registry, session);
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.editor_background);
        let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR));
        let mut bar_ui = ui.new_child(UiBuilder::new().id_salt("database-compare-bar").max_rect(bar));
        self.bar(&mut bar_ui, p, registry, session, (bar, listed));
        let mut top = bar.bottom();
        if self.state["status"] == "done" {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), BAR));
            let mut strip_ui = ui.new_child(UiBuilder::new().id_salt("database-compare-summary").max_rect(strip));
            self.summary(&mut strip_ui, p, registry, session, strip);
            top = strip.bottom();
        }
        let problem = self.error.clone().or_else(|| self.state["error"].as_str().map(str::to_owned));
        let line = problem
            .as_ref()
            .map(|text| (text.clone(), p.error_text))
            .or_else(|| self.notice.clone().map(|text| (text, p.text_secondary)));
        if let Some((text, color)) = line {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), 24.0));
            let job = vcs::truncated(&text, theme::regular(12.0), color, strip.width() - 24.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(strip.left() + 12.0, strip.center().y - galley.size().y / 2.0), galley, color);
            top = strip.bottom();
        }
        let body = Rect::from_min_max(pos2(area.left(), top), area.max);
        ui.painter().hline(body.x_range(), body.top() + 0.5, Stroke::new(1.0, p.frame));
        if self.state["status"] == "done" {
            let split = body.left() + body.width() * TREE;
            let tree = Rect::from_min_max(body.min, pos2(split, body.bottom()));
            let detail = Rect::from_min_max(pos2(split + 1.0, body.top()), body.max);
            ui.painter().vline(split + 0.5, body.y_range(), Stroke::new(1.0, p.frame));
            self.tree(ui, p, tree);
            self.detail(ui, p, registry, session, detail);
        } else if self.data.is_some() {
            self.data_panel(ui, p, registry, session, body);
        } else {
            let text = if Self::running(&self.state) { t!("ide-dbcompare-reading") } else { t!("ide-dbcompare-empty") };
            ui.painter().text(body.center(), Align2::CENTER_CENTER, text, theme::regular(13.0), p.text_secondary);
        }
        ui.advance_cursor_after_rect(area);
    }

    #[allow(clippy::too_many_lines, reason = "one bar with both sides' pickers")]
    fn bar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (bar, listed): (Rect, &Listed),
    ) {
        ui.painter().rect_filled(bar, 0, p.island);
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let y = bar.center().y;
        let mut at = bar.left() + 10.0;
        for target in [false, true] {
            let caption = if target { t!("ide-dbcompare-target") } else { t!("ide-dbcompare-source") };
            at = label_at(ui, (at, y), &caption, p.text_secondary);
            let side = if target { &self.target } else { &self.source };
            let choose = t!("ide-dbcompare-choose");
            let label = side.label(listed);
            let shown = label.clone().unwrap_or(choose);
            let salt = if target { "compare-target" } else { "compare-source" };
            let (picker, next) = super::DatabaseView::selector(ui, p, at, (&shown, label.is_none()), salt);
            at = next;
            if picker.clicked() && self.snapshots.is_none() {
                self.snapshots = registry
                    .dispatch(session, "db.compare.snapshots", json!({ "limit": 30 }))
                    .ok()
                    .and_then(|answer| answer["snapshots"].as_array().cloned());
            }
            let mut chosen: Option<Side> = None;
            egui::Popup::menu(&picker).show(|ui| {
                ui.set_min_width(220.0);
                for connection in &listed.connections {
                    let Some(id) = connection["id"].as_str() else { continue };
                    let name = connection["name"].as_str().unwrap_or(id);
                    let on = side.connection.as_deref() == Some(id) && side.snapshot.is_none();
                    if ui.selectable_label(on, name).clicked() {
                        chosen = Some(Side { connection: Some(id.to_owned()), snapshot: None, schema: String::new() });
                        ui.close();
                    }
                }
                let saved = self.snapshots.clone().unwrap_or_default();
                if !saved.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new(t!("ide-dbcompare-saved-snapshots")).color(p.text_secondary));
                    for snapshot in saved {
                        let Some(path) = snapshot["path"].as_str() else { continue };
                        let label = format!(
                            "{} · {}",
                            snapshot["connection"].as_str().unwrap_or_default(),
                            snapshot["schema"].as_str().unwrap_or_default()
                        );
                        let file = PathBuf::from(path);
                        let on = side.snapshot.as_ref().is_some_and(|(known, _)| *known == file);
                        if ui.selectable_label(on, &label).on_hover_text(path).clicked() {
                            chosen =
                                Some(Side { connection: None, snapshot: Some((file, label)), schema: String::new() });
                            ui.close();
                        }
                    }
                }
            });
            if let Some(mut picked) = chosen {
                if let Some(connection) = picked.connection.clone() {
                    picked.schema = Self::first_schema(registry, session, &connection);
                }
                if target {
                    self.target = picked;
                } else {
                    self.source = picked;
                }
            }
            let side = if target { &mut self.target } else { &mut self.source };
            if side.snapshot.is_none() && side.connection.is_some() {
                let field = Rect::from_min_size(pos2(at, y - 12.0), vec2(120.0, 24.0));
                ui.painter().rect_filled(field, 5, p.text_field);
                ui.painter().rect_stroke(field, 5, Stroke::new(1.0, p.text_field_border), StrokeKind::Inside);
                let hint = t!("ide-dbcompare-schema");
                let response = ui.put(
                    field.shrink2(vec2(6.0, 2.0)),
                    egui::TextEdit::singleline(&mut side.schema)
                        .id_salt((salt, "schema"))
                        .frame(egui::Frame::NONE)
                        .font(theme::regular(12.5))
                        .hint_text(hint.clone()),
                );
                accessibility::labelled(&response, &hint);
                at = field.right() + 8.0;
            }
            if !target {
                let swap_label = t!("ide-dbcompare-swap");
                let (swap, next) = super::DatabaseView::tool_button(
                    ui,
                    p,
                    (at, glyph_swap, p.text_secondary),
                    true,
                    (&swap_label, None),
                );
                if swap.clicked() {
                    std::mem::swap(&mut self.source, &mut self.target);
                }
                at = next + 6.0;
            }
        }
        let ready = self.source.params().is_some() && self.target.params().is_some() && !Self::running(&self.state);
        let (run, next) = text_button(ui, p, (at + 4.0, y), &t!("ide-dbcompare-compare"), ready);
        if run.clicked() {
            self.compare(registry, session);
        }
        if Self::running(&self.state)
            && let Some(id) = self.id
        {
            let (cancel, _) = text_button(ui, p, (next, y), &t!("ide-dbcompare-cancel"), true);
            if cancel.clicked() {
                self.error = registry.dispatch(session, "db.compare.cancel", json!({ "id": id })).err().map(message);
            }
        }
    }

    fn summary(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, strip: Rect) {
        ui.painter().hline(strip.x_range(), strip.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let y = strip.center().y;
        let summary = &self.state["summary"];
        let count = |field: &str| summary[field].as_u64().unwrap_or(0);
        let mut at = strip.left() + 12.0;
        if count("added") + count("removed") + count("changed") == 0 {
            at = label_at(ui, (at, y), &t!("ide-dbcompare-identical", same = count("same")), p.text_secondary);
        } else {
            for (field, color) in [("added", p.vcs_added), ("removed", p.vcs_deleted), ("changed", p.vcs_modified)] {
                let text = match field {
                    "added" => t!("ide-dbcompare-count-added", count = count(field)),
                    "removed" => t!("ide-dbcompare-count-removed", count = count(field)),
                    _ => t!("ide-dbcompare-count-changed", count = count(field)),
                };
                at = label_at(ui, (at, y), &text, color) + 6.0;
            }
            at = label_at(ui, (at, y), &t!("ide-dbcompare-count-same", count = count("same")), p.text_secondary) + 10.0;
        }
        if summary["partial"].as_u64().unwrap_or(0) > 0 {
            at = label_at(
                ui,
                (at, y),
                &t!("ide-dbcompare-partial", count = summary["partial"].as_u64().unwrap_or(0)),
                p.warning_text,
            ) + 10.0;
        }
        let Some(id) = self.id else { return };
        let mut drop = self.drop;
        let check_rect = Rect::from_min_size(pos2(at, y - 11.0), vec2(160.0, 22.0));
        let response = ui.put(check_rect, egui::Checkbox::new(&mut drop, t!("ide-dbcompare-drops")));
        accessibility::labelled(&response, &t!("ide-dbcompare-drops"));
        self.drop = drop;
        at = check_rect.right() + 6.0;
        let (open, next) = text_button(ui, p, (at, y), &t!("ide-dbcompare-open-script"), true);
        if open.clicked() {
            self.script(registry, session, id, true, ui);
        }
        let (copy, next) = text_button(ui, p, (next, y), &t!("ide-dbcompare-copy-script"), true);
        if copy.clicked() {
            self.script(registry, session, id, false, ui);
        }
        if let Some(connection) = self.source.connection.clone().filter(|_| self.source.snapshot.is_none()) {
            let (save, _) = text_button(ui, p, (next, y), &t!("ide-dbcompare-save-snapshot"), true);
            if save.clicked() {
                let params = json!({ "connection": connection, "schema": self.source.schema.trim() });
                match registry.dispatch(session, "db.compare.saveSnapshot", params) {
                    Ok(saved) => {
                        self.snapshots = None;
                        self.error = None;
                        self.notice =
                            Some(t!("ide-dbcompare-saved", path = saved["path"].as_str().unwrap_or_default()));
                    }
                    Err(error) => self.error = Some(message(error)),
                }
            }
        }
    }

    fn script(&mut self, registry: &Registry, session: &mut Session, id: u64, open: bool, ui: &Ui) {
        let params = json!({ "id": id, "drop": self.drop, "open": open, "limit": 64 * 1024 * 1024 });
        match registry.dispatch(session, "db.compare.script", params) {
            Ok(answer) => {
                self.error = None;
                if let Some(text) = answer["text"].as_str() {
                    ui.ctx().copy_text(text.to_owned());
                    self.notice =
                        Some(t!("ide-dbcompare-copied", statements = answer["statements"].as_u64().unwrap_or(0)));
                } else {
                    self.notice = None;
                }
            }
            Err(error) => self.error = Some(message(error)),
        }
    }

    fn tree(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect) {
        let objects = self.state["objects"].as_array().cloned().unwrap_or_default();
        let mut child = ui.new_child(UiBuilder::new().id_salt("database-compare-tree").max_rect(rect));
        let mut picked = None;
        egui::ScrollArea::vertical().id_salt("database-compare-tree-scroll").auto_shrink([false, false]).show(
            &mut child,
            |ui| {
                let width = rect.width();
                let mut last_kind = String::new();
                for object in &objects {
                    let kind = object["kind"].as_str().unwrap_or_default().to_owned();
                    let key = object["key"].as_str().unwrap_or_default().to_owned();
                    if kind != last_kind {
                        let (header, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                        ui.painter().text(
                            pos2(header.left() + 10.0, header.center().y),
                            Align2::LEFT_CENTER,
                            kind_label(&kind),
                            theme::bold(12.0),
                            p.text_secondary,
                        );
                        accessibility::heading(ui, ui.scope_id().with(("compare-kind", &kind)), header, || {
                            kind_label(&kind)
                        });
                        last_kind.clone_from(&kind);
                    }
                    let id = format!("{kind}/{key}");
                    let items = object["items"].as_array().cloned().unwrap_or_default();
                    let open = self.expanded.contains(&id);
                    let selected = self.selected.as_ref() == Some(&(kind.clone(), key.clone()));
                    let (row, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                    if selected {
                        ui.painter().rect_filled(row, 0, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(row, 0, p.hover);
                    }
                    let change = object["change"].as_str().unwrap_or_default();
                    let color = change_color(p, change);
                    if !items.is_empty() {
                        chevron(ui.painter(), pos2(row.left() + 18.0, row.center().y), open, p.text_secondary);
                        if response.clicked()
                            && ui
                                .input(|input| input.pointer.interact_pos())
                                .is_some_and(|pos| pos.x < row.left() + 26.0)
                        {
                            if open {
                                self.expanded.remove(&id);
                            } else {
                                self.expanded.insert(id.clone());
                            }
                        }
                    }
                    ui.painter().text(
                        pos2(row.left() + 30.0, row.center().y),
                        Align2::LEFT_CENTER,
                        change_mark(change),
                        theme::bold(12.5),
                        color,
                    );
                    let name = object["name"].as_str().unwrap_or_default();
                    let job = vcs::truncated(name, theme::regular(12.5), color, width - 60.0);
                    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                    ui.painter().galley(pos2(row.left() + 44.0, row.center().y - galley.size().y / 2.0), galley, color);
                    accessibility::tree_item(&response, 1, (!items.is_empty()).then_some(open), selected, name);
                    if response.clicked() {
                        picked = Some((kind.clone(), key.clone()));
                    }
                    if response.double_clicked() {
                        if open {
                            self.expanded.remove(&id);
                        } else {
                            self.expanded.insert(id.clone());
                        }
                    }
                    if open {
                        for item in &items {
                            let (line, item_response) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                            let change = item["change"].as_str().unwrap_or_default();
                            let color = change_color(p, change);
                            let mut text = format!(
                                "{} {}",
                                part_label(item["part"].as_str().unwrap_or_default()),
                                item["name"].as_str().unwrap_or_default()
                            );
                            if let Some(fields) = item["fields"].as_array().filter(|fields| !fields.is_empty()) {
                                let fields: Vec<&str> = fields.iter().filter_map(Value::as_str).collect();
                                text.push_str(&format!(" ({})", fields.join(", ")));
                            }
                            ui.painter().text(
                                pos2(line.left() + 50.0, line.center().y),
                                Align2::LEFT_CENTER,
                                change_mark(change),
                                theme::bold(12.0),
                                color,
                            );
                            let job = vcs::truncated(&text, theme::regular(12.0), color, width - 80.0);
                            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                            ui.painter().galley(
                                pos2(line.left() + 64.0, line.center().y - galley.size().y / 2.0),
                                galley,
                                color,
                            );
                            let detail = [item["source"].as_str(), item["target"].as_str()]
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join("\n→ ");
                            if !detail.is_empty() {
                                item_response.on_hover_text(detail);
                            }
                        }
                    }
                }
            },
        );
        if let Some(chosen) = picked {
            self.selected = Some(chosen);
        }
    }

    fn ddl_tab(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        id: u64,
        (kind, key): (&str, &str),
    ) -> Option<&mut DiffTab> {
        let wanted = (id, kind.to_owned(), key.to_owned());
        if self.ddl.as_ref().is_none_or(|(known, _)| *known != wanted) {
            let ddl = registry
                .dispatch(session, "db.compare.ddl", json!({ "id": id, "kind": kind, "key": key, "limit": DDL }))
                .ok()?;
            let source = ddl["source"].as_str().unwrap_or_default();
            let target = ddl["target"].as_str().unwrap_or_default();
            let titles = (
                self.state["source"]["label"].as_str().unwrap_or_default(),
                self.state["target"]["label"].as_str().unwrap_or_default(),
            );
            let compared = registry
                .dispatch(
                    session,
                    "file.compare.texts",
                    json!({ "left": source, "right": target, "leftTitle": titles.0, "rightTitle": titles.1 }),
                )
                .ok()?;
            let mut view: DiffView = serde_json::from_value(compared).ok()?;
            view.before_editable = false;
            view.after_editable = false;
            view.relative = key.to_owned();
            self.ddl = Some((wanted, DiffTab::new(view, None, false)));
        }
        self.ddl.as_mut().map(|(_, tab)| tab)
    }

    fn detail(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, rect: Rect) {
        let data_height = if self.data.is_some() { rect.height() * DATA } else { 0.0 };
        let ddl_rect = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - data_height));
        let Some(id) = self.id else { return };
        match self.selected.clone() {
            Some((kind, key)) => {
                let header = Rect::from_min_size(ddl_rect.min, vec2(ddl_rect.width(), BAR));
                let mut header_ui =
                    ui.new_child(UiBuilder::new().id_salt("database-compare-detail-bar").max_rect(header));
                let y = header.center().y;
                let at = label_at(&header_ui, (header.left() + 12.0, y), &key, p.text);
                let object = self.state["objects"]
                    .as_array()
                    .and_then(|objects| {
                        objects.iter().find(|object| object["kind"] == kind.as_str() && object["key"] == key.as_str())
                    })
                    .cloned();
                let rows = object.as_ref().is_some_and(|object| object["change"] == "changed" && kind == "table")
                    && self.source.snapshot.is_none()
                    && self.target.snapshot.is_none();
                if rows {
                    let (start, _) =
                        text_button(&mut header_ui, p, (at + 8.0, y), &t!("ide-dbcompare-compare-data"), true);
                    if start.clicked() {
                        self.start_data(registry, session, &key);
                    }
                }
                let body = Rect::from_min_max(pos2(ddl_rect.left(), header.bottom()), ddl_rect.max);
                if let Some(tab) = self.ddl_tab(registry, session, id, (&kind, &key)) {
                    let mut child =
                        ui.new_child(UiBuilder::new().id_salt(("database-compare-ddl", &kind, &key)).max_rect(body));
                    tab.show(&mut child, p);
                }
            }
            None => {
                ui.painter().text(
                    ddl_rect.center(),
                    Align2::CENTER_CENTER,
                    t!("ide-dbcompare-pick-object"),
                    theme::regular(13.0),
                    p.text_secondary,
                );
            }
        }
        if data_height > 0.0 {
            let data = Rect::from_min_max(pos2(rect.left(), rect.bottom() - data_height), rect.max);
            ui.painter().hline(data.x_range(), data.top() + 0.5, Stroke::new(1.0, p.frame));
            self.data_panel(ui, p, registry, session, data);
        }
    }

    fn start_data(&mut self, registry: &Registry, session: &mut Session, table: &str) {
        let side = |side: &Side| json!({ "connection": side.connection, "schema": side.schema.trim(), "table": table });
        let params = json!({ "source": side(&self.source), "target": side(&self.target) });
        match registry.dispatch(session, "db.compare.data", params) {
            Ok(answer) => {
                self.error = None;
                self.data = answer["id"].as_u64().map(|id| (id, Value::Null));
            }
            Err(error) => self.error = Some(message(error)),
        }
    }

    fn data_panel(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, rect: Rect) {
        let Some((id, state)) = self.data.clone() else { return };
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), BAR));
        let mut header_ui = ui.new_child(UiBuilder::new().id_salt("database-compare-data-bar").max_rect(header));
        header_ui.painter().rect_filled(header, 0, p.island);
        let y = header.center().y;
        let data = &state["data"];
        let title = t!(
            "ide-dbcompare-data-title",
            source = data["sourceTable"].as_str().unwrap_or_default(),
            target = data["targetTable"].as_str().unwrap_or_default()
        );
        let mut at = label_at(&header_ui, (header.left() + 12.0, y), &title, p.text) + 8.0;
        let counts = &data["counts"];
        let count = |field: &str| counts[field].as_u64().unwrap_or(0);
        for (text, color) in [
            (t!("ide-dbcompare-rows-same", count = count("same")), p.text_secondary),
            (t!("ide-dbcompare-rows-changed", count = count("changed")), p.vcs_modified),
            (t!("ide-dbcompare-rows-added", count = count("added")), p.vcs_added),
            (t!("ide-dbcompare-rows-removed", count = count("removed")), p.vcs_deleted),
        ] {
            at = label_at(&header_ui, (at, y), &text, color) + 4.0;
        }
        if let Some(error) = state["error"].as_str() {
            at = label_at(&header_ui, (at + 6.0, y), error, p.error_text);
        }
        let running = Self::running(&state);
        if running {
            let (cancel, _) = text_button(&mut header_ui, p, (at + 6.0, y), &t!("ide-dbcompare-cancel"), true);
            if cancel.clicked() {
                self.error = registry.dispatch(session, "db.compare.cancel", json!({ "id": id })).err().map(message);
            }
        } else if state["status"] == "done" {
            let (open, next) =
                text_button(&mut header_ui, p, (at + 6.0, y), &t!("ide-dbcompare-open-data-script"), true);
            if open.clicked() {
                self.script(registry, session, id, true, ui);
            }
            let (close, _) = text_button(&mut header_ui, p, (next, y), &t!("ide-dbcompare-close-data"), true);
            if close.clicked() {
                self.data = None;
                return;
            }
        }
        let columns: Vec<String> =
            data["columns"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        let rows = state["rows"].as_array().cloned().unwrap_or_default();
        let body = Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max);
        let mut child = ui.new_child(UiBuilder::new().id_salt("database-compare-rows").max_rect(body));
        egui::ScrollArea::vertical().id_salt("database-compare-rows-scroll").auto_shrink([false, false]).show(
            &mut child,
            |ui| {
                for row in &rows {
                    let (line, response) = ui.allocate_exact_size(vec2(body.width(), ROW), Sense::hover());
                    let change = row["change"].as_str().unwrap_or_default();
                    let color = change_color(p, change);
                    let key: Vec<String> = row["key"].as_array().into_iter().flatten().map(value_text).collect();
                    let differs: Vec<&str> =
                        row["columns"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    let values = |side: &str| -> Vec<String> {
                        row[side].as_array().into_iter().flatten().map(value_text).collect()
                    };
                    let (source, target) = (values("source"), values("target"));
                    let text = match change {
                        "changed" => differs
                            .iter()
                            .filter_map(|name| {
                                let at = columns.iter().position(|column| column == name)?;
                                Some(format!("{name}: {} → {}", source.get(at)?, target.get(at)?))
                            })
                            .collect::<Vec<_>>()
                            .join(", "),
                        "added" => source.get(key.len()..).unwrap_or_default().join(", "),
                        _ => target.get(key.len()..).unwrap_or_default().join(", "),
                    };
                    ui.painter().text(
                        pos2(line.left() + 12.0, line.center().y),
                        Align2::LEFT_CENTER,
                        change_mark(change),
                        theme::bold(12.0),
                        color,
                    );
                    let line_text = format!("{}  {text}", key.join(", "));
                    let job = vcs::truncated(&line_text, theme::regular(12.0), p.text, body.width() - 40.0);
                    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                    ui.painter().galley(
                        pos2(line.left() + 26.0, line.center().y - galley.size().y / 2.0),
                        galley,
                        p.text,
                    );
                    accessibility::label(ui, response.id, line, || line_text.clone());
                }
                if state["truncated"] == true {
                    ui.label(egui::RichText::new(t!("ide-dbcompare-rows-more")).color(p.text_secondary));
                }
            },
        );
    }
}

#[cfg(test)]
mod tests;
