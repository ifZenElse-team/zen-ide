use egui::{Align, Align2, Color32, FontId, Layout, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::workspaces::InnerTab;

use super::super::database_navigator::type_color;
use super::super::glyphs::{glyph_copy, glyph_run, glyph_search, glyph_settings};
use super::super::{IdePalette, customization, theme, vcs};
use super::{Listed, message};
use crate::accessibility;

const BAR: f32 = 36.0;

const ROW: f32 = 26.0;

const LISTED: usize = 500;

const DETAIL: f32 = 0.4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    All,
    Statement,
    Script,
    Filter,
    Data,
    Save,
    Transaction,
    Export,
    Import,
    Session,
    Structure,
    Search,
    Agent,
}

impl Kind {
    const ALL: [Kind; 13] = [
        Kind::All,
        Kind::Statement,
        Kind::Script,
        Kind::Filter,
        Kind::Data,
        Kind::Save,
        Kind::Transaction,
        Kind::Export,
        Kind::Import,
        Kind::Session,
        Kind::Structure,
        Kind::Search,
        Kind::Agent,
    ];

    fn label(self) -> String {
        match self {
            Kind::All => t!("ide-dbhistory-all-types"),
            Kind::Statement => t!("ide-dbhistory-type-statement"),
            Kind::Script => t!("ide-dbhistory-type-script"),
            Kind::Filter => t!("ide-dbhistory-type-filter"),
            Kind::Data => t!("ide-dbhistory-type-data"),
            Kind::Save => t!("ide-dbhistory-type-save"),
            Kind::Transaction => t!("ide-dbhistory-type-transaction"),
            Kind::Export => t!("ide-dbhistory-type-export"),
            Kind::Import => t!("ide-dbhistory-type-import"),
            Kind::Session => t!("ide-dbhistory-type-session"),
            Kind::Structure => t!("ide-dbhistory-type-structure"),
            Kind::Search => t!("ide-dbhistory-type-search"),
            Kind::Agent => t!("ide-dbhistory-type-agent"),
        }
    }

    fn params(self, params: &mut Value) {
        let source = match self {
            Kind::All => return,
            Kind::Agent => {
                params["agent"] = json!(true);
                return;
            }
            Kind::Statement => "statement",
            Kind::Script => "script",
            Kind::Filter => "filter",
            Kind::Data => "data",
            Kind::Save => "save",
            Kind::Transaction => "transaction",
            Kind::Export => "export",
            Kind::Import => "import",
            Kind::Session => "session",
            Kind::Structure => "structure",
            Kind::Search => "search",
        };
        params["sources"] = json!([source]);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Any,
    Done,
    Failed,
    Cancelled,
    Refused,
}

impl Ending {
    const ALL: [Ending; 5] = [Ending::Any, Ending::Done, Ending::Failed, Ending::Cancelled, Ending::Refused];

    fn label(self) -> String {
        match self {
            Ending::Any => t!("ide-dbhistory-any-outcome"),
            Ending::Done => t!("ide-dbhistory-outcome-done"),
            Ending::Failed => t!("ide-dbhistory-outcome-failed"),
            Ending::Cancelled => t!("ide-dbhistory-outcome-cancelled"),
            Ending::Refused => t!("ide-dbhistory-outcome-refused"),
        }
    }

