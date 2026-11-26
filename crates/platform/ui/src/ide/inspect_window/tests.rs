use egui::{Event, PointerButton, Pos2};
use zen_workbench::inspect_code::{InspectionNode, SeverityNode, TreeFile, TreeFolder};
use zen_workbench::language::Problem;

use super::*;

fn problem(start: usize, line: u32, message: &str, severity: Severity) -> Problem {
    Problem {
        start,
        end: start + 1,
        line,
        column: 1,
        severity,
        message: message.to_owned(),
        source: Some("lint".to_owned()),
        code: Some("unused".to_owned()),
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        deprecated: false,
        unused: false,
    }
}

fn item(id: u64, relative: &str, message: &str) -> ResultItem {
    ResultItem {
        id,
        path: PathBuf::from("/p").join(relative),
        relative: relative.to_owned(),
        inspection: "lint/unused".to_owned(),
        excluded: false,
        problem: problem(id as usize * 10, id as u32, message, Severity::Warning),
    }
}

fn results() -> InspectionResults {
    InspectionResults {
        run: 1,
        title: "Whole project".to_owned(),
        request: InspectRequest::default(),
        files: 2,
        items: vec![
            item(1, "a.go", "x is not used"),
            item(2, "a.go", "y is not used"),
            item(3, "sub/b.go", "z is not used"),
        ],
        unchecked: Vec::new(),
        note: None,
    }
}

fn tree(by_directory: bool) -> ResultsTree {
    let file = |relative: &str, items: Vec<u64>| TreeFile {
        path: PathBuf::from("/p").join(relative),
        relative: relative.to_owned(),
        items,
    };
    let folders = if by_directory {
        vec![
            TreeFolder { directory: Some(String::new()), files: vec![file("a.go", vec![1, 2])] },
            TreeFolder { directory: Some("sub".to_owned()), files: vec![file("sub/b.go", vec![3])] },
        ]
    } else {
        vec![TreeFolder { directory: None, files: vec![file("a.go", vec![1, 2]), file("sub/b.go", vec![3])] }]
    };
    ResultsTree {
        run: 1,
        count: 3,
        severities: vec![SeverityNode {
            severity: Severity::Warning,
            count: 3,
            inspections: vec![InspectionNode {
                id: "lint/unused".to_owned(),
                name: "unused".to_owned(),
                group: "lint".to_owned(),
                description: "Things nothing uses".to_owned(),
                count: 3,
                folders,
            }],
        }],
    }
}

struct Harness {
    ctx: egui::Context,
    window: InspectWindow,
    actions: Vec<InspectAction>,
    time: f64,
}

impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut window = InspectWindow { tab: Some(1), ..InspectWindow::default() };
        window.results = Some(results());
        window.tree = Some(tree(false));
        window.rebuild();
        let mut harness = Self { ctx, window, actions: Vec::new(), time: 1.0 };
        harness.frame(Vec::new());
        harness
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.02;
        let input = egui::RawInput {
            time: Some(self.time),
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 500.0))),
            ..Default::default()
        };
        let mut output = self.ctx.run_ui(input, |ui| {
            self.actions.extend(self.window.show(ui, &IdePalette::zen_dark()));
        });
        output.textures_delta.clear();
    }

    fn click(&mut self, at: Pos2, double: bool) {
        self.time += 1.0;
        self.frame(vec![Event::PointerMoved(at)]);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        for _ in 0..if double { 2 } else { 1 } {
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }
        self.frame(Vec::new());
    }

    fn key(&mut self, key: Key) {
        let event = Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![event]);
    }

    fn at(&self, node: &Node) -> Pos2 {
        self.window
            .drawn
            .iter()
            .find(|(drawn, _)| drawn == node)
            .map(|(_, rect)| rect.right_center() - vec2(20.0, 0.0))
            .unwrap()
    }
}

fn labels(window: &InspectWindow) -> Vec<(usize, String)> {
    window.rows.iter().map(|row| (row.depth, row.label.clone())).collect()
}

#[test]
fn the_tree_is_severity_inspection_file_problem() {
    let harness = Harness::new();
    let rows = labels(&harness.window);
    assert_eq!(rows[1], (1, "unused".to_owned()));
    assert_eq!(rows[2], (2, "a.go".to_owned()));
    assert_eq!(rows[3], (3, "x is not used".to_owned()));
    assert_eq!(rows.len(), 7);
}

#[test]
fn group_by_directory_adds_the_folders() {
    let mut harness = Harness::new();
    harness.window.tree = Some(tree(true));
    harness.window.rebuild();
    let rows = labels(&harness.window);
    assert_eq!(rows[2], (2, "/".to_owned()));
    assert_eq!(rows[3], (3, "a.go".to_owned()));
    assert!(rows.contains(&(2, "sub".to_owned())), "{rows:?}");
}

