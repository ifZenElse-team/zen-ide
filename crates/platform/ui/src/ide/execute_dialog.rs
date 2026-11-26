use egui::{Align, Key, Layout, Modifiers, RichText, Ui};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;

use super::database_navigator::type_color;
use super::{IdePalette, dialog_frame, theme, vcs};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ExecuteQuestion {
    message: String,
    token: String,
    target: Value,
    statements: Vec<Value>,
    total: usize,
    command: String,
    params: Value,
    current: usize,
    skip: Vec<u64>,
    skippable: bool,
    understood: bool,
    error: Option<String>,
}

impl ExecuteQuestion {
    pub(super) fn same(&self, other: &ExecuteQuestion) -> bool {
        self.token == other.token && self.command == other.command
    }
}

pub(super) fn question(error: &DispatchError) -> Option<ExecuteQuestion> {
    let DispatchError::Failed { error, .. } = error else { return None };
    if error.code.as_deref() != Some(zen_workbench::database::safety::CONFIRM_EXECUTE) {
        return None;
    }
    let data = error.data.as_ref()?;
    let retry = data.get("retry")?;
    let statements = data.get("statements").and_then(Value::as_array).cloned().unwrap_or_default();
    if statements.is_empty() {
        return None;
    }
    Some(ExecuteQuestion {
        message: error.message.clone(),
        token: data.get("token").and_then(Value::as_str).unwrap_or_default().to_owned(),
        target: data.get("target").cloned().unwrap_or_default(),
        total: data.get("total").and_then(Value::as_u64).map_or(statements.len(), |total| total as usize),
        statements,
        command: retry.get("command")?.as_str()?.to_owned(),
        params: retry.get("params").cloned().unwrap_or_else(|| json!({})),
        current: 0,
        skip: Vec::new(),
        skippable: data.get("skippable").and_then(Value::as_bool).unwrap_or(true),
        understood: false,
        error: None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    Yes,
    Skip,
    All,
    Cancel,
}

fn dangerous(statement: &Value) -> bool {
    statement["dangers"].as_array().is_some_and(|dangers| !dangers.is_empty())
}

fn effect_text(statement: &Value) -> String {
    match statement["effect"].as_str() {
        Some("data") => t!("ide-dbsafety-effect-data"),
        Some("structure") => t!("ide-dbsafety-effect-structure"),
        _ => t!("ide-dbsafety-effect-other"),
    }
}

fn label(ui: &mut Ui, text: String, color: egui::Color32) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::regular(13.0)).color(color)).wrap());
}

pub(super) fn danger_button(ui: &mut Ui, p: &IdePalette, text: &str, enabled: bool) -> egui::Response {
    let button = egui::Button::new(RichText::new(text).font(theme::regular(13.0)).color(p.accent_text))
        .min_size(egui::vec2(84.0, 30.0))
        .corner_radius(6)
        .fill(p.error_text);
    ui.add_enabled(enabled, button)
}

fn connection_line(ui: &mut Ui, p: &IdePalette, target: &Value) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let color = target["color"].as_str().and_then(|color| type_color(p, color)).unwrap_or(p.text_secondary);
        let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 4.0, color);
        let name = target["name"].as_str().unwrap_or_default();
        ui.label(RichText::new(name).font(theme::semibold(13.0)).color(p.text));
        let kind = target["typeName"].as_str().unwrap_or_default();
        ui.label(RichText::new(kind).font(theme::semibold(13.0)).color(color));
    });
}

