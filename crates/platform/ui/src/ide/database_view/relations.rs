use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Key, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::super::data_grid::{GridAction, GridOutcome};
use super::super::glyphs::{glyph_back, glyph_forward};
use super::super::transaction_dialog;
use super::super::{IdePalette, theme};
use super::DatabaseView;
use crate::accessibility;

const ASK_AGAIN: Duration = Duration::from_millis(500);
const KEPT: usize = 64;
const SHOWN_CHARS: usize = 32;

fn shortened(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() > SHOWN_CHARS || line.len() < text.len() {
        format!("{}…", line.chars().take(SHOWN_CHARS).collect::<String>())
    } else {
        line.to_owned()
    }
}

struct LookupView {
    result: u64,
    row: u64,
    column: usize,
    search: String,
    asked: String,
    focused: bool,
}

#[derive(Default)]
pub(super) struct Links {
    known: HashMap<u64, (Instant, Value)>,
    navigation: Option<(u64, Value)>,
    lookup: Option<LookupView>,
}

impl Links {
    pub(super) fn popups(&self) -> Vec<String> {
        self.lookup.iter().map(|_| "databaseKeyLookup".to_owned()).collect()
    }
}

pub(super) fn relations(view: &mut DatabaseView, registry: &Registry, session: &mut Session, id: u64) -> Value {
    let links = &mut view.results.links;
    if let Some((at, known)) = links.known.get(&id)
        && (known["reading"] != true || at.elapsed() < ASK_AGAIN)
    {
        return known.clone();
    }
    let answer =
        registry.dispatch(session, "db.result.relations", json!({ "id": id, "limit": 50 })).unwrap_or_default();
    if links.known.len() >= KEPT {
        links.known.clear();
    }
    links.known.insert(id, (Instant::now(), answer.clone()));
    answer
}

pub(super) fn labels(relations: &Value, columns: usize) -> (Vec<Option<String>>, Vec<String>) {
    let mut links = vec![None; columns];
    for link in relations["links"].as_array().into_iter().flatten() {
        let table = link["table"].as_str().unwrap_or_default();
        for column in link["columns"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            if let Some(slot) = usize::try_from(column).ok().and_then(|column| links.get_mut(column))
                && slot.is_none()
            {
                *slot = Some(table.to_owned());
            }
        }
    }
    let references = relations["references"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|reference| {
            let names: Vec<&str> =
                reference["otherColumns"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            format!("{} ({})", reference["table"].as_str().unwrap_or_default(), names.join(", "))
        })
        .collect();
    (links, references)
}

fn run(
    view: &mut DatabaseView,
    registry: &Registry,
    session: &mut Session,
    id: u64,
    command: &str,
    params: Value,
) -> Option<Value> {
    match registry.dispatch(session, command, params) {
        Ok(answer) => {
            view.results.links.navigation = None;
            Some(answer)
        }
        Err(error) => {
            if !transaction_dialog::ask(session, &error) {
                view.results.data.tell(id, super::message(error), true);
            }
            None
        }
    }
}

pub(super) fn act(
    view: &mut DatabaseView,
    registry: &Registry,
    session: &mut Session,
    (id, relations): (u64, &Value),
    outcome: &GridOutcome,
) {
    for action in &outcome.actions {
        match action {
            GridAction::FollowKey { row, column } => {
                let params = json!({ "id": id, "row": row, "column": column });
                run(view, registry, session, id, "db.result.openReferenced", params);
            }
            GridAction::OpenReferences { row, reference } => {
                let found = &relations["references"][*reference];
                let params = json!({ "id": id, "row": row, "table": found["table"], "key": found["name"] });
                run(view, registry, session, id, "db.result.openReferencing", params);
            }
            GridAction::Lookup { row, column } => {
                let params = json!({ "id": id, "row": row, "column": column, "search": "" });
                if run(view, registry, session, id, "db.lookup.start", params).is_some() {
                    view.results.links.lookup = Some(LookupView {
                        result: id,
                        row: *row,
                        column: *column,
                        search: String::new(),
                        asked: String::new(),
                        focused: false,
                    });
                }
            }
            _ => {}
        }
    }
}

fn arrow_button(ui: &mut Ui, p: &IdePalette, rect: Rect, back: bool, (enabled, tip): (bool, &str)) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, ui.scope_id().with(("database-data-navigation", back)), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let color = if enabled { p.text } else { p.text_secondary.gamma_multiply(0.5) };
    let glyph = Rect::from_center_size(rect.center(), vec2(11.0, 9.0));
    if back {
        glyph_back(ui.painter(), glyph, color);
    } else {
        glyph_forward(ui.painter(), glyph, color);
    }
    accessibility::button_with(&response, || tip.to_owned());
    if !enabled {
        accessibility::disabled(&response);
    }
    response.on_hover_text(tip)
}

