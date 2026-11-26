use egui::{Ui, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_syntax::{ToolInput, ToolInputKind, ToolRow};
use zen_workbench::plugin_tools::inputs;

use super::IdePalette;
use crate::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InputInfo {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) kind: ToolInputKind,
    pub(super) min: u32,
    pub(super) max: u32,
    pub(super) default: Option<u32>,
    pub(super) default_from: String,
}

pub(super) fn inputs_of(action: &Value) -> Vec<InputInfo> {
    action["inputs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|input| InputInfo {
            id: input["id"].as_str().unwrap_or_default().to_owned(),
            title: input["title"].as_str().unwrap_or_default().to_owned(),
            kind: match input["kind"].as_str() {
                Some("port") => ToolInputKind::Port,
                Some("forward") => ToolInputKind::Forward,
                _ => ToolInputKind::Count,
            },
            min: input["min"].as_u64().and_then(|value| u32::try_from(value).ok()).unwrap_or_default(),
            max: input["max"].as_u64().and_then(|value| u32::try_from(value).ok()).unwrap_or_default(),
            default: input["default"].as_u64().and_then(|value| u32::try_from(value).ok()),
            default_from: input["defaultFrom"].as_str().unwrap_or_default().to_owned(),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Field {
    pub(super) input: InputInfo,
    pub(super) text: String,
    pub(super) remote: String,
}

impl Field {
    pub(super) fn starting(input: &InputInfo, columns: &[String], row: Option<&ToolRow>) -> Self {
        let declared = ToolInput {
            id: input.id.clone(),
            kind: input.kind,
            default: input.default,
            default_from: input.default_from.clone(),
            ..ToolInput::default()
        };
        let start = inputs::starting_value(&declared, columns, row).unwrap_or_default();
        let (text, remote) = match input.kind {
            ToolInputKind::Forward => {
                let (local, remote) = start.split_once(':').unwrap_or_default();
                (local.to_owned(), remote.to_owned())
            }
            _ => (start, String::new()),
        };
        Self { input: input.clone(), text, remote }
    }

    pub(super) fn raw(&self) -> String {
        match self.input.kind {
            ToolInputKind::Forward => format!("{}:{}", self.text.trim(), self.remote.trim()),
            _ => self.text.trim().to_owned(),
        }
    }

    pub(super) fn value(&self) -> Option<String> {
        inputs::checked(self.input.kind, &self.raw())
    }

    fn problem(&self) -> Option<String> {
        if self.value().is_some() {
            return None;
        }
        Some(match self.input.kind {
            ToolInputKind::Forward => t!("ide-plugin-tool-input-forward"),
            _ => t!("ide-plugin-tool-input-range", min = self.input.min, max = self.input.max),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InputDialog {
    pub(super) tool: &'static str,
    pub(super) action: String,
    pub(super) rows: Vec<String>,
    pub(super) title: String,
    pub(super) fields: Vec<Field>,
    pub(super) token: Option<String>,
    pub(super) question: Option<String>,
}

impl InputDialog {
    pub(super) fn values(&self) -> Option<Value> {
        let mut values = serde_json::Map::new();
        for field in &self.fields {
            values.insert(field.input.id.clone(), json!(field.value()?));
        }
        Some(Value::Object(values))
    }

    pub(super) fn question(&self) -> Option<String> {
        let question = self.question.clone()?;
        Some(self.fields.iter().fold(question, |text, field| {
            let shown = field.value().unwrap_or_else(|| "…".to_owned());
            text.replace(&format!("${{input.{}}}", field.input.id), &shown)
        }))
    }
}

pub(super) fn input_dialog(ui: &mut Ui, p: &IdePalette, dialog: &mut InputDialog) -> Option<bool> {
    let mut choice = None;
    let valid = dialog.values().is_some();
    let modal =
        egui::Modal::new(ui.scope_id().with("plugin-tool-inputs")).frame(super::dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(400.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(&dialog.title).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(12.0);
            for (index, field) in dialog.fields.iter_mut().enumerate() {
                ui.label(egui::RichText::new(&field.input.title).font(theme::regular(13.0)).color(p.text));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let first = ui.add(egui::TextEdit::singleline(&mut field.text).desired_width(90.0).id_salt((
                        "plugin-tool-input",
                        index,
                        "local",
                    )));
                    if index == 0 && !first.has_focus() && ui.memory(|memory| memory.focused().is_none()) {
                        first.request_focus();
                    }
                    if field.input.kind == ToolInputKind::Forward {
                        ui.label(egui::RichText::new(":").color(p.text_secondary));
                        ui.add(egui::TextEdit::singleline(&mut field.remote).desired_width(90.0).id_salt((
                            "plugin-tool-input",
                            index,
                            "remote",
                        )));
                        ui.label(
                            egui::RichText::new(t!("ide-plugin-tool-input-forward-hint"))
                                .font(theme::regular(12.0))
                                .color(p.text_secondary),
                        );
                    }
                });
                if let Some(problem) = field.problem().filter(|_| !field.raw().trim_matches(':').is_empty()) {
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(problem).font(theme::regular(12.0)).color(p.error_text));
                }
                ui.add_space(10.0);
            }
            if let Some(question) = dialog.question() {
                ui.add(egui::Label::new(egui::RichText::new(question).font(theme::regular(13.5)).color(p.text)).wrap());
                ui.add_space(10.0);
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let go = egui::Button::new(
                    egui::RichText::new(dialog.title.trim_end_matches('…'))
                        .font(theme::regular(13.0))
                        .color(p.accent_text),
                )
                .min_size(vec2(96.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add_enabled(valid, go).clicked() {
                    choice = Some(true);
                }
                if super::vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    choice = Some(false);
                }
            });
            if valid && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                choice = Some(true);
            }
        });
    choice.or_else(|| modal.should_close().then_some(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count() -> InputInfo {
        InputInfo {
            id: "replicas".into(),
            title: "Replicas".into(),
            kind: ToolInputKind::Count,
            min: 0,
            max: 1000,
            default: None,
            default_from: "ready".into(),
        }
    }

    #[test]
    fn the_inputs_are_read_from_the_list_and_start_from_the_row() {
        let action = json!({ "inputs": [{ "id": "ports", "title": "Ports", "kind": "forward", "min": 1024, "max": 65535, "defaultFrom": "ports" }] });
        let read = inputs_of(&action);
        assert_eq!(read[0].kind, ToolInputKind::Forward);
        let row = ToolRow { id: "web".into(), columns: vec!["web".into(), "80/TCP".into()], state: None };
        let field = Field::starting(&read[0], &["name".to_owned(), "ports".to_owned()], Some(&row));
        assert_eq!((field.text.as_str(), field.remote.as_str()), ("", "80"));
        assert_eq!(field.value(), None, "the local port is the person's to type");
    }

    #[test]
    fn the_dialog_is_valid_only_with_every_value_in_range_and_shows_them_in_the_question() {
        let mut dialog = InputDialog {
            tool: "kubernetes",
            action: "scale".into(),
            rows: vec!["web".into()],
            title: "Scale…".into(),
            fields: vec![Field { input: count(), text: "1001".into(), remote: String::new() }],
            token: None,
            question: Some("Scale web in prod to ${input.replicas}?".into()),
        };
        assert!(dialog.values().is_none());
        assert_eq!(dialog.question().as_deref(), Some("Scale web in prod to …?"));
        dialog.fields[0].text = " 3 ".into();
        assert_eq!(dialog.values(), Some(json!({ "replicas": "3" })));
        assert_eq!(dialog.question().as_deref(), Some("Scale web in prod to 3?"));
    }
}
