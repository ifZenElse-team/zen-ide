use egui::{Color32, Key, Ui};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::search_scopes::{ALL_PLACES, PROJECT_FILES, ScopeArea, ScopeSelection};

use super::IdePalette;
use super::scopes_dialog::ScopesDialog;
use crate::{accessibility, theme};

const RECENT_DIRECTORIES: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ScopeEntry {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) named: bool,
    pub(super) own: bool,
    pub(super) pattern: String,
    pub(super) error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ScopeChoices {
    pub(super) scopes: Vec<ScopeEntry>,
    pub(super) modules: Vec<String>,
    pub(super) selection: ScopeSelection,
}

impl ScopeChoices {
    pub(super) fn load(registry: &Registry, session: &mut Session, everywhere: bool) -> Self {
        let list =
            registry.dispatch(session, "search.scope.list", json!({ "everywhere": everywhere })).unwrap_or_default();
        let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        let scopes = list["scopes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|scope| ScopeEntry {
                id: text(&scope["id"]),
                name: text(&scope["name"]),
                named: scope["group"] == "named",
                own: scope["own"] == true,
                pattern: text(&scope["pattern"]),
                error: scope["error"].as_str().map(str::to_owned),
            })
            .collect();
        let modules = list["modules"].as_array().into_iter().flatten().map(|module| text(&module["name"])).collect();
        let selection = serde_json::from_value(list["selection"].clone()).unwrap_or_default();
        Self { scopes, modules, selection }
    }

    pub(super) fn name_of(&self, id: &str) -> String {
        self.scopes.iter().find(|scope| scope.id == id).map_or_else(|| id.to_owned(), |scope| scope.name.clone())
    }

    pub(super) fn select(&self, registry: &Registry, session: &mut Session) {
        let _ = registry.dispatch(session, "search.scope.select", json!(self.selection));
    }
}

pub(super) fn scope_combo(ui: &mut Ui, salt: &str, choices: &ScopeChoices, chosen: &mut String, width: f32) -> bool {
    let before = chosen.clone();
    let response = egui::ComboBox::from_id_salt(salt)
        .selected_text(egui::RichText::new(choices.name_of(chosen)).font(theme::regular(12.5)))
        .width(width)
        .truncate()
        .height(420.0)
        .show_ui(ui, |ui| {
            let mut named = false;
            for scope in &choices.scopes {
                if scope.named && !named {
                    named = true;
                    ui.separator();
                }
                let text = egui::RichText::new(&scope.name).font(theme::regular(12.5));
                let item = ui.add_enabled(scope.error.is_none(), egui::Button::selectable(*chosen == scope.id, text));
                if let Some(error) = &scope.error {
                    item.clone().on_disabled_hover_text(error);
                }
                if item.clicked() {
                    *chosen = scope.id.clone();
                }
            }
        });
    accessibility::labelled(&response.response, &t!("ide-scope-scope"));
    *chosen != before
}

#[derive(Default)]
pub(super) struct ScopeRow {
    choices: Option<ScopeChoices>,
    folder: Option<String>,
    recent: Vec<String>,
    dialog: Option<ScopesDialog>,
}

impl ScopeRow {
    pub(super) fn reopen(&mut self, folder: Option<String>) {
        self.choices = None;
        self.folder = folder;
    }

    pub(super) fn choices(&mut self, registry: &Registry, session: &mut Session) -> &mut ScopeChoices {
        if self.choices.is_none() {
            let mut choices = ScopeChoices::load(registry, session, false);
            if choices.selection.area == ScopeArea::Directory
                && let Some(folder) = self.folder.take()
            {
                choices.selection.directory = folder;
                choices.select(registry, session);
            }
            self.choices = Some(choices);
        }
        self.choices.get_or_insert_with(ScopeChoices::default)
    }

    pub(super) fn params(&mut self, registry: &Registry, session: &mut Session) -> Value {
        self.choices(registry, session).selection.search_params()
    }

