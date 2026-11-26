use std::path::PathBuf;

use egui::accesskit::{Role, TreeUpdate};
use egui::vec2;
use serde_json::json;

use super::{Window, click_at, sql_window, until};

const WIDTH: f32 = 980.0;

fn read_tree(window: &mut Window) -> TreeUpdate {
    read_tree_at(window, WIDTH)
}

fn png(window: &mut Window, dir: &Option<PathBuf>, name: &str) {
    png_at(window, dir, name, WIDTH);
}

fn named(tree: &TreeUpdate, role: Role, name: &str) -> Option<egui::accesskit::Rect> {
    tree.nodes
        .iter()
        .find(|(_, node)| node.role() == role && node.label().is_some_and(|label| label == name))
        .and_then(|(_, node)| node.bounds())
}

fn inside(tree: &TreeUpdate, role: Role, name: &str) {
    let bounds = named(tree, role, name).unwrap_or_else(|| panic!("no {role:?} {name:?}"));
    assert!(bounds.x1 <= f64::from(WIDTH) - 30.0, "{name:?} is cut off at the edge: {bounds:?}");
}

fn every_button_inside(tree: &TreeUpdate, what: &str) {
    every_button_within(tree, what, WIDTH);
}

fn wait_plan(window: &mut Window) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        window.frames(1);
        let shown = window.run("workspace.host.state", json!({ "id": "database" }));
        let plan = shown["tabs"].as_array().unwrap().iter().any(|tab| tab["kind"] == "plan");
        let state = window.run("db.plan.state", json!({})).to_string();
        if plan && state.contains("\"done\"") {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "the plan never came: {state}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn database_bars_fit_a_narrow_window_with_the_rest_in_a_more_menu_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, _) = sql_window(&theme);
        let console = window.run("db.console.new", json!({ "connection": id, "text": "SELECT * FROM customers;\n" }));
        window.run("db.transaction.mode", json!({ "mode": "manual" }));
        let started = window.run("db.console.executeScript", json!({}));
        window.run("db.query.state", json!({ "query": started["query"], "waitMs": 10_000 }));
        let tree = read_tree(&mut window);
        inside(&tree, Role::Button, &zen_i18n::t!("ide-database-more-actions"));
        for control in [
            zen_i18n::t!("ide-database-choose-connection"),
            zen_i18n::t!("ide-dbtx-mode-hint"),
            zen_i18n::t!("ide-dbtx-log-open"),
        ] {
            inside(&tree, Role::Button, &control);
        }
        every_button_inside(&tree, "the console toolbar");
        assert!(named(&tree, Role::Button, &zen_i18n::t!("ide-dbplan-explain-plan")).is_none(), "Explain is in ⋯");
        png(&mut window, &snapshots, &format!("b7-console-toolbar-{name}"));

        window.run("db.console.explain", json!({ "path": console["path"] }));
        wait_plan(&mut window);
        let tree = read_tree(&mut window);
        every_button_inside(&tree, "the plan bar");
        png(&mut window, &snapshots, &format!("b7-plan-{name}"));

        window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
        window.run("db.object.showPage", json!({ "page": "properties" }));
        until(&mut window, "the properties", |texts| texts.iter().any(|(_, text)| text.contains("Columns")));
        let tree = read_tree(&mut window);
        every_button_inside(&tree, "the properties");
        for control in [
            zen_i18n::t!("ide-database-selector-fixed"),
            zen_i18n::t!("ide-dbtx-mode-hint"),
            zen_i18n::t!("ide-dbtx-commit"),
            zen_i18n::t!("ide-dbtx-rollback"),
            zen_i18n::t!("ide-dbtx-log-open"),
        ] {
            assert!(named(&tree, Role::Button, &control).is_some(), "{control:?} stays on the toolbar of a table");
        }
        png(&mut window, &snapshots, &format!("b7-properties-{name}"));

        window.run("db.search.open", json!({ "connection": id, "text": "Customer" }));
        let tree = read_tree(&mut window);
        every_button_inside(&tree, "Find in Database");
        png(&mut window, &snapshots, &format!("b7-find-{name}"));

        window.run("db.session.open", json!({ "connection": id }));
        let tree = read_tree(&mut window);
        every_button_inside(&tree, "the sessions");
        png(&mut window, &snapshots, &format!("b7-sessions-{name}"));
    }
}

#[test]
fn two_consoles_of_different_connections_are_told_apart_on_their_tabs_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, shop, _) = sql_window(&theme);
        let billing = window.run(
            "db.dataSource.add",
            json!({ "name": "Billing", "driver": zen_db::memory::SQL, "type": "test", "password": "pw" }),
        );
        window.run("db.console.new", json!({ "connection": shop, "text": "SELECT 1;\n" }));
        window.run("db.console.new", json!({ "connection": billing["id"], "text": "SELECT 2;\n" }));
        let state = window.run("workspace.host.state", json!({ "id": "database" }));
        let titles: Vec<&str> =
            state["tabs"].as_array().unwrap().iter().filter_map(|tab| tab["title"].as_str()).collect();
        assert_eq!(titles, ["Overview", "console.sql (Shop)", "console.sql (Billing)"]);
        let tree = read_tree(&mut window);
        for title in ["console.sql (Shop)", "console.sql (Billing)"] {
            let shown = tree.nodes.iter().any(|(_, node)| node.label().is_some_and(|label| label.contains(title)));
            assert!(shown, "the tab {title:?} is drawn");
        }
        png(&mut window, &snapshots, &format!("b8-console-tabs-{name}"));
    }
}

