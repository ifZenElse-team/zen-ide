use egui::{Event, PointerButton, Pos2};

use super::*;

struct Harness {
    ctx: egui::Context,
    window: HierarchyWindow,
    actions: Vec<HierarchyAction>,
    time: f64,
    drawn: Vec<String>,
}

impl Harness {
    fn new(state: HierarchyState) -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut window = HierarchyWindow::default();
        window.update(state, 1, 1);
        let mut harness = Self { ctx, window, actions: Vec::new(), time: 0.0, drawn: Vec::new() };
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        harness
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.01;
        let input = egui::RawInput {
            events,
            time: Some(self.time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(420.0, 500.0))),
            ..Default::default()
        };
        let mut output = self.ctx.run_ui(input, |ui| {
            self.actions.extend(self.window.show(ui, &IdePalette::zen_dark()));
        });
        output.textures_delta.clear();
        self.drawn = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
    }

    fn click_with(&mut self, at: Pos2, count: usize) {
        self.time += 1.0;
        self.frame(vec![Event::PointerMoved(at)]);
        for _ in 0..count {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }
    }

    fn click(&mut self, at: Pos2) {
        self.click_with(at, 1);
    }

    fn key(&mut self, key: Key) {
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
    }

    fn row(&self, node: usize) -> Rect {
        self.window.rows.iter().find(|(row, _)| row.node == node).map(|(_, rect)| *rect).unwrap()
    }

    fn button(&self, id: &str) -> Rect {
        self.window.buttons.iter().find(|(drawn, _)| *drawn == id).map(|(_, rect)| *rect).unwrap()
    }

    fn take(&mut self) -> Vec<HierarchyAction> {
        std::mem::take(&mut self.actions)
    }
}

fn node(id: usize, name: &str, parent: Option<usize>, location: &str, children: Value) -> Value {
    json!({
        "id": id,
        "name": name,
        "kind": "function",
        "detail": "example.com/shop/cart • cart.go",
        "path": format!("/p/{name}.go"),
        "range": [0, 10],
        "selection": { "path": format!("/p/{name}.go"), "start": 5 + id, "end": 9, "line": 1, "column": 6 },
        "location": location,
        "parent": parent,
        "calls": [],
        "children": children,
        "collapsed": false,
        "recursive": false,
        "pending": false,
    })
}

fn recursive(mut node: Value) -> Value {
    node["recursive"] = json!(true);
    node
}

fn call_state(single_click: bool) -> HierarchyState {
    serde_json::from_value(json!({
        "open": true,
        "tab": 0,
        "kind": "call",
        "direction": "callers",
        "scope": "all",
        "singleClick": single_click,
        "symbol": "Total",
        "supported": true,
        "roots": [0],
        "nodes": [
            node(0, "Total", None, "project", json!([1, 2])),
            node(1, "Run", Some(0), "project", Value::Null),
            node(2, "TestTotal", Some(0), "test", json!([3])),
            recursive(node(3, "TestTotal", Some(2), "test", json!([]))),
        ],
        "tabs": [
            { "tab": 0, "kind": "call", "direction": "callers", "symbol": "Total", "pinned": false },
        ],
        "active": 0,
    }))
    .unwrap()
}

