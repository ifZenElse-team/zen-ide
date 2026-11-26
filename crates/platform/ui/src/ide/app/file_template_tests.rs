use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;

fn product() -> IdeProduct {
    IdeProduct {
        folding_options: Vec::new(),
        name: "GoGarden",
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

struct Ide {
    app: IdeApp,
    project: PathBuf,
    data: PathBuf,
    _dir: tempfile::TempDir,
}

impl Ide {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir_all(project.join("internal")).unwrap();
        std::fs::write(project.join("internal/worker.go"), "package internal\n\nfunc Work() {}\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let app = IdeApp::new(product(), registry, Some(dir.path().join("recent.json")), vec![project.clone()]);
        let project = app.shared.state().windows[0].path.clone();
        Self { app, project, data: dir.path().to_owned(), _dir: dir }
    }

    fn with_view<T>(&self, change: impl FnOnce(&mut IdeView, &Registry, &mut Session) -> T) -> T {
        let windows = self.app.shared.state().windows.clone();
        let mut window = windows[0].window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *window;
        change(view, &self.app.shared.registry, session)
    }

    fn dispatch(&self, id: &str, params: Value) -> Result<Value, String> {
        self.with_view(|_, registry, session| registry.dispatch(session, id, params).map_err(|error| error.to_string()))
    }
}

fn text_of(answer: &Value) -> Value {
    let text = answer["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{answer}"));
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
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
    let (_, json) = response.split_once("\r\n\r\n").unwrap();
    serde_json::from_str(json).unwrap()
}

const SERVICE: &str = "#parse(\"File Header.go\")\n#set($TYPE = \"${NAME}Service\")\npackage ${PACKAGE}\n\n#if($WITH_LOG == \"yes\")\nimport \"log\"\n\n#end\n// ${DS}Id${DS} ${PRODUCT_NAME} ${PROJECT_NAME}/${DIR_PATH}/${FILE_NAME} ${YEAR}\ntype ${TYPE} struct{}\n";

#[test]
fn agents_create_files_from_templates_over_mcp() {
    let mut ide = Ide::new();
    ide.app.enable_automation(0);
    let ctx = egui::Context::default();
    let (repaint, repaints) = mpsc::channel();
    ctx.set_request_repaint_callback(move |_| {
        let _ = repaint.send(());
    });
    let logic = |app: &mut IdeApp| {
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
    };
    logic(&mut ide.app);
    let url = ide.app.shared.automation_url.lock().unwrap().clone().expect("the server runs");
    let worker = ide.project.join("internal/worker.go");
    let agent = std::thread::spawn(move || {
        let dispatch = |id: u64, command: &str, params: Value| {
            call(&url, id, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let header =
            json!({ "name": "File Header", "extension": "go", "kind": "include", "text": "// Created by ${USER}\n" });
        let service = json!({ "name": "Service", "extension": "go", "fileName": "${NAME}_service", "text": SERVICE, "reformat": false });
        let answers = vec![
            dispatch(1, "file.templates.set", json!({ "template": header, "scheme": "project" })),
            dispatch(2, "file.templates.set", json!({ "template": service, "scheme": "project" })),
            dispatch(
                3,
                "file.templates.render",
                json!({ "template": "Service", "directory": "internal", "name": "orders" }),
            ),
            dispatch(
                4,
                "file.templates.create",
                json!({ "template": "Service", "directory": "internal", "name": "orders" }),
            ),
            dispatch(
                5,
                "file.templates.create",
                json!({ "template": "Service", "directory": "internal", "name": "orders", "variables": { "PACKAGE": "internal", "WITH_LOG": "yes" } }),
            ),
            dispatch(6, "file.document.open", json!({ "path": worker })),
            dispatch(7, "file.templates.saveFileAs", json!({})),
            dispatch(8, "file.templates.list", json!({})),
            call(
                &url,
                9,
                "run_action",
                json!({ "action": "view.fileTemplates.new", "params": { "template": "Service" } }),
            ),
            dispatch(
                10,
                "file.templates.set",
                json!({ "template": { "name": "Bad", "text": "#foreach($x in $y)$x#end" }, "scheme": "default" }),
            ),
            dispatch(11, "file.templates.create", json!({ "template": "Bad", "name": "bad" })),
        ];
        answers
    });
    while !agent.is_finished() {
        if repaints.recv_timeout(Duration::from_millis(50)).is_ok() {
            logic(&mut ide.app);
        }
    }
    let answers = agent.join().unwrap();
    for (index, answer) in answers.iter().enumerate().take(10) {
        if index != 3 {
            assert_eq!(answer["result"]["isError"], false, "call {}: {answer}", index + 1);
        }
    }
    let preview = text_of(&answers[2]);
    assert_eq!(preview["asked"], json!(["NAME", "PACKAGE", "WITH_LOG"]), "{preview}");
    assert_eq!(preview["missing"], json!(["PACKAGE", "WITH_LOG"]), "{preview}");
    assert_eq!(answers[3]["result"]["isError"], true, "a missing value is refused: {}", answers[3]);
    assert!(answers[3].to_string().contains("PACKAGE, WITH_LOG"), "{}", answers[3]);
    let created = ide.project.join("internal/orders_service.go");
    let written = std::fs::read_to_string(&created).unwrap();
    let year = zen_workbench::file_templates::variables::Moment::now().year;
    let user = zen_workbench::file_templates::variables::user();
    assert_eq!(
        written,
        format!(
            "// Created by {user}\npackage internal\n\nimport \"log\"\n\n// $Id$ GoGarden shop/internal/orders_service.go {year}\ntype ordersService struct{{}}\n"
        )
    );
    let idea = ide.project.join(".idea");
    assert!(idea.join("fileTemplates/Service.go.ft").is_file());
    assert!(idea.join("fileTemplates/includes/File Header.go.ft").is_file());
    assert!(
        std::fs::read_to_string(idea.join("file.template.settings.xml"))
            .unwrap()
            .contains("file-name=\"${NAME}_service\"")
    );
    let saved = text_of(&answers[6]);
    assert_eq!((saved["template"]["name"].as_str(), saved["source"].as_str()), (Some("worker"), Some("project")));
    assert!(idea.join("fileTemplates/worker.go.ft").is_file(), "Save File as Template keeps it in the scheme in use");
    let listed = text_of(&answers[7]);
    assert_eq!(listed["scheme"], "project");
    let action = text_of(&answers[8]);
    assert_eq!(action["window"]["popups"], json!(["newFromTemplate"]), "{action}");
    assert_eq!(answers[10]["result"]["isError"], true);
    assert!(answers[10].to_string().contains("#foreach"), "a broken template is reported: {}", answers[10]);
    assert!(ide.data.join("fileTemplates/Bad.ft").is_file());
}

fn draw(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, events: Vec<egui::Event>) {
    screen.step(events, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
}

fn picture(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp) -> crate::offscreen::Frame {
    screen.render(|ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
}

fn click(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str) {
    let tree = screen.accessibility_tree(|ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    let node = crate::accessibility::testing::find(&tree, egui::accesskit::Role::Button, name)
        .unwrap_or_else(|| panic!("no button named {name:?}"));
    let bounds = node.bounds().expect("the button has bounds");
    let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
        draw(screen, app, events);
    }
}

#[test]
fn the_creation_dialog_asks_for_the_name_and_every_variable_without_a_value() {
    let mut ide = Ide::new();
    let handler = json!({ "name": "Handler", "extension": "go", "fileName": "${NAME}_${KIND}", "text": "package ${PACKAGE}\n// ${NAME} handles ${KIND} requests.\n", "reformat": false });
    ide.dispatch("file.templates.set", json!({ "template": handler, "scheme": "default" })).unwrap();
    ide.dispatch(
        "file.templates.set",
        json!({ "template": { "name": "Notes", "extension": "md", "text": "# ${NAME}\n" }, "scheme": "default" }),
    )
    .unwrap();
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0);
    draw(&mut screen, &mut ide.app, Vec::new());
    let opened = ide.with_view(|view, registry, session| {
        view.perform_action(
            &egui::Context::default(),
            registry,
            session,
            "view.fileTemplates.new",
            json!({ "template": "Handler", "directory": "internal" }),
            true,
        )
    });
    assert!(opened.is_ok(), "{opened:?}");
    let fields = ide.with_view(|view, _, _| {
        let dialog = view.new_from_template.as_ref().unwrap();
        (dialog.asks_name(), dialog.values.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>())
    });
    assert_eq!(
        fields,
        (true, vec!["KIND".to_owned(), "PACKAGE".to_owned()]),
        "the name, then the pattern's and the text's variables"
    );
    for _ in 0..3 {
        draw(&mut screen, &mut ide.app, Vec::new());
    }
    if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
        for language in ["en", "ru"] {
            zen_i18n::set_language(Some(language));
            draw(&mut screen, &mut ide.app, Vec::new());
            let image = picture(&mut screen, &mut ide.app);
            crate::offscreen::write_png(&Path::new(&dir).join(format!("new-from-template-{language}.png")), &image)
                .unwrap();
        }
        zen_i18n::set_language(Some("en"));
    }
    let key = |key: egui::Key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    draw(&mut screen, &mut ide.app, vec![egui::Event::Text("user".to_owned())]);
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Tab)]);
    draw(&mut screen, &mut ide.app, vec![egui::Event::Text("http".to_owned())]);
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Tab)]);
    draw(&mut screen, &mut ide.app, vec![egui::Event::Text("api".to_owned())]);
    let typed = ide.with_view(|view, _, _| {
        let dialog = view.new_from_template.as_ref().unwrap();
        (dialog.name.clone(), dialog.values.clone())
    });
    assert_eq!(
        typed,
        ("user".to_owned(), vec![("KIND".to_owned(), "http".to_owned()), ("PACKAGE".to_owned(), "api".to_owned())])
    );
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Enter)]);
    draw(&mut screen, &mut ide.app, Vec::new());
    let created = ide.project.join("internal/user_http.go");
    assert_eq!(std::fs::read_to_string(&created).unwrap(), "package api\n// user handles http requests.\n");
    assert!(ide.with_view(|view, _, _| view.new_from_template.is_none()), "the dialog closes");
    let front = ide.with_view(|_, _, session| {
        session.state::<Workbench>().unwrap().active_document().map(|d| d.path().to_owned())
    });
    assert_eq!(front, Some(created), "the new file is open in front");
}

