use std::collections::HashMap;

use egui::{Align2, Key, Modifiers, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::super::data_grid::{GridAction, GridOutcome};
use super::super::{IdePalette, theme, vcs};
use super::DatabaseView;
use crate::accessibility;

pub(super) const FILTER_BAR: f32 = 30.0;
pub(super) const EDIT_BAR: f32 = 28.0;
const COMPLETIONS_SHOWN: usize = 8;
const REFUSAL_SHOWN: f64 = 4.0;

#[derive(Clone, Debug)]
struct Completion {
    result: u64,
    items: Vec<Value>,
    selected: usize,
    start: usize,
    end: usize,
    asked: (String, usize),
}

#[derive(Clone, Debug)]
struct Preview {
    result: u64,
    sql: String,
    total: u64,
    confirm: bool,
}

#[derive(Clone, Debug)]
struct KeyDialog {
    result: u64,
    columns: Vec<(String, bool)>,
    remember: bool,
}

#[derive(Clone, Debug)]
struct ValuesDialog {
    result: u64,
    column: usize,
    name: String,
    values: Vec<(Value, bool)>,
    partial: bool,
}

#[derive(Default)]
pub(super) struct DataEdit {
    boxes: HashMap<u64, String>,
    focus: Option<u64>,
    completion: Option<Completion>,
    preview: Option<Preview>,
    key: Option<KeyDialog>,
    values: Option<ValuesDialog>,
    naming: Option<(u64, String)>,
    message: Option<(u64, String, bool)>,
    states: Option<(u64, u64, Value, Value)>,
    focus_next: bool,
    refused: Option<(u64, f64)>,
}

impl DataEdit {
    pub(super) fn popups(&self) -> Vec<String> {
        let mut open = Vec::new();
        if self.completion.is_some() {
            open.push("databaseFilterCompletion".to_owned());
        }
        if self.preview.is_some() {
            open.push("databasePreviewChanges".to_owned());
        }
        if self.key.is_some() {
            open.push("databaseRowKey".to_owned());
        }
        if self.values.is_some() {
            open.push("databaseColumnValues".to_owned());
        }
        if self.naming.is_some() {
            open.push("databaseSaveFilter".to_owned());
        }
        open
    }

    pub(super) fn states(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        id: u64,
    ) -> (Value, Value) {
        let pass = ctx.cumulative_pass_nr();
        if let Some((at, known, filter, edit)) = &self.states
            && *at == pass
            && *known == id
        {
            return (filter.clone(), edit.clone());
        }
        let filter =
            registry.dispatch(session, "db.result.filterState", json!({ "id": id, "limit": 20 })).unwrap_or_default();
        let edit = registry.dispatch(session, "db.result.editState", json!({ "id": id })).unwrap_or_default();
        if filter["query"].is_u64() || edit["saving"].is_u64() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.states = Some((pass, id, filter.clone(), edit.clone()));
        (filter, edit)
    }

    fn say(&mut self, id: u64, answer: Result<Value, zen_core::DispatchError>) -> Option<Value> {
        self.states = None;
        match answer {
            Ok(value) => {
                if self.message.as_ref().is_some_and(|(known, _, error)| *known == id && *error) {
                    self.message = None;
                }
                Some(value)
            }
            Err(error) => {
                self.message = Some((id, super::message(error), true));
                None
            }
        }
    }

    pub(super) fn focus_filter(&mut self) {
        self.focus_next = true;
    }

    pub(super) fn tell(&mut self, id: u64, text: String, error: bool) {
        self.message = Some((id, text, error));
    }

    pub(super) fn refuse(&mut self, id: u64) {
        self.refused = Some((id, f64::NAN));
    }

    pub(super) fn answer(&mut self, id: u64, answer: Result<Value, zen_core::DispatchError>) -> Option<Value> {
        self.say(id, answer)
    }
}

fn icon_button(
    ui: &mut Ui,
    p: &IdePalette,
    rect: Rect,
    label: &str,
    enabled: bool,
    tip: &str,
    salt: (&str, u64),
) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, ui.scope_id().with(salt), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let color = if enabled { p.text } else { p.text_secondary.gamma_multiply(0.5) };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::regular(13.0), color);
    accessibility::button_with(&response, || tip.to_owned());
    if !enabled {
        accessibility::disabled(&response);
    }
    response.on_hover_text(tip)
}

fn caret(ctx: &egui::Context, id: egui::Id, text: &str) -> usize {
    egui::TextEdit::load_state(ctx, id)
        .and_then(|state| state.cursor.char_range())
        .map_or_else(|| text.chars().count(), |range| range.primary.index.into())
}

fn set_caret(ctx: &egui::Context, id: egui::Id, at: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
        state.store(ctx, id);
    }
}

