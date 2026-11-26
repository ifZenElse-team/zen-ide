use std::path::PathBuf;
use std::sync::mpsc;

use egui::accesskit::{Role, TreeUpdate};
use serde_json::{Value, json};

use super::*;
use crate::accessibility::testing::find;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 900.0);

const README: &str = r#"# Zen IDE

A family of IDEs in **pure Rust**, with *native* rendering, `cargo xtask ci` and ~~webviews~~ none.
See [the other page](other.md#details), [the setup](#setup) or https://ide.ifzenelse.space.

![Logo](images/logo.svg) ![Badge](http://127.0.0.1:9/badge.svg)

> [!TIP]
> Open a Markdown file to see this preview beside it.

## Setup

1. Install Zen Toolbox
2. Pick an IDE
   - [x] GoGarden
   - [ ] RustRonin

| Product | Language | Version |
|:--------|:--------:|--------:|
| GoGarden | Go | 0.38 |
| RustRonin | Rust | 0.2 |

```rust
fn main() {
    println!("hello");
}
```

> A plain quote, with a footnote[^1].

---

[^1]: Footnotes are gathered at the end.
"#;

const LOGO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="32"><rect width="96" height="32" rx="6" fill="#3d8fd1"/></svg>"##;

pub(in crate::ide) struct Keywords;

impl zen_syntax::Highlighter for Keywords {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".rs")
    }

    fn highlight(&self, text: &str) -> Vec<zen_syntax::Span> {
        let mut spans = Vec::new();
        for (at, _) in text.match_indices("fn ") {
            spans.push(zen_syntax::Span { range: at..at + 2, kind: zen_syntax::HighlightKind::Keyword });
        }
        let mut from = 0;
        while let Some(open) = text[from..].find('"') {
            let open = from + open;
            let Some(close) = text[open + 1..].find('"') else { break };
            spans.push(zen_syntax::Span { range: open..open + close + 2, kind: zen_syntax::HighlightKind::String });
            from = open + close + 2;
        }
        spans.sort_by_key(|span| span.range.start);
        spans
    }
}

struct Window {
    app: IdeApp,
    screen: crate::offscreen::Offscreen,
    data: tempfile::TempDir,
}

fn product() -> IdeProduct {
    IdeProduct {
        folding_options: Vec::new(),
        name: "Test",
        id: "test",
        version: "0.0.0",
        highlighters: vec![Arc::new(Keywords)],
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
    }
}

impl Window {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir_all(project.join("images")).unwrap();
        std::fs::write(project.join("README.md"), README).unwrap();
        std::fs::write(project.join("other.md"), "# Other\n\n## Details\n\nMore.\n").unwrap();
        std::fs::write(project.join("images/logo.svg"), LOGO).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let app = IdeApp::new(product(), registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        let screen = crate::offscreen::Offscreen::new(SIZE, 1.0);
        let mut window = Self { app, screen, data };
        window.steps(5);
        window.command("file.document.open", json!({ "path": project.join("README.md") }));
        window.settled();
        window.steps(150);
        window
    }

    fn project(&self) -> PathBuf {
        self.data.path().join("project")
    }

    fn steps(&mut self, count: usize) {
        for _ in 0..count {
            self.step(Vec::new());
        }
    }