#[test]
fn a_click_selects_the_chevron_expands_and_a_double_click_enter_or_f4_go_to_the_code() {
    let mut hierarchy = Harness::new(call_state(false));
    assert_eq!(hierarchy.window.selected, Some(0), "the root is selected");
    let rows: Vec<(usize, usize)> = hierarchy.window.tree.iter().map(|row| (row.node, row.depth)).collect();
    assert_eq!(rows, [(0, 0), (1, 1), (2, 1), (3, 2)]);
    hierarchy.click(hierarchy.row(1).center());
    assert_eq!(hierarchy.window.selected, Some(1));
    assert!(hierarchy.take().is_empty(), "without single-click navigation a click only selects");
    let chevron = hierarchy.row(1).left_center() + vec2(6.0 + metrics::TREE_INDENT + metrics::TREE_ARROW, 0.0);
    hierarchy.click(chevron);
    assert_eq!(hierarchy.take(), [HierarchyAction::Expand(1)], "a node not expanded yet is asked for");
    let chevron = hierarchy.row(0).left_center() + vec2(6.0 + metrics::TREE_ARROW, 0.0);
    hierarchy.click(chevron);
    assert_eq!(hierarchy.take(), [HierarchyAction::Collapse(0)]);
    let recursion = hierarchy.row(3).left_center() + vec2(6.0 + 2.0 * metrics::TREE_INDENT + metrics::TREE_ARROW, 0.0);
    hierarchy.click(recursion);
    assert!(
        !hierarchy.take().iter().any(|action| matches!(action, HierarchyAction::Expand(_))),
        "a recursive call has nothing to expand"
    );
    hierarchy.click_with(hierarchy.row(2).center(), 2);
    let go = HierarchyAction::GoTo { path: PathBuf::from("/p/TestTotal.go"), offset: 7, focus_editor: true };
    let taken = hierarchy.take();
    assert!(taken.contains(&go), "a double click goes to the code: {taken:?}");
    hierarchy.key(Key::Enter);
    assert_eq!(hierarchy.take(), std::slice::from_ref(&go));
    hierarchy.key(Key::F4);
    assert_eq!(hierarchy.take(), [go]);
    assert_eq!(
        HierarchyAction::GoTo { path: PathBuf::from("/p/a.go"), offset: 3, focus_editor: true }.command(Some(0)),
        Some(("navigate.place.goTo", json!({ "path": "/p/a.go", "offset": 3 }))),
        "through the command that records the place for Back"
    );
}

#[test]
fn with_single_click_navigation_a_click_goes_to_the_code() {
    let mut hierarchy = Harness::new(call_state(true));
    hierarchy.click(hierarchy.row(1).center());
    assert_eq!(
        hierarchy.take(),
        [HierarchyAction::GoTo { path: PathBuf::from("/p/Run.go"), offset: 6, focus_editor: false }]
    );
}

#[test]
fn keys_expand_collapse_and_walk_the_tree() {
    let mut hierarchy = Harness::new(call_state(false));
    hierarchy.click(hierarchy.row(0).center());
    hierarchy.take();
    hierarchy.key(Key::ArrowLeft);
    assert_eq!(hierarchy.take(), [HierarchyAction::Collapse(0)], "← collapses an expanded node");
    hierarchy.key(Key::ArrowRight);
    assert_eq!(hierarchy.window.selected, Some(1), "→ on an expanded node goes to its first child");
    hierarchy.key(Key::ArrowRight);
    assert_eq!(hierarchy.take(), [HierarchyAction::Expand(1)], "→ expands a node not expanded yet");
    hierarchy.key(Key::ArrowLeft);
    assert_eq!(hierarchy.window.selected, Some(0), "← on a leaf goes to its parent");
    hierarchy.key(Key::ArrowDown);
    hierarchy.key(Key::ArrowDown);
    assert_eq!(hierarchy.window.selected, Some(2));
    hierarchy.key(Key::Escape);
    assert_eq!(hierarchy.take(), [HierarchyAction::FocusEditor]);
}

