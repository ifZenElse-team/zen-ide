use std::path::{Path, PathBuf};

use egui::{Align, Key, Layout, Modifiers, RichText, Ui};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;

use super::database_navigator::type_color;
use super::transaction_dialog::{Dialog, dialog as refusal_dialog, request};
use super::{IdePalette, dialog_frame, theme, vcs};

const COMMAND: &str = "db.script.runOnConnections";

const STATEMENTS_SHOWN: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SeveralQuestion {
    path: PathBuf,
    chosen: Vec<String>,
    stop_on_error: bool,
    connections: Option<Vec<Value>>,
    types: Vec<Value>,
    confirm: Option<(String, Value)>,
    refused: Option<(String, Value)>,
    understood: bool,
    error: Option<String>,
}

impl SeveralQuestion {
    fn new(path: PathBuf, own: Option<String>) -> Self {
        Self {
            path,
            chosen: own.into_iter().collect(),
            stop_on_error: true,
            connections: None,
            types: Vec::new(),
            confirm: None,
            refused: None,
            understood: false,
            error: None,
        }
    }
}

pub(super) fn open(session: &mut Session, path: &Path, own: Option<String>) {
    request(session, Dialog::Several(SeveralQuestion::new(path.to_owned(), own)));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    Run,
    Back,
    Cancel,
}

fn label(ui: &mut Ui, text: String, color: egui::Color32) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::regular(13.0)).color(color)).wrap());
}

fn danger_button(ui: &mut Ui, p: &IdePalette, text: &str, enabled: bool) -> egui::Response {
    let button = egui::Button::new(RichText::new(text).font(theme::regular(13.0)).color(p.accent_text))
        .min_size(egui::vec2(84.0, 30.0))
        .corner_radius(6)
        .fill(p.error_text);
    ui.add_enabled(enabled, button)
}

fn kind_of<'a>(question: &'a SeveralQuestion, id: &str) -> Option<&'a Value> {
    question.types.iter().find(|kind| kind["id"] == id)
}

fn connection_line(ui: &mut Ui, p: &IdePalette, (name, kind, color): (&str, &str, &str)) {
    let color = type_color(p, color).unwrap_or(p.text_secondary);
    let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
    ui.painter().circle_filled(dot.center(), 4.0, color);
    ui.label(RichText::new(name).font(theme::semibold(13.0)).color(p.text));
    ui.label(RichText::new(kind).font(theme::regular(12.5)).color(color));
}

fn choose(ui: &mut Ui, p: &IdePalette, question: &mut SeveralQuestion) {
    label(ui, t!("ide-several-choose"), p.text_secondary);
    ui.add_space(8.0);
    let connections = question.connections.clone().unwrap_or_default();
    if connections.is_empty() {
        label(ui, t!("ide-database-no-connections"), p.text_secondary);
    }
    egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for connection in &connections {
                let Some(id) = connection["id"].as_str() else { continue };
                let name = connection["name"].as_str().unwrap_or(id);
                let kind = connection["type"].as_str().unwrap_or_default();
                let (type_name, color) =
                    kind_of(question, kind).map_or((kind.to_owned(), "none".to_owned()), |known| {
                        (
                            known["name"].as_str().unwrap_or(kind).to_owned(),
                            known["color"].as_str().unwrap_or("none").to_owned(),
                        )
                    });
                let order = question.chosen.iter().position(|chosen| chosen == id);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let mut ticked = order.is_some();
                    let text = RichText::new(name).font(theme::regular(13.0)).color(p.text);
                    if ui.checkbox(&mut ticked, text).changed() {
                        if ticked {
                            question.chosen.push(id.to_owned());
                        } else {
                            question.chosen.retain(|chosen| chosen != id);
                        }
                    }
                    let shown = type_color(p, &color).unwrap_or(p.text_secondary);
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, shown);
                    ui.label(RichText::new(type_name).font(theme::regular(12.5)).color(shown));
                    if let Some(order) = order {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let place = t!("ide-several-order", number = order + 1);
                            ui.label(RichText::new(place).font(theme::regular(12.0)).color(p.text_secondary));
                        });
                    }
                });
            }
        });
    });
    ui.add_space(10.0);
    ui.checkbox(
        &mut question.stop_on_error,
        RichText::new(t!("ide-several-stop-on-error")).font(theme::regular(13.0)).color(p.text),
    );
}

fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let short: String = line.chars().take(80).collect();
    if short.len() < line.len() { format!("{short}…") } else { short }
}

