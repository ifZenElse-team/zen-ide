use std::sync::mpsc;

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use serde_json::{Value, json};

use super::*;
use crate::accessibility::testing::find;
use crate::offscreen::Offscreen;

const IDEA_CONFIG: &str = "<!-- Synthetic fixture. -->\n<component name=\"ProjectCodeStyleConfiguration\"><state>\
    <option name=\"USE_PER_PROJECT_SETTINGS\" value=\"true\" /></state></component>\n";

const IDEA_YAML_TABS: &str = "<!-- Synthetic fixture. -->\n<component name=\"ProjectCodeStyleConfiguration\">\
    <code_scheme name=\"Project\" version=\"173\"><codeStyleSettings language=\"yaml\"><indentOptions>\
    <option name=\"USE_TAB_CHARACTER\" value=\"true\" /></indentOptions></codeStyleSettings></code_scheme></component>\n";

const SIZE: egui::Vec2 = egui::vec2(1280.0, 820.0);

fn product() -> IdeProduct {
    IdeProduct {
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
        inspectors: Vec::new(),
        language_servers: Vec::new(),
        templates: Vec::new(),
        folding_options: Vec::new(),
    }
}

fn ide(data: &Path) -> IdeApp {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    let themes = data.join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    let dusk = r##"{"formatVersion": 1, "id": "dusk", "name": "Dusk", "dark": true, "ui": {"accent": "#FF0000"}}"##;
    std::fs::write(themes.join("dusk.json"), dusk).unwrap();
    let project = data.join("project");
    for (name, text) in [
        ("main.go", "package main\n\nfunc main() {\n}\n"),
        ("b.yaml", "a:\n  b: 1\n"),
        (".idea/codeStyles/codeStyleConfig.xml", IDEA_CONFIG),
        (".idea/codeStyles/Project.xml", IDEA_YAML_TABS),
    ] {
        let path = project.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut app = IdeApp::new(product(), registry, Some(data.join("recent.json")), vec![project.clone()]);
    let ctx = egui::Context::default();
    let file =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../jb-import/tests/fixtures/keymaps/synthetic-macos.xml");
    app.tool(&ctx, "import_keymap", &json!({ "path": file })).unwrap();
    let main = project.join("main.go");
    let windows = app.shared.state().windows.clone();
    let mut window = windows[0].window.lock().unwrap();
    let ProjectWindow { session, .. } = &mut *window;
    app.shared.registry.dispatch(session, "file.document.open", json!({ "path": main })).unwrap();
    drop(window);
    app
}

fn keep_global_keymap() {
    crate::keymap::load(Arc::new(Mutex::new(KeymapStore::new(None, Vec::new()))));
}

fn frame(ctx: &egui::Context, app: &mut IdeApp) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
        ..egui::RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    output.textures_delta.clear();
}

fn ask(ctx: &egui::Context, app: &mut IdeApp, tool: &str, arguments: Value) -> Result<Value, String> {
    let (sender, answer) = mpsc::channel();
    app.route(
        ctx,
        tool,
        &arguments,
        Box::new(move |result| {
            let _ = sender.send(result);
        }),
    );
    for _ in 0..6 {
        if let Ok(result) = answer.try_recv() {
            for _ in 0..2 {
                frame(ctx, app);
            }
            return result;
        }
        frame(ctx, app);
    }
    answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments}"))
}

fn keys(ctx: &egui::Context, app: &mut IdeApp, keys: &[&str]) -> Value {
    ask(ctx, app, "press_keys", json!({ "keys": keys })).unwrap()
}

fn popups(ctx: &egui::Context, app: &mut IdeApp) -> Value {
    ask(ctx, app, "get_window_state", json!({})).unwrap()["popups"].clone()
}

fn saved(app: &IdeApp) -> Value {
    serde_json::from_str(&std::fs::read_to_string(app.shared.settings_file.as_ref().unwrap()).unwrap()).unwrap()
}

fn open(ctx: &egui::Context, app: &mut IdeApp) {
    if cfg!(target_os = "macos") {
        keys(ctx, app, &["⌃`"]);
    } else {
        ask(ctx, app, "run_action", json!({ "action": "view.quickSwitchScheme.open" })).unwrap();
    }
    assert_eq!(popups(ctx, app), json!(["quickSwitchScheme"]));
}