#[test]
fn the_toolbar_asks_for_its_commands() {
    let mut hierarchy = Harness::new(call_state(false));
    let expected = [
        ("direction-second", HierarchyAction::Direction(HierarchyDirection::Callees)),
        ("sort", HierarchyAction::Sort(true)),
        ("refresh", HierarchyAction::Refresh),
        ("single-click", HierarchyAction::SingleClick(true)),
        ("expand-all", HierarchyAction::ExpandAll),
        ("collapse-all", HierarchyAction::CollapseAll),
        ("pin", HierarchyAction::Pin(true)),
        ("close", HierarchyAction::Close),
    ];
    for (button, action) in expected {
        hierarchy.click(hierarchy.button(button).center());
        assert_eq!(hierarchy.take(), [action], "{button}");
    }
    hierarchy.click(hierarchy.button("direction-first").center());
    assert!(hierarchy.take().is_empty(), "the direction shown is not asked for again");
    let commands = [
        (
            HierarchyAction::Direction(HierarchyDirection::Callees),
            "navigate.hierarchy.setOptions",
            json!({ "tab": 0, "direction": "callees" }),
        ),
        (HierarchyAction::Sort(true), "navigate.hierarchy.setOptions", json!({ "tab": 0, "sort": true })),
        (
            HierarchyAction::Scope(HierarchyScope::Test),
            "navigate.hierarchy.setOptions",
            json!({ "tab": 0, "scope": "test" }),
        ),
        (HierarchyAction::SingleClick(true), "navigate.hierarchy.setOptions", json!({ "singleClick": true })),
        (HierarchyAction::Refresh, "navigate.hierarchy.refresh", json!({ "tab": 0 })),
        (HierarchyAction::ExpandAll, "navigate.hierarchy.expandAll", json!({ "tab": 0 })),
        (HierarchyAction::CollapseAll, "navigate.hierarchy.collapse", json!({ "tab": 0 })),
        (HierarchyAction::Pin(true), "navigate.hierarchy.pin", json!({ "tab": 0, "pinned": true })),
        (HierarchyAction::Close, "navigate.hierarchy.close", json!({ "tab": 0 })),
        (HierarchyAction::Select(2), "navigate.hierarchy.select", json!({ "tab": 2 })),
        (HierarchyAction::Expand(4), "navigate.hierarchy.expand", json!({ "node": 4 })),
        (HierarchyAction::Collapse(4), "navigate.hierarchy.collapse", json!({ "node": 4 })),
    ];
    for (action, command, params) in commands {
        assert_eq!(action.command(Some(0)), Some((command, params)), "{action:?}");
    }
    assert_eq!(HierarchyAction::Hide.command(Some(0)), None);

    let mut state = call_state(false);
    state.kind = Some(HierarchyKind::Type);
    state.direction = Some(HierarchyDirection::Supertypes);
    let mut types = Harness::new(state);
    types.click(types.button("direction-second").center());
    assert_eq!(types.take(), [HierarchyAction::Direction(HierarchyDirection::Subtypes)], "types go up or down");
}

#[test]
fn tabs_are_titled_by_direction_and_symbol_and_a_click_selects_one() {
    let mut state = call_state(false);
    state.tabs = serde_json::from_value(json!([
        { "tab": 0, "kind": "call", "direction": "callers", "symbol": "Total", "pinned": true },
        { "tab": 3, "kind": "type", "direction": "subtypes", "symbol": "Store", "pinned": false },
    ]))
    .unwrap();
    let mut hierarchy = Harness::new(state);
    assert_eq!(tab_title(&hierarchy.window.state().tabs[0]), t!("ide-hierarchy-tab-callers", symbol = "Total"));
    assert_eq!(tab_title(&hierarchy.window.state().tabs[1]), t!("ide-hierarchy-tab-subtypes", symbol = "Store"));
    let (_, second) = hierarchy.window.tabs[1];
    hierarchy.click(second.center());
    assert_eq!(hierarchy.take(), [HierarchyAction::Select(3)]);
    let (_, first) = hierarchy.window.tabs[0];
    hierarchy.click(first.center());
    assert!(hierarchy.take().is_empty(), "the selected tab stays");
}

#[test]
fn a_new_hierarchy_is_told_once_and_the_state_is_read_after_changes() {
    let mut window = HierarchyWindow::default();
    let all = Hierarchies::default();
    assert!(window.should_read(&all), "never read");
    assert!(!window.update(HierarchyState::default(), all.revision(), all.started()));
    assert!(!window.should_read(&all));
    assert!(window.update(call_state(false), 1, 1), "a new hierarchy shows the tool window");
    assert!(!window.update(call_state(false), 2, 1), "the same one changed");
    assert_eq!(window.selected, Some(0));
}

