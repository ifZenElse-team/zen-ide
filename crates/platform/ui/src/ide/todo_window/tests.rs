use std::sync::Arc;

use egui::{Event, PointerButton, Pos2};
use zen_syntax::{HighlightKind, Highlighter, Span};
use zen_workbench::todo::{TodoFile, TodoItem};

use super::*;

struct Harness {
    ctx: egui::Context,
    window: TodoWindow,
    layout: ToolWindowLayout,
    actions: Vec<TodoAction>,
    filters: Vec<String>,
    time: f64,
}

impl Harness {
    fn new(list: TodoList) -> Self {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut window = TodoWindow::default();
        window.update(list);
        let mut harness = Self {
            ctx,
            window,
            layout: ToolWindowLayout::default(),
            actions: Vec::new(),
            filters: Vec::new(),
            time: 1.0,
        };
        harness.frame(Vec::new());
        harness
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.02;
        let input = egui::RawInput {
            time: Some(self.time),
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 400.0))),
            ..Default::default()
        };
        let mut output = self.ctx.run_ui(input, |ui| {
            self.actions.extend(self.window.show(ui, &IdePalette::zen_dark(), &self.layout, &self.filters));
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
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
    }

    fn row(&self, row: Row) -> Rect {
        self.window.rows.iter().find(|(drawn, _)| *drawn == row).map(|(_, rect)| *rect).unwrap()
    }
}

fn item(line: usize, offset: usize, text: &str) -> TodoItem {
    TodoItem {
        line,
        column: 3,
        offset,
        length: text.chars().count(),
        text: text.to_owned(),
        line_text: format!("// {text}"),
        continued: Vec::new(),
    }
}

fn list() -> TodoList {
    TodoList {
        files: vec![
            TodoFile {
                path: PathBuf::from("/p/a.go"),
                relative: "a.go".to_owned(),
                items: vec![item(2, 20, "TODO one"), item(5, 50, "FIXME two")],
            },
            TodoFile {
                path: PathBuf::from("/p/b/b.go"),
                relative: "b/b.go".to_owned(),
                items: vec![item(0, 3, "todo three")],
            },
        ],
        total: 3,
        done: true,
        scan: 1,
        ..TodoList::default()
    }
}

fn go(path: &str, offset: usize, focus_editor: bool) -> TodoAction {
    TodoAction::GoTo { path: PathBuf::from(path), offset, focus_editor }
}

#[test]
fn a_click_selects_a_double_click_opens_and_single_click_navigation_opens_at_once() {
    let mut todo = Harness::new(list());
    assert_eq!(todo.window.selected, Some(Selected::Item(PathBuf::from("/p/a.go"), 2)), "the first item");
    todo.click(todo.row(Row::Item(0, 1)).center(), false);
    assert!(todo.actions.is_empty(), "a click only selects");
    assert_eq!(todo.window.selected, Some(Selected::Item(PathBuf::from("/p/a.go"), 5)));
    todo.click(todo.row(Row::Item(1, 0)).center(), true);
    assert_eq!(todo.actions.last(), Some(&go("/p/b/b.go", 3, true)));
    todo.actions.clear();
    todo.window.options.navigate_with_single_click = true;
    todo.click(todo.row(Row::Item(0, 0)).center(), false);
    assert_eq!(todo.actions, [go("/p/a.go", 20, false)]);
}

#[test]
fn keys_move_fold_open_and_expand_or_collapse_all() {
    let mut todo = Harness::new(list());
    todo.click(todo.row(Row::Item(0, 0)).center(), false);
    todo.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(todo.window.selected_row(), Some(Row::Item(0, 1)));
    todo.key(Key::Enter, Modifiers::NONE);
    assert_eq!(todo.actions, [go("/p/a.go", 50, true)]);
    todo.key(Key::F4, Modifiers::NONE);
    assert_eq!(todo.actions.last(), Some(&go("/p/a.go", 50, true)), "F4 is Jump to Source");
    todo.key(Key::Minus, Modifiers::COMMAND);
    assert_eq!(todo.window.collapsed.len(), 2);
    assert_eq!(todo.window.selected_row(), Some(Row::File(0)), "the selection moves to its file");
    assert_eq!(todo.window.visible(), [Row::File(0), Row::File(1)]);
    todo.key(Key::Equals, Modifiers::COMMAND);
    assert!(todo.window.collapsed.is_empty());
    todo.key(Key::ArrowLeft, Modifiers::NONE);
    assert!(todo.window.collapsed.contains(Path::new("/p/a.go")));
    todo.key(Key::Escape, Modifiers::NONE);
    assert_eq!(todo.actions.last(), Some(&TodoAction::FocusEditor));
}