fn project_yaml_tabs(app: &IdeApp) -> bool {
    let windows = app.shared.state().windows.clone();
    let window = windows[0].window.lock().unwrap();
    let path = window.session.state::<Workbench>().unwrap().project().unwrap().join("b.yaml");
    zen_workbench::project_settings::code_style_for_path(&window.session, &path).use_tabs
}

#[test]
fn every_choice_applies_at_once_and_is_saved_as_settings_saves_it() {
    let (popup_data, settings_data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut popup = ide(popup_data.path());
    let mut settings = ide(settings_data.path());
    keep_global_keymap();
    let (ctx, other) = (egui::Context::default(), egui::Context::default());
    for _ in 0..2 {
        frame(&ctx, &mut popup);
        frame(&other, &mut settings);
    }
    let same = |popup: &mut IdeApp, settings: &mut IdeApp, command: &str, params: Value| {
        let answer = ask(&other, settings, "dispatch_command", json!({ "command": command, "params": params }));
        answer.unwrap();
        frame(&ctx, popup);
        assert_eq!(saved(popup), saved(settings), "after {command}");
        saved(popup)
    };

    open(&ctx, &mut popup);
    keys(&ctx, &mut popup, &["1"]);
    assert_eq!(popups(&ctx, &mut popup), json!(["quickSwitchScheme"]), "1 shows Theme in its place");
    keys(&ctx, &mut popup, &["3"]);
    assert_eq!(popups(&ctx, &mut popup), json!([]), "3 chooses the third theme and closes the popup");
    let file = same(&mut popup, &mut settings, "ui.theme.select", json!({ "id": "dusk" }));
    assert_eq!(file["theme"], "dusk");
    assert_eq!(popup.palette(), settings.palette(), "the theme is in use at once");
    assert_ne!(popup.palette(), IdePalette::zen_dark());

    open(&ctx, &mut popup);
    keys(&ctx, &mut popup, &["↓", "⏎"]);
    keys(&ctx, &mut popup, &["↓", "⏎"]);
    assert_eq!(popups(&ctx, &mut popup), json!([]));
    let file = same(&mut popup, &mut settings, "ui.theme.select", json!({ "id": "zen-light" }));
    assert_eq!(file["theme"], "zen-light", "Editor Color Scheme: on Zen Dark after Dusk, ↓ Zen Light, Enter uses it");
    assert_eq!(popup.palette(), IdePalette::from_theme(&zen_theme::zen_light().resolve(|_| None).theme));

    open(&ctx, &mut popup);
    keys(&ctx, &mut popup, &["4"]);
    keys(&ctx, &mut popup, &["⏎"]);
    let file = same(&mut popup, &mut settings, "ui.keymap.select", json!({ "id": "fixture-keys" }));
    assert_eq!(file["keymap"], "fixture-keys");
    assert_eq!(keymaps::lock(&popup.shared.keymaps).current(), "fixture-keys");

    assert!(project_yaml_tabs(&popup), "the project's scheme at first");
    open(&ctx, &mut popup);
    keys(&ctx, &mut popup, &["3"]);
    keys(&ctx, &mut popup, &["2"]);
    let answer = ask(&other, &mut settings, "set_settings", json!({ "codeStyleScheme": "default" }));
    assert_eq!(answer.unwrap()["codeStyleScheme"], "default");
    frame(&ctx, &mut popup);
    assert_eq!(saved(&popup), saved(&settings), "Code Style Scheme → Default");
    assert_eq!(saved(&popup)["codeStyleScheme"], "default");
    assert!(!project_yaml_tabs(&popup), "the Default scheme leaves the project's code style out");
    let listed = ask(&ctx, &mut popup, "run_action", json!({ "action": "view.quickSwitchScheme.open" })).unwrap();
    let schemes = &listed["result"]["groups"][2]["choices"];
    assert_eq!((schemes[0]["current"].clone(), schemes[1]["current"].clone()), (json!(false), json!(true)));
    keys(&ctx, &mut popup, &["3", "1"]);
    assert_eq!(saved(&popup)["codeStyleScheme"], "project");
    assert!(project_yaml_tabs(&popup));
    ask(&other, &mut settings, "set_settings", json!({ "codeStyleScheme": "project" })).unwrap();
    assert_eq!(saved(&popup), saved(&settings), "Code Style Scheme → Project");

    open(&ctx, &mut popup);
    keys(&ctx, &mut popup, &["6", "↓", "⏎"]);
    assert_eq!(popups(&ctx, &mut popup), json!([]));
    let answer = ask(&other, &mut settings, "set_settings", json!({ "ideZoom": 110 }));
    assert_eq!(answer.unwrap()["ideZoom"], 110);
    frame(&ctx, &mut popup);
    assert_eq!(saved(&popup), saved(&settings), "Zoom → 110%");
    assert_eq!(saved(&popup)["ideZoom"], 110);
}

#[test]
fn view_mode_reads_enter_or_exit_and_the_popup_opens_from_find_action_and_closes_with_its_keys() {
    let data = tempfile::tempdir().unwrap();
    let mut app = ide(data.path());
    keep_global_keymap();
    let ctx = egui::Context::default();
    for _ in 0..2 {
        frame(&ctx, &mut app);
    }
    let modes = |app: &mut IdeApp| -> Vec<String> {
        let listed = ask(&ctx, app, "run_action", json!({ "action": "view.quickSwitchScheme.open" })).unwrap();
        let group = &listed["result"]["groups"][4];
        assert_eq!(group["title"], zen_i18n::t!("ide-quick-switch-view-mode"));
        group["choices"].as_array().unwrap().iter().map(|choice| choice["label"].as_str().unwrap().to_owned()).collect()
    };
    let titles = |on: [bool; 4]| -> Vec<String> {
        ["presentationMode", "distractionFreeMode", "fullScreen", "zenMode"]
            .iter()
            .zip(on)
            .map(|(mode, on)| zen_i18n::t!(&format!("menu-view-{}-{mode}", if on { "exit" } else { "enter" })))
            .collect()
    };
    assert_eq!(modes(&mut app), titles([false; 4]));
    keys(&ctx, &mut app, &["5", "1"]);
    let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
    assert_eq!((state["appearance"].clone(), state["popups"].clone()), (json!("presentation"), json!([])));
    assert_eq!(modes(&mut app), titles([true, false, true, false]), "Presentation Mode went full screen");
    keys(&ctx, &mut app, &["5", "1"]);
    let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
    assert_eq!(state["appearance"], Value::Null, "Exit Presentation Mode left it");
    keys(&ctx, &mut app, &["⎋"]);

    let found = ask(&ctx, &mut app, "find_action", json!({ "query": "quick switch scheme", "limit": 5 })).unwrap();
    let entry = found["actions"].as_array().unwrap().iter().find(|e| e["action"] == "view.quickSwitchScheme.open");
    let entry = entry.unwrap_or_else(|| panic!("{found}"));
    assert_eq!(entry["shortcut"], "⌃`", "{entry}");
    ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⌘A"], "text": "Quick Switch Scheme" })).unwrap();
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    keys(&ctx, &mut app, &["⏎"]);
    assert_eq!(popups(&ctx, &mut app), json!(["quickSwitchScheme"]), "Find Action opens it");

    let typed = ask(&ctx, &mut app, "press_keys", json!({ "keys": [], "text": "л" })).unwrap();
    assert_eq!(typed["window"]["popups"], json!(["quickSwitchScheme"]), "{typed}");
    let shown = ask(&ctx, &mut app, "run_action", json!({ "action": "view.quickSwitchScheme.open" })).unwrap();
    assert_eq!((shown["result"]["filter"].clone(), shown["result"]["selected"].clone()), (json!("л"), json!(3)));
    keys(&ctx, &mut app, &["⏎"]);
    let shown = ask(&ctx, &mut app, "run_action", json!({ "action": "view.quickSwitchScheme.open" })).unwrap();
    assert_eq!(shown["result"]["group"], "keymap", "typing found Keymap, Enter shows it");
    keys(&ctx, &mut app, &["⎋"]);
    assert_eq!(popups(&ctx, &mut app), json!([]), "Escape closes the list");
    if cfg!(target_os = "macos") {
        open(&ctx, &mut app);
        keys(&ctx, &mut app, &["↓", "⌃`"]);
        assert_eq!(popups(&ctx, &mut app), json!(["quickSwitchScheme"]), "⌃` again does nothing");
        let state = ask(&ctx, &mut app, "run_action", json!({ "action": "view.quickSwitchScheme.open" })).unwrap();
        assert_eq!((state["result"]["group"].clone(), state["result"]["selected"].clone()), (Value::Null, json!(1)));
        keys(&ctx, &mut app, &["⎋"]);
    }
    let windows = app.shared.state().windows.clone();
    let window = windows[0].window.lock().unwrap();
    let text = window.session.state::<Workbench>().unwrap().active_document().unwrap().snapshot().to_string();
    assert_eq!(text, "package main\n\nfunc main() {\n}\n", "the digits typed nothing into the file");
    let in_view = super::super::main_menu::MENUS.iter().find(|(menu, _)| *menu == "menu-view").unwrap();
    assert!(
        in_view
            .1
            .iter()
            .any(|entry| matches!(entry, super::super::main_menu::Entry::Action("view.quickSwitchScheme.open"))),
        "View → Quick Switch Scheme…"
    );
}

struct English;

impl Drop for English {
    fn drop(&mut self) {
        zen_i18n::set_language(Some("en"));
    }
}

fn press(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers })
        .collect()
}