fn confirm(ui: &mut Ui, p: &IdePalette, question: &mut SeveralQuestion) {
    let Some((message, data)) = question.confirm.clone() else { return };
    label(ui, message, p.text_secondary);
    ui.add_space(10.0);
    egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
            for entry in data["targets"].as_array().into_iter().flatten() {
                let target = &entry["target"];
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    connection_line(
                        ui,
                        p,
                        (
                            target["name"].as_str().unwrap_or_default(),
                            target["typeName"].as_str().unwrap_or_default(),
                            target["color"].as_str().unwrap_or("none"),
                        ),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (text, color) = if entry["asks"] == true {
                            (t!("ide-several-asks", count = entry["asked"].as_u64().unwrap_or(0)), p.warning_text)
                        } else {
                            (t!("ide-several-runs"), p.text_secondary)
                        };
                        ui.label(RichText::new(text).font(theme::regular(12.0)).color(color));
                    });
                });
                for statement in entry["statements"].as_array().into_iter().flatten().take(STATEMENTS_SHOWN) {
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        let text = first_line(statement["text"].as_str().unwrap_or_default());
                        ui.add(
                            egui::Label::new(RichText::new(text).font(egui::FontId::monospace(12.0)).color(p.text))
                                .truncate(),
                        );
                    });
                    for risk in statement["risks"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                        ui.horizontal(|ui| {
                            ui.add_space(22.0);
                            ui.add(
                                egui::Label::new(RichText::new(risk).font(theme::regular(12.0)).color(p.error_text))
                                    .wrap(),
                            );
                        });
                    }
                }
                ui.add_space(6.0);
            }
        });
    });
    if data["dangerous"] == true {
        ui.add_space(8.0);
        ui.checkbox(
            &mut question.understood,
            RichText::new(t!("ide-several-understood")).font(theme::regular(13.0)).color(p.text),
        );
    }
}

fn refused(ui: &mut Ui, p: &IdePalette, question: &SeveralQuestion) {
    let Some((message, data)) = &question.refused else { return };
    label(ui, message.clone(), p.error_text);
    ui.add_space(10.0);
    for entry in data["targets"].as_array().into_iter().flatten() {
        let target = &entry["target"];
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            connection_line(
                ui,
                p,
                (
                    target["name"].as_str().unwrap_or_default(),
                    target["typeName"].as_str().unwrap_or_default(),
                    target["color"].as_str().unwrap_or("none"),
                ),
            );
        });
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            let text = entry["message"].as_str().unwrap_or_default();
            ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
        });
        ui.add_space(6.0);
    }
}

fn draw(ui: &mut Ui, p: &IdePalette, question: &mut SeveralQuestion, fresh: bool) -> Option<Answer> {
    let mut answer = None;
    let dangerous = question.confirm.as_ref().is_some_and(|(_, data)| data["dangerous"] == true);
    let modal =
        egui::Modal::new(ui.scope_id().with("several-connections")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.input_mut(|input| {
                for modifiers in
                    [Modifiers::NONE, Modifiers::COMMAND, Modifiers::CTRL, Modifiers::SHIFT, Modifiers::ALT]
                {
                    while input.consume_key(modifiers, Key::Enter) {}
                }
            });
            ui.set_width(560.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let heading = match (&question.confirm, &question.refused) {
                (Some(_), _) if dangerous => t!("ide-several-title-danger"),
                (Some(_), _) => t!("ide-several-title-confirm"),
                (_, Some(_)) => t!("ide-several-title-refused"),
                _ => t!("ide-several-title"),
            };
            ui.add(egui::Label::new(RichText::new(heading).font(theme::semibold(15.0)).color(p.text)).wrap());
            ui.add_space(4.0);
            let file = question.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            label(ui, file, p.text_secondary);
            ui.add_space(10.0);
            if question.confirm.is_some() {
                confirm(ui, p, question);
            } else if question.refused.is_some() {
                refused(ui, p, question);
            } else {
                choose(ui, p, question);
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
                if (question.confirm.is_some() || question.refused.is_some())
                    && vcs::secondary_button(ui, p, &t!("ide-several-back")).clicked()
                {
                    answer = Some(Answer::Back);
                }
                if question.refused.is_none() {
                    let run = t!("ide-several-run", count = question.chosen.len());
                    let clicked = if dangerous {
                        danger_button(ui, p, &t!("ide-dbsafety-run-anyway"), question.understood).clicked()
                    } else {
                        ui.add_enabled_ui(!question.chosen.is_empty(), |ui| vcs::primary_button(ui, p, &run))
                            .inner
                            .clicked()
                    };
                    if clicked {
                        answer = Some(Answer::Run);
                    }
                }
            });
            if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
                answer = Some(Answer::Cancel);
            }
        });
    answer.or_else(|| (!fresh && modal.should_close()).then_some(Answer::Cancel))
}

fn after(error: DispatchError, mut question: SeveralQuestion) -> Option<Dialog> {
    if let Some(dialog) = refusal_dialog(&error) {
        return Some(dialog);
    }
    let DispatchError::Failed { error, .. } = error else {
        question.error = Some(error.to_string());
        return Some(Dialog::Several(question));
    };
    let data = error.data.map(Value::Object).unwrap_or_default();
    question.understood = false;
    question.error = None;
    if data["targets"].is_array() && error.code.as_deref() == Some(zen_workbench::database::safety::CONFIRM_EXECUTE) {
        question.confirm = Some((error.message, data));
    } else if data["targets"].is_array() {
        question.confirm = None;
        question.refused = Some((error.message, data));
    } else {
        question.error = Some(error.message);
    }
    Some(Dialog::Several(question))
}

