use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_editor::{Selection, Selections};
use zen_workbench::Workbench;
use zen_workbench::groups::GroupId;
use zen_workbench::tabs::TabPlacement;

use super::super::view_modes::ViewClass;
use super::super::{IdeProduct, IdeView, workspace};

struct Window {
    ctx: egui::Context,
    registry: Registry,
    session: Session,
    view: IdeView,
    project: PathBuf,
    time: f64,
    uploads: usize,
    widths: Vec<(String, f32)>,
    detached: egui::Context,
    _dir: tempfile::TempDir,
}

pub(in crate::ide) fn product() -> IdeProduct {
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

fn window(files: &[(&str, &str)]) -> Window {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    for (name, text) in files {
        std::fs::write(project.join(name), text).unwrap();
    }
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
    view.project_open = false;
    Window {
        ctx: egui::Context::default(),
        registry,
        session,
        view,
        project,
        time: 0.0,
        uploads: 0,
        widths: Vec::new(),
        detached: egui::Context::default(),
        _dir: dir,
    }
}

impl Window {
    fn run(&mut self, command: &str, params: serde_json::Value) -> serde_json::Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap()
    }

    fn open(&mut self, name: &str) {
        let path = self.project.join(name);
        self.run("file.document.open", json!({ "path": path }));
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<(Pos2, String)> {
        self.time += 1.0;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
        let mut output = self.ctx.run_ui(input, |ui| view.show(ui, registry, session));
        self.uploads += output.textures_delta.set.len();
        output.textures_delta.clear();
        let mut texts = Vec::new();
        self.widths.clear();
        for clipped in output.shapes {
            collect(&clipped.shape, &mut texts);
            widths(&clipped.shape, &mut self.widths);
        }
        texts
    }

    fn frames(&mut self, count: usize) -> Vec<(Pos2, String)> {
        let mut texts = Vec::new();
        for _ in 0..count {
            texts = self.frame(Vec::new());
        }
        texts
    }

    fn workbench(&mut self) -> &mut Workbench {
        self.session.state_mut::<Workbench>()
    }

    fn caret(&mut self) -> usize {
        self.workbench().active_document().unwrap().selections().primary().head
    }
}

fn collect(shape: &Shape, out: &mut Vec<(Pos2, String)>) {
    match shape {
        Shape::Text(text) => out.push((text.pos, text.galley.text().to_owned())),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, out)),
        _ => {}
    }
}

fn widths(shape: &Shape, out: &mut Vec<(String, f32)>) {
    match shape {
        Shape::Text(text) => out.push((text.galley.text().to_owned(), text.galley.size().x)),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| widths(shape, out)),
        _ => {}
    }
}

fn count(texts: &[(Pos2, String)], wanted: &str) -> usize {
    texts.iter().filter(|(_, text)| text.contains(wanted)).count()
}

fn click(at: Pos2) -> Vec<Event> {
    let press =
        |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    vec![Event::PointerMoved(at), press(true), press(false)]
}

#[test]
fn both_groups_draw_the_file_and_typing_in_one_shows_in_the_other_with_carets_of_their_own() {
    let mut window = window(&[("a.txt", "alpha line\n")]);
    window.open("a.txt");
    window.frames(2);
    window.workbench().active_document_mut().unwrap().set_selections(Selections::single(Selection::caret(5)));
    window.run("editor.split.right", json!({}));
    let texts = window.frames(3);
    assert_eq!(count(&texts, "alpha line"), 2, "each group draws the file: {texts:?}");
    assert_eq!(window.caret(), 5, "the new group starts at the caret of the old one");

    window.workbench().active_document_mut().unwrap().set_selections(Selections::single(Selection::caret(0)));
    window.frames(1);
    window.frame(vec![Event::Text("Z".to_owned())]);
    let texts = window.frames(2);
    assert_eq!(window.workbench().documents().len(), 1, "one document for both groups");
    assert_eq!(count(&texts, "Zalpha line"), 2, "the edit shows in both groups: {texts:?}");
    assert_eq!(window.caret(), 1, "typed in the focused group, the second");

    window.run("editor.split.focus", json!({ "group": 1 }));
    window.frames(2);
    assert_eq!(window.caret(), 6, "the first group's caret stayed, moved along with the text");
}