#[test]
fn occurrences_walk_the_items_across_files_unfolding_them() {
    let mut window = TodoWindow::default();
    window.update(list());
    window.selected = None;
    window.collapse_all();
    assert_eq!(window.occurrence(true), Some(go("/p/a.go", 20, false)));
    assert!(!window.collapsed.contains(Path::new("/p/a.go")), "its file unfolds");
    assert_eq!(window.occurrence(true), Some(go("/p/a.go", 50, false)));
    assert_eq!(window.occurrence(true), Some(go("/p/b/b.go", 3, false)));
    assert_eq!(window.occurrence(true), None, "the last one stays");
    assert_eq!(window.occurrence(false), Some(go("/p/a.go", 50, false)));
    window.select(Row::File(1));
    assert_eq!(window.occurrence(false), Some(go("/p/a.go", 50, false)), "from a file, the item before it");
    let state = window.state();
    assert_eq!((state["items"].as_u64(), state["files"].as_u64()), (Some(3), Some(2)));
    assert_eq!(state["selected"]["line"], 5);
    assert_eq!(state["tabs"], json!(["project", "currentFile", "scopeBased"]));
}

#[test]
fn new_items_keep_the_selection_by_its_line() {
    let mut window = TodoWindow::default();
    window.update(list());
    window.select(Row::Item(1, 0));
    let mut moved = list();
    moved.files.remove(0);
    window.update(moved);
    assert_eq!(window.selected_row(), Some(Row::Item(0, 0)));
    window.update(TodoList::default());
    assert_eq!(window.selected, None);
    assert!(window.select_tab(TodoTab::CurrentChangelist, None).is_err(), "not without Git");
    window.select_tab(TodoTab::ScopeBased, Some("named:Store".to_owned())).unwrap();
    assert_eq!(window.params(), json!({ "tab": "scopeBased", "scope": "named:Store" }));
}

#[test]
fn the_items_are_a_tree_for_screen_readers() {
    let mut todo = Harness::new(list());
    let tree = accessibility::testing::tree_of(&todo.ctx, vec2(900.0, 400.0), |ui| {
        let _ = todo.window.show(ui, &IdePalette::zen_dark(), &todo.layout, &[]);
    });
    assert_eq!(accessibility::unnamed(&tree), Vec::<String>::new());
    let file = accessibility::testing::find(&tree, egui::Role::TreeItem, "a.go").unwrap();
    assert_eq!((file.level(), file.is_expanded()), (Some(1), Some(true)));
    let item = accessibility::testing::find(&tree, egui::Role::TreeItem, "3: // TODO one").unwrap();
    assert_eq!(item.level(), Some(2));
}

struct Comments;

impl Highlighter for Comments {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if let Some(at) = line.find("//") {
                spans.push(Span { range: offset + at..offset + line.trim_end().len(), kind: HighlightKind::Comment });
            }
            offset += line.len();
        }
        spans
    }
}

pub(in crate::ide) fn comments_by_slashes(session: &mut Session) {
    let comments: Arc<dyn Highlighter> = Arc::new(Comments);
    session
        .state_mut::<zen_workbench::brackets::Highlighters>()
        .set_source(Arc::new(move |name| Comments.handles(name).then(|| Arc::clone(&comments))));
}

fn settle(window: &mut TodoWindow, registry: &Registry, session: &mut Session, now: f64) -> usize {
    for step in 0..400 {
        window.follow(registry, session, now + f64::from(step) * 0.01, (false, 0));
        if window.scan.is_none() && window.changed.is_none() {
            return window.list.total;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the items did not settle");
}

#[test]
fn the_items_follow_edits_and_changes_on_disk_a_moment_after_them() {
    zen_i18n::register(crate::toolbox::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("main.go"), "package main\n\n// TODO one\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    comments_by_slashes(&mut session);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    while registry.dispatch(&mut session, "search.index.status", json!({})).unwrap()["namesReady"] != true {
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut window = TodoWindow::default();
    assert_eq!(settle(&mut window, &registry, &mut session, 0.0), 1, "the first scan starts at once");

    registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("main.go") })).unwrap();
    registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "// FIXME two\n" })).unwrap();
    assert!(window.follow(&registry, &mut session, 10.0, (false, 0)).is_some(), "it waits a moment after an edit");
    assert_eq!(window.list.total, 1);
    assert_eq!(settle(&mut window, &registry, &mut session, 10.5), 2);

    std::fs::write(root.join("more.go"), "package main\n// todo three\n").unwrap();
    registry.dispatch(&mut session, "file.index.refresh", json!({ "paths": [root.join("more.go")] })).unwrap();
    window.disk_changed();
    window.follow(&registry, &mut session, 20.0, (false, 0));
    assert_eq!(settle(&mut window, &registry, &mut session, 20.5), 3);

    window.options.tab = TodoTab::CurrentFile;
    assert_eq!(settle(&mut window, &registry, &mut session, 30.0), 2, "the file in the editor");
}

