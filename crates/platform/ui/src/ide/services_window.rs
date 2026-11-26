use std::path::PathBuf;

use egui::{Align2, FontId, Rect, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::http::HttpClient;

use super::glyphs::{glyph_minus, glyph_run};
use super::{IdeView, Side, hint, hover, metrics};
use crate::theme;

mod runs;

const LIST_WIDTH: f32 = 300.0;
const ROW: f32 = 24.0;
const ENVIRONMENTS_EVERY: f64 = 2.0;

#[derive(Default)]
pub(super) struct ServicesWindow {
    revision: Option<u64>,
    responses: Vec<Value>,
    selected: Option<u64>,
    environments: Option<(PathBuf, f64, Vec<String>)>,
    shown: Option<(u64, u64, String)>,
    pub(super) runs: runs::RunServicesView,
}

impl ServicesWindow {
    pub(super) fn refresh(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let Some(revision) = session.state::<HttpClient>().map(HttpClient::revision) else { return false };
        if self.revision == Some(revision) {
            return false;
        }
        let first = self.revision.is_none();
        self.revision = Some(revision);
        let newest = self.responses.first().and_then(|response| response["id"].as_u64());
        self.responses = registry
            .dispatch(session, "http.responses.list", json!({}))
            .ok()
            .and_then(|answer| answer["responses"].as_array().cloned())
            .unwrap_or_default();
        let latest = self.responses.first().and_then(|response| response["id"].as_u64());
        let started = !first && latest.is_some() && latest != newest;
        if started || self.selected.is_none_or(|id| !self.responses.iter().any(|r| r["id"] == id)) {
            self.selected = latest;
        }
        started
    }

    pub(super) fn any_running(&self) -> bool {
        self.responses.iter().any(|response| response["state"] == "running")
    }

    fn selected(&self) -> Option<&Value> {
        self.responses.iter().find(|response| response["id"].as_u64() == self.selected)
    }

    fn selected_text(&mut self) -> &str {
        let (Some(id), Some(revision)) = (self.selected, self.revision) else { return "" };
        if self.shown.as_ref().is_none_or(|(shown, at, _)| (*shown, *at) != (id, revision)) {
            let text = self.selected().map(response_text).unwrap_or_default();
            self.shown = Some((id, revision, text));
        }
        self.shown.as_ref().map_or("", |(_, _, text)| text.as_str())
    }

    fn environments(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        file: Option<PathBuf>,
        now: f64,
    ) -> Vec<String> {
        let Some(file) = file else { return Vec::new() };
        let fresh = self
            .environments
            .as_ref()
            .is_some_and(|(shown, read, _)| *shown == file && now - read < ENVIRONMENTS_EVERY);
        if !fresh {
            let listed = registry
                .dispatch(session, "http.requests.list", json!({ "path": file }))
                .ok()
                .and_then(|answer| answer["environments"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect();
            self.environments = Some((file, now, listed));
        }
        self.environments.as_ref().map(|(_, _, names)| names.clone()).unwrap_or_default()
    }
}

pub(super) fn response_text(exchange: &Value) -> String {
    let mut out = format!(
        "{} {}\n",
        exchange["method"].as_str().unwrap_or_default(),
        exchange["url"].as_str().unwrap_or_default()
    );
    match exchange["state"].as_str() {
        Some("running") => {
            out.push('\n');
            out.push_str(&t!("services-http-running"));
            return out;
        }
        Some("failed") => {
            out.push('\n');
            out.push_str(exchange["error"].as_str().unwrap_or_default());
            return out;
        }
        _ => {}
    }
    let response = &exchange["response"];
    let status = response["status"].as_u64().unwrap_or_default();
    let reason = response["reason"].as_str().unwrap_or_default();
    out.push_str(&format!("\n{} {status} {reason}\n", response["version"].as_str().unwrap_or("HTTP/1.1")));
    for header in response["headers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}: {}\n",
            header["name"].as_str().unwrap_or_default(),
            header["value"].as_str().unwrap_or_default()
        ));
    }
    out.push('\n');
    if response["binary"] == true {
        out.push_str(&t!("services-http-binary"));
        out.push('\n');
    } else {
        let body =
            zen_http::pretty_body(response["contentType"].as_str(), response["body"].as_str().unwrap_or_default());
        if !body.is_empty() {
            out.push_str(&body);
            out.push('\n');
        }
        if response["truncated"] == true {
            out.push_str(&t!("services-http-truncated"));
            out.push('\n');
        }
    }
    out.push('\n');
    out.push_str(&t!(
        "services-http-summary",
        status = status.to_string(),
        reason = reason.to_owned(),
        ms = exchange["durationMs"].as_u64().unwrap_or_default().to_string(),
        size = response["size"].as_u64().unwrap_or_default().to_string()
    ));
    out
}

fn row_status(exchange: &Value) -> String {
    match exchange["state"].as_str() {
        Some("running") => "…".to_owned(),
        Some("failed") => t!("services-http-failed"),
        _ => exchange["response"]["status"].as_u64().map(|status| status.to_string()).unwrap_or_default(),
    }
}

fn row_label(exchange: &Value) -> String {
    let url = exchange["url"].as_str().unwrap_or_default();
    let shown = url.split_once("://").map_or(url, |(_, rest)| rest.find('/').map_or(rest, |slash| &rest[slash..]));
    match exchange["name"].as_str() {
        Some(name) => format!("{name} · {} {shown}", exchange["method"].as_str().unwrap_or_default()),
        None => format!("{} {shown}", exchange["method"].as_str().unwrap_or_default()),
    }
}

impl IdeView {
    fn service_action(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, action: runs::ServiceAction) {
        let result = match action {
            runs::ServiceAction::Debug(params) => {
                self.launch_run(registry, session, "debug.session.start", params, false).map(|_| ())
            }
            runs::ServiceAction::Dispatch(command, params) => {
                registry.dispatch(session, command, params).map(|_| ()).map_err(super::agent::ActionError::Dispatch)
            }
        };
        self.services_window.runs.invalidate();
        if let Err(super::agent::ActionError::Dispatch(error)) = result {
            self.report(ui, error);
        }
    }

    pub(super) fn follow_services(&mut self, registry: &Registry, session: &mut Session) {
        if self.services_window.refresh(registry, session) {
            self.show_tool("services");
        }
    }

    pub(super) fn services_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-tool-services"),
            theme::semibold(13.0),
            p.text,
        );
        let now = ui.input(|input| input.time);
        self.services_window.runs.buttons.clear();
        self.services_window.runs.refresh(registry, session, now);
        let (toolbar_end, mut action) =
            self.services_window.runs.toolbar(ui, &p, title.right() + 12.0, header.center().y);
        let active = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned())
            .filter(|path| zen_http::is_http_file(path));
        let file = active.or_else(|| {
            self.services_window.selected().and_then(|exchange| exchange["file"].as_str()).map(PathBuf::from)
        });
        let environments = self.services_window.environments(registry, session, file.clone(), now);
        let selected = session.state::<HttpClient>().and_then(HttpClient::environment).map(str::to_owned);
        let combo = Rect::from_min_size(pos2(toolbar_end + 8.0, header.center().y - 12.0), vec2(220.0, 24.0));
        let mut chosen = None;
        ui.scope_builder(UiBuilder::new().max_rect(combo), |ui| {
            let none = t!("services-http-no-environment");
            let shown = selected.clone().map_or(none.clone(), |env| t!("services-http-run-with", env = env));
            let label = shown.clone();
            let combo = egui::ComboBox::from_id_salt("services-http-environment")
                .selected_text(shown)
                .width(220.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(selected.is_none(), none).clicked() {
                        chosen = Some(None);
                    }
                    for env in &environments {
                        if ui.selectable_label(selected.as_deref() == Some(env.as_str()), env).clicked() {
                            chosen = Some(Some(env.clone()));
                        }
                    }
                });
            crate::accessibility::name(&combo.response, egui::Role::ComboBox, &label);
        });
        if let Some(env) = chosen
            && let Err(error) = registry.dispatch(session, "http.environment.select", json!({ "env": env }))
        {
            self.post(ui, error.text());
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("services-hide"), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        if response.clicked() {
            self.bottom = None;
        }

        if let Some(added) = self.services_window.runs.add_menu(ui, &p) {
            action = Some(added);
        }
        if let Some(action) = action {
            self.service_action(ui, registry, session, action);
        }

        let body = ui.available_rect_before_wrap();
        if self.services_window.responses.is_empty() && self.services_window.runs.is_empty() {
            ui.painter().text(
                body.center(),
                Align2::CENTER_CENTER,
                t!("services-empty"),
                theme::regular(13.0),
                p.text_secondary,
            );
            ui.allocate_rect(body, Sense::hover());
            return;
        }
        let list = Rect::from_min_max(body.min, pos2((body.left() + LIST_WIDTH).min(body.right()), body.bottom()));
        let detail = Rect::from_min_max(pos2(list.right() + 1.0, body.top()), body.max);
        ui.painter().line_segment(
            [pos2(list.right(), body.top()), pos2(list.right(), body.bottom())],
            egui::Stroke::new(1.0, p.tab_border),
        );
        let mut clicked = None;
        let services = &mut self.services_window;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical().id_salt("services-http-list").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                services.runs.tree(ui, &p);
                if !services.responses.is_empty() && !services.runs.is_empty() {
                    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    ui.painter().text(
                        pos2(header.left() + 8.0, header.center().y),
                        Align2::LEFT_CENTER,
                        t!("services-http-client"),
                        theme::regular(13.0),
                        p.text,
                    );
                }
                for exchange in &services.responses {
                    let id = exchange["id"].as_u64();
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    if id == services.selected && services.runs.chosen.is_none() {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.hover);
                    }
                    let failed = exchange["state"] == "failed"
                        || exchange["response"]["status"].as_u64().is_some_and(|status| status >= 400);
                    let icon = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(10.0));
                    glyph_run(ui.painter(), icon, if failed { p.error_text } else { p.run });
                    let status = ui.painter().text(
                        pos2(rect.right() - 8.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        row_status(exchange),
                        theme::regular(12.0),
                        if failed { p.error_text } else { p.text_secondary },
                    );
                    let room = (status.left() - rect.left() - 36.0).max(0.0);
                    let galley = ui.painter().layout(row_label(exchange), theme::regular(13.0), p.text, f32::INFINITY);
                    let clip = Rect::from_min_size(pos2(rect.left() + 28.0, rect.top()), vec2(room, ROW));
                    ui.painter().with_clip_rect(clip).galley(
                        pos2(clip.left(), rect.center().y - galley.size().y / 2.0),
                        galley,
                        p.text,
                    );
                    if response.clicked() {
                        clicked = id;
                    }
                }
            });
        });
        if clicked.is_some() {
            self.services_window.selected = clicked;
            self.services_window.runs.chosen = None;
            self.services_window.runs.kind = None;
        }
        if self.services_window.runs.chosen.is_some() {
            ui.scope_builder(UiBuilder::new().max_rect(detail), |ui| {
                self.services_window.runs.console(ui, &p, registry, session);
            });
            ui.allocate_rect(body, Sense::hover());
            return;
        }
        let text = self.services_window.selected_text();
        ui.scope_builder(UiBuilder::new().max_rect(detail.shrink2(vec2(8.0, 4.0))), |ui| {
            egui::ScrollArea::both().id_salt("services-http-response").auto_shrink([false, false]).show(ui, |ui| {
                let mut shown = text;
                ui.add(
                    egui::TextEdit::multiline(&mut shown)
                        .font(FontId::monospace(13.0))
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY),
                );
            });
        });
        ui.allocate_rect(body, Sense::hover());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_started_request_selects_itself_and_opens_the_window() {
        use zen_http::test_server::TestServer;
        let server = TestServer::start(vec![TestServer::json(200, "[]")]);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.http");
        std::fs::write(&file, format!("GET {}\n", server.url("/x"))).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let mut window = ServicesWindow::default();
        assert!(!window.refresh(&registry, &mut session), "nothing before the client exists");
        registry.dispatch(&mut session, "http.responses.list", json!({})).unwrap();
        assert!(!window.refresh(&registry, &mut session), "the first reading opens nothing");
        assert!(window.responses.is_empty());
        let started =
            registry.dispatch(&mut session, "http.request.run", json!({ "path": file, "waitMs": 10000 })).unwrap();
        assert!(window.refresh(&registry, &mut session), "a new request opens the window");
        assert_eq!(window.selected, started["id"].as_u64());
        assert!(!window.any_running());
        assert!(!window.refresh(&registry, &mut session), "read once per change");
    }

    #[test]
    fn the_response_console_text() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let exchange = json!({
            "method": "GET", "url": "http://127.0.0.1:1/items", "state": "done", "durationMs": 12,
            "response": {
                "status": 200, "reason": "OK", "version": "HTTP/1.1", "contentType": "application/json",
                "headers": [{ "name": "content-type", "value": "application/json" }],
                "body": "{\"a\":[1]}", "size": 9, "truncated": false, "binary": false
            }
        });
        let text = response_text(&exchange);
        assert!(text.starts_with("GET http://127.0.0.1:1/items\n\nHTTP/1.1 200 OK\ncontent-type: application/json\n\n{\n  \"a\": [\n    1\n  ]\n}\n\n"), "{text}");
        assert!(text.ends_with(&t!("services-http-summary", status = "200", reason = "OK", ms = "12", size = "9")));
        let failed = json!({ "method": "GET", "url": "http://x/", "state": "failed", "error": "refused" });
        assert_eq!(response_text(&failed), "GET http://x/\n\nrefused");
        assert_eq!(
            row_label(&json!({ "method": "POST", "url": "https://h:1/a/b?c=1", "name": "create" })),
            "create · POST /a/b?c=1"
        );
        assert_eq!(row_label(&json!({ "method": "GET", "url": "http://h" })), "GET h");
    }
}