#[test]
fn new_console_without_a_connection_opens_new_connection_instead() {
    let (mut window, shop, _) = sql_window(&zen_theme::ResolvedTheme::zen_light());
    window.run("db.dataSource.remove", json!({ "id": shop }));
    window.run("workspace.host.expand", json!({ "id": "database" }));
    let tree = read_tree(&mut window);
    let button = named(&tree, Role::Button, &zen_i18n::t!("ide-database-new-console")).expect("the New Console button");
    let at = egui::pos2(((button.x0 + button.x1) / 2.0) as f32, ((button.y0 + button.y1) / 2.0) as f32);
    let mut screen = crate::offscreen::Offscreen::new(vec2(WIDTH, 700.0), 1.0);
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    for _ in 0..3 {
        screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    }
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [vec![egui::Event::PointerMoved(at)], vec![press(true)], vec![press(false)], Vec::new(), Vec::new()] {
        screen.step(events, |ui| view.show(ui, registry, session));
    }
    assert!(window.view.connection_dialog.is_some(), "New Connection opened instead of a console");
    let consoles = window.run("db.console.list", json!({}));
    assert_eq!(consoles["total"], 0, "no console without a connection: {consoles}");
}

#[derive(Default)]
struct SilentStore {
    answer: std::sync::Mutex<bool>,
    answered: std::sync::Condvar,
}

impl SilentStore {
    fn answer(&self) {
        *self.answer.lock().unwrap() = true;
        self.answered.notify_all();
    }
}

impl zen_secrets::SecretStore for SilentStore {
    fn get(&self, _: &zen_secrets::SecretId) -> Result<Option<zen_secrets::Secret>, zen_secrets::SecretError> {
        let answer = self.answer.lock().unwrap();
        drop(self.answered.wait_while(answer, |answer| !*answer).unwrap());
        Ok(Some(zen_secrets::Secret::new("pw")))
    }

    fn set(&self, _: &zen_secrets::SecretId, _: &zen_secrets::Secret) -> Result<(), zen_secrets::SecretError> {
        Ok(())
    }

    fn delete(&self, _: &zen_secrets::SecretId) -> Result<bool, zen_secrets::SecretError> {
        Ok(false)
    }
}

#[test]
fn a_connection_waiting_for_the_keychain_says_so_and_cancel_stops_it_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, _) = sql_window(&theme);
        let store = std::sync::Arc::new(SilentStore::default());
        window.session.set_state(zen_workbench::database::Secrets(std::sync::Arc::clone(&store) as _));
        window.run("workspace.host.expand", json!({ "id": "database" }));
        let started = window.run("db.connection.connect", json!({ "connection": id }));
        let waiting = zen_i18n::t!("ide-database-state-waiting-keychain");
        let texts = until(&mut window, "the Keychain wait", |texts| texts.iter().any(|(_, text)| *text == waiting));
        let listed = window.run("db.dataSource.get", json!({ "id": id }));
        assert_eq!(listed["waitingFor"], "keychain", "{listed}");
        png(&mut window, &snapshots, &format!("db-keychain-wait-{name}"));
        let cancel = zen_i18n::t!("ide-database-cancel-connect");
        let (at, _) = texts.iter().find(|(_, text)| *text == cancel).expect("a Cancel button");
        click_at(&mut window, *at);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let state = loop {
            let state = window.run("db.operation.state", json!({ "operation": started["operation"] }));
            if state["status"] != "running" || std::time::Instant::now() > deadline {
                break state;
            }
            window.frames(1);
        };
        assert_eq!(state["error"]["kind"], "cancelled", "{state}");
        store.answer();
    }
}

#[test]
fn a_console_of_a_removed_connection_says_it_has_none_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, shop, _) = sql_window(&theme);
        window.run("db.dataSource.add", json!({ "name": "Billing", "driver": zen_db::memory::SQL, "password": "pw" }));
        window.run("db.object.open", json!({ "connection": shop, "path": ["shop", "public", "tables", "customers"] }));
        window.run("db.console.new", json!({ "connection": shop, "text": "SELECT 1;\n" }));
        let removed = window.run("db.dataSource.remove", json!({ "id": shop }));
        assert_eq!(removed["closedTabs"], 1, "{removed}");
        let said = zen_i18n::t!("ide-database-connection-removed", name = "Shop");
        until(&mut window, "the removed connection", |texts| texts.iter().any(|(_, text)| *text == said));
        let state = window.run("workspace.host.state", json!({ "id": "database" }));
        let titles: Vec<&str> =
            state["tabs"].as_array().unwrap().iter().filter_map(|tab| tab["title"].as_str()).collect();
        assert_eq!(titles, ["Overview", "console.sql"]);
        png(&mut window, &snapshots, &format!("db-removed-connection-{name}"));
    }
}

