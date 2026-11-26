use egui::{Align, Align2, Color32, FontId, Layout, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::super::{IdePalette, theme, vcs};
use super::message;
use crate::accessibility;

const BAR: f32 = 36.0;

const ROW: f32 = 26.0;

const DETAIL: f32 = 0.5;

#[derive(Default)]
pub(super) struct RecoveryView {
    shown: Option<(u64, Value)>,
    selected: Option<u64>,
    detail: Option<Value>,
    error: Option<String>,
    question: Option<Action>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Restore(u64),
    Discard(u64),
    DiscardAll,
}

fn kind_label(kind: &str) -> String {
    match kind {
        "console" => t!("ide-dbrecovery-kind-console"),
        "edits" => t!("ide-dbrecovery-kind-edits"),
        _ => t!("ide-dbrecovery-kind-transaction"),
    }
}

fn kind_color(p: &IdePalette, kind: &str) -> Color32 {
    match kind {
        "console" => p.link,
        "edits" => p.warning_text,
        _ => p.error_text,
    }
}

fn figures(item: &Value) -> String {
    let count = item["count"].as_u64().unwrap_or(0);
    match item["kind"].as_str() {
        Some("console") => t!("ide-dbrecovery-lines", count = count),
        Some("edits") => t!("ide-dbrecovery-rows", count = count),
        _ => t!("ide-dbrecovery-statements", count = count, modifying = item["modifying"].as_u64().unwrap_or(0)),
    }
}

impl RecoveryView {
    fn refresh(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let revision = zen_workbench::database::recovery::store(session).map_or(0, |recovery| recovery.revision());
        if self.shown.as_ref().is_some_and(|(shown, _)| *shown == revision) {
            return;
        }
        let answer = match registry.dispatch(session, "db.recovery.list", json!({ "limit": 500 })) {
            Ok(answer) => answer,
            Err(error) => {
                self.error = Some(message(error));
                Value::Null
            }
        };
        if answer["loading"] == true {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let listed = answer["items"].as_array().into_iter().flatten().any(|item| item["id"].as_u64() == self.selected);
        if !listed {
            self.selected = None;
            self.detail = None;
        }
        self.shown = Some((revision, answer));
    }

    fn items(&self) -> &[Value] {
        self.shown.as_ref().and_then(|(_, answer)| answer["items"].as_array()).map_or(&[], Vec::as_slice)
    }

    fn select(&mut self, registry: &Registry, session: &mut Session, id: u64) {
        self.selected = Some(id);
        self.detail = registry.dispatch(session, "db.recovery.get", json!({ "id": id, "textLimit": 20_000 })).ok();
    }

    fn act(&mut self, registry: &Registry, session: &mut Session, action: Action) {
        let asks = match action {
            Action::Restore(_) => false,
            Action::Discard(id) => {
                self.detail.as_ref().is_none_or(|item| item["id"].as_u64() != Some(id) || item["kind"] != "transaction")
            }
            Action::DiscardAll => true,
        };
        if asks {
            self.question = Some(action);
        } else {
            self.perform(registry, session, action);
        }
    }

    fn answer(&mut self, registry: &Registry, session: &mut Session, confirmed: bool) {
        if let Some(action) = self.question.take().filter(|_| confirmed) {
            self.perform(registry, session, action);
        }
    }

    fn perform(&mut self, registry: &Registry, session: &mut Session, action: Action) {
        let result = match action {
            Action::Restore(id) => registry.dispatch(session, "db.recovery.restore", json!({ "id": id })),
            Action::Discard(id) => {
                registry.dispatch(session, "db.recovery.discard", json!({ "ids": [id], "confirmed": true }))
            }
            Action::DiscardAll => {
                registry.dispatch(session, "db.recovery.discard", json!({ "all": true, "confirmed": true }))
            }
        };
        self.error = result.err().map(message);
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        self.refresh(ui.ctx(), registry, session);
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.editor_background);
        let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR));
        let mut actions = Vec::new();
        self.bar(ui, p, bar, &mut actions);
        let mut top = bar.bottom();
        if let Some(error) = &self.error {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), 24.0));
            let job = vcs::truncated(error, theme::regular(12.0), p.error_text, strip.width() - 24.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                pos2(strip.left() + 12.0, strip.center().y - galley.size().y / 2.0),
                galley,
                p.error_text,
            );
            top = strip.bottom();
        }
        if self.question.is_some() {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), BAR));
            self.question_strip(ui, p, registry, session, strip);
            top = strip.bottom();
        }
        let detail_height = if self.detail.is_some() { (area.height() * DETAIL).max(140.0) } else { 0.0 };
        let list = Rect::from_min_max(pos2(area.left(), top), pos2(area.right(), area.bottom() - detail_height));
        self.list(ui, p, registry, session, list);
        if detail_height > 0.0 {
            let detail = Rect::from_min_max(pos2(area.left(), list.bottom()), area.max);
            self.detail(ui, p, detail, &mut actions);
        }
        for action in actions {
            self.act(registry, session, action);
        }
        ui.advance_cursor_after_rect(area);
    }

    fn bar(&mut self, ui: &mut Ui, p: &IdePalette, bar: Rect, actions: &mut Vec<Action>) {
        ui.painter().rect_filled(bar, 0, p.island);
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("database-recovery-bar")
                .max_rect(bar.shrink2(vec2(12.0, 4.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        if !self.items().is_empty() && vcs::secondary_button(&mut child, p, &t!("ide-dbrecovery-discard-all")).clicked()
        {
            actions.push(Action::DiscardAll);
        }
        let note = t!("ide-dbrecovery-note");
        let job = vcs::truncated(&note, theme::regular(12.5), p.text_secondary, (bar.width() - 200.0).max(80.0));
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(bar.left() + 12.0, bar.center().y - galley.size().y / 2.0), galley, p.text_secondary);
    }

    fn question_strip(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, strip: Rect) {
        let Some(action) = self.question else { return };
        ui.painter().rect_filled(strip, 0, p.banner);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("database-recovery-question")
                .max_rect(strip.shrink2(vec2(12.0, 3.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let (question, confirm) = match action {
            Action::DiscardAll => (t!("ide-dbrecovery-discard-all-question"), t!("ide-dbrecovery-discard-all")),
            _ => (t!("ide-dbrecovery-discard-question"), t!("ide-dbrecovery-discard")),
        };
        child.label(egui::RichText::new(question).font(theme::regular(13.0)).color(p.text));
        if vcs::secondary_button(&mut child, p, &confirm).clicked() {
            self.answer(registry, session, true);
        }
        if vcs::secondary_button(&mut child, p, &t!("ide-dbrecovery-cancel")).clicked() {
            self.answer(registry, session, false);
        }
    }

    fn list(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, rect: Rect) {
        let items = self.items().to_vec();
        if items.is_empty() {
            let loading = self.shown.as_ref().is_some_and(|(_, answer)| answer["loading"] == true);
            let text = if loading { t!("ide-dbrecovery-loading") } else { t!("ide-dbrecovery-empty") };
            let job = vcs::truncated(&text, theme::regular(13.0), p.text_secondary, rect.width() - 32.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 40.0),
                galley,
                p.text_secondary,
            );
            return;
        }
        let mut child = ui.new_child(UiBuilder::new().id_salt("database-recovery-list").max_rect(rect));
        child.spacing_mut().item_spacing.y = 0.0;
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            &mut child,
            ROW,
            items.len(),
            |ui, range| {
                for item in &items[range] {
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let id = item["id"].as_u64().unwrap_or_default();
                    if self.selected == Some(id) {
                        ui.painter().rect_filled(row, 0, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(row, 0, p.hover);
                    }
                    draw_row(ui, p, row, item);
                    accessibility::name_with(&response, egui::Role::ListItem, || {
                        format!(
                            "{} {} {}",
                            item["time"].as_str().unwrap_or_default(),
                            kind_label(item["kind"].as_str().unwrap_or_default()),
                            item["title"].as_str().unwrap_or_default()
                        )
                    });
                    if response.clicked() {
                        chosen = Some(id);
                    }
                }
            },
        );
        if let Some(id) = chosen {
            self.select(registry, session, id);
        }
    }

    fn detail(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect, actions: &mut Vec<Action>) {
        let Some(item) = self.detail.clone() else { return };
        let id = item["id"].as_u64().unwrap_or_default();
        let kind = item["kind"].as_str().unwrap_or_default();
        ui.painter().rect_filled(rect, 0, p.island);
        ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.frame));
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), BAR));
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("database-recovery-actions")
                .max_rect(head.shrink2(vec2(12.0, 4.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        if kind != "transaction" {
            let label = if kind == "edits" { t!("ide-dbrecovery-open-sql") } else { t!("ide-dbrecovery-restore") };
            if vcs::secondary_button(&mut child, p, &label).clicked() {
                actions.push(Action::Restore(id));
            }
        }
        let discard = if kind == "transaction" { t!("ide-dbrecovery-dismiss") } else { t!("ide-dbrecovery-discard") };
        if vcs::secondary_button(&mut child, p, &discard).clicked() {
            actions.push(Action::Discard(id));
        }
        let body =
            Rect::from_min_max(pos2(rect.left() + 12.0, head.bottom()), pos2(rect.right() - 12.0, rect.bottom() - 6.0));
        let mut child = ui.new_child(
            UiBuilder::new().id_salt("database-recovery-detail").max_rect(body).layout(Layout::top_down(Align::Min)),
        );
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
            let explained = match kind {
                "console" => t!("ide-dbrecovery-console-explained"),
                "edits" => t!("ide-dbrecovery-edits-explained"),
                _ => t!("ide-dbrecovery-transaction-explained"),
            };
            ui.add(
                egui::Label::new(egui::RichText::new(explained).font(theme::regular(12.5)).color(p.text_secondary))
                    .wrap(),
            );
            ui.add_space(6.0);
            let text = match kind {
                "transaction" => item["log"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => item["text"].as_str().unwrap_or_default().to_owned(),
            };
            ui.add(
                egui::Label::new(egui::RichText::new(text).font(FontId::monospace(12.5)).color(p.text))
                    .wrap()
                    .selectable(true),
            );
        });
    }
}

