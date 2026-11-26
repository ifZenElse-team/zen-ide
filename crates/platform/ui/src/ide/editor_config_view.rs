use std::path::{Path, PathBuf};

use egui::{Color32, Key, Painter, Rect, Stroke, Ui, pos2, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::editor_config::{EditorConfigFile, Stubs};

use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

#[derive(Debug)]
pub(super) struct NewEditorConfigDialog {
    folder: PathBuf,
    stubs: Stubs,
    error: Option<String>,
}

#[derive(Debug)]
pub(super) struct EditorConfigFiles {
    files: Vec<EditorConfigFile>,
}

enum Choice {
    Ok,
    Cancel,
}

pub(super) fn mark(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let page = rect.shrink2(vec2(rect.width() * 0.16, rect.height() * 0.08));
    painter.rect_stroke(page, 2.0, stroke, egui::StrokeKind::Inside);
    let left = page.left() + page.width() * 0.24;
    for (row, length) in [(0.3, 0.52), (0.5, 0.36), (0.7, 0.44)] {
        let y = page.top() + page.height() * row;
        painter.line_segment([pos2(left, y), pos2(left + page.width() * length, y)], stroke);
    }
}

fn text(text: String, color: Color32) -> egui::RichText {
    egui::RichText::new(text).font(theme::regular(13.0)).color(color)
}

fn buttons(ui: &mut Ui, p: &IdePalette, ok: Option<String>) -> Option<Choice> {
    let mut choice = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(ok) = ok {
            let ok =
                egui::Button::new(text(ok, p.accent_text)).min_size(vec2(84.0, 30.0)).corner_radius(6).fill(p.accent);
            if ui.add(ok).clicked() {
                choice = Some(Choice::Ok);
            }
        }
        let cancel = egui::Button::new(text(t!("settings-cancel"), p.text))
            .min_size(vec2(84.0, 30.0))
            .corner_radius(6)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, p.button_border));
        if ui.add(cancel).clicked() {
            choice = Some(Choice::Cancel);
        }
    });
    choice
}

fn relative(root: Option<&Path>, path: &Path) -> String {
    match root.and_then(|root| path.strip_prefix(root).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => ".".to_owned(),
        Some(rest) => rest.display().to_string(),
        None => path.display().to_string(),
    }
}

