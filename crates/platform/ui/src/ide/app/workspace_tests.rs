use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_workbench::workspaces::{WorkspaceInfo, Workspaces};

use super::tests::{ask, frame, test_product, themed_app, window_project};
use super::*;

struct Fixture {
    ctx: egui::Context,
    app: IdeApp,
    data: PathBuf,
    project: PathBuf,
    main: PathBuf,
}

impl Fixture {
    fn new(data: &Path) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let data = &std::fs::canonicalize(data).unwrap();
        let project = data.join("project");
        let main = project.join("main.go");
        if !project.exists() {
            window_project(data);
        }
        let mut app = themed_app(data, vec![project.clone()]);
        app.shared.settings.lock().unwrap().autosave_on_idle = false;
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            let info = WorkspaceInfo { id: "test".into(), title: "Test".into(), icon: "database".into() };
            window.session.state_mut::<Workspaces>().register(info);
        }
        frame(&ctx, &mut app);
        Self { ctx, app, data: data.to_owned(), project, main }
    }

    fn console(&self, name: &str, text: &str) -> PathBuf {
        let folder = self.data.join("consoles/project/local");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn call(&mut self, command: &str, params: Value) -> Result<Value, String> {
        ask(&self.ctx, &mut self.app, "dispatch_command", json!({ "command": command, "params": params }))
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.call(command, params).unwrap()
    }

    fn keys(&mut self, keys: &[&str]) -> Value {
        let answer = ask(&self.ctx, &mut self.app, "press_keys", json!({ "keys": keys })).unwrap();
        for _ in 0..3 {
            frame(&self.ctx, &mut self.app);
        }
        answer
    }

    fn typed(&mut self, text: &str) {
        ask(&self.ctx, &mut self.app, "press_keys", json!({ "text": text })).unwrap();
        frame(&self.ctx, &mut self.app);
    }

    fn text(&mut self, path: &Path) -> String {
        let answer = ask(&self.ctx, &mut self.app, "get_document_text", json!({ "path": path })).unwrap();
        answer["text"].as_str().unwrap_or_default().to_owned()
    }

    fn window(&mut self) -> Value {
        ask(&self.ctx, &mut self.app, "get_window_state", json!({})).unwrap()
    }

    fn state(&mut self) -> Value {
        self.run("workspace.host.state", json!({ "id": "test" }))
    }

    fn titles(&mut self) -> Vec<String> {
        let state = self.state();
        state["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap().to_owned()).collect()
    }
}

#[test]
fn keys_and_agents_act_on_the_workspace_in_front_never_on_hidden_code() {
    let data = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(data.path());
    let main = fixture.main.clone();
    fixture.run("file.document.open", json!({ "path": main }));
    let console = fixture.console("console.sql", "select 1;\n");
    fixture.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    fixture.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    for _ in 0..3 {
        frame(&fixture.ctx, &mut fixture.app);
    }

    let window = fixture.window();
    assert_eq!(window["workspace"]["id"], "test", "{window}");
    assert_eq!(window["workspace"]["expanded"], true);
    assert_eq!(window["workspace"]["activeTab"]["kind"], "history");
    assert_eq!(window["workspace"]["activeTab"]["title"], "Query History");
    assert_eq!(window["editor"], "workspace", "no file is in front");
    let refused = fixture.call("editor.edit.insert", json!({ "text": "x" })).unwrap_err();
    assert!(refused.contains("Query History") && refused.contains("Test"), "{refused}");
    let refused = ask(&fixture.ctx, &mut fixture.app, "run_action", json!({ "action": "view.gotoLine.open" }));
    let refused = refused.map(|answer| answer.to_string()).unwrap_or_else(|error| error);
    assert!(refused.contains("Query History"), "Go to Line names the tab in front: {refused}");

    fixture.keys(&["⌘W"]);
    assert_eq!(fixture.titles(), ["console.sql"], "⌘W closes the active inner tab");
    let window = fixture.window();
    assert_eq!(window["workspace"]["expanded"], true);
    assert_eq!(window["workspace"]["activeTab"]["path"], json!(console));
    assert_eq!(window["editor"], json!(console), "the console is the text in front");
    let open = fixture.run("editor.split.list", json!({}));
    assert!(open.to_string().contains("main.go"), "the hidden code tab stays open: {open}");
    fixture.run("editor.edit.insert", json!({ "text": "-- " }));
    let text = fixture.text(&console);
    assert!(text.contains("-- select 1;"), "editor commands work in a console: {text}");

    fixture.keys(&["⎋"]);
    assert_eq!(fixture.window()["workspace"]["expanded"], true, "⎋ never collapses");
    fixture.keys(&["⇧⎋"]);
    let window = fixture.window();
    assert_eq!(window["workspace"]["expanded"], false, "⇧⎋ collapses: {window}");
    assert_eq!(window["editor"], json!(main), "the code is in front again");
    assert_eq!(fixture.titles(), ["console.sql"], "collapsing closes nothing");

    fixture.keys(&["⌘W"]);
    assert_eq!(fixture.titles(), ["console.sql"], "collapsed, ⌘W closes the code tab, not the console");

    fixture.run("workspace.tab.open", json!({ "workspace": "test", "kind": "sessions", "title": "Sessions" }));
    fixture.run("workspace.host.collapse", json!({}));
    fixture.keys(&["⌘E"]);
    assert!(fixture.window()["popups"].to_string().contains("recentFiles"));
    fixture.typed("Sess");
    fixture.keys(&["⏎"]);
    let window = fixture.window();
    assert_eq!(window["workspace"]["expanded"], true, "picking an inner tab in Recent Files expands it: {window}");
    assert_eq!(window["workspace"]["activeTab"]["title"], "Sessions");

    fixture.run("workspace.host.collapse", json!({}));
    fixture.keys(&["⌘E"]);
    fixture.typed("console");
    fixture.keys(&["⏎"]);
    let window = fixture.window();
    assert_eq!(window["workspace"]["expanded"], true, "picking a console while collapsed expands: {window}");
    assert_eq!(window["workspace"]["activeTab"]["path"], json!(console));
}

#[test]
fn reformat_code_with_no_file_in_front_does_nothing_and_posts_nothing() {
    let data = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(data.path());
    let console = fixture.console("console.sql", "select 1;\n");
    fixture.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    fixture.run("workspace.host.collapse", json!({}));
    for _ in 0..3 {
        frame(&fixture.ctx, &mut fixture.app);
    }
    let refused = fixture.call("editor.code.reformat", json!({})).unwrap_err();
    assert!(refused.contains("No file is open"), "the command itself still says why: {refused}");
    let before = fixture.run("ui.notifications.list", json!({}));
    fixture.keys(&["⌥⌘L"]);
    let refused = ask(&fixture.ctx, &mut fixture.app, "run_action", json!({ "action": "editor.code.reformat" }));
    let refused = refused.map(|answer| answer.to_string()).unwrap_or_else(|error| error);
    assert!(refused.contains("needs a file open"), "{refused}");
    for _ in 0..3 {
        frame(&fixture.ctx, &mut fixture.app);
    }
    let after = fixture.run("ui.notifications.list", json!({}));
    assert_eq!(after, before, "a keystroke with nothing to format posts no notification");
    assert_eq!(fixture.text(&console), "select 1;\n");
}

#[test]
fn a_killed_ide_comes_back_with_its_workspace_tabs_and_console_text() {
    let data = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(data.path());
    let console = fixture.console("console.sql", "");
    fixture.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    fixture.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    fixture.run("editor.edit.insert", json!({ "text": "select 42;\n" }));
    let deadline = Instant::now() + Duration::from_secs(60);
    std::thread::sleep(Duration::from_millis(1100));
    frame(&fixture.ctx, &mut fixture.app);
    while std::fs::read_to_string(&console).unwrap() != "select 42;\n" && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        frame(&fixture.ctx, &mut fixture.app);
    }
    assert_eq!(std::fs::read_to_string(&console).unwrap(), "select 42;\n", "saved on idle with autosave off");
    let main = fixture.main.clone();
    std::fs::write(&main, "package main // changed on disk\n").unwrap();
    fixture.app.remember_workspace(&fixture.ctx, true);
    let project = fixture.project.clone();
    drop(fixture);

    let mut fixture = Fixture::new(data.path());
    assert_eq!(fixture.project, project);
    assert_eq!(fixture.titles(), ["Query History", "console.sql"], "the tabs come back in order");
    let state = fixture.state();
    assert_eq!(state["expanded"], true, "expanded as it was left: {state}");
    let active = state["active"].clone();
    let console_tab = state["tabs"][1]["id"].clone();
    assert_eq!(active, console_tab, "the console is active again");
    let text = fixture.text(&console);
    assert!(text.contains("select 42;"), "{text}");
    let window = fixture.window();
    assert_eq!(window["editor"], json!(console));
}

