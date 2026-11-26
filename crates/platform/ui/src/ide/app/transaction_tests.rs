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
        let mut window = Self { app, screen, _data: data };
        window.steps(150);
        {
            let slot = window.app.slot_for(None).unwrap();
            let mut shown = slot.window.lock().unwrap();
            let driver = Arc::new(zen_db::memory::MemoryDriver::new(false));
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
        self.click_at(at);
    }

    fn click_rightmost(&mut self, name: &str) {
        self.settle();
        let tree = self.tree();
        let bounds = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::Button && node.label().is_some_and(|label| label == name))
            .filter_map(|(_, node)| node.bounds())
            .max_by(|a, b| a.x0.total_cmp(&b.x0))
            .unwrap_or_else(|| panic!("no button named {name:?}"));
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
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

    fn popups(&mut self) -> Value {
        self.call("get_window_state", json!({})).unwrap()["popups"].clone()
    }

    fn snapshot(&mut self, name: &str) {
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) {
            let Self { app, screen, .. } = self;
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            crate::offscreen::write_png(&dir.join(format!("transactions-{name}.png")), &image).unwrap();
        }
    }
}

#[test]
fn a_manual_console_shows_its_transaction_and_disconnecting_asks() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").is_some();
    let languages: &[&str] = if snapshots { &["en", "ru"] } else { &["en"] };
    for language in languages {
        zen_i18n::set_language(Some(language));
        let mut window = Window::new();
        zen_i18n::set_language(Some(language));
        let added =
            window.command("db.dataSource.add", json!({ "name": "Shop", "driver": "memory-kv", "password": "pw" }));
        let id = added["id"].as_str().unwrap().to_owned();
        let console = window.command("db.console.new", json!({ "text": "SET a 1\nGET a\nSET b 2\n" }));
        window.command("db.transaction.mode", json!({ "mode": "manual" }));
        let started = window.command("db.console.executeScript", json!({}));
        let done = window.command("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }));
        assert_eq!(done["status"], "done", "{done}");
        window.steps(5);
        let state = window.command("db.console.state", json!({ "path": console["path"] }));
        assert_eq!(state["transaction"]["statements"], 3, "{state}");
        let tree = window.tree();
        assert!(find(&tree, Role::Button, &t!("ide-dbtx-log-open")).is_some(), "the counter opens the log");
        for theme in ["zen-dark", "zen-light"] {
            window.command("ui.theme.select", json!({ "id": theme }));
            window.steps(3);
            window.snapshot(&format!("toolbar-{}-{language}", theme.trim_start_matches("zen-")));
        }
        window.click(Role::Button, &t!("ide-dbtx-log-open"));
        assert_eq!(window.popups(), json!(["transactionLog"]));
        window.click(Role::Button, &t!("ide-dbtx-close"));
        window.command("workspace.tab.select", json!({ "workspace": "database", "tab": "overview" }));
        window.steps(3);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        window.steps(3);
        window.click_rightmost(&t!("ide-database-disconnect"));
        let status = window.command("db.connection.status", json!({ "connection": id }));
        assert_eq!(window.popups(), json!(["transactionQuestion"]), "{status}");
        for theme in ["zen-dark", "zen-light"] {
            window.command("ui.theme.select", json!({ "id": theme }));
            window.steps(3);
            window.snapshot(&format!("disconnect-{}-{language}", theme.trim_start_matches("zen-")));
        }
        window.click(Role::Button, &t!("ide-dbtx-rollback"));
        assert_eq!(window.popups(), json!([]));
        let status = window.command("db.connection.status", json!({ "connection": id }));
        let state = status["connections"][0]["state"].clone();
        assert!(state == "disconnected" || state == "connected", "{status}");
        window.command("ui.theme.select", json!({ "id": "zen-dark" }));
    }
    zen_i18n::set_language(Some("en"));
}
