use std::path::PathBuf;
use std::sync::Arc;

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_editor::{Selection, Selections};
use zen_workbench::Workbench;
use zen_workbench::workspaces::{InnerTab, WorkspaceInfo, Workspaces};

use super::super::appearance::Mode;
use super::super::{IdePalette, IdeProduct, IdeView};
use super::WorkspaceView;

#[cfg(unix)]
mod backup;
mod database_layout;
mod relations;
mod splits;
mod strip;

#[derive(Default)]
struct TestView {
    navigator: Option<f32>,
}

impl WorkspaceView for TestView {
    fn toolbar_height(&self) -> f32 {
        34.0
    }

    fn toolbar(&mut self, ui: &mut Ui, palette: &IdePalette, _: &Registry, _: &mut Session) {
        let rect = ui.max_rect();
        let font = egui::FontId::proportional(13.0);
        ui.painter().text(
            rect.left_center() + vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "Toolbar",
            font,
            palette.text,
        );
    }

    fn left_panel_width(&self) -> f32 {
        220.0
    }

    fn left_panel(&mut self, ui: &mut Ui, palette: &IdePalette, _: &Registry, _: &mut Session) {
        let rect = ui.max_rect();
        let font = egui::FontId::proportional(13.0);
        ui.painter().text(rect.left_top() + vec2(12.0, 12.0), egui::Align2::LEFT_TOP, "Navigator", font, palette.text);
    }

    fn tab(&mut self, ui: &mut Ui, palette: &IdePalette, _: &Registry, _: &mut Session, tab: &InnerTab) {
        let rect = ui.available_rect_before_wrap();
        let font = egui::FontId::proportional(13.0);
        let text = format!("View of {}", tab.title);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, font, palette.text);
    }

    fn tab_color(&self, tab: &InnerTab, palette: &IdePalette) -> Option<egui::Color32> {
        (tab.kind == "history").then_some(palette.accent)
    }

    fn panel_sizes(&self) -> std::collections::BTreeMap<String, f32> {
        self.navigator.map(|width| ("navigator".to_owned(), width)).into_iter().collect()
    }

    fn restore_panel_sizes(&mut self, sizes: &std::collections::BTreeMap<String, f32>) {
        self.navigator = sizes.get("navigator").copied();
    }
}

struct Connect(Arc<std::sync::atomic::AtomicUsize>);

impl zen_core::Command for Connect {
    type Params = zen_core::NoParams;
    type Output = ();
    const ID: &'static str = "db.connection.connect";
    const TITLE: &'static str = "Connect";
    const DESCRIPTION: &'static str = "Counts the connections asked for.";

    fn run(&self, _: &mut Session, _: zen_core::NoParams) -> Result<(), zen_core::CommandError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

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
        folding_options: Vec::new(),
        debugger: None,
        project_models: Vec::new(),
        outliners: Vec::new(),
        run_providers: Vec::new(),
        language_servers: Vec::new(),
        templates: Vec::new(),
        inspectors: Vec::new(),
    }
}

struct Window {
    ctx: egui::Context,
    registry: Registry,
    session: Session,
    view: IdeView,
    dir: tempfile::TempDir,
    time: f64,
}

fn window() -> Window {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let code: String = (0..200).map(|line| format!("main line {line:03}\n")).collect();
    std::fs::write(project.join("main.txt"), code).unwrap();
    std::fs::write(project.join("other.txt"), "other text\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    let workspaces = session.state_mut::<Workspaces>();
    workspaces.set_data_dir(dir.path().join("data"));
    workspaces.register(WorkspaceInfo { id: "test".into(), title: "Test".into(), icon: "database".into() });
    let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
    view.project_open = false;
    view.register_workspace_view("test", Box::new(TestView::default()));
    let mut window = Window { ctx: egui::Context::default(), registry, session, view, dir, time: 0.0 };
    for name in ["main.txt", "other.txt"] {
        let path = window.project(name);
        window.run("file.document.open", json!({ "path": path }));
    }
    window
}

impl Window {
    fn project(&self, name: &str) -> PathBuf {
        self.dir.path().join("project").join(name)
    }

    fn console(&self, name: &str, text: &str) -> PathBuf {
        let folder = self.dir.path().join("data/consoles/project/local");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn frame_after(&mut self, step: f64, events: Vec<Event>) -> Vec<(Pos2, String)> {
        self.time += step;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
        let mut output = self.ctx.run_ui(input, |ui| view.show(ui, registry, session));
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in output.shapes {
            collect(&clipped.shape, &mut texts);
        }
        texts
    }

    fn frames(&mut self, count: usize) -> Vec<(Pos2, String)> {
        let mut texts = Vec::new();
        for _ in 0..count {
            texts = self.frame_after(1.0, Vec::new());
        }
        texts
    }

    fn workbench(&mut self) -> &mut Workbench {
        self.session.state_mut::<Workbench>()
    }

    fn state(&mut self) -> Value {
        self.run("workspace.host.state", json!({ "id": "test" }))
    }
}

fn click_at(window: &mut Window, pos: Pos2) {
    window.frame_after(0.05, vec![Event::PointerMoved(pos), button(pos, PointerButton::Primary, true)]);
    window.frame_after(0.05, vec![button(pos, PointerButton::Primary, false)]);
    window.frame_after(0.3, Vec::new());
}

fn collect(shape: &Shape, out: &mut Vec<(Pos2, String)>) {
    match shape {
        Shape::Text(text) => out.push((text.pos, text.galley.text().to_owned())),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, out)),
        _ => {}
    }
}

fn shows(texts: &[(Pos2, String)], wanted: &str) -> bool {
    texts.iter().any(|(_, text)| text.contains(wanted))
}

fn at(texts: &[(Pos2, String)], wanted: &str) -> Pos2 {
    texts.iter().find(|(_, text)| text == wanted).map(|(at, _)| *at).unwrap_or_else(|| panic!("no {wanted:?}"))
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
}

#[test]
fn the_host_replaces_the_split_editors_and_gives_them_back_untouched() {
    let mut window = window();
    window.run("editor.split.right", json!({ "index": 0 }));
    window.run("editor.caret.move", json!({ "movement": { "kind": "documentEnd" }, "extend": false }));
    let texts = window.frames(3);
    assert!(shows(&texts, "main line 199"), "the right group is scrolled to the end");
    let caret = window.workbench().active_document().unwrap().selections().primary().head;

    window.run("workspace.host.expand", json!({ "id": "test" }));
    let texts = window.frames(3);
    assert!(shows(&texts, "Test has no open tabs"), "{texts:?}");
    assert!(shows(&texts, "Toolbar") && shows(&texts, "Navigator"), "the feature's slots");
    assert!(!shows(&texts, "main line"), "the split editors are not drawn");
    assert!(!shows(&texts, "other.txt"), "nor their tabs");

    window.run("workspace.host.collapse", json!({}));
    let texts = window.frames(3);
    assert!(shows(&texts, "main line 199"), "the scroll is as it was");
    assert!(shows(&texts, "other text"), "the other group too");
    assert_eq!(window.workbench().groups().focused(), 2, "the keyboard is back in the group that had it");
    assert_eq!(window.workbench().active_document().unwrap().selections().primary().head, caret);
    assert!(window.view.editor_view.has_focus_in(&window.ctx), "its editor has the keyboard");
}

#[test]
fn a_console_is_the_real_editor_and_feature_tabs_show_their_view() {
    let mut window = window();
    let console = window.console("console.sql", "select 1;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    let texts = window.frames(3);
    assert!(shows(&texts, "select 1;"), "{texts:?}");
    assert!(window.view.editor_view.has_focus_in(&window.ctx), "expanding puts the caret in the console");
    window.workbench().active_document_mut().unwrap().set_selections(Selections::single(Selection::caret(0)));
    window.frame_after(1.0, vec![Event::Text("-- ".to_owned())]);
    let texts = window.frames(2);
    assert!(shows(&texts, "-- select 1;"), "typing goes into the console: {texts:?}");
    window.run("editor.edit.undo", json!({}));
    let texts = window.frames(2);
    assert!(shows(&texts, "select 1;") && !shows(&texts, "-- select"), "undo works in it");

    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    let texts = window.frames(3);
    assert!(shows(&texts, "View of Query History"));
    assert!(!shows(&texts, "select 1;"), "only the active tab shows");

    window.run("file.document.open", json!({ "path": window.project("other.txt") }));
    let texts = window.frames(3);
    assert!(shows(&texts, "other text"), "opening a code file collapses and shows it");
    assert_eq!(window.state()["expanded"], false);
    window.run("workspace.host.toggle", json!({ "id": "test" }));
    let texts = window.frames(3);
    assert!(shows(&texts, "View of Query History"), "the tabs come back as left");
    assert!(shows(&texts, "console.sql"));
}

#[test]
fn the_slide_draws_both_sides_and_the_editor_beneath_takes_no_click() {
    let mut window = window();
    window.frames(2);
    window.run("workspace.host.expand", json!({ "id": "test" }));
    let texts = window.frame_after(0.05, Vec::new());
    assert!(shows(&texts, "other text"), "the editor beneath still shows during the slide");
    let texts = window.frame_after(0.05, Vec::new());
    assert!(shows(&texts, "other text") && shows(&texts, "Toolbar"), "the host slides in over it: {texts:?}");
    let (line, _) = texts.iter().find(|(_, text)| text.contains("other text")).cloned().unwrap();
    let click = line + vec2(4.0, 4.0);
    window.frame_after(0.01, vec![Event::PointerMoved(click), button(click, PointerButton::Primary, true)]);
    window.frame_after(0.01, vec![button(click, PointerButton::Primary, false)]);
    assert_eq!(window.state()["expanded"], true, "a click on the editor beneath does not collapse");
    let texts = window.frame_after(0.2, Vec::new());
    assert!(!shows(&texts, "other text"), "after ~150 ms only the host shows");
}

#[test]
fn inner_tabs_close_on_a_middle_click_reorder_by_drag_and_overflow_into_a_list() {
    let mut window = window();
    for title in ["Alpha", "Beta", "Gamma"] {
        window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": title }));
    }
    let texts = window.frames(3);
    let beta = at(&texts, "Beta") + vec2(4.0, 6.0);
    window.frame_after(0.05, vec![Event::PointerMoved(beta), button(beta, PointerButton::Middle, true)]);
    window.frame_after(0.05, vec![button(beta, PointerButton::Middle, false)]);
    let titles = |window: &mut Window| -> Vec<String> {
        window.state()["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(titles(&mut window), ["Alpha", "Gamma"], "a middle click closes");

    let texts = window.frames(2);
    let alpha = at(&texts, "Alpha") + vec2(4.0, 6.0);
    let past = at(&texts, "Gamma") + vec2(60.0, 6.0);
    window.frame_after(0.05, vec![Event::PointerMoved(alpha), button(alpha, PointerButton::Primary, true)]);
    for step in 1..=6 {
        let pos = alpha + (past - alpha) * (step as f32 / 6.0);
        window.frame_after(0.02, vec![Event::PointerMoved(pos)]);
    }
    window.frame_after(0.02, vec![button(past, PointerButton::Primary, false)]);
    window.frames(1);
    assert_eq!(titles(&mut window), ["Gamma", "Alpha"], "a tab dragged along the strip moves");

    for index in 0..30 {
        let title = format!("Tab number {index:02}");
        window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": title }));
    }
    let texts = window.frames(3);
    assert!(shows(&texts, "Tab number 29"), "the active tab is brought into view");
    assert!(!shows(&texts, "Gamma"), "the first tabs scrolled out of view");
}

#[test]
fn the_modes_keep_an_expanded_workspace_and_the_host_is_named() {
    let mut window = window();
    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    window.frames(2);
    for mode in [Mode::DistractionFree, Mode::Zen, Mode::Presentation] {
        window.view.toggle_mode(mode);
        let texts = window.frames(3);
        assert_eq!(window.state()["expanded"], true, "{mode:?} keeps the workspace expanded");
        assert!(shows(&texts, "View of Query History"), "{mode:?}");
    }
    window.ctx.enable_accesskit();
    window.time += 1.0;
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
        time: Some(window.time),
        ..RawInput::default()
    };
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    let output = window.ctx.run_ui(input, |ui| view.show(ui, registry, session));
    let tree = output.platform_output.accesskit_update.expect("the accessibility tree");
    let names: Vec<String> =
        tree.nodes.iter().filter_map(|(_, node)| crate::accessibility::accessible_name(node, &tree.nodes)).collect();
    assert!(names.iter().any(|name| name == "Test workspace"), "{names:?}");
    assert!(names.iter().any(|name| name == "Query History"), "{names:?}");
}

#[test]
fn snapshots_of_the_host_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let mut window = window();
        window.view.set_theme(&theme);
        window.view.palette.apply(&window.ctx);
        let palette = window.view.palette;
        let console = window.console("console.sql", "select id, name\n  from users\n where id = 1;\n");
        window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
        window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
        window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "sessions", "title": "Sessions" }));
        let id = window.state()["tabs"][0]["id"].clone();
        window.run("workspace.tab.select", json!({ "workspace": "test", "tab": id }));
        let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 700.0), 1.5);
        window.view.palette.apply(screen.context());
        let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
        let draw = |screen: &mut crate::offscreen::Offscreen, view: &mut IdeView, session: &mut Session| {
            screen.render(|ui| view.show(ui, registry, session))
        };
        let expanded = draw(&mut screen, view, session);
        let at = |frame: &crate::offscreen::Frame, x: f32, y: f32| {
            let image = frame.image();
            image.pixels[(y * 1.5) as usize * image.size[0] + (x * 1.5) as usize]
        };
        assert_ne!(at(&expanded, 600.0, 400.0), egui::Color32::TRANSPARENT);
        let _ = palette;
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("workspace-expanded-{name}.png")), expanded.image()).unwrap();
        }
        registry.dispatch(session, "workspace.host.collapse", json!({})).unwrap();
        let collapsed = draw(&mut screen, view, session);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("workspace-collapsed-{name}.png")), collapsed.image())
                .unwrap();
        }
        for index in 0..20 {
            let params = json!({ "workspace": "test", "kind": "history", "title": format!("Result {index}") });
            registry.dispatch(session, "workspace.tab.open", params).unwrap();
        }
        let overflow = draw(&mut screen, view, session);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("workspace-overflow-{name}.png")), overflow.image()).unwrap();
        }
    }
}

