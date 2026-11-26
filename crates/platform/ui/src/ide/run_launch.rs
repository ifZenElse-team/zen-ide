use egui::{Color32, Key, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_run::macros::Macro;
use zen_workbench::plugin_runs::{PERSON_CONFIRMATION, PersonQuestion};
use zen_workbench::run::{PROMPT_NEEDED, RERUN_CONFIRMATION, Runs};

use super::agent::ActionError;
use super::external_tools::ToolPrompt;
use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

#[derive(Debug)]
pub(super) struct RerunQuestion {
    params: Value,
    question: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Rerun,
    Cancel,
}

fn code(error: &DispatchError) -> Option<&str> {
    match error {
        DispatchError::Failed { error, .. } => error.code.as_deref(),
        _ => None,
    }
}

fn message(error: &DispatchError) -> String {
    match error {
        DispatchError::Failed { error, .. } => error.message.clone(),
        other => other.to_string(),
    }
}

pub(super) fn with_confirmation(mut params: Value, confirm: bool) -> Value {
    if !params.is_object() {
        params = json!({});
    }
    if params.get("confirmRerun").is_none() {
        params["confirmRerun"] = json!(confirm);
    }
    params
}

pub(super) fn tool_window(session: &Session, run_id: u64) -> (bool, bool) {
    session
        .state::<Runs>()
        .and_then(|runs| runs.run(run_id))
        .map(|run| (run.configuration.common.activate_tool_window, run.configuration.common.focus_tool_window))
        .unwrap_or((true, false))
}

fn show(ui: &mut Ui, p: &IdePalette, question: &str) -> Option<Choice> {
    let mut choice = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("rerun-confirmation")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(400.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(egui::Label::new(egui::RichText::new(question).font(theme::semibold(15.0)).color(p.text)).wrap());
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let rerun = egui::Button::new(
                    egui::RichText::new(t!("ide-run-stop-and-rerun")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add(rerun).clicked() {
                    choice = Some(Choice::Rerun);
                }
                let cancel = egui::Button::new(
                    egui::RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(cancel).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                choice = Some(Choice::Rerun);
            }
        });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

pub(super) fn refusal_while_person_decides(session: &Session) -> Option<String> {
    let runs = session.state::<Runs>()?;
    if runs.person_question().is_some() {
        return Some(t!("ide-run-person-keys-question"));
    }
    runs.person_only_running().then(|| t!("ide-run-person-keys-running"))
}

fn show_person(ui: &mut Ui, p: &IdePalette, question: &PersonQuestion) -> Option<Choice> {
    let mut choice = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("person-confirmation")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let title = t!("ide-run-person-title", name = question.name.as_str());
            ui.add(egui::Label::new(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text)).wrap());
            ui.add_space(8.0);
            let why = t!("ide-run-person-why", plugin = question.plugin.as_str());
            ui.add(egui::Label::new(egui::RichText::new(why).font(theme::regular(13.0)).color(p.text)).wrap());
            ui.add_space(12.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&question.command_line).font(egui::FontId::monospace(13.0)).color(p.text),
                )
                .wrap()
                .selectable(true),
            );
            ui.add_space(8.0);
            let folder = t!("ide-run-person-folder", folder = question.folder.display().to_string());
            ui.add(
                egui::Label::new(egui::RichText::new(folder).font(theme::regular(12.0)).color(p.text_secondary)).wrap(),
            );
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let run = egui::Button::new(
                    egui::RichText::new(t!("ide-run-person-run")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add(run).clicked() {
                    choice = Some(Choice::Rerun);
                }
                let cancel = egui::Button::new(
                    egui::RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(cancel).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

impl IdeView {
    pub(super) fn person_question(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(question) = session.state::<Runs>().and_then(Runs::person_question).cloned() else { return };
        match show_person(ui, &p, &question) {
            Some(Choice::Cancel) => session.state_mut::<Runs>().dismiss_person_question(),
            Some(Choice::Rerun) => {
                let params = json!({ "configuration": question.configuration, "confirmation": question.token });
                if let Err(ActionError::Dispatch(error)) = self.start_run(registry, session, params, false) {
                    session.state_mut::<Runs>().dismiss_person_question();
                    self.report(ui, error);
                }
            }
            None => {}
        }
    }

    pub(super) fn start_run(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        params: Value,
        agent: bool,
    ) -> Result<Value, ActionError> {
        let params = if agent { params } else { with_confirmation(params, self.overview.settings.run_confirm_rerun) };
        match registry.dispatch(session, "run.process.start", params.clone()) {
            Ok(started) => {
                if let Some(run_id) = started.get("runId").and_then(Value::as_u64) {
                    self.front_run(session, run_id);
                }
                Ok(started)
            }
            Err(error) if !agent && code(&error) == Some(RERUN_CONFIRMATION) => {
                let mut params = params;
                params["confirmRerun"] = json!(false);
                self.rerun_confirmation = Some(RerunQuestion { params, question: message(&error) });
                Ok(json!({ "asking": "rerunConfirmation" }))
            }
            Err(error) if !agent && code(&error) == Some(PERSON_CONFIRMATION) => {
                Ok(json!({ "asking": "personConfirmation" }))
            }
            Err(error) if !agent && code(&error) == Some(PROMPT_NEEDED) => {
                let text = message(&error);
                let (prompt, file) = (text.contains(&Macro::Prompt.token()), text.contains(&Macro::FilePrompt.token()));
                let title = t!("ide-run-prompt-title");
                self.external_tool_prompt = Some(ToolPrompt::new("run.process.start", params, title, prompt, file));
                Ok(json!({ "asking": "externalToolPrompt" }))
            }
            Err(error) => Err(ActionError::Dispatch(error)),
        }
    }

    pub(super) fn front_run(&mut self, session: &Session, run_id: u64) {
        let (activate, focus) = tool_window(session, run_id);
        if activate || focus {
            self.show_tool("run");
        }
        if focus {
            self.tool_window_took_keyboard();
        }
    }

    pub(super) fn rerun_confirmation(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(question) = &self.rerun_confirmation else { return };
        match show(ui, &p, &question.question) {
            Some(Choice::Cancel) => self.rerun_confirmation = None,
            Some(Choice::Rerun) => {
                let params = question.params.clone();
                self.rerun_confirmation = None;
                if let Err(ActionError::Dispatch(error)) = self.start_run(registry, session, params, false) {
                    self.report(ui, error);
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_asks_before_a_rerun_only_while_the_setting_is_on_and_never_overrides_the_caller() {
        assert_eq!(with_confirmation(json!({ "configurationId": "a" }), true)["confirmRerun"], true);
        assert_eq!(with_confirmation(json!(null), false), json!({ "confirmRerun": false }));
        assert_eq!(with_confirmation(json!({ "confirmRerun": false }), true)["confirmRerun"], false);
    }
}
