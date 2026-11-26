use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::macros::{MacroStep, Recorder};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::macros::{Macro, MacroState};

use super::agent::ActionError;
use super::keymap_page::action_title;
use super::{IdePalette, IdeView, dialog_frame};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const LIST_WIDTH: f32 = 260.0;
const STEPS_WIDTH: f32 = 340.0;
const LIST_HEIGHT: f32 = 260.0;

#[derive(Default)]
pub(super) struct MacroDialogs {
    name: Option<NameForm>,
    edit: Option<EditMacros>,
    play: Option<PlaySaved>,
}

#[derive(Default)]
struct NameForm {
    text: String,
    focused: bool,
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Change {
    Rename { name: String, new_name: String },
    Remove(String),
    RemoveStep { name: String, index: usize },
}

struct EditMacros {
    macros: Vec<Macro>,
    selected: Option<usize>,
    step: Option<usize>,
    changes: Vec<Change>,
    rename: Option<NameForm>,
}

struct PlaySaved {
    names: Vec<String>,
    selected: usize,
}

enum NameAnswer {
    Keep(String),
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditAnswer {
    Ok,
    Cancel,
}

impl MacroDialogs {
    pub(super) fn open_names(&self) -> Vec<&'static str> {
        [
            (self.name.is_some(), "macroName"),
            (self.edit.is_some(), "editMacros"),
            (self.play.is_some(), "playSavedMacros"),
        ]
        .into_iter()
        .filter_map(|(open, name)| open.then_some(name))
        .collect()
    }

    pub(super) fn is_open(&self) -> bool {
        self.name.is_some() || self.edit.is_some() || self.play.is_some()
    }

    fn open_name(&mut self) {
        self.name = Some(NameForm::default());
    }

    fn open_edit(&mut self, state: &MacroState) {
        let selected = (!state.macros.is_empty()).then_some(0);
        self.edit =
            Some(EditMacros { macros: state.macros.clone(), selected, step: None, changes: Vec::new(), rename: None });
    }

    fn open_play(&mut self, state: &MacroState) {
        let names = state.macros.iter().map(|saved| saved.name.clone()).collect();
        self.play = Some(PlaySaved { names, selected: 0 });
    }
}

impl EditMacros {
    fn selected_macro(&self) -> Option<&Macro> {
        self.selected.and_then(|index| self.macros.get(index))
    }

    fn apply(&mut self, change: Change) {
        match &change {
            Change::Rename { name, new_name } => {
                if let Some(found) = self.macros.iter_mut().find(|saved| saved.name == *name) {
                    new_name.clone_into(&mut found.name);
                }
            }
            Change::Remove(name) => {
                self.macros.retain(|saved| saved.name != *name);
                self.selected =
                    self.selected.filter(|_| !self.macros.is_empty()).map(|at| at.min(self.macros.len() - 1));
                self.step = None;
            }
            Change::RemoveStep { name, index } => {
                if let Some(found) = self.macros.iter_mut().find(|saved| saved.name == *name)
                    && *index < found.steps.len()
                {
                    found.steps.remove(*index);
                    self.step = self.step.filter(|_| !found.steps.is_empty()).map(|at| at.min(found.steps.len() - 1));
                }
            }
        }
        self.changes.push(change);
    }

    fn rename_to(&mut self, new_name: &str) -> Result<(), String> {
        let new_name = new_name.trim();
        let Some(name) = self.selected_macro().map(|saved| saved.name.clone()) else { return Ok(()) };
        if new_name.is_empty() {
            return Err(t!("workbench-macro-empty-name"));
        }
        if new_name != name && self.macros.iter().any(|saved| saved.name == new_name) {
            return Err(t!("workbench-macro-exists", name = new_name));
        }
        if new_name != name {
            self.apply(Change::Rename { name, new_name: new_name.to_owned() });
        }
        Ok(())
    }

    fn remove_selected(&mut self) {
        if let Some(name) = self.selected_macro().map(|saved| saved.name.clone()) {
            self.apply(Change::Remove(name));
        }
    }