#[test]
fn a_click_in_the_other_group_gives_it_the_keyboard() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.run("editor.split.right", json!({ "move": true }));
    let texts = window.frames(3);
    assert_eq!(window.workbench().groups().focused(), 2);
    let (at, _) = texts.iter().find(|(_, text)| text == "first").cloned().expect("the first group draws a.txt");
    window.frame(click(at + vec2(2.0, 4.0)));
    window.frames(2);
    assert_eq!(window.workbench().groups().focused(), 1);
    let a = window.project.join("a.txt");
    assert_eq!(window.workbench().active_document().unwrap().path(), a);
    assert!(window.view.editor_view.has_focus_in(&window.ctx), "the editor of the first group has the keyboard");
}

#[test]
fn a_new_split_starts_scrolled_where_its_source_was() {
    let text: String = (0..300).map(|line| format!("line {line:03}\n")).collect();
    let mut window = window(&[("long.txt", &text)]);
    window.open("long.txt");
    window.frames(2);
    window.run("editor.caret.move", json!({ "movement": { "kind": "documentEnd" }, "extend": false }));
    let texts = window.frames(3);
    assert_eq!(count(&texts, "line 299"), 1);
    assert_eq!(count(&texts, "line 000"), 0, "scrolled to the end");
    window.run("editor.split.right", json!({}));
    let texts = window.frames(3);
    assert_eq!(count(&texts, "line 299"), 2, "both groups show the end: {texts:?}");
    assert_eq!(count(&texts, "line 000"), 0);
}

#[test]
fn a_tab_dragged_to_the_other_group_moves_there() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.run("editor.split.right", json!({}));
    let texts = window.frames(3);
    let (tab, _) = texts.iter().find(|(_, text)| text == "a.txt").cloned().expect("the tab of a.txt");
    let (target, _) =
        texts.iter().filter(|(_, text)| text == "second").max_by(|a, b| a.0.x.total_cmp(&b.0.x)).cloned().unwrap();
    let grab = tab + vec2(4.0, 6.0);
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    window.frame(vec![Event::PointerMoved(grab), button(grab, true)]);
    window.frame(vec![Event::PointerMoved(grab + vec2(30.0, 0.0))]);
    window.frame(vec![Event::PointerMoved(target + vec2(0.0, 40.0))]);
    window.frame(vec![button(target + vec2(0.0, 40.0), false)]);
    window.frames(2);
    let groups = window.workbench().groups().clone();
    let tabs: Vec<Vec<PathBuf>> = groups.groups().iter().map(|group| group.tabs.clone()).collect();
    let (a, b) = (window.project.join("a.txt"), window.project.join("b.txt"));
    assert_eq!(tabs, [vec![b.clone()], vec![b, a.clone()]]);
    assert_eq!(groups.focused(), 2);
    assert_eq!(window.workbench().active_document().unwrap().path(), a);
}

#[test]
fn the_splits_survive_a_restart_and_show_in_the_window_state() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n")]);
    window.open("a.txt");
    window.open("b.txt");
    assert!(window.view.window_state(&window.session)["splits"].is_null());
    window.run("editor.split.down", json!({ "index": 0, "move": true }));
    window.run("editor.split.resize", json!({ "before": 2, "ratio": 0.25 }));
    window.frames(2);
    let state = window.view.window_state(&window.session);
    assert_eq!(state["splits"]["root"]["direction"], "down");
    assert_eq!(state["splits"]["focused"], 2);
    let project = window.project.clone();
    let layout = workspace::capture(&window.view, &mut window.session, &project);
    let saved = serde_json::to_value(&layout).unwrap();
    assert_eq!(saved["splits"]["root"]["ratio"], 0.25);
    let read: workspace::ProjectLayout = serde_json::from_value(saved).unwrap();
    assert_eq!(read, layout);

    let mut restarted = window_at(&project);
    workspace::restore(&mut restarted.view, &restarted.registry, &mut restarted.session, &read);
    let groups = restarted.workbench().groups().clone();
    assert_eq!(&groups, window.workbench().groups());
    assert_eq!(restarted.workbench().active_document().unwrap().path(), project.join("a.txt"));
    let texts = restarted.frames(3);
    assert_eq!(count(&texts, "first"), 1);
    assert_eq!(count(&texts, "second"), 1);

    let plain: workspace::ProjectLayout = serde_json::from_str(r#"{ "documents": [] }"#).unwrap();
    assert_eq!(plain.splits, None, "older files have no splits");
    window.run("editor.split.unsplitAll", json!({}));
    let layout = workspace::capture(&window.view, &mut window.session, &project);
    assert!(serde_json::to_value(&layout).unwrap().get("splits").is_none(), "nothing is saved without a split");
}