    fn id(self) -> Option<&'static str> {
        match self {
            Ending::Any => None,
            Ending::Done => Some("done"),
            Ending::Failed => Some("failed"),
            Ending::Cancelled => Some("cancelled"),
            Ending::Refused => Some("refused"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum When {
    Any,
    Today,
    Week,
    Month,
}

impl When {
    const ALL: [When; 4] = [When::Any, When::Today, When::Week, When::Month];

    fn label(self) -> String {
        match self {
            When::Any => t!("ide-dbhistory-any-time"),
            When::Today => t!("ide-dbhistory-today"),
            When::Week => t!("ide-dbhistory-week"),
            When::Month => t!("ide-dbhistory-month"),
        }
    }

    fn id(self) -> Option<&'static str> {
        match self {
            When::Any => None,
            When::Today => Some("today"),
            When::Week => Some("week"),
            When::Month => Some("month"),
        }
    }
}

struct Shown {
    params: Value,
    revision: u64,
    answer: Value,
}

pub(super) struct HistoryView {
    query: String,
    connection: Option<String>,
    kind: Kind,
    ending: Ending,
    when: When,
    preset: Value,
    shown: Option<Shown>,
    selected: Option<u64>,
    detail: Option<Value>,
    confirm_clear: bool,
    error: Option<String>,
    settings: Option<Value>,
}

impl Default for HistoryView {
    fn default() -> Self {
        Self {
            query: String::new(),
            connection: None,
            kind: Kind::All,
            ending: Ending::Any,
            when: When::Any,
            preset: Value::Null,
            shown: None,
            selected: None,
            detail: None,
            confirm_clear: false,
            error: None,
            settings: None,
        }
    }
}

pub(super) fn glyph_history(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new((rect.width() / 12.0).max(1.2), color);
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.44;
    painter.circle_stroke(center, radius, stroke);
    painter.line_segment([center, center - vec2(0.0, radius * 0.62)], stroke);
    painter.line_segment([center, center + vec2(radius * 0.48, radius * 0.18)], stroke);
}

fn source_label(source: &str) -> String {
    match source {
        "statement" => t!("ide-dbhistory-source-statement"),
        "script" => t!("ide-dbhistory-source-script"),
        "filter" => t!("ide-dbhistory-source-filter"),
        "data" => t!("ide-dbhistory-source-data"),
        "save" => t!("ide-dbhistory-source-save"),
        "transaction" => t!("ide-dbhistory-source-transaction"),
        "export" => t!("ide-dbhistory-source-export"),
        "import" => t!("ide-dbhistory-source-import"),
        "session" => t!("ide-dbhistory-source-session"),
        "structure" => t!("ide-dbhistory-source-structure"),
        "connections" => t!("ide-dbhistory-source-connections"),
        "search" => t!("ide-dbhistory-source-search"),
        _ => t!("ide-dbhistory-source-query"),
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn outcome_color(p: &IdePalette, outcome: &str) -> Color32 {
    match outcome {
        "failed" => p.error_text,
        "cancelled" | "refused" => p.warning_text,
        _ => p.vcs_added,
    }
}

fn figures(entry: &Value) -> String {
    let mut parts = vec![t!("ide-dbhistory-ms", ms = entry["durationMs"].as_u64().unwrap_or(0))];
    if let Some(rows) = entry["rows"].as_u64() {
        parts.push(t!("ide-dbhistory-rows", rows = rows));
    } else if let Some(changed) = entry["affected"].as_u64() {
        parts.push(t!("ide-dbhistory-changed", rows = changed));
    }
    parts.join(" · ")
}

impl HistoryView {
    fn params(&self) -> Value {
        let mut params = json!({ "limit": LISTED, "textLimit": 300 });
        if !self.query.trim().is_empty() {
            params["query"] = json!(self.query);
        }
        if let Some(connection) = &self.connection {
            params["connection"] = json!(connection);
        }
        self.kind.params(&mut params);
        if let Some(outcome) = self.ending.id() {
            params["outcome"] = json!(outcome);
        }
        if let Some(period) = self.when.id() {
            params["period"] = json!(period);
        }
        params
    }

    fn take_preset(&mut self, tab: &InnerTab) {
        if tab.data == self.preset {
            return;
        }
        self.preset = tab.data.clone();
        if let Some(query) = tab.data["query"].as_str() {
            self.query = query.to_owned();
        }
        if let Some(connection) = tab.data["connection"].as_str() {
            self.connection = Some(connection.to_owned());
        }
    }

    fn refresh(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let revision = zen_workbench::database::history::journal(session).map_or(0, |journal| journal.revision());
        let params = self.params();
        let current = self.shown.as_ref().is_some_and(|shown| shown.params == params && shown.revision == revision);
        if current {
            return;
        }
        let answer = match registry.dispatch(session, "db.history.list", params.clone()) {
            Ok(answer) => answer,
            Err(error) => {
                self.error = Some(message(error));
                Value::Null
            }
        };
        if answer["loading"] == true {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let listed =
            answer["entries"].as_array().into_iter().flatten().any(|entry| entry["id"].as_u64() == self.selected);
        if !listed {
            self.selected = None;
            self.detail = None;
        }
        self.shown = Some(Shown { params, revision, answer });
    }

    fn entries(&self) -> &[Value] {
        self.shown.as_ref().and_then(|shown| shown.answer["entries"].as_array()).map_or(&[], Vec::as_slice)
    }

    fn select(&mut self, registry: &Registry, session: &mut Session, id: u64) {
        self.selected = Some(id);
        self.detail = registry.dispatch(session, "db.history.get", json!({ "id": id })).ok();
    }

    fn act(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, (id, action): (u64, Action)) {
        let result = match action {
            Action::Open => registry.dispatch(session, "db.history.openInConsole", json!({ "id": id })).map(|_| ()),
            Action::Run => {
                registry.dispatch(session, "db.history.openInConsole", json!({ "id": id, "run": true })).map(|_| ())
            }
            Action::Copy => registry.dispatch(session, "db.history.get", json!({ "id": id })).map(|entry| {
                ui.ctx().copy_text(entry["text"].as_str().unwrap_or_default().to_owned());
            }),
        };
        self.error = result.err().map(message);
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (tab, listed): (&InnerTab, &Listed),
    ) {
        self.take_preset(tab);
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.editor_background);
        let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR));
        let mut actions = Vec::new();
        let mut bar_ui = ui.new_child(UiBuilder::new().id_salt("database-history-bar").max_rect(bar));
        self.bar(&mut bar_ui, p, registry, session, (bar, listed));
        self.refresh(ui.ctx(), registry, session);
        let mut top = bar.bottom();
        if self.confirm_clear {
            let strip = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), BAR));
            self.clear_question(ui, p, registry, session, strip);
            top = strip.bottom();
        }
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
        let detail_height = if self.detail.is_some() { (area.height() * DETAIL).max(120.0) } else { 0.0 };
        let list = Rect::from_min_max(pos2(area.left(), top), pos2(area.right(), area.bottom() - detail_height));
        self.list(ui, p, registry, session, (list, listed), &mut actions);
        if detail_height > 0.0 {
            let detail = Rect::from_min_max(pos2(area.left(), list.bottom()), area.max);
            self.detail(ui, p, detail, &mut actions);
        }
        for action in actions {
            self.act(ui, registry, session, action);
        }
        ui.advance_cursor_after_rect(area);
    }

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
        let field = Rect::from_min_size(
            pos2(bar.left() + 8.0, bar.top() + 5.0),
            vec2(280.0_f32.min(bar.width() * 0.35), BAR - 10.0),
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
        let hint_text = t!("ide-dbhistory-search");
        let response = ui.put(
            text,
            egui::TextEdit::singleline(&mut self.query)
                .id_salt("database-history-search")
                .frame(egui::Frame::NONE)
                .font(theme::regular(13.0))
                .align(Align2::LEFT_CENTER)
                .hint_text(hint_text.clone())
                .desired_width(text.width()),
        );
        accessibility::labelled(&response, &hint_text);
        if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.query.clear();
        }
        let mut at = field.right() + 8.0;
        let all = t!("ide-dbhistory-all-connections");
        let name = self
            .connection
            .as_ref()
            .map(|id| {
                listed
                    .connections
                    .iter()
                    .find(|known| known["id"] == id.as_str())
                    .and_then(|known| known["name"].as_str())
                    .unwrap_or(id)
                    .to_owned()
            })
            .unwrap_or(all.clone());
        let (chooser, next) =
            super::DatabaseView::selector(ui, p, at, (&name, self.connection.is_none()), "history-connection");
        at = next;
        egui::Popup::menu(&chooser).show(|ui| {
            ui.set_min_width(200.0);
            if ui.selectable_label(self.connection.is_none(), all.as_str()).clicked() {
                self.connection = None;
                ui.close();
            }
            for connection in &listed.connections {
                let Some(id) = connection["id"].as_str() else { continue };
                let label = connection["name"].as_str().unwrap_or(id);
                if ui.selectable_label(self.connection.as_deref() == Some(id), label).clicked() {
                    self.connection = Some(id.to_owned());
                    ui.close();
                }
            }
        });
        let (kinds, next) =
            super::DatabaseView::selector(ui, p, at, (&self.kind.label(), self.kind == Kind::All), "history-kind");
        at = next;
        egui::Popup::menu(&kinds).show(|ui| {
            ui.set_min_width(180.0);
            for kind in Kind::ALL {
                if ui.selectable_label(self.kind == kind, kind.label()).clicked() {
                    self.kind = kind;
                    ui.close();
                }
            }
        });
        let (endings, next) = super::DatabaseView::selector(
            ui,
            p,
            at,
            (&self.ending.label(), self.ending == Ending::Any),
            "history-outcome",
        );
        at = next;
        egui::Popup::menu(&endings).show(|ui| {
            ui.set_min_width(160.0);
            for ending in Ending::ALL {
                if ui.selectable_label(self.ending == ending, ending.label()).clicked() {
                    self.ending = ending;
                    ui.close();
                }
            }
        });
        let (times, _) =
            super::DatabaseView::selector(ui, p, at, (&self.when.label(), self.when == When::Any), "history-when");
        egui::Popup::menu(&times).show(|ui| {
            ui.set_min_width(160.0);
            for when in When::ALL {
                if ui.selectable_label(self.when == when, when.label()).clicked() {
                    self.when = when;
                    ui.close();
                }
            }
        });
        let gear = Rect::from_center_size(pos2(bar.right() - 20.0, bar.center().y), vec2(26.0, 24.0));
        let settings = ui.interact(gear, ui.scope_id().with("database-history-settings"), Sense::click());
        if settings.hovered() {
            ui.painter().rect_filled(gear, 4, p.hover);
        }
        glyph_settings(ui.painter(), Rect::from_center_size(gear.center(), vec2(14.0, 14.0)), p.text_secondary);
        accessibility::button_with(&settings, || t!("ide-dbhistory-settings"));
        if settings.clicked() {
            self.settings = registry.dispatch(session, "db.history.settings", json!({})).ok();
        }
        let mut change: Option<Value> = None;
        egui::Popup::menu(&settings).show(|ui| {
            ui.set_min_width(240.0);
            let Some(known) = self.settings.clone() else { return };
            let mut enabled = known["settings"]["enabled"] == true;
            if ui.checkbox(&mut enabled, t!("ide-dbhistory-record")).changed() {
                change = Some(json!({ "enabled": enabled }));
            }
            let mut days = known["settings"]["retentionDays"].as_u64().unwrap_or(90);
            ui.horizontal(|ui| {
                ui.label(t!("ide-dbhistory-keep-days"));
                if ui.add(egui::DragValue::new(&mut days).range(1..=3650)).changed() {
                    change = Some(json!({ "retentionDays": days }));
                }
            });
            let mut most = known["settings"]["maxEntries"].as_u64().unwrap_or(10_000);
            ui.horizontal(|ui| {
                ui.label(t!("ide-dbhistory-keep-most"));
                if ui.add(egui::DragValue::new(&mut most).range(100..=100_000).speed(100)).changed() {
                    change = Some(json!({ "maxEntries": most }));
                }
            });
            ui.separator();
            if ui.button(t!("ide-dbhistory-clear")).clicked() {
                self.confirm_clear = true;
                ui.close();
            }
        });
        if let Some(params) = change {
            match registry.dispatch(session, "db.history.settings", params) {
                Ok(answer) => self.settings = Some(answer),
                Err(error) => self.error = Some(message(error)),
            }
        }
        let total = self.shown.as_ref().and_then(|shown| shown.answer["total"].as_u64()).unwrap_or(0);
        let count = t!("ide-dbhistory-count", shown = self.entries().len(), total = total);
        ui.painter().text(
            pos2(gear.left() - 8.0, bar.center().y),
            Align2::RIGHT_CENTER,
            count,
            theme::regular(12.0),
            p.text_secondary,
        );
    }

    fn clear_question(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session, strip: Rect) {
        ui.painter().rect_filled(strip, 0, p.banner);
        let mut child = ui.new_child(
            UiBuilder::new().max_rect(strip.shrink2(vec2(12.0, 3.0))).layout(Layout::left_to_right(Align::Center)),
        );
        child.label(egui::RichText::new(t!("ide-dbhistory-clear-question")).font(theme::regular(13.0)).color(p.text));
        if vcs::secondary_button(&mut child, p, &t!("ide-dbhistory-clear-confirm")).clicked() {
            self.confirm_clear = false;
            self.error = registry.dispatch(session, "db.history.clear", json!({})).err().map(message);
        }
        if vcs::secondary_button(&mut child, p, &t!("ide-dbhistory-cancel")).clicked() {
            self.confirm_clear = false;
        }
    }

    fn empty_text(&self) -> String {
        let answer = self.shown.as_ref().map(|shown| &shown.answer);
        let kept = answer.and_then(|answer| answer["kept"].as_u64()).unwrap_or(0);
        if answer.is_some_and(|answer| answer["loading"] == true) {
            t!("ide-dbhistory-loading")
        } else if answer.is_some_and(|answer| answer["enabled"] == false) && kept == 0 {
            t!("ide-dbhistory-off")
        } else if kept == 0 {
            t!("ide-dbhistory-empty")
        } else {
            t!("ide-dbhistory-no-match")
        }
    }

    fn list(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (rect, listed): (Rect, &Listed),
        actions: &mut Vec<(u64, Action)>,
    ) {
        let entries = self.entries().to_vec();
        if entries.is_empty() {
            let text = self.empty_text();
            let job = vcs::truncated(&text, theme::regular(13.0), p.text_secondary, rect.width() - 32.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 40.0),
                galley,
                p.text_secondary,
            );
            return;
        }
        let mut child = ui.new_child(UiBuilder::new().id_salt("database-history-list").max_rect(rect));
        child.spacing_mut().item_spacing.y = 0.0;
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            &mut child,
            ROW,
            entries.len(),
            |ui, range| {
                for index in range {
                    let entry = &entries[index];
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let id = entry["id"].as_u64().unwrap_or_default();
                    if self.selected == Some(id) {
                        ui.painter().rect_filled(row, 0, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(row, 0, p.hover);
                    }
                    draw_row(ui, p, row, entry, listed);
                    let text = one_line(entry["text"].as_str().unwrap_or_default());
                    accessibility::name_with(&response, egui::Role::ListItem, || {
                        format!(
                            "{} {} {}",
                            entry["time"].as_str().unwrap_or_default(),
                            source_label(entry["source"].as_str().unwrap_or_default()),
                            text
                        )
                    });
                    if response.clicked() {
                        chosen = Some(id);
                    }
                    if response.double_clicked() {
                        actions.push((id, Action::Open));
                    }
                    response.context_menu(|ui| {
                        customization::popup(
                            ui,
                            customization::DATABASE_HISTORY_POPUP,
                            |_| true,
                            |ui, slot| {
                                if ui.button(t!(slot)).clicked() {
                                    actions.push((
                                        id,
                                        match slot {
                                            "ide-dbhistory-open-console" => Action::Open,
                                            "ide-dbhistory-run-again" => Action::Run,
                                            _ => Action::Copy,
                                        },
                                    ));
                                    ui.close();
                                }
                            },
                        );
                    });
                }
            },
        );
        if let Some(id) = chosen {
            self.select(registry, session, id);
        }
    }

    fn detail(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect, actions: &mut Vec<(u64, Action)>) {
        let Some(entry) = self.detail.clone() else { return };
        let id = entry["id"].as_u64().unwrap_or_default();
        ui.painter().rect_filled(rect, 0, p.island);
        ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.frame));
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), BAR));
        let mut at = head.left() + 8.0;
        for (label, glyph, color, action) in [
            (
                t!("ide-dbhistory-open-console"),
                glyph_open as super::super::glyphs::Glyph,
                p.text_secondary,
                Action::Open,
            ),
            (t!("ide-dbhistory-run-again"), glyph_run, p.run, Action::Run),
            (t!("ide-dbhistory-copy"), glyph_copy, p.text_secondary, Action::Copy),
        ] {
            let font = theme::regular(12.5);
            let width = ui.painter().layout_no_wrap(label.clone(), font.clone(), p.text).size().x + 34.0;
            let button = Rect::from_min_size(pos2(at, head.center().y - 12.0), vec2(width, 24.0));
            let response =
                ui.interact(button, ui.scope_id().with(("database-history-action", label.as_str())), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(button, 4, p.hover);
            }
            glyph(
                ui.painter(),
                Rect::from_center_size(pos2(button.left() + 13.0, button.center().y), vec2(12.0, 12.0)),
                color,
            );
            ui.painter().text(pos2(button.left() + 26.0, button.center().y), Align2::LEFT_CENTER, &label, font, p.text);
            accessibility::button_with(&response, || label.clone());
            if response.clicked() {
                actions.push((id, action));
            }
            at = button.right() + 4.0;
        }
        let mut facts = vec![entry["time"].as_str().unwrap_or_default().to_owned()];
        facts.push(entry["connectionName"].as_str().or(entry["connection"].as_str()).unwrap_or_default().to_owned());
        if let Some(schema) = entry["schema"].as_str() {
            facts.push(schema.to_owned());
        }
        facts.push(entry["context"].as_str().unwrap_or_default().to_owned());
        facts.push(figures(&entry));
        let job = vcs::truncated(
            &facts.join(" · "),
            theme::regular(12.0),
            p.text_secondary,
            (head.right() - at - 16.0).max(40.0),
        );
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(
            pos2(head.right() - 12.0 - galley.size().x, head.center().y - galley.size().y / 2.0),
            galley,
            p.text_secondary,
        );
        let body =
            Rect::from_min_max(pos2(rect.left() + 12.0, head.bottom()), pos2(rect.right() - 12.0, rect.bottom() - 6.0));
        let mut child = ui.new_child(
            UiBuilder::new().id_salt("database-history-detail").max_rect(body).layout(Layout::top_down(Align::Min)),
        );
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
            if let Some(error) = entry["error"].as_str() {
                ui.add(
                    egui::Label::new(egui::RichText::new(error).font(theme::regular(12.5)).color(p.error_text)).wrap(),
                );
                ui.add_space(6.0);
            }
            let mut notes = Vec::new();
            if entry["textCut"] == true {
                notes.push(t!("ide-dbhistory-cut"));
            }
            if entry["redacted"] == true {
                notes.push(t!("ide-dbhistory-redacted"));
            }
            if !notes.is_empty() {
                ui.label(egui::RichText::new(notes.join(" ")).font(theme::regular(12.0)).color(p.text_secondary));
                ui.add_space(4.0);
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(entry["text"].as_str().unwrap_or_default())
                        .font(FontId::monospace(12.5))
                        .color(p.text),
                )
                .wrap()
                .selectable(true),
            );
        });
    }
}

