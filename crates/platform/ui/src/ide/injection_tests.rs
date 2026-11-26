use std::path::PathBuf;
use std::sync::Arc;

use egui::{Color32, Rect, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_syntax::{HighlightKind, Highlighter, Span};

use super::editor_view::EditorView;
use super::{IdePalette, IdeProduct, IdeView};

struct GoStrings;

impl Highlighter for GoStrings {
    fn handles(&self, name: &str) -> bool {
        name.ends_with(".go")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let bytes = text.as_bytes();
        let mut spans = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'"' || bytes[at] == b'`' {
                let quote = bytes[at];
                let end = text[at + 1..].find(char::from(quote)).map_or(text.len(), |end| at + end + 2);
                spans.push(Span { range: at..end, kind: HighlightKind::String });
                at = end;
            } else if text[at..].starts_with("package") || text[at..].starts_with("func") {
                let end = at + if text[at..].starts_with("package") { 7 } else { 4 };
                spans.push(Span { range: at..end, kind: HighlightKind::Keyword });
                at = end;
            } else {
                at += 1;
            }
        }
        spans
    }
}

const MAIN: &str = "package main\n\nfunc load() {\n\tq := \"SELECT id, total FROM orders WHERE id = $1\"\n\tr := `UPDATE orders\n\t\tSET total = 0\n\t\tWHERE id = $1`\n\ts := \"Select a file\"\n}\n";

fn opened() -> (Registry, Session, PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.go");
    std::fs::write(&file, MAIN).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    let go: Arc<dyn Highlighter> = Arc::new(GoStrings);
    let highlighters = session.state_mut::<zen_workbench::brackets::Highlighters>();
    highlighters.set_source(Arc::new(move |name| go.handles(name).then(|| Arc::clone(&go))));
    highlighters.set_injectors(Arc::new(|name| {
        if zen_db::inject::hosts(name) {
            vec![Arc::new(zen_db::inject::SqlStrings::new(None)) as Arc<dyn zen_syntax::inject::Injector>]
        } else {
            Vec::new()
        }
    }));
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    (registry, session, file, dir)
}

fn colours_of(shapes: &[egui::Shape], word: &str) -> Vec<Color32> {
    let mut found = Vec::new();
    for shape in shapes {
        let egui::Shape::Text(text) = shape else { continue };
        let job = &text.galley.job;
        for (at, _) in job.text.match_indices(word) {
            let section = job
                .sections
                .iter()
                .find(|section| (section.byte_range.start.0..section.byte_range.end.0).contains(&at));
            found.extend(section.map(|section| section.format.color));
        }
    }
    found
}

#[test]
fn sql_in_strings_takes_the_keyword_colour_and_other_strings_stay() {
    let (registry, mut session, _, _dir) = opened();
    for palette in [IdePalette::zen_dark(), IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light())] {
        let keyword = palette.syntax(HighlightKind::Keyword).color;
        let string = palette.syntax(HighlightKind::String).color;
        assert_ne!(keyword, string);
        let ctx = egui::Context::default();
        let mut view = EditorView::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 400.0))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                view.show(ui, &registry, &mut session, Some(Arc::new(GoStrings)), &palette);
            });
            output.textures_delta.clear();
            shapes = output.shapes.into_iter().map(|clipped| clipped.shape).collect();
        }
        assert_eq!(colours_of(&shapes, "SELECT"), [keyword], "the SQL keyword inside the string");
        assert_eq!(colours_of(&shapes, "WHERE"), [keyword, keyword], "on one line and in a raw string's third line");
        assert_eq!(colours_of(&shapes, "Select a file"), [string], "a sentence stays a string");
    }
}

