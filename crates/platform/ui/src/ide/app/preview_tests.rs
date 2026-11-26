use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::{Value, json};

use super::*;

const SIZE: egui::Vec2 = egui::vec2(1100.0, 700.0);

const ICON: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"48\" height=\"48\" viewBox=\"0 0 48 48\">\n  <circle cx=\"24\" cy=\"24\" r=\"20\" fill=\"#3d8fd1\"/>\n  <rect x=\"14\" y=\"14\" width=\"20\" height=\"20\" fill=\"#f2a541\" opacity=\"0.8\"/>\n</svg>\n";

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
    }
}

impl Window {
    fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        std::fs::create_dir(data.path().join("project")).unwrap();
        std::fs::write(data.path().join("project/icon.svg"), ICON).unwrap();
        Self::in_data(data)
    }

    fn in_data(data: tempfile::TempDir) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let project = data.path().join("project");
        let app = IdeApp::new(product(), registry, Some(data.path().join("recent.json")), vec![project]);
        let screen = crate::offscreen::Offscreen::new(SIZE, 1.0);
        let mut window = Self { app, screen, data };
        window.steps(5);
        let icon = window.icon();
        window.command("file.document.open", json!({ "path": icon }));
        window.steps(5);
        window
    }

    fn icon(&self) -> PathBuf {
        self.data.path().join("project/icon.svg")
    }

    fn steps(&mut self, count: usize) {
        for _ in 0..count {
            let Self { app, screen, .. } = self;
            screen.step(Vec::new(), |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        }
    }

    fn render(&mut self) -> crate::offscreen::Frame {
        let Self { app, screen, .. } = self;
        screen.render(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
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

    fn state(&mut self) -> Value {
        self.call("get_window_state", json!({})).unwrap()
    }

    fn settled(&mut self) -> Value {
        for _ in 0..500 {
            let answer = self.command("editor.preview.getMode", json!({}));
            if answer["state"] != "rendering" {
                self.steps(3);
                return answer;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.steps(1);
        }
        panic!("the preview is not drawn");
    }

    fn snapshot(&mut self, name: &str, language: &str) {
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) {
            let image = self.render();
            crate::offscreen::write_png(&dir.join(format!("preview-{name}-{language}.png")), &image).unwrap();
        }
    }
}

#[test]
fn an_svg_opens_as_text_with_its_preview_and_the_switch_changes_and_keeps_the_mode() {
    let languages: &[&str] = if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { &["en", "ru"] } else { &["en"] };
    for language in languages {
        zen_i18n::set_language(Some(language));
        let mut window = Window::new();
        let state = window.state();
        assert!(state["editor"].as_str().unwrap().ends_with("/project/icon.svg"), "{state}");
        assert_eq!(
            state["preview"],
            json!({ "kind": "svg", "mode": "editorAndPreview", "orientation": "vertical", "scope": "default" }),
            "{state}"
        );
        let answer = window.settled();
        assert_eq!((&answer["state"], &answer["size"]), (&json!("ready"), &json!([48.0, 48.0])));
        window.snapshot("editor-and-preview", language);

        for (action, mode) in [
            ("editor.preview.previewOnly", "previewOnly"),
            ("editor.preview.editorOnly", "editorOnly"),
            ("editor.preview.editorAndPreview", "editorAndPreview"),
        ] {
            let ran = window.call("run_action", json!({ "action": action })).unwrap();
            assert_eq!(ran["window"]["preview"]["mode"], mode, "{ran}");
            assert_eq!(ran["window"]["preview"]["scope"], "file", "the switch sets the file's own mode");
            window.steps(3);
            let name = mode.replace("And", "-and-").replace("Only", "-only").to_lowercase();
            window.snapshot(&name, language);
        }
        window.command("editor.preview.setMode", json!({ "mode": "previewOnly" }));
        let data = std::mem::replace(&mut window.data, tempfile::tempdir().unwrap());
        drop(window);
        let mut again = Window::in_data(data);
        assert_eq!(again.state()["preview"]["mode"], "previewOnly", "kept for the file");
        again.command("editor.preview.setMode", json!({ "mode": "editorAndPreview" }));
    }
    zen_i18n::set_language(Some("en"));
}

#[test]
fn the_preview_follows_unsaved_edits_and_tells_why_a_broken_svg_is_not_shown() {
    let mut window = Window::new();
    window.settled();
    let icon = window.icon();
    let wider = ICON.replace(
        "width=\"48\" height=\"48\" viewBox=\"0 0 48 48\"",
        "width=\"96\" height=\"48\" viewBox=\"0 0 96 48\"",
    );
    window.command("editor.text.write", json!({ "path": icon, "text": wider, "save": false }));
    let answer = window.settled();
    assert_eq!(answer["size"], json!([96.0, 48.0]), "{answer}");
    window.command(
        "editor.text.write",
        json!({ "path": icon, "text": "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle", "save": false }),
    );
    let answer = window.settled();
    assert_eq!(answer["state"], "invalid", "{answer}");
    window.snapshot("invalid", "en");
    let png = window.data.path().join("project/dot.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
    window.command("file.document.open", json!({ "path": png }));
    assert_eq!(window.state()["preview"], json!(null));
}

#[test]
fn the_orientation_switch_stacks_the_editor_over_the_preview_and_the_file_keeps_it() {
    let mut window = Window::new();
    window.settled();
    let ran = window.call("run_action", json!({ "action": "editor.preview.toggleOrientation" })).unwrap();
    assert_eq!(ran["window"]["preview"]["orientation"], "horizontal", "{ran}");
    window.steps(5);
    window.snapshot("split-horizontal", "en");
    let data = std::mem::replace(&mut window.data, tempfile::tempdir().unwrap());
    drop(window);
    let mut again = Window::in_data(data);
    assert_eq!(again.state()["preview"]["orientation"], "horizontal", "kept for the file");
    let set = again.command("editor.preview.setOrientation", json!({ "orientation": "vertical" }));
    assert_eq!(set["orientation"], "vertical", "{set}");
}

#[test]
fn find_action_lists_the_three_modes() {
    let mut window = Window::new();
    for (query, id) in [
        ("editor only", "editor.preview.editorOnly"),
        ("editor and preview", "editor.preview.editorAndPreview"),
        ("preview only", "editor.preview.previewOnly"),
        ("change split orientation", "editor.preview.toggleOrientation"),
    ] {
        let found = window.call("find_action", json!({ "query": query, "limit": 8 })).unwrap();
        assert!(found["actions"].as_array().unwrap().iter().any(|entry| entry["action"] == id), "{query}: {found}");
    }
}