#[derive(Clone, Copy)]
enum Action {
    Open,
    Run,
    Copy,
}

fn glyph_open(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.rect_stroke(rect.shrink(0.5), 2.0, stroke, StrokeKind::Inside);
    let left = rect.left() + rect.width() * 0.25;
    painter.line_segment(
        [pos2(left, rect.top() + rect.height() * 0.35), pos2(left + rect.width() * 0.2, rect.center().y)],
        stroke,
    );
    painter.line_segment(
        [pos2(left + rect.width() * 0.2, rect.center().y), pos2(left, rect.top() + rect.height() * 0.65)],
        stroke,
    );
}

fn draw_row(ui: &Ui, p: &IdePalette, row: Rect, entry: &Value, listed: &Listed) {
    let painter = ui.painter();
    let outcome = entry["outcome"].as_str().unwrap_or("done");
    painter.circle_filled(pos2(row.left() + 12.0, row.center().y), 3.5, outcome_color(p, outcome));
    let font = theme::regular(12.5);
    let time = entry["time"].as_str().unwrap_or_default();
    let shown_time = time.get(5..).unwrap_or(time);
    painter.text(
        pos2(row.left() + 24.0, row.center().y),
        Align2::LEFT_CENTER,
        shown_time,
        font.clone(),
        p.text_secondary,
    );
    let connection_left = row.left() + 150.0;
    let id = entry["connection"].as_str().unwrap_or_default();
    let known = listed.connections.iter().find(|known| known["id"] == id);
    let color = known
        .and_then(|known| known["type"].as_str())
        .and_then(|kind| listed.colors.iter().find(|(known, _)| known == kind))
        .and_then(|(_, color)| type_color(p, color));
    if let Some(color) = color {
        painter.circle_filled(pos2(connection_left + 3.0, row.center().y), 3.0, color);
    }
    let name = entry["connectionName"].as_str().unwrap_or(id);
    let job = vcs::truncated(name, font.clone(), p.text, 110.0);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    painter.galley(pos2(connection_left + 12.0, row.center().y - galley.size().y / 2.0), galley, p.text);
    let source_left = connection_left + 130.0;
    let mut source = source_label(entry["source"].as_str().unwrap_or_default());
    if entry["agent"] == true {
        source = format!("{source} · {}", t!("ide-dbhistory-agent"));
    }
    if entry["confirmed"] == true {
        source = format!("{source} · {}", t!("ide-dbhistory-confirmed"));
    }
    if entry["outcome"] == "refused" {
        source = format!("{source} · {}", t!("ide-dbhistory-outcome-refused"));
    }
    let source_color = if entry["agent"] == true { p.link } else { p.text_secondary };
    let job = vcs::truncated(&source, font.clone(), source_color, 120.0);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    painter.galley(pos2(source_left, row.center().y - galley.size().y / 2.0), galley, source_color);
    let figures_right = row.right() - 12.0;
    let numbers = figures(entry);
    let numbers_width = painter.layout_no_wrap(numbers.clone(), font.clone(), p.text_secondary).size().x;
    painter.text(pos2(figures_right, row.center().y), Align2::RIGHT_CENTER, numbers, font, p.text_secondary);
    let text_left = source_left + 130.0;
    let text_width = (figures_right - numbers_width - 16.0 - text_left).max(40.0);
    let text = one_line(entry["text"].as_str().unwrap_or_default());
    let color = if outcome == "failed" { p.error_text } else { p.text };
    let job = vcs::truncated(&text, FontId::monospace(12.5), color, text_width);
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    painter.galley(pos2(text_left, row.center().y - galley.size().y / 2.0), galley, color);
}

#[cfg(test)]
mod tests;
