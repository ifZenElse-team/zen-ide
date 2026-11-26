use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2};
use serde_json::{Value, json};
use zen_workbench::language::ServerSpec;

use super::*;
use crate::offscreen::Offscreen;

const GO_MOD: &str = "module example.com/shop\n\ngo 1.22\n";
const CART: &str = "package shop\n\n// Total sums the prices.\nfunc Total(prices []int) int {\n\treturn sum(prices)\n}\n\nfunc sum(values []int) int {\n\tn := 0\n\tfor _, v := range values {\n\t\tn += v\n\t}\n\treturn n\n}\n\n// Walk counts down to zero.\nfunc Walk(n int) int {\n\tif n == 0 {\n\t\treturn 0\n\t}\n\treturn Walk(n - 1)\n}\n\n// Use calls Walk.\nfunc Use() int { return Walk(3) }\n";
const RUN: &str = "package shop\n\n// Run totals an empty cart.\nfunc Run() int { return Total(nil) }\n\n// Checkout totals two prices.\nfunc Checkout() int { return Total([]int{1, 2}) }\n";
const TEST: &str = "package shop\n\nimport \"testing\"\n\nfunc TestTotal(t *testing.T) {\n\tif Total([]int{2}) != 2 {\n\t\tt.Fatal(\"total\")\n\t}\n}\n";
const TYPES: &str = "package shop\n\n// Store keeps things.\ntype Store interface{ Get() int }\n\n// Memory is a Store in memory.\ntype Memory struct{}\n\n// Get returns nothing.\nfunc (Memory) Get() int { return 0 }\n\n// Disk is another Store.\ntype Disk struct{}\n\n// Get reads one.\nfunc (Disk) Get() int { return 1 }\n";

struct Ide {
    app: IdeApp,
    screen: Offscreen,
    project: PathBuf,
    scale: f32,
    label: &'static str,
    _data: tempfile::TempDir,
}

fn chars(text: &str, needle: &str) -> usize {
    text[..text.find(needle).unwrap()].chars().count()
}

impl Ide {
    fn new(label: &'static str, spec: impl FnOnce(&Path) -> ServerSpec) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().canonicalize().unwrap().join("shop");
        std::fs::create_dir_all(&project).unwrap();
        for (name, text) in
            [("go.mod", GO_MOD), ("cart.go", CART), ("run.go", RUN), ("cart_test.go", TEST), ("types.go", TYPES)]
        {
            std::fs::write(project.join(name), text).unwrap();
        }
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct { name: "Test", id: "test", version: "0.0.0", mcp_port: 0, ..IdeProduct::default() };
        let app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        let spec = spec(&project);
        app.with_session(|session| {
            session.state_mut::<zen_workbench::language::Languages>().set_specs(vec![spec]);
        })
        .expect("the project is open");
        let scale = if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { 2.0 } else { 1.0 };
        let screen = Offscreen::new(egui::vec2(1280.0, 760.0), scale);
        let mut ide = Self { app, screen, project, scale, label, _data: data };
        ide.step(Vec::new());
        ide
    }

    fn step(&mut self, events: Vec<Event>) {
        let app = &mut self.app;
        self.screen.step(events, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
    }

    fn render(&mut self) -> crate::offscreen::Frame {
        let app = &mut self.app;
        self.screen.render(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
    }

    fn ask(&mut self, tool: &str, arguments: Value) -> Result<Value, String> {
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
        for _ in 0..10 {
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            self.step(Vec::new());
        }
        answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments}"))
    }

    fn run(&mut self, command: &str, params: Value) -> Result<Value, String> {
        self.ask("dispatch_command", json!({ "command": command, "params": params }))
    }

    fn keys(&mut self, keys: &[&str]) {
        self.ask("press_keys", json!({ "keys": keys })).unwrap();
        self.step(Vec::new());
    }

    fn view<T>(&self, f: impl FnOnce(&IdeView, &Session) -> T) -> T {
        let windows = self.app.shared.state().windows.clone();
        let window = windows[0].window.lock().unwrap();
        f(&window.view, &window.session)
    }

    fn caret(&self) -> (PathBuf, usize) {
        self.view(|_, session| {
            let document = session.state::<Workbench>().unwrap().active_document().unwrap();
            (document.path().to_owned(), document.selections().primary().head)
        })
    }

    fn put_caret(&mut self, file: &str, offset: usize) {
        let path = self.project.join(file);
        self.run("navigate.place.goTo", json!({ "path": path, "offset": offset })).unwrap();
        self.step(Vec::new());
    }

    fn settled(&mut self, wanted: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let state = self.run("navigate.hierarchy.get", json!({ "waitMs": 500 })).unwrap();
            if state["pending"] == false && wanted(&state) {
                self.step(Vec::new());
                return state;
            }
            assert!(Instant::now() < deadline, "the hierarchy never settled: {state}");
            self.step(Vec::new());
        }
    }

