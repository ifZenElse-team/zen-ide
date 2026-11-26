use std::path::PathBuf;
use std::sync::Arc;

use egui::{Color32, Rect, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_syntax::{HighlightKind, Highlighter, Span};

use super::editor_view::EditorView;
use super::{IdePalette, IdeProduct, IdeView};

struct GoLike;

impl Highlighter for GoLike {
    fn handles(&self, name: &str) -> bool {
        name.ends_with(".go")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        let bytes = text.as_bytes();
        let mut at = 0;
        let mut declaring = false;
        while at < bytes.len() {
            let rest = &text[at..];
            if rest.starts_with("//") {
                let end = rest.find('\n').map_or(text.len(), |end| at + end);
                spans.push(Span { range: at..end, kind: HighlightKind::Comment });
                at = end;
            } else if bytes[at] == b'"' {
                let end = text[at + 1..].find('"').map_or(text.len(), |end| at + end + 2);
                spans.push(Span { range: at..end, kind: HighlightKind::String });
                at = end;
            } else if bytes[at].is_ascii_digit() {
                let end = rest.find(|c: char| !c.is_ascii_digit()).map_or(text.len(), |end| at + end);
                spans.push(Span { range: at..end, kind: HighlightKind::Number });
                at = end;
            } else if bytes[at].is_ascii_alphabetic() || bytes[at] == b'_' {
                let end =
                    rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(text.len(), |end| at + end);
                let word = &text[at..end];
                if ["package", "import", "func", "return", "type", "struct", "string", "error", "if", "nil"]
                    .contains(&word)
                {
                    spans.push(Span { range: at..end, kind: HighlightKind::Keyword });
                    declaring = word == "func" || word == "type";
                } else if declaring {
                    spans.push(Span { range: at..end, kind: HighlightKind::Function });
                    declaring = false;
                }
                at = end;
            } else {
                at += 1;
            }
        }
        spans
    }
}

const MAIN: &str = "package main\n\nimport \"fmt\"\n\n// parseUrlHadnler reads the adress of a reqeust.\nfunc parseUrlHadnler(raw string) (string, error) {\n\tmesage := \"cannot parse the wrold\"\n\tif raw == \"\" {\n\t\treturn \"\", fmt.Errorf(mesage)\n\t}\n\treturn raw, nil\n}\n";

fn product() -> IdeProduct {
    IdeProduct {
        name: "Test",
        id: "test",
        version: "0.0.0",
        highlighters: vec![Arc::new(GoLike)],
        ..IdeProduct::default()
    }
}

fn opened() -> (Registry, Session, PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.go");
    std::fs::write(&file, MAIN).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    let go: Arc<dyn Highlighter> = Arc::new(GoLike);
    session
        .state_mut::<zen_workbench::brackets::Highlighters>()
        .set_source(Arc::new(move |name| go.handles(name).then(|| Arc::clone(&go))));
    session.state_mut::<zen_workbench::spelling::Spelling>().activate();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    let typos = registry.dispatch(&mut session, "spelling.typos.list", json!({ "path": file })).unwrap();
    let words: Vec<&str> =
        typos["typos"].as_array().unwrap().iter().map(|typo| typo["word"].as_str().unwrap()).collect();
    assert_eq!(words, ["Hadnler", "adress", "reqeust", "Hadnler", "mesage", "wrold"], "{typos}");
    (registry, session, file, dir)
}

fn wavy_lines(shapes: &[egui::Shape], color: Color32) -> usize {
    shapes
        .iter()
        .filter(|shape| {
            matches!(shape, egui::Shape::Path(path)
                if path.stroke.color == egui::epaint::ColorMode::Solid(color) && path.points.len() > 2)
        })
        .count()
}

#[test]
fn typos_are_underlined_in_the_typo_colour() {
    let (registry, mut session, _, _dir) = opened();
    for palette in [IdePalette::zen_dark(), IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light())] {
        assert_ne!(palette.diagnostic_typo, palette.diagnostic_error);
        assert_eq!(palette.problem("typo"), palette.diagnostic_typo);
        let ctx = egui::Context::default();
        let mut view = EditorView::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 400.0))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                view.show(ui, &registry, &mut session, Some(Arc::new(GoLike)), &palette);
            });
            output.textures_delta.clear();
            shapes = output.shapes.into_iter().map(|clipped| clipped.shape).collect();
        }
        assert_eq!(wavy_lines(&shapes, palette.diagnostic_typo), 6, "one under each typo");
        assert_eq!(wavy_lines(&shapes, palette.diagnostic_error), 0);
        assert_eq!(wavy_lines(&shapes, palette.diagnostic_warning), 0);
    }
}

