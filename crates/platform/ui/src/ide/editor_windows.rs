use std::path::Path;

use egui::{Rect, Ui, Vec2, ViewportBuilder, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_workbench::Workbench;
use zen_workbench::groups::{GroupId, WindowBounds};

use super::IdeView;
use super::view_modes::ViewClass;

const NEW_WINDOW: Vec2 = vec2(800.0, 600.0);
const LEAST: [f32; 2] = [320.0, 200.0];

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DetachedEditor {
    pub(super) group: GroupId,
    pub(super) title: String,
    pub(super) bounds: Option<WindowBounds>,
}

pub(super) fn editor_window_builder(editor: &DetachedEditor, product_id: &str) -> ViewportBuilder {
    let size = editor.bounds.map_or(NEW_WINDOW, |bounds| vec2(bounds.width, bounds.height));
    let builder = ViewportBuilder::default()
        .with_title(editor.title.clone())
        .with_app_id(format!("space.ifzenelse.{product_id}"))
        .with_inner_size(size)
        .with_min_inner_size(LEAST);
    match editor.bounds {
        Some(bounds) => builder.with_position([bounds.x, bounds.y]),
        None => builder,
    }
}

pub(super) fn window_bounds_at(at: egui::Pos2) -> WindowBounds {
    WindowBounds { x: at.x, y: at.y, width: NEW_WINDOW.x, height: NEW_WINDOW.y }
}

impl IdeView {
    pub(super) fn detached_editors(&mut self, session: &Session) -> Vec<DetachedEditor> {
        let Some(workbench) = session.state::<Workbench>() else { return Vec::new() };
        let groups = workbench.groups();
        let opened = &mut self.groups_ui.opened_windows;
        opened.retain(|(group, _)| groups.window(*group).is_some());
        groups
            .windows()
            .iter()
            .map(|window| {
                let group = window.group.id;
                let bounds = match opened.iter().find(|(open, _)| *open == group) {
                    Some((_, bounds)) => *bounds,
                    None => {
                        opened.push((group, window.bounds));
                        window.bounds
                    }
                };
                let title = window.group.active_path().and_then(Path::file_name);
                let title = title.map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                DetachedEditor { group, title, bounds }
            })
            .collect()
    }

    pub(super) fn window_to_raise(&mut self, session: &Session) -> Option<Option<GroupId>> {
        let groups = session.state::<Workbench>()?.groups();
        let now = groups.window(groups.focused()).map(|window| window.group.id);
        let before = std::mem::replace(&mut self.groups_ui.focused_window, now);
        (before != now).then_some(now)
    }

    pub(super) fn detached_editor_window(
        &mut self,
        ui: &mut Ui,
        group: GroupId,
        (class, size): (ViewClass, Vec2),
        registry: &Registry,
        session: &mut Session,
    ) -> bool {
        let shown = |session: &Session| {
            session.state::<Workbench>().is_some_and(|workbench| workbench.groups().window(group).is_some())
        };
        if !shown(session) {
            return true;
        }
        let mut changed = false;
        if class == ViewClass::Own {
            let info = ui.input(|input| input.viewport().clone());
            if info.close_requested() {
                self.dispatch(ui, registry, session, "editor.window.close", json!({ "group": group }));
                return true;
            }
            if let (Some(outer), Some(inner)) = (info.outer_rect, info.inner_rect) {
                let bounds =
                    WindowBounds { x: outer.min.x, y: outer.min.y, width: inner.width(), height: inner.height() };
                let moved = session
                    .state::<Workbench>()
                    .and_then(|workbench| workbench.groups().window(group))
                    .is_some_and(|window| window.bounds != Some(bounds));
                if moved {
                    let params = json!({ "group": group, "bounds": bounds });
                    let _ = registry.dispatch(session, "editor.window.setBounds", params);
                }
            }
        }
        let pressed = ui.input(|input| input.pointer.any_pressed());
        let focused = session.state::<Workbench>().is_some_and(|workbench| workbench.groups().focused() == group);
        if pressed && !focused {
            let _ = registry.dispatch(session, "editor.split.focus", json!({ "group": group }));
            changed = true;
        }
        self.follow_groups(ui.ctx(), ui.scope_id(), session);
        let rect = match class {
            ViewClass::Own => ui.ctx().content_rect(),
            ViewClass::Embedded => Rect::from_min_size(ui.min_rect().min, size),
        };
        ui.painter().rect_filled(rect, 0, self.palette.frame);
        ui.allocate_rect(rect, egui::Sense::hover());
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt(("editor-window", group)).max_rect(rect));
        if group == self.groups_ui.view_group {
            session.state_mut::<Workbench>().present(group);
            self.editor_group(&mut child, registry, session, group, true);
            let workbench = session.state_mut::<Workbench>();
            workbench.present(workbench.groups().focused());
            self.shortcuts(ui, registry, session);
        } else {
            self.waiting_group(&mut child, registry, session, group, false);
        }
        self.dragged_tab(ui, registry, session, &[], Some(group));
        changed || !shown(session) || !focused
    }

    pub(super) fn drop_outside(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        (from, index): (GroupId, usize),
        pointer: Option<egui::Pos2>,
    ) {
        let origin = ui.input(|input| input.viewport().inner_rect).map(|rect| rect.min);
        let screen = pointer.zip(origin).map(|(at, origin)| origin + at.to_vec2());
        let window = session.state::<Workbench>().is_some_and(|workbench| workbench.groups().window(from).is_some());
        let over_main = screen.zip(self.main_screen).is_some_and(|(at, main)| main.contains(at));
        if window && over_main {
            let main = session.state::<Workbench>().map(|workbench| workbench.groups().main_focused());
            let params = json!({ "from": from, "index": index, "to": main });
            self.dispatch(ui, registry, session, "editor.split.moveTab", params);
            return;
        }
        if window
            && session
                .state::<Workbench>()
                .and_then(|workbench| workbench.groups().window(from))
                .is_some_and(|window| window.group.tabs.len() < 2)
        {
            return;
        }
        let bounds = screen.map(|at| window_bounds_at(pos2(at.x - 40.0, at.y - 16.0)));
        let params = json!({ "group": from, "index": index, "bounds": bounds });
        self.dispatch(ui, registry, session, "editor.window.openTab", params);
    }
}
