use egui::{Key, Modifiers, Sense, Ui, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::artifacts::{ArtifactsSection, buildable};

use super::{IdePalette, IdeView};
use crate::theme;
use zen_workbench::notifications::{Group, Kind, NewNotification};

pub(super) const OPEN: &str = "view.artifacts.build";

const ROW: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Build,
    Rebuild,
    Clean,
    Edit,
}

const ACTIONS: [Action; 4] = [Action::Build, Action::Rebuild, Action::Clean, Action::Edit];

impl Action {
    fn label(self) -> String {
        match self {
            Action::Build => t!("ide-artifacts-build"),
            Action::Rebuild => t!("ide-artifacts-rebuild"),
            Action::Clean => t!("ide-artifacts-clean"),
            Action::Edit => t!("ide-artifacts-edit"),
        }
    }

    fn command(self) -> Option<&'static str> {
        match self {
            Action::Build => Some("build.artifacts.build"),
            Action::Rebuild => Some("build.artifacts.rebuild"),
            Action::Clean => Some("build.artifacts.clean"),
            Action::Edit => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    label: String,
    names: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ArtifactsPopup {
    rows: Vec<Row>,
    selected: usize,
    actions: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Run { action: Action, names: Vec<String> },
    Close,
}

impl ArtifactsPopup {
    pub(super) fn new(section: &ArtifactsSection) -> Self {
        let listed = buildable(section);
        let mut rows: Vec<Row> = listed
            .iter()
            .map(|artifact| Row { label: artifact.name.clone(), names: vec![artifact.name.clone()] })
            .collect();
        if listed.len() > 1 {
            rows.insert(0, Row { label: t!("ide-artifacts-all"), names: Vec::new() });
        }
        Self { rows, selected: 0, actions: None }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    #[cfg(test)]
    pub(super) fn labels(&self) -> Vec<String> {
        match self.actions {
            Some(_) => ACTIONS.iter().map(|action| action.label()).collect(),
            None => self.rows.iter().map(|row| row.label.clone()).collect(),
        }
    }

    fn count(&self) -> usize {
        if self.actions.is_some() { ACTIONS.len() } else { self.rows.len() }
    }

    fn enter(&mut self, index: usize) -> Option<Choice> {
        match self.actions {
            None => {
                self.actions = Some(index);
                self.selected = 0;
                None
            }
            Some(row) => Some(Choice::Run { action: ACTIONS[index], names: self.rows[row].names.clone() }),
        }
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Option<Choice> {
        let count = self.count();
        let (up, down, enter, right, left, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::ArrowRight),
                input.consume_key(Modifiers::NONE, Key::ArrowLeft),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
        }
        if down {
            self.selected = (self.selected + 1) % count;
        }
        if enter || right && self.actions.is_none() {
            return self.enter(self.selected);
        }
        if escape || left {
            match self.actions.take() {
                Some(row) => self.selected = row,
                None if escape => return Some(Choice::Close),
                None => {}
            }
        }
        None
    }

    pub(super) fn show(&mut self, ui: &Ui, p: &IdePalette) -> Option<Choice> {
        let screen = ui.ctx().content_rect();
        let width = 320.0;
        let at = pos2(screen.center().x - width / 2.0, screen.top() + screen.height() * 0.2);
        let title = match self.actions {
            Some(row) => self.rows[row].label.clone(),
            None => t!("ide-artifacts-title"),
        };
        let labels: Vec<String> = match self.actions {
            Some(_) => ACTIONS.iter().map(|action| action.label()).collect(),
            None => self.rows.iter().map(|row| row.label.clone()).collect(),
        };
        let mut clicked = None;
        let area = egui::Area::new(ui.scope_id().with("build-artifacts"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                p.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                    ui.painter().text(
                        pos2(rect.left() + 10.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &title,
                        theme::semibold(13.0),
                        p.text_secondary,
                    );
                    for (index, label) in labels.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                        if index == self.selected {
                            ui.painter().rect_filled(rect, 4, p.selection);
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 4, p.popup_hover);
                        }
                        crate::accessibility::name_with(&response, egui::Role::ListBoxOption, || label.clone());
                        crate::accessibility::edit(&response, |node| node.set_selected(index == self.selected));
                        ui.painter().text(
                            pos2(rect.left() + 10.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            theme::regular(13.0),
                            p.text,
                        );
                        if self.actions.is_none() {
                            crate::icons::chevron_right(
                                ui.painter(),
                                egui::Rect::from_center_size(
                                    pos2(rect.right() - 12.0, rect.center().y),
                                    vec2(10.0, 10.0),
                                ),
                                p.text_secondary,
                            );
                        }
                        if response.clicked() {
                            clicked = Some(index);
                        }
                    }
                });
            });
        if let Some(index) = clicked {
            self.selected = index;
            return self.enter(index);
        }
        let outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| !area.response.rect.contains(pos))
        });
        outside.then_some(Choice::Close)
    }
}