fn failure(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

impl IdeView {
    pub(super) fn open_new_editor_config(&mut self, folder: PathBuf) {
        self.new_editor_config = Some(NewEditorConfigDialog { folder, stubs: Stubs::default(), error: None });
    }

    pub(super) fn new_file_folder(&self, session: &Session) -> Option<PathBuf> {
        let root = session.state::<Workbench>().and_then(Workbench::project)?.to_owned();
        let selected = self
            .tree_focused
            .then(|| self.selection.items(&self.visible_rows).last().cloned())
            .flatten()
            .filter(|path| path.starts_with(&root))
            .map(|path| super::folder_of(&path));
        let active = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .and_then(|document| document.path().parent().map(Path::to_owned))
            .filter(|folder| folder.starts_with(&root));
        Some(selected.or(active).unwrap_or(root))
    }

    pub(super) fn show_editor_config_files(
        &mut self,
        registry: &Registry,
        session: &mut Session,
    ) -> Result<(), zen_core::DispatchError> {
        let listed = registry.dispatch(session, "project.editorConfig.list", serde_json::json!({}))?;
        let files: Vec<EditorConfigFile> = listed
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|file| {
                Some(EditorConfigFile {
                    path: PathBuf::from(file["path"].as_str()?),
                    relative: file["relative"].as_str()?.to_owned(),
                    root: file["root"].as_bool().unwrap_or(false),
                    problems: file["problems"].as_u64().unwrap_or(0) as usize,
                })
            })
            .collect();
        self.editor_config_files = Some(EditorConfigFiles { files });
        Ok(())
    }

    pub(super) fn disable_editor_config(&mut self, session: &mut Session) {
        let _ = session.settings_mut().set(zen_workbench::editor_config::ENABLED, false);
        let mut settings = self.overview.settings.clone();
        settings.editor_config = false;
        self.overview.settings = settings.clone();
        self.requests.push(super::app::Request::Settings(Box::new(settings)));
    }

    pub(super) fn new_editor_config_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &mut self.new_editor_config else { return };
        let p = self.palette;
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let folder = relative(root.as_deref(), &dialog.folder);
        let mut choice = None;
        let modal =
            egui::Modal::new(ui.scope_id().with("new-editorconfig")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(
                    egui::RichText::new(t!("ide-editorconfig-new-title")).font(theme::semibold(15.0)).color(p.text),
                );
                ui.label(text(t!("ide-editorconfig-new-folder", folder = folder.as_str()), p.text_secondary));
                ui.label(text(t!("ide-editorconfig-new-stubs"), p.text));
                ui.checkbox(&mut dialog.stubs.root, text(t!("ide-editorconfig-stub-root"), p.text));
                ui.checkbox(&mut dialog.stubs.standard, text(t!("ide-editorconfig-stub-standard"), p.text));
                ui.checkbox(&mut dialog.stubs.intellij, text(t!("ide-editorconfig-stub-intellij"), p.text));
                if let Some(error) = &dialog.error {
                    ui.add(egui::Label::new(text(error.clone(), p.error_text)).wrap());
                }
                ui.add_space(8.0);
                choice = buttons(ui, &p, Some(t!("ide-launcher-ok")));
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                    choice = Some(Choice::Ok);
                }
            });
        match choice.or_else(|| modal.should_close().then_some(Choice::Cancel)) {
            Some(Choice::Cancel) => self.new_editor_config = None,
            Some(Choice::Ok) => {
                let params = serde_json::json!({ "folder": dialog.folder, "stubs": dialog.stubs });
                match registry.dispatch(session, "project.editorConfig.create", params) {
                    Ok(_) => self.new_editor_config = None,
                    Err(error) => dialog.error = Some(failure(error)),
                }
            }
            None => {}
        }
    }

    pub(super) fn editor_config_files_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(shown) = &self.editor_config_files else { return };
        let p = self.palette;
        let mut chosen = None;
        let mut choice = None;
        let modal =
            egui::Modal::new(ui.scope_id().with("editorconfig-files")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(
                    egui::RichText::new(t!("ide-editorconfig-files-title")).font(theme::semibold(15.0)).color(p.text),
                );
                if shown.files.is_empty() {
                    ui.label(text(t!("ide-editorconfig-files-none"), p.text_secondary));
                }
                for file in &shown.files {
                    let mut label = file.relative.clone();
                    if file.root {
                        label = t!("ide-editorconfig-files-root", file = label);
                    }
                    if file.problems > 0 {
                        label = t!("ide-editorconfig-files-problems", file = label, count = file.problems);
                    }
                    let row =
                        egui::Button::new(text(label, p.text)).fill(Color32::TRANSPARENT).min_size(vec2(400.0, 24.0));
                    if ui.add(row).clicked() {
                        chosen = Some(file.path.clone());
                    }
                }
                ui.add_space(8.0);
                choice = buttons(ui, &p, None);
            });
        if chosen.is_some() || choice.is_some() || modal.should_close() {
            self.editor_config_files = None;
        }
        if let Some(path) = chosen {
            self.dispatch(ui, registry, session, "file.document.open", serde_json::json!({ "path": path }));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use egui::accesskit::Role;
    use serde_json::json;
    use zen_i18n::t;

    use crate::ide::{IdeProduct, IdeView, keymap_page};

    struct Harness {
        registry: zen_core::Registry,
        session: zen_core::Session,
        view: IdeView,
        screen: crate::offscreen::Offscreen,
        project: tempfile::TempDir,
        snapshots: Option<PathBuf>,
        language: &'static str,
    }

    impl Harness {
        fn new(language: &'static str) -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            zen_i18n::register(zen_workbench::LOCALES);
            zen_i18n::set_language(Some(language));
            let project = tempfile::tempdir().unwrap();
            for (name, text) in [
                (
                    ".editorconfig",
                    "root = true\n\n[*.txt]\nindent_style = space\nindent_size = 3\n# saved as is\ntrim_trailing_whitespace = true\nmy_option = 1\n",
                ),
                ("a.txt", "one\n   two\n"),
                ("sub/b.txt", "b\n"),
            ] {
                let path = project.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            let mut registry = zen_core::Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = zen_core::Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
            let product = IdeProduct {
                folding_options: Vec::new(),
                name: "Test",
                id: "test",
                version: "0.0.0",
                highlighters: Vec::new(),
                plugins: None,
                mcp_port: 0,
                modules: Vec::new(),
                areas: Vec::new(),
                settings_pages: Vec::new(),
                code_styles: Vec::new(),
                debugger: None,
                project_models: Vec::new(),
                outliners: Vec::new(),
                run_providers: Vec::new(),
                language_servers: Vec::new(),
                templates: Vec::new(),
                inspectors: Vec::new(),
            };
            let view = IdeView::new(std::sync::Arc::new(product), crate::plugins::Plugins::new(None));
            let screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 560.0), 1.0);
            let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
            Self { registry, session, view, screen, project, snapshots, language }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.project.path().join(name)
        }

        fn run(&mut self, id: &str, params: serde_json::Value) -> serde_json::Value {
            self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
        }

        fn frame(&mut self) -> crate::offscreen::Frame {
            let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
            self.screen.render(|ui| view.show(ui, registry, session))
        }

        fn snapshot(&mut self, name: &str) {
            self.frame();
            let image = self.frame();
            if let Some(dir) = &self.snapshots {
                crate::offscreen::write_png(&dir.join(format!("{name}-{}.png", self.language)), &image).unwrap();
            }
        }

        fn find(&mut self, role: Role, name: &str) -> Option<egui::Pos2> {
            let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
            let tree = self.screen.accessibility_tree(|ui| view.show(ui, registry, session));
            let node = crate::accessibility::testing::find(&tree, role, name)?;
            let bounds = node.bounds()?;
            Some(egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32))
        }

        fn click(&mut self, role: Role, name: &str) {
            let at = self.find(role, name).unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
                self.screen.step(events, |ui| view.show(ui, registry, session));
            }
        }

        fn choose(&mut self, widget: &str, item: &str) {
            for _ in 0..10 {
                self.click(Role::Button, widget);
                for _ in 0..30 {
                    if self.find(Role::Button, item).is_some() {
                        self.click(Role::Button, item);
                        return;
                    }
                }
                self.frame();
            }
            panic!("{widget} never opened its menu with {item}");
        }

        fn active(&self) -> PathBuf {
            let workbench = self.session.state::<zen_workbench::Workbench>().unwrap();
            workbench.active_document().unwrap().path().to_owned()
        }

        fn widget(&self) -> String {
            t!("ide-status-indent-editorconfig", indent = t!("ide-status-spaces", count = 3))
        }
    }

    #[test]
    fn the_indentation_widget_shows_editorconfig_and_its_menu_works() {
        let languages: &[&'static str] =
            if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { &["en", "ru"] } else { &["en"] };
        for language in languages {
            let mut h = Harness::new(language);
            let a = h.path("a.txt");
            h.run("file.document.open", json!({ "path": a }));
            h.snapshot("editorconfig-status-mark");
            let widget = h.widget();
            if h.snapshots.is_some() {
                h.click(Role::Button, &widget);
                h.snapshot("editorconfig-status-menu");
                h.click(Role::Button, &keymap_page::action_title("view.editorConfig.showFiles"));
            } else {
                h.choose(&widget, &keymap_page::action_title("view.editorConfig.showFiles"));
            }
            assert!(h.view.editor_config_files.is_some());
            h.snapshot("editorconfig-files");
            h.click(Role::Button, &t!("ide-editorconfig-files-root", file = ".editorconfig"));
            assert_eq!(h.active(), h.path(".editorconfig"), "the chosen file opens");
            assert!(h.view.editor_config_files.is_none());
            h.run("editor.selection.set", json!({ "selections": [{ "anchor": 57, "head": 57 }] }));
            h.run("editor.edit.type", json!({ "text": "\nind" }));
            let completion = h.run("editor.completion.start", json!({}));
            assert_eq!(completion["items"][0]["label"], "indent_size", "{completion}");
            h.snapshot("editorconfig-highlighting-completion");
            h.run("editor.completion.cancel", json!({}));
            h.run("editor.edit.undo", json!({}));
            h.run("file.document.save", json!({}));

            h.run("file.document.open", json!({ "path": a }));
            h.choose(&widget, &keymap_page::action_title("project.editorConfig.open"));
            assert_eq!(h.active(), h.path(".editorconfig"));

            h.click(Role::Button, &t!("ide-project-new"));
            h.snapshot("editorconfig-tree-new-menu");
            h.click(Role::ListBoxOption, &keymap_page::action_title("view.editorConfig.new"));
            assert!(h.view.new_editor_config.as_ref().is_some_and(|dialog| dialog.folder == h.path("")));
            h.click(Role::Button, &t!("settings-cancel"));
            assert!(h.view.new_editor_config.is_none());

            let (view, registry, session) = (&mut h.view, &h.registry, &mut h.session);
            let ctx = egui::Context::default();
            view.perform_action(&ctx, registry, session, "view.editorConfig.new", json!({ "folder": "sub" }), true)
                .unwrap();
            h.snapshot("editorconfig-new-dialog");
            h.click(Role::CheckBox, &t!("ide-editorconfig-stub-intellij"));
            h.click(Role::Button, &t!("ide-launcher-ok"));
            assert!(h.view.new_editor_config.is_none(), "closed after creating the file");
            let created = std::fs::read_to_string(h.path("sub/.editorconfig")).unwrap();
            assert!(created.starts_with("root = true\n\n[*]\ncharset = utf-8\n"), "{created}");
            assert!(created.contains("ij_continuation_indent_size = 8\n"), "{created}");
            assert_eq!(h.active(), h.path("sub/.editorconfig"), "opened");
            let (view, registry, session) = (&mut h.view, &h.registry, &mut h.session);
            view.perform_action(&ctx, registry, session, "view.editorConfig.new", json!({ "folder": "sub" }), true)
                .unwrap();
            h.click(Role::Button, &t!("ide-launcher-ok"));
            assert!(h.view.new_editor_config.as_ref().is_some_and(|dialog| dialog.error.is_some()), "it exists");
            h.snapshot("editorconfig-new-dialog-exists");
            h.click(Role::Button, &t!("settings-cancel"));

            h.run("file.document.open", json!({ "path": a }));
            h.choose(&widget, &keymap_page::action_title("view.editorConfig.disable"));
            assert!(!zen_workbench::editor_config::enabled(h.session.settings()));
            assert!(!h.view.overview.settings.editor_config, "saved in the settings");
            assert!(h.find(Role::Button, &widget).is_none(), "no mark once EditorConfig is off");
            h.snapshot("editorconfig-disabled");
        }

        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn a_background_task_does_not_move_the_status_bar_widgets() {
        let mut h = Harness::new("en");
        let a = h.path("a.txt");
        h.run("file.document.open", json!({ "path": a }));
        let widget = h.widget();
        let before = h.find(Role::Button, &widget).expect("the indentation widget");
        let task = h.session.state_mut::<zen_workbench::tasks::Tasks>().start("Indexing…", "index", false);
        let during = h.find(Role::Button, &widget).expect("the indentation widget while a task runs");
        assert_eq!(before, during, "a task that starts does not shift the widget under the pointer");
        task.finish(Ok(String::new()));
        assert_eq!(h.find(Role::Button, &widget), Some(before));
    }
}
