use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::Registry;
use zen_i18n::t;
use zen_ui::ide::{IdeApp, IdeProduct};
use zen_ui::offscreen::Offscreen;

const GO: &str = r#"// Copyright 2026 Fictional Holder Ltd, an invented name.
// Licensed under the Apache License, Version 2.0.

// Package server serves the example.
package server

import (
	"errors"
	"fmt"
)

type Empty struct {
}

// Start runs the server.
func Start(name string, port int) error {
	if err := listen(port); err != nil {
		return fmt.Errorf("start %s on %d: %w", name, port, err)
	}
	switch port {
	case 80:
		return errors.New("privileged")
	default:
		fmt.Printf("port %d\n", port)
	}
	return nil
}

func Name() string {
	return "server"
}

func noop() {
}

func listen(int) error {
	return nil
}
"#;

struct Ide {
    app: IdeApp,
    screen: Offscreen,
    url: String,
    calls: u64,
    snapshots: Option<PathBuf>,
}

impl Ide {
    fn new(data: &Path, project: &Path) -> Self {
        zen_i18n::register(zen_ui::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::register(zen_lang_go::LOCALES);
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct {
            name: "GoGarden",
            id: "gogarden",
            version: "0.0.0",
            highlighters: vec![Arc::new(zen_lang_go::GoHighlighter), Arc::new(zen_lang_rust::RustHighlighter)],
            folding_options: vec![zen_lang_go::module::FOLDING],
            ..IdeProduct::default()
        };
        let mut app = IdeApp::in_safe_mode(product, registry, Some(data.join("recent.json")), vec![project.to_owned()]);
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        app.enable_automation(port);
        let screen = Offscreen::new(eframe::egui::vec2(1100.0, 720.0), 1.0);
        app.palette().apply(screen.context());
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let mut ide = Self { app, screen, url: format!("127.0.0.1:{port}"), calls: 0, snapshots };
        for _ in 0..7 * 60 {
            ide.step(Vec::new());
        }
        ide
    }

    fn step(&mut self, events: Vec<eframe::egui::Event>) {
        let app = &mut self.app;
        self.screen.step(events, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
    }

    fn ask(&mut self, tool: &str, arguments: Value) -> Value {
        self.calls += 1;
        let (address, id, tool_name) = (self.url.clone(), self.calls, tool.to_owned());
        let agent = std::thread::spawn(move || call(&address, id, &tool_name, arguments));
        let started = std::time::Instant::now();
        while !agent.is_finished() {
            assert!(started.elapsed().as_secs() < 60, "no answer to {tool}");
            self.step(Vec::new());
        }
        let answer = agent.join().unwrap();
        assert_eq!(answer["result"]["isError"], false, "{tool}: {answer}");
        let text = answer["result"]["content"][0]["text"].as_str().unwrap_or("null");
        serde_json::from_str(text).unwrap_or(Value::String(text.to_owned()))
    }

    fn command(&mut self, command: &str, params: Value) -> Value {
        self.ask("dispatch_command", json!({ "command": command, "params": params }))
    }

    fn snapshot(&mut self, name: &str) {
        let app = &mut self.app;
        let image = self.screen.frame(Vec::new(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        if let Some(dir) = &self.snapshots {
            zen_ui::offscreen::write_png(&dir.join(format!("{name}.png")), &image).unwrap();
        }
    }

    fn arrows(&mut self) -> usize {
        let app = &mut self.app;
        let tree = self.screen.accessibility_tree(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        tree.nodes.iter().filter(|(_, node)| node.role() == eframe::egui::accesskit::Role::DisclosureTriangle).count()
    }
}

fn call(address: &str, id: u64, tool: &str, arguments: Value) -> Value {
    let body = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": arguments } })
        .to_string();
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (_, json) = response.split_once("\r\n\r\n").unwrap();
    serde_json::from_str(json).unwrap()
}

fn go_options() -> Vec<&'static str> {
    zen_lang_go::module::FOLDING.options.iter().map(|option| option.key).collect()
}

#[test]
fn the_code_folding_options_fold_a_go_file_draw_the_arrows_and_are_found_in_settings() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let file = project.join("server.go");
    std::fs::write(&file, GO).unwrap();
    let mut ide = Ide::new(data.path(), &project);

    let declared: serde_json::Map<String, Value> =
        go_options().into_iter().map(|key| (key.to_owned(), json!(true))).collect();
    let applied = ide.ask(
        "set_settings",
        json!({ "language": "en", "declaredSettings": declared, "foldFileHeader": true, "foldArrows": "always", "foldBottomArrows": true }),
    );
    assert_eq!((applied["foldArrows"].clone(), applied["foldBottomArrows"].clone()), (json!("always"), json!(true)));
    let options = ide.command("editor.folding.options", json!({}));
    assert_eq!((options["arrows"].clone(), options["bottomArrows"].clone()), (json!("always"), json!(true)));
    let on: Vec<&str> = options["groups"][1]["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|option| option["value"] == true)
        .map(|option| option["key"].as_str().unwrap())
        .collect();
    assert_eq!(on, go_options());

    ide.command("file.document.open", json!({ "path": file }));
    let folds = ide.command("editor.fold.list", json!({}));
    let folded: Vec<&str> = folds
        .as_array()
        .unwrap()
        .iter()
        .filter(|fold| fold["folded"] == true)
        .map(|fold| fold["option"].as_str().unwrap())
        .collect();
    assert_eq!(
        folded,
        [
            zen_modules::folding::FILE_HEADER,
            zen_modules::folding::IMPORTS,
            zen_lang_go::module::EMPTY_TYPES,
            zen_lang_go::module::IF_ERRORS,
            zen_lang_go::module::FORMATTED_STRINGS,
            zen_lang_go::module::ONE_LINE_CASES,
            zen_lang_go::module::ONE_LINE_CASES,
            zen_lang_go::module::FORMATTED_STRINGS,
            zen_lang_go::module::ONE_LINE_RETURNS,
            zen_lang_go::module::EMPTY_FUNCTIONS,
            zen_lang_go::module::ONE_LINE_RETURNS,
        ],
        "{folds}"
    );
    let multi_line = folds.as_array().unwrap().iter().filter(|fold| fold["endLine"] != fold["startLine"]).count();
    let unfolded_multi_line = folds
        .as_array()
        .unwrap()
        .iter()
        .filter(|fold| fold["endLine"] != fold["startLine"] && fold["folded"] == false)
        .count();
    for _ in 0..3 {
        ide.step(Vec::new());
    }
    assert_eq!(
        ide.arrows(),
        multi_line + unfolded_multi_line,
        "always: every region's arrow and the bottom ones of the open regions"
    );

    ide.ask("set_settings", json!({ "foldArrows": "onHover", "foldBottomArrows": false }));
    let folded_multi_line = folds
        .as_array()
        .unwrap()
        .iter()
        .filter(|fold| fold["endLine"] != fold["startLine"] && fold["folded"] == true)
        .count();
    assert_eq!(
        ide.arrows(),
        folded_multi_line,
        "on mouse hover, away from the gutter: only the folded regions' arrows"
    );

    for language in ["en", "ru"] {
        ide.ask("set_settings", json!({ "language": language, "foldArrows": "always", "foldBottomArrows": true }));
        ide.snapshot(&format!("ide-go-folded-always-bottom-{language}"));
        ide.ask("set_settings", json!({ "foldArrows": "onHover", "foldBottomArrows": false }));
        ide.snapshot(&format!("ide-go-folded-hover-{language}"));
        let state =
            ide.ask("run_action", json!({ "action": "view.settings.open", "params": { "page": "codeFolding" } }));
        assert_eq!(state["window"]["popups"], json!(["settings:codeFolding"]), "{state}");
        for _ in 0..30 {
            ide.step(Vec::new());
        }
        ide.snapshot(&format!("ide-settings-code-folding-{language}"));
        let labels = [
            "settings-folding-show-arrows",
            "settings-folding-bottom-arrows",
            "settings-folding-by-default",
            "settings-folding-file-header",
            "settings-folding-imports",
            "settings-folding-doc-comments",
            "settings-folding-method-bodies",
            "settings-folding-custom-regions",
        ]
        .into_iter()
        .chain(zen_lang_go::module::FOLDING.options.iter().map(|option| option.label));
        ide.ask("press_keys", json!({ "keys": ["⎋"] }));
        for key in labels {
            let label = t!(key);
            let state =
                ide.ask("run_action", json!({ "action": "view.settings.open", "params": { "page": "appearance" } }));
            assert_eq!(state["window"]["popups"], json!(["settings:appearance"]), "{state}");
            ide.ask("press_keys", json!({ "keys": ["⌘F"], "text": label.trim_end_matches(':') }));
            ide.step(Vec::new());
            let shown = ide.ask("get_window_state", json!({}));
            let popups = &shown["popups"];
            assert_eq!(popups, &json!(["settings:codeFolding"]), "{language}: {label} goes to Code Folding: {shown}");
            if key == "settings-go-fold-formatted-strings" {
                for _ in 0..30 {
                    ide.step(Vec::new());
                }
                ide.snapshot(&format!("ide-settings-search-go-{language}"));
            }
            ide.ask("press_keys", json!({ "keys": ["⎋"] }));
            ide.ask("press_keys", json!({ "keys": ["⎋"] }));
            ide.step(Vec::new());
            assert_eq!(ide.ask("get_window_state", json!({}))["popups"], json!([]), "{language}: the dialog closes");
        }
    }
    ide.ask("set_settings", json!({ "language": "en" }));

    let only_strings: serde_json::Map<String, Value> = go_options()
        .into_iter()
        .map(|key| (key.to_owned(), json!(key == zen_lang_go::module::FORMATTED_STRINGS)))
        .collect();
    ide.ask("set_settings", json!({ "declaredSettings": only_strings }));
    let strings = project.join("report.go");
    std::fs::write(
        &strings,
        "package server\n\nimport \"fmt\"\n\nfunc report(name string, done, total int) string {\n\tfmt.Printf(\"%s: %d of %d\\n\", name, done, total)\n\treturn fmt.Sprintf(\"%.1f%%\", float64(done)/float64(total)*100)\n}\n",
    )
    .unwrap();
    ide.command("file.document.open", json!({ "path": strings }));
    let folds = ide.command("editor.fold.list", json!({}));
    let shown: Vec<&str> = folds
        .as_array()
        .unwrap()
        .iter()
        .filter(|fold| fold["folded"] == true)
        .map(|fold| fold["placeholder"].as_str().unwrap())
        .collect();
    assert_eq!(shown, ["\"{name}: {done} of {total}\\n\"", "\"{float64(done)/float64(total)*100}%\""]);
    ide.snapshot("ide-go-formatted-strings-en");
}

fn folded_now(ide: &mut Ide) -> Vec<(u64, u64, String)> {
    let folds = ide.command("editor.fold.list", json!({}));
    folds
        .as_array()
        .unwrap()
        .iter()
        .filter(|fold| fold["folded"] == true)
        .map(|fold| {
            let tag = fold["option"].as_str().unwrap_or(fold["kind"].as_str().unwrap()).to_owned();
            (fold["start"].as_u64().unwrap(), fold["end"].as_u64().unwrap(), tag)
        })
        .collect()
}

#[test]
fn the_folds_the_user_changed_come_back_after_a_restart_instead_of_the_options() {
    let data = tempfile::tempdir().unwrap();
    let project = data.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let file = project.join("server.go");
    std::fs::write(&file, GO).unwrap();
    let kept = {
        let mut ide = Ide::new(data.path(), &project);
        let declared: serde_json::Map<String, Value> =
            go_options().into_iter().map(|key| (key.to_owned(), json!(true))).collect();
        ide.ask("set_settings", json!({ "language": "en", "declaredSettings": declared, "foldFileHeader": true }));
        ide.command("file.document.open", json!({ "path": file }));
        let first = folded_now(&mut ide);
        assert!(first.iter().any(|(.., tag)| tag == zen_lang_go::module::FORMATTED_STRINGS), "{first:?}");
        for (start, _, tag) in &first {
            if tag != zen_modules::folding::IMPORTS {
                ide.command("editor.fold.toggle", json!({ "start": start }));
            }
        }
        let body = GO.find("\n\tif err := listen").unwrap();
        ide.command("editor.fold.toggle", json!({ "start": body }));
        let name = GO.find("\"server\"").unwrap();
        ide.command("editor.selection.set", json!({ "selections": [{ "anchor": name, "head": name + 8 }] }));
        ide.command("editor.fold.selection", json!({}));
        let kept = folded_now(&mut ide);
        assert_eq!(
            kept.iter().map(|(.., tag)| tag.as_str()).collect::<Vec<_>>(),
            [zen_modules::folding::IMPORTS, zen_modules::folding::METHOD_BODIES, "manual"],
            "{kept:?}"
        );
        for _ in 0..90 {
            ide.step(Vec::new());
        }
        kept
    };
    let mut ide = Ide::new(data.path(), &project);
    let options = ide.command("editor.folding.options", json!({}));
    assert_eq!(options["groups"][1]["options"][0]["value"], true, "the options are still on: {options}");
    ide.command("file.document.open", json!({ "path": file }));
    assert_eq!(folded_now(&mut ide), kept, "the user's folds, not the options'");
}