fn window_at(project: &Path) -> Window {
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    let mut view = IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
    view.project_open = false;
    Window {
        ctx: egui::Context::default(),
        registry,
        session,
        view,
        project: project.to_owned(),
        time: 0.0,
        uploads: 0,
        widths: Vec::new(),
        detached: egui::Context::default(),
        _dir: tempfile::tempdir().unwrap(),
    }
}

#[test]
fn goto_next_splitter_moves_the_keyboard_between_the_editors() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.run("editor.split.right", json!({ "move": true }));
    window.frames(3);
    window.run("editor.split.next", json!({}));
    window.frames(2);
    assert_eq!(window.workbench().groups().focused(), 1);
    assert!(window.view.editor_view.has_focus_in(&window.ctx));
    window.frame(vec![Event::Text("X".to_owned())]);
    window.frames(1);
    let text = window.workbench().active_document().unwrap().text().to_owned();
    assert_eq!(text, "Xfirst\n", "typing goes to the group that took the keyboard");
}

#[test]
fn a_split_without_the_keyboard_never_takes_it_when_its_tab_changes() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n"), ("c.txt", "third\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.open("c.txt");
    window.run("editor.split.right", json!({ "move": true }));
    window.frames(3);
    assert_eq!(window.workbench().groups().focused(), 2);
    window.run("file.document.close", json!({ "group": 1 }));
    for _ in 0..3 {
        window.frame(vec![Event::Text("X".to_owned())]);
    }
    let c = window.project.join("c.txt");
    let document = window.workbench().documents().iter().find(|document| document.path() == c).unwrap().clone();
    assert_eq!(document.text(), "XXXthird\n", "typing stays in the split that has the keyboard");
    assert_eq!(window.workbench().groups().focused(), 2);
}

#[test]
fn two_previews_in_two_splits_are_uploaded_once() {
    let svg = |colour: &str| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="{colour}"/></svg>"#
        )
    };
    let (red, blue) = (svg("red"), svg("blue"));
    let mut window = window(&[("a.svg", &red), ("b.svg", &blue)]);
    window.open("a.svg");
    window.open("b.svg");
    window.run("editor.split.right", json!({ "move": true }));
    for name in ["a.svg", "b.svg"] {
        let path = window.project.join(name);
        let answer = window.run("editor.preview.getMode", json!({ "path": path, "waitMs": 5000 }));
        assert_eq!(answer["state"], "ready", "{answer}");
    }
    window.frames(3);
    window.uploads = 0;
    window.frames(5);
    assert_eq!(window.uploads, 0, "each split keeps the texture of its picture");
}

