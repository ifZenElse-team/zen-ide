use egui::Ui;
use serde_json::{Value, json};
use zen_i18n::t;

use super::{Dispatch, widgets};
use crate::ide::IdePalette;
use crate::theme;

#[derive(Default)]
pub(in crate::ide) struct SchemasPage {
    listed: Option<Value>,
    rows: Vec<(String, String)>,
    edited: bool,
    message: Option<String>,
}

fn rows_of(listed: &Value) -> Vec<(String, String)> {
    listed["mappings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|mapping| {
            let files: Vec<&str> =
                mapping["files"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            (mapping["schema"].as_str().unwrap_or_default().to_owned(), files.join(", "))
        })
        .collect()
}

pub(in crate::ide) fn mappings_of(rows: &[(String, String)]) -> Value {
    let mappings: Vec<Value> = rows
        .iter()
        .filter(|(schema, files)| !schema.trim().is_empty() || !files.trim().is_empty())
        .map(|(schema, files)| {
            let files: Vec<&str> = files.split(',').map(str::trim).filter(|file| !file.is_empty()).collect();
            json!({ "schema": schema.trim(), "files": files })
        })
        .collect();
    json!({ "mappings": mappings })
}

impl SchemasPage {
    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("schemas.catalog.list", json!({})) {
            Ok(listed) => {
                if !self.edited {
                    self.rows = rows_of(&listed);
                }
                self.listed = Some(listed);
            }
            Err(error) => self.message = Some(error),
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        query: Option<&str>,
        dispatch: Option<&mut Dispatch<'_>>,
    ) {
        ui.add(egui::Label::new(egui::RichText::new(t!("settings-schemas-intro")).color(p.text_secondary)).wrap());
        let Some(dispatch) = dispatch else { return };
        if self.listed.is_none() {
            self.load(dispatch);
        }
        let Some(listed) = self.listed.clone() else { return };
        ui.add_space(10.0);
        widgets::group_title(ui, p, &t!("settings-schemas-shipped"), query);
        for shipped in listed["shipped"].as_array().into_iter().flatten() {
            let files: Vec<&str> =
                shipped["files"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(shipped["name"].as_str().unwrap_or_default()).font(theme::semibold(13.0)));
                ui.label(egui::RichText::new(files.join("  ")).font(theme::regular(12.0)).color(p.text_secondary));
            });
            ui.label(
                egui::RichText::new(shipped["uri"].as_str().unwrap_or_default())
                    .font(egui::FontId::monospace(12.0))
                    .color(p.gutter_text),
            );
            ui.add_space(4.0);
        }
        ui.add_space(10.0);
        widgets::group_title(ui, p, &t!("settings-schemas-own"), query);
        let mut removed = None;
        for (index, (schema, files)) in self.rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let schema_field = ui.add(
                    egui::TextEdit::singleline(schema)
                        .hint_text(t!("settings-schemas-schema-hint"))
                        .desired_width(260.0),
                );
                let files_field = ui.add(
                    egui::TextEdit::singleline(files).hint_text(t!("settings-schemas-files-hint")).desired_width(220.0),
                );
                if schema_field.changed() || files_field.changed() {
                    self.edited = true;
                }
                if ui.button(t!("settings-schemas-remove")).clicked() {
                    removed = Some(index);
                }
            });
        }
        if let Some(index) = removed {
            self.rows.remove(index);
            self.edited = true;
        }
        ui.horizontal(|ui| {
            if ui.button(t!("settings-schemas-add")).clicked() {
                self.rows.push((String::new(), String::new()));
                self.edited = true;
            }
            if ui.add_enabled(self.edited, egui::Button::new(t!("settings-schemas-apply"))).clicked() {
                match dispatch("schemas.mappings.set", mappings_of(&self.rows)) {
                    Ok(answer) => {
                        self.edited = false;
                        self.rows = rows_of(&answer);
                        self.listed = Some(answer);
                        self.message = None;
                    }
                    Err(error) => self.message = Some(error),
                }
            }
        });
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.diagnostic_error));
        }
        ui.add_space(10.0);
        let mut downloads = listed["downloads"].as_bool().unwrap_or(false);
        if widgets::checkbox(ui, p, &mut downloads, &t!("settings-schemas-downloads"), query).changed() {
            match dispatch("schemas.downloads.set", json!({ "allowed": downloads })) {
                Ok(answer) => self.listed = Some(answer),
                Err(error) => self.message = Some(error),
            }
        }
        ui.add(
            egui::Label::new(egui::RichText::new(t!("settings-schemas-downloads-note")).color(p.text_secondary)).wrap(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_become_mappings_with_their_patterns_split() {
        let rows = vec![
            ("yaml/kubernetes".to_owned(), "deploy/*.yaml, cluster.yaml".to_owned()),
            (String::new(), String::new()),
        ];
        assert_eq!(
            mappings_of(&rows),
            json!({ "mappings": [{ "schema": "yaml/kubernetes", "files": ["deploy/*.yaml", "cluster.yaml"] }] })
        );
        let listed = json!({ "mappings": [{ "schema": "a.json", "files": ["x.yaml", "y.yaml"] }] });
        assert_eq!(rows_of(&listed), [("a.json".to_owned(), "x.yaml, y.yaml".to_owned())]);
    }
}