impl IdeView {
    pub(super) fn open_artifacts_popup(&mut self, registry: &Registry, session: &mut Session) -> Result<(), String> {
        let section = registry
            .dispatch(session, "project.structure.artifacts", json!({}))
            .map_err(|error| error.to_string())
            .and_then(|value| serde_json::from_value::<ArtifactsSection>(value).map_err(|error| error.to_string()))?;
        let popup = ArtifactsPopup::new(&section);
        if popup.is_empty() {
            return Err(t!("ide-artifacts-none"));
        }
        self.artifacts_popup = Some(popup);
        Ok(())
    }

    pub(super) fn artifacts_popup(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(popup) = &mut self.artifacts_popup else { return };
        let p = self.palette;
        let choice = popup.keys(ui).or_else(|| popup.show(ui, &p));
        match choice {
            None => {}
            Some(Choice::Close) => self.artifacts_popup = None,
            Some(Choice::Run { action, names }) => {
                self.artifacts_popup = None;
                match action.command() {
                    None => {
                        self.settings.open_at(super::settings::PageId::ProjectStructure);
                        self.settings.show_project_structure_part("artifacts");
                    }
                    Some(command) => {
                        if let Err(error) = registry.dispatch(session, command, json!({ "names": names })) {
                            let message = match error {
                                zen_core::DispatchError::Failed { error, .. } => error.message,
                                other => other.to_string(),
                            };
                            self.notify(NewNotification::message(Group::General, Kind::Error, message));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use zen_workbench::artifacts::Artifact;

    use super::*;

    fn artifact(name: &str) -> Artifact {
        Artifact { name: name.to_owned(), ..Artifact::default() }
    }

    #[test]
    fn rows_offer_all_artifacts_first_and_each_opens_its_actions() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let section = ArtifactsSection {
            suggestions: vec![artifact("shop:mvn package"), artifact("shop:jar")],
            ..Default::default()
        };
        let mut popup = ArtifactsPopup::new(&section);
        assert_eq!(popup.labels(), [t!("ide-artifacts-all"), "shop:mvn package".into(), "shop:jar".into()]);
        assert_eq!(popup.enter(2), None);
        assert_eq!(popup.labels(), ACTIONS.map(Action::label));
        assert_eq!(popup.enter(1), Some(Choice::Run { action: Action::Rebuild, names: vec!["shop:jar".into()] }));
        let mut popup = ArtifactsPopup::new(&section);
        popup.enter(0);
        assert_eq!(popup.enter(0), Some(Choice::Run { action: Action::Build, names: Vec::new() }), "all of them");

        let kept = ArtifactsSection { artifacts: vec![artifact("app")], ..section };
        assert_eq!(ArtifactsPopup::new(&kept).labels(), ["app"], "kept artifacts replace the suggestions");
        assert!(ArtifactsPopup::new(&ArtifactsSection::default()).is_empty());
    }
}
