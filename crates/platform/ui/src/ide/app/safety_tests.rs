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

    fn has(&mut self, role: Role, name: &str) -> bool {
        self.settle();
        let tree = self.tree();
        find(&tree, role, name).is_some()
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
            crate::offscreen::write_png(&dir.join(format!("safety-{name}.png")), &image).unwrap();
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
}

#[test]
fn production_asks_statement_by_statement_and_a_risk_needs_its_tick() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").is_some();
    let languages: &[&str] = if snapshots { &["en", "ru"] } else { &["en"] };
    for language in languages {
        zen_i18n::set_language(Some(language));
        let mut window = Window::new();
        zen_i18n::set_language(Some(language));
        let added = window.command(
            "db.dataSource.add",
            json!({ "name": "Shop", "driver": zen_db::memory::SQL, "password": "pw", "type": "prod" }),
        );
        let id = added["id"].as_str().unwrap().to_owned();
        let text = "SELECT 1;\nDELETE FROM orders;\nUPDATE orders SET paid = true WHERE id = 1;\n";
        let console = window.command("db.console.new", json!({ "connection": id, "text": text }));
        let path = console["path"].as_str().unwrap().to_owned();
        window.steps(5);
        let state = window.command("db.console.state", json!({}));
        assert_eq!(state["safety"]["typeName"], "Production", "{state}");
        window.themed("console", language);

        window.click(Role::Button, &t!("ide-database-execute-script"));
        assert_eq!(window.popups(), json!(["executeConfirmation"]));
        assert!(window.executed().is_empty(), "nothing runs before the answer");
        window.press_enter(egui::Modifiers::COMMAND);
        window.press_enter(egui::Modifiers::NONE);
        assert_eq!(window.popups(), json!(["executeConfirmation"]), "Enter and ⌘⏎ never answer");
        assert!(window.executed().is_empty(), "{:?}", window.executed());
        let shown = window.call("get_document_text", json!({ "path": path })).unwrap();
        assert_eq!(shown["text"], text, "nothing typed into the console behind the question: {shown}");
        window.themed("danger", language);
        let run_anyway = t!("ide-dbsafety-run-anyway");
        window.click(Role::Button, &run_anyway);
        assert!(window.executed().is_empty(), "the risk is not ticked yet");
        let risk = zen_workbench::database::safety::risk_text(&zen_db::safety::Danger {
            risk: zen_db::safety::Risk::DeleteWithoutWhere,
            object: Some("orders".to_owned()),
        });
        window.click(Role::CheckBox, &t!("ide-dbsafety-understood", risk = risk.as_str()));
        window.click(Role::Button, &run_anyway);
        assert_eq!(window.popups(), json!(["executeConfirmation"]), "the UPDATE asks next");
        assert!(window.has(Role::Button, &t!("ide-dbsafety-skip")));
        window.themed("confirm", language);
        window.click(Role::Button, &t!("ide-dbsafety-skip"));
        assert_eq!(window.popups(), json!([]));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !window.executed().iter().any(|text| text == "DELETE FROM orders") && std::time::Instant::now() < deadline
        {
            window.steps(1);
        }
        let executed = window.executed();
        assert!(executed.iter().any(|text| text == "DELETE FROM orders"), "{executed:?}");
        assert!(!executed.iter().any(|text| text.starts_with("UPDATE")), "the skipped UPDATE never ran: {executed:?}");

        window.click(Role::Button, &t!("ide-dbsafety-read-only-off"));
        let source = window.command("db.dataSource.get", json!({ "id": id }));
        assert_eq!(source["readOnly"], true, "{source}");
        window.steps(3);
        assert!(window.has(Role::Button, &t!("ide-dbsafety-read-only-on")));
        window.themed("read-only", language);
        window.click(Role::Button, &t!("ide-dbsafety-read-only-on"));
        assert_eq!(window.command("db.dataSource.get", json!({ "id": id }))["readOnly"], false);
    }
    zen_i18n::set_language(Some("en"));
}

