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

    fn press_enter(&mut self) {
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![key(true)]);
        self.step(vec![key(false)]);
        self.steps(3);
    }

    fn type_text(&mut self, text: &str) {
        self.step(vec![egui::Event::Text(text.to_owned())]);
        self.steps(2);
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

    fn themed(&mut self, what: &str, language: &str) {
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) else { return };
        for theme in ["zen-dark", "zen-light"] {
            self.command("ui.theme.select", json!({ "id": theme }));
            self.steps(3);
            let Self { app, screen, .. } = self;
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            let name = format!("parameters-{what}-{}-{language}.png", theme.trim_start_matches("zen-"));
            crate::offscreen::write_png(&dir.join(name), &image).unwrap();
        }
        self.command("ui.theme.select", json!({ "id": "zen-dark" }));
        self.steps(3);
    }
}

#[test]
fn the_dialog_asks_typed_values_runs_them_bound_and_the_variables_tab_keeps_them() {
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
        let text = "@set tbl = customers\nSELECT * FROM ${tbl} WHERE id = :id AND name = :name;\n";
        window.command("db.console.new", json!({ "connection": id, "text": text }));
        window.command("db.console.setParameter", json!({ "name": "name", "value": "Ann" }));
        window.steps(5);

        window.click(Role::Button, &t!("ide-database-execute-script"));
        assert_eq!(window.popups(), json!(["bindParameters"]));
        assert!(window.driver.server.bound().is_empty(), "nothing runs before the values are given");
        window.click(Role::TextInput, &t!("ide-dbparams-value-of", name = ":id"));
        window.type_text("42");
        window.click(Role::ComboBox, &t!("ide-dbparams-type-of", name = ":id"));
        window.click(Role::Button, &t!("ide-dbparams-type-number"));
        window.themed("dialog", language);
        window.press_enter();
        assert_eq!(window.popups(), json!([]));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while window.driver.server.bound().is_empty() && std::time::Instant::now() < deadline {
            window.steps(1);
        }
        let bound = window.driver.server.bound();
        assert_eq!(
            bound.last().unwrap(),
            &(
                "SELECT * FROM customers WHERE id = $1 AND name = $2".to_owned(),
                vec![Some("42".to_owned()), Some("Ann".to_owned())]
            )
        );

        window.click(Role::Tab, &t!("ide-database-variables"));
        window.steps(3);
        let kept = window.command("db.console.variables", json!({}));
        assert_eq!(kept["variables"][0]["value"], "customers", "{kept}");
        assert_eq!(kept["parameters"].as_array().unwrap().len(), 2, "{kept}");
        window.themed("variables", language);
    }
    zen_i18n::set_language(Some("en"));
}