    fn remove_step(&mut self) {
        if let (Some(name), Some(index)) = (self.selected_macro().map(|saved| saved.name.clone()), self.step) {
            self.apply(Change::RemoveStep { name, index });
        }
    }
}

pub(super) fn step_title(step: &MacroStep, registry: &Registry) -> String {
    let text = || step.params["text"].as_str().unwrap_or_default().replace('\n', "⏎");
    match step.command.as_str() {
        "editor.edit.type" | "editor.edit.insert" => return t!("ide-macro-step-type", text = text()),
        "editor.edit.paste" => return t!("ide-macro-step-paste", text = text()),
        _ => {}
    }
    let mut params = step.params.clone();
    if let Some(fields) = params.as_object_mut() {
        fields.remove("visual");
    }
    let bindings = crate::keymap::current().bindings();
    if let Some(binding) = bindings.iter().find(|binding| {
        binding.command == step.command
            && (binding.params == params
                || (binding.params == json!({}) && params.as_object().is_none_or(|p| p.is_empty())))
    }) {
        return action_title(&binding.action);
    }
    match registry.info(&step.command) {
        Some(info) => super::search_everywhere::command_title(info).0,
        None => step.command.clone(),
    }
}

fn recording(session: &Session) -> bool {
    zen_core::macros::is_recording(session)
}

fn macro_state(registry: &Registry, session: &mut Session) -> Result<MacroState, ActionError> {
    let value = registry.dispatch(session, "editor.macro.list", json!({})).map_err(ActionError::Dispatch)?;
    Ok(MacroState {
        recording: value["recording"].as_bool().unwrap_or(false),
        recorded: serde_json::from_value(value["recorded"].clone()).unwrap_or_default(),
        last: serde_json::from_value(value["last"].clone()).unwrap_or_default(),
        macros: serde_json::from_value(value["macros"].clone()).unwrap_or_default(),
    })
}

impl IdeView {
    pub(super) fn macro_action(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, ActionError> {
        match action {
            "view.macro.toggleRecording" if recording(session) => match params.get("name").and_then(Value::as_str) {
                Some(name) => registry
                    .dispatch(session, "editor.macro.stopRecording", json!({ "name": name }))
                    .map_err(ActionError::Dispatch),
                None => {
                    self.editor_view.release_focus(ctx);
                    self.macro_dialogs.open_name();
                    Ok(Value::Null)
                }
            },
            "view.macro.toggleRecording" => {
                let started = registry.dispatch(session, "editor.macro.startRecording", json!({}));
                self.editor_view.focus();
                started.map_err(ActionError::Dispatch)
            }
            "view.macro.edit" => {
                let state = macro_state(registry, session)?;
                self.editor_view.release_focus(ctx);
                self.macro_dialogs.open_edit(&state);
                Ok(Value::Null)
            }
            _ => {
                let state = macro_state(registry, session)?;
                self.editor_view.release_focus(ctx);
                self.macro_dialogs.open_play(&state);
                Ok(Value::Null)
            }
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
    pub(super) fn macro_title(&self, action: &str, session: &Session) -> Option<String> {
        (action == "view.macro.toggleRecording" && recording(session)).then(|| t!("menu-edit-macro-stop"))
    }

    pub(super) fn macro_dialogs(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        if let Some(form) = &mut self.macro_dialogs.name {
            match name_dialog(ui, &p, form) {
                Some(NameAnswer::Keep(name)) => {
                    match registry.dispatch(session, "editor.macro.stopRecording", json!({ "name": name })) {
                        Ok(_) => {
                            self.macro_dialogs.name = None;
                            self.editor_view.focus();
                        }
                        Err(zen_core::DispatchError::Failed { error, .. }) => form.error = Some(error.message),
                        Err(_) => self.macro_dialogs.name = None,
                    }
                }
                Some(NameAnswer::Cancel) => {
                    let _ = registry.dispatch(session, "editor.macro.stopRecording", json!({}));
                    self.macro_dialogs.name = None;
                    self.editor_view.focus();
                }
                None => {}
            }
            if !recording(session) && self.macro_dialogs.name.as_ref().is_some_and(|form| form.error.is_none()) {
                self.macro_dialogs.name = None;
            }
        }
        if let Some(edit) = &mut self.macro_dialogs.edit
            && let Some(answer) = edit_dialog(ui, &p, registry, edit)
        {
            let changes = std::mem::take(&mut edit.changes);
            self.macro_dialogs.edit = None;
            if answer == EditAnswer::Ok {
                self.apply_macro_changes(ui.ctx(), registry, session, changes);
            }
        }
        if let Some(play) = &mut self.macro_dialogs.play {
            match play_dialog(ui, &p, play) {
                Some(Some(name)) => {
                    self.macro_dialogs.play = None;
                    self.editor_view.focus();
                    if let Err(error) = registry.dispatch(session, "editor.macro.play", json!({ "name": name })) {
                        self.report(ui, error);
                    }
                }
                Some(None) => self.macro_dialogs.play = None,
                None => {}
            }
        }
    }

    fn apply_macro_changes(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        changes: Vec<Change>,
    ) {
        for change in changes {
            let (command, params) = match change {
                Change::Rename { name, new_name } => {
                    ("editor.macro.rename", json!({ "name": name, "newName": new_name }))
                }
                Change::Remove(name) => ("editor.macro.remove", json!({ "name": name })),
                Change::RemoveStep { name, index } => {
                    ("editor.macro.removeStep", json!({ "name": name, "index": index }))
                }
            };
            if let Err(error) = registry.dispatch(session, command, params) {
                self.report_at(ctx, error);
                return;
            }
        }
    }

    pub(super) fn macro_status(&mut self, ui: &mut Ui, session: &Session, right: f32, y: f32) -> f32 {
        if !recording(session) {
            return right;
        }
        let p = self.palette;
        let stop = Rect::from_center_size(pos2(right - 11.0, y), Vec2::splat(22.0));
        let response = ui.interact(stop, ui.scope_id().with("status-macro-stop"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(stop, 4, p.hover);
        }
        ui.painter().rect_filled(Rect::from_center_size(stop.center(), Vec2::splat(9.0)), 2, p.text_secondary);
        let name = t!("menu-edit-macro-stop");
        super::hint(ui, &response, &name, None, super::Side::Left);
        if response.clicked() {
            self.editor_view.release_focus(ui.ctx());
            self.macro_dialogs.open_name();
        }
        let steps = session.state::<Recorder>().map_or(0, |recorder| recorder.steps().len());
        let label = t!("ide-macro-recording");
        let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(12.5), p.text_secondary);
        let text = Align2::RIGHT_CENTER.anchor_size(pos2(stop.left() - 4.0, y), galley.size());
        ui.painter().galley(text.min, galley, p.text_secondary);
        let dot = pos2(text.left() - 9.0, y);
        ui.painter().circle_filled(dot, 4.0, p.diagnostic_error);
        let mark = Rect::from_min_max(pos2(dot.x - 5.0, text.top()), text.max);
        accessibility::label(ui, ui.scope_id().with("status-macro"), mark, || {
            t!("ide-macro-recording-steps", steps = steps)
        });
        mark.left() - 12.0
    }
}

fn text_field(ui: &mut Ui, p: &IdePalette, form: &mut NameForm, id: &str) -> egui::Response {
    let output = egui::Frame::new()
        .fill(p.text_field)
        .stroke(Stroke::new(1.5, if form.error.is_some() { p.excluded } else { p.accent }))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            if !form.focused {
                let field = ui.make_persistent_id(id);
                ui.memory_mut(|memory| memory.request_focus(field));
            }
            egui::TextEdit::singleline(&mut form.text)
                .id_salt(id)
                .frame(egui::Frame::NONE)
                .font(theme::regular(14.0))
                .desired_width(f32::INFINITY)
                .show(ui)
        })
        .inner;
    if !form.focused {
        output.response.request_focus();
        form.focused = true;
    }
    if output.response.changed() {
        form.error = None;
    }
    output.response.response
}

fn buttons(ui: &mut Ui, p: &IdePalette, ok_enabled: bool) -> (bool, bool) {
    let mut answer = (false, false);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let ok =
            egui::Button::new(egui::RichText::new(t!("ide-macro-ok")).font(theme::regular(13.0)).color(p.accent_text))
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(if ok_enabled { p.accent } else { p.accent.gamma_multiply(0.4) });
        answer.0 = ui.add_enabled(ok_enabled, ok).clicked();
        let cancel =
            egui::Button::new(egui::RichText::new(t!("ide-macro-cancel")).font(theme::regular(13.0)).color(p.text))
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
        answer.1 = ui.add(cancel).clicked();
    });
    answer
}