#[test]
fn the_tab_menu_closes_the_other_tabs_saving_their_changes() {
    zen_i18n::register(crate::toolbox::LOCALES);
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n"), ("c.txt", "third\n")]);
    window.open("a.txt");
    window.run("editor.edit.insert", json!({ "text": "edited " }));
    window.open("b.txt");
    window.open("c.txt");
    let texts = window.frames(3);
    let (tab, _) = texts.iter().find(|(_, text)| text == "b.txt").cloned().expect("the tab of b.txt");
    let at = tab + vec2(4.0, 6.0);
    let secondary = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    window.frame(vec![Event::PointerMoved(at)]);
    window.frame(vec![secondary(true), secondary(false)]);
    let menu = window.frames(2);
    let labels = [
        "keymap-file-document-close",
        "keymap-file-document-closeOthers",
        "keymap-file-document-closeAll",
        "keymap-file-document-closeUnmodified",
        "keymap-file-document-closeAllButPinned",
        "keymap-file-document-closeLeft",
        "keymap-file-document-closeRight",
        "keymap-file-document-closeReadOnly",
        "ide-tab-copy-reference",
        "keymap-editor-split-right",
        "keymap-editor-split-right-move",
        "keymap-editor-split-down",
        "keymap-editor-split-down-move",
        "keymap-editor-split-moveToOpposite",
        "ide-tab-pin-active",
        "keymap-editor-tab-keepOpen",
        "keymap-view-settings-open-editorTabs",
        "keymap-file-document-reopenClosed",
        "ide-tool-bookmarks",
        "menu-file-local-history",
    ];
    let mut above = f32::NEG_INFINITY;
    for label in labels {
        let item = zen_i18n::t!(label);
        let found: Vec<&(Pos2, String)> = menu.iter().filter(|(_, text)| *text == item).collect();
        assert_eq!(found.len(), 1, "{item} is in the tab menu once");
        assert!(found[0].0.y > above, "{item} comes after the item before it");
        above = found[0].0.y;
    }
    let others = zen_i18n::t!("keymap-file-document-closeOthers");
    let (item, _) = menu.iter().find(|(_, text)| *text == others).cloned().unwrap();
    window.frame(click(item + vec2(4.0, 4.0)));
    window.frames(2);
    let tabs = window.workbench().groups().groups()[0].tabs.clone();
    assert_eq!(tabs, [window.project.join("b.txt")]);
    assert_eq!(std::fs::read_to_string(window.project.join("a.txt")).unwrap(), "edited first\n", "saved as it closed");
}

fn key(key: egui::Key, modifiers: Modifiers) -> Vec<Event> {
    vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers }]
}

fn close_button(window: &Window, texts: &[(Pos2, String)], name: &str) -> Pos2 {
    let (at, _) =
        texts.iter().filter(|(_, text)| text == name).min_by(|a, b| a.0.y.total_cmp(&b.0.y)).cloned().expect("the tab");
    let width = window.widths.iter().find(|(text, _)| text == name).map(|(_, width)| *width).unwrap();
    at + vec2(width + 16.0, 8.0)
}

#[test]
fn alt_click_on_the_close_button_closes_the_other_tabs_and_a_middle_click_closes_a_tab() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n"), ("c.txt", "third\n")]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        window.open(name);
    }
    let texts = window.frames(3);
    let (a, _) =
        texts.iter().filter(|(_, text)| text == "a.txt").min_by(|a, b| a.0.y.total_cmp(&b.0.y)).cloned().unwrap();
    let middle = |pressed| Event::PointerButton {
        pos: a + vec2(4.0, 6.0),
        button: PointerButton::Middle,
        pressed,
        modifiers: Modifiers::NONE,
    };
    window.frame(vec![Event::PointerMoved(a + vec2(4.0, 6.0))]);
    window.frame(vec![middle(true), middle(false)]);
    let texts = window.frames(2);
    assert_eq!(window.workbench().groups().groups()[0].tabs.len(), 2, "the middle click closed a.txt");
    let at = close_button(&window, &texts, "c.txt");
    let alt =
        |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::ALT };
    window.frame(vec![Event::PointerMoved(at)]);
    window.frame(vec![alt(true), alt(false)]);
    window.frames(2);
    let c = window.project.join("c.txt");
    assert_eq!(window.workbench().groups().groups()[0].tabs, [c]);
}

#[test]
fn a_pinned_tab_goes_first_with_a_pin_that_unpins_it() {
    zen_i18n::register(crate::toolbox::LOCALES);
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.run("editor.tab.pin", json!({}));
    let texts = window.frames(3);
    let x = |name: &str| {
        texts.iter().filter(|(_, text)| text == name).map(|(at, _)| at).min_by(|a, b| a.y.total_cmp(&b.y)).unwrap().x
    };
    assert!(x("b.txt") < x("a.txt"), "the pinned tab is drawn first");
    window.run("editor.tab.select", json!({ "index": 1 }));
    let texts = window.frames(2);
    let at = close_button(&window, &texts, "b.txt");
    window.frame(click(at));
    window.frames(2);
    let group = window.workbench().groups().groups()[0].clone();
    assert!(group.pinned.is_empty(), "the pin of an inactive tab unpins it");
    assert_eq!(group.tabs.len(), 2, "and closes nothing");
}