fn tabs_restored_after_deleting(active: &str, deleted: &str) -> String {
    let data = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(data.path());
    let names = ["a.go", "b.go", "c.go"];
    let paths = names.map(|name| fixture.project.join(name));
    for path in &paths {
        std::fs::write(path, "package main\n").unwrap();
        fixture.run("file.document.open", json!({ "path": path }));
    }
    fixture.run("file.document.open", json!({ "path": fixture.project.join(active) }));
    fixture.app.remember_workspace(&fixture.ctx, true);
    std::fs::remove_file(fixture.project.join(deleted)).unwrap();
    drop(fixture);
    let mut fixture = Fixture::new(data.path());
    let window = fixture.window();
    window["editor"].as_str().unwrap().rsplit('/').next().unwrap().to_owned()
}

#[test]
fn the_active_tab_comes_back_by_its_file_when_an_earlier_file_is_gone() {
    assert_eq!(tabs_restored_after_deleting("b.go", "a.go"), "b.go");
}

#[test]
fn a_deleted_active_file_gives_way_to_its_neighbour() {
    assert_eq!(tabs_restored_after_deleting("b.go", "b.go"), "c.go");
}

#[test]
fn a_project_switch_saves_the_workspace_tabs_with_the_project_they_belong_to() {
    let data = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(data.path());
    let console = fixture.console("console.sql", "select 7;\n");
    fixture.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    let other = fixture.data.join("other");
    std::fs::create_dir(&other).unwrap();
    fixture.run("file.project.open", json!({ "path": other }));
    assert_eq!(fixture.titles(), Vec::<String>::new(), "the other project has no workspace tabs");
    fixture.app.remember_workspace(&fixture.ctx, true);
    let first = fixture.project.clone();
    let kept = &fixture.app.workspace.projects[&first].workspaces;
    assert_eq!(kept[0].tabs[0].path.as_ref(), Some(&console), "kept in the first project's layout");
    assert!(fixture.app.workspace.projects.contains_key(&other), "the window's layout is the new project's");

    fixture.run("file.project.open", json!({ "path": first }));
    assert_eq!(fixture.titles(), ["console.sql"], "switching back gives them back");
    assert_eq!(fixture.state()["expanded"], false, "collapsed");
}