fn apply_filter(view: &mut DatabaseView, registry: &Registry, session: &mut Session, id: u64, condition: &str) {
    let answer = registry.dispatch(session, "db.result.filter", json!({ "id": id, "condition": condition }));
    view.results.data.completion = None;
    view.results.data.say(id, answer);
}

pub(super) fn filter_bar(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (id, filter): (u64, &Value),
    rect: Rect,
) {
    ui.painter().rect_filled(rect, 0, p.island);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.frame));
    let y = rect.top() + 4.0;
    let height = rect.height() - 8.0;
    let mut x = rect.left() + 6.0;
    let filterable = filter["filterable"] == true;
    let back = icon_button(
        ui,
        p,
        Rect::from_min_size(pos2(x, y), vec2(22.0, height)),
        "‹",
        filter["canGoBack"] == true,
        &t!("ide-database-filter-back"),
        ("database-filter-back", id),
    );
    x += 24.0;
    let forward = icon_button(
        ui,
        p,
        Rect::from_min_size(pos2(x, y), vec2(22.0, height)),
        "›",
        filter["canGoForward"] == true,
        &t!("ide-database-filter-forward"),
        ("database-filter-forward", id),
    );
    x += 28.0;
    if back.clicked() {
        let answer = registry.dispatch(session, "db.result.filterBack", json!({ "id": id }));
        view.results.data.say(id, answer);
    }
    if forward.clicked() {
        let answer = registry.dispatch(session, "db.result.filterForward", json!({ "id": id }));
        view.results.data.say(id, answer);
    }
    let condition = filter["condition"].as_str().unwrap_or_default().to_owned();
    let running = filter["query"].as_u64();
    let applied = filter["applied"].as_str().unwrap_or_default().to_owned();
    let side = (rect.width() * 0.3).clamp(0.0, 360.0);
    let field_right = rect.right() - side - 60.0;
    let field = Rect::from_min_max(pos2(x, y), pos2(field_right.max(x + 120.0), y + height));
    let failed = running.is_none() && filter["error"].is_string();
    let border = if failed { p.diagnostic_error } else { p.text_field_border };
    ui.painter().rect_filled(field, 4, p.text_field);
    ui.painter().rect_stroke(field, 4, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    ui.painter().text(
        pos2(field.left() + 8.0, field.center().y),
        Align2::LEFT_CENTER,
        "WHERE",
        theme::semibold(11.5),
        p.keyword,
    );
    if std::mem::take(&mut view.results.data.focus_next) {
        view.results.data.focus = Some(id);
    }
    let edit_id = ui.make_persistent_id(("database-filter", id));
    let text = view.results.data.boxes.entry(id).or_insert_with(|| condition.clone());
    let focused = ui.memory(|memory| memory.has_focus(edit_id));
    if !focused && view.results.data.focus != Some(id) && *text != condition && running.is_none() {
        *text = condition.clone();
    }
    let open = view
        .results
        .data
        .completion
        .as_ref()
        .is_some_and(|completion| completion.result == id && !completion.items.is_empty());
    let mut accept = false;
    if focused && open {
        let completion = view.results.data.completion.as_mut().expect("open");
        let shown = completion.items.len().min(COMPLETIONS_SHOWN);
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowDown)) {
            completion.selected = (completion.selected + 1) % shown.max(1);
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowUp)) {
            completion.selected = (completion.selected + shown.max(1) - 1) % shown.max(1);
        }
        accept = ui.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Enter) || input.consume_key(Modifiers::NONE, Key::Tab)
        });
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            view.results.data.completion = None;
        }
    }
    let force = focused && ui.input_mut(|input| input.consume_key(Modifiers::CTRL, Key::Space));
    let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(
        pos2(field.left() + 58.0, field.top() + 1.0),
        pos2(field.right() - 4.0, field.bottom() - 1.0),
    )));
    let text = view.results.data.boxes.get_mut(&id).expect("box");
    let hint = if filterable { t!("ide-database-filter-hint") } else { t!("ide-database-filter-not-filterable") };
    let response = child.add_enabled(
        filterable,
        egui::TextEdit::singleline(text)
            .id(edit_id)
            .frame(egui::Frame::NONE)
            .font(egui::FontId::monospace(12.5))
            .text_color(p.text)
            .hint_text(vcs::truncated(&hint, theme::regular(12.5), p.text_secondary, field.width() - 70.0))
            .desired_width(field.width() - 60.0),
    );
    if text.is_empty() {
        response.clone().on_hover_text(hint);
    }
    accessibility::labelled(&response, &t!("ide-database-filter-label"));
    if !applied.is_empty() {
        response.clone().on_hover_text(&applied);
    }
    if view.results.data.focus == Some(id) {
        view.results.data.focus = None;
        response.request_focus();
        set_caret(ui.ctx(), edit_id, text.chars().count());
    }
    let typed = text.clone();
    let caret_at = caret(ui.ctx(), edit_id, &typed);
    if accept && let Some(completion) = view.results.data.completion.take() {
        if let Some(item) = completion.items.get(completion.selected) {
            let insert = item["insert"].as_str().unwrap_or_default();
            let chars: Vec<char> = typed.chars().collect();
            let start = completion.start.min(chars.len());
            let end = completion.end.min(chars.len()).max(start);
            let replaced: String =
                chars[..start].iter().collect::<String>() + insert + &chars[end..].iter().collect::<String>();
            let at = start + insert.chars().count();
            view.results.data.boxes.insert(id, replaced);
            set_caret(ui.ctx(), edit_id, at);
        }
    } else if response.has_focus() && (response.changed() || force) {
        let word_start = typed
            .chars()
            .take(caret_at)
            .collect::<Vec<_>>()
            .iter()
            .rev()
            .take_while(|c| c.is_alphanumeric() || **c == '_' || **c == '"')
            .count();
        let asked = (typed.clone(), caret_at);
        let wanted = force || word_start > 0;
        let stale = view.results.data.completion.as_ref().is_none_or(|completion| completion.asked != asked);
        if wanted && stale {
            let answer = registry
                .dispatch(
                    session,
                    "db.result.completeFilter",
                    json!({ "id": id, "text": typed, "caret": caret_at, "limit": 30 }),
                )
                .unwrap_or_default();
            let items = answer["items"].as_array().cloned().unwrap_or_default();
            view.results.data.completion = Some(Completion {
                result: id,
                items,
                selected: 0,
                start: answer["start"].as_u64().unwrap_or(0) as usize,
                end: answer["end"].as_u64().unwrap_or(0) as usize,
                asked,
            });
        } else if !wanted {
            view.results.data.completion = None;
        }
    } else if response.lost_focus() {
        let enter = ui.input(|input| input.key_pressed(Key::Enter));
        let current = view.results.data.boxes.get(&id).cloned().unwrap_or_default();
        if enter {
            apply_filter(view, registry, session, id, &current);
        }
        view.results.data.completion = None;
    }
    if let Some(completion) = view
        .results
        .data
        .completion
        .clone()
        .filter(|completion| completion.result == id && !completion.items.is_empty() && response.has_focus())
    {
        let chosen = completion_popup(ui, p, &completion, field, edit_id);
        if let Some(chosen) = chosen
            && let Some(kept) = view.results.data.completion.as_mut()
        {
            kept.selected = chosen;
        }
    }
    x = field.right() + 4.0;
    let history_rect = Rect::from_min_size(pos2(x, y), vec2(22.0, height));
    let history = icon_button(
        ui,
        p,
        history_rect,
        "",
        filterable,
        &t!("ide-database-filter-history"),
        ("database-filter-history", id),
    );
    super::super::data_grid::chevron(
        ui.painter(),
        history_rect.center(),
        if filterable { p.text } else { p.text_secondary },
    );
    x += 24.0;
    let clear_rect = Rect::from_min_size(pos2(x, y), vec2(22.0, height));
    let clear = icon_button(
        ui,
        p,
        clear_rect,
        "",
        filterable && (!condition.is_empty() || !filter["sorts"].as_array().is_none_or(Vec::is_empty)),
        &t!("ide-database-filter-clear"),
        ("database-filter-clear", id),
    );
    super::super::glyphs::glyph_close(
        ui.painter(),
        Rect::from_center_size(clear_rect.center(), vec2(9.0, 9.0)),
        p.text_secondary,
    );
    x += 28.0;
    if clear.clicked() {
        view.results.data.boxes.insert(id, String::new());
        let answer = registry.dispatch(session, "db.result.resetFilter", json!({ "id": id }));
        view.results.data.say(id, answer);
    }
    let mut picked = None;
    egui::Popup::menu(&history).show(|ui| {
        ui.set_min_width(320.0);
        let listed = filter["history"].as_array().cloned().unwrap_or_default();
        if listed.is_empty() {
            ui.label(egui::RichText::new(t!("ide-database-filter-no-history")).color(p.text_secondary));
        }
        for condition in listed.iter().filter_map(Value::as_str) {
            if ui.button(egui::RichText::new(condition).font(egui::FontId::monospace(12.0))).clicked() {
                picked = Some(condition.to_owned());
                ui.close();
            }
        }
        let saved = filter["saved"].as_array().cloned().unwrap_or_default();
        if !saved.is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new(t!("ide-database-filter-saved"))
                    .font(theme::semibold(12.0))
                    .color(p.text_secondary),
            );
            for filter in &saved {
                let name = filter["name"].as_str().unwrap_or_default();
                let condition = filter["condition"].as_str().unwrap_or_default();
                if ui.button(name).on_hover_text(condition).clicked() {
                    picked = Some(condition.to_owned());
                    ui.close();
                }
            }
        }
        ui.separator();
        if ui.add_enabled(!condition.is_empty(), egui::Button::new(t!("ide-database-filter-save"))).clicked() {
            view.results.data.naming = Some((id, String::new()));
            ui.close();
        }
    });
    if let Some(condition) = picked {
        view.results.data.boxes.insert(id, condition.clone());
        apply_filter(view, registry, session, id, &condition);
    }
    let right = Rect::from_min_max(pos2(x, rect.top()), pos2(rect.right() - 8.0, rect.bottom()));
    if let Some(query) = running {
        let label = t!("ide-database-filter-running");
        ui.painter().text(
            pos2(right.left(), right.center().y),
            Align2::LEFT_CENTER,
            &label,
            theme::regular(12.0),
            p.text_secondary,
        );
        let width = ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.clone(), theme::regular(12.0), p.text).size().x);
        let cancel = icon_button(
            ui,
            p,
            Rect::from_min_size(pos2(right.left() + width + 8.0, y), vec2(60.0, height)),
            &t!("ide-database-cancel-button"),
            true,
            &t!("ide-database-cancel-button"),
            ("database-filter-cancel", id),
        );
        if cancel.clicked() {
            let _ = registry.dispatch(session, "db.query.cancel", json!({ "query": query }));
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    } else if let Some(error) = filter["error"].as_str() {
        let job = vcs::truncated(error, theme::regular(12.0), p.error_text, right.width());
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(right.left(), right.center().y - galley.size().y / 2.0), galley, p.error_text);
    } else if !applied.is_empty() {
        let one_line = applied.split_whitespace().collect::<Vec<_>>().join(" ");
        let job = vcs::truncated(&one_line, egui::FontId::monospace(11.5), p.text_secondary, right.width());
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(right.left(), right.center().y - galley.size().y / 2.0), galley, p.text_secondary);
        let label = ui.interact(right, ui.scope_id().with(("database-filter-applied", id)), Sense::hover());
        accessibility::name_with(&label, egui::Role::Label, || applied.clone());
        label.on_hover_text(applied);
    }
}