fn title(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.label(egui::RichText::new(text).font(theme::semibold(15.0)).color(p.text));
}

fn early_text(ui: &Ui, form: &mut NameForm) {
    if form.focused {
        return;
    }
    let mut typed = String::new();
    ui.input_mut(|input| {
        input.events.retain(|event| match event {
            egui::Event::Text(text) => {
                typed.push_str(text);
                false
            }
            _ => true,
        });
    });
    form.text.push_str(&typed);
}

fn name_dialog(ui: &Ui, p: &IdePalette, form: &mut NameForm) -> Option<NameAnswer> {
    early_text(ui, form);
    let mut answer = None;
    let modal = egui::Modal::new(ui.scope_id().with("macro-name")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        title(ui, p, t!("ide-macro-name-title"));
        ui.add_space(14.0);
        ui.label(egui::RichText::new(t!("ide-macro-name-label")).font(theme::regular(12.5)).color(p.text_secondary));
        ui.add_space(6.0);
        let field = text_field(ui, p, form, "macro-name-text");
        accessibility::labelled(&field, &t!("ide-macro-name-label"));
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            answer = Some(NameAnswer::Keep(form.text.trim().to_owned()));
        }
        ui.add_space(6.0);
        let note = form.error.clone().unwrap_or_else(|| t!("ide-macro-name-note"));
        let color = if form.error.is_some() { p.excluded } else { p.text_secondary };
        ui.add(egui::Label::new(egui::RichText::new(note).font(theme::regular(12.0)).color(color)).wrap());
        ui.add_space(20.0);
        let (ok, cancel) = buttons(ui, p, true);
        if ok {
            answer = Some(NameAnswer::Keep(form.text.trim().to_owned()));
        } else if cancel {
            answer = Some(NameAnswer::Cancel);
        }
    });
    if answer.is_none() && modal.should_close() {
        answer = Some(NameAnswer::Cancel);
    }
    answer
}