#[test]
fn the_workspace_button_on_the_right_stripe_expands_and_collapses_and_the_tabs_survive() {
    let mut window = window();
    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    window.run("workspace.host.collapse", json!({}));
    window.frames(2);
    let button = window.view.workspace_button("test").expect("a button for the registered workspace");
    let git = window.view.workspace_button("git");
    assert!(git.is_none(), "the Git workspace keeps its own button");
    assert!(button.top() > 40.0, "under the Git workspace's button: {button:?}");
    let first_tool = window.view.first_right_tool_button().unwrap();
    assert!(first_tool > button.bottom(), "the right tool windows start under the last workspace button");

    click_at(&mut window, button.center());
    assert_eq!(window.state()["expanded"], true, "a click expands");
    let texts = window.frames(2);
    assert!(shows(&texts, "View of Query History"));
    window.ctx.enable_accesskit();
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
        time: Some(window.time + 1.0),
        ..RawInput::default()
    };
    window.time += 1.0;
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    let output = window.ctx.run_ui(input, |ui| view.show(ui, registry, session));
    let tree = output.platform_output.accesskit_update.expect("the accessibility tree");
    let pressed = tree.nodes.iter().any(|(_, node)| {
        crate::accessibility::accessible_name(node, &tree.nodes).as_deref() == Some("Test")
            && node.toggled() == Some(egui::accesskit::Toggled::True)
    });
    assert!(pressed, "the lit button is pressed for screen readers");

    click_at(&mut window, button.center());
    assert_eq!(window.state()["expanded"], false, "a second click collapses");
    let state = window.state();
    assert_eq!(state["tabs"][0]["title"], "Query History", "the tabs survive: {state}");
    click_at(&mut window, button.center());
    let texts = window.frames(2);
    assert!(shows(&texts, "View of Query History"), "and come back");
}

#[test]
fn find_action_lists_the_workspace_and_reaches_it_in_every_mode() {
    let mut window = window();
    window.frames(1);
    let found =
        super::super::search_everywhere::find_actions("test workspace", &window.registry, &mut window.session, 3);
    let action = found.first().expect("Test Workspace is listed");
    assert_eq!(action.name, "Test Workspace");
    assert_eq!(action.binding.command, "workspace.host.toggle");
    assert_eq!(action.binding.params, json!({ "id": "test" }));
    for mode in [Mode::DistractionFree, Mode::Zen, Mode::Presentation] {
        window.view.toggle_mode(mode);
        window.frames(2);
        let found =
            super::super::search_everywhere::find_actions("test workspace", &window.registry, &mut window.session, 1);
        let binding = found[0].binding.clone();
        let ctx = window.ctx.clone();
        let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
        view.perform_action(&ctx, registry, session, &binding.command, binding.params.clone(), false).unwrap();
        assert_eq!(window.state()["expanded"], true, "{mode:?}: Find Action expands it with the stripes hidden");
        window.run("workspace.host.collapse", json!({}));
        window.view.toggle_mode(mode);
    }
}

#[test]
fn shift_escape_hides_a_tool_window_that_has_the_keyboard_and_collapses_the_workspace_otherwise() {
    let mut window = window();
    let console = window.console("console.sql", "select 1;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    window.view.toggle_tool("problems");
    window.frames(2);
    let key =
        |modifiers| Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers };
    window.frame_after(0.05, vec![key(Modifiers::SHIFT)]);
    window.frames(1);
    assert!(!window.view.tool_visible("problems"), "the tool window shown last had the keyboard");
    assert_eq!(window.state()["expanded"], true);
    let console_text = window.frames(1);
    let line = at(&console_text, "select 1;") + vec2(4.0, 4.0);
    click_at(&mut window, line);
    window.frame_after(0.05, vec![key(Modifiers::NONE)]);
    window.frames(1);
    assert_eq!(window.state()["expanded"], true, "⎋ never collapses");
    window.frame_after(0.05, vec![key(Modifiers::SHIFT)]);
    window.frames(1);
    assert_eq!(window.state()["expanded"], false, "⇧⎋ in the workspace collapses it");
}

#[test]
fn a_restart_restores_the_workspace_as_left_and_connects_nowhere() {
    let mut window = window();
    let code: String = (0..300).map(|line| format!("select {line};\n")).collect();
    let console = window.console("console.sql", &code);
    let gone = window.console("gone.sql", "select 0;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": gone }));
    window.run(
        "workspace.tab.open",
        json!({ "workspace": "test", "kind": "history", "title": "Query History", "data": { "filter": "select" } }),
    );
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    window.run("editor.caret.goTo", json!({ "line": 120, "column": 3 }));
    window.frames(3);
    window.view.editor_view.scroll_to_line(100);
    window.frames(3);
    let project = window.dir.path().join("project");
    let layout = super::super::workspace::capture(&window.view, &mut window.session, &project);
    assert!(
        layout.documents.iter().all(|path| !path.starts_with(window.dir.path().join("data"))),
        "consoles are not main tabs"
    );
    let saved = serde_json::to_value(&layout).unwrap();
    let kept = &saved["workspaces"][0];
    assert_eq!(kept["expanded"], true, "{kept}");
    assert_eq!(kept["tabs"].as_array().unwrap().len(), 3);
    assert_eq!(kept["tabs"][1]["data"], json!({ "filter": "select" }));
    assert_eq!(kept["tabs"][2]["firstLine"], 100);
    assert!(kept["tabs"][2]["caret"].as_u64().unwrap() > 0);
    std::fs::remove_file(&gone).unwrap();

    let mut saved = saved;
    saved["workspaces"].as_array_mut().unwrap().push(json!({ "id": 7, "tabs": "broken" }));
    saved["workspaces"][0]["tabs"].as_array_mut().unwrap().push(json!({ "title": "no id" }));
    saved["workspaces"][0]["panels"] = json!({ "navigator": 260.0, "bad": "wide" });
    let read: super::super::workspace::ProjectLayout = serde_json::from_value(saved).unwrap();
    assert_eq!(read.workspaces.len(), 1, "a bad entry drops itself");
    assert_eq!(read.workspaces[0].tabs.len(), 3, "a bad tab drops itself");
    let older: super::super::workspace::ProjectLayout = serde_json::from_value(json!({ "documents": [] })).unwrap();
    assert!(older.workspaces.is_empty(), "older files load");

    let mut restarted = window_without_files(&window);
    let connects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    restarted.registry.register(Connect(Arc::clone(&connects))).unwrap();
    let (view, registry, session) = (&mut restarted.view, &restarted.registry, &mut restarted.session);
    super::super::workspace::restore(view, registry, session, &read);
    let texts = restarted.frames(3);
    assert_eq!(connects.load(std::sync::atomic::Ordering::SeqCst), 0, "restoring connects nowhere");
    let state = restarted.state();
    let titles: Vec<&str> =
        state["tabs"].as_array().unwrap().iter().map(|tab| tab["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Query History", "console.sql"], "the missing console is dropped, the order kept");
    assert_eq!(state["expanded"], true);
    assert_eq!(state["tabs"][0]["data"], json!({ "filter": "select" }));
    assert!(shows(&texts, "select 100;") && !shows(&texts, "select 99;"), "the console shows from its first line");
    let caret = restarted.workbench().active_document().unwrap().selections().primary().head;
    assert_eq!(caret as u64, kept_caret(&read), "the console's caret comes back");
    let again = super::super::workspace::capture(&restarted.view, &mut restarted.session, &project);
    assert_eq!(again.workspaces[0].panels.get("navigator"), Some(&260.0), "the feature's panel sizes come back");
}

fn kept_caret(layout: &super::super::workspace::ProjectLayout) -> u64 {
    layout.workspaces[0].tabs.iter().find(|tab| tab.title == "console.sql").and_then(|tab| tab.caret).unwrap() as u64
}