fn completion_popup(ui: &Ui, p: &IdePalette, completion: &Completion, field: Rect, edit: egui::Id) -> Option<usize> {
    let mut chosen = None;
    let shown = &completion.items[..completion.items.len().min(COMPLETIONS_SHOWN)];
    egui::Area::new(edit.with("completion"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(field.left() + 48.0, field.bottom() + 2.0))
        .show(ui.ctx(), |ui| {
            super::super::doc_popups::popup_frame(p).show(ui, |ui| {
                ui.set_width(300.0);
                for (at, item) in shown.iter().enumerate() {
                    let label = item["label"].as_str().unwrap_or_default();
                    let detail = item["detail"].as_str().unwrap_or_default();
                    let (rect, response) = ui.allocate_exact_size(vec2(300.0, 22.0), Sense::click());
                    if at == completion.selected {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.popup_hover);
                    }
                    let color = if item["kind"] == "keyword" { p.keyword } else { p.text };
                    ui.painter().text(
                        pos2(rect.left() + 8.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        egui::FontId::monospace(12.5),
                        color,
                    );
                    ui.painter().text(
                        pos2(rect.right() - 8.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        detail,
                        theme::regular(11.5),
                        p.text_secondary,
                    );
                    accessibility::item(&response, egui::Role::ListItem, at == completion.selected, label);
                    if response.hovered() {
                        chosen = Some(at);
                    }
                }
            });
        });
    chosen
}

fn reason_offers_key(edit: &Value) -> bool {
    edit["reason"] == "noKey"
}

pub(super) fn edit_bar(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (id, edit): (u64, &Value),
    rect: Rect,
) {
    ui.painter().rect_filled(rect, 0, p.island);
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.frame));
    let font = theme::regular(12.0);
    let y = rect.top() + 3.0;
    let height = rect.height() - 6.0;
    let mut x = rect.left() + 8.0;
    let text_at = |ui: &Ui, x: f32, text: &str, color| {
        ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, text, theme::regular(12.0), color);
        x + ui.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), theme::regular(12.0), color).size().x) + 10.0
    };
    let button = |ui: &mut Ui, x: &mut f32, label: String, salt: &str| {
        let width = ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.clone(), font.clone(), p.text).size().x) + 18.0;
        let at = Rect::from_min_size(pos2(*x, y), vec2(width, height));
        *x = at.right() + 4.0;
        let response = ui.interact(at, ui.scope_id().with((salt, id)), Sense::click());
        ui.painter().rect_stroke(at, 4, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
        if response.hovered() {
            ui.painter().rect_filled(at, 4, p.hover);
        }
        ui.painter().text(at.center(), Align2::CENTER_CENTER, &label, font.clone(), p.text);
        accessibility::button_with(&response, || label.clone());
        response
    };
    let pending = &edit["pending"];
    if view.results.clip.job_of(id).is_some() {
        x = text_at(ui, x, &t!("ide-database-copy-running"), p.text_secondary);
        if button(ui, &mut x, t!("ide-database-copy-cancel"), "database-copy-cancel").clicked() {
            super::clipboard::cancel(view, registry, session, id);
        }
        return;
    }
    if edit["saving"].is_u64() {
        text_at(ui, x, &t!("ide-database-edit-saving"), p.text_secondary);
        return;
    }
    if edit["dirty"] == true {
        let summary = t!(
            "ide-database-edit-pending",
            updated = pending["updated"].as_u64().unwrap_or(0),
            inserted = pending["inserted"].as_u64().unwrap_or(0),
            deleted = pending["deleted"].as_u64().unwrap_or(0)
        );
        let save_key = crate::keymap::shortcut("db.result.save").map(|key| format!(" {key}")).unwrap_or_default();
        let save_label = format!("{}{save_key}", t!("ide-database-edit-save"));
        let buttons: f32 = [save_label.clone(), t!("ide-database-edit-cancel"), t!("ide-database-edit-preview")]
            .iter()
            .map(|label| {
                ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.clone(), font.clone(), p.text).size().x) + 22.0
            })
            .sum();
        let room = (rect.right() - x - buttons - 16.0).max(40.0);
        let job = vcs::truncated(&summary, theme::regular(12.0), p.text, room);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let width = galley.size().x;
        ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, p.text);
        if width < ui.fonts_mut(|fonts| fonts.layout_no_wrap(summary.clone(), theme::regular(12.0), p.text).size().x) {
            let label = ui.interact(
                Rect::from_min_size(pos2(x, rect.top()), vec2(width, rect.height())),
                ui.scope_id().with(("database-edit-summary", id)),
                Sense::hover(),
            );
            label.on_hover_text(&summary);
        }
        x += width + 10.0;
        if button(ui, &mut x, save_label, "database-edit-save").clicked() {
            save(view, registry, session, id, edit);
        }
        if button(ui, &mut x, t!("ide-database-edit-cancel"), "database-edit-cancel").clicked() {
            let answer = registry.dispatch(session, "db.result.revert", json!({ "id": id }));
            view.results.data.say(id, answer);
        }
        if button(ui, &mut x, t!("ide-database-edit-preview"), "database-edit-preview").clicked() {
            open_preview(view, registry, session, id, false);
        }
    } else if let Some(message) = edit["message"].as_str() {
        x = read_only_chip(ui, p, x, rect);
        let now = ui.input(|input| input.time);
        let flashing = match view.results.data.refused.as_mut() {
            Some((known, at)) if *known == id => {
                if at.is_nan() {
                    *at = now;
                }
                now - *at < REFUSAL_SHOWN
            }
            _ => false,
        };
        if flashing {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        } else if view.results.data.refused.is_some_and(|(known, _)| known == id) {
            view.results.data.refused = None;
        }
        let room = (rect.right() - x - 8.0).max(40.0);
        let color = if flashing { p.error_text } else { p.text_secondary };
        let job = vcs::truncated(message, theme::regular(12.0), color, room);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let width = galley.size().x;
        ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
        let label = ui.interact(
            Rect::from_min_size(pos2(x, rect.top()), vec2(width, rect.height())),
            ui.scope_id().with(("database-edit-reason", id)),
            Sense::hover(),
        );
        accessibility::name_with(&label, egui::Role::Label, || message.to_owned());
        label.on_hover_text(message);
        x += width + 10.0;
        if reason_offers_key(edit)
            && button(ui, &mut x, t!("ide-database-edit-choose-key"), "database-edit-key").clicked()
        {
            open_key_dialog(view, id, edit);
        }
    }
    let last = &edit["lastSave"];
    let note = match (&view.results.data.message, last["status"].as_str()) {
        (Some((known, message, error)), _) if *known == id => Some((message.clone(), *error)),
        (_, Some("failed" | "cancelled")) => Some((last["message"].as_str().unwrap_or_default().to_owned(), true)),
        (_, Some("done")) if edit["dirty"] != true => {
            let report = &last["report"];
            let mut text = t!(
                "ide-database-edit-saved",
                updated = report["updated"].as_u64().unwrap_or(0),
                inserted = report["inserted"].as_u64().unwrap_or(0),
                deleted = report["deleted"].as_u64().unwrap_or(0)
            );
            if report["inTransaction"] == true {
                text = format!("{text} {}", t!("ide-database-edit-in-transaction"));
            }
            Some((text, false))
        }
        _ => None,
    };
    if let Some((note, error)) = note {
        let color = if error { p.error_text } else { p.text_secondary };
        let job = vcs::truncated(&note, theme::regular(12.0), color, (rect.right() - x - 8.0).max(40.0));
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(x + 6.0, rect.center().y - galley.size().y / 2.0), galley, color);
    }
}