    fn step(&mut self, events: Vec<egui::Event>) {
        let Self { app, screen, .. } = self;
        screen.step(events, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
    }

    fn tree(&mut self) -> TreeUpdate {
        let Self { app, screen, .. } = self;
        screen.accessibility_tree(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
    }

    fn click(&mut self, role: Role, name: &str) {
        let tree = self.tree();
        let node = find(&tree, role, name).unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        let bounds = node.bounds().expect("bounds");
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, (bounds.y0 + 6.0) as f32);
        self.click_at(at);
    }

    fn click_at(&mut self, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            self.step(events);
        }
        self.steps(3);
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        let (sender, answer) = mpsc::channel();
        let ctx = self.screen.context().clone();
        self.app.route(
            &ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for _ in 0..20 {
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            self.steps(1);
        }
        answer.recv_timeout(std::time::Duration::from_secs(10)).map_err(|error| error.to_string())?
    }

    fn command(&mut self, command: &str, params: Value) -> Value {
        self.call("dispatch_command", json!({ "command": command, "params": params }))
            .unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn settled(&mut self) -> Value {
        for _ in 0..500 {
            let answer = self.command("editor.preview.getMode", json!({}));
            if answer["state"] != "rendering" {
                for _ in 0..30 {
                    self.steps(1);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                return answer;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.steps(1);
        }
        panic!("the preview is not drawn");
    }

    fn text(&mut self) -> String {
        self.call("get_document_text", json!({ "path": self.project().join("README.md") })).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn snapshot(&mut self, name: &str) {
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) {
            let Self { app, screen, .. } = self;
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            crate::offscreen::write_png(&dir.join(format!("markdown-{name}.png")), &image).unwrap();
        }
    }
}

#[test]
fn a_readme_opens_with_its_preview_in_both_themes_and_languages() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").is_some();
    let languages: &[&str] = if snapshots { &["en", "ru"] } else { &["en"] };
    for language in languages {
        zen_i18n::set_language(Some(language));
        let mut window = Window::new();
        zen_i18n::set_language(Some(language));
        window.steps(3);
        let state = window.call("get_window_state", json!({})).unwrap();
        assert_eq!(
            state["preview"],
            json!({ "kind": "markdown", "mode": "editorAndPreview", "orientation": "vertical", "scope": "default" }),
            "{state}"
        );
        let answer = window.command("editor.preview.getMode", json!({}));
        assert_eq!((&answer["kind"], &answer["state"]), (&json!("markdown"), &json!("ready")));
        let tree = window.tree();
        for (role, name) in [
            (Role::Label, "A family of IDEs in pure Rust"),
            (Role::Label, "Setup"),
            (Role::CheckBox, &zen_i18n::t!("ide-markdown-task")),
            (Role::Link, &zen_i18n::t!("ide-markdown-load-images", host = "127.0.0.1")),
        ] {
            assert!(find(&tree, role, name).is_some(), "{role:?} {name:?} is drawn");
        }
        for theme in ["zen-dark", "zen-light"] {
            window.command("ui.theme.select", json!({ "id": theme }));
            window.steps(4);
            window.snapshot(&format!("{}-{language}", theme.trim_start_matches("zen-")));
        }
        window.command("ui.theme.select", json!({ "id": "zen-dark" }));
    }
    zen_i18n::set_language(Some("en"));
}

#[test]
fn a_click_puts_the_caret_on_the_line_and_a_checkbox_ticks_its_task() {
    let mut window = Window::new();
    window.click(Role::Label, "Setup");
    let selections = window.command("editor.selection.get", json!({}));
    assert_eq!(selections[0]["line"], 10, "the heading's line (11, from 0 here): {selections}");
    let tree = window.tree();
    let boxes: Vec<_> = tree.nodes.iter().filter(|(_, node)| node.role() == Role::CheckBox).collect();
    assert_eq!(boxes.len(), 2);
    let bounds = boxes[1].1.bounds().unwrap();
    window.click_at(egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32));
    assert!(window.text().contains("   - [x] RustRonin"), "{}", window.text());
}

#[test]
fn links_open_files_and_remote_images_wait_for_consent() {
    let mut window = Window::new();
    assert_eq!(window.command("markdown.images.list", json!({})), json!({ "hosts": [] }));
    window.click(Role::Link, &zen_i18n::t!("ide-markdown-load-images", host = "127.0.0.1"));
    assert_eq!(window.command("markdown.images.list", json!({})), json!({ "hosts": ["127.0.0.1"] }));
    window.click(Role::Link, "the other page");
    let state = window.call("get_window_state", json!({})).unwrap();
    assert!(state["editor"].as_str().unwrap().ends_with("other.md"), "{state}");
}

impl Window {
    fn places(&mut self) -> (f32, Option<usize>) {
        let slot = self.app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        let path = window.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        let offset = window.view.markdown_views.offset(&path).unwrap_or(0.0);
        let top = window.session.state::<zen_workbench::sticky::ShownTop>().and_then(|shown| shown.get(&path));
        (offset, top)
    }

    fn wheel(&mut self, at: egui::Pos2, lines: f32, frames: usize) {
        self.step(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..frames {
            let wheel = egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -lines),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::NONE,
            };
            self.step(vec![egui::Event::PointerMoved(at), wheel]);
        }
        self.steps(30);
    }
}

fn long_readme() -> String {
    (1..=60)
        .map(|section| format!("## Section {section}\n\nThe text of section {section}, long enough to read.\n\n"))
        .collect()
}

#[test]
fn preview_only_scrolls_and_keeps_its_place_and_the_editor_follows_when_shown() {
    let mut window = Window::new();
    let readme = window.project().join("README.md");
    window.command("editor.text.write", json!({ "path": readme, "text": long_readme() }));
    window.settled();
    window.command("editor.preview.previewOnly", json!({}));
    window.steps(10);
    let middle = egui::pos2(SIZE.x / 2.0, SIZE.y / 2.0);
    window.wheel(middle, 6.0, 8);
    let (scrolled, _) = window.places();
    assert!(scrolled > 300.0, "the preview scrolled: {scrolled}");
    window.steps(30);
    let (kept, _) = window.places();
    assert!((kept - scrolled).abs() < 1.0, "Preview Only keeps its place: {scrolled} then {kept}");
    window.wheel(middle, 6.0, 4);
    let (further, _) = window.places();
    assert!(further > kept + 100.0, "and scrolls on from there: {kept} then {further}");

    window.command(
        "editor.text.replace",
        json!({ "path": readme, "oldText": "section 60,", "newText": "section sixty," }),
    );
    window.settled();
    let (after_render, _) = window.places();
    assert!((after_render - further).abs() < 1.0, "a new rendering keeps the place: {further} then {after_render}");

    window.command("editor.preview.editorAndPreview", json!({}));
    window.steps(30);
    let (split_offset, top) = window.places();
    let top = top.expect("the editor is drawn");
    assert!(top > 20, "the editor came to the preview's place: line {top}");
    assert!((split_offset - after_render).abs() < 40.0, "the preview stayed: {after_render} then {split_offset}");
    window.steps(30);
    assert_eq!(window.places().1, Some(top), "the sides do not fight");

    let editor = egui::pos2(SIZE.x * 0.42, SIZE.y / 2.0);
    let preview = egui::pos2(SIZE.x * 0.8, SIZE.y / 2.0);
    window.wheel(editor, -10.0, 4);
    let (offset_up, top_up) = window.places();
    assert!(top_up.unwrap() + 10 < top, "the editor scrolled up: {top} then {top_up:?}");
    assert!(offset_up + 100.0 < split_offset, "the preview followed it: {split_offset} then {offset_up}");
    window.wheel(preview, 8.0, 4);
    let (offset_down, top_down) = window.places();
    assert!(offset_down > offset_up + 100.0, "the preview scrolled: {offset_up} then {offset_down}");
    assert!(top_down.unwrap() > top_up.unwrap() + 5, "the editor followed it: {top_up:?} then {top_down:?}");
    window.steps(30);
    assert_eq!(window.places(), (offset_down, top_down), "and both stay");

    window.command("editor.preview.previewOnly", json!({}));
    window.steps(30);
    assert!((window.places().0 - offset_down).abs() < 1.0, "Preview Only keeps the place of the split");
}

const DIAGRAMS: &str = "# Diagrams\n\n```mermaid\nflowchart TD\n  A[Start] --> B{Ready?}\n  B -->|Yes| C[Ship]\n```\n\n```mermaid\nsequenceDiagram\n  participant A as Alice\n  A->>Bob: Hello\n  Bob-->>A: Hi\n```\n\n```mermaid\nclassDiagram\n  class Animal {\n    +String name\n  }\n  Animal <|-- Dog\n```\n\n```mermaid\nstateDiagram-v2\n  [*] --> Idle\n  Idle --> Running : start\n  Running --> [*]\n```\n\n```mermaid\nnot a diagram at all\n```\n";

impl Window {
    fn drawn_diagrams(&mut self) {
        for _ in 0..600 {
            self.steps(1);
            let slot = self.app.slot_for(None).unwrap();
            let loading = slot.window.lock().unwrap().view.markdown_views.math_loading();
            if !loading {
                break;
            }
            std::thread::yield_now();
        }
        self.steps(5);
    }
}

#[test]
fn mermaid_blocks_are_drawn_and_one_that_cannot_be_shows_its_source_with_a_note() {
    let mut window = Window::new();
    let readme = window.project().join("README.md");
    window.command("editor.text.write", json!({ "path": readme, "text": DIAGRAMS }));
    window.settled();
    window.command("editor.preview.previewOnly", json!({}));
    window.drawn_diagrams();
    let tree = window.tree();
    assert!(find(&tree, Role::Image, &zen_i18n::t!("ide-markdown-mermaid-diagram")).is_some(), "a diagram is drawn");
    window.snapshot("mermaid");
    let listed = window.command("markdown.mermaid.list", json!({}));
    let diagrams = listed["diagrams"].as_array().unwrap();
    assert_eq!(
        diagrams.iter().map(|diagram| diagram["drawn"] == true).collect::<Vec<_>>(),
        [true, true, true, true, false],
        "{listed}"
    );
}

#[test]
fn a_diagram_that_cannot_be_read_says_why_under_its_source() {
    let mut window = Window::new();
    let readme = window.project().join("README.md");
    let text = "```mermaid\nnot a diagram at all\n```\n";
    window.command("editor.text.write", json!({ "path": readme, "text": text }));
    window.settled();
    window.command("editor.preview.previewOnly", json!({}));
    window.drawn_diagrams();
    let listed = window.command("markdown.mermaid.list", json!({}));
    let error = listed["diagrams"][0]["error"].as_str().unwrap().to_owned();
    let note = zen_i18n::t!("ide-markdown-mermaid-error", error = error.as_str());
    assert!(find(&window.tree(), Role::Label, &note).is_some(), "{note}");
}

impl Window {
    fn focus_editor_and_select(&mut self, from: usize, to: usize) {
        self.app.slot_for(None).unwrap().window.lock().unwrap().view.editor_view.focus();
        self.steps(3);
        self.command("editor.selection.set", json!({ "selections": [{ "anchor": from, "head": to }] }));
        self.steps(5);
    }

