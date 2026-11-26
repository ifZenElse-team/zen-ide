use egui::{Align, Key, Layout, Modifiers, RichText, Ui};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_db::params::{ParamType, ParamValue};
use zen_i18n::t;

use super::transaction_dialog::Dialog;
use super::{IdePalette, dialog_frame, theme, vcs};
use crate::accessibility;

const TYPES: [ParamType; 4] = [ParamType::Text, ParamType::Number, ParamType::Boolean, ParamType::Null];

#[derive(Clone, Debug, PartialEq)]
struct Row {
    name: String,
    shown: String,
    kind: ParamType,
    value: String,
    secret: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ParametersQuestion {
    message: String,
    rows: Vec<Row>,
    variables: Vec<(String, String, bool)>,
    inlined: bool,
    command: String,
    params: Value,
    error: Option<String>,
    focused: bool,
    fields: Vec<egui::Id>,
    focus_to: Option<usize>,
}

fn shown_name(name: &str, kind: &str) -> String {
    if kind == "named" { format!(":{name}") } else { name.to_owned() }
}

pub(super) fn question(error: &DispatchError) -> Option<ParametersQuestion> {
    let DispatchError::Failed { error, .. } = error else { return None };
    if error.code.as_deref() != Some(zen_workbench::database::variables::PARAMETERS_NEEDED) {
        return None;
    }
    let data = error.data.as_ref()?;
    let retry = data.get("retry")?;
    let rows = data["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|asked| {
            let name = asked["name"].as_str()?.to_owned();
            let value: Option<ParamValue> = serde_json::from_value(asked["value"].clone()).ok();
            let inferred: ParamType = serde_json::from_value(asked["inferred"].clone()).unwrap_or_default();
            let value = value.unwrap_or(ParamValue { kind: inferred, value: String::new() });
            Some(Row {
                shown: shown_name(&name, asked["kind"].as_str().unwrap_or_default()),
                name,
                kind: value.kind,
                value: value.value,
                secret: asked["secret"] == true,
            })
        })
        .collect();
    let variables = data["variables"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|asked| {
            let name = asked["name"].as_str()?.to_owned();
            Some((name, asked["value"].as_str().unwrap_or_default().to_owned(), asked["secret"] == true))
        })
        .collect();
    Some(ParametersQuestion {
        message: error.message.clone(),
        rows,
        variables,
        inlined: data["inlined"] == true,
        command: retry.get("command")?.as_str()?.to_owned(),
        params: retry.get("params").cloned().unwrap_or_else(|| json!({})),
        error: None,
        focused: false,
        fields: Vec::new(),
        focus_to: None,
    })
}

fn type_label(kind: ParamType) -> String {
    match kind {
        ParamType::Text => t!("ide-dbparams-type-text"),
        ParamType::Number => t!("ide-dbparams-type-number"),
        ParamType::Boolean => t!("ide-dbparams-type-boolean"),
        ParamType::Null => t!("ide-dbparams-type-null"),
    }
}

fn row_error(row: &Row) -> Option<String> {
    if row.value.is_empty() {
        return None;
    }
    ParamValue { kind: row.kind, value: row.value.clone() }
        .to_value()
        .err()
        .map(|error| zen_workbench::database::variables::param_error(&error))
}

impl ParametersQuestion {
    fn params_with_values(&self) -> Value {
        let mut params = self.params.clone();
        let parameters: serde_json::Map<String, Value> = self
            .rows
            .iter()
            .filter(|row| !(row.secret && row.value.is_empty() && row.kind != ParamType::Null))
            .map(|row| {
                let value = ParamValue { kind: row.kind, value: row.value.clone() };
                (row.name.clone(), serde_json::to_value(value).unwrap_or_default())
            })
            .collect();
        let variables: serde_json::Map<String, Value> = self
            .variables
            .iter()
            .filter(|(_, value, secret)| !(*secret && value.is_empty()))
            .map(|(name, value, _)| (name.clone(), json!(value)))
            .collect();
        if let Some(object) = params.as_object_mut() {
            object.insert("parameters".to_owned(), Value::Object(parameters));
            if !variables.is_empty() {
                object.insert("variables".to_owned(), Value::Object(variables));
            }
        }
        params
    }

    fn valid(&self) -> bool {
        self.empty().is_empty() && self.rows.iter().all(|row| row_error(row).is_none())
    }