fn draw(app: &mut IdeApp) -> impl FnMut(&mut Ui) + '_ {
    |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    }
}

fn open_popup(screen: &mut Offscreen, app: &mut IdeApp) {
    let windows = app.shared.state().windows.clone();
    let opened = {
        let mut window = windows[0].window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *window;
        let action = "view.quickSwitchScheme.open";
        view.perform_action(screen.context(), &app.shared.registry, session, action, json!({}), false)
    };
    assert!(opened.is_ok_and(|groups| groups["groups"].as_array().is_some_and(|groups| groups.len() == 6)));
    screen.step(Vec::new(), draw(app));
}

#[test]
fn a_click_chooses_and_every_list_is_drawn_with_its_numbers_and_marks() {
    let _english = std::env::var_os("ZEN_UI_SNAPSHOTS").map(|_| English);
    let data = tempfile::tempdir().unwrap();
    let mut app = ide(data.path());
    keep_global_keymap();
    let mut screen = Offscreen::new(SIZE, 1.0);
    screen.render(draw(&mut app));

    open_popup(&mut screen, &mut app);
    screen.step(press(Key::Num4, Modifiers::NONE), draw(&mut app));
    let tree = screen.accessibility_tree(draw(&mut app));
    let macos = find(&tree, Role::MenuItemRadio, "macOS").expect("the keymap in use");
    assert_eq!(macos.toggled(), Some(egui::accesskit::Toggled::True), "marked");
    assert_eq!(macos.is_selected(), Some(false));
    let row = find(&tree, Role::MenuItemRadio, "Fixture Keys").expect("the imported keymap");
    assert_eq!((row.toggled(), row.is_selected()), (Some(egui::accesskit::Toggled::False), Some(true)));
    assert!(find(&tree, Role::Heading, &zen_i18n::t!("ide-quick-switch-keymap")).is_some(), "Keymap took its place");
    assert!(find(&tree, Role::MenuItem, &zen_i18n::t!("ide-quick-switch-configure-keymap")).is_some());
    let bounds = row.bounds().unwrap();
    let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    for events in [vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
        screen.step(events, draw(&mut app));
    }
    assert_eq!(keymaps::lock(&app.shared.keymaps).current(), "fixture-keys", "the click chose it");
    assert_eq!(saved(&app)["keymap"], "fixture-keys");
    let windows = app.shared.state().windows.clone();
    assert!(!windows[0].window.lock().unwrap().view.quick_switch.is_open());

    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
    for language in languages {
        if snapshots.is_some() {
            zen_i18n::set_language(Some(language));
        }
        open_popup(&mut screen, &mut app);
        let image = screen.render(draw(&mut app));
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("quick-switch-{language}.png")), &image).unwrap();
        }
        let tree = screen.accessibility_tree(draw(&mut app));
        assert!(find(&tree, Role::Heading, &zen_i18n::t!("ide-quick-switch-title")).is_some());
        screen.step(press(Key::Escape, Modifiers::NONE), draw(&mut app));
        for (index, (key, name)) in [
            (Key::Num1, "theme"),
            (Key::Num2, "editor-color-scheme"),
            (Key::Num3, "code-style-scheme"),
            (Key::Num4, "keymap"),
            (Key::Num5, "view-mode"),
            (Key::Num6, "zoom"),
        ]
        .into_iter()
        .enumerate()
        {
            open_popup(&mut screen, &mut app);
            screen.step(press(key, Modifiers::NONE), draw(&mut app));
            let image = screen.render(draw(&mut app));
            if let Some(dir) = &snapshots {
                let file = dir.join(format!("quick-switch-{}-{name}-{language}.png", index + 1));
                crate::offscreen::write_png(&file, &image).unwrap();
            }
            let tree = screen.accessibility_tree(draw(&mut app));
            assert_eq!(crate::accessibility::unnamed(&tree), Vec::<String>::new(), "{name}");
            let checked = tree
                .nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == Role::MenuItemRadio && node.toggled() == Some(egui::accesskit::Toggled::True)
                })
                .count();
            assert_eq!(checked, usize::from(name != "view-mode"), "{name}: one in use, none in View Mode");
            screen.step(press(Key::Escape, Modifiers::NONE), draw(&mut app));
        }
    }
    let image = {
        open_popup(&mut screen, &mut app);
        type_text(&mut screen, &mut app, "k");
        screen.render(draw(&mut app))
    };
    if let Some(dir) = &snapshots {
        crate::offscreen::write_png(&dir.join("quick-switch-filter.png"), &image).unwrap();
    }
    let tree = screen.accessibility_tree(draw(&mut app));
    let search = tree.nodes.iter().find(|(_, node)| node.role() == Role::SearchInput).expect("the search field");
    assert_eq!(search.1.value(), Some("k"));
    assert!(find(&tree, Role::MenuItem, &zen_i18n::t!("ide-quick-switch-keymap")).is_some());
    assert!(find(&tree, Role::MenuItem, &zen_i18n::t!("ide-quick-switch-theme")).is_none(), "filtered out");
    assert_eq!(crate::accessibility::unnamed(&tree), Vec::<String>::new());
}