fn window_without_files(first: &Window) -> Window {
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let project = first.dir.path().join("project");
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    let workspaces = session.state_mut::<Workspaces>();
    workspaces.set_data_dir(first.dir.path().join("data"));
    workspaces.register(WorkspaceInfo { id: "test".into(), title: "Test".into(), icon: "database".into() });
    let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
    view.project_open = false;
    view.register_workspace_view("test", Box::new(TestView::default()));
    let dir = tempfile::tempdir().unwrap();
    Window { ctx: egui::Context::default(), registry, session, view, dir, time: 0.0 }
}

#[test]
fn the_database_workspace_opens_on_its_overview_and_new_console_gives_a_real_editor() {
    zen_i18n::register(zen_workbench::LOCALES);
    let mut window = window();
    zen_workbench::database::register(&mut window.registry).unwrap();
    zen_workbench::database::install(&mut window.session);
    window.run("workspace.host.expand", json!({ "id": "database" }));
    let texts = window.frames(3);
    assert!(shows(&texts, "No connections yet") && shows(&texts, "Overview"), "{texts:?}");
    let button = texts.iter().filter(|(_, text)| text == "New Console").map(|(at, _)| *at).next_back().unwrap();
    click_at(&mut window, button + vec2(8.0, 6.0));
    window.frames(2);
    assert!(window.view.connection_dialog.is_some(), "with no connection, New Console opens New Connection");
    window.view.connection_dialog = None;
    window.run("db.dataSource.add", json!({ "name": "Shop", "url": "postgresql://ann@localhost:5432/shop" }));
    window.frames(2);
    click_at(&mut window, button + vec2(8.0, 6.0));
    let texts = window.frames(2);
    assert!(shows(&texts, "console.sql"), "the console's tab: {texts:?}");
    assert!(window.view.editor_view.has_focus_in(&window.ctx), "the caret is in the console");
    let console = window.workbench().active_document().unwrap().path().to_owned();
    assert!(console.starts_with(window.dir.path().join("data/consoles")), "{console:?}");
    assert!(console.ends_with("local/console.sql"));

    window.frame_after(1.0, vec![Event::Text("select 1;".to_owned())]);
    let texts = window.frames(2);
    assert!(shows(&texts, "select 1;"), "typing goes into the console: {texts:?}");
    window.run("editor.find.open", json!({}));
    window.run("editor.find.set", json!({ "query": { "text": "select" } }));
    let found = window.run("editor.find.info", json!({}));
    assert_eq!(found["count"], 1, "{found}");
    let texts = window.frames(2);
    assert!(shows(&texts, "1/1"), "the search bar shows over the console: {texts:?}");
}

#[test]
fn the_overview_lists_connections_and_connects_them() {
    zen_i18n::register(zen_workbench::LOCALES);
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let mut window = window();
        window.view.set_theme(&theme);
        window.view.palette.apply(&window.ctx);
        zen_workbench::database::register(&mut window.registry).unwrap();
        zen_workbench::database::install(&mut window.session);
        let secrets = Arc::new(zen_secrets::MemoryStore::new());
        window.session.set_state(zen_workbench::database::Secrets(secrets));
        let driver = Arc::new(zen_db::memory::MemoryDriver::new(false));
        let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::new(zen_db::postgres::Postgres), driver];
        window.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
        window.run(
            "db.dataSource.add",
            json!({ "name": "Orders", "driver": "memory-kv", "host": "cache.local", "password": "pw" }),
        );
        window.run(
            "db.dataSource.add",
            json!({ "name": "Billing", "host": "db.prod", "database": "billing", "type": "prod" }),
        );
        window.run("db.dataSource.add", json!({ "name": "Reports", "host": "localhost", "type": "test" }));
        window.run("workspace.host.expand", json!({ "id": "database" }));
        let texts = window.frames(2);
        assert!(shows(&texts, "Orders") && shows(&texts, "Billing") && shows(&texts, "Disconnected"), "{texts:?}");
        let connect = texts.iter().filter(|(_, text)| text == "Connect").map(|(at, _)| *at).next().unwrap();
        click_at(&mut window, connect + vec2(8.0, 6.0));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut texts = window.frames(1);
        while !shows(&texts, "Connected") && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
            texts = window.frames(2);
        }
        assert!(shows(&texts, "Connected") && shows(&texts, "Disconnect"), "{texts:?}");
        let edit = texts.iter().filter(|(_, text)| text == "Edit").map(|(at, _)| *at).next().unwrap();
        click_at(&mut window, edit + vec2(8.0, 6.0));
        window.frames(1);
        assert_eq!(window.view.window_state(&window.session)["popups"], json!(["connectionDialog"]));
        let (command, params) = window.view.connection_dialog.as_ref().unwrap().save_call();
        assert_eq!(
            (command, params["name"].as_str()),
            ("db.dataSource.update", Some("Orders")),
            "Edit reopens the dialog"
        );
        window.view.connection_dialog = None;
        window.frames(1);
        let new = texts.iter().filter(|(_, text)| text == "New Connection…").map(|(at, _)| *at).next().unwrap();
        click_at(&mut window, new + vec2(8.0, 6.0));
        window.frames(1);
        let (command, _) = window.view.connection_dialog.as_ref().unwrap().save_call();
        assert_eq!(command, "db.dataSource.add", "New Connection… opens it empty");
        window.view.connection_dialog = None;
        window.frames(1);
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 600.0), 1.5);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-overview-{name}.png")), frame.image()).unwrap();
        }
    }
}

fn database_window(theme: &zen_theme::ResolvedTheme) -> Window {
    zen_i18n::register(zen_workbench::LOCALES);
    let mut window = window();
    window.view.set_theme(theme);
    window.view.palette.apply(&window.ctx);
    zen_workbench::database::register(&mut window.registry).unwrap();
    zen_workbench::database::install(&mut window.session);
    window.session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let driver = Arc::new(zen_db::memory::MemoryDriver::new(false));
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::new(zen_db::postgres::Postgres), driver];
    window.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    window
        .run("db.dataSource.add", json!({ "name": "Orders", "driver": "memory-kv", "type": "test", "password": "pw" }));
    window
}

fn key(key: egui::Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers }
}

fn until(window: &mut Window, what: &str, done: impl Fn(&[(Pos2, String)]) -> bool) -> Vec<(Pos2, String)> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let texts = window.frames(1);
        if done(&texts) {
            return texts;
        }
        assert!(std::time::Instant::now() < deadline, "{what}: {texts:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn option_x_runs_the_script_without_typing_its_character() {
    let mut window = database_window(&zen_theme::ResolvedTheme::zen_dark());
    let console = window.run("db.console.new", json!({ "text": "SET greeting hello\nGET greeting\n" }));
    window.frames(3);
    let option = Modifiers { alt: true, ..Modifiers::NONE };
    window.frame_after(0.1, vec![key(egui::Key::X, option), Event::Text("≈".to_owned())]);
    until(&mut window, "the script ran", |texts| shows(texts, "2 statements"));
    let text = std::fs::read_to_string(console["path"].as_str().unwrap()).unwrap_or_default();
    let shown = window
        .session
        .state::<Workbench>()
        .and_then(|workbench| workbench.active_document().map(|document| document.text().to_owned()))
        .unwrap_or(text);
    assert!(!shown.contains('≈'), "⌥X typed its character: {shown:?}");
}

#[test]
fn a_console_runs_its_script_and_shows_results_output_and_log() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let mut window = database_window(&theme);
        window.run(
            "db.console.new",
            json!({ "text": "SET greeting hello\nNOTICE hello from the server\nROWS 300\nGET greeting\n" }),
        );
        let texts = window.frames(3);
        assert!(
            shows(&texts, "Orders") && shows(&texts, "Default schema"),
            "the toolbar shows the console's connection: {texts:?}"
        );
        window.frame_after(
            0.1,
            vec![key(egui::Key::Enter, Modifiers { alt: true, mac_cmd: true, command: true, ..Modifiers::NONE })],
        );
        let texts = until(&mut window, "the script ran", |texts| {
            shows(texts, "Result 2") && shows(texts, "300 rows –") && shows(texts, "4 statements")
        });
        assert!(
            shows(&texts, "Result 1") && shows(&texts, "Output (1)") && shows(&texts, "Execution Log"),
            "{texts:?}"
        );
        assert!(
            shows(&texts, "4 statements") && shows(&texts, "1 row updated") && shows(&texts, "301 rows read"),
            "the header line: {texts:?}"
        );
        assert!(shows(&texts, "[NULL]") && shows(&texts, "row 0") && shows(&texts, "nothing"), "the grid: {texts:?}");
        assert!(shows(&texts, "300 rows"), "the time and rows after the statement, inline: {texts:?}");
        assert!(shows(&texts, "Grid") && shows(&texts, "Record") && shows(&texts, "Fetch All"), "the bottom bar");
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-console-results-{name}.png")), frame.image())
                .unwrap();
        }
        let output = at(&texts, "Output (1)");
        click_at(&mut window, output + vec2(6.0, 6.0));
        let texts = window.frames(2);
        assert!(shows(&texts, "NOTICE: hello from the server"), "the server's output: {texts:?}");
        let log = at(&texts, "Execution Log");
        click_at(&mut window, log + vec2(6.0, 6.0));
        let texts = window.frames(2);
        assert!(shows(&texts, "ROWS 300") && shows(&texts, "300 rows"), "the execution log: {texts:?}");
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-console-log-{name}.png")), frame.image()).unwrap();
        }
        let first = window.run("db.console.state", json!({}))["tabs"][0]["result"].clone();
        window.run("db.console.showPanelTab", json!({ "tab": { "kind": "result", "result": first } }));
        window.run("db.result.view", json!({ "view": "record" }));
        let texts = window.frames(2);
        assert!(shows(&texts, "Row 1 of 300") || shows(&texts, "Filter columns"), "the record view: {texts:?}");
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-console-record-{name}.png")), frame.image())
                .unwrap();
        }
        window.run("db.result.view", json!({ "view": "grid" }));
        if name == "dark" {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.0);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let tree = screen.accessibility_tree(|ui| view.show(ui, registry, session));
            let names: Vec<String> = tree
                .nodes
                .iter()
                .filter_map(|(_, node)| crate::accessibility::accessible_name(node, &tree.nodes))
                .collect();
            assert!(names.iter().any(|name| name == "Results"), "{names:?}");
            assert!(names.iter().any(|name| name == "Execution Log"), "{names:?}");
            assert!(names.iter().any(|name| name == "Execute SQL Statement"), "{names:?}");
        }
    }
}