    fn empty(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|row| row.kind != ParamType::Null && row.value.is_empty())
            .map(|row| row.shown.as_str())
            .collect()
    }

    fn tab(&mut self, ui: &Ui) {
        let count = self.fields.len();
        let Some(at) = self.fields.iter().position(|id| ui.memory(|memory| memory.has_focus(*id))) else { return };
        let (back, forward) = ui.input_mut(|input| {
            (input.consume_key(Modifiers::SHIFT, Key::Tab), input.consume_key(Modifiers::NONE, Key::Tab))
        });
        if forward || back {
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
            self.focus_to = Some(if forward { (at + 1) % count } else { (at + count - 1) % count });
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    Run,
    Cancel,
}

fn field(ui: &mut Ui, p: &IdePalette, value: &mut String, secret: bool, name: &str) -> egui::Response {
    let edit = egui::TextEdit::singleline(value)
        .password(secret)
        .font(egui::FontId::monospace(12.5))
        .text_color(p.text)
        .desired_width(260.0);
    let response = super::connection_dialog::input(ui, p, edit);
    accessibility::name(&response, egui::Role::TextInput, name);
    response
}

fn draw(ui: &mut Ui, p: &IdePalette, question: &mut ParametersQuestion, fresh: bool) -> Option<Answer> {
    let mut answer = None;
    let modal = egui::Modal::new(ui.scope_id().with("bind-parameters")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(560.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(
            egui::Label::new(RichText::new(t!("ide-dbparams-title")).font(theme::semibold(15.0)).color(p.text)).wrap(),
        );
        ui.add_space(8.0);
        ui.add(
            egui::Label::new(RichText::new(t!("ide-dbparams-hint")).font(theme::regular(13.0)).color(p.text_secondary))
                .wrap(),
        );
        if question.inlined {
            ui.add_space(6.0);
            let note = RichText::new(t!("ide-dbparams-inlined")).font(theme::regular(12.5)).color(p.warning_text);
            ui.add(egui::Label::new(note).wrap());
        }
        ui.add_space(12.0);
        question.tab(ui);
        let mut first = !question.focused;
        let focus_to = question.focus_to.take();
        let mut fields = Vec::new();
        let focus = |response: &egui::Response, fields: &mut Vec<egui::Id>, first: &mut bool| {
            if focus_to == Some(fields.len()) || (*first && focus_to.is_none()) {
                response.request_focus();
            }
            *first = false;
            fields.push(response.id);
        };
        egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            if !question.rows.is_empty() {
                egui::Grid::new("bind-parameters-grid").num_columns(3).spacing([12.0, 8.0]).min_row_height(28.0).show(
                    ui,
                    |ui| {
                        for header in [t!("ide-dbparams-name"), t!("ide-dbparams-type"), t!("ide-dbparams-value")] {
                            ui.label(RichText::new(header).font(theme::semibold(12.5)).color(p.text_secondary));
                        }
                        ui.end_row();
                        for (index, row) in question.rows.iter_mut().enumerate() {
                            ui.label(RichText::new(&row.shown).font(egui::FontId::monospace(12.5)).color(p.text));
                            let type_name = t!("ide-dbparams-type-of", name = row.shown.as_str());
                            let shown = type_label(row.kind);
                            super::form::combo(
                                ui,
                                p,
                                ("bind-parameter-type", index),
                                &type_name,
                                &shown,
                                110.0,
                                |ui| {
                                    for kind in TYPES {
                                        ui.selectable_value(&mut row.kind, kind, type_label(kind));
                                    }
                                },
                            );
                            ui.vertical(|ui| {
                                let enabled = row.kind != ParamType::Null;
                                let name = t!("ide-dbparams-value-of", name = row.shown.as_str());
                                let response = ui
                                    .add_enabled_ui(enabled, |ui| field(ui, p, &mut row.value, row.secret, &name))
                                    .inner;
                                if enabled {
                                    focus(&response, &mut fields, &mut first);
                                }
                                if let Some(error) = row_error(row) {
                                    ui.add_space(2.0);
                                    ui.label(RichText::new(error).font(theme::regular(12.0)).color(p.error_text));
                                }
                            });
                            ui.end_row();
                        }
                    },
                );
            }
            if !question.variables.is_empty() {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(t!("ide-dbparams-variables")).font(theme::semibold(12.5)).color(p.text_secondary),
                );
                ui.add_space(6.0);
                egui::Grid::new("bind-variables-grid").num_columns(2).spacing([12.0, 8.0]).min_row_height(28.0).show(
                    ui,
                    |ui| {
                        for (name, value, secret) in &mut question.variables {
                            let shown = format!("${{{name}}}");
                            ui.label(RichText::new(&shown).font(egui::FontId::monospace(12.5)).color(p.text));
                            let label = t!("ide-dbparams-value-of", name = shown.as_str());
                            let response = field(ui, p, value, *secret, &label);
                            focus(&response, &mut fields, &mut first);
                            ui.end_row();
                        }
                    },
                );
            }
        });
        question.focused = true;
        question.fields = fields;
        let empty = question.empty();
        if !empty.is_empty() {
            ui.add_space(8.0);
            let names = empty.join(", ");
            let note = RichText::new(t!("ide-dbparams-empty", names = names.as_str())).font(theme::regular(12.5));
            ui.add(egui::Label::new(note.color(p.text_secondary)).wrap());
        }
        if let Some(error) = &question.error {
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(RichText::new(error.clone()).font(theme::regular(13.0)).color(p.error_text)).wrap(),
            );
        }
        ui.add_space(18.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                answer = Some(Answer::Cancel);
            }
            let valid = question.valid();
            let run = ui.add_enabled_ui(valid, |ui| vcs::primary_button(ui, p, &t!("ide-dbparams-run"))).inner;
            if run.clicked() {
                answer = Some(Answer::Run);
            }
        });
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            answer = Some(Answer::Cancel);
        }
        if question.valid() && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
            answer = Some(Answer::Run);
        }
    });
    answer.or_else(|| (!fresh && modal.should_close()).then_some(Answer::Cancel))
}