fn database_app(data: &Path) -> (egui::Context, IdeApp) {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let project = data.join("project");
    if !project.exists() {
        window_project(data);
    }
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let product = IdeProduct {
        modules: vec![zen_modules::BuiltinModule::on(&zen_workbench::database::MODULE)],
        ..test_product()
    };
    let mut app = IdeApp::new(product, registry, Some(data.join("recent.json")), vec![project]);
    app.shared.settings.lock().unwrap().autosave_on_idle = false;
    let ctx = egui::Context::default();
    for _ in 0..2 {
        frame(&ctx, &mut app);
    }
    (ctx, app)
}

#[test]
fn a_product_with_database_support_shows_the_workspace_and_keeps_its_consoles() {
    let data = tempfile::tempdir().unwrap();
    let data = std::fs::canonicalize(data.path()).unwrap();
    let (ctx, mut app) = database_app(&data);
    let call = |app: &mut IdeApp, command: &str, params: Value| {
        ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
    };
    let listed = call(&mut app, "workspace.host.list", json!({}));
    assert_eq!(listed["workspaces"][0]["id"], "database", "{listed}");
    let actions = ask(&ctx, &mut app, "find_action", json!({ "query": "database workspace" })).unwrap();
    assert!(actions.to_string().contains("workspace.host.toggle/database"), "{actions}");

    call(&mut app, "db.dataSource.add", json!({ "name": "Shop", "url": "postgresql://ann@localhost:5432/shop" }));
    let opened = call(&mut app, "db.console.new", json!({}));
    let console = PathBuf::from(opened["path"].as_str().unwrap());
    assert!(console.starts_with(data.join("consoles")), "{console:?}");
    ask(&ctx, &mut app, "press_keys", json!({ "text": "select 1;" })).unwrap();
    frame(&ctx, &mut app);
    let text = ask(&ctx, &mut app, "get_document_text", json!({ "path": console })).unwrap();
    assert_eq!(text["text"], "select 1;", "{text}");
    let window = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
    assert_eq!(window["workspace"]["id"], "database");
    assert_eq!(window["editor"], json!(console));
    let recent = call(&mut app, "navigate.recent.files", json!({}));
    assert!(recent.to_string().contains("console.sql"), "{recent}");

    call(&mut app, "file.document.saveAll", json!({}));
    app.remember_workspace(&ctx, true);
    drop(app);
    let (ctx, mut app) = database_app(&data);
    let state = ask(
        &ctx,
        &mut app,
        "dispatch_command",
        json!({ "command": "workspace.host.state", "params": { "id": "database" } }),
    )
    .unwrap();
    let titles: Vec<&str> =
        state["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Overview", "console.sql"], "{state}");
    assert_eq!(state["expanded"], true);
    let text = ask(&ctx, &mut app, "get_document_text", json!({ "path": console })).unwrap();
    assert_eq!(text["text"], "select 1;");
}

