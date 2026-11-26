use std::path::Path;

use egui::{Color32, Order, Rect, Stroke, Ui, Vec2, ViewportClass, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::agent::ActionError;
use super::app::Request;
use super::tool_stripes::kind;
use super::tool_windows::{
    Anchor, Bounds, DEFAULT_LAYOUT, SavedLayouts, Side, ToolWindowLayout, ViewMode, is_default_name,
};
use super::{IdePalette, IdeView, dialog_frame, metrics};
use crate::theme;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Detached {
    pub(super) id: &'static str,
    pub(super) mode: ViewMode,
    pub(super) title: String,
    pub(super) bounds: Option<Bounds>,
    pub(super) size: Vec2,
}

#[derive(Clone, Debug)]
pub(super) struct LayoutForm {
    purpose: Purpose,
    text: String,
    focused: bool,
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Purpose {
    SaveAsNew,
    Rename(String),
    Delete(String),
}

impl IdeView {
    pub(super) fn set_view_mode(&mut self, id: &str, mode: ViewMode) {
        self.tool_windows.set_mode(id, mode);
        if self.tool_windows.is_visible(id) {
            self.tool_windows.activate(id);
        }
        self.sync_tool_flags();
    }

    pub(super) fn follow_tool_focus(&mut self, ui: &Ui) {
        let Some(pos) = ui.input(|input| input.pointer.any_pressed().then(|| input.pointer.interact_pos()).flatten())
        else {
            return;
        };
        if egui::Popup::is_any_open(ui.ctx()) || self.overlay_open(ui.ctx()) || self.stripes.on_stripe(pos) {
            return;
        }
        let keep = self.stripes.drawn_at(pos);
        if self.tool_windows.lose_focus(keep) {
            self.sync_tool_flags();
        }
    }

    pub(super) fn undocked_tool_windows(
        &mut self,
        ui: &mut Ui,
        editor: Rect,
        registry: &Registry,
        session: &mut Session,
        project: &Path,
    ) {
        let shown: Vec<(&'static str, Side, f32)> = self
            .tool_windows
            .undocked()
            .filter_map(|window| Some((kind(&window.id)?.id, window.side, window.size)))
            .collect();
        for (id, side, size) in shown {
            let outer = undocked_rect(editor, side.anchor(), size);
            let inner = super::tool_stripes::inner_rect(outer, side.anchor());
            self.stripes.set_drawn(side, id, inner);
            let p = self.palette;
            egui::Area::new(ui.scope_id().with(("ide-tool-undocked", id)))
                .order(Order::Middle)
                .fixed_pos(outer.min)
                .constrain(false)
                .show(ui.ctx(), |ui| {
                    ui.set_min_size(outer.size());
                    ui.set_max_size(outer.size());
                    ui.painter().rect_filled(outer, 0, p.frame);
                    let shadow = egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: p.popup_shadow };
                    ui.painter().add(shadow.as_shape(inner, metrics::ISLAND_RADIUS));
                    self.tool_window_part(ui, id, inner, registry, session, project);
                    self.edge_handle(ui, side.anchor(), outer);
                });
        }
    }

    pub(super) fn detached_tools(&mut self) -> Vec<Detached> {
        let now: Vec<(&'static str, ViewMode, Option<Bounds>, Side, f32)> = self
            .tool_windows
            .detached()
            .filter_map(|window| Some((kind(&window.id)?.id, window.mode, window.bounds, window.side, window.size)))
            .collect();
        self.detached_open.retain(|(id, mode, _)| now.iter().any(|(other, now, ..)| other == id && now == mode));
        now.into_iter()
            .map(|(id, mode, bounds, side, size)| {
                let opened = match self.detached_open.iter().find(|(open, ..)| *open == id) {
                    Some((_, _, opened)) => *opened,
                    None => {
                        self.detached_open.push((id, mode, bounds));
                        bounds
                    }
                };
                let size = match side.anchor() {
                    Anchor::Left | Anchor::Right => vec2(size, 520.0),
                    Anchor::Bottom => vec2(760.0, size),
                };
                let title = kind(id).map(|kind| t!(kind.title)).unwrap_or_else(|| id.to_owned());
                Detached { id, mode, title, bounds: opened, size }
            })
            .collect()
    }

    pub(super) fn detached_tool_window(
        &mut self,
        ui: &mut Ui,
        id: &str,
        (class, size): (ViewClass, Vec2),
        registry: &Registry,
        session: &mut Session,
    ) -> bool {
        let Some(id) = kind(id).map(|kind| kind.id) else { return false };
        if !self.tool_windows.is_visible(id) || !self.tool_windows.mode_of(id).is_some_and(ViewMode::detached) {
            return true;
        }
        let mut changed = false;
        if class == ViewClass::Own {
            let info = ui.input(|input| input.viewport().clone());
            if info.close_requested() {
                self.hide_tool(id);
                return true;
            }
            if let (Some(outer), Some(inner)) = (info.outer_rect, info.inner_rect) {
                let bounds = Bounds { x: outer.min.x, y: outer.min.y, width: inner.width(), height: inner.height() };
                changed |= self.tool_windows.set_bounds(id, bounds);
            }
        }
        if ui.input(|input| input.pointer.any_pressed()) {
            self.tool_windows.activate(id);
            changed |= self.tool_windows.lose_focus(Some(id));
            self.sync_tool_flags();
        }
        let Some(project) = session.state::<Workbench>().and_then(|workbench| workbench.project().map(Path::to_owned))
        else {
            return changed;
        };
        let p = self.palette;
        let rect = match class {
            ViewClass::Own => ui.ctx().content_rect(),
            ViewClass::Embedded => Rect::from_min_size(ui.min_rect().min, size),
        };
        let inner = rect.shrink(metrics::GAP / 2.0);
        if class == ViewClass::Own {
            ui.painter().rect_filled(rect, 0, p.frame);
        }
        ui.allocate_rect(rect, egui::Sense::hover());
        self.tool_window_part(ui, id, inner, registry, session, &project);
        changed || !self.tool_windows.is_visible(id)
    }

    pub(super) fn saved_layout_action(&mut self, action: &str, params: &Value) -> Result<Value, ActionError> {
        use ActionError::Refused;
        let text = |key: &str| params.get(key).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
        let name = text("name");
        let names = {
            let mut names = vec![DEFAULT_LAYOUT.to_owned()];
            names.extend(self.overview.layouts.layouts.iter().map(|layout| layout.name.clone()));
            names.join(", ")
        };
        let current = self.overview.layouts.current.clone();
        let saved_name = |name: Option<&str>| -> Result<String, ActionError> {
            let Some(name) = name.map(str::to_owned).or_else(|| current.clone()) else {
                return Err(Refused("the default layout cannot be changed; name a saved one with `name`".to_owned()));
            };
            match self.overview.layouts.get(&name) {
                Some(found) => Ok(found.name.clone()),
                None if is_default_name(&name) => {
                    Err(Refused("the default layout cannot be changed; name a saved one with `name`".to_owned()))
                }
                None => Err(Refused(format!("no saved layout `{name}`; the layouts are {}", names))),
            }
        };
        match action {
            "view.layout.restoreDefault" => {
                self.restore_default_layout();
                Ok(Value::Null)
            }
            "view.layout.restore" | "view.layout.apply" => {
                let restored = name.map(str::to_owned).or_else(|| current.clone());
                let name = match (action, restored) {
                    (_, Some(name)) => name,
                    ("view.layout.restore", None) => DEFAULT_LAYOUT.to_owned(),
                    _ => return Err(Refused(format!("give `name`: one of {}", names))),
                };
                if !self.apply_layout(&name) {
                    return Err(Refused(format!("no layout `{name}`; the layouts are {}", names)));
                }
                Ok(json!({ "current": self.layouts_state()["current"] }))
            }
            "view.layout.saveAsNew" => {
                let Some(name) = name else {
                    self.layout_form = Some(LayoutForm::new(Purpose::SaveAsNew));
                    return Ok(json!({ "dialog": "saveLayout" }));
                };
                match self.save_layout_as(name) {
                    Ok(name) => Ok(json!({ "name": name })),
                    Err(message) => Err(Refused(message)),
                }
            }
            "view.layout.saveChanges" => {
                let name = saved_name(name)?;
                let mut saved = self.overview.layouts.clone();
                saved.save_changes(&name, &self.tool_windows);
                self.keep_layouts(saved);
                Ok(json!({ "saved": name }))
            }
            "view.layout.rename" => {
                let name = saved_name(name)?;
                let Some(new_name) = text("newName") else {
                    self.layout_form = Some(LayoutForm::new(Purpose::Rename(name)));
                    return Ok(json!({ "dialog": "renameLayout" }));
                };
                match self.rename_layout(&name, new_name) {
                    Ok(new_name) => Ok(json!({ "name": new_name })),
                    Err(message) => Err(Refused(message)),
                }
            }
            "view.layout.delete" => {
                let Some(name) = name else {
                    return Err(Refused(format!("give `name`: one of the saved layouts, {}", names)));
                };
                let name = saved_name(Some(name))?;
                if current.as_deref().is_some_and(|current| current.eq_ignore_ascii_case(&name)) {
                    return Err(Refused(format!(
                        "`{name}` is the current layout and cannot be deleted; apply another one first"
                    )));
                }
                if params.get("ask").and_then(Value::as_bool) == Some(true) {
                    self.layout_form = Some(LayoutForm::new(Purpose::Delete(name)));
                    return Ok(json!({ "dialog": "deleteLayout" }));
                }
                self.delete_layout(&name);
                Ok(json!({ "deleted": name }))
            }
            _ => Err(Refused(format!("no layout action `{action}`"))),
        }
    }

    fn rename_layout(&mut self, name: &str, new_name: &str) -> Result<String, String> {
        let mut saved = self.overview.layouts.clone();
        match saved.rename(name, new_name) {
            Some(Ok(new_name)) => {
                self.keep_layouts(saved);
                Ok(new_name)
            }
            Some(Err(error)) => Err(t!(error.message())),
            None => Err(format!("no saved layout `{name}`")),
        }
    }

    fn delete_layout(&mut self, name: &str) {
        let mut saved = self.overview.layouts.clone();
        if saved.delete(name) {
            self.keep_layouts(saved);
        }
    }

    pub(super) fn apply_layout(&mut self, name: &str) -> bool {
        if is_default_name(name) {
            self.restore_default_layout();
            return true;
        }
        let Some(found) = self.overview.layouts.get(name).cloned() else { return false };
        self.tool_windows = ToolWindowLayout::applied(found.layout);
        self.sync_tool_flags();
        let mut saved = self.overview.layouts.clone();
        saved.current = Some(found.name);
        self.keep_layouts(saved);
        true
    }

    pub(super) fn save_layout_as(&mut self, name: &str) -> Result<String, String> {
        let mut saved = self.overview.layouts.clone();
        let name = saved.save_as(name, &self.tool_windows).map_err(|error| t!(error.message()))?;
        self.keep_layouts(saved);
        Ok(name)
    }

    pub(super) fn default_layout_current(&mut self) {
        if self.overview.layouts.current.is_some() {
            let saved = SavedLayouts { current: None, ..self.overview.layouts.clone() };
            self.keep_layouts(saved);
        }
    }

    fn keep_layouts(&mut self, saved: SavedLayouts) {
        self.overview.layouts = saved.clone();
        self.requests.push(Request::Layouts(Box::new(saved)));
    }

    pub(super) fn layouts_state(&self) -> Value {
        let saved: Vec<&str> = self.overview.layouts.layouts.iter().map(|layout| layout.name.as_str()).collect();
        json!({ "saved": saved, "current": self.overview.layouts.current.as_deref().unwrap_or(DEFAULT_LAYOUT) })
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn layout_items(&self) -> Vec<(String, String, bool)> {
        let current = self.overview.layouts.current.as_deref();
        let mut items = vec![(t!("menu-window-layout-default"), DEFAULT_LAYOUT.to_owned(), current.is_none())];
        items.extend(self.overview.layouts.layouts.iter().map(|layout| {
            let on = current.is_some_and(|current| current.eq_ignore_ascii_case(&layout.name));
            (layout.name.clone(), layout.name.clone(), on)
        }));
        items
    }

    pub(super) fn layout_dialog(&mut self, ui: &Ui) {
        let Some(mut form) = self.layout_form.take() else { return };
        let p = self.palette;
        let Some(answer) = form.show(ui, &p) else {
            self.layout_form = Some(form);
            return;
        };
        let Some(text) = answer else { return };
        let done = match &form.purpose {
            Purpose::SaveAsNew => self.save_layout_as(&text).map(|_| ()),
            Purpose::Rename(name) => self.rename_layout(&name.clone(), &text).map(|_| ()),
            Purpose::Delete(name) => {
                self.delete_layout(&name.clone());
                Ok(())
            }
        };
        if let Err(message) = done {
            form.error = Some(message);
            form.focused = false;
            self.layout_form = Some(form);
        }
    }

    pub(super) fn layout_dialog_open(&self) -> bool {
        self.layout_form.is_some()
    }

    pub(super) fn layout_dialog_name(&self) -> Option<&'static str> {
        self.layout_form.as_ref().map(|form| match form.purpose {
            Purpose::SaveAsNew => "saveLayout",
            Purpose::Rename(_) => "renameLayout",
            Purpose::Delete(_) => "deleteLayout",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ViewClass {
    Own,
    Embedded,
}

impl From<ViewportClass> for ViewClass {
    fn from(class: ViewportClass) -> Self {
        match class {
            ViewportClass::Deferred | ViewportClass::Immediate | ViewportClass::Root => ViewClass::Own,
            ViewportClass::EmbeddedWindow => ViewClass::Embedded,
        }
    }
}

pub(super) fn undocked_rect(room: Rect, anchor: Anchor, size: f32) -> Rect {
    let across = |extent: f32| size.min((extent - metrics::GAP).max(0.0));
    match anchor {
        Anchor::Left => Rect::from_min_size(room.min, vec2(across(room.width()), room.height())),
        Anchor::Right => {
            let width = across(room.width());
            Rect::from_min_max(pos2(room.right() - width, room.top()), room.max)
        }
        Anchor::Bottom => {
            let height = across(room.height());
            Rect::from_min_max(pos2(room.left(), room.bottom() - height), room.max)
        }
    }
}

impl LayoutForm {
    fn new(purpose: Purpose) -> Self {
        let text = match &purpose {
            Purpose::Rename(name) => name.clone(),
            Purpose::SaveAsNew | Purpose::Delete(_) => String::new(),
        };
        Self { purpose, text, focused: false, error: None }
    }

    fn show(&mut self, ui: &Ui, p: &IdePalette) -> Option<Option<String>> {
        let mut answer = None;
        let deleting = match &self.purpose {
            Purpose::Delete(name) => Some(name.clone()),
            Purpose::SaveAsNew | Purpose::Rename(_) => None,
        };
        let valid = deleting.is_some() || !self.text.trim().is_empty();
        let (title, ok) = match &self.purpose {
            Purpose::SaveAsNew => ("ide-layout-save-title", "ide-layout-save-ok"),
            Purpose::Rename(_) => ("ide-layout-rename-title", "ide-layout-rename-ok"),
            Purpose::Delete(_) => ("ide-layout-delete-title", "ide-layout-delete-ok"),
        };
        let modal =
            egui::Modal::new(ui.scope_id().with("ide-save-layout")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(380.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(t!(title)).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(14.0);
                if let Some(name) = &deleting {
                    ui.label(
                        egui::RichText::new(t!("ide-layout-delete-question", name = name.as_str()))
                            .font(theme::regular(13.0))
                            .color(p.text),
                    );
                    if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                        answer = Some(Some(name.clone()));
                    }
                } else {
                    self.name_field(ui, p, valid, &mut answer);
                }
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let ok =
                        egui::Button::new(egui::RichText::new(t!(ok)).font(theme::regular(13.0)).color(p.accent_text))
                            .min_size(vec2(84.0, 30.0))
                            .corner_radius(6)
                            .fill(if valid { p.accent } else { p.accent.gamma_multiply(0.4) });
                    if ui.add_enabled(valid, ok).clicked() {
                        answer = Some(Some(deleting.clone().unwrap_or_else(|| self.text.trim().to_owned())));
                    }
                    let cancel = egui::Button::new(
                        egui::RichText::new(t!("ide-layout-save-cancel")).font(theme::regular(13.0)).color(p.text),
                    )
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, p.button_border));
                    if ui.add(cancel).clicked() {
                        answer = Some(None);
                    }
                });
            });
        if answer.is_none() && modal.should_close() {
            answer = Some(None);
        }
        answer
    }

    fn name_field(&mut self, ui: &mut Ui, p: &IdePalette, valid: bool, answer: &mut Option<Option<String>>) {
        ui.label(egui::RichText::new(t!("ide-layout-save-name")).font(theme::regular(12.5)).color(p.text_secondary));
        ui.add_space(6.0);
        let field = egui::Frame::new()
            .fill(p.text_field)
            .stroke(Stroke::new(1.5, if self.error.is_some() { p.excluded } else { p.accent }))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                egui::TextEdit::singleline(&mut self.text)
                    .id_salt("ide-save-layout-name")
                    .frame(egui::Frame::NONE)
                    .font(theme::regular(14.0))
                    .desired_width(f32::INFINITY)
                    .show(ui)
            })
            .inner;
        if !self.focused {
            field.response.request_focus();
            self.focused = true;
        }
        if field.response.changed() {
            self.error = None;
        }
        if field.response.lost_focus()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            if valid {
                *answer = Some(Some(self.text.trim().to_owned()));
            } else {
                field.response.request_focus();
            }
        }
        if let Some(error) = &self.error {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(error).font(theme::regular(12.5)).color(p.excluded));
        }
    }
}
