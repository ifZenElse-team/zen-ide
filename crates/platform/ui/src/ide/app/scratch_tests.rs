use std::sync::mpsc;

use egui::accesskit::{Role, TreeUpdate};
use serde_json::{Value, json};

use super::*;
use crate::accessibility::testing::find;

struct Harness {
    ctx: egui::Context,
    app: IdeApp,
    time: f64,
    project: PathBuf,
    data: tempfile::TempDir,
    events: Vec<egui::Event>,
}

impl Harness {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("main.go"), "package main\n\nfunc main() {\n\tprintln(1)\n}\n").unwrap();
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
        let mut harness = Self { ctx, app, time: 0.0, project, data, events: Vec::new() };
        harness.frame();
        harness
    }

    fn scratches(&self) -> PathBuf {
        self.data.path().join("scratches")
    }

    fn buffers(&self) -> PathBuf {
        self.data.path().join("buffers")
    }

    fn input(&mut self) -> egui::RawInput {
        self.time += 1.0 / 60.0;
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0))),
            time: Some(self.time),
            events: std::mem::take(&mut self.events),
            ..egui::RawInput::default()
        }
    }

    fn frame(&mut self) -> TreeUpdate {
        let input = self.input();
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

    fn action(&mut self, action: &str, params: Value) -> Value {
        self.ask("run_action", json!({ "action": action, "params": params })).unwrap()
    }

    fn command(&mut self, command: &str, params: Value) -> Value {
        self.ask("dispatch_command", json!({ "command": command, "params": params })).unwrap()
    }

    fn keys(&mut self, keys: Value) -> Value {
        let answer = self.ask("press_keys", keys).unwrap();
        self.frame();
        answer
    }

    fn popups(&mut self) -> Value {
        self.frame();
        self.ask("get_window_state", json!({})).unwrap()["popups"].clone()
    }

    fn with_view(&self, change: impl FnOnce(&mut IdeView, &Registry, &mut Session)) {
        let windows = self.app.shared.state().windows.clone();
        let mut window = windows[0].window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *window;
        change(view, &self.app.shared.registry, session);
    }

    fn active(&self) -> Option<PathBuf> {
        let mut active = None;
        self.with_view(|_, _, session| {
            active = session
                .state::<Workbench>()
                .and_then(Workbench::active_document)
                .map(|document| document.path().to_owned());
        });
        active
    }

    fn click(&mut self, name: &str) {
        let tree = self.settle();
        let node = find(&tree, Role::MenuItem, name)
            .or_else(|| find(&tree, Role::Button, name))
            .unwrap_or_else(|| panic!("nothing named {name:?}"));
        let bounds = node.bounds().expect("bounds");
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            self.events = events;
            self.frame();
        }
        self.settle();
    }

    fn expand_scratches(&self) {
        let project = self.project.clone();
        let (scratches, buffers) = (self.scratches(), self.buffers());
        self.with_view(|view, _, _| {
            view.show_tool("project");
            view.expanded.insert(super::super::libraries_tree::group_path(&project, "scratches"));
            view.expanded.insert(scratches);
            view.expanded.insert(buffers);
        });
    }
}