    fn press(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let event = egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers };
        self.step(vec![event]);
        self.steps(3);
    }
}

#[test]
fn the_format_shortcuts_work_in_a_markdown_editor_and_only_there() {
    let mut window = Window::new();
    let readme = window.project().join("README.md");
    window.command("editor.text.write", json!({ "path": readme, "text": "one two three\n" }));
    window.settled();
    window.steps(5);
    window.focus_editor_and_select(0, 3);
    let command = egui::Modifiers::COMMAND;
    let shift = egui::Modifiers::SHIFT | egui::Modifiers::COMMAND;
    window.press(egui::Key::B, command);
    assert_eq!(window.text(), "**one** two three\n");
    window.press(egui::Key::B, command);
    assert_eq!(window.text(), "one two three\n");
    for (key, modifiers, expected) in [
        (egui::Key::I, command, "*one* two three\n"),
        (egui::Key::S, shift, "~~one~~ two three\n"),
        (egui::Key::C, shift, "`one` two three\n"),
    ] {
        window.focus_editor_and_select(0, 3);
        window.press(key, modifiers);
        assert_eq!(window.text(), expected);
        window.command("editor.text.write", json!({ "path": readme, "text": "one two three\n" }));
        window.settled();
    }
    window.focus_editor_and_select(0, 3);
    window.press(egui::Key::U, shift);
    assert_eq!(window.text(), "[one]() two three\n");

    let plain = window.project().join("notes.txt");
    std::fs::write(&plain, "one two three\n").unwrap();
    window.command("file.document.open", json!({ "path": plain }));
    window.steps(5);
    window.focus_editor_and_select(0, 3);
    window.press(egui::Key::B, command);
    let text = window.call("get_document_text", json!({ "path": plain })).unwrap();
    assert_eq!(text["text"], "one two three\n", "⌘B is not bold outside Markdown");
}

