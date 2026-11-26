use std::path::PathBuf;
use std::time::Duration;

use egui::text::CCursorRange;
use egui::{Color32, Key, Stroke, Ui, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::refactoring_preview::{PreviewTab, RenamePreview};
use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

const POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RenameTarget {
    pending: bool,
    placeholder: String,
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RenameAction {
    Refactor(String),
    Preview(String),
    Cancel,
}

#[derive(Debug)]
pub(super) enum RenameOutcome {
    Notice(String),
    Preview(Box<PreviewTab>),
    Closed,
}

#[derive(Debug, Default)]
pub(super) struct RenameDialog {
    state: Option<State>,
}

#[derive(Debug)]
enum State {
    Preparing { path: PathBuf, offset: usize },
    Open(Form),
}

#[derive(Debug)]
struct Form {
    path: PathBuf,
    offset: usize,
    name: String,
    text: String,
    focused: bool,
    error: Option<String>,
    waiting: Option<RenameAction>,
    buttons: [egui::Rect; 3],
}

impl RenameDialog {
    pub(super) fn open(&mut self, path: PathBuf, offset: usize) {
        self.state = Some(State::Preparing { path, offset });
    }

    pub(super) fn close(&mut self) {
        self.state = None;
    }

    pub(super) fn is_open(&self) -> bool {
        self.state.is_some()
    }

    pub(super) fn update(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
    ) -> Vec<RenameOutcome> {
        let mut outcomes = Vec::new();
        match self.state.take() {
            None => {}
            Some(State::Preparing { path, offset }) => {
                let params = json!({ "path": path, "offset": offset });
                match run(registry, session, "refactor.rename.prepare", params) {
                    Err(message) => outcomes.extend(message.map(RenameOutcome::Notice)),
                    Ok(value) => {
                        let target: RenameTarget = serde_json::from_value(value).unwrap_or_default();
                        if let Some(error) = target.error {
                            outcomes.push(RenameOutcome::Notice(error));
                        } else if target.pending {
                            ui.ctx().request_repaint_after(POLL);
                            self.state = Some(State::Preparing { path, offset });
                        } else {
                            let text = target.placeholder.clone();
                            let name = target.placeholder;
                            let form = Form {
                                path,
                                offset,
                                name,
                                text,
                                focused: false,
                                error: None,
                                waiting: None,
                                buttons: [egui::Rect::NOTHING; 3],
                            };
                            self.state = Some(State::Open(form));
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }
            Some(State::Open(mut form)) => {
                if let Some(action) = form.show(ui, p) {
                    form.error = None;
                    form.waiting = Some(action);
                }
                match form.waiting.clone() {
                    None => self.state = Some(State::Open(form)),
                    Some(RenameAction::Cancel) => {
                        let _ = run(registry, session, "refactor.rename.cancel", json!({}));
                        outcomes.push(RenameOutcome::Closed);
                    }
                    Some(RenameAction::Refactor(name)) if name == form.name => {
                        let _ = run(registry, session, "refactor.rename.cancel", json!({}));
                        outcomes.push(RenameOutcome::Closed);
                    }
                    Some(action @ (RenameAction::Refactor(_) | RenameAction::Preview(_))) => {
                        self.state = form.advance(ui, registry, session, &action, &mut outcomes).map(State::Open);
                    }
                }
            }
        }
        outcomes
    }
}

impl RenameAction {
    pub(super) fn command(&self, path: &std::path::Path, offset: usize) -> (&'static str, Value) {
        match self {
            Self::Refactor(name) => {
                ("refactor.rename.apply", json!({ "newName": name, "path": path, "offset": offset }))
            }
            Self::Preview(name) => {
                ("refactor.rename.preview", json!({ "newName": name, "path": path, "offset": offset }))
            }
            Self::Cancel => ("refactor.rename.cancel", json!({})),
        }
    }
}

impl Form {
    fn advance(
        mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        action: &RenameAction,
        outcomes: &mut Vec<RenameOutcome>,
    ) -> Option<Self> {
        let name = match action {
            RenameAction::Refactor(name) | RenameAction::Preview(name) => name.clone(),
            RenameAction::Cancel => return None,
        };
        let (command, params) = RenameAction::Preview(name.clone()).command(&self.path, self.offset);
        let preview = match run(registry, session, command, params) {
            Ok(value) => serde_json::from_value::<RenamePreview>(value).unwrap_or_default(),
            Err(message) => {
                self.waiting = None;
                self.error = message;
                return Some(self);
            }
        };
        if preview.pending {
            ui.ctx().request_repaint_after(POLL);
            return Some(self);
        }
        if let Some(error) = preview.error {
            self.waiting = None;
            self.error = Some(error);
            self.focused = false;
            ui.ctx().request_repaint();
            return Some(self);
        }
        match action {
            RenameAction::Preview(_) => outcomes.push(RenameOutcome::Preview(Box::new(PreviewTab::new(
                name,
                self.path.clone(),
                self.offset,
                preview,
            )))),
            _ => {
                let (command, params) = action.command(&self.path, self.offset);
                if let Err(Some(message)) = run(registry, session, command, params) {
                    outcomes.push(RenameOutcome::Notice(message));
                }
                outcomes.push(RenameOutcome::Closed);
            }
        }
        None
    }

    fn show(&mut self, ui: &Ui, p: &IdePalette) -> Option<RenameAction> {
        let mut action = None;
        let busy = self.waiting.is_some();
        let valid = !self.text.trim().is_empty() && !busy;
        let modal = egui::Modal::new(ui.scope_id().with("rename-dialog")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("ide-rename-title")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(14.0);
            let label = ui.label(
                egui::RichText::new(t!("ide-rename-label", name = self.name.as_str()))
                    .font(theme::regular(12.5))
                    .color(p.text_secondary),
            );
            ui.add_space(6.0);
            let border = if self.error.is_some() { p.excluded } else { p.accent };
            let field = egui::Frame::new()
                .fill(p.text_field)
                .stroke(Stroke::new(1.5, border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    egui::TextEdit::singleline(&mut self.text)
                        .id_salt("rename-name")
                        .frame(egui::Frame::NONE)
                        .font(theme::regular(14.0))
                        .desired_width(f32::INFINITY)
                        .show(ui)
                })
                .inner;
            crate::accessibility::edit(&field.response, |node| node.push_labelled_by(label.id.accesskit_id()));
            if !self.focused {
                field.response.request_focus();
                let mut state = field.state.clone();
                state.cursor.set_char_range(Some(CCursorRange::select_all_str(&self.text)));
                state.store(ui.ctx(), field.response.id);
                self.focused = true;
            }
            if field.response.changed() {
                self.error = None;
            }
            if field.response.lost_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter))
            {
                if valid {
                    action = Some(RenameAction::Refactor(self.text.trim().to_owned()));
                } else {
                    field.response.request_focus();
                }
            }
            if let Some(error) = &self.error {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(error).font(theme::regular(12.5)).color(p.excluded));
            }
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let refactor = egui::Button::new(
                    egui::RichText::new(t!("ide-rename-refactor")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(if valid { p.accent } else { p.accent.gamma_multiply(0.4) });
                let response = ui.add_enabled(valid, refactor);
                self.buttons[0] = response.rect;
                if response.clicked() {
                    action = Some(RenameAction::Refactor(self.text.trim().to_owned()));
                }
                let cancel = egui::Button::new(
                    egui::RichText::new(t!("ide-rename-cancel")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                let response = ui.add(cancel);
                self.buttons[1] = response.rect;
                if response.clicked() {
                    action = Some(RenameAction::Cancel);
                }
                let preview = egui::Button::new(
                    egui::RichText::new(t!("ide-rename-preview")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                let response = ui.add_enabled(valid, preview);
                self.buttons[2] = response.rect;
                if response.clicked() {
                    action = Some(RenameAction::Preview(self.text.trim().to_owned()));
                }
            });
        });
        if action.is_none() && modal.should_close() {
            action = Some(RenameAction::Cancel);
        }
        action
    }
}

fn run(registry: &Registry, session: &mut Session, command: &str, params: Value) -> Result<Value, Option<String>> {
    registry.dispatch(session, command, params).map_err(|error| match error {
        DispatchError::Failed { error, .. } => Some(error.message),
        DispatchError::Unavailable { .. } => None,
        other => Some(other.to_string()),
    })
}

impl IdeView {
    pub(super) fn open_rename(&mut self, session: &Session) {
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return };
        if !self.rename.is_open() {
            self.rename.open(document.path().to_owned(), document.selections().primary().head);
            if self.find_window.close_refactoring() && self.tool_visible("find") {
                self.hide_tool("find");
            }
        }
    }

    pub(super) fn rename_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        for outcome in self.rename.update(ui, registry, session, &palette) {
            match outcome {
                RenameOutcome::Notice(message) => self.post(ui, message),
                RenameOutcome::Preview(tab) => {
                    self.find_window.show_refactoring(*tab);
                    self.show_tool("find");
                }
                RenameOutcome::Closed => self.editor_view.focus(),
            }
        }
    }
}

#[cfg(test)]
mod tests;