#[test]
fn a_failing_statement_says_where_and_cancel_stops_a_long_one() {
    let theme = zen_theme::ResolvedTheme::zen_dark();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut window = database_window(&theme);
    window.run("db.console.new", json!({ "text": "SET a 1\nFAIL here\n" }));
    window.frames(2);
    window.run("db.console.executeScript", json!({}));
    let texts = until(&mut window, "the error shows", |texts| shows(texts, "Failed at line 2"));
    assert!(shows(&texts, "syntax error"), "{texts:?}");
    let state = window.run("db.console.state", json!({}));
    assert_eq!(state["panel"]["active"]["kind"], "log");
    if let Some(dir) = &snapshots {
        let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
        window.view.palette.apply(screen.context());
        let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
        let frame = screen.render(|ui| view.show(ui, registry, session));
        crate::offscreen::write_png(&dir.join("database-console-error-dark.png"), frame.image()).unwrap();
    }
    window.run("editor.selection.selectAll", json!({}));
    window.run("editor.edit.insert", json!({ "text": "SLEEP 5000" }));
    window.frames(1);
    window.frame_after(0.1, vec![key(egui::Key::Enter, Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE })]);
    let texts = until(&mut window, "it runs", |texts| shows(texts, "Running…"));
    let cancel = at(&texts, "Cancel");
    click_at(&mut window, cancel + vec2(4.0, 4.0));
    until(&mut window, "it was cancelled", |texts| shows(texts, "Cancelled"));
}

#[test]
fn detected_databases_are_offered_on_the_overview_and_imports_are_listed_before_they_are_taken() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let mut window = database_window(&theme);
        std::fs::write(
            window.project("compose.yaml"),
            "services:\n  db:\n    image: postgres:17\n    ports: [\"5434:5432\"]\n    environment:\n      POSTGRES_PASSWORD: pw\n      POSTGRES_DB: shop\n  cache:\n    image: redis:7\n",
        )
        .unwrap();
        std::fs::write(window.project(".env"), "DATABASE_URL=postgres://app@db.local:5432/orders\n").unwrap();
        std::fs::create_dir_all(window.project(".idea")).unwrap();
        std::fs::write(
            window.project(".idea/dataSources.xml"),
            r#"<project version="4"><component name="DataSourceManagerImpl"><data-source name="warehouse@db" uuid="w1"><driver-ref>postgresql</driver-ref><jdbc-url>jdbc:postgresql://db:5432/warehouse</jdbc-url></data-source><data-source name="snow" uuid="s1"><driver-ref>snowflake</driver-ref><jdbc-url>jdbc:snowflake://x</jdbc-url></data-source></component></project>"#,
        )
        .unwrap();
        window.run("workspace.host.expand", json!({ "id": "database" }));
        let texts = until(&mut window, "the offers", |texts| shows(texts, "Detected in this project"));
        assert!(shows(&texts, "PostgreSQL localhost:5434 / shop · compose.yaml"), "{texts:?}");
        assert!(shows(&texts, "PostgreSQL db.local:5432 / orders · .env") && shows(&texts, "Redis"), "{texts:?}");
        assert!(shows(&texts, "port not published"), "{texts:?}");
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 600.0), 1.5);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-detected-{name}.png")), frame.image()).unwrap();
        }
        let add = texts.iter().filter(|(_, text)| text == "Add…").map(|(at, _)| *at).next().unwrap();
        click_at(&mut window, add + vec2(8.0, 6.0));
        window.frames(1);
        let (command, params) = window.view.connection_dialog.as_ref().unwrap().save_call();
        assert_eq!((command, params["host"].as_str()), ("db.detect.accept", Some("db.local")), "{params}");
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 700.0), 1.5);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-detected-dialog-{name}.png")), frame.image())
                .unwrap();
        }
        window.view.connection_dialog = None;
        let texts = window.frames(1);
        let dismiss = texts.iter().filter(|(_, text)| text == "Dismiss").map(|(at, _)| *at).next().unwrap();
        click_at(&mut window, dismiss + vec2(8.0, 6.0));
        let texts = window.frames(2);
        assert!(!shows(&texts, "orders · .env") && shows(&texts, "compose.yaml"), "a dismissed offer goes: {texts:?}");

        super::super::import_dialog::request(&mut window.session, Some("jetbrains"));
        let texts = window.frames(2);
        assert_eq!(window.view.window_state(&window.session)["popups"], json!(["importConnections"]));
        assert!(shows(&texts, "warehouse@db") && shows(&texts, "no driver for snowflake"), "{texts:?}");
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 700.0), 1.5);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-import-{name}.png")), frame.image()).unwrap();
        }
        let import = texts.iter().filter(|(_, text)| text == "Import").map(|(at, _)| *at).next().unwrap();
        click_at(&mut window, import + vec2(8.0, 6.0));
        window.frames(1);
        assert!(window.view.import_dialog.is_none());
        let listed = window.run("db.dataSource.list", json!({}));
        assert!(listed.to_string().contains("warehouse@db"), "{listed}");
    }
}

fn customers_snapshot(id: &str, driver: &str) -> zen_db::schema::Snapshot {
    use zen_db::schema::{
        Catalog, Column, Constraint, ConstraintKind, Index, Level, Object, ObjectKind, Reference, Schema,
    };
    let column = |position: i32, name: &str, type_name: &str, nullable: bool| Column {
        name: name.into(),
        position,
        type_name: type_name.into(),
        nullable,
        ..Column::default()
    };
    let mut customers = Object::new(ObjectKind::Table, "customers");
    customers.detailed = true;
    customers.owner = Some("ann".into());
    customers.comment = Some("People who buy".into());
    customers.size = Some(98_304);
    customers.rows = Some(25);
    customers.acl = vec!["ann=arwdDxtm/ann".into(), "reporting=r/ann".into()];
    customers.columns = vec![
        Column { identity: Some("always".into()), ..column(1, "id", "bigint", false) },
        Column { comment: Some("Full name".into()), ..column(2, "name", "text", true) },
        Column { default: Some("'{}'::jsonb".into()), ..column(3, "payload", "jsonb", true) },
        column(4, "picture", "bytea", true),
        column(5, "tags", "text[]", true),
        Column { default: Some("now()".into()), ..column(6, "created", "timestamp with time zone", false) },
    ];
    customers.constraints = vec![Constraint {
        name: "customers_pkey".into(),
        kind: ConstraintKind::PrimaryKey,
        columns: vec!["id".into()],
        definition: "PRIMARY KEY (id)".into(),
        references: None,
        index: Some("customers_pkey".into()),
        comment: None,
    }];
    customers.indexes = vec![
        Index {
            name: "customers_pkey".into(),
            columns: vec!["id".into()],
            unique: true,
            primary: true,
            method: "btree".into(),
            size: Some(16_384),
            definition: "CREATE UNIQUE INDEX customers_pkey ON public.customers USING btree (id)".into(),
            constraint: Some("customers_pkey".into()),
            comment: None,
        },
        Index {
            name: "customers_name".into(),
            columns: vec!["name".into()],
            method: "btree".into(),
            size: Some(8_192),
            definition: "CREATE INDEX customers_name ON public.customers USING btree (name)".into(),
            ..Index::default()
        },
    ];
    let mut orders = Object::new(ObjectKind::Table, "orders");
    orders.detailed = true;
    orders.columns = vec![column(1, "id", "bigint", false), column(2, "customer", "bigint", false)];
    orders.constraints = vec![Constraint {
        name: "orders_customer_fkey".into(),
        kind: ConstraintKind::ForeignKey,
        columns: vec!["customer".into()],
        definition: "FOREIGN KEY (customer) REFERENCES customers(id)".into(),
        references: Some(Reference {
            schema: "public".into(),
            table: "customers".into(),
            columns: vec!["id".into()],
            on_update: "no action".into(),
            on_delete: "cascade".into(),
        }),
        index: None,
        comment: None,
    }];
    zen_db::schema::Snapshot {
        taken: 1,
        catalogs: vec![Catalog {
            name: "shop".into(),
            current: true,
            schemas_read: true,
            schemas: vec![Schema {
                name: "public".into(),
                owner: Some("ann".into()),
                level: Level::Details,
                objects: vec![customers, orders],
                ..Schema::default()
            }],
            ..Catalog::default()
        }],
        ..zen_db::schema::Snapshot::new(id, driver)
    }
}

fn snapshot_png(window: &mut Window, dir: &Option<PathBuf>, name: &str) {
    if let Some(dir) = dir {
        let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
        window.view.palette.apply(screen.context());
        let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
        screen.render(|ui| view.show(ui, registry, session));
        let frame = screen.render(|ui| view.show(ui, registry, session));
        crate::offscreen::write_png(&dir.join(format!("{name}.png")), frame.image()).unwrap();
    }
}

fn snapshot_variants(window: &mut Window, dir: &Option<PathBuf>, name: &str) {
    snapshot_png(window, dir, name);
    let Some(dir) = dir else { return };
    for (variant, size, language) in [("narrow", vec2(900.0, 640.0), "en"), ("ru", vec2(1400.0, 900.0), "ru")] {
        zen_i18n::set_language(Some(language));
        let mut screen = crate::offscreen::Offscreen::new(size, 1.5);
        window.view.palette.apply(screen.context());
        let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
        screen.render(|ui| view.show(ui, registry, session));
        let frame = screen.render(|ui| view.show(ui, registry, session));
        crate::offscreen::write_png(&dir.join(format!("{name}-{variant}.png")), frame.image()).unwrap();
        zen_i18n::set_language(Some("en"));
    }
}

#[test]
fn an_object_tab_shows_its_data_properties_and_ddl_and_the_value_panel_reads_json() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let mut window = database_window(&theme);
        let listed = window.run("db.dataSource.list", json!({}));
        let memory = listed["connections"][0]["id"].as_str().unwrap().to_owned();
        let added = window.run(
            "db.dataSource.add",
            json!({ "name": "Shop", "url": "postgresql://ann@db.example.com:5432/shop", "type": "prod" }),
        );
        let shop = added["id"].as_str().unwrap().to_owned();
        for (id, driver) in [(&memory, "memory-kv"), (&shop, "postgresql")] {
            let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
            zen_db::schema::write_snapshot(&file, &customers_snapshot(id, driver)).unwrap();
        }
        let customers = json!(["shop", "public", "tables", "customers"]);

        window.run("db.object.open", json!({ "connection": shop, "path": customers }));
        let texts = window.frames(3);
        assert!(shows(&texts, "Read Data") && shows(&texts, "SELECT * FROM \"public\".\"customers\""), "{texts:?}");
        let status = window.run("db.connection.status", json!({ "connection": shop }));
        assert_eq!(status["connections"][0]["state"], "disconnected", "drawing the Data page connects nowhere");
        window.run("db.object.showPage", json!({ "page": "properties" }));
        let texts = window.frames(3);
        for wanted in [
            "Data",
            "Properties",
            "DDL",
            "Diagram",
            "Tablespace",
            "People who buy",
            "Foreign Keys",
            "identity (always)",
        ] {
            assert!(shows(&texts, wanted), "{wanted}: {texts:?}");
        }
        snapshot_png(&mut window, &snapshots, &format!("database-object-properties-{name}"));
        window.run("db.object.ddlOptions", json!({ "nested": true, "comments": true }));
        window.run("db.object.showPage", json!({ "page": "ddl" }));
        let texts = window.frames(3);
        assert!(
            shows(&texts, "CREATE TABLE \"public\".\"customers\"") && shows(&texts, "Open in Console"),
            "{texts:?}"
        );
        assert!(shows(&texts, "CREATE INDEX customers_name"), "nested objects: {texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-object-ddl-{name}"));

        window.run("db.object.open", json!({ "connection": memory, "path": customers }));
        window.run("db.object.readData", json!({}));
        let texts =
            until(&mut window, "the rows were read", |texts| shows(texts, "Customer 1") && shows(texts, "25 rows –"));
        assert!(shows(&texts, "Value Panel"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-object-data-{name}"));
        let state = window.run("db.object.state", json!({}));
        let result = state["data"]["result"].as_u64().unwrap();
        let cell =
            texts.iter().find(|(_, text)| text.starts_with("{\"id\":1,")).map(|(at, _)| *at).expect("the payload cell");
        click_at(&mut window, cell + vec2(6.0, 6.0));
        window.run("db.result.toggleValuePanel", json!({ "shown": true }));
        let texts = until(&mut window, "the value panel", |texts| shows(texts, "\"tier\": \"gold\""));
        assert!(shows(&texts, "payload · jsonb") && shows(&texts, "JSON") && shows(&texts, "Calc"), "{texts:?}");
        let value = window.run("db.result.value", json!({ "id": result, "row": 0, "column": 2 }));
        assert_eq!(value["format"], "json");
        snapshot_png(&mut window, &snapshots, &format!("database-value-panel-{name}"));
        if name == "dark" {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.0);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let tree = screen.accessibility_tree(|ui| view.show(ui, registry, session));
            let names: Vec<String> = tree
                .nodes
                .iter()
                .filter_map(|(_, node)| crate::accessibility::accessible_name(node, &tree.nodes))
                .collect();
            for wanted in ["Data", "Properties", "Value", "Value Panel", "Calc"] {
                assert!(
                    names.iter().any(|known| known == wanted || known.starts_with(wanted)),
                    "{wanted} in {names:?}"
                );
            }
        }
        window.run("db.result.toggleValuePanel", json!({ "shown": false }));
        window.run("db.connection.disconnect", json!({ "connection": memory, "transactions": "rollback" }));
    }
}