    fn row(&self, node: &Value) -> egui::Rect {
        let id = node["id"].as_u64().unwrap() as usize;
        self.view(|view, _| view.hierarchy_window.row_rect(id)).unwrap_or_else(|| panic!("no row for {node}"))
    }

    fn click(&mut self, at: Pos2, count: usize) {
        self.step(vec![Event::PointerMoved(at)]);
        for _ in 0..count {
            for pressed in [true, false] {
                let event = Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                };
                self.step(vec![event]);
            }
        }
        for _ in 0..3 {
            self.step(Vec::new());
        }
    }

    fn snapshot(&mut self, name: &str) {
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
        for language in ["en", "ru"] {
            zen_i18n::set_language(Some(language));
            let image = self.render();
            crate::offscreen::write_png(
                &Path::new(&dir).join(format!("hierarchy-{}-{name}-{language}.png", self.label)),
                &image,
            )
            .unwrap();
        }
        zen_i18n::set_language(Some("en"));
        self.step(Vec::new());
    }

    fn tool_window(&mut self) -> Value {
        let state = self.ask("get_window_state", json!({})).unwrap();
        state["toolWindows"]["windows"]
            .as_array()
            .and_then(|windows| windows.iter().find(|window| window["id"] == "hierarchy").cloned())
            .unwrap_or_else(|| panic!("no hierarchy in {state}"))
    }
}

fn names(state: &Value) -> Vec<String> {
    state["nodes"].as_array().unwrap().iter().map(|node| node["name"].as_str().unwrap().to_owned()).collect()
}

fn node<'a>(state: &'a Value, name: &str) -> &'a Value {
    state["nodes"].as_array().unwrap().iter().find(|node| node["name"] == name).unwrap_or_else(|| panic!("{name}"))
}

