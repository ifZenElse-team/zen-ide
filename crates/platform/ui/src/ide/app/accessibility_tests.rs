use egui::accesskit::{Node, Role, TreeUpdate};
use serde_json::json;
use zen_i18n::t;

use super::*;
use crate::accessibility;
use crate::accessibility::testing::find;
use crate::ide::settings::PageId;

type Open = fn(&mut IdeView);

struct Window {
    ctx: egui::Context,
    app: IdeApp,
    time: f64,
    project: PathBuf,
    _data: tempfile::TempDir,
}

impl Window {
    fn new(open: bool) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("main.go"), "package main\n\nfunc main() {\n\tprintln(\"hi\")\n}\n").unwrap();
        std::fs::write(project.join("src").join("util.go"), "package src\n").unwrap();
        std::fs::write(project.join("README.md"), "# Synthetic\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct {
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
            folding_options: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        };
        let projects = if open { vec![project.clone()] } else { Vec::new() };
        let app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), projects);
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.enable_accesskit();
        let window = Self { ctx, app, time: 0.0, project, _data: data };
        if open {
            let main = window.project.join("main.go");
            window.with_view(|_, registry, session| {
                registry.dispatch(session, "file.document.open", json!({ "path": main })).unwrap();
            });
        }
        window
    }

    fn with_view(&self, change: impl FnOnce(&mut IdeView, &Registry, &mut Session)) {
        let windows = self.app.shared.state().windows.clone();
        let mut window = windows[0].window.lock().unwrap();
        let ProjectWindow { session, view } = &mut *window;
        change(view, &self.app.shared.registry, session);
    }

    fn frame(&mut self) -> TreeUpdate {
        self.frame_with(Vec::new())
    }

    fn frame_with(&mut self, events: Vec<egui::Event>) -> TreeUpdate {
        self.time += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0))),
            time: Some(self.time),
            events,
            ..egui::RawInput::default()
        };
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
        output.platform_output.accesskit_update.expect("the tree is built while AccessKit is on")
    }

    fn frame_any(&mut self) {
        self.time += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0))),
            time: Some(self.time),
            ..egui::RawInput::default()
        };
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
    }

    fn settle(&mut self) -> TreeUpdate {
        for _ in 0..4 {
            self.frame();
        }
        self.frame()
    }
}

fn unnamed_in(state: &str, tree: &TreeUpdate, missing: &mut Vec<String>) {
    missing.extend(accessibility::unnamed(tree).into_iter().map(|node| format!("{state}: {node}")));
}

#[test]
fn every_control_a_screen_reader_reaches_has_a_role_and_a_name() {
    let mut window = Window::new(true);
    let mut missing = Vec::new();
    for declared in crate::ide::tool_windows::DECLARED {
        let id = declared.id;
        window.with_view(|view, _, _| view.show_tool(id));
        if id == "project" {
            let project = window.project.clone();
            window.with_view(|view, _, _| {
                view.expanded.insert(project.join("src"));
            });
        }
        unnamed_in(id, &window.settle(), &mut missing);
        window.with_view(|view, _, _| view.hide_tool(id));
    }
    window.with_view(|view, _, _| view.show_tool("project"));
    let dialogs: [(&str, Open); 9] = [
        ("search everywhere", |view| view.search_everywhere.toggle(super::super::search_everywhere::Tab::ALL)),
        ("recent files", |view| view.recent_files.press()),
        ("switcher", |view| view.recent_files.switch(false, None)),
        ("recent locations", |view| view.recent_locations.press()),
        ("file structure", |view| view.file_structure.open(Path::new("/p/main.go"), serde_json::json!({}))),
        ("paste history", |view| view.paste_history.open()),
        ("find in files", |view| view.find_in_files.open(false, None, None)),
        ("go to line", IdeView::open_goto),
        ("run configurations", |view| view.run_configurations.open()),
    ];
    for (name, open) in dialogs {
        let mut window = Window::new(true);
        let _ = window.settle();
        window.with_view(|view, _, _| open(view));
        unnamed_in(name, &window.settle(), &mut missing);
    }
    for page in PageId::ALL {
        window.with_view(|view, _, _| view.settings.open_at(*page));
        unnamed_in(&format!("settings {page:?}"), &window.settle(), &mut missing);
    }
    assert!(missing.is_empty(), "controls without a role or a name:\n{}", missing.join("\n"));
}