#[test]
fn the_value_panel_draws_the_spatial_values_of_the_selection_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let mut window = database_window(&theme);
        window.run("db.console.new", json!({ "text": "PLACES\n" }));
        window.frames(3);
        window.frame_after(
            0.1,
            vec![key(egui::Key::Enter, Modifiers { alt: true, mac_cmd: true, command: true, ..Modifiers::NONE })],
        );
        let texts = until(&mut window, "the places were read", |texts| shows(texts, "Park") && shows(texts, "Nowhere"));
        let cell = |texts: &[(Pos2, String)], start: &str| {
            texts
                .iter()
                .find(|(_, text)| text.starts_with(start))
                .map(|(at, _)| *at)
                .unwrap_or_else(|| panic!("{start}"))
        };
        let (area, point, route) = (cell(&texts, "\\x0103"), cell(&texts, "\\x0101"), cell(&texts, "LINESTRING"));
        click_at(&mut window, pos2(route.x, point.y) + vec2(6.0, 6.0));
        let at = area + vec2(6.0, 6.0);
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        window.frame_after(
            0.05,
            vec![
                Event::PointerMoved(at),
                Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: shift },
            ],
        );
        window.frame_after(
            0.05,
            vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: shift }],
        );
        window.run("db.result.toggleValuePanel", json!({ "shown": true }));
        window.run("db.console.maximizeResults", json!({}));
        let texts = until(&mut window, "the geometry viewer", |texts| {
            shows(texts, "POLYGON · SRID 4326 · 10 points · 3 shapes in the selection")
        });
        assert!(shows(&texts, "area · geometry") && shows(&texts, "Geometry"), "{texts:?}");
        let figures = texts.iter().find(|(_, text)| text.starts_with("Count: 3")).map(|(_, text)| text.clone());
        assert!(figures.as_ref().is_some_and(|text| text.len() < 160 && text.contains("…")), "{figures:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-geometry-viewer-{name}"));
        if name == "dark" {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.0);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let tree = screen.accessibility_tree(|ui| view.show(ui, registry, session));
            let names: Vec<String> = tree
                .nodes
                .iter()
                .filter_map(|(_, node)| crate::accessibility::accessible_name(node, &tree.nodes))
                .collect();
            assert!(names.iter().any(|known| known.starts_with("Geometry viewer: POLYGON")), "{names:?}");
        }
        window.run("db.result.toggleValuePanel", json!({ "shown": false }));
    }
}

#[test]
fn structure_changes_are_staged_on_properties_and_save_shows_their_script() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let mut window = database_window(&theme);
        let added = window.run(
            "db.dataSource.add",
            json!({ "name": "Shop", "url": "postgresql://ann@db.example.com:5432/shop", "type": "dev" }),
        );
        let shop = added["id"].as_str().unwrap().to_owned();
        let file = window.dir.path().join(format!("data/caches/databases/global/{shop}.snapshot"));
        zen_db::schema::write_snapshot(&file, &customers_snapshot(&shop, "postgresql")).unwrap();
        let opened = window.run(
            "db.object.open",
            json!({ "connection": shop, "path": ["shop", "public", "tables", "customers"], "page": "properties" }),
        );
        let tab = opened["tab"]["id"].as_str().unwrap().to_owned();
        let texts = window.frames(3);
        for wanted in ["Add Column", "Edit Column", "Rename…", "Comment…", "Delete…"] {
            assert!(shows(&texts, wanted), "{wanted}: {texts:?}");
        }
        click_at(&mut window, at(&texts, "Add Column") + vec2(6.0, 6.0));
        let texts = window.frames(2);
        assert!(shows(&texts, "Add Column to customers") && shows(&texts, "Add to Changes"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-structure-form-{name}"));
        click_at(&mut window, at(&texts, "Cancel") + vec2(6.0, 6.0));
        let texts = window.frames(2);
        assert!(!shows(&texts, "Add to Changes"), "Cancel closes the form: {texts:?}");
        window.run(
            "db.structure.stage",
            json!({ "tab": tab, "change": { "action": "addColumn", "schema": "public", "table": "customers",
                "column": { "name": "email", "type": "varchar(200)", "notNull": true, "default": "''" } } }),
        );
        window.run(
            "db.structure.stage",
            json!({ "tab": tab, "change": { "action": "dropColumn", "schema": "public", "table": "customers",
                "column": "tags" } }),
        );
        let texts = window.frames(3);
        assert!(shows(&texts, "2 changes not run"), "{texts:?}");
        assert!(shows(&texts, "1. Add column email varchar(200) to customers"), "{texts:?}");
        assert!(shows(&texts, "2. Drop column tags of customers"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-structure-staged-{name}"));
        click_at(&mut window, at(&texts, "Save…") + vec2(6.0, 6.0));
        let texts = window.frames(3);
        assert!(shows(&texts, "Structure Changes Script") && shows(&texts, "4 statements will run."), "{texts:?}");
        for wanted in ["Execute", "Open in Console", "Save as Migration", "ADD COLUMN \"email\" varchar(200)"] {
            assert!(shows(&texts, wanted), "{wanted}: {texts:?}");
        }
        snapshot_png(&mut window, &snapshots, &format!("database-structure-preview-{name}"));
        click_at(&mut window, at(&texts, "Cancel") + vec2(6.0, 6.0));
        let state = window.run("db.structure.state", json!({ "tab": tab }));
        assert_eq!(state["total"], 2, "Cancel keeps the changes staged");
        let status = window.run("db.connection.status", json!({ "connection": shop }));
        assert_eq!(status["connections"][0]["state"], "disconnected", "staging and the preview connect nowhere");
    }
}

#[test]
fn find_usages_and_a_rename_on_an_object_tab_list_the_usages_in_the_find_tool_window() {
    let mut window = database_window(&zen_theme::ResolvedTheme::zen_dark());
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "url": "postgresql://ann@db.example.com:5432/shop", "type": "dev" }),
    );
    let shop = added["id"].as_str().unwrap().to_owned();
    let file = window.dir.path().join(format!("data/caches/databases/global/{shop}.snapshot"));
    zen_db::schema::write_snapshot(&file, &customers_snapshot(&shop, "postgresql")).unwrap();
    std::fs::write(window.project("report.sql"), "SELECT name FROM customers;\n").unwrap();
    window.run(
        "db.object.open",
        json!({ "connection": shop, "path": ["shop", "public", "tables", "customers"], "page": "properties" }),
    );
    let texts = window.frames(3);
    click_at(&mut window, at(&texts, "Find Usages") + vec2(6.0, 6.0));
    let results = window.run("search.results.tree", json!({}));
    assert_eq!((results["kind"].as_str(), results["title"].as_str()), (Some("database"), Some("Usages of customers")));

    let texts = window.frames(2);
    click_at(&mut window, at(&texts, "Rename…") + vec2(6.0, 6.0));
    let texts = window.frames(2);
    assert!(shows(&texts, "Find usages in the project to rename"), "{texts:?}");
    let field = texts.iter().rev().find(|(_, text)| text == "customers").map(|(at, _)| *at).unwrap();
    click_at(&mut window, field + vec2(4.0, 4.0));
    window.frame_after(0.1, vec![key(egui::Key::A, Modifiers::COMMAND)]);
    window.frame_after(0.1, vec![Event::Text("clients".into())]);
    let texts = window.frames(2);
    click_at(&mut window, at(&texts, "Add to Changes") + vec2(6.0, 6.0));
    let results = window.run("search.results.tree", json!({}));
    assert_eq!(results["title"], "Rename customers to clients", "{results}");
    let status = window.run("db.connection.status", json!({ "connection": shop }));
    assert_eq!(status["connections"][0]["state"], "disconnected", "finding usages connects nowhere");
}

fn sql_window(theme: &zen_theme::ResolvedTheme) -> (Window, String, Arc<zen_db::memory::MemoryDriver>) {
    zen_i18n::register(zen_workbench::LOCALES);
    let mut window = window();
    window.view.set_theme(theme);
    window.view.palette.apply(&window.ctx);
    zen_workbench::database::register(&mut window.registry).unwrap();
    zen_workbench::database::install(&mut window.session);
    window.session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let driver = Arc::new(zen_db::memory::MemoryDriver::sql());
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::clone(&driver) as _];
    window.session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let added = window.run(
        "db.dataSource.add",
        json!({ "name": "Shop", "driver": zen_db::memory::SQL, "type": "test", "password": "pw" }),
    );
    let id = added["id"].as_str().unwrap().to_owned();
    let mut snapshot = customers_snapshot(&id, zen_db::memory::SQL);
    snapshot.catalogs[0].schemas[0].objects[0].oid = zen_db::memory::TABLE;
    let file = window.dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &snapshot).unwrap();
    (window, id, driver)
}