fn read_only_chip(ui: &Ui, p: &IdePalette, x: f32, rect: Rect) -> f32 {
    let label = t!("ide-database-edit-read-only");
    let font = theme::regular(11.5);
    let width = ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.clone(), font.clone(), p.text).size().x) + 30.0;
    let chip = Rect::from_min_size(pos2(x, rect.top() + 5.0), vec2(width, rect.height() - 10.0));
    ui.painter().rect_filled(chip, 4, p.hover);
    super::super::glyphs::glyph_lock(
        ui.painter(),
        Rect::from_center_size(pos2(chip.left() + 11.0, chip.center().y), vec2(10.0, 11.0)),
        p.text_secondary,
    );
    ui.painter().text(pos2(chip.left() + 20.0, chip.center().y), Align2::LEFT_CENTER, label, font, p.text);
    chip.right() + 8.0
}

pub(super) fn wants_edit_bar(view: &DatabaseView, id: u64, edit: &Value) -> bool {
    edit["dirty"] == true
        || view.results.clip.job_of(id).is_some()
        || edit["saving"].is_u64()
        || edit["reason"].is_string()
        || edit["lastSave"].is_object()
        || view.results.data.message.as_ref().is_some_and(|(known, _, _)| *known == id)
}

fn open_preview(view: &mut DatabaseView, registry: &Registry, session: &mut Session, id: u64, confirm: bool) {
    let answer = registry.dispatch(session, "db.result.previewChanges", json!({ "id": id, "limit": 500 }));
    if let Some(answer) = view.results.data.say(id, answer) {
        view.results.data.preview = Some(Preview {
            result: id,
            sql: answer["sql"].as_str().unwrap_or_default().to_owned(),
            total: answer["total"].as_u64().unwrap_or(0),
            confirm: confirm || answer["confirm"] == true,
        });
    }
}