fn draw(ui: &mut Ui, p: &IdePalette, question: &mut ExecuteQuestion, fresh: bool) -> Option<Answer> {
    let mut answer = None;
    let statement = question.statements.get(question.current).cloned().unwrap_or_default();
    let risky = dangerous(&statement);
    let later_risky = question.statements.iter().skip(question.current + 1).any(dangerous);
    let modal =
        egui::Modal::new(ui.scope_id().with("execute-confirmation")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.input_mut(|input| {
                for modifiers in
                    [Modifiers::NONE, Modifiers::COMMAND, Modifiers::CTRL, Modifiers::SHIFT, Modifiers::ALT]
                {
                    while input.consume_key(modifiers, Key::Enter) {}
                }
            });
            ui.set_width(560.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let heading = if risky { t!("ide-dbsafety-title-danger") } else { t!("ide-dbsafety-title") };
            ui.add(egui::Label::new(RichText::new(heading).font(theme::semibold(15.0)).color(p.text)).wrap());
            ui.add_space(10.0);
            connection_line(ui, p, &question.target);
            if question.current == 0 {
                ui.add_space(8.0);
                label(ui, question.message.clone(), p.text_secondary);
            }
            ui.add_space(10.0);
            if question.statements.len() > 1 {
                let progress =
                    t!("ide-dbsafety-progress", current = question.current + 1, count = question.statements.len());
                label(ui, progress, p.text_secondary);
                ui.add_space(6.0);
            }
            egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().max_height(180.0).auto_shrink([false, true]).show(ui, |ui| {
                    let text = statement["text"].as_str().unwrap_or_default();
                    ui.label(RichText::new(text).font(egui::FontId::monospace(12.5)).color(p.text));
                });
            });
            let parameters: Vec<&str> =
                statement["parameters"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !parameters.is_empty() {
                ui.add_space(6.0);
                label(ui, t!("ide-dbsafety-parameters", values = parameters.join(", ")), p.text_secondary);
            }
            ui.add_space(8.0);
            let risks: Vec<String> = statement["risks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            if risks.is_empty() {
                label(ui, effect_text(&statement), p.warning_text);
            }
            for risk in &risks {
                label(ui, risk.clone(), p.error_text);
                ui.add_space(3.0);
            }
            if risky {
                ui.add_space(8.0);
                let understood = t!("ide-dbsafety-understood", risk = risks.first().map_or("", String::as_str));
                super::form::check(ui, p, &mut question.understood, &understood);
            }
            if let Some(error) = &question.error {
                ui.add_space(8.0);
                label(ui, error.clone(), p.error_text);
            }
            ui.add_space(18.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    answer = Some(Answer::Cancel);
                }
                if !risky
                    && !later_risky
                    && question.statements.len() > question.current + 1
                    && vcs::secondary_button(ui, p, &t!("ide-dbsafety-yes-all")).clicked()
                {
                    answer = Some(Answer::All);
                }
                if question.skippable
                    && question.total > 1
                    && vcs::secondary_button(ui, p, &t!("ide-dbsafety-skip")).clicked()
                {
                    answer = Some(Answer::Skip);
                }
                let yes = if risky {
                    danger_button(ui, p, &t!("ide-dbsafety-run-anyway"), question.understood)
                } else {
                    vcs::primary_button(ui, p, &t!("ide-dbsafety-yes"))
                };
                if yes.clicked() {
                    answer = Some(Answer::Yes);
                }
            });
            if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
                answer = Some(Answer::Cancel);
            }
        });
    answer.or_else(|| (!fresh && modal.should_close()).then_some(Answer::Cancel))
}

fn run(registry: &Registry, session: &mut Session, mut question: ExecuteQuestion) -> Option<ExecuteQuestion> {
    if question.skip.len() >= question.total {
        return None;
    }
    let mut params = question.params.clone();
    if let Some(object) = params.as_object_mut() {
        object.insert("skip".to_owned(), json!(question.skip));
    }
    match registry.dispatch(session, &question.command, params) {
        Ok(_) => None,
        Err(error) => match self::question(&error) {
            Some(next) => Some(next),
            None => {
                question.error = Some(match error {
                    DispatchError::Failed { error, .. } => error.message,
                    other => other.to_string(),
                });
                question.current = question.current.min(question.statements.len().saturating_sub(1));
                Some(question)
            }
        },
    }
}

pub(super) fn show(
    ui: &mut Ui,
    p: &IdePalette,
    (registry, session): (&Registry, &mut Session),
    mut question: ExecuteQuestion,
    fresh: bool,
) -> Option<ExecuteQuestion> {
    let Some(answer) = draw(ui, p, &mut question, fresh) else { return Some(question) };
    let index = question.statements.get(question.current).and_then(|statement| statement["index"].as_u64());
    match answer {
        Answer::Cancel => return None,
        Answer::All => return run(registry, session, question),
        Answer::Yes => {
            if dangerous(&question.statements[question.current]) && !question.understood {
                return Some(question);
            }
        }
        Answer::Skip => question.skip.extend(index),
    }
    question.current += 1;
    question.understood = false;
    question.error = None;
    if question.current >= question.statements.len() {
        return run(registry, session, question);
    }
    Some(question)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_to_confirm_becomes_a_question_and_others_do_not() {
        let error = zen_core::CommandError::with_code("confirmExecute", "asks").with_data(json!({
            "token": "ab12",
            "target": { "name": "Shop", "typeName": "Production", "color": "red" },
            "statements": [{ "index": 1, "text": "DELETE FROM orders", "effect": "data", "dangers": [{ "risk": "deleteWithoutWhere" }], "risks": ["DELETE without WHERE removes every row"] }],
            "total": 2,
            "retry": { "command": "db.console.executeScript", "params": { "path": "/c.sql", "confirmed": "ab12" } },
        }));
        let refused =
            DispatchError::Failed { id: zen_core::CommandId::new("db.console.executeScript").unwrap(), error };
        let asked = question(&refused).unwrap();
        assert_eq!((asked.command.as_str(), asked.total, asked.statements.len()), ("db.console.executeScript", 2, 1));
        assert!(dangerous(&asked.statements[0]));
        assert!(asked.same(&asked.clone()));
        let other = DispatchError::Failed {
            id: zen_core::CommandId::new("db.console.execute").unwrap(),
            error: zen_core::CommandError::with_code("readOnly", "no"),
        };
        assert!(question(&other).is_none());
    }
}
