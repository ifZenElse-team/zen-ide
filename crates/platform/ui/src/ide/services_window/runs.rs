use egui::{Align2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::run::Runs;
use zen_workbench::run_services::RunServices;

use super::super::glyphs::{
    Glyph, glyph_bug, glyph_eye, glyph_eye_off, glyph_folder, glyph_plus, glyph_rerun, glyph_run, glyph_stop,
};
use super::super::run_window::{Console, Shown, Tool};
use super::super::{IdePalette, Side, doc_popups};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const LIST_EVERY: f64 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::ide) enum ServiceAction {
    Dispatch(&'static str, Value),
    Debug(Value),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Seen {
    runs: Vec<(u64, bool)>,
    services: RunServices,
}

impl Seen {
    fn of(session: &Session) -> Self {
        Self {
            runs: session
                .state::<Runs>()
                .map(|runs| runs.runs().iter().map(|run| (run.id, run.is_running())).collect())
                .unwrap_or_default(),
            services: session.state::<RunServices>().cloned().unwrap_or_default(),
        }
    }
}

#[derive(Default)]
pub(in crate::ide) struct RunServicesView {
    list: Value,
    read_at: Option<f64>,
    seen: Option<Seen>,
    pub(in crate::ide) chosen: Option<String>,
    pub(in crate::ide) kind: Option<String>,
    console: Option<(u64, Option<u64>, Console)>,
    adding: bool,
    pub(in crate::ide) buttons: Vec<(String, Rect)>,
}

fn kinds(list: &Value) -> &[Value] {
    list["kinds"].as_array().map(Vec::as_slice).unwrap_or_default()
}

impl RunServicesView {
    pub(in crate::ide) fn refresh(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        let seen = Seen::of(session);
        let stale = self.read_at.is_none_or(|read| now - read >= LIST_EVERY);
        if !stale && self.seen.as_ref() == Some(&seen) {
            return;
        }
        self.read_at = Some(now);
        self.seen = Some(seen);
        self.list = registry.dispatch(session, "run.services.list", json!({})).unwrap_or(Value::Null);
        let known = |id: &str| kinds(&self.list).iter().any(|kind| kind_items(kind).any(|item| item["id"] == id));
        if self.chosen.as_deref().is_some_and(|id| !known(id)) {
            self.chosen = None;
        }
    }

    pub(in crate::ide) fn invalidate(&mut self) {
        self.read_at = None;
    }

    pub(in crate::ide) fn is_empty(&self) -> bool {
        kinds(&self.list).is_empty()
    }

    pub(in crate::ide) fn chosen_item(&self) -> Option<&Value> {
        let id = self.chosen.as_deref()?;
        kinds(&self.list).iter().flat_map(kind_items).find(|item| item["id"] == id)
    }

    pub(in crate::ide) fn row(&self, name: &str) -> Option<Rect> {
        self.buttons.iter().find(|(shown, _)| shown == name).map(|(_, rect)| *rect)
    }

    pub(in crate::ide) fn toolbar(
        &mut self,
        ui: &Ui,
        p: &IdePalette,
        left: f32,
        y: f32,
    ) -> (f32, Option<ServiceAction>) {
        let chosen = self.chosen_item().cloned();
        let id = chosen.as_ref().and_then(|item| item["id"].as_str()).map(str::to_owned);
        let running = chosen.as_ref().is_some_and(|item| item["running"] == true);
        let hidden = self.list["hidden"].as_array().is_some_and(|hidden| !hidden.is_empty());
        let removable = self.kind.clone().filter(|_| id.is_none());
        let by_id = |id: &Option<String>| json!({ "configurationId": id });
        let hide_name = if removable.is_some() { t!("services-remove-kind") } else { t!("services-hide") };
        let tools: [(&'static str, Glyph, String, bool); 7] = [
            ("services-add", glyph_plus, t!("services-add"), true),
            ("services-run", glyph_run, t!("services-run"), id.is_some()),
            ("services-debug", glyph_bug, t!("services-debug"), id.is_some()),
            ("services-stop", glyph_stop, t!("services-stop"), running),
            ("services-rerun", glyph_rerun, t!("services-rerun"), id.is_some()),
            ("services-hide", glyph_eye_off, hide_name, id.is_some() || removable.is_some()),
            ("services-restore", glyph_eye, t!("services-restore"), hidden),
        ];
        let mut action = None;
        let mut x = left;
        for (salt, glyph, name, enabled) in tools {
            let rect = Rect::from_center_size(pos2(x + 13.0, y), Vec2::splat(26.0));
            let color = match salt {
                "services-run" | "services-debug" | "services-rerun" => p.run,
                "services-stop" => p.problem("error"),
                _ => p.text,
            };
            if Tool::new(salt, glyph, name, color).show_hinted(ui, p, rect, enabled, Side::Below) {
                action = match salt {
                    "services-add" => {
                        self.adding = !self.adding;
                        None
                    }
                    "services-run" | "services-rerun" => Some(ServiceAction::Dispatch("run.process.start", by_id(&id))),
                    "services-debug" => Some(ServiceAction::Debug(by_id(&id))),
                    "services-stop" => Some(ServiceAction::Dispatch("run.process.stop", by_id(&id))),
                    "services-hide" => match (&id, &removable) {
                        (Some(_), _) => Some(ServiceAction::Dispatch("run.services.hide", by_id(&id))),
                        (None, Some(kind)) => {
                            Some(ServiceAction::Dispatch("run.services.remove", json!({ "kind": kind })))
                        }
                        _ => None,
                    },
                    _ => Some(ServiceAction::Dispatch("run.services.restore", json!({}))),
                };
            }
            self.buttons.push((salt.to_owned(), rect));
            x += 28.0;
        }
        (x, action)
    }

    pub(in crate::ide) fn add_menu(&mut self, ui: &Ui, p: &IdePalette) -> Option<ServiceAction> {
        if !self.adding {
            return None;
        }
        let anchor = self.row("services-add")?;
        let mut chosen = None;
        let available: Vec<(String, String)> = self.list["available"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|pair| Some((pair[0].as_str()?.to_owned(), pair[1].as_str()?.to_owned())))
            .collect();
        let mut rows = Vec::new();
        let area = egui::Area::new(ui.scope_id().with("services-add-menu"))
            .order(egui::Order::Tooltip)
            .fixed_pos(anchor.left_bottom() + vec2(0.0, 4.0))
            .show(ui.ctx(), |ui| {
                doc_popups::popup_frame(p).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.set_min_width(240.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    ui.painter().text(
                        pos2(header.left() + 8.0, header.center().y),
                        Align2::LEFT_CENTER,
                        t!("services-run-configuration-type"),
                        theme::semibold(12.5),
                        p.text_secondary,
                    );
                    for (kind, message) in &available {
                        let name = t!(message);
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        accessibility::name(&response, egui::Role::MenuItem, &name);
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 4, p.selection);
                        }
                        ui.painter().text(
                            pos2(rect.left() + 16.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            name,
                            theme::regular(13.0),
                            p.text,
                        );
                        rows.push((format!("add:{kind}"), rect));
                        if response.clicked() {
                            chosen = Some(kind.clone());
                        }
                    }
                });
            });
        self.buttons.extend(rows);
        if let Some(kind) = chosen {
            self.adding = false;
            return Some(ServiceAction::Dispatch("run.services.add", json!({ "kind": kind })));
        }
        let outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos))
        });
        if outside || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.adding = false;
        }
        None
    }

    pub(in crate::ide) fn tree(&mut self, ui: &mut Ui, p: &IdePalette) -> bool {
        let mut picked = false;
        let list = self.list.clone();
        for kind in kinds(&list) {
            let id = kind["kind"].as_str().unwrap_or_default().to_owned();
            let message = kind["nameMessage"].as_str().unwrap_or_default();
            let name = if message.is_empty() { id.clone() } else { t!(message) };
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let selected = self.chosen.is_none() && self.kind.as_deref() == Some(id.as_str());
            if selected {
                ui.painter().rect_filled(rect, 4, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            accessibility::tree_item(&response, 1, Some(true), selected, &name);
            let icon = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(12.0));
            glyph_folder(ui.painter(), icon, p.text_secondary);
            ui.painter().text(
                pos2(rect.left() + 28.0, rect.center().y),
                Align2::LEFT_CENTER,
                &name,
                theme::regular(13.0),
                p.text,
            );
            self.buttons.push((format!("kind:{id}"), rect));
            if response.clicked() {
                self.kind = Some(id.clone());
                self.chosen = None;
                picked = true;
            }
            for item in kind_items(kind) {
                let item_id = item["id"].as_str().unwrap_or_default().to_owned();
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                let selected = self.chosen.as_deref() == Some(item_id.as_str());
                if selected {
                    ui.painter().rect_filled(rect, 4, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 4, p.hover);
                }
                let label = item["name"].as_str().unwrap_or_default();
                accessibility::tree_item(&response, 2, None, selected, label);
                let running = item["running"] == true;
                let icon = Rect::from_center_size(pos2(rect.left() + 34.0, rect.center().y), Vec2::splat(10.0));
                glyph_run(ui.painter(), icon, if running { p.run } else { p.text_secondary });
                let status = ui.painter().text(
                    pos2(rect.right() - 8.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    state_text(item),
                    theme::regular(12.0),
                    p.text_secondary,
                );
                let clip =
                    Rect::from_min_max(pos2(rect.left() + 46.0, rect.top()), pos2(status.left() - 8.0, rect.bottom()));
                ui.painter().with_clip_rect(clip).text(
                    pos2(clip.left(), rect.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    theme::regular(13.0),
                    p.text,
                );
                self.buttons.push((format!("item:{item_id}"), rect));
                if response.clicked() {
                    self.chosen = Some(item_id);
                    self.kind = Some(id.clone());
                    picked = true;
                }
            }
        }
        picked
    }

    pub(in crate::ide) fn console(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        let Some(item) = self.chosen_item().cloned() else { return };
        let Some(run_id) = item["runId"].as_u64() else {
            super::super::run_window::message(ui, p, t!("services-not-started"));
            return;
        };
        if self.console.as_ref().is_none_or(|(shown, _, _)| *shown != run_id) {
            self.console = Some((run_id, None, Console::default()));
        }
        let revision = session
            .state::<Runs>()
            .and_then(|runs| runs.run(run_id))
            .map(|run| run.output.read(|output| output.revision()));
        if let Some((_, seen, console)) = &mut self.console
            && *seen != revision
        {
            *seen = revision;
            let params = json!({ "runId": run_id, "from": console.next(), "waitMs": 0 });
            if let Ok(answer) = registry.dispatch(session, "run.output.get", params)
                && let (Ok(lines), Some(next)) =
                    (serde_json::from_value(answer["lines"].clone()), answer["next"].as_u64())
            {
                console.extend(lines, usize::try_from(next).unwrap_or_default());
            }
        }
        if let Some((_, _, console)) = &mut self.console {
            let shown = Shown::All(console.all());
            let id = ui.scope_id().with(("services-console", run_id));
            console.show(ui, p, id, id.with("scroll"), &shown);
        }
    }
}

fn kind_items(kind: &Value) -> impl Iterator<Item = &Value> {
    kind["configurations"].as_array().into_iter().flatten()
}

fn state_text(item: &Value) -> String {
    match item["state"].as_str() {
        Some("running") => t!("services-state-running"),
        Some("stopped") => t!("services-state-stopped"),
        Some("failed" | "beforeLaunchFailed") => t!("services-state-failed"),
        Some(_) => t!("services-state-finished"),
        None => t!("services-not-started"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{Event, Modifiers, PointerButton, Pos2};
    use zen_run::RunEnv;
    use zen_run::fake::{FakeProvider, KIND};

    use super::*;

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        view: RunServicesView,
        actions: Vec<ServiceAction>,
        time: f64,
        _dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            zen_i18n::register(zen_workbench::LOCALES);
            let dir = tempfile::tempdir().unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            session.set_state(zen_workbench::build_tools::BuildEnv::default());
            registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
            session.state_mut::<Runs>().set_providers(vec![Arc::new(FakeProvider::new())], RunEnv::default());
            let ctx = egui::Context::default();
            theme::install(&ctx);
            Self { ctx, registry, session, view: RunServicesView::default(), actions: Vec::new(), time: 0.0, _dir: dir }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0))),
                ..Default::default()
            };
            let Self { ctx, registry, session, view, actions, time, .. } = self;
            let p = IdePalette::zen_dark();
            let mut output = ctx.run_ui(input, |ui| {
                view.buttons.clear();
                view.refresh(registry, session, *time);
                let (_, action) = view.toolbar(ui, &p, 10.0, 20.0);
                actions.extend(action);
                actions.extend(view.add_menu(ui, &p));
                let mut list = ui.new_child(
                    egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(0.0, 40.0), vec2(300.0, 500.0))),
                );
                view.tree(&mut list, &p);
            });
            output.textures_delta.clear();
        }

        fn click(&mut self, name: &str) {
            self.frame(Vec::new());
            let at = self.view.row(name).unwrap_or_else(|| panic!("{name} is not shown")).center();
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }

        fn perform(&mut self) {
            for action in std::mem::take(&mut self.actions) {
                let (command, params) = match action {
                    ServiceAction::Dispatch(command, params) => (command, params),
                    ServiceAction::Debug(params) => ("debug.session.start", params),
                };
                self.registry.dispatch(&mut self.session, command, params).unwrap();
            }
            self.view.invalidate();
            self.frame(Vec::new());
        }
    }

    #[test]
    fn add_service_shows_a_kind_whose_configurations_run_hide_and_go_from_the_toolbar() {
        let mut harness = Harness::new();
        let params = json!({ "configuration": FakeProvider::configuration("app") });
        let created = harness.registry.dispatch(&mut harness.session, "run.configuration.create", params).unwrap();
        let id = created["id"].as_str().unwrap().to_owned();
        harness.frame(Vec::new());
        assert!(harness.view.is_empty());
        harness.click("services-add");
        harness.click(&format!("add:{KIND}"));
        harness.perform();
        assert!(!harness.view.is_empty());
        assert!(harness.view.row(&format!("kind:{KIND}")).is_some());
        harness.click(&format!("item:{id}"));
        assert_eq!(harness.view.chosen.as_deref(), Some(id.as_str()));
        harness.click("services-run");
        assert_eq!(harness.actions, [ServiceAction::Dispatch("run.process.start", json!({ "configurationId": id }))]);
        harness.perform();
        assert_eq!(harness.view.chosen_item().unwrap()["state"], "finished");
        harness.click("services-hide");
        harness.perform();
        assert!(harness.view.row(&format!("item:{id}")).is_none(), "a hidden configuration is not shown");
        harness.click("services-restore");
        harness.perform();
        assert!(harness.view.row(&format!("item:{id}")).is_some());
        harness.click(&format!("kind:{KIND}"));
        harness.click("services-hide");
        assert_eq!(harness.actions, [ServiceAction::Dispatch("run.services.remove", json!({ "kind": KIND }))]);
        harness.perform();
        assert!(harness.view.is_empty());
    }
}