#[test]
fn a_selection_shows_the_floating_toolbar_and_the_setting_hides_it() {
    let mut window = Window::new();
    let readme = window.project().join("README.md");
    window.command("editor.text.write", json!({ "path": readme, "text": "# Title\n\none two three\n" }));
    window.settled();
    window.steps(5);
    let bold = zen_i18n::t!("ide-markdown-toolbar-bold");
    window.focus_editor_and_select(0, 0);
    assert!(find(&window.tree(), Role::Button, &bold).is_none(), "no selection, no toolbar");
    window.focus_editor_and_select(9, 12);
    assert!(find(&window.tree(), Role::Button, &bold).is_some(), "shown over the selection");
    window.snapshot("floating-toolbar");
    window.click(Role::Button, &bold);
    window.steps(3);
    assert_eq!(window.text(), "# Title\n\n**one** two three\n");
    window.click(Role::Button, &zen_i18n::t!("ide-markdown-toolbar-check-list"));
    window.steps(3);
    assert_eq!(window.text(), "# Title\n\n- [ ] **one** two three\n");

    window.call("set_settings", json!({ "markdownHideFloatingToolbar": true })).unwrap();
    window.steps(5);
    window.focus_editor_and_select(0, 3);
    assert!(find(&window.tree(), Role::Button, &bold).is_none(), "Hide the formatting toolbar over a selection");
}

