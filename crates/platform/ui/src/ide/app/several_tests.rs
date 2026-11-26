use std::path::PathBuf;
use std::sync::mpsc;

use egui::accesskit::{Role, TreeUpdate};
use serde_json::{Value, json};
use zen_i18n::t;

use super::tests::{test_product, window_project};
use super::*;
use crate::accessibility::testing::find;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 720.0);

struct Window {
    app: IdeApp,
    screen: crate::offscreen::Offscreen,
    driver: Arc<zen_db::memory::MemoryDriver>,
    _data: tempfile::TempDir,
}

impl Window {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(data.path()).unwrap();
        window_project(&root);
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct {
            modules: vec![zen_modules::BuiltinModule::on(&zen_workbench::database::MODULE)],
            ..test_product()
        };
        let app = IdeApp::new(product, registry, Some(root.join("recent.json")), vec![root.join("project")]);
        app.shared.settings.lock().unwrap().autosave_on_idle = false;
        let screen = crate::offscreen::Offscreen::new(SIZE, 1.0);
        let driver = Arc::new(zen_db::memory::MemoryDriver::sql());
        let mut window = Self { app, screen, driver: Arc::clone(&driver), _data: data };
        window.steps(150);
        {
            let slot = window.app.slot_for(None).unwrap();
            let mut shown = slot.window.lock().unwrap();
            let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![driver];
            shown.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
            let secrets = Arc::new(zen_secrets::MemoryStore::new());
            shown.session.set_state(zen_workbench::database::Secrets(secrets));
        }
        window
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

    fn settle(&mut self) {
        let Self { app, screen, .. } = self;
        drop(screen.render(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        }));
    }

    fn click(&mut self, role: Role, name: &str) {
        self.settle();
        let tree = self.tree();
        let node = find(&tree, role, name).unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        let bounds = node.bounds().expect("bounds");
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
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

    fn press_enter(&mut self, modifiers: egui::Modifiers) {
        let key =
            |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers };
        self.step(vec![key(true)]);
        self.step(vec![key(false)]);
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

    fn popups(&mut self) -> Value {
        self.call("get_window_state", json!({})).unwrap()["popups"].clone()
    }

    fn executed(&self) -> Vec<String> {
        self.driver.server.executed()
    }

    fn snapshot(&mut self, name: &str) {
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) {
            let Self { app, screen, .. } = self;
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            crate::offscreen::write_png(&dir.join(format!("several-{name}.png")), &image).unwrap();
        }
    }

    fn themed(&mut self, what: &str, language: &str) {
        for theme in ["zen-dark", "zen-light"] {
            self.command("ui.theme.select", json!({ "id": theme }));
            self.steps(3);
            self.snapshot(&format!("{what}-{}-{language}", theme.trim_start_matches("zen-")));
        }
        self.command("ui.theme.select", json!({ "id": "zen-dark" }));
        self.steps(3);
    }

    fn connection(&mut self, name: &str, kind: &str) -> String {
        let added = self.command(
            "db.dataSource.add",
            json!({ "name": name, "driver": zen_db::memory::SQL, "password": "pw", "type": kind }),
        );
        added["id"].as_str().unwrap().to_owned()
    }
}

#[test]
fn a_script_runs_on_ticked_connections_after_one_confirmation_for_all_of_them() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").is_some();
    let languages: &[&str] = if snapshots { &["en", "ru"] } else { &["en"] };
    for language in languages {
        zen_i18n::set_language(Some(language));
        let mut window = Window::new();
        zen_i18n::set_language(Some(language));
        let eu = window.connection("Orders EU", "dev");
        window.connection("Orders US", "test");
        window.connection("Orders Live", "prod");
        let text = "SELECT 1;\nDEL cache;\n";
        let console = window.command("db.console.new", json!({ "connection": eu, "text": text }));
        let path = console["path"].as_str().unwrap().to_owned();
        window.steps(5);

        window.click(Role::Button, &t!("ide-database-more-actions"));
        window.click(Role::Button, &t!("ide-database-execute-several"));
        assert_eq!(window.popups(), json!(["severalConnections"]));
        window.click(Role::CheckBox, "Orders US");
        window.click(Role::CheckBox, "Orders Live");
        window.themed("choose", language);
        let run = t!("ide-several-run", count = 3);
        window.click(Role::Button, &run);
        assert_eq!(window.popups(), json!(["severalConnections"]), "Production asks first");
        assert!(window.executed().is_empty(), "nothing runs before the answer: {:?}", window.executed());
        window.press_enter(egui::Modifiers::COMMAND);
        window.press_enter(egui::Modifiers::NONE);
        assert_eq!(window.popups(), json!(["severalConnections"]), "Enter and ⌘⏎ never answer");
        assert!(window.executed().is_empty());
        let shown = window.call("get_document_text", json!({ "path": path })).unwrap();
        assert_eq!(shown["text"], text, "nothing typed into the console behind the dialog: {shown}");
        window.themed("confirm", language);

        window.click(Role::Button, &run);
        assert_eq!(window.popups(), json!([]));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut state = window.command("db.script.connectionsState", json!({ "path": path }));
        while state["running"] == true && std::time::Instant::now() < deadline {
            window.steps(1);
            state = window.command("db.script.connectionsState", json!({ "path": path }));
        }
        let statuses: Vec<&str> =
            state["targets"].as_array().unwrap().iter().map(|target| target["status"].as_str().unwrap()).collect();
        assert_eq!(statuses, ["done", "done", "done"], "{state}");
        assert_eq!(window.executed().iter().filter(|text| text.starts_with("DEL")).count(), 3);
        window.steps(5);
        let tab = t!("ide-database-connections-tab");
        assert!(find(&window.tree(), Role::Tab, &tab).is_some(), "the results panel has the Connections tab");
        window.themed("results", language);
    }
    zen_i18n::set_language(Some("en"));
}