    pub(super) fn busy(&self, ctx: &egui::Context) -> bool {
        self.dialog.is_some() || egui::Popup::is_any_open(ctx)
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        folder: Option<String>,
        active: bool,
    ) {
        let mut edit = false;
        let mut changed = false;
        let recent = self.recent.clone();
        let choices = self.choices(registry, session);
        let mut remember = false;
        let control = egui::Modifiers::CTRL | egui::Modifiers::ALT;
        let toggle = active && ui.input_mut(|input| input.consume_key(control, Key::Y));
        if toggle && choices.selection.area == ScopeArea::Directory {
            choices.selection.recursive = !choices.selection.recursive;
            changed = true;
        }
        let areas = [
            (ScopeArea::Project, t!("ide-scope-project"), Key::P, "⌃⌥P"),
            (ScopeArea::Module, t!("ide-scope-module"), Key::M, "⌃⌥M"),
            (ScopeArea::Directory, t!("ide-scope-directory"), Key::D, "⌃⌥D"),
            (ScopeArea::Scope, t!("ide-scope-scope"), Key::S, "⌃⌥S"),
        ];
        let pressed =
            areas.iter().find(|(_, _, key, _)| active && ui.input_mut(|input| input.consume_key(control, *key)));
        let mut pick = pressed.map(|(area, ..)| *area).filter(|area| *area != choices.selection.area);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (area, label, key, shortcut) in &areas {
                let area = *area;
                let chosen = choices.selection.area == area;
                let color = if chosen { palette.text } else { palette.text_secondary };
                let button = egui::Button::new(mnemonic(label, key.name(), color))
                    .fill(if chosen { palette.selection } else { Color32::TRANSPARENT })
                    .corner_radius(5);
                let response = ui.add(button);
                super::hint(ui, &response, label, Some(shortcut), super::Side::Below);
                accessibility::pressed(&response, chosen);
                if response.clicked() && !chosen {
                    pick = Some(area);
                }
            }
            if let Some(area) = pick {
                choices.selection.area = area;
                if area == ScopeArea::Directory && choices.selection.directory.is_empty() {
                    choices.selection.directory = folder.clone().unwrap_or_default();
                }
                if area == ScopeArea::Module && choices.selection.module.is_none() {
                    choices.selection.module = choices.modules.first().cloned();
                }
                changed = true;
            }
            ui.add_space(8.0);
            match choices.selection.area {
                ScopeArea::Project => {}
                ScopeArea::Module => {
                    let shown = choices.selection.module.clone().unwrap_or_else(|| t!("ide-scope-module-none"));
                    let before = choices.selection.module.clone();
                    let modules = choices.modules.clone();
                    let response = egui::ComboBox::from_id_salt("fif-module")
                        .selected_text(egui::RichText::new(shown).font(theme::regular(12.5)))
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            for module in &modules {
                                let text = egui::RichText::new(module).font(theme::regular(12.5));
                                ui.selectable_value(&mut choices.selection.module, Some(module.clone()), text);
                            }
                        });
                    accessibility::labelled(&response.response, &t!("ide-scope-module"));
                    changed |= choices.selection.module != before;
                }
                ScopeArea::Directory => {
                    let hint = t!("ide-scope-directory");
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut choices.selection.directory)
                            .font(theme::regular(12.5))
                            .hint_text(t!("ide-fif-project-root"))
                            .desired_width(260.0),
                    );
                    accessibility::labelled(&field, &hint);
                    changed |= field.changed();
                    remember |= field.lost_focus();
                    let menu = ui.menu_button("▾", |ui| {
                        if recent.is_empty() {
                            ui.label(
                                egui::RichText::new(t!("ide-scope-recent-directories")).color(palette.text_secondary),
                            );
                        }
                        for directory in &recent {
                            if ui.button(directory).clicked() {
                                choices.selection.directory = directory.clone();
                                changed = true;
                                remember = true;
                                ui.close();
                            }
                        }
                    });
                    accessibility::labelled(&menu.response, &t!("ide-scope-recent-directories"));
                    let browse = ui.button("…");
                    super::hint(ui, &browse, &t!("ide-scope-browse"), None, super::Side::Below);
                    accessibility::labelled(&browse, &t!("ide-scope-browse"));
                    if browse.clicked() {
                        let root =
                            session.state::<zen_workbench::Workbench>().and_then(|w| w.project()).map(|p| p.to_owned());
                        if let Some(chosen) = browse_folder(root.as_deref(), &choices.selection.directory) {
                            choices.selection.directory = chosen;
                            changed = true;
                            remember = true;
                        }
                    }
                    let check = ui.checkbox(
                        &mut choices.selection.recursive,
                        egui::RichText::new(t!("ide-scope-recursive")).font(theme::regular(12.5)),
                    );
                    super::hint(ui, &check, &t!("ide-scope-recursive"), Some("⌃⌥Y"), super::Side::Below);
                    changed |= check.changed();
                }
                ScopeArea::Scope => {
                    let snapshot = choices.clone();
                    changed |= scope_combo(ui, "fif-scope", &snapshot, &mut choices.selection.scope, 260.0);
                    let more = ui.button("…");
                    super::hint(ui, &more, &t!("ide-scope-edit"), None, super::Side::Below);
                    accessibility::labelled(&more, &t!("ide-scope-edit"));
                    edit = more.clicked();
                }
            }
        });
        if changed {
            choices.select(registry, session);
        }
        let directory = choices.selection.directory.trim_matches('/').to_owned();
        if remember && choices.selection.area == ScopeArea::Directory && !directory.is_empty() {
            self.recent.retain(|other| *other != directory);
            self.recent.insert(0, directory);
            self.recent.truncate(RECENT_DIRECTORIES);
        }
        if edit {
            let current = self.choices.as_ref().map(|choices| choices.selection.scope.clone()).unwrap_or_default();
            self.dialog = Some(ScopesDialog::new(current));
        }
        if let Some(dialog) = &mut self.dialog {
            let choices = self.choices.clone().unwrap_or_default();
            match dialog.show(ui, registry, session, palette, &choices) {
                None => {}
                Some(chosen) => {
                    self.dialog = None;
                    let mut reread = ScopeChoices::load(registry, session, false);
                    if let Some(id) = chosen.filter(|id| reread.scopes.iter().any(|scope| scope.id == *id)) {
                        reread.selection.area = ScopeArea::Scope;
                        reread.selection.scope = id;
                        reread.select(registry, session);
                    }
                    self.choices = Some(reread);
                }
            }
        }
    }
}

