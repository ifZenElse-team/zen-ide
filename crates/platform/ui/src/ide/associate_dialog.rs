use std::path::PathBuf;

use egui::{Color32, Key, Rect, Sense, Stroke, Ui, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_vfs::file_types::{DETECTED, NATIVE};

use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Way {
    Detect,
    Type,
    Application,
}

#[derive(Debug)]
pub(super) struct AssociateDialog {
    pub(super) path: PathBuf,
    pub(super) pattern: String,
    pub(super) way: Way,
    pub(super) chosen: Option<String>,
    types: Vec<(String, String, String)>,
    error: Option<String>,
}

impl AssociateDialog {
    pub(super) fn new(path: PathBuf) -> Self {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let pattern = zen_workbench::file_types::suggested_patterns(&name).into_iter().next().unwrap_or(name);
        let mut types: Vec<(String, String, String)> = zen_workbench::file_types::list()
            .types
            .into_iter()
            .filter(|ty| ty.id != DETECTED && ty.id != NATIVE)
            .map(|ty| {
                let sample =
                    ty.patterns.first().map_or_else(|| "file".to_owned(), |pattern| pattern.replace('*', "file"));
                (ty.id, ty.name, sample)
            })
            .collect();
        types.sort_by_key(|(_, name, _)| name.to_lowercase());
        Self { path, pattern, way: Way::Detect, chosen: None, types, error: None }
    }

    pub(super) fn params(&self) -> Option<serde_json::Value> {
        let type_id = match self.way {
            Way::Detect => DETECTED.to_owned(),
            Way::Application => NATIVE.to_owned(),
            Way::Type => self.chosen.clone()?,
        };
        Some(serde_json::json!({ "pattern": self.pattern.trim(), "type": type_id, "path": self.path }))
    }
}

enum Choice {
    Ok,
    Cancel,
}

impl IdeView {
    pub(super) fn file_context(&self, session: &Session) -> Option<PathBuf> {
        let selected = self.selection.lead().filter(|lead| lead.is_file()).cloned();
        if self.tree_focused && selected.is_some() {
            return selected;
        }
        let workbench = session.state::<zen_workbench::Workbench>();
        workbench
            .and_then(zen_workbench::Workbench::active_document)
            .map(|document| document.path().to_owned())
            .or(selected)
    }

    pub(super) fn open_associate_dialog(&mut self, path: PathBuf) {
        self.associate_dialog = Some(AssociateDialog::new(path));
    }

    pub(super) fn associate_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let product = self.product.name;
        let p = self.palette;
        let Some(dialog) = &mut self.associate_dialog else { return };
        match show(ui, &p, dialog, product) {
            Some(Choice::Cancel) => self.associate_dialog = None,
            Some(Choice::Ok) => {
                let Some(params) = dialog.params() else { return };
                match registry.dispatch(session, "editor.fileTypes.associate", params) {
                    Ok(_) => self.associate_dialog = None,
                    Err(zen_core::DispatchError::Failed { error, .. }) => dialog.error = Some(error.message),
                    Err(other) => dialog.error = Some(other.to_string()),
                }
            }
            None => {}
        }
    }
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut AssociateDialog, product: &str) -> Option<Choice> {
    let mut choice = None;
    let text = |text: String, color: Color32| egui::RichText::new(text).font(theme::regular(13.0)).color(color);
    let modal =
        egui::Modal::new(ui.scope_id().with("associate-file-type")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(egui::RichText::new(t!("ide-associate-title")).font(theme::semibold(15.0)).color(p.text));
            ui.horizontal(|ui| {
                ui.label(text(t!("ide-associate-pattern"), p.text));
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.pattern).desired_width(300.0).id_salt("associate-pattern"),
                );
            });
            let in_ide = dialog.way != Way::Application;
            if ui.radio(in_ide, text(t!("ide-associate-open-in", product = product), p.text)).clicked() && !in_ide {
                dialog.way = Way::Detect;
            }
            ui.indent("associate-ways", |ui| {
                ui.radio_value(&mut dialog.way, Way::Detect, text(t!("ide-associate-detect"), p.text));
                ui.radio_value(&mut dialog.way, Way::Type, text(t!("ide-associate-type"), p.text));
                let enabled = dialog.way == Way::Type;
                egui::Frame::new().stroke(Stroke::new(1.0, p.button_border)).corner_radius(4).inner_margin(2).show(
                    ui,
                    |ui| {
                        egui::ScrollArea::vertical().max_height(176.0).auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            for (id, name, sample) in &dialog.types {
                                let selected = dialog.chosen.as_deref() == Some(id.as_str());
                                let (row, response) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                                crate::accessibility::item(&response, egui::Role::ListBoxOption, selected, name);
                                if selected {
                                    let fill = if enabled { p.selection } else { p.selection.gamma_multiply(0.5) };
                                    ui.painter().rect_filled(row, 3, fill);
                                } else if response.hovered() {
                                    ui.painter().rect_filled(row, 3, p.hover);
                                }
                                let icon = Rect::from_center_size(
                                    egui::pos2(row.left() + 14.0, row.center().y),
                                    vec2(15.0, 15.0),
                                );
                                crate::file_icons::file_icon(ui.painter(), icon, sample);
                                let color = if enabled { p.text } else { p.text_secondary };
                                ui.painter().text(
                                    egui::pos2(row.left() + 28.0, row.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    name,
                                    theme::regular(13.0),
                                    color,
                                );
                                if response.clicked() {
                                    dialog.chosen = Some(id.clone());
                                    dialog.way = Way::Type;
                                }
                            }
                        });
                    },
                );
            });
            ui.radio_value(&mut dialog.way, Way::Application, text(t!("ide-associate-application"), p.text));
            if let Some(error) = &dialog.error {
                ui.add(egui::Label::new(text(error.clone(), p.error_text)).wrap());
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ready = dialog.params().is_some() && !dialog.pattern.trim().is_empty();
                let ok = egui::Button::new(
                    egui::RichText::new(t!("ide-associate-ok")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(if ready { p.accent } else { p.accent.gamma_multiply(0.4) });
                if ui.add_enabled(ready, ok).clicked() {
                    choice = Some(Choice::Ok);
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
                if ready && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                    choice = Some(Choice::Ok);
                }
            });
        });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_starts_as_the_extension_and_ok_names_the_way_chosen() {
        let mut dialog = AssociateDialog::new(PathBuf::from("/p/dump.zt-bin"));
        assert_eq!(dialog.pattern, "*.zt-bin");
        assert_eq!(dialog.params().unwrap()["type"], "detected");
        dialog.way = Way::Type;
        assert_eq!(dialog.params(), None, "a type is to be chosen first");
        dialog.chosen = Some("text".to_owned());
        assert_eq!(dialog.params().unwrap()["type"], "text");
        dialog.way = Way::Application;
        dialog.pattern = " dump.zt-bin ".to_owned();
        let params = dialog.params().unwrap();
        assert_eq!(
            (&params["type"], &params["pattern"]),
            (&serde_json::json!("native"), &serde_json::json!("dump.zt-bin"))
        );
        assert_eq!(AssociateDialog::new(PathBuf::from("/p/Blob")).pattern, "Blob", "no extension: the name");
    }
}