fn pixels_of(image: &egui::ColorImage, color: Color32, rows: std::ops::Range<usize>) -> usize {
    let width = image.size[0];
    image.pixels[rows.start * width..rows.end.min(image.size[1]) * width]
        .iter()
        .filter(|pixel| {
            let distance = |a: u8, b: u8| i32::from(a).abs_diff(i32::from(b));
            distance(pixel.r(), color.r()) + distance(pixel.g(), color.g()) + distance(pixel.b(), color.b()) <= 24
        })
        .count()
}

#[test]
fn the_window_shows_typos_in_the_editor_and_the_problems_tool_window() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for language in ["en", "ru"] {
        zen_i18n::with_language(language, || {
            let (registry, mut session, _, _dir) = opened();
            let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
            view.project_open = false;
            view.show_tool("problems");
            let (width, height) = (1100.0, 620.0);
            let scale = if snapshots.is_some() { 2.0 } else { 1.0 };
            let mut screen = crate::offscreen::Offscreen::new(vec2(width, height), scale);
            for _ in 0..3 {
                screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
            }
            let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
            let typo = view.palette.diagnostic_typo;
            let rows = image.size[1];
            assert!(pixels_of(&image, typo, 0..rows * 45 / 100) > 0, "the wavy lines and the count's mark");
            assert!(pixels_of(&image, typo, rows * 55 / 100..rows) > 0, "the marks of the Problems rows");
            assert_eq!(pixels_of(&image, view.palette.diagnostic_error, 0..rows), 0, "nothing is an error");
            let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
            let message = zen_i18n::t!("workbench-spelling-typo", word = "Hadnler");
            let row = zen_i18n::t!("a11y-problem", severity = "typo", message = message.as_str(), line = 5);
            assert!(
                crate::accessibility::testing::find(&tree, egui::accesskit::Role::ListItem, &row).is_some(),
                "{row} is a row of the Problems tool window"
            );
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("spelling-window-{language}.png")), &image).unwrap();
            }
        });
    }
}

struct Window {
    registry: Registry,
    session: Session,
    view: IdeView,
    screen: crate::offscreen::Offscreen,
    file: PathBuf,
    _dir: tempfile::TempDir,
    data: tempfile::TempDir,
}

impl Window {
    fn new(scale: f32) -> Window {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let (registry, mut session, file, dir) = opened();
        let data = tempfile::tempdir().unwrap();
        session
            .state_mut::<zen_workbench::spelling::Spelling>()
            .set_application_dictionary(data.path().join("dictionaries/application.dic"));
        let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        view.project_open = false;
        let screen = crate::offscreen::Offscreen::new(vec2(1100.0, 620.0), scale);
        let mut window = Window { registry, session, view, screen, file, _dir: dir, data };
        window.steps(3, Vec::new());
        window
    }

    fn steps(&mut self, count: usize, events: Vec<egui::Event>) {
        let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
        self.screen.step(events, |ui| view.show(ui, registry, session));
        for _ in 1..count {
            self.screen.step(Vec::new(), |ui| view.show(ui, registry, session));
        }
    }

    fn image(&mut self) -> egui::ColorImage {
        let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
        for _ in 0..20 {
            self.screen.step(Vec::new(), |ui| view.show(ui, registry, session));
        }
        self.screen.frame(Vec::new(), |ui| view.show(ui, registry, session)).into_image()
    }

    fn caret_at(&mut self, word: &str, nth: usize) {
        let offset = MAIN.match_indices(word).nth(nth).unwrap().0;
        let line = MAIN[..offset].matches('\n').count() + 1;
        let column = offset - MAIN[..offset].rfind('\n').map_or(0, |at| at + 1) + 2;
        self.registry
            .dispatch(&mut self.session, "editor.caret.goTo", json!({ "line": line, "column": column }))
            .unwrap();
        self.registry.dispatch(&mut self.session, "spelling.typos.list", json!({ "path": self.file })).unwrap();
    }

    fn alt_enter(&mut self) {
        let ctx = self.screen.context().clone();
        self.view
            .perform_action(&ctx, &self.registry, &mut self.session, "view.intentions.open", json!({}), true)
            .unwrap();
        self.steps(2, Vec::new());
    }

    fn key(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let press = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        self.steps(3, vec![press]);
    }

