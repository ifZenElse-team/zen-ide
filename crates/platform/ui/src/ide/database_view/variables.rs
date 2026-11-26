use std::collections::HashMap;
use std::path::Path;

use egui::{Align2, FontId, RichText, Ui, UiBuilder, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::super::glyphs::glyph_close;
use super::super::{IdePalette, theme, vcs};
use super::DatabaseView;
use crate::accessibility;

#[derive(Default)]
pub(super) struct VariablesEdit {
    edited: HashMap<String, String>,
    name: String,
    value: String,
}

fn field(
    ui: &mut Ui,
    p: &IdePalette,
    value: &mut String,
    (width, hint, name): (f32, String, String),
) -> egui::Response {
    let edit = egui::TextEdit::singleline(value)
        .font(FontId::monospace(12.5))
        .text_color(p.text)
        .hint_text(hint)
        .desired_width(width);
    let response = ui
        .allocate_ui(vec2(width + 14.0, 24.0), |ui| {
            ui.set_min_width(width + 14.0);
            super::super::connection_dialog::input(ui, p, edit)
        })
        .inner;
    accessibility::name(&response, egui::Role::TextInput, &name);
    response
}

fn remove_button(ui: &mut Ui, p: &IdePalette, label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    glyph_close(ui.painter(), rect.shrink(4.0), if response.hovered() { p.text } else { p.text_secondary });
    accessibility::button_with(&response, || label.to_owned());
    response.on_hover_text(label.to_owned()).clicked()
}

fn heading(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.label(RichText::new(text).font(theme::semibold(12.5)).color(p.text_secondary));
    ui.add_space(4.0);
}

fn value_text(value: &Value) -> String {
    match value["type"].as_str() {
        Some("null") => "NULL".to_owned(),
        Some("text") => format!("'{}'", value["value"].as_str().unwrap_or_default()),
        _ => value["value"].as_str().unwrap_or_default().to_owned(),
    }
}

pub(super) fn body(
    view: &mut DatabaseView,
    ui: &mut Ui,
    p: &IdePalette,
    registry: &Registry,
    session: &mut Session,
    path: &Path,
) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0, p.editor_background);
    let answer = registry.dispatch(session, "db.console.variables", json!({ "path": path })).unwrap_or_default();
    let variables = answer["variables"].as_array().cloned().unwrap_or_default();
    let parameters = answer["parameters"].as_array().cloned().unwrap_or_default();
    let mut child =
        ui.new_child(UiBuilder::new().id_salt("database-variables").max_rect(area.shrink2(vec2(12.0, 8.0))));
    let mut commands: Vec<(&str, Value)> = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        heading(ui, p, t!("ide-dbvars-variables"));
        let edit = &mut view.results.variables;
        egui::Grid::new("database-variables-grid").num_columns(3).spacing([10.0, 6.0]).show(ui, |ui| {
            for variable in &variables {
                let Some(name) = variable["name"].as_str() else { continue };
                let shown = format!("${{{name}}}");
                ui.label(RichText::new(&shown).font(FontId::monospace(12.5)).color(p.text));
                if variable["secret"] == true {
                    ui.label(RichText::new(t!("ide-dbvars-secret")).font(theme::regular(12.5)).color(p.text_secondary));
                } else {
                    let stored = variable["value"].as_str().unwrap_or_default().to_owned();
                    let value = edit.edited.entry(name.to_owned()).or_insert_with(|| stored.clone());
                    let response =
                        field(ui, p, value, (280.0, String::new(), t!("ide-dbparams-value-of", name = shown.as_str())));
                    if response.lost_focus() && *value != stored {
                        commands
                            .push(("db.console.setVariable", json!({ "path": path, "name": name, "value": value })));
                    }
                    if !response.has_focus() && !response.lost_focus() {
                        *value = stored;
                    }
                }
                if remove_button(ui, p, &t!("ide-dbvars-remove", name = shown.as_str())) {
                    edit.edited.remove(name);
                    commands.push(("db.console.setVariable", json!({ "path": path, "name": name })));
                }
                ui.end_row();
            }
            let name_response =
                field(ui, p, &mut edit.name, (140.0, t!("ide-dbvars-new-name"), t!("ide-dbvars-new-name")));
            let value_response =
                field(ui, p, &mut edit.value, (280.0, t!("ide-dbvars-new-value"), t!("ide-dbvars-new-value")));
            let entered = (name_response.lost_focus() || value_response.lost_focus())
                && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let add = vcs::secondary_button(ui, p, &t!("ide-dbvars-add")).clicked() || entered;
            if add && !edit.name.trim().is_empty() {
                let name = std::mem::take(&mut edit.name);
                let value = std::mem::take(&mut edit.value);
                commands.push(("db.console.setVariable", json!({ "path": path, "name": name.trim(), "value": value })));
            }
            ui.end_row();
        });
        ui.add_space(14.0);
        heading(ui, p, t!("ide-dbvars-parameters"));
        if parameters.is_empty() {
            ui.label(RichText::new(t!("ide-dbvars-no-parameters")).font(theme::regular(12.5)).color(p.text_secondary));
        }
        egui::Grid::new("database-parameters-grid").num_columns(3).spacing([10.0, 6.0]).show(ui, |ui| {
            for parameter in &parameters {
                let Some(name) = parameter["name"].as_str() else { continue };
                let shown = if name.starts_with(['$', '?']) { name.to_owned() } else { format!(":{name}") };
                ui.label(RichText::new(&shown).font(FontId::monospace(12.5)).color(p.text));
                let value =
                    if parameter["secret"] == true { t!("ide-dbvars-secret") } else { value_text(&parameter["value"]) };
                ui.label(RichText::new(value).font(FontId::monospace(12.5)).color(p.text));
                if remove_button(ui, p, &t!("ide-dbvars-forget", name = shown.as_str())) {
                    commands.push(("db.console.setParameter", json!({ "path": path, "name": name, "forget": true })));
                }
                ui.end_row();
            }
        });
    });
    if variables.is_empty() && parameters.is_empty() {
        let bottom = egui::pos2(area.center().x, area.bottom() - 18.0);
        ui.painter().text(bottom, Align2::CENTER_CENTER, t!("ide-dbvars-hint"), theme::regular(12.0), p.text_secondary);
    }
    for (command, params) in commands {
        view.dispatch(registry, session, command, params);
    }
}