fn mnemonic(label: &str, letter: &str, color: Color32) -> egui::text::LayoutJob {
    let plain = egui::TextFormat::simple(theme::regular(12.5), color);
    let marked = egui::TextFormat { underline: egui::Stroke::new(1.0, color), ..plain.clone() };
    let mut job = egui::text::LayoutJob::default();
    let at = label.char_indices().find(|(_, c)| c.to_string().eq_ignore_ascii_case(letter));
    match at {
        Some((byte, c)) => {
            job.append(&label[..byte], 0.0, plain.clone());
            job.append(&label[byte..byte + c.len_utf8()], 0.0, marked);
            job.append(&label[byte + c.len_utf8()..], 0.0, plain);
        }
        None => job.append(label, 0.0, plain),
    }
    job
}

pub(super) fn includes_non_project(scope: &str) -> bool {
    scope == ALL_PLACES
}

pub(super) fn non_project_scope(include: bool) -> &'static str {
    if include { ALL_PLACES } else { PROJECT_FILES }
}

fn browse_folder(root: Option<&std::path::Path>, current: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut dialog = rfd::FileDialog::new().set_title(t!("ide-scope-browse"));
        if let Some(root) = root {
            let start = root.join(current);
            dialog = dialog.set_directory(if start.is_dir() { start } else { root.to_owned() });
        }
        let chosen = dialog.pick_folder()?;
        let relative = root.and_then(|root| chosen.strip_prefix(root).ok())?;
        Some(relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (root, current);
        None
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Pos2, Rect, vec2};

    use super::*;

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        row: ScopeRow,
        active: bool,
        _dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            zen_i18n::register(zen_workbench::LOCALES);
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join("internal/cart")).unwrap();
            std::fs::write(dir.path().join("internal/cart/cart.go"), "package cart\n").unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            let ctx = egui::Context::default();
            theme::install(&ctx);
            Self { ctx, registry, session, row: ScopeRow::default(), active: true, _dir: dir }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                ..Default::default()
            };
            let Self { ctx, registry, session, row, active, .. } = self;
            let mut output = ctx.run_ui(input, |ui| {
                row.show(ui, registry, session, &IdePalette::zen_dark(), Some("internal".to_owned()), *active);
            });
            output.textures_delta.clear();
        }

        fn remembered(&mut self) -> Value {
            self.registry.dispatch(&mut self.session, "search.scope.list", json!({})).unwrap()["selection"].clone()
        }
    }

    #[test]
    fn the_search_follows_the_remembered_row_and_the_tree_folder() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        let params = harness.row.params(&harness.registry, &mut harness.session);
        assert_eq!(params, json!({ "scope": "projectFiles", "directory": "", "recursive": true }));
        let chosen = json!({ "area": "scope", "scope": "tests" });
        harness.registry.dispatch(&mut harness.session, "search.scope.select", chosen).unwrap();
        harness.row.reopen(Some("internal".to_owned()));
        assert_eq!(harness.row.params(&harness.registry, &mut harness.session)["scope"], "tests");
        let chosen = json!({ "area": "directory", "directory": "cmd" });
        harness.registry.dispatch(&mut harness.session, "search.scope.select", chosen).unwrap();
        harness.row.reopen(Some("internal/cart".to_owned()));
        let params = harness.row.params(&harness.registry, &mut harness.session);
        assert_eq!(params["directory"], "internal/cart", "the folder chosen in the Project tool window");
        assert_eq!(harness.remembered()["directory"], "internal/cart");
    }

    #[test]
    fn control_option_y_searches_the_subfolders_or_not() {
        let mut harness = Harness::new();
        let chosen = json!({ "area": "directory", "directory": "internal" });
        harness.registry.dispatch(&mut harness.session, "search.scope.select", chosen).unwrap();
        harness.frame(Vec::new());
        let key = |pressed| Event::Key {
            key: Key::Y,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::CTRL | egui::Modifiers::ALT,
        };
        harness.frame(vec![key(true), key(false)]);
        assert_eq!(harness.remembered()["recursive"], false);
        assert_eq!(harness.row.params(&harness.registry, &mut harness.session)["recursive"], false);
        harness.frame(vec![key(true), key(false)]);
        assert_eq!(harness.remembered()["recursive"], true);
    }

    #[test]
    fn control_option_letters_choose_the_area() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        let press = |key| {
            let event = |pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::CTRL | egui::Modifiers::ALT,
            };
            vec![event(true), event(false)]
        };
        for (key, area) in [(Key::D, "directory"), (Key::M, "module"), (Key::S, "scope"), (Key::P, "project")] {
            harness.frame(press(key));
            assert_eq!(harness.remembered()["area"], area);
        }
        assert_eq!(harness.remembered()["directory"], "internal", "Directory starts at the tree's folder");
        harness.active = false;
        harness.frame(press(Key::D));
        assert_eq!(harness.remembered()["area"], "project", "a window left for the editor leaves the keys alone");
    }

    #[test]
    fn the_mnemonic_letter_is_underlined() {
        let job = mnemonic("In Project", "P", Color32::WHITE);
        let underlined: Vec<&str> = job
            .sections
            .iter()
            .filter(|section| section.format.underline.width > 0.0)
            .map(|section| &job.text[section.byte_range.start.0..section.byte_range.end.0])
            .collect();
        assert_eq!(underlined, ["P"]);
        assert!(mnemonic("Весь проект", "P", Color32::WHITE).sections.iter().all(|s| s.format.underline.width == 0.0));
    }

    #[test]
    fn the_list_is_read_with_the_named_scopes() {
        let mut harness = Harness::new();
        let saved = json!({ "name": "Cart", "pattern": "file:internal/cart//*" });
        harness.registry.dispatch(&mut harness.session, "search.scope.save", saved).unwrap();
        let choices = ScopeChoices::load(&harness.registry, &mut harness.session, false);
        let cart = choices.scopes.iter().find(|scope| scope.id == "named:Cart").unwrap();
        assert!(cart.named && cart.own && cart.error.is_none());
        assert_eq!(choices.name_of("projectFiles"), "Project Files");
        assert!(choices.scopes.iter().any(|scope| scope.id == "currentFile"));
        let everywhere = ScopeChoices::load(&harness.registry, &mut harness.session, true);
        assert!(!everywhere.scopes.iter().any(|scope| scope.id == "currentFile"));
    }

    #[test]
    fn the_non_project_check_box_is_all_places() {
        assert!(includes_non_project(non_project_scope(true)));
        assert!(!includes_non_project(non_project_scope(false)));
        assert!(!includes_non_project("projectAndLibraries"));
    }
}
