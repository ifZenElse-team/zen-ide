use std::path::{Path, PathBuf};

use egui::{Event, PointerButton, Pos2, vec2};
use serde_json::{Value, json};
use zen_core::Session;
use zen_workbench::workspaces::Workspaces;

use super::{Window, at, button, click_at, shows, window};

fn takes_sql(_: &Session, path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "sql") && !path.to_string_lossy().contains("consoles")
}

fn drag(window: &mut Window, from: Pos2, to: Pos2) {
    window.frame_after(0.05, vec![Event::PointerMoved(from), button(from, PointerButton::Primary, true)]);
    for step in 1..=8 {
        let pos = from + (to - from) * (step as f32 / 8.0);
        window.frame_after(0.02, vec![Event::PointerMoved(pos)]);
    }
    window.frame_after(0.02, vec![button(to, PointerButton::Primary, false)]);
    window.frames(2);
}

fn top(texts: &[(Pos2, String)], wanted: &str) -> Pos2 {
    texts.iter().filter(|(_, text)| text == wanted).map(|(at, _)| *at).min_by(|a, b| a.y.total_cmp(&b.y)).unwrap()
}

fn panes(window: &mut Window) -> Vec<(u64, Value)> {
    let state = window.state();
    state["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pane| (pane["group"].as_u64().unwrap(), pane["active"].clone()))
        .collect()
}

fn split_window() -> (Window, PathBuf, Value) {
    let mut window = window();
    let console = window.console("console.sql", "select 1;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    let history = window
        .run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }))["id"]
        .clone();
    window.run("workspace.tab.split", json!({ "workspace": "test", "tab": history, "direction": "right" }));
    (window, console, history)
}

#[test]
fn a_split_workspace_draws_each_group_with_its_own_tabs_and_a_click_moves_the_keyboard() {
    let (mut window, _, _) = split_window();
    let texts = window.frames(3);
    assert!(shows(&texts, "select 1;"), "the console in the left group: {texts:?}");
    assert!(shows(&texts, "View of Query History"), "the feature's tab in the right group");
    let console_tab = at(&texts, "console.sql");
    let history_tab = at(&texts, "Query History");
    assert!(history_tab.x > console_tab.x + 300.0, "side by side");
    assert!((history_tab.y - console_tab.y).abs() < 1.0, "each group has its tab strip on top");
    let [(left, _), (right, _)] = <[_; 2]>::try_from(panes(&mut window)).unwrap();
    assert_eq!(window.workbench().groups().focused(), right);

    let line = at(&texts, "select 1;") + vec2(30.0, 4.0);
    click_at(&mut window, line);
    assert_eq!(window.workbench().groups().focused(), left, "a click in the left group gives it the keyboard");
    assert_eq!(window.state()["expanded"], true);
    assert!(window.view.editor_view.has_focus_in(&window.ctx), "and its editor");

    window.run("editor.split.unsplitAll", json!({}));
    let texts = window.frames(2);
    assert_eq!(panes(&mut window).len(), 1);
    assert!(shows(&texts, "console.sql") && shows(&texts, "Query History"), "one strip with both tabs again");
}

#[test]
fn an_inner_tab_dragged_onto_another_group_moves_there() {
    let (mut window, console, _) = split_window();
    let texts = window.frames(3);
    let from = top(&texts, "console.sql") + vec2(6.0, 6.0);
    let to = at(&texts, "View of Query History") + vec2(0.0, 80.0);
    drag(&mut window, from, to);
    let state = window.state();
    let console_tab =
        state["tabs"].as_array().unwrap().iter().find(|tab| tab["path"] == json!(console)).unwrap().clone();
    assert_eq!(panes(&mut window).len(), 1, "the emptied group goes: {state}");
    assert_eq!(state["tabs"][1]["group"], console_tab["group"]);
}

#[test]
fn scripts_move_between_the_workspace_and_the_editor_by_dragging() {
    let mut window = window();
    window.session.state_mut::<Workspaces>().accept_files("test", takes_sql);
    let script = window.project("report.sql");
    std::fs::write(&script, "select 42;\n").unwrap();
    window.run("file.document.open", json!({ "path": script }));
    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    window.run("workspace.host.collapse", json!({}));
    let texts = window.frames(3);
    let tab = top(&texts, "report.sql") + vec2(6.0, 6.0);
    let workspace = top(&texts, "Test") + vec2(4.0, 6.0);
    drag(&mut window, tab, workspace);
    let state = window.state();
    assert_eq!(state["expanded"], true, "dropped on the workspace's tab, the script goes into it: {state}");
    assert!(state["tabs"].as_array().unwrap().iter().any(|tab| tab["path"] == json!(script)));
    let main = window.run("editor.split.list", json!({}));
    assert!(!main["root"]["tabs"].as_array().unwrap().contains(&json!(script)), "and leaves the code editor");

    let texts = window.frames(2);
    let inner = top(&texts, "report.sql");
    let strip = top(&texts, "other.txt") + vec2(60.0, 6.0);
    drag(&mut window, inner + vec2(6.0, 6.0), strip);
    let state = window.state();
    assert_eq!(state["expanded"], false, "dropped on the editor's strip, it goes back to the code: {state}");
    let main = window.run("editor.split.list", json!({}));
    assert!(main["root"]["tabs"].as_array().unwrap().contains(&json!(script)));

    window.run("workspace.host.expand", json!({ "id": "test" }));
    let texts = window.frames(3);
    let tab = top(&texts, "report.sql") + vec2(6.0, 6.0);
    let body = at(&texts, "View of Query History") + vec2(0.0, 60.0);
    drag(&mut window, tab, body);
    let state = window.state();
    assert!(
        state["tabs"].as_array().unwrap().iter().any(|tab| tab["path"] == json!(script)),
        "dragged from the strip into the host: {state}"
    );
}

#[test]
fn a_console_dragged_to_the_editor_stays_and_says_why() {
    let mut window = window();
    window.session.state_mut::<Workspaces>().accept_files("test", takes_sql);
    let console = window.console("console.sql", "select 1;\n");
    window.run("workspace.tab.open", json!({ "workspace": "test", "path": console }));
    let texts = window.frames(3);
    let inner = top(&texts, "console.sql");
    let strip = top(&texts, "other.txt") + vec2(60.0, 6.0);
    drag(&mut window, inner + vec2(6.0, 6.0), strip);
    let state = window.state();
    assert_eq!(state["expanded"], true);
    assert!(state["tabs"].as_array().unwrap().iter().any(|tab| tab["path"] == json!(console)), "{state}");
}

#[test]
fn snapshots_of_a_split_workspace_in_light_and_dark() {
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let (mut window, console, _) = split_window();
        window.view.set_theme(&theme);
        window.view.palette.apply(&window.ctx);
        let tab = window.state()["tabs"].as_array().unwrap().iter().find(|tab| tab["path"] == json!(console)).unwrap()
            ["id"]
            .clone();
        window.run("workspace.tab.split", json!({ "workspace": "test", "tab": tab, "direction": "down" }));
        let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 700.0), 1.5);
        window.view.palette.apply(screen.context());
        let (view, registry, session) = (&mut window.view, &window.registry, &mut window.session);
        let frame = screen.render(|ui| view.show(ui, registry, session));
        let frame = {
            drop(frame);
            screen.render(|ui| view.show(ui, registry, session))
        };
        let image = frame.image();
        assert!(image.pixels.iter().any(|pixel| *pixel != egui::Color32::TRANSPARENT));
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("workspace-split-{name}.png")), image).unwrap();
        }
    }
}
