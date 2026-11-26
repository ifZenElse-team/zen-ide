use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::database_navigator::type_color;
use super::database_view::{DatabaseView, glyph_run_script, glyph_run_several};
use super::glyphs::glyph_run;
use super::{IdePalette, theme, transaction_dialog, vcs};

pub(super) const HEIGHT: f32 = 32.0;

const REFRESH: Duration = Duration::from_millis(500);

const LISTED: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(super) struct ScriptBar {
    state: Option<(PathBuf, Instant, Value)>,
    connections: Option<(Instant, Vec<Value>)>,
    pending: Option<Value>,
    ask: bool,
    asked: bool,
    error: Option<String>,
}

fn message(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

pub(super) fn not_bound(error: &zen_core::DispatchError) -> bool {
    matches!(error, zen_core::DispatchError::Failed { error, .. } if error.code.as_deref() == Some("noBinding"))
}

impl ScriptBar {
    pub(super) fn ask_binding(&mut self, run: Value) {
        self.pending = Some(run);
        self.ask = true;
        self.asked = false;
    }

    fn state(&mut self, registry: &Registry, session: &mut Session, path: &Path) -> Value {
        if let Some((known, read, state)) = &self.state
            && known == path
            && read.elapsed() < REFRESH
        {
            return state.clone();
        }
        let state = registry.dispatch(session, "db.script.state", json!({ "path": path })).unwrap_or_default();
        self.state = Some((path.to_owned(), Instant::now(), state.clone()));
        state
    }

    fn connections(&mut self, registry: &Registry, session: &mut Session) -> Vec<Value> {
        if let Some((read, listed)) = &self.connections
            && read.elapsed() < LISTED
        {
            return listed.clone();
        }
        let listed = registry.dispatch(session, "db.dataSource.list", json!({ "limit": 200 })).unwrap_or_default();
        let listed = listed["connections"].as_array().cloned().unwrap_or_default();
        self.connections = Some((Instant::now(), listed.clone()));
        listed
    }

    fn dispatch(&mut self, registry: &Registry, session: &mut Session, command: &str, params: Value) -> bool {
        self.state = None;
        match registry.dispatch(session, command, params.clone()) {
            Ok(_) => {
                self.error = None;
                true
            }
            Err(error) if not_bound(&error) => {
                self.ask_binding(params);
                false
            }
            Err(error) => {
                if !transaction_dialog::ask(session, &error) {
                    self.error = Some(message(error));
                }
                false
            }
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
    ) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
        ui.painter().rect_filled(rect, 0, p.island);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, p.frame));
        let mut bar = ui.new_child(egui::UiBuilder::new().id_salt(("sql-script-bar", path)).max_rect(rect));
        let ui = &mut bar;
        let state = self.state(registry, session, path);
        let shortcut = |action: &str| crate::keymap::shortcut(action);
        let at = rect.left() + 6.0;
        let execute = t!("ide-database-execute");
        let key = shortcut("db.script.run");
        let (run, at) = DatabaseView::tool_button(ui, p, (at, glyph_run, p.run), true, (&execute, key.as_deref()));
        let script = t!("ide-database-execute-script");
        let key = shortcut("db.script.run/script");
        let (all, at) =
            DatabaseView::tool_button(ui, p, (at, glyph_run_script, p.run), true, (&script, key.as_deref()));
        let several = t!("ide-database-execute-several");
        let (on_several, at) = DatabaseView::tool_button(ui, p, (at, glyph_run_several, p.run), true, (&several, None));
        if run.clicked() {
            self.dispatch(registry, session, "db.script.run", json!({ "path": path }));
        }
        if on_several.clicked() {
            super::several_dialog::open(session, path, state["connection"].as_str().map(str::to_owned));
        }
        if all.clicked() {
            self.dispatch(registry, session, "db.script.run", json!({ "path": path, "scope": "script" }));
        }
        ui.painter().vline(at + 4.0, (rect.top() + 7.0)..=(rect.bottom() - 7.0), egui::Stroke::new(1.0, p.frame));
        let at = at + 10.0;
        let name = state["connectionName"].as_str().map(str::to_owned);
        let none = t!("ide-database-no-connection");
        let (chooser, at) =
            DatabaseView::selector(ui, p, at, (name.as_deref().unwrap_or(&none), name.is_none()), "connection");
        if let Some(color) = state["color"].as_str().and_then(|color| type_color(p, color)) {
            ui.painter().circle_filled(pos2(chooser.rect.left() + 3.0, chooser.rect.center().y), 2.5, color);
        }
        super::hint(ui, &chooser, &t!("ide-database-choose-connection"), None, super::Side::Below);
        let bound = state["bound"] == true;
        let schema = state["schema"].as_str().map(str::to_owned);
        let default_schema = t!("ide-database-default-schema");
        let schemas = bound.then(|| {
            let shown = (schema.as_deref().unwrap_or(&default_schema), schema.is_none());
            let (schemas, _) = DatabaseView::selector(ui, p, at, shown, "schema");
            super::hint(ui, &schemas, &t!("ide-database-choose-schema"), None, super::Side::Below);
            schemas
        });
        let connection_popup = egui::Popup::default_response_id(&chooser);
        if std::mem::take(&mut self.ask) {
            egui::Popup::open_id(ui.ctx(), connection_popup);
            self.asked = true;
        } else if self.asked && !egui::Popup::is_id_open(ui.ctx(), connection_popup) {
            self.asked = false;
            self.pending = None;
        }
        let connections = if egui::Popup::is_id_open(ui.ctx(), connection_popup) || chooser.clicked() {
            self.connections(registry, session)
        } else {
            Vec::new()
        };
        let mut chosen = None;
        let asking = self.pending.is_some();
        egui::Popup::menu(&chooser).show(|ui| {
            ui.set_min_width(240.0);
            if asking {
                ui.label(egui::RichText::new(t!("ide-sql-script-choose-to-run")).color(p.text_secondary));
                ui.separator();
            }
            if connections.is_empty() {
                ui.label(egui::RichText::new(t!("ide-database-no-connections")).color(p.text_secondary));
            }
            for connection in &connections {
                let name = connection["name"].as_str().unwrap_or_default();
                if ui.button(name).clicked() {
                    chosen = connection["id"].as_str().map(str::to_owned);
                    ui.close();
                }
            }
        });
        if let Some(id) = chosen {
            let bound = self.dispatch(registry, session, "db.script.bind", json!({ "path": path, "connection": id }));
            self.asked = false;
            if let Some(run) = self.pending.take().filter(|_| bound) {
                self.dispatch(registry, session, "db.script.run", run);
            }
        }
        if let Some(schemas) = schemas {
            self.schema_popup(ui, p, registry, session, (path, &schemas));
        }
        let right = rect.right() - 10.0;
        let (text, color) = match &self.error {
            Some(error) => (error.clone(), p.error_text),
            None if !bound => (t!("ide-sql-script-not-bound"), p.text_secondary),
            None => (t!("ide-sql-script-results-there"), p.text_secondary),
        };
        let room = (right - at - 24.0).max(0.0);
        if room > 40.0 {
            let job = vcs::truncated(&text, theme::regular(12.0), color, room);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(right - galley.size().x, rect.center().y - galley.size().y / 2.0), galley, color);
        }
    }

    fn schema_popup(
        &mut self,
        ui: &Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (path, schemas): (&Path, &egui::Response),
    ) {
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(schemas));
        let listed = if open {
            registry.dispatch(session, "db.console.schemas", json!({ "path": path })).unwrap_or_default()
        } else {
            Value::Null
        };
        let mut picked: Option<Option<String>> = None;
        egui::Popup::menu(schemas).show(|ui| {
            ui.set_min_width(200.0);
            if ui.button(t!("ide-database-default-schema")).clicked() {
                picked = Some(None);
                ui.close();
            }
            ui.separator();
            match listed["schemas"].as_array() {
                Some(names) => {
                    for name in names.iter().filter_map(Value::as_str) {
                        if ui.button(name).clicked() {
                            picked = Some(Some(name.to_owned()));
                            ui.close();
                        }
                    }
                }
                None if listed["reading"] == true => {
                    ui.label(egui::RichText::new(t!("ide-database-reading")).color(p.text_secondary));
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
                None => {
                    ui.label(egui::RichText::new(t!("ide-database-connect-for-schemas")).color(p.text_secondary));
                }
            }
        });
        if let Some(schema) = picked {
            self.dispatch(registry, session, "db.script.bind", json!({ "path": path, "schema": schema }));
        }
    }
}

#[cfg(test)]
mod tests;