fn read_tree_at(window: &mut Window, width: f32) -> TreeUpdate {
    let mut screen = crate::offscreen::Offscreen::new(vec2(width, 700.0), 1.0);
    window.view.palette.apply(screen.context());
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    for _ in 0..4 {
        screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    }
    screen.accessibility_tree(|ui| view.show(ui, registry, session))
}

fn png_at(window: &mut Window, dir: &Option<PathBuf>, name: &str, width: f32) {
    let Some(dir) = dir else { return };
    let mut screen = crate::offscreen::Offscreen::new(vec2(width, 700.0), 1.5);
    window.view.palette.apply(screen.context());
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    for _ in 0..4 {
        screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    }
    let frame = screen.render(|ui| view.show(ui, registry, session));
    crate::offscreen::write_png(&dir.join(format!("{name}.png")), frame.image()).unwrap();
}

fn every_button_within(tree: &TreeUpdate, what: &str, width: f32) {
    for (_, node) in &tree.nodes {
        if !matches!(node.role(), Role::Button | Role::CheckBox | Role::ComboBox | Role::SpinButton | Role::TextInput) {
            continue;
        }
        let Some(bounds) = node.bounds() else { continue };
        assert!(
            bounds.x1 <= f64::from(width) + 0.5,
            "{what}: {:?} {:?} reaches past the window: {bounds:?}",
            node.role(),
            node.label()
        );
    }
}

#[test]
fn the_toolbar_keeps_the_connection_and_transaction_on_every_tab_narrow_and_in_russian() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, _) = sql_window(&theme);
        window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
        window.run("db.object.showPage", json!({ "page": "properties" }));
        until(&mut window, "the properties", |texts| texts.iter().any(|(_, text)| text.contains("Columns")));
        for (language, width) in [("en", WIDTH), ("ru", WIDTH), ("ru", 760.0)] {
            zen_i18n::set_language(Some(language));
            let tree = read_tree_at(&mut window, width);
            every_button_within(&tree, "the toolbar of a table", width);
            for control in [zen_i18n::t!("ide-dbtx-commit"), zen_i18n::t!("ide-dbtx-mode-hint")] {
                assert!(named(&tree, Role::Button, &control).is_some(), "{control:?} on a table, {language} {width}");
            }
            png_at(&mut window, &snapshots, &format!("xo-table-toolbar-{language}-{width}-{name}"), width);
        }
        zen_i18n::set_language(Some("en"));
        window.run("workspace.tab.select", json!({ "workspace": "database", "tab": "overview" }));
        window.run("db.navigator.select", json!({ "connection": id }));
        let tree = read_tree(&mut window);
        assert!(
            named(&tree, Role::Button, &zen_i18n::t!("ide-dbtx-rollback")).is_some(),
            "the navigator's connection on the Overview"
        );
        png(&mut window, &snapshots, &format!("xo-overview-toolbar-{name}"));
        window.run("db.console.new", json!({ "connection": id, "text": "SELECT * FROM customers;\n" }));
        let tree = read_tree(&mut window);
        assert!(named(&tree, Role::Button, &zen_i18n::t!("ide-database-choose-connection")).is_some());
        png(&mut window, &snapshots, &format!("xo-console-toolbar-{name}"));
    }
}

#[test]
fn overview_rows_line_up_without_overlaps_at_any_width_and_in_russian() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, _, _) = sql_window(&theme);
        for long in ["notifier-service-with-a-very-long-name", "billing"] {
            window.run(
                "db.dataSource.add",
                json!({ "name": long, "driver": zen_db::memory::SQL, "type": "test", "password": "pw",
                        "database": "a_database_name_that_is_far_too_long_for_one_row_of_the_overview" }),
            );
        }
        window.run("workspace.host.expand", json!({ "id": "database" }));
        for (language, width) in [("en", WIDTH), ("ru", WIDTH), ("ru", 760.0), ("en", 600.0)] {
            zen_i18n::set_language(Some(language));
            let tree = read_tree_at(&mut window, width);
            every_button_within(&tree, "the Overview", width);
            let edit = zen_i18n::t!("ide-database-edit");
            let lefts: Vec<f64> = tree
                .nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == Role::Button
                        && node.label().is_some_and(|label| label.starts_with(&format!("{edit} ")))
                })
                .filter_map(|(_, node)| node.bounds().map(|bounds| bounds.x0))
                .collect();
            assert_eq!(lefts.len(), 3, "an Edit button per connection, {language} {width}");
            assert!(lefts.iter().all(|left| (left - lefts[0]).abs() < 0.5), "Edit buttons line up: {lefts:?}");
            png_at(&mut window, &snapshots, &format!("le-overview-{language}-{width}-{name}"), width);
        }
        zen_i18n::set_language(Some("en"));
    }
}