#[test]
fn a_template_with_kinds_asks_for_the_kind_in_its_dialog() {
    let mut ide = Ide::new();
    let declared = zen_workbench::file_templates::Declared::of_plugin(
        "go",
        &[
            json!({ "name": "Go File", "extension": "go", "text": "package x\n// ${NAME}\n" }),
            json!({ "name": "App", "extension": "go", "kindOf": "Go File", "text": "package main\n// ${NAME}\n" }),
            json!({ "name": "Other", "extension": "go", "text": "package y\n// ${NAME}\n" }),
        ],
        &[],
    );
    ide.app.shared.file_templates.declare(declared);
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0);
    draw(&mut screen, &mut ide.app, Vec::new());
    let folder = ide.project.join("internal");
    let opened = ide.with_view(|view, registry, session| {
        view.open_new_from_template(registry, session, Some("Go File"), Some(folder), true)
    });
    assert!(opened.is_ok(), "{opened:?}");
    for _ in 0..3 {
        draw(&mut screen, &mut ide.app, Vec::new());
    }
    choose(&mut screen, &mut ide.app, "Kind:", "App");
    let params = ide.with_view(|view, _, _| view.new_from_template.as_ref().unwrap().create_params().unwrap());
    assert_eq!(params["template"], "App");
}

#[test]
fn save_file_as_template_fills_the_editor_from_the_file_in_front() {
    let mut ide = Ide::new();
    let worker = ide.project.join("internal/worker.go");
    ide.dispatch("file.document.open", json!({ "path": worker })).unwrap();
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 720.0), 2.0);
    draw(&mut screen, &mut ide.app, Vec::new());
    let opened = ide.with_view(|view, registry, session| {
        view.perform_action(&egui::Context::default(), registry, session, "view.fileTemplates.saveAs", json!({}), true)
    });
    assert!(opened.is_ok(), "{opened:?}");
    let template = ide.with_view(|view, _, _| view.save_as_template.as_ref().unwrap().template.clone());
    assert_eq!((template.name.as_str(), template.extension.as_str()), ("worker", "go"));
    assert_eq!(template.text, "package internal\n\nfunc Work() {}\n");
    for _ in 0..3 {
        draw(&mut screen, &mut ide.app, Vec::new());
    }
    if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
        for language in ["en", "ru"] {
            zen_i18n::set_language(Some(language));
            draw(&mut screen, &mut ide.app, Vec::new());
            let image = picture(&mut screen, &mut ide.app);
            crate::offscreen::write_png(&Path::new(&dir).join(format!("save-file-as-template-{language}.png")), &image)
                .unwrap();
        }
        zen_i18n::set_language(Some("en"));
    }
    ide.with_view(|view, _, _| {
        let dialog = view.save_as_template.as_mut().unwrap();
        dialog.template.name = "Worker".to_owned();
        dialog.template.file_name = "${NAME}_worker".to_owned();
    });
    click(&mut screen, &mut ide.app, "OK");
    assert!(ide.with_view(|view, _, _| view.save_as_template.is_none()), "OK saves and closes");
    let listed = ide.dispatch("file.templates.list", json!({})).unwrap();
    assert_eq!(listed["templates"][0]["template"]["fileName"], "${NAME}_worker");
    assert!(ide.data.join("fileTemplates/Worker.go.ft").is_file());
    let menu = super::super::main_menu::MENUS[0].1;
    assert!(
        menu.iter().any(|entry| matches!(entry, super::super::main_menu::Entry::Action("view.fileTemplates.saveAs")))
    );
    let refused = ide.with_view(|view, registry, session| {
        view.save_as_template = None;
        registry.dispatch(session, "file.document.close", json!({})).ok();
        view.perform_action(&egui::Context::default(), registry, session, "view.fileTemplates.saveAs", json!({}), true)
    });
    assert!(refused.is_err(), "without a file in front there is nothing to save");
}