fn names(rows: &Value) -> Vec<String> {
    rows.as_array().unwrap().iter().map(|row| row["name"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn file_new_scratch_file_lists_the_languages_recent_first_and_creates_numbered_files() {
    let mut h = Harness::new();
    let opened = h.action("view.scratch.new", json!({}));
    assert_eq!(opened["window"]["popups"], json!(["scratchLanguages"]), "{opened}");
    assert_eq!(names(&opened["result"]), ["Go", "Plain Text", "Rust"], "{opened}");
    h.keys(json!({ "text": "ru" }));
    h.keys(json!({ "keys": ["⏎"] }));
    assert_eq!(h.popups(), json!([]));
    let rust = h.scratches().join("scratch.rs");
    assert!(rust.is_file());
    assert_eq!(h.active(), Some(rust));

    let again = h.action("view.scratch.new", json!({}));
    assert_eq!(names(&again["result"]), ["Rust", "Go", "Plain Text"], "the recent language first: {again}");
    h.keys(json!({ "keys": ["⏎"] }));
    assert!(h.scratches().join("scratch_1.rs").is_file(), "numbered");
    h.action("view.scratch.new", json!({}));
    h.keys(json!({ "keys": ["⎋"] }));
    assert_eq!(h.popups(), json!([]), "⎋ closes the list");

    let found = h.ask("find_action", json!({ "query": "scratch", "limit": 10 })).unwrap();
    let entry =
        |action: &str| found["actions"].as_array().unwrap().iter().find(|entry| entry["action"] == action).cloned();
    assert_eq!(entry("view.scratch.new").unwrap_or_else(|| panic!("{found}"))["shortcut"], "⇧⌘N");
    assert_eq!(entry("view.scratch.newBuffer").unwrap_or_else(|| panic!("{found}"))["name"], "New Scratch Buffer");
    assert_eq!(entry("view.scratch.show").unwrap_or_else(|| panic!("{found}"))["name"], "Show Scratch Files");
    assert!(entry("file.scratch.newBuffer").is_none(), "listed once: {found}");

    let shown = h.action("view.scratch.show", json!({}));
    assert_eq!(shown["window"]["popups"], json!(["scratchFiles"]), "{shown}");
    assert_eq!(names(&shown["result"]), ["scratch.rs", "scratch_1.rs"], "{shown}");
    h.keys(json!({ "keys": ["↓", "⏎"] }));
    assert_eq!(h.active(), Some(h.scratches().join("scratch_1.rs")));
}

#[test]
fn a_reused_buffer_with_text_asks_before_it_is_cleared() {
    let mut h = Harness::new();
    for slot in 1..=5 {
        let opened = h.action("view.scratch.newBuffer", json!({}));
        assert_eq!(opened["result"]["path"], json!(h.buffers().join(format!("buffer{slot}.txt"))), "{opened}");
        if slot == 1 {
            h.command("editor.edit.type", json!({ "text": "old notes" }));
            h.command("file.document.saveAll", json!({}));
        }
    }
    let reused = h.action("view.scratch.newBuffer", json!({}));
    assert_eq!(reused["result"]["needsAnswer"], true, "{reused}");
    assert_eq!(h.popups(), json!(["clearBuffer"]));
    h.keys(json!({ "keys": ["⏎"] }));
    assert_eq!(h.popups(), json!([]));
    assert_eq!(std::fs::read_to_string(h.buffers().join("buffer1.txt")).unwrap(), "");
    assert_eq!(h.active(), Some(h.buffers().join("buffer1.txt")));
}

#[test]
fn the_project_tree_shows_scratches_and_consoles_while_the_advanced_setting_is_on() {
    let mut h = Harness::new();
    h.command("file.scratch.new", json!({ "language": "go", "text": "package main\n" }));
    h.command("file.scratch.newBuffer", json!({}));
    h.expand_scratches();
    let tree = h.settle();
    let node = zen_i18n::t!("ide-scratch-node");
    assert!(find(&tree, Role::TreeItem, &node).is_some(), "Scratches and Consoles is shown");
    assert!(find(&tree, Role::TreeItem, "Scratches").is_some());
    assert!(find(&tree, Role::TreeItem, "Buffers").is_some());
    assert!(find(&tree, Role::TreeItem, "scratch.go").is_some());
    assert!(find(&tree, Role::TreeItem, "buffer1.txt").is_some());

    let menu = h.action("view.project.contextMenu", json!({ "path": h.scratches().join("scratch.go") }));
    assert!(menu["result"].is_null(), "{menu}");
    let tree = h.settle();
    for item in ["Change Language…", "Rename…", "Delete…", "Move…"] {
        let found = find(&tree, Role::MenuItem, item).or_else(|| find(&tree, Role::Button, item));
        assert!(found.is_some(), "the menu has {item}");
    }
    for absent in ["Cut", "Paste", "Reformat Code"] {
        let found = find(&tree, Role::MenuItem, absent).or_else(|| find(&tree, Role::Button, absent));
        assert!(found.is_none(), "the menu has no {absent}");
    }
    h.keys(json!({ "keys": ["⎋"] }));

    h.ask("set_settings", json!({ "projectViewScratches": false })).unwrap();
    let tree = h.settle();
    assert!(find(&tree, Role::TreeItem, &node).is_none(), "the setting hides it");
    let settings = h.ask("get_settings", json!({})).unwrap();
    assert_eq!(settings["projectViewScratches"], false, "{settings}");
}

#[test]
fn a_scratch_that_runs_alone_has_run_on_its_context_menu_and_opens_the_run_window() {
    let mut h = Harness::new();
    h.command("file.scratch.new", json!({ "language": "go", "text": "package main\n" }));
    h.command("file.scratch.new", json!({ "language": "txt", "text": "notes\n" }));
    let fake = zen_run::fake::FakeProvider::new();
    fake.set_single_file(vec![zen_run::RunTarget {
        line: 0,
        kind: zen_run::TargetKind::Main,
        name: "main".to_owned(),
        package: String::new(),
        configuration: zen_run::fake::FakeProvider::configuration("scratch.go"),
    }]);
    fake.set_script(&[(zen_run::Stream::Stdout, "ran")], 0);
    let provider = fake.clone();
    h.with_view(move |_, _, session| {
        let runs = session.state_mut::<zen_workbench::run::Runs>();
        runs.set_providers(vec![Arc::new(provider)], zen_run::RunEnv::default());
    });
    h.expand_scratches();
    h.settle();
    let run = "Run 'scratch.go'";
    h.action("view.project.contextMenu", json!({ "path": h.scratches().join("scratch.go") }));
    let tree = h.settle();
    assert!(
        find(&tree, Role::MenuItem, run).or_else(|| find(&tree, Role::Button, run)).is_some(),
        "the menu has {run}"
    );
    h.keys(json!({ "keys": ["⎋"] }));

    fake.set_single_file(Vec::new());
    h.action("view.project.contextMenu", json!({ "path": h.scratches().join("scratch.txt") }));
    let tree = h.settle();
    let any_run = find(&tree, Role::MenuItem, run).or_else(|| find(&tree, Role::Button, run));
    assert!(any_run.is_none(), "a scratch that cannot run has no Run");
    h.keys(json!({ "keys": ["⎋"] }));

    fake.set_single_file(vec![zen_run::RunTarget {
        line: 0,
        kind: zen_run::TargetKind::Main,
        name: "main".to_owned(),
        package: String::new(),
        configuration: zen_run::fake::FakeProvider::configuration("scratch.go"),
    }]);
    h.action("view.project.contextMenu", json!({ "path": h.scratches().join("scratch.go") }));
    h.click(run);
    let mut shown = (false, 0);
    h.with_view(|view, _, session| {
        let runs = session.state::<zen_workbench::run::Runs>().map(|runs| runs.temporary().len()).unwrap_or_default();
        shown = (view.tool_visible("run"), runs);
    });
    assert_eq!(shown, (true, 1), "the run starts as a temporary configuration and the Run tool window opens");
}

#[test]
fn move_and_change_language_take_a_scratch_file() {
    let mut h = Harness::new();
    h.command("file.scratch.new", json!({ "language": "go" }));
    let scratch = h.scratches().join("scratch.go");
    let refused =
        h.ask("run_action", json!({ "action": "view.project.move", "params": { "path": h.project.join("main.go") } }));
    assert!(refused.is_err(), "{refused:?}");

    let changed = h.action("view.project.changeLanguage", json!({ "path": scratch }));
    assert_eq!(changed["window"]["popups"], json!(["scratchLanguages"]), "{changed}");
    h.keys(json!({ "text": "plain" }));
    h.keys(json!({ "keys": ["⏎"] }));
    let text = h.scratches().join("scratch.txt");
    assert!(text.is_file() && !scratch.exists());
    assert_eq!(h.active(), Some(text.clone()), "the tab follows");

    let moving = h.action("view.project.move", json!({ "path": text }));
    assert_eq!(moving["window"]["popups"], json!(["moveScratch"]), "{moving}");
    h.keys(json!({ "keys": ["⏎"] }));
    assert_eq!(h.popups(), json!([]));
    let moved = h.project.join("scratch.txt");
    assert!(moved.is_file() && !text.exists());
    assert_eq!(h.active(), Some(moved));
}

#[test]
fn alt_enter_on_a_selection_offers_a_scratch_file_from_it() {
    let mut h = Harness::new();
    h.command("file.document.open", json!({ "path": h.project.join("main.go") }));
    h.command("editor.selection.set", json!({ "selections": [{ "anchor": 14, "head": 41 }] }));
    let opened = h.action("view.intentions.open", json!({}));
    assert!(opened.get("error").is_none(), "{opened}");
    assert_eq!(h.popups(), json!(["intentions"]));
    let tree = h.settle();
    assert!(find(&tree, Role::ListBoxOption, "Create new scratch file from selection").is_some());
    h.keys(json!({ "keys": ["⏎"] }));
    let scratch = h.scratches().join("scratch.go");
    assert_eq!(std::fs::read_to_string(&scratch).unwrap(), "func main() {\n\tprintln(1)\n}");
    assert_eq!(h.active(), Some(scratch));
}

#[test]
fn closing_an_empty_scratch_tab_deletes_the_file() {
    let mut h = Harness::new();
    h.action("view.scratch.new", json!({}));
    h.keys(json!({ "keys": ["⏎"] }));
    let scratch = h.scratches().join("scratch.go");
    assert!(scratch.is_file());
    h.action("file.document.close", json!({}));
    assert!(!scratch.exists());
}

#[test]
fn an_empty_scratch_open_in_another_window_is_kept_until_its_last_tab_closes() {
    let mut h = Harness::new();
    let second = h.data.path().join("second");
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(second.join("main.go"), "package main\n").unwrap();
    h.ask("open_project", json!({ "path": second })).unwrap();
    let second = std::fs::canonicalize(&second).unwrap();
    let first = h.project.clone();
    let run = |h: &mut Harness, project: &Path, command: &str, params: Value| {
        h.ask("dispatch_command", json!({ "command": command, "params": params, "project": project })).unwrap()
    };
    let scratch =
        PathBuf::from(run(&mut h, &first, "file.scratch.new", json!({ "language": "txt" })).as_str().unwrap());
    run(&mut h, &second, "file.document.open", json!({ "path": scratch }));
    run(&mut h, &first, "file.document.close", json!({}));
    assert!(scratch.exists(), "the other window still shows it");
    run(&mut h, &second, "file.document.close", json!({}));
    assert!(!scratch.exists(), "closing its last tab deletes it");
}

#[test]
fn scratch_snapshots_in_english_and_russian() {
    let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
    let dir = PathBuf::from(dir);
    let mut h = Harness::new();
    h.command("file.scratch.new", json!({ "language": "go", "text": "package main\n\nfunc main() {}\n" }));
    h.command("file.scratch.new", json!({ "language": "txt", "text": "notes\n" }));
    h.command("file.scratch.newBuffer", json!({}));
    h.command("editor.edit.type", json!({ "text": "buffer text" }));
    h.command("file.document.saveAll", json!({}));
    h.expand_scratches();
    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1280.0, 820.0), 2.0);
    let mut shoot = |h: &mut Harness, name: &str, language: &str| {
        let app = &mut h.app;
        for _ in 0..3 {
            screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        }
        let image = screen.render(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        crate::offscreen::write_png(&dir.join(format!("scratch-{name}-{language}.png")), &image).unwrap();
    };
    for language in ["en", "ru"] {
        zen_i18n::set_language(Some(language));
        h.expand_scratches();
        shoot(&mut h, "tree", language);
        h.with_view(|view, registry, session| {
            view.scratch.open_languages(registry, session, super::super::scratch_popups::LanguageTarget::New).unwrap();
        });
        shoot(&mut h, "languages", language);
        h.with_view(|view, registry, session| {
            view.scratch.open_files(registry, session).unwrap();
        });
        shoot(&mut h, "files", language);
        let buffer = h.buffers().join("buffer1.txt");
        h.with_view(|view, _, _| view.scratch.ask_clear(buffer));
        shoot(&mut h, "clear-buffer", language);
        let (scratch, project) = (h.scratches().join("scratch.go"), h.project.clone());
        h.with_view(|view, _, _| view.scratch.open_move(scratch, &project));
        shoot(&mut h, "move", language);
        h.with_view(|view, _, _| view.scratch = Default::default());
        let scratch = h.scratches().join("scratch.go");
        h.with_view(|view, _, _| view.menu_request = Some((scratch, true)));
        shoot(&mut h, "context-menu", language);
    }
    zen_i18n::set_language(Some("en"));
}