#[test]
fn with_sync_scroll_off_each_side_scrolls_alone() {
    let mut window = Window::new();
    let readme = window.project().join("README.md");
    window.command("editor.text.write", json!({ "path": readme, "text": long_readme() }));
    window.settled();
    window.call("set_settings", json!({ "markdown": { "syncScroll": false } })).unwrap();
    window.steps(10);
    assert_eq!(window.call("get_settings", json!({})).unwrap()["markdown"]["syncScroll"], false);
    let (offset, top) = window.places();
    window.wheel(egui::pos2(SIZE.x * 0.8, SIZE.y / 2.0), 6.0, 6);
    let (scrolled, kept) = window.places();
    assert!(scrolled > offset + 100.0, "the preview scrolled: {offset} then {scrolled}");
    assert_eq!(kept, top, "the editor stayed");
    window.wheel(egui::pos2(SIZE.x * 0.42, SIZE.y / 2.0), 6.0, 6);
    let (stayed, moved) = window.places();
    assert!(moved.unwrap() > top.unwrap_or(0) + 5, "the editor scrolled: {top:?} then {moved:?}");
    assert!((stayed - scrolled).abs() < 1.0, "the preview stayed: {scrolled} then {stayed}");
}

#[test]
fn the_preview_font_size_scales_the_preview_and_is_range_checked() {
    let mut window = Window::new();
    let readme = window.project().join("README.md");
    window.command("editor.text.write", json!({ "path": readme, "text": long_readme() }));
    window.settled();
    window.steps(10);
    let height = |window: &mut Window| {
        window.command("editor.preview.previewOnly", json!({}));
        window.steps(10);
        let at = egui::pos2(SIZE.x / 2.0, SIZE.y / 2.0);
        window.wheel(at, 1000.0, 6);
        window.places().0
    };
    let default = height(&mut window);
    window.call("set_settings", json!({ "markdown": { "previewFontSize": 29 } })).unwrap();
    window.steps(10);
    let large = height(&mut window);
    assert!(large > default * 1.3, "the longest scroll grew with the text: {default} then {large}");
    assert!(window.call("set_settings", json!({ "markdown": { "previewFontSize": 2 } })).is_err());
}

#[test]
fn the_preview_layout_setting_and_the_kinds_mode_are_one_value() {
    let mut window = Window::new();
    window.command("file.document.open", json!({ "path": window.project().join("other.md") }));
    let mode = |window: &mut Window| window.command("editor.preview.getMode", json!({}))["mode"].clone();
    assert_eq!(mode(&mut window), "editorAndPreview");
    window.call("set_settings", json!({ "markdown": { "previewLayout": "previewOnly" } })).unwrap();
    window.steps(5);
    assert_eq!(mode(&mut window), "previewOnly", "a file never switched takes the setting");

    window.command("editor.preview.setMode", json!({ "mode": "editorOnly", "scope": "type" }));
    window.steps(5);
    let settings = window.call("get_settings", json!({})).unwrap();
    assert_eq!(settings["markdown"]["previewLayout"], "editorOnly", "{settings}");

    window.command("editor.preview.setMode", json!({ "mode": "editorAndPreview" }));
    window.call("set_settings", json!({ "markdown": { "previewLayout": "previewOnly" } })).unwrap();
    window.steps(5);
    assert_eq!(mode(&mut window), "editorAndPreview", "a file switched keeps its own");
}

#[test]
fn only_a_move_of_the_editor_moves_the_preview() {
    use super::super::markdown_view::follows_editor;
    assert!(follows_editor(Some(12), Some(10), None), "the editor scrolled");
    assert!(!follows_editor(Some(10), Some(10), None), "a hidden or still editor never pulls the preview back");
    assert!(!follows_editor(Some(12), Some(10), Some(12)), "the editor came where the preview asked");
    assert!(!follows_editor(None, None, None));
}