fn snapshot_languages() -> &'static [&'static str] {
    if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { &["en", "ru"] } else { &[] }
}

fn press(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, role: egui::accesskit::Role, name: &str) {
    let tree = screen.accessibility_tree(|ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    let node = tree
        .nodes
        .iter()
        .rev()
        .find(|(_, node)| {
            node.role() == role
                && crate::accessibility::accessible_name(node, &tree.nodes).is_some_and(|found| found.contains(name))
        })
        .map(|(_, node)| node)
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    let bounds = node.bounds().expect("it has bounds");
    let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
        draw(screen, app, events);
    }
}

fn choose(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, list: &str, option: &str) {
    press(screen, app, egui::accesskit::Role::ComboBox, list);
    let tree = screen.accessibility_tree(|ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    let nodes = &tree.nodes;
    let named = |role: egui::accesskit::Role, name: &str, exact: bool| -> Vec<&egui::accesskit::Node> {
        nodes
            .iter()
            .filter(|(_, node)| {
                node.role() == role
                    && crate::accessibility::accessible_name(node, nodes)
                        .is_some_and(|found| if exact { found == name } else { found.contains(name) })
            })
            .map(|(_, node)| node)
            .collect()
    };
    let combo = named(egui::accesskit::Role::ComboBox, list, false).into_iter().next().expect("the list");
    let below = combo.bounds().expect("it has bounds").y1;
    let node = named(egui::accesskit::Role::Button, option, true)
        .into_iter()
        .filter_map(|node| Some(((node.bounds()?.y0 - below).abs(), node)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, found)| found)
        .unwrap_or_else(|| panic!("no option {option:?} of {list:?}"));
    let bounds = node.bounds().expect("it has bounds");
    let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
        draw(screen, app, events);
    }
}