#[test]
fn select_next_and_previous_tab_follow_the_default_shortcuts() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n"), ("c.txt", "third\n")]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        window.open(name);
    }
    window.frames(2);
    let shown = |window: &mut Window| window.workbench().active_document().unwrap().name();
    let command_shift = Modifiers { shift: true, ..Modifiers::MAC_CMD };
    window.frame(key(egui::Key::CloseBracket, command_shift));
    window.frames(1);
    assert_eq!(shown(&mut window), "a.txt", "⇧⌘] goes round to the first tab");
    window.frame(key(egui::Key::ArrowRight, Modifiers::CTRL));
    window.frames(1);
    assert_eq!(shown(&mut window), "b.txt", "⌃→");
    window.frame(key(egui::Key::OpenBracket, command_shift));
    window.frames(1);
    assert_eq!(shown(&mut window), "a.txt", "⇧⌘[");
    window.frame(key(egui::Key::ArrowLeft, Modifiers::CTRL));
    window.frames(1);
    assert_eq!(shown(&mut window), "c.txt", "⌃← goes round to the last tab");
}

#[test]
fn pinned_tabs_and_closed_tabs_survive_a_restart_per_project() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n"), ("c.txt", "third\n")]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        window.open(name);
    }
    window.run("editor.tab.pin", json!({ "index": 1 }));
    window.run("file.document.close", json!({ "index": 2 }));
    window.frames(2);
    let project = window.project.clone();
    let layout = workspace::capture(&window.view, &mut window.session, &project);
    let read: workspace::ProjectLayout = serde_json::from_value(serde_json::to_value(&layout).unwrap()).unwrap();
    assert_eq!(read, layout);

    let mut restarted = window_at(&project);
    workspace::restore(&mut restarted.view, &restarted.registry, &mut restarted.session, &read);
    let group = restarted.workbench().groups().groups()[0].clone();
    assert_eq!(group.tabs, [project.join("b.txt"), project.join("a.txt")]);
    assert_eq!(group.pinned, [project.join("b.txt")]);
    restarted.run("file.document.reopenClosed", json!({}));
    let tabs = restarted.workbench().groups().groups()[0].tabs.clone();
    assert_eq!(tabs, [project.join("b.txt"), project.join("a.txt"), project.join("c.txt")]);
}

fn tab_row(entries: &[(&str, bool, &str)]) -> Vec<(String, bool, PathBuf)> {
    entries.iter().map(|(name, dirty, path)| ((*name).to_owned(), *dirty, path.into())).collect()
}

#[test]
fn tab_titles_follow_the_appearance_settings() {
    let tabs =
        tab_row(&[("main.go", false, "/p/a/main.go"), ("main.go", true, "/p/b/main.go"), ("x.rs", true, "/p/x.rs")]);
    let mut settings = zen_workbench::tabs::EditorTabSettings::default();
    assert_eq!(super::tab_titles(&tabs, &settings), ["a/main.go", "b/main.go", "x.rs"]);
    settings.show_directory = false;
    settings.show_extension = false;
    settings.mark_modified = true;
    assert_eq!(super::tab_titles(&tabs, &settings), ["main", "*main", "*x"]);
}

fn set_tabs(window: &mut Window, change: impl FnOnce(&mut zen_workbench::tabs::EditorTabSettings)) {
    change(&mut window.view.overview.settings.editor_tabs);
    let settings = window.view.overview.settings.clone();
    settings.apply_to_session(window.session.settings_mut());
    zen_workbench::tabs::follow_policy(&mut window.session);
}

#[test]
fn the_tab_row_goes_where_tab_placement_says() {
    let inactive_tab = |window: &mut Window| {
        let texts = window.frames(3);
        texts.iter().find(|(_, text)| text == "a.txt").map(|(at, _)| *at)
    };
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n")]);
    window.open("a.txt");
    window.open("b.txt");
    let top = inactive_tab(&mut window).unwrap();
    set_tabs(&mut window, |tabs| tabs.placement = TabPlacement::Bottom);
    let bottom = inactive_tab(&mut window).unwrap();
    assert!(bottom.y > top.y + 100.0, "{top:?} {bottom:?}");
    set_tabs(&mut window, |tabs| tabs.placement = TabPlacement::Left);
    let left = inactive_tab(&mut window).unwrap();
    set_tabs(&mut window, |tabs| tabs.placement = TabPlacement::Right);
    let right = inactive_tab(&mut window).unwrap();
    assert!(right.x > left.x + 100.0, "{left:?} {right:?}");
    assert!((right.y - left.y).abs() < 1.0, "the same column on the other side");
    set_tabs(&mut window, |tabs| tabs.placement = TabPlacement::None);
    assert_eq!(inactive_tab(&mut window), None, "no tabs");
}