fn draw_row(ui: &Ui, p: &IdePalette, row: Rect, item: &Value) {
    let painter = ui.painter();
    let kind = item["kind"].as_str().unwrap_or_default();
    painter.circle_filled(pos2(row.left() + 12.0, row.center().y), 3.5, kind_color(p, kind));
    let font = theme::regular(12.5);
    let time = item["time"].as_str().unwrap_or_default();
    painter.text(
        pos2(row.left() + 24.0, row.center().y),
        Align2::LEFT_CENTER,
        time.get(5..).unwrap_or(time),
        font.clone(),
        p.text_secondary,
    );
    let kind_left = row.left() + 150.0;
    let job = vcs::truncated(&kind_label(kind), font.clone(), p.text_secondary, 150.0);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    painter.galley(pos2(kind_left, row.center().y - galley.size().y / 2.0), galley, p.text_secondary);
    let title_left = kind_left + 160.0;
    let mut title = item["title"].as_str().unwrap_or_default().to_owned();
    if let Some(name) = item["connectionName"].as_str() {
        title = format!("{title} · {name}");
    }
    let numbers = figures(item);
    let numbers_width = painter.layout_no_wrap(numbers.clone(), font.clone(), p.text_secondary).size().x;
    painter.text(
        pos2(row.right() - 12.0, row.center().y),
        Align2::RIGHT_CENTER,
        numbers,
        font.clone(),
        p.text_secondary,
    );
    let job = vcs::truncated(&title, font, p.text, (row.right() - 28.0 - numbers_width - title_left).max(40.0));
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    painter.galley(pos2(title_left, row.center().y - galley.size().y / 2.0), galley, p.text);
}

#[cfg(test)]
mod tests;
