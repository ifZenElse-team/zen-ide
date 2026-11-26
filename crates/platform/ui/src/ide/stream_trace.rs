use std::time::Duration;

use egui::{Key, Modifiers, Ui, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::debug_window::plain_value;
use super::run_window::{call, error_color};
use super::{IdePalette, metrics};
use crate::{accessibility, theme};

pub(super) const STREAM_TRACE: &str = "view.streamTrace.open";

const COLUMN_WIDTH: f32 = 190.0;
const VALUES_HEIGHT: f32 = 280.0;
const DIALOG_WIDTH: f32 = 820.0;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
struct Column {
    label: String,
    kind: String,
    values: Vec<String>,
    count: Option<i64>,
    result: Option<String>,
    error: Option<String>,
    pending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
struct Trace {
    chain: String,
    columns: Vec<Column>,
    pending: bool,
}

#[derive(Debug)]
struct Dialog {
    stops: Value,
    params: Value,
    trace: Trace,
    error: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct StreamTraces {
    dialog: Option<Dialog>,
}

fn stopped_state(registry: &Registry, session: &mut Session) -> Option<Value> {
    call(registry, session, "debug.session.get", json!({})).ok().filter(|state| state["state"] == "stopped")
}

impl StreamTraces {
    pub(super) fn name(&self) -> Option<&'static str> {
        self.dialog.as_ref().map(|_| "streamTrace")
    }

    pub(super) fn open(&mut self, registry: &Registry, session: &mut Session, params: &Value) -> Result<(), String> {
        let Some(state) = stopped_state(registry, session) else { return Err(t!("ide-stream-not-stopped")) };
        let mut asked = json!({ "waitMs": 0 });
        for key in ["path", "line"] {
            if let Some(value) = params.get(key) {
                asked[key] = value.clone();
            }
        }
        let answer = call(registry, session, "debug.stream.trace", asked.clone())?;
        let trace = serde_json::from_value::<Trace>(answer).unwrap_or_default();
        self.dialog = Some(Dialog { stops: state["stops"].clone(), params: asked, trace, error: None });
        Ok(())
    }

    pub(super) fn is_pending(&self) -> bool {
        self.dialog.as_ref().is_some_and(|dialog| dialog.trace.pending && dialog.error.is_none())
    }

    pub(super) fn poll(&mut self, registry: &Registry, session: &mut Session) {
        let Some(dialog) = self.dialog.as_mut() else { return };
        let still = stopped_state(registry, session).is_some_and(|state| state["stops"] == dialog.stops);
        if !still {
            self.dialog = None;
            return;
        }
        if !dialog.trace.pending || dialog.error.is_some() {
            return;
        }
        match call(registry, session, "debug.stream.trace", dialog.params.clone()) {
            Ok(answer) => dialog.trace = serde_json::from_value(answer).unwrap_or_default(),
            Err(message) => dialog.error = Some(message),
        }
    }

    #[cfg(test)]
    pub(super) fn shown(&self) -> Option<Vec<(String, Vec<String>)>> {
        let dialog = self.dialog.as_ref()?;
        Some(dialog.trace.columns.iter().map(|column| (column.label.clone(), column_lines(column))).collect())
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(dialog) = self.dialog.as_ref() else { return };
        let mut open = true;
        let id = ui.make_persistent_id("stream-trace-dialog");
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .movable(true)
            .default_pos(egui::pos2(160.0, 120.0))
            .show(ui.ctx(), |ui| {
                super::dialog_frame(p).show(ui, |ui| {
                    ui.set_max_width(DIALOG_WIDTH);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(t!("ide-stream-title")).font(theme::semibold(15.0)).color(p.text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let close = ui.add(egui::Button::new("×").frame(false));
                            accessibility::name(&close, egui::Role::Button, &t!("ide-stream-close"));
                            if close.clicked() {
                                open = false;
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&dialog.trace.chain)
                                .font(egui::FontId::monospace(12.5))
                                .color(p.text_secondary),
                        )
                        .wrap(),
                    );
                    if let Some(error) = &dialog.error {
                        ui.add(egui::Label::new(egui::RichText::new(error).color(error_color(p))).wrap());
                    }
                    ui.add_space(8.0);
                    egui::ScrollArea::horizontal().id_salt("stream-trace-columns").show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            for column in &dialog.trace.columns {
                                show_column(ui, p, column);
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(t!("ide-stream-note")).font(theme::regular(12.0)).color(p.text_secondary),
                    );
                });
            });
        accessibility::name(&area.response, egui::Role::Dialog, &t!("ide-stream-title"));
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            open = false;
        }
        if !open {
            self.dialog = None;
        } else if self.is_pending() {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
    }
}

fn column_lines(column: &Column) -> Vec<String> {
    if column.pending {
        return vec![t!("ide-stream-pending")];
    }
    if let Some(error) = &column.error {
        return vec![error.clone()];
    }
    let mut lines = Vec::new();
    match column.kind.as_str() {
        "unbounded" => lines.push(t!("ide-stream-unbounded")),
        "notRun" => lines.push(t!("ide-stream-not-run")),
        _ => {
            if let Some(result) = &column.result {
                lines.push(t!("ide-stream-result", result = plain_value(result).1));
            }
            lines.extend(column.values.iter().map(|value| plain_value(value).1.to_owned()));
            if column.values.is_empty() && column.result.is_none() {
                lines.push(t!("ide-stream-empty"));
            }
        }
    }
    lines
}

fn show_column(ui: &mut Ui, p: &IdePalette, column: &Column) {
    egui::Frame::new()
        .fill(p.text_field)
        .stroke(egui::Stroke::new(1.0, p.text_field_border))
        .corner_radius(4)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(COLUMN_WIDTH);
            ui.vertical(|ui| {
                let header = ui.add(
                    egui::Label::new(
                        egui::RichText::new(&column.label).font(egui::FontId::monospace(12.5)).color(p.text),
                    )
                    .truncate(),
                );
                header.on_hover_text(&column.label);
                let count = column.count.map(|count| {
                    let shown = column.values.len() as i64;
                    if shown < count {
                        t!("ide-stream-shown", shown = shown, count = count)
                    } else {
                        t!("ide-stream-count", count = count)
                    }
                });
                if let Some(count) = count {
                    ui.label(egui::RichText::new(count).font(theme::regular(12.0)).color(p.text_secondary));
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(("stream-trace-values", &column.label))
                    .max_height(VALUES_HEIGHT)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let failed = column.error.is_some();
                        for line in column_lines(column) {
                            let color = if failed { error_color(p) } else { p.text };
                            let (rect, response) = ui.allocate_exact_size(
                                vec2(ui.available_width(), metrics::TREE_ROW),
                                egui::Sense::hover(),
                            );
                            accessibility::name(&response, egui::Role::ListItem, &line);
                            ui.painter().with_clip_rect(rect).text(
                                rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                &line,
                                theme::regular(13.0),
                                color,
                            );
                        }
                    });
            });
        });
}