pub(super) fn navigation_buttons(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    (id, x, y, height): (u64, f32, f32, f32),
) -> f32 {
    let pass = ui.ctx().cumulative_pass_nr();
    let state = match &view.results.links.navigation {
        Some((at, state)) if *at == pass => state.clone(),
        _ => {
            let state = registry.dispatch(session, "db.data.navigation", json!({})).unwrap_or_default();
            view.results.links.navigation = Some((pass, state.clone()));
            state
        }
    };
    let titled = |key: &str, list: &str| match state[list][0].as_str() {
        Some(title) => format!("{} · {title}", t!(key)),
        None => t!(key),
    };
    let back_tip = titled("ide-database-data-back", "back");
    let forward_tip = titled("ide-database-data-forward", "forward");
    let back = arrow_button(
        ui,
        p,
        Rect::from_min_size(pos2(x, y), vec2(24.0, height)),
        true,
        (state["canGoBack"] == true, &back_tip),
    );
    let forward = arrow_button(
        ui,
        p,
        Rect::from_min_size(pos2(x + 26.0, y), vec2(24.0, height)),
        false,
        (state["canGoForward"] == true, &forward_tip),
    );
    if back.clicked() {
        run(view, registry, session, id, "db.data.back", json!({}));
    }
    if forward.clicked() {
        run(view, registry, session, id, "db.data.forward", json!({}));
    }
    52.0
}

pub(super) fn lookup_popup(
    view: &mut DatabaseView,
    ui: &Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
) {
    let Some(mut lookup) = view.results.links.lookup.take() else { return };
    let state = registry.dispatch(session, "db.lookup.state", json!({})).unwrap_or_default();
    if state["open"] != true {
        return;
    }
    if state["status"] == "running" {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
    let mut chosen = None;
    let mut close = false;
    let title = t!(
        "ide-database-lookup-title",
        key = state["key"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
        table = state["table"].as_str().unwrap_or_default()
    );
    let modal = egui::Modal::new(ui.scope_id().with("database-key-lookup")).frame(super::super::dialog_frame(p)).show(
        ui.ctx(),
        |ui| {
            ui.set_width(560.0);
            ui.label(egui::RichText::new(&title).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(8.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut lookup.search)
                    .hint_text(t!("ide-database-lookup-search"))
                    .desired_width(540.0),
            );
            accessibility::labelled(&field, &t!("ide-database-lookup-search"));
            if !lookup.focused {
                field.request_focus();
                lookup.focused = true;
            }
            ui.add_space(8.0);
            let headers: Vec<String> = state["headers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            let rows = state["rows"].as_array().cloned().unwrap_or_default();
            egui::ScrollArea::both().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("database-key-lookup-rows").striped(true).spacing(vec2(14.0, 4.0)).show(ui, |ui| {
                    for header in &headers {
                        ui.label(egui::RichText::new(header).font(theme::semibold(12.5)).color(p.text));
                    }
                    ui.end_row();
                    for (index, row) in rows.iter().enumerate() {
                        for (at, cell) in row.as_array().into_iter().flatten().enumerate() {
                            let (text, color) = match cell.as_str() {
                                Some(text) => (shortened(text), if at == 0 { p.link } else { p.text }),
                                None => ("[NULL]".to_owned(), p.text_secondary),
                            };
                            let label =
                                egui::Label::new(egui::RichText::new(text).font(theme::regular(12.5)).color(color))
                                    .sense(Sense::click())
                                    .extend();
                            let response = ui.add(label);
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if response.clicked() {
                                chosen = Some(index);
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(6.0);
            let status = match state["status"].as_str() {
                Some("running") => t!("ide-database-lookup-reading"),
                Some("failed") => state["error"].as_str().unwrap_or_default().to_owned(),
                _ if state["truncated"] == true => t!("ide-database-lookup-truncated", rows = rows.len()),
                _ => t!("ide-database-lookup-rows", rows = rows.len()),
            };
            let color = if state["status"] == "failed" { p.error_text } else { p.text_secondary };
            ui.label(egui::RichText::new(status).font(theme::regular(12.0)).color(color));
            ui.add_space(10.0);
            if ui.button(t!("ide-database-preview-close")).clicked() {
                close = true;
            }
        },
    );
    if lookup.search != lookup.asked {
        lookup.asked.clone_from(&lookup.search);
        let params =
            json!({ "id": lookup.result, "row": lookup.row, "column": lookup.column, "search": lookup.search });
        run(view, registry, session, lookup.result, "db.lookup.start", params);
    }
    if let Some(row) = chosen {
        run(view, registry, session, lookup.result, "db.lookup.choose", json!({ "row": row }));
        return;
    }
    if close || modal.should_close() || ui.input(|input| input.key_pressed(Key::Escape)) {
        let _ = registry.dispatch(session, "db.lookup.close", json!({}));
        return;
    }
    view.results.links.lookup = Some(lookup);
}