    fn text(&self) -> String {
        self.session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn application_words(&self) -> String {
        std::fs::read_to_string(self.data.path().join("dictionaries/application.dic")).unwrap_or_default()
    }
}

#[test]
fn alt_enter_on_a_typo_changes_it_saves_it_and_cmd_z_undoes_both() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    assert!(crate::keymap::shortcut("view.intentions.open").is_some_and(|keys| keys.contains('⌥')), "⌥⏎ opens it");
    for language in ["en", "ru"] {
        zen_i18n::with_language(language, || {
            let mut window = Window::new(if snapshots.is_some() { 2.0 } else { 1.0 });
            window.caret_at("adress", 0);
            window.alt_enter();
            let titles = window.view.editor_view.menu_titles();
            assert_eq!(titles[0], zen_i18n::t!("workbench-spelling-fix-typo"), "{language}: {titles:?}");
            assert_eq!(titles[1], "address", "the suggestions follow the greyed Fix typo: {titles:?}");
            assert_eq!(titles.last(), Some(&zen_i18n::t!("workbench-spelling-save", word = "adress")));
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("spelling-alt-enter-{language}.png")), &window.image())
                    .unwrap();
            }
            window.key(egui::Key::Enter, egui::Modifiers::NONE);
            assert_eq!(window.text(), MAIN.replacen("adress", "address", 1), "the first suggestion is picked");
            assert!(window.view.editor_view.menu_titles().is_empty());
            window.view.editor_view.focus();
            window.steps(2, Vec::new());
            window.key(egui::Key::Z, egui::Modifiers::COMMAND);
            assert_eq!(window.text(), MAIN, "⌘Z brings the typo back");

            window.caret_at("reqeust", 0);
            window.alt_enter();
            window.key(egui::Key::ArrowUp, egui::Modifiers::NONE);
            window.key(egui::Key::Enter, egui::Modifiers::NONE);
            assert_eq!(window.view.editor_view.menu_title(), Some(zen_i18n::t!("ide-spelling-select-dictionary")));
            assert_eq!(
                window.view.editor_view.menu_titles(),
                [zen_i18n::t!("ide-spelling-application-level"), zen_i18n::t!("ide-spelling-project-level")],
                "Use single dictionary is off by default, so Save asks, application-level first"
            );
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(
                    &dir.join(format!("spelling-select-dictionary-{language}.png")),
                    &window.image(),
                )
                .unwrap();
            }
            window.key(egui::Key::Enter, egui::Modifiers::NONE);
            assert_eq!(window.application_words(), "reqeust\n", "Save 'reqeust' to dictionary");
            let typos = window
                .registry
                .dispatch(&mut window.session, "spelling.typos.list", json!({ "path": window.file }))
                .unwrap();
            assert!(!typos.to_string().contains("\"reqeust\""), "{typos}");
            window.view.editor_view.focus();
            window.steps(2, Vec::new());
            window.key(egui::Key::Z, egui::Modifiers::COMMAND);
            assert_eq!(window.application_words(), "", "⌘Z takes the saved word out");
            assert_eq!(window.text(), MAIN, "and leaves the text alone");
        });
    }
}

#[test]
fn a_declared_name_offers_one_rename_and_save_goes_to_the_chosen_dictionary() {
    let mut window = Window::new(1.0);
    window.caret_at("Hadnler", 1);
    window.alt_enter();
    let titles = window.view.editor_view.menu_titles();
    assert_eq!(
        titles,
        [
            zen_i18n::t!("workbench-spelling-rename-to", name = "parseUrlHandler"),
            zen_i18n::t!("workbench-spelling-save", word = "Hadnler"),
        ]
    );
    window.key(egui::Key::Escape, egui::Modifiers::NONE);
    window.caret_at("mesage", 0);
    window.alt_enter();
    window.key(egui::Key::ArrowUp, egui::Modifiers::NONE);
    window.key(egui::Key::Enter, egui::Modifiers::NONE);
    window.key(egui::Key::ArrowDown, egui::Modifiers::NONE);
    window.key(egui::Key::Enter, egui::Modifiers::NONE);
    let project = window.file.parent().unwrap().join(".idea/dictionaries/project.xml");
    let xml = std::fs::read_to_string(&project).unwrap();
    assert!(xml.contains("<component name=\"ProjectDictionaryState\">") && xml.contains("<w>mesage</w>"), "{xml}");
    let state = window.registry.dispatch(&mut window.session, "spelling.dictionaries.get", json!({})).unwrap();
    assert_eq!(state["project"]["words"], json!(["mesage"]));
    window.view.overview.settings.spelling.use_single_dictionary = true;
    window.view.overview.settings.spelling.saving_dictionary = zen_workbench::spelling::DictionaryLevel::Application;
    window.steps(2, Vec::new());
    window.caret_at("wrold", 0);
    window.alt_enter();
    window.key(egui::Key::ArrowUp, egui::Modifiers::NONE);
    window.key(egui::Key::Enter, egui::Modifiers::NONE);
    assert!(window.view.editor_view.menu_titles().is_empty(), "with a single dictionary Save does not ask");
    assert_eq!(window.application_words(), "wrold\n");
}