#[test]
fn the_tree_is_read_by_screen_readers_with_test_and_recursive_nodes_told() {
    let mut hierarchy = Harness::new(call_state(false));
    let tree = accessibility::testing::tree_of(&hierarchy.ctx, vec2(420.0, 500.0), |ui| {
        let _ = hierarchy.window.show(ui, &IdePalette::zen_dark());
    });
    assert_eq!(accessibility::unnamed(&tree), Vec::<String>::new());
    let run = accessibility::testing::find(&tree, egui::Role::TreeItem, "Run").unwrap();
    assert_eq!((run.level(), run.is_expanded()), (Some(2), Some(false)), "not expanded yet, but it can be");
    let test = accessibility::testing::find(&tree, egui::Role::TreeItem, "TestTotal").unwrap();
    assert!(test.description().unwrap().contains(&t!("ide-hierarchy-test")), "{:?}", test.description());
    let recursive = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.description().is_some_and(|text| text.contains(&t!("ide-hierarchy-recursive"))));
    assert_eq!(recursive.count(), 1, "the recursive call is told");
    assert!(accessibility::testing::find(&tree, egui::Role::Button, &t!("ide-hierarchy-pin")).is_some());
    assert!(
        accessibility::testing::find(&tree, egui::Role::Tab, &t!("ide-hierarchy-tab-callers", symbol = "Total"))
            .is_some()
    );
}

#[test]
fn without_a_hierarchy_the_window_says_how_to_have_one() {
    let mut window = HierarchyWindow::default();
    window.update(HierarchyState::default(), 1, 0);
    assert_eq!(window.message(), Some(t!("ide-hierarchy-empty")));
    let mut state = call_state(false);
    state.nodes.clear();
    state.pending = true;
    window.update(state.clone(), 2, 1);
    assert_eq!(window.message(), Some(t!("ide-hierarchy-building")));
    state.pending = false;
    state.error = Some("gopls does not provide a type hierarchy".to_owned());
    window.update(state, 3, 1);
    assert_eq!(window.message().as_deref(), Some("gopls does not provide a type hierarchy"));
}

fn wide_state(children: usize) -> HierarchyState {
    let mut nodes = vec![node(0, "Total", None, "project", json!((1..=children).collect::<Vec<_>>()))];
    nodes.extend((1..=children).map(|id| node(id, &format!("Caller{id}"), Some(0), "project", Value::Null)));
    let mut state = call_state(false);
    state.nodes = serde_json::from_value(Value::Array(nodes)).unwrap();
    state
}

#[test]
fn a_big_hierarchy_draws_only_the_rows_in_view_and_reveals_the_selection() {
    let mut hierarchy = Harness::new(wide_state(1000));
    assert_eq!(hierarchy.window.tree.len(), 1001);
    for _ in 0..3 {
        let laid_out = hierarchy.window.rows.len();
        assert!((5..60).contains(&laid_out), "only the rows in view are laid out: {laid_out}");
        let painted = hierarchy.drawn.iter().filter(|text| text.starts_with("Caller")).count();
        assert!((5..60).contains(&painted), "only the rows in view are painted: {painted}");
        assert!(
            !hierarchy.drawn.iter().any(|text| text.starts_with("Caller1000")),
            "the last row is not drawn out of view"
        );
        hierarchy.frame(Vec::new());
    }
    hierarchy.window.selected = Some(1000);
    hierarchy.window.reveal = true;
    hierarchy.frame(Vec::new());
    hierarchy.frame(Vec::new());
    assert!(hierarchy.window.row_rect(1000).is_some(), "the selected row is scrolled into view");
    assert!(hierarchy.window.row_rect(1).is_none(), "and the first rows are out of it");
    hierarchy.click(hierarchy.row(999).center());
    assert_eq!(hierarchy.window.selected, Some(999), "a row scrolled into view takes clicks");
    hierarchy.key(Key::ArrowDown);
    assert_eq!(hierarchy.window.selected, Some(1000));
}