fn list_frame(ui: &mut Ui, p: &IdePalette, width: f32, id: &str, rows: impl FnOnce(&mut Ui)) {
    egui::Frame::new().stroke(Stroke::new(1.0, p.popup_border)).corner_radius(6).inner_margin(4).show(ui, |ui| {
        ui.set_width(width);
        ui.set_height(LIST_HEIGHT);
        egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, rows);
    });
}

fn row(ui: &mut Ui, p: &IdePalette, text: &str, selected: bool, color: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 4, p.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.popup_hover);
    }
    let shown = ui.painter().layout_no_wrap(text.to_owned(), theme::regular(13.0), color);
    let clip = ui.painter().with_clip_rect(rect.shrink2(vec2(6.0, 0.0)));
    clip.galley(pos2(rect.left() + 8.0, rect.center().y - shown.size().y / 2.0), shown, color);
    accessibility::item(&response, egui::Role::ListBoxOption, selected, text);
    response
}

fn small_button(ui: &mut Ui, p: &IdePalette, text: String, enabled: bool) -> bool {
    let button = egui::Button::new(egui::RichText::new(text).font(theme::regular(12.5)).color(p.text))
        .min_size(vec2(0.0, 26.0))
        .corner_radius(5)
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, p.button_border));
    ui.add_enabled(enabled, button).clicked()
}

fn edit_dialog(ui: &Ui, p: &IdePalette, registry: &Registry, edit: &mut EditMacros) -> Option<EditAnswer> {
    let mut answer = None;
    let modal = egui::Modal::new(ui.scope_id().with("edit-macros")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        title(ui, p, action_title("view.macro.edit"));
        ui.add_space(14.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(egui::RichText::new(t!("ide-macro-macros")).font(theme::semibold(12.5)).color(p.text));
                list_frame(ui, p, LIST_WIDTH, "edit-macros-list", |ui| {
                    if edit.macros.is_empty() {
                        ui.label(egui::RichText::new(t!("ide-macro-none")).color(p.text_secondary));
                    }
                    for index in 0..edit.macros.len() {
                        let selected = edit.selected == Some(index);
                        if row(ui, p, &edit.macros[index].name.clone(), selected, p.text).clicked() {
                            edit.selected = Some(index);
                            edit.step = None;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let chosen = edit.selected.is_some();
                    if small_button(ui, p, t!("ide-macro-rename"), chosen) {
                        let text = edit.selected_macro().map(|saved| saved.name.clone()).unwrap_or_default();
                        edit.rename = Some(NameForm { text, ..NameForm::default() });
                    }
                    if small_button(ui, p, t!("ide-macro-remove"), chosen) {
                        edit.remove_selected();
                    }
                });
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(egui::RichText::new(t!("ide-macro-actions")).font(theme::semibold(12.5)).color(p.text));
                let steps = edit.selected_macro().map(|saved| saved.steps.clone()).unwrap_or_default();
                list_frame(ui, p, STEPS_WIDTH, "edit-macros-steps", |ui| {
                    for (index, step) in steps.iter().enumerate() {
                        let selected = edit.step == Some(index);
                        if row(ui, p, &step_title(step, registry), selected, p.text).clicked() {
                            edit.step = Some(index);
                        }
                    }
                });
                if small_button(ui, p, t!("ide-macro-remove"), edit.step.is_some()) {
                    edit.remove_step();
                }
            });
        });
        ui.add_space(20.0);
        let (ok, cancel) = buttons(ui, p, true);
        if ok {
            answer = Some(EditAnswer::Ok);
        } else if cancel {
            answer = Some(EditAnswer::Cancel);
        }
    });
    if let Some(form) = &mut edit.rename {
        let mut done = None;
        egui::Modal::new(ui.scope_id().with("rename-macro")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            title(ui, p, t!("ide-macro-rename-title"));
            ui.add_space(12.0);
            let field = text_field(ui, p, form, "rename-macro-text");
            accessibility::labelled(&field, &t!("ide-macro-rename-title"));
            let entered = !form.text.trim().is_empty()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if let Some(error) = &form.error {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(error).font(theme::regular(12.0)).color(p.excluded));
            }
            ui.add_space(16.0);
            let (ok, cancel) = buttons(ui, p, !form.text.trim().is_empty());
            if ok || entered {
                done = Some(Some(form.text.clone()));
            } else if cancel {
                done = Some(None);
            }
        });
        match done {
            Some(Some(name)) => match edit.rename_to(&name) {
                Ok(()) => edit.rename = None,
                Err(error) => {
                    if let Some(form) = &mut edit.rename {
                        form.error = Some(error);
                    }
                }
            },
            Some(None) => edit.rename = None,
            None => {}
        }
        return None;
    }
    if answer.is_none() && modal.should_close() {
        answer = Some(EditAnswer::Cancel);
    }
    answer
}