#[test]
fn multiple_rows_put_the_tabs_that_do_not_fit_on_a_row_below() {
    let files: Vec<(String, &str)> = (0..30).map(|at| (format!("rather-long-file-name-{at}.txt"), "text\n")).collect();
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(name, text)| (name.as_str(), *text)).collect();
    let mut window = window(&borrowed);
    for (name, _) in &files {
        window.open(name);
    }
    set_tabs(&mut window, |tabs| tabs.rows = zen_workbench::tabs::TabRows::Multiple);
    let texts = window.frames(3);
    let rows: std::collections::BTreeSet<i32> = texts
        .iter()
        .filter(|(_, text)| text.starts_with("rather-long-file-name-"))
        .map(|(at, _)| at.y.round() as i32)
        .collect();
    assert!(rows.len() > 1, "{rows:?}");
    for (name, _) in &files {
        assert!(count(&texts, name) > 0, "{name} is drawn");
    }
}

#[test]
fn pinned_tabs_get_a_row_of_their_own_when_asked() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.run("editor.tab.pin", json!({}));
    set_tabs(&mut window, |tabs| tabs.pinned_row = true);
    let texts = window.frames(3);
    let y = |name: &str| texts.iter().filter(|(_, text)| text == name).map(|(at, _)| at.y).fold(f32::MAX, f32::min);
    assert!(y("b.txt") < y("a.txt"), "the pinned tab's row is above the others");
}

#[test]
fn a_single_click_in_the_project_tree_previews_the_file_and_a_double_click_keeps_it() {
    let mut window = window(&[("a.txt", "first\n"), ("b.txt", "second\n")]);
    window.view.project_open = true;
    set_tabs(&mut window, |tabs| tabs.preview = true);
    let texts = window.frames(3);
    let row =
        |texts: &[(Pos2, String)], name: &str| texts.iter().find(|(_, text)| text == name).unwrap().0 + vec2(4.0, 6.0);
    let at = row(&texts, "a.txt");
    window.frame(click(at));
    window.frames(3);
    let group = window.workbench().groups().groups()[0].clone();
    assert_eq!(group.tabs.len(), 1, "one click opens it");
    assert!(group.is_preview(&group.tabs[0]));
    let texts = window.frames(1);
    let at = row(&texts, "b.txt");
    window.frame(click(at));
    window.frames(3);
    let group = window.workbench().groups().groups()[0].clone();
    assert_eq!(group.tabs.len(), 1, "the next click reuses the tab");
    assert!(group.tabs[0].ends_with("b.txt"));
    let mut twice = click(at);
    twice.extend(click(at));
    window.frame(twice);
    window.frames(3);
    let group = window.workbench().groups().groups()[0].clone();
    assert_eq!(group.preview, None, "a double click keeps it");
}

#[test]
fn a_double_click_on_a_preview_tab_keeps_it_open() {
    let mut window = window(&[("a.txt", "first\n")]);
    set_tabs(&mut window, |tabs| tabs.preview = true);
    let file = window.project.join("a.txt");
    window.run("file.document.open", json!({ "path": file, "preview": true }));
    let texts = window.frames(3);
    let at = texts.iter().filter(|(_, text)| text == "a.txt").min_by(|a, b| a.0.y.total_cmp(&b.0.y)).unwrap().0;
    let mut twice = click(at + vec2(4.0, 6.0));
    twice.extend(click(at + vec2(4.0, 6.0)));
    window.frame(twice);
    window.frames(2);
    assert_eq!(window.workbench().groups().groups()[0].preview, None);
}