#[test]
fn agents_open_the_connection_dialog_filled_in_and_find_action_offers_it() {
    let data = tempfile::tempdir().unwrap();
    let data = std::fs::canonicalize(data.path()).unwrap();
    let (ctx, mut app) = database_app(&data);
    {
        let slot = app.slot_for(None).unwrap();
        let mut window = slot.window.lock().unwrap();
        let secrets = Arc::new(zen_secrets::MemoryStore::new());
        window.session.set_state(zen_workbench::database::Secrets(secrets));
    }
    let actions = ask(&ctx, &mut app, "find_action", json!({ "query": "new database connection" })).unwrap();
    assert!(actions.to_string().contains("view.dataSource.open"), "{actions}");
    let opened = ask(
        &ctx,
        &mut app,
        "run_action",
        json!({ "action": "view.dataSource.open", "params": { "url": "postgresql://ann@db.local:6543/shop", "name": "Shop" } }),
    )
    .unwrap();
    assert!(opened.to_string().contains("connectionDialog"), "{opened}");
    frame(&ctx, &mut app);
    let window = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
    assert_eq!(window["popups"], json!(["connectionDialog"]), "{window}");
    let slot = app.slot_for(None).unwrap();
    let window = slot.window.lock().unwrap();
    let (command, params) = window.view.connection_dialog.as_ref().unwrap().save_call();
    assert_eq!(command, "db.dataSource.add");
    assert_eq!(
        (params["name"].as_str(), params["host"].as_str(), params["port"].as_u64(), params["user"].as_str()),
        (Some("Shop"), Some("db.local"), Some(6543), Some("ann"))
    );
}