fn save(view: &mut DatabaseView, registry: &Registry, session: &mut Session, id: u64, edit: &Value) {
    if edit["confirm"] == true {
        open_preview(view, registry, session, id, true);
        return;
    }
    let answer = registry.dispatch(session, "db.result.save", json!({ "id": id }));
    view.results.data.say(id, answer);
}

fn open_key_dialog(view: &mut DatabaseView, id: u64, edit: &Value) {
    let columns = edit["columns"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|column| column["sourced"] == true)
        .filter_map(|column| Some((column["name"].as_str()?.to_owned(), false)))
        .collect();
    view.results.data.key = Some(KeyDialog { result: id, columns, remember: true });
}

pub(super) fn act(
    view: &mut DatabaseView,
    ui: &Ui,
    registry: &Registry,
    session: &mut Session,
    (id, filter, edit): (u64, &Value, &Value),
    names: &[String],
    outcome: &GridOutcome,
) -> Option<u64> {
    let mut select = None;
    if let Some((column, add)) = outcome.sort {
        let answer = registry.dispatch(session, "db.result.sort", json!({ "id": id, "column": column, "add": add }));
        view.results.data.say(id, answer);
    }
    for action in &outcome.actions {
        let (command, params) = match action {
            GridAction::Commit { row, column, text } => {
                ("db.result.setCell", json!({ "id": id, "row": row, "column": column, "text": text }))
            }
            GridAction::Revert { row, column } => {
                ("db.result.revertCell", json!({ "id": id, "row": row, "column": column }))
            }
            GridAction::SetNull { row, column } => {
                ("db.result.setCell", json!({ "id": id, "row": row, "column": column, "null": true }))
            }
            GridAction::SetDefault { row, column } => {
                ("db.result.setCell", json!({ "id": id, "row": row, "column": column, "default": true }))
            }
            GridAction::CopyAbove { row } => ("db.result.copyFromAbove", json!({ "id": id, "row": row })),
            GridAction::AddRow => ("db.result.addRow", json!({ "id": id })),
            GridAction::Duplicate { row } => ("db.result.duplicateRow", json!({ "id": id, "row": row })),
            GridAction::Delete { rows } => ("db.result.deleteRows", json!({ "id": id, "rows": rows })),
            GridAction::Restore { rows } => {
                ("db.result.deleteRows", json!({ "id": id, "rows": rows, "restore": true }))
            }
            GridAction::Filter { row, column, operator } => {
                ("db.result.filterByValue", json!({ "id": id, "row": row, "column": column, "operator": operator }))
            }
            GridAction::FilterIn { rows, column } => (
                "db.result.filterByValue",
                json!({ "id": id, "row": rows.first(), "rows": rows.get(1..).unwrap_or_default(), "column": column, "operator": "in" }),
            ),
            GridAction::FilterClipboard { column, operator } => {
                let value = zen_workbench::file_clipboard::clipboard_text(session);
                ("db.result.filterByValue", json!({ "id": id, "column": column, "operator": operator, "value": value }))
            }
            GridAction::FilterCustom { column } => {
                let quoted = zen_db::schema::quote(names.get(*column).map_or("", String::as_str));
                let current = filter["condition"].as_str().unwrap_or_default();
                let text =
                    if current.is_empty() { format!("{quoted} = ") } else { format!("{current} AND {quoted} = ") };
                view.results.data.boxes.insert(id, text);
                view.results.data.focus = Some(id);
                continue;
            }
            GridAction::FilterValues { column } => {
                let answer = registry.dispatch(
                    session,
                    "db.result.distinctValues",
                    json!({ "id": id, "column": column, "limit": 200 }),
                );
                if let Some(answer) = view.results.data.say(id, answer) {
                    let name = edit["columns"][*column]["name"].as_str().unwrap_or_default().to_owned();
                    view.results.data.values = Some(ValuesDialog {
                        result: id,
                        column: *column,
                        name,
                        values: answer["values"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|value| (value, false))
                            .collect(),
                        partial: answer["partial"] == true,
                    });
                }
                continue;
            }
            GridAction::ResetColumn { column } => ("db.result.resetFilter", json!({ "id": id, "column": column })),
            GridAction::Generate { kind, rows } => {
                let params = json!({ "id": id, "kind": kind, "rows": [rows.start, rows.end.saturating_sub(1)] });
                let answer = registry.dispatch(session, "db.result.generateSql", params);
                if let Some(answer) = view.results.data.say(id, answer) {
                    ui.ctx().copy_text(answer["sql"].as_str().unwrap_or_default().to_owned());
                    view.results.data.message = Some((id, t!("ide-database-edit-generated"), false));
                }
                continue;
            }
            GridAction::Save => {
                save(view, registry, session, id, edit);
                continue;
            }
            GridAction::Preview => {
                open_preview(view, registry, session, id, false);
                continue;
            }
            GridAction::CopyAs { .. }
            | GridAction::CopyAdvanced { .. }
            | GridAction::Paste { .. }
            | GridAction::PasteAdvanced { .. }
            | GridAction::FollowKey { .. }
            | GridAction::OpenReferences { .. }
            | GridAction::Lookup { .. }
            | GridAction::GoToRow
            | GridAction::FetchNext
            | GridAction::FetchAll
            | GridAction::ToggleValuePanel
            | GridAction::View { .. }
            | GridAction::Export => continue,
            GridAction::SortBy { column, direction } => {
                ("db.result.sort", json!({ "id": id, "column": column, "direction": direction }))
            }
        };
        let answer = registry.dispatch(session, command, params);
        if let Some(answer) = view.results.data.say(id, answer)
            && let Some(row) = answer["row"].as_u64()
        {
            select = Some(row);
        }
    }
    select
}