fn many(files: usize, items: usize) -> TodoList {
    let files: Vec<TodoFile> = (0..files)
        .map(|file| TodoFile {
            path: PathBuf::from(format!("/p/f{file:04}.go")),
            relative: format!("f{file:04}.go"),
            items: (0..items).map(|line| item(line, line * 10, "TODO later")).collect(),
        })
        .collect();
    TodoList { files, total: 0, done: true, scan: 1, ..TodoList::default() }
}

#[test]
fn only_the_rows_in_sight_are_drawn_and_an_occurrence_scrolls_to_its_row() {
    let mut todo = Harness::new(many(1000, 10));
    assert!(todo.window.rows.len() < 20, "{} rows drawn of 11000", todo.window.rows.len());
    todo.window.selected = None;
    todo.window.occurrence(false);
    for _ in 0..50 {
        todo.frame(Vec::new());
    }
    assert_eq!(todo.window.selected_row(), Some(Row::Item(999, 9)));
    assert!(todo.window.rows.iter().any(|(row, _)| *row == Row::Item(999, 9)), "the last item is in sight");
}

#[test]
fn filter_todo_items_picks_a_filter_or_opens_the_settings() {
    let mut harness = Harness::new(list());
    harness.filters = vec!["bugs".to_owned()];
    harness.window.options.filter = Some("bugs".to_owned());
    harness.frame(Vec::new());
    assert_eq!(harness.window.params()["filter"], "bugs", "the scan asks for the filter");
    harness.window.options.filter = None;
    assert!(harness.window.params()["filter"].is_null());
}

#[test]
fn the_toolbar_has_navigate_with_single_click_before_expand_all() {
    let mut todo = Harness::new(list());
    let toolbar = |index: f32| pos2(TOOLBAR / 2.0, metrics::TOOL_HEADER + 16.0 + 28.0 * index + 8.0);
    todo.click(toolbar(2.0), false);
    assert!(todo.window.options.navigate_with_single_click);
    todo.click(toolbar(4.0) + vec2(0.0, 8.0), false);
    assert_eq!(todo.window.collapsed.len(), 2, "Collapse All");
}

#[test]
fn snapshots_of_the_todo_tool_window() {
    let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
    zen_i18n::register(crate::toolbox::LOCALES);
    for language in ["en", "ru"] {
        zen_i18n::set_language(Some(language));
        for preview in [false, true] {
            let mut window = TodoWindow::default();
            window.update(TodoList {
                files: vec![
                    TodoFile {
                        path: PathBuf::from("/p/cmd/server/main.go"),
                        relative: "cmd/server/main.go".to_owned(),
                        items: vec![
                            TodoItem {
                                line: 11,
                                column: 4,
                                offset: 0,
                                length: 30,
                                text: "TODO: read the port from flags".to_owned(),
                                line_text: "\t// TODO: read the port from flags".to_owned(),
                                continued: Vec::new(),
                            },
                            TodoItem {
                                line: 40,
                                column: 21,
                                offset: 0,
                                length: 22,
                                text: "FIXME close on timeout".to_owned(),
                                line_text: "\treturn conn, nil // FIXME close on timeout".to_owned(),
                                continued: Vec::new(),
                            },
                        ],
                    },
                    TodoFile {
                        path: PathBuf::from("/p/store.go"),
                        relative: "store.go".to_owned(),
                        items: vec![TodoItem {
                            line: 2,
                            column: 3,
                            offset: 0,
                            length: 20,
                            text: "todo: cache the rows".to_owned(),
                            line_text: "/* todo: cache the rows */".to_owned(),
                            continued: Vec::new(),
                        }],
                    },
                ],
                total: 3,
                done: true,
                scan: 1,
                ..TodoList::default()
            });
            window.git = true;
            window.options.preview_source = preview;
            window.preview = preview.then(|| {
                let lines = (0..30).map(|line| format!("line {line}")).collect();
                ((PathBuf::from("/p/cmd/server/main.go"), 11, 0), Preview { first: 0, lines })
            });
            let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 320.0), 2.0);
            let palette = IdePalette::zen_dark();
            palette.apply(screen.context());
            let layout = ToolWindowLayout::default();
            let mut draw = |ui: &mut Ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
                let _ = window.show(ui, &palette, &layout, &[]);
            };
            screen.render(&mut draw);
            let image = screen.frame(Vec::new(), &mut draw);
            let name = if preview { "todo-window-preview" } else { "todo-window" };
            let file = Path::new(&dir).join(format!("{name}-{language}.png"));
            crate::offscreen::write_png(&file, &image).unwrap();
        }
    }
    zen_i18n::set_language(Some("en"));
}