#[test]
fn the_window_shows_sql_in_a_go_file_in_both_themes() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (registry, mut session, _, _dir) = opened();
        let product = IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: vec![Arc::new(GoStrings)],
            ..IdeProduct::default()
        };
        let mut view = IdeView::new(Arc::new(product), crate::plugins::Plugins::new(None));
        view.project_open = false;
        view.set_theme(&theme);
        let scale = if snapshots.is_some() { 2.0 } else { 1.0 };
        let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 420.0), scale);
        view.palette.apply(screen.context());
        for _ in 0..3 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session)).into_image();
        let keyword = view.palette.syntax(HighlightKind::Keyword).color;
        let close = |pixel: &Color32| {
            let distance = |a: u8, b: u8| i32::from(a).abs_diff(i32::from(b));
            distance(pixel.r(), keyword.r()) + distance(pixel.g(), keyword.g()) + distance(pixel.b(), keyword.b()) <= 24
        };
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("sql-in-go-{name}.png")), &image).unwrap();
        }
        assert!(image.pixels.iter().filter(|pixel| close(pixel)).count() > 0, "keywords are drawn");
    }
}

fn with_database(registry: &mut Registry, session: &mut Session, dir: &std::path::Path) -> String {
    zen_workbench::database::register(registry).unwrap();
    session.state_mut::<zen_workbench::workspaces::Workspaces>().set_data_dir(dir.join("data"));
    zen_workbench::database::install(session);
    session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::new(zen_db::memory::MemoryDriver::sql())];
    session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let added = registry
        .dispatch(
            session,
            "db.dataSource.add",
            json!({ "name": "Shop", "driver": zen_db::memory::SQL, "password": "pw" }),
        )
        .unwrap();
    registry
        .dispatch(
            session,
            "db.dataSource.add",
            json!({ "name": "Billing", "driver": zen_db::memory::SQL, "password": "pw" }),
        )
        .unwrap();
    added["id"].as_str().unwrap().to_owned()
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

#[test]
fn running_sql_without_a_connection_asks_for_one_at_the_caret_and_then_asks_for_parameters() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut registry, mut session, file, dir) = opened();
        let shop = with_database(&mut registry, &mut session, dir.path());
        let at = MAIN.find("FROM orders").unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": at }] })).unwrap();
        let product = IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: vec![Arc::new(GoStrings)],
            ..IdeProduct::default()
        };
        let mut view = IdeView::new(Arc::new(product), crate::plugins::Plugins::new(None));
        view.project_open = false;
        view.set_theme(&theme);
        let scale = if snapshots.is_some() { 2.0 } else { 1.0 };
        let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 420.0), scale);
        view.palette.apply(screen.context());
        for _ in 0..3 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        screen.step(Vec::new(), |ui| {
            view.run_action(ui, &registry, &mut session, "db.injection.run", json!({}));
            view.show(ui, &registry, &mut session);
        });
        assert!(view.editor_view.sql_connection_open(), "an unbound run asks for a connection");
        for _ in 0..2 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session)).into_image();
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("sql-in-go-connection-{name}.png")), &image).unwrap();
        }
        screen.step(vec![key(egui::Key::Enter)], |ui| view.show(ui, &registry, &mut session));
        for _ in 0..3 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        assert!(!view.editor_view.sql_connection_open());
        let listed = registry.dispatch(&mut session, "db.injection.list", json!({ "path": file })).unwrap();
        assert_eq!((listed["binding"].as_str(), listed["connection"].as_str()), (Some("file"), Some(shop.as_str())));
        assert!(
            matches!(view.transaction_dialogs.open, Some(super::transaction_dialog::Dialog::Parameters(_))),
            "the run goes on and asks for the value of $1"
        );
        for _ in 0..30 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session)).into_image();
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("sql-in-go-parameters-{name}.png")), &image).unwrap();
        }
    }
}