#[test]
fn f4_enter_and_a_double_click_jump_to_the_source() {
    let mut harness = Harness::new();
    let go = InspectAction::GoTo { path: PathBuf::from("/p/a.go"), offset: 20, focus_editor: true };
    harness.click(harness.at(&Node::Item(2)), true);
    assert_eq!(harness.actions.last(), Some(&go), "a double click");
    harness.actions.clear();
    harness.key(Key::F4);
    assert_eq!(harness.actions.last(), Some(&go), "F4");
    harness.actions.clear();
    harness.key(Key::Enter);
    assert_eq!(harness.actions, [go], "Enter");
}

#[test]
fn keys_walk_the_tree_fold_groups_and_exclude() {
    let mut harness = Harness::new();
    harness.click(harness.at(&Node::Item(1)), false);
    harness.key(Key::ArrowDown);
    assert_eq!(harness.window.selected, Some(Node::Item(2)));
    harness.key(Key::Delete);
    assert_eq!(harness.actions, [InspectAction::Exclude { run: 1, items: vec![2], excluded: true }]);
    let file = Node::File(Severity::Warning, "lint/unused".to_owned(), "a.go".to_owned());
    harness.click(harness.at(&file), false);
    harness.key(Key::ArrowLeft);
    assert!(harness.window.rows.iter().all(|row| row.node != Node::Item(1)), "the file folds");
    harness.key(Key::ArrowRight);
    assert!(harness.window.rows.iter().any(|row| row.node == Node::Item(1)));
}

#[test]
fn a_file_offers_fix_all_in_file_and_an_item_its_fixes() {
    let mut harness = Harness::new();
    harness.window.selected = Some(Node::File(Severity::Warning, "lint/unused".to_owned(), "a.go".to_owned()));
    let buttons = harness.window.buttons();
    assert_eq!(buttons[0].0, t!("ide-inspect-fix-all-file"));
    assert_eq!(buttons[0].1, InspectAction::Fix { run: 1, items: vec![1, 2], fix: None });
    harness.window.selected = Some(Node::Item(1));
    assert!(harness.window.buttons().is_empty(), "not known yet");
    let list = ItemFixList {
        pending: false,
        fixes: vec![
            zen_workbench::inspect_code::ItemFix { id: "f".to_owned(), title: "Remove x".to_owned(), action: None },
            zen_workbench::inspect_code::ItemFix {
                id: "s".to_owned(),
                title: "Open settings".to_owned(),
                action: Some(json!({ "action": "view.settings.open" })),
            },
        ],
    };
    harness.window.fixes = Some((1, list));
    let buttons = harness.window.buttons();
    assert_eq!(buttons[0].1, InspectAction::Fix { run: 1, items: vec![1], fix: Some("f".to_owned()) });
    assert_eq!(buttons[1].1, InspectAction::RunWindowAction(json!({ "action": "view.settings.open" })));
}

#[test]
fn files_no_server_checked_are_counted_above_the_tree() {
    let mut harness = Harness::new();
    let mut results = results();
    results.unchecked.push(zen_workbench::inspect_code::UncheckedFile {
        relative: "x.go".to_owned(),
        reason: "no answer".to_owned(),
    });
    harness.window.results = Some(results);
    assert!(harness.window.unchecked_notice().is_some());
}

#[test]
fn the_dialog_asks_for_what_was_chosen_and_needs_a_directory() {
    let mut dialog = InspectDialog::default();
    dialog.options.scope = ScopeKind::Directory;
    assert!(!dialog.ready());
    dialog.options.directory = " src ".to_owned();
    dialog.file_mask = "*.go".to_owned();
    assert!(dialog.ready());
    let request = dialog.request();
    assert_eq!((request.directory.as_str(), request.file_mask.as_str()), ("src", "*.go"));
    assert_eq!(request.include_tests, Some(true));
    assert_eq!(request.inspection, None);
    dialog.by_name =
        Some(ByName { chosen: Some(Named { id: "printf".to_owned(), ..Named::default() }), ..ByName::default() });
    assert_eq!(dialog.request().inspection.as_deref(), Some("printf"));
    dialog.options.scope = ScopeKind::Files;
    assert!(!dialog.ready(), "no files chosen");
}

#[test]
fn run_by_name_filters_by_name_group_and_description() {
    let named = |title: &str, group: &str, description: &str| Named {
        id: title.to_owned(),
        title: title.to_owned(),
        group: group.to_owned(),
        description: description.to_owned(),
    };
    let mut by_name = ByName {
        list: vec![named("printf", "gopls", "Format strings"), named("newerVersion", "go.mod", "A newer one")],
        ..ByName::default()
    };
    by_name.query = "FORMAT".to_owned();
    assert_eq!(by_name.shown().len(), 1);
    by_name.query = "go.mod".to_owned();
    assert_eq!(by_name.shown()[0].id, "newerVersion");
}

