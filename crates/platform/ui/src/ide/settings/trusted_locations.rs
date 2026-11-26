use std::path::PathBuf;

use egui::{Sense, Ui, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_workbench::trust::covers_home;

use super::Dispatch;
use super::external_tools::{glyph_down, glyph_up, tool_button};
use crate::accessibility;
use crate::ide::IdePalette;
use crate::ide::glyphs::{glyph_minus, glyph_plus};

#[derive(Default)]
pub(super) struct TrustedLocationsPage {
    original: Vec<PathBuf>,
    pub(super) draft: Vec<PathBuf>,
    selected: Option<usize>,
    message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Add,
    Remove,
    Up,
    Down,
}

#[derive(serde::Deserialize)]
struct Listed {
    locations: Vec<Location>,
}

#[derive(serde::Deserialize)]
struct Location {
    path: PathBuf,
}

impl TrustedLocationsPage {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch);
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("project.trustedLocations.get", json!({}))
            .and_then(|value| serde_json::from_value::<Listed>(value).map_err(|error| error.to_string()))
        {
            Ok(listed) => {
                self.draft = listed.locations.into_iter().map(|location| location.path).collect();
                self.original.clone_from(&self.draft);
            }
            Err(error) => self.message = Some(error),
        }
        self.selected = self.selected.filter(|index| *index < self.draft.len());
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.draft == self.original {
            return;
        }
        match dispatch("project.trustedLocations.set", json!({ "locations": self.draft })) {
            Ok(_) => self.message = None,
            Err(error) => {
                self.message = Some(error);
                return;
            }
        }
        self.load(dispatch);
    }

    pub(super) fn add(&mut self, folder: PathBuf) {
        match self.draft.iter().position(|kept| *kept == folder) {
            Some(index) => self.selected = Some(index),
            None => {
                self.draft.push(folder);
                self.selected = Some(self.draft.len() - 1);
            }
        }
    }

    fn act(&mut self, tool: Tool) {
        let selected = self.selected.filter(|index| *index < self.draft.len());
        match (tool, selected) {
            (Tool::Add, _) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(folder) =
                    rfd::FileDialog::new().set_title(t!("settings-trusted-locations-add")).pick_folder()
                {
                    self.add(std::fs::canonicalize(&folder).unwrap_or(folder));
                }
            }
            (Tool::Remove, Some(index)) => {
                self.draft.remove(index);
                self.selected = (!self.draft.is_empty()).then(|| index.min(self.draft.len() - 1));
            }
            (Tool::Up, Some(index)) if index > 0 => {
                self.draft.swap(index, index - 1);
                self.selected = Some(index - 1);
            }
            (Tool::Down, Some(index)) if index + 1 < self.draft.len() => {
                self.draft.swap(index, index + 1);
                self.selected = Some(index + 1);
            }
            _ => {}
        }
    }

    pub(super) fn warns(&self) -> bool {
        self.draft.iter().any(|path| covers_home(path))
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.add(
            egui::Label::new(egui::RichText::new(t!("settings-trusted-locations-about")).color(p.text_secondary))
                .wrap(),
        );
        ui.add_space(8.0);
        let selected = self.selected.filter(|index| *index < self.draft.len());
        let buttons: [(Tool, crate::ide::glyphs::Glyph, String, bool); 4] = [
            (Tool::Add, glyph_plus, t!("settings-trusted-locations-add"), true),
            (Tool::Remove, glyph_minus, t!("settings-trusted-locations-remove"), selected.is_some()),
            (Tool::Up, glyph_up, t!("settings-trusted-locations-up"), selected.is_some_and(|index| index > 0)),
            (
                Tool::Down,
                glyph_down,
                t!("settings-trusted-locations-down"),
                selected.is_some_and(|index| index + 1 < self.draft.len()),
            ),
        ];
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (tool, glyph, name, enabled) in buttons {
                if tool_button(ui, p, glyph, &name, enabled) {
                    chosen = Some(tool);
                }
            }
        });
        if let Some(tool) = chosen {
            self.act(tool);
        }
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).color(p.diagnostic_error));
        }
        ui.add_space(4.0);
        egui::Frame::new().fill(p.island).corner_radius(4).inner_margin(4).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(220.0);
            if self.draft.is_empty() {
                ui.add_space(90.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(t!("settings-trusted-locations-empty")).color(p.text_secondary));
                });
                return;
            }
            self.list(ui, p);
        });
        if self.warns() {
            ui.add_space(8.0);
            let warning = egui::RichText::new(t!("settings-trusted-locations-home")).color(p.diagnostic_warning);
            ui.add(egui::Label::new(warning).wrap());
        }
    }

    fn list(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        for index in 0..self.draft.len() {
            let text = self.draft[index].display().to_string();
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
            if self.selected == Some(index) {
                ui.painter().rect_filled(rect, 3, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 3, p.popup_hover);
            }
            ui.painter().text(
                rect.left_center() + vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &text,
                crate::theme::regular(13.0),
                p.text,
            );
            accessibility::button(&response, &text);
            if response.clicked() {
                self.selected = Some(index);
            }
        }
    }
}