impl Window {
    fn bounds(&mut self, role: Role, name: &str) -> egui::Rect {
        let tree = self.tree();
        let node = find(&tree, role, name).unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        let bounds = node.bounds().expect("bounds");
        egui::Rect::from_min_max(
            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
        )
    }

    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![egui::Event::PointerMoved(from)]);
        self.step(vec![button(from, true)]);
        for step in 1..=8 {
            self.step(vec![egui::Event::PointerMoved(from.lerp(to, step as f32 / 8.0))]);
        }
        self.step(vec![button(to, false)]);
        self.steps(3);
    }

    fn clicks(&mut self, at: egui::Pos2, count: usize) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..count {
            self.step(vec![button(true)]);
            self.step(vec![button(false)]);
        }
        self.steps(3);
    }

    fn selection(&mut self) -> (String, String) {
        let answer = self.command("markdown.preview.getSelection", json!({}));
        (answer["text"].as_str().unwrap().to_owned(), answer["markdown"].as_str().unwrap().to_owned())
    }

    fn history(&mut self) -> String {
        self.command("editor.clipboardHistory.list", json!({})).to_string()
    }
}

#[test]
fn a_drag_selects_across_blocks_without_moving_the_caret_and_copy_takes_it() {
    let mut window = Window::new();
    let caret = window.command("editor.selection.get", json!({}));
    let first = window.bounds(Role::Label, "A family of IDEs in pure Rust");
    let tip = window.bounds(Role::Label, "Open a Markdown file to see this preview");
    window.drag(egui::pos2(first.left() + 1.0, first.top() + 6.0), egui::pos2(tip.right() + 40.0, tip.center().y));
    window.snapshot("selection");
    let (text, markdown) = window.selection();
    assert!(text.starts_with("A family of IDEs in pure Rust, with native rendering, cargo xtask ci"), "{text}");
    assert!(text.ends_with("Open a Markdown file to see this preview beside it."), "{text}");
    assert!(markdown.starts_with("A family of IDEs in **pure Rust**"), "{markdown}");
    assert!(markdown.ends_with("> Open a Markdown file to see this preview beside it."), "{markdown}");
    assert_eq!(window.command("editor.selection.get", json!({})), caret, "a drag never moves the caret");

    window.step(vec![egui::Event::Copy]);
    window.steps(2);
    let history = window.history();
    assert!(history.contains("\"preview\":\"A family of IDEs in pure Rust, with native"), "⌘C copies: {history}");

    window.click(Role::Label, "Setup");
    assert_eq!(window.selection().0, "", "a plain click drops the selection");
    assert_eq!(window.command("editor.selection.get", json!({}))[0]["line"], 10, "and still moves the caret");
}