fn product() -> crate::ide::IdeProduct {
    crate::ide::IdeProduct {
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

struct Lines;

impl zen_project::inspect::Inspector for Lines {
    fn id(&self) -> &str {
        "lines"
    }

    fn handles(&self, _: &str) -> bool {
        true
    }

    fn inspect(&self, context: &zen_project::inspect::InspectContext<'_>) -> Vec<zen_project::inspect::Finding> {
        let found = |at: usize| zen_project::inspect::Finding {
            start: at,
            end: at + 4,
            severity: zen_project::inspect::FindingSeverity::Warning,
            message: "Left to do".to_owned(),
            note: None,
            rows: Vec::new(),
            details: None,
            fixes: Vec::new(),
            source: "lines".to_owned(),
            code: "todo".to_owned(),
            deprecated: false,
            unused: false,
        };
        context.text.match_indices("TODO").map(|(at, _)| found(at)).collect()
    }

    fn apply(
        &self,
        _: &zen_project::inspect::InspectContext<'_>,
        _: &[zen_project::inspect::InspectionFix],
    ) -> Result<Box<dyn zen_project::inspect::FixJob>, String> {
        Err("no fixes".to_owned())
    }
}

#[test]
fn the_dialog_runs_and_the_results_open_in_a_tab_of_the_problems_window() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "TODO one\nTODO two\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    session
        .state_mut::<zen_workbench::inspections::Inspections>()
        .set_inspectors(vec![std::sync::Arc::new(Lines)], None);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while registry.dispatch(&mut session, "search.scope.files", json!({})).unwrap()["total"] == 0 {
        assert!(std::time::Instant::now() < deadline, "the index");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut view = IdeView::new(std::sync::Arc::new(product()), crate::plugins::Plugins::new(None));
    let mut screen = crate::offscreen::Offscreen::new(vec2(1100.0, 640.0), 1.5);
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let save = |name: &str, image: &crate::offscreen::Frame| {
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(name), image).unwrap();
        }
    };
    view.open_inspect_dialog(&registry, &mut session, false);
    assert!(view.inspect_dialog.is_some());
    for _ in 0..3 {
        screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    save("inspect-dialog.png", &image);
    let enter =
        Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    screen.frame(vec![enter], |ui| view.show(ui, &registry, &mut session));
    assert!(view.inspect_dialog.is_none(), "Enter is Analyze");
    let pending = view.inspect_window.pending.expect("the run started");
    while session.state::<InspectCode>().unwrap().runs().iter().all(|results| results.run != pending) {
        assert!(std::time::Instant::now() < deadline, "the run");
        screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    for _ in 0..4 {
        screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    assert_eq!(view.inspect_window.tab, Some(pending), "the tab opens by itself");
    assert!(view.tool_visible("problems"));
    assert_eq!(view.inspect_window.rows.len(), 5, "{:?}", labels(&view.inspect_window));
    let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    save("inspect-results.png", &image);
    view.open_inspect_dialog(&registry, &mut session, true);
    for _ in 0..3 {
        screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    save("inspect-by-name.png", &image);
    view.inspect_dialog = None;
    view.open_cleanup_dialog(&registry, &mut session);
    for _ in 0..3 {
        screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    let image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    save("code-cleanup-dialog.png", &image);
    let tab = view.inspect_window.tab;
    let enter =
        Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    screen.frame(vec![enter], |ui| view.show(ui, &registry, &mut session));
    assert!(view.inspect_dialog.is_none(), "Enter is Run");
    assert_eq!(view.inspect_window.pending, None, "a cleanup opens no results");
    while session.state::<InspectCode>().unwrap().running() {
        assert!(std::time::Instant::now() < deadline, "the cleanup");
        screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    assert_eq!(view.inspect_window.tab, tab);
    let status = registry.dispatch(&mut session, "inspect.code.status", serde_json::json!({})).unwrap();
    assert_eq!(status["cleanup"]["fixed"], 0, "no inspection here is a cleanup one: {status}");
    assert!(view.silent_cleanup(&registry, &mut session).is_err(), "nothing selected and no file open");
}

#[test]
fn a_problem_row_has_a_menu_that_says_when_it_cannot_be_suppressed() {
    use crate::accessibility::testing::find;
    use egui::accesskit::Role;
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "TODO one\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    session
        .state_mut::<zen_workbench::inspections::Inspections>()
        .set_inspectors(vec![std::sync::Arc::new(Lines)], None);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    let mut view = IdeView::new(std::sync::Arc::new(product()), crate::plugins::Plugins::new(None));
    view.project_open = false;
    view.show_tool("problems");
    let mut screen = crate::offscreen::Offscreen::new(vec2(1100.0, 640.0), 1.0);
    for _ in 0..3 {
        screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session));
    }
    let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
    let row = zen_i18n::t!("a11y-problem", severity = "warning", message = "Left to do", line = 1);
    let bounds = find(&tree, Role::ListItem, &row).expect("the row").bounds().expect("its place");
    let at = pos2(bounds.x0 as f32 + 40.0, (bounds.y0 + bounds.y1) as f32 / 2.0);
    let secondary = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    for events in [vec![Event::PointerMoved(at)], vec![secondary(true)], vec![secondary(false)], Vec::new()] {
        screen.frame(events, |ui| view.show(ui, &registry, &mut session));
    }
    let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
    let none = zen_i18n::t!("ide-problems-no-suppression");
    assert!(find(&tree, Role::Button, &none).is_some(), "the menu says so");
}