fn type_text(screen: &mut Offscreen, app: &mut IdeApp, text: &str) {
    screen.step(vec![Event::Text(text.to_owned())], draw(app));
}

fn call(url: &str, id: u64, tool: &str, arguments: Value) -> Value {
    use std::io::{Read, Write};
    let address = url.trim_start_matches("http://").trim_end_matches("/mcp");
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
    let (_, answer) = response.split_once("\r\n\r\n").unwrap();
    let answer: Value = serde_json::from_str(answer).unwrap();
    assert_eq!(answer["result"]["isError"], false, "{tool}: {answer}");
    serde_json::from_str(answer["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn agents_open_read_and_drive_the_popup_over_mcp() {
    let data = tempfile::tempdir().unwrap();
    let mut app = ide(data.path());
    keep_global_keymap();
    app.enable_automation(0);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    let url = app.shared.automation_url.lock().unwrap().clone().expect("the server runs");
    let agent = std::thread::spawn(move || {
        let opened = call(&url, 1, "run_action", json!({ "action": "view.quickSwitchScheme.open" }));
        let theme = call(&url, 2, "press_keys", json!({ "keys": ["1", "3"] }));
        let themes = call(&url, 3, "list_themes", json!({}));
        let again = call(&url, 4, "dispatch_command", json!({ "command": "view.quickSwitchScheme.open" }));
        (opened, theme, themes, again)
    });
    while !agent.is_finished() {
        frame(&ctx, &mut app);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (opened, theme, themes, again) = agent.join().unwrap();
    assert_eq!(opened["window"]["popups"], json!(["quickSwitchScheme"]), "{opened}");
    let groups = opened["result"]["groups"].as_array().unwrap();
    let ids: Vec<&str> = groups.iter().map(|group| group["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["theme", "editorColorScheme", "codeStyleScheme", "keymap", "viewMode", "zoom"]);
    let dusk = &groups[0]["choices"][2];
    assert_eq!(dusk["runs"], json!({ "command": "ui.theme.select", "params": { "id": "dusk" } }), "{dusk}");
    assert_eq!(theme["window"]["popups"], json!([]), "{theme}");
    assert_eq!(themes["current"], "dusk", "{themes}");
    assert_eq!(again["result"]["groups"][0]["choices"][2]["current"], true, "{again}");
}