impl Window {
    fn detached_frame(&mut self, group: GroupId, events: Vec<Event>) -> Vec<(Pos2, String)> {
        self.time += 1.0;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
        let mut output = self.detached.run_ui(input, |ui| {
            let class = (ViewClass::Own, vec2(800.0, 600.0));
            view.detached_editor_window(ui, group, class, registry, session);
        });
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in output.shapes {
            collect(&clipped.shape, &mut texts);
        }
        texts
    }

    fn windows(&mut self) -> Vec<GroupId> {
        self.workbench().groups().windows().iter().map(|window| window.group.id).collect()
    }
}

#[test]
fn a_detached_window_draws_its_tab_and_typing_there_shows_in_the_main_window() {
    let mut window = window(&[("a.txt", "alpha line\n"), ("b.txt", "beta\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.frames(2);
    window.run("editor.split.right", json!({ "index": 0 }));
    let split = window.workbench().groups().focused();
    window.run("editor.window.openTab", json!({ "group": split, "index": 0 }));
    let detached = window.windows()[0];
    window.run("editor.split.focus", json!({ "group": 1 }));
    window.run("editor.tab.select", json!({ "group": 1, "index": 0 }));
    window.frames(2);
    window.run("editor.split.focus", json!({ "group": detached }));
    let main = window.frame(Vec::new());
    assert_eq!(count(&main, "alpha line"), 1, "the main window still shows the file: {main:?}");
    let texts = (0..3).map(|_| window.detached_frame(detached, Vec::new())).last().unwrap();
    assert_eq!(count(&texts, "alpha line"), 1, "the window shows it: {texts:?}");
    assert_eq!(count(&texts, "a.txt"), 1, "and has its own tab row: {texts:?}");
    window.detached_frame(detached, vec![Event::Text("Z".to_owned())]);
    assert_eq!(window.workbench().documents().len(), 2, "no second document");
    let main = window.frames(2);
    assert_eq!(count(&main, "Zalpha line"), 1, "the edit shows in the main window: {main:?}");
}

#[test]
fn a_press_in_a_detached_window_gives_it_the_focus_and_its_close_box_docks_the_tabs() {
    let mut window = window(&[("a.txt", "alpha\n"), ("b.txt", "beta\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.run("editor.window.openTab", json!({}));
    let detached = window.windows()[0];
    window.run("editor.split.focus", json!({ "group": 1 }));
    window.frames(2);
    assert_eq!(window.view.window_to_raise(&window.session), None, "no change yet");
    window.detached_frame(detached, click(egui::pos2(300.0, 300.0)));
    assert_eq!(window.workbench().groups().focused(), detached);
    assert_eq!(window.view.window_to_raise(&window.session), Some(Some(detached)), "it comes to the front");
    window.run("editor.split.focus", json!({ "group": 1 }));
    assert_eq!(window.view.window_to_raise(&window.session), Some(None), "and so does the main window");
    let mut close =
        RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..RawInput::default() };
    close.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    let mut output = window.detached.run_ui(close, |ui| {
        view.detached_editor_window(ui, detached, (ViewClass::Own, vec2(800.0, 600.0)), registry, session);
    });
    output.textures_delta.clear();
    assert!(window.windows().is_empty(), "closing the window docks its tabs");
    assert_eq!(window.workbench().groups().groups()[0].tabs.len(), 2);
}

#[test]
fn a_tab_dragged_out_of_the_window_opens_in_a_new_one() {
    let mut window = window(&[("a.txt", "alpha\n"), ("b.txt", "beta\n")]);
    window.open("a.txt");
    window.open("b.txt");
    let texts = window.frames(3);
    let (tab, _) = texts.iter().find(|(_, text)| text == "a.txt").cloned().expect("the tab");
    let tab = tab + vec2(4.0, 6.0);
    let press = |at, pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    window.frame(vec![Event::PointerMoved(tab), press(tab, true)]);
    window.frame(vec![Event::PointerMoved(tab + vec2(30.0, 0.0))]);
    let outside = egui::pos2(1500.0, 500.0);
    window.frame(vec![Event::PointerMoved(outside)]);
    window.frame(vec![press(outside, false)]);
    window.frames(1);
    assert_eq!(window.windows().len(), 1, "the tab left the window");
    let tabs = &window.workbench().groups().groups()[0].tabs.clone();
    assert_eq!(tabs.len(), 1);
}

#[test]
fn a_tab_dragged_from_a_detached_window_onto_the_main_window_docks_it() {
    let mut window = window(&[("a.txt", "alpha\n"), ("b.txt", "beta\n")]);
    window.open("a.txt");
    window.open("b.txt");
    window.run("editor.window.openTab", json!({}));
    let detached = window.windows()[0];
    window.frames(1);
    window.view.main_screen = Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0)));
    let mut input =
        RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..RawInput::default() };
    let inner = Some(Rect::from_min_size(egui::pos2(100.0, 100.0), vec2(800.0, 600.0)));
    input.viewports.entry(egui::ViewportId::ROOT).or_default().inner_rect = inner;
    let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
    let mut output = window.detached.run_ui(input, |ui| {
        view.drop_outside(ui, registry, session, (detached, 0), Some(egui::pos2(50.0, 50.0)));
    });
    output.textures_delta.clear();
    assert!(window.windows().is_empty(), "the last tab docked, so the window closed");
    assert_eq!(window.workbench().groups().groups()[0].tabs.len(), 2);
}

#[test]
fn the_tab_menu_offers_a_new_window() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut screen = crate::offscreen::Offscreen::new(vec2(400.0, 500.0), 1.0);
    let tree = screen.accessibility_tree(|ui| {
        let menu = super::TabMenu {
            group: 1,
            index: 0,
            path: Path::new("/p/a.txt"),
            pins: &[false],
            previews: &[false],
            split: false,
            opposite: false,
            bookmarked: false,
            reopen: false,
        };
        super::tab_menu(ui, &menu);
    });
    let title = zen_i18n::t!("keymap-editor-window-openTab");
    assert_eq!(title, "Open Tab in New Window");
    assert!(crate::accessibility::testing::find(&tree, egui::accesskit::Role::Button, &title).is_some());
}