#[test]
fn the_welcome_screen_names_its_controls() {
    let mut window = Window::new(false);
    let mut overview = window.app.shared.overview();
    overview.recent = vec![RecentProject { path: window.project.clone(), opened: 1, open: false }];
    let welcome = &mut window.app.welcome;
    let mut tree = None;
    for _ in 0..3 {
        let mut output = window.ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = welcome.show(ui, &overview, &mut |_, _| Ok(serde_json::Value::Null));
        });
        output.textures_delta.clear();
        tree = output.platform_output.accesskit_update;
    }
    let tree = tree.expect("the tree is built while AccessKit is on");
    assert_eq!(accessibility::unnamed(&tree), Vec::<String>::new());
    assert!(find(&tree, Role::ListBoxOption, "project").is_some(), "a recent project");
}

#[test]
fn painted_text_that_matters_is_read() {
    let mut window = Window::new(true);
    window.with_view(|view, _, _| view.show_tool("problems"));
    let ctx = window.ctx.clone();
    window.with_view(|view, _, _| view.post_at(&ctx, "Synthetic notice".to_owned()));
    let tree = window.settle();
    assert!(find(&tree, Role::Heading, &t!("ide-project-title")).is_some(), "the Project tool window's title");
    assert!(find(&tree, Role::Heading, &t!("ide-problems-title")).is_some(), "the Problems tool window's title");
    assert!(find(&tree, Role::Label, "1:1").is_none(), "the caret position is a button, not a label");
    assert!(find(&tree, Role::Button, "1:1").is_some(), "the caret position opens Go to Line");
    let encoding = t!("ide-status-encoding-named", charset = "UTF-8");
    assert!(find(&tree, Role::Button, &encoding).is_some(), "the status bar's encoding opens its list");
    let notice = find(&tree, Role::Status, "Synthetic notice").expect("the notification is read");
    assert_eq!(notice.live(), Some(egui::accesskit::Live::Polite));
    let tab = find(&tree, Role::Tab, "main.go").expect("the editor tab");
    assert_eq!(tab.is_selected(), Some(true));
    let row = find(&tree, Role::TreeItem, "src").expect("the project tree's folder");
    assert_eq!((row.level(), row.is_expanded()), (Some(2), Some(false)));
    let stripe = find(&tree, Role::Button, &t!("ide-tool-problems")).expect("the Problems stripe button");
    assert_eq!(stripe.toggled(), Some(egui::accesskit::Toggled::True));
}