pub(super) fn show(
    ui: &mut Ui,
    p: &IdePalette,
    (registry, session): (&Registry, &mut Session),
    mut question: ParametersQuestion,
    fresh: bool,
) -> Option<Dialog> {
    let Some(answer) = draw(ui, p, &mut question, fresh) else { return Some(Dialog::Parameters(question)) };
    match answer {
        Answer::Cancel => None,
        Answer::Run => match registry.dispatch(session, &question.command, question.params_with_values()) {
            Ok(_) => None,
            Err(error) => match super::transaction_dialog::dialog(&error) {
                Some(Dialog::Parameters(next)) => {
                    question.error = Some(next.message);
                    Some(Dialog::Parameters(question))
                }
                Some(next) => Some(next),
                None if let Some(several) =
                    super::several_dialog::resumed(&error, (&question.command, &question.params)) =>
                {
                    Some(several)
                }
                None => {
                    question.error = Some(match error {
                        DispatchError::Failed { error, .. } => error.message,
                        other => other.to_string(),
                    });
                    Some(Dialog::Parameters(question))
                }
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_for_values_becomes_a_dialog_that_runs_again_with_them() {
        let error = zen_core::CommandError::with_code("parametersNeeded", "values").with_data(json!({
            "parameters": [
                { "name": "id", "kind": "named", "value": { "type": "number", "value": "7" }, "secret": false },
                { "name": "$2", "kind": "numbered", "secret": false, "inferred": "number" },
                { "name": "password", "kind": "named", "secret": true },
            ],
            "variables": [{ "name": "table", "value": "orders", "secret": false }],
            "inlined": false,
            "retry": { "command": "db.console.executeScript", "params": { "path": "/c.sql", "newTab": false } },
        }));
        let refused =
            DispatchError::Failed { id: zen_core::CommandId::new("db.console.executeScript").unwrap(), error };
        let mut asked = question(&refused).unwrap();
        assert_eq!(asked.rows.iter().map(|row| row.shown.as_str()).collect::<Vec<_>>(), [":id", "$2", ":password"]);
        assert_eq!((asked.rows[0].kind, asked.rows[0].value.as_str()), (ParamType::Number, "7"));
        assert!(asked.rows[2].secret && asked.rows[2].value.is_empty());
        assert_eq!(asked.rows[1].kind, ParamType::Number, "the type read from where it is used comes first");
        assert_eq!(asked.rows[2].kind, ParamType::Text);
        assert!(row_error(&asked.rows[1]).is_none(), "an empty number is not an error before anything is typed");
        assert!(!asked.valid(), "it is still asked for");
        asked.rows[1].kind = ParamType::Null;
        asked.rows[2].value = "pw".into();
        assert!(asked.valid());
        let params = asked.params_with_values();
        assert_eq!(params["path"], "/c.sql");
        assert_eq!(params["parameters"]["id"], json!({ "type": "number", "value": "7" }));
        assert_eq!(params["parameters"]["$2"]["type"], "null");
        assert_eq!(params["variables"], json!({ "table": "orders" }));
        asked.rows[0].value = "seven".into();
        assert!(!asked.valid(), "a number that does not read as one cannot run");
        let other = DispatchError::Failed {
            id: zen_core::CommandId::new("db.console.execute").unwrap(),
            error: zen_core::CommandError::with_code("readOnly", "no"),
        };
        assert!(question(&other).is_none());
    }

    fn asked() -> ParametersQuestion {
        let error = zen_core::CommandError::with_code("parametersNeeded", "values").with_data(json!({
            "parameters": [
                { "name": "id", "kind": "named", "value": { "type": "number", "value": "7" }, "secret": false },
                { "name": "name", "kind": "named", "secret": false },
                { "name": "skip", "kind": "named", "value": { "type": "null", "value": "" }, "secret": false },
            ],
            "variables": [{ "name": "table", "value": "orders", "secret": false }],
            "inlined": false,
            "retry": { "command": "db.console.execute", "params": { "path": "/c.sql" } },
        }));
        let refused = DispatchError::Failed { id: zen_core::CommandId::new("db.console.execute").unwrap(), error };
        question(&refused).unwrap()
    }

    fn tab(shift: bool) -> egui::Event {
        let modifiers = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
        egui::Event::Key { key: Key::Tab, physical_key: Some(Key::Tab), pressed: true, repeat: false, modifiers }
    }

    fn focused_value(screen: &mut crate::offscreen::Offscreen, question: &ParametersQuestion) -> Option<usize> {
        question.fields.iter().position(|id| screen.context().memory(|memory| memory.has_focus(*id)))
    }

    #[test]
    fn tab_moves_between_the_value_fields_and_the_first_has_the_keyboard_on_open() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let p = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let mut question = asked();
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(900.0, 600.0), 1.0);
        screen.step(Vec::new(), |ui| {
            draw(ui, &p, &mut question, true);
        });
        screen.step(Vec::new(), |ui| {
            draw(ui, &p, &mut question, false);
        });
        assert_eq!(question.fields.len(), 3, "the NULL parameter has no field to fill");
        assert_eq!(focused_value(&mut screen, &question), Some(0), "the first value has the keyboard");
        for (events, expected) in [
            (vec![tab(false)], 1),
            (vec![egui::Event::Text("Ann".into())], 1),
            (vec![tab(false)], 2),
            (vec![tab(false)], 0),
            (vec![tab(true)], 2),
            (vec![tab(true)], 1),
        ] {
            screen.step(events, |ui| {
                draw(ui, &p, &mut question, false);
            });
            screen.step(Vec::new(), |ui| {
                draw(ui, &p, &mut question, false);
            });
            assert_eq!(focused_value(&mut screen, &question), Some(expected));
        }
        assert_eq!(question.rows[0].value, "7", "the tab did not type into the first field");
        assert_eq!(question.rows[1].value, "Ann");
        assert_eq!(question.variables[0].1, "orders");
    }

    #[test]
    fn an_empty_value_blocks_run_until_it_is_filled_or_null() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let mut question = asked();
        assert_eq!(question.empty(), [":name"]);
        assert!(!question.valid(), "an empty text cannot run");
        question.rows[1].kind = ParamType::Null;
        assert!(question.valid(), "NULL needs no value");
        question.rows[1].kind = ParamType::Text;
        question.rows[1].value = "Ann".into();
        assert!(question.valid());
        let p = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        question.rows[1].value.clear();
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(900.0, 600.0), 1.0);
        let mut answers = Vec::new();
        for events in [
            Vec::new(),
            vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: Some(Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
        ] {
            screen.step(events, |ui| answers.push(draw(ui, &p, &mut question, false)));
        }
        assert!(answers.iter().all(|answer| *answer != Some(Answer::Run)), "Enter does not run with an empty value");
    }

    #[test]
    fn the_dialog_renders_in_both_themes() {
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from) else { return };
        zen_i18n::register(crate::toolbox::LOCALES);
        for (name, theme) in
            [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
        {
            let p = IdePalette::from_theme(&theme);
            let mut question = asked();
            let mut screen = crate::offscreen::Offscreen::new(egui::vec2(900.0, 600.0), 1.5);
            p.apply(screen.context());
            screen.step(Vec::new(), |ui| {
                draw(ui, &p, &mut question, true);
            });
            screen.step(vec![tab(false)], |ui| {
                draw(ui, &p, &mut question, false);
            });
            let frame = screen.render(|ui| {
                draw(ui, &p, &mut question, false);
            });
            crate::offscreen::write_png(&dir.join(format!("b1-bind-parameters-{name}.png")), frame.image()).unwrap();
        }
    }
}

#[cfg(test)]
mod secret_tests {
    use super::*;

    fn row(name: &str, value: &str, secret: bool) -> Row {
        Row { name: name.to_owned(), shown: format!(":{name}"), kind: ParamType::Text, value: value.to_owned(), secret }
    }

    #[test]
    fn a_secret_left_empty_is_not_sent_so_the_remembered_value_stays() {
        let question = ParametersQuestion {
            message: String::new(),
            rows: vec![row("id", "", false), row("password", "", true), row("token", "t0k", true)],
            variables: vec![("schema".to_owned(), String::new(), false), ("api_key".to_owned(), String::new(), true)],
            inlined: false,
            command: "db.console.execute".to_owned(),
            params: json!({}),
            error: None,
            focused: false,
            fields: Vec::new(),
            focus_to: None,
        };
        let params = question.params_with_values();
        let sent: Vec<&String> = params["parameters"].as_object().unwrap().keys().collect();
        assert_eq!(sent, ["id", "token"], "an empty text is a value, an empty secret is not");
        let variables: Vec<&String> = params["variables"].as_object().unwrap().keys().collect();
        assert_eq!(variables, ["schema"]);
    }
}
