use egui::{RichText, Ui};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::IdePalette;
use super::run_window::{call, message};
use crate::theme;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Coroutine {
    pub state: String,
    pub name: Option<String>,
    pub dispatcher: Option<String>,
    pub job: String,
    pub frames: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Answer {
    pub coroutines: Vec<Coroutine>,
    pub more: bool,
    pub message: Option<String>,
    pub pending: bool,
}

#[derive(Debug, Default)]
pub(super) struct CoroutinesView {
    answer: Option<Answer>,
    error: Option<String>,
    read: Option<u64>,
    pending: bool,
}

fn state_label(state: &str) -> String {
    match state {
        "RUNNING" => t!("ide-debug-coroutine-running"),
        "SUSPENDED" => t!("ide-debug-coroutine-suspended"),
        "CREATED" => t!("ide-debug-coroutine-created"),
        other => other.to_owned(),
    }
}

impl CoroutinesView {
    #[cfg(test)]
    pub(super) fn answer(&self) -> Option<&Answer> {
        self.answer.as_ref()
    }

    pub(super) fn poll(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        debug: &Value,
        stop: Option<u64>,
    ) -> bool {
        let Some(stop) = stop else {
            *self = Self::default();
            return false;
        };
        if self.read == Some(stop) && !self.pending {
            return false;
        }
        match call(registry, session, "debug.coroutines.list", json!({ "sessionId": debug, "waitMs": 0 })) {
            Ok(value) => {
                let answer = serde_json::from_value::<Answer>(value).unwrap_or_default();
                self.pending = answer.pending;
                if !answer.pending {
                    self.answer = Some(answer);
                    self.error = None;
                }
            }
            Err(error) => {
                self.pending = false;
                self.answer = None;
                self.error = Some(error);
            }
        }
        self.read = Some(stop);
        self.pending
    }

    pub(super) fn show(&self, ui: &mut Ui, p: &IdePalette, stopped: bool, salt: &str) {
        if !stopped {
            return message(ui, p, t!("ide-debug-coroutines-running"));
        }
        if let Some(error) = &self.error {
            return message(ui, p, error.clone());
        }
        let Some(answer) = self.answer.as_ref().filter(|_| !self.pending) else {
            return message(ui, p, t!("ide-debug-coroutines-reading"));
        };
        if let Some(text) = &answer.message {
            return message(ui, p, text.clone());
        }
        if answer.coroutines.is_empty() {
            return message(ui, p, t!("ide-debug-coroutines-empty"));
        }
        egui::ScrollArea::vertical().id_salt(("debug-coroutines", salt)).auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            for coroutine in &answer.coroutines {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let title = coroutine.name.clone().unwrap_or_else(|| coroutine.job.clone());
                    ui.label(RichText::new(title).font(theme::regular(13.0)).color(p.text));
                    ui.label(RichText::new(state_label(&coroutine.state)).color(p.text_secondary));
                    if let Some(dispatcher) = &coroutine.dispatcher {
                        ui.label(RichText::new(dispatcher).color(p.text_secondary));
                    }
                });
                for frame in &coroutine.frames {
                    ui.horizontal(|ui| {
                        ui.add_space(34.0);
                        ui.label(RichText::new(frame).font(theme::regular(12.0)).color(p.text_secondary));
                    });
                }
            }
            if answer.more {
                let count = zen_workbench::java::coroutines::COROUTINE_LIMIT;
                message(ui, p, t!("ide-debug-coroutines-more", count = count));
            }
        });
    }
}