#[test]
fn the_editor_is_a_multi_line_text_field_with_its_text_and_caret() {
    let mut window = Window::new(true);
    window.with_view(|_, registry, session| {
        let selection = json!({ "selections": [{ "anchor": 15, "head": 19 }] });
        registry.dispatch(session, "editor.selection.set", selection).unwrap();
    });
    let tree = window.settle();
    let (editor_id, editor) =
        tree.nodes.iter().find(|(_, node)| node.role() == Role::MultilineTextInput).expect("the editor's node");
    assert_eq!(editor.label(), Some(t!("a11y-editor", file = "main.go").as_str()));
    assert_eq!(tree.focus, *editor_id, "the editor has the keyboard");
    let runs: Vec<(&egui::accesskit::NodeId, &Node)> =
        tree.nodes.iter().filter(|(_, node)| node.role() == Role::TextRun).map(|(id, node)| (id, node)).collect();
    let node = |id: &egui::accesskit::NodeId| tree.nodes.iter().find(|(other, _)| other == id).map(|(_, node)| node);
    let lines = tree.nodes.iter().find(|(_, node)| node.children().contains(runs[0].0)).map(|(_, node)| node).unwrap();
    let text: String = lines.children().iter().filter_map(|id| node(id).and_then(Node::value)).collect();
    assert_eq!(text, "package main\n\nfunc main() {\n\tprintln(\"hi\")\n}\n", "the lines in their order");
    let selection = editor.text_selection().expect("the caret and the selection");
    let line = |id| runs.iter().find(|(run, _)| **run == id).and_then(|(_, node)| node.value()).unwrap();
    assert_eq!((line(selection.anchor.node), selection.anchor.character_index), ("func main() {\n", 1));
    assert_eq!((line(selection.focus.node), selection.focus.character_index), ("func main() {\n", 5));
    let parent = |child: egui::accesskit::NodeId| {
        tree.nodes.iter().find(|(_, node)| node.children().contains(&child)).map(|(id, _)| *id)
    };
    let mut ancestor = parent(*runs[0].0);
    while let Some(id) = ancestor.filter(|id| id != editor_id) {
        ancestor = parent(id);
    }
    assert_eq!(ancestor, Some(*editor_id), "the lines are inside the editor's node");
}

#[test]
fn each_split_editor_is_named_with_its_group() {
    let mut window = Window::new(true);
    let _ = window.settle();
    window.with_view(|_, registry, session| {
        registry.dispatch(session, "editor.split.right", json!({})).unwrap();
    });
    let tree = window.settle();
    let mut editors: Vec<&str> = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::MultilineTextInput)
        .filter_map(|(_, node)| node.label())
        .collect();
    editors.sort_unstable();
    let named = |split: usize| t!("a11y-editor-split", file = "main.go", split = split, count = 2);
    assert_eq!(editors, [named(1), named(2)], "both editors are read with their group");
    assert!(find(&tree, Role::Splitter, &t!("a11y-editor-divider")).is_some(), "the divider is named");
    assert_eq!(accessibility::unnamed(&tree), Vec::<String>::new());
}

#[test]
fn without_a_screen_reader_the_window_builds_no_tree() {
    let mut window = Window::new(true);
    window.ctx.disable_accesskit();
    for _ in 0..3 {
        window.time += 1.0 / 60.0;
        let app = &mut window.app;
        let input = egui::RawInput { time: Some(window.time), ..egui::RawInput::default() };
        let mut output = window.ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
        assert!(output.platform_output.accesskit_update.is_none());
    }
    window.with_view(|view, _, _| assert!(view.editor_view.access_is_empty(), "the editor keeps no text runs"));
}

#[test]
#[ignore = "a measurement: cargo test --release -p zen-ui --lib frame_cost -- --ignored --nocapture"]
#[allow(clippy::print_stdout, reason = "a measurement, read on the terminal")]
fn frame_cost_with_and_without_a_screen_reader() {
    let window = &mut Window::new(true);
    let big = window.project.join("big.go");
    let text: String = (0..10_000).map(|line| format!("\tvalue{line} := compute({line}) // line {line}\n")).collect();
    std::fs::write(&big, format!("package main\n\nfunc main() {{\n{text}}}\n")).unwrap();
    window.with_view(|view, registry, session| {
        view.show_tool("problems");
        registry.dispatch(session, "file.document.open", json!({ "path": big })).unwrap();
    });
    let measure = |window: &mut Window, on: bool| {
        if on {
            window.ctx.enable_accesskit();
        } else {
            window.ctx.disable_accesskit();
        }
        for _ in 0..20 {
            window.frame_any();
        }
        let started = std::time::Instant::now();
        for _ in 0..300 {
            window.frame_any();
        }
        started.elapsed().as_secs_f64() * 1000.0 / 300.0
    };
    let off = measure(window, false);
    let on = measure(window, true);
    let off_again = measure(window, false);
    window.ctx.enable_accesskit();
    let tree = window.frame();
    let runs = tree.nodes.iter().filter(|(_, node)| node.role() == Role::TextRun).count();
    println!(
        "frame: {off:.3} ms without a screen reader, {on:.3} ms with one ({} nodes, {runs} text runs), {off_again:.3} ms without again",
        tree.nodes.len()
    );
}

