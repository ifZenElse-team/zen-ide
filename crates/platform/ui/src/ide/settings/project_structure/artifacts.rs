use egui::Ui;
use zen_i18n::t;
use zen_workbench::artifacts::{Artifact, ArtifactKind, LibraryPlacement, jar_path};
use zen_workbench::java_run::{BuildTool, build_tool};

use super::super::widgets;
use super::{ProjectStructurePage, choice};
use crate::accessibility;
use crate::ide::IdePalette;

impl ProjectStructurePage {
    pub(super) fn add(&mut self, mut artifact: Artifact) {
        let base = artifact.name.clone();
        let mut count = 1;
        while self.artifacts.iter().any(|(_, kept)| kept.name == artifact.name) {
            count += 1;
            artifact.name = format!("{base} {count}");
        }
        self.artifacts.push((None, artifact));
        self.artifact = self.artifacts.len() - 1;
    }

    fn suggestion_label(&self, artifact: &Artifact) -> String {
        match artifact.kind {
            ArtifactKind::Build => t!("settings-project-structure-artifact-add-build", name = artifact.name.as_str()),
            ArtifactKind::Jar => t!("settings-project-structure-artifact-add-jar", name = artifact.name.as_str()),
        }
    }

    pub(super) fn artifacts_view(&mut self, ui: &mut Ui, p: &IdePalette) {
        widgets::group_title(ui, p, &t!("settings-project-structure-artifacts"), None);
        ui.label(egui::RichText::new(t!("settings-project-structure-artifacts-note")).color(p.text_secondary));
        ui.add_space(6.0);
        let mut added = None;
        let mut removed = false;
        ui.horizontal(|ui| {
            let add = t!("settings-project-structure-artifact-add");
            let menu = ui.add_enabled_ui(!self.suggestions.is_empty(), |ui| {
                ui.menu_button(add.clone(), |ui| {
                    for (index, suggestion) in self.suggestions.iter().enumerate() {
                        if ui.button(self.suggestion_label(suggestion)).clicked() {
                            added = Some(index);
                            ui.close();
                        }
                    }
                })
            });
            accessibility::name(&menu.inner.response, egui::Role::Button, &add);
            let remove = t!("settings-project-structure-artifact-remove");
            if ui.add_enabled(!self.artifacts.is_empty(), egui::Button::new(&remove)).clicked() {
                removed = true;
            }
        });
        if let Some(index) = added {
            let artifact = self.suggestions[index].clone();
            self.add(artifact);
        }
        if removed && self.artifact < self.artifacts.len() {
            self.artifacts.remove(self.artifact);
            self.artifact = self.artifact.min(self.artifacts.len().saturating_sub(1));
        }
        ui.add_space(4.0);
        if self.artifacts.is_empty() {
            let note = if self.suggestions.is_empty() {
                t!("settings-project-structure-no-artifacts")
            } else {
                t!("settings-project-structure-artifacts-offered")
            };
            ui.label(egui::RichText::new(note).color(p.text_secondary));
            return;
        }
        egui::Frame::new().stroke(egui::Stroke::new(1.0, p.button_border)).inner_margin(4).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for index in 0..self.artifacts.len() {
                let name = self.artifacts[index].1.name.clone();
                let response = ui.selectable_label(self.artifact == index, &name);
                accessibility::item(&response, egui::Role::ListBoxOption, self.artifact == index, &name);
                if response.clicked() {
                    self.artifact = index;
                }
            }
        });
        ui.add_space(8.0);
        self.artifact_form(ui, p);
    }

    fn artifact_form(&mut self, ui: &mut Ui, p: &IdePalette) {
        let root = self.project.as_ref().map(|project| project.root.clone()).unwrap_or_default();
        let modules: Vec<String> = self.modules.iter().map(|module| module.name.clone()).collect();
        let Some((_, artifact)) = self.artifacts.get_mut(self.artifact) else { return };
        widgets::text_field(ui, p, &t!("settings-project-structure-artifact-name"), None, &mut artifact.name);
        let kind = match artifact.kind {
            ArtifactKind::Jar => t!("settings-project-structure-artifact-type-jar"),
            ArtifactKind::Build => t!("settings-project-structure-artifact-type-build"),
        };
        Self::field(ui, p, &t!("settings-project-structure-artifact-type"), |ui| {
            ui.label(egui::RichText::new(kind).color(p.text));
        });
        match artifact.kind {
            ArtifactKind::Jar => {
                if !modules.is_empty() {
                    ui.label(egui::RichText::new(t!("settings-project-structure-artifact-modules")).color(p.text));
                    for module in &modules {
                        let mut on = artifact.modules.contains(module);
                        if widgets::checkbox(ui, p, &mut on, module, None).changed() {
                            if on {
                                artifact.modules.push(module.clone());
                            } else {
                                artifact.modules.retain(|kept| kept != module);
                            }
                        }
                    }
                    ui.add_space(4.0);
                }
                widgets::text_field(
                    ui,
                    p,
                    &t!("settings-project-structure-artifact-main-class"),
                    None,
                    &mut artifact.main_class,
                );
                ui.label(egui::RichText::new(t!("settings-project-structure-artifact-libraries")).color(p.text));
                for (placement, key) in [
                    (LibraryPlacement::Inside, "settings-project-structure-artifact-extract"),
                    (LibraryPlacement::NextTo, "settings-project-structure-artifact-copy"),
                ] {
                    let label = t!(key);
                    let response = ui.radio_value(&mut artifact.libraries, placement, &label);
                    accessibility::item(&response, egui::Role::RadioButton, artifact.libraries == placement, &label);
                }
                ui.add_space(4.0);
                widgets::text_field(
                    ui,
                    p,
                    &t!("settings-project-structure-artifact-output"),
                    None,
                    &mut artifact.output,
                );
                let jar = jar_path(&root, artifact);
                let shown = jar.strip_prefix(&root).unwrap_or(&jar).display().to_string();
                ui.label(
                    egui::RichText::new(t!("settings-project-structure-artifact-file", path = shown))
                        .color(p.text_secondary),
                );
            }
            ArtifactKind::Build => {
                if !modules.is_empty() {
                    let label = t!("settings-project-structure-module");
                    let options: Vec<(String, String)> =
                        modules.iter().map(|module| (module.clone(), module.clone())).collect();
                    let mut chosen = artifact.modules.first().cloned().or_else(|| modules.first().cloned());
                    Self::field(ui, p, &label, |ui| {
                        choice(ui, "project-structure-artifact-module", &label, &options, &mut chosen)
                    });
                    artifact.modules = chosen.into_iter().collect();
                }
                widgets::text_field(ui, p, &t!("settings-project-structure-artifact-task"), None, &mut artifact.task);
                let folder = self
                    .modules
                    .iter()
                    .find(|module| artifact.modules.first() == Some(&module.name))
                    .map_or_else(|| root.clone(), |module| module.root.clone());
                let tool = if build_tool(&folder) == BuildTool::Gradle { "gradle" } else { "mvn" };
                let line = format!("{tool} {}", artifact.task.trim());
                ui.label(
                    egui::RichText::new(t!("settings-project-structure-artifact-runs", line = line))
                        .color(p.text_secondary),
                );
            }
        }
    }
}