fn read_done(window: &mut Window, statement: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        window.frames(1);
        let state = window.run("db.object.state", json!({}));
        let data = &state["data"];
        if data["reading"] == false && data["statement"].as_str().is_some_and(|text| text.contains(statement)) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "the rows of `{statement}` were not read: {state}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_data_view_filters_with_completion_stages_edits_and_previews_them_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, driver) = sql_window(&theme);
        window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
        window.run("db.object.readData", json!({}));
        let texts =
            until(&mut window, "the rows were read", |texts| shows(texts, "Customer 1") && shows(texts, "WHERE"));
        assert!(shows(&texts, "SELECT * FROM \"public\".\"customers\""), "the statement applied shows: {texts:?}");

        let field = at(&texts, "WHERE") + vec2(80.0, 6.0);
        click_at(&mut window, field);
        window.frame_after(0.1, vec![Event::Text("na".into())]);
        window.frames(1);
        let texts = until(&mut window, "completion", |texts| {
            texts.iter().any(|(_, text)| text == "na") && texts.iter().any(|(_, text)| text == "text")
        });
        assert!(shows(&texts, "name"), "{texts:?}");
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
            window.view.palette.apply(screen.context());
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            screen.step(Vec::new(), |ui| view.show(ui, registry, session));
            screen.step(vec![Event::PointerMoved(field), button(field, PointerButton::Primary, true)], |ui| {
                view.show(ui, registry, session)
            });
            screen.step(vec![button(field, PointerButton::Primary, false)], |ui| view.show(ui, registry, session));
            screen.step(vec![Event::Text("id > 3 AND na".into())], |ui| view.show(ui, registry, session));
            screen.step(Vec::new(), |ui| view.show(ui, registry, session));
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-filter-completion-{name}.png")), frame.image())
                .unwrap();
        }
        window.frame_after(0.1, vec![key(egui::Key::Escape, Modifiers::NONE)]);

        driver.server.hold(true);
        window.run("db.result.filter", json!({ "condition": "name LIKE 'Customer 1%'" }));
        let texts = window.frames(1);
        assert!(shows(&texts, "Filtering…"), "an object tab shows its filter running: {texts:?}");
        driver.server.hold(false);
        until(&mut window, "the filtered rows", |texts| {
            shows(texts, "WHERE (name LIKE 'Customer 1%')") && !shows(texts, "Filtering…")
        });
        read_done(&mut window, "WHERE (name LIKE 'Customer 1%')");
        let texts = window.frames(1);
        assert!(shows(&texts, "11 rows") && !shows(&texts, "Customer 2"), "only the rows the filter keeps: {texts:?}");
        let sorted = window.run("db.result.sort", json!({ "column": 1, "direction": "desc" }));
        assert_eq!(sorted["clientSort"], true, "every filtered row is loaded: {sorted}");
        window.run("db.result.setCell", json!({ "row": 0, "column": 1, "text": "Ann Smith" }));
        let added = window.run("db.result.addRow", json!({}));
        window.run("db.result.setCell", json!({ "row": added["row"], "column": 1, "text": "New customer" }));
        window.run("db.result.deleteRows", json!({ "rows": [2] }));
        let texts = until(&mut window, "the staged edits", |texts| shows(texts, "Ann Smith") && shows(texts, "new: 1"));
        assert!(shows(&texts, "[DEFAULT]") && shows(&texts, "Preview…"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-grid-edits-{name}"));

        let save = texts.iter().find(|(_, text)| text.starts_with("Save")).map(|(at, _)| *at).expect("Save");
        click_at(&mut window, save + vec2(4.0, 4.0));
        let texts = until(&mut window, "the preview", |texts| shows(texts, "Preview Changes"));
        assert!(shows(&texts, "confirm data changes"), "the Test type confirms: {texts:?}");
        assert!(texts.iter().any(|(_, text)| text.contains("UPDATE \"public\".\"customers\"")), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-preview-{name}"));
        window.run("db.result.revert", json!({}));
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}

#[test]
fn a_read_only_grid_says_why_it_cannot_be_edited_and_a_failed_filter_shows_its_error_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, driver) = sql_window(&theme);
        driver.server.fail_reads_with(Some("nope"));
        window.run("db.dataSource.update", json!({ "id": id, "readOnly": true }));
        window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
        window.run("db.object.readData", json!({}));
        let texts = until(&mut window, "the reason", |texts| {
            shows(texts, "Customer 1") && shows(texts, "the connection is read-only")
        });
        assert!(texts.iter().any(|(_, text)| text == "Read-only"), "the lock chip: {texts:?}");
        let cell = at(&texts, "Customer 2") + vec2(4.0, 4.0);
        let press = |pressed| button(cell, PointerButton::Primary, pressed);
        window.frame_after(0.05, vec![Event::PointerMoved(cell), press(true)]);
        window.frame_after(0.05, vec![press(false)]);
        window.frame_after(0.05, vec![press(true)]);
        window.frame_after(0.05, vec![press(false)]);
        window.frame_after(0.05, vec![Event::Text("x".into())]);
        let texts = window.frames(1);
        assert!(!shows(&texts, "Customer 2x") && shows(&texts, "Customer 2"), "nothing is edited: {texts:?}");
        let state = window.run("db.result.editState", json!({}));
        assert_eq!((state["reason"].as_str(), state["dirty"].as_bool()), (Some("connection"), Some(false)));
        snapshot_png(&mut window, &snapshots, &format!("database-grid-read-only-{name}"));

        window.run("db.result.filter", json!({ "condition": "nope = 1" }));
        let texts = until(&mut window, "the filter's error", |texts| {
            shows(texts, "The rows could not be read with this filter")
        });
        assert!(!shows(&texts, "Customer 3"), "the rows before are not shown as the filter's: {texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-grid-filter-failed-{name}"));
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}