fn center_of(tree: &TreeUpdate, role: Role, name: &str) -> egui::Pos2 {
    let bounds = find(tree, role, name).and_then(Node::bounds).unwrap_or_else(|| panic!("{name} is drawn"));
    egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32)
}

fn click(window: &mut Window, at: egui::Pos2, button: egui::PointerButton) -> TreeUpdate {
    let press = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
    window.frame_with(vec![egui::Event::PointerMoved(at)]);
    window.frame_with(vec![press(true)]);
    window.frame_with(vec![press(false)]);
    window.settle()
}

fn hover(window: &mut Window, at: egui::Pos2) -> TreeUpdate {
    window.frame_with(vec![egui::Event::PointerMoved(at)]);
    window.settle()
}

fn named(tree: &TreeUpdate, name: &str) -> usize {
    tree.nodes
        .iter()
        .filter(|(_, node)| {
            node.role() == Role::Button && accessibility::accessible_name(node, &tree.nodes).as_deref() == Some(name)
        })
        .count()
}

#[test]
fn the_editor_tab_menu_is_one_menu_with_every_item() {
    let mut window = Window::new(true);
    window.app.splash = super::super::splash::Splash::finished();
    let tree = window.settle();
    let tab = center_of(&tree, Role::Tab, "main.go");
    let menu = click(&mut window, tab, egui::PointerButton::Secondary);
    let items = [
        t!("keymap-file-document-close"),
        t!("keymap-editor-split-right"),
        t!("keymap-editor-split-down"),
        t!("keymap-editor-split-right-move"),
        t!("keymap-editor-split-down-move"),
        t!("ide-tree-open-in"),
        t!("menu-file-local-history"),
    ];
    for item in &items {
        assert_eq!(named(&menu, item), 1, "{item} is in the menu once");
    }
    assert_eq!(named(&menu, &t!("keymap-editor-split-unsplit")), 0, "Unsplit only when split");

    let open_in = hover(&mut window, center_of(&menu, Role::Button, &t!("ide-tree-open-in")));
    for item in [super::super::file_manager_label(), t!("ide-tree-open-in-terminal")] {
        assert_eq!(named(&open_in, &item), named(&menu, &item) + 1, "Open In → {item}");
    }

    let history = click(
        &mut window,
        center_of(&menu, Role::Button, &t!("menu-file-local-history")),
        egui::PointerButton::Primary,
    );
    assert_eq!(named(&history, &t!("keymap-view-localHistory-putLabel")), 1);
    let show = center_of(&history, Role::Button, &t!("keymap-view-localHistory-show"));
    click(&mut window, show, egui::PointerButton::Primary);
    window.with_view(|view, _, _| assert!(view.local_history.is_history_open(), "Show History opens it"));

    window.with_view(|view, registry, session| {
        view.local_history = Default::default();
        registry.dispatch(session, "editor.split.right", json!({})).unwrap();
    });
    let tree = window.settle();
    let tab = center_of(&tree, Role::Tab, "main.go");
    let menu = click(&mut window, tab, egui::PointerButton::Secondary);
    assert_eq!(named(&menu, &t!("keymap-editor-split-unsplit")), 1, "Unsplit when split");
    assert_eq!(named(&menu, &t!("keymap-editor-split-unsplitAll")), 1, "Unsplit All when split");
    assert_eq!(named(&menu, &t!("keymap-file-document-close")), 1);
}