fn walk_the_hierarchy_window(mut ide: Ide) {
    assert_eq!(ide.tool_window()["visible"], false, "hidden until a hierarchy is asked for");
    ide.run("file.document.open", json!({ "path": ide.project.join("cart.go") })).unwrap();
    ide.put_caret("cart.go", chars(CART, "Total(prices") + 2);
    ide.keys(&["⌃⌥H"]);
    let window = ide.tool_window();
    assert_eq!((window["visible"].clone(), window["side"].clone()), (json!(true), json!("rightTop")), "{window}");
    let state = ide.settled(|state| state["nodes"].as_array().is_some_and(|nodes| nodes.len() >= 4));
    assert_eq!((state["kind"].clone(), state["direction"].clone()), (json!("call"), json!("callers")), "{state}");
    let mut callers = names(&state);
    callers.sort();
    assert_eq!(callers, ["Checkout", "Run", "TestTotal", "Total"], "{state}");
    assert_eq!(node(&state, "TestTotal")["location"], "test");
    assert_eq!(node(&state, "Run")["children"], Value::Null, "deeper levels wait to be expanded");
    ide.snapshot("callers");

    let test_row = ide.row(node(&state, "TestTotal"));
    let image = ide.render();
    let at = |rect: egui::Rect| (((rect.left() + 2.0) * ide.scale) as usize, (rect.center().y * ide.scale) as usize);
    let pixel = image[at(test_row)];
    let colour = ide.view(|view, _| view.palette.test_row);
    assert_eq!(pixel, colour, "a test node is drawn in the test colour");
    let plain = ide.row(node(&state, "Run"));
    assert_ne!(image[at(plain)], colour);

    let origin = ide.caret();
    let run = ide.row(node(&state, "Run"));
    ide.click(run.center(), 2);
    let run_go = ide.project.join("run.go");
    assert_eq!(ide.caret(), (run_go.clone(), chars(RUN, "Run()")), "a double click goes to the code");
    ide.run("navigate.history.back", json!({})).unwrap();
    assert_eq!(ide.caret(), origin, "and leaves a place for Back");
    let checkout = ide.row(node(&state, "Checkout"));
    ide.click(checkout.center(), 1);
    assert_eq!(ide.caret(), origin, "a click only selects");
    ide.keys(&["F4"]);
    assert_eq!(ide.caret(), (run_go.clone(), chars(RUN, "Checkout()")), "F4 goes to the code");
    ide.run("navigate.history.back", json!({})).unwrap();
    let total = ide.row(node(&state, "Total"));
    ide.click(total.center(), 1);
    ide.keys(&["↓", "⏎"]);
    let first = state["nodes"][1]["name"].as_str().unwrap().to_owned();
    assert_eq!(ide.caret().0.file_name().unwrap(), if first == "TestTotal" { "cart_test.go" } else { "run.go" });
    ide.run("navigate.history.back", json!({})).unwrap();

    ide.put_caret("cart.go", chars(CART, "Walk(n int)") + 1);
    ide.keys(&["⌃⌥H"]);
    let state =
        ide.settled(|state| state["symbol"] == "Walk" && state["nodes"].as_array().is_some_and(|n| n.len() >= 3));
    assert_eq!(state["tabs"].as_array().unwrap().len(), 1, "the unpinned tab is replaced: {state}");
    ide.run("navigate.hierarchy.expandAll", json!({ "waitMs": 5000 })).unwrap();
    let state = ide.settled(|_| true);
    let recursive: Vec<&Value> =
        state["nodes"].as_array().unwrap().iter().filter(|node| node["recursive"] == true).collect();
    assert_eq!(recursive.len(), 1, "Walk calls itself once, and that is not expanded: {state}");
    assert_eq!(recursive[0]["name"], "Walk");
    assert!(names(&state).len() <= 4, "the recursion ends: {state}");
    ide.snapshot("recursion");

    let pinned = ide.run("navigate.hierarchy.pin", json!({})).unwrap();
    assert_eq!(pinned["pinned"], true);
    ide.run("file.document.open", json!({ "path": ide.project.join("types.go") })).unwrap();
    ide.put_caret("types.go", chars(TYPES, "Store interface") + 1);
    ide.keys(&["⌃H"]);
    let state = ide.settled(|state| state["kind"] == "type" && state["nodes"].as_array().is_some_and(|n| n.len() >= 3));
    assert_eq!(state["direction"], "subtypes", "{state}");
    let mut subtypes = names(&state);
    subtypes.sort();
    assert_eq!(subtypes, ["Disk", "Memory", "Store"], "{state}");
    let tabs = state["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 2, "the pinned tab stays: {state}");
    assert_eq!((tabs[0]["symbol"].clone(), tabs[0]["pinned"].clone()), (json!("Walk"), json!(true)));
    ide.snapshot("pinned-tabs");

    let walk_tab = tabs[0]["tab"].as_u64().unwrap() as usize;
    let title = ide.view(|view, _| view.hierarchy_window.tab_rect(walk_tab)).unwrap();
    ide.click(title.center(), 1);
    let state = ide.settled(|_| true);
    assert_eq!(
        (state["active"].clone(), state["symbol"].clone()),
        (json!(walk_tab), json!("Walk")),
        "a title switches tabs"
    );
    let memory_state = ide.run("navigate.hierarchy.get", json!({ "tab": tabs[1]["tab"] })).unwrap();
    let store = node(&memory_state, "Store")["id"].clone();
    let expanded = ide.run("navigate.hierarchy.expand", json!({ "node": store, "waitMs": 3000 })).unwrap();
    assert_eq!(expanded["tab"], tabs[1]["tab"], "a node of another tab expands in its tab");

    ide.run(
        "navigate.hierarchy.setOptions",
        json!({ "tab": tabs[1]["tab"], "direction": "supertypes", "waitMs": 5000 }),
    )
    .unwrap();
    ide.run("navigate.hierarchy.select", json!({ "tab": tabs[1]["tab"] })).unwrap();
    ide.click(egui::pos2(520.0, 420.0), 1);
    ide.put_caret("types.go", chars(TYPES, "Memory struct") + 1);
    ide.keys(&["⌃H"]);
    let state =
        ide.settled(|state| state["symbol"] == "Memory" && state["nodes"].as_array().is_some_and(|n| n.len() >= 2));
    assert_eq!(
        (state["direction"].clone(), names(&state)),
        (json!("supertypes"), vec!["Memory".to_owned(), "Store".to_owned()]),
        "{state}"
    );
    ide.run("navigate.hierarchy.setOptions", json!({ "scope": "test" })).unwrap();
    ide.snapshot("supertypes");

    ide.run("navigate.hierarchy.close", json!({})).unwrap();
    ide.run("navigate.hierarchy.close", json!({})).unwrap();
    let state = ide.settled(|_| true);
    assert_eq!(state["open"], false, "{state}");
    ide.snapshot("empty");
}

fn find_program(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|path| path.is_file())
}

