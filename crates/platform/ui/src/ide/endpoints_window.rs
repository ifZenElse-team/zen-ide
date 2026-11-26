use std::path::Path;

use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::glyphs::*;
use super::{IdeView, Side, hint, hover, metrics};
use crate::{accessibility, theme};

pub(super) const ID: &str = "endpoints";
const READ_EVERY: f64 = 2.0;
const CHECK_EVERY: f64 = 1.0;
const PLUGIN: &str = "spring-boot";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Endpoint {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) handler: String,
}

pub(super) fn endpoints(list: &Value) -> Vec<Endpoint> {
    list["endpoints"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|endpoint| {
            Some(Endpoint {
                method: endpoint["method"].as_str()?.to_owned(),
                path: endpoint["path"].as_str()?.to_owned(),
                handler: endpoint["handler"].as_str()?.to_owned(),
            })
        })
        .collect()
}

#[derive(Default)]
pub(super) struct EndpointsWindow {
    checked: Option<(std::path::PathBuf, u64, f64)>,
    list: Option<Value>,
    error: Option<String>,
    read: f64,
    selected: Option<usize>,
}

impl EndpointsWindow {
    fn read(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        if now - self.read < READ_EVERY {
            return;
        }
        self.read = now;
        match registry.dispatch(session, "springBoot.endpoints.list", json!({})) {
            Ok(list) => {
                self.list = Some(list);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn read_now(&mut self, registry: &Registry, session: &mut Session) {
        self.read = f64::NEG_INFINITY;
        self.read(registry, session, f64::MAX);
        self.read = f64::NEG_INFINITY;
    }

    fn rows(&self) -> Vec<Endpoint> {
        self.list.as_ref().map(endpoints).unwrap_or_default()
    }

    pub(super) fn state(&self, available: bool, shown: bool) -> Value {
        let mut state = json!({ "available": available, "selected": self.selected });
        if available && shown {
            let rows: Vec<Value> = self
                .rows()
                .iter()
                .map(
                    |endpoint| json!({ "method": endpoint.method, "path": endpoint.path, "handler": endpoint.handler }),
                )
                .collect();
            state["rows"] = Value::Array(rows);
            state["loading"] = self.list.as_ref().map_or(json!(true), |list| list["loading"].clone());
        }
        state
    }
}

fn open(registry: &Registry, session: &mut Session, endpoint: &Endpoint) -> Result<(), String> {
    let params = json!({ "handler": endpoint.handler, "path": endpoint.path });
    registry.dispatch(session, "springBoot.endpoints.open", params).map(|_| ()).map_err(|error| error.to_string())
}

impl IdeView {
    pub(super) fn follow_endpoints(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        if self.build_tools.endpoints && self.tool_visible(ID) {
            self.endpoints.read(registry, session, now);
        }
        let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
            self.build_tools.endpoints = false;
            return;
        };
        let generation = self.plugins.generation();
        let fresh = self.endpoints.checked.as_ref().is_some_and(|(seen, at_generation, at)| {
            *seen == root && *at_generation == generation && now - at < CHECK_EVERY
        });
        if fresh {
            return;
        }
        self.endpoints.checked = Some((root, generation, now));
        self.build_tools.endpoints = session
            .state::<zen_workbench::plugin_projects::PluginProjects>()
            .is_some_and(|projects| projects.installed(PLUGIN));
        if !self.build_tools.endpoints && self.tool_visible(ID) {
            self.hide_tool(ID);
        }
    }

    pub(super) fn toggle_endpoints(&mut self, registry: &Registry, session: &mut Session) -> Result<(), String> {
        if !self.build_tools.endpoints {
            return Err(t!("ide-endpoints-not-available"));
        }
        self.toggle_tool(ID);
        if self.tool_visible(ID) {
            self.endpoints.read_now(registry, session);
        }
        Ok(())
    }

    pub(super) fn endpoints_state(&self) -> Value {
        self.endpoints.state(self.build_tools.endpoints, self.tool_visible(ID))
    }

    pub(super) fn endpoints_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let now = ui.input(|input| input.time);
        self.endpoints.read(registry, session, now);
        let rows = self.endpoints.rows();
        let loading = self.endpoints.list.as_ref().is_none_or(|list| list["loading"] == true);

        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-endpoints-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("endpoints-title"), title, || t!("ide-endpoints-title"));
        let refresh = Rect::from_center_size(pos2(title.right() + 26.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(refresh, ui.scope_id().with("endpoints-refresh"), Sense::click());
        hover(ui, &response, &p);
        glyph_rerun(ui.painter(), refresh.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-endpoints-refresh"), None, Side::Below);
        accessibility::button(&response, &t!("ide-endpoints-refresh"));
        if response.clicked() {
            let _ = registry.dispatch(session, "springBoot.endpoints.refresh", json!({}));
            self.endpoints.read_now(registry, session);
        }
        let starters = Rect::from_center_size(pos2(title.right() + 52.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(starters, ui.scope_id().with("endpoints-starters"), Sense::click());
        hover(ui, &response, &p);
        glyph_plus(ui.painter(), starters.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-starters-edit"), None, Side::Below);
        accessibility::button(&response, &t!("ide-starters-edit"));
        if response.clicked() {
            self.run_action(ui, registry, session, super::starters_dialog::OPEN, json!({}));
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("endpoints-hide"), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        accessibility::button(&response, &t!("ide-run-hide"));
        if response.clicked() {
            self.hide_tool(ID);
        }
        if let Some(error) = self.endpoints.error.clone() {
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            ui.painter().with_clip_rect(line).text(
                pos2(line.left() + 12.0, line.center().y),
                Align2::LEFT_CENTER,
                error,
                theme::regular(12.0),
                p.error_text,
            );
        }

        let method_width = 64.0;
        let mut activated = None;
        let mut selected = self.endpoints.selected;
        egui::ScrollArea::vertical().id_salt("endpoints-list").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (index, endpoint) in rows.iter().enumerate() {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
                let is_selected = selected == Some(index);
                let label = format!("{} {}", endpoint.method, endpoint.path);
                accessibility::tree_item(&response, 1, None, is_selected, &label);
                let highlight =
                    Rect::from_min_max(pos2(rect.left() + 12.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                if is_selected {
                    ui.painter().rect_filled(highlight, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(highlight, 5, p.hover);
                }
                let painter = ui.painter().with_clip_rect(rect);
                let method = if endpoint.method.is_empty() { t!("ide-endpoints-any") } else { endpoint.method.clone() };
                painter.text(
                    pos2(highlight.left() + 8.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    method,
                    theme::semibold(12.0),
                    p.link,
                );
                let path = painter.text(
                    pos2(highlight.left() + 8.0 + method_width, rect.center().y),
                    Align2::LEFT_CENTER,
                    &endpoint.path,
                    theme::regular(13.0),
                    p.text,
                );
                painter.text(
                    pos2(path.right() + 12.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &endpoint.handler,
                    theme::regular(12.0),
                    p.text_secondary,
                );
                if response.clicked() {
                    selected = Some(index);
                }
                if response.double_clicked() {
                    activated = Some(index);
                }
            }
            if rows.is_empty() {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    let text = if loading { t!("ide-endpoints-loading") } else { t!("ide-endpoints-empty") };
                    ui.label(egui::RichText::new(text).color(p.text_secondary).font(theme::regular(12.5)));
                });
            }
        });
        if ui.ui_contains_pointer() || selected.is_some() {
            let enter = ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if enter && activated.is_none() && ui.ui_contains_pointer() {
                activated = selected;
            }
        }
        self.endpoints.selected = selected;
        if let Some(endpoint) = activated.and_then(|index| rows.get(index))
            && let Err(error) = open(registry, session, endpoint)
        {
            self.post(ui, error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_are_the_listed_mappings_and_agents_read_them() {
        let list = json!({ "loading": false, "endpoints": [
            { "method": "GET", "path": "/api/orders", "handler": "OrderController.all", "file": "/p/A.java", "line": 9 },
            { "method": "", "path": "/any", "handler": "AnyController.any", "file": "/p/B.java", "line": 3 },
            { "path": "/broken" }
        ] });
        let rows = endpoints(&list);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1],
            Endpoint { method: String::new(), path: "/any".into(), handler: "AnyController.any".into() }
        );
        let window = EndpointsWindow { list: Some(list), ..EndpointsWindow::default() };
        let state = window.state(true, true);
        assert_eq!(state["rows"][0]["path"], "/api/orders");
        assert_eq!(state["loading"], false);
        assert!(window.state(false, true).get("rows").is_none(), "nothing while it is not available");
    }
}