fn disabled(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str) -> bool {
    let tree = screen.accessibility_tree(|ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    crate::accessibility::testing::find(&tree, egui::accesskit::Role::Button, name)
        .unwrap_or_else(|| panic!("no button named {name:?}"))
        .is_disabled()
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn snapshot(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str, language: &str) {
    if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
        let image = picture(screen, app);
        crate::offscreen::write_png(&Path::new(&dir).join(format!("{name}-{language}.png")), &image).unwrap();
    }
}

fn declare_builtin(ide: &Ide) {
    ide.app.shared.file_templates.declare(zen_workbench::file_templates::Declared {
        origin: "web".to_owned(),
        templates: zen_workbench::file_templates::FileTemplate::parse_list(
            r#"[
                { "name": "HTML File", "extension": "html", "text": "<!DOCTYPE html>\n<title>${NAME}</title>\n" },
                { "name": "Go File", "extension": "go", "text": "package ${NAME}\n" },
                { "name": "HTML Header", "kind": "include", "extension": "html", "text": "<!-- ${USER} -->\n" }
            ]"#,
        )
        .unwrap(),
        variables: Vec::new(),
        values: None,
        offers: vec![(
            "Go File".to_owned(),
            zen_workbench::file_templates::Offer { markers: vec!["go.mod".to_owned()], ..Default::default() },
        )],
    });
}

const SERVICE_TEXT: &str = "package ${PACKAGE}\n\n// Owner: ${OWNER}\ntype ${NAME} struct {\n\t#[[$END$]]#\n}\n";

fn new_items(ide: &Ide) -> Vec<String> {
    ide.with_view(|view, _, _| view.new_popup.as_ref().map(|_| view.new_popup_titles()).unwrap_or_default())
}