#[test]
fn a_console_completes_from_its_snapshot_in_a_popup_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, _) = sql_window(&theme);
        window.run("db.object.get", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
        let console = window.run("db.console.new", json!({ "text": "SELECT c FROM customers c WHERE c.id > 3" }));
        window.frames(2);
        window.run("editor.selection.set", json!({ "selections": [{ "head": "SELECT c".len() }] }));
        window.frame_after(0.1, vec![Event::Text(".".into())]);
        let texts = until(&mut window, "the completion popup", |texts| {
            texts.iter().any(|(_, text)| text == "name") && texts.iter().any(|(_, text)| text == "jsonb")
        });
        assert!(shows(&texts, "created") && shows(&texts, "timestamp with time zone"), "every column: {texts:?}");
        assert!(texts.iter().any(|(_, text)| text == "c"), "the column's mark: {texts:?}");
        if let Some(dir) = &snapshots {
            let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
            let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
            let inside = Pos2::new(630.0, 127.0);
            screen.step(Vec::new(), |ui| view.show(ui, registry, session));
            screen.step(vec![Event::PointerMoved(inside), button(inside, PointerButton::Primary, true)], |ui| {
                view.show(ui, registry, session)
            });
            screen.step(vec![button(inside, PointerButton::Primary, false)], |ui| view.show(ui, registry, session));
            registry
                .dispatch(session, "editor.selection.set", json!({ "selections": [{ "anchor": 8, "head": 9 }] }))
                .unwrap();
            screen.step(vec![Event::Text(".".into())], |ui| view.show(ui, registry, session));
            screen.step(Vec::new(), |ui| view.show(ui, registry, session));
            let frame = screen.render(|ui| view.show(ui, registry, session));
            crate::offscreen::write_png(&dir.join(format!("database-sql-completion-{name}.png")), frame.image())
                .unwrap();
        }
        window.frame_after(0.1, vec![Event::Text("na".into())]);
        window.frames(1);
        window.frame_after(0.1, vec![key(egui::Key::Enter, Modifiers::NONE)]);
        window.frames(1);
        let path = PathBuf::from(console["path"].as_str().unwrap());
        let text = window
            .session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .map(|document| document.text().to_owned());
        assert_eq!(text.as_deref(), Some("SELECT c.name FROM customers c WHERE c.id > 3"), "↩ accepts");

        window.run("editor.documentation.show", json!({ "offset": "SELECT c.name FROM cus".len() }));
        let texts = until(&mut window, "the documentation", |texts| shows(texts, "People who buy"));
        assert!(shows(&texts, "public.customers") && shows(&texts, "Full name"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-sql-documentation-{name}"));
        window.frame_after(0.1, vec![key(egui::Key::Escape, Modifiers::NONE)]);
    }
}

#[test]
fn the_export_dialog_writes_a_result_lists_a_schemas_tables_and_shows_progress_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, driver) = sql_window(&theme);
        let out = window.dir.path().join("exports");
        std::fs::create_dir(&out).unwrap();
        window.run("db.console.new", json!({ "text": "SELECT * FROM public.customers" }));
        window.run("db.console.chooseConnection", json!({ "connection": id }));
        window.frames(2);
        window.run("db.console.execute", json!({}));
        let texts = until(&mut window, "the rows", |texts| shows(texts, "Customer 1") && shows(texts, "Export…"));
        click_at(&mut window, at(&texts, "Export…") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the dialog", |texts| shows(texts, "Export Data"));
        assert_eq!(window.view.window_state(&window.session)["popups"], json!(["exportData"]));
        assert!(shows(&texts, "SQL INSERT") && shows(&texts, "Escape formulas") && shows(&texts, "UTF-8"), "{texts:?}");
        snapshot_variants(&mut window, &snapshots, &format!("database-export-{name}"));
        window.view.export_dialog.as_mut().unwrap().set_folder(out.to_str().unwrap());
        let call = window.view.export_dialog.as_ref().unwrap().start_call(false);
        assert_eq!(call["source"]["kind"], "result", "{call}");
        assert_eq!(call["settings"]["format"], "csv", "{call}");
        let export = texts.iter().rev().find(|(_, text)| text == "Export").map(|(at, _)| *at).expect("Export");
        click_at(&mut window, export + vec2(6.0, 6.0));
        until(&mut window, "the export ended", |_| std::fs::read_dir(&out).unwrap().count() == 1);
        let texts = until(&mut window, "the dialog closed", |texts| !shows(texts, "Export Data"));
        assert!(window.view.export_dialog.is_none(), "{texts:?}");
        let file = std::fs::read_dir(&out).unwrap().next().unwrap().unwrap().path();
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(file.file_name().unwrap().to_string_lossy().starts_with("customers_"), "{file:?}");
        assert_eq!(written.lines().count(), 26, "{written}");
        assert!(written.starts_with("id,name,payload,picture,tags,created\n1,Customer 1,"), "{written}");

        let result = call["source"]["id"].clone();
        window.run("ui.notifications.clear", json!({}));
        window.frames(2);
        super::super::export_dialog::request(
            &mut window.session,
            json!({ "source": { "kind": "result", "id": result }, "selection": { "rows": [0, 2], "columns": [0, 1] } }),
        );
        let wanted = "Selected cells only (3 rows × 2 columns)";
        let texts = until(&mut window, "the selection", |texts| shows(texts, wanted));
        click_at(&mut window, at(&texts, wanted) - vec2(10.0, -6.0));
        let texts = until(&mut window, "selected only", |texts| {
            shows(texts, "The selected cells as they are shown, staged changes included")
        });
        snapshot_variants(&mut window, &snapshots, &format!("database-export-cells-{name}"));
        let call = window.view.export_dialog.as_ref().unwrap().start_call(false);
        assert_eq!(
            call["source"],
            json!({ "kind": "cells", "id": result, "rows": [0, 2], "columns": [0, 1] }),
            "{call}"
        );
        let cells_out = window.dir.path().join("cells");
        std::fs::create_dir(&cells_out).unwrap();
        window.view.export_dialog.as_mut().unwrap().set_folder(cells_out.to_str().unwrap());
        click_at(&mut window, last_at(&texts, "Export") + vec2(6.0, 6.0));
        until(&mut window, "the cells were written", |_| std::fs::read_dir(&cells_out).unwrap().count() == 1);
        assert!(window.view.export_dialog.is_none(), "a cells export closes the dialog");
        let cells = std::fs::read_dir(&cells_out).unwrap().next().unwrap().unwrap().path();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::fs::read_to_string(&cells).unwrap_or_default().lines().count() < 4
            && std::time::Instant::now() < deadline
        {
            window.frames(1);
        }
        let written = std::fs::read_to_string(&cells).unwrap();
        assert_eq!(written, "id,name\n1,Customer 1\n2,Customer 2\n3,Customer 3\n", "{written}");
        let remembered = window.run("db.export.settings", json!({}));
        assert_eq!(remembered["folder"].as_str(), cells_out.to_str(), "the last folder is remembered: {remembered}");

        super::super::export_dialog::request(
            &mut window.session,
            json!({ "connection": id, "path": ["shop", "public"] }),
        );
        let texts = until(&mut window, "the tables", |texts| shows(texts, "Tables of public, each to its own file"));
        assert!(shows(&texts, "customers"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-export-tables-{name}"));
        window.view.export_dialog = None;

        driver.server.hold(true);
        super::super::export_dialog::request(
            &mut window.session,
            json!({ "source": { "kind": "query", "connection": id, "sql": "SELECT * FROM public.customers" } }),
        );
        let texts = until(&mut window, "the query dialog", |texts| shows(texts, "Export Data"));
        window.view.export_dialog.as_mut().unwrap().set_folder(out.to_str().unwrap());
        let row = at(&texts, "Cancel").y;
        let export =
            texts.iter().find(|(at, text)| text == "Export" && at.y == row).map(|(at, _)| *at).expect("Export");
        click_at(&mut window, export + vec2(6.0, 6.0));
        let texts = until(&mut window, "the progress", |texts| shows(texts, "Cancel Export"));
        assert!(shows(&texts, "Exporting query…") && shows(&texts, "Run in Background"), "{texts:?}");
        assert!(window.view.export_dialog.as_ref().unwrap().running().is_some());
        snapshot_png(&mut window, &snapshots, &format!("database-export-progress-{name}"));
        click_at(&mut window, at(&texts, "Cancel Export") + vec2(6.0, 6.0));
        until(&mut window, "the cancelled export", |texts| !shows(texts, "Cancel Export"));
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1, "the cancelled export left nothing");
        driver.server.hold(false);
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}

#[test]
fn the_csv_import_dialog_walks_its_steps_shows_the_sample_and_the_progress_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, driver) = sql_window(&theme);
        let file = window.dir.path().join("customers.csv");
        let mut text = String::from("id,name,created,note\n");
        for at in 1..=40 {
            text.push_str(&format!("{at},Customer {at},2026-10-27 09:{:02}:00,\"a, quoted\"\n", at % 60));
        }
        std::fs::write(&file, text).unwrap();
        super::super::csv_import_dialog::request(
            &mut window.session,
            json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }),
        );
        let texts = until(&mut window, "the dialog", |texts| shows(texts, "Import from CSV"));
        snapshot_variants(&mut window, &snapshots, &format!("database-import-file-{name}"));
        assert_eq!(window.view.window_state(&window.session)["popups"], json!(["importData"]));
        assert!(shows(&texts, "public.customers") && shows(&texts, "1. File"), "{texts:?}");
        window.view.csv_import_dialog.as_mut().unwrap().set_file(file.to_str().unwrap());
        click_at(&mut window, last_at(&texts, "Next") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the sample", |texts| shows(texts, "Customer 3"));
        assert_eq!(
            window.view.csv_import_dialog.as_ref().unwrap().step(),
            super::super::csv_import_dialog::Step::Format
        );
        assert!(shows(&texts, "The first 40 rows, 4 columns") && shows(&texts, "Date and time"), "{texts:?}");
        snapshot_variants(&mut window, &snapshots, &format!("database-import-format-{name}"));
        click_at(&mut window, last_at(&texts, "Next") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the mapping", |texts| shows(texts, "Table column"));
        assert!(shows(&texts, "Skip") && shows(&texts, "created"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-import-mapping-{name}"));
        click_at(&mut window, last_at(&texts, "Next") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the settings", |texts| shows(texts, "Commit every:"));
        assert!(shows(&texts, "Before loading:"), "{texts:?}");
        snapshot_variants(&mut window, &snapshots, &format!("database-import-settings-{name}"));
        click_at(&mut window, last_at(&texts, "Next") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the check", |texts| shows(texts, "Loading:"));
        assert!(shows(&texts, "id, name, created") && shows(&texts, "one transaction"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-import-confirm-{name}"));
        let call = window.view.csv_import_dialog.as_ref().unwrap().start_call(None);
        assert_eq!(
            call["columns"],
            json!([
                { "source": 0, "target": "id", "kind": "integer" },
                { "source": 1, "target": "name", "kind": "text" },
                { "source": 2, "target": "created", "kind": "timestamp" },
            ]),
            "{call}"
        );
        driver.server.hold(true);
        click_at(&mut window, last_at(&texts, "Import") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the progress", |texts| shows(texts, "Cancel Import"));
        assert!(
            shows(&texts, "Importing customers.csv into customers") && shows(&texts, "Run in Background"),
            "{texts:?}"
        );
        assert!(window.view.csv_import_dialog.as_ref().unwrap().running().is_some());
        snapshot_png(&mut window, &snapshots, &format!("database-import-progress-{name}"));
        click_at(&mut window, at(&texts, "Cancel Import") + vec2(6.0, 6.0));
        driver.server.hold(false);
        until(&mut window, "the cancelled import", |texts| !shows(texts, "Cancel Import"));
        assert!(window.view.csv_import_dialog.is_none());
        assert!(!driver.server.executed().iter().any(|sql| sql.starts_with("COMMIT")), "nothing was committed");
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}

#[test]
fn the_transfer_dialog_maps_tables_onto_another_connection_and_shows_the_progress_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, driver) = sql_window(&theme);
        window.run(
            "db.dataSource.add",
            json!({ "name": "Reports", "driver": zen_db::memory::SQL, "readOnly": true, "password": "pw" }),
        );
        let added = window.run(
            "db.dataSource.add",
            json!({ "name": "Archive", "driver": zen_db::memory::SQL, "type": "test", "password": "pw" }),
        );
        let target = added["id"].as_str().unwrap().to_owned();
        let mut snapshot = customers_snapshot(&target, zen_db::memory::SQL);
        snapshot.catalogs[0].schemas[0].objects[0].oid = zen_db::memory::TABLE;
        let file = window.dir.path().join(format!("data/caches/databases/global/{target}.snapshot"));
        zen_db::schema::write_snapshot(&file, &snapshot).unwrap();
        super::super::transfer_dialog::request(
            &mut window.session,
            json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }),
        );
        let texts = until(&mut window, "the dialog", |texts| shows(texts, "Transfer Data"));
        snapshot_variants(&mut window, &snapshots, &format!("database-transfer-target-{name}"));
        assert_eq!(window.view.window_state(&window.session)["popups"], json!(["transferData"]));
        assert!(shows(&texts, "1. Target") && shows(&texts, "Shop") && shows(&texts, "Archive"), "{texts:?}");
        click_at(&mut window, last_at(&texts, "Next") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the mapping", |texts| shows(texts, "Target column"));
        assert_eq!(window.view.transfer_dialog.as_ref().unwrap().step(), super::super::transfer_dialog::Step::Mapping);
        assert!(shows(&texts, "existing") && shows(&texts, "public.customers"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-transfer-mapping-{name}"));
        click_at(&mut window, last_at(&texts, "Next") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the settings", |texts| shows(texts, "Commit every:"));
        snapshot_variants(&mut window, &snapshots, &format!("database-transfer-settings-{name}"));
        click_at(&mut window, last_at(&texts, "Next") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the check", |texts| shows(texts, "Loading:"));
        assert!(shows(&texts, "one transaction a table"), "{texts:?}");
        snapshot_variants(&mut window, &snapshots, &format!("database-transfer-confirm-{name}"));
        let call = window.view.transfer_dialog.as_ref().unwrap().start_call(None);
        assert_eq!((call["source"].as_str(), call["target"].as_str()), (Some(id.as_str()), Some(target.as_str())));
        assert_eq!(call["schema"], json!(["shop", "public"]), "{call}");
        assert_eq!(call["tables"][0]["name"], "customers", "{call}");
        assert!(call["tables"][0]["columns"].as_array().is_some_and(|columns| !columns.is_empty()), "{call}");
        driver.server.hold(true);
        click_at(&mut window, last_at(&texts, "Transfer") + vec2(6.0, 6.0));
        let texts = until(&mut window, "the progress", |texts| shows(texts, "Cancel Transfer"));
        assert!(shows(&texts, "Run in Background"), "{texts:?}");
        assert!(window.view.transfer_dialog.as_ref().unwrap().running().is_some());
        snapshot_png(&mut window, &snapshots, &format!("database-transfer-progress-{name}"));
        click_at(&mut window, at(&texts, "Cancel Transfer") + vec2(6.0, 6.0));
        driver.server.hold(false);
        until(&mut window, "the cancelled transfer", |texts| !shows(texts, "Cancel Transfer"));
        assert!(window.view.transfer_dialog.is_none());
        assert!(!driver.server.executed().iter().any(|sql| sql.starts_with("COMMIT")), "nothing was committed");
        for connection in [&id, &target] {
            window.run("db.connection.disconnect", json!({ "connection": connection, "transactions": "rollback" }));
        }
    }
}

fn last_at(texts: &[(Pos2, String)], wanted: &str) -> Pos2 {
    texts.iter().rev().find(|(_, text)| text == wanted).map(|(at, _)| *at).unwrap_or_else(|| panic!("no {wanted:?}"))
}

fn close_asks(window: &mut Window, tab: &str) -> Vec<(Pos2, String)> {
    let params = json!({ "workspace": zen_workbench::database::WORKSPACE, "tab": tab });
    let error = window.registry.dispatch(&mut window.session, "workspace.tab.close", params).unwrap_err();
    assert!(crate::ide::transaction_dialog::ask(&mut window.session, &error), "{error}");
    until(window, "the question", |texts| shows(texts, "Unsaved data changes"))
}

fn open_with_edit(window: &mut Window, id: &str) -> String {
    let opened =
        window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
    window.run("db.object.readData", json!({}));
    until(window, "the rows were read", |texts| shows(texts, "Customer 1") && shows(texts, "WHERE"));
    let state = window.run("db.object.state", json!({}));
    assert_eq!(state["data"]["reading"], false, "{state}");
    window.run("db.result.setCell", json!({ "row": 0, "column": 1, "text": "Ann Smith" }));
    opened["tab"]["id"].as_str().unwrap().to_owned()
}

fn tab_open(window: &mut Window, tab: &str) -> bool {
    let state = window.run("workspace.host.state", json!({ "id": zen_workbench::database::WORKSPACE }));
    state["tabs"].as_array().unwrap().iter().any(|known| known["id"] == tab)
}

#[test]
fn closing_a_tab_with_staged_edits_asks_save_discard_or_cancel() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, driver) = sql_window(&theme);
        let tab = open_with_edit(&mut window, &id);
        let texts = close_asks(&mut window, &tab);
        assert!(
            shows(&texts, "This tab has data changes that are not saved. Save them before closing it?"),
            "{texts:?}"
        );
        snapshot_variants(&mut window, &snapshots, &format!("database-staged-question-{name}"));
        click_at(&mut window, last_at(&texts, "Cancel") + vec2(4.0, 4.0));
        let texts = window.frames(2);
        assert!(!shows(&texts, "Unsaved data changes"), "Cancel closes the question: {texts:?}");
        assert!(tab_open(&mut window, &tab), "Cancel keeps the tab");
        assert_eq!(window.run("db.result.editState", json!({}))["dirty"], true, "and the changes");

        let texts = close_asks(&mut window, &tab);
        click_at(&mut window, last_at(&texts, "Discard") + vec2(4.0, 4.0));
        window.frames(2);
        assert!(!tab_open(&mut window, &tab), "Discard drops the changes and closes the tab");
        assert!(!driver.server.bound().iter().any(|(sql, _)| sql.starts_with("UPDATE")), "nothing was saved");

        let tab = open_with_edit(&mut window, &id);
        let texts = close_asks(&mut window, &tab);
        click_at(&mut window, last_at(&texts, "Save") + vec2(4.0, 4.0));
        let texts = until(&mut window, "the preview to confirm", |texts| shows(texts, "confirm data changes"));
        assert!(texts.iter().any(|(_, text)| text.contains("UPDATE \"public\".\"customers\"")), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-staged-confirm-{name}"));
        click_at(&mut window, last_at(&texts, "Save") + vec2(4.0, 4.0));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while tab_open(&mut window, &tab) {
            assert!(std::time::Instant::now() < deadline, "the tab closes once the changes are saved");
            window.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(driver.server.bound().iter().any(|(sql, _)| sql.starts_with("UPDATE")), "Save saved first");
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}

fn customers_shown(window: &mut Window, id: &str) -> Vec<(Pos2, String)> {
    window.run("db.object.open", json!({ "connection": id, "path": ["shop", "public", "tables", "customers"] }));
    window.run("db.object.readData", json!({}));
    until(window, "the rows were read", |texts| shows(texts, "Customer 2") && shows(texts, "WHERE"));
    read_done(window, "customers");
    window.frames(2)
}

fn clipboard_of(window: &mut Window) -> String {
    zen_workbench::file_clipboard::clipboard_text(&mut window.session)
}

fn copy_as_menu_png(
    window: &mut Window,
    dir: &Option<PathBuf>,
    name: &str,
    cell: Pos2,
    (advanced, copy_as): (Pos2, Pos2),
) {
    let Some(dir) = dir else { return };
    let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
    window.view.palette.apply(screen.context());
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    screen.step(vec![Event::PointerMoved(cell), button(cell, PointerButton::Secondary, true)], |ui| {
        view.show(ui, registry, session)
    });
    screen.step(vec![button(cell, PointerButton::Secondary, false)], |ui| view.show(ui, registry, session));
    screen.step(vec![Event::PointerMoved(advanced + vec2(6.0, 6.0))], |ui| view.show(ui, registry, session));
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    screen.step(vec![Event::PointerMoved(copy_as + vec2(6.0, 6.0))], |ui| view.show(ui, registry, session));
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    let frame = screen.render(|ui| view.show(ui, registry, session));
    crate::offscreen::write_png(&dir.join(format!("{name}.png")), frame.image()).unwrap();
}

#[test]
fn a_data_view_copies_as_formats_and_pastes_from_spreadsheets_with_a_preview_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, driver) = sql_window(&theme);
        let texts = customers_shown(&mut window, &id);
        let cell = at(&texts, "Customer 2") + vec2(4.0, 4.0);
        click_at(&mut window, cell);
        window.frame_after(0.05, vec![Event::PointerMoved(cell), button(cell, PointerButton::Secondary, true)]);
        window.frame_after(0.05, vec![button(cell, PointerButton::Secondary, false)]);
        let texts = window.frame_after(0.05, Vec::new());
        let menu: Vec<&String> =
            texts.iter().map(|(_, text)| text).filter(|text| text.starts_with(['C', 'A', 'P'])).collect();
        for group in ["Edit", "Filter", "Order", "Navigate", "View/Format", "Panels", "Layout", "Advanced Paste"] {
            assert!(texts.iter().any(|(_, text)| text == group), "the {group} group: {menu:?}");
        }
        let advanced = at(&texts, "Advanced Copy");
        window.frame_after(0.05, vec![Event::PointerMoved(advanced + vec2(6.0, 6.0))]);
        let texts = window.frames(2);
        assert!(shows(&texts, "Copy Column Names") && shows(&texts, "Advanced Copy…"), "{texts:?}");
        let copy_as = at(&texts, "Copy As");
        window.frame_after(0.05, vec![Event::PointerMoved(copy_as + vec2(6.0, 6.0))]);
        let texts = window.frames(2);
        assert!(shows(&texts, "SQL INSERT") && shows(&texts, "TypeScript") && shows(&texts, "SQL IN (…)"), "{texts:?}");
        copy_as_menu_png(&mut window, &snapshots, &format!("database-copy-as-{name}"), cell, (advanced, copy_as));
        let insert = at(&texts, "SQL INSERT") + vec2(4.0, 4.0);
        window.frame_after(0.05, vec![Event::PointerMoved(insert)]);
        click_at(&mut window, insert);
        let copied = clipboard_of(&mut window);
        assert!(
            copied.starts_with("INSERT INTO \"public\".\"customers\" (\"name\") VALUES\n\t('Customer 2');"),
            "{copied}"
        );

        click_at(&mut window, cell);
        window.frame_after(0.05, vec![Event::Copy]);
        assert_eq!(clipboard_of(&mut window), "Customer 2", "⌘C copies the value");

        window.frame_after(0.05, vec![key(egui::Key::C, Modifiers::COMMAND | Modifiers::SHIFT)]);
        let texts = until(&mut window, "Advanced Copy", |texts| shows(texts, "Quote values:"));
        assert!(shows(&texts, "Row numbers"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-advanced-copy-{name}"));
        let copy = texts.iter().rev().find(|(_, text)| text == "Copy").map(|(at, _)| *at).expect("the dialog's Copy");
        click_at(&mut window, copy + vec2(4.0, 4.0));
        window.frames(1);
        assert_eq!(clipboard_of(&mut window), "name\nCustomer 2", "the remembered options start with the header");

        click_at(&mut window, cell);
        window.frame_after(0.05, vec![Event::Paste("name\tpayload\nAnn\tnot json\nBob\t{}\n".to_owned())]);
        let texts = until(&mut window, "the paste preview", |texts| shows(texts, "Advanced Paste"));
        assert!(shows(&texts, "One value does not fit"), "{texts:?}");
        assert!(shows(&texts, "Line 1, payload: Not valid JSON."), "{texts:?}");
        assert!(shows(&texts, "Rows: 2, columns: 2, delimiter: Tab"), "{texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-paste-preview-{name}"));
        click_at(&mut window, at(&texts, "Paste Valid Cells") + vec2(4.0, 4.0));
        let texts = until(&mut window, "the pasted cells", |texts| shows(texts, "Pasted cells: 3"));
        assert!(shows(&texts, "Ann") && shows(&texts, "Bob"), "{texts:?}");
        let state = window.run("db.result.editState", json!({}));
        assert_eq!(state["pending"]["updated"], 2, "{state}");
        assert!(!driver.server.bound().iter().any(|(sql, _)| sql.starts_with("UPDATE")), "nothing goes to the server");
        window.run("db.result.revert", json!({}));
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}

fn steps_png(window: &mut Window, dir: &Option<PathBuf>, name: &str, steps: Vec<Vec<Event>>) {
    let Some(dir) = dir else { return };
    let mut screen = crate::offscreen::Offscreen::new(vec2(1400.0, 900.0), 1.5);
    window.view.palette.apply(screen.context());
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    for events in steps {
        screen.step(events, |ui| view.show(ui, registry, session));
    }
    screen.step(Vec::new(), |ui| view.show(ui, registry, session));
    let frame = screen.render(|ui| view.show(ui, registry, session));
    crate::offscreen::write_png(&dir.join(format!("{name}.png")), frame.image()).unwrap();
}

#[test]
fn the_data_grid_has_its_value_panel_menus_and_go_to_row_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, id, _) = sql_window(&theme);
        let texts = customers_shown(&mut window, &id);
        assert!(shows(&texts, "SELECT * FROM \"public\".\"customers\"") && !shows(&texts, "ORDER BY"), "{texts:?}");
        assert!(shows(&texts, "Calc"), "the value panel is open at first: {texts:?}");
        assert!(texts.iter().any(|(_, text)| text == "200"), "the fetch size: {texts:?}");
        snapshot_png(&mut window, &snapshots, &format!("database-grid-{name}"));

        let cell = at(&texts, "Customer 2") + vec2(4.0, 4.0);
        click_at(&mut window, cell);
        window.frame_after(0.05, vec![Event::PointerMoved(cell), button(cell, PointerButton::Secondary, true)]);
        window.frame_after(0.05, vec![button(cell, PointerButton::Secondary, false)]);
        let texts = window.frame_after(0.05, Vec::new());
        let navigate = at(&texts, "Navigate");
        steps_png(
            &mut window,
            &snapshots,
            &format!("database-grid-menu-{name}"),
            vec![
                vec![Event::PointerMoved(cell), button(cell, PointerButton::Secondary, true)],
                vec![button(cell, PointerButton::Secondary, false)],
                vec![Event::PointerMoved(navigate + vec2(6.0, 6.0))],
            ],
        );
        window.frame_after(0.05, vec![key(egui::Key::Escape, Modifiers::NONE)]);
        click_at(&mut window, cell);
        click_at(&mut window, cell);
        window.frame_after(0.05, vec![key(egui::Key::G, Modifiers::CTRL)]);
        until(&mut window, "Go to Row", |texts| shows(texts, "Row number"));
        snapshot_png(&mut window, &snapshots, &format!("database-go-to-row-{name}"));
        window.frame_after(0.05, vec![Event::Text("20".into())]);
        window.frame_after(0.05, vec![key(egui::Key::Enter, Modifiers::NONE)]);
        let texts = until(&mut window, "row 20 selected", |texts| {
            !shows(texts, "Row number") && texts.iter().filter(|(_, text)| text == "Customer 20").count() >= 2
        });
        assert!(!texts.is_empty());
        let console = window.run("db.console.new", json!({ "connection": id, "text": "SELECT * FROM customers;\n" }));
        window.run("db.console.executeScript", json!({ "path": console["path"] }));
        let texts = until(&mut window, "the result tab", |texts| texts.iter().any(|(_, text)| text == "customers"));
        assert!(!texts.iter().any(|(_, text)| text == "customers 1"), "{texts:?}");
        window.run("db.connection.disconnect", json!({ "connection": id, "transactions": "rollback" }));
    }
}