#[test]
fn a_connection_made_production_while_connected_asks_and_says_the_change_stays_in_the_transaction() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").is_some();
    let languages: &[&str] = if snapshots { &["en", "ru"] } else { &["en"] };
    for language in languages {
        zen_i18n::set_language(Some(language));
        let mut window = Window::new();
        zen_i18n::set_language(Some(language));
        let added = window.command(
            "db.dataSource.add",
            json!({ "name": "Shop", "driver": zen_db::memory::SQL, "password": "pw", "type": "dev" }),
        );
        let id = added["id"].as_str().unwrap().to_owned();
        let text = "UPDATE orders SET paid = true WHERE id = 1;\n";
        let console = window.command("db.console.new", json!({ "connection": id, "text": text }));
        let path = console["path"].as_str().unwrap().to_owned();
        window.command("db.connection.connect", json!({ "connection": id }));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while window.command("db.connection.status", json!({ "connection": id }))["state"] != "connected"
            && std::time::Instant::now() < deadline
        {
            window.steps(1);
        }
        assert_eq!(window.command("db.transaction.status", json!({ "path": path }))["autoCommit"], true);
        window.command("db.dataSource.update", json!({ "id": id, "type": "prod" }));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while window.command("db.transaction.status", json!({ "path": path }))["autoCommit"] != false
            && std::time::Instant::now() < deadline
        {
            window.steps(1);
        }
        let status = window.command("db.transaction.status", json!({ "path": path }));
        assert_eq!(
            (status["mode"].as_str(), status["autoCommit"].as_bool()),
            (Some("manual"), Some(false)),
            "{status}"
        );
        window.steps(3);
        window.click(Role::Button, &t!("ide-database-execute-script"));
        assert_eq!(window.popups(), json!(["executeConfirmation"]));
        assert!(window.executed().iter().all(|text| !text.starts_with("UPDATE")), "nothing runs before the answer");
        let tree = window.tree();
        let stays = t!("database-confirm-commit", commit = "manual");
        let shown = tree.nodes.iter().any(|(_, node)| {
            node.label().is_some_and(|label| label.contains(&stays))
                || node.value().is_some_and(|value| value.contains(&stays))
        });
        assert!(shown, "the question says the change stays in the transaction");
        window.themed("production-now", language);
        if snapshots {
            window.screen.set_size(egui::vec2(820.0, 640.0));
            window.steps(3);
            window.themed("production-now-narrow", language);
        }
    }
    zen_i18n::set_language(Some("en"));
}

#[test]
fn a_console_without_a_connection_tells_a_person_what_to_choose_without_command_ids() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").is_some();
    let languages: &[&str] = if snapshots { &["en", "ru"] } else { &["en"] };
    for language in languages {
        zen_i18n::set_language(Some(language));
        let mut window = Window::new();
        zen_i18n::set_language(Some(language));
        window.command(
            "db.dataSource.add",
            json!({ "name": "Shop", "driver": zen_db::memory::SQL, "password": "pw", "type": "dev" }),
        );
        window.command(
            "db.dataSource.add",
            json!({ "name": "Archive", "driver": zen_db::memory::SQL, "password": "pw", "type": "dev" }),
        );
        window.command("db.console.new", json!({ "text": "SELECT 1;\n" }));
        window.steps(5);
        window.press_enter(egui::Modifiers::COMMAND);
        window.steps(5);
        let wanted = t!("database-error-no-console-connection");
        let tree = window.tree();
        let texts: Vec<String> =
            tree.nodes.iter().filter_map(|(_, node)| node.label().or(node.value()).map(str::to_owned)).collect();
        assert!(texts.iter().any(|text| text.contains(&wanted)), "{wanted} in {texts:?}");
        assert!(!texts.iter().any(|text| text.contains("db.console.")), "{texts:?}");
        window.themed("no-connection", language);
        if snapshots {
            window.screen.set_size(egui::vec2(820.0, 640.0));
            window.steps(3);
            window.themed("no-connection-narrow", language);
        }
    }
    zen_i18n::set_language(Some("en"));
}