#[test]
fn command_n_in_the_project_tool_window_opens_the_new_popup_and_a_template_opens_at_its_end() {
    let mut ide = Ide::new();
    declare_builtin(&ide);
    let service =
        json!({ "name": "Service", "extension": "go", "text": SERVICE_TEXT, "reformat": false, "liveTemplates": true });
    ide.dispatch("file.templates.set", json!({ "template": service, "scheme": "default" })).unwrap();
    let internal = ide.project.join("internal");
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0);
    draw(&mut screen, &mut ide.app, Vec::new());
    ide.with_view(|view, _, _| {
        view.selection.select(internal.clone());
        view.tree_focused = true;
    });
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::N, egui::Modifiers::COMMAND)]);
    draw(&mut screen, &mut ide.app, Vec::new());
    let popups = ide.with_view(|view, _, session| view.window_state(session)["popups"].clone());
    assert_eq!(popups, json!(["newElement"]), "⌘N in the Project tool window opens New");
    assert_eq!(
        new_items(&ide),
        ["File", "Scratch File", "Directory", "HTML File", "Service", "EditorConfig File"],
        "Go File needs a go.mod above the folder"
    );
    let listed = ide.dispatch("file.new.list", json!({ "directory": "internal" })).unwrap();
    let titles: Vec<&str> =
        listed["items"].as_array().unwrap().iter().map(|item| item["title"].as_str().unwrap()).collect();
    let plugged = ["Scratch File", "EditorConfig File"];
    let shown: Vec<String> = new_items(&ide).into_iter().filter(|title| !plugged.contains(&title.as_str())).collect();
    assert_eq!(titles, shown, "agents get the same list but the plugged window actions");
    let menu = ide.with_view(|view, _, session| {
        view.cached_menu_items(session).into_iter().map(|item| item.key()).collect::<Vec<_>>()
    });
    assert_eq!(
        menu,
        [
            "new:file",
            "new:action:view.scratch.new",
            "new:directory",
            "new:template:HTML File",
            "new:template:Service",
            "new:action:view.editorConfig.new"
        ],
        "File → New lists the same items here"
    );
    for language in snapshot_languages() {
        zen_i18n::with_language(language, || {
            ide.with_view(|view, _, session| {
                view.new_items = super::super::new_popup::NewItemsCache::default();
                view.open_new_popup(session, Some(internal.clone()), None).unwrap();
            });
            draw(&mut screen, &mut ide.app, Vec::new());
            draw(&mut screen, &mut ide.app, Vec::new());
            snapshot(&mut screen, &mut ide.app, "new-popup", language);
        });
    }
    ide.with_view(|view, _, session| {
        view.new_items = super::super::new_popup::NewItemsCache::default();
        view.open_new_popup(session, Some(internal.clone()), None).unwrap();
    });
    draw(&mut screen, &mut ide.app, Vec::new());
    for _ in 0..4 {
        draw(&mut screen, &mut ide.app, vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)]);
    }
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    draw(&mut screen, &mut ide.app, Vec::new());
    let dialog = ide.with_view(|view, _, session| {
        let dialog = view
            .new_from_template
            .as_ref()
            .map(|dialog| (dialog.asks_name(), dialog.values.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>()));
        (dialog, view.window_state(session)["popups"].clone())
    });
    assert_eq!(dialog.0, Some((true, vec!["PACKAGE".to_owned(), "OWNER".to_owned()])), "the name and the variables");
    assert_eq!(dialog.1, json!(["newFromTemplate"]), "the popup gives way to the dialog");
    for language in snapshot_languages() {
        zen_i18n::with_language(language, || {
            draw(&mut screen, &mut ide.app, Vec::new());
            snapshot(&mut screen, &mut ide.app, "new-service-dialog", language);
        });
    }
    draw(&mut screen, &mut ide.app, vec![egui::Event::Text("Orders".to_owned())]);
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    draw(&mut screen, &mut ide.app, vec![egui::Event::Text("internal".to_owned())]);
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    draw(&mut screen, &mut ide.app, vec![egui::Event::Text("ann".to_owned())]);
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    draw(&mut screen, &mut ide.app, Vec::new());
    let created = internal.join("Orders.go");
    let text = std::fs::read_to_string(&created).unwrap();
    assert_eq!(text, "package internal\n\n// Owner: ann\ntype Orders struct {\n\t\n}\n");
    let (front, caret) = ide.with_view(|_, _, session| {
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        (document.path().to_owned(), document.selections().primary())
    });
    assert_eq!(front, created, "the new file opens in front");
    let end = text.find("\t\n}").unwrap() + 1;
    assert_eq!((caret.anchor, caret.head), (end, end), "the caret is at $END$");
    for language in snapshot_languages() {
        zen_i18n::with_language(language, || {
            draw(&mut screen, &mut ide.app, Vec::new());
            snapshot(&mut screen, &mut ide.app, "new-service-created", language);
        });
    }

    ide.with_view(|view, _, session| view.open_new_popup(session, Some(internal.clone()), None).unwrap());
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    draw(&mut screen, &mut ide.app, Vec::new());
    let popups = ide.with_view(|view, _, session| view.window_state(session)["popups"].clone());
    assert_eq!(popups, json!(["newEntry"]), "File asks for the new file's name");
    ide.with_view(|view, _, _| view.new_entry = None);

    let refused = ide.with_view(|view, registry, session| {
        view.tree_focused = false;
        view.perform_action(&egui::Context::default(), registry, session, "view.newElement.open", json!({}), false)
    });
    assert!(refused.is_err(), "⌘N in the editor is not New");
    let agent = ide.with_view(|view, registry, session| {
        view.perform_action(
            &egui::Context::default(),
            registry,
            session,
            "view.newElement.open",
            json!({ "directory": "internal" }),
            true,
        )
    });
    assert!(agent.is_ok(), "agents open it anywhere: {agent:?}");
}

#[test]
fn the_tree_context_menu_new_has_the_same_items() {
    let ide = Ide::new();
    declare_builtin(&ide);
    let internal = ide.project.join("internal");
    std::fs::write(ide.project.join("go.mod"), "module shop\n").unwrap();
    let titles = ide.with_view(|_, _, session| {
        super::super::new_popup::items(session, &internal).into_iter().map(|item| item.title).collect::<Vec<_>>()
    });
    assert_eq!(titles, ["File", "Scratch File", "Directory", "HTML File", "Go File", "EditorConfig File"]);
}