#[test]
fn detached_windows_come_back_with_the_project() {
    let mut first = window(&[("a.txt", "alpha\n"), ("b.txt", "beta\n")]);
    first.open("a.txt");
    first.open("b.txt");
    let bounds = json!({ "x": 40.0, "y": 50.0, "width": 500.0, "height": 400.0 });
    first.run("editor.window.openTab", json!({ "bounds": bounds }));
    let detached = first.windows()[0];
    let project = first.project.clone();
    let layout = workspace::capture(&first.view, &mut first.session, &project);
    let saved = serde_json::to_string(&layout).unwrap();
    let read: workspace::ProjectLayout = serde_json::from_str(&saved).unwrap();
    let mut second = window_at(&project);
    workspace::restore(&mut second.view, &second.registry, &mut second.session, &read);
    assert_eq!(second.windows(), [detached]);
    let editors = second.view.detached_editors(&second.session);
    assert_eq!(editors[0].title, "b.txt");
    assert_eq!(editors[0].bounds.map(|bounds| bounds.width), Some(500.0));
    assert_eq!(second.workbench().groups().groups()[0].tabs.len(), 1, "a.txt stays in the main window");
    assert_eq!(second.workbench().documents().len(), 2);
}

#[test]
fn a_tab_dropped_in_its_row_takes_the_place_between_its_neighbours() {
    let rect = |left: f32| Rect::from_min_size(Pos2::new(left, 0.0), vec2(90.0, 28.0));
    let row = [(0, rect(0.0)), (1, rect(100.0)), (2, rect(200.0))];
    let place = |dragged: usize, x: f32| super::place_in_row(&row, dragged, Pos2::new(x, 14.0), false);
    assert_eq!(place(0, 280.0), Some(2), "past the last centre: the end");
    assert_eq!(place(0, 160.0), Some(1));
    assert_eq!(place(2, 10.0), Some(0), "before the first centre: the start");
    assert_eq!(place(1, 140.0), Some(1), "its own place");
    assert_eq!(super::place_in_row(&row, 0, Pos2::new(150.0, 90.0), false), None, "off the row");
    let column = [
        (0, Rect::from_min_size(Pos2::ZERO, vec2(200.0, 28.0))),
        (1, Rect::from_min_size(Pos2::new(0.0, 30.0), vec2(200.0, 28.0))),
    ];
    assert_eq!(super::place_in_row(&column, 0, Pos2::new(20.0, 50.0), true), Some(1), "a vertical row goes by height");
}