#[test]
#[ignore = "needs gopls"]
fn the_hierarchy_tool_window_with_gopls() {
    let gopls = find_program("gopls").expect("gopls on PATH");
    let ide = Ide::new("gopls", move |_| ServerSpec {
        language_id: "go".to_owned(),
        extensions: vec!["go".to_owned()],
        launch: Arc::new(move |folder| {
            Ok(zen_lsp::process_launcher(gopls.clone(), Vec::new(), folder.to_owned(), Vec::new()))
        }),
        initialization_options: json!({}),
    });
    walk_the_hierarchy_window(ide);
}

fn utf16(text: &str, at: usize) -> Value {
    let before = &text[..at];
    let line = before.matches('\n').count();
    let column = before[before.rfind('\n').map_or(0, |at| at + 1)..].encode_utf16().count();
    json!({ "line": line, "character": column })
}

fn item(project: &Path, file: &str, text: &str, name: &str, kind: u64, declaration: &str) -> Value {
    let at = text.find(declaration).unwrap();
    let line = text[..at].rfind('\n').map_or(0, |at| at + 1);
    let end = text[at..].find('\n').map_or(text.len(), |end| at + end);
    json!({
        "name": name,
        "kind": kind,
        "detail": format!("example.com/shop • {file}"),
        "uri": zen_lsp::position::file_uri(&project.join(file)),
        "range": { "start": utf16(text, line), "end": utf16(text, end) },
        "selectionRange": { "start": utf16(text, at), "end": utf16(text, at + name.len()) },
    })
}

fn fake_gopls(project: &Path) -> zen_lsp::fake::FakeServer {
    let server = zen_lsp::fake::FakeServer::new();
    server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "callHierarchyProvider": true,
        "typeHierarchyProvider": true
    }));
    let total = item(project, "cart.go", CART, "Total", 12, "Total(prices");
    let walk = item(project, "cart.go", CART, "Walk", 12, "Walk(n int)");
    let use_walk = item(project, "cart.go", CART, "Use", 12, "Use()");
    let run = item(project, "run.go", RUN, "Run", 12, "Run()");
    let checkout = item(project, "run.go", RUN, "Checkout", 12, "Checkout()");
    let test = item(project, "cart_test.go", TEST, "TestTotal", 12, "TestTotal");
    let store = item(project, "types.go", TYPES, "Store", 11, "Store interface");
    let memory = item(project, "types.go", TYPES, "Memory", 23, "Memory struct");
    let disk = item(project, "types.go", TYPES, "Disk", 23, "Disk struct");
    let (on_total, on_walk) = (total.clone(), walk.clone());
    server.on_with("textDocument/prepareCallHierarchy", move |params| {
        if params["position"]["line"] == 3 { json!([on_total]) } else { json!([on_walk]) }
    });
    let calls = |from: &Value| json!({ "from": from, "fromRanges": [] });
    let (total_callers, walk_callers) =
        (json!([calls(&run), calls(&checkout), calls(&test)]), json!([calls(&use_walk), calls(&walk)]));
    server.on_with("callHierarchy/incomingCalls", move |params| match params["item"]["name"].as_str() {
        Some("Total") => total_callers.clone(),
        Some("Walk") => walk_callers.clone(),
        _ => json!([]),
    });
    let (on_store, on_memory) = (store.clone(), memory.clone());
    server.on_with("textDocument/prepareTypeHierarchy", move |params| {
        if params["position"]["line"] == 3 { json!([on_store]) } else { json!([on_memory]) }
    });
    let subtypes = json!([memory, disk]);
    server.on_with("typeHierarchy/subtypes", move |params| {
        if params["item"]["name"] == "Store" { subtypes.clone() } else { json!([]) }
    });
    let supertypes = json!([store]);
    server.on_with("typeHierarchy/supertypes", move |params| {
        if params["item"]["name"] == "Store" { json!([]) } else { supertypes.clone() }
    });
    server
}

#[test]
fn the_hierarchy_tool_window_with_a_scripted_server() {
    let ide = Ide::new("scripted", |project| {
        let server = fake_gopls(project);
        ServerSpec {
            language_id: "go".to_owned(),
            extensions: vec!["go".to_owned()],
            launch: Arc::new(move |_| Ok(server.launcher())),
            initialization_options: json!({}),
        }
    });
    walk_the_hierarchy_window(ide);
}
