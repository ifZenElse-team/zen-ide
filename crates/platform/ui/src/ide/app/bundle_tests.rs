use std::sync::{Arc, mpsc};

use egui::accesskit::{Role, TreeUpdate};
use serde_json::{Value, json};
use zen_plugins::native::{Exports, NativePlugin};

use super::*;
use crate::accessibility::testing::find;

struct Harness {
    ctx: egui::Context,
    app: IdeApp,
    time: f64,
    project: PathBuf,
    _data: tempfile::TempDir,
}

fn outline(input: &[u8]) -> Vec<u8> {
    zen_plugin_properties::outline_json(&zen_plugin_properties::entries(std::str::from_utf8(input).unwrap_or("")))
        .into_bytes()
}

impl Harness {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        let resources = project.join("src/main/resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("messages.properties"), "hello=Hello\nbye=Bye\n").unwrap();
        std::fs::write(resources.join("messages_ru.properties"), "hello=Привет\n").unwrap();
        std::fs::write(resources.join("other.properties"), "x=1\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
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
        let app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.enable_accesskit();
        let project = std::fs::canonicalize(&project).unwrap();
        let mut harness = Self { ctx, app, time: 0.0, project, _data: data };
        harness.frame();
        harness
    }

    fn resources(&self) -> PathBuf {
        self.project.join("src/main/resources")
    }

    fn frame(&mut self) -> TreeUpdate {
        self.time += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0))),
            time: Some(self.time),
            ..egui::RawInput::default()
        };
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
        output.platform_output.accesskit_update.expect("the tree is built while AccessKit is on")
    }

    fn settle(&mut self) -> TreeUpdate {
        for _ in 0..4 {
            self.frame();
        }
        self.frame()
    }

    fn ask(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
        let (sender, answer) = mpsc::channel();
        let ctx = self.ctx.clone();
        self.app.route(
            &ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for _ in 0..5 {
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            self.frame();
        }
        answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments} after 5 frames"))
    }

    fn action(&mut self, action: &str, params: Value) -> Result<Value, String> {
        self.ask("run_action", json!({ "action": action, "params": params }))
    }

    fn with_view(&self, change: impl FnOnce(&mut IdeView, &Registry, &mut Session)) {
        let windows = self.app.shared.state().windows.clone();
        let mut window = windows[0].window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *window;
        change(view, &self.app.shared.registry, session);
    }

    fn with_properties_plugin(&self) {
        self.with_view(|_, _, session| {
            let plugin = NativePlugin::new(&["properties"], Exports { outline: Some(outline), ..Exports::default() });
            session.state_mut::<zen_workbench::plugins::PluginFeatures>().set(vec![Arc::new(plugin)]);
        });
    }

    fn expand(&self, folders: &[PathBuf]) {
        let folders = folders.to_vec();
        self.with_view(|view, _, _| {
            view.show_tool("project");
            view.expanded.extend(folders);
        });
    }
}

#[test]
fn the_project_tree_groups_a_bundle_s_files_under_one_node() {
    let mut h = Harness::new();
    h.with_properties_plugin();
    let folders = [h.project.clone(), h.project.join("src"), h.project.join("src/main"), h.resources()];
    h.expand(&folders);
    let tree = h.settle();
    assert!(find(&tree, Role::TreeItem, "Resource Bundle 'messages'").is_some());
    assert!(find(&tree, Role::TreeItem, "other.properties").is_some(), "a lone file stays a file");
    assert!(find(&tree, Role::TreeItem, "messages_ru.properties").is_none(), "inside the closed bundle");
    h.expand(&[super::super::bundle_view::tree_path(&h.resources(), "messages")]);
    let tree = h.settle();
    assert!(find(&tree, Role::TreeItem, "messages_ru.properties").is_some());
    let node = super::super::bundle_view::tree_path(&h.resources(), "messages");
    h.with_view(|view, _, _| {
        view.selection.select(node);
        view.tree_focused = true;
    });
    h.ask("press_keys", json!({ "keys": ["⏎"] })).unwrap();
    h.settle();
    let state = h.ask("get_window_state", json!({})).unwrap();
    assert_eq!(state["resourceBundle"]["rows"], json!(["hello", "bye"]), "Enter opens the bundle: {state}");
}

#[test]
fn without_a_properties_plug_in_the_files_stay_apart() {
    let mut h = Harness::new();
    let folders = [h.project.clone(), h.project.join("src"), h.project.join("src/main"), h.resources()];
    h.expand(&folders);
    let tree = h.settle();
    assert!(find(&tree, Role::TreeItem, "Resource Bundle 'messages'").is_none());
    assert!(find(&tree, Role::TreeItem, "messages_ru.properties").is_some());
}

#[test]
fn the_resource_bundle_editor_opens_as_a_tab_and_follows_changes() {
    let mut h = Harness::new();
    let path = h.resources().join("messages_ru.properties");
    let opened = h.action("view.resourceBundle.open", json!({ "path": path })).unwrap();
    assert_eq!(opened["result"]["rows"], json!(["hello", "bye"]), "{opened}");
    let tree = h.settle();
    assert!(find(&tree, Role::Row, "bye").is_some(), "a row per key");
    let state = h.ask("get_window_state", json!({})).unwrap();
    assert_eq!(state["resourceBundle"]["path"], json!(h.resources().join("messages.properties")), "{state}");
    h.ask(
        "dispatch_command",
        json!({ "command": "properties.bundle.addKey", "params": { "path": path, "key": "new" } }),
    )
    .unwrap();
    h.settle();
    let filtered = h.action("view.resourceBundle.filter", json!({ "missingOnly": true })).unwrap();
    assert_eq!(filtered["result"]["rows"], json!(["bye", "new"]), "{filtered}");
    let refused = h.action("view.resourceBundle.open", json!({ "path": h.resources().join("other.properties") }));
    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn the_resource_bundle_editor_draws_in_both_languages() {
    let mut h = Harness::new();
    let path = h.resources().join("messages.properties");
    h.action("view.resourceBundle.open", json!({ "path": path })).unwrap();
    let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
    h.action("view.resourceBundle.addKey", json!({})).unwrap();
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1000.0, 560.0), 2.0);
    for language in ["en", "ru"] {
        zen_i18n::set_language(Some(language));
        let app = &mut h.app;
        let mut image = None;
        for _ in 0..3 {
            image = Some(screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            }));
        }
        let path = Path::new(&dir).join(format!("resource-bundle-{language}.png"));
        crate::offscreen::write_png(&path, &image.unwrap()).unwrap();
    }
    zen_i18n::set_language(Some("en"));
}