#[test]
fn a_double_click_takes_a_word_a_triple_click_its_block_and_select_all_everything() {
    let mut window = Window::new();
    let setup = window.bounds(Role::Label, "Setup");
    window.clicks(egui::pos2(setup.left() + 12.0, setup.center().y), 2);
    assert_eq!(window.selection().0, "Setup");
    let tip = window.bounds(Role::Label, "Open a Markdown file to see this preview");
    window.clicks(egui::pos2(tip.left() + 30.0, tip.center().y), 3);
    assert_eq!(window.selection().0, "Open a Markdown file to see this preview beside it.");

    let select_all = egui::Event::Key {
        key: egui::Key::A,
        physical_key: Some(egui::Key::A),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    window.step(vec![select_all]);
    window.steps(2);
    let (text, markdown) = window.selection();
    assert!(text.starts_with("Zen IDE\n\nA family of IDEs"), "{text}");
    for part in [
        "1. Install Zen Toolbox\n",
        "[x] GoGarden\n",
        "Product\tLanguage\tVersion\nGoGarden\tGo\t0.38",
        "fn main() {\n    println!(\"hello\");\n}",
    ] {
        assert!(text.contains(part), "{part:?} in {text}");
    }
    assert_eq!(markdown, README.trim_end(), "Copy as Markdown takes the source lines");
    let editor_text = window.text();
    assert_eq!(editor_text, README, "⌘A in the preview leaves the editor alone");

    let answer = window.command("view.markdownPreview.copyMarkdown", json!({}));
    assert_eq!(answer["result"]["text"], README.trim_end(), "{answer}");
    window.steps(2);
    let history = window.history();
    assert!(history.contains("\"preview\":\"# Zen IDE"), "Copy as Markdown copies: {history}");
}

const MATH: &str = r#"# Maths

The area is $\pi r^2$ and $E = mc^2$ holds.

$$
\sum_{i=1}^{n} i = \frac{n(n+1)}{2}
$$

```math
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
```

| Rule | Formula |
|------|---------|
| Root | $\sqrt{x}$ |

Broken $\nosuch{x}$ inline.

$$
\left( x
$$
"#;

impl Window {
    fn typeset(&mut self) {
        let readme = self.project().join("README.md");
        self.command("editor.text.write", json!({ "path": readme, "text": MATH }));
        self.settled();
        for _ in 0..2000 {
            self.steps(1);
            let slot = self.app.slot_for(None).unwrap();
            let loading = slot.window.lock().unwrap().view.markdown_views.math_loading();
            if !loading {
                self.steps(3);
                return;
            }
        }
        panic!("the formulas are not typeset");
    }
}

#[test]
fn math_is_typeset_inline_in_display_and_in_math_fences_in_both_themes() {
    let mut window = Window::new();
    window.typeset();
    let tree = window.tree();
    for (role, name) in [
        (Role::Math, r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}"),
        (Role::Math, r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
        (Role::Label, r"The area is $\pi r^2$ and $E = mc^2$ holds."),
    ] {
        assert!(find(&tree, role, name).is_some(), "{role:?} {name:?} is drawn");
    }
    let sum = window.bounds(Role::Math, r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}");
    assert!(sum.height() > 40.0, "display math is set with its limits over and under: {sum:?}");
    for theme in ["zen-dark", "zen-light"] {
        window.command("ui.theme.select", json!({ "id": theme }));
        window.typeset();
        window.snapshot(&format!("math-{}", theme.trim_start_matches("zen-")));
    }
    window.command("ui.theme.select", json!({ "id": "zen-dark" }));
}

#[test]
fn math_that_cannot_be_typeset_shows_its_source_with_a_note() {
    let mut window = Window::new();
    window.typeset();
    let tree = window.tree();
    assert!(find(&tree, Role::Label, r"Broken \nosuch{x} inline.").is_some(), "the inline source stays");
    assert!(find(&tree, Role::Math, r"\left( x").is_none());
    let note = zen_i18n::t!("ide-markdown-math-error", error = "");
    assert!(find(&tree, Role::Label, &note).is_some(), "a note says why: {note}");
    window.snapshot("math-error");
}

#[test]
fn selecting_and_copying_typeset_math_takes_its_tex() {
    let mut window = Window::new();
    window.typeset();
    let area = window.bounds(Role::Label, r"The area is $\pi r^2$ and $E = mc^2$ holds.");
    window.clicks(egui::pos2(area.left() + 20.0, area.center().y), 3);
    assert_eq!(window.selection().0, r"The area is $\pi r^2$ and $E = mc^2$ holds.");
    let sum = window.bounds(Role::Math, r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}");
    window.drag(egui::pos2(area.left() + 1.0, area.center().y), egui::pos2(sum.right() + 20.0, sum.center().y));
    window.snapshot("math-selection");
    let (text, markdown) = window.selection();
    assert_eq!(text, "The area is $\\pi r^2$ and $E = mc^2$ holds.\n\n$$ \\sum_{i=1}^{n} i = \\frac{n(n+1)}{2} $$");
    assert!(markdown.starts_with("The area is $\\pi r^2$") && markdown.ends_with("$$"), "{markdown}");
    window.step(vec![egui::Event::Copy]);
    window.steps(2);
    let history = window.history();
    assert!(history.contains(r#""preview":"The area is $\\pi r^2$"#), "{history}");
    window.command("view.markdownPreview.selectAll", json!({}));
    let (all, _) = window.selection();
    for part in
        [r"$$ \begin{pmatrix} a & b \\ c & d \end{pmatrix} $$", "Root\t$\\sqrt{x}$", r"Broken \nosuch{x} inline."]
    {
        assert!(all.contains(part), "{part:?} in {all}");
    }
}