#[test]
fn the_connection_chooser_starts_on_the_bound_connection_and_counts_every_arrow() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let (mut registry, mut session, file, dir) = opened();
    with_database(&mut registry, &mut session, dir.path());
    for name in ["Audit", "Reports"] {
        registry
            .dispatch(&mut session, "db.dataSource.add", json!({ "name": name, "driver": zen_db::memory::SQL }))
            .unwrap();
    }
    let listed = registry.dispatch(&mut session, "db.dataSource.list", json!({})).unwrap();
    let ids: Vec<String> = listed["connections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|connection| connection["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids.len(), 4);
    registry.dispatch(&mut session, "db.injection.bind", json!({ "path": file, "connection": ids[1] })).unwrap();
    let at = MAIN.find("FROM orders").unwrap();
    registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": at }] })).unwrap();
    let product = IdeProduct {
        name: "Test",
        id: "test",
        version: "0.0.0",
        highlighters: vec![Arc::new(GoStrings)],
        ..IdeProduct::default()
    };
    let mut view = IdeView::new(Arc::new(product), crate::plugins::Plugins::new(None));
    view.project_open = false;
    let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 420.0), 1.0);
    view.palette.apply(screen.context());
    for _ in 0..3 {
        screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    view.editor_view.ask_sql_connection(&registry, &mut session, &file, None);
    for _ in 0..2 {
        screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    assert_eq!(view.editor_view.sql_connection_highlighted(), Some(ids[1].as_str()), "the bound connection");
    screen.step(vec![key(egui::Key::ArrowDown), key(egui::Key::ArrowDown)], |ui| {
        view.show(ui, &registry, &mut session);
    });
    assert_eq!(view.editor_view.sql_connection_highlighted(), Some(ids[3].as_str()), "both presses count");
    screen.step(vec![key(egui::Key::Enter)], |ui| view.show(ui, &registry, &mut session));
    for _ in 0..2 {
        screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    assert!(!view.editor_view.sql_connection_open());
    let listed = registry.dispatch(&mut session, "db.injection.list", json!({ "path": file })).unwrap();
    assert_eq!(listed["connection"].as_str(), Some(ids[3].as_str()), "Enter binds the highlighted row");
}

#[test]
fn running_sql_in_code_keeps_the_scroll_and_marks_after_the_literal() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let short = "'text' AS words";
    let long = "'a fairly long text that keeps going' AS words";
    for (name, theme, width, words) in [
        ("dark", zen_theme::ResolvedTheme::zen_dark(), 1300.0, short),
        ("light", zen_theme::ResolvedTheme::zen_light(), 1300.0, short),
        ("narrow", zen_theme::ResolvedTheme::zen_dark(), 900.0, long),
    ] {
        let (mut registry, mut session, _, dir) = opened();
        let shop = with_database(&mut registry, &mut session, dir.path());
        let text =
            format!("package main\n\nfunc load() {{\n\tquery := db.Query(\"SELECT 1 AS answer, {words}\");\n}}\n");
        let file = dir.path().join("load.go");
        std::fs::write(&file, &text).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        registry.dispatch(&mut session, "db.injection.bind", json!({ "path": file, "connection": shop })).unwrap();
        let at = text.find("SELECT").unwrap() + 3;
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": at }] })).unwrap();
        let product = IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: vec![Arc::new(GoStrings)],
            ..IdeProduct::default()
        };
        let mut view = IdeView::new(Arc::new(product), crate::plugins::Plugins::new(None));
        view.project_open = false;
        view.set_theme(&theme);
        let scale = if snapshots.is_some() { 2.0 } else { 1.0 };
        let mut screen = crate::offscreen::Offscreen::new(vec2(width, 420.0), scale);
        view.palette.apply(screen.context());
        for _ in 0..3 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        assert_eq!(view.editor_view.scrolled_x(), 0.0);
        screen.step(Vec::new(), |ui| {
            view.run_action(ui, &registry, &mut session, "db.injection.run", json!({}));
            view.show(ui, &registry, &mut session);
        });
        for _ in 0..40 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session)).into_image();
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("sql-in-go-ran-{name}.png")), &image).unwrap();
        }
        assert_eq!(view.editor_view.scrolled_x(), 0.0, "the editor keeps its horizontal scroll");
        let state = registry.dispatch(&mut session, "db.console.state", json!({ "path": file })).unwrap();
        let line_end = text.find(");\n}").unwrap() + 2;
        assert_eq!(state["hints"][0]["offset"].as_u64(), Some(line_end as u64), "after the literal and `);`: {state}");
    }
}