pub(super) fn resumed(error: &DispatchError, (command, params): (&str, &Value)) -> Option<Dialog> {
    let DispatchError::Failed { error: failed, .. } = error else { return None };
    let lists = failed.data.as_ref().is_some_and(|data| data.get("targets").is_some_and(Value::is_array));
    if command != COMMAND || !lists {
        return None;
    }
    let mut question = SeveralQuestion::new(PathBuf::from(params["path"].as_str().unwrap_or_default()), None);
    question.chosen =
        params["connections"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    question.stop_on_error = params["stopOnError"].as_bool().unwrap_or(true);
    after(error.clone(), question)
}

fn run(registry: &Registry, session: &mut Session, question: SeveralQuestion) -> Option<Dialog> {
    let (command, params) = match &question.confirm {
        Some((_, data)) => {
            (data["retry"]["command"].as_str().unwrap_or(COMMAND).to_owned(), data["retry"]["params"].clone())
        }
        None => (
            COMMAND.to_owned(),
            json!({ "path": question.path, "connections": question.chosen, "stopOnError": question.stop_on_error }),
        ),
    };
    match registry.dispatch(session, &command, params) {
        Ok(_) => None,
        Err(error) => after(error, question),
    }
}

pub(super) fn show(
    ui: &mut Ui,
    p: &IdePalette,
    (registry, session): (&Registry, &mut Session),
    mut question: SeveralQuestion,
    fresh: bool,
) -> Option<Dialog> {
    if question.connections.is_none() {
        let listed = registry.dispatch(session, "db.dataSource.list", json!({ "limit": 200 })).unwrap_or_default();
        question.connections = Some(listed["connections"].as_array().cloned().unwrap_or_default());
        let types = registry.dispatch(session, "db.connectionType.list", json!({})).unwrap_or_default();
        question.types = types["types"].as_array().cloned().unwrap_or_default();
    }
    match draw(ui, p, &mut question, fresh) {
        None => Some(Dialog::Several(question)),
        Some(Answer::Cancel) => None,
        Some(Answer::Back) => {
            question.confirm = None;
            question.refused = None;
            question.understood = false;
            Some(Dialog::Several(question))
        }
        Some(Answer::Run) => {
            if question.confirm.as_ref().is_some_and(|(_, data)| data["dangerous"] == true) && !question.understood {
                return Some(Dialog::Several(question));
            }
            run(registry, session, question)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_that_lists_connections_becomes_the_confirmation_or_the_reasons() {
        let question = SeveralQuestion::new(PathBuf::from("/c.sql"), Some("dev".to_owned()));
        assert_eq!(question.chosen, ["dev"]);
        let confirm = zen_core::CommandError::with_code("confirmExecute", "asks").with_data(json!({
            "token": "ab12",
            "targets": [{ "target": { "name": "Live", "typeName": "Production", "color": "red" }, "asks": true }],
            "retry": { "command": COMMAND, "params": { "confirmed": "ab12" } },
        }));
        let id = zen_core::CommandId::new(COMMAND).unwrap();
        let asked = after(DispatchError::Failed { id: id.clone(), error: confirm }, question.clone());
        let Some(Dialog::Several(asked)) = asked else { panic!("not this dialog") };
        assert!(asked.confirm.is_some() && asked.refused.is_none());
        let refusal = zen_core::CommandError::with_code("readOnly", "no").with_data(json!({
            "targets": [{ "target": { "name": "Audit" }, "code": "readOnly", "message": "read-only" }],
        }));
        let Some(Dialog::Several(refused)) = after(DispatchError::Failed { id, error: refusal }, question) else {
            panic!("not this dialog")
        };
        assert!(refused.refused.is_some() && refused.confirm.is_none());
    }

    #[test]
    fn a_confirmation_after_the_parameters_dialog_opens_this_dialog_again() {
        let params =
            json!({ "path": "/c.sql", "connections": ["dev", "live"], "stopOnError": false, "parameters": {} });
        let confirm = zen_core::CommandError::with_code("confirmExecute", "asks").with_data(json!({
            "token": "ab12",
            "targets": [{ "target": { "name": "Live" }, "asks": true }],
            "retry": { "command": COMMAND, "params": { "confirmed": "ab12", "parameters": { "id": 1 } } },
        }));
        let failed = DispatchError::Failed { id: zen_core::CommandId::new(COMMAND).unwrap(), error: confirm };
        let Some(Dialog::Several(asked)) = resumed(&failed, (COMMAND, &params)) else { panic!("not this dialog") };
        assert_eq!(
            (asked.path.as_path(), asked.chosen.as_slice()),
            (Path::new("/c.sql"), ["dev", "live"].map(String::from).as_slice())
        );
        assert!(!asked.stop_on_error && asked.confirm.is_some());
        assert!(resumed(&failed, ("db.console.execute", &params)).is_none());
    }
}