pub(super) fn dialogs(view: &mut DatabaseView, ui: &Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
    if let Some(preview) = view.results.data.preview.clone() {
        let mut close = false;
        let modal = egui::Modal::new(ui.scope_id().with("database-preview")).frame(super::super::dialog_frame(p)).show(
            ui.ctx(),
            |ui| {
                ui.set_width(720.0);
                ui.label(
                    egui::RichText::new(t!("ide-database-preview-title")).font(theme::semibold(15.0)).color(p.text),
                );
                ui.add_space(4.0);
                let note = if preview.confirm {
                    t!("ide-database-preview-confirm", count = preview.total)
                } else {
                    t!("ide-database-preview-note", count = preview.total)
                };
                ui.label(egui::RichText::new(note).font(theme::regular(12.5)).color(if preview.confirm {
                    p.warning_text
                } else {
                    p.text_secondary
                }));
                ui.add_space(10.0);
                let (area, _) = ui.allocate_exact_size(vec2(720.0, 320.0), Sense::hover());
                ui.painter().rect_filled(area, 6, p.editor_background);
                super::object::code_view(
                    ui,
                    p,
                    area.shrink(6.0),
                    &preview.sql,
                    super::code::Lang::Sql,
                    "database-preview-sql",
                    &t!("ide-database-preview-title"),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if vcs::primary_button(ui, p, &t!("ide-database-edit-save")).clicked() {
                        let params = json!({ "id": preview.result, "confirmed": true });
                        let answer = registry.dispatch(session, "db.result.save", params);
                        view.results.data.say(preview.result, answer);
                        close = true;
                    }
                    if vcs::secondary_button(ui, p, &t!("ide-database-preview-copy")).clicked() {
                        ui.ctx().copy_text(preview.sql.clone());
                    }
                    if vcs::secondary_button(ui, p, &t!("ide-database-preview-close")).clicked() {
                        close = true;
                    }
                });
            },
        );
        if close || modal.should_close() {
            view.results.data.preview = None;
        }
    }
    if let Some(mut dialog) = view.results.data.key.clone() {
        let mut close = false;
        let mut chosen = None;
        let modal = egui::Modal::new(ui.scope_id().with("database-row-key")).frame(super::super::dialog_frame(p)).show(
            ui.ctx(),
            |ui| {
                ui.set_width(420.0);
                ui.label(egui::RichText::new(t!("ide-database-key-title")).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t!("ide-database-key-note"))
                            .font(theme::regular(12.5))
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);
                for (name, on) in &mut dialog.columns {
                    ui.checkbox(on, name.as_str());
                }
                ui.add_space(6.0);
                ui.checkbox(&mut dialog.remember, t!("ide-database-key-remember"));
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t!("ide-database-key-all-warning"))
                            .font(theme::regular(12.0))
                            .color(p.warning_text),
                    )
                    .wrap(),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let picked: Vec<String> =
                        dialog.columns.iter().filter(|(_, on)| *on).map(|(name, _)| name.clone()).collect();
                    if ui.add_enabled(!picked.is_empty(), egui::Button::new(t!("ide-database-key-custom"))).clicked() {
                        chosen = Some(json!({ "kind": "columns", "columns": picked }));
                    }
                    if ui.button(t!("ide-database-key-all")).clicked() {
                        chosen = Some(json!({ "kind": "allColumns" }));
                    }
                    if ui.button(t!("ide-database-preview-close")).clicked() {
                        close = true;
                    }
                });
            },
        );
        if let Some(key) = chosen {
            let remember = dialog.remember && key["kind"] == "columns";
            let answer = registry.dispatch(
                session,
                "db.result.setRowKey",
                json!({ "id": dialog.result, "key": key, "remember": remember }),
            );
            view.results.data.say(dialog.result, answer);
            close = true;
        }
        view.results.data.key = if close || modal.should_close() { None } else { Some(dialog) };
    }
    if let Some(mut dialog) = view.results.data.values.clone() {
        let mut close = false;
        let mut apply = false;
        let modal = egui::Modal::new(ui.scope_id().with("database-values")).frame(super::super::dialog_frame(p)).show(
            ui.ctx(),
            |ui| {
                ui.set_width(420.0);
                ui.label(
                    egui::RichText::new(t!("ide-database-values-title", column = dialog.name.as_str()))
                        .font(theme::semibold(15.0))
                        .color(p.text),
                );
                if dialog.partial {
                    ui.label(
                        egui::RichText::new(t!("ide-database-values-partial"))
                            .font(theme::regular(12.0))
                            .color(p.text_secondary),
                    );
                }
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (value, on) in &mut dialog.values {
                        let text = value["text"].as_str().map_or_else(|| "[NULL]".to_owned(), str::to_owned);
                        ui.checkbox(on, format!("{text}  ({})", value["count"].as_u64().unwrap_or(0)));
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let any = dialog.values.iter().any(|(_, on)| *on);
                    if ui.add_enabled(any, egui::Button::new(t!("ide-database-values-apply"))).clicked() {
                        apply = true;
                    }
                    if ui.button(t!("ide-database-preview-close")).clicked() {
                        close = true;
                    }
                });
            },
        );
        if apply {
            let rows: Vec<u64> =
                dialog.values.iter().filter(|(_, on)| *on).filter_map(|(value, _)| value["row"].as_u64()).collect();
            let params = json!({ "id": dialog.result, "column": dialog.column, "row": rows.first(), "rows": rows.get(1..).unwrap_or_default(), "operator": "in" });
            let answer = registry.dispatch(session, "db.result.filterByValue", params);
            view.results.data.say(dialog.result, answer);
            close = true;
        }
        view.results.data.values = if close || modal.should_close() { None } else { Some(dialog) };
    }
    if let Some((result, mut name)) = view.results.data.naming.clone() {
        let mut close = false;
        let mut keep = false;
        let modal = egui::Modal::new(ui.scope_id().with("database-save-filter"))
            .frame(super::super::dialog_frame(p))
            .show(ui.ctx(), |ui| {
                ui.set_width(360.0);
                ui.label(egui::RichText::new(t!("ide-database-filter-save")).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(8.0);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text(t!("ide-database-filter-name"))
                        .desired_width(340.0),
                );
                accessibility::labelled(&field, &t!("ide-database-filter-name"));
                field.request_focus();
                keep = ui.input(|input| input.key_pressed(Key::Enter)) && !name.trim().is_empty();
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!name.trim().is_empty(), egui::Button::new(t!("ide-database-filter-save")))
                        .clicked()
                    {
                        keep = true;
                    }
                    if ui.button(t!("ide-database-preview-close")).clicked() {
                        close = true;
                    }
                });
            });
        if keep {
            let answer = registry.dispatch(session, "db.filter.save", json!({ "id": result, "name": name.trim() }));
            view.results.data.say(result, answer);
            close = true;
        }
        view.results.data.naming = if close || modal.should_close() { None } else { Some((result, name)) };
    }
}
