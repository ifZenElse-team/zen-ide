use egui::{Rect, Sense, Ui, Vec2, pos2};
use serde_json::json;
use zen_core::{Command, Registry, Session};
use zen_i18n::t;
use zen_workbench::Document;
use zen_workbench::Workbench;
use zen_workbench::read_only::ToggleReadOnly;

use super::{IdeView, Side, hint};
use crate::accessibility;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LockState {
    pub(super) closed: bool,
    pub(super) toggles: bool,
    pub(super) name: String,
    pub(super) detail: String,
}

impl LockState {
    pub(super) fn of(document: &Document) -> Self {
        let reason = document.read_only();
        let closed = reason.is_some();
        let toggles = document.is_locked() || !closed;
        let name = if closed { t!("ide-status-read-only") } else { t!("ide-status-writable") };
        let detail = match (toggles, reason) {
            (false, Some(reason)) => reason.to_owned(),
            _ => menu_name(document.is_locked()),
        };
        Self { closed, toggles, name, detail }
    }
}

fn menu_name(locked: bool) -> String {
    if locked { t!("menu-file-make-writable") } else { t!("menu-file-make-read-only") }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
pub(super) fn menu_title(session: &Session, action: &str) -> Option<String> {
    if action != ToggleReadOnly::ID {
        return None;
    }
    let locked = session.state::<Workbench>()?.active_document()?.is_locked();
    Some(menu_name(locked))
}

impl IdeView {
    pub(super) fn read_only_widget(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        right: f32,
        y: f32,
    ) -> f32 {
        if zen_workbench::workspaces::tab_in_front(session).is_some() {
            return right;
        }
        let Some(state) = session.state::<Workbench>().and_then(Workbench::active_document).map(LockState::of) else {
            return right;
        };
        let p = self.palette;
        let rect = Rect::from_center_size(pos2(right - 11.0, y), Vec2::splat(22.0));
        let sense = if state.toggles { Sense::click() } else { Sense::hover() };
        let response = ui.interact(rect, ui.scope_id().with("status-read-only"), sense);
        if state.toggles && response.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        crate::icons::lock(ui.painter(), rect.shrink(4.0), p.text_secondary, !state.closed);
        hint(ui, &response, &state.name, Some(&state.detail), Side::Left);
        if state.toggles {
            accessibility::button(&response, &state.name);
            accessibility::edit(&response, |node| node.set_description(state.detail.clone()));
        } else {
            accessibility::name(&response, egui::Role::Label, &state.name);
        }
        if state.toggles && response.clicked() {
            self.dispatch(ui, registry, session, ToggleReadOnly::ID, json!({}));
        }
        rect.left()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zen_core::Registry;

    use super::*;

    #[test]
    fn the_lock_is_open_for_a_writable_file_and_closed_for_one_that_cannot_be_edited() {
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::register(crate::toolbox::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let (file, odd) = (dir.path().join("main.go"), dir.path().join("odd.txt"));
        std::fs::write(&file, "package main\n").unwrap();
        std::fs::write(&odd, b"caf\xe9\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        let state = |session: &Session| LockState::of(session.state::<Workbench>().unwrap().active_document().unwrap());
        assert_eq!(menu_title(&session, ToggleReadOnly::ID), None, "no file");

        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let open = state(&session);
        assert!(!open.closed && open.toggles);
        assert_eq!(open.name, t!("ide-status-writable"));
        assert_eq!(open.detail, t!("menu-file-make-read-only"));
        assert_eq!(menu_title(&session, ToggleReadOnly::ID), Some(t!("menu-file-make-read-only")));
        assert_eq!(menu_title(&session, "file.document.save"), None);

        registry.dispatch(&mut session, ToggleReadOnly::ID, json!({})).unwrap();
        let locked = state(&session);
        assert!(locked.closed && locked.toggles);
        assert_eq!(locked.name, t!("ide-status-read-only"));
        assert_eq!(locked.detail, t!("menu-file-make-writable"));
        assert_eq!(menu_title(&session, ToggleReadOnly::ID), Some(t!("menu-file-make-writable")));
        registry.dispatch(&mut session, ToggleReadOnly::ID, json!({})).unwrap();

        registry.dispatch(&mut session, "file.document.open", json!({ "path": odd })).unwrap();
        let encoding = state(&session);
        assert!(encoding.closed && !encoding.toggles, "an encoding is not a permission");
        assert!(encoding.detail.contains("odd.txt"), "{}", encoding.detail);
    }
}