fn play_dialog(ui: &Ui, p: &IdePalette, play: &mut PlaySaved) -> Option<Option<String>> {
    let mut answer = None;
    let count = play.names.len();
    let (up, down, enter) = ui.input_mut(|input| {
        let none = egui::Modifiers::NONE;
        (
            input.consume_key(none, egui::Key::ArrowUp),
            input.consume_key(none, egui::Key::ArrowDown),
            input.consume_key(none, egui::Key::Enter),
        )
    });
    if count > 0 {
        if up {
            play.selected = play.selected.checked_sub(1).unwrap_or(count - 1);
        }
        if down {
            play.selected = (play.selected + 1) % count;
        }
        if enter {
            answer = Some(play.names.get(play.selected).cloned());
        }
    }
    let modal = egui::Modal::new(ui.scope_id().with("play-saved-macros")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(300.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        title(ui, p, t!("ide-macro-play-title"));
        ui.add_space(10.0);
        if play.names.is_empty() {
            ui.label(egui::RichText::new(t!("ide-macro-none")).color(p.text_secondary));
        }
        for index in 0..count {
            let name = play.names[index].clone();
            let response = row(ui, p, &name, play.selected == index, p.text);
            if response.clicked() {
                answer = Some(Some(name));
            }
        }
    });
    if answer.is_none() && modal.should_close() {
        answer = Some(None);
    }
    answer
}

#[cfg(test)]
impl MacroDialogs {
    pub(super) fn edit_select(&mut self, index: usize, step: Option<usize>) {
        if let Some(edit) = &mut self.edit {
            edit.selected = Some(index);
            edit.step = step;
        }
    }

    pub(super) fn edit_rename(&mut self, name: &str) -> Result<(), String> {
        self.edit.as_mut().map_or(Ok(()), |edit| edit.rename_to(name))
    }

    pub(super) fn edit_remove(&mut self) {
        if let Some(edit) = &mut self.edit {
            edit.remove_selected();
        }
    }

    pub(super) fn edit_remove_step(&mut self) {
        if let Some(edit) = &mut self.edit {
            edit.remove_step();
        }
    }

    pub(super) fn edit_names(&self) -> Vec<String> {
        self.edit.as_ref().map(|edit| edit.macros.iter().map(|saved| saved.name.clone()).collect()).unwrap_or_default()
    }

    pub(super) fn edit_open_rename(&mut self) {
        if let Some(edit) = &mut self.edit {
            let text = edit.selected_macro().map(|saved| saved.name.clone()).unwrap_or_default();
            edit.rename = Some(NameForm { text, ..NameForm::default() });
        }
    }
}

#[cfg(test)]
impl IdeView {
    pub(super) fn macro_edit_ok(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        if let Some(edit) = self.macro_dialogs.edit.take() {
            self.apply_macro_changes(ctx, registry, session, edit.changes);
        }
    }
}
