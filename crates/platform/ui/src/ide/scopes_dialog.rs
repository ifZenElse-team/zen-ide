use egui::{Sense, Ui, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::search_scopes::NAMED_PREFIX;

use super::scope_row::{ScopeChoices, ScopeEntry};
use super::{IdePalette, dialog_frame};
use crate::{accessibility, theme};

#[derive(Default)]
pub(super) struct ScopesDialog {
    initial: String,
    selected: Option<String>,
    name: String,
    pattern: String,
    previous: Option<String>,
    own: bool,
    checked: Option<(String, Result<usize, String>)>,
    error: Option<String>,
}

impl ScopesDialog {
    pub(super) fn new(id: String) -> Self {
        Self { initial: id, ..Self::default() }
    }

    fn pick(&mut self, scope: &ScopeEntry) {
        self.selected = Some(scope.id.clone());
        self.name = scope.name.clone();
        self.pattern = scope.pattern.clone();
        self.previous = scope.own.then(|| scope.name.clone());
        self.own = scope.own;
        self.error = None;
    }

    fn check(&mut self, registry: &Registry, session: &mut Session) -> Result<usize, String> {
        if let Some((pattern, result)) = &self.checked
            && *pattern == self.pattern
        {
            return result.clone();
        }
        let found = registry.dispatch(session, "search.scope.files", json!({ "pattern": self.pattern, "limit": 0 }));
        let result =
            found.map(|found| found["total"].as_u64().unwrap_or(0) as usize).map_err(|error| error.to_string());
        self.checked = Some((self.pattern.clone(), result.clone()));
        result
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        choices: &ScopeChoices,
    ) -> Option<Option<String>> {
        let named: Vec<&ScopeEntry> = choices.scopes.iter().filter(|scope| scope.named).collect();
        if self.selected.is_none() {
            let first = named.iter().find(|scope| scope.id == self.initial).or(named.first()).copied();
            match first {
                Some(scope) => self.pick(scope),
                None => self.selected = Some(String::new()),
            }
        }
        let checked = (!self.pattern.trim().is_empty()).then(|| self.check(registry, session));
        let (mut close, mut saved) = (ui.input(|input| input.key_pressed(egui::Key::Escape)), None);
        let screen = ui.ctx().content_rect();
        let size = vec2(620.0, 380.0);
        egui::Area::new(ui.scope_id().with("scopes-dialog"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos2(screen.center().x - size.x / 2.0, screen.center().y - size.y / 2.0))
            .show(ui.ctx(), |ui| {
                dialog_frame(palette).show(ui, |ui| {
                    ui.set_width(size.x - 40.0);
                    ui.label(
                        egui::RichText::new(t!("ide-scopes-title")).font(theme::semibold(14.0)).color(palette.text),
                    );
                    ui.add_space(10.0);
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(180.0);
                            ui.horizontal(|ui| {
                                let add = ui.button("+");
                                super::hint(ui, &add, &t!("ide-scopes-add"), None, super::Side::Below);
                                if add.clicked() {
                                    self.selected = Some(String::new());
                                    self.name = t!("ide-scopes-new");
                                    self.pattern.clear();
                                    self.previous = None;
                                    self.own = true;
                                }
                                let delete =
                                    ui.add_enabled(self.own && self.previous.is_some(), egui::Button::new("−"));
                                super::hint(ui, &delete, &t!("ide-scopes-delete"), None, super::Side::Below);
                                if delete.clicked() {
                                    let name = self.previous.clone().unwrap_or_default();
                                    match registry.dispatch(session, "search.scope.delete", json!({ "name": name })) {
                                        Ok(_) => saved = Some(None),
                                        Err(error) => self.error = Some(error.to_string()),
                                    }
                                }
                            });
                            ui.add_space(6.0);
                            egui::ScrollArea::vertical().id_salt("scopes-list").max_height(260.0).show(ui, |ui| {
                                for scope in &named {
                                    let chosen = self.selected.as_deref() == Some(scope.id.as_str());
                                    let text = egui::RichText::new(&scope.name).font(theme::regular(12.5));
                                    let item = ui.add(egui::Button::selectable(chosen, text));
                                    accessibility::item(&item, egui::Role::ListBoxOption, chosen, &scope.name);
                                    if item.clicked() {
                                        self.pick(scope);
                                    }
                                }
                            });
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            let label =
                                ui.label(egui::RichText::new(t!("ide-scopes-name")).color(palette.text_secondary));
                            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY))
                                .labelled_by(label.id);
                            ui.add_space(6.0);
                            let label =
                                ui.label(egui::RichText::new(t!("ide-scopes-pattern")).color(palette.text_secondary));
                            ui.add(
                                egui::TextEdit::multiline(&mut self.pattern)
                                    .font(theme::regular(12.5))
                                    .hint_text(t!("ide-scopes-pattern-hint"))
                                    .desired_rows(4)
                                    .desired_width(f32::INFINITY),
                            )
                            .labelled_by(label.id);
                            ui.add_space(4.0);
                            match &checked {
                                Some(Ok(count)) => {
                                    ui.label(
                                        egui::RichText::new(t!("ide-scopes-files", count = *count))
                                            .color(palette.text_secondary),
                                    );
                                }
                                Some(Err(error)) => {
                                    ui.label(egui::RichText::new(error).color(palette.excluded));
                                }
                                None => {}
                            }
                            if !self.own && self.selected.as_deref().is_some_and(|id| !id.is_empty()) {
                                ui.label(egui::RichText::new(t!("ide-scopes-read-only")).color(palette.text_secondary));
                            }
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(t!("ide-scopes-help"))
                                    .font(theme::regular(11.5))
                                    .color(palette.text_secondary),
                            );
                            if let Some(error) = &self.error {
                                ui.label(egui::RichText::new(error).color(palette.excluded));
                            }
                        });
                    });
                    ui.add_space(10.0);
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
                    let mut buttons = ui.new_child(
                        egui::UiBuilder::new().max_rect(row).layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    if buttons.button(t!("ide-scopes-close")).clicked() {
                        close = true;
                    }
                    let can_save = matches!(checked, Some(Ok(_))) && !self.name.trim().is_empty();
                    let save = egui::Button::new(egui::RichText::new(t!("ide-scopes-save")).color(palette.accent_text))
                        .fill(palette.accent);
                    if buttons.add_enabled(can_save, save).clicked() {
                        let params =
                            json!({ "name": self.name.trim(), "pattern": self.pattern, "previous": self.previous });
                        match registry.dispatch(session, "search.scope.save", params) {
                            Ok(_) => saved = Some(Some(format!("{NAMED_PREFIX}{}", self.name.trim()))),
                            Err(error) => self.error = Some(error.to_string()),
                        }
                    }
                });
            });
        saved.or(close.then_some(None))
    }
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect};

    use super::*;

    #[test]
    fn the_window_opens_on_the_scope_chosen_and_counts_its_files() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "a\n").unwrap();
        std::fs::write(dir.path().join("b.go"), "package b\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        while registry.dispatch(&mut session, "search.scope.files", json!({})).unwrap()["total"] != 2 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for (name, pattern) in [("Docs", "file:*.md"), ("Go", "file:*.go")] {
            registry.dispatch(&mut session, "search.scope.save", json!({ "name": name, "pattern": pattern })).unwrap();
        }
        let choices = ScopeChoices::load(&registry, &mut session, false);
        let mut dialog = ScopesDialog::new("named:Go".to_owned());
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
            ..Default::default()
        };
        let mut answer = None;
        let mut output = ctx.run_ui(input, |ui| {
            answer = dialog.show(ui, &registry, &mut session, &IdePalette::zen_dark(), &choices);
        });
        output.textures_delta.clear();
        assert_eq!(answer, None, "it stays open");
        assert_eq!((dialog.name.as_str(), dialog.pattern.as_str(), dialog.own), ("Go", "file:*.go", true));
        assert_eq!(dialog.checked, Some(("file:*.go".to_owned(), Ok(1))));
        dialog.pattern = "file:*.go&&".to_owned();
        assert!(dialog.check(&registry, &mut session).unwrap_err().contains("column"));
    }
}