#[test]
fn the_file_and_code_templates_page_edits_resets_and_switches_the_scheme() {
    let mut ide = Ide::new();
    declare_builtin(&ide);
    ide.dispatch(
        "file.templates.set",
        json!({ "template": { "name": "Mine", "extension": "txt", "text": "mine ${NAME}\n" }, "scheme": "default" }),
    )
    .unwrap();
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1200.0, 760.0), 2.0);
    draw(&mut screen, &mut ide.app, Vec::new());
    let opened = ide.with_view(|view, registry, session| {
        view.perform_action(
            &egui::Context::default(),
            registry,
            session,
            "view.settings.open",
            json!({ "page": "fileTemplates" }),
            true,
        )
    });
    assert!(opened.is_ok(), "{opened:?}");
    for _ in 0..3 {
        draw(&mut screen, &mut ide.app, Vec::new());
    }
    use egui::accesskit::Role;
    press(&mut screen, &mut ide.app, Role::ListBoxOption, "HTML File.html");
    assert!(disabled(&mut screen, &mut ide.app, "Remove Template"), "a built-in template cannot be removed");
    assert!(disabled(&mut screen, &mut ide.app, "Revert to Original Template"), "nothing to reset yet");
    assert!(!disabled(&mut screen, &mut ide.app, "Create Child Template File"));
    press(&mut screen, &mut ide.app, Role::Button, "Create Child Template File");
    assert!(!disabled(&mut screen, &mut ide.app, "Remove Template"), "its child can be removed");
    press(&mut screen, &mut ide.app, Role::ListBoxOption, "HTML File.html");
    assert!(
        !disabled(&mut screen, &mut ide.app, "Revert to Original Template"),
        "a changed built-in template can be reset"
    );
    for language in snapshot_languages() {
        zen_i18n::with_language(language, || {
            draw(&mut screen, &mut ide.app, Vec::new());
            draw(&mut screen, &mut ide.app, Vec::new());
            snapshot(&mut screen, &mut ide.app, "file-templates-files", language);
        });
    }
    press(&mut screen, &mut ide.app, Role::Button, "Apply");
    let html = ide.dispatch("file.templates.list", json!({ "kind": "file" })).unwrap()["templates"][0].clone();
    assert_eq!(html["template"]["name"], "HTML File");
    assert_eq!(
        (html["modified"].as_bool(), html["template"]["children"].as_array().map(Vec::len)),
        (Some(true), Some(1))
    );
    assert!(ide.data.join("fileTemplates/HTML File.html.child.0.html").is_file());

    press(&mut screen, &mut ide.app, Role::ListBoxOption, "HTML File.html");
    press(&mut screen, &mut ide.app, Role::Button, "Revert to Original Template");
    press(&mut screen, &mut ide.app, Role::ListBoxOption, "Mine.txt");
    press(&mut screen, &mut ide.app, Role::Button, "Copy Template");
    press(&mut screen, &mut ide.app, Role::Button, "Apply");
    let listed = ide.dispatch("file.templates.list", json!({ "kind": "file" })).unwrap();
    let names: Vec<&str> =
        listed["templates"].as_array().unwrap().iter().map(|t| t["template"]["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["HTML File", "Go File", "Mine", "Mine copy"]);
    assert_eq!(listed["templates"][0]["modified"], false, "Revert to Original Template took it back");

    press(&mut screen, &mut ide.app, Role::ListBoxOption, "Mine copy.txt");
    press(&mut screen, &mut ide.app, Role::Button, "Remove Template");
    press(&mut screen, &mut ide.app, Role::Tab, "Includes");
    press(&mut screen, &mut ide.app, Role::ListBoxOption, "HTML Header.html");
    for language in snapshot_languages() {
        zen_i18n::with_language(language, || {
            draw(&mut screen, &mut ide.app, Vec::new());
            draw(&mut screen, &mut ide.app, Vec::new());
            snapshot(&mut screen, &mut ide.app, "file-templates-includes", language);
        });
    }
    press(&mut screen, &mut ide.app, Role::Tab, "Code");
    assert!(disabled(&mut screen, &mut ide.app, "Create Template"), "Code templates are made by the IDE");
    choose(&mut screen, &mut ide.app, "Scheme", "Project");
    for language in snapshot_languages() {
        zen_i18n::with_language(language, || {
            draw(&mut screen, &mut ide.app, Vec::new());
            draw(&mut screen, &mut ide.app, Vec::new());
            snapshot(&mut screen, &mut ide.app, "file-templates-project", language);
        });
    }
    press(&mut screen, &mut ide.app, Role::DefaultButton, "OK");
    let listed = ide.dispatch("file.templates.list", json!({})).unwrap();
    assert_eq!(listed["scheme"], "project", "the Project scheme is in use");
    let names: Vec<String> = listed["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["template"]["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(!names.contains(&"Mine copy".to_owned()), "the removed copy is gone: {names:?}");
}

#[test]
fn the_page_keeps_a_draft_cancel_drops_it_and_changes_made_meanwhile_survive_apply() {
    let mut ide = Ide::new();
    ide.dispatch(
        "file.templates.set",
        json!({ "template": { "name": "Mine", "extension": "txt", "text": "mine\n" }, "scheme": "default" }),
    )
    .unwrap();
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1200.0, 760.0), 1.0);
    let names = |ide: &Ide| -> Vec<String> {
        let listed = ide.dispatch("file.templates.list", json!({ "kind": "file" })).unwrap();
        listed["templates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["template"]["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let open = |ide: &mut Ide, screen: &mut crate::offscreen::Offscreen| {
        draw(screen, &mut ide.app, Vec::new());
        ide.with_view(|view, registry, session| {
            view.perform_action(
                &egui::Context::default(),
                registry,
                session,
                "view.settings.open",
                json!({ "page": "fileTemplates" }),
                true,
            )
        })
        .unwrap();
        for _ in 0..3 {
            draw(screen, &mut ide.app, Vec::new());
        }
    };
    use egui::accesskit::Role;
    open(&mut ide, &mut screen);
    press(&mut screen, &mut ide.app, Role::Button, "Create Template");
    assert_eq!(names(&ide), ["Mine"], "the page edits a draft: nothing is saved before OK or Apply");
    ide.dispatch(
        "file.templates.set",
        json!({ "template": { "name": "Agent", "extension": "txt", "text": "agent\n" }, "scheme": "default" }),
    )
    .unwrap();
    press(&mut screen, &mut ide.app, Role::Button, "Cancel");
    assert_eq!(names(&ide), ["Agent", "Mine"], "Cancel drops the draft and keeps what an agent saved meanwhile");

    open(&mut ide, &mut screen);
    press(&mut screen, &mut ide.app, Role::Button, "Create Template");
    ide.dispatch(
        "file.templates.set",
        json!({ "template": { "name": "Mine", "extension": "txt", "text": "changed by an agent\n" }, "scheme": "default" }),
    )
    .unwrap();
    ide.dispatch(
        "file.templates.set",
        json!({ "template": { "name": "Later", "extension": "txt", "text": "later\n" }, "scheme": "default" }),
    )
    .unwrap();
    press(&mut screen, &mut ide.app, Role::Button, "Apply");
    assert_eq!(names(&ide), ["Agent", "Later", "Mine", "Unnamed"], "Apply sends only what the page changed");
    let listed = ide.dispatch("file.templates.list", json!({ "kind": "file" })).unwrap();
    let mine =
        listed["templates"].as_array().unwrap().iter().find(|t| t["template"]["name"] == "Mine").unwrap().clone();
    assert_eq!(
        mine["template"]["text"], "changed by an agent\n",
        "a template the page did not touch is not overwritten"
    );
}

fn declare_java(ide: &Ide) {
    let templates = ide
        .with_view(|_, _, session| session.state::<zen_workbench::file_templates::FileTemplates>().cloned().unwrap());
    let manifest: Value = serde_json::from_str(include_str!("../../../../../../plugins/java/plugin.json")).unwrap();
    let declared = manifest["fileTemplates"]["templates"].as_array().unwrap();
    templates.declare(zen_workbench::file_templates::Declared::of_plugin("Java", declared, &[]));
}

fn popup_titles(ide: &Ide, folder: &Path) -> Vec<String> {
    ide.with_view(|view, _, session| {
        view.new_items = super::super::new_popup::NewItemsCache::default();
        view.open_new_popup(session, Some(folder.to_owned()), None).unwrap();
        let titles = view.new_popup_titles();
        view.new_popup = None;
        titles
    })
}

#[test]
fn a_java_source_folder_offers_the_class_templates_file_and_package_and_other_folders_do_not() {
    let ide = Ide::new();
    declare_java(&ide);
    let sources = ide.project.join("src/main/java/com/acme");
    std::fs::create_dir_all(&sources).unwrap();
    let expected = ["Java Class", "File", "Package", "package-info.java", "module-info.java"];
    assert_eq!(popup_titles(&ide, &ide.project.join("src/main/java")), expected);
    assert_eq!(popup_titles(&ide, &sources), expected, "a package is in the source folder");
    let resources = ide.project.join("src/main/resources");
    std::fs::create_dir_all(&resources).unwrap();
    let plain = popup_titles(&ide, &resources);
    assert!(plain.contains(&"Directory".to_owned()));
    assert!(!plain.contains(&"Java Class".to_owned()), "no Java Class outside source folders: {plain:?}");
}

#[test]
fn file_new_never_offers_a_class_whatever_is_selected() {
    let ide = Ide::new();
    declare_java(&ide);
    std::fs::write(ide.project.join("pom.xml"), "<project/>").unwrap();
    let sources = ide.project.join("src/main/java");
    std::fs::create_dir_all(&sources).unwrap();
    let keys = ide.with_view(|view, _, session| {
        view.selection.select(sources.clone());
        view.tree_focused = true;
        view.new_items = super::super::new_popup::NewItemsCache::default();
        view.cached_menu_items(session).into_iter().map(|item| item.title).collect::<Vec<_>>()
    });
    for class in ["Java Class", "Java Interface", "Java Enum", "Java Record", "Java Annotation"] {
        assert!(!keys.contains(&class.to_owned()), "{keys:?}");
    }
    assert!(!keys.iter().any(|title| title.contains("-info")), "{keys:?}");
    assert_eq!(&keys[..3], ["File", "Scratch File", "Directory"]);
}

#[test]
fn the_new_popup_filters_by_its_search_and_enter_takes_the_first_shown() {
    let mut ide = Ide::new();
    declare_java(&ide);
    let sources = ide.project.join("src/main/java");
    std::fs::create_dir_all(&sources).unwrap();
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0);
    draw(&mut screen, &mut ide.app, Vec::new());
    ide.with_view(|view, _, session| view.open_new_popup(session, Some(sources.clone()), None).unwrap());
    draw(&mut screen, &mut ide.app, Vec::new());
    draw(&mut screen, &mut ide.app, Vec::new());
    assert!(ide.with_view(|view, _, _| view.new_popup_titles()).len() > 1);
    draw(&mut screen, &mut ide.app, vec![egui::Event::Text("pack".to_owned())]);
    draw(&mut screen, &mut ide.app, Vec::new());
    let shown = ide.with_view(|view, _, _| view.new_popup_titles());
    assert_eq!(shown, ["Package", "package-info.java"]);
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    draw(&mut screen, &mut ide.app, Vec::new());
    let entry = ide.with_view(|view, _, _| view.new_entry.as_ref().map(|entry| entry.package));
    assert_eq!(entry, Some(true), "Package asks for a name");
}

#[test]
fn a_new_package_s_dots_make_nested_folders() {
    let mut ide = Ide::new();
    declare_java(&ide);
    let sources = ide.project.join("src/main/java");
    std::fs::create_dir_all(&sources).unwrap();
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0);
    draw(&mut screen, &mut ide.app, Vec::new());
    ide.with_view(|view, _, _| view.pending_tree_action = Some(super::super::TreeAction::NewPackage(sources.clone())));
    draw(&mut screen, &mut ide.app, Vec::new());
    draw(&mut screen, &mut ide.app, Vec::new());
    draw(&mut screen, &mut ide.app, vec![egui::Event::Text("com.acme".to_owned())]);
    draw(&mut screen, &mut ide.app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    draw(&mut screen, &mut ide.app, Vec::new());
    assert!(sources.join("com/acme").is_dir());
}

#[test]
fn the_java_class_row_opens_the_name_dialog_with_the_kinds_of_class() {
    let ide = Ide::new();
    declare_java(&ide);
    let sources = ide.project.join("src/main/java");
    std::fs::create_dir_all(&sources).unwrap();
    let kinds = ide.with_view(|view, registry, session| {
        view.open_new_in_group(registry, session, "Java Class", sources.clone()).unwrap();
        view.new_from_template.as_ref().unwrap().template_names()
    });
    assert_eq!(kinds, ["Java Class", "Java Interface", "Java Enum", "Java Record", "Java Annotation"]);
}

#[test]
fn the_new_lists_of_several_folders_are_kept_between_frames() {
    let ide = Ide::new();
    let internal = ide.project.join("internal");
    let project = ide.project.clone();
    let titles = |ide: &Ide, folder: &Path| -> Vec<String> {
        ide.with_view(|view, _, session| {
            view.cached_new_items(session, folder).into_iter().map(|item| item.title).collect::<Vec<_>>()
        })
    };
    let before = titles(&ide, &internal);
    assert_eq!(titles(&ide, &project), before);
    std::fs::create_dir_all(project.join(".idea/fileTemplates")).unwrap();
    std::fs::write(project.join(".idea/fileTemplates/Disk.txt.ft"), "disk\n").unwrap();
    assert_eq!(titles(&ide, &internal), before, "the tree's menu and File → New do not read the disk every frame");
    assert_eq!(titles(&ide, &project), before);
    ide.with_view(|view, _, _| view.new_items = super::super::new_popup::NewItemsCache::default());
    assert!(titles(&ide, &internal).contains(&"Disk".to_owned()), "read again once dropped");
}