#[test]
fn a_detected_database_opens_the_dialog_filled_in_and_agents_never_get_its_values() {
    let data = tempfile::tempdir().unwrap();
    let data = std::fs::canonicalize(data.path()).unwrap();
    window_project(&data);
    let project = data.join("project");
    std::fs::write(project.join(".env"), "DB_PASSWORD=compose-s3cret\n").unwrap();
    std::fs::write(
        project.join("compose.yaml"),
        "services:\n  db:\n    image: postgres:17\n    ports: [\"5434:5432\"]\n    environment:\n      POSTGRES_PASSWORD: ${DB_PASSWORD}\n",
    )
    .unwrap();
    let (ctx, mut app) = database_app(&data);
    let secrets = Arc::new(zen_secrets::MemoryStore::new());
    {
        let slot = app.slot_for(None).unwrap();
        let mut window = slot.window.lock().unwrap();
        window.session.set_state(zen_workbench::database::Secrets(secrets.clone()));
    }
    let list = |app: &mut IdeApp| {
        let slot = app.slot_for(None).unwrap();
        let mut window = slot.window.lock().unwrap();
        app.shared.registry.dispatch(&mut window.session, "db.detect.list", json!({})).unwrap()
    };
    let started = Instant::now();
    let listed = loop {
        let listed = list(&mut app);
        if listed["scanning"] == false {
            break listed;
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    };
    let id = listed["detections"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(listed["detections"][0]["port"], 5434, "{listed}");

    let refused =
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.dataSource.open", "params": { "detection": id } }));
    assert!(refused.as_ref().is_err_and(|error| error.contains("hidden from agents")), "{refused:?}");
    let offers = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "db.detect.list", "params": {} })).unwrap();
    assert!(!offers.to_string().contains("5434") && offers["detections"][0]["redacted"] == true, "{offers}");

    {
        let slot = app.slot_for(None).unwrap();
        let mut window = slot.window.lock().unwrap();
        window.session.state_mut::<super::super::connection_dialog::ConnectionDialogRequest>().0 =
            Some(json!({ "detection": id }));
    }
    frame(&ctx, &mut app);
    let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
    assert_eq!(state["popups"], json!(["connectionDialog"]), "{state}");
    let slot = app.slot_for(None).unwrap();
    let mut window = slot.window.lock().unwrap();
    let (command, params) = window.view.connection_dialog.as_ref().unwrap().save_call();
    assert_eq!(command, "db.detect.accept");
    assert_eq!(
        (params["id"].as_str(), params["port"].as_u64(), params["name"].as_str()),
        (Some(id.as_str()), Some(5434), Some("db"))
    );
    let added = app.shared.registry.dispatch(&mut window.session, command, params).unwrap();
    assert!(!added.to_string().contains("compose-s3cret"), "{added}");
    let connection = added["connection"]["id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let params = json!({ "operation": added["connection"]["secretOperation"] });
        let state = app.shared.registry.dispatch(&mut window.session, "db.operation.state", params).unwrap();
        if state["status"] != "running" {
            assert_eq!(state["status"], "done", "{state}");
            break;
        }
        assert!(Instant::now() < deadline, "{state}");
        std::thread::sleep(Duration::from_millis(5));
    }
    let key = zen_secrets::database::id(None, connection, zen_secrets::database::Kind::Password).unwrap();
    assert_eq!(zen_secrets::SecretStore::get(secrets.as_ref(), &key).unwrap().unwrap().expose(), "compose-s3cret");
}

#[test]
fn agents_open_the_database_dialogs_by_their_window_actions() {
    let data = tempfile::tempdir().unwrap();
    let data = std::fs::canonicalize(data.path()).unwrap();
    let (ctx, mut app) = database_app(&data);
    let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.database.export" })).unwrap();
    assert_eq!(opened["result"]["dialog"], "exportData", "{opened}");
    let unknown = ask(&ctx, &mut app, "run_action", json!({ "action": "view.database.nothing" })).unwrap_err();
    assert!(unknown.contains("no action"), "{unknown}");
}
